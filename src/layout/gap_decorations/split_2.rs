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
    crossing_rule_width: f32,
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
            crossing_gap.size(),
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
    crossing_gap_width: f32,
    crossing_rule_width: f32,
    crossing_rule_can_paint: bool,
) -> GapRuleEndpoint {
    let crossing_segment_absent = context.container_kind == GapContainerKind::Grid
        && !crossing_rule_can_paint
        || grid_crossing_segment_present_at_junction(context, gap, crossing_gap)
            .is_some_and(|present| !present);
    if crossing_segment_absent {
        GapRuleEndpoint::cap(position)
    } else {
        GapRuleEndpoint::junction(position, crossing_gap_width, crossing_rule_width)
    }
}

pub(in crate::layout) fn crossing_rule_can_paint(
    width: f32,
    style: Option<BorderStyle>,
    color: Option<Color>,
) -> bool {
    width > GAP_RULE_EPSILON
        && !style.unwrap_or(BorderStyle::None).suppresses_used_width()
        && color.unwrap_or(Color::TRANSPARENT).is_visible()
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
    crossing_gaps.iter().copied().find(|gap| {
        gap.start <= position + GAP_RULE_EPSILON && gap.end >= position - GAP_RULE_EPSILON
    })
}

pub(in crate::layout) fn crossing_width_for_gap(
    context: AxisRuleContext<'_>,
    gap: GapBand,
) -> Option<f32> {
    context
        .crossing_gaps
        .iter()
        .position(|candidate| {
            (candidate.start - gap.start).abs() <= GAP_RULE_EPSILON
                && (candidate.end - gap.end).abs() <= GAP_RULE_EPSILON
        })
        .and_then(|index| {
            let crossing_gap_count = context.crossing_gaps.len();
            context
                .crossing_rule
                .widths
                .value_for_index(index, crossing_gap_count)
                .map(|width| used_gap_rule_length(width, gap.size()))
        })
}

pub(in crate::layout) fn crossing_can_paint_for_gap(
    context: AxisRuleContext<'_>,
    gap: GapBand,
) -> Option<bool> {
    context
        .crossing_gaps
        .iter()
        .position(|candidate| {
            (candidate.start - gap.start).abs() <= GAP_RULE_EPSILON
                && (candidate.end - gap.end).abs() <= GAP_RULE_EPSILON
        })
        .map(|index| {
            let crossing_gap_count = context.crossing_gaps.len();
            crossing_rule_can_paint(
                context
                    .crossing_rule
                    .widths
                    .value_for_index(index, crossing_gap_count)
                    .map(|width| used_gap_rule_length(width, gap.size()))
                    .unwrap_or(0.0),
                context
                    .crossing_rule
                    .styles
                    .value_for_index(index, crossing_gap_count),
                context
                    .crossing_rule
                    .colors
                    .value_for_index(index, crossing_gap_count),
            )
        })
}

pub(in crate::layout) fn offset_gap_rule_segment(
    rule: &css::GapRuleAxis,
    segment: GapDecorationSegment,
) -> GapDecorationSegment {
    let start_inset = used_gap_rule_endpoint_inset(
        match segment.start.kind {
            GapRuleEndpointKind::Cap => rule.inset_cap_start,
            GapRuleEndpointKind::Junction => rule.inset_junction_start,
        },
        segment.start,
    );
    let end_inset = used_gap_rule_endpoint_inset(
        match segment.end.kind {
            GapRuleEndpointKind::Cap => rule.inset_cap_end,
            GapRuleEndpointKind::Junction => rule.inset_junction_end,
        },
        segment.end,
    );
    GapDecorationSegment {
        start: GapRuleEndpoint {
            position: segment.start.position + start_inset,
            ..segment.start
        },
        end: GapRuleEndpoint {
            position: segment.end.position - end_inset,
            ..segment.end
        },
    }
}

pub(in crate::layout) fn used_gap_rule_endpoint_inset(
    value: css::GapRuleInsetValue,
    endpoint: GapRuleEndpoint,
) -> f32 {
    match value {
        css::GapRuleInsetValue::LengthPercentage(value) => {
            used_gap_rule_length(value, endpoint.crossing_gap_width)
        }
        css::GapRuleInsetValue::OverlapJoin if endpoint.kind == GapRuleEndpointKind::Junction => {
            -(endpoint.crossing_gap_width + endpoint.crossing_rule_width) * 0.5
        }
        css::GapRuleInsetValue::OverlapJoin => 0.0,
    }
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
        (_, _, css::GapRuleVisibilityItems::Normal)
            if context.container_kind == GapContainerKind::Grid =>
        {
            css::GapRuleVisibilityItems::All
        }
        (
            GapContainerKind::Multicol,
            GapRuleAxisKind::Column,
            css::GapRuleVisibilityItems::Normal,
        ) => css::GapRuleVisibilityItems::Between,
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
                    item.x >= gap.end - GAP_RULE_EPSILON
                } else {
                    item.x_end() <= gap.start + GAP_RULE_EPSILON
                };
                adjacent
                    && item.y < segment.end.position - GAP_RULE_EPSILON
                    && item.y_end() > segment.start.position + GAP_RULE_EPSILON
            }),
        GapRuleAxisKind::Row => grid_item_has_adjacent_area(context, *item, gap, segment, after)
            .unwrap_or_else(|| {
                let adjacent = if after {
                    item.y >= gap.end - GAP_RULE_EPSILON
                } else {
                    item.y_end() <= gap.start + GAP_RULE_EPSILON
                };
                adjacent
                    && item.x < segment.end.position - GAP_RULE_EPSILON
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

pub(in crate::layout) fn gap_rule_segment_primitives(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: f32,
    style: BorderStyle,
    color: Color,
) -> Vec<PaintPrimitive> {
    if width <= GAP_RULE_EPSILON || style.suppresses_used_width() || !color.is_visible() {
        return Vec::new();
    }
    match style {
        BorderStyle::Double if width >= 3.0 => {
            double_gap_rule_primitives(context, gap, segment, width, color)
        }
        BorderStyle::Groove | BorderStyle::Ridge => {
            groove_ridge_gap_rule_primitives(context, gap, segment, width, style, color)
        }
        BorderStyle::Inset | BorderStyle::Outset => {
            let edge = gap_rule_border_edge(context.kind, true);
            vec![solid_gap_rule_primitive(
                context,
                gap,
                segment,
                width,
                inset_outset_border_color(style, edge, color),
                None,
            )]
        }
        BorderStyle::Dotted => vec![solid_gap_rule_primitive(
            context,
            gap,
            segment,
            width,
            color,
            Some((width, width)),
        )],
        BorderStyle::Dashed => vec![solid_gap_rule_primitive(
            context,
            gap,
            segment,
            width,
            color,
            Some((width * 3.0, width * 3.0)),
        )],
        BorderStyle::Solid | BorderStyle::Double => {
            vec![solid_gap_rule_primitive(
                context, gap, segment, width, color, None,
            )]
        }
        BorderStyle::None | BorderStyle::Hidden => Vec::new(),
    }
}

pub(in crate::layout) fn double_gap_rule_primitives(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: f32,
    color: Color,
) -> Vec<PaintPrimitive> {
    let stripe = (width / 3.0).max(1.0);
    let offset = width / 3.0;
    vec![
        solid_gap_rule_primitive_with_cross_offset(context, gap, segment, stripe, color, -offset),
        solid_gap_rule_primitive_with_cross_offset(context, gap, segment, stripe, color, offset),
    ]
}

pub(in crate::layout) fn groove_ridge_gap_rule_primitives(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: f32,
    style: BorderStyle,
    color: Color,
) -> Vec<PaintPrimitive> {
    let (first, second) =
        groove_ridge_border_colors(style, gap_rule_border_edge(context.kind, true), color);
    let half = width / 2.0;
    vec![
        solid_gap_rule_primitive_with_cross_offset(context, gap, segment, half, first, -half / 2.0),
        solid_gap_rule_primitive_with_cross_offset(
            context,
            gap,
            segment,
            width - half,
            second,
            (width - half) / 2.0,
        ),
    ]
}

pub(in crate::layout) fn solid_gap_rule_primitive(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: f32,
    color: Color,
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
    width: f32,
    color: Color,
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
    width: f32,
    color: Color,
    cross_offset: f32,
    dash: Option<(f32, f32)>,
) -> PaintPrimitive {
    match context.kind {
        GapRuleAxisKind::Column => {
            let x = context.origin_x + gap.center() + cross_offset;
            PaintPrimitive::Stroke(RenderedStroke::new(
                x,
                context.content_top - segment.start.position,
                x,
                context.content_top - segment.end.position,
                width,
                color,
                dash,
            ))
        }
        GapRuleAxisKind::Row => {
            let y = context.content_top - gap.center() - cross_offset;
            PaintPrimitive::Stroke(RenderedStroke::new(
                context.origin_x + segment.start.position,
                y,
                context.origin_x + segment.end.position,
                y,
                width,
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
            GapRuleAxisKind::Column => self.block_size,
            GapRuleAxisKind::Row => self.inline_size,
        }
    }
}

pub(in crate::layout) fn used_gap_rule_length(
    value: css::ComputedLengthPercentage,
    percentage_basis: f32,
) -> f32 {
    value
        .used_length_with_percentage_basis(percentage_basis)
        .unwrap_or(value.length_with_percentage_basis(percentage_basis))
        .max(0.0)
}
