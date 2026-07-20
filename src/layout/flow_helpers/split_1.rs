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
            is_table_or_replaced_element(box_.core.element)
                || has_table_or_replaced_descendant_box(&box_.core.children)
        }
        box_tree::FormattingBox::Block(box_) => {
            has_table_or_replaced_descendant_box(&box_.core.children)
        }
        box_tree::FormattingBox::Inline(box_) => {
            has_table_or_replaced_descendant_box(&box_.core.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            has_table_or_replaced_descendant_box(&box_.core.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            has_table_or_replaced_descendant_box(&box_.children)
        }
        box_tree::FormattingBox::Flex(box_) => {
            has_table_or_replaced_descendant_box(&box_.core.children)
        }
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

/// The page value associated with one side of a structural class-A boundary.
///
/// `Inherited` is deliberately distinct from `Auto`: a box with an omitted
/// `page` declaration still participates in the boundary, but its used value
/// has to be resolved from the active lexical page-value scope. An explicit
/// `page: auto` resolves to the nearest non-auto ancestor while retaining its
/// structural identity. `Inapplicable` is used for a box that
/// cannot establish a page boundary at all (for example text before it is
/// wrapped in its anonymous block).
///
/// CSS Paged Media's page-group algorithm uses the used value for the
/// boundary comparison while preserving this lexical distinction:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::layout) enum PageBoundaryValue {
    Inapplicable,
    Inherited,
    Auto,
    Named(String),
}

impl PageBoundaryValue {
    pub(in crate::layout) fn from_style(style: &ComputedStyle) -> Self {
        if !style.page_name_specified {
            return Self::Inherited;
        }
        style
            .page_name
            .clone()
            .map(Self::Named)
            .unwrap_or(Self::Auto)
    }

    /// Whether this child value replaces its parent's structural start/end
    /// summary. An inherited child has no authored value to propagate.
    pub(in crate::layout) fn overrides_parent_summary(&self) -> bool {
        matches!(self, Self::Auto | Self::Named(_))
    }
}

/// First and last page-boundary values propagated from one formatting box.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::layout) struct PageBoundaryValues {
    pub(in crate::layout) start: PageBoundaryValue,
    pub(in crate::layout) end: PageBoundaryValue,
}

/// The used page types at the two propagated sides of a formatting box.
///
/// Unlike [`PageBoundaryValues`], this record contains no authored-value
/// placeholders. `auto` has already been resolved against the nearest
/// non-auto ancestor in the *formatting tree*, before layout starts moving the
/// page cursor. This is the value class-A break selection compares.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::layout) struct ResolvedPageBoundaryValues {
    pub(in crate::layout) start: Option<String>,
    pub(in crate::layout) end: Option<String>,
}

impl ResolvedPageBoundaryValues {
    fn uniform(page_name: Option<String>) -> Self {
        Self {
            start: page_name.clone(),
            end: page_name,
        }
    }

    fn inapplicable() -> Self {
        Self::uniform(None)
    }
}

impl PageBoundaryValues {
    pub(in crate::layout) fn inapplicable() -> Self {
        Self {
            start: PageBoundaryValue::Inapplicable,
            end: PageBoundaryValue::Inapplicable,
        }
    }

    pub(in crate::layout) fn from_style(style: &ComputedStyle) -> Self {
        let own = PageBoundaryValue::from_style(style);
        Self {
            start: own.clone(),
            end: own,
        }
    }
}

/// Returns first/last CSS `page` boundary values.
///
/// CSS Paged Media's `auto` page value can explicitly end an ancestor named
/// page group, while an omitted `page` declaration inherits the surrounding
/// page group at a class-A boundary. Layout therefore has to preserve the
/// distinction instead of flattening both to `None`:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn page_value_sources_from_style_and_children(
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> PageBoundaryValues {
    let PageBoundaryValues { mut start, mut end } = PageBoundaryValues::from_style(style);
    if style.display.is_flex() {
        return PageBoundaryValues { start, end };
    }
    // Ignore transparent wrappers rooted in boxes to which `page` cannot
    // apply (such as an absolutely positioned descendant), but retain an
    // inline atomic box as an inapplicable first/last participant. The latter
    // makes its parent fall back to its own used value rather than allowing a
    // later named sibling to select the document's first page.
    // <https://drafts.csswg.org/css-page-3/#using-named-pages>
    let mut normal_flow_children = child_boxes
        .iter()
        .filter(|child| formatting_box_is_page_value_participant(child));
    let Some(first) = normal_flow_children.next() else {
        return PageBoundaryValues { start, end };
    };
    let first_sources = formatting_box_page_value_sources(first);
    if first_sources.start.overrides_parent_summary() {
        start = first_sources.start.clone();
    }
    // A single normal-flow child supplies both boundaries. Compute its paired
    // summary once: recursively querying it separately for start and end
    // would revisit the same sole-child chain twice at every depth.
    let last_sources = normal_flow_children
        .next_back()
        .map(formatting_box_page_value_sources)
        .unwrap_or(first_sources);
    if last_sources.end.overrides_parent_summary() {
        end = last_sources.end;
    }
    PageBoundaryValues { start, end }
}

/// Returns page-value sources for an element's box, retaining leading direct
/// inline content as the element's first page-group participant.
///
/// Formatting-tree normalization stores direct text separately from the
/// element's block-level child boxes. A later descendant with `page: <name>`
/// must not make the document root select that named page before direct text
/// which precedes it in tree order. That text belongs to the initial `auto`
/// page group and establishes the class-A boundary before the named child.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
pub(in crate::layout) fn page_value_sources_from_element_style_and_children(
    element: &Element,
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> PageBoundaryValues {
    let mut sources = page_value_sources_from_style_and_children(style, child_boxes);
    if !style.page_name_specified && element_has_leading_direct_inline_content(element, style) {
        sources.start = PageBoundaryValue::Inherited;
    }
    sources
}

/// Whether direct inline content precedes the first element child.
///
/// Inline element children remain represented in the formatting tree and are
/// therefore covered by `page_value_sources_from_style_and_children`. This
/// helper accounts only for direct text and `<br>` nodes, which normalization
/// otherwise keeps outside that child-box sequence.
fn element_has_leading_direct_inline_content(element: &Element, style: &ComputedStyle) -> bool {
    if element_suppresses_direct_text_children(element) {
        return false;
    }
    let mut text = String::new();
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(value) => text.push_str(value),
            NodeKind::Element(child) if is_line_break_element(child) => text.push(INLINE_BREAK),
            NodeKind::Element(_) => break,
        }
    }
    !normalized_text_for_style(&text, style).is_empty()
}

pub(in crate::layout) fn formatting_box_page_value_sources(
    box_: &box_tree::FormattingBox<'_>,
) -> PageBoundaryValues {
    match box_ {
        box_tree::FormattingBox::Block(box_) => page_value_sources_from_element_style_and_children(
            box_.core.element,
            &box_.core.style,
            &box_.core.children,
        ),
        box_tree::FormattingBox::Inline(box_) => {
            page_value_sources_from_element_style_and_children(
                box_.core.element,
                &box_.core.style,
                &box_.core.children,
            )
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            page_value_sources_from_element_style_and_children(
                box_.core.element,
                &box_.core.style,
                &box_.core.children,
            )
        }
        // `page` does not apply to an atomic inline box itself, but a nested
        // normal-flow descendant can still carry the first/last class-A
        // boundary values for the atomic formatting context. Suppress only
        // the atomic box's own value; discarding its children loses named
        // page transitions inside inline blocks and tables.
        // In particular, an inline `<canvas style="page:b">` must not
        // select the document's first page, while an inline-block containing
        // `page:a` then `page:b` descendants propagates `(a, b)`.
        // <https://drafts.csswg.org/css-page-3/#using-named-pages>
        box_tree::FormattingBox::AtomicInline(box_) => {
            let mut descendant_style = box_.core.style.as_ref().clone();
            descendant_style.page_name_specified = false;
            descendant_style.page_name = None;
            page_value_sources_from_style_and_children(&descendant_style, &box_.core.children)
        }
        box_tree::FormattingBox::Flex(box_) => PageBoundaryValues::from_style(&box_.core.style),
        box_tree::FormattingBox::Table(box_) => page_value_sources_from_element_style_and_children(
            box_.core.element,
            &box_.core.style,
            &box_.core.children,
        ),
        box_tree::FormattingBox::Replaced(box_) => PageBoundaryValues::from_style(&box_.core.style),
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            page_value_sources_from_style_and_children(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::Text(_) => PageBoundaryValues::inapplicable(),
    }
}

/// Resolves the used `page` value for one style without consulting the output
/// page cursor. CSS Paged Media resolves `auto` to the nearest ancestor whose
/// used page value is not `auto`; this lexical operation must happen before
/// start/end values are propagated through a formatting tree.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
fn resolved_page_name_for_style(
    style: &ComputedStyle,
    inherited_page_name: Option<&str>,
) -> Option<String> {
    if style.page_name_specified {
        style
            .page_name
            .clone()
            .or_else(|| inherited_page_name.map(str::to_string))
    } else {
        inherited_page_name.map(str::to_string)
    }
}

/// Resolves start/end page types for a style and its formatting-tree children.
///
/// The structural source helper retains whether a descendant's `auto` or
/// named value replaces its parent summary. This helper supplies the missing
/// lexical half: every recursive call receives the parent's already-used page
/// type, so a deep `page:auto` cannot accidentally bind to the mutable page
/// selected by an unrelated preceding sibling.
pub(in crate::layout) fn resolved_page_boundary_values_from_style_and_children(
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
    inherited_page_name: Option<&str>,
) -> ResolvedPageBoundaryValues {
    let own_page_name = resolved_page_name_for_style(style, inherited_page_name);
    let mut values = ResolvedPageBoundaryValues::uniform(own_page_name.clone());
    if style.display.is_flex() {
        return values;
    }
    let mut normal_flow_children = child_boxes
        .iter()
        .filter(|child| formatting_box_is_page_value_participant(child));
    let Some(first) = normal_flow_children.next() else {
        return values;
    };
    let first_sources = formatting_box_page_value_sources(first);
    let first_values =
        resolved_formatting_box_page_boundary_values(first, own_page_name.as_deref());
    if first_sources.start.overrides_parent_summary() {
        values.start = first_values.start;
    }
    let last = normal_flow_children.next_back().unwrap_or(first);
    let last_sources = formatting_box_page_value_sources(last);
    if last_sources.end.overrides_parent_summary() {
        values.end =
            resolved_formatting_box_page_boundary_values(last, own_page_name.as_deref()).end;
    }
    values
}

/// Resolves propagated class-A start/end page types for one formatting box.
///
/// This deliberately follows formatting boxes rather than DOM children so
/// display-none, floated, positioned, and otherwise non-participating boxes
/// never take part in named-page propagation.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
pub(in crate::layout) fn resolved_formatting_box_page_boundary_values(
    box_: &box_tree::FormattingBox<'_>,
    inherited_page_name: Option<&str>,
) -> ResolvedPageBoundaryValues {
    match box_ {
        box_tree::FormattingBox::Block(box_) => {
            let mut values = resolved_page_boundary_values_from_style_and_children(
                &box_.core.style,
                &box_.core.children,
                inherited_page_name,
            );
            if !box_.core.style.page_name_specified
                && element_has_leading_direct_inline_content(box_.core.element, &box_.core.style)
            {
                values.start = resolved_page_name_for_style(&box_.core.style, inherited_page_name);
            }
            values
        }
        box_tree::FormattingBox::Inline(box_) => {
            let mut values = resolved_page_boundary_values_from_style_and_children(
                &box_.core.style,
                &box_.core.children,
                inherited_page_name,
            );
            if !box_.core.style.page_name_specified
                && element_has_leading_direct_inline_content(box_.core.element, &box_.core.style)
            {
                values.start = resolved_page_name_for_style(&box_.core.style, inherited_page_name);
            }
            values
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            let mut values = resolved_page_boundary_values_from_style_and_children(
                &box_.core.style,
                &box_.core.children,
                inherited_page_name,
            );
            if !box_.core.style.page_name_specified
                && element_has_leading_direct_inline_content(box_.core.element, &box_.core.style)
            {
                values.start = resolved_page_name_for_style(&box_.core.style, inherited_page_name);
            }
            values
        }
        // Atomic inline boxes do not supply their own `page` value. Their
        // descendants nevertheless retain the surrounding lexical scope.
        box_tree::FormattingBox::AtomicInline(box_) => {
            let mut style = box_.core.style.as_ref().clone();
            style.page_name_specified = false;
            style.page_name = None;
            resolved_page_boundary_values_from_style_and_children(
                &style,
                &box_.core.children,
                inherited_page_name,
            )
        }
        box_tree::FormattingBox::Flex(box_) => ResolvedPageBoundaryValues::uniform(
            resolved_page_name_for_style(&box_.core.style, inherited_page_name),
        ),
        box_tree::FormattingBox::Table(box_) => {
            let mut values = resolved_page_boundary_values_from_style_and_children(
                &box_.core.style,
                &box_.core.children,
                inherited_page_name,
            );
            if !box_.core.style.page_name_specified
                && element_has_leading_direct_inline_content(box_.core.element, &box_.core.style)
            {
                values.start = resolved_page_name_for_style(&box_.core.style, inherited_page_name);
            }
            values
        }
        box_tree::FormattingBox::Replaced(box_) => ResolvedPageBoundaryValues::uniform(
            resolved_page_name_for_style(&box_.core.style, inherited_page_name),
        ),
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            resolved_page_boundary_values_from_style_and_children(
                &box_.style,
                &box_.children,
                inherited_page_name,
            )
        }
        box_tree::FormattingBox::Text(_) => ResolvedPageBoundaryValues::inapplicable(),
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
    // Anonymous inline runs participate in the class-A boundary surrounding
    // their containing block even though a text formatting box is not a
    // normal-flow *box* for the generic flow helper.  Otherwise a named
    // block followed by direct text has no succeeding page value, and the
    // return to the parent's used page type is never forced.
    // <https://www.w3.org/TR/css-page-3/#using-named-pages>
    if let box_tree::FormattingBox::Text(box_) = box_ {
        return !(box_.text.is_empty()
            || (box_.style.white_space.collapses_spaces()
                && box_.text.chars().all(is_css_collapsible_whitespace)));
    }
    if !formatting_box_is_in_normal_flow(box_) {
        return false;
    }
    match box_ {
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .any(formatting_box_is_page_value_participant),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .core
            .children
            .iter()
            .any(formatting_box_is_page_value_participant),
        _ => !formatting_box_can_only_create_phantom_line_boxes(box_),
    }
}

/// Returns the first and last effective CSS `page` values for one formatting box.
///
/// Absolutely positioned, fixed-position, floated, running, and display-none
/// boxes are not in normal flow and therefore do not create class A sibling
/// page-name boundaries:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
#[cfg(test)]
pub(in crate::layout) fn formatting_box_page_values(
    box_: &box_tree::FormattingBox<'_>,
) -> (Option<String>, Option<String>) {
    let sources = formatting_box_page_value_sources(box_);
    let name = |source: PageBoundaryValue| match source {
        PageBoundaryValue::Named(name) => Some(name),
        PageBoundaryValue::Inapplicable
        | PageBoundaryValue::Inherited
        | PageBoundaryValue::Auto => None,
    };
    (name(sources.start), name(sources.end))
}

pub(in crate::layout) fn formatting_box_is_in_normal_flow(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    !matches!(box_, box_tree::FormattingBox::Text(_)) && style_is_in_normal_flow(box_.style())
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
    inherited_page_name: Option<&str>,
) -> Option<String> {
    child_boxes
        .iter()
        .skip(current_index + 1)
        .filter(|child| formatting_box_is_in_normal_flow(child))
        .find(|child| !formatting_box_is_zero_height_page_boundary(child))
        .map(|child| resolved_formatting_box_page_boundary_values(child, inherited_page_name).start)
        .unwrap_or_else(|| {
            resolved_formatting_box_page_boundary_values(
                &child_boxes[current_index],
                inherited_page_name,
            )
            .start
        })
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
            box_.core
                .style
                .before_style
                .as_deref()
                .is_some_and(|style| style.content.is_generated())
                || box_
                    .core
                    .style
                    .after_style
                    .as_deref()
                    .is_some_and(|style| style.content.is_generated())
                || box_.core.style.content.is_generated()
                || box_.core.style.float != Float::None
                || inline_box_fragment_has_owned_inline_edge(&box_.core.style, box_.fragment_edges)
                || formatting_box_has_inline_content(&box_.core.children)
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
        inline_start_side(style.writing_mode, style.used_direction())
    } else {
        inline_end_side(style.writing_mode, style.used_direction())
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
                collect_inline_text_from_formatting_boxes(&box_.core.children, output);
            }
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.core.children, output);
            }
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.children, output);
            }
            box_tree::FormattingBox::AtomicInline(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.core.children, output);
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
        // A suppressed descendant still changes the rendered text stream:
        // flattening the parent's DOM text would otherwise resurrect fallback
        // text from `display: none` / unboxed HTML elements. Route this
        // through the item collector, which observes the descendant's used
        // display value before adding its content.
        // <https://www.w3.org/TR/css-display-3/#box-generation>
        if child_style.display.is_none() {
            return true;
        }
        if child_style.display.is_block_level() {
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
        || child.font_palette != parent.font_palette
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
                // A float or out-of-flow positioned child does not contribute
                // a normal-flow block, but it still selects placement at its
                // source position amongst adjacent inline content. It
                // therefore needs the same ordered traversal boundary as a
                // flow child: collecting the entire inline run before DOM
                // traversal would place it after text that follows it in
                // source order (and can replay the float twice).
                // <https://www.w3.org/TR/css-position-3/#static-position>
                let is_flow_child = child_style.float != Float::None
                    || is_normal_block_flow_child(child_element, &child_style)
                    || is_replaced_element(child_element)
                    || matches!(child_style.position, Position::Absolute | Position::Fixed);
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
    border_edges: UsedEdges,
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
        && border_edges.top == layout_pt(0.0)
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
    border_edges: UsedEdges,
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
        && border_edges.bottom == layout_pt(0.0)
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

    /// Whether a relative-position translation has no observable visual
    /// effect. This keeps the decision at the relative-positioning boundary,
    /// rather than comparing generic coordinates at each paint caller.
    /// <https://drafts.csswg.org/css-position-3/#relative-positioning>
    pub(in crate::layout) fn is_zero(self) -> bool {
        self.x().abs() <= 0.01 && self.y().abs() <= 0.01
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
    RelativeOffset {
        vector: ContainerVector::new(
            left.unwrap_or_else(|| -right.unwrap_or(0.0)),
            bottom.unwrap_or_else(|| -top.unwrap_or(0.0)),
        ),
    }
}

/// Resolve a relative-position translation from explicit percentage bases.
///
/// Table tracks have a final used block size even where their parent table
/// part's `height` remains indefinite. Keeping the percentage basis explicit
/// prevents that used geometry from accidentally resolving a percentage inset:
/// <https://drafts.csswg.org/css-position-3/#relative-positioning> and
/// <https://drafts.csswg.org/css-sizing-3/#definite>.
pub(in crate::layout) fn relative_position_offset_with_bases(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<ContentBoxLength>,
    block_basis: PercentageBasis<ContentBoxLength>,
) -> RelativeOffset {
    if !matches!(style.position, Position::Relative | Position::Sticky) {
        return RelativeOffset::zero();
    }
    let left = used_length_percentage_or_auto_with_basis(
        style.box_values.inset_left.clone(),
        inline_basis,
    )
    .map(|length| length.points());
    let right = used_length_percentage_or_auto_with_basis(
        style.box_values.inset_right.clone(),
        inline_basis,
    )
    .map(|length| length.points());
    let top =
        used_length_percentage_or_auto_with_basis(style.box_values.inset_top.clone(), block_basis)
            .map(|length| length.points());
    let bottom = used_length_percentage_or_auto_with_basis(
        style.box_values.inset_bottom.clone(),
        block_basis,
    )
    .map(|length| length.points());
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
