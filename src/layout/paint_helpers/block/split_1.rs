use super::*;

/// Builds PDF paint primitives for a CSS block's background and border area.
///
/// CSS Backgrounds and Borders paints backgrounds and borders over the border
/// box; boxes with nonpositive used border-box area do not contribute visible
/// background paint:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background> and
/// <https://www.w3.org/TR/css-backgrounds-3/#borders>.
pub(crate) fn block_paint_ops(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
) -> (
    Vec<RenderedRect>,
    Vec<RenderedRoundedRect>,
    Vec<RenderedPath>,
    Vec<RenderedStroke>,
) {
    block_paint_ops_with_border_insets(x, y, width, height, style, used_border_widths(style), true)
}

/// Builds PDF paint primitives for a CSS block with caller-supplied border
/// insets.
///
/// Collapsed table cells use resolved grid half-widths for decoration
/// geometry, while their actual borders are painted later from the collapsed
/// border grid:
/// <https://drafts.csswg.org/css-tables-3/#in-collapsed-borders-mode>.
pub(crate) fn block_paint_ops_with_border_insets(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border_insets: css::Edges,
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
    if width <= 0.0 || height <= 0.0 {
        return (rects, rounded_rects, paths, strokes);
    }
    let geometry = BoxPaintGeometry {
        x,
        y,
        width,
        height,
        border_insets,
    };
    paint_box_shadows(&mut rects, geometry, style, false);
    if let Some(fill) = style.background_color
        && fill.is_visible()
    {
        let area = background_rect_area_for_box(
            x,
            y,
            width,
            height,
            style,
            border_insets,
            style.background_clip,
        );
        if area.width <= 0.0 || area.height <= 0.0 {
            // Nothing to paint for the solid color layer after clipping.
        } else if style.border_radius.is_zero() {
            rects.push(RenderedRect::from_paint_rect(
                paint_space_rect(area.x, area.y, area.width, area.height),
                Some(fill),
            ));
        } else if style.corner_shapes.all_round() {
            if style.background_clip == css::BackgroundBox::Border {
                rounded_rects.push(RenderedRoundedRect::from_paint_rect(
                    paint_space_rect(area.x, area.y, area.width, area.height),
                    used_rounded_rect_radii(style.border_radius, width, height),
                    Some(fill),
                    None,
                    0.0,
                ));
            } else if let Some(clip) = rounded_background_clip_for_box(
                x,
                y,
                width,
                height,
                style,
                border_insets,
                style.background_clip,
            ) {
                paths.push(RenderedPath::new(
                    clip.commands,
                    Some(fill),
                    clip.fill_rule,
                    None,
                    0.0,
                    None,
                ));
            } else {
                rects.push(RenderedRect::from_paint_rect(
                    paint_space_rect(area.x, area.y, area.width, area.height),
                    Some(fill),
                ));
            }
        } else {
            if let Some(clip) = rounded_background_clip_for_box(
                x,
                y,
                width,
                height,
                style,
                border_insets,
                style.background_clip,
            ) {
                paths.push(RenderedPath::new(
                    clip.commands,
                    Some(fill),
                    clip.fill_rule,
                    None,
                    0.0,
                    None,
                ));
            } else {
                rects.push(RenderedRect::from_paint_rect(
                    paint_space_rect(area.x, area.y, area.width, area.height),
                    Some(fill),
                ));
            }
        }
    }
    if style.border_radius.is_zero() {
        rects.extend(linear_gradient_rects(
            x,
            y,
            width,
            height,
            style,
            border_insets,
        ));
    } else {
        paths.extend(linear_gradient_rect_paths(
            x,
            y,
            width,
            height,
            style,
            border_insets,
        ));
    }
    paths.extend(linear_gradient_paths(
        x,
        y,
        width,
        height,
        style,
        border_insets,
    ));
    paint_box_shadows(&mut rects, geometry, style, true);
    if !paint_borders || style.border_image.source.is_some() {
        return (rects, rounded_rects, paths, strokes);
    }
    if !paint_uniform_rounded_border(&mut rounded_rects, x, y, width, height, style)
        && !paint_uniform_double_rounded_border(&mut paths, x, y, width, height, style)
        && !paint_solid_rounded_border_ring(&mut paths, x, y, width, height, style)
        && !paint_patterned_rounded_border_sides(&mut paths, x, y, width, height, style)
        && !paint_clipped_rounded_border_sides(&mut paths, x, y, width, height, style)
    {
        paint_border_edges(&mut rects, &mut paths, x, y + height, width, height, style);
    }
    (rects, rounded_rects, paths, strokes)
}

/// Converts supported linear gradients to filled rectangle bands.
///
/// CSS Images defines gradients as generated images. For axis-aligned
/// hard-stop gradients, equivalent rectangle bands preserve the specified
/// colors and stop positions exactly in PDF output:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
pub(in crate::layout) fn linear_gradient_rects(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Vec<RenderedRect> {
    linear_gradient_rects_with_clip(x, y, width, height, style, border_insets, None)
}

/// Converts axis-aligned hard-stop linear gradients with an extra clip.
///
/// CSS Images positions gradients in their generated image box, while CSS
/// Backgrounds clips each layer independently. Table structural backgrounds
/// reuse the full column box for positioning and row fragments as the clip:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients> and
/// <https://www.w3.org/TR/css-backgrounds-3/#backgrounds>.
pub(in crate::layout) fn linear_gradient_rects_with_clip(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border_insets: css::Edges,
    extra_clip: Option<BackgroundRectArea>,
) -> Vec<RenderedRect> {
    let mut rects = Vec::new();
    for layer in background_layers_for_gradient_paint(style).iter().rev() {
        let Some(BackgroundImage::LinearGradient(gradient)) = &layer.image else {
            continue;
        };
        if !linear_gradient_can_paint_as_vector(gradient, layer) {
            continue;
        }
        let area =
            background_rect_area_for_box(x, y, width, height, style, border_insets, layer.origin);
        let clip = background_rect_clip_area_for_box(
            x,
            y,
            width,
            height,
            style,
            border_insets,
            layer.clip,
            extra_clip,
        );
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
            area.x,
            area.y,
            area.width,
            area.height,
            0.0,
            first.position,
            first.color,
        );
        for pair in stops.windows(2) {
            push_gradient_band(
                &mut rects,
                axis_direction,
                area.x,
                area.y,
                area.width,
                area.height,
                pair[0].position,
                pair[1].position,
                pair[0].color,
            );
        }
        let last = *stops.last().expect("checked length above");
        push_gradient_band(
            &mut rects,
            axis_direction,
            area.x,
            area.y,
            area.width,
            area.height,
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
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Vec<RenderedPath> {
    linear_gradient_rect_paths_with_clip(x, y, width, height, style, border_insets, None)
}

/// Converts rounded axis-aligned hard-stop gradients with an extra clip.
///
/// CSS Backgrounds clips generated-image layers to `background-clip`; callers
/// may intersect that clip with a fragment-local exposed area:
/// <https://www.w3.org/TR/css-backgrounds-3/#background-clip>.
pub(in crate::layout) fn linear_gradient_rect_paths_with_clip(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border_insets: css::Edges,
    extra_clip: Option<BackgroundRectArea>,
) -> Vec<RenderedPath> {
    let mut paths = Vec::new();
    for layer in background_layers_for_gradient_paint(style).iter().rev() {
        let Some(BackgroundImage::LinearGradient(gradient)) = &layer.image else {
            continue;
        };
        if !linear_gradient_can_paint_as_vector(gradient, layer) {
            continue;
        }
        let Some(axis_direction) = axis_aligned_gradient_direction(gradient.direction) else {
            continue;
        };
        let area =
            background_rect_area_for_box(x, y, width, height, style, border_insets, layer.origin);
        let clip = background_rect_clip_area_for_box(
            x,
            y,
            width,
            height,
            style,
            border_insets,
            layer.clip,
            extra_clip,
        );
        let Some(rounded_clip) =
            rounded_background_clip_for_box(x, y, width, height, style, border_insets, layer.clip)
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
            area.x,
            area.y,
            area.width,
            area.height,
            0.0,
            first.position,
            first.color,
        );
        for pair in stops.windows(2) {
            push_gradient_band(
                &mut rects,
                axis_direction,
                area.x,
                area.y,
                area.width,
                area.height,
                pair[0].position,
                pair[1].position,
                pair[0].color,
            );
        }
        let last = *stops.last().expect("checked length above");
        push_gradient_band(
            &mut rects,
            axis_direction,
            area.x,
            area.y,
            area.width,
            area.height,
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
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Vec<RenderedPath> {
    linear_gradient_paths_with_clip(x, y, width, height, style, border_insets, None)
}

/// Converts angled hard-stop linear gradients with an extra clip.
///
/// CSS Images defines angled gradients in the full gradient box. The optional
/// clip only constrains the painted polygon, preserving that coordinate space:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
pub(in crate::layout) fn linear_gradient_paths_with_clip(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border_insets: css::Edges,
    extra_clip: Option<BackgroundRectArea>,
) -> Vec<RenderedPath> {
    let mut paths = Vec::new();
    for layer in background_layers_for_gradient_paint(style).iter().rev() {
        let Some(BackgroundImage::LinearGradient(gradient)) = &layer.image else {
            continue;
        };
        if !linear_gradient_can_paint_as_vector(gradient, layer)
            || axis_aligned_gradient_direction(gradient.direction).is_some()
        {
            continue;
        }
        let area =
            background_rect_area_for_box(x, y, width, height, style, border_insets, layer.origin);
        let clip = background_rect_clip_area_for_box(
            x,
            y,
            width,
            height,
            style,
            border_insets,
            layer.clip,
            extra_clip,
        );
        let line = angled_gradient_line(gradient.direction, area);
        let Some(stops) = fixed_gradient_stops(gradient, line.axis_length) else {
            continue;
        };
        if !fixed_gradient_is_hard_stop(&stops) {
            continue;
        }

        let rounded_clip =
            rounded_background_clip_for_box(x, y, width, height, style, border_insets, layer.clip);

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
    }
    paths
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

pub(in crate::layout) fn gradient_stop_position(
    stop: css::GradientColorStop,
    axis_length: f32,
) -> Option<f32> {
    let position = stop.position?;
    Some(
        position
            .used_length_with_percentage_basis(axis_length)
            .unwrap_or(position.length_with_percentage_basis(axis_length)),
    )
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FixedGradientStop {
    pub(in crate::layout) color: Color,
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
        .copied()
        .map(|stop| gradient_stop_position(stop, axis_length))
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

    Some(
        gradient
            .stops
            .iter()
            .zip(positions)
            .map(|(stop, position)| FixedGradientStop {
                color: stop.color,
                position: position.expect("all positions fixed up"),
            })
            .collect(),
    )
}

pub(in crate::layout) fn fixed_gradient_is_hard_stop(stops: &[FixedGradientStop]) -> bool {
    stops.windows(2).all(|pair| {
        (pair[0].position - pair[1].position).abs() <= 0.001 || pair[0].color == pair[1].color
    })
}

pub(in crate::layout) fn background_layers_for_gradient_paint(
    style: &ComputedStyle,
) -> Vec<css::BackgroundLayer> {
    if !style.background_layers.is_empty() {
        return style.background_layers.clone();
    }
    vec![css::BackgroundLayer {
        image: style.background_image.clone(),
        position: style.background_position,
        size: style.background_size,
        repeat: style.background_repeat,
        origin: style.background_origin,
        clip: style.background_clip,
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

fn axis_aligned_gradient_length(
    direction: LinearGradientDirection,
    area: BackgroundRectArea,
) -> f32 {
    match direction {
        LinearGradientDirection::Angle(angle)
            if (angle.rem_euclid(360.0) - 0.0).abs() < 0.001
                || (angle.rem_euclid(360.0) - 180.0).abs() < 0.001 =>
        {
            area.height
        }
        LinearGradientDirection::Angle(angle)
            if (angle.rem_euclid(360.0) - 90.0).abs() < 0.001
                || (angle.rem_euclid(360.0) - 270.0).abs() < 0.001 =>
        {
            area.width
        }
        _ => 0.0,
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AngledGradientLine {
    pub(in crate::layout) center_x: f32,
    pub(in crate::layout) center_y: f32,
    pub(in crate::layout) dir_x: f32,
    pub(in crate::layout) dir_y: f32,
    pub(in crate::layout) axis_length: f32,
}

pub(in crate::layout) fn angled_gradient_line(
    direction: LinearGradientDirection,
    area: BackgroundRectArea,
) -> AngledGradientLine {
    let angle = gradient_direction_angle_for_area(direction, area);
    let radians = angle.to_radians();
    let dir_x = radians.sin();
    let dir_y = radians.cos();
    let axis_length = area.width * dir_x.abs() + area.height * dir_y.abs();
    AngledGradientLine {
        center_x: area.x + area.width / 2.0,
        center_y: area.y + area.height / 2.0,
        dir_x,
        dir_y,
        axis_length,
    }
}

pub(in crate::layout) fn gradient_direction_angle_for_area(
    direction: LinearGradientDirection,
    area: BackgroundRectArea,
) -> f32 {
    match direction {
        LinearGradientDirection::Angle(angle) => angle,
        LinearGradientDirection::Corner {
            horizontal,
            vertical,
        } => {
            let x = match horizontal {
                css::GradientHorizontalDirection::Left => -area.width,
                css::GradientHorizontalDirection::Right => area.width,
            };
            let y = match vertical {
                css::GradientVerticalDirection::Top => area.height,
                css::GradientVerticalDirection::Bottom => -area.height,
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
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border_insets: css::Edges,
    clip_box: css::BackgroundBox,
) -> Option<RenderedPathClip> {
    if style.border_radius.is_zero() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let area = background_rect_area_for_box(x, y, width, height, style, border_insets, clip_box);
    if area.width <= 0.0 || area.height <= 0.0 {
        return None;
    }
    let insets = background_clip_edge_insets(style, border_insets, clip_box);
    let mut radii = used_rounded_rect_radii(style.border_radius, width, height);
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
    scale_rounded_radii_to_area(&mut radii, area.width, area.height);
    if rounded_radii_are_zero(radii) {
        return None;
    }
    Some(RenderedPathClip::new(
        shaped_rect_path_commands(
            area.x,
            area.y,
            area.width,
            area.height,
            radii,
            css::CornerShapes::ROUND,
        ),
        RenderedPathFillRule::NonZero,
        Vec::new(),
    ))
}

fn background_clip_edge_insets(
    style: &ComputedStyle,
    border_insets: css::Edges,
    clip_box: css::BackgroundBox,
) -> css::Edges {
    match clip_box {
        css::BackgroundBox::Border => css::Edges::ZERO,
        css::BackgroundBox::Padding => border_insets,
        css::BackgroundBox::Content => css::Edges {
            top: border_insets.top + style.padding.top,
            right: border_insets.right + style.padding.right,
            bottom: border_insets.bottom + style.padding.bottom,
            left: border_insets.left + style.padding.left,
        },
    }
}

fn scale_rounded_radii_to_area(radii: &mut RenderedRoundedRectRadii, width: f32, height: f32) {
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
}

fn rounded_radii_are_zero(radii: RenderedRoundedRectRadii) -> bool {
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
        0.0,
        Some(clip),
    ))
}

fn push_gradient_polygon_band(
    paths: &mut Vec<RenderedPath>,
    line: AngledGradientLine,
    clip: BackgroundRectArea,
    start: f32,
    end: f32,
    color: Color,
    rounded_clip: Option<RenderedPathClip>,
) {
    if !color.is_visible() {
        return;
    }
    let start = start.clamp(0.0, line.axis_length);
    let end = end.clamp(0.0, line.axis_length);
    if end <= start || clip.width <= 0.0 || clip.height <= 0.0 {
        return;
    }
    let mut polygon = vec![
        (clip.x, clip.y),
        (clip.x + clip.width, clip.y),
        (clip.x + clip.width, clip.y + clip.height),
        (clip.x, clip.y + clip.height),
    ];
    polygon = clip_gradient_polygon(polygon, line, start, true);
    polygon = clip_gradient_polygon(polygon, line, end, false);
    if polygon.len() < 3 {
        return;
    }
    let mut commands = Vec::with_capacity(polygon.len() + 1);
    commands.push(RenderedPathCommand::move_to(paint_space_point(
        polygon[0].0,
        polygon[0].1,
    )));
    for point in &polygon[1..] {
        commands.push(RenderedPathCommand::line_to(paint_space_point(
            point.0, point.1,
        )));
    }
    commands.push(RenderedPathCommand::Close);
    paths.push(RenderedPath::new(
        commands,
        Some(color),
        RenderedPathFillRule::NonZero,
        None,
        0.0,
        rounded_clip,
    ));
}

fn clip_gradient_polygon(
    polygon: Vec<(f32, f32)>,
    line: AngledGradientLine,
    boundary: f32,
    keep_after: bool,
) -> Vec<(f32, f32)> {
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
    point: (f32, f32),
    line: AngledGradientLine,
) -> f32 {
    (point.0 - line.center_x) * line.dir_x
        + (point.1 - line.center_y) * line.dir_y
        + line.axis_length / 2.0
}

fn gradient_boundary_intersection(
    start: (f32, f32),
    end: (f32, f32),
    start_value: f32,
    end_value: f32,
) -> Option<(f32, f32)> {
    let denominator = start_value - end_value;
    if denominator.abs() <= f32::EPSILON {
        return None;
    }
    let t = (start_value / denominator).clamp(0.0, 1.0);
    Some((
        start.0 + (end.0 - start.0) * t,
        start.1 + (end.1 - start.1) * t,
    ))
}

pub(in crate::layout) fn background_rect_area_for_box(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border: css::Edges,
    box_: css::BackgroundBox,
) -> BackgroundRectArea {
    let area = BackgroundRectArea {
        x,
        y,
        width,
        height,
    };
    match box_ {
        css::BackgroundBox::Border => area,
        css::BackgroundBox::Padding => area.inset(border),
        css::BackgroundBox::Content => area.inset(border).inset(style.padding),
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
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border: css::Edges,
    box_: css::BackgroundBox,
    extra_clip: Option<BackgroundRectArea>,
) -> BackgroundRectArea {
    let clip = background_rect_area_for_box(x, y, width, height, style, border, box_);
    extra_clip.map_or(clip, |extra_clip| clip.intersection(extra_clip))
}

#[derive(Clone, Copy)]
pub(in crate::layout) struct BoxPaintGeometry {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
    pub(in crate::layout) border_insets: css::Edges,
}

#[derive(Clone, Copy)]
pub(in crate::layout) struct BackgroundRectArea {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
}

impl BackgroundRectArea {
    pub(in crate::layout) fn inset(self, edges: css::Edges) -> Self {
        Self {
            x: self.x + edges.left,
            y: self.y + edges.bottom,
            width: (self.width - edges.left - edges.right).max(0.0),
            height: (self.height - edges.top - edges.bottom).max(0.0),
        }
    }

    pub(in crate::layout) fn intersection(self, other: Self) -> Self {
        let left = self.x.max(other.x);
        let bottom = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let top = (self.y + self.height).min(other.y + other.height);
        Self {
            x: left,
            y: bottom,
            width: (right - left).max(0.0),
            height: (top - bottom).max(0.0),
        }
    }
}

pub(in crate::layout) fn paint_box_shadows(
    rects: &mut Vec<RenderedRect>,
    geometry: BoxPaintGeometry,
    style: &ComputedStyle,
    inset: bool,
) {
    for shadow in style
        .box_shadow
        .iter()
        .rev()
        .filter(|shadow| shadow.inset == inset)
    {
        let color = shadow.color.resolve(style.color);
        if !color.is_visible()
            || shadow.blur_radius.length_points() > 0.0
            || !style.border_radius.is_zero()
        {
            continue;
        }
        if shadow.inset {
            paint_inset_box_shadow(rects, geometry, *shadow, color);
        } else {
            paint_outer_box_shadow(rects, geometry, *shadow, color);
        }
    }
}

pub(in crate::layout) fn paint_outer_box_shadow(
    rects: &mut Vec<RenderedRect>,
    geometry: BoxPaintGeometry,
    shadow: css::BoxShadow,
    color: Color,
) {
    let offset_x = shadow.offset_x.length_points();
    let offset_y = shadow.offset_y.length_points();
    let spread = shadow.spread.length_points();
    let shadow_x = geometry.x + offset_x - spread;
    let shadow_y = geometry.y - offset_y - spread;
    let shadow_width = geometry.width + spread * 2.0;
    let shadow_height = geometry.height + spread * 2.0;
    if shadow_width <= 0.0 || shadow_height <= 0.0 {
        return;
    }

    push_rect_difference(
        rects,
        BackgroundRectArea {
            x: shadow_x,
            y: shadow_y,
            width: shadow_width,
            height: shadow_height,
        },
        BackgroundRectArea {
            x: geometry.x,
            y: geometry.y,
            width: geometry.width,
            height: geometry.height,
        },
        color,
    );
}

pub(in crate::layout) fn paint_inset_box_shadow(
    rects: &mut Vec<RenderedRect>,
    geometry: BoxPaintGeometry,
    shadow: css::BoxShadow,
    color: Color,
) {
    let padding = BackgroundRectArea {
        x: geometry.x,
        y: geometry.y,
        width: geometry.width,
        height: geometry.height,
    }
    .inset(geometry.border_insets);
    if padding.width <= 0.0 || padding.height <= 0.0 {
        return;
    }

    let spread = shadow.spread.length_points_max_zero();
    let offset_x = shadow.offset_x.length_points();
    let offset_y = shadow.offset_y.length_points();
    let left = inset_shadow_edge_width(offset_x, spread, true).min(padding.width);
    let right = inset_shadow_edge_width(offset_x, spread, false).min(padding.width);
    let top = inset_shadow_edge_width(offset_y, spread, true).min(padding.height);
    let bottom = inset_shadow_edge_width(offset_y, spread, false).min(padding.height);

    push_shadow_rect(rects, padding.x, padding.y, left, padding.height, color);
    push_shadow_rect(
        rects,
        padding.x + padding.width - right,
        padding.y,
        right,
        padding.height,
        color,
    );
    push_shadow_rect(
        rects,
        padding.x,
        padding.y + padding.height - top,
        padding.width,
        top,
        color,
    );
    push_shadow_rect(rects, padding.x, padding.y, padding.width, bottom, color);
}

pub(in crate::layout) fn inset_shadow_edge_width(
    offset: f32,
    spread: f32,
    start_edge: bool,
) -> f32 {
    match (offset > 0.0, offset < 0.0, offset == 0.0 && spread > 0.0) {
        (true, _, _) if start_edge => offset + spread,
        (_, true, _) if !start_edge => -offset + spread,
        (_, _, true) => spread,
        _ => 0.0,
    }
}

pub(in crate::layout) fn push_rect_difference(
    rects: &mut Vec<RenderedRect>,
    subject: BackgroundRectArea,
    cutout: BackgroundRectArea,
    color: Color,
) {
    let left = subject.x;
    let right = subject.x + subject.width;
    let bottom = subject.y;
    let top = subject.y + subject.height;
    let cut_left = cutout.x.max(left).min(right);
    let cut_right = (cutout.x + cutout.width).max(left).min(right);
    let cut_bottom = cutout.y.max(bottom).min(top);
    let cut_top = (cutout.y + cutout.height).max(bottom).min(top);

    push_shadow_rect(
        rects,
        left,
        bottom,
        subject.width,
        cut_bottom - bottom,
        color,
    );
    push_shadow_rect(rects, left, cut_top, subject.width, top - cut_top, color);
    push_shadow_rect(
        rects,
        left,
        cut_bottom,
        cut_left - left,
        cut_top - cut_bottom,
        color,
    );
    push_shadow_rect(
        rects,
        cut_right,
        cut_bottom,
        right - cut_right,
        cut_top - cut_bottom,
        color,
    );
}

pub(in crate::layout) fn push_shadow_rect(
    rects: &mut Vec<RenderedRect>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
) {
    if width > 0.0 && height > 0.0 {
        rects.push(RenderedRect::from_paint_rect(
            paint_space_rect(x, y, width, height),
            Some(color),
        ));
    }
}

pub(in crate::layout) fn clip_gradient_rect(rect: &mut RenderedRect, clip: BackgroundRectArea) {
    let x1 = rect.x().max(clip.x);
    let y1 = rect.y().max(clip.y);
    let x2 = (rect.x() + rect.width()).min(clip.x + clip.width);
    let y2 = (rect.y() + rect.height()).min(clip.y + clip.height);
    rect.set_paint_rect(paint_space_rect(
        x1,
        y1,
        (x2 - x1).max(0.0),
        (y2 - y1).max(0.0),
    ));
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn push_gradient_band(
    rects: &mut Vec<RenderedRect>,
    direction: LinearGradientDirection,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    start: f32,
    end: f32,
    color: Color,
) {
    if !color.is_visible() {
        return;
    }
    let angle = match direction {
        LinearGradientDirection::Angle(angle) => angle.rem_euclid(360.0),
        LinearGradientDirection::Corner { .. } => return,
    };
    let axis_length = if (angle - 0.0).abs() < 0.001 || (angle - 180.0).abs() < 0.001 {
        height
    } else if (angle - 90.0).abs() < 0.001 || (angle - 270.0).abs() < 0.001 {
        width
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
            paint_space_rect(x, y + height - end, width, end - start),
            Some(color),
        )
    } else if (angle - 0.0).abs() < 0.001 {
        RenderedRect::from_paint_rect(
            paint_space_rect(x, y + start, width, end - start),
            Some(color),
        )
    } else if (angle - 90.0).abs() < 0.001 {
        RenderedRect::from_paint_rect(
            paint_space_rect(x + start, y, end - start, height),
            Some(color),
        )
    } else {
        RenderedRect::from_paint_rect(
            paint_space_rect(x + width - end, y, end - start, height),
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
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
) -> bool {
    if style.border_radius.is_zero() || !style.corner_shapes.all_round() {
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
    if !same_width(top.used_width, right.used_width)
        || !same_width(top.used_width, bottom.used_width)
        || !same_width(top.used_width, left.used_width)
        || top.color != right.color
        || top.color != bottom.color
        || top.color != left.color
    {
        return false;
    }

    let border_width = top.used_width.min(width).min(height);
    if border_width <= 0.0 {
        return true;
    }

    let inset = border_width / 2.0;
    let mut radii = used_rounded_rect_radii(style.border_radius, width, height);
    inset_rounded_rect_radii(&mut radii, inset);
    rounded_rects.push(RenderedRoundedRect::from_paint_rect(
        paint_space_rect(
            x + inset,
            y + inset,
            width - border_width,
            height - border_width,
        ),
        radii,
        None,
        Some(top.color),
        border_width,
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
    if !same_width(top.used_width, right.used_width)
        || !same_width(top.used_width, bottom.used_width)
        || !same_width(top.used_width, left.used_width)
        || top.color != right.color
        || top.color != bottom.color
        || top.color != left.color
    {
        return false;
    }

    let border_width = top.used_width.min(width).min(height);
    if border_width <= 0.0 || !top.color.is_visible() {
        return true;
    }
    if border_width < 3.0 {
        let outer_radii = used_rounded_rect_radii(style.border_radius, width, height);
        paths.push(uniform_rounded_ring_path(
            x,
            y,
            width,
            height,
            outer_radii,
            border_width,
            top.color,
        ));
        return true;
    }

    let stripe = (border_width / 3.0).max(1.0);
    let outer_radii = used_rounded_rect_radii(style.border_radius, width, height);
    paths.push(uniform_rounded_ring_path(
        x,
        y,
        width,
        height,
        outer_radii,
        stripe,
        top.color,
    ));

    let inner_outer_inset = border_width - stripe;
    let inner_width = (width - 2.0 * inner_outer_inset).max(0.0);
    let inner_height = (height - 2.0 * inner_outer_inset).max(0.0);
    if inner_width > 0.0 && inner_height > 0.0 {
        let mut inner_outer_radii = outer_radii;
        inset_rounded_rect_radii(&mut inner_outer_radii, inner_outer_inset);
        paths.push(uniform_rounded_ring_path(
            x + inner_outer_inset,
            y + inner_outer_inset,
            inner_width,
            inner_height,
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

    let inner_width = (width - left.used_width - right.used_width).max(0.0);
    let inner_height = (height - top.used_width - bottom.used_width).max(0.0);
    if width <= 0.0 || height <= 0.0 || !top.color.is_visible() {
        return true;
    }

    let outer_radii = used_rounded_rect_radii(style.border_radius, width, height);
    let inner_radii = RenderedRoundedRectRadii {
        top_left: RenderedCornerRadius::new(
            outer_radii.top_left.x() - left.used_width,
            outer_radii.top_left.y() - top.used_width,
        ),
        top_right: RenderedCornerRadius::new(
            outer_radii.top_right.x() - right.used_width,
            outer_radii.top_right.y() - top.used_width,
        ),
        bottom_right: RenderedCornerRadius::new(
            outer_radii.bottom_right.x() - right.used_width,
            outer_radii.bottom_right.y() - bottom.used_width,
        ),
        bottom_left: RenderedCornerRadius::new(
            outer_radii.bottom_left.x() - left.used_width,
            outer_radii.bottom_left.y() - bottom.used_width,
        ),
    };

    let mut commands =
        shaped_rect_path_commands(x, y, width, height, outer_radii, style.corner_shapes);
    if inner_width > 0.0 && inner_height > 0.0 {
        commands.extend(shaped_rect_path_commands(
            x + left.used_width,
            y + bottom.used_width,
            inner_width,
            inner_height,
            inner_radii,
            style.corner_shapes,
        ));
    }
    paths.push(RenderedPath::new(
        commands,
        Some(top.color),
        RenderedPathFillRule::EvenOdd,
        None,
        0.0,
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
        .any(|(_, side)| border_side_has_area(*side) && !is_patterned_side_style(side.style))
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
        let clip = rounded_border_pattern_clip(edge, x, y, width, height, style, borders);
        let (axis_start, axis_length, cross_start, cross_width, horizontal) =
            border_side_geometry(edge, x, y + height, width, height, side.used_width);
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
                side.used_width,
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
        color: Color,
        position: Option<css::ComputedLengthPercentage>,
    ) -> css::GradientColorStop {
        css::GradientColorStop { color, position }
    }

    fn gradient(stops: Vec<css::GradientColorStop>) -> css::LinearGradient {
        css::LinearGradient {
            direction: LinearGradientDirection::Angle(180.0),
            repeating: false,
            stops,
            hints: Vec::new(),
        }
    }

    #[test]
    fn fixed_gradient_stops_default_and_distribute_omitted_positions() {
        let stops = fixed_gradient_stops(
            &gradient(vec![
                stop(Color::new(255, 0, 0), None),
                stop(Color::new(0, 128, 0), None),
                stop(
                    Color::new(0, 0, 255),
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
                    Color::new(255, 0, 0),
                    Some(css::ComputedLengthPercentage::from_percent(0.75)),
                ),
                stop(
                    Color::new(0, 128, 0),
                    Some(css::ComputedLengthPercentage::from_percent(0.25)),
                ),
                stop(Color::new(0, 0, 255), None),
            ]),
            100.0,
        )
        .expect("gradient stops should fix up");

        assert_eq!(stops[0].position, 75.0);
        assert_eq!(stops[1].position, 75.0);
        assert_eq!(stops[2].position, 100.0);
    }
}
