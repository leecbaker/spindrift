use super::*;

pub(in crate::layout) fn is_default_block_container_tag(tag: &str) -> bool {
    // CSS Display defines block containers by computed display, while HTML only
    // supplies the default display values through the UA stylesheet.
    // https://www.w3.org/TR/css-display-3/#block-container
    css::default_style_for_tag(tag).display.is_block_level()
}

pub(in crate::layout) fn element_sibling_signature_list(
    element: &Element,
) -> ElementSiblingSignatureList {
    element_child_signature_list(element)
}

pub(in crate::layout) fn element_child_signature_list(
    element: &Element,
) -> ElementSiblingSignatureList {
    ElementSiblingSignatureList::from_vec(
        element
            .children
            .iter()
            .filter_map(|child| match &child.kind {
                NodeKind::Element(child_element) => Some(element_selector_signature(child_element)),
                NodeKind::Text(_) => None,
            })
            .collect::<Vec<_>>(),
    )
}

pub(in crate::layout) fn element_signature(element: &Element) -> ElementSignature {
    let signature = element_selector_signature(element);
    let mut element_signature =
        ElementSignature::new(signature.tag.clone(), signature.attrs.clone())
            .with_document_is_html(signature.document_is_html)
            .with_namespace(signature.namespace_url, signature.namespace_attrs)
            .with_child_list(signature.children, signature.has_text_child);
    element_signature.document_direction = signature.document_direction;
    element_signature.is_target = signature.is_target;
    element_signature.has_target_descendant = signature.has_target_descendant;
    element_signature
}

pub(in crate::layout) fn element_selector_signature(element: &Element) -> ElementSiblingSignature {
    let mut has_text_child = false;
    for child in &element.children {
        if let NodeKind::Text(text) = &child.kind {
            has_text_child |= !text.is_empty();
        }
    }
    let children = element_child_signature_list(element);
    let namespace_attrs = element
        .namespace_attrs
        .iter()
        .map(|attr| {
            ElementAttributeSignature::new(
                attr.namespace_url.clone(),
                attr.local_name.clone(),
                attr.value.clone(),
            )
        })
        .collect();
    let mut signature = ElementSiblingSignature::new(element.tag.clone(), element.attrs.clone())
        .with_namespace(element.namespace_url.clone(), namespace_attrs)
        .with_document_is_html(element.document_syntax == dom::DocumentSyntax::Html)
        .with_child_list(children, has_text_child);
    if let Some(direction) = element_document_direction(element) {
        signature = signature.with_document_direction(direction);
    }
    signature.is_target = element.is_target;
    signature
}

pub(in crate::layout) struct DomStyleResolver<'a> {
    pub(in crate::layout) font_system: &'a mut FontSystem,
}

impl<'a> DomStyleResolver<'a> {
    pub(in crate::layout) fn with_font_system(font_system: &'a mut FontSystem) -> Self {
        Self { font_system }
    }

    pub(in crate::layout) fn style_for_element(
        &mut self,
        element: &Element,
        signature: ElementSignature,
        stylesheets: &[Stylesheet],
        parent: Option<&ComputedStyle>,
        ancestors: &[ElementSignature],
    ) -> ComputedStyle {
        let inheritance_source = parent.cloned().unwrap_or_else(ComputedStyle::initial);
        let mut parent_ch_advance = css::fallback_ch_advance_for_style(&inheritance_source);
        let mut style = style_for_layout_element_with_parent_ch_advance(
            element,
            signature.clone(),
            stylesheets,
            parent,
            ancestors,
            parent_ch_advance,
        );
        if style
            .deferred_font_size
            .requires_parent_ch_advance(inheritance_source.font_size)
        {
            parent_ch_advance = self.font_system.ch_advance(&inheritance_source);
            style = style_for_layout_element_with_parent_ch_advance(
                element,
                signature.clone(),
                stylesheets,
                parent,
                ancestors,
                parent_ch_advance,
            );
        }
        let signature = layout_element_signature(element, signature, parent);
        let pseudo_parent_ch_advance = css::fallback_ch_advance_for_style(&style);
        css::apply_pseudo_rules_with_parent_ch_advance(
            &mut style,
            &signature,
            stylesheets,
            ancestors,
            pseudo_parent_ch_advance,
        );
        if style.pseudo_styles_require_parent_ch_advance() {
            let pseudo_parent_ch_advance = self.font_system.ch_advance(&style);
            css::apply_pseudo_rules_with_parent_ch_advance(
                &mut style,
                &signature,
                stylesheets,
                ancestors,
                pseudo_parent_ch_advance,
            );
        }
        style
    }
}

pub(in crate::layout) fn definition_list_column_groups_with_font_metrics<'a>(
    element: &'a Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> Vec<Vec<DefinitionListColumnItem<'a>>> {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    definition_list_column_groups_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
    )
}

pub(in crate::layout) fn definition_list_column_groups_with_resolver<'a>(
    element: &'a Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> Vec<Vec<DefinitionListColumnItem<'a>>> {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut groups: Vec<Vec<DefinitionListColumnItem<'a>>> = Vec::new();
    let mut current_group_has_description = false;

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
        let style = resolver.style_for_element(
            child_element,
            signature.clone(),
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        let item = DefinitionListColumnItem {
            element: child_element,
            signature,
            style,
            children: None,
        };

        match definition_list_item_kind(child_element) {
            DefinitionListItemKind::Term => {
                if groups.is_empty() || current_group_has_description {
                    groups.push(Vec::new());
                    current_group_has_description = false;
                }
                groups.last_mut().expect("group exists").push(item);
            }
            DefinitionListItemKind::Description => {
                if groups.is_empty() {
                    groups.push(Vec::new());
                }
                current_group_has_description = true;
                groups.last_mut().expect("group exists").push(item);
            }
            DefinitionListItemKind::Other => {
                groups.push(vec![item]);
                current_group_has_description = true;
            }
        }
    }

    groups
}

pub(in crate::layout) fn definition_list_column_groups_from_boxes<'a>(
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> Vec<Vec<DefinitionListColumnItem<'a>>> {
    let mut groups: Vec<Vec<DefinitionListColumnItem<'a>>> = Vec::new();
    let mut current_group_has_description = false;

    for child_box in child_boxes {
        let Some((element, signature, style, children)) = child_box.element_parts() else {
            continue;
        };
        let item = DefinitionListColumnItem {
            element,
            signature: signature.clone(),
            style: style.clone(),
            children: Some(children),
        };

        match definition_list_item_kind(element) {
            DefinitionListItemKind::Term => {
                if groups.is_empty() || current_group_has_description {
                    groups.push(Vec::new());
                    current_group_has_description = false;
                }
                groups.last_mut().expect("group exists").push(item);
            }
            DefinitionListItemKind::Description => {
                if groups.is_empty() {
                    groups.push(Vec::new());
                }
                current_group_has_description = true;
                groups.last_mut().expect("group exists").push(item);
            }
            DefinitionListItemKind::Other => {
                groups.push(vec![item]);
                current_group_has_description = true;
            }
        }
    }

    groups
}

pub(in crate::layout) fn has_table_or_replaced_descendant(element: &Element) -> bool {
    element.children.iter().any(|child| {
        let NodeKind::Element(child_element) = &child.kind else {
            return false;
        };
        is_table_or_replaced_element(child_element)
            || has_table_or_replaced_descendant(child_element)
    })
}

pub(in crate::layout) fn has_table_or_replaced_descendant_box(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| match child {
        box_tree::FormattingBox::Table(_) | box_tree::FormattingBox::Replaced(_) => true,
        box_tree::FormattingBox::AtomicInline(box_) => {
            is_table_or_replaced_element(box_.element)
                || has_table_or_replaced_descendant_box(&box_.children)
        }
        box_tree::FormattingBox::Block(box_) => {
            has_table_or_replaced_descendant_box(&box_.children)
        }
        box_tree::FormattingBox::Inline(box_) => {
            has_table_or_replaced_descendant_box(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            has_table_or_replaced_descendant_box(&box_.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            has_table_or_replaced_descendant_box(&box_.children)
        }
        box_tree::FormattingBox::Flex(box_) => has_table_or_replaced_descendant_box(&box_.children),
        box_tree::FormattingBox::Text(_) => false,
    })
}

pub(in crate::layout) fn inline_text_from_formatting_boxes(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> String {
    let mut text = String::new();
    collect_inline_text_from_formatting_boxes(child_boxes, &mut text);
    text
}

/// Returns first/last CSS `page` values with whether the value was specified.
///
/// CSS Paged Media's `auto` page value can explicitly end an ancestor named
/// page group, while an omitted `page` declaration inherits the surrounding
/// page group at a class-A boundary. Layout therefore has to preserve the
/// distinction instead of flattening both to `None`:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn page_value_sources_from_style_and_children(
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> ((Option<String>, bool), (Option<String>, bool)) {
    let own = style.page_name_specified.then(|| style.page_name.clone());
    let mut start = own
        .clone()
        .map(|name| (name, true))
        .unwrap_or((None, false));
    let mut end = own.map(|name| (name, true)).unwrap_or((None, false));
    if style.display.is_flex() {
        return (start, end);
    }
    // Only child boxes to which `page` applies can propagate start/end page
    // values. Formatting whitespace can generate a normal-flow anonymous
    // box, but it does not create a class-A break point and must not mask the
    // first block-level child's page value.
    // <https://www.w3.org/TR/css-page-3/#using-named-pages>
    let mut normal_flow_children = child_boxes
        .iter()
        .filter(|child| formatting_box_is_page_value_participant(child));
    let Some(first) = normal_flow_children.next() else {
        return (start, end);
    };
    let first_sources = formatting_box_page_value_sources(first);
    if first_sources.0.1 {
        start = first_sources.0.clone();
    }
    // A single normal-flow child supplies both boundaries. Compute its paired
    // summary once: recursively querying it separately for start and end
    // would revisit the same sole-child chain twice at every depth.
    let last_sources = normal_flow_children
        .next_back()
        .map(formatting_box_page_value_sources)
        .unwrap_or(first_sources);
    if last_sources.1.1 {
        end = last_sources.1;
    }
    (start, end)
}

pub(in crate::layout) fn formatting_box_page_value_sources(
    box_: &box_tree::FormattingBox<'_>,
) -> ((Option<String>, bool), (Option<String>, bool)) {
    match box_ {
        box_tree::FormattingBox::Block(box_) => {
            page_value_sources_from_style_and_children(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::Inline(box_) => {
            page_value_sources_from_style_and_children(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            page_value_sources_from_style_and_children(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::AtomicInline(box_) => {
            if box_.style.display.is_replaced() {
                return ((None, false), (None, false));
            }
            page_value_sources_from_style_and_children(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::Flex(box_) => {
            let own = box_
                .style
                .page_name_specified
                .then(|| box_.style.page_name.clone())
                .map(|name| (name, true))
                .unwrap_or((None, false));
            (own.clone(), own)
        }
        box_tree::FormattingBox::Table(box_) => {
            page_value_sources_from_style_and_children(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::Replaced(box_) => {
            let own = box_
                .style
                .page_name_specified
                .then(|| box_.style.page_name.clone())
                .map(|name| (name, true))
                .unwrap_or((None, false));
            (own.clone(), own)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            page_value_sources_from_style_and_children(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::Text(_) => ((None, false), (None, false)),
    }
}

/// Whether a formatting box can contribute a propagated start/end `page`
/// value to its parent.
///
/// CSS Paged Media propagates values only from child boxes to which `page`
/// applies. Anonymous wrappers generated solely around formatting whitespace or
/// out-of-flow descendants create no class-A boundary, so they are transparent
/// for this purpose:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn formatting_box_is_page_value_participant(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    if !formatting_box_is_in_normal_flow(box_) {
        return false;
    }
    match box_ {
        box_tree::FormattingBox::Text(box_) => {
            !(box_.text.is_empty()
                || (box_.style.white_space.collapses_spaces()
                    && box_.text.chars().all(is_css_collapsible_whitespace)))
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .any(formatting_box_is_page_value_participant),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .children
            .iter()
            .any(formatting_box_is_page_value_participant),
        _ => !formatting_box_can_only_create_phantom_line_boxes(box_),
    }
}

/// Resolves a child boundary page value in its parent page scope.
///
/// At CSS Paged Media class-A sibling boundaries, an explicitly specified
/// child `page` value wins. An explicit `page:auto` resolves to the nearest
/// non-auto ancestor, which is the immediate parent when it has a named page;
/// an omitted value follows that same parent page group:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn page_boundary_name_in_parent_scope(
    source: (Option<String>, bool),
    parent_style: &ComputedStyle,
) -> Option<String> {
    if source.1 {
        // Preserve explicit `page:auto`: it clears the parent group instead
        // of falling back to the parent's named page value.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        source.0
    } else if parent_style.page_name_specified {
        parent_style.page_name.clone()
    } else {
        None
    }
}

/// Returns the first and last effective CSS `page` values for one formatting box.
///
/// Absolutely positioned, fixed-position, floated, running, and display-none
/// boxes are not in normal flow and therefore do not create class A sibling
/// page-name boundaries:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
pub(in crate::layout) fn formatting_box_page_values(
    box_: &box_tree::FormattingBox<'_>,
) -> (Option<String>, Option<String>) {
    let ((start, _), (end, _)) = formatting_box_page_value_sources(box_);
    (start, end)
}

pub(in crate::layout) fn formatting_box_is_in_normal_flow(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    match box_ {
        box_tree::FormattingBox::Block(box_) => style_is_in_normal_flow(&box_.style),
        box_tree::FormattingBox::Inline(box_) => style_is_in_normal_flow(&box_.style),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            style_is_in_normal_flow(&box_.style)
        }
        box_tree::FormattingBox::AtomicInline(box_) => style_is_in_normal_flow(&box_.style),
        box_tree::FormattingBox::Flex(box_) => style_is_in_normal_flow(&box_.style),
        box_tree::FormattingBox::Table(box_) => style_is_in_normal_flow(&box_.style),
        box_tree::FormattingBox::Replaced(box_) => style_is_in_normal_flow(&box_.style),
        box_tree::FormattingBox::AnonymousBlock(box_) => style_is_in_normal_flow(&box_.style),
        box_tree::FormattingBox::Text(_) => true,
    }
}

/// Returns true for an explicit zero-height page-owning block boundary.
///
/// CSS Paged Media forms page groups at class A break opportunities, but WPT
/// `page-name-zero-height-001-print.html` treats consecutive `height: 0`
/// page-owning siblings as not forcing separate page boxes. Their overflowing
/// contents are laid out in the next nonzero page group:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn formatting_box_is_zero_height_page_boundary(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    let Some((_, _, style, _)) = box_.element_parts() else {
        return false;
    };
    formatting_box_is_in_normal_flow(box_)
        && style.page_name_specified
        && style
            .box_values
            .height
            .length_if_no_percent()
            .is_some_and(|height| height.abs() < 0.01)
}

/// Finds the page value that a zero-height page-owning sibling run coalesces into.
///
/// Consecutive explicit zero-height page-owning boxes do not each create a
/// separate page group. The run is laid out in the next nonzero in-flow
/// sibling's start page group when one exists, otherwise in the current box's
/// own start group:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn coalesced_zero_height_page_start(
    child_boxes: &[box_tree::FormattingBox<'_>],
    current_index: usize,
) -> Option<String> {
    child_boxes
        .iter()
        .skip(current_index + 1)
        .filter(|child| formatting_box_is_in_normal_flow(child))
        .find(|child| !formatting_box_is_zero_height_page_boundary(child))
        .and_then(|child| formatting_box_page_values(child).0)
        .or_else(|| formatting_box_page_values(&child_boxes[current_index]).0)
}

pub(in crate::layout) fn style_is_in_normal_flow(style: &ComputedStyle) -> bool {
    !style.display.is_none()
        && !matches!(style.position, Position::Absolute | Position::Fixed)
        && style.float == Float::None
        && style.running_element_name.is_none()
}

pub(in crate::layout) fn formatting_box_has_inline_content(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| match child {
        _ if box_tree::is_out_of_flow_box(child) => true,
        box_tree::FormattingBox::Text(box_) => {
            inline_text_has_non_phantom_content(&box_.text, &box_.style)
        }
        box_tree::FormattingBox::Inline(box_) => {
            // Inline boxes with generated pseudo content must keep the rich
            // inline collector active even when their DOM text is empty. CSS
            // 2.2 also routes floated inline boxes through inline collection so
            // they can be blockified and placed as floats. Empty inline boxes
            // with owned inline-axis margin, border, or padding still generate
            // inline boxes whose decorations and advance must be preserved:
            // <https://www.w3.org/TR/CSS22/box.html#inline-boxes>.
            box_.style
                .before_style
                .as_deref()
                .is_some_and(|style| style.content.is_generated())
                || box_
                    .style
                    .after_style
                    .as_deref()
                    .is_some_and(|style| style.content.is_generated())
                || box_.style.content.is_generated()
                || box_.style.float != Float::None
                || inline_box_fragment_has_owned_inline_edge(&box_.style, box_.fragment_edges)
                || formatting_box_has_inline_content(&box_.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            formatting_box_has_inline_content(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(_) => false,
        box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_) => true,
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_) => false,
    })
}

/// Return whether text contributes a line box after CSS white-space trimming.
///
/// A run made solely of collapsible space at an otherwise empty block edge
/// does not create a line box. Keeping that distinction here lets margin
/// adjacency use the same content test as inline collection without hiding
/// preserved whitespace or non-space inline content.
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
pub(in crate::layout) fn inline_text_has_non_phantom_content(
    text: &str,
    style: &ComputedStyle,
) -> bool {
    let text = normalized_text_for_style(text, style);
    if style.white_space.collapses_spaces() {
        !crate::text::trim_css_collapsible_whitespace(&text).is_empty()
    } else {
        !text.is_empty()
    }
}

fn inline_box_fragment_has_owned_inline_edge(
    style: &ComputedStyle,
    fragment_edges: box_tree::InlineBoxFragmentEdges,
) -> bool {
    (fragment_edges.owns_start && inline_box_logical_edge_has_nonzero_component(style, true))
        || (fragment_edges.owns_end && inline_box_logical_edge_has_nonzero_component(style, false))
}

fn inline_box_logical_edge_has_nonzero_component(style: &ComputedStyle, is_start: bool) -> bool {
    let side = if is_start {
        inline_start_side(style.writing_mode, style.direction)
    } else {
        inline_end_side(style.writing_mode, style.direction)
    };
    let borders = used_border_widths(style);
    let (margin, border, padding) = match side {
        PhysicalSide::Top => (style.margin.top, borders.top, style.padding.top),
        PhysicalSide::Right => (style.margin.right, borders.right, style.padding.right),
        PhysicalSide::Bottom => (style.margin.bottom, borders.bottom, style.padding.bottom),
        PhysicalSide::Left => (style.margin.left, borders.left, style.padding.left),
    };
    margin.abs() > 0.001 || border.abs() > 0.001 || padding.abs() > 0.001
}

pub(in crate::layout) fn collect_inline_text_from_formatting_boxes(
    child_boxes: &[box_tree::FormattingBox<'_>],
    output: &mut String,
) {
    for child in child_boxes {
        match child {
            box_tree::FormattingBox::Text(box_) => output.push_str(&box_.text),
            box_tree::FormattingBox::Inline(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.children, output);
            }
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.children, output);
            }
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.children, output);
            }
            box_tree::FormattingBox::AtomicInline(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.children, output);
            }
            box_tree::FormattingBox::Block(_)
            | box_tree::FormattingBox::Table(_)
            | box_tree::FormattingBox::Flex(_)
            | box_tree::FormattingBox::Replaced(_) => {}
        }
    }
}

pub(in crate::layout) fn has_styled_inline_descendant_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_styled_inline_descendant_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
    )
}

pub(in crate::layout) fn has_styled_inline_descendant_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
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
        let child_style = resolver.style_for_element(
            child_element,
            signature.clone(),
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        if child_style.display.is_none() || child_style.display.is_block_level() {
            return false;
        }
        inline_style_affects_line(parent_style, &child_style) || {
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature);
            has_styled_inline_descendant_with_resolver(
                child_element,
                &child_style,
                stylesheets,
                &child_ancestors,
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
        || child.float != Float::None
        || child.color != parent.color
        || child.font_family != parent.font_family
        || child.font_size != parent.font_size
        || child.font_style != parent.font_style
        || child.font_weight != parent.font_weight
        || child.font_width != parent.font_width
        || child.line_height != parent.line_height
        || child.text_decoration != parent.text_decoration
        || child.text_transform != parent.text_transform
        || child.vertical_align != parent.vertical_align
        || child.white_space != parent.white_space
}

pub(in crate::layout) fn has_direct_inline_replaced_child(element: &Element) -> bool {
    element.children.iter().any(|child| {
        matches!(&child.kind, NodeKind::Element(child_element) if is_replaced_element(child_element))
    })
}

pub(in crate::layout) fn has_direct_flow_child_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_direct_flow_child_with_resolver(element, parent_style, stylesheets, &mut resolver)
}

pub(in crate::layout) fn has_direct_flow_child_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
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
        let style = resolver.style_for_element(
            child_element,
            signature,
            stylesheets,
            Some(parent_style),
            &[],
        );
        if is_replaced_element(child_element) && style.display.is_inline_level() {
            return false;
        }
        style.display.is_block_level() || is_html_table_element(child_element)
    })
}

pub(in crate::layout) fn has_ordered_mixed_flow_content_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
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

pub(in crate::layout) fn has_ordered_mixed_flow_content_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
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
                    signature,
                    stylesheets,
                    Some(parent_style),
                    ancestors,
                );
                let is_flow_child = is_normal_block_flow_child(child_element, &child_style)
                    || is_replaced_element(child_element);
                if is_flow_child {
                    has_flow = true;
                } else if is_line_break_element(child_element)
                    || !inline_text(child_element).is_empty()
                {
                    has_inline = true;
                }
            }
        }

        if has_inline && has_flow {
            return true;
        }
    }

    false
}

/// Returns whether a block container's start edge can adjoin child margins.
///
/// CSS 2.2 allows parent/child vertical margin collapse through ordinary flow
/// block boxes without border, padding, or inline content. A specified
/// `height` does not prevent the block-start margins from adjoining; it only
/// prevents a last child's block-end margin from collapsing through the
/// parent. A non-auto `min-height` similarly matters to the block-end case.
/// Layout and paint containment establish independent formatting contexts, so
/// their principal boxes never adjoin descendant margins.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
/// <https://www.w3.org/TR/css-display-3/#valdef-display-flow-root>
/// <https://www.w3.org/TR/css-contain-1/#containment-layout>
pub(in crate::layout) fn can_collapse_block_start_margin(
    style: &ComputedStyle,
    border_widths: css::Edges,
    has_direct_inline_content: bool,
    used_overflow: css::Overflow,
) -> bool {
    style.display.is_flow()
        && !style.display.establishes_block_formatting_context()
        && !style_establishes_multicol_formatting_context(style)
        && !style.contain.layout
        && !style.contain.paint
        && style.float == Float::None
        && !has_direct_inline_content
        && used_overflow == css::Overflow::Visible
        && style.padding.top == 0.0
        && border_widths.top == 0.0
}

/// Returns whether a block container's end edge can adjoin child margins.
///
/// This is the block-end counterpart to `can_collapse_block_start_margin`. The
/// block layout pass later decides whether the final min-height-constrained
/// content height actually keeps this adjoining margin inside the parent.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
/// <https://www.w3.org/TR/css-display-3/#valdef-display-flow-root>
/// <https://www.w3.org/TR/css-contain-1/#containment-layout>
pub(in crate::layout) fn can_collapse_block_end_margin(
    style: &ComputedStyle,
    border_widths: css::Edges,
    has_direct_inline_content: bool,
    used_overflow: css::Overflow,
) -> bool {
    style.display.is_flow()
        && !style.display.establishes_block_formatting_context()
        && !style_establishes_multicol_formatting_context(style)
        && !style.contain.layout
        && !style.contain.paint
        && style.float == Float::None
        && !has_direct_inline_content
        && used_overflow == css::Overflow::Visible
        && style.padding.bottom == 0.0
        && border_widths.bottom == 0.0
        && has_auto_height(style)
}

/// Returns whether the computed column properties establish a multi-column
/// formatting context.
///
/// A multicol container establishes an independent formatting context, so its
/// contents do not participate in margin collapsing with the container even
/// when its inner display type is ordinary `flow`.
/// <https://www.w3.org/TR/css-multicol-1/#multicol-container>
pub(in crate::layout) fn style_establishes_multicol_formatting_context(
    style: &ComputedStyle,
) -> bool {
    style.column_count.is_some()
        || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
        || matches!(style.column_height, css::ComputedColumnHeight::Length(_))
}

/// Returns whether a child participates in normal block flow for margin collapse.
///
/// CSS 2.2 defines floated boxes as out of normal flow, so they must not
/// contribute adjoining margins to their block container:
/// <https://www.w3.org/TR/CSS22/visuren.html#positioning-scheme> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
pub(in crate::layout) fn is_normal_block_flow_child(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    !matches!(style.position, Position::Absolute | Position::Fixed)
        && style.float == Float::None
        && (style.display.is_block_level() || is_html_table_element(element))
}

pub(in crate::layout) fn is_collapsible_block_child(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    is_normal_block_flow_child(element, style)
        && !style.display.is_flex()
        && !is_replaced_element(element)
}

/// Returns whether a normal-flow block-level child's outer margins can adjoin siblings.
///
/// CSS margin collapse applies only to adjoining block-flow boxes. Grid
/// containers establish an independent formatting context, so their outer
/// block margins remain separate from adjacent normal-flow siblings. This is
/// also what keeps a sequence of scrollable Grid containers from losing each
/// later block-start margin during parent-flow preprocessing:
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins> and
/// <https://www.w3.org/TR/css-grid-1/#grid-containers>.
pub(in crate::layout) fn is_sibling_margin_collapsible_block_child(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    is_normal_block_flow_child(element, style)
        && !style.display.is_grid()
        && !is_replaced_element(element)
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct RelativeOffset {
    pub(in crate::layout) vector: ContainerVector,
}

impl RelativeOffset {
    pub(in crate::layout) fn zero() -> Self {
        Self {
            vector: ContainerVector::zero(),
        }
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.vector.x
    }

    pub(in crate::layout) fn y(self) -> f32 {
        self.vector.y
    }
}

pub(in crate::layout) fn relative_position_offset(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> RelativeOffset {
    if !matches!(style.position, Position::Relative | Position::Sticky) {
        return RelativeOffset::zero();
    }
    let left = used_inset_left(style, containing_block);
    let right = used_inset_right(style, containing_block);
    let top = used_inset_top(style, containing_block);
    let bottom = used_inset_bottom(style, containing_block);
    // CSS 2.1 9.4.3/9.3.2: relative positioning offsets the visual box while
    // preserving its normal-flow space. Opposing insets over-constrain the axis;
    // for left-to-right content, `left` wins horizontally, and `top` wins
    // vertically.
    let x = left.unwrap_or_else(|| -right.unwrap_or(0.0));
    let y = bottom.unwrap_or_else(|| -top.unwrap_or(0.0));
    RelativeOffset {
        vector: ContainerVector::new(x, y),
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Resolve a normal-flow box's relative-positioning offset.
    ///
    /// A flex or grid item's replayed formatting context has an already used
    /// physical content box. Descendants in normal flow use that box for their
    /// relative-position percentage bases, without treating it as an absolute
    /// positioning containing block:
    /// <https://www.w3.org/TR/css-position-3/#relative-positioning>.
    pub(in crate::layout) fn normal_flow_relative_position_offset(
        &self,
        style: &ComputedStyle,
    ) -> RelativeOffset {
        let Some(containing_block) = self.normal_flow_relative_containing_blocks.last() else {
            return relative_position_offset(style, self.current_containing_block());
        };
        if !matches!(style.position, Position::Relative | Position::Sticky) {
            return RelativeOffset::zero();
        }

        let width_basis =
            PercentageBasis::definite(containing_block.physical_content_width.content_box_length());
        let height_basis = containing_block
            .physical_content_height
            .map(|height| PercentageBasis::definite(height.content_box_length()))
            .unwrap_or_else(PercentageBasis::indefinite);
        let left = used_length_percentage_or_auto_with_basis(
            style.box_values.inset_left.clone(),
            width_basis,
        )
        .map(|length| length.points());
        let right = used_length_percentage_or_auto_with_basis(
            style.box_values.inset_right.clone(),
            width_basis,
        )
        .map(|length| length.points());
        let top = used_length_percentage_or_auto_with_basis(
            style.box_values.inset_top.clone(),
            height_basis,
        )
        .map(|length| length.points());
        let bottom = used_length_percentage_or_auto_with_basis(
            style.box_values.inset_bottom.clone(),
            height_basis,
        )
        .map(|length| length.points());
        RelativeOffset {
            vector: ContainerVector::new(
                left.unwrap_or_else(|| -right.unwrap_or(0.0)),
                bottom.unwrap_or_else(|| -top.unwrap_or(0.0)),
            ),
        }
    }
}

pub(in crate::layout) fn has_later_normal_block_flow_child_with_font_metrics(
    element: &Element,
    start_element_index: usize,
    sibling_tags: &ElementSiblingSignatureList,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_later_normal_block_flow_child_with_resolver(
        element,
        start_element_index,
        sibling_tags,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
    )
}
