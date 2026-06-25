use super::*;

pub(crate) fn parse_declarations(block: &str) -> Declarations {
    parse_declarations_with_urls(block, None, None)
}

pub(super) fn parse_declarations_with_urls(
    block: &str,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
) -> Declarations {
    let mut input = ParserInput::new(block);
    let mut parser = Parser::new(&mut input);
    parse_declarations_from_parser(&mut parser, base_url, root_url)
}

pub(super) fn parse_declarations_from_parser<'i, 't>(
    input: &mut Parser<'i, 't>,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
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
        Ok((name.to_ascii_lowercase().to_string(), value))
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
