use super::*;

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
