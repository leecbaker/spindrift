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
    rect: PaintRect,
    style: &ComputedStyle,
) -> bool {
    // Degenerate used radii are square corners, so they belong to the normal
    // straight-side painter instead of this clipped rounded-path painter.
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
        .any(|(_, side)| border_side_has_area(*side) && !is_clipped_rounded_side_style(side.style))
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
        let clip = Some(rounded_border_side_clip(edge, rect, borders));
        match side.style {
            BorderStyle::Inset | BorderStyle::Outset => paths.push(solid_rounded_border_ring_path(
                rect,
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
                    rect,
                    style,
                    outer_color,
                    clip.clone(),
                    css::Edges::ZERO,
                    half,
                ));
                paths.push(rounded_border_ring_between_path(
                    rect,
                    style,
                    inner_color,
                    clip,
                    half,
                    full,
                ));
            }
            BorderStyle::Double => {
                if DoubleBorderBands::for_used_width(side.used_width).is_none() {
                    paths.push(solid_rounded_border_ring_path(
                        rect, style, side.color, clip,
                    ));
                } else {
                    let stripe = double_stripe_insets(borders);
                    let inner_outer = double_inner_outer_insets(borders);
                    let full = border_insets(borders);
                    paths.push(rounded_border_ring_between_path(
                        rect,
                        style,
                        side.color,
                        clip.clone(),
                        css::Edges::ZERO,
                        stripe,
                    ));
                    paths.push(rounded_border_ring_between_path(
                        rect,
                        style,
                        side.color,
                        clip,
                        inner_outer,
                        full,
                    ));
                }
            }
            _ => paths.push(solid_rounded_border_ring_path(
                rect, style, side.color, clip,
            )),
        }
    }
    true
}

pub(in crate::layout) fn uniform_rounded_ring_path(
    rect: PaintRect,
    outer_radii: RenderedRoundedRectRadii,
    inset: f32,
    color: CssColor,
) -> RenderedPath {
    let inner = inset_paint_rect(
        rect,
        css::Edges {
            top: inset,
            right: inset,
            bottom: inset,
            left: inset,
        },
    );
    let mut commands = shaped_rect_path_commands(rect, outer_radii, css::CornerShapes::ROUND);
    if inner.size.width > 0.0 && inner.size.height > 0.0 {
        let mut inner_radii = outer_radii;
        inset_rounded_rect_radii(&mut inner_radii, inset);
        commands.extend(shaped_rect_path_commands(
            inner,
            inner_radii,
            css::CornerShapes::ROUND,
        ));
    }
    RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::EvenOdd,
        None,
        PaintStrokeWidth::ZERO,
        None,
    )
}

pub(in crate::layout) fn solid_rounded_border_ring_path(
    rect: PaintRect,
    style: &ComputedStyle,
    color: CssColor,
    clip: Option<RenderedPathClip>,
) -> RenderedPath {
    rounded_border_ring_between_path(
        rect,
        style,
        color,
        clip,
        css::Edges::ZERO,
        border_insets(used_border(style)),
    )
}

pub(in crate::layout) fn rounded_border_ring_between_path(
    rect: PaintRect,
    style: &ComputedStyle,
    color: CssColor,
    clip: Option<RenderedPathClip>,
    outer_inset: css::Edges,
    inner_inset: css::Edges,
) -> RenderedPath {
    let mut commands = rounded_box_path_commands_for_insets(rect, style, outer_inset);
    let inner = inset_paint_rect(rect, inner_inset);
    if inner.size.width > 0.0 && inner.size.height > 0.0 {
        commands.extend(rounded_box_path_commands_for_insets(
            rect,
            style,
            inner_inset,
        ));
    }
    RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::EvenOdd,
        None,
        PaintStrokeWidth::ZERO,
        clip,
    )
}

pub(in crate::layout) fn rounded_box_path_commands_for_insets(
    rect: PaintRect,
    style: &ComputedStyle,
    inset: css::Edges,
) -> Vec<RenderedPathCommand> {
    let inset_rect = inset_paint_rect(rect, inset);
    let outer_radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
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
    shaped_rect_path_commands(inset_rect, radii, style.corner_shapes)
}

pub(in crate::layout) fn rounded_border_side_clip(
    edge: BorderEdge,
    rect: PaintRect,
    borders: UsedBorder,
) -> RenderedPathClip {
    let x0 = rect.origin.x;
    let x1 = rect.max_x();
    let y0 = rect.origin.y;
    let y1 = rect.max_y();
    let inner_left = x0 + borders.left.used_width.get();
    let inner_right = x1 - borders.right.used_width.get();
    let inner_bottom = y0 + borders.bottom.used_width.get();
    let inner_top = y1 - borders.top.used_width.get();
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
    rect: PaintRect,
    style: &ComputedStyle,
    borders: UsedBorder,
) -> RenderedPathClip {
    let mut clip = rounded_border_side_clip(edge, rect, borders);
    clip.additional_clips
        .push(rounded_border_ring_clip_path(rect, style, borders));
    clip
}

pub(in crate::layout) fn rounded_border_ring_clip_path(
    rect: PaintRect,
    style: &ComputedStyle,
    borders: UsedBorder,
) -> RenderedPathClipPath {
    let mut commands = rounded_box_path_commands_for_insets(rect, style, css::Edges::ZERO);
    commands.extend(rounded_box_path_commands_for_insets(
        rect,
        style,
        border_insets(borders),
    ));
    RenderedPathClipPath::new(commands, RenderedPathFillRule::EvenOdd)
}

pub(in crate::layout) fn border_side_has_area(side: UsedBorderSide) -> bool {
    side.used_width > layout_pt(0.0) && !side.style.suppresses_used_width()
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
        top: borders.top.used_width.get(),
        right: borders.right.used_width.get(),
        bottom: borders.bottom.used_width.get(),
        left: borders.left.used_width.get(),
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

pub(in crate::layout) fn double_stripe_width(border_width: LayoutLength) -> f32 {
    DoubleBorderBands::for_used_width(border_width)
        .expect("double border stripe requires a double-band width")
        .stripe
        .get()
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
