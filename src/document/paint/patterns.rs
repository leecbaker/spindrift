use std::rc::Rc;

use crate::image_store::ImageId;

use super::geometry::{PaintPoint, PaintRect, PaintSize, PaintTransform, PaintTranslation};
use super::images::{InlineRasterImage, RenderedImageSource};
use super::paths::{RenderedGradient, RenderedPath, RenderedPathClip};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintPatternTiling {
    pub tile_size: PaintSize,
    pub step: PaintSize,
    pub origin: PaintPoint,
}

impl PaintPatternTiling {
    pub fn new(tile_size: PaintSize, step: PaintSize, origin: PaintPoint) -> Self {
        Self {
            tile_size,
            step,
            origin,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedImagePattern {
    pub background: bool,
    pub(crate) source: RenderedImageSource,
    pub(in crate::document) rect: PaintRect,
    pub tiling: PaintPatternTiling,
    pub interpolate: bool,
    clip: Option<RenderedPathClip>,
}

#[allow(dead_code)]
impl RenderedImagePattern {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_paint_rect(
        rect: PaintRect,
        background: bool,
        tiling: PaintPatternTiling,
        pixel_width: u32,
        pixel_height: u32,
        interpolate: bool,
        rgb: Rc<[u8]>,
        alpha: Option<Rc<[u8]>>,
    ) -> Self {
        Self {
            background,
            source: RenderedImageSource::Inline {
                raster: InlineRasterImage {
                    pixel_width,
                    pixel_height,
                    color_space: crate::color::RasterColorSpace::SRGB,
                    rgb,
                    alpha,
                },
                source_rect: None,
            },
            rect,
            tiling,
            interpolate,
            clip: None,
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

    /// Attach a PDF path clipping scope to a repeated background pattern.
    ///
    /// CSS Backgrounds clips each repeated image layer to the selected
    /// `background-clip` area, including rounded corners:
    /// <https://www.w3.org/TR/css-backgrounds-3/#background-clip>.
    pub(crate) fn with_clip(mut self, clip: RenderedPathClip) -> Self {
        self.clip = Some(clip);
        self
    }

    pub(crate) fn clip(&self) -> Option<&RenderedPathClip> {
        self.clip.as_ref()
    }

    pub(crate) fn with_image_id(mut self, image_id: Option<ImageId>) -> Self {
        if let Some(image_id) = image_id {
            let RenderedImageSource::Inline { raster, .. } = &self.source else {
                return self;
            };
            self.source = RenderedImageSource::Stored {
                image_id,
                source_rect: RenderedImageSourceRect {
                    x: 0,
                    y: 0,
                    width: raster.pixel_width,
                    height: raster.pixel_height,
                },
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

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        self.tiling.origin = offset.transform_point(self.tiling.origin);
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

/// A repeated CSS gradient painted by a reusable PDF tiling pattern.
///
/// The pattern stores resolved CSS tile geometry independently from its PDF
/// resource allocation. Its cell paints the shared axial or radial shading;
/// the outer path applies CSS `background-clip`.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedGradientPattern {
    pub(crate) rect: PaintRect,
    pub(crate) tiling: PaintPatternTiling,
    pub(crate) gradient: RenderedGradient,
    clip: Option<RenderedPathClip>,
    /// Maps retained source geometry into its destination fragmentainer.
    transform: PaintTransform,
}

#[allow(dead_code)]
impl RenderedGradientPattern {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        rect: PaintRect,
        tiling: PaintPatternTiling,
        gradient: RenderedGradient,
        clip: Option<RenderedPathClip>,
    ) -> Self {
        Self {
            rect,
            tiling,
            gradient,
            clip,
            transform: PaintTransform::identity(),
        }
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn paint_bounds(&self) -> PaintRect {
        self.transform
            .apply_clip_to_aabb(super::geometry::PaintClip::from_paint_rect(self.rect))
            .paint_rect()
    }

    pub(crate) fn transform(&self) -> PaintTransform {
        self.transform
    }

    pub(crate) fn transformed(mut self, transform: PaintTransform) -> Self {
        self.transform = transform.multiply(self.transform);
        self
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
    pub(crate) fn clip(&self) -> Option<&RenderedPathClip> {
        self.clip.as_ref()
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        if self.transform != PaintTransform::identity() {
            self.transform = PaintTransform::translate(offset).multiply(self.transform);
            return self;
        }
        self.rect = offset.transform_rect(&self.rect);
        self.tiling.origin = offset.transform_point(self.tiling.origin);
        self.gradient.transform =
            PaintTransform::translate(offset).multiply(self.gradient.transform);
        if let Some(clip) = &mut self.clip {
            for command in &mut clip.commands {
                command.translate(offset);
            }
            for nested in &mut clip.additional_clips {
                for command in &mut nested.commands {
                    command.translate(offset);
                }
            }
        }
        self
    }
}

/// A reusable vector tile for a repeated URL SVG background.
///
/// PDF emission serializes its paths once into a Form XObject and invokes that
/// form from a Type 1 tiling pattern, avoiding one page primitive per CSS tile.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedSvgPattern {
    pub(crate) rect: PaintRect,
    pub(crate) tiling: PaintPatternTiling,
    pub(crate) paths: Vec<RenderedPath>,
    clip: Option<RenderedPathClip>,
}

impl RenderedSvgPattern {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        rect: PaintRect,
        tiling: PaintPatternTiling,
        paths: Vec<RenderedPath>,
        clip: Option<RenderedPathClip>,
    ) -> Self {
        Self {
            rect,
            tiling,
            paths,
            clip,
        }
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn clip(&self) -> Option<&RenderedPathClip> {
        self.clip.as_ref()
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        self.tiling.origin = offset.transform_point(self.tiling.origin);
        // `paths` are local to the Form XObject cell and deliberately remain
        // at its origin. Only the page placement and its CSS clip move.
        if let Some(clip) = &mut self.clip {
            for command in &mut clip.commands {
                command.translate(offset);
            }
            for nested in &mut clip.additional_clips {
                for command in &mut nested.commands {
                    command.translate(offset);
                }
            }
        }
        self
    }
}

/// Pixel-space source rectangle for drawing a cropped PDF image XObject.
///
/// CSS Border Images use nine-slice scaling: each destination border segment
/// maps to a source image slice. PDF image XObjects have fixed pixel data, so
/// source cropping is normalized before resource emission:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-images> and ISO
/// 32000-1:2008, 8.9 "Images".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderedImageSourceRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl RenderedImageSourceRect {
    pub fn x(&self) -> u32 {
        self.x
    }

    pub fn y(&self) -> u32 {
        self.y
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}
