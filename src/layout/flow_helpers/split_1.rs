use super::*;

pub(in crate::layout) fn is_default_block_container_tag(tag: &str) -> bool {
    // CSS Display defines block containers by computed display, while HTML only
    // supplies the default display values through the UA stylesheet. The
    // cascade-derived result is cached per tag because inline-text collection
    // performs this classification for every nested element.
    // https://www.w3.org/TR/css-display-3/#block-container
    css::default_display_is_block_level_for_tag(tag)
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
    ElementSignature::from_selector_snapshot(element_selector_signature(element))
}

pub(in crate::layout) fn element_selector_signature(element: &Element) -> ElementSiblingSignature {
    element_selector_signature_with_link_context(element, None, None)
}

fn element_selector_signature_with_link_context(
    element: &Element,
    document_url: Option<&url::Url>,
    base_url: Option<&url::Url>,
) -> ElementSiblingSignature {
    if let Some(signature) = element.selector_snapshot.get() {
        return signature.clone();
    }
    let mut has_text_child = false;
    for child in &element.children {
        if let NodeKind::Text(text) = &child.kind {
            has_text_child |= !text.is_empty();
        }
    }
    let children = ElementSiblingSignatureList::from_vec(
        element
            .children
            .iter()
            .filter_map(|child| match &child.kind {
                NodeKind::Element(child) => Some(element_selector_signature_with_link_context(
                    child,
                    document_url,
                    base_url,
                )),
                NodeKind::Text(_) => None,
            })
            .collect(),
    );
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
        .with_source_element_id(element.id)
        .with_namespace(element.namespace_url.clone(), namespace_attrs)
        .with_document_is_html(element.document_syntax == dom::DocumentSyntax::Html)
        .with_document_compatibility_mode(element.document_compatibility_mode)
        .with_link_state(element_link_state(element, document_url, base_url))
        .with_child_list(children, has_text_child);
    if let Some(direction) = element_document_direction(element) {
        signature = signature.with_document_direction(direction);
    }
    // Snapshot construction happens after target selection.
    let snapshot = signature.with_target(element.is_target);
    let _ = element.selector_snapshot.set(snapshot.clone());
    snapshot
}

/// Eagerly materialize the immutable selector tree for one prepared DOM.
///
/// Layout replays may build the formatting tree repeatedly, so doing this at
/// the pipeline boundary keeps selector snapshots stable and shared across
/// every pass.
pub(in crate::layout) fn prime_selector_snapshots(
    node: &Node,
    document_url: Option<&url::Url>,
    base_url: Option<&url::Url>,
) {
    let NodeKind::Element(element) = &node.kind else {
        return;
    };
    let _ = element_selector_signature_with_link_context(element, document_url, base_url);
}

fn element_link_state(
    element: &Element,
    document_url: Option<&url::Url>,
    base_url: Option<&url::Url>,
) -> css::LinkState {
    let is_link =
        matches!(element.tag.as_str(), "a" | "area" | "link") && element.attrs.contains_key("href");
    let Some(destination) = is_link
        .then(|| element.attrs.get("href"))
        .flatten()
        .and_then(|href| crate::resource::resolve_url(href, base_url, None))
    else {
        return css::LinkState::Unvisited;
    };
    let Some(document_url) = document_url.cloned() else {
        return css::LinkState::Unvisited;
    };
    let mut destination = destination;
    let mut document_url = document_url;
    destination.set_fragment(None);
    document_url.set_fragment(None);
    if destination == document_url {
        css::LinkState::Visited
    } else {
        css::LinkState::Unvisited
    }
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
        stylesheets: &Stylesheets<'_>,
        parent: Option<&ComputedStyle>,
        ancestors: &[ElementSignature],
    ) -> ComputedStyle {
        let mut style = self.principal_style_for_element(
            element,
            signature.clone(),
            stylesheets,
            parent,
            ancestors,
        );
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

    /// Resolve only an element's principal computed style for structural DOM
    /// classification.
    ///
    /// The result deliberately omits every pseudo-element style. Callers may
    /// inspect principal-box properties such as `display`, `float`, and
    /// `position`, but must not use it to inspect generated content or retain
    /// it for formatting-tree construction.
    pub(in crate::layout) fn structural_style_for_element(
        &mut self,
        element: &Element,
        signature: ElementSignature,
        stylesheets: &Stylesheets<'_>,
        parent: Option<&ComputedStyle>,
        ancestors: &[ElementSignature],
    ) -> ComputedStyle {
        self.principal_style_for_element(element, signature, stylesheets, parent, ancestors)
    }

    fn principal_style_for_element(
        &mut self,
        element: &Element,
        signature: ElementSignature,
        stylesheets: &Stylesheets<'_>,
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
                signature,
                stylesheets,
                parent,
                ancestors,
                parent_ch_advance,
            );
        }
        style
    }
}

pub(in crate::layout) fn definition_list_column_groups_with_font_metrics<'a>(
    element: &'a Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
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
    stylesheets: &Stylesheets<'_>,
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
        let signature =
            ElementSignature::from_sibling_snapshot(element_index, sibling_tags.clone())
                .expect("source child must have a cached sibling signature");
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
        if !style.page.is_specified() {
            return Self::Inherited;
        }
        style
            .page
            .specified_name()
            .map(|name| name.as_str().to_string())
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
    pub(in crate::layout) fn uniform(page_name: Option<String>) -> Self {
        Self {
            start: page_name.clone(),
            end: page_name,
        }
    }

    pub(in crate::layout) fn inapplicable() -> Self {
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
    if !style.page.is_specified() && element_has_leading_direct_inline_content(element, style) {
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
        // `page` does not apply to inline boxes. Keep their normal-flow
        // descendants, which may themselves establish class-A boundaries,
        // but discard the inline box's authored value.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        box_tree::FormattingBox::Inline(box_) => {
            let mut descendant_style = box_.core.style.as_ref().clone();
            descendant_style.page = css::PageAssignment::Unspecified;
            page_value_sources_from_element_style_and_children(
                box_.core.element,
                &descendant_style,
                &box_.core.children,
            )
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            let mut descendant_style = box_.core.style.as_ref().clone();
            descendant_style.page = css::PageAssignment::Unspecified;
            page_value_sources_from_element_style_and_children(
                box_.core.element,
                &descendant_style,
                &box_.core.children,
            )
        }
        // Atomic inline formatting contexts do not establish class-A page
        // boundaries in their parent flow. Their descendants remain atomic
        // with the inline box rather than propagating page groups outward.
        // <https://drafts.csswg.org/css-page-3/#using-named-pages>
        box_tree::FormattingBox::AtomicInline(_) => PageBoundaryValues::inapplicable(),
        box_tree::FormattingBox::Flex(box_) => PageBoundaryValues::from_style(&box_.core.style),
        // A table's durable fragment is the CSS table formatting tree. Its
        // generic core children still describe the pre-fixup element tree and
        // can therefore omit or mis-order the effective row boundaries.
        // Named-page propagation must follow the same row sequence table
        // layout fragments.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        box_tree::FormattingBox::Table(box_) => {
            table::table_page_boundary_summary(&box_.fragment, &box_.core.style, None).sources
        }
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
    style
        .page
        .effective_name(inherited_page_name.map(str::to_string))
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
            if !box_.core.style.page.is_specified()
                && element_has_leading_direct_inline_content(box_.core.element, &box_.core.style)
            {
                values.start = resolved_page_name_for_style(&box_.core.style, inherited_page_name);
            }
            values
        }
        box_tree::FormattingBox::Inline(box_) => {
            let mut style = box_.core.style.as_ref().clone();
            style.page = css::PageAssignment::Unspecified;
            let mut values = resolved_page_boundary_values_from_style_and_children(
                &style,
                &box_.core.children,
                inherited_page_name,
            );
            if element_has_leading_direct_inline_content(box_.core.element, &style) {
                values.start = resolved_page_name_for_style(&style, inherited_page_name);
            }
            values
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            let mut style = box_.core.style.as_ref().clone();
            style.page = css::PageAssignment::Unspecified;
            let mut values = resolved_page_boundary_values_from_style_and_children(
                &style,
                &box_.core.children,
                inherited_page_name,
            );
            if element_has_leading_direct_inline_content(box_.core.element, &style) {
                values.start = resolved_page_name_for_style(&style, inherited_page_name);
            }
            values
        }
        box_tree::FormattingBox::AtomicInline(_) => ResolvedPageBoundaryValues::inapplicable(),
        box_tree::FormattingBox::Flex(box_) => ResolvedPageBoundaryValues::uniform(
            resolved_page_name_for_style(&box_.core.style, inherited_page_name),
        ),
        box_tree::FormattingBox::Table(box_) => {
            table::table_page_boundary_summary(
                &box_.fragment,
                &box_.core.style,
                inherited_page_name,
            )
            .resolved
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
        && style.page.is_specified()
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
        && style.position.is_normal_flow()
        && style.float == Float::None
        && !style.position.is_running()
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
    _inside_inline_flow: bool,
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
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
                _inside_inline_flow
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
                // Floats need a source boundary in the parent block flow.
                // <https://www.w3.org/TR/css-position-3/#static-position>
                let is_flow_child = child_style.float == Float::Footnote
                    || (child_style.float != Float::None && child_style.display.is_block_level())
                    || is_normal_block_flow_child(child_element, &child_style)
                    // HTML table structure still needs source-order traversal
                    // around block siblings, but its computed outer display
                    // decides whether the table itself is dispatched as block
                    // flow or collected as an atomic inline.
                    // <https://drafts.csswg.org/css-display-3/#box-generation>
                    || (is_html_table_element(child_element)
                        && child_style.display.is_block_level())
                    || (is_replaced_element(child_element)
                        && child_style.display.is_block_level());
                if is_flow_child {
                    has_later_float_after_static_boundary |= has_block_static_boundary_after_flow
                        && matches!(
                            child_style.float,
                            Float::Left | Float::Right | Float::InlineStart | Float::InlineEnd
                        );
                    has_flow = true;
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
    element: &Element,
    style: &ComputedStyle,
    border_edges: UsedEdges,
    has_direct_inline_content: bool,
    used_overflow: css::Overflow,
) -> bool {
    style.display.is_flow()
        && !style.display.establishes_block_formatting_context()
        && !style_establishes_multicol_formatting_context(style)
        && !style_establishes_line_clamp_formatting_context(style)
        && !property_containment_establishes_independent_formatting_context(element, style)
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
    element: &Element,
    style: &ComputedStyle,
    containing_block_height_basis: BlockSizePercentageBasis,
    border_edges: UsedEdges,
    has_direct_inline_content: bool,
    used_overflow: css::Overflow,
) -> bool {
    style.display.is_flow()
        && !style.display.establishes_block_formatting_context()
        && !style_establishes_multicol_formatting_context(style)
        && !style_establishes_line_clamp_formatting_context(style)
        && !property_containment_establishes_independent_formatting_context(element, style)
        && style.float == Float::None
        && !has_direct_inline_content
        && used_overflow == css::Overflow::Visible
        && style.padding.bottom == 0.0
        && border_edges.bottom == layout_pt(0.0)
        && height_behaves_as_auto_for_margin_collapse(style, containing_block_height_basis)
}

/// Returns whether a preferred physical height behaves as `auto` for CSS 2
/// margin-collapse eligibility.
///
/// CSS Sizing updates legacy CSS 2 conditions on computed `height: auto` to
/// include values that behave as if `auto` were specified.  Keep this
/// classification at the margin-collapse used-value boundary: the computed
/// value remains necessary for ordinary sizing, paint, and fragmentation.
///
/// <https://drafts.csswg.org/css-sizing-3/#behave-auto>
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn height_behaves_as_auto_for_margin_collapse(
    style: &ComputedStyle,
    containing_block_height_basis: BlockSizePercentageBasis,
) -> bool {
    match &*style.box_values.height {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => true,
        css::ComputedLengthPercentageOrAuto::Stretch => {
            matches!(containing_block_height_basis, PercentageBasis::Indefinite)
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            matches!(containing_block_height_basis, PercentageBasis::Indefinite)
                && value.needs_percentage_basis()
        }
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => {
            matches!(&value.basis, css::CalcSizeBasis::Auto)
        }
    }
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
    matches!(style.column_count, css::ColumnCount::Count(_))
        || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
        || matches!(style.column_height, css::ComputedColumnHeight::Length(_))
}

/// `continue: collapse` and `continue: discard` establish an independent
/// formatting context when they create a line-clamp container. Multicol
/// containers are explicitly exempt: their `continue` behavior remains
/// `auto`.
/// <https://drafts.csswg.org/css-overflow-4/#continue>
pub(in crate::layout) fn style_establishes_line_clamp_formatting_context(
    style: &ComputedStyle,
) -> bool {
    !style_establishes_multicol_formatting_context(style)
        && matches!(
            style.used_continuation(),
            css::UsedContinuation::LineClamp(_) | css::UsedContinuation::Discard(_)
        )
}

/// Resolve the clamp state consumed by inline layout for this block container.
///
/// The computed declaration stays untouched; an ancestor traversal may supply
/// a smaller layout budget through `line_limit_traversal`. A multicol container is
/// deliberately ineligible because `continue: collapse` behaves as `auto`
/// there.
/// <https://drafts.csswg.org/css-overflow-4/#continue>
pub(in crate::layout) fn used_line_clamp_for_style(
    style: &ComputedStyle,
) -> Option<css::InlineLineClamp<'_>> {
    style
        .line_limit_traversal
        .as_ref()
        .map(css::InlineLineClamp::Used)
        .or_else(|| {
            (!style_establishes_multicol_formatting_context(style))
                .then(|| {
                    style
                        .line_clamp_container()
                        .map(css::InlineLineClamp::Computed)
                })
                .flatten()
        })
}

/// Returns whether a child participates in normal block flow for margin collapse.
///
/// CSS 2.2 defines floated boxes as out of normal flow, so they must not
/// contribute adjoining margins to their block container:
/// <https://www.w3.org/TR/CSS22/visuren.html#positioning-scheme> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
pub(in crate::layout) fn is_normal_block_flow_child(
    _element: &Element,
    style: &ComputedStyle,
) -> bool {
    style.position.is_normal_flow() && style.float == Float::None && style.display.is_block_level()
}

/// Returns whether a normal-flow block child's outer margins can adjoin its parent.
///
/// A flex container prevents its items' margins from collapsing through it,
/// but its own block-start and block-end margins remain eligible to collapse
/// with an adjoining block parent:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-containers> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
pub(in crate::layout) fn is_collapsible_block_child(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    is_normal_block_flow_child(element, style) && !is_replaced_element(element)
}

/// Returns whether a block's outer margins can adjoin an in-flow block sibling.
///
/// An independent formatting context prevents margins from collapsing through
/// its own edges with its children. It does not isolate the principal box's
/// outer margins from an adjacent sibling in the parent's block formatting
/// context. Consequently, block-level Grid, Flex, and flow-root containers
/// remain eligible here; their inner formatting contexts are handled only by
/// the parent/child margin-collapse predicates.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn outer_margins_adjoin_block_siblings(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    is_normal_block_flow_child(element, style) && !is_replaced_element(element)
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
    stylesheets: &Stylesheets<'_>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_break_policy_differences_require_inline_item_collection() {
        let mut parent = ComputedStyle::initial();
        let mut child = parent.clone();

        assert!(!inline_break_policy_differs(&parent, &child));
        assert!(!inline_style_affects_line(&parent, &child));

        child.hyphens = css::Hyphens::Auto;
        child.language = css::ContentLanguage::from_html_attribute("en");
        assert!(inline_break_policy_differs(&parent, &child));
        assert!(inline_style_affects_line(&parent, &child));

        parent = child.clone();
        child.hyphens = css::Hyphens::None;
        assert!(inline_break_policy_differs(&parent, &child));

        child = parent.clone();
        child.hyphenate_character = css::HyphenateCharacter::String("=".into());
        assert!(inline_break_policy_differs(&parent, &child));

        child = parent.clone();
        child.hyphenate_limit_chars = css::HyphenateLimitChars {
            total: 6,
            before: 3,
            after: 2,
        };
        assert!(inline_break_policy_differs(&parent, &child));

        child = parent.clone();
        child.word_break = css::WordBreak::BreakAll;
        assert!(inline_break_policy_differs(&parent, &child));

        child = parent.clone();
        child.overflow_wrap = css::OverflowWrap::Anywhere;
        assert!(inline_break_policy_differs(&parent, &child));

        child = parent.clone();
        child.line_break = css::LineBreak::Anywhere;
        assert!(inline_break_policy_differs(&parent, &child));

        child = parent.clone();
        child.text_wrap_mode = css::TextWrapMode::NoWrap;
        assert!(inline_break_policy_differs(&parent, &child));

        child = parent.clone();
        child.text_wrap_style = css::TextWrapStyle::Balance;
        assert!(inline_break_policy_differs(&parent, &child));
    }
}
