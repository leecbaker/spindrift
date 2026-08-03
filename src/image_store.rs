//! Document-owned raster-image sources and transient PDF rasterization.
//!
//! CSS layout needs intrinsic image dimensions, but PDF emission is the first
//! stage that needs decoded RGB samples. Keeping the encoded source here makes
//! rendered documents self-contained without retaining one expanded raster for
//! every image use.

use image::metadata::Orientation;
use image::{AnimationDecoder, ColorType, ImageDecoder, ImageReader};
use std::collections::HashMap;
use std::io::Cursor;
use std::rc::Rc;
use url::Url;

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

/// Classify a CSS `type()` descriptor against Quire's actual image decoders.
pub(crate) fn image_mime_support(mime_type: &str) -> MimeSupport {
    let Some(mime_type) = declared_mime_essence(mime_type) else {
        return MimeSupport::Unsupported;
    };
    let supported = matches!(mime_type.as_str(), "image/svg+xml" | "image/jxl")
        || image::ImageFormat::from_mime_type(&mime_type).is_some_and(|format| {
            matches!(
                format,
                image::ImageFormat::Png
                    | image::ImageFormat::Jpeg
                    | image::ImageFormat::Gif
                    | image::ImageFormat::WebP
            )
        });
    if supported {
        MimeSupport::Supported
    } else {
        MimeSupport::Unsupported
    }
}

/// Return the normalized media-type essence from a `type()` descriptor.
///
/// Parameters do not affect decoder selection, but the essence must retain
/// the RFC media-type token shape so malformed descriptors never accidentally
/// match a decoder. CSS Images treats an unknown type as an unsupported
/// candidate rather than making the surrounding `image-set()` syntactically
/// invalid.
fn declared_mime_essence(value: &str) -> Option<String> {
    let value = value.trim_matches(|character: char| character.is_ascii_whitespace());
    let (essence, parameters) = value
        .split_once(';')
        .map_or((value, None), |(head, tail)| (head, Some(tail)));
    if let Some(parameters) = parameters {
        for parameter in parameters.split(';') {
            let parameter =
                parameter.trim_matches(|character: char| character.is_ascii_whitespace());
            let (name, value) = parameter.split_once('=')?;
            if name.is_empty()
                || !name.bytes().all(is_mime_token_character)
                || !valid_mime_parameter_value(
                    value.trim_matches(|character: char| character.is_ascii_whitespace()),
                )
            {
                return None;
            }
        }
    }
    let (type_, subtype) = essence.split_once('/')?;
    if type_.is_empty()
        || subtype.is_empty()
        || subtype.contains('/')
        || !type_.bytes().all(is_mime_token_character)
        || !subtype.bytes().all(is_mime_token_character)
    {
        return None;
    }
    Some(essence.to_ascii_lowercase())
}

/// Validate the token-or-quoted-string parameter grammar used by the
/// MIME Sniffing standard's `is a valid MIME type string` algorithm.
/// <https://mimesniff.spec.whatwg.org/#valid-mime-type-string>
fn valid_mime_parameter_value(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if let Some(quoted) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        let mut escaped = false;
        for byte in quoted.bytes() {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte.is_ascii_control() || byte == b'"' {
                return false;
            }
        }
        return !escaped;
    }
    value.bytes().all(is_mime_token_character)
}

fn is_mime_token_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

pub(crate) fn supports_declared_image_mime_type(mime_type: &str) -> bool {
    image_mime_support(mime_type) == MimeSupport::Supported
}

/// Stable, document-local reference to an image source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ImageId(u32);

impl ImageId {
    const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageMetadata {
    pub(crate) pixel_width: u32,
    pub(crate) pixel_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EncodedImage {
    bytes: Rc<[u8]>,
    format: EncodedImageFormat,
    metadata: ImageMetadata,
    apply_orientation: bool,
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
                key.u32(metadata.pixel_width);
                key.u32(metadata.pixel_height);
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
                key.u32(metadata.pixel_width);
                key.u32(metadata.pixel_height);
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
}

impl DocumentImageStore {
    pub(crate) fn resolve_url_with_orientation(
        &mut self,
        url: Url,
        bytes: Rc<[u8]>,
        apply_orientation: bool,
    ) -> Option<(ImageId, ImageMetadata)> {
        let existing = if apply_orientation {
            self.oriented_urls.get(&url).cloned()
        } else {
            self.urls.get(&url).cloned()
        };
        if let Some(id) = existing {
            return Some((id, self.metadata(id)?));
        }
        let (id, metadata) = self.insert(bytes, apply_orientation)?;
        if apply_orientation {
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
        apply_orientation: bool,
    ) -> Option<(ImageId, ImageMetadata)> {
        let existing = if apply_orientation {
            self.oriented_data_urls.get(source).cloned()
        } else {
            self.data_urls.get(source).cloned()
        };
        if let Some(id) = existing {
            return Some((id, self.metadata(id)?));
        }
        let (id, metadata) = self.insert(bytes, apply_orientation)?;
        if apply_orientation {
            self.oriented_data_urls.insert(source.to_owned(), id);
        } else {
            self.data_urls.insert(source.to_owned(), id);
        }
        Some((id, metadata))
    }

    fn insert(
        &mut self,
        bytes: Rc<[u8]>,
        apply_orientation: bool,
    ) -> Option<(ImageId, ImageMetadata)> {
        let (metadata, format, color_space, source_orientation, direct_jpeg) =
            image_metadata(&bytes, apply_orientation)?;
        let id = ImageId(u32::try_from(self.images.len()).ok()?);
        self.images.push(ImageAsset::Encoded(EncodedImage {
            bytes,
            format,
            metadata,
            apply_orientation,
            source_orientation,
            direct_jpeg,
            color_space,
        }));
        Some((id, metadata))
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
            && !(image.apply_orientation && image.source_orientation != Orientation::NoTransforms))
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
        // GIF and animated WebP expose their logical screen dimensions and
        // contribute only their first image frame. This deliberately gives
        // animated images a stable PDF representation: frame timing, looping,
        // disposal, and later-frame compositing are not part of a static PDF
        // image.
        // https://www.w3.org/TR/css-images-3/#image-notation
        let (mut decoded, orientation) = if image.format
            == EncodedImageFormat::Image(image::ImageFormat::WebP)
        {
            decode_webp_first_frame(&image.bytes)?
        } else if image.format == EncodedImageFormat::JpegXl {
            let mut decoder =
                jxl_oxide::integration::JxlDecoder::new(Cursor::new(image.bytes.as_ref())).ok()?;
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            (
                image::DynamicImage::from_decoder(decoder).ok()?,
                orientation,
            )
        } else {
            let EncodedImageFormat::Image(format) = image.format else {
                unreachable!("JPEG XL is handled by jxl-oxide above");
            };
            let mut decoder = ImageReader::with_format(Cursor::new(image.bytes.as_ref()), format)
                .into_decoder()
                .ok()?;
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            (
                image::DynamicImage::from_decoder(decoder).ok()?,
                orientation,
            )
        };
        if image.apply_orientation {
            decoded.apply_orientation(orientation);
        }
        let rgba = decoded.to_rgba8();
        let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(rgba.len() / 4);
        let mut has_alpha = false;
        for pixel in rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&pixel[..3]);
            alpha.push(pixel[3]);
            has_alpha |= pixel[3] < 255;
        }
        Some(RasterImage {
            metadata: image.metadata,
            color_space: image.color_space.clone(),
            rgb,
            alpha: has_alpha.then_some(alpha),
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
                write(&image.metadata.pixel_width.to_be_bytes());
                write(&image.metadata.pixel_height.to_be_bytes());
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

fn image_metadata(
    bytes: &[u8],
    apply_orientation: bool,
) -> Option<(
    ImageMetadata,
    EncodedImageFormat,
    crate::color::RasterColorSpace,
    Orientation,
    bool,
)> {
    if is_jpeg_xl(bytes) {
        return jpeg_xl_metadata(bytes, apply_orientation);
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
        Ok(None) => png_color_space(bytes, format).unwrap_or_else(|| {
            log::debug!("using the sRGB fallback for an image without color metadata");
            crate::color::RasterColorSpace::SRGB
        }),
        Err(error) => {
            log::debug!(
                "using the sRGB fallback after reading an image ICC profile failed: {error}"
            );
            crate::color::RasterColorSpace::SRGB
        }
    };
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let direct_jpeg = format == image::ImageFormat::Jpeg && decoder.color_type() == ColorType::Rgb8;
    let (mut pixel_width, mut pixel_height) = decoder.dimensions();
    if apply_orientation
        && matches!(
            orientation,
            Orientation::Rotate90
                | Orientation::Rotate270
                | Orientation::Rotate90FlipH
                | Orientation::Rotate270FlipH
        )
    {
        std::mem::swap(&mut pixel_width, &mut pixel_height);
    }
    Some((
        ImageMetadata {
            pixel_width,
            pixel_height,
        },
        EncodedImageFormat::Image(format),
        color_space,
        orientation,
        direct_jpeg,
    ))
}

/// Read PNG color chunks that the generic image decoder does not expose.
///
/// An embedded `iCCP` profile is handled by `ImageDecoder::icc_profile` and
/// takes precedence. This parser only reads singleton pre-IDAT color chunks;
/// the image decoder remains responsible for validating and decoding PNG data.
/// <https://www.w3.org/TR/png-3/#11iCCP>
/// <https://www.w3.org/TR/png-3/#11gAMA>
/// <https://www.w3.org/TR/png-3/#11cHRM>
fn png_color_space(
    bytes: &[u8],
    format: image::ImageFormat,
) -> Option<crate::color::RasterColorSpace> {
    if format != image::ImageFormat::Png || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }

    let mut gamma = None;
    let mut chromaticities = None;
    let mut offset: usize = 8;
    while let Some(header) = bytes.get(offset..offset.checked_add(8)?) {
        let length = u32::from_be_bytes(header[..4].try_into().ok()?) as usize;
        let chunk_start = offset.checked_add(8)?;
        let data_end = chunk_start.checked_add(length)?;
        let chunk_end = data_end.checked_add(4)?;
        let chunk = bytes.get(chunk_start..data_end)?;
        match &header[4..] {
            b"sRGB" if chunk.len() == 1 => return Some(crate::color::RasterColorSpace::SRGB),
            b"gAMA" if chunk.len() == 4 && gamma.is_none() => {
                gamma = Some(u32::from_be_bytes(chunk.try_into().ok()?));
            }
            b"cHRM" if chunk.len() == 32 && chromaticities.is_none() => {
                chromaticities = png_chromaticities(chunk);
            }
            b"IDAT" => break,
            _ => {}
        }
        offset = chunk_end;
    }

    crate::color::png_gamma_chromaticities_profile(f64::from(gamma?) / 100_000.0, chromaticities?)
}

fn png_chromaticities(bytes: &[u8]) -> Option<crate::color::PngChromaticities> {
    let mut values = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|value| u32::from_be_bytes(*value) as f64 / 100_000.0);
    Some(crate::color::PngChromaticities {
        white_x: values.next()?,
        white_y: values.next()?,
        red_x: values.next()?,
        red_y: values.next()?,
        green_x: values.next()?,
        green_y: values.next()?,
        blue_x: values.next()?,
        blue_y: values.next()?,
    })
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
    apply_orientation: bool,
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
    let (mut pixel_width, mut pixel_height) = decoder.dimensions();
    if apply_orientation
        && matches!(
            orientation,
            Orientation::Rotate90
                | Orientation::Rotate270
                | Orientation::Rotate90FlipH
                | Orientation::Rotate270FlipH
        )
    {
        std::mem::swap(&mut pixel_width, &mut pixel_height);
    }
    Some((
        ImageMetadata {
            pixel_width,
            pixel_height,
        },
        EncodedImageFormat::JpegXl,
        color_space,
        orientation,
        false,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use image::{ExtendedColorType, Frame, ImageEncoder, RgbaImage};

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
        let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
        encoder.set_icc_profile(profile).unwrap();
        encoder
            .write_image(&[230, 32, 16], 1, 1, ExtendedColorType::Rgb8)
            .unwrap();
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
            .insert(Rc::from(bytes.into_boxed_slice()), false)
            .unwrap();
        assert!(store.direct_jpeg(image_id).is_some());

        let ImageAsset::Encoded(image) = &mut store.images[image_id.index()] else {
            panic!("inserted image must be encoded");
        };
        image.apply_orientation = true;
        image.source_orientation = Orientation::Rotate90;
        assert!(store.direct_jpeg(image_id).is_none());
    }

    #[test]
    fn png_and_jpeg_retain_valid_embedded_rgb_profiles() {
        let profile =
            crate::color::icc_profile_bytes(crate::css::CssColorSpace::DisplayP3).unwrap();
        for bytes in [tagged_png(profile.clone()), tagged_jpeg(profile.clone())] {
            let (_, _, color_space, _, _) = image_metadata(&bytes, false).unwrap();
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
        let (metadata, format, color_space, _, _) = image_metadata(&bytes, false).unwrap();
        assert_eq!(format, EncodedImageFormat::Image(image::ImageFormat::WebP));
        assert_eq!(
            color_space,
            crate::color::RasterColorSpace::EmbeddedRgb(Rc::from(profile))
        );

        let mut store = DocumentImageStore::default();
        let (image_id, stored_metadata) = store
            .insert(Rc::from(bytes.into_boxed_slice()), false)
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
            .insert(Rc::from(LOSSY_WEBP), false)
            .expect("lossy WebP image is recognized");
        assert_eq!(
            metadata,
            ImageMetadata {
                pixel_width: 2,
                pixel_height: 2,
            }
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
        let (image_id, metadata) = store.insert(bytes.into(), false).unwrap();
        assert_eq!(
            metadata,
            ImageMetadata {
                pixel_width: 3,
                pixel_height: 3
            }
        );
        assert!(store.direct_jpeg(image_id).is_none());
        let raster = store.with_rasterized(image_id, |raster| raster).unwrap();
        assert_eq!(raster.metadata, metadata);
        assert_eq!(raster.rgb.len(), 3 * 3 * 3);
        assert_eq!(raster.alpha.as_ref().map(Vec::len), Some(3 * 3));
    }

    #[test]
    fn png_gamma_and_chromaticities_preserve_the_declared_rgb_encoding() {
        // Self-contained 3×3 PNG from WPT's JPEG XL 8-bit reference. It has
        // no iCCP profile, but its gAMA=45455 and cHRM declarations describe
        // sRGB primaries with a 2.2 decoding exponent.
        let bytes = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAMAAAADCAIAAADZSiLoAAAABGdBTUEAALGPC/xhBQAAACBjSFJNAAB6JgAAgIQAAPoAAACA6QAAdTAAAOpgAAA6mAAAF3AbHJp/AAAAI0lEQVQImQXBAREAMAgEIHYrYhSjWdUkLwiC/Ndtq6wkM4MDjYQKASYFwFoAAAAASUVORK5CYII=")
            .unwrap();
        let (metadata, format, color_space, _, _) = image_metadata(&bytes, false).unwrap();
        assert_eq!(
            metadata,
            ImageMetadata {
                pixel_width: 3,
                pixel_height: 3,
            }
        );
        assert_eq!(format, EncodedImageFormat::Image(image::ImageFormat::Png));
        assert!(matches!(
            color_space,
            crate::color::RasterColorSpace::EmbeddedRgb(_)
        ));

        let mut store = DocumentImageStore::default();
        let (image_id, _) = store.insert(bytes.into(), false).unwrap();
        let raster = store.with_rasterized(image_id, |raster| raster).unwrap();
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
    fn webp_applies_embedded_exif_orientation() {
        let bytes = exif_oriented_lossless_webp();
        let mut store = DocumentImageStore::default();
        let (_, raw_metadata) = store
            .insert(Rc::from(bytes.clone().into_boxed_slice()), false)
            .expect("raw WebP image is recognized");
        let (image_id, oriented_metadata) = store
            .insert(Rc::from(bytes.into_boxed_slice()), true)
            .expect("oriented WebP image is recognized");
        assert_eq!(
            raw_metadata,
            ImageMetadata {
                pixel_width: 2,
                pixel_height: 1,
            }
        );
        assert_eq!(
            oriented_metadata,
            ImageMetadata {
                pixel_width: 1,
                pixel_height: 2,
            }
        );
        let raster = store
            .with_rasterized(image_id, |raster| raster)
            .expect("oriented WebP rasterizes");
        assert_eq!(raster.metadata, oriented_metadata);
        assert_eq!(raster.rgb, vec![230, 32, 16, 10, 20, 30]);
        assert_eq!(raster.alpha, None);
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
            .insert(Rc::from(bytes.into_boxed_slice()), true)
            .expect("animated WebP image is recognized");
        assert_eq!(
            metadata,
            ImageMetadata {
                pixel_width: 2,
                pixel_height: 1,
            }
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
            .insert(Rc::from(two_frame_gif().into_boxed_slice()), true)
            .expect("GIF image is recognized");

        assert_eq!(
            metadata,
            ImageMetadata {
                pixel_width: 2,
                pixel_height: 2,
            }
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
            let (_, _, color_space, _, _) = image_metadata(&tagged_png(profile), false).unwrap();
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
