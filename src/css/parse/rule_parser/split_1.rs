use super::*;

pub(in crate::css) fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

pub(in crate::css) fn find_matching_brace(source: &str, open: usize) -> Option<usize> {
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
pub(in crate::css) fn find_matching_brace_or_eof(source: &str, open: usize) -> Option<usize> {
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
pub(in crate::css) enum ParsedCssRule {
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
pub(in crate::css) enum RoutedPseudoElement {
    Marker,
    Before,
    After,
    FirstLine,
    FirstLetter,
}

pub(in crate::css) struct CssRuleParser {
    pub(in crate::css) base_url: Option<PathBuf>,
    pub(in crate::css) root_url: Option<PathBuf>,
    pub(in crate::css) layers: SharedLayerRegistry,
    pub(in crate::css) namespaces: SharedNamespaceRegistry,
    pub(in crate::css) current_layer: Option<String>,
    pub(in crate::css) current_scopes: Vec<ScopeRule>,
}

pub(in crate::css) type SharedLayerRegistry = Rc<RefCell<LayerRegistry>>;
pub(in crate::css) type SharedNamespaceRegistry = Rc<RefCell<NamespaceRegistry>>;

/// Namespace declarations in scope for selector parsing.
///
/// CSS Namespaces Level 3 lets `@namespace` declarations define a default
/// namespace and prefix mappings for selectors:
/// <https://www.w3.org/TR/css-namespaces-3/#declaration>.
#[derive(Debug, Default)]
pub(in crate::css) struct NamespaceRegistry {
    pub(in crate::css) default_namespace: Option<String>,
    pub(in crate::css) prefixes: HashMap<String, String>,
}

impl NamespaceRegistry {
    pub(in crate::css) fn new_shared() -> SharedNamespaceRegistry {
        Rc::new(RefCell::new(Self::default()))
    }

    pub(in crate::css) fn selector_parser(&self) -> ReasySelectorParser {
        ReasySelectorParser::new(self.default_namespace.clone(), self.prefixes.clone())
    }

    pub(in crate::css) fn register(&mut self, prefix: Option<String>, namespace_url: String) {
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
pub(in crate::css) struct LayerRegistry {
    pub(in crate::css) names: Vec<String>,
    pub(in crate::css) anonymous_count: usize,
}

impl LayerRegistry {
    pub(in crate::css) fn new_shared() -> SharedLayerRegistry {
        Rc::new(RefCell::new(Self::default()))
    }

    pub(in crate::css) fn names(&self) -> Vec<String> {
        self.names.clone()
    }

    pub(in crate::css) fn register(&mut self, name: &str) {
        if !self.names.iter().any(|existing| existing == name) {
            self.names.push(name.to_string());
        }
    }

    pub(in crate::css) fn anonymous_name(&mut self) -> String {
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

pub(in crate::css) enum AtRulePrelude {
    Media(bool),
    Supports(bool),
    Layer(String),
    Namespace(Option<(Option<String>, String)>),
    Scope(Option<ScopeRule>),
    Page(String),
    Ignored,
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

pub(in crate::css) fn media_query_applies(query: &str) -> bool {
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

pub(in crate::css) fn media_query_without_not_applies(query: &str) -> bool {
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

pub(in crate::css) fn media_type_applies(part: &str) -> bool {
    matches!(part.trim().to_ascii_lowercase().as_str(), "all" | "print")
}

pub(in crate::css) fn media_feature_applies(part: &str) -> bool {
    let feature = strip_enclosing_parentheses(part.trim());
    matches!(
        feature.to_ascii_lowercase().as_str(),
        "update: none" | "overflow-block: paged"
    )
}

pub(in crate::css) fn split_top_level_commas(value: &str) -> Vec<&str> {
    split_top_level_delimiter(value, ',')
}

/// Parses the supported CSS Cascade 5 `@scope` prelude forms.
///
/// Reasyprint currently accepts explicit root selectors and optional lower
/// boundaries, `@scope (<root>)` and `@scope (<root>) to (<limit>)`. Invalid
/// or unsupported preludes are ignored so their declarations do not enter the
/// cascade:
/// <https://www.w3.org/TR/css-cascade-5/#scope-atrule>.
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

pub(in crate::css) fn parse_scope_prelude(
    prelude: &str,
    selector_parser: &ReasySelectorParser,
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
    selector_parser: &ReasySelectorParser,
) -> Option<SelectorList<ReasySelectorImpl>> {
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

pub(in crate::css) fn supports_condition_applies_with_selector_parser(
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
pub(in crate::css) fn supports_selector_condition(
    condition: &str,
    selector_parser: &ReasySelectorParser,
) -> bool {
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

pub(in crate::css) fn supports_declaration_condition(condition: &str) -> bool {
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
            value.eq_ignore_ascii_case("auto")
                || parse_computed_length_percentage(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "text-underline-position" => supports_text_underline_position_value(value),
        "text-emphasis-style" => supports_text_emphasis_style_value(value),
        "text-emphasis" => supports_text_emphasis_value(value),
        "text-emphasis-color" => parse_color(value).is_some(),
        "text-emphasis-position" => supports_text_emphasis_position_value(value),
        "text-emphasis-skip" => supports_text_emphasis_skip_value(value),
        "text-shadow" => supports_text_shadow_value(value),
        "box-shadow" => supports_box_shadow_value(value),
        "border-spacing" => parse_border_spacing(value, crate::css::ROOT_FONT_SIZE_PT).is_some(),
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
        | "padding-inline-end" => {
            parse_computed_length_percentage(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "border-width" => supports_border_width_list(value, 4),
        "border-block-width" | "border-inline-width" => supports_border_width_list(value, 2),
        "outline-width" => supports_border_width_value(value),
        "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width"
        | "border-block-start-width"
        | "border-block-end-width"
        | "border-inline-start-width"
        | "border-inline-end-width" => supports_border_width_value(value),
        "gap" | "row-gap" | "column-gap" | "grid-gap" | "grid-row-gap" | "grid-column-gap" => {
            supports_gap_value(value)
        }
        "column-rule" | "row-rule" | "rule" => {
            parse_gap_rule_shorthand(value, crate::css::ROOT_FONT_SIZE_PT, Color::BLACK).is_some()
        }
        "column-rule-width" | "row-rule-width" | "rule-width" => {
            parse_gap_rule_width_list(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "column-rule-style" | "row-rule-style" | "rule-style" => {
            parse_gap_rule_style_list(value).is_some()
        }
        "column-rule-color" | "row-rule-color" | "rule-color" => {
            parse_gap_rule_color_list(value, Color::BLACK).is_some()
        }
        "column-rule-break" | "row-rule-break" | "rule-break" => {
            parse_gap_rule_break(value).is_some()
        }
        "column-rule-visibility-items" | "row-rule-visibility-items" | "rule-visibility-items" => {
            parse_gap_rule_visibility_items(value).is_some()
        }
        "rule-overlap" => parse_gap_rule_overlap(value).is_some(),
        "column-rule-inset"
        | "row-rule-inset"
        | "rule-inset"
        | "column-rule-inset-start"
        | "column-rule-inset-end"
        | "row-rule-inset-start"
        | "row-rule-inset-end"
        | "rule-inset-start"
        | "rule-inset-end"
        | "column-rule-inset-cap"
        | "column-rule-inset-junction"
        | "row-rule-inset-cap"
        | "row-rule-inset-junction"
        | "rule-inset-cap"
        | "rule-inset-junction" => supports_gap_rule_inset_shorthand(value),
        "column-rule-inset-cap-start"
        | "column-rule-inset-cap-end"
        | "column-rule-inset-junction-start"
        | "column-rule-inset-junction-end"
        | "row-rule-inset-cap-start"
        | "row-rule-inset-cap-end"
        | "row-rule-inset-junction-start"
        | "row-rule-inset-junction-end" => {
            parse_gap_rule_inset_value(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
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

pub(in crate::css) fn supports_text_transform_value(value: &str) -> bool {
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

pub(in crate::css) fn supports_border_width_value(value: &str) -> bool {
    parse_computed_border_width(trim_css_value(value), crate::css::ROOT_FONT_SIZE_PT).is_some()
}
