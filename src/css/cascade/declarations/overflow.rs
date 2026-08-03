use super::*;

/// Parses a single CSS Overflow keyword.
///
/// CSS Overflow defines the `overflow`, `overflow-x`, and `overflow-y`
/// properties as keyword values controlling visible, clipped, and scrollable
/// overflow. The legacy `overlay` keyword is an alias of `auto`:
/// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>.
pub(in crate::css) fn parse_overflow_value(value: &str) -> Option<Overflow> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "visible" => Some(Overflow::Visible),
        "hidden" => Some(Overflow::Hidden),
        "clip" => Some(Overflow::Clip),
        "scroll" => Some(Overflow::Scroll),
        "auto" | "overlay" => Some(Overflow::Auto),
        _ => None,
    }
}
