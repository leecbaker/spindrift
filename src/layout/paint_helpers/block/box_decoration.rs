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
