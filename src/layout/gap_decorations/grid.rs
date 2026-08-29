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

pub(in crate::layout) fn segment_crosses_spanning_item(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    crossing_gap: GapBand,
    own_width: GapRuleWidth,
) -> bool {
    if context.items.is_empty() {
        return false;
    }
    let half = own_width.overlap_with_gap_half_extent(gap);
    context.items.iter().any(|item| match context.kind {
        GapRuleAxisKind::Column => grid_item_spans_intersection(
            context.container_kind,
            *item,
            gap.grid_line,
            crossing_gap.grid_line,
            GapRuleAxisKind::Column,
        )
        .unwrap_or_else(|| {
            item.rect.origin.x < gap.center() + half
                && item.x_end() > gap.center() - half
                && item.rect.origin.y <= crossing_gap.start + GAP_RULE_EPSILON
                && item.y_end() >= crossing_gap.end - GAP_RULE_EPSILON
        }),
        GapRuleAxisKind::Row => grid_item_spans_intersection(
            context.container_kind,
            *item,
            gap.grid_line,
            crossing_gap.grid_line,
            GapRuleAxisKind::Row,
        )
        .unwrap_or_else(|| {
            item.rect.origin.y < gap.center() + half
                && item.y_end() > gap.center() - half
                && item.rect.origin.x <= crossing_gap.start + GAP_RULE_EPSILON
                && item.x_end() >= crossing_gap.end - GAP_RULE_EPSILON
        }),
    })
}

pub(in crate::layout) fn grid_item_spans_intersection(
    container_kind: GapContainerKind,
    item: GapDecorationItem,
    gap_line: Option<u16>,
    crossing_gap_line: Option<u16>,
    axis: GapRuleAxisKind,
) -> Option<bool> {
    if container_kind != GapContainerKind::Grid {
        return None;
    }
    let area = item.grid_area?;
    let gap_line = gap_line?;
    let crossing_gap_line = crossing_gap_line?;
    let crosses_own_axis = match axis {
        GapRuleAxisKind::Column => area.column_start < gap_line && area.column_end > gap_line,
        GapRuleAxisKind::Row => area.row_start < gap_line && area.row_end > gap_line,
    };
    let crosses_cross_axis = match axis {
        GapRuleAxisKind::Column => {
            area.row_start < crossing_gap_line && area.row_end > crossing_gap_line
        }
        GapRuleAxisKind::Row => {
            area.column_start < crossing_gap_line && area.column_end > crossing_gap_line
        }
    };
    Some(crosses_own_axis && crosses_cross_axis)
}

pub(in crate::layout) fn grid_segment_is_flanked_by_spanning_items(
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
    let mut before_side = false;
    let mut after_side = false;
    let mut saw_grid_area = false;
    for area in context.items.iter().filter_map(|item| item.grid_area) {
        saw_grid_area = true;
        match context.kind {
            GapRuleAxisKind::Column if grid_area_spans_row_line(area, row_line) => {
                before_side |= area.column_start < column_line && area.column_end <= column_line;
                after_side |= area.column_start >= column_line && area.column_end > column_line;
            }
            GapRuleAxisKind::Row if grid_area_spans_column_line(area, column_line) => {
                before_side |= area.row_start < row_line && area.row_end <= row_line;
                after_side |= area.row_start >= row_line && area.row_end > row_line;
            }
            _ => {}
        }
    }
    saw_grid_area.then_some(before_side && after_side)
}

pub(in crate::layout) fn grid_area_spans_row_line(
    area: GapDecorationGridArea,
    row_line: u16,
) -> bool {
    area.row_start < row_line && area.row_end > row_line
}

pub(in crate::layout) fn grid_area_spans_column_line(
    area: GapDecorationGridArea,
    column_line: u16,
) -> bool {
    area.column_start < column_line && area.column_end > column_line
}

/// Whether joining the two segments adjacent to a grid gap junction would
/// paint through an item.
///
/// The `normal` break behavior joins two adjacent segments unless their union
/// is discontiguous. A union is discontiguous when its gap-width line segment
/// intersects an item. At a grid junction that means an item spanning the
/// decorated grid line in either track immediately adjacent to the crossing
/// line. <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
pub(in crate::layout) fn grid_junction_candidate_is_discontiguous(
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
    let mut saw_grid_area = false;
    let mut intersects_candidate = false;
    for area in context.items.iter().filter_map(|item| item.grid_area) {
        saw_grid_area = true;
        let intersects = match context.kind {
            GapRuleAxisKind::Column => {
                area.column_start < column_line
                    && area.column_end > column_line
                    && area.row_start <= row_line
                    && area.row_end >= row_line
            }
            GapRuleAxisKind::Row => {
                area.row_start < row_line
                    && area.row_end > row_line
                    && area.column_start <= column_line
                    && area.column_end >= column_line
            }
        };
        intersects_candidate |= intersects;
    }
    saw_grid_area.then_some(intersects_candidate)
}

/// Whether a grid gap-rule segment intersects a grid item.
///
/// CSS Gaps discards a segment whose gap-width line segment intersects a
/// child item. Grid-area line numbers make that test independent of physical
/// writing direction and of track-size rounding.
/// <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
pub(in crate::layout) fn grid_gap_rule_segment_is_discontiguous(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
) -> Option<bool> {
    if context.container_kind != GapContainerKind::Grid {
        return None;
    }
    let own_line = gap.grid_line?;
    let (cross_start, cross_end) = grid_segment_cross_axis_line_range(context, segment)?;
    let mut saw_grid_area = false;
    let mut intersects_item = false;
    for area in context.items.iter().filter_map(|item| item.grid_area) {
        saw_grid_area = true;
        intersects_item |= match context.kind {
            GapRuleAxisKind::Column => {
                area.column_start < own_line
                    && area.column_end > own_line
                    && area.row_start < cross_end
                    && area.row_end > cross_start
            }
            GapRuleAxisKind::Row => {
                area.row_start < own_line
                    && area.row_end > own_line
                    && area.column_start < cross_end
                    && area.column_end > cross_start
            }
        };
    }
    saw_grid_area.then_some(intersects_item)
}
