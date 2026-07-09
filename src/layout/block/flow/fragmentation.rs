use super::*;

/// Inputs for deciding whether a definite-height block should prebreak.
///
/// CSS Fragmentation allows class A breaks between sibling block boxes before
/// layout. Keeping those inputs together makes the decision explicit while
/// allowing avoid-retry pagination state to tailor the rule:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
pub(in crate::layout) struct DefiniteBlockBreakContext<'a> {
    pub(in crate::layout) definite_content_height: Option<f32>,
    pub(in crate::layout) vertical_extras: f32,
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) current_fragmentainer: Fragmentainer,
    /// The empty destination fragmentainer selected for a prebreak.
    ///
    /// Page sequences may change the used page size, so this can differ from
    /// the current fragmentainer even for an ordinary class A page break.
    pub(in crate::layout) empty_destination_fragmentainer: Fragmentainer,
    pub(in crate::layout) fragmentainer_has_occupied_flow: bool,
    pub(in crate::layout) at_page_top: bool,
    pub(in crate::layout) suppress_for_avoid_retry: bool,
}

pub(in crate::layout) struct AvoidBreakRunCandidateMeta {
    pub(in crate::layout) index: usize,
    pub(in crate::layout) element_index: usize,
    pub(in crate::layout) previous_flow_bottom_margin: Option<f32>,
    pub(in crate::layout) seen_flow_child: bool,
    pub(in crate::layout) trim_block_start_adjoining_margins: bool,
    pub(in crate::layout) collapsed_end_margin: bool,
    pub(in crate::layout) previous_child_page_end: Option<Option<String>>,
    pub(in crate::layout) float_run: FloatRunState,
    pub(in crate::layout) remaining_line_clamp: Option<usize>,
    pub(in crate::layout) height: f32,
}

pub(in crate::layout) struct PendingAvoidBreakRunCandidate {
    pub(in crate::layout) meta: AvoidBreakRunCandidateMeta,
}

pub(in crate::layout) struct AvoidBreakRunCandidate {
    snapshot: Box<LayoutSnapshot>,
    pub(in crate::layout) meta: AvoidBreakRunCandidateMeta,
}

impl PendingAvoidBreakRunCandidate {
    /// Capture before the first builder mutation that a later avoid-break
    /// retry must undo.
    pub(in crate::layout) fn arm(self, builder: &LayoutBuilder<'_>) -> AvoidBreakRunCandidate {
        AvoidBreakRunCandidate {
            snapshot: Box::new(builder.snapshot()),
            meta: self.meta,
        }
    }
}

impl AvoidBreakRunCandidate {
    pub(in crate::layout) fn height(&self) -> f32 {
        self.meta.height
    }

    pub(in crate::layout) fn add_height(mut self, height: f32) -> Self {
        self.meta.height += height;
        self
    }

    pub(in crate::layout) fn restore(
        self,
        builder: &mut LayoutBuilder<'_>,
    ) -> AvoidBreakRunCandidateMeta {
        builder.restore(*self.snapshot);
        self.meta
    }
}

pub(in crate::layout) struct AdjoiningFloatReplayCandidateMeta {
    pub(in crate::layout) index: usize,
    pub(in crate::layout) element_index: usize,
    pub(in crate::layout) previous_flow_bottom_margin: Option<f32>,
    pub(in crate::layout) seen_flow_child: bool,
    pub(in crate::layout) trim_block_start_adjoining_margins: bool,
    pub(in crate::layout) collapsed_end_margin: bool,
    pub(in crate::layout) previous_child_page_end: Option<Option<String>>,
    pub(in crate::layout) float_run: FloatRunState,
    pub(in crate::layout) previous_break_after: PageBreak,
}

pub(in crate::layout) struct PendingAdjoiningFloatReplayCandidate {
    pub(in crate::layout) meta: AdjoiningFloatReplayCandidateMeta,
}

pub(in crate::layout) struct AdjoiningFloatReplayCandidate {
    snapshot: Box<LayoutSnapshot>,
    pub(in crate::layout) meta: AdjoiningFloatReplayCandidateMeta,
}

impl PendingAdjoiningFloatReplayCandidate {
    /// Capture before the self-collapsing child layout whose adjoining floats
    /// may need to be replayed at a later collapsed-margin origin.
    pub(in crate::layout) fn arm(
        self,
        builder: &LayoutBuilder<'_>,
    ) -> AdjoiningFloatReplayCandidate {
        AdjoiningFloatReplayCandidate {
            snapshot: Box::new(builder.snapshot()),
            meta: self.meta,
        }
    }
}

impl AdjoiningFloatReplayCandidate {
    pub(in crate::layout) fn snapshot(&self) -> &LayoutSnapshot {
        &self.snapshot
    }

    pub(in crate::layout) fn snapshot_cursor_y(&self) -> f32 {
        self.snapshot.cursor_y
    }

    pub(in crate::layout) fn restore(
        self,
        builder: &mut LayoutBuilder<'_>,
    ) -> AdjoiningFloatReplayCandidateMeta {
        builder.restore(*self.snapshot);
        self.meta
    }
}

pub(in crate::layout) fn should_move_avoid_break_run_to_next_fragmentainer(
    run_height: f32,
    next_height: f32,
    current_fragmentainer: Fragmentainer,
    at_page_top: bool,
) -> bool {
    FragmentPrebreakDecision::choose(FragmentPrebreakInput {
        can_advance: !at_page_top,
        current_fragmentainer,
        required_block_size: next_height,
        empty_fragmentainer: current_fragmentainer,
        empty_fit_block_size: run_height + next_height,
    })
    .should_break
}

/// Returns whether a definite-height normal-flow block should start a new page.
///
/// CSS Fragmentation allows breaks between sibling block boxes. When a block's
/// used border-box height is definite and it fits in an empty page area but not
/// in the remaining fragmentainer space, laying it out after a class A break
/// keeps its own background, border, and descendants in the next page
/// coordinate space:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks> and
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
pub(in crate::layout) fn should_prebreak_definite_block(
    context: DefiniteBlockBreakContext<'_>,
) -> bool {
    let Some(content_height) = context.definite_content_height else {
        return false;
    };
    let block_height = context.style.margin.top
        + context.vertical_extras
        + content_height.max(0.0)
        + context.style.margin.bottom;
    // A box at the start of a short fragmentainer normally stays there even
    // when oversized, because retrying an equivalent empty fragmentainer
    // cannot improve placement. Nested fragmentation can instead make the
    // next outer row strictly taller. Advance in that case when the box fits
    // the larger destination; this is both forward progress and the legal
    // class-A break that avoids slicing a monolithic box.
    // <https://www.w3.org/TR/css-break-3/#breaking-rules>
    let improves_empty_destination = context
        .empty_destination_fragmentainer
        .fragmentainer_block_size()
        > context.current_fragmentainer.fragmentainer_block_size() + 0.01
        && context
            .empty_destination_fragmentainer
            .block_size_fits_empty(block_height);
    if (!context.fragmentainer_has_occupied_flow || context.at_page_top)
        && !improves_empty_destination
    {
        return false;
    }
    if context.suppress_for_avoid_retry
        && context
            .current_fragmentainer
            .block_size_fits_empty(block_height)
    {
        return false;
    }
    FragmentPrebreakDecision::choose(FragmentPrebreakInput {
        can_advance: true,
        current_fragmentainer: context.current_fragmentainer,
        required_block_size: block_height,
        empty_fragmentainer: context.empty_destination_fragmentainer,
        empty_fit_block_size: block_height,
    })
    .should_break
}
