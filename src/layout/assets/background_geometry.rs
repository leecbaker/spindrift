use super::*;

/// A CSS background positioning or clipping area in one bottom-left-origin
/// coordinate space.
///
/// CSS Backgrounds resolves positioning, clipping, and repeat geometry in a
/// common coordinate system.  Keeping the marker on that rectangle prevents
/// document-canvas geometry from reaching page-local primitive emission before
/// the page-projection boundary:
/// <https://www.w3.org/TR/css-backgrounds-3/#backgrounds>.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(transparent)]
pub(in crate::layout) struct BackgroundArea<Space>(euclid::Rect<f32, Space>);

/// A background area ready for page-local paint primitive emission.
pub(in crate::layout) type PaintBackgroundArea = BackgroundArea<PaintSpace>;
/// A background area positioned on the assembled document canvas.
pub(in crate::layout) type DocumentCanvasBackgroundArea = BackgroundArea<DocumentCanvasSpace>;

impl<Space> BackgroundArea<Space> {
    pub(in crate::layout) fn new(
        origin: euclid::Point2D<f32, Space>,
        size: euclid::Size2D<f32, Space>,
    ) -> Self {
        Self(euclid::Rect::new(
            origin,
            euclid::Size2D::new(size.width.max(0.0), size.height.max(0.0)),
        ))
    }

    pub(in crate::layout) fn x(&self) -> f32 {
        self.0.origin.x
    }

    pub(in crate::layout) fn y(&self) -> f32 {
        self.0.origin.y
    }

    pub(in crate::layout) fn width(&self) -> f32 {
        self.0.size.width
    }

    pub(in crate::layout) fn height(&self) -> f32 {
        self.0.size.height
    }

    pub(in crate::layout) fn size(&self) -> euclid::Size2D<f32, Space> {
        self.0.size
    }

    pub(in crate::layout) fn inset(self, edges: css::Edges) -> Self {
        Self::new(
            euclid::Point2D::new(self.x() + edges.left, self.y() + edges.bottom),
            euclid::Size2D::new(
                (self.width() - edges.left - edges.right).max(0.0),
                (self.height() - edges.top - edges.bottom).max(0.0),
            ),
        )
    }

    pub(in crate::layout) fn intersect(self, other: Self) -> Option<Self> {
        self.0.intersection(&other.0).map(Self)
    }
}

impl BackgroundArea<PaintSpace> {
    pub(in crate::layout) fn from_paint_rect(rect: PaintRect) -> Self {
        Self(rect)
    }

    pub(in crate::layout) fn paint_rect(self) -> PaintRect {
        self.0
    }
}

impl BackgroundArea<DocumentCanvasSpace> {
    pub(in crate::layout) fn from_document_canvas_rect(rect: DocumentCanvasRect) -> Self {
        Self(rect)
    }

    /// Project a document-canvas area onto one page before page-local
    /// primitive emission.
    pub(in crate::layout) fn project_to_paint(
        self,
        page_document_bottom: f32,
    ) -> PaintBackgroundArea {
        PaintBackgroundArea::new(
            PaintPoint::new(self.x(), self.y() - page_document_bottom),
            PaintSize::new(self.width(), self.height()),
        )
    }
}

/// The distinct coordinate areas used to resolve a background image.
///
/// The selected `background-origin` area positions a normal layer, its
/// `background-clip` area bounds painting, and a fixed layer may instead use
/// a viewport-equivalent positioning area. CSS defines these independently:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin>,
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-attachment>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct BackgroundPaintAreas<Space> {
    pub(in crate::layout) positioning_border_area: BackgroundArea<Space>,
    pub(in crate::layout) clip_border_area: BackgroundArea<Space>,
    pub(in crate::layout) fixed_positioning_area: Option<BackgroundArea<Space>>,
    pub(in crate::layout) fixed_attachment_is_scrolled_by_transform: bool,
}

/// Fully resolved geometry for one CSS background layer.
///
/// CSS Backgrounds resolves the positioning area, used image size, position,
/// and repeat behavior before painting the image. Keeping that result together
/// ensures vector and raster image sources use identical tile geometry:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-image>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct ResolvedBackgroundTile {
    pub(in crate::layout) positioning_area: PaintBackgroundArea,
    pub(in crate::layout) clip_area: PaintBackgroundArea,
    pub(in crate::layout) rounded_clip: Option<RenderedPathClip>,
    pub(in crate::layout) size: PaintSize,
    pub(in crate::layout) offset: PaintBackgroundOffset,
    pub(in crate::layout) repeat: css::BackgroundRepeat,
}

impl ResolvedBackgroundTile {
    pub(in crate::layout) fn new(
        positioning_area: PaintBackgroundArea,
        clip_area: PaintBackgroundArea,
        rounded_clip: Option<RenderedPathClip>,
        layer: &css::BackgroundLayer,
        size: PaintSize,
    ) -> Self {
        let offset = background_position(layer.position.clone(), positioning_area.size(), size);
        Self {
            positioning_area,
            clip_area,
            rounded_clip,
            size,
            offset,
            repeat: layer.repeat,
        }
    }

    pub(in crate::layout) fn tile_xs(&self) -> Vec<f32> {
        let repeat = self.repeat.x_axis();
        let (area_start, area_size) = if repeat == css::BackgroundRepeatAxis::Space {
            (self.positioning_area.x(), self.positioning_area.width())
        } else {
            (self.clip_area.x(), self.clip_area.width())
        };
        background_tile_positions(
            (f64::from(self.positioning_area.x()) + self.offset.x) as f32,
            area_start,
            area_size,
            self.size.width,
            repeat,
        )
    }

    pub(in crate::layout) fn tile_ys(&self) -> Vec<f32> {
        let repeat = self.repeat.y_axis();
        let (area_start, area_size) = if repeat == css::BackgroundRepeatAxis::Space {
            (self.positioning_area.y(), self.positioning_area.height())
        } else {
            (self.clip_area.y(), self.clip_area.height())
        };
        background_tile_positions(
            (f64::from(self.positioning_area.y()) + self.offset.y) as f32,
            area_start,
            area_size,
            self.size.height,
            repeat,
        )
    }

    pub(in crate::layout) fn tiles(&self) -> Vec<PaintBackgroundArea> {
        let tile_xs = self.tile_xs();
        self.tile_ys()
            .into_iter()
            .flat_map(|y| {
                tile_xs
                    .iter()
                    .cloned()
                    .map(move |x| PaintBackgroundArea::new(PaintPoint::new(x, y), self.size))
            })
            .collect()
    }
}
pub(in crate::layout) fn used_background_layer_size(
    decoded: &DecodedPngImage,
    layer: &css::BackgroundLayer,
    positioning_area: PaintSize,
) -> PaintSize {
    let Some(image) = layer.image.as_image() else {
        return PaintSize::new(0.0, 0.0);
    };
    let selected_image = image.selected_image();
    let generated_image = matches!(
        selected_image,
        BackgroundImage::LinearGradient(_)
            | BackgroundImage::RadialGradient(_)
            | BackgroundImage::ConicGradient(_)
            | BackgroundImage::CssColor(_)
    );
    let mut size = if generated_image {
        used_generated_background_size(positioning_area, layer.size.clone())
    } else {
        used_background_size(
            decoded,
            positioning_area,
            layer.size.clone(),
            image.intrinsic_resolution(),
        )
    };

    let (width_is_auto, height_is_auto) = match &layer.size {
        css::BackgroundSize::Auto => (true, true),
        css::BackgroundSize::Explicit { width, height } => (
            matches!(width, css::BackgroundSizeAxis::Auto),
            matches!(height, css::BackgroundSizeAxis::Auto),
        ),
        css::BackgroundSize::Cover | css::BackgroundSize::Contain => (false, false),
    };
    let aspect_ratio = (!generated_image && decoded.pixel_height > 0)
        .then(|| decoded.pixel_width as f32 / decoded.pixel_height as f32);
    if layer.repeat.x_axis() == css::BackgroundRepeatAxis::Round {
        size.width = rounded_background_tile_size(size.width, positioning_area.width);
        if height_is_auto && let Some(aspect_ratio) = aspect_ratio {
            size.height = size.width / aspect_ratio;
        }
    }
    if layer.repeat.y_axis() == css::BackgroundRepeatAxis::Round {
        size.height = rounded_background_tile_size(size.height, positioning_area.height);
        if width_is_auto && let Some(aspect_ratio) = aspect_ratio {
            size.width = size.height * aspect_ratio;
        }
    }
    size
}

pub(in crate::layout) fn used_generated_background_layer_size(
    layer: &css::BackgroundLayer,
    positioning_area: PaintSize,
) -> PaintSize {
    let mut size = used_generated_background_size(positioning_area, layer.size.clone());
    if layer.repeat.x_axis() == css::BackgroundRepeatAxis::Round {
        size.width = rounded_background_tile_size(size.width, positioning_area.width);
    }
    if layer.repeat.y_axis() == css::BackgroundRepeatAxis::Round {
        size.height = rounded_background_tile_size(size.height, positioning_area.height);
    }
    size
}

pub(in crate::layout) fn rounded_background_tile_size(tile_size: f32, area_size: f32) -> f32 {
    if tile_size <= 0.0 || area_size <= 0.0 {
        return tile_size;
    }
    let count = (area_size / tile_size).round().max(1.0);
    area_size / count
}

pub(in crate::layout) fn used_generated_background_size(
    positioning_area: PaintSize,
    value: css::BackgroundSize,
) -> PaintSize {
    match value {
        css::BackgroundSize::Auto | css::BackgroundSize::Cover | css::BackgroundSize::Contain => {
            positioning_area
        }
        css::BackgroundSize::Explicit { width, height } => {
            let used_width = used_background_size_axis(width, positioning_area.width)
                .unwrap_or(positioning_area.width);
            let used_height = used_background_size_axis(height, positioning_area.height)
                .unwrap_or(positioning_area.height);
            PaintSize::new(used_width, used_height)
        }
    }
}

pub(in crate::layout) fn background_layers_for_paint(
    style: &ComputedStyle,
) -> Vec<css::BackgroundLayer> {
    if !style.background.background_layers.is_empty() {
        return style.background.background_layers.clone();
    }
    vec![css::BackgroundLayer {
        image: style.background.background_image.clone(),
        position: style.background.background_position.clone(),
        size: style.background.background_size.clone(),
        repeat: style.background.background_repeat,
        attachment: style.background.background_attachment,
        origin: style.background.background_origin,
        clip: style.background.background_clip,
    }]
}

pub(in crate::layout) fn background_paint_area_for_box<Space>(
    area: BackgroundArea<Space>,
    style: &ComputedStyle,
    box_: css::BackgroundBox,
) -> BackgroundArea<Space> {
    let border = used_border_widths(style);
    match box_ {
        css::BackgroundBox::Border | css::BackgroundBox::BorderArea => area,
        css::BackgroundBox::Padding => area.inset(border),
        css::BackgroundBox::Content => area.inset(border).inset(style.padding),
    }
}

/// Resolve the background positioning area for one layer.
///
/// `background-origin` selects an element box for scroll/local layers. A
/// fixed layer instead uses the viewport-equivalent area supplied by layout,
/// so its origin keyword has no effect.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-attachment>
pub(in crate::layout) fn background_positioning_area_for_layer<Space>(
    positioning_border_area: BackgroundArea<Space>,
    fixed_positioning_area: Option<BackgroundArea<Space>>,
    fixed_attachment_is_scrolled_by_transform: bool,
    style: &ComputedStyle,
    layer: &css::BackgroundLayer,
) -> BackgroundArea<Space> {
    match (layer.attachment, fixed_positioning_area) {
        // A transform turns a non-root fixed background into a scroll
        // background. Its image is part of the transformed element's paint
        // subtree rather than a viewport-fixed source.
        // <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
        (css::BackgroundAttachment::Fixed, _) if fixed_attachment_is_scrolled_by_transform => {
            background_paint_area_for_box(positioning_border_area, style, layer.origin)
        }
        (css::BackgroundAttachment::Fixed, Some(area)) => area,
        _ => background_paint_area_for_box(positioning_border_area, style, layer.origin),
    }
}

/// Clip a raster background image to its destination-space paint area.
///
/// A CSS clip constrains the image's output; it must not change the mapping
/// between source pixels and its original destination tile. In particular,
/// converting a fractional source-pixel edge to an integer PDF source rect
/// would rescale the retained pixels. Keep the image geometry intact and
/// express any partial tile through a PDF destination clip instead.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-clip>
pub(in crate::layout) fn clip_background_image_to_paint_area(
    mut image: RenderedImage,
    clip: PaintBackgroundArea,
    rounded_clip: Option<RenderedPathClip>,
) -> Option<RenderedImage> {
    let image_rect = image.paint_rect();
    let visible = image_rect.intersection(&clip.paint_rect())?;
    let clip = if visible != image_rect {
        let mut rectangular_clip = RenderedPathClip::new(
            paint_rect_path_commands(visible),
            RenderedPathFillRule::NonZero,
            Vec::new(),
        );
        if let Some(rounded_clip) = rounded_clip {
            rectangular_clip
                .additional_clips
                .push(RenderedPathClipPath::new(
                    rounded_clip.commands,
                    rounded_clip.fill_rule,
                ));
            rectangular_clip
                .additional_clips
                .extend(rounded_clip.additional_clips);
        }
        Some(rectangular_clip)
    } else {
        rounded_clip
    };
    if let Some(clip) = clip {
        image = image.with_clip(clip);
    }
    Some(image)
}

/// Returns tile origins that intersect a background positioning area.
///
/// CSS Backgrounds and Borders repeats from the positioned first tile in both
/// directions as needed, but PDF emission needs a finite set of image
/// placements for the current painted area:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
pub(in crate::layout) fn background_tile_positions(
    positioned_start: f32,
    area_start: f32,
    area_size: f32,
    tile_size: f32,
    repeat: css::BackgroundRepeatAxis,
) -> Vec<f32> {
    if tile_size <= 0.0 {
        return Vec::new();
    }
    match repeat {
        css::BackgroundRepeatAxis::NoRepeat => return vec![positioned_start],
        css::BackgroundRepeatAxis::Space => {
            let count = spaced_background_tile_count(tile_size, area_size);
            if count < 2 {
                return vec![positioned_start];
            }
            let step = spaced_background_tile_step(tile_size, area_size, count);
            return (0..count)
                .map(|index| area_start + index as f32 * step)
                .collect();
        }
        css::BackgroundRepeatAxis::Repeat | css::BackgroundRepeatAxis::Round => {}
    }
    if area_size <= 0.0 {
        return Vec::new();
    }

    let area_end = area_start + area_size;
    let mut first = positioned_start;
    while first > area_start {
        first -= tile_size;
    }
    while first + tile_size <= area_start {
        first += tile_size;
    }

    let mut positions = Vec::new();
    let mut current = first;
    while current < area_end {
        positions.push(current);
        current += tile_size;
    }
    positions
}

pub(in crate::layout) fn background_first_tile_position(
    positioned_start: f32,
    area_start: f32,
    area_size: f32,
    tile_size: f32,
    repeat: css::BackgroundRepeatAxis,
) -> f32 {
    match repeat {
        css::BackgroundRepeatAxis::NoRepeat => return positioned_start,
        css::BackgroundRepeatAxis::Space => {
            if spaced_background_tile_count(tile_size, area_size) >= 2 {
                return area_start;
            }
            return positioned_start;
        }
        css::BackgroundRepeatAxis::Repeat | css::BackgroundRepeatAxis::Round => {}
    }
    if tile_size <= 0.0 {
        return positioned_start;
    }
    let mut first = positioned_start;
    while first > area_start {
        first -= tile_size;
    }
    while first + tile_size <= area_start {
        first += tile_size;
    }
    first
}

pub(in crate::layout) fn background_pattern_step(
    tile_size: f32,
    area_size: f32,
    repeat: css::BackgroundRepeatAxis,
) -> f32 {
    match repeat {
        css::BackgroundRepeatAxis::Repeat | css::BackgroundRepeatAxis::Round => tile_size,
        css::BackgroundRepeatAxis::Space => {
            let count = spaced_background_tile_count(tile_size, area_size);
            if count >= 2 {
                spaced_background_tile_step(tile_size, area_size, count)
            } else {
                non_repeating_pattern_step(tile_size, area_size)
            }
        }
        css::BackgroundRepeatAxis::NoRepeat => non_repeating_pattern_step(tile_size, area_size),
    }
}

pub(in crate::layout) fn spaced_background_tile_count(tile_size: f32, area_size: f32) -> usize {
    if tile_size <= 0.0 || area_size <= 0.0 {
        return 0;
    }
    (area_size / tile_size).floor().max(1.0) as usize
}

pub(in crate::layout) fn spaced_background_tile_step(
    tile_size: f32,
    area_size: f32,
    count: usize,
) -> f32 {
    if count < 2 {
        return non_repeating_pattern_step(tile_size, area_size);
    }
    (area_size - tile_size) / (count - 1) as f32
}

pub(in crate::layout) fn non_repeating_pattern_step(tile_size: f32, area_size: f32) -> f32 {
    tile_size.max(area_size.abs() * 2.0 + tile_size)
}
