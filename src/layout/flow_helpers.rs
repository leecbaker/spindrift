use super::*;

pub(super) fn is_default_block_container_tag(tag: &str) -> bool {
    // CSS Display defines block containers by computed display, while HTML only
    // supplies the default display values through the UA stylesheet.
    // https://www.w3.org/TR/css-display-3/#block-container
    css::default_style_for_tag(tag).display.is_block_level()
}

pub(super) fn element_sibling_tags(element: &Element) -> Vec<ElementSiblingSignature> {
    element
        .children
        .iter()
        .filter_map(|child| match &child.kind {
            NodeKind::Element(child_element) => Some(element_selector_signature(child_element)),
            NodeKind::Text(_) => None,
        })
        .collect()
}

pub(super) fn element_selector_signature(element: &Element) -> ElementSiblingSignature {
    let mut children = Vec::new();
    let mut has_text_child = false;
    for child in &element.children {
        match &child.kind {
            NodeKind::Element(child_element) => {
                children.push(element_selector_signature(child_element))
            }
            NodeKind::Text(text) => has_text_child |= !text.is_empty(),
        }
    }
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
        .with_children(children, has_text_child);
    signature.is_target = element.is_target;
    signature
}

struct DomStyleResolver<'a> {
    font_system: &'a mut FontSystem,
}

impl<'a> DomStyleResolver<'a> {
    fn with_font_system(font_system: &'a mut FontSystem) -> Self {
        Self { font_system }
    }

    fn style_for_element(
        &mut self,
        element: &Element,
        signature: ElementSignature,
        stylesheets: &[Stylesheet],
        parent: Option<&ComputedStyle>,
        ancestors: &[ElementSignature],
    ) -> ComputedStyle {
        let inheritance_source = parent.cloned().unwrap_or_else(ComputedStyle::initial);
        let parent_ch_advance = self.font_system.ch_advance(&inheritance_source);
        let mut style = style_for_layout_element_with_parent_ch_advance(
            element,
            signature.clone(),
            stylesheets,
            parent,
            ancestors,
            parent_ch_advance,
        );
        let pseudo_parent_ch_advance = self.font_system.ch_advance(&style);
        let signature = layout_element_signature(element, signature, parent);
        css::apply_pseudo_rules_with_parent_ch_advance(
            &mut style,
            &signature,
            stylesheets,
            ancestors,
            pseudo_parent_ch_advance,
        );
        style
    }
}

pub(super) fn definition_list_column_groups_with_font_metrics<'a>(
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

fn definition_list_column_groups_with_resolver<'a>(
    element: &'a Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> Vec<Vec<DefinitionListColumnItem<'a>>> {
    let sibling_tags = element_sibling_tags(element);
    let mut element_index = 0usize;
    let mut groups: Vec<Vec<DefinitionListColumnItem<'a>>> = Vec::new();
    let mut current_group_has_description = false;

    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            continue;
        };
        let signature = ElementSignature::with_siblings(
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

pub(super) fn definition_list_column_groups_from_boxes<'a>(
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

pub(super) fn has_table_or_replaced_descendant(element: &Element) -> bool {
    element.children.iter().any(|child| {
        let NodeKind::Element(child_element) = &child.kind else {
            return false;
        };
        is_table_or_replaced_element(child_element)
            || has_table_or_replaced_descendant(child_element)
    })
}

pub(super) fn has_table_or_replaced_descendant_box(
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
        box_tree::FormattingBox::Line(_) | box_tree::FormattingBox::Text(_) => false,
    })
}

pub(super) fn inline_text_from_formatting_boxes(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> String {
    let mut text = String::new();
    collect_inline_text_from_formatting_boxes(child_boxes, &mut text);
    text
}

/// Returns the first and last effective CSS `page` values for a box subtree.
///
/// CSS Paged Media defines named page groups from the `page` property at class
/// A break boundaries. WeasyPrint models this with start/end page values for
/// each formatting box; this helper mirrors that approach for our normalized
/// box tree:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(super) fn page_values_from_style_and_children(
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> (Option<String>, Option<String>) {
    let ((start, _), (end, _)) = page_value_sources_from_style_and_children(style, child_boxes);
    (start, end)
}

/// Returns first/last CSS `page` values with whether the value was specified.
///
/// CSS Paged Media's `auto` page value can explicitly end an ancestor named
/// page group, while an omitted `page` declaration inherits the surrounding
/// page group at a class-A boundary. Layout therefore has to preserve the
/// distinction instead of flattening both to `None`:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(super) fn page_value_sources_from_style_and_children(
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
    let normal_flow_children = child_boxes
        .iter()
        .filter(|child| formatting_box_is_in_normal_flow(child))
        .collect::<Vec<_>>();
    if let Some(first) = normal_flow_children.first() {
        let child_start = formatting_box_page_value_sources(first).0;
        if child_start.1 {
            start = child_start;
        }
    }
    if let Some(last) = normal_flow_children.last() {
        let child_end = formatting_box_page_value_sources(last).1;
        if child_end.1 {
            end = child_end;
        }
    }
    (start, end)
}

/// Returns the first and last effective CSS `page` values for formatting boxes.
///
/// Parent boxes use the first and last in-flow child values to decide whether
/// a class A page boundary is needed between siblings:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(super) fn formatting_boxes_page_values(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> (Option<String>, Option<String>) {
    let normal_flow_children = child_boxes
        .iter()
        .filter(|child| formatting_box_is_in_normal_flow(child))
        .collect::<Vec<_>>();
    let start = normal_flow_children
        .first()
        .and_then(|child| formatting_box_page_values(child).0);
    let end = normal_flow_children
        .last()
        .and_then(|child| formatting_box_page_values(child).1);
    (start, end)
}

pub(super) fn formatting_box_page_value_sources(
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
        box_tree::FormattingBox::Line(_) | box_tree::FormattingBox::Text(_) => {
            ((None, false), (None, false))
        }
    }
}

/// Resolves a child boundary page value in its parent page scope.
///
/// At CSS Paged Media class-A sibling boundaries, an explicitly specified
/// child `page` value wins. If the child omitted `page`, it remains in the
/// parent's named page group; if it specified `page:auto`, the source flag is
/// true and the returned value stays `None`:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(super) fn page_boundary_name_in_parent_scope(
    source: (Option<String>, bool),
    parent_style: &ComputedStyle,
) -> Option<String> {
    if source.1 {
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
pub(super) fn formatting_box_page_values(
    box_: &box_tree::FormattingBox<'_>,
) -> (Option<String>, Option<String>) {
    let ((start, _), (end, _)) = formatting_box_page_value_sources(box_);
    (start, end)
}

pub(super) fn formatting_box_is_in_normal_flow(box_: &box_tree::FormattingBox<'_>) -> bool {
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
        box_tree::FormattingBox::Line(_) | box_tree::FormattingBox::Text(_) => true,
    }
}

/// Returns true for an explicit zero-height page-owning block boundary.
///
/// CSS Paged Media forms page groups at class A break opportunities, but WPT
/// `page-name-zero-height-001-print.html` treats consecutive `height: 0`
/// page-owning siblings as not forcing separate page boxes. Their overflowing
/// contents are laid out in the next nonzero page group:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(super) fn formatting_box_is_zero_height_page_boundary(
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
pub(super) fn coalesced_zero_height_page_start(
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

fn style_is_in_normal_flow(style: &ComputedStyle) -> bool {
    !style.display.is_none()
        && !matches!(style.position, Position::Absolute | Position::Fixed)
        && style.float == Float::None
        && style.running_element_name.is_none()
}

pub(super) fn formatting_box_has_inline_content(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| match child {
        _ if box_tree::is_out_of_flow_box(child) => true,
        box_tree::FormattingBox::Text(box_) => {
            !normalized_text_for_style(&box_.text, &box_.style).is_empty()
        }
        box_tree::FormattingBox::Line(box_) => box_
            .children
            .iter()
            .any(|text| !normalized_text_for_style(&text.text, &text.style).is_empty()),
        box_tree::FormattingBox::Inline(box_) => {
            // Inline boxes with generated pseudo content must keep the rich
            // inline collector active even when their DOM text is empty.
            box_.style.before_style.is_some()
                || box_.style.after_style.is_some()
                || box_.style.content.is_generated()
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

pub(super) fn collect_inline_text_from_formatting_boxes(
    child_boxes: &[box_tree::FormattingBox<'_>],
    output: &mut String,
) {
    for child in child_boxes {
        match child {
            box_tree::FormattingBox::Text(box_) => output.push_str(&box_.text),
            box_tree::FormattingBox::Line(box_) => {
                for text_box in &box_.children {
                    output.push_str(&text_box.text);
                }
            }
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

pub(super) fn has_styled_inline_descendant_with_font_metrics(
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

fn has_styled_inline_descendant_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_tags(element);
    let mut element_index = 0usize;
    element.children.iter().any(|child| {
        let NodeKind::Element(child_element) = &child.kind else {
            return false;
        };
        let signature = ElementSignature::with_siblings(
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

pub(super) fn inline_style_affects_line(parent: &ComputedStyle, child: &ComputedStyle) -> bool {
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

pub(super) fn has_direct_inline_replaced_child(element: &Element) -> bool {
    element.children.iter().any(|child| {
        matches!(&child.kind, NodeKind::Element(child_element) if is_replaced_element(child_element))
    })
}

pub(super) fn has_direct_flow_child_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_direct_flow_child_with_resolver(element, parent_style, stylesheets, &mut resolver)
}

fn has_direct_flow_child_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_tags(element);
    let mut element_index = 0usize;
    element.children.iter().any(|child| {
        let NodeKind::Element(child_element) = &child.kind else {
            return false;
        };
        let signature = ElementSignature::with_siblings(
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

pub(super) fn has_ordered_mixed_flow_content_with_font_metrics(
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

fn has_ordered_mixed_flow_content_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    if suppresses_ordered_mixed_flow_detection(element) {
        return false;
    }

    let sibling_tags = element_sibling_tags(element);
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
                let signature = ElementSignature::with_siblings(
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
/// block boxes without border, padding, inline content, or fixed height. A
/// non-auto `min-height` only prevents a last child's bottom margin from
/// collapsing through when that margin also adjoins the parent's top edge; the
/// final used min/max-height constraint is applied after child layout.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
/// <https://www.w3.org/TR/css-display-3/#valdef-display-flow-root>
pub(super) fn can_collapse_block_start_margin(
    style: &ComputedStyle,
    border_widths: css::Edges,
    has_direct_inline_content: bool,
) -> bool {
    style.display.is_flow()
        && !has_direct_inline_content
        && style.padding.top == 0.0
        && border_widths.top == 0.0
        && has_auto_height(style)
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
pub(super) fn can_collapse_block_end_margin(
    style: &ComputedStyle,
    border_widths: css::Edges,
    has_direct_inline_content: bool,
) -> bool {
    style.display.is_flow()
        && !has_direct_inline_content
        && style.padding.bottom == 0.0
        && border_widths.bottom == 0.0
        && has_auto_height(style)
}

/// Returns whether a child participates in normal block flow for margin collapse.
///
/// CSS 2.2 defines floated boxes as out of normal flow, so they must not
/// contribute adjoining margins to their block container:
/// <https://www.w3.org/TR/CSS22/visuren.html#positioning-scheme> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
pub(super) fn is_normal_block_flow_child(element: &Element, style: &ComputedStyle) -> bool {
    !matches!(style.position, Position::Absolute | Position::Fixed)
        && style.float == Float::None
        && (style.display.is_block_level() || is_html_table_element(element))
}

pub(super) fn is_collapsible_block_child(element: &Element, style: &ComputedStyle) -> bool {
    is_normal_block_flow_child(element, style)
        && !style.display.is_flex()
        && !is_replaced_element(element)
}

/// Returns whether a normal-flow block-level child's outer margins can adjoin siblings.
///
/// CSS 2.2 collapses adjoining vertical margins between in-flow block-level
/// siblings. A flex container establishes its own formatting context, so its
/// margins do not collapse with its contents, but its outer margins still
/// participate as a normal-flow block-level sibling:
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-containers>.
pub(super) fn is_sibling_margin_collapsible_block_child(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    is_normal_block_flow_child(element, style) && !is_replaced_element(element)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RelativeOffset {
    pub(super) x: f32,
    pub(super) y: f32,
}

pub(super) fn relative_position_offset(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> RelativeOffset {
    if !matches!(style.position, Position::Relative | Position::Sticky) {
        return RelativeOffset { x: 0.0, y: 0.0 };
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
    RelativeOffset { x, y }
}

pub(super) fn has_later_normal_block_flow_child_with_font_metrics(
    element: &Element,
    start_element_index: usize,
    sibling_tags: &[ElementSiblingSignature],
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

fn has_later_normal_block_flow_child_with_resolver(
    element: &Element,
    start_element_index: usize,
    sibling_tags: &[ElementSiblingSignature],
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let mut element_index = 0usize;
    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            continue;
        };
        let current_index = element_index;
        element_index += 1;
        if current_index < start_element_index {
            continue;
        }
        let signature = ElementSignature::with_siblings(
            child_element.tag.clone(),
            child_element.attrs.clone(),
            current_index,
            sibling_tags.to_vec(),
        );
        let style = resolver.style_for_element(
            child_element,
            signature,
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        if is_normal_block_flow_child(child_element, &style) || is_replaced_element(child_element) {
            return true;
        }
    }
    false
}

pub(super) fn has_later_normal_block_flow_box_child(
    child_boxes: &[box_tree::FormattingBox<'_>],
    start_index: usize,
    parent: &Element,
) -> bool {
    child_boxes
        .iter()
        .skip(start_index)
        .any(|child| formatting_box_is_normal_block_flow_sibling(child, parent))
}

/// Return whether a formatting box is a later in-flow block sibling.
///
/// CSS 2.2 mixed-flow normalization creates anonymous block boxes around runs
/// of inline content. Those anonymous boxes are real block-level siblings for
/// normal-flow ordering and margin-adjacency decisions; a preceding block must
/// not collapse its end margin through the parent as if the anonymous block did
/// not exist:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
fn formatting_box_is_normal_block_flow_sibling(
    child: &box_tree::FormattingBox<'_>,
    parent: &Element,
) -> bool {
    match child {
        box_tree::FormattingBox::AnonymousBlock(box_) => style_is_in_normal_flow(&box_.style),
        _ => child
            .element_parts()
            .is_some_and(|(child_element, _, child_style, _)| {
                is_normal_block_flow_child(child_element, child_style)
                    || is_document_canvas_element(parent)
                    || is_replaced_element(child_element)
            }),
    }
}

pub(super) fn collapse_margins(first: f32, second: f32) -> f32 {
    if first >= 0.0 && second >= 0.0 {
        first.max(second)
    } else if first <= 0.0 && second <= 0.0 {
        first.min(second)
    } else {
        first + second
    }
}

/// Collapses an adjoining set of vertical margins.
///
/// CSS 2.2 collapses an adjoining margin set to the maximum positive margin
/// plus the minimum negative margin. This differs from pairwise collapsing
/// when a set contains more than two mixed-sign margins.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
pub(super) fn collapse_margin_set(margins: impl IntoIterator<Item = f32>) -> f32 {
    let mut max_positive = 0.0f32;
    let mut min_negative = 0.0f32;
    for margin in margins {
        if margin > max_positive {
            max_positive = margin;
        }
        if margin < min_negative {
            min_negative = margin;
        }
    }
    max_positive + min_negative
}

pub(super) fn page_start_margin(margin: f32, starts_at_page_top: bool) -> f32 {
    if starts_at_page_top && margin > 0.0 {
        0.0
    } else {
        margin
    }
}

pub(super) fn collapsed_start_margin_delta(
    previous_applied: f32,
    next: f32,
    starts_at_page_top: bool,
) -> f32 {
    let collapsed = collapse_margins(previous_applied, next);
    page_start_margin(collapsed, starts_at_page_top) - previous_applied
}

pub(super) fn collapsed_margin_delta(previous_applied: f32, next: f32) -> f32 {
    collapse_margins(previous_applied, next) - previous_applied
}

/// Applies CSS Box Model Level 4 `margin-trim: block-start` to the first
/// in-flow child adjoining a block container's block-start edge.
///
/// The margin to trim is the collapsed adjoining margin set at the parent's
/// block-start edge, not just the child's authored `margin-top`. Cancelling
/// that collapsed contribution is observable when CSS 2.2 block-in-inline
/// splitting exposes a self-collapsing block as the first in-flow child.
///
/// <https://drafts.csswg.org/css-box-4/#margin-trim>
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(super) fn trim_adjoining_block_start_margin(
    parent_style: &ComputedStyle,
    child_style: &mut ComputedStyle,
    is_first_flow_child: bool,
    descendant_start_margin: Option<f32>,
) -> bool {
    if !parent_style.margin_trim.block_start || !is_first_flow_child {
        return false;
    }
    let adjoining_start_margin = descendant_start_margin
        .map(|descendant| collapse_margins(child_style.margin.top, descendant))
        .unwrap_or(child_style.margin.top);
    child_style.margin.top -= adjoining_start_margin;
    true
}

/// Returns whether a block's own top and bottom margins may adjoin.
///
/// CSS 2.2 allows a block's own margins to be adjoining when it has no border,
/// padding, line boxes, min-height, or in-flow content separating the edges,
/// and its height is either `auto` or zero.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(super) fn can_collapse_own_block_margins(
    style: &ComputedStyle,
    border_widths: css::Edges,
    has_direct_inline_content: bool,
) -> bool {
    style.display.is_flow()
        && !has_direct_inline_content
        && style.padding.top == 0.0
        && style.padding.bottom == 0.0
        && border_widths.top == 0.0
        && border_widths.bottom == 0.0
        && height_is_auto_or_zero(style)
        && style.box_values.min_height.is_auto()
}

fn height_is_auto_or_zero(style: &ComputedStyle) -> bool {
    style.box_values.height.is_auto()
        || style
            .box_values
            .height
            .length_if_no_percent()
            .is_some_and(|height| height == 0.0)
}

/// Returns whether a block's own start and end margins are adjoining.
///
/// CSS 2.2 defines the margins of a box with no border, padding, min-height,
/// fixed height, line box, or in-flow non-self-collapsing children as
/// adjoining. Such a child keeps its parent's block-start collapsed margin set
/// open, which matters for `margin-trim: block-start`.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://drafts.csswg.org/css-box-4/#margin-trim>
pub(super) fn is_self_collapsing_block_box(
    element: &Element,
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    let has_direct_inline_content = has_direct_inline_content_box(child_boxes);
    is_collapsible_block_child(element, style)
        && can_collapse_own_block_margins(
            style,
            used_border_widths(style),
            has_direct_inline_content,
        )
        && child_boxes
            .iter()
            .all(formatting_box_keeps_self_collapsing_parent)
}

pub(super) fn self_collapsing_block_margin_set_for_box(
    style: &ComputedStyle,
    descendant_start_margin: Option<f32>,
) -> f32 {
    let margins = [
        Some(style.margin.top),
        descendant_start_margin,
        Some(style.margin.bottom),
    ];
    collapse_margin_set(margins.into_iter().flatten())
}

fn formatting_box_keeps_self_collapsing_parent(box_: &box_tree::FormattingBox<'_>) -> bool {
    if box_tree::is_out_of_flow_box(box_) {
        return true;
    }
    if let box_tree::FormattingBox::AnonymousBlock(box_) = box_ {
        return box_
            .children
            .iter()
            .all(formatting_box_keeps_self_collapsing_parent);
    }
    let Some((element, _, style, children)) = box_.element_parts() else {
        return false;
    };
    is_normal_block_flow_child(element, style)
        && is_self_collapsing_block_box(element, style, children)
}

pub(super) fn collapsible_first_child_start_margin_from_boxes(
    child_boxes: &[box_tree::FormattingBox<'_>],
    parent: &Element,
    parent_style: &ComputedStyle,
) -> Option<f32> {
    for child_box in child_boxes {
        let Some((child_element, _, child_style, child_children)) = child_box.element_parts()
        else {
            if matches!(
                child_box,
                box_tree::FormattingBox::Inline(_)
                    | box_tree::FormattingBox::Line(_)
                    | box_tree::FormattingBox::Text(_)
            ) {
                return None;
            }
            continue;
        };
        let is_flow_child = is_normal_block_flow_child(child_element, child_style)
            || is_document_canvas_element(parent)
            || is_replaced_element(child_element);
        if !is_flow_child {
            if !inline_text(child_element).is_empty() {
                return None;
            }
            continue;
        }
        if parent_style.margin_trim.block_start {
            return Some(0.0);
        }
        return Some(collapsible_start_margin_for_box(
            child_element,
            child_style,
            child_children,
        ));
    }
    None
}

pub(super) fn collapsible_start_margin_for_box(
    element: &Element,
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> f32 {
    if can_collapse_block_start_margin(
        style,
        used_border_widths(style),
        has_direct_inline_content_box(child_boxes),
    ) && let Some(descendant_margin) =
        collapsible_first_child_start_margin_from_boxes(child_boxes, element, style)
    {
        if is_self_collapsing_block_box(element, style, child_boxes) {
            return self_collapsing_block_margin_set_for_box(style, Some(descendant_margin));
        }
        collapse_margins(style.margin.top, descendant_margin)
    } else if is_self_collapsing_block_box(element, style, child_boxes) {
        self_collapsing_block_margin_set_for_box(style, None)
    } else {
        style.margin.top
    }
}

pub(super) fn has_direct_inline_content_box(child_boxes: &[box_tree::FormattingBox<'_>]) -> bool {
    child_boxes.iter().any(|child| {
        matches!(
            child,
            box_tree::FormattingBox::Inline(_)
                | box_tree::FormattingBox::Line(_)
                | box_tree::FormattingBox::Text(_)
        )
    })
}

pub(super) fn has_non_inline_formatting_box(child_boxes: &[box_tree::FormattingBox<'_>]) -> bool {
    child_boxes.iter().any(|child| {
        if box_tree::is_out_of_flow_box(child) {
            return false;
        }
        matches!(
            child,
            box_tree::FormattingBox::AnonymousBlock(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::Table(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_)
        )
    })
}

pub(super) fn has_atomic_inline_formatting_box(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| match child {
        box_tree::FormattingBox::AtomicInline(_) => true,
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            has_atomic_inline_formatting_box(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            has_atomic_inline_formatting_box(&box_.children)
        }
        box_tree::FormattingBox::Inline(box_) => has_atomic_inline_formatting_box(&box_.children),
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Line(_)
        | box_tree::FormattingBox::Text(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    })
}

pub(super) fn collapsible_first_child_start_margin_dom_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> Option<f32> {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    collapsible_first_child_start_margin_dom_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
    )
}

fn collapsible_first_child_start_margin_dom_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> Option<f32> {
    let sibling_tags = element_sibling_tags(element);
    let mut element_index = 0usize;
    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            if let NodeKind::Text(text) = &child.kind
                && !collapse_whitespace(text).is_empty()
            {
                return None;
            }
            continue;
        };
        let signature = ElementSignature::with_siblings(
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
        let is_flow_child = is_normal_block_flow_child(child_element, &child_style)
            || is_document_canvas_element(element)
            || is_replaced_element(child_element);
        if !is_flow_child {
            if !inline_text(child_element).is_empty() {
                return None;
            }
            continue;
        }
        if parent_style.margin_trim.block_start {
            return Some(0.0);
        }
        let mut child_ancestors = ancestors.to_vec();
        child_ancestors.push(signature);
        return Some(collapsible_start_margin_dom_with_resolver(
            child_element,
            &child_style,
            stylesheets,
            &child_ancestors,
            resolver,
        ));
    }
    None
}

fn collapsible_start_margin_dom_with_resolver(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> f32 {
    if can_collapse_block_start_margin(
        style,
        used_border_widths(style),
        has_direct_inline_content_dom_with_resolver(
            element,
            style,
            stylesheets,
            ancestors,
            resolver,
        ),
    ) && let Some(descendant_margin) = collapsible_first_child_start_margin_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
    ) {
        if is_self_collapsing_block_dom_with_resolver(
            element,
            style,
            stylesheets,
            ancestors,
            resolver,
        ) {
            return self_collapsing_block_margin_set_for_box(style, Some(descendant_margin));
        }
        collapse_margins(style.margin.top, descendant_margin)
    } else if is_self_collapsing_block_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
    ) {
        self_collapsing_block_margin_set_for_box(style, None)
    } else {
        style.margin.top
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
pub(super) fn is_self_collapsing_block_dom_with_font_metrics(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    is_self_collapsing_block_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        &mut resolver,
    )
}

fn is_self_collapsing_block_dom_with_resolver(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let has_direct_inline_content = has_direct_inline_content_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
    );
    is_collapsible_block_child(element, style)
        && can_collapse_own_block_margins(
            style,
            used_border_widths(style),
            has_direct_inline_content,
        )
        && dom_children_keep_self_collapsing_parent(
            element,
            style,
            stylesheets,
            ancestors,
            resolver,
        )
}

fn dom_children_keep_self_collapsing_parent(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_tags(element);
    let mut element_index = 0usize;
    element.children.iter().all(|child| match &child.kind {
        NodeKind::Text(text) => collapse_whitespace(text).is_empty(),
        NodeKind::Element(child) => {
            let signature = ElementSignature::with_siblings(
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
            is_normal_block_flow_child(child, &child_style)
                && is_self_collapsing_block_dom_with_resolver(
                    child,
                    &child_style,
                    stylesheets,
                    &child_ancestors,
                    resolver,
                )
        }
    })
}

pub(super) fn has_direct_inline_content_dom_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_direct_inline_content_dom_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
    )
}

fn has_direct_inline_content_dom_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_tags(element);
    let mut element_index = 0usize;
    element.children.iter().any(|child| match &child.kind {
        NodeKind::Text(text) => !collapse_whitespace(text).is_empty(),
        NodeKind::Element(child) => {
            let signature = ElementSignature::with_siblings(
                child.tag.clone(),
                child.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = resolver.style_for_element(
                child,
                signature,
                stylesheets,
                Some(parent_style),
                ancestors,
            );
            !is_normal_block_flow_child(child, &child_style)
                && !is_replaced_element(child)
                && !inline_text(child).is_empty()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_parent_style() -> ComputedStyle {
        ComputedStyle {
            font_size: 12.0,
            line_height: 14.4,
            color: Color::BLACK,
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

    #[tokio::test]
    async fn atomic_inline_page_values_include_descendant_start_and_end_values() {
        let root = dom::parse(
            "<html><body>\
             <div style=\"page:c; display:inline-block\">\
               <div style=\"page:a\">A</div>\
               <div style=\"page:b\">B</div>\
             </div>\
             <div style=\"page:c\">C</div>\
             </body></html>",
        );
        let stylesheets = vec![css::html5_user_agent_stylesheet()];
        let page = box_tree::build_page_box(&root, &stylesheets, &test_parent_style());
        let body = &page.children[0].children()[0];
        let anonymous_inline_run = &body.children()[0];

        assert_eq!(
            formatting_box_page_values(anonymous_inline_run),
            (Some("a".to_string()), Some("b".to_string()))
        );
        assert_eq!(
            formatting_box_page_values(&body.children()[1]),
            (Some("c".to_string()), Some("c".to_string()))
        );
    }

    #[tokio::test]
    async fn definition_list_dom_grouping_uses_measured_parent_ch_for_child_font_size() {
        let root =
            dom::parse("<html><body><dl><dt style=\"font-size: 2ch\">Term</dt></dl></body></html>");
        let dl = first_element_by_tag(&root, "dl").expect("expected dl element");
        let stylesheet = css::parse_stylesheet(
            &css::Css::from_string(
                r#"@font-face {
                    font-family: MetricProbe;
                    src: url("tests/resources/fonts/noto-sans-v8-latin-regular.woff");
                }"#,
            )
            .with_base_url(Some(PathBuf::from("."))),
        );
        let stylesheets = vec![css::html5_user_agent_stylesheet(), stylesheet];
        let mut parent_style = ComputedStyle {
            font_family: css::FontFamily::Names(vec!["MetricProbe".to_string()]),
            font_size: 40.0,
            line_height: 40.0,
            ..ComputedStyle::initial()
        };
        parent_style.line_height_value = css::ComputedLineHeight::from_length(40.0);
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;
        let parent_ch_advance = font_system.ch_advance(&parent_style);
        assert!(
            (parent_ch_advance - parent_style.font_size * 0.5).abs() > 0.01,
            "fixture must differ from the generic 0.5em ch fallback"
        );

        let groups = definition_list_column_groups_with_font_metrics(
            dl,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        );

        assert_eq!(groups.len(), 1);
        assert!((groups[0][0].style.font_size - parent_ch_advance * 2.0).abs() < 0.01);
    }
}
