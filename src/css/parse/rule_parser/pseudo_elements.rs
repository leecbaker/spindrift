use cssparser::Delimiter;

use super::*;
use crate::css::{LayerName, StylesheetScopeAnchor};

/// Splits a selector list only at CSS Syntax comma delimiters.  This is kept
/// with pseudo-element routing because it works on selector source retained
/// for that routing path, rather than being part of CSS Nesting expansion.
pub(in crate::css) fn split_selector_list(selectors: &str) -> Vec<&str> {
    let mut input = ParserInput::new(selectors);
    let mut parser = Parser::new(&mut input);
    let mut parts = Vec::new();
    while !parser.is_exhausted() {
        let start = parser.position();
        if parser
            .parse_until_before(Delimiter::Comma, |input| {
                while input.next_including_whitespace_and_comments().is_ok() {}
                Ok::<(), cssparser::ParseError<'_, ()>>(())
            })
            .is_err()
        {
            return Vec::new();
        }
        let part = parser.slice_from(start).trim();
        if part.is_empty() {
            return Vec::new();
        }
        parts.push(part);
        if parser.next().is_err() {
            break;
        }
    }
    parts
}

pub(in crate::css) fn split_pseudo_element_rule(
    selector_text: &str,
    selector_parser: &QuireSelectorParser,
    declarations: &Declarations,
    layer_name: Option<LayerName>,
    scopes: Vec<ScopeRule>,
    stylesheet_scope_anchor: StylesheetScopeAnchor,
) -> Vec<ParsedCssRule> {
    // CSS Pseudo-Elements 4 pseudo rules are matched against their originating
    // elements, then applied in pseudo-specific cascade/layout paths.
    // <https://www.w3.org/TR/css-pseudo-4/#pseudo-elements>
    let pseudo_names = [
        (RoutedPseudoElement::BeforeMarker, "before::marker"),
        (RoutedPseudoElement::AfterMarker, "after::marker"),
        (RoutedPseudoElement::Marker, "marker"),
        (RoutedPseudoElement::Before, "before"),
        (RoutedPseudoElement::After, "after"),
        (RoutedPseudoElement::FootnoteCall, "footnote-call"),
        (RoutedPseudoElement::FootnoteMarker, "footnote-marker"),
        (RoutedPseudoElement::FirstLine, "first-line"),
        (RoutedPseudoElement::FirstLetter, "first-letter"),
    ];
    let mut normal_selectors = Vec::new();
    let mut routed_selectors = Vec::new();
    for selector in split_selector_list(selector_text) {
        let mut routed = false;
        for (pseudo, name) in pseudo_names {
            if let Some(base) = strip_pseudo_selector(selector, name) {
                routed_selectors.push((pseudo, base.to_string()));
                routed = true;
                break;
            }
        }
        if !routed {
            normal_selectors.push(selector.trim().to_string());
        }
    }
    if routed_selectors.is_empty() {
        return Vec::new();
    }

    let mut rules = Vec::new();
    if !normal_selectors.is_empty() {
        let selector_text = normal_selectors.join(", ");
        if let Some(selector) = parse_selector_list_text(&selector_text, selector_parser) {
            let specificity = selector
                .slice()
                .iter()
                .map(|branch| branch.specificity())
                .max()
                .unwrap_or(0);
            rules.push(ParsedCssRule::Style(StyleRule {
                selector_text,
                selector,
                stylesheet_scope_anchor,
                declarations: declarations.clone(),
                specificity,
                order: 0,
                layer_name: layer_name.clone(),
                scopes: scopes.clone(),
            }));
        }
    }
    for (pseudo, _name) in pseudo_names {
        let base_selectors = routed_selectors
            .iter()
            .filter_map(|(routed_pseudo, selector)| (*routed_pseudo == pseudo).then_some(selector))
            .cloned()
            .collect::<Vec<_>>();
        if base_selectors.is_empty() {
            continue;
        }
        let selector_text = base_selectors.join(", ");
        let Some(selector) = parse_selector_list_text(&selector_text, selector_parser) else {
            continue;
        };
        let specificity = selector
            .slice()
            .iter()
            .map(|branch| branch.specificity())
            .max()
            .unwrap_or(0);
        let rule = StyleRule {
            selector_text,
            selector,
            stylesheet_scope_anchor,
            declarations: declarations.clone(),
            specificity,
            order: 0,
            layer_name: layer_name.clone(),
            scopes: scopes.clone(),
        };
        rules.push(match pseudo {
            RoutedPseudoElement::Marker => ParsedCssRule::Marker(rule),
            RoutedPseudoElement::BeforeMarker => ParsedCssRule::BeforeMarker(rule),
            RoutedPseudoElement::AfterMarker => ParsedCssRule::AfterMarker(rule),
            RoutedPseudoElement::Before => ParsedCssRule::Before(rule),
            RoutedPseudoElement::After => ParsedCssRule::After(rule),
            RoutedPseudoElement::FootnoteCall => ParsedCssRule::FootnoteCall(rule),
            RoutedPseudoElement::FootnoteMarker => ParsedCssRule::FootnoteMarker(rule),
            RoutedPseudoElement::FirstLine => ParsedCssRule::FirstLine(rule),
            RoutedPseudoElement::FirstLetter => ParsedCssRule::FirstLetter(rule),
        });
    }
    rules
}

pub(in crate::css) fn parse_selector_list_text(
    selector_text: &str,
    selector_parser: &QuireSelectorParser,
) -> Option<SelectorList<QuireSelectorImpl>> {
    let mut input = ParserInput::new(selector_text);
    let mut parser = Parser::new(&mut input);
    SelectorList::parse(selector_parser, &mut parser, ParseRelative::No).ok()
}

pub(in crate::css) fn strip_pseudo_selector<'a>(
    selector: &'a str,
    pseudo: &str,
) -> Option<std::borrow::Cow<'a, str>> {
    let trimmed = selector.trim();
    let double_colon = format!("::{pseudo}");
    let single_colon = format!(":{pseudo}");
    let raw_base = trimmed
        .strip_suffix(&double_colon)
        .or_else(|| trimmed.strip_suffix(&single_colon))?;
    let base = raw_base.trim();
    if base.is_empty() {
        return Some(std::borrow::Cow::Borrowed("*"));
    }
    if raw_base
        .chars()
        .last()
        .is_some_and(|character| character.is_ascii_whitespace())
        || base.ends_with(['>', '+', '~'])
    {
        return Some(std::borrow::Cow::Owned(format!("{base} *")));
    }
    Some(std::borrow::Cow::Borrowed(base))
}
