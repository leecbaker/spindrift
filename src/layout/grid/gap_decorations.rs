use super::*;

/// Return the used extent of a resolved grid axis for gap decoration painting.
///
/// Grid gap rules cover the resolved grid tracks, not free space remaining in
/// the grid container after fixed tracks have been laid out. Taffy's final
/// line offsets preserve that distinction, including content distribution and
/// implicit tracks; an empty offset record retains the container fallback.
/// <https://www.w3.org/TR/css-grid-1/#grid-definition> and
/// <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>
pub(super) fn grid_used_track_extent(
    line_offsets: &[f32],
    items: &[GridItemLayout],
    axis: GridAxis,
    fallback: f32,
) -> f32 {
    let line_extent = line_offsets.last().cloned().unwrap_or(0.0);
    let item_extent = items
        .iter()
        .filter(|item| item.area.is_some())
        .map(|item| match axis {
            GridAxis::Column => item.x() + item.width(),
            GridAxis::Row => item.y() + item.height(),
        })
        .fold(0.0_f32, f32::max);
    line_extent.max(item_extent).min(fallback).max(0.0)
}

pub(super) struct GridGapFragmentProjection<'a> {
    pub(super) style: &'a ComputedStyle,
    pub(super) content_origin: PageTopPoint,
    pub(super) inner_width: PhysicalContentWidth,
    pub(super) total_content_height: f32,
    pub(super) items: &'a [GapDecorationItem],
    pub(super) gutters: &'a GapDecorationGridGutters,
    pub(super) source_block_start: GridFragmentBlockOffset,
    pub(super) source_block_end: GridFragmentBlockOffset,
    pub(super) ends_at_fragment_break: bool,
}

pub(super) fn grid_gap_decoration_primitives_for_page(
    projection: GridGapFragmentProjection<'_>,
) -> Vec<PaintPrimitive> {
    let block_start = projection
        .source_block_start
        .points()
        .clamp(0.0, projection.total_content_height);
    let block_end = projection
        .source_block_end
        .points()
        .clamp(block_start, projection.total_content_height);
    let fragment_height = (block_end - block_start).max(0.0);
    if fragment_height <= 0.01 {
        return Vec::new();
    }

    // Segment rule geometry before fragment projection. In particular,
    // `rule-break` junctions must see neighboring tracks/items in their
    // source coordinate system; clipping those inputs first changes a
    // junction into a cap at the fragment boundary.
    let source_segments = grid_gap_rule_paint_segments(
        projection.style,
        GapDecorationContainer::new(
            projection.content_origin.x(),
            projection.content_origin.top_y(),
            projection.inner_width.points(),
            projection.total_content_height,
        ),
        projection.items,
        projection.gutters,
    );
    let page_container = GapDecorationContainer::new(
        projection.content_origin.x(),
        projection.content_origin.top_y(),
        projection.inner_width.points(),
        fragment_height,
    );
    source_segments
        .into_iter()
        .filter_map(|segment| {
            let crossing_gaps = match segment.kind {
                GapRuleAxisKind::Column => &projection.gutters.rows,
                GapRuleAxisKind::Row => &projection.gutters.columns,
            };
            project_grid_gap_rule_segment_to_block_range(
                segment,
                block_start,
                block_end,
                projection.ends_at_fragment_break,
                crossing_gaps,
            )
        })
        .flat_map(|segment| {
            grid_gap_rule_segment_primitives(projection.style, page_container, segment)
        })
        .collect()
}

/// Intersects a rule's centerline, rather than its already-expanded painted
/// area, with a committed grid fragment source range.
pub(super) fn project_grid_gap_rule_segment_to_block_range(
    mut rule_segment: GapRulePaintSegment,
    block_start: f32,
    block_end: f32,
    ends_at_fragment_break: bool,
    crossing_gaps: &[GapDecorationGutter],
) -> Option<GapRulePaintSegment> {
    match rule_segment.kind {
        GapRuleAxisKind::Column => {
            let source_start = rule_segment.segment.start.position;
            let source_end = rule_segment.segment.end.position;
            let fragment_content_start =
                grid_fragment_content_start_after_removed_cross_gap(block_start, crossing_gaps);
            let start = source_start.max(fragment_content_start);
            let end = source_end.min(block_end);
            if end <= start + GAP_RULE_EPSILON {
                return None;
            }
            let end = grid_fragment_terminal_rule_cap_end(
                source_end,
                end,
                block_end,
                rule_segment.width,
                ends_at_fragment_break,
                crossing_gaps,
            )
            .unwrap_or(end);
            let start = start - fragment_content_start;
            let end = end - fragment_content_start;
            if end <= start + GAP_RULE_EPSILON {
                return None;
            }
            rule_segment.segment.start = GapRuleEndpoint::cap(start);
            rule_segment.segment.end = GapRuleEndpoint::cap(end);
            Some(rule_segment)
        }
        GapRuleAxisKind::Row => {
            let center = rule_segment.gap.center();
            if center < block_start - GAP_RULE_EPSILON || center > block_end + GAP_RULE_EPSILON {
                return None;
            }
            if (rule_segment.gap.start - block_start).abs() <= GAP_RULE_EPSILON {
                return None;
            }
            let fragment_content_start =
                grid_fragment_content_start_after_removed_cross_gap(block_start, crossing_gaps);
            rule_segment.gap.start -= fragment_content_start;
            rule_segment.gap.end -= fragment_content_start;
            Some(rule_segment)
        }
    }
}

/// A cross gap that starts at a fragmentation break disappears from the next
/// fragment. Its following source content is rebased to the fragment-local
/// origin before rule geometry is expanded.
/// <https://drafts.csswg.org/css-gaps-1/#fragmentation>
fn grid_fragment_content_start_after_removed_cross_gap(
    block_start: f32,
    crossing_gaps: &[GapDecorationGutter],
) -> f32 {
    crossing_gaps
        .iter()
        .find(|gap| (gap.span.start - block_start).abs() <= GAP_RULE_EPSILON)
        .map_or(block_start, |gap| gap.span.end)
}

/// Returns the painted terminal cap at a fragmented source boundary. The
/// fragment's semantic segment is expanded only after source-range projection,
/// preserving the full square cap that belongs to its final painted endpoint.
/// <https://drafts.csswg.org/css-gaps-1/#fragmentation>
fn grid_fragment_terminal_rule_cap_end(
    source_segment_end: f32,
    projected_segment_end: f32,
    fragment_boundary: f32,
    rule_width: GapRuleWidth,
    ends_at_fragment_break: bool,
    crossing_gaps: &[GapDecorationGutter],
) -> Option<f32> {
    let source_segment_crosses_boundary =
        source_segment_end > projected_segment_end + GAP_RULE_EPSILON;
    let boundary_splits_cross_gap = crossing_gaps
        .iter()
        .any(|gap| (gap.span.start - projected_segment_end).abs() <= GAP_RULE_EPSILON);
    (ends_at_fragment_break
        && (projected_segment_end - fragment_boundary).abs() <= GAP_RULE_EPSILON
        && (source_segment_crosses_boundary || boundary_splits_cross_gap))
        .then(|| rule_width.extend_axis_position(projected_segment_end))
}

pub(super) fn grid_fragment_source_range_from_bounds(
    fragment_bounds: PaintClip,
    content_top: PageTopBlockPosition,
    total_content_height: f32,
) -> (GridFragmentBlockOffset, GridFragmentBlockOffset) {
    let fragment_top = fragment_bounds.y() + fragment_bounds.height();
    let block_start = (content_top.points() - fragment_top).clamp(0.0, total_content_height);
    let block_end =
        (content_top.points() - fragment_bounds.y()).clamp(block_start, total_content_height);
    (
        GridFragmentBlockOffset::new(block_start),
        GridFragmentBlockOffset::new(block_end),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_gap_decorations_project_from_committed_source_range() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(CssColor::new(255, 0, 0));
        let gutters = GapDecorationGridGutters {
            columns: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
            rows: vec![GapDecorationGutter::with_grid_line(80.0, 90.0, Some(2))],
        };
        let items = [
            GapDecorationItem::from_rect_with_grid_area(
                GapDecorationRect::new(
                    GapDecorationPoint::new(0.0, 0.0),
                    GapDecorationSize::new(50.0, 120.0),
                ),
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 3,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::from_rect_with_grid_area(
                GapDecorationRect::new(
                    GapDecorationPoint::new(60.0, 0.0),
                    GapDecorationSize::new(50.0, 120.0),
                ),
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 3,
                    column_start: 2,
                    column_end: 3,
                },
            ),
        ];

        let source_segments = grid_gap_rule_paint_segments(
            &style,
            GapDecorationContainer::new(0.0, 200.0, 110.0, 120.0),
            &items,
            &gutters,
        );
        assert_eq!(source_segments.len(), 1);
        assert_eq!(source_segments[0].segment.start.position, 0.0);
        assert_eq!(source_segments[0].segment.end.position, 120.0);

        let primitives = grid_gap_decoration_primitives_for_page(GridGapFragmentProjection {
            style: &style,
            content_origin: PageTopPoint::new(0.0, 200.0),
            inner_width: PhysicalContentWidth::new(content_box_pt(110.0)),
            total_content_height: 120.0,
            items: &items,
            gutters: &gutters,
            source_block_start: GridFragmentBlockOffset::new(50.0),
            source_block_end: GridFragmentBlockOffset::new(100.0),
            ends_at_fragment_break: true,
        });
        let strokes = solid_gap_rule_centerlines(&primitives);

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].x1(), 55.0);
        assert_eq!(strokes[0].y1(), 200.0);
        assert_eq!(strokes[0].y2(), 146.0);
        assert_eq!(strokes[0].stroke_width, PaintStrokeWidth::new(4.0));
    }

    #[test]
    fn fragmented_grid_gap_rule_expands_terminal_cap_after_projection() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(CssColor::new(0, 0, 255));
        let gutters = GapDecorationGridGutters {
            columns: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
            rows: Vec::new(),
        };

        let source_segments = grid_gap_rule_paint_segments(
            &style,
            GapDecorationContainer::new(0.0, 120.0, 110.0, 120.0),
            &[],
            &gutters,
        );
        assert_eq!(source_segments.len(), 1);
        assert_eq!(source_segments[0].segment.start.position, 0.0);
        assert_eq!(source_segments[0].segment.end.position, 120.0);

        let first =
            project_grid_gap_rule_segment_to_block_range(source_segments[0], 0.0, 100.0, true, &[])
                .expect("first fragment should contain the source rule");
        let second = project_grid_gap_rule_segment_to_block_range(
            source_segments[0],
            100.0,
            120.0,
            true,
            &[],
        )
        .expect("second fragment should contain the source rule");

        assert_eq!(first.segment.start.position, 0.0);
        assert_eq!(first.segment.end.position, 104.0);
        assert_eq!(second.segment.start.position, 0.0);
        assert_eq!(second.segment.end.position, 20.0);
    }
}
