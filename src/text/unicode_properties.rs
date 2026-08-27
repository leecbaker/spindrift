use icu_locale_core::LanguageIdentifier;

use super::*;

static JOINING_TYPES: OnceLock<CodePointMapDataBorrowed<'static, JoiningType>> = OnceLock::new();
static JOIN_CONTROLS: OnceLock<CodePointSetDataBorrowed<'static>> = OnceLock::new();
static BIDI_CONTROLS: OnceLock<CodePointSetDataBorrowed<'static>> = OnceLock::new();
static BIDI_CLASSES: OnceLock<CodePointMapDataBorrowed<'static, BidiClass>> = OnceLock::new();
static BIDI_MIRRORING_GLYPHS: OnceLock<CodePointMapDataBorrowed<'static, BidiMirroringGlyph>> =
    OnceLock::new();
static LINE_BREAK_CLASSES: OnceLock<CodePointMapDataBorrowed<'static, LineBreak>> = OnceLock::new();
static EAST_ASIAN_WIDTHS: OnceLock<CodePointMapDataBorrowed<'static, EastAsianWidth>> =
    OnceLock::new();
static VERTICAL_ORIENTATIONS: OnceLock<CodePointMapDataBorrowed<'static, VerticalOrientation>> =
    OnceLock::new();
static WORD_BREAK_CLASSES: OnceLock<CodePointMapDataBorrowed<'static, IcuWordBreak>> =
    OnceLock::new();
static GENERAL_CATEGORIES: OnceLock<CodePointMapDataBorrowed<'static, GeneralCategory>> =
    OnceLock::new();
static DEFAULT_IGNORABLE_CODE_POINTS: OnceLock<CodePointSetDataBorrowed<'static>> = OnceLock::new();
static EMOJI_CODE_POINTS: OnceLock<CodePointSetDataBorrowed<'static>> = OnceLock::new();
static EMOJI_PRESENTATION_CODE_POINTS: OnceLock<CodePointSetDataBorrowed<'static>> =
    OnceLock::new();

/// The CSS Text writing system inferred from a declared BCP 47 content
/// language.
///
/// The writing system, rather than the language alone, controls the
/// language-sensitive line-breaking and segment-break tailoring.  An explicit
/// ISO 15924 script subtag takes precedence over the language's usual writing
/// system, for example `ja-Hang` is Korean while `en-Hrkt` is Japanese.
/// <https://drafts.csswg.org/css-text-3/#script-tagging>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContentWritingSystem {
    Unknown,
    Chinese,
    Japanese,
    Korean,
    Yi,
    Other,
}

/// CSS Text 4's relevant CJK punctuation classes.
///
/// The classification is deliberately narrower than generic Unicode
/// punctuation: `text-spacing-trim` operates only on the CJK/fullwidth
/// punctuation classes defined by CSS Text, with language-sensitive treatment
/// for colon and dot punctuation:
/// <https://drafts.csswg.org/css-text-4/#text-spacing-character-classes>.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextSpacingPunctuationClass {
    Opening,
    Closing,
    MiddleDot,
    IdeographicSpace,
    NarrowOpening,
    NarrowClosing,
}

/// Classify one base character for CSS `text-spacing-trim`.
pub(crate) fn text_spacing_punctuation_class(
    character: char,
    language: Option<&str>,
    vertical: bool,
) -> Option<TextSpacingPunctuationClass> {
    if matches!(character, '(' | '[' | '{') {
        return Some(TextSpacingPunctuationClass::NarrowOpening);
    }
    if matches!(character, ')' | ']' | '}') {
        return Some(TextSpacingPunctuationClass::NarrowClosing);
    }
    if character == '\u{3000}' {
        return Some(TextSpacingPunctuationClass::IdeographicSpace);
    }
    if matches!(character, '\u{2018}' | '\u{201c}') {
        return Some(TextSpacingPunctuationClass::Opening);
    }
    if matches!(character, '\u{2019}' | '\u{201d}') {
        return Some(TextSpacingPunctuationClass::Closing);
    }
    if matches!(character, '\u{00b7}' | '\u{2027}' | '\u{30fb}') {
        return Some(TextSpacingPunctuationClass::MiddleDot);
    }
    if matches!(character, '\u{ff1a}' | '\u{ff1b}') {
        return Some(colon_or_dot_spacing_class(language, vertical, true));
    }
    if matches!(character, '\u{3001}' | '\u{3002}' | '\u{ff0c}' | '\u{ff0e}') {
        return Some(colon_or_dot_spacing_class(language, vertical, false));
    }

    let east_asian_width = EAST_ASIAN_WIDTHS
        .get_or_init(CodePointMapData::<EastAsianWidth>::new)
        .get(character);
    let is_fullwidth_cjk_punctuation = matches!(east_asian_width, EastAsianWidth::Fullwidth)
        || ('\u{3000}'..='\u{303f}').contains(&character);
    if !is_fullwidth_cjk_punctuation {
        return None;
    }
    match general_category(character) {
        GeneralCategory::OpenPunctuation => Some(TextSpacingPunctuationClass::Opening),
        GeneralCategory::ClosePunctuation => Some(TextSpacingPunctuationClass::Closing),
        _ => None,
    }
}

fn colon_or_dot_spacing_class(
    language: Option<&str>,
    _vertical: bool,
    colon: bool,
) -> TextSpacingPunctuationClass {
    let language = language.unwrap_or_default().trim().to_ascii_lowercase();
    // CSS Text permits language and writing-mode conventions here. These are
    // the normative informative defaults: simplified Chinese places both on
    // the closing side, traditional Chinese centers both, Japanese centers
    // colons and treats dots as closing, and Korean follows the same split.
    if language.starts_with("zh-hant") || language.starts_with("zh-tw") {
        TextSpacingPunctuationClass::MiddleDot
    } else if language.starts_with("ja") || language.starts_with("ko") {
        if colon {
            TextSpacingPunctuationClass::MiddleDot
        } else {
            TextSpacingPunctuationClass::Closing
        }
    } else {
        TextSpacingPunctuationClass::Closing
    }
}

/// Resolve the CSS Text content writing system from a declared BCP 47 tag.
///
/// Unknown or malformed language tags remain unknown.  A recognized script
/// subtag always wins over the language's customary writing system, so callers
/// cannot accidentally apply Japanese behavior to `ja-Latn` or `ja-Hang`.
/// <https://drafts.csswg.org/css-text-3/#script-tagging>
pub(crate) fn content_writing_system(language: Option<&str>) -> ContentWritingSystem {
    let Some(language) = language else {
        return ContentWritingSystem::Unknown;
    };
    let language = language.trim();
    // BCP 47 uses `-` separators, but accept underscore-separated tags for
    // compatibility. Keep standard tags borrowed so resolving a break policy
    // does not allocate on every inline fragment.
    let normalized = language.contains('_').then(|| language.replace('_', "-"));
    let language = normalized.as_deref().unwrap_or(language);
    let Ok(identifier) = language.parse::<LanguageIdentifier>() else {
        return ContentWritingSystem::Unknown;
    };
    if let Some(script) = identifier.script {
        return match script.as_str() {
            "Hant" | "Hans" | "Hani" | "Hanb" | "Bopo" => ContentWritingSystem::Chinese,
            "Jpan" | "Hrkt" | "Hira" | "Kana" => ContentWritingSystem::Japanese,
            "Kore" | "Hang" | "Jamo" => ContentWritingSystem::Korean,
            "Yiii" => ContentWritingSystem::Yi,
            "Zzzz" => ContentWritingSystem::Unknown,
            _ => ContentWritingSystem::Other,
        };
    }
    match identifier.language.as_str() {
        "zh" => ContentWritingSystem::Chinese,
        "ja" => ContentWritingSystem::Japanese,
        "ko" => ContentWritingSystem::Korean,
        "ii" => ContentWritingSystem::Yi,
        "und" => ContentWritingSystem::Unknown,
        _ => ContentWritingSystem::Other,
    }
}

/// One of CSS Text's cursive scripts.
///
/// CSS Text names these scripts as unable to admit inter-letter gaps. Unicode
/// `Joining_Type` captures directional joining for most of them, but Mongolian
/// and Phags Pa require script-aware contextual handling instead.
/// <https://www.w3.org/TR/css-text-3/#script-spacing>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursiveScript {
    Arabic,
    HanifiRohingya,
    Mandaic,
    Mongolian,
    Nko,
    PhagsPa,
    Syriac,
}

/// Return the CSS Text cursive script of a Unicode letter.
///
/// CSS Text resolves script membership with `Script_Extensions`, rather than
/// `Script` alone. Restricting this to letters keeps script punctuation,
/// digits, marks, and controls from becoming a contextual-joining neighbor.
/// <https://www.w3.org/TR/css-text-3/#script-spacing> and
/// <https://www.unicode.org/reports/tr24/>
fn cursive_script(character: char) -> Option<CursiveScript> {
    if !character_is_unicode_letter(character) {
        return None;
    }
    let scripts = ScriptWithExtensions::new();
    [
        (IcuScript::Arabic, CursiveScript::Arabic),
        (IcuScript::HanifiRohingya, CursiveScript::HanifiRohingya),
        (IcuScript::Mandaic, CursiveScript::Mandaic),
        (IcuScript::Mongolian, CursiveScript::Mongolian),
        (IcuScript::Nko, CursiveScript::Nko),
        (IcuScript::PhagsPa, CursiveScript::PhagsPa),
        (IcuScript::Syriac, CursiveScript::Syriac),
    ]
    .into_iter()
    .find_map(|(script, cursive)| scripts.has_script(character, script).then_some(cursive))
}

/// Return whether a character has Unicode directional joining behavior.
///
/// This intentionally remains a direct `Joining_Type` query. Arabic
/// presentation-form scalars are not source letters that may acquire a new
/// join, and therefore remain non-joining here.
/// <https://www.unicode.org/reports/tr44/#Joining_Type>
fn character_has_unicode_joining_behavior(character: char) -> bool {
    matches!(
        joining_type(character),
        JoiningType::JoinCausing
            | JoiningType::DualJoining
            | JoiningType::LeftJoining
            | JoiningType::RightJoining
    )
}

/// Return whether a character participates in CSS cursive shaping.
///
/// CSS Text prevents gaps within all seven listed cursive scripts. Unicode
/// directional joining covers Arabic, Hanifi Rohingya, Mandaic, N'Ko, and
/// Syriac; Mongolian and Phags Pa letter shaping is contextual despite not
/// exposing the required `Joining_Type` directionality.
/// <https://www.w3.org/TR/css-text-3/#script-spacing> and
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>
pub(crate) fn character_has_cursive_shaping_behavior(character: char) -> bool {
    character_has_unicode_joining_behavior(character) || cursive_script(character).is_some()
}

/// Return whether two source letters need contextual shaping across their
/// boundary.
///
/// The CSS Text cursive script must match first: Unicode `Joining_Type` alone
/// can otherwise make characters from unrelated cursive scripts look
/// compatible. Within one supported script, Unicode joining scripts use their
/// directional compatibility. Mongolian and Phags Pa form contextual glyphs
/// without that directional property, so their same-script letter pairs always
/// receive shaping context.
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>
pub(crate) fn cursive_boundary_needs_context(left: char, right: char) -> bool {
    match (cursive_script(left), cursive_script(right)) {
        (Some(CursiveScript::Mongolian), Some(CursiveScript::Mongolian))
        | (Some(CursiveScript::PhagsPa), Some(CursiveScript::PhagsPa)) => true,
        (Some(left_script), Some(right_script)) if left_script == right_script => {
            character_can_join_following(left) && character_can_join_preceding(right)
        }
        _ => false,
    }
}

/// Return whether a character can join the following logical character.
///
/// CSS Text boundary shaping must preserve cursive joins across inline
/// boundaries, but Unicode Joining_Type is directional: a synthetic ZWJ is
/// valid only when the left-side character can connect forward and the
/// right-side character can connect backward:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.unicode.org/reports/tr44/#Joining_Type>.
pub(super) fn character_can_join_following(character: char) -> bool {
    matches!(
        joining_type(character),
        JoiningType::JoinCausing | JoiningType::DualJoining | JoiningType::LeftJoining
    )
}

/// Return whether a character can join the preceding logical character.
///
/// This is the right-side half of CSS Text boundary-shaping compatibility and
/// uses Unicode Joining_Type rather than script or code point ranges:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.unicode.org/reports/tr44/#Joining_Type>.
pub(super) fn character_can_join_preceding(character: char) -> bool {
    matches!(
        joining_type(character),
        JoiningType::JoinCausing | JoiningType::DualJoining | JoiningType::RightJoining
    )
}

/// Return whether a character is a Unicode join-control format character.
///
/// CSS Text requires shaping behavior to honor the Unicode join controls
/// U+200C ZERO WIDTH NON-JOINER and U+200D ZERO WIDTH JOINER, even when font
/// fallback or inline markup would otherwise split glyph runs:
/// <https://www.w3.org/TR/css-text-3/#text-encoding> and
/// <https://www.unicode.org/reports/tr44/#Join_Control>.
pub(crate) fn character_is_join_control(character: char) -> bool {
    JOIN_CONTROLS
        .get_or_init(CodePointSetData::new::<JoinControl>)
        .contains(character)
}

/// Return whether a character is U+0640 ARABIC TATWEEL.
///
/// ALReq describes tatweel as a joining character used to extend Arabic
/// cursive connections. It is visible text, unlike ZWJ/ZWNJ, but it must still
/// provide joining context across font and inline style boundaries:
/// <https://www.w3.org/TR/alreq/#h_joining_enforcement>.
pub(crate) fn character_is_arabic_tatweel(character: char) -> bool {
    character == '\u{0640}'
}

/// Return the Unicode Line_Break class for CSS Text line breaking.
///
/// CSS Text line breaking is based on UAX #14 classes, with CSS-specific
/// tailoring layered on top by `line-break`, `word-break`, and white-space:
/// <https://www.w3.org/TR/css-text-3/#line-breaking> and
/// <https://www.unicode.org/reports/tr14/>.
pub(super) fn line_break_class(character: char) -> LineBreak {
    LINE_BREAK_CLASSES
        .get_or_init(CodePointMapData::<LineBreak>::new)
        .get(character)
}

/// Return whether UAX #14 classifies this scalar as an unconditional line
/// break independently of CSS `white-space` segment-break processing.
///
/// `BK` and `NL` are mandatory breaks. CSS Text keeps LF/CR in its distinct
/// segment-break transformation pipeline, so they are deliberately excluded
/// here and handled by inline collection before this predicate is reached.
/// <https://www.unicode.org/reports/tr14/#BK> and
/// <https://drafts.csswg.org/css-text-3/#line-break-details>
pub(crate) fn character_is_mandatory_line_break(character: char) -> bool {
    matches!(
        line_break_class(character),
        LineBreak::MandatoryBreak | LineBreak::NextLine
    )
}

/// The UAX #14 class that protects a CSS Text atomic-inline boundary.
///
/// Atomic inlines normally gain soft-wrap opportunities on both sides, but
/// CSS Text keeps the UAX #14 `GL`, `WJ`, and `ZWJ` protections. U+00A0 is a
/// deliberate compatibility exception to that atomic-inline override; other
/// `GL` characters, including U+202F NARROW NO-BREAK SPACE, remain protected.
///
/// Keeping this distinct from CSS Text's *other space separator* classification
/// prevents line-edge whitespace processing from accidentally changing UAX #14
/// break behavior.
/// <https://www.w3.org/TR/css-text-3/#line-breaking>
/// <https://www.w3.org/TR/css-text-3/#line-break-details>
/// <https://www.unicode.org/reports/tr14/#GL>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Uax14BoundaryProtection {
    None,
    Glue,
    WordJoiner,
    ZeroWidthJoiner,
}

/// Classify a character's retained UAX #14 protection at an atomic-inline
/// boundary.
///
/// This is intentionally not a generic line-break predicate: ordinary text
/// boundaries are resolved by the complete UAX #14 segmenter, which preserves
/// context-sensitive exceptions such as LB12a's break before `GL` after a
/// regular space or hyphen.
pub(crate) fn uax14_atomic_boundary_protection(character: char) -> Uax14BoundaryProtection {
    if character == '\u{00a0}' {
        return Uax14BoundaryProtection::None;
    }
    match line_break_class(character) {
        LineBreak::Glue => Uax14BoundaryProtection::Glue,
        LineBreak::WordJoiner => Uax14BoundaryProtection::WordJoiner,
        LineBreak::ZWJ => Uax14BoundaryProtection::ZeroWidthJoiner,
        _ => Uax14BoundaryProtection::None,
    }
}

/// Neighboring text and writing-system context used to transform a collapsible
/// CSS Text segment break.
///
/// The segment break itself belongs to the style that produced it. Keeping the
/// language alongside both neighboring characters prevents collection from
/// baking an untagged East-Asian-width heuristic into every inline boundary.
/// <https://drafts.csswg.org/css-text-3/#line-break-transform> and
/// <https://drafts.csswg.org/css-text-3/#script-tagging>
#[derive(Clone, Copy, Debug)]
pub(crate) struct SegmentBreakContext<'a> {
    pub(crate) before: char,
    pub(crate) after: char,
    /// Whether the preceding text token contains a currency symbol, such as
    /// the `1.00` at the end of `$1.00`.
    pub(crate) before_is_currency_amount: bool,
    pub(crate) language: Option<&'a str>,
}

/// Return whether CSS Text removes the segment break in this typed context.
///
/// Level 3 leaves the choice between a space and removal to language-aware UA
/// rules. Quire uses no-word-separator behavior for the Chinese, Japanese,
/// and Yi writing systems, and retains the conservative Unicode width fallback
/// for untagged content. Currency symbols and Hangul retain their established
/// word-separating behavior. Default ignorables are skipped by inline
/// collection before this resolver is called.
/// <https://drafts.csswg.org/css-text-3/#line-break-transform> and
/// <https://drafts.csswg.org/css-text-3/#script-tagging>
pub(crate) fn segment_break_is_removable(context: SegmentBreakContext<'_>) -> bool {
    let SegmentBreakContext {
        before,
        after,
        before_is_currency_amount,
        language,
    } = context;
    if before == '\u{200b}' || after == '\u{200b}' {
        return true;
    }
    if before_is_currency_amount
        || character_is_currency_symbol(before)
        || character_is_currency_symbol(after)
        || character_is_hangul_for_segment_break(before)
        || character_is_hangul_for_segment_break(after)
    {
        return false;
    }
    if writing_system_omits_word_separators(language) {
        return true;
    }
    east_asian_segment_break_width(before) && east_asian_segment_break_width(after)
}

/// Return whether the declared language identifies a writing system that
/// conventionally omits inter-word separators.
///
/// CSS Text requires at least Chinese, Japanese, and Yi to be identified from
/// language and ISO 15924 script subtags. This deliberately recognizes script
/// subtags independently of their primary language, such as `ain-Kana`.
/// <https://drafts.csswg.org/css-text-3/#script-tagging>
fn writing_system_omits_word_separators(language: Option<&str>) -> bool {
    matches!(
        content_writing_system(language),
        ContentWritingSystem::Chinese | ContentWritingSystem::Japanese | ContentWritingSystem::Yi
    )
}

/// Return whether a scalar has Unicode General_Category `Sc`.
///
/// Segment-break transformation protects complete currency amounts from
/// no-word-separator writing-system tailoring, while text collection uses the
/// same property to identify the amount's preceding token.
/// <https://drafts.csswg.org/css-text-3/#line-break-transform> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>
pub(crate) fn character_is_currency_symbol(character: char) -> bool {
    matches!(general_category(character), GeneralCategory::CurrencySymbol)
}

fn east_asian_segment_break_width(character: char) -> bool {
    matches!(
        EAST_ASIAN_WIDTHS
            .get_or_init(CodePointMapData::<EastAsianWidth>::new)
            .get(character),
        EastAsianWidth::Fullwidth | EastAsianWidth::Wide | EastAsianWidth::Halfwidth
    )
}

fn character_is_hangul_for_segment_break(character: char) -> bool {
    matches!(
        line_break_class(character),
        LineBreak::H2 | LineBreak::H3 | LineBreak::JL | LineBreak::JV | LineBreak::JT
    )
}

/// Return the Unicode `Vertical_Orientation` class for a character.
///
/// CSS Writing Modes defines `text-orientation: mixed` in terms of Unicode
/// Vertical_Orientation. Keeping this lookup in the shared text property layer
/// lets vertical placement, diagnostics, and tests use the same policy:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation> and
/// <https://www.unicode.org/reports/tr50/#vo>.
pub(crate) fn character_vertical_orientation(character: char) -> VerticalOrientation {
    VERTICAL_ORIENTATIONS
        .get_or_init(CodePointMapData::<VerticalOrientation>::new)
        .get(character)
}

/// Return whether a character belongs to a script with an intrinsic vertical
/// presentation.
///
/// UAX #50 assigns Mongolian and Phags Pa letters the default `R` value
/// because their code-chart glyphs are shown in their horizontal-text form.
/// CSS Writing Modes separately classifies both scripts as vertical scripts.
/// The resolved vertical-unit plan uses this only to select the coherent
/// horizontal-composition/sideways path needed for their intrinsic form.
/// <https://drafts.csswg.org/css-writing-modes-4/#vertical-orientations>
/// <https://www.unicode.org/reports/tr50/>
pub(crate) fn character_is_native_vertical_script(character: char) -> bool {
    let script = CodePointMapData::<IcuScript>::new().get(character);
    PropertyNamesShort::<IcuScript>::new()
        .get_locale_script(script)
        .is_some_and(|script| script.into_raw() == *b"Mong" || script.into_raw() == *b"Phag")
}

/// The orientation selected for one typographic unit by `text-orientation: mixed`.
///
/// CSS Writing Modes maps Unicode `U`, `Tu`, and `Tr` units to upright
/// typesetting; only `R` units are typeset sideways. Keep this as one policy rather
/// than allowing shaping and paint to classify transformed units differently:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MixedTextOrientation {
    Upright,
    Sideways,
}

/// Return the `text-orientation: mixed` policy for one typographic unit.
///
/// Combining marks and default-ignorable controls inherit the first visible
/// base character in the unit instead of deciding orientation on their own.
pub(crate) fn typographic_unit_mixed_orientation(text: &str) -> MixedTextOrientation {
    if text
        .chars()
        .find(|character| !character_inherits_vertical_orientation(*character))
        .is_some_and(|character| {
            matches!(
                character_vertical_orientation(character),
                VerticalOrientation::Upright
                    | VerticalOrientation::TransformedUpright
                    | VerticalOrientation::TransformedRotated
            )
        })
    {
        MixedTextOrientation::Upright
    } else {
        MixedTextOrientation::Sideways
    }
}

/// Return whether one typographic unit is upright under `text-orientation: mixed`.
pub(crate) fn typographic_unit_is_upright_in_mixed_orientation(text: &str) -> bool {
    typographic_unit_mixed_orientation(text) == MixedTextOrientation::Upright
}

/// Return whether one typographic unit selects OpenType vertical glyph forms
/// under `text-orientation: mixed`.
///
pub(crate) fn character_inherits_vertical_orientation(character: char) -> bool {
    character_is_default_ignorable_code_point(character)
        || GeneralCategoryGroup::Mark.contains(general_category(character))
}

/// Return whether a character is a CSS Text "other space separator".
///
/// CSS Text distinguishes document white space such as U+0020 SPACE and tabs
/// from other Unicode separator characters. U+00A0 NO-BREAK SPACE remains an
/// inter-word separator with UAX #14 GL behavior, not an unconditional
/// hanging separator. The remaining characters stay visible text, but CSS
/// Text phase II lets them hang at the end of lines for collapsing white-space
/// modes:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(crate) fn character_is_css_other_space_separator(character: char) -> bool {
    character != '\u{00a0}'
        && !is_css_collapsible_whitespace(character)
        && matches!(
            GENERAL_CATEGORIES
                .get_or_init(CodePointMapData::<GeneralCategory>::new)
                .get(character),
            GeneralCategory::SpaceSeparator
        )
}

/// Return whether a character is a CSS Text inter-word justification separator.
///
/// CSS Text expands `text-justify: inter-word` at word separators rather than
/// at every typographic character unit. The separator set includes CSS
/// document spaces, Unicode space separators such as no-break spaces, and the
/// encoded historical word-separator punctuation characters used for scripts
/// whose writing systems mark word boundaries with punctuation:
/// <https://www.w3.org/TR/css-text-3/#valdef-text-justify-inter-word>,
/// <https://www.w3.org/TR/css-text-3/#word-separator>, and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_css_word_separator(character: char) -> bool {
    character == '\u{00a0}'
        || is_css_preserved_document_space(character)
        || character_is_css_other_space_separator(character)
        || matches!(
            character,
            '\u{1361}' // ETHIOPIC WORDSPACE
                | '\u{10100}' // AEGEAN WORD SEPARATOR LINE
                | '\u{10101}' // AEGEAN WORD SEPARATOR DOT
                | '\u{1039f}' // UGARITIC WORD DIVIDER
                | '\u{1091f}' // PHOENICIAN WORD SEPARATOR
        )
}

/// Return whether a character is a CSS Text Decoration spacer.
///
/// `text-decoration-skip-spaces` defines spacers using Unicode General
/// Category `Zs`, excluding U+202F NARROW NO-BREAK SPACE:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>.
pub(crate) fn character_is_text_decoration_spacer(character: char) -> bool {
    if character == '\u{202f}' {
        return false;
    }
    let category = general_category(character);
    GeneralCategoryGroup::Separator.contains(category)
        && matches!(category, GeneralCategory::SpaceSeparator)
}

/// Return whether a character receives a CSS text emphasis mark.
///
/// CSS Text Decoration excludes separators, punctuation, controls, formatting
/// characters, and unassigned code points from emphasis marks. Unicode
/// General_Category data is the normative class source for this filtering:
/// <https://www.w3.org/TR/css-text-decor-3/#text-emphasis-style-property> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_receives_text_emphasis_mark(character: char) -> bool {
    let category = general_category(character);
    !GeneralCategoryGroup::Separator
        .union(GeneralCategoryGroup::Punctuation)
        .union(GeneralCategoryGroup::Control)
        .union(GeneralCategoryGroup::Format)
        .union(GeneralCategoryGroup::Unassigned)
        .contains(category)
}

/// Return whether a character can hang for `hanging-punctuation: last`.
///
/// CSS Text includes Unicode categories Pe, Pf, and Pi, plus ASCII quotes,
/// when deciding whether the last punctuation mark may hang at line end:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_last_hangable_punctuation(character: char) -> bool {
    if matches!(character, '"' | '\'') {
        return true;
    }
    matches!(
        GeneralCategoryGroup::from(general_category(character)),
        GeneralCategoryGroup::ClosePunctuation
            | GeneralCategoryGroup::FinalPunctuation
            | GeneralCategoryGroup::InitialPunctuation
    )
}

/// Return whether a character can hang for `hanging-punctuation: first`.
///
/// CSS Text includes opening punctuation categories Ps/Pf/Pi, ASCII quotes,
/// and U+3000 IDEOGRAPHIC SPACE when `first` is enabled:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_first_hangable_punctuation(character: char) -> bool {
    if matches!(character, '"' | '\'' | '\u{3000}') {
        return true;
    }
    matches!(
        GeneralCategoryGroup::from(general_category(character)),
        GeneralCategoryGroup::OpenPunctuation
            | GeneralCategoryGroup::FinalPunctuation
            | GeneralCategoryGroup::InitialPunctuation
    )
}

/// Return whether a character can hang for `force-end`/`allow-end`.
///
/// CSS Text defines the stop/comma set explicitly and lets UAs add more. This
/// renderer uses the standardized set only so behavior is predictable:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(crate) fn character_is_hangable_stop_or_comma(character: char) -> bool {
    matches!(
        character,
        ',' | '.'
            | '\u{060c}'
            | '\u{06d4}'
            | '\u{3001}'
            | '\u{3002}'
            | '\u{ff0c}'
            | '\u{ff0e}'
            | '\u{fe50}'
            | '\u{fe51}'
            | '\u{fe52}'
            | '\u{ff61}'
            | '\u{ff64}'
    )
}

/// Return whether a character belongs to a Unicode letter category.
///
/// CSS Text hyphenation operates on words in Unicode text, so word-letter
/// detection must come from Unicode general categories rather than ASCII or
/// locale-specific shortcuts:
/// <https://www.w3.org/TR/css-text-3/#hyphenation> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_unicode_letter(character: char) -> bool {
    GeneralCategoryGroup::Letter.contains(general_category(character))
}

/// Return whether a character can be the typographic letter selected by CSS
/// `text-transform: capitalize`.
///
/// Unicode `Nl` Letter_Number characters, notably Roman numerals, have case
/// mappings but are outside the `L*` General_Category group. CSS Text asks for
/// the first typographic letter unit rather than only an alphabetic letter.
/// <https://www.w3.org/TR/css-text-3/#valdef-text-transform-capitalize> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>
pub(crate) fn character_is_unicode_typographic_letter(character: char) -> bool {
    character_is_unicode_letter(character)
        || matches!(general_category(character), GeneralCategory::LetterNumber)
}

/// Return whether a character belongs to a Unicode letter or number category.
///
/// CSS Text `text-transform: capitalize` finds word starts in Unicode text;
/// using the Unicode general category keeps non-ASCII letters and digits in
/// the same word model as the shaping and line-breaking stack:
/// <https://www.w3.org/TR/css-text-3/#text-transform-property> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_unicode_alphanumeric(character: char) -> bool {
    let category = general_category(character);
    GeneralCategoryGroup::Letter.contains(category)
        || GeneralCategoryGroup::Number.contains(category)
}

/// Return whether a character belongs to a Unicode punctuation category.
///
/// CSS Pseudo-Elements includes punctuation adjacent to the first typographic
/// letter in `::first-letter`; using Unicode General Category keeps that
/// selection consistent with the renderer's shaping and line-breaking data:
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_unicode_punctuation(character: char) -> bool {
    GeneralCategoryGroup::Punctuation.contains(general_category(character))
}

/// Return whether a character can be the typographic initial selected by
/// `::first-letter`.
///
/// CSS Pseudo-Elements selects the first typographic character unit whose
/// base character is in Unicode's Letter, Number, or Symbol categories.
/// Keeping this classification beside the general-category helpers prevents
/// first-letter selection from drifting from the Unicode data used elsewhere
/// in text layout:
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>.
pub(crate) fn character_is_unicode_first_letter_base(character: char) -> bool {
    character_is_unicode_alphanumeric(character) || character_is_unicode_symbol(character)
}

/// Return whether a character is associated space around `::first-letter`
/// punctuation.
///
/// CSS Pseudo-Elements includes `Zs` characters, except U+3000 IDEOGRAPHIC
/// SPACE, between associated punctuation and the typographic initial. The
/// caller decides whether that space is actually attached to preceding or
/// following punctuation:
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>.
pub(crate) fn character_is_first_letter_associated_space(character: char) -> bool {
    character != '\u{3000}'
        && matches!(general_category(character), GeneralCategory::SpaceSeparator)
}

/// Return whether punctuation can be associated after a `::first-letter`
/// typographic initial.
///
/// Opening and dash punctuation terminate the suffix; all other Unicode `P*`
/// punctuation remains part of the first-letter text:
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>.
pub(crate) fn character_is_first_letter_suffix_punctuation(character: char) -> bool {
    matches!(
        general_category(character),
        GeneralCategory::ClosePunctuation
            | GeneralCategory::ConnectorPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::OtherPunctuation
    )
}

/// Return whether a character belongs to a Unicode symbol category.
///
/// CSS Text Decoration's `text-emphasis-skip: symbols` is defined in terms of
/// typographic character classes. Unicode General Category provides the symbol
/// class used by the prepared emphasis annotation policy:
/// <https://drafts.csswg.org/css-text-decor-4/#text-emphasis-skip-property>
/// and <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_unicode_symbol(character: char) -> bool {
    GeneralCategoryGroup::Symbol.contains(general_category(character))
}

/// Return whether a character belongs to a Unicode mark category.
///
/// CSS text emphasis is assigned to typographic character units, so combining
/// marks must inherit the decision from their base character instead of
/// independently creating emphasis annotations:
/// <https://www.w3.org/TR/css-text-3/#typographic-character-unit> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_unicode_mark(character: char) -> bool {
    GeneralCategoryGroup::Mark.contains(general_category(character))
}

/// Return whether a character is an ideograph for CSS `text-autospace`.
///
/// CSS Text Level 4 defines this class independently of UAX #14's broader
/// `Ideographic` line-break class. In particular, it includes Japanese kana,
/// CJK strokes, Katakana extensions, and every character whose
/// `Script_Extensions` includes Han, while excluding punctuation:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property> and
/// <https://www.unicode.org/reports/tr24/>.
pub(crate) fn character_is_autospace_ideograph(character: char) -> bool {
    !GeneralCategoryGroup::Punctuation.contains(general_category(character))
        && (('\u{3041}'..='\u{30ff}').contains(&character)
            || ('\u{31c0}'..='\u{31ef}').contains(&character)
            || ('\u{31f0}'..='\u{31ff}').contains(&character)
            || ScriptWithExtensions::new().has_script(character, IcuScript::Han))
}

/// Return whether a scalar is eligible for the UA's default ruby
/// inter-character distribution.
///
/// CSS Ruby's `text-justify: ruby` distributes between adjacent CJK
/// characters, but not adjacent Latin or Bopomofo characters. Unicode East
/// Asian Width supplies the CJK-wide classification without maintaining
/// brittle script ranges in the ruby formatter.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-align-property>
pub(crate) fn character_is_ruby_justification_eligible(character: char) -> bool {
    matches!(
        EAST_ASIAN_WIDTHS
            .get_or_init(CodePointMapData::<EastAsianWidth>::new)
            .get(character),
        EastAsianWidth::Fullwidth | EastAsianWidth::Wide
    )
}

/// Return whether a character is a non-ideographic letter for autospace.
///
/// The `ideograph-alpha` value inserts spacing between Han ideographs and
/// adjacent letters. CSS Text includes both Unicode Letter and Mark units,
/// but excludes ideographs and East Asian Wide or Fullwidth units:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_autospace_alpha(character: char) -> bool {
    !character_is_autospace_ideograph(character)
        && (GeneralCategoryGroup::Letter.contains(general_category(character))
            || GeneralCategoryGroup::Mark.contains(general_category(character)))
        && !matches!(
            EAST_ASIAN_WIDTHS
                .get_or_init(CodePointMapData::<EastAsianWidth>::new)
                .get(character),
            EastAsianWidth::Wide | EastAsianWidth::Fullwidth
        )
}

/// Return whether a character is numeric for CSS `text-autospace`.
///
/// The `ideograph-numeric` value inserts spacing between Han ideographs and
/// adjacent Unicode decimal digits, not just ASCII digits. Fullwidth digits
/// are explicitly excluded:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_autospace_numeric(character: char) -> bool {
    matches!(general_category(character), GeneralCategory::DecimalNumber)
        && !matches!(
            EAST_ASIAN_WIDTHS
                .get_or_init(CodePointMapData::<EastAsianWidth>::new)
                .get(character),
            EastAsianWidth::Fullwidth
        )
}

/// Return whether a non-word segment can keep CSS capitalize word context.
///
/// CSS Text leaves the exact word-boundary detection for `capitalize` to the
/// UA, but the implementation should use Unicode word-boundary data rather
/// than treating every punctuation mark as a break. These UAX #29
/// word-internal classes let inline-fragment boundaries preserve context
/// around apostrophes, middle dots, and similar connectors:
/// <https://www.w3.org/TR/css-text-3/#text-transform-property> and
/// <https://www.unicode.org/reports/tr29/#Word_Boundaries>.
pub(crate) fn character_preserves_word_boundary_context(character: char) -> bool {
    matches!(
        WORD_BREAK_CLASSES
            .get_or_init(CodePointMapData::<IcuWordBreak>::new)
            .get(character),
        IcuWordBreak::MidLetter
            | IcuWordBreak::MidNum
            | IcuWordBreak::MidNumLet
            | IcuWordBreak::SingleQuote
            | IcuWordBreak::DoubleQuote
            | IcuWordBreak::ExtendNumLet
    )
}

/// Return whether text contains right-to-left or Arabic-letter bidi classes.
///
/// CSS Writing Modes and Unicode Bidirectional Algorithm processing depend on
/// Unicode bidi classes rather than script-specific code point ranges:
/// <https://www.w3.org/TR/css-writing-modes-3/#text-direction> and
/// <https://www.unicode.org/reports/tr9/>.
pub(super) fn character_has_rtl_bidi_class(character: char) -> bool {
    matches!(
        BIDI_CLASSES
            .get_or_init(CodePointMapData::<BidiClass>::new)
            .get(character),
        BidiClass::RightToLeft | BidiClass::ArabicLetter
    )
}

/// Return the Unicode Bidi_Mirroring_Glyph counterpart for a scalar.
///
/// UAX #9 L4 changes the displayed glyph at an odd resolved embedding level,
/// but it does not change the underlying Unicode text retained for extraction:
/// <https://www.unicode.org/reports/tr9/#L4> and
/// <https://www.unicode.org/reports/tr44/#Bidi_Mirroring_Glyph>.
pub(crate) fn bidi_mirroring_glyph(character: char) -> Option<char> {
    BIDI_MIRRORING_GLYPHS
        .get_or_init(CodePointMapData::<BidiMirroringGlyph>::new)
        .get(character)
        .mirroring_glyph
}

/// Return whether a code point is a UBA directional formatting control.
///
/// CSS `unicode-bidi` maps inline scoping to Unicode Bidirectional Algorithm
/// formatting controls. These characters participate in ordering but never
/// generate visible glyphs:
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi> and
/// <https://www.unicode.org/reports/tr9/#Directional_Formatting_Characters>.
pub(crate) fn character_is_bidi_format_control(character: char) -> bool {
    !character_is_join_control(character)
        && BIDI_CONTROLS
            .get_or_init(CodePointSetData::new::<BidiControl>)
            .contains(character)
}

/// Return whether a character is a Unicode control character.
///
/// CSS Text has special handling for document white-space controls, bidi
/// controls, and other visible control characters. The base classification
/// comes from Unicode General_Category Cc:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_unicode_control(character: char) -> bool {
    matches!(general_category(character), GeneralCategory::Control)
}

/// Return whether a code point has Unicode `Default_Ignorable_Code_Point`.
///
/// CSS Text, CSS Writing Modes, and shaping all need format controls such as
/// variation selectors, join controls, and bidi isolates to affect text
/// processing without necessarily painting visible glyphs. Unicode's derived
/// core property is the central source for that default-ignorable set:
/// <https://www.unicode.org/reports/tr44/#Default_Ignorable_Code_Point> and
/// <https://www.w3.org/TR/css-text-3/#text-processing-order>.
pub(crate) fn character_is_default_ignorable_code_point(character: char) -> bool {
    DEFAULT_IGNORABLE_CODE_POINTS
        .get_or_init(CodePointSetData::new::<DefaultIgnorableCodePoint>)
        .contains(character)
}

/// Returns whether a character has Unicode's `Emoji` property.
///
/// This is used only to select the platform's `emoji` generic when script
/// fallback cannot represent Common-script emoji characters.
/// <https://www.unicode.org/reports/tr51/#Emoji_Properties>
pub(crate) fn character_is_emoji(character: char) -> bool {
    EMOJI_CODE_POINTS
        .get_or_init(CodePointSetData::new::<Emoji>)
        .contains(character)
}

/// Return whether an emoji character has Unicode emoji presentation by
/// default, before an explicit U+FE0E/U+FE0F selector is applied.
/// <https://unicode.org/reports/tr51/#Emoji_Presentation>
pub(crate) fn character_has_emoji_presentation(character: char) -> bool {
    EMOJI_PRESENTATION_CODE_POINTS
        .get_or_init(CodePointSetData::new::<EmojiPresentation>)
        .contains(character)
}

/// Return whether a default-ignorable control is neutral for font selection.
///
/// CSS Text shaping must preserve controls that affect glyph selection or
/// ordering, such as join controls, bidi controls, and Unicode variation
/// selectors. Other default-ignorable controls, including CGJ and MVS, affect
/// text processing such as line breaking but must not force visible glyphs
/// into a fallback font during PDF text emission:
/// <https://www.w3.org/TR/css-text-3/#text-processing-order>,
/// <https://www.w3.org/TR/css-text-3/#line-break-details>, and
/// <https://www.unicode.org/reports/tr44/#Default_Ignorable_Code_Point>.
pub(crate) fn character_is_font_neutral_default_ignorable(character: char) -> bool {
    character_is_default_ignorable_code_point(character)
        && !character_is_join_control(character)
        && !character_is_bidi_format_control(character)
        && !character_is_variation_selector(character)
}

fn character_is_variation_selector(character: char) -> bool {
    matches!(character, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}

/// Resolve plaintext paragraph direction from the first strong bidi character.
///
/// CSS Writing Modes `unicode-bidi: plaintext` uses the Unicode Bidirectional
/// Algorithm's paragraph direction resolution for each plaintext paragraph or
/// line, which starts from the first strong directional character:
/// <https://www.w3.org/TR/css-writing-modes-4/#valdef-unicode-bidi-plaintext>
/// and <https://www.unicode.org/reports/tr9/#The_Paragraph_Level>.
pub(crate) fn plaintext_direction_for_text(text: &str) -> Option<Direction> {
    text.chars().find_map(|character| {
        match BIDI_CLASSES
            .get_or_init(CodePointMapData::<BidiClass>::new)
            .get(character)
        {
            BidiClass::LeftToRight => Some(Direction::Ltr),
            BidiClass::RightToLeft | BidiClass::ArabicLetter => Some(Direction::Rtl),
            _ => None,
        }
    })
}

fn joining_type(character: char) -> JoiningType {
    JOINING_TYPES
        .get_or_init(CodePointMapData::<JoiningType>::new)
        .get(character)
}

fn general_category(character: char) -> GeneralCategory {
    GENERAL_CATEGORIES
        .get_or_init(CodePointMapData::<GeneralCategory>::new)
        .get(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autospace_classes_follow_css_text_character_definitions() {
        assert!(character_is_autospace_ideograph('あ'));
        assert!(character_is_autospace_ideograph('\u{31c0}'));
        assert!(character_is_autospace_ideograph('\u{31f0}'));
        assert!(character_is_autospace_ideograph('\u{20000}'));
        assert!(!character_is_autospace_ideograph('。'));

        assert!(character_is_autospace_alpha('A'));
        assert!(character_is_autospace_alpha('\u{0301}'));
        assert!(!character_is_autospace_alpha('Ａ'));
        assert!(!character_is_autospace_alpha('中'));

        assert!(character_is_autospace_numeric('3'));
        assert!(!character_is_autospace_numeric('３'));
        assert!(!character_is_autospace_numeric('Ⅳ'));
    }

    #[test]
    fn unicode_vertical_orientation_helper_returns_expected_classes() {
        assert_eq!(
            character_vertical_orientation('a'),
            VerticalOrientation::Rotated
        );
        assert_eq!(
            character_vertical_orientation('§'),
            VerticalOrientation::Upright
        );
        assert_eq!(
            character_vertical_orientation('、'),
            VerticalOrientation::TransformedUpright
        );
        assert_eq!(
            character_vertical_orientation('中'),
            VerticalOrientation::Upright
        );
        assert_eq!(
            character_vertical_orientation('！'),
            VerticalOrientation::TransformedUpright
        );
        assert_eq!(
            character_vertical_orientation('\u{2329}'),
            VerticalOrientation::TransformedRotated
        );
    }

    #[test]
    fn native_vertical_script_classification_identifies_mongolian_and_phags_pa() {
        assert!(character_is_native_vertical_script('\u{1828}'));
        assert!(character_is_native_vertical_script('\u{a840}'));
        assert!(!typographic_unit_is_upright_in_mixed_orientation(
            "\u{1828}"
        ));
        assert!(!character_is_native_vertical_script('a'));
    }

    #[test]
    fn mixed_orientation_policy_uses_visible_base_character() {
        assert_eq!(
            typographic_unit_mixed_orientation("a"),
            MixedTextOrientation::Sideways,
            "Unicode Vertical_Orientation=R remains sideways"
        );
        assert_eq!(
            typographic_unit_mixed_orientation("§"),
            MixedTextOrientation::Upright,
            "Unicode Vertical_Orientation=U is upright"
        );
        assert_eq!(
            typographic_unit_mixed_orientation("、"),
            MixedTextOrientation::Upright,
            "Unicode Vertical_Orientation=Tu is upright"
        );
        assert_eq!(
            typographic_unit_mixed_orientation("\u{2329}"),
            MixedTextOrientation::Upright,
            "Unicode Vertical_Orientation=Tr is upright"
        );
        assert!(!typographic_unit_is_upright_in_mixed_orientation(
            "\u{0301}"
        ));
        assert!(!typographic_unit_is_upright_in_mixed_orientation(
            "\u{0301}a"
        ));
        assert!(typographic_unit_is_upright_in_mixed_orientation(
            "\u{200d}中"
        ));

        assert_eq!(
            typographic_unit_mixed_orientation("\u{0301}\u{2329}"),
            MixedTextOrientation::Upright
        );
        assert_eq!(
            typographic_unit_mixed_orientation("a\u{0301}"),
            MixedTextOrientation::Sideways,
            "a combining mark inherits the preceding base typographic unit"
        );
    }

    #[test]
    fn segment_break_keeps_currency_symbols_separate_from_cjk_text() {
        assert!(east_asian_segment_break_width('₩'));
        assert!(!segment_break_is_removable(SegmentBreakContext {
            before: '価',
            after: '₩',
            before_is_currency_amount: false,
            language: Some("ja"),
        }));
        assert!(!segment_break_is_removable(SegmentBreakContext {
            before: '₩',
            after: '格',
            before_is_currency_amount: false,
            language: Some("ja"),
        }));
        assert!(!segment_break_is_removable(SegmentBreakContext {
            before: '0',
            after: '格',
            before_is_currency_amount: true,
            language: Some("ja"),
        }));
    }

    #[test]
    fn segment_break_uses_declared_no_word_separator_writing_systems() {
        assert!(segment_break_is_removable(SegmentBreakContext {
            before: 'E',
            after: '～',
            before_is_currency_amount: false,
            language: Some("ja"),
        }));
        assert!(segment_break_is_removable(SegmentBreakContext {
            before: '“',
            after: 'ア',
            before_is_currency_amount: false,
            language: Some("ain-Kana"),
        }));
        assert!(!segment_break_is_removable(SegmentBreakContext {
            before: 'E',
            after: '～',
            before_is_currency_amount: false,
            language: Some("ja-Hang"),
        }));
        assert!(segment_break_is_removable(SegmentBreakContext {
            before: 'E',
            after: '～',
            before_is_currency_amount: false,
            language: Some("ko-Hani"),
        }));
        assert!(!segment_break_is_removable(SegmentBreakContext {
            before: 'E',
            after: '～',
            before_is_currency_amount: false,
            language: Some("en"),
        }));
    }

    #[test]
    fn explicit_script_subtags_override_the_language_writing_system() {
        assert_eq!(
            content_writing_system(Some("ja")),
            ContentWritingSystem::Japanese
        );
        assert_eq!(
            content_writing_system(Some("ja-Hang")),
            ContentWritingSystem::Korean
        );
        assert_eq!(
            content_writing_system(Some("en-Hrkt")),
            ContentWritingSystem::Japanese
        );
        assert_eq!(
            content_writing_system(Some("ko-Hani")),
            ContentWritingSystem::Chinese
        );
        assert_eq!(
            content_writing_system(Some("ja-Latn")),
            ContentWritingSystem::Other
        );
        assert_eq!(
            content_writing_system(Some("ja_Hang")),
            ContentWritingSystem::Korean
        );
        assert_eq!(
            content_writing_system(Some("en_Hrkt")),
            ContentWritingSystem::Japanese
        );
        assert_eq!(
            content_writing_system(Some("ko_Hani")),
            ContentWritingSystem::Chinese
        );
        assert_eq!(
            content_writing_system(Some("  ja-Hang  ")),
            ContentWritingSystem::Korean
        );
    }

    #[test]
    fn no_break_space_is_a_word_separator_but_not_a_hanging_other_separator() {
        assert!(!character_is_css_other_space_separator('\u{00a0}'));
        assert!(character_is_css_word_separator('\u{00a0}'));
        assert!(character_is_css_other_space_separator('\u{3000}'));
    }

    #[test]
    fn atomic_boundary_protection_keeps_narrow_no_break_space_glue() {
        assert_eq!(
            uax14_atomic_boundary_protection('\u{00a0}'),
            Uax14BoundaryProtection::None,
            "CSS Text's atomic-inline compatibility override retains NBSP"
        );
        assert_eq!(
            uax14_atomic_boundary_protection('\u{202f}'),
            Uax14BoundaryProtection::Glue
        );
        assert_eq!(
            uax14_atomic_boundary_protection('\u{2060}'),
            Uax14BoundaryProtection::WordJoiner
        );
        assert_eq!(
            uax14_atomic_boundary_protection('\u{200d}'),
            Uax14BoundaryProtection::ZeroWidthJoiner
        );
    }

    #[test]
    fn text_spacing_punctuation_classes_follow_cjk_language_conventions() {
        assert_eq!(
            text_spacing_punctuation_class('（', Some("ja"), false),
            Some(TextSpacingPunctuationClass::Opening)
        );
        assert_eq!(
            text_spacing_punctuation_class('(', Some("ja"), false),
            Some(TextSpacingPunctuationClass::NarrowOpening)
        );
        assert_eq!(
            text_spacing_punctuation_class('）', Some("ja"), false),
            Some(TextSpacingPunctuationClass::Closing)
        );
        assert_eq!(
            text_spacing_punctuation_class('　', Some("ja"), false),
            Some(TextSpacingPunctuationClass::IdeographicSpace)
        );
        assert_eq!(
            text_spacing_punctuation_class('：', Some("ja"), false),
            Some(TextSpacingPunctuationClass::MiddleDot)
        );
        assert_eq!(
            text_spacing_punctuation_class('：', Some("zh-hans"), false),
            Some(TextSpacingPunctuationClass::Closing)
        );
        assert_eq!(
            text_spacing_punctuation_class('。', Some("zh-hant"), true),
            Some(TextSpacingPunctuationClass::MiddleDot)
        );
    }

    #[test]
    fn emoji_presentation_distinguishes_emoji_and_text_default_scalars() {
        assert!(character_has_emoji_presentation('\u{1fae8}'));
        assert!(!character_has_emoji_presentation('\u{2139}'));
    }

    #[test]
    fn css_text_cursive_scripts_use_script_extensions_for_letters_only() {
        for (character, expected_script) in [
            ('ع', CursiveScript::Arabic),
            ('\u{10d00}', CursiveScript::HanifiRohingya),
            ('\u{0840}', CursiveScript::Mandaic),
            ('\u{1828}', CursiveScript::Mongolian),
            ('\u{07de}', CursiveScript::Nko),
            ('\u{a840}', CursiveScript::PhagsPa),
            ('\u{0710}', CursiveScript::Syriac),
        ] {
            assert_eq!(cursive_script(character), Some(expected_script));
        }

        for character in ['\u{1810}', '\u{1801}', '\u{a874}'] {
            assert_eq!(cursive_script(character), None);
        }
    }
}
