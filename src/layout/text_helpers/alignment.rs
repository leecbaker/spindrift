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

/// Returns the alignment that applies to one inline line box with its resolved
/// base direction.
///
/// CSS Text resolves `start` and `end` against the line box's inline base
/// direction. For `unicode-bidi: plaintext`, that direction is resolved when
/// selected line records are finalized, before paint:
/// <https://www.w3.org/TR/css-text-3/#bidi-linebox>.
pub(in crate::layout) fn text_align_for_inline_line_with_base_direction(
    style: &ComputedStyle,
    is_last_line: bool,
    base_direction: Direction,
) -> TextAlign {
    if style.used_direction() == base_direction {
        return text_align_for_inline_line(style, is_last_line);
    }
    let mut effective_style = style.clone();
    effective_style.direction = base_direction;
    text_align_for_inline_line(&effective_style, is_last_line)
}
