use super::generated_content::{
    annotate_line_break_element_breaks_with_clear, generated_content_originating_clear,
    generated_pseudo_counter_source, generated_pseudo_inline_content_style,
    inline_style_establishes_positioning_containing_block,
};
use super::positioned::{
    DeferredInlinePositionedDescendant, DeferredInlineStaticPositionedDescendant,
    DeferredStaticPositionedContent,
};
use super::ruby::{
    inline_item_has_typographic_content, ruby_has_out_of_flow_descendant, ruby_out_of_flow_overlay,
};
use super::*;
use crate::units::glyph_baseline_displacement_pt;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrozenInlineReplayEligibility {
    Eligible,
    HasInlineFlowEffects,
    EstablishesScrollContainer,
}

/// A side-effect-free frozen inline source that can be selected while sizing
/// an orthogonal block and committed after its final physical geometry is
/// known.
///
/// `eligibility` records why the candidate may not be replayed. In
/// particular, a scroll container has clipping and fragmentation behavior
/// beyond a simple line stack, so it stays on the normal final-layout path.
/// The item stream is assembled by the same frozen-box collector as final
/// inline layout. Positioned descendants are retained as static-position
/// recipes: they are out of flow and therefore cannot affect the selected
/// line stack, but must be materialized against the final containing box.
/// <https://drafts.csswg.org/css-overflow-3/#scroll-containers>
/// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
/// <https://drafts.csswg.org/css-position-3/#abspos-layout>
#[derive(Clone)]
pub(in crate::layout) struct FrozenInlineReplayInput {
    items: Vec<InlineItem>,
    deferred_static_positioned_descendants: Vec<DeferredInlineStaticPositionedDescendant>,
    eligibility: FrozenInlineReplayEligibility,
}

impl FrozenInlineReplayInput {
    pub(in crate::layout) fn selection_items(&self) -> Vec<InlineItem> {
        self.items.clone()
    }

    pub(in crate::layout) fn is_replay_safe(&self) -> bool {
        self.eligibility == FrozenInlineReplayEligibility::Eligible
    }
}

pub(super) fn positioned_descendant_has_explicit_inset(style: &ComputedStyle) -> bool {
    [
        &style.box_values.inset_top,
        &style.box_values.inset_right,
        &style.box_values.inset_bottom,
        &style.box_values.inset_left,
    ]
    .into_iter()
    .any(|inset| !matches!(inset, css::ComputedLengthPercentageOrAuto::Auto))
}

/// Mark generated text belonging to one footnote call without assigning the
/// footnote to a page yet. The marker is retained through graph selection and
/// consumed only when the owning line is committed.
fn mark_inline_items_as_footnote_call(items: &mut [InlineItem], element: ElementId) {
    for item in items {
        if let InlineItem::Word(word) = item {
            word.source = InlineTextSource::FootnoteCall(element);
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn current_static_position_containing_block(
        &self,
    ) -> Option<StaticPositionContainingBlock> {
        self.static_position_containing_blocks.last().copied()
    }

    pub(in crate::layout) fn push_element_content_items_from_dom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        let mut deferred_static_positioned_descendants = Vec::new();
        self.push_element_content_items_from_dom_with_positioned_descendants(
            element,
            style,
            style,
            stylesheets,
            inherited_link,
            placement,
            None,
            None,
            Some(&mut deferred_static_positioned_descendants),
            output,
        );
        self.layout_deferred_inline_static_positioned_descendants(
            deferred_static_positioned_descendants,
            stylesheets,
            output,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn push_element_content_items_from_dom_with_positioned_descendants(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        line_formatting_context_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        placement: InlinePlacement,
        active_positioning_containing_block: Option<
            BorrowedInlinePositioningContainingBlockSource<'_>,
        >,
        mut deferred_positioned_descendants: Option<&mut Vec<DeferredInlinePositionedDescendant>>,
        mut deferred_static_positioned_descendants: Option<
            &mut Vec<DeferredInlineStaticPositionedDescendant>,
        >,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_inline_items_with_positioned_descendants(
                        element,
                        style,
                        line_formatting_context_style,
                        stylesheets,
                        inherited_link.clone(),
                        placement,
                        active_positioning_containing_block,
                        deferred_positioned_descendants.as_deref_mut(),
                        deferred_static_positioned_descendants.as_deref_mut(),
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                box_tree::CounterEventSource::Principal,
                inherited_link.clone(),
                placement.baseline_shift(),
                placement.visual_offset,
                alt_text.clone(),
                output,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_element_content_items_from_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        source: box_tree::CounterEventSource,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_inline_box_items(
                        children,
                        stylesheets,
                        inherited_link.clone(),
                        baseline_shift,
                        visual_offset,
                        block_style,
                        propagated_decoration_layers.clone(),
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                source,
                inherited_link.clone(),
                baseline_shift,
                visual_offset,
                alt_text.clone(),
                output,
            );
        }
    }

    pub(in crate::layout) fn collect_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        let mut deferred_static_positioned_descendants = Vec::new();
        self.collect_inline_items_with_positioned_descendants(
            element,
            style,
            style,
            stylesheets,
            inherited_link,
            placement,
            None,
            None,
            Some(&mut deferred_static_positioned_descendants),
            output,
        );
        self.layout_deferred_inline_static_positioned_descendants(
            deferred_static_positioned_descendants,
            stylesheets,
            output,
        );
    }

    /// Collects inline DOM content, deferring explicit-inset descendants of a
    /// positioned inline until its final source edge is available.
    ///
    /// CSS 2.2 derives a positioned inline's containing block from its first
    /// and last line boxes, so eager layout at the descendant's DOM position
    /// observes an incomplete rectangle.
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>
    #[allow(clippy::too_many_arguments)]
    fn collect_inline_items_with_positioned_descendants(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        line_formatting_context_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        placement: InlinePlacement,
        active_positioning_containing_block: Option<
            BorrowedInlinePositioningContainingBlockSource<'_>,
        >,
        mut deferred_positioned_descendants: Option<&mut Vec<DeferredInlinePositionedDescendant>>,
        mut deferred_static_positioned_descendants: Option<
            &mut Vec<DeferredInlineStaticPositionedDescendant>,
        >,
        output: &mut Vec<InlineItem>,
    ) {
        #[cfg(all(feature = "stack-profile", target_os = "macos"))]
        let mut stack_profile_scope =
            stack_profile::enter("collect_inline_items_with_positioned_descendants");
        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        for child in &element.children {
            #[cfg(all(feature = "stack-profile", target_os = "macos"))]
            stack_profile_scope.set_source_index(element_index);
            match &child.kind {
                NodeKind::Text(text) => {
                    if element_suppresses_direct_text_children(element) {
                        continue;
                    }
                    self.push_inline_words(
                        text,
                        style,
                        inherited_link.clone(),
                        placement.baseline_shift(),
                        placement.visual_offset,
                        output,
                    );
                }
                NodeKind::Element(child_element) => {
                    if is_html_select_item_element(child_element)
                        && !has_html_select_context(element, &self.ancestors)
                    {
                        continue;
                    }
                    let child_signature = ElementSignature::from_sibling_snapshot(
                        element_index,
                        sibling_tags.clone(),
                    )
                    .expect("source child must have a cached sibling signature");
                    element_index += 1;
                    let mut child_style = self.style_for_layout_element_with_parent_font_metrics(
                        child_element,
                        child_signature.clone(),
                        stylesheets,
                        Some(style),
                    );
                    // CSS Display Appendix B makes `display: contents`
                    // compute to `none` for HTML line-break controls. Check
                    // the computed display before recognizing `<br>`'s HTML
                    // forced-break behavior so a suppressed break contributes
                    // neither a line boundary nor a placeholder.
                    // <https://drafts.csswg.org/css-display-3/#unbox-html>
                    if child_style.display.is_none() {
                        continue;
                    }
                    // The box-tree builder retargets a suppressed source
                    // event to the nearest visible source boundary. This
                    // direct-DOM path bypasses frozen formatting boxes, so it
                    // must consume that boundary here just as the frozen-box
                    // collector does.
                    // <https://www.w3.org/TR/css-gcpm-3/#setting-named-strings>
                    self.capture_suppressed_named_strings_before(child_element.id);
                    if is_line_break_element(child_element) {
                        // `<br>` is an HTML forced-break boundary. Its
                        // generated UA `::before` content supplies the
                        // rendering fallback, but cannot remove the semantic
                        // line break or its static-position placeholder.
                        // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
                        output.push(InlineItem::Break(InlineBreak {
                            clear: child_style.clear,
                            origin: InlineBreakOrigin::Explicit,
                        }));
                        continue;
                    }
                    if child_style.float == Float::Footnote {
                        // Footnote bodies are detached from the principal
                        // inline stream. Their call remains at the source
                        // position and is the only in-flow representation of
                        // this element.
                        // <https://www.w3.org/TR/css-gcpm-3/#footnotes>
                        self.push_generated_pseudo_items(
                            child_element,
                            &child_style,
                            child_style.footnote_call_style.as_deref(),
                            inherited_link.clone(),
                            placement.baseline_shift(),
                            placement.visual_offset,
                            GeneratedPseudoCounterMode::Commit,
                            output,
                        );
                        self.capture_suppressed_named_strings_after(child_element.id);
                        continue;
                    }
                    if child_style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            child_element.clone(),
                            child_signature,
                            child_style,
                            false,
                            None,
                            active_positioning_containing_block
                                .map(BorrowedInlinePositioningContainingBlockSource::into_owned)
                                .or_else(|| {
                                    inline_style_establishes_positioning_containing_block(style)
                                        .then(|| InlinePositioningContainingBlockSource {
                                            id: InlinePositioningContainingBlockId(output.len()),
                                            style: Box::new(
                                                self.style_with_current_used_lengths(style),
                                            ),
                                        })
                                }),
                        ))));
                        continue;
                    }
                    // CSS Ruby inlinifies a direct in-flow block child before
                    // it reaches the generic DOM inline collector. The
                    // frozen box-tree normalizer performs the equivalent
                    // transformation, but this source-DOM path must retain
                    // the same atomic flow-root boundary for painting and
                    // sizing.
                    // <https://drafts.csswg.org/css-ruby-1/#anon-gen-inlinize>
                    let ruby_inlinifies_direct_block = (style.display.is_ruby()
                        || style.display.is_ruby_internal())
                        && !matches!(child_style.position, Position::Absolute | Position::Fixed)
                        && child_style.display.is_block_level()
                        && child_style.display.is_flow();
                    if ruby_inlinifies_direct_block {
                        child_style.display =
                            Display::new(DisplayOuter::Inline, DisplayInner::FlowRoot);
                    }
                    // This DOM collector owns inline-formatting content.
                    // Normal-flow block-level source boxes are represented
                    // by frozen formatting boxes. Inline-level tables instead
                    // establish an atomic inline formatting context and must
                    // remain in this collector. An absolutely positioned
                    // descendant instead participates in the inline
                    // ancestor's static-position algorithm regardless of its
                    // outer display type.
                    // <https://www.w3.org/TR/css-position-3/#abspos-layout>
                    // <https://www.w3.org/TR/css-display-3/#inlinification>
                    let participates_in_inline_collection = !child_style.display.is_none()
                        && (matches!(child_style.position, Position::Absolute | Position::Fixed)
                            || !child_style.display.is_block_level());
                    if participates_in_inline_collection
                        && matches!(child_style.position, Position::Absolute | Position::Fixed)
                    {
                        if positioned_descendant_has_explicit_inset(&child_style)
                            && let Some(containing_block_source) =
                                active_positioning_containing_block
                            && let Some(deferred) = deferred_positioned_descendants.as_deref_mut()
                        {
                            deferred.push(DeferredInlinePositionedDescendant {
                                element: child_element.clone(),
                                style: child_style,
                                static_position_container_style: style.clone(),
                                containing_block_source: containing_block_source.into_owned(),
                            });
                            continue;
                        }
                        // A non-atomic inline abspos source owns a static
                        // rectangle spanning the *selected* hypothetical
                        // line.  That line can include following siblings,
                        // so measuring it at this source position would use
                        // an incomplete line strut.  Keep its source boundary
                        // and replay after this inline scope closes.
                        // <https://drafts.csswg.org/css-position-3/#static-position>
                        if !(child_style.abspos_static_source.is_atomic_inline()
                            || child_style.display.is_atomic_inline())
                            && let Some(deferred) =
                                deferred_static_positioned_descendants.as_deref_mut()
                        {
                            deferred.push(DeferredInlineStaticPositionedDescendant {
                                element: child_element.clone(),
                                style: child_style,
                                line_formatting_context_style: line_formatting_context_style
                                    .clone(),
                                static_position_container_style: style.clone(),
                                static_position_containing_block: self
                                    .current_static_position_containing_block(),
                                positioning_containing_block_source:
                                    active_positioning_containing_block.map(
                                        BorrowedInlinePositioningContainingBlockSource::into_owned,
                                    ),
                                hypothetical_ancestor_offset: placement.visual_offset,
                                content: DeferredStaticPositionedContent::Dom,
                                static_position_index: output.len(),
                            });
                            continue;
                        }
                        let positioning_containing_block_source =
                            inline_style_establishes_positioning_containing_block(style).then(
                                || InlinePositioningContainingBlockSource {
                                    id: InlinePositioningContainingBlockId(output.len()),
                                    style: Box::new(self.style_with_current_used_lengths(style)),
                                },
                            );
                        self.layout_positioned_inline_descendant(
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                            None,
                            if child_style.abspos_static_source.is_inline_level()
                                || child_style.display.is_inline_level()
                            {
                                line_formatting_context_style
                            } else {
                                style
                            },
                            style,
                            positioning_containing_block_source
                                .as_ref()
                                .map(InlinePositioningContainingBlockSource::as_borrowed),
                            None,
                            None,
                            placement.visual_offset,
                            output,
                        );
                        continue;
                    }
                    if !participates_in_inline_collection {
                        continue;
                    }
                    let link = child_element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_is_atomic_inline = child_style.display.is_atomic_inline()
                        || (is_replaced_element(child_element)
                            && child_style.display.is_inline_level());
                    let child_placement = placement
                        // Atomic inline boxes resolve their own baseline from
                        // their margin box below. Applying the generic inline
                        // box shift here as well would align the same
                        // `vertical-align` value twice.
                        // <https://www.w3.org/TR/css-inline-3/#atomic-inline>
                        .with_added_baseline_placement(if !child_is_atomic_inline {
                            self.vertical_align_baseline_shift_for_inline_style(&child_style, style)
                        } else {
                            InlineBaselinePlacement::from_inherited_glyph_displacement(
                                glyph_baseline_displacement_pt(0.0),
                            )
                        })
                        .with_added_visual_offset(
                            self.inline_visual_offset_for_style(&child_style),
                        );
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &style.text_decoration_origins.effective_layers_vec(),
                        &child_style,
                    );
                    apply_propagated_decoration_layers(&mut child_style, &decoration_layers);
                    // The inline collector recurses through the source DOM
                    // rather than frozen formatting boxes. Keep the current
                    // child in the selector ancestry for all of its
                    // descendants so child combinators remain direct-child
                    // combinators instead of degrading to descendant matches.
                    // <https://drafts.csswg.org/selectors-4/#child-combinators>
                    self.with_ancestor_signature(child_signature.clone(), |layout| {
                        // Atomic inline boxes establish an independent formatting
                        // context even when the parent block is using the DOM
                        // inline collector. Rebuild their frozen child stream at
                        // this boundary so their descendants, own decorations,
                        // and exported baseline remain one atomic line item.
                        // Flattening a nonempty inline-block into an inline scope
                        // loses that boundary and drops its captured paint.
                        // <https://www.w3.org/TR/css-display-3/#atomic-inline>
                        // <https://www.w3.org/TR/css-inline-3/#inline-boxes>
                        if child_is_atomic_inline {
                            let child_boxes = layout
                                .build_frozen_child_boxes_with_current_ancestors(
                                    child_element,
                                    stylesheets,
                                    &child_style,
                                );
                            let built_table_fragment;
                            let table_fragment = if child_style.display.is_table() {
                                built_table_fragment = box_tree::build_frozen_table_fragment(
                                    child_element,
                                    &child_signature,
                                    &child_style,
                                    &child_boxes,
                                );
                                Some(&built_table_fragment)
                            } else {
                                None
                            };
                            let counter_scope =
                                layout.begin_counter_scope(child_element, &child_style);
                            let atom = layout.inline_atom_for_element(
                                child_element,
                                &child_signature,
                                &child_style,
                                &child_boxes,
                                table_fragment,
                                stylesheets,
                                child_placement.baseline_shift(),
                                child_placement.visual_offset,
                                link.clone(),
                            );
                            layout.end_counter_scope(counter_scope);
                            if let Some(mut atom) = atom {
                                atom.baseline_shift += layout
                                    .vertical_align_baseline_shift_for_atom(&atom, style)
                                    .glyph_displacement()
                                    .get();
                                output.push(InlineItem::Atom(Box::new(atom)));
                            }
                            return;
                        }
                        let scope = layout.begin_inline_element_scope(
                            child_element,
                            &child_style,
                            link.clone(),
                            child_placement,
                            InlineElementScopeOptions::DOM_PAINT.with_preserved_empty_metrics(
                                empty_inline_scope_has_distinct_metrics(style, &child_style),
                            ),
                            output,
                        );
                        let scope_positioning_containing_block =
                            scope.positioning_containing_block_source();
                        let next_positioning_containing_block =
                            if inline_style_establishes_positioning_containing_block(&child_style) {
                                scope_positioning_containing_block
                            } else {
                                active_positioning_containing_block
                            };
                        let scope_establishes_positioned_containing_block =
                            scope_positioning_containing_block.is_some();
                        let mut scope_deferred_positioned_descendants = Vec::new();
                        let mut scope_deferred_static_positioned_descendants = Vec::new();
                        layout.push_generated_pseudo_items(
                            child_element,
                            &child_style,
                            child_style.before_style.as_deref(),
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            GeneratedPseudoCounterMode::Commit,
                            output,
                        );
                        if child_style.content.is_generated() {
                            layout.push_element_content_items_from_dom_with_positioned_descendants(
                                child_element,
                                &child_style,
                                line_formatting_context_style,
                                stylesheets,
                                link.clone(),
                                child_placement,
                                next_positioning_containing_block,
                                if scope_establishes_positioned_containing_block {
                                    Some(&mut scope_deferred_positioned_descendants)
                                } else {
                                    deferred_positioned_descendants.as_deref_mut()
                                },
                                Some(&mut scope_deferred_static_positioned_descendants),
                                output,
                            );
                        } else {
                            layout.collect_inline_items_with_positioned_descendants(
                                child_element,
                                &child_style,
                                line_formatting_context_style,
                                stylesheets,
                                link.clone(),
                                child_placement,
                                next_positioning_containing_block,
                                if scope_establishes_positioned_containing_block {
                                    Some(&mut scope_deferred_positioned_descendants)
                                } else {
                                    deferred_positioned_descendants.as_deref_mut()
                                },
                                Some(&mut scope_deferred_static_positioned_descendants),
                                output,
                            );
                        }
                        layout.push_generated_pseudo_items(
                            child_element,
                            &child_style,
                            child_style.after_style.as_deref(),
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            GeneratedPseudoCounterMode::Commit,
                            output,
                        );
                        layout.end_inline_element_scope(scope, &child_style, output);
                        layout.layout_deferred_inline_static_positioned_descendants(
                            scope_deferred_static_positioned_descendants,
                            stylesheets,
                            output,
                        );
                        if scope_establishes_positioned_containing_block {
                            layout.layout_deferred_inline_positioned_descendants(
                                scope_deferred_positioned_descendants,
                                stylesheets,
                                style,
                                output,
                            );
                        }
                    });
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn collect_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
        output: &mut Vec<InlineItem>,
    ) {
        let mut deferred_static_positioned_descendants = Vec::new();
        self.collect_inline_box_items_with_float_containing_block(
            children,
            stylesheets,
            inherited_link,
            baseline_shift,
            visual_offset,
            block_style,
            block_style,
            block_style,
            propagated_decoration_layers,
            None,
            None,
            Some(&mut deferred_static_positioned_descendants),
            output,
        );
        self.layout_deferred_inline_static_positioned_descendants(
            deferred_static_positioned_descendants,
            stylesheets,
            output,
        );
    }

    /// Collect the final inline-box representation without committing its
    /// positioned descendants or other layout side effects.
    ///
    /// This is deliberately not the intrinsic collector: atomic inline
    /// construction, inline edges, whitespace inputs, and bidi controls must
    /// match the stream final line layout would select. The snapshot restores
    /// counter, paint, and positioning state after collection; positioned
    /// descendants are preserved as recipes for the final replay.
    /// <https://drafts.csswg.org/css-inline-3/#line-box>
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn collect_frozen_inline_replay_input(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
    ) -> FrozenInlineReplayInput {
        let snapshot = self.snapshot();
        let mut items = Vec::new();
        let mut deferred_static_positioned_descendants = Vec::new();
        self.with_positioned_layout_suppressed(|layout| {
            layout.collect_inline_box_items_with_float_containing_block(
                children,
                stylesheets,
                inherited_link,
                baseline_shift,
                visual_offset,
                block_style,
                block_style,
                block_style,
                propagated_decoration_layers,
                None,
                None,
                Some(&mut deferred_static_positioned_descendants),
                &mut items,
            );
        });
        self.restore(snapshot);
        let eligibility =
            if block_style.overflow_x.is_scrollable() || block_style.overflow_y.is_scrollable() {
                FrozenInlineReplayEligibility::EstablishesScrollContainer
            } else if items
                .iter()
                .all(|item| matches!(item, InlineItem::Word(_) | InlineItem::Atom(_)))
            {
                FrozenInlineReplayEligibility::Eligible
            } else {
                FrozenInlineReplayEligibility::HasInlineFlowEffects
            };
        FrozenInlineReplayInput {
            items,
            deferred_static_positioned_descendants,
            eligibility,
        }
    }

    /// Materialize the out-of-flow portion of a frozen replay only after the
    /// selected orthogonal line stack has established its containing box.
    pub(in crate::layout) fn layout_frozen_inline_replay_positioned_descendants(
        &mut self,
        input: &FrozenInlineReplayInput,
        stylesheets: &Stylesheets<'_>,
    ) {
        self.layout_deferred_inline_static_positioned_descendants(
            input.deferred_static_positioned_descendants.clone(),
            stylesheets,
            &input.items,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_inline_box_items_with_float_containing_block(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        parent_baseline_style: &ComputedStyle,
        static_position_container_style: &ComputedStyle,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
        active_float_containing_block: Option<BorrowedInlinePositioningContainingBlockSource<'_>>,
        mut deferred_positioned_descendants: Option<&mut Vec<DeferredInlinePositionedDescendant>>,
        mut deferred_static_positioned_descendants: Option<
            &mut Vec<DeferredInlineStaticPositionedDescendant>,
        >,
        output: &mut Vec<InlineItem>,
    ) {
        #[cfg(all(feature = "stack-profile", target_os = "macos"))]
        let mut stack_profile_scope =
            stack_profile::enter("collect_inline_box_items_with_float_containing_block");
        for (child_index, child) in children.iter().enumerate() {
            #[cfg(all(feature = "stack-profile", target_os = "macos"))]
            stack_profile_scope.set_source_index(child_index);
            #[cfg(not(all(feature = "stack-profile", target_os = "macos")))]
            let _ = child_index;
            if let Some((element, _, style, child_boxes)) = child.element_parts()
                && matches!(style.position, Position::Absolute | Position::Fixed)
            {
                if positioned_descendant_has_explicit_inset(style)
                    && let Some(containing_block_source) = active_float_containing_block
                    && let Some(deferred) = deferred_positioned_descendants.as_deref_mut()
                {
                    deferred.push(DeferredInlinePositionedDescendant {
                        element: element.clone(),
                        style: style.clone(),
                        static_position_container_style: static_position_container_style.clone(),
                        containing_block_source: containing_block_source.into_owned(),
                    });
                    continue;
                }
                if !(style.abspos_static_source.is_atomic_inline()
                    || style.display.is_atomic_inline())
                    && let Some(deferred) = deferred_static_positioned_descendants.as_deref_mut()
                {
                    deferred.push(DeferredInlineStaticPositionedDescendant {
                        element: element.clone(),
                        style: style.clone(),
                        line_formatting_context_style: block_style.clone(),
                        static_position_container_style: static_position_container_style.clone(),
                        static_position_containing_block: self
                            .current_static_position_containing_block(),
                        positioning_containing_block_source: active_float_containing_block
                            .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                        hypothetical_ancestor_offset: visual_offset,
                        content: DeferredStaticPositionedContent::Frozen,
                        static_position_index: output.len(),
                    });
                    continue;
                }
                let table_fragment = match child {
                    box_tree::FormattingBox::AtomicInline(box_) => box_.table_fragment.as_ref(),
                    box_tree::FormattingBox::Table(box_) => Some(&box_.fragment),
                    _ => None,
                };
                self.layout_positioned_inline_descendant(
                    element,
                    style,
                    stylesheets,
                    Some(child_boxes),
                    table_fragment,
                    block_style,
                    static_position_container_style,
                    active_float_containing_block,
                    None,
                    None,
                    visual_offset,
                    output,
                );
                continue;
            }
            if let Some((element, signature, style, _)) = child.element_parts()
                && style.float != Float::None
            {
                output.push(InlineItem::Float(Box::new(InlineFloat::new(
                    element.clone(),
                    signature.clone(),
                    style.clone(),
                    style.content.is_generated(),
                    child
                        .element_core()
                        .and_then(|core| generated_pseudo_counter_source(&core.source)),
                    active_float_containing_block
                        .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                ))));
                continue;
            }
            if let box_tree::FormattingBox::Block(box_) = child
                && matches!(&box_.core.source, box_tree::BoxSource::GeneratedPseudo(_))
            {
                // CSS Pseudo-Elements tree-abiding generated content can
                // generate block-level boxes. Even empty block pseudos create
                // block boundaries in an inline formatting context, such as
                // `dt::before { content: ""; display: block }`.
                if output
                    .last()
                    .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                self.collect_inline_box_items_with_float_containing_block(
                    &box_.core.children,
                    stylesheets,
                    inherited_link.clone(),
                    baseline_shift,
                    visual_offset,
                    block_style,
                    parent_baseline_style,
                    static_position_container_style,
                    propagated_decoration_layers_for_child(
                        &propagated_decoration_layers,
                        &box_.core.style,
                    ),
                    active_float_containing_block,
                    deferred_positioned_descendants.as_deref_mut(),
                    deferred_static_positioned_descendants.as_deref_mut(),
                    output,
                );
                if formatting_box_has_inline_content(&box_.core.children)
                    && output
                        .last()
                        .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                continue;
            }
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    let mut text_style = box_tree::owned_style(&box_.style);
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &propagated_decoration_layers,
                        &text_style,
                    );
                    apply_propagated_decoration_layers(&mut text_style, &decoration_layers);
                    self.push_inline_words(
                        &box_.text,
                        &text_style,
                        inherited_link.clone(),
                        baseline_shift,
                        visual_offset,
                        output,
                    );
                }
                box_tree::FormattingBox::Inline(box_) => {
                    let footnote_call = matches!(
                        &box_.core.source,
                        box_tree::BoxSource::GeneratedPseudo(pseudo)
                            if pseudo.kind == box_tree::GeneratedPseudoKind::FootnoteCall
                    )
                    .then_some(box_.core.element.id);
                    let principal_source =
                        matches!(&box_.core.source, box_tree::BoxSource::Principal);
                    if principal_source {
                        self.capture_suppressed_named_strings_before(box_.core.element.id);
                    }
                    if box_.core.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            box_.core.element.clone(),
                            box_.core.signature.clone(),
                            (*box_.core.style).clone(),
                            box_.core.style.content.is_generated(),
                            generated_pseudo_counter_source(&box_.core.source),
                            active_float_containing_block
                                .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                        ))));
                        continue;
                    }
                    let mut inline_style = box_tree::owned_style(&box_.core.style);
                    inline_style.suppress_inapplicable_transform();
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &propagated_decoration_layers,
                        &inline_style,
                    );
                    apply_propagated_decoration_layers(&mut inline_style, &decoration_layers);
                    // A ruby text container containing only out-of-flow
                    // descendants has no anonymous annotation box, but its
                    // descendants still need the normal positioned/float
                    // collection path below. Suppressing this source box
                    // outright would lose an abspos descendant and its
                    // static-position containing block.
                    // <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
                    // Only defer ruby materialization when a first-letter
                    // pseudo actually needs to select from the base level.
                    // A ruby at the beginning of an ordinary line must still
                    // go through paired base/annotation layout; otherwise
                    // the generic inline recursion flattens its annotations
                    // into parent text.
                    // <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
                    // <https://drafts.csswg.org/css-pseudo-4/#first-letter-pseudo>
                    let ruby_can_own_first_letter = inline_style.display.is_ruby()
                        && block_style.first_letter_style.is_some()
                        && !output.iter().any(inline_item_has_typographic_content);
                    let child_placement = InlinePlacement::new(baseline_shift, visual_offset)
                        .with_added_baseline_placement(
                            self.vertical_align_baseline_shift_for_inline_style(
                                &inline_style,
                                parent_baseline_style,
                            ),
                        )
                        .with_added_visual_offset(
                            self.inline_visual_offset_for_style(&inline_style),
                        );
                    // A ruby formatting context contributes its bases to the
                    // parent inline stream while annotations are sidecars.
                    // Materialize the normalized in-flow levels as a coupled
                    // ruby atom until the graph gains per-column break nodes;
                    // this keeps annotations out of ordinary parent text,
                    // spacing, and justification. Positioned and floated
                    // descendants stay on the generic scope path below so
                    // their containing-block ownership remains intact.
                    // <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
                    if inline_style.display.is_ruby() {
                        let out_of_flow_overlay =
                            ruby_has_out_of_flow_descendant(&box_.core.children).then(|| {
                                ruby_out_of_flow_overlay(&box_tree::FormattingBox::Inline(
                                    box_.clone(),
                                ))
                            });
                        if self.collect_normalized_ruby_items(
                            &box_.core.children,
                            &inline_style,
                            stylesheets,
                            box_.core
                                .element
                                .attrs
                                .get("href")
                                .cloned()
                                .or_else(|| inherited_link.clone()),
                            child_placement,
                            block_style,
                            ruby_can_own_first_letter
                                .then_some(block_style.first_letter_style.as_deref())
                                .flatten(),
                            decoration_layers.clone(),
                            output,
                        ) {
                            if let Some(out_of_flow_overlay) = out_of_flow_overlay {
                                self.collect_inline_box_items_with_float_containing_block(
                                    std::slice::from_ref(&out_of_flow_overlay),
                                    stylesheets,
                                    inherited_link.clone(),
                                    child_placement.baseline_shift(),
                                    child_placement.visual_offset,
                                    block_style,
                                    &inline_style,
                                    static_position_container_style,
                                    decoration_layers.clone(),
                                    active_float_containing_block,
                                    deferred_positioned_descendants.as_deref_mut(),
                                    deferred_static_positioned_descendants.as_deref_mut(),
                                    output,
                                );
                            }
                            if principal_source {
                                self.capture_suppressed_named_strings_after(box_.core.element.id);
                            }
                            continue;
                        }
                    }
                    // A principal HTML `br` is a semantic forced line break,
                    // not merely the UA `::before` newline used as its
                    // fallback representation.  Box-tree collection reaches
                    // this path after pseudo generation, so recognize the
                    // principal box directly; author `br::before { content:
                    // none }` must not erase the HTML line boundary.
                    // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
                    if principal_source && is_line_break_element(box_.core.element) {
                        output.push(InlineItem::Break(InlineBreak {
                            clear: inline_style.clear,
                            origin: InlineBreakOrigin::Explicit,
                        }));
                        self.capture_suppressed_named_strings_after(box_.core.element.id);
                        continue;
                    }
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let contents_generated_pseudo = inline_style.display.is_contents()
                        && matches!(&box_.core.source, box_tree::BoxSource::GeneratedPseudo(_));
                    // `display: contents` retains generated content but
                    // suppresses the pseudo-element's principal box. Do not
                    // open an inline paint/positioning scope for it: borders,
                    // backgrounds, margins, and inline-box geometry belong to
                    // the absent box, not its generated text or images.
                    // <https://drafts.csswg.org/css-display-3/#valdef-display-contents>
                    let scope = (!contents_generated_pseudo).then(|| {
                        self.begin_inline_element_scope(
                            box_.core.element,
                            &inline_style,
                            link.clone(),
                            child_placement,
                            InlineElementScopeOptions::BOX_PAINT
                                .with_fragment_edges(box_.fragment_edges)
                                .with_preserved_empty_metrics(
                                    empty_inline_scope_has_distinct_metrics(
                                        block_style,
                                        &inline_style,
                                    ),
                                ),
                            output,
                        )
                    });
                    let scope_positioning_containing_block = scope
                        .as_ref()
                        .and_then(InlineElementScopeState::positioning_containing_block_source);
                    let next_float_containing_block =
                        if inline_style_establishes_positioning_containing_block(&inline_style) {
                            scope_positioning_containing_block
                        } else {
                            active_float_containing_block
                        };
                    let scope_establishes_positioned_containing_block =
                        scope_positioning_containing_block.is_some();
                    let ruby_positioning_source = (inline_style.display.is_ruby()
                        || inline_style.display.is_ruby_internal())
                    .then(|| {
                        scope_positioning_containing_block
                            .map(BorrowedInlinePositioningContainingBlockSource::into_owned)
                    })
                    .flatten();
                    let mut scope_deferred_positioned_descendants = Vec::new();
                    let inlinified_ruby_children =
                        inline_style.display.is_ruby_internal().then(|| {
                            crate::layout::ruby::inlinified_direct_children(&box_.core.children)
                        });
                    let inline_children = inlinified_ruby_children
                        .as_deref()
                        .unwrap_or(&box_.core.children);
                    if inline_style.content.is_generated() {
                        let generated_pseudo_content_style =
                            matches!(&box_.core.source, box_tree::BoxSource::GeneratedPseudo(_))
                                .then(|| generated_pseudo_inline_content_style(&inline_style));
                        let content_style = generated_pseudo_content_style
                            .as_ref()
                            .unwrap_or(&inline_style);
                        let start_len = output.len();
                        self.push_element_content_items_from_boxes(
                            box_.core.element,
                            content_style,
                            match &box_.core.source {
                                box_tree::BoxSource::GeneratedPseudo(pseudo) => {
                                    pseudo.kind.counter_event_source()
                                }
                                box_tree::BoxSource::Principal => {
                                    box_tree::CounterEventSource::Principal
                                }
                            },
                            inline_children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            block_style,
                            decoration_layers.clone(),
                            output,
                        );
                        let clear = generated_content_originating_clear(&box_.core.source)
                            .unwrap_or(inline_style.clear);
                        annotate_line_break_element_breaks_with_clear(
                            box_.core.element,
                            clear,
                            output,
                            start_len,
                        );
                        if let Some(footnote_call) = footnote_call {
                            mark_inline_items_as_footnote_call(
                                &mut output[start_len..],
                                footnote_call,
                            );
                        }
                    } else {
                        self.collect_inline_box_items_with_float_containing_block(
                            inline_children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            block_style,
                            &inline_style,
                            &inline_style,
                            decoration_layers,
                            next_float_containing_block,
                            if scope_establishes_positioned_containing_block {
                                Some(&mut scope_deferred_positioned_descendants)
                            } else {
                                deferred_positioned_descendants.as_deref_mut()
                            },
                            deferred_static_positioned_descendants.as_deref_mut(),
                            output,
                        );
                    }
                    if let Some(scope) = scope {
                        self.end_inline_element_scope(scope, &inline_style, output);
                    }
                    if scope_establishes_positioned_containing_block {
                        let deferred_element_ids = scope_deferred_positioned_descendants
                            .iter()
                            .map(|descendant| descendant.element.id)
                            .collect::<Vec<_>>();
                        self.layout_deferred_inline_positioned_descendants(
                            scope_deferred_positioned_descendants,
                            stylesheets,
                            block_style,
                            output,
                        );
                        if let Some(source) = ruby_positioning_source.as_ref() {
                            self.layout_undeferred_ruby_positioned_descendants(
                                &box_.core.children,
                                stylesheets,
                                block_style,
                                source,
                                &deferred_element_ids,
                                output,
                            );
                        }
                    }
                    if principal_source {
                        self.capture_suppressed_named_strings_after(box_.core.element.id);
                    }
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    let principal_source =
                        matches!(&box_.core.source, box_tree::BoxSource::Principal);
                    if principal_source && is_line_break_element(box_.core.element) {
                        // HTML keeps `<br>` as a forced line-break control
                        // even when an author declaration gives its principal
                        // box an atomic inline display. Treating that box as
                        // an ordinary atom would import the generated newline
                        // as an internal line, giving the break a height and a
                        // baseline of its own before a following float.
                        // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
                        self.capture_suppressed_named_strings_before(box_.core.element.id);
                        output.push(InlineItem::Break(InlineBreak {
                            clear: box_.core.style.clear,
                            origin: InlineBreakOrigin::Explicit,
                        }));
                        self.capture_suppressed_named_strings_after(box_.core.element.id);
                        continue;
                    }
                    if box_.core.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            box_.core.element.clone(),
                            box_.core.signature.clone(),
                            (*box_.core.style).clone(),
                            box_.core.style.content.is_generated(),
                            generated_pseudo_counter_source(&box_.core.source),
                            active_float_containing_block
                                .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                        ))));
                        continue;
                    }
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset =
                        visual_offset.plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.inline_atom_for_element(
                        box_.core.element,
                        &box_.core.signature,
                        &box_.core.style,
                        &box_.core.children,
                        box_.table_fragment.as_ref(),
                        stylesheets,
                        baseline_shift,
                        atom_visual_offset,
                        link.clone(),
                    );
                    self.end_counter_scope(counter_scope);
                    if let Some(mut atom) = atom {
                        atom.baseline_shift += self
                            .vertical_align_baseline_shift_for_atom(&atom, parent_baseline_style)
                            .glyph_displacement()
                            .get();
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.core.element, &box_.core.style);
                        self.push_inline_words(
                            &text,
                            &box_.core.style,
                            link,
                            baseline_shift,
                            atom_visual_offset,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::Table(box_)
                    if box_.core.style.display.is_inline_level() =>
                {
                    // The table tree retains its durable `Table` variant
                    // independently of its outer display. An `inline-table`
                    // is nevertheless an atomic inline in this formatting
                    // context, so collecting it as a block drops both its
                    // intrinsic inline contribution and exported baseline.
                    // <https://drafts.csswg.org/css-display-3/#valdef-display-inline-table>
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset =
                        visual_offset.plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.inline_atom_for_element(
                        box_.core.element,
                        &box_.core.signature,
                        &box_.core.style,
                        &box_.core.children,
                        Some(&box_.fragment),
                        stylesheets,
                        baseline_shift,
                        atom_visual_offset,
                        link.clone(),
                    );
                    self.end_counter_scope(counter_scope);
                    if let Some(mut atom) = atom {
                        atom.baseline_shift += self
                            .vertical_align_baseline_shift_for_atom(&atom, parent_baseline_style)
                            .glyph_displacement()
                            .get();
                        output.push(InlineItem::Atom(Box::new(atom)));
                    }
                }
                box_tree::FormattingBox::Replaced(box_) => {
                    // Replaced boxes are atomic inline-level boxes. They
                    // participate in the same line construction, float
                    // blockification, baseline calculation, and principal
                    // paint path as an `AtomicInline` box; dropping them
                    // here makes a normal-flow block or float lose inline
                    // images, canvases, and other replaced content.
                    // <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>
                    // <https://www.w3.org/TR/css-display-3/#replaced-element>
                    if box_.core.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            box_.core.element.clone(),
                            box_.core.signature.clone(),
                            (*box_.core.style).clone(),
                            box_.core.style.content.is_generated(),
                            generated_pseudo_counter_source(&box_.core.source),
                            active_float_containing_block
                                .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                        ))));
                        continue;
                    }
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset =
                        visual_offset.plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.inline_atom_for_element(
                        box_.core.element,
                        &box_.core.signature,
                        &box_.core.style,
                        &box_.core.children,
                        None,
                        stylesheets,
                        baseline_shift,
                        atom_visual_offset,
                        link.clone(),
                    );
                    self.end_counter_scope(counter_scope);
                    if let Some(mut atom) = atom {
                        atom.baseline_shift += self
                            .vertical_align_baseline_shift_for_atom(&atom, parent_baseline_style)
                            .glyph_displacement()
                            .get();
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.core.element, &box_.core.style);
                        self.push_inline_words(
                            &text,
                            &box_.core.style,
                            link,
                            baseline_shift,
                            atom_visual_offset,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self
                    .collect_inline_box_items_with_float_containing_block(
                        &box_.children,
                        stylesheets,
                        inherited_link.clone(),
                        baseline_shift,
                        visual_offset,
                        block_style,
                        parent_baseline_style,
                        static_position_container_style,
                        propagated_decoration_layers_for_child(
                            &propagated_decoration_layers,
                            &box_.style,
                        ),
                        active_float_containing_block,
                        deferred_positioned_descendants.as_deref_mut(),
                        deferred_static_positioned_descendants.as_deref_mut(),
                        output,
                    ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Flex(_) => {}
                box_tree::FormattingBox::Table(_) => {}
            }
        }
    }
}
