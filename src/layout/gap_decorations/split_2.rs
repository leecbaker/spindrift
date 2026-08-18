use super::*;

pub(in crate::layout) fn grid_area_occupies_quadrant(
    area: GapDecorationGridArea,
    column_line: u16,
    row_line: u16,
    after_column: bool,
    after_row: bool,
) -> bool {
    let occupies_column = if after_column {
        area.column_start <= column_line && area.column_end > column_line
    } else {
        area.column_start < column_line && area.column_end >= column_line
    };
    let occupies_row = if after_row {
        area.row_start <= row_line && area.row_end > row_line
    } else {
        area.row_start < row_line && area.row_end >= row_line
    };
    occupies_column && occupies_row
}

pub(in crate::layout) fn segment_start_endpoint(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    cursor: f32,
    crossing_gap: GapBand,
    crossing_rule_width: GapRuleWidth,
    crossing_rule_can_paint: bool,
) -> GapRuleEndpoint {
    if cursor <= GAP_RULE_EPSILON {
        GapRuleEndpoint::cap(0.0)
    } else {
        segment_junction_endpoint(
            context,
            gap,
            cursor,
            crossing_gap,
            crossing_rule_width,
            crossing_rule_can_paint,
        )
    }
}

pub(in crate::layout) fn segment_junction_endpoint(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    position: f32,
    crossing_gap: GapBand,
    crossing_rule_width: GapRuleWidth,
    crossing_rule_can_paint: bool,
) -> GapRuleEndpoint {
    // A geometric crossing is a junction only when its crossing decoration
    // exists. In particular, a hidden/zero-width flex rule leaves a cap;
    // its inset must not acquire `overlap-join` behavior merely because the
    // two resolved gutter rectangles meet.
    // <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
    let crossing_segment_absent = !crossing_rule_can_paint
        || grid_crossing_segment_present_at_junction(context, gap, crossing_gap)
            .is_some_and(|present| !present);
    if crossing_segment_absent {
        GapRuleEndpoint::cap(position)
    } else {
        GapRuleEndpoint::junction(position, crossing_gap, crossing_rule_width)
    }
}

pub(in crate::layout) fn crossing_rule_can_paint(
    width: GapRuleWidth,
    style: Option<BorderStyle>,
    color: Option<CssColor>,
) -> bool {
    width.can_paint()
        && !style.unwrap_or(BorderStyle::None).suppresses_used_width()
        && color.unwrap_or(CssColor::TRANSPARENT).is_visible()
}

pub(in crate::layout) fn grid_crossing_segment_present_at_junction(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    crossing_gap: GapBand,
) -> Option<bool> {
    if context.container_kind != GapContainerKind::Grid {
        return None;
    }
    let own_line = gap.grid_line?;
    let crossing_line = crossing_gap.grid_line?;
    let (column_line, row_line) = match context.kind {
        GapRuleAxisKind::Column => (own_line, crossing_line),
        GapRuleAxisKind::Row => (crossing_line, own_line),
    };
    let visibility = match context.crossing_rule.visibility_items {
        css::GapRuleVisibilityItems::Normal => css::GapRuleVisibilityItems::All,
        visibility => visibility,
    };
    if matches!(
        visibility,
        css::GapRuleVisibilityItems::All | css::GapRuleVisibilityItems::Normal
    ) {
        return Some(true);
    }

    let mut before_column_before_row = false;
    let mut after_column_before_row = false;
    let mut before_column_after_row = false;
    let mut after_column_after_row = false;
    let mut saw_grid_area = false;
    for area in context.items.iter().filter_map(|item| item.grid_area) {
        saw_grid_area = true;
        before_column_before_row |=
            grid_area_occupies_quadrant(area, column_line, row_line, false, false);
        after_column_before_row |=
            grid_area_occupies_quadrant(area, column_line, row_line, true, false);
        before_column_after_row |=
            grid_area_occupies_quadrant(area, column_line, row_line, false, true);
        after_column_after_row |=
            grid_area_occupies_quadrant(area, column_line, row_line, true, true);
    }
    if !saw_grid_area {
        return None;
    }

    let (before_side, after_side) = match context.kind {
        GapRuleAxisKind::Column => (
            before_column_before_row || after_column_before_row,
            before_column_after_row || after_column_after_row,
        ),
        GapRuleAxisKind::Row => (
            before_column_before_row || before_column_after_row,
            after_column_before_row || after_column_after_row,
        ),
    };
    Some(match visibility {
        css::GapRuleVisibilityItems::Around => before_side || after_side,
        css::GapRuleVisibilityItems::Between => before_side && after_side,
        css::GapRuleVisibilityItems::All | css::GapRuleVisibilityItems::Normal => true,
    })
}

pub(in crate::layout) fn nearest_crossing_gap(
    crossing_gaps: &[GapBand],
    position: f32,
) -> Option<GapBand> {
    crossing_gaps.iter().cloned().find(|gap| {
        gap.start <= position + GAP_RULE_EPSILON && gap.end >= position - GAP_RULE_EPSILON
    })
}

/// Whether a physical crossing portion reaches this gap's centerline.
///
/// Wrapped flex main-axis gaps are finite portions of otherwise aligned
/// physical bands. Their `segment_range` is therefore part of the junction
/// geometry, rather than paint-only clipping metadata. Grid and ordinary
/// multicol gaps leave it absent and remain full-span crossings.
pub(in crate::layout) fn crossing_portion_reaches_gap(gap: GapBand, crossing_gap: GapBand) -> bool {
    crossing_gap.segment_range.is_none_or(|range| {
        // A flex portion may terminate at a crossing gap's edge. That shared
        // boundary is still a CSS Gaps junction, even though the crossing
        // rule's centerline lies inside the row/column gutter rather than in
        // the portion's line box.
        range.start <= gap.end + GAP_RULE_EPSILON && range.end >= gap.start - GAP_RULE_EPSILON
    })
}

pub(in crate::layout) fn crossing_width_for_gap(
    context: AxisRuleContext<'_>,
    gap: GapBand,
) -> Option<GapRuleWidth> {
    crossing_rule_slot_for_gap(context, gap).and_then(|(index, count)| {
        context
            .crossing_rule
            .widths
            .value_for_index(index, count)
            .map(|width| {
                used_gap_rule_width(width, PercentageBasis::definite(layout_pt(gap.size())))
            })
    })
}

pub(in crate::layout) fn crossing_can_paint_for_gap(
    context: AxisRuleContext<'_>,
    gap: GapBand,
) -> Option<bool> {
    crossing_rule_slot_for_gap(context, gap).map(|(index, count)| {
        crossing_rule_can_paint(
            context
                .crossing_rule
                .widths
                .value_for_index(index, count)
                .map(|width| {
                    used_gap_rule_width(width, PercentageBasis::definite(layout_pt(gap.size())))
                })
                .unwrap_or(GapRuleWidth::ZERO),
            context.crossing_rule.styles.value_for_index(index, count),
            context.crossing_rule.colors.value_for_index(index, count),
        )
    })
}

/// Resolve a crossing band to the value-list position committed by its layout
/// topology. Flex portions can share a physical coordinate while consuming
/// different sequential slots, so their vector position is not an assignment
/// identity.
fn crossing_rule_slot_for_gap(
    context: AxisRuleContext<'_>,
    gap: GapBand,
) -> Option<(usize, usize)> {
    let sequence_len = context
        .crossing_gaps
        .iter()
        .enumerate()
        .map(|(physical_index, candidate)| candidate.rule_index.unwrap_or(physical_index))
        .max()
        .map(|last_index| last_index + 1)?;
    context
        .crossing_gaps
        .iter()
        .enumerate()
        .find(|(_, candidate)| {
            (candidate.start - gap.start).abs() <= GAP_RULE_EPSILON
                && (candidate.end - gap.end).abs() <= GAP_RULE_EPSILON
                && candidate.segment_range == gap.segment_range
        })
        .map(|(physical_index, candidate)| {
            (candidate.rule_index.unwrap_or(physical_index), sequence_len)
        })
}

pub(in crate::layout) fn offset_gap_rule_segment(
    rule: &css::GapRuleAxis,
    segment: GapDecorationSegment,
) -> GapDecorationSegment {
    let start_inset = used_gap_rule_endpoint_inset(
        match segment.start.kind {
            GapRuleEndpointKind::Cap => rule.inset_cap_start.clone(),
            GapRuleEndpointKind::Junction(_) => rule.inset_junction_start.clone(),
        },
        segment.start,
    );
    let end_inset = used_gap_rule_endpoint_inset(
        match segment.end.kind {
            GapRuleEndpointKind::Cap => rule.inset_cap_end.clone(),
            GapRuleEndpointKind::Junction(_) => rule.inset_junction_end.clone(),
        },
        segment.end,
    );
    GapDecorationSegment {
        start: GapRuleEndpoint {
            position: segment.start.position + start_inset,
            kind: segment.start.kind,
        },
        end: GapRuleEndpoint {
            position: segment.end.position - end_inset,
            kind: segment.end.kind,
        },
    }
}

pub(in crate::layout) fn used_gap_rule_endpoint_inset(
    value: css::GapRuleInsetValue,
    endpoint: GapRuleEndpoint,
) -> f32 {
    match value {
        css::GapRuleInsetValue::LengthPercentage(value) => match endpoint.kind {
            GapRuleEndpointKind::Cap => value.length_points(),
            GapRuleEndpointKind::Junction(junction) => value
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                    junction.crossing_gap.size(),
                )))
                .map(layout_points)
                .unwrap_or_else(|| value.length_points()),
        },
        css::GapRuleInsetValue::OverlapJoin
            if matches!(endpoint.kind, GapRuleEndpointKind::Junction(_)) =>
        {
            match endpoint.kind {
                GapRuleEndpointKind::Junction(junction) => junction
                    .crossing_rule_width
                    .overlap_join_inset(junction.crossing_gap),
                GapRuleEndpointKind::Cap => unreachable!("junction match retains junction data"),
            }
        }
        css::GapRuleInsetValue::OverlapJoin => 0.0,
    }
}

/// Converts the filled rectangles used for solid gap rules to the centerline
/// geometry asserted by layout tests. Solid rules deliberately paint as areas,
/// rather than strokes, so cap geometry is independent of backend stroke
/// rasterization.
#[cfg(test)]
pub(in crate::layout) fn solid_gap_rule_centerlines(
    primitives: &[PaintPrimitive],
) -> Vec<RenderedStroke> {
    primitives
        .iter()
        .filter_map(|primitive| {
            let PaintPrimitive::Rect(rect) = primitive else {
                return None;
            };
            let color = rect.fill?;
            if rect.width() < rect.height() {
                Some(RenderedStroke::new(
                    rect.x() + rect.width() / 2.0,
                    rect.y() + rect.height(),
                    rect.x() + rect.width() / 2.0,
                    rect.y(),
                    PaintStrokeWidth::new(rect.width()),
                    color,
                    None,
                ))
            } else if rect.height() < rect.width() {
                Some(RenderedStroke::new(
                    rect.x(),
                    rect.y() + rect.height() / 2.0,
                    rect.x() + rect.width(),
                    rect.y() + rect.height() / 2.0,
                    PaintStrokeWidth::new(rect.height()),
                    color,
                    None,
                ))
            } else {
                None
            }
        })
        .collect()
}

pub(in crate::layout) fn segment_is_visible(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
) -> bool {
    match effective_visibility_items(context) {
        css::GapRuleVisibilityItems::All => true,
        css::GapRuleVisibilityItems::Around => {
            segment_has_adjacent_item(context, gap, segment, false)
                || segment_has_adjacent_item(context, gap, segment, true)
        }
        css::GapRuleVisibilityItems::Between => {
            segment_has_adjacent_item(context, gap, segment, false)
                && segment_has_adjacent_item(context, gap, segment, true)
        }
        css::GapRuleVisibilityItems::Normal => true,
    }
}

pub(in crate::layout) fn effective_visibility_items(
    context: AxisRuleContext<'_>,
) -> css::GapRuleVisibilityItems {
    match (
        context.container_kind,
        context.kind,
        context.rule.visibility_items,
    ) {
        // `*-rule-visibility-items` does not apply to flex containers, so
        // its initial `normal` value must not make flex decoration painting
        // depend on incidental item-rectangle metadata. Flex layout has
        // already materialized only resolved gutters between flex items or
        // lines at this boundary.
        // <https://drafts.csswg.org/css-gaps-1/#gap-rule-visibility>
        (GapContainerKind::Flex, _, css::GapRuleVisibilityItems::Normal) => {
            css::GapRuleVisibilityItems::All
        }
        (_, _, css::GapRuleVisibilityItems::Normal)
            if context.container_kind == GapContainerKind::Grid =>
        {
            css::GapRuleVisibilityItems::All
        }
        (
            GapContainerKind::Multicol,
            GapRuleAxisKind::Column,
            css::GapRuleVisibilityItems::Normal,
        ) => {
            // Multicol callers materialize only gutters whose adjacent
            // anonymous columns both received content. Unlike grid/flex they
            // do not have item rectangles to re-derive that fact here.
            // <https://www.w3.org/TR/css-multicol-1/#column-gaps-and-rules>
            css::GapRuleVisibilityItems::All
        }
        (GapContainerKind::Multicol, GapRuleAxisKind::Row, css::GapRuleVisibilityItems::Normal) => {
            css::GapRuleVisibilityItems::All
        }
        (_, _, css::GapRuleVisibilityItems::Normal) => css::GapRuleVisibilityItems::Between,
        (_, _, visibility) => visibility,
    }
}

pub(in crate::layout) fn segment_has_adjacent_item(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    after: bool,
) -> bool {
    if context.items.is_empty() {
        return true;
    }
    context.items.iter().any(|item| match context.kind {
        GapRuleAxisKind::Column => grid_item_has_adjacent_area(context, *item, gap, segment, after)
            .unwrap_or_else(|| {
                let adjacent = if after {
                    item.rect.origin.x >= gap.end - GAP_RULE_EPSILON
                } else {
                    item.x_end() <= gap.start + GAP_RULE_EPSILON
                };
                adjacent
                    && item.rect.origin.y < segment.end.position - GAP_RULE_EPSILON
                    && item.y_end() > segment.start.position + GAP_RULE_EPSILON
            }),
        GapRuleAxisKind::Row => grid_item_has_adjacent_area(context, *item, gap, segment, after)
            .unwrap_or_else(|| {
                let adjacent = if after {
                    item.rect.origin.y >= gap.end - GAP_RULE_EPSILON
                } else {
                    item.y_end() <= gap.start + GAP_RULE_EPSILON
                };
                adjacent
                    && item.rect.origin.x < segment.end.position - GAP_RULE_EPSILON
                    && item.x_end() > segment.start.position + GAP_RULE_EPSILON
            }),
    })
}

pub(in crate::layout) fn grid_item_has_adjacent_area(
    context: AxisRuleContext<'_>,
    item: GapDecorationItem,
    gap: GapBand,
    segment: GapDecorationSegment,
    after: bool,
) -> Option<bool> {
    if context.container_kind != GapContainerKind::Grid {
        return None;
    }
    let area = item.grid_area?;
    let gap_line = gap.grid_line?;
    let (segment_start_line, segment_end_line) =
        grid_segment_cross_axis_line_range(context, segment)?;
    let adjacent = match context.kind {
        GapRuleAxisKind::Column if after => area.column_end > gap_line,
        GapRuleAxisKind::Column => area.column_start < gap_line,
        GapRuleAxisKind::Row if after => area.row_end > gap_line,
        GapRuleAxisKind::Row => area.row_start < gap_line,
    };
    if !adjacent {
        return Some(false);
    }
    let overlaps_segment = match context.kind {
        GapRuleAxisKind::Column => {
            area.row_start < segment_end_line && area.row_end > segment_start_line
        }
        GapRuleAxisKind::Row => {
            area.column_start < segment_end_line && area.column_end > segment_start_line
        }
    };
    Some(overlaps_segment)
}

pub(in crate::layout) fn grid_segment_cross_axis_line_range(
    context: AxisRuleContext<'_>,
    segment: GapDecorationSegment,
) -> Option<(u16, u16)> {
    let start_line = if segment.start.position <= GAP_RULE_EPSILON {
        Some(1)
    } else {
        grid_line_for_crossing_position(context.crossing_gaps, segment.start.position)
    }?;
    let end_line = if segment.end.position >= context.axis_size() - GAP_RULE_EPSILON {
        grid_cross_axis_end_line(context)
    } else {
        grid_line_for_crossing_position(context.crossing_gaps, segment.end.position)
    }?;
    (end_line > start_line).then_some((start_line, end_line))
}

pub(in crate::layout) fn grid_line_for_crossing_position(
    crossing_gaps: &[GapBand],
    position: f32,
) -> Option<u16> {
    nearest_crossing_gap(crossing_gaps, position).and_then(|gap| gap.grid_line)
}

pub(in crate::layout) fn grid_cross_axis_end_line(context: AxisRuleContext<'_>) -> Option<u16> {
    let gap_line = context
        .crossing_gaps
        .iter()
        .filter_map(|gap| gap.grid_line)
        .max()
        .map(|line| line + 1);
    let item_line = context
        .items
        .iter()
        .filter_map(|item| item.grid_area)
        .map(|area| match context.kind {
            GapRuleAxisKind::Column => area.row_end,
            GapRuleAxisKind::Row => area.column_end,
        })
        .max();
    match (gap_line, item_line) {
        (Some(gap_line), Some(item_line)) => Some(gap_line.max(item_line)),
        (Some(line), None) | (None, Some(line)) => Some(line),
        (None, None) => None,
    }
}

/// Expand a gap-rule segment while retaining its logical patterned-rule
/// phase. The phase is only observable for dotted/dashed rules; solid and
/// shaded rules intentionally share the ordinary primitive path.
pub(in crate::layout) fn gap_rule_segment_primitives_with_pattern_phase(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    style: BorderStyle,
    color: CssColor,
    pattern_phase: f32,
) -> Vec<PaintPrimitive> {
    if !width.can_paint() || style.suppresses_used_width() || !color.is_visible() {
        return Vec::new();
    }
    match style {
        BorderStyle::Double if width.double_bands().is_some() => {
            double_gap_rule_primitives(context, gap, segment, width, color)
        }
        BorderStyle::Groove | BorderStyle::Ridge => {
            groove_ridge_gap_rule_primitives(context, gap, segment, width, style, color)
        }
        BorderStyle::Inset | BorderStyle::Outset => groove_ridge_gap_rule_primitives(
            context,
            gap,
            segment,
            width,
            if style == BorderStyle::Inset {
                BorderStyle::Ridge
            } else {
                BorderStyle::Groove
            },
            color,
        ),
        BorderStyle::Dotted | BorderStyle::Dashed => {
            patterned_gap_rule_primitives(context, gap, segment, width, style, color, pattern_phase)
        }
        BorderStyle::Solid | BorderStyle::Double => {
            vec![solid_gap_rule_primitive(
                context, gap, segment, width, color, None,
            )]
        }
        BorderStyle::None | BorderStyle::Hidden => Vec::new(),
    }
}

fn patterned_gap_rule_primitives(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    style: BorderStyle,
    color: CssColor,
    pattern_phase: f32,
) -> Vec<PaintPrimitive> {
    let (axis_start, axis_length, cross_start, horizontal) = match context.kind {
        GapRuleAxisKind::Column => (
            context.container.page_rect.top_y() - segment.end.position,
            segment.end.position - segment.start.position,
            width
                .centered_span(context.container.page_rect.x() + gap.center())
                .start,
            false,
        ),
        GapRuleAxisKind::Row => (
            context.container.page_rect.x() + segment.start.position,
            segment.end.position - segment.start.position,
            width
                .centered_span(context.container.page_rect.top_y() - gap.center())
                .start,
            true,
        ),
    };
    // Preserve the established native border-side painter for an unbroken
    // gap. Besides being cheaper, it is the reference rendering for CSS
    // dotted/dashed caps. The phased path below is only needed when one gap
    // has been split into several portions at an intersection.
    if pattern_phase.abs() <= GAP_RULE_EPSILON {
        if style == BorderStyle::Dotted {
            let mut paths = Vec::new();
            paint_dotted_border_side(
                &mut paths,
                axis_start,
                axis_length,
                cross_start,
                width.into_paint_stroke_width(),
                horizontal,
                color,
            );
            return paths.into_iter().map(PaintPrimitive::Path).collect();
        }
        let mut rects = Vec::new();
        paint_dashed_border_side(
            &mut rects,
            axis_start,
            axis_length,
            cross_start,
            width.into_paint_stroke_width(),
            horizontal,
            width.into_paint_stroke_width(),
            color,
        );
        return rects.into_iter().map(PaintPrimitive::Rect).collect();
    }
    // `axis_start` is the physical paint projection of the portion's logical
    // start.  Keep the sequence phase in that same start-relative direction;
    // the page-coordinate inversion for column rules has already happened
    // while constructing `axis_start` above.
    let phase_at_paint_start = pattern_phase;
    let stroke_width = width.into_paint_stroke_width().points();
    if style == BorderStyle::Dotted {
        let pitch = stroke_width * 2.0;
        let mut center = axis_start + stroke_width * 0.5 - phase_at_paint_start.rem_euclid(pitch);
        let axis_end = axis_start + axis_length;
        let mut paths = Vec::new();
        while center < axis_end - GAP_RULE_EPSILON {
            if center >= axis_start - stroke_width * 0.5 {
                let (x, y) = if horizontal {
                    (center, cross_start + stroke_width * 0.5)
                } else {
                    (cross_start + stroke_width * 0.5, center)
                };
                paths.push(circle_path(x, y, stroke_width * 0.5, color, None));
            }
            center += pitch;
        }
        paths.into_iter().map(PaintPrimitive::Path).collect()
    } else {
        let dash_length = (stroke_width * 3.0).max(1.0);
        let period = dash_length * 2.0;
        let axis_end = axis_start + axis_length;
        let mut dash_start = axis_start - phase_at_paint_start.rem_euclid(period);
        let mut rects = Vec::new();
        while dash_start < axis_end - GAP_RULE_EPSILON {
            let start = dash_start.max(axis_start);
            let end = (dash_start + dash_length).min(axis_end);
            if end > start + GAP_RULE_EPSILON {
                let (x, y, width, height) = if horizontal {
                    (start, cross_start, end - start, stroke_width)
                } else {
                    (cross_start, start, stroke_width, end - start)
                };
                // Patterned gap rules are not border-side boxes; their
                // portions have already been clipped by topology resolution.
                // Emit the remaining dash rectangle directly in paint space.
                rects.push(RenderedRect::new(
                    x,
                    y,
                    width,
                    height,
                    Some(color),
                    None,
                    PaintStrokeWidth::ZERO,
                ));
            }
            dash_start += period;
        }
        rects.into_iter().map(PaintPrimitive::Rect).collect()
    }
}

pub(in crate::layout) fn double_gap_rule_primitives(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    color: CssColor,
) -> Vec<PaintPrimitive> {
    // A gap rule has a centerline, unlike a particular side of a border box.
    // Reusing `paint_border_side` here puts the second `double` stripe on the
    // outside of its selected box side, which can move it out of the gap.
    // Keep both stripes symmetric around the rule centerline instead.
    // <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>
    let stripe = GapRuleWidth::new(
        width
            .double_bands()
            .expect("double gap-rule paint requires a double-band width")
            .stripe
            .get(),
    );
    let offset = width.center_offset() - stripe.center_offset();
    vec![
        solid_gap_rule_primitive_with_cross_offset(context, gap, segment, stripe, color, -offset),
        solid_gap_rule_primitive_with_cross_offset(context, gap, segment, stripe, color, offset),
    ]
}

pub(in crate::layout) fn groove_ridge_gap_rule_primitives(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    style: BorderStyle,
    color: CssColor,
) -> Vec<PaintPrimitive> {
    let (first, second) =
        groove_ridge_border_colors(style, gap_rule_border_edge(context.kind, true), color);
    let half = width.half();
    vec![
        solid_gap_rule_primitive_with_cross_offset(
            context,
            gap,
            segment,
            half,
            first,
            -half.center_offset(),
        ),
        solid_gap_rule_primitive_with_cross_offset(
            context,
            gap,
            segment,
            width.remainder_after(half),
            second,
            width.remainder_after(half).center_offset(),
        ),
    ]
}

pub(in crate::layout) fn solid_gap_rule_primitive(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    color: CssColor,
    dash: Option<(f32, f32)>,
) -> PaintPrimitive {
    solid_gap_rule_primitive_with_cross_offset_and_dash(
        context, gap, segment, width, color, 0.0, dash,
    )
}

pub(in crate::layout) fn solid_gap_rule_primitive_with_cross_offset(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    color: CssColor,
    cross_offset: f32,
) -> PaintPrimitive {
    solid_gap_rule_primitive_with_cross_offset_and_dash(
        context,
        gap,
        segment,
        width,
        color,
        cross_offset,
        None,
    )
}

pub(in crate::layout) fn solid_gap_rule_primitive_with_cross_offset_and_dash(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    color: CssColor,
    cross_offset: f32,
    dash: Option<(f32, f32)>,
) -> PaintPrimitive {
    // A solid gap rule is an opaque rectangular rule area. Representing it as
    // a stroked centerline makes cap rasterization and coincident endpoints
    // renderer-dependent, especially after negative insets join segments.
    // Patterned rules remain strokes because dash phase is part of their
    // painting model.
    // <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>
    if dash.is_none() {
        return match context.kind {
            GapRuleAxisKind::Column => {
                let span = width
                    .centered_span(context.container.page_rect.x() + gap.center() + cross_offset);
                let rect = RenderedRect::new(
                    span.start,
                    context.container.page_rect.top_y() - segment.end.position,
                    span.size(),
                    (segment.end.position - segment.start.position).max(0.0),
                    Some(color),
                    None,
                    PaintStrokeWidth::ZERO,
                );
                // A multicolumn rule paints over its container background.
                // Mark this opaque cover so PDF serialization can remove the
                // fully hidden underpaint before rasterization exposes it at
                // the rule's terminal edge.
                // <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>
                PaintPrimitive::Rect(if context.container_kind == GapContainerKind::Multicol {
                    rect.with_opaque_underpaint_culling()
                } else {
                    rect
                })
            }
            GapRuleAxisKind::Row => {
                let span = width.centered_span(
                    context.container.page_rect.top_y() - gap.center() - cross_offset,
                );
                PaintPrimitive::Rect(RenderedRect::new(
                    context.container.page_rect.x() + segment.start.position,
                    span.start,
                    (segment.end.position - segment.start.position).max(0.0),
                    span.size(),
                    Some(color),
                    None,
                    PaintStrokeWidth::ZERO,
                ))
            }
        };
    }
    match context.kind {
        GapRuleAxisKind::Column => {
            let x = context.container.page_rect.x() + gap.center() + cross_offset;
            PaintPrimitive::Stroke(RenderedStroke::new(
                x,
                context.container.page_rect.top_y() - segment.start.position,
                x,
                context.container.page_rect.top_y() - segment.end.position,
                width.into_paint_stroke_width(),
                color,
                dash,
            ))
        }
        GapRuleAxisKind::Row => {
            let y = context.container.page_rect.top_y() - gap.center() - cross_offset;
            PaintPrimitive::Stroke(RenderedStroke::new(
                context.container.page_rect.x() + segment.start.position,
                y,
                context.container.page_rect.x() + segment.end.position,
                y,
                width.into_paint_stroke_width(),
                color,
                dash,
            ))
        }
    }
}

pub(in crate::layout) fn gap_rule_border_edge(
    kind: GapRuleAxisKind,
    first_half: bool,
) -> BorderEdge {
    match (kind, first_half) {
        (GapRuleAxisKind::Column, true) => BorderEdge::Left,
        (GapRuleAxisKind::Column, false) => BorderEdge::Right,
        (GapRuleAxisKind::Row, true) => BorderEdge::Top,
        (GapRuleAxisKind::Row, false) => BorderEdge::Bottom,
    }
}

impl AxisRuleContext<'_> {
    pub(in crate::layout) fn axis_size(&self) -> f32 {
        match self.kind {
            GapRuleAxisKind::Column => self.container.local_size.height,
            GapRuleAxisKind::Row => self.container.local_size.width,
        }
    }
}

pub(in crate::layout) fn used_gap_rule_width<T, Source>(
    value: css::ComputedLengthPercentage,
    percentage_basis: PercentageBasis<T, Source>,
) -> GapRuleWidth
where
    T: SemanticLengthExt,
{
    GapRuleWidth::new(
        value
            .used_length_with_percentage_basis(percentage_basis)
            .map(layout_points)
            .unwrap_or(value.length_points()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junction_insets_retain_crossing_gap_geometry() {
        let crossing_gap = GapBand {
            start: 10.0,
            end: 30.0,
            grid_line: None,
            segment_range: None,
            rule_index: None,
        };
        let junction = GapRuleEndpoint::junction(20.0, crossing_gap, GapRuleWidth::new(4.0));
        let percentage = css::GapRuleInsetValue::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );

        assert_eq!(
            used_gap_rule_endpoint_inset(percentage.clone(), junction),
            10.0
        );
        assert_eq!(
            used_gap_rule_endpoint_inset(css::GapRuleInsetValue::OverlapJoin, junction),
            -12.0
        );

        let cap = GapRuleEndpoint::cap(20.0);
        assert_eq!(used_gap_rule_endpoint_inset(percentage, cap), 0.0);
        assert_eq!(
            used_gap_rule_endpoint_inset(css::GapRuleInsetValue::OverlapJoin, cap),
            0.0
        );
    }
}
