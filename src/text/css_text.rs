//! CSS Text scalar classification and white-space processing helpers.
//!
//! These operations are shared by DOM/generated-text normalization, line
//! breaking, inline layout, and shaping. They implement CSS Text's processing
//! phases without conflating CSS document white space with Unicode whitespace.

use super::*;

/// One scalar after CSS Text's control-character classification.
///
/// CSS Text gives Unicode controls and UAX #14 mandatory-break scalars a
/// meaning before white-space processing and shaping. Keeping that
/// classification typed makes it impossible for a mandatory break to be
/// replaced with a visible control glyph before inline collection can emit
/// its forced line boundary.
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://drafts.csswg.org/css-text-3/#line-break-details>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CssTextScalar {
    DocumentSpace,
    Tab,
    SegmentBreak,
    CarriageReturn,
    MandatoryLineBreak(char),
    VisibleControl(VisibleControlCharacter),
    Text(char),
}

/// A Unicode control character CSS Text requires the UA to render visibly.
///
/// Construction is private to [`classify_css_text_scalar`], which excludes
/// CSS document white-space controls and UAX #14 mandatory-break scalars.
/// Those characters retain their separate line-layout semantics.
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://drafts.csswg.org/css-text-3/#line-break-details>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisibleControlCharacter(pub(super) char);

impl VisibleControlCharacter {
    /// The portable visible symbol Spindrift shapes for a control character.
    ///
    /// U+25A0 BLACK SQUARE is an `So` / Common character, matching CSS Text's
    /// required text-processing class without relying on a font-specific
    /// `.notdef` glyph for a C0 or C1 code point. The original scalar remains
    /// in the DOM/computed generated-content value; this is rendering text.
    pub(crate) const fn synthesized_glyph(self) -> char {
        '\u{25a0}'
    }
}

/// Classify one scalar according to CSS Text control and white-space rules.
pub(crate) fn classify_css_text_scalar(character: char) -> CssTextScalar {
    match character {
        ' ' => CssTextScalar::DocumentSpace,
        '\t' => CssTextScalar::Tab,
        '\n' => CssTextScalar::SegmentBreak,
        '\r' => CssTextScalar::CarriageReturn,
        character if character_is_mandatory_line_break(character) => {
            CssTextScalar::MandatoryLineBreak(character)
        }
        character if character_is_unicode_control(character) => {
            CssTextScalar::VisibleControl(VisibleControlCharacter(character))
        }
        character => CssTextScalar::Text(character),
    }
}

/// Materialize text for CSS whitespace processing and shaping.
///
/// Carriage return becomes an ordinary space. Non-mandatory Cc characters
/// apart from tab and line feed become visible `So` / Common symbols, while
/// `BK`/`NL` scalars remain intact until inline collection converts them into
/// forced line boundaries. This is the shared text-materialization boundary
/// for DOM and generated text.
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://drafts.csswg.org/css-text-3/#line-break-details>
pub(crate) fn css_text_rendering_text(text: &str) -> String {
    text.chars()
        .map(|character| match classify_css_text_scalar(character) {
            CssTextScalar::CarriageReturn => ' ',
            CssTextScalar::MandatoryLineBreak(character) => character,
            CssTextScalar::VisibleControl(control) => control.synthesized_glyph(),
            CssTextScalar::DocumentSpace => ' ',
            CssTextScalar::Tab => '\t',
            CssTextScalar::SegmentBreak => '\n',
            CssTextScalar::Text(character) => character,
        })
        .collect()
}

/// Return whether a character is CSS document white space that can collapse.
///
/// CSS Text white-space processing operates only on spaces, tabs, and
/// segment breaks; a carriage return is first converted to a space. Other
/// Unicode space separators, including U+3000 IDEOGRAPHIC SPACE, are handled
/// through Unicode line breaking, while U+000C FORM FEED has its separate
/// UAX #14 mandatory-break meaning:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(crate) fn is_css_collapsible_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r')
}

/// Return whether a character is a CSS document space preserved by `pre-wrap`.
///
/// CSS Text distinguishes space and tab document characters from segment
/// breaks during preserved white-space processing. Unicode space separators
/// such as U+3000 are not part of this CSS document-space set and are handled
/// through Unicode category and line-break data instead:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
pub(crate) fn is_css_preserved_document_space(character: char) -> bool {
    matches!(character, ' ' | '\t')
}

/// Return whether a text run contains only CSS-collapsible white space.
///
/// This intentionally differs from Rust/Unicode `trim` semantics: U+3000 and
/// other non-document space separators remain visible text for CSS Text line
/// layout:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(crate) fn text_is_css_collapsible_whitespace(text: &str) -> bool {
    text.chars().all(is_css_collapsible_whitespace)
}

/// Trim only CSS-collapsible document white space from both text edges.
///
/// CSS Text phase II removes line-edge collapsible spaces, not all Unicode
/// white-space code points:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(crate) fn trim_css_collapsible_whitespace(text: &str) -> &str {
    text.trim_matches(is_css_collapsible_whitespace)
}

/// Trim CSS-collapsible document white space from the start of text.
///
/// CSS Text trims document white-space characters at line edges; Unicode
/// space separators such as U+3000 remain visible content:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(crate) fn trim_start_css_collapsible_whitespace(text: &str) -> &str {
    text.trim_start_matches(is_css_collapsible_whitespace)
}

/// Trim CSS-collapsible document white space from the end of text.
///
/// CSS Text trims document white-space characters at line edges; Unicode
/// space separators such as U+3000 remain visible content:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(crate) fn trim_end_css_collapsible_whitespace(text: &str) -> &str {
    text.trim_end_matches(is_css_collapsible_whitespace)
}

/// Collapse CSS-collapsible document white space to single U+0020 spaces.
///
/// CSS Text collapses only document white-space characters. Other Unicode
/// separators such as U+3000 remain visible characters and must not be
/// collapsed by generic Unicode whitespace APIs:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
#[cfg(test)]
pub(crate) fn collapse_css_collapsible_whitespace(text: &str) -> String {
    let text = trim_css_collapsible_whitespace(text);
    let mut output = String::with_capacity(text.len());
    let mut in_whitespace = false;
    for character in text.chars() {
        if is_css_collapsible_whitespace(character) {
            if !in_whitespace {
                output.push(' ');
            }
            in_whitespace = true;
        } else {
            output.push(character);
            in_whitespace = false;
        }
    }
    output
}

/// Trim trailing CSS Text hanging space separators for line measurement.
///
/// CSS Text phase II says trailing "other space separators" hang for
/// every legacy white-space mode other than `break-spaces`: they remain in the
/// line's text and can paint, but their advance is excluded from line
/// measurement.
/// This helper returns the measured prefix without mutating the source text:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
#[cfg(test)]
pub(crate) fn trim_trailing_css_hanging_space_separators<'a>(
    text: &'a str,
    style: &ComputedStyle,
) -> &'a str {
    if !style.white_space.hangs_trailing_space_separators() {
        return text;
    }
    let end = text
        .char_indices()
        .rev()
        .find_map(|(offset, character)| {
            (!character_is_css_other_space_separator(character))
                .then_some(offset + character.len_utf8())
        })
        .unwrap_or(0);
    &text[..end]
}

/// Return the inline-end shaper tracking used by legacy text-only tests.
///
/// Production inline layout shapes without backend-owned tracking and resolves
/// line-edge suppression in `MeasuredInlineAdvance`; this helper remains only
/// for the isolated computed-spacing shaping assertions.
#[cfg(test)]
pub(crate) fn line_end_letter_spacing_width(text: &str, style: &ComputedStyle) -> LayoutLength {
    let letter_spacing = style.used_letter_spacing().points();
    if letter_spacing == 0.0
        || text.is_empty()
        || text.chars().any(|character| {
            character_has_cursive_shaping_behavior(character)
                && !character_is_join_control(character)
        })
    {
        return layout_pt(0.0);
    }
    let boundaries = GraphemeClusterSegmenter::new()
        .segment_str(text)
        .collect::<Vec<_>>();
    if boundaries.windows(2).any(|window| {
        text[window[0]..window[1]]
            .chars()
            .any(|character| !character_is_bidi_format_control(character))
    }) {
        layout_pt(letter_spacing)
    } else {
        layout_pt(0.0)
    }
}
