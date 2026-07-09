use super::selector::{QuireSelectorImpl, QuireSelectorParser};
use super::types::{
    ContainerRule, CounterStyleRange, CounterStyleRangeInterval, CounterStyleRule,
    CounterStyleSystem, Css, CssFontFace, Declarations, Direction, Display, FontFaceSource,
    FontStyle, FontWeight, FontWidth, KeyframeStep, KeyframesRule, MediaEnvironment, PagePseudo,
    PageRule, PageSelector, PageSpecificity, ScopeRule, StyleRule, Stylesheet, StylesheetOrigin,
    UnicodeRange,
};
use super::values::{
    parse_color, parse_css_string_token, parse_display, parse_font_family_names, parse_font_style,
    parse_font_weight, parse_font_width, split_css_component_values, trim_css_value,
};
use cssparser::{
    BasicParseErrorKind, CowRcStr, Parser, ParserInput, ParserState, RuleBodyItemParser,
    RuleBodyParser, StyleSheetParser,
};
use selectors::parser::{ParseRelative, SelectorList, SelectorParseErrorKind};
use std::collections::HashMap;
use std::rc::Rc;

pub(crate) fn parse_stylesheet(css: &Css) -> Stylesheet {
    parse_stylesheet_with_media_environment(css, &MediaEnvironment::default())
}

pub(crate) fn parse_stylesheet_with_media_environment(
    css: &Css,
    media_environment: &MediaEnvironment,
) -> Stylesheet {
    let expanded_source = recover_eof_component_blocks(&expand_nested_rules(
        &expand_custom_media_rules(css.source()),
    ));
    let mut input = ParserInput::new(&expanded_source);
    let mut parser = Parser::new(&mut input);
    let layers = LayerRegistry::new_shared();
    let namespaces = NamespaceRegistry::new_shared();
    {
        let mut layers = layers.borrow_mut();
        for name in css.layer_order_prefix() {
            layers.register(name);
        }
        if let Some(layer_name) = css.import_layer_name() {
            layers.register(layer_name);
        }
    }
    let mut rule_parser = CssRuleParser {
        base_url: css.base_url().cloned(),
        root_url: css.root_url().cloned(),
        layers: Rc::clone(&layers),
        namespaces,
        current_layer: css.import_layer_name().map(ToOwned::to_owned),
        current_scopes: Vec::new(),
        media_environment: *media_environment,
    };
    let mut parsed_rules = Vec::new();
    let mut parsed_container_rules = Vec::new();
    let mut parsed_marker_rules = Vec::new();
    let mut parsed_before_marker_rules = Vec::new();
    let mut parsed_after_marker_rules = Vec::new();
    let mut parsed_before_rules = Vec::new();
    let mut parsed_after_rules = Vec::new();
    let mut parsed_first_line_rules = Vec::new();
    let mut parsed_first_letter_rules = Vec::new();
    let mut parsed_keyframes = Vec::new();

    for item in StyleSheetParser::new(&mut parser, &mut rule_parser).flatten() {
        flatten_rule(
            item,
            &mut parsed_rules,
            &mut parsed_container_rules,
            &mut parsed_marker_rules,
            &mut parsed_before_marker_rules,
            &mut parsed_after_marker_rules,
            &mut parsed_before_rules,
            &mut parsed_after_rules,
            &mut parsed_first_line_rules,
            &mut parsed_first_letter_rules,
            &mut parsed_keyframes,
        );
    }

    let rules = parsed_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let marker_rules = parsed_marker_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let before_marker_rules = parsed_before_marker_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let after_marker_rules = parsed_after_marker_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let before_rules = parsed_before_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let after_rules = parsed_after_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let first_line_rules = parsed_first_line_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let first_letter_rules = parsed_first_letter_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let layer_names = layers.borrow().names();
    let namespace_prefixes = rule_parser.namespaces.borrow().prefixes.clone();
    let page_rules = parse_page_rules(
        css.source(),
        css.base_url(),
        css.root_url(),
        css.origin(),
        &layer_names,
        css.import_layer_name(),
        media_environment,
    );
    let page_declarations = page_rules
        .iter()
        .filter(|rule| rule.selectors.is_empty())
        .fold(Declarations::new(), |mut declarations, rule| {
            declarations.extend(rule.declarations.clone());
            declarations
        });
    let first_page_declarations = cascade_page_declarations(&page_rules, 1);

    Stylesheet {
        origin: css.origin(),
        html_presentational_hints: false,
        specificity_override: css.specificity_override(),
        layer_names,
        namespace_prefixes,
        rules,
        container_rules: parsed_container_rules,
        keyframes: parsed_keyframes,
        marker_rules,
        before_marker_rules,
        after_marker_rules,
        before_rules,
        after_rules,
        first_line_rules,
        first_letter_rules,
        page_rules,
        page_declarations,
        first_page_declarations,
        font_faces: parse_font_faces(css),
        font_feature_values: parse_font_feature_values(css),
        font_palette_values: parse_font_palette_values(css),
        counter_styles: parse_counter_styles(css),
    }
}

/// Recovers simple blocks that reach stylesheet EOF without a closing token.
///
/// CSS Syntax's error handling consumes an unterminated simple block through
/// EOF rather than discarding declarations that preceded it. Completing the
/// structural delimiters gives the declaration parser that same recovery
/// boundary while preserving all tokens that were present in the stylesheet.
/// <https://www.w3.org/TR/css-syntax-3/#consume-a-simple-block>
fn recover_eof_component_blocks(source: &str) -> String {
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for byte in source.bytes() {
        if let Some(quote_byte) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == quote_byte {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => parentheses = parentheses.saturating_add(1),
            b')' => parentheses = parentheses.saturating_sub(1),
            b'[' => brackets = brackets.saturating_add(1),
            b']' => brackets = brackets.saturating_sub(1),
            b'{' => braces = braces.saturating_add(1),
            b'}' => braces = braces.saturating_sub(1),
            _ => {}
        }
    }
    if quote.is_none() && parentheses == 0 && brackets == 0 && braces == 0 {
        return source.to_string();
    }
    let mut recovered = source.to_string();
    if let Some(quote) = quote {
        recovered.push(quote as char);
    }
    recovered.extend(std::iter::repeat_n(')', parentheses));
    recovered.extend(std::iter::repeat_n(']', brackets));
    recovered.extend(std::iter::repeat_n('}', braces));
    recovered
}

/// Expands stylesheet-scoped custom media aliases before normal rule parsing.
///
/// CSS Media Queries Level 5 defines `@custom-media` as a named media-query
/// alias. Definitions are substituted in later uses in the same stylesheet;
/// malformed definitions remain inert:
/// <https://drafts.csswg.org/mediaqueries-5/#custom-mq>.
fn expand_custom_media_rules(source: &str) -> String {
    let mut definitions = HashMap::new();
    let mut output = String::new();
    for statement in source.split_inclusive(';') {
        let trimmed = statement.trim();
        let Some(prelude) = trimmed.strip_prefix("@custom-media") else {
            output.push_str(statement);
            continue;
        };
        let mut parts = prelude.trim().splitn(2, char::is_whitespace);
        let Some(name) = parts.next().filter(|name| name.starts_with("--")) else {
            continue;
        };
        let Some(query) = parts
            .next()
            .map(str::trim)
            .filter(|query| !query.is_empty())
        else {
            continue;
        };
        definitions.insert(name.to_string(), query.to_string());
    }
    for (name, query) in definitions {
        output = output.replace(&format!("({name})"), &format!("({query})"));
    }
    output
}

mod counter_style;
mod declarations;
mod font_face;
mod nesting;
mod page_margin;
mod rule_parser;

use counter_style::*;
pub(crate) use declarations::parse_declarations;
use declarations::*;
use font_face::*;
use nesting::*;
pub(crate) use page_margin::cascade_page_declarations;
use page_margin::*;
use rule_parser::*;
pub(crate) use rule_parser::{
    custom_property_value_is_valid, is_custom_property_name, media_rule_applies,
    media_rule_applies_in_environment, supports_condition_applies,
};
