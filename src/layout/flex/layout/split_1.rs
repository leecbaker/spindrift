use super::*;

/// Input geometry for an abspos flex child's static-position calculation.
///
/// CSS Flexbox derives the static position of an absolutely positioned flex
/// child from the flex container's content box and hypothetical sole-item flex
/// placement:
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
pub(in crate::layout::flex) struct PositionedFlexStaticContext<'a> {
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) stylesheets: &'a [Stylesheet],
    pub(in crate::layout::flex) available: FlexAvailableSpace,
    pub(in crate::layout::flex) inner_x: f32,
    pub(in crate::layout::flex) inner_width: f32,
    pub(in crate::layout::flex) content_top: f32,
}

/// One flex fragmentation boundary in the physical block direction.
///
/// CSS Flexbox fragments row containers by flex line and column containers by
/// item progression in paged media:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
#[derive(Debug, Clone)]
pub(in crate::layout::flex) struct FlexBreakUnit {
    pub(in crate::layout::flex) item_indices: Vec<usize>,
    pub(in crate::layout::flex) line_start: usize,
    pub(in crate::layout::flex) line_end: usize,
    pub(in crate::layout::flex) block_start: f32,
    pub(in crate::layout::flex) block_end: f32,
    pub(in crate::layout::flex) break_before: PageBreak,
    pub(in crate::layout::flex) break_after: PageBreak,
    pub(in crate::layout::flex) break_inside_avoid: bool,
}

impl FlexBreakUnit {
    pub(in crate::layout::flex) fn block_size(&self) -> f32 {
        (self.block_end - self.block_start).max(0.0)
    }

    pub(in crate::layout::flex) fn slice(&self, block_start: f32, block_end: f32) -> Self {
        Self {
            item_indices: self.item_indices.clone(),
            line_start: self.line_start,
            line_end: self.line_end,
            block_start,
            block_end,
            break_before: self.break_before,
            break_after: self.break_after,
            break_inside_avoid: self.break_inside_avoid,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexFragmentBuildContext {
    pub(in crate::layout::flex) page_index: usize,
    pub(in crate::layout::flex) outer_x: f32,
    pub(in crate::layout::flex) outer_width: f32,
    pub(in crate::layout::flex) content_top: f32,
    pub(in crate::layout::flex) block_offset: f32,
    /// Remaining capacity in the fragmentainer that materialized this slice.
    pub(in crate::layout::flex) first_fragmentainer_capacity: f32,
    /// Capacity of an empty continuation fragmentainer of the active kind.
    pub(in crate::layout::flex) continuation_fragmentainer_capacity: f32,
    pub(in crate::layout::flex) starts_page_fragment: bool,
}

/// Current fragment-local cursor for flex container fragmentation.
///
/// CSS Flexbox fragments row containers by flex line and column containers by
/// item progression. This cursor records the fragment-local flex content top and
/// source block offset selected by the committed fragment transition before
/// item fragments are built:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::flex) struct FlexFragmentCursor {
    pub(in crate::layout::flex) content_top: f32,
    pub(in crate::layout::flex) block_offset: f32,
}

/// Why a flex container fragment starts at this source offset.
///
/// CSS Fragmentation commits a new fragmentainer before laying out the next
/// piece. Flex pagination uses this reason only internally so overflow,
/// avoid-boundary moves, forced item breaks, and oversized-item slicing all
/// route through the same committed fragment-transition shape:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexFragmentBreakReason {
    Forced,
    OverflowOrAvoid,
    OversizedSliceProgress,
    SliceContinuation,
}

/// Committed transition for one flex container fragment boundary.
///
/// The decision owns the active fragmentainer kind and the next source block
/// offset before replay materializes a target-specific fragmentainer advance.
/// Painting then consumes the resulting `FlexFragmentCursor` when constructing
/// fragment-local item fragments:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexFragmentTransitionDecision {
    pub(in crate::layout::flex) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::flex) reason: FlexFragmentBreakReason,
    pub(in crate::layout::flex) next_block_offset: f32,
}

/// Committed pre-slice decision for one flex break unit.
///
/// CSS Flexbox pagination allows a break before a flex line or item-progression
/// unit when it overflows the current fragmentainer or when an avoid break
/// constraint applies. Forced breaks are applied first so this decision uses
/// the post-forced-break cursor when checking overflow:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination> and
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexUnitPrebreakDecision {
    pub(in crate::layout::flex) transition_before_unit: Option<FlexFragmentTransitionDecision>,
}

pub(in crate::layout::flex) struct FlexUnitPrebreakDecisionInput {
    pub(in crate::layout::flex) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::flex) break_is_applicable: bool,
    pub(in crate::layout::flex) unit_is_oversized: bool,
    pub(in crate::layout::flex) has_prior_unit: bool,
    pub(in crate::layout::flex) has_later_unit: bool,
    pub(in crate::layout::flex) cursor: FlexFragmentCursor,
    pub(in crate::layout::flex) unit_block_start: f32,
    pub(in crate::layout::flex) unit_block_end: f32,
    pub(in crate::layout::flex) current_fragmentainer: Fragmentainer,
    pub(in crate::layout::flex) break_opportunity: FragmentBreakOpportunity,
    pub(in crate::layout::flex) can_advance: bool,
}

/// Fragment-local decision for the next flex break-unit slice.
///
/// CSS Flexbox fragments flex containers by flex line or item progression, and
/// CSS Fragmentation may split an oversized flex unit across fragmentainers.
/// This decision fixes the fragment-local source slice before item fragments are
/// built, or commits a progress transition when no block-size is currently
/// available:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexUnitSliceDecision {
    pub(in crate::layout::flex) slice_start: f32,
    pub(in crate::layout::flex) slice_end: f32,
    pub(in crate::layout::flex) transition_before_paint: Option<FlexFragmentTransitionDecision>,
}

pub(in crate::layout::flex) struct FlexUnitSliceDecisionInput {
    pub(in crate::layout::flex) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::flex) break_is_applicable: bool,
    pub(in crate::layout::flex) can_slice_at_fragmentainer_boundary: bool,
    pub(in crate::layout::flex) unit_block_end: f32,
    pub(in crate::layout::flex) slice_start: f32,
    pub(in crate::layout::flex) available_block_end: f32,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct SplitFlexItemPaintContext {
    /// The used physical border-box dimensions from the flex algorithm.
    /// These must not be confused with the content-box percentage bases used
    /// when replaying descendants.
    pub(in crate::layout::flex) item_width: BorderBoxLength,
    pub(in crate::layout::flex) item_height: BorderBoxLength,
    pub(in crate::layout::flex) percentage_height_basis: FlexPercentageBasis,
    pub(in crate::layout::flex) slice_border_box: PaintClip,
    pub(in crate::layout::flex) source_item_top: f32,
    /// Committed source range and item-local ordinal for this replay. This is
    /// produced by the materialized flex fragment plan, rather than guessed
    /// from a page-sized source offset during painting.
    pub(in crate::layout::flex) continuation: FlexItemContinuation,
    /// Whether this fragment preserves the item’s source coordinate system.
    /// Row flex lines and wrapped column lines use a contiguous source slice,
    /// while a single-line column continuation retains its main-axis replay
    /// path until it has explicit fragment-local relayout state.
    pub(in crate::layout::flex) replay_source_slice_offset: bool,
    /// The flex container's containing block expressed in the target
    /// fragmentainer. Split-item replay translates its source painting to an
    /// off-page coordinate system, so this is remapped before descendants are
    /// laid out. CSS Positioned Layout keeps an absolute descendant attached
    /// to the same containing block even when its in-flow ancestor fragments:
    /// <https://www.w3.org/TR/css-position-3/#def-cb>.
    pub(in crate::layout::flex) positioning_containing_block: Option<ContainingBlock>,
    pub(in crate::layout::flex) establishes_fixed_containing_block: bool,
    /// Fragment-local clip for descendants whose containing block is the flex
    /// container instead of the split flex item.
    pub(in crate::layout::flex) positioned_descendant_clip: Option<PaintClip>,
}

pub(in crate::layout::flex) fn placed_flex_item_style(
    child_style: &ComputedStyle,
    item_width: f32,
    item_height: f32,
    container_flex_direction: FlexDirection,
) -> ComputedStyle {
    let mut placed_style =
        replayed_item_fragmentation_base_style(child_style, ReplayedItemFragmentationPolicy::Flex);
    let borders = used_border_widths(child_style);
    let horizontal_non_content =
        child_style.padding.left + child_style.padding.right + borders.left + borders.right;
    let vertical_non_content =
        child_style.padding.top + child_style.padding.bottom + borders.top + borders.bottom;
    let table_content_box =
        child_style.display.is_table() && matches!(child_style.box_sizing, BoxSizing::ContentBox);
    // The flex adapter records Taffy's used item size in content-box space.
    // Replay freezes its style as a border-box size, so it converts the
    // content-box main size exactly once at this boundary.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-item-sizing>
    // A flex main-size is the table grid's supplied content-size when the
    // table uses content-box sizing. The cross-size, however, is a final
    // outer flex-item size and must be converted back to the table grid's
    // content box before table layout replays it.
    // <https://drafts.csswg.org/css-flexbox-1/#definite-sizes> and
    // <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>.
    let main_size_is_content_box =
        !table_content_box && matches!(child_style.box_sizing, BoxSizing::ContentBox);
    let used_width = if table_content_box && container_flex_direction.is_column_axis() {
        (item_width - horizontal_non_content).max(0.0)
    } else if main_size_is_content_box && container_flex_direction.is_row_axis() {
        item_width + horizontal_non_content
    } else {
        item_width
    };
    let used_height = if table_content_box && container_flex_direction.is_row_axis() {
        (item_height - vertical_non_content).max(0.0)
    } else if main_size_is_content_box && container_flex_direction.is_column_axis() {
        item_height + vertical_non_content
    } else {
        item_height
    };
    set_style_used_width(&mut placed_style, used_width);
    set_style_used_height(&mut placed_style, used_height);
    // Preserve the resolved main-axis bound while replaying the item. The
    // temporary formatting context must not reapply an authored min/max
    // percentage against a different containing block after Flexbox has
    // already resolved the item's used main size.
    // <https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths>
    if container_flex_direction.is_row_axis() {
        set_style_used_width_bounds(&mut placed_style, used_width);
    } else {
        set_style_used_height_bounds(&mut placed_style, used_height);
    }
    placed_style.box_sizing = if table_content_box {
        BoxSizing::ContentBox
    } else {
        BoxSizing::BorderBox
    };
    placed_style
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::flex) fn layout_flex_item_contents(
        &mut self,
        child: &StyledChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        percentage_height_basis: FlexPercentageBasis,
    ) {
        self.with_replayed_flex_item_percentage_height_basis(percentage_height_basis, |layout| {
            // The flex container owns fragmentation at flex-line/item
            // boundaries. Replaying a final, unsplit item through ordinary
            // block flow must therefore keep its descendants in the assigned
            // item fragment rather than letting a descendant manufacture an
            // independent page break before the item's used height is applied.
            // <https://drafts.csswg.org/css-flexbox-1/#pagination>
            layout.fragmentation_suppression_depth += 1;
            layout.layout_formatting_context_item_contents(child, placed_style, stylesheets);
            layout.fragmentation_suppression_depth -= 1;
        });
    }
}

impl FlexFragmentCursor {
    pub(in crate::layout::flex) fn new(content_top: f32, block_offset: f32) -> Self {
        Self {
            content_top,
            block_offset,
        }
    }
}

impl FlexFragmentTransitionDecision {
    pub(in crate::layout::flex) fn forced(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: f32,
    ) -> Self {
        Self {
            fragmentainer_kind,
            reason: FlexFragmentBreakReason::Forced,
            next_block_offset,
        }
    }

    pub(in crate::layout::flex) fn oversized_slice_progress(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: f32,
    ) -> Self {
        Self {
            fragmentainer_kind,
            reason: FlexFragmentBreakReason::OversizedSliceProgress,
            next_block_offset,
        }
    }

    pub(in crate::layout::flex) fn slice_continuation(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: f32,
    ) -> Self {
        Self {
            fragmentainer_kind,
            reason: FlexFragmentBreakReason::SliceContinuation,
            next_block_offset,
        }
    }

    #[cfg(test)]
    pub(in crate::layout::flex) fn materializes_page_cursor(self) -> bool {
        self.fragmentainer_kind.materializes_page_cursor()
    }

    pub(in crate::layout::flex) fn cursor_after_fragmentainer_advance(
        self,
        content_top: f32,
    ) -> FlexFragmentCursor {
        FlexFragmentCursor::new(content_top, self.next_block_offset)
    }
}

impl FlexUnitPrebreakDecision {
    pub(in crate::layout::flex) fn choose(input: FlexUnitPrebreakDecisionInput) -> Self {
        let required_block_size = (input.unit_block_end - input.cursor.block_offset).max(0.0);
        let unit_overflows = input
            .current_fragmentainer
            .required_block_size_overflows(required_block_size);
        // An oversized unit within a sequence cannot be kept together in a
        // fresh fragmentainer. Moving it before its first slice would leave
        // usable remainder space empty, so CSS Fragmentation must slice it at
        // the current fragmentainer instead. A sole oversized unit retains the
        // normal whole-box prebreak path, which handles a container that itself
        // began late in a fragmentainer.
        // <https://www.w3.org/TR/css-break-3/#breaking-rules>
        let can_keep_unit_together =
            !input.unit_is_oversized || (!input.has_prior_unit && !input.has_later_unit);
        // `avoid` participates in the boundary's break decision even before
        // overflow: a matching avoid value joins the flex unit to the next
        // fragmentainer rather than admitting a break at that boundary.
        // <https://www.w3.org/TR/css-break-3/#avoid-breaks>
        let avoid_break = input
            .break_opportunity
            .avoids_break_in(input.fragmentainer_kind)
            && can_keep_unit_together;
        let transition_before_unit = FragmentAdvanceDecision::choose(FragmentAdvanceInput {
            break_is_applicable: input.break_is_applicable,
            // Flex fragmentation has a break opportunity at every flex-line
            // (row axis) or item (column axis) boundary. A unit in a sequence
            // that fits in an empty fragmentainer moves wholesale when it no
            // longer fits in the remaining space; only a lone box or a unit
            // larger than a full page is sliced through that boundary.
            overflows: unit_overflows
                && can_keep_unit_together
                && (avoid_break || input.has_prior_unit || input.has_later_unit),
            avoid_break,
            can_advance: input.can_advance,
        })
        .should_advance
        .then_some(FlexFragmentTransitionDecision {
            fragmentainer_kind: input.fragmentainer_kind,
            reason: FlexFragmentBreakReason::OverflowOrAvoid,
            next_block_offset: input.unit_block_start,
        });
        Self {
            transition_before_unit,
        }
    }
}

impl FlexUnitSliceDecision {
    pub(in crate::layout::flex) fn choose(input: FlexUnitSliceDecisionInput) -> Self {
        let source_slice = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
            break_is_applicable: input.break_is_applicable,
            // A flex break unit is a source range that can be sliced through
            // its items. It need not exceed a full empty fragmentainer before
            // the current fragmentainer boundary becomes a valid slice point.
            source_is_oversized: input.can_slice_at_fragmentainer_boundary,
            source_block_end: input.unit_block_end,
            slice_start: input.slice_start,
            available_block_end: input.available_block_end,
        });
        let transition_before_paint = source_slice.advance_before_slice.then_some(
            FlexFragmentTransitionDecision::oversized_slice_progress(
                input.fragmentainer_kind,
                source_slice.slice_start,
            ),
        );
        Self {
            slice_start: source_slice.slice_start,
            slice_end: source_slice.slice_end,
            transition_before_paint,
        }
    }

    pub(in crate::layout::flex) fn paints_slice(self) -> bool {
        self.transition_before_paint.is_none()
    }
}

/// Resolve a flex container width keyword from known intrinsic contributions.
///
/// CSS Sizing defines `fit-content` as
/// `min(max-content, max(min-content, stretch-or-argument))`. Auto widths keep
/// normal block fill behavior, except float and inline-flex atom callers pass
/// `shrink_auto_width` to request CSS 2.2 shrink-to-fit sizing. Browser WPTs
/// for multi-line column flexboxes shrink unconstrained auto-width floats to
/// their min-content cross size while still letting a smaller containing block
/// clamp between min-content and wrapped max-content:
/// <https://www.w3.org/TR/css-sizing-3/#fit-content-size>,
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>, and
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes>.
pub(in crate::layout::flex) fn flex_container_content_width_from_intrinsic(
    style: &ComputedStyle,
    available_outer_width: f32,
    horizontal_extras: f32,
    intrinsic: FlexItemEstimate,
    shrink_auto_width: bool,
) -> ContentBoxLength {
    let min_content = intrinsic.min_width.points().max(0.0);
    let max_content = flex_container_shrink_to_fit_max_content_width(
        style,
        available_outer_width,
        horizontal_extras,
        min_content,
        intrinsic.width.points().max(min_content).max(0.0),
        shrink_auto_width,
    );
    let auto_width = if shrink_auto_width {
        intrinsic::IntrinsicAutoWidth::ShrinkToFit
    } else {
        intrinsic::IntrinsicAutoWidth::FillAvailable
    };
    intrinsic::content_box_width_from_intrinsic(
        style,
        layout_pt(available_outer_width),
        non_content_pt(horizontal_extras),
        content_box_pt(min_content),
        content_box_pt(max_content),
        auto_width,
    )
}

/// Return the max-content width used by auto-width flex shrink-to-fit sizing.
///
/// CSS Flexbox defines multi-line column cross-size contributions separately
/// from normal block intrinsic widths. Floated and atomic flex containers then
/// feed those contributions into CSS 2.2 shrink-to-fit width resolution. An
/// explicit balanced line count is different: it fixes the requested set of
/// flex lines, so the cross size must include every balanced line instead of
/// collapsing to the single-line min-content contribution:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes> and
/// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property> and
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
fn flex_container_shrink_to_fit_max_content_width(
    style: &ComputedStyle,
    available_outer_width: f32,
    horizontal_extras: f32,
    min_content: f32,
    max_content: f32,
    shrink_auto_width: bool,
) -> f32 {
    if !shrink_auto_width
        || !style.box_values.width.is_auto()
        || style.flex_wrap == FlexWrap::NoWrap
        || !style.flex_direction.is_column_axis()
        || (style.flex_wrap.balances_lines() && style.flex_line_count.is_some())
    {
        return max_content;
    }

    let available_content_width = (available_outer_width - horizontal_extras).max(0.0);
    if available_content_width > max_content + 0.01 {
        min_content
    } else {
        max_content
    }
}

/// Returns whether a block flex container's auto physical width needs intrinsic sizing.
///
/// CSS Writing Modes sizes orthogonal flow roots with the fit-content rule
/// rather than stretching the block axis to the containing block's physical
/// width. For a vertical-writing flex container in horizontal flow, that means
/// `width:auto` must shrink-wrap the flex cross size while `height` remains
/// the container's logical inline/main size:
/// <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
pub(in crate::layout::flex) fn orthogonal_auto_width_flex_container_needs_intrinsic(
    style: &ComputedStyle,
    containing_space: ChildAvailableSpace,
) -> bool {
    style.box_values.width.is_auto()
        && matches!(
            (containing_space.writing_mode, style.writing_mode),
            (
                WritingMode::HorizontalTb,
                WritingMode::VerticalRl
                    | WritingMode::VerticalLr
                    | WritingMode::SidewaysRl
                    | WritingMode::SidewaysLr
            ) | (
                WritingMode::VerticalRl
                    | WritingMode::VerticalLr
                    | WritingMode::SidewaysRl
                    | WritingMode::SidewaysLr,
                WritingMode::HorizontalTb
            )
        )
}
