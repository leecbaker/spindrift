use super::*;

pub(crate) fn collapse_whitespace(text: &str) -> String {
    let mut output = String::new();
    let mut last_was_space = true;
    for character in text.chars() {
        if is_css_collapsible_whitespace(character) {
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

/// Returns whether a character is document white space that CSS can collapse.
///
/// CSS Text defines collapsible white space as spaces, tabs, segment breaks,
/// and form feeds; NBSP is not collapsible and must still generate content:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(crate) fn is_css_collapsible_whitespace(character: char) -> bool {
    crate::text::is_css_collapsible_whitespace(character)
}

/// Returns whether a text node is entirely collapsible in its current style.
///
/// CSS Text's white-space processing only removes document white-space
/// characters for modes that collapse spaces:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(crate) fn text_is_css_collapsible_space(text: &str, style: &ComputedStyle) -> bool {
    style.white_space.collapses_spaces() && crate::text::text_is_css_collapsible_whitespace(text)
}
