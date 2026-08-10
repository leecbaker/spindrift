use super::*;
pub(in crate::layout) fn anonymous_inline_content_needs_normalized_style(
    style: &ComputedStyle,
) -> bool {
    (style.display.is_block_level() && style.unicode_bidi == UnicodeBidi::Isolate)
        || (!style.display.is_inline_level() && style.vertical_align != VerticalAlign::BASELINE)
}

/// Return the style used by anonymous inline text inside a block container.
///
/// CSS Inline lays out anonymous text inside a block container as inline-level
/// boxes, but properties such as table-cell `vertical-align` align the cell's
/// contents rather than shifting that anonymous text's baseline. Resetting the
/// inline-only value here prevents table-cell alignment from becoming a false
/// CSS Text shaping boundary. The block's isolate value likewise remains on
/// the block formatting context boundary instead of becoming an extra anonymous
/// inline isolation span:
/// <https://www.w3.org/TR/css-inline-3/#anonymous>,
/// <https://www.w3.org/TR/CSS22/tables.html#height-layout>, and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(in crate::layout) fn normalized_anonymous_inline_content_style(
    style: &ComputedStyle,
) -> ComputedStyle {
    let mut style = if style.display.is_block_level() && style.unicode_bidi == UnicodeBidi::Isolate
    {
        inline_content_style_without_block_isolate(style)
    } else {
        style.clone()
    };
    if !style.display.is_inline_level() {
        style.vertical_align = VerticalAlign::BASELINE;
    }
    style
}

pub(in crate::layout) fn inline_content_style_without_block_isolate(
    style: &ComputedStyle,
) -> ComputedStyle {
    let mut style = style.clone();
    style.unicode_bidi = UnicodeBidi::Normal;
    style
}
