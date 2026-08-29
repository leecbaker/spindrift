use super::*;

/// Build a PDF-compatible rounded rectangle subpath for CSS border geometry.
///
/// CSS Backgrounds and Borders Level 3 uses quarter ellipses for rounded
/// corners; PDF paths approximate those arcs with cubic Bezier segments:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-radius>.
pub(crate) fn rounded_rect_path_commands(
    rect: PaintRect,
    radii: RenderedRoundedRectRadii,
) -> Vec<RenderedPathCommand> {
    const KAPPA: f32 = 0.552_284_8;
    let x0 = rect.origin.x;
    let y0 = rect.origin.y;
    let x1 = rect.max_x();
    let y1 = rect.max_y();
    let tl = radii.top_left;
    let tr = radii.top_right;
    let br = radii.bottom_right;
    let bl = radii.bottom_left;

    let mut commands = Vec::with_capacity(10);
    commands.push(RenderedPathCommand::move_to(paint_space_point(
        x0 + bl.x(),
        y0,
    )));
    push_line_to_if_distinct(&mut commands, paint_space_point(x1 - br.x(), y0));
    if br.x() > 0.0 || br.y() > 0.0 {
        commands.push(RenderedPathCommand::curve_to(
            paint_space_point(x1 - br.x() + br.x() * KAPPA, y0),
            paint_space_point(x1, y0 + br.y() - br.y() * KAPPA),
            paint_space_point(x1, y0 + br.y()),
        ));
    }
    push_line_to_if_distinct(&mut commands, paint_space_point(x1, y1 - tr.y()));
    if tr.x() > 0.0 || tr.y() > 0.0 {
        commands.push(RenderedPathCommand::curve_to(
            paint_space_point(x1, y1 - tr.y() + tr.y() * KAPPA),
            paint_space_point(x1 - tr.x() + tr.x() * KAPPA, y1),
            paint_space_point(x1 - tr.x(), y1),
        ));
    }
    push_line_to_if_distinct(&mut commands, paint_space_point(x0 + tl.x(), y1));
    if tl.x() > 0.0 || tl.y() > 0.0 {
        commands.push(RenderedPathCommand::curve_to(
            paint_space_point(x0 + tl.x() - tl.x() * KAPPA, y1),
            paint_space_point(x0, y1 - tl.y() + tl.y() * KAPPA),
            paint_space_point(x0, y1 - tl.y()),
        ));
    }
    push_line_to_if_distinct(&mut commands, paint_space_point(x0, y0 + bl.y()));
    if bl.x() > 0.0 || bl.y() > 0.0 {
        commands.push(RenderedPathCommand::curve_to(
            paint_space_point(x0, y0 + bl.y() - bl.y() * KAPPA),
            paint_space_point(x0 + bl.x() - bl.x() * KAPPA, y0),
            paint_space_point(x0 + bl.x(), y0),
        ));
    }
    commands.push(RenderedPathCommand::Close);
    commands
}

/// Append a straight segment unless the current subpath is already at its end.
///
/// Fully saturated adjacent corner radii meet at a shared tangent point.  A
/// path must not add a zero-length side there: although it does not change the
/// mathematical contour, PDF viewers can rasterize an extra endpoint coverage
/// sample when the path is used as a clip.
fn push_line_to_if_distinct(commands: &mut Vec<RenderedPathCommand>, point: PaintPoint) {
    let current = commands.last().and_then(|command| match *command {
        RenderedPathCommand::MoveTo(point) | RenderedPathCommand::LineTo(point) => Some(point),
        RenderedPathCommand::CurveTo { end, .. } => Some(end),
        RenderedPathCommand::Close => None,
    });
    if current != Some(point) {
        commands.push(RenderedPathCommand::line_to(point));
    }
}

/// Build a border contour path for CSS Borders 4 shaped corners.
///
/// CSS Borders and Box Decorations Level 4 defines `corner-*-shape` as the
/// contour between the two radius tangent points. Keyword aliases map onto
/// superellipse parameters, and finite non-keyword values are represented by a
/// deterministic polyline approximation of the superellipse contour:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape> and
/// <https://drafts.csswg.org/css-borders-4/#corner-rendering>.
pub(crate) fn shaped_rect_path_commands(
    rect: PaintRect,
    radii: RenderedRoundedRectRadii,
    shapes: css::CornerShapes,
) -> Vec<RenderedPathCommand> {
    if shapes.all_round() {
        return rounded_rect_path_commands(rect, radii);
    }

    let x0 = rect.origin.x;
    let y0 = rect.origin.y;
    let x1 = rect.max_x();
    let y1 = rect.max_y();
    let tl = radii.top_left;
    let tr = radii.top_right;
    let br = radii.bottom_right;
    let bl = radii.bottom_left;

    let mut commands = Vec::with_capacity(14);
    commands.push(RenderedPathCommand::move_to(paint_space_point(
        x0 + bl.x(),
        y0,
    )));
    push_line_to_if_distinct(&mut commands, paint_space_point(x1 - br.x(), y0));
    append_corner_shape(
        &mut commands,
        shapes.bottom_right,
        paint_space_point(x1 - br.x(), y0),
        paint_space_point(x1, y0 + br.y()),
        paint_space_point(x1 - br.x(), y0 + br.y()),
        br,
        CornerPathKind::BottomRight,
    );
    push_line_to_if_distinct(&mut commands, paint_space_point(x1, y1 - tr.y()));
    append_corner_shape(
        &mut commands,
        shapes.top_right,
        paint_space_point(x1, y1 - tr.y()),
        paint_space_point(x1 - tr.x(), y1),
        paint_space_point(x1 - tr.x(), y1 - tr.y()),
        tr,
        CornerPathKind::TopRight,
    );
    push_line_to_if_distinct(&mut commands, paint_space_point(x0 + tl.x(), y1));
    append_corner_shape(
        &mut commands,
        shapes.top_left,
        paint_space_point(x0 + tl.x(), y1),
        paint_space_point(x0, y1 - tl.y()),
        paint_space_point(x0 + tl.x(), y1 - tl.y()),
        tl,
        CornerPathKind::TopLeft,
    );
    push_line_to_if_distinct(&mut commands, paint_space_point(x0, y0 + bl.y()));
    append_corner_shape(
        &mut commands,
        shapes.bottom_left,
        paint_space_point(x0, y0 + bl.y()),
        paint_space_point(x0 + bl.x(), y0),
        paint_space_point(x0 + bl.x(), y0 + bl.y()),
        bl,
        CornerPathKind::BottomLeft,
    );
    commands.push(RenderedPathCommand::Close);
    commands
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum CornerPathKind {
    BottomRight,
    TopRight,
    TopLeft,
    BottomLeft,
}

pub(in crate::layout) fn append_corner_shape(
    commands: &mut Vec<RenderedPathCommand>,
    shape: css::CornerShape,
    start: PaintPoint,
    end: PaintPoint,
    inner: PaintPoint,
    radius: RenderedCornerRadius,
    kind: CornerPathKind,
) {
    if radius.x() <= 0.0 && radius.y() <= 0.0 {
        commands.push(RenderedPathCommand::line_to(end));
        return;
    }
    match shape.superellipse {
        css::SuperellipseParameter::Infinity => {
            let outer = corner_outer_point(start, end, kind);
            commands.push(RenderedPathCommand::line_to(outer));
            commands.push(RenderedPathCommand::line_to(end));
        }
        css::SuperellipseParameter::NegativeInfinity => {
            commands.push(RenderedPathCommand::line_to(inner));
            commands.push(RenderedPathCommand::line_to(end));
        }
        css::SuperellipseParameter::Number(1.0) => {
            append_round_corner(commands, start, end, radius, kind);
        }
        css::SuperellipseParameter::Number(0.0) => {
            commands.push(RenderedPathCommand::line_to(end));
        }
        css::SuperellipseParameter::Number(-1.0) => {
            append_scoop_corner(commands, start, end, radius, kind);
        }
        css::SuperellipseParameter::Number(value) => {
            append_sampled_superellipse_corner(commands, start, end, value, kind);
        }
    }
}

pub(in crate::layout) fn append_scoop_corner(
    commands: &mut Vec<RenderedPathCommand>,
    start: PaintPoint,
    end: PaintPoint,
    radius: RenderedCornerRadius,
    kind: CornerPathKind,
) {
    const KAPPA: f32 = 0.552_284_8;
    let (c1, c2) = match kind {
        CornerPathKind::BottomRight => (
            paint_space_point(start.x, start.y + radius.y() * KAPPA),
            paint_space_point(end.x - radius.x() * KAPPA, end.y),
        ),
        CornerPathKind::TopRight => (
            paint_space_point(start.x - radius.x() * KAPPA, start.y),
            paint_space_point(end.x, end.y - radius.y() * KAPPA),
        ),
        CornerPathKind::TopLeft => (
            paint_space_point(start.x, start.y - radius.y() * KAPPA),
            paint_space_point(end.x + radius.x() * KAPPA, end.y),
        ),
        CornerPathKind::BottomLeft => (
            paint_space_point(start.x + radius.x() * KAPPA, start.y),
            paint_space_point(end.x, end.y + radius.y() * KAPPA),
        ),
    };
    commands.push(RenderedPathCommand::curve_to(c1, c2, end));
}

fn append_sampled_superellipse_corner(
    commands: &mut Vec<RenderedPathCommand>,
    start: PaintPoint,
    end: PaintPoint,
    value: f32,
    kind: CornerPathKind,
) {
    const SEGMENTS: usize = 16;
    const EXTREME: f32 = 20.0;
    let outer = corner_outer_point(start, end, kind);
    if value >= EXTREME {
        commands.push(RenderedPathCommand::line_to(outer));
        commands.push(RenderedPathCommand::line_to(end));
        return;
    }
    if value <= -EXTREME {
        let inner = corner_inner_point(start, end, outer);
        commands.push(RenderedPathCommand::line_to(inner));
        commands.push(RenderedPathCommand::line_to(end));
        return;
    }

    commands.extend((1..=SEGMENTS).map(|segment| {
        let theta = std::f32::consts::FRAC_PI_2 * segment as f32 / SEGMENTS as f32;
        let (u, v) = sampled_superellipse_unit_point(theta, value);
        RenderedPathCommand::line_to(corner_point_from_unit(start, end, outer, u, v))
    }));
}

fn sampled_superellipse_unit_point(theta: f32, value: f32) -> (f32, f32) {
    if value > 0.0 {
        let exponent = 2.0_f32.powf(value);
        let x = theta.cos().max(0.0).powf(2.0 / exponent);
        let y = theta.sin().max(0.0).powf(2.0 / exponent);
        (1.0 - y, 1.0 - x)
    } else {
        let exponent = 2.0_f32.powf(-value);
        let x = theta.cos().max(0.0).powf(2.0 / exponent);
        let y = theta.sin().max(0.0).powf(2.0 / exponent);
        (x, y)
    }
}

fn corner_outer_point(start: PaintPoint, end: PaintPoint, kind: CornerPathKind) -> PaintPoint {
    match kind {
        CornerPathKind::BottomRight => paint_space_point(end.x, start.y),
        CornerPathKind::TopRight => paint_space_point(start.x, end.y),
        CornerPathKind::TopLeft => paint_space_point(end.x, start.y),
        CornerPathKind::BottomLeft => paint_space_point(start.x, end.y),
    }
}

fn corner_inner_point(start: PaintPoint, end: PaintPoint, outer: PaintPoint) -> PaintPoint {
    paint_space_point(start.x + end.x - outer.x, start.y + end.y - outer.y)
}

fn corner_point_from_unit(
    start: PaintPoint,
    end: PaintPoint,
    outer: PaintPoint,
    u: f32,
    v: f32,
) -> PaintPoint {
    paint_space_point(
        outer.x + (start.x - outer.x) * u + (end.x - outer.x) * v,
        outer.y + (start.y - outer.y) * u + (end.y - outer.y) * v,
    )
}

pub(in crate::layout) fn append_round_corner(
    commands: &mut Vec<RenderedPathCommand>,
    start: PaintPoint,
    end: PaintPoint,
    radius: RenderedCornerRadius,
    kind: CornerPathKind,
) {
    const KAPPA: f32 = 0.552_284_8;
    let (c1, c2) = match kind {
        CornerPathKind::BottomRight => (
            paint_space_point(start.x + radius.x() * KAPPA, start.y),
            paint_space_point(end.x, end.y - radius.y() * KAPPA),
        ),
        CornerPathKind::TopRight => (
            paint_space_point(start.x, start.y + radius.y() * KAPPA),
            paint_space_point(end.x + radius.x() * KAPPA, end.y),
        ),
        CornerPathKind::TopLeft => (
            paint_space_point(start.x - radius.x() * KAPPA, start.y),
            paint_space_point(end.x, end.y + radius.y() * KAPPA),
        ),
        CornerPathKind::BottomLeft => (
            paint_space_point(start.x, start.y - radius.y() * KAPPA),
            paint_space_point(end.x - radius.x() * KAPPA, end.y),
        ),
    };
    commands.push(RenderedPathCommand::curve_to(c1, c2, end));
}

pub(in crate::layout) fn paint_tuple_point(point: (f32, f32)) -> PaintPoint {
    paint_space_point(point.0, point.1)
}

pub(in crate::layout) fn same_width(left: f32, right: f32) -> bool {
    (left - right).abs() < 0.01
}

pub(in crate::layout) fn inset_rounded_rect_radii(
    radii: &mut RenderedRoundedRectRadii,
    inset: f32,
) {
    radii.top_left.inset(inset);
    radii.top_right.inset(inset);
    radii.bottom_right.inset(inset);
    radii.bottom_left.inset(inset);
}

/// Resolve border-radius used values for a border box.
///
/// CSS Backgrounds and Borders Level 3 §5.1 defines percent resolution and the
/// proportional reduction used when corner curves overlap along an edge.
pub(crate) fn used_rounded_rect_radii<Space>(
    radius: css::BorderRadius,
    size: euclid::Size2D<f32, Space>,
) -> RenderedRoundedRectRadii {
    let width = size.width;
    let height = size.height;
    let mut radii = RenderedRoundedRectRadii {
        top_left: RenderedCornerRadius::new(
            radius
                .top_left
                .horizontal
                .resolve(PercentageBasis::definite(layout_pt(width)))
                .points(),
            radius
                .top_left
                .vertical
                .resolve(PercentageBasis::definite(layout_pt(height)))
                .points(),
        ),
        top_right: RenderedCornerRadius::new(
            radius
                .top_right
                .horizontal
                .resolve(PercentageBasis::definite(layout_pt(width)))
                .points(),
            radius
                .top_right
                .vertical
                .resolve(PercentageBasis::definite(layout_pt(height)))
                .points(),
        ),
        bottom_right: RenderedCornerRadius::new(
            radius
                .bottom_right
                .horizontal
                .resolve(PercentageBasis::definite(layout_pt(width)))
                .points(),
            radius
                .bottom_right
                .vertical
                .resolve(PercentageBasis::definite(layout_pt(height)))
                .points(),
        ),
        bottom_left: RenderedCornerRadius::new(
            radius
                .bottom_left
                .horizontal
                .resolve(PercentageBasis::definite(layout_pt(width)))
                .points(),
            radius
                .bottom_left
                .vertical
                .resolve(PercentageBasis::definite(layout_pt(height)))
                .points(),
        ),
    };
    // A radius with either zero component is a square corner. Keeping its
    // nonzero companion component would ask the path backend to approximate a
    // degenerate ellipse and can produce antialiased slivers at what CSS
    // defines as a rectangular corner.
    // <https://drafts.csswg.org/css-backgrounds-3/#corner-shaping>
    for corner in [
        &mut radii.top_left,
        &mut radii.top_right,
        &mut radii.bottom_right,
        &mut radii.bottom_left,
    ] {
        if corner.x() == 0.0 || corner.y() == 0.0 {
            *corner = RenderedCornerRadius::ZERO;
        }
    }
    let scale = [
        edge_radius_scale(width, radii.top_left.x() + radii.top_right.x()),
        edge_radius_scale(height, radii.top_right.y() + radii.bottom_right.y()),
        edge_radius_scale(width, radii.bottom_left.x() + radii.bottom_right.x()),
        edge_radius_scale(height, radii.top_left.y() + radii.bottom_left.y()),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);
    if scale < 1.0 {
        radii.top_left.scale(scale);
        radii.top_right.scale(scale);
        radii.bottom_right.scale(scale);
        radii.bottom_left.scale(scale);
    }
    radii
}

pub(crate) fn edge_radius_scale(edge_length: f32, radius_sum: f32) -> f32 {
    if radius_sum <= 0.0 {
        1.0
    } else {
        (edge_length / radius_sum).min(1.0)
    }
}

pub(in crate::layout) fn rounded_radii_are_zero(radii: RenderedRoundedRectRadii) -> bool {
    [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ]
    .into_iter()
    .all(|radius| radius.x() <= 0.0 && radius.y() <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_radii() -> RenderedRoundedRectRadii {
        let radius = RenderedCornerRadius::new(2.0, 2.0);
        RenderedRoundedRectRadii {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    fn top_left_shape(shape: css::CornerShape) -> css::CornerShapes {
        css::CornerShapes {
            top_left: shape,
            top_right: css::CornerShape::ROUND,
            bottom_right: css::CornerShape::ROUND,
            bottom_left: css::CornerShape::ROUND,
        }
    }

    fn test_rect() -> PaintRect {
        paint_space_rect(0.0, 0.0, 10.0, 10.0)
    }

    #[test]
    fn notch_corner_path_visits_inner_corner() {
        let commands = shaped_rect_path_commands(
            test_rect(),
            test_radii(),
            top_left_shape(css::CornerShape::NOTCH),
        );

        assert!(commands.contains(&RenderedPathCommand::line_to(paint_space_point(2.0, 8.0))));
    }

    #[test]
    fn negative_infinite_superellipse_matches_notch_path() {
        let notch = shaped_rect_path_commands(
            test_rect(),
            test_radii(),
            top_left_shape(css::CornerShape::NOTCH),
        );
        let superellipse = shaped_rect_path_commands(
            test_rect(),
            test_radii(),
            top_left_shape(css::CornerShape::superellipse(
                css::SuperellipseParameter::NegativeInfinity,
            )),
        );

        assert_eq!(superellipse, notch);
    }

    #[test]
    fn positive_infinite_superellipse_matches_square_path() {
        let square = shaped_rect_path_commands(
            test_rect(),
            test_radii(),
            top_left_shape(css::CornerShape::SQUARE),
        );
        let superellipse = shaped_rect_path_commands(
            test_rect(),
            test_radii(),
            top_left_shape(css::CornerShape::superellipse(
                css::SuperellipseParameter::Infinity,
            )),
        );

        assert_eq!(superellipse, square);
    }

    #[test]
    fn all_round_shape_uses_existing_rounded_path() {
        let radii = test_radii();

        assert_eq!(
            shaped_rect_path_commands(test_rect(), radii, css::CornerShapes::ROUND),
            rounded_rect_path_commands(test_rect(), radii)
        );
    }

    #[test]
    fn zero_radius_component_produces_a_square_corner() {
        let degenerate = css::CornerRadius {
            horizontal: css::CornerRadiusComponent::ZERO,
            vertical: css::CornerRadiusComponent {
                value: css::ComputedLengthPercentage::from_points(5.0),
            },
        };
        let radius = css::BorderRadius {
            top_left: degenerate.clone(),
            top_right: degenerate.clone(),
            bottom_right: degenerate.clone(),
            bottom_left: degenerate,
        };

        let radii = used_rounded_rect_radii(radius, PaintSize::new(20.0, 20.0));

        assert_eq!(radii, RenderedRoundedRectRadii::ZERO);
    }

    #[test]
    fn fully_saturated_bevel_path_has_no_zero_length_sides() {
        let radius = RenderedCornerRadius::new(5.0, 5.0);
        let radii = RenderedRoundedRectRadii {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        };
        let bevel = css::CornerShapes {
            top_left: css::CornerShape::BEVEL,
            top_right: css::CornerShape::BEVEL,
            bottom_right: css::CornerShape::BEVEL,
            bottom_left: css::CornerShape::BEVEL,
        };

        assert_eq!(
            shaped_rect_path_commands(test_rect(), radii, bevel),
            vec![
                RenderedPathCommand::move_to(paint_space_point(5.0, 0.0)),
                RenderedPathCommand::line_to(paint_space_point(10.0, 5.0)),
                RenderedPathCommand::line_to(paint_space_point(5.0, 10.0)),
                RenderedPathCommand::line_to(paint_space_point(0.0, 5.0)),
                RenderedPathCommand::line_to(paint_space_point(5.0, 0.0)),
                RenderedPathCommand::Close,
            ]
        );
    }

    #[test]
    fn shaped_corner_path_preserves_nonzero_paint_rect_origin() {
        let commands = shaped_rect_path_commands(
            paint_space_rect(10.0, 20.0, 10.0, 10.0),
            test_radii(),
            top_left_shape(css::CornerShape::NOTCH),
        );

        assert!(commands.contains(&RenderedPathCommand::line_to(paint_space_point(12.0, 28.0))));
    }

    #[test]
    fn border_side_clip_uses_the_paint_rect_edges() {
        let side = UsedBorderSide::new(layout_pt(2.0), BorderStyle::Solid, CssColor::new(0, 0, 0));
        let borders = UsedBorder {
            top: side,
            right: side,
            bottom: side,
            left: side,
        };
        let clip = rounded_border_side_clip(
            BorderEdge::Top,
            paint_space_rect(10.0, 20.0, 30.0, 40.0),
            borders,
        );

        assert_eq!(
            clip.commands,
            vec![
                RenderedPathCommand::move_to(paint_space_point(10.0, 60.0)),
                RenderedPathCommand::line_to(paint_space_point(40.0, 60.0)),
                RenderedPathCommand::line_to(paint_space_point(38.0, 58.0)),
                RenderedPathCommand::line_to(paint_space_point(12.0, 58.0)),
                RenderedPathCommand::Close,
            ]
        );
    }

    #[test]
    fn uniform_ring_insets_a_nonzero_paint_rect() {
        let path = uniform_rounded_ring_path(
            paint_space_rect(10.0, 20.0, 30.0, 40.0),
            test_radii(),
            2.0,
            CssColor::new(0, 0, 0),
        );

        assert!(
            path.commands
                .contains(&RenderedPathCommand::move_to(paint_space_point(12.0, 22.0)))
        );
    }
}
