use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexReplayTopology {
    Unfragmented,
    Fragmented,
}

/// One flex fragmentation boundary in the physical block direction.
///
/// CSS Flexbox fragments row containers by flex line and column containers by
/// item progression in paged media:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
#[derive(Debug, Clone)]
pub(in crate::layout::flex) struct FlexBreakUnit {
    pub(in crate::layout::flex) topology: FlexReplayTopology,
    pub(in crate::layout::flex) item_indices: Vec<usize>,
    pub(in crate::layout::flex) line_start: usize,
    pub(in crate::layout::flex) line_end: usize,
    pub(in crate::layout::flex) block_start: FlexFragmentBlockOffset,
    pub(in crate::layout::flex) block_end: FlexFragmentBlockOffset,
    pub(in crate::layout::flex) break_before: PageBreak,
    pub(in crate::layout::flex) break_after: PageBreak,
    pub(in crate::layout::flex) break_inside_avoid: bool,
}

impl FlexBreakUnit {
    pub(in crate::layout::flex) fn block_size(&self) -> FlexFragmentBlockSize {
        FlexFragmentBlockBounds::new(self.block_start, self.block_end).size()
    }

    pub(in crate::layout::flex) fn slice(
        &self,
        block_start: FlexFragmentBlockOffset,
        block_end: FlexFragmentBlockOffset,
    ) -> Self {
        Self {
            topology: self.topology,
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

/// Selects the logical Flexbox boundary that projects onto the physical block
/// direction of a fragmentainer.
///
/// A logical row in vertical writing progresses along physical Y, so its
/// fragmentainer boundaries come from item main-axis intervals, not its flex
/// lines' physical-X cross intervals.  Horizontal rows are the converse: a
/// line cross interval already is a physical block interval.  Keeping this
/// projection explicit prevents a specified `flex-direction` from being
/// mistaken for a physical fragmentainer axis.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexFragmentationBoundaryProjection {
    LineCrossAxis,
    ItemMainAxis,
}

impl FlexFragmentationBoundaryProjection {
    pub(in crate::layout::flex) fn for_style(style: &ComputedStyle) -> Self {
        if physical_flex_direction(style).is_row_axis() {
            Self::LineCrossAxis
        } else {
            Self::ItemMainAxis
        }
    }

    /// Project one flex line's already-physical cross-axis interval onto the
    /// fragmentainer block axis.
    ///
    /// The flex/Taffy adapter resolves writing mode before constructing line
    /// metadata. Consequently a physical row has a physical-Y cross axis,
    /// including a logical `column` in vertical writing. Keeping this
    /// conversion here prevents pagination consumers from treating the
    /// specified Flexbox axis as a page-flow axis.
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    pub(in crate::layout::flex) fn line_cross_block_bounds(
        self,
        line: &FlexLineLayout,
    ) -> FlexFragmentBlockBounds {
        debug_assert_eq!(self, Self::LineCrossAxis);
        FlexFragmentBlockBounds::new(
            FlexFragmentBlockOffset::new(line.cross_start.points()),
            FlexFragmentBlockOffset::new(line.cross_end.points()),
        )
    }

    /// Project one flex item's physical main-axis interval onto the physical
    /// fragmentainer block axis.
    ///
    /// This is selected for physical columns, which includes logical rows in
    /// vertical writing. Fragmentable descendant overflow extends only the
    /// projected source end; it never changes the item's used border box.
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>
    pub(in crate::layout::flex) fn item_main_block_bounds(
        self,
        item: &FlexItemLayout,
        use_fragmentation_height: bool,
    ) -> FlexFragmentBlockBounds {
        debug_assert_eq!(self, Self::ItemMainAxis);
        flex_item_block_bounds(item, use_fragmentation_height)
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexFragmentBuildContext {
    pub(in crate::layout::flex) page_index: usize,
    /// Physical page-inline span of this flex container fragment.
    pub(in crate::layout::flex) outer_inline_span: PageInlineSpan,
    pub(in crate::layout::flex) content_top: PageTopBlockPosition,
    pub(in crate::layout::flex) block_offset: FlexFragmentBlockOffset,
    /// Remaining capacity in the fragmentainer that materialized this slice.
    pub(in crate::layout::flex) first_fragmentainer_capacity: LayoutLength,
    /// Capacity of an empty continuation fragmentainer of the active kind.
    pub(in crate::layout::flex) continuation_fragmentainer_capacity: LayoutLength,
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
    pub(in crate::layout::flex) content_top: PageTopBlockPosition,
    pub(in crate::layout::flex) block_offset: FlexFragmentBlockOffset,
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
    pub(in crate::layout::flex) next_block_offset: FlexFragmentBlockOffset,
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
    pub(in crate::layout::flex) unit_block_start: FlexFragmentBlockOffset,
    pub(in crate::layout::flex) unit_block_end: FlexFragmentBlockOffset,
    /// Usable flex-content capacity remaining in the current fragmentainer.
    /// This is distinct from raw fragmentainer geometry when the container
    /// reserves cloned border and padding at its broken edges.
    pub(in crate::layout::flex) available_content_block_size: LayoutLength,
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
    pub(in crate::layout::flex) slice_start: FlexFragmentBlockOffset,
    pub(in crate::layout::flex) slice_end: FlexFragmentBlockOffset,
    pub(in crate::layout::flex) transition_before_paint: Option<FlexFragmentTransitionDecision>,
}

pub(in crate::layout::flex) struct FlexUnitSliceDecisionInput {
    pub(in crate::layout::flex) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::flex) break_is_applicable: bool,
    pub(in crate::layout::flex) can_slice_at_fragmentainer_boundary: bool,
    pub(in crate::layout::flex) unit_block_end: FlexFragmentBlockOffset,
    pub(in crate::layout::flex) slice_start: FlexFragmentBlockOffset,
    pub(in crate::layout::flex) available_block_end: FlexFragmentBlockOffset,
}

impl FlexFragmentCursor {
    pub(in crate::layout::flex) fn new(
        content_top: PageTopBlockPosition,
        block_offset: FlexFragmentBlockOffset,
    ) -> Self {
        Self {
            content_top,
            block_offset,
        }
    }
}

/// Project a page fragmentainer's typed remaining capacity into flex's local
/// source block-offset coordinate system.
pub(in crate::layout::flex) fn flex_source_block_end_after_available_capacity(
    source_block_offset: FlexFragmentBlockOffset,
    available_content_block_size: LayoutLength,
) -> FlexFragmentBlockOffset {
    source_block_offset + FlexFragmentBlockSize::new(available_content_block_size.points())
}

impl FlexFragmentTransitionDecision {
    pub(in crate::layout::flex) fn forced(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: FlexFragmentBlockOffset,
    ) -> Self {
        Self {
            fragmentainer_kind,
            reason: FlexFragmentBreakReason::Forced,
            next_block_offset,
        }
    }

    pub(in crate::layout::flex) fn oversized_slice_progress(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: FlexFragmentBlockOffset,
    ) -> Self {
        Self {
            fragmentainer_kind,
            reason: FlexFragmentBreakReason::OversizedSliceProgress,
            next_block_offset,
        }
    }

    pub(in crate::layout::flex) fn slice_continuation(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: FlexFragmentBlockOffset,
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
        content_top: PageTopBlockPosition,
    ) -> FlexFragmentCursor {
        FlexFragmentCursor::new(content_top, self.next_block_offset)
    }
}

impl FlexUnitPrebreakDecision {
    pub(in crate::layout::flex) fn choose(input: FlexUnitPrebreakDecisionInput) -> Self {
        let required_block_size =
            (input.unit_block_end - input.cursor.block_offset).non_negative_size();
        let unit_overflows =
            required_block_size.points() > input.available_content_block_size.points() + 0.01;
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
            // A flex container can be entered at an already exhausted
            // fragmentainer boundary.  Even its sole item/line must then
            // advance as a whole when it fits in a fresh fragmentainer;
            // otherwise replay paints the complete unit into a zero-capacity
            // source slice.  This is a normal unforced break before the
            // flex boundary, not a writing-mode-specific exception.
            // <https://www.w3.org/TR/css-break-3/#breaking-rules>
            overflows: unit_overflows
                && can_keep_unit_together
                && (avoid_break
                    || input.has_prior_unit
                    || input.has_later_unit
                    || input.available_content_block_size.points() <= 0.01
                    // Fragmentainers use one CSS-pixel of numerical slack to
                    // avoid zero-sized arithmetic. In a column that slack is
                    // not usable layout capacity: an atomic flex line must
                    // advance rather than be micro-sliced into many 0.75pt
                    // source fragments.
                    || (input.fragmentainer_kind == FragmentainerKind::Column
                        && input.available_content_block_size.points()
                            <= css::CSS_PX_TO_PT + 0.01)),
            can_advance: input.can_advance,
        })
        .should_advance
        .then_some(FlexFragmentTransitionDecision {
            fragmentainer_kind: input.fragmentainer_kind,
            reason: FlexFragmentBreakReason::OverflowOrAvoid,
            // A class-A prebreak may occur after source space that contains
            // only a gap before the next flex line/item. Consume the part of
            // that gap that fit in the preceding fragmentainer, but never
            // consume the unit itself: the continuation cursor resumes at
            // the smaller of the fragmentainer source end and the unit start.
            // <https://www.w3.org/TR/css-break-3/#breaks-between>
            next_block_offset: FlexFragmentBlockOffset::new(
                input.unit_block_start.points().min(
                    flex_source_block_end_after_available_capacity(
                        input.cursor.block_offset,
                        input.available_content_block_size,
                    )
                    .points(),
                ),
            ),
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
            source_block_end: input.unit_block_end.points(),
            slice_start: input.slice_start.points(),
            available_block_end: input.available_block_end.points(),
        });
        let transition_before_paint = source_slice.advance_before_slice.then_some(
            FlexFragmentTransitionDecision::oversized_slice_progress(
                input.fragmentainer_kind,
                FlexFragmentBlockOffset::new(source_slice.slice_start),
            ),
        );
        Self {
            slice_start: FlexFragmentBlockOffset::new(source_slice.slice_start),
            slice_end: FlexFragmentBlockOffset::new(source_slice.slice_end),
            transition_before_paint,
        }
    }

    pub(in crate::layout::flex) fn paints_slice(self) -> bool {
        self.transition_before_paint.is_none()
    }
}
