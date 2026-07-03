use super::*;

pub(in crate::layout) const GAP_RULE_EPSILON: f32 = 0.01;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapDecorationGutter {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) end: f32,
    pub(in crate::layout) grid_line: Option<u16>,
}

impl GapDecorationGutter {
    pub(in crate::layout) fn new(start: f32, end: f32) -> Self {
        Self::with_grid_line(start, end, None)
    }

    pub(in crate::layout) fn with_grid_line(start: f32, end: f32, grid_line: Option<u16>) -> Self {
        Self {
            start: start.max(0.0),
            end: end.max(start).max(0.0),
            grid_line,
        }
    }

    pub(in crate::layout) fn clipped_to_block_range(
        self,
        block_start: f32,
        block_end: f32,
    ) -> Option<Self> {
        let start = self.start.max(block_start);
        let end = self.end.min(block_end);
        (end > start + GAP_RULE_EPSILON)
            .then(|| Self::with_grid_line(start - block_start, end - block_start, self.grid_line))
    }
}

#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct GapDecorationGutters {
    pub(in crate::layout) columns: Vec<GapDecorationGutter>,
    pub(in crate::layout) rows: Vec<GapDecorationGutter>,
}

pub(in crate::layout) type GapDecorationGridGutters = GapDecorationGutters;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapDecorationItem {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
    pub(in crate::layout) grid_area: Option<GapDecorationGridArea>,
}

impl GapDecorationItem {
    pub(in crate::layout) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
            grid_area: None,
        }
    }

    pub(in crate::layout) fn with_grid_area(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        grid_area: GapDecorationGridArea,
    ) -> Self {
        Self {
            grid_area: Some(grid_area),
            ..Self::new(x, y, width, height)
        }
    }

    pub(in crate::layout) fn x_end(self) -> f32 {
        self.x + self.width
    }

    pub(in crate::layout) fn y_end(self) -> f32 {
        self.y + self.height
    }

    pub(in crate::layout) fn clipped_to_block_range(
        self,
        block_start: f32,
        block_end: f32,
    ) -> Option<Self> {
        let start = self.y.max(block_start);
        let end = self.y_end().min(block_end);
        (end > start + GAP_RULE_EPSILON).then_some(Self {
            y: start - block_start,
            height: end - start,
            ..self
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapDecorationGridArea {
    pub(in crate::layout) row_start: u16,
    pub(in crate::layout) row_end: u16,
    pub(in crate::layout) column_start: u16,
    pub(in crate::layout) column_end: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GapContainerKind {
    Flex,
    Grid,
    Multicol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GapRuleAxisKind {
    Column,
    Row,
}

/// Paints CSS Gap Decoration rules for resolved flex gutters.
///
/// Flex layout resolves gaps from line and item placement after wrapping and
/// alignment. Supplying that metadata avoids treating unrelated item bands as
/// synthetic gutters:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-lines> and
/// <https://drafts.csswg.org/css-gaps-1/#segments>.
pub(in crate::layout) fn flex_gap_decoration_primitives_with_gutters(
    style: &ComputedStyle,
    origin_x: f32,
    content_top: f32,
    content_width: f32,
    content_height: f32,
    items: &[GapDecorationItem],
    gutters: &GapDecorationGutters,
) -> Vec<PaintPrimitive> {
    let column_gaps = gutters
        .columns
        .iter()
        .copied()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let row_gaps = gutters
        .rows
        .iter()
        .copied()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    gap_decoration_primitives_for_gaps(GapDecorationContext {
        style,
        origin_x,
        content_top,
        content_width,
        content_height,
        column_gaps: &column_gaps,
        row_gaps: &row_gaps,
        items,
        container_kind: GapContainerKind::Flex,
    })
}

/// Paints CSS Gap Decoration rules for resolved gutters in a grid container.
///
/// CSS Grid resolves explicit/implicit tracks and gutters before CSS Gaps
/// assigns decoration rules to the resulting gap sequence:
/// <https://www.w3.org/TR/css-grid-1/#track-sizing> and
/// <https://drafts.csswg.org/css-gaps-1/#assigning>.
pub(in crate::layout) fn grid_gap_decoration_primitives(
    style: &ComputedStyle,
    origin_x: f32,
    content_top: f32,
    content_width: f32,
    content_height: f32,
    items: &[GapDecorationItem],
    gutters: &GapDecorationGridGutters,
) -> Vec<PaintPrimitive> {
    let column_gaps = gutters
        .columns
        .iter()
        .copied()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let row_gaps = gutters
        .rows
        .iter()
        .copied()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    gap_decoration_primitives_for_gaps(GapDecorationContext {
        style,
        origin_x,
        content_top,
        content_width,
        content_height,
        column_gaps: &column_gaps,
        row_gaps: &row_gaps,
        items,
        container_kind: GapContainerKind::Grid,
    })
}

/// Builds grid gutter bands from Taffy's detailed track sizes.
///
/// Taffy models gutters as zero or positive tracks between grid tracks. CSS gap
/// decorations paint those gutter tracks, while any distributed free space from
/// `align-content`/`justify-content` remains outside the decorated gutter.
pub(in crate::layout) fn grid_gap_decoration_gutters_from_tracks(
    column_sizes: &[f32],
    column_gutters: &[f32],
    row_sizes: &[f32],
    row_gutters: &[f32],
    style: &ComputedStyle,
    content_width: f32,
    content_height: f32,
) -> GapDecorationGridGutters {
    GapDecorationGutters {
        columns: grid_axis_gutters_from_tracks(
            column_sizes,
            column_gutters,
            content_width,
            style.justify_content,
            style.direction == Direction::Rtl,
        ),
        rows: grid_axis_gutters_from_tracks(
            row_sizes,
            row_gutters,
            content_height,
            style.align_content,
            false,
        ),
    }
}

pub(in crate::layout) struct GapDecorationContext<'a> {
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) origin_x: f32,
    pub(in crate::layout) content_top: f32,
    pub(in crate::layout) content_width: f32,
    pub(in crate::layout) content_height: f32,
    pub(in crate::layout) column_gaps: &'a [GapBand],
    pub(in crate::layout) row_gaps: &'a [GapBand],
    pub(in crate::layout) items: &'a [GapDecorationItem],
    pub(in crate::layout) container_kind: GapContainerKind,
}

pub(in crate::layout) fn gap_decoration_primitives_for_gaps(
    context: GapDecorationContext<'_>,
) -> Vec<PaintPrimitive> {
    if context.style.visibility != Visibility::Visible
        || (context.column_gaps.is_empty() && context.row_gaps.is_empty())
    {
        return Vec::new();
    }

    let column_rules = axis_rule_primitives(AxisRuleContext {
        kind: GapRuleAxisKind::Column,
        container_kind: context.container_kind,
        rule: &context.style.column_rule,
        crossing_rule: &context.style.row_rule,
        origin_x: context.origin_x,
        content_top: context.content_top,
        inline_size: context.content_width,
        block_size: context.content_height,
        gaps: context.column_gaps,
        crossing_gaps: context.row_gaps,
        items: context.items,
    });
    let row_rules = axis_rule_primitives(AxisRuleContext {
        kind: GapRuleAxisKind::Row,
        container_kind: context.container_kind,
        rule: &context.style.row_rule,
        crossing_rule: &context.style.column_rule,
        origin_x: context.origin_x,
        content_top: context.content_top,
        inline_size: context.content_width,
        block_size: context.content_height,
        gaps: context.row_gaps,
        crossing_gaps: context.column_gaps,
        items: context.items,
    });

    let mut primitives = Vec::new();
    match context.style.rule_overlap {
        css::GapRuleOverlap::RowOverColumn => {
            primitives.extend(column_rules);
            primitives.extend(row_rules);
        }
        css::GapRuleOverlap::ColumnOverRow => {
            primitives.extend(row_rules);
            primitives.extend(column_rules);
        }
    }
    primitives
}

/// Paints CSS Gap Decoration column rules for multi-column gutters.
///
/// CSS Multi-column layout uses `column-gap` to separate adjacent column boxes,
/// while CSS Gaps applies column-rule decorations to those gaps:
/// <https://www.w3.org/TR/css-multicol-1/#column-gaps-and-rules> and
/// <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>.
pub(in crate::layout) fn multicol_gap_decoration_primitives(
    style: &ComputedStyle,
    content_left: f32,
    content_top: f32,
    content_bottom: f32,
    column_width: f32,
    gap: f32,
    column_count: usize,
) -> Vec<PaintPrimitive> {
    if style.visibility != Visibility::Visible || column_count < 2 || gap <= GAP_RULE_EPSILON {
        return Vec::new();
    }
    let gaps = (0..column_count.saturating_sub(1))
        .map(|index| {
            let start = (column_width + gap) * index as f32 + column_width;
            GapBand {
                start,
                end: start + gap,
                grid_line: None,
            }
        })
        .collect::<Vec<_>>();
    axis_rule_primitives(AxisRuleContext {
        kind: GapRuleAxisKind::Column,
        container_kind: GapContainerKind::Multicol,
        rule: &style.column_rule,
        crossing_rule: &style.row_rule,
        origin_x: content_left,
        content_top,
        inline_size: (column_width * column_count as f32
            + gap * column_count.saturating_sub(1) as f32)
            .max(0.0),
        block_size: (content_top - content_bottom).max(0.0),
        gaps: &gaps,
        crossing_gaps: &[],
        items: &[],
    })
}

#[derive(Clone, Copy)]
pub(in crate::layout) struct AxisRuleContext<'a> {
    pub(in crate::layout) kind: GapRuleAxisKind,
    pub(in crate::layout) container_kind: GapContainerKind,
    pub(in crate::layout) rule: &'a css::GapRuleAxis,
    pub(in crate::layout) crossing_rule: &'a css::GapRuleAxis,
    pub(in crate::layout) origin_x: f32,
    pub(in crate::layout) content_top: f32,
    pub(in crate::layout) inline_size: f32,
    pub(in crate::layout) block_size: f32,
    pub(in crate::layout) gaps: &'a [GapBand],
    pub(in crate::layout) crossing_gaps: &'a [GapBand],
    pub(in crate::layout) items: &'a [GapDecorationItem],
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapBand {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) end: f32,
    pub(in crate::layout) grid_line: Option<u16>,
}

impl GapBand {
    pub(in crate::layout) fn size(self) -> f32 {
        (self.end - self.start).max(0.0)
    }

    pub(in crate::layout) fn center(self) -> f32 {
        (self.start + self.end) * 0.5
    }
}

impl From<GapDecorationGutter> for GapBand {
    fn from(gutter: GapDecorationGutter) -> Self {
        Self {
            start: gutter.start,
            end: gutter.end,
            grid_line: gutter.grid_line,
        }
    }
}

pub(in crate::layout) fn grid_axis_gutters_from_tracks(
    sizes: &[f32],
    gutters: &[f32],
    axis_size: f32,
    alignment: AlignContent,
    axis_is_reversed: bool,
) -> Vec<GapDecorationGutter> {
    if sizes.is_empty() || gutters.len() != sizes.len() + 1 {
        return Vec::new();
    }
    let used_size = sizes.iter().chain(gutters.iter()).sum::<f32>();
    let free_space = axis_size - used_size;
    let track_count = sizes
        .iter()
        .filter(|size| **size > GAP_RULE_EPSILON)
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
    for (index, size) in sizes.iter().copied().enumerate() {
        let gutter_size = gutters[index].max(0.0);
        if index > 0 && gutter_size > GAP_RULE_EPSILON {
            bands.push(GapDecorationGutter::with_grid_line(
                cursor,
                cursor + gutter_size,
                Some((index + 1) as u16),
            ));
        }
        cursor += gutter_size;

        let is_track = size > GAP_RULE_EPSILON;
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

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapDecorationSegment {
    pub(in crate::layout) start: GapRuleEndpoint,
    pub(in crate::layout) end: GapRuleEndpoint,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapRuleEndpoint {
    pub(in crate::layout) position: f32,
    pub(in crate::layout) kind: GapRuleEndpointKind,
    pub(in crate::layout) crossing_gap_width: f32,
    pub(in crate::layout) crossing_rule_width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GapRuleEndpointKind {
    Cap,
    Junction,
}

impl GapRuleEndpoint {
    pub(in crate::layout) fn cap(position: f32) -> Self {
        Self {
            position,
            kind: GapRuleEndpointKind::Cap,
            crossing_gap_width: 0.0,
            crossing_rule_width: 0.0,
        }
    }

    pub(in crate::layout) fn junction(
        position: f32,
        crossing_gap_width: f32,
        crossing_rule_width: f32,
    ) -> Self {
        Self {
            position,
            kind: GapRuleEndpointKind::Junction,
            crossing_gap_width,
            crossing_rule_width,
        }
    }
}

pub(in crate::layout) fn axis_rule_primitives(context: AxisRuleContext<'_>) -> Vec<PaintPrimitive> {
    let gap_count = context.gaps.len();
    let mut primitives = Vec::new();
    for (index, gap) in context.gaps.iter().copied().enumerate() {
        let width = used_gap_rule_length(
            context
                .rule
                .widths
                .value_for_index(index, gap_count)
                .expect("gap rule width should exist for gap index"),
            gap.size(),
        );
        let segments = gap_rule_segments(context, gap, width);
        for segment in segments {
            let segment = offset_gap_rule_segment(context.rule, segment);
            if segment.end.position <= segment.start.position + GAP_RULE_EPSILON
                || !segment_is_visible(context, gap, segment)
            {
                continue;
            }
            primitives.extend(gap_rule_segment_primitives(
                context,
                gap,
                segment,
                width,
                context
                    .rule
                    .styles
                    .value_for_index(index, gap_count)
                    .expect("gap rule style should exist for gap index"),
                context
                    .rule
                    .colors
                    .value_for_index(index, gap_count)
                    .expect("gap rule color should exist for gap index"),
            ));
        }
    }
    primitives
}

pub(in crate::layout) fn gap_rule_segments(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    own_width: f32,
) -> Vec<GapDecorationSegment> {
    let axis_size = context.axis_size();
    if axis_size <= GAP_RULE_EPSILON {
        return Vec::new();
    }
    let break_behavior = effective_rule_break(context);
    if break_behavior == css::GapRuleBreak::None {
        return vec![GapDecorationSegment {
            start: GapRuleEndpoint::cap(0.0),
            end: GapRuleEndpoint::cap(axis_size),
        }];
    }

    let crossing_gap_count = context.crossing_gaps.len();
    let mut segments = Vec::new();
    let mut cursor = 0.0;
    for (cross_index, crossing_gap) in context.crossing_gaps.iter().copied().enumerate() {
        let crossing_rule_width = context
            .crossing_rule
            .widths
            .value_for_index(cross_index, crossing_gap_count)
            .map(|width| used_gap_rule_length(width, crossing_gap.size()))
            .unwrap_or(0.0);
        let crossing_rule_can_paint = crossing_rule_can_paint(
            crossing_rule_width,
            context
                .crossing_rule
                .styles
                .value_for_index(cross_index, crossing_gap_count),
            context
                .crossing_rule
                .colors
                .value_for_index(cross_index, crossing_gap_count),
        );
        if grid_normal_crossing_is_visible(context, gap, crossing_gap, crossing_rule_can_paint)
            .is_some_and(|visible| !visible)
        {
            continue;
        }
        let junction_start = crossing_gap.start.clamp(0.0, axis_size);
        let junction_end = crossing_gap.end.clamp(0.0, axis_size);
        if junction_start > cursor + GAP_RULE_EPSILON {
            segments.push(GapDecorationSegment {
                start: segment_start_endpoint(
                    context,
                    gap,
                    cursor,
                    crossing_gap,
                    crossing_rule_width,
                    crossing_rule_can_paint,
                ),
                end: segment_junction_endpoint(
                    context,
                    gap,
                    junction_start,
                    crossing_gap,
                    crossing_gap.size(),
                    crossing_rule_width,
                    crossing_rule_can_paint,
                ),
            });
        }
        if should_join_across_junction(context, gap, crossing_gap, own_width) {
            cursor = junction_start;
        } else {
            cursor = junction_end.max(cursor);
        }
    }
    if axis_size > cursor + GAP_RULE_EPSILON {
        segments.push(GapDecorationSegment {
            start: if cursor <= GAP_RULE_EPSILON {
                GapRuleEndpoint::cap(0.0)
            } else {
                nearest_crossing_gap(context.crossing_gaps, cursor)
                    .map(|crossing_gap| {
                        segment_junction_endpoint(
                            context,
                            gap,
                            cursor,
                            crossing_gap,
                            crossing_gap.size(),
                            crossing_width_for_gap(context, crossing_gap).unwrap_or(0.0),
                            crossing_can_paint_for_gap(context, crossing_gap).unwrap_or(true),
                        )
                    })
                    .unwrap_or_else(|| GapRuleEndpoint::cap(cursor))
            },
            end: GapRuleEndpoint::cap(axis_size),
        });
    }
    segments
}

pub(in crate::layout) fn effective_rule_break(context: AxisRuleContext<'_>) -> css::GapRuleBreak {
    match (
        context.container_kind,
        context.kind,
        context.rule.rule_break,
    ) {
        (GapContainerKind::Multicol, GapRuleAxisKind::Column, css::GapRuleBreak::Normal) => {
            css::GapRuleBreak::Intersection
        }
        (GapContainerKind::Multicol, GapRuleAxisKind::Row, css::GapRuleBreak::Normal) => {
            css::GapRuleBreak::None
        }
        (GapContainerKind::Flex, _, css::GapRuleBreak::Normal) => css::GapRuleBreak::None,
        (_, _, rule_break) => rule_break,
    }
}

pub(in crate::layout) fn should_join_across_junction(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    crossing_gap: GapBand,
    own_width: f32,
) -> bool {
    match effective_rule_break(context) {
        css::GapRuleBreak::Intersection => {
            grid_segment_is_flanked_by_spanning_items(context, gap, crossing_gap).unwrap_or_else(
                || segment_crosses_spanning_item(context, gap, crossing_gap, own_width),
            )
        }
        css::GapRuleBreak::Normal if context.container_kind == GapContainerKind::Grid => {
            grid_junction_is_cross_intersection(context, gap, crossing_gap).unwrap_or(false)
        }
        _ => false,
    }
}

pub(in crate::layout) fn grid_normal_crossing_is_visible(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    crossing_gap: GapBand,
    crossing_rule_can_paint: bool,
) -> Option<bool> {
    if context.container_kind != GapContainerKind::Grid
        || effective_rule_break(context) != css::GapRuleBreak::Normal
    {
        return None;
    }
    if !crossing_rule_can_paint {
        return Some(false);
    }
    grid_crossing_segment_present_at_junction(context, gap, crossing_gap)
}

pub(in crate::layout) fn segment_crosses_spanning_item(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    crossing_gap: GapBand,
    own_width: f32,
) -> bool {
    if context.items.is_empty() {
        return false;
    }
    let half = own_width.max(gap.size()) * 0.5;
    context.items.iter().any(|item| match context.kind {
        GapRuleAxisKind::Column => grid_item_spans_intersection(
            context.container_kind,
            *item,
            gap.grid_line,
            crossing_gap.grid_line,
            GapRuleAxisKind::Column,
        )
        .unwrap_or_else(|| {
            item.x < gap.center() + half
                && item.x_end() > gap.center() - half
                && item.y <= crossing_gap.start + GAP_RULE_EPSILON
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
            item.y < gap.center() + half
                && item.y_end() > gap.center() - half
                && item.x <= crossing_gap.start + GAP_RULE_EPSILON
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

pub(in crate::layout) fn grid_junction_is_cross_intersection(
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
    let mut before_column_before_row = false;
    let mut after_column_before_row = false;
    let mut before_column_after_row = false;
    let mut after_column_after_row = false;
    for area in context.items.iter().filter_map(|item| item.grid_area) {
        before_column_before_row |=
            grid_area_occupies_quadrant(area, column_line, row_line, false, false);
        after_column_before_row |=
            grid_area_occupies_quadrant(area, column_line, row_line, true, false);
        before_column_after_row |=
            grid_area_occupies_quadrant(area, column_line, row_line, false, true);
        after_column_after_row |=
            grid_area_occupies_quadrant(area, column_line, row_line, true, true);
    }
    Some(
        before_column_before_row
            && after_column_before_row
            && before_column_after_row
            && after_column_after_row,
    )
}
