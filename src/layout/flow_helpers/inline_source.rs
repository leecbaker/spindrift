use super::*;

pub(in crate::layout) fn has_styled_inline_descendant_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_styled_inline_descendant_with_inline_flow_scope(
        element,
        parent_style,
        stylesheets,
        ancestors,
        false,
        &mut resolver,
    )
}

fn has_styled_inline_descendant_with_inline_flow_scope(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    inside_inline_flow: bool,
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let has_non_phantom_direct_text = element.children.iter().any(|child| {
        matches!(
            &child.kind,
            NodeKind::Text(text) if inline_text_has_non_phantom_content(text, parent_style)
        )
    });
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    element.children.iter().any(|child| {
        let NodeKind::Element(child_element) = &child.kind else {
            return false;
        };
        let signature =
            ElementSignature::from_sibling_snapshot(element_index, sibling_tags.clone())
                .expect("source child must have a cached sibling signature");
        element_index += 1;
        let child_style = resolver.style_for_element(
            child_element,
            signature.clone(),
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        // A suppressed descendant still changes the rendered text stream:
        // flattening the parent's DOM text would otherwise resurrect fallback
        // text from `display: none` / unboxed HTML elements. Route this
        // through the item collector, which observes the descendant's used
        // display value before adding its content.
        // <https://www.w3.org/TR/css-display-3/#box-generation>
        if child_style.display.is_none() {
            return true;
        }
        // Absolute and fixed descendants are blockified for their own layout,
        // but they remain at this inline source boundary for static-position
        // selection. The plain-text fast path would discard that boundary and
        // never materialize their positioned paint.
        // <https://www.w3.org/TR/css-position-3/#static-position>
        if matches!(child_style.position, Position::Absolute | Position::Fixed) {
            return true;
        }
        // A float is blockified for its own layout, but stays at this inline
        // source position as a zero-width marker. The scalar-text shortcut
        // would otherwise flatten its surrounding text and lose the marker.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        if child_style.float != Float::None {
            // A direct block float stays in the parent's block-flow child
            // traversal unless direct text forms an inline source run beside
            // it. Once an inline flow scope owns the source, its float marker
            // must likewise be collected with the surrounding text.
            return inside_inline_flow || has_non_phantom_direct_text;
        }
        if child_style.display.is_block_level() {
            return false;
        }
        // Ruby is a non-atomic inline-level formatting context whose base
        // content participates in the parent line. Its annotations cannot be
        // represented by this scalar-text shortcut, so force the inline-item
        // collector even when its inherited typography matches the parent.
        // <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
        if child_style.display.is_ruby() {
            return true;
        }
        // Link annotations belong to the inline fragment sequence. The
        // scalar text fast path can paint the glyphs but has no source range
        // on which to record the hyperlink rectangle.
        // <https://www.w3.org/TR/css-ui-4/#cursor>
        if child_element.attrs.contains_key("href") {
            return true;
        }
        // Atomic inline-level boxes contribute their own dimensions and
        // baseline even when their descendants share the parent's font
        // metrics. Route them through inline-item collection rather than
        // collapsing the fragment to a plain text run.
        // <https://drafts.csswg.org/css-display-3/#atomic-inline>
        if (child_style.display.is_atomic_inline() || is_replaced_element(child_element))
            && child_style.display.is_inline_level()
            || (child_style.display.is_table() && child_style.display.is_inline_or_run_in_level())
        {
            return true;
        }
        inline_style_affects_line(parent_style, &child_style) || {
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature);
            has_styled_inline_descendant_with_inline_flow_scope(
                child_element,
                &child_style,
                stylesheets,
                &child_ancestors,
                inside_inline_flow
                    || (child_style.display.is_inline_level() && child_style.display.is_flow()),
                resolver,
            )
        }
    })
}

pub(in crate::layout) fn inline_style_affects_line(
    parent: &ComputedStyle,
    child: &ComputedStyle,
) -> bool {
    child.before_style.is_some()
        || child.after_style.is_some()
        // A scalar text run has no lexical boundary at which to emit the UAX
        // #9 controls required by an inline CSS bidi scope. In particular,
        // `unicode-bidi: isolate` must be externally represented as one
        // neutral object instead of flattening its text into the paragraph.
        // <https://drafts.csswg.org/css-writing-modes-4/#unicode-bidi>
        || inline_bidi_scope_affects_line_ordering(child)
        || child.float != Float::None
        || child.color != parent.color
        || child.font_family != parent.font_family
        || child.font_size != parent.font_size
        || child.font_style != parent.font_style
        || child.font_weight != parent.font_weight
        || child.font_width != parent.font_width
        || child.font_palette != parent.font_palette
        || child.line_height != parent.line_height
        || child.text_decoration != parent.text_decoration
        || child.text_transform != parent.text_transform
        || child.word_space_transform != parent.word_space_transform
        // A preserved tab belongs to the inline style that owns its source
        // character. Flattening a `tab-size` change into the block's scalar
        // text fast path would lose that value before line-level tab-stop
        // resolution can select its period.
        // <https://www.w3.org/TR/css-text-3/#tab-size-property>
        || child.tab_size != parent.tab_size
        || child.vertical_align != parent.vertical_align
        || child.white_space != parent.white_space
        || inline_break_policy_differs(parent, child)
        // A non-inherited inline decoration needs the item collector even
        // when its typography is identical to the parent.  The plain-text
        // fast path has no inline box fragments on which to paint these
        // backgrounds/borders or relative visual offsets.
        // <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>
        // <https://www.w3.org/TR/css-position-3/#relative-positioning>
        || child.background.background_color.is_potentially_visible()
        || child.background.background_image.is_image()
        || child.background.background_layers.iter().any(|layer| layer.image.is_image())
        || used_border_width(child) > layout_pt(0.0)
        || child.margin != parent.margin
        || child.padding != parent.padding
        || child.box_values.margin != parent.box_values.margin
        || child.box_values.padding != parent.box_values.padding
        || matches!(child.position, Position::Relative | Position::Sticky)
        // Opacity establishes an atomic compositing group. Retain the
        // lexical inline scope so its text, decorations, and descendants are
        // painted into that group instead of being flattened into the parent
        // text run.
        // <https://www.w3.org/TR/css-color-4/#transparency>
        || child.opacity.value() < 1.0
}

/// Whether flattening a descendant inline box into its parent text run would
/// change the available break opportunities or the marker painted at one.
///
/// CSS Text defines hyphenation as a language-sensitive soft wrap opportunity
/// and requires inline element boundaries to be ignored when determining word
/// boundaries. The scalar text fast path therefore remains valid only when it
/// retains the same break policy for every descendant text segment.
/// <https://www.w3.org/TR/css-text-3/#hyphenation>
/// <https://www.w3.org/TR/css-text-3/#line-break-details>
fn inline_break_policy_differs(parent: &ComputedStyle, child: &ComputedStyle) -> bool {
    child.language != parent.language
        || child.hyphens != parent.hyphens
        || child.hyphenate_character != parent.hyphenate_character
        || child.hyphenate_limit_chars != parent.hyphenate_limit_chars
        || child.word_break != parent.word_break
        || child.overflow_wrap != parent.overflow_wrap
        || child.line_break != parent.line_break
        || child.text_wrap_mode != parent.text_wrap_mode
        || child.text_wrap_style != parent.text_wrap_style
}

pub(in crate::layout) fn has_direct_inline_replaced_child(element: &Element) -> bool {
    element.children.iter().any(|child| {
        matches!(&child.kind, NodeKind::Element(child_element) if is_replaced_element(child_element))
    })
}

pub(in crate::layout) fn has_direct_flow_child_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_direct_flow_child_with_resolver(element, parent_style, stylesheets, &mut resolver)
}

/// Whether a direct DOM source contains floats but no parent inline-line
/// content or normal-flow child.
///
/// A float's descendants belong to its own formatting context. Therefore a
/// direct floated element is terminal for this parent-source classifier, just
/// as a floated formatting box is terminal after normalization.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
pub(in crate::layout) fn has_direct_float_only_source_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut has_float = false;

    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                if inline_text_has_non_phantom_content(text, parent_style) {
                    return false;
                }
            }
            NodeKind::Element(child_element) => {
                let signature = ElementSignature::with_sibling_list(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let style = resolver.structural_style_for_element(
                    child_element,
                    signature,
                    stylesheets,
                    Some(parent_style),
                    &[],
                );
                if style.float != Float::None {
                    has_float = true;
                } else if style.display.is_none()
                    || matches!(style.position, Position::Absolute | Position::Fixed)
                {
                    continue;
                } else {
                    return false;
                }
            }
        }
    }

    has_float
}

pub(in crate::layout) fn has_direct_flow_child_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    element.children.iter().any(|child| {
        let NodeKind::Element(child_element) = &child.kind else {
            return false;
        };
        let signature = ElementSignature::with_sibling_list(
            child_element.tag.clone(),
            child_element.attrs.clone(),
            element_index,
            sibling_tags.clone(),
        );
        element_index += 1;
        let style = resolver.structural_style_for_element(
            child_element,
            signature,
            stylesheets,
            Some(parent_style),
            &[],
        );
        if is_replaced_element(child_element) && style.display.is_inline_level() {
            return false;
        }
        // HTML table semantics select table layout, but do not override the
        // computed outer display role. In particular, `inline-table` remains
        // an inline-level atomic child of its block container.
        // <https://drafts.csswg.org/css-display-3/#outer-role>
        // <https://drafts.csswg.org/css-tables-3/#table-root>
        style.display.is_block_level()
    })
}

pub(in crate::layout) fn has_ordered_mixed_flow_content_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_ordered_mixed_flow_content_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
    )
}

/// Returns whether direct-DOM block layout must materialize its child
/// formatting tree to perform CSS block-in-inline splitting.
///
/// A normal-flow block descendant of an inline flow wrapper is not an inline
/// item. CSS 2.2 splits the inline wrapper around it, then places the block
/// between anonymous block boxes in the enclosing block flow. The DOM inline
/// collector intentionally does not lay out ordinary block descendants, so
/// this structural boundary must use the normalized formatting-tree path.
///
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
pub(in crate::layout) fn has_block_in_inline_split_boundary_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_block_in_inline_split_boundary_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        false,
        &mut resolver,
    )
}

/// Returns whether a block's inline source contains a ruby formatting
/// context. Ruby layout has its own anonymous box generation and pairing
/// phase, so its subtree cannot use the scalar DOM-text fast path.
///
/// <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
/// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
pub(in crate::layout) fn has_ruby_formatting_descendant_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
    cached_descendants: &mut HashMap<ElementId, bool>,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_ruby_formatting_descendant_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
        cached_descendants,
    )
}

/// Return whether a descendant table-internal box needs CSS Tables anonymous
/// wrapper construction before normal block layout can dispatch it.
///
/// A `table-row`, `table-cell`, and the other table-internal display types do
/// not independently establish a table formatting context. When encountered
/// by the direct DOM traversal, they would otherwise be treated as ordinary
/// block/inline content and their table fixup—including anonymous cell block
/// container normalization—would be skipped.
/// <https://drafts.csswg.org/css-tables-3/#fixup-algorithm>
pub(in crate::layout) fn has_unwrapped_table_internal_descendant_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    let mut ancestor_stack = ancestors.to_vec();
    has_unwrapped_table_internal_descendant_with_resolver(
        element,
        parent_style,
        stylesheets,
        &mut ancestor_stack,
        &mut resolver,
    )
}

/// Return whether direct child normalization must resolve a CSS `run-in`
/// sequence before block layout chooses its child traversal.
///
/// Run-in placement depends on the following in-flow sibling, so the direct
/// DOM walker cannot decide it one child at a time. It must first use the
/// block-container's normalized formatting tree.
/// <https://drafts.csswg.org/css-display-3/#run-in-layout>
pub(in crate::layout) fn has_direct_run_in_child_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    let mut element_index = 0usize;
    element.children.iter().any(|child| {
        let NodeKind::Element(child_element) = &child.kind else {
            return false;
        };
        let signature = ElementSignature::with_sibling_list(
            child_element.tag.clone(),
            child_element.attrs.clone(),
            element_index,
            sibling_tags.clone(),
        );
        element_index += 1;
        resolver
            .structural_style_for_element(
                child_element,
                signature,
                stylesheets,
                Some(parent_style),
                ancestors,
            )
            .display
            .is_run_in()
    })
}

fn has_unwrapped_table_internal_descendant_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &mut Vec<ElementSignature>,
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            continue;
        };
        let signature =
            ElementSignature::from_sibling_snapshot(element_index, sibling_tags.clone())
                .expect("source child must have a cached sibling signature");
        element_index += 1;
        let child_style = resolver.structural_style_for_element(
            child_element,
            signature.clone(),
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        if child_style.display.is_none() {
            continue;
        }
        if is_table_internal_display(child_style.display) {
            return true;
        }
        // A proper table root owns its descendants' table fixup. Requiring a
        // parent structural rebuild for it would only bypass the direct table
        // layout path without adding information.
        if child_style.display.is_table() {
            continue;
        }
        ancestors.push(signature);
        let has_unwrapped_descendant = has_unwrapped_table_internal_descendant_with_resolver(
            child_element,
            &child_style,
            stylesheets,
            ancestors,
            resolver,
        );
        let popped = ancestors.pop();
        debug_assert!(
            popped.is_some(),
            "recursive table probe must pop its pushed ancestor"
        );
        if has_unwrapped_descendant {
            return true;
        }
    }
    false
}

fn is_table_internal_display(display: Display) -> bool {
    matches!(
        display.inner,
        DisplayInner::TableCaption
            | DisplayInner::TableColumnGroup
            | DisplayInner::TableColumn
            | DisplayInner::TableHeaderGroup
            | DisplayInner::TableRowGroup
            | DisplayInner::TableFooterGroup
            | DisplayInner::TableRow
            | DisplayInner::TableCell
    )
}

fn has_block_in_inline_split_boundary_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    inside_inline_flow: bool,
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;

    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            continue;
        };
        let signature = ElementSignature::with_sibling_list(
            child_element.tag.clone(),
            child_element.attrs.clone(),
            element_index,
            sibling_tags.clone(),
        );
        element_index += 1;
        let child_style = resolver.structural_style_for_element(
            child_element,
            signature.clone(),
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        if child_style.display.is_none() {
            continue;
        }
        if inside_inline_flow && is_normal_block_flow_child(child_element, &child_style) {
            return true;
        }
        // Atomic inline boxes establish their own formatting context and
        // therefore do not take part in their parent's block-in-inline
        // transformation. Display-contents contributes no box, so it
        // preserves an enclosing inline-flow scope for its descendants.
        // Ruby layout-internal boxes establish a separate formatting model,
        // rather than an ordinary inline flow.  They nevertheless need the
        // same structural-tree boundary here: a direct in-flow block child is
        // inlinified by CSS Ruby before CSS Display's block-in-inline split
        // can inspect the enclosing tree.
        // <https://drafts.csswg.org/css-ruby-1/#anon-gen-inlinize>
        let continues_inline_flow = child_style.display.is_contents()
            || ((child_style.display.is_inline_level() && child_style.display.is_flow())
                || child_style.display.is_ruby()
                || child_style.display.is_ruby_internal())
                && child_style.float == Float::None
                && matches!(
                    child_style.position,
                    Position::Static | Position::Relative | Position::Running(_)
                );
        if continues_inline_flow {
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature);
            if has_block_in_inline_split_boundary_with_resolver(
                child_element,
                &child_style,
                stylesheets,
                &child_ancestors,
                true,
                resolver,
            ) {
                return true;
            }
        }
    }

    false
}

fn has_ruby_formatting_descendant_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    cached_descendants: &mut HashMap<ElementId, bool>,
) -> bool {
    if let Some(&contains_ruby) = cached_descendants.get(&element.id) {
        return contains_ruby;
    }
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            continue;
        };
        let signature = ElementSignature::with_sibling_list(
            child_element.tag.clone(),
            child_element.attrs.clone(),
            element_index,
            sibling_tags.clone(),
        );
        element_index += 1;
        let child_style = resolver.structural_style_for_element(
            child_element,
            signature.clone(),
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        if child_style.display.is_none() {
            continue;
        }
        if child_style.display.is_ruby() {
            cached_descendants.insert(element.id, true);
            return true;
        }
        // Ruby's anonymous-box construction affects the inline formatting
        // context that contains it. A descendant block, float, or atomic
        // inline establishes its own relevant formatting context and checks
        // its own source when it is laid out; walking through it here would
        // make every ancestor repeatedly cascade an unrelated subtree.
        // `display: contents` and ordinary inline flow preserve the current
        // inline formatting context, while ruby-internal boxes remain part of
        // the ruby structure that owns it.
        // <https://drafts.csswg.org/css-display-3/#block-in-inline>
        // <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
        let continues_inline_flow = child_style.display.is_contents()
            || ((child_style.display.is_inline_level() && child_style.display.is_flow())
                || child_style.display.is_ruby_internal())
                && child_style.float == Float::None
                && matches!(
                    child_style.position,
                    Position::Static | Position::Relative | Position::Running(_)
                );
        if !continues_inline_flow {
            continue;
        }
        let mut child_ancestors = ancestors.to_vec();
        child_ancestors.push(signature);
        if has_ruby_formatting_descendant_with_resolver(
            child_element,
            &child_style,
            stylesheets,
            &child_ancestors,
            resolver,
            cached_descendants,
        ) {
            cached_descendants.insert(element.id, true);
            return true;
        }
    }
    cached_descendants.insert(element.id, false);
    false
}

/// Whether a block needs the ordered mixed inline/block child traversal.
///
/// Absolutely and fixed positioned descendants do not establish an
/// auto-height parent's fragmentainer-local flow end. A block-origin
/// positioned sibling only makes the sequence source-sensitive when it lies
/// between an earlier in-flow child and a later CSS float: its static position
/// is selected after the earlier child, before that float. Route that precise
/// sequence through the ordered traversal so the generic inline collector
/// cannot descend into the later float before the block has committed its
/// cursor.
///
/// <https://www.w3.org/TR/css-position-3/#absolute-positioning>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
pub(in crate::layout) fn has_ordered_mixed_flow_content_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    if suppresses_ordered_mixed_flow_detection(element) {
        return false;
    }

    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut has_inline = false;
    let mut has_flow = false;
    let mut has_positioned_static_boundary = false;
    let mut has_block_static_boundary_after_flow = false;
    let mut has_later_float_after_static_boundary = false;

    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                if !normalize_inline_text(text).is_empty() {
                    has_inline = true;
                }
            }
            NodeKind::Element(child_element) => {
                let signature = ElementSignature::with_sibling_list(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = resolver.style_for_element(
                    child_element,
                    signature.clone(),
                    stylesheets,
                    Some(parent_style),
                    ancestors,
                );
                if matches!(child_style.position, Position::Absolute | Position::Fixed) {
                    // Out-of-flow boxes retain source order only for static
                    // positioning. They do not contribute an in-flow
                    // endpoint to an otherwise block-only auto-height parent.
                    // An inline-origin source nevertheless needs the ordered
                    // traversal at a preceding or following block boundary:
                    // its hypothetical inline line is the input to its
                    // static-position rectangle, even though the box itself
                    // has been blockified for layout.
                    // <https://drafts.csswg.org/css-position-3/#static-position>
                    let is_block_static_boundary = child_style.display.is_block_level();
                    has_positioned_static_boundary |= is_block_static_boundary;
                    has_block_static_boundary_after_flow |= is_block_static_boundary && has_flow;
                    has_inline |= child_style.display.is_inline_level();
                    continue;
                }
                // A float is out of normal flow. Its used display type is
                // blockified, but that does not make it an in-flow block
                // boundary for this source-order classifier. A direct float
                // is handled by the ordinary child traversal, while a float
                // inside an inline source run is collected as an inline
                // marker by that run.
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                let is_float_source = child_style.float != Float::None;
                let is_normal_flow_child = is_normal_block_flow_child(child_element, &child_style)
                    // HTML table structure still needs source-order traversal
                    // around block siblings, but its computed outer display
                    // decides whether the table itself is dispatched as block
                    // flow or collected as an atomic inline.
                    // <https://drafts.csswg.org/css-display-3/#box-generation>
                    || (is_html_table_element(child_element)
                        && child_style.display.is_block_level())
                    || (is_replaced_element(child_element)
                        && child_style.display.is_block_level());
                if is_float_source || is_normal_flow_child {
                    has_later_float_after_static_boundary |= has_block_static_boundary_after_flow
                        && matches!(
                            child_style.float,
                            Float::Left | Float::Right | Float::InlineStart | Float::InlineEnd
                        );
                    has_flow |= is_normal_flow_child;
                } else if child_style.display.is_contents() {
                    let mut child_ancestors = ancestors.to_vec();
                    child_ancestors.push(signature);
                    if display_contents_has_inline_flow_contribution_with_resolver(
                        child_element,
                        &child_style,
                        stylesheets,
                        &child_ancestors,
                        resolver,
                    ) {
                        has_inline = true;
                    }
                } else if child_style.display.is_inline_level()
                    || is_line_break_element(child_element)
                    || !inline_text(child_element).is_empty()
                {
                    has_inline = true;
                }
            }
        }

        if (has_inline && (has_flow || has_positioned_static_boundary))
            || has_later_float_after_static_boundary
        {
            return true;
        }
    }

    false
}

/// Return whether a `display: contents` subtree contributes inline source to
/// its parent's formatting context.
///
/// A `display: contents` element has no principal box, so its in-flow
/// descendants and tree-abiding generated pseudo-elements retain their source
/// position in the parent's mixed block/inline sequence.  Looking only at DOM
/// text loses generated `::before`/`::after` content and causes the generic
/// parent-inline collector to replay that content before preceding block
/// siblings.
///
/// <https://drafts.csswg.org/css-display-3/#valdef-display-contents>
/// <https://drafts.csswg.org/css-pseudo-4/#treelike>
fn display_contents_has_inline_flow_contribution_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;

    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                if !normalize_inline_text(text).is_empty() {
                    return true;
                }
            }
            NodeKind::Element(child_element) => {
                let signature = ElementSignature::with_sibling_list(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = resolver.style_for_element(
                    child_element,
                    signature.clone(),
                    stylesheets,
                    Some(parent_style),
                    ancestors,
                );
                if child_style.display.is_none() {
                    continue;
                }

                let preserves_parent_inline_context = child_style.display.is_contents()
                    || (child_style.display.is_inline_level()
                        && child_style.float == Float::None
                        && matches!(
                            child_style.position,
                            Position::Static | Position::Relative | Position::Running(_)
                        ));
                if !preserves_parent_inline_context {
                    continue;
                }

                let has_generated_inline_content = child_style
                    .before_style
                    .as_deref()
                    .is_some_and(generated_content_has_non_phantom_inline_content)
                    || child_style
                        .after_style
                        .as_deref()
                        .is_some_and(generated_content_has_non_phantom_inline_content);
                if has_generated_inline_content || child_style.display.is_inline_level() {
                    return true;
                }

                debug_assert!(child_style.display.is_contents());
                let mut child_ancestors = ancestors.to_vec();
                child_ancestors.push(signature);
                if display_contents_has_inline_flow_contribution_with_resolver(
                    child_element,
                    &child_style,
                    stylesheets,
                    &child_ancestors,
                    resolver,
                ) {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests;
