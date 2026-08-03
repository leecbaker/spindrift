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

/// Finds an ASCII substring without allocating a case-folded copy.
pub(in crate::css) fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Removes CSS declaration priority syntax before property value parsing.
///
/// CSS Cascade Level 5 defines `!important` as declaration priority rather
/// than part of the property value:
/// <https://www.w3.org/TR/css-cascade-5/#importance>.
pub(crate) fn trim_css_value(value: &str) -> &str {
    let value = value.trim();
    let important = "!important";
    let suffix_start = value.len().saturating_sub(important.len());
    if let Some(suffix) = value.get(suffix_start..)
        && suffix.eq_ignore_ascii_case(important)
    {
        value.get(..suffix_start).unwrap_or(value).trim_end()
    } else {
        value
    }
}

pub(crate) fn parse_css_string_token(value: &str) -> Option<(String, &str)> {
    let mut chars = value.char_indices();
    let (_, quote) = chars.next()?;
    if !matches!(quote, '"' | '\'') {
        return None;
    }
    let mut output = String::new();
    while let Some((index, character)) = chars.next() {
        if character == '\\' {
            push_css_string_escape(&mut output, &mut chars);
        } else if character == quote {
            return Some((output, &value[index + character.len_utf8()..]));
        } else {
            output.push(character);
        }
    }
    None
}

/// Decodes one escaped CSS string component.
///
/// CSS Syntax defines string escapes as either up to six hexadecimal digits
/// plus optional trailing whitespace, or a single escaped code point:
/// <https://www.w3.org/TR/css-syntax-3/#consume-escaped-code-point>.
pub(in crate::css) fn push_css_string_escape(
    output: &mut String,
    chars: &mut std::str::CharIndices<'_>,
) {
    let mut clone = chars.clone();
    let mut hex = String::new();
    while hex.len() < 6 {
        let Some((_, character)) = clone.next() else {
            break;
        };
        if character.is_ascii_hexdigit() {
            hex.push(character);
        } else {
            break;
        }
    }
    if !hex.is_empty() {
        for _ in 0..hex.len() {
            chars.next();
        }
        if let Ok(codepoint) = u32::from_str_radix(&hex, 16)
            && let Some(character) = char::from_u32(codepoint)
        {
            output.push(character);
        }
        if chars
            .clone()
            .next()
            .is_some_and(|(_, character)| character.is_whitespace())
        {
            chars.next();
        }
        return;
    }
    if let Some((_, character)) = chars.next()
        && !matches!(character, '\n' | '\r' | '\u{000c}')
    {
        output.push(character);
    }
}

/// Decode CSS escapes in an identifier-like token.
///
/// CSS Syntax uses the same escaped-code-point algorithm for identifiers and
/// strings.  Keeping the decoding at the token boundary lets callers retain
/// their own identifier comparison rules (for example, CSS font family names
/// are ASCII-case-insensitive while `@font-feature-values` aliases are
/// case-sensitive):
/// <https://www.w3.org/TR/css-syntax-3/#consume-escaped-code-point>.
pub(crate) fn decode_css_escapes(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.char_indices();
    while let Some((_, character)) = characters.next() {
        if character == '\\' {
            push_css_string_escape(&mut output, &mut characters);
        } else {
            output.push(character);
        }
    }
    output
}

pub(crate) fn strip_ascii_function<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let prefix_len = name.len();
    let prefix = value.get(..prefix_len)?;
    if !prefix.eq_ignore_ascii_case(name) {
        return None;
    }
    let after_name = value[prefix_len..].trim_start();
    after_name.strip_prefix('(')
}

pub(crate) fn split_function_argument(value_after_open: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value_after_open.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() {
            if character == '\\' {
                escaped = true;
            } else if Some(character) == quote {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '(' => depth += 1,
            ')' if depth == 0 => {
                return Some((&value_after_open[..index], &value_after_open[index + 1..]));
            }
            ')' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

pub(crate) fn is_css_ident_continue(character: char) -> bool {
    character == '-'
        || character == '_'
        || character.is_ascii_alphanumeric()
        || !character.is_ascii()
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

/// Splits at an outermost CSS delimiter, discarding empty trimmed components.
///
/// This is for property grammars that accept comma- or slash-separated lists
/// but reject empty list entries themselves.
pub(in crate::css) fn split_nonempty_css_top_level_delimiter(
    value: &str,
    delimiter: char,
) -> Vec<&str> {
    split_css_top_level_delimiter(value, delimiter)
        .into_iter()
        .map(trim_css_value)
        .filter(|part| !part.is_empty())
        .collect()
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

/// Returns the decoded name, raw body, and remaining source for a leading CSS
/// function.  Unlike [`css_single_function`], this is suitable for grammars
/// such as `image-set()` whose function is followed by descriptors.
///
/// The boundaries come from CSS Syntax tokens, so escaped identifiers,
/// comments, strings, and nested blocks cannot be mistaken for delimiters.
/// <https://www.w3.org/TR/css-syntax-3/#component-value>
pub(crate) fn css_leading_function(value: &str) -> Option<(String, &str, &str)> {
    let value = value.trim_start();
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
    Some((name, body, &value[parser.position().byte_index()..]))
}

/// Collect URL sources that can occur in CSS `<image>` values.
///
/// This is intentionally a component-value traversal rather than a substring
/// search. In particular, strings only become resource URLs when they are the
/// first component of an `image-set()` option; descriptor strings in `type()`
/// are never returned.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
pub(crate) fn css_image_candidate_urls(value: &str) -> Vec<String> {
    let mut urls = Vec::new();
    collect_css_image_candidate_urls(value, &mut urls);
    urls
}

fn collect_css_image_candidate_urls(value: &str, urls: &mut Vec<String>) {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        let token = token.clone();
        match token {
            Token::UnquotedUrl(url) => {
                if !url.is_empty() {
                    urls.push(url.to_string());
                }
            }
            Token::Function(name) => {
                let name = name.to_string();
                let _ = parser.parse_nested_block(
                    |nested| -> Result<(), cssparser::ParseError<'_, ()>> {
                        let start = nested.position();
                        while let Ok(token) = nested.next_including_whitespace_and_comments() {
                            if is_simple_block(token) && !consume_nested_block(nested) {
                                return Err(nested.new_custom_error(()));
                            }
                        }
                        let body = nested.slice_from(start);
                        if name.eq_ignore_ascii_case("url") {
                            if let Some((url, _)) = parse_css_string_token(body.trim()) {
                                if !url.is_empty() {
                                    urls.push(url);
                                }
                            } else if let Some(url) = split_css_component_values(body).first() {
                                let url = decode_css_escapes(url);
                                if !url.is_empty() {
                                    urls.push(url);
                                }
                            }
                        }
                        if name.eq_ignore_ascii_case("image-set")
                            || name.eq_ignore_ascii_case("-webkit-image-set")
                        {
                            for option in split_css_top_level_delimiter(body, ',') {
                                let components = split_css_component_values(option);
                                let Some(source) = components.first() else {
                                    continue;
                                };
                                if let Some((url, tail)) = parse_css_string_token(source)
                                    && tail.trim().is_empty()
                                    && !url.is_empty()
                                {
                                    urls.push(url);
                                }
                            }
                        }
                        // Recurse into nested image functions, but not into a
                        // `type()` descriptor: its string is metadata, never
                        // a fetchable image reference.
                        if !name.eq_ignore_ascii_case("type") {
                            collect_css_image_candidate_urls(body, urls);
                        }
                        Ok(())
                    },
                );
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                let _ = parser.parse_nested_block(
                    |nested| -> Result<(), cssparser::ParseError<'_, ()>> {
                        let start = nested.position();
                        while let Ok(token) = nested.next_including_whitespace_and_comments() {
                            if is_simple_block(token) && !consume_nested_block(nested) {
                                return Err(nested.new_custom_error(()));
                            }
                        }
                        collect_css_image_candidate_urls(nested.slice_from(start), urls);
                        Ok(())
                    },
                );
            }
            _ => {}
        }
    }
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
        assert_eq!(
            split_nonempty_css_top_level_delimiter("first, , func(1, 2),  ", ','),
            ["first", "func(1, 2)"]
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
    fn image_candidate_urls_follow_component_value_boundaries() {
        assert_eq!(
            css_image_candidate_urls(
                r#"image-set("a\2epng" 1x type("image/png"), linear-gradient(red, blue) 2x, url("c.png") 3x)"#,
            ),
            ["a.png", "c.png"]
        );
        assert_eq!(
            css_image_candidate_urls(r#"image-set(image(url("nested.png")) 1x type("not-a-url"))"#),
            ["nested.png"]
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

    #[test]
    fn ascii_source_search_ignores_case_without_allocating() {
        assert_eq!(
            find_ascii_case_insensitive("before @FoNt-FaCe after", "@font-face"),
            Some(7)
        );
        assert_eq!(find_ascii_case_insensitive("stylesheet", "@media"), None);
    }
}
