use super::{LinkMatching, QuireSelectorImpl, StyleElement};
use crate::css::types::{ElementSignature, ScopeRule};
use crate::css::{ScopeRoot, StylesheetScopeAnchor};
use selectors::OpaqueElement;
use selectors::context::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
    SelectorCaches,
};
use selectors::matching::{matches_selector, matches_selector_list};
use selectors::parser::SelectorList;
use std::borrow::Cow;
use std::rc::Rc;

/// Matches a style rule selector against a prebuilt selector chain.
///
/// A scoped declaration applies only if each enclosing `@scope` contains the
/// element. The final selector is then matched with the innermost scoping root
/// as Selectors 4 `:scope`. The returned proximity is the ancestor distance
/// from the element to that root; lower distances sort stronger in Cascade 5:
/// <https://www.w3.org/TR/css-cascade-5/#scoped-styles>.
pub(in crate::css) fn selector_matches_with_scope_proximity_in_chain_with_link_matching<'a>(
    selector: &SelectorList<QuireSelectorImpl>,
    scopes: &[ScopeRule],
    chain: &Rc<Vec<Cow<'a, ElementSignature>>>,
    current_index: usize,
    caches: &mut SelectorCaches,
    link_matching: LinkMatching,
) -> Option<(usize, u32)> {
    if scopes.is_empty() {
        return selector_matching_specificity_at(
            selector,
            chain,
            current_index,
            None,
            caches,
            link_matching,
        )
        .map(|specificity| (usize::MAX, specificity));
    }
    let mut proximity = usize::MAX;
    let mut scope_root_index = None;
    let mut parent_scope: Option<(&ScopeRule, usize)> = None;
    for scope in scopes {
        let (root_index, distance) = scope_rule_distance_with_link_matching(
            scope,
            chain,
            current_index,
            parent_scope,
            caches,
            link_matching,
        )?;
        scope_root_index = Some(root_index);
        proximity = distance;
        parent_scope = Some((scope, root_index));
    }
    selector_matching_specificity_at(
        selector,
        chain,
        current_index,
        scope_root_index,
        caches,
        link_matching,
    )
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

fn selector_matches_at_with_link_matching<'a>(
    selector: &SelectorList<QuireSelectorImpl>,
    chain: &Rc<Vec<Cow<'a, ElementSignature>>>,
    index: usize,
    scope_index: Option<usize>,
    caches: &mut SelectorCaches,
    link_matching: LinkMatching,
) -> bool {
    let element = StyleElement {
        chain: Rc::clone(chain),
        index,
        link_matching,
    };
    let scope_element = scope_index.map(|index| OpaqueElement::new(&*chain[index].opaque_id));
    let mut context = matching_context(
        caches,
        scope_element,
        chain[index].document_compatibility_mode,
    );
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
    link_matching: LinkMatching,
) -> Option<u32> {
    let element = StyleElement {
        chain: Rc::clone(chain),
        index,
        link_matching,
    };
    let scope_element = scope_index.map(|index| OpaqueElement::new(&*chain[index].opaque_id));
    let mut context = matching_context(
        caches,
        scope_element,
        chain[index].document_compatibility_mode,
    );
    selector
        .slice()
        .iter()
        .filter(|branch| matches_selector(*branch, 0, None, &element, &mut context))
        .map(|branch| branch.specificity())
        .max()
}

fn matching_context<'a>(
    caches: &'a mut SelectorCaches,
    scope_element: Option<OpaqueElement>,
    compatibility_mode: crate::dom::DocumentCompatibilityMode,
) -> MatchingContext<'a, QuireSelectorImpl> {
    let mut context = MatchingContext::new(
        MatchingMode::Normal,
        None,
        caches,
        match compatibility_mode {
            crate::dom::DocumentCompatibilityMode::NoQuirks => QuirksMode::NoQuirks,
            crate::dom::DocumentCompatibilityMode::LimitedQuirks => QuirksMode::LimitedQuirks,
            crate::dom::DocumentCompatibilityMode::Quirks => QuirksMode::Quirks,
        },
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    context.scope_element = scope_element;
    context
}

fn scope_rule_distance_with_link_matching<'a>(
    scope: &ScopeRule,
    chain: &Rc<Vec<Cow<'a, ElementSignature>>>,
    current_index: usize,
    parent_scope: Option<(&ScopeRule, usize)>,
    caches: &mut SelectorCaches,
    link_matching: LinkMatching,
) -> Option<(usize, usize)> {
    for root_index in (0..=current_index).rev() {
        if let Some((parent, parent_root)) = parent_scope
            && (root_index < parent_root
                || scope_limit_matches(
                    parent,
                    chain,
                    parent_root,
                    root_index,
                    caches,
                    link_matching,
                ))
        {
            continue;
        }
        let outer_scope_root = parent_scope.map(|(_, root)| root);
        if !scope_root_matches(
            scope,
            chain,
            root_index,
            outer_scope_root,
            caches,
            link_matching,
        ) {
            continue;
        }
        if scope_limit_matches(
            scope,
            chain,
            root_index,
            current_index,
            caches,
            link_matching,
        ) {
            continue;
        }
        return Some((root_index, current_index - root_index));
    }
    None
}

fn scope_limit_matches<'a>(
    scope: &ScopeRule,
    chain: &Rc<Vec<Cow<'a, ElementSignature>>>,
    root_index: usize,
    candidate_index: usize,
    caches: &mut SelectorCaches,
    link_matching: LinkMatching,
) -> bool {
    scope.limit.as_ref().is_some_and(|limit| {
        (root_index + 1..=candidate_index).any(|index| {
            selector_matches_at_with_link_matching(
                limit,
                chain,
                index,
                Some(root_index),
                caches,
                link_matching,
            )
        })
    })
}

fn scope_root_matches<'a>(
    scope: &ScopeRule,
    chain: &Rc<Vec<Cow<'a, ElementSignature>>>,
    index: usize,
    outer_scope_root: Option<usize>,
    caches: &mut SelectorCaches,
    link_matching: LinkMatching,
) -> bool {
    match &scope.root {
        ScopeRoot::Explicit(selector) => selector_matches_at_with_link_matching(
            selector,
            chain,
            index,
            outer_scope_root,
            caches,
            link_matching,
        ),
        ScopeRoot::Owner(StylesheetScopeAnchor::DocumentRoot) => index == 0,
        ScopeRoot::Owner(StylesheetScopeAnchor::Element(owner)) => chain[index]
            .source_element_id
            .is_some_and(|id| id == *owner),
    }
}
