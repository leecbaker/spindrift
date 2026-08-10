use super::declarations::{CascadedDeclaration, affected_longhands, expand_modeled_shorthands};
use super::*;
use crate::css::component_values::CssComponentValueList;
use crate::css::{custom_property_value_is_valid, is_custom_property_name};
use cssparser::{Parser, ParserInput};
use std::borrow::Cow;

pub(super) fn apply_cascaded_custom_property_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
) {
    style.custom_properties.retain(|name, _| {
        style
            .registered_custom_properties
            .by_name
            .get(name)
            .is_none_or(|registration| registration.inherits)
    });
    for (name, registration) in &style.registered_custom_properties.by_name {
        style
            .custom_properties
            .entry(name.clone())
            .or_insert_with(|| ComputedCustomPropertyValue::Color(registration.initial_color));
    }
    for declaration in declarations {
        if let Some(name) = declaration.property.custom_name() {
            let value = trim_css_value(&declaration.value);
            let registration = style.registered_custom_properties.by_name.get(name);
            match value.to_ascii_lowercase().as_str() {
                // CSS-wide keywords have their ordinary cascade meaning for
                // custom properties. `initial` produces the guaranteed-invalid
                // value, represented by removing the inherited entry; custom
                // properties inherit by default, so `inherit` and `unset`
                // retain the already-inherited map entry.
                // <https://www.w3.org/TR/css-variables-1/#defining-variables>
                "initial" => {
                    if let Some(registration) = registration {
                        style.custom_properties.insert(
                            name.to_string(),
                            ComputedCustomPropertyValue::Color(registration.initial_color),
                        );
                    } else {
                        style.custom_properties.remove(name);
                    }
                }
                "inherit" => {
                    if let Some(registration) = registration
                        && !registration.inherits
                    {
                        let value = inheritance_source
                            .custom_properties
                            .get(name)
                            .cloned()
                            .unwrap_or(ComputedCustomPropertyValue::Color(
                                registration.initial_color,
                            ));
                        style.custom_properties.insert(name.to_string(), value);
                    }
                }
                "unset" if registration.is_some_and(|registration| !registration.inherits) => {
                    style.custom_properties.insert(
                        name.to_string(),
                        ComputedCustomPropertyValue::Color(
                            registration.expect("checked").initial_color,
                        ),
                    );
                }
                "unset" => {}
                _ => {
                    let value = CssComponentValueList::parse(value)
                        .expect("custom-property declaration was validated before cascade");
                    style
                        .custom_properties
                        .insert(name.to_string(), ComputedCustomPropertyValue::Tokens(value));
                }
            }
        }
    }
    resolve_custom_properties_at_computed_value_time(style);
}

/// Applies `<color>` registration syntax after the owning element's used color
/// scheme is known. Invalid values reset to the registered initial value.
pub(super) fn compute_registered_custom_property_values(style: &mut ComputedStyle) {
    let registrations = std::sync::Arc::clone(&style.registered_custom_properties);
    for (name, registration) in &registrations.by_name {
        let value = style
            .custom_properties
            .get(name)
            .map(ComputedCustomPropertyValue::substitution_tokens);
        let color = value.as_deref().and_then(|value| {
            parse_color_from_currentcolor_in_scheme(value, style.color, style.used_color_scheme)
                .or_else(|| parse_color(value))
        });
        style.custom_properties.insert(
            name.clone(),
            color.map(ComputedCustomPropertyValue::Color).unwrap_or(
                ComputedCustomPropertyValue::Color(registration.initial_color),
            ),
        );
    }
}

/// Computes the inherited `color-scheme` property before colors are parsed.
/// A `light-dark()` color must observe the scheme of its owning element, not
/// the renderer's print default.
/// <https://www.w3.org/TR/css-color-adjust-1/#color-scheme-prop>
pub(super) fn apply_cascaded_color_scheme_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
    color_scheme_preference: ColorSchemePreference,
) {
    for declaration in declarations
        .iter()
        .rev()
        .filter(|declaration| declaration.property.css_name() == "color-scheme")
    {
        let raw = trim_css_value(&declaration.value);
        let resolved;
        let value = if contains_css_variable_reference(raw) {
            let Some(value) = resolve_css_variables(raw, &style.custom_properties) else {
                style.color_scheme = inheritance_source.color_scheme.clone();
                style.used_color_scheme = inheritance_source.used_color_scheme;
                return;
            };
            resolved = value;
            trim_css_value(&resolved)
        } else {
            raw
        };
        match value.to_ascii_lowercase().as_str() {
            "initial" => {
                style.color_scheme = ComputedColorScheme::Normal;
                // The initial `normal` value uses the page's scheme, rather
                // than a fixed light scheme. This matters on descendants of a
                // root whose used scheme is dark.
                style.used_color_scheme = style.page_color_scheme;
                return;
            }
            "inherit" | "unset" => {
                style.color_scheme = inheritance_source.color_scheme.clone();
                style.used_color_scheme = inheritance_source.used_color_scheme;
                return;
            }
            _ => {}
        }
        if let Some(color_scheme) = ComputedColorScheme::parse(value) {
            let page_scheme = style.page_color_scheme;
            style.used_color_scheme =
                color_scheme.used_scheme(color_scheme_preference, page_scheme);
            style.color_scheme = color_scheme;
            return;
        }
    }
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
        .filter_map(|(name, value)| match value {
            // Registered properties have already reached their typed computed
            // value.  They contain no unresolved `var()` references, and
            // serializing them here would add an unnecessary parse/serialize
            // round trip before the actual substitution boundary.
            ComputedCustomPropertyValue::Color(color) => {
                Some((name.clone(), ComputedCustomPropertyValue::Color(*color)))
            }
            ComputedCustomPropertyValue::Tokens(value) => {
                resolve_css_variables(value.as_css(), &custom_properties)
                    .and_then(|value| CssComponentValueList::parse(&value))
                    .map(|value| (name.clone(), ComputedCustomPropertyValue::Tokens(value)))
            }
        })
        .collect();
    style.custom_properties = resolved;
}

fn cyclic_custom_properties(
    custom_properties: &std::collections::HashMap<String, ComputedCustomPropertyValue>,
) -> std::collections::HashSet<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Visit {
        Visiting,
        Visited,
    }

    fn visit(
        name: &str,
        custom_properties: &std::collections::HashMap<String, ComputedCustomPropertyValue>,
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
        for dependency in value
            .token_stream()
            .map(custom_property_dependencies)
            .unwrap_or_default()
        {
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
    for declaration in declarations.iter().rev().filter(|declaration| {
        matches!(
            declaration.property.modeled(),
            Some(
                ModeledProperty::Longhand(ModeledLonghand::FontSize)
                    | ModeledProperty::FontComponent(ModeledLonghand::FontSize)
                    | ModeledProperty::Shorthand(ModeledShorthand::Font)
            )
        )
    }) {
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
        if matches!(
            declaration.property.modeled(),
            Some(ModeledProperty::Longhand(ModeledLonghand::FontSize))
        ) && let Some(font_size) = parse_deferred_font_size(value)
        {
            set_deferred_font_size(style, font_size, inherited_font_size, inherited_ch_advance);
            return;
        }
        if matches!(
            declaration.property.modeled(),
            Some(
                ModeledProperty::FontComponent(ModeledLonghand::FontSize)
                    | ModeledProperty::Shorthand(ModeledShorthand::Font)
            )
        ) && let Some(font) = parse_font_shorthand_with_parent_ch_advance(
            value,
            inherited_font_size,
            inherited_ch_advance,
            style.font_weight,
        ) {
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
        .filter(|declaration| declaration.property.css_name() == "color")
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
        if let Some(color) = parse_color_from_currentcolor_in_scheme(
            value,
            inheritance_source.color,
            style.used_color_scheme,
        )
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
    custom_properties: &std::collections::HashMap<String, ComputedCustomPropertyValue>,
    direction: Direction,
    writing_mode: WritingMode,
) -> Vec<CascadedDeclaration<'a>> {
    let mut output = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        if declaration.property.custom_name().is_some()
            || !contains_css_variable_reference(&declaration.value)
        {
            output.push(declaration.clone());
            continue;
        }
        let Some(property) = declaration.property.modeled() else {
            output.push(declaration.clone());
            continue;
        };
        let affected = affected_longhands(property, direction, writing_mode);
        let requires_longhand_expansion = !matches!(property, ModeledProperty::Longhand(_));
        if !requires_longhand_expansion {
            // Longhands retain their authored var() token stream. Their
            // property-specific cascade path performs substitution and can
            // therefore distinguish a post-substitution grammar failure
            // (invalid at computed-value time) from an earlier winner.
            output.push(declaration.clone());
            continue;
        }
        let Some(value) = resolve_css_variables(&declaration.value, custom_properties) else {
            // A pending shorthand still wins the cascade for every longhand
            // it addresses. Keep one unresolved declaration per affected
            // longhand so the normal invalid-at-computed-value-time handling
            // suppresses earlier winners instead of reviving them.
            output.extend(affected.into_iter().map(|target| {
                let mut pending = declaration.clone();
                pending.property = CascadedProperty::Modeled(ModeledProperty::Longhand(target));
                pending
            }));
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
                property: match expanded.property {
                    CascadedProperty::Modeled(property) => CascadedProperty::Modeled(property),
                    CascadedProperty::Custom(_) => {
                        unreachable!("custom properties do not enter shorthand expansion")
                    }
                },
                value: Cow::Owned(expanded.value.into_owned()),
                origin: declaration.origin,
                base_url: declaration.base_url,
                root_url: declaration.root_url,
                important: declaration.important,
                layer_order: declaration.layer_order.clone(),
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
    custom_properties: &std::collections::HashMap<String, ComputedCustomPropertyValue>,
) -> Option<String> {
    if !custom_property_value_is_valid(value) {
        return None;
    }
    resolve_css_variables_inner(value, custom_properties, &mut Vec::new()).map(|value| {
        crate::css::component_values::trim_css_component_value_edges(&value).to_string()
    })
}

/// Returns whether a token stream contains a `var()` function. CSS function
/// names are ASCII case-insensitive, unlike custom-property names.
pub(in crate::css) fn contains_css_variable_reference(value: &str) -> bool {
    css_variable_references(value).is_some_and(|references| !references.is_empty())
}

fn resolve_css_variables_inner(
    value: &str,
    custom_properties: &std::collections::HashMap<String, ComputedCustomPropertyValue>,
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
    custom_properties: &std::collections::HashMap<String, ComputedCustomPropertyValue>,
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
            // surrounding source. A CSS comment is the canonical token
            // boundary: it prevents re-tokenization from merging adjacent
            // identifiers, numbers, or dimensions without adding whitespace
            // semantics to the substituted value.
            output.push_str("/**/");
            output.push_str(&replacement);
            output.push_str("/**/");
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
    custom_properties: &std::collections::HashMap<String, ComputedCustomPropertyValue>,
    stack: &mut Vec<String>,
) -> Option<String> {
    let name = parser.expect_ident().ok()?.to_string();
    if !is_custom_property_name(&name) {
        return None;
    }
    let fallback = if parser.is_exhausted() {
        None
    } else {
        if !matches!(parser.next().ok()?, cssparser::Token::Comma) {
            return None;
        }
        let start = parser.position();
        consume_component_values(parser).ok()?;
        let fallback = parser.slice(start..parser.position()).to_string();
        Some(fallback)
    };

    // CSS Variables substitutes the fallback only when the referenced
    // property is guaranteed-invalid. Resolving it eagerly would make an
    // unselected `var(--missing)` invalidate `var(--defined, var(--missing))`.
    // The complete value has already passed specified-value-time validation in
    // `resolve_css_variables`, so consuming the fallback above only preserves
    // this function's component-value boundary for the enclosing parser.
    let replacement = if stack.iter().any(|item| item == &name) {
        None
    } else if let Some(replacement) = custom_properties.get(&name) {
        stack.push(name);
        let replacement = replacement.substitution_tokens();
        let replacement = resolve_css_variables_inner(&replacement, custom_properties, stack);
        stack.pop();
        replacement
    } else {
        None
    };
    replacement.or_else(|| {
        fallback
            .and_then(|fallback| resolve_css_variables_inner(&fallback, custom_properties, stack))
    })
}

/// Consumes a component-value stream without performing variable substitution.
///
/// `var()` fallbacks must be parsed immediately so the enclosing function has
/// a well-defined boundary, but their substitutions are conditional on the
/// primary custom property's validity.
fn consume_component_values(parser: &mut Parser<'_, '_>) -> Result<(), ()> {
    while !parser.is_exhausted() {
        let token = parser
            .next_including_whitespace_and_comments()
            .map_err(|_| ())?;
        if matches!(
            token,
            cssparser::Token::Function(_)
                | cssparser::Token::ParenthesisBlock
                | cssparser::Token::SquareBracketBlock
                | cssparser::Token::CurlyBracketBlock
        ) {
            parser
                .parse_nested_block(|nested| {
                    consume_component_values(nested)
                        .map_err(|_| nested.new_custom_error::<(), ()>(()))
                })
                .map_err(|_| ())?;
        }
    }
    Ok(())
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
        if token.is_parse_error() {
            return None;
        }
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
