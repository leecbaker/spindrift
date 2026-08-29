use std::num::NonZeroUsize;

use super::*;

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
    let topology = multicol_gap_topology_for_row(MulticolGapTopologyRowInput {
        style,
        content_left,
        content_top,
        column_height: (content_top - content_bottom).max(0.0),
        inline_size: (column_width * column_count as f32
            + gap * column_count.saturating_sub(1) as f32)
            .max(0.0),
        column_width,
        column_gap: gap,
        column_count,
        row: MulticolumnRowIndex::new(0),
        previous_row_gap: None,
        boundary: MulticolRowBoundary::Final,
    });
    gap_decoration_primitives_for_topology(style, &topology)
}

/// One committed multicolumn row and its adjacent row-gap topology.
///
/// The previous row gap is crossing-only: it classifies the current column
/// portions' start endpoints without repainting an already committed rule.
/// The following row gap is paintable and is emitted together with the row's
/// column portions, so `rule-overlap` is independent of replay timing.
#[derive(Clone, Copy)]
pub(in crate::layout) struct MulticolGapTopologyRowInput<'a> {
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_top: f32,
    pub(in crate::layout) column_height: f32,
    pub(in crate::layout) inline_size: f32,
    pub(in crate::layout) column_width: f32,
    pub(in crate::layout) column_gap: f32,
    pub(in crate::layout) column_count: usize,
    pub(in crate::layout) row: MulticolumnRowIndex,
    pub(in crate::layout) previous_row_gap: Option<f32>,
    pub(in crate::layout) boundary: MulticolRowBoundary,
}

/// The committed boundary after one wrapped multicolumn row.
///
/// A following row is represented by a prevalidated slot, so constructing a
/// paintable row gap without a corresponding entry in its CSS rule sequence
/// is impossible.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum MulticolRowBoundary {
    Final,
    FollowedBy { gap: f32, slot: GapRuleSlot },
}

/// Build the physical topology for one committed multicolumn row.
///
/// Column portions end at a row gap rather than overlapping it.  The shared
/// resolver still receives the abutting gap as crossing geometry, which gives
/// the endpoint its required junction classification.
/// <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
pub(in crate::layout) fn multicol_gap_topology_for_row(
    input: MulticolGapTopologyRowInput<'_>,
) -> ResolvedGapTopology {
    let MulticolGapTopologyRowInput {
        style,
        content_left,
        content_top,
        column_height,
        inline_size,
        column_width,
        column_gap,
        column_count,
        row,
        previous_row_gap,
        boundary,
    } = input;
    let column_height = column_height.max(0.0);
    let following_row_gap = match boundary {
        MulticolRowBoundary::Final => None,
        MulticolRowBoundary::FollowedBy { gap, .. } => Some(gap.max(0.0)),
    };
    let mut topology = ResolvedGapTopology::multicol(GapDecorationContainer::new(
        content_left,
        content_top,
        inline_size.max(0.0),
        column_height + following_row_gap.unwrap_or(0.0),
    ));
    let column_segment = GapAxisSpan::new(0.0, column_height);
    let column_rule_sequence = NonZeroUsize::new(column_count.saturating_sub(1));
    for physical_index in 0..column_count.saturating_sub(1) {
        let start = (column_width + column_gap) * physical_index as f32 + column_width;
        let rule_index = if physical_column_rule_sequence_is_reversed(style) {
            column_count
                .saturating_sub(2)
                .saturating_sub(physical_index)
        } else {
            physical_index
        };
        let portion = MulticolGapPortion::new(
            row,
            GapBand {
                start,
                end: start + column_gap,
                grid_line: None,
                segment_range: Some(column_segment),
                rule_index: None,
            },
            GapRuleSlot::new(
                GapRuleIndex::new(rule_index),
                column_rule_sequence.expect("a multicolumn gap has a rule sequence"),
            )
            .expect("physical multicol gap index is inside its rule sequence"),
        );
        topology.column_gaps.push(portion.band);
        topology.row_crossings.push(portion.band.band);
    }

    if let Some(previous_gap) = previous_row_gap.filter(|gap| *gap >= 0.0) {
        topology.column_crossings.push(GapBand {
            start: -previous_gap,
            end: 0.0,
            grid_line: None,
            segment_range: Some(GapAxisSpan::new(0.0, inline_size)),
            rule_index: None,
        });
    }
    if let MulticolRowBoundary::FollowedBy { slot, .. } = boundary {
        let next_gap = following_row_gap.expect("following row boundary carries its gap");
        let row_gap = GapBand {
            start: column_height,
            end: column_height + next_gap,
            grid_line: None,
            segment_range: Some(GapAxisSpan::new(0.0, inline_size)),
            rule_index: None,
        };
        topology.column_crossings.push(row_gap);
        topology.row_gaps.push(AssignedGapBand::new(row_gap, slot));
    }
    topology
}

/// Resolves how many anonymous columns contribute gutters for
/// `column-rule-visibility-items`.
/// <https://drafts.csswg.org/css-gaps-1/#rule-visibility>
pub(in crate::layout) fn multicol_decorated_column_count(
    style: &ComputedStyle,
    occupied: usize,
    available: usize,
) -> usize {
    match style.column_rule.visibility_items {
        // With `column-count:auto`, `all` bypasses item-adjacency suppression
        // but does not instantiate otherwise nonexistent anonymous columns;
        // the available inline size only caps how many columns may be made.
        // An explicit count establishes that complete track sequence, so all
        // of its gaps remain eligible.
        // <https://drafts.csswg.org/css-gaps-1/#rule-visibility>
        css::GapRuleVisibilityItems::All
            if matches!(style.column_count, css::ColumnCount::Auto) =>
        {
            occupied.min(available)
        }
        css::GapRuleVisibilityItems::All => available,
        css::GapRuleVisibilityItems::Around if occupied > 0 => (occupied + 1).min(available),
        css::GapRuleVisibilityItems::Normal
        | css::GapRuleVisibilityItems::Between
        | css::GapRuleVisibilityItems::Around => occupied.min(available),
    }
}

/// Paints the row rule between two adjacent wrapped multicol rows.
///
/// Multicol Level 2 creates a row gap after each complete row when overflow
/// columns wrap in the block direction. Column gaps crossing that row gap are
/// supplied so `rule-break` and junction inset behavior use the same segment
/// model as flex and grid decorations.
/// <https://drafts.csswg.org/css-multicol-2/#row-gaps>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn multicol_row_gap_decoration_primitives(
    style: &ComputedStyle,
    content_left: f32,
    content_top: f32,
    inline_size: f32,
    column_height: f32,
    row_gap: f32,
    row_index: usize,
    row_gap_count: usize,
    column_width: f32,
    column_gap: f32,
    column_count: usize,
) -> Vec<PaintPrimitive> {
    if style.visibility != Visibility::Visible || row_gap < 0.0 {
        return Vec::new();
    }
    let Some(rule_sequence) = NonZeroUsize::new(row_gap_count) else {
        return Vec::new();
    };
    let row_start = column_height + row_index as f32 * (column_height + row_gap);
    let row_rule_index = if style.writing_mode.ltr_inline_progresses_upward() {
        row_gap_count.saturating_sub(1).saturating_sub(row_index)
    } else {
        row_index
    };
    let row_gap_band = GapBand {
        start: row_start,
        end: row_start + row_gap,
        grid_line: None,
        segment_range: Some(GapAxisSpan::new(0.0, inline_size)),
        rule_index: None,
    };
    let row_crossings = (0..column_count.saturating_sub(1))
        .map(|index| {
            let start = (column_width + column_gap) * index as f32 + column_width;
            GapBand {
                start,
                end: start + column_gap,
                grid_line: None,
                segment_range: None,
                rule_index: None,
            }
        })
        .collect::<Vec<_>>();
    let mut topology = ResolvedGapTopology::multicol(GapDecorationContainer::new(
        content_left,
        content_top,
        inline_size,
        row_start + row_gap,
    ));
    let Some(row_slot) = GapRuleSlot::new(GapRuleIndex::new(row_rule_index), rule_sequence) else {
        return Vec::new();
    };
    topology
        .row_gaps
        .push(AssignedGapBand::new(row_gap_band, row_slot));
    topology.row_crossings = row_crossings;
    gap_decoration_primitives_for_topology(style, &topology)
}
