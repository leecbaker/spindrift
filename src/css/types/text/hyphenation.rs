#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hyphens {
    None,
    Manual,
    Auto,
}

/// Computed CSS `hyphenate-character` value.
///
/// CSS Text inserts this string only when a selected line ends at a manual or
/// automatic hyphenation opportunity. `auto` intentionally remains distinct
/// from an authored string so a future language/font-specific UA default does
/// not lose that distinction during cascade:
/// <https://drafts.csswg.org/css-text-4/#hyphenate-character>.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum HyphenateCharacter {
    #[default]
    Auto,
    String(String),
}

impl HyphenateCharacter {
    /// Resolve the language-sensitive used string for `hyphenate-character`.
    ///
    /// CSS Text leaves the `auto` glyph to the UA, including its choice for a
    /// particular writing system.  Keep that choice at the line-edge
    /// materialization boundary: only a selected discretionary break inserts
    /// this text, whereas an unselected soft hyphen has no used glyph.
    /// <https://drafts.csswg.org/css-text-4/#hyphenate-character>
    pub(crate) fn used_text_for_language(&self, language: Option<&str>) -> &str {
        match self {
            Self::Auto => match language {
                // Uyghur uses kashida as its conventional discretionary
                // marker. The graph materializer supplies the ZWJ context at
                // a joining-script source boundary.
                Some(language) if language_has_primary_subtag(language, "ug") => "\u{0640}",
                // Canadian Aboriginal Syllabics uses U+1400 HYPHEN.
                Some(language) if language_has_primary_subtag(language, "cr") => "\u{1400}",
                // U+2010 is CSS Text's conventional conditional-hyphen
                // presentation for writing systems without a more specific
                // language convention.
                _ => "\u{2010}",
            },
            Self::String(value) => value,
        }
    }
}

/// Match an ASCII BCP 47 primary language subtag without allocating a
/// normalized copy of the full tag.
fn language_has_primary_subtag(language: &str, primary: &str) -> bool {
    language
        .get(..primary.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(primary))
        && language
            .as_bytes()
            .get(primary.len())
            .is_none_or(|separator| *separator == b'-')
}

/// Computed value of CSS `hyphenate-limit-chars`.
///
/// CSS Text defines this as total word characters, characters before the
/// hyphenation break, and characters after the break. `auto` values are
/// user-agent defined; this renderer uses the CSS Text examples' conventional
/// defaults of 5/2/2:
/// <https://www.w3.org/TR/css-text-4/#hyphenate-limit-chars>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HyphenateLimitChars {
    pub(crate) total: u16,
    pub(crate) before: u16,
    pub(crate) after: u16,
}

impl HyphenateLimitChars {
    pub(crate) const AUTO_TOTAL: u16 = 5;
    pub(crate) const AUTO_BEFORE: u16 = 2;
    pub(crate) const AUTO_AFTER: u16 = 2;

    pub(crate) const AUTO: Self = Self {
        total: Self::AUTO_TOTAL,
        before: Self::AUTO_BEFORE,
        after: Self::AUTO_AFTER,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_hyphenate_character_matches_ascii_language_subtags_without_normalizing() {
        let automatic = HyphenateCharacter::Auto;
        assert_eq!(automatic.used_text_for_language(Some("ug")), "\u{0640}");
        assert_eq!(
            automatic.used_text_for_language(Some("UG-Arab-CN")),
            "\u{0640}"
        );
        assert_eq!(automatic.used_text_for_language(Some("Cr")), "\u{1400}");
        assert_eq!(
            automatic.used_text_for_language(Some("CR-Latn")),
            "\u{1400}"
        );
        assert_eq!(automatic.used_text_for_language(Some("ugx")), "\u{2010}");
        assert_eq!(automatic.used_text_for_language(Some("crx")), "\u{2010}");
        assert_eq!(automatic.used_text_for_language(Some("en")), "\u{2010}");
        assert_eq!(automatic.used_text_for_language(None), "\u{2010}");
    }

    #[test]
    fn explicit_hyphenate_character_does_not_depend_on_language() {
        let explicit = HyphenateCharacter::String("=".into());
        assert_eq!(explicit.used_text_for_language(Some("UG-Arab")), "=");
    }
}
