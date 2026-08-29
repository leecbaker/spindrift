use std::borrow::Cow;
use std::rc::Rc;

use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::context::MatchingContext;
use selectors::matching::ElementSelectorFlags;
use selectors::{Element as SelectorElement, OpaqueElement};

use super::{
    CssAtom, CssString, QuirePseudoClass, QuirePseudoElement, QuireSelectorImpl,
    language_from_attrs, language_matches_any_range,
};
use crate::css::html_form_state;
use crate::css::types::{
    ContentLanguage, Direction, ElementSignature, LinkState, ResolvedLanguage,
};

pub(in crate::css) const HTML_NAMESPACE_URL: &str = "http://www.w3.org/1999/xhtml";

pub(in crate::css) fn normalized_element_namespace(
    namespace_url: &str,
    document_is_html: bool,
) -> &str {
    if document_is_html && namespace_url.is_empty() {
        HTML_NAMESPACE_URL
    } else {
        namespace_url
    }
}

pub(in crate::css) fn element_namespace_matches(
    element_namespace: &str,
    document_is_html: bool,
    selector_namespace: &str,
) -> bool {
    selector_namespace.is_empty()
        || normalized_element_namespace(element_namespace, document_is_html) == selector_namespace
}

#[derive(Clone, Debug)]
pub(in crate::css) struct StyleElement<'a> {
    pub(in crate::css) chain: Rc<Vec<Cow<'a, ElementSignature>>>,
    pub(in crate::css) index: usize,
    pub(in crate::css) link_matching: LinkMatching,
}

/// Which link-state view a cascade selector match observes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::css) enum LinkMatching {
    Actual,
    ForceUnvisited,
}

impl StyleElement<'_> {
    pub(in crate::css) fn signature(&self) -> &ElementSignature {
        &self.chain[self.index]
    }

    fn link_state(&self) -> LinkState {
        match self.link_matching {
            LinkMatching::Actual => self.signature().link_state,
            LinkMatching::ForceUnvisited => LinkState::Unvisited,
        }
    }

    pub(in crate::css) fn sibling_element_at(&self, sibling_index: usize) -> Option<Self> {
        let sibling = self.signature().sibling_at(sibling_index)?;
        let mut chain = self.chain[..self.index].to_vec();
        chain.push(Cow::Owned(sibling));
        Some(Self {
            chain: Rc::new(chain),
            index: self.index,
            link_matching: self.link_matching,
        })
    }

    pub(in crate::css) fn child_element_at(&self, child_index: usize) -> Option<Self> {
        let child = self.signature().child_at(child_index)?;
        // `self` can be an ancestor borrowed from a longer subject chain while
        // evaluating a relational selector. Its synthetic child belongs below
        // this element, not below the original subject that follows it in the
        // chain. Keep only the path through `self` before appending the child.
        // <https://drafts.csswg.org/selectors-4/#relational>
        let mut chain = self.chain[..=self.index].to_vec();
        chain.push(Cow::Owned(child));
        let index = chain.len() - 1;
        Some(Self {
            chain: Rc::new(chain),
            index,
            link_matching: self.link_matching,
        })
    }

    pub(in crate::css) fn directionality(&self) -> Option<Direction> {
        if let Some(direction) = self
            .signature()
            .document_direction
            .or(self.signature().html_direction)
        {
            return Some(direction);
        }
        if let Some(direction) = self.signature().attrs.get("dir").and_then(|value| {
            match value.trim().to_ascii_lowercase().as_str() {
                "ltr" => Some(Direction::Ltr),
                "rtl" => Some(Direction::Rtl),
                _ => None,
            }
        }) {
            return Some(direction);
        }
        self.parent_element()
            .and_then(|parent| parent.directionality())
            .or(Some(Direction::Ltr))
    }

    pub(in crate::css) fn language(&self) -> ResolvedLanguage {
        if self.signature().resolved_language != ResolvedLanguage::Unresolved {
            return self.signature().resolved_language.clone();
        }
        if let Some(language) = language_from_attrs(&self.signature().attrs) {
            return language;
        }
        self.parent_element()
            .map(|parent| parent.language())
            .unwrap_or(ResolvedLanguage::Resolved(ContentLanguage::Unknown))
    }

    pub(in crate::css) fn is_disabled(&self) -> bool {
        if !html_form_state::disableable_element(self.signature().tag.as_str()) {
            return false;
        }
        if self.signature().attrs.contains_key("disabled") {
            return true;
        }
        match self.signature().tag.as_str() {
            "option" => {
                if self.parent_element().is_some_and(|parent| {
                    matches!(parent.signature().tag.as_str(), "optgroup" | "select")
                        && parent.is_disabled()
                }) {
                    return true;
                }
            }
            "optgroup"
                if self.parent_element().is_some_and(|parent| {
                    parent.signature().tag == "select" && parent.is_disabled()
                }) =>
            {
                return true;
            }
            _ => {}
        }
        self.disabled_by_fieldset()
    }

    pub(in crate::css) fn is_checked(&self) -> bool {
        match self.signature().tag.as_str() {
            "input" => {
                matches!(
                    html_form_state::input_type(&self.signature().tag, &self.signature().attrs)
                        .as_deref(),
                    Some("checkbox" | "radio")
                ) && self.signature().attrs.contains_key("checked")
            }
            "option" => self.is_option_selected(),
            _ => false,
        }
    }

    pub(in crate::css) fn is_default(&self) -> bool {
        match self.signature().tag.as_str() {
            "input" => {
                matches!(
                    html_form_state::input_type(&self.signature().tag, &self.signature().attrs)
                        .as_deref(),
                    Some("checkbox" | "radio" | "submit")
                ) && self.signature().attrs.contains_key("checked")
            }
            "option" => self.signature().attrs.contains_key("selected"),
            _ => false,
        }
    }

    pub(in crate::css) fn is_indeterminate(&self) -> bool {
        match self.signature().tag.as_str() {
            // HTML exposes checkbox indeterminacy only through IDL state, but
            // static documents can express `<progress>` indeterminacy by
            // omitting `value`.
            "progress" => !self.signature().attrs.contains_key("value"),
            "input" => {
                matches!(
                    html_form_state::input_type(&self.signature().tag, &self.signature().attrs)
                        .as_deref(),
                    Some("checkbox" | "radio")
                ) && self.signature().attrs.contains_key("indeterminate")
            }
            _ => false,
        }
    }

    pub(in crate::css) fn is_unchecked(&self) -> bool {
        match self.signature().tag.as_str() {
            "input" => {
                matches!(
                    html_form_state::input_type(&self.signature().tag, &self.signature().attrs)
                        .as_deref(),
                    Some("checkbox" | "radio")
                ) && !self.is_checked()
                    && !self.is_indeterminate()
            }
            "option" => !self.is_checked(),
            _ => false,
        }
    }

    pub(in crate::css) fn is_required_capable(&self) -> bool {
        html_form_state::required_capable(&self.signature().tag, &self.signature().attrs)
    }

    pub(in crate::css) fn is_read_write(&self) -> bool {
        html_form_state::read_write(
            &self.signature().tag,
            &self.signature().attrs,
            self.is_disabled(),
        )
    }

    pub(in crate::css) fn is_placeholder_shown(&self) -> bool {
        html_form_state::placeholder_shown(&self.signature().tag, &self.signature().attrs)
    }

    pub(in crate::css) fn is_valid(&self) -> bool {
        self.is_validation_candidate() && !self.is_invalid()
    }

    pub(in crate::css) fn is_invalid(&self) -> bool {
        html_form_state::statically_invalid(
            &self.signature().tag,
            &self.signature().attrs,
            self.is_disabled(),
        )
    }

    pub(in crate::css) fn is_in_range(&self) -> bool {
        html_form_state::numeric_in_range(&self.signature().tag, &self.signature().attrs)
    }

    pub(in crate::css) fn is_out_of_range(&self) -> bool {
        html_form_state::numeric_out_of_range(&self.signature().tag, &self.signature().attrs)
    }

    pub(in crate::css) fn is_validation_candidate(&self) -> bool {
        html_form_state::validation_candidate(
            &self.signature().tag,
            &self.signature().attrs,
            self.is_disabled(),
        )
    }

    /// Match Selectors 4 `:open` for static HTML states visible in markup.
    ///
    /// The pseudo-class represents host-language open/closed state. In static
    /// HTML documents Quire can deterministically derive that state from
    /// boolean `open` attributes on elements whose open state is represented in
    /// markup:
    /// <https://drafts.csswg.org/selectors-4/#open-state> and
    /// <https://html.spec.whatwg.org/multipage/interactive-elements.html#the-details-element>.
    pub(in crate::css) fn is_open(&self) -> bool {
        matches!(self.signature().tag.as_str(), "details" | "dialog")
            && self.signature().attrs.contains_key("open")
    }

    pub(in crate::css) fn disabled_by_fieldset(&self) -> bool {
        let mut ancestor = self.parent_element();
        while let Some(element) = ancestor {
            if element.signature().tag == "fieldset"
                && element.signature().attrs.contains_key("disabled")
                && !self.is_inside_first_legend_of_fieldset(&element)
            {
                return true;
            }
            ancestor = element.parent_element();
        }
        false
    }

    pub(in crate::css) fn is_inside_first_legend_of_fieldset(&self, fieldset: &Self) -> bool {
        let mut ancestor = self.parent_element();
        while let Some(element) = ancestor {
            if element.signature().tag == "legend"
                && element.parent_element().as_ref().is_some_and(|parent| {
                    parent.opaque() == fieldset.opaque()
                        && parent
                            .signature()
                            .children
                            .iter()
                            .find(|child| child.tag == "legend")
                            .is_some_and(|legend| legend.opaque_id == element.signature().opaque_id)
                })
            {
                return true;
            }
            if element.opaque() == fieldset.opaque() {
                return false;
            }
            ancestor = element.parent_element();
        }
        false
    }

    pub(in crate::css) fn is_option_selected(&self) -> bool {
        if self.signature().attrs.contains_key("selected") {
            return true;
        }
        if self.signature().tag != "option" {
            return false;
        }
        let Some(index) = self.signature().sibling_index else {
            return false;
        };
        let any_selected = self
            .signature()
            .sibling_signatures
            .iter()
            .any(|sibling| sibling.tag == "option" && sibling.attrs.contains_key("selected"));
        !any_selected
            && self
                .signature()
                .sibling_signatures
                .iter()
                .position(|sibling| sibling.tag == "option")
                == Some(index)
    }
}

impl SelectorElement for StyleElement<'_> {
    type Impl = QuireSelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(&*self.signature().opaque_id)
    }

    fn parent_element(&self) -> Option<Self> {
        (self.index > 0).then(|| Self {
            chain: Rc::clone(&self.chain),
            index: self.index - 1,
            link_matching: self.link_matching,
        })
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let index = self.signature().sibling_index?;
        if index == 0 {
            return None;
        }
        self.sibling_element_at(index - 1)
    }

    fn next_sibling_element(&self) -> Option<Self> {
        let signature = self.signature();
        let index = signature.sibling_index?;
        if index + 1 >= signature.sibling_signatures.len() {
            return None;
        }
        self.sibling_element_at(index + 1)
    }

    fn first_element_child(&self) -> Option<Self> {
        self.child_element_at(0)
    }

    fn is_html_element_in_html_document(&self) -> bool {
        self.signature().document_is_html
    }

    fn has_local_name(&self, local_name: &CssAtom) -> bool {
        self.signature().tag == local_name.as_str()
    }

    fn has_namespace(&self, ns: &CssAtom) -> bool {
        element_namespace_matches(
            &self.signature().namespace_url,
            self.signature().document_is_html,
            ns.as_str(),
        )
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.signature().tag == other.signature().tag
            && normalized_element_namespace(
                &self.signature().namespace_url,
                self.signature().document_is_html,
            ) == normalized_element_namespace(
                &other.signature().namespace_url,
                other.signature().document_is_html,
            )
    }

    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&CssAtom>,
        local_name: &CssAtom,
        operation: &AttrSelectorOperation<&CssString>,
    ) -> bool {
        match ns {
            NamespaceConstraint::Any => self.signature().namespace_attrs.iter().any(|attr| {
                attr.local_name == local_name.as_str() && operation.eval_str(&attr.value)
            }),
            NamespaceConstraint::Specific(namespace) => {
                self.signature().namespace_attrs.iter().any(|attr| {
                    attr.namespace_url == namespace.as_str()
                        && attr.local_name == local_name.as_str()
                        && operation.eval_str(&attr.value)
                })
            }
        }
    }

    fn match_non_ts_pseudo_class(
        &self,
        pc: &QuirePseudoClass,
        _context: &mut MatchingContext<QuireSelectorImpl>,
    ) -> bool {
        match pc {
            QuirePseudoClass::AnyLink => self.is_link(),
            QuirePseudoClass::Link => self.is_link() && self.link_state() == LinkState::Unvisited,
            QuirePseudoClass::Visited => self.is_link() && self.link_state() == LinkState::Visited,
            QuirePseudoClass::Dir(direction) => self.directionality() == Some(*direction),
            QuirePseudoClass::Lang(ranges) => language_matches_any_range(&self.language(), ranges),
            QuirePseudoClass::StaticFalse(_) => false,
            QuirePseudoClass::Target => self.signature().is_target,
            QuirePseudoClass::TargetWithin => {
                self.signature().is_target || self.signature().has_target_descendant
            }
            // The static renderer's current target is its fragment-navigation
            // target. Relative marker state needs layout geometry and is
            // therefore supplied only once a render-scoped topology exists.
            QuirePseudoClass::TargetCurrent => self.signature().is_target,
            QuirePseudoClass::TargetBefore | QuirePseudoClass::TargetAfter => false,
            QuirePseudoClass::Open => self.is_open(),
            QuirePseudoClass::Defined => true,
            QuirePseudoClass::Enabled => {
                html_form_state::disableable_element(self.signature().tag.as_str())
                    && !self.is_disabled()
            }
            QuirePseudoClass::Disabled => self.is_disabled(),
            QuirePseudoClass::Checked => self.is_checked(),
            QuirePseudoClass::Indeterminate => self.is_indeterminate(),
            QuirePseudoClass::Default => self.is_default(),
            QuirePseudoClass::Unchecked => self.is_unchecked(),
            QuirePseudoClass::PlaceholderShown => self.is_placeholder_shown(),
            QuirePseudoClass::Valid => self.is_valid(),
            QuirePseudoClass::Invalid => self.is_invalid(),
            QuirePseudoClass::InRange => self.is_in_range(),
            QuirePseudoClass::OutOfRange => self.is_out_of_range(),
            QuirePseudoClass::Required => {
                self.is_required_capable() && self.signature().attrs.contains_key("required")
            }
            QuirePseudoClass::Optional => {
                self.is_required_capable() && !self.signature().attrs.contains_key("required")
            }
            QuirePseudoClass::ReadWrite => self.is_read_write(),
            QuirePseudoClass::ReadOnly => !self.is_read_write(),
        }
    }

    fn match_pseudo_element(
        &self,
        _pe: &QuirePseudoElement,
        _context: &mut MatchingContext<QuireSelectorImpl>,
    ) -> bool {
        false
    }

    fn apply_selector_flags(&self, _flags: ElementSelectorFlags) {}

    fn is_link(&self) -> bool {
        matches!(self.signature().tag.as_str(), "a" | "area" | "link")
            && self.signature().attrs.contains_key("href")
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(&self, id: &CssAtom, case_sensitivity: CaseSensitivity) -> bool {
        self.signature()
            .attrs
            .get("id")
            .is_some_and(|value| case_sensitivity.eq(value.as_bytes(), id.as_str().as_bytes()))
    }

    fn has_class(&self, name: &CssAtom, case_sensitivity: CaseSensitivity) -> bool {
        self.signature().attrs.get("class").is_some_and(|value| {
            value.split_whitespace().any(|candidate| {
                case_sensitivity.eq(candidate.as_bytes(), name.as_str().as_bytes())
            })
        })
    }

    fn has_custom_state(&self, _name: &CssAtom) -> bool {
        false
    }

    fn imported_part(&self, _name: &CssAtom) -> Option<CssAtom> {
        None
    }

    fn is_part(&self, _name: &CssAtom) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        !self.signature().has_text_child && self.signature().children.is_empty()
    }

    fn is_root(&self) -> bool {
        self.index == 0
    }

    fn add_element_unique_hashes(&self, _filter: &mut selectors::bloom::BloomFilter) -> bool {
        false
    }
}
