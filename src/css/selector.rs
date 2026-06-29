use super::html_form_state;
use super::types::{Direction, ElementSignature, ResolvedLanguage, ScopeRule};
use cssparser::{
    CowRcStr, Parser as CssParser, SourceLocation, ToCss, serialize_identifier, serialize_string,
};
use precomputed_hash::PrecomputedHash;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::context::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
    SelectorCaches,
};
use selectors::matching::{ElementSelectorFlags, matches_selector_list};
use selectors::parser::{
    NonTSPseudoClass, Parser as SelectorParser, PseudoElement, SelectorImpl, SelectorList,
    SelectorParseErrorKind,
};
use selectors::{Element as SelectorElement, OpaqueElement};
use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

const HTML_NAMESPACE_URL: &str = "http://www.w3.org/1999/xhtml";

fn normalized_element_namespace(namespace_url: &str) -> &str {
    if namespace_url.is_empty() {
        HTML_NAMESPACE_URL
    } else {
        namespace_url
    }
}

fn element_namespace_matches(element_namespace: &str, selector_namespace: &str) -> bool {
    selector_namespace.is_empty()
        || normalized_element_namespace(element_namespace) == selector_namespace
}

/// Matches a style rule selector and returns its Cascade 5 scoped proximity.
///
/// A scoped declaration applies only if each enclosing `@scope` contains the
/// element. The final selector is then matched with the innermost scoping root
/// as Selectors 4 `:scope`. The returned proximity is the ancestor distance
/// from the element to that root; lower distances sort stronger in Cascade 5:
/// <https://www.w3.org/TR/css-cascade-5/#scoped-styles>.
pub(super) fn selector_matches_with_scope_proximity(
    selector: &SelectorList<ReasySelectorImpl>,
    scopes: &[ScopeRule],
    current: &ElementSignature,
    ancestors: &[ElementSignature],
) -> Option<usize> {
    let chain = selector_chain(current, ancestors);
    if scopes.is_empty() {
        return selector_matches_at(selector, &chain, ancestors.len(), None).then_some(usize::MAX);
    }
    let mut proximity = usize::MAX;
    let mut scope_root_index = None;
    for scope in scopes {
        let (root_index, distance) = scope_rule_distance(scope, &chain, ancestors.len())?;
        scope_root_index = Some(root_index);
        proximity = distance;
    }
    selector_matches_at(selector, &chain, ancestors.len(), scope_root_index).then_some(proximity)
}

fn selector_chain<'a>(
    current: &'a ElementSignature,
    ancestors: &'a [ElementSignature],
) -> Arc<Vec<Cow<'a, ElementSignature>>> {
    let mut chain = Vec::with_capacity(ancestors.len() + 1);
    chain.extend(ancestors.iter().map(Cow::Borrowed));
    chain.push(Cow::Borrowed(current));
    Arc::new(chain)
}

fn selector_matches_at<'a>(
    selector: &SelectorList<ReasySelectorImpl>,
    chain: &Arc<Vec<Cow<'a, ElementSignature>>>,
    index: usize,
    scope_index: Option<usize>,
) -> bool {
    let element = StyleElement {
        chain: Arc::clone(chain),
        index,
    };
    let scope_element = scope_index.map(|index| OpaqueElement::new(&*chain[index].opaque_id));
    let mut caches = SelectorCaches::default();
    let mut context = MatchingContext::new(
        MatchingMode::Normal,
        None,
        &mut caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    context.scope_element = scope_element;
    matches_selector_list(selector, &element, &mut context)
}

fn scope_rule_distance<'a>(
    scope: &ScopeRule,
    chain: &Arc<Vec<Cow<'a, ElementSignature>>>,
    current_index: usize,
) -> Option<(usize, usize)> {
    for root_index in (0..=current_index).rev() {
        if !selector_matches_at(&scope.root, chain, root_index, None) {
            continue;
        }
        if let Some(limit) = &scope.limit
            && (root_index + 1..=current_index)
                .any(|index| selector_matches_at(limit, chain, index, None))
        {
            continue;
        }
        return Some((root_index, current_index - root_index));
    }
    None
}

#[derive(Clone, Debug)]
struct StyleElement<'a> {
    chain: Arc<Vec<Cow<'a, ElementSignature>>>,
    index: usize,
}

impl StyleElement<'_> {
    fn signature(&self) -> &ElementSignature {
        &self.chain[self.index]
    }

    fn sibling_element_at(&self, sibling_index: usize) -> Option<Self> {
        let sibling = self.signature().sibling_at(sibling_index)?;
        let mut chain = self.chain[..self.index].to_vec();
        chain.push(Cow::Owned(sibling));
        Some(Self {
            chain: Arc::new(chain),
            index: self.index,
        })
    }

    fn child_element_at(&self, child_index: usize) -> Option<Self> {
        let child = self.signature().child_at(child_index)?;
        let mut chain = self.chain.as_ref().clone();
        chain.push(Cow::Owned(child));
        let index = chain.len() - 1;
        Some(Self {
            chain: Arc::new(chain),
            index,
        })
    }

    fn directionality(&self) -> Option<Direction> {
        self.signature()
            .html_direction
            .or(self.signature().resolved_direction)
            .or_else(|| {
                self.signature().attrs.get("dir").and_then(|value| {
                    match value.trim().to_ascii_lowercase().as_str() {
                        "ltr" => Some(Direction::Ltr),
                        "rtl" => Some(Direction::Rtl),
                        _ => None,
                    }
                })
            })
    }

    fn language(&self) -> ResolvedLanguage {
        if self.signature().resolved_language != ResolvedLanguage::Unresolved {
            return self.signature().resolved_language.clone();
        }
        if let Some(language) = language_from_attrs(&self.signature().attrs) {
            return language;
        }
        self.parent_element()
            .map(|parent| parent.language())
            .unwrap_or(ResolvedLanguage::Unknown)
    }

    fn is_disabled(&self) -> bool {
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

    fn is_checked(&self) -> bool {
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

    fn is_default(&self) -> bool {
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

    fn is_indeterminate(&self) -> bool {
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

    fn is_unchecked(&self) -> bool {
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

    fn is_required_capable(&self) -> bool {
        html_form_state::required_capable(&self.signature().tag, &self.signature().attrs)
    }

    fn is_read_write(&self) -> bool {
        html_form_state::read_write(
            &self.signature().tag,
            &self.signature().attrs,
            self.is_disabled(),
        )
    }

    fn is_placeholder_shown(&self) -> bool {
        html_form_state::placeholder_shown(&self.signature().tag, &self.signature().attrs)
    }

    fn is_valid(&self) -> bool {
        self.is_validation_candidate() && !self.is_invalid()
    }

    fn is_invalid(&self) -> bool {
        html_form_state::statically_invalid(
            &self.signature().tag,
            &self.signature().attrs,
            self.is_disabled(),
        )
    }

    fn is_in_range(&self) -> bool {
        html_form_state::numeric_in_range(&self.signature().tag, &self.signature().attrs)
    }

    fn is_out_of_range(&self) -> bool {
        html_form_state::numeric_out_of_range(&self.signature().tag, &self.signature().attrs)
    }

    fn is_validation_candidate(&self) -> bool {
        html_form_state::validation_candidate(
            &self.signature().tag,
            &self.signature().attrs,
            self.is_disabled(),
        )
    }

    fn disabled_by_fieldset(&self) -> bool {
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

    fn is_inside_first_legend_of_fieldset(&self, fieldset: &Self) -> bool {
        let mut ancestor = self.parent_element();
        while let Some(element) = ancestor {
            if element.signature().tag == "legend"
                && element.parent_element().as_ref().is_some_and(|parent| {
                    parent.opaque() == fieldset.opaque()
                        && parent
                            .signature()
                            .child_signatures
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

    fn is_option_selected(&self) -> bool {
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

fn language_from_attrs(
    attrs: &std::collections::HashMap<String, String>,
) -> Option<ResolvedLanguage> {
    attrs
        .get("lang")
        .or_else(|| attrs.get("xml:lang"))
        .map(|value| ResolvedLanguage::from_html_attribute(value))
}

/// Match Selectors `:lang()` language ranges using RFC 4647 extended filtering.
///
/// Selectors Level 4 defines `:lang()` in terms of an element's document
/// language and BCP 47 language ranges, while RFC 4647 defines extended
/// filtering with wildcard subtags:
/// <https://www.w3.org/TR/selectors-4/#the-lang-pseudo> and
/// <https://www.rfc-editor.org/rfc/rfc4647#section-3.3.2>.
fn language_matches_any_range(language: &ResolvedLanguage, ranges: &[LanguageRange]) -> bool {
    match language {
        ResolvedLanguage::Unknown | ResolvedLanguage::Unresolved => {
            ranges.iter().any(|range| range.as_str().is_empty())
        }
        ResolvedLanguage::Tag(tag) => ranges
            .iter()
            .any(|range| extended_language_range_matches(tag, range.as_str())),
    }
}

fn extended_language_range_matches(tag: &str, range: &str) -> bool {
    if range.is_empty() {
        return false;
    }
    let tag = tag.trim().to_ascii_lowercase();
    if !is_valid_language_tag(&tag) {
        return false;
    }
    let range_parts: Vec<&str> = range.split('-').collect();
    let tag_parts: Vec<&str> = tag.split('-').collect();
    let Some(first_range) = range_parts.first() else {
        return false;
    };
    if *first_range != "*" && *first_range != tag_parts[0] {
        return false;
    }

    let mut tag_index = 1usize;
    for range_part in range_parts.iter().skip(1) {
        if *range_part == "*" {
            continue;
        }
        loop {
            let Some(tag_part) = tag_parts.get(tag_index) else {
                return false;
            };
            if range_part == tag_part {
                tag_index += 1;
                break;
            }
            if tag_part.len() == 1 {
                return false;
            }
            tag_index += 1;
        }
    }
    true
}

fn is_valid_extended_language_range(range: &str) -> bool {
    let mut parts = range.split('-');
    let Some(first) = parts.next() else {
        return false;
    };
    is_wildcard_or_language_range_subtag(first) && parts.all(is_wildcard_or_language_range_subtag)
}

fn is_valid_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let Some(first) = parts.next() else {
        return false;
    };
    is_language_range_subtag(first) && parts.all(is_language_range_subtag)
}

fn is_wildcard_or_language_range_subtag(value: &str) -> bool {
    value == "*" || is_language_range_subtag(value)
}

fn is_language_range_subtag(value: &str) -> bool {
    !value.is_empty() && value.len() <= 8 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

impl SelectorElement for StyleElement<'_> {
    type Impl = ReasySelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(&*self.signature().opaque_id)
    }

    fn parent_element(&self) -> Option<Self> {
        (self.index > 0).then(|| Self {
            chain: Arc::clone(&self.chain),
            index: self.index - 1,
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
        true
    }

    fn has_local_name(&self, local_name: &CssAtom) -> bool {
        self.signature().tag == local_name.as_str()
    }

    fn has_namespace(&self, ns: &CssAtom) -> bool {
        element_namespace_matches(&self.signature().namespace_url, ns.as_str())
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.signature().tag == other.signature().tag
            && normalized_element_namespace(&self.signature().namespace_url)
                == normalized_element_namespace(&other.signature().namespace_url)
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
        pc: &ReasyPseudoClass,
        _context: &mut MatchingContext<ReasySelectorImpl>,
    ) -> bool {
        match pc {
            ReasyPseudoClass::AnyLink | ReasyPseudoClass::Link => self.is_link(),
            ReasyPseudoClass::Dir(direction) => self.directionality() == Some(*direction),
            ReasyPseudoClass::Lang(ranges) => language_matches_any_range(&self.language(), ranges),
            ReasyPseudoClass::StaticFalse(_) => false,
            ReasyPseudoClass::Target => self.signature().is_target,
            ReasyPseudoClass::TargetWithin => {
                self.signature().is_target || self.signature().has_target_descendant
            }
            ReasyPseudoClass::Defined => true,
            ReasyPseudoClass::Enabled => {
                html_form_state::disableable_element(self.signature().tag.as_str())
                    && !self.is_disabled()
            }
            ReasyPseudoClass::Disabled => self.is_disabled(),
            ReasyPseudoClass::Checked => self.is_checked(),
            ReasyPseudoClass::Indeterminate => self.is_indeterminate(),
            ReasyPseudoClass::Default => self.is_default(),
            ReasyPseudoClass::Unchecked => self.is_unchecked(),
            ReasyPseudoClass::PlaceholderShown => self.is_placeholder_shown(),
            ReasyPseudoClass::Valid => self.is_valid(),
            ReasyPseudoClass::Invalid => self.is_invalid(),
            ReasyPseudoClass::InRange => self.is_in_range(),
            ReasyPseudoClass::OutOfRange => self.is_out_of_range(),
            ReasyPseudoClass::Required => {
                self.is_required_capable() && self.signature().attrs.contains_key("required")
            }
            ReasyPseudoClass::Optional => {
                self.is_required_capable() && !self.signature().attrs.contains_key("required")
            }
            ReasyPseudoClass::ReadWrite => self.is_read_write(),
            ReasyPseudoClass::ReadOnly => !self.is_read_write(),
        }
    }

    fn match_pseudo_element(
        &self,
        _pe: &ReasyPseudoElement,
        _context: &mut MatchingContext<ReasySelectorImpl>,
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
        !self.signature().has_text_child && self.signature().child_signatures.is_empty()
    }

    fn is_root(&self) -> bool {
        self.index == 0
    }

    fn add_element_unique_hashes(&self, _filter: &mut selectors::bloom::BloomFilter) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReasySelectorImpl;

impl SelectorImpl for ReasySelectorImpl {
    type ExtraMatchingData<'a> = ();
    type AttrValue = CssString;
    type Identifier = CssAtom;
    type LocalName = CssAtom;
    type NamespaceUrl = CssAtom;
    type NamespacePrefix = CssAtom;
    type BorrowedLocalName = CssAtom;
    type BorrowedNamespaceUrl = CssAtom;
    type NonTSPseudoClass = ReasyPseudoClass;
    type PseudoElement = ReasyPseudoElement;
}

#[derive(Debug, Clone, Default)]
pub(super) struct ReasySelectorParser {
    default_namespace: Option<CssAtom>,
    namespaces: HashMap<String, CssAtom>,
}

impl ReasySelectorParser {
    pub(super) fn new(
        default_namespace: Option<String>,
        namespaces: HashMap<String, String>,
    ) -> Self {
        Self {
            default_namespace: default_namespace.map(CssAtom),
            namespaces: namespaces
                .into_iter()
                .map(|(prefix, namespace)| (prefix, CssAtom(namespace)))
                .collect(),
        }
    }
}

impl<'i> SelectorParser<'i> for ReasySelectorParser {
    type Impl = ReasySelectorImpl;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_is_and_where(&self) -> bool {
        true
    }

    /// Parse Selectors 4 child-indexed pseudo-class filters:
    /// `:nth-child(An+B of S)` and `:nth-last-child(An+B of S)`.
    ///
    /// <https://www.w3.org/TR/selectors-4/#child-index>
    fn parse_nth_child_of(&self) -> bool {
        true
    }

    fn parse_has(&self) -> bool {
        true
    }

    fn default_namespace(&self) -> Option<CssAtom> {
        self.default_namespace.clone()
    }

    fn namespace_for_prefix(&self, prefix: &CssAtom) -> Option<CssAtom> {
        self.namespaces.get(prefix.as_str()).cloned()
    }

    fn parse_non_ts_pseudo_class(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<ReasyPseudoClass, cssparser::ParseError<'i, Self::Error>> {
        match name.as_ref().to_ascii_lowercase().as_str() {
            "link" => Ok(ReasyPseudoClass::Link),
            "any-link" => Ok(ReasyPseudoClass::AnyLink),
            "visited" => Ok(ReasyPseudoClass::StaticFalse("visited")),
            "target" => Ok(ReasyPseudoClass::Target),
            "target-within" => Ok(ReasyPseudoClass::TargetWithin),
            "hover" => Ok(ReasyPseudoClass::StaticFalse("hover")),
            "active" => Ok(ReasyPseudoClass::StaticFalse("active")),
            "focus" => Ok(ReasyPseudoClass::StaticFalse("focus")),
            "focus-visible" => Ok(ReasyPseudoClass::StaticFalse("focus-visible")),
            "focus-within" => Ok(ReasyPseudoClass::StaticFalse("focus-within")),
            "playing" => Ok(ReasyPseudoClass::StaticFalse("playing")),
            "paused" => Ok(ReasyPseudoClass::StaticFalse("paused")),
            "seeking" => Ok(ReasyPseudoClass::StaticFalse("seeking")),
            "buffering" => Ok(ReasyPseudoClass::StaticFalse("buffering")),
            "stalled" => Ok(ReasyPseudoClass::StaticFalse("stalled")),
            "muted" => Ok(ReasyPseudoClass::StaticFalse("muted")),
            "volume-locked" => Ok(ReasyPseudoClass::StaticFalse("volume-locked")),
            "open" => Ok(ReasyPseudoClass::StaticFalse("open")),
            "popover-open" => Ok(ReasyPseudoClass::StaticFalse("popover-open")),
            "modal" => Ok(ReasyPseudoClass::StaticFalse("modal")),
            "fullscreen" => Ok(ReasyPseudoClass::StaticFalse("fullscreen")),
            "picture-in-picture" => Ok(ReasyPseudoClass::StaticFalse("picture-in-picture")),
            "autofill" => Ok(ReasyPseudoClass::StaticFalse("autofill")),
            "default" => Ok(ReasyPseudoClass::Default),
            "unchecked" => Ok(ReasyPseudoClass::Unchecked),
            "placeholder-shown" => Ok(ReasyPseudoClass::PlaceholderShown),
            "valid" => Ok(ReasyPseudoClass::Valid),
            "invalid" => Ok(ReasyPseudoClass::Invalid),
            "in-range" => Ok(ReasyPseudoClass::InRange),
            "out-of-range" => Ok(ReasyPseudoClass::OutOfRange),
            "user-valid" => Ok(ReasyPseudoClass::StaticFalse("user-valid")),
            "user-invalid" => Ok(ReasyPseudoClass::StaticFalse("user-invalid")),
            "defined" => Ok(ReasyPseudoClass::Defined),
            "enabled" => Ok(ReasyPseudoClass::Enabled),
            "disabled" => Ok(ReasyPseudoClass::Disabled),
            "checked" => Ok(ReasyPseudoClass::Checked),
            "indeterminate" => Ok(ReasyPseudoClass::Indeterminate),
            "required" => Ok(ReasyPseudoClass::Required),
            "optional" => Ok(ReasyPseudoClass::Optional),
            "read-write" => Ok(ReasyPseudoClass::ReadWrite),
            "read-only" => Ok(ReasyPseudoClass::ReadOnly),
            _ => Err(location.new_custom_error(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
            )),
        }
    }

    fn parse_non_ts_functional_pseudo_class<'t>(
        &self,
        name: CowRcStr<'i>,
        parser: &mut CssParser<'i, 't>,
        _after_part: bool,
    ) -> Result<ReasyPseudoClass, cssparser::ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("lang") {
            let ranges = parser.parse_comma_separated(|parser| {
                let argument = if let Ok(argument) = parser.try_parse(|parser| {
                    parser
                        .expect_ident_or_string()
                        .map(|value| value.as_ref().to_string())
                }) {
                    argument
                } else {
                    parser.expect_delim('*')?;
                    "*".to_string()
                };
                LanguageRange::parse(&argument).ok_or_else(|| {
                    parser.new_custom_error(
                        SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name.clone()),
                    )
                })
            })?;
            parser.expect_exhausted()?;
            if ranges.is_empty() {
                return Err(parser.new_custom_error(
                    SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
                ));
            }
            return Ok(ReasyPseudoClass::Lang(ranges));
        }
        if !name.eq_ignore_ascii_case("dir") {
            return Err(parser.new_custom_error(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
            ));
        }
        let direction = {
            let argument = parser.expect_ident()?;
            match argument.as_ref().to_ascii_lowercase().as_str() {
                "ltr" => Direction::Ltr,
                "rtl" => Direction::Rtl,
                _ => {
                    return Err(parser.new_custom_error(
                        SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
                    ));
                }
            }
        };
        parser.expect_exhausted()?;
        Ok(ReasyPseudoClass::Dir(direction))
    }

    fn parse_pseudo_element(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<ReasyPseudoElement, cssparser::ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("before") {
            Ok(ReasyPseudoElement::Before)
        } else if name.eq_ignore_ascii_case("after") {
            Ok(ReasyPseudoElement::After)
        } else if name.eq_ignore_ascii_case("marker") {
            Ok(ReasyPseudoElement::Marker)
        } else if name.eq_ignore_ascii_case("first-line") {
            Ok(ReasyPseudoElement::FirstLine)
        } else if name.eq_ignore_ascii_case("first-letter") {
            Ok(ReasyPseudoElement::FirstLetter)
        } else {
            Err(
                location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClassOrElement(
                    name,
                )),
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReasyPseudoClass {
    Link,
    AnyLink,
    Dir(Direction),
    Lang(Vec<LanguageRange>),
    StaticFalse(&'static str),
    Target,
    TargetWithin,
    Defined,
    Enabled,
    Disabled,
    Checked,
    Indeterminate,
    Default,
    Unchecked,
    PlaceholderShown,
    Valid,
    Invalid,
    InRange,
    OutOfRange,
    Required,
    Optional,
    ReadWrite,
    ReadOnly,
}

impl ToCss for ReasyPseudoClass {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        match self {
            ReasyPseudoClass::Link => dest.write_str(":link"),
            ReasyPseudoClass::AnyLink => dest.write_str(":any-link"),
            ReasyPseudoClass::Dir(Direction::Ltr) => dest.write_str(":dir(ltr)"),
            ReasyPseudoClass::Dir(Direction::Rtl) => dest.write_str(":dir(rtl)"),
            ReasyPseudoClass::StaticFalse(name) => write!(dest, ":{name}"),
            ReasyPseudoClass::Target => dest.write_str(":target"),
            ReasyPseudoClass::TargetWithin => dest.write_str(":target-within"),
            ReasyPseudoClass::Defined => dest.write_str(":defined"),
            ReasyPseudoClass::Enabled => dest.write_str(":enabled"),
            ReasyPseudoClass::Disabled => dest.write_str(":disabled"),
            ReasyPseudoClass::Checked => dest.write_str(":checked"),
            ReasyPseudoClass::Indeterminate => dest.write_str(":indeterminate"),
            ReasyPseudoClass::Default => dest.write_str(":default"),
            ReasyPseudoClass::Unchecked => dest.write_str(":unchecked"),
            ReasyPseudoClass::PlaceholderShown => dest.write_str(":placeholder-shown"),
            ReasyPseudoClass::Valid => dest.write_str(":valid"),
            ReasyPseudoClass::Invalid => dest.write_str(":invalid"),
            ReasyPseudoClass::InRange => dest.write_str(":in-range"),
            ReasyPseudoClass::OutOfRange => dest.write_str(":out-of-range"),
            ReasyPseudoClass::Required => dest.write_str(":required"),
            ReasyPseudoClass::Optional => dest.write_str(":optional"),
            ReasyPseudoClass::ReadWrite => dest.write_str(":read-write"),
            ReasyPseudoClass::ReadOnly => dest.write_str(":read-only"),
            ReasyPseudoClass::Lang(ranges) => {
                dest.write_str(":lang(")?;
                for (index, range) in ranges.iter().enumerate() {
                    if index > 0 {
                        dest.write_str(", ")?;
                    }
                    range.to_css(dest)?;
                }
                dest.write_char(')')
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanguageRange(String);

impl LanguageRange {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || is_valid_extended_language_range(value) {
            Some(Self(value.to_ascii_lowercase()))
        } else {
            None
        }
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl ToCss for LanguageRange {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        if self.0.is_empty() || self.0.contains('*') {
            serialize_string(&self.0, dest)
        } else {
            serialize_identifier(&self.0, dest)
        }
    }
}

impl NonTSPseudoClass for ReasyPseudoClass {
    type Impl = ReasySelectorImpl;

    fn is_active_or_hover(&self) -> bool {
        false
    }

    fn is_user_action_state(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReasyPseudoElement {
    Before,
    After,
    Marker,
    FirstLine,
    FirstLetter,
}

impl ToCss for ReasyPseudoElement {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        dest.write_str(match self {
            ReasyPseudoElement::Before => "::before",
            ReasyPseudoElement::After => "::after",
            ReasyPseudoElement::Marker => "::marker",
            ReasyPseudoElement::FirstLine => "::first-line",
            ReasyPseudoElement::FirstLetter => "::first-letter",
        })
    }
}

impl PseudoElement for ReasyPseudoElement {
    type Impl = ReasySelectorImpl;
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct CssAtom(String);

impl CssAtom {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CssAtom {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl ToCss for CssAtom {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        serialize_identifier(&self.0, dest)
    }
}

impl PrecomputedHash for CssAtom {
    fn precomputed_hash(&self) -> u32 {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        hasher.finish() as u32
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct CssString(String);

impl AsRef<str> for CssString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CssString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl ToCss for CssString {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        dest.write_char('"')?;
        write!(cssparser::CssStringWriter::new(dest), "{}", self.0)?;
        dest.write_char('"')
    }
}
