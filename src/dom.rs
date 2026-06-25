use std::collections::HashMap;

use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData as RcNodeData, RcDom};

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
    pub tag: String,
    pub namespace_url: String,
    pub attrs: HashMap<String, String>,
    pub namespace_attrs: Vec<NamespacedAttribute>,
    pub children: Vec<Node>,
    pub is_target: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamespacedAttribute {
    pub namespace_url: String,
    pub local_name: String,
    pub value: String,
}

impl Node {
    pub fn element(tag: impl Into<String>) -> Self {
        Self {
            kind: NodeKind::Element(Element {
                tag: tag.into(),
                namespace_url: String::new(),
                attrs: HashMap::new(),
                namespace_attrs: Vec::new(),
                children: Vec::new(),
                is_target: false,
            }),
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: NodeKind::Text(text.into()),
        }
    }

    fn as_element_mut(&mut self) -> Option<&mut Element> {
        match &mut self.kind {
            NodeKind::Element(element) => Some(element),
            NodeKind::Text(_) => None,
        }
    }
}

pub(crate) fn parse(source: &str) -> Node {
    let dom = parse_document(RcDom::default(), Default::default()).one(source);
    convert_document(&dom.document)
}

fn convert_document(handle: &Handle) -> Node {
    let mut root = Node::element("document");
    let element = root.as_element_mut().unwrap();
    for child in handle.children.borrow().iter() {
        if let Some(child) = convert_node(child) {
            element.children.push(child);
        }
    }
    root
}

fn convert_node(handle: &Handle) -> Option<Node> {
    match &handle.data {
        RcNodeData::Document => Some(convert_document(handle)),
        RcNodeData::Text { contents } => Some(Node::text(contents.borrow().to_string())),
        RcNodeData::Element { name, attrs, .. } => {
            let mut node = Node::element(name.local.to_string());
            let element = node.as_element_mut().unwrap();
            element.namespace_url = name.ns.to_string();
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
                if let Some(child) = convert_node(child) {
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

pub(crate) fn text_content(node: &Node) -> String {
    let mut output = String::new();
    collect_text(node, &mut output);
    normalize_text(&output)
}

pub(crate) fn first_element_text(node: &Node, tag: &str) -> Option<String> {
    match &node.kind {
        NodeKind::Text(_) => None,
        NodeKind::Element(element) => {
            if element.tag == tag {
                let text = text_content(node);
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

pub(crate) fn stylesheet_links(node: &Node) -> Vec<String> {
    let mut links = Vec::new();
    collect_stylesheet_links(node, &mut links);
    links
}

pub(crate) fn normalize_text(text: &str) -> String {
    decode_entities(&collapse_whitespace(text))
}

fn collect_stylesheet_links(node: &Node, links: &mut Vec<String>) {
    match &node.kind {
        NodeKind::Text(_) => {}
        NodeKind::Element(element) => {
            if element.tag == "link"
                && element.attrs.contains_key("href")
                && element.attrs.get("rel").is_some_and(|rel| {
                    rel.split_whitespace()
                        .any(|part| part.eq_ignore_ascii_case("stylesheet"))
                })
                && let Some(href) = element.attrs.get("href")
            {
                links.push(href.clone());
            }
            for child in &element.children {
                collect_stylesheet_links(child, links);
            }
        }
    }
}

fn collect_text(node: &Node, output: &mut String) {
    match &node.kind {
        NodeKind::Text(text) => output.push_str(text),
        NodeKind::Element(element) => {
            if matches!(element.tag.as_str(), "style" | "script" | "head") {
                return;
            }
            if matches!(
                element.tag.as_str(),
                "p" | "div" | "h1" | "h2" | "h3" | "br"
            ) {
                output.push('\n');
            }
            for child in &element.children {
                collect_text(child, output);
            }
            if matches!(element.tag.as_str(), "p" | "div" | "h1" | "h2" | "h3") {
                output.push('\n');
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

fn decode_entities(text: &str) -> String {
    decode_entities_public(text)
}

pub(crate) fn decode_entities_public(text: &str) -> String {
    decode_numeric_entities(
        &text
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&nbsp;", " "),
    )
}

fn decode_numeric_entities(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("&#") {
        output.push_str(&rest[..start]);
        let entity = &rest[start + 2..];
        let Some(end) = entity.find(';') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let number = &entity[..end];
        let codepoint = number
            .strip_prefix('x')
            .or_else(|| number.strip_prefix('X'))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .or_else(|| number.parse::<u32>().ok());
        if let Some(character) = codepoint.and_then(char::from_u32) {
            output.push(character);
        } else {
            output.push_str(&rest[start..start + end + 3]);
        }
        rest = &entity[end + 1..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::{NodeKind, parse, text_content};

    #[tokio::test]
    async fn parses_nested_elements() {
        let root = parse("<div style=\"color: red\"><p>Hello &amp; PDF</p></div>");
        assert_eq!(text_content(&root), "Hello & PDF");
        let NodeKind::Element(root) = root.kind else {
            panic!("expected element");
        };
        assert_eq!(root.children.len(), 1);
    }
}
