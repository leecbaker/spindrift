use super::*;

pub(in crate::css) fn parse_transform_function_calls(value: &str) -> Option<Vec<(&str, &str)>> {
    let mut calls = Vec::new();
    let mut rest = trim_css_value(value);
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = trim_css_value(&rest[..open]);
        if name.is_empty() {
            return None;
        }
        let close = find_matching_close_paren(rest, open)?;
        calls.push((name, &rest[open + 1..close]));
        rest = trim_css_value(&rest[close + 1..]);
    }
    Some(calls)
}

pub(in crate::css) fn find_matching_close_paren(value: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in value.bytes().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

pub(in crate::css) fn split_css_args(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            candidate if candidate == delimiter && depth == 0 => {
                parts.push(trim_css_value(&value[start..index]));
                start = index + candidate.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(trim_css_value(&value[start..]));
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

pub(in crate::css) fn split_css_whitespace_args(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => {
                if start.is_none() {
                    start = Some(index);
                }
                depth += 1;
            }
            ')' => depth = depth.saturating_sub(1),
            character if character.is_whitespace() && depth == 0 => {
                if let Some(part_start) = start.take() {
                    let part = trim_css_value(&value[part_start..index]);
                    if !part.is_empty() {
                        parts.push(part);
                    }
                }
            }
            _ if start.is_none() => start = Some(index),
            _ => {}
        }
    }
    if let Some(part_start) = start {
        let part = trim_css_value(&value[part_start..]);
        if !part.is_empty() {
            parts.push(part);
        }
    }
    parts
}

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
