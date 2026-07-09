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
    if paint_outer_shadows {
        paint_box_shadows(&mut rects, geometry, style, false);
    }
    if paint_backgrounds
        && let Some(fill) = style.background_color
        && fill.is_visible()
    {
        let color_clip = style.background_color_clip();
        let area = background_rect_area_for_box(rect, style, border_insets, color_clip);
        if area.size.width <= 0.0 || area.size.height <= 0.0 {
            // Nothing to paint for the solid color layer after clipping.
        } else if style.border_radius.clone().is_zero() {
            rects.push(RenderedRect::from_paint_rect(area, Some(fill)));
        } else if style.corner_shapes.all_round() {
            if color_clip == css::BackgroundBox::Border {
                rounded_rects.push(RenderedRoundedRect::from_paint_rect(
                    area,
                    used_rounded_rect_radii(style.border_radius.clone(), rect.size),
                    Some(fill),
                    None,
                    0.0,
                ));
            } else if let Some(clip) =
                rounded_background_clip_for_box(rect, style, border_insets, color_clip)
            {
                paths.push(RenderedPath::new(
                    clip.commands,
                    Some(fill),
                    clip.fill_rule,
                    None,
                    0.0,
                    None,
                ));
            } else {
                rects.push(RenderedRect::from_paint_rect(area, Some(fill)));
            }
        } else {
            if let Some(clip) =
                rounded_background_clip_for_box(rect, style, border_insets, color_clip)
            {
                paths.push(RenderedPath::new(
                    clip.commands,
                    Some(fill),
                    clip.fill_rule,
                    None,
                    0.0,
                    None,
                ));
            } else {
                rects.push(RenderedRect::from_paint_rect(area, Some(fill)));
            }
        }
    }
    if paint_backgrounds {
        if style.border_radius.clone().is_zero() {
            rects.extend(linear_gradient_rects(rect, style, border_insets));
        } else {
            paths.extend(linear_gradient_rect_paths(rect, style, border_insets));
        }
        paths.extend(linear_gradient_paths(rect, style, border_insets));
    }
    if paint_inset_shadows {
        paint_box_shadows(&mut rects, geometry, style, true);
    }
    if !paint_borders || style.border_image.source.is_some() {
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
        let Some(BackgroundImage::LinearGradient(gradient)) = &layer.image else {
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
        let Some(BackgroundImage::LinearGradient(gradient)) = &layer.image else {
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
        let Some(BackgroundImage::LinearGradient(gradient)) = &layer.image else {
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

/// Whether [`block_paint_ops_with_phases`] emits this layer as exact
/// axis-aligned hard-stop vector bands rather than delegating it to the
/// generic generated-image painter.
pub(in crate::layout) fn linear_gradient_is_painted_by_box_decoration(
    gradient: &css::LinearGradient,
    layer: &css::BackgroundLayer,
    size: PaintSize,
) -> bool {
    let Some(direction) = axis_aligned_gradient_direction(gradient.direction) else {
        return false;
    };
    linear_gradient_can_paint_as_vector(gradient, layer)
        && gradient.stops.iter().all(|stop| stop.color.is_opaque())
        && fixed_gradient_stops(
            gradient,
            axis_aligned_gradient_length(
                direction,
                PaintRect::new(PaintPoint::new(0.0, 0.0), size),
            ),
        )
        .is_some_and(|stops| fixed_gradient_is_hard_stop(&stops))
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
    if !style.background_layers.is_empty() {
        return style.background_layers.clone();
    }
    vec![css::BackgroundLayer {
        image: style.background_image.clone(),
        position: style.background_position.clone(),
        size: style.background_size.clone(),
        repeat: style.background_repeat,
        attachment: style.background_attachment,
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
                css::GradientHorizontalDirection::Left => -area.size.width,
                css::GradientHorizontalDirection::Right => area.size.width,
            };
            let y = match vertical {
                css::GradientVerticalDirection::Top => area.size.height,
                css::GradientVerticalDirection::Bottom => -area.size.height,
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
            css::CornerShapes::ROUND,
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
    Some(RenderedRoundedRect::new(
        area.origin.x,
        area.origin.y,
        area.size.width,
        area.size.height,
        radii,
        None,
        None,
        0.0,
    ))
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
    clip: PaintRect,
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
    let mut commands = Vec::with_capacity(polygon.len() + 1);
    commands.push(RenderedPathCommand::move_to(polygon[0]));
    for point in &polygon[1..] {
        commands.push(RenderedPathCommand::line_to(*point));
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
            || !style.border_radius.clone().is_zero()
        {
            continue;
        }
        if shadow.inset {
            paint_inset_box_shadow(rects, geometry, shadow.clone(), color);
        } else {
            paint_outer_box_shadow(rects, geometry, shadow.clone(), color);
        }
    }
}

pub(in crate::layout) fn paint_outer_box_shadow(
    rects: &mut Vec<RenderedRect>,
    geometry: BoxPaintGeometry,
    shadow: css::BoxShadow,
    color: Color,
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
    color: Color,
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
    color: Color,
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
    color: Color,
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
    if style.border_radius.clone().is_zero() || !style.corner_shapes.all_round() {
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

    let border_width = top.used_width.min(rect.size.width).min(rect.size.height);
    if border_width <= 0.0 {
        return true;
    }

    let inset = border_width / 2.0;
    let mut radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
    inset_rounded_rect_radii(&mut radii, inset);
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
    rect: PaintRect,
    style: &ComputedStyle,
) -> bool {
    if style.border_radius.clone().is_zero() {
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

    let border_width = top.used_width.min(rect.size.width).min(rect.size.height);
    if border_width <= 0.0 || !top.color.is_visible() {
        return true;
    }
    if border_width < 3.0 {
        let outer_radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
        paths.push(uniform_rounded_ring_path(
            rect,
            outer_radii,
            border_width,
            top.color,
        ));
        return true;
    }

    let stripe = (border_width / 3.0).max(1.0);
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
        let mut inner_outer_radii = outer_radii;
        inset_rounded_rect_radii(&mut inner_outer_radii, inner_outer_inset);
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
    if style.border_radius.clone().is_zero() {
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

    let inner_width = (rect.size.width - left.used_width - right.used_width).max(0.0);
    let inner_height = (rect.size.height - top.used_width - bottom.used_width).max(0.0);
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 || !top.color.is_visible() {
        return true;
    }

    let outer_radii = used_rounded_rect_radii(style.border_radius.clone(), rect.size);
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

    let mut commands = shaped_rect_path_commands(rect, outer_radii, style.corner_shapes);
    if inner_width > 0.0 && inner_height > 0.0 {
        let inner_rect = inset_paint_rect(
            rect,
            css::Edges {
                top: top.used_width,
                right: right.used_width,
                bottom: bottom.used_width,
                left: left.used_width,
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
    rect: PaintRect,
    style: &ComputedStyle,
) -> bool {
    if style.border_radius.clone().is_zero() {
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
            side.used_width,
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
}
