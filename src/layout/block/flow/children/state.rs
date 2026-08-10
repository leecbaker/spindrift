use crate::css::{
    AutomaticBlockSizeTraversal, ClampContinuation, ComputedStyle, LineLimitTraversal,
    RemainingLineSlots,
};
use crate::document::paint::geometry::{PaintPoint, PaintRect, PaintSize};
use crate::layout::{
    BlockEndMarginCollapse, LayoutLength, style_establishes_multicol_formatting_context,
};
use crate::units::{ContentBoxLength, SemanticLengthExt, content_box_pt, layout_pt};
use std::num::NonZeroUsize;

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct ChildFlowTraversalOutcome {
    pub(in crate::layout) pending_end_margin_collapse: Option<BlockEndMarginCollapse>,
    pub(in crate::layout) collapsed_start_margin_offset: LayoutLength,
    /// Static geometry exported by the rendered-legend source child, when
    /// this traversal owns an HTML fieldset.  It is deliberately kept out of
    /// the ordinary block cursor state: the legend's border interruption is a
    /// parent decoration concern, not an ink-bound-based flow adjustment.
    pub(in crate::layout) rendered_legend: Option<RenderedLegendGeometry>,
}

/// Untransformed source-fragment geometry required by HTML's rendered-legend
/// border rule.
///
/// The margin rectangle is retained separately because HTML excludes the
/// fieldset border behind the legend's *margin box*, while the legend itself
/// paints its border box in normal CSS paint order.
///
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct RenderedLegendGeometry {
    pub(in crate::layout) border_box: PaintRect,
    pub(in crate::layout) margin_box: PaintRect,
    pub(in crate::layout) source_fragmentainer: usize,
}

impl RenderedLegendGeometry {
    /// Derive the static margin rectangle from an already resolved physical
    /// border rectangle.  CSS logical margins have been mapped to these four
    /// physical used edges at this boundary.
    pub(in crate::layout) fn from_static_border_box(
        border_box: PaintRect,
        margins: crate::css::Edges,
        source_fragmentainer: usize,
    ) -> Self {
        let margin_box = PaintRect::new(
            PaintPoint::new(
                border_box.origin.x - margins.left,
                border_box.origin.y - margins.bottom,
            ),
            PaintSize::new(
                border_box.size.width + margins.left + margins.right,
                border_box.size.height + margins.top + margins.bottom,
            ),
        );
        Self {
            border_box,
            margin_box,
            source_fragmentainer,
        }
    }

    /// Derive the fieldset-border-only visible regions around this legend's
    /// static margin rectangle.  This is a physical paint-boundary type: the
    /// CSS logical sides have already been mapped by the resolved layout
    /// geometry above.
    ///
    /// <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
    /// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
    pub(in crate::layout) fn border_exclusion(
        self,
        fieldset_border_box: PaintRect,
        source_fragmentainer: usize,
    ) -> Option<RenderedLegendBorderExclusion> {
        if self.source_fragmentainer != source_fragmentainer {
            return None;
        }
        let hole = self.margin_box.intersection(&fieldset_border_box)?;
        // An auto-sized ordinary block spans the fieldset's entire inline
        // axis. Do not turn that unresolved intermediate geometry into a
        // whole-border erasure: the HTML fit-content override must establish
        // the rendered legend's finite inline span before border exclusion is
        // meaningful.
        if hole.size.width >= fieldset_border_box.size.width - 0.01
            || hole.size.height >= fieldset_border_box.size.height - 0.01
        {
            return None;
        }
        let mut visible_regions = Vec::with_capacity(4);
        let push = |regions: &mut Vec<PaintRect>, x, y, width, height| {
            if width > 0.0 && height > 0.0 {
                regions.push(PaintRect::new(
                    PaintPoint::new(x, y),
                    PaintSize::new(width, height),
                ));
            }
        };
        push(
            &mut visible_regions,
            fieldset_border_box.min_x(),
            fieldset_border_box.min_y(),
            hole.min_x() - fieldset_border_box.min_x(),
            fieldset_border_box.size.height,
        );
        push(
            &mut visible_regions,
            hole.max_x(),
            fieldset_border_box.min_y(),
            fieldset_border_box.max_x() - hole.max_x(),
            fieldset_border_box.size.height,
        );
        push(
            &mut visible_regions,
            hole.min_x(),
            fieldset_border_box.min_y(),
            hole.size.width,
            hole.min_y() - fieldset_border_box.min_y(),
        );
        push(
            &mut visible_regions,
            hole.min_x(),
            hole.max_y(),
            hole.size.width,
            fieldset_border_box.max_y() - hole.max_y(),
        );
        (!visible_regions.is_empty()).then_some(RenderedLegendBorderExclusion { visible_regions })
    }
}

/// Physical border-paint regions left after subtracting a rendered legend's
/// static margin rectangle.  This applies to border primitives only; fieldset
/// backgrounds retain their ordinary CSS paint area.
///
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct RenderedLegendBorderExclusion {
    pub(in crate::layout) visible_regions: Vec<PaintRect>,
}

/// The line-clamp budget shared by one block-flow child traversal.
///
/// The source clamp remains immutable so each child receives a style with the
/// currently available line slots, while replay restores the exact remaining
/// budget captured at the class-A break candidate.
///
/// A Category-3 discard break can retain a non-empty source prefix. Keep that
/// endpoint opaque so the multicol replay path cannot accidentally turn it
/// into a page or column fragmentainer index.
/// <https://drafts.csswg.org/css-overflow-4/#continue>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct DiscardSourcePrefix(NonZeroUsize);

impl DiscardSourcePrefix {
    pub(in crate::layout) fn child_count(self) -> usize {
        self.0.get()
    }
}

/// A non-empty local region allowance for the discard controller.
///
/// Page and column fragmentainers never use this type; it exists only at the
/// block-flow source boundary for Category-3 discard.
/// <https://drafts.csswg.org/css-overflow-4/#continue>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct DiscardRegionLimit(NonZeroUsize);

impl DiscardRegionLimit {
    pub(in crate::layout) const fn new(value: NonZeroUsize) -> Self {
        Self(value)
    }

    fn reached_by(self, completed_local_regions: usize) -> bool {
        completed_local_regions >= self.0.get()
    }
}

pub(in crate::layout) struct BlockFlowChildTraversalState {
    source: Option<LineLimitTraversal>,
    remaining: Option<RemainingLineSlots>,
    automatic: Option<AutomaticBlockSizeTraversal>,
    local_continuation_cutoff: bool,
    discard_region_limit: Option<DiscardRegionLimit>,
    discard_region_traversal: Option<crate::css::DiscardRegionTraversal>,
    descendant_clamp_line_slots: usize,
}

/// Resolve the finite non-percentage used block-size constraint that can be
/// handed from an automatic clamp container to its in-flow descendants.
///
/// Percentage constraints require the final containing-block basis and are
/// deliberately resolved by the inline controller once that basis exists.
/// This adapter handles absolute and `lh` values at the owning block's used
/// line-height boundary.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
pub(in crate::layout) fn direct_automatic_block_size_constraint(
    style: &ComputedStyle,
) -> Option<ContentBoxLength> {
    let applies = match style.used_continuation() {
        crate::css::UsedContinuation::LineClamp(container) => {
            matches!(
                container.cutoff,
                crate::css::ClampPointRule::AutomaticBlockSize
            )
        }
        crate::css::UsedContinuation::Discard(container) => {
            matches!(container.max_lines, crate::css::MaxLines::None)
        }
        crate::css::UsedContinuation::Ordinary => false,
    };
    if !applies {
        return None;
    }
    let mut height = style.box_values.height.clone();
    let mut min_height = style.box_values.min_height.clone();
    let mut max_height = style.box_values.max_height.clone();
    let line_height = layout_pt(style.line_height);
    height.resolve_line_height_relative_lengths(line_height);
    min_height.resolve_line_height_relative_lengths(line_height);
    max_height.resolve_line_height_relative_lengths(line_height);
    let upper = height
        .length_if_no_percent()
        .into_iter()
        .chain(max_height.length_if_no_percent())
        .reduce(f32::min)?;
    let used = min_height
        .length_if_no_percent()
        .map_or(upper, |minimum| upper.max(minimum));
    Some(content_box_pt(used.max(0.0)))
}

impl BlockFlowChildTraversalState {
    /// Convert source-order reachability into the terminal clamp signal.
    /// Keeping the conversion here prevents traversal sites from treating an
    /// out-of-flow effect as overflow by accident.
    pub(in crate::layout) fn continuation_for_later_in_flow_source(
        has_later_in_flow_source: bool,
    ) -> ClampContinuation {
        if has_later_in_flow_source {
            ClampContinuation::LaterInFlowContent
        } else {
            ClampContinuation::None
        }
    }

    pub(in crate::layout) fn new(
        style: &ComputedStyle,
        automatic_constraint: Option<ContentBoxLength>,
    ) -> Self {
        // `continue: collapse`, which `line-clamp` sets, behaves as `auto`
        // on multicol containers. Do not create a budget that descendants
        // could accidentally spend while the multicol implementation lays
        // its independent formatting contexts.
        // <https://drafts.csswg.org/css-overflow-4/#continue>
        let source = (!style_establishes_multicol_formatting_context(style))
            .then(|| {
                style
                    .line_clamp_container()
                    .map(LineLimitTraversal::from_container)
            })
            .flatten();
        let remaining = source.as_ref().map(|clamp| clamp.remaining);
        let automatic = style.automatic_block_size_traversal.clone().or_else(|| {
            (!style_establishes_multicol_formatting_context(style))
                .then(|| match style.used_continuation() {
                    crate::css::UsedContinuation::LineClamp(container)
                        if matches!(
                            container.cutoff,
                            crate::css::ClampPointRule::AutomaticBlockSize
                        ) =>
                    {
                        automatic_constraint.map(|remaining| {
                            AutomaticBlockSizeTraversal::new(remaining, container.marker.clone())
                        })
                    }
                    crate::css::UsedContinuation::Discard(container)
                        if matches!(container.max_lines, crate::css::MaxLines::None) =>
                    {
                        automatic_constraint.map(|remaining| {
                            AutomaticBlockSizeTraversal::new(remaining, container.marker.clone())
                        })
                    }
                    crate::css::UsedContinuation::Ordinary
                    | crate::css::UsedContinuation::LineClamp(_)
                    | crate::css::UsedContinuation::Discard(_) => None,
                })
                .flatten()
        });
        Self {
            remaining,
            source,
            automatic,
            local_continuation_cutoff: false,
            discard_region_limit: None,
            discard_region_traversal: matches!(
                style.used_continuation(),
                crate::css::UsedContinuation::Discard(_)
            )
            .then_some(crate::css::DiscardRegionTraversal::default()),
            descendant_clamp_line_slots: 0,
        }
    }

    pub(in crate::layout) fn is_exhausted(&self) -> bool {
        self.local_continuation_cutoff
            || self.remaining == Some(RemainingLineSlots::Exhausted)
            || self
                .automatic
                .as_ref()
                .is_some_and(AutomaticBlockSizeTraversal::is_exhausted)
    }

    /// Capture a local automatic clamp point or Category-3 discard break
    /// selected by a preceding direct inline run. This flag intentionally
    /// affects only source traversal; it has no fragmentainer transition API.
    pub(in crate::layout) fn mark_local_continuation_cutoff(&mut self) {
        self.local_continuation_cutoff = true;
    }

    pub(in crate::layout) fn has_local_continuation_cutoff(&self) -> bool {
        self.local_continuation_cutoff
    }

    /// Capture the retained source prefix at the first local discard break.
    /// The non-zero constructor is the source-boundary adapter: a local
    /// region can only be exhausted after at least one source child entered
    /// the temporary multicol flow.
    pub(in crate::layout) fn capture_discard_source_prefix(&mut self, child_count: NonZeroUsize) {
        self.mark_local_continuation_cutoff();
        let Some(traversal) = self.discard_region_traversal.as_mut() else {
            debug_assert!(
                false,
                "only a discard controller captures a local region break"
            );
            return;
        };
        traversal.capture_overflow(crate::css::RegionOverflowPoint::after_direct_children(
            child_count,
        ));
    }

    /// Capture the forced max-lines boundary before a later normal-flow
    /// child. This constructs a block endpoint only for the discard
    /// controller; collapse continues to use its inline terminal-line path.
    pub(in crate::layout) fn capture_forced_discard_before_later_child(
        &mut self,
        later_child_index: usize,
    ) {
        let Some(traversal) = self.discard_region_traversal.as_mut() else {
            return;
        };
        let point = match later_child_index.checked_sub(1) {
            Some(preceding_in_flow_child_index) => crate::css::ClampPoint::BetweenBlockSiblings(
                crate::css::BlockClampPoint::after_in_flow_child(preceding_in_flow_child_index),
            ),
            None => crate::css::ClampPoint::AtContainerStart,
        };
        traversal.capture_forced_after_lines(point);
    }

    pub(in crate::layout) fn discard_source_prefix(&self) -> Option<DiscardSourcePrefix> {
        self.discard_region_traversal.and_then(|traversal| {
            traversal.first_break().and_then(|break_| match break_ {
                crate::css::CapturedRegionBreak::Overflow(point) => {
                    Some(DiscardSourcePrefix(point.retained_direct_children()))
                }
                crate::css::CapturedRegionBreak::ForcedAfterLines(
                    crate::css::ClampPoint::BetweenBlockSiblings(point),
                ) => {
                    // This accessor is intentionally local: a forced region
                    // break has no replayable direct-child prefix, but its
                    // block endpoint remains observable to this controller.
                    let _ = point.preceding_in_flow_child_index();
                    None
                }
                crate::css::CapturedRegionBreak::ForcedAfterLines(
                    crate::css::ClampPoint::AtContainerStart
                    | crate::css::ClampPoint::AfterInlineLine(_),
                ) => None,
            })
        })
    }

    pub(in crate::layout) fn set_discard_region_limit(
        &mut self,
        limit: Option<DiscardRegionLimit>,
    ) {
        self.discard_region_limit = limit;
    }

    /// The current local region is represented by the builder's temporary
    /// completed-column count. Reaching the limit suppresses source before a
    /// later child can be placed into another temporary region.
    pub(in crate::layout) fn has_reached_discard_region_limit(
        &self,
        completed_local_regions: usize,
    ) -> bool {
        self.discard_region_limit
            .is_some_and(|limit| limit.reached_by(completed_local_regions))
    }

    /// Commit an inline result at this source boundary.  The typed local
    /// cutoff must travel with the otherwise legacy scalar line-slot count.
    pub(in crate::layout) fn debit_inline_outcome(
        &mut self,
        outcome: crate::layout::inline_layout::InlineLayoutOutcome,
    ) {
        self.debit_rendered_slots(outcome.clamp_line_slots);
        self.debit_automatic_block_contribution(outcome.clamp_block_advance);
        if outcome.has_local_continuation_cutoff {
            self.mark_local_continuation_cutoff();
        }
    }

    /// The number of source line slots that may still be committed by this
    /// traversal.  A source-order side effect can use this to decide whether
    /// a speculative inline run reached the clamp boundary before committing
    /// its own paint or geometry.
    pub(in crate::layout) fn remaining_line_slots(&self) -> Option<RemainingLineSlots> {
        self.remaining
    }

    /// Whether this traversal is spending an ancestor's clamp budget.
    ///
    /// A child's own `line-clamp` must not treat its later siblings as source
    /// that continues *inside* that child. Only a clamp propagated by this
    /// traversal can cross a child boundary.
    pub(in crate::layout) fn has_active_clamp(&self) -> bool {
        self.remaining.is_some() || self.automatic.is_some()
    }

    /// An automatic clamp may cross a specified zero-height block to find
    /// the terminal same-BFC line. Numeric clamps cannot: their endpoint is
    /// already fixed by line count.
    pub(in crate::layout) fn admits_zero_height_automatic_child(
        &self,
        child_style: &ComputedStyle,
    ) -> bool {
        self.remaining.is_none()
            && self
                .automatic
                .as_ref()
                .is_some_and(AutomaticBlockSizeTraversal::is_exhausted)
            && child_style
                .box_values
                .height
                .length_if_no_percent()
                .is_some_and(|height| height.abs() <= 0.01)
    }

    /// Capture the clamp portion of an avoid-break candidate explicitly.
    ///
    /// Builder snapshots restore paint and fragmentainer state, but the
    /// traversal's budget is deliberately local to the source traversal.
    pub(in crate::layout) fn capture_avoid_replay(&self) -> Option<RemainingLineSlots> {
        self.remaining
    }

    /// Restore the budget associated with an avoid-break candidate before its
    /// source children are replayed.
    pub(in crate::layout) fn restore_avoid_replay(
        &mut self,
        remaining: Option<RemainingLineSlots>,
    ) {
        self.remaining = remaining;
    }

    pub(in crate::layout) fn debit(&mut self, slots: crate::css::PositiveLineCount) {
        if let Some(remaining) = self.remaining {
            self.remaining = Some(remaining.debit(slots));
        }
    }

    /// Convert the untyped count emitted at the inline-layout boundary into a
    /// positive count before mutating the traversal state.
    pub(in crate::layout) fn debit_rendered_slots(&mut self, slots: usize) {
        if let Some(slots) = crate::css::PositiveLineCount::from_rendered_slots(slots) {
            self.debit(slots);
        }
    }

    /// Propagate the remaining budget together with a known later in-flow
    /// continuation. The inline selector consumes this only at the terminal
    /// line, so an intermediate child cannot create a marker.
    pub(in crate::layout) fn apply_to_with_continuation(
        &self,
        style: &mut ComputedStyle,
        continuation: ClampContinuation,
    ) {
        if let (Some(remaining), Some(clamp)) = (self.remaining, self.source.as_ref()) {
            style.line_limit_traversal = Some(
                clamp
                    .with_remaining(remaining)
                    .with_continuation(continuation),
            );
        }
        if let Some(automatic) = self.automatic.as_ref() {
            style.automatic_block_size_traversal = Some(automatic.clone());
        }
    }

    /// Reserve a child's full normal-flow non-content block contribution
    /// before its inline controller selects a source line. This keeps a
    /// descendant's candidate clamp point in the parent content-box coordinate
    /// system: both the leading and trailing margin/border/padding must fit.
    pub(in crate::layout) fn reserve_automatic_child_non_content(
        style: &mut ComputedStyle,
        contribution: ContentBoxLength,
    ) {
        debug_assert!(contribution.points() >= 0.0);
        if let Some(automatic) = style.automatic_block_size_traversal.as_mut() {
            automatic.debit(contribution);
        }
    }

    /// The next block's non-content envelope reaches the current automatic
    /// boundary, so the preceding same-BFC terminal line—not the following
    /// block boundary—owns the marker.
    pub(in crate::layout) fn require_automatic_terminal_marker_when_full(
        style: &mut ComputedStyle,
    ) {
        if let Some(automatic) = style.automatic_block_size_traversal.take() {
            style.automatic_block_size_traversal = Some(automatic.with_terminal_marker_when_full());
        }
    }

    /// Pass a marker-only automatic boundary through a specified zero-height
    /// child. The child cannot spend additional parent block size, so its
    /// final eligible line—not a fabricated zero allowance—selects the
    /// marker.
    pub(in crate::layout) fn apply_zero_height_automatic_boundary(
        &self,
        style: &mut ComputedStyle,
        has_later_in_flow_source: bool,
    ) {
        if self.admits_zero_height_automatic_child(style)
            && let Some(automatic) = self.automatic.as_ref()
        {
            style.automatic_block_size_traversal = None;
            style.automatic_block_boundary_marker = has_later_in_flow_source
                .then(|| crate::css::AutomaticBlockBoundaryMarker(automatic.marker().clone()));
        }
    }

    /// Commit the normal-flow block contribution that a child supplied to an
    /// ancestor automatic clamp container. This is intentionally separate
    /// from line-slot debiting: a block boundary is a legal cutoff with no
    /// marker line.
    pub(in crate::layout) fn debit_automatic_block_contribution(&mut self, used: ContentBoxLength) {
        if let Some(automatic) = self.automatic.as_mut() {
            automatic.debit(used);
        }
    }

    #[cfg(test)]
    pub(in crate::layout) fn apply_to(&self, style: &mut ComputedStyle) {
        self.apply_to_with_continuation(style, ClampContinuation::None);
    }

    pub(in crate::layout) fn style_with_remaining(
        &self,
        style: &ComputedStyle,
    ) -> Option<ComputedStyle> {
        self.style_with_remaining_and_continuation(style, ClampContinuation::None)
    }

    /// Clone a layout style for an inline run while preserving the shared
    /// budget and a known later in-flow continuation.
    pub(in crate::layout) fn style_with_remaining_and_continuation(
        &self,
        style: &ComputedStyle,
        continuation: ClampContinuation,
    ) -> Option<ComputedStyle> {
        (self.remaining.is_some() || self.automatic.is_some()).then(|| {
            let mut style = style.clone();
            self.apply_to_with_continuation(&mut style, continuation);
            style
        })
    }

    pub(in crate::layout) fn record_descendant_clamp_line_slots(&mut self, slots: usize) {
        self.descendant_clamp_line_slots += slots;
    }

    pub(in crate::layout) fn descendant_clamp_line_slots(&self) -> usize {
        self.descendant_clamp_line_slots
    }
}
