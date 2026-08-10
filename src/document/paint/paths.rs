use crate::CssColor;

use super::geometry::{
    PaintClip, PaintPoint, PaintRect, PaintSize, PaintStrokeWidth, PaintTransform, PaintTranslation,
};

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPath {
    pub clip: Option<RenderedPathClip>,
    pub(crate) transform: PaintTransform,
    pub commands: Vec<RenderedPathCommand>,
    pub fill: Option<CssColor>,
    pub(crate) fill_paint: Option<RenderedPathPaint>,
    pub fill_rule: RenderedPathFillRule,
    pub stroke: Option<CssColor>,
    pub(crate) stroke_paint: Option<RenderedPathPaint>,
    pub stroke_width: PaintStrokeWidth,
    pub(crate) stroke_style: RenderedPathStrokeStyle,
    pub(crate) paint_order: RenderedPathPaintOrder,
    /// A page-space opaque region whose visible ink is supplied by this path.
    ///
    /// This is intentionally narrower than a generic path bound: it is set
    /// only for paths proven to cover the rectangle opaquely, allowing the
    /// PDF serializer to omit an entirely hidden earlier fill before either
    /// edge is antialiased.
    pub(crate) opaque_coverage_rect: Option<PaintRect>,
}

/// A vector paint source for a [`RenderedPath`].
///
/// Gradient paint servers are retained as typed geometry instead of being
/// sampled into raster pixels. PDF axial and radial shadings provide the
/// corresponding vector primitive for SVG and CSS Images gradients: SVG 2,
/// 13.2; CSS Images 3, 3.4 and 3.5; and ISO 32000-2:2020, 8.7.4.3.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RenderedPathPaint {
    Solid(CssColor),
    Gradient(RenderedGradient),
    SvgPattern(RenderedSvgPathPattern),
}

impl RenderedPathPaint {
    fn solid_color(&self) -> Option<CssColor> {
        match self {
            Self::Solid(color) => Some(*color),
            Self::Gradient(_) | Self::SvgPattern(_) => None,
        }
    }
}

/// A vector SVG paint-server tile applied while its target path's local CTM
/// is active.
///
/// SVG 2 patterns repeat their children in the target element's user space.
/// Keeping this distinct from a CSS background SVG tile means PDF emission can
/// apply the path's CTM exactly once to both the geometry and the pattern:
/// <https://www.w3.org/TR/SVG2/pservers.html#Patterns>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedSvgPathPattern {
    pub(crate) tile_size: PaintSize,
    pub(crate) origin: PaintPoint,
    pub(crate) transform: PaintTransform,
    pub(crate) paths: Vec<RenderedPath>,
    pub(crate) opacity: f32,
}

/// A normalized linear or radial gradient in the path's local paint space.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedGradient {
    pub(crate) kind: RenderedGradientKind,
    /// The common component space used by every color stop.
    pub(crate) color_space: crate::css::CssColorSpace,
    pub(crate) stops: Vec<RenderedGradientStop>,
    /// A single CSS repeating-gradient cycle evaluated by a PDF calculator
    /// function. SVG and finite CSS gradients leave this unset.
    pub(crate) periodic: Option<Box<RenderedPeriodicGradient>>,
    /// Maps the gradient's local coordinates to paint-space coordinates.
    ///
    /// This is SVG's `gradientTransform` for SVG gradients, and represents
    /// the affine ellipse transform for CSS radial gradients.
    pub(crate) transform: PaintTransform,
}

impl RenderedGradient {
    pub(crate) fn has_transparent_stop(&self) -> bool {
        self.periodic
            .as_ref()
            .map_or(&self.stops, |periodic| &periodic.stops)
            .iter()
            .any(|stop| !stop.color.is_opaque())
    }
}

/// One resolved CSS repeating-gradient cycle in paint-space units.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedPeriodicGradient {
    pub(crate) stops: Vec<RenderedGradientStop>,
    pub(crate) start: f32,
    pub(crate) period: f32,
    pub(crate) domain_length: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RenderedGradientKind {
    Linear {
        start: PaintPoint,
        end: PaintPoint,
    },
    Radial {
        start_center: PaintPoint,
        start_radius: f32,
        end_center: PaintPoint,
        end_radius: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedGradientStop {
    pub(crate) offset: f32,
    pub(crate) color: CssColor,
    /// Exponent for the interval beginning at this stop. CSS transition hints
    /// map directly to PDF Type 2 exponential interpolation functions.
    /// CSS Images 3, 3.4.2 defines the exponent as `log_H(.5)`.
    pub(crate) interpolation_exponent: f32,
}

/// Stroke state for a vector path.
///
/// PDF's line cap, join, miter and dash graphics-state parameters correspond
/// directly to SVG's `stroke-*` properties: ISO 32000-1:2008, 8.4.3 and SVG
/// 2, 13.5.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedPathStrokeStyle {
    pub(crate) line_cap: RenderedPathLineCap,
    pub(crate) line_join: RenderedPathLineJoin,
    pub(crate) miter_limit: f32,
    pub(crate) dash_array: Vec<f32>,
    pub(crate) dash_offset: f32,
}

impl Default for RenderedPathStrokeStyle {
    fn default() -> Self {
        Self {
            line_cap: RenderedPathLineCap::Butt,
            line_join: RenderedPathLineJoin::Miter,
            miter_limit: 10.0,
            dash_array: Vec::new(),
            dash_offset: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderedPathLineCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderedPathLineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RenderedPathPaintOrder {
    #[default]
    FillThenStroke,
    StrokeThenFill,
}

#[allow(dead_code)]
impl RenderedPath {
    /// Return a conservative page-space bounding box for this path.
    ///
    /// SVG paths retain local commands plus a paint transform. Consumers that
    /// inspect rendered output therefore need transformed geometry rather than
    /// raw command coordinates. Bézier control points bound their curve, so
    /// this is conservative for curved segments.
    pub fn bounds(&self) -> Option<PaintRect> {
        let mut left = f32::INFINITY;
        let mut bottom = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        let mut top = f32::NEG_INFINITY;
        let mut include = |point: PaintPoint| {
            let point = self.transform.apply_point(point);
            left = left.min(point.x);
            bottom = bottom.min(point.y);
            right = right.max(point.x);
            top = top.max(point.y);
        };
        for command in &self.commands {
            match command {
                RenderedPathCommand::MoveTo(point) | RenderedPathCommand::LineTo(point) => {
                    include(*point);
                }
                RenderedPathCommand::CurveTo {
                    control_1,
                    control_2,
                    end,
                } => {
                    include(*control_1);
                    include(*control_2);
                    include(*end);
                }
                RenderedPathCommand::Close => {}
            }
        }
        (left.is_finite() && bottom.is_finite() && right.is_finite() && top.is_finite()).then(
            || {
                PaintRect::new(
                    PaintPoint::new(left, bottom),
                    PaintSize::new((right - left).max(0.0), (top - bottom).max(0.0)),
                )
            },
        )
    }

    pub(crate) fn new(
        commands: Vec<RenderedPathCommand>,
        fill: Option<CssColor>,
        fill_rule: RenderedPathFillRule,
        stroke: Option<CssColor>,
        stroke_width: PaintStrokeWidth,
        clip: Option<RenderedPathClip>,
    ) -> Self {
        Self {
            clip,
            transform: PaintTransform::identity(),
            commands,
            fill,
            fill_paint: fill.map(RenderedPathPaint::Solid),
            fill_rule,
            stroke,
            stroke_paint: stroke.map(RenderedPathPaint::Solid),
            stroke_width,
            stroke_style: RenderedPathStrokeStyle::default(),
            paint_order: RenderedPathPaintOrder::default(),
            opaque_coverage_rect: None,
        }
    }

    pub(crate) fn with_paints(
        mut self,
        fill: Option<RenderedPathPaint>,
        stroke: Option<RenderedPathPaint>,
    ) -> Self {
        self.fill = fill.as_ref().and_then(RenderedPathPaint::solid_color);
        self.stroke = stroke.as_ref().and_then(RenderedPathPaint::solid_color);
        self.fill_paint = fill;
        self.stroke_paint = stroke;
        self
    }

    pub(crate) fn with_stroke_style(mut self, stroke_style: RenderedPathStrokeStyle) -> Self {
        self.stroke_style = stroke_style;
        self
    }

    pub(crate) fn with_paint_order(mut self, paint_order: RenderedPathPaintOrder) -> Self {
        self.paint_order = paint_order;
        self
    }

    pub(crate) fn with_transform(mut self, transform: PaintTransform) -> Self {
        self.transform = transform;
        self
    }

    /// Project this retained path, including its PDF clip scope, into a
    /// destination paint space.
    ///
    /// Path clips are installed before the path's own CTM by the PDF writer,
    /// so clip contours must be transformed explicitly while path geometry and
    /// gradient paint remain under the composed CTM. This keeps a fragmented
    /// structural background's source clip and ink in the same destination:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering> and
    /// ISO 32000-2:2020, 8.5.4.
    pub(crate) fn transformed(mut self, transform: PaintTransform) -> Self {
        self.transform = transform.multiply(self.transform);
        if let Some(rect) = self.opaque_coverage_rect {
            self.opaque_coverage_rect = Some(
                transform
                    .apply_clip_to_aabb(PaintClip::from_paint_rect(rect))
                    .paint_rect(),
            );
        }
        if let Some(clip) = &mut self.clip {
            clip.transform(transform);
        }
        // PDF shading-pattern matrices are resolved independently of the
        // path CTM.  Project the retained gradient matrix alongside its path
        // geometry so the fill remains registered with the transformed clip.
        for paint in [&mut self.fill_paint, &mut self.stroke_paint]
            .into_iter()
            .flatten()
        {
            if let RenderedPathPaint::Gradient(gradient) = paint {
                gradient.transform = transform.multiply(gradient.transform);
            }
        }
        self
    }

    /// Mark this path as an opaque replacement for `rect`.
    ///
    /// Callers must establish that every point in the rectangle is covered
    /// by the path using an opaque source; generic path bounds are not enough.
    pub(crate) fn with_opaque_coverage_rect(mut self, rect: PaintRect) -> Self {
        self.opaque_coverage_rect = Some(rect);
        self
    }

    /// Conservative paint-space bounds for the path's transformed geometry.
    ///
    /// This includes path control points, which is sufficient for paint-order
    /// inspection and replaced-element tests; PDF clipping remains represented
    /// independently by [`RenderedPathClip`].
    pub fn paint_bounds(&self) -> Option<PaintRect> {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        let mut include = |point: PaintPoint| {
            let point = self.transform.apply_point(point);
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        };
        for command in &self.commands {
            match *command {
                RenderedPathCommand::MoveTo(point) | RenderedPathCommand::LineTo(point) => {
                    include(point);
                }
                RenderedPathCommand::CurveTo {
                    control_1,
                    control_2,
                    end,
                } => {
                    include(control_1);
                    include(control_2);
                    include(end);
                }
                RenderedPathCommand::Close => {}
            }
        }
        (min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()).then(
            || {
                PaintRect::new(
                    PaintPoint::new(min_x, min_y),
                    PaintSize::new(max_x - min_x, max_y - min_y),
                )
            },
        )
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.opaque_coverage_rect = self
            .opaque_coverage_rect
            .map(|rect| offset.transform_rect(&rect));
        let transformed = self.transform != PaintTransform::identity();
        if transformed {
            self.transform = PaintTransform::translate(offset).multiply(self.transform);
        }
        for paint in [&mut self.fill_paint, &mut self.stroke_paint]
            .into_iter()
            .flatten()
        {
            if let RenderedPathPaint::Gradient(gradient) = paint {
                gradient.transform = PaintTransform::translate(offset).multiply(gradient.transform);
            }
        }
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
        if !transformed {
            for command in &mut self.commands {
                command.translate(offset);
            }
        }
        self
    }
}

/// A PDF path clipping scope applied before painting a vector path.
///
/// PDF clipping paths are established with `W`/`W*` and the current path, then
/// later drawing is limited to that region until the graphics state is
/// restored. CSS border side painting uses this to isolate one side of a
/// rounded border ring when side colors or styles differ:
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-shaping> and ISO
/// 32000-1:2008, 8.5.4 "Clipping Path Operators".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPathClip {
    pub commands: Vec<RenderedPathCommand>,
    pub fill_rule: RenderedPathFillRule,
    pub additional_clips: Vec<RenderedPathClipPath>,
}

impl RenderedPathClip {
    pub(crate) fn new(
        commands: Vec<RenderedPathCommand>,
        fill_rule: RenderedPathFillRule,
        additional_clips: Vec<RenderedPathClipPath>,
    ) -> Self {
        Self {
            commands,
            fill_rule,
            additional_clips,
        }
    }

    /// Translate every contour in a retained PDF clipping scope.
    pub(crate) fn translated(mut self, offset: PaintTranslation) -> Self {
        for command in &mut self.commands {
            command.translate(offset);
        }
        for clip in &mut self.additional_clips {
            for command in &mut clip.commands {
                command.translate(offset);
            }
        }
        self
    }

    pub(crate) fn transform(&mut self, transform: PaintTransform) {
        for command in &mut self.commands {
            command.transform(transform);
        }
        for clip in &mut self.additional_clips {
            for command in &mut clip.commands {
                command.transform(transform);
            }
        }
    }
}

/// One additional PDF clipping path intersected with an active clip scope.
///
/// CSS rounded patterned borders need the intersection of a side transition
/// region and the rounded border ring. PDF models this by applying multiple
/// clipping paths in sequence within one graphics state:
/// ISO 32000-1:2008, 8.5.4 "Clipping Path Operators".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPathClipPath {
    pub commands: Vec<RenderedPathCommand>,
    pub fill_rule: RenderedPathFillRule,
}

impl RenderedPathClipPath {
    pub(crate) fn new(commands: Vec<RenderedPathCommand>, fill_rule: RenderedPathFillRule) -> Self {
        Self {
            commands,
            fill_rule,
        }
    }
}

/// A PDF-compatible path construction command.
///
/// The variants map directly to PDF `m`, `l`, `c`, and `h` operators from ISO
/// 32000-1:2008, 8.5.2 "Path Construction Operators".
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderedPathCommand {
    MoveTo(PaintPoint),
    LineTo(PaintPoint),
    CurveTo {
        control_1: PaintPoint,
        control_2: PaintPoint,
        end: PaintPoint,
    },
    Close,
}

impl RenderedPathCommand {
    pub(crate) fn move_to(point: PaintPoint) -> Self {
        Self::MoveTo(point)
    }

    pub(crate) fn line_to(point: PaintPoint) -> Self {
        Self::LineTo(point)
    }

    pub(crate) fn curve_to(control_1: PaintPoint, control_2: PaintPoint, end: PaintPoint) -> Self {
        Self::CurveTo {
            control_1,
            control_2,
            end,
        }
    }

    pub(crate) fn typed_points(self) -> RenderedPathCommandPoints {
        match self {
            Self::MoveTo(point) => RenderedPathCommandPoints::MoveTo(point),
            Self::LineTo(point) => RenderedPathCommandPoints::LineTo(point),
            Self::CurveTo {
                control_1,
                control_2,
                end,
            } => RenderedPathCommandPoints::CurveTo {
                control_1,
                control_2,
                end,
            },
            Self::Close => RenderedPathCommandPoints::Close,
        }
    }

    pub(in crate::document) fn translate(&mut self, offset: PaintTranslation) {
        match self {
            Self::MoveTo(point) | Self::LineTo(point) => {
                *point = offset.transform_point(*point);
            }
            Self::CurveTo {
                control_1,
                control_2,
                end,
            } => {
                *control_1 = offset.transform_point(*control_1);
                *control_2 = offset.transform_point(*control_2);
                *end = offset.transform_point(*end);
            }
            Self::Close => {}
        }
    }

    pub(crate) fn transform(&mut self, transform: PaintTransform) {
        match self {
            Self::MoveTo(point) | Self::LineTo(point) => {
                *point = transform.apply_point(*point);
            }
            Self::CurveTo {
                control_1,
                control_2,
                end,
            } => {
                *control_1 = transform.apply_point(*control_1);
                *control_2 = transform.apply_point(*control_2);
                *end = transform.apply_point(*end);
            }
            Self::Close => {}
        }
    }
}

/// Typed paint-space points for a rendered path command.
///
/// The public command enum keeps scalar fields for compatibility, while this
/// view gives the PDF backend explicit paint-space coordinates before the
/// final conversion to PDF user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RenderedPathCommandPoints {
    MoveTo(PaintPoint),
    LineTo(PaintPoint),
    CurveTo {
        control_1: PaintPoint,
        control_2: PaintPoint,
        end: PaintPoint,
    },
    Close,
}

/// Fill rule for a PDF path.
///
/// PDF defines nonzero winding (`f`) and even-odd (`f*`) fill operators; CSS
/// border rings use even-odd filling so the padding-edge subpath cuts out the
/// content area without depending on subpath winding direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RenderedPathFillRule {
    #[default]
    NonZero,
    EvenOdd,
}

pub(crate) fn paint_rect_path_commands(rect: PaintRect) -> Vec<RenderedPathCommand> {
    vec![
        RenderedPathCommand::move_to(rect.origin),
        RenderedPathCommand::line_to(PaintPoint::new(rect.max_x(), rect.min_y())),
        RenderedPathCommand::line_to(PaintPoint::new(rect.max_x(), rect.max_y())),
        RenderedPathCommand::line_to(PaintPoint::new(rect.min_x(), rect.max_y())),
        RenderedPathCommand::Close,
    ]
}

pub(in crate::document) fn path_bounds(path: &RenderedPath) -> Option<PaintClip> {
    path.paint_bounds().map(|bounds| {
        let outset = path.stroke_width.points().max(0.0) / 2.0;
        PaintClip::new(
            bounds.origin.x - outset,
            bounds.origin.y - outset,
            bounds.size.width + outset * 2.0,
            bounds.size.height + outset * 2.0,
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::CssColor;

    use super::{
        RenderedPath, RenderedPathCommand, RenderedPathCommandPoints, paint_rect_path_commands,
        path_bounds,
    };
    use crate::document::paint::geometry::{
        PaintClip, PaintPoint, PaintRect, PaintSize, PaintStrokeWidth, PaintTransform,
    };

    #[test]
    fn paint_rect_path_commands_preserve_a_nonzero_origin() {
        let rect = PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(30.0, 40.0));

        assert_eq!(
            paint_rect_path_commands(rect),
            vec![
                RenderedPathCommand::move_to(PaintPoint::new(10.0, 20.0)),
                RenderedPathCommand::line_to(PaintPoint::new(40.0, 20.0)),
                RenderedPathCommand::line_to(PaintPoint::new(40.0, 60.0)),
                RenderedPathCommand::line_to(PaintPoint::new(10.0, 60.0)),
                RenderedPathCommand::Close,
            ]
        );
    }

    #[test]
    fn path_commands_expose_typed_paint_points() {
        assert_eq!(
            RenderedPathCommand::move_to(PaintPoint::new(1.0, 2.0)).typed_points(),
            RenderedPathCommandPoints::MoveTo(PaintPoint::new(1.0, 2.0))
        );
        assert_eq!(
            RenderedPathCommand::curve_to(
                PaintPoint::new(1.0, 2.0),
                PaintPoint::new(3.0, 4.0),
                PaintPoint::new(5.0, 6.0),
            )
            .typed_points(),
            RenderedPathCommandPoints::CurveTo {
                control_1: PaintPoint::new(1.0, 2.0),
                control_2: PaintPoint::new(3.0, 4.0),
                end: PaintPoint::new(5.0, 6.0),
            }
        );
    }

    #[test]
    fn path_bounds_use_transformed_geometry_and_stroke_outset() {
        let path = RenderedPath::new(
            paint_rect_path_commands(PaintRect::new(
                PaintPoint::new(0.0, 0.0),
                PaintSize::new(10.0, 20.0),
            )),
            Some(CssColor::BLACK),
            super::RenderedPathFillRule::NonZero,
            Some(CssColor::BLACK),
            PaintStrokeWidth::new(4.0),
            None,
        )
        .with_transform(PaintTransform::new(1.0, 0.0, 0.0, 1.0, 30.0, 40.0));

        assert_eq!(
            path_bounds(&path),
            Some(PaintClip::new(28.0, 38.0, 14.0, 24.0))
        );
    }
}
