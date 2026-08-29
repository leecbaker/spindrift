use super::*;

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

pub(super) fn gradient_rect_path(
    rect: RenderedRect,
    clip: RenderedPathClip,
) -> Option<RenderedPath> {
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
}
