use super::*;
use crate::css::component_values::css_leading_function_matching;

/// Cross-origin mode requested by CSS Values Level 5's `cross-origin()` URL
/// modifier.
/// <https://drafts.csswg.org/css-values-5/#request-url-modifiers>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrossOriginRequestMode {
    Anonymous,
    UseCredentials,
}

/// Request modifiers attached to a CSS `url()` source.
///
/// The modifiers stay with the image source until resource selection, where
/// CORS, integrity, and referrer policy are enforceable against the actual
/// fetch and document origins.
/// <https://drafts.csswg.org/css-values-5/#request-url-modifiers>
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RequestUrlModifiers {
    pub(crate) cross_origin: Option<CrossOriginRequestMode>,
    pub(crate) integrity: Option<String>,
    pub(crate) referrer_policy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedCssUrl {
    pub(crate) src: String,
    pub(crate) modifiers: RequestUrlModifiers,
}

/// Finds the first CSS `url()` value anywhere in a value.
///
/// This is used for shorthands such as `background`, where `url()` can appear
/// after other component values. The CSS Values 5 request URL modifiers live
/// inside `url()`, which means they are not accepted by cssparser's ordinary
/// URL-token helper and must be parsed as a function value:
/// <https://drafts.csswg.org/css-backgrounds-3/#the-background> and
/// <https://drafts.csswg.org/css-values-5/#request-url-modifiers>.
pub(crate) fn parse_first_css_url_with_modifiers(value: &str) -> Option<ParsedCssUrl> {
    let value = trim_css_value(value);
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    while !parser.is_exhausted() {
        let start = parser.position().byte_index();
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        match token {
            cssparser::Token::UnquotedUrl(src) => {
                return Some(ParsedCssUrl {
                    src: src.to_string(),
                    modifiers: RequestUrlModifiers::default(),
                });
            }
            cssparser::Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                return parse_css_url_token_with_modifiers(&value[start..]).map(|(url, _)| url);
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn parse_css_url_token(value: &str) -> Option<(String, &str)> {
    parse_css_url_token_with_modifiers(value).map(|(url, tail)| (url.src, tail))
}

pub(crate) fn parse_css_url_token_with_modifiers(value: &str) -> Option<(ParsedCssUrl, &str)> {
    let value = trim_css_value(value).trim_start();
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    if let Ok(url) = parser.try_parse(|input| input.expect_url()) {
        return Some((
            ParsedCssUrl {
                src: url.to_string(),
                modifiers: RequestUrlModifiers::default(),
            },
            &value[parser.position().byte_index()..],
        ));
    }
    let (arguments, tail) = css_leading_function_matching(value, "url")?;
    let (url, modifiers) = parse_url_source_and_modifiers(arguments)?;
    Some((
        ParsedCssUrl {
            src: url,
            modifiers: parse_request_url_modifiers(modifiers)?,
        },
        tail,
    ))
}

fn parse_url_source_and_modifiers(value: &str) -> Option<(String, &str)> {
    let value = value.trim();
    if let Some((url, tail)) = parse_css_string_token(value) {
        return Some((url, tail.trim()));
    }
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let url = parser.expect_url().ok()?.to_string();
    Some((url, value[parser.position().byte_index()..].trim()))
}

fn parse_request_url_modifiers(value: &str) -> Option<RequestUrlModifiers> {
    let mut modifiers = RequestUrlModifiers::default();
    for (name, arguments) in crate::css::component_values::css_function_list(value)? {
        if name.eq_ignore_ascii_case("cross-origin") {
            let mode = match crate::css::component_values::css_single_ident(arguments)?
                .to_ascii_lowercase()
                .as_str()
            {
                "anonymous" => CrossOriginRequestMode::Anonymous,
                "use-credentials" => CrossOriginRequestMode::UseCredentials,
                _ => return None,
            };
            if modifiers.cross_origin.replace(mode).is_some() {
                return None;
            }
        } else if name.eq_ignore_ascii_case("integrity") {
            let (integrity, tail) = parse_css_string_token(arguments.trim())?;
            if !tail.trim().is_empty() {
                return None;
            }
            if modifiers.integrity.replace(integrity).is_some() {
                return None;
            }
        } else if name.eq_ignore_ascii_case("referrer-policy") {
            let policy =
                crate::css::component_values::css_single_ident(arguments)?.to_ascii_lowercase();
            if !matches!(
                policy.as_str(),
                "no-referrer"
                    | "no-referrer-when-downgrade"
                    | "origin"
                    | "origin-when-cross-origin"
                    | "same-origin"
                    | "strict-origin"
                    | "strict-origin-when-cross-origin"
                    | "unsafe-url"
            ) {
                return None;
            }
            if modifiers.referrer_policy.replace(policy).is_some() {
                return None;
            }
        } else {
            return None;
        }
    }
    Some(modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_url_with_request_modifiers() {
        let value = r#"url("https://example.test/image.png" cross-origin(anonymous) integrity("sha256-value"))"#;

        assert_eq!(
            parse_css_url_token(value),
            Some(("https://example.test/image.png".to_string(), ""))
        );
    }

    #[test]
    fn request_modifiers_are_consumed_before_image_set_resolution() {
        let value = r#"url("image.png" referrer-policy(no-referrer)) 2x"#;

        assert_eq!(
            parse_css_url_token(value),
            Some(("image.png".to_string(), " 2x"))
        );
    }

    #[test]
    fn rejects_duplicate_request_modifiers() {
        assert!(
            parse_css_url_token(
                r#"url("image.png" cross-origin(anonymous) cross-origin(use-credentials))"#
            )
            .is_none()
        );
    }

    #[test]
    fn url_function_and_modifier_names_are_token_decoded() {
        assert_eq!(
            parse_css_url_token(r#"\75 rl("image.png" cross-origin(\61 nonymous))"#),
            Some(("image.png".to_string(), ""))
        );
    }

    #[test]
    fn complete_url_values_do_not_accept_a_valid_prefix() {
        assert!(
            parse_css_url_token(
                r#"url("image.png" integrity("bad
value"))"#
            )
            .is_none()
        );
    }
}
