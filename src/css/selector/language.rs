use crate::css::types::ResolvedLanguage;
use cssparser::{ToCss, serialize_identifier, serialize_string};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanguageRange(pub(in crate::css) String);

impl LanguageRange {
    pub(in crate::css) fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || is_valid_extended_language_range(value) {
            Some(Self(value.to_ascii_lowercase()))
        } else {
            None
        }
    }

    pub(in crate::css) fn as_str(&self) -> &str {
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

pub(in crate::css) fn language_from_attrs(
    attrs: &HashMap<String, String>,
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
        ResolvedLanguage::Malformed(_) => false,
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
