use super::*;
use crate::css::values::{
    parse_alignment_baseline, parse_baseline_shift, parse_baseline_source, parse_dominant_baseline,
    parse_font_feature_settings, parse_font_kerning, parse_font_shorthand, parse_font_size_adjust,
    parse_font_variant, parse_font_variant_alternates, parse_font_variant_caps,
    parse_font_variant_east_asian, parse_font_variant_emoji, parse_font_variant_ligatures,
    parse_font_variant_numeric, parse_font_variant_position, parse_hanging_punctuation,
    parse_letter_spacing, parse_tab_size, parse_text_indent, parse_vertical_align,
    parse_word_spacing,
};
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

pub(super) fn find_matching_brace(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in source.as_bytes().iter().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Finds a CSS block close, accepting EOF as an implicit close for recovery.
///
/// CSS Syntax consumes a simple block until the ending token or EOF. Page-rule
/// scanning uses this tolerant helper so an otherwise valid final `@page` rule
/// is not discarded when the stylesheet reaches EOF before the last `}`:
/// <https://www.w3.org/TR/css-syntax-3/#consume-simple-block>.
pub(super) fn find_matching_brace_or_eof(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in source.as_bytes().iter().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    (depth > 0).then_some(source.len())
}

#[derive(Debug)]
pub(super) enum ParsedCssRule {
    Style(StyleRule),
    Marker(StyleRule),
    Before(StyleRule),
    After(StyleRule),
    FirstLine(StyleRule),
    FirstLetter(StyleRule),
    Nested(Vec<ParsedCssRule>),
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoutedPseudoElement {
    Marker,
    Before,
    After,
    FirstLine,
    FirstLetter,
}

pub(super) struct CssRuleParser {
    pub(super) base_url: Option<PathBuf>,
    pub(super) root_url: Option<PathBuf>,
    pub(super) layers: SharedLayerRegistry,
    pub(super) namespaces: SharedNamespaceRegistry,
    pub(super) current_layer: Option<String>,
    pub(super) current_scopes: Vec<ScopeRule>,
}

pub(super) type SharedLayerRegistry = Rc<RefCell<LayerRegistry>>;
pub(super) type SharedNamespaceRegistry = Rc<RefCell<NamespaceRegistry>>;

/// Namespace declarations in scope for selector parsing.
///
/// CSS Namespaces Level 3 lets `@namespace` declarations define a default
/// namespace and prefix mappings for selectors:
/// <https://www.w3.org/TR/css-namespaces-3/#declaration>.
#[derive(Debug, Default)]
pub(super) struct NamespaceRegistry {
    default_namespace: Option<String>,
    prefixes: HashMap<String, String>,
}

impl NamespaceRegistry {
    pub(super) fn new_shared() -> SharedNamespaceRegistry {
        Rc::new(RefCell::new(Self::default()))
    }

    fn selector_parser(&self) -> ReasySelectorParser {
        ReasySelectorParser::new(self.default_namespace.clone(), self.prefixes.clone())
    }

    fn register(&mut self, prefix: Option<String>, namespace_url: String) {
        match prefix {
            Some(prefix) => {
                self.prefixes.insert(prefix, namespace_url);
            }
            None => self.default_namespace = Some(namespace_url),
        }
    }
}

/// Tracks cascade layer order while parsing a stylesheet.
///
/// CSS Cascade Level 5 orders layers by the first `@layer` statement or block
/// that declares them. Anonymous layers participate in that same order but
/// cannot be referenced by later rules:
/// <https://www.w3.org/TR/css-cascade-5/#layer-order>.
#[derive(Debug, Default)]
pub(super) struct LayerRegistry {
    names: Vec<String>,
    anonymous_count: usize,
}

impl LayerRegistry {
    pub(super) fn new_shared() -> SharedLayerRegistry {
        Rc::new(RefCell::new(Self::default()))
    }

    pub(super) fn names(&self) -> Vec<String> {
        self.names.clone()
    }

    pub(super) fn register(&mut self, name: &str) {
        if !self.names.iter().any(|existing| existing == name) {
            self.names.push(name.to_string());
        }
    }

    fn anonymous_name(&mut self) -> String {
        let name = format!("__anonymous_layer_{}", self.anonymous_count);
        self.anonymous_count = self.anonymous_count.saturating_add(1);
        self.register(&name);
        name
    }
}

impl<'i> cssparser::QualifiedRuleParser<'i> for CssRuleParser {
    type Prelude = (String, SelectorList<ReasySelectorImpl>);
    type QualifiedRule = ParsedCssRule;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        let start = input.position();
        let parse_relative = if self.current_scopes.is_empty() {
            ParseRelative::No
        } else {
            ParseRelative::ForScope
        };
        let selector_parser = self.namespaces.borrow().selector_parser();
        let selector = SelectorList::parse(&selector_parser, input, parse_relative)?;
        let selector_text = input.slice_from(start).trim().to_string();
        Ok((selector_text, selector))
    }

    fn parse_block<'t>(
        &mut self,
        (selector_text, selector): Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, cssparser::ParseError<'i, Self::Error>> {
        let specificity = selector
            .slice()
            .iter()
            .map(|selector| selector.specificity())
            .max()
            .unwrap_or(0);
        let declarations = parse_declarations_from_parser(
            input,
            self.base_url.as_deref(),
            self.root_url.as_deref(),
        );
        let routed_rules = split_pseudo_element_rule(
            &selector_text,
            &self.namespaces.borrow().selector_parser(),
            &declarations,
            specificity,
            self.current_layer.clone(),
            self.current_scopes.clone(),
        );
        if !routed_rules.is_empty() {
            return if routed_rules.len() == 1 {
                Ok(routed_rules.into_iter().next().expect("one routed rule"))
            } else {
                Ok(ParsedCssRule::Nested(routed_rules))
            };
        }
        Ok(ParsedCssRule::Style(StyleRule {
            selector_text,
            selector,
            declarations,
            specificity,
            order: 0,
            layer_name: self.current_layer.clone(),
            scopes: self.current_scopes.clone(),
        }))
    }
}

impl<'i> cssparser::AtRuleParser<'i> for CssRuleParser {
    type Prelude = AtRulePrelude;
    type AtRule = ParsedCssRule;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        let prelude = consume_remaining_input(input);
        if name.eq_ignore_ascii_case("media") {
            Ok(AtRulePrelude::Media(media_rule_applies(&prelude)))
        } else if name.eq_ignore_ascii_case("supports") {
            let selector_parser = self.namespaces.borrow().selector_parser();
            Ok(AtRulePrelude::Supports(
                supports_condition_applies_with_selector_parser(&prelude, &selector_parser),
            ))
        } else if name.eq_ignore_ascii_case("layer") {
            Ok(AtRulePrelude::Layer(prelude))
        } else if name.eq_ignore_ascii_case("namespace") {
            Ok(AtRulePrelude::Namespace(parse_namespace_prelude(&prelude)))
        } else if name.eq_ignore_ascii_case("scope") {
            let selector_parser = self.namespaces.borrow().selector_parser();
            Ok(AtRulePrelude::Scope(parse_scope_prelude(
                &prelude,
                &selector_parser,
            )))
        } else if name.eq_ignore_ascii_case("page") {
            Ok(AtRulePrelude::Page(prelude))
        } else {
            Ok(AtRulePrelude::Ignored)
        }
    }

    fn rule_without_block(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        match prelude {
            AtRulePrelude::Layer(prelude) => {
                for name in parse_layer_name_list(self.current_layer.as_deref(), &prelude) {
                    self.layers.borrow_mut().register(&name);
                }
                Ok(ParsedCssRule::Ignored)
            }
            AtRulePrelude::Namespace(Some((prefix, namespace_url))) => {
                self.namespaces.borrow_mut().register(prefix, namespace_url);
                Ok(ParsedCssRule::Ignored)
            }
            AtRulePrelude::Media(_)
            | AtRulePrelude::Supports(_)
            | AtRulePrelude::Page(_)
            | AtRulePrelude::Scope(_)
            | AtRulePrelude::Namespace(None)
            | AtRulePrelude::Ignored => Ok(ParsedCssRule::Ignored),
        }
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, cssparser::ParseError<'i, Self::Error>> {
        match prelude {
            AtRulePrelude::Media(true) => {
                let mut parser = CssRuleParser {
                    base_url: self.base_url.clone(),
                    root_url: self.root_url.clone(),
                    layers: self.layers.clone(),
                    namespaces: self.namespaces.clone(),
                    current_layer: self.current_layer.clone(),
                    current_scopes: self.current_scopes.clone(),
                };
                let nested = StyleSheetParser::new(input, &mut parser)
                    .flatten()
                    .collect::<Vec<_>>();
                Ok(ParsedCssRule::Nested(nested))
            }
            AtRulePrelude::Supports(true) => {
                let mut parser = CssRuleParser {
                    base_url: self.base_url.clone(),
                    root_url: self.root_url.clone(),
                    layers: self.layers.clone(),
                    namespaces: self.namespaces.clone(),
                    current_layer: self.current_layer.clone(),
                    current_scopes: self.current_scopes.clone(),
                };
                let nested = StyleSheetParser::new(input, &mut parser)
                    .flatten()
                    .collect::<Vec<_>>();
                Ok(ParsedCssRule::Nested(nested))
            }
            AtRulePrelude::Layer(prelude) => {
                let layer_name = if prelude.trim().is_empty() {
                    self.layers.borrow_mut().anonymous_name()
                } else if prelude.contains(',') {
                    consume_remaining_input(input);
                    return Ok(ParsedCssRule::Ignored);
                } else {
                    let Some(name) = qualify_layer_name(self.current_layer.as_deref(), &prelude)
                    else {
                        consume_remaining_input(input);
                        return Ok(ParsedCssRule::Ignored);
                    };
                    self.layers.borrow_mut().register(&name);
                    name
                };
                let mut parser = CssRuleParser {
                    base_url: self.base_url.clone(),
                    root_url: self.root_url.clone(),
                    layers: self.layers.clone(),
                    namespaces: self.namespaces.clone(),
                    current_layer: Some(layer_name),
                    current_scopes: self.current_scopes.clone(),
                };
                let nested = StyleSheetParser::new(input, &mut parser)
                    .flatten()
                    .collect::<Vec<_>>();
                Ok(ParsedCssRule::Nested(nested))
            }
            AtRulePrelude::Scope(Some(scope)) => {
                let mut current_scopes = self.current_scopes.clone();
                current_scopes.push(scope);
                let mut parser = CssRuleParser {
                    base_url: self.base_url.clone(),
                    root_url: self.root_url.clone(),
                    layers: self.layers.clone(),
                    namespaces: self.namespaces.clone(),
                    current_layer: self.current_layer.clone(),
                    current_scopes,
                };
                let nested = StyleSheetParser::new(input, &mut parser)
                    .flatten()
                    .collect::<Vec<_>>();
                Ok(ParsedCssRule::Nested(nested))
            }
            AtRulePrelude::Page(prelude) => {
                let _ = prelude;
                consume_remaining_input(input);
                Ok(ParsedCssRule::Ignored)
            }
            AtRulePrelude::Media(false)
            | AtRulePrelude::Supports(false)
            | AtRulePrelude::Scope(None)
            | AtRulePrelude::Namespace(_)
            | AtRulePrelude::Ignored => {
                consume_remaining_input(input);
                Ok(ParsedCssRule::Ignored)
            }
        }
    }
}

pub(super) enum AtRulePrelude {
    Media(bool),
    Supports(bool),
    Layer(String),
    Namespace(Option<(Option<String>, String)>),
    Scope(Option<ScopeRule>),
    Page(String),
    Ignored,
}

pub(super) fn parse_layer_name_list(parent: Option<&str>, prelude: &str) -> Vec<String> {
    prelude
        .split(',')
        .filter_map(|name| qualify_layer_name(parent, name))
        .collect()
}

fn qualify_layer_name(parent: Option<&str>, name: &str) -> Option<String> {
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

/// Evaluates a conservative print-context subset of CSS Media Queries.
///
/// CSS Conditional Rules delegates `@media` to Media Queries. Reasyprint's
/// current rendering context is print, so this accepts media query lists whose
/// type query matches `print`/`all`, supports comma-list OR semantics, handles
/// `not` negation and `only`, and treats unsupported media features as false:
/// <https://www.w3.org/TR/css-conditional-3/#at-media> and
/// <https://www.w3.org/TR/mediaqueries-4/#mq-list>.
pub(crate) fn media_rule_applies(prelude: &str) -> bool {
    split_top_level_commas(prelude)
        .into_iter()
        .any(media_query_applies)
}

fn media_query_applies(query: &str) -> bool {
    let mut query = query.trim();
    if query.is_empty() {
        return false;
    }
    let negated = if let Some(rest) = strip_ascii_word_prefix(query, "not") {
        query = rest.trim();
        true
    } else {
        false
    };
    if let Some(rest) = strip_ascii_word_prefix(query, "only") {
        query = rest.trim();
    }
    let applies = media_query_without_not_applies(query);
    if negated { !applies } else { applies }
}

fn media_query_without_not_applies(query: &str) -> bool {
    let parts = split_top_level_keyword(query, "and");
    let mut first = true;
    for part in parts {
        let part = part.trim();
        let applies = if first && media_type_applies(part) {
            true
        } else {
            media_feature_applies(part)
        };
        if !applies {
            return false;
        }
        first = false;
    }
    true
}

fn media_type_applies(part: &str) -> bool {
    matches!(part.trim().to_ascii_lowercase().as_str(), "all" | "print")
}

fn media_feature_applies(part: &str) -> bool {
    let feature = strip_enclosing_parentheses(part.trim());
    matches!(
        feature.to_ascii_lowercase().as_str(),
        "update: none" | "overflow-block: paged"
    )
}

fn split_top_level_commas(value: &str) -> Vec<&str> {
    split_top_level_delimiter(value, ',')
}

/// Parses the supported CSS Cascade 5 `@scope` prelude forms.
///
/// Reasyprint currently accepts explicit root selectors and optional lower
/// boundaries, `@scope (<root>)` and `@scope (<root>) to (<limit>)`. Invalid
/// or unsupported preludes are ignored so their declarations do not enter the
/// cascade:
/// <https://www.w3.org/TR/css-cascade-5/#scope-atrule>.
fn parse_namespace_prelude(prelude: &str) -> Option<(Option<String>, String)> {
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

fn parse_scope_prelude(prelude: &str, selector_parser: &ReasySelectorParser) -> Option<ScopeRule> {
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

fn parse_parenthesized_selector(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start();
    if !value.starts_with('(') {
        return None;
    }
    let close = matching_parenthesis(value, 0)?;
    let selector = value[1..close].trim();
    (!selector.is_empty()).then_some((selector, &value[close + 1..]))
}

fn parse_scope_selector(
    selector: &str,
    selector_parser: &ReasySelectorParser,
) -> Option<SelectorList<ReasySelectorImpl>> {
    let mut input = ParserInput::new(selector);
    let mut parser = Parser::new(&mut input);
    let selector = SelectorList::parse(selector_parser, &mut parser, ParseRelative::No).ok()?;
    parser.is_exhausted().then_some(selector)
}

fn matching_parenthesis(value: &str, open: usize) -> Option<usize> {
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

/// Evaluates the supported subset of CSS Conditional Rules `@supports`.
///
/// CSS Conditional Rules Level 3 defines support conditions as declaration
/// tests combined with `not`, `and`, and `or`. This evaluator is intentionally
/// conservative: unknown condition forms and unknown properties evaluate false
/// rather than letting unsupported blocks leak into the cascade:
/// <https://www.w3.org/TR/css-conditional-3/#at-supports>.
pub(crate) fn supports_condition_applies(prelude: &str) -> bool {
    supports_condition_applies_with_selector_parser(prelude, &ReasySelectorParser::default())
}

fn supports_condition_applies_with_selector_parser(
    prelude: &str,
    selector_parser: &ReasySelectorParser,
) -> bool {
    let condition = strip_enclosing_parentheses(prelude.trim());
    if condition.is_empty() {
        return false;
    }
    if let Some(rest) = strip_ascii_word_prefix(condition, "not") {
        return !supports_condition_applies_with_selector_parser(rest, selector_parser);
    }
    let or_parts = split_top_level_keyword(condition, "or");
    if or_parts.len() > 1 {
        return or_parts
            .into_iter()
            .any(|part| supports_condition_applies_with_selector_parser(part, selector_parser));
    }
    let and_parts = split_top_level_keyword(condition, "and");
    if and_parts.len() > 1 {
        return and_parts
            .into_iter()
            .all(|part| supports_condition_applies_with_selector_parser(part, selector_parser));
    }
    supports_selector_condition(condition, selector_parser)
        || supports_declaration_condition(condition)
}

/// Evaluates CSS Conditional `@supports selector(...)` with the selector parser.
///
/// Conditional Rules defines selector feature queries as true when the selector
/// argument parses as a supported selector; unsupported selectors evaluate
/// false and keep the block out of the cascade:
/// <https://www.w3.org/TR/css-conditional-4/#typedef-supports-selector-fn>.
fn supports_selector_condition(condition: &str, selector_parser: &ReasySelectorParser) -> bool {
    let condition = condition.trim();
    let Some(rest) = strip_ascii_word_prefix(condition, "selector") else {
        return false;
    };
    let Some((selector, after_selector)) = parse_parenthesized_selector(rest) else {
        return false;
    };
    if !after_selector.trim().is_empty() {
        return false;
    }
    parse_scope_selector(selector, selector_parser).is_some()
}

fn supports_declaration_condition(condition: &str) -> bool {
    let declaration = strip_enclosing_parentheses(condition.trim());
    let Some((name, value)) = declaration.split_once(':') else {
        return false;
    };
    let name = name.trim().to_ascii_lowercase();
    let value = trim_css_value(value);
    if name.is_empty() || value.is_empty() {
        return false;
    }
    match name.as_str() {
        "display" => supports_display_value(value),
        "direction" => matches!(value.to_ascii_lowercase().as_str(), "ltr" | "rtl"),
        "unicode-bidi" => matches!(
            value.to_ascii_lowercase().as_str(),
            "normal" | "embed" | "isolate" | "bidi-override" | "isolate-override" | "plaintext"
        ),
        "writing-mode" => matches!(
            value.to_ascii_lowercase().as_str(),
            "horizontal-tb" | "vertical-rl" | "vertical-lr"
        ),
        "text-orientation" => matches!(
            value.to_ascii_lowercase().as_str(),
            "mixed" | "upright" | "sideways"
        ),
        "text-align" => supports_text_align_value(value),
        "text-align-all" => supports_text_align_all_value(value),
        "text-align-last" => supports_text_align_last_value(value),
        "text-autospace" => supports_text_autospace_value(value),
        "text-transform" => supports_text_transform_value(value),
        "tab-size" => parse_tab_size(value, 12.0).is_some(),
        "text-decoration" => supports_text_decoration_value(value),
        "text-decoration-line" => supports_text_decoration_line_value(value),
        "text-decoration-style" => supports_text_decoration_style_value(value),
        "text-decoration-color" => parse_color(value).is_some(),
        "text-decoration-thickness" => supports_text_decoration_thickness_value(value),
        "text-decoration-inset" => supports_text_decoration_inset_value(value),
        "text-decoration-skip" => matches!(
            trim_css_value(value).to_ascii_lowercase().as_str(),
            "auto" | "none"
        ),
        "text-decoration-skip-ink" => matches!(
            trim_css_value(value).to_ascii_lowercase().as_str(),
            "auto" | "all" | "none"
        ),
        "text-decoration-skip-self" => supports_text_decoration_skip_self_value(value),
        "text-decoration-skip-box" => matches!(
            trim_css_value(value).to_ascii_lowercase().as_str(),
            "none" | "all"
        ),
        "text-decoration-skip-spaces" => supports_text_decoration_skip_spaces_value(value),
        "text-underline-offset" => {
            value.eq_ignore_ascii_case("auto") || parse_length(value).is_some()
        }
        "text-underline-position" => supports_text_underline_position_value(value),
        "text-emphasis-style" => supports_text_emphasis_style_value(value),
        "text-emphasis" => supports_text_emphasis_value(value),
        "text-emphasis-color" => parse_color(value).is_some(),
        "text-emphasis-position" => supports_text_emphasis_position_value(value),
        "text-emphasis-skip" => supports_text_emphasis_skip_value(value),
        "text-shadow" => supports_text_shadow_value(value),
        "box-shadow" => supports_box_shadow_value(value),
        "letter-spacing" => {
            value.eq_ignore_ascii_case("normal") || parse_letter_spacing(value, 12.0).is_some()
        }
        "word-spacing" => {
            value.eq_ignore_ascii_case("normal") || parse_word_spacing(value, 12.0).is_some()
        }
        "font" => parse_font_shorthand(value, 12.0, FontWeight::NORMAL).is_some(),
        "font-feature-settings" => parse_font_feature_settings(value).is_some(),
        "font-kerning" => parse_font_kerning(value).is_some(),
        "font-size-adjust" => parse_font_size_adjust(value).is_some(),
        "font-variant" => parse_font_variant(value).is_some(),
        "font-variant-ligatures" => parse_font_variant_ligatures(value).is_some(),
        "font-variant-position" => parse_font_variant_position(value).is_some(),
        "font-variant-caps" => parse_font_variant_caps(value).is_some(),
        "font-variant-numeric" => parse_font_variant_numeric(value).is_some(),
        "font-variant-alternates" => parse_font_variant_alternates(value).is_some(),
        "font-variant-east-asian" => parse_font_variant_east_asian(value).is_some(),
        "font-variant-emoji" => parse_font_variant_emoji(value).is_some(),
        "text-indent" => parse_text_indent(value, 12.0).is_some(),
        "hanging-punctuation" => parse_hanging_punctuation(value).is_some(),
        "vertical-align" => parse_vertical_align(value, 12.0).is_some(),
        "dominant-baseline" => parse_dominant_baseline(value).is_some(),
        "alignment-baseline" => parse_alignment_baseline(value).is_some(),
        "baseline-source" => parse_baseline_source(value).is_some(),
        "baseline-shift" => parse_baseline_shift(value, 12.0).is_some(),
        "margin-block" | "margin-inline" => supports_box_edge_axis_value(value, true),
        "padding-block" | "padding-inline" => supports_box_edge_axis_value(value, false),
        "color"
        | "background-color"
        | "background-origin"
        | "background-clip"
        | "border-color"
        | "border-top-color"
        | "border-right-color"
        | "border-bottom-color"
        | "border-left-color"
        | "border-block-color"
        | "border-block-start-color"
        | "border-block-end-color"
        | "border-inline-color"
        | "border-inline-start-color"
        | "border-inline-end-color" => parse_color(value).is_some(),
        "font-size"
        | "width"
        | "height"
        | "inline-size"
        | "block-size"
        | "min-width"
        | "max-width"
        | "min-height"
        | "max-height"
        | "min-inline-size"
        | "max-inline-size"
        | "min-block-size"
        | "max-block-size"
        | "left"
        | "top"
        | "right"
        | "bottom"
        | "margin"
        | "margin-top"
        | "margin-right"
        | "margin-bottom"
        | "margin-left"
        | "margin-block-start"
        | "margin-block-end"
        | "margin-inline-start"
        | "margin-inline-end"
        | "padding"
        | "padding-top"
        | "padding-right"
        | "padding-bottom"
        | "padding-left"
        | "padding-block-start"
        | "padding-block-end"
        | "padding-inline-start"
        | "padding-inline-end"
        | "border-width"
        | "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width"
        | "border-block-width"
        | "border-block-start-width"
        | "border-block-end-width"
        | "border-inline-width"
        | "border-inline-start-width"
        | "border-inline-end-width"
        | "gap"
        | "row-gap"
        | "column-gap" => value.eq_ignore_ascii_case("auto") || parse_length(value).is_some(),
        "position" => matches!(
            value.to_ascii_lowercase().as_str(),
            "static" | "relative" | "absolute" | "fixed" | "sticky"
        ),
        "isolation" => matches!(value.to_ascii_lowercase().as_str(), "auto" | "isolate"),
        "mix-blend-mode" => matches!(
            value.to_ascii_lowercase().as_str(),
            "normal"
                | "multiply"
                | "screen"
                | "overlay"
                | "darken"
                | "lighten"
                | "color-dodge"
                | "color-burn"
                | "hard-light"
                | "soft-light"
                | "difference"
                | "exclusion"
                | "hue"
                | "saturation"
                | "color"
                | "luminosity"
        ),
        "filter" | "clip-path" | "mask" | "mask-image" | "will-change" => true,
        "contain" => {
            let value = value.to_ascii_lowercase();
            value == "none"
                || value == "strict"
                || value == "content"
                || value.split_whitespace().all(|token| {
                    matches!(token, "size" | "inline-size" | "layout" | "style" | "paint")
                })
        }
        "content-visibility" => matches!(
            value.to_ascii_lowercase().as_str(),
            "visible" | "auto" | "hidden"
        ),
        "float" => matches!(
            value.to_ascii_lowercase().as_str(),
            "left" | "right" | "inline-start" | "inline-end" | "none"
        ),
        "clear" => matches!(
            value.to_ascii_lowercase().as_str(),
            "left" | "right" | "both" | "inline-start" | "inline-end" | "none"
        ),
        _ => supported_property_name(&name),
    }
}

fn supports_text_transform_value(value: &str) -> bool {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return false;
    }
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("none") {
        return true;
    }

    let mut saw_case = false;
    let mut saw_full_width = false;
    let mut saw_full_size_kana = false;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "none" => return false,
            "uppercase" | "lowercase" | "capitalize" if !saw_case => saw_case = true,
            "full-width" if !saw_full_width => saw_full_width = true,
            "full-size-kana" if !saw_full_size_kana => saw_full_size_kana = true,
            _ => return false,
        }
    }
    saw_case || saw_full_width || saw_full_size_kana
}

fn supports_display_value(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "none"
            | "block"
            | "inline"
            | "inline-block"
            | "flex"
            | "inline-flex"
            | "table"
            | "inline-table"
            | "table-caption"
            | "table-column-group"
            | "table-column"
            | "table-header-group"
            | "table-footer-group"
            | "table-row-group"
            | "table-row"
            | "table-cell"
            | "list-item"
    ) {
        return true;
    }
    let display = parse_display(value, Display::NONE);
    display != Display::NONE || display.is_list_item()
}

fn supported_property_name(name: &str) -> bool {
    matches!(
        name,
        "direction"
            | "unicode-bidi"
            | "writing-mode"
            | "flex-direction"
            | "justify-content"
            | "justify-items"
            | "justify-self"
            | "align-content"
            | "align-items"
            | "align-self"
            | "place-content"
            | "place-items"
            | "place-self"
            | "flex-wrap"
            | "flex-flow"
            | "flex-grow"
            | "flex-shrink"
            | "flex-basis"
            | "flex"
            | "columns"
            | "column-count"
            | "column-width"
            | "margin-block"
            | "margin-block-start"
            | "margin-block-end"
            | "margin-inline"
            | "margin-inline-start"
            | "margin-inline-end"
            | "padding-block"
            | "padding-block-start"
            | "padding-block-end"
            | "padding-inline"
            | "padding-inline-start"
            | "padding-inline-end"
            | "border"
            | "border-top"
            | "border-right"
            | "border-bottom"
            | "border-left"
            | "border-block"
            | "border-block-start"
            | "border-block-end"
            | "border-inline"
            | "border-inline-start"
            | "border-inline-end"
            | "border-image"
            | "border-image-source"
            | "border-image-slice"
            | "border-image-width"
            | "border-image-outset"
            | "border-image-repeat"
            | "border-style"
            | "border-top-style"
            | "border-right-style"
            | "border-bottom-style"
            | "border-left-style"
            | "border-block-style"
            | "border-block-start-style"
            | "border-block-end-style"
            | "border-inline-style"
            | "border-inline-start-style"
            | "border-inline-end-style"
            | "border-block-color"
            | "border-block-start-color"
            | "border-block-end-color"
            | "border-inline-color"
            | "border-inline-start-color"
            | "border-inline-end-color"
            | "border-block-width"
            | "border-block-start-width"
            | "border-block-end-width"
            | "border-inline-width"
            | "border-inline-start-width"
            | "border-inline-end-width"
            | "border-start-start-radius"
            | "border-start-end-radius"
            | "border-end-start-radius"
            | "border-end-end-radius"
            | "corner"
            | "corner-shape"
            | "corner-top-left-shape"
            | "corner-top-right-shape"
            | "corner-bottom-right-shape"
            | "corner-bottom-left-shape"
            | "border-collapse"
            | "caption-side"
            | "table-layout"
            | "empty-cells"
            | "border-spacing"
            | "background"
            | "background-image"
            | "background-size"
            | "background-position"
            | "background-repeat"
            | "background-origin"
            | "background-clip"
            | "letter-spacing"
            | "line-height"
            | "box-sizing"
            | "z-index"
            | "isolation"
            | "mix-blend-mode"
            | "filter"
            | "clip-path"
            | "mask"
            | "mask-image"
            | "contain"
            | "content-visibility"
            | "will-change"
            | "text-align"
            | "text-align-all"
            | "text-align-last"
            | "text-justify"
            | "text-autospace"
            | "text-orientation"
            | "text-indent"
            | "hanging-punctuation"
            | "vertical-align"
            | "dominant-baseline"
            | "alignment-baseline"
            | "baseline-source"
            | "baseline-shift"
            | "font-weight"
            | "font-style"
            | "font-width"
            | "font-stretch"
            | "font-family"
            | "font"
            | "font-feature-settings"
            | "font-kerning"
            | "font-size-adjust"
            | "font-variant"
            | "font-variant-ligatures"
            | "font-variant-position"
            | "font-variant-caps"
            | "font-variant-numeric"
            | "font-variant-alternates"
            | "font-variant-east-asian"
            | "font-variant-emoji"
            | "bookmark-level"
            | "bookmark-label"
            | "bookmark-state"
            | "text-transform"
            | "tab-size"
            | "visibility"
            | "list-style"
            | "list-style-type"
            | "list-style-position"
            | "list-style-image"
            | "counter-reset"
            | "counter-increment"
            | "counter-set"
            | "string-set"
            | "page"
            | "break-before"
            | "break-after"
            | "break-inside"
            | "page-break-before"
            | "page-break-after"
            | "page-break-inside"
            | "orphans"
            | "widows"
            | "text-decoration"
            | "text-decoration-line"
            | "text-decoration-style"
            | "text-decoration-color"
            | "text-decoration-thickness"
            | "text-decoration-inset"
            | "text-decoration-skip"
            | "text-decoration-skip-ink"
            | "text-decoration-skip-self"
            | "text-decoration-skip-box"
            | "text-decoration-skip-spaces"
            | "text-underline-offset"
            | "text-underline-position"
            | "text-emphasis"
            | "text-emphasis-style"
            | "text-emphasis-color"
            | "text-emphasis-position"
            | "text-emphasis-skip"
            | "text-shadow"
            | "box-shadow"
            | "white-space"
            | "word-break"
            | "overflow"
            | "overflow-x"
            | "overflow-y"
            | "overflow-wrap"
            | "word-wrap"
            | "line-break"
            | "hyphens"
            | "word-spacing"
            | "hyphenate-limit-chars"
    )
}

/// Return whether a `text-align` declaration uses a supported keyword.
///
/// CSS Text Level 3 defines the grammar as `start | end | left | right |
/// center | justify | match-parent | justify-all`.
/// <https://www.w3.org/TR/css-text-3/#text-align-property>.
fn supports_text_align_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "start" | "end" | "left" | "right" | "center" | "justify" | "match-parent" | "justify-all"
    )
}

fn supports_text_align_all_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "start" | "end" | "left" | "right" | "center" | "justify" | "match-parent"
    )
}

/// Return whether a `text-align-last` declaration uses a supported keyword.
///
/// CSS Text Level 3 defines `text-align-last` separately from `text-align`;
/// it supports `justify` but not the `text-align`-only `justify-all` keyword:
/// <https://www.w3.org/TR/css-text-3/#text-align-last-property>.
fn supports_text_align_last_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "auto" | "start" | "end" | "left" | "right" | "center" | "justify" | "match-parent"
    )
}

/// Return whether a `text-autospace` declaration uses a supported keyword set.
///
/// CSS Text Level 4 defines this as a draft unordered keyword set. Support
/// mirrors the computed-value parser so `@supports` does not claim values that
/// the cascade later ignores:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>.
fn supports_text_autospace_value(value: &str) -> bool {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return false;
    }
    if tokens.len() == 1 {
        return matches!(
            tokens[0].to_ascii_lowercase().as_str(),
            "normal"
                | "auto"
                | "no-autospace"
                | "ideograph-alpha"
                | "ideograph-numeric"
                | "punctuation"
        );
    }
    let mut ideograph_alpha = false;
    let mut ideograph_numeric = false;
    let mut punctuation = false;
    let mut mode = false;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "normal" | "auto" | "no-autospace" => return false,
            "ideograph-alpha" if !ideograph_alpha => ideograph_alpha = true,
            "ideograph-numeric" if !ideograph_numeric => ideograph_numeric = true,
            "punctuation" if !punctuation => punctuation = true,
            "insert" | "replace" if !mode => mode = true,
            _ => return false,
        }
    }
    ideograph_alpha || ideograph_numeric || punctuation
}

fn supports_text_decoration_line_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("none") {
        return true;
    }
    let mut underline = false;
    let mut overline = false;
    let mut line_through = false;
    let mut blink = false;
    let mut spelling_error = false;
    let mut grammar_error = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "none" => return false,
            "underline" if !underline => underline = true,
            "overline" if !overline => overline = true,
            "line-through" if !line_through => line_through = true,
            "blink" if !blink => blink = true,
            "spelling-error" if !spelling_error => spelling_error = true,
            "grammar-error" if !grammar_error => grammar_error = true,
            _ => return false,
        }
    }
    underline || overline || line_through || blink || spelling_error || grammar_error
}

fn supports_text_decoration_style_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "solid" | "double" | "dotted" | "dashed" | "wavy"
    )
}

fn supports_text_decoration_thickness_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "auto" | "from-font" | "thin" | "medium" | "thick"
    ) || parse_length(value).is_some()
}

fn supports_text_decoration_inset_value(value: &str) -> bool {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return true;
    }
    let parts = split_css_component_values(value);
    matches!(parts.len(), 1 | 2) && parts.iter().all(|part| parse_length(part).is_some())
}

fn supports_text_decoration_skip_self_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    if parts.len() == 1 {
        return matches!(
            parts[0].to_ascii_lowercase().as_str(),
            "auto"
                | "skip-all"
                | "no-skip"
                | "skip-underline"
                | "skip-overline"
                | "skip-line-through"
        );
    }
    let mut underline = false;
    let mut overline = false;
    let mut line_through = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "skip-underline" if !underline => underline = true,
            "skip-overline" if !overline => overline = true,
            "skip-line-through" if !line_through => line_through = true,
            _ => return false,
        }
    }
    underline || overline || line_through
}

fn supports_text_decoration_skip_spaces_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    if parts.len() == 1 {
        return matches!(
            parts[0].to_ascii_lowercase().as_str(),
            "none" | "all" | "start" | "end"
        );
    }
    let mut start = false;
    let mut end = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "start" if !start => start = true,
            "end" if !end => end = true,
            _ => return false,
        }
    }
    start || end
}

fn supports_text_underline_position_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    let mut auto = false;
    let mut under = false;
    let mut side = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "auto" if !auto && !under => auto = true,
            "under" if !under && !auto => under = true,
            "left" | "right" if !side => side = true,
            _ => return false,
        }
    }
    auto || under || side
}

fn supports_text_decoration_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    let mut saw_line = false;
    let mut saw_style = false;
    let mut saw_color = false;
    let mut saw_thickness = false;
    for part in &parts {
        if supports_text_decoration_line_value(part) {
            if part.eq_ignore_ascii_case("none") && parts.len() > 1 {
                return false;
            }
            if saw_line {
                return false;
            }
            saw_line = true;
            continue;
        }
        if supports_text_decoration_style_value(part) {
            if saw_style {
                return false;
            }
            saw_style = true;
            continue;
        }
        if supports_text_decoration_thickness_value(part) {
            if saw_thickness {
                return false;
            }
            saw_thickness = true;
            continue;
        }
        if parse_color(part).is_some() {
            if saw_color {
                return false;
            }
            saw_color = true;
            continue;
        }
        return false;
    }
    saw_line || saw_style || saw_color || saw_thickness
}

fn supports_text_emphasis_style_value(value: &str) -> bool {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return true;
    }
    if parse_css_string_token(value).is_some_and(|(_, tail)| tail.trim().is_empty()) {
        return true;
    }
    let mut saw_fill = false;
    let mut saw_shape = false;
    for part in split_css_component_values(value) {
        match part.to_ascii_lowercase().as_str() {
            "filled" | "open" if !saw_fill => saw_fill = true,
            "dot" | "circle" | "double-circle" | "triangle" | "sesame" if !saw_shape => {
                saw_shape = true;
            }
            _ => return false,
        }
    }
    saw_fill || saw_shape
}

fn supports_text_emphasis_value(value: &str) -> bool {
    let mut saw_style = false;
    let mut saw_color = false;
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    for split_index in 0..=parts.len() {
        let style_part = parts[..split_index].join(" ");
        let color_part = parts[split_index..].join(" ");
        if (!style_part.is_empty() && supports_text_emphasis_style_value(&style_part))
            && (color_part.is_empty() || parse_color(&color_part).is_some())
        {
            saw_style = true;
            saw_color = !color_part.is_empty();
            break;
        }
        if (!color_part.is_empty() && supports_text_emphasis_style_value(&color_part))
            && (style_part.is_empty() || parse_color(&style_part).is_some())
        {
            saw_style = true;
            saw_color = !style_part.is_empty();
            break;
        }
    }
    saw_style || saw_color
}

fn supports_text_emphasis_position_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() || parts.len() > 2 {
        return false;
    }
    let mut saw_over_under = false;
    let mut saw_side = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "over" | "under" if !saw_over_under => saw_over_under = true,
            "right" | "left" if !saw_side => saw_side = true,
            _ => return false,
        }
    }
    true
}

fn supports_text_emphasis_skip_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    let mut spaces = false;
    let mut punctuation = false;
    let mut symbols = false;
    let mut narrow = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "spaces" if !spaces => spaces = true,
            "punctuation" if !punctuation => punctuation = true,
            "symbols" if !symbols => symbols = true,
            "narrow" if !narrow => narrow = true,
            _ => return false,
        }
    }
    true
}

fn supports_text_shadow_value(value: &str) -> bool {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return true;
    }
    split_css_args(value, ',')
        .into_iter()
        .all(supports_text_shadow_layer_value)
}

fn split_css_args(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            candidate if candidate == delimiter && depth == 0 => {
                let part = trim_css_value(&value[start..index]);
                if !part.is_empty() {
                    parts.push(part);
                }
                start = index + candidate.len_utf8();
            }
            _ => {}
        }
    }
    let part = trim_css_value(&value[start..]);
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

fn supports_text_shadow_layer_value(value: &str) -> bool {
    let mut color = false;
    let mut inset = false;
    let mut lengths = 0usize;
    for part in split_css_component_values(value) {
        if part.eq_ignore_ascii_case("inset") && !inset {
            inset = true;
            continue;
        }
        if !color && (part.eq_ignore_ascii_case("currentcolor") || parse_color(part).is_some()) {
            color = true;
            continue;
        }
        if parse_length(part).is_some() {
            lengths += 1;
            continue;
        }
        return false;
    }
    (2..=4).contains(&lengths)
}

fn supports_box_shadow_value(value: &str) -> bool {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return true;
    }
    split_css_args(value, ',')
        .into_iter()
        .all(supports_box_shadow_layer_value)
}

fn supports_box_shadow_layer_value(value: &str) -> bool {
    let mut color = false;
    let mut inset = false;
    let mut lengths = Vec::new();
    for part in split_css_component_values(value) {
        if part.eq_ignore_ascii_case("inset") && !inset {
            inset = true;
            continue;
        }
        if !color && (part.eq_ignore_ascii_case("currentcolor") || parse_color(part).is_some()) {
            color = true;
            continue;
        }
        if let Some(length) = parse_length(part) {
            lengths.push(length);
            continue;
        }
        return false;
    }
    (2..=4).contains(&lengths.len()) && !lengths.get(2).is_some_and(|blur| *blur < 0.0)
}

/// Returns whether a logical margin/padding axis value has valid arity.
///
/// CSS Logical Properties defines `margin-block`/`margin-inline` and
/// `padding-block`/`padding-inline` as one-or-two-value shorthands for their
/// logical start/end sides:
/// <https://www.w3.org/TR/css-logical-1/#box>.
fn supports_box_edge_axis_value(value: &str, allow_auto: bool) -> bool {
    let parts = split_css_component_values(trim_css_value(value));
    matches!(parts.len(), 1 | 2)
        && parts.iter().all(|part| {
            (allow_auto && part.eq_ignore_ascii_case("auto")) || parse_length(part).is_some()
        })
}

fn strip_enclosing_parentheses(value: &str) -> &str {
    let mut value = value.trim();
    while value.starts_with('(') && value.ends_with(')') && outer_parentheses_wrap(value) {
        value = value[1..value.len() - 1].trim();
    }
    value
}

fn outer_parentheses_wrap(value: &str) -> bool {
    let mut depth = 0usize;
    for (index, byte) in value.as_bytes().iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && index != value.len() - 1 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn strip_ascii_word_prefix<'a>(value: &'a str, word: &str) -> Option<&'a str> {
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

fn split_top_level_keyword<'a>(value: &'a str, keyword: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let bytes = value.as_bytes();
    let keyword_bytes = keyword.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0
                && ascii_keyword_at(bytes, index, keyword_bytes)
                && word_boundary_before(bytes, index)
                && word_boundary_after(bytes, index + keyword_bytes.len()) =>
            {
                parts.push(value[start..index].trim());
                start = index + keyword_bytes.len();
                index = start;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(value[start..].trim());
    parts
}

fn split_top_level_delimiter(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let delimiter = delimiter as u8;
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && *byte == delimiter => {
                parts.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}

fn ascii_keyword_at(bytes: &[u8], index: usize, keyword: &[u8]) -> bool {
    bytes
        .get(index..index + keyword.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(keyword))
}

fn word_boundary_before(bytes: &[u8], index: usize) -> bool {
    index == 0
        || bytes
            .get(index - 1)
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-' && *byte != b'_')
}

fn word_boundary_after(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index)
        .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-' && *byte != b'_')
}

pub(super) fn strip_pseudo_selector<'a>(selector: &'a str, pseudo: &str) -> Option<&'a str> {
    let trimmed = selector.trim();
    let double_colon = format!("::{pseudo}");
    let single_colon = format!(":{pseudo}");
    let base = trimmed
        .strip_suffix(&double_colon)
        .or_else(|| trimmed.strip_suffix(&single_colon))?
        .trim();
    (!base.is_empty()).then_some(base)
}

fn split_pseudo_element_rule(
    selector_text: &str,
    selector_parser: &ReasySelectorParser,
    declarations: &Declarations,
    specificity: u32,
    layer_name: Option<String>,
    scopes: Vec<ScopeRule>,
) -> Vec<ParsedCssRule> {
    // CSS Pseudo-Elements 4 pseudo rules are matched against their originating
    // elements, then applied in pseudo-specific cascade/layout paths.
    // <https://www.w3.org/TR/css-pseudo-4/#pseudo-elements>
    let pseudo_names = [
        (RoutedPseudoElement::Marker, "marker"),
        (RoutedPseudoElement::Before, "before"),
        (RoutedPseudoElement::After, "after"),
        (RoutedPseudoElement::FirstLine, "first-line"),
        (RoutedPseudoElement::FirstLetter, "first-letter"),
    ];
    let mut normal_selectors = Vec::new();
    let mut routed_selectors = Vec::new();
    for selector in split_selector_list(selector_text) {
        let mut routed = false;
        for (pseudo, name) in pseudo_names {
            if let Some(base) = strip_pseudo_selector(selector, name) {
                routed_selectors.push((pseudo, base.to_string()));
                routed = true;
                break;
            }
        }
        if !routed {
            normal_selectors.push(selector.trim().to_string());
        }
    }
    if routed_selectors.is_empty() {
        return Vec::new();
    }

    let mut rules = Vec::new();
    if !normal_selectors.is_empty() {
        let selector_text = normal_selectors.join(", ");
        if let Some(selector) = parse_selector_list_text(&selector_text, selector_parser) {
            rules.push(ParsedCssRule::Style(StyleRule {
                selector_text,
                selector,
                declarations: declarations.clone(),
                specificity,
                order: 0,
                layer_name: layer_name.clone(),
                scopes: scopes.clone(),
            }));
        }
    }
    for (pseudo, _name) in pseudo_names {
        let base_selectors = routed_selectors
            .iter()
            .filter_map(|(routed_pseudo, selector)| (*routed_pseudo == pseudo).then_some(selector))
            .cloned()
            .collect::<Vec<_>>();
        if base_selectors.is_empty() {
            continue;
        }
        let selector_text = base_selectors.join(", ");
        let Some(selector) = parse_selector_list_text(&selector_text, selector_parser) else {
            continue;
        };
        let rule = StyleRule {
            selector_text,
            selector,
            declarations: declarations.clone(),
            specificity,
            order: 0,
            layer_name: layer_name.clone(),
            scopes: scopes.clone(),
        };
        rules.push(match pseudo {
            RoutedPseudoElement::Marker => ParsedCssRule::Marker(rule),
            RoutedPseudoElement::Before => ParsedCssRule::Before(rule),
            RoutedPseudoElement::After => ParsedCssRule::After(rule),
            RoutedPseudoElement::FirstLine => ParsedCssRule::FirstLine(rule),
            RoutedPseudoElement::FirstLetter => ParsedCssRule::FirstLetter(rule),
        });
    }
    rules
}

fn parse_selector_list_text(
    selector_text: &str,
    selector_parser: &ReasySelectorParser,
) -> Option<SelectorList<ReasySelectorImpl>> {
    let mut input = ParserInput::new(selector_text);
    let mut parser = Parser::new(&mut input);
    SelectorList::parse(selector_parser, &mut parser, ParseRelative::No).ok()
}
