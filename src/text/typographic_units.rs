use super::*;

/// CSS Text units whose boundaries must preserve cursive joining.
///
/// This is intentionally distinct from [`LineBreakAnywhereUnitRanges`]. CSS
/// Text permits typographic character units to be tailored for each operation:
/// inter-character spacing cannot introduce a gap within a cursive joining
/// sequence, while `line-break:anywhere` may wrap there:
/// <https://www.w3.org/TR/css-text-3/#typographic-character-unit>,
/// <https://www.w3.org/TR/css-text-3/#text-justify-property>, and
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
#[derive(Debug, Clone)]
pub(crate) struct CursiveProtectedUnitRanges(Vec<Range<usize>>);

impl CursiveProtectedUnitRanges {
    pub(crate) fn new(text: &str) -> Self {
        let mut ranges = Vec::<Range<usize>>::new();
        let mut previous_blocks_gap = false;
        for range in extended_grapheme_cluster_ranges(text) {
            let unit = &text[range.clone()];
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
        Self(ranges)
    }

    /// Iterate ranges safe to use as inter-character spacing boundaries.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Range<usize>> {
        self.0.iter()
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl IntoIterator for CursiveProtectedUnitRanges {
    type Item = Range<usize>;
    type IntoIter = std::vec::IntoIter<Range<usize>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// CSS Text units that create `line-break:anywhere` opportunities.
///
/// These units keep extended grapheme clusters intact and attach a run of
/// default-ignorable or join controls to its following visible unit (or the
/// preceding one at end of text). They deliberately do *not* coalesce cursive
/// letters: a soft wrap at one of their boundaries reuses the complete shaped
/// source run, preserving its contextual glyph forms.
/// <https://drafts.csswg.org/css-text-3/#typographic-character-unit> and
/// <https://drafts.csswg.org/css-text-3/#valdef-line-break-anywhere>.
#[derive(Debug, Clone)]
pub(crate) struct LineBreakAnywhereUnitRanges {
    ranges: Vec<Range<usize>>,
    text_len: usize,
}

impl LineBreakAnywhereUnitRanges {
    pub(crate) fn new(text: &str) -> Self {
        let mut unit_ends = extended_grapheme_cluster_ranges(text)
            .into_iter()
            .map(|range| range.end)
            .collect::<Vec<_>>();
        let mut pending_control_start = None;
        for (offset, character) in text.char_indices() {
            if character_is_line_break_anywhere_control(character) {
                pending_control_start.get_or_insert(offset);
                continue;
            }
            let Some(start) = pending_control_start.take() else {
                continue;
            };
            unit_ends.retain(|position| *position <= start || *position > offset);
            unit_ends.push(start);
        }
        unit_ends.sort_unstable();
        unit_ends.dedup();

        let mut start = 0;
        let mut ranges = Vec::new();
        for end in unit_ends {
            if end > start {
                ranges.push(start..end);
                start = end;
            }
        }
        Self {
            ranges,
            text_len: text.len(),
        }
    }

    /// Return whether `position` is the end of one of these units.
    pub(crate) fn contains_boundary(&self, position: usize) -> bool {
        self.ranges
            .binary_search_by_key(&position, |range| range.end)
            .is_ok()
    }

    /// Iterate only internal source boundaries eligible for this property.
    pub(crate) fn internal_boundaries(&self) -> impl Iterator<Item = usize> + '_ {
        self.ranges
            .iter()
            .map(|range| range.end)
            .filter(|position| *position > 0 && *position < self.text_len)
    }
}

/// Return the extended grapheme clusters from which CSS Text operations build
/// their own typographic units.
fn extended_grapheme_cluster_ranges(text: &str) -> Vec<Range<usize>> {
    GraphemeClusterSegmenter::new()
        .segment_str(text)
        .collect::<Vec<_>>()
        .windows(2)
        .filter_map(|window| {
            let range = window[0]..window[1];
            (!range.is_empty()).then_some(range)
        })
        .collect()
}

fn character_is_line_break_anywhere_control(character: char) -> bool {
    character_is_default_ignorable_code_point(character) || character_is_join_control(character)
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
    character_has_cursive_shaping_behavior(character)
        || character_is_inter_character_control(character)
}

pub(crate) fn character_is_inter_character_control(character: char) -> bool {
    character_is_join_control(character)
        || character_is_unicode_control(character)
        || character_is_default_ignorable_code_point(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursive_protected_units_keep_grapheme_clusters_indivisible() {
        let text = "e\u{301}x";
        let units = CursiveProtectedUnitRanges::new(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();
        assert_eq!(units, vec!["e\u{301}", "x"]);
    }
    #[test]
    fn cursive_protected_units_group_joining_sequences() {
        let text = "سلام";
        let units = CursiveProtectedUnitRanges::new(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();
        assert_eq!(units, vec!["سلام"]);
        assert!(!inter_character_gap_allowed_between_text("س", "ل"));
    }

    #[test]
    fn cursive_protected_units_keep_bengali_yaphala_indivisible() {
        let text = "ক্য";
        let units = CursiveProtectedUnitRanges::new(text)
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
    fn zero_width_space_is_attached_to_the_preceding_cursive_protected_unit() {
        let text = "12\u{200b}三";
        let units = CursiveProtectedUnitRanges::new(text)
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
    fn cursive_protected_units_split_latin_and_cjk_units() {
        let text = "AB中文";
        let units = CursiveProtectedUnitRanges::new(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();
        assert_eq!(units, vec!["A", "B", "中", "文"]);
    }

    #[test]
    fn cursive_protected_units_split_latin_around_halfwidth_katakana_middle_dot() {
        let text = "a･a";
        let units = CursiveProtectedUnitRanges::new(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();

        assert_eq!(units, vec!["a", "･", "a"]);
    }

    #[test]
    fn line_break_anywhere_units_expose_arabic_joining_boundaries() {
        let text = "عائلة";
        let cursive_units = CursiveProtectedUnitRanges::new(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();
        assert_eq!(cursive_units, vec!["عائلة"]);
        assert_eq!(
            LineBreakAnywhereUnitRanges::new(text)
                .internal_boundaries()
                .map(|position| &text[..position])
                .collect::<Vec<_>>(),
            vec!["ع", "عا", "عائ", "عائل"]
        );
    }

    #[test]
    fn line_break_anywhere_units_attach_controls_to_visible_neighbors() {
        for (text, expected) in [
            ("\u{200d}ab", vec!["\u{200d}a"]),
            ("a\u{200d}b", vec!["a"]),
            ("a\u{200d}", vec![]),
            ("a\u{034f}b", vec!["a"]),
        ] {
            let boundaries = LineBreakAnywhereUnitRanges::new(text)
                .internal_boundaries()
                .map(|position| &text[..position])
                .collect::<Vec<_>>();
            assert_eq!(boundaries, expected, "{text:?}");
        }
    }

    #[test]
    fn keep_all_policy_suppresses_word_units_but_not_space_or_punctuation() {
        assert!(keep_all_suppresses_break_between('A', '中'));
        assert!(keep_all_suppresses_break_between('中', '文'));
        assert!(!keep_all_suppresses_break_between('中', ' '));
        assert!(!keep_all_suppresses_break_between('。', '文'));
    }
}
