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

#[derive(Debug, Clone, Copy)]
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

/// Paints CSS Gap Decoration rules for resolved gutters in a grid container.
///
/// CSS Grid resolves explicit/implicit tracks and gutters before CSS Gaps
/// assigns decoration rules to the resulting gap sequence:
/// <https://www.w3.org/TR/css-grid-1/#track-sizing> and
/// <https://drafts.csswg.org/css-gaps-1/#assigning>.
pub(in crate::layout) fn grid_gap_decoration_primitives(
    style: &ComputedStyle,
    container: GapDecorationContainer,
    items: &[GapDecorationItem],
    gutters: &GapDecorationGridGutters,
) -> Vec<PaintPrimitive> {
    let column_gaps = gutters
        .columns
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let row_gaps = gutters
        .rows
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    gap_decoration_primitives_for_gaps(GapDecorationContext {
        style,
        container,
        column_gaps: &column_gaps,
        row_gaps: &row_gaps,
        items,
        container_kind: GapContainerKind::Grid,
    })
}

/// Resolves grid gap rules into source-coordinate centerline segments.
///
/// This is deliberately separate from primitive generation because a grid may
/// fragment after its complete track and item geometry has been resolved.
/// <https://drafts.csswg.org/css-gaps-1/#fragmentation>
pub(in crate::layout) fn grid_gap_rule_paint_segments(
    style: &ComputedStyle,
    container: GapDecorationContainer,
    items: &[GapDecorationItem],
    gutters: &GapDecorationGridGutters,
) -> Vec<GapRulePaintSegment> {
    let axis_mapped_style = GapRulePhysicalProjection::new(style).project_style(style);
    let style = &axis_mapped_style;
    let column_gaps = gutters
        .columns
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let row_gaps = gutters
        .rows
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let assigned_column_gaps = assign_gap_bands(&column_gaps);
    let assigned_row_gaps = assign_gap_bands(&row_gaps);
    let column_rules = axis_rule_paint_segments(AxisRuleContext {
        kind: GapRuleAxisKind::Column,
        container_kind: GapContainerKind::Grid,
        rule: &style.column_rule,
        crossing_rule: &style.row_rule,
        container,
        gaps: &assigned_column_gaps,
        crossing_gaps: &row_gaps,
        items,
    });
    let row_rules = axis_rule_paint_segments(AxisRuleContext {
        kind: GapRuleAxisKind::Row,
        container_kind: GapContainerKind::Grid,
        rule: &style.row_rule,
        crossing_rule: &style.column_rule,
        container,
        gaps: &assigned_row_gaps,
        crossing_gaps: &column_gaps,
        items,
    });
    match style.rule_overlap {
        css::GapRuleOverlap::RowOverColumn => column_rules.into_iter().chain(row_rules).collect(),
        css::GapRuleOverlap::ColumnOverRow => row_rules.into_iter().chain(column_rules).collect(),
    }
}

/// Expands a previously resolved grid segment into paint primitives.
pub(in crate::layout) fn grid_gap_rule_segment_primitives(
    style: &ComputedStyle,
    container: GapDecorationContainer,
    rule_segment: GapRulePaintSegment,
) -> Vec<PaintPrimitive> {
    let (rule, crossing_rule) = match rule_segment.kind {
        GapRuleAxisKind::Column => (&style.column_rule, &style.row_rule),
        GapRuleAxisKind::Row => (&style.row_rule, &style.column_rule),
    };
    gap_rule_segment_primitives_with_pattern_phase(
        AxisRuleContext {
            kind: rule_segment.kind,
            container_kind: GapContainerKind::Grid,
            rule,
            crossing_rule,
            container,
            gaps: &[],
            crossing_gaps: &[],
            items: &[],
        },
        rule_segment.gap,
        rule_segment.segment,
        rule_segment.width,
        rule_segment.style,
        rule_segment.color,
        rule_segment.pattern_phase,
    )
}

/// Projects Grid's final canonical track topology into paintable gutter bands.
///
/// The Grid layout owns `auto-fit` collapse provenance. Keeping this as a
/// projection rather than accepting Taffy's detailed gutter record ensures
/// gap-rule assignment happens after collapsed tracks and their gutters have
/// been removed from the used Grid topology.
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>
pub(in crate::layout) fn grid_gap_decoration_gutters_from_topologies(
    columns: &GridAxisTopology,
    rows: &GridAxisTopology,
    style: &ComputedStyle,
    content_width: f32,
    content_height: f32,
) -> GapDecorationGridGutters {
    GapDecorationGutters {
        columns: grid_axis_gutters_from_topology(
            columns,
            content_width,
            style.justify_content,
            style.direction == Direction::Rtl,
        ),
        rows: grid_axis_gutters_from_topology(rows, content_height, style.align_content, false),
    }
}

pub(in crate::layout) fn grid_axis_gutters_from_topology(
    topology: &GridAxisTopology,
    axis_size: f32,
    alignment: AlignContent,
    axis_is_reversed: bool,
) -> Vec<GapDecorationGutter> {
    let sizes = topology.track_sizes();
    let gutters = topology.interior_gutters();
    let collapsed_tracks = topology.collapsed_auto_fit_tracks();
    if sizes.is_empty()
        || gutters.len() != sizes.len().saturating_sub(1)
        || collapsed_tracks.len() != sizes.len()
    {
        return Vec::new();
    }
    let used_size = sizes.iter().chain(gutters.iter()).sum::<f32>();
    let free_space = axis_size - used_size;
    let track_count = sizes
        .iter()
        .enumerate()
        .filter(|(index, _)| !collapsed_tracks[*index])
        .count();
    let alignment = grid_alignment_fallback(free_space, track_count, alignment);
    let alignment = if axis_is_reversed {
        reverse_grid_alignment(alignment)
    } else {
        alignment
    };

    let mut cursor = 0.0;
    let mut seen_track = false;
    let mut bands = Vec::new();
    for (index, size) in sizes.iter().cloned().enumerate() {
        let gutter_size = index
            .checked_sub(1)
            .and_then(|gutter_index| gutters.get(gutter_index))
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        if index > 0 && gutter_size > GAP_RULE_EPSILON {
            bands.push(GapDecorationGutter::with_grid_line(
                cursor,
                cursor + gutter_size,
                Some((index + 1) as u16),
            ));
        }
        cursor += gutter_size;

        let is_track = !collapsed_tracks[index];
        if is_track {
            cursor += grid_alignment_offset(free_space, track_count, alignment, !seen_track);
            seen_track = true;
        }
        cursor += size.max(0.0);
    }
    bands
}

pub(in crate::layout) fn grid_alignment_fallback(
    free_space: f32,
    track_count: usize,
    alignment: AlignContent,
) -> ContentAlignmentKeyword {
    let mut keyword = grid_alignment_keyword(alignment.keyword);
    let mut safe = alignment.safety == AlignmentSafety::Safe;
    if track_count <= 1 || free_space <= 0.0 {
        (keyword, safe) = match keyword {
            ContentAlignmentKeyword::Stretch | ContentAlignmentKeyword::SpaceBetween => {
                (ContentAlignmentKeyword::FlexStart, true)
            }
            ContentAlignmentKeyword::SpaceAround | ContentAlignmentKeyword::SpaceEvenly => {
                (ContentAlignmentKeyword::Center, true)
            }
            other => (other, safe),
        };
    }
    if free_space <= 0.0 && safe {
        ContentAlignmentKeyword::Start
    } else {
        keyword
    }
}

pub(in crate::layout) fn grid_alignment_keyword(
    keyword: ContentAlignmentKeyword,
) -> ContentAlignmentKeyword {
    match keyword {
        ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch => {
            ContentAlignmentKeyword::Stretch
        }
        ContentAlignmentKeyword::Left | ContentAlignmentKeyword::FlexStart => {
            ContentAlignmentKeyword::FlexStart
        }
        ContentAlignmentKeyword::Right | ContentAlignmentKeyword::FlexEnd => {
            ContentAlignmentKeyword::FlexEnd
        }
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            ContentAlignmentKeyword::FlexStart
        }
        other => other,
    }
}

pub(in crate::layout) fn reverse_grid_alignment(
    keyword: ContentAlignmentKeyword,
) -> ContentAlignmentKeyword {
    match keyword {
        ContentAlignmentKeyword::Start => ContentAlignmentKeyword::End,
        ContentAlignmentKeyword::End => ContentAlignmentKeyword::Start,
        ContentAlignmentKeyword::FlexStart => ContentAlignmentKeyword::FlexEnd,
        ContentAlignmentKeyword::FlexEnd => ContentAlignmentKeyword::FlexStart,
        other => other,
    }
}

pub(in crate::layout) fn grid_alignment_offset(
    free_space: f32,
    track_count: usize,
    alignment: ContentAlignmentKeyword,
    is_first: bool,
) -> f32 {
    if track_count == 0 {
        return 0.0;
    }
    if is_first {
        match alignment {
            ContentAlignmentKeyword::Start
            | ContentAlignmentKeyword::FlexStart
            | ContentAlignmentKeyword::Stretch
            | ContentAlignmentKeyword::SpaceBetween => 0.0,
            ContentAlignmentKeyword::End | ContentAlignmentKeyword::FlexEnd => free_space,
            ContentAlignmentKeyword::Center => free_space / 2.0,
            ContentAlignmentKeyword::SpaceAround => {
                if free_space >= 0.0 {
                    (free_space / track_count as f32) / 2.0
                } else {
                    free_space / 2.0
                }
            }
            ContentAlignmentKeyword::SpaceEvenly => {
                if free_space >= 0.0 {
                    free_space / (track_count + 1) as f32
                } else {
                    free_space / 2.0
                }
            }
            ContentAlignmentKeyword::Normal
            | ContentAlignmentKeyword::Left
            | ContentAlignmentKeyword::Right
            | ContentAlignmentKeyword::Baseline
            | ContentAlignmentKeyword::LastBaseline => 0.0,
        }
    } else {
        let free_space = free_space.max(0.0);
        match alignment {
            ContentAlignmentKeyword::SpaceBetween if track_count > 1 => {
                free_space / (track_count - 1) as f32
            }
            ContentAlignmentKeyword::SpaceAround => free_space / track_count as f32,
            ContentAlignmentKeyword::SpaceEvenly => free_space / (track_count + 1) as f32,
            _ => 0.0,
        }
    }
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
