use super::*;

pub(in crate::css) fn parse_css_number(value: &str) -> Option<f32> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("calc(infinity)") || value.eq_ignore_ascii_case("calc(+infinity)")
    {
        return Some(f32::INFINITY);
    }
    if value.eq_ignore_ascii_case("calc(-infinity)") {
        return Some(f32::NEG_INFINITY);
    }
    value.parse::<f32>().ok()
}

pub(in crate::css) fn parse_css_angle_radians(value: &str) -> Option<f32> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    if let Some(number) = lower.strip_suffix("deg") {
        return parse_css_number(number).map(f32::to_radians);
    }
    if let Some(number) = lower.strip_suffix("grad") {
        return parse_css_number(number).map(|value| value * std::f32::consts::PI / 200.0);
    }
    if let Some(number) = lower.strip_suffix("turn") {
        return parse_css_number(number).map(|value| value * std::f32::consts::TAU);
    }
    lower
        .strip_suffix("rad")
        .and_then(parse_css_number)
        .or_else(|| parse_css_number(value).filter(|value| *value == 0.0))
}
