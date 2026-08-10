use super::*;
use crate::css::component_values::{css_leading_function_matching, css_leading_ident};

pub(crate) fn parse_bookmark_level(value: &str) -> Option<Option<u32>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|level| *level >= 1)
        .map(Some)
}

pub(crate) fn parse_bookmark_state(value: &str) -> Option<CssBookmarkState> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "open" => Some(CssBookmarkState::Open),
        "closed" => Some(CssBookmarkState::Closed),
        _ => None,
    }
}

pub(crate) fn parse_bookmark_label(value: &str) -> Option<BookmarkLabel> {
    let mut rest = trim_css_value(value);
    let mut parts = Vec::new();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some((string, tail)) = parse_css_string_token(rest) {
            parts.push(BookmarkLabelPart::String(string));
            rest = tail;
        } else if let Some((argument, tail)) = css_leading_function_matching(rest, "content") {
            let argument = argument.trim().to_ascii_lowercase();
            if argument.is_empty() || argument == "text" {
                parts.push(BookmarkLabelPart::ContentText);
                rest = tail;
            } else {
                return None;
            }
        } else if let Some((argument, tail)) = css_leading_function_matching(rest, "attr") {
            let name = argument
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if name.is_empty()
                || crate::css::component_values::try_split_css_component_values(&name)
                    .is_none_or(|parts| parts.len() != 1)
            {
                return None;
            }
            parts.push(BookmarkLabelPart::Attr(name));
            rest = tail;
        } else if let Some((ident, tail)) = css_leading_ident(rest)
            && ident.eq_ignore_ascii_case("contents")
        {
            parts.push(BookmarkLabelPart::ContentText);
            rest = tail;
        } else {
            return None;
        }
    }
    (!parts.is_empty()).then_some(BookmarkLabel { parts })
}
