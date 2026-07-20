use super::*;

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
        let start = input.position();
        consume_remaining_input(input);
        let value = input.slice_from(start).trim().to_string();
        let name = if is_custom_property_name(&name) {
            name.to_string()
        } else {
            name.to_ascii_lowercase()
        };
        // CSSOM-compatible WebKit aliases participate in the same cascade
        // slot as their unprefixed longhand.  Canonicalizing before cascade
        // ordering preserves `inherit` and CSS-wide keyword semantics rather
        // than treating this as an unrelated property.
        // <https://compat.spec.whatwg.org/#propdef--webkit-flex-basis>
        let name = match name.as_str() {
            "-webkit-flex-basis" => "flex-basis".to_string(),
            _ => name,
        };
        // `var()` has a parse-time grammar of its own. A malformed reference
        // invalidates the declaration rather than becoming an
        // invalid-at-computed-value-time winner, so an earlier declaration may
        // still win the cascade.
        // <https://www.w3.org/TR/css-variables-1/#using-variables>
        let contains_variable_reference =
            crate::css::cascade::variables::contains_css_variable_reference(&value);
        if (is_custom_property_name(&name) || contains_variable_reference)
            && !custom_property_value_is_valid(&value)
        {
            return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid));
        }
        Ok((name, value))
    }
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
