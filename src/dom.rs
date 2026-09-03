use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::{QuirksMode as HtmlParserQuirksMode, TreeBuilderOpts};
use html5ever::{ParseOpts as HtmlParseOptions, parse_document as parse_html_document};
use markup5ever_rcdom::{Handle, NodeData as RcNodeData, RcDom};
use xml5ever::driver::parse_document as parse_xml_document;

use crate::css::ResolveViewportLengths;
use crate::units::{LayoutSize, PercentageBasis, SemanticLengthExt, layout_pt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentSyntax {
    Html,
    Xml,
}

/// Compatibility mode selected by HTML's tree-construction algorithm.
///
/// This is document state rather than a CSS heuristic: HTML determines it
/// from the parsed doctype and passes it to layout and selector matching.
/// <https://html.spec.whatwg.org/multipage/parsing.html#the-initial-insertion-mode>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DocumentCompatibilityMode {
    #[default]
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

impl From<HtmlParserQuirksMode> for DocumentCompatibilityMode {
    fn from(mode: HtmlParserQuirksMode) -> Self {
        match mode {
            HtmlParserQuirksMode::NoQuirks => Self::NoQuirks,
            HtmlParserQuirksMode::LimitedQuirks => Self::LimitedQuirks,
            HtmlParserQuirksMode::Quirks => Self::Quirks,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
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
    pub document_compatibility_mode: DocumentCompatibilityMode,
    pub attrs: HashMap<String, String>,
    pub namespace_attrs: Vec<NamespacedAttribute>,
    pub children: Vec<Node>,
    pub is_target: bool,
    /// Selector metadata materialized once after the prepared DOM has reached
    /// its render-time stable state (including fragment-target selection).
    pub(crate) selector_snapshot: OnceCell<crate::css::ElementSiblingSignature>,
    /// Static rendering outcome selected for an HTML `<object>` element.
    ///
    /// The HTML resource-selection algorithm decides whether an object
    /// represents its external resource or its fallback subtree. This is
    /// resolved after optional visual resources have been preloaded and before
    /// CSS box construction, so all layout paths observe the same result.
    /// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-object-element>
    pub object_rendering: ObjectRendering,
    /// The image candidate selected before visual-resource preloading.
    ///
    /// `<picture>` source selection must be shared by preloading, static
    /// availability selection, and layout so every stage observes the same
    /// resource and density.
    /// <https://html.spec.whatwg.org/multipage/images.html#updating-the-image-data>
    pub selected_image_source: Option<SelectedImageSource>,
    /// Static rendering outcome selected for an HTML `<img>` element.
    ///
    /// A failed image with non-empty alternative text is rendered as ordinary
    /// fallback text, rather than as a replaced element. This is resolved
    /// after optional visual resources have been preloaded and before CSS box
    /// construction, so layout never has to infer resource availability.
    /// <https://html.spec.whatwg.org/multipage/rendering.html>
    pub image_rendering: ImageRendering,
}

/// The static renderer's selected representation for an HTML `<object>`.
///
/// A live browser can change this as a resource loads. Spindrift resolves the
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

/// The static renderer's selected representation for an HTML `<img>`.
///
/// A live browser can transition through loading and failure states. Spindrift
/// performs one deterministic paged layout, so it selects the decoded image,
/// its alternative text fallback, or an empty fallback once visual resources
/// have been preloaded.
/// <https://html.spec.whatwg.org/multipage/rendering.html>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ImageRendering {
    /// Render a decoded raster or SVG resource as a replaced image.
    #[default]
    Image,
    /// Render the non-empty `alt` attribute through ordinary text layout.
    AltText,
    /// Render no fallback text; the image keeps zero natural dimensions.
    Empty,
}

/// A fixed-resolution image candidate selected for an HTML `<img>` element.
///
/// The density is finite and strictly positive by construction, making its
/// equality relation reflexive despite its floating-point representation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SelectedImageSource {
    pub(crate) url: String,
    pub(crate) density: ImageDensity,
    /// The dimension attributes that HTML selected with this source, if a
    /// `<picture>` source supplied either dimension.
    pub(crate) dimensions: Option<ImageDimensionAttributes>,
}

impl Eq for SelectedImageSource {}

/// A selected image's effective pixel density.
///
/// A width candidate with a zero source size has an infinite density; HTML
/// gives that resource zero density-corrected natural dimensions rather than
/// allowing an IEEE infinity into layout arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageDensity {
    Finite(OrderedImageDensity),
    Infinite,
}

/// A finite, strictly-positive image density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OrderedImageDensity(f32);

impl Eq for OrderedImageDensity {}

impl OrderedImageDensity {
    fn new(value: f32) -> Option<Self> {
        (value.is_finite() && value > 0.0).then_some(Self(value))
    }

    pub(crate) fn value(self) -> f32 {
        self.0
    }
}

impl ImageDensity {
    pub(crate) fn one() -> Self {
        Self::Finite(OrderedImageDensity::new(1.0).expect("1x is valid"))
    }
}

/// Raw HTML dimension attributes selected for an `<img>` resource.
///
/// These are deliberately not selector attributes: a selected `<source>` can
/// supply presentational dimensions without changing what CSS selectors or
/// `attr()` see on the actual `<img>` element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageDimensionAttributes {
    pub(crate) width: Option<String>,
    pub(crate) height: Option<String>,
}

impl SelectedImageSource {
    fn new(url: impl Into<String>, density: ImageDensity) -> Self {
        Self {
            url: url.into(),
            density,
            dimensions: None,
        }
    }

    pub(crate) fn with_dimensions(mut self, dimensions: ImageDimensionAttributes) -> Self {
        self.dimensions = Some(dimensions);
        self
    }
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
    /// Returns the HTML dimension attributes selected with this image source.
    pub(crate) fn selected_image_dimensions(&self) -> Option<&ImageDimensionAttributes> {
        self.selected_image_source
            .as_ref()
            .and_then(|source| source.dimensions.as_ref())
    }

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
                document_compatibility_mode: DocumentCompatibilityMode::NoQuirks,
                attrs: HashMap::new(),
                namespace_attrs: Vec::new(),
                children: Vec::new(),
                is_target: false,
                selector_snapshot: OnceCell::new(),
                object_rendering: ObjectRendering::Fallback,
                selected_image_source: None,
                image_rendering: ImageRendering::Image,
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

    pub(crate) fn as_element_mut(&mut self) -> Option<&mut Element> {
        match &mut self.kind {
            NodeKind::Element(element) => Some(element),
            NodeKind::Text(_) => None,
        }
    }
}

/// Select the fixed-resolution source used by Spindrift for an HTML `<img>`.
///
/// The paged renderer has a fixed 1dppx output resolution. Density candidates
/// follow Spindrift's fixed-resolution selection policy and preserve source order
/// for equal densities.
/// <https://html.spec.whatwg.org/multipage/images.html#attr-img-srcset>
/// <https://drafts.csswg.org/css-images-4/#image-set-resolution>
pub(crate) fn selected_img_source(element: &Element) -> Option<(&str, ImageDensity)> {
    debug_assert_eq!(element.tag, "img");
    element
        .selected_image_source
        .as_ref()
        .map(|source| (source.url.as_str(), source.density))
        // DOM preparation always sets the stored value before resource
        // discovery. Keep the attribute fallback for isolated layout tests
        // and internal callers that construct an element without that pass.
        .or_else(|| {
            element
                .attrs
                .get("src")
                .map(|src| (src.as_str(), ImageDensity::one()))
        })
}

/// Resolve this renderer's fixed-resolution candidate from an image-like
/// element's own attributes. `<picture>` ownership is resolved separately by
/// the HTML preparation pass and stored on its child `<img>`.
pub(crate) fn selected_image_source_from_attributes(
    element: &Element,
    media_environment: &crate::css::MediaEnvironment,
) -> Option<SelectedImageSource> {
    selected_source_set_candidate(
        element.attrs.get("src").map(String::as_str),
        element.attrs.get("srcset").map(String::as_str),
        element.attrs.get("sizes").map(String::as_str),
        media_environment,
    )
}

/// Resolve one static HTML source set at the renderer's output density.
///
/// This owns source-set parsing, width-descriptor normalization, and the
/// deterministic static choice shared by image preloading and layout.
/// <https://html.spec.whatwg.org/multipage/images.html#creating-a-source-set>
pub(crate) fn selected_source_set_candidate(
    src: Option<&str>,
    srcset: Option<&str>,
    sizes: Option<&str>,
    media_environment: &crate::css::MediaEnvironment,
) -> Option<SelectedImageSource> {
    let mut candidates = srcset
        .filter(|value| !value.is_empty())
        .map(parse_srcset_candidates)
        .unwrap_or_default();
    let has_width_descriptor = candidates
        .iter()
        .any(|candidate| matches!(candidate.descriptor, ImageCandidateDescriptor::Width(_)));
    if !has_width_descriptor && let Some(src) = src.filter(|value| !value.is_empty()) {
        candidates.push(ParsedImageCandidate {
            url: src.to_owned(),
            descriptor: ImageCandidateDescriptor::Density(
                OrderedImageDensity::new(1.0).expect("1x is valid"),
            ),
        });
    }
    let source_size = source_size_from_sizes(sizes, media_environment);
    let target = media_environment.resolution_dppx;
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let density = match candidate.descriptor {
                ImageCandidateDescriptor::Density(density) => ImageDensity::Finite(density),
                ImageCandidateDescriptor::Width(width) => {
                    if source_size == 0.0 {
                        ImageDensity::Infinite
                    } else {
                        ImageDensity::Finite(OrderedImageDensity::new(width as f32 / source_size)?)
                    }
                }
            };
            Some(SelectedImageSource::new(candidate.url, density))
        })
        .fold(None, |selected: Option<SelectedImageSource>, candidate| {
            let candidate_density = image_density_order(candidate.density);
            match selected {
                None => Some(candidate),
                Some(selected) => {
                    let selected_density = image_density_order(selected.density);
                    let candidate_is_sufficient = candidate_density >= target;
                    let selected_is_sufficient = selected_density >= target;
                    if (candidate_is_sufficient
                        && (!selected_is_sufficient || candidate_density < selected_density))
                        || (!candidate_is_sufficient
                            && !selected_is_sufficient
                            && candidate_density > selected_density)
                    {
                        Some(candidate)
                    } else {
                        Some(selected)
                    }
                }
            }
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedImageCandidate {
    url: String,
    descriptor: ImageCandidateDescriptor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageCandidateDescriptor {
    Density(OrderedImageDensity),
    Width(u32),
}

fn parse_srcset_candidates(srcset: &str) -> Vec<ParsedImageCandidate> {
    parse_srcset::parse_srcset(srcset)
        .into_iter()
        .filter_map(|candidate| {
            // `h` is not a current HTML image-candidate descriptor.
            if candidate.height.is_some() {
                return None;
            }
            let descriptor = match (candidate.width, candidate.density) {
                (Some(width), None) => ImageCandidateDescriptor::Width(u32::try_from(width).ok()?),
                (None, Some(density)) => {
                    ImageCandidateDescriptor::Density(OrderedImageDensity::new(density as f32)?)
                }
                (None, None) => ImageCandidateDescriptor::Density(
                    OrderedImageDensity::new(1.0).expect("1x is valid"),
                ),
                (Some(_), Some(_)) => return None,
            };
            Some(ParsedImageCandidate {
                url: candidate.url,
                descriptor,
            })
        })
        .collect()
}

pub(crate) fn image_density_order(density: ImageDensity) -> f32 {
    match density {
        ImageDensity::Finite(density) => density.value(),
        ImageDensity::Infinite => f32::INFINITY,
    }
}

fn source_size_from_sizes(
    sizes: Option<&str>,
    media_environment: &crate::css::MediaEnvironment,
) -> f32 {
    let fallback = media_environment.viewport.width.max(0.0);
    let Some(sizes) = sizes else {
        return fallback;
    };
    let entries = crate::css::component_values::split_css_top_level_delimiter(sizes, ',');
    for entry in entries {
        let components = crate::css::split_css_component_values(entry);
        let Some((length, condition)) = components.split_last() else {
            return fallback;
        };
        if length.eq_ignore_ascii_case("auto") {
            continue;
        }
        if !condition.is_empty()
            && !crate::css::media_rule_applies_in_environment(
                &condition.join(" "),
                media_environment,
            )
        {
            continue;
        }
        let Some(mut length) =
            crate::css::parse_computed_length_percentage(length, crate::css::ROOT_FONT_SIZE_PT)
        else {
            return fallback;
        };
        if length.contains_percentage() {
            return fallback;
        }
        let viewport = LayoutSize::new(
            media_environment.viewport.width * crate::css::CSS_PX_TO_PT,
            media_environment.viewport.height * crate::css::CSS_PX_TO_PT,
        );
        length.resolve_viewport_lengths(crate::css::ViewportLengthBasis::for_writing_mode(
            viewport,
            crate::css::WritingMode::HorizontalTb,
        ));
        let value = length
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(0.0)))
            .map(SemanticLengthExt::points)
            .unwrap_or(0.0)
            / crate::css::CSS_PX_TO_PT;
        return if value.is_finite() && value >= 0.0 {
            value
        } else {
            fallback
        };
    }
    fallback
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
    // Spindrift is a static renderer and never runs document scripts. Select the
    // corresponding HTML parser mode so `<noscript>` fallback markup is built
    // as DOM content instead of being retained as raw text.
    // https://html.spec.whatwg.org/multipage/scripting.html#the-noscript-element
    let options = HtmlParseOptions {
        tree_builder: TreeBuilderOpts {
            scripting_enabled: false,
            ..TreeBuilderOpts::default()
        },
        ..HtmlParseOptions::default()
    };
    let dom = parse_html_document(RcDom::default(), options).one(source);
    convert_document(
        &dom.document,
        DocumentSyntax::Html,
        dom.quirks_mode.get().into(),
    )
}

fn parse_xml(source: &str) -> crate::Result<Node> {
    // xml5ever currently stops building the document after an external DTD
    // declaration.  XHTML reftests commonly carry the XHTML 1.0 external DTD,
    // although Spindrift deliberately does not fetch document-external entities.
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
    Ok(convert_document(
        &dom.document,
        DocumentSyntax::Xml,
        DocumentCompatibilityMode::NoQuirks,
    ))
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

fn convert_document(
    handle: &Handle,
    syntax: DocumentSyntax,
    compatibility_mode: DocumentCompatibilityMode,
) -> Node {
    let mut root = Node::element("document");
    let element = root.as_element_mut().unwrap();
    element.document_syntax = syntax;
    element.document_compatibility_mode = compatibility_mode;
    for child in handle.children.borrow().iter() {
        if let Some(child) = convert_node(child, syntax, compatibility_mode) {
            element.children.push(child);
        }
    }
    root
}

fn convert_node(
    handle: &Handle,
    syntax: DocumentSyntax,
    compatibility_mode: DocumentCompatibilityMode,
) -> Option<Node> {
    match &handle.data {
        RcNodeData::Document => Some(convert_document(handle, syntax, compatibility_mode)),
        RcNodeData::Text { contents } => Some(Node::text(contents.borrow().to_string())),
        RcNodeData::Element { name, attrs, .. } => {
            let mut node = Node::element(name.local.to_string());
            let element = node.as_element_mut().unwrap();
            element.namespace_url = name.ns.to_string();
            element.document_syntax = syntax;
            element.document_compatibility_mode = compatibility_mode;
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
                if let Some(child) = convert_node(child, syntax, compatibility_mode) {
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
        DocumentCompatibilityMode, DocumentSyntax, ImageDensity, Node, NodeKind, StylesheetSource,
        collect_descendant_text, first_element_text, image_density_order, parse,
        parse_srcset_candidates, parse_with_syntax, selected_image_source_from_attributes,
        selected_img_source, selected_source_set_candidate, stylesheet_sources_in_document_order,
        without_xml_doctype,
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
    fn html_noscript_fallback_markup_is_parsed_as_dom_content() {
        let root = parse("<noscript><span>fallback</span></noscript>");
        let mut text = String::new();
        collect_descendant_text(&root, &mut text);

        assert_eq!(
            first_element_text(&root, "span"),
            Some("fallback".to_string())
        );
        assert!(
            !text.contains("<span>"),
            "scripting-enabled parsing would retain fallback markup as raw text: {text:?}"
        );
    }

    #[test]
    fn image_source_selection_merges_src_with_density_candidates() {
        let mut image = Node::element("img");
        let image = image.as_element_mut().expect("image element");
        image.attrs.insert(
            "srcset".to_string(),
            "small.png 0.5x, normal.png 1x, large.png 2x".to_string(),
        );

        image.selected_image_source =
            selected_image_source_from_attributes(image, &crate::css::MediaEnvironment::default());
        assert_eq!(
            selected_img_source(image).map(|(url, density)| (url, image_density_order(density))),
            Some(("normal.png", 1.0))
        );

        image
            .attrs
            .insert("src".to_string(), "fallback.png".to_string());
        image.selected_image_source =
            selected_image_source_from_attributes(image, &crate::css::MediaEnvironment::default());
        assert_eq!(
            selected_img_source(image).map(|(url, density)| (url, image_density_order(density))),
            Some(("normal.png", 1.0))
        );
    }

    #[test]
    fn width_descriptors_normalize_against_the_default_source_size() {
        let environment = crate::css::MediaEnvironment::new(
            crate::css::MediaType::Print,
            crate::css::CssViewportSize::new(300.0, 200.0),
        );
        let selected = selected_source_set_candidate(
            None,
            Some("red.png 1w, green.png 200w"),
            None,
            &environment,
        )
        .expect("width candidates select an image");

        assert_eq!(selected.url, "green.png");
        assert_eq!(image_density_order(selected.density), 200.0 / 300.0);
    }

    #[test]
    fn sizes_uses_the_first_matching_media_condition() {
        let environment = crate::css::MediaEnvironment::new(
            crate::css::MediaType::Print,
            crate::css::CssViewportSize::new(500.0, 200.0),
        );
        let selected = selected_source_set_candidate(
            None,
            Some("small.png 200w, large.png 500w"),
            Some("(min-width: 400px) 50vw, 100vw"),
            &environment,
        )
        .expect("width candidates select an image");

        assert_eq!(selected.url, "large.png");
        assert_eq!(image_density_order(selected.density), 2.0);
    }

    #[test]
    fn zero_source_size_keeps_infinite_density_out_of_layout_scalars() {
        let environment = crate::css::MediaEnvironment::new(
            crate::css::MediaType::Print,
            crate::css::CssViewportSize::new(500.0, 200.0),
        );
        let selected =
            selected_source_set_candidate(None, Some("green.png 200w"), Some("0px"), &environment)
                .expect("zero source size still selects its candidate");

        assert_eq!(selected.density, ImageDensity::Infinite);
    }

    #[test]
    fn srcset_adapter_preserves_data_url_commas_and_rejects_height_descriptors() {
        let candidates = parse_srcset_candidates(
            "data:image/svg+xml,%3Csvg%3E 1x, rejected.png 20w 10h, selected.png 2x",
        );

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].url, "data:image/svg+xml,%3Csvg%3E");
        assert_eq!(candidates[1].url, "selected.png");
    }

    #[test]
    fn html_parser_compatibility_mode_is_preserved_on_the_document_tree() {
        let quirks = parse("<p>quirks");
        let standards = parse("<!doctype html><p>standards");
        let NodeKind::Element(quirks_document) = quirks.kind else {
            panic!("expected document element");
        };
        let NodeKind::Element(standards_document) = standards.kind else {
            panic!("expected document element");
        };

        assert_eq!(
            quirks_document.document_compatibility_mode,
            DocumentCompatibilityMode::Quirks
        );
        assert_eq!(
            standards_document.document_compatibility_mode,
            DocumentCompatibilityMode::NoQuirks
        );
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
    fn html_table_fixup_preserves_implicit_rows_and_cells() {
        let document = parse(
            "<table><col width=40><col width=100><tr><td class=corner><td class=edge></table>",
        );

        fn collect_tags(node: &super::Node, tags: &mut Vec<String>) {
            let NodeKind::Element(element) = &node.kind else {
                return;
            };
            tags.push(element.tag.clone());
            for child in &element.children {
                collect_tags(child, tags);
            }
        }

        let mut tags = Vec::new();
        collect_tags(&document, &mut tags);
        assert_eq!(tags.iter().filter(|tag| tag.as_str() == "col").count(), 2);
        assert_eq!(tags.iter().filter(|tag| tag.as_str() == "tbody").count(), 1);
        assert_eq!(tags.iter().filter(|tag| tag.as_str() == "tr").count(), 1);
        assert_eq!(tags.iter().filter(|tag| tag.as_str() == "td").count(), 2);
    }

    #[test]
    fn title_text_uses_parser_decoded_character_references_once() {
        let root = parse("<title>&copy; &#x1f642; &amp;lt;</title>");

        assert_eq!(
            first_element_text(&root, "title"),
            Some("© 🙂 &lt;".to_string())
        );
    }

    /// HTML retains legacy named references without a semicolon when their
    /// following character cannot continue an ASCII name.  The two references
    /// here are separated by `&` and `<`, respectively.
    /// <https://html.spec.whatwg.org/multipage/parsing.html#named-character-reference-state>
    #[test]
    fn html_legacy_named_references_without_semicolons_are_decoded() {
        let root = parse("<p>&nbsp&nbsp</p>");
        let mut text = String::new();
        collect_descendant_text(&root, &mut text);

        assert_eq!(text, "\u{a0}\u{a0}");
    }

    /// HTML maps C1 numeric character references through its Windows-1252
    /// replacement table, but preserves a literal NEL scalar in the DOM.
    /// <https://html.spec.whatwg.org/multipage/parsing.html#numeric-character-reference-end-state>
    #[test]
    fn html_c1_numeric_reference_is_not_a_literal_nel() {
        let literal = parse("<p>A\u{0085}B</p>");
        let numeric_reference = parse("<p>A&#x0085;B</p>");
        let mut literal_text = String::new();
        let mut numeric_reference_text = String::new();
        collect_descendant_text(&literal, &mut literal_text);
        collect_descendant_text(&numeric_reference, &mut numeric_reference_text);

        assert_eq!(literal_text, "A\u{0085}B");
        assert_eq!(numeric_reference_text, "A\u{2026}B");
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
