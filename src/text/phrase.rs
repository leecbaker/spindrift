use std::sync::OnceLock;

use icu_segmenter::WordSegmenter;
use icu_segmenter::options::WordBreakInvariantOptions;
use kham_core::Tokenizer;
use kham_core::ne::NeTagger;

/// A declared language for which Spindrift has a phrase-boundary provider.
///
/// This is intentionally smaller than the general content-language model:
/// CSS `word-break:auto-phrase` must fall back to normal wrapping when the
/// user agent cannot analyze the declared language.
/// <https://drafts.csswg.org/css-text-4/#valdef-word-break-auto-phrase>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoPhraseLanguage {
    Thai,
    Japanese,
}

impl AutoPhraseLanguage {
    pub(crate) fn from_language(language: Option<&str>) -> Option<Self> {
        let primary = language?
            .trim()
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match primary.as_str() {
            "th" => Some(Self::Thai),
            "ja" => Some(Self::Japanese),
            _ => None,
        }
    }
}

/// A source-faithful phrase-analysis span.
///
/// The offsets are UTF-8 byte positions in the original CSS Text stream. The
/// inline opportunity graph maps them to its typed graph positions; this
/// module never inserts virtual separators or rewrites source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PhraseAnalysisSpan {
    boundaries: Vec<usize>,
}

impl PhraseAnalysisSpan {
    #[cfg(test)]
    pub(crate) fn boundary_offsets(&self) -> &[usize] {
        &self.boundaries
    }

    pub(crate) fn contains_boundary(&self, byte_offset: usize) -> bool {
        self.boundaries.binary_search(&byte_offset).is_ok()
    }
}

/// Return phrase boundaries for one source-faithful text span.
///
/// A mismatched script is deliberately unsupported rather than guessed. This
/// makes `lang=ja` Thai content retain `word-break:normal` behavior, as CSS
/// Text requires when phrase detection is unavailable for the content.
pub(crate) fn phrase_boundaries(
    text: &str,
    language: AutoPhraseLanguage,
) -> Option<PhraseAnalysisSpan> {
    let boundaries = match language {
        AutoPhraseLanguage::Thai if contains_thai(text) => thai_phrase_boundaries(text),
        AutoPhraseLanguage::Japanese if contains_japanese(text) => japanese_phrase_boundaries(text),
        AutoPhraseLanguage::Thai | AutoPhraseLanguage::Japanese => return None,
    };
    Some(PhraseAnalysisSpan { boundaries })
}

fn contains_thai(text: &str) -> bool {
    text.chars()
        .any(|character| ('\u{0e00}'..='\u{0e7f}').contains(&character))
}

fn contains_japanese(text: &str) -> bool {
    text.chars().any(|character| {
        matches!(character,
            '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
        )
    })
}

fn thai_phrase_boundaries(text: &str) -> Vec<usize> {
    static THAI_ANALYZER: OnceLock<(Tokenizer, NeTagger)> = OnceLock::new();
    let (tokenizer, names) = THAI_ANALYZER.get_or_init(|| (Tokenizer::new(), NeTagger::builtin()));
    let tokens = names.tag_tokens(tokenizer.segment(text), text);
    let mut boundaries = tokens
        .into_iter()
        .map(|token| token.span.end)
        .filter(|offset| *offset > 0 && *offset < text.len())
        .collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
}

fn japanese_phrase_boundaries(text: &str) -> Vec<usize> {
    WordSegmenter::new_auto(WordBreakInvariantOptions::default())
        .segment_str(text)
        .filter(|offset| {
            *offset > 0
                && *offset < text.len()
                && matches!(
                    text[..*offset].chars().next_back(),
                    Some(
                        'は' | 'が' | 'を' | 'に' | 'へ' | 'と' | 'で' | 'の' | 'も' | 'や' | 'か'
                    )
                )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thai_named_entity_is_one_phrase_unit() {
        let text = "กรุงเทพคือสวยงาม";
        let boundaries =
            phrase_boundaries(text, AutoPhraseLanguage::Thai).expect("Thai has a phrase provider");
        assert_eq!(
            boundaries.boundary_offsets(),
            ["กรุงเทพ".len(), "กรุงเทพคือ".len()]
        );
        assert!(boundaries.contains_boundary("กรุงเทพ".len()));
        assert!(boundaries.contains_boundary("กรุงเทพคือ".len()));
    }

    #[test]
    fn mistagged_thai_is_not_analyzed_as_japanese() {
        assert!(phrase_boundaries("กรุงเทพคือสวยงาม", AutoPhraseLanguage::Japanese).is_none());
    }
}
