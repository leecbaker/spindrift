use super::*;

pub(crate) fn parse_marker_content(value: &str) -> Option<MarkerContent> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(MarkerContent::Auto);
    }
    if value.eq_ignore_ascii_case("none") {
        return Some(MarkerContent::None);
    }

    let mut rest = value.trim();
    let mut parts = Vec::new();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if let Some((text, tail)) = parse_css_string_token(rest) {
            parts.push(MarkerContentPart::Text(text));
            rest = tail;
        } else if let Some((style, tail)) = parse_list_item_counter_token(rest) {
            parts.push(style);
            rest = tail;
        } else if let Some((counters, tail)) = parse_counters_token(rest) {
            parts.push(counters);
            rest = tail;
        } else if let Some((quote, tail)) = parse_generated_quote_token(rest) {
            parts.push(MarkerContentPart::Quote(quote));
            rest = tail;
        } else {
            return None;
        }
    }
    (!parts.is_empty()).then_some(MarkerContent::Parts(parts))
}

pub(crate) fn parse_content_property(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<Content> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(Content::Normal);
    }
    if value.eq_ignore_ascii_case("none") {
        return Some(Content::None);
    }

    let (content, alt) = split_top_level_slash(value);
    let mut parts = parse_generated_content_parts(content, base_url, root_url)?;
    let alt = if let Some(alt) = alt {
        Some(parse_generated_alt_text(alt)?)
    } else {
        None
    };
    if parts.len() == 1
        && let GeneratedContentPart::Image { .. } = parts[0]
    {
        return Some(Content::Replacement {
            image: parts.remove(0),
            alt,
        });
    }
    Some(Content::List { parts, alt })
}

fn parse_generated_content_parts(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<GeneratedContent> {
    let mut rest = value.trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if let Some((text, tail)) = parse_css_string_token(rest) {
            parts.push(GeneratedContentPart::Text(text));
            rest = tail;
        } else if let Some((part, tail)) = parse_generated_attr_token(rest) {
            parts.push(part);
            rest = tail;
        } else if let Some((part, tail)) = parse_generated_counter_token(rest) {
            parts.push(part);
            rest = tail;
        } else if let Some((part, tail)) = parse_generated_counters_token(rest) {
            parts.push(part);
            rest = tail;
        } else if let Some((part, tail)) = parse_generated_target_counter_token(rest) {
            parts.push(part);
            rest = tail;
        } else if let Some((part, tail)) = parse_generated_target_text_token(rest) {
            parts.push(part);
            rest = tail;
        } else if let Some((part, tail)) = parse_generated_image_token(rest, base_url, root_url) {
            parts.push(part);
            rest = tail;
        } else if rest
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("contents"))
            && rest[8..]
                .chars()
                .next()
                .is_none_or(|character| !is_css_ident_continue(character))
        {
            parts.push(GeneratedContentPart::Contents);
            rest = &rest[8..];
        } else if let Some((quote, tail)) = parse_generated_quote_token(rest) {
            parts.push(GeneratedContentPart::Quote(quote));
            rest = tail;
        } else if let Some((leader, tail)) = parse_generated_leader_token(rest) {
            parts.push(GeneratedContentPart::Leader(leader));
            rest = tail;
        } else {
            return None;
        }
    }
    Some(parts)
}

fn parse_generated_alt_text(value: &str) -> Option<GeneratedAltText> {
    let mut rest = value.trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if let Some((text, tail)) = parse_css_string_token(rest) {
            parts.push(GeneratedAltTextPart::Text(text));
            rest = tail;
        } else if let Some((part, tail)) = parse_generated_attr_token(rest) {
            parts.push(generated_part_to_alt(part)?);
            rest = tail;
        } else if let Some((part, tail)) = parse_generated_counter_token(rest) {
            parts.push(generated_part_to_alt(part)?);
            rest = tail;
        } else if let Some((part, tail)) = parse_generated_counters_token(rest) {
            parts.push(generated_part_to_alt(part)?);
            rest = tail;
        } else {
            return None;
        }
    }
    Some(parts)
}

fn generated_part_to_alt(part: GeneratedContentPart) -> Option<GeneratedAltTextPart> {
    match part {
        GeneratedContentPart::Text(text) => Some(GeneratedAltTextPart::Text(text)),
        GeneratedContentPart::Attr { name, fallback } => {
            Some(GeneratedAltTextPart::Attr { name, fallback })
        }
        GeneratedContentPart::Counter { name, style } => {
            Some(GeneratedAltTextPart::Counter { name, style })
        }
        GeneratedContentPart::Counters {
            name,
            separator,
            style,
        } => Some(GeneratedAltTextPart::Counters {
            name,
            separator,
            style,
        }),
        GeneratedContentPart::Contents
        | GeneratedContentPart::Image { .. }
        | GeneratedContentPart::TargetCounter { .. }
        | GeneratedContentPart::TargetText { .. }
        | GeneratedContentPart::Quote(_)
        | GeneratedContentPart::Leader(_) => None,
    }
}

pub(crate) fn parse_named_string_sets(value: &str) -> Option<Vec<NamedStringSet>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }

    let mut sets = Vec::new();
    for item in split_top_level_commas(value) {
        let item = item.trim();
        if item.is_empty() {
            return None;
        }
        let (name, rest) = split_leading_ident(item)?;
        let mut rest = rest.trim_start();
        let mut parts = Vec::new();
        while !rest.is_empty() {
            if let Some((text, tail)) = parse_css_string_token(rest) {
                parts.push(NamedStringPart::String(text));
                rest = tail.trim_start();
            } else if let Some(tail) = strip_ascii_function(rest, "content") {
                let (argument, tail) = split_function_argument(tail)?;
                let argument = argument.trim().to_ascii_lowercase();
                if argument.is_empty() || argument == "text" {
                    parts.push(NamedStringPart::ContentText);
                    rest = tail.trim_start();
                } else if argument == "first-letter" {
                    parts.push(NamedStringPart::ContentFirstLetter);
                    rest = tail.trim_start();
                } else if argument == "marker" {
                    parts.push(NamedStringPart::ContentMarker);
                    rest = tail.trim_start();
                } else if argument == "before" {
                    parts.push(NamedStringPart::BeforeContent);
                    rest = tail.trim_start();
                } else if argument == "after" {
                    parts.push(NamedStringPart::AfterContent);
                    rest = tail.trim_start();
                } else {
                    return None;
                }
            } else if let Some((attr, tail)) = parse_named_string_attr_token(rest) {
                parts.push(attr);
                rest = tail.trim_start();
            } else if let Some((image, tail)) = parse_named_string_image_token(rest) {
                parts.push(image);
                rest = tail.trim_start();
            } else if let Some((quote, tail)) = parse_generated_quote_token(rest) {
                parts.push(NamedStringPart::Quote(quote));
                rest = tail.trim_start();
            } else if let Some((leader, tail)) = parse_generated_leader_token(rest) {
                parts.push(NamedStringPart::Leader(leader));
                rest = tail.trim_start();
            } else if let Some((target_counter, tail)) = parse_named_string_target_counter(rest) {
                parts.push(target_counter);
                rest = tail.trim_start();
            } else if let Some((target_text, tail)) = parse_named_string_target_text(rest) {
                parts.push(target_text);
                rest = tail.trim_start();
            } else if let Some((counter, tail)) = parse_named_string_counter_token(rest) {
                parts.push(counter);
                rest = tail.trim_start();
            } else if let Some((counters, tail)) = parse_named_string_counters_token(rest) {
                parts.push(counters);
                rest = tail.trim_start();
            } else {
                return None;
            }
        }
        if parts.is_empty() {
            return None;
        }
        sets.push(NamedStringSet {
            name: name.to_string(),
            parts,
        });
    }
    Some(sets)
}

pub(crate) fn parse_list_item_counter_token(value: &str) -> Option<(MarkerContentPart, &str)> {
    let (name, style, tail) = parse_counter_token(value)?;
    Some((MarkerContentPart::Counter { name, style }, tail))
}

pub(crate) fn parse_named_string_counter_token(value: &str) -> Option<(NamedStringPart, &str)> {
    let (name, style, tail) = parse_counter_token(value)?;
    Some((NamedStringPart::Counter { name, style }, tail))
}

pub(crate) fn parse_counters_token(value: &str) -> Option<(MarkerContentPart, &str)> {
    let (name, separator, style, tail) = parse_counters_components(value)?;
    Some((
        MarkerContentPart::Counters {
            name,
            separator,
            style,
        },
        tail,
    ))
}

pub(crate) fn parse_named_string_counters_token(value: &str) -> Option<(NamedStringPart, &str)> {
    let (name, separator, style, tail) = parse_counters_components(value)?;
    Some((
        NamedStringPart::Counters {
            name,
            separator,
            style,
        },
        tail,
    ))
}

fn parse_named_string_attr_token(value: &str) -> Option<(NamedStringPart, &str)> {
    let body = strip_ascii_function(value, "attr")?;
    let (argument, tail) = split_function_argument(body)?;
    let mut parts = split_top_level_commas(argument);
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    let name = parse_attr_name(parts.remove(0))?;
    let fallback = if let Some(fallback) = parts.first() {
        let fallback = fallback.trim();
        if fallback.is_empty() {
            None
        } else {
            let (text, tail) = parse_css_string_token(fallback)?;
            if !tail.trim().is_empty() {
                return None;
            }
            Some(text)
        }
    } else {
        None
    };
    Some((NamedStringPart::Attr { name, fallback }, tail))
}

fn parse_named_string_image_token(value: &str) -> Option<(NamedStringPart, &str)> {
    let (part, tail) = parse_generated_image_token(value, None, None)?;
    let GeneratedContentPart::Image { image } = part else {
        return None;
    };
    Some((NamedStringPart::Image(image), tail))
}

fn parse_named_string_target_counter(value: &str) -> Option<(NamedStringPart, &str)> {
    let body = strip_ascii_function(value, "target-counter")?;
    let (argument, tail) = split_function_argument(body)?;
    let arguments = split_top_level_commas(argument);
    if !(2..=3).contains(&arguments.len()) {
        return None;
    }
    let target = parse_target_reference(arguments[0].trim())?;
    let name = arguments[1].trim();
    if name.is_empty() {
        return None;
    }
    let style = if let Some(argument) = arguments.get(2) {
        Some(parse_list_style_type(argument.trim())?)
    } else {
        None
    };
    Some((
        NamedStringPart::TargetCounter {
            target,
            name: name.to_string(),
            style,
        },
        tail,
    ))
}

fn parse_named_string_target_text(value: &str) -> Option<(NamedStringPart, &str)> {
    let body = strip_ascii_function(value, "target-text")?;
    let (argument, tail) = split_function_argument(body)?;
    let arguments = split_top_level_commas(argument);
    if !(1..=2).contains(&arguments.len()) {
        return None;
    }
    let target = parse_target_reference(arguments[0].trim())?;
    let keyword = arguments
        .get(1)
        .map(|argument| parse_named_string_target_text_keyword(argument.trim()))
        .unwrap_or(Some(NamedStringTargetTextKeyword::Content))?;
    Some((NamedStringPart::TargetText { target, keyword }, tail))
}

fn parse_named_string_target_text_keyword(value: &str) -> Option<NamedStringTargetTextKeyword> {
    match value.to_ascii_lowercase().as_str() {
        "content" => Some(NamedStringTargetTextKeyword::Content),
        "before" => Some(NamedStringTargetTextKeyword::Before),
        "after" => Some(NamedStringTargetTextKeyword::After),
        "first-letter" => Some(NamedStringTargetTextKeyword::FirstLetter),
        _ => None,
    }
}

fn parse_target_reference(value: &str) -> Option<String> {
    if let Some((text, tail)) = parse_css_string_token(value)
        && tail.trim().is_empty()
    {
        return Some(text);
    }
    let body = strip_ascii_function(value, "url")?;
    let (argument, tail) = split_function_argument(body)?;
    if !tail.trim().is_empty() {
        return None;
    }
    if let Some((text, tail)) = parse_css_string_token(argument.trim())
        && tail.trim().is_empty()
    {
        return Some(text);
    }
    let target = argument.trim();
    (!target.is_empty()).then(|| target.to_string())
}

fn parse_generated_attr_token(value: &str) -> Option<(GeneratedContentPart, &str)> {
    let body = strip_ascii_function(value, "attr")?;
    let (argument, tail) = split_function_argument(body)?;
    let mut parts = split_top_level_commas(argument);
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    let name = parse_attr_name(parts.remove(0))?;
    let fallback = if let Some(fallback) = parts.first() {
        let fallback = fallback.trim();
        if fallback.is_empty() {
            None
        } else {
            let (text, tail) = parse_css_string_token(fallback)?;
            if !tail.trim().is_empty() {
                return None;
            }
            Some(text)
        }
    } else {
        None
    };
    Some((GeneratedContentPart::Attr { name, fallback }, tail))
}

fn parse_generated_counter_token(value: &str) -> Option<(GeneratedContentPart, &str)> {
    let (name, style, tail) = parse_counter_token(value)?;
    Some((GeneratedContentPart::Counter { name, style }, tail))
}

fn parse_generated_counters_token(value: &str) -> Option<(GeneratedContentPart, &str)> {
    let (name, separator, style, tail) = parse_counters_components(value)?;
    Some((
        GeneratedContentPart::Counters {
            name,
            separator,
            style,
        },
        tail,
    ))
}

fn parse_generated_image_token<'a>(
    value: &'a str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<(GeneratedContentPart, &'a str)> {
    if let Some((url, tail)) = parse_css_url_token(value) {
        return Some((
            GeneratedContentPart::Image {
                image: BackgroundImage::Url {
                    src: url,
                    base_url: base_url.cloned(),
                    root_url: root_url.cloned(),
                    request_modifiers: RequestUrlModifiers::default(),
                },
            },
            tail,
        ));
    }
    if let Some(body) = strip_ascii_function(value, "image-set") {
        let (argument, tail) = split_function_argument(body)?;
        let image = crate::css::parse_background_image(
            &format!("image-set({argument})"),
            base_url,
            root_url,
        )?;
        return Some((GeneratedContentPart::Image { image }, tail));
    }
    for name in [
        "linear-gradient",
        "repeating-linear-gradient",
        "radial-gradient",
        "repeating-radial-gradient",
    ] {
        let Some(body) = strip_ascii_function(value, name) else {
            continue;
        };
        let (argument, tail) = split_function_argument(body)?;
        let image_text = format!("{name}({argument})");
        let image = crate::css::parse_background_image(&image_text, base_url, root_url)?;
        return Some((GeneratedContentPart::Image { image }, tail));
    }
    None
}

fn parse_generated_target_counter_token(value: &str) -> Option<(GeneratedContentPart, &str)> {
    let body = strip_ascii_function(value, "target-counter")?;
    let (argument, tail) = split_function_argument(body)?;
    let arguments = split_top_level_commas(argument);
    if !(2..=3).contains(&arguments.len()) {
        return None;
    }
    let target = parse_target_reference(arguments[0].trim())?;
    let name = arguments[1].trim();
    if name.is_empty() {
        return None;
    }
    let style = if let Some(argument) = arguments.get(2) {
        Some(parse_list_style_type(argument.trim())?)
    } else {
        None
    };
    Some((
        GeneratedContentPart::TargetCounter {
            target,
            name: name.to_string(),
            style,
        },
        tail,
    ))
}

fn parse_generated_target_text_token(value: &str) -> Option<(GeneratedContentPart, &str)> {
    let body = strip_ascii_function(value, "target-text")?;
    let (argument, tail) = split_function_argument(body)?;
    let arguments = split_top_level_commas(argument);
    if !(1..=2).contains(&arguments.len()) {
        return None;
    }
    let target = parse_target_reference(arguments[0].trim())?;
    let keyword = arguments
        .get(1)
        .map(|argument| parse_named_string_target_text_keyword(argument.trim()))
        .unwrap_or(Some(NamedStringTargetTextKeyword::Content))?;
    Some((GeneratedContentPart::TargetText { target, keyword }, tail))
}

fn parse_generated_quote_token(value: &str) -> Option<(GeneratedQuote, &str)> {
    let (ident, tail) = split_leading_ident(value)?;
    let quote = match ident.to_ascii_lowercase().as_str() {
        "open-quote" => GeneratedQuote::Open,
        "close-quote" => GeneratedQuote::Close,
        "no-open-quote" => GeneratedQuote::NoOpen,
        "no-close-quote" => GeneratedQuote::NoClose,
        _ => return None,
    };
    Some((quote, tail))
}

fn parse_generated_leader_token(value: &str) -> Option<(String, &str)> {
    let body = strip_ascii_function(value, "leader")?;
    let (argument, tail) = split_function_argument(body)?;
    let argument = argument.trim();
    let leader = if let Some((text, string_tail)) = parse_css_string_token(argument) {
        if !string_tail.trim().is_empty() {
            return None;
        }
        text
    } else {
        match argument.to_ascii_lowercase().as_str() {
            "dotted" => ".".to_string(),
            "solid" => "_".to_string(),
            "space" => " ".to_string(),
            _ => return None,
        }
    };
    Some((leader, tail))
}

pub(crate) fn parse_quotes(value: &str, inherited: &Quotes) -> Option<Quotes> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(Quotes::auto());
    }
    if value.eq_ignore_ascii_case("none") {
        return Some(Quotes::None);
    }
    if value.eq_ignore_ascii_case("match-parent") {
        return Some(inherited.clone());
    }
    let mut rest = value;
    let mut strings = Vec::new();
    while !rest.trim_start().is_empty() {
        rest = rest.trim_start();
        let (text, tail) = parse_css_string_token(rest)?;
        strings.push(text);
        rest = tail;
    }
    if strings.is_empty() || strings.len() % 2 != 0 {
        return None;
    }
    let pairs = strings
        .chunks_exact(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect::<Vec<_>>();
    Some(Quotes::Pairs(pairs))
}

fn parse_counter_token(value: &str) -> Option<(String, Option<ListStyleType>, &str)> {
    let body = strip_ascii_function(value, "counter")?;
    let (argument, tail) = split_function_argument(body)?;
    let parts = split_top_level_commas(argument);
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    let name = parse_counter_name(parts[0])?;
    let style = if let Some(style) = parts.get(1) {
        Some(parse_list_style_type(style.trim())?)
    } else {
        None
    };
    Some((name, style, tail))
}

fn parse_counters_components(value: &str) -> Option<(String, String, Option<ListStyleType>, &str)> {
    let body = strip_ascii_function(value, "counters")?;
    let (argument, tail) = split_function_argument(body)?;
    let parts = split_top_level_commas(argument);
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let name = parse_counter_name(parts[0])?;
    let separator = parse_counter_separator(parts[1])?;
    let style = if let Some(style) = parts.get(2) {
        Some(parse_list_style_type(style.trim())?)
    } else {
        None
    };
    Some((name, separator, style, tail))
}

fn parse_attr_name(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('"').trim_matches('\'');
    if value.is_empty() || value.split_whitespace().count() != 1 {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

pub(crate) fn parse_counter_name(value: &str) -> Option<String> {
    let value = value.trim();
    let mut chars = value.chars();
    let first = chars.next()?;
    ((first == '_' || first == '-' || first.is_ascii_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '-' || character.is_ascii_alphanumeric()
        }))
    .then(|| value.to_string())
}

pub(crate) fn parse_counter_separator(value: &str) -> Option<String> {
    let (value, tail) = parse_css_string_token(value.trim())?;
    tail.trim().is_empty().then_some(value)
}

pub(crate) fn split_leading_ident(value: &str) -> Option<(&str, &str)> {
    let mut end = None;
    for (index, character) in value.char_indices() {
        if index == 0 && !is_css_ident_start(character) {
            return None;
        }
        if !is_css_ident_continue(character) {
            end = Some(index);
            break;
        }
    }
    let end = end.unwrap_or(value.len());
    Some((&value[..end], &value[end..]))
}

pub(crate) fn split_top_level_commas(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
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
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

fn split_top_level_slash(value: &str) -> (&str, Option<&str>) {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
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
            ')' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => return (&value[..index], Some(&value[index + 1..])),
            _ => {}
        }
    }
    (value, None)
}

pub(crate) fn is_css_ident_start(character: char) -> bool {
    character == '_' || character == '-' || character.is_ascii_alphabetic() || !character.is_ascii()
}
