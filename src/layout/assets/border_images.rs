use super::*;

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
    pub(in crate::layout) source_offset: u32,
    pub(in crate::layout) source_size: u32,
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
    decoded: &DecodedPngImage,
    destination: RenderedImageTileRect,
    source: RenderedImageSourceRect,
    repeat_x: css::BorderImageRepeatKeyword,
    repeat_y: css::BorderImageRepeatKeyword,
    interpolate: bool,
) {
    let tile_size = border_image_base_tile_size(destination, source, repeat_x, repeat_y);
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
                || x_segment.source_size == 0
                || y_segment.source_size == 0
            {
                continue;
            }
            images.push(
                RenderedImage::from_paint_rect(
                    paint_space_rect(
                        destination.x() + x_segment.destination_offset,
                        destination.y() + y_segment.destination_offset,
                        x_segment.destination_size,
                        y_segment.destination_size,
                    ),
                    true,
                    decoded.pixel_width,
                    decoded.pixel_height,
                    Some(RenderedImageSourceRect {
                        x: source.x + x_segment.source_offset,
                        y: source.y + y_segment.source_offset,
                        width: x_segment.source_size,
                        height: y_segment.source_size,
                    }),
                    interpolate,
                    decoded.rgb.shared(),
                    decoded.alpha.clone(),
                    None,
                )
                .with_raster_color_space(decoded.color_space.clone())
                .with_image_id(decoded.image_id),
            );
        }
    }
}

pub(in crate::layout) fn border_image_base_tile_size(
    destination: RenderedImageTileRect,
    source: RenderedImageSourceRect,
    repeat_x: css::BorderImageRepeatKeyword,
    repeat_y: css::BorderImageRepeatKeyword,
) -> PaintSize {
    // Raster-image dimensions are CSS pixels, while the destination is laid
    // out in PDF points. Convert before deriving cross-axis tile scaling.
    let mut tile_width = source.width as f32 * css::CSS_PX_TO_PT;
    let mut tile_height = source.height as f32 * css::CSS_PX_TO_PT;
    if repeat_x != css::BorderImageRepeatKeyword::Stretch
        && repeat_y == css::BorderImageRepeatKeyword::Stretch
        && source.height > 0
    {
        let scale = destination.height() / (source.height as f32 * css::CSS_PX_TO_PT);
        tile_width *= scale;
    }
    if repeat_y != css::BorderImageRepeatKeyword::Stretch
        && repeat_x == css::BorderImageRepeatKeyword::Stretch
        && source.width > 0
    {
        let scale = destination.width() / (source.width as f32 * css::CSS_PX_TO_PT);
        tile_height *= scale;
    }
    if repeat_x == css::BorderImageRepeatKeyword::Stretch {
        tile_width = destination.width();
    }
    if repeat_y == css::BorderImageRepeatKeyword::Stretch {
        tile_height = destination.height();
    }
    PaintSize::new(tile_width.max(0.0), tile_height.max(0.0))
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
    source_size: u32,
) -> Vec<BorderImageTileSegment> {
    if destination_size <= 0.0 || source_size == 0 {
        return Vec::new();
    }
    if repeat == css::BorderImageRepeatKeyword::Stretch || base_tile_size <= 0.0 {
        return vec![BorderImageTileSegment {
            destination_offset: 0.0,
            destination_size,
            source_offset: 0,
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
                    source_offset: 0,
                    source_size,
                })
                .collect()
        }
        css::BorderImageRepeatKeyword::Space => {
            let count = (destination_size / base_tile_size).floor() as usize;
            let count = count.max(1);
            let tile_size = base_tile_size.min(destination_size);
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
                    source_offset: 0,
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
    source_size: u32,
) -> Vec<BorderImageTileSegment> {
    if destination_size <= 0.0 || tile_size <= 0.0 || source_size == 0 {
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
        let source_start = ((visible_start - tile_start) * source_size as f32 / tile_size)
            .round()
            .clamp(0.0, source_size as f32) as u32;
        let source_end = ((visible_end - tile_start) * source_size as f32 / tile_size)
            .round()
            .clamp(source_start as f32, source_size as f32) as u32;
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
