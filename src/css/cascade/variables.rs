use super::declarations::CascadedDeclaration;
use super::*;
use crate::css::{custom_property_value_is_valid, is_custom_property_name};

pub(super) fn apply_cascaded_custom_property_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
) {
    for declaration in declarations {
        let name = declaration.name.as_ref();
        if is_custom_property_name(name) {
            let value = trim_css_value(&declaration.value);
            match value.to_ascii_lowercase().as_str() {
                // CSS-wide keywords have their ordinary cascade meaning for
                // custom properties. `initial` produces the guaranteed-invalid
                // value, represented by removing the inherited entry; custom
                // properties inherit by default, so `inherit` and `unset`
                // retain the already-inherited map entry.
                // <https://www.w3.org/TR/css-variables-1/#defining-variables>
                "initial" => {
                    style.custom_properties.remove(name);
                }
                "inherit" | "unset" => {}
                _ => {
                    style
                        .custom_properties
                        .insert(name.to_string(), value.to_string());
                }
            }
        }
    }
    resolve_custom_properties_at_computed_value_time(style);
}

/// Resolves custom properties before they are inherited by descendants.
///
/// CSS Variables computes each custom property on its owning element. Values
/// that are guaranteed-invalid (including every member of a reference cycle)
/// do not become raw expressions that a child can later resolve against its
/// own custom-property environment.
/// <https://www.w3.org/TR/css-variables-1/#cycles>
/// <https://www.w3.org/TR/css-variables-1/#computed-value>
fn resolve_custom_properties_at_computed_value_time(style: &mut ComputedStyle) {
    let mut custom_properties = std::mem::take(&mut style.custom_properties);
    let cycles = cyclic_custom_properties(&custom_properties);
    for name in cycles {
        custom_properties.remove(&name);
    }

    let resolved = custom_properties
        .iter()
        .filter_map(|(name, value)| {
            resolve_css_variables(value, &custom_properties).map(|value| (name.clone(), value))
        })
        .collect();
    style.custom_properties = resolved;
}

fn cyclic_custom_properties(
    custom_properties: &std::collections::HashMap<String, String>,
) -> std::collections::HashSet<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Visit {
        Visiting,
        Visited,
    }

    fn visit(
        name: &str,
        custom_properties: &std::collections::HashMap<String, String>,
        states: &mut std::collections::HashMap<String, Visit>,
        stack: &mut Vec<String>,
        cycles: &mut std::collections::HashSet<String>,
    ) {
        match states.get(name) {
            Some(Visit::Visited) => return,
            Some(Visit::Visiting) => {
                if let Some(start) = stack.iter().position(|candidate| candidate == name) {
                    cycles.extend(stack[start..].iter().cloned());
                }
                return;
            }
            None => {}
        }
        let Some(value) = custom_properties.get(name) else {
            return;
        };
        states.insert(name.to_string(), Visit::Visiting);
        stack.push(name.to_string());
        for dependency in custom_property_dependencies(value) {
            if custom_properties.contains_key(&dependency) {
                visit(&dependency, custom_properties, states, stack, cycles);
            }
        }
        stack.pop();
        states.insert(name.to_string(), Visit::Visited);
    }

    let mut states = std::collections::HashMap::new();
    let mut stack = Vec::new();
    let mut cycles = std::collections::HashSet::new();
    for name in custom_properties.keys() {
        visit(
            name,
            custom_properties,
            &mut states,
            &mut stack,
            &mut cycles,
        );
    }
    cycles
}

fn custom_property_dependencies(value: &str) -> Vec<String> {
    let mut dependencies = Vec::new();
    let mut rest = value;
    while let Some(start) = find_css_variable_function(rest) {
        let after_var = &rest[start + "var(".len()..];
        let Some(end) = find_matching_paren(after_var) else {
            break;
        };
        let (name, fallback) = split_var_body(&after_var[..end]);
        let name = name.trim();
        if is_custom_property_name(name) {
            dependencies.push(name.to_string());
        }
        // A dependency in a fallback can participate in a cycle when that
        // fallback is selected, so retain it in the graph as well.
        if let Some(fallback) = fallback {
            dependencies.extend(custom_property_dependencies(fallback));
        }
        rest = &after_var[end + 1..];
    }
    dependencies
}

pub(super) fn apply_cascaded_font_size_declarations_with_parent_ch_advance(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    defaulted_style: &ComputedStyle,
    inherited_ch_advance: LayoutLength,
) {
    let inherited_font_size = style.font_size;
    for declaration in declarations
        .iter()
        .rev()
        .filter(|declaration| matches!(declaration.name.as_ref(), "font-size" | "font"))
    {
        let resolved_value;
        let value = trim_css_value(&declaration.value);
        let contains_var = contains_css_variable_reference(value);
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
                style.font_size = defaulted_style.font_size;
                style.deferred_font_size = DeferredFontSize::Inherit;
                project_line_height(style);
                return;
            }
            _ => {}
        }
        if declaration.name.as_ref() == "font-size"
            && let Some(font_size) = parse_deferred_font_size(value)
        {
            set_deferred_font_size(style, font_size, inherited_font_size, inherited_ch_advance);
            return;
        }
        if declaration.name.as_ref() == "font"
            && let Some(font) = parse_font_shorthand_with_parent_ch_advance(
                value,
                inherited_font_size,
                inherited_ch_advance,
                style.font_weight,
            )
        {
            set_deferred_font_size(
                style,
                font.deferred_size,
                inherited_font_size,
                inherited_ch_advance,
            );
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
        let contains_var = contains_css_variable_reference(value);
        let value = if contains_var {
            let Some(resolved) = resolve_css_variables(value, &style.custom_properties) else {
                // An unresolved or syntactically invalid substitution makes
                // the winning declaration invalid at computed-value time.
                // `color` is inherited, so its computed value falls back to
                // the inherited value rather than an earlier declaration.
                // <https://www.w3.org/TR/css-variables-1/#invalid-variables>
                style.color = inheritance_source.color;
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
        if let Some(color) = parse_color_from_currentcolor(value, inheritance_source.color)
            .or_else(|| parse_color_mix(value, inheritance_source.color))
            .or_else(|| parse_color(value))
        {
            style.color = color;
            return;
        }
        if contains_var {
            // Substitution succeeded but did not produce a valid color. This
            // has the same invalid-at-computed-value-time behavior.
            style.color = inheritance_source.color;
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
    if !custom_property_value_is_valid(value) {
        return None;
    }
    resolve_css_variables_inner(value, custom_properties, &mut Vec::new())
}

/// Returns whether a token stream contains a `var()` function. CSS function
/// names are ASCII case-insensitive, unlike custom-property names.
pub(super) fn contains_css_variable_reference(value: &str) -> bool {
    find_css_variable_function(value).is_some()
}

fn find_css_variable_function(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    for index in 0..bytes.len().saturating_sub(3) {
        if !bytes[index..].starts_with(b"var(")
            && !bytes[index..]
                .get(..4)
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(b"var("))
        {
            continue;
        }
        // Do not match the trailing `var(` in another identifier such as
        // `myvar(`. CSS tokenization makes only a standalone identifier
        // followed directly by `(` a Function token.
        let preceded_by_identifier_code_point = index
            .checked_sub(1)
            .and_then(|previous| bytes.get(previous))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        if !preceded_by_identifier_code_point {
            return Some(index);
        }
    }
    None
}

fn resolve_css_variables_inner(
    value: &str,
    custom_properties: &std::collections::HashMap<String, String>,
    stack: &mut Vec<String>,
) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = find_css_variable_function(rest) {
        append_css_component_fragment(&mut output, &rest[..start]);
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
            let replacement = resolve_css_variables_inner(replacement, custom_properties, stack);
            stack.pop();
            if let Some(replacement) = replacement {
                append_css_component_fragment(&mut output, &replacement);
            } else if let Some(fallback) = fallback {
                let fallback =
                    resolve_css_variables_inner(fallback.trim(), custom_properties, stack)?;
                append_css_component_fragment(&mut output, &fallback);
            } else {
                return None;
            }
        } else if let Some(fallback) = fallback {
            let fallback = resolve_css_variables_inner(fallback.trim(), custom_properties, stack)?;
            append_css_component_fragment(&mut output, &fallback);
        } else {
            return None;
        }
        rest = &after_var[end + 1..];
    }
    append_css_component_fragment(&mut output, rest);
    Some(output)
}

/// Appends serialized component values without accidentally retokenizing two
/// adjacent identifiers as one identifier. `var()` substitutes a token stream,
/// so `var(--a)var(--b)` containing `orange` and `red` must not become the
/// single `orangered` token.
fn append_css_component_fragment(output: &mut String, fragment: &str) {
    let Some(first) = fragment.chars().next() else {
        return;
    };
    if output
        .chars()
        .next_back()
        .is_some_and(is_identifier_code_point)
        && is_identifier_code_point(first)
    {
        output.push(' ');
    }
    output.push_str(fragment);
}

fn is_identifier_code_point(character: char) -> bool {
    character == '-'
        || character == '_'
        || character.is_ascii_alphanumeric()
        || !character.is_ascii()
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
