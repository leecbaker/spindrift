use super::*;
use crate::text::character_is_unicode_letter;
use icu_casemap::{CaseMapper, TitlecaseMapper, options::TitlecaseOptions};
use icu_locale_core::LanguageIdentifier;
use icu_segmenter::{WordSegmenter, options::WordBreakInvariantOptions};

pub(super) fn evaluate_bookmark_label(element: &Element, style: &ComputedStyle) -> String {
    let mut output = String::new();
    for part in &style.bookmark_label.parts {
        match part {
            BookmarkLabelPart::String(text) => output.push_str(text),
            BookmarkLabelPart::ContentText => output.push_str(&inline_text(element)),
            BookmarkLabelPart::Attr(name) => {
                if let Some(value) = element.attrs.get(name) {
                    output.push_str(value);
                }
            }
        }
    }
    output
}

pub(super) fn evaluate_generated_content_text(
    element: &Element,
    content: &[GeneratedContentPart],
    counter_stack: &HashMap<String, Vec<i32>>,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> String {
    let mut output = String::new();
    for part in content {
        match part {
            GeneratedContentPart::Text(text) => output.push_str(text),
            GeneratedContentPart::Contents => output.push_str(&inline_text(element)),
            GeneratedContentPart::Attr { name, fallback } => {
                if let Some(value) = element.attrs.get(name) {
                    output.push_str(value);
                } else if let Some(fallback) = fallback {
                    output.push_str(fallback);
                }
            }
            GeneratedContentPart::Counter {
                name,
                style: counter_style,
            } => {
                let value = counter_stack
                    .get(name)
                    .and_then(|values| values.last().copied())
                    .unwrap_or(0);
                if let Some(counter) = list::counter_text(
                    counter_style.clone().unwrap_or(ListStyleType::Decimal),
                    value,
                    counter_styles,
                ) {
                    output.push_str(&counter);
                }
            }
            GeneratedContentPart::Counters {
                name,
                separator,
                style: counter_style,
            } => {
                let style = counter_style.clone().unwrap_or(ListStyleType::Decimal);
                let counters = counter_stack
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| vec![0])
                    .into_iter()
                    .filter_map(|value| list::counter_text(style.clone(), value, counter_styles))
                    .collect::<Vec<_>>();
                output.push_str(&counters.join(separator));
            }
            GeneratedContentPart::Image { .. } => {}
            GeneratedContentPart::Quote(_) => {}
            GeneratedContentPart::Leader(text) => output.push_str(text),
        }
    }
    output
}

pub(super) fn evaluate_generated_alt_text(
    element: &Element,
    content: &[GeneratedAltTextPart],
    counter_stack: &HashMap<String, Vec<i32>>,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> String {
    let mut output = String::new();
    for part in content {
        match part {
            GeneratedAltTextPart::Text(text) => output.push_str(text),
            GeneratedAltTextPart::Attr { name, fallback } => {
                if let Some(value) = element.attrs.get(name) {
                    output.push_str(value);
                } else if let Some(fallback) = fallback {
                    output.push_str(fallback);
                }
            }
            GeneratedAltTextPart::Counter {
                name,
                style: counter_style,
            } => {
                let value = counter_stack
                    .get(name)
                    .and_then(|values| values.last().copied())
                    .unwrap_or(0);
                if let Some(counter) = list::counter_text(
                    counter_style.clone().unwrap_or(ListStyleType::Decimal),
                    value,
                    counter_styles,
                ) {
                    output.push_str(&counter);
                }
            }
            GeneratedAltTextPart::Counters {
                name,
                separator,
                style: counter_style,
            } => {
                let style = counter_style.clone().unwrap_or(ListStyleType::Decimal);
                let counters = counter_stack
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| vec![0])
                    .into_iter()
                    .filter_map(|value| list::counter_text(style.clone(), value, counter_styles))
                    .collect::<Vec<_>>();
                output.push_str(&counters.join(separator));
            }
        }
    }
    output
}

/// Returns the logical alignment that applies to one inline line box.
///
/// CSS Text applies `text-align-last` only to the last line of a block or to a
/// line before a forced break. `auto` keeps ordinary `text-align` behavior,
/// except that a justified affected line falls back to logical start:
/// <https://www.w3.org/TR/css-text-3/#text-align-last-property>.
pub(super) fn text_align_for_inline_line(style: &ComputedStyle, is_last_line: bool) -> TextAlign {
    if is_last_line {
        logical_text_align_last(style)
    } else {
        style.text_align
    }
}

fn logical_text_align_last(style: &ComputedStyle) -> TextAlign {
    match style.text_align_last {
        TextAlignLast::Align(align) => align,
        TextAlignLast::Auto => match style.text_align {
            TextAlign::Justify => TextAlign::Start,
            TextAlign::JustifyAll => TextAlign::Justify,
            align => align,
        },
    }
}

/// Returns the alignment that applies to one inline line box with line text.
///
/// CSS Writing Modes `unicode-bidi: plaintext` resolves each plaintext line's
/// base direction from its own first strong character. CSS Text `start` and
/// `end` alignment then resolve against that line direction rather than the
/// containing block's inherited `direction`:
/// <https://www.w3.org/TR/css-writing-modes-4/#valdef-unicode-bidi-plaintext>
/// and <https://www.w3.org/TR/css-text-3/#text-align-property>.
/// Returns line alignment while carrying plaintext paragraph direction state.
///
/// CSS Text says `unicode-bidi: plaintext` resolves paragraph direction using
/// UAX #9 P2/P3. Paragraphs without strong characters use the previous
/// paragraph direction when available, otherwise the containing block
/// direction; `text-align: start/end` resolves against that used direction:
/// <https://www.w3.org/TR/css-text-3/#bidi-linebox> and
/// <https://www.unicode.org/reports/tr9/#P2>.
pub(super) fn text_align_for_inline_line_text_with_state(
    style: &ComputedStyle,
    is_last_line: bool,
    line_text: &str,
    plaintext_direction_state: &mut Option<Direction>,
) -> TextAlign {
    let mut effective_style;
    let style = if style.unicode_bidi == UnicodeBidi::Plaintext {
        let direction = plaintext_direction_for_text(line_text)
            .or(*plaintext_direction_state)
            .unwrap_or(style.direction);
        *plaintext_direction_state = Some(direction);
        effective_style = style.clone();
        effective_style.direction = direction;
        &effective_style
    } else {
        style
    };
    text_align_for_inline_line(style, is_last_line)
}

/// Returns the used inline offset for one formatted line.
///
/// CSS Text applies `text-indent` to the first formatted line of a block
/// container and, with `each-line`, to lines after forced line breaks while
/// excluding soft wraps. Percentages resolve against the containing block's
/// inline size; existing caller-supplied hanging indents are retained for later
/// line offsets:
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>.
pub(super) fn used_line_indent(
    line_index: usize,
    starts_after_forced_break: bool,
    hanging_indent: f32,
    style: &ComputedStyle,
    available_width: f32,
) -> f32 {
    let is_indent_line =
        line_index == 0 || (style.text_indent.each_line && starts_after_forced_break);
    let applies_text_indent = is_indent_line != style.text_indent.hanging;
    let text_indent = if applies_text_indent {
        used_text_indent(style, available_width)
    } else {
        0.0
    };
    text_indent + if line_index > 0 { hanging_indent } else { 0.0 }
}

fn used_text_indent(style: &ComputedStyle, available_width: f32) -> f32 {
    style
        .text_indent
        .amount
        .used_length_with_percentage_basis(available_width)
        .unwrap_or(
            style.text_indent.amount.length + style.text_indent.amount.percent * available_width,
        )
}

impl<'a> LayoutBuilder<'a> {
    /// Resolve the inline-level `vertical-align` shift for text fragments.
    ///
    /// CSS 2.2 defines most `vertical-align` values in terms of the parent
    /// inline box's baseline, content area, or x-height. This helper returns a
    /// shift where positive values raise the child inline box and negative
    /// values lower it:
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(super) fn vertical_align_baseline_shift_for_inline_style(
        &mut self,
        style: &ComputedStyle,
        parent_style: &ComputedStyle,
    ) -> f32 {
        let own_baseline = self.font_system.rendered_first_line_baseline_offset(style);
        self.vertical_align_baseline_shift_for_box(
            style,
            parent_style,
            style.line_height,
            own_baseline,
        )
    }

    /// Resolve the inline-level `vertical-align` shift for an atomic inline box.
    ///
    /// Atomic inline boxes expose synthesized baselines and margin-box extents,
    /// but CSS 2.2 alignment values still use the containing inline box as the
    /// reference:
    /// <https://www.w3.org/TR/css-inline-3/#atomic-inline> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(super) fn vertical_align_baseline_shift_for_atom(
        &mut self,
        atom: &InlineAtom,
        parent_style: &ComputedStyle,
    ) -> f32 {
        let own_block_size = inline_atom_logical_block_size(atom, parent_style);
        let own_baseline = match parent_style.writing_mode {
            WritingMode::HorizontalTb => atom.style.margin.top + atom.baseline_offset,
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                inline_atom_logical_block_start_margin(atom, parent_style)
                    + inline_atom_logical_border_block_size(atom, parent_style)
            }
        };
        self.vertical_align_baseline_shift_for_box(
            &atom.style,
            parent_style,
            own_block_size,
            own_baseline,
        )
    }

    fn vertical_align_baseline_shift_for_box(
        &mut self,
        style: &ComputedStyle,
        parent_style: &ComputedStyle,
        own_block_size: f32,
        own_baseline: f32,
    ) -> f32 {
        let alignment_shift = match resolved_alignment_baseline_metric(style, parent_style) {
            BaselineMetric::Alphabetic => 0.0,
            BaselineMetric::Middle => {
                let parent_x_height = self
                    .font_system
                    .x_height_for_style(parent_style)
                    .unwrap_or(parent_style.font_size * 0.5);
                own_block_size / 2.0 - own_baseline + parent_x_height / 2.0
            }
            BaselineMetric::TextTop | BaselineMetric::Hanging => {
                own_baseline
                    - self
                        .font_system
                        .rendered_first_line_baseline_offset(parent_style)
            }
            BaselineMetric::TextBottom | BaselineMetric::Ideographic => {
                let parent_baseline = self
                    .font_system
                    .rendered_first_line_baseline_offset(parent_style);
                own_block_size - own_baseline - (parent_style.font_size - parent_baseline)
            }
            BaselineMetric::Central | BaselineMetric::Mathematical => {
                own_block_size / 2.0 - own_baseline + parent_style.font_size / 2.0
            }
        };
        let baseline_shift = match style.vertical_align.baseline_shift {
            BaselineShift::LengthPercentage(_) => style
                .vertical_align
                .length_percentage_shift(style.line_height),
            BaselineShift::Super => self
                .font_system
                .script_vertical_align_shift(style, BaselineShift::Super)
                .unwrap_or(style.font_size * 0.45),
            BaselineShift::Sub => self
                .font_system
                .script_vertical_align_shift(style, BaselineShift::Sub)
                .unwrap_or(-style.font_size * 0.4),
            BaselineShift::Top | BaselineShift::Center | BaselineShift::Bottom => 0.0,
        };
        alignment_shift + baseline_shift
    }
}

fn resolved_alignment_baseline_metric(
    style: &ComputedStyle,
    parent_style: &ComputedStyle,
) -> BaselineMetric {
    match style.vertical_align.alignment_baseline {
        AlignmentBaseline::Metric(metric) => metric,
        AlignmentBaseline::Baseline => match parent_style.vertical_align.dominant_baseline {
            DominantBaseline::Metric(metric) => metric,
            DominantBaseline::Auto => BaselineMetric::Alphabetic,
        },
    }
}

pub(super) fn alpha_marker(mut index: usize, uppercase: bool) -> String {
    let mut marker = String::new();
    while index > 0 {
        index -= 1;
        let base = if uppercase { b'A' } else { b'a' };
        marker.insert(0, char::from(base + (index % 26) as u8));
        index /= 26;
    }
    marker
}

/// Stateful CSS `text-transform` word-boundary context for one inline formatting context.
///
/// CSS Text Level 3 defines `capitalize` word boundaries across inline box
/// boundaries, and requires out-of-flow boxes to be ignored while determining
/// those boundaries:
/// <https://www.w3.org/TR/css-text-3/#text-transform-property>.
#[derive(Debug, Clone)]
pub(super) struct TextTransformState {
    new_word: bool,
}

impl Default for TextTransformState {
    fn default() -> Self {
        Self { new_word: true }
    }
}

/// Applies CSS `text-transform` while updating word-boundary state.
///
/// CSS Text Level 3 allows UAs to choose word-boundary detection for
/// `capitalize`, but inline box boundaries and out-of-flow boxes must not
/// introduce boundaries. Callers that lay out a sequence of inline fragments
/// should share one state across in-flow text fragments:
/// <https://www.w3.org/TR/css-text-3/#text-transform-property>.
pub(super) fn transform_text_with_state(
    text: &str,
    style: &ComputedStyle,
    state: &mut TextTransformState,
) -> String {
    transform_text_inner(text, style, Some(state))
}

/// Applies CSS `text-transform` for independent text contexts.
///
/// CSS Text Level 3 defines the case-transform values for generated visual
/// text. This convenience wrapper starts a fresh word-boundary context, which
/// is appropriate for isolated block text and intrinsic-size estimates:
/// <https://www.w3.org/TR/css-text-3/#text-transform-property>.
pub(super) fn transform_text(text: &str, style: &ComputedStyle) -> String {
    transform_text_inner(text, style, None)
}

fn transform_text_inner(
    text: &str,
    style: &ComputedStyle,
    state: Option<&mut TextTransformState>,
) -> String {
    let mut fallback_state = TextTransformState::default();
    let state = state.unwrap_or(&mut fallback_state);
    let mut text = match style.text_transform.case {
        TextTransformCase::None => {
            map_text_transform_characters(text, state, |character, _| character.to_string())
        }
        TextTransformCase::Uppercase => uppercase_text(text, style.language.as_deref(), state),
        TextTransformCase::Lowercase => lowercase_text(text, style.language.as_deref(), state),
        TextTransformCase::Capitalize => capitalize_text(text, style.language.as_deref(), state),
    };
    if style.text_transform.full_width {
        text = full_width_text(&text);
    }
    if style.text_transform.full_size_kana {
        text = full_size_kana_text(&text);
    }
    text
}

/// Map text through ICU's full uppercase mapping.
///
/// CSS Text defines `text-transform: uppercase` in terms of the Unicode
/// Default Case Conversion algorithm, with language-sensitive tailorings from
/// the element language:
/// <https://www.w3.org/TR/css-text-3/#valdef-text-transform-uppercase> and
/// <https://www.unicode.org/versions/latest/ch03.pdf#G33992>.
fn uppercase_text(text: &str, language: Option<&str>, state: &mut TextTransformState) -> String {
    let language = case_mapping_language(language);
    let mapped = CaseMapper::new().uppercase_to_string(text, &language);
    update_text_transform_state_for_output(state, text);
    mapped.into_owned()
}

/// Map text through ICU's full lowercase mapping.
///
/// CSS Text defines `text-transform: lowercase` in terms of the Unicode
/// Default Case Conversion algorithm, with language-sensitive tailorings from
/// the element language:
/// <https://www.w3.org/TR/css-text-3/#valdef-text-transform-lowercase> and
/// <https://www.unicode.org/versions/latest/ch03.pdf#G33992>.
fn lowercase_text(text: &str, language: Option<&str>, state: &mut TextTransformState) -> String {
    let language = case_mapping_language(language);
    let mapped = CaseMapper::new().lowercase_to_string(text, &language);
    update_text_transform_state_for_output(state, text);
    mapped.into_owned()
}

fn map_text_transform_characters(
    text: &str,
    state: &mut TextTransformState,
    mut map: impl FnMut(char, bool) -> String,
) -> String {
    let mut output = String::new();
    for character in text.chars() {
        let new_word = state.new_word;
        output.push_str(&map(character, new_word));
        state.update(character);
    }
    output
}

fn capitalize_text(text: &str, language: Option<&str>, state: &mut TextTransformState) -> String {
    let mut output = String::new();
    let language = case_mapping_language(language);
    let segmenter = WordSegmenter::new_auto(WordBreakInvariantOptions::default());
    let mut start = 0usize;
    for (end, word_type) in segmenter.segment_str(text).iter_with_word_type() {
        if end == 0 {
            continue;
        }
        let segment = &text[start..end];
        if word_type.is_word_like() {
            push_capitalized_word_segment(&mut output, segment, &language, state);
            state.mark_after_word();
        } else {
            output.push_str(segment);
            state.update_non_word_segment(segment);
        }
        start = end;
    }
    if start < text.len() {
        let segment = &text[start..];
        output.push_str(segment);
        state.update_non_word_segment(segment);
    }
    output
}

fn push_capitalized_word_segment(
    output: &mut String,
    segment: &str,
    language: &LanguageIdentifier,
    state: &mut TextTransformState,
) {
    for (offset, character) in segment.char_indices() {
        if character_is_unicode_alphanumeric(character) {
            if state.new_word {
                if character_is_unicode_letter(character) {
                    output.push_str(&titlecase_word_tail(&segment[offset..], language));
                    update_text_transform_state_for_output(state, &segment[offset..]);
                    break;
                } else {
                    state.update(character);
                    output.push(character);
                }
            } else {
                output.push(character);
                state.update(character);
            }
        } else {
            output.push(character);
        }
    }
}

/// Titlecase one CSS word tail while preserving non-initial casing.
///
/// CSS `capitalize` titlecases the first typographic letter unit of each word
/// and leaves the remaining characters unchanged. ICU's titlecase mapping is
/// used to identify the full language-tailored leading titlecase unit, and the
/// untouched source tail is spliced back afterward so CSS trailing case is
/// preserved:
/// <https://www.w3.org/TR/css-text-3/#valdef-text-transform-capitalize> and
/// <https://www.unicode.org/reports/tr21/tr21-5.html#Caseless_Matching>.
fn titlecase_word_tail(segment: &str, language: &LanguageIdentifier) -> String {
    let options = TitlecaseOptions::default();
    let title_lower = TitlecaseMapper::new()
        .titlecase_segment_to_string(segment, language, options)
        .into_owned();
    let lower_source = CaseMapper::new()
        .lowercase_to_string(segment, language)
        .into_owned();
    if title_lower == lower_source {
        return segment.to_string();
    }
    for source_boundary in segment
        .char_indices()
        .map(|(offset, character)| offset + character.len_utf8())
    {
        let lower_tail = CaseMapper::new()
            .lowercase_to_string(&segment[source_boundary..], language)
            .into_owned();
        for title_boundary in title_lower
            .char_indices()
            .map(|(offset, character)| offset + character.len_utf8())
        {
            if title_lower[title_boundary..] == lower_tail {
                let mut output = String::with_capacity(title_lower.len() + segment.len());
                output.push_str(&title_lower[..title_boundary]);
                output.push_str(&segment[source_boundary..]);
                return output;
            }
        }
    }
    title_lower
}

fn case_mapping_language(language: Option<&str>) -> LanguageIdentifier {
    language
        .and_then(|language| language.replace('_', "-").parse().ok())
        .unwrap_or_else(root_language_identifier)
}

fn root_language_identifier() -> LanguageIdentifier {
    "und"
        .parse()
        .expect("the Unicode root language identifier is valid")
}

fn update_text_transform_state_for_output(state: &mut TextTransformState, text: &str) {
    for character in text.chars() {
        state.update(character);
    }
}

/// Map text for `text-transform: full-width`.
///
/// CSS Text defines `full-width` as converting characters to their fullwidth
/// forms, notably ASCII and halfwidth Katakana compatibility characters:
/// <https://www.w3.org/TR/css-text-3/#valdef-text-transform-full-width>.
fn full_width_text(text: &str) -> String {
    let mut output = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(next) = characters.peek().copied() {
            if next == '\u{ff9e}'
                && let Some(composed) = full_width_voiced_kana(character)
            {
                output.push_str(composed);
                characters.next();
                continue;
            }
            if next == '\u{ff9f}'
                && let Some(composed) = full_width_semi_voiced_kana(character)
            {
                output.push_str(composed);
                characters.next();
                continue;
            }
        }
        let mapped = full_width_character(character);
        if mapped.is_empty() {
            output.push(character);
        } else {
            output.push_str(mapped);
        }
    }
    output
}

fn full_width_character(character: char) -> &'static str {
    match character {
        ' ' => "\u{3000}",
        '!' => "！",
        '"' => "＂",
        '#' => "＃",
        '$' => "＄",
        '%' => "％",
        '&' => "＆",
        '\'' => "＇",
        '(' => "（",
        ')' => "）",
        '*' => "＊",
        '+' => "＋",
        ',' => "，",
        '-' => "－",
        '.' => "．",
        '/' => "／",
        '0' => "０",
        '1' => "１",
        '2' => "２",
        '3' => "３",
        '4' => "４",
        '5' => "５",
        '6' => "６",
        '7' => "７",
        '8' => "８",
        '9' => "９",
        ':' => "：",
        ';' => "；",
        '<' => "＜",
        '=' => "＝",
        '>' => "＞",
        '?' => "？",
        '@' => "＠",
        'A' => "Ａ",
        'B' => "Ｂ",
        'C' => "Ｃ",
        'D' => "Ｄ",
        'E' => "Ｅ",
        'F' => "Ｆ",
        'G' => "Ｇ",
        'H' => "Ｈ",
        'I' => "Ｉ",
        'J' => "Ｊ",
        'K' => "Ｋ",
        'L' => "Ｌ",
        'M' => "Ｍ",
        'N' => "Ｎ",
        'O' => "Ｏ",
        'P' => "Ｐ",
        'Q' => "Ｑ",
        'R' => "Ｒ",
        'S' => "Ｓ",
        'T' => "Ｔ",
        'U' => "Ｕ",
        'V' => "Ｖ",
        'W' => "Ｗ",
        'X' => "Ｘ",
        'Y' => "Ｙ",
        'Z' => "Ｚ",
        '[' => "［",
        '\\' => "＼",
        ']' => "］",
        '^' => "＾",
        '_' => "＿",
        '`' => "｀",
        'a' => "ａ",
        'b' => "ｂ",
        'c' => "ｃ",
        'd' => "ｄ",
        'e' => "ｅ",
        'f' => "ｆ",
        'g' => "ｇ",
        'h' => "ｈ",
        'i' => "ｉ",
        'j' => "ｊ",
        'k' => "ｋ",
        'l' => "ｌ",
        'm' => "ｍ",
        'n' => "ｎ",
        'o' => "ｏ",
        'p' => "ｐ",
        'q' => "ｑ",
        'r' => "ｒ",
        's' => "ｓ",
        't' => "ｔ",
        'u' => "ｕ",
        'v' => "ｖ",
        'w' => "ｗ",
        'x' => "ｘ",
        'y' => "ｙ",
        'z' => "ｚ",
        '{' => "｛",
        '|' => "｜",
        '}' => "｝",
        '~' => "～",
        '\u{ff61}' => "\u{3002}",
        '\u{ff62}' => "\u{300c}",
        '\u{ff63}' => "\u{300d}",
        '\u{ff64}' => "\u{3001}",
        '\u{ff65}' => "\u{30fb}",
        '\u{ff66}' => "\u{30f2}",
        '\u{ff67}' => "\u{30a1}",
        '\u{ff68}' => "\u{30a3}",
        '\u{ff69}' => "\u{30a5}",
        '\u{ff6a}' => "\u{30a7}",
        '\u{ff6b}' => "\u{30a9}",
        '\u{ff6c}' => "\u{30e3}",
        '\u{ff6d}' => "\u{30e5}",
        '\u{ff6e}' => "\u{30e7}",
        '\u{ff6f}' => "\u{30c3}",
        '\u{ff70}' => "\u{30fc}",
        '\u{ff71}' => "\u{30a2}",
        '\u{ff72}' => "\u{30a4}",
        '\u{ff73}' => "\u{30a6}",
        '\u{ff74}' => "\u{30a8}",
        '\u{ff75}' => "\u{30aa}",
        '\u{ff76}' => "\u{30ab}",
        '\u{ff77}' => "\u{30ad}",
        '\u{ff78}' => "\u{30af}",
        '\u{ff79}' => "\u{30b1}",
        '\u{ff7a}' => "\u{30b3}",
        '\u{ff7b}' => "\u{30b5}",
        '\u{ff7c}' => "\u{30b7}",
        '\u{ff7d}' => "\u{30b9}",
        '\u{ff7e}' => "\u{30bb}",
        '\u{ff7f}' => "\u{30bd}",
        '\u{ff80}' => "\u{30bf}",
        '\u{ff81}' => "\u{30c1}",
        '\u{ff82}' => "\u{30c4}",
        '\u{ff83}' => "\u{30c6}",
        '\u{ff84}' => "\u{30c8}",
        '\u{ff85}' => "\u{30ca}",
        '\u{ff86}' => "\u{30cb}",
        '\u{ff87}' => "\u{30cc}",
        '\u{ff88}' => "\u{30cd}",
        '\u{ff89}' => "\u{30ce}",
        '\u{ff8a}' => "\u{30cf}",
        '\u{ff8b}' => "\u{30d2}",
        '\u{ff8c}' => "\u{30d5}",
        '\u{ff8d}' => "\u{30d8}",
        '\u{ff8e}' => "\u{30db}",
        '\u{ff8f}' => "\u{30de}",
        '\u{ff90}' => "\u{30df}",
        '\u{ff91}' => "\u{30e0}",
        '\u{ff92}' => "\u{30e1}",
        '\u{ff93}' => "\u{30e2}",
        '\u{ff94}' => "\u{30e4}",
        '\u{ff95}' => "\u{30e6}",
        '\u{ff96}' => "\u{30e8}",
        '\u{ff97}' => "\u{30e9}",
        '\u{ff98}' => "\u{30ea}",
        '\u{ff99}' => "\u{30eb}",
        '\u{ff9a}' => "\u{30ec}",
        '\u{ff9b}' => "\u{30ed}",
        '\u{ff9c}' => "\u{30ef}",
        '\u{ff9d}' => "\u{30f3}",
        '\u{ff9e}' => "\u{3099}",
        '\u{ff9f}' => "\u{309a}",
        _ => "",
    }
}

fn full_width_voiced_kana(character: char) -> Option<&'static str> {
    match character {
        '\u{ff73}' => Some("\u{30f4}"),
        '\u{ff76}' => Some("\u{30ac}"),
        '\u{ff77}' => Some("\u{30ae}"),
        '\u{ff78}' => Some("\u{30b0}"),
        '\u{ff79}' => Some("\u{30b2}"),
        '\u{ff7a}' => Some("\u{30b4}"),
        '\u{ff7b}' => Some("\u{30b6}"),
        '\u{ff7c}' => Some("\u{30b8}"),
        '\u{ff7d}' => Some("\u{30ba}"),
        '\u{ff7e}' => Some("\u{30bc}"),
        '\u{ff7f}' => Some("\u{30be}"),
        '\u{ff80}' => Some("\u{30c0}"),
        '\u{ff81}' => Some("\u{30c2}"),
        '\u{ff82}' => Some("\u{30c5}"),
        '\u{ff83}' => Some("\u{30c7}"),
        '\u{ff84}' => Some("\u{30c9}"),
        '\u{ff8a}' => Some("\u{30d0}"),
        '\u{ff8b}' => Some("\u{30d3}"),
        '\u{ff8c}' => Some("\u{30d6}"),
        '\u{ff8d}' => Some("\u{30d9}"),
        '\u{ff8e}' => Some("\u{30dc}"),
        _ => None,
    }
}

fn full_width_semi_voiced_kana(character: char) -> Option<&'static str> {
    match character {
        '\u{ff8a}' => Some("\u{30d1}"),
        '\u{ff8b}' => Some("\u{30d4}"),
        '\u{ff8c}' => Some("\u{30d7}"),
        '\u{ff8d}' => Some("\u{30da}"),
        '\u{ff8e}' => Some("\u{30dd}"),
        _ => None,
    }
}

/// Map text for `text-transform: full-size-kana`.
///
/// CSS Text defines `full-size-kana` as converting small Kana to their
/// ordinary-sized equivalents for ruby and emphasis readability:
/// <https://www.w3.org/TR/css-text-3/#valdef-text-transform-full-size-kana>.
fn full_size_kana_text(text: &str) -> String {
    let mut output = String::new();
    for character in text.chars() {
        let mapped = full_size_kana_character(character);
        if mapped.is_empty() {
            output.push(character);
        } else {
            output.push_str(mapped);
        }
    }
    output
}

fn full_size_kana_character(character: char) -> &'static str {
    match character {
        'ぁ' => "あ",
        'ぃ' => "い",
        'ぅ' => "う",
        'ぇ' => "え",
        'ぉ' => "お",
        'ゕ' => "か",
        'ゖ' => "け",
        'っ' => "つ",
        'ゃ' => "や",
        'ゅ' => "ゆ",
        'ょ' => "よ",
        'ゎ' => "わ",
        'ァ' => "ア",
        'ィ' => "イ",
        'ゥ' => "ウ",
        'ェ' => "エ",
        'ォ' => "オ",
        'ヵ' => "カ",
        'ヶ' => "ケ",
        'ッ' => "ツ",
        'ャ' => "ヤ",
        'ュ' => "ユ",
        'ョ' => "ヨ",
        'ヮ' => "ワ",
        'ㇰ' => "ク",
        'ㇱ' => "シ",
        'ㇲ' => "ス",
        'ㇳ' => "ト",
        'ㇴ' => "ヌ",
        'ㇵ' => "ハ",
        'ㇶ' => "ヒ",
        'ㇷ' => "フ",
        'ㇸ' => "ヘ",
        'ㇹ' => "ホ",
        'ㇺ' => "ム",
        'ㇻ' => "ラ",
        'ㇼ' => "リ",
        'ㇽ' => "ル",
        'ㇾ' => "レ",
        'ㇿ' => "ロ",
        _ => "",
    }
}

impl TextTransformState {
    pub(super) fn force_word_boundary(&mut self) {
        self.new_word = true;
    }

    fn update(&mut self, character: char) {
        self.new_word = !character_is_unicode_alphanumeric(character);
    }

    fn mark_after_word(&mut self) {
        self.new_word = false;
    }

    fn update_non_word_segment(&mut self, segment: &str) {
        if self.new_word {
            return;
        }
        if segment
            .chars()
            .all(character_preserves_word_boundary_context)
        {
            return;
        }
        self.new_word = true;
    }
}

pub(super) fn constrain(mut value: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    if let Some(min) = min {
        value = value.max(min);
    }
    if let Some(max) = max {
        value = value.min(max);
    }
    value
}

pub(super) fn inline_text(element: &Element) -> String {
    let mut output = String::new();
    for child in &element.children {
        collect_inline_text(child, &mut output);
    }
    normalize_inline_text(&output)
}

pub(super) fn normalized_text_for_style(text: &str, style: &ComputedStyle) -> String {
    let text = match style.white_space {
        WhiteSpace::Normal | WhiteSpace::NoWrap => normalize_inline_text(text),
        WhiteSpace::PreLine => normalize_pre_line_text_for_style(text, style),
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces => {
            normalize_pre_wrap_text_for_style(text, style)
        }
    };
    text_with_visible_control_characters(&text)
}

pub(super) fn inline_text_for_style(element: &Element, style: &ComputedStyle) -> String {
    let text = match style.white_space {
        WhiteSpace::Normal | WhiteSpace::NoWrap => inline_text(element),
        WhiteSpace::PreLine => pre_line_inline_text_for_style(element, style),
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces => {
            pre_wrap_inline_text_for_style(element, style)
        }
    };
    text_with_visible_control_characters(&text)
}

pub(super) fn own_inline_text(element: &Element) -> String {
    let mut output = String::new();
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                output.push_str(text);
                output.push(' ');
            }
            NodeKind::Element(child) if is_line_break_element(child) => output.push(INLINE_BREAK),
            _ => {}
        }
    }
    normalize_inline_text(&output)
}

pub(super) fn own_inline_text_for_style(element: &Element, style: &ComputedStyle) -> String {
    let text = match style.white_space {
        WhiteSpace::Normal | WhiteSpace::NoWrap => own_inline_text(element),
        WhiteSpace::PreLine => {
            let mut output = String::new();
            for child in &element.children {
                match &child.kind {
                    NodeKind::Text(text) => output.push_str(text),
                    NodeKind::Element(child) if is_line_break_element(child) => output.push('\n'),
                    _ => {}
                }
            }
            normalize_pre_line_text_for_style(&output, style)
        }
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces => {
            let mut output = String::new();
            for child in &element.children {
                match &child.kind {
                    NodeKind::Text(text) => output.push_str(text),
                    NodeKind::Element(child) if is_line_break_element(child) => output.push('\n'),
                    _ => {}
                }
            }
            normalize_pre_wrap_text_for_style(&output, style)
        }
    };
    text_with_visible_control_characters(&text)
}

/// Replaces non-whitespace Unicode control characters with a visible glyph.
///
/// CSS Text white-space processing keeps control characters other than
/// document white space visible instead of silently discarding them. Use U+FFFD
/// so PDF output has a font-fallback-visible glyph even when no font maps the
/// original C0/C1 control code:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(super) fn text_with_visible_control_characters(text: &str) -> String {
    text.chars()
        .map(|character| {
            if is_visible_control_character(character) {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

fn is_visible_control_character(character: char) -> bool {
    character_is_unicode_control(character)
        && !is_css_collapsible_whitespace(character)
        && character != INLINE_BREAK
}

pub(super) fn pre_wrap_inline_text_for_style(element: &Element, style: &ComputedStyle) -> String {
    let mut output = String::new();
    for child in &element.children {
        collect_pre_wrap_inline_text(child, &mut output);
    }
    normalize_pre_wrap_text_for_style(&output, style)
}

pub(super) fn pre_line_inline_text_for_style(element: &Element, style: &ComputedStyle) -> String {
    let mut output = String::new();
    for child in &element.children {
        collect_pre_wrap_inline_text(child, &mut output);
    }
    normalize_pre_line_text_for_style(&output, style)
}

pub(super) fn collect_inline_text(node: &Node, output: &mut String) {
    match &node.kind {
        NodeKind::Text(text) => {
            output.push_str(text);
            output.push(' ');
        }
        NodeKind::Element(element) if is_line_break_element(element) => output.push(INLINE_BREAK),
        NodeKind::Element(element) if is_default_block_container_tag(&element.tag) => {}
        NodeKind::Element(element) => {
            for child in &element.children {
                collect_inline_text(child, output);
            }
        }
    }
}

pub(super) fn collect_pre_wrap_inline_text(node: &Node, output: &mut String) {
    match &node.kind {
        NodeKind::Text(text) => output.push_str(text),
        NodeKind::Element(element) if is_line_break_element(element) => output.push('\n'),
        NodeKind::Element(element) if is_default_block_container_tag(&element.tag) => {}
        NodeKind::Element(element) => {
            for child in &element.children {
                collect_pre_wrap_inline_text(child, output);
            }
        }
    }
}

pub(super) const INLINE_BREAK: char = '\u{000B}';

pub(super) fn normalize_inline_text(text: &str) -> String {
    let mut output = String::new();
    let mut last_was_space = true;
    for character in dom::decode_entities_public(text).chars() {
        if character == INLINE_BREAK {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push('\n');
            last_was_space = true;
        } else if is_css_collapsible_whitespace(character) {
            if !last_was_space {
                output.push(' ');
                last_was_space = true;
            }
        } else {
            output.push(character);
            last_was_space = false;
        }
    }
    crate::text::trim_css_collapsible_whitespace(&output).to_string()
}

pub(super) fn normalize_pre_wrap_text_for_style(text: &str, _style: &ComputedStyle) -> String {
    dom::decode_entities_public(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

pub(super) fn normalize_pre_line_text_for_style(text: &str, style: &ComputedStyle) -> String {
    let mut output = String::new();
    let mut last_was_space = true;
    for character in normalize_pre_wrap_text_for_style(text, style).chars() {
        if character == '\n' || character == INLINE_BREAK {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push('\n');
            last_was_space = true;
        } else if is_css_collapsible_whitespace(character) {
            if !last_was_space {
                output.push(' ');
                last_was_space = true;
            }
        } else {
            output.push(character);
            last_was_space = false;
        }
    }
    crate::text::trim_css_collapsible_whitespace(&output).to_string()
}
