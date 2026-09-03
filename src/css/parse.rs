use std::collections::HashMap;
use std::rc::Rc;

use cssparser::{
    BasicParseErrorKind, CowRcStr, Parser, ParserInput, ParserState, RuleBodyItemParser,
    RuleBodyParser, StyleSheetParser,
};
use selectors::parser::{ParseRelative, SelectorList, SelectorParseErrorKind};

use super::component_values::{parse_css_string_token, split_css_component_values, trim_css_value};
use super::selector::{SpindriftSelectorImpl, SpindriftSelectorParser};
use super::types::{
    ContainerRule, CounterStyleRange, CounterStyleRangeInterval, CounterStyleRule,
    CounterStyleSystem, Css, CssFontFace, Declarations, Direction, Display, FontFaceSource,
    FontPaletteValues, FontStyle, FontWeight, FontWidth, KeyframeStep, KeyframesName,
    KeyframesRule, MediaEnvironment, PagePseudo, PageRule, PageSelector, PageSpecificity,
    ScopeRule, StyleRule, Stylesheet, StylesheetOrigin, UnicodeRange,
};
use super::values::{
    parse_color, parse_display, parse_font_family_names, parse_font_style, parse_font_weight,
    parse_font_width,
};

pub(crate) fn parse_stylesheet(css: &Css) -> Stylesheet {
    parse_stylesheet_with_media_environment(css, &MediaEnvironment::default())
}

pub(crate) fn parse_stylesheet_with_media_environment(
    css: &Css,
    media_environment: &MediaEnvironment,
) -> Stylesheet {
    let expanded_source = recover_eof_component_blocks(&expand_custom_media_rules(css.source()));
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
        base_url: css.base_url(),
        root_url: css.root_url(),
        layers: Rc::clone(&layers),
        namespaces,
        current_layer: css.import_layer_name().cloned(),
        current_scopes: Vec::new(),
        selector_scope_anchor: css.selector_scope_anchor(),
        scope_anchor: css.scope_anchor(),
        origin: css.origin(),
        media_environment: *media_environment,
        nesting: None,
        namespace_prelude_open: true,
    };
    let mut parsed_rules = Vec::new();
    let mut parsed_container_rules = Vec::new();
    let mut parsed_marker_rules = Vec::new();
    let mut parsed_before_marker_rules = Vec::new();
    let mut parsed_after_marker_rules = Vec::new();
    let mut parsed_before_rules = Vec::new();
    let mut parsed_after_rules = Vec::new();
    let mut parsed_scroll_marker_rules = Vec::new();
    let mut parsed_scroll_marker_group_rules = Vec::new();
    let mut parsed_footnote_call_rules = Vec::new();
    let mut parsed_footnote_marker_rules = Vec::new();
    let mut parsed_first_line_rules = Vec::new();
    let mut parsed_first_letter_rules = Vec::new();
    let mut parsed_keyframes = Vec::new();
    let mut parsed_font_faces = Vec::new();
    let mut parsed_counter_styles = Vec::new();
    let mut parsed_font_feature_values = Vec::new();
    let mut parsed_font_palette_values = Vec::new();
    let mut parsed_property_registrations = Vec::new();
    let mut parsed_page_rules = Vec::new();

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
            &mut parsed_scroll_marker_rules,
            &mut parsed_scroll_marker_group_rules,
            &mut parsed_footnote_call_rules,
            &mut parsed_footnote_marker_rules,
            &mut parsed_first_line_rules,
            &mut parsed_first_letter_rules,
            &mut parsed_keyframes,
            &mut parsed_font_faces,
            &mut parsed_counter_styles,
            &mut parsed_font_feature_values,
            &mut parsed_font_palette_values,
            &mut parsed_property_registrations,
            &mut parsed_page_rules,
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
    let scroll_marker_rules = parsed_scroll_marker_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let scroll_marker_group_rules = parsed_scroll_marker_group_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let footnote_call_rules = parsed_footnote_call_rules
        .into_iter()
        .enumerate()
        .map(|(order, mut rule)| {
            rule.order = order;
            rule
        })
        .collect();
    let footnote_marker_rules = parsed_footnote_marker_rules
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
    let page_rules = parsed_page_rules
        .into_iter()
        .enumerate()
        .map(|(order, rule)| PageRule {
            origin: css.origin(),
            selectors: rule.selectors,
            declarations: rule.declarations,
            margin_boxes: rule.margin_boxes,
            footnote_area: rule.footnote_area,
            order,
            layer_order: rule
                .layer
                .as_ref()
                .and_then(|name| layers.borrow().order_for(name)),
        })
        .collect::<Vec<_>>();
    let page_declarations = page_rules
        .iter()
        .filter(|rule| rule.selectors.is_empty())
        .fold(Declarations::new(), |mut declarations, rule| {
            declarations.extend(rule.declarations.clone());
            declarations
        });
    let first_page_declarations = cascade_page_declarations(&page_rules, 1);
    let font_feature_values =
        parse_font_feature_values_rules(parsed_font_feature_values, &layers.borrow());

    Stylesheet {
        origin: css.origin(),
        base_url: css.base_url().cloned(),
        root_url: css.root_url().cloned(),
        forced_colors: media_environment.forced_colors,
        color_scheme_preference: media_environment.color_scheme_preference,
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
        scroll_marker_rules,
        scroll_marker_group_rules,
        footnote_call_rules,
        footnote_marker_rules,
        first_line_rules,
        first_letter_rules,
        page_rules,
        page_declarations,
        first_page_declarations,
        font_faces: parsed_font_faces,
        font_feature_values,
        font_palette_values: {
            let mut values = FontPaletteValues::default();
            for (name, definition) in parsed_font_palette_values {
                values.insert(name, definition);
            }
            values
        },
        counter_styles: parsed_counter_styles,
        property_registrations: parsed_property_registrations,
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
            } else if matches!(byte, b'\n' | b'\r' | 0x0C) {
                // A newline terminates an unterminated string as a CSS Syntax
                // BadString token. Do not turn that tokenizer error into a
                // valid EOF-closed string while recovering outer blocks.
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
pub(in crate::css) use rule_parser::{
    LayerRegistry, cascaded_declaration_is_valid, parse_layer_name, parse_layer_name_list,
};
pub(crate) use rule_parser::{
    custom_property_value_is_valid, is_custom_property_name, media_rule_applies,
    media_rule_applies_in_environment, supports_condition_applies,
};

/// Parses a declaration at specified-value time and returns the canonical
/// property/value operation the cascade should apply.
#[allow(
    dead_code,
    reason = "the cascade uses borrowed validation; this adapter remains for standalone callers that need an owned canonical operation"
)]
pub(in crate::css) fn declaration_operation(name: &str, value: &str) -> Option<(String, String)> {
    match rule_parser::parse_canonical_declaration(name, value) {
        rule_parser::DeclarationParseResult::Valid(operation) => {
            Some((operation.name.into_owned(), operation.value.into_owned()))
        }
        rule_parser::DeclarationParseResult::UnsupportedProperty
        | rule_parser::DeclarationParseResult::InvalidValue => None,
    }
}
