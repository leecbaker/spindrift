use super::*;

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
pub(crate) fn parse_first_css_url(value: &str) -> Option<String> {
    parse_first_css_url_with_modifiers(value).map(|url| url.src)
}

pub(crate) fn parse_first_css_url_with_modifiers(value: &str) -> Option<ParsedCssUrl> {
    let mut remaining = trim_css_value(value);
    while let Some(offset) = find_ascii_url_function(remaining) {
        remaining = &remaining[offset..];
        if let Some((url, _)) = parse_css_url_token_with_modifiers(remaining) {
            return Some(url);
        }
        remaining = &remaining["url".len()..];
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
    let body = strip_ascii_function(value, "url")?;
    let (arguments, tail) = split_function_argument(body)?;
    let (url, modifiers) = parse_url_source_and_modifiers(arguments)?;
    Some((
        ParsedCssUrl {
            src: url,
            modifiers: parse_request_url_modifiers(modifiers)?,
        },
        tail,
    ))
}

fn find_ascii_url_function(value: &str) -> Option<usize> {
    for (index, _) in value.char_indices() {
        let Some(name) = value.get(index..index + 3) else {
            continue;
        };
        if !name.eq_ignore_ascii_case("url")
            || index > 0 && is_css_ident_continue(value[..index].chars().next_back()?)
        {
            continue;
        }
        let after_name = value[index + 3..].trim_start();
        if after_name.starts_with('(') {
            return Some(index);
        }
    }
    None
}

fn parse_url_source_and_modifiers(value: &str) -> Option<(String, &str)> {
    let value = value.trim();
    if let Some((url, tail)) = parse_css_string_token(value) {
        return Some((url, tail.trim()));
    }
    let end = value.find(char::is_whitespace).unwrap_or(value.len());
    let url = value[..end].trim();
    (!url.is_empty()).then_some((url.to_string(), value[end..].trim()))
}

fn parse_request_url_modifiers(mut value: &str) -> Option<RequestUrlModifiers> {
    let mut modifiers = RequestUrlModifiers::default();
    while !value.is_empty() {
        let name_end = value.find('(')?;
        let name = value[..name_end].trim();
        if name.is_empty() || !name.chars().all(is_css_ident_continue) {
            return None;
        }
        let (arguments, tail) = split_function_argument(&value[name_end + 1..])?;
        let arguments = arguments.trim();
        if name.eq_ignore_ascii_case("cross-origin") {
            let mode = match arguments.to_ascii_lowercase().as_str() {
                "anonymous" => CrossOriginRequestMode::Anonymous,
                "use-credentials" => CrossOriginRequestMode::UseCredentials,
                _ => return None,
            };
            if modifiers.cross_origin.replace(mode).is_some() {
                return None;
            }
        } else if name.eq_ignore_ascii_case("integrity") {
            let (integrity, tail) = parse_css_string_token(arguments)?;
            if !tail.trim().is_empty() {
                return None;
            }
            if modifiers.integrity.replace(integrity).is_some() {
                return None;
            }
        } else if name.eq_ignore_ascii_case("referrer-policy") {
            let policy = arguments.to_ascii_lowercase();
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
        value = tail.trim();
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
}
