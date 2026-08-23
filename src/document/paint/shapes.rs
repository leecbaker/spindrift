use super::geometry::{
    PaintClip, PaintPoint, PaintRect, PaintSize, PaintStrokeWidth, PaintTranslation,
};
use crate::{CssColor, css};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedRoundedRectRadii {
    pub top_left: RenderedCornerRadius,
    pub top_right: RenderedCornerRadius,
    pub bottom_right: RenderedCornerRadius,
    pub bottom_left: RenderedCornerRadius,
}

#[allow(dead_code)]
impl RenderedRoundedRectRadii {
    pub const ZERO: Self = Self {
        top_left: RenderedCornerRadius::ZERO,
        top_right: RenderedCornerRadius::ZERO,
        bottom_right: RenderedCornerRadius::ZERO,
        bottom_left: RenderedCornerRadius::ZERO,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedCornerRadius {
    pub(in crate::document) size: PaintSize,
}

#[allow(dead_code)]
impl RenderedCornerRadius {
    pub const ZERO: Self = Self {
        size: PaintSize::new(0.0, 0.0),
    };

    pub fn new(x: f32, y: f32) -> Self {
        Self {
            size: PaintSize::new(x.max(0.0), y.max(0.0)),
        }
    }

    pub fn x(&self) -> f32 {
        self.size.width
    }

    pub fn y(&self) -> f32 {
        self.size.height
    }

    pub(crate) fn inset(&mut self, inset: f32) {
        self.size.width = (self.size.width - inset).max(0.0);
        self.size.height = (self.size.height - inset).max(0.0);
    }

    pub(crate) fn scale(&mut self, factor: f32) {
        self.size.width *= factor;
        self.size.height *= factor;
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedRect {
    pub(in crate::document) rect: PaintRect,
    pub fill: Option<CssColor>,
    pub stroke: Option<CssColor>,
    pub stroke_width: PaintStrokeWidth,
    /// An opaque decorative rectangle may explicitly remove the hidden
    /// background beneath it during PDF realization. Gap rules use this to
    /// prevent the background from leaking through a rasterized rule edge.
    pub(crate) culls_opaque_underpaint: bool,
    /// A PDF-private background for a later opaque edge; it is intentionally
    /// retained so that edge's fractional coverage has the correct backdrop.
    pub(crate) preserves_opaque_backdrop: bool,
}

impl RenderedRect {
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        fill: Option<CssColor>,
        stroke: Option<CssColor>,
        stroke_width: PaintStrokeWidth,
    ) -> Self {
        Self {
            rect: PaintRect::new(
                PaintPoint::new(x, y),
                PaintSize::new(width.max(0.0), height.max(0.0)),
            ),
            fill,
            stroke,
            stroke_width,
            culls_opaque_underpaint: false,
            preserves_opaque_backdrop: false,
        }
    }

    pub(crate) fn with_opaque_underpaint_culling(mut self) -> Self {
        self.culls_opaque_underpaint = true;
        self
    }

    pub(crate) fn with_opaque_backdrop_preservation(mut self) -> Self {
        self.preserves_opaque_backdrop = true;
        self
    }

    pub(crate) fn from_paint_rect(rect: PaintRect, fill: Option<CssColor>) -> Self {
        Self {
            rect,
            fill,
            stroke: None,
            stroke_width: PaintStrokeWidth::ZERO,
            culls_opaque_underpaint: false,
            preserves_opaque_backdrop: false,
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

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedRoundedRect {
    pub(in crate::document) rect: PaintRect,
    pub radii: RenderedRoundedRectRadii,
    /// CSS Borders 4 corner contours carried with an otherwise rounded box.
    ///
    /// Rounded rectangles remain a compact paint primitive for the common
    /// `round` case, while overflow clips also need to preserve a non-round
    /// `corner-shape` contour through to PDF path emission:
    /// <https://drafts.csswg.org/css-borders-4/#corner-shaping>.
    pub(crate) corner_shapes: css::CornerShapes,
    pub fill: Option<CssColor>,
    pub stroke: Option<CssColor>,
    pub stroke_width: PaintStrokeWidth,
}

#[allow(dead_code)]
impl RenderedRoundedRect {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radii: RenderedRoundedRectRadii,
        fill: Option<CssColor>,
        stroke: Option<CssColor>,
        stroke_width: PaintStrokeWidth,
    ) -> Self {
        Self::from_paint_rect(
            PaintRect::new(
                PaintPoint::new(x, y),
                PaintSize::new(width.max(0.0), height.max(0.0)),
            ),
            radii,
            fill,
            stroke,
            stroke_width,
        )
    }

    pub(crate) fn from_paint_rect(
        rect: PaintRect,
        radii: RenderedRoundedRectRadii,
        fill: Option<CssColor>,
        stroke: Option<CssColor>,
        stroke_width: PaintStrokeWidth,
    ) -> Self {
        Self {
            rect,
            radii,
            corner_shapes: css::CornerShapes::ROUND,
            fill,
            stroke,
            stroke_width,
        }
    }

    pub fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(self) -> f32 {
        self.rect.size.width
    }

    pub fn height(self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(self) -> PaintRect {
        self.rect
    }

    /// Retain CSS Borders 4 corner contours for a rounded clip or paint
    /// primitive whose radii were resolved from this box.
    pub(crate) fn with_corner_shapes(mut self, shapes: css::CornerShapes) -> Self {
        self.corner_shapes = shapes;
        self
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        self
    }
}

/// A generic PDF path paint primitive used when a CSS feature cannot be
/// represented by a rectangle, rounded rectangle, or single stroke.
///
/// CSS Backgrounds and Borders Level 3 models border areas as curved regions,
/// and PDF content streams represent those regions with path construction and
/// painting operators: <https://www.w3.org/TR/css-backgrounds-3/#borders> and
/// ISO 32000-1:2008, 8.5 "Path Construction and Painting".

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedStroke {
    pub(in crate::document) start: PaintPoint,
    pub(in crate::document) end: PaintPoint,
    pub stroke_width: PaintStrokeWidth,
    pub color: CssColor,
    pub dash: Option<(f32, f32)>,
}

#[allow(dead_code)]
impl RenderedStroke {
    pub fn new(
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        stroke_width: PaintStrokeWidth,
        color: CssColor,
        dash: Option<(f32, f32)>,
    ) -> Self {
        Self::from_paint_points(
            PaintPoint::new(x1, y1),
            PaintPoint::new(x2, y2),
            stroke_width,
            color,
            dash,
        )
    }

    pub(crate) fn from_paint_points(
        start: PaintPoint,
        end: PaintPoint,
        stroke_width: PaintStrokeWidth,
        color: CssColor,
        dash: Option<(f32, f32)>,
    ) -> Self {
        Self {
            start,
            end,
            stroke_width,
            color,
            dash,
        }
    }

    pub fn x1(&self) -> f32 {
        self.start.x
    }

    pub fn y1(&self) -> f32 {
        self.start.y
    }

    pub fn x2(&self) -> f32 {
        self.end.x
    }

    pub fn y2(&self) -> f32 {
        self.end.y
    }

    pub(crate) fn paint_points(self) -> (PaintPoint, PaintPoint) {
        (self.start, self.end)
    }

    pub(crate) fn paint_bounds(self) -> PaintClip {
        let (start, end) = self.paint_points();
        let half = self.stroke_width.points() / 2.0;
        let left = start.x.min(end.x) - half;
        let right = start.x.max(end.x) + half;
        let bottom = start.y.min(end.y) - half;
        let top = start.y.max(end.y) + half;
        PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(left, bottom),
            PaintSize::new((right - left).max(0.0), (top - bottom).max(0.0)),
        ))
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.start = offset.transform_point(self.start);
        self.end = offset.transform_point(self.end);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderedRect, RenderedStroke};
    use crate::CssColor;
    use crate::document::paint::geometry::{
        PaintClip, PaintPoint, PaintRect, PaintSize, PaintStrokeWidth,
    };

    #[test]
    fn rendered_rect_exposes_paint_rect() {
        let rect = PaintRect::new(PaintPoint::new(3.0, 4.0), PaintSize::new(5.0, 6.0));
        let rendered = RenderedRect::from_paint_rect(rect, Some(CssColor::BLACK));

        assert_eq!(rendered.paint_rect(), rect);
        assert_eq!(rendered.fill, Some(CssColor::BLACK));
    }

    #[test]
    fn stroke_exposes_typed_paint_points_and_bounds() {
        let stroke = RenderedStroke::from_paint_points(
            PaintPoint::new(10.0, 20.0),
            PaintPoint::new(30.0, 40.0),
            PaintStrokeWidth::new(4.0),
            CssColor::BLACK,
            None,
        );
        assert_eq!(
            stroke.paint_points(),
            (PaintPoint::new(10.0, 20.0), PaintPoint::new(30.0, 40.0))
        );
        assert_eq!(stroke.stroke_width, PaintStrokeWidth::new(4.0));
        assert_eq!(stroke.paint_bounds(), PaintClip::new(8.0, 18.0, 24.0, 24.0));
    }
}
