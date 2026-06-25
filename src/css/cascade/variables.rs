use super::declarations::CascadedDeclaration;
use super::*;

pub(super) fn apply_cascaded_custom_property_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
) {
    for declaration in declarations {
        let name = declaration.name.as_ref();
        if name.starts_with("--") {
            style.custom_properties.insert(
                name.to_string(),
                trim_css_value(&declaration.value).to_string(),
            );
        }
    }
}

pub(super) fn apply_cascaded_font_size_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    defaulted_style: &ComputedStyle,
) {
    let inherited_font_size = style.font_size;
    for declaration in declarations
        .iter()
        .rev()
        .filter(|declaration| matches!(declaration.name.as_ref(), "font-size" | "font"))
    {
        let resolved_value;
        let value = trim_css_value(&declaration.value);
        let contains_var = value.contains("var(");
        let value = if contains_var {
            let Some(resolved) = resolve_css_variables(value, &style.custom_properties) else {
                return;
            };
            resolved_value = resolved;
            trim_css_value(&resolved_value)
        } else {
            value
        };
        match value.to_ascii_lowercase().as_str() {
            "initial" => {
                set_font_size(style, ComputedStyle::initial().font_size);
                return;
            }
            "inherit" | "unset" => {
                set_font_size(style, defaulted_style.font_size);
                return;
            }
            _ => {}
        }
        let font_size = if declaration.name.as_ref() == "font" {
            parse_font_shorthand(value, inherited_font_size, style.font_weight)
                .map(|font| font.size)
        } else {
            parse_font_size(value, inherited_font_size)
        };
        if let Some(font_size) = font_size {
            set_font_size(style, font_size);
            return;
        }
        if contains_var {
            return;
        }
    }
}

/// Applies the winning `color` before dependent `currentColor` values.
///
/// CSS Color defines `currentColor` as the computed value of the `color`
/// property, so border-color and related properties must see the final
/// cascaded color regardless of declaration order:
/// <https://www.w3.org/TR/css-color-4/#currentcolor-color>.
pub(super) fn apply_cascaded_color_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
) {
    for declaration in declarations
        .iter()
        .rev()
        .filter(|declaration| declaration.name.as_ref() == "color")
    {
        let resolved_value;
        let value = trim_css_value(&declaration.value);
        let contains_var = value.contains("var(");
        let value = if contains_var {
            let Some(resolved) = resolve_css_variables(value, &style.custom_properties) else {
                return;
            };
            resolved_value = resolved;
            trim_css_value(&resolved_value)
        } else {
            value
        };
        match value.to_ascii_lowercase().as_str() {
            "initial" => {
                style.color = ComputedStyle::initial().color;
                return;
            }
            "inherit" | "unset" => {
                style.color = inheritance_source.color;
                return;
            }
            _ => {}
        }
        if let Some(color) = parse_color(value) {
            style.color = color;
            return;
        }
        if contains_var {
            return;
        }
    }
}

/// Resolves CSS custom property substitutions in a declaration value.
///
/// CSS Cascade Level 5 defines custom property substitution and invalid
/// at computed-value time behavior for unresolved `var()` references:
/// <https://www.w3.org/TR/css-cascade-5/#invalid-at-computed-value-time>.
pub(super) fn resolve_css_variables(
    value: &str,
    custom_properties: &std::collections::HashMap<String, String>,
) -> Option<String> {
    resolve_css_variables_inner(value, custom_properties, &mut Vec::new())
}

fn resolve_css_variables_inner(
    value: &str,
    custom_properties: &std::collections::HashMap<String, String>,
    stack: &mut Vec<String>,
) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("var(") {
        output.push_str(&rest[..start]);
        let after_var = &rest[start + "var(".len()..];
        let end = find_matching_paren(after_var)?;
        let body = &after_var[..end];
        let (name, fallback) = split_var_body(body);
        let name = name.trim();
        if !name.starts_with("--") {
            return None;
        }
        if stack.iter().any(|item| item == name) {
            return None;
        }
        if let Some(replacement) = custom_properties.get(name) {
            stack.push(name.to_string());
            let replacement = resolve_css_variables_inner(replacement, custom_properties, stack)?;
            stack.pop();
            output.push_str(&replacement);
        } else if let Some(fallback) = fallback {
            let fallback = resolve_css_variables_inner(fallback.trim(), custom_properties, stack)?;
            output.push_str(&fallback);
        } else {
            return None;
        }
        rest = &after_var[end + 1..];
    }
    output.push_str(rest);
    Some(output)
}

pub(super) fn find_matching_paren(value: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    for (index, byte) in value.bytes().enumerate() {
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => depth += 1,
            b')' if depth == 0 => return Some(index),
            b')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

pub(super) fn split_var_body(body: &str) -> (&str, Option<&str>) {
    let mut depth = 0usize;
    let mut quote = None;
    for (index, byte) in body.bytes().enumerate() {
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => return (&body[..index], Some(&body[index + 1..])),
            _ => {}
        }
    }
    (body, None)
}
