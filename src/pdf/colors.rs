//! ICC color-space planning for vector PDF paint.
//!
//! CSS CssColor 4 requires color conversion only at an output boundary:
//! <https://www.w3.org/TR/css-color-4/#color-conversion>. PDF represents
//! calibrated component values with ICCBased color spaces (ISO 32000-2:2020,
//! 8.6.5.5), and associates PDF/A output with an OutputIntent profile.

use crate::css::CssColorSpace;
use crate::{CssColor, PdfCompression, PdfProfile, Result};
use pdf_writer::{Content, Filter, Name, Pdf, Ref};
use std::rc::Rc;

/// The color conversion policy selected by the PDF profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PdfColorMode {
    PreserveCssSpace,
    SrgbOutputIntent,
}

/// Final legal RGB paint samples for one PDF graphics operation.
///
/// This is deliberately not a CSS color: it cannot carry D50 PCS coordinates
/// or extended-range components. Construction is private to this module so
/// `PdfColorPlan` remains the CSS-to-PDF conversion authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PdfPaintColor {
    space: CssColorSpace,
    samples: [u8; 3],
    alpha: f32,
}

impl PdfPaintColor {
    fn new(space: CssColorSpace, components: [f32; 3], alpha: f32) -> Self {
        debug_assert!(space != CssColorSpace::XyzD50);
        Self {
            space,
            samples: components.map(|component| (component.clamp(0.0, 1.0) * 255.0).round() as u8),
            alpha,
        }
    }

    pub(super) const fn space(self) -> CssColorSpace {
        self.space
    }

    pub(super) fn components(self) -> [f32; 3] {
        self.samples.map(|sample| sample as f32 / 255.0)
    }

    pub(super) const fn alpha(self) -> f32 {
        self.alpha
    }
}

/// A generated ICC profile and the resource name that refers to it.
#[derive(Debug, Clone)]
pub(super) struct PdfIccColorSpace {
    pub(super) space: CssColorSpace,
    pub(super) name: &'static [u8],
    pub(super) object_id: usize,
    pub(super) bytes: Vec<u8>,
}

/// A source-image RGB profile retained by an ordinary PDF.
#[derive(Debug, Clone)]
struct EmbeddedRgbIccProfile {
    bytes: Rc<[u8]>,
    object_id: usize,
}

/// Per-document ICC resources for all direct vector paints.
///
/// The generated resources follow PDF's ICCBased color-space model (ISO
/// 32000-2:2020, 8.6.5.5); `PdfProfile::Pdf` preserves the CSS source space,
/// while PDF/A uses its tagged sRGB output intent.
#[derive(Debug, Clone)]
pub(super) struct PdfColorPlan {
    mode: PdfColorMode,
    spaces: Vec<PdfIccColorSpace>,
    embedded_rgb_profiles: Vec<EmbeddedRgbIccProfile>,
}

impl PdfColorPlan {
    pub(super) fn new(
        profile: PdfProfile,
        first_object_id: usize,
        used_css_spaces: impl IntoIterator<Item = CssColorSpace>,
        embedded_rgb_profiles: Vec<Rc<[u8]>>,
    ) -> Result<Self> {
        let mode = if profile.is_pdfa() {
            PdfColorMode::SrgbOutputIntent
        } else {
            PdfColorMode::PreserveCssSpace
        };
        // PDF/A converts every CSS paint to its tagged sRGB output condition.
        // Ordinary PDF instead plans only the authored spaces actually used by
        // vector paint or raster images, preserving CSS CssColor 4 semantics
        // without attaching unused profiles to every page.
        let used_css_spaces = used_css_spaces
            .into_iter()
            .map(ordinary_pdf_output_space)
            .collect::<std::collections::HashSet<_>>();
        let retained_spaces = match mode {
            PdfColorMode::SrgbOutputIntent => vec![CssColorSpace::Srgb],
            PdfColorMode::PreserveCssSpace => [
                CssColorSpace::Srgb,
                CssColorSpace::DisplayP3,
                CssColorSpace::A98Rgb,
                CssColorSpace::ProphotoRgb,
                CssColorSpace::Rec2020,
            ]
            .into_iter()
            // Raster glyph fallbacks and late image materialization can add
            // sRGB samples after document-level resource planning. Keep the
            // universal CSS default available while continuing to plan wider
            // authored spaces lazily.
            .filter(|space| {
                matches!(space, CssColorSpace::Srgb | CssColorSpace::DisplayP3)
                    || used_css_spaces.contains(space)
            })
            .collect(),
        };
        let spaces = retained_spaces
            .iter()
            .copied()
            .enumerate()
            .map(|(index, space)| {
                Ok(PdfIccColorSpace {
                    space,
                    name: color_space_name(space),
                    object_id: first_object_id + index,
                    bytes: crate::color::icc_profile_bytes(space)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let embedded_rgb_profiles = if mode == PdfColorMode::PreserveCssSpace {
            embedded_rgb_profiles
                .into_iter()
                .enumerate()
                .map(|(index, bytes)| EmbeddedRgbIccProfile {
                    bytes,
                    object_id: first_object_id + spaces.len() + index,
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            mode,
            spaces,
            embedded_rgb_profiles,
        })
    }

    pub(super) const fn mode(&self) -> PdfColorMode {
        self.mode
    }

    pub(super) fn object_count(&self) -> usize {
        self.spaces.len() + self.embedded_rgb_profiles.len()
    }

    pub(super) fn space(&self, space: CssColorSpace) -> &PdfIccColorSpace {
        self.spaces
            .iter()
            .find(|candidate| candidate.space == space)
            .unwrap_or_else(|| panic!("CSS color space {space:?} was not planned for this PDF"))
    }

    /// Resolve an authored gradient or generated-image space for this PDF
    /// policy. PDF/A has exactly one tagged output condition.
    pub(super) const fn output_space(&self, authored: CssColorSpace) -> CssColorSpace {
        match self.mode {
            PdfColorMode::PreserveCssSpace => ordinary_pdf_output_space(authored),
            PdfColorMode::SrgbOutputIntent => CssColorSpace::Srgb,
        }
    }

    pub(super) fn output_color(&self, color: CssColor) -> PdfPaintColor {
        pdf_paint_color(color, self.mode)
    }

    pub(super) fn profile_object_id(&self, space: CssColorSpace) -> usize {
        self.space(self.output_space(space)).object_id
    }

    /// Resolve an image's retained raster profile under this PDF policy.
    pub(super) fn image_profile_object_id(
        &self,
        color_space: &crate::color::RasterColorSpace,
    ) -> usize {
        match (self.mode, color_space) {
            (PdfColorMode::SrgbOutputIntent, _) => self.space(CssColorSpace::Srgb).object_id,
            (PdfColorMode::PreserveCssSpace, crate::color::RasterColorSpace::BuiltIn(space)) => {
                self.space(ordinary_pdf_output_space(*space)).object_id
            }
            (
                PdfColorMode::PreserveCssSpace,
                crate::color::RasterColorSpace::EmbeddedRgb(bytes),
            ) => {
                self.embedded_rgb_profiles
                    .iter()
                    .find(|profile| profile.bytes.as_ref() == bytes.as_ref())
                    .expect("every retained image profile is planned before PDF object allocation")
                    .object_id
            }
        }
    }

    pub(super) fn write_profiles(&self, pdf: &mut Pdf, compression: PdfCompression) {
        for space in &self.spaces {
            let stream = super::encode_pdf_stream(compression, &space.bytes);
            let mut profile = pdf.icc_profile(Ref::new(space.object_id as i32), stream.bytes());
            profile.n(3);
            match space.space {
                // The ICC alternate is used only by readers without ICC
                // support, but must still describe the same components.
                // Labeling Display-P3 coordinates as sRGB subtly shifts every
                // PCS-derived color in those readers.
                CssColorSpace::DisplayP3 => profile.alternate().display_p3(),
                _ => profile.alternate().srgb(),
            }
            profile.range([0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
            if stream.uses_flate() {
                profile.filter(Filter::FlateDecode);
            }
        }
        for profile in &self.embedded_rgb_profiles {
            let stream = super::encode_pdf_stream(compression, &profile.bytes);
            let mut writer = pdf.icc_profile(Ref::new(profile.object_id as i32), stream.bytes());
            writer.n(3).alternate().srgb();
            writer.range([0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
            if stream.uses_flate() {
                writer.filter(Filter::FlateDecode);
            }
        }
    }

    pub(super) fn write_page_resources(&self, resources: &mut pdf_writer::writers::Resources<'_>) {
        let mut color_spaces = resources.color_spaces();
        for space in &self.spaces {
            color_spaces
                .insert(Name(space.name))
                .start::<pdf_writer::writers::ColorSpace>()
                .icc_based(Ref::new(space.object_id as i32));
        }
    }

    pub(super) fn srgb_profile_object_id(&self) -> usize {
        self.space(CssColorSpace::Srgb).object_id
    }
}

/// Set one direct PDF fill color in the selected profile-aware color space.
pub(super) fn set_fill_color(content: &mut Content, color: CssColor, mode: PdfColorMode) {
    let color = pdf_paint_color(color, mode);
    content
        .set_fill_color_space(Name(color_space_name(color.space())))
        .set_fill_color([
            color.components()[0],
            color.components()[1],
            color.components()[2],
        ]);
}

/// Set one direct PDF stroke color in the selected profile-aware color space.
pub(super) fn set_stroke_color(content: &mut Content, color: CssColor, mode: PdfColorMode) {
    let color = pdf_paint_color(color, mode);
    content
        .set_stroke_color_space(Name(color_space_name(color.space())))
        .set_stroke_color([
            color.components()[0],
            color.components()[1],
            color.components()[2],
        ]);
}

pub(super) fn output_color(color: CssColor, mode: PdfColorMode) -> CssColor {
    match mode {
        // sRGB is the interoperable ordinary-PDF encoding when a CSS color is
        // representable there, regardless of the space in which the author
        // expressed it. P3 is the next preferred ordinary-PDF condition. This
        // is an output conversion, not CSS gamut mapping: conversion retains
        // extended-range components and only selects a space when all of them
        // already fit. Quantizing the final legal samples makes equivalent
        // colors meet without PDF-rasterizer seams.
        PdfColorMode::PreserveCssSpace => {
            let srgb = crate::css::color_to_predefined_rgb(color, CssColorSpace::Srgb)
                .expect("sRGB is a built-in CSS output space");
            if rgb_coordinates_are_in_unit_gamut(srgb) {
                quantized_rgb_pdf_color(srgb)
            } else {
                let p3 = crate::css::color_to_predefined_rgb(color, CssColorSpace::DisplayP3)
                    .expect("Display-P3 is a built-in CSS output space");
                // D50 XYZ is Quire's PCS, not an ordinary PDF paint space.
                // P3 is likewise the fixed ordinary-PDF output condition for
                // every color that cannot use sRGB. Its final components are
                // clipped only here, never through an sRGB detour. Emitting
                // one wide-gamut condition also keeps equivalent Lab and
                // authored-predefined colors as one PDF paint primitive.
                quantized_rgb_pdf_color(p3)
            }
        }
        // PDF/A vector paint must use the document's tagged sRGB output
        // condition. moxcms applies the same ICC transforms used to build
        // the embedded profiles; the CSS conversion is an infallible safety
        // fallback for a failure constructing one of our built-in profiles.
        PdfColorMode::SrgbOutputIntent => crate::color::convert_color(color, CssColorSpace::Srgb)
            .or_else(|| crate::css::color_to_predefined_rgb(color, CssColorSpace::Srgb))
            .expect("sRGB is a built-in CSS output space"),
    }
}

fn pdf_paint_color(color: CssColor, mode: PdfColorMode) -> PdfPaintColor {
    let color = output_color(color, mode);
    PdfPaintColor::new(color.space(), color.components(), color.alpha())
}

/// Whether an encoded RGB paint can be represented without output clipping.
///
/// CSS predefined-RGB conversion retains extended-range components. This
/// check is deliberately after conversion and before PDF serialization, so a
/// PCS color only takes the sRGB path when every component is already legal.
const fn rgb_coordinates_are_in_unit_gamut(color: CssColor) -> bool {
    // Conversion of a neutral PCS value can land a few ULPs outside the unit
    // cube. Treat no more than one 8-bit PDF sample as numerical noise, then
    // clamp at the actual output boundary. Clearly wide-gamut values still
    // take the Display-P3 route unchanged.
    const EPSILON: f32 = 1.0 / 255.0;
    color.components()[0] >= -EPSILON
        && color.components()[0] <= 1.0 + EPSILON
        && color.components()[1] >= -EPSILON
        && color.components()[1] <= 1.0 + EPSILON
        && color.components()[2] >= -EPSILON
        && color.components()[2] <= 1.0 + EPSILON
}

/// Match the legacy CSS RGB paint representation used by Quire's PDF path.
///
/// An `rgb()` percentage is stored as an 8-bit sample today. Quantizing an
/// equivalently in-gamut PCS conversion at the same paint boundary prevents
/// seams where two CSS-equivalent colors meet, while alpha remains continuous.
fn quantized_rgb_pdf_color(color: CssColor) -> CssColor {
    let sample = |component: f32| (component.clamp(0.0, 1.0) * 255.0).round() / 255.0;
    CssColor::in_space(
        color.space(),
        sample(color.components()[0]),
        sample(color.components()[1]),
        sample(color.components()[2]),
        color.alpha(),
    )
}

/// PDF's ordinary output condition for an authored CSS component space.
///
/// D50 XYZ is an internal profile-connection space. CSS CssColor 4 values
/// produced from Lab, LCH, OKLab, OKLCH, `xyz*`, and PCS interpolation reserve
/// Display-P3 as their wide-gamut ordinary-PDF fallback rather than emitting
/// the PCS directly.
const fn ordinary_pdf_output_space(authored: CssColorSpace) -> CssColorSpace {
    match authored {
        CssColorSpace::XyzD50 => CssColorSpace::DisplayP3,
        rgb => rgb,
    }
}

pub(super) const fn color_space_name(space: CssColorSpace) -> &'static [u8] {
    match space {
        CssColorSpace::Srgb => b"CSsRGB",
        CssColorSpace::DisplayP3 => b"CSDisplayP3",
        CssColorSpace::A98Rgb => b"CSA98RGB",
        CssColorSpace::ProphotoRgb => b"CSProPhoto",
        CssColorSpace::Rec2020 => b"CSRec2020",
        CssColorSpace::XyzD50 => b"CSXYZD50",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.003,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn css_converts_display_p3_to_unbounded_srgb() {
        // CSS CssColor 4's Display-P3-to-sRGB matrices and transfer functions
        // produce approximately (0.98089, 0.01784, -0.07893) for this sample.
        // The transform retains the out-of-gamut component; PDF/A clips only
        // when serializing its tagged sRGB output color space.
        let converted = crate::color::convert_color(
            CssColor::in_space(CssColorSpace::DisplayP3, 0.9, 0.2, 0.1, 1.0),
            CssColorSpace::Srgb,
        )
        .expect("built-in CSS Display-P3 profile is transformable");
        assert_eq!(converted.space(), CssColorSpace::Srgb);
        assert_close(converted.components()[0], 0.98089);
        assert_close(converted.components()[1], 0.01784);
        assert_close(converted.components()[2], -0.07893);
    }

    #[test]
    fn d50_xyz_white_maps_to_srgb_white() {
        let converted = crate::color::convert_color(
            CssColor::in_space(CssColorSpace::XyzD50, 0.96422, 1.0, 0.82521, 1.0),
            CssColorSpace::Srgb,
        )
        .expect("D50 XYZ conversion is supported");
        assert_close(converted.components()[0], 1.0);
        assert_close(converted.components()[1], 1.0);
        assert_close(converted.components()[2], 1.0);
    }

    #[test]
    fn in_gamut_srgb_paint_uses_the_legacy_eight_bit_samples() {
        let color = quantized_rgb_pdf_color(CssColor::srgb(0.756_208, 0.304_487, 0.475_634, 0.6));

        assert_eq!(
            color,
            CssColor::srgb(193.0 / 255.0, 78.0 / 255.0, 121.0 / 255.0, 0.6)
        );
    }

    #[test]
    fn d50_pcs_preserves_display_p3_green_without_srgb_clipping() {
        let p3_green = CssColor::rgb(crate::css::RgbColorSpace::DisplayP3, 0.0, 1.0, 0.0, 1.0);
        let pcs = crate::css::color_to_xyz_d50(p3_green);
        let output = output_color(pcs, PdfColorMode::PreserveCssSpace);

        assert_eq!(output.space(), CssColorSpace::DisplayP3);
        assert_close(output.components()[0], 0.0);
        assert_close(output.components()[1], 1.0);
        assert_close(output.components()[2], 0.0);

        let srgb = crate::color::convert_color(pcs, CssColorSpace::Srgb)
            .expect("sRGB is a built-in CSS output space");
        assert!(
            srgb.components()[0] < 0.0 || srgb.components()[1] > 1.0 || srgb.components()[2] < 0.0
        );
    }

    #[test]
    fn ordinary_pdf_canonicalizes_an_in_gamut_authored_p3_paint_to_srgb() {
        // CSS CssColor's sample is the Display-P3 encoding of sRGB #008000.
        // The output conversion must not retain an approximate PDF fallback
        // profile when the color is already exactly representable in sRGB.
        let output = output_color(
            CssColor::rgb(
                crate::css::RgbColorSpace::DisplayP3,
                0.216_04,
                0.494_18,
                0.131_51,
                1.0,
            ),
            PdfColorMode::PreserveCssSpace,
        );

        assert_eq!(output, CssColor::srgb(0.0, 128.0 / 255.0, 0.0, 1.0));
    }

    #[test]
    fn ordinary_pdf_retargets_pcs_resources_to_display_p3() {
        let plan = PdfColorPlan::new(PdfProfile::Pdf, 1, [CssColorSpace::XyzD50], Vec::new())
            .expect("built-in Display-P3 ICC profile is available");
        assert_eq!(
            plan.output_space(CssColorSpace::XyzD50),
            CssColorSpace::DisplayP3
        );
        assert!(
            plan.spaces
                .iter()
                .all(|space| space.space != CssColorSpace::XyzD50)
        );
    }
}
