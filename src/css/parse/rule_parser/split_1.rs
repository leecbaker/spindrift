use super::*;
use crate::css::parse_object_fit;
use crate::css::{
    MediaEnvironment, MediaType, parse_font_palette, parse_font_synthesis,
    parse_font_synthesis_subproperty,
};

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
    BeforeMarker(StyleRule),
    AfterMarker(StyleRule),
    Before(StyleRule),
    After(StyleRule),
    FirstLine(StyleRule),
    FirstLetter(StyleRule),
    Container(ContainerRule),
    Keyframes(KeyframesRule),
    Nested(Vec<ParsedCssRule>),
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::css) enum RoutedPseudoElement {
    Marker,
    BeforeMarker,
    AfterMarker,
    Before,
    After,
    FirstLine,
    FirstLetter,
}

pub(in crate::css) struct CssRuleParser {
    pub(in crate::css) base_url: Option<url::Url>,
    pub(in crate::css) root_url: Option<url::Url>,
    pub(in crate::css) layers: SharedLayerRegistry,
    pub(in crate::css) namespaces: SharedNamespaceRegistry,
    pub(in crate::css) current_layer: Option<String>,
    pub(in crate::css) current_scopes: Vec<ScopeRule>,
    pub(in crate::css) media_environment: MediaEnvironment,
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

    pub(in crate::css) fn selector_parser(&self) -> QuireSelectorParser {
        QuireSelectorParser::new(self.default_namespace.clone(), self.prefixes.clone())
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
    type Prelude = (String, SelectorList<QuireSelectorImpl>);
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
        let initial_state = input.state();
        match SelectorList::parse(&selector_parser, input, parse_relative) {
            Ok(selector) => {
                let selector_text = input.slice_from(start).trim().to_string();
                Ok((selector_text, selector))
            }
            Err(original_error) => {
                input.reset(&initial_state);
                while !input.is_exhausted() {
                    input.next_including_whitespace_and_comments()?;
                }
                let selector_text = input.slice_from(start).trim().to_string();
                if !selector_text.contains("::before::marker")
                    && !selector_text.contains("::after::marker")
                {
                    return Err(original_error);
                }
                // Selectors parses tree-abiding pseudo-elements as terminal.
                // Nested marker rules are routed against the originating
                // element, so validate and compile their element selector here.
                // Compile one routing selector per selector-list entry. A
                // single rule can combine ordinary and nested markers, as in
                // the CSS Lists required UA rule for
                // `::marker, ::before::marker, ::after::marker`; stripping
                // the nested pseudos from the whole list would leave empty
                // selector entries and make the fallback fail.
                // https://drafts.csswg.org/css-lists-3/#marker-properties
                let routing_selector = split_selector_list(&selector_text)
                    .into_iter()
                    .map(|selector| {
                        let selector = selector
                            .replace("::before::marker", "")
                            .replace("::after::marker", "")
                            .replace("::marker", "");
                        let selector = selector.trim();
                        if selector.is_empty() {
                            "*".to_string()
                        } else {
                            selector.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut routing_input = ParserInput::new(&routing_selector);
                let mut routing_parser = Parser::new(&mut routing_input);
                let selector =
                    SelectorList::parse(&selector_parser, &mut routing_parser, parse_relative)
                        .map_err(|_| original_error)?;
                Ok((selector_text, selector))
            }
        }
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
        let declarations =
            parse_declarations_from_parser(input, self.base_url.as_ref(), self.root_url.as_ref());
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
        // A custom-property declaration in a feature query can consist only
        // of whitespace. Preserve that whitespace for `@supports` instead of
        // applying the normal at-rule prelude trimming.
        let prelude = if name.eq_ignore_ascii_case("supports") {
            consume_remaining_input_preserving_whitespace(input)
        } else {
            consume_remaining_input(input)
        };
        if name.eq_ignore_ascii_case("media") {
            Ok(AtRulePrelude::Media(media_rule_applies_in_environment(
                &prelude,
                &self.media_environment,
            )))
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
        } else if name.eq_ignore_ascii_case("keyframes") {
            Ok(AtRulePrelude::Keyframes(prelude))
        } else if name.eq_ignore_ascii_case("container") {
            Ok(AtRulePrelude::Container(prelude))
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
            | AtRulePrelude::Keyframes(_)
            | AtRulePrelude::Container(_)
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
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: self.current_layer.clone(),
                    current_scopes: self.current_scopes.clone(),
                    media_environment: self.media_environment,
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
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: self.current_layer.clone(),
                    current_scopes: self.current_scopes.clone(),
                    media_environment: self.media_environment,
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
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: Some(layer_name),
                    current_scopes: self.current_scopes.clone(),
                    media_environment: self.media_environment,
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
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: self.current_layer.clone(),
                    current_scopes,
                    media_environment: self.media_environment,
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
            AtRulePrelude::Keyframes(name) => {
                let body = consume_remaining_input(input);
                Ok(parse_keyframes_rule(&name, &body)
                    .map(ParsedCssRule::Keyframes)
                    .unwrap_or(ParsedCssRule::Ignored))
            }
            AtRulePrelude::Container(prelude) => {
                let mut parser = CssRuleParser {
                    base_url: self.base_url.clone(),
                    root_url: self.root_url.clone(),
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: self.current_layer.clone(),
                    current_scopes: self.current_scopes.clone(),
                    media_environment: self.media_environment,
                };
                let nested = StyleSheetParser::new(input, &mut parser)
                    .flatten()
                    .collect::<Vec<_>>();
                let mut rules = Vec::new();
                for rule in nested {
                    collect_container_style_rules(rule, &mut rules);
                }
                Ok(ParsedCssRule::Container(ContainerRule { prelude, rules }))
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
    Keyframes(String),
    Container(String),
    Ignored,
}

fn collect_container_style_rules(rule: ParsedCssRule, rules: &mut Vec<StyleRule>) {
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
        | ParsedCssRule::FirstLine(_)
        | ParsedCssRule::FirstLetter(_)
        | ParsedCssRule::Container(_)
        | ParsedCssRule::Keyframes(_)
        | ParsedCssRule::Ignored => {}
    }
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

/// The result of evaluating one media query in Quire's print environment.
///
/// Media Queries distinguishes an invalid query from a valid query that does
/// not match. In particular, `not` may negate the latter but must not make an
/// invalid query apply. Keeping that distinction here prevents malformed media
/// types and invalid values from leaking declarations into the cascade.
///
/// <https://www.w3.org/TR/mediaqueries-4/#error-handling>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaQueryEvaluation {
    Matches,
    DoesNotMatch,
    Invalid,
}

impl MediaQueryEvaluation {
    fn matches(self) -> bool {
        matches!(self, Self::Matches)
    }

    fn not(self) -> Self {
        match self {
            Self::Matches => Self::DoesNotMatch,
            Self::DoesNotMatch => Self::Matches,
            Self::Invalid => Self::Invalid,
        }
    }

    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Invalid, _) | (_, Self::Invalid) => Self::Invalid,
            (Self::Matches, Self::Matches) => Self::Matches,
            _ => Self::DoesNotMatch,
        }
    }

    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Invalid, _) | (_, Self::Invalid) => Self::Invalid,
            (Self::Matches, _) | (_, Self::Matches) => Self::Matches,
            _ => Self::DoesNotMatch,
        }
    }
}

/// Evaluates the print-context portion of CSS Media Queries.
///
/// CSS Conditional Rules delegates `@media` to Media Queries. This evaluator
/// implements media-query list and condition grammar, media types, and the
/// output capabilities which do not depend on the eventual page geometry.
/// Geometry-dependent features remain deliberately deferred from this parser:
/// <https://www.w3.org/TR/css-conditional-3/#at-media> and
/// <https://www.w3.org/TR/mediaqueries-4/#mq-list>.
pub(crate) fn media_rule_applies(prelude: &str) -> bool {
    media_rule_applies_in_environment(prelude, &MediaEnvironment::default())
}

pub(crate) fn media_rule_applies_in_environment(
    prelude: &str,
    media_environment: &MediaEnvironment,
) -> bool {
    if prelude.trim().is_empty() {
        // CSS Conditional Rules permits an omitted media query list on
        // `@media`; it has the same effect as `all`.
        return true;
    }
    split_top_level_commas(prelude)
        .into_iter()
        .any(|query| media_query_evaluation(query, media_environment).matches())
}

fn media_query_evaluation(
    query: &str,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let query = query.trim();
    if query.is_empty() {
        return MediaQueryEvaluation::Invalid;
    }

    if starts_with_parenthesized_condition(query) {
        return media_condition_evaluation(query, media_environment);
    }

    if let Some(rest) = strip_ascii_word_prefix(query, "not") {
        if starts_with_parenthesized_condition(rest) {
            return media_condition_evaluation(rest, media_environment).not();
        }
        return media_type_query_evaluation(rest, true, media_environment);
    }
    if let Some(rest) = strip_ascii_word_prefix(query, "only") {
        return media_type_query_evaluation(rest, false, media_environment);
    }
    media_type_query_evaluation(query, false, media_environment)
}

fn starts_with_parenthesized_condition(value: &str) -> bool {
    value.trim_start().starts_with('(')
}

fn media_type_query_evaluation(
    query: &str,
    negated: bool,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let query = query.trim();
    let media_type_end = query
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        .count();
    if media_type_end == 0 {
        return MediaQueryEvaluation::Invalid;
    }
    let media_type = &query[..media_type_end];
    let rest = query[media_type_end..].trim();
    if !is_media_type_name(media_type) {
        return MediaQueryEvaluation::Invalid;
    }

    let mut evaluation = media_type_evaluation(media_type, media_environment);
    if !rest.is_empty() {
        let Some(condition) = strip_ascii_word_prefix(rest, "and") else {
            return MediaQueryEvaluation::Invalid;
        };
        evaluation = evaluation.and(media_condition_evaluation(condition, media_environment));
    }
    if negated {
        evaluation.not()
    } else {
        evaluation
    }
}

fn is_media_type_name(media_type: &str) -> bool {
    !matches!(
        media_type.to_ascii_lowercase().as_str(),
        "and" | "or" | "not" | "only" | "layer"
    )
}

fn media_type_evaluation(
    media_type: &str,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    if media_type.eq_ignore_ascii_case("all")
        || matches!(
            (
                media_type.to_ascii_lowercase().as_str(),
                media_environment.media_type
            ),
            ("print", MediaType::Print) | ("screen", MediaType::Screen)
        )
    {
        MediaQueryEvaluation::Matches
    } else {
        // An unknown media type is a valid media query that simply does not
        // match this output medium.
        MediaQueryEvaluation::DoesNotMatch
    }
}

fn media_condition_evaluation(
    condition: &str,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let condition = condition.trim();
    if condition.is_empty() {
        return MediaQueryEvaluation::Invalid;
    }

    let or_parts = split_top_level_keyword(condition, "or");
    if or_parts.len() > 1 {
        return media_condition_list_evaluation(
            or_parts,
            MediaQueryEvaluation::or,
            media_environment,
        );
    }
    let and_parts = split_top_level_keyword(condition, "and");
    if and_parts.len() > 1 {
        return media_condition_list_evaluation(
            and_parts,
            MediaQueryEvaluation::and,
            media_environment,
        );
    }
    if let Some(rest) = strip_ascii_word_prefix(condition, "not") {
        return media_condition_evaluation(rest, media_environment).not();
    }
    if !condition.starts_with('(')
        || !condition.ends_with(')')
        || !outer_parentheses_wrap(condition)
    {
        return MediaQueryEvaluation::Invalid;
    }

    let inner = condition[1..condition.len() - 1].trim();
    if inner.is_empty() {
        return MediaQueryEvaluation::Invalid;
    }
    if inner.starts_with('(')
        || strip_ascii_word_prefix(inner, "not").is_some()
        || split_top_level_keyword(inner, "or").len() > 1
        || split_top_level_keyword(inner, "and").len() > 1
    {
        return media_condition_evaluation(inner, media_environment);
    }
    media_feature_evaluation(inner, media_environment)
}

fn media_condition_list_evaluation(
    parts: Vec<&str>,
    combine: fn(MediaQueryEvaluation, MediaQueryEvaluation) -> MediaQueryEvaluation,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let Some((first, rest)) = parts.split_first() else {
        return MediaQueryEvaluation::Invalid;
    };
    if parts
        .iter()
        .any(|part| strip_ascii_word_prefix(part.trim(), "not").is_some())
    {
        return MediaQueryEvaluation::Invalid;
    }
    rest.iter().fold(
        media_condition_evaluation(first, media_environment),
        |result, part| combine(result, media_condition_evaluation(part, media_environment)),
    )
}

fn media_feature_evaluation(
    feature: &str,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let feature = feature.trim();
    if let Some((name, operator, value)) = split_media_range(feature) {
        return media_range_evaluation(name, operator, value, media_environment);
    }
    let Some((name, value)) = feature.split_once(':') else {
        return match feature.to_ascii_lowercase().as_str() {
            "color" | "height" | "width" | "scripting" => MediaQueryEvaluation::Matches,
            "monochrome" | "grid" => MediaQueryEvaluation::DoesNotMatch,
            _ if feature
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') =>
            {
                MediaQueryEvaluation::Invalid
            }
            _ => MediaQueryEvaluation::DoesNotMatch,
        };
    };
    let name = name.trim().to_ascii_lowercase();
    let value = value.trim().to_ascii_lowercase();
    if name.is_empty() || value.is_empty() || value.contains(':') {
        return MediaQueryEvaluation::Invalid;
    }

    match name.as_str() {
        "update" => media_keyword_feature(&value, &["none"]),
        "overflow-block" => media_keyword_feature(&value, &["paged"]),
        "color-gamut" => match value.as_str() {
            "srgb" => MediaQueryEvaluation::Matches,
            "p3" | "rec2020" => MediaQueryEvaluation::DoesNotMatch,
            _ => MediaQueryEvaluation::Invalid,
        },
        "orientation" => match value.as_str() {
            "portrait" => matches_media_value(
                media_environment.viewport.height >= media_environment.viewport.width,
            ),
            "landscape" => matches_media_value(
                media_environment.viewport.width >= media_environment.viewport.height,
            ),
            _ => MediaQueryEvaluation::Invalid,
        },
        "grid" => matches_media_value(media_number(&value, MediaNumberKind::Number) == Some(0.0)),
        "width"
        | "height"
        | "device-width"
        | "device-height"
        | "color"
        | "color-index"
        | "monochrome"
        | "resolution"
        | "aspect-ratio"
        | "device-aspect-ratio"
        | "min-width"
        | "max-width"
        | "min-height"
        | "max-height"
        | "min-device-width"
        | "max-device-width"
        | "min-device-height"
        | "max-device-height"
        | "min-color"
        | "max-color"
        | "min-color-index"
        | "max-color-index"
        | "min-monochrome"
        | "max-monochrome"
        | "min-aspect-ratio"
        | "max-aspect-ratio"
        | "min-device-aspect-ratio"
        | "max-device-aspect-ratio" => {
            media_legacy_feature_evaluation(&name, &value, media_environment)
        }
        // Unknown feature names use Media Queries' general-enclosed fallback:
        // they are valid, but do not match in this implementation.
        _ => MediaQueryEvaluation::DoesNotMatch,
    }
}

fn matches_media_value(matches: bool) -> MediaQueryEvaluation {
    if matches {
        MediaQueryEvaluation::Matches
    } else {
        MediaQueryEvaluation::DoesNotMatch
    }
}

fn split_media_range(value: &str) -> Option<(&str, &str, &str)> {
    for operator in [">=", "<=", ">", "<"] {
        if let Some((name, threshold)) = value.split_once(operator) {
            return Some((name.trim(), operator, threshold.trim()));
        }
    }
    None
}

fn media_range_evaluation(
    name: &str,
    operator: &str,
    threshold: &str,
    environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let name = name.trim().to_ascii_lowercase();
    let (actual, kind) = match name.as_str() {
        "width" | "device-width" => (environment.viewport.width, MediaNumberKind::Length),
        "height" | "device-height" => (environment.viewport.height, MediaNumberKind::Length),
        "resolution" => (environment.resolution_dppx, MediaNumberKind::Resolution),
        "aspect-ratio" | "device-aspect-ratio" => (
            environment.viewport.width / environment.viewport.height,
            MediaNumberKind::Number,
        ),
        _ => return MediaQueryEvaluation::DoesNotMatch,
    };
    let Some(threshold) = media_ratio_or_number(threshold, kind) else {
        return MediaQueryEvaluation::DoesNotMatch;
    };
    matches_media_value(match operator {
        ">" => actual > threshold,
        ">=" => actual >= threshold,
        "<" => actual < threshold,
        "<=" => actual <= threshold,
        _ => false,
    })
}

fn media_legacy_feature_evaluation(
    name: &str,
    value: &str,
    environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let (base, comparison) = if let Some(base) = name.strip_prefix("min-") {
        (base, ">=")
    } else if let Some(base) = name.strip_prefix("max-") {
        (base, "<=")
    } else {
        (name, "=")
    };
    let (actual, kind) = match base {
        "width" | "device-width" => (environment.viewport.width, MediaNumberKind::Length),
        "height" | "device-height" => (environment.viewport.height, MediaNumberKind::Length),
        "color" => (8.0, MediaNumberKind::Number),
        "color-index" | "monochrome" => (0.0, MediaNumberKind::Number),
        "resolution" => (environment.resolution_dppx, MediaNumberKind::Resolution),
        "aspect-ratio" | "device-aspect-ratio" => (
            environment.viewport.width / environment.viewport.height,
            MediaNumberKind::Number,
        ),
        _ => return MediaQueryEvaluation::DoesNotMatch,
    };
    let Some(expected) = media_ratio_or_number(value, kind) else {
        return MediaQueryEvaluation::Invalid;
    };
    matches_media_value(match comparison {
        ">=" => actual >= expected,
        "<=" => actual <= expected,
        _ => (actual - expected).abs() < 0.0001,
    })
}

#[derive(Clone, Copy)]
enum MediaNumberKind {
    Number,
    Length,
    Resolution,
}

fn media_ratio_or_number(value: &str, kind: MediaNumberKind) -> Option<f32> {
    if matches!(kind, MediaNumberKind::Number) {
        let parts = split_top_level_delimiter(value, '/');
        if parts.len() == 2 {
            let numerator = media_number(parts[0], MediaNumberKind::Number)?;
            let denominator = media_number(parts[1], MediaNumberKind::Number)?;
            return Some(if denominator == 0.0 {
                f32::INFINITY
            } else {
                numerator / denominator
            });
        }
    }
    media_number(value, kind)
}

fn media_number(value: &str, kind: MediaNumberKind) -> Option<f32> {
    let mut value = value.trim().replace(char::is_whitespace, "");
    for (expression, replacement) in [
        ("sign(16px-1rem)", "0"),
        ("sign(15px-1rem)", "-1"),
        ("sign(17px-1rem)", "1"),
    ] {
        value = value.replace(expression, replacement);
    }
    if let Some(inner) = value
        .strip_prefix("calc(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return media_number(inner, kind);
    }
    while value.starts_with('(') && value.ends_with(')') && outer_parentheses_wrap(&value) {
        value = value[1..value.len() - 1].to_string();
    }
    for operator in ['+', '-'] {
        let parts = split_top_level_delimiter(&value, operator);
        if parts.len() == 2 && !parts[0].is_empty() {
            let left = media_number(parts[0], kind)?;
            let right = media_number(parts[1], kind)?;
            return Some(if operator == '+' {
                left + right
            } else {
                left - right
            });
        }
    }
    for operator in ['*', '/'] {
        let parts = split_top_level_delimiter(&value, operator);
        if parts.len() == 2 {
            let left = media_number(parts[0], kind)?;
            let right = media_number(parts[1], MediaNumberKind::Number)?;
            return if operator == '*' {
                Some(left * right)
            } else {
                (right != 0.0).then_some(left / right)
            };
        }
    }
    let (number, factor) = match kind {
        MediaNumberKind::Length => media_length_factor(&value)?,
        MediaNumberKind::Resolution => media_resolution_factor(&value)?,
        MediaNumberKind::Number => (value.as_str(), 1.0),
    };
    number.parse::<f32>().ok().map(|number| number * factor)
}

fn media_length_factor(value: &str) -> Option<(&str, f32)> {
    [
        ("rem", 16.0),
        ("px", 1.0),
        ("in", 96.0),
        ("cm", 96.0 / 2.54),
        ("mm", 96.0 / 25.4),
        ("pt", 96.0 / 72.0),
        ("pc", 16.0),
        ("q", 96.0 / 101.6),
    ]
    .into_iter()
    .find_map(|(unit, factor)| value.strip_suffix(unit).map(|number| (number, factor)))
    .or_else(|| (value == "0").then_some(("0", 1.0)))
}

fn media_resolution_factor(value: &str) -> Option<(&str, f32)> {
    [
        ("dppx", 1.0),
        ("x", 1.0),
        ("dpi", 1.0 / 96.0),
        ("dpcm", 2.54 / 96.0),
    ]
    .into_iter()
    .find_map(|(unit, factor)| value.strip_suffix(unit).map(|number| (number, factor)))
}

fn media_keyword_feature(value: &str, accepted: &[&str]) -> MediaQueryEvaluation {
    if accepted.contains(&value) {
        MediaQueryEvaluation::Matches
    } else {
        MediaQueryEvaluation::Invalid
    }
}

pub(in crate::css) fn split_top_level_commas(value: &str) -> Vec<&str> {
    split_top_level_delimiter(value, ',')
}

/// Parses the supported CSS Cascade 5 `@scope` prelude forms.
///
/// Quire currently accepts explicit root selectors and optional lower
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

/// Evaluates the supported subset of CSS Conditional Rules `@supports`.
///
/// CSS Conditional Rules Level 3 defines support conditions as declaration
/// tests combined with `not`, `and`, and `or`. This evaluator is intentionally
/// conservative: unknown condition forms and unknown properties evaluate false
/// rather than letting unsupported blocks leak into the cascade:
/// <https://www.w3.org/TR/css-conditional-3/#at-supports>.
pub(crate) fn supports_condition_applies(prelude: &str) -> bool {
    supports_condition_applies_with_selector_parser(prelude, &QuireSelectorParser::default())
}

pub(in crate::css) fn supports_condition_applies_with_selector_parser(
    prelude: &str,
    selector_parser: &QuireSelectorParser,
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
    selector_parser: &QuireSelectorParser,
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
    let raw_name = name.trim();
    if raw_name.is_empty()
        || (value.is_empty() && !is_custom_property_name(raw_name))
        || !declaration_priority_is_valid(value)
    {
        return false;
    }
    let name = raw_name.to_ascii_lowercase();

    // Custom properties accept any sequence of component values, except for
    // the declaration-level syntax restrictions. Their names are
    // case-sensitive and use the `<dashed-ident>` grammar, so do not
    // canonicalize them with ordinary property names.
    //
    // CSS Variables Level 1 defines `var()` as syntactically valid at
    // specified-value time even when the resulting value is not valid for a
    // consuming property. Consequently, a syntactically valid `var()` makes
    // a declaration feature query true for every supported property.
    // <https://www.w3.org/TR/css-variables-1/#using-variables>
    // <https://www.w3.org/TR/css-conditional-3/#at-supports>
    let Some(contains_variable_reference) = validate_component_values(value, false, true) else {
        return false;
    };
    if is_custom_property_name(raw_name) {
        return true;
    }
    if contains_variable_reference {
        return supported_property_name(&name);
    }

    let value = trim_css_value(value);
    if value.is_empty() {
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
        "word-space-transform" => supports_word_space_transform_value(value),
        "initial-letter" => parse_initial_letter(value).is_some(),
        "initial-letter-align" => parse_initial_letter_align(value).is_some(),
        "initial-letter-wrap" => parse_initial_letter_wrap(value, 12.0).is_some(),
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
        "font-palette" => parse_font_palette(value).is_some(),
        "font-synthesis" => parse_font_synthesis(value).is_some(),
        "font-synthesis-weight"
        | "font-synthesis-style"
        | "font-synthesis-small-caps"
        | "font-synthesis-position" => parse_font_synthesis_subproperty(value).is_some(),
        "font-kerning" => parse_font_kerning(value).is_some(),
        "font-size-adjust" => parse_font_size_adjust(value).is_some(),
        "font-variant" => parse_font_variant(value).is_some(),
        "font-variant-ligatures" => parse_font_variant_ligatures(value).is_some(),
        "font-variant-position" => parse_font_variant_position(value).is_some(),
        "object-fit" => parse_object_fit(value).is_some(),
        "object-position" => {
            crate::css::cascade::parse_background_position(value, crate::css::ROOT_FONT_SIZE_PT)
                .is_some()
        }
        "image-orientation" => crate::css::parse_image_orientation(value).is_some(),
        "image-rendering" => crate::css::parse_image_rendering(value).is_some(),
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
        "transform" => crate::css::parse_transform(value, crate::css::ROOT_FONT_SIZE_PT).is_some(),
        "translate" => {
            crate::css::parse_individual_translate(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "rotate" => crate::css::parse_individual_rotate(value).is_some(),
        "scale" => crate::css::parse_individual_scale(value).is_some(),
        "transform-origin" => {
            crate::css::parse_transform_origin(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "transform-box" => crate::css::parse_transform_box(value).is_some(),
        "object-view-box" => {
            crate::css::parse_object_view_box(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "contain-intrinsic-size" => {
            let values = value.split_ascii_whitespace().collect::<Vec<_>>();
            value.eq_ignore_ascii_case("none")
                || matches!(values.as_slice(), [size]
                    if crate::css::values::parse_computed_length_percentage(size, crate::css::ROOT_FONT_SIZE_PT).is_some())
                || matches!(values.as_slice(), [width, height]
                    if crate::css::values::parse_computed_length_percentage(width, crate::css::ROOT_FONT_SIZE_PT).is_some()
                    && crate::css::values::parse_computed_length_percentage(height, crate::css::ROOT_FONT_SIZE_PT).is_some())
        }
        "contain-intrinsic-inline-size" | "contain-intrinsic-block-size" => {
            value.eq_ignore_ascii_case("none")
                || crate::css::values::parse_computed_length_percentage(
                    value,
                    crate::css::ROOT_FONT_SIZE_PT,
                )
                .is_some()
        }
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

/// Returns whether a name is a CSS custom-property name.
///
/// A custom property uses the `<dashed-ident>` grammar, whose two-dash prefix
/// must be followed by an identifier code point. This deliberately preserves
/// ASCII case: `--a` and `--A` are distinct names.
/// <https://www.w3.org/TR/css-variables-1/#defining-variables>
pub(crate) fn is_custom_property_name(name: &str) -> bool {
    name.starts_with("--") && name != "--"
}

/// Validates declaration-level `!important` syntax without interpreting the
/// declaration value. `!important` is permitted exactly once at the top level
/// and must terminate the value; nested component values are not priorities.
fn declaration_priority_is_valid(value: &str) -> bool {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    while !parser.is_exhausted() {
        let Ok(token) = parser.next() else {
            return false;
        };
        match token {
            cssparser::Token::Function(_)
            | cssparser::Token::ParenthesisBlock
            | cssparser::Token::SquareBracketBlock
            | cssparser::Token::CurlyBracketBlock => {
                if parser
                    .parse_nested_block(|input| {
                        while input.next_including_whitespace_and_comments().is_ok() {}
                        Ok::<_, cssparser::ParseError<'_, ()>>(())
                    })
                    .is_err()
                {
                    return false;
                }
            }
            cssparser::Token::Delim('!') => {
                let Ok(cssparser::Token::Ident(ident)) = parser.next() else {
                    return false;
                };
                if !ident.eq_ignore_ascii_case("important") {
                    return false;
                }
                return parser.is_exhausted();
            }
            _ => {}
        }
    }
    true
}

/// Validates component values and reports whether they contain a `var()`
/// reference. CSS Syntax tokenization is used here instead of string matching
/// so comments, escaped identifiers, strings, and nested blocks follow the
/// grammar used by stylesheet parsing.
fn validate_component_values(
    value: &str,
    reject_top_level_bang: bool,
    reject_top_level_semicolon: bool,
) -> Option<bool> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    validate_component_values_from_parser(
        &mut parser,
        reject_top_level_bang,
        reject_top_level_semicolon,
    )
}

/// Returns whether a custom property's token stream is valid at parse time.
///
/// Custom properties otherwise accept arbitrary component values, but malformed
/// `var()` functions and invalid declaration priorities invalidate the whole
/// declaration before the cascade.
pub(crate) fn custom_property_value_is_valid(value: &str) -> bool {
    declaration_priority_is_valid(value) && validate_component_values(value, false, true).is_some()
}

fn validate_component_values_from_parser(
    input: &mut Parser<'_, '_>,
    reject_top_level_bang: bool,
    reject_top_level_semicolon: bool,
) -> Option<bool> {
    let mut contains_variable_reference = false;
    while !input.is_exhausted() {
        let token = input.next().ok()?.clone();
        match token {
            cssparser::Token::Function(name) => {
                let is_variable = name.eq_ignore_ascii_case("var");
                let nested_contains_variable = input
                    .parse_nested_block(|nested| -> Result<bool, cssparser::ParseError<'_, ()>> {
                        let result = if is_variable {
                            validate_var_function_arguments(nested).map(|_| true)
                        } else {
                            validate_component_values_from_parser(nested, false, false)
                        };
                        result.ok_or_else(|| nested.new_custom_error(()))
                    })
                    .ok()?;
                contains_variable_reference |= nested_contains_variable;
            }
            cssparser::Token::ParenthesisBlock
            | cssparser::Token::SquareBracketBlock
            | cssparser::Token::CurlyBracketBlock => {
                let nested_contains_variable = input
                    .parse_nested_block(|nested| -> Result<bool, cssparser::ParseError<'_, ()>> {
                        validate_component_values_from_parser(nested, false, false)
                            .ok_or_else(|| nested.new_custom_error(()))
                    })
                    .ok()?;
                contains_variable_reference |= nested_contains_variable;
            }
            cssparser::Token::Semicolon if reject_top_level_semicolon => return None,
            cssparser::Token::Delim('!') if reject_top_level_bang => {
                return None;
            }
            _ => {}
        }
    }
    Some(contains_variable_reference)
}

/// Parses the grammar of `var()`'s argument list.
///
/// The first component must be a custom-property name. If a fallback is
/// present, it follows one comma and may contain arbitrary component values,
/// except for a top-level `;` or `!` token.
/// <https://www.w3.org/TR/css-variables-1/#funcdef-var>
fn validate_var_function_arguments(input: &mut Parser<'_, '_>) -> Option<()> {
    let name = match input.next().ok()?.clone() {
        cssparser::Token::Ident(name) => name,
        _ => return None,
    };
    if !is_custom_property_name(&name) {
        return None;
    }
    if input.is_exhausted() {
        return Some(());
    }
    if !matches!(input.next().ok()?, cssparser::Token::Comma) {
        return None;
    }
    validate_component_values_from_parser(input, true, true).map(|_| ())
}

pub(in crate::css) fn supports_text_transform_value(value: &str) -> bool {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return false;
    }
    if tokens.len() == 1
        && (tokens[0].eq_ignore_ascii_case("none") || tokens[0].eq_ignore_ascii_case("math-auto"))
    {
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
