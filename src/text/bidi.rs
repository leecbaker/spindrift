use super::*;

/// Return the Unicode controls that model an inline CSS `unicode-bidi` scope.
///
/// CSS Writing Modes defines `unicode-bidi` in terms of extra Unicode
/// Bidirectional Algorithm embeddings, overrides, isolates, and plaintext
/// paragraph direction. Inline scopes map directly to UAX #9 formatting
/// controls for text shaping and visual ordering; block-level `isolate` is a
/// formatting-context boundary in this layout engine and does not inject text
/// controls into the block's own anonymous inline content:
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>,
/// <https://www.w3.org/TR/css-display-3/#formatting-context> and
/// <https://www.unicode.org/reports/tr9/#Directional_Formatting_Characters>.
pub(crate) fn bidi_control_scope_for_style(
    style: &ComputedStyle,
) -> Option<(&'static str, &'static str)> {
    match (style.unicode_bidi, style.direction) {
        (UnicodeBidi::Normal, _) => None,
        (UnicodeBidi::Isolate, _) if style.display.is_block_level() => None,
        (UnicodeBidi::Embed, Direction::Ltr) => Some(("\u{202a}", "\u{202c}")),
        (UnicodeBidi::Embed, Direction::Rtl) => Some(("\u{202b}", "\u{202c}")),
        (UnicodeBidi::Isolate, Direction::Ltr) => Some(("\u{2066}", "\u{2069}")),
        (UnicodeBidi::Isolate, Direction::Rtl) => Some(("\u{2067}", "\u{2069}")),
        (UnicodeBidi::BidiOverride, Direction::Ltr) => Some(("\u{202d}", "\u{202c}")),
        (UnicodeBidi::BidiOverride, Direction::Rtl) => Some(("\u{202e}", "\u{202c}")),
        (UnicodeBidi::IsolateOverride, Direction::Ltr) => {
            Some(("\u{2066}\u{202d}", "\u{202c}\u{2069}"))
        }
        (UnicodeBidi::IsolateOverride, Direction::Rtl) => {
            Some(("\u{2067}\u{202e}", "\u{202c}\u{2069}"))
        }
        (UnicodeBidi::Plaintext, _) => Some(("\u{2068}", "\u{2069}")),
    }
}

/// Remove Unicode bidi formatting controls from text intended for painting.
///
/// CSS `unicode-bidi` controls influence ordering during the Unicode
/// Bidirectional Algorithm but do not create visible glyphs:
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(crate) fn text_without_bidi_format_controls(text: &str) -> Cow<'_, str> {
    if !text.chars().any(character_is_bidi_format_control) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter(|character| !character_is_bidi_format_control(*character))
            .collect(),
    )
}

/// Text prepared for a shaper that does not expose CSS paragraph direction.
///
/// CSS `direction` sets the base direction for bidi paragraph resolution,
/// while `unicode-bidi` can add embeddings, overrides, isolates, or plaintext
/// behavior. Parley 0.10 resolves paragraph base direction from the text, so
/// Reasyprint prefixes a directional mark before the CSS `unicode-bidi`
/// controls and records where the caller's original payload lives:
/// <https://www.w3.org/TR/css-writing-modes-4/#direction>,
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>, and
/// <https://www.unicode.org/reports/tr9/#P2>.
pub(crate) struct CssBidiText<'a> {
    text: Cow<'a, str>,
    payload: Range<usize>,
}

impl<'a> CssBidiText<'a> {
    pub(crate) fn as_str(&self) -> &str {
        self.text.as_ref()
    }

    pub(crate) fn original_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        let start = range.start.max(self.payload.start);
        let end = range.end.min(self.payload.end);
        (start < end).then(|| start - self.payload.start..end - self.payload.start)
    }
}

/// Return text wrapped in controls needed to apply CSS bidi state in Parley.
///
/// `unicode-bidi: plaintext` intentionally keeps paragraph direction
/// first-strong, so it uses the existing FSI/PDI scope without an explicit
/// LRM/RLM base-direction mark.
pub(crate) fn text_with_css_bidi_controls<'a>(
    text: &'a str,
    style: &ComputedStyle,
) -> CssBidiText<'a> {
    let base_direction = match (style.unicode_bidi, style.direction) {
        (UnicodeBidi::Plaintext, _) => "",
        (_, Direction::Ltr) => "\u{200e}",
        (_, Direction::Rtl) => "\u{200f}",
    };
    let (scope_start, scope_end) = bidi_control_scope_for_style(style).unwrap_or(("", ""));
    if base_direction.is_empty() && scope_start.is_empty() && scope_end.is_empty() {
        return CssBidiText {
            text: Cow::Borrowed(text),
            payload: 0..text.len(),
        };
    }

    let payload_start = base_direction.len() + scope_start.len();
    let payload_end = payload_start + text.len();
    let mut output = String::with_capacity(
        base_direction.len() + scope_start.len() + text.len() + scope_end.len(),
    );
    output.push_str(base_direction);
    output.push_str(scope_start);
    output.push_str(text);
    output.push_str(scope_end);
    CssBidiText {
        text: Cow::Owned(output),
        payload: payload_start..payload_end,
    }
}
