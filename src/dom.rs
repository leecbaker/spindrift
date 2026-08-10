use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use html5ever::parse_document as parse_html_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData as RcNodeData, RcDom};
use xml5ever::driver::parse_document as parse_xml_document;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentSyntax {
    Html,
    Xml,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeKind {
    Text(String),
    Element(Element),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Element {
    pub id: ElementId,
    pub tag: String,
    pub namespace_url: String,
    pub document_syntax: DocumentSyntax,
    pub attrs: HashMap<String, String>,
    pub namespace_attrs: Vec<NamespacedAttribute>,
    pub children: Vec<Node>,
    pub is_target: bool,
    /// Static rendering outcome selected for an HTML `<object>` element.
    ///
    /// The HTML resource-selection algorithm decides whether an object
    /// represents its external resource or its fallback subtree. This is
    /// resolved after optional visual resources have been preloaded and before
    /// CSS box construction, so all layout paths observe the same result.
    /// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-object-element>
    pub object_rendering: ObjectRendering,
}

/// The static renderer's selected representation for an HTML `<object>`.
///
/// A live browser can change this as a resource loads. Quire resolves the
/// available state once before paged layout, selecting fallback whenever it
/// cannot represent the resource as a supported static image.
/// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-object-element>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ObjectRendering {
    /// Render the object's child subtree through the ordinary CSS box model.
    #[default]
    Fallback,
    /// Render a successfully decoded raster or SVG resource as a replaced image.
    Image,
}

/// Stable identity for a source DOM element, preserved by layout clones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ElementId(u64);

impl ElementId {
    pub(crate) fn next() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamespacedAttribute {
    pub namespace_url: String,
    pub local_name: String,
    pub value: String,
}

impl Element {
    /// Return the null-namespace attribute addressed by an unprefixed CSS
    /// `attr()` name on this element.
    ///
    /// The HTML host language lowercases the requested name only for HTML
    /// elements in HTML documents; foreign-content and XML names remain
    /// case-sensitive.
    /// <https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-the-css-attr()-function>
    pub(crate) fn unprefixed_css_attr(&self, name: &str) -> Option<&str> {
        self.namespace_attrs
            .iter()
            .find(|attribute| {
                crate::css::unprefixed_attr_name_matches(
                    &self.namespace_url,
                    self.document_syntax == DocumentSyntax::Html,
                    &attribute.namespace_url,
                    &attribute.local_name,
                    name,
                )
            })
            .map(|attribute| attribute.value.as_str())
    }
}

impl Node {
    pub fn element(tag: impl Into<String>) -> Self {
        Self {
            kind: NodeKind::Element(Element {
                id: ElementId::next(),
                tag: tag.into(),
                namespace_url: String::new(),
                document_syntax: DocumentSyntax::Html,
                attrs: HashMap::new(),
                namespace_attrs: Vec::new(),
                children: Vec::new(),
                is_target: false,
                object_rendering: ObjectRendering::Fallback,
            }),
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: NodeKind::Text(text.into()),
        }
    }

    /// Returns this node's element data when it is an element node.
    #[cfg(test)]
    pub(crate) fn as_element(&self) -> Option<&Element> {
        match &self.kind {
            NodeKind::Element(element) => Some(element),
            NodeKind::Text(_) => None,
        }
    }

    fn as_element_mut(&mut self) -> Option<&mut Element> {
        match &mut self.kind {
            NodeKind::Element(element) => Some(element),
            NodeKind::Text(_) => None,
        }
    }
}

#[cfg(test)]
pub(crate) fn parse(source: &str) -> Node {
    parse_with_syntax(source, DocumentSyntax::Html).expect("HTML parsing should not fail")
}

pub(crate) fn parse_with_syntax(source: &str, syntax: DocumentSyntax) -> crate::Result<Node> {
    match syntax {
        DocumentSyntax::Html => Ok(parse_html(source)),
        DocumentSyntax::Xml => parse_xml(source),
    }
}

fn parse_html(source: &str) -> Node {
    let dom = parse_html_document(RcDom::default(), Default::default()).one(source);
    convert_document(&dom.document, DocumentSyntax::Html)
}

fn parse_xml(source: &str) -> crate::Result<Node> {
    // xml5ever currently stops building the document after an external DTD
    // declaration.  XHTML reftests commonly carry the XHTML 1.0 external DTD,
    // although Quire deliberately does not fetch document-external entities.
    // Drop that declaration before parsing so the XML tree (and its XHTML
    // namespace/case information) remains available to CSS and layout.
    let source = without_xml_doctype(source);
    let dom = parse_xml_document(RcDom::default(), Default::default()).one(source.as_ref());
    let errors = dom.errors.borrow();
    if let Some(error) = errors.first() {
        return Err(crate::Error::InvalidInput(format!(
            "XML parse error: {error}"
        )));
    }
    Ok(convert_document(&dom.document, DocumentSyntax::Xml))
}

/// Remove the optional XML document type declaration before tree construction.
///
/// The declaration may contain an internal subset, where `>` is not the end of
/// the declaration, so scanning tracks quotes and square brackets.  This only
/// affects a declaration at the start of the XML prolog; a textual `DOCTYPE`
/// later in document content is left untouched.
fn without_xml_doctype(source: &str) -> Cow<'_, str> {
    let leading = source.len() - source.trim_start().len();
    let Some(rest) = source.get(leading..) else {
        return Cow::Borrowed(source);
    };
    let Some(doctype) = rest.strip_prefix("<!DOCTYPE") else {
        return Cow::Borrowed(source);
    };
    let mut quote = None;
    let mut internal_subset_depth = 0usize;
    for (index, character) in doctype.char_indices() {
        match (quote, character) {
            (Some(delimiter), character) if character == delimiter => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(character),
            (None, '[') => internal_subset_depth += 1,
            (None, ']') => internal_subset_depth = internal_subset_depth.saturating_sub(1),
            (None, '>') if internal_subset_depth == 0 => {
                let end = leading + "<!DOCTYPE".len() + index + character.len_utf8();
                return Cow::Owned(format!("{}{}", &source[..leading], &source[end..]));
            }
            (None, _) => {}
        }
    }
    Cow::Borrowed(source)
}

fn convert_document(handle: &Handle, syntax: DocumentSyntax) -> Node {
    let mut root = Node::element("document");
    let element = root.as_element_mut().unwrap();
    element.document_syntax = syntax;
    for child in handle.children.borrow().iter() {
        if let Some(child) = convert_node(child, syntax) {
            element.children.push(child);
        }
    }
    root
}

fn convert_node(handle: &Handle, syntax: DocumentSyntax) -> Option<Node> {
    match &handle.data {
        RcNodeData::Document => Some(convert_document(handle, syntax)),
        RcNodeData::Text { contents } => Some(Node::text(contents.borrow().to_string())),
        RcNodeData::Element { name, attrs, .. } => {
            let mut node = Node::element(name.local.to_string());
            let element = node.as_element_mut().unwrap();
            element.namespace_url = name.ns.to_string();
            element.document_syntax = syntax;
            for attr in attrs.borrow().iter() {
                element
                    .attrs
                    .insert(attr.name.local.to_string(), attr.value.to_string());
                element.namespace_attrs.push(NamespacedAttribute {
                    namespace_url: attr.name.ns.to_string(),
                    local_name: attr.name.local.to_string(),
                    value: attr.value.to_string(),
                });
            }
            for child in handle.children.borrow().iter() {
                if let Some(child) = convert_node(child, syntax) {
                    element.children.push(child);
                }
            }
            Some(node)
        }
        RcNodeData::Doctype { .. }
        | RcNodeData::Comment { .. }
        | RcNodeData::ProcessingInstruction { .. } => None,
    }
}

/// Mark the document element targeted by a URL fragment.
///
/// HTML fragment navigation finds an element with a matching `id`, with
/// historical anchor-name compatibility for `<a name=...>`:
/// <https://html.spec.whatwg.org/multipage/browsing-the-web.html#scroll-to-the-fragment-identifier>.
pub(crate) fn mark_target_fragment(node: &mut Node, fragment: Option<&str>) {
    let Some(fragment) = fragment.filter(|fragment| !fragment.is_empty()) else {
        clear_target_fragment(node);
        return;
    };
    mark_target_fragment_inner(node, fragment, &mut false);
}

fn clear_target_fragment(node: &mut Node) {
    match &mut node.kind {
        NodeKind::Text(_) => {}
        NodeKind::Element(element) => {
            element.is_target = false;
            for child in &mut element.children {
                clear_target_fragment(child);
            }
        }
    }
}

fn mark_target_fragment_inner(node: &mut Node, fragment: &str, matched: &mut bool) {
    match &mut node.kind {
        NodeKind::Text(_) => {}
        NodeKind::Element(element) => {
            element.is_target = !*matched && element_matches_fragment(element, fragment);
            if element.is_target {
                *matched = true;
            }
            for child in &mut element.children {
                mark_target_fragment_inner(child, fragment, matched);
            }
        }
    }
}

fn element_matches_fragment(element: &Element, fragment: &str) -> bool {
    element.attrs.get("id").is_some_and(|id| id == fragment)
        || (element.tag == "a"
            && element
                .attrs
                .get("name")
                .is_some_and(|name| name == fragment))
}

pub(crate) fn first_element_text(node: &Node, tag: &str) -> Option<String> {
    match &node.kind {
        NodeKind::Text(_) => None,
        NodeKind::Element(element) => {
            if element.tag == tag {
                let mut text = String::new();
                collect_descendant_text(node, &mut text);
                let text = collapse_whitespace(&text);
                if !text.is_empty() {
                    return Some(text);
                }
            }
            element
                .children
                .iter()
                .find_map(|child| first_element_text(child, tag))
        }
    }
}

pub(crate) fn first_meta_content(node: &Node, name: &str) -> Option<String> {
    match &node.kind {
        NodeKind::Text(_) => None,
        NodeKind::Element(element) => {
            if element.tag == "meta"
                && element
                    .attrs
                    .get("name")
                    .or_else(|| element.attrs.get("property"))
                    .is_some_and(|value| value.eq_ignore_ascii_case(name))
            {
                return element.attrs.get("content").cloned();
            }
            element
                .children
                .iter()
                .find_map(|child| first_meta_content(child, name))
        }
    }
}

/// Returns every `content` value from matching `<meta>` elements in source order.
pub(crate) fn meta_contents(node: &Node, name: &str) -> Vec<String> {
    let mut contents = Vec::new();
    collect_meta_contents(node, name, &mut contents);
    contents
}

fn collect_meta_contents(node: &Node, name: &str, contents: &mut Vec<String>) {
    match &node.kind {
        NodeKind::Text(_) => {}
        NodeKind::Element(element) => {
            if element.tag == "meta"
                && element
                    .attrs
                    .get("name")
                    .or_else(|| element.attrs.get("property"))
                    .is_some_and(|value| value.eq_ignore_ascii_case(name))
                && let Some(content) = element.attrs.get("content")
            {
                contents.push(content.clone());
            }
            for child in &element.children {
                collect_meta_contents(child, name, contents);
            }
        }
    }
}

/// Returns the non-empty `lang` attribute of the document's root HTML element.
pub(crate) fn document_language(node: &Node) -> Option<String> {
    let NodeKind::Element(document) = &node.kind else {
        return None;
    };

    document.children.iter().find_map(|child| {
        let NodeKind::Element(element) = &child.kind else {
            return None;
        };
        (element.tag == "html")
            .then(|| element.attrs.get("lang"))
            .flatten()
            .filter(|language| !language.is_empty())
            .cloned()
    })
}

/// An author stylesheet source in document order.
///
/// CSS Cascade orders author stylesheets by their position in the document;
/// embedded `<style>` elements and external stylesheet links therefore cannot
/// be collected in separate batches:
/// <https://www.w3.org/TR/css-cascade-5/#cascade-order>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StylesheetSource {
    Embedded {
        css: String,
        scope_anchor: crate::css::StylesheetScopeAnchor,
    },
    Link {
        href: String,
        scope_anchor: crate::css::StylesheetScopeAnchor,
    },
}

pub(crate) fn stylesheet_sources_in_document_order(node: &Node) -> Vec<StylesheetSource> {
    let mut sources = Vec::new();
    collect_stylesheet_sources_in_document_order(node, &mut sources, None);
    sources
}

fn collect_stylesheet_sources_in_document_order(
    node: &Node,
    output: &mut Vec<StylesheetSource>,
    parent: Option<ElementId>,
) {
    let NodeKind::Element(element) = &node.kind else {
        return;
    };
    let scope_anchor = parent
        .map(crate::css::StylesheetScopeAnchor::Element)
        .unwrap_or(crate::css::StylesheetScopeAnchor::DocumentRoot);
    if element.tag == "style" {
        let mut css = String::new();
        collect_descendant_text(node, &mut css);
        output.push(StylesheetSource::Embedded { css, scope_anchor });
        return;
    }
    if element.tag == "link"
        && element.attrs.get("rel").is_some_and(|rel| {
            rel.split_whitespace()
                .any(|part| part.eq_ignore_ascii_case("stylesheet"))
        })
        && let Some(href) = element.attrs.get("href")
    {
        output.push(StylesheetSource::Link {
            href: href.clone(),
            scope_anchor,
        });
    }
    for child in &element.children {
        collect_stylesheet_sources_in_document_order(child, output, Some(element.id));
    }
}

/// Collect text-node descendants without applying HTML or CSS rendering rules.
///
/// HTML and XML parsing resolves character references before this DOM is built,
/// so callers must consume these values directly rather than decode them again.
/// <https://html.spec.whatwg.org/multipage/parsing.html#tokenizing-character-references>
fn collect_descendant_text(node: &Node, output: &mut String) {
    match &node.kind {
        NodeKind::Text(text) => output.push_str(text),
        NodeKind::Element(element) => {
            for child in &element.children {
                collect_descendant_text(child, output);
            }
        }
    }
}

fn collapse_whitespace(text: &str) -> String {
    let mut output = String::new();
    let mut last_was_space = true;
    for character in text.chars() {
        if character.is_whitespace() {
            if !last_was_space {
                output.push(' ');
                last_was_space = true;
            }
        } else {
            output.push(character);
            last_was_space = false;
        }
    }
    output.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        DocumentSyntax, NodeKind, StylesheetSource, first_element_text, parse, parse_with_syntax,
        stylesheet_sources_in_document_order, without_xml_doctype,
    };

    #[test]
    fn parses_nested_elements() {
        let root = parse("<div style=\"color: red\"><p>Hello &amp; PDF</p></div>");
        assert_eq!(
            first_element_text(&root, "div"),
            Some("Hello & PDF".to_string())
        );
        let NodeKind::Element(root) = root.kind else {
            panic!("expected element");
        };
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn block_start_implicitly_closes_paragraph() {
        let implicit = parse("<p>before<div>after</div>");
        let NodeKind::Element(document) = implicit.kind else {
            panic!("expected document");
        };
        let NodeKind::Element(html) = &document.children[0].kind else {
            panic!("expected html element");
        };
        let NodeKind::Element(body) = &html.children[1].kind else {
            panic!("expected body element");
        };
        let tags = body
            .children
            .iter()
            .filter_map(|child| match &child.kind {
                NodeKind::Element(element) => Some(element.tag.as_str()),
                NodeKind::Text(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tags, ["p", "div"]);
    }

    #[test]
    fn title_text_uses_parser_decoded_character_references_once() {
        let root = parse("<title>&copy; &#x1f642; &amp;lt;</title>");

        assert_eq!(
            first_element_text(&root, "title"),
            Some("© 🙂 &lt;".to_string())
        );
    }

    #[test]
    fn xml_text_uses_parser_decoded_character_references_once() {
        let root = parse_with_syntax("<Root>&amp;lt;</Root>", DocumentSyntax::Xml).unwrap();

        assert_eq!(first_element_text(&root, "Root"), Some("&lt;".to_string()));
    }

    #[test]
    fn parses_xml_with_case_and_namespace_preserved() {
        let root = parse_with_syntax(
            r#"<Root xmlns="urn:test"><Child id="a">Hello</Child></Root>"#,
            DocumentSyntax::Xml,
        )
        .unwrap();
        let NodeKind::Element(document) = root.kind else {
            panic!("expected document element");
        };
        let NodeKind::Element(root_element) = &document.children[0].kind else {
            panic!("expected XML root element");
        };
        let NodeKind::Element(child_element) = &root_element.children[0].kind else {
            panic!("expected XML child element");
        };

        assert_eq!(root_element.tag, "Root");
        assert_eq!(root_element.namespace_url, "urn:test");
        assert_eq!(root_element.document_syntax, DocumentSyntax::Xml);
        assert_eq!(child_element.tag, "Child");
        assert_eq!(child_element.namespace_url, "urn:test");
    }

    #[test]
    fn parses_xhtml_after_ignoring_external_doctype() {
        let root = parse_with_syntax(
            "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \\
             \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd\">\
             <html xmlns=\"http://www.w3.org/1999/xhtml\"><body><img src=\"image.png\"/></body></html>",
            DocumentSyntax::Xml,
        )
        .unwrap();
        let NodeKind::Element(document) = root.kind else {
            panic!("expected document element");
        };
        let NodeKind::Element(html) = &document.children[0].kind else {
            panic!("expected XHTML root element");
        };
        let NodeKind::Element(body) = &html.children[0].kind else {
            panic!("expected XHTML body element");
        };
        let NodeKind::Element(image) = &body.children[0].kind else {
            panic!("expected XHTML image element");
        };

        assert_eq!(html.namespace_url, "http://www.w3.org/1999/xhtml");
        assert_eq!(image.tag, "img");
        assert_eq!(image.attrs.get("src"), Some(&"image.png".to_string()));
    }

    #[test]
    fn embedded_stylesheet_records_its_parent_as_the_implicit_scope_root() {
        let mut root = super::Node::element("document");
        let mut section = super::Node::element("section");
        let section_id = section.as_element().expect("section element").id;
        let mut style = super::Node::element("style");
        style
            .as_element_mut()
            .expect("style element")
            .children
            .push(super::Node::text("@scope { p { color: red } }"));
        section
            .as_element_mut()
            .expect("section element")
            .children
            .push(style);
        root.as_element_mut()
            .expect("document element")
            .children
            .push(section);

        let sources = stylesheet_sources_in_document_order(&root);
        assert_eq!(sources.len(), 1);
        assert!(matches!(
            &sources[0],
            StylesheetSource::Embedded { scope_anchor, .. }
                if *scope_anchor == crate::css::StylesheetScopeAnchor::Element(section_id)
        ));
    }

    #[test]
    fn doctype_strip_keeps_internal_subset_boundaries_intact() {
        let source = "<!DOCTYPE root [<!ENTITY label \"a > b\">]><root/>";

        assert_eq!(without_xml_doctype(source), "<root/>");
    }

    #[test]
    fn reports_xml_parse_errors() {
        let error = parse_with_syntax("<root><child></root>", DocumentSyntax::Xml)
            .unwrap_err()
            .to_string();

        assert!(error.contains("XML parse error"));
    }
}
