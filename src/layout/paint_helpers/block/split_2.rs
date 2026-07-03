use super::*;

/// Paint rounded single-ring borders with side-specific colors.
///
/// CSS Backgrounds and Borders defines the rounded border as one ring between
/// the outer border edge and the inner padding edge. When side colors differ,
/// each side is painted independently in an implementation-defined transition
/// zone; this follows WeasyPrint's clip-then-fill strategy using PDF clipping
/// paths while preserving one shared used-border geometry. `double` splits the
/// used width into two painted stripes, and `inset`, `outset`, `groove`, and
/// `ridge` use the side-dependent color adjustment defined for CSS 3D border
/// styles:
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-shaping>.
pub(crate) fn paint_clipped_rounded_border_sides(
    paths: &mut Vec<RenderedPath>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
) -> bool {
    if style.border_radius.is_zero() {
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
        .any(|(_, side)| border_side_has_area(*side) && !is_clipped_rounded_side_style(side.style))
    {
        return false;
    }
    if width <= 0.0 || height <= 0.0 {
        return true;
    }

    for (edge, side) in sides {
        if !side.is_visible() {
            continue;
        }
        let clip = Some(rounded_border_side_clip(edge, x, y, width, height, borders));
        match side.style {
            BorderStyle::Inset | BorderStyle::Outset => paths.push(solid_rounded_border_ring_path(
                x,
                y,
                width,
                height,
                style,
                inset_outset_border_color(side.style, edge, side.color),
                clip,
            )),
            BorderStyle::Groove | BorderStyle::Ridge => {
                let (outer_color, inner_color) =
                    groove_ridge_border_colors(side.style, edge, side.color);
                let half = scaled_border_insets(borders, 0.5);
                let full = border_insets(borders);
                paths.push(rounded_border_ring_between_path(
                    x,
                    y,
                    width,
                    height,
                    style,
                    outer_color,
                    clip.clone(),
                    css::Edges::ZERO,
                    half,
                ));
                paths.push(rounded_border_ring_between_path(
                    x,
                    y,
                    width,
                    height,
                    style,
                    inner_color,
                    clip,
                    half,
                    full,
                ));
            }
            BorderStyle::Double => {
                if side.used_width < 3.0 {
                    paths.push(solid_rounded_border_ring_path(
                        x, y, width, height, style, side.color, clip,
                    ));
                } else {
                    let stripe = double_stripe_insets(borders);
                    let inner_outer = double_inner_outer_insets(borders);
                    let full = border_insets(borders);
                    paths.push(rounded_border_ring_between_path(
                        x,
                        y,
                        width,
                        height,
                        style,
                        side.color,
                        clip.clone(),
                        css::Edges::ZERO,
                        stripe,
                    ));
                    paths.push(rounded_border_ring_between_path(
                        x,
                        y,
                        width,
                        height,
                        style,
                        side.color,
                        clip,
                        inner_outer,
                        full,
                    ));
                }
            }
            _ => paths.push(solid_rounded_border_ring_path(
                x, y, width, height, style, side.color, clip,
            )),
        }
    }
    true
}

pub(in crate::layout) fn uniform_rounded_ring_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    outer_radii: RenderedRoundedRectRadii,
    inset: f32,
    color: Color,
) -> RenderedPath {
    let inner_width = (width - 2.0 * inset).max(0.0);
    let inner_height = (height - 2.0 * inset).max(0.0);
    let mut commands =
        shaped_rect_path_commands(x, y, width, height, outer_radii, css::CornerShapes::ROUND);
    if inner_width > 0.0 && inner_height > 0.0 {
        let mut inner_radii = outer_radii;
        inset_rounded_rect_radii(&mut inner_radii, inset);
        commands.extend(shaped_rect_path_commands(
            x + inset,
            y + inset,
            inner_width,
            inner_height,
            inner_radii,
            css::CornerShapes::ROUND,
        ));
    }
    RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::EvenOdd,
        None,
        0.0,
        None,
    )
}

pub(in crate::layout) fn solid_rounded_border_ring_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    color: Color,
    clip: Option<RenderedPathClip>,
) -> RenderedPath {
    rounded_border_ring_between_path(
        x,
        y,
        width,
        height,
        style,
        color,
        clip,
        css::Edges::ZERO,
        border_insets(used_border(style)),
    )
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn rounded_border_ring_between_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    color: Color,
    clip: Option<RenderedPathClip>,
    outer_inset: css::Edges,
    inner_inset: css::Edges,
) -> RenderedPath {
    let mut commands =
        rounded_box_path_commands_for_insets(x, y, width, height, style, outer_inset);
    let inner_width = (width - inner_inset.left - inner_inset.right).max(0.0);
    let inner_height = (height - inner_inset.top - inner_inset.bottom).max(0.0);
    if inner_width > 0.0 && inner_height > 0.0 {
        commands.extend(rounded_box_path_commands_for_insets(
            x,
            y,
            width,
            height,
            style,
            inner_inset,
        ));
    }
    RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::EvenOdd,
        None,
        0.0,
        clip,
    )
}

pub(in crate::layout) fn rounded_box_path_commands_for_insets(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    inset: css::Edges,
) -> Vec<RenderedPathCommand> {
    let inset_width = (width - inset.left - inset.right).max(0.0);
    let inset_height = (height - inset.top - inset.bottom).max(0.0);
    let outer_radii = used_rounded_rect_radii(style.border_radius, width, height);
    let radii = RenderedRoundedRectRadii {
        top_left: RenderedCornerRadius::new(
            outer_radii.top_left.x() - inset.left,
            outer_radii.top_left.y() - inset.top,
        ),
        top_right: RenderedCornerRadius::new(
            outer_radii.top_right.x() - inset.right,
            outer_radii.top_right.y() - inset.top,
        ),
        bottom_right: RenderedCornerRadius::new(
            outer_radii.bottom_right.x() - inset.right,
            outer_radii.bottom_right.y() - inset.bottom,
        ),
        bottom_left: RenderedCornerRadius::new(
            outer_radii.bottom_left.x() - inset.left,
            outer_radii.bottom_left.y() - inset.bottom,
        ),
    };
    shaped_rect_path_commands(
        x + inset.left,
        y + inset.bottom,
        inset_width,
        inset_height,
        radii,
        style.corner_shapes,
    )
}

pub(in crate::layout) fn rounded_border_side_clip(
    edge: BorderEdge,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    borders: UsedBorder,
) -> RenderedPathClip {
    let x0 = x;
    let x1 = x + width;
    let y0 = y;
    let y1 = y + height;
    let inner_left = x0 + borders.left.used_width;
    let inner_right = x1 - borders.right.used_width;
    let inner_bottom = y0 + borders.bottom.used_width;
    let inner_top = y1 - borders.top.used_width;
    let points = match edge {
        BorderEdge::Top => [
            (x0, y1),
            (x1, y1),
            (inner_right, inner_top),
            (inner_left, inner_top),
        ],
        BorderEdge::Right => [
            (x1, y1),
            (x1, y0),
            (inner_right, inner_bottom),
            (inner_right, inner_top),
        ],
        BorderEdge::Bottom => [
            (x1, y0),
            (x0, y0),
            (inner_left, inner_bottom),
            (inner_right, inner_bottom),
        ],
        BorderEdge::Left => [
            (x0, y0),
            (x0, y1),
            (inner_left, inner_top),
            (inner_left, inner_bottom),
        ],
    };
    RenderedPathClip::new(
        vec![
            RenderedPathCommand::move_to(paint_tuple_point(points[0])),
            RenderedPathCommand::line_to(paint_tuple_point(points[1])),
            RenderedPathCommand::line_to(paint_tuple_point(points[2])),
            RenderedPathCommand::line_to(paint_tuple_point(points[3])),
            RenderedPathCommand::Close,
        ],
        RenderedPathFillRule::NonZero,
        Vec::new(),
    )
}

pub(in crate::layout) fn rounded_border_pattern_clip(
    edge: BorderEdge,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    borders: UsedBorder,
) -> RenderedPathClip {
    let mut clip = rounded_border_side_clip(edge, x, y, width, height, borders);
    clip.additional_clips.push(rounded_border_ring_clip_path(
        x, y, width, height, style, borders,
    ));
    clip
}

pub(in crate::layout) fn rounded_border_ring_clip_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    borders: UsedBorder,
) -> RenderedPathClipPath {
    let mut commands =
        rounded_box_path_commands_for_insets(x, y, width, height, style, css::Edges::ZERO);
    commands.extend(rounded_box_path_commands_for_insets(
        x,
        y,
        width,
        height,
        style,
        border_insets(borders),
    ));
    RenderedPathClipPath::new(commands, RenderedPathFillRule::EvenOdd)
}

pub(in crate::layout) fn border_side_has_area(side: UsedBorderSide) -> bool {
    side.used_width > 0.0 && !side.style.suppresses_used_width()
}

pub(in crate::layout) fn is_clipped_rounded_side_style(style: BorderStyle) -> bool {
    matches!(
        style,
        BorderStyle::Solid
            | BorderStyle::Inset
            | BorderStyle::Outset
            | BorderStyle::Groove
            | BorderStyle::Ridge
            | BorderStyle::Double
    )
}

pub(in crate::layout) fn is_patterned_side_style(style: BorderStyle) -> bool {
    matches!(style, BorderStyle::Dashed | BorderStyle::Dotted)
}

pub(in crate::layout) fn border_insets(borders: UsedBorder) -> css::Edges {
    css::Edges {
        top: borders.top.used_width,
        right: borders.right.used_width,
        bottom: borders.bottom.used_width,
        left: borders.left.used_width,
    }
}

pub(in crate::layout) fn scaled_border_insets(borders: UsedBorder, scale: f32) -> css::Edges {
    let mut insets = border_insets(borders);
    insets.top *= scale;
    insets.right *= scale;
    insets.bottom *= scale;
    insets.left *= scale;
    insets
}

pub(in crate::layout) fn double_stripe_insets(borders: UsedBorder) -> css::Edges {
    css::Edges {
        top: double_stripe_width(borders.top.used_width),
        right: double_stripe_width(borders.right.used_width),
        bottom: double_stripe_width(borders.bottom.used_width),
        left: double_stripe_width(borders.left.used_width),
    }
}

pub(in crate::layout) fn double_inner_outer_insets(borders: UsedBorder) -> css::Edges {
    let full = border_insets(borders);
    let stripe = double_stripe_insets(borders);
    css::Edges {
        top: (full.top - stripe.top).max(0.0),
        right: (full.right - stripe.right).max(0.0),
        bottom: (full.bottom - stripe.bottom).max(0.0),
        left: (full.left - stripe.left).max(0.0),
    }
}

pub(in crate::layout) fn double_stripe_width(border_width: f32) -> f32 {
    (border_width / 3.0).max(1.0)
}

/// Build a PDF-compatible rounded rectangle subpath for CSS border geometry.
///
/// CSS Backgrounds and Borders Level 3 uses quarter ellipses for rounded
/// corners; PDF paths approximate those arcs with cubic Bezier segments:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-radius>.
pub(crate) fn rounded_rect_path_commands(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radii: RenderedRoundedRectRadii,
) -> Vec<RenderedPathCommand> {
    const KAPPA: f32 = 0.552_284_8;
    let x0 = x;
    let y0 = y;
    let x1 = x + width;
    let y1 = y + height;
    let tl = radii.top_left;
    let tr = radii.top_right;
    let br = radii.bottom_right;
    let bl = radii.bottom_left;

    let mut commands = Vec::with_capacity(10);
    commands.push(RenderedPathCommand::move_to(paint_space_point(
        x0 + bl.x(),
        y0,
    )));
    commands.push(RenderedPathCommand::line_to(paint_space_point(
        x1 - br.x(),
        y0,
    )));
    if br.x() > 0.0 || br.y() > 0.0 {
        commands.push(RenderedPathCommand::curve_to(
            paint_space_point(x1 - br.x() + br.x() * KAPPA, y0),
            paint_space_point(x1, y0 + br.y() - br.y() * KAPPA),
            paint_space_point(x1, y0 + br.y()),
        ));
    }
    commands.push(RenderedPathCommand::line_to(paint_space_point(
        x1,
        y1 - tr.y(),
    )));
    if tr.x() > 0.0 || tr.y() > 0.0 {
        commands.push(RenderedPathCommand::curve_to(
            paint_space_point(x1, y1 - tr.y() + tr.y() * KAPPA),
            paint_space_point(x1 - tr.x() + tr.x() * KAPPA, y1),
            paint_space_point(x1 - tr.x(), y1),
        ));
    }
    commands.push(RenderedPathCommand::line_to(paint_space_point(
        x0 + tl.x(),
        y1,
    )));
    if tl.x() > 0.0 || tl.y() > 0.0 {
        commands.push(RenderedPathCommand::curve_to(
            paint_space_point(x0 + tl.x() - tl.x() * KAPPA, y1),
            paint_space_point(x0, y1 - tl.y() + tl.y() * KAPPA),
            paint_space_point(x0, y1 - tl.y()),
        ));
    }
    commands.push(RenderedPathCommand::line_to(paint_space_point(
        x0,
        y0 + bl.y(),
    )));
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

/// Build a border contour path for CSS Borders 4 shaped corners.
///
/// CSS Borders and Box Decorations Level 4 defines `corner-*-shape` as the
/// contour between the two radius tangent points. Keyword aliases map onto
/// superellipse parameters, and finite non-keyword values are represented by a
/// deterministic polyline approximation of the superellipse contour:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape> and
/// <https://drafts.csswg.org/css-borders-4/#corner-rendering>.
pub(crate) fn shaped_rect_path_commands(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radii: RenderedRoundedRectRadii,
    shapes: css::CornerShapes,
) -> Vec<RenderedPathCommand> {
    if shapes.all_round() {
        return rounded_rect_path_commands(x, y, width, height, radii);
    }

    let x0 = x;
    let y0 = y;
    let x1 = x + width;
    let y1 = y + height;
    let tl = radii.top_left;
    let tr = radii.top_right;
    let br = radii.bottom_right;
    let bl = radii.bottom_left;

    let mut commands = Vec::with_capacity(14);
    commands.push(RenderedPathCommand::move_to(paint_space_point(
        x0 + bl.x(),
        y0,
    )));
    commands.push(RenderedPathCommand::line_to(paint_space_point(
        x1 - br.x(),
        y0,
    )));
    append_corner_shape(
        &mut commands,
        shapes.bottom_right,
        (x1 - br.x(), y0),
        (x1, y0 + br.y()),
        (x1 - br.x(), y0 + br.y()),
        br,
        CornerPathKind::BottomRight,
    );
    commands.push(RenderedPathCommand::line_to(paint_space_point(
        x1,
        y1 - tr.y(),
    )));
    append_corner_shape(
        &mut commands,
        shapes.top_right,
        (x1, y1 - tr.y()),
        (x1 - tr.x(), y1),
        (x1 - tr.x(), y1 - tr.y()),
        tr,
        CornerPathKind::TopRight,
    );
    commands.push(RenderedPathCommand::line_to(paint_space_point(
        x0 + tl.x(),
        y1,
    )));
    append_corner_shape(
        &mut commands,
        shapes.top_left,
        (x0 + tl.x(), y1),
        (x0, y1 - tl.y()),
        (x0 + tl.x(), y1 - tl.y()),
        tl,
        CornerPathKind::TopLeft,
    );
    commands.push(RenderedPathCommand::line_to(paint_space_point(
        x0,
        y0 + bl.y(),
    )));
    append_corner_shape(
        &mut commands,
        shapes.bottom_left,
        (x0, y0 + bl.y()),
        (x0 + bl.x(), y0),
        (x0 + bl.x(), y0 + bl.y()),
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
    start: (f32, f32),
    end: (f32, f32),
    inner: (f32, f32),
    radius: RenderedCornerRadius,
    kind: CornerPathKind,
) {
    if radius.x() <= 0.0 && radius.y() <= 0.0 {
        commands.push(RenderedPathCommand::line_to(paint_tuple_point(end)));
        return;
    }
    match shape.superellipse {
        css::SuperellipseParameter::Infinity => {
            let outer = corner_outer_point(start, end, kind);
            commands.push(RenderedPathCommand::line_to(paint_tuple_point(outer)));
            commands.push(RenderedPathCommand::line_to(paint_tuple_point(end)));
        }
        css::SuperellipseParameter::NegativeInfinity => {
            commands.push(RenderedPathCommand::line_to(paint_tuple_point(inner)));
            commands.push(RenderedPathCommand::line_to(paint_tuple_point(end)));
        }
        css::SuperellipseParameter::Number(1.0) => {
            append_round_corner(commands, start, end, radius, kind);
        }
        css::SuperellipseParameter::Number(0.0) => {
            commands.push(RenderedPathCommand::line_to(paint_tuple_point(end)));
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
    start: (f32, f32),
    end: (f32, f32),
    radius: RenderedCornerRadius,
    kind: CornerPathKind,
) {
    const KAPPA: f32 = 0.552_284_8;
    let (c1, c2) = match kind {
        CornerPathKind::BottomRight => (
            (start.0, start.1 + radius.y() * KAPPA),
            (end.0 - radius.x() * KAPPA, end.1),
        ),
        CornerPathKind::TopRight => (
            (start.0 - radius.x() * KAPPA, start.1),
            (end.0, end.1 - radius.y() * KAPPA),
        ),
        CornerPathKind::TopLeft => (
            (start.0, start.1 - radius.y() * KAPPA),
            (end.0 + radius.x() * KAPPA, end.1),
        ),
        CornerPathKind::BottomLeft => (
            (start.0 + radius.x() * KAPPA, start.1),
            (end.0, end.1 + radius.y() * KAPPA),
        ),
    };
    commands.push(RenderedPathCommand::curve_to(
        paint_tuple_point(c1),
        paint_tuple_point(c2),
        paint_tuple_point(end),
    ));
}

fn append_sampled_superellipse_corner(
    commands: &mut Vec<RenderedPathCommand>,
    start: (f32, f32),
    end: (f32, f32),
    value: f32,
    kind: CornerPathKind,
) {
    const SEGMENTS: usize = 16;
    const EXTREME: f32 = 20.0;
    let outer = corner_outer_point(start, end, kind);
    if value >= EXTREME {
        commands.push(RenderedPathCommand::line_to(paint_tuple_point(outer)));
        commands.push(RenderedPathCommand::line_to(paint_tuple_point(end)));
        return;
    }
    if value <= -EXTREME {
        let inner = corner_inner_point(start, end, outer);
        commands.push(RenderedPathCommand::line_to(paint_tuple_point(inner)));
        commands.push(RenderedPathCommand::line_to(paint_tuple_point(end)));
        return;
    }

    for segment in 1..=SEGMENTS {
        let theta = std::f32::consts::FRAC_PI_2 * segment as f32 / SEGMENTS as f32;
        let (u, v) = sampled_superellipse_unit_point(theta, value);
        let point = corner_point_from_unit(start, end, outer, u, v);
        commands.push(RenderedPathCommand::line_to(paint_tuple_point(point)));
    }
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

fn corner_outer_point(start: (f32, f32), end: (f32, f32), kind: CornerPathKind) -> (f32, f32) {
    match kind {
        CornerPathKind::BottomRight => (end.0, start.1),
        CornerPathKind::TopRight => (start.0, end.1),
        CornerPathKind::TopLeft => (end.0, start.1),
        CornerPathKind::BottomLeft => (start.0, end.1),
    }
}

fn corner_inner_point(start: (f32, f32), end: (f32, f32), outer: (f32, f32)) -> (f32, f32) {
    (start.0 + end.0 - outer.0, start.1 + end.1 - outer.1)
}

fn corner_point_from_unit(
    start: (f32, f32),
    end: (f32, f32),
    outer: (f32, f32),
    u: f32,
    v: f32,
) -> (f32, f32) {
    (
        outer.0 + (start.0 - outer.0) * u + (end.0 - outer.0) * v,
        outer.1 + (start.1 - outer.1) * u + (end.1 - outer.1) * v,
    )
}

pub(in crate::layout) fn append_round_corner(
    commands: &mut Vec<RenderedPathCommand>,
    start: (f32, f32),
    end: (f32, f32),
    radius: RenderedCornerRadius,
    kind: CornerPathKind,
) {
    const KAPPA: f32 = 0.552_284_8;
    let (c1, c2) = match kind {
        CornerPathKind::BottomRight => (
            (start.0 + radius.x() * KAPPA, start.1),
            (end.0, end.1 - radius.y() * KAPPA),
        ),
        CornerPathKind::TopRight => (
            (start.0, start.1 + radius.y() * KAPPA),
            (end.0 + radius.x() * KAPPA, end.1),
        ),
        CornerPathKind::TopLeft => (
            (start.0 - radius.x() * KAPPA, start.1),
            (end.0, end.1 + radius.y() * KAPPA),
        ),
        CornerPathKind::BottomLeft => (
            (start.0, start.1 - radius.y() * KAPPA),
            (end.0 - radius.x() * KAPPA, end.1),
        ),
    };
    commands.push(RenderedPathCommand::curve_to(
        paint_tuple_point(c1),
        paint_tuple_point(c2),
        paint_tuple_point(end),
    ));
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
pub(crate) fn used_rounded_rect_radii(
    radius: css::BorderRadius,
    width: f32,
    height: f32,
) -> RenderedRoundedRectRadii {
    let mut radii = RenderedRoundedRectRadii {
        top_left: RenderedCornerRadius::new(
            radius.top_left.x.resolve(width),
            radius.top_left.y.resolve(height),
        ),
        top_right: RenderedCornerRadius::new(
            radius.top_right.x.resolve(width),
            radius.top_right.y.resolve(height),
        ),
        bottom_right: RenderedCornerRadius::new(
            radius.bottom_right.x.resolve(width),
            radius.bottom_right.y.resolve(height),
        ),
        bottom_left: RenderedCornerRadius::new(
            radius.bottom_left.x.resolve(width),
            radius.bottom_left.y.resolve(height),
        ),
    };
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

    #[test]
    fn notch_corner_path_visits_inner_corner() {
        let commands = shaped_rect_path_commands(
            0.0,
            0.0,
            10.0,
            10.0,
            test_radii(),
            top_left_shape(css::CornerShape::NOTCH),
        );

        assert!(commands.contains(&RenderedPathCommand::line_to(paint_space_point(2.0, 8.0))));
    }

    #[test]
    fn negative_infinite_superellipse_matches_notch_path() {
        let notch = shaped_rect_path_commands(
            0.0,
            0.0,
            10.0,
            10.0,
            test_radii(),
            top_left_shape(css::CornerShape::NOTCH),
        );
        let superellipse = shaped_rect_path_commands(
            0.0,
            0.0,
            10.0,
            10.0,
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
            0.0,
            0.0,
            10.0,
            10.0,
            test_radii(),
            top_left_shape(css::CornerShape::SQUARE),
        );
        let superellipse = shaped_rect_path_commands(
            0.0,
            0.0,
            10.0,
            10.0,
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
            shaped_rect_path_commands(0.0, 0.0, 10.0, 10.0, radii, css::CornerShapes::ROUND),
            rounded_rect_path_commands(0.0, 0.0, 10.0, 10.0, radii)
        );
    }
}
