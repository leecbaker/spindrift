//! ICC color-space planning for vector PDF paint.
//!
//! CSS Color 4 requires color conversion only at an output boundary:
//! <https://www.w3.org/TR/css-color-4/#color-conversion>. PDF represents
//! calibrated component values with ICCBased color spaces (ISO 32000-2:2020,
//! 8.6.5.5), and associates PDF/A output with an OutputIntent profile.

use crate::css::ColorSpace;
use crate::{Color, PdfCompression, PdfProfile, Result};
use pdf_writer::{Content, Filter, Name, Pdf, Ref};
use std::rc::Rc;

/// The color conversion policy selected by the PDF profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PdfColorMode {
    PreserveCssSpace,
    SrgbOutputIntent,
}

/// A generated ICC profile and the resource name that refers to it.
#[derive(Debug, Clone)]
pub(super) struct PdfIccColorSpace {
    pub(super) space: ColorSpace,
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
        embedded_rgb_profiles: Vec<Rc<[u8]>>,
    ) -> Result<Self> {
        let mode = if profile.is_pdfa() {
            PdfColorMode::SrgbOutputIntent
        } else {
            PdfColorMode::PreserveCssSpace
        };
        // PDF/A converts every CSS paint to its tagged sRGB output condition,
        // so only that color space can be selected by a content stream. Keep
        // the wider set for ordinary PDF, where source color spaces are
        // intentionally preserved.
        let retained_spaces: &[ColorSpace] = match mode {
            PdfColorMode::SrgbOutputIntent => &[ColorSpace::Srgb],
            PdfColorMode::PreserveCssSpace => &[
                ColorSpace::Srgb,
                ColorSpace::DisplayP3,
                ColorSpace::A98Rgb,
                ColorSpace::ProphotoRgb,
                ColorSpace::Rec2020,
                ColorSpace::XyzD50,
            ],
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

    pub(super) fn space(&self, space: ColorSpace) -> &PdfIccColorSpace {
        self.spaces
            .iter()
            .find(|candidate| candidate.space == space)
            .expect("every retained CSS color space has an ICC resource")
    }

    /// Resolve an authored gradient or generated-image space for this PDF
    /// policy. PDF/A has exactly one tagged output condition.
    pub(super) const fn output_space(&self, authored: ColorSpace) -> ColorSpace {
        match self.mode {
            PdfColorMode::PreserveCssSpace => authored,
            PdfColorMode::SrgbOutputIntent => ColorSpace::Srgb,
        }
    }

    pub(super) fn output_color(&self, color: Color) -> Color {
        output_color(color, self.mode)
    }

    pub(super) fn profile_object_id(&self, space: ColorSpace) -> usize {
        self.space(self.output_space(space)).object_id
    }

    /// Resolve an image's retained raster profile under this PDF policy.
    pub(super) fn image_profile_object_id(
        &self,
        color_space: &crate::color::RasterColorSpace,
    ) -> usize {
        match (self.mode, color_space) {
            (PdfColorMode::SrgbOutputIntent, _) => self.space(ColorSpace::Srgb).object_id,
            (PdfColorMode::PreserveCssSpace, crate::color::RasterColorSpace::BuiltIn(space)) => {
                self.space(*space).object_id
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
            profile.n(3).alternate().srgb();
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
        self.space(ColorSpace::Srgb).object_id
    }
}

/// Set one direct PDF fill color in the selected profile-aware color space.
pub(super) fn set_fill_color(content: &mut Content, color: Color, mode: PdfColorMode) {
    let color = output_color(color, mode);
    content
        .set_fill_color_space(Name(color_space_name(color.space())))
        .set_fill_color([color.r, color.g, color.b]);
}

/// Set one direct PDF stroke color in the selected profile-aware color space.
pub(super) fn set_stroke_color(content: &mut Content, color: Color, mode: PdfColorMode) {
    let color = output_color(color, mode);
    content
        .set_stroke_color_space(Name(color_space_name(color.space())))
        .set_stroke_color([color.r, color.g, color.b]);
}

pub(super) fn output_color(color: Color, mode: PdfColorMode) -> Color {
    match mode {
        PdfColorMode::PreserveCssSpace => color,
        // PDF/A vector paint must use the document's tagged sRGB output
        // condition. LittleCMS applies the same ICC transforms used to build
        // the embedded profiles; the CSS conversion is an infallible safety
        // fallback for a failure constructing one of our built-in profiles.
        PdfColorMode::SrgbOutputIntent => crate::color::convert_color(color, ColorSpace::Srgb)
            .map(|color| Color::srgb(color.r, color.g, color.b, color.a))
            .unwrap_or_else(|| crate::css::color_to_srgb(color)),
    }
}

pub(super) const fn color_space_name(space: ColorSpace) -> &'static [u8] {
    match space {
        ColorSpace::Srgb => b"CSsRGB",
        ColorSpace::DisplayP3 => b"CSDisplayP3",
        ColorSpace::A98Rgb => b"CSA98RGB",
        ColorSpace::ProphotoRgb => b"CSProPhoto",
        ColorSpace::Rec2020 => b"CSRec2020",
        ColorSpace::XyzD50 => b"CSXYZD50",
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
    fn lcms_converts_css_display_p3_to_unbounded_srgb() {
        // CSS Color 4's Display-P3-to-sRGB matrices and transfer functions
        // produce approximately (0.98089, 0.01784, -0.09119) for this sample.
        // The transform retains the out-of-gamut component; PDF/A clips only
        // when serializing its tagged sRGB output color space.
        let converted = crate::color::convert_color(
            Color::in_space(ColorSpace::DisplayP3, 0.9, 0.2, 0.1, 1.0),
            ColorSpace::Srgb,
        )
        .expect("built-in CSS Display-P3 profile is transformable");
        assert_eq!(converted.space(), ColorSpace::Srgb);
        assert_close(converted.r, 0.98089);
        assert_close(converted.g, 0.01784);
        assert_close(converted.b, -0.09119);
    }

    #[test]
    fn lcms_d50_xyz_white_maps_to_srgb_white() {
        let converted = crate::color::convert_color(
            Color::in_space(ColorSpace::XyzD50, 0.96422, 1.0, 0.82521, 1.0),
            ColorSpace::Srgb,
        )
        .expect("built-in XYZ D50 profile is transformable");
        assert_close(converted.r, 1.0);
        assert_close(converted.g, 1.0);
        assert_close(converted.b, 1.0);
    }
}
