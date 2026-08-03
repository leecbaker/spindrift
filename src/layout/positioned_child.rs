use super::*;
use std::borrow::Cow;

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
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedChildStaticRect {
    left: f32,
    right: f32,
    top: f32,
    containing_block: Option<ContainingBlock>,
    static_alignment: Option<AbsposStaticAlignment>,
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
    fragment: PositionedFragmentReplay,
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
    destination_translation: PaintTranslation,
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
        destination_translation: PaintTranslation,
    ) -> Self {
        Self {
            source_clip,
            source_block_start,
            source_block_end,
            destination_translation,
            destination_clip: PaintClip::new(
                source_clip.x() + destination_translation.x,
                source_clip.y() + destination_translation.y,
                source_clip.width(),
                source_clip.height(),
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct PositionedFragmentReplay {
    source_static_rect: PositionedChildStaticRect,
    positioning_containing_block: Option<(PositionedContainingBlockMode, ContainingBlock)>,
    owning_fragmentainer_index: Option<usize>,
    source_fragment_block_offset: LayoutLength,
    local_to_page_translation: PaintTranslation,
    clip: Option<PaintClip>,
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
            owning_fragmentainer_index: None,
            source_fragment_block_offset: layout_pt(0.0),
            local_to_page_translation: PaintTranslation::identity(),
            clip: None,
            owner_resolution: PositionedFragmentOwnerResolution::None,
            candidate_source_space: PositionedCandidateSourceSpace::StaticPositionLocal,
            unresolved_candidates: Vec::new(),
        }
    }

    pub(in crate::layout) fn committed_to_fragmentainer(
        mut self,
        owning_fragmentainer_index: usize,
        local_to_page_translation: PaintTranslation,
        clip: Option<PaintClip>,
    ) -> Self {
        self.owning_fragmentainer_index = Some(owning_fragmentainer_index);
        self.local_to_page_translation = local_to_page_translation;
        self.clip = clip;
        self
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

    /// Adds the destination clip selected by the owning committed
    /// fragmentainer.
    ///
    /// A direct positioned descendant captures its static rectangle while the
    /// multicolumn source pages are active, but its destination clip is only
    /// known after those pages have been projected back into the real page.
    /// <https://www.w3.org/TR/css-position-3/#static-position>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn with_destination_clip(mut self, clip: PaintClip) -> Self {
        self.clip = Some(clip);
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
        ) && self.owning_fragmentainer_index.is_none()
            && !self.unresolved_candidates.iter().any(|existing| {
                existing.source_clip == candidate.source_clip
                    && existing.destination_translation == candidate.destination_translation
            })
        {
            self.unresolved_candidates.push(candidate);
        }
    }

    /// Projects source-page positioned geometry into its committed destination
    /// fragmentainer.
    ///
    /// The static rectangle is retained in source coordinates and translated
    /// during replay. The containing block moves now, because positioned inset
    /// resolution consumes it during that replay.
    /// <https://www.w3.org/TR/css-position-3/#static-position>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn projected_to_destination(
        mut self,
        translation: PaintTranslation,
    ) -> Self {
        debug_assert_eq!(
            self.local_to_page_translation,
            PaintTranslation::identity(),
            "a positioned source record may be committed to one destination only",
        );
        self.local_to_page_translation = translation;
        self.positioning_containing_block = self
            .positioning_containing_block
            .map(|(mode, containing_block)| (mode, containing_block.translated(translation)));
        self
    }

    /// Whether this record was captured on `fragmentainer_index`.
    pub(in crate::layout) fn owns_source_fragmentainer(&self, fragmentainer_index: usize) -> bool {
        self.owning_fragmentainer_index == Some(fragmentainer_index)
    }

    pub(in crate::layout) fn has_unresolved_candidates(&self) -> bool {
        !matches!(
            self.owner_resolution,
            PositionedFragmentOwnerResolution::None
        ) && self.owning_fragmentainer_index.is_none()
            && !self.unresolved_candidates.is_empty()
    }

    fn candidates_for_owner(&self) -> Vec<PositionedFragmentCandidate> {
        let selected = self
            .unresolved_candidates
            .iter()
            .copied()
            .filter(|candidate| match self.owner_resolution {
                PositionedFragmentOwnerResolution::None => false,
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

    /// Translate a layer resolved in source-global block coordinates into one
    /// committed multicolumn destination.
    ///
    /// A definite block inset is measured from the flex container's source
    /// block start. A later source slice therefore moves toward the final
    /// destination block start by that slice's source offset. This differs
    /// from a static-position-local layer, which is already expressed in its
    /// candidate's source fragmentainer coordinates.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://www.w3.org/TR/css-position-3/#inset-properties>
    fn destination_translation_for_candidate(
        &self,
        candidate: PositionedFragmentCandidate,
    ) -> PaintTranslation {
        let source_block_offset = matches!(
            self.candidate_source_space,
            PositionedCandidateSourceSpace::DefiniteBlockInsetGlobal
        )
        .then(|| candidate.source_block_start.points())
        .unwrap_or(0.0);
        PaintTranslation::new(
            candidate.destination_translation.x,
            candidate.destination_translation.y + source_block_offset,
        )
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
            .push(DeferredMulticolPositionedChild {
                element: element.clone(),
                signature: Box::new(signature.clone()),
                style,
                fragment,
            });
    }

    /// Project deferred positioned records captured on one temporary
    /// multicolumn source fragmentainer into its committed destination.
    ///
    /// Direct positioned children are retained locally by multicolumn layout,
    /// while flex positioned children enter the shared deferred queue. Both
    /// records must receive the identical source-to-destination translation
    /// before positioned inset resolution consumes their containing blocks.
    /// <https://www.w3.org/TR/css-position-3/#static-position>
    /// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
    pub(in crate::layout) fn project_deferred_multicol_positioned_fragments(
        &mut self,
        start: usize,
        source_fragmentainer_index: usize,
        translation: PaintTranslation,
    ) {
        for child in self.deferred_multicol_positioned_children[start..].iter_mut() {
            if child
                .fragment
                .owns_source_fragmentainer(source_fragmentainer_index)
            {
                child.fragment = child.fragment.clone().projected_to_destination(translation);
            }
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
        source_clip: PaintClip,
        source_block_start: LayoutLength,
        source_block_end: LayoutLength,
        destination_translation: PaintTranslation,
    ) {
        let candidate = PositionedFragmentCandidate::new(
            source_clip,
            source_block_start,
            source_block_end,
            destination_translation,
        );
        for child in self.deferred_multicol_positioned_children[start..].iter_mut() {
            child.fragment.add_unresolved_candidate(candidate);
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
        let deferred = self.deferred_multicol_positioned_children.split_off(start);
        for child in deferred {
            let static_rect = child
                .fragment
                .source_static_rect
                .translated(child.fragment.local_to_page_translation);
            let _owning_fragmentainer = child.fragment.owning_fragmentainer_index;
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
            if child.fragment.localizes_static_rect_per_candidate() {
                for candidate in child.fragment.candidates_for_owner() {
                    let global_block_inset = matches!(
                        child.fragment.candidate_source_space,
                        PositionedCandidateSourceSpace::DefiniteBlockInsetGlobal
                    );
                    let scope = if global_block_inset {
                        child.fragment.source_containing_block()
                    } else {
                        child
                            .fragment
                            .containing_block_local_to_candidate(candidate)
                    }
                    .map(|(mode, containing_block)| {
                        self.push_positioned_containing_block(mode, containing_block)
                    });
                    let positioned_layer_start = self.positioned_layers.len();
                    let candidate_static_rect = if global_block_inset {
                        static_rect
                    } else {
                        static_rect.translated(PaintTranslation::new(
                            0.0,
                            candidate.source_block_start.points(),
                        ))
                    };
                    let stylesheets = self.stylesheets;
                    self.layout_positioned_formatting_context_child(
                        &replay_child,
                        &stylesheets,
                        candidate_static_rect,
                    );
                    let source_layers = self.positioned_layers.split_off(positioned_layer_start);
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
                        layer = layer.translated(
                            child
                                .fragment
                                .destination_translation_for_candidate(candidate),
                        );
                        layer.page_index = self.pages.len();
                        layer.context.effects.overflow_clip = Some(
                            layer
                                .context
                                .effects
                                .overflow_clip
                                .and_then(|existing| existing.intersect(candidate.destination_clip))
                                .unwrap_or(candidate.destination_clip),
                        );
                        self.positioned_layers.push(layer);
                    }
                    if let Some(scope) = scope {
                        self.pop_positioned_containing_block(scope);
                    }
                }
                continue;
            }
            let scope =
                child
                    .fragment
                    .positioning_containing_block
                    .map(|(mode, containing_block)| {
                        self.push_positioned_containing_block(mode, containing_block)
                    });
            let positioned_layer_start = self.positioned_layers.len();
            let stylesheets = self.stylesheets;
            self.layout_positioned_formatting_context_child(
                &replay_child,
                &stylesheets,
                static_rect,
            );
            if child.fragment.has_unresolved_candidates() {
                let candidates = child.fragment.candidates_for_owner();
                let source_layers = self.positioned_layers.split_off(positioned_layer_start);
                for layer in source_layers {
                    for candidate in &candidates {
                        let mut projected_layer =
                            layer.clone().translated(candidate.destination_translation);
                        projected_layer.page_index = self.pages.len();
                        projected_layer.context.effects.overflow_clip = Some(
                            projected_layer
                                .context
                                .effects
                                .overflow_clip
                                .and_then(|existing| existing.intersect(candidate.destination_clip))
                                .unwrap_or(candidate.destination_clip),
                        );
                        self.positioned_layers.push(projected_layer);
                    }
                }
            } else if let Some(fragment_clip) = child.fragment.clip {
                for layer in &mut self.positioned_layers[positioned_layer_start..] {
                    layer.context.effects.overflow_clip = Some(
                        layer
                            .context
                            .effects
                            .overflow_clip
                            .and_then(|existing| existing.intersect(fragment_clip))
                            .unwrap_or(fragment_clip),
                    );
                }
            }
            if let Some(scope) = scope {
                self.pop_positioned_containing_block(scope);
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
        let scope = PositionedContainingBlockScope {
            containing_blocks_depth: self.containing_blocks.len(),
            fixed_containing_blocks_depth: self.fixed_containing_blocks.len(),
            mode,
        };
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
        *self
            .containing_blocks
            .last_mut()
            .expect("positioned containing-block scope has one absolute stack entry") =
            containing_block;
        if scope.mode.establishes_fixed_containing_block() {
            debug_assert_eq!(
                self.fixed_containing_blocks.len(),
                scope.fixed_containing_blocks_depth + 1,
                "fixed containing-block scope must match absolute scope",
            );
            *self
                .fixed_containing_blocks
                .last_mut()
                .expect("fixed containing-block scope has one fixed stack entry") =
                containing_block;
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
            self.containing_blocks.push(containing_block);
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
    fn positioned_fragment_replay_projects_static_rect_and_containing_block_once() {
        let source_containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(10.0, 20.0, 30.0, 40.0));
        let destination_clip = PaintClip::new(100.0, -30.0, 30.0, 40.0);
        let replay = PositionedFragmentReplay::unfragmented(
            PositionedChildStaticRect::new(1.0, 4.0, 3.0),
            Some((
                PositionedContainingBlockMode::AbsoluteOnly,
                source_containing_block,
            )),
        )
        .committed_to_fragmentainer(2, PaintTranslation::identity(), None)
        .projected_to_destination(PaintTranslation::new(100.0, -50.0))
        .with_destination_clip(destination_clip);

        assert!(replay.owns_source_fragmentainer(2));
        assert_eq!(
            replay
                .source_static_rect
                .translated(replay.local_to_page_translation)
                .left,
            101.0
        );
        assert_eq!(
            replay
                .source_static_rect
                .translated(replay.local_to_page_translation)
                .top,
            -47.0
        );
        let (_, projected_containing_block) = replay.positioning_containing_block.unwrap();
        assert_eq!(projected_containing_block.x(), 110.0);
        assert_eq!(projected_containing_block.top_y(), -30.0);
        assert_eq!(replay.clip, Some(destination_clip));
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
            PaintTranslation::new(30.0, -20.0),
        ));
        replay.add_unresolved_candidate(PositionedFragmentCandidate::new(
            PaintClip::new(0.0, 100.0, 20.0, 40.0),
            layout_pt(0.0),
            layout_pt(40.0),
            PaintTranslation::new(30.0, -20.0),
        ));
        replay.add_unresolved_candidate(PositionedFragmentCandidate::new(
            PaintClip::new(0.0, 140.0, 20.0, 40.0),
            layout_pt(40.0),
            layout_pt(80.0),
            PaintTranslation::new(60.0, -40.0),
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
            ));
        }

        assert_eq!(
            replay.candidates_for_owner(),
            replay.unresolved_candidates[..3],
            "a column-axis flex static rectangle retains every intersected source fragment",
        );
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
    fn global_block_inset_projection_advances_by_source_slice_start() {
        let candidate = PositionedFragmentCandidate::new(
            PaintClip::new(0.0, 100.0, 20.0, 40.0),
            layout_pt(40.0),
            layout_pt(80.0),
            PaintTranslation::new(30.0, -20.0),
        );
        let local = PositionedFragmentReplay::unfragmented(
            PositionedChildStaticRect::new(1.0, 4.0, 3.0),
            None,
        )
        .resolving_owner_from_source_block_interval(layout_pt(0.0), layout_pt(80.0));
        let global = local.clone().with_definite_block_inset_source_coordinates();

        assert_eq!(
            local.destination_translation_for_candidate(candidate),
            PaintTranslation::new(30.0, -20.0),
        );
        assert_eq!(
            global.destination_translation_for_candidate(candidate),
            PaintTranslation::new(30.0, 20.0),
            "a source-global top inset advances with its committed source slice",
        );
    }
}
