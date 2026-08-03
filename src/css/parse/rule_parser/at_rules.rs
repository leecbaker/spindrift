use super::*;
use crate::css::component_values::find_matching_brace;
use crate::css::{PropertyRegistrationRule, RegisteredCustomProperty};

pub(super) fn collect_container_style_rules(rule: ParsedCssRule, rules: &mut Vec<StyleRule>) {
    match rule {
        ParsedCssRule::Style(rule) => rules.push(rule),
        ParsedCssRule::Nested(nested) => {
            for rule in nested {
                collect_container_style_rules(rule, rules);
            }
        }
        ParsedCssRule::Marker(_)
        | ParsedCssRule::BeforeMarker(_)
        | ParsedCssRule::AfterMarker(_)
        | ParsedCssRule::Before(_)
        | ParsedCssRule::After(_)
        | ParsedCssRule::FootnoteCall(_)
        | ParsedCssRule::FootnoteMarker(_)
        | ParsedCssRule::FirstLine(_)
        | ParsedCssRule::FirstLetter(_)
        | ParsedCssRule::Container(_)
        | ParsedCssRule::Keyframes(_)
        | ParsedCssRule::FontFace(_)
        | ParsedCssRule::CounterStyle(_)
        | ParsedCssRule::FontFeatureValues(_)
        | ParsedCssRule::FontPaletteValues(_, _)
        | ParsedCssRule::Property(_)
        | ParsedCssRule::Ignored => {}
    }
}

/// Parses the `<color>` subset of CSS Properties and Values registrations.
/// Unknown descriptors are intentionally ignored, whereas missing required
/// descriptors invalidate the whole rule.
pub(in crate::css) fn parse_property_rule(
    prelude: &str,
    block: &str,
) -> Option<PropertyRegistrationRule> {
    let names = crate::css::component_values::split_css_top_level_delimiter(prelude, ',')
        .into_iter()
        .map(str::trim)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if names.is_empty()
        || names
            .iter()
            .any(|name| !crate::css::is_custom_property_name(name))
    {
        return None;
    }
    let declarations = crate::css::parse::parse_declarations(block);
    let mut syntax = None;
    let mut inherits = None;
    let mut initial_value = None;
    for (name, value) in declarations.iter() {
        match name.as_str() {
            "syntax" => {
                syntax = crate::css::component_values::parse_css_string_token(value)
                    .filter(|(_, remainder)| remainder.trim().is_empty())
                    .map(|(value, _)| value)
            }
            "inherits" => {
                inherits = match value.trim().to_ascii_lowercase().as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => return None,
                }
            }
            "initial-value" => initial_value = Some(value.trim()),
            _ => {}
        }
    }
    if syntax.as_deref()? != "<color>" {
        return None;
    }
    let initial_value = initial_value?;
    // `currentColor` and light-dark() are not computationally independent.
    if initial_value.to_ascii_lowercase().contains("currentcolor")
        || initial_value.to_ascii_lowercase().contains("light-dark(")
    {
        return None;
    }
    Some(PropertyRegistrationRule {
        names,
        registration: RegisteredCustomProperty {
            inherits: inherits?,
            initial_color: crate::css::values::parse_color(initial_value)?,
        },
    })
}

pub(in crate::css) fn parse_layer_name_list(parent: Option<&str>, prelude: &str) -> Vec<String> {
    prelude
        .split(',')
        .filter_map(|name| qualify_layer_name(parent, name))
        .collect()
}

pub(in crate::css) fn qualify_layer_name(parent: Option<&str>, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let name = match parent {
        Some(parent) if !parent.is_empty() => format!("{parent}.{name}"),
        _ => name.to_string(),
    };
    Some(name)
}

pub(in crate::css) fn parse_namespace_prelude(prelude: &str) -> Option<(Option<String>, String)> {
    let mut input = ParserInput::new(prelude);
    let mut parser = Parser::new(&mut input);
    let prefix = parser
        .try_parse(|parser| parser.expect_ident_cloned())
        .ok()
        .map(|prefix| prefix.as_ref().to_string());
    let namespace_url = if let Ok(url) = parser.try_parse(|parser| parser.expect_url()) {
        url.as_ref().to_string()
    } else {
        parser
            .expect_string_cloned()
            .ok()
            .map(|value| value.as_ref().to_string())?
    };
    parser.is_exhausted().then_some((prefix, namespace_url))
}

/// Parses the supported CSS Cascade 5 `@scope` prelude forms.
///
/// Quire currently accepts explicit root selectors and optional lower
/// boundaries, `@scope (<root>)` and `@scope (<root>) to (<limit>)`. Invalid
/// or unsupported preludes are ignored so their declarations do not enter the
/// cascade:
/// <https://www.w3.org/TR/css-cascade-5/#scope-atrule>.
pub(in crate::css) fn parse_scope_prelude(
    prelude: &str,
    selector_parser: &QuireSelectorParser,
) -> Option<ScopeRule> {
    let prelude = prelude.trim();
    let (root_text, after_root) = parse_parenthesized_selector(prelude)?;
    let root = parse_scope_selector(root_text, selector_parser)?;
    let after_root = after_root.trim();
    if after_root.is_empty() {
        return Some(ScopeRule { root, limit: None });
    }
    let after_to = strip_ascii_word_prefix(after_root, "to")?.trim();
    let (limit_text, after_limit) = parse_parenthesized_selector(after_to)?;
    if !after_limit.trim().is_empty() {
        return None;
    }
    Some(ScopeRule {
        root,
        limit: Some(parse_scope_selector(limit_text, selector_parser)?),
    })
}

pub(in crate::css) fn parse_parenthesized_selector(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start();
    if !value.starts_with('(') {
        return None;
    }
    let close = matching_parenthesis(value, 0)?;
    let selector = value[1..close].trim();
    (!selector.is_empty()).then_some((selector, &value[close + 1..]))
}

pub(in crate::css) fn parse_scope_selector(
    selector: &str,
    selector_parser: &QuireSelectorParser,
) -> Option<SelectorList<QuireSelectorImpl>> {
    let mut input = ParserInput::new(selector);
    let mut parser = Parser::new(&mut input);
    let selector = SelectorList::parse(selector_parser, &mut parser, ParseRelative::No).ok()?;
    parser.is_exhausted().then_some(selector)
}

pub(in crate::css) fn matching_parenthesis(value: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut string_quote = None;
    let bytes = value.as_bytes();
    let mut index = open;
    while index < bytes.len() {
        if let Some(quote) = string_quote {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index] == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }
        match bytes[index] {
            b'\'' | b'"' => string_quote = Some(bytes[index]),
            b'(' => depth = depth.saturating_add(1),
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

pub(in crate::css) fn strip_ascii_word_prefix<'a>(value: &'a str, word: &str) -> Option<&'a str> {
    let value = value.trim_start();
    let prefix = value.get(..word.len())?;
    if !prefix.eq_ignore_ascii_case(word) {
        return None;
    }
    if !word_boundary_after(value.as_bytes(), word.len()) {
        return None;
    }
    let rest = value[word.len()..].trim_start();
    (!rest.is_empty()).then_some(rest)
}

fn word_boundary_after(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index)
        .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-' && *byte != b'_')
}

/// Parses the contents of a CSS `@keyframes` rule.
///
/// Keyframe selectors accept `from`, `to`, and percentages. A selector list
/// creates one step for each valid offset, while invalid selectors are simply
/// ignored as required by CSS Animations error handling:
/// <https://www.w3.org/TR/css-animations-1/#keyframes>
pub(in crate::css) fn parse_keyframes_rule(name: &str, body: &str) -> Option<KeyframesRule> {
    let name = name.trim();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let mut steps = Vec::new();
    let mut rest = body;
    while let Some(open) = crate::css::component_values::find_next_top_level_open_brace(rest, 0) {
        let selectors = rest[..open].trim();
        let Some(close) = find_matching_brace(rest, open, false) else {
            break;
        };
        let declarations = parse_declarations(&rest[open + 1..close]);
        for selector in crate::css::component_values::split_css_top_level_delimiter(selectors, ',')
        {
            let offset = match selector.trim().to_ascii_lowercase().as_str() {
                "from" => Some(0.0),
                "to" => Some(1.0),
                percentage => percentage
                    .strip_suffix('%')
                    .and_then(|value| value.trim().parse::<f32>().ok())
                    .map(|value| value / 100.0)
                    .filter(|value| (0.0..=1.0).contains(value)),
            };
            if let Some(offset) = offset {
                steps.push(KeyframeStep {
                    offset,
                    declarations: declarations.clone(),
                });
            }
        }
        rest = &rest[close + 1..];
    }
    (!steps.is_empty()).then_some(KeyframesRule {
        name: name.to_string(),
        steps,
    })
}
