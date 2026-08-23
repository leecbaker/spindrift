use std::rc::Rc;

use super::geometry::{PaintRect, PaintTransform, PaintTranslation};
use super::paths::{RenderedPathClip, RenderedPathClipPath};
use super::patterns::RenderedImageSourceRect;
use crate::image_store::{DocumentImageStore, ImageId};

/// The raster scaling behavior selected by CSS `image-rendering`.
///
/// This is retained with an image until PDF resource preparation, where the
/// final object-fit geometry and output device-density are both known.
/// <https://drafts.csswg.org/css-images-3/#the-image-rendering>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum RasterSampling {
    #[default]
    Auto,
    Smooth,
    HighQuality,
    Pixelated,
    CrispEdges,
}

impl From<bool> for RasterSampling {
    fn from(interpolate: bool) -> Self {
        if interpolate {
            Self::Auto
        } else {
            Self::CrispEdges
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RenderedImageSource {
    Stored {
        image_id: ImageId,
        source_rect: RenderedImageSourceRect,
        pixel_width: u32,
        pixel_height: u32,
    },
    Inline {
        raster: InlineRasterImage,
        source_rect: Option<RenderedImageSourceRect>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InlineRasterImage {
    pub(crate) pixel_width: u32,
    pub(crate) pixel_height: u32,
    /// CSS intrinsic dimensions after image metadata such as EXIF density has
    /// been applied. These are distinct from encoded sample dimensions.
    pub(crate) natural_size: crate::units::CssPixelSize,
    pub(crate) color_space: crate::color::RasterColorSpace,
    pub(crate) sample_depth: crate::image_store::RasterSampleDepth,
    pub(crate) rgb: Rc<[u8]>,
    pub(crate) alpha: Option<Rc<[u8]>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedImage {
    pub background: bool,
    pub(crate) source: RenderedImageSource,
    pub(in crate::document) rect: PaintRect,
    pub(crate) sampling: RasterSampling,
    pub alt_text: Option<Rc<str>>,
    /// Exact logical text represented by this otherwise non-text paint.
    ///
    /// Bitmap OpenType glyphs are emitted as PDF image XObjects.  `/ActualText`
    /// preserves their authored Unicode content for extraction without
    /// conflating a glyph replacement with an image's alternative text.
    pub(crate) actual_text: Option<Rc<str>>,
    /// Optional local-to-page transform for paint sources whose natural
    /// geometry is not axis-aligned in page space, such as vertical bitmap
    /// OpenType glyphs.
    pub(crate) transform: Option<PaintTransform>,
    clip: Option<RenderedPathClip>,
    /// Whether `clip` is only the image's own destination rectangle.
    ///
    /// Object fitting normally needs a clip, but an uncropped concrete object
    /// already paints inside this rectangle. Retain that semantic distinction
    /// instead of asking PDF serialization to recover it from floating-point
    /// path coordinates.
    destination_rect_clip: bool,
}

#[allow(dead_code)]
impl RenderedImage {
    /// Detach this image from a document-local image store for embedding in a
    /// different document. Image IDs are only meaningful within their owning
    /// [`DocumentImageStore`], so an iframe page must carry samples rather
    /// than its child's numeric handle into its parent paint tree.
    /// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
    pub(in crate::document) fn materialize_store_backing(&mut self, store: &DocumentImageStore) {
        let RenderedImageSource::Stored {
            image_id,
            source_rect,
            ..
        } = &self.source
        else {
            return;
        };
        let image_id = *image_id;
        let source_rect = *source_rect;
        let Some(source) = store.with_rasterized(image_id, |raster| RenderedImageSource::Inline {
            raster: InlineRasterImage {
                pixel_width: raster.metadata.pixel_size.width,
                pixel_height: raster.metadata.pixel_size.height,
                natural_size: raster.metadata.natural_size,
                color_space: raster.color_space,
                sample_depth: raster.sample_depth,
                rgb: Rc::from(raster.rgb),
                alpha: raster.alpha.map(Rc::from),
            },
            source_rect: Some(source_rect),
        }) else {
            return;
        };
        self.source = source;
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_paint_rect(
        rect: PaintRect,
        background: bool,
        pixel_width: u32,
        pixel_height: u32,
        source_rect: Option<RenderedImageSourceRect>,
        sampling: impl Into<RasterSampling>,
        rgb: Rc<[u8]>,
        alpha: Option<Rc<[u8]>>,
        alt_text: Option<Rc<str>>,
    ) -> Self {
        Self {
            background,
            source: RenderedImageSource::Inline {
                raster: InlineRasterImage {
                    pixel_width,
                    pixel_height,
                    natural_size: crate::units::CssPixelSize::new(pixel_width, pixel_height),
                    color_space: crate::color::RasterColorSpace::SRGB,
                    sample_depth: crate::image_store::RasterSampleDepth::Eight,
                    rgb,
                    alpha,
                },
                source_rect,
            },
            rect,
            sampling: sampling.into(),
            alt_text,
            actual_text: None,
            transform: None,
            clip: None,
            destination_rect_clip: false,
        }
    }

    pub fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(&self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn set_paint_rect(&mut self, rect: PaintRect) {
        self.rect = rect;
    }

    /// Attach a PDF path clipping scope to an image draw operation.
    ///
    /// CSS Backgrounds clips background image layers to the selected
    /// `background-clip` area, including rounded corners from
    /// `border-radius`:
    /// <https://www.w3.org/TR/css-backgrounds-3/#corner-clipping>.
    pub(crate) fn with_clip(mut self, clip: RenderedPathClip) -> Self {
        self.clip = Some(clip);
        self.destination_rect_clip = false;
        self
    }

    /// Intersect this image's retained clip with an SVG descendant clip.
    ///
    /// SVG group and viewport clips apply to image nodes just as they do to
    /// vector paths.  Keeping both contours avoids reconstructing an image's
    /// object-fit geometry from scalars at the PDF boundary.
    pub(crate) fn with_intersected_clip(mut self, clip: RenderedPathClip) -> Self {
        if let Some(existing) = self.clip.take() {
            let mut combined = clip;
            combined.additional_clips.push(RenderedPathClipPath::new(
                existing.commands,
                existing.fill_rule,
            ));
            combined.additional_clips.extend(existing.additional_clips);
            self.clip = Some(combined);
        } else {
            self.clip = Some(clip);
        }
        self.destination_rect_clip = false;
        self
    }

    /// Attach the redundant rectangular clip generated for an uncropped
    /// concrete object. PDF output may omit this clip without changing CSS
    /// image geometry or clipping semantics.
    pub(crate) fn with_destination_rect_clip(mut self, clip: RenderedPathClip) -> Self {
        self.clip = Some(clip);
        self.destination_rect_clip = true;
        self
    }

    /// Associate an exact Unicode replacement string with this image paint.
    pub(crate) fn with_actual_text(mut self, actual_text: Rc<str>) -> Self {
        if !actual_text.is_empty() {
            self.actual_text = Some(actual_text);
        }
        self
    }

    /// Apply a local affine transform before placing this image rectangle.
    pub(crate) fn with_transform(mut self, transform: PaintTransform) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Project this image and its retained clip into a destination paint
    /// space. The PDF image rectangle remains source-local and is transformed
    /// at emission time; its clip is transformed here because PDF installs it
    /// before the image CTM.
    pub(crate) fn transformed(mut self, transform: PaintTransform) -> Self {
        if transform == PaintTransform::identity() {
            return self;
        }
        self.transform =
            Some(transform.multiply(self.transform.unwrap_or_else(PaintTransform::identity)));
        if let Some(clip) = &mut self.clip {
            clip.transform(transform);
        }
        self
    }

    pub(crate) fn clip(&self) -> Option<&RenderedPathClip> {
        self.clip.as_ref()
    }

    pub(crate) fn has_destination_rect_clip(&self) -> bool {
        self.destination_rect_clip
    }

    /// Whether this image is constrained by a paint clip.
    ///
    /// CSS background clipping is represented as a destination-space clip
    /// rather than destructively shrinking the image's destination rectangle.
    pub fn is_clipped(&self) -> bool {
        self.clip.is_some()
    }

    pub(crate) fn with_image_id(mut self, image_id: Option<ImageId>) -> Self {
        if let Some(image_id) = image_id {
            let RenderedImageSource::Inline {
                raster,
                source_rect,
            } = &self.source
            else {
                return self;
            };
            self.source = RenderedImageSource::Stored {
                image_id,
                source_rect: source_rect.unwrap_or(RenderedImageSourceRect {
                    x: 0,
                    y: 0,
                    width: raster.pixel_width,
                    height: raster.pixel_height,
                }),
                pixel_width: raster.pixel_width,
                pixel_height: raster.pixel_height,
            };
        }
        self
    }

    /// Preserve the calibrated component space of generated inline samples.
    pub(crate) fn with_raster_color_space(
        mut self,
        color_space: crate::color::RasterColorSpace,
    ) -> Self {
        if let RenderedImageSource::Inline { raster, .. } = &mut self.source {
            raster.color_space = color_space;
        }
        self
    }

    /// Set the depth shared by this image's RGB and opacity sample planes.
    pub(crate) fn with_raster_sample_depth(
        mut self,
        sample_depth: crate::image_store::RasterSampleDepth,
    ) -> Self {
        if let RenderedImageSource::Inline { raster, .. } = &mut self.source {
            raster.sample_depth = sample_depth;
        }
        self
    }

    /// Returns the pixel rectangle selected from the intrinsic image.
    ///
    /// A store-backed image always has an explicit full-image or cropped
    /// source rectangle; legacy inline images preserve whether a crop was
    /// specified by their creator.
    pub fn source_rect(&self) -> Option<RenderedImageSourceRect> {
        match &self.source {
            RenderedImageSource::Stored { source_rect, .. } => Some(*source_rect),
            RenderedImageSource::Inline { source_rect, .. } => *source_rect,
        }
    }

    /// Returns the intrinsic image width in device pixels.
    pub fn pixel_width(&self) -> u32 {
        match &self.source {
            RenderedImageSource::Stored { pixel_width, .. } => *pixel_width,
            RenderedImageSource::Inline { raster, .. } => raster.pixel_width,
        }
    }

    /// Returns the intrinsic image height in device pixels.
    pub fn pixel_height(&self) -> u32 {
        match &self.source {
            RenderedImageSource::Stored { pixel_height, .. } => *pixel_height,
            RenderedImageSource::Inline { raster, .. } => raster.pixel_height,
        }
    }

    pub(crate) fn set_source_rect(&mut self, source_rect: RenderedImageSourceRect) {
        match &mut self.source {
            RenderedImageSource::Stored {
                source_rect: current,
                ..
            } => *current = source_rect,
            RenderedImageSource::Inline {
                source_rect: current,
                ..
            } => *current = Some(source_rect),
        }
    }

    pub(crate) fn inline_pixel_size(&self) -> Option<(u32, u32)> {
        match &self.source {
            RenderedImageSource::Stored { .. } => None,
            RenderedImageSource::Inline { raster, .. } => {
                Some((raster.pixel_width, raster.pixel_height))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn pixel_storage_ptr_eq(&self, other: &Self) -> bool {
        match (&self.source, &other.source) {
            (
                RenderedImageSource::Stored { image_id: left, .. },
                RenderedImageSource::Stored {
                    image_id: right, ..
                },
            ) => left == right,
            (
                RenderedImageSource::Inline { raster: left, .. },
                RenderedImageSource::Inline { raster: right, .. },
            ) => {
                Rc::ptr_eq(&left.rgb, &right.rgb)
                    && match (&left.alpha, &right.alpha) {
                        (Some(left), Some(right)) => Rc::ptr_eq(left, right),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

impl RenderedImage {
    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        if let Some(clip) = &mut self.clip {
            for command in &mut clip.commands {
                command.translate(offset);
            }
            for nested_clip in &mut clip.additional_clips {
                for command in &mut nested_clip.commands {
                    command.translate(offset);
                }
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::RenderedImage;
    use crate::document::paint::geometry::{PaintPoint, PaintRect, PaintSize};

    #[test]
    fn cloned_images_share_pixel_storage() {
        let image = RenderedImage::from_paint_rect(
            PaintRect::new(PaintPoint::new(0.0, 0.0), PaintSize::new(2.0, 1.0)),
            false,
            2,
            1,
            None,
            false,
            Rc::from(vec![1, 2, 3, 4, 5, 6].into_boxed_slice()),
            Some(Rc::from(vec![255, 127].into_boxed_slice())),
            Some(Rc::from("alt")),
        );
        let cloned = image.clone();

        assert!(image.pixel_storage_ptr_eq(&cloned));
    }

    #[test]
    fn rendered_image_exposes_paint_rect() {
        let rect = PaintRect::new(PaintPoint::new(3.0, 4.0), PaintSize::new(5.0, 6.0));
        let image = RenderedImage::from_paint_rect(
            rect,
            false,
            5,
            6,
            None,
            false,
            Rc::from(Vec::new().into_boxed_slice()),
            None,
            Some(Rc::from("alt")),
        );

        assert_eq!(image.paint_rect(), rect);
        assert_eq!(image.width(), 5.0);
        assert_eq!(image.height(), 6.0);
    }

    #[test]
    fn destination_rect_clip_is_distinct_from_an_authored_clip() {
        let rect = PaintRect::new(PaintPoint::new(3.0, 4.0), PaintSize::new(5.0, 6.0));
        let clip = crate::document::paint::paths::RenderedPathClip::new(
            Vec::new(),
            crate::document::paint::paths::RenderedPathFillRule::NonZero,
            Vec::new(),
        );
        let image = RenderedImage::from_paint_rect(
            rect,
            false,
            1,
            1,
            None,
            false,
            Rc::from([0_u8, 128, 0]),
            None,
            None,
        );

        assert!(
            image
                .clone()
                .with_destination_rect_clip(clip.clone())
                .has_destination_rect_clip()
        );
        assert!(!image.with_clip(clip).has_destination_rect_clip());
    }
}
