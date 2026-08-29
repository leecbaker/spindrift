//! Table pagination break selection and repeat policy.

use std::rc::Rc;

use super::*;
#[derive(Clone)]
pub(in crate::layout::table) struct TableBreakCandidateMeta {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) table_body_fragment: Option<TableBodyPaintFragment>,
    pub(in crate::layout::table) wrapper_timeline_checkpoint:
        Option<TableWrapperTimelineCheckpoint>,
    pub(in crate::layout::table) repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) height: f32,
}

pub(in crate::layout::table) struct PendingTableBreakCandidate {
    pub(in crate::layout::table) meta: TableBreakCandidateMeta,
}

#[derive(Clone)]
pub(in crate::layout::table) struct TableBreakCandidate {
    snapshot: Rc<LayoutSnapshot>,
    pub(in crate::layout::table) meta: TableBreakCandidateMeta,
}

/// Rolling break-candidate state for row-level avoid constraints.
///
/// CSS Fragmentation treats `break-before: avoid` and `break-after: avoid` as
/// constraints on class A break opportunities. Table pagination captures
/// rollback candidates at row starts and updates this state after each source
/// row is consumed so a later overflow can restore the chosen row boundary:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
pub(in crate::layout::table) struct TableAvoidBreakCandidateState {
    fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) avoid_break_candidate: Option<TableBreakCandidate>,
    pub(in crate::layout::table) previous_row_candidate: Option<TableBreakCandidate>,
    pub(in crate::layout::table) previous_break_after: PageBreak,
}

/// Table-local spelling for the shared adjacent-box break context.
pub(in crate::layout::table) type TableRowBreakContext = FragmentBreakContext;

/// Table-local spelling for shared cross-sibling forced break carry state.
pub(in crate::layout::table) type TableForcedBreakCarryState = ForcedBreakCarryState;

/// Committed decision to roll an avoid-constrained run back to an earlier row.
///
/// CSS Fragmentation treats `break-before: avoid` and `break-after: avoid` as
/// constraints between adjacent boxes. Table pagination records row-start
/// rollback candidates before painting, then commits a rollback only when the
/// measured avoid run fits in the next fragmentainer:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Clone)]
pub(in crate::layout::table) struct TableAvoidRunBreakDecision {
    pub(in crate::layout::table) candidate: TableBreakCandidate,
    pub(in crate::layout::table) avoid_run_height: f32,
    pub(in crate::layout::table) incoming_repeat_policy: TableFragmentRepeatPolicy,
}

pub(in crate::layout::table) struct TableAvoidRunBreakInput {
    pub(in crate::layout::table) candidate: TableBreakCandidate,
    pub(in crate::layout::table) row_height: f32,
    pub(in crate::layout::table) current_fragmentainer: TableFragmentainer,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) can_advance: bool,
}

/// Committed overflow break before a table body row fragment.
///
/// CSS Fragmentation places content into a finite fragmentainer and chooses a
/// break when the next row would overflow the available block-size. Table
/// pagination records the measured row height, current fragmentainer state, and
/// incoming repeated table chrome policy before advancing to the next fragment:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowOverflowBreakDecision {
    pub(in crate::layout::table) row_height: f32,
    pub(in crate::layout::table) incoming_repeat_policy: TableFragmentRepeatPolicy,
}

pub(in crate::layout::table) struct TableRowOverflowBreakInput {
    pub(in crate::layout::table) row_height: f32,
    pub(in crate::layout::table) row_required_height: f32,
    pub(in crate::layout::table) current_fragmentainer: TableFragmentainer,
    pub(in crate::layout::table) row_kept_by_avoid_group: bool,
    /// An oversized row with an authored row-level avoid still prefers its
    /// first child fragment to begin at the next class-A boundary.
    pub(in crate::layout::table) prefer_fresh_fragment: bool,
    pub(in crate::layout::table) can_break: bool,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
}

/// Fragment-local decision for the next slice of an oversized table row.
///
/// CSS Fragmentation may split an oversized row across fragmentainers. The
/// table body chooses the current piece height from the remaining source row
/// height and the actual fragmentainer body capacity, including repeated
/// chrome and cloned table-wrapper decoration, before table-cell descendants
/// are replayed for that row slice. A zero-height pre-break is legal only
/// when the destination can consume the deferred cell child.
///
/// <https://drafts.csswg.org/css-tables/#table-fragmentation>
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
/// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableOversizedRowSliceDecision {
    pub(in crate::layout::table) kind: TableOversizedRowSliceDecisionKind,
    pub(in crate::layout::table) remaining_height: f32,
    pub(in crate::layout::table) available_body_size: f32,
    pub(in crate::layout::table) piece_height: f32,
    pub(in crate::layout::table) incoming_repeat_policy: TableFragmentRepeatPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableOversizedRowSliceDecisionKind {
    AdvanceBeforeSlice,
    PaintSlice,
    /// The row cannot be divided at a legal cell-child boundary and the next
    /// fragmentainer has no more body capacity.  Commit it at this
    /// fragmentainer rather than repeatedly advancing through equivalent
    /// fragmentainers.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    PaintUnfragmentedOverflow,
}

pub(in crate::layout::table) struct TableOversizedRowSliceInput {
    pub(in crate::layout::table) remaining_height: f32,
    pub(in crate::layout::table) row_required_height: f32,
    pub(in crate::layout::table) current_fragmentainer: TableFragmentainer,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) can_advance: bool,
}

/// Committed action at the boundary between two table body fragments.
///
/// CSS Fragmentation chooses page-fragment boundaries before the next
/// fragmentainer is laid out. For tables, that same boundary also decides
/// whether optional repeated footer chrome is part of the outgoing fragment:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentBoundaryDecision {
    pub(in crate::layout::table) repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) footer_action: TableFragmentFooterAction,
}

impl TableFragmentBoundaryDecision {
    pub(in crate::layout::table) fn new(
        repeat_policy: TableFragmentRepeatPolicy,
        footer_action: TableFragmentFooterAction,
    ) -> Self {
        Self {
            repeat_policy,
            footer_action,
        }
    }
}

/// Repeated-footer handling committed at a table body fragment boundary.
///
/// Intermediate page boundaries replay repeated footer chrome after the body
/// fragment is finalized. The final table fragment only records repeated
/// footer rows in the fragment plan so structural backgrounds and border
/// painting can account for footer rows already present in source order:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableFragmentFooterAction {
    Omit,
    RecordOnly,
    PaintRepeated,
}

impl TableFragmentFooterAction {
    pub(in crate::layout::table) fn paint_repeated_if(condition: bool) -> Self {
        if condition {
            Self::PaintRepeated
        } else {
            Self::Omit
        }
    }

    pub(in crate::layout::table) fn record_repeated_rows(self) -> bool {
        matches!(self, Self::RecordOnly | Self::PaintRepeated)
    }

    pub(in crate::layout::table) fn paint_repeated_chrome(self) -> bool {
        self == Self::PaintRepeated
    }
}

/// Committed action at the start of a table body fragment.
///
/// CSS Fragmentation creates a new fragmentainer slice with a known break
/// reason before the first body row is painted. For tables, the same start
/// decision owns whether optional repeated header chrome participates in that
/// new fragment:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentStartDecision {
    pub(in crate::layout::table) break_reason: TableFragmentBreakReason,
    pub(in crate::layout::table) repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) paint_repeated_header: bool,
}

impl TableFragmentStartDecision {
    pub(in crate::layout::table) fn new(
        break_reason: TableFragmentBreakReason,
        repeat_policy: TableFragmentRepeatPolicy,
        paint_repeated_header: bool,
    ) -> Self {
        Self {
            break_reason,
            repeat_policy,
            paint_repeated_header,
        }
    }

    pub(in crate::layout::table) fn repeated_header_rows<'a>(
        &self,
        rows: &'a [usize],
    ) -> &'a [usize] {
        if self.paint_repeated_header {
            self.repeat_policy.header_rows(rows)
        } else {
            &[]
        }
    }
}

/// Committed transition between two table body fragments.
///
/// CSS Fragmentation treats a break as an outgoing fragment boundary plus an
/// incoming fragmentainer start. Keeping both halves together lets table
/// pagination reserve footer chrome, carry the active fragmentainer kind, and
/// replay header chrome as one committed table-local transition:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentTransitionDecision {
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) boundary: TableFragmentBoundaryDecision,
    pub(in crate::layout::table) start: TableFragmentStartDecision,
}

/// Inputs used to commit one table body fragment transition.
///
/// CSS Fragmentation makes a fragmentainer transition as an outgoing fragment
/// boundary followed by an incoming fragmentainer start. Table pagination has
/// to bind that model to optional repeated table header/footer chrome, so
/// callers pass both repeat policies and the chrome actions as one value
/// instead of assembling boundary and start decisions independently:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentTransitionInput {
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) outgoing_repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) footer_action: TableFragmentFooterAction,
    pub(in crate::layout::table) break_reason: TableFragmentBreakReason,
    pub(in crate::layout::table) incoming_repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) paint_repeated_header: bool,
}

impl TableFragmentTransitionDecision {
    pub(in crate::layout::table) fn new(
        fragmentainer_kind: FragmentainerKind,
        boundary: TableFragmentBoundaryDecision,
        start: TableFragmentStartDecision,
    ) -> Self {
        Self {
            fragmentainer_kind,
            boundary,
            start,
        }
    }

    pub(in crate::layout::table) fn from_input(input: TableFragmentTransitionInput) -> Self {
        Self::new(
            input.fragmentainer_kind,
            TableFragmentBoundaryDecision::new(input.outgoing_repeat_policy, input.footer_action),
            TableFragmentStartDecision::new(
                input.break_reason,
                input.incoming_repeat_policy,
                input.paint_repeated_header,
            ),
        )
    }
}

/// Committed forced break before a table body row fragment.
///
/// Forced breaks are class A break opportunities in CSS Fragmentation. The
/// table body must commit the outgoing fragment boundary before applying the
/// forced page change, then carry a committed start decision for the incoming
/// fragment's repeated table chrome:
/// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableForcedBreakDecision {
    pub(in crate::layout::table) boundary: TableFragmentBoundaryDecision,
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) page_break: PageBreak,
    pub(in crate::layout::table) start: TableFragmentStartDecision,
}

/// Inputs for choosing a table body forced break decision.
///
/// CSS Fragmentation decides the forced break first, while CSS 2.2 table
/// header/footer repetition determines the usable body capacity on the
/// incoming fragment. Keeping these inputs together prevents forced break
/// branches from recomputing table chrome policy independently:
/// <https://www.w3.org/TR/css-break-3/#break-between> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableForcedBreakInput {
    pub(in crate::layout::table) outgoing_repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) page_break: PageBreak,
    pub(in crate::layout::table) row_required_height: f32,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) paint_repeated_footer: bool,
}

impl TableForcedBreakDecision {
    pub(in crate::layout::table) fn choose(input: TableForcedBreakInput) -> Self {
        let incoming_repeat_policy = input
            .chrome_context
            .repeat_policy(layout_pt(input.row_required_height));
        Self {
            boundary: TableFragmentBoundaryDecision::new(
                input.outgoing_repeat_policy,
                TableFragmentFooterAction::paint_repeated_if(input.paint_repeated_footer),
            ),
            fragmentainer_kind: input.fragmentainer_kind,
            page_break: input.page_break,
            start: TableFragmentStartDecision::new(
                TableFragmentBreakReason::Forced,
                incoming_repeat_policy,
                input.chrome_context.allow_header,
            ),
        }
    }
}

/// Committed named-page group transition before a table body row fragment.
///
/// CSS Paged Media forms named page groups at class A break opportunities.
/// Table body pagination treats the named-page switch as an outgoing table
/// fragment boundary plus an incoming fragment start so repeated table chrome
/// stays tied to the same committed named-page transition:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableNamedPageBreakDecision {
    pub(in crate::layout::table) boundary: TableFragmentBoundaryDecision,
    pub(in crate::layout::table) page_name: Option<String>,
    pub(in crate::layout::table) start: TableFragmentStartDecision,
}

/// Inputs for choosing a table body named-page transition.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableNamedPageBreakInput {
    pub(in crate::layout::table) previous_page_end: Option<String>,
    pub(in crate::layout::table) row_page_start: Option<String>,
    pub(in crate::layout::table) outgoing_repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) row_required_height: f32,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) paint_repeated_footer: bool,
}

impl TableNamedPageBreakDecision {
    pub(in crate::layout::table) fn choose(input: TableNamedPageBreakInput) -> Option<Self> {
        if input.previous_page_end == input.row_page_start {
            return None;
        }

        let incoming_repeat_policy = input
            .chrome_context
            .repeat_policy(layout_pt(input.row_required_height));
        Some(Self {
            boundary: TableFragmentBoundaryDecision::new(
                input.outgoing_repeat_policy,
                TableFragmentFooterAction::paint_repeated_if(input.paint_repeated_footer),
            ),
            page_name: input.row_page_start,
            start: TableFragmentStartDecision::new(
                TableFragmentBreakReason::Forced,
                incoming_repeat_policy,
                input.chrome_context.allow_header,
            ),
        })
    }
}

impl PendingTableBreakCandidate {
    /// Capture before the first row layout mutation that a later table
    /// avoid-break retry must undo.
    pub(in crate::layout::table) fn arm(self, builder: &LayoutBuilder<'_>) -> TableBreakCandidate {
        TableBreakCandidate {
            snapshot: Rc::new(builder.snapshot()),
            meta: self.meta,
        }
    }
}

impl TableBreakCandidate {
    pub(in crate::layout::table) fn height(&self) -> f32 {
        self.meta.height
    }

    pub(in crate::layout::table) fn with_height(mut self, height: f32) -> Self {
        self.meta.height = height;
        self
    }

    pub(in crate::layout::table) fn restore(
        self,
        builder: &mut LayoutBuilder<'_>,
    ) -> TableBreakCandidateMeta {
        let snapshot = Rc::try_unwrap(self.snapshot).unwrap_or_else(|snapshot| (*snapshot).clone());
        builder.restore(snapshot);
        self.meta
    }
}

impl TableAvoidBreakCandidateState {
    pub(in crate::layout::table) fn new(fragmentainer_kind: FragmentainerKind) -> Self {
        Self {
            fragmentainer_kind,
            avoid_break_candidate: None,
            previous_row_candidate: None,
            previous_break_after: PageBreak::Auto,
        }
    }

    pub(in crate::layout::table) fn row_start_may_be_rollback_target(
        &self,
        row_collapsed: bool,
        row_is_running: bool,
        row_breaks: TableRowBreakContext,
    ) -> bool {
        // A current row's `break-before: avoid` protects the boundary before
        // that row, so overflow should roll back to the previous row candidate
        // rather than arming the current row start as a new target.
        let row_start_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            if row_collapsed || row_is_running {
                PageBreak::Auto
            } else {
                row_breaks.after
            },
            row_breaks.next_before,
        );
        FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            participates_in_flow: true,
            fragmentainer_kind: self.fragmentainer_kind,
            break_context: row_start_breaks,
            break_opportunity: FragmentBreakOpportunity::before_box_boundary(
                self.fragmentainer_kind,
                0.0,
                row_start_breaks,
                self.previous_break_after,
                false,
            ),
            next_break_before: Some(row_breaks.next_before),
            has_avoid_run_candidate: self.avoid_break_candidate.is_some(),
        })
        .should_arm_start_candidate
    }

    pub(in crate::layout::table) fn boundary_candidate(
        &self,
        row_breaks: TableRowBreakContext,
    ) -> Option<TableBreakCandidate> {
        match row_breaks
            .avoid_boundary_side_before_box_in(self.fragmentainer_kind, self.previous_break_after)
        {
            FragmentAvoidBoundarySide::Previous => self.avoid_break_candidate.clone(),
            FragmentAvoidBoundarySide::Current => self.previous_row_candidate.clone(),
            FragmentAvoidBoundarySide::None => None,
        }
    }

    pub(in crate::layout::table) fn reset(&mut self) {
        self.avoid_break_candidate = None;
        self.previous_row_candidate = None;
        self.previous_break_after = PageBreak::Auto;
    }

    pub(in crate::layout::table) fn finish_non_content_row(
        &mut self,
        row_breaks: TableRowBreakContext,
        row_start_candidate: Option<TableBreakCandidate>,
    ) {
        self.previous_row_candidate = row_breaks
            .next_avoid_before_in(self.fragmentainer_kind)
            .is_some()
            .then(|| Self::expect_row_start_candidate(row_start_candidate).with_height(0.0));
        self.avoid_break_candidate = None;
        self.previous_break_after = PageBreak::Auto;
    }

    pub(in crate::layout::table) fn finish_content_row(
        &mut self,
        row_breaks: TableRowBreakContext,
        row_start_candidate: Option<TableBreakCandidate>,
        row_height: f32,
    ) {
        let row_candidate = if self.previous_break_after_avoids() {
            let this = self
                .avoid_break_candidate
                .clone()
                .unwrap_or_else(|| Self::expect_row_start_candidate(row_start_candidate.clone()));
            let height = self
                .avoid_break_candidate
                .as_ref()
                .map(TableBreakCandidate::height)
                .unwrap_or(0.0)
                + row_height;
            Some(this.with_height(height))
        } else if row_breaks.seeds_later_avoid_boundary_in_context_for(self.fragmentainer_kind) {
            Some(Self::expect_row_start_candidate(row_start_candidate).with_height(row_height))
        } else {
            None
        };
        self.previous_row_candidate = row_breaks
            .next_avoid_before_in(self.fragmentainer_kind)
            .is_some()
            .then(|| {
                row_candidate
                    .clone()
                    .expect("table break candidate must exist for next row break-before: avoid")
            });
        let avoid_after = row_breaks.avoid_after_in(self.fragmentainer_kind);
        self.avoid_break_candidate = if avoid_after.is_some() {
            Some(row_candidate.expect("table break candidate must exist for break-after: avoid"))
        } else {
            None
        };
        self.previous_break_after = avoid_after.unwrap_or(PageBreak::Auto);
    }

    fn expect_row_start_candidate(candidate: Option<TableBreakCandidate>) -> TableBreakCandidate {
        candidate.expect(
            "row start candidate must be armed when this row can become a table break candidate",
        )
    }

    fn previous_break_after_avoids(&self) -> bool {
        self.fragmentainer_kind
            .is_avoid_break(self.previous_break_after)
    }
}

impl Default for TableAvoidBreakCandidateState {
    fn default() -> Self {
        Self::new(FragmentainerKind::Page)
    }
}

impl TableAvoidRunBreakDecision {
    pub(in crate::layout::table) fn choose(input: TableAvoidRunBreakInput) -> Option<Self> {
        let avoid_run_height = input.candidate.height() + input.row_height;
        let incoming_repeat_policy = input
            .chrome_context
            .repeat_policy(layout_pt(avoid_run_height));
        let next_fragmentainer = input
            .chrome_context
            .fresh_fragmentainer(incoming_repeat_policy);
        FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: input.can_advance,
            current_fragmentainer: input.current_fragmentainer.as_fragmentainer(),
            required_block_size: layout_pt(input.row_height),
            empty_fragmentainer: next_fragmentainer.body_capacity_fragmentainer(),
            empty_fit_block_size: layout_pt(avoid_run_height),
        })
        .should_break
        .then_some(Self {
            candidate: input.candidate,
            avoid_run_height,
            incoming_repeat_policy,
        })
    }
}

impl TableRowOverflowBreakDecision {
    pub(in crate::layout::table) fn choose(input: TableRowOverflowBreakInput) -> Option<Self> {
        // A table body can be fragmented by a column whose usable body area is
        // smaller than the backing page canvas. Compare with the table-local
        // body capacity, not the physical page height, or a row larger than a
        // short column is repeatedly moved to another equally short column
        // without ever becoming eligible for row slicing.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        let row_requires_split =
            input.row_height > input.current_fragmentainer.body_capacity.points() + 0.01;
        // `row_required_height` includes any non-row table edge that must be
        // emitted immediately after this row. The row itself remains the
        // paint/slicing unit, but the fragmentation fit check must reserve the
        // complete trailing contribution.
        let row_overflows_page = if row_requires_split {
            input.prefer_fresh_fragment
                || !input.row_kept_by_avoid_group
                    && input.current_fragmentainer.available_block_size().points() <= 0.01
        } else {
            input.row_required_height > input.current_fragmentainer.available_block_size().points()
        };
        let row_overflows_reserved_footer = if row_requires_split {
            !input.row_kept_by_avoid_group
                && input.current_fragmentainer.available_body_size().points() <= 0.01
        } else {
            input.row_required_height + input.current_fragmentainer.reserved_footer_height.points()
                > input.current_fragmentainer.available_block_size().points()
        };
        let should_advance = FragmentAdvanceDecision::choose(FragmentAdvanceInput {
            break_is_applicable: true,
            overflows: row_overflows_page || row_overflows_reserved_footer,
            can_advance: input.can_break,
        })
        .should_advance;
        if !should_advance {
            return None;
        }

        Some(Self {
            row_height: input.row_height,
            incoming_repeat_policy: input
                .chrome_context
                .repeat_policy(layout_pt(input.row_required_height)),
        })
    }
}

impl TableOversizedRowSliceDecision {
    pub(in crate::layout::table) fn choose(input: TableOversizedRowSliceInput) -> Self {
        let raw_available_body_size = input
            .current_fragmentainer
            .available_body_size()
            .points()
            .min(input.current_fragmentainer.body_capacity.points());
        let available_body_size = raw_available_body_size;
        let incoming_repeat_policy = input
            .chrome_context
            .repeat_policy(layout_pt(input.row_required_height));
        if available_body_size > 0.01 && input.remaining_height > available_body_size + 0.01 {
            return Self {
                kind: TableOversizedRowSliceDecisionKind::PaintSlice,
                remaining_height: input.remaining_height,
                available_body_size,
                piece_height: available_body_size,
                incoming_repeat_policy,
            };
        }
        let source_slice = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
            break_is_applicable: input.can_advance,
            source_is_oversized: true,
            source_block_end: input.remaining_height,
            slice_start: 0.0,
            available_block_end: available_body_size,
        });
        if !source_slice.paints_slice() {
            return Self {
                kind: TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice,
                remaining_height: input.remaining_height,
                available_body_size,
                piece_height: 0.0,
                incoming_repeat_policy,
            };
        }

        Self {
            kind: TableOversizedRowSliceDecisionKind::PaintSlice,
            remaining_height: input.remaining_height,
            available_body_size,
            piece_height: source_slice.slice_end,
            incoming_repeat_policy,
        }
    }

    pub(in crate::layout::table) fn paints_slice(self) -> bool {
        matches!(
            self.kind,
            TableOversizedRowSliceDecisionKind::PaintSlice
                | TableOversizedRowSliceDecisionKind::PaintUnfragmentedOverflow
        )
    }

    pub(in crate::layout::table) fn continues_after_slice(self) -> bool {
        self.remaining_height - self.piece_height > 0.01
    }

    /// Restrict a height-based candidate to a legal shared table-cell child
    /// boundary. A zero-sized result may advance only after the caller has
    /// verified that the exact destination body capacity can paint the
    /// deferred child; otherwise it must consume a non-zero source slice.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    /// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
    pub(in crate::layout::table) fn at_child_boundary(mut self, piece_height: f32) -> Self {
        debug_assert!(piece_height >= 0.0);
        if !self.paints_slice() {
            return self;
        }
        self.piece_height = piece_height.min(self.piece_height).max(0.0);
        if self.piece_height <= 0.01 {
            self.kind = TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice;
        }
        self
    }

    /// Return whether an empty child-boundary pre-break would make no
    /// fragmentation progress.
    ///
    /// A table row may be taller than every available fragmentainer while its
    /// first cell child is atomic.  Retrying that child in another
    /// fragmentainer is only useful when the destination has strictly more
    /// usable table-body space; otherwise CSS fragmentation must accept the
    /// row's unfragmented overflow at its current start.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    pub(in crate::layout::table) fn needs_unfragmented_overflow(
        self,
        next_body_capacity: f32,
    ) -> bool {
        self.kind == TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice
            && next_body_capacity <= self.available_body_size + 0.01
    }

    /// Convert a zero-progress pre-break into one unfragmented row fragment.
    ///
    /// The caller restricts this to the first source piece of the row, so
    /// consuming all remaining height keeps the row's source and destination
    /// fragments identical rather than synthesizing an unanchored partial
    /// slice.
    pub(in crate::layout::table) fn as_unfragmented_overflow(
        mut self,
        next_body_capacity: f32,
    ) -> Self {
        debug_assert!(
            self.needs_unfragmented_overflow(next_body_capacity),
            "only a no-progress table pre-break may become unfragmented overflow"
        );
        self.kind = TableOversizedRowSliceDecisionKind::PaintUnfragmentedOverflow;
        self.piece_height = self.remaining_height;
        self
    }

    pub(in crate::layout::table) fn is_unfragmented_overflow(self) -> bool {
        self.kind == TableOversizedRowSliceDecisionKind::PaintUnfragmentedOverflow
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) struct TableFragmentRepeatPolicy {
    pub(in crate::layout::table) repeat_header: bool,
    pub(in crate::layout::table) repeat_footer: bool,
}

pub(in crate::layout::table) const TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE: f32 = 2.0;

/// Table row-group range with a `break-inside: avoid-*` constraint.
///
/// CSS Fragmentation treats row groups as fragmentation containers. Keeping
/// the constrained source range explicit lets table pagination choose a group
/// fragment before painting rows:
/// <https://www.w3.org/TR/css-break-3/#break-within>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) struct TableAvoidRowGroup {
    pub(in crate::layout::table) start: usize,
    pub(in crate::layout::table) end: usize,
}

/// Complete block-axis space an avoided row group consumes in one table
/// fragment.
///
/// A row group's grid tracks are not the same as its fragmentainer footprint:
/// in the separated-border model the destination fragment also owns the
/// spacing on both sides of the participating range. Keeping that distinction
/// explicit prevents a keep-together decision from accepting a group which
/// the eventual row placement cannot fit.
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
/// <https://www.w3.org/TR/css-break-3/#break-within>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableRowGroupFragmentRequirement {
    row_grid: LayoutLength,
    leading_edge_spacing: LayoutLength,
    trailing_edge_spacing: LayoutLength,
}

impl TableRowGroupFragmentRequirement {
    pub(in crate::layout::table) fn from_row_group(
        group: TableAvoidRowGroup,
        row_heights: &[f32],
        row_occupancy: &[bool],
        table_metrics: TableMetrics,
    ) -> Self {
        let row_grid = layout_pt(table_row_span_height(
            row_heights,
            row_occupancy,
            group.start,
            group.row_span(),
            table_metrics.clone(),
        ));
        let group_end = group.end.min(row_occupancy.len());
        let group_has_occupied_row = row_occupancy
            .get(group.start..group_end)
            .is_some_and(|rows| rows.iter().any(|occupied| *occupied));
        let edge_spacing = if group_has_occupied_row {
            layout_pt(table_vertical_edge_spacing(row_occupancy, table_metrics))
        } else {
            layout_pt(0.0)
        };
        Self {
            row_grid,
            leading_edge_spacing: edge_spacing,
            trailing_edge_spacing: edge_spacing,
        }
    }

    pub(in crate::layout::table) fn block_size(self) -> LayoutLength {
        layout_pt(
            self.row_grid.points()
                + self.leading_edge_spacing.points()
                + self.trailing_edge_spacing.points(),
        )
    }
}

impl TableAvoidRowGroup {
    pub(in crate::layout::table) fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub(in crate::layout::table) fn row_span(self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

impl TableFragmentRepeatPolicy {
    pub(in crate::layout::table) fn header_rows<'a>(&self, rows: &'a [usize]) -> &'a [usize] {
        if self.repeat_header { rows } else { &[] }
    }

    pub(in crate::layout::table) fn footer_rows<'a>(&self, rows: &'a [usize]) -> &'a [usize] {
        if self.repeat_footer { rows } else { &[] }
    }

    pub(in crate::layout::table) fn reserved_footer_height(
        &self,
        footer_height: LayoutLength,
    ) -> LayoutLength {
        if self.repeat_footer {
            footer_height
        } else {
            layout_pt(0.0)
        }
    }

    pub(in crate::layout::table) fn body_capacity(
        &self,
        fragmentainer_block_size: LayoutLength,
        header_height: LayoutLength,
        footer_height: LayoutLength,
    ) -> LayoutLength {
        let repeated_height = if self.repeat_header {
            header_height
        } else {
            layout_pt(0.0)
        } + if self.repeat_footer {
            footer_height
        } else {
            layout_pt(0.0)
        };
        layout_pt((fragmentainer_block_size.points() - repeated_height.points()).max(0.0))
    }
}

/// Decoration owned by one table-wrapper fragment in the block direction.
///
/// The values retain their non-content-box meaning until they cross into the
/// generic fragmentainer adapter. Block-level margins are deliberately absent:
/// CSS Fragmentation truncates cloned margins for block-level boxes.
///
/// <https://drafts.csswg.org/css-tables/#table-fragmentation>
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
/// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableWrapperFragmentChrome {
    pub(in crate::layout::table) continuation_block_start: NonContentLength,
    pub(in crate::layout::table) continuation_block_end: NonContentLength,
}

impl TableWrapperFragmentChrome {
    #[cfg(test)]
    pub(in crate::layout::table) const fn none() -> Self {
        Self {
            continuation_block_start: non_content_pt(0.0),
            continuation_block_end: non_content_pt(0.0),
        }
    }

    /// Build the decoration consumed by every continuation fragment.
    ///
    /// `clone` independently wraps every box fragment with border and padding;
    /// `slice` does not insert them at an internal break. Separated-border edge
    /// spacing belongs to the source table grid, rather than to the cloned
    /// wrapper decoration, and is therefore handled by row placement.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    /// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
    pub(in crate::layout::table) fn for_table(
        style: &ComputedStyle,
        table_width: UsedTableWidth,
    ) -> Self {
        let cloned = style.box_decoration_break == css::BoxDecorationBreak::Clone;
        let start = if cloned {
            table_width.border_widths.top + table_width.padding.top
        } else {
            0.0
        };
        let end = if cloned {
            table_width.border_widths.bottom + table_width.padding.bottom
        } else {
            0.0
        };
        Self {
            continuation_block_start: non_content_pt(start),
            continuation_block_end: non_content_pt(end),
        }
    }

    pub(in crate::layout::table) fn continuation_block_start(self) -> NonContentLength {
        self.continuation_block_start
    }

    pub(in crate::layout::table) fn continuation_block_end(self) -> NonContentLength {
        self.continuation_block_end
    }

    /// Return the body area left after this wrapper fragment's decorations.
    ///
    /// CSS Fragmentation permits truncating cloned decoration before allowing a
    /// zero-progress break. This adapter first reserves both sides, then trims
    /// the cloned decoration to leave one paintable layout quantum whenever
    /// the fragmentainer itself has positive capacity.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    /// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
    pub(in crate::layout::table) fn fresh_body_capacity(
        self,
        capacity_before_wrapper_chrome: LayoutLength,
    ) -> LayoutLength {
        let chrome = self.truncated_for_capacity(capacity_before_wrapper_chrome);
        layout_pt(
            (capacity_before_wrapper_chrome.points()
                - chrome.continuation_block_start.points()
                - chrome.continuation_block_end.points())
            .max(0.0),
        )
    }

    /// Truncate cloned decoration only when it would otherwise leave no
    /// content slice in a positive-capacity fragmentainer.
    ///
    /// The retained lengths remain typed non-content-box quantities; scalar
    /// arithmetic is confined to this fragmentation-boundary adapter.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    /// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
    fn truncated_for_capacity(self, capacity: LayoutLength) -> Self {
        const MINIMUM_PAINTABLE_SLICE: f32 = 0.01;

        let decoration =
            self.continuation_block_start.points() + self.continuation_block_end.points();
        let available = capacity.points().max(0.0);
        if available <= MINIMUM_PAINTABLE_SLICE || decoration < available - MINIMUM_PAINTABLE_SLICE
        {
            return self;
        }
        let decoration_budget = (available - MINIMUM_PAINTABLE_SLICE).max(0.0);
        let continuation_block_start = self
            .continuation_block_start
            .points()
            .min(decoration_budget);
        let continuation_block_end = self
            .continuation_block_end
            .points()
            .min((decoration_budget - continuation_block_start).max(0.0));
        Self {
            continuation_block_start: non_content_pt(continuation_block_start),
            continuation_block_end: non_content_pt(continuation_block_end),
        }
    }
}

/// Table-local repeated chrome capacity context for a target fragmentainer.
///
/// CSS Fragmentation defines a finite fragmentainer block-size, while CSS 2.2
/// table header/footer groups may reserve repeated chrome around the table
/// body in paged output. Keeping those values together lets table break
/// decisions share the same capacity calculation without treating every
/// fragmentainer as a page cursor transition:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentChromeContext {
    pub(in crate::layout::table) fragmentainer_block_size: LayoutLength,
    pub(in crate::layout::table) header_height: LayoutLength,
    pub(in crate::layout::table) footer_height: LayoutLength,
    pub(in crate::layout::table) wrapper_chrome: TableWrapperFragmentChrome,
    pub(in crate::layout::table) allow_header: bool,
    pub(in crate::layout::table) allow_footer: bool,
}

impl TableFragmentChromeContext {
    pub(in crate::layout::table) fn repeat_policy(
        self,
        required_body_height: LayoutLength,
    ) -> TableFragmentRepeatPolicy {
        let body_fragmentainer_size = self
            .wrapper_chrome
            .fresh_body_capacity(self.fragmentainer_block_size);
        table_fragment_repeat_policy(
            required_body_height,
            body_fragmentainer_size,
            self.header_height,
            self.footer_height,
            self.allow_header,
            self.allow_footer,
        )
    }

    pub(in crate::layout::table) fn fresh_fragmentainer(
        self,
        repeat_policy: TableFragmentRepeatPolicy,
    ) -> TableFragmentainer {
        TableFragmentainer::fresh_with_wrapper_chrome(
            self.fragmentainer_block_size,
            repeat_policy,
            self.header_height,
            self.footer_height,
            self.wrapper_chrome,
        )
    }

    pub(in crate::layout::table) fn current_fragmentainer(
        self,
        content_block_start: PageTopBlockPosition,
        fragmentainer_block_end: PageTopBlockPosition,
        repeat_policy: TableFragmentRepeatPolicy,
        reserve_footer: bool,
    ) -> TableFragmentainer {
        TableFragmentainer::current_from_page_cursor_bounds(
            self.fragmentainer_block_size,
            content_block_start,
            fragmentainer_block_end,
            repeat_policy,
            self.header_height,
            self.footer_height,
            reserve_footer,
        )
        .with_wrapper_end_reservation(self.wrapper_chrome.continuation_block_end())
    }

    pub(in crate::layout::table) fn without_repeats(self) -> Self {
        Self {
            allow_header: false,
            allow_footer: false,
            ..self
        }
    }
}

/// Table-local view of a page fragmentainer while paginating body rows.
///
/// CSS Fragmentation lays boxes into fragmentainers with a finite block-size,
/// while repeated table header/footer groups reserve page-fragment chrome
/// around the table body. This value keeps the current remaining block-size,
/// optional repeated-footer reservation, and fresh-page body capacity together
/// so table break decisions consume one fragmentainer model instead of
/// repeating cursor arithmetic inline:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentainer {
    base: Fragmentainer,
    pub(in crate::layout::table) reserved_footer_height: LayoutLength,
    reserved_wrapper_end: LayoutLength,
    pub(in crate::layout::table) body_capacity: LayoutLength,
}

impl TableFragmentainer {
    fn with_base(
        base: Fragmentainer,
        fragmentainer_block_size: LayoutLength,
        repeat_policy: TableFragmentRepeatPolicy,
        header_height: LayoutLength,
        footer_height: LayoutLength,
        reserve_footer: bool,
    ) -> Self {
        let reserved_footer_height = if reserve_footer {
            repeat_policy.reserved_footer_height(footer_height)
        } else {
            layout_pt(0.0)
        };
        Self {
            base,
            reserved_footer_height,
            reserved_wrapper_end: layout_pt(0.0),
            body_capacity: repeat_policy.body_capacity(
                fragmentainer_block_size,
                header_height,
                footer_height,
            ),
        }
    }

    pub(in crate::layout::table) fn current_from_page_cursor_bounds(
        fragmentainer_block_size: LayoutLength,
        content_block_start: PageTopBlockPosition,
        fragmentainer_block_end: PageTopBlockPosition,
        repeat_policy: TableFragmentRepeatPolicy,
        header_height: LayoutLength,
        footer_height: LayoutLength,
        reserve_footer: bool,
    ) -> Self {
        Self::with_base(
            Fragmentainer::from_page_cursor_bounds(
                fragmentainer_block_size,
                content_block_start,
                fragmentainer_block_end,
            ),
            fragmentainer_block_size,
            repeat_policy,
            header_height,
            footer_height,
            reserve_footer,
        )
    }

    fn fresh_with_wrapper_chrome(
        fragmentainer_block_size: LayoutLength,
        repeat_policy: TableFragmentRepeatPolicy,
        header_height: LayoutLength,
        footer_height: LayoutLength,
        wrapper_chrome: TableWrapperFragmentChrome,
    ) -> Self {
        let body_capacity = wrapper_chrome.fresh_body_capacity(repeat_policy.body_capacity(
            fragmentainer_block_size,
            header_height,
            footer_height,
        ));
        Self {
            base: Fragmentainer::new(fragmentainer_block_size, body_capacity),
            reserved_footer_height: layout_pt(0.0),
            reserved_wrapper_end: layout_pt(0.0),
            body_capacity,
        }
    }

    fn with_wrapper_end_reservation(mut self, wrapper_end: NonContentLength) -> Self {
        self.reserved_wrapper_end = layout_pt(wrapper_end.points());
        self
    }

    #[cfg(test)]
    pub(in crate::layout::table) fn fragmentainer_block_size(&self) -> LayoutLength {
        self.base.fragmentainer_block_size()
    }

    pub(in crate::layout::table) fn available_block_size(&self) -> LayoutLength {
        self.base.available_block_size()
    }

    pub(in crate::layout::table) fn required_block_size_overflows(
        &self,
        block_size: LayoutLength,
    ) -> bool {
        self.base.required_block_size_overflows(block_size)
    }

    pub(in crate::layout::table) fn available_body_size(&self) -> LayoutLength {
        self.base.available_block_size_after_reservation(layout_pt(
            self.reserved_footer_height.points() + self.reserved_wrapper_end.points(),
        ))
    }

    pub(in crate::layout::table) fn as_fragmentainer(&self) -> Fragmentainer {
        self.base
    }

    pub(in crate::layout::table) fn body_capacity_fragmentainer(&self) -> Fragmentainer {
        Fragmentainer::new(self.body_capacity, self.body_capacity)
    }
}

/// How an avoided table row group is kept together on the next fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableRowGroupAvoidMode {
    FitsNextFragment,
    KeptByChromeOverflow,
}

/// Committed keep-together choice for one avoided table row group.
///
/// The decision captures the row-group source range, measured block size, the
/// repeated header/footer policy chosen for the destination fragment, and
/// whether optional table chrome had to be suppressed to make progress:
/// <https://www.w3.org/TR/css-break-3/#break-within>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowGroupAvoidDecision {
    pub(in crate::layout::table) group: TableAvoidRowGroup,
    pub(in crate::layout::table) required_block_size: LayoutLength,
    pub(in crate::layout::table) repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) mode: TableRowGroupAvoidMode,
}

/// Tracks source rows kept together after a row-group avoid decision.
///
/// When a row group is kept together by allowing bounded table-chrome overflow,
/// subsequent source rows in that group must consume the committed
/// `KeptByAvoidOverflow` row mode and must not trigger nested row splitting.
/// This state records the committed source range until pagination advances past
/// the group end:
/// <https://www.w3.org/TR/css-break-3/#break-within>.
#[derive(Debug, Default, Clone, Copy)]
pub(in crate::layout::table) struct TableAvoidRowGroupKeepState {
    end: Option<usize>,
}

pub(in crate::layout::table) struct TableRowGroupAvoidDecisionInput {
    pub(in crate::layout::table) group: TableAvoidRowGroup,
    pub(in crate::layout::table) required_block_size: LayoutLength,
    pub(in crate::layout::table) current_fragmentainer: TableFragmentainer,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) can_advance: bool,
}

impl TableRowGroupAvoidDecision {
    pub(in crate::layout::table) fn choose(input: TableRowGroupAvoidDecisionInput) -> Option<Self> {
        if !input.can_advance {
            return None;
        }

        if !input
            .current_fragmentainer
            .required_block_size_overflows(input.required_block_size)
        {
            return None;
        }

        let repeat_policy = input
            .chrome_context
            .repeat_policy(input.required_block_size);
        let repeat_fragmentainer = input.chrome_context.fresh_fragmentainer(repeat_policy);
        if FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: input.can_advance,
            current_fragmentainer: input.current_fragmentainer.as_fragmentainer(),
            required_block_size: input.required_block_size,
            empty_fragmentainer: repeat_fragmentainer.body_capacity_fragmentainer(),
            empty_fit_block_size: input.required_block_size,
        })
        .should_break
        {
            return Some(Self {
                group: input.group,
                required_block_size: input.required_block_size,
                repeat_policy,
                mode: TableRowGroupAvoidMode::FitsNextFragment,
            });
        }

        let no_repeat_policy = TableFragmentRepeatPolicy {
            repeat_header: false,
            repeat_footer: false,
        };
        let no_repeat_fragmentainer = input
            .chrome_context
            .without_repeats()
            .fresh_fragmentainer(no_repeat_policy);
        (input.required_block_size.points()
            <= no_repeat_fragmentainer.body_capacity.points()
                + TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE)
            .then_some(Self {
                group: input.group,
                required_block_size: input.required_block_size,
                repeat_policy: no_repeat_policy,
                mode: TableRowGroupAvoidMode::KeptByChromeOverflow,
            })
    }

    pub(in crate::layout::table) fn keeps_with_overflow(self) -> bool {
        self.mode == TableRowGroupAvoidMode::KeptByChromeOverflow
    }
}

impl TableAvoidRowGroupKeepState {
    pub(in crate::layout::table) fn commit(&mut self, decision: TableRowGroupAvoidDecision) {
        if decision.keeps_with_overflow() {
            self.end = Some(decision.group.end);
        }
    }

    pub(in crate::layout::table) fn contains_row(self, row_index: usize) -> bool {
        self.end.is_some_and(|end| row_index < end)
    }

    pub(in crate::layout::table) fn finish_row(&mut self, next_row_index: usize) {
        if self.end.is_some_and(|end| next_row_index >= end) {
            self.end = None;
        }
    }
}

/// Choose optional repeated table rows for a fragment with required body space.
///
/// CSS 2.2 permits print user agents to repeat table header and footer groups
/// on each page, but CSS Fragmentation still requires progress and treats
/// `break-inside: avoid` as a constraint to honor when possible. Prefer
/// preserving both repeated groups, then the header, then the footer, and
/// finally suppress optional repeats before creating a fragmentainer with no
/// usable body area. The repeated chrome is page-oriented today, while the
/// capacity math consumes a generic fragmentainer block size:
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>
/// <https://www.w3.org/TR/css-break-3/#break-within>
pub(in crate::layout::table) fn table_fragment_repeat_policy(
    required_body_height: LayoutLength,
    fragmentainer_block_size: LayoutLength,
    header_height: LayoutLength,
    footer_height: LayoutLength,
    allow_header: bool,
    allow_footer: bool,
) -> TableFragmentRepeatPolicy {
    let candidates = [
        TableFragmentRepeatPolicy {
            repeat_header: allow_header,
            repeat_footer: allow_footer,
        },
        TableFragmentRepeatPolicy {
            repeat_header: allow_header,
            repeat_footer: false,
        },
        TableFragmentRepeatPolicy {
            repeat_header: false,
            repeat_footer: allow_footer,
        },
        TableFragmentRepeatPolicy {
            repeat_header: false,
            repeat_footer: false,
        },
    ];

    let required_body_height = layout_pt(required_body_height.points().max(0.0));
    for policy in candidates {
        let body_capacity =
            policy.body_capacity(fragmentainer_block_size, header_height, footer_height);
        if body_capacity.points() > 0.01
            && required_body_height.points() <= body_capacity.points() + 0.01
        {
            return policy;
        }
    }

    candidates
        .into_iter()
        .find(|policy| {
            policy
                .body_capacity(fragmentainer_block_size, header_height, footer_height)
                .points()
                > 0.01
        })
        .unwrap_or(TableFragmentRepeatPolicy {
            repeat_header: false,
            repeat_footer: false,
        })
}
