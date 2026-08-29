use super::*;

/// Final block-size ownership after normal-flow children have been laid out.
///
/// A pre-layout definite height supplies a descendant percentage basis. In
/// contrast, an automatic block size clamped by `min-height` or `max-height`
/// becomes fixed only after measuring its children, and must not retroactively
/// make descendant percentage heights definite. Both cases nevertheless own a
/// fixed principal-box extent when assigning fragments and decorations.
/// <https://www.w3.org/TR/CSS2/visudet.html#min-max-heights>
/// <https://www.w3.org/TR/css-break-3/#parallel-flows>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum PrincipalBlockSizeDisposition {
    ContentSized,
    Fixed(FixedPrincipalBlockSize),
}

/// A fixed principal block size retained in the box-model space that supplied
/// it to fragmentation.
///
/// Descendant percentage layout always needs the resolved content-box height,
/// but cloned decoration changes how a fixed border-box extent is distributed
/// over destination fragmentainers. Retaining both facts prevents the
/// fragmentation cursor from mistaking `box-sizing: border-box` for an
/// equivalent content-box constraint.
/// <https://www.w3.org/TR/css-sizing-3/#box-model>
/// <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FixedPrincipalBlockSize {
    content_height: PhysicalContentHeight,
    specified_box: FixedPrincipalBlockSpecifiedBox,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum FixedPrincipalBlockSpecifiedBox {
    ContentBox,
    BorderBox(BorderBoxLength),
}

impl FixedPrincipalBlockSize {
    pub(in crate::layout) fn from_resolved_content_height(
        content_height: PhysicalContentHeight,
        box_sizing: BoxSizing,
        vertical_non_content: NonContentLength,
    ) -> Self {
        let specified_box = match box_sizing {
            BoxSizing::ContentBox => FixedPrincipalBlockSpecifiedBox::ContentBox,
            BoxSizing::BorderBox => {
                FixedPrincipalBlockSpecifiedBox::BorderBox(content_box_to_border_box_length(
                    content_height.content_box_length(),
                    vertical_non_content,
                ))
            }
        };
        Self {
            content_height,
            specified_box,
        }
    }

    pub(in crate::layout) const fn content_height(self) -> PhysicalContentHeight {
        self.content_height
    }

    pub(in crate::layout) const fn specified_box(self) -> FixedPrincipalBlockSpecifiedBox {
        self.specified_box
    }

    /// Start source-progress accounting for a cloned, fixed border-box.
    ///
    /// A content-box height remains source content. A border-box height,
    /// however, is a destination extent whose cloned decorations are paid in
    /// every fragmentainer before content can advance.
    pub(in crate::layout) fn cloned_border_box_progress(
        self,
        decoration: FragmentDecoration,
        reservation: FragmentDecorationReservation,
    ) -> Option<ClonedBorderBoxProgress> {
        match (self.specified_box, decoration) {
            (FixedPrincipalBlockSpecifiedBox::BorderBox(border_box), FragmentDecoration::Clone) => {
                Some(ClonedBorderBoxProgress::new(border_box, reservation))
            }
            _ => None,
        }
    }
}

/// Destination-space progress for a fixed border-box fragmented with cloned
/// decoration.
///
/// This keeps a fixed `border-box` height distinct from the source content
/// consumed by its fragments. Every completed destination fragment consumes
/// its owned cloned border and padding, while only the remaining interior
/// advances normal-flow source content.
/// <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ClonedBorderBoxProgress {
    remaining_border_box: BorderBoxLength,
    reservation: FragmentDecorationReservation,
}

impl ClonedBorderBoxProgress {
    fn new(
        remaining_border_box: BorderBoxLength,
        reservation: FragmentDecorationReservation,
    ) -> Self {
        debug_assert!(reservation.block_start().points() >= 0.0);
        debug_assert!(reservation.block_end().points() >= 0.0);
        Self {
            remaining_border_box,
            reservation,
        }
    }

    /// Consume one destination fragment's content capacity and return the
    /// corresponding source content extent.
    pub(in crate::layout) fn consume_content_capacity(
        &mut self,
        content_capacity: LayoutLength,
    ) -> LayoutLength {
        let owned_decoration =
            self.reservation.block_start().points() + self.reservation.block_end().points();
        let destination_capacity =
            (content_capacity.points().max(0.0) + owned_decoration).max(css::CSS_PX_TO_PT);
        let consumed_border_box = self
            .remaining_border_box
            .points()
            .min(destination_capacity)
            .max(0.0);
        self.remaining_border_box =
            border_box_pt((self.remaining_border_box.points() - consumed_border_box).max(0.0));
        layout_pt(
            (consumed_border_box - owned_decoration)
                .max(0.0)
                .min(content_capacity.points().max(0.0)),
        )
    }

    pub(in crate::layout) fn is_complete(self) -> bool {
        self.remaining_border_box.points() <= 0.01
    }

    pub(in crate::layout) const fn remaining_border_box(self) -> BorderBoxLength {
        self.remaining_border_box
    }
}

impl PrincipalBlockSizeDisposition {
    pub(in crate::layout) fn fixed_content_height(self) -> Option<PhysicalContentHeight> {
        match self {
            Self::ContentSized => None,
            Self::Fixed(size) => Some(size.content_height()),
        }
    }
}

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

/// The extent that must fit before a definite block can end at a class-A
/// fragmentation break.
///
/// A block-end margin is deliberately not part of `extent_before_break`.
/// When an unforced break is selected after this block, that margin adjoins
/// the break and is truncated. Keeping it as a separate pending value prevents
/// prebreak fitting from reserving space that the selected break cannot use.
/// <https://www.w3.org/TR/css-break-3/#break-margins>
#[derive(Debug, Clone, Copy, PartialEq)]
struct DefiniteBlockPrebreakRequirement {
    extent_before_break: LayoutLength,
    pending_block_end_margin: PendingFragmentationMargin,
}

impl DefiniteBlockPrebreakRequirement {
    fn from_definite_block(
        style: &ComputedStyle,
        vertical_non_content: NonContentLength,
        content_height: f32,
    ) -> Self {
        Self {
            extent_before_break: layout_pt(
                style.margin.top + vertical_non_content.points() + content_height.max(0.0),
            ),
            pending_block_end_margin: PendingFragmentationMargin::new(style.margin.bottom),
        }
    }

    fn fits_empty(self, fragmentainer: Fragmentainer) -> bool {
        fragmentainer.block_size_fits_empty(self.extent_before_break)
    }

    fn as_prebreak_input(self, context: DefiniteBlockBreakContext<'_>) -> FragmentPrebreakInput {
        debug_assert_eq!(
            self.pending_block_end_margin
                .resolve_for_fragmentainer_advance(FragmentainerAdvance::Unforced),
            layout_pt(0.0),
            "an unforced class-A break truncates the block-end margin"
        );
        FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer: context.current_fragmentainer,
            required_block_size: self.extent_before_break,
            empty_fragmentainer: context.empty_destination_fragmentainer,
            empty_fit_block_size: self.extent_before_break,
        }
    }

    #[cfg(test)]
    fn pending_block_end_margin(self) -> PendingFragmentationMargin {
        self.pending_block_end_margin
    }
}

/// A block-end margin whose used value depends on the selected break.
///
/// Normal sibling flow consumes this margin through margin collapsing. CSS
/// Fragmentation instead truncates it at an unforced break, while a forced
/// break retains the margin after the break. This type is intentionally
/// separate from CSS Box's author-controlled `margin-trim` state.
/// <https://www.w3.org/TR/css-break-3/#break-margins>
#[derive(Debug, Clone, Copy, PartialEq)]
struct PendingFragmentationMargin(LayoutLength);

impl PendingFragmentationMargin {
    fn new(points: f32) -> Self {
        Self(layout_pt(points))
    }

    fn resolve_for_fragmentainer_advance(self, advance: FragmentainerAdvance) -> LayoutLength {
        match advance {
            FragmentainerAdvance::Unforced => layout_pt(0.0),
            FragmentainerAdvance::Forced(_) => self.0,
        }
    }

    #[cfg(test)]
    fn resolve_for_sibling(self) -> LayoutLength {
        self.0
    }
}

/// Tracks whether the current parent has encountered an in-flow child. Floats
/// and positioned descendants deliberately cannot advance this state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::layout) enum FirstInFlowChildState {
    #[default]
    NotSeen,
    Seen,
}

impl FirstInFlowChildState {
    pub(in crate::layout) fn has_seen(self) -> bool {
        matches!(self, Self::Seen)
    }

    pub(in crate::layout) fn record_in_flow_child(&mut self) {
        *self = Self::Seen;
    }
}

pub(in crate::layout) struct AvoidBreakRunCandidateMeta {
    pub(in crate::layout) index: usize,
    pub(in crate::layout) element_index: usize,
    pub(in crate::layout) previous_flow_bottom_margin: Option<f32>,
    pub(in crate::layout) seen_flow_child: FirstInFlowChildState,
    pub(in crate::layout) trim_block_start_adjoining_margins: bool,
    pub(in crate::layout) collapsed_end_margin: bool,
    pub(in crate::layout) previous_child_page_end: Option<Option<String>>,
    pub(in crate::layout) float_run: FloatRunState,
    pub(in crate::layout) remaining_line_clamp: Option<css::RemainingLineSlots>,
    pub(in crate::layout) block_extent: LayoutLength,
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

/// Input state that can affect an avoid-run preflight extent.
///
/// The preflight is an estimate, not a fragment replay: it answers only how
/// much block-axis space a source child would require before the class-A break
/// is selected. A child can be revisited after an avoid-run snapshot is
/// restored, so retain a result only while every layout input that can alter
/// its estimate is identical. In particular, float exclusions are compared as
/// used geometry rather than by count: two floats with the same count can
/// expose different line widths.
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct AvoidRunPreflightKey {
    child_index: usize,
    available_outer_width: f32,
    writing_mode: WritingMode,
    fragmentainer_kind: FragmentainerKind,
    fragmentainer: Fragmentainer,
    float_context: Option<FloatContext>,
}

impl AvoidRunPreflightKey {
    pub(in crate::layout) fn capture(
        builder: &LayoutBuilder<'_>,
        child_index: usize,
        available_outer_width: f32,
        writing_mode: WritingMode,
        fragmentainer_kind: FragmentainerKind,
        fragmentainer: Fragmentainer,
    ) -> Self {
        Self {
            child_index,
            available_outer_width,
            writing_mode,
            fragmentainer_kind,
            fragmentainer,
            float_context: builder.float_contexts.last().cloned(),
        }
    }
}

/// Per-formatting-context memoization for avoid-run preflight extents.
///
/// A cache belongs to one child traversal and is deliberately not stored on
/// [`LayoutBuilder`]. This prevents stale source geometry from crossing into a
/// different formatting context while still allowing a restored sibling run to
/// reuse its preflight measurement when its complete context matches.
#[derive(Debug, Default)]
pub(in crate::layout) struct AvoidRunPreflightCache {
    entries: Vec<(AvoidRunPreflightKey, Option<LayoutLength>)>,
}

impl AvoidRunPreflightCache {
    pub(in crate::layout) fn get(
        &self,
        key: &AvoidRunPreflightKey,
    ) -> Option<Option<LayoutLength>> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, extent)| *extent)
    }

    pub(in crate::layout) fn insert(
        &mut self,
        key: AvoidRunPreflightKey,
        extent: Option<LayoutLength>,
    ) {
        self.entries.push((key, extent));
    }
}

/// Inputs for moving an avoid-constrained sibling run before the next child.
///
/// Grouping the source and destination fragmentainers with the run sizes
/// avoids accidentally comparing the next child against current remaining
/// space or using a page-only empty-state flag for a column continuation.
/// <https://www.w3.org/TR/css-break-3/#break-between>
pub(in crate::layout) struct AvoidRunPrebreakInput {
    pub(in crate::layout) run_block_extent: LayoutLength,
    pub(in crate::layout) next_block_extent: LayoutLength,
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
    pub(in crate::layout) fn block_extent(&self) -> LayoutLength {
        self.meta.block_extent
    }

    pub(in crate::layout) fn add_block_extent(mut self, block_extent: LayoutLength) -> Self {
        self.meta.block_extent += block_extent;
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
        if self.snapshot.current_page_has_flow_content() {
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
    pub(in crate::layout) seen_flow_child: FirstInFlowChildState,
    pub(in crate::layout) trim_block_start_adjoining_margins: bool,
    pub(in crate::layout) collapsed_end_margin: bool,
    pub(in crate::layout) previous_child_page_end: Option<Option<String>>,
    pub(in crate::layout) float_run: FloatRunState,
    pub(in crate::layout) previous_break_after: PageBreak,
}

pub(in crate::layout) struct PendingAdjoiningFloatReplayCandidate {
    pub(in crate::layout) snapshot: Box<LayoutSnapshot>,
    pub(in crate::layout) meta: AdjoiningFloatReplayCandidateMeta,
}

/// A resolved CSS2 clearance edge that prevents adjoining-float replay across
/// it within the float formatting context that produced it.
///
/// CSS2 clearance moves a box's border edge below the relevant floats. A
/// transparent descendant can emit a float after that move, so this records
/// the used edge rather than the syntactic `clear` value.
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatReplayClearanceBoundary(PageTopBlockPosition);

impl FloatReplayClearanceBoundary {
    pub(in crate::layout) const fn new(border_top: PageTopBlockPosition) -> Self {
        Self(border_top)
    }

    pub(in crate::layout) const fn border_top(self) -> PageTopBlockPosition {
        self.0
    }
}

pub(in crate::layout) struct AdjoiningFloatReplayCandidate {
    snapshot: Box<LayoutSnapshot>,
    pub(in crate::layout) meta: AdjoiningFloatReplayCandidateMeta,
    /// A descendant clearance boundary that gives this otherwise
    /// self-collapsing candidate a real block-end placement for its following
    /// sibling. It must not be replayed as an adjoining float run.
    clearance_boundary: Option<FloatReplayClearanceBoundary>,
}

impl PendingAdjoiningFloatReplayCandidate {
    /// Finalize the pre-layout checkpoint after the self-collapsing child has
    /// reported its lexical clearance scope.
    pub(in crate::layout) fn arm(
        self,
        builder: &LayoutBuilder<'_>,
    ) -> AdjoiningFloatReplayCandidate {
        AdjoiningFloatReplayCandidate {
            snapshot: self.snapshot,
            meta: self.meta,
            clearance_boundary: builder.current_float_replay_clearance_boundary(),
        }
    }
}

impl AdjoiningFloatReplayCandidate {
    pub(in crate::layout) fn snapshot(&self) -> &LayoutSnapshot {
        &self.snapshot
    }

    pub(in crate::layout) fn snapshot_cursor_y(&self) -> f32 {
        self.snapshot.cursor_y()
    }

    pub(in crate::layout) fn clearance_boundary(&self) -> Option<FloatReplayClearanceBoundary> {
        self.clearance_boundary
    }

    pub(in crate::layout) fn restore(
        self,
        builder: &mut LayoutBuilder<'_>,
    ) -> AdjoiningFloatReplayCandidateMeta {
        builder.restore(*self.snapshot);
        self.meta
    }
}

impl LayoutBuilder<'_> {
    /// Return the active lexical clearance boundary for adjoining-float replay.
    pub(in crate::layout) fn current_float_replay_clearance_boundary(
        &self,
    ) -> Option<FloatReplayClearanceBoundary> {
        self.float_replay_clearance_scopes.last().copied().flatten()
    }

    /// Run a child-flow phase with its inherited CSS2 clearance boundary.
    ///
    /// A nested independent formatting context receives `None`: its floats
    /// cannot affect the outer float context or its replay decision.
    pub(in crate::layout) fn with_float_replay_clearance_scope<T>(
        &mut self,
        boundary: Option<FloatReplayClearanceBoundary>,
        layout: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.float_replay_clearance_scopes.push(boundary);
        let result = layout(self);
        let completed_boundary = self
            .float_replay_clearance_scopes
            .pop()
            .expect("float replay clearance scope is balanced");
        if let Some(completed_boundary) = completed_boundary
            && let Some(inherited_boundary) = self.float_replay_clearance_scopes.last_mut()
        {
            *inherited_boundary = Some(completed_boundary);
        }
        result
    }
}

pub(in crate::layout) fn should_move_avoid_break_run_to_next_fragmentainer(
    input: AvoidRunPrebreakInput,
) -> bool {
    FragmentPrebreakDecision::choose(FragmentPrebreakInput {
        can_advance: input.retry_context.can_advance(),
        current_fragmentainer: input.retry_context.current_fragmentainer,
        required_block_size: input.next_block_extent,
        empty_fragmentainer: input.retry_context.empty_destination_fragmentainer,
        empty_fit_block_size: input.run_block_extent + input.next_block_extent,
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
                .resolved_page_context(
                    self.destination_document_page_number(self.pages.len() + 2),
                    false,
                )
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
    let requirement = DefiniteBlockPrebreakRequirement::from_definite_block(
        context.style,
        context.vertical_non_content,
        content_height,
    );
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
        && requirement.fits_empty(context.empty_destination_fragmentainer);
    if (!context.fragmentainer_has_occupied_flow || context.at_page_top)
        && !improves_empty_destination
    {
        return false;
    }
    if context.suppress_for_avoid_retry && requirement.fits_empty(context.current_fragmentainer) {
        return false;
    }
    FragmentPrebreakDecision::choose(requirement.as_prebreak_input(context)).should_break
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_in_flow_child_state_ignores_out_of_flow_siblings_until_recorded() {
        let mut state = FirstInFlowChildState::NotSeen;

        assert!(!state.has_seen());
        // Floats and positioned descendants intentionally have no state
        // transition API: only an in-flow child can consume first-child
        // status.
        assert!(!state.has_seen());
        state.record_in_flow_child();
        assert!(state.has_seen());
    }

    fn preflight_key(
        available_outer_width: f32,
        writing_mode: WritingMode,
        fragmentainer_kind: FragmentainerKind,
        float_context: Option<FloatContext>,
    ) -> AvoidRunPreflightKey {
        AvoidRunPreflightKey {
            child_index: 3,
            available_outer_width,
            writing_mode,
            fragmentainer_kind,
            fragmentainer: Fragmentainer::new(layout_pt(100.0), layout_pt(40.0)),
            float_context,
        }
    }

    #[test]
    fn avoid_run_preflight_cache_requires_the_full_layout_context() {
        let mut cache = AvoidRunPreflightCache::default();
        let float_context = FloatContext {
            shapes: vec![FloatShape::from_edges(
                FloatId(1),
                Float::Left,
                UsedFloatSide::Left,
                0,
                0,
                false,
                false,
                0,
                0.0,
                20.0,
                50.0,
                20.0,
            )],
        };
        let key = preflight_key(
            80.0,
            WritingMode::HorizontalTb,
            FragmentainerKind::Column,
            Some(float_context.clone()),
        );
        cache.insert(key.clone(), Some(layout_pt(24.0)));

        assert_eq!(cache.get(&key), Some(Some(layout_pt(24.0))));
        assert_eq!(
            cache.get(&preflight_key(
                81.0,
                WritingMode::HorizontalTb,
                FragmentainerKind::Column,
                Some(float_context.clone()),
            )),
            None
        );
        assert_eq!(
            cache.get(&preflight_key(
                80.0,
                WritingMode::VerticalRl,
                FragmentainerKind::Column,
                Some(float_context.clone()),
            )),
            None
        );
        assert_eq!(
            cache.get(&preflight_key(
                80.0,
                WritingMode::HorizontalTb,
                FragmentainerKind::Page,
                Some(float_context.clone()),
            )),
            None
        );

        let mut moved_float_context = float_context;
        moved_float_context.shapes[0].rect = PageTopRect::new(0.0, 45.0, 20.0, 30.0);
        assert_eq!(
            cache.get(&preflight_key(
                80.0,
                WritingMode::HorizontalTb,
                FragmentainerKind::Column,
                Some(moved_float_context),
            )),
            None
        );
    }

    #[test]
    fn definite_block_prebreak_ignores_a_margin_truncated_by_an_unforced_break() {
        let mut style = ComputedStyle::initial();
        style.margin.bottom = 50.0;
        let current_fragmentainer = Fragmentainer::new(layout_pt(100.0), layout_pt(50.0));
        let empty_destination_fragmentainer =
            Fragmentainer::new(layout_pt(100.0), layout_pt(100.0));
        let requirement = DefiniteBlockPrebreakRequirement::from_definite_block(
            &style,
            non_content_pt(0.0),
            50.0,
        );

        assert_eq!(requirement.extent_before_break, layout_pt(50.0));
        assert_eq!(
            requirement
                .pending_block_end_margin()
                .resolve_for_fragmentainer_advance(FragmentainerAdvance::Unforced),
            layout_pt(0.0)
        );
        assert!(requirement.fits_empty(current_fragmentainer));
        assert!(!should_prebreak_definite_block(DefiniteBlockBreakContext {
            definite_content_height: Some(50.0),
            vertical_non_content: non_content_pt(0.0),
            style: &style,
            current_fragmentainer,
            empty_destination_fragmentainer,
            fragmentainer_has_occupied_flow: true,
            at_page_top: false,
            suppress_for_avoid_retry: false,
        }));
    }

    #[test]
    fn pending_fragmentation_margin_follows_break_disposition() {
        let margin = PendingFragmentationMargin::new(50.0);

        assert_eq!(margin.resolve_for_sibling(), layout_pt(50.0));
        assert_eq!(
            margin.resolve_for_fragmentainer_advance(FragmentainerAdvance::Unforced),
            layout_pt(0.0)
        );
        assert_eq!(
            margin.resolve_for_fragmentainer_advance(FragmentainerAdvance::Forced(PageBreak::Page)),
            layout_pt(50.0)
        );
    }
}
