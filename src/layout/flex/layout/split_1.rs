use super::*;

/// How a replayed flex item's root formatting context obtains its descendant
/// percentage-height basis.
///
/// `Override(Indefinite)` is intentionally distinct from deriving a basis from
/// the temporary replayed style: Flexbox can assign a numeric used height
/// without making that height definite for percentage descendants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum FlexDescendantPercentageHeightBasis {
    DeriveFromContainer,
    Override(BlockSizePercentageBasis),
}

impl FlexDescendantPercentageHeightBasis {
    pub(in crate::layout::flex) fn available_height_basis(
        self,
        container_height: Option<ContentBoxLength>,
    ) -> FlexAvailablePercentageBasis {
        match self {
            Self::DeriveFromContainer => flex_available_percentage_basis(
                container_height,
                FlexAvailableSizeSource::ContainingBlock,
            ),
            Self::Override(basis) => basis.map_source(|_| FlexAvailableSizeSource::ContainingBlock),
        }
    }

    pub(in crate::layout::flex) fn override_basis(self) -> Option<BlockSizePercentageBasis> {
        match self {
            Self::DeriveFromContainer => None,
            Self::Override(basis) => Some(basis),
        }
    }
}

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
    /// Physical page-inline span of the flex container's content box.
    pub(in crate::layout::flex) inner_inline_span: PageInlineSpan,
    pub(in crate::layout::flex) content_top: PageTopBlockPosition,
    /// Source block offset of the temporary fragmentainer currently owning a
    /// deferred positioned child. Static flex geometry is initially expressed
    /// in the unfragmented flex source coordinate system, so it must be
    /// localized before multicolumn projection chooses its destination.
    pub(in crate::layout::flex) source_fragment_block_offset: FlexFragmentBlockOffset,
    /// Source block capacity of the first committed flex fragmentainer.
    /// A definite physical `top` inside this range remains in the original
    /// source fragment; only a later inset needs candidate projection.
    pub(in crate::layout::flex) first_fragment_source_block_size: FlexFragmentBlockSize,
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

/// Page-local geometry supplied when flex gap decorations are emitted for one
/// fragmented container slice. It keeps the content span, physical content
/// height, and paint clip from being recombined as unrelated scalars.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexGapDecorationFragmentContext {
    pub(in crate::layout::flex) page_index: usize,
    pub(in crate::layout::flex) content_inline_span: PageInlineSpan,
    pub(in crate::layout::flex) content_height: PhysicalContentHeight,
    pub(in crate::layout::flex) fragment_bounds: PaintClip,
    pub(in crate::layout::flex) has_forced_item_breaks: bool,
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

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct SplitFlexItemPaintContext {
    /// The used physical border-box dimensions from the flex algorithm.
    /// These must not be confused with the content-box percentage bases used
    /// when replaying descendants.
    pub(in crate::layout::flex) item_width: BorderBoxLength,
    pub(in crate::layout::flex) item_height: BorderBoxLength,
    pub(in crate::layout::flex) percentage_height_basis: FlexPercentageBasis,
    pub(in crate::layout::flex) slice_border_box: PaintClip,
    pub(in crate::layout::flex) source_item_top: PageTopBlockPosition,
    /// Committed source range and item-local ordinal for this replay. This is
    /// produced by the materialized flex fragment plan, rather than guessed
    /// from a page-sized source offset during painting.
    pub(in crate::layout::flex) continuation: FlexItemContinuation,
    /// Whether this fragment preserves the item’s source coordinate system.
    /// Row flex lines and wrapped column lines use a contiguous source slice,
    /// while a single-line column continuation retains its main-axis replay
    /// path until it has explicit fragment-local relayout state.
    pub(in crate::layout::flex) replay_source_slice_offset: bool,
    /// Descendant paint overflow extends the flex source interval without
    /// becoming a child formatting-context continuation. Such a child keeps
    /// one source paint tree while flex clips it across its own fragments.
    pub(in crate::layout::flex) has_descendant_source_overflow: bool,
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

impl SplitFlexItemPaintContext {
    /// Adapt the frozen source border-box extent to the nested replay
    /// formatting-context availability. Replay applies its frozen box metrics
    /// itself, so this is intentionally not a general box-model conversion.
    pub(in crate::layout::flex) fn available_width_for_replay(&self) -> PhysicalContentWidth {
        PhysicalContentWidth::new(content_box_pt(self.item_width.points()))
    }

    /// See [`Self::available_width_for_replay`].
    pub(in crate::layout::flex) fn available_height_for_replay(&self) -> PhysicalContentHeight {
        PhysicalContentHeight::new(content_box_pt(self.item_height.points()))
    }
}

pub(in crate::layout::flex) fn placed_flex_item_style(
    child_style: &ComputedStyle,
    item_width: BorderBoxLength,
    item_height: BorderBoxLength,
    physical_direction: PhysicalFlexDirection,
) -> ComputedStyle {
    // CSS used-value setters remain scalar legacy APIs. The flex layout
    // boundary nevertheless records the final dimensions as border-box
    // extents, so extract only after entering this style adapter.
    let item_width = item_width.points();
    let item_height = item_height.points();
    let mut placed_style =
        replayed_item_fragmentation_base_style(child_style, ReplayedItemFragmentationPolicy::Flex);
    let borders = used_border_widths(child_style);
    let horizontal_non_content =
        child_style.padding.left + child_style.padding.right + borders.left + borders.right;
    let vertical_non_content =
        child_style.padding.top + child_style.padding.bottom + borders.top + borders.bottom;
    let table_content_box =
        child_style.display.is_table() && matches!(child_style.box_sizing, BoxSizing::ContentBox);
    // Taffy's resolved cross size is an outer size, while the flex main-size
    // adapter records a content-box main size for ordinary content-box items.
    // In particular, a physical column's resolved main height is already an
    // outer size after its automatic minimum transferred through a replaced
    // item's aspect ratio. Do not add the vertical decoration a second time
    // while freezing that replay size. A content-box table wrapper remains a
    // separate adapter because table replay consumes its grid content box.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-item-sizing>
    // <https://drafts.csswg.org/css-flexbox-1/#definite-sizes> and
    // <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>.
    let main_size_is_content_box =
        !table_content_box && matches!(child_style.box_sizing, BoxSizing::ContentBox);
    let used_width = if table_content_box && !physical_direction.is_row_axis() {
        (item_width - horizontal_non_content).max(0.0)
    } else if main_size_is_content_box && physical_direction.is_row_axis() {
        item_width + horizontal_non_content
    } else {
        item_width
    };
    let used_height = if table_content_box && physical_direction.is_row_axis() {
        (item_height - vertical_non_content).max(0.0)
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
    if physical_direction.is_row_axis() {
        set_style_used_content_width_bounds(
            &mut placed_style,
            border_box_to_content_box_length(
                border_box_pt(item_width),
                non_content_pt(horizontal_non_content),
            ),
        );
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
            if child.style.display.is_table() {
                layout.layout_formatting_context_item_contents(child, placed_style, stylesheets);
                return;
            }
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

    /// Lay out a nested continuation source for a split flex item.
    ///
    /// Unlike [`Self::layout_flex_item_contents`], this keeps child
    /// fragmentation enabled: the resulting local fragment sequence is
    /// committed by the flex item's replay record and later continuations
    /// select that sequence by ordinal. Suppressing fragmentation here would
    /// make every later flex slice reconstruct a single monolithic child tree.
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>
    pub(in crate::layout::flex) fn layout_split_flex_item_continuation_contents(
        &mut self,
        child: &StyledChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        percentage_height_basis: FlexPercentageBasis,
    ) {
        self.with_replayed_flex_item_percentage_height_basis(percentage_height_basis, |layout| {
            layout.layout_formatting_context_item_contents(child, placed_style, stylesheets);
        });
    }
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
    fragmentainer: Fragmentainer,
) -> FlexFragmentBlockOffset {
    source_block_offset + FlexFragmentBlockSize::new(fragmentainer.available_block_size().points())
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
        let unit_overflows = input
            .current_fragmentainer
            .required_block_size_overflows(layout_pt(required_block_size.points()));
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
                    || input.current_fragmentainer.available_block_size().points() <= 0.01
                    // Fragmentainers use one CSS-pixel of numerical slack to
                    // avoid zero-sized arithmetic. In a column that slack is
                    // not usable layout capacity: an atomic flex line must
                    // advance rather than be micro-sliced into many 0.75pt
                    // source fragments.
                    || (input.fragmentainer_kind == FragmentainerKind::Column
                        && input.current_fragmentainer.available_block_size().points()
                            <= css::CSS_PX_TO_PT + 0.01)),
            avoid_break,
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
                        input.current_fragmentainer,
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
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
    intrinsic: FlexItemEstimate,
    shrink_auto_width: bool,
) -> PhysicalContentWidth {
    let min_content = PhysicalContentWidth::new(intrinsic.min_width);
    let max_content = flex_container_shrink_to_fit_max_content_width(
        style,
        available_outer_width,
        horizontal_non_content,
        min_content,
        PhysicalContentWidth::new(content_box_pt(
            intrinsic.width.points().max(min_content.points()).max(0.0),
        )),
        shrink_auto_width,
    );
    let auto_width = if shrink_auto_width {
        intrinsic::IntrinsicAutoWidth::ShrinkToFit
    } else {
        intrinsic::IntrinsicAutoWidth::FillAvailable
    };
    PhysicalContentWidth::new(intrinsic::content_box_width_from_intrinsic(
        style,
        available_outer_width,
        horizontal_non_content,
        min_content.content_box_length(),
        max_content.content_box_length(),
        auto_width,
    ))
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
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
    min_content: PhysicalContentWidth,
    max_content: PhysicalContentWidth,
    shrink_auto_width: bool,
) -> PhysicalContentWidth {
    if !shrink_auto_width
        || !style.box_values.width.is_auto()
        || style.flex_wrap == FlexWrap::NoWrap
        || !physical_flex_direction(style).is_column_axis()
        || (style.flex_wrap.balances_lines() && style.flex_line_count.is_some())
    {
        return max_content;
    }

    let available_content_width =
        (available_outer_width.points() - horizontal_non_content.points()).max(0.0);
    if available_content_width > max_content.points() + 0.01 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendant_percentage_height_basis_preserves_override_definiteness() {
        assert_eq!(
            FlexDescendantPercentageHeightBasis::DeriveFromContainer
                .available_height_basis(Some(content_box_pt(24.0))),
            PercentageBasis::definite_from(
                content_box_pt(24.0),
                FlexAvailableSizeSource::ContainingBlock,
            )
        );

        let definite = FlexDescendantPercentageHeightBasis::Override(
            PercentageBasis::definite_from(content_box_pt(48.0), BlockSizeBasisSource::FlexItem),
        )
        .available_height_basis(None);
        assert_eq!(
            definite,
            PercentageBasis::definite_from(
                content_box_pt(48.0),
                FlexAvailableSizeSource::ContainingBlock,
            )
        );

        let indefinite =
            FlexDescendantPercentageHeightBasis::Override(PercentageBasis::indefinite())
                .available_height_basis(Some(content_box_pt(48.0)));
        assert_eq!(indefinite, PercentageBasis::indefinite());
    }

    #[test]
    fn placed_column_item_does_not_duplicate_its_border_box_decoration() {
        let mut child = ComputedStyle::initial();
        child.box_sizing = BoxSizing::ContentBox;
        child.padding.top = 2.0;
        child.padding.bottom = 3.0;

        let placed = placed_flex_item_style(
            &child,
            border_box_pt(40.0),
            border_box_pt(20.0),
            PhysicalFlexDirection::new(FlexDirection::ColumnReverse),
        );

        let height = used_length_percentage_or_auto_with_basis(
            placed.box_values.height,
            PercentageBasis::<ContentBoxLength>::indefinite(),
        )
        .expect("the replayed physical main size is definite");
        assert_eq!(height.points(), 20.0);
    }

    #[test]
    fn vertical_logical_row_projects_item_main_intervals_to_fragmentainers() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.flex_direction = FlexDirection::Row;
        assert_eq!(
            FlexFragmentationBoundaryProjection::for_style(&style),
            FlexFragmentationBoundaryProjection::ItemMainAxis,
        );

        style.writing_mode = WritingMode::HorizontalTb;
        assert_eq!(
            FlexFragmentationBoundaryProjection::for_style(&style),
            FlexFragmentationBoundaryProjection::LineCrossAxis,
        );
    }

    #[test]
    fn boundary_projection_returns_physical_fragmentainer_intervals() {
        let mut vertical_row = ComputedStyle::initial();
        vertical_row.writing_mode = WritingMode::VerticalRl;
        vertical_row.flex_direction = FlexDirection::Row;
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(40.0, 30.0),
            ContainerSize::new(20.0, 50.0),
        ));
        assert_eq!(
            FlexFragmentationBoundaryProjection::for_style(&vertical_row)
                .item_main_block_bounds(&item, false),
            FlexFragmentBlockBounds::new(
                FlexFragmentBlockOffset::new(30.0),
                FlexFragmentBlockOffset::new(80.0),
            ),
            "a vertical logical row fragments along its physical-Y main axis",
        );

        let horizontal_row = ComputedStyle::initial();
        let line = FlexLineLayout {
            item_indices: vec![0],
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(50.0),
            cross_start: FlexCrossOffset::new(30.0),
            cross_end: FlexCrossOffset::new(80.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        assert_eq!(
            FlexFragmentationBoundaryProjection::for_style(&horizontal_row)
                .line_cross_block_bounds(&line),
            FlexFragmentBlockBounds::new(
                FlexFragmentBlockOffset::new(30.0),
                FlexFragmentBlockOffset::new(80.0),
            ),
            "a horizontal physical row fragments along its physical-Y cross axis",
        );
    }

    #[test]
    fn intrinsic_flex_container_width_projects_to_physical_content_width() {
        let style = ComputedStyle::initial();
        let intrinsic = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(90.0)),
            PhysicalContentHeight::new(content_box_pt(20.0)),
        );

        let width = flex_container_content_width_from_intrinsic(
            &style,
            layout_pt(120.0),
            non_content_pt(20.0),
            intrinsic,
            false,
        );

        let _: PhysicalContentWidth = width;
        let _: ContentBoxLength = intrinsic.min_width;
        assert_eq!(width.points(), 100.0);
    }

    #[test]
    fn shrink_to_fit_compares_typed_outer_and_content_widths() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Column;
        style.flex_wrap = FlexWrap::Wrap;

        let width = flex_container_shrink_to_fit_max_content_width(
            &style,
            layout_pt(120.0),
            non_content_pt(20.0),
            PhysicalContentWidth::new(content_box_pt(30.0)),
            PhysicalContentWidth::new(content_box_pt(90.0)),
            true,
        );

        assert_eq!(width, PhysicalContentWidth::new(content_box_pt(30.0)));
    }
}
