use super::*;

/// Stateful CSS `text-transform` word-boundary context for one inline formatting context.
///
/// CSS Text Level 3 defines `capitalize` word boundaries across inline box
/// boundaries, and requires out-of-flow boxes to be ignored while determining
/// those boundaries:
/// <https://www.w3.org/TR/css-text-3/#text-transform-property>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct TextTransformState {
    pub(in crate::layout) new_word: bool,
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
pub(in crate::layout) fn transform_text_with_state(
    text: &str,
    style: &ComputedStyle,
    state: &mut TextTransformState,
) -> String {
    transform_text_inner(text, style, Some(state))
}

/// Apply CSS's permitted small-caps synthesis when the selected face has no
/// matching OpenType caps feature. Compatibility ligatures and U+00DF are
/// expanded even when the face has the feature: those source glyphs commonly
/// lack a caps alternate and must receive tracking between their expansion.
/// <https://drafts.csswg.org/css-fonts-4/#font-variant-caps-prop>
pub(in crate::layout) fn synthesize_missing_font_caps_text(
    font_system: &mut FontSystem,
    text: &str,
    style: &ComputedStyle,
) -> String {
    if !style.font_synthesis.small_caps
        || !matches!(
            style.font_variant_caps,
            crate::css::FontVariantCaps::SmallCaps
                | crate::css::FontVariantCaps::AllSmallCaps
                | crate::css::FontVariantCaps::PetiteCaps
                | crate::css::FontVariantCaps::AllPetiteCaps
        )
    {
        return text.to_owned();
    }
    let synthesize_all = !font_system.selected_font_supports_caps_feature(style);
    let has_unsupported_compatibility_form = text
        .chars()
        .any(|character| character == '\u{00df}' || matches!(character, '\u{fb00}'..='\u{fb06}'));
    if !synthesize_all && !has_unsupported_compatibility_form {
        return text.to_owned();
    }
    let language = case_mapping_language(style.language.as_deref());
    let mapper = CaseMapper::new();
    text.chars()
        .map(|character| {
            if synthesize_all
                || character == '\u{00df}'
                || matches!(character, '\u{fb00}'..='\u{fb06}')
            {
                mapper
                    .uppercase_to_string(&character.to_string(), &language)
                    .into_owned()
            } else {
                character.to_string()
            }
        })
        .collect()
}

/// Applies CSS `text-transform` for independent text contexts.
///
/// CSS Text Level 3 defines the case-transform values for generated visual
/// text. This convenience wrapper starts a fresh word-boundary context, which
/// is appropriate for isolated block text and intrinsic-size estimates:
/// <https://www.w3.org/TR/css-text-3/#text-transform-property>.
pub(in crate::layout) fn transform_text(text: &str, style: &ComputedStyle) -> String {
    transform_text_inner(text, style, None)
}

pub(in crate::layout) fn transform_text_inner(
    text: &str,
    style: &ComputedStyle,
    state: Option<&mut TextTransformState>,
) -> String {
    let mut fallback_state = TextTransformState::default();
    let state = state.unwrap_or(&mut fallback_state);
    let mut text = match style.text_transform.case() {
        None => map_text_transform_characters(text, state, |character, _| character.to_string()),
        Some(TextTransformCase::Uppercase) => {
            uppercase_text(text, style.language.as_deref(), state)
        }
        Some(TextTransformCase::Lowercase) => {
            lowercase_text(text, style.language.as_deref(), state)
        }
        Some(TextTransformCase::Capitalize) => {
            capitalize_text(text, style.language.as_deref(), state)
        }
    };
    if style.text_transform.applies_full_width() {
        text = full_width_text(&text);
    }
    if style.text_transform.applies_full_size_kana() {
        text = full_size_kana_text(&text);
    }
    if style.text_transform.applies_math_auto() {
        text = math_auto_text(&text);
    }
    text
}

/// Apply MathML Core's `math-auto` mathematical-italic mappings.
///
/// These are Unicode character substitutions, not an OpenType feature: the
/// transformed scalar is shaped, measured, painted, and exported through the
/// ordinary text pipeline. The exceptional Latin `h` and the Greek symbol
/// variants occupy compatibility code points, while the primary Latin and
/// Greek alphabets use contiguous Mathematical Alphanumeric Symbol ranges:
/// <https://w3c.github.io/mathml-core/#math-auto-transform> and
/// <https://w3c.github.io/mathml-core/#italic-mappings>.
pub(in crate::layout) fn math_auto_text(text: &str) -> String {
    let mut characters = text.chars();
    let Some(character) = characters.next() else {
        return String::new();
    };
    if characters.next().is_some() {
        return text.to_owned();
    }
    math_auto_character(character).to_string()
}

fn math_auto_character(character: char) -> char {
    let scalar = character as u32;
    let mapped = match character {
        'A'..='Z' => Some(0x1D434 + scalar - 'A' as u32),
        'a'..='g' | 'i'..='z' => Some(0x1D44E + scalar - 'a' as u32),
        'h' => Some(0x210E),
        '\u{0131}' => Some(0x1D6A4),
        '\u{0237}' => Some(0x1D6A5),
        '\u{0391}'..='\u{03A1}' => Some(0x1D6E2 + scalar - 0x0391),
        '\u{03F4}' => Some(0x1D6F3),
        '\u{03A3}'..='\u{03A9}' => Some(0x1D6F4 + scalar - 0x03A3),
        '\u{2207}' => Some(0x1D6FB),
        '\u{03B1}'..='\u{03C9}' => Some(0x1D6FC + scalar - 0x03B1),
        '\u{2202}' => Some(0x1D715),
        '\u{03F5}' => Some(0x1D716),
        '\u{03D1}' => Some(0x1D717),
        '\u{03F0}' => Some(0x1D718),
        '\u{03D5}' => Some(0x1D719),
        '\u{03F1}' => Some(0x1D71A),
        '\u{03D6}' => Some(0x1D71B),
        _ => None,
    };
    mapped.and_then(char::from_u32).unwrap_or(character)
}

/// Map text through ICU's full uppercase mapping.
///
/// CSS Text defines `text-transform: uppercase` in terms of the Unicode
/// Default Case Conversion algorithm, with language-sensitive tailorings from
/// the element language:
/// <https://www.w3.org/TR/css-text-3/#valdef-text-transform-uppercase> and
/// <https://www.unicode.org/versions/latest/ch03.pdf#G33992>.
pub(in crate::layout) fn uppercase_text(
    text: &str,
    language: Option<&str>,
    state: &mut TextTransformState,
) -> String {
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
pub(in crate::layout) fn lowercase_text(
    text: &str,
    language: Option<&str>,
    state: &mut TextTransformState,
) -> String {
    let language = case_mapping_language(language);
    let mapped = CaseMapper::new().lowercase_to_string(text, &language);
    update_text_transform_state_for_output(state, text);
    mapped.into_owned()
}

pub(in crate::layout) fn map_text_transform_characters(
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

pub(in crate::layout) fn capitalize_text(
    text: &str,
    language: Option<&str>,
    state: &mut TextTransformState,
) -> String {
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

pub(in crate::layout) fn push_capitalized_word_segment(
    output: &mut String,
    segment: &str,
    language: &LanguageIdentifier,
    state: &mut TextTransformState,
) {
    for (offset, character) in segment.char_indices() {
        if character_is_unicode_alphanumeric(character) {
            if state.new_word {
                if character_is_unicode_typographic_letter(character) {
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
pub(in crate::layout) fn titlecase_word_tail(
    segment: &str,
    language: &LanguageIdentifier,
) -> String {
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

pub(in crate::layout) fn case_mapping_language(language: Option<&str>) -> LanguageIdentifier {
    let Some(language) = language else {
        return root_language_identifier();
    };
    let language = language.replace('_', "-");
    let identifier = language
        .parse::<LanguageIdentifier>()
        .unwrap_or_else(|_| root_language_identifier());
    if language_uses_turkic_case_mapping(&language) && language_declares_non_latin_script(&language)
    {
        // CSS Text's writing-system rules give an explicit script subtag
        // precedence over a contradictory language default. Turkish and Azeri
        // dotted-I casing applies to their Latin writing systems, not to text
        // explicitly tagged as Cyrillic, Arabic, and so on.
        // <https://www.w3.org/TR/css-text-3/#script-tagging>
        return root_language_identifier();
    }
    identifier
}

fn language_uses_turkic_case_mapping(language: &str) -> bool {
    matches!(
        language.split('-').next(),
        Some(primary) if primary.eq_ignore_ascii_case("tr") || primary.eq_ignore_ascii_case("az")
    )
}

fn language_declares_non_latin_script(language: &str) -> bool {
    language.split('-').skip(1).any(|subtag| {
        subtag.len() == 4
            && subtag
                .chars()
                .all(|character| character.is_ascii_alphabetic())
            && !subtag.eq_ignore_ascii_case("latn")
    })
}

pub(in crate::layout) fn root_language_identifier() -> LanguageIdentifier {
    "und"
        .parse()
        .expect("the Unicode root language identifier is valid")
}

pub(in crate::layout) fn update_text_transform_state_for_output(
    state: &mut TextTransformState,
    text: &str,
) {
    for character in text.chars() {
        state.update(character);
    }
}

/// Map text for `text-transform: full-width`.
///
/// CSS Text defines `full-width` as converting characters to their fullwidth
/// forms, notably ASCII and halfwidth Katakana compatibility characters:
/// <https://www.w3.org/TR/css-text-3/#valdef-text-transform-full-width>.
pub(in crate::layout) fn full_width_text(text: &str) -> String {
    let mut output = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(next) = characters.peek().cloned() {
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
        if let Some(mapped) = full_width_compatibility_character(character) {
            output.push(mapped);
            continue;
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

/// Return the single-scalar compatibility mappings used by CSS `full-width`
/// that do not belong to ASCII or halfwidth Katakana.
///
/// The halfwidth Hangul ranges have sparse source code-point ranges but map to
/// consecutive Hangul Compatibility Jamo ranges. The remaining mappings are
/// the Unicode `<wide>` compatibility mappings selected by CSS Text.
/// <https://www.unicode.org/Public/UCD/latest/ucd/UnicodeData.txt> and
/// <https://www.w3.org/TR/css-text-3/#valdef-text-transform-full-width>
pub(in crate::layout) fn full_width_compatibility_character(character: char) -> Option<char> {
    let scalar = character as u32;
    let mapped = match scalar {
        0x2985 => 0xff5f,
        0x2986 => 0xff60,
        0xffa0 => 0x3164,
        0xffa1..=0xffbe => 0x3131 + scalar - 0xffa1,
        0xffc2..=0xffc6 => 0x314f + scalar - 0xffc2,
        0xffc7 => 0x3154,
        0xffca..=0xffcf => 0x3155 + scalar - 0xffca,
        0xffd2..=0xffd7 => 0x315b + scalar - 0xffd2,
        0xffda..=0xffdc => 0x3161 + scalar - 0xffda,
        0x00a2 => 0xffe0,
        0x00a3 => 0xffe1,
        0x00ac => 0xffe2,
        0x00af => 0xffe3,
        0x00a6 => 0xffe4,
        0x00a5 => 0xffe5,
        0x20a9 => 0xffe6,
        0xffe8 => 0x2502,
        0xffe9 => 0x2190,
        0xffea => 0x2191,
        0xffeb => 0x2192,
        0xffec => 0x2193,
        0xffed => 0x25a0,
        0xffee => 0x25cb,
        _ => return None,
    };
    char::from_u32(mapped)
}

pub(in crate::layout) fn full_width_character(character: char) -> &'static str {
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

pub(in crate::layout) fn full_width_voiced_kana(character: char) -> Option<&'static str> {
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

pub(in crate::layout) fn full_width_semi_voiced_kana(character: char) -> Option<&'static str> {
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
pub(in crate::layout) fn full_size_kana_text(text: &str) -> String {
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

pub(in crate::layout) fn full_size_kana_character(character: char) -> &'static str {
    match character {
        // CSS Text Appendix G's normative small Kana mapping table, synchronized
        // through Unicode 15.0. This is not Unicode normalization or a
        // fullwidth conversion: the halfwidth mappings deliberately retain
        // halfwidth output.
        // <https://drafts.csswg.org/css-text-3/#small-kana-mappings>
        '\u{1b132}' => "こ",
        '\u{1b150}' => "ゐ",
        '\u{1b151}' => "ゑ",
        '\u{1b152}' => "を",
        '\u{1b155}' => "コ",
        '\u{1b164}' => "ヰ",
        '\u{1b165}' => "ヱ",
        '\u{1b166}' => "ヲ",
        '\u{1b167}' => "ン",
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
        'ｧ' => "ｱ",
        'ｨ' => "ｲ",
        'ｩ' => "ｳ",
        'ｪ' => "ｴ",
        'ｫ' => "ｵ",
        'ｯ' => "ﾂ",
        'ｬ' => "ﾔ",
        'ｭ' => "ﾕ",
        'ｮ' => "ﾖ",
        _ => "",
    }
}

impl TextTransformState {
    pub(in crate::layout) fn force_word_boundary(&mut self) {
        self.new_word = true;
    }

    pub(in crate::layout) fn update(&mut self, character: char) {
        self.new_word = !character_is_unicode_alphanumeric(character);
    }

    pub(in crate::layout) fn mark_after_word(&mut self) {
        self.new_word = false;
    }

    pub(in crate::layout) fn update_non_word_segment(&mut self, segment: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_auto_uses_mathml_italic_unicode_mappings_for_single_character_text() {
        assert_eq!(math_auto_text("∂"), "\u{1d715}");
        assert_eq!(math_auto_text("∂∇"), "∂∇");
    }

    #[test]
    fn explicit_non_latin_script_overrides_turkish_case_tailoring() {
        let mut state = TextTransformState::default();
        assert_eq!(lowercase_text("I", Some("tr-Cyrl"), &mut state), "i");

        let mut latin_state = TextTransformState::default();
        assert_eq!(lowercase_text("I", Some("tr-Latn"), &mut latin_state), "ı");
    }

    #[test]
    fn full_size_kana_maps_every_css_text_appendix_g_pair() {
        let mappings = [
            ('ぁ', "あ"),
            ('ぃ', "い"),
            ('ぅ', "う"),
            ('ぇ', "え"),
            ('ぉ', "お"),
            ('ゕ', "か"),
            ('ゖ', "け"),
            ('\u{1b132}', "こ"),
            ('っ', "つ"),
            ('ゃ', "や"),
            ('ゅ', "ゆ"),
            ('ょ', "よ"),
            ('ゎ', "わ"),
            ('\u{1b150}', "ゐ"),
            ('\u{1b151}', "ゑ"),
            ('\u{1b152}', "を"),
            ('ァ', "ア"),
            ('ィ', "イ"),
            ('ゥ', "ウ"),
            ('ェ', "エ"),
            ('ォ', "オ"),
            ('ヵ', "カ"),
            ('ㇰ', "ク"),
            ('ヶ', "ケ"),
            ('\u{1b155}', "コ"),
            ('ㇱ', "シ"),
            ('ㇲ', "ス"),
            ('ッ', "ツ"),
            ('ㇳ', "ト"),
            ('ㇴ', "ヌ"),
            ('ㇵ', "ハ"),
            ('ㇶ', "ヒ"),
            ('ㇷ', "フ"),
            ('ㇸ', "ヘ"),
            ('ㇹ', "ホ"),
            ('ㇺ', "ム"),
            ('ャ', "ヤ"),
            ('ュ', "ユ"),
            ('ョ', "ヨ"),
            ('ㇻ', "ラ"),
            ('ㇼ', "リ"),
            ('ㇽ', "ル"),
            ('ㇾ', "レ"),
            ('ㇿ', "ロ"),
            ('ヮ', "ワ"),
            ('\u{1b164}', "ヰ"),
            ('\u{1b165}', "ヱ"),
            ('\u{1b166}', "ヲ"),
            ('\u{1b167}', "ン"),
            ('ｧ', "ｱ"),
            ('ｨ', "ｲ"),
            ('ｩ', "ｳ"),
            ('ｪ', "ｴ"),
            ('ｫ', "ｵ"),
            ('ｯ', "ﾂ"),
            ('ｬ', "ﾔ"),
            ('ｭ', "ﾕ"),
            ('ｮ', "ﾖ"),
        ];

        for (small, full_size) in mappings {
            assert_eq!(
                full_size_kana_character(small),
                full_size,
                "U+{:04X}",
                small as u32
            );
            assert_eq!(
                full_size_kana_text(&small.to_string()),
                full_size,
                "U+{:04X}",
                small as u32
            );
        }
    }

    #[test]
    fn full_width_maps_hangul_and_symbol_compatibility_characters() {
        assert_eq!(
            full_width_text(
                "\u{2985}\u{2986}\u{ffa0}\u{ffa1}\u{ffbe}\u{ffc2}\u{ffdc}\u{a2}\u{20a9}\u{ffee}"
            ),
            "｟｠ㅤㄱㅎㅏㅣ￠￦○"
        );
    }

    #[test]
    fn capitalize_titlecases_letter_number_characters() {
        let mut state = TextTransformState::default();
        assert_eq!(capitalize_text("ⅰⅰⅰ", Some("en"), &mut state), "Ⅰⅰⅰ");
    }
}
