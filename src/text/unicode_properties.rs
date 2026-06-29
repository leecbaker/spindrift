use super::*;

static JOINING_TYPES: OnceLock<CodePointMapDataBorrowed<'static, JoiningType>> = OnceLock::new();
static JOIN_CONTROLS: OnceLock<CodePointSetDataBorrowed<'static>> = OnceLock::new();
static BIDI_CONTROLS: OnceLock<CodePointSetDataBorrowed<'static>> = OnceLock::new();
static BIDI_CLASSES: OnceLock<CodePointMapDataBorrowed<'static, BidiClass>> = OnceLock::new();
static LINE_BREAK_CLASSES: OnceLock<CodePointMapDataBorrowed<'static, LineBreak>> = OnceLock::new();
static VERTICAL_ORIENTATIONS: OnceLock<CodePointMapDataBorrowed<'static, VerticalOrientation>> =
    OnceLock::new();
static WORD_BREAK_CLASSES: OnceLock<CodePointMapDataBorrowed<'static, IcuWordBreak>> =
    OnceLock::new();
static GENERAL_CATEGORIES: OnceLock<CodePointMapDataBorrowed<'static, GeneralCategory>> =
    OnceLock::new();
static DEFAULT_IGNORABLE_CODE_POINTS: OnceLock<CodePointSetDataBorrowed<'static>> = OnceLock::new();

/// Return whether a character participates in cursive joining.
///
/// CSS Text requires letter spacing to preserve cursive joining behavior, and
/// Unicode defines the joining classes used by Arabic-family shaping engines:
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property> and
/// <https://www.unicode.org/reports/tr44/#Joining_Type>.
pub(super) fn character_has_joining_behavior(character: char) -> bool {
    matches!(
        joining_type(character),
        JoiningType::JoinCausing
            | JoiningType::DualJoining
            | JoiningType::LeftJoining
            | JoiningType::RightJoining
    )
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

/// Return whether one typographic unit is upright under `text-orientation: mixed`.
///
/// Unicode `Vertical_Orientation=U` and `Tu` are treated as upright for
/// placement. `R` and `Tr` are emitted sideways in this pass; OpenType vertical
/// alternates and transformed glyph substitution are tracked separately.
/// Combining marks and default-ignorable controls inherit the first visible
/// base character in the unit instead of deciding orientation on their own.
pub(crate) fn typographic_unit_is_upright_in_mixed_orientation(text: &str) -> bool {
    text.chars()
        .find(|character| !character_inherits_vertical_orientation(*character))
        .is_some_and(|character| {
            matches!(
                character_vertical_orientation(character),
                VerticalOrientation::Upright | VerticalOrientation::TransformedUpright
            )
        })
}

pub(crate) fn character_inherits_vertical_orientation(character: char) -> bool {
    character_is_default_ignorable_code_point(character)
        || GeneralCategoryGroup::Mark.contains(general_category(character))
}

/// Return whether a character is a CSS Text "other space separator".
///
/// CSS Text distinguishes document white space such as U+0020 SPACE and tabs
/// from other Unicode separator characters. The latter remain visible text,
/// but CSS Text phase II lets them hang at the end of lines for collapsing
/// white-space modes:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(crate) fn character_is_css_other_space_separator(character: char) -> bool {
    !is_css_collapsible_whitespace(character)
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
    is_css_preserved_document_space(character)
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
/// CSS Text Level 4 automatic spacing is defined around Han ideographs. UAX
/// #14 exposes this through the `Line_Break=Ideographic` class, which covers
/// BMP and supplementary CJK ideographs without hardcoded Unicode ranges:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property> and
/// <https://www.unicode.org/reports/tr14/>.
pub(crate) fn character_is_autospace_ideograph(character: char) -> bool {
    line_break_class(character) == LineBreak::Ideographic
        && GeneralCategoryGroup::Letter.contains(general_category(character))
}

/// Return whether a character is a non-ideographic letter for autospace.
///
/// The `ideograph-alpha` value inserts spacing between Han ideographs and
/// adjacent letters. Unicode general categories provide the letter side of
/// that boundary, while ideographs are excluded by line-break class:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_autospace_alpha(character: char) -> bool {
    !character_is_autospace_ideograph(character)
        && GeneralCategoryGroup::Letter.contains(general_category(character))
}

/// Return whether a character is numeric for CSS `text-autospace`.
///
/// The `ideograph-numeric` value inserts spacing between Han ideographs and
/// adjacent Unicode numbers, not just ASCII digits:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(crate) fn character_is_autospace_numeric(character: char) -> bool {
    GeneralCategoryGroup::Number.contains(general_category(character))
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
    fn mixed_orientation_policy_uses_visible_base_character() {
        assert!(!typographic_unit_is_upright_in_mixed_orientation("a"));
        assert!(typographic_unit_is_upright_in_mixed_orientation("§"));
        assert!(typographic_unit_is_upright_in_mixed_orientation("、"));
        assert!(!typographic_unit_is_upright_in_mixed_orientation(
            "\u{0301}"
        ));
        assert!(!typographic_unit_is_upright_in_mixed_orientation(
            "\u{0301}a"
        ));
        assert!(typographic_unit_is_upright_in_mixed_orientation(
            "\u{200d}中"
        ));
    }
}
