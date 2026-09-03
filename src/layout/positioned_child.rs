use std::borrow::Cow;

use super::*;

/// Selects which positioned-descendant containing-block stacks a box establishes.
///
/// CSS Positioned Layout defines the absolute-position containing block for
/// positioned ancestors, while layout/paint containment and transforms also
/// establish the containing block for fixed-position descendants:
/// <https://www.w3.org/TR/css-position-3/#def-cb> and
/// <https://drafts.csswg.org/css-transforms-1/#transform-rendering>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum PositionedContainingBlockMode {
    AbsoluteOnly,
    FixedAndAbsolute,
}

impl PositionedContainingBlockMode {
    /// Select containment positioning effects from the used principal-box
    /// containment, rather than directly from the authored shorthand.
    pub(in crate::layout) fn for_element(element: &Element, style: &ComputedStyle) -> Option<Self> {
        if used_property_containment(element, style).establishes_fixed_position_containing_block()
            || style.has_transform()
        {
            Some(Self::FixedAndAbsolute)
        } else if matches!(
            style.position,
            Position::Relative | Position::Absolute | Position::Fixed | Position::Sticky
        ) {
            Some(Self::AbsoluteOnly)
        } else {
            None
        }
    }

    pub(in crate::layout) fn for_style(style: &ComputedStyle) -> Option<Self> {
        if style.contain.layout || style.contain.paint || style.has_transform() {
            Some(Self::FixedAndAbsolute)
        } else if matches!(
            style.position,
            Position::Relative | Position::Absolute | Position::Fixed | Position::Sticky
        ) {
            Some(Self::AbsoluteOnly)
        } else {
            None
        }
    }

    fn establishes_fixed_containing_block(self) -> bool {
        matches!(self, Self::FixedAndAbsolute)
    }
}

/// Records the stack depths before a positioned-containing-block scope.
///
/// The token intentionally does not borrow the builder, so a caller can retain
/// it while replaying fragments in a separate temporary layout context.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedContainingBlockScope {
    containing_blocks_depth: usize,
    fixed_containing_blocks_depth: usize,
    mode: PositionedContainingBlockMode,
    multicol_span_id: Option<u64>,
}

/// State restored after laying out one atomic inline formatting context on
/// its scratch page.
///
/// The coordinate-space identity and static-position containing block have
/// the same lifetime: descendants may retain either after deferred positioned
/// layout, but an enclosing atom must never observe them as its own state.
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AtomicInlineFormattingContextScope {
    coordinate_space: AtomicInlineCoordinateSpaceId,
    previous_axes: WritingModeAxes,
    static_position_context_depth: usize,
}

/// Durable source geometry for a positioned containing block encountered in
/// temporary multicolumn layout. Descendant replay resolves against this
/// continuous source containing block, then uses the shared source-to-
/// destination candidates retained by the multicolumn owner.
#[derive(Debug, Clone)]
pub(in crate::layout) struct MulticolPositionedContainingBlockSpan {
    pub(in crate::layout) id: u64,
    pub(in crate::layout) mode: PositionedContainingBlockMode,
    pub(in crate::layout) containing_block: ContainingBlock,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedChildStaticRect {
    left: f32,
    right: f32,
    top: f32,
    containing_block: Option<ContainingBlock>,
    static_alignment: Option<AbsposStaticAlignment>,
}

/// The source-coordinate overflow-clip ancestry of a positioned descendant
/// deferred out of temporary multicolumn layout.
///
/// An empty chain is meaningful: it records that the descendant was captured
/// with no ancestor overflow clips.  Replay must use this durable source
/// geometry rather than whichever clip scopes happen to be active after the
/// temporary column pages have been restored.
/// <https://drafts.csswg.org/css-overflow-3/#overflow-clip-edge>
#[derive(Debug, Clone)]
struct PositionedSourceOverflowClipChain(Vec<OverflowClip>);

impl PositionedSourceOverflowClipChain {
    fn capture(clips: &[OverflowClip]) -> Self {
        Self(clips.to_vec())
    }

    fn clips(&self) -> &[OverflowClip] {
        &self.0
    }
}

/// Durable source context required to replay one positioned descendant after
/// its multicolumn owner restores the enclosing layout state.
///
/// The containing-block owner and overflow-clip ancestry are one source
/// coordinate-system contract. Keeping them together prevents replay from
/// restoring a containing block while silently inheriting unrelated ambient
/// clips from the outer builder.
/// <https://www.w3.org/TR/css-position-3/#abspos-containing-block>
/// <https://drafts.csswg.org/css-overflow-3/#overflow-clipping>
#[derive(Debug, Clone)]
struct DeferredMulticolReplayContext {
    /// Stable owner captured from the normal-flow positioned containing block,
    /// rather than a temporary multicolumn page index.
    containing_block_span_id: Option<u64>,
    source_overflow_clips: PositionedSourceOverflowClipChain,
}

/// An out-of-flow flex descendant measured while a multicolumn container owns
/// temporary fragmentainer pages.
///
/// Column pages are implementation scratch space, so their positioned paint
/// layers cannot be committed directly.  The flex static-position rectangle is
/// nevertheless final geometry and is retained until the enclosing multicol
/// container restores its real containing-block context.
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
#[derive(Debug, Clone)]
pub(in crate::layout) struct DeferredMulticolPositionedChild {
    element: Element,
    signature: Box<ElementSignature>,
    style: ComputedStyle,
    replay_context: DeferredMulticolReplayContext,
    fragment: PositionedFragmentReplay,
}

impl DeferredMulticolPositionedChild {
    /// Direct children of the principal multicol box use that continuous
    /// containing block, not a fragmented in-flow ancestor span captured
    /// while anonymous columns were being measured.
    pub(in crate::layout) fn with_principal_multicol_ownership(mut self) -> Self {
        self.replay_context.containing_block_span_id = None;
        self
    }
}

/// Committed destination data for an out-of-flow child emitted by a fragmented
/// formatting context.
///
/// The static rectangle stays in source coordinates, while translation and
/// clip describe the owning fragmentainer. Keeping these in one record avoids
/// replaying a child against temporary multicolumn scratch-page coordinates.
/// Both direct positioned children and flex positioned children can use this
/// same payload when their containing context commits a fragment.
/// <https://www.w3.org/TR/css-position-3/#static-position>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
/// One source-to-destination projection available to a positioned child whose
/// final fragment owner depends on resolved inset geometry.
///
/// Normal-flow and already-owned positioned records have one committed
/// projection. An unresolved physical-row flex static position keeps all
/// candidate source slices until positioned layout establishes which paint
/// intersects each slice.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PositionedFragmentCandidate {
    source_clip: PaintClip,
    source_block_start: LayoutLength,
    source_block_end: LayoutLength,
    continuous_source_to_local: PaintTranslation,
    source_to_destination: PaintTranslation,
    destination_clip: PaintClip,
}

/// The geometry that selects one or more committed multicolumn projections
/// for a deferred positioned descendant.
///
/// A physical-row flex child selects its owner from the final resolved block
/// inset. A physical-column flex child instead has a static main-axis interval
/// in the fragmented source coordinate system, which can intersect several
/// source fragmentainers.
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
enum PositionedFragmentOwnerResolution {
    None,
    /// The positioned principal is resolved in one continuous source
    /// containing-block coordinate system, then its resulting paint is
    /// clipped and projected through every committed multicolumn source
    /// fragmentainer.
    ///
    /// Absolutely positioned descendants do not participate in normal flow,
    /// so no temporary column page can be their single owner.  Selecting one
    /// would either discard overflow that crosses a column boundary or move
    /// the entire principal into a later anonymous column.
    /// <https://www.w3.org/TR/css-position-3/#fragmenting-absolutely-positioned-elements>
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>
    AllCommittedSourceFragments,
    FinalBlockInset(Option<LayoutLength>),
    SourceBlockInterval {
        start: LayoutLength,
        end: LayoutLength,
    },
}

/// Coordinate space of source layers produced while resolving an unresolved
/// positioned-fragment owner.
///
/// An automatic inset uses the flex static-position rectangle, which is
/// already local to its selected source fragmentainer. A definite physical
/// block inset resolves to a source-global coordinate that must be localized
/// before its layer is projected into a destination fragmentainer.
/// <https://www.w3.org/TR/css-position-3/#static-position>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionedCandidateSourceSpace {
    StaticPositionLocal,
    DefiniteBlockInsetGlobal,
}

impl PositionedFragmentCandidate {
    pub(in crate::layout) fn new(
        source_clip: PaintClip,
        source_block_start: LayoutLength,
        source_block_end: LayoutLength,
        continuous_source_to_local: PaintTranslation,
        source_to_destination: PaintTranslation,
        destination_clip: PaintClip,
    ) -> Self {
        Self {
            source_clip,
            source_block_start,
            source_block_end,
            continuous_source_to_local,
            source_to_destination,
            destination_clip,
        }
    }

    fn continuous_source_to_destination(self) -> PaintTranslation {
        PaintTranslation::new(
            self.continuous_source_to_local.x + self.source_to_destination.x,
            self.continuous_source_to_local.y + self.source_to_destination.y,
        )
    }

    /// Continue an equal-sized committed column projection from the preceding
    /// pair. The source-local clip intentionally remains the same temporary
    /// column canvas; only the continuous-source and destination mappings
    /// advance.
    fn continuation_after(self, previous: Self) -> Self {
        let source_to_destination_step = PaintTranslation::new(
            self.source_to_destination.x - previous.source_to_destination.x,
            self.source_to_destination.y - previous.source_to_destination.y,
        );
        let continuous_to_local_step = PaintTranslation::new(
            self.continuous_source_to_local.x - previous.continuous_source_to_local.x,
            self.continuous_source_to_local.y - previous.continuous_source_to_local.y,
        );
        let source_block_extent = self.source_block_end - self.source_block_start;
        Self {
            source_clip: self.source_clip,
            source_block_start: self.source_block_end,
            source_block_end: self.source_block_end + source_block_extent,
            continuous_source_to_local: PaintTranslation::new(
                self.continuous_source_to_local.x + continuous_to_local_step.x,
                self.continuous_source_to_local.y + continuous_to_local_step.y,
            ),
            source_to_destination: PaintTranslation::new(
                self.source_to_destination.x + source_to_destination_step.x,
                self.source_to_destination.y + source_to_destination_step.y,
            ),
            destination_clip: self.destination_clip.translated(source_to_destination_step),
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct PositionedFragmentReplay {
    source_static_rect: PositionedChildStaticRect,
    positioning_containing_block: Option<(PositionedContainingBlockMode, ContainingBlock)>,
    source_fragment_block_offset: LayoutLength,
    owner_resolution: PositionedFragmentOwnerResolution,
    candidate_source_space: PositionedCandidateSourceSpace,
    unresolved_candidates: Vec<PositionedFragmentCandidate>,
}

impl PositionedFragmentReplay {
    pub(in crate::layout) fn unfragmented(
        source_static_rect: PositionedChildStaticRect,
        positioning_containing_block: Option<(PositionedContainingBlockMode, ContainingBlock)>,
    ) -> Self {
        Self {
            source_static_rect,
            positioning_containing_block,
            source_fragment_block_offset: layout_pt(0.0),
            owner_resolution: PositionedFragmentOwnerResolution::None,
            candidate_source_space: PositionedCandidateSourceSpace::StaticPositionLocal,
            unresolved_candidates: Vec::new(),
        }
    }

    /// Retain the temporary source fragmentainer that supplied the captured
    /// containing-block geometry. Candidate replay translates that geometry
    /// into each selected source fragmentainer before projecting paint back to
    /// its final multicolumn destination.
    /// <https://www.w3.org/TR/css-position-3/#def-cb>
    pub(in crate::layout) fn with_source_fragment_block_offset(
        mut self,
        source_fragment_block_offset: LayoutLength,
    ) -> Self {
        self.source_fragment_block_offset = source_fragment_block_offset;
        self
    }

    /// Mark a record whose physical-row fragment owner depends on a definite
    /// physical block inset resolved by positioned layout.
    ///
    /// Auto block insets retain their source static placement and do not need
    /// candidate replay; projecting them through every multicolumn slice
    /// would duplicate or displace a valid static-position descendant.
    /// <https://www.w3.org/TR/css-position-3/#inset-properties>
    pub(in crate::layout) fn resolving_owner_from_final_block_inset(
        mut self,
        final_block_inset_from_start: Option<LayoutLength>,
    ) -> Self {
        self.owner_resolution =
            PositionedFragmentOwnerResolution::FinalBlockInset(final_block_inset_from_start);
        self
    }

    /// Replay a positioned principal through every committed multicolumn
    /// source slice that intersects its resolved source paint.
    ///
    /// Candidate clips eliminate non-intersecting ink after positioned layout,
    /// so this intentionally retains all source slices rather than predicting
    /// ownership from the static-position rectangle.  The latter is only a
    /// fallback position and is not the fragmentation extent of an abspos box.
    /// <https://www.w3.org/TR/css-position-3/#static-position>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn across_committed_multicolumn_fragments(mut self) -> Self {
        debug_assert!(matches!(
            self.owner_resolution,
            PositionedFragmentOwnerResolution::None
        ));
        self.owner_resolution = PositionedFragmentOwnerResolution::AllCommittedSourceFragments;
        self
    }

    /// Mark a record whose physical-column static main-axis interval intersects
    /// one or more committed source fragmentainers.
    ///
    /// The static interval is supplied in the flex source block coordinate
    /// system. It remains independent from the page-space static rectangle,
    /// which positioned layout uses for inset resolution.
    /// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>
    pub(in crate::layout) fn resolving_owner_from_source_block_interval(
        mut self,
        start: LayoutLength,
        end: LayoutLength,
    ) -> Self {
        debug_assert!(end >= start, "a positioned source interval is monotonic");
        self.owner_resolution =
            PositionedFragmentOwnerResolution::SourceBlockInterval { start, end };
        self
    }

    /// The resolved positioned layer uses a source-global physical block
    /// coordinate rather than the selected fragmentainer's local static
    /// position.
    pub(in crate::layout) fn with_definite_block_inset_source_coordinates(mut self) -> Self {
        self.candidate_source_space = PositionedCandidateSourceSpace::DefiniteBlockInsetGlobal;
        self
    }

    /// Retain a candidate projection while final positioned geometry is still
    /// unresolved. Candidates are source-slice disjoint and retain the exact
    /// destination clip used by normal-flow multicolumn paint.
    pub(in crate::layout) fn add_unresolved_candidate(
        &mut self,
        candidate: PositionedFragmentCandidate,
    ) {
        if !matches!(
            self.owner_resolution,
            PositionedFragmentOwnerResolution::None
        ) && !self.unresolved_candidates.iter().any(|existing| {
            existing.source_clip == candidate.source_clip
                && existing.source_block_start == candidate.source_block_start
                && existing.source_block_end == candidate.source_block_end
                && existing.source_to_destination == candidate.source_to_destination
        }) {
            self.unresolved_candidates.push(candidate);
        }
    }

    pub(in crate::layout) fn has_unresolved_candidates(&self) -> bool {
        !matches!(
            self.owner_resolution,
            PositionedFragmentOwnerResolution::None
        ) && !self.unresolved_candidates.is_empty()
    }

    fn candidates_for_owner(&self) -> Vec<PositionedFragmentCandidate> {
        let selected = self
            .unresolved_candidates
            .iter()
            .copied()
            .filter(|candidate| match self.owner_resolution {
                PositionedFragmentOwnerResolution::None => false,
                PositionedFragmentOwnerResolution::AllCommittedSourceFragments => true,
                PositionedFragmentOwnerResolution::FinalBlockInset(Some(inset)) => {
                    inset >= candidate.source_block_start
                        && inset < candidate.source_block_end - layout_pt(0.01)
                }
                PositionedFragmentOwnerResolution::FinalBlockInset(None) => true,
                PositionedFragmentOwnerResolution::SourceBlockInterval { start, end } => {
                    start < candidate.source_block_end - layout_pt(0.01)
                        && candidate.source_block_start < end - layout_pt(0.01)
                }
            })
            .collect::<Vec<_>>();
        if selected.is_empty() {
            self.unresolved_candidates.clone()
        } else {
            selected
        }
    }

    fn localizes_static_rect_per_candidate(&self) -> bool {
        matches!(
            self.owner_resolution,
            PositionedFragmentOwnerResolution::SourceBlockInterval { .. }
        )
    }

    fn source_layers_select_candidates(&self) -> bool {
        !matches!(
            self.owner_resolution,
            PositionedFragmentOwnerResolution::AllCommittedSourceFragments
        )
    }

    /// Extend an equal-sized multicolumn projection sequence when the resolved
    /// principal has finite, axis-aligned source ink beyond the normal-flow
    /// columns that happened to be committed. Out-of-flow boxes do not create
    /// normal-flow columns, but CSS still fragments their overflowing paint.
    ///
    /// Effects with indeterminate bounds deliberately do not synthesize a
    /// destination continuation here; those retain every already-committed
    /// candidate in `append_multicol_positioned_candidate_layers`.
    /// <https://www.w3.org/TR/css-position-3/#fragmenting-absolutely-positioned-elements>
    fn candidates_extended_for_source_layers(
        &self,
        mut candidates: Vec<PositionedFragmentCandidate>,
        source_layers: &[PositionedPaintLayer],
        pages: &[Page],
        current_page: &Page,
    ) -> Vec<PositionedFragmentCandidate> {
        if !matches!(
            self.owner_resolution,
            PositionedFragmentOwnerResolution::AllCommittedSourceFragments
        ) || candidates.len() < 2
        {
            return candidates;
        }
        let second = candidates[1];
        let first = candidates[0];
        let continuous_block_step = second.source_block_end - second.source_block_start;
        if continuous_block_step <= layout_pt(0.01) {
            return candidates;
        }
        let block_uses_vertical_paint_axis =
            (second.continuous_source_to_local.y - first.continuous_source_to_local.y).abs()
                >= (second.continuous_source_to_local.x - first.continuous_source_to_local.x).abs();
        let local_fragmentainer_extent = if block_uses_vertical_paint_axis {
            first.source_clip.height()
        } else {
            first.source_clip.width()
        };
        if local_fragmentainer_extent <= 0.01 {
            return candidates;
        }
        let mut required_source_extent = 0.0f32;
        for layer in source_layers {
            let page = pages.get(layer.page_index).unwrap_or(current_page);
            let source_context = layer.context.clone().into_primitive_nodes(page);
            let Ok(Some(bounds)) = source_context.recorded_paint_bounds(page) else {
                continue;
            };
            let extent = if block_uses_vertical_paint_axis {
                bounds.height()
            } else {
                bounds.width()
            };
            required_source_extent = required_source_extent.max(extent);
        }
        let required_candidates = (required_source_extent / local_fragmentainer_extent)
            .ceil()
            .max(1.0) as usize;
        while candidates.len() < required_candidates {
            let previous = candidates[candidates.len() - 2];
            let current = candidates[candidates.len() - 1];
            candidates.push(current.continuation_after(previous));
        }
        candidates
    }

    fn source_containing_block(&self) -> Option<(PositionedContainingBlockMode, ContainingBlock)> {
        self.positioning_containing_block
    }

    fn containing_block_local_to_candidate(
        &self,
        candidate: PositionedFragmentCandidate,
    ) -> Option<(PositionedContainingBlockMode, ContainingBlock)> {
        self.positioning_containing_block
            .map(|(mode, containing_block)| {
                let source_translation = PaintTranslation::new(
                    0.0,
                    candidate.source_block_start.points()
                        - self.source_fragment_block_offset.points(),
                );
                (mode, containing_block.translated(source_translation))
            })
    }
}

impl PositionedChildStaticRect {
    pub(in crate::layout) fn new(left: f32, right: f32, top: f32) -> Self {
        Self {
            left,
            right,
            top,
            containing_block: None,
            static_alignment: None,
        }
    }

    pub(in crate::layout) fn with_containing_block(
        left: f32,
        right: f32,
        top: f32,
        containing_block: ContainingBlock,
    ) -> Self {
        Self {
            left,
            right,
            top,
            containing_block: Some(containing_block),
            static_alignment: None,
        }
    }

    pub(in crate::layout) fn with_static_alignment(
        mut self,
        static_alignment: AbsposStaticAlignment,
    ) -> Self {
        self.static_alignment = Some(static_alignment);
        self
    }

    /// Replace selected physical edges of the static-position rectangle while
    /// retaining its independently resolved containing block.
    ///
    /// Grid's automatic placement lines may lie on the opposite padding edge
    /// of an explicit line outside the explicit grid. The geometric area is
    /// normalized for the containing block, but the static corner must retain
    /// that explicit logical edge. CSS Grid §9.1 defines these separately.
    pub(in crate::layout) fn with_static_physical_edges(
        mut self,
        left: Option<f32>,
        right: Option<f32>,
        top: Option<f32>,
    ) -> Self {
        if let Some(left) = left {
            self.left = left;
        }
        if let Some(right) = right {
            self.right = right;
        }
        if let Some(top) = top {
            self.top = top;
        }
        self
    }

    fn layout_right(self) -> f32 {
        self.left + (self.right - self.left).max(1.0)
    }

    /// Translate the static-position fallback edges into one committed
    /// fragmentainer coordinate system.
    ///
    /// Alignment areas remain expressed against the positioned-layout
    /// containing block retained separately by the replay record. They must
    /// not be translated with this fallback geometry.
    /// <https://www.w3.org/TR/css-position-3/#static-position>
    pub(in crate::layout) fn translated(mut self, translation: PaintTranslation) -> Self {
        self.left += translation.x;
        self.right += translation.x;
        self.top += translation.y;
        self
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn begin_atomic_inline_formatting_context(
        &mut self,
        style: &ComputedStyle,
        content_rect: PageTopRect,
    ) -> AtomicInlineFormattingContextScope {
        let coordinate_space =
            AtomicInlineCoordinateSpaceId::new(self.next_atomic_inline_coordinate_space_id);
        self.next_atomic_inline_coordinate_space_id += 1;
        self.active_atomic_inline_coordinate_spaces
            .push(coordinate_space);

        let previous_axes = WritingModeAxes::new(
            self.containing_block_writing_mode,
            self.containing_block_direction,
        );
        let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
        self.containing_block_writing_mode = axes.writing_mode();
        self.containing_block_direction = axes.direction();
        let static_position_context_depth = self.static_position_containing_blocks.len();
        self.static_position_containing_blocks
            .push(StaticPositionContainingBlock::new(
                axes,
                content_rect,
                style.justify_items,
            ));

        AtomicInlineFormattingContextScope {
            coordinate_space,
            previous_axes,
            static_position_context_depth,
        }
    }

    pub(in crate::layout) fn end_atomic_inline_formatting_context(
        &mut self,
        scope: AtomicInlineFormattingContextScope,
    ) {
        debug_assert_eq!(
            self.static_position_containing_blocks.len(),
            scope.static_position_context_depth + 1,
        );
        self.static_position_containing_blocks.pop();
        debug_assert_eq!(
            self.active_atomic_inline_coordinate_spaces.last(),
            Some(&scope.coordinate_space),
        );
        self.active_atomic_inline_coordinate_spaces.pop();
        self.containing_block_writing_mode = scope.previous_axes.writing_mode();
        self.containing_block_direction = scope.previous_axes.direction();
    }

    pub(in crate::layout) fn current_positioned_coordinate_space(
        &self,
    ) -> PositionedCoordinateSpace {
        self.active_atomic_inline_coordinate_spaces
            .last()
            .copied()
            .map(PositionedCoordinateSpace::AtomicInline)
            .unwrap_or(PositionedCoordinateSpace::Page)
    }

    pub(in crate::layout) fn positioned_containing_block_context(
        &self,
        geometry: ContainingBlock,
    ) -> PositionedContainingBlockContext {
        PositionedContainingBlockContext::in_space(
            geometry,
            self.current_positioned_coordinate_space(),
        )
    }

    /// Lay out one deferred multicolumn positioned principal into a private
    /// layer vector, then restore the enclosing replay's layers unchanged.
    ///
    /// Nested multicolumn layout uses the builder's positioned-layer vector as
    /// temporary scratch state. Keeping an enclosing deferred replay in that
    /// vector lets an inner column set clear or consume the outer layers,
    /// which both loses paint and invalidates a later `split_off` index.
    /// <https://www.w3.org/TR/css-position-3/#fragmenting-absolutely-positioned-elements>
    fn capture_deferred_multicol_source_layers(
        &mut self,
        layout: impl FnOnce(&mut Self),
    ) -> Vec<PositionedPaintLayer> {
        let parent_positioned_layers = std::mem::take(&mut self.positioned_layers);
        layout(self);
        let source_layers =
            std::mem::replace(&mut self.positioned_layers, parent_positioned_layers);
        // Fixed layers have independent document-level ownership. In
        // particular, a viewport-fixed descendant discovered while resolving
        // this source principal must remain in that queue rather than being
        // folded into its fragmentainer-local continuation layers.
        source_layers
    }

    /// Capture the source state that a deferred positioned child must replay
    /// after temporary multicolumn layout restores the enclosing builder.
    fn deferred_multicol_replay_context(
        &self,
        position: &Position,
    ) -> DeferredMulticolReplayContext {
        DeferredMulticolReplayContext {
            containing_block_span_id: self
                .active_multicol_positioned_containing_block_span_id(position),
            source_overflow_clips: PositionedSourceOverflowClipChain::capture(&self.overflow_clips),
        }
    }

    pub(in crate::layout) fn deferred_multicol_positioned_child(
        &self,
        element: &Element,
        signature: &ElementSignature,
        style: ComputedStyle,
        fragment: PositionedFragmentReplay,
    ) -> DeferredMulticolPositionedChild {
        let replay_context = self.deferred_multicol_replay_context(&style.position);
        debug_assert!(replay_context.containing_block_span_id.is_none_or(|id| {
            self.multicol_positioned_containing_block_spans
                .iter()
                .any(|span| span.id == id)
        }));
        DeferredMulticolPositionedChild {
            element: element.clone(),
            signature: Box::new(signature.clone()),
            style,
            replay_context,
            fragment,
        }
    }

    /// Run deferred source layout under its captured containing-block and
    /// overflow-clip context, then restore the enclosing builder state.
    fn with_deferred_multicol_replay_context<T>(
        &mut self,
        context: &DeferredMulticolReplayContext,
        containing_block: Option<(PositionedContainingBlockMode, ContainingBlock)>,
        layout: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved_overflow_clips = std::mem::replace(
            &mut self.overflow_clips,
            context.source_overflow_clips.clips().to_vec(),
        );
        let containing_block_scope = containing_block.map(|(mode, containing_block)| {
            self.push_positioned_containing_block(mode, containing_block)
        });
        let result = layout(self);
        if let Some(scope) = containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        self.overflow_clips = saved_overflow_clips;
        result
    }

    fn replay_deferred_multicol_source_layers(
        &mut self,
        context: &DeferredMulticolReplayContext,
        containing_block: Option<(PositionedContainingBlockMode, ContainingBlock)>,
        child: &FormattingContextChild<'_>,
        stylesheets: &Stylesheets<'_>,
        static_rect: PositionedChildStaticRect,
    ) -> Vec<PositionedPaintLayer> {
        self.with_deferred_multicol_replay_context(context, containing_block, |layout| {
            layout.capture_deferred_multicol_source_layers(|layout| {
                layout.layout_positioned_formatting_context_child(child, stylesheets, static_rect);
            })
        })
    }

    /// Queue a positioned child with its already committed fragment payload.
    pub(in crate::layout) fn defer_multicol_positioned_fragment_child(
        &mut self,
        child: &FormattingContextChild<'_>,
        fragment: PositionedFragmentReplay,
    ) {
        let Some((element, signature, _)) = child.element_parts() else {
            return;
        };
        self.defer_multicol_positioned_fragment_element(
            element,
            signature,
            child.style.clone(),
            fragment,
        );
    }

    /// Queue a direct positioned descendant with a committed multicolumn
    /// fragment payload.
    ///
    /// Direct and flex-positioned children differ only in how they derive
    /// their source static rectangle. Once the containing multicolumn
    /// fragment is committed, both must use the same replay record so neither
    /// can accidentally bind to temporary column-page coordinates.
    /// <https://www.w3.org/TR/css-position-3/#static-position>
    /// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
    pub(in crate::layout) fn defer_multicol_positioned_fragment_element(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: ComputedStyle,
        fragment: PositionedFragmentReplay,
    ) {
        self.deferred_multicol_positioned_children
            .push(self.deferred_multicol_positioned_child(element, signature, style, fragment));
    }

    /// Remove scratch-column captures superseded by direct principal-box
    /// positioned records.
    ///
    /// Direct children are encountered once while the anonymous column flow
    /// is speculative and again when the multicol principal containing block
    /// is committed. The latter record is authoritative: retaining both
    /// duplicates the positioned principal, while retaining only the scratch
    /// record incorrectly clips it through anonymous columns. Descendants
    /// whose containing blocks live inside fragmented content have different
    /// element identities and remain queued.
    /// <https://drafts.csswg.org/css-multicol-2/#multi-column-model>
    /// <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
    pub(in crate::layout) fn remove_superseded_direct_multicol_positioned_captures(
        &mut self,
        start: usize,
        replacements: &[DeferredMulticolPositionedChild],
    ) {
        if start >= self.deferred_multicol_positioned_children.len() || replacements.is_empty() {
            return;
        }
        let replacement_ids = replacements
            .iter()
            .map(|child| child.element.id)
            .collect::<Vec<_>>();
        let mut captured = self.deferred_multicol_positioned_children.split_off(start);
        captured.retain(|child| !replacement_ids.contains(&child.element.id));
        self.deferred_multicol_positioned_children.extend(captured);
    }

    /// Capture a positioned principal whose containing block was established
    /// while multicolumn layout owns temporary fragmentainer pages.
    ///
    /// The frozen element/style identity and static rectangle survive the
    /// temporary layout transaction; replay later binds them to the committed
    /// projections of the actual containing-block scope instead of the page
    /// on which the speculative callback happened to run.
    pub(in crate::layout) fn capture_multicol_positioned_principal(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        source_static_rect: PositionedChildStaticRect,
    ) -> bool {
        if self.multicol_positioned_replay_capture_depth == 0 {
            return false;
        }
        let Some(containing_block_span_id) =
            self.active_multicol_positioned_containing_block_span_id(&style.position)
        else {
            // Only a positioned containing block that was itself captured by
            // the temporary multicolumn transaction may be replayed as an
            // independent positioned fragment.  In particular, opacity and
            // the other effect scopes establish stacking contexts without
            // establishing an absolute-position containing block.  Capturing
            // one of their descendants here would detach its paint from the
            // ancestor's compositing group.
            //
            // A genuine direct principal child is captured explicitly when
            // the multicol principal is committed.  All other descendants
            // remain in their captured ancestor paint/effect scope.
            // <https://drafts.csswg.org/css-multicol-2/#multi-column-model>
            // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
            // <https://drafts.csswg.org/css-color-4/#transparency>
            return false;
        };
        if self
            .deferred_multicol_positioned_children
            .iter()
            .any(|child| {
                child.element.id == element.id
                    && child.replay_context.containing_block_span_id
                        == Some(containing_block_span_id)
            })
        {
            return true;
        }
        let signature = self
            .ancestors
            .last()
            .cloned()
            .unwrap_or_else(|| element_signature(element));
        self.deferred_multicol_positioned_children.push(
            self.deferred_multicol_positioned_child(
                element,
                &signature,
                style.clone(),
                PositionedFragmentReplay::unfragmented(source_static_rect, None)
                    .across_committed_multicolumn_fragments(),
            ),
        );
        true
    }

    fn active_multicol_positioned_containing_block_span_id(
        &self,
        position: &Position,
    ) -> Option<u64> {
        if matches!(position, Position::Fixed) {
            self.active_multicol_positioned_containing_block_spans
                .iter()
                .rev()
                .find(|&&id| {
                    self.multicol_positioned_containing_block_spans
                        .iter()
                        .any(|span| span.id == id && span.mode.establishes_fixed_containing_block())
                })
                .copied()
        } else {
            self.active_multicol_positioned_containing_block_spans
                .last()
                .copied()
        }
    }

    /// Retain one committed multicolumn source slice for positioned records
    /// whose final owner cannot be selected from their static rectangle alone.
    ///
    /// The deferred replay lays such a child out once in source coordinates,
    /// then projects each intersecting positioned layer through these exact
    /// normal-flow slice mappings.
    /// <https://www.w3.org/TR/css-position-3/#static-position>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn retain_deferred_multicol_positioned_candidate(
        &mut self,
        start: usize,
        projection: FragmentainerProjection,
        source_block_start: LayoutLength,
        source_block_end: LayoutLength,
        continuous_source_to_local: PaintTranslation,
    ) {
        let candidate = PositionedFragmentCandidate::new(
            projection.source_clip(),
            source_block_start,
            source_block_end,
            continuous_source_to_local,
            projection.destination_translation(),
            projection.destination_clip(),
        );
        for child in self.deferred_multicol_positioned_children[start..].iter_mut() {
            child.fragment.add_unresolved_candidate(candidate);
        }
    }

    /// Commit every destination-local slice of one resolved positioned
    /// principal as one page-local stacking layer.
    ///
    /// Each layer retains its native effects and clip. A slice discriminator
    /// extends the page-local commit key, so normal speculative retries still
    /// coalesce while distinct source-to-destination continuations do not
    /// discard one another.
    /// <https://www.w3.org/TR/css-position-3/#fragmenting-absolutely-positioned-elements>
    /// <https://www.w3.org/TR/CSS22/zindex.html>
    fn append_multicol_positioned_candidate_layers(
        &mut self,
        fragment: &PositionedFragmentReplay,
        source_layers: Vec<PositionedPaintLayer>,
        monolithic_principal_paint: bool,
    ) {
        let candidates = fragment.candidates_extended_for_source_layers(
            fragment.candidates_for_owner(),
            &source_layers,
            &self.pages,
            &self.current_page,
        );
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            // Size containment makes the principal box monolithic. It is
            // assigned to its originating fragmentainer as one paint unit;
            // later candidate slices describe descendant fragmentation, not
            // independent clipped pieces of the contained principal.
            // <https://drafts.csswg.org/css-contain-1/#containment-size>
            // <https://www.w3.org/TR/css-break-3/#monolithic>
            if monolithic_principal_paint && candidate_index != 0 {
                break;
            }
            for layer in &source_layers {
                // Source replay can still reference recorded page operations.
                // Materialize those operations while their source page is
                // authoritative, before translating and clipping each
                // continuation. This makes the projection's primitive
                // geometry, rather than an unrelated later page operation,
                // the source of truth for slice-edge coverage.
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                let source_page = self
                    .pages
                    .get(layer.page_index)
                    .unwrap_or(&self.current_page);
                let source_context = layer.context.clone().into_primitive_nodes(source_page);
                let candidate_local_source = source_context
                    .clone()
                    .translated(candidate.continuous_source_to_local);
                if fragment.source_layers_select_candidates() {
                    match candidate_local_source.recorded_paint_bounds(source_page) {
                        Ok(Some(bounds)) if bounds.intersect(candidate.source_clip).is_none() => {
                            continue;
                        }
                        Ok(None) => continue,
                        // Transforms, masks, and other non-rectangular effects
                        // have indeterminate source bounds. Keep every candidate
                        // rather than risking discarded paint.
                        Ok(Some(_)) | Err(()) => {}
                    }
                }
                let mut projected_layer = layer.clone();
                projected_layer.context =
                    source_context.translated(candidate.continuous_source_to_destination());
                projected_layer.page_index = self.pages.len();
                let effective_clip = (!monolithic_principal_paint).then(|| {
                    projected_layer
                        .context
                        .effects
                        .overflow_clip_bounds()
                        .and_then(|existing| existing.intersect(candidate.destination_clip))
                        .unwrap_or(candidate.destination_clip)
                });
                // Materialized rectangular ink is cut to the projected
                // fragmentainer before serialization. The rectangular effect
                // below still clips complex and deferred ink, but trimming
                // simple primitives avoids a PDF clip-edge seam and gives a
                // continuation the same coverage as an independently painted
                // fragment.
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                if let Some(effective_clip) = effective_clip {
                    projected_layer.context = projected_layer
                        .context
                        .sliced_primitives_to_fragmentainer_rect(effective_clip);
                }
                // The source layer was captured while a scratch column page
                // was active. Its original paint cursor is consequently
                // older than normal-flow descendants committed into later
                // real columns. Reassign it at final replay so auto-level
                // positioned paint remains in the positioned phase of CSS
                // painting order, while stack-level ordering is unchanged.
                // <https://www.w3.org/TR/CSS22/zindex.html>
                projected_layer.context.source_order = self.next_paint_source_order();
                // Rectangles that were reduced above already have the exact
                // continuation edge. Do not wrap that narrow, fully
                // materialized case in a redundant PDF clip, because PDF clip
                // coverage differs from an independently painted rectangle.
                // Retain the clip for all existing effects, deferred
                // operations, paths, images, and nested contexts.
                if let Some(effective_clip) = effective_clip {
                    if projected_layer
                        .context
                        .can_elide_overflow_clip_after_materialization(effective_clip)
                    {
                        projected_layer.context.effects.overflow_clip_effect = None;
                    } else {
                        projected_layer
                            .context
                            .effects
                            .intersect_overflow_clip_bounds(effective_clip);
                    }
                }
                self.positioned_layers
                    .push(projected_layer.with_multicol_fragment_index(candidate_index));
            }
        }
    }

    /// Replay queued positioned descendants after the outermost multicolumn
    /// container has restored its principal containing-block context.
    ///
    /// A nested column set leaves its records queued for the outermost owner;
    /// replaying against an intermediate scratch page would reintroduce the
    /// coordinate and clipping error this queue avoids.
    pub(in crate::layout) fn replay_deferred_multicol_positioned_children(&mut self, start: usize) {
        if self.multicol_positioned_replay_capture_depth != 0
            || start >= self.deferred_multicol_positioned_children.len()
        {
            return;
        }
        debug_assert!(
            self.deferred_multicol_positioned_children[start..]
                .iter()
                .all(|child| {
                    child
                        .replay_context
                        .containing_block_span_id
                        .is_none_or(|id| {
                            self.multicol_positioned_containing_block_spans
                                .iter()
                                .any(|span| span.id == id)
                        })
                }),
            "deferred multicol positioned descendants must reference a durable containing-block span"
        );
        let deferred = self.deferred_multicol_positioned_children.split_off(start);
        for child in deferred {
            let monolithic_principal_paint =
                used_property_containment(&child.element, &child.style).size;
            let containing_block_span =
                child
                    .replay_context
                    .containing_block_span_id
                    .and_then(|id| {
                        self.multicol_positioned_containing_block_spans
                            .iter()
                            .find(|span| span.id == id)
                            .cloned()
                    });
            let static_rect = child.fragment.source_static_rect;
            let stylesheets = self.stylesheets;
            let child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                &child.element,
                &stylesheets,
                &child.style,
            );
            let replay_child = FormattingContextChild {
                kind: FormattingContextChildKind::Element {
                    element: &child.element,
                    signature: child.signature,
                    generated_pseudo: None,
                    children: Some(Cow::Owned(child_boxes)),
                    table_fragment: None,
                },
                style: child.style,
            };
            if let Some(span) = containing_block_span {
                // A positioned principal is laid out in the continuous source
                // containing block.  Its paint, rather than its fallback
                // static rectangle or the temporary column that happened to
                // discover it, determines which committed column slices own
                // the result.
                // <https://www.w3.org/TR/css-position-3/#fragmenting-absolutely-positioned-elements>
                let stylesheets = self.stylesheets;
                let source_layers = self.replay_deferred_multicol_source_layers(
                    &child.replay_context,
                    Some((span.mode, span.containing_block)),
                    &replay_child,
                    &stylesheets,
                    static_rect,
                );
                self.append_multicol_positioned_candidate_layers(
                    &child.fragment,
                    source_layers,
                    monolithic_principal_paint,
                );
                continue;
            }
            if child.fragment.localizes_static_rect_per_candidate() {
                for candidate in child.fragment.candidates_for_owner() {
                    let global_block_inset = matches!(
                        child.fragment.candidate_source_space,
                        PositionedCandidateSourceSpace::DefiniteBlockInsetGlobal
                    );
                    let containing_block = if global_block_inset {
                        child.fragment.source_containing_block()
                    } else {
                        child
                            .fragment
                            .containing_block_local_to_candidate(candidate)
                    };
                    let candidate_static_rect = if global_block_inset {
                        static_rect
                    } else {
                        static_rect.translated(PaintTranslation::new(
                            0.0,
                            candidate.source_block_start.points(),
                        ))
                    };
                    let stylesheets = self.stylesheets;
                    let source_layers = self.replay_deferred_multicol_source_layers(
                        &child.replay_context,
                        containing_block,
                        &replay_child,
                        &stylesheets,
                        candidate_static_rect,
                    );
                    for mut layer in source_layers {
                        // Positioned layout produces source-coordinate ink.
                        // A global definite inset retains its original source
                        // block offset until this point, so its source slice
                        // must advance to the committed destination block
                        // start. Translating the static rectangle instead
                        // would move a later fragment's `top` inset below its
                        // own clip.
                        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                        // <https://www.w3.org/TR/css-position-3/#static-position>
                        layer = layer.translated(candidate.source_to_destination);
                        layer.page_index = self.pages.len();
                        let destination_clip = layer
                            .context
                            .effects
                            .overflow_clip_bounds()
                            .and_then(|existing| existing.intersect(candidate.destination_clip))
                            .unwrap_or(candidate.destination_clip);
                        layer
                            .context
                            .effects
                            .intersect_overflow_clip_bounds(destination_clip);
                        self.positioned_layers.push(layer);
                    }
                }
                continue;
            }
            let stylesheets = self.stylesheets;
            let source_layers = self.replay_deferred_multicol_source_layers(
                &child.replay_context,
                child.fragment.positioning_containing_block,
                &replay_child,
                &stylesheets,
                static_rect,
            );
            if child.fragment.has_unresolved_candidates() {
                self.append_multicol_positioned_candidate_layers(
                    &child.fragment,
                    source_layers,
                    monolithic_principal_paint,
                );
            } else {
                self.positioned_layers.extend(source_layers);
            }
        }
    }

    /// Push the positioned-containing-block stacks established by one box.
    ///
    /// Callers retain ownership of the box geometry; this only centralizes the
    /// paired stack lifecycle required by CSS Positioned Layout and Transforms.
    /// <https://www.w3.org/TR/css-position-3/#def-cb> and
    /// <https://drafts.csswg.org/css-transforms-1/#transform-rendering>.
    pub(in crate::layout) fn push_positioned_containing_block(
        &mut self,
        mode: PositionedContainingBlockMode,
        containing_block: ContainingBlock,
    ) -> PositionedContainingBlockScope {
        let containing_block = self.positioned_containing_block_context(containing_block);
        let multicol_span_id = (self.multicol_positioned_replay_capture_depth > 0).then(|| {
            let id = self.next_multicol_positioned_containing_block_span_id;
            self.next_multicol_positioned_containing_block_span_id += 1;
            self.multicol_positioned_containing_block_spans.push(
                MulticolPositionedContainingBlockSpan {
                    id,
                    mode,
                    containing_block: containing_block.geometry,
                },
            );
            id
        });
        let scope = PositionedContainingBlockScope {
            containing_blocks_depth: self.containing_blocks.len(),
            fixed_containing_blocks_depth: self.fixed_containing_blocks.len(),
            mode,
            multicol_span_id,
        };
        if let Some(id) = multicol_span_id {
            self.active_multicol_positioned_containing_block_spans
                .push(id);
        }
        self.containing_blocks.push(containing_block);
        if mode.establishes_fixed_containing_block() {
            self.fixed_containing_blocks.push(containing_block);
        }
        scope
    }

    /// Restore the positioned-containing-block stacks recorded by `scope`.
    ///
    /// The depth assertions make leaked nested scopes visible at their owning
    /// layout boundary rather than silently popping an ancestor's geometry.
    pub(in crate::layout) fn pop_positioned_containing_block(
        &mut self,
        scope: PositionedContainingBlockScope,
    ) {
        debug_assert_eq!(
            self.containing_blocks.len(),
            scope.containing_blocks_depth + 1,
            "positioned containing-block scopes must be popped in nesting order",
        );
        debug_assert_eq!(
            self.fixed_containing_blocks.len(),
            scope.fixed_containing_blocks_depth
                + usize::from(scope.mode.establishes_fixed_containing_block()),
            "fixed containing-block scopes must be popped in nesting order",
        );
        if scope.mode.establishes_fixed_containing_block() {
            self.fixed_containing_blocks.pop();
        }
        self.containing_blocks.pop();
        if let Some(id) = scope.multicol_span_id {
            debug_assert_eq!(
                self.active_multicol_positioned_containing_block_spans.pop(),
                Some(id),
                "multicol positioned-containing-block spans must be popped in nesting order",
            );
        }
        debug_assert_eq!(self.containing_blocks.len(), scope.containing_blocks_depth,);
        debug_assert_eq!(
            self.fixed_containing_blocks.len(),
            scope.fixed_containing_blocks_depth,
        );
    }

    /// Replace the geometry of the innermost positioned containing block
    /// after its auto-sized principal flow has committed.
    ///
    /// Atomic formatting contexts initially need a containing block while
    /// collecting descendants, but their final padding-box height is only
    /// known after principal-flow layout.  Updating both stacks together
    /// keeps fixed descendants viewport-owned for `AbsoluteOnly` scopes.
    /// <https://www.w3.org/TR/css-position-3/#def-cb>
    pub(in crate::layout) fn finalize_positioned_containing_block(
        &mut self,
        scope: PositionedContainingBlockScope,
        containing_block: ContainingBlock,
    ) {
        debug_assert_eq!(
            self.containing_blocks.len(),
            scope.containing_blocks_depth + 1,
            "positioned containing-block scopes must be finalized in nesting order",
        );
        self.containing_blocks
            .last_mut()
            .expect("positioned containing-block scope has one absolute stack entry")
            .geometry = containing_block;
        if scope.mode.establishes_fixed_containing_block() {
            debug_assert_eq!(
                self.fixed_containing_blocks.len(),
                scope.fixed_containing_blocks_depth + 1,
                "fixed containing-block scope must match absolute scope",
            );
            self.fixed_containing_blocks
                .last_mut()
                .expect("fixed containing-block scope has one fixed stack entry")
                .geometry = containing_block;
        }
        if let Some(id) = scope.multicol_span_id
            && let Some(span) = self
                .multicol_positioned_containing_block_spans
                .iter_mut()
                .find(|span| span.id == id)
        {
            span.containing_block = containing_block;
        }
    }

    /// Replay an absolutely positioned flex/grid child from a precomputed
    /// static-position rectangle.
    ///
    /// CSS Flexbox and CSS Grid compute different hypothetical positions for
    /// out-of-flow children, but both replay the same child under that temporary
    /// static-position geometry:
    /// <https://www.w3.org/TR/css-flexbox-1/#abspos-items> and
    /// <https://www.w3.org/TR/css-grid-1/#abspos-items>.
    pub(in crate::layout) fn layout_positioned_formatting_context_child(
        &mut self,
        child: &FormattingContextChild<'_>,
        stylesheets: &Stylesheets<'_>,
        static_rect: PositionedChildStaticRect,
    ) {
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_absolute_static_position = self.absolute_static_position;
        let pushed_containing_block = if let Some(containing_block) = static_rect.containing_block {
            let context = self.positioned_containing_block_context(containing_block);
            self.containing_blocks.push(context);
            true
        } else {
            false
        };

        self.content_left = static_rect.left;
        self.content_right = static_rect.layout_right();
        self.cursor_y = static_rect.top;
        let static_position = AbsoluteStaticPosition::from_page_rect_with_horizontal_outside(
            static_rect.left,
            static_rect.right,
            static_rect.top,
            true,
        );
        self.absolute_static_position = Some(match static_rect.static_alignment {
            Some(static_alignment) => static_position.with_static_alignment(static_alignment),
            None => static_position,
        });

        let mut positioned_style = child.style.clone();
        if positioned_style.display.is_inline_level() {
            positioned_style.display = positioned_style.display.blockified();
        }

        if let Some((child_element, signature, child_boxes)) = child.element_parts() {
            self.push_ancestor_signature(signature.clone());
            self.layout_element_with_child_boxes(
                child_element,
                &positioned_style,
                stylesheets,
                child_boxes,
            );
            self.ancestors.pop();
        }

        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        self.absolute_static_position = previous_absolute_static_position;
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_layout_builder<'a, Collection: crate::css::StylesheetCollection + ?Sized>(
        options: &'a RenderOptions,
        stylesheets: &'a Collection,
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
        let stylesheets = crate::css::StylesheetCollection::stylesheet_view(stylesheets);
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
            target_references: crate::layout::TargetReferenceSnapshot::default(),
            font_system: FontSystem::new(),
        })
    }

    #[test]
    fn positioned_containing_block_mode_follows_positioning_and_effects() {
        let mut style = ComputedStyle::initial();
        assert_eq!(PositionedContainingBlockMode::for_style(&style), None);

        style.position = Position::Relative;
        assert_eq!(
            PositionedContainingBlockMode::for_style(&style),
            Some(PositionedContainingBlockMode::AbsoluteOnly),
        );

        style.position = Position::Absolute;
        assert_eq!(
            PositionedContainingBlockMode::for_style(&style),
            Some(PositionedContainingBlockMode::AbsoluteOnly),
        );

        style.position = Position::Fixed;
        assert_eq!(
            PositionedContainingBlockMode::for_style(&style),
            Some(PositionedContainingBlockMode::AbsoluteOnly),
        );

        style.position = Position::Static;
        style.contain.layout = true;
        assert_eq!(
            PositionedContainingBlockMode::for_style(&style),
            Some(PositionedContainingBlockMode::FixedAndAbsolute),
        );
    }

    #[test]
    fn positioned_containing_block_scope_restores_both_stack_variants() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(10.0, 20.0, 30.0, 40.0));

        let initial_containing_blocks = builder.containing_blocks.len();
        let initial_fixed_containing_blocks = builder.fixed_containing_blocks.len();
        let absolute_scope = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::AbsoluteOnly,
            containing_block,
        );
        assert_eq!(
            builder.containing_blocks.len(),
            initial_containing_blocks + 1
        );
        assert_eq!(
            builder.fixed_containing_blocks.len(),
            initial_fixed_containing_blocks
        );
        builder.pop_positioned_containing_block(absolute_scope);
        assert_eq!(builder.containing_blocks.len(), initial_containing_blocks);
        assert_eq!(
            builder.fixed_containing_blocks.len(),
            initial_fixed_containing_blocks
        );

        let fixed_scope = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::FixedAndAbsolute,
            containing_block,
        );
        assert_eq!(
            builder.containing_blocks.len(),
            initial_containing_blocks + 1
        );
        assert_eq!(
            builder.fixed_containing_blocks.len(),
            initial_fixed_containing_blocks + 1
        );
        builder.pop_positioned_containing_block(fixed_scope);
        assert_eq!(builder.containing_blocks.len(), initial_containing_blocks);
        assert_eq!(
            builder.fixed_containing_blocks.len(),
            initial_fixed_containing_blocks
        );
    }

    #[test]
    fn positioned_containing_blocks_inherit_atomic_coordinate_space_provenance() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let outer_geometry =
            ContainingBlock::from_page_top_rect(PageTopRect::new(10.0, 20.0, 30.0, 40.0));
        let nested_geometry =
            ContainingBlock::from_page_top_rect(PageTopRect::new(50.0, 60.0, 70.0, 80.0));

        let page_scope = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::AbsoluteOnly,
            outer_geometry,
        );
        assert_eq!(
            builder.containing_blocks.last().unwrap().coordinate_space,
            PositionedCoordinateSpace::Page
        );
        builder.pop_positioned_containing_block(page_scope);

        let outer_space = AtomicInlineCoordinateSpaceId::new(41);
        builder
            .active_atomic_inline_coordinate_spaces
            .push(outer_space);
        let outer_scope = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::FixedAndAbsolute,
            outer_geometry,
        );
        let nested_scope = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::AbsoluteOnly,
            nested_geometry,
        );
        assert_eq!(
            builder.containing_blocks.last().unwrap().coordinate_space,
            PositionedCoordinateSpace::AtomicInline(outer_space)
        );
        assert_eq!(
            builder
                .fixed_containing_blocks
                .last()
                .unwrap()
                .coordinate_space,
            PositionedCoordinateSpace::AtomicInline(outer_space)
        );

        builder.pop_positioned_containing_block(nested_scope);
        builder.pop_positioned_containing_block(outer_scope);
        builder.active_atomic_inline_coordinate_spaces.pop();

        let inner_space = AtomicInlineCoordinateSpaceId::new(42);
        builder
            .active_atomic_inline_coordinate_spaces
            .extend([outer_space, inner_space]);
        let inner_scope = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::AbsoluteOnly,
            nested_geometry,
        );
        assert_eq!(
            builder.containing_blocks.last().unwrap().coordinate_space,
            PositionedCoordinateSpace::AtomicInline(inner_space)
        );
        builder.pop_positioned_containing_block(inner_scope);
    }

    #[test]
    fn atomic_inline_formatting_context_scope_nests_and_restores_state() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let initial_axes = WritingModeAxes::new(
            builder.containing_block_writing_mode,
            builder.containing_block_direction,
        );
        let initial_static_depth = builder.static_position_containing_blocks.len();
        let initial_atomic_depth = builder.active_atomic_inline_coordinate_spaces.len();

        let mut outer_style = ComputedStyle::initial();
        outer_style.writing_mode = WritingMode::SidewaysRl;
        outer_style.direction = Direction::Rtl;
        let outer_rect = PageTopRect::new(7.0, 91.0, 31.0, 47.0);
        let outer = builder.begin_atomic_inline_formatting_context(&outer_style, outer_rect);
        assert_eq!(
            builder.active_atomic_inline_coordinate_spaces.last(),
            Some(&outer.coordinate_space)
        );
        let outer_context = builder.static_position_containing_blocks.last().unwrap();
        assert_eq!(outer_context.content_rect, outer_rect);
        assert_eq!(
            outer_context.axes,
            WritingModeAxes::new(WritingMode::SidewaysRl, Direction::Rtl)
        );

        let mut inner_style = ComputedStyle::initial();
        inner_style.writing_mode = WritingMode::SidewaysLr;
        let inner = builder.begin_atomic_inline_formatting_context(
            &inner_style,
            PageTopRect::new(13.0, 73.0, 19.0, 29.0),
        );
        assert_ne!(inner.coordinate_space, outer.coordinate_space);
        builder.end_atomic_inline_formatting_context(inner);
        assert_eq!(
            WritingModeAxes::new(
                builder.containing_block_writing_mode,
                builder.containing_block_direction,
            ),
            WritingModeAxes::new(WritingMode::SidewaysRl, Direction::Rtl)
        );
        assert_eq!(
            builder.active_atomic_inline_coordinate_spaces.last(),
            Some(&outer.coordinate_space)
        );

        builder.end_atomic_inline_formatting_context(outer);
        assert_eq!(
            builder.static_position_containing_blocks.len(),
            initial_static_depth
        );
        assert_eq!(
            builder.active_atomic_inline_coordinate_spaces.len(),
            initial_atomic_depth
        );
        assert_eq!(
            WritingModeAxes::new(
                builder.containing_block_writing_mode,
                builder.containing_block_direction,
            ),
            initial_axes
        );

        let snapshot = builder.snapshot();
        let first_retry = builder.begin_atomic_inline_formatting_context(&outer_style, outer_rect);
        let first_retry_id = first_retry.coordinate_space;
        builder.end_atomic_inline_formatting_context(first_retry);
        builder.restore(snapshot);
        let second_retry = builder.begin_atomic_inline_formatting_context(&outer_style, outer_rect);
        assert_ne!(second_retry.coordinate_space, first_retry_id);
        builder.end_atomic_inline_formatting_context(second_retry);
    }

    #[test]
    fn deferred_multicol_replay_context_restores_captured_clip_chain_and_parent_state() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let source_clip = OverflowClip::from_paint_rect_with_axes_and_non_scrollable(
            paint_space_rect(1.0, 2.0, 30.0, 40.0),
            true,
            true,
            false,
            true,
        );
        let enclosing_clip = OverflowClip::from_paint_rect(paint_space_rect(5.0, 6.0, 7.0, 8.0));
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(10.0, 20.0, 30.0, 40.0));

        builder.overflow_clips = vec![source_clip];
        let clipped_context = builder.deferred_multicol_replay_context(&Position::Absolute);
        builder.overflow_clips = vec![enclosing_clip];
        let initial_containing_block_depth = builder.containing_blocks.len();
        let observed = builder.with_deferred_multicol_replay_context(
            &clipped_context,
            Some((
                PositionedContainingBlockMode::AbsoluteOnly,
                containing_block,
            )),
            |layout| {
                (
                    layout.overflow_clips.clone(),
                    layout.containing_blocks.len(),
                )
            },
        );
        assert_eq!(observed.0, vec![source_clip]);
        assert_eq!(observed.1, initial_containing_block_depth + 1);
        assert_eq!(builder.overflow_clips, vec![enclosing_clip]);
        assert_eq!(
            builder.containing_blocks.len(),
            initial_containing_block_depth
        );

        builder.overflow_clips.clear();
        let empty_context = builder.deferred_multicol_replay_context(&Position::Absolute);
        builder.overflow_clips = vec![enclosing_clip];
        let observed_empty =
            builder.with_deferred_multicol_replay_context(&empty_context, None, |layout| {
                layout.overflow_clips.clone()
            });
        assert!(observed_empty.is_empty());
        assert_eq!(builder.overflow_clips, vec![enclosing_clip]);
    }

    #[test]
    fn multicol_containing_block_spans_survive_snapshot_restore_without_paint() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.multicol_positioned_replay_capture_depth = 1;
        let snapshot = builder.snapshot();
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(10.0, 20.0, 30.0, 40.0));
        let initial_positioned_layers = builder.positioned_layers.len();
        let outer = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::AbsoluteOnly,
            containing_block,
        );
        let inner = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::FixedAndAbsolute,
            containing_block,
        );
        assert_ne!(outer.multicol_span_id, inner.multicol_span_id);
        assert_eq!(
            builder.active_multicol_positioned_containing_block_spans,
            vec![
                outer.multicol_span_id.unwrap(),
                inner.multicol_span_id.unwrap()
            ],
        );
        builder.pop_positioned_containing_block(inner);
        builder.pop_positioned_containing_block(outer);

        builder.restore(snapshot);
        assert_eq!(builder.multicol_positioned_containing_block_spans.len(), 2);
        assert!(
            builder
                .active_multicol_positioned_containing_block_spans
                .is_empty()
        );
        assert_eq!(builder.positioned_layers.len(), initial_positioned_layers);
    }

    #[test]
    fn unresolved_positioned_replay_retains_distinct_multicol_candidates() {
        let mut replay = PositionedFragmentReplay::unfragmented(
            PositionedChildStaticRect::new(1.0, 4.0, 3.0),
            None,
        )
        .resolving_owner_from_final_block_inset(Some(layout_pt(55.0)));
        replay.add_unresolved_candidate(PositionedFragmentCandidate::new(
            PaintClip::new(0.0, 100.0, 20.0, 40.0),
            layout_pt(0.0),
            layout_pt(40.0),
            PaintTranslation::identity(),
            PaintTranslation::new(30.0, -20.0),
            PaintClip::new(30.0, 80.0, 20.0, 40.0),
        ));
        replay.add_unresolved_candidate(PositionedFragmentCandidate::new(
            PaintClip::new(0.0, 100.0, 20.0, 40.0),
            layout_pt(0.0),
            layout_pt(40.0),
            PaintTranslation::identity(),
            PaintTranslation::new(30.0, -20.0),
            PaintClip::new(30.0, 80.0, 20.0, 40.0),
        ));
        replay.add_unresolved_candidate(PositionedFragmentCandidate::new(
            PaintClip::new(0.0, 140.0, 20.0, 40.0),
            layout_pt(40.0),
            layout_pt(80.0),
            PaintTranslation::new(0.0, 40.0),
            PaintTranslation::new(60.0, -40.0),
            PaintClip::new(60.0, 100.0, 20.0, 40.0),
        ));

        assert!(replay.has_unresolved_candidates());
        assert_eq!(replay.unresolved_candidates.len(), 2);
        assert_eq!(
            replay.unresolved_candidates[0].destination_clip,
            PaintClip::new(30.0, 80.0, 20.0, 40.0),
        );
        assert_eq!(
            replay.candidates_for_owner(),
            vec![replay.unresolved_candidates[1]],
            "a resolved top inset selects its owning source fragmentainer interval",
        );
    }

    #[test]
    fn source_static_interval_selects_each_intersected_multicol_candidate() {
        let mut replay = PositionedFragmentReplay::unfragmented(
            PositionedChildStaticRect::new(1.0, 4.0, 3.0),
            None,
        )
        .resolving_owner_from_source_block_interval(layout_pt(35.0), layout_pt(85.0));
        for (start, end) in [(0.0, 40.0), (40.0, 80.0), (80.0, 120.0)] {
            replay.add_unresolved_candidate(PositionedFragmentCandidate::new(
                PaintClip::new(start, 100.0, 20.0, 40.0),
                layout_pt(start),
                layout_pt(end),
                PaintTranslation::identity(),
                PaintTranslation::identity(),
                PaintClip::new(start, 100.0, 20.0, 40.0),
            ));
        }

        assert_eq!(
            replay.candidates_for_owner(),
            replay.unresolved_candidates[..3],
            "a column-axis flex static rectangle retains every intersected source fragment",
        );
    }

    #[test]
    fn positioned_principal_replays_through_every_committed_multicol_candidate() {
        let mut replay = PositionedFragmentReplay::unfragmented(
            PositionedChildStaticRect::new(1.0, 4.0, 3.0),
            None,
        )
        .across_committed_multicolumn_fragments();
        for (start, end) in [(0.0, 40.0), (40.0, 80.0)] {
            replay.add_unresolved_candidate(PositionedFragmentCandidate::new(
                PaintClip::new(0.0, start, 20.0, end - start),
                layout_pt(start),
                layout_pt(end),
                PaintTranslation::new(0.0, start),
                PaintTranslation::identity(),
                PaintClip::new(0.0, start, 20.0, end - start),
            ));
        }

        assert_eq!(replay.candidates_for_owner(), replay.unresolved_candidates);
    }

    #[test]
    fn definite_block_inset_marks_candidate_layers_as_source_global() {
        let local = PositionedFragmentReplay::unfragmented(
            PositionedChildStaticRect::new(1.0, 4.0, 3.0),
            None,
        )
        .resolving_owner_from_source_block_interval(layout_pt(0.0), layout_pt(40.0));
        let global = local.clone().with_definite_block_inset_source_coordinates();
        assert_eq!(
            local.candidate_source_space,
            PositionedCandidateSourceSpace::StaticPositionLocal
        );
        assert_eq!(
            global.candidate_source_space,
            PositionedCandidateSourceSpace::DefiniteBlockInsetGlobal
        );
    }

    #[test]
    fn continuous_source_projection_localizes_before_destination_translation() {
        let candidate = PositionedFragmentCandidate::new(
            PaintClip::new(0.0, 100.0, 20.0, 40.0),
            layout_pt(40.0),
            layout_pt(80.0),
            PaintTranslation::new(0.0, 40.0),
            PaintTranslation::new(30.0, -20.0),
            PaintClip::new(30.0, 80.0, 20.0, 40.0),
        );
        assert_eq!(
            candidate.continuous_source_to_destination(),
            PaintTranslation::new(30.0, 20.0),
            "continuous positioned paint localizes itself to each source column slice",
        );
    }

    #[test]
    fn projected_candidate_continuation_preserves_source_local_clip_and_step() {
        let first = PositionedFragmentCandidate::new(
            PaintClip::new(4.0, 100.0, 20.0, 40.0),
            layout_pt(0.0),
            layout_pt(40.0),
            PaintTranslation::identity(),
            PaintTranslation::new(10.0, -5.0),
            PaintClip::new(14.0, 95.0, 20.0, 40.0),
        );
        let second = PositionedFragmentCandidate::new(
            first.source_clip,
            layout_pt(40.0),
            layout_pt(80.0),
            PaintTranslation::new(0.0, 40.0),
            PaintTranslation::new(30.0, -5.0),
            PaintClip::new(34.0, 95.0, 20.0, 40.0),
        );

        let continuation = second.continuation_after(first);
        assert_eq!(continuation.source_clip, first.source_clip);
        assert_eq!(continuation.source_block_start, layout_pt(80.0));
        assert_eq!(continuation.source_block_end, layout_pt(120.0));
        assert_eq!(
            continuation.continuous_source_to_local,
            PaintTranslation::new(0.0, 80.0)
        );
        assert_eq!(
            continuation.destination_clip,
            PaintClip::new(54.0, 95.0, 20.0, 40.0)
        );
    }
}
