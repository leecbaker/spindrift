use super::*;
use crate::layout::block::suppress_fragmented_box_edges;
use crate::layout::builder::page_for_context;
use std::collections::{BTreeMap, HashSet};

/// A page index in the document's final page sequence.
///
/// Positioned layout first produces paint in scratch fragmentainers, then
/// assigns that paint to this final sequence. Keeping the two index spaces
/// distinct prevents an unremapped scratch fragment from being committed to a
/// document page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::layout) struct DocumentPageIndex(usize);

impl DocumentPageIndex {
    pub(in crate::layout) const fn new(index: usize) -> Self {
        Self(index)
    }

    pub(in crate::layout) const fn get(self) -> usize {
        self.0
    }
}

/// An ordinal in a conceptual fragmentation sequence.
///
/// This deliberately differs from [`FragmentainerCount`]: adding a count to
/// an ordinal selects a fragmentainer, while adding two ordinals is never a
/// meaningful layout operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::layout) struct FragmentainerOrdinal(usize);

impl FragmentainerOrdinal {
    pub(in crate::layout) const fn new(value: usize) -> Self {
        Self(value)
    }

    pub(in crate::layout) const fn get(self) -> usize {
        self.0
    }
}

/// The number of fragmentainers in one logical run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct FragmentainerCount(usize);

impl FragmentainerCount {
    pub(in crate::layout) const fn new(value: usize) -> Self {
        Self(value)
    }

    pub(in crate::layout) const fn get(self) -> usize {
        self.0
    }
}

/// Physical margin-box geometry interpreted along a fragmentainer's logical
/// block axis.
///
/// CSS Positioned Layout resolves the physical inset properties against the
/// continuous containing block. CSS Fragmentation subsequently assigns that
/// already-resolved physical box to fragmentainers using the principal
/// flow's block direction. Keeping those two steps separate prevents a
/// vertical principal flow from treating the physical Y axis as its page
/// progression axis.
/// <https://drafts.csswg.org/css-position-3/#abspos-insets>
/// <https://drafts.csswg.org/css-break-4/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentainerBlockMarginBox {
    physical: PageTopRect,
}

impl FragmentainerBlockMarginBox {
    pub(in crate::layout) fn new(physical: PageTopRect) -> Self {
        Self { physical }
    }

    /// Distance from a fragmentainer block-start edge to this margin box's
    /// block-start edge, measured in the direction of block progression.
    pub(in crate::layout) fn start_distance_from(
        self,
        fragmentainer_block_start: f32,
        block_start_side: PhysicalSide,
    ) -> f32 {
        match block_start_side {
            PhysicalSide::Top => fragmentainer_block_start - self.physical.top_y(),
            PhysicalSide::Bottom => self.physical.bottom_y() - fragmentainer_block_start,
            PhysicalSide::Left => self.physical.x() - fragmentainer_block_start,
            PhysicalSide::Right => {
                fragmentainer_block_start - (self.physical.x() + self.physical.width())
            }
        }
    }

    pub(in crate::layout) fn block_extent(self, block_start_side: PhysicalSide) -> LayoutLength {
        match block_start_side {
            PhysicalSide::Top | PhysicalSide::Bottom => layout_pt(self.physical.height()),
            PhysicalSide::Left | PhysicalSide::Right => layout_pt(self.physical.width()),
        }
    }
}

/// Physical page-area edge corresponding to the fragmentainer block start.
///
/// This maps only the fragmentainer coordinate system; physical inset
/// properties and overflow rectangles keep their CSS physical meaning.
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
fn fragmentainer_block_start_for_page(context: PageContext, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => context.top(),
        PhysicalSide::Bottom => context.bottom(),
        PhysicalSide::Left => context.left(),
        PhysicalSide::Right => context.right(),
    }
}

/// A consecutive conceptual run of equal-sized fragmentainers.
///
/// The tail of a clipped positioned box must retain its logical extent for
/// multicolumn ordering and balancing even though no PDF page or paint
/// fragment is allocated for it.
/// <https://drafts.csswg.org/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct LogicalFragmentainerSpan {
    pub(in crate::layout) start: FragmentainerOrdinal,
    pub(in crate::layout) count: FragmentainerCount,
    pub(in crate::layout) fragmentainer_block_size: LayoutLength,
    pub(in crate::layout) final_fragment_used_block_size: LayoutLength,
}

impl LogicalFragmentainerSpan {
    fn through(
        start: FragmentainerOrdinal,
        end: FragmentainerOrdinal,
        fragmentainer_block_size: LayoutLength,
    ) -> Self {
        debug_assert!(end >= start);
        Self {
            start,
            count: FragmentainerCount::new(end.get().saturating_sub(start.get()) + 1),
            fragmentainer_block_size,
            final_fragment_used_block_size: fragmentainer_block_size,
        }
    }

    fn end(self) -> FragmentainerOrdinal {
        FragmentainerOrdinal::new(
            self.start
                .get()
                .saturating_add(self.count.get().saturating_sub(1)),
        )
    }
}

/// Owned materialized fragments plus an optional conceptual continuation.
///
/// `T` is intentionally generic so paint capture and continuation planning
/// share the same ownership boundary without allowing the logical tail to
/// acquire a cloned paint payload.
#[derive(Debug)]
pub(in crate::layout) struct MaterializedFragmentPrefix<T> {
    fragments: Vec<T>,
    unmaterialized_tail: Option<LogicalFragmentainerSpan>,
}

impl<T> MaterializedFragmentPrefix<T> {
    fn new(fragments: Vec<T>) -> Self {
        Self {
            fragments,
            unmaterialized_tail: None,
        }
    }

    fn into_fragments(self) -> Vec<T> {
        debug_assert!(
            self.unmaterialized_tail.is_none(),
            "paint capture must not consume an unmaterialized logical tail"
        );
        self.fragments
    }
}

/// The only positioned-paint reach that can suppress a conceptual tail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum PositionedPaintReach {
    PotentiallyVisible,
    Clipped { clip: PaintClip },
}

impl PositionedPaintReach {
    /// Determine whether a non-scrollable overflow clip proves that later
    /// positioned fragments cannot contribute to static output.
    ///
    /// The relevant clip edge is selected in the destination fragmentainer's
    /// logical block direction. Overflow rectangles themselves remain
    /// physical, as required by CSS Overflow and CSS Writing Modes.
    /// <https://drafts.csswg.org/css-overflow-3/#overflow-areas>
    /// <https://drafts.csswg.org/css-writing-modes-4/#physical-mapping>
    pub(in crate::layout) fn from_overflow_clips(
        clips: &[OverflowClip],
        fragmentainer_axes: FlowAxes,
    ) -> Self {
        let block_start = fragmentainer_axes.block_start_side();
        clips
            .iter()
            .filter(|clip| match block_start {
                PhysicalSide::Top | PhysicalSide::Bottom => clip.clips_y && clip.non_scrollable_y,
                PhysicalSide::Left | PhysicalSide::Right => clip.clips_x && clip.non_scrollable_x,
            })
            .reduce(|nearest, candidate| {
                let candidate_is_nearer = match block_start {
                    // Paint-space Y grows upward, whereas logical block
                    // progress from a top edge grows downward.
                    PhysicalSide::Top => candidate.rect.origin.y > nearest.rect.origin.y,
                    PhysicalSide::Bottom => {
                        candidate.rect.origin.y + candidate.rect.size.height
                            < nearest.rect.origin.y + nearest.rect.size.height
                    }
                    PhysicalSide::Left => {
                        candidate.rect.origin.x + candidate.rect.size.width
                            < nearest.rect.origin.x + nearest.rect.size.width
                    }
                    PhysicalSide::Right => candidate.rect.origin.x > nearest.rect.origin.x,
                };
                if candidate_is_nearer {
                    candidate
                } else {
                    nearest
                }
            })
            .map(|clip| Self::Clipped {
                clip: PaintClip::from_paint_rect(clip.paint_rect()),
            })
            .unwrap_or(Self::PotentiallyVisible)
    }
}

/// Mapping from a positioned source fragmentainer sequence to document pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct FragmentainerProjection {
    source_start: FragmentainerOrdinal,
    destination_start: DocumentPageIndex,
}

impl FragmentainerProjection {
    fn for_document_page(destination_start: usize) -> Self {
        Self {
            source_start: FragmentainerOrdinal::new(0),
            destination_start: DocumentPageIndex::new(destination_start),
        }
    }
}

/// Resolved positioned fragmentation with separate logical and paintable ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PositionedFragmentationPlan {
    projection: FragmentainerProjection,
    logical_span: Option<LogicalFragmentainerSpan>,
    paint_reach: PositionedPaintReach,
    materialized_destination_end: Option<DocumentPageIndex>,
}

impl PositionedFragmentationPlan {
    pub(in crate::layout) fn for_absolute_box(
        destination_start: usize,
        logical_destination_end: Option<usize>,
        fragmentainer_block_start: f32,
        fragmentainer_block_start_side: PhysicalSide,
        page_block_size: LayoutLength,
        paint_reach: PositionedPaintReach,
    ) -> Self {
        let projection = FragmentainerProjection::for_document_page(destination_start);
        let logical_span = logical_destination_end.map(|end| {
            LogicalFragmentainerSpan::through(
                FragmentainerOrdinal::new(destination_start),
                FragmentainerOrdinal::new(end.max(destination_start)),
                page_block_size,
            )
        });
        let materialized_destination_end = logical_destination_end.map(|end| {
            let clipped_end = match paint_reach {
                PositionedPaintReach::PotentiallyVisible => end,
                PositionedPaintReach::Clipped { clip } => {
                    let visible_distance = match fragmentainer_block_start_side {
                        PhysicalSide::Top => fragmentainer_block_start - clip.y(),
                        PhysicalSide::Bottom => {
                            clip.y() + clip.height() - fragmentainer_block_start
                        }
                        PhysicalSide::Left => clip.x() + clip.width() - fragmentainer_block_start,
                        PhysicalSide::Right => fragmentainer_block_start - clip.x(),
                    }
                    .max(0.0);
                    let visible_offset = ((visible_distance - 0.01).max(0.0)
                        / page_block_size.points().max(1.0))
                    .floor() as usize;
                    end.min(destination_start.saturating_add(visible_offset))
                }
            };
            DocumentPageIndex::new(clipped_end)
        });
        Self {
            projection,
            logical_span,
            paint_reach,
            materialized_destination_end,
        }
    }

    pub(in crate::layout) fn materialized_destination_end(self) -> Option<usize> {
        self.materialized_destination_end
            .map(DocumentPageIndex::get)
    }

    /// The scratch page sequence may be bounded only when an ancestor's
    /// non-scrollable overflow clip proves later positioned fragments cannot
    /// contribute to static PDF output.
    pub(in crate::layout) fn scratch_page_limit(self) -> Option<usize> {
        matches!(self.paint_reach, PositionedPaintReach::Clipped { .. })
            .then(|| {
                self.materialized_destination_end
                    .map(DocumentPageIndex::get)
            })
            .flatten()
            .map(|last_page| last_page.saturating_add(1))
    }

    pub(in crate::layout) fn logical_tail(self) -> Option<LogicalFragmentainerSpan> {
        let logical_span = self.logical_span?;
        let materialized_end = self.materialized_destination_end?;
        debug_assert_eq!(
            self.projection.source_start,
            FragmentainerOrdinal::new(0),
            "positioned page projection currently begins at the source fragmentainer"
        );
        (materialized_end.get() < logical_span.end().get()).then(|| {
            LogicalFragmentainerSpan::through(
                FragmentainerOrdinal::new(materialized_end.get() + 1),
                logical_span.end(),
                logical_span.fragmentainer_block_size,
            )
        })
    }

    pub(in crate::layout) fn with_materialized_destination_end(
        mut self,
        destination_end: Option<usize>,
    ) -> Self {
        let requested = destination_end.map(DocumentPageIndex::new);
        self.materialized_destination_end = match self.paint_reach {
            PositionedPaintReach::PotentiallyVisible => self
                .materialized_destination_end
                .into_iter()
                .chain(requested)
                .max(),
            PositionedPaintReach::Clipped { .. } => {
                match (self.materialized_destination_end, requested) {
                    (Some(bound), Some(requested)) => Some(bound.min(requested)),
                    (bound, None) => bound,
                    (None, requested) => requested,
                }
            }
        };
        self
    }

    pub(in crate::layout) fn with_observed_destination_end(
        mut self,
        destination_end: Option<usize>,
    ) -> Self {
        self.materialized_destination_end = destination_end.map(DocumentPageIndex::new);
        self
    }
}

/// Pending positioned output that keeps an unpaintable logical tail separate
/// from actual document pages awaiting materialization.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct PendingPositionedFragmentation {
    materialized_destination_end: Option<DocumentPageIndex>,
    logical_tail: Option<LogicalFragmentainerSpan>,
}

impl PendingPositionedFragmentation {
    pub(in crate::layout) fn record(&mut self, plan: PositionedFragmentationPlan) {
        self.materialized_destination_end = self
            .materialized_destination_end
            .into_iter()
            .chain(plan.materialized_destination_end)
            .max();
        self.logical_tail = self
            .logical_tail
            .into_iter()
            .chain(plan.logical_tail())
            .max_by_key(|span| span.end());
    }

    pub(in crate::layout) fn materialized_destination_end(self) -> Option<usize> {
        self.materialized_destination_end
            .map(DocumentPageIndex::get)
    }

    pub(in crate::layout) fn take_materialized_destination_end(&mut self) -> Option<usize> {
        self.materialized_destination_end
            .take()
            .map(DocumentPageIndex::get)
    }

    pub(in crate::layout) fn merge(&mut self, other: Self) {
        self.materialized_destination_end = self
            .materialized_destination_end
            .into_iter()
            .chain(other.materialized_destination_end)
            .max();
        self.logical_tail = self
            .logical_tail
            .into_iter()
            .chain(other.logical_tail)
            .max_by_key(|span| span.end());
    }
}

/// A page index in the temporary sequence used while laying out one
/// positioned subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::layout) struct ScratchPageIndex(usize);

impl ScratchPageIndex {
    fn new(index: usize) -> Self {
        Self(index)
    }

    fn get(self) -> usize {
        self.0
    }
}

/// Paint extracted from a positioned subtree's scratch fragmentainers.
///
/// This deliberately has no `Clone` implementation. It must be remapped to
/// a document page and consumed into one positioned layer before it can be
/// committed.
#[must_use]
pub(in crate::layout) struct ScratchPositionedFragment {
    scratch_page: ScratchPageIndex,
    fragment: PaintFragment,
}

/// Paint whose page ownership has been resolved, but which has not yet been
/// converted to a positioned stacking layer.
#[must_use]
pub(in crate::layout) struct FinalPositionedFragment {
    destination_page: DocumentPageIndex,
    fragment: PaintFragment,
}

impl FinalPositionedFragment {
    pub(in crate::layout) fn new(
        destination_page: DocumentPageIndex,
        fragment: PaintFragment,
    ) -> Self {
        Self {
            destination_page,
            fragment,
        }
    }

    pub(in crate::layout) fn destination_page(&self) -> DocumentPageIndex {
        self.destination_page
    }

    pub(in crate::layout) fn fragment(&self) -> &PaintFragment {
        &self.fragment
    }

    pub(in crate::layout) fn fragment_mut(&mut self) -> &mut PaintFragment {
        &mut self.fragment
    }

    pub(in crate::layout) fn links(&self) -> &[RenderedLink] {
        &self.fragment.links
    }

    pub(in crate::layout) fn is_empty(&self) -> bool {
        self.fragment.is_empty()
    }

    pub(in crate::layout) fn contains_overflow_clip(&self) -> bool {
        self.fragment.contains_overflow_clip()
    }

    pub(in crate::layout) fn with_monolithic_fragmentation_scope(
        mut self,
        bounds: PaintClip,
    ) -> Self {
        self.fragment = self.fragment.with_monolithic_fragmentation_scope(bounds);
        self
    }

    pub(in crate::layout) fn with_contents_clipped_to_rect(
        mut self,
        clip: PaintClip,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        self.fragment = self
            .fragment
            .with_contents_clipped_to_rect(clip, child_contexts);
        self
    }

    pub(in crate::layout) fn empty(destination_page: DocumentPageIndex) -> Self {
        Self::new(
            destination_page,
            PaintFragment::from_primitives(Vec::new(), Vec::new()),
        )
    }

    /// Consume final-page paint into its only page-local stacking-layer
    /// representation. The underlying `PaintFragment` never leaves this
    /// ownership boundary after final-page remapping.
    pub(in crate::layout) fn into_page_local_layer(
        self,
        identity: PositionedPaintIdentity,
        stacking: PositionedStackingMetadata,
    ) -> PendingPageLocalLayer {
        let context = PaintStackingContext::from_banded_fragment_with_stack_level(
            stacking.stack_level,
            self.fragment,
            stacking.child_contexts,
        )
        .with_source_order(stacking.source_order)
        .with_effects(stacking.effects)
        .with_bounds(stacking.bounds);
        PendingPageLocalLayer::new(
            self.destination_page,
            identity,
            context,
            PageLocalLayerMetadata {
                transaction_depth: stacking.transaction_depth,
                source_is_target: stacking.source_is_target,
                stack_level: stacking.stack_level,
                links: stacking.links,
                escaped_atom_translation: stacking.escaped_atom_translation,
            },
        )
    }

    /// Fixed paint is intentionally replayable: it is retained as a
    /// viewport-relative layer and borrowed once for each final page instead
    /// of entering the single-use page-local layer queue.
    pub(in crate::layout) fn into_viewport_fixed_layer(
        self,
        source_element: crate::dom::ElementId,
        source_style: ComputedStyle,
        stacking: PositionedStackingMetadata,
    ) -> FixedPaintLayer {
        let context = PaintStackingContext::from_banded_fragment_with_stack_level(
            stacking.stack_level,
            self.fragment,
            stacking.child_contexts,
        )
        .with_source_order(stacking.source_order)
        .with_effects(stacking.effects)
        .with_bounds(stacking.bounds);
        FixedPaintLayer {
            source_element,
            source_style,
            stack_level: stacking.stack_level,
            context,
            links: stacking.links,
        }
    }
}

/// Identifies one logical positioned principal across speculative layout
/// retries. Ownership prevents one captured fragment from being committed
/// twice; this identity lets the layout engine replace a separately produced
/// retry of the same principal.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct PositionedPaintIdentity {
    pub(in crate::layout) source_element: Option<crate::dom::ElementId>,
    pub(in crate::layout) source_style: ComputedStyle,
    pub(in crate::layout) source_style_identity: usize,
}

/// Page-local stacking data supplied when consuming final positioned paint.
/// It deliberately excludes the fragment: only `FinalPositionedFragment` can
/// pair this metadata with principal paint.
pub(in crate::layout) struct PositionedStackingMetadata {
    pub(in crate::layout) transaction_depth: usize,
    pub(in crate::layout) source_is_target: bool,
    pub(in crate::layout) stack_level: StackLevel,
    pub(in crate::layout) source_order: usize,
    pub(in crate::layout) effects: PaintEffects,
    pub(in crate::layout) bounds: PaintClip,
    pub(in crate::layout) child_contexts: Vec<PaintStackingContext>,
    pub(in crate::layout) links: Vec<RenderedLink>,
    pub(in crate::layout) escaped_atom_translation: EscapedAtomTranslation,
}

/// A page-local positioned layer awaiting insertion into one enclosing paint
/// context. Unlike viewport-fixed paint, it is consumed exactly once.
#[must_use]
pub(in crate::layout) struct PendingPageLocalLayer {
    destination_page: DocumentPageIndex,
    layer: PositionedPaintLayer,
}

struct PageLocalLayerMetadata {
    transaction_depth: usize,
    source_is_target: bool,
    stack_level: StackLevel,
    links: Vec<RenderedLink>,
    escaped_atom_translation: EscapedAtomTranslation,
}

impl PendingPageLocalLayer {
    fn new(
        destination_page: DocumentPageIndex,
        identity: PositionedPaintIdentity,
        context: PaintStackingContext,
        metadata: PageLocalLayerMetadata,
    ) -> Self {
        Self {
            destination_page,
            layer: PositionedPaintLayer {
                page_index: destination_page.get(),
                transaction_depth: metadata.transaction_depth,
                source_element: identity.source_element,
                source_style: identity.source_style,
                source_style_identity: identity.source_style_identity,
                multicol_fragment_index: None,
                source_is_target: metadata.source_is_target,
                stack_level: metadata.stack_level,
                context,
                links: metadata.links,
                escaped_atom_translation: metadata.escaped_atom_translation,
            },
        }
    }

    pub(in crate::layout) fn destination_page(&self) -> DocumentPageIndex {
        self.destination_page
    }

    pub(in crate::layout) fn into_layer(self) -> PositionedPaintLayer {
        self.layer
    }

    pub(in crate::layout) fn release_to_transaction_depth(
        mut self,
        transaction_depth: usize,
    ) -> PositionedPaintLayer {
        self.layer.transaction_depth = transaction_depth;
        self.layer
    }
}

/// Page-local child layers keyed by their final destination. Draining a page
/// consumes its layers, preventing an enclosing positioned principal from
/// attaching the same child context to more than one fragment.
#[derive(Default)]
pub(in crate::layout) struct PendingPageLocalLayers(
    BTreeMap<DocumentPageIndex, Vec<PendingPageLocalLayer>>,
);

impl PendingPageLocalLayers {
    pub(in crate::layout) fn from_positioned_layers(layers: Vec<PositionedPaintLayer>) -> Self {
        let mut pending = Self::default();
        for layer in layers {
            let destination_page = DocumentPageIndex::new(layer.page_index);
            let identity = PositionedPaintIdentity {
                source_element: layer.source_element,
                source_style: layer.source_style,
                source_style_identity: layer.source_style_identity,
            };
            pending.insert(PendingPageLocalLayer::new(
                destination_page,
                identity,
                layer.context,
                PageLocalLayerMetadata {
                    transaction_depth: layer.transaction_depth,
                    source_is_target: layer.source_is_target,
                    stack_level: layer.stack_level,
                    links: layer.links,
                    escaped_atom_translation: layer.escaped_atom_translation,
                },
            ));
        }
        pending
    }

    pub(in crate::layout) fn insert(&mut self, layer: PendingPageLocalLayer) {
        self.0
            .entry(layer.destination_page())
            .or_default()
            .push(layer);
    }

    pub(in crate::layout) fn page_indices(&self) -> impl Iterator<Item = DocumentPageIndex> + '_ {
        self.0.keys().copied()
    }

    pub(in crate::layout) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(in crate::layout) fn drain_for_page(
        &mut self,
        page: DocumentPageIndex,
    ) -> Vec<PendingPageLocalLayer> {
        self.0.remove(&page).unwrap_or_default()
    }
}

/// The destination-page placement for the scratch fragments of an absolutely
/// positioned principal.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AbsoluteFragmentPlacement {
    scratch_start: ScratchPageIndex,
    destination_start: DocumentPageIndex,
}

impl AbsoluteFragmentPlacement {
    pub(in crate::layout) fn new(scratch_start: usize, destination_start: usize) -> Self {
        Self {
            scratch_start: ScratchPageIndex::new(scratch_start),
            destination_start: DocumentPageIndex::new(destination_start),
        }
    }
}

/// The committed result of one positioned paint transaction.
#[must_use]
pub(in crate::layout) struct CapturedPositionedPaint {
    fragments: MaterializedFragmentPrefix<ScratchPositionedFragment>,
    effects: CapturedPositionedSideEffects,
    source_page_start: DocumentPageIndex,
    pub(in crate::layout) initial_page_context: PageContext,
}

/// Observable output emitted while a positioned subtree owns scratch pages.
///
/// The page indices are deliberately scratch-local until this value is
/// consumed by one of [`CapturedPositionedPaint`]'s projection methods.
/// Scratch effects can therefore never be committed directly to the document.
#[derive(Debug, Default)]
pub(in crate::layout) struct CapturedPositionedSideEffects {
    bookmarks: Vec<Bookmark>,
    anchors: Vec<(String, ScratchPageIndex)>,
    anchor_source_positions: Vec<(String, PaintPoint)>,
    anchor_text: Vec<(String, AnchorText)>,
    anchor_counters: Vec<(String, HashMap<String, Vec<i32>>)>,
    page_effects: Vec<(ScratchPageIndex, PendingPageSideEffects)>,
}

/// Paint and semantic output whose page ownership has been resolved together.
#[must_use]
pub(in crate::layout) struct FinalPositionedReplay {
    pub(in crate::layout) fragments: Vec<FinalPositionedFragment>,
    effects: CommittedPositionedSideEffects,
}

/// Semantic output after scratch ownership has been resolved. This stays
/// strongly typed until the final builder-facing application boundary.
#[derive(Debug, Default)]
struct CommittedPositionedSideEffects {
    bookmarks: Vec<(DocumentPageIndex, Bookmark)>,
    anchors: Vec<(String, DocumentPageIndex)>,
    anchor_source_positions: Vec<(String, PaintPoint)>,
    anchor_text: Vec<(String, AnchorText)>,
    anchor_counters: Vec<(String, HashMap<String, Vec<i32>>)>,
    page_effects: Vec<(DocumentPageIndex, PendingPageSideEffects)>,
}

impl FinalPositionedReplay {
    pub(in crate::layout) fn effect_pages(&self) -> impl Iterator<Item = DocumentPageIndex> + '_ {
        self.effects
            .bookmarks
            .iter()
            .map(|(page, _)| *page)
            .chain(self.effects.anchors.iter().map(|(_, page)| *page))
            .chain(self.effects.page_effects.iter().map(|(page, _)| *page))
    }

    pub(in crate::layout) fn retain_effects_through(&mut self, last_page: DocumentPageIndex) {
        self.effects
            .bookmarks
            .retain(|(page, _)| *page <= last_page);
        self.effects.anchors.retain(|(_, page)| *page <= last_page);
        // Anchor text and counter snapshots are paired with an anchor and are
        // only observable when that anchor survived the destination clip.
        let retained_targets: HashSet<_> = self
            .effects
            .anchors
            .iter()
            .map(|(target, _)| target.as_str())
            .collect();
        self.effects
            .anchor_source_positions
            .retain(|(target, _)| retained_targets.contains(target.as_str()));
        self.effects
            .anchor_text
            .retain(|(target, _)| retained_targets.contains(target.as_str()));
        self.effects
            .anchor_counters
            .retain(|(target, _)| retained_targets.contains(target.as_str()));
        self.effects
            .page_effects
            .retain(|(page, _)| *page <= last_page);
    }

    pub(in crate::layout) fn has_effects(&self) -> bool {
        !self.effects.bookmarks.is_empty()
            || !self.effects.anchors.is_empty()
            || !self.effects.page_effects.is_empty()
    }

    pub(in crate::layout) fn into_deferred_layout_side_effects(self) -> DeferredLayoutSideEffects {
        self.effects.into_deferred_layout_side_effects()
    }
}

impl CommittedPositionedSideEffects {
    fn into_deferred_layout_side_effects(self) -> DeferredLayoutSideEffects {
        DeferredLayoutSideEffects {
            bookmarks: self
                .bookmarks
                .into_iter()
                .map(|(page, mut bookmark)| {
                    bookmark.page_index = page.get();
                    bookmark
                })
                .collect(),
            anchors: self
                .anchors
                .into_iter()
                .map(|(target, page)| (target, page.get()))
                .collect(),
            anchor_source_positions: self.anchor_source_positions,
            anchor_text: self.anchor_text,
            anchor_counters: self.anchor_counters,
            page_effects: self
                .page_effects
                .into_iter()
                .map(|(page, mut effects)| {
                    effects.page_index = page.get();
                    effects
                })
                .collect(),
        }
    }
}

impl CapturedPositionedPaint {
    pub(in crate::layout) fn into_final_absolute(
        self,
        placement: AbsoluteFragmentPlacement,
    ) -> FinalPositionedReplay {
        let map_page = |scratch_page: ScratchPageIndex| {
            let relative_page = scratch_page
                .get()
                .saturating_sub(placement.scratch_start.get());
            DocumentPageIndex::new(placement.destination_start.get() + relative_page)
        };
        FinalPositionedReplay {
            fragments: self
                .fragments
                .into_fragments()
                .into_iter()
                .map(|fragment| {
                    FinalPositionedFragment::new(map_page(fragment.scratch_page), fragment.fragment)
                })
                .collect(),
            effects: self.effects.into_document_effects(map_page),
        }
    }

    pub(in crate::layout) fn into_final_same_pages(self) -> FinalPositionedReplay {
        let map_page = |scratch_page: ScratchPageIndex| {
            DocumentPageIndex::new(self.source_page_start.get() + scratch_page.get())
        };
        FinalPositionedReplay {
            fragments: self
                .fragments
                .into_fragments()
                .into_iter()
                .map(|fragment| {
                    FinalPositionedFragment::new(map_page(fragment.scratch_page), fragment.fragment)
                })
                .collect(),
            effects: self.effects.into_document_effects(map_page),
        }
    }
}

impl CapturedPositionedSideEffects {
    /// Convert scratch-local effects into a continuous source artifact. This
    /// is only for an isolated replay whose caller retains the scratch
    /// geometry; normal positioned replay must use a document-page mapping.
    pub(in crate::layout) fn into_continuous_source_effects(self) -> DeferredLayoutSideEffects {
        self.into_document_effects(|page| DocumentPageIndex::new(page.get()))
            .into_deferred_layout_side_effects()
    }

    fn into_document_effects(
        self,
        map_page: impl Fn(ScratchPageIndex) -> DocumentPageIndex,
    ) -> CommittedPositionedSideEffects {
        CommittedPositionedSideEffects {
            bookmarks: self
                .bookmarks
                .into_iter()
                .map(|bookmark| {
                    let page = map_page(ScratchPageIndex::new(bookmark.page_index));
                    (page, bookmark)
                })
                .collect(),
            anchors: self
                .anchors
                .into_iter()
                .map(|(target, page)| (target, map_page(page)))
                .collect(),
            anchor_source_positions: self.anchor_source_positions,
            anchor_text: self.anchor_text,
            anchor_counters: self.anchor_counters,
            page_effects: self
                .page_effects
                .into_iter()
                .map(|(page, effects)| (map_page(page), effects))
                .collect(),
        }
    }
}

/// Owns the checkpoint and pagination snapshot for one positioned subtree.
/// Consuming it extracts all scratch paint and restores the parent sequence.
#[must_use]
pub(in crate::layout) struct PositionedPaintTransaction {
    pagination: PositionedPaginationState,
    scratch_start: ScratchPageIndex,
    source_page_start: DocumentPageIndex,
    checkpoint: PaintCheckpoint,
    page_value_scope_depth: usize,
    assignment_capture_depth: usize,
}

pub(in crate::layout) struct PositionedPaginationState {
    pages: Vec<Page>,
    page_names: Vec<Option<String>>,
    page_blanks: Vec<bool>,
    page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    current_page: Page,
    current_page_has_flow_content: bool,
    current_page_has_named_page_flow_content: bool,
    current_page_selected_name: Option<String>,
    current_page_name: Option<String>,
    pub(in crate::layout) current_page_context: PageContext,
    current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    current_page_running_elements: HashMap<String, Vec<NamedStringAssignment>>,
    cursor_y: f32,
    content_left: f32,
    content_right: f32,
    fragment_top_offsets: Vec<FragmentTopOffset>,
    truncate_page_start_margins: bool,
    pending_paint_fragments: Vec<PendingPaintFragment>,
    pending_page_side_effects: Vec<PendingPageSideEffects>,
    positioned_paint_transaction_depth: usize,
    positioned_scratch_page_limit: Option<usize>,
    positioned_scratch_page_origin: Option<DocumentPageIndex>,
    absolute_positioned_page_span_target: Option<usize>,
    pending_positioned_fragmentation: PendingPositionedFragmentation,
    bookmarks: Vec<Bookmark>,
    page_anchors: HashMap<String, usize>,
    page_anchor_source_positions: HashMap<String, PaintPoint>,
    page_anchor_text: HashMap<String, AnchorText>,
    page_anchor_counters: HashMap<String, HashMap<String, Vec<i32>>>,
}

/// Retain page fragments established by nested out-of-flow layout when its
/// scratch pagination state is restored to the enclosing formatting context.
///
/// Each nested positioned subtree can extend the final document independently,
/// so neither requirement may replace the other.
/// <https://www.w3.org/TR/css-position-3/#fragmenting-absolutely-positioned-elements>
pub(in crate::layout) fn merged_positioned_page_span_target(
    enclosing: Option<usize>,
    nested: Option<usize>,
) -> Option<usize> {
    enclosing.into_iter().chain(nested).max()
}

impl PositionedPaintTransaction {
    /// Begin recording one positioned subtree into scratch fragmentainers.
    ///
    /// The checkpoint and pagination snapshot are deliberately created as one
    /// value. Callers can therefore only recover the captured paint by
    /// consuming this transaction, which restores the parent page sequence at
    /// the same ownership boundary.
    pub(in crate::layout) fn begin(
        layout: &mut LayoutBuilder<'_>,
        scratch_page_limit: Option<usize>,
    ) -> Self {
        debug_assert!(
            !layout.is_positioned_auto_size_measurement(),
            "positioned paint transactions belong only to final positioned layout"
        );
        // Positioned paint is speculative output. Move the parent output out
        // of the builder instead of cloning a growing page vector, then give
        // the surrogate a fresh local page sequence. CSS layout state stays
        // continuous while paint ownership is explicitly transactional.
        let source_page_start = DocumentPageIndex::new(
            layout
                .positioned_scratch_page_origin
                .map_or(layout.pages.len(), |origin| {
                    origin.get() + layout.pages.len()
                }),
        );
        let page_value_scope_depth = layout.page_value_scope_stack.len();
        let assignment_capture_depth = layout.assignment_capture_stack.len();
        let pagination = layout.take_positioned_pagination_state();
        let transaction = Self {
            scratch_start: ScratchPageIndex::new(0),
            source_page_start,
            checkpoint: layout.current_page.paint_checkpoint(),
            pagination,
            page_value_scope_depth,
            assignment_capture_depth,
        };
        layout.positioned_paint_transaction_depth += 1;
        layout.positioned_scratch_page_origin = Some(source_page_start);
        layout.positioned_scratch_page_limit =
            match (layout.positioned_scratch_page_limit, scratch_page_limit) {
                (Some(enclosing), Some(descendant)) => Some(enclosing.min(descendant)),
                (Some(enclosing), None) => Some(enclosing),
                (None, descendant) => descendant,
            };
        transaction
    }

    /// Drain scratch paint, merge durable nested page-span requests, and
    /// restore the parent pagination state.
    pub(in crate::layout) fn capture_and_restore(
        self,
        layout: &mut LayoutBuilder<'_>,
    ) -> CapturedPositionedPaint {
        debug_assert!(
            !layout.is_positioned_auto_size_measurement(),
            "positioned paint capture belongs only to final positioned layout"
        );
        debug_assert!(
            layout.positioned_paint_transaction_depth > 0,
            "positioned paint transaction must own scratch layout before capture"
        );
        let nested_positioned_fragmentation = layout.pending_positioned_fragmentation;
        let nested_absolute_positioned_page_span_target =
            layout.absolute_positioned_page_span_target;
        let fragments = layout
            .take_positioned_fragments_since(self.scratch_start.get(), self.checkpoint)
            .into_iter()
            .map(|(page_index, fragment)| ScratchPositionedFragment {
                scratch_page: ScratchPageIndex::new(page_index),
                fragment,
            })
            .collect();
        let effects = layout.take_positioned_scratch_side_effects();
        let initial_page_context = self.pagination.current_page_context;
        layout.restore_positioned_pagination_state(self.pagination);
        debug_assert_eq!(
            layout.page_value_scope_stack.len(),
            self.page_value_scope_depth,
            "positioned scratch replay must restore page-value scopes"
        );
        debug_assert_eq!(
            layout.assignment_capture_stack.len(),
            self.assignment_capture_depth,
            "positioned scratch replay must restore assignment-capture scopes"
        );
        layout
            .pending_positioned_fragmentation
            .merge(nested_positioned_fragmentation);
        layout.absolute_positioned_page_span_target = merged_positioned_page_span_target(
            layout.absolute_positioned_page_span_target,
            nested_absolute_positioned_page_span_target,
        );
        CapturedPositionedPaint {
            fragments: MaterializedFragmentPrefix::new(fragments),
            effects,
            source_page_start: self.source_page_start,
            initial_page_context,
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Returns the number of scratch pages that may own positioned paint.
    ///
    /// This value is installed only by a positioned transaction with a
    /// resolved, non-scrollable `overflow: clip` reach. It must never be used
    /// for normal-flow or potentially-visible overflow.
    pub(in crate::layout) fn positioned_scratch_page_limit(&self) -> Option<usize> {
        (self.positioned_paint_transaction_depth > 0)
            .then_some(self.positioned_scratch_page_limit)
            .flatten()
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn extend_positioned_principal_decoration_fragments(
        &mut self,
        fragments: &mut Vec<FinalPositionedFragment>,
        style: &ComputedStyle,
        border_box: PaintClip,
        first_page_index: usize,
        captured_last_page_index: usize,
        target_page_index: usize,
        first_page_context: PageContext,
    ) {
        if target_page_index <= captured_last_page_index || style.visibility != Visibility::Visible
        {
            return;
        }
        let fragmentainer_height = first_page_context.area_height().max(1.0);
        let box_top = border_box.y() + border_box.height();
        let box_start_distance = (first_page_context.top() - box_top).max(0.0);
        let box_end_distance = box_start_distance + border_box.height();

        for page_index in captured_last_page_index + 1..=target_page_index {
            let page_distance =
                page_index.saturating_sub(first_page_index) as f32 * fragmentainer_height;
            let slice_start = box_start_distance.max(page_distance);
            let slice_end = box_end_distance.min(page_distance + fragmentainer_height);
            if slice_end <= slice_start + 0.01 {
                continue;
            }
            let slice_top = first_page_context.top() - (slice_start - page_distance);
            let slice_height = slice_end - slice_start;
            let owns_block_start = slice_start <= box_start_distance + 0.01;
            let owns_block_end = slice_end >= box_end_distance - 0.01;
            let mut fragment_style = style.clone();
            suppress_fragmented_box_edges(&mut fragment_style, owns_block_start, owns_block_end);
            let background = self.box_background_primitives(
                paint_space_rect(
                    border_box.x(),
                    slice_top - slice_height,
                    border_box.width(),
                    slice_height,
                ),
                &fragment_style,
            );
            let outline = self.box_outline_primitives(
                paint_space_rect(
                    border_box.x(),
                    slice_top - slice_height,
                    border_box.width(),
                    slice_height,
                ),
                &fragment_style,
            );
            if background.is_empty() && outline.is_empty() {
                continue;
            }
            if let Some(fragment) = fragments
                .iter_mut()
                .find(|fragment| fragment.destination_page().get() == page_index)
            {
                fragment
                    .fragment_mut()
                    .prepend_primitives_in_band(PaintBand::BackgroundBorder, background);
                fragment
                    .fragment_mut()
                    .append_primitives_in_band(PaintBand::Outline, outline);
            } else {
                let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
                fragment.prepend_primitives_in_band(PaintBand::BackgroundBorder, background);
                fragment.append_primitives_in_band(PaintBand::Outline, outline);
                fragments.push(FinalPositionedFragment::new(
                    DocumentPageIndex::new(page_index),
                    fragment,
                ));
            }
        }
        fragments.sort_by_key(|fragment| fragment.destination_page());
    }

    /// Returns the last page index occupied by an absolutely positioned box.
    ///
    /// The margin-box span determines which page fragments an absolute box
    /// occupies. Its principal paint may be transparent, but the used box
    /// still establishes destination fragmentainers: fixed-position
    /// descendants must replay on them and later positioned descendants use
    /// their page-local containing blocks.
    ///
    /// CSS Positioned Layout makes absolutely positioned boxes out-of-flow;
    /// CSS Fragmentation permits their rendered fragments to cross
    /// fragmentainer boundaries:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn absolute_positioned_page_span_target(
        &mut self,
        style: &ComputedStyle,
        margin_box: FragmentainerBlockMarginBox,
        fragmentainer_axes: FlowAxes,
        destination_start_page_index: usize,
        destination_start_progress: f32,
    ) -> Option<usize> {
        if style.position != Position::Absolute {
            return None;
        }
        let block_start_side = fragmentainer_axes.block_start_side();
        let margin_box_block_size = margin_box.block_extent(block_start_side);
        if margin_box_block_size.points() <= 0.0 {
            return None;
        }
        // Size containment makes the principal box monolithic, but it does
        // not confine an oversized box's graphical representation to its
        // start fragmentainer. Its continuous margin-box extent therefore
        // still bounds every potential decoration slice.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        // <https://www.w3.org/TR/css-break-3/#monolithic>
        // `destination_start_progress` is source-flow progress within the
        // resolved first destination fragmentainer. The containing box's
        // physical position can include earlier source fragmentainers, which
        // must not be consumed again after the start page has been selected.
        let mut remaining_distance =
            (destination_start_progress + margin_box_block_size.points()).max(0.0);
        if remaining_distance <= 0.0 {
            return None;
        }

        // The resolved absolute margin box is continuous, but each
        // destination fragmentainer can have a different logical block
        // extent. Advance through the actual destination contexts instead of
        // dividing the source extent by the first page's capacity.
        // <https://drafts.csswg.org/css-break-4/#varying-size-fragmentainers>
        let mut destination_page_index = destination_start_page_index;
        let current_document_page_index = self
            .positioned_scratch_page_origin
            .map_or(self.pages.len(), |origin| origin.get() + self.pages.len());
        let mut destination_context = if destination_page_index == current_document_page_index {
            self.current_page_context
        } else {
            self.resolved_page_context(destination_page_index + 1, false)
        };
        loop {
            let capacity = destination_context
                .logical_block_size(fragmentainer_axes.writing_mode())
                .max(1.0);
            if remaining_distance <= capacity + 0.01 {
                return Some(destination_page_index);
            }
            remaining_distance -= capacity;
            destination_page_index += 1;
            destination_context = self.resolved_page_context(destination_page_index + 1, false);
        }
    }

    pub(in crate::layout) fn absolute_positioned_page_start_offset(
        &self,
        margin_box: FragmentainerBlockMarginBox,
        fragmentainer_axes: FlowAxes,
    ) -> (usize, f32) {
        let block_start_side = fragmentainer_axes.block_start_side();
        let fragmentainer_block_size = self
            .current_page_context
            .logical_block_size(fragmentainer_axes.writing_mode())
            .max(1.0);
        let start_distance = margin_box
            .start_distance_from(
                fragmentainer_block_start_for_page(self.current_page_context, block_start_side),
                block_start_side,
            )
            .max(0.0);
        // Treat a position at a fragmentainer end as the next
        // fragmentainer's block start. Floating-point layout arithmetic can
        // otherwise turn an exact boundary into `N - ε`, retaining a zero-use
        // source fragment and adding a spurious continuation page.
        // <https://drafts.csswg.org/css-break-4/#breaking-rules>
        let page_offset = ((start_distance + 0.01) / fragmentainer_block_size).floor() as usize;
        (
            page_offset,
            (start_distance - page_offset as f32 * fragmentainer_block_size)
                .max(0.0)
                .min(fragmentainer_block_size),
        )
    }

    /// Records final document pages required by positioned paint or descendant layers.
    ///
    /// The positioned subtree is first laid out against scratch page state so
    /// descendant fragmentation can be harvested without advancing normal flow.
    /// Only non-empty paint fragments and positioned descendant layers extend
    /// the real page sequence; an empty absolute margin-box span does not:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn ensure_positioned_page_span(
        &mut self,
        plan: PositionedFragmentationPlan,
    ) {
        if plan.materialized_destination_end().is_none() {
            return;
        }
        self.pending_positioned_fragmentation.record(plan);
        // A positioned descendant is measured while its source inline run is
        // still selecting normal-flow fragment breaks. It may need a later
        // fragmentainer for its paint, but must not advance that source flow
        // or make its widow/orphan decision from the provisional destination.
        // The enclosing formatter materializes this retained span once it
        // has committed the in-flow break sequence.
        // <https://www.w3.org/TR/css-position-3/#absolute-positioning>
        // <https://www.w3.org/TR/css-break-3/#widows-orphans>
    }

    /// Retains an absolute box's logical fragmentainer span independently
    /// from its current paint. Viewport-fixed descendants replay against the
    /// final document sequence, so their retention cannot depend on whether
    /// the fixed layer appeared before or during this subtree.
    ///
    /// <https://www.w3.org/TR/css-position-3/#fixed-pos>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn retain_absolute_positioned_page_span(
        &mut self,
        target_page_index: Option<usize>,
    ) {
        let Some(target_page_index) = target_page_index else {
            return;
        };
        self.absolute_positioned_page_span_target = Some(
            self.absolute_positioned_page_span_target
                .map_or(target_page_index, |existing| {
                    existing.max(target_page_index)
                }),
        );
    }

    pub(in crate::layout) fn materialize_pending_positioned_page_span(&mut self) {
        debug_assert!(
            !self.is_positioned_auto_size_measurement(),
            "positioned auto-size measurement must not request document pages"
        );
        if self.is_positioned_auto_size_measurement() {
            return;
        }
        if self.out_of_flow_prebreak_suppression_depth == 0 {
            let target_page_index = self
                .pending_positioned_fragmentation
                .take_materialized_destination_end()
                .into_iter()
                .chain(self.positioned_layers.iter().map(|layer| layer.page_index))
                .chain(
                    (!self.fixed_layers.is_empty())
                        .then_some(self.absolute_positioned_page_span_target)
                        .flatten(),
                )
                .max();
            let Some(target_page_index) = target_page_index else {
                return;
            };
            while self.pages.len() < target_page_index {
                if !self.current_page_has_content() {
                    self.mark_current_page_flow_content();
                }
                self.push_page_without_flushing_positioned_layers();
            }
            if self.pages.len() == target_page_index {
                self.mark_current_page_flow_content();
            }
        }
    }

    pub(in crate::layout) fn push_page_without_flushing_positioned_layers(&mut self) {
        if !self.current_page_has_content() {
            self.mark_current_page_flow_content();
        }
        let offsets = self.current_fragment_offsets_for_page_break();
        // Positioned overflow must advance through the active fragmentainer
        // sequence without flushing layers that still belong to its containing
        // stacking context. In a multicol probe the next fragment is another
        // anonymous column box, not a document page.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
        // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>.
        let next_context = self
            .fragmentainer_override
            .map(|override_| override_.context_for_fragmentainer(self.pages.len() + 1))
            .unwrap_or_else(|| {
                self.resolved_page_context(
                    self.destination_document_page_number(self.pages.len() + 2),
                    false,
                )
            });
        let next_page = page_for_context(next_context);
        let page = std::mem::replace(&mut self.current_page, next_page);
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.pages.push(page);
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(false);
        self.page_named_strings
            .push(std::mem::take(&mut self.current_page_named_strings));
        self.page_running_elements
            .push(std::mem::take(&mut self.current_page_running_elements));
        self.apply_page_context(next_context, offsets);
        self.current_page_selected_name = None;
        self.truncate_page_start_margins = true;
        self.apply_pending_fragments_for_current_page();
    }

    pub(in crate::layout) fn take_positioned_pagination_state(
        &mut self,
    ) -> PositionedPaginationState {
        let current_page_context = self.current_page_context;
        let state = PositionedPaginationState {
            pages: std::mem::take(&mut self.pages),
            page_names: std::mem::take(&mut self.page_names),
            page_blanks: std::mem::take(&mut self.page_blanks),
            page_named_strings: std::mem::take(&mut self.page_named_strings),
            page_running_elements: std::mem::take(&mut self.page_running_elements),
            current_page: std::mem::replace(
                &mut self.current_page,
                page_for_context(current_page_context),
            ),
            current_page_has_flow_content: self.current_page_has_flow_content,
            current_page_has_named_page_flow_content: self.current_page_has_named_page_flow_content,
            current_page_selected_name: std::mem::take(&mut self.current_page_selected_name),
            current_page_name: std::mem::take(&mut self.current_page_name),
            current_page_context: self.current_page_context,
            current_page_named_strings: std::mem::take(&mut self.current_page_named_strings),
            current_page_running_elements: std::mem::take(&mut self.current_page_running_elements),
            cursor_y: self.cursor_y,
            content_left: self.content_left,
            content_right: self.content_right,
            fragment_top_offsets: std::mem::take(&mut self.fragment_top_offsets),
            truncate_page_start_margins: self.truncate_page_start_margins,
            pending_paint_fragments: std::mem::take(&mut self.pending_paint_fragments),
            pending_page_side_effects: std::mem::take(&mut self.pending_page_side_effects),
            positioned_paint_transaction_depth: self.positioned_paint_transaction_depth,
            positioned_scratch_page_limit: self.positioned_scratch_page_limit,
            positioned_scratch_page_origin: self.positioned_scratch_page_origin,
            absolute_positioned_page_span_target: self.absolute_positioned_page_span_target,
            pending_positioned_fragmentation: self.pending_positioned_fragmentation,
            bookmarks: std::mem::take(&mut self.bookmarks),
            page_anchors: std::mem::take(&mut self.page_anchors),
            page_anchor_source_positions: std::mem::take(&mut self.page_anchor_source_positions),
            page_anchor_text: std::mem::take(&mut self.page_anchor_text),
            page_anchor_counters: std::mem::take(&mut self.page_anchor_counters),
        };
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.truncate_page_start_margins = false;
        self.positioned_scratch_page_limit = None;
        self.positioned_scratch_page_origin = None;
        self.pending_positioned_fragmentation = PendingPositionedFragmentation::default();
        state
    }

    pub(in crate::layout) fn restore_positioned_pagination_state(
        &mut self,
        state: PositionedPaginationState,
    ) {
        self.pages = state.pages;
        self.page_names = state.page_names;
        self.page_blanks = state.page_blanks;
        self.page_named_strings = state.page_named_strings;
        self.page_running_elements = state.page_running_elements;
        self.current_page = state.current_page;
        self.current_page_has_flow_content = state.current_page_has_flow_content;
        self.current_page_has_named_page_flow_content =
            state.current_page_has_named_page_flow_content;
        self.current_page_selected_name = state.current_page_selected_name;
        self.current_page_name = state.current_page_name;
        self.current_page_context = state.current_page_context;
        self.current_page_named_strings = state.current_page_named_strings;
        self.current_page_running_elements = state.current_page_running_elements;
        self.cursor_y = state.cursor_y;
        self.content_left = state.content_left;
        self.content_right = state.content_right;
        self.fragment_top_offsets = state.fragment_top_offsets;
        self.truncate_page_start_margins = state.truncate_page_start_margins;
        self.pending_paint_fragments = state.pending_paint_fragments;
        self.pending_page_side_effects = state.pending_page_side_effects;
        self.positioned_paint_transaction_depth = state.positioned_paint_transaction_depth;
        self.positioned_scratch_page_limit = state.positioned_scratch_page_limit;
        self.positioned_scratch_page_origin = state.positioned_scratch_page_origin;
        self.absolute_positioned_page_span_target = state.absolute_positioned_page_span_target;
        self.pending_positioned_fragmentation = state.pending_positioned_fragmentation;
        self.bookmarks = state.bookmarks;
        self.page_anchors = state.page_anchors;
        self.page_anchor_source_positions = state.page_anchor_source_positions;
        self.page_anchor_text = state.page_anchor_text;
        self.page_anchor_counters = state.page_anchor_counters;
    }

    pub(in crate::layout) fn take_positioned_scratch_side_effects(
        &mut self,
    ) -> CapturedPositionedSideEffects {
        let scratch_page_count = self.pages.len();
        let mut named_strings = std::mem::take(&mut self.page_named_strings);
        let mut running_elements = std::mem::take(&mut self.page_running_elements);
        named_strings.resize_with(scratch_page_count, HashMap::new);
        running_elements.resize_with(scratch_page_count, HashMap::new);
        let mut page_effects = named_strings
            .into_iter()
            .zip(running_elements)
            .enumerate()
            .filter_map(|(page_index, (named_strings, running_elements))| {
                (!named_strings.is_empty() || !running_elements.is_empty()).then_some((
                    ScratchPageIndex::new(page_index),
                    PendingPageSideEffects {
                        page_index,
                        named_strings,
                        running_elements,
                        links: Vec::new(),
                    },
                ))
            })
            .collect::<Vec<_>>();
        let current_named_strings = std::mem::take(&mut self.current_page_named_strings);
        let current_running_elements = std::mem::take(&mut self.current_page_running_elements);
        if !current_named_strings.is_empty() || !current_running_elements.is_empty() {
            page_effects.push((
                ScratchPageIndex::new(scratch_page_count),
                PendingPageSideEffects {
                    page_index: scratch_page_count,
                    named_strings: current_named_strings,
                    running_elements: current_running_elements,
                    links: Vec::new(),
                },
            ));
        }
        CapturedPositionedSideEffects {
            bookmarks: std::mem::take(&mut self.bookmarks),
            anchors: std::mem::take(&mut self.page_anchors)
                .into_iter()
                .map(|(target, page)| (target, ScratchPageIndex::new(page)))
                .collect(),
            anchor_source_positions: std::mem::take(&mut self.page_anchor_source_positions)
                .into_iter()
                .collect(),
            anchor_text: std::mem::take(&mut self.page_anchor_text)
                .into_iter()
                .collect(),
            anchor_counters: std::mem::take(&mut self.page_anchor_counters)
                .into_iter()
                .collect(),
            page_effects,
        }
    }

    /// Captures out-of-flow positioned paint fragments from every page touched by layout.
    ///
    /// CSS Positioned Layout takes absolutely positioned boxes out of normal
    /// flow, while CSS Fragmentation still allows their contents to split
    /// across page fragmentainers. Each produced page fragment must therefore
    /// be replayed in the positioned stacking level for that page, not left as
    /// normal-flow paint and not replayed as one page-local fragment:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn take_positioned_fragments_since(
        &mut self,
        paint_page_index: usize,
        paint_checkpoint: PaintCheckpoint,
    ) -> Vec<(usize, PaintFragment)> {
        if self.pages.len() == paint_page_index {
            return vec![(
                paint_page_index,
                self.current_page
                    .take_paint_fragment_since(paint_checkpoint),
            )];
        }

        let mut fragments = Vec::new();
        if let Some(page) = self.pages.get_mut(paint_page_index) {
            fragments.push((
                paint_page_index,
                page.take_paint_fragment_since(paint_checkpoint),
            ));
        }
        for page_index in paint_page_index + 1..self.pages.len() {
            let fragment = self.pages[page_index].take_paint_fragment();
            fragments.push((page_index, fragment));
        }
        fragments.push((self.pages.len(), self.current_page.take_paint_fragment()));
        fragments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margin_box_progress_uses_the_fragmentainer_block_axis() {
        let margin_box =
            FragmentainerBlockMarginBox::new(PageTopRect::new(40.0, 180.0, 30.0, 60.0));

        assert_eq!(
            margin_box.start_distance_from(200.0, PhysicalSide::Top),
            20.0
        );
        assert_eq!(
            margin_box.start_distance_from(10.0, PhysicalSide::Left),
            30.0
        );
        assert_eq!(
            margin_box.start_distance_from(100.0, PhysicalSide::Right),
            30.0
        );
        assert_eq!(margin_box.block_extent(PhysicalSide::Top), layout_pt(60.0));
        assert_eq!(
            margin_box.block_extent(PhysicalSide::Right),
            layout_pt(30.0)
        );
    }

    #[test]
    fn clipped_positioned_span_retains_only_the_reachable_prefix() {
        let plan = PositionedFragmentationPlan::for_absolute_box(
            0,
            Some(1_000_000),
            100.0,
            PhysicalSide::Top,
            layout_pt(100.0),
            PositionedPaintReach::Clipped {
                clip: PaintClip::new(0.0, 0.0, 100.0, 100.0),
            },
        );

        assert_eq!(plan.materialized_destination_end(), Some(0));
        assert_eq!(
            plan.logical_tail(),
            Some(LogicalFragmentainerSpan {
                start: FragmentainerOrdinal::new(1),
                count: FragmentainerCount::new(1_000_000),
                fragmentainer_block_size: layout_pt(100.0),
                final_fragment_used_block_size: layout_pt(100.0),
            })
        );
    }

    #[test]
    fn potentially_visible_positioned_span_stays_fully_materialized() {
        let plan = PositionedFragmentationPlan::for_absolute_box(
            3,
            Some(12),
            100.0,
            PhysicalSide::Top,
            layout_pt(100.0),
            PositionedPaintReach::PotentiallyVisible,
        );

        assert_eq!(plan.materialized_destination_end(), Some(12));
        assert_eq!(plan.logical_tail(), None);
    }

    #[test]
    fn vertical_fragmentainers_use_the_horizontal_overflow_clip_axis() {
        let clip = OverflowClip::from_paint_rect_with_axes_and_non_scrollable(
            paint_space_rect(0.0, 0.0, 150.0, 100.0),
            true,
            false,
            true,
            false,
        );
        let reach = PositionedPaintReach::from_overflow_clips(
            &[clip],
            FlowAxes::new(WritingMode::VerticalLr, Direction::Ltr),
        );
        assert!(matches!(reach, PositionedPaintReach::Clipped { .. }));

        let plan = PositionedFragmentationPlan::for_absolute_box(
            0,
            Some(1_000_000),
            0.0,
            PhysicalSide::Left,
            layout_pt(100.0),
            reach,
        );
        assert_eq!(plan.materialized_destination_end(), Some(1));
    }

    #[test]
    fn right_to_left_vertical_fragmentainers_measure_clipping_from_the_right_edge() {
        let clip = OverflowClip::from_paint_rect_with_axes_and_non_scrollable(
            paint_space_rect(150.0, 0.0, 150.0, 100.0),
            true,
            false,
            true,
            false,
        );
        let reach = PositionedPaintReach::from_overflow_clips(
            &[clip],
            FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr),
        );
        assert!(matches!(reach, PositionedPaintReach::Clipped { .. }));

        let plan = PositionedFragmentationPlan::for_absolute_box(
            0,
            Some(1_000_000),
            300.0,
            PhysicalSide::Right,
            layout_pt(100.0),
            reach,
        );
        assert_eq!(plan.materialized_destination_end(), Some(1));
    }

    #[test]
    fn pending_positioned_fragmentation_never_materializes_a_clipped_tail() {
        let mut pending = PendingPositionedFragmentation::default();
        pending.record(PositionedFragmentationPlan::for_absolute_box(
            0,
            Some(50_000),
            100.0,
            PhysicalSide::Top,
            layout_pt(100.0),
            PositionedPaintReach::Clipped {
                clip: PaintClip::new(0.0, 0.0, 100.0, 100.0),
            },
        ));

        assert_eq!(pending.take_materialized_destination_end(), Some(0));
        assert_eq!(pending.take_materialized_destination_end(), None);
        assert_eq!(
            pending
                .logical_tail
                .expect("logical tail is retained")
                .count,
            FragmentainerCount::new(50_000)
        );
    }

    #[test]
    fn scratch_effects_map_once_and_clipped_tail_effects_are_omitted() {
        let source = CapturedPositionedPaint {
            fragments: MaterializedFragmentPrefix::new(Vec::new()),
            effects: CapturedPositionedSideEffects {
                anchors: vec![
                    ("first".to_owned(), ScratchPageIndex::new(0)),
                    ("clipped-tail".to_owned(), ScratchPageIndex::new(2)),
                ],
                anchor_text: vec![
                    (
                        "first".to_owned(),
                        AnchorText {
                            content: "first".to_owned(),
                            before: String::new(),
                            after: String::new(),
                        },
                    ),
                    (
                        "clipped-tail".to_owned(),
                        AnchorText {
                            content: "tail".to_owned(),
                            before: String::new(),
                            after: String::new(),
                        },
                    ),
                ],
                ..CapturedPositionedSideEffects::default()
            },
            source_page_start: DocumentPageIndex::new(4),
            initial_page_context: PageContext::from_options(&RenderOptions::default()),
        };

        let mut replay = source.into_final_same_pages();
        assert_eq!(
            replay.effects.anchors,
            vec![
                ("first".to_owned(), DocumentPageIndex::new(4)),
                ("clipped-tail".to_owned(), DocumentPageIndex::new(6)),
            ]
        );

        replay.retain_effects_through(DocumentPageIndex::new(4));
        assert_eq!(
            replay.effects.anchors,
            vec![("first".to_owned(), DocumentPageIndex::new(4))]
        );
        assert_eq!(replay.effects.anchor_text.len(), 1);
        assert_eq!(replay.effects.anchor_text[0].0, "first");
    }
}
