use super::*;

pub(super) const SOFT_HYPHEN: char = '\u{00ad}';
pub(super) const ZERO_WIDTH_SPACE: char = '\u{200b}';

#[allow(dead_code)]
pub(super) fn line_ends_with_visible_soft_hyphen(
    text: &str,
    range: &Range<usize>,
    break_reason: BreakReason,
) -> bool {
    matches!(break_reason, BreakReason::Regular | BreakReason::Emergency)
        && range.end > range.start
        && text[..range.end].ends_with(SOFT_HYPHEN)
}

#[allow(dead_code)]
pub(super) fn normalize_soft_hyphens(
    mut text: String,
    visible_trailing_soft_hyphen: bool,
) -> String {
    if text.contains(ZERO_WIDTH_SPACE) {
        text = text.replace(ZERO_WIDTH_SPACE, "");
    }
    if !text.contains(SOFT_HYPHEN) {
        return text;
    }
    if visible_trailing_soft_hyphen && text.ends_with(SOFT_HYPHEN) {
        text.pop();
        text = text.replace(SOFT_HYPHEN, "");
        text.push('-');
        text
    } else {
        text.replace(SOFT_HYPHEN, "")
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
    if style.hyphens == Hyphens::Auto
        && let Some(language) = style.language.as_deref()
        && let Some(hyphenator) = hyphenator_for_language(language)
    {
        let text = output.as_ref();
        if !text.is_empty() {
            output = Cow::Owned(text_with_auto_hyphenation(
                text,
                &hyphenator,
                style.hyphenate_limit_chars,
            ));
        }
    }
    if style.white_space.allows_soft_wrap() && line_break_strictness(style.line_break).is_some() {
        let text = output.as_ref();
        if !text.is_empty() {
            output = Cow::Owned(text_with_css_line_breaks(text, style));
        }
    }
    output
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
pub(super) fn text_with_auto_hyphenation(
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

pub(super) fn hyphenator_for_language(language: &str) -> Option<Arc<Standard>> {
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

pub(super) fn text_with_css_line_breaks(text: &str, style: &ComputedStyle) -> String {
    let Some(strictness) = line_break_strictness(style.line_break) else {
        return text.to_string();
    };
    let mut options = LineBreakOptions::default();
    options.strictness = Some(strictness);
    options.word_option = Some(line_break_word_option(style.word_break));
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
    }
}

pub(crate) fn measured_break_opportunities(text: &str, style: &ComputedStyle) -> Vec<usize> {
    let mut options = LineBreakOptions::default();
    options.strictness = line_break_strictness(style.line_break);
    options.word_option = Some(line_break_word_option(style.word_break));
    let segmenter = LineSegmenter::new_auto(options);
    let mut breaks = segmenter
        .segment_str(text)
        .filter(|position| *position > 0 && *position <= text.len())
        .collect::<Vec<_>>();
    apply_css_line_break_class_tailoring(text, style, &mut breaks);
    suppress_keep_all_unit_breaks(text, style, &mut breaks);
    if style.white_space == crate::css::WhiteSpace::PreWrap {
        breaks.extend(pre_wrap_preserved_tab_breaks(text));
    }

    if matches!(style.word_break, CssWordBreak::BreakAll)
        || matches!(style.line_break, CssLineBreak::Anywhere)
        || matches!(style.overflow_wrap, CssOverflowWrap::Anywhere)
    {
        breaks.extend(grapheme_cluster_inner_boundaries(text));
    }
    breaks.push(text.len());
    breaks.sort_unstable();
    breaks.dedup();
    breaks
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
/// breaking as U+FFFC OBJECT REPLACEMENT CHARACTER. Reasyprint uses this
/// helper for mixed inline line construction so no-break characters such as
/// U+034F COMBINING GRAPHEME JOINER, U+200D ZERO WIDTH JOINER, and U+202F
/// NARROW NO-BREAK SPACE can suppress the item boundary around an atomic box.
/// CSS Text's atomic-inline tailoring still allows U+00A0 NBSP next to an
/// atomic inline:
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
#[allow(dead_code)]
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

fn nbsp_is_next_to_atomic_inline(before: &str, after: &str) -> bool {
    matches!(
        (before.chars().next_back(), after.chars().next()),
        (Some('\u{00a0}'), Some(OBJECT_REPLACEMENT_CHARACTER))
            | (Some(OBJECT_REPLACEMENT_CHARACTER), Some('\u{00a0}'))
    )
}

pub(crate) fn grapheme_cluster_inner_boundaries(text: &str) -> Vec<usize> {
    GraphemeClusterSegmenter::new()
        .segment_str(text)
        .filter(|position| *position > 0 && *position < text.len())
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
    style: &ComputedStyle,
    breaks: &mut Vec<usize>,
) {
    breaks.retain(|position| {
        let previous_allows_break = text[..*position]
            .chars()
            .next_back()
            .is_none_or(|character| line_break_class(character) != LineBreak::OpenPunctuation);
        let next_allows_break = text[*position..]
            .chars()
            .next()
            .is_none_or(|character| !line_break_class_suppresses_line_start(character));
        previous_allows_break && next_allows_break
    });

    let mut previous: Option<(usize, char)> = None;
    for (index, character) in text.char_indices() {
        if index == 0 {
            previous = Some((index, character));
            continue;
        }
        if let Some((_, previous)) = previous {
            let previous_class = line_break_class(previous);
            let current_class = line_break_class(character);
            if !matches!(style.word_break, CssWordBreak::KeepAll)
                && previous_class == LineBreak::Ideographic
                && current_class == LineBreak::Ideographic
            {
                breaks.push(index);
            }
            if current_class == LineBreak::OpenPunctuation
                && previous_class == LineBreak::Ideographic
            {
                breaks.push(index);
            }
        }
        previous = Some((index, character));
    }
}

/// Suppress implicit breaks inside `word-break: keep-all` text runs.
///
/// CSS Text defines `keep-all` as forbidding soft wrap opportunities between
/// CJK and non-CJK word units while keeping ordinary whitespace and punctuation
/// opportunities available:
/// <https://www.w3.org/TR/css-text-3/#word-break-property>.
fn suppress_keep_all_unit_breaks(text: &str, style: &ComputedStyle, breaks: &mut Vec<usize>) {
    if !matches!(style.word_break, CssWordBreak::KeepAll) {
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

#[allow(dead_code)]
pub(super) fn measured_emergency_breaks_allowed(style: &ComputedStyle) -> bool {
    matches!(style.word_break, CssWordBreak::BreakAll)
        || matches!(style.line_break, CssLineBreak::Anywhere)
        || matches!(
            style.overflow_wrap,
            CssOverflowWrap::Anywhere | CssOverflowWrap::BreakWord
        )
}

pub(crate) fn contains_bidi_text(text: &str) -> bool {
    text.chars().any(|character| {
        character_has_rtl_bidi_class(character) || character_is_bidi_format_control(character)
    })
}
