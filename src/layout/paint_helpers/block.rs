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
        if style.border_radius.is_zero() {
            rects.push(RenderedRect::from_paint_rect(
                paint_space_rect(x, y, width, height),
                Some(fill),
            ));
        } else if style.corner_shapes.all_round() {
            rounded_rects.push(RenderedRoundedRect::from_paint_rect(
                paint_space_rect(x, y, width, height),
                used_rounded_rect_radii(style.border_radius, width, height),
                Some(fill),
                None,
                0.0,
            ));
        } else {
            paths.push(RenderedPath::new(
                shaped_rect_path_commands(
                    x,
                    y,
                    width,
                    height,
                    used_rounded_rect_radii(style.border_radius, width, height),
                    style.corner_shapes,
                ),
                Some(fill),
                RenderedPathFillRule::NonZero,
                None,
                0.0,
                None,
            ));
        }
    }
    rects.extend(linear_gradient_rects(
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
fn linear_gradient_rects(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Vec<RenderedRect> {
    let mut rects = Vec::new();
    for layer in background_layers_for_gradient_paint(style).iter().rev() {
        let Some(BackgroundImage::LinearGradient(gradient)) = &layer.image else {
            continue;
        };
        let area =
            background_rect_area_for_box(x, y, width, height, style, border_insets, layer.origin);
        let clip =
            background_rect_area_for_box(x, y, width, height, style, border_insets, layer.clip);
        let axis_length = match gradient.direction {
            LinearGradientDirection::Bottom | LinearGradientDirection::Top => area.height,
            LinearGradientDirection::Right | LinearGradientDirection::Left => area.width,
        };
        if axis_length <= 0.0 || gradient.stops.len() < 2 {
            continue;
        }

        let before = rects.len();
        let first = gradient.stops[0];
        push_gradient_band(
            &mut rects,
            gradient.direction,
            area.x,
            area.y,
            area.width,
            area.height,
            0.0,
            first.position,
            first.color,
        );
        for pair in gradient.stops.windows(2) {
            push_gradient_band(
                &mut rects,
                gradient.direction,
                area.x,
                area.y,
                area.width,
                area.height,
                pair[0].position,
                pair[1].position,
                pair[0].color,
            );
        }
        let last = *gradient.stops.last().expect("checked length above");
        push_gradient_band(
            &mut rects,
            gradient.direction,
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

fn background_layers_for_gradient_paint(style: &ComputedStyle) -> Vec<css::BackgroundLayer> {
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

fn background_rect_area_for_box(
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

#[derive(Clone, Copy)]
struct BoxPaintGeometry {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    border_insets: css::Edges,
}

#[derive(Clone, Copy)]
struct BackgroundRectArea {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl BackgroundRectArea {
    fn inset(self, edges: css::Edges) -> Self {
        Self {
            x: self.x + edges.left,
            y: self.y + edges.bottom,
            width: (self.width - edges.left - edges.right).max(0.0),
            height: (self.height - edges.top - edges.bottom).max(0.0),
        }
    }
}

fn paint_box_shadows(
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
        if !color.is_visible() || shadow.blur_radius > 0.0 || !style.border_radius.is_zero() {
            continue;
        }
        if shadow.inset {
            paint_inset_box_shadow(rects, geometry, *shadow, color);
        } else {
            paint_outer_box_shadow(rects, geometry, *shadow, color);
        }
    }
}

fn paint_outer_box_shadow(
    rects: &mut Vec<RenderedRect>,
    geometry: BoxPaintGeometry,
    shadow: css::BoxShadow,
    color: Color,
) {
    let shadow_x = geometry.x + shadow.offset_x - shadow.spread;
    let shadow_y = geometry.y - shadow.offset_y - shadow.spread;
    let shadow_width = geometry.width + shadow.spread * 2.0;
    let shadow_height = geometry.height + shadow.spread * 2.0;
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

fn paint_inset_box_shadow(
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

    let spread = shadow.spread.max(0.0);
    let left = inset_shadow_edge_width(shadow.offset_x, spread, true).min(padding.width);
    let right = inset_shadow_edge_width(shadow.offset_x, spread, false).min(padding.width);
    let top = inset_shadow_edge_width(shadow.offset_y, spread, true).min(padding.height);
    let bottom = inset_shadow_edge_width(shadow.offset_y, spread, false).min(padding.height);

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

fn inset_shadow_edge_width(offset: f32, spread: f32, start_edge: bool) -> f32 {
    match (offset > 0.0, offset < 0.0, offset == 0.0 && spread > 0.0) {
        (true, _, _) if start_edge => offset + spread,
        (_, true, _) if !start_edge => -offset + spread,
        (_, _, true) => spread,
        _ => 0.0,
    }
}

fn push_rect_difference(
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

fn push_shadow_rect(
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

fn clip_gradient_rect(rect: &mut RenderedRect, clip: BackgroundRectArea) {
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
fn push_gradient_band(
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
    let axis_length = match direction {
        LinearGradientDirection::Bottom | LinearGradientDirection::Top => height,
        LinearGradientDirection::Right | LinearGradientDirection::Left => width,
    };
    let start = start.clamp(0.0, axis_length);
    let end = end.clamp(0.0, axis_length);
    if end <= start {
        return;
    }
    let rect = match direction {
        LinearGradientDirection::Bottom => RenderedRect::from_paint_rect(
            paint_space_rect(x, y + height - end, width, end - start),
            Some(color),
        ),
        LinearGradientDirection::Top => RenderedRect::from_paint_rect(
            paint_space_rect(x, y + start, width, end - start),
            Some(color),
        ),
        LinearGradientDirection::Right => RenderedRect::from_paint_rect(
            paint_space_rect(x + start, y, end - start, height),
            Some(color),
        ),
        LinearGradientDirection::Left => RenderedRect::from_paint_rect(
            paint_space_rect(x + width - end, y, end - start, height),
            Some(color),
        ),
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

fn uniform_rounded_ring_path(
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

fn solid_rounded_border_ring_path(
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
fn rounded_border_ring_between_path(
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

fn rounded_box_path_commands_for_insets(
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

fn rounded_border_side_clip(
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

fn rounded_border_pattern_clip(
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

fn rounded_border_ring_clip_path(
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

fn border_side_has_area(side: UsedBorderSide) -> bool {
    side.used_width > 0.0 && !side.style.suppresses_used_width()
}

fn is_clipped_rounded_side_style(style: BorderStyle) -> bool {
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

fn is_patterned_side_style(style: BorderStyle) -> bool {
    matches!(style, BorderStyle::Dashed | BorderStyle::Dotted)
}

fn border_insets(borders: UsedBorder) -> css::Edges {
    css::Edges {
        top: borders.top.used_width,
        right: borders.right.used_width,
        bottom: borders.bottom.used_width,
        left: borders.left.used_width,
    }
}

fn scaled_border_insets(borders: UsedBorder, scale: f32) -> css::Edges {
    let mut insets = border_insets(borders);
    insets.top *= scale;
    insets.right *= scale;
    insets.bottom *= scale;
    insets.left *= scale;
    insets
}

fn double_stripe_insets(borders: UsedBorder) -> css::Edges {
    css::Edges {
        top: double_stripe_width(borders.top.used_width),
        right: double_stripe_width(borders.right.used_width),
        bottom: double_stripe_width(borders.bottom.used_width),
        left: double_stripe_width(borders.left.used_width),
    }
}

fn double_inner_outer_insets(borders: UsedBorder) -> css::Edges {
    let full = border_insets(borders);
    let stripe = double_stripe_insets(borders);
    css::Edges {
        top: (full.top - stripe.top).max(0.0),
        right: (full.right - stripe.right).max(0.0),
        bottom: (full.bottom - stripe.bottom).max(0.0),
        left: (full.left - stripe.left).max(0.0),
    }
}

fn double_stripe_width(border_width: f32) -> f32 {
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
/// contour between the two radius tangent points. `round` uses the existing
/// quarter ellipse; `bevel`, `scoop`, and `notch` are represented with PDF
/// path segments matching their keyword geometry:
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
enum CornerPathKind {
    BottomRight,
    TopRight,
    TopLeft,
    BottomLeft,
}

fn append_corner_shape(
    commands: &mut Vec<RenderedPathCommand>,
    shape: css::CornerShape,
    start: (f32, f32),
    end: (f32, f32),
    inner: (f32, f32),
    radius: RenderedCornerRadius,
    kind: CornerPathKind,
) {
    const KAPPA: f32 = 0.552_284_8;
    if radius.x() <= 0.0 && radius.y() <= 0.0 {
        commands.push(RenderedPathCommand::line_to(paint_tuple_point(end)));
        return;
    }
    match shape {
        css::CornerShape::Round => append_round_corner(commands, start, end, radius, kind),
        css::CornerShape::Bevel => {
            commands.push(RenderedPathCommand::line_to(paint_tuple_point(end)))
        }
        css::CornerShape::Notch => {
            commands.push(RenderedPathCommand::line_to(paint_tuple_point(inner)));
            commands.push(RenderedPathCommand::line_to(paint_tuple_point(end)));
        }
        css::CornerShape::Scoop => {
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
    }
}

fn append_round_corner(
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

fn paint_tuple_point(point: (f32, f32)) -> PaintPoint {
    paint_space_point(point.0, point.1)
}

fn same_width(left: f32, right: f32) -> bool {
    (left - right).abs() < 0.01
}

fn inset_rounded_rect_radii(radii: &mut RenderedRoundedRectRadii, inset: f32) {
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
