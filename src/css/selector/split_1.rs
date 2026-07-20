use super::*;

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

/// Matches a style rule selector against a prebuilt selector chain.
///
/// A scoped declaration applies only if each enclosing `@scope` contains the
/// element. The final selector is then matched with the innermost scoping root
/// as Selectors 4 `:scope`. The returned proximity is the ancestor distance
/// from the element to that root; lower distances sort stronger in Cascade 5:
/// <https://www.w3.org/TR/css-cascade-5/#scoped-styles>.
pub(in crate::css) fn selector_matches_with_scope_proximity_in_chain<'a>(
    selector: &SelectorList<QuireSelectorImpl>,
    scopes: &[ScopeRule],
    chain: &Rc<Vec<Cow<'a, ElementSignature>>>,
    current_index: usize,
    caches: &mut SelectorCaches,
) -> Option<(usize, u32)> {
    if scopes.is_empty() {
        return selector_matching_specificity_at(selector, chain, current_index, None, caches)
            .map(|specificity| (usize::MAX, specificity));
    }
    let mut proximity = usize::MAX;
    let mut scope_root_index = None;
    for scope in scopes {
        let (root_index, distance) = scope_rule_distance(scope, chain, current_index, caches)?;
        scope_root_index = Some(root_index);
        proximity = distance;
    }
    selector_matching_specificity_at(selector, chain, current_index, scope_root_index, caches)
        .map(|specificity| (proximity, specificity))
}

pub(in crate::css) fn selector_chain<'a>(
    current: &'a ElementSignature,
    ancestors: &'a [ElementSignature],
) -> Rc<Vec<Cow<'a, ElementSignature>>> {
    let mut chain = Vec::with_capacity(ancestors.len() + 1);
    chain.extend(ancestors.iter().map(Cow::Borrowed));
    chain.push(Cow::Borrowed(current));
    Rc::new(chain)
}

pub(in crate::css) fn selector_matches_at<'a>(
    selector: &SelectorList<QuireSelectorImpl>,
    chain: &Rc<Vec<Cow<'a, ElementSignature>>>,
    index: usize,
    scope_index: Option<usize>,
    caches: &mut SelectorCaches,
) -> bool {
    let element = StyleElement {
        chain: Rc::clone(chain),
        index,
    };
    let scope_element = scope_index.map(|index| OpaqueElement::new(&*chain[index].opaque_id));
    let mut context = MatchingContext::new(
        MatchingMode::Normal,
        None,
        caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    context.scope_element = scope_element;
    matches_selector_list(selector, &element, &mut context)
}

/// Return the specificity of the matching branch of a selector list.
///
/// CSS selector lists are an `:is()`-like disjunction only for matching; the
/// cascade uses the specificity of the particular selector that matched, not
/// the maximum specificity declared elsewhere in the comma-separated list.
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>
fn selector_matching_specificity_at<'a>(
    selector: &SelectorList<QuireSelectorImpl>,
    chain: &Rc<Vec<Cow<'a, ElementSignature>>>,
    index: usize,
    scope_index: Option<usize>,
    caches: &mut SelectorCaches,
) -> Option<u32> {
    let element = StyleElement {
        chain: Rc::clone(chain),
        index,
    };
    let scope_element = scope_index.map(|index| OpaqueElement::new(&*chain[index].opaque_id));
    let mut context = MatchingContext::new(
        MatchingMode::Normal,
        None,
        caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    context.scope_element = scope_element;
    selector
        .slice()
        .iter()
        .filter(|branch| matches_selector(*branch, 0, None, &element, &mut context))
        .map(|branch| branch.specificity())
        .max()
}

pub(in crate::css) fn scope_rule_distance<'a>(
    scope: &ScopeRule,
    chain: &Rc<Vec<Cow<'a, ElementSignature>>>,
    current_index: usize,
    caches: &mut SelectorCaches,
) -> Option<(usize, usize)> {
    for root_index in (0..=current_index).rev() {
        if !selector_matches_at(&scope.root, chain, root_index, None, caches) {
            continue;
        }
        if let Some(limit) = &scope.limit
            && (root_index + 1..=current_index)
                .any(|index| selector_matches_at(limit, chain, index, None, caches))
        {
            continue;
        }
        return Some((root_index, current_index - root_index));
    }
    None
}

#[derive(Clone, Debug)]
pub(in crate::css) struct StyleElement<'a> {
    pub(in crate::css) chain: Rc<Vec<Cow<'a, ElementSignature>>>,
    pub(in crate::css) index: usize,
}

impl StyleElement<'_> {
    pub(in crate::css) fn signature(&self) -> &ElementSignature {
        &self.chain[self.index]
    }

    pub(in crate::css) fn sibling_element_at(&self, sibling_index: usize) -> Option<Self> {
        let sibling = self.signature().sibling_at(sibling_index)?;
        let mut chain = self.chain[..self.index].to_vec();
        chain.push(Cow::Owned(sibling));
        Some(Self {
            chain: Rc::new(chain),
            index: self.index,
        })
    }

    pub(in crate::css) fn child_element_at(&self, child_index: usize) -> Option<Self> {
        let child = self.signature().child_at(child_index)?;
        let mut chain = self.chain.as_ref().clone();
        chain.push(Cow::Owned(child));
        let index = chain.len() - 1;
        Some(Self {
            chain: Rc::new(chain),
            index,
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
            .unwrap_or(ResolvedLanguage::Unknown)
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

pub(in crate::css) fn language_from_attrs(
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
pub(in crate::css) fn language_matches_any_range(
    language: &ResolvedLanguage,
    ranges: &[LanguageRange],
) -> bool {
    match language {
        ResolvedLanguage::Unknown | ResolvedLanguage::Unresolved => {
            ranges.iter().any(|range| range.as_str().is_empty())
        }
        ResolvedLanguage::Tag(tag) => ranges
            .iter()
            .any(|range| extended_language_range_matches(tag, range.as_str())),
    }
}

pub(in crate::css) fn extended_language_range_matches(tag: &str, range: &str) -> bool {
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

pub(in crate::css) fn is_valid_extended_language_range(range: &str) -> bool {
    let mut parts = range.split('-');
    let Some(first) = parts.next() else {
        return false;
    };
    is_wildcard_or_language_range_subtag(first) && parts.all(is_wildcard_or_language_range_subtag)
}

pub(in crate::css) fn is_valid_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let Some(first) = parts.next() else {
        return false;
    };
    is_language_range_subtag(first) && parts.all(is_language_range_subtag)
}

pub(in crate::css) fn is_wildcard_or_language_range_subtag(value: &str) -> bool {
    value == "*" || is_language_range_subtag(value)
}

pub(in crate::css) fn is_language_range_subtag(value: &str) -> bool {
    !value.is_empty() && value.len() <= 8 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
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
            QuirePseudoClass::AnyLink | QuirePseudoClass::Link | QuirePseudoClass::Visited => {
                self.is_link()
            }
            QuirePseudoClass::Dir(direction) => self.directionality() == Some(*direction),
            QuirePseudoClass::Lang(ranges) => language_matches_any_range(&self.language(), ranges),
            QuirePseudoClass::StaticFalse(_) => false,
            QuirePseudoClass::Target => self.signature().is_target,
            QuirePseudoClass::TargetWithin => {
                self.signature().is_target || self.signature().has_target_descendant
            }
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
pub(crate) struct QuireSelectorImpl;

impl SelectorImpl for QuireSelectorImpl {
    type ExtraMatchingData<'a> = ();
    type AttrValue = CssString;
    type Identifier = CssAtom;
    type LocalName = CssAtom;
    type NamespaceUrl = CssAtom;
    type NamespacePrefix = CssAtom;
    type BorrowedLocalName = CssAtom;
    type BorrowedNamespaceUrl = CssAtom;
    type NonTSPseudoClass = QuirePseudoClass;
    type PseudoElement = QuirePseudoElement;
}

#[derive(Debug, Clone, Default)]
pub(in crate::css) struct QuireSelectorParser {
    pub(in crate::css) default_namespace: Option<CssAtom>,
    pub(in crate::css) namespaces: HashMap<String, CssAtom>,
}

impl QuireSelectorParser {
    pub(in crate::css) fn new(
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

impl<'i> SelectorParser<'i> for QuireSelectorParser {
    type Impl = QuireSelectorImpl;
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
    ) -> Result<QuirePseudoClass, cssparser::ParseError<'i, Self::Error>> {
        match name.as_ref().to_ascii_lowercase().as_str() {
            "link" => Ok(QuirePseudoClass::Link),
            "any-link" => Ok(QuirePseudoClass::AnyLink),
            // A static document renderer has no browser history. Treat links
            // as visited so the print rendering environment has a stable
            // visited-link used style rather than depending on host history.
            "visited" => Ok(QuirePseudoClass::Visited),
            "target" => Ok(QuirePseudoClass::Target),
            "target-within" => Ok(QuirePseudoClass::TargetWithin),
            "hover" => Ok(QuirePseudoClass::StaticFalse("hover")),
            "active" => Ok(QuirePseudoClass::StaticFalse("active")),
            "focus" => Ok(QuirePseudoClass::StaticFalse("focus")),
            "focus-visible" => Ok(QuirePseudoClass::StaticFalse("focus-visible")),
            "focus-within" => Ok(QuirePseudoClass::StaticFalse("focus-within")),
            "playing" => Ok(QuirePseudoClass::StaticFalse("playing")),
            "paused" => Ok(QuirePseudoClass::StaticFalse("paused")),
            "seeking" => Ok(QuirePseudoClass::StaticFalse("seeking")),
            "buffering" => Ok(QuirePseudoClass::StaticFalse("buffering")),
            "stalled" => Ok(QuirePseudoClass::StaticFalse("stalled")),
            "muted" => Ok(QuirePseudoClass::StaticFalse("muted")),
            "volume-locked" => Ok(QuirePseudoClass::StaticFalse("volume-locked")),
            "open" => Ok(QuirePseudoClass::Open),
            "popover-open" => Ok(QuirePseudoClass::StaticFalse("popover-open")),
            "modal" => Ok(QuirePseudoClass::StaticFalse("modal")),
            "fullscreen" => Ok(QuirePseudoClass::StaticFalse("fullscreen")),
            "picture-in-picture" => Ok(QuirePseudoClass::StaticFalse("picture-in-picture")),
            "autofill" => Ok(QuirePseudoClass::StaticFalse("autofill")),
            "default" => Ok(QuirePseudoClass::Default),
            "unchecked" => Ok(QuirePseudoClass::Unchecked),
            "placeholder-shown" => Ok(QuirePseudoClass::PlaceholderShown),
            "valid" => Ok(QuirePseudoClass::Valid),
            "invalid" => Ok(QuirePseudoClass::Invalid),
            "in-range" => Ok(QuirePseudoClass::InRange),
            "out-of-range" => Ok(QuirePseudoClass::OutOfRange),
            "user-valid" => Ok(QuirePseudoClass::StaticFalse("user-valid")),
            "user-invalid" => Ok(QuirePseudoClass::StaticFalse("user-invalid")),
            "defined" => Ok(QuirePseudoClass::Defined),
            "enabled" => Ok(QuirePseudoClass::Enabled),
            "disabled" => Ok(QuirePseudoClass::Disabled),
            "checked" => Ok(QuirePseudoClass::Checked),
            "indeterminate" => Ok(QuirePseudoClass::Indeterminate),
            "required" => Ok(QuirePseudoClass::Required),
            "optional" => Ok(QuirePseudoClass::Optional),
            "read-write" => Ok(QuirePseudoClass::ReadWrite),
            "read-only" => Ok(QuirePseudoClass::ReadOnly),
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
    ) -> Result<QuirePseudoClass, cssparser::ParseError<'i, Self::Error>> {
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
            return Ok(QuirePseudoClass::Lang(ranges));
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
        Ok(QuirePseudoClass::Dir(direction))
    }

    fn parse_pseudo_element(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<QuirePseudoElement, cssparser::ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("before") {
            Ok(QuirePseudoElement::Before)
        } else if name.eq_ignore_ascii_case("after") {
            Ok(QuirePseudoElement::After)
        } else if name.eq_ignore_ascii_case("footnote-call") {
            Ok(QuirePseudoElement::FootnoteCall)
        } else if name.eq_ignore_ascii_case("footnote-marker") {
            Ok(QuirePseudoElement::FootnoteMarker)
        } else if name.eq_ignore_ascii_case("marker") {
            Ok(QuirePseudoElement::Marker)
        } else if name.eq_ignore_ascii_case("first-line") {
            Ok(QuirePseudoElement::FirstLine)
        } else if name.eq_ignore_ascii_case("first-letter") {
            Ok(QuirePseudoElement::FirstLetter)
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
pub(crate) enum QuirePseudoClass {
    Link,
    AnyLink,
    Visited,
    Dir(Direction),
    Lang(Vec<LanguageRange>),
    StaticFalse(&'static str),
    Target,
    TargetWithin,
    Open,
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

impl ToCss for QuirePseudoClass {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        match self {
            QuirePseudoClass::Link => dest.write_str(":link"),
            QuirePseudoClass::AnyLink => dest.write_str(":any-link"),
            QuirePseudoClass::Visited => dest.write_str(":visited"),
            QuirePseudoClass::Dir(Direction::Ltr) => dest.write_str(":dir(ltr)"),
            QuirePseudoClass::Dir(Direction::Rtl) => dest.write_str(":dir(rtl)"),
            QuirePseudoClass::StaticFalse(name) => write!(dest, ":{name}"),
            QuirePseudoClass::Target => dest.write_str(":target"),
            QuirePseudoClass::TargetWithin => dest.write_str(":target-within"),
            QuirePseudoClass::Open => dest.write_str(":open"),
            QuirePseudoClass::Defined => dest.write_str(":defined"),
            QuirePseudoClass::Enabled => dest.write_str(":enabled"),
            QuirePseudoClass::Disabled => dest.write_str(":disabled"),
            QuirePseudoClass::Checked => dest.write_str(":checked"),
            QuirePseudoClass::Indeterminate => dest.write_str(":indeterminate"),
            QuirePseudoClass::Default => dest.write_str(":default"),
            QuirePseudoClass::Unchecked => dest.write_str(":unchecked"),
            QuirePseudoClass::PlaceholderShown => dest.write_str(":placeholder-shown"),
            QuirePseudoClass::Valid => dest.write_str(":valid"),
            QuirePseudoClass::Invalid => dest.write_str(":invalid"),
            QuirePseudoClass::InRange => dest.write_str(":in-range"),
            QuirePseudoClass::OutOfRange => dest.write_str(":out-of-range"),
            QuirePseudoClass::Required => dest.write_str(":required"),
            QuirePseudoClass::Optional => dest.write_str(":optional"),
            QuirePseudoClass::ReadWrite => dest.write_str(":read-write"),
            QuirePseudoClass::ReadOnly => dest.write_str(":read-only"),
            QuirePseudoClass::Lang(ranges) => {
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
pub(crate) struct LanguageRange(pub(in crate::css) String);
