use super::*;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_snapshots_are_shared_after_dom_preparation() {
        let root = dom::parse(
            "<html><body><section id=\"hit\"><span lang=\"fr\">texte</span></section></body></html>",
        );
        prime_selector_snapshots(&root, None, None);
        let section = first_element_by_tag(&root, "section").expect("expected section");

        let first = element_selector_signature(section);
        let replay = element_selector_signature(section);

        assert!(first.shares_snapshot_with(&replay));
        assert_eq!(first.children.len(), 1);
        assert_eq!(first.children[0].attrs.get("lang"), Some(&"fr".to_string()));
    }

    #[test]
    fn selector_preparation_marks_only_document_local_links_visited() {
        let document_url = url::Url::parse("https://example.test/document.html#source")
            .expect("valid document URL");

        let self_link = dom::parse("<a href=\"#section\"></a>");
        prime_selector_snapshots(&self_link, Some(&document_url), Some(&document_url));
        assert_eq!(
            element_selector_signature(first_element_by_tag(&self_link, "a").unwrap()).link_state,
            css::LinkState::Visited
        );

        let external_link = dom::parse("<a href=\"other.html\"></a>");
        prime_selector_snapshots(&external_link, Some(&document_url), Some(&document_url));
        assert_eq!(
            element_selector_signature(first_element_by_tag(&external_link, "a").unwrap())
                .link_state,
            css::LinkState::Unvisited
        );

        let base_changed_link = dom::parse("<a href=\"\"></a>");
        let base_url = url::Url::parse("https://assets.example.test/").expect("valid base URL");
        prime_selector_snapshots(&base_changed_link, Some(&document_url), Some(&base_url));
        assert_eq!(
            element_selector_signature(first_element_by_tag(&base_changed_link, "a").unwrap())
                .link_state,
            css::LinkState::Unvisited
        );

        let string_document = dom::parse("<a href=\"\"></a>");
        prime_selector_snapshots(&string_document, None, Some(&document_url));
        assert_eq!(
            element_selector_signature(first_element_by_tag(&string_document, "a").unwrap())
                .link_state,
            css::LinkState::Unvisited
        );
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
    async fn structural_style_keeps_principal_cascade_and_omits_generated_pseudos() {
        let root = dom::parse("<span></span>");
        let span = first_element_by_tag(&root, "span").expect("expected span element");
        let stylesheet = css::parse_stylesheet(&css::Css::from_string(
            "span { display: block; font-size: 3ch } \
             span::before { content: var(--missing, 'generated') }",
        ));
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&stylesheet),
        );
        let parent_style = ComputedStyle {
            font_size: 20.0,
            line_height: 20.0,
            ..ComputedStyle::initial()
        };
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;
        let signature = element_signature(span);
        let mut resolver = DomStyleResolver::with_font_system(&mut font_system);

        let full = resolver.style_for_element(
            span,
            signature.clone(),
            &stylesheets,
            Some(&parent_style),
            &[],
        );
        let structural = resolver.structural_style_for_element(
            span,
            signature,
            &stylesheets,
            Some(&parent_style),
            &[],
        );

        assert_eq!(structural.display, full.display);
        assert_eq!(structural.font_size, full.font_size);
        assert!(full.before_style.is_some());
        assert!(structural.before_style.is_none());
        assert!(structural.after_style.is_none());
        assert!(structural.marker_style.is_none());
    }
}
