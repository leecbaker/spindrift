use super::*;

pub(in crate::css) fn remove_vertical_align_baseline_source(
    value: &str,
) -> Option<(BaselineSource, &str)> {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    for (keyword, source) in [
        ("first", BaselineSource::First),
        ("last", BaselineSource::Last),
    ] {
        if lower == keyword {
            return Some((source, ""));
        }
        if let Some(lower_rest) = lower.strip_prefix(keyword)
            && lower_rest.starts_with(char::is_whitespace)
        {
            let rest = &trimmed[keyword.len()..];
            return Some((source, rest.trim()));
        }
        if let Some(lower_rest) = lower.strip_suffix(keyword)
            && lower_rest.ends_with(char::is_whitespace)
        {
            let rest = &trimmed[..trimmed.len() - keyword.len()];
            return Some((source, rest.trim()));
        }
    }
    Some((BaselineSource::Auto, trimmed))
}

/// Parses the computed CSS `hanging-punctuation` keyword set.
///
/// CSS Text defines the grammar as
/// `none | [ first || [ force-end | allow-end ] || last ]`, with the computed
/// value preserving the specified keywords:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(crate) fn parse_hanging_punctuation(value: &str) -> Option<HangingPunctuation> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(HangingPunctuation::NONE);
    }

    let mut output = HangingPunctuation::NONE;
    let mut saw_keyword = false;
    for keyword in value.split_whitespace() {
        match keyword.to_ascii_lowercase().as_str() {
            "first" if !output.first => output.first = true,
            "last" if !output.last => output.last = true,
            "force-end" if !output.force_end && !output.allow_end => output.force_end = true,
            "allow-end" if !output.force_end && !output.allow_end => output.allow_end = true,
            _ => return None,
        }
        saw_keyword = true;
    }
    saw_keyword.then_some(output)
}

pub(in crate::css) fn remove_unordered_text_indent_keyword(
    value: &str,
    keyword: &str,
) -> (String, bool) {
    let Some(range) = find_top_level_keyword(value, keyword) else {
        return (value.trim().to_string(), false);
    };
    let mut output = String::with_capacity(value.len().saturating_sub(range.len()));
    output.push_str(value[..range.start].trim_end());
    if !output.is_empty() && !value[range.end..].trim_start().is_empty() {
        output.push(' ');
    }
    output.push_str(value[range.end..].trim_start());
    (output.trim().to_string(), true)
}

pub(in crate::css) fn find_top_level_keyword(
    value: &str,
    keyword: &str,
) -> Option<std::ops::Range<usize>> {
    let first = keyword.chars().next()?;
    let mut depth = 0usize;
    for (start, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && character.eq_ignore_ascii_case(&first) => {
                let end = start + keyword.len();
                if value
                    .get(start..end)
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(keyword))
                    && keyword_boundary(value, start, end)
                {
                    return Some(start..end);
                }
            }
            _ => {}
        }
    }
    None
}

pub(in crate::css) fn keyword_boundary(value: &str, start: usize, end: usize) -> bool {
    !value[..start]
        .chars()
        .next_back()
        .is_some_and(is_css_identifier_character)
        && !value[end..]
            .chars()
            .next()
            .is_some_and(is_css_identifier_character)
}

pub(in crate::css) fn is_css_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '-'
}

/// Updates the temporary used line-height projection from the computed value.
///
/// CSS Cascade separates computed values from used values; this keeps the
/// legacy numeric layout fields derived from `ComputedLineHeight` until layout
/// can consume the typed value directly:
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
pub(crate) fn project_line_height(style: &mut ComputedStyle) {
    let (line_height, multiplier, is_normal) = style.line_height_value.projected(style.font_size);
    style.line_height = line_height;
    style.line_height_multiplier = multiplier;
    style.line_height_is_normal = is_normal;
}

pub(crate) fn parse_line_height(value: &str, font_size: f32) -> Option<(f32, Option<f32>, bool)> {
    let mut style = ComputedStyle {
        font_size,
        ..ComputedStyle::initial()
    };
    style.line_height_value = parse_computed_line_height(value, font_size)?;
    project_line_height(&mut style);
    Some((
        style.line_height,
        style.line_height_multiplier,
        style.line_height_is_normal,
    ))
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
