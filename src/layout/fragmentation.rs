use super::*;

/// Maximum anonymous column fragmentainers retained for one committed replay.
///
/// Continuous multicol overflow can contain an authored block size that would
/// imply millions of columns, while only a finite prefix can intersect a PDF
/// page or its bounded nested-fragment replay. Layout retains that prefix and
/// carries the logical tail arithmetically instead of allocating one temporary
/// [`Page`] per off-canvas column.
/// <https://www.w3.org/TR/css-multicol-1/#overflow>
pub(in crate::layout) const MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS: usize = 256;

/// Maximum page fragments retained while consuming one monolithic definite
/// block size. This is a resource boundary for pathological CSS lengths; the
/// remaining logical extent is carried arithmetically rather than allocating
/// unbounded empty PDF pages.
pub(in crate::layout) const MAX_MATERIALIZED_PAGE_FRAGMENTAINERS: usize = 256;

/// Maximum conceptual columns considered by the multicol balancing probe.
///
/// Balancing repeatedly lays out the same content. Large overflow is still
/// represented by the committed continuation plan, but probing a result whose
/// balance would require more than this many fragmentainers cannot improve the
/// visible bounded prefix and would multiply work by the binary-search count.
pub(in crate::layout) const MAX_MULTICOL_BALANCE_PROBE_FRAGMENTAINERS: usize = 4;

/// Bounded materialization for a run of equal-size column continuations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ColumnContinuationMaterialization {
    pub(in crate::layout) pages_to_push: usize,
    pub(in crate::layout) last_fragment_used_block_size: f32,
    pub(in crate::layout) has_unmaterialized_tail: bool,
}

/// Plan equal-size anonymous column continuations without work proportional
/// to an authored CSS length.
///
/// The returned final block offset is based on the full conceptual run, even
/// when only its paint-relevant prefix is retained. This keeps following
/// off-canvas flow ordered while bounding temporary memory and runtime.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
pub(in crate::layout) fn column_continuation_materialization(
    remaining_block_size: f32,
    continuation_block_size: f32,
    already_materialized: usize,
) -> ColumnContinuationMaterialization {
    continuation_materialization(
        remaining_block_size,
        continuation_block_size,
        already_materialized,
        MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS,
    )
}

/// Plan a bounded continuation run for any equal-size fragmentainer sequence.
pub(in crate::layout) fn continuation_materialization(
    remaining_block_size: f32,
    continuation_block_size: f32,
    already_materialized: usize,
    materialization_limit: usize,
) -> ColumnContinuationMaterialization {
    let continuation_block_size = continuation_block_size.max(css::CSS_PX_TO_PT);
    let remaining_block_size = remaining_block_size.max(0.0);
    let required =
        ((remaining_block_size - 0.01).max(0.0) / continuation_block_size).ceil() as usize;
    let available = materialization_limit.saturating_sub(already_materialized.max(1));
    let pages_to_push = required.min(available);
    let last_fragment_used_block_size = if required == 0 {
        0.0
    } else {
        let preceding = continuation_block_size * required.saturating_sub(1) as f32;
        (remaining_block_size - preceding).clamp(0.0, continuation_block_size)
    };
    ColumnContinuationMaterialization {
        pages_to_push,
        last_fragment_used_block_size,
        has_unmaterialized_tail: required > pages_to_push,
    }
}

/// Finite capacity for a CSS fragmentainer in the block direction.
///
/// CSS Fragmentation lays content into fragmentainers with a finite block-size.
/// This type carries the fragmentainer's empty block-size and current remaining
/// block-size so layout modes can share overflow and slice-boundary arithmetic
/// while keeping mode-specific reservations, such as repeated table chrome,
/// local:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct Fragmentainer {
    fragmentainer_block_size: f32,
    available_block_size: f32,
}

/// Fragmentation context targeted by a break decision.
///
/// CSS Break defines common break values across fragmentation contexts, but
/// target-specific values only apply to their matching fragmentainer type. The
/// shared layout code uses this kind to keep page and column decisions on the
/// same algorithm while preserving `avoid-page`/`avoid-column` and
/// `page`/`column` scoping:
/// <https://www.w3.org/TR/css-break-3/#break-types>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FragmentainerKind {
    Page,
    Column,
}

/// The reason a layout algorithm advances to its next fragmentainer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FragmentainerAdvance {
    Unforced,
    Forced(PageBreak),
}

impl LayoutBuilder<'_> {
    /// Materialize a layout algorithm's transition to another fragmentainer.
    ///
    /// CSS Fragmentation defines the distinction between ordinary and forced
    /// breaks in [§ 3.1](https://www.w3.org/TR/css-break-3/#breaking-controls).
    pub(in crate::layout) fn materialize_fragmentainer_advance(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        advance: FragmentainerAdvance,
    ) -> Option<f32> {
        if !self.fragmentainer_materializes_cursor(fragmentainer_kind) {
            return None;
        }

        match advance {
            FragmentainerAdvance::Unforced => self.push_page(),
            FragmentainerAdvance::Forced(page_break) => {
                self.apply_forced_break_in(fragmentainer_kind, page_break);
            }
        }

        Some(self.cursor_y)
    }
}

/// Resolved break values around one fragmentable source box.
///
/// CSS Fragmentation evaluates forced and avoided breaks at class A
/// opportunities between adjacent boxes. This context keeps the pending
/// incoming break, current box `break-before`, current box `break-after`, and
/// next sibling `break-before` together so layout modes do not mix authored
/// break sources while choosing forced or avoid-constrained boundaries:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentBreakContext {
    pub(in crate::layout) pending_before: PageBreak,
    pub(in crate::layout) before: PageBreak,
    pub(in crate::layout) after: PageBreak,
    pub(in crate::layout) next_before: PageBreak,
}

/// Cross-sibling forced break state carried while planning fragments.
///
/// CSS Fragmentation resolves forced breaks between adjacent boxes at class A
/// break opportunities. A source box's `break-after` becomes the next sibling's
/// pending `break-before` while siblings remain, or leaves the fragmenting
/// container as the outgoing forced break when the source list is exhausted:
/// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ForcedBreakCarryState {
    fragmentainer_kind: FragmentainerKind,
    before_next_box: PageBreak,
    after_source_boxes: PageBreak,
}

/// Shared decision for arming and consuming an avoid-run break candidate.
///
/// CSS Fragmentation treats `break-before: avoid` and `break-after: avoid` as
/// constraints at class A break opportunities. Layout modes that can roll an
/// avoid-constrained sibling run to the next fragmentainer need the same
/// inputs: whether the source box participates in the relevant flow, the
/// current adjacent-box break context, the committed break opportunity before
/// the source box, optional next sibling avoid state, and whether a rollback
/// candidate already exists:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentAvoidRunStartDecision {
    pub(in crate::layout) should_arm_start_candidate: bool,
    pub(in crate::layout) is_avoid_boundary: bool,
    pub(in crate::layout) seeds_later_avoid_boundary: bool,
}

/// A class A break opportunity at a source block-axis boundary.
///
/// CSS Fragmentation resolves forced and avoided breaks at boundaries between
/// in-flow boxes. Layout modes provide the source boundary geometry, while the
/// shared fragmentation layer applies target-aware break value scoping for
/// pages, columns, and future fragmentainer types:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks> and
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentBreakOpportunity {
    pub(in crate::layout) source_block_offset: f32,
    pub(in crate::layout) break_before: PageBreak,
    pub(in crate::layout) break_after: PageBreak,
    pub(in crate::layout) break_inside_avoid: bool,
}

/// Source-range query for choosing a committed break boundary.
///
/// CSS Fragmentation first determines which possible break points are
/// available in the current fragmentainer, then chooses forced breaks before
/// unforced breaks. Layout modes pass their ordered source boundaries here and
/// keep any mode-specific replay metadata outside the shared chooser:
/// <https://www.w3.org/TR/css-break-3/#forced-breaks> and
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentBreakOpportunitySearch<'a> {
    pub(in crate::layout) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout) opportunities: &'a [FragmentBreakOpportunity],
    pub(in crate::layout) source_block_start: f32,
    pub(in crate::layout) available_block_end: f32,
    pub(in crate::layout) content_block_end: f32,
}

/// Which side of an adjacent-box boundary contributes an avoid constraint.
///
/// CSS Fragmentation evaluates `break-after` from the previous box and
/// `break-before` from the following box at the same class A break
/// opportunity. Some layout modes need to know the side, not just whether the
/// boundary is avoided, because rollback metadata belongs to the source that
/// started the keep-together run:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FragmentAvoidBoundarySide {
    None,
    Previous,
    Current,
}

pub(in crate::layout) struct FragmentAvoidRunStartInput {
    pub(in crate::layout) participates_in_flow: bool,
    pub(in crate::layout) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout) break_context: FragmentBreakContext,
    pub(in crate::layout) break_opportunity: FragmentBreakOpportunity,
    pub(in crate::layout) next_break_before: Option<PageBreak>,
    pub(in crate::layout) has_avoid_run_candidate: bool,
}

/// Fragment-local slice chosen from a fragmentable source range.
///
/// CSS Fragmentation lets a box that is larger than a fragmentainer be split
/// across fragmentainers. Layout modes still own their source geometry and
/// paint/replay metadata, but the arithmetic for choosing the current
/// source-range end and detecting "advance before painting" is common:
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentSourceSliceDecision {
    pub(in crate::layout) slice_start: f32,
    pub(in crate::layout) slice_end: f32,
    pub(in crate::layout) advance_before_slice: bool,
}

pub(in crate::layout) struct FragmentSourceSliceInput {
    pub(in crate::layout) break_is_applicable: bool,
    pub(in crate::layout) source_is_oversized: bool,
    pub(in crate::layout) source_block_end: f32,
    pub(in crate::layout) slice_start: f32,
    pub(in crate::layout) available_block_end: f32,
}

/// Whole-source prebreak decision for a keepable unit.
///
/// CSS Fragmentation may choose an unforced break before a source box or run
/// when the break avoids overflow in the current fragmentainer and the kept
/// unit fits in an empty fragmentainer. Layout modes provide the
/// source-specific sizes and the empty fragmentainer because repeated table
/// chrome or other fragment-local reservations can make fresh capacity differ
/// from the current fragmentainer's nominal block-size:
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentPrebreakDecision {
    pub(in crate::layout) should_break: bool,
}

pub(in crate::layout) struct FragmentPrebreakInput {
    pub(in crate::layout) can_advance: bool,
    pub(in crate::layout) current_fragmentainer: Fragmentainer,
    pub(in crate::layout) required_block_size: f32,
    pub(in crate::layout) empty_fragmentainer: Fragmentainer,
    pub(in crate::layout) empty_fit_block_size: f32,
}

/// Decision to advance to another fragmentainer before a source unit.
///
/// CSS Fragmentation may break before a unit when overflow or avoid pressure
/// applies, provided the layout mode can make forward progress. Layout modes
/// still own target-specific transition metadata such as table repeated chrome
/// or flex source offsets; this shared decision keeps the common advance gate
/// from being restated in each mode:
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentAdvanceDecision {
    pub(in crate::layout) should_advance: bool,
}

pub(in crate::layout) struct FragmentAdvanceInput {
    pub(in crate::layout) break_is_applicable: bool,
    pub(in crate::layout) overflows: bool,
    pub(in crate::layout) avoid_break: bool,
    pub(in crate::layout) can_advance: bool,
}

impl Fragmentainer {
    pub(in crate::layout) fn new(fragmentainer_block_size: f32, available_block_size: f32) -> Self {
        Self {
            fragmentainer_block_size,
            available_block_size: available_block_size.max(0.0),
        }
    }

    /// Build a fragmentainer from the current physical cursor bounds.
    ///
    /// Quire's paged-media replay currently measures remaining block capacity
    /// from the current content block-start cursor down to the fragmentainer
    /// block-end edge. The arithmetic is shared by any fragmentainer whose
    /// physical cursor uses the same block-axis coordinates; callers remain
    /// responsible for passing target-specific bounds:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn from_cursor_bounds(
        fragmentainer_block_size: f32,
        content_block_start: f32,
        fragmentainer_block_end: f32,
    ) -> Self {
        Self::new(
            fragmentainer_block_size,
            content_block_start - fragmentainer_block_end,
        )
    }

    pub(in crate::layout) fn fragmentainer_block_size(self) -> f32 {
        self.fragmentainer_block_size
    }

    pub(in crate::layout) fn available_block_size(self) -> f32 {
        self.available_block_size
    }

    pub(in crate::layout) fn available_block_size_after_reservation(
        self,
        reserved_block_size: f32,
    ) -> f32 {
        (self.available_block_size - reserved_block_size).max(0.0)
    }

    pub(in crate::layout) fn required_block_size_overflows(self, block_size: f32) -> bool {
        block_size > self.available_block_size + 0.01
    }

    pub(in crate::layout) fn block_size_fits_empty(self, block_size: f32) -> bool {
        block_size <= self.fragmentainer_block_size + 0.01
    }

    pub(in crate::layout) fn available_block_end_from(self, block_offset: f32) -> f32 {
        block_offset + self.available_block_size
    }
}

impl FragmentainerKind {
    pub(in crate::layout) fn is_forced_break(self, value: PageBreak) -> bool {
        match self {
            Self::Page => value.is_forced(),
            Self::Column => matches!(value, PageBreak::Column),
        }
    }

    pub(in crate::layout) fn is_avoid_break(self, value: PageBreak) -> bool {
        match self {
            Self::Page => value.avoids_page(),
            Self::Column => value.avoids_column(),
        }
    }

    /// Return whether this fragmentainer kind is currently materialized by the
    /// paged-media page cursor.
    ///
    /// CSS Fragmentation uses the same break selection model for page and
    /// column fragmentation. Quire currently has concrete cursor materialization
    /// only for pages in these replay paths; column-targeted transitions remain
    /// committed break decisions but must not mutate paged-media state:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn materializes_page_cursor(self) -> bool {
        matches!(self, Self::Page)
    }

    /// Combine break values contributed by boxes in a layout unit.
    ///
    /// CSS Fragmentation treats forced breaks as stronger than avoid breaks,
    /// and target-specific break values only apply to their matching
    /// fragmentation context. Layout modes that aggregate child break values
    /// before exposing one class A boundary use this method to keep page and
    /// column scoping consistent:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks> and
    /// <https://www.w3.org/TR/css-break-3/#break-types>.
    pub(in crate::layout) fn combine_break(
        self,
        current: PageBreak,
        candidate: PageBreak,
    ) -> PageBreak {
        if self.is_forced_break(current) {
            current
        } else if self.is_forced_break(candidate) || self.is_avoid_break(candidate) {
            candidate
        } else {
            current
        }
    }

    /// Return whether `break-inside` avoids this fragmentation context.
    ///
    /// CSS Break lets `break-inside: avoid` apply to every fragmentation
    /// context, while `avoid-page` and `avoid-column` are target-specific.
    /// Computed style stores those target constraints separately; layout code
    /// should consume them through the active fragmentainer kind so page and
    /// column fragmentation share the same call shape:
    /// <https://www.w3.org/TR/css-break-3/#propdef-break-inside>.
    pub(in crate::layout) fn avoids_break_inside(self, style: &ComputedStyle) -> bool {
        match self {
            Self::Page => style.break_inside_avoid,
            Self::Column => style.break_inside_avoid_column,
        }
    }
}

impl FragmentBreakContext {
    pub(in crate::layout) fn new(
        pending_before: PageBreak,
        before: PageBreak,
        after: PageBreak,
        next_before: PageBreak,
    ) -> Self {
        Self {
            pending_before,
            before,
            after,
            next_before,
        }
    }

    /// Build a break context for a single generated box boundary.
    ///
    /// CSS Fragmentation resolves `break-before` before the generated box and
    /// `break-after` after it. Containers that are not currently carrying an
    /// adjacent sibling break use this context to keep those standalone box
    /// breaks on the same target-aware path as sibling break opportunities:
    /// <https://www.w3.org/TR/css-break-3/#break-between>.
    pub(in crate::layout) fn for_standalone_box(style: &ComputedStyle) -> Self {
        Self::new(
            PageBreak::Auto,
            style.break_before,
            style.break_after,
            PageBreak::Auto,
        )
    }

    /// Return the forced break to apply before this box in the target context.
    ///
    /// CSS Fragmentation resolves multiple forced breaks at the same class A
    /// break point by taking the value latest in flow. The following box's
    /// `break-before` therefore wins over the previous box's carried
    /// `break-after` for the same fragmentainer kind:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
    pub(in crate::layout) fn forced_break_before_in(
        self,
        kind: FragmentainerKind,
    ) -> Option<PageBreak> {
        if kind.is_forced_break(self.before) {
            Some(self.before)
        } else if kind.is_forced_break(self.pending_before) {
            Some(self.pending_before)
        } else {
            None
        }
    }

    pub(in crate::layout) fn forced_break_after_in(
        self,
        kind: FragmentainerKind,
    ) -> Option<PageBreak> {
        kind.is_forced_break(self.after).then_some(self.after)
    }

    pub(in crate::layout) fn forced_break_after_or_in(
        self,
        kind: FragmentainerKind,
        fallback: PageBreak,
    ) -> PageBreak {
        self.forced_break_after_in(kind).unwrap_or(fallback)
    }

    /// Returns whether the following sibling's forced `break-before` wins
    /// over this box's forced `break-after` at their shared class A boundary.
    ///
    /// CSS Fragmentation resolves forced breaks at the latest declaration in
    /// flow order, so an adjacent following box's `break-before` takes
    /// precedence over the preceding box's `break-after`:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
    pub(in crate::layout) fn next_forced_break_supersedes_after_in(
        self,
        kind: FragmentainerKind,
    ) -> bool {
        kind.is_forced_break(self.after) && kind.is_forced_break(self.next_before)
    }

    pub(in crate::layout) fn effective_break_before_in(self, kind: FragmentainerKind) -> PageBreak {
        if let Some(forced_break) = self.forced_break_before_in(kind) {
            forced_break
        } else if kind.is_avoid_break(self.pending_before) {
            self.pending_before
        } else {
            self.before
        }
    }

    /// Return which side avoids the boundary before this box.
    ///
    /// CSS Break combines the previous box's `break-after` and this box's
    /// `break-before` at a class A opportunity. `previous_break_after` is
    /// passed separately because some layout modes keep avoid state outside
    /// forced-break carry state while planning rollback candidates.
    pub(in crate::layout) fn avoid_boundary_side_before_box_in(
        self,
        kind: FragmentainerKind,
        previous_break_after: PageBreak,
    ) -> FragmentAvoidBoundarySide {
        if kind.is_avoid_break(previous_break_after) || kind.is_avoid_break(self.pending_before) {
            FragmentAvoidBoundarySide::Previous
        } else if kind.is_avoid_break(self.before) {
            FragmentAvoidBoundarySide::Current
        } else {
            FragmentAvoidBoundarySide::None
        }
    }

    pub(in crate::layout) fn seeds_later_avoid_boundary_in(
        self,
        kind: FragmentainerKind,
        next_break_before: Option<PageBreak>,
    ) -> bool {
        self.avoid_after_in(kind).is_some()
            || next_break_before.is_some_and(|value| kind.is_avoid_break(value))
    }

    pub(in crate::layout) fn avoid_after_in(self, kind: FragmentainerKind) -> Option<PageBreak> {
        kind.is_avoid_break(self.after).then_some(self.after)
    }

    pub(in crate::layout) fn next_avoid_before_in(
        self,
        kind: FragmentainerKind,
    ) -> Option<PageBreak> {
        kind.is_avoid_break(self.next_before)
            .then_some(self.next_before)
    }

    /// Return whether this adjacent-box boundary has authored break pressure
    /// for the requested fragmentation context.
    ///
    /// This remains useful to layout modes that need to decide whether a
    /// speculative class A boundary is required even though the former simple
    /// multicol support gate no longer consumes it.
    /// <https://www.w3.org/TR/css-break-3/#possible-breaks>
    #[cfg(test)]
    pub(in crate::layout) fn needs_class_a_break_decision_in(
        self,
        kind: FragmentainerKind,
    ) -> bool {
        kind.is_forced_break(self.effective_break_before_in(kind))
            || kind.is_forced_break(self.after)
            || kind.is_avoid_break(self.pending_before)
            || kind.is_avoid_break(self.before)
            || kind.is_avoid_break(self.after)
            || kind.is_avoid_break(self.next_before)
    }

    pub(in crate::layout) fn seeds_later_avoid_boundary_in_context_for(
        self,
        kind: FragmentainerKind,
    ) -> bool {
        self.seeds_later_avoid_boundary_in(kind, Some(self.next_before))
    }

    pub(in crate::layout) fn forced_after_source_boxes_in(
        self,
        kind: FragmentainerKind,
        has_next_box: bool,
    ) -> Option<PageBreak> {
        (kind.is_forced_break(self.after) && !has_next_box).then_some(self.after)
    }

    pub(in crate::layout) fn forced_before_next_box_in(
        self,
        kind: FragmentainerKind,
        has_next_box: bool,
    ) -> Option<PageBreak> {
        (kind.is_forced_break(self.after) && has_next_box).then_some(self.after)
    }
}

impl FragmentAvoidRunStartDecision {
    pub(in crate::layout) fn choose(input: FragmentAvoidRunStartInput) -> Self {
        let break_boundary_avoid = input
            .break_opportunity
            .avoids_break_in(input.fragmentainer_kind);
        let next_break_before_avoid = input
            .next_break_before
            .map(|value| input.fragmentainer_kind.is_avoid_break(value));
        let is_avoid_boundary = input.participates_in_flow && break_boundary_avoid;
        let seeds_later_avoid_boundary = input.participates_in_flow
            && input
                .break_context
                .seeds_later_avoid_boundary_in(input.fragmentainer_kind, input.next_break_before);
        let should_arm_start_candidate = input.participates_in_flow
            && (input
                .break_context
                .avoid_after_in(input.fragmentainer_kind)
                .is_some()
                || next_break_before_avoid.unwrap_or(true)
                || (break_boundary_avoid && !input.has_avoid_run_candidate));
        Self {
            should_arm_start_candidate,
            is_avoid_boundary,
            seeds_later_avoid_boundary,
        }
    }
}

impl FragmentBreakOpportunity {
    /// Construct the class A boundary before a source box.
    ///
    /// CSS Fragmentation combines the previous sibling's `break-after`, the
    /// current box's `break-before`, and ancestor/current `break-inside`
    /// constraints at the boundary before the current box. Forced breaks are
    /// already carried through `FragmentBreakContext`; avoid breaks from the
    /// previous sibling remain target-scoped and are represented as the
    /// boundary's `break-after` side:
    /// <https://www.w3.org/TR/css-break-3/#break-between>.
    pub(in crate::layout) fn before_box_boundary(
        kind: FragmentainerKind,
        source_block_offset: f32,
        break_context: FragmentBreakContext,
        previous_break_after: PageBreak,
        break_inside_avoid: bool,
    ) -> Self {
        let effective_break_before = break_context.effective_break_before_in(kind);
        Self {
            source_block_offset,
            break_before: if kind.is_forced_break(effective_break_before)
                || kind.is_avoid_break(effective_break_before)
            {
                effective_break_before
            } else {
                PageBreak::Auto
            },
            break_after: if kind.is_avoid_break(previous_break_after) {
                previous_break_after
            } else {
                PageBreak::Auto
            },
            break_inside_avoid,
        }
    }

    pub(in crate::layout) fn has_forced_break_in(self, kind: FragmentainerKind) -> bool {
        kind.is_forced_break(self.break_before) || kind.is_forced_break(self.break_after)
    }

    pub(in crate::layout) fn avoids_break_in(self, kind: FragmentainerKind) -> bool {
        self.break_inside_avoid
            || kind.is_avoid_break(self.break_before)
            || kind.is_avoid_break(self.break_after)
    }

    pub(in crate::layout) fn first_forced_in(
        search: FragmentBreakOpportunitySearch<'_>,
    ) -> Option<Self> {
        search
            .opportunities_in_fragmentainer()
            .filter(|opportunity| opportunity.has_forced_break_in(search.fragmentainer_kind))
            .min_by(|a, b| a.source_block_offset.total_cmp(&b.source_block_offset))
    }

    pub(in crate::layout) fn latest_unforced_in(
        search: FragmentBreakOpportunitySearch<'_>,
        allow_avoids: bool,
    ) -> Option<Self> {
        search
            .opportunities_in_fragmentainer()
            .filter(|opportunity| {
                allow_avoids || !opportunity.avoids_break_in(search.fragmentainer_kind)
            })
            .max_by(|a, b| a.source_block_offset.total_cmp(&b.source_block_offset))
    }

    pub(in crate::layout) fn latest_unforced_preferring_allowed_in(
        search: FragmentBreakOpportunitySearch<'_>,
    ) -> Option<Self> {
        Self::latest_unforced_in(search, false).or_else(|| Self::latest_unforced_in(search, true))
    }
}

impl<'a> FragmentBreakOpportunitySearch<'a> {
    fn opportunities_in_fragmentainer(self) -> impl Iterator<Item = FragmentBreakOpportunity> + 'a {
        self.opportunities
            .iter()
            .cloned()
            .filter(move |opportunity| {
                opportunity.source_block_offset > self.source_block_start + 0.01
                    && opportunity.source_block_offset <= self.available_block_end + 0.01
                    && opportunity.source_block_offset < self.content_block_end - 0.01
            })
    }
}

impl FragmentSourceSliceDecision {
    pub(in crate::layout) fn choose(input: FragmentSourceSliceInput) -> Self {
        let slice_end = if input.break_is_applicable
            && input.source_is_oversized
            && input.source_block_end > input.available_block_end + 0.01
        {
            input
                .available_block_end
                .min(input.source_block_end)
                .max(input.slice_start)
        } else {
            input.source_block_end
        };
        let advance_before_slice = input.break_is_applicable
            && slice_end <= input.slice_start + 0.01
            && input.source_block_end > input.slice_start + 0.01;
        Self {
            slice_start: input.slice_start,
            slice_end,
            advance_before_slice,
        }
    }

    pub(in crate::layout) fn paints_slice(self) -> bool {
        !self.advance_before_slice
    }
}

impl FragmentPrebreakDecision {
    pub(in crate::layout) fn choose(input: FragmentPrebreakInput) -> Self {
        let should_break = input.can_advance
            && input
                .current_fragmentainer
                .required_block_size_overflows(input.required_block_size)
            && input
                .empty_fragmentainer
                .block_size_fits_empty(input.empty_fit_block_size);
        Self { should_break }
    }
}

impl FragmentAdvanceDecision {
    pub(in crate::layout) fn choose(input: FragmentAdvanceInput) -> Self {
        Self {
            should_advance: input.break_is_applicable
                && input.can_advance
                && (input.overflows || input.avoid_break),
        }
    }
}

impl Default for ForcedBreakCarryState {
    fn default() -> Self {
        Self::new(FragmentainerKind::Page)
    }
}

impl ForcedBreakCarryState {
    pub(in crate::layout) fn new(fragmentainer_kind: FragmentainerKind) -> Self {
        Self {
            fragmentainer_kind,
            before_next_box: PageBreak::Auto,
            after_source_boxes: PageBreak::Auto,
        }
    }

    fn box_context(
        self,
        before: PageBreak,
        after: PageBreak,
        next_before: PageBreak,
    ) -> FragmentBreakContext {
        FragmentBreakContext::new(self.before_next_box, before, after, next_before)
    }

    pub(in crate::layout) fn take_box_context(
        &mut self,
        before: PageBreak,
        after: PageBreak,
        next_before: PageBreak,
    ) -> FragmentBreakContext {
        let context = self.box_context(before, after, next_before);
        self.clear_before_next_box();
        context
    }

    fn clear_before_next_box(&mut self) {
        self.before_next_box = PageBreak::Auto;
    }

    pub(in crate::layout) fn finish_box(
        &mut self,
        break_context: FragmentBreakContext,
        has_next_box: bool,
    ) {
        if let Some(forced_before_next_box) =
            break_context.forced_before_next_box_in(self.fragmentainer_kind, has_next_box)
        {
            self.before_next_box = forced_before_next_box;
        }
        self.after_source_boxes = break_context
            .forced_after_source_boxes_in(self.fragmentainer_kind, has_next_box)
            .unwrap_or(PageBreak::Auto);
    }

    pub(in crate::layout) fn outgoing_source_break(self) -> PageBreak {
        self.after_source_boxes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_layout_builder<'a>(
        options: &'a RenderOptions,
        stylesheets: &'a [Stylesheet],
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
        LayoutBuilder::new(LayoutBuilderConfig {
            options,
            stylesheets,
            base_url: None,
            root_url: None,
            resource_cache,
            iframe_documents: Box::leak(Box::new(HashMap::new())),
            iframe_viewport: None,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            font_system: FontSystem::new(),
        })
    }

    #[test]
    fn fragmentainer_advance_materializes_unforced_and_forced_page_transitions() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);

        builder.mark_current_page_flow_content();
        let page_count = builder.pages.len();
        let content_top = builder
            .materialize_fragmentainer_advance(
                FragmentainerKind::Page,
                FragmentainerAdvance::Unforced,
            )
            .expect("page fragmentainer materializes a cursor");
        assert_eq!(builder.pages.len(), page_count + 1);
        assert_eq!(content_top, builder.cursor_y);

        builder.mark_current_page_flow_content();
        let page_count = builder.pages.len();
        let content_top = builder
            .materialize_fragmentainer_advance(
                FragmentainerKind::Page,
                FragmentainerAdvance::Forced(PageBreak::Page),
            )
            .expect("forced page break materializes a cursor");
        assert_eq!(builder.pages.len(), page_count + 1);
        assert_eq!(content_top, builder.cursor_y);
    }

    #[test]
    fn fragmentainer_advance_leaves_nonmaterialized_cursor_unchanged() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let page_count = builder.pages.len();
        let cursor_y = builder.cursor_y;

        assert_eq!(
            builder.materialize_fragmentainer_advance(
                FragmentainerKind::Column,
                FragmentainerAdvance::Unforced,
            ),
            None
        );
        assert_eq!(builder.pages.len(), page_count);
        assert_eq!(builder.cursor_y, cursor_y);
    }

    #[test]
    fn fragmentainer_capacity_uses_empty_and_remaining_block_sizes() {
        let fragmentainer = Fragmentainer::new(100.0, 40.0);

        assert_eq!(fragmentainer.fragmentainer_block_size(), 100.0);
        assert_eq!(fragmentainer.available_block_size(), 40.0);
        assert!(fragmentainer.block_size_fits_empty(100.0));
        assert!(!fragmentainer.block_size_fits_empty(101.0));
        assert!(fragmentainer.required_block_size_overflows(41.0));
        assert_eq!(
            fragmentainer.available_block_size_after_reservation(15.0),
            25.0
        );
        assert_eq!(
            fragmentainer.available_block_size_after_reservation(80.0),
            0.0
        );
    }

    #[test]
    fn fragmentainer_capacity_can_derive_remaining_size_from_cursor_bounds() {
        let fragmentainer = Fragmentainer::from_cursor_bounds(200.0, 640.0, 500.0);

        assert_eq!(fragmentainer.fragmentainer_block_size(), 200.0);
        assert_eq!(fragmentainer.available_block_size(), 140.0);
    }

    #[test]
    fn source_slice_paints_available_oversized_piece() {
        let decision = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
            break_is_applicable: true,
            source_is_oversized: true,
            source_block_end: 120.0,
            slice_start: 40.0,
            available_block_end: 75.0,
        });

        assert!(decision.paints_slice());
        assert_eq!(decision.slice_start, 40.0);
        assert_eq!(decision.slice_end, 75.0);
        assert!(!decision.advance_before_slice);
    }

    #[test]
    fn source_slice_advances_when_no_progress_is_possible() {
        let decision = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
            break_is_applicable: true,
            source_is_oversized: true,
            source_block_end: 120.0,
            slice_start: 40.0,
            available_block_end: 40.0,
        });

        assert!(!decision.paints_slice());
        assert_eq!(decision.slice_start, 40.0);
        assert_eq!(decision.slice_end, 40.0);
        assert!(decision.advance_before_slice);
    }

    #[test]
    fn source_slice_keeps_unfragmented_end_when_breaks_are_not_applicable() {
        let decision = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
            break_is_applicable: false,
            source_is_oversized: true,
            source_block_end: 120.0,
            slice_start: 40.0,
            available_block_end: 75.0,
        });

        assert!(decision.paints_slice());
        assert_eq!(decision.slice_start, 40.0);
        assert_eq!(decision.slice_end, 120.0);
        assert!(!decision.advance_before_slice);
    }

    #[test]
    fn prebreak_moves_keepable_unit_that_overflows_remaining_space() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer: Fragmentainer::new(100.0, 40.0),
            required_block_size: 60.0,
            empty_fragmentainer: Fragmentainer::new(100.0, 100.0),
            empty_fit_block_size: 80.0,
        });

        assert!(decision.should_break);
    }

    #[test]
    fn prebreak_stays_when_unit_fits_remaining_space() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer: Fragmentainer::new(100.0, 40.0),
            required_block_size: 40.0,
            empty_fragmentainer: Fragmentainer::new(100.0, 100.0),
            empty_fit_block_size: 80.0,
        });

        assert!(!decision.should_break);
    }

    #[test]
    fn prebreak_stays_when_kept_unit_cannot_fit_empty_fragmentainer() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer: Fragmentainer::new(100.0, 40.0),
            required_block_size: 60.0,
            empty_fragmentainer: Fragmentainer::new(100.0, 100.0),
            empty_fit_block_size: 120.0,
        });

        assert!(!decision.should_break);
    }

    #[test]
    fn prebreak_uses_explicit_empty_fragmentainer_capacity() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer: Fragmentainer::new(100.0, 40.0),
            required_block_size: 60.0,
            empty_fragmentainer: Fragmentainer::new(50.0, 50.0),
            empty_fit_block_size: 80.0,
        });

        assert!(!decision.should_break);
    }

    #[test]
    fn prebreak_stays_when_fragmentainer_cannot_advance() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: false,
            current_fragmentainer: Fragmentainer::new(100.0, 40.0),
            required_block_size: 60.0,
            empty_fragmentainer: Fragmentainer::new(100.0, 100.0),
            empty_fit_block_size: 80.0,
        });

        assert!(!decision.should_break);
    }

    #[test]
    fn advance_decision_moves_for_overflow_or_avoid_pressure() {
        assert!(
            FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: true,
                overflows: true,
                avoid_break: false,
                can_advance: true,
            })
            .should_advance
        );
        assert!(
            FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: true,
                overflows: false,
                avoid_break: true,
                can_advance: true,
            })
            .should_advance
        );
    }

    #[test]
    fn advance_decision_stays_without_applicable_break_or_pressure() {
        assert!(
            !FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: false,
                overflows: true,
                avoid_break: true,
                can_advance: true,
            })
            .should_advance
        );
        assert!(
            !FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: true,
                overflows: false,
                avoid_break: false,
                can_advance: true,
            })
            .should_advance
        );
        assert!(
            !FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: true,
                overflows: true,
                avoid_break: true,
                can_advance: false,
            })
            .should_advance
        );
    }

    #[test]
    fn break_opportunity_selects_first_forced_boundary_for_target_fragmentainer() {
        let opportunities = [
            FragmentBreakOpportunity {
                source_block_offset: 40.0,
                break_before: PageBreak::Column,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            FragmentBreakOpportunity {
                source_block_offset: 80.0,
                break_before: PageBreak::Page,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            FragmentBreakOpportunity {
                source_block_offset: 120.0,
                break_before: PageBreak::Left,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
        ];

        let page_search = FragmentBreakOpportunitySearch {
            fragmentainer_kind: FragmentainerKind::Page,
            opportunities: &opportunities,
            source_block_start: 0.0,
            available_block_end: 150.0,
            content_block_end: 200.0,
        };
        let column_search = FragmentBreakOpportunitySearch {
            fragmentainer_kind: FragmentainerKind::Column,
            ..page_search
        };

        assert_eq!(
            FragmentBreakOpportunity::first_forced_in(page_search)
                .map(|boundary| { boundary.source_block_offset }),
            Some(80.0)
        );
        assert_eq!(
            FragmentBreakOpportunity::first_forced_in(column_search)
                .map(|boundary| { boundary.source_block_offset }),
            Some(40.0)
        );
    }

    #[test]
    fn fragmentainer_break_combiner_scopes_forced_and_avoid_values() {
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Auto, PageBreak::Column),
            PageBreak::Auto
        );
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Auto, PageBreak::AvoidColumn),
            PageBreak::Auto
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Auto, PageBreak::Column),
            PageBreak::Column
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Auto, PageBreak::AvoidColumn),
            PageBreak::AvoidColumn
        );
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Left, PageBreak::Page),
            PageBreak::Left
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Column, PageBreak::Avoid),
            PageBreak::Column
        );
    }

    #[test]
    fn fragmentainer_kind_page_cursor_materialization_is_target_specific() {
        assert!(FragmentainerKind::Page.materializes_page_cursor());
        assert!(!FragmentainerKind::Column.materializes_page_cursor());
    }

    #[test]
    fn avoid_boundary_side_preserves_boundary_source_precedence() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::AvoidPage,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert_eq!(
            context.avoid_boundary_side_before_box_in(FragmentainerKind::Page, PageBreak::Auto),
            FragmentAvoidBoundarySide::Current
        );
        assert_eq!(
            context
                .avoid_boundary_side_before_box_in(FragmentainerKind::Page, PageBreak::AvoidPage),
            FragmentAvoidBoundarySide::Previous
        );
        assert_eq!(
            FragmentBreakContext::new(
                PageBreak::AvoidPage,
                PageBreak::AvoidPage,
                PageBreak::Auto,
                PageBreak::Auto,
            )
            .avoid_boundary_side_before_box_in(FragmentainerKind::Page, PageBreak::Auto),
            FragmentAvoidBoundarySide::Previous
        );
    }

    #[test]
    fn avoid_boundary_side_scopes_avoid_values_to_fragmentainer() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::AvoidColumn,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert_eq!(
            context.avoid_boundary_side_before_box_in(FragmentainerKind::Page, PageBreak::Auto),
            FragmentAvoidBoundarySide::None
        );
        assert_eq!(
            context.avoid_boundary_side_before_box_in(FragmentainerKind::Column, PageBreak::Auto),
            FragmentAvoidBoundarySide::Current
        );
    }

    #[test]
    fn break_context_returns_target_specific_avoid_values() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::AvoidColumn,
            PageBreak::AvoidPage,
        );

        assert_eq!(context.avoid_after_in(FragmentainerKind::Page), None);
        assert_eq!(
            context.avoid_after_in(FragmentainerKind::Column),
            Some(PageBreak::AvoidColumn)
        );
        assert_eq!(
            context.next_avoid_before_in(FragmentainerKind::Page),
            Some(PageBreak::AvoidPage)
        );
        assert_eq!(
            context.next_avoid_before_in(FragmentainerKind::Column),
            None
        );
    }

    #[test]
    fn before_box_break_opportunity_preserves_target_specific_previous_avoid() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::AvoidPage,
            PageBreak::Auto,
            PageBreak::Auto,
        );
        let page_opportunity = FragmentBreakOpportunity::before_box_boundary(
            FragmentainerKind::Page,
            40.0,
            context,
            PageBreak::AvoidColumn,
            false,
        );
        let column_opportunity = FragmentBreakOpportunity::before_box_boundary(
            FragmentainerKind::Column,
            40.0,
            context,
            PageBreak::AvoidColumn,
            false,
        );

        assert!(page_opportunity.avoids_break_in(FragmentainerKind::Page));
        assert!(!page_opportunity.avoids_break_in(FragmentainerKind::Column));
        assert!(column_opportunity.avoids_break_in(FragmentainerKind::Column));
        assert!(!column_opportunity.avoids_break_in(FragmentainerKind::Page));
    }

    #[test]
    fn avoid_run_start_decision_consumes_target_specific_break_opportunity() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
        );
        let opportunity = FragmentBreakOpportunity {
            source_block_offset: 40.0,
            break_before: PageBreak::Auto,
            break_after: PageBreak::AvoidColumn,
            break_inside_avoid: false,
        };

        let page_decision = FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            participates_in_flow: true,
            fragmentainer_kind: FragmentainerKind::Page,
            break_context: context,
            break_opportunity: opportunity,
            next_break_before: Some(PageBreak::Auto),
            has_avoid_run_candidate: false,
        });
        let column_decision = FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            fragmentainer_kind: FragmentainerKind::Column,
            ..FragmentAvoidRunStartInput {
                participates_in_flow: true,
                fragmentainer_kind: FragmentainerKind::Page,
                break_context: context,
                break_opportunity: opportunity,
                next_break_before: Some(PageBreak::Auto),
                has_avoid_run_candidate: false,
            }
        });

        assert!(!page_decision.is_avoid_boundary);
        assert!(!page_decision.should_arm_start_candidate);
        assert!(column_decision.is_avoid_boundary);
        assert!(column_decision.should_arm_start_candidate);
    }

    #[test]
    fn avoid_run_start_decision_scopes_next_break_before_to_fragmentainer() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
        );
        let opportunity = FragmentBreakOpportunity {
            source_block_offset: 40.0,
            break_before: PageBreak::Auto,
            break_after: PageBreak::Auto,
            break_inside_avoid: false,
        };

        let page_decision = FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            participates_in_flow: true,
            fragmentainer_kind: FragmentainerKind::Page,
            break_context: context,
            break_opportunity: opportunity,
            next_break_before: Some(PageBreak::AvoidColumn),
            has_avoid_run_candidate: false,
        });
        let column_decision = FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            fragmentainer_kind: FragmentainerKind::Column,
            ..FragmentAvoidRunStartInput {
                participates_in_flow: true,
                fragmentainer_kind: FragmentainerKind::Page,
                break_context: context,
                break_opportunity: opportunity,
                next_break_before: Some(PageBreak::AvoidColumn),
                has_avoid_run_candidate: false,
            }
        });

        assert!(!page_decision.seeds_later_avoid_boundary);
        assert!(!page_decision.should_arm_start_candidate);
        assert!(column_decision.seeds_later_avoid_boundary);
        assert!(column_decision.should_arm_start_candidate);
    }

    #[test]
    fn break_opportunity_prefers_latest_non_avoid_boundary_before_avoidable_boundary() {
        let opportunities = [
            FragmentBreakOpportunity {
                source_block_offset: 40.0,
                break_before: PageBreak::Auto,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            FragmentBreakOpportunity {
                source_block_offset: 80.0,
                break_before: PageBreak::Auto,
                break_after: PageBreak::AvoidPage,
                break_inside_avoid: false,
            },
            FragmentBreakOpportunity {
                source_block_offset: 120.0,
                break_before: PageBreak::Auto,
                break_after: PageBreak::Auto,
                break_inside_avoid: true,
            },
        ];
        let search = FragmentBreakOpportunitySearch {
            fragmentainer_kind: FragmentainerKind::Page,
            opportunities: &opportunities,
            source_block_start: 0.0,
            available_block_end: 150.0,
            content_block_end: 200.0,
        };

        assert_eq!(
            FragmentBreakOpportunity::latest_unforced_preferring_allowed_in(search)
                .map(|boundary| boundary.source_block_offset),
            Some(40.0)
        );
        assert_eq!(
            FragmentBreakOpportunity::latest_unforced_preferring_allowed_in(
                FragmentBreakOpportunitySearch {
                    source_block_start: 40.0,
                    ..search
                },
            )
            .map(|boundary| boundary.source_block_offset),
            Some(120.0)
        );
    }

    #[test]
    fn target_specific_break_context_keeps_page_and_column_values_separate() {
        let page_context = FragmentBreakContext::new(
            PageBreak::AvoidPage,
            PageBreak::Auto,
            PageBreak::Page,
            PageBreak::Auto,
        );
        assert!(page_context.needs_class_a_break_decision_in(FragmentainerKind::Page));
        assert!(!page_context.needs_class_a_break_decision_in(FragmentainerKind::Column));

        let column_context = FragmentBreakContext::new(
            PageBreak::AvoidColumn,
            PageBreak::Auto,
            PageBreak::Column,
            PageBreak::Auto,
        );
        assert!(!column_context.needs_class_a_break_decision_in(FragmentainerKind::Page));
        assert!(column_context.needs_class_a_break_decision_in(FragmentainerKind::Column));
    }

    #[test]
    fn effective_break_before_ignores_other_fragmentainer_pending_breaks() {
        let context = FragmentBreakContext::new(
            PageBreak::Column,
            PageBreak::Page,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert_eq!(
            context.effective_break_before_in(FragmentainerKind::Page),
            PageBreak::Page
        );
        assert_eq!(
            context.effective_break_before_in(FragmentainerKind::Column),
            PageBreak::Column
        );
        assert!(context.needs_class_a_break_decision_in(FragmentainerKind::Page));
        assert!(context.needs_class_a_break_decision_in(FragmentainerKind::Column));
    }

    #[test]
    fn forced_break_before_uses_latest_break_at_boundary() {
        let context = FragmentBreakContext::new(
            PageBreak::Page,
            PageBreak::Left,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert_eq!(
            context.forced_break_before_in(FragmentainerKind::Page),
            Some(PageBreak::Left)
        );
        assert_eq!(
            context.effective_break_before_in(FragmentainerKind::Page),
            PageBreak::Left
        );
    }

    #[test]
    fn standalone_box_break_context_scopes_forced_box_boundaries() {
        let mut style = ComputedStyle::initial();
        style.break_before = PageBreak::Column;
        style.break_after = PageBreak::Page;
        let context = FragmentBreakContext::for_standalone_box(&style);

        assert_eq!(
            context.forced_break_before_in(FragmentainerKind::Page),
            None
        );
        assert_eq!(
            context.forced_break_before_in(FragmentainerKind::Column),
            Some(PageBreak::Column)
        );
        assert_eq!(
            context.forced_break_after_in(FragmentainerKind::Page),
            Some(PageBreak::Page)
        );
        assert_eq!(
            context.forced_break_after_in(FragmentainerKind::Column),
            None
        );
    }

    #[test]
    fn standalone_box_break_after_can_fall_back_to_descendant_outgoing_break() {
        let mut style = ComputedStyle::initial();
        style.break_after = PageBreak::Auto;
        let context = FragmentBreakContext::for_standalone_box(&style);

        assert_eq!(
            context.forced_break_after_or_in(FragmentainerKind::Page, PageBreak::Left),
            PageBreak::Left
        );

        style.break_after = PageBreak::Right;
        let context = FragmentBreakContext::for_standalone_box(&style);

        assert_eq!(
            context.forced_break_after_or_in(FragmentainerKind::Page, PageBreak::Left),
            PageBreak::Right
        );
    }

    #[test]
    fn generic_avoid_applies_to_every_fragmentainer_kind() {
        let context = FragmentBreakContext::new(
            PageBreak::Avoid,
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert!(context.needs_class_a_break_decision_in(FragmentainerKind::Page));
        assert!(context.needs_class_a_break_decision_in(FragmentainerKind::Column));
    }

    #[test]
    fn break_inside_avoid_is_target_specific() {
        let mut style = ComputedStyle::initial();
        style.break_inside_avoid = true;

        assert!(FragmentainerKind::Page.avoids_break_inside(&style));
        assert!(!FragmentainerKind::Column.avoids_break_inside(&style));

        style.break_inside_avoid = false;
        style.break_inside_avoid_column = true;

        assert!(!FragmentainerKind::Page.avoids_break_inside(&style));
        assert!(FragmentainerKind::Column.avoids_break_inside(&style));

        style.break_inside_avoid = true;

        assert!(FragmentainerKind::Page.avoids_break_inside(&style));
        assert!(FragmentainerKind::Column.avoids_break_inside(&style));
    }

    #[test]
    fn forced_break_carry_is_target_specific() {
        let mut page_carry = ForcedBreakCarryState::new(FragmentainerKind::Page);
        let page_context =
            page_carry.take_box_context(PageBreak::Auto, PageBreak::Column, PageBreak::Auto);
        page_carry.finish_box(page_context, true);
        let next_page_context =
            page_carry.take_box_context(PageBreak::Auto, PageBreak::Auto, PageBreak::Auto);
        assert_eq!(next_page_context.pending_before, PageBreak::Auto);

        let mut column_carry = ForcedBreakCarryState::new(FragmentainerKind::Column);
        let column_context =
            column_carry.take_box_context(PageBreak::Auto, PageBreak::Column, PageBreak::Auto);
        column_carry.finish_box(column_context, true);
        let next_column_context =
            column_carry.take_box_context(PageBreak::Auto, PageBreak::Auto, PageBreak::Auto);
        assert_eq!(next_column_context.pending_before, PageBreak::Column);

        let mut outgoing_column_carry = ForcedBreakCarryState::new(FragmentainerKind::Column);
        let outgoing_context = outgoing_column_carry.take_box_context(
            PageBreak::Auto,
            PageBreak::Column,
            PageBreak::Auto,
        );
        outgoing_column_carry.finish_box(outgoing_context, false);
        assert_eq!(
            outgoing_column_carry.outgoing_source_break(),
            PageBreak::Column
        );
    }

    #[test]
    fn column_continuation_plan_materializes_normal_runs_exactly() {
        let plan = column_continuation_materialization(250.0, 100.0, 1);

        assert_eq!(plan.pages_to_push, 3);
        assert_eq!(plan.last_fragment_used_block_size, 50.0);
        assert!(!plan.has_unmaterialized_tail);
    }

    #[test]
    fn column_continuation_plan_bounds_extreme_authored_lengths() {
        let plan = column_continuation_materialization(1_000_000_000.0, 100.0, 1);

        assert_eq!(
            plan.pages_to_push,
            MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS - 1
        );
        assert!(plan.last_fragment_used_block_size > 0.0);
        assert!(plan.last_fragment_used_block_size <= 100.0);
        assert!(plan.has_unmaterialized_tail);
    }
}
