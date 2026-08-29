use super::*;

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

pub(super) fn adjusted_outset_rounded_rect_radii(
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
pub(super) fn padding_edge_rounded_rect_radii(
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

pub(super) fn clip_gradient_rect(rect: &mut RenderedRect, clip: PaintRect) {
    rect.set_paint_rect(intersect_paint_rect_or_empty(rect.paint_rect(), clip));
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_gradient_band(
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
