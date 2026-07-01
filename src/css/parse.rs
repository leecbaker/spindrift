use super::selector::{ReasySelectorImpl, ReasySelectorParser};
use super::types::{
    CounterStyleRange, CounterStyleRangeInterval, CounterStyleRule, CounterStyleSystem, Css,
    CssFontFace, Declarations, Direction, Display, FontFaceSource, FontStyle, FontWeight,
    FontWidth, PagePseudo, PageRule, PageSelector, PageSpecificity, ScopeRule, StyleRule,
    Stylesheet, StylesheetOrigin, UnicodeRange,
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
use std::path::{Path, PathBuf};

pub(crate) fn parse_stylesheet(css: &Css) -> Stylesheet {
    let expanded_source = expand_nested_rules(css.source());
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
        base_url: css.base_url().map(Path::to_path_buf),
        root_url: css.root_url().map(Path::to_path_buf),
        layers: layers.clone(),
        namespaces: namespaces.clone(),
        current_layer: css.import_layer_name().map(ToOwned::to_owned),
        current_scopes: Vec::new(),
    };
    let mut parsed_rules = Vec::new();
    let mut parsed_marker_rules = Vec::new();
    let mut parsed_before_rules = Vec::new();
    let mut parsed_after_rules = Vec::new();
    let mut parsed_first_line_rules = Vec::new();
    let mut parsed_first_letter_rules = Vec::new();

    for item in StyleSheetParser::new(&mut parser, &mut rule_parser).flatten() {
        flatten_rule(
            item,
            &mut parsed_rules,
            &mut parsed_marker_rules,
            &mut parsed_before_rules,
            &mut parsed_after_rules,
            &mut parsed_first_line_rules,
            &mut parsed_first_letter_rules,
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
    let page_rules = parse_page_rules(
        css.source(),
        css.base_url(),
        css.root_url(),
        css.origin(),
        &layer_names,
        css.import_layer_name(),
    );
    let page_declarations = page_rules
        .iter()
        .filter(|rule| rule.selectors.is_empty())
        .fold(Declarations::new(), |mut declarations, rule| {
            declarations.extend(rule.declarations.clone());
            declarations
        });
    let first_page_declarations = cascade_page_declarations(&page_rules, 1);
    let page_margin_boxes = cascade_page_margin_boxes(&page_rules, 1);

    Stylesheet {
        origin: css.origin(),
        html_presentational_hints: false,
        specificity_override: css.specificity_override(),
        layer_names,
        rules,
        marker_rules,
        before_rules,
        after_rules,
        first_line_rules,
        first_letter_rules,
        page_rules,
        page_declarations,
        first_page_declarations,
        page_margin_boxes,
        font_faces: parse_font_faces(css),
        font_feature_values: parse_font_feature_values(css),
        counter_styles: parse_counter_styles(css),
    }
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
pub(crate) use rule_parser::{media_rule_applies, supports_condition_applies};
