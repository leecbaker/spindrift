use std::collections::HashMap;

/// Return the normalized HTML `input` type.
///
/// HTML treats a missing or unsupported `type` as the Text state for form
/// control behavior:
/// <https://html.spec.whatwg.org/multipage/input.html#attr-input-type>.
pub(crate) fn input_type(tag: &str, attrs: &HashMap<String, String>) -> Option<String> {
    (tag == "input").then(|| {
        attrs
            .get("type")
            .map(|value| value.to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "text".to_string())
    })
}

/// HTML form-associated elements that can be disabled by `disabled` or a
/// disabled fieldset.
///
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#concept-fe-disabled>
pub(crate) fn disableable_element(tag: &str) -> bool {
    matches!(
        tag,
        "button" | "fieldset" | "input" | "optgroup" | "option" | "select" | "textarea"
    )
}

pub(crate) fn required_capable(tag: &str, attrs: &HashMap<String, String>) -> bool {
    match tag {
        "select" | "textarea" => true,
        "input" => !matches!(
            input_type(tag, attrs).as_deref(),
            Some("hidden" | "range" | "color" | "submit" | "image" | "reset" | "button")
        ),
        _ => false,
    }
}

pub(crate) fn read_write(tag: &str, attrs: &HashMap<String, String>, disabled: bool) -> bool {
    match tag {
        "textarea" => !disabled && !attrs.contains_key("readonly"),
        "input" => {
            !disabled
                && !attrs.contains_key("readonly")
                && !matches!(
                    input_type(tag, attrs).as_deref(),
                    Some(
                        "hidden"
                            | "range"
                            | "color"
                            | "checkbox"
                            | "radio"
                            | "file"
                            | "submit"
                            | "image"
                            | "reset"
                            | "button"
                    )
                )
        }
        _ if attrs.contains_key("contenteditable") => attrs
            .get("contenteditable")
            .is_none_or(|value| !value.eq_ignore_ascii_case("false")),
        _ => false,
    }
}

pub(crate) fn placeholder_shown(tag: &str, attrs: &HashMap<String, String>) -> bool {
    matches!(tag, "input" | "textarea")
        && attrs.contains_key("placeholder")
        && control_value(attrs).is_empty()
}

pub(crate) fn validation_candidate(
    tag: &str,
    attrs: &HashMap<String, String>,
    disabled: bool,
) -> bool {
    !disabled
        && !attrs.contains_key("readonly")
        && match tag {
            "textarea" | "select" => true,
            "input" => !matches!(
                input_type(tag, attrs).as_deref(),
                Some("hidden" | "button" | "submit" | "reset" | "image" | "color" | "file")
            ),
            _ => false,
        }
}

/// Evaluate deterministic HTML constraint-validation failures available from
/// static element attributes.
///
/// This intentionally excludes user/session-dependent states and browser IDL
/// state, but covers value-missing, basic type mismatch, length, numeric
/// range, and numeric step mismatch:
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#constraints>.
pub(crate) fn statically_invalid(
    tag: &str,
    attrs: &HashMap<String, String>,
    disabled: bool,
) -> bool {
    if !validation_candidate(tag, attrs, disabled) {
        return false;
    }
    let value = control_value(attrs);
    if attrs.contains_key("required") && value.is_empty() {
        return true;
    }
    if value.is_empty() {
        return false;
    }
    length_invalid(attrs, value)
        || type_mismatch(tag, attrs, value)
        || numeric_range_invalid(tag, attrs, value)
        || numeric_step_mismatch(tag, attrs, value)
}

pub(crate) fn numeric_in_range(tag: &str, attrs: &HashMap<String, String>) -> bool {
    numeric_value(tag, attrs).is_some_and(|value| numeric_value_is_in_range(attrs, value))
}

pub(crate) fn numeric_out_of_range(tag: &str, attrs: &HashMap<String, String>) -> bool {
    numeric_value(tag, attrs).is_some_and(|value| !numeric_value_is_in_range(attrs, value))
}

fn control_value(attrs: &HashMap<String, String>) -> &str {
    attrs.get("value").map(String::as_str).unwrap_or("")
}

fn length_invalid(attrs: &HashMap<String, String>, value: &str) -> bool {
    let length = value.chars().count();
    attrs
        .get("minlength")
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|min| length < min)
        || attrs
            .get("maxlength")
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|max| length > max)
}

fn type_mismatch(tag: &str, attrs: &HashMap<String, String>, value: &str) -> bool {
    match input_type(tag, attrs).as_deref() {
        Some("email") => {
            if attrs.contains_key("multiple") {
                value
                    .split(',')
                    .map(str::trim)
                    .any(|part| !looks_like_email(part))
            } else {
                !looks_like_email(value.trim())
            }
        }
        Some("url") => !(value.starts_with("http://") || value.starts_with("https://")),
        _ => false,
    }
}

fn looks_like_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains(char::is_whitespace)
}

fn numeric_range_invalid(tag: &str, attrs: &HashMap<String, String>, value: &str) -> bool {
    matches!(input_type(tag, attrs).as_deref(), Some("number" | "range"))
        && value
            .parse::<f32>()
            .ok()
            .is_none_or(|value| !numeric_value_is_in_range(attrs, value))
}

fn numeric_value(tag: &str, attrs: &HashMap<String, String>) -> Option<f32> {
    if !matches!(input_type(tag, attrs).as_deref(), Some("number" | "range")) {
        return None;
    }
    control_value(attrs).parse::<f32>().ok()
}

fn numeric_value_is_in_range(attrs: &HashMap<String, String>, value: f32) -> bool {
    let min_ok = attrs
        .get("min")
        .and_then(|min| min.parse::<f32>().ok())
        .is_none_or(|min| value >= min);
    let max_ok = attrs
        .get("max")
        .and_then(|max| max.parse::<f32>().ok())
        .is_none_or(|max| value <= max);
    min_ok && max_ok
}

fn numeric_step_mismatch(tag: &str, attrs: &HashMap<String, String>, value: &str) -> bool {
    if !matches!(input_type(tag, attrs).as_deref(), Some("number" | "range")) {
        return false;
    }
    let Some(step) = attrs.get("step") else {
        return false;
    };
    if step.eq_ignore_ascii_case("any") {
        return false;
    }
    let Some(step) = step.parse::<f32>().ok().filter(|step| *step > 0.0) else {
        return false;
    };
    let Some(value) = value.parse::<f32>().ok() else {
        return true;
    };
    let base = attrs
        .get("min")
        .and_then(|min| min.parse::<f32>().ok())
        .unwrap_or(0.0);
    let steps = (value - base) / step;
    (steps.round() - steps).abs() > 0.0001
}
