use crate::css::{ComputedStyle, LineClamp};
use crate::layout::{BlockEndMarginCollapse, LayoutLength};

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct ChildFlowTraversalOutcome {
    pub(in crate::layout) pending_end_margin_collapse: Option<BlockEndMarginCollapse>,
    pub(in crate::layout) collapsed_start_margin_offset: LayoutLength,
}

/// The line-clamp budget shared by one block-flow child traversal.
///
/// The source clamp remains immutable so each child receives a style with the
/// currently available line slots, while replay restores the exact remaining
/// budget captured at the class-A break candidate.
pub(in crate::layout) struct BlockFlowChildTraversalState {
    source: Option<LineClamp>,
    remaining: Option<usize>,
    descendant_clamp_line_slots: usize,
}

impl BlockFlowChildTraversalState {
    pub(in crate::layout) fn new(style: &ComputedStyle) -> Self {
        Self {
            source: style.line_clamp.clone(),
            remaining: style.line_clamp.as_ref().map(|clamp| clamp.max_lines),
            descendant_clamp_line_slots: 0,
        }
    }

    pub(in crate::layout) fn is_exhausted(&self) -> bool {
        self.remaining == Some(0)
    }

    /// Capture the clamp portion of an avoid-break candidate explicitly.
    ///
    /// Builder snapshots restore paint and fragmentainer state, but the
    /// traversal's budget is deliberately local to the source traversal.
    pub(in crate::layout) fn capture_avoid_replay(&self) -> Option<usize> {
        self.remaining
    }

    /// Restore the budget associated with an avoid-break candidate before its
    /// source children are replayed.
    pub(in crate::layout) fn restore_avoid_replay(&mut self, remaining: Option<usize>) {
        self.remaining = remaining;
    }

    pub(in crate::layout) fn debit(&mut self, slots: usize) {
        if let Some(remaining) = &mut self.remaining {
            *remaining = remaining.saturating_sub(slots);
        }
    }

    pub(in crate::layout) fn apply_to(&self, style: &mut ComputedStyle) {
        if let (Some(limit), Some(mut clamp)) = (self.remaining, self.source.clone()) {
            clamp.max_lines = limit;
            style.line_clamp = Some(clamp);
        }
    }

    pub(in crate::layout) fn style_with_remaining(
        &self,
        style: &ComputedStyle,
    ) -> Option<ComputedStyle> {
        self.remaining.map(|_| {
            let mut style = style.clone();
            self.apply_to(&mut style);
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
