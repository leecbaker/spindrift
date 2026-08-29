use cssparser::{
    AtRuleParser, BasicParseErrorKind, DeclarationParser, Parser, ParserState, RuleBodyItemParser,
    RuleBodyParser, Token,
};

use super::*;
use crate::css::{
    LayerName, LayerSegment, PropertyRegistrationRule, RegisteredCustomProperty, ScopeRoot,
    StylesheetScopeAnchor,
};

pub(super) fn collect_container_style_rules(
    rule: ParsedCssRule,
    rules: &mut Vec<StyleRule>,
    nested_containers: &mut Vec<ContainerRule>,
) {
    match rule {
        ParsedCssRule::Style(rule) => rules.push(rule),
        ParsedCssRule::Nested(nested_rules) => {
            for rule in nested_rules {
                collect_container_style_rules(rule, rules, nested_containers);
            }
        }
        ParsedCssRule::Container(rule) => nested_containers.push(rule),
        ParsedCssRule::Marker(_)
        | ParsedCssRule::BeforeMarker(_)
        | ParsedCssRule::AfterMarker(_)
        | ParsedCssRule::Before(_)
        | ParsedCssRule::After(_)
        | ParsedCssRule::ScrollMarker(_)
        | ParsedCssRule::ScrollMarkerGroup(_)
        | ParsedCssRule::FootnoteCall(_)
        | ParsedCssRule::FootnoteMarker(_)
        | ParsedCssRule::FirstLine(_)
        | ParsedCssRule::FirstLetter(_)
        | ParsedCssRule::Keyframes(_)
        | ParsedCssRule::FontFace(_)
        | ParsedCssRule::CounterStyle(_)
        | ParsedCssRule::FontFeatureValues(_)
        | ParsedCssRule::FontPaletteValues(_, _)
        | ParsedCssRule::Property(_)
        | ParsedCssRule::Page(_)
        | ParsedCssRule::Ignored => {}
    }
}

/// Parses the comma-separated `<custom-property-name>#` prelude of an
/// `@property` rule. Names are decoded by CSS Syntax before registration.
/// <https://drafts.css-houdini.org/css-properties-values-api/#at-property-rule>
pub(in crate::css) fn parse_property_names<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<Vec<String>, cssparser::ParseError<'i, BasicParseErrorKind<'i>>> {
    input.parse_comma_separated(|input| {
        let name = input.expect_ident_cloned()?;
        input.expect_exhausted()?;
        if !crate::css::is_custom_property_name(&name) {
            return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid));
        }
        Ok(name.to_string())
    })
}

/// Parses Quire's supported `<color>` subset of an `@property` rule.
///
/// Descriptor boundaries and names are validated by CSS Syntax. Unknown or
/// malformed descriptors recover locally; the completed rule is emitted only
/// when its final modeled descriptors form a usable `<color>` registration.
pub(in crate::css) fn parse_property_rule<'i, 't>(
    names: Vec<String>,
    input: &mut Parser<'i, 't>,
) -> Option<PropertyRegistrationRule> {
    let mut parser = PropertyRuleBodyParser::default();
    for _ in RuleBodyParser::new(input, &mut parser) {}
    parser.finish(names)
}

#[derive(Default)]
struct PropertyRuleBodyParser {
    syntax: Option<String>,
    inherits: Option<bool>,
    initial_value: Option<String>,
}

impl PropertyRuleBodyParser {
    fn finish(self, names: Vec<String>) -> Option<PropertyRegistrationRule> {
        if self.syntax.as_deref()? != "<color>" {
            return None;
        }
        let initial_value = self.initial_value?;
        // `currentColor` and light-dark() are not computationally independent.
        if initial_value.to_ascii_lowercase().contains("currentcolor")
            || initial_value.to_ascii_lowercase().contains("light-dark(")
        {
            return None;
        }
        Some(PropertyRegistrationRule {
            names,
            registration: RegisteredCustomProperty {
                inherits: self.inherits?,
                initial_color: crate::css::values::parse_color(&initial_value)?,
            },
        })
    }
}

impl<'i> DeclarationParser<'i> for PropertyRuleBodyParser {
    type Declaration = ();
    type Error = BasicParseErrorKind<'i>;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _declaration_start: &ParserState,
    ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("syntax") {
            let syntax = input.expect_string_cloned()?;
            input.expect_exhausted()?;
            self.syntax = Some(syntax.to_string());
            return Ok(());
        }
        if name.eq_ignore_ascii_case("inherits") {
            let inherits = input.expect_ident_cloned()?;
            input.expect_exhausted()?;
            self.inherits = match inherits.to_ascii_lowercase().as_ref() {
                "true" => Some(true),
                "false" => Some(false),
                _ => return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid)),
            };
            return Ok(());
        }
        if name.eq_ignore_ascii_case("initial-value") {
            let start = input.position();
            let is_valid =
                crate::css::component_values::validate_component_value_list_from_parser(input);
            let value = input.slice_from(start).trim();
            if !is_valid {
                return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid));
            }
            self.initial_value = Some(value.to_string());
            return Ok(());
        }
        Err(input.new_custom_error(BasicParseErrorKind::AtRuleInvalid(name)))
    }
}

impl<'i> AtRuleParser<'i> for PropertyRuleBodyParser {
    type Prelude = ();
    type AtRule = ();
    type Error = BasicParseErrorKind<'i>;
}

impl<'i> cssparser::QualifiedRuleParser<'i> for PropertyRuleBodyParser {
    type Prelude = ();
    type QualifiedRule = ();
    type Error = BasicParseErrorKind<'i>;
}

impl<'i> RuleBodyItemParser<'i, (), BasicParseErrorKind<'i>> for PropertyRuleBodyParser {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

/// Parses one complete CSS Cascade `<layer-name>` from decoded tokens.
///
/// Delimiter dots are structural tokens: whitespace and comments around one
/// are invalid rather than silently becoming part of a byte-scanned name.
/// <https://www.w3.org/TR/css-cascade-5/#typedef-layer-name>
pub(in crate::css) fn parse_layer_name<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<LayerName, cssparser::ParseError<'i, BasicParseErrorKind<'i>>> {
    let first = input.expect_ident_cloned()?;
    if is_css_wide_keyword(&first) {
        return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid));
    }
    let mut segments = vec![LayerSegment::Named(first.to_string())];
    while !input.is_exhausted() {
        let token = input.next_including_whitespace_and_comments()?.clone();
        if !matches!(token, Token::Delim('.')) {
            return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid));
        }
        let Token::Ident(next) = input.next_including_whitespace_and_comments()?.clone() else {
            return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid));
        };
        if is_css_wide_keyword(&next) {
            return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid));
        }
        segments.push(LayerSegment::Named(next.to_string()));
    }
    Ok(LayerName(segments))
}

pub(in crate::css) fn parse_layer_name_list<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<Vec<LayerName>, cssparser::ParseError<'i, BasicParseErrorKind<'i>>> {
    input.parse_comma_separated(parse_layer_name)
}

pub(in crate::css) fn qualify_layer_name(parent: Option<&LayerName>, name: LayerName) -> LayerName {
    parent.map_or(name.clone(), |parent| parent.nested(name))
}

fn is_css_wide_keyword(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "initial" | "inherit" | "unset" | "revert" | "revert-layer"
    )
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
pub(in crate::css) fn parse_scope_prelude<'i, 't>(
    input: &mut Parser<'i, 't>,
    selector_parser: &QuireSelectorParser,
    owner: StylesheetScopeAnchor,
    nesting_parent: Option<&SelectorList<QuireSelectorImpl>>,
) -> Result<ScopeRule, cssparser::ParseError<'i, SelectorParseErrorKind<'i>>> {
    let root = if input
        .try_parse(|input| input.expect_parenthesis_block())
        .is_ok()
    {
        ScopeRoot::Explicit(input.parse_nested_block(|input| {
            parse_scope_selector_from_parser(input, selector_parser, nesting_parent)
        })?)
    } else {
        ScopeRoot::Owner(owner)
    };
    let limit = if input.is_exhausted() {
        None
    } else {
        let to = input.expect_ident_cloned()?;
        if !to.eq_ignore_ascii_case("to") {
            return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
        }
        input.expect_parenthesis_block()?;
        Some(input.parse_nested_block(|input| {
            let scope_parent = nesting_scope_limit_parent(selector_parser)?;
            parse_scope_selector_from_parser(input, selector_parser, Some(&scope_parent))
        })?)
    };
    input.expect_exhausted()?;
    Ok(ScopeRule { root, limit })
}

fn parse_scope_selector_from_parser<'i, 't>(
    input: &mut Parser<'i, 't>,
    selector_parser: &QuireSelectorParser,
    nesting_parent: Option<&SelectorList<QuireSelectorImpl>>,
) -> Result<SelectorList<QuireSelectorImpl>, cssparser::ParseError<'i, SelectorParseErrorKind<'i>>>
{
    let selector = SelectorList::parse(selector_parser, input, ParseRelative::No)?;
    input.expect_exhausted()?;
    let selector = if selector
        .slice()
        .iter()
        .any(|branch| branch.has_parent_selector())
    {
        selector.replace_parent_selector(
            nesting_parent
                .ok_or_else(|| input.new_custom_error(SelectorParseErrorKind::InvalidState))?,
        )
    } else {
        selector
    };
    if selector
        .slice()
        .iter()
        .any(|branch| branch.has_pseudo_element())
    {
        return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
    }
    Ok(selector)
}

/// In a nested `@scope` limit, `&` is a zero-specificity `:where(:scope)`.
/// Parsing that replacement as selectors keeps it in the same selector model
/// as ordinary scope boundaries.
fn nesting_scope_limit_parent<'i>(
    selector_parser: &QuireSelectorParser,
) -> Result<SelectorList<QuireSelectorImpl>, cssparser::ParseError<'i, SelectorParseErrorKind<'i>>>
{
    let mut input = ParserInput::new(":where(:scope)");
    let mut parser = Parser::new(&mut input);
    let selector = SelectorList::parse(selector_parser, &mut parser, ParseRelative::No)?;
    parser.expect_exhausted()?;
    Ok(selector)
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

/// Parses a tokenized `@keyframes` rule body.
///
/// A malformed selector list invalidates its complete keyframe block, but CSS
/// Syntax recovery continues with later blocks in the same rule:
/// <https://www.w3.org/TR/css-animations-1/#keyframes>
pub(in crate::css) fn parse_keyframes_rule<'i, 't>(
    name: KeyframesName,
    input: &mut Parser<'i, 't>,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<KeyframesRule> {
    let mut parser = KeyframesRuleBodyParser {
        steps: Vec::new(),
        base_url,
        root_url,
    };
    for _ in RuleBodyParser::new(input, &mut parser) {}
    (!parser.steps.is_empty()).then_some(KeyframesRule {
        name,
        steps: parser.steps,
    })
}

struct KeyframesRuleBodyParser<'a> {
    steps: Vec<KeyframeStep>,
    base_url: Option<&'a url::Url>,
    root_url: Option<&'a url::Url>,
}

impl<'i> AtRuleParser<'i> for KeyframesRuleBodyParser<'_> {
    type Prelude = ();
    type AtRule = ();
    type Error = BasicParseErrorKind<'i>;
}

impl<'i> DeclarationParser<'i> for KeyframesRuleBodyParser<'_> {
    type Declaration = ();
    type Error = BasicParseErrorKind<'i>;
}

impl<'i> cssparser::QualifiedRuleParser<'i> for KeyframesRuleBodyParser<'_> {
    type Prelude = Vec<f32>;
    type QualifiedRule = ();
    type Error = BasicParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        input.parse_comma_separated(parse_keyframe_selector)
    }

    fn parse_block<'t>(
        &mut self,
        selectors: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, cssparser::ParseError<'i, Self::Error>> {
        let declarations = parse_declarations_from_parser(input, self.base_url, self.root_url);
        self.steps
            .extend(selectors.into_iter().map(|offset| KeyframeStep {
                offset,
                declarations: declarations.clone(),
            }));
        Ok(())
    }
}

impl<'i> RuleBodyItemParser<'i, (), BasicParseErrorKind<'i>> for KeyframesRuleBodyParser<'_> {
    fn parse_declarations(&self) -> bool {
        false
    }

    fn parse_qualified(&self) -> bool {
        true
    }
}

fn parse_keyframe_selector<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<f32, cssparser::ParseError<'i, BasicParseErrorKind<'i>>> {
    let offset = match input.next()?.clone() {
        Token::Ident(name) if name.eq_ignore_ascii_case("from") => 0.0,
        Token::Ident(name) if name.eq_ignore_ascii_case("to") => 1.0,
        Token::Percentage { unit_value, .. } => unit_value,
        _ => return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid)),
    };
    input.expect_exhausted()?;
    if !offset.is_finite() || !(0.0..=1.0).contains(&offset) {
        return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid));
    }
    Ok(offset)
}
