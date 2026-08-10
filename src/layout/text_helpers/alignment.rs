use super::*;

/// Returns the logical alignment that applies to one inline line box.
///
/// CSS Text applies `text-align-last` only to the last line of a block or to a
/// line before a forced break. `auto` keeps ordinary `text-align` behavior,
/// except that a justified affected line falls back to logical start:
/// <https://www.w3.org/TR/css-text-3/#text-align-last-property>.
pub(in crate::layout) fn text_align_for_inline_line(
    style: &ComputedStyle,
    is_last_line: bool,
) -> TextAlign {
    if is_last_line {
        logical_text_align_last(style)
    } else {
        style.text_align
    }
}

pub(in crate::layout) fn logical_text_align_last(style: &ComputedStyle) -> TextAlign {
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
pub(in crate::layout) fn text_align_for_inline_line_text_with_state(
    style: &ComputedStyle,
    is_last_line: bool,
    line_text: &str,
    plaintext_direction_state: &mut Option<Direction>,
) -> TextAlign {
    let mut effective_style;
    let style = if style.unicode_bidi == UnicodeBidi::Plaintext {
        // A soft-wrapped line remains in the same plaintext paragraph as its
        // preceding line. Resolve P2/P3 only for that paragraph's first
        // selected line; later soft lines retain the established base level.
        let direction = if let Some(direction) = *plaintext_direction_state {
            direction
        } else if let Some(direction) = plaintext_direction_for_text(line_text) {
            // Only P2's first-strong result establishes this plaintext
            // paragraph's direction. A leading whitespace-only selected line
            // uses P3's fallback transiently, so later strong text can still
            // establish the paragraph level.
            *plaintext_direction_state = Some(direction);
            direction
        } else {
            style.used_direction()
        };
        effective_style = style.clone();
        effective_style.direction = direction;
        &effective_style
    } else {
        style
    };
    text_align_for_inline_line(style, is_last_line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_direction_is_established_by_first_strong_and_kept_for_soft_lines() {
        let mut style = ComputedStyle::initial();
        style.unicode_bidi = UnicodeBidi::Plaintext;
        let mut direction = None;

        text_align_for_inline_line_text_with_state(&style, false, "אבג", &mut direction);
        assert_eq!(direction, Some(Direction::Rtl));

        // A later soft-wrapped line has no first-strong character of its own,
        // but remains in the paragraph established by the earlier line.
        text_align_for_inline_line_text_with_state(&style, false, " 123 ", &mut direction);
        assert_eq!(direction, Some(Direction::Rtl));

        // A whitespace-only line before the first strong character uses P3's
        // fallback only transiently and cannot lock the paragraph direction.
        direction = None;
        text_align_for_inline_line_text_with_state(&style, false, "   ", &mut direction);
        assert_eq!(direction, None);
        text_align_for_inline_line_text_with_state(&style, false, "אבג", &mut direction);
        assert_eq!(direction, Some(Direction::Rtl));
    }
}
