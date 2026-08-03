use super::super::*;

/// Parse `will-change` features that may pre-create stacking contexts.
///
/// CSS Will Change lets authors request the same stacking behavior that the
/// named property would have at a non-initial value:
/// <https://www.w3.org/TR/css-will-change-1/#will-change>.
pub(in crate::css) fn parse_will_change(value: &str) -> Option<WillChange> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(WillChange::default());
    }
    let mut will_change = WillChange::default();
    for token in value
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token.to_ascii_lowercase().as_str() {
            "contents" => will_change.contents = true,
            "scroll-position" => will_change.scroll_position = true,
            "opacity" => will_change.opacity = true,
            "transform" => will_change.transform = true,
            "filter" => will_change.filter = true,
            "clip-path" => will_change.clip_path = true,
            "mask" | "mask-image" => will_change.mask = true,
            "mix-blend-mode" => will_change.mix_blend_mode = true,
            "isolation" => will_change.isolation = true,
            "contain" => will_change.contain = true,
            _ => return None,
        }
    }
    Some(will_change)
}
