use cssparser::Delimiter;

use super::*;

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

/// Remove one already-validated routed pseudo-element from a serialized
/// selector. The selector parser establishes which pseudo-element is present;
/// this helper only adapts that generated-box selector to its originating
/// element selector.
///
/// CSS Overflow 5 permits target-state pseudo-classes after
/// `::scroll-marker`; retain that state for the originating-element cascade.
/// <https://www.w3.org/TR/css-pseudo-4/#generated-content>
/// <https://drafts.csswg.org/css-overflow-5/#scroll-markers>
pub(in crate::css) fn strip_routed_pseudo_selector(
    selector: &str,
    pseudo: RoutedPseudoElement,
) -> Option<std::borrow::Cow<'_, str>> {
    let name = match pseudo {
        RoutedPseudoElement::Marker => "marker",
        RoutedPseudoElement::Before => "before",
        RoutedPseudoElement::After => "after",
        RoutedPseudoElement::ScrollMarker => "scroll-marker",
        RoutedPseudoElement::ScrollMarkerGroup => "scroll-marker-group",
        RoutedPseudoElement::FootnoteCall => "footnote-call",
        RoutedPseudoElement::FootnoteMarker => "footnote-marker",
        RoutedPseudoElement::FirstLine => "first-line",
        RoutedPseudoElement::FirstLetter => "first-letter",
        // The selector crate cannot represent chained tree-abiding
        // pseudo-elements. Those use the source-only fallback below.
        RoutedPseudoElement::BeforeMarker | RoutedPseudoElement::AfterMarker => return None,
    };
    strip_pseudo_selector(selector, name)
}

/// Route the only chained tree-abiding pseudo-elements outside the selector
/// crate's AST, plus CSS Overflow target state after ::scroll-marker. This is
/// deliberately exact: a parse failure must not turn an
/// arbitrary selector containing pseudo-element-like text into an
/// originating-element rule.
pub(in crate::css) fn source_only_routed_pseudo_route(
    selector: &str,
) -> Option<(RoutedPseudoElement, std::borrow::Cow<'_, str>)> {
    let trimmed = selector.trim();
    for (pseudo, suffix) in [
        (RoutedPseudoElement::BeforeMarker, "::before::marker"),
        (RoutedPseudoElement::AfterMarker, "::after::marker"),
    ] {
        if trimmed.ends_with(suffix) {
            return strip_pseudo_selector(trimmed, &suffix[2..]).map(|base| (pseudo, base));
        }
    }
    for state in ["target-current", "target-before", "target-after"] {
        let suffix = format!("::scroll-marker:{state}");
        if trimmed.ends_with(&suffix) {
            return strip_pseudo_selector(trimmed, "scroll-marker")
                .map(|base| (RoutedPseudoElement::ScrollMarker, base));
        }
    }
    None
}

pub(in crate::css) fn strip_pseudo_selector<'a>(
    selector: &'a str,
    pseudo: &str,
) -> Option<std::borrow::Cow<'a, str>> {
    let trimmed = selector.trim();
    let double_colon = format!("::{pseudo}");
    let single_colon = format!(":{pseudo}");
    let (raw_base, trailing_state) = if let Some(raw_base) = trimmed.strip_suffix(&double_colon) {
        (raw_base, "")
    } else if let Some(raw_base) = trimmed.strip_suffix(&single_colon) {
        (raw_base, "")
    } else {
        // CSS Overflow 5 permits target-state pseudo-classes after its
        // pseudo-elements. Route the pseudo-element away while retaining the
        // state selector on its originating element for the pseudo cascade.
        let position = trimmed.find(&double_colon)?;
        let after = &trimmed[position + double_colon.len()..];
        if !after.starts_with(':') {
            return None;
        }
        (&trimmed[..position], after)
    };
    let base = raw_base.trim();
    if base.is_empty() {
        return Some(std::borrow::Cow::Owned(format!("*{trailing_state}")));
    }
    if raw_base
        .chars()
        .last()
        .is_some_and(|character| character.is_ascii_whitespace())
        || base.ends_with(['>', '+', '~'])
    {
        return Some(std::borrow::Cow::Owned(format!("{base} *{trailing_state}")));
    }
    if trailing_state.is_empty() {
        Some(std::borrow::Cow::Borrowed(base))
    } else {
        Some(std::borrow::Cow::Owned(format!("{base}{trailing_state}")))
    }
}
