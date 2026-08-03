use super::at_rules::collect_container_style_rules;
use super::*;
use crate::css::FontPaletteDefinition;
use crate::css::PropertyRegistrationRule;

#[derive(Debug)]
pub(in crate::css) enum ParsedCssRule {
    Style(StyleRule),
    Marker(StyleRule),
    BeforeMarker(StyleRule),
    AfterMarker(StyleRule),
    Before(StyleRule),
    After(StyleRule),
    FootnoteCall(StyleRule),
    FootnoteMarker(StyleRule),
    FirstLine(StyleRule),
    FirstLetter(StyleRule),
    Container(ContainerRule),
    Keyframes(KeyframesRule),
    FontFace(CssFontFace),
    CounterStyle(CounterStyleRule),
    FontFeatureValues(FontFeatureValuesRule),
    FontPaletteValues(String, FontPaletteDefinition),
    Property(PropertyRegistrationRule),
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
    FootnoteCall,
    FootnoteMarker,
    FirstLine,
    FirstLetter,
}

pub(in crate::css) struct CssRuleParser<'a> {
    /// URLs are immutable stylesheet context. Nested rule parsers only live
    /// while their parent parser is parsing, so they can borrow this context
    /// instead of cloning it for every conditional grouping rule.
    pub(in crate::css) base_url: Option<&'a url::Url>,
    pub(in crate::css) root_url: Option<&'a url::Url>,
    pub(in crate::css) layers: SharedLayerRegistry,
    pub(in crate::css) namespaces: SharedNamespaceRegistry,
    pub(in crate::css) current_layer: Option<String>,
    pub(in crate::css) current_scopes: Vec<ScopeRule>,
    pub(in crate::css) media_environment: MediaEnvironment,
    /// `@namespace` is legal only in the stylesheet prelude, never in a
    /// conditional grouping rule.
    pub(in crate::css) namespace_prelude_open: bool,
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

impl<'a, 'i> cssparser::QualifiedRuleParser<'i> for CssRuleParser<'a> {
    type Prelude = (String, SelectorList<QuireSelectorImpl>);
    type QualifiedRule = ParsedCssRule;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        self.namespace_prelude_open = false;
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
            .map(|branch| branch.specificity())
            .max()
            .unwrap_or(0);
        let declarations = parse_declarations_from_parser(input, self.base_url, self.root_url);
        let routed_rules = split_pseudo_element_rule(
            &selector_text,
            &self.namespaces.borrow().selector_parser(),
            &declarations,
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

impl<'a, 'i> cssparser::AtRuleParser<'i> for CssRuleParser<'a> {
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
        if !name.eq_ignore_ascii_case("namespace")
            && !name.eq_ignore_ascii_case("import")
            && !name.eq_ignore_ascii_case("charset")
        {
            self.namespace_prelude_open = false;
        }
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
            Ok(AtRulePrelude::Namespace(
                self.namespace_prelude_open
                    .then(|| parse_namespace_prelude(&prelude))
                    .flatten(),
            ))
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
        } else if name.eq_ignore_ascii_case("font-face") {
            Ok(AtRulePrelude::FontFace)
        } else if name.eq_ignore_ascii_case("counter-style") {
            Ok(AtRulePrelude::CounterStyle(prelude))
        } else if name.eq_ignore_ascii_case("font-feature-values") {
            Ok(AtRulePrelude::FontFeatureValues(prelude))
        } else if name.eq_ignore_ascii_case("font-palette-values") {
            Ok(AtRulePrelude::FontPaletteValues(prelude))
        } else if name.eq_ignore_ascii_case("container") {
            Ok(AtRulePrelude::Container(prelude))
        } else if name.eq_ignore_ascii_case("property") {
            Ok(AtRulePrelude::Property(prelude))
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
            | AtRulePrelude::FontFace
            | AtRulePrelude::CounterStyle(_)
            | AtRulePrelude::FontFeatureValues(_)
            | AtRulePrelude::FontPaletteValues(_)
            | AtRulePrelude::Container(_)
            | AtRulePrelude::Property(_)
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
                    base_url: self.base_url,
                    root_url: self.root_url,
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: self.current_layer.clone(),
                    current_scopes: self.current_scopes.clone(),
                    media_environment: self.media_environment,
                    namespace_prelude_open: false,
                };
                let nested = StyleSheetParser::new(input, &mut parser)
                    .flatten()
                    .collect::<Vec<_>>();
                Ok(ParsedCssRule::Nested(nested))
            }
            AtRulePrelude::Supports(true) => {
                let mut parser = CssRuleParser {
                    base_url: self.base_url,
                    root_url: self.root_url,
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: self.current_layer.clone(),
                    current_scopes: self.current_scopes.clone(),
                    media_environment: self.media_environment,
                    namespace_prelude_open: false,
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
                    base_url: self.base_url,
                    root_url: self.root_url,
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: Some(layer_name),
                    current_scopes: self.current_scopes.clone(),
                    media_environment: self.media_environment,
                    namespace_prelude_open: false,
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
                    base_url: self.base_url,
                    root_url: self.root_url,
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: self.current_layer.clone(),
                    current_scopes,
                    media_environment: self.media_environment,
                    namespace_prelude_open: false,
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
            AtRulePrelude::FontFace => {
                let body = consume_remaining_input(input);
                Ok(parse_font_face_rule(&body, self.base_url, self.root_url)
                    .map(ParsedCssRule::FontFace)
                    .unwrap_or(ParsedCssRule::Ignored))
            }
            AtRulePrelude::CounterStyle(name) => {
                let body = consume_remaining_input(input);
                Ok(parse_counter_style_rule(&name, &body)
                    .map(ParsedCssRule::CounterStyle)
                    .unwrap_or(ParsedCssRule::Ignored))
            }
            AtRulePrelude::FontFeatureValues(prelude) => {
                let block = consume_remaining_input(input);
                Ok(ParsedCssRule::FontFeatureValues(FontFeatureValuesRule {
                    prelude,
                    block,
                    layer: self.current_layer.clone(),
                    order: 0,
                }))
            }
            AtRulePrelude::FontPaletteValues(prelude) => {
                let block = consume_remaining_input(input);
                Ok(parse_font_palette_rule(&prelude, &block)
                    .map(|(name, definition)| ParsedCssRule::FontPaletteValues(name, definition))
                    .unwrap_or(ParsedCssRule::Ignored))
            }
            AtRulePrelude::Container(prelude) => {
                let mut parser = CssRuleParser {
                    base_url: self.base_url,
                    root_url: self.root_url,
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: self.current_layer.clone(),
                    current_scopes: self.current_scopes.clone(),
                    media_environment: self.media_environment,
                    namespace_prelude_open: false,
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
            AtRulePrelude::Property(prelude) => {
                let block = consume_remaining_input(input);
                Ok(parse_property_rule(&prelude, &block)
                    .map(ParsedCssRule::Property)
                    .unwrap_or(ParsedCssRule::Ignored))
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
    FontFace,
    CounterStyle(String),
    FontFeatureValues(String),
    FontPaletteValues(String),
    Container(String),
    Property(String),
    Ignored,
}
