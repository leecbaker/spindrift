//! Shared CSS Color 4 conversion and ICC-profile support.
//!
//! CSS colors are converted only at a concrete output boundary. The helpers
//! here use the built-in CSS Color 4 RGB spaces and D50 PCS:
//! <https://www.w3.org/TR/css-color-4/#color-conversion>.

use crate::css::ColorSpace;
use crate::{Color, Error, Result};
use lcms2::{
    CIExyY, CIExyYTRIPLE, ColorSpaceSignature, Intent, PixelFormat, Profile, ToneCurve, Transform,
};
use std::rc::Rc;

/// The calibrated component space carried by decoded raster samples.
///
/// CSS Color 4 defers conversion until an output boundary. Raster images need
/// the same distinction: source RGB samples may carry an embedded ICC profile,
/// while generated samples use one of Quire's built-in CSS spaces.
/// <https://www.w3.org/TR/css-color-4/#color-conversion>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum RasterColorSpace {
    BuiltIn(ColorSpace),
    EmbeddedRgb(Rc<[u8]>),
}

impl RasterColorSpace {
    pub(crate) const SRGB: Self = Self::BuiltIn(ColorSpace::Srgb);
}

/// Validate and retain an embedded RGB ICC profile from a decoded image.
///
/// PDF image XObjects use the profile's component count to interpret samples
/// (ISO 32000-2:2020, 8.9.5.5). This milestone accepts only RGB profiles,
/// because Quire's PNG/JPEG raster path materializes three RGB components.
pub(crate) fn embedded_rgb_profile(bytes: Vec<u8>) -> Option<RasterColorSpace> {
    let profile = Profile::new_icc(&bytes).ok()?;
    if profile.color_space() != ColorSpaceSignature::RgbData {
        return None;
    }
    // A syntactically valid profile can still lack the transform tables needed
    // for a usable source-to-output conversion. Validate that boundary once at
    // registration so PDF/A cannot silently label unconverted samples as sRGB.
    Transform::<[f32; 3], [f32; 3]>::new(
        &profile,
        PixelFormat::RGB_FLT,
        &Profile::new_srgb(),
        PixelFormat::RGB_FLT,
        Intent::RelativeColorimetric,
    )
    .ok()?;
    Some(RasterColorSpace::EmbeddedRgb(Rc::from(
        bytes.into_boxed_slice(),
    )))
}

/// Select the one component space required by a PDF shading or raster image.
/// Mixed CSS spaces normalize through the D50 profile-connection space.
pub(crate) fn common_color_space(colors: impl IntoIterator<Item = Color>) -> ColorSpace {
    let mut colors = colors.into_iter();
    let Some(first) = colors.next() else {
        return ColorSpace::Srgb;
    };
    if colors.all(|color| color.space() == first.space()) {
        first.space()
    } else {
        ColorSpace::XyzD50
    }
}

/// Convert one unpremultiplied CSS color between built-in component spaces.
pub(crate) fn convert_color(color: Color, target: ColorSpace) -> Option<Color> {
    if color.space() == target {
        return Some(color);
    }
    let source = profile(color.space()).ok()?;
    let destination = profile(target).ok()?;
    let input = [[color.r, color.g, color.b]];
    let mut output = [[0.0; 3]];
    let transform = Transform::<[f32; 3], [f32; 3]>::new(
        &source,
        pixel_format(color.space()),
        &destination,
        pixel_format(target),
        Intent::RelativeColorimetric,
    )
    .ok()?;
    transform.transform_pixels(&input, &mut output);
    Some(Color::in_space(
        target,
        output[0][0],
        output[0][1],
        output[0][2],
        color.a,
    ))
}

/// Convert encoded 8-bit RGB/XYZ samples for a generated image.
pub(crate) fn convert_samples(
    samples: &[u8],
    source_space: ColorSpace,
    target_space: ColorSpace,
) -> Option<Vec<u8>> {
    if source_space == target_space {
        return Some(samples.to_vec());
    }
    if !samples.len().is_multiple_of(3) {
        return None;
    }
    let source = profile(source_space).ok()?;
    let destination = profile(target_space).ok()?;
    let input = samples
        .chunks_exact(3)
        .map(|pixel| {
            [
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                pixel[2] as f32 / 255.0,
            ]
        })
        .collect::<Vec<_>>();
    let mut output = vec![[0.0; 3]; input.len()];
    let transform = Transform::<[f32; 3], [f32; 3]>::new(
        &source,
        pixel_format(source_space),
        &destination,
        pixel_format(target_space),
        Intent::RelativeColorimetric,
    )
    .ok()?;
    transform.transform_pixels(&input, &mut output);
    Some(
        output
            .into_iter()
            .flat_map(|pixel| {
                pixel.map(|component| (component * 255.0).round().clamp(0.0, 255.0) as u8)
            })
            .collect(),
    )
}

/// Transform decoded RGB image samples from a retained embedded ICC profile.
///
/// The source profile is validated when the image enters the document store;
/// this reopens it only for the short-lived PDF emission transform.
pub(crate) fn convert_embedded_rgb_samples(
    samples: &[u8],
    embedded_profile: &[u8],
    target_space: ColorSpace,
) -> Option<Vec<u8>> {
    if !samples.len().is_multiple_of(3) {
        return None;
    }
    let source = Profile::new_icc(embedded_profile).ok()?;
    if source.color_space() != ColorSpaceSignature::RgbData {
        return None;
    }
    let destination = profile(target_space).ok()?;
    let input = samples
        .chunks_exact(3)
        .map(|pixel| {
            [
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                pixel[2] as f32 / 255.0,
            ]
        })
        .collect::<Vec<_>>();
    let mut output = vec![[0.0; 3]; input.len()];
    let transform = Transform::<[f32; 3], [f32; 3]>::new(
        &source,
        PixelFormat::RGB_FLT,
        &destination,
        pixel_format(target_space),
        Intent::RelativeColorimetric,
    )
    .ok()?;
    transform.transform_pixels(&input, &mut output);
    Some(
        output
            .into_iter()
            .flat_map(|pixel| {
                pixel.map(|component| (component * 255.0).round().clamp(0.0, 255.0) as u8)
            })
            .collect(),
    )
}

/// Build the ICC bytes embedded by the PDF writer for a built-in CSS space.
pub(crate) fn icc_profile_bytes(space: ColorSpace) -> Result<Vec<u8>> {
    profile(space)?.icc().map_err(lcms_error)
}

fn pixel_format(space: ColorSpace) -> PixelFormat {
    if space == ColorSpace::XyzD50 {
        PixelFormat::XYZ_FLT
    } else {
        PixelFormat::RGB_FLT
    }
}

fn profile(space: ColorSpace) -> Result<Profile> {
    match space {
        ColorSpace::Srgb => Ok(Profile::new_srgb()),
        ColorSpace::XyzD50 => Ok(Profile::new_xyz()),
        ColorSpace::DisplayP3 => rgb_profile(
            xy(0.3127, 0.3290),
            primaries((0.680, 0.320), (0.265, 0.690), (0.150, 0.060)),
            srgb_curve()?,
        ),
        ColorSpace::A98Rgb => rgb_profile(
            xy(0.3127, 0.3290),
            primaries((0.6400, 0.3300), (0.2100, 0.7100), (0.1500, 0.0600)),
            ToneCurve::new(563.0 / 256.0),
        ),
        ColorSpace::ProphotoRgb => rgb_profile(
            xy(0.3457, 0.3585),
            primaries((0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001)),
            ToneCurve::new_parametric(4, &[1.8, 1.0, 0.0, 1.0 / 16.0, 1.0 / 32.0])
                .map_err(lcms_error)?,
        ),
        ColorSpace::Rec2020 => rgb_profile(
            xy(0.3127, 0.3290),
            primaries((0.708, 0.292), (0.170, 0.797), (0.131, 0.046)),
            ToneCurve::new_parametric(
                4,
                &[
                    1.0 / 0.45,
                    1.0 / 1.0993,
                    0.0993 / 1.0993,
                    1.0 / 4.5,
                    0.08145,
                ],
            )
            .map_err(lcms_error)?,
        ),
    }
}

fn srgb_curve() -> Result<ToneCurve> {
    ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
        .map_err(lcms_error)
}

fn rgb_profile(white: CIExyY, primaries: CIExyYTRIPLE, curve: ToneCurve) -> Result<Profile> {
    Profile::new_rgb(&white, &primaries, &[&curve, &curve, &curve]).map_err(lcms_error)
}

const fn xy(x: f64, y: f64) -> CIExyY {
    CIExyY { x, y, Y: 1.0 }
}

const fn primaries(red: (f64, f64), green: (f64, f64), blue: (f64, f64)) -> CIExyYTRIPLE {
    CIExyYTRIPLE {
        Red: xy(red.0, red.1),
        Green: xy(green.0, green.1),
        Blue: xy(blue.0, blue.1),
    }
}

fn lcms_error(error: lcms2::Error) -> Error {
    Error::InvalidInput(format!("could not construct ICC color profile: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_rgb_samples_match_the_builtin_profile_transform() {
        let profile = icc_profile_bytes(ColorSpace::DisplayP3).unwrap();
        let samples = [230, 32, 16, 16, 64, 240];

        assert_eq!(
            convert_embedded_rgb_samples(&samples, &profile, ColorSpace::Srgb),
            convert_samples(&samples, ColorSpace::DisplayP3, ColorSpace::Srgb)
        );
    }
}
