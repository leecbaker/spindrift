use super::*;

pub(in crate::layout) fn collapsible_first_child_start_margin_dom_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
    overflow_context: DocumentCanvasResolution,
) -> Option<AdjoiningMarginSet> {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    collapsible_first_child_start_margin_dom_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
        overflow_context,
    )
}

pub(in crate::layout) fn collapsible_first_child_start_margin_dom_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> Option<AdjoiningMarginSet> {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut has_preceding_css_float = false;
    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            if let NodeKind::Text(text) = &child.kind
                && !collapse_whitespace(text).is_empty()
            {
                return None;
            }
            continue;
        };
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
        // Floats and positioned descendants are out of normal flow, even
        // below the document canvas; they cannot supply an adjoining margin.
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        if child_style.float != Float::None {
            has_preceding_css_float = true;
            continue;
        }
        if matches!(child_style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let is_flow_child = is_normal_block_flow_child(child_element, &child_style);
        if !is_flow_child {
            if !inline_text(child_element).is_empty() {
                return None;
            }
            continue;
        }
        // A cleared first in-flow child after an adjoining float needs the
        // parent-start `clear:none` hypothesis rather than a pre-collapsed
        // used start margin. Other cleared descendants retain the ordinary
        // margin-collapse probe; block layout resolves whether they actually
        // introduce clearance in their containing float context.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        if child_style.clear != Clear::None && has_preceding_css_float {
            return None;
        }
        if parent_style.margin_trim.block_start {
            return Some(AdjoiningMarginSet::from_margin(layout_pt(0.0)));
        }
        let mut child_ancestors = ancestors.to_vec();
        child_ancestors.push(signature);
        return Some(collapsible_start_margin_dom_with_resolver(
            child_element,
            &child_style,
            stylesheets,
            &child_ancestors,
            resolver,
            overflow_context,
        ));
    }
    None
}

pub(in crate::layout) fn clear_none_hypothetical_start_margin_dom_with_resolver(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> Option<LayoutLength> {
    if style.margin_trim.block_start {
        return None;
    }
    let mut set = AdjoiningMarginSet::from_margin(layout_pt(style.margin.top));
    complete_adjoining_child_margin_set_dom(
        &mut set,
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
        overflow_context,
    )
    .then(|| set.collapsed())
}

/// Extend a CSS2 hypothetical start-margin set through every adjoining
/// self-collapsing descendant and sibling in DOM source order.
///
/// This is deliberately separate from the scalar first-child query used by
/// ordinary child placement: future sibling margins belong in the
/// counterfactual clearance position, but must not be consumed early by the
/// runtime sibling cursor.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
fn complete_adjoining_child_margin_set_dom(
    set: &mut AdjoiningMarginSet,
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut found_in_flow_child = false;
    let mut has_preceding_css_float = false;
    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            if let NodeKind::Text(text) = &child.kind
                && !collapse_whitespace(text).is_empty()
            {
                break;
            }
            continue;
        };
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
        if child_style.float != Float::None {
            has_preceding_css_float = true;
            continue;
        }
        if matches!(child_style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let is_flow_child = is_normal_block_flow_child(child_element, &child_style);
        if !is_flow_child {
            if !inline_text(child_element).is_empty() {
                break;
            }
            continue;
        }
        if child_style.clear != Clear::None && has_preceding_css_float {
            break;
        }

        found_in_flow_child = true;
        let mut child_ancestors = ancestors.to_vec();
        child_ancestors.push(signature);
        let child_collapses_through = is_self_collapsing_block_dom_with_resolver(
            child_element,
            &child_style,
            stylesheets,
            &child_ancestors,
            resolver,
            overflow_context,
        );
        let mut child_set = AdjoiningMarginSet::from_margin(layout_pt(child_style.margin.top));
        if !child_style.margin_trim.block_start
            && can_collapse_block_start_margin(
                child_element,
                &child_style,
                UsedEdges::from_css_edges(used_border_widths(&child_style)),
                has_direct_inline_content_dom_with_resolver(
                    child_element,
                    &child_style,
                    stylesheets,
                    &child_ancestors,
                    resolver,
                ),
                overflow_context.used_overflow(child_element, &child_style),
            )
        {
            complete_adjoining_child_margin_set_dom(
                &mut child_set,
                child_element,
                &child_style,
                stylesheets,
                &child_ancestors,
                resolver,
                overflow_context,
            );
        }
        if child_collapses_through {
            child_set.include(layout_pt(child_style.margin.bottom));
        }
        set.merge(child_set);
        if !child_collapses_through {
            break;
        }
    }
    found_in_flow_child
}

pub(in crate::layout) fn collapsible_start_margin_dom_with_resolver(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> AdjoiningMarginSet {
    if can_collapse_block_start_margin(
        element,
        style,
        UsedEdges::from_css_edges(used_border_widths(style)),
        has_direct_inline_content_dom_with_resolver(
            element,
            style,
            stylesheets,
            ancestors,
            resolver,
        ),
        overflow_context.used_overflow(element, style),
    ) && let Some(descendant_margin) = collapsible_first_child_start_margin_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
        overflow_context,
    ) {
        if is_self_collapsing_block_dom_with_resolver(
            element,
            style,
            stylesheets,
            ancestors,
            resolver,
            overflow_context,
        ) {
            return self_collapsing_block_margin_set_for_box(style, Some(descendant_margin));
        }
        AdjoiningMarginSet::from_margin(layout_pt(style.margin.top)).merged(descendant_margin)
    } else if is_self_collapsing_block_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
        overflow_context,
    ) {
        self_collapsing_block_margin_set_for_box(style, None)
    } else {
        AdjoiningMarginSet::from_margin(layout_pt(style.margin.top))
    }
}

/// Returns whether a DOM-backed block's own start and end margins are adjoining.
///
/// This is the DOM traversal equivalent of `is_self_collapsing_block_box` for
/// code paths that have not yet received normalized formatting boxes. It keeps
/// CSS 2.2 margin-collapse and CSS Box 4 `margin-trim` behavior consistent
/// across both layout entrypoints.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://drafts.csswg.org/css-box-4/#margin-trim>
pub(in crate::layout) fn is_self_collapsing_block_dom_with_font_metrics(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
    overflow_context: DocumentCanvasResolution,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    is_self_collapsing_block_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        &mut resolver,
        overflow_context,
    )
}

pub(in crate::layout) fn is_self_collapsing_block_dom_with_resolver(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> bool {
    let has_direct_inline_content = has_direct_inline_content_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
    ) || style_has_non_phantom_generated_pseudo_content(style)
        || style_has_in_flow_marker_line(style);
    is_collapsible_block_child(element, style)
        && can_collapse_own_block_margins(
            element,
            style,
            used_border_widths(style),
            has_direct_inline_content,
            overflow_context.used_overflow(element, style),
        )
        && dom_children_keep_self_collapsing_parent(
            element,
            style,
            stylesheets,
            ancestors,
            resolver,
            overflow_context,
        )
}

/// Generated `::before` and `::after` boxes participate in their originating
/// block's principal inline flow.  They are stored separately from DOM and
/// formatting-box children, so self-collapsing classification must include
/// them explicitly.
/// <https://drafts.csswg.org/css-pseudo-4/#generated-content>
pub(super) fn style_has_non_phantom_generated_pseudo_content(style: &ComputedStyle) -> bool {
    style
        .before_style
        .as_deref()
        .is_some_and(generated_content_has_non_phantom_inline_content)
        || style
            .after_style
            .as_deref()
            .is_some_and(generated_content_has_non_phantom_inline_content)
}

/// Whether a computed list item necessarily supplies an inside marker line.
///
/// Margin-collapse classification happens before the inline formatter creates
/// the actual marker.  This mirrors its empty-marker test so a marker-only
/// list item, and every ancestor whose size depends on it, cannot be treated
/// as self-collapsing.  Counter values are allowed to vary, but every
/// non-`none` counter style has the decimal fallback representation required
/// by CSS Counter Styles.
///
/// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-fallback>
pub(super) fn style_has_in_flow_marker_line(style: &ComputedStyle) -> bool {
    if !style.display.is_list_item() {
        return false;
    }
    let inside = if style.display.is_inline_level() && !style.display.is_atomic_inline() {
        true
    } else {
        style.list_style_position == ListStylePosition::Inside
    };
    if !inside {
        return false;
    }

    let marker_style = style.marker_style.as_deref().unwrap_or(style);
    match &marker_style.marker_content {
        MarkerContent::None => false,
        MarkerContent::Auto => {
            style.list_style_image.is_image()
                || list_style_type_has_nonempty_representation(&style.list_style_type)
        }
        MarkerContent::Parts(parts) => parts
            .iter()
            .any(|part| marker_content_part_has_nonempty_representation(part, marker_style)),
    }
}

fn list_style_type_has_nonempty_representation(style: &ListStyleType) -> bool {
    match style {
        ListStyleType::None => false,
        ListStyleType::String(text) => {
            !crate::text::trim_css_collapsible_whitespace(text).is_empty()
        }
        ListStyleType::Disc
        | ListStyleType::Circle
        | ListStyleType::Square
        | ListStyleType::DisclosureOpen
        | ListStyleType::DisclosureClosed
        | ListStyleType::Decimal
        | ListStyleType::Anonymous(_)
        | ListStyleType::Named(_) => true,
    }
}

fn marker_content_part_has_nonempty_representation(
    part: &MarkerContentPart,
    marker_style: &ComputedStyle,
) -> bool {
    match part {
        MarkerContentPart::Text(text) => {
            !crate::text::trim_css_collapsible_whitespace(text).is_empty()
        }
        MarkerContentPart::Counter { style, .. } | MarkerContentPart::Counters { style, .. } => {
            style
                .as_ref()
                .is_none_or(list_style_type_has_nonempty_representation)
        }
        MarkerContentPart::Quote(GeneratedQuote::NoOpen | GeneratedQuote::NoClose) => false,
        MarkerContentPart::Quote(GeneratedQuote::Open | GeneratedQuote::Close) => {
            match &marker_style.quotes {
                Quotes::None => false,
                Quotes::Auto(_) => true,
                Quotes::Pairs(pairs) => pairs.iter().any(|(open, close)| {
                    !crate::text::trim_css_collapsible_whitespace(open).is_empty()
                        || !crate::text::trim_css_collapsible_whitespace(close).is_empty()
                }),
            }
        }
    }
}

pub(in crate::layout) fn dom_children_keep_self_collapsing_parent(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    element.children.iter().all(|child| match &child.kind {
        NodeKind::Text(text) => dom_text_can_only_create_phantom_line_box(text, parent_style),
        NodeKind::Element(child) => {
            let signature = ElementSignature::with_sibling_list(
                child.tag.clone(),
                child.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = resolver.style_for_element(
                child,
                signature.clone(),
                stylesheets,
                Some(parent_style),
                ancestors,
            );
            if matches!(child_style.position, Position::Absolute | Position::Fixed)
                || child_style.float != Float::None
            {
                return true;
            }
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature);
            if is_normal_block_flow_child(child, &child_style) {
                is_self_collapsing_block_dom_with_resolver(
                    child,
                    &child_style,
                    stylesheets,
                    &child_ancestors,
                    resolver,
                    overflow_context,
                )
            } else {
                dom_inline_element_can_only_create_phantom_line_boxes(
                    child,
                    &child_style,
                    stylesheets,
                    &child_ancestors,
                    resolver,
                )
            }
        }
    })
}

/// Whether an inline DOM subtree can only create an ignorable line box.
///
/// The box-tree margin-collapse path already recognizes this state. Mirror it
/// while block children are still represented by DOM nodes so whitespace and
/// empty inline wrappers do not cause the two paths to disagree.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
fn dom_inline_element_can_only_create_phantom_line_boxes(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    // Keep the DOM fallback in lockstep with
    // `formatting_box_can_only_create_phantom_line_boxes`: an atomic inline
    // (including a replaced element) is an actual participant in its
    // enclosing line box, even when it owns no text descendants.  Treating
    // it as phantom lets an ancestor self-collapse after the inline formatter
    // has painted and advanced that line, so a following block can overlap
    // the atom.
    //
    // <https://www.w3.org/TR/css-display-3/#atomic-inline>
    // <https://www.w3.org/TR/css-inline-3/#line-box>
    if style.display.is_atomic_inline() || is_replaced_element(element) {
        return false;
    }
    style.display.is_inline_level()
        && style.display.is_flow()
        && super::formatting_boxes::inline_box_has_no_nonzero_inline_axis_component(style)
        && !style.content.is_generated()
        && !style
            .before_style
            .as_deref()
            .is_some_and(|style| style.content.is_generated())
        && !style
            .after_style
            .as_deref()
            .is_some_and(|style| style.content.is_generated())
        && {
            let sibling_tags = element_sibling_signature_list(element);
            let mut element_index = 0usize;
            element.children.iter().all(|child| match &child.kind {
                NodeKind::Text(text) => dom_text_can_only_create_phantom_line_box(text, style),
                NodeKind::Element(child) => {
                    let signature = ElementSignature::with_sibling_list(
                        child.tag.clone(),
                        child.attrs.clone(),
                        element_index,
                        sibling_tags.clone(),
                    );
                    element_index += 1;
                    let child_style = resolver.style_for_element(
                        child,
                        signature.clone(),
                        stylesheets,
                        Some(style),
                        ancestors,
                    );
                    let mut child_ancestors = ancestors.to_vec();
                    child_ancestors.push(signature);
                    dom_inline_element_can_only_create_phantom_line_boxes(
                        child,
                        &child_style,
                        stylesheets,
                        &child_ancestors,
                        resolver,
                    )
                }
            })
        }
}

fn dom_text_can_only_create_phantom_line_box(text: &str, style: &ComputedStyle) -> bool {
    style.white_space.collapses_spaces() && collapse_whitespace(text).is_empty()
}

/// Return whether direct inline content precedes the first normal-flow block
/// child in DOM order.
///
/// Only inline content before that child prevents the parent's block-start
/// margin from adjoining the child's margin. Inline content after a first
/// block belongs to a later anonymous block box and cannot retroactively
/// separate the parent's block-start edge:
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins> and
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
pub(in crate::layout) fn has_direct_inline_content_before_first_flow_child_dom_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) if !collapse_whitespace(text).is_empty() => return true,
            NodeKind::Text(_) => {}
            NodeKind::Element(child) => {
                let signature = ElementSignature::with_sibling_list(
                    child.tag.clone(),
                    child.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = resolver.style_for_element(
                    child,
                    signature.clone(),
                    stylesheets,
                    Some(parent_style),
                    ancestors,
                );
                if is_normal_block_flow_child(child, &child_style) {
                    return false;
                }
                let mut child_ancestors = ancestors.to_vec();
                child_ancestors.push(signature);
                if is_line_break_element(child)
                    || (child_style.display.is_inline_level()
                        && !dom_inline_element_can_only_create_phantom_line_boxes(
                            child,
                            &child_style,
                            stylesheets,
                            &child_ancestors,
                            &mut resolver,
                        ))
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Return whether a raw DOM block container has any non-phantom direct inline
/// contribution.
///
/// This is used for self-collapsing and parent/child-collapse checks, where an
/// inline run after a block child still gives the parent non-zero content.
/// A sized atomic inline box contributes a line box even when it has no text.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn has_direct_inline_content_dom_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    element.children.iter().any(|child| match &child.kind {
        NodeKind::Text(text) => !collapse_whitespace(text).is_empty(),
        NodeKind::Element(child) => {
            let signature = ElementSignature::with_sibling_list(
                child.tag.clone(),
                child.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = resolver.style_for_element(
                child,
                signature.clone(),
                stylesheets,
                Some(parent_style),
                ancestors,
            );
            if !child_style.display.is_inline_level() {
                return false;
            }
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature);
            !dom_inline_element_can_only_create_phantom_line_boxes(
                child,
                &child_style,
                stylesheets,
                &child_ancestors,
                resolver,
            )
        }
    })
}

/// Whether a raw DOM fallback contains only direct in-flow inline content.
///
/// The document canvas deliberately has no own text run, so this identifies
/// the narrow case where its direct DOM children must be collected as one
/// inline formatting context.  Block, float, replaced, and out-of-flow
/// children retain their dedicated layout paths.
///
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>
pub(in crate::layout) fn has_only_direct_in_flow_inline_dom_content_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut has_content = false;

    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                has_content |= !collapse_whitespace(text).is_empty();
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
                    signature,
                    stylesheets,
                    Some(parent_style),
                    ancestors,
                );
                if child_style.display.is_none() {
                    continue;
                }
                if !child_style.display.is_inline_level()
                    || child_style.float != Float::None
                    || matches!(child_style.position, Position::Absolute | Position::Fixed)
                    || is_replaced_element(child_element)
                {
                    return false;
                }
                has_content |= !inline_text(child_element).is_empty();
            }
        }
    }
    has_content
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_parent_style() -> ComputedStyle {
        ComputedStyle {
            font_size: 12.0,
            line_height: 14.4,
            color: CssColor::BLACK,
            ..ComputedStyle::initial()
        }
    }

    fn first_element_by_tag<'a>(node: &'a Node, tag: &str) -> Option<&'a Element> {
        match &node.kind {
            NodeKind::Text(_) => None,
            NodeKind::Element(element) => {
                if element.tag == tag {
                    return Some(element);
                }
                element
                    .children
                    .iter()
                    .find_map(|child| first_element_by_tag(child, tag))
            }
        }
    }

    #[test]
    fn margin_collapse_keeps_signed_layout_lengths_typed() {
        let mixed: LayoutLength = collapse_margins(layout_pt(12.0), layout_pt(-4.0));
        let negative: LayoutLength = collapse_margins(layout_pt(-3.0), layout_pt(-9.0));
        let set: LayoutLength = AdjoiningMarginSet::from_margin(layout_pt(8.0))
            .with_margin(layout_pt(-3.0))
            .with_margin(layout_pt(5.0))
            .collapsed();

        assert_eq!(mixed, layout_pt(8.0));
        assert_eq!(negative, layout_pt(-9.0));
        assert_eq!(set, layout_pt(5.0));
        assert_eq!(page_start_margin(layout_pt(7.0), true), layout_pt(0.0));
    }

    #[test]
    fn independent_formatting_contexts_keep_outer_margins_adjoining_siblings() {
        let root = dom::parse("<html><body><div></div><img></body></html>");
        let block = first_element_by_tag(&root, "div").expect("expected block element");
        let image = first_element_by_tag(&root, "img").expect("expected image element");

        for (name, display) in [
            ("grid", Display::GRID),
            ("flex", Display::FLEX),
            (
                "flow-root",
                Display::BLOCK.with_inner(DisplayInner::FlowRoot),
            ),
        ] {
            let mut style = ComputedStyle::initial();
            style.display = display;
            assert!(
                outer_margins_adjoin_block_siblings(block, &style),
                "a block-level {name} container's outer margins should adjoin a normal-flow sibling"
            );
        }

        let mut absolute = ComputedStyle::initial();
        absolute.position = Position::Absolute;
        assert!(!outer_margins_adjoin_block_siblings(block, &absolute));

        let mut fixed = ComputedStyle::initial();
        fixed.position = Position::Fixed;
        assert!(!outer_margins_adjoin_block_siblings(block, &fixed));

        let mut floated = ComputedStyle::initial();
        floated.float = Float::Left;
        assert!(!outer_margins_adjoin_block_siblings(block, &floated));

        let mut atomic_inline = ComputedStyle::initial();
        atomic_inline.display = Display::INLINE_BLOCK;
        assert!(!outer_margins_adjoin_block_siblings(block, &atomic_inline));

        assert!(!outer_margins_adjoin_block_siblings(
            image,
            &ComputedStyle::initial()
        ));
    }

    #[test]
    fn flow_root_does_not_self_collapse_margins() {
        let root = dom::parse("<div></div>");
        let element = first_element_by_tag(&root, "div").expect("expected block element");
        let mut style = ComputedStyle::initial();

        assert!(can_collapse_own_block_margins(
            element,
            &style,
            css::Edges::ZERO,
            false,
            css::Overflow::Visible,
        ));

        style.display = Display::BLOCK.with_inner(DisplayInner::FlowRoot);
        assert!(!can_collapse_own_block_margins(
            element,
            &style,
            css::Edges::ZERO,
            false,
            css::Overflow::Visible,
        ));
    }

    #[tokio::test]
    async fn sized_atomic_inline_blocks_start_margin_collapse_but_empty_inline_does_not() {
        let stylesheets = Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;

        let atomic_root = dom::parse(
            "<html><body><div><span style=\"display:inline-block; width:100px; height:100px\"></span><div></div></div></body></html>",
        );
        let atomic_parent = first_element_by_tag(&atomic_root, "div")
            .expect("expected the atomic inline's parent block");
        assert!(
            has_direct_inline_content_before_first_flow_child_dom_with_font_metrics(
                atomic_parent,
                &test_parent_style(),
                &stylesheets,
                &[],
                &mut font_system,
            )
        );

        let phantom_root =
            dom::parse("<html><body><div><span></span><div></div></div></body></html>");
        let phantom_parent = first_element_by_tag(&phantom_root, "div")
            .expect("expected the phantom inline's parent block");
        assert!(
            !has_direct_inline_content_before_first_flow_child_dom_with_font_metrics(
                phantom_parent,
                &test_parent_style(),
                &stylesheets,
                &[],
                &mut font_system,
            )
        );
    }
}
