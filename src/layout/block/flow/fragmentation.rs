use super::*;

/// Inputs for deciding whether a definite-height block should prebreak.
///
/// CSS Fragmentation allows class A breaks between sibling block boxes before
/// layout. Keeping those inputs together makes the decision explicit while
/// allowing avoid-retry pagination state to tailor the rule:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
pub(in crate::layout) struct DefiniteBlockBreakContext<'a> {
    pub(in crate::layout) definite_content_height: Option<f32>,
    pub(in crate::layout) vertical_non_content: NonContentLength,
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
    pub(in crate::layout) remaining_line_clamp: Option<css::RemainingLineSlots>,
    pub(in crate::layout) height: f32,
}

pub(in crate::layout) struct PendingAvoidBreakRunCandidate {
    pub(in crate::layout) meta: AvoidBreakRunCandidateMeta,
}

pub(in crate::layout) struct AvoidBreakRunCandidate {
    snapshot: Box<LayoutSnapshot>,
    pub(in crate::layout) meta: AvoidBreakRunCandidateMeta,
}

/// Whether the source boundary of an avoid-run retry belongs to an occupied
/// fragmentainer or starts a fresh one.
///
/// An empty source may advance only when the next fragmentainer has strictly
/// more usable block capacity. Keeping this distinct from a page-top cursor
/// prevents temporary multicolumn pages from being treated as ordinary page
/// continuations.
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum AvoidRunSourceFragmentainerOccupancy {
    Empty,
    Occupied,
}

/// Geometry and source occupancy needed to retry an avoided sibling run.
///
/// CSS Fragmentation chooses the destination before moving a source run. The
/// record keeps the current remaining capacity separate from the next empty
/// capacity, which can differ for the first short anonymous column in a
/// nested multicolumn context.
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
/// <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AvoidRunRetryContext {
    pub(in crate::layout) current_fragmentainer: Fragmentainer,
    pub(in crate::layout) empty_destination_fragmentainer: Fragmentainer,
    pub(in crate::layout) source_occupancy: AvoidRunSourceFragmentainerOccupancy,
}

impl AvoidRunRetryContext {
    /// Whether an unforced retry can make progress before source sizing is
    /// considered.
    pub(in crate::layout) fn can_advance(self) -> bool {
        match self.source_occupancy {
            AvoidRunSourceFragmentainerOccupancy::Occupied => true,
            AvoidRunSourceFragmentainerOccupancy::Empty => {
                self.empty_destination_fragmentainer
                    .fragmentainer_block_size()
                    .points()
                    > self
                        .current_fragmentainer
                        .fragmentainer_block_size()
                        .points()
                        + 0.01
            }
        }
    }
}

/// Inputs for moving an avoid-constrained sibling run before the next child.
///
/// Grouping the source and destination fragmentainers with the run sizes
/// avoids accidentally comparing the next child against current remaining
/// space or using a page-only empty-state flag for a column continuation.
/// <https://www.w3.org/TR/css-break-3/#break-between>
pub(in crate::layout) struct AvoidRunPrebreakInput {
    pub(in crate::layout) run_height: f32,
    pub(in crate::layout) next_height: f32,
    pub(in crate::layout) retry_context: AvoidRunRetryContext,
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

    /// Return the occupancy of the fragmentainer at the saved source boundary.
    ///
    /// This is stronger than a cursor-at-top comparison because leading
    /// margins may have collapsed before the first in-flow box is committed.
    /// The snapshot can represent an anonymous multicolumn fragmentainer as
    /// well as an ordinary page.
    /// <https://www.w3.org/TR/css-break-3/#breaking-rules>
    pub(in crate::layout) fn source_fragmentainer_occupancy(
        &self,
    ) -> AvoidRunSourceFragmentainerOccupancy {
        if self.snapshot.current_page_has_flow_content {
            AvoidRunSourceFragmentainerOccupancy::Occupied
        } else {
            AvoidRunSourceFragmentainerOccupancy::Empty
        }
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
    input: AvoidRunPrebreakInput,
) -> bool {
    FragmentPrebreakDecision::choose(FragmentPrebreakInput {
        can_advance: input.retry_context.can_advance(),
        current_fragmentainer: input.retry_context.current_fragmentainer,
        required_block_size: layout_pt(input.next_height),
        empty_fragmentainer: input.retry_context.empty_destination_fragmentainer,
        empty_fit_block_size: layout_pt(input.run_height + input.next_height),
    })
    .should_break
}

impl LayoutBuilder<'_> {
    /// Return the empty destination selected by the next unforced break.
    ///
    /// The initial anonymous multicolumn fragmentainer can be shorter than
    /// its continuations. Resolve the next override context here instead of
    /// reusing current remaining capacity at individual avoid-run call sites.
    /// <https://www.w3.org/TR/css-break-3/#breaking-rules>
    /// <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
    pub(in crate::layout) fn next_empty_fragmentainer(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
    ) -> Fragmentainer {
        let capacity = match fragmentainer_kind {
            FragmentainerKind::Page => self
                .resolved_page_context(self.pages.len() + 2, false)
                .area_height(),
            FragmentainerKind::Column => self
                .fragmentainer_override
                .map(|override_| {
                    override_
                        .context_for_fragmentainer(self.pages.len() + 1)
                        .area_height()
                })
                .unwrap_or_else(|| self.page_area_height()),
        };
        Fragmentainer::new(layout_pt(capacity), layout_pt(capacity))
    }
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
        + context.vertical_non_content.points()
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
        .points()
        > context
            .current_fragmentainer
            .fragmentainer_block_size()
            .points()
            + 0.01
        && context
            .empty_destination_fragmentainer
            .block_size_fits_empty(layout_pt(block_height));
    if (!context.fragmentainer_has_occupied_flow || context.at_page_top)
        && !improves_empty_destination
    {
        return false;
    }
    if context.suppress_for_avoid_retry
        && context
            .current_fragmentainer
            .block_size_fits_empty(layout_pt(block_height))
    {
        return false;
    }
    FragmentPrebreakDecision::choose(FragmentPrebreakInput {
        can_advance: true,
        current_fragmentainer: context.current_fragmentainer,
        required_block_size: layout_pt(block_height),
        empty_fragmentainer: context.empty_destination_fragmentainer,
        empty_fit_block_size: layout_pt(block_height),
    })
    .should_break
}
