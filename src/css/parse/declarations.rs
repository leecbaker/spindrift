use super::*;
use cssparser::Token;

pub(crate) fn parse_declarations(block: &str) -> Declarations {
    parse_declarations_with_urls(block, None, None)
}

pub(super) fn parse_declarations_with_urls(
    block: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Declarations {
    let mut input = ParserInput::new(block);
    let mut parser = Parser::new(&mut input);
    parse_declarations_from_parser(&mut parser, base_url, root_url)
}

pub(super) fn parse_declarations_from_parser<'i, 't>(
    input: &mut Parser<'i, 't>,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Declarations {
    let mut parser = DeclarationCollector;
    RuleBodyParser::new(input, &mut parser)
        .filter_map(Result::ok)
        .collect::<Declarations>()
        .with_urls(base_url, root_url)
}

pub(super) struct DeclarationCollector;

impl<'i> cssparser::DeclarationParser<'i> for DeclarationCollector {
    type Declaration = (String, String);
    type Error = BasicParseErrorKind<'i>;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _declaration_start: &ParserState,
    ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
        parse_declaration_value(name, input)
            .ok_or_else(|| input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid))
    }
}

/// Consumes and validates one declaration value for any CSS rule-body parser.
/// The caller supplies its parser-specific error type after this shared
/// token-validation boundary has rejected malformed input.
pub(in crate::css) fn parse_declaration_value<'i, 't>(
    name: CowRcStr<'i>,
    input: &mut Parser<'i, 't>,
) -> Option<(String, String)> {
    let start = input.position();
    let mut has_tokenizer_error = false;
    while let Ok(token) = input.next_including_whitespace_and_comments() {
        has_tokenizer_error |= token.is_parse_error();
    }
    if has_tokenizer_error {
        return None;
    }
    // Preserve the raw token stream through specified-value validation.
    // In particular, the newline terminating an unterminated string makes
    // it a CSS Syntax `BadString` token; trimming here would incorrectly
    // recover it as an EOF-closed string.
    let raw_value = input.slice_from(start);
    crate::css::component_values::CssComponentValueList::parse(raw_value)?;
    let name = if is_custom_property_name(&name) {
        name.to_string()
    } else {
        name.to_ascii_lowercase()
    };
    // CSSOM-compatible WebKit aliases participate in the same cascade slot as
    // their unprefixed longhand.
    let name = match name.as_str() {
        "-webkit-flex-basis" => "flex-basis".to_string(),
        _ => name,
    };
    let contains_variable_reference =
        crate::css::cascade::variables::contains_css_variable_reference(raw_value);
    if (is_custom_property_name(&name) || contains_variable_reference)
        && !custom_property_value_is_valid(raw_value)
    {
        return None;
    }
    Some((name, raw_value.trim().to_string()))
}

/// Parses a declaration in a nested style-rule body.
///
/// CSS Syntax's style-block algorithm must distinguish a declaration from a
/// nested qualified rule with an identifier and pseudo-class prelude, such as
/// `a:hover { ... }`. `cssparser`'s generic `RuleBodyParser` first offers that
/// form to `DeclarationParser`, so reject an ordinary declaration value that
/// contains a top-level curly block and let it retry as a qualified rule.
/// Custom-property values intentionally retain arbitrary component blocks.
/// <https://drafts.csswg.org/css-syntax-3/#consume-a-style-blocks-contents>
pub(in crate::css) fn parse_nested_declaration_value<'i, 't>(
    name: CowRcStr<'i>,
    input: &mut Parser<'i, 't>,
) -> Option<(String, String)> {
    // A known property may validly contain a curly component block, including
    // an unresolved `var()` reference. Keep it as a declaration so computed
    // value invalidation can suppress an earlier cascade winner. Unknown
    // `identifier: … {}` candidates remain available to the nested selector
    // parser (`a:hover { ... }`), which is the ambiguity CSS Nesting needs to
    // resolve at this boundary.
    let is_known_declaration = is_custom_property_name(&name)
        || crate::css::cascade::is_modeled_property_name(&name.to_ascii_lowercase());
    if !is_known_declaration && declaration_value_contains_curly_block(input) {
        return None;
    }
    parse_declaration_value(name, input)
}

fn declaration_value_contains_curly_block(input: &mut Parser<'_, '_>) -> bool {
    let state = input.state();
    let mut contains_curly_block = false;
    while let Ok(token) = input.next_including_whitespace_and_comments() {
        contains_curly_block |= matches!(token, Token::CurlyBracketBlock);
    }
    input.reset(&state);
    contains_curly_block
}

impl<'i> cssparser::AtRuleParser<'i> for DeclarationCollector {
    type Prelude = ();
    type AtRule = (String, String);
    type Error = BasicParseErrorKind<'i>;
}

impl<'i> cssparser::QualifiedRuleParser<'i> for DeclarationCollector {
    type Prelude = ();
    type QualifiedRule = (String, String);
    type Error = BasicParseErrorKind<'i>;
}

impl<'i> RuleBodyItemParser<'i, (String, String), BasicParseErrorKind<'i>>
    for DeclarationCollector
{
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

pub(super) fn consume_remaining_input<'i, 't>(input: &mut Parser<'i, 't>) -> String {
    let start = input.position();
    while input.next_including_whitespace_and_comments().is_ok() {}
    input.slice_from(start).trim().to_string()
}

/// Consumes an at-rule prelude without discarding significant whitespace.
///
/// Most at-rule preludes are whitespace-insensitive, but CSS Conditional Rules
/// declaration tests retain the distinction between an empty custom-property
/// value and a value containing a whitespace token.
pub(super) fn consume_remaining_input_preserving_whitespace<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> String {
    let start = input.position();
    while input.next_including_whitespace_and_comments().is_ok() {}
    input.slice_from(start).to_string()
}
