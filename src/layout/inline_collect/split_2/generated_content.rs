use super::*;
use crate::layout::inline_layout::InlineLineStackCursor;

/// An explicitly inset positioned descendant held until its positioned inline
/// ancestor has emitted the final edge that completes its containing block.
///
/// The request owns only DOM and computed-style inputs. Its frozen child boxes
/// are rebuilt when the scope closes, avoiding a borrowed formatting-tree
/// lifetime in the inline collection state.
/// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>
#[derive(Clone)]
struct DeferredInlinePositionedDescendant {
    element: Element,
    style: ComputedStyle,
    containing_block_source: InlinePositioningContainingBlockSource,
}

fn positioned_descendant_has_explicit_inset(style: &ComputedStyle) -> bool {
    [
        &style.box_values.inset_top,
        &style.box_values.inset_right,
        &style.box_values.inset_bottom,
        &style.box_values.inset_left,
    ]
    .into_iter()
    .any(|inset| !matches!(inset, css::ComputedLengthPercentageOrAuto::Auto))
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn push_element_content_items_from_dom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        self.push_element_content_items_from_dom_with_positioned_descendants(
            element,
            style,
            stylesheets,
            inherited_link,
            placement,
            None,
            None,
            output,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn push_element_content_items_from_dom_with_positioned_descendants(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        active_positioning_containing_block: Option<&InlinePositioningContainingBlockSource>,
        mut deferred_positioned_descendants: Option<&mut Vec<DeferredInlinePositionedDescendant>>,
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
                        stylesheets,
                        inherited_link.clone(),
                        placement,
                        active_positioning_containing_block,
                        deferred_positioned_descendants.as_deref_mut(),
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
                placement.baseline_shift,
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
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration: css::TextDecoration,
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
                        propagated_decoration.clone(),
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
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        self.collect_inline_items_with_positioned_descendants(
            element,
            style,
            stylesheets,
            inherited_link,
            placement,
            None,
            None,
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
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        active_positioning_containing_block: Option<&InlinePositioningContainingBlockSource>,
        mut deferred_positioned_descendants: Option<&mut Vec<DeferredInlinePositionedDescendant>>,
        output: &mut Vec<InlineItem>,
    ) {
        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        for child in &element.children {
            match &child.kind {
                NodeKind::Text(text) => {
                    if element_suppresses_direct_text_children(element) {
                        continue;
                    }
                    self.push_inline_words(
                        text,
                        style,
                        inherited_link.clone(),
                        placement.baseline_shift,
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
                    let child_signature = ElementSignature::with_sibling_list(
                        child_element.tag.clone(),
                        child_element.attrs.clone(),
                        element_index,
                        sibling_tags.clone(),
                    );
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
                    if child_style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            child_element.clone(),
                            child_signature,
                            child_style,
                            false,
                            inline_style_establishes_positioning_containing_block(style).then(
                                || InlinePositioningContainingBlockSource {
                                    id: InlinePositioningContainingBlockId(output.len()),
                                    style: style.clone(),
                                },
                            ),
                        ))));
                        continue;
                    }
                    // This DOM collector owns only inline-formatting content.
                    // Block/table source boxes are represented by frozen
                    // formatting boxes and their positioned descendants must
                    // be laid out from that owner exactly once.
                    // <https://www.w3.org/TR/css-display-3/#inlinification>
                    let participates_in_inline_collection = !child_style.display.is_none()
                        && !child_style.display.is_block_level()
                        && !child_style.display.is_table();
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
                                containing_block_source: containing_block_source.clone(),
                            });
                            continue;
                        }
                        let positioning_containing_block_source =
                            inline_style_establishes_positioning_containing_block(style).then(
                                || InlinePositioningContainingBlockSource {
                                    id: InlinePositioningContainingBlockId(output.len()),
                                    style: style.clone(),
                                },
                            );
                        self.layout_positioned_inline_descendant(
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                            None,
                            style,
                            positioning_containing_block_source.as_ref(),
                            output,
                        );
                        continue;
                    }
                    child_style.text_decoration = child_style
                        .text_decoration
                        .with_propagated_lines(style.text_decoration.clone());
                    if !participates_in_inline_collection {
                        continue;
                    }
                    let link = child_element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_placement = placement
                        .with_added_baseline_shift(
                            self.vertical_align_baseline_shift_for_inline_style(
                                &child_style,
                                style,
                            ),
                        )
                        .with_added_visual_offset(
                            self.inline_visual_offset_for_style(&child_style),
                        );
                    // An empty `inline-block` still establishes an atomic
                    // inline-level box.  It must not be represented only by
                    // transparent inline-scope edge markers: doing so drops
                    // its explicit dimensions and background, particularly
                    // after a forced break where no text fragment can keep
                    // the scope alive.  Content-bearing atomic inline boxes
                    // are collected through their frozen formatting boxes;
                    // this direct-DOM path supplies the corresponding empty
                    // principal box.
                    // <https://www.w3.org/TR/css-display-3/#atomic-inline>
                    // <https://www.w3.org/TR/css-inline-3/#inline-boxes>
                    if child_style.display.is_atomic_inline()
                        && child_element.children.is_empty()
                        && !child_style.content.is_generated()
                        && child_style.before_style.is_none()
                        && child_style.after_style.is_none()
                    {
                        let counter_scope = self.begin_counter_scope(child_element, &child_style);
                        let atom = self.inline_atom_for_element(
                            child_element,
                            &child_signature,
                            &child_style,
                            &[],
                            None,
                            stylesheets,
                            child_placement.baseline_shift,
                            child_placement.visual_offset,
                            link.clone(),
                        );
                        self.end_counter_scope(counter_scope);
                        if let Some(mut atom) = atom {
                            atom.baseline_shift +=
                                self.vertical_align_baseline_shift_for_atom(&atom, style);
                            output.push(InlineItem::Atom(Box::new(atom)));
                        }
                        continue;
                    }
                    let scope = self.begin_inline_element_scope(
                        child_element,
                        &child_style,
                        link.clone(),
                        child_placement,
                        InlineElementScopeOptions::DOM_PAINT,
                        output,
                    );
                    let next_positioning_containing_block =
                        if inline_style_establishes_positioning_containing_block(&child_style) {
                            scope.positioning_containing_block_source.as_ref()
                        } else {
                            active_positioning_containing_block
                        };
                    let scope_establishes_positioned_containing_block =
                        scope.positioning_containing_block_source.is_some();
                    let mut scope_deferred_positioned_descendants = Vec::new();
                    self.push_generated_pseudo_items(
                        child_element,
                        &child_style,
                        child_style.before_style.as_deref(),
                        link.clone(),
                        child_placement.baseline_shift,
                        child_placement.visual_offset,
                        GeneratedPseudoCounterMode::Commit,
                        output,
                    );
                    if child_style.content.is_generated() {
                        self.push_element_content_items_from_dom_with_positioned_descendants(
                            child_element,
                            &child_style,
                            stylesheets,
                            link.clone(),
                            child_placement,
                            next_positioning_containing_block,
                            if scope_establishes_positioned_containing_block {
                                Some(&mut scope_deferred_positioned_descendants)
                            } else {
                                deferred_positioned_descendants.as_deref_mut()
                            },
                            output,
                        );
                    } else {
                        self.collect_inline_items_with_positioned_descendants(
                            child_element,
                            &child_style,
                            stylesheets,
                            link.clone(),
                            child_placement,
                            next_positioning_containing_block,
                            if scope_establishes_positioned_containing_block {
                                Some(&mut scope_deferred_positioned_descendants)
                            } else {
                                deferred_positioned_descendants.as_deref_mut()
                            },
                            output,
                        );
                    }
                    self.push_generated_pseudo_items(
                        child_element,
                        &child_style,
                        child_style.after_style.as_deref(),
                        link.clone(),
                        child_placement.baseline_shift,
                        child_placement.visual_offset,
                        GeneratedPseudoCounterMode::Commit,
                        output,
                    );
                    self.end_inline_element_scope(scope, &child_style, output);
                    if scope_establishes_positioned_containing_block {
                        self.layout_deferred_inline_positioned_descendants(
                            scope_deferred_positioned_descendants,
                            stylesheets,
                            style,
                            output,
                        );
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn collect_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration: css::TextDecoration,
        output: &mut Vec<InlineItem>,
    ) {
        self.collect_inline_box_items_with_float_containing_block(
            children,
            stylesheets,
            inherited_link,
            baseline_shift,
            visual_offset,
            block_style,
            propagated_decoration,
            None,
            None,
            output,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_inline_box_items_with_float_containing_block(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration: css::TextDecoration,
        active_float_containing_block: Option<&InlinePositioningContainingBlockSource>,
        mut deferred_positioned_descendants: Option<&mut Vec<DeferredInlinePositionedDescendant>>,
        output: &mut Vec<InlineItem>,
    ) {
        for child in children {
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
                        containing_block_source: containing_block_source.clone(),
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
                    active_float_containing_block,
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
                    active_float_containing_block.cloned(),
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
                    box_.core
                        .style
                        .text_decoration
                        .clone()
                        .with_propagated_lines(propagated_decoration.clone()),
                    active_float_containing_block,
                    deferred_positioned_descendants.as_deref_mut(),
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
                    text_style.text_decoration = text_style
                        .text_decoration
                        .with_propagated_lines(propagated_decoration.clone());
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
                    if matches!(
                        &box_.core.source,
                        box_tree::BoxSource::GeneratedPseudo(pseudo)
                            if pseudo.kind == box_tree::GeneratedPseudoKind::FootnoteCall
                    ) {
                        self.handle_footnote_call(box_.core.element);
                    }
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
                            active_float_containing_block.cloned(),
                        ))));
                        continue;
                    }
                    let mut inline_style = box_tree::owned_style(&box_.core.style);
                    inline_style.text_decoration = inline_style
                        .text_decoration
                        .with_propagated_lines(propagated_decoration.clone());
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_placement = InlinePlacement::new(baseline_shift, visual_offset)
                        .with_added_baseline_shift(
                            self.vertical_align_baseline_shift_for_inline_style(
                                &inline_style,
                                block_style,
                            ),
                        )
                        .with_added_visual_offset(
                            self.inline_visual_offset_for_style(&inline_style),
                        );
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
                                .with_fragment_edges(box_.fragment_edges),
                            output,
                        )
                    });
                    let next_float_containing_block =
                        if inline_style_establishes_positioning_containing_block(&inline_style) {
                            scope
                                .as_ref()
                                .map(|scope| &scope.positioning_containing_block_source)
                                .and_then(Option::as_ref)
                        } else {
                            active_float_containing_block
                        };
                    let scope_establishes_positioned_containing_block = scope
                        .as_ref()
                        .is_some_and(|scope| scope.positioning_containing_block_source.is_some());
                    let mut scope_deferred_positioned_descendants = Vec::new();
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
                            &box_.core.children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift,
                            child_placement.visual_offset,
                            block_style,
                            inline_style.text_decoration.clone(),
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
                    } else {
                        self.collect_inline_box_items_with_float_containing_block(
                            &box_.core.children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift,
                            child_placement.visual_offset,
                            block_style,
                            inline_style.text_decoration.clone(),
                            next_float_containing_block,
                            if scope_establishes_positioned_containing_block {
                                Some(&mut scope_deferred_positioned_descendants)
                            } else {
                                deferred_positioned_descendants.as_deref_mut()
                            },
                            output,
                        );
                    }
                    if let Some(scope) = scope {
                        self.end_inline_element_scope(scope, &inline_style, output);
                    }
                    if scope_establishes_positioned_containing_block {
                        self.layout_deferred_inline_positioned_descendants(
                            scope_deferred_positioned_descendants,
                            stylesheets,
                            block_style,
                            output,
                        );
                    }
                    if principal_source {
                        self.capture_suppressed_named_strings_after(box_.core.element.id);
                    }
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    if box_.core.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            box_.core.element.clone(),
                            box_.core.signature.clone(),
                            (*box_.core.style).clone(),
                            box_.core.style.content.is_generated(),
                            active_float_containing_block.cloned(),
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
                        atom.baseline_shift +=
                            self.vertical_align_baseline_shift_for_atom(&atom, block_style);
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
                            active_float_containing_block.cloned(),
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
                        atom.baseline_shift +=
                            self.vertical_align_baseline_shift_for_atom(&atom, block_style);
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
                        box_.style
                            .text_decoration
                            .clone()
                            .with_propagated_lines(propagated_decoration.clone()),
                        active_float_containing_block,
                        deferred_positioned_descendants.as_deref_mut(),
                        output,
                    ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Table(_)
                | box_tree::FormattingBox::Flex(_) => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_positioned_inline_descendant(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        block_style: &ComputedStyle,
        positioning_containing_block_source: Option<&InlinePositioningContainingBlockSource>,
        output: &[InlineItem],
    ) {
        if self.positioned_inline_layout_suppression_depth > 0 {
            return;
        }
        let source_was_inline_level =
            style.abspos_static_source_was_inline_level || style.display.is_inline_level();
        if source_was_inline_level {
            // A horizontal replaced source inside a principal vertical flow
            // is measured in a scratch physical span. Its normal-flow
            // parent is replayed at the vertical ancestor's block-start, but
            // positioned paint is owned independently and therefore needs the
            // same hypothetical horizontal static rectangle before layout.
            // Block sources retain their own block static-position rules.
            // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
            // <https://www.w3.org/TR/css-position-3/#static-position>
            let previous_principal_static_position = self.absolute_static_position;
            if (is_replaced_element(element) || matches!(element.tag.as_str(), "audio" | "svg"))
                && !block_style.writing_mode.has_vertical_lines()
                && let Some(vertical_parent) = self.block_static_position_contexts.last()
                && vertical_parent.writing_mode.has_vertical_lines()
                && vertical_parent.writing_mode == WritingMode::VerticalRl
            {
                let child_physical_width = (self.content_right - self.content_left).max(0.0);
                let static_x = match block_start_side(vertical_parent.writing_mode) {
                    PhysicalSide::Left => vertical_parent.content_left,
                    PhysicalSide::Right => vertical_parent.content_right - child_physical_width,
                    PhysicalSide::Top | PhysicalSide::Bottom => {
                        unreachable!("a vertical writing mode must have a horizontal block axis")
                    }
                };
                self.absolute_static_position = Some(
                    AbsoluteStaticPosition::from_page_horizontal_position(static_x, static_x),
                );
            }
            let mut positioned_style = style.clone();
            positioned_style.abspos_static_source_was_inline_level = true;
            positioned_style.abspos_static_source_was_atomic_inline =
                style.abspos_static_source_was_atomic_inline || style.display.is_atomic_inline();
            let static_position = self.inline_static_position_from_hypothetical_placeholder(
                element,
                &positioned_style,
                stylesheets,
                child_boxes,
                table_fragment,
                block_style,
                output,
            );
            let has_explicit_inset = [
                &positioned_style.box_values.inset_top,
                &positioned_style.box_values.inset_right,
                &positioned_style.box_values.inset_bottom,
                &positioned_style.box_values.inset_left,
            ]
            .into_iter()
            .any(|inset| !matches!(inset, css::ComputedLengthPercentageOrAuto::Auto));
            let previous_escaped_atom_containing_block = self.escaped_atom_containing_block;
            let positioned_containing_block_scope = has_explicit_inset
                .then_some(positioning_containing_block_source)
                .flatten()
                .and_then(|source| {
                    let mode = PositionedContainingBlockMode::for_style(&source.style)?;
                    let containing_block = self.inline_positioning_containing_block_from_items(
                        source,
                        block_style,
                        output,
                    )?;
                    // An inline-block lays out its contents in a temporary
                    // page before replaying its atom at the final line
                    // position.  This inline source is local to that same
                    // temporary page, so its positioned descendants must
                    // escape with the atom rather than retain the temporary
                    // page coordinates.
                    // <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>
                    if self.escaped_atom_positioning_depth > 0 {
                        self.escaped_atom_containing_block = Some(containing_block);
                    }
                    Some(self.push_positioned_containing_block(mode, containing_block))
                });
            self.out_of_flow_prebreak_suppression_depth += 1;
            self.layout_positioned_block_with_inline_static_position(
                element,
                &positioned_style,
                stylesheets,
                child_boxes,
                table_fragment,
                static_position,
            );
            self.out_of_flow_prebreak_suppression_depth -= 1;
            if let Some(scope) = positioned_containing_block_scope {
                self.pop_positioned_containing_block(scope);
                self.escaped_atom_containing_block = previous_escaped_atom_containing_block;
            }
            self.absolute_static_position = previous_principal_static_position;
            return;
        }

        let static_y_offset = self.block_static_position_y_offset_from_buffer(output, block_style);
        if self.escaped_atom_positioning_depth == 0
            && !output.is_empty()
            && output.iter().all(|item| {
            matches!(item, InlineItem::Word(word) if word.text.chars().all(|character| character == '\u{a0}'))
        })
        {
            // This is a block-level source after an inline-only buffer. Its
            // hypothetical box starts at the block formatting context's
            // inline edge, not after the buffered glyph advance. The latter
            // is the static position for an inline-level source, and moves a
            // block abspos after an NBSP by that glyph's width.
            // <https://www.w3.org/TR/css-position-3/#static-position>
            let previous = self.absolute_static_position;
            self.absolute_static_position = Some(
                AbsoluteStaticPosition::from_page_horizontal_position(
                    self.content_left,
                    self.content_right,
                ),
            );
            self.out_of_flow_prebreak_suppression_depth += 1;
            self.layout_positioned_block_with_block_static_y_offset(
                element,
                style,
                stylesheets,
                child_boxes,
                table_fragment,
                static_y_offset,
            );
            self.out_of_flow_prebreak_suppression_depth -= 1;
            self.absolute_static_position = previous;
            return;
        }
        let previous_escaped_atom_containing_block = self.escaped_atom_containing_block;
        let previous_block_static_rectangle = self.absolute_static_position;
        if block_style.writing_mode.has_vertical_lines() {
            // A block-level abspos descendant participates in the static
            // hypothetical layout even though it is removed from the actual
            // flow. In a vertical parent that hypothetical layout can extend
            // the parent's auto physical block size, so the current collapsed
            // horizontal cursor is not its static-position rectangle.
            //
            // Preserve the rectangle, including the parent's inline-axis
            // span and direction, until the positioned box has its used
            // physical height. CSS Positioned Layout selects the correct
            // inline-start edge only at that point.
            // <https://www.w3.org/TR/css-position-3/#static-position>
            // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
            let child_border_width = style.box_values.width.length_if_no_percent().unwrap_or(0.0)
                + style.padding.left
                + style.padding.right
                + used_border_widths(style).left
                + used_border_widths(style).right;
            let child_block_advance = child_border_width + style.margin.left + style.margin.right;
            let static_context = self
                .block_static_position_contexts
                .last()
                .copied()
                .filter(|context| context.writing_mode == block_style.writing_mode);
            let parent_block_size_is_auto = static_context
                .map(|context| context.physical_block_size_is_auto)
                .unwrap_or_else(|| block_style.box_values.width.is_auto());
            let child_auto_block_advance = if parent_block_size_is_auto {
                child_block_advance
            } else {
                0.0
            };
            let static_x = match block_start_side(
                static_context.map_or(block_style.writing_mode, |context| context.writing_mode),
            ) {
                PhysicalSide::Left => self.content_left + child_auto_block_advance,
                PhysicalSide::Right => self.content_right - child_auto_block_advance,
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    unreachable!("a vertical writing mode must have a horizontal block axis")
                }
            };
            let child_border_height = style
                .box_values
                .height
                .length_if_no_percent()
                .unwrap_or(0.0)
                + style.padding.top
                + style.padding.bottom
                + used_border_widths(style).top
                + used_border_widths(style).bottom;
            let static_top_y = parent_block_size_is_auto.then(|| {
                static_context.map_or(self.cursor_y, |context| {
                    match inline_start_side(context.writing_mode, context.direction) {
                        PhysicalSide::Top => context.content_top_y,
                        PhysicalSide::Bottom => {
                            context.content_top_y - context.content_height + child_border_height
                        }
                        PhysicalSide::Left | PhysicalSide::Right => self.cursor_y,
                    }
                })
            });
            self.absolute_static_position = Some(
                AbsoluteStaticPosition::from_page_rect_with_horizontal_outside(
                    static_x,
                    static_x,
                    static_top_y.unwrap_or(self.cursor_y),
                    true,
                ),
            );
        }
        let positioned_containing_block_scope =
            positioning_containing_block_source.and_then(|source| {
                let mode = PositionedContainingBlockMode::for_style(&source.style)?;
                let containing_block = self.inline_positioning_containing_block_from_items(
                    source,
                    block_style,
                    output,
                )?;
                // See the corresponding inline-level branch above.  The
                // source containing block is expressed in the temporary
                // atom page and therefore moves with that atom on escape.
                if self.escaped_atom_positioning_depth > 0 {
                    self.escaped_atom_containing_block = Some(containing_block);
                }
                Some(self.push_positioned_containing_block(mode, containing_block))
            });
        self.out_of_flow_prebreak_suppression_depth += 1;
        self.layout_positioned_block_with_block_static_y_offset(
            element,
            style,
            stylesheets,
            child_boxes,
            table_fragment,
            static_y_offset,
        );
        self.out_of_flow_prebreak_suppression_depth -= 1;
        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
            self.escaped_atom_containing_block = previous_escaped_atom_containing_block;
        }
        self.absolute_static_position = previous_block_static_rectangle;
    }

    /// Resolves the padding-box rectangle established by a positioned inline.
    ///
    /// Inline collection retains zero-advance edge atoms for positioned
    /// ancestors. Replaying those source markers through normal line
    /// preparation gives the first and last generated inline fragments their
    /// final physical coordinates, including bidi reordering, fragmentation,
    /// and writing-mode transforms. CSS 2.2 defines the absolute containing
    /// block from exactly those padding edges:
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
    fn inline_positioning_containing_block_from_items(
        &mut self,
        source: &InlinePositioningContainingBlockSource,
        block_style: &ComputedStyle,
        output: &[InlineItem],
    ) -> Option<ContainingBlock> {
        let mut items = output.to_vec();
        // The positioned descendant is encountered before its enclosing
        // inline scope emits the end marker. Add that marker only to this
        // hypothetical line sequence so the real source stream remains in
        // DOM order.
        self.push_inline_box_edge_item(
            &source.style,
            InlineBoxEdge::End,
            Some(source.id),
            0.0,
            InlineVisualOffset::zero(),
            None,
            &mut items,
        );
        let available_width = self.current_content_logical_inline_size().max(1.0);
        let snapshot = self.snapshot();
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            block_style,
            available_width,
            0.0,
            0.0,
        );
        self.restore(snapshot);

        let saved_cursor_y = self.cursor_y;
        let saved_left = self.content_left;
        let saved_right = self.content_right;
        let context = sequence.context(block_style);
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        let mut plaintext_direction_state = None;
        let mut stack = InlineLineStackCursor::new(
            block_style,
            self.content_left,
            self.content_right,
            self.cursor_y,
        );
        if matches!(
            block_style.writing_mode,
            WritingMode::VerticalRl | WritingMode::SidewaysRl
        ) {
            stack.advance(records.first().map(|record| record.height()).unwrap_or(0.0));
        }
        let mut start = None;
        let mut end = None;
        let mut generated_fragment_bounds: Option<(f32, f32, f32, f32)> = None;
        for record in &records {
            stack.apply(self);
            self.apply_line_block_start_trim_for_paint(record, block_style.writing_mode);
            if let Some(prepared) =
                self.prepare_inline_line_record(record, context, &mut plaintext_direction_state)
            {
                for item in &prepared.paint_items {
                    if let PreparedInlinePaintItem::FragmentBackground(fragment) = item
                        && fragment.fragment.ancestor_inline_decorations().iter().any(
                            |decoration| {
                                decoration.positioning_containing_block_id == Some(source.id)
                            },
                        )
                    {
                        let rect = fragment.rect;
                        let bounds = (rect.x(), rect.y(), rect.width(), rect.height());
                        generated_fragment_bounds = Some(match generated_fragment_bounds {
                            Some((left, bottom, right, top)) => (
                                left.min(bounds.0),
                                bottom.min(bounds.1),
                                right.max(bounds.0 + bounds.2),
                                top.max(bounds.1 + bounds.3),
                            ),
                            None => (bounds.0, bounds.1, bounds.0 + bounds.2, bounds.1 + bounds.3),
                        });
                    }
                    let PreparedInlinePaintItem::Atom(atom) = item else {
                        continue;
                    };
                    let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) =
                        atom.atom.content()
                    else {
                        continue;
                    };
                    if edge.positioning_containing_block_id != Some(source.id) {
                        continue;
                    }
                    let rect = atom.content_rect;
                    let bounds = (rect.x(), rect.y(), rect.width(), rect.height());
                    match edge.logical_edge {
                        InlineLogicalEdge::Start => {
                            start.get_or_insert(bounds);
                        }
                        InlineLogicalEdge::End => end = Some(bounds),
                    };
                }
            }
            stack.advance(record.height());
        }
        self.cursor_y = saved_cursor_y;
        self.content_left = saved_left;
        self.content_right = saved_right;

        if WritingModeAxes::new(source.style.writing_mode, source.style.direction)
            .swaps_physical_axes()
            && let Some((left, bottom, right, top)) = generated_fragment_bounds
        {
            return Some(ContainingBlock::from_page_top_rect(PageTopRect::new(
                left,
                top,
                (right - left).max(0.0),
                (top - bottom).max(0.0),
            )));
        }

        let (start_x, start_y, start_width, start_height) = start?;
        let (end_x, end_y, end_width, end_height) = end?;
        let left = start_x.min(end_x);
        let right = (start_x + start_width).max(end_x + end_width);
        let bottom = start_y.min(end_y);
        let top = (start_y + start_height).max(end_y + end_height);
        Some(ContainingBlock::from_page_top_rect(PageTopRect::new(
            left,
            top,
            (right - left).max(0.0),
            (top - bottom).max(0.0),
        )))
    }

    fn layout_deferred_inline_positioned_descendants(
        &mut self,
        descendants: Vec<DeferredInlinePositionedDescendant>,
        stylesheets: &[Stylesheet],
        block_style: &ComputedStyle,
        output: &[InlineItem],
    ) {
        for descendant in descendants {
            // Rebuild only at the final inline edge, where its ancestor's
            // containing block can be measured from a complete item stream.
            // This preserves the immutable frozen-tree boundary without
            // carrying borrowed child boxes through collection.
            let child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                &descendant.element,
                stylesheets,
                &descendant.style,
            );
            self.layout_positioned_inline_descendant(
                &descendant.element,
                &descendant.style,
                stylesheets,
                Some(&child_boxes),
                None,
                block_style,
                Some(&descendant.containing_block_source),
                output,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_static_position_from_hypothetical_placeholder(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        block_style: &ComputedStyle,
        output: &[InlineItem],
    ) -> InlineStaticPosition {
        let placeholder = self.inline_static_position_placeholder_atom(
            element,
            style,
            stylesheets,
            child_boxes,
            table_fragment,
        );
        let mut hypothetical_items = Vec::with_capacity(output.len() + 1);
        hypothetical_items.extend_from_slice(output);
        hypothetical_items.push(InlineItem::Atom(Box::new(placeholder)));
        let available_width = self.current_content_logical_inline_size().max(1.0);
        // CSS Positioned Layout defines the static-position rectangle as the
        // box's hypothetical normal-flow position. Carrying a non-painting
        // placeholder through ordinary inline line selection keeps forced
        // breaks, wrapping, and line metrics aligned with the same CSS Text
        // machinery used for real inline content:
        // https://www.w3.org/TR/css-position-3/#staticpos-rect
        // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
        // Static-position resolution is a hypothetical inline layout. Its
        // float placement may build paint fragments and exclusions while
        // fitting the placeholder, but none of those side effects belong to
        // the real inline run that follows.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        // <https://www.w3.org/TR/CSS22/visuren.html#abs-non-replaced-height>
        let snapshot = self.snapshot();
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            hypothetical_items,
            block_style,
            available_width,
            0.0,
            0.0,
        );
        self.restore(snapshot);
        self.inline_static_position_from_placeholder_sequence(&sequence, block_style)
            .unwrap_or_else(|| InlineStaticPosition {
                start_x: self.content_left,
                end_x: self.content_right,
                top_y: self.cursor_y,
                baseline_y: self.inline_static_baseline_y_from_buffer(output, style),
                use_margin_box_top: false,
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_static_position_placeholder_atom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> InlineAtom {
        let available_width = (self.content_right - self.content_left).max(style.font_size);
        let mut placeholder_style = self.style_with_current_viewport_lengths(style);
        apply_used_box_metrics(
            &mut placeholder_style,
            PercentageBasis::definite(layout_pt(available_width)),
        );
        let horizontal_non_content = placeholder_style.padding.left
            + placeholder_style.padding.right
            + horizontal_border_width(&placeholder_style);
        let positioned_available_outer_width =
            (available_width - placeholder_style.margin.left - placeholder_style.margin.right)
                .max(placeholder_style.font_size);
        let content_width = self
            .used_intrinsic_or_shrink_to_fit_width(
                element,
                &placeholder_style,
                stylesheets,
                layout_pt(positioned_available_outer_width),
                non_content_pt(horizontal_non_content),
                child_boxes,
                table_fragment,
            )
            .points();
        let border_box_width = content_width + horizontal_non_content;
        let vertical_non_content = placeholder_style.padding.top
            + placeholder_style.padding.bottom
            + vertical_border_width(&placeholder_style);
        let containing_block_height = self
            .definite_block_size_stack
            .last()
            .cloned()
            .unwrap_or_else(PercentageBasis::indefinite);
        let content_height = used_content_box_height_or_auto_with_basis(
            &placeholder_style,
            containing_block_height,
            non_content_pt(vertical_non_content),
        )
        .map(|height| {
            constrain_content_height(
                &placeholder_style,
                height,
                PercentageBasis::definite(layout_pt(available_width)),
            )
            .points()
        })
        .unwrap_or(placeholder_style.line_height);
        let border_box_height = content_height + vertical_non_content;
        let line_baseline_offset = if placeholder_style.display.is_atomic_inline()
            || placeholder_style.abspos_static_source_was_atomic_inline
        {
            Self::inline_block_baseline_offset(&placeholder_style, border_box_height, None)
        } else {
            self.font_system
                .rendered_first_line_baseline_offset(&placeholder_style)
                .points()
        };

        InlineAtom::new(
            InlineAtomContent::StaticPositionPlaceholder,
            placeholder_style.clone(),
            None,
            InlineSize::new(
                border_box_width + placeholder_style.margin.left + placeholder_style.margin.right,
                border_box_height + placeholder_style.margin.top + placeholder_style.margin.bottom,
            ),
            line_baseline_offset,
            0.0,
            None,
            None,
        )
    }

    pub(in crate::layout) fn inline_static_position_from_placeholder_sequence(
        &mut self,
        sequence: &inline_layout::InlineLineSequence,
        block_style: &ComputedStyle,
    ) -> Option<InlineStaticPosition> {
        let saved_cursor_y = self.cursor_y;
        let saved_left = self.content_left;
        let saved_right = self.content_right;
        let context = sequence.context(block_style);
        let mut plaintext_direction_state = None;
        let mut stack = InlineLineStackCursor::new(
            block_style,
            self.content_left,
            self.content_right,
            self.cursor_y,
        );
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        for record in &records {
            if let Some(fragment) = &record.fragment && fragment.items().iter().any(|item| {
                matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if matches!(atom.content(), InlineAtomContent::StaticPositionPlaceholder)
                )
            }) {
                stack.apply(self);
                self.apply_line_block_start_trim_for_paint(record, block_style.writing_mode);
                let position = self
                    .prepare_inline_line_record(record, context, &mut plaintext_direction_state)
                    .and_then(|prepared| {
                        // The prepared line owns the canonical static
                        // baseline. Do not reconstruct it from the atom's
                        // border geometry: leading and font metrics can make
                        // that a different coordinate.
                        let baseline_y = self.cursor_y - prepared.metrics.baseline_offset;
                        prepared.paint_items.iter().find_map(|item| {
                            let PreparedInlinePaintItem::Atom(atom) = item else {
                                return None;
                            };
                            matches!(
                                atom.atom.content(),
                                InlineAtomContent::StaticPositionPlaceholder
                            )
                            .then_some(InlineStaticPosition {
                                start_x: atom.content_rect.x(),
                                end_x: atom.content_rect.x() + atom.content_rect.width(),
                                top_y: atom.content_rect.y()
                                    + atom.content_rect.height()
                                    + atom.atom.style().margin.top,
                                // The positioned fragment is later translated
                                // from its own first-line baseline. Preserve
                                // the matching baseline of the hypothetical
                                // placeholder atom, rather than rebuilding it
                                // from the enclosing line metrics: the two
                                // may differ through leading and atomic
                                // baseline participation.
                                baseline_y,
                                // Horizontal non-atomic inline sources align
                                // their first line to the static-position
                                // baseline. A vertical inline source's inline
                                // progression is physical Y, so its
                                // hypothetical rectangle instead supplies
                                // the positioned margin box's physical top
                                // edge. Atomic inline sources likewise use
                                // their margin box.
                                // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
                                use_margin_box_top: atom.atom.style().display.is_atomic_inline()
                                    || atom.atom.style().abspos_static_source_was_atomic_inline
                                    // A definite block-size gives the
                                    // hypothetical inline box a concrete
                                    // margin-box block-start. Auto-height
                                    // textual sources instead align their
                                    // first formatted line to the prepared
                                    // static baseline.
                                    || !atom.atom.style().box_values.height.is_auto()
                                    || WritingModeAxes::new(
                                        atom.atom.style().writing_mode,
                                        atom.atom.style().direction,
                                    )
                                    .swaps_physical_axes(),
                            })
                        })
                    });
                self.cursor_y = saved_cursor_y;
                self.content_left = saved_left;
                self.content_right = saved_right;
                return position;
            }
            stack.advance(record.height());
        }
        self.cursor_y = saved_cursor_y;
        self.content_left = saved_left;
        self.content_right = saved_right;
        None
    }

    pub(in crate::layout) fn collect_intrinsic_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        context: IntrinsicInlineCollectionContext<'_>,
        output: &mut Vec<InlineItem>,
    ) {
        for child in children {
            if let Some((_, _, style, _)) = child.element_parts()
                && (matches!(style.position, Position::Absolute | Position::Fixed)
                    || style.float != Float::None)
            {
                continue;
            }
            if let box_tree::FormattingBox::Block(box_) = child
                && matches!(&box_.core.source, box_tree::BoxSource::GeneratedPseudo(_))
            {
                if output
                    .last()
                    .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                let propagated_decoration = context.propagated_decoration.clone();
                self.collect_intrinsic_inline_box_items(
                    &box_.core.children,
                    stylesheets,
                    inherited_link.clone(),
                    context
                        .clone()
                        .with_block_style(&box_.core.style)
                        .with_propagated_decoration(
                            box_.core
                                .style
                                .text_decoration
                                .clone()
                                .with_propagated_lines(propagated_decoration),
                        ),
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
                    text_style.text_decoration = text_style
                        .text_decoration
                        .with_propagated_lines(context.propagated_decoration.clone());
                    self.push_inline_words(
                        &box_.text,
                        &text_style,
                        inherited_link.clone(),
                        context.baseline_shift,
                        context.visual_offset,
                        output,
                    );
                }
                box_tree::FormattingBox::Inline(box_) => {
                    let mut inline_style = box_tree::owned_style(&box_.core.style);
                    inline_style.text_decoration = inline_style
                        .text_decoration
                        .with_propagated_lines(context.propagated_decoration.clone());
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_placement =
                        InlinePlacement::new(context.baseline_shift, context.visual_offset)
                            .with_added_baseline_shift(
                                self.vertical_align_baseline_shift_for_inline_style(
                                    &inline_style,
                                    context.block_style,
                                ),
                            )
                            .with_added_visual_offset(
                                self.inline_visual_offset_for_style(&inline_style),
                            );
                    let scope = self.begin_inline_element_scope(
                        box_.core.element,
                        &inline_style,
                        link.clone(),
                        child_placement,
                        InlineElementScopeOptions::BOX_INTRINSIC
                            .with_fragment_edges(box_.fragment_edges),
                        output,
                    );
                    if inline_style.content.is_generated() {
                        let start_len = output.len();
                        self.push_intrinsic_element_content_items_from_boxes(
                            box_.core.element,
                            &inline_style.clone(),
                            &box_.core.children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift,
                            child_placement.visual_offset,
                            inline_style.text_decoration.clone(),
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
                    } else {
                        self.collect_intrinsic_inline_box_items(
                            &box_.core.children,
                            stylesheets,
                            link.clone(),
                            context
                                .clone()
                                .with_baseline_shift(child_placement.baseline_shift)
                                .with_visual_offset(child_placement.visual_offset)
                                .with_block_style(&inline_style.clone())
                                .with_propagated_decoration(inline_style.text_decoration.clone()),
                            output,
                        );
                    }
                    self.end_inline_element_scope(scope, &inline_style, output);
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset = context
                        .visual_offset
                        .plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_snapshot = self.counter_set.clone();
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.intrinsic_inline_atom_for_element(
                        box_.core.element,
                        &box_.core.style,
                        &box_.core.children,
                        box_.table_fragment.as_ref(),
                        stylesheets,
                        context.baseline_shift,
                        atom_visual_offset,
                        link,
                    );
                    self.end_counter_scope(counter_scope);
                    self.counter_set = counter_snapshot;
                    if let Some(mut atom) = atom {
                        atom.baseline_shift +=
                            self.vertical_align_baseline_shift_for_atom(&atom, context.block_style);
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.core.element, &box_.core.style);
                        self.push_inline_words(
                            &text,
                            &box_.core.style,
                            inherited_link.clone(),
                            context.baseline_shift,
                            atom_visual_offset,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self
                    .collect_intrinsic_inline_box_items(
                        &box_.children,
                        stylesheets,
                        inherited_link.clone(),
                        context
                            .clone()
                            .with_block_style(&box_.style)
                            .with_propagated_decoration(
                                box_.style
                                    .text_decoration
                                    .clone()
                                    .with_propagated_lines(context.propagated_decoration.clone()),
                            ),
                        output,
                    ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Table(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_) => {}
            }
        }
    }
}

/// Produce the anonymous replaced-content style inside an inline generated
/// pseudo-element.
///
/// A `content: url(...)` item is the child of the tree-abiding pseudo-element,
/// not a replacement of that pseudo-element itself. Its parent owns the
/// pseudo's box decoration; copying those edges into the image atom would
/// paint the border twice and size the image as the pseudo's border box:
/// <https://www.w3.org/TR/css-content-3/#content-property> and
/// <https://drafts.csswg.org/css-pseudo-4/#generated-content>.
fn generated_pseudo_inline_content_style(style: &ComputedStyle) -> ComputedStyle {
    let mut content_style = style.clone();
    content_style.margin = css::Edges::ZERO;
    content_style.ua_margin_em = css::OptionalEdges::NONE;
    content_style.box_values.margin = css::CssEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
    content_style.padding = css::Edges::ZERO;
    content_style.box_values.padding = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
    content_style.border_width = 0.0;
    content_style.border_widths = css::Edges::ZERO;
    content_style.border_width_values = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
    content_style.border_styles = css::BorderStyles::NONE;
    content_style.border_radius = css::BorderRadius::ZERO;
    content_style.corner_shapes = css::CornerShapes::ROUND;
    content_style.border_image = css::BorderImage::initial();
    content_style.background_color = None;
    content_style.background_image = css::ComputedImage::None;
    content_style.background_layers.clear();
    content_style
}

pub(in crate::layout) fn annotate_line_break_element_breaks(
    element: &Element,
    style: &ComputedStyle,
    output: &mut [InlineItem],
    start_len: usize,
) {
    annotate_line_break_element_breaks_with_clear(element, style.clear, output, start_len);
}

pub(in crate::layout) fn annotate_line_break_element_breaks_with_clear(
    element: &Element,
    clear: Clear,
    output: &mut [InlineItem],
    start_len: usize,
) {
    if !is_line_break_element(element) || clear == Clear::None {
        return;
    }
    for item in output.iter_mut().skip(start_len) {
        match item {
            InlineItem::Break(break_) => break_.clear = clear,
            InlineItem::Word(word) if word.source == InlineTextSource::Generated => {
                std::rc::Rc::make_mut(&mut word.style).clear = clear;
            }
            _ => {}
        }
    }
}

fn generated_content_originating_clear(source: &box_tree::BoxSource<'_>) -> Option<Clear> {
    match source {
        box_tree::BoxSource::GeneratedPseudo(pseudo) => Some(pseudo.originating_clear),
        box_tree::BoxSource::Principal => None,
    }
}

fn inline_style_establishes_positioning_containing_block(style: &ComputedStyle) -> bool {
    matches!(
        style.position,
        Position::Absolute | Position::Fixed | Position::Relative | Position::Sticky
    ) || style.has_transform()
}
