use std::borrow::Cow;

use cssparser::{Parser, ParserInput};

use super::declarations::{
    CascadedDeclaration, affected_longhands, expand_modeled_shorthands, same_cascade_layer,
    same_or_stronger_reverted_origin,
};
use super::*;
use crate::css::component_values::{CssComponentValueList, parse_var_function_arguments};
use crate::css::{custom_property_value_is_valid, is_custom_property_name};

pub(super) fn apply_cascaded_custom_property_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
) {
    initialize_custom_property_inheritance(style);
    let inherited_values = style.custom_properties.clone();

    // First form a complete, raw custom-property environment. It lets a
    // rollback produced by `var()` observe the same substituted value as the
    // declaration that ultimately wins the ordinary cascade.
    apply_custom_property_declarations(style, declarations, inheritance_source);
    let candidates =
        declarations_after_custom_property_rollbacks(declarations, &style.custom_properties);

    // Rollback keywords discard candidates, rather than becoming a stored
    // custom-property value. Reapply the surviving candidates to the inherited
    // and registered-initial terminal values.
    style.custom_properties = inherited_values;
    apply_custom_property_declarations(style, &candidates, inheritance_source);
    resolve_custom_properties_at_computed_value_time(style, inheritance_source);
}

fn initialize_custom_property_inheritance(style: &mut ComputedStyle) {
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
}

fn apply_custom_property_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
) {
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
}

/// Selects custom-property candidates after resolving rollback values that
/// arise through `var()` substitution. CSS Cascade uses the same candidate
/// removal for direct and post-substitution `revert` / `revert-layer`.
/// <https://drafts.csswg.org/css-cascade-5/#revert>
/// <https://drafts.csswg.org/css-cascade-5/#revert-layer>
fn declarations_after_custom_property_rollbacks<'a>(
    declarations: &[CascadedDeclaration<'a>],
    custom_properties: &std::collections::HashMap<String, ComputedCustomPropertyValue>,
) -> Vec<CascadedDeclaration<'a>> {
    let mut output = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let Some(name) = declaration.property.custom_name() else {
            output.push(declaration.clone());
            continue;
        };
        match custom_property_rollback_keyword(declaration, custom_properties) {
            Some(CssWideKeyword::Revert) => output.retain(|candidate| {
                candidate.property.custom_name() != Some(name)
                    || !same_or_stronger_reverted_origin(candidate, declaration)
            }),
            Some(CssWideKeyword::RevertLayer) => output.retain(|candidate| {
                candidate.property.custom_name() != Some(name)
                    || !same_cascade_layer(candidate, declaration)
            }),
            _ => output.push(declaration.clone()),
        }
    }
    output
}

fn custom_property_rollback_keyword(
    declaration: &CascadedDeclaration<'_>,
    custom_properties: &std::collections::HashMap<String, ComputedCustomPropertyValue>,
) -> Option<CssWideKeyword> {
    let value = trim_css_value(&declaration.value);
    let value = if contains_css_variable_reference(value) {
        resolve_css_variables(value, custom_properties)?
    } else {
        value.to_string()
    };
    match ResolvedCustomProperty::from_value(&value) {
        ResolvedCustomProperty::CssWide(
            keyword @ (CssWideKeyword::Revert | CssWideKeyword::RevertLayer),
        ) => Some(keyword),
        _ => None,
    }
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
fn resolve_custom_properties_at_computed_value_time(
    style: &mut ComputedStyle,
    inheritance_source: &ComputedStyle,
) {
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
                let outcome = resolve_css_variables(value.as_css(), &custom_properties)
                    .map(|value| ResolvedCustomProperty::from_value(trim_css_value(&value)))
                    .unwrap_or(ResolvedCustomProperty::GuaranteedInvalid);
                match outcome {
                    ResolvedCustomProperty::CssWide(CssWideKeyword::Initial) => style
                        .registered_custom_properties
                        .by_name
                        .get(name)
                        .map(|registration| {
                            (
                                name.clone(),
                                ComputedCustomPropertyValue::Color(registration.initial_color),
                            )
                        }),
                    ResolvedCustomProperty::CssWide(CssWideKeyword::Inherit) => {
                        inherited_custom_property_value(name, style, inheritance_source)
                    }
                    ResolvedCustomProperty::CssWide(CssWideKeyword::Unset) => {
                        let registration = style.registered_custom_properties.by_name.get(name);
                        if registration.is_some_and(|registration| !registration.inherits) {
                            Some((
                                name.clone(),
                                ComputedCustomPropertyValue::Color(
                                    registration.expect("checked").initial_color,
                                ),
                            ))
                        } else {
                            inherited_custom_property_value(name, style, inheritance_source)
                        }
                    }
                    // `revert` and `revert-layer` are handled from their
                    // cascaded declaration candidates before this final token
                    // normalization pass. Keeping them out of stored custom
                    // property tokens prevents a consumer from observing an
                    // invalid literal keyword.
                    ResolvedCustomProperty::CssWide(CssWideKeyword::Revert)
                    | ResolvedCustomProperty::CssWide(CssWideKeyword::RevertLayer) => None,
                    ResolvedCustomProperty::Tokens(value) => CssComponentValueList::parse(&value)
                        .map(|value| (name.clone(), ComputedCustomPropertyValue::Tokens(value))),
                    ResolvedCustomProperty::GuaranteedInvalid => None,
                }
            }
        })
        .collect();
    style.custom_properties = resolved;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CssWideKeyword {
    Initial,
    Inherit,
    Unset,
    Revert,
    RevertLayer,
}

impl CssWideKeyword {
    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "initial" => Some(Self::Initial),
            "inherit" => Some(Self::Inherit),
            "unset" => Some(Self::Unset),
            "revert" => Some(Self::Revert),
            "revert-layer" => Some(Self::RevertLayer),
            _ => None,
        }
    }
}

/// The computed-value outcome of a custom-property token stream. CSS-wide
/// keywords remain distinct from ordinary tokens so they cannot leak through
/// as a custom property value after substitution.
enum ResolvedCustomProperty {
    Tokens(String),
    GuaranteedInvalid,
    CssWide(CssWideKeyword),
}

impl ResolvedCustomProperty {
    fn from_value(value: &str) -> Self {
        if let Some(keyword) = CssWideKeyword::parse(value) {
            Self::CssWide(keyword)
        } else {
            Self::Tokens(value.to_string())
        }
    }
}

fn inherited_custom_property_value(
    name: &str,
    style: &ComputedStyle,
    inheritance_source: &ComputedStyle,
) -> Option<(String, ComputedCustomPropertyValue)> {
    let registration = style.registered_custom_properties.by_name.get(name);
    inheritance_source
        .custom_properties
        .get(name)
        .cloned()
        .or_else(|| {
            registration
                .map(|registration| ComputedCustomPropertyValue::Color(registration.initial_color))
        })
        .map(|value| (name.to_string(), value))
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
    let mut references = Vec::new();
    visit_css_variable_references(value, |name| references.push(name.to_string()))
        .map(|()| references)
        .unwrap_or_default()
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
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    contains_var_function_from_parser(&mut parser).unwrap_or(false)
}

fn contains_var_function_from_parser(parser: &mut Parser<'_, '_>) -> Option<bool> {
    let mut contains = false;
    while !parser.is_exhausted() {
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        if token.is_parse_error() {
            return None;
        }
        let is_var = matches!(token, cssparser::Token::Function(ref name) if name.eq_ignore_ascii_case("var"));
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
        let nested_contains = parser
            .parse_nested_block(|nested| {
                if is_var {
                    parse_var_function_arguments(nested)
                        .map(|_| true)
                        .ok_or_else(|| nested.new_custom_error::<(), ()>(()))
                } else {
                    contains_var_function_from_parser(nested)
                        .ok_or_else(|| nested.new_custom_error::<(), ()>(()))
                }
            })
            .ok()?;
        contains |= nested_contains;
    }
    Some(contains)
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
    let arguments = parse_var_function_arguments(parser)?;
    let name =
        resolve_css_variables_inner(arguments.name_argument.as_css(), custom_properties, stack)
            .and_then(|value| resolved_custom_property_name(&value));

    // CSS Variables substitutes the fallback only when the referenced
    // property is guaranteed-invalid. Resolving it eagerly would make an
    // unselected `var(--missing)` invalidate `var(--defined, var(--missing))`.
    // The complete value has already passed specified-value-time validation in
    // `resolve_css_variables`, so consuming the fallback above only preserves
    // this function's component-value boundary for the enclosing parser.
    let replacement = name.as_ref().and_then(|name| {
        if stack.iter().any(|item| item == name) {
            None
        } else if let Some(replacement) = custom_properties.get(name) {
            stack.push(name.clone());
            let replacement = replacement.substitution_tokens();
            let replacement = resolve_css_variables_inner(&replacement, custom_properties, stack);
            stack.pop();
            replacement
        } else {
            None
        }
    });
    replacement.or_else(|| {
        arguments.fallback.as_ref().and_then(|fallback| {
            resolve_css_variables_inner(fallback.as_css(), custom_properties, stack)
        })
    })
}

fn resolved_custom_property_name(value: &str) -> Option<String> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let name = parser.expect_ident().ok()?.to_string();
    parser.expect_exhausted().ok()?;
    is_custom_property_name(&name).then_some(name)
}

/// Visits decoded custom-property references found in a valid CSS component
/// value. Names are CSS token values, not source substrings.
///
/// Callers that only need to know whether a reference exists can inspect the
/// borrowed name without allocating. Dependency collection is the sole caller
/// that retains names for use after parsing.
fn visit_css_variable_references(value: &str, mut visit: impl FnMut(&str)) -> Option<()> {
    visit_css_variable_references_with(value, &mut visit)
}

fn visit_css_variable_references_with(value: &str, visit: &mut dyn FnMut(&str)) -> Option<()> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    visit_css_variable_references_from_parser(&mut parser, visit)
}

fn visit_css_variable_references_from_parser(
    parser: &mut Parser<'_, '_>,
    visit: &mut dyn FnMut(&str),
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
                    let arguments = parse_var_function_arguments(nested)
                        .ok_or_else(|| nested.new_custom_error(()))?;
                    if let Some(name) =
                        resolved_custom_property_name(arguments.name_argument.as_css())
                    {
                        visit(&name);
                    } else {
                        visit_css_variable_references_with(arguments.name_argument.as_css(), visit)
                            .ok_or_else(|| nested.new_custom_error(()))?;
                    }
                    if let Some(fallback) = arguments.fallback {
                        visit_css_variable_references_with(fallback.as_css(), visit)
                            .ok_or_else(|| nested.new_custom_error(()))?;
                    }
                    return Ok::<_, cssparser::ParseError<'_, ()>>(());
                }
                visit_css_variable_references_from_parser(nested, visit)
                    .ok_or_else(|| nested.new_custom_error(()))
            })
            .ok()?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_variable_reference_detection_uses_css_syntax() {
        for value in ["red", r#""var(--tone)""#] {
            assert!(!contains_css_variable_reference(value), "{value}");
        }
        for value in [
            "calc(1px + var(--nested, var(--fallback)))",
            "[var(--one)] function(var(--two))",
            r"var(--\74 one)",
        ] {
            assert!(contains_css_variable_reference(value), "{value}");
        }
    }

    #[test]
    fn css_variable_reference_detection_rejects_malformed_functions_and_tokens() {
        for value in ["var()", "var(--tone) \"\n"] {
            assert!(!contains_css_variable_reference(value), "{value}");
        }
        for value in ["var(theme)", "var(--tone invalid)"] {
            assert!(contains_css_variable_reference(value), "{value}");
        }
    }

    #[test]
    fn custom_property_dependencies_collect_primary_and_fallback_references() {
        assert_eq!(
            custom_property_dependencies("var(--primary, calc(var(--fallback)))"),
            vec!["--primary", "--fallback"],
        );
    }

    #[test]
    fn var_name_argument_is_substituted_before_custom_property_lookup() {
        let properties = std::collections::HashMap::from([
            (
                "--name".to_string(),
                ComputedCustomPropertyValue::Tokens(
                    CssComponentValueList::parse("--color").expect("token stream"),
                ),
            ),
            (
                "--color".to_string(),
                ComputedCustomPropertyValue::Tokens(
                    CssComponentValueList::parse("green").expect("token stream"),
                ),
            ),
        ]);
        assert_eq!(
            resolve_css_variables("var(var(--name), red)", &properties),
            Some("green".to_string()),
        );
        assert_eq!(
            resolve_css_variables("var(1px, red)", &properties),
            Some("red".to_string()),
        );
        assert_eq!(
            resolve_css_variables("var(--, green)", &properties),
            Some("green".to_string()),
        );
        assert_eq!(
            resolve_css_variables("var(--missing,)", &properties),
            Some("".to_string()),
        );
    }

    #[test]
    fn custom_property_dependencies_visit_substituted_names_and_fallbacks() {
        assert_eq!(
            custom_property_dependencies("var(var(--name), var(--fallback))"),
            vec!["--name", "--fallback"],
        );
    }
}
