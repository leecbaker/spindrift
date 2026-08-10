use super::*;

/// Returns the used inline offset for one formatted line.
///
/// CSS Text applies `text-indent` to the first formatted line of a block
/// container and, with `each-line`, to lines after forced line breaks while
/// excluding soft wraps. Percentages resolve against the containing block's
/// inline size; existing caller-supplied hanging indents are retained for later
/// line offsets:
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>.
pub(in crate::layout) fn used_line_indent(
    line_index: usize,
    starts_after_forced_break: bool,
    hanging_indent: f32,
    style: &ComputedStyle,
    available_width: f32,
) -> f32 {
    used_line_indent_for_formatted_line(
        line_index == 0,
        starts_after_forced_break,
        hanging_indent,
        style,
        available_width,
    )
}

/// Resolve `text-indent` from a selected line's formatted-line identity.
///
/// A float can consume physical line-box block-size before any inline content
/// is selected. CSS Text applies indentation to the first *formatted* line,
/// not to that empty float-excluded physical line:
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>.
pub(in crate::layout) fn used_line_indent_for_formatted_line(
    is_first_formatted_line: bool,
    starts_after_forced_break: bool,
    hanging_indent: f32,
    style: &ComputedStyle,
    available_width: f32,
) -> f32 {
    let is_indent_line =
        is_first_formatted_line || (style.text_indent.each_line && starts_after_forced_break);
    let applies_text_indent = is_indent_line != style.text_indent.hanging;
    let text_indent = if applies_text_indent {
        used_text_indent(style, layout_pt(available_width)).points()
    } else {
        0.0
    };
    text_indent
        + if is_first_formatted_line {
            0.0
        } else {
            hanging_indent
        }
}

/// Resolve `text-indent` against the available inline size.
///
/// The inline line-construction algorithm consumes a scalar coordinate after
/// this CSS used-value boundary.
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>
pub(in crate::layout) fn used_text_indent(
    style: &ComputedStyle,
    available_width: LayoutLength,
) -> LayoutLength {
    style
        .text_indent
        .amount
        .used_length_with_percentage_basis(PercentageBasis::definite(available_width))
        .unwrap_or_else(|| layout_pt(style.text_indent.amount.length_points()))
}
