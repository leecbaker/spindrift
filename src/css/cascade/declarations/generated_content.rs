use super::*;

pub(in crate::css) fn parse_positive_integer(value: &str) -> Option<usize> {
    let value = value.trim();
    if value.starts_with('+') || value.starts_with('-') || value.contains('.') {
        return None;
    }
    value.parse::<usize>().ok().filter(|value| *value > 0)
}

/// Parse CSS `hyphenate-limit-chars`.
///
/// CSS Text defines the grammar as one to three values, each `auto` or a
/// positive integer: total word length, minimum characters before the break,
/// and minimum characters after the break:
/// <https://www.w3.org/TR/css-text-4/#hyphenate-limit-chars>.
pub(in crate::css) fn parse_hyphenate_limit_chars(value: &str) -> Option<HyphenateLimitChars> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let total = parse_hyphenate_limit_component(parts[0], HyphenateLimitChars::AUTO_TOTAL)?;
    let before = parts
        .get(1)
        .map(|value| parse_hyphenate_limit_component(value, HyphenateLimitChars::AUTO_BEFORE))
        .unwrap_or(Some(HyphenateLimitChars::AUTO_BEFORE))?;
    let after = parts
        .get(2)
        .map(|value| parse_hyphenate_limit_component(value, HyphenateLimitChars::AUTO_AFTER))
        .unwrap_or(Some(HyphenateLimitChars::AUTO_AFTER))?;
    Some(HyphenateLimitChars {
        total,
        before,
        after,
    })
}

/// Parse CSS Text's `hyphenate-character` keyword or string.
///
/// <https://drafts.csswg.org/css-text-4/#hyphenate-character>
pub(in crate::css) fn parse_hyphenate_character(value: &str) -> Option<HyphenateCharacter> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(HyphenateCharacter::Auto);
    }
    let (value, tail) = parse_css_string_token(value)?;
    tail.trim()
        .is_empty()
        .then_some(HyphenateCharacter::String(value))
}

pub(in crate::css) fn parse_hyphenate_limit_component(value: &str, auto_value: u16) -> Option<u16> {
    if value.eq_ignore_ascii_case("auto") {
        return Some(auto_value);
    }
    u16::try_from(parse_positive_integer(value)?).ok()
}

pub(in crate::css) fn is_css_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '-' || first.is_ascii_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '-' || character.is_ascii_alphanumeric()
        })
}

pub(in crate::css) fn parse_running_position(value: &str) -> Option<String> {
    let value = value.trim();
    let prefix = value.get(.."running".len())?;
    if !prefix.eq_ignore_ascii_case("running") {
        return None;
    }
    let argument = value["running".len()..]
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    is_css_identifier(argument).then(|| argument.to_string())
}

pub(in crate::css) fn parse_page_break(value: &str) -> PageBreak {
    match value.trim().to_ascii_lowercase().as_str() {
        "avoid" | "avoid-page" => PageBreak::AvoidPage,
        "page" | "always" => PageBreak::Page,
        "left" => PageBreak::Left,
        "right" => PageBreak::Right,
        "recto" => PageBreak::Recto,
        "verso" => PageBreak::Verso,
        _ => PageBreak::Auto,
    }
}

pub(in crate::css) fn parse_fragment_break(value: &str) -> PageBreak {
    match value.trim().to_ascii_lowercase().as_str() {
        "avoid" => PageBreak::Avoid,
        "avoid-page" => PageBreak::AvoidPage,
        "avoid-column" => PageBreak::AvoidColumn,
        "column" => PageBreak::Column,
        _ => parse_page_break(value),
    }
}
