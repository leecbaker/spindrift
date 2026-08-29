use super::*;

/// Used paint geometry for one CSS Borders 4 circular `border-shape`.
///
/// A `border-shape` is visual-only, so this is constructed at the paint
/// boundary from the relevant border/padding/content geometry rather than
/// leaking shape coordinates into layout sizing.
#[derive(Debug, Clone, Copy)]
pub(super) struct ResolvedBorderShapeCircle {
    center: PaintPoint,
    radius: f32,
}

/// Used paint geometry for one CSS Borders 4 elliptical `border-shape`.
#[derive(Debug, Clone, Copy)]
pub(super) struct ResolvedBorderShapeEllipse {
    center: PaintPoint,
    horizontal_radius: f32,
    vertical_radius: f32,
}

/// Used paint geometry for a closed line-only CSS `shape()` border path.
#[derive(Debug, Clone)]
pub(super) struct ResolvedBorderShapePath {
    vertices: Vec<PaintPoint>,
}

/// A resolved uniform rounded `inset()` contour.
#[derive(Debug, Clone, Copy)]
pub(super) struct ResolvedBorderShapeRoundedRect {
    rect: PaintRect,
    radius_x: f32,
    radius_y: f32,
}

/// One resolved single-shape contour usable by background, border, and
/// overflow paint without exposing basic-shape geometry to layout.
#[derive(Debug, Clone)]
pub(super) enum ResolvedBorderShape {
    /// A valid basic shape that has collapsed to an empty used contour.
    ///
    /// This is not the same as no `border-shape`: backgrounds and descendant
    /// overflow must be suppressed rather than reverting to the box rectangle.
    Empty,
    Circle(ResolvedBorderShapeCircle),
    Ellipse(ResolvedBorderShapeEllipse),
    Path(ResolvedBorderShapePath),
    RoundedRect(ResolvedBorderShapeRoundedRect),
}

/// The descendant overflow contour established by a single `border-shape`.
///
/// A collapsed shape still clips; it simply admits no descendant paint.
#[derive(Debug, Clone)]
pub(in crate::layout) enum BorderShapeOverflowClip {
    /// Exact contour for atomic content whose paint primitive can carry a
    /// path clip directly, without the fixed-size retained polygon limit.
    Path(RenderedPathClip),
    Empty,
}

/// The annular fill region bounded by two CSS Borders 4 basic-shape paths.
#[derive(Debug, Clone)]
pub(super) struct ResolvedBorderShapePair {
    outer: ResolvedBorderShape,
    pub(super) inner: ResolvedBorderShape,
}

impl ResolvedBorderShapePair {
    pub(super) fn commands(&self) -> Vec<RenderedPathCommand> {
        let mut commands = self.outer.commands();
        commands.extend(self.inner.commands());
        commands
    }
}

/// Builds the outline paint bands for a circular or elliptical `border-shape`.
///
/// An outline follows the outermost contour of a multi-path `border-shape`;
/// it must not reuse the element's filled annulus.  In particular, a two-path
/// shape has an outline outside the outer path only.  Circles and ellipses
/// have a well-defined parallel contour, represented here by growing both
/// radii by the used outline offset and width:
/// <https://drafts.csswg.org/css-borders-4/#border-shape> and
/// <https://drafts.csswg.org/css-ui/#outline-props>.
pub(in crate::layout) fn border_shape_outline_paths(
    border_rect: PaintRect,
    style: &ComputedStyle,
) -> Option<Vec<RenderedPath>> {
    let outer = resolved_outer_border_shape(border_rect, style, used_border_widths(style))?;
    let outline_offset = style.outline_offset.length_points();
    let outline_width = style.outline_width;
    let color = style.outline_color.resolve(style.color);
    let paint_band = |outer: ResolvedBorderShape, inner: Option<ResolvedBorderShape>| {
        let has_inner = inner.is_some();
        let mut commands = outer.commands();
        if let Some(inner) = &inner {
            commands.extend(inner.commands());
        }
        RenderedPath::new(
            commands,
            Some(color),
            if has_inner {
                RenderedPathFillRule::EvenOdd
            } else {
                RenderedPathFillRule::NonZero
            },
            None,
            PaintStrokeWidth::ZERO,
            None,
        )
    };

    let paths = match style.outline_style {
        css::BorderStyle::Solid => {
            let outer = outer.outset(outline_offset + outline_width)?;
            let inner = outer.outset(-outline_width);
            vec![paint_band(outer, inner)]
        }
        css::BorderStyle::Double => {
            // CSS `double` divides its used width into three equal bands:
            // the outer and inner bands are painted and the middle band is
            // transparent.  Construct those bands directly rather than
            // filling the element's two-path border-shape annulus.
            // <https://www.w3.org/TR/css-backgrounds-3/#border-style>
            let line_width = outline_width / 3.0;
            let outer_edge = outer.outset(outline_offset + outline_width)?;
            let outer_inner = outer_edge.outset(-line_width);
            let inner_outer = outer.outset(outline_offset + line_width)?;
            let inner_inner = inner_outer.outset(-line_width);
            vec![
                paint_band(outer_edge, outer_inner),
                paint_band(inner_outer, inner_inner),
            ]
        }
        _ => return None,
    };
    Some(paths)
}

impl ResolvedBorderShape {
    pub(super) fn commands(&self) -> Vec<RenderedPathCommand> {
        match self {
            Self::Empty => Vec::new(),
            Self::Circle(circle) => circle.commands(),
            Self::Ellipse(ellipse) => ellipse.commands(),
            Self::Path(path) => path.commands(),
            Self::RoundedRect(rounded) => rounded.commands(),
        }
    }

    fn inner_overflow_path_clip(&self, inset: f32) -> Option<BorderShapeOverflowClip> {
        let inner = match self {
            Self::Empty => return Some(BorderShapeOverflowClip::Empty),
            Self::Circle(circle) => {
                let radius = circle.radius - inset;
                (radius > 0.0).then_some(Self::Circle(ResolvedBorderShapeCircle {
                    center: circle.center,
                    radius,
                }))
            }
            Self::Ellipse(ellipse) => {
                let horizontal_radius = ellipse.horizontal_radius - inset;
                let vertical_radius = ellipse.vertical_radius - inset;
                (horizontal_radius > 0.0 && vertical_radius > 0.0).then_some(Self::Ellipse(
                    ResolvedBorderShapeEllipse {
                        center: ellipse.center,
                        horizontal_radius,
                        vertical_radius,
                    },
                ))
            }
            Self::Path(path) => {
                if inset.abs() <= f32::EPSILON {
                    Some(Self::Path(path.clone()))
                } else {
                    path.outset(-inset).map(Self::Path)
                }
            }
            Self::RoundedRect(rounded) => Some(Self::RoundedRect(*rounded)),
        };
        let commands = inner?.commands();
        (!commands.is_empty())
            .then(|| {
                BorderShapeOverflowClip::Path(RenderedPathClip::new(
                    commands,
                    RenderedPathFillRule::NonZero,
                    Vec::new(),
                ))
            })
            .or(Some(BorderShapeOverflowClip::Empty))
    }

    pub(super) fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Returns the parallel circular or elliptical contour at `distance` from
    /// this contour.  Positive values expand away from the center and negative
    /// values contract toward it.
    pub(super) fn outset(&self, distance: f32) -> Option<Self> {
        match self {
            Self::Empty => None,
            Self::Circle(circle) => {
                let radius = circle.radius + distance;
                (radius > 0.0).then_some(Self::Circle(ResolvedBorderShapeCircle {
                    center: circle.center,
                    radius,
                }))
            }
            Self::Ellipse(ellipse) => {
                let horizontal_radius = ellipse.horizontal_radius + distance;
                let vertical_radius = ellipse.vertical_radius + distance;
                (horizontal_radius > 0.0 && vertical_radius > 0.0).then_some(Self::Ellipse(
                    ResolvedBorderShapeEllipse {
                        center: ellipse.center,
                        horizontal_radius,
                        vertical_radius,
                    },
                ))
            }
            Self::Path(path) => path.outset(distance).map(Self::Path),
            Self::RoundedRect(_) => None,
        }
    }

    /// Translates a resolved paint contour without re-resolving its CSS
    /// percentage geometry against a different box.
    pub(super) fn translated(&self, displacement: PaintDisplacement) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Circle(circle) => Self::Circle(ResolvedBorderShapeCircle {
                center: circle.center + displacement,
                radius: circle.radius,
            }),
            Self::Ellipse(ellipse) => Self::Ellipse(ResolvedBorderShapeEllipse {
                center: ellipse.center + displacement,
                horizontal_radius: ellipse.horizontal_radius,
                vertical_radius: ellipse.vertical_radius,
            }),
            Self::Path(path) => Self::Path(ResolvedBorderShapePath {
                vertices: path
                    .vertices
                    .iter()
                    .copied()
                    .map(|vertex| vertex + displacement)
                    .collect(),
            }),
            Self::RoundedRect(rounded) => Self::RoundedRect(ResolvedBorderShapeRoundedRect {
                rect: PaintRect::new(rounded.rect.origin + displacement, rounded.rect.size),
                radius_x: rounded.radius_x,
                radius_y: rounded.radius_y,
            }),
        }
    }
}

impl ResolvedBorderShapePath {
    fn commands(&self) -> Vec<RenderedPathCommand> {
        let Some((&first, rest)) = self.vertices.split_first() else {
            return Vec::new();
        };
        std::iter::once(RenderedPathCommand::move_to(first))
            .chain(rest.iter().copied().map(RenderedPathCommand::line_to))
            .chain(std::iter::once(RenderedPathCommand::Close))
            .collect()
    }

    /// Offset a closed polygonal contour by a physical paint-space distance.
    ///
    /// Each edge is shifted along its outward unit normal and adjacent shifted
    /// lines are intersected at the vertex. This retains the CSS basic shape
    /// as geometry rather than approximating its shadows, outlines, or clips
    /// with the element rectangle. Degenerate edges and self-intersections are
    /// rejected because they do not have a single well-defined miter contour.
    fn outset(&self, distance: f32) -> Option<Self> {
        const EPSILON: f32 = 1e-5;
        if self.vertices.len() < 3 || !distance.is_finite() {
            return None;
        }
        let signed_area_twice = self
            .vertices
            .iter()
            .zip(self.vertices.iter().cycle().skip(1))
            .take(self.vertices.len())
            .map(|(start, end)| start.x * end.y - end.x * start.y)
            .sum::<f32>();
        if signed_area_twice.abs() < EPSILON {
            return None;
        }
        let outward_normal = |start: PaintPoint, end: PaintPoint| {
            let dx = end.x - start.x;
            let dy = end.y - start.y;
            let length = (dx * dx + dy * dy).sqrt();
            (length >= EPSILON).then(|| {
                let (x, y) = if signed_area_twice > 0.0 {
                    // Counter-clockwise contours keep their interior on the
                    // left of each edge, so the right normal is outward.
                    (dy / length, -dx / length)
                } else {
                    (-dy / length, dx / length)
                };
                PaintDisplacement::new(x, y)
            })
        };
        let mut vertices = Vec::with_capacity(self.vertices.len());
        for index in 0..self.vertices.len() {
            let previous = self.vertices[(index + self.vertices.len() - 1) % self.vertices.len()];
            let current = self.vertices[index];
            let next = self.vertices[(index + 1) % self.vertices.len()];
            let previous_normal = outward_normal(previous, current)?;
            let next_normal = outward_normal(current, next)?;
            let previous_start = previous + previous_normal * distance;
            let previous_end = current + previous_normal * distance;
            let next_start = current + next_normal * distance;
            let next_end = next + next_normal * distance;
            let previous_direction = previous_end - previous_start;
            let next_direction = next_end - next_start;
            let denominator =
                previous_direction.x * next_direction.y - previous_direction.y * next_direction.x;
            if denominator.abs() < EPSILON {
                return None;
            }
            let start_delta = next_start - previous_start;
            let along_previous =
                (start_delta.x * next_direction.y - start_delta.y * next_direction.x) / denominator;
            let vertex = previous_start + previous_direction * along_previous;
            if !vertex.x.is_finite() || !vertex.y.is_finite() {
                return None;
            }
            vertices.push(vertex);
        }
        Some(Self { vertices })
    }
}

impl ResolvedBorderShapeRoundedRect {
    fn commands(self) -> Vec<RenderedPathCommand> {
        rounded_rect_path_commands(
            self.rect,
            RenderedRoundedRectRadii {
                top_left: RenderedCornerRadius::new(self.radius_x, self.radius_y),
                top_right: RenderedCornerRadius::new(self.radius_x, self.radius_y),
                bottom_right: RenderedCornerRadius::new(self.radius_x, self.radius_y),
                bottom_left: RenderedCornerRadius::new(self.radius_x, self.radius_y),
            },
        )
    }
}

impl ResolvedBorderShapeCircle {
    fn commands(self) -> Vec<RenderedPathCommand> {
        // The canonical cubic-circle approximation used by the SVG adapter.
        // Keeping this shared coefficient makes CSS `circle()` and an
        // equivalent SVG `<circle>` rasterize on the same contour.
        const KAPPA: f32 = 0.552_284_8;
        let radius = self.radius.max(0.0);
        let x = self.center.x;
        let y = self.center.y;
        vec![
            RenderedPathCommand::move_to(PaintPoint::new(x + radius, y)),
            RenderedPathCommand::curve_to(
                PaintPoint::new(x + radius, y + radius * KAPPA),
                PaintPoint::new(x + radius * KAPPA, y + radius),
                PaintPoint::new(x, y + radius),
            ),
            RenderedPathCommand::curve_to(
                PaintPoint::new(x - radius * KAPPA, y + radius),
                PaintPoint::new(x - radius, y + radius * KAPPA),
                PaintPoint::new(x - radius, y),
            ),
            RenderedPathCommand::curve_to(
                PaintPoint::new(x - radius, y - radius * KAPPA),
                PaintPoint::new(x - radius * KAPPA, y - radius),
                PaintPoint::new(x, y - radius),
            ),
            RenderedPathCommand::curve_to(
                PaintPoint::new(x + radius * KAPPA, y - radius),
                PaintPoint::new(x + radius, y - radius * KAPPA),
                PaintPoint::new(x + radius, y),
            ),
            RenderedPathCommand::Close,
        ]
    }
}

impl ResolvedBorderShapeEllipse {
    fn commands(self) -> Vec<RenderedPathCommand> {
        const KAPPA: f32 = 0.552_284_8;
        let horizontal_radius = self.horizontal_radius.max(0.0);
        let vertical_radius = self.vertical_radius.max(0.0);
        let x = self.center.x;
        let y = self.center.y;
        vec![
            RenderedPathCommand::move_to(PaintPoint::new(x + horizontal_radius, y)),
            RenderedPathCommand::curve_to(
                PaintPoint::new(x + horizontal_radius, y + vertical_radius * KAPPA),
                PaintPoint::new(x + horizontal_radius * KAPPA, y + vertical_radius),
                PaintPoint::new(x, y + vertical_radius),
            ),
            RenderedPathCommand::curve_to(
                PaintPoint::new(x - horizontal_radius * KAPPA, y + vertical_radius),
                PaintPoint::new(x - horizontal_radius, y + vertical_radius * KAPPA),
                PaintPoint::new(x - horizontal_radius, y),
            ),
            RenderedPathCommand::curve_to(
                PaintPoint::new(x - horizontal_radius, y - vertical_radius * KAPPA),
                PaintPoint::new(x - horizontal_radius * KAPPA, y - vertical_radius),
                PaintPoint::new(x, y - vertical_radius),
            ),
            RenderedPathCommand::curve_to(
                PaintPoint::new(x + horizontal_radius * KAPPA, y - vertical_radius),
                PaintPoint::new(x + horizontal_radius, y - vertical_radius * KAPPA),
                PaintPoint::new(x + horizontal_radius, y),
            ),
            RenderedPathCommand::Close,
        ]
    }
}

/// Resolve the exact inner shape contour for atomic descendant content.
///
/// Image and inline-SVG primitives can carry a full PDF path clip, avoiding
/// the chord error of the compact polygon retained by normal-flow effects.
/// <https://drafts.csswg.org/css-borders-4/#border-shape>
#[allow(dead_code)]
pub(in crate::layout) fn single_border_shape_overflow_path_clip(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Option<BorderShapeOverflowClip> {
    let shape = resolved_single_border_shape(rect, style, border_insets)?;
    let stroke_width = relevant_border_shape_side(style).map_or(0.0, |side| side.used_width.get());
    shape.inner_overflow_path_clip(stroke_width / 2.0)
}

pub(super) fn resolved_single_border_shape(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Option<ResolvedBorderShape> {
    (!matches!(style.border_shape, css::BorderShape::Pair { .. }))
        .then(|| resolved_border_shape_value(rect, style, border_insets, &style.border_shape))
        .flatten()
}

/// Resolves the outermost path of a supported `border-shape`.
///
/// Multi-path shapes deliberately resolve to their first path here: outlines
/// trace the outer edge of the shape, whereas background and border painting
/// use the complete pair to form an annulus.
pub(super) fn resolved_outer_border_shape(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Option<ResolvedBorderShape> {
    let shape = match &style.border_shape {
        css::BorderShape::Pair { outer, .. } => outer,
        shape => shape,
    };
    resolved_border_shape_value(rect, style, border_insets, shape)
}

/// Resolve the contour that bounds a `border-shape` background and therefore
/// its inset shadow. Paired shapes expose the inner contour to the element's
/// background, while a single shape uses its only contour.
pub(super) fn resolved_inset_border_shape(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Option<ResolvedBorderShape> {
    if let Some(pair) = resolved_border_shape_pair(rect, style, border_insets) {
        return Some(pair.inner);
    }
    // A single border-shape path is centered on the relevant border stroke;
    // backgrounds and inset shadows are bounded by its inner visible edge.
    // Preserve that distinction for arbitrary polygonal paths as well as the
    // analytically offset circle and ellipse contours.
    let stroke_width = relevant_border_shape_side(style).map_or(0.0, |side| side.used_width.get());
    resolved_outer_border_shape(rect, style, border_insets)?.outset(-stroke_width / 2.0)
}

/// Resolve the inner visible contour of a `border-shape` for any contained
/// paint, including descendant overflow and replaced content.
///
/// Two-path shapes supply this contour directly. For one path, CSS Borders 4
/// paints the stroke centered on that path, so the content side is offset by
/// half the relevant used border width.
pub(in crate::layout) fn border_shape_inner_content_clip(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Option<BorderShapeOverflowClip> {
    let shape = resolved_inset_border_shape(rect, style, border_insets)?;
    let commands = shape.commands();
    (!commands.is_empty())
        .then(|| {
            BorderShapeOverflowClip::Path(RenderedPathClip::new(
                commands,
                RenderedPathFillRule::NonZero,
                Vec::new(),
            ))
        })
        .or(Some(BorderShapeOverflowClip::Empty))
}

pub(super) fn resolved_border_shape_pair(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Option<ResolvedBorderShapePair> {
    let css::BorderShape::Pair { outer, inner } = &style.border_shape else {
        return None;
    };
    Some(ResolvedBorderShapePair {
        outer: resolved_border_shape_value(rect, style, border_insets, outer)?,
        inner: resolved_border_shape_value(rect, style, border_insets, inner)?,
    })
}

/// Resolve one non-pair basic shape at the paint-time percentage boundary.
fn resolved_border_shape_value(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    shape: &css::BorderShape,
) -> Option<ResolvedBorderShape> {
    match shape {
        css::BorderShape::Circle(circle) => {
            resolved_border_shape_circle(rect, style, border_insets, circle).map(|circle| {
                if circle.radius <= 0.0 {
                    ResolvedBorderShape::Empty
                } else {
                    ResolvedBorderShape::Circle(circle)
                }
            })
        }
        css::BorderShape::Ellipse(ellipse) => {
            resolved_border_shape_ellipse(rect, style, border_insets, ellipse).map(|ellipse| {
                if ellipse.horizontal_radius <= 0.0 || ellipse.vertical_radius <= 0.0 {
                    ResolvedBorderShape::Empty
                } else {
                    ResolvedBorderShape::Ellipse(ellipse)
                }
            })
        }
        css::BorderShape::Path(path) => {
            resolved_border_shape_path(rect, style, border_insets, path)
                .map(ResolvedBorderShape::Path)
        }
        css::BorderShape::Inset(inset) => {
            resolved_border_shape_inset_path(rect, style, border_insets, inset)
        }
        css::BorderShape::Polygon(polygon) => {
            resolved_border_shape_polygon(rect, style, border_insets, polygon)
                .map(ResolvedBorderShape::Path)
        }
        css::BorderShape::None | css::BorderShape::Pair { .. } => None,
    }
}

fn resolved_border_shape_circle(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    circle: &css::BorderShapeCircle,
) -> Option<ResolvedBorderShapeCircle> {
    let area = border_shape_geometry_box_rect(rect, style, border_insets, circle.geometry_box);
    if area.size.width <= 0.0 || area.size.height <= 0.0 {
        return None;
    }
    let resolve = |value: &css::ComputedLengthPercentage, basis: f32| {
        value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
            .map(layout_points)
            .unwrap_or_else(|| value.length_points())
    };
    let x = area.min_x() + resolve(&circle.position.x, area.size.width);
    // CSS shape coordinates are top-left based; paint coordinates are
    // bottom-left based.
    let y = area.max_y() - resolve(&circle.position.y, area.size.height);
    let distances = [
        (x - area.min_x()).max(0.0),
        (area.max_x() - x).max(0.0),
        (y - area.min_y()).max(0.0),
        (area.max_y() - y).max(0.0),
    ];
    let radius = match &circle.radius {
        css::BorderShapeCircleRadius::LengthPercentage(value) => {
            let diagonal = ((area.size.width.powi(2) + area.size.height.powi(2)) / 2.0).sqrt();
            resolve(value, diagonal)
        }
        css::BorderShapeCircleRadius::ClosestSide => {
            distances.into_iter().fold(f32::INFINITY, f32::min)
        }
        css::BorderShapeCircleRadius::FarthestSide => distances.into_iter().fold(0.0, f32::max),
        css::BorderShapeCircleRadius::ClosestCorner => [
            (x - area.min_x()).hypot(y - area.min_y()),
            (x - area.min_x()).hypot(area.max_y() - y),
            (area.max_x() - x).hypot(y - area.min_y()),
            (area.max_x() - x).hypot(area.max_y() - y),
        ]
        .into_iter()
        .fold(f32::INFINITY, f32::min),
        css::BorderShapeCircleRadius::FarthestCorner => [
            (x - area.min_x()).hypot(y - area.min_y()),
            (x - area.min_x()).hypot(area.max_y() - y),
            (area.max_x() - x).hypot(y - area.min_y()),
            (area.max_x() - x).hypot(area.max_y() - y),
        ]
        .into_iter()
        .fold(0.0, f32::max),
    };
    Some(ResolvedBorderShapeCircle {
        center: PaintPoint::new(x, y),
        radius: radius.max(0.0),
    })
}

fn resolved_border_shape_ellipse(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    ellipse: &css::BorderShapeEllipse,
) -> Option<ResolvedBorderShapeEllipse> {
    let area = border_shape_geometry_box_rect(rect, style, border_insets, ellipse.geometry_box);
    if area.size.width <= 0.0 || area.size.height <= 0.0 {
        return None;
    }
    let resolve = |value: &css::ComputedLengthPercentage, basis: f32| {
        value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
            .map(layout_points)
            .unwrap_or_else(|| value.length_points())
    };
    let x = area.min_x() + resolve(&ellipse.position.x, area.size.width);
    let y = area.max_y() - resolve(&ellipse.position.y, area.size.height);
    let corner_distances = [
        (x - area.min_x()).hypot(y - area.min_y()),
        (x - area.min_x()).hypot(area.max_y() - y),
        (area.max_x() - x).hypot(y - area.min_y()),
        (area.max_x() - x).hypot(area.max_y() - y),
    ];
    let horizontal_radius = match &ellipse.horizontal_radius {
        css::BorderShapeEllipseRadius::LengthPercentage(value) => resolve(value, area.size.width),
        css::BorderShapeEllipseRadius::ClosestSide => {
            (x - area.min_x()).min(area.max_x() - x).max(0.0)
        }
        css::BorderShapeEllipseRadius::FarthestSide => {
            (x - area.min_x()).max(area.max_x() - x).max(0.0)
        }
        css::BorderShapeEllipseRadius::ClosestCorner => {
            corner_distances.into_iter().fold(f32::INFINITY, f32::min)
        }
        css::BorderShapeEllipseRadius::FarthestCorner => {
            corner_distances.into_iter().fold(0.0, f32::max)
        }
    };
    let vertical_radius = match &ellipse.vertical_radius {
        css::BorderShapeEllipseRadius::LengthPercentage(value) => resolve(value, area.size.height),
        css::BorderShapeEllipseRadius::ClosestSide => {
            (y - area.min_y()).min(area.max_y() - y).max(0.0)
        }
        css::BorderShapeEllipseRadius::FarthestSide => {
            (y - area.min_y()).max(area.max_y() - y).max(0.0)
        }
        css::BorderShapeEllipseRadius::ClosestCorner => {
            corner_distances.into_iter().fold(f32::INFINITY, f32::min)
        }
        css::BorderShapeEllipseRadius::FarthestCorner => {
            corner_distances.into_iter().fold(0.0, f32::max)
        }
    };
    Some(ResolvedBorderShapeEllipse {
        center: PaintPoint::new(x, y),
        horizontal_radius: horizontal_radius.max(0.0),
        vertical_radius: vertical_radius.max(0.0),
    })
}

fn resolved_border_shape_path(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    path: &css::BorderShapePath,
) -> Option<ResolvedBorderShapePath> {
    let area = border_shape_geometry_box_rect(rect, style, border_insets, path.geometry_box);
    if area.size.width <= 0.0 || area.size.height <= 0.0 || path.vertices.len() < 3 {
        return None;
    }
    let resolve = |value: &css::ComputedLengthPercentage, basis: f32| {
        value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
            .map(layout_points)
            .unwrap_or_else(|| value.length_points())
    };
    Some(ResolvedBorderShapePath {
        vertices: path
            .vertices
            .iter()
            .map(|vertex| {
                PaintPoint::new(
                    area.min_x() + resolve(&vertex.x, area.size.width),
                    area.max_y() - resolve(&vertex.y, area.size.height),
                )
            })
            .collect(),
    })
}

fn resolved_border_shape_polygon(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    polygon: &css::BorderShapePolygon,
) -> Option<ResolvedBorderShapePath> {
    let area = border_shape_geometry_box_rect(rect, style, border_insets, polygon.geometry_box);
    if area.size.width <= 0.0 || area.size.height <= 0.0 || polygon.vertices.len() < 3 {
        return None;
    }
    let resolve = |value: &css::ComputedLengthPercentage, basis: f32| {
        value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
            .map(layout_points)
            .unwrap_or_else(|| value.length_points())
    };
    Some(ResolvedBorderShapePath {
        vertices: polygon
            .vertices
            .iter()
            .map(|vertex| {
                PaintPoint::new(
                    area.min_x() + resolve(&vertex.x, area.size.width),
                    area.max_y() - resolve(&vertex.y, area.size.height),
                )
            })
            .collect(),
    })
}

fn resolved_border_shape_inset_path(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    inset: &css::BorderShapeInset,
) -> Option<ResolvedBorderShape> {
    let area = border_shape_geometry_box_rect(rect, style, border_insets, inset.geometry_box);
    if area.size.width <= 0.0 || area.size.height <= 0.0 {
        return None;
    }
    let resolve = |value: &css::ComputedLengthPercentage, basis: f32| {
        value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
            .map(layout_points)
            .unwrap_or_else(|| value.length_points())
    };
    let left = area.min_x() + resolve(&inset.left, area.size.width);
    let right = area.max_x() - resolve(&inset.right, area.size.width);
    // CSS basic-shape coordinates use a top-left origin, whereas paint uses a
    // bottom-left origin.
    let top = area.max_y() - resolve(&inset.top, area.size.height);
    let bottom = area.min_y() + resolve(&inset.bottom, area.size.height);
    if left >= right || bottom >= top {
        return Some(ResolvedBorderShape::Empty);
    }
    let inset_rect = PaintRect::new(
        PaintPoint::new(left, bottom),
        PaintSize::new(right - left, top - bottom),
    );
    if let Some(radius) = &inset.corner_radius {
        let radius_x = resolve(radius, inset_rect.size.width)
            .max(0.0)
            .min(inset_rect.size.width / 2.0);
        let radius_y = resolve(radius, inset_rect.size.height)
            .max(0.0)
            .min(inset_rect.size.height / 2.0);
        return Some(ResolvedBorderShape::RoundedRect(
            ResolvedBorderShapeRoundedRect {
                rect: inset_rect,
                radius_x,
                radius_y,
            },
        ));
    }
    Some(ResolvedBorderShape::Path(ResolvedBorderShapePath {
        vertices: vec![
            PaintPoint::new(left, top),
            PaintPoint::new(right, top),
            PaintPoint::new(right, bottom),
            PaintPoint::new(left, bottom),
        ],
    }))
}

fn border_shape_geometry_box_rect(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    geometry_box: css::BorderShapeGeometryBox,
) -> PaintRect {
    match geometry_box {
        css::BorderShapeGeometryBox::Border => rect,
        css::BorderShapeGeometryBox::Padding => inset_paint_rect(rect, border_insets),
        css::BorderShapeGeometryBox::Content => inset_paint_rect(
            rect,
            css::Edges {
                top: border_insets.top + style.padding.top,
                right: border_insets.right + style.padding.right,
                bottom: border_insets.bottom + style.padding.bottom,
                left: border_insets.left + style.padding.left,
            },
        ),
        css::BorderShapeGeometryBox::Margin => inset_paint_rect(
            rect,
            css::Edges {
                top: -style.margin.top,
                right: -style.margin.right,
                bottom: -style.margin.bottom,
                left: -style.margin.left,
            },
        ),
        css::BorderShapeGeometryBox::HalfBorder => inset_paint_rect(
            rect,
            css::Edges {
                top: border_insets.top / 2.0,
                right: border_insets.right / 2.0,
                bottom: border_insets.bottom / 2.0,
                left: border_insets.left / 2.0,
            },
        ),
    }
}

pub(super) fn paint_single_border_shape(
    paths: &mut Vec<RenderedPath>,
    shape: ResolvedBorderShape,
    style: &ComputedStyle,
) {
    let Some(side) = relevant_border_shape_side(style) else {
        return;
    };
    paths.push(RenderedPath::new(
        shape.commands(),
        None,
        RenderedPathFillRule::NonZero,
        Some(side.color),
        PaintStrokeWidth::new(side.used_width.get()),
        None,
    ));
}

fn relevant_border_shape_side(style: &ComputedStyle) -> Option<UsedBorderSide> {
    let borders = used_border(style);
    let logical_order = [
        block_start_side(style.writing_mode),
        inline_start_side(style.writing_mode, style.used_direction()),
        block_end_side(style.writing_mode),
        inline_end_side(style.writing_mode, style.used_direction()),
    ];
    logical_order.into_iter().find_map(|side| {
        let side = match side {
            PhysicalSide::Top => borders.top,
            PhysicalSide::Right => borders.right,
            PhysicalSide::Bottom => borders.bottom,
            PhysicalSide::Left => borders.left,
        };
        side.is_visible().then_some(side)
    })
}

/// Resolve the color source for a two-path `border-shape` fill.
///
/// The annular fill follows the first renderable logical border side, using
/// the same relevant-side selection as a single shape's stroke. A `none` or
/// `hidden` side therefore cannot contribute its initial (often black) color:
/// <https://drafts.csswg.org/css-borders-4/#relevant-side-for-border-shape>.
pub(super) fn relevant_border_shape_color(style: &ComputedStyle) -> Option<CssColor> {
    relevant_border_shape_side(style).map(|side| side.color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsed_border_shape_retains_an_empty_overflow_contour() {
        assert!(matches!(
            ResolvedBorderShape::Empty.inner_overflow_path_clip(0.0),
            Some(BorderShapeOverflowClip::Empty)
        ));
    }

    #[test]
    fn unstroked_polygon_uses_its_original_contour_for_overflow() {
        let shape = ResolvedBorderShape::Path(ResolvedBorderShapePath {
            vertices: vec![
                PaintPoint::new(50.0, 0.0),
                PaintPoint::new(100.0, 50.0),
                PaintPoint::new(50.0, 100.0),
                PaintPoint::new(0.0, 50.0),
            ],
        });
        let Some(BorderShapeOverflowClip::Path(clip)) = shape.inner_overflow_path_clip(0.0) else {
            panic!("an unstroked polygon should retain its exact vertices");
        };
        assert_eq!(clip.commands.len(), 5);
        assert_eq!(
            clip.commands[0],
            RenderedPathCommand::move_to(PaintPoint::new(50.0, 0.0))
        );
    }
}
