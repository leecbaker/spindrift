//! Document-owned raster-image sources and transient PDF rasterization.
//!
//! CSS layout needs intrinsic image dimensions, but PDF emission is the first
//! stage that needs decoded RGB samples. Keeping the encoded source here makes
//! rendered documents self-contained without retaining one expanded raster for
//! every image use.

use image::metadata::Orientation;
use image::{ImageDecoder, ImageReader};
use std::collections::HashMap;
use std::io::Cursor;
use std::rc::Rc;
use url::Url;

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
    format: image::ImageFormat,
    metadata: ImageMetadata,
    apply_orientation: bool,
    color_space: crate::color::RasterColorSpace,
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
        size: crate::document::PaintSize,
        metadata: ImageMetadata,
    },
    Radial {
        gradient: crate::css::RadialGradient,
        size: crate::document::PaintSize,
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

    /// Resolve the component space that gradient rasterization will use
    /// without materializing its pixels. This keeps PDF resource planning
    /// bounded while matching the common-space rule used at rasterization.
    fn color_space(&self) -> crate::color::RasterColorSpace {
        let space = match self {
            Self::Linear { gradient, .. } => {
                crate::color::common_color_space(gradient.stops.iter().map(|stop| stop.color))
            }
            Self::Radial { gradient, .. } => {
                crate::color::common_color_space(gradient.stops.iter().map(|stop| stop.color))
            }
        };
        crate::color::RasterColorSpace::BuiltIn(space)
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

    fn color(&mut self, color: crate::Color) {
        self.u32(u32::from(color.space().cache_key()));
        self.f32(color.r);
        self.f32(color.g);
        self.f32(color.b);
        self.f32(color.a);
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
            self.color(stop.color);
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
        self.bool(gradient.repeating);
        self.stops_and_hints(&gradient.stops, &gradient.hints);
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
        let (metadata, format, color_space) = image_metadata(&bytes, apply_orientation)?;
        let id = ImageId(u32::try_from(self.images.len()).ok()?);
        self.images.push(ImageAsset::Encoded(EncodedImage {
            bytes,
            format,
            metadata,
            apply_orientation,
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
        let mut decoder = ImageReader::with_format(Cursor::new(image.bytes.as_ref()), image.format)
            .into_decoder()
            .ok()?;
        let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
        let mut decoded = image::DynamicImage::from_decoder(decoder).ok()?;
        if image.apply_orientation {
            decoded.apply_orientation(orientation);
        }
        let rgba = decoded.to_rgba8();
        let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(rgba.len() / 4);
        let mut has_alpha = false;
        for pixel in rgba.chunks_exact(4) {
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

fn image_metadata(
    bytes: &[u8],
    apply_orientation: bool,
) -> Option<(
    ImageMetadata,
    image::ImageFormat,
    crate::color::RasterColorSpace,
)> {
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
            log::debug!("using the sRGB fallback for an image without an embedded ICC profile");
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
        format,
        color_space,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder};

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

    #[test]
    fn png_and_jpeg_retain_valid_embedded_rgb_profiles() {
        let profile = crate::color::icc_profile_bytes(crate::css::ColorSpace::DisplayP3).unwrap();
        for bytes in [tagged_png(profile.clone()), tagged_jpeg(profile.clone())] {
            let (_, _, color_space) = image_metadata(&bytes, false).unwrap();
            assert_eq!(
                color_space,
                crate::color::RasterColorSpace::EmbeddedRgb(Rc::from(profile.clone()))
            );
        }
    }

    #[test]
    fn malformed_and_non_rgb_embedded_profiles_fall_back_to_srgb() {
        let non_rgb = lcms2::Profile::new_xyz().icc().unwrap();
        for profile in [vec![1, 2, 3], non_rgb] {
            let (_, _, color_space) = image_metadata(&tagged_png(profile), false).unwrap();
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
