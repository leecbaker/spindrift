use super::*;
pub(in crate::css) fn parse_nonnegative_flex_number(value: &str) -> Option<String> {
    let value = trim_css_value(value);
    let number = parse_css_number(value)?;
    (number >= 0.0).then(|| value.to_string())
}

/// Expands CSS Box Alignment `place-*` shorthands into modeled longhands.
///
/// CSS Box Alignment defines `place-content`, `place-items`, and `place-self`
/// as paired block/inline-axis alignment shorthands:
/// <https://www.w3.org/TR/css-align-3/#place-content-property>,
/// <https://www.w3.org/TR/css-align-3/#place-items-property>, and
/// <https://www.w3.org/TR/css-align-3/#place-self-property>.
pub(in crate::css) fn expand_alignment_place_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    match name {
        "place-content" => {
            let (align, justify) = split_place_content_shorthand(value)?;
            Some(vec![("align-content", align), ("justify-content", justify)])
        }
        "place-items" => {
            let (align, justify) = split_place_shorthand(
                value,
                parse_align_items_keyword,
                parse_justify_items_keyword,
            )?;
            Some(vec![("align-items", align), ("justify-items", justify)])
        }
        "place-self" => {
            let (align, justify) =
                split_place_shorthand(value, parse_align_self_keyword, parse_justify_self_keyword)?;
            Some(vec![("align-self", align), ("justify-self", justify)])
        }
        _ => None,
    }
}

pub(in crate::css) fn split_place_content_shorthand(value: &str) -> Option<(String, String)> {
    let value = trim_css_value(value);
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if let Some(align) = parse_content_alignment_keyword(value, false, true) {
        if parse_justify_content_keyword(value).is_some() {
            return Some((value.to_string(), value.to_string()));
        }
        if matches!(
            align.keyword,
            ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline
        ) {
            return Some((value.to_string(), "start".to_string()));
        }
    }
    for split in 1..tokens.len() {
        let align = tokens[..split].join(" ");
        let justify = tokens[split..].join(" ");
        if parse_align_content_keyword(&align).is_some()
            && parse_justify_content_keyword(&justify).is_some()
        {
            return Some((align, justify));
        }
    }
    None
}

pub(in crate::css) fn split_place_shorthand<A, J>(
    value: &str,
    parse_align: A,
    parse_justify: J,
) -> Option<(String, String)>
where
    A: Fn(&str) -> Option<()>,
    J: Fn(&str) -> Option<()>,
{
    let value = trim_css_value(value);
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if parse_align(value).is_some() && parse_justify(value).is_some() {
        return Some((value.to_string(), value.to_string()));
    }
    for split in 1..tokens.len() {
        let align = tokens[..split].join(" ");
        let justify = tokens[split..].join(" ");
        if parse_align(&align).is_some() && parse_justify(&justify).is_some() {
            return Some((align, justify));
        }
    }
    None
}

pub(in crate::css) fn parse_alignment_safety_and_keyword(value: &str) -> (AlignmentSafety, String) {
    let mut parts = split_css_component_values(value);
    let safety = match parts.first().map(|part| part.to_ascii_lowercase()) {
        Some(keyword) if keyword == "safe" => {
            parts.remove(0);
            AlignmentSafety::Safe
        }
        Some(keyword) if keyword == "unsafe" => {
            parts.remove(0);
            AlignmentSafety::Unsafe
        }
        _ => AlignmentSafety::Default,
    };
    let keyword = parts
        .into_iter()
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    (safety, keyword)
}

pub(in crate::css) fn content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> ContentAlignment {
    match safety {
        AlignmentSafety::Default => ContentAlignment::new(keyword),
        AlignmentSafety::Unsafe => ContentAlignment::unsafe_position(keyword),
        AlignmentSafety::Safe => ContentAlignment::safe(keyword),
    }
}

pub(in crate::css) fn self_alignment(
    keyword: SelfAlignmentKeyword,
    safety: AlignmentSafety,
) -> SelfAlignment {
    match safety {
        AlignmentSafety::Default => SelfAlignment::new(keyword),
        AlignmentSafety::Unsafe => SelfAlignment::unsafe_position(keyword),
        AlignmentSafety::Safe => SelfAlignment::safe(keyword),
    }
}

pub(in crate::css) fn alignment_safety_allowed_for_content(
    keyword: ContentAlignmentKeyword,
) -> bool {
    matches!(
        keyword,
        ContentAlignmentKeyword::Normal
            | ContentAlignmentKeyword::Start
            | ContentAlignmentKeyword::End
            | ContentAlignmentKeyword::FlexStart
            | ContentAlignmentKeyword::FlexEnd
            | ContentAlignmentKeyword::Left
            | ContentAlignmentKeyword::Right
            | ContentAlignmentKeyword::Center
    )
}

pub(in crate::css) fn alignment_safety_allowed_for_self(keyword: SelfAlignmentKeyword) -> bool {
    matches!(
        keyword,
        SelfAlignmentKeyword::Normal
            | SelfAlignmentKeyword::Start
            | SelfAlignmentKeyword::End
            | SelfAlignmentKeyword::SelfStart
            | SelfAlignmentKeyword::SelfEnd
            | SelfAlignmentKeyword::FlexStart
            | SelfAlignmentKeyword::FlexEnd
            | SelfAlignmentKeyword::Left
            | SelfAlignmentKeyword::Right
            | SelfAlignmentKeyword::Center
    )
}

pub(in crate::css) fn parse_content_alignment_keyword(
    value: &str,
    allow_left_right: bool,
    allow_baseline: bool,
) -> Option<ContentAlignment> {
    let (safety, keyword) = parse_alignment_safety_and_keyword(value);
    let keyword = match keyword.as_str() {
        "normal" => ContentAlignmentKeyword::Normal,
        "center" => ContentAlignmentKeyword::Center,
        "space-between" => ContentAlignmentKeyword::SpaceBetween,
        "space-around" => ContentAlignmentKeyword::SpaceAround,
        "space-evenly" => ContentAlignmentKeyword::SpaceEvenly,
        "stretch" => ContentAlignmentKeyword::Stretch,
        "flex-start" => ContentAlignmentKeyword::FlexStart,
        "flex-end" => ContentAlignmentKeyword::FlexEnd,
        "start" => ContentAlignmentKeyword::Start,
        "end" => ContentAlignmentKeyword::End,
        "left" if allow_left_right => ContentAlignmentKeyword::Left,
        "right" if allow_left_right => ContentAlignmentKeyword::Right,
        "baseline" | "first baseline" if allow_baseline => ContentAlignmentKeyword::Baseline,
        "last baseline" if allow_baseline => ContentAlignmentKeyword::LastBaseline,
        _ => return None,
    };
    if safety != AlignmentSafety::Default && !alignment_safety_allowed_for_content(keyword) {
        return None;
    }
    Some(content_alignment(keyword, safety))
}

pub(in crate::css) fn parse_self_alignment_keyword(
    value: &str,
    allow_auto: bool,
    allow_left_right: bool,
) -> Option<SelfAlignment> {
    let (safety, keyword) = parse_alignment_safety_and_keyword(value);
    let keyword = match keyword.as_str() {
        "auto" if allow_auto => SelfAlignmentKeyword::Auto,
        "normal" => SelfAlignmentKeyword::Normal,
        "stretch" => SelfAlignmentKeyword::Stretch,
        "center" => SelfAlignmentKeyword::Center,
        "flex-start" => SelfAlignmentKeyword::FlexStart,
        "flex-end" => SelfAlignmentKeyword::FlexEnd,
        "start" => SelfAlignmentKeyword::Start,
        "end" => SelfAlignmentKeyword::End,
        "self-start" => SelfAlignmentKeyword::SelfStart,
        "self-end" => SelfAlignmentKeyword::SelfEnd,
        "left" if allow_left_right => SelfAlignmentKeyword::Left,
        "right" if allow_left_right => SelfAlignmentKeyword::Right,
        "baseline" | "first baseline" => SelfAlignmentKeyword::Baseline,
        "last baseline" => SelfAlignmentKeyword::LastBaseline,
        _ => return None,
    };
    if safety != AlignmentSafety::Default && !alignment_safety_allowed_for_self(keyword) {
        return None;
    }
    Some(self_alignment(keyword, safety))
}

pub(in crate::css) fn parse_justify_content_keyword(value: &str) -> Option<()> {
    parse_content_alignment_keyword(value, true, false).map(|_| ())
}

pub(in crate::css) fn parse_align_content_keyword(value: &str) -> Option<()> {
    parse_content_alignment_keyword(value, false, true).map(|_| ())
}

pub(in crate::css) fn parse_align_items_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, false, false).map(|_| ())
}

pub(in crate::css) fn parse_align_self_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, true, false).map(|_| ())
}

pub(in crate::css) fn parse_justify_items_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, false, true).map(|_| ())
}

pub(in crate::css) fn parse_justify_self_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, true, true).map(|_| ())
}

pub(in crate::css) fn parse_justify_content(
    value: &str,
    current: JustifyContent,
) -> JustifyContent {
    parse_content_alignment_keyword(value, true, false).unwrap_or(current)
}

pub(in crate::css) fn parse_align_content(value: &str, current: AlignContent) -> AlignContent {
    parse_content_alignment_keyword(value, false, true).unwrap_or(current)
}

pub(in crate::css) fn parse_align_items(value: &str, current: AlignItems) -> AlignItems {
    parse_self_alignment_keyword(value, false, false).unwrap_or(current)
}

pub(in crate::css) fn parse_align_self(value: &str, current: AlignSelf) -> AlignSelf {
    parse_self_alignment_keyword(value, true, false).unwrap_or(current)
}

pub(in crate::css) fn parse_justify_items(value: &str, current: JustifyItems) -> JustifyItems {
    parse_self_alignment_keyword(value, false, true).unwrap_or(current)
}

pub(in crate::css) fn parse_justify_self(value: &str, current: JustifySelf) -> JustifySelf {
    parse_self_alignment_keyword(value, true, true).unwrap_or(current)
}
