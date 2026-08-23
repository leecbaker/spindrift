use cssparser::{AtRuleParser, DeclarationParser, ToCss};

use super::at_rules::collect_container_style_rules;
use super::*;
use crate::css::{
    FontPaletteDefinition, LayerName, LayerOrder, LayerSegment, PropertyRegistrationRule,
    StylesheetOrigin, StylesheetScopeAnchor,
};

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
    Page(ParsedPageRule),
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
    pub(in crate::css) current_layer: Option<LayerName>,
    pub(in crate::css) current_scopes: Vec<ScopeRule>,
    pub(in crate::css) selector_scope_anchor: StylesheetScopeAnchor,
    pub(in crate::css) scope_anchor: StylesheetScopeAnchor,
    pub(in crate::css) origin: StylesheetOrigin,
    pub(in crate::css) media_environment: MediaEnvironment,
    /// The nearest style rule while parsing a CSS Nesting block.  It is kept
    /// as parsed selectors so `&` replacement follows Selectors' `:is()`
    /// semantics instead of constructing selector strings.
    /// <https://drafts.csswg.org/css-nesting-1/#nest-selector>
    pub(in crate::css) nesting: Option<NestingContext>,
    /// `@namespace` is legal only in the stylesheet prelude, never in a
    /// conditional grouping rule.
    pub(in crate::css) namespace_prelude_open: bool,
}

#[derive(Clone)]
pub(in crate::css) struct NestingContext {
    selector_text: String,
    selector: SelectorList<QuireSelectorImpl>,
    /// Pseudo-element branches cannot be represented by CSS Nesting's `&`.
    /// Direct declarations retain the complete selector list above.
    nesting_parent: Option<SelectorList<QuireSelectorImpl>>,
}

impl NestingContext {
    fn new(selector_text: String, selector: SelectorList<QuireSelectorImpl>) -> Self {
        let branches = selector
            .slice()
            .iter()
            .filter(|branch| !branch.has_pseudo_element())
            .cloned()
            .collect::<Vec<_>>();
        let nesting_parent =
            (!branches.is_empty()).then(|| SelectorList::from_iter(branches.into_iter()));
        Self {
            selector_text,
            selector,
            nesting_parent,
        }
    }

    fn resolve(
        &self,
        relative: SelectorList<QuireSelectorImpl>,
    ) -> Option<(String, SelectorList<QuireSelectorImpl>)> {
        let selector = relative.replace_parent_selector(self.nesting_parent.as_ref()?);
        let mut selector_text = String::new();
        selector.to_css(&mut selector_text).ok()?;
        Some((selector_text, selector))
    }
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
    roots: Vec<LayerNode>,
    names: Vec<LayerName>,
}

#[derive(Debug)]
struct LayerNode {
    segment: LayerSegment,
    children: Vec<LayerNode>,
}

impl LayerRegistry {
    pub(in crate::css) fn new_shared() -> SharedLayerRegistry {
        Rc::new(RefCell::new(Self::default()))
    }

    pub(in crate::css) fn names(&self) -> Vec<LayerName> {
        self.names.clone()
    }

    pub(in crate::css) fn register(&mut self, name: &LayerName) {
        if name.is_empty() {
            return;
        }
        let mut children = &mut self.roots;
        for segment in &name.0 {
            let index = children
                .iter()
                .position(|node| node.segment == *segment)
                .unwrap_or_else(|| {
                    children.push(LayerNode {
                        segment: segment.clone(),
                        children: Vec::new(),
                    });
                    children.len() - 1
                });
            children = &mut children[index].children;
        }
        if !self.names.iter().any(|existing| existing == name) {
            self.names.push(name.clone());
        }
    }

    pub(in crate::css) fn anonymous_name(&mut self) -> LayerName {
        let name = LayerName::anonymous();
        self.register(&name);
        name
    }

    /// Maps a layer to the implicit final sublayer that owns declarations
    /// written directly in that layer.
    pub(in crate::css) fn order_for(&self, name: &LayerName) -> Option<LayerOrder> {
        let mut children = &self.roots;
        let mut order = Vec::with_capacity(name.0.len() + 1);
        for segment in &name.0 {
            let index = children.iter().position(|node| node.segment == *segment)?;
            order.push(index);
            children = &children[index].children;
        }
        order.push(children.len());
        Some(LayerOrder(order))
    }
}

impl<'a> CssRuleParser<'a> {
    fn with_nesting(&self, nesting: NestingContext) -> Self {
        Self {
            base_url: self.base_url,
            root_url: self.root_url,
            layers: Rc::clone(&self.layers),
            namespaces: Rc::clone(&self.namespaces),
            current_layer: self.current_layer.clone(),
            current_scopes: self.current_scopes.clone(),
            selector_scope_anchor: self.selector_scope_anchor,
            scope_anchor: self.scope_anchor,
            origin: self.origin,
            media_environment: self.media_environment,
            nesting: Some(nesting),
            namespace_prelude_open: false,
        }
    }

    fn style_rules(
        &self,
        selector_text: String,
        selector: SelectorList<QuireSelectorImpl>,
        declarations: Declarations,
    ) -> Vec<ParsedCssRule> {
        let routed_rules = split_pseudo_element_rule(
            &selector_text,
            &self
                .namespaces
                .borrow()
                .selector_parser()
                .with_parent_selector(),
            &declarations,
            self.current_layer.clone(),
            self.current_scopes.clone(),
            self.selector_scope_anchor,
        );
        if !routed_rules.is_empty() {
            return routed_rules;
        }
        let specificity = selector
            .slice()
            .iter()
            .map(|branch| branch.specificity())
            .max()
            .unwrap_or(0);
        vec![ParsedCssRule::Style(StyleRule {
            selector_text,
            selector,
            stylesheet_scope_anchor: self.selector_scope_anchor,
            declarations,
            specificity,
            order: 0,
            layer_name: self.current_layer.clone(),
            scopes: self.current_scopes.clone(),
        })]
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
        let parse_relative = if self.nesting.is_some() {
            ParseRelative::ForNesting
        } else if self.current_scopes.is_empty() {
            ParseRelative::No
        } else {
            ParseRelative::ForScope
        };
        // Outside a nested style rule CSS Nesting defines `&` as `:scope`.
        // Keep the selector crate's parent-selector component in that case;
        // matching supplies the stylesheet or `@scope` context later.
        // <https://drafts.csswg.org/css-nesting-1/#nest-selector>
        let selector_parser = self
            .namespaces
            .borrow()
            .selector_parser()
            .with_parent_selector();
        let initial_state = input.state();
        match SelectorList::parse(&selector_parser, input, parse_relative) {
            Ok(selector) => {
                if let Some(nesting) = &self.nesting {
                    nesting
                        .resolve(selector)
                        .ok_or_else(|| input.new_custom_error(SelectorParseErrorKind::InvalidState))
                } else {
                    let selector_text = input.slice_from(start).trim().to_string();
                    Ok((selector_text, selector))
                }
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
                let routing_selector = super::pseudo_elements::split_selector_list(&selector_text)
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
                if let Some(nesting) = &self.nesting {
                    nesting
                        .resolve(selector)
                        .ok_or_else(|| input.new_custom_error(SelectorParseErrorKind::InvalidState))
                } else {
                    Ok((selector_text, selector))
                }
            }
        }
    }

    fn parse_block<'t>(
        &mut self,
        (selector_text, selector): Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, cssparser::ParseError<'i, Self::Error>> {
        let nesting = NestingContext::new(selector_text, selector);
        let mut parser = self.with_nesting(nesting);
        let nested = parse_nested_style_block(input, &mut parser);
        Ok(ParsedCssRule::Nested(nested))
    }
}

enum NestedStyleItem {
    Declaration((String, String)),
    Rule(Box<ParsedCssRule>),
}

/// Parses the block contents allowed by CSS Nesting.  `RuleBodyParser` keeps
/// declarations, qualified rules, and at-rules at CSS component-value
/// boundaries, so braces or delimiters inside values cannot become rules.
/// <https://drafts.csswg.org/css-nesting-1/#style-rules>
struct NestedStyleBodyParser<'a, 'p> {
    parser: &'p mut CssRuleParser<'a>,
}

impl<'a, 'p, 'i> AtRuleParser<'i> for NestedStyleBodyParser<'a, 'p> {
    type Prelude = AtRulePrelude;
    type AtRule = NestedStyleItem;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        // CSS Nesting permits only group rules in a style rule.  Consume the
        // remaining prelude before ignoring another at-rule, so recovery is
        // confined to that rule.
        if !name.eq_ignore_ascii_case("media")
            && !name.eq_ignore_ascii_case("supports")
            && !name.eq_ignore_ascii_case("container")
            && !name.eq_ignore_ascii_case("layer")
            && !name.eq_ignore_ascii_case("scope")
        {
            consume_remaining_input(input);
            return Ok(AtRulePrelude::Ignored);
        }
        <CssRuleParser<'a> as AtRuleParser<'i>>::parse_prelude(self.parser, name, input)
    }

    fn rule_without_block(
        &mut self,
        prelude: Self::Prelude,
        start: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        <CssRuleParser<'a> as AtRuleParser<'i>>::rule_without_block(self.parser, prelude, start)
            .map(|rule| NestedStyleItem::Rule(Box::new(rule)))
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, cssparser::ParseError<'i, Self::Error>> {
        <CssRuleParser<'a> as AtRuleParser<'i>>::parse_block(self.parser, prelude, start, input)
            .map(|rule| NestedStyleItem::Rule(Box::new(rule)))
    }
}

impl<'a, 'p, 'i> cssparser::QualifiedRuleParser<'i> for NestedStyleBodyParser<'a, 'p> {
    type Prelude = (String, SelectorList<QuireSelectorImpl>);
    type QualifiedRule = NestedStyleItem;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        <CssRuleParser<'a> as cssparser::QualifiedRuleParser<'i>>::parse_prelude(self.parser, input)
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, cssparser::ParseError<'i, Self::Error>> {
        <CssRuleParser<'a> as cssparser::QualifiedRuleParser<'i>>::parse_block(
            self.parser,
            prelude,
            start,
            input,
        )
        .map(|rule| NestedStyleItem::Rule(Box::new(rule)))
    }
}

impl<'a, 'p, 'i> DeclarationParser<'i> for NestedStyleBodyParser<'a, 'p> {
    type Declaration = NestedStyleItem;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _declaration_start: &ParserState,
    ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
        crate::css::parse::parse_nested_declaration_value(name, input)
            .map(NestedStyleItem::Declaration)
            .ok_or_else(|| input.new_custom_error(SelectorParseErrorKind::InvalidState))
    }
}

impl<'a, 'p, 'i> RuleBodyItemParser<'i, NestedStyleItem, SelectorParseErrorKind<'i>>
    for NestedStyleBodyParser<'a, 'p>
{
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        true
    }
}

fn parse_nested_style_block<'a, 'i, 't>(
    input: &mut Parser<'i, 't>,
    parser: &mut CssRuleParser<'a>,
) -> Vec<ParsedCssRule> {
    let context = parser
        .nesting
        .clone()
        .expect("nested style parser has a parent selector");
    let mut declarations = Declarations::new().with_urls(parser.base_url, parser.root_url);
    let mut rules = Vec::new();
    let items = {
        let mut body_parser = NestedStyleBodyParser { parser };
        RuleBodyParser::new(input, &mut body_parser)
            .flatten()
            .collect::<Vec<_>>()
    };
    for item in items {
        match item {
            NestedStyleItem::Declaration(declaration) => {
                declarations.extend(std::iter::once(declaration).collect());
            }
            NestedStyleItem::Rule(rule) => {
                if !declarations.is_empty() {
                    rules.extend(parser.style_rules(
                        context.selector_text.clone(),
                        context.selector.clone(),
                        std::mem::replace(
                            &mut declarations,
                            Declarations::new().with_urls(parser.base_url, parser.root_url),
                        ),
                    ));
                }
                rules.push(*rule);
            }
        }
    }
    if !declarations.is_empty() {
        rules.extend(parser.style_rules(context.selector_text, context.selector, declarations));
    }
    rules
}

fn parse_style_rule_list<'a, 'i, 't>(
    input: &mut Parser<'i, 't>,
    parser: &mut CssRuleParser<'a>,
) -> Vec<ParsedCssRule> {
    if parser.nesting.is_some() {
        parse_nested_style_block(input, parser)
    } else {
        StyleSheetParser::new(input, parser).flatten().collect()
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
        if name.eq_ignore_ascii_case("page") {
            self.namespace_prelude_open = false;
            return parse_page_selector_list(input)
                .map(AtRulePrelude::Page)
                .map_err(|_| input.new_custom_error(SelectorParseErrorKind::InvalidState));
        }
        if name.eq_ignore_ascii_case("keyframes") {
            self.namespace_prelude_open = false;
            return KeyframesName::parse(input)
                .map(AtRulePrelude::Keyframes)
                .map_err(|_| input.new_custom_error(SelectorParseErrorKind::InvalidState));
        }
        if name.eq_ignore_ascii_case("property") {
            self.namespace_prelude_open = false;
            return parse_property_names(input)
                .map(AtRulePrelude::Property)
                .map_err(|_| input.new_custom_error(SelectorParseErrorKind::InvalidState));
        }
        if name.eq_ignore_ascii_case("layer") {
            self.namespace_prelude_open = false;
            let names = if input.is_exhausted() {
                Vec::new()
            } else {
                parse_layer_name_list(input)
                    .map_err(|_| input.new_custom_error(SelectorParseErrorKind::InvalidState))?
            };
            return Ok(AtRulePrelude::Layer(names));
        }
        if name.eq_ignore_ascii_case("scope") {
            self.namespace_prelude_open = false;
            let selector_parser = self.namespaces.borrow().selector_parser();
            let nesting_parent = self
                .nesting
                .as_ref()
                .and_then(|context| context.nesting_parent.as_ref());
            let selector_parser = if nesting_parent.is_some() {
                selector_parser.with_parent_selector()
            } else {
                selector_parser
            };
            return parse_scope_prelude(input, &selector_parser, self.scope_anchor, nesting_parent)
                .map(AtRulePrelude::Scope)
                .map_err(|_| input.new_custom_error(SelectorParseErrorKind::InvalidState));
        }
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
        } else if name.eq_ignore_ascii_case("namespace") {
            Ok(AtRulePrelude::Namespace(
                self.namespace_prelude_open
                    .then(|| parse_namespace_prelude(&prelude))
                    .flatten(),
            ))
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
            AtRulePrelude::Layer(names) => {
                if names.is_empty() {
                    return Ok(ParsedCssRule::Ignored);
                }
                for name in names {
                    let name = qualify_layer_name(self.current_layer.as_ref(), name);
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
                    selector_scope_anchor: self.selector_scope_anchor,
                    scope_anchor: self.scope_anchor,
                    origin: self.origin,
                    media_environment: self.media_environment,
                    nesting: self.nesting.clone(),
                    namespace_prelude_open: false,
                };
                let nested = parse_style_rule_list(input, &mut parser);
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
                    selector_scope_anchor: self.selector_scope_anchor,
                    scope_anchor: self.scope_anchor,
                    origin: self.origin,
                    media_environment: self.media_environment,
                    nesting: self.nesting.clone(),
                    namespace_prelude_open: false,
                };
                let nested = parse_style_rule_list(input, &mut parser);
                Ok(ParsedCssRule::Nested(nested))
            }
            AtRulePrelude::Layer(names) => {
                let layer_name = if names.is_empty() {
                    self.layers.borrow_mut().anonymous_name()
                } else if names.len() != 1 {
                    consume_remaining_input(input);
                    return Ok(ParsedCssRule::Ignored);
                } else {
                    let name = qualify_layer_name(
                        self.current_layer.as_ref(),
                        names.into_iter().next().expect("one layer name"),
                    );
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
                    selector_scope_anchor: self.selector_scope_anchor,
                    scope_anchor: self.scope_anchor,
                    origin: self.origin,
                    media_environment: self.media_environment,
                    nesting: self.nesting.clone(),
                    namespace_prelude_open: false,
                };
                let nested = parse_style_rule_list(input, &mut parser);
                Ok(ParsedCssRule::Nested(nested))
            }
            AtRulePrelude::Scope(scope) => {
                let mut current_scopes = self.current_scopes.clone();
                current_scopes.push(scope);
                let mut parser = CssRuleParser {
                    base_url: self.base_url,
                    root_url: self.root_url,
                    layers: Rc::clone(&self.layers),
                    namespaces: Rc::clone(&self.namespaces),
                    current_layer: self.current_layer.clone(),
                    current_scopes,
                    selector_scope_anchor: self.selector_scope_anchor,
                    scope_anchor: self.scope_anchor,
                    origin: self.origin,
                    media_environment: self.media_environment,
                    nesting: self.nesting.clone(),
                    namespace_prelude_open: false,
                };
                let nested = parse_style_rule_list(input, &mut parser);
                Ok(ParsedCssRule::Nested(nested))
            }
            AtRulePrelude::Page(prelude) => {
                let mut parser = PageRuleBodyParser::new(self.base_url, self.root_url);
                for _ in RuleBodyParser::new(input, &mut parser) {}
                Ok(ParsedCssRule::Page(
                    parser.finish(prelude, self.current_layer.clone()),
                ))
            }
            AtRulePrelude::Keyframes(name) => {
                Ok(
                    parse_keyframes_rule(name, input, self.base_url, self.root_url)
                        .map(ParsedCssRule::Keyframes)
                        .unwrap_or(ParsedCssRule::Ignored),
                )
            }
            AtRulePrelude::FontFace => {
                let body = consume_remaining_input(input);
                Ok(parse_font_face_rule(&body, self.base_url, self.root_url)
                    .map(ParsedCssRule::FontFace)
                    .unwrap_or(ParsedCssRule::Ignored))
            }
            AtRulePrelude::CounterStyle(name) => {
                let body = consume_remaining_input(input);
                Ok(parse_counter_style_rule(&name, &body, self.origin)
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
                    selector_scope_anchor: self.selector_scope_anchor,
                    scope_anchor: self.scope_anchor,
                    origin: self.origin,
                    media_environment: self.media_environment,
                    nesting: self.nesting.clone(),
                    namespace_prelude_open: false,
                };
                let nested = parse_style_rule_list(input, &mut parser);
                let mut rules = Vec::new();
                let mut nested_containers = Vec::new();
                for rule in nested {
                    collect_container_style_rules(rule, &mut rules, &mut nested_containers);
                }
                Ok(ParsedCssRule::Container(ContainerRule {
                    prelude,
                    rules,
                    nested: nested_containers,
                }))
            }
            AtRulePrelude::Property(names) => Ok(parse_property_rule(names, input)
                .map(ParsedCssRule::Property)
                .unwrap_or(ParsedCssRule::Ignored)),
            AtRulePrelude::Media(false)
            | AtRulePrelude::Supports(false)
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
    Layer(Vec<LayerName>),
    Namespace(Option<(Option<String>, String)>),
    Scope(ScopeRule),
    Page(Vec<PageSelector>),
    Keyframes(KeyframesName),
    FontFace,
    CounterStyle(String),
    FontFeatureValues(String),
    FontPaletteValues(String),
    Container(String),
    Property(Vec<String>),
    Ignored,
}
