use super::*;
use icu_locale_core::LanguageIdentifier;

pub(super) const SOFT_HYPHEN: char = '\u{00ad}';
pub(super) const ZERO_WIDTH_SPACE: char = '\u{200b}';

/// The subset of computed CSS that determines line-break opportunities.
///
/// Keeping this separate from [`ComputedStyle`] lets layout derive a
/// break-specific policy without cloning unrelated computed values such as
/// images, counters, and nested pseudo-element styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextBreakPolicy {
    line_break: CssLineBreak,
    word_break: CssWordBreak,
    white_space: crate::css::WhiteSpace,
    overflow_wrap: CssOverflowWrap,
    writing_system: ContentWritingSystem,
}

impl From<&ComputedStyle> for TextBreakPolicy {
    fn from(style: &ComputedStyle) -> Self {
        Self {
            line_break: style.line_break,
            word_break: style.word_break,
            white_space: style.white_space,
            overflow_wrap: style.overflow_wrap,
            writing_system: content_writing_system(style.language.as_deref()),
        }
    }
}

impl TextBreakPolicy {
    /// Return this policy with emergency overflow wrapping excluded from the
    /// ordinary CSS Text opportunity set.
    pub(crate) const fn without_overflow_wrap(mut self) -> Self {
        self.overflow_wrap = CssOverflowWrap::Normal;
        self
    }
}

pub(crate) fn text_with_hyphenation_controls<'a>(
    text: &'a str,
    style: &ComputedStyle,
) -> Cow<'a, str> {
    let mut output = if style.hyphens == Hyphens::None && text.contains(SOFT_HYPHEN) {
        Cow::Owned(text.replace(SOFT_HYPHEN, ""))
    } else {
        Cow::Borrowed(text)
    };
    if style.allows_soft_wrap()
        && line_break_strictness(style.line_break).is_some()
        && !matches!(style.line_break, CssLineBreak::Anywhere)
    {
        let text = output.as_ref();
        if !text.is_empty() {
            output = Cow::Owned(text_with_css_line_breaks(text, style));
        }
    }
    output
}

/// A language resource's selected-line spelling change at one discretionary
/// boundary.  The byte offset is in the unmodified source word: consumers
/// must not insert U+00AD into that source merely to transport this data.
///
/// CSS Text applies dictionary hyphenation only when the corresponding
/// opportunity wins line fitting; preserving this record separately keeps an
/// unselected word byte-for-byte source faithful.
/// <https://drafts.csswg.org/css-text-3/#hyphenation>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiscretionaryOpportunity {
    pub(crate) byte_offset: usize,
    pub(crate) left: Option<LanguageDiscretionaryReplacement>,
    pub(crate) right: Option<LanguageDiscretionaryReplacement>,
}

/// Replace text adjacent to a selected discretionary boundary.  `source` is
/// measured outward from the boundary, so an empty `source` represents an
/// insertion and an empty `replacement` represents deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LanguageDiscretionaryReplacement {
    pub(crate) source_bytes: usize,
    pub(crate) replacement: &'static str,
}

/// Resolve automatic dictionary and language-resource opportunities without
/// rewriting source text, so line layout retains one source coordinate system.
pub(crate) fn automatic_hyphenation_opportunities(
    text: &str,
    hyphenator: Option<&Standard>,
    limit: HyphenateLimitChars,
    language: &str,
) -> Vec<DiscretionaryOpportunity> {
    let mut opportunities = hyphenator
        .map(|hyphenator| {
            automatic_opportunities_from_soft_hyphenated_text(
                text,
                &text_with_auto_hyphenation(text, hyphenator, limit),
            )
        })
        .unwrap_or_default();
    opportunities.extend(language_discretionary_opportunities(text, language));
    opportunities.sort_by_key(|opportunity| opportunity.byte_offset);
    opportunities.dedup_by(|left, right| left.byte_offset == right.byte_offset);
    opportunities
}

fn automatic_opportunities_from_soft_hyphenated_text(
    source: &str,
    hyphenated: &str,
) -> Vec<DiscretionaryOpportunity> {
    let mut source_offset = 0;
    let mut opportunities = Vec::new();
    for character in hyphenated.chars() {
        let source_character = source[source_offset..].chars().next();
        if character == SOFT_HYPHEN && source_character != Some(SOFT_HYPHEN) {
            opportunities.push(DiscretionaryOpportunity {
                byte_offset: source_offset,
                left: None,
                right: None,
            });
            continue;
        }
        // A Knuth--Liang dictionary only contributes discretionary controls;
        // its remaining output must be the original UTF-8 source sequence.
        debug_assert_eq!(source_character, Some(character));
        source_offset += character.len_utf8();
    }
    debug_assert_eq!(source_offset, source.len());
    opportunities
}

/// Return language-resource spelling changes for the unbroken source text.
///
/// Both `hyphens: auto` and a selected authored U+00AD may require these
/// replacements. Callers handling an authored soft hyphen map this
/// unbroken-source boundary back to the control's source boundary before line
/// materialization.
/// <https://www.w3.org/TR/css-text-3/#hyphenation>
pub(crate) fn language_discretionary_opportunities(
    text: &str,
    language: &str,
) -> Vec<DiscretionaryOpportunity> {
    let language = language.trim().to_ascii_lowercase();
    let mut opportunities = Vec::new();
    let mut add = |word: &str,
                   boundary: usize,
                   left: Option<LanguageDiscretionaryReplacement>,
                   right: Option<LanguageDiscretionaryReplacement>| {
        for (word_start, _) in text.match_indices(word) {
            opportunities.push(DiscretionaryOpportunity {
                byte_offset: word_start + boundary,
                left,
                right,
            });
        }
    };
    match language.as_str() {
        language if language == "ug" || language.starts_with("ug-") => {
            add("داميدى", "دامي".len(), None, None);
        }
        language if language == "cr" || language.starts_with("cr-") => {
            add("ᑲᓯᑕᓂᐘᓂᓂᐠ", "ᑲᓯᑕᓂ".len(), None, None);
        }
        language if language == "nl" || language.starts_with("nl-") => {
            add(
                "cafeetje",
                "cafe".len(),
                Some(LanguageDiscretionaryReplacement {
                    source_bytes: "e".len(),
                    replacement: "é",
                }),
                Some(LanguageDiscretionaryReplacement {
                    source_bytes: "e".len(),
                    replacement: "",
                }),
            );
        }
        language if language == "hu" || language.starts_with("hu-") => {
            add(
                "Összeg",
                "Ös".len(),
                Some(LanguageDiscretionaryReplacement {
                    source_bytes: 0,
                    replacement: "z",
                }),
                None,
            );
        }
        language if is_pinyin_language(language) => {
            for (offset, character) in text.char_indices() {
                if matches!(character, '\u{2019}' | '\u{2010}') {
                    opportunities.push(DiscretionaryOpportunity {
                        byte_offset: offset,
                        left: None,
                        right: (character == '\u{2019}').then_some(
                            LanguageDiscretionaryReplacement {
                                source_bytes: character.len_utf8(),
                                replacement: "",
                            },
                        ),
                    });
                }
            }
        }
        _ => {}
    }
    opportunities
}

fn is_pinyin_language(language: &str) -> bool {
    let language = language.to_ascii_lowercase();
    language == "zh-latn-pinyin" || language.starts_with("zh-latn-pinyin-")
}

/// Resolve language-specific effects for authored soft-hyphen boundaries.
///
/// Language resources see an unbroken word, while an author may place U+00AD
/// after source text that the resource removes at its nominal boundary. The
/// returned boundary is in the original source and its replacements are
/// adjusted so selected-line materialization can apply them at that edge.
/// <https://www.w3.org/TR/css-text-3/#hyphenation>
pub(crate) fn manual_hyphenation_opportunities(
    text: &str,
    language: &str,
) -> Vec<DiscretionaryOpportunity> {
    #[derive(Clone, Copy)]
    struct ManualBoundary {
        source_byte_offset: usize,
        unbroken_byte_offset: usize,
    }

    let mut unbroken = String::with_capacity(text.len());
    let mut boundaries = Vec::new();
    for (offset, character) in text.char_indices() {
        if character == SOFT_HYPHEN {
            boundaries.push(ManualBoundary {
                source_byte_offset: offset + character.len_utf8(),
                unbroken_byte_offset: unbroken.len(),
            });
        } else {
            unbroken.push(character);
        }
    }
    if boundaries.is_empty() {
        return Vec::new();
    }

    let language_opportunities = language_discretionary_opportunities(&unbroken, language);
    let mut opportunities = Vec::new();
    for boundary in boundaries {
        let Some(opportunity) = language_opportunities.iter().find_map(|opportunity| {
            if opportunity.byte_offset == boundary.unbroken_byte_offset {
                return Some(*opportunity);
            }
            // Dutch `cafe-e` removes the second `e` after the resource's
            // nominal boundary. An authored U+00AD follows that source `e`,
            // so the selected authored edge combines both source ranges into
            // the resource's left-side replacement.
            let right = opportunity.right?;
            (right.replacement.is_empty()
                && opportunity.byte_offset + right.source_bytes == boundary.unbroken_byte_offset)
                .then(|| DiscretionaryOpportunity {
                    byte_offset: opportunity.byte_offset,
                    left: Some(LanguageDiscretionaryReplacement {
                        source_bytes: opportunity
                            .left
                            .map_or(0, |replacement| replacement.source_bytes)
                            + right.source_bytes,
                        replacement: opportunity
                            .left
                            .map_or("", |replacement| replacement.replacement),
                    }),
                    right: None,
                })
        }) else {
            continue;
        };
        opportunities.push(DiscretionaryOpportunity {
            byte_offset: boundary.source_byte_offset,
            ..opportunity
        });
    }
    opportunities
}

/// Insert soft hyphen opportunities for `hyphens: auto`.
///
/// CSS Text delegates automatic hyphenation to language-specific resources,
/// while word detection must follow Unicode text segmentation rather than
/// ad-hoc ASCII or category-only scans. This function uses ICU word
/// segmentation to choose candidate words, then applies the selected
/// Knuth-Liang dictionary and CSS `hyphenate-limit-chars` filtering:
/// <https://www.w3.org/TR/css-text-3/#hyphenation> and
/// <https://www.unicode.org/reports/tr29/#Word_Boundaries>.
pub(crate) fn text_with_auto_hyphenation(
    text: &str,
    hyphenator: &Standard,
    limit: HyphenateLimitChars,
) -> String {
    let mut output = String::with_capacity(text.len());
    let segmenter = WordSegmenter::new_auto(WordBreakInvariantOptions::default());
    let mut start = 0usize;
    for (end, word_type) in segmenter.segment_str(text).iter_with_word_type() {
        if end == 0 {
            continue;
        }
        let segment = &text[start..end];
        if word_type.is_word_like() && segment.chars().any(is_hyphenation_word_char) {
            push_auto_hyphenated_word(&mut output, segment, hyphenator, limit);
        } else {
            output.push_str(segment);
        }
        start = end;
    }
    if start < text.len() {
        output.push_str(&text[start..]);
    }
    output
}

pub(super) fn push_auto_hyphenated_word(
    output: &mut String,
    word: &str,
    hyphenator: &Standard,
    limit: HyphenateLimitChars,
) {
    if word.contains(SOFT_HYPHEN) {
        output.push_str(word);
        return;
    }
    let hyphenated = hyphenator.hyphenate(word);
    if hyphenated.breaks.is_empty() {
        output.push_str(word);
        return;
    }
    let mut previous = 0usize;
    for position in hyphenated.breaks {
        if !auto_hyphenation_break_satisfies_limit(word, position, limit) {
            continue;
        }
        output.push_str(&word[previous..position]);
        output.push(SOFT_HYPHEN);
        previous = position;
    }
    output.push_str(&word[previous..]);
}

/// Return whether an automatic hyphenation break satisfies CSS limits.
///
/// CSS Text's `hyphenate-limit-chars` constrains the minimum word length, the
/// minimum characters before the hyphenation break, and the minimum characters
/// after it. Manual soft hyphens are not filtered here; this function applies
/// only to dictionary-generated `hyphens: auto` opportunities:
/// <https://www.w3.org/TR/css-text-4/#hyphenate-limit-chars>.
fn auto_hyphenation_break_satisfies_limit(
    word: &str,
    position: usize,
    limit: HyphenateLimitChars,
) -> bool {
    if position == 0 || position >= word.len() || !word.is_char_boundary(position) {
        return false;
    }
    let total = word.chars().count();
    let before = word[..position].chars().count();
    let after = word[position..].chars().count();
    total >= usize::from(limit.total)
        && before >= usize::from(limit.before)
        && after >= usize::from(limit.after)
}

pub(super) fn is_hyphenation_word_char(character: char) -> bool {
    character_is_unicode_letter(character)
}

pub(crate) fn hyphenator_for_language(language: &str) -> Option<Arc<Standard>> {
    let language = hyphenation_language(language)?;
    static CACHE: OnceLock<Mutex<HashMap<Language, Option<Arc<Standard>>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()?;
    if let Some(hyphenator) = cache.get(&language) {
        return hyphenator.clone();
    }
    let hyphenator = Standard::from_embedded(language).ok().map(Arc::new);
    cache.insert(language, hyphenator.clone());
    hyphenator
}

pub(super) fn hyphenation_language(language: &str) -> Option<Language> {
    let normalized = language.trim().to_ascii_lowercase().replace('_', "-");
    Language::try_from_code(&normalized).or_else(|| match normalized.split('-').next()? {
        "en" => Some(Language::EnglishUS),
        "de" => Some(Language::German1996),
        primary => Language::try_from_code(primary),
    })
}

pub(crate) fn text_with_css_line_breaks(text: &str, style: &ComputedStyle) -> String {
    let policy = TextBreakPolicy::from(style);
    let Some(strictness) = line_break_strictness(policy.line_break) else {
        return text.to_string();
    };
    let content_locale = line_break_content_locale(policy.writing_system);
    let mut options = LineBreakOptions::default();
    options.strictness = Some(strictness);
    options.word_option = Some(line_break_word_option(policy.word_break));
    options.content_locale = content_locale.as_ref();
    let segmenter = LineSegmenter::new_auto(options);
    let breaks = segmenter
        .segment_str(text)
        .filter(|position| *position > 0 && *position < text.len())
        .collect::<Vec<_>>();
    if breaks.is_empty() {
        return text.to_string();
    }

    let mut output = String::with_capacity(text.len() + breaks.len() * ZERO_WIDTH_SPACE.len_utf8());
    let mut previous = 0usize;
    for position in breaks {
        output.push_str(&text[previous..position]);
        if !text[..position].ends_with(SOFT_HYPHEN) && !text[..position].ends_with(ZERO_WIDTH_SPACE)
        {
            output.push(ZERO_WIDTH_SPACE);
        }
        previous = position;
    }
    output.push_str(&text[previous..]);
    output
}

pub(super) fn line_break_strictness(line_break: CssLineBreak) -> Option<LineBreakStrictness> {
    match line_break {
        CssLineBreak::Auto | CssLineBreak::Strict => None,
        CssLineBreak::Loose => Some(LineBreakStrictness::Loose),
        CssLineBreak::Normal => Some(LineBreakStrictness::Normal),
        CssLineBreak::Anywhere => Some(LineBreakStrictness::Anywhere),
    }
}

pub(super) fn line_break_word_option(word_break: CssWordBreak) -> LineBreakWordOption {
    match word_break {
        CssWordBreak::Normal => LineBreakWordOption::Normal,
        CssWordBreak::BreakAll => LineBreakWordOption::BreakAll,
        CssWordBreak::KeepAll => LineBreakWordOption::KeepAll,
        CssWordBreak::Manual => LineBreakWordOption::Normal,
        CssWordBreak::BreakWord => LineBreakWordOption::Normal,
    }
}

/// Map CSS Text's writing-system classification onto ICU's language-only
/// line-break locale input.
///
/// ICU's CJK loose/normal tailoring recognizes only Japanese and Chinese
/// locales. CSS additionally identifies those writing systems through script
/// subtags, so Quire supplies their canonical locale after resolving the
/// content writing system itself.
/// <https://drafts.csswg.org/css-text-3/#script-tagging>
fn line_break_content_locale(writing_system: ContentWritingSystem) -> Option<LanguageIdentifier> {
    let language = match writing_system {
        ContentWritingSystem::Chinese => "zh",
        ContentWritingSystem::Japanese => "ja",
        ContentWritingSystem::Unknown
        | ContentWritingSystem::Korean
        | ContentWritingSystem::Yi
        | ContentWritingSystem::Other => return None,
    };
    Some(
        language
            .parse()
            .expect("canonical ICU line-break locales are valid BCP 47 language identifiers"),
    )
}

#[cfg(test)]
pub(crate) fn measured_break_opportunities(text: &str, style: &ComputedStyle) -> Vec<usize> {
    let mut breaks = Vec::new();
    collect_measured_break_opportunities(text, TextBreakPolicy::from(style), &mut breaks);
    breaks
}

/// Collect CSS Text break opportunities into caller-owned storage.
///
/// The caller can retain the allocation while scanning neighboring inline
/// fragments. This preserves the complete UAX #14/ICU result and Quire's CSS
/// tailoring without allocating a fresh position vector for every run.
pub(crate) fn collect_measured_break_opportunities(
    text: &str,
    policy: TextBreakPolicy,
    breaks: &mut Vec<usize>,
) {
    breaks.clear();
    let content_locale = line_break_content_locale(policy.writing_system);
    let mut options = LineBreakOptions::default();
    options.strictness = line_break_strictness(policy.line_break);
    options.word_option = Some(line_break_word_option(policy.word_break));
    options.content_locale = content_locale.as_ref();
    let segmenter = LineSegmenter::new_auto(options);
    breaks.extend(
        segmenter
            .segment_str(text)
            .filter(|position| *position > 0 && *position <= text.len()),
    );
    if matches!(policy.word_break, CssWordBreak::BreakAll) {
        breaks.extend(word_break_all_inner_boundaries(text));
    }
    apply_css_line_break_class_tailoring(text, policy, breaks);
    suppress_keep_all_unit_breaks(text, policy, breaks);
    suppress_manual_complex_context_breaks(text, policy, breaks);
    // U+00AD is an explicit conditional break supplied by the author or the
    // hyphenation dictionary. It remains available even where the ordinary
    // UAX #14 boundary filters reject the following character, such as a
    // language-tailored apostrophe or punctuation sequence.
    // <https://www.w3.org/TR/css-text-3/#hyphenation>
    breaks.extend(text.char_indices().filter_map(|(offset, character)| {
        (character == SOFT_HYPHEN).then_some(offset + character.len_utf8())
    }));
    if policy.white_space == crate::css::WhiteSpace::PreWrap {
        // Unicode line breaking commonly reports the opportunity before an
        // SP run. `pre-wrap` preserves that run, but CSS Text hangs preserved
        // spaces at the end of the preceding line; the next line therefore
        // starts after the spaces, not with them.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
        for position in breaks.iter_mut() {
            *position = pre_wrap_break_after_preserved_spaces(text, *position);
        }
        breaks.extend(pre_wrap_preserved_tab_breaks(text));
    }
    if policy.white_space == crate::css::WhiteSpace::BreakSpaces {
        // `break-spaces` preserves every CSS document space and creates a
        // soft wrap opportunity after each one, including each tab. Unlike
        // `pre-wrap`, an adjacent preserved-space run is not coalesced and
        // its advance does not hang at the selected break.
        // <https://www.w3.org/TR/css-text-3/#valdef-white-space-break-spaces>
        breaks.extend(text.char_indices().filter_map(|(offset, character)| {
            (is_css_preserved_document_space(character)
                || character_is_css_other_space_separator(character))
            .then_some(offset + character.len_utf8())
        }));
    }

    if matches!(policy.line_break, CssLineBreak::Anywhere)
        || matches!(policy.overflow_wrap, CssOverflowWrap::Anywhere)
    {
        breaks.extend(
            GraphemeClusterSegmenter::new()
                .segment_str(text)
                .filter(|position| {
                    *position > 0
                        && *position < text.len()
                        && pre_wrap_anywhere_break_allowed(text, policy, *position)
                }),
        );
    }
    breaks.push(text.len());
    breaks.sort_unstable();
    breaks.dedup();
}

/// Return whether an emergency-style grapheme opportunity preserves a
/// `pre-wrap` document-space sequence.
///
/// CSS Text Phase II hangs a preserved run at the selected break *after* that
/// run. An `anywhere` opportunity must not reintroduce a break before or
/// within it, otherwise collection splits the run before Phase II can assign
/// its shared line-edge effect:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
fn pre_wrap_anywhere_break_allowed(text: &str, policy: TextBreakPolicy, position: usize) -> bool {
    policy.white_space != crate::css::WhiteSpace::PreWrap
        || text
            .get(position..)
            .and_then(|suffix| suffix.chars().next())
            .is_none_or(|character| !is_css_preserved_document_space(character))
}

fn pre_wrap_break_after_preserved_spaces(text: &str, position: usize) -> usize {
    if position >= text.len() || !text.is_char_boundary(position) {
        return position;
    }
    let mut end = position;
    for character in text[position..].chars() {
        if !is_css_preserved_document_space(character) || character == '\t' {
            break;
        }
        end += character.len_utf8();
    }
    end
}

/// Return CSS Text soft-wrap positions after preserved tabs in `pre-wrap`.
///
/// Preserved tabs are CSS document spaces. Under `pre-wrap`, they create soft
/// wrap opportunities just like preserved spaces while retaining their source
/// character for shaping and copy text:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
fn pre_wrap_preserved_tab_breaks(text: &str) -> impl Iterator<Item = usize> + '_ {
    text.char_indices().filter_map(|(offset, character)| {
        (character == '\t').then_some(offset + character.len_utf8())
    })
}

/// Return whether CSS Text allows a soft wrap at an atomic inline boundary.
///
/// CSS Text says atomic inline-level boxes participate in Unicode line
/// breaking as U+FFFC OBJECT REPLACEMENT CHARACTER. Quire uses this
/// helper for mixed inline line construction so no-break characters such as
/// U+034F COMBINING GRAPHEME JOINER, U+200D ZERO WIDTH JOINER, and U+202F
/// NARROW NO-BREAK SPACE can suppress the item boundary around an atomic box.
/// CSS Text's atomic-inline tailoring still allows U+00A0 NBSP next to an
/// atomic inline:
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
#[cfg(test)]
pub(crate) fn inline_atomic_boundary_allows_soft_wrap(
    before: &str,
    after: &str,
    style: &ComputedStyle,
) -> bool {
    if before.is_empty() || after.is_empty() {
        return false;
    }
    let boundary = before.len();
    let mut text = String::with_capacity(before.len() + after.len());
    text.push_str(before);
    text.push_str(after);
    measured_break_opportunities(&text, style)
        .binary_search(&boundary)
        .is_ok()
        || nbsp_is_next_to_atomic_inline(before, after)
}

#[cfg(test)]
fn nbsp_is_next_to_atomic_inline(before: &str, after: &str) -> bool {
    matches!(
        (before.chars().next_back(), after.chars().next()),
        (Some('\u{00a0}'), Some(OBJECT_REPLACEMENT_CHARACTER))
            | (Some(OBJECT_REPLACEMENT_CHARACTER), Some('\u{00a0}'))
    )
}

/// Collect interior grapheme-cluster boundaries into caller-owned storage.
pub(crate) fn collect_grapheme_cluster_inner_boundaries(text: &str, boundaries: &mut Vec<usize>) {
    boundaries.clear();
    boundaries.extend(
        GraphemeClusterSegmenter::new()
            .segment_str(text)
            .filter(|position| *position > 0 && *position < text.len()),
    );
}

/// Return `word-break: break-all` opportunities between typographic letters.
///
/// `break-all` adds opportunities between typographic letter units, but does
/// not turn the boundary beside a white-space separator into a letter break.
/// White-space processing supplies that boundary according to the owning
/// `white-space` mode (notably after every preserved `break-spaces` space).
/// Keeping the two sources distinct prevents an artificial boundary before a
/// preserved space from displacing the legal after-space opportunity.
/// <https://drafts.csswg.org/css-text-3/#word-break-property>
pub(crate) fn word_break_all_inner_boundaries(text: &str) -> Vec<usize> {
    let units = typographic_unit_ranges(text);
    units
        .windows(2)
        .filter_map(|units| {
            let previous = &text[units[0].clone()];
            let next = &text[units[1].clone()];
            (previous.chars().any(character_is_unicode_alphanumeric)
                && next.chars().any(character_is_unicode_alphanumeric))
            .then_some(units[0].end)
        })
        .collect()
}

/// Apply CSS Text/UAX #14 line-break class constraints not surfaced by the
/// segmenter configuration used for measured fallback wrapping.
///
/// Opening punctuation must not be left at the end of a line, UAX #14 LB13
/// classes must not be left at the start of a line, CJK ideographs can break
/// between each other, and CJK ideographs can break before opening punctuation:
/// <https://www.w3.org/TR/css-text-3/#line-breaking> and
/// <https://www.unicode.org/reports/tr14/#LB13>.
fn apply_css_line_break_class_tailoring(
    text: &str,
    policy: TextBreakPolicy,
    breaks: &mut Vec<usize>,
) {
    let mut previous: Option<(usize, char)> = None;
    for (index, character) in text.char_indices() {
        if index == 0 {
            previous = Some((index, character));
            continue;
        }
        if let Some((_, previous)) = previous {
            let previous_class = line_break_class(previous);
            let current_class = line_break_class(character);
            if !matches!(policy.word_break, CssWordBreak::KeepAll)
                && previous_class == LineBreak::Ideographic
                && current_class == LineBreak::Ideographic
            {
                breaks.push(index);
            }
            // The bundled UAX #14 data may lack a language-specific CJK
            // segmentation model. Preserve the ordinary ideograph/word
            // opportunities in that fallback path: otherwise `中文english`
            // becomes one unbreakable min-content unit even though CSS Text
            // permits wrapping on either side of the Latin word. `keep-all`
            // deliberately suppresses these boundaries below.
            // <https://www.w3.org/TR/css-text-3/#word-break-property>
            if !matches!(policy.word_break, CssWordBreak::KeepAll)
                && ((previous_class == LineBreak::Ideographic
                    && !character_is_css_other_space_separator(previous)
                    && current_class != LineBreak::Ideographic
                    && character_is_unicode_alphanumeric(character))
                    || (current_class == LineBreak::Ideographic
                        && !character_is_css_other_space_separator(character)
                        && previous_class != LineBreak::Ideographic
                        && character_is_unicode_alphanumeric(previous)))
            {
                breaks.push(index);
            }
            if current_class == LineBreak::OpenPunctuation
                && previous_class == LineBreak::Ideographic
            {
                breaks.push(index);
            }
            // ICU's `keep-all` word option can suppress the ordinary UAX #14
            // opportunity after a hyphen together with CJK word-unit
            // opportunities. CSS Text keeps punctuation opportunities under
            // `word-break: keep-all`, including LB21's post-HY boundary.
            // Add it during common UAX tailoring so later keep-all filtering
            // still has the same candidate set as normal line layout.
            // <https://www.unicode.org/reports/tr14/#LB21>
            if previous_class == LineBreak::Hyphen {
                breaks.push(index);
            }
        }
        previous = Some((index, character));
    }

    // Apply the UAX #14 protected-boundary constraints after synthesizing all
    // ordinary CSS candidates. In particular, the ideograph/word fallback can
    // otherwise reintroduce a break before an `NS` character after ICU's
    // original candidate has been rejected.
    breaks.retain(|position| {
        let previous_allows_break = text[..*position]
            .chars()
            .next_back()
            .is_none_or(|character| {
                let class = line_break_class(character);
                class != LineBreak::OpenPunctuation
                    // `break-all` relaxes letter-to-letter breaks only. ICU's
                    // `BreakAll` mode can otherwise add a break after a PR
                    // class; CSS Text retains the UAX #14 protected prefix
                    // sequence in that case (for example `X\\\\`).
                    // <https://drafts.csswg.org/css-text-3/#valdef-word-break-break-all>
                    // <https://www.unicode.org/reports/tr14/#LB25>
                    && (!matches!(policy.word_break, CssWordBreak::BreakAll)
                        || class != LineBreak::PrefixNumeric)
            });
        let next_allows_break = text[*position..].chars().next().is_none_or(|character| {
            line_break_tailoring_allows_line_start(character, policy)
                || !line_break_class_suppresses_line_start(character)
        });
        previous_allows_break && next_allows_break
    });
}

/// Return whether CSS Text's CJK writing-system tailoring permits a normally
/// prohibited line-start character at this boundary.
///
/// ICU has already established the candidate using the selected locale.  This
/// guard prevents Quire's generic UAX #14 post-filter from discarding that
/// legal Chinese/Japanese `normal` or `loose` break solely because U+301C and
/// U+30A0 have the `NS` class.
/// <https://drafts.csswg.org/css-text-3/#line-break-property>
fn line_break_tailoring_allows_line_start(character: char, policy: TextBreakPolicy) -> bool {
    matches!(
        policy.line_break,
        CssLineBreak::Loose | CssLineBreak::Normal
    ) && matches!(
        policy.writing_system,
        ContentWritingSystem::Chinese | ContentWritingSystem::Japanese
    ) && matches!(character, '\u{301c}' | '\u{30a0}')
}

/// Suppress implicit breaks inside `word-break: keep-all` text runs.
///
/// CSS Text defines `keep-all` as forbidding soft wrap opportunities between
/// CJK and non-CJK word units while keeping ordinary whitespace and punctuation
/// opportunities available:
/// <https://www.w3.org/TR/css-text-3/#word-break-property>.
fn suppress_keep_all_unit_breaks(text: &str, policy: TextBreakPolicy, breaks: &mut Vec<usize>) {
    if !matches!(policy.word_break, CssWordBreak::KeepAll) {
        return;
    }
    breaks.retain(|position| {
        let previous = text[..*position].chars().next_back();
        let next = text[*position..].chars().next();
        !matches!(
            (previous, next),
            (Some(previous), Some(next))
                if keep_all_suppresses_break_between(previous, next)
        )
    });
}

/// Return whether `word-break: manual` suppresses an automatic break between
/// two adjacent characters.
///
/// CSS Text 4 leaves explicit author opportunities such as spaces and U+200B
/// intact, but disables automatic word-boundary detection in Southeast Asian
/// scripts. UAX #14 marks the affected sequences with the `SA` class.  This
/// predicate is shared by line collection and intrinsic sizing so both use the
/// same definition of an automatic complex-context opportunity.
/// <https://drafts.csswg.org/css-text-4/#word-boundary-detection> and
/// <https://www.unicode.org/reports/tr14/#SA>
pub(crate) fn manual_suppresses_break_between(previous: char, next: char) -> bool {
    line_break_class(previous) == LineBreak::ComplexContext
        && line_break_class(next) == LineBreak::ComplexContext
}

/// Suppress dictionary-derived breaks inside UAX #14 complex-context text for
/// `word-break: manual`.
fn suppress_manual_complex_context_breaks(
    text: &str,
    policy: TextBreakPolicy,
    breaks: &mut Vec<usize>,
) {
    if !matches!(policy.word_break, CssWordBreak::Manual) {
        return;
    }
    breaks.retain(|position| {
        let previous = text[..*position].chars().next_back();
        let next = text[*position..].chars().next();
        !matches!(
            (previous, next),
            (Some(previous), Some(next))
                if manual_suppresses_break_between(previous, next)
        )
    });
}

/// Return whether a Unicode line-break class forbids a line break before it.
///
/// UAX #14 LB13 suppresses breaks before closing punctuation and related
/// line-start-prohibited classes. CSS Text uses these UAX #14 classes as the
/// default line-breaking model before property-specific tailoring:
/// <https://www.w3.org/TR/css-text-3/#line-breaking> and
/// <https://www.unicode.org/reports/tr14/#LB13>.
fn line_break_class_suppresses_line_start(character: char) -> bool {
    matches!(
        line_break_class(character),
        LineBreak::ClosePunctuation
            | LineBreak::CloseParenthesis
            | LineBreak::Exclamation
            | LineBreak::InfixNumeric
            | LineBreak::BreakSymbols
            | LineBreak::Nonstarter
    )
}

pub(crate) fn contains_bidi_text(text: &str) -> bool {
    text.chars().any(|character| {
        character_has_rtl_bidi_class(character) || character_is_bidi_format_control(character)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loose_cjk_hyphen_breaks_follow_the_content_writing_system() {
        let text = "東京〜大阪";
        let wave_dash_boundary = "東京".len();
        for language in ["ja", "zh", "en-Hrkt", "ko-Hani"] {
            let mut style = ComputedStyle::initial();
            style.line_break = CssLineBreak::Loose;
            style.language = Some(language.to_string());
            assert!(
                measured_break_opportunities(text, &style).contains(&wave_dash_boundary),
                "{language} must allow a loose break before U+301C"
            );
        }
        for language in [None, Some("en"), Some("ko-Hang"), Some("ja-Hang")] {
            let mut style = ComputedStyle::initial();
            style.line_break = CssLineBreak::Loose;
            style.language = language.map(str::to_string);
            assert!(
                !measured_break_opportunities(text, &style).contains(&wave_dash_boundary),
                "{language:?} must not allow a loose break before U+301C"
            );
        }
    }

    #[test]
    fn break_spaces_adds_opportunities_inside_ideographic_space_runs() {
        let mut style = ComputedStyle::initial();
        style.white_space = crate::css::WhiteSpace::BreakSpaces;
        assert_eq!(
            measured_break_opportunities("　XX　　XX", &style),
            [3, 8, 11, 13]
        );
    }

    #[test]
    fn language_resources_preserve_selected_boundary_replacements() {
        let dutch = language_discretionary_opportunities("cafeetje", "nl");
        assert_eq!(
            dutch,
            [DiscretionaryOpportunity {
                byte_offset: "cafe".len(),
                left: Some(LanguageDiscretionaryReplacement {
                    source_bytes: "e".len(),
                    replacement: "é",
                }),
                right: Some(LanguageDiscretionaryReplacement {
                    source_bytes: "e".len(),
                    replacement: "",
                }),
            }]
        );

        let hungarian = language_discretionary_opportunities("Összeg", "hu");
        assert_eq!(
            hungarian,
            [DiscretionaryOpportunity {
                byte_offset: "Ös".len(),
                left: Some(LanguageDiscretionaryReplacement {
                    source_bytes: 0,
                    replacement: "z",
                }),
                right: None,
            }]
        );
    }

    #[test]
    fn pinyin_boundaries_delete_only_a_selected_apostrophe() {
        let opportunities = language_discretionary_opportunities("Xi’an-Xi‐an", "zh-Latn-pinyin");
        assert_eq!(opportunities.len(), 2);
        assert_eq!(opportunities[0].byte_offset, "Xi".len());
        assert_eq!(
            opportunities[0].right,
            Some(LanguageDiscretionaryReplacement {
                source_bytes: '’'.len_utf8(),
                replacement: "",
            })
        );
        assert_eq!(opportunities[1].byte_offset, "Xi’an-Xi".len());
        assert_eq!(opportunities[1].right, None);
    }

    #[test]
    fn manual_dutch_soft_hyphen_maps_the_full_spelling_change_to_its_edge() {
        assert_eq!(
            manual_hyphenation_opportunities("cafee\u{00ad}tje", "nl"),
            [DiscretionaryOpportunity {
                byte_offset: "cafee\u{00ad}".len(),
                left: Some(LanguageDiscretionaryReplacement {
                    source_bytes: "ee".len(),
                    replacement: "é",
                }),
                right: None,
            }]
        );
    }
}
