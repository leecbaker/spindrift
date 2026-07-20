use super::*;

/// A byte range covering one CSS Text typographic character unit.
///
/// CSS Text defines line breaking, inter-character justification, and several
/// intrinsic sizing decisions in terms of typographic character units rather
/// than scalar values. ICU grapheme clusters provide the base unit, with CSS
/// Text's cursive-joining constraint layered on top so justification does not
/// insert expansion inside joining sequences:
/// <https://www.w3.org/TR/css-text-3/#typographic-character-unit>,
/// <https://www.w3.org/TR/css-text-3/#text-justify-property>, and
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
pub(crate) fn typographic_unit_ranges(text: &str) -> Vec<Range<usize>> {
    let grapheme_boundaries = GraphemeClusterSegmenter::new()
        .segment_str(text)
        .collect::<Vec<_>>();
    if grapheme_boundaries.len() <= 2 {
        return (!text.is_empty())
            .then_some(0..text.len())
            .into_iter()
            .collect();
    }

    let mut ranges = Vec::<Range<usize>>::new();
    let mut previous_blocks_gap = false;
    for window in grapheme_boundaries.windows(2) {
        let range = window[0]..window[1];
        let unit = &text[range.clone()];
        if unit.is_empty() {
            continue;
        }
        let blocks_gap = text_unit_blocks_inter_character_gap(unit);
        if ranges.is_empty() {
            ranges.push(range);
        } else if (blocks_gap && previous_blocks_gap) || text_unit_is_control_only(unit) {
            if let Some(previous) = ranges.last_mut() {
                previous.end = range.end;
            }
        } else {
            ranges.push(range);
        }
        previous_blocks_gap = blocks_gap;
    }
    ranges
}

pub(crate) fn typographic_unit_count(text: &str) -> usize {
    typographic_unit_ranges(text).len()
}

/// Return whether source text consists entirely of formatting characters that
/// cannot own an inter-character tracking boundary.
pub(crate) fn text_is_inter_character_control_only(text: &str) -> bool {
    !text.is_empty() && text.chars().all(character_is_inter_character_control)
}

/// Return whether an inter-character justification gap is valid between two
/// already materialized text units.
///
/// CSS Text allows `text-justify: inter-character` to expand between adjacent
/// typographic character units, but expansion must not disrupt cursive joining
/// or turn Unicode controls into spacing points:
/// <https://www.w3.org/TR/css-text-3/#text-justify-property> and
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(crate) fn inter_character_gap_allowed_between_text(left: &str, right: &str) -> bool {
    text_allows_inter_character_gap_after(left) && text_allows_inter_character_gap_before(right)
}

pub(crate) fn text_allows_inter_character_gap_after(text: &str) -> bool {
    text.chars()
        .rev()
        .find(|character| !character_is_inter_character_control(*character))
        .is_some_and(|character| !character_blocks_inter_character_gap(character))
}

pub(crate) fn text_allows_inter_character_gap_before(text: &str) -> bool {
    text.chars()
        .find(|character| !character_is_inter_character_control(*character))
        .is_some_and(|character| !character_blocks_inter_character_gap(character))
}

/// Return whether `word-break: keep-all` suppresses a candidate break between
/// the two adjacent text units.
///
/// CSS Text preserves whitespace and punctuation wrap opportunities under
/// `keep-all`, while suppressing breaks within CJK and non-CJK word units:
/// <https://www.w3.org/TR/css-text-3/#word-break-property>.
pub(crate) fn keep_all_suppresses_break_between(previous: char, next: char) -> bool {
    keep_all_unbreakable_unit(previous) && keep_all_unbreakable_unit(next)
}

/// Return whether an intra-text break contributes to min-content sizing.
///
/// CSS Sizing min-content uses ordinary CSS Text soft wrap opportunities,
/// including normal UAX #14 ideographic boundaries. `keep-all` may suppress
/// such a boundary; `overflow-wrap:break-word` emergency opportunities are
/// intentionally handled by graph metadata and do not call this predicate:
/// <https://www.w3.org/TR/css-sizing-3/#min-content> and
/// <https://www.w3.org/TR/css-text-3/#overflow-wrap-property>.
pub(crate) fn text_break_is_min_content_eligible(
    text: &str,
    style: &ComputedStyle,
    byte_offset: usize,
) -> bool {
    if byte_offset == 0 || byte_offset >= text.len() || !text.is_char_boundary(byte_offset) {
        return false;
    }
    let previous = text[..byte_offset].chars().next_back();
    let next = text[byte_offset..].chars().next();
    if let (Some(previous), Some(next)) = (previous, next) {
        match style.word_break {
            CssWordBreak::KeepAll if keep_all_suppresses_break_between(previous, next) => {
                return false;
            }
            CssWordBreak::Manual if manual_suppresses_break_between(previous, next) => {
                return false;
            }
            _ => {}
        }
    }
    true
}

fn keep_all_unbreakable_unit(character: char) -> bool {
    character_is_unicode_alphanumeric(character)
        || matches!(line_break_class(character), LineBreak::Ideographic)
}

fn text_unit_blocks_inter_character_gap(text: &str) -> bool {
    text.chars().any(character_blocks_inter_character_gap)
}

fn text_unit_is_control_only(text: &str) -> bool {
    text.chars().all(character_is_inter_character_control)
}

fn character_blocks_inter_character_gap(character: char) -> bool {
    character_has_joining_behavior(character) || character_is_inter_character_control(character)
}

fn character_is_inter_character_control(character: char) -> bool {
    character_is_join_control(character)
        || character_is_unicode_control(character)
        || character_is_default_ignorable_code_point(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typographic_units_keep_grapheme_clusters_indivisible() {
        let text = "e\u{301}x";
        let units = typographic_unit_ranges(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();
        assert_eq!(units, vec!["e\u{301}", "x"]);
    }
    #[test]
    fn typographic_units_group_joining_sequences() {
        let text = "سلام";
        let units = typographic_unit_ranges(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();
        assert_eq!(units, vec!["سلام"]);
        assert!(!inter_character_gap_allowed_between_text("س", "ل"));
    }

    #[test]
    fn typographic_units_keep_bengali_yaphala_indivisible() {
        let text = "ক্য";
        let units = typographic_unit_ranges(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();

        assert_eq!(units, vec!["ক্য"]);
    }

    #[test]
    fn controls_are_transparent_to_tracking_boundaries() {
        for control in ['\u{200c}', '\u{200d}', '\u{200e}'] {
            let control = control.to_string();
            assert!(!inter_character_gap_allowed_between_text(&control, "a"));
            assert!(!inter_character_gap_allowed_between_text("a", &control));
            assert!(inter_character_gap_allowed_between_text(
                "a",
                &format!("{control}b")
            ));
            assert!(inter_character_gap_allowed_between_text(
                &format!("a{control}"),
                "b"
            ));
        }
    }

    #[test]
    fn zero_width_space_is_attached_to_the_preceding_typographic_unit() {
        let text = "12\u{200b}三";
        let units = typographic_unit_ranges(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();

        assert_eq!(units, vec!["1", "2\u{200b}", "三"]);
    }

    #[test]
    fn arabic_word_can_track_after_an_intervening_space() {
        assert!(!inter_character_gap_allowed_between_text("س", " "));
        assert!(inter_character_gap_allowed_between_text(" ", "a"));
        assert!(!inter_character_gap_allowed_between_text(" ", "ل"));
    }

    #[test]
    fn typographic_units_split_latin_and_cjk_units() {
        let text = "AB中文";
        let units = typographic_unit_ranges(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();
        assert_eq!(units, vec!["A", "B", "中", "文"]);
    }

    #[test]
    fn keep_all_policy_suppresses_word_units_but_not_space_or_punctuation() {
        assert!(keep_all_suppresses_break_between('A', '中'));
        assert!(keep_all_suppresses_break_between('中', '文'));
        assert!(!keep_all_suppresses_break_between('中', ' '));
        assert!(!keep_all_suppresses_break_between('。', '文'));
    }

    #[test]
    fn min_content_uses_ordinary_ideographic_soft_wraps() {
        let mut style = ComputedStyle::initial();
        let text = "中文";
        assert!(text_break_is_min_content_eligible(text, &style, "中".len()));

        style.word_break = CssWordBreak::KeepAll;
        assert!(!text_break_is_min_content_eligible(
            text,
            &style,
            "中".len()
        ));
    }

    #[test]
    fn min_content_manual_excludes_complex_context_dictionary_breaks() {
        let mut style = ComputedStyle::initial();
        style.word_break = CssWordBreak::Manual;
        let text = "กรุงเทพ";
        assert!(!text_break_is_min_content_eligible(text, &style, "ก".len()));
    }
}
