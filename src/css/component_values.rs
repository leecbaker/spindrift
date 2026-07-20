//! Token-aware helpers for CSS component-value boundaries.
//!
//! CSS Syntax tokenizes comments, escapes, strings, functions, and simple
//! blocks before a property grammar sees its components.  Keeping the source
//! slices returned here lets legacy value parsers retain their cheap borrowed
//! input while making their structural decisions from tokens instead of bytes.
//! <https://www.w3.org/TR/css-syntax-3/#component-value>

use cssparser::{Parser, ParserInput, SourcePosition, Token};

fn consume_nested_block(parser: &mut Parser<'_, '_>) -> bool {
    parser
        .parse_nested_block(|nested| {
            while let Ok(token) = nested.next_including_whitespace_and_comments() {
                if is_simple_block(token) && !consume_nested_block(nested) {
                    return Err(nested.new_custom_error(()));
                }
            }
            Ok::<_, cssparser::ParseError<'_, ()>>(())
        })
        .is_ok()
}

fn is_simple_block(token: &Token<'_>) -> bool {
    matches!(
        token,
        Token::Function(_)
            | Token::ParenthesisBlock
            | Token::SquareBracketBlock
            | Token::CurlyBracketBlock
    )
}

fn source_offset(parser: &Parser<'_, '_>, source_start: SourcePosition) -> usize {
    parser.slice_from(source_start).len()
}

/// Splits at CSS whitespace component-value boundaries.
pub(in crate::css) fn split_css_component_values(value: &str) -> Vec<&str> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut parts = Vec::new();
    let mut component_start = None;

    loop {
        let token_start = parser.position();
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => break,
        };
        if matches!(token, Token::WhiteSpace(_) | Token::Comment(_)) {
            if let Some(start) = component_start.take() {
                let part = parser.slice(start..token_start).trim();
                if !part.is_empty() {
                    parts.push(part);
                }
            }
            continue;
        }
        component_start.get_or_insert(token_start);
        if is_simple_block(&token) && !consume_nested_block(&mut parser) {
            break;
        }
    }
    if let Some(start) = component_start {
        let part = parser.slice_from(start).trim();
        if !part.is_empty() {
            parts.push(part);
        }
    }
    parts
}

/// Splits at an outermost CSS delimiter token.
pub(in crate::css) fn split_css_top_level_delimiter(value: &str, delimiter: char) -> Vec<&str> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut parts = Vec::new();
    let source_start = parser.position();
    let mut start = 0usize;
    loop {
        let token_start = source_offset(&parser, source_start);
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => break,
        };
        let is_delimiter = matches!(token, Token::Comma) && delimiter == ','
            || matches!(token, Token::Delim(found) if found == delimiter);
        if is_delimiter {
            parts.push(value[start..token_start].trim());
            start = source_offset(&parser, source_start);
        } else if is_simple_block(&token) && !consume_nested_block(&mut parser) {
            break;
        }
    }
    parts.push(value[start..].trim());
    parts
}

/// Splits once at an outermost CSS delimiter token.
pub(in crate::css) fn split_css_top_level_once(
    value: &str,
    delimiter: char,
) -> Option<(&str, &str)> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let source_start = parser.position();
    let start = 0usize;
    loop {
        let token_start = source_offset(&parser, source_start);
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        let is_delimiter = matches!(token, Token::Comma) && delimiter == ','
            || matches!(token, Token::Delim(found) if found == delimiter);
        if is_delimiter {
            return Some((
                value[start..token_start].trim(),
                &value[source_offset(&parser, source_start)..],
            ));
        }
        if is_simple_block(&token) && !consume_nested_block(&mut parser) {
            return None;
        }
    }
}

/// Returns the decoded function name and raw body of a single CSS function.
pub(in crate::css) fn css_single_function(value: &str) -> Option<(String, &str)> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let name = match parser.next().ok()? {
        Token::Function(name) => name.to_string(),
        _ => return None,
    };
    let body = parser
        .parse_nested_block(|nested| {
            let start = nested.position();
            while let Ok(token) = nested.next_including_whitespace_and_comments() {
                if is_simple_block(token) && !consume_nested_block(nested) {
                    return Err(nested.new_custom_error(()));
                }
            }
            Ok::<_, cssparser::ParseError<'_, ()>>(nested.slice_from(start))
        })
        .ok()?;
    parser.is_exhausted().then_some((name, body))
}

/// Returns a raw function body only when the decoded function name matches.
pub(in crate::css) fn css_function_body<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let (actual, body) = css_single_function(value)?;
    actual.eq_ignore_ascii_case(name).then_some(body)
}

/// Returns one decoded identifier when it is the complete component value.
pub(in crate::css) fn css_single_ident(value: &str) -> Option<String> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let ident = parser.expect_ident_cloned().ok()?.to_string();
    parser.is_exhausted().then_some(ident)
}

/// Splits at an outermost decoded identifier keyword.
pub(in crate::css) fn split_css_top_level_keyword<'a>(
    value: &'a str,
    keyword: &str,
) -> Vec<&'a str> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut parts = Vec::new();
    let source_start = parser.position();
    let mut start = 0usize;
    loop {
        let token_start = source_offset(&parser, source_start);
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => break,
        };
        if matches!(token, Token::Ident(ref name) if name.eq_ignore_ascii_case(keyword)) {
            parts.push(value[start..token_start].trim());
            start = source_offset(&parser, source_start);
        } else if is_simple_block(&token) && !consume_nested_block(&mut parser) {
            break;
        }
    }
    parts.push(value[start..].trim());
    parts
}

/// Finds the next outermost `{` token from `start` and returns its byte offset.
pub(in crate::css) fn find_next_top_level_open_brace(source: &str, start: usize) -> Option<usize> {
    let source = source.get(start..)?;
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let source_start = parser.position();
    loop {
        let token_start = source_offset(&parser, source_start);
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        if matches!(token, Token::CurlyBracketBlock) {
            return Some(start + token_start);
        }
        if is_simple_block(&token) && !consume_nested_block(&mut parser) {
            return None;
        }
    }
}

/// Finds the matching close for a `{` token using CSS tokenization.
pub(in crate::css) fn find_matching_brace(
    source: &str,
    open: usize,
    recover_eof: bool,
) -> Option<usize> {
    let suffix = source.get(open..)?;
    let mut input = ParserInput::new(suffix);
    let mut parser = Parser::new(&mut input);
    if !matches!(
        parser.next_including_whitespace_and_comments().ok()?,
        Token::CurlyBracketBlock
    ) {
        return None;
    }
    let body_len = parser
        .parse_nested_block(|nested| {
            let body_start = nested.position();
            while let Ok(token) = nested.next_including_whitespace_and_comments() {
                if is_simple_block(token) && !consume_nested_block(nested) {
                    return Err(nested.new_custom_error(()));
                }
            }
            Ok::<_, cssparser::ParseError<'_, ()>>(nested.slice_from(body_start).len())
        })
        .ok()?;
    let close = open + 1 + body_len;
    if source.as_bytes().get(close) == Some(&b'}') {
        Some(close)
    } else {
        recover_eof.then_some(source.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_boundaries_follow_css_tokens() {
        assert_eq!(
            split_css_component_values("solid /* separator */ rgb(1 2 3) 'x y'"),
            ["solid", "rgb(1 2 3)", "'x y'"]
        );
        assert_eq!(
            split_css_top_level_delimiter("a, func(1, 2), 'x,y'", ','),
            ["a", "func(1, 2)", "'x,y'"]
        );
        assert_eq!(
            split_css_top_level_once("rgb(1 / 2 / 3) / .5", '/'),
            Some(("rgb(1 / 2 / 3)", " .5"))
        );
    }

    #[test]
    fn identifiers_and_functions_are_decoded_before_matching() {
        assert_eq!(
            css_single_ident("\\74 ransparent"),
            Some("transparent".into())
        );
        assert_eq!(
            css_single_function("\\72 gb(1 2 3)"),
            Some(("rgb".into(), "1 2 3"))
        );
        assert_eq!(
            split_css_top_level_keyword("(a and b) \\61 nd c", "and"),
            ["(a and b)", "c"]
        );
    }

    #[test]
    fn block_scanning_ignores_braces_in_components_and_recovers_eof() {
        let source = "a { content: \"}\"; fn({ x }) { color: red; } }";
        let open = find_next_top_level_open_brace(source, 0).unwrap();
        assert_eq!(find_matching_brace(source, open, false), source.rfind('}'));

        let eof = "a { content: \"}\"; fn({ x })";
        let open = find_next_top_level_open_brace(eof, 0).unwrap();
        assert_eq!(find_matching_brace(eof, open, false), None);
        assert_eq!(find_matching_brace(eof, open, true), Some(eof.len()));
    }
}
