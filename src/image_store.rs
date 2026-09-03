//! Document-owned raster-image sources and transient PDF rasterization.
//!
//! CSS layout needs intrinsic image dimensions, but PDF emission is the first
//! stage that needs decoded RGB samples. Keeping the encoded source here makes
//! rendered documents self-contained without retaining one expanded raster for
//! every image use.

use std::collections::HashMap;
use std::io::{BufReader, Cursor};
use std::rc::Rc;

use image::metadata::Orientation;
use image::{AnimationDecoder, ColorType, ImageDecoder, ImageReader};
use url::Url;

use crate::mime::{MimeEssence, parse_mime_type_essence, parse_valid_mime_type_essence};
use crate::units::{CssPixelSize, RasterPixelSize};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
/// Keep the allocation allowance used by `image`'s previous PNG decoder.
const MAX_PNG_DECODER_BYTES: usize = 512 * 1024 * 1024;

/// The integer precision of decoded raster color and opacity samples.
///
/// PDF image XObjects permit 8- and 16-bit component samples.  The sample
/// bytes for [`Self::Sixteen`] are stored in network byte order, which is also
/// the byte order required by PNG and PDF image streams.
/// ISO 32000-2:2020, 8.9.5; W3C PNG 3, 11.2.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RasterSampleDepth {
    Eight,
    Sixteen,
}

impl RasterSampleDepth {
    pub(crate) const fn bits_per_component(self) -> i32 {
        match self {
            Self::Eight => 8,
            Self::Sixteen => 16,
        }
    }

    pub(crate) const fn bytes_per_component(self) -> usize {
        match self {
            Self::Eight => 1,
            Self::Sixteen => 2,
        }
    }

    const fn opaque_component(self) -> [u8; 2] {
        match self {
            Self::Eight => [u8::MAX, 0],
            Self::Sixteen => [u8::MAX, u8::MAX],
        }
    }
}

/// Whether a MIME type named by CSS `image-set()` is backed by a decoder in
/// this build.
///
/// The descriptor participates only in candidate negotiation: it does not
/// constrain the bytes eventually fetched for a selected URL.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MimeSupport {
    Supported,
    Unsupported,
}

/// Classify a CSS `type()` descriptor against Spindrift's actual image decoders.
pub(crate) fn image_mime_support(mime_type: &str) -> MimeSupport {
    let Some(mime_type) = parse_valid_mime_type_essence(mime_type) else {
        return MimeSupport::Unsupported;
    };
    image_mime_essence_support(&mime_type)
}

fn image_mime_essence_support(mime_type: &MimeEssence) -> MimeSupport {
    let supported =
        matches!(
            mime_type.as_str(),
            "image/svg+xml" | "image/jxl" | "image/png"
        ) || image::ImageFormat::from_mime_type(mime_type.as_str()).is_some_and(|format| {
            matches!(
                format,
                image::ImageFormat::Jpeg | image::ImageFormat::Gif | image::ImageFormat::WebP
            )
        });
    if supported {
        MimeSupport::Supported
    } else {
        MimeSupport::Unsupported
    }
}

pub(crate) fn supports_declared_image_mime_type(mime_type: &str) -> bool {
    image_mime_support(mime_type) == MimeSupport::Supported
}

/// Return whether HTML `<source type>` can select an image decoder.
///
/// HTML evaluates the MIME type after the recoverable MIME parsing algorithm,
/// unlike CSS `image-set()` which requires a valid MIME type string.
/// <https://html.spec.whatwg.org/multipage/images.html#updating-the-source-set>
pub(crate) fn supports_html_source_image_mime_type(mime_type: &str) -> bool {
    parse_mime_type_essence(mime_type)
        .is_some_and(|mime_type| image_mime_essence_support(&mime_type) == MimeSupport::Supported)
}

/// Stable, document-local reference to an image source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ImageId(u32);

impl ImageId {
    const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The decoded representation selected for a raster image source.
///
/// CSS Images applies `image-orientation` before intrinsic sizing, image
/// painting, and border-image slicing.  Keeping that choice in the resource
/// identity prevents a raw asset from being accidentally reused where the
/// metadata-oriented representation is required.
/// <https://drafts.csswg.org/css-images-3/#propdef-image-orientation>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RasterOrientationPolicy {
    FromImage,
    Encoded,
}

impl RasterOrientationPolicy {
    pub(crate) const fn applies_metadata_orientation(self) -> bool {
        matches!(self, Self::FromImage)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageMetadata {
    /// Encoded raster sample dimensions used by PDF image resources.
    pub(crate) pixel_size: RasterPixelSize,
    /// Preferred natural dimensions used by CSS image sizing.
    pub(crate) natural_size: CssPixelSize,
}

impl ImageMetadata {
    /// Construct metadata whose natural CSS size uses the source-pixel count.
    pub(crate) fn from_pixel_size(pixel_size: RasterPixelSize) -> Self {
        Self {
            pixel_size,
            natural_size: CssPixelSize::new(pixel_size.width, pixel_size.height),
        }
    }

    /// Apply an EXIF quarter-turn to every axis-bearing image dimension.
    fn with_orientation(
        mut self,
        orientation: Orientation,
        orientation_policy: RasterOrientationPolicy,
    ) -> Self {
        if orientation_policy.applies_metadata_orientation()
            && matches!(
                orientation,
                Orientation::Rotate90
                    | Orientation::Rotate270
                    | Orientation::Rotate90FlipH
                    | Orientation::Rotate270FlipH
            )
        {
            self.pixel_size = RasterPixelSize::new(self.pixel_size.height, self.pixel_size.width);
            self.natural_size =
                CssPixelSize::new(self.natural_size.height, self.natural_size.width);
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EncodedImage {
    bytes: Rc<[u8]>,
    format: EncodedImageFormat,
    metadata: ImageMetadata,
    orientation_policy: RasterOrientationPolicy,
    source_orientation: Orientation,
    direct_jpeg: bool,
    color_space: crate::color::RasterColorSpace,
}

/// Raster-image encodings supported by the document image store.
///
/// JPEG XL is intentionally represented outside [`image::ImageFormat`]: the
/// `image` crate can dispatch third-party decoders but does not have a JPEG XL
/// enum variant. Keeping that distinction here lets this store retain the
/// original source and use `jxl-oxide` only at decode time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EncodedImageFormat {
    Png,
    Image(image::ImageFormat),
    JpegXl,
}

/// A JPEG source that can be represented directly by a PDF `/DCTDecode`
/// image XObject without changing any pixel samples.
///
/// PDF's DCT filter consumes the JPEG interchange stream directly (ISO
/// 32000-2:2020, 7.4.8). Keeping the source bytes shared avoids decoding a
/// photographic image just to recompress its samples less efficiently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectJpegImage {
    pub(crate) bytes: Rc<[u8]>,
    pub(crate) metadata: ImageMetadata,
    pub(crate) color_space: crate::color::RasterColorSpace,
}

/// Expanded samples used only while writing one PDF image object.
/// Expanded samples that are consumed by a single PDF-image emission.
///
/// This intentionally has no `Clone` implementation: retaining or sharing a
/// decoded raster across image objects defeats the store's bounded-memory
/// contract.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RasterImage {
    pub(crate) metadata: ImageMetadata,
    /// The ICC component space for generated or decoded source samples.
    pub(crate) color_space: crate::color::RasterColorSpace,
    pub(crate) sample_depth: RasterSampleDepth,
    pub(crate) rgb: Vec<u8>,
    pub(crate) alpha: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
enum ImageAsset {
    Encoded(EncodedImage),
    Generated(Box<GeneratedRasterImage>),
}

/// Fully resolved CSS generated-image inputs. Gradient computation happens at
/// PDF emission, after layout has fixed the concrete paint size.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GeneratedRasterImage {
    Linear {
        gradient: crate::css::LinearGradient,
        size: crate::document::paint::geometry::PaintSize,
        metadata: ImageMetadata,
    },
    Radial {
        gradient: crate::css::RadialGradient,
        size: crate::document::paint::geometry::PaintSize,
        metadata: ImageMetadata,
    },
}

/// Canonical lookup key for one resolved generated image.
///
/// It is deliberately distinct from URLs and image handles. Concrete raster
/// dimensions are represented with `f32::to_bits`, preserving distinctions
/// such as `-0.0` and NaN payloads that ordinary float equality would lose.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GeneratedImageKey(Box<str>);

impl GeneratedRasterImage {
    fn metadata(&self) -> ImageMetadata {
        match self {
            Self::Linear { metadata, .. } | Self::Radial { metadata, .. } => *metadata,
        }
    }

    /// Resolve the RGB storage space that gradient rasterization will use
    /// without materializing its pixels. This keeps PDF resource planning
    /// bounded while matching the final image encoding rather than treating
    /// CSS interpolation coordinates as image samples.
    fn color_space(&self) -> crate::color::RasterColorSpace {
        let space = match self {
            Self::Linear { gradient, size, .. } => {
                crate::layout::generated_linear_gradient_raster_color_space(
                    gradient,
                    *size,
                    crate::css::CssColor::TRANSPARENT,
                )
            }
            Self::Radial { gradient, size, .. } => {
                crate::layout::generated_radial_gradient_raster_color_space(
                    gradient,
                    *size,
                    crate::css::CssColor::TRANSPARENT,
                )
            }
        };
        crate::color::RasterColorSpace::BuiltIn(space.unwrap_or(crate::css::CssColorSpace::Srgb))
    }

    fn key(&self) -> GeneratedImageKey {
        let mut key = GeneratedImageKeyBuilder::default();
        match self {
            Self::Linear {
                gradient,
                size,
                metadata,
            } => {
                key.tag("linear");
                key.linear_gradient(gradient);
                key.f32(size.width);
                key.f32(size.height);
                key.u32(metadata.pixel_size.width);
                key.u32(metadata.pixel_size.height);
            }
            Self::Radial {
                gradient,
                size,
                metadata,
            } => {
                key.tag("radial");
                key.radial_gradient(gradient);
                key.f32(size.width);
                key.f32(size.height);
                key.u32(metadata.pixel_size.width);
                key.u32(metadata.pixel_size.height);
            }
        }
        GeneratedImageKey(key.output.into_boxed_str())
    }
}

/// Match generated-gradient rasterization's resolved component space without
/// expanding its pixels. CSS gradients interpolate in their selected space;
/// perceptual and Lab-family methods use the D50 XYZ connection space.
/// <https://drafts.csswg.org/css-color-4/#interpolation-space>
#[allow(dead_code)]
fn generated_gradient_interpolation_output_space(
    method: crate::css::GradientInterpolationMethod,
) -> crate::css::CssColorSpace {
    match method.space {
        crate::css::GradientInterpolationSpace::Srgb
        | crate::css::GradientInterpolationSpace::SrgbLinear
        | crate::css::GradientInterpolationSpace::Hsl
        | crate::css::GradientInterpolationSpace::Hwb => crate::css::CssColorSpace::Srgb,
        crate::css::GradientInterpolationSpace::DisplayP3
        | crate::css::GradientInterpolationSpace::DisplayP3Linear => {
            crate::css::CssColorSpace::DisplayP3
        }
        crate::css::GradientInterpolationSpace::A98Rgb => crate::css::CssColorSpace::A98Rgb,
        crate::css::GradientInterpolationSpace::ProphotoRgb => {
            crate::css::CssColorSpace::ProphotoRgb
        }
        crate::css::GradientInterpolationSpace::Rec2020 => crate::css::CssColorSpace::Rec2020,
        crate::css::GradientInterpolationSpace::XyzD50
        | crate::css::GradientInterpolationSpace::XyzD65
        | crate::css::GradientInterpolationSpace::Lab
        | crate::css::GradientInterpolationSpace::Oklab
        | crate::css::GradientInterpolationSpace::Lch
        | crate::css::GradientInterpolationSpace::Oklch => crate::css::CssColorSpace::XyzD50,
    }
}

#[derive(Default)]
struct GeneratedImageKeyBuilder {
    output: String,
}

impl GeneratedImageKeyBuilder {
    fn tag(&mut self, value: &str) {
        self.output.push_str(value);
        self.output.push('|');
    }

    fn u32(&mut self, value: u32) {
        self.output.push_str(&value.to_string());
        self.output.push('|');
    }

    fn usize(&mut self, value: usize) {
        self.output.push_str(&value.to_string());
        self.output.push('|');
    }

    fn bool(&mut self, value: bool) {
        self.u32(u32::from(value));
    }

    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    fn color(&mut self, color: crate::CssColor) {
        self.u32(u32::from(color.space().cache_key()));
        self.f32(color.components()[0]);
        self.f32(color.components()[1]);
        self.f32(color.components()[2]);
        self.f32(color.alpha());
    }

    fn length_percentage(&mut self, value: crate::css::ComputedLengthPercentage) {
        let mut key = String::new();
        value.write_cache_key(&mut key);
        self.tag(&key);
    }

    fn stops_and_hints(
        &mut self,
        stops: &[crate::css::GradientColorStop],
        hints: &[crate::css::GradientColorHint],
    ) {
        self.usize(stops.len());
        for stop in stops {
            self.gradient_color(stop.color);
            match &stop.position {
                Some(position) => {
                    self.bool(true);
                    self.length_percentage(position.clone());
                }
                None => self.bool(false),
            }
        }
        self.usize(hints.len());
        for hint in hints {
            self.usize(hint.after_stop);
            self.length_percentage(hint.position.clone());
        }
    }

    fn gradient_color(&mut self, color: crate::css::GradientColor) {
        match color {
            crate::css::GradientColor::CssColor(color) => {
                self.tag("color");
                self.color(color);
            }
            crate::css::GradientColor::ColorWithMissing {
                color,
                missing,
                source,
            } => {
                self.tag("color-with-missing");
                self.color(color);
                self.u32(u32::from(missing.bits()));
                self.tag(match source {
                    crate::css::GradientMissingComponentSpace::Rgb => "rgb",
                    crate::css::GradientMissingComponentSpace::Xyz => "xyz",
                    crate::css::GradientMissingComponentSpace::Lab => "lab",
                    crate::css::GradientMissingComponentSpace::Oklab => "oklab",
                    crate::css::GradientMissingComponentSpace::Hsl => "hsl",
                    crate::css::GradientMissingComponentSpace::Hwb => "hwb",
                    crate::css::GradientMissingComponentSpace::Lch => "lch",
                    crate::css::GradientMissingComponentSpace::Oklch => "oklch",
                });
            }
            crate::css::GradientColor::CurrentColor => self.tag("currentcolor"),
        }
    }

    fn linear_gradient(&mut self, gradient: &crate::css::LinearGradient) {
        match gradient.direction {
            crate::css::LinearGradientDirection::Angle(angle) => {
                self.tag("angle");
                self.f32(angle);
            }
            crate::css::LinearGradientDirection::Corner {
                horizontal,
                vertical,
            } => {
                self.tag("corner");
                self.tag(match horizontal {
                    crate::css::GradientHorizontalDirection::Left => "left",
                    crate::css::GradientHorizontalDirection::Right => "right",
                });
                self.tag(match vertical {
                    crate::css::GradientVerticalDirection::Top => "top",
                    crate::css::GradientVerticalDirection::Bottom => "bottom",
                });
            }
        }
        self.gradient_interpolation(gradient.interpolation);
        self.bool(gradient.repeating);
        self.stops_and_hints(&gradient.stops, &gradient.hints);
    }

    fn radial_gradient(&mut self, gradient: &crate::css::RadialGradient) {
        self.tag(match gradient.shape {
            crate::css::RadialGradientShape::Circle => "circle",
            crate::css::RadialGradientShape::Ellipse => "ellipse",
        });
        match &gradient.size {
            crate::css::RadialGradientSize::Extent(extent) => self.tag(match extent {
                crate::css::RadialGradientExtent::ClosestSide => "closest-side",
                crate::css::RadialGradientExtent::FarthestSide => "farthest-side",
                crate::css::RadialGradientExtent::ClosestCorner => "closest-corner",
                crate::css::RadialGradientExtent::FarthestCorner => "farthest-corner",
            }),
            crate::css::RadialGradientSize::CircleRadius(radius) => {
                self.tag("circle-radius");
                self.length_percentage(radius.clone());
            }
            crate::css::RadialGradientSize::EllipseRadii { x, y } => {
                self.tag("ellipse-radii");
                self.length_percentage(x.clone());
                self.length_percentage(y.clone());
            }
        }
        self.position_axis(gradient.position.x.clone());
        self.position_axis(gradient.position.y.clone());
        self.gradient_interpolation(gradient.interpolation);
        self.bool(gradient.repeating);
        self.stops_and_hints(&gradient.stops, &gradient.hints);
    }

    fn gradient_interpolation(&mut self, method: crate::css::GradientInterpolationMethod) {
        self.tag(match method.space {
            crate::css::GradientInterpolationSpace::Srgb => "srgb",
            crate::css::GradientInterpolationSpace::SrgbLinear => "srgb-linear",
            crate::css::GradientInterpolationSpace::DisplayP3 => "display-p3",
            crate::css::GradientInterpolationSpace::DisplayP3Linear => "display-p3-linear",
            crate::css::GradientInterpolationSpace::A98Rgb => "a98-rgb",
            crate::css::GradientInterpolationSpace::ProphotoRgb => "prophoto-rgb",
            crate::css::GradientInterpolationSpace::Rec2020 => "rec2020",
            crate::css::GradientInterpolationSpace::XyzD50 => "xyz-d50",
            crate::css::GradientInterpolationSpace::XyzD65 => "xyz-d65",
            crate::css::GradientInterpolationSpace::Lab => "lab",
            crate::css::GradientInterpolationSpace::Oklab => "oklab",
            crate::css::GradientInterpolationSpace::Hsl => "hsl",
            crate::css::GradientInterpolationSpace::Hwb => "hwb",
            crate::css::GradientInterpolationSpace::Lch => "lch",
            crate::css::GradientInterpolationSpace::Oklch => "oklch",
        });
        self.tag(match method.hue {
            crate::css::HueInterpolationMethod::Shorter => "shorter",
            crate::css::HueInterpolationMethod::Longer => "longer",
            crate::css::HueInterpolationMethod::Increasing => "increasing",
            crate::css::HueInterpolationMethod::Decreasing => "decreasing",
        });
    }

    fn position_axis(&mut self, axis: crate::css::BackgroundPositionAxis) {
        self.tag(match axis.origin {
            crate::css::BackgroundPositionOrigin::Start => "start",
            crate::css::BackgroundPositionOrigin::Center => "center",
            crate::css::BackgroundPositionOrigin::End => "end",
        });
        self.length_percentage(axis.offset);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct DocumentImageStore {
    images: Vec<ImageAsset>,
    urls: HashMap<Url, ImageId>,
    oriented_urls: HashMap<Url, ImageId>,
    data_urls: HashMap<String, ImageId>,
    oriented_data_urls: HashMap<String, ImageId>,
    generated_images: HashMap<GeneratedImageKey, ImageId>,
    /// The CSS device density chosen while laying out this static document.
    /// PDF serialization uses it as the deterministic raster sampling grid.
    output_resolution_dppx: Option<f32>,
}

impl DocumentImageStore {
    pub(crate) fn set_output_resolution_dppx(&mut self, resolution_dppx: f32) {
        debug_assert!(resolution_dppx.is_finite() && resolution_dppx > 0.0);
        self.output_resolution_dppx = Some(resolution_dppx);
    }

    pub(crate) fn output_resolution_dppx(&self) -> f32 {
        self.output_resolution_dppx.unwrap_or(1.0)
    }

    pub(crate) fn resolve_url_with_orientation(
        &mut self,
        url: Url,
        bytes: Rc<[u8]>,
        orientation_policy: RasterOrientationPolicy,
    ) -> Option<(ImageId, ImageMetadata)> {
        let existing = if orientation_policy.applies_metadata_orientation() {
            self.oriented_urls.get(&url).cloned()
        } else {
            self.urls.get(&url).cloned()
        };
        if let Some(id) = existing {
            return Some((id, self.metadata(id)?));
        }
        // `image-orientation: from-image` and the encoded representation are
        // identical when the source has no orientation transform. Reuse the
        // established document-local identity instead of retaining the same
        // bytes twice merely because an early availability check used the
        // encoded policy.
        let alternate = if orientation_policy.applies_metadata_orientation() {
            self.urls.get(&url).copied()
        } else {
            self.oriented_urls.get(&url).copied()
        };
        if let Some(id) = alternate.filter(|id| self.orientation_is_noop(*id)) {
            return Some((id, self.metadata(id)?));
        }
        let (id, metadata) = self.insert(bytes, orientation_policy)?;
        if orientation_policy.applies_metadata_orientation() {
            self.oriented_urls.insert(url, id);
        } else {
            self.urls.insert(url, id);
        }
        Some((id, metadata))
    }

    pub(crate) fn resolve_data_url_with_orientation(
        &mut self,
        source: &str,
        bytes: Rc<[u8]>,
        orientation_policy: RasterOrientationPolicy,
    ) -> Option<(ImageId, ImageMetadata)> {
        let existing = if orientation_policy.applies_metadata_orientation() {
            self.oriented_data_urls.get(source).cloned()
        } else {
            self.data_urls.get(source).cloned()
        };
        if let Some(id) = existing {
            return Some((id, self.metadata(id)?));
        }
        let alternate = if orientation_policy.applies_metadata_orientation() {
            self.data_urls.get(source).copied()
        } else {
            self.oriented_data_urls.get(source).copied()
        };
        if let Some(id) = alternate.filter(|id| self.orientation_is_noop(*id)) {
            return Some((id, self.metadata(id)?));
        }
        let (id, metadata) = self.insert(bytes, orientation_policy)?;
        if orientation_policy.applies_metadata_orientation() {
            self.oriented_data_urls.insert(source.to_owned(), id);
        } else {
            self.data_urls.insert(source.to_owned(), id);
        }
        Some((id, metadata))
    }

    fn insert(
        &mut self,
        bytes: Rc<[u8]>,
        orientation_policy: RasterOrientationPolicy,
    ) -> Option<(ImageId, ImageMetadata)> {
        let (metadata, format, color_space, source_orientation, direct_jpeg) =
            image_metadata(&bytes, orientation_policy)?;
        let id = ImageId(u32::try_from(self.images.len()).ok()?);
        self.images.push(ImageAsset::Encoded(EncodedImage {
            bytes,
            format,
            metadata,
            orientation_policy,
            source_orientation,
            direct_jpeg,
            color_space,
        }));
        Some((id, metadata))
    }

    fn orientation_is_noop(&self, id: ImageId) -> bool {
        matches!(
            self.images.get(id.index()),
            Some(ImageAsset::Encoded(EncodedImage {
                source_orientation: Orientation::NoTransforms,
                ..
            }))
        )
    }

    pub(crate) fn metadata(&self, id: ImageId) -> Option<ImageMetadata> {
        self.images.get(id.index()).map(|image| match image {
            ImageAsset::Encoded(image) => image.metadata,
            ImageAsset::Generated(image) => image.metadata(),
        })
    }

    /// Return the retained source profile without decoding its pixels.
    pub(crate) fn color_space(&self, id: ImageId) -> Option<crate::color::RasterColorSpace> {
        self.images.get(id.index()).map(|image| match image {
            ImageAsset::Encoded(image) => image.color_space.clone(),
            ImageAsset::Generated(image) => image.color_space(),
        })
    }

    /// Return an original JPEG stream when PDF emission can use it without
    /// bypassing a required EXIF-orientation transform.
    pub(crate) fn direct_jpeg(&self, id: ImageId) -> Option<DirectJpegImage> {
        let ImageAsset::Encoded(image) = self.images.get(id.index())? else {
            return None;
        };
        (image.direct_jpeg
            && !(image.orientation_policy.applies_metadata_orientation()
                && image.source_orientation != Orientation::NoTransforms))
            .then(|| DirectJpegImage {
                bytes: Rc::clone(&image.bytes),
                metadata: image.metadata,
                color_space: image.color_space.clone(),
            })
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.images.len()
    }

    pub(crate) fn register_generated(&mut self, image: GeneratedRasterImage) -> ImageId {
        let key = image.key();
        if let Some(id) = self.generated_images.get(&key).cloned() {
            return id;
        }
        let id =
            ImageId(u32::try_from(self.images.len()).expect("document image count exceeds u32"));
        self.images.push(ImageAsset::Generated(Box::new(image)));
        self.generated_images.insert(key, id);
        id
    }

    /// Materialize an image for one short-lived consumer. The decoded raster
    /// cannot be stored in a PDF plan because it is only provided to this
    /// closure and `RasterImage` is non-cloneable.
    pub(crate) fn with_rasterized<T>(
        &self,
        id: ImageId,
        consume: impl FnOnce(RasterImage) -> T,
    ) -> Option<T> {
        let image = self.images.get(id.index())?;
        let raster = match image {
            ImageAsset::Generated(recipe) => crate::layout::rasterize_generated_image(recipe),
            ImageAsset::Encoded(image) => self.rasterize_encoded(image),
        }?;
        Some(consume(raster))
    }

    fn rasterize_encoded(&self, image: &EncodedImage) -> Option<RasterImage> {
        if image.format == EncodedImageFormat::Png {
            let decoded = decode_png_samples(&image.bytes, MAX_PNG_DECODER_BYTES)?;
            let decoded = if image.orientation_policy.applies_metadata_orientation() {
                apply_png_orientation(decoded, image.source_orientation)?
            } else {
                decoded
            };
            debug_assert_eq!(
                (decoded.width, decoded.height),
                (
                    image.metadata.pixel_size.width,
                    image.metadata.pixel_size.height
                ),
                "PNG metadata and raster dimensions must agree"
            );
            return Some(RasterImage {
                metadata: image.metadata,
                color_space: image.color_space.clone(),
                sample_depth: decoded.sample_depth,
                rgb: decoded.rgb,
                alpha: decoded.alpha,
            });
        }

        // GIF and animated WebP expose their logical screen dimensions and
        // contribute only their first image frame. This deliberately gives
        // animated images a stable PDF representation: frame timing, looping,
        // disposal, and later-frame compositing are not part of a static PDF
        // image.
        // https://www.w3.org/TR/css-images-3/#image-notation
        let mut decoded = if image.format == EncodedImageFormat::Image(image::ImageFormat::WebP) {
            decode_webp_first_frame(&image.bytes)?.0
        } else if image.format == EncodedImageFormat::JpegXl {
            let decoder =
                jxl_oxide::integration::JxlDecoder::new(Cursor::new(image.bytes.as_ref())).ok()?;
            image::DynamicImage::from_decoder(decoder).ok()?
        } else {
            let EncodedImageFormat::Image(format) = image.format else {
                unreachable!("PNG and JPEG XL are handled above");
            };
            let decoder = ImageReader::with_format(Cursor::new(image.bytes.as_ref()), format)
                .into_decoder()
                .ok()?;
            image::DynamicImage::from_decoder(decoder).ok()?
        };
        if image.orientation_policy.applies_metadata_orientation() {
            decoded.apply_orientation(image.source_orientation);
        }
        let (sample_depth, rgb, alpha) = raster_samples_from_dynamic_image(decoded)?;
        Some(RasterImage {
            metadata: image.metadata,
            color_space: image.color_space.clone(),
            sample_depth,
            rgb,
            alpha,
        })
    }

    /// Feed a stable representation of an asset into a document-level hash
    /// without requiring rasterization of encoded images.
    pub(crate) fn write_asset_identity(&self, id: ImageId, mut write: impl FnMut(&[u8])) {
        let Some(asset) = self.images.get(id.index()) else {
            write(b"missing-image");
            return;
        };
        match asset {
            ImageAsset::Encoded(image) => {
                write(b"encoded-image");
                write(format!("{:?}", image.format).as_bytes());
                write(&image.metadata.pixel_size.width.to_be_bytes());
                write(&image.metadata.pixel_size.height.to_be_bytes());
                write(&image.bytes);
            }
            ImageAsset::Generated(image) => {
                write(b"generated-image");
                write(image.key().0.as_bytes());
            }
        }
    }

    /// Drop layout-only source lookup keys before publishing the document.
    pub(crate) fn finalize(&mut self) {
        self.urls.clear();
        self.data_urls.clear();
        self.generated_images.clear();
    }
}

/// Decode an image payload embedded by another document format.
///
/// SVG `data:` image resources are not addressable through the document's URL
/// cache, but they must use the same static-image decoding rules as HTML/CSS
/// images (including the first-frame and colour-profile handling).  The
/// returned samples are deliberately detached from this short-lived store and
/// become an inline paint source owned by the SVG scene.
pub(crate) fn decode_embedded_raster(bytes: Rc<[u8]>) -> Option<RasterImage> {
    let mut store = DocumentImageStore::default();
    let (image, _) = store.insert(bytes, RasterOrientationPolicy::Encoded)?;
    store.with_rasterized(image, |raster| raster)
}

/// Decode a WebP image as one static CSS image.
///
/// Animated WebP uses the first composited frame, which covers the decoder's
/// logical canvas. Static WebP retains the ordinary decoder path so its native
/// RGB/RGBA representation remains unchanged.
/// <https://www.w3.org/TR/css-images-3/#image-notation>
fn decode_webp_first_frame(bytes: &[u8]) -> Option<(image::DynamicImage, Orientation)> {
    let mut decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let decoded = if decoder.has_animation() {
        image::DynamicImage::ImageRgba8(decoder.into_frames().next()?.ok()?.into_buffer())
    } else {
        image::DynamicImage::from_decoder(decoder).ok()?
    };
    Some((decoded, orientation))
}

/// Convert a decoded image into Spindrift's interleaved RGB and optional opacity
/// planes without widening 8-bit input. JPEG XL's integration decoder exposes
/// integer images as the matching `image` depth, and float/HDR images as
/// `Rgb32F`/`Rgba32F`; the latter have no lossless PDF image representation.
fn raster_samples_from_dynamic_image(
    decoded: image::DynamicImage,
) -> Option<(RasterSampleDepth, Vec<u8>, Option<Vec<u8>>)> {
    let depth = match decoded.color() {
        ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8 => {
            RasterSampleDepth::Eight
        }
        ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16 => {
            RasterSampleDepth::Sixteen
        }
        ColorType::Rgb32F | ColorType::Rgba32F | _ => return None,
    };
    let component_bytes = depth.bytes_per_component();
    let opaque = &depth.opaque_component()[..component_bytes];
    let (rgba, pixel_count) = match depth {
        RasterSampleDepth::Eight => {
            let rgba = decoded.to_rgba8().into_raw();
            let pixel_count = rgba.len().checked_div(4)?;
            (rgba, pixel_count)
        }
        RasterSampleDepth::Sixteen => {
            let rgba = decoded.to_rgba16().into_raw();
            let pixel_count = rgba.len().checked_div(4)?;
            let mut bytes = Vec::with_capacity(rgba.len().checked_mul(2)?);
            for component in rgba {
                bytes.extend_from_slice(&component.to_be_bytes());
            }
            (bytes, pixel_count)
        }
    };
    let mut rgb = Vec::with_capacity(pixel_count.checked_mul(3)?.checked_mul(component_bytes)?);
    let mut alpha = Vec::with_capacity(pixel_count.checked_mul(component_bytes)?);
    let mut has_alpha = false;
    for pixel in rgba.chunks_exact(4 * component_bytes) {
        let (color, opacity) = pixel.split_at(3 * component_bytes);
        rgb.extend_from_slice(color);
        alpha.extend_from_slice(opacity);
        has_alpha |= opacity != opaque;
    }
    Some((depth, rgb, has_alpha.then_some(alpha)))
}

fn image_metadata(
    bytes: &[u8],
    orientation_policy: RasterOrientationPolicy,
) -> Option<(
    ImageMetadata,
    EncodedImageFormat,
    crate::color::RasterColorSpace,
    Orientation,
    bool,
)> {
    if is_png(bytes) {
        return png_metadata(bytes, orientation_policy);
    }
    if is_jpeg_xl(bytes) {
        return jpeg_xl_metadata(bytes, orientation_policy);
    }
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let format = reader.format()?;
    let mut decoder = reader.into_decoder().ok()?;
    let color_space = match decoder.icc_profile() {
        Ok(Some(profile)) => crate::color::embedded_rgb_profile(profile).unwrap_or_else(|| {
            log::debug!("ignoring an invalid or non-RGB embedded image ICC profile");
            crate::color::RasterColorSpace::SRGB
        }),
        Ok(None) => {
            log::debug!("using the sRGB fallback for an image without color metadata");
            crate::color::RasterColorSpace::SRGB
        }
        Err(error) => {
            log::debug!(
                "using the sRGB fallback after reading an image ICC profile failed: {error}"
            );
            crate::color::RasterColorSpace::SRGB
        }
    };
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let direct_jpeg = format == image::ImageFormat::Jpeg && decoder.color_type() == ColorType::Rgb8;
    let (pixel_width, pixel_height) = decoder.dimensions();
    let pixel_size = RasterPixelSize::new(pixel_width, pixel_height);
    let natural_size = exif_preferred_natural_size(bytes, pixel_size)
        .unwrap_or_else(|| CssPixelSize::new(pixel_size.width, pixel_size.height));
    let metadata = ImageMetadata {
        pixel_size,
        natural_size,
    }
    .with_orientation(orientation, orientation_policy);
    Some((
        metadata,
        EncodedImageFormat::Image(format),
        color_space,
        orientation,
        direct_jpeg,
    ))
}

/// Return the preferred CSS-pixel dimensions declared by valid EXIF image
/// metadata.
///
/// HTML only honors this metadata when the image's physical dimensions and
/// both EXIF resolutions exactly describe the preferred dimensions. EXIF
/// resolution and uses its specified 72-factor equation, rather than CSS's
/// 96 pixels per inch.
/// <https://html.spec.whatwg.org/multipage/images.html#updating-the-image-data>
fn exif_preferred_natural_size(bytes: &[u8], pixel_size: RasterPixelSize) -> Option<CssPixelSize> {
    let exif = exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()?;
    let unsigned = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .and_then(|field| field.value.get_uint(0))
            .filter(|value| *value > 0)
    };
    let resolution = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .and_then(|field| match &field.value {
                exif::Value::Rational(values) => values
                    .first()
                    .copied()
                    .filter(|value| value.num > 0 && value.denom > 0),
                _ => None,
            })
    };
    let preferred_width = unsigned(exif::Tag::PixelXDimension)?;
    let preferred_height = unsigned(exif::Tag::PixelYDimension)?;
    if unsigned(exif::Tag::ResolutionUnit)? != 2 {
        return None;
    }
    let x_resolution = resolution(exif::Tag::XResolution)?;
    let y_resolution = resolution(exif::Tag::YResolution)?;
    if exif_resolution_matches(pixel_size.width, preferred_width, x_resolution)
        && exif_resolution_matches(pixel_size.height, preferred_height, y_resolution)
    {
        Some(CssPixelSize::new(preferred_width, preferred_height))
    } else {
        None
    }
}

/// Check one exact preferred-dimension equation from HTML image presentation.
fn exif_resolution_matches(
    physical_dimension: u32,
    preferred_dimension: u32,
    resolution: exif::Rational,
) -> bool {
    u128::from(physical_dimension) * 72 * u128::from(resolution.denom)
        == u128::from(preferred_dimension) * u128::from(resolution.num)
}

/// Native-depth PNG samples before they enter Spindrift's shared raster-image
/// pipeline.
pub(crate) struct DecodedPngSamples {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) sample_depth: RasterSampleDepth,
    pub(crate) rgb: Vec<u8>,
    pub(crate) alpha: Option<Vec<u8>>,
}

/// Decode one PNG into Spindrift's RGB-plus-optional-alpha sample representation.
///
/// PNG's native decoder validates chunk ordering and checksums while retaining
/// 8- or 16-bit sample precision. PNG and PDF both store 16-bit components in
/// network byte order.
pub(crate) fn decode_png_samples(
    bytes: &[u8],
    allocation_limit: usize,
) -> Option<DecodedPngSamples> {
    let mut decoder = png::Decoder::new_with_limits(
        BufReader::new(Cursor::new(bytes)),
        png::Limits {
            bytes: allocation_limit,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let output_len = reader.output_buffer_size()?;
    if output_len > allocation_limit {
        return None;
    }
    let mut samples = vec![0; output_len];
    let output = reader.next_frame(&mut samples).ok()?;
    let sample_depth = match output.bit_depth {
        png::BitDepth::Eight => RasterSampleDepth::Eight,
        png::BitDepth::Sixteen => RasterSampleDepth::Sixteen,
        _ => return None,
    };
    samples.truncate(output.buffer_size());
    let pixel_count = usize::try_from(output.width)
        .ok()?
        .checked_mul(usize::try_from(output.height).ok()?)?;

    let component_bytes = sample_depth.bytes_per_component();
    let opaque = &sample_depth.opaque_component()[..component_bytes];
    let (rgb, alpha) = match output.color_type {
        png::ColorType::Grayscale => {
            if samples.len() != pixel_count.checked_mul(component_bytes)? {
                return None;
            }
            let mut rgb =
                Vec::with_capacity(pixel_count.checked_mul(3)?.checked_mul(component_bytes)?);
            for gray in samples.chunks_exact(component_bytes) {
                rgb.extend_from_slice(gray);
                rgb.extend_from_slice(gray);
                rgb.extend_from_slice(gray);
            }
            (rgb, None)
        }
        png::ColorType::GrayscaleAlpha => {
            if samples.len() != pixel_count.checked_mul(2)?.checked_mul(component_bytes)? {
                return None;
            }
            let mut rgb =
                Vec::with_capacity(pixel_count.checked_mul(3)?.checked_mul(component_bytes)?);
            let mut alpha = Vec::with_capacity(pixel_count.checked_mul(component_bytes)?);
            let mut has_alpha = false;
            for pixel in samples.chunks_exact(2 * component_bytes) {
                let (gray, opacity) = pixel.split_at(component_bytes);
                rgb.extend_from_slice(gray);
                rgb.extend_from_slice(gray);
                rgb.extend_from_slice(gray);
                alpha.extend_from_slice(opacity);
                has_alpha |= opacity != opaque;
            }
            (rgb, has_alpha.then_some(alpha))
        }
        png::ColorType::Rgb => {
            if samples.len() != pixel_count.checked_mul(3)?.checked_mul(component_bytes)? {
                return None;
            }
            (samples, None)
        }
        png::ColorType::Rgba => {
            if samples.len() != pixel_count.checked_mul(4)?.checked_mul(component_bytes)? {
                return None;
            }
            let mut rgb =
                Vec::with_capacity(pixel_count.checked_mul(3)?.checked_mul(component_bytes)?);
            let mut alpha = Vec::with_capacity(pixel_count.checked_mul(component_bytes)?);
            let mut has_alpha = false;
            for pixel in samples.chunks_exact(4 * component_bytes) {
                let (color, opacity) = pixel.split_at(3 * component_bytes);
                rgb.extend_from_slice(color);
                alpha.extend_from_slice(opacity);
                has_alpha |= opacity != opaque;
            }
            (rgb, has_alpha.then_some(alpha))
        }
        png::ColorType::Indexed => return None,
    };
    Some(DecodedPngSamples {
        width: output.width,
        height: output.height,
        sample_depth,
        rgb,
        alpha,
    })
}

fn png_metadata(
    bytes: &[u8],
    orientation_policy: RasterOrientationPolicy,
) -> Option<(
    ImageMetadata,
    EncodedImageFormat,
    crate::color::RasterColorSpace,
    Orientation,
    bool,
)> {
    let decoder = png::Decoder::new_with_limits(
        BufReader::new(Cursor::new(bytes)),
        png::Limits {
            bytes: MAX_PNG_DECODER_BYTES,
        },
    );
    let reader = decoder.read_info().ok()?;
    let info = reader.info();
    let pixel_size = RasterPixelSize::new(info.width, info.height);
    let layout_exif_is_eligible = png_layout_exif_is_eligible(bytes);
    let orientation = layout_exif_is_eligible
        .then(|| png_orientation(bytes))
        .flatten()
        .unwrap_or(Orientation::NoTransforms);
    let natural_size = layout_exif_is_eligible
        .then(|| exif_preferred_natural_size(bytes, pixel_size))
        .flatten()
        .unwrap_or_else(|| CssPixelSize::new(pixel_size.width, pixel_size.height));
    Some((
        ImageMetadata {
            pixel_size,
            natural_size,
        }
        .with_orientation(orientation, orientation_policy),
        EncodedImageFormat::Png,
        png_color_space(info),
        orientation,
        false,
    ))
}

/// Read PNG color metadata through the same decoder that validates and
/// expands the image. PNG's `iCCP` declaration takes precedence over all
/// other color declarations, followed by `sRGB`, then `gAMA`/`cHRM`.
/// <https://www.w3.org/TR/png-3/#11iCCP>
/// <https://www.w3.org/TR/png-3/#11sRGB>
/// <https://www.w3.org/TR/png-3/#11gAMA>
/// <https://www.w3.org/TR/png-3/#11cHRM>
fn png_color_space(info: &png::Info<'_>) -> crate::color::RasterColorSpace {
    if let Some(profile) = &info.icc_profile {
        return crate::color::embedded_rgb_profile(profile.to_vec()).unwrap_or_else(|| {
            log::debug!("ignoring an invalid or non-RGB embedded PNG ICC profile");
            crate::color::RasterColorSpace::SRGB
        });
    }
    if info.srgb.is_some() {
        return crate::color::RasterColorSpace::SRGB;
    }
    let color_space = info
        .gama_chunk
        .zip(info.chrm_chunk)
        .and_then(|(gamma, chromaticities)| {
            crate::color::png_gamma_chromaticities_profile(
                f64::from(gamma.into_scaled()) / 100_000.0,
                crate::color::PngChromaticities {
                    white_x: f64::from(chromaticities.white.0.into_scaled()) / 100_000.0,
                    white_y: f64::from(chromaticities.white.1.into_scaled()) / 100_000.0,
                    red_x: f64::from(chromaticities.red.0.into_scaled()) / 100_000.0,
                    red_y: f64::from(chromaticities.red.1.into_scaled()) / 100_000.0,
                    green_x: f64::from(chromaticities.green.0.into_scaled()) / 100_000.0,
                    green_y: f64::from(chromaticities.green.1.into_scaled()) / 100_000.0,
                    blue_x: f64::from(chromaticities.blue.0.into_scaled()) / 100_000.0,
                    blue_y: f64::from(chromaticities.blue.1.into_scaled()) / 100_000.0,
                },
            )
        });
    color_space.unwrap_or_else(|| {
        log::debug!("using the sRGB fallback for a PNG without color metadata");
        crate::color::RasterColorSpace::SRGB
    })
}

fn png_orientation(bytes: &[u8]) -> Option<Orientation> {
    let exif = exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()?;
    let orientation = exif
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)?
        .value
        .get_uint(0)?;
    Orientation::from_exif(u8::try_from(orientation).ok()?)
}

/// Whether EXIF metadata in this PNG may affect CSS layout and painting.
///
/// CSS Images asks UAs to ignore metadata that occurs after image data starts,
/// but to retain metadata if the placement cannot be determined.  The bounded
/// chunk walk deliberately reports malformed or truncated streams as
/// indeterminate rather than guessing.
/// <https://drafts.csswg.org/css-images-3/#url-metadata>
/// <https://www.w3.org/TR/png-3/#11eXIf>
fn png_layout_exif_is_eligible(bytes: &[u8]) -> bool {
    if !is_png(bytes) {
        return true;
    }

    let mut image_data_started = false;
    let mut offset: usize = 8;
    loop {
        let Some(header_end) = offset.checked_add(8) else {
            return true;
        };
        let Some(header) = bytes.get(offset..header_end) else {
            return true;
        };
        let Ok(length_bytes) = header[..4].try_into() else {
            return true;
        };
        let length = u32::from_be_bytes(length_bytes) as usize;
        let Some(chunk_start) = offset.checked_add(8) else {
            return true;
        };
        let Some(data_end) = chunk_start.checked_add(length) else {
            return true;
        };
        let Some(chunk_end) = data_end.checked_add(4) else {
            return true;
        };
        if bytes.get(chunk_start..chunk_end).is_none() {
            return true;
        }
        match &header[4..] {
            b"eXIf" => return !image_data_started,
            b"IDAT" => image_data_started = true,
            b"IEND" => return true,
            _ => {}
        }
        offset = chunk_end;
    }
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(PNG_SIGNATURE)
}

fn apply_png_orientation(
    decoded: DecodedPngSamples,
    orientation: Orientation,
) -> Option<DecodedPngSamples> {
    if orientation == Orientation::NoTransforms {
        return Some(decoded);
    }
    let component_bytes = decoded.sample_depth.bytes_per_component();
    let rgb = orient_png_sample_plane(
        &decoded.rgb,
        decoded.width,
        decoded.height,
        3 * component_bytes,
        orientation,
    )?;
    let alpha = match decoded.alpha.as_deref() {
        Some(alpha) => Some(orient_png_sample_plane(
            alpha,
            decoded.width,
            decoded.height,
            component_bytes,
            orientation,
        )?),
        None => None,
    };
    let (width, height) = match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (decoded.height, decoded.width),
        Orientation::NoTransforms
        | Orientation::Rotate180
        | Orientation::FlipHorizontal
        | Orientation::FlipVertical => (decoded.width, decoded.height),
    };
    Some(DecodedPngSamples {
        width,
        height,
        sample_depth: decoded.sample_depth,
        rgb,
        alpha,
    })
}

fn orient_png_sample_plane(
    source: &[u8],
    width: u32,
    height: u32,
    components: usize,
    orientation: Orientation,
) -> Option<Vec<u8>> {
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    let pixel_count = width.checked_mul(height)?;
    if source.len() != pixel_count.checked_mul(components)? {
        return None;
    }
    let (output_width, output_height) = match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        Orientation::NoTransforms
        | Orientation::Rotate180
        | Orientation::FlipHorizontal
        | Orientation::FlipVertical => (width, height),
    };
    let mut output = vec![0; source.len()];
    for output_y in 0..output_height {
        for output_x in 0..output_width {
            let (source_x, source_y) = match orientation {
                Orientation::NoTransforms => (output_x, output_y),
                Orientation::Rotate90 => (output_y, height - 1 - output_x),
                Orientation::Rotate180 => (width - 1 - output_x, height - 1 - output_y),
                Orientation::Rotate270 => (width - 1 - output_y, output_x),
                Orientation::FlipHorizontal => (width - 1 - output_x, output_y),
                Orientation::FlipVertical => (output_x, height - 1 - output_y),
                Orientation::Rotate90FlipH => (output_y, output_x),
                Orientation::Rotate270FlipH => (width - 1 - output_y, height - 1 - output_x),
            };
            let source_start = source_y
                .checked_mul(width)?
                .checked_add(source_x)?
                .checked_mul(components)?;
            let output_start = output_y
                .checked_mul(output_width)?
                .checked_add(output_x)?
                .checked_mul(components)?;
            output[output_start..output_start + components]
                .copy_from_slice(&source[source_start..source_start + components]);
        }
    }
    Some(output)
}

/// Return whether bytes begin with either JPEG XL file signature from
/// ISO/IEC 18181-2:2024, 11.2.2 (codestream) or 11.2.3 (container).
const fn is_jpeg_xl(bytes: &[u8]) -> bool {
    matches!(bytes, [0xff, 0x0a, ..])
        || matches!(
            bytes,
            [
                0,
                0,
                0,
                0x0c,
                b'J',
                b'X',
                b'L',
                b' ',
                0x0d,
                0x0a,
                0x87,
                0x0a,
                ..
            ]
        )
}

/// Read JPEG XL metadata through `jxl-oxide`'s `image` integration.
///
/// The decoder's ICC profile describes the rendered samples, so retaining it
/// preserves the same source-color-space contract as the built-in decoders.
fn jpeg_xl_metadata(
    bytes: &[u8],
    orientation_policy: RasterOrientationPolicy,
) -> Option<(
    ImageMetadata,
    EncodedImageFormat,
    crate::color::RasterColorSpace,
    Orientation,
    bool,
)> {
    let mut decoder = jxl_oxide::integration::JxlDecoder::new(Cursor::new(bytes)).ok()?;
    let color_space = match decoder.icc_profile() {
        Ok(Some(profile)) => crate::color::embedded_rgb_profile(profile).unwrap_or_else(|| {
            log::debug!("ignoring an invalid or non-RGB JPEG XL ICC profile");
            crate::color::RasterColorSpace::SRGB
        }),
        Ok(None) => crate::color::RasterColorSpace::SRGB,
        Err(error) => {
            log::debug!(
                "using the sRGB fallback after reading a JPEG XL ICC profile failed: {error}"
            );
            crate::color::RasterColorSpace::SRGB
        }
    };
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (pixel_width, pixel_height) = decoder.dimensions();
    Some((
        ImageMetadata::from_pixel_size(RasterPixelSize::new(pixel_width, pixel_height))
            .with_orientation(orientation, orientation_policy),
        EncodedImageFormat::JpegXl,
        color_space,
        orientation,
        false,
    ))
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use image::{ExtendedColorType, Frame, ImageEncoder, RgbaImage};

    use super::*;

    #[test]
    fn image_set_mime_parameters_require_valid_mime_syntax() {
        assert_eq!(
            image_mime_support("image/png; charset=utf-8"),
            MimeSupport::Supported
        );
        assert_eq!(
            image_mime_support("image/png; profile=\"display p3\""),
            MimeSupport::Supported
        );
        for invalid in [
            "image/png; charset",
            "image/png; =utf-8",
            "image/png; charset=\"unterminated",
            "image/png; charset=bad value",
            "image/png;",
        ] {
            assert_eq!(
                image_mime_support(invalid),
                MimeSupport::Unsupported,
                "{invalid}"
            );
        }
    }

    fn tagged_png(profile: Vec<u8>) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut info = png::Info::with_size(1, 1);
        info.color_type = png::ColorType::Rgb;
        info.icc_profile = Some(profile.into());
        let mut writer = png::Encoder::with_info(&mut bytes, info)
            .unwrap()
            .write_header()
            .unwrap();
        writer.write_image_data(&[230, 32, 16]).unwrap();
        drop(writer);
        bytes
    }

    fn tagged_16_bit_png(profile: Vec<u8>) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut info = png::Info::with_size(1, 1);
        info.color_type = png::ColorType::Rgba;
        info.bit_depth = png::BitDepth::Sixteen;
        info.icc_profile = Some(profile.into());
        let mut writer = png::Encoder::with_info(&mut bytes, info)
            .unwrap()
            .write_header()
            .unwrap();
        // Components are deliberately repeated bytes, so PNG's 16→8
        // normalization has the same result as the former `to_rgba8()` path.
        writer
            .write_image_data(&[0, 0, 128, 128, 255, 255, 128, 128])
            .unwrap();
        drop(writer);
        bytes
    }

    fn indexed_png_with_transparency() -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut info = png::Info::with_size(2, 1);
        info.color_type = png::ColorType::Indexed;
        info.palette = Some(vec![10, 20, 30, 40, 50, 60].into());
        info.trns = Some(vec![255, 0].into());
        let mut writer = png::Encoder::with_info(&mut bytes, info)
            .unwrap()
            .write_header()
            .unwrap();
        writer.write_image_data(&[0, 1]).unwrap();
        drop(writer);
        bytes
    }

    fn tagged_jpeg(profile: Vec<u8>) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut bytes);
        encoder.set_icc_profile(profile).unwrap();
        encoder
            .write_image(&[230, 32, 16], 1, 1, ExtendedColorType::Rgb8)
            .unwrap();
        bytes
    }

    fn two_frame_gif() -> Vec<u8> {
        let first = RgbaImage::from_raw(
            2,
            2,
            vec![
                230, 32, 16, 255, // opaque red
                0, 0, 0, 0, // transparent
                10, 20, 30, 255, // opaque dark blue
                0, 0, 0, 0, // transparent
            ],
        )
        .expect("RGBA dimensions match the sample pixels");
        let second = RgbaImage::from_raw(
            2,
            2,
            vec![
                0, 96, 255, 255, 0, 96, 255, 255, 0, 96, 255, 255, 0, 96, 255, 255,
            ],
        )
        .expect("RGBA dimensions match the sample pixels");
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        encoder
            .encode_frames([Frame::new(first), Frame::new(second)])
            .expect("GIF sample encodes");
        drop(encoder);
        bytes
    }

    fn tagged_lossless_webp(profile: Vec<u8>) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut bytes);
        encoder.set_icc_profile(profile).unwrap();
        encoder
            .write_image(
                &[230, 32, 16, 255, 0, 0, 0, 0],
                2,
                1,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        bytes
    }

    fn exif_oriented_lossless_webp() -> Vec<u8> {
        // Little-endian TIFF with one IFD0 Orientation (0x0112) entry set to
        // 6, Rotate90.
        const EXIF_ORIENTATION_ROTATE_90: &[u8] = &[
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0,
            0,
        ];
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut bytes);
        encoder
            .set_exif_metadata(EXIF_ORIENTATION_ROTATE_90.to_vec())
            .unwrap();
        encoder
            .write_image(
                &[230, 32, 16, 255, 10, 20, 30, 255],
                2,
                1,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        bytes
    }

    fn exif_oriented_png() -> Vec<u8> {
        fn push_u16(bytes: &mut Vec<u8>, value: u16) {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        fn push_u32(bytes: &mut Vec<u8>, value: u32) {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        fn push_entry(bytes: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
            push_u16(bytes, tag);
            push_u16(bytes, field_type);
            push_u32(bytes, count);
            push_u32(bytes, value);
        }

        // Big-endian TIFF: IFD0 combines Orientation=6 with 144dpi density;
        // the EXIF IFD declares a 2×1 preferred CSS size for 4×2 samples.
        const X_RESOLUTION_OFFSET: u32 = 74;
        const Y_RESOLUTION_OFFSET: u32 = 82;
        const EXIF_IFD_OFFSET: u32 = 90;
        let mut exif = Vec::new();
        exif.extend_from_slice(b"MM\0*");
        push_u32(&mut exif, 8);
        push_u16(&mut exif, 5);
        push_entry(&mut exif, 0x0112, 3, 1, 6 << 16);
        push_entry(&mut exif, 0x011a, 5, 1, X_RESOLUTION_OFFSET);
        push_entry(&mut exif, 0x011b, 5, 1, Y_RESOLUTION_OFFSET);
        push_entry(&mut exif, 0x0128, 3, 1, 2 << 16);
        push_entry(&mut exif, 0x8769, 4, 1, EXIF_IFD_OFFSET);
        push_u32(&mut exif, 0);
        for _ in 0..2 {
            push_u32(&mut exif, 144);
            push_u32(&mut exif, 1);
        }
        push_u16(&mut exif, 2);
        push_entry(&mut exif, 0xa002, 3, 1, 2 << 16);
        push_entry(&mut exif, 0xa003, 3, 1, 1 << 16);
        push_u32(&mut exif, 0);
        let mut bytes = Vec::new();
        let mut info = png::Info::with_size(4, 2);
        info.color_type = png::ColorType::Rgba;
        info.exif_metadata = Some(exif.into());
        let mut writer = png::Encoder::with_info(&mut bytes, info)
            .expect("PNG encoder accepts EXIF metadata")
            .write_header()
            .expect("PNG header encodes");
        writer
            .write_image_data(&[
                230, 32, 16, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 0, 255, 255,
                255, 255, 0, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255,
            ])
            .expect("PNG sample encodes");
        drop(writer);
        bytes
    }

    /// Relocate the PNG's single eXIf chunk after image data, retaining its
    /// original bytes and CRC. This makes a valid fixture for CSS Images'
    /// late-metadata rule without relying on files outside this crate.
    fn move_png_exif_after_idat(mut bytes: Vec<u8>) -> Vec<u8> {
        let mut offset = 8;
        let exif_range = loop {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let chunk_end = offset + 12 + length;
            if &bytes[offset + 4..offset + 8] == b"eXIf" {
                break offset..chunk_end;
            }
            offset = chunk_end;
        };
        let exif = bytes[exif_range.clone()].to_vec();
        bytes.drain(exif_range);
        let mut offset = 8;
        loop {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            if &bytes[offset + 4..offset + 8] == b"IEND" {
                bytes.splice(offset..offset, exif);
                return bytes;
            }
            offset += 12 + length;
        }
    }

    /// Build a JPEG container with the EXIF fields used by HTML's
    /// density-corrected image sizing algorithm.
    fn density_exif_jpeg(
        preferred_size: (u32, u32),
        resolution: (u32, u32),
        resolution_unit: u16,
    ) -> Vec<u8> {
        fn push_u16(bytes: &mut Vec<u8>, value: u16) {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        fn push_u32(bytes: &mut Vec<u8>, value: u32) {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        fn push_entry(bytes: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
            push_u16(bytes, tag);
            push_u16(bytes, field_type);
            push_u32(bytes, count);
            push_u32(bytes, value);
        }

        // Big-endian TIFF: IFD0 holds resolutions and the EXIF IFD pointer;
        // the child EXIF IFD holds PixelX/YDimension. The fixed offsets keep
        // the fixture readable without a separate test-only TIFF writer.
        const X_RESOLUTION_OFFSET: u32 = 62;
        const Y_RESOLUTION_OFFSET: u32 = 70;
        const EXIF_IFD_OFFSET: u32 = 78;
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"MM\0*");
        push_u32(&mut tiff, 8);
        push_u16(&mut tiff, 4);
        push_entry(&mut tiff, 0x011a, 5, 1, X_RESOLUTION_OFFSET);
        push_entry(&mut tiff, 0x011b, 5, 1, Y_RESOLUTION_OFFSET);
        push_entry(&mut tiff, 0x0128, 3, 1, u32::from(resolution_unit) << 16);
        push_entry(&mut tiff, 0x8769, 4, 1, EXIF_IFD_OFFSET);
        push_u32(&mut tiff, 0);
        for dimension in [resolution.0, resolution.1] {
            push_u32(&mut tiff, dimension);
            push_u32(&mut tiff, 1);
        }
        push_u16(&mut tiff, 2);
        // WPT's JPEG fixtures encode the preferred dimensions as SHORT, a
        // valid EXIF integer representation alongside LONG.
        assert!(preferred_size.0 <= u32::from(u16::MAX));
        assert!(preferred_size.1 <= u32::from(u16::MAX));
        push_entry(&mut tiff, 0xa002, 3, 1, preferred_size.0 << 16);
        push_entry(&mut tiff, 0xa003, 3, 1, preferred_size.1 << 16);
        push_u32(&mut tiff, 0);

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        push_u16(&mut jpeg, (2 + 6 + tiff.len()) as u16);
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }

    #[test]
    fn exif_density_selects_only_html_consistent_natural_dimensions() {
        let pixels = RasterPixelSize::new(100, 50);
        let natural_size = |preferred_size, resolution, resolution_unit| {
            exif_preferred_natural_size(
                &density_exif_jpeg(preferred_size, resolution, resolution_unit),
                pixels,
            )
        };

        assert_eq!(
            natural_size((50, 25), (144, 144), 2),
            Some(CssPixelSize::new(50, 25)),
            "high-density EXIF dimensions are honored"
        );
        assert_eq!(
            natural_size((200, 100), (36, 36), 2),
            Some(CssPixelSize::new(200, 100)),
            "low-density EXIF dimensions are honored"
        );
        assert_eq!(
            natural_size((50, 100), (144, 36), 2),
            Some(CssPixelSize::new(50, 100)),
            "X and Y density are independently validated"
        );

        assert_eq!(natural_size((51, 25), (144, 144), 2), None);
        assert_eq!(natural_size((50, 25), (0, 144), 2), None);
        assert_eq!(natural_size((50, 25), (144, 144), 3), None);
        assert_eq!(
            exif_preferred_natural_size(&[0xff, 0xd8, 0xff, 0xd9], pixels),
            None
        );
    }

    #[test]
    fn image_metadata_relabels_fallback_pixels_and_rotates_both_axis_sizes() {
        let pixels = RasterPixelSize::new(100, 50);
        assert_eq!(
            ImageMetadata::from_pixel_size(pixels),
            ImageMetadata {
                pixel_size: pixels,
                natural_size: CssPixelSize::new(100, 50),
            }
        );

        let metadata = ImageMetadata {
            pixel_size: pixels,
            natural_size: CssPixelSize::new(50, 100),
        };
        assert_eq!(
            metadata.with_orientation(Orientation::Rotate90, RasterOrientationPolicy::FromImage),
            ImageMetadata {
                pixel_size: RasterPixelSize::new(50, 100),
                natural_size: CssPixelSize::new(100, 50),
            }
        );
        assert_eq!(
            metadata.with_orientation(Orientation::Rotate90, RasterOrientationPolicy::Encoded),
            metadata
        );
    }

    /// A 2x2 lossy VP8 WebP generated with ImageMagick.
    const LOSSY_WEBP: &[u8] = &[
        0x52, 0x49, 0x46, 0x46, 0x3c, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38,
        0x20, 0x30, 0x00, 0x00, 0x00, 0xd0, 0x01, 0x00, 0x9d, 0x01, 0x2a, 0x02, 0x00, 0x02, 0x00,
        0x02, 0x00, 0x34, 0x25, 0xa0, 0x02, 0x74, 0xba, 0x01, 0xf8, 0x00, 0x03, 0xb0, 0x00, 0xfe,
        0xf0, 0xc4, 0x0b, 0xff, 0x20, 0xb9, 0x61, 0x75, 0xc8, 0xd7, 0xff, 0x20, 0x3f, 0xe4, 0x07,
        0xfc, 0x80, 0xff, 0xf8, 0xf2, 0x00, 0x00, 0x00,
    ];

    /// A two-frame lossless WebP: red/black first frame, blue second frame.
    fn animated_webp() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode("UklGRoYAAABXRUJQVlA4WAoAAAACAAAAAQAAAAAAQU5JTQYAAAD/////AABBTk1GKgAAAAAAAAAAAAEAAAAAAGQAAAJWUDhMEgAAAC8BAAAADzAgYz4Q8x94yIj+B0FOTUYoAAAAAAAAAAAAAQAAAAAAZAAAAFZQOEwQAAAALwEAAAAHULDof/8DEdH/AA==")
            .expect("animated WebP fixture is valid base64")
    }

    #[test]
    fn jpeg_passthrough_requires_rgb_samples_and_no_applied_orientation() {
        let bytes =
            tagged_jpeg(crate::color::icc_profile_bytes(crate::css::CssColorSpace::Srgb).unwrap());
        let mut store = DocumentImageStore::default();
        let (image_id, _) = store
            .insert(
                Rc::from(bytes.into_boxed_slice()),
                RasterOrientationPolicy::Encoded,
            )
            .unwrap();
        assert!(store.direct_jpeg(image_id).is_some());

        let ImageAsset::Encoded(image) = &mut store.images[image_id.index()] else {
            panic!("inserted image must be encoded");
        };
        image.orientation_policy = RasterOrientationPolicy::FromImage;
        image.source_orientation = Orientation::Rotate90;
        assert!(store.direct_jpeg(image_id).is_none());
    }

    #[test]
    fn png_and_jpeg_retain_valid_embedded_rgb_profiles() {
        let profile =
            crate::color::icc_profile_bytes(crate::css::CssColorSpace::DisplayP3).unwrap();
        for bytes in [tagged_png(profile.clone()), tagged_jpeg(profile.clone())] {
            let (_, _, color_space, _, _) =
                image_metadata(&bytes, RasterOrientationPolicy::Encoded).unwrap();
            assert_eq!(
                color_space,
                crate::color::RasterColorSpace::EmbeddedRgb(Rc::from(profile.clone()))
            );
        }
    }

    #[test]
    fn lossless_webp_retains_alpha_and_embedded_rgb_profile() {
        let profile =
            crate::color::icc_profile_bytes(crate::css::CssColorSpace::DisplayP3).unwrap();
        let bytes = tagged_lossless_webp(profile.clone());
        let (metadata, format, color_space, _, _) =
            image_metadata(&bytes, RasterOrientationPolicy::Encoded).unwrap();
        assert_eq!(format, EncodedImageFormat::Image(image::ImageFormat::WebP));
        assert_eq!(
            color_space,
            crate::color::RasterColorSpace::EmbeddedRgb(Rc::from(profile))
        );

        let mut store = DocumentImageStore::default();
        let (image_id, stored_metadata) = store
            .insert(
                Rc::from(bytes.into_boxed_slice()),
                RasterOrientationPolicy::Encoded,
            )
            .expect("lossless WebP image is recognized");
        assert_eq!(stored_metadata, metadata);
        assert!(store.direct_jpeg(image_id).is_none());
        let raster = store
            .with_rasterized(image_id, |raster| raster)
            .expect("lossless WebP rasterizes");
        assert_eq!(raster.rgb, vec![230, 32, 16, 0, 0, 0]);
        assert_eq!(raster.alpha, Some(vec![255, 0]));
    }

    #[test]
    fn lossy_webp_rasterizes_as_an_opaque_static_image() {
        let mut store = DocumentImageStore::default();
        let (image_id, metadata) = store
            .insert(Rc::from(LOSSY_WEBP), RasterOrientationPolicy::Encoded)
            .expect("lossy WebP image is recognized");
        assert_eq!(
            metadata,
            ImageMetadata::from_pixel_size(RasterPixelSize::new(2, 2))
        );
        assert!(store.direct_jpeg(image_id).is_none());
        let raster = store
            .with_rasterized(image_id, |raster| raster)
            .expect("lossy WebP rasterizes");
        assert_eq!(raster.alpha, None);
        assert_eq!(raster.rgb.len(), 2 * 2 * 3);
        assert!(
            raster
                .rgb
                .as_chunks::<3>()
                .0
                .iter()
                .all(|pixel| pixel == &raster.rgb[0..3])
        );
    }

    #[test]
    fn jpeg_xl_is_recognized_and_rasterized() {
        // 3×3 lossless RGBA JPEG XL image from WPT's JPEG XL resources. Keep
        // the fixture inline so this crate's test suite is self-contained.
        let bytes = base64::engine::general_purpose::STANDARD
            .decode("/woQELCgcm8VRQgQEADIAEsYixUKJYLsdkD5lm3pAEP51y/mVkAqNRs+7Y8+vyhKKU5gn/q/jPolL15LnJgIAAAA")
            .unwrap();
        let mut store = DocumentImageStore::default();
        let (image_id, metadata) = store
            .insert(bytes.into(), RasterOrientationPolicy::Encoded)
            .unwrap();
        assert_eq!(
            metadata,
            ImageMetadata::from_pixel_size(RasterPixelSize::new(3, 3))
        );
        assert!(store.direct_jpeg(image_id).is_none());
        let raster = store.with_rasterized(image_id, |raster| raster).unwrap();
        assert_eq!(raster.metadata, metadata);
        assert_eq!(raster.rgb.len(), 3 * 3 * 3);
        assert_eq!(raster.alpha.as_ref().map(Vec::len), Some(3 * 3));
    }

    #[test]
    fn high_depth_dynamic_decoder_output_uses_sixteen_bit_pdf_samples() {
        let source =
            image::ImageBuffer::from_raw(1, 1, vec![0x0102_u16, 0x8081, 0xfeff, 0x1234]).unwrap();
        let (depth, rgb, alpha) =
            raster_samples_from_dynamic_image(image::DynamicImage::ImageRgba16(source)).unwrap();

        assert_eq!(depth, RasterSampleDepth::Sixteen);
        assert_eq!(rgb, vec![0x01, 0x02, 0x80, 0x81, 0xfe, 0xff]);
        assert_eq!(alpha, Some(vec![0x12, 0x34]));
    }

    #[test]
    fn floating_point_dynamic_decoder_output_is_rejected() {
        let source = image::ImageBuffer::from_raw(1, 1, vec![0.0_f32, 0.5, 1.0, 1.0]).unwrap();
        assert!(
            raster_samples_from_dynamic_image(image::DynamicImage::ImageRgba32F(source)).is_none()
        );
    }

    #[test]
    fn png_palette_transparency_expands_to_rgb_and_alpha() {
        let decoded = decode_png_samples(&indexed_png_with_transparency(), MAX_PNG_DECODER_BYTES)
            .expect("valid indexed PNG decodes");
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.rgb, vec![10, 20, 30, 40, 50, 60]);
        assert_eq!(decoded.alpha, Some(vec![255, 0]));
    }

    #[test]
    fn malformed_png_is_rejected_before_document_registration() {
        let mut store = DocumentImageStore::default();
        assert!(
            store
                .insert(
                    Rc::from(&PNG_SIGNATURE[..]),
                    RasterOrientationPolicy::Encoded,
                )
                .is_none()
        );
    }

    #[test]
    fn png_gamma_and_chromaticities_preserve_the_declared_rgb_encoding() {
        // Self-contained 3×3 PNG from WPT's JPEG XL 8-bit reference. It has
        // no iCCP profile, but its gAMA=45455 and cHRM declarations describe
        // sRGB primaries with a 2.2 decoding exponent.
        let bytes = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAMAAAADCAIAAADZSiLoAAAABGdBTUEAALGPC/xhBQAAACBjSFJNAAB6JgAAgIQAAPoAAACA6QAAdTAAAOpgAAA6mAAAF3AbHJp/AAAAI0lEQVQImQXBAREAMAgEIHYrYhSjWdUkLwiC/Ndtq6wkM4MDjYQKASYFwFoAAAAASUVORK5CYII=")
            .unwrap();
        let (metadata, format, color_space, _, _) =
            image_metadata(&bytes, RasterOrientationPolicy::Encoded).unwrap();
        assert_eq!(
            metadata,
            ImageMetadata::from_pixel_size(RasterPixelSize::new(3, 3))
        );
        assert_eq!(format, EncodedImageFormat::Png);
        assert!(matches!(
            color_space,
            crate::color::RasterColorSpace::EmbeddedRgb(_)
        ));

        let mut store = DocumentImageStore::default();
        let (image_id, _) = store
            .insert(bytes.into(), RasterOrientationPolicy::Encoded)
            .unwrap();
        let raster = store.with_rasterized(image_id, |raster| raster).unwrap();
        assert_eq!(raster.sample_depth, RasterSampleDepth::Eight);
        assert_eq!(raster.alpha, None);
        assert_eq!(
            raster.rgb,
            vec![
                255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 64, 64, 64, 128, 64, 64, 64, 128, 255, 255,
                255, 128, 128, 128, 0, 0, 0,
            ]
        );
    }

    #[test]
    fn sixteen_bit_pngs_preserve_components_at_the_shared_raster_boundary() {
        let profile =
            crate::color::icc_profile_bytes(crate::css::CssColorSpace::DisplayP3).unwrap();
        let bytes = tagged_16_bit_png(profile.clone());
        let (_, format, color_space, _, _) =
            image_metadata(&bytes, RasterOrientationPolicy::Encoded).unwrap();
        assert_eq!(format, EncodedImageFormat::Png);
        assert_eq!(
            color_space,
            crate::color::RasterColorSpace::EmbeddedRgb(Rc::from(profile))
        );

        let mut store = DocumentImageStore::default();
        let (image_id, _) = store
            .insert(bytes.into(), RasterOrientationPolicy::Encoded)
            .unwrap();
        let raster = store.with_rasterized(image_id, |raster| raster).unwrap();
        assert_eq!(raster.sample_depth, RasterSampleDepth::Sixteen);
        assert_eq!(raster.rgb, vec![0, 0, 128, 128, 255, 255]);
        assert_eq!(raster.alpha, Some(vec![128, 128]));
    }

    #[test]
    fn png_orientation_moves_complete_sixteen_bit_components() {
        let decoded = DecodedPngSamples {
            width: 2,
            height: 1,
            sample_depth: RasterSampleDepth::Sixteen,
            rgb: vec![0, 1, 0, 2, 0, 3, 1, 1, 1, 2, 1, 3],
            alpha: Some(vec![0xaa, 0xbb, 0xcc, 0xdd]),
        };
        let oriented = apply_png_orientation(decoded, Orientation::Rotate90).unwrap();

        assert_eq!((oriented.width, oriented.height), (1, 2));
        assert_eq!(oriented.rgb, vec![0, 1, 0, 2, 0, 3, 1, 1, 1, 2, 1, 3]);
        assert_eq!(oriented.alpha, Some(vec![0xaa, 0xbb, 0xcc, 0xdd]));
    }

    #[test]
    fn webp_applies_embedded_exif_orientation() {
        let bytes = exif_oriented_lossless_webp();
        let mut store = DocumentImageStore::default();
        let (_, raw_metadata) = store
            .insert(
                Rc::from(bytes.clone().into_boxed_slice()),
                RasterOrientationPolicy::Encoded,
            )
            .expect("raw WebP image is recognized");
        let (image_id, oriented_metadata) = store
            .insert(
                Rc::from(bytes.into_boxed_slice()),
                RasterOrientationPolicy::FromImage,
            )
            .expect("oriented WebP image is recognized");
        assert_eq!(
            raw_metadata,
            ImageMetadata::from_pixel_size(RasterPixelSize::new(2, 1))
        );
        assert_eq!(
            oriented_metadata,
            ImageMetadata::from_pixel_size(RasterPixelSize::new(1, 2))
        );
        let raster = store
            .with_rasterized(image_id, |raster| raster)
            .expect("oriented WebP rasterizes");
        assert_eq!(raster.metadata, oriented_metadata);
        assert_eq!(raster.rgb, vec![230, 32, 16, 10, 20, 30]);
        assert_eq!(raster.alpha, None);
    }

    #[test]
    fn png_exif_after_image_data_does_not_affect_layout_or_pixels() {
        let before_image_data = exif_oriented_png();
        let after_image_data = move_png_exif_after_idat(before_image_data.clone());
        assert!(png_layout_exif_is_eligible(&before_image_data));
        assert!(!png_layout_exif_is_eligible(&after_image_data));

        let mut store = DocumentImageStore::default();
        let (before_id, before_metadata) = store
            .insert(
                Rc::from(before_image_data.into_boxed_slice()),
                RasterOrientationPolicy::FromImage,
            )
            .expect("PNG with early eXIf is recognized");
        let (after_id, after_metadata) = store
            .insert(
                Rc::from(after_image_data.into_boxed_slice()),
                RasterOrientationPolicy::FromImage,
            )
            .expect("PNG with late eXIf is recognized");

        assert_eq!(before_metadata.pixel_size, RasterPixelSize::new(2, 4));
        assert_eq!(before_metadata.natural_size, CssPixelSize::new(1, 2));
        assert_eq!(after_metadata.pixel_size, RasterPixelSize::new(4, 2));
        assert_eq!(after_metadata.natural_size, CssPixelSize::new(4, 2));
        let before_raster = store.with_rasterized(before_id, |raster| raster).unwrap();
        let after_raster = store.with_rasterized(after_id, |raster| raster).unwrap();
        assert_eq!(before_raster.metadata, before_metadata);
        assert_eq!(after_raster.metadata, after_metadata);
        assert_ne!(before_raster.rgb, after_raster.rgb);
    }

    #[test]
    fn animated_webp_uses_its_first_composited_frame() {
        let bytes = animated_webp();
        let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(&bytes)).unwrap();
        assert!(decoder.has_animation());
        let first_frame = decoder
            .into_frames()
            .next()
            .expect("animated WebP has a first frame")
            .expect("animated WebP first frame decodes");
        assert_eq!(first_frame.buffer().dimensions(), (2, 1));
        assert_eq!(
            first_frame.buffer().as_raw(),
            &[230, 32, 16, 255, 0, 0, 0, 255]
        );
        let mut store = DocumentImageStore::default();
        let (image_id, metadata) = store
            .insert(
                Rc::from(bytes.into_boxed_slice()),
                RasterOrientationPolicy::FromImage,
            )
            .expect("animated WebP image is recognized");
        assert_eq!(
            metadata,
            ImageMetadata::from_pixel_size(RasterPixelSize::new(2, 1))
        );
        assert!(store.direct_jpeg(image_id).is_none());

        let raster = store
            .with_rasterized(image_id, |raster| raster)
            .expect("animated WebP first frame rasterizes");
        assert_eq!(raster.rgb, vec![230, 32, 16, 0, 0, 0]);
        assert_eq!(raster.alpha, None);
        assert!(
            !raster.rgb.as_chunks::<3>().0.contains(&[0, 96, 255]),
            "later WebP frames must not contribute samples: {raster:?}"
        );
    }

    #[test]
    fn animated_gif_uses_its_first_frame_on_the_logical_screen() {
        let mut store = DocumentImageStore::default();
        let (image_id, metadata) = store
            .insert(
                Rc::from(two_frame_gif().into_boxed_slice()),
                RasterOrientationPolicy::FromImage,
            )
            .expect("GIF image is recognized");

        assert_eq!(
            metadata,
            ImageMetadata::from_pixel_size(RasterPixelSize::new(2, 2))
        );
        assert!(store.direct_jpeg(image_id).is_none());

        let raster = store
            .with_rasterized(image_id, |raster| raster)
            .expect("GIF first frame rasterizes");
        assert_eq!(raster.metadata, metadata);
        assert_eq!(raster.alpha, Some(vec![255, 0, 255, 0]));
        assert_eq!(&raster.rgb[0..3], &[230, 32, 16]);
        assert_eq!(&raster.rgb[6..9], &[10, 20, 30]);
        assert!(
            !raster.rgb.as_chunks::<3>().0.contains(&[0, 96, 255]),
            "later GIF frames must not contribute samples: {raster:?}"
        );
    }

    #[test]
    fn malformed_and_non_rgb_embedded_profiles_fall_back_to_srgb() {
        let non_rgb = moxcms::ColorProfile::new_lab().encode().unwrap();
        for profile in [vec![1, 2, 3], non_rgb] {
            let (_, _, color_space, _, _) =
                image_metadata(&tagged_png(profile), RasterOrientationPolicy::Encoded).unwrap();
            assert_eq!(color_space, crate::color::RasterColorSpace::SRGB);
        }
    }

    #[test]
    fn generated_image_keys_distinguish_deferred_length_expression_terms() {
        let mut em = GeneratedImageKeyBuilder::default();
        let mut rem = GeneratedImageKeyBuilder::default();
        let mut zero_percent = GeneratedImageKeyBuilder::default();
        let mut zero_length = GeneratedImageKeyBuilder::default();

        em.length_percentage(crate::css::ComputedLengthPercentage::from_em(1.0));
        rem.length_percentage(crate::css::ComputedLengthPercentage::from_rem(1.0));
        zero_percent.length_percentage(crate::css::ComputedLengthPercentage::from_percent(0.0));
        zero_length.length_percentage(crate::css::ComputedLengthPercentage::ZERO);

        assert_ne!(em.output, rem.output);
        assert_ne!(zero_percent.output, zero_length.output);
    }
}
