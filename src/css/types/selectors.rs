use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

pub(in crate::css) static NEXT_ELEMENT_SIGNATURE_OPAQUE_ID: AtomicUsize = AtomicUsize::new(1);

pub(in crate::css) fn next_element_signature_opaque_id() -> Rc<usize> {
    Rc::new(NEXT_ELEMENT_SIGNATURE_OPAQUE_ID.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementAttributeSignature {
    pub namespace_url: String,
    pub local_name: String,
    pub value: String,
}

impl ElementAttributeSignature {
    pub(crate) fn new(
        namespace_url: impl Into<String>,
        local_name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            namespace_url: namespace_url.into(),
            local_name: local_name.into(),
            value: value.into(),
        }
    }
}

pub(in crate::css) fn local_attribute_signatures(
    attrs: &HashMap<String, String>,
) -> Vec<ElementAttributeSignature> {
    attrs
        .iter()
        .map(|(name, value)| ElementAttributeSignature::new("", name.clone(), value.clone()))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementSiblingSignatureList(Rc<[ElementSiblingSignature]>);

impl ElementSiblingSignatureList {
    pub(crate) fn empty() -> Self {
        Self::from_vec(Vec::<ElementSiblingSignature>::new())
    }

    pub(crate) fn from_vec(siblings: Vec<ElementSiblingSignature>) -> Self {
        Self(Rc::from(siblings.into_boxed_slice()))
    }

    pub(crate) fn as_slice(&self) -> &[ElementSiblingSignature] {
        &self.0
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, ElementSiblingSignature> {
        self.as_slice().iter()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&ElementSiblingSignature> {
        self.as_slice().get(index)
    }

    pub(crate) fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

impl std::ops::Deref for ElementSiblingSignatureList {
    type Target = [ElementSiblingSignature];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementSelectorSnapshot {
    pub tag: String,
    pub namespace_url: String,
    pub document_is_html: bool,
    pub document_compatibility_mode: crate::dom::DocumentCompatibilityMode,
    pub attrs: HashMap<String, String>,
    pub namespace_attrs: Vec<ElementAttributeSignature>,
    pub opaque_id: Rc<usize>,
    /// Stable source-DOM identity, when this signature represents a real
    /// element rather than a generated layout snapshot.
    pub source_element_id: Option<crate::dom::ElementId>,
    pub children: ElementSiblingSignatureList,
    pub has_text_child: bool,
    pub is_target: bool,
    pub has_target_descendant: bool,
    /// Deterministic static-document link history state. This is prepared
    /// from the document and effective base URLs before selector snapshots
    /// become immutable.
    pub link_state: LinkState,
    /// HTML/document directionality known from the element itself.
    ///
    /// Selectors `:dir()` matches document-language directionality rather than
    /// CSS `direction`, so selector snapshots preserve explicit `dir`,
    /// `dir=auto`, and default `<bdi>` resolution for reconstructed descendants:
    /// <https://drafts.csswg.org/selectors/#the-dir-pseudo> and
    /// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality>.
    pub document_direction: Option<Direction>,
}

/// The mutually-exclusive Selectors link state of a hyperlink.
///
/// Static rendering has no persistent history. A prepared document may mark
/// self-links as visited without exposing host navigation history.
/// <https://drafts.csswg.org/selectors/#link>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkState {
    Unvisited,
    Visited,
}

/// Immutable selector-relevant source metadata for one DOM element.
///
/// A source element may be styled many times while fragmentation, intrinsic
/// sizing, and target-reference resolution replay layout.  Keep its DOM
/// selector data behind an [`Rc`] so those replays only copy the contextual
/// selector state instead of cloning attributes and descendant signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementSiblingSignature(Rc<ElementSelectorSnapshot>);

impl std::ops::Deref for ElementSiblingSignature {
    type Target = ElementSelectorSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ElementSiblingSignature {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Rc::make_mut(&mut self.0)
    }
}

impl ElementSiblingSignature {
    pub(crate) fn with_target(mut self, is_target: bool) -> Self {
        Rc::make_mut(&mut self.0).is_target = is_target;
        self
    }

    #[cfg(test)]
    pub(crate) fn shares_snapshot_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl ElementSiblingSignature {
    pub(crate) fn new(tag: impl Into<String>, attrs: HashMap<String, String>) -> Self {
        let namespace_attrs = local_attribute_signatures(&attrs);
        Self(Rc::new(ElementSelectorSnapshot {
            tag: tag.into(),
            namespace_url: String::new(),
            document_is_html: true,
            document_compatibility_mode: crate::dom::DocumentCompatibilityMode::NoQuirks,
            attrs,
            namespace_attrs,
            opaque_id: next_element_signature_opaque_id(),
            source_element_id: None,
            children: ElementSiblingSignatureList::empty(),
            has_text_child: false,
            is_target: false,
            has_target_descendant: false,
            link_state: LinkState::Unvisited,
            document_direction: None,
        }))
    }

    pub(crate) fn with_source_element_id(mut self, id: crate::dom::ElementId) -> Self {
        Rc::make_mut(&mut self.0).source_element_id = Some(id);
        self
    }

    pub(crate) fn with_namespace(
        mut self,
        namespace_url: impl Into<String>,
        namespace_attrs: Vec<ElementAttributeSignature>,
    ) -> Self {
        let snapshot = Rc::make_mut(&mut self.0);
        snapshot.namespace_url = namespace_url.into();
        snapshot.namespace_attrs = namespace_attrs;
        self
    }

    pub(crate) fn with_document_is_html(mut self, document_is_html: bool) -> Self {
        Rc::make_mut(&mut self.0).document_is_html = document_is_html;
        self
    }

    pub(crate) fn with_document_compatibility_mode(
        mut self,
        document_compatibility_mode: crate::dom::DocumentCompatibilityMode,
    ) -> Self {
        Rc::make_mut(&mut self.0).document_compatibility_mode = document_compatibility_mode;
        self
    }

    pub(crate) fn with_link_state(mut self, link_state: LinkState) -> Self {
        Rc::make_mut(&mut self.0).link_state = link_state;
        self
    }

    pub(crate) fn with_child_list(
        mut self,
        children: ElementSiblingSignatureList,
        has_text_child: bool,
    ) -> Self {
        let snapshot = Rc::make_mut(&mut self.0);
        snapshot.children = children;
        snapshot.has_text_child = has_text_child;
        snapshot.has_target_descendant = snapshot
            .children
            .iter()
            .any(|child| child.is_target || child.has_target_descendant);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_children<Sibling>(self, children: Vec<Sibling>, has_text_child: bool) -> Self
    where
        Sibling: Into<ElementSiblingSignature>,
    {
        self.with_child_list(
            ElementSiblingSignatureList::from_vec(children.into_iter().map(Into::into).collect()),
            has_text_child,
        )
    }

    pub(crate) fn with_document_direction(mut self, direction: Direction) -> Self {
        Rc::make_mut(&mut self.0).document_direction = Some(direction);
        self
    }
}

impl From<&str> for ElementSiblingSignature {
    fn from(tag: &str) -> Self {
        Self::new(tag, HashMap::new())
    }
}

impl From<String> for ElementSiblingSignature {
    fn from(tag: String) -> Self {
        Self::new(tag, HashMap::new())
    }
}

impl ElementSignature {
    pub fn new(tag: impl Into<String>, attrs: HashMap<String, String>) -> Self {
        Self {
            selector: ElementSiblingSignature::new(tag, attrs),
            sibling_index: None,
            sibling_signatures: ElementSiblingSignatureList::empty(),
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
            selected_image_dimensions: None,
        }
    }

    pub(crate) fn from_selector_snapshot(selector: ElementSiblingSignature) -> Self {
        Self {
            selector,
            sibling_index: None,
            sibling_signatures: ElementSiblingSignatureList::empty(),
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
            selected_image_dimensions: None,
        }
    }

    /// Return the null-namespace attribute addressed by an unprefixed CSS
    /// `attr()` name on this selector snapshot.
    ///
    /// This mirrors DOM lookup so computed-time typed `attr()` substitutions
    /// retain the host-language case semantics used by deferred generated
    /// content.
    /// <https://drafts.csswg.org/css-values-5/#attr-notation>
    pub(crate) fn unprefixed_css_attr(&self, name: &str) -> Option<&str> {
        self.namespace_attrs
            .iter()
            .find(|attribute| {
                crate::css::unprefixed_attr_name_matches(
                    &self.namespace_url,
                    self.document_is_html,
                    &attribute.namespace_url,
                    &attribute.local_name,
                    name,
                )
            })
            .map(|attribute| attribute.value.as_str())
    }

    #[cfg(test)]
    pub(crate) fn with_siblings<Sibling>(
        tag: impl Into<String>,
        attrs: HashMap<String, String>,
        sibling_index: usize,
        sibling_signatures: Vec<Sibling>,
    ) -> Self
    where
        Sibling: Into<ElementSiblingSignature>,
    {
        Self::with_sibling_list(
            tag,
            attrs,
            sibling_index,
            ElementSiblingSignatureList::from_vec(
                sibling_signatures.into_iter().map(Into::into).collect(),
            ),
        )
    }

    pub(crate) fn with_sibling_list(
        tag: impl Into<String>,
        attrs: HashMap<String, String>,
        sibling_index: usize,
        sibling_signatures: ElementSiblingSignatureList,
    ) -> Self {
        let selected_sibling = sibling_signatures.get(sibling_index);
        // A signature reconstructed from a sibling list can itself later be
        // an ancestor in a selector chain. Preserve its complete selector
        // snapshot, including children, so relational selectors such as
        // `:has(> .match)` inspect the source DOM rather than an empty shell.
        // <https://drafts.csswg.org/selectors-4/#relational>
        let mut selector = ElementSiblingSignature::new(tag, attrs);
        if let Some(selected_sibling) = selected_sibling {
            // Callers can intentionally supply selector-local tag/attribute
            // data that differs from a sibling template.  Retain that public
            // construction behavior while sharing the template's recursive
            // source metadata.
            selector.namespace_url = selected_sibling.namespace_url.clone();
            selector.document_is_html = selected_sibling.document_is_html;
            selector.namespace_attrs = selected_sibling.namespace_attrs.clone();
            selector.opaque_id = Rc::clone(&selected_sibling.opaque_id);
            selector.source_element_id = selected_sibling.source_element_id;
            selector.children = selected_sibling.children.clone();
            selector.has_text_child = selected_sibling.has_text_child;
            selector.is_target = selected_sibling.is_target;
            selector.has_target_descendant = selected_sibling.has_target_descendant;
            selector.link_state = selected_sibling.link_state;
            selector.document_direction = selected_sibling.document_direction;
        }
        Self {
            selector,
            sibling_index: Some(sibling_index),
            sibling_signatures,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
            selected_image_dimensions: None,
        }
    }

    /// Reconstruct an element solely from a cached sibling snapshot.
    ///
    /// Source-DOM layout always has an entry for its sibling index, so this
    /// path avoids cloning tag, attribute, namespace, and child metadata.
    pub(crate) fn from_sibling_snapshot(
        sibling_index: usize,
        sibling_signatures: ElementSiblingSignatureList,
    ) -> Option<Self> {
        let selector = sibling_signatures.get(sibling_index)?.clone();
        Some(Self {
            selector,
            sibling_index: Some(sibling_index),
            sibling_signatures,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
            selected_image_dimensions: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_link_state(mut self, link_state: LinkState) -> Self {
        self.selector = self.selector.with_link_state(link_state);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_child_list(
        mut self,
        children: ElementSiblingSignatureList,
        has_text_child: bool,
    ) -> Self {
        self.selector = self.selector.with_child_list(children, has_text_child);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_children<Sibling>(self, children: Vec<Sibling>, has_text_child: bool) -> Self
    where
        Sibling: Into<ElementSiblingSignature>,
    {
        self.with_child_list(
            ElementSiblingSignatureList::from_vec(children.into_iter().map(Into::into).collect()),
            has_text_child,
        )
    }

    #[cfg(test)]
    pub(crate) fn with_namespace(
        mut self,
        namespace_url: impl Into<String>,
        namespace_attrs: Vec<ElementAttributeSignature>,
    ) -> Self {
        self.selector = self.selector.with_namespace(namespace_url, namespace_attrs);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_document_is_html(mut self, document_is_html: bool) -> Self {
        self.selector = self.selector.with_document_is_html(document_is_html);
        self
    }

    /// Attach HTML/document directionality for selector matching.
    ///
    /// Selectors `:dir()` uses the host language's directionality, not CSS
    /// `direction`; undefined directionality inherits during selector matching:
    /// <https://drafts.csswg.org/selectors/#the-dir-pseudo> and
    /// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality>.
    pub(crate) fn with_document_direction(mut self, direction: Direction) -> Self {
        self.selector = self.selector.with_document_direction(direction);
        self
    }

    /// Attach HTML's dynamically resolved directionality for cascade input.
    ///
    /// The HTML `dir=auto` and default `<bdi>` algorithms produce an element
    /// directionality value that the Rendering section maps through UA
    /// `direction` rules using `:dir()`:
    /// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality> and
    /// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering>.
    pub(crate) fn with_html_direction(mut self, direction: Direction) -> Self {
        self.html_direction = Some(direction);
        self
    }

    /// Attach the element direction known before selector matching.
    ///
    /// This is the inherited/computed `direction` available during cascade
    /// construction for layout-facing style resolution. It is intentionally not
    /// used for Selectors `:dir()`, which matches document-language
    /// directionality rather than CSS `direction`:
    /// <https://drafts.csswg.org/selectors/#the-dir-pseudo>.
    pub(crate) fn with_resolved_direction(mut self, direction: Direction) -> Self {
        self.resolved_direction = Some(direction);
        self
    }

    /// Attach the element language known before selector matching.
    ///
    /// Selectors `:lang()` matches the element's document language, including
    /// inherited language and explicit unknown language. CSS delegates the
    /// language range matching to RFC 4647 filtering:
    /// <https://www.w3.org/TR/selectors-4/#the-lang-pseudo> and
    /// <https://www.rfc-editor.org/rfc/rfc4647#section-3.3.2>.
    pub(crate) fn with_resolved_language(mut self, language: ResolvedLanguage) -> Self {
        self.resolved_language = language;
        self
    }

    pub(crate) fn sibling_at(&self, index: usize) -> Option<Self> {
        let sibling = self.sibling_signatures.get(index)?.clone();
        Some(Self {
            selector: sibling,
            sibling_index: Some(index),
            sibling_signatures: self.sibling_signatures.clone(),
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
            selected_image_dimensions: None,
        })
    }

    pub(crate) fn child_at(&self, index: usize) -> Option<Self> {
        let child = self.children.get(index)?.clone();
        Some(Self {
            selector: child,
            sibling_index: Some(index),
            sibling_signatures: self.children.clone(),
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
            selected_image_dimensions: None,
        })
    }
}
