use super::*;

/// Finds the first CSS `url()` token anywhere in a value.
///
/// This is used for shorthands such as `background`, where `url()` can appear
/// after other component values. Tokenizing with `cssparser` avoids ad hoc
/// parenthesis and quote matching:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background>.
pub(crate) fn parse_first_css_url(value: &str) -> Option<String> {
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    while !parser.is_exhausted() {
        if let Ok(url) = parser.try_parse(|input| input.expect_url()) {
            return Some(url.to_string());
        }
        parser.next_including_whitespace_and_comments().ok()?;
    }
    None
}

pub(crate) fn parse_css_url_token(value: &str) -> Option<(String, &str)> {
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    let url = parser
        .try_parse(|input| input.expect_url())
        .ok()?
        .to_string();
    let position = parser.position().byte_index();
    Some((url, &value[position..]))
}
