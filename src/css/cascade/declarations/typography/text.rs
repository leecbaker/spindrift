use super::*;

/// Parse CSS Text Level 4's `text-spacing-trim` keyword.
///
/// <https://drafts.csswg.org/css-text-4/#text-spacing-trim-property>.
pub(in crate::css) fn parse_text_spacing_trim(value: &str) -> Option<TextSpacingTrim> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "space-all" => Some(TextSpacingTrim::SpaceAll),
        "normal" => Some(TextSpacingTrim::Normal),
        "space-first" => Some(TextSpacingTrim::SpaceFirst),
        "trim-start" => Some(TextSpacingTrim::TrimStart),
        "trim-both" => Some(TextSpacingTrim::TrimBoth),
        "trim-all" => Some(TextSpacingTrim::TrimAll),
        "auto" => Some(TextSpacingTrim::Auto),
        _ => None,
    }
}

/// Parse CSS Text Level 4's `text-spacing` shorthand.
///
/// The shorthand owns both text-spacing longhands, so an omitted component
/// resets to that longhand's initial value:
/// <https://drafts.csswg.org/css-text-4/#text-spacing-property>.
pub(in crate::css) fn parse_text_spacing(value: &str) -> Option<(TextSpacingTrim, TextAutospace)> {
    let tokens = split_css_component_values(value);
    if tokens.len() == 1 {
        return match tokens[0].to_ascii_lowercase().as_str() {
            "none" => Some((TextSpacingTrim::SpaceAll, TextAutospace::NONE)),
            "normal" => Some((TextSpacingTrim::Normal, TextAutospace::NORMAL)),
            "auto" => Some((TextSpacingTrim::Auto, TextAutospace::NORMAL)),
            _ => parse_text_spacing_trim(tokens[0])
                .map(|trim| (trim, TextAutospace::NORMAL))
                .or_else(|| {
                    parse_text_autospace(tokens[0])
                        .map(|autospace| (TextSpacingTrim::Normal, autospace))
                }),
        };
    }
    if tokens.is_empty() {
        return None;
    }
    let mut trim = None;
    let mut autospace_tokens = Vec::new();
    for token in tokens {
        if let Some(value) = parse_text_spacing_trim(token) {
            if trim.replace(value).is_some() {
                return None;
            }
        } else {
            autospace_tokens.push(token);
        }
    }
    let autospace = parse_text_autospace(&autospace_tokens.join(" "))?;
    Some((trim.unwrap_or(TextSpacingTrim::Normal), autospace))
}

/// Parse CSS Text Level 4's `text-autospace` keyword set.
///
/// The grammar accepts `normal`, `auto`, `no-autospace`, or an unordered set
/// of autospace features with an optional insertion/replacement mode. The
/// current layout engine uses insertion semantics for both modes because PDF
/// output has no editable text-replacement phase:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>.
pub(in crate::css) fn parse_text_autospace(value: &str) -> Option<TextAutospace> {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 {
        return match tokens[0].to_ascii_lowercase().as_str() {
            "normal" | "auto" => Some(TextAutospace::NORMAL),
            "no-autospace" => Some(TextAutospace::NONE),
            "ideograph-alpha" => Some(TextAutospace {
                ideograph_alpha: true,
                ..TextAutospace::NONE
            }),
            "ideograph-numeric" => Some(TextAutospace {
                ideograph_numeric: true,
                ..TextAutospace::NONE
            }),
            "punctuation" => Some(TextAutospace {
                punctuation: true,
                ..TextAutospace::NONE
            }),
            _ => None,
        };
    }

    let mut autospace = TextAutospace::NONE;
    let mut saw_mode = false;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "normal" | "auto" | "no-autospace" => return None,
            "ideograph-alpha" if !autospace.ideograph_alpha => {
                autospace.ideograph_alpha = true;
            }
            "ideograph-numeric" if !autospace.ideograph_numeric => {
                autospace.ideograph_numeric = true;
            }
            "punctuation" if !autospace.punctuation => {
                autospace.punctuation = true;
            }
            "insert" | "replace" if !saw_mode => {
                saw_mode = true;
            }
            _ => return None,
        }
    }

    (!autospace.is_none()).then_some(autospace)
}

/// Parse CSS Text Level 4's `word-space-transform` keyword set.
///
/// The value is an unordered pair of one replacement keyword and optional
/// `auto-phrase`; `none` cannot be combined with either:
/// <https://drafts.csswg.org/css-text-4/#word-space-transform>.
pub(in crate::css) fn parse_word_space_transform(value: &str) -> Option<WordSpaceTransform> {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("none") {
        return Some(WordSpaceTransform::NONE);
    }
    let mut transform = WordSpaceTransform::NONE;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "space" if transform.replacement.is_none() => {
                transform.replacement = Some(WordSpaceReplacement::Space);
            }
            "ideographic-space" if transform.replacement.is_none() => {
                transform.replacement = Some(WordSpaceReplacement::IdeographicSpace);
            }
            "auto-phrase" if !transform.auto_phrase => transform.auto_phrase = true,
            _ => return None,
        }
    }
    (transform.replacement.is_some() || transform.auto_phrase).then_some(transform)
}

pub(in crate::css) fn parse_text_align_all(
    value: &str,
    inheritance_source: &ComputedStyle,
    allow_justify_all: bool,
) -> Option<TextAlign> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "start" => Some(TextAlign::Start),
        "end" => Some(TextAlign::End),
        "center" => Some(TextAlign::Center),
        "right" => Some(TextAlign::Right),
        "justify" => Some(TextAlign::Justify),
        "justify-all" if allow_justify_all => Some(TextAlign::JustifyAll),
        "left" => Some(TextAlign::Left),
        "match-parent" => Some(resolve_match_parent_text_align(
            inheritance_source.text_align,
            inheritance_source.direction,
        )),
        _ => None,
    }
}

pub(in crate::css) fn parse_text_align_last(
    value: &str,
    inheritance_source: &ComputedStyle,
) -> Option<TextAlignLast> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(TextAlignLast::Auto),
        "start" => Some(TextAlignLast::Align(TextAlign::Start)),
        "end" => Some(TextAlignLast::Align(TextAlign::End)),
        "center" => Some(TextAlignLast::Align(TextAlign::Center)),
        "right" => Some(TextAlignLast::Align(TextAlign::Right)),
        "justify" => Some(TextAlignLast::Align(TextAlign::Justify)),
        "left" => Some(TextAlignLast::Align(TextAlign::Left)),
        "match-parent" => Some(match inheritance_source.text_align_last {
            TextAlignLast::Auto => TextAlignLast::Auto,
            TextAlignLast::Align(align) => TextAlignLast::Align(resolve_match_parent_text_align(
                align,
                inheritance_source.direction,
            )),
        }),
        _ => None,
    }
}

pub(in crate::css) fn resolve_match_parent_text_align(
    align: TextAlign,
    parent_direction: Direction,
) -> TextAlign {
    match align {
        TextAlign::Start | TextAlign::End => align.physical(parent_direction),
        TextAlign::JustifyAll => TextAlign::JustifyAll,
        align => align,
    }
}

/// Parse CSS Text's `text-transform` keyword set.
///
/// CSS Text defines `text-transform` as either `none`, `math-auto`, or a
/// combination of at most one case transform with optional `full-width` and
/// `full-size-kana`:
/// <https://www.w3.org/TR/css-text-3/#text-transform-property>.
pub(in crate::css) fn parse_text_transform(value: &str) -> Option<TextTransform> {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("none") {
        return Some(TextTransform::NONE);
    }
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("math-auto") {
        return Some(TextTransform::MathAuto);
    }

    let mut case = None;
    let mut full_width = false;
    let mut full_size_kana = false;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "none" => return None,
            "uppercase" if case.is_none() => case = Some(TextTransformCase::Uppercase),
            "lowercase" if case.is_none() => case = Some(TextTransformCase::Lowercase),
            "capitalize" if case.is_none() => case = Some(TextTransformCase::Capitalize),
            "full-width" if !full_width => full_width = true,
            "full-size-kana" if !full_size_kana => full_size_kana = true,
            _ => return None,
        }
    }

    TextTransformKeywords::new(case, full_width, full_size_kana).map(TextTransform::Keywords)
}

#[derive(Debug, Clone, Copy)]
pub(in crate::css) struct TextDecorationLineParts {
    pub(in crate::css) underline: bool,
    pub(in crate::css) overline: bool,
    pub(in crate::css) line_through: bool,
    pub(in crate::css) blink: bool,
    pub(in crate::css) spelling_error: bool,
    pub(in crate::css) grammar_error: bool,
}
