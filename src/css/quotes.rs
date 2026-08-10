/// Language-specific quote pairs for CSS `quotes: auto`.
///
/// CSS Content Level 3 defines `quotes: auto` as a user-agent-chosen quotation
/// mark system based on the parent content language, and points to CLDR for
/// suitable language data:
/// <https://www.w3.org/TR/css-content-3/#quotes-property>.
///
/// The table is ported from checked-out WeasyPrint's CLDR-derived
/// `weasyprint.text.constants.LANG_QUOTES`.
/// A static quotation-mark system selected for a computed `quotes: auto`
/// value.
///
/// The system holds only references into the CLDR-derived table, so resolving
/// the parent language does not retain or allocate an owned language string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedAutoQuotes {
    open: &'static [&'static str],
    close: &'static [&'static str],
}

impl ResolvedAutoQuotes {
    pub(crate) fn pair_at_depth(self, depth: usize) -> (&'static str, &'static str) {
        let open = self
            .open
            .get(depth)
            .or_else(|| self.open.last())
            .copied()
            .unwrap_or(DEFAULT_QUOTES.open[0]);
        let close = self
            .close
            .get(depth)
            .or_else(|| self.close.last())
            .copied()
            .unwrap_or(DEFAULT_QUOTES.close[0]);
        (open, close)
    }
}

#[derive(Debug, Clone, Copy)]
struct LangQuotes {
    language: &'static str,
    open: &'static [&'static str],
    close: &'static [&'static str],
}

const DEFAULT_QUOTES: LangQuotes = LangQuotes {
    language: "",
    open: &["“", "‘"],
    close: &["”", "’"],
};

const LANG_QUOTES: &[LangQuotes] = &[
    LangQuotes {
        language: "ab",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "agq",
        open: &["„", "‚"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "am",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "an",
        open: &["«", "”"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "ar",
        open: &["”", "’"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "ast",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "az_Arab",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "az_Cyrl",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "bas",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "be",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "bg",
        open: &["„"],
        close: &["“"],
    },
    LangQuotes {
        language: "blo",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "bm",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "br",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "bs",
        open: &["„", "‘"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "bs_Cyrl",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "ca",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "co",
        open: &["«"],
        close: &["»"],
    },
    LangQuotes {
        language: "cs",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "cu",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "cv",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "de",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "dsb",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "dua",
        open: &["«", "‘"],
        close: &["»", "’"],
    },
    LangQuotes {
        language: "dyo",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "el",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "el_POLYTON",
        open: &["«", "‘"],
        close: &["»", "’"],
    },
    LangQuotes {
        language: "es_US",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "et",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "eu",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "ewo",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "fa",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "ff",
        open: &["„", "‚"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "fi",
        open: &["”", "’"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "fr",
        open: &["«"],
        close: &["»"],
    },
    LangQuotes {
        language: "fr_CA",
        open: &["«", "”"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "fr_CH",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "fur",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "gsw",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "he",
        open: &["”", "’"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "hr",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "hsb",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "hu",
        open: &["„", "»"],
        close: &["”", "«"],
    },
    LangQuotes {
        language: "hy",
        open: &["«"],
        close: &["»"],
    },
    LangQuotes {
        language: "ia",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "ie",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "is",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "it",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "it_CH",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "ja",
        open: &["「", "『"],
        close: &["」", "』"],
    },
    LangQuotes {
        language: "jgo",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "ka",
        open: &["„", "«"],
        close: &["“", "»"],
    },
    LangQuotes {
        language: "kab",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "kk",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "kkj",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "kl",
        open: &["»", "›"],
        close: &["«", "‹"],
    },
    LangQuotes {
        language: "ksf",
        open: &["«", "‘"],
        close: &["»", "’"],
    },
    LangQuotes {
        language: "ksh",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "ky",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "lag",
        open: &["”", "’"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "lb",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "lij",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "lt",
        open: &["„"],
        close: &["“"],
    },
    LangQuotes {
        language: "luy",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "mg",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "mk",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "ms_Arab",
        open: &["”", "’"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "mua",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "mzn",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "nds",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "nl",
        open: &["‘"],
        close: &["’"],
    },
    LangQuotes {
        language: "nmg",
        open: &["„", "«"],
        close: &["”", "»"],
    },
    LangQuotes {
        language: "nnh",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "no",
        open: &["«", "‘"],
        close: &["»", "’"],
    },
    LangQuotes {
        language: "nr",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "nso",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "oc",
        open: &["«"],
        close: &["»"],
    },
    LangQuotes {
        language: "oc_ES",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "os",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "pl",
        open: &["„", "«"],
        close: &["”", "»"],
    },
    LangQuotes {
        language: "prg",
        open: &["„"],
        close: &["“"],
    },
    LangQuotes {
        language: "pt_PT",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "rm",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "rn",
        open: &["”", "’"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "ro",
        open: &["„", "«"],
        close: &["”", "»"],
    },
    LangQuotes {
        language: "ru",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "rw",
        open: &["«", "‘"],
        close: &["»", "’"],
    },
    LangQuotes {
        language: "sah",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "sc",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "sdh",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "se",
        open: &["”", "’"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "sg",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "shi",
        open: &["«", "„"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "sk",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "sl",
        open: &["„", "‚"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "sn",
        open: &["”", "’"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "sq",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "sr",
        open: &["„", "‘"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "sr_Latn",
        open: &["„", "‘"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "ss",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "st",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "sv",
        open: &["”", "’"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "syr",
        open: &["”", "’"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "szl",
        open: &["„", "»"],
        close: &["”", "«"],
    },
    LangQuotes {
        language: "tg",
        open: &["»", "‘"],
        close: &["«", "’"],
    },
    LangQuotes {
        language: "ti",
        open: &["«", "“"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "ti_ER",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "tk",
        open: &["“"],
        close: &["”"],
    },
    LangQuotes {
        language: "tn",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "ts",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "ug",
        open: &["»", "›"],
        close: &["«", "‹"],
    },
    LangQuotes {
        language: "uk",
        open: &["«", "„"],
        close: &["»", "“"],
    },
    LangQuotes {
        language: "ur",
        open: &["”", "’"],
        close: &["“", "‘"],
    },
    LangQuotes {
        language: "uz",
        open: &["“", "’"],
        close: &["”", "‘"],
    },
    LangQuotes {
        language: "ve",
        open: &["‘", "“"],
        close: &["’", "”"],
    },
    LangQuotes {
        language: "wae",
        open: &["«", "‹"],
        close: &["»", "›"],
    },
    LangQuotes {
        language: "yav",
        open: &["«"],
        close: &["»"],
    },
    LangQuotes {
        language: "yi",
        open: &["”", "’"],
        close: &["”", "’"],
    },
    LangQuotes {
        language: "yue",
        open: &["「", "『"],
        close: &["」", "』"],
    },
    LangQuotes {
        language: "zgh",
        open: &["«", "„"],
        close: &["»", "”"],
    },
    LangQuotes {
        language: "zh_Hant",
        open: &["「", "『"],
        close: &["」", "』"],
    },
];

pub(crate) fn resolved_auto_quotes_for_language(language: Option<&str>) -> ResolvedAutoQuotes {
    let quotes = language
        .and_then(lang_quotes_for_language)
        .unwrap_or(DEFAULT_QUOTES);
    ResolvedAutoQuotes {
        open: quotes.open,
        close: quotes.close,
    }
}

fn lang_quotes_for_language(language: &str) -> Option<LangQuotes> {
    if let Some(quotes) = LANG_QUOTES
        .iter()
        .find(|quotes| language_tag_eq(language, quotes.language))
    {
        return Some(*quotes);
    }
    LANG_QUOTES
        .iter()
        .filter(|quotes| language_matches_parent(language, quotes.language))
        .max_by_key(|quotes| quotes.language.len())
        .copied()
}

fn language_tag_eq(language: &str, candidate: &str) -> bool {
    language.len() == candidate.len()
        && language
            .bytes()
            .zip(candidate.bytes())
            .all(|(actual, expected)| language_tag_byte_eq(actual, expected))
}

fn language_matches_parent(language: &str, candidate: &str) -> bool {
    language.len() > candidate.len()
        && language
            .as_bytes()
            .get(..candidate.len())
            .is_some_and(|prefix| {
                prefix
                    .iter()
                    .copied()
                    .zip(candidate.bytes())
                    .all(|(actual, expected)| language_tag_byte_eq(actual, expected))
            })
        && matches!(language.as_bytes().get(candidate.len()), Some(b'-' | b'_'))
}

fn language_tag_byte_eq(actual: u8, expected: u8) -> bool {
    actual.eq_ignore_ascii_case(&expected)
        || matches!((actual, expected), (b'-', b'_') | (b'_', b'-'))
}

#[cfg(test)]
mod tests {
    use super::resolved_auto_quotes_for_language;

    fn language_quote_pair(language: Option<&str>, depth: usize) -> (&'static str, &'static str) {
        resolved_auto_quotes_for_language(language).pair_at_depth(depth)
    }

    #[test]
    fn unknown_language_uses_default_curly_quotes() {
        assert_eq!(language_quote_pair(None, 0), ("“", "”"));
        assert_eq!(language_quote_pair(Some("unknown"), 1), ("‘", "’"));
    }

    #[test]
    fn cldr_languages_resolve_from_weasyprint_table() {
        assert_eq!(language_quote_pair(Some("el"), 0), ("«", "»"));
        assert_eq!(language_quote_pair(Some("fa"), 1), ("‹", "›"));
        assert_eq!(language_quote_pair(Some("ja"), 1), ("『", "』"));
        assert_eq!(language_quote_pair(Some("ar"), 0), ("”", "“"));
        assert_eq!(language_quote_pair(Some("ug"), 1), ("›", "‹"));
        assert_eq!(language_quote_pair(Some("zh-Hant"), 0), ("「", "」"));
    }

    #[test]
    fn lookup_normalizes_separators_case_and_parent_tags() {
        assert_eq!(language_quote_pair(Some("fr"), 1), ("«", "»"));
        assert_eq!(language_quote_pair(Some("fr-CH"), 1), ("‹", "›"));
        assert_eq!(language_quote_pair(Some("fr_CH_alt"), 1), ("‹", "›"));
        assert_eq!(language_quote_pair(Some("EL-polyton"), 1), ("‘", "’"));
    }

    #[test]
    fn depth_past_available_pairs_reuses_deepest_pair() {
        assert_eq!(language_quote_pair(Some("fr"), 8), ("«", "»"));
        assert_eq!(language_quote_pair(Some("fr-CH"), 8), ("‹", "›"));
    }

    #[test]
    fn malformed_or_non_ascii_language_uses_default_quotes_without_panicking() {
        assert_eq!(language_quote_pair(Some("é-fr"), 0), ("“", "”"));
        assert_eq!(language_quote_pair(Some("\u{e3}"), 1), ("‘", "’"));
    }
}
