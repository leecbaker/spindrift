use std::rc::Rc;

use super::*;
use crate::document::paint::patterns::RenderedImageSourceRect;

/// The border-image resolution result that controls whether ordinary border
/// styles remain visible. A successfully resolved image replaces the normal
/// border even when its slice geometry produces no paint primitives.
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-source>
pub(in crate::layout) enum BorderPaint {
    UseNormalBorder,
    ReplaceNormalBorder { primitives: Vec<PaintPrimitive> },
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct RenderedImageTileRect {
    /// Destination tile region in page-local CSS paint coordinates.
    ///
    /// CSS Backgrounds and Borders slices `border-image` into destination
    /// regions that are painted into the border-image area. At this stage the
    /// layout box has already been projected into paint space, so the rectangle
    /// uses the same bottom-left-origin coordinate system as rendered images:
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
    pub(in crate::layout) rect: PaintRect,
}

/// A rectangle in a border-image source's own coordinate system.
///
/// This intentionally differs from [`RenderedImageSourceRect`]: CSS source
/// coordinates and percentage-derived slices can be fractional. Raster tiles
/// retain this geometry through slicing and tiling. At the raster resource
/// boundary, integral bounds become PDF-image crops while fractional bounds
/// become isolated, edge-extended rasters.
/// <https://drafts.csswg.org/css-backgrounds-3/#border-image-slice>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BorderImageSourceRect {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
}

impl BorderImageSourceRect {
    pub(in crate::layout) const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

impl RenderedImageTileRect {
    pub(in crate::layout) fn from_paint_rect(rect: PaintRect) -> Self {
        Self { rect }
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub(in crate::layout) fn y(self) -> f32 {
        self.rect.origin.y
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(in crate::layout) fn height(self) -> f32 {
        self.rect.size.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BorderImageTileSegment {
    pub(in crate::layout) destination_offset: f32,
    pub(in crate::layout) destination_size: f32,
    pub(in crate::layout) source_offset: f32,
    pub(in crate::layout) source_size: f32,
}

/// Fully resolved raster paint input for one border-image slice region.
///
/// This keeps the full source bounds, selected source geometry, destination
/// geometry, and repeat-stage dimensions together through the PDF-image
/// emission boundary.
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>
pub(in crate::layout) struct RasterBorderImageTilePaint<'a> {
    pub(in crate::layout) decoded: &'a DecodedPngImage,
    pub(in crate::layout) destination: RenderedImageTileRect,
    /// Full CSS source-coordinate bounds of `decoded`. Generated CSS images
    /// may have a supersampled raster backing, so these do not necessarily
    /// equal the decoded pixel dimensions.
    pub(in crate::layout) source_image_bounds: BorderImageSourceRect,
    pub(in crate::layout) source: BorderImageSourceRect,
    pub(in crate::layout) tile_size: PaintSize,
    pub(in crate::layout) repeat_x: css::BorderImageRepeatKeyword,
    pub(in crate::layout) repeat_y: css::BorderImageRepeatKeyword,
    pub(in crate::layout) sampling: crate::document::paint::images::RasterSampling,
}

/// A sampling-contained raster source for one border-image tile.
///
/// CSS slices the source image before the resulting image is scaled or tiled.
/// An integral external-raster slice can use the PDF resource crop directly.
/// A fractional slice instead owns a local, edge-extended raster so a backend
/// clip or interpolation filter cannot observe samples from an adjacent CSS
/// border-image region.
/// <https://drafts.csswg.org/css-backgrounds-3/#border-image-process>
#[derive(Debug, Clone)]
enum RasterBorderImageTileSource {
    IntegralCrop(RenderedImageSourceRect),
    EdgeExtended {
        image: DecodedPngImage,
        content_pixels: RasterPixelSize,
    },
}

/// Emits the repeated image tiles for one border-image slice region.
///
/// CSS Backgrounds and Borders Level 3 applies `border-image-repeat` after the
/// source image has been sliced into a 3x3 grid. Corners are stretched, edge
/// regions repeat only along their long axis, and the optional center region
/// repeats on both axes:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
pub(in crate::layout) fn push_border_image_tiles(
    images: &mut Vec<RenderedImage>,
    resource_cache: &crate::resource::ResourceCache,
    paint: RasterBorderImageTilePaint<'_>,
) {
    let RasterBorderImageTilePaint {
        decoded,
        destination,
        source_image_bounds,
        source,
        tile_size,
        repeat_x,
        repeat_y,
        sampling,
    } = paint;
    let mut resolved_sources = Vec::<(BorderImageSourceRect, RasterBorderImageTileSource)>::new();
    let x_segments =
        border_image_tile_segments(repeat_x, destination.width(), tile_size.width, source.width);
    let y_segments = border_image_tile_segments(
        repeat_y,
        destination.height(),
        tile_size.height,
        source.height,
    );
    for y_segment in &y_segments {
        for x_segment in &x_segments {
            if x_segment.destination_size <= 0.0
                || y_segment.destination_size <= 0.0
                || x_segment.source_size <= 0.0
                || y_segment.source_size <= 0.0
            {
                continue;
            }
            let selected_source = BorderImageSourceRect::new(
                source.x + x_segment.source_offset,
                source.y + y_segment.source_offset,
                x_segment.source_size,
                y_segment.source_size,
            );
            let tile_rect = paint_space_rect(
                destination.x() + x_segment.destination_offset,
                destination.y() + y_segment.destination_offset,
                x_segment.destination_size,
                y_segment.destination_size,
            );
            let tile_source = if let Some((_, source)) = resolved_sources
                .iter()
                .find(|(existing, _)| *existing == selected_source)
            {
                source.clone()
            } else {
                let Some(source) = raster_border_image_tile_source(
                    decoded,
                    resource_cache,
                    source_image_bounds,
                    selected_source,
                ) else {
                    continue;
                };
                resolved_sources.push((selected_source, source.clone()));
                source
            };
            match tile_source {
                RasterBorderImageTileSource::IntegralCrop(source_rect) => {
                    images.push(
                        RenderedImage::from_paint_rect(
                            tile_rect,
                            true,
                            decoded.pixel_size.width,
                            decoded.pixel_size.height,
                            Some(source_rect),
                            sampling,
                            decoded.rgb.shared(),
                            decoded.alpha.clone(),
                            None,
                        )
                        .with_raster_sample_depth(decoded.sample_depth)
                        .with_raster_color_space(decoded.color_space.clone())
                        .with_image_id(decoded.image_id)
                        // A source crop selects the image samples, while the
                        // destination clip remains the CSS tile boundary.
                        // Keeping both prevents interpolation from sampling a
                        // neighboring border-image slice at a shared edge.
                        .with_clip(RenderedPathClip::new(
                            paint_rect_path_commands(tile_rect),
                            RenderedPathFillRule::NonZero,
                            Vec::new(),
                        )),
                    );
                }
                RasterBorderImageTileSource::EdgeExtended {
                    image,
                    content_pixels,
                } => {
                    let pixel_scale_x = tile_rect.size.width / content_pixels.width as f32;
                    let pixel_scale_y = tile_rect.size.height / content_pixels.height as f32;
                    if !pixel_scale_x.is_finite()
                        || !pixel_scale_y.is_finite()
                        || pixel_scale_x <= 0.0
                        || pixel_scale_y <= 0.0
                    {
                        continue;
                    }
                    let padded_rect = paint_space_rect(
                        tile_rect.origin.x - pixel_scale_x,
                        tile_rect.origin.y - pixel_scale_y,
                        tile_rect.size.width + 2.0 * pixel_scale_x,
                        tile_rect.size.height + 2.0 * pixel_scale_y,
                    );
                    images.push(
                        RenderedImage::from_paint_rect(
                            padded_rect,
                            true,
                            image.pixel_size.width,
                            image.pixel_size.height,
                            None,
                            sampling,
                            image.rgb.shared(),
                            image.alpha.clone(),
                            None,
                        )
                        .with_raster_sample_depth(image.sample_depth)
                        .with_raster_color_space(image.color_space.clone())
                        .with_clip(RenderedPathClip::new(
                            paint_rect_path_commands(tile_rect),
                            RenderedPathFillRule::NonZero,
                            Vec::new(),
                        )),
                    );
                }
            }
        }
    }
}

fn raster_border_image_tile_source(
    decoded: &DecodedPngImage,
    resource_cache: &crate::resource::ResourceCache,
    source_image_bounds: BorderImageSourceRect,
    selected_source: BorderImageSourceRect,
) -> Option<RasterBorderImageTileSource> {
    let integral_source = integral_source_rect(decoded, source_image_bounds, selected_source);
    if let Some(source_rect) = integral_source {
        return Some(RasterBorderImageTileSource::IntegralCrop(source_rect));
    }
    let materialized = decoded.image_id.and_then(|image_id| {
        resource_cache.with_rasterized_image(image_id, |raster| DecodedPngImage {
            image_id: Some(image_id),
            pixel_size: raster.metadata.pixel_size,
            source_rect: None,
            natural_size: raster.metadata.natural_size,
            sample_depth: raster.sample_depth,
            rgb: EncodedRasterRgbSamples::new(raster.rgb),
            alpha: raster.alpha.map(|alpha| Rc::from(alpha.into_boxed_slice())),
            color_space: raster.color_space,
        })
    });
    edge_extended_fractional_source(
        materialized.as_ref().unwrap_or(decoded),
        source_image_bounds,
        selected_source,
    )
    .map(
        |(image, content_pixels)| RasterBorderImageTileSource::EdgeExtended {
            image,
            content_pixels,
        },
    )
}

fn integral_source_rect(
    decoded: &DecodedPngImage,
    source_image_bounds: BorderImageSourceRect,
    source: BorderImageSourceRect,
) -> Option<RenderedImageSourceRect> {
    if source_image_bounds.x != 0.0
        || source_image_bounds.y != 0.0
        || source_image_bounds.width != decoded.pixel_size.width as f32
        || source_image_bounds.height != decoded.pixel_size.height as f32
    {
        return None;
    }
    let coordinate = |value: f32| {
        let rounded = value.round();
        ((value - rounded).abs() <= f32::EPSILON && rounded >= 0.0).then_some(rounded as u32)
    };
    let (x, y, width, height) = (
        coordinate(source.x)?,
        coordinate(source.y)?,
        coordinate(source.width)?,
        coordinate(source.height)?,
    );
    (width > 0
        && height > 0
        && x.checked_add(width)? <= decoded.pixel_size.width
        && y.checked_add(height)? <= decoded.pixel_size.height)
        .then_some(RenderedImageSourceRect {
            x,
            y,
            width,
            height,
        })
}

/// Materialize one fractional raster source region with a duplicated one-pixel
/// border. The local image's inner sample grid maps to the selected CSS source
/// rectangle; the duplicated border exists solely for sampling containment at
/// the PDF clip boundary.
fn edge_extended_fractional_source(
    decoded: &DecodedPngImage,
    source_image_bounds: BorderImageSourceRect,
    source: BorderImageSourceRect,
) -> Option<(DecodedPngImage, RasterPixelSize)> {
    if source_image_bounds.width <= 0.0
        || source_image_bounds.height <= 0.0
        || source.width <= 0.0
        || source.height <= 0.0
    {
        return None;
    }
    let density_x = decoded.pixel_size.width as f32 / source_image_bounds.width;
    let density_y = decoded.pixel_size.height as f32 / source_image_bounds.height;
    let content_width = sampled_axis_length(source.width, density_x)?;
    let content_height = sampled_axis_length(source.height, density_y)?;
    let pixel_width = content_width.checked_add(2)?;
    let pixel_height = content_height.checked_add(2)?;
    let component_bytes = decoded.sample_depth.bytes_per_component();
    let source_pixel_count = usize::try_from(decoded.pixel_size.width)
        .ok()?
        .checked_mul(usize::try_from(decoded.pixel_size.height).ok()?)?;
    if decoded.rgb.len()
        != source_pixel_count
            .checked_mul(3)?
            .checked_mul(component_bytes)?
        || decoded
            .alpha
            .as_ref()
            .is_some_and(|alpha| alpha.len() != source_pixel_count * component_bytes)
    {
        return None;
    }
    let pixel_count = usize::try_from(pixel_width)
        .ok()?
        .checked_mul(usize::try_from(pixel_height).ok()?)?;
    let mut rgb = Vec::new();
    rgb.try_reserve(pixel_count.checked_mul(3)?.checked_mul(component_bytes)?)
        .ok()?;
    let mut alpha = decoded.alpha.as_ref().map(|_| Vec::new());
    if let Some(alpha) = &mut alpha {
        alpha
            .try_reserve(pixel_count.checked_mul(component_bytes)?)
            .ok()?;
    }

    for output_y in 0..pixel_height {
        let content_y = output_y.saturating_sub(1).min(content_height - 1);
        let source_y = sampled_source_index(
            source.y,
            source.height,
            content_y,
            content_height,
            density_y,
            decoded.pixel_size.height,
        );
        for output_x in 0..pixel_width {
            let content_x = output_x.saturating_sub(1).min(content_width - 1);
            let source_x = sampled_source_index(
                source.x,
                source.width,
                content_x,
                content_width,
                density_x,
                decoded.pixel_size.width,
            );
            let pixel_index = usize::try_from(source_y)
                .ok()?
                .checked_mul(usize::try_from(decoded.pixel_size.width).ok()?)?
                .checked_add(usize::try_from(source_x).ok()?)?;
            let rgb_start = pixel_index.checked_mul(3)?.checked_mul(component_bytes)?;
            let rgb_end = rgb_start.checked_add(3 * component_bytes)?;
            rgb.extend_from_slice(&decoded.rgb[rgb_start..rgb_end]);
            if let (Some(source_alpha), Some(alpha)) = (&decoded.alpha, &mut alpha) {
                let alpha_start = pixel_index.checked_mul(component_bytes)?;
                let alpha_end = alpha_start.checked_add(component_bytes)?;
                alpha.extend_from_slice(&source_alpha[alpha_start..alpha_end]);
            }
        }
    }

    let mut image = DecodedPngImage::new(pixel_width, pixel_height, rgb, alpha);
    image.sample_depth = decoded.sample_depth;
    image.color_space = decoded.color_space.clone();
    Some((image, RasterPixelSize::new(content_width, content_height)))
}

fn sampled_axis_length(source_length: f32, density: f32) -> Option<u32> {
    let length = (source_length * density).ceil();
    (length.is_finite() && length >= 1.0 && length <= u32::MAX as f32).then_some(length as u32)
}

fn sampled_source_index(
    source_start: f32,
    source_length: f32,
    output_index: u32,
    output_length: u32,
    density: f32,
    source_pixels: u32,
) -> u32 {
    debug_assert!(output_length > 0);
    let source_coordinate =
        source_start + (output_index as f32 + 0.5) * source_length / output_length as f32;
    (source_coordinate * density)
        .floor()
        .clamp(0.0, source_pixels.saturating_sub(1) as f32) as u32
}

/// Computes destination/source segments for one `border-image-repeat` axis.
///
/// The CSS border-image process defines four repeat modes: `stretch` scales one
/// image to the region, `repeat` clips repeated tiles at the ends, `round`
/// adjusts the tile size to fit an integer number of tiles, and `space`
/// distributes whole tiles with gaps:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-repeat>.
pub(in crate::layout) fn border_image_tile_segments(
    repeat: css::BorderImageRepeatKeyword,
    destination_size: f32,
    base_tile_size: f32,
    source_size: f32,
) -> Vec<BorderImageTileSegment> {
    if destination_size <= 0.0 || source_size <= 0.0 {
        return Vec::new();
    }
    if repeat == css::BorderImageRepeatKeyword::Stretch || base_tile_size <= 0.0 {
        return vec![BorderImageTileSegment {
            destination_offset: 0.0,
            destination_size,
            source_offset: 0.0,
            source_size,
        }];
    }
    match repeat {
        css::BorderImageRepeatKeyword::Repeat => {
            repeat_border_image_tile_segments(destination_size, base_tile_size, source_size)
        }
        css::BorderImageRepeatKeyword::Round => {
            let count = (destination_size / base_tile_size).round().max(1.0) as usize;
            let tile_size = destination_size / count as f32;
            (0..count)
                .map(|index| BorderImageTileSegment {
                    destination_offset: index as f32 * tile_size,
                    destination_size: tile_size,
                    source_offset: 0.0,
                    source_size,
                })
                .collect()
        }
        css::BorderImageRepeatKeyword::Space => {
            let count = (destination_size / base_tile_size).floor() as usize;
            if count == 0 {
                return Vec::new();
            }
            let tile_size = base_tile_size;
            // Border-image `space` distributes its leftover space around the
            // complete tiles: before the first tile, between every pair, and
            // after the last. This is intentionally different from
            // background-repeat: space, whose outer edges have no gap.
            // <https://www.w3.org/TR/css-backgrounds-3/#border-image-repeat>
            let spacing = (destination_size - tile_size * count as f32) / (count + 1) as f32;
            (0..count)
                .map(|index| BorderImageTileSegment {
                    destination_offset: spacing + index as f32 * (tile_size + spacing),
                    destination_size: tile_size,
                    source_offset: 0.0,
                    source_size,
                })
                .collect()
        }
        css::BorderImageRepeatKeyword::Stretch => unreachable!(),
    }
}

pub(in crate::layout) fn repeat_border_image_tile_segments(
    destination_size: f32,
    tile_size: f32,
    source_size: f32,
) -> Vec<BorderImageTileSegment> {
    if destination_size <= 0.0 || tile_size <= 0.0 || source_size <= 0.0 {
        return Vec::new();
    }

    // `repeat` centers the integer sequence of whole tiles in the edge
    // region, then clips the equal overhang at either end. Starting at the
    // leading edge instead selects a different part of a source tile whenever
    // the region is not an exact multiple (including the common one-tile
    // case). CSS Border Images describes the centered, symmetrically clipped
    // placement in its border-image process:
    // <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
    let tile_count = (destination_size / tile_size).ceil().max(1.0) as usize;
    let sequence_size = tile_count as f32 * tile_size;
    let first_offset = (destination_size - sequence_size) / 2.0;
    let mut segments = Vec::with_capacity(tile_count);

    for index in 0..tile_count {
        let tile_start = first_offset + index as f32 * tile_size;
        let tile_end = tile_start + tile_size;
        let visible_start = tile_start.max(0.0);
        let visible_end = tile_end.min(destination_size);
        if visible_end <= visible_start {
            continue;
        }
        let source_start =
            ((visible_start - tile_start) * source_size / tile_size).clamp(0.0, source_size);
        let source_end =
            ((visible_end - tile_start) * source_size / tile_size).clamp(source_start, source_size);
        if source_end <= source_start {
            continue;
        }
        segments.push(BorderImageTileSegment {
            destination_offset: visible_start,
            destination_size: visible_end - visible_start,
            source_offset: source_start,
            source_size: source_end - source_start,
        });
    }
    segments
}

/// Resolve the first-stage scale for the center `border-image` region on one
/// axis.
///
/// The center inherits the scale of the corresponding first edge. A zero or
/// infinite edge scale is explicitly unusable: the opposite edge is tried,
/// and if that is unusable too the center is not scaled on that axis. This is
/// important when `border-style: none` gives both used edge widths zero while
/// `border-image-slice: ... fill` still paints its middle region.
/// <https://drafts.csswg.org/css-backgrounds-3/#border-image-process>
pub(in crate::layout) fn border_image_center_axis_scale(
    first_destination: f32,
    first_source: f32,
    second_destination: f32,
    second_source: f32,
) -> f32 {
    let edge_scale = |destination: f32, source: f32| {
        (source > 0.0)
            .then_some(destination / source)
            .filter(|scale| scale.is_finite() && *scale > 0.0)
    };
    edge_scale(first_destination, first_source)
        .or_else(|| edge_scale(second_destination, second_source))
        .unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integral_raster_tiles_keep_an_isolated_pdf_source_crop() {
        let decoded = DecodedPngImage::new(20, 16, vec![0; 20 * 16 * 3], None);
        let mut images = Vec::new();

        push_border_image_tiles(
            &mut images,
            &crate::resource::ResourceCache::default(),
            RasterBorderImageTilePaint {
                decoded: &decoded,
                destination: RenderedImageTileRect::from_paint_rect(paint_space_rect(
                    0.0, 10.0, 18.0, 3.0,
                )),
                source_image_bounds: BorderImageSourceRect::new(0.0, 0.0, 20.0, 16.0),
                source: BorderImageSourceRect::new(2.0, 3.0, 4.0, 2.0),
                tile_size: PaintSize::new(6.0, 3.0),
                repeat_x: css::BorderImageRepeatKeyword::Repeat,
                repeat_y: css::BorderImageRepeatKeyword::Stretch,
                sampling: false.into(),
            },
        );

        assert_eq!(images.len(), 3);
        assert!(images.iter().all(|image| {
            image.source_rect()
                == Some(RenderedImageSourceRect {
                    x: 2,
                    y: 3,
                    width: 4,
                    height: 2,
                })
        }));
        assert!(images.iter().all(|image| image.is_clipped()));
    }

    #[test]
    fn fractional_source_is_edge_extended_without_adjacent_color_samples() {
        let decoded =
            DecodedPngImage::new(4, 1, vec![255, 0, 0, 0, 128, 0, 0, 128, 0, 255, 0, 0], None);
        let RasterBorderImageTileSource::EdgeExtended {
            image,
            content_pixels,
        } = raster_border_image_tile_source(
            &decoded,
            &crate::resource::ResourceCache::default(),
            BorderImageSourceRect::new(0.0, 0.0, 4.0, 1.0),
            BorderImageSourceRect::new(1.5, 0.0, 1.0, 1.0),
        )
        .expect("fractional source is materialized")
        else {
            panic!("fractional source must not use an integer crop");
        };

        assert_eq!(content_pixels, RasterPixelSize::new(1, 1));
        assert_eq!(image.pixel_size, RasterPixelSize::new(3, 3));
        assert!(
            image
                .rgb
                .as_chunks::<3>()
                .0
                .iter()
                .all(|sample| sample == &[0, 128, 0])
        );
    }

    #[test]
    fn scaled_and_destination_clipped_fractional_tile_stays_source_contained() {
        let decoded =
            DecodedPngImage::new(4, 1, vec![255, 0, 0, 0, 128, 0, 0, 128, 0, 255, 0, 0], None);
        let mut images = Vec::new();

        push_border_image_tiles(
            &mut images,
            &crate::resource::ResourceCache::default(),
            RasterBorderImageTilePaint {
                decoded: &decoded,
                destination: RenderedImageTileRect::from_paint_rect(paint_space_rect(
                    0.0, 0.0, 10.0, 2.0,
                )),
                source_image_bounds: BorderImageSourceRect::new(0.0, 0.0, 4.0, 1.0),
                source: BorderImageSourceRect::new(1.5, 0.0, 1.0, 1.0),
                tile_size: PaintSize::new(10.0, 2.0),
                repeat_x: css::BorderImageRepeatKeyword::Stretch,
                repeat_y: css::BorderImageRepeatKeyword::Stretch,
                sampling: true.into(),
            },
        );

        assert_eq!(images.len(), 1);
        assert!(images[0].is_clipped());
        assert_eq!(images[0].width(), 30.0);
        assert_eq!(images[0].height(), 6.0);
        let crate::document::paint::images::RenderedImageSource::Inline { raster, .. } =
            &images[0].source
        else {
            panic!("fractional tile must use its isolated inline raster");
        };
        assert!(
            raster
                .rgb
                .as_chunks::<3>()
                .0
                .iter()
                .all(|sample| sample == &[0, 128, 0])
        );
    }

    #[test]
    fn fractional_source_preserves_sixteen_bit_alpha_samples() {
        let mut decoded = DecodedPngImage::new(
            2,
            1,
            vec![0, 1, 0, 2, 0, 3, 1, 1, 1, 2, 1, 3],
            Some(vec![0xff, 0xff, 0x12, 0x34]),
        );
        decoded.sample_depth = crate::image_store::RasterSampleDepth::Sixteen;
        let RasterBorderImageTileSource::EdgeExtended { image, .. } =
            raster_border_image_tile_source(
                &decoded,
                &crate::resource::ResourceCache::default(),
                BorderImageSourceRect::new(0.0, 0.0, 2.0, 1.0),
                BorderImageSourceRect::new(0.25, 0.0, 1.0, 1.0),
            )
            .expect("fractional source is materialized")
        else {
            panic!("fractional source must not use an integer crop");
        };

        assert_eq!(
            image.sample_depth,
            crate::image_store::RasterSampleDepth::Sixteen
        );
        assert_eq!(image.color_space, decoded.color_space);
        assert_eq!(image.rgb.len(), 3 * 3 * 3 * 2);
        assert_eq!(image.alpha.as_deref().map(<[u8]>::len), Some(3 * 3 * 2));
    }

    #[test]
    fn partial_repeat_tile_materializes_its_fractional_source_phase() {
        let decoded = DecodedPngImage::new(20, 16, vec![0; 20 * 16 * 3], None);
        let mut images = Vec::new();

        push_border_image_tiles(
            &mut images,
            &crate::resource::ResourceCache::default(),
            RasterBorderImageTilePaint {
                decoded: &decoded,
                destination: RenderedImageTileRect::from_paint_rect(paint_space_rect(
                    0.0, 10.0, 14.0, 3.0,
                )),
                source_image_bounds: BorderImageSourceRect::new(0.0, 0.0, 20.0, 16.0),
                source: BorderImageSourceRect::new(2.0, 3.0, 4.0, 2.0),
                tile_size: PaintSize::new(6.0, 3.0),
                repeat_x: css::BorderImageRepeatKeyword::Repeat,
                repeat_y: css::BorderImageRepeatKeyword::Stretch,
                sampling: false.into(),
            },
        );

        assert_eq!(images.len(), 3);
        assert!(images.iter().all(|image| image.is_clipped()));
        assert_eq!(
            images[0].clip().expect("partial tile clip").commands,
            paint_rect_path_commands(paint_space_rect(0.0, 10.0, 4.0, 3.0)),
        );
    }

    #[test]
    fn center_scale_uses_unscaled_source_when_both_edges_are_zero() {
        assert_eq!(border_image_center_axis_scale(0.0, 100.0, 0.0, 100.0), 1.0);
    }

    #[test]
    fn center_scale_uses_the_opposite_edge_when_the_first_is_zero() {
        assert_eq!(border_image_center_axis_scale(0.0, 100.0, 50.0, 100.0), 0.5);
    }
}
