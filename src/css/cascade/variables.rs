use super::declarations::{CascadedDeclaration, affected_longhands, expand_modeled_shorthands};
use super::*;
use crate::css::{custom_property_value_is_valid, is_custom_property_name};
use cssparser::{Parser, ParserInput};
use std::borrow::Cow;

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
    // A fallback can participate in a cycle if it is selected, so the token
    // walker intentionally returns references from both arguments.
    css_variable_references(value).unwrap_or_default()
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
/// CSS CssColor defines `currentColor` as the computed value of the `color`
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

/// Substitutes custom properties before parsing a shorthand's value.
///
/// A shorthand containing `var()` cannot be expanded during the ordinary
/// cascade prepass, because its grammar is unknown until computed-value time.
/// Once its token stream has been substituted it must follow the same
/// expansion path as an authored shorthand without variables:
/// <https://www.w3.org/TR/css-variables-1/#variables-in-shorthands>.
pub(super) fn declarations_after_variable_substitution_and_shorthand_expansion<'a>(
    declarations: &[CascadedDeclaration<'a>],
    custom_properties: &std::collections::HashMap<String, String>,
    direction: Direction,
    writing_mode: WritingMode,
) -> Vec<CascadedDeclaration<'a>> {
    let mut output = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        if is_custom_property_name(&declaration.name)
            || !contains_css_variable_reference(&declaration.value)
        {
            output.push(declaration.clone());
            continue;
        }
        let Some(value) = resolve_css_variables(&declaration.value, custom_properties) else {
            // A pending shorthand still wins the cascade for every longhand
            // it addresses. Keep one unresolved declaration per affected
            // longhand so the normal invalid-at-computed-value-time handling
            // suppresses earlier winners instead of reviving them.
            let targets =
                affected_longhands(&declaration.name, direction, writing_mode).filter(|targets| {
                    targets.len() != 1
                        || targets
                            .first()
                            .is_none_or(|target| *target != declaration.name)
                });
            if let Some(targets) = targets {
                output.extend(targets.into_iter().map(|target| {
                    let mut pending = declaration.clone();
                    pending.name = Cow::Owned(target.to_string());
                    pending
                }));
            } else {
                output.push(declaration.clone());
            }
            continue;
        };
        let mut resolved = declaration.clone();
        resolved.value = Cow::Owned(value);
        for expanded in
            expand_modeled_shorthands(std::slice::from_ref(&resolved), direction, writing_mode)
        {
            // `expand_modeled_shorthands` is deliberately borrowing-friendly
            // for the normal cascade path. This post-substitution path owns
            // its value so the expanded declarations can outlive `resolved`.
            output.push(CascadedDeclaration {
                name: Cow::Owned(expanded.name.into_owned()),
                value: Cow::Owned(expanded.value.into_owned()),
                origin: declaration.origin,
                base_url: declaration.base_url,
                root_url: declaration.root_url,
                important: declaration.important,
                layer_order: declaration.layer_order,
                specificity: declaration.specificity,
                scope_proximity: declaration.scope_proximity,
                stylesheet_index: declaration.stylesheet_index,
                rule_order: declaration.rule_order,
                declaration_order: declaration.declaration_order,
            });
        }
    }
    output
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
pub(in crate::css) fn contains_css_variable_reference(value: &str) -> bool {
    css_variable_references(value).is_some_and(|references| !references.is_empty())
}

fn resolve_css_variables_inner(
    value: &str,
    custom_properties: &std::collections::HashMap<String, String>,
    stack: &mut Vec<String>,
) -> Option<String> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    resolve_component_values(&mut parser, custom_properties, stack).ok()
}

/// Resolves a token stream while preserving source spelling for all components
/// other than substituted `var()` functions.
///
/// CSS Syntax, rather than string scanning, is responsible for recognizing
/// functions and identifiers here. This makes escaped custom-property names,
/// comments, nested component blocks, and EOF-closed blocks behave exactly as
/// they do in declaration parsing:
/// <https://www.w3.org/TR/css-syntax-3/#tokenization>.
fn resolve_component_values(
    parser: &mut Parser<'_, '_>,
    custom_properties: &std::collections::HashMap<String, String>,
    stack: &mut Vec<String>,
) -> Result<String, ()> {
    let source_start = parser.position();
    let mut copied_start = source_start;
    let mut output = String::new();

    loop {
        let token_start = parser.position();
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token,
            Err(_) => break,
        };
        let block_kind = match token {
            cssparser::Token::Function(name) => Some(name.eq_ignore_ascii_case("var")),
            cssparser::Token::ParenthesisBlock
            | cssparser::Token::SquareBracketBlock
            | cssparser::Token::CurlyBracketBlock => Some(false),
            _ => None,
        };
        let Some(is_var) = block_kind else {
            continue;
        };
        let block_start = parser.position();
        output.push_str(parser.slice(copied_start..token_start));
        let (replacement, block_end) = if is_var {
            let replacement = parser
                .parse_nested_block(|nested| {
                    resolve_var_function(nested, custom_properties, stack)
                        .ok_or_else(|| nested.new_custom_error::<(), ()>(()))
                })
                .map_err(|_| ())?;
            (replacement, block_start)
        } else {
            let (contents, block_end) = parser
                .parse_nested_block(|nested| {
                    let contents = resolve_component_values(nested, custom_properties, stack)
                        .map_err(|_| nested.new_custom_error(()))?;
                    Ok::<_, cssparser::ParseError<'_, ()>>((contents, nested.position()))
                })
                .map_err(|_| ())?;
            output.push_str(parser.slice(token_start..block_start));
            output.push_str(&contents);
            (String::new(), block_end)
        };
        if is_var {
            // A substituted value is a token stream, not text pasted into the
            // surrounding source. Whitespace keeps the later property parser
            // from merging adjacent identifier, number, or dimension tokens.
            output.push(' ');
            output.push_str(&replacement);
            output.push(' ');
        }
        let after_block = parser.position();
        if !is_var {
            output.push_str(parser.slice(block_end..after_block));
        }
        copied_start = after_block;
    }
    output.push_str(parser.slice_from(copied_start));
    Ok(output)
}

fn resolve_var_function(
    parser: &mut Parser<'_, '_>,
    custom_properties: &std::collections::HashMap<String, String>,
    stack: &mut Vec<String>,
) -> Option<String> {
    let name = parser.expect_ident().ok()?.to_string();
    if !is_custom_property_name(&name) {
        return None;
    }
    let has_fallback = if parser.is_exhausted() {
        false
    } else {
        matches!(parser.next().ok()?, cssparser::Token::Comma)
    };
    let fallback = has_fallback
        .then(|| resolve_component_values(parser, custom_properties, stack))
        .transpose()
        .ok()
        .flatten();
    if has_fallback && fallback.is_none() {
        return None;
    }
    if !parser.is_exhausted() {
        return None;
    }
    if stack.iter().any(|item| item == &name) {
        return fallback;
    }
    if let Some(replacement) = custom_properties.get(&name) {
        stack.push(name);
        let replacement = resolve_css_variables_inner(replacement, custom_properties, stack);
        stack.pop();
        replacement.or(fallback)
    } else {
        fallback
    }
}

/// Returns decoded custom-property references found in a valid CSS component
/// value. Names are CSS token values, not source substrings.
fn css_variable_references(value: &str) -> Option<Vec<String>> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut references = Vec::new();
    collect_css_variable_references(&mut parser, &mut references)?;
    Some(references)
}

fn collect_css_variable_references(
    parser: &mut Parser<'_, '_>,
    references: &mut Vec<String>,
) -> Option<()> {
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        let is_var =
            matches!(token, cssparser::Token::Function(name) if name.eq_ignore_ascii_case("var"));
        let is_block = is_var
            || matches!(
                token,
                cssparser::Token::Function(_)
                    | cssparser::Token::ParenthesisBlock
                    | cssparser::Token::SquareBracketBlock
                    | cssparser::Token::CurlyBracketBlock
            );
        if !is_block {
            continue;
        }
        parser
            .parse_nested_block(|nested| {
                if is_var {
                    let name = nested.expect_ident()?.to_string();
                    if !is_custom_property_name(&name) {
                        return Err(nested.new_custom_error(()));
                    }
                    references.push(name);
                    if nested.is_exhausted() {
                        return Ok::<_, cssparser::ParseError<'_, ()>>(());
                    }
                    if !matches!(nested.next()?, cssparser::Token::Comma) {
                        return Err(nested.new_custom_error(()));
                    }
                }
                collect_css_variable_references(nested, references)
                    .ok_or_else(|| nested.new_custom_error(()))
            })
            .ok()?;
    }
    Some(())
}
