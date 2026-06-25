use super::*;

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
        } else if let Some(tail) = strip_ascii_function(rest, "content") {
            let (argument, tail) = split_function_argument(tail)?;
            let argument = argument.trim().to_ascii_lowercase();
            if argument.is_empty() || argument == "text" {
                parts.push(BookmarkLabelPart::ContentText);
                rest = tail;
            } else {
                return None;
            }
        } else if let Some(tail) = strip_ascii_function(rest, "attr") {
            let (argument, tail) = split_function_argument(tail)?;
            let name = argument
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if name.is_empty() || name.split_whitespace().count() != 1 {
                return None;
            }
            parts.push(BookmarkLabelPart::Attr(name));
            rest = tail;
        } else if rest
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("contents"))
        {
            let tail = &rest[8..];
            if tail.chars().next().is_some_and(is_css_ident_continue) {
                return None;
            }
            parts.push(BookmarkLabelPart::ContentText);
            rest = tail;
        } else {
            return None;
        }
    }
    (!parts.is_empty()).then_some(BookmarkLabel { parts })
}
