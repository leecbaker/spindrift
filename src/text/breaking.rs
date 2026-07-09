use super::*;

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
}

impl From<&ComputedStyle> for TextBreakPolicy {
    fn from(style: &ComputedStyle) -> Self {
        Self {
            line_break: style.line_break,
            word_break: style.word_break,
            white_space: style.white_space,
            overflow_wrap: style.overflow_wrap,
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
        CssWordBreak::Manual => LineBreakWordOption::Normal,
        CssWordBreak::BreakWord => LineBreakWordOption::Normal,
    }
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
    let mut options = LineBreakOptions::default();
    options.strictness = line_break_strictness(policy.line_break);
    options.word_option = Some(line_break_word_option(policy.word_break));
    let segmenter = LineSegmenter::new_auto(options);
    breaks.extend(
        segmenter
            .segment_str(text)
            .filter(|position| *position > 0 && *position <= text.len()),
    );
    apply_css_line_break_class_tailoring(text, policy, breaks);
    suppress_keep_all_unit_breaks(text, policy, breaks);
    suppress_manual_complex_context_breaks(text, policy, breaks);
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

    if matches!(policy.word_break, CssWordBreak::BreakAll) {
        breaks.extend(word_break_all_inner_boundaries(text));
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
    fn break_spaces_adds_opportunities_inside_ideographic_space_runs() {
        let mut style = ComputedStyle::initial();
        style.white_space = crate::css::WhiteSpace::BreakSpaces;
        assert_eq!(
            measured_break_opportunities("　XX　　XX", &style),
            [3, 8, 11, 13]
        );
    }
}
