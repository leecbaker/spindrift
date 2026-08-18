//! Shared CSS CssColor 4 conversion and ICC-profile support.
//!
//! CSS colors are converted only at a concrete output boundary. The helpers
//! here use the built-in CSS CssColor 4 RGB spaces and D50 PCS:
//! <https://www.w3.org/TR/css-color-4/#color-conversion>.

use crate::css::CssColorSpace;
use crate::{CssColor, Error, Result};
use moxcms::{
    Chromaticity, ColorPrimaries, ColorProfile, DataColorSpace, Layout, RenderingIntent,
    ToneReprCurve, TransformOptions, XyY,
};
use palette::{
    Lab, Oklab, Xyz,
    convert::FromColorUnclamped,
    white_point::{D50, D65},
};
use std::rc::Rc;

type GradientCoordinates = ([f64; 3], f64);
type GradientPair = (GradientCoordinates, GradientCoordinates, CssColorSpace);

/// The calibrated component space carried by decoded raster samples.
///
/// CSS CssColor 4 defers conversion until an output boundary. Raster images need
/// the same distinction: source RGB samples may carry an embedded ICC profile,
/// while generated samples use one of Quire's built-in CSS spaces.
/// <https://www.w3.org/TR/css-color-4/#color-conversion>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum RasterColorSpace {
    BuiltIn(CssColorSpace),
    EmbeddedRgb(Rc<[u8]>),
}

impl RasterColorSpace {
    pub(crate) const SRGB: Self = Self::BuiltIn(CssColorSpace::Srgb);
}

/// Validate and retain an embedded RGB ICC profile from a decoded image.
///
/// PDF image XObjects use the profile's component count to interpret samples
/// (ISO 32000-2:2020, 8.9.5.5). This milestone accepts only RGB profiles,
/// because Quire's PNG/JPEG raster path materializes three RGB components.
pub(crate) fn embedded_rgb_profile(bytes: Vec<u8>) -> Option<RasterColorSpace> {
    let profile = ColorProfile::new_from_slice(&bytes).ok()?;
    if profile.color_space != DataColorSpace::Rgb {
        return None;
    }
    // A syntactically valid profile can still lack the transform tables needed
    // for a usable source-to-output conversion. Validate that boundary once at
    // registration so PDF/A cannot silently label unconverted samples as sRGB.
    profile
        .create_transform_8bit(
            Layout::Rgb,
            &ColorProfile::new_srgb(),
            Layout::Rgb,
            transform_options(),
        )
        .ok()?;
    Some(RasterColorSpace::EmbeddedRgb(Rc::from(
        bytes.into_boxed_slice(),
    )))
}

/// Construct an RGB ICC profile from PNG's `gAMA` and `cHRM` declarations.
///
/// PNG stores chromaticity coordinates and the reciprocal of the samples'
/// decoding exponent. ICC parametric curve type 0 expresses that decoding
/// exponent directly, so this is a lossless representation of this PNG color
/// encoding at Quire's raster/PDF boundary.
/// <https://www.w3.org/TR/png-3/#11gAMA>
/// <https://www.w3.org/TR/png-3/#11cHRM>
pub(crate) fn png_gamma_chromaticities_profile(
    encoded_gamma: f64,
    chromaticities: PngChromaticities,
) -> Option<RasterColorSpace> {
    if !encoded_gamma.is_finite() || encoded_gamma <= 0.0 {
        return None;
    }
    let coordinates = [
        chromaticities.white_x,
        chromaticities.white_y,
        chromaticities.red_x,
        chromaticities.red_y,
        chromaticities.green_x,
        chromaticities.green_y,
        chromaticities.blue_x,
        chromaticities.blue_y,
    ];
    if coordinates
        .iter()
        .any(|coordinate| !coordinate.is_finite() || !(0.0..=1.0).contains(coordinate))
        || chromaticities.white_y == 0.0
        || chromaticities.red_y == 0.0
        || chromaticities.green_y == 0.0
        || chromaticities.blue_y == 0.0
    {
        return None;
    }

    let white_point = XyY::new(chromaticities.white_x, chromaticities.white_y, 1.0);
    let mut profile = ColorProfile::new_srgb();
    profile.update_rgb_colorimetry(
        white_point,
        ColorPrimaries {
            red: Chromaticity {
                x: chromaticities.red_x as f32,
                y: chromaticities.red_y as f32,
            },
            green: Chromaticity {
                x: chromaticities.green_x as f32,
                y: chromaticities.green_y as f32,
            },
            blue: Chromaticity {
                x: chromaticities.blue_x as f32,
                y: chromaticities.blue_y as f32,
            },
        },
    );
    profile.media_white_point = Some(white_point.to_xyzd());
    profile = profile_with_curve(
        profile,
        ToneReprCurve::Parametric(vec![(1.0 / encoded_gamma) as f32]),
    );
    embedded_rgb_profile(profile.encode().ok()?)
}

/// Chromaticities stored by PNG's `cHRM` chunk, normalized to CIE xy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PngChromaticities {
    pub(crate) white_x: f64,
    pub(crate) white_y: f64,
    pub(crate) red_x: f64,
    pub(crate) red_y: f64,
    pub(crate) green_x: f64,
    pub(crate) green_y: f64,
    pub(crate) blue_x: f64,
    pub(crate) blue_y: f64,
}

/// Convert one unpremultiplied CSS color between built-in component spaces.
pub(crate) fn convert_color(color: CssColor, target: CssColorSpace) -> Option<CssColor> {
    if color.space() == target {
        return Some(color);
    }
    if target == CssColorSpace::XyzD50 {
        return Some(crate::css::color_to_xyz_d50(color));
    }
    // CSS CssColor's predefined spaces are defined by exact matrices and transfer
    // functions, not ICC perceptual transforms. Keeping this path local also
    // preserves extended-range values between two built-in spaces. moxcms is
    // intentionally reserved for embedded/profile-generated raster ICC work.
    crate::css::color_to_predefined_rgb(color, target)
}

/// Apply a filter color transform in the sRGB space mandated for CSS Filter
/// Functions, before the PDF output profile selects its own representation.
///
/// The transform is deliberately restricted to the bounded linear subset so
/// this operation preserves alpha and can be distributed through normal
/// source-over descendants of an isolated filter group.
/// <https://www.w3.org/TR/filter-effects-1/#filter-functions>
pub(crate) fn apply_bounded_srgb_filter_transform(
    color: CssColor,
    transform: crate::css::BoundedSrgbColorTransform,
) -> CssColor {
    let color = convert_color(color, CssColorSpace::Srgb)
        .or_else(|| crate::css::color_to_predefined_rgb(color, CssColorSpace::Srgb))
        .expect("sRGB is a built-in CSS color space");
    let [red, green, blue] = transform.apply(color.components());
    CssColor::in_space(CssColorSpace::Srgb, red, green, blue, color.alpha())
}

/// Applies CSS CssColor's missing-component replacement in the selected
/// interpolation coordinates, then uses premultiplied component interpolation.
pub(crate) fn interpolate_color_with_missing(
    start: CssColor,
    end: CssColor,
    method: crate::css::GradientInterpolationMethod,
    progress: f32,
    mut start_missing: u8,
    mut end_missing: u8,
) -> CssColor {
    use crate::css::GradientInterpolationSpace as Space;
    let t = progress.clamp(0.0, 1.0) as f64;
    let (start, end, output) = match method.space {
        Space::Srgb => gradient_rgb_pair(start, end, CssColorSpace::Srgb, false),
        Space::DisplayP3 => gradient_rgb_pair(start, end, CssColorSpace::DisplayP3, false),
        Space::A98Rgb => gradient_rgb_pair(start, end, CssColorSpace::A98Rgb, false),
        Space::ProphotoRgb => gradient_rgb_pair(start, end, CssColorSpace::ProphotoRgb, false),
        Space::Rec2020 => gradient_rgb_pair(start, end, CssColorSpace::Rec2020, false),
        Space::SrgbLinear => gradient_rgb_pair(start, end, CssColorSpace::Srgb, true),
        Space::DisplayP3Linear => gradient_rgb_pair(start, end, CssColorSpace::DisplayP3, true),
        Space::XyzD50 => gradient_xyz_pair(start, end, false),
        Space::XyzD65 => gradient_xyz_pair(start, end, true),
        Space::Lab => gradient_lab_pair(start, end, false),
        Space::Lch => gradient_lab_pair(start, end, true),
        Space::Oklab => gradient_oklab_pair(start, end, false),
        Space::Oklch => gradient_oklab_pair(start, end, true),
        Space::Hsl => gradient_hsl_pair(start, end, false),
        Space::Hwb => gradient_hsl_pair(start, end, true),
    };
    let mut start_components = start.0;
    let mut end_components = end.0;
    // In polar spaces hue is powerless at zero chroma, and missing when the
    // color is fully transparent. CSS CssColor treats that hue as missing before
    // it selects the interpolation arc.
    if method.is_polar() {
        if start.1 <= 0.0 || start_components[1].abs() <= f64::EPSILON {
            start_missing |= 1 << 2;
        }
        if end.1 <= 0.0 || end_components[1].abs() <= f64::EPSILON {
            end_missing |= 1 << 2;
        }
    }
    for component in 0..3 {
        if start_missing & (1 << component) != 0 {
            start_components[component] = end_components[component];
        }
        if end_missing & (1 << component) != 0 {
            end_components[component] = start.0[component];
        }
    }
    let start_alpha = if start_missing & (1 << 3) != 0 {
        end.1
    } else {
        start.1
    };
    let end_alpha = if end_missing & (1 << 3) != 0 {
        start.1
    } else {
        end.1
    };
    let alpha = start_alpha + (end_alpha - start_alpha) * t;
    let premultiply = |components: [f64; 3], alpha: f64, polar: bool| {
        if polar {
            [components[0] * alpha, components[1] * alpha, components[2]]
        } else {
            components.map(|component| component * alpha)
        }
    };
    let polar = method.is_polar();
    let start_premultiplied = premultiply(start_components, start_alpha, polar);
    if polar {
        end_components[2] =
            interpolate_hue_endpoint(start_components[2], end_components[2], method.hue);
    }
    let end_premultiplied = premultiply(end_components, end_alpha, polar);
    let mut components = std::array::from_fn(|index| {
        start_premultiplied[index] + (end_premultiplied[index] - start_premultiplied[index]) * t
    });
    if alpha > 0.0 {
        if polar {
            components[0] /= alpha;
            components[1] /= alpha;
        } else {
            components = components.map(|component| component / alpha);
        }
    }
    gradient_components_to_color(method.space, components, alpha as f32, output)
}

fn gradient_rgb_pair(
    start: CssColor,
    end: CssColor,
    space: CssColorSpace,
    linear: bool,
) -> GradientPair {
    let components = |color: CssColor| {
        let color = convert_color(color, space).unwrap_or_else(|| {
            crate::css::color_to_predefined_rgb(color, space)
                .expect("gradient interpolation space is a predefined CSS RGB space")
        });
        let components = [
            color.components()[0] as f64,
            color.components()[1] as f64,
            color.components()[2] as f64,
        ];
        let components = if linear {
            components.map(srgb_to_linear_component)
        } else {
            components
        };
        (components, color.alpha() as f64)
    };
    (components(start), components(end), space)
}

fn gradient_xyz_pair(start: CssColor, end: CssColor, d65: bool) -> GradientPair {
    let components = |color: CssColor| {
        let color = crate::css::color_to_xyz_d50(color);
        let components = [
            color.components()[0] as f64,
            color.components()[1] as f64,
            color.components()[2] as f64,
        ];
        (
            if d65 {
                adapt_d50_to_d65(components)
            } else {
                components
            },
            color.alpha() as f64,
        )
    };
    (components(start), components(end), CssColorSpace::XyzD50)
}

fn gradient_lab_pair(start: CssColor, end: CssColor, polar: bool) -> GradientPair {
    let components = |color: CssColor| {
        let xyz = crate::css::color_to_xyz_d50(color);
        let [lightness, a, b] = xyz_d50_to_lab([
            xyz.components()[0] as f64,
            xyz.components()[1] as f64,
            xyz.components()[2] as f64,
        ]);
        let components = if polar {
            [
                lightness,
                a.hypot(b),
                b.atan2(a).to_degrees().rem_euclid(360.0),
            ]
        } else {
            [lightness, a, b]
        };
        (components, xyz.alpha() as f64)
    };
    (components(start), components(end), CssColorSpace::XyzD50)
}

fn gradient_oklab_pair(start: CssColor, end: CssColor, polar: bool) -> GradientPair {
    let components = |color: CssColor| {
        let xyz = crate::css::color_to_xyz_d50(color);
        let [lightness, a, b] = xyz_d65_to_oklab(adapt_d50_to_d65([
            xyz.components()[0] as f64,
            xyz.components()[1] as f64,
            xyz.components()[2] as f64,
        ]));
        let components = if polar {
            [
                lightness,
                a.hypot(b),
                b.atan2(a).to_degrees().rem_euclid(360.0),
            ]
        } else {
            [lightness, a, b]
        };
        (components, xyz.alpha() as f64)
    };
    (components(start), components(end), CssColorSpace::XyzD50)
}

fn gradient_hsl_pair(start: CssColor, end: CssColor, hwb: bool) -> GradientPair {
    let components = |color: CssColor| {
        let color = convert_color(color, CssColorSpace::Srgb).unwrap_or_else(|| {
            crate::css::color_to_predefined_rgb(color, CssColorSpace::Srgb)
                .expect("sRGB is a predefined CSS RGB space")
        });
        let [r, g, b] = [
            color.components()[0] as f64,
            color.components()[1] as f64,
            color.components()[2] as f64,
        ];
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let chroma = max - min;
        let hue = if chroma == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / chroma).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / chroma + 2.0)
        } else {
            60.0 * ((r - g) / chroma + 4.0)
        };
        let components = if hwb {
            // The shared polar interpolation program uses
            // [non-hue, non-hue, hue] for every polar space.
            [min, 1.0 - max, hue]
        } else {
            let lightness = (max + min) / 2.0;
            let saturation = if chroma == 0.0 {
                0.0
            } else {
                chroma / (1.0 - (2.0 * lightness - 1.0).abs())
            };
            [lightness, saturation, hue]
        };
        (components, color.alpha() as f64)
    };
    (components(start), components(end), CssColorSpace::Srgb)
}

fn gradient_components_to_color(
    space: crate::css::GradientInterpolationSpace,
    components: [f64; 3],
    alpha: f32,
    output: CssColorSpace,
) -> CssColor {
    use crate::css::GradientInterpolationSpace as Space;
    match space {
        Space::Srgb | Space::DisplayP3 | Space::A98Rgb | Space::ProphotoRgb | Space::Rec2020 => {
            CssColor::in_space(
                output,
                components[0] as f32,
                components[1] as f32,
                components[2] as f32,
                alpha,
            )
        }
        Space::SrgbLinear | Space::DisplayP3Linear => CssColor::in_space(
            output,
            linear_to_srgb_component(components[0]) as f32,
            linear_to_srgb_component(components[1]) as f32,
            linear_to_srgb_component(components[2]) as f32,
            alpha,
        ),
        Space::XyzD50 => CssColor::in_space(
            CssColorSpace::XyzD50,
            components[0] as f32,
            components[1] as f32,
            components[2] as f32,
            alpha,
        ),
        Space::XyzD65 => {
            let xyz = adapt_d65_to_d50(components);
            CssColor::in_space(
                CssColorSpace::XyzD50,
                xyz[0] as f32,
                xyz[1] as f32,
                xyz[2] as f32,
                alpha,
            )
        }
        Space::Lab => {
            let xyz = lab_to_xyz_d50(components[0], components[1], components[2]);
            CssColor::in_space(
                CssColorSpace::XyzD50,
                xyz[0] as f32,
                xyz[1] as f32,
                xyz[2] as f32,
                alpha,
            )
        }
        Space::Lch => {
            let hue = components[2].to_radians();
            let xyz = lab_to_xyz_d50(
                components[0],
                components[1] * hue.cos(),
                components[1] * hue.sin(),
            );
            CssColor::in_space(
                CssColorSpace::XyzD50,
                xyz[0] as f32,
                xyz[1] as f32,
                xyz[2] as f32,
                alpha,
            )
        }
        Space::Oklab | Space::Oklch => {
            let [lightness, a, b] = if matches!(space, Space::Oklch) {
                let hue = components[2].to_radians();
                [
                    components[0],
                    components[1] * hue.cos(),
                    components[1] * hue.sin(),
                ]
            } else {
                components
            };
            let xyz = adapt_d65_to_d50(oklab_to_xyz_d65([lightness, a, b]));
            CssColor::in_space(
                CssColorSpace::XyzD50,
                xyz[0] as f32,
                xyz[1] as f32,
                xyz[2] as f32,
                alpha,
            )
        }
        Space::Hsl => {
            let rgb = hsl_to_rgb(components[2], components[1], components[0]);
            CssColor::in_space(
                CssColorSpace::Srgb,
                rgb[0] as f32,
                rgb[1] as f32,
                rgb[2] as f32,
                alpha,
            )
        }
        Space::Hwb => {
            let rgb = hwb_to_rgb(components[2], components[0], components[1]);
            CssColor::in_space(
                CssColorSpace::Srgb,
                rgb[0] as f32,
                rgb[1] as f32,
                rgb[2] as f32,
                alpha,
            )
        }
    }
}

fn interpolate_hue_endpoint(
    start: f64,
    end: f64,
    method: crate::css::HueInterpolationMethod,
) -> f64 {
    use crate::css::HueInterpolationMethod::*;
    let start = start.rem_euclid(360.0);
    let end = end.rem_euclid(360.0);
    let delta = match method {
        Shorter => (end - start + 180.0).rem_euclid(360.0) - 180.0,
        Longer => {
            let short = (end - start + 180.0).rem_euclid(360.0) - 180.0;
            if short >= 0.0 {
                short - 360.0
            } else {
                short + 360.0
            }
        }
        Increasing => (end - start).rem_euclid(360.0),
        Decreasing => -((start - end).rem_euclid(360.0)),
    };
    start + delta
}

fn srgb_to_linear_component(value: f64) -> f64 {
    let sign = value.signum();
    let value = value.abs();
    sign * if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_component(value: f64) -> f64 {
    let sign = value.signum();
    let value = value.abs();
    sign * if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn xyz_d65_to_oklab(xyz: [f64; 3]) -> [f64; 3] {
    // Palette owns the standard D65 XYZ ↔ OKLab transform. Its unchecked
    // conversion uses signed cube roots, so CSS extended-range coordinates
    // remain representable until the eventual output boundary.
    let oklab = Oklab::from_color_unclamped(Xyz::<D65, f64>::new(xyz[0], xyz[1], xyz[2]));
    [oklab.l, oklab.a, oklab.b]
}

fn oklab_to_xyz_d65(oklab: [f64; 3]) -> [f64; 3] {
    let xyz: Xyz<D65, f64> = Xyz::from_color_unclamped(Oklab::new(oklab[0], oklab[1], oklab[2]));
    [xyz.x, xyz.y, xyz.z]
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [f64; 3] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (hue.rem_euclid(360.0) / 60.0).floor() as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let offset = lightness - chroma / 2.0;
    [r + offset, g + offset, b + offset]
}

fn hwb_to_rgb(hue: f64, whiteness: f64, blackness: f64) -> [f64; 3] {
    let sum = whiteness + blackness;
    if sum >= 1.0 {
        return [whiteness / sum; 3];
    }
    let base = hsl_to_rgb(hue, 1.0, 0.5);
    base.map(|component| component * (1.0 - whiteness - blackness) + whiteness)
}

fn adapt_d50_to_d65(xyz: [f64; 3]) -> [f64; 3] {
    matrix(
        [
            [
                0.955_473_421_488_075,
                -0.023_098_454_948_764_71,
                0.063_259_308_661_021_7,
            ],
            [
                -0.028_369_709_333_863_7,
                1.009_995_398_081_304_1,
                0.021_041_441_191_917_323,
            ],
            [
                0.012_314_014_864_481_998,
                -0.020_507_649_298_898_964,
                1.330_365_926_242_124,
            ],
        ],
        xyz,
    )
}

fn adapt_d65_to_d50(xyz: [f64; 3]) -> [f64; 3] {
    matrix(
        [
            [
                1.047_929_820_840_548_8,
                0.022_946_793_341_019_088,
                -0.050_192_229_543_135_57,
            ],
            [
                0.029_627_815_688_159_344,
                0.990_434_484_573_249,
                -0.017_073_825_029_385_14,
            ],
            [
                -0.009_243_058_152_591_178,
                0.015_055_144_896_577_895,
                0.751_874_289_958_000_8,
            ],
        ],
        xyz,
    )
}

fn xyz_d50_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let lab = Lab::from_color_unclamped(Xyz::<D50, f64>::new(xyz[0], xyz[1], xyz[2]));
    [lab.l, lab.a, lab.b]
}

fn lab_to_xyz_d50(lightness: f64, a: f64, b: f64) -> [f64; 3] {
    let xyz: Xyz<D50, f64> = Xyz::from_color_unclamped(Lab::new(lightness, a, b));
    [xyz.x, xyz.y, xyz.z]
}

fn matrix(matrix: [[f64; 3]; 3], value: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row[0] * value[0] + row[1] * value[1] + row[2] * value[2])
}

/// Convert interleaved RGB samples while retaining their integer precision.
pub(crate) fn convert_samples_at_depth(
    samples: &[u8],
    sample_depth: crate::image_store::RasterSampleDepth,
    source_space: CssColorSpace,
    target_space: CssColorSpace,
) -> Option<Vec<u8>> {
    if source_space == target_space {
        return Some(samples.to_vec());
    }
    let component_bytes = sample_depth.bytes_per_component();
    if !samples.len().is_multiple_of(3 * component_bytes) {
        return None;
    }
    if source_space == CssColorSpace::XyzD50 || target_space == CssColorSpace::XyzD50 {
        let mut converted = Vec::with_capacity(samples.len());
        for sample in samples.chunks_exact(3 * component_bytes) {
            let components = sample_components(sample, sample_depth)?;
            let color = CssColor::in_space(
                source_space,
                components[0],
                components[1],
                components[2],
                1.0,
            );
            let color = convert_color(color, target_space)?;
            append_color_components(&mut converted, color, sample_depth);
        }
        return Some(converted);
    }
    let source = profile(source_space).ok()?;
    let destination = profile(target_space).ok()?;
    transform_samples_at_depth(&source, &destination, samples, sample_depth)
}

/// Transform retained embedded-ICC RGB samples without changing sample depth.
pub(crate) fn convert_embedded_rgb_samples_at_depth(
    samples: &[u8],
    sample_depth: crate::image_store::RasterSampleDepth,
    embedded_profile: &[u8],
    target_space: CssColorSpace,
) -> Option<Vec<u8>> {
    if !samples
        .len()
        .is_multiple_of(3 * sample_depth.bytes_per_component())
    {
        return None;
    }
    let source = ColorProfile::new_from_slice(embedded_profile).ok()?;
    if source.color_space != DataColorSpace::Rgb {
        return None;
    }
    let destination = profile(target_space).ok()?;
    if target_space == CssColorSpace::XyzD50 {
        return None;
    }
    transform_samples_at_depth(&source, &destination, samples, sample_depth)
}

fn transform_samples_at_depth(
    source: &ColorProfile,
    destination: &ColorProfile,
    samples: &[u8],
    sample_depth: crate::image_store::RasterSampleDepth,
) -> Option<Vec<u8>> {
    match sample_depth {
        crate::image_store::RasterSampleDepth::Eight => {
            let transform = source
                .create_transform_8bit(Layout::Rgb, destination, Layout::Rgb, transform_options())
                .ok()?;
            let mut output = vec![0; samples.len()];
            transform.transform(samples, &mut output).ok()?;
            Some(output)
        }
        crate::image_store::RasterSampleDepth::Sixteen => {
            let mut input = Vec::with_capacity(samples.len() / 2);
            for component in samples.chunks_exact(2) {
                input.push(u16::from_be_bytes([component[0], component[1]]));
            }
            let transform = source
                .create_transform_16bit(Layout::Rgb, destination, Layout::Rgb, transform_options())
                .ok()?;
            let mut output = vec![0; input.len()];
            transform.transform(&input, &mut output).ok()?;
            Some(output.into_iter().flat_map(u16::to_be_bytes).collect())
        }
    }
}

fn sample_components(
    sample: &[u8],
    sample_depth: crate::image_store::RasterSampleDepth,
) -> Option<[f32; 3]> {
    match sample_depth {
        crate::image_store::RasterSampleDepth::Eight => Some([
            f32::from(*sample.first()?) / 255.0,
            f32::from(*sample.get(1)?) / 255.0,
            f32::from(*sample.get(2)?) / 255.0,
        ]),
        crate::image_store::RasterSampleDepth::Sixteen => Some([
            f32::from(u16::from_be_bytes([*sample.first()?, *sample.get(1)?])) / 65535.0,
            f32::from(u16::from_be_bytes([*sample.get(2)?, *sample.get(3)?])) / 65535.0,
            f32::from(u16::from_be_bytes([*sample.get(4)?, *sample.get(5)?])) / 65535.0,
        ]),
    }
}

fn append_color_components(
    output: &mut Vec<u8>,
    color: CssColor,
    sample_depth: crate::image_store::RasterSampleDepth,
) {
    match sample_depth {
        crate::image_store::RasterSampleDepth::Eight => {
            output.extend(color_components_to_u8(color))
        }
        crate::image_store::RasterSampleDepth::Sixteen => {
            for component in color.components() {
                output.extend_from_slice(
                    &((component * 65535.0).round().clamp(0.0, 65535.0) as u16).to_be_bytes(),
                );
            }
        }
    }
}

/// The ICC's published sRGB v4 profile is used for tagged PDF sRGB output.
///
/// This binary is the unmodified `sRGB2014.icc` distributed with WeasyPrint,
/// whose BSD-3-Clause license is retained in its checked-out source tree. It
/// maps the CSS sRGB primary and secondary endpoints exactly in common PDF
/// rasterizers; generated profiles remain appropriate for CSS-only spaces.
const SRGB_2014_ICC_PROFILE: &[u8] = include_bytes!("pdf/sRGB2014.icc");

/// Build the ICC bytes embedded by the PDF writer for a built-in CSS space.
pub(crate) fn icc_profile_bytes(space: CssColorSpace) -> Result<Vec<u8>> {
    match space {
        CssColorSpace::Srgb => Ok(SRGB_2014_ICC_PROFILE.to_vec()),
        _ => profile(space)?.encode().map_err(mox_error),
    }
}

fn transform_options() -> TransformOptions {
    TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        // CSS CssColor 4 requires conversion to retain out-of-gamut components
        // until the concrete output boundary, rather than clipping them in
        // the profile transform.
        allow_extended_range_rgb_xyz: true,
        ..TransformOptions::default()
    }
}

fn color_components_to_u8(color: CssColor) -> [u8; 3] {
    [
        color.components()[0],
        color.components()[1],
        color.components()[2],
    ]
    .map(|component| (component * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn profile(space: CssColorSpace) -> Result<ColorProfile> {
    match space {
        CssColorSpace::Srgb => Ok(ColorProfile::new_srgb()),
        CssColorSpace::XyzD50 => Ok(xyz_d50_profile()),
        CssColorSpace::DisplayP3 => Ok(ColorProfile::new_display_p3()),
        CssColorSpace::A98Rgb => Ok(ColorProfile::new_adobe_rgb()),
        CssColorSpace::ProphotoRgb => Ok(profile_with_curve(
            ColorProfile::new_pro_photo_rgb(),
            ToneReprCurve::Parametric(vec![1.8, 1.0, 0.0, 1.0 / 16.0, 1.0 / 32.0]),
        )),
        CssColorSpace::Rec2020 => Ok(profile_with_curve(
            ColorProfile::new_bt2020(),
            ToneReprCurve::Parametric(vec![
                1.0 / 0.45,
                1.0 / 1.0993,
                0.0993 / 1.0993,
                1.0 / 4.5,
                0.08145,
            ]),
        )),
    }
}

fn profile_with_curve(mut profile: ColorProfile, curve: ToneReprCurve) -> ColorProfile {
    profile.red_trc = Some(curve.clone());
    profile.green_trc = Some(curve.clone());
    profile.blue_trc = Some(curve);
    profile
}

fn xyz_d50_profile() -> ColorProfile {
    // Keep moxcms' private profile-version bookkeeping from its default
    // constructor, then project only the public XYZ/D50 fields required by
    // the generated ICC profile.
    let mut profile = ColorProfile::default();
    profile.color_space = DataColorSpace::Xyz;
    profile.pcs = DataColorSpace::Xyz;
    profile.rendering_intent = RenderingIntent::RelativeColorimetric;
    profile.white_point = moxcms::white_point_d50().to_xyzd();
    profile.media_white_point = Some(moxcms::white_point_d50().to_xyzd());
    profile
}

fn mox_error(error: moxcms::CmsError) -> Error {
    Error::InvalidInput(format!("could not construct ICC color profile: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_rgb_samples_match_the_builtin_profile_transform() {
        let profile = icc_profile_bytes(CssColorSpace::DisplayP3).unwrap();
        let samples = [230, 32, 16, 16, 64, 240];

        assert_eq!(
            convert_embedded_rgb_samples_at_depth(
                &samples,
                crate::image_store::RasterSampleDepth::Eight,
                &profile,
                CssColorSpace::Srgb,
            ),
            convert_samples_at_depth(
                &samples,
                crate::image_store::RasterSampleDepth::Eight,
                CssColorSpace::DisplayP3,
                CssColorSpace::Srgb,
            )
        );
    }

    #[test]
    fn sixteen_bit_embedded_rgb_samples_keep_their_depth_during_conversion() {
        let profile = icc_profile_bytes(CssColorSpace::DisplayP3).unwrap();
        let samples = [0xe6, 0x00, 0x20, 0x00, 0x10, 0x00];

        let converted = convert_embedded_rgb_samples_at_depth(
            &samples,
            crate::image_store::RasterSampleDepth::Sixteen,
            &profile,
            CssColorSpace::Srgb,
        )
        .unwrap();

        assert_eq!(converted.len(), samples.len());
        assert_ne!(converted, samples);
    }

    #[test]
    fn generated_icc_profiles_round_trip_with_their_component_spaces() {
        for space in [
            CssColorSpace::Srgb,
            CssColorSpace::DisplayP3,
            CssColorSpace::A98Rgb,
            CssColorSpace::ProphotoRgb,
            CssColorSpace::Rec2020,
            CssColorSpace::XyzD50,
        ] {
            let bytes = icc_profile_bytes(space).unwrap();
            let parsed = ColorProfile::new_from_slice(&bytes).unwrap();
            assert_eq!(
                parsed.color_space,
                if space == CssColorSpace::XyzD50 {
                    DataColorSpace::Xyz
                } else {
                    DataColorSpace::Rgb
                },
                "{space:?} profile component space"
            );
        }
    }

    #[test]
    fn tagged_srgb_uses_the_standard_profile_bytes_for_exact_primary_endpoints() {
        let bytes = icc_profile_bytes(CssColorSpace::Srgb).unwrap();

        assert_eq!(bytes, SRGB_2014_ICC_PROFILE);
        assert_eq!(
            ColorProfile::new_from_slice(&bytes).unwrap().color_space,
            DataColorSpace::Rgb
        );
    }

    #[test]
    fn css_custom_transfer_curves_override_moxcms_convenience_profiles() {
        let ColorProfile {
            red_trc: Some(ToneReprCurve::Parametric(prophoto)),
            ..
        } = profile(CssColorSpace::ProphotoRgb).unwrap()
        else {
            panic!("ProPhoto RGB has a parametric transfer curve");
        };
        assert_eq!(prophoto, vec![1.8, 1.0, 0.0, 1.0 / 16.0, 1.0 / 32.0]);

        let ColorProfile {
            red_trc: Some(ToneReprCurve::Parametric(rec2020)),
            ..
        } = profile(CssColorSpace::Rec2020).unwrap()
        else {
            panic!("Rec.2020 has a parametric transfer curve");
        };
        assert_eq!(
            rec2020,
            vec![
                1.0 / 0.45,
                1.0 / 1.0993,
                0.0993 / 1.0993,
                1.0 / 4.5,
                0.08145,
            ]
        );
    }

    #[test]
    fn gradient_srgb_interpolation_has_the_encoded_component_midpoint() {
        let method = crate::css::GradientInterpolationMethod {
            space: crate::css::GradientInterpolationSpace::Srgb,
            hue: crate::css::HueInterpolationMethod::Shorter,
        };
        let midpoint = interpolate_color_with_missing(
            CssColor::srgb(1.0, 0.0, 0.0, 1.0),
            CssColor::srgb(0.0, 1.0, 0.0, 1.0),
            method,
            0.5,
            0,
            0,
        );
        assert_eq!(midpoint.space(), CssColorSpace::Srgb);
        assert!((midpoint.components()[0] - 0.5).abs() < 0.0001);
        assert!((midpoint.components()[1] - 0.5).abs() < 0.0001);
        assert_eq!(midpoint.components()[2], 0.0);
    }

    #[test]
    fn gradient_missing_component_uses_the_other_endpoint_before_interpolation() {
        let method = crate::css::GradientInterpolationMethod {
            space: crate::css::GradientInterpolationSpace::Srgb,
            hue: crate::css::HueInterpolationMethod::Shorter,
        };
        let midpoint = interpolate_color_with_missing(
            CssColor::srgb(0.0, 0.5, 0.5, 1.0),
            CssColor::srgb(1.0, 1.0, 1.0, 1.0),
            method,
            0.5,
            0b0001,
            0,
        );
        assert!((midpoint.components()[0] - 1.0).abs() < 0.0001);
        assert!((midpoint.components()[1] - 0.75).abs() < 0.0001);
        assert!((midpoint.components()[2] - 0.75).abs() < 0.0001);
    }

    #[test]
    fn gradient_hue_methods_choose_distinct_arcs() {
        let shorter =
            interpolate_hue_endpoint(10.0, 350.0, crate::css::HueInterpolationMethod::Shorter);
        let longer =
            interpolate_hue_endpoint(10.0, 350.0, crate::css::HueInterpolationMethod::Longer);
        assert!((shorter - -10.0).abs() < 0.001);
        assert!((longer - 350.0).abs() < 0.001);
    }

    #[test]
    fn hsl_decreasing_and_longer_match_when_the_end_hue_is_increasing() {
        let method = |hue| crate::css::GradientInterpolationMethod {
            space: crate::css::GradientInterpolationSpace::Hsl,
            hue,
        };
        let start = CssColor::new(255, 0, 0);
        let end = CssColor::new(255, 165, 0);
        let decreasing = interpolate_color_with_missing(
            start,
            end,
            method(crate::css::HueInterpolationMethod::Decreasing),
            0.5,
            0,
            0,
        );
        let longer = interpolate_color_with_missing(
            start,
            end,
            method(crate::css::HueInterpolationMethod::Longer),
            0.5,
            0,
            0,
        );
        assert_eq!(decreasing, longer);
        assert!(
            longer.components()[2] > 0.5,
            "longer HSL path should pass through cyan"
        );
    }
}
