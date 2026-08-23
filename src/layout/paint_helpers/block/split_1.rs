use super::*;

/// Builds PDF paint primitives for a CSS block's background and border area.
///
/// CSS Backgrounds and Borders paints backgrounds and borders over the border
/// box; boxes with nonpositive used border-box area do not contribute visible
/// background paint:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background> and
/// <https://www.w3.org/TR/css-backgrounds-3/#borders>.
pub(crate) fn block_paint_ops(
    rect: PaintRect,
    style: &ComputedStyle,
) -> (
    Vec<RenderedRect>,
    Vec<RenderedRoundedRect>,
    Vec<RenderedPath>,
    Vec<RenderedStroke>,
) {
    block_paint_ops_with_border_insets(rect, style, used_border_widths(style), true)
}

/// Builds PDF paint primitives for a CSS block with caller-supplied border
/// insets.
///
/// Collapsed table cells use resolved grid half-widths for decoration
/// geometry, while their actual borders are painted later from the collapsed
/// border grid:
/// <https://drafts.csswg.org/css-tables-3/#in-collapsed-borders-mode>.
pub(crate) fn block_paint_ops_with_border_insets(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    paint_borders: bool,
) -> (
    Vec<RenderedRect>,
    Vec<RenderedRoundedRect>,
    Vec<RenderedPath>,
    Vec<RenderedStroke>,
) {
    block_paint_ops_with_phases(rect, style, border_insets, true, true, true, paint_borders)
}

/// Builds one or more ordered phases of a CSS box decoration.
///
/// CSS Backgrounds paints outer shadows, background color/images, inset
/// shadows, and borders as separate layers. Keeping the phases selectable lets
/// URL/SVG background images be inserted between the generated background
/// fills and the border without changing the established primitive types:
/// <https://www.w3.org/TR/css-backgrounds-3/#layering>.
#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn block_paint_ops_with_phases(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    paint_outer_shadows: bool,
    paint_backgrounds: bool,
    paint_inset_shadows: bool,
    paint_borders: bool,
) -> (
    Vec<RenderedRect>,
    Vec<RenderedRoundedRect>,
    Vec<RenderedPath>,
    Vec<RenderedStroke>,
) {
    let mut rects = Vec::new();
    let mut rounded_rects = Vec::new();
    let mut paths = Vec::new();
    let strokes = Vec::new();
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 {
        return (rects, rounded_rects, paths, strokes);
    }
    let geometry = BoxPaintGeometry {
        rect,
        border_insets,
    };
    let border_shape = resolved_single_border_shape(rect, style, border_insets);
    let border_shape_is_empty = border_shape
        .as_ref()
        .is_some_and(ResolvedBorderShape::is_empty);
    let border_shape_pair = resolved_border_shape_pair(rect, style, border_insets);
    if paint_outer_shadows {
        paint_box_shadows(&mut rects, &mut paths, geometry, style, false);
    }
    if paint_backgrounds
        && let Some(fill) = style.background.background_color.visible_color(style.color)
    {
        let color_clip = style.background_color_clip();
        // CSS paints the background over the selected clip box before it
        // paints the border.  Keep that complete paint area even if an opaque
        // border will cover part of it later: replacing it with a padding-box
        // fill loses the specified background layer geometry and changes the
        // observable fragment paint stack.
        // <https://www.w3.org/TR/css-backgrounds-3/#layering>
        let area = background_rect_area_for_box(rect, style, border_insets, color_clip);
        if area.size.width <= 0.0 || area.size.height <= 0.0 {
            // Nothing to paint for the solid color layer after clipping.
        } else if let Some(pair) = border_shape_pair.clone() {
            // With two paths, the element background fills the inner shape;
            // the annulus itself is the border-shape fill, whose default is
            // the relevant border side color:
            // <https://drafts.csswg.org/css-borders-4/#border-shape>.
            paths.push(RenderedPath::new(
                pair.inner.commands(),
                Some(fill),
                RenderedPathFillRule::NonZero,
                None,
                PaintStrokeWidth::ZERO,
                None,
            ));
            let ring_fill = if style.svg_fill.is_overridden() {
                style
                    .svg_fill
                    .paint
                    .resolve(style.color)
                    .unwrap_or(CssColor::TRANSPARENT)
            } else {
                relevant_border_shape_color(style).unwrap_or(fill)
            };
            if ring_fill.is_visible() {
                paths.push(RenderedPath::new(
                    pair.commands(),
                    Some(ring_fill),
                    RenderedPathFillRule::EvenOdd,
                    None,
                    PaintStrokeWidth::ZERO,
                    None,
                ));
            }
        } else if let Some(shape) = border_shape.clone() {
            paths.push(RenderedPath::new(
                shape.commands(),
                Some(fill),
                RenderedPathFillRule::NonZero,
                None,
                PaintStrokeWidth::ZERO,
                None,
            ));
        } else if style.border_radius.clone().is_zero() {
            rects.push(RenderedRect::from_paint_rect(area, Some(fill)));
        } else if style.corner_shapes.all_round() {
            if color_clip == css::BackgroundBox::Border {
                rounded_rects.push(RenderedRoundedRect::from_paint_rect(
                    area,
                    used_rounded_rect_radii(style.border_radius.clone(), rect.size),
                    Some(fill),
                    None,
                    PaintStrokeWidth::ZERO,
                ));
            } else if let Some(clip) =
                rounded_background_clip_for_box(rect, style, border_insets, color_clip)
            {
                paths.push(RenderedPath::new(
                    clip.commands,
                    Some(fill),
                    clip.fill_rule,
                    None,
                    PaintStrokeWidth::ZERO,
                    None,
                ));
            } else {
                rects.push(RenderedRect::from_paint_rect(area, Some(fill)));
            }
        } else {
            if let Some(clip) =
                rounded_background_clip_for_box(rect, style, border_insets, color_clip)
            {
                // `corner-shape` establishes the background's contour, but
                // it does not turn the source background layer into that
                // contour.  Keep the complete clip-box surface and apply the
                // contour as a PDF clip.  Besides matching CSS's paint model,
                // this avoids rasterizer-dependent coverage differences
                // between filling a bevel directly and clipping its
                // background surface to the same bevel.
                // <https://drafts.csswg.org/css-borders-4/#corner-shaping>
                paths.push(RenderedPath::new(
                    paint_rect_path_commands(area),
                    Some(fill),
                    RenderedPathFillRule::NonZero,
                    None,
                    PaintStrokeWidth::ZERO,
                    Some(clip),
                ));
            } else {
                rects.push(RenderedRect::from_paint_rect(area, Some(fill)));
            }
        }
    }
    if paint_backgrounds && !border_shape_is_empty {
        if let Some(shape) = resolved_inset_border_shape(rect, style, border_insets) {
            // CSS Backgrounds positions the generated image in its normal
            // origin box, then clips it to the border-shape's inner contour.
            // Keep those concerns separate: each gradient band retains its
            // existing image-space coordinates while this path clip supplies
            // the visual boundary.
            let shape_clip =
                RenderedPathClip::new(shape.commands(), RenderedPathFillRule::NonZero, Vec::new());
            paths.extend(
                linear_gradient_rects(rect, style, border_insets)
                    .into_iter()
                    .filter_map(|band| gradient_rect_path(band, shape_clip.clone())),
            );
            let mut angled_paths = linear_gradient_paths(rect, style, border_insets);
            for path in &mut angled_paths {
                path.clip = Some(shape_clip.clone());
            }
            paths.extend(angled_paths);
        } else if style.border_radius.clone().is_zero() {
            rects.extend(linear_gradient_rects(rect, style, border_insets));
        } else {
            paths.extend(linear_gradient_rect_paths(rect, style, border_insets));
        }
        paths.extend(linear_gradient_paths(rect, style, border_insets));
    }
    if paint_inset_shadows {
        paint_box_shadows(&mut rects, &mut paths, geometry, style, true);
    }
    if !paint_borders || style.border_image.source.is_image() {
        return (rects, rounded_rects, paths, strokes);
    }
    if let Some(pair) = border_shape_pair {
        // Outline synthesis clears backgrounds before reusing this paint
        // helper. Its two-shape contour is still an annular border and must
        // therefore paint from the relevant synthetic outline side.
        if style.background.background_color.is_transparent() {
            let ring_fill = if style.svg_fill.is_overridden() {
                style
                    .svg_fill
                    .paint
                    .resolve(style.color)
                    .unwrap_or(CssColor::TRANSPARENT)
            } else {
                relevant_border_shape_color(style).unwrap_or(CssColor::TRANSPARENT)
            };
            if ring_fill.is_visible() {
                paths.push(RenderedPath::new(
                    pair.commands(),
                    Some(ring_fill),
                    RenderedPathFillRule::EvenOdd,
                    None,
                    PaintStrokeWidth::ZERO,
                    None,
                ));
            }
        }
        return (rects, rounded_rects, paths, strokes);
    }
    if let Some(shape) = border_shape {
        paint_single_border_shape(&mut paths, shape, style);
        return (rects, rounded_rects, paths, strokes);
    }
    if !paint_uniform_rounded_border(&mut rounded_rects, rect, style)
        && !paint_uniform_double_rounded_border(&mut paths, rect, style)
        && !paint_solid_rounded_border_ring(&mut paths, rect, style)
        && !paint_patterned_rounded_border_sides(&mut paths, rect, style)
        && !paint_clipped_rounded_border_sides(&mut paths, rect, style)
    {
        paint_border_edges(
            &mut rects,
            &mut paths,
            PageTopRect::new(
                rect.origin.x,
                rect.max_y(),
                rect.size.width,
                rect.size.height,
            ),
            style,
        );
    }
    (rects, rounded_rects, paths, strokes)
}

/// Used paint geometry for one CSS Borders 4 circular `border-shape`.
///
/// A `border-shape` is visual-only, so this is constructed at the paint
/// boundary from the relevant border/padding/content geometry rather than
/// leaking shape coordinates into layout sizing.
#[derive(Debug, Clone, Copy)]
struct ResolvedBorderShapeCircle {
    center: PaintPoint,
    radius: f32,
}

/// Used paint geometry for one CSS Borders 4 elliptical `border-shape`.
#[derive(Debug, Clone, Copy)]
struct ResolvedBorderShapeEllipse {
    center: PaintPoint,
    horizontal_radius: f32,
    vertical_radius: f32,
}

/// Used paint geometry for a closed line-only CSS `shape()` border path.
#[derive(Debug, Clone)]
struct ResolvedBorderShapePath {
    vertices: Vec<PaintPoint>,
}

/// A resolved uniform rounded `inset()` contour.
#[derive(Debug, Clone, Copy)]
struct ResolvedBorderShapeRoundedRect {
    rect: PaintRect,
    radius_x: f32,
    radius_y: f32,
}

/// One resolved single-shape contour usable by background, border, and
/// overflow paint without exposing basic-shape geometry to layout.
#[derive(Debug, Clone)]
enum ResolvedBorderShape {
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
struct ResolvedBorderShapePair {
    outer: ResolvedBorderShape,
    inner: ResolvedBorderShape,
}

impl ResolvedBorderShapePair {
    fn commands(&self) -> Vec<RenderedPathCommand> {
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
    fn commands(&self) -> Vec<RenderedPathCommand> {
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

    fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Returns the parallel circular or elliptical contour at `distance` from
    /// this contour.  Positive values expand away from the center and negative
    /// values contract toward it.
    fn outset(&self, distance: f32) -> Option<Self> {
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
    fn translated(&self, displacement: PaintDisplacement) -> Self {
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

fn resolved_single_border_shape(
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
fn resolved_outer_border_shape(
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
fn resolved_inset_border_shape(
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

fn resolved_border_shape_pair(
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

fn paint_single_border_shape(
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
fn relevant_border_shape_color(style: &ComputedStyle) -> Option<CssColor> {
    relevant_border_shape_side(style).map(|side| side.color)
}

/// Converts supported linear gradients to filled rectangle bands.
///
/// CSS Images defines gradients as generated images. For axis-aligned
/// hard-stop gradients, equivalent rectangle bands preserve the specified
/// colors and stop positions exactly in PDF output:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
pub(in crate::layout) fn linear_gradient_rects(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Vec<RenderedRect> {
    linear_gradient_rects_with_clip(rect, style, border_insets, None)
}

/// Converts axis-aligned hard-stop linear gradients with an extra clip.
///
/// CSS Images positions gradients in their generated image box, while CSS
/// Backgrounds clips each layer independently. Table structural backgrounds
/// reuse the full column box for positioning and row fragments as the clip:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients> and
/// <https://www.w3.org/TR/css-backgrounds-3/#backgrounds>.
pub(in crate::layout) fn linear_gradient_rects_with_clip(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    extra_clip: Option<PaintRect>,
) -> Vec<RenderedRect> {
    let mut rects = Vec::new();
    for layer in background_layers_for_gradient_paint(style).iter().rev() {
        let Some(BackgroundImage::LinearGradient(gradient)) = layer.image.as_image() else {
            continue;
        };
        if !linear_gradient_can_paint_as_vector(gradient, layer) {
            continue;
        }
        let area = background_rect_area_for_box(rect, style, border_insets, layer.origin);
        let clip =
            background_rect_clip_area_for_box(rect, style, border_insets, layer.clip, extra_clip);
        let Some(axis_direction) = axis_aligned_gradient_direction(gradient.direction) else {
            continue;
        };
        let axis_length = axis_aligned_gradient_length(axis_direction, area);
        let Some(stops) = fixed_gradient_stops(gradient, axis_length) else {
            continue;
        };
        if !fixed_gradient_is_hard_stop(&stops) {
            continue;
        }

        let before = rects.len();
        let first = stops[0];
        push_gradient_band(
            &mut rects,
            axis_direction,
            area,
            0.0,
            first.position,
            first.color,
        );
        for pair in stops.windows(2) {
            push_gradient_band(
                &mut rects,
                axis_direction,
                area,
                pair[0].position,
                pair[1].position,
                pair[0].color,
            );
        }
        let last = *stops.last().expect("checked length above");
        push_gradient_band(
            &mut rects,
            axis_direction,
            area,
            last.position,
            axis_length,
            last.color,
        );
        for rect in &mut rects[before..] {
            clip_gradient_rect(rect, clip);
        }
    }
    rects.retain(|rect| rect.width() > 0.0 && rect.height() > 0.0);
    rects
}

/// Converts supported axis-aligned hard-stop linear gradients to filled paths
/// clipped by the rounded background clip area.
///
/// CSS Backgrounds clips background images, including CSS Images gradients, to
/// the curve of the `background-clip` box when `border-radius` is nonzero:
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-clipping> and
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
pub(in crate::layout) fn linear_gradient_rect_paths(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Vec<RenderedPath> {
    linear_gradient_rect_paths_with_clip(rect, style, border_insets, None)
}

/// Converts rounded axis-aligned hard-stop gradients with an extra clip.
///
/// CSS Backgrounds clips generated-image layers to `background-clip`; callers
/// may intersect that clip with a fragment-local exposed area:
/// <https://www.w3.org/TR/css-backgrounds-3/#background-clip>.
pub(in crate::layout) fn linear_gradient_rect_paths_with_clip(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    extra_clip: Option<PaintRect>,
) -> Vec<RenderedPath> {
    let mut paths = Vec::new();
    for layer in background_layers_for_gradient_paint(style).iter().rev() {
        let Some(BackgroundImage::LinearGradient(gradient)) = layer.image.as_image() else {
            continue;
        };
        if !linear_gradient_can_paint_as_vector(gradient, layer) {
            continue;
        }
        let Some(axis_direction) = axis_aligned_gradient_direction(gradient.direction) else {
            continue;
        };
        let area = background_rect_area_for_box(rect, style, border_insets, layer.origin);
        let clip =
            background_rect_clip_area_for_box(rect, style, border_insets, layer.clip, extra_clip);
        let Some(rounded_clip) =
            rounded_background_clip_for_box(rect, style, border_insets, layer.clip)
        else {
            continue;
        };
        let axis_length = axis_aligned_gradient_length(axis_direction, area);
        let Some(stops) = fixed_gradient_stops(gradient, axis_length) else {
            continue;
        };
        if !fixed_gradient_is_hard_stop(&stops) {
            continue;
        }

        let mut rects = Vec::new();
        let first = stops[0];
        push_gradient_band(
            &mut rects,
            axis_direction,
            area,
            0.0,
            first.position,
            first.color,
        );
        for pair in stops.windows(2) {
            push_gradient_band(
                &mut rects,
                axis_direction,
                area,
                pair[0].position,
                pair[1].position,
                pair[0].color,
            );
        }
        let last = *stops.last().expect("checked length above");
        push_gradient_band(
            &mut rects,
            axis_direction,
            area,
            last.position,
            axis_length,
            last.color,
        );
        for rect in &mut rects {
            clip_gradient_rect(rect, clip);
        }
        paths.extend(
            rects
                .into_iter()
                .filter(|rect| rect.width() > 0.0 && rect.height() > 0.0)
                .filter_map(|rect| gradient_rect_path(rect, rounded_clip.clone())),
        );
    }
    paths
}

/// Converts supported angled hard-stop linear gradients to filled polygons.
///
/// CSS Images defines angle gradients by projecting color stops onto a
/// gradient line through the gradient box. For hard-stop gradients, each color
/// band is an intersection of the background clip rectangle with two
/// perpendicular half-planes:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
pub(in crate::layout) fn linear_gradient_paths(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Vec<RenderedPath> {
    linear_gradient_paths_with_clip(rect, style, border_insets, None)
}

/// Converts angled hard-stop linear gradients with an extra clip.
///
/// CSS Images defines angled gradients in the full gradient box. The optional
/// clip only constrains the painted polygon, preserving that coordinate space:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
pub(in crate::layout) fn linear_gradient_paths_with_clip(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    extra_clip: Option<PaintRect>,
) -> Vec<RenderedPath> {
    let mut paths = Vec::new();
    for layer in background_layers_for_gradient_paint(style).iter().rev() {
        let Some(BackgroundImage::LinearGradient(gradient)) = layer.image.as_image() else {
            continue;
        };
        let area = background_rect_area_for_box(rect, style, border_insets, layer.origin);
        let clip =
            background_rect_clip_area_for_box(rect, style, border_insets, layer.clip, extra_clip);
        let rounded_clip = rounded_background_clip_for_box(rect, style, border_insets, layer.clip);
        if let Some(layer_paths) =
            linear_gradient_hard_stop_paths(gradient, layer, area, clip, rounded_clip)
        {
            paths.extend(layer_paths);
        }
    }
    paths
}

/// Paint a non-repeating, angled, hard-stop gradient as CSS image-space
/// polygons. The positioning and clip rectangles are supplied independently
/// so table structural backgrounds can preserve their full column image box
/// while exposing only row-fragment slices.
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>
pub(in crate::layout) fn linear_gradient_hard_stop_paths(
    gradient: &css::LinearGradient,
    layer: &css::BackgroundLayer,
    area: PaintRect,
    clip: PaintRect,
    rounded_clip: Option<RenderedPathClip>,
) -> Option<Vec<RenderedPath>> {
    if !linear_gradient_can_paint_as_vector(gradient, layer)
        || axis_aligned_gradient_direction(gradient.direction).is_some()
    {
        return None;
    }
    linear_gradient_hard_stop_paths_in_gradient_box(gradient, area, clip, rounded_clip)
}

/// Paint an angled hard-stop gradient whose `area` is its resolved generated
/// image tile. Unlike box-decoration painting, this path receives the used
/// `background-size` tile directly and therefore does not require the layer's
/// size or position to be initial values.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>
pub(in crate::layout) fn linear_gradient_hard_stop_tile_paths(
    gradient: &css::LinearGradient,
    area: PaintRect,
    clip: PaintRect,
    rounded_clip: Option<RenderedPathClip>,
) -> Option<Vec<RenderedPath>> {
    if gradient.repeating || !gradient.hints.is_empty() {
        return None;
    }
    linear_gradient_hard_stop_paths_in_gradient_box(gradient, area, clip, rounded_clip)
}

fn linear_gradient_hard_stop_paths_in_gradient_box(
    gradient: &css::LinearGradient,
    area: PaintRect,
    clip: PaintRect,
    rounded_clip: Option<RenderedPathClip>,
) -> Option<Vec<RenderedPath>> {
    let line = angled_gradient_line(gradient.direction, area);
    let stops = fixed_gradient_stops(gradient, line.axis_length)?;
    if !fixed_gradient_is_hard_stop(&stops) {
        return None;
    }

    let mut paths = Vec::new();
    let first = stops[0];
    push_gradient_polygon_band(
        &mut paths,
        line,
        clip,
        0.0,
        first.position,
        first.color,
        rounded_clip.clone(),
    );
    for pair in stops.windows(2) {
        push_gradient_polygon_band(
            &mut paths,
            line,
            clip,
            pair[0].position,
            pair[1].position,
            pair[0].color,
            rounded_clip.clone(),
        );
    }
    let last = *stops.last().expect("checked length above");
    push_gradient_polygon_band(
        &mut paths,
        line,
        clip,
        last.position,
        line.axis_length,
        last.color,
        rounded_clip,
    );
    Some(paths)
}

pub(in crate::layout) fn linear_gradient_can_paint_as_vector(
    gradient: &css::LinearGradient,
    layer: &css::BackgroundLayer,
) -> bool {
    !gradient.repeating
        && gradient.hints.is_empty()
        && layer.size == css::BackgroundSize::Auto
        && layer.position == css::BackgroundPosition::INITIAL
}

/// Whether the box-decoration painter already emits this hard-stop layer as
/// exact vector geometry, so the generic generated-image painter must not
/// paint it a second time.
pub(in crate::layout) fn linear_gradient_is_painted_by_box_decoration(
    gradient: &css::LinearGradient,
    layer: &css::BackgroundLayer,
    size: PaintSize,
) -> bool {
    if !linear_gradient_can_paint_as_vector(gradient, layer) {
        return false;
    }
    let gradient_box = PaintRect::new(PaintPoint::new(0.0, 0.0), size);
    if let Some(direction) = axis_aligned_gradient_direction(gradient.direction) {
        return gradient
            .stops
            .iter()
            .all(|stop| stop.color.as_color().is_some_and(CssColor::is_opaque))
            && fixed_gradient_stops(
                gradient,
                axis_aligned_gradient_length(direction, gradient_box),
            )
            .is_some_and(|stops| fixed_gradient_is_hard_stop(&stops));
    }
    linear_gradient_hard_stop_paths(gradient, layer, gradient_box, gradient_box, None).is_some()
}

pub(in crate::layout) fn gradient_stop_position(
    stop: css::GradientColorStop,
    axis_length: f32,
) -> Option<f32> {
    let position = stop.position?;
    Some(
        position
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(axis_length)))
            .map(layout_points)
            .unwrap_or(position.length_points()),
    )
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FixedGradientStop {
    pub(in crate::layout) color: CssColor,
    pub(in crate::layout) missing_components: css::GradientMissingComponents,
    pub(in crate::layout) position: f32,
}

/// Applies the CSS Images Level 3 color-stop fixup algorithm.
///
/// The first and last omitted positions are defaulted to the line endpoints,
/// decreasing explicit positions are moved forward, and omitted runs are
/// evenly distributed between surrounding explicit positions:
/// <https://www.w3.org/TR/css-images-3/#color-stop-fixup>.
pub(in crate::layout) fn fixed_gradient_stops(
    gradient: &css::LinearGradient,
    axis_length: f32,
) -> Option<Vec<FixedGradientStop>> {
    if axis_length <= 0.0 || gradient.stops.len() < 2 {
        return None;
    }
    let mut positions = gradient
        .stops
        .iter()
        .cloned()
        .map(|stop| gradient_stop_position(stop, axis_length).map(canonical_gradient_stop_position))
        .collect::<Vec<_>>();
    positions[0].get_or_insert(0.0);
    let last_index = positions.len() - 1;
    positions[last_index].get_or_insert(axis_length);

    let mut previous = positions[0].expect("defaulted first stop");
    for position in positions.iter_mut().skip(1).flatten() {
        if *position < previous {
            *position = previous;
        }
        previous = *position;
    }

    let mut index = 0usize;
    while index < positions.len() {
        if positions[index].is_some() {
            index += 1;
            continue;
        }
        let run_start = index;
        while index < positions.len() && positions[index].is_none() {
            index += 1;
        }
        let before = positions[run_start - 1].expect("first stop defaulted");
        let after = positions[index].expect("last stop defaulted");
        let slots = (index - run_start + 1) as f32;
        for (offset, position) in positions[run_start..index].iter_mut().enumerate() {
            let step = (offset + 1) as f32 / slots;
            *position = Some(before + (after - before) * step);
        }
    }

    gradient
        .stops
        .iter()
        .zip(positions)
        .map(|(stop, position)| {
            Some(FixedGradientStop {
                color: stop.color.as_color()?,
                missing_components: stop.color.missing_components_for(gradient.interpolation),
                position: position.expect("all positions fixed up"),
            })
        })
        .collect()
}

/// Canonicalize a used gradient coordinate before color-stop fixup and raster
/// sampling.
///
/// CSS calculations such as `calc(10% + 20px)` and an equivalent `30px`
/// position may arrive through distinct floating-point operation sequences.
/// Generated images sample those coordinates many times, so retaining a
/// sub-ULP difference can produce a one-channel raster discrepancy despite
/// identical CSS used values. One 1/4096-point quantum is far below the
/// generated-image sampling grid while making equivalent used coordinates
/// stable across expression forms:
/// <https://www.w3.org/TR/css-images-3/#color-stop-fixup>.
fn canonical_gradient_stop_position(position: f32) -> f32 {
    const QUANTA_PER_POINT: f32 = 4096.0;
    (position * QUANTA_PER_POINT).round() / QUANTA_PER_POINT
}

pub(in crate::layout) fn fixed_gradient_is_hard_stop(stops: &[FixedGradientStop]) -> bool {
    stops.windows(2).all(|pair| {
        (pair[0].position - pair[1].position).abs() <= 0.001 || pair[0].color == pair[1].color
    })
}

pub(in crate::layout) fn background_layers_for_gradient_paint(
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

fn axis_aligned_gradient_direction(
    direction: LinearGradientDirection,
) -> Option<LinearGradientDirection> {
    let LinearGradientDirection::Angle(angle) = direction else {
        return None;
    };
    let angle = angle.rem_euclid(360.0);
    if (angle - 0.0).abs() < 0.001 {
        Some(LinearGradientDirection::Angle(0.0))
    } else if (angle - 90.0).abs() < 0.001 {
        Some(LinearGradientDirection::Angle(90.0))
    } else if (angle - 180.0).abs() < 0.001 {
        Some(LinearGradientDirection::Angle(180.0))
    } else if (angle - 270.0).abs() < 0.001 {
        Some(LinearGradientDirection::Angle(270.0))
    } else {
        None
    }
}

fn axis_aligned_gradient_length(direction: LinearGradientDirection, area: PaintRect) -> f32 {
    match direction {
        LinearGradientDirection::Angle(angle)
            if (angle.rem_euclid(360.0) - 0.0).abs() < 0.001
                || (angle.rem_euclid(360.0) - 180.0).abs() < 0.001 =>
        {
            area.size.height
        }
        LinearGradientDirection::Angle(angle)
            if (angle.rem_euclid(360.0) - 90.0).abs() < 0.001
                || (angle.rem_euclid(360.0) - 270.0).abs() < 0.001 =>
        {
            area.size.width
        }
        _ => 0.0,
    }
}

/// Unitless normalized direction of a gradient axis.
///
/// Gradient direction components are not page-local distances. Keeping them
/// separate from [`PaintDisplacement`] prevents using a direction where a
/// physical paint offset is required.
#[derive(Debug, Clone, Copy)]
struct PaintDirection(euclid::Vector2D<f32, GradientDirectionSpace>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GradientDirectionSpace {}

impl PaintDirection {
    fn from_components(x: f32, y: f32) -> Self {
        debug_assert!(((x * x + y * y).sqrt() - 1.0).abs() <= 0.001);
        Self(euclid::Vector2D::new(x, y))
    }

    fn project(self, displacement: PaintDisplacement) -> f32 {
        displacement.x * self.0.x + displacement.y * self.0.y
    }

    fn scaled(self, length: f32) -> PaintDisplacement {
        PaintDisplacement::new(self.0.x * length, self.0.y * length)
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AngledGradientLine {
    pub(in crate::layout) center: PaintPoint,
    direction: PaintDirection,
    pub(in crate::layout) axis_length: f32,
}

impl AngledGradientLine {
    /// Returns the start and end of the gradient axis in paint space.
    ///
    /// The unitless direction remains private so callers cannot mistake it
    /// for a physical [`PaintDisplacement`].
    pub(in crate::layout) fn endpoints(self) -> (PaintPoint, PaintPoint) {
        let half_length = self.axis_length / 2.0;
        (
            self.center - self.direction.scaled(half_length),
            self.center + self.direction.scaled(half_length),
        )
    }
}

pub(in crate::layout) fn angled_gradient_line(
    direction: LinearGradientDirection,
    area: PaintRect,
) -> AngledGradientLine {
    let angle = gradient_direction_angle_for_area(direction, area);
    let radians = angle.to_radians();
    let dir_x = radians.sin();
    let dir_y = radians.cos();
    let axis_length = area.size.width * dir_x.abs() + area.size.height * dir_y.abs();
    AngledGradientLine {
        center: PaintPoint::new(
            area.origin.x + area.size.width / 2.0,
            area.origin.y + area.size.height / 2.0,
        ),
        direction: PaintDirection::from_components(dir_x, dir_y),
        axis_length,
    }
}

/// Resolve a CSS linear-gradient direction against its concrete gradient box.
///
/// For `to <corner>`, CSS Images requires the gradient line to point into the
/// requested quadrant while remaining perpendicular to the line through the
/// two neighboring corners.  Using the opposite box span for each directional
/// component preserves that “magic corners” invariant on non-square boxes:
/// a 50% color stop intersects those neighboring corners.
/// <https://drafts.csswg.org/css-images-3/#linear-gradients>
pub(in crate::layout) fn gradient_direction_angle_for_area(
    direction: LinearGradientDirection,
    area: PaintRect,
) -> f32 {
    match direction {
        LinearGradientDirection::Angle(angle) => angle,
        LinearGradientDirection::Corner {
            horizontal,
            vertical,
        } => {
            let x = match horizontal {
                css::GradientHorizontalDirection::Left => -area.size.height,
                css::GradientHorizontalDirection::Right => area.size.height,
            };
            let y = match vertical {
                css::GradientVerticalDirection::Top => area.size.width,
                css::GradientVerticalDirection::Bottom => -area.size.width,
            };
            x.atan2(y).to_degrees().rem_euclid(360.0)
        }
    }
}

/// Build the rounded clipping path for a CSS background layer.
///
/// CSS Backgrounds and Borders Level 3 clips backgrounds to the curve
/// established by `border-radius`; CSS Borders 4 `corner-shape` is kept for
/// border contour painting rather than changing the background fill clip:
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-clipping>.
pub(in crate::layout) fn rounded_background_clip_for_box(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    clip_box: css::BackgroundBox,
) -> Option<RenderedPathClip> {
    if clip_box == css::BackgroundBox::BorderArea {
        let outer_radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
        let inner =
            background_rect_area_for_box(rect, style, border_insets, css::BackgroundBox::Padding);
        let mut commands = shaped_rect_path_commands(rect, outer_radii, style.corner_shapes);
        if inner.size.width > 0.0 && inner.size.height > 0.0 {
            let inner_radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
            commands.extend(shaped_rect_path_commands(
                inner,
                RenderedRoundedRectRadii {
                    top_left: RenderedCornerRadius::new(
                        inner_radii.top_left.x() - border_insets.left,
                        inner_radii.top_left.y() - border_insets.top,
                    ),
                    top_right: RenderedCornerRadius::new(
                        inner_radii.top_right.x() - border_insets.right,
                        inner_radii.top_right.y() - border_insets.top,
                    ),
                    bottom_right: RenderedCornerRadius::new(
                        inner_radii.bottom_right.x() - border_insets.right,
                        inner_radii.bottom_right.y() - border_insets.bottom,
                    ),
                    bottom_left: RenderedCornerRadius::new(
                        inner_radii.bottom_left.x() - border_insets.left,
                        inner_radii.bottom_left.y() - border_insets.bottom,
                    ),
                },
                style.corner_shapes,
            ));
        }
        return Some(RenderedPathClip::new(
            commands,
            RenderedPathFillRule::EvenOdd,
            Vec::new(),
        ));
    }
    let rounded_rect = rounded_clip_rect_for_box(rect, style, border_insets, clip_box)?;
    Some(RenderedPathClip::new(
        shaped_rect_path_commands(
            rounded_rect.paint_rect(),
            rounded_rect.radii,
            style.corner_shapes,
        ),
        RenderedPathFillRule::NonZero,
        Vec::new(),
    ))
}

/// Build the rounded used clip area for a CSS box edge.
///
/// Paint containment clips descendants at the padding edge, including the
/// curve derived from the principal box's border radii. Returning geometry
/// separately lets both background primitives and whole captured paint
/// fragments share the same used-radius calculation:
/// <https://www.w3.org/TR/css-contain-1/#containment-paint> and
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-clipping>.
pub(in crate::layout) fn rounded_clip_rect_for_box(
    rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    clip_box: css::BackgroundBox,
) -> Option<RenderedRoundedRect> {
    if style.border_radius.clone().is_zero() || rect.size.width <= 0.0 || rect.size.height <= 0.0 {
        return None;
    }
    let area = background_rect_area_for_box(rect, style, border_insets, clip_box);
    if area.size.width <= 0.0 || area.size.height <= 0.0 {
        return None;
    }
    let insets = background_clip_edge_insets(style, border_insets, clip_box);
    let mut radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
    radii.top_left = RenderedCornerRadius::new(
        radii.top_left.x() - insets.left,
        radii.top_left.y() - insets.top,
    );
    radii.top_right = RenderedCornerRadius::new(
        radii.top_right.x() - insets.right,
        radii.top_right.y() - insets.top,
    );
    radii.bottom_right = RenderedCornerRadius::new(
        radii.bottom_right.x() - insets.right,
        radii.bottom_right.y() - insets.bottom,
    );
    radii.bottom_left = RenderedCornerRadius::new(
        radii.bottom_left.x() - insets.left,
        radii.bottom_left.y() - insets.bottom,
    );
    // The outer radii were already reduced together against the border box by
    // `used_rounded_rect_radii`.  CSS derives each inner edge by subtracting
    // its corresponding border (and, for the content edge, padding) width,
    // clamping at zero.  Reducing those derived radii a second time against
    // the smaller inner rectangle changes the curve, notably for a single
    // `100%` corner.
    // <https://www.w3.org/TR/css-backgrounds-3/#corner-shaping>.
    if rounded_radii_are_zero(radii) {
        return None;
    }
    Some(
        RenderedRoundedRect::new(
            area.origin.x,
            area.origin.y,
            area.size.width,
            area.size.height,
            radii,
            None,
            None,
            PaintStrokeWidth::ZERO,
        )
        .with_corner_shapes(style.corner_shapes),
    )
}

/// Derive the exact rounded edge at an already-resolved rectangle.
///
/// CSS Backgrounds adjusts outward-growing radii with its coverage/ratio
/// cubic, while inward movement subtracts the inset and floors at zero.
/// <https://www.w3.org/TR/css-backgrounds-3/#shadow-shape>
pub(in crate::layout) fn rounded_clip_rect_for_box_at_edge(
    border_rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    reference_box: css::BackgroundBox,
    edge_rect: PaintRect,
) -> Option<RenderedRoundedRect> {
    if edge_rect.size.width <= 0.0 || edge_rect.size.height <= 0.0 {
        return None;
    }
    let reference = rounded_clip_rect_for_box(border_rect, style, border_insets, reference_box)?;
    let reference_rect = reference.paint_rect();
    let radii = adjusted_outset_rounded_rect_radii(
        reference.radii,
        reference_rect.size,
        css::Edges {
            top: edge_rect.max_y() - reference_rect.max_y(),
            right: edge_rect.max_x() - reference_rect.max_x(),
            bottom: reference_rect.min_y() - edge_rect.min_y(),
            left: reference_rect.min_x() - edge_rect.min_x(),
        },
    );
    if rounded_radii_are_zero(radii) {
        return None;
    }
    Some(
        RenderedRoundedRect::from_paint_rect(edge_rect, radii, None, None, PaintStrokeWidth::ZERO)
            .with_corner_shapes(style.corner_shapes),
    )
}

fn background_clip_edge_insets(
    style: &ComputedStyle,
    border_insets: css::Edges,
    clip_box: css::BackgroundBox,
) -> css::Edges {
    match clip_box {
        css::BackgroundBox::Border | css::BackgroundBox::BorderArea => css::Edges::ZERO,
        css::BackgroundBox::Padding => border_insets,
        css::BackgroundBox::Content => css::Edges {
            top: border_insets.top + style.padding.top,
            right: border_insets.right + style.padding.right,
            bottom: border_insets.bottom + style.padding.bottom,
            left: border_insets.left + style.padding.left,
        },
    }
}

pub(super) fn rounded_radii_are_zero(radii: RenderedRoundedRectRadii) -> bool {
    [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ]
    .into_iter()
    .all(|radius| radius.x() <= 0.0 && radius.y() <= 0.0)
}

fn gradient_rect_path(rect: RenderedRect, clip: RenderedPathClip) -> Option<RenderedPath> {
    let fill = rect.fill?;
    if !fill.is_visible() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    let x = rect.x();
    let y = rect.y();
    let width = rect.width();
    let height = rect.height();
    Some(RenderedPath::new(
        vec![
            RenderedPathCommand::move_to(paint_space_point(x, y)),
            RenderedPathCommand::line_to(paint_space_point(x + width, y)),
            RenderedPathCommand::line_to(paint_space_point(x + width, y + height)),
            RenderedPathCommand::line_to(paint_space_point(x, y + height)),
            RenderedPathCommand::Close,
        ],
        Some(fill),
        RenderedPathFillRule::NonZero,
        None,
        PaintStrokeWidth::ZERO,
        Some(clip),
    ))
}

fn push_gradient_polygon_band(
    paths: &mut Vec<RenderedPath>,
    line: AngledGradientLine,
    clip: PaintRect,
    start: f32,
    end: f32,
    color: CssColor,
    rounded_clip: Option<RenderedPathClip>,
) {
    if !color.is_visible() {
        return;
    }
    let start = start.clamp(0.0, line.axis_length);
    let end = end.clamp(0.0, line.axis_length);
    if end <= start || clip.size.width <= 0.0 || clip.size.height <= 0.0 {
        return;
    }
    let mut polygon = vec![
        clip.origin,
        PaintPoint::new(clip.origin.x + clip.size.width, clip.origin.y),
        PaintPoint::new(
            clip.origin.x + clip.size.width,
            clip.origin.y + clip.size.height,
        ),
        PaintPoint::new(clip.origin.x, clip.origin.y + clip.size.height),
    ];
    polygon = clip_gradient_polygon(polygon, line, start, true);
    polygon = clip_gradient_polygon(polygon, line, end, false);
    if polygon.len() < 3 {
        return;
    }
    let commands = std::iter::once(RenderedPathCommand::move_to(polygon[0]))
        .chain(
            polygon[1..]
                .iter()
                .copied()
                .map(RenderedPathCommand::line_to),
        )
        .chain(std::iter::once(RenderedPathCommand::Close))
        .collect::<Vec<_>>();
    paths.push(RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::NonZero,
        None,
        PaintStrokeWidth::ZERO,
        rounded_clip,
    ));
}

fn clip_gradient_polygon(
    polygon: Vec<PaintPoint>,
    line: AngledGradientLine,
    boundary: f32,
    keep_after: bool,
) -> Vec<PaintPoint> {
    if polygon.is_empty() {
        return polygon;
    }
    let mut output = Vec::new();
    let mut previous = *polygon.last().expect("checked non-empty");
    let mut previous_value = gradient_axis_position(previous, line) - boundary;
    let mut previous_inside = if keep_after {
        previous_value >= -0.001
    } else {
        previous_value <= 0.001
    };
    for current in polygon {
        let current_value = gradient_axis_position(current, line) - boundary;
        let current_inside = if keep_after {
            current_value >= -0.001
        } else {
            current_value <= 0.001
        };
        if current_inside != previous_inside
            && let Some(intersection) =
                gradient_boundary_intersection(previous, current, previous_value, current_value)
        {
            output.push(intersection);
        }
        if current_inside {
            output.push(current);
        }
        previous = current;
        previous_value = current_value;
        previous_inside = current_inside;
    }
    output
}

pub(in crate::layout) fn gradient_axis_position(
    point: PaintPoint,
    line: AngledGradientLine,
) -> f32 {
    line.direction.project(point - line.center) + line.axis_length / 2.0
}

fn gradient_boundary_intersection(
    start: PaintPoint,
    end: PaintPoint,
    start_value: f32,
    end_value: f32,
) -> Option<PaintPoint> {
    let denominator = start_value - end_value;
    if denominator.abs() <= f32::EPSILON {
        return None;
    }
    let t = (start_value / denominator).clamp(0.0, 1.0);
    Some(PaintPoint::new(
        start.x + (end.x - start.x) * t,
        start.y + (end.y - start.y) * t,
    ))
}

pub(in crate::layout) fn background_rect_area_for_box(
    rect: PaintRect,
    style: &ComputedStyle,
    border: css::Edges,
    box_: css::BackgroundBox,
) -> PaintRect {
    let area = rect;
    match box_ {
        css::BackgroundBox::Border | css::BackgroundBox::BorderArea => area,
        css::BackgroundBox::Padding => inset_paint_rect(area, border),
        css::BackgroundBox::Content => {
            inset_paint_rect(inset_paint_rect(area, border), style.padding)
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Return a background clip area intersected with an additional fragment clip.
///
/// CSS Backgrounds resolves `background-clip` against the box being painted;
/// fragmentation or table structural painting can further restrict the exposed
/// portion without changing the background positioning area:
/// <https://www.w3.org/TR/css-backgrounds-3/#background-clip>.
pub(in crate::layout) fn background_rect_clip_area_for_box(
    rect: PaintRect,
    style: &ComputedStyle,
    border: css::Edges,
    box_: css::BackgroundBox,
    extra_clip: Option<PaintRect>,
) -> PaintRect {
    let clip = background_rect_area_for_box(rect, style, border, box_);
    extra_clip.map_or(clip, |extra_clip| {
        intersect_paint_rect_or_empty(clip, extra_clip)
    })
}

#[derive(Clone, Copy)]
pub(in crate::layout) struct BoxPaintGeometry {
    pub(in crate::layout) rect: PaintRect,
    pub(in crate::layout) border_insets: css::Edges,
}

pub(in crate::layout) fn paint_box_shadows(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    geometry: BoxPaintGeometry,
    style: &ComputedStyle,
    inset: bool,
) {
    // Box decoration paint receives the frozen cascaded style while its
    // geometry has already crossed the layout used-value boundary. Materialize
    // the matching zoomed clone here so fixed shadow lengths cross that
    // boundary exactly once as CSS Viewport requires.
    // <https://drafts.csswg.org/css-viewport/#zoom-property>
    let zoomed_style = css::LayoutStyle::from_computed(style).into_zoomed();
    let style = &zoomed_style;
    for shadow in style
        .box_shadow
        .iter()
        .rev()
        .filter(|shadow| shadow.inset == inset)
    {
        let color = shadow.color.resolve(style.color);
        if !color.is_visible() || shadow.blur_radius.length_points() > 0.0 {
            continue;
        }
        if shadow.inset
            && let Some(shape) =
                resolved_inset_border_shape(geometry.rect, style, geometry.border_insets)
        {
            paint_inset_border_shape_shadow(paths, shape, shadow.clone(), color);
        } else if shadow.inset && !style.border_radius.clone().is_zero() {
            paint_inset_rounded_box_shadow(paths, geometry, style, shadow.clone(), color);
        } else if !shadow.inset
            && let Some(shape) =
                resolved_outer_border_shape(geometry.rect, style, geometry.border_insets)
        {
            paint_outer_border_shape_shadow(paths, shape, shadow.clone(), color);
        } else if !shadow.inset && !style.border_radius.clone().is_zero() {
            paint_outer_rounded_box_shadow(paths, geometry, style, shadow.clone(), color);
        } else if shadow.inset {
            paint_inset_box_shadow(rects, geometry, shadow.clone(), color);
        } else {
            paint_outer_box_shadow(rects, geometry, shadow.clone(), color);
        }
    }
}

/// Paint the non-blurred outer shadow of a CSS Borders 4 basic-shape contour.
///
/// CSS Backgrounds defines an outer shadow as the region between the shifted,
/// spread shadow shape and the unshifted border edge. For circles and
/// ellipses, spread is a contour offset rather than a rectangular expansion:
/// <https://www.w3.org/TR/css-backgrounds-3/#shadow-shape>.
fn paint_outer_border_shape_shadow(
    paths: &mut Vec<RenderedPath>,
    shape: ResolvedBorderShape,
    shadow: css::BoxShadow,
    color: CssColor,
) {
    let Some(shadow_outer) = shape.outset(shadow.spread.length_points()) else {
        return;
    };
    let shadow_outer = shadow_outer.translated(box_shadow_paint_offset(shadow));
    let mut commands = shadow_outer.commands();
    commands.extend(shape.commands());
    paths.push(RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::EvenOdd,
        None,
        PaintStrokeWidth::ZERO,
        None,
    ));
}

/// Paint the non-blurred inset shadow inside the visible background contour of
/// a CSS Borders 4 `border-shape`.
///
/// The shifted inset perimeter contracts by spread but preserves the resolved
/// circle or ellipse rather than falling back to a rectangle:
/// <https://www.w3.org/TR/css-backgrounds-3/#shadow-shape>.
fn paint_inset_border_shape_shadow(
    paths: &mut Vec<RenderedPath>,
    subject: ResolvedBorderShape,
    shadow: css::BoxShadow,
    color: CssColor,
) {
    let Some(perimeter) = subject.outset(-shadow.spread.length_points()) else {
        return;
    };
    let perimeter = perimeter.translated(box_shadow_paint_offset(shadow));
    let mut commands = subject.commands();
    commands.extend(perimeter.commands());
    paths.push(RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::EvenOdd,
        None,
        PaintStrokeWidth::ZERO,
        None,
    ));
}

/// Paint the non-blurred inset shadow inside a rounded or corner-shaped box.
///
/// The padding edge bounds the inset shadow and the shifted perimeter keeps
/// the same CSS Borders 4 corner contour:
/// <https://www.w3.org/TR/css-backgrounds-3/#shadow-shape>.
fn paint_inset_rounded_box_shadow(
    paths: &mut Vec<RenderedPath>,
    geometry: BoxPaintGeometry,
    style: &ComputedStyle,
    shadow: css::BoxShadow,
    color: CssColor,
) {
    let padding = inset_paint_rect(geometry.rect, geometry.border_insets);
    if padding.size.width <= 0.0 || padding.size.height <= 0.0 {
        return;
    }
    let spread = shadow.spread.length_points();
    let perimeter = PaintRect::new(
        padding.origin + PaintDisplacement::new(spread, spread) + box_shadow_paint_offset(shadow),
        PaintSize::new(
            (padding.size.width - spread * 2.0).max(0.0),
            (padding.size.height - spread * 2.0).max(0.0),
        ),
    );
    let subject_radii = padding_edge_rounded_rect_radii(
        used_rounded_rect_radii(style.border_radius.clone(), geometry.rect.size),
        geometry.border_insets,
    );
    let perimeter_radii = adjusted_outset_rounded_rect_radii(
        subject_radii,
        padding.size,
        css::Edges {
            top: -spread,
            right: -spread,
            bottom: -spread,
            left: -spread,
        },
    );
    let mut commands = shaped_rect_path_commands(padding, subject_radii, style.corner_shapes);
    if perimeter.size.width > 0.0 && perimeter.size.height > 0.0 {
        commands.extend(shaped_rect_path_commands(
            perimeter,
            perimeter_radii,
            style.corner_shapes,
        ));
    }
    paths.push(RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::EvenOdd,
        None,
        PaintStrokeWidth::ZERO,
        None,
    ));
}

/// Paint the non-blurred outer shadow of a rounded rectangle.
///
/// Positive spread grows both the shadow box and its corner radii. The path
/// ring makes an ordinary rounded box agree with an equivalent ellipse-shaped
/// border contour:
/// <https://www.w3.org/TR/css-backgrounds-3/#shadow-shape>.
fn paint_outer_rounded_box_shadow(
    paths: &mut Vec<RenderedPath>,
    geometry: BoxPaintGeometry,
    style: &ComputedStyle,
    shadow: css::BoxShadow,
    color: CssColor,
) {
    let spread = shadow.spread.length_points();
    let outer_size = PaintSize::new(
        geometry.rect.size.width + spread * 2.0,
        geometry.rect.size.height + spread * 2.0,
    );
    if outer_size.width <= 0.0 || outer_size.height <= 0.0 {
        return;
    }
    let outer_rect = PaintRect::new(
        geometry.rect.origin + box_shadow_paint_offset(shadow)
            - PaintDisplacement::new(spread, spread),
        outer_size,
    );
    let outer_radii = adjusted_outset_rounded_rect_radii(
        used_rounded_rect_radii(style.border_radius.clone(), geometry.rect.size),
        geometry.rect.size,
        css::Edges {
            top: spread,
            right: spread,
            bottom: spread,
            left: spread,
        },
    );
    let mut commands = shaped_rect_path_commands(outer_rect, outer_radii, style.corner_shapes);
    commands.extend(rounded_box_path_commands_for_insets(
        geometry.rect,
        style,
        css::Edges::ZERO,
    ));
    paths.push(RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::EvenOdd,
        None,
        PaintStrokeWidth::ZERO,
        None,
    ));
}

fn adjusted_outset_rounded_rect_radii(
    radii: RenderedRoundedRectRadii,
    edge_size: PaintSize,
    outsets: css::Edges,
) -> RenderedRoundedRectRadii {
    let outset_corner = |corner: RenderedCornerRadius, x_outset: f32, y_outset: f32| {
        let coverage = 2.0
            * (corner.x() / edge_size.width.max(f32::EPSILON))
                .min(corner.y() / edge_size.height.max(f32::EPSILON));
        let adjust = |radius: f32, outset: f32| {
            if outset <= 0.0 {
                return (radius + outset).max(0.0);
            }
            if radius > outset || coverage > 1.0 {
                return radius + outset;
            }
            let ratio = radius / outset;
            radius + outset * (1.0 - (1.0 - ratio).powi(3) * (1.0 - coverage.powi(3)))
        };
        RenderedCornerRadius::new(adjust(corner.x(), x_outset), adjust(corner.y(), y_outset))
    };
    RenderedRoundedRectRadii {
        top_left: outset_corner(radii.top_left, outsets.left, outsets.top),
        top_right: outset_corner(radii.top_right, outsets.right, outsets.top),
        bottom_right: outset_corner(radii.bottom_right, outsets.right, outsets.bottom),
        bottom_left: outset_corner(radii.bottom_left, outsets.left, outsets.bottom),
    }
}

/// Derive the padding-edge corner radii from a border-edge rounded rectangle.
///
/// CSS Backgrounds reduces each physical corner axis by the adjacent used
/// border width when moving from the border edge to the padding edge:
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-shaping>.
fn padding_edge_rounded_rect_radii(
    radii: RenderedRoundedRectRadii,
    inset: css::Edges,
) -> RenderedRoundedRectRadii {
    RenderedRoundedRectRadii {
        top_left: RenderedCornerRadius::new(
            radii.top_left.x() - inset.left,
            radii.top_left.y() - inset.top,
        ),
        top_right: RenderedCornerRadius::new(
            radii.top_right.x() - inset.right,
            radii.top_right.y() - inset.top,
        ),
        bottom_right: RenderedCornerRadius::new(
            radii.bottom_right.x() - inset.right,
            radii.bottom_right.y() - inset.bottom,
        ),
        bottom_left: RenderedCornerRadius::new(
            radii.bottom_left.x() - inset.left,
            radii.bottom_left.y() - inset.bottom,
        ),
    }
}

pub(in crate::layout) fn paint_outer_box_shadow(
    rects: &mut Vec<RenderedRect>,
    geometry: BoxPaintGeometry,
    shadow: css::BoxShadow,
    color: CssColor,
) {
    let offset = box_shadow_paint_offset(shadow.clone());
    let spread = shadow.spread.length_points();
    let shadow_width = geometry.rect.size.width + spread * 2.0;
    let shadow_height = geometry.rect.size.height + spread * 2.0;
    if shadow_width <= 0.0 || shadow_height <= 0.0 {
        return;
    }

    push_rect_difference(
        rects,
        PaintRect::new(
            geometry.rect.origin + offset - PaintDisplacement::new(spread, spread),
            PaintSize::new(shadow_width, shadow_height),
        ),
        geometry.rect,
        color,
    );
}

pub(in crate::layout) fn paint_inset_box_shadow(
    rects: &mut Vec<RenderedRect>,
    geometry: BoxPaintGeometry,
    shadow: css::BoxShadow,
    color: CssColor,
) {
    let padding = inset_paint_rect(geometry.rect, geometry.border_insets);
    if padding.size.width <= 0.0 || padding.size.height <= 0.0 {
        return;
    }

    let spread = shadow.spread.length_points();
    let offset = box_shadow_paint_offset(shadow);
    // An inset shadow paints the padding box outside the shifted shadow
    // perimeter.  Its spread contracts that perimeter for a positive value
    // and expands it for a negative value; only after that adjustment is the
    // perimeter shifted by the shadow offsets.  Paint-space Y grows upward,
    // hence CSS's downward-positive Y offset is negated here.
    // <https://www.w3.org/TR/css-backgrounds-3/#shadow-shape>
    let perimeter_width = (padding.size.width - spread * 2.0).max(0.0);
    let perimeter_height = (padding.size.height - spread * 2.0).max(0.0);
    let perimeter = PaintRect::new(
        padding.origin + PaintDisplacement::new(spread, spread) + offset,
        PaintSize::new(perimeter_width, perimeter_height),
    );
    push_rect_difference(rects, padding, perimeter, color);
}

/// Resolve CSS's right/down shadow offset into bottom-left paint space.
fn box_shadow_paint_offset(shadow: css::BoxShadow) -> PaintDisplacement {
    PaintDisplacement::new(
        shadow.offset_x.length_points(),
        -shadow.offset_y.length_points(),
    )
}

pub(in crate::layout) fn push_rect_difference(
    rects: &mut Vec<RenderedRect>,
    subject: PaintRect,
    cutout: PaintRect,
    color: CssColor,
) {
    let left = subject.origin.x;
    let right = subject.origin.x + subject.size.width;
    let bottom = subject.origin.y;
    let top = subject.origin.y + subject.size.height;
    let cut_left = cutout.origin.x.max(left).min(right);
    let cut_right = (cutout.origin.x + cutout.size.width).max(left).min(right);
    let cut_bottom = cutout.origin.y.max(bottom).min(top);
    let cut_top = (cutout.origin.y + cutout.size.height).max(bottom).min(top);

    push_shadow_rect(
        rects,
        paint_space_rect(left, bottom, subject.size.width, cut_bottom - bottom),
        color,
    );
    push_shadow_rect(
        rects,
        paint_space_rect(left, cut_top, subject.size.width, top - cut_top),
        color,
    );
    push_shadow_rect(
        rects,
        paint_space_rect(left, cut_bottom, cut_left - left, cut_top - cut_bottom),
        color,
    );
    push_shadow_rect(
        rects,
        paint_space_rect(
            cut_right,
            cut_bottom,
            right - cut_right,
            cut_top - cut_bottom,
        ),
        color,
    );
}

pub(in crate::layout) fn push_shadow_rect(
    rects: &mut Vec<RenderedRect>,
    rect: PaintRect,
    color: CssColor,
) {
    if rect.size.width > 0.0 && rect.size.height > 0.0 {
        rects.push(RenderedRect::from_paint_rect(rect, Some(color)));
    }
}

pub(in crate::layout) fn clip_gradient_rect(rect: &mut RenderedRect, clip: PaintRect) {
    rect.set_paint_rect(intersect_paint_rect_or_empty(rect.paint_rect(), clip));
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn push_gradient_band(
    rects: &mut Vec<RenderedRect>,
    direction: LinearGradientDirection,
    rect: PaintRect,
    start: f32,
    end: f32,
    color: CssColor,
) {
    if !color.is_visible() {
        return;
    }
    let angle = match direction {
        LinearGradientDirection::Angle(angle) => angle.rem_euclid(360.0),
        LinearGradientDirection::Corner { .. } => return,
    };
    let axis_length = if (angle - 0.0).abs() < 0.001 || (angle - 180.0).abs() < 0.001 {
        rect.size.height
    } else if (angle - 90.0).abs() < 0.001 || (angle - 270.0).abs() < 0.001 {
        rect.size.width
    } else {
        return;
    };
    let start = start.clamp(0.0, axis_length);
    let end = end.clamp(0.0, axis_length);
    if end <= start {
        return;
    }
    let rect = if (angle - 180.0).abs() < 0.001 {
        RenderedRect::from_paint_rect(
            paint_space_rect(
                rect.origin.x,
                rect.origin.y + rect.size.height - end,
                rect.size.width,
                end - start,
            ),
            Some(color),
        )
    } else if (angle - 0.0).abs() < 0.001 {
        RenderedRect::from_paint_rect(
            paint_space_rect(
                rect.origin.x,
                rect.origin.y + start,
                rect.size.width,
                end - start,
            ),
            Some(color),
        )
    } else if (angle - 90.0).abs() < 0.001 {
        RenderedRect::from_paint_rect(
            paint_space_rect(
                rect.origin.x + start,
                rect.origin.y,
                end - start,
                rect.size.height,
            ),
            Some(color),
        )
    } else {
        RenderedRect::from_paint_rect(
            paint_space_rect(
                rect.origin.x + rect.size.width - end,
                rect.origin.y,
                end - start,
                rect.size.height,
            ),
            Some(color),
        )
    };
    rects.push(rect);
}

/// Paint a uniform solid rounded border as one inset stroked rounded path.
///
/// CSS Backgrounds and Borders Level 3 defines rounded border curves as the
/// area between the outer border edge and the inner padding edge. For the
/// uniform solid case, a PDF stroked rounded path centered halfway through the
/// border width is the vector primitive that preserves the correct outer and
/// inner radius relationship without decomposing the border into rectangular
/// side strips:
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-shaping>.
pub(crate) fn paint_uniform_rounded_border(
    rounded_rects: &mut Vec<RenderedRoundedRect>,
    rect: PaintRect,
    style: &ComputedStyle,
) -> bool {
    // Select the straight-border paint representation when the *used* radii
    // are square. A radius such as `0 / 5px` has a nonzero computed vertical
    // component, but CSS defines its corner as square; sending that geometry
    // through a zero-radius rounded-path primitive changes PDF edge coverage.
    if rounded_radii_are_zero(used_rounded_rect_radii(
        style.border_radius.clone(),
        rect.size,
    )) || !style.corner_shapes.all_round()
    {
        return false;
    }

    let borders = used_border(style);
    let sides = [borders.top, borders.right, borders.bottom, borders.left];
    if sides.iter().all(|side| !side.is_visible()) {
        return true;
    }
    let [top, right, bottom, left] = sides;
    if ![top, right, bottom, left]
        .iter()
        .all(|side| side.is_visible() && side.style == BorderStyle::Solid)
    {
        return false;
    }
    if !same_width(top.used_width.get(), right.used_width.get())
        || !same_width(top.used_width.get(), bottom.used_width.get())
        || !same_width(top.used_width.get(), left.used_width.get())
        || top.color != right.color
        || top.color != bottom.color
        || top.color != left.color
    {
        return false;
    }

    let border_width = top
        .used_width
        .get()
        .min(rect.size.width)
        .min(rect.size.height);
    if border_width <= 0.0 {
        return true;
    }

    let inset = border_width / 2.0;
    let radii = padding_edge_rounded_rect_radii(
        used_rounded_rect_radii(style.border_radius.clone(), rect.size),
        css::Edges {
            top: inset,
            right: inset,
            bottom: inset,
            left: inset,
        },
    );
    rounded_rects.push(RenderedRoundedRect::from_paint_rect(
        paint_space_rect(
            rect.origin.x + inset,
            rect.origin.y + inset,
            rect.size.width - border_width,
            rect.size.height - border_width,
        ),
        radii,
        None,
        Some(top.color),
        PaintStrokeWidth::new(border_width),
    ));
    true
}

/// Paint a uniform rounded `double` border as two filled rounded border rings.
///
/// CSS Backgrounds and Borders Level 3 defines `double` as two lines whose
/// total line plus gap width equals `border-width`; it does not require exact
/// proportions. This follows the existing straight-border model by splitting
/// the width into thirds and painting the outer and inner thirds as vector
/// rings:
/// <https://www.w3.org/TR/css-backgrounds-3/#valdef-border-style-double>.
pub(crate) fn paint_uniform_double_rounded_border(
    paths: &mut Vec<RenderedPath>,
    rect: PaintRect,
    style: &ComputedStyle,
) -> bool {
    if rounded_radii_are_zero(used_rounded_rect_radii(
        style.border_radius.clone(),
        rect.size,
    )) {
        return false;
    }

    let borders = used_border(style);
    let sides = [borders.top, borders.right, borders.bottom, borders.left];
    if sides.iter().all(|side| !side.is_visible()) {
        return true;
    }
    let [top, right, bottom, left] = sides;
    if ![top, right, bottom, left]
        .iter()
        .all(|side| side.is_visible() && side.style == BorderStyle::Double)
    {
        return false;
    }
    if !same_width(top.used_width.get(), right.used_width.get())
        || !same_width(top.used_width.get(), bottom.used_width.get())
        || !same_width(top.used_width.get(), left.used_width.get())
        || top.color != right.color
        || top.color != bottom.color
        || top.color != left.color
    {
        return false;
    }

    let border_width = top
        .used_width
        .get()
        .min(rect.size.width)
        .min(rect.size.height);
    if border_width <= 0.0 || !top.color.is_visible() {
        return true;
    }
    let Some(bands) = DoubleBorderBands::for_used_width(layout_pt(border_width)) else {
        let outer_radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
        paths.push(uniform_rounded_ring_path(
            rect,
            outer_radii,
            border_width,
            top.color,
        ));
        return true;
    };

    let stripe = bands.stripe.get();
    let outer_radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
    paths.push(uniform_rounded_ring_path(
        rect,
        outer_radii,
        stripe,
        top.color,
    ));

    let inner_outer_inset = border_width - stripe;
    let inner_rect = inset_paint_rect(
        rect,
        css::Edges {
            top: inner_outer_inset,
            right: inner_outer_inset,
            bottom: inner_outer_inset,
            left: inner_outer_inset,
        },
    );
    if inner_rect.size.width > 0.0 && inner_rect.size.height > 0.0 {
        let inner_outer_radii = padding_edge_rounded_rect_radii(
            outer_radii,
            css::Edges {
                top: inner_outer_inset,
                right: inner_outer_inset,
                bottom: inner_outer_inset,
                left: inner_outer_inset,
            },
        );
        paths.push(uniform_rounded_ring_path(
            inner_rect,
            inner_outer_radii,
            stripe,
            top.color,
        ));
    }

    true
}

/// Paint a same-color solid rounded border ring with independent side widths.
///
/// CSS Backgrounds and Borders Level 3 defines the border painting area as the
/// region between the outer border edge and the inner padding edge. For rounded
/// borders, the inner corner radii are the outer radii reduced by the adjacent
/// border widths and clamped at zero:
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-shaping>.
pub(crate) fn paint_solid_rounded_border_ring(
    paths: &mut Vec<RenderedPath>,
    rect: PaintRect,
    style: &ComputedStyle,
) -> bool {
    if rounded_radii_are_zero(used_rounded_rect_radii(
        style.border_radius.clone(),
        rect.size,
    )) {
        return false;
    }

    let borders = used_border(style);
    let sides = [borders.top, borders.right, borders.bottom, borders.left];
    if sides.iter().all(|side| !side.is_visible()) {
        return true;
    }
    let [top, right, bottom, left] = sides;
    if ![top, right, bottom, left]
        .iter()
        .all(|side| side.is_visible() && side.style == BorderStyle::Solid)
    {
        return false;
    }
    if top.color != right.color || top.color != bottom.color || top.color != left.color {
        return false;
    }

    let inner_width = (rect.size.width - left.used_width.get() - right.used_width.get()).max(0.0);
    let inner_height = (rect.size.height - top.used_width.get() - bottom.used_width.get()).max(0.0);
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 || !top.color.is_visible() {
        return true;
    }

    let outer_radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
    let inner_radii = RenderedRoundedRectRadii {
        top_left: RenderedCornerRadius::new(
            outer_radii.top_left.x() - left.used_width.get(),
            outer_radii.top_left.y() - top.used_width.get(),
        ),
        top_right: RenderedCornerRadius::new(
            outer_radii.top_right.x() - right.used_width.get(),
            outer_radii.top_right.y() - top.used_width.get(),
        ),
        bottom_right: RenderedCornerRadius::new(
            outer_radii.bottom_right.x() - right.used_width.get(),
            outer_radii.bottom_right.y() - bottom.used_width.get(),
        ),
        bottom_left: RenderedCornerRadius::new(
            outer_radii.bottom_left.x() - left.used_width.get(),
            outer_radii.bottom_left.y() - bottom.used_width.get(),
        ),
    };

    let mut commands = shaped_rect_path_commands(rect, outer_radii, style.corner_shapes);
    if inner_width > 0.0 && inner_height > 0.0 {
        let inner_rect = inset_paint_rect(
            rect,
            css::Edges {
                top: top.used_width.get(),
                right: right.used_width.get(),
                bottom: bottom.used_width.get(),
                left: left.used_width.get(),
            },
        );
        commands.extend(shaped_rect_path_commands(
            inner_rect,
            inner_radii,
            style.corner_shapes,
        ));
    }
    paths.push(RenderedPath::new(
        commands,
        Some(top.color),
        RenderedPathFillRule::EvenOdd,
        None,
        PaintStrokeWidth::ZERO,
        None,
    ));
    true
}

/// Paint rounded dashed and dotted borders through clipped path segments.
///
/// CSS Backgrounds and Borders defines dashed and dotted border styles but
/// intentionally leaves exact dash placement flexible. This reuses the
/// straight-edge WeasyPrint-compatible dash/dot distribution, represents every
/// segment as a PDF path, and clips the result to the intersection of the
/// rounded border ring and the side transition region:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-style>.
pub(crate) fn paint_patterned_rounded_border_sides(
    paths: &mut Vec<RenderedPath>,
    rect: PaintRect,
    style: &ComputedStyle,
) -> bool {
    if rounded_radii_are_zero(used_rounded_rect_radii(
        style.border_radius.clone(),
        rect.size,
    )) {
        return false;
    }

    let borders = used_border(style);
    let sides = [
        (BorderEdge::Bottom, borders.bottom),
        (BorderEdge::Left, borders.left),
        (BorderEdge::Right, borders.right),
        (BorderEdge::Top, borders.top),
    ];
    if sides.iter().all(|(_, side)| !border_side_has_area(*side)) {
        return true;
    }
    if sides
        .iter()
        .any(|(_, side)| border_side_has_area(*side) && !is_patterned_side_style(side.style))
    {
        return false;
    }
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 {
        return true;
    }

    for (edge, side) in sides {
        if !side.is_visible() {
            continue;
        }
        let clip = rounded_border_pattern_clip(edge, rect, style, borders);
        let geometry = border_side_geometry(
            edge,
            PageTopRect::new(
                rect.origin.x,
                rect.max_y(),
                rect.size.width,
                rect.size.height,
            ),
            side.used_width.get(),
        );
        let axis_start = geometry.axis_start();
        let axis_length = geometry.axis_length();
        let cross_start = geometry.cross_start();
        let cross_width = geometry.cross_width();
        let horizontal = geometry.horizontal;
        match side.style {
            BorderStyle::Dotted => paint_dotted_border_side_with_clip(
                paths,
                axis_start,
                axis_length,
                cross_start,
                cross_width,
                horizontal,
                side.color,
                Some(clip),
            ),
            BorderStyle::Dashed => paint_dashed_border_side_with_clip(
                paths,
                axis_start,
                axis_length,
                cross_start,
                cross_width,
                horizontal,
                side.used_width.get(),
                side.color,
                Some(clip),
            ),
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(
        color: CssColor,
        position: Option<css::ComputedLengthPercentage>,
    ) -> css::GradientColorStop {
        css::GradientColorStop {
            color: css::GradientColor::CssColor(color),
            position,
        }
    }

    fn gradient(stops: Vec<css::GradientColorStop>) -> css::LinearGradient {
        css::LinearGradient {
            direction: LinearGradientDirection::Angle(180.0),
            interpolation: css::GradientInterpolationMethod::default(),
            repeating: false,
            stops,
            hints: Vec::new(),
        }
    }

    #[test]
    fn fixed_gradient_stops_default_and_distribute_omitted_positions() {
        let stops = fixed_gradient_stops(
            &gradient(vec![
                stop(CssColor::new(255, 0, 0), None),
                stop(CssColor::new(0, 128, 0), None),
                stop(
                    CssColor::new(0, 0, 255),
                    Some(css::ComputedLengthPercentage::from_percent(1.0)),
                ),
            ]),
            120.0,
        )
        .expect("gradient stops should fix up");

        assert_eq!(stops[0].position, 0.0);
        assert_eq!(stops[1].position, 60.0);
        assert_eq!(stops[2].position, 120.0);
    }

    #[test]
    fn fixed_gradient_stops_move_decreasing_positions_forward() {
        let stops = fixed_gradient_stops(
            &gradient(vec![
                stop(
                    CssColor::new(255, 0, 0),
                    Some(css::ComputedLengthPercentage::from_percent(0.75)),
                ),
                stop(
                    CssColor::new(0, 128, 0),
                    Some(css::ComputedLengthPercentage::from_percent(0.25)),
                ),
                stop(CssColor::new(0, 0, 255), None),
            ]),
            100.0,
        )
        .expect("gradient stops should fix up");

        assert_eq!(stops[0].position, 75.0);
        assert_eq!(stops[1].position, 75.0);
        assert_eq!(stops[2].position, 100.0);
    }

    #[test]
    fn axis_aligned_hard_stop_tiles_use_vector_bands() {
        let gradient = gradient(vec![
            stop(
                CssColor::new(255, 0, 0),
                Some(css::ComputedLengthPercentage::from_percent(0.5)),
            ),
            stop(
                CssColor::TRANSPARENT,
                Some(css::ComputedLengthPercentage::from_percent(0.5)),
            ),
        ]);
        let area = paint_space_rect(0.0, 0.0, 30.0, 60.0);

        assert!(
            linear_gradient_hard_stop_tile_paths(&gradient, area, area, None).is_some(),
            "axis-aligned hard stops must not fall back to raster sampling at a cell edge",
        );
    }

    #[test]
    fn non_axis_aligned_gradient_direction_projects_paint_displacements() {
        let line = AngledGradientLine {
            center: PaintPoint::new(10.0, 20.0),
            direction: PaintDirection::from_components(0.6, 0.8),
            axis_length: 20.0,
        };

        assert_eq!(
            gradient_axis_position(PaintPoint::new(13.0, 24.0), line),
            15.0
        );
        assert_eq!(
            line.endpoints(),
            (PaintPoint::new(4.0, 12.0), PaintPoint::new(16.0, 28.0))
        );
    }

    #[test]
    fn non_square_corner_gradient_directions_preserve_magic_corners() {
        let area = paint_space_rect(0.0, 0.0, 200.0, 100.0);
        let cases = [
            (
                LinearGradientDirection::Corner {
                    horizontal: css::GradientHorizontalDirection::Right,
                    vertical: css::GradientVerticalDirection::Bottom,
                },
                153.43495,
                [PaintPoint::new(0.0, 0.0), PaintPoint::new(200.0, 100.0)],
            ),
            (
                LinearGradientDirection::Corner {
                    horizontal: css::GradientHorizontalDirection::Left,
                    vertical: css::GradientVerticalDirection::Bottom,
                },
                206.56505,
                [PaintPoint::new(200.0, 0.0), PaintPoint::new(0.0, 100.0)],
            ),
            (
                LinearGradientDirection::Corner {
                    horizontal: css::GradientHorizontalDirection::Left,
                    vertical: css::GradientVerticalDirection::Top,
                },
                333.43494,
                [PaintPoint::new(0.0, 0.0), PaintPoint::new(200.0, 100.0)],
            ),
            (
                LinearGradientDirection::Corner {
                    horizontal: css::GradientHorizontalDirection::Right,
                    vertical: css::GradientVerticalDirection::Top,
                },
                26.565052,
                [PaintPoint::new(200.0, 0.0), PaintPoint::new(0.0, 100.0)],
            ),
        ];

        for (direction, expected_angle, neighboring_corners) in cases {
            let angle = gradient_direction_angle_for_area(direction, area);
            assert!(
                (angle - expected_angle).abs() < 0.001,
                "expected {expected_angle}deg, got {angle}deg"
            );

            let line = angled_gradient_line(direction, area);
            let midpoint = line.axis_length / 2.0;
            for corner in neighboring_corners {
                assert!(
                    (gradient_axis_position(corner, line) - midpoint).abs() < 0.001,
                    "50% stop must pass through {corner:?} for {direction:?}"
                );
            }
        }
    }

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
