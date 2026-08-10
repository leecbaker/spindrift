//! Token-aware helpers for CSS component-value boundaries.
//!
//! CSS Syntax tokenizes comments, escapes, strings, functions, and simple
//! blocks before a property grammar sees its components.  Keeping the source
//! slices returned here lets legacy value parsers retain their cheap borrowed
//! input while making their structural decisions from tokens instead of bytes.
//! <https://www.w3.org/TR/css-syntax-3/#component-value>

use cssparser::{Parser, ParserInput, SourcePosition, ToCss, Token, TokenSerializationType};

/// An owned, canonical CSS component-value stream.
///
/// CSS Variables substitutes component values, rather than concatenating
/// source text.  Retaining a token-aware serialization at this boundary keeps
/// adjacent substituted values from being tokenized again as a different CSS
/// value and rejects CSS Syntax tokenizer errors before cascade processing.
/// <https://www.w3.org/TR/css-syntax-3/#component-value>
/// <https://www.w3.org/TR/css-variables-1/#using-variables>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CssComponentValueList {
    css: String,
}

impl CssComponentValueList {
    /// Parses and serializes a complete component-value list.
    ///
    /// CSS Syntax permits EOF-closed simple blocks, but its bad-string,
    /// bad-URL, and unmatched closing-delimiter tokens are parse errors and
    /// therefore cannot form a declaration value.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let mut input = ParserInput::new(value);
        let mut parser = Parser::new(&mut input);
        serialize_component_values(&mut parser)
            .ok()
            .map(|stream| Self { css: stream.css })
    }

    pub(crate) fn as_css(&self) -> &str {
        &self.css
    }
}

#[derive(Default)]
struct SerializedComponentValues {
    css: String,
    last_token_type: TokenSerializationType,
}

impl SerializedComponentValues {
    fn push_component(&mut self, css: &str, token_type: TokenSerializationType) {
        if self.last_token_type.needs_separator_when_before(token_type) {
            // A comment is a CSS token boundary without imposing whitespace
            // semantics on grammars where whitespace is observable.
            self.css.push_str("/**/");
        }
        self.css.push_str(css);
        self.last_token_type = token_type;
    }
}

fn serialize_component_values(
    parser: &mut Parser<'_, '_>,
) -> Result<SerializedComponentValues, ()> {
    let mut output = SerializedComponentValues::default();
    while !parser.is_exhausted() {
        let token = parser
            .next_including_whitespace_and_comments()
            .map_err(|_| ())?;
        if token.is_parse_error() {
            return Err(());
        }
        let token = token.clone();
        let token_type = token.serialization_type();
        let css = match token {
            Token::Function(_) => {
                let mut css = token.to_css_string();
                let nested = parser
                    .parse_nested_block(|nested| {
                        serialize_component_values(nested)
                            .map_err(|_| nested.new_custom_error::<(), ()>(()))
                    })
                    .map_err(|_| ())?;
                css.push_str(&nested.css);
                css.push(')');
                css
            }
            Token::ParenthesisBlock => {
                let mut css = token.to_css_string();
                let nested = parser
                    .parse_nested_block(|nested| {
                        serialize_component_values(nested)
                            .map_err(|_| nested.new_custom_error::<(), ()>(()))
                    })
                    .map_err(|_| ())?;
                css.push_str(&nested.css);
                css.push(')');
                css
            }
            Token::SquareBracketBlock => {
                let mut css = token.to_css_string();
                let nested = parser
                    .parse_nested_block(|nested| {
                        serialize_component_values(nested)
                            .map_err(|_| nested.new_custom_error::<(), ()>(()))
                    })
                    .map_err(|_| ())?;
                css.push_str(&nested.css);
                css.push(']');
                css
            }
            Token::CurlyBracketBlock => {
                let mut css = token.to_css_string();
                let nested = parser
                    .parse_nested_block(|nested| {
                        serialize_component_values(nested)
                            .map_err(|_| nested.new_custom_error::<(), ()>(()))
                    })
                    .map_err(|_| ())?;
                css.push_str(&nested.css);
                css.push('}');
                css
            }
            _ => token.to_css_string(),
        };
        output.push_component(&css, token_type);
    }
    Ok(output)
}

fn consume_nested_block(parser: &mut Parser<'_, '_>) -> bool {
    parser
        .parse_nested_block(|nested| {
            while let Ok(token) = nested.next_including_whitespace_and_comments() {
                if token.is_parse_error() {
                    return Err(nested.new_custom_error(()));
                }
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
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let token = parser.next().ok()?.clone();
    let Token::QuotedString(text) = token else {
        return None;
    };
    Some((
        text.to_string(),
        trim_css_component_value_start(&value[parser.position().byte_index()..]),
    ))
}

/// Skip CSS whitespace and comments before the next component value.
///
/// Variable substitution uses comments as token separators. They are not
/// property data, so string-token consumers must not expose a comment-only
/// tail as another component to parse.
pub(crate) fn trim_css_component_value_start(value: &str) -> &str {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    loop {
        let token_start = parser.position().byte_index();
        match parser.next_including_whitespace_and_comments() {
            Ok(Token::WhiteSpace(_) | Token::Comment(_)) => {}
            Ok(_) => return &value[token_start..],
            Err(_) => return &value[value.len()..],
        }
    }
}

/// Removes only leading and trailing CSS whitespace/comment tokens.
///
/// A complete `var()` substitution can be surrounded by the resolver's token
/// separators even when no authored component is adjacent to it. Those
/// separators are not part of a property's value at this boundary, while
/// comments between two significant tokens must remain so they continue to
/// prevent accidental token merging.
pub(crate) fn trim_css_component_value_edges(value: &str) -> &str {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut first = None;
    let mut last = 0;

    loop {
        let token_start = parser.position().byte_index();
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token,
            Err(_) => break,
        };
        let is_ignorable = matches!(token, Token::WhiteSpace(_) | Token::Comment(_));
        if !is_ignorable && is_simple_block(token) && !consume_nested_block(&mut parser) {
            return value;
        }
        let token_end = parser.position().byte_index();
        if !is_ignorable {
            first.get_or_insert(token_start);
            last = token_end;
        }
    }

    first.map_or(&value[value.len()..], |first| &value[first..last])
}

/// Splits at CSS whitespace component-value boundaries.
pub(in crate::css) fn split_css_component_values(value: &str) -> Vec<&str> {
    try_split_css_component_values(value).unwrap_or_default()
}

/// Splits at CSS whitespace component-value boundaries, rejecting malformed
/// component streams rather than treating a valid prefix as a complete value.
pub(in crate::css) fn try_split_css_component_values(value: &str) -> Option<Vec<&str>> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut parts = Vec::new();
    let mut component_start = None;

    while !parser.is_exhausted() {
        let token_start = parser.position();
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        if token.is_parse_error() {
            return None;
        }
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
            return None;
        }
    }
    if let Some(start) = component_start {
        let part = parser.slice_from(start).trim();
        if !part.is_empty() {
            parts.push(part);
        }
    }
    Some(parts)
}

/// Splits at an outermost CSS delimiter token.
pub(crate) fn split_css_top_level_delimiter(value: &str, delimiter: char) -> Vec<&str> {
    try_split_css_top_level_delimiter(value, delimiter).unwrap_or_default()
}

/// Splits at an outermost CSS delimiter token, rejecting malformed component
/// streams rather than returning components parsed before the error.
pub(in crate::css) fn try_split_css_top_level_delimiter(
    value: &str,
    delimiter: char,
) -> Option<Vec<&str>> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut parts = Vec::new();
    let source_start = parser.position();
    let mut start = 0usize;
    while !parser.is_exhausted() {
        let token_start = source_offset(&parser, source_start);
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        if token.is_parse_error() {
            return None;
        }
        let is_delimiter = matches!(token, Token::Comma) && delimiter == ','
            || matches!(token, Token::Delim(found) if found == delimiter);
        if is_delimiter {
            parts.push(value[start..token_start].trim());
            start = source_offset(&parser, source_start);
        } else if is_simple_block(&token) && !consume_nested_block(&mut parser) {
            return None;
        }
    }
    parts.push(value[start..].trim());
    Some(parts)
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

/// Parses a complete CSS function list and returns decoded names with their
/// raw, token-validated argument component streams.
///
/// Property grammars such as `transform` need to retain argument source for
/// their own semantic parsing, but function boundaries and names must still
/// follow CSS Syntax rather than byte searches.
pub(in crate::css) fn css_function_list(value: &str) -> Option<Vec<(String, &str)>> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut functions = Vec::new();

    while !parser.is_exhausted() {
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        if matches!(token, Token::WhiteSpace(_) | Token::Comment(_)) {
            continue;
        }
        let Token::Function(name) = token else {
            return None;
        };
        let body = parser
            .parse_nested_block(|nested| {
                let start = nested.position();
                while let Ok(token) = nested.next_including_whitespace_and_comments() {
                    if token.is_parse_error()
                        || (is_simple_block(token) && !consume_nested_block(nested))
                    {
                        return Err(nested.new_custom_error(()));
                    }
                }
                if nested.is_exhausted() {
                    Ok::<_, cssparser::ParseError<'_, ()>>(nested.slice_from(start))
                } else {
                    Err(nested.new_custom_error(()))
                }
            })
            .ok()?;
        functions.push((name.to_string(), body));
    }
    Some(functions)
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
                if token.is_parse_error()
                    || (is_simple_block(token) && !consume_nested_block(nested))
                {
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
                if token.is_parse_error()
                    || (is_simple_block(token) && !consume_nested_block(nested))
                {
                    return Err(nested.new_custom_error(()));
                }
            }
            Ok::<_, cssparser::ParseError<'_, ()>>(nested.slice_from(start))
        })
        .ok()?;
    Some((name, body, &value[parser.position().byte_index()..]))
}

/// Parses one leading CSS function with a particular decoded name.
///
/// The function and its nested component values are consumed by `cssparser`;
/// callers receive only the token-bounded argument stream and the remaining
/// declaration source for grammars that contain multiple component values.
pub(crate) fn css_leading_function_matching<'a>(
    value: &'a str,
    expected_name: &str,
) -> Option<(&'a str, &'a str)> {
    let (name, body, tail) = css_leading_function(value)?;
    name.eq_ignore_ascii_case(expected_name)
        .then_some((body, tail))
}

/// Parses one leading CSS identifier and returns its decoded value and tail.
pub(crate) fn css_leading_ident(value: &str) -> Option<(String, &str)> {
    let value = trim_css_component_value_start(value);
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let ident = parser.expect_ident_cloned().ok()?.to_string();
    Some((ident, &value[parser.position().byte_index()..]))
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
                            } else if let Some(url) = css_single_ident(body)
                                && !url.is_empty()
                            {
                                urls.push(url);
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
pub(crate) fn css_single_ident(value: &str) -> Option<String> {
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

/// Returns the source range of the first outermost decoded identifier matching
/// `keyword`. Nested component values, strings, comments, and escapes are
/// handled according to CSS Syntax.
pub(in crate::css) fn find_css_top_level_keyword_range(
    value: &str,
    keyword: &str,
) -> Option<std::ops::Range<usize>> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    while !parser.is_exhausted() {
        let start = parser.position().byte_index();
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        if token.is_parse_error() {
            return None;
        }
        if matches!(token, Token::Ident(ref name) if name.eq_ignore_ascii_case(keyword)) {
            return Some(start..parser.position().byte_index());
        }
        if is_simple_block(&token) && !consume_nested_block(&mut parser) {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_value_stream_rejects_css_syntax_error_tokens() {
        for value in ["red)", "var(--color, \"\n", "var(--color, url(\"\n"] {
            assert!(CssComponentValueList::parse(value).is_none(), "{value}");
        }
    }

    #[test]
    fn component_value_stream_accepts_balanced_blocks_and_cdo_cdc() {
        for value in [
            "{ [ var(--color) ] }",
            "var(--color) <!--",
            "--> var(--color)",
        ] {
            assert!(CssComponentValueList::parse(value).is_some(), "{value}");
        }
    }

    #[test]
    fn component_boundaries_follow_css_tokens() {
        assert_eq!(
            split_css_component_values("solid /* separator */ rgb(1 2 3) 'x y'"),
            ["solid", "rgb(1 2 3)", "'x y'"]
        );
        assert_eq!(
            split_css_component_values("foo/**/bar 1/**/2px"),
            ["foo", "bar", "1", "2px"]
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
    fn strict_value_boundaries_reject_invalid_prefixes() {
        for value in ["red)", "url(\"\n"] {
            assert!(try_split_css_component_values(value).is_none(), "{value}");
            assert!(
                try_split_css_top_level_delimiter(value, ',').is_none(),
                "{value}"
            );
        }
        assert_eq!(split_css_component_values("red)"), Vec::<&str>::new());
        assert_eq!(
            try_split_css_component_values("calc(1px"),
            Some(vec!["calc(1px"])
        );
        assert_eq!(
            try_split_css_top_level_delimiter(r#"url("a,b"), c"#, ','),
            Some(vec![r#"url("a,b")"#, "c"])
        );
    }

    #[test]
    fn css_function_lists_decode_names_and_preserve_token_boundaries() {
        let functions = css_function_list(r"tr\61 nslate(10px/**/, 20px) /**/ sc\61 le(2)")
            .expect("valid CSS function list");
        assert_eq!(
            functions,
            vec![
                ("translate".to_owned(), "10px/**/, 20px"),
                ("scale".to_owned(), "2"),
            ]
        );
        assert!(css_function_list("scale(2) nope").is_none());
        assert!(css_function_list("scale(2").is_some());
    }

    #[test]
    fn token_consumers_ignore_comment_boundaries() {
        let (text, tail) = parse_css_string_token("/**/\"hello\"/**/ \"there\"")
            .expect("comment-delimited string token");
        assert_eq!(text, "hello");
        assert_eq!(tail, "\"there\"");
        assert_eq!(trim_css_component_value_edges("/**/10px/**/"), "10px");
        assert_eq!(
            trim_css_component_value_edges("/**/calc(1px)/**/"),
            "calc(1px)"
        );
        assert_eq!(trim_css_component_value_edges("foo/**/bar"), "foo/**/bar");
        assert_eq!(
            split_css_top_level_delimiter("/**/Ahem/**/,/**/sans-serif/**/", ','),
            ["/**/Ahem/**/", "/**/sans-serif/**/"]
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
}
