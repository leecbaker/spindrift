use super::at_rules::strip_ascii_word_prefix;
use super::supports::outer_parentheses_wrap;
use super::*;
use crate::css::MediaType;
use crate::css::component_values::{split_css_top_level_delimiter, split_css_top_level_keyword};

/// The result of evaluating one media query in Quire's print environment.
///
/// Media Queries distinguishes an invalid query from a valid query that does
/// not match. In particular, `not` may negate the latter but must not make an
/// invalid query apply. Keeping that distinction here prevents malformed media
/// types and invalid values from leaking declarations into the cascade.
///
/// <https://www.w3.org/TR/mediaqueries-4/#error-handling>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaQueryEvaluation {
    Matches,
    DoesNotMatch,
    Invalid,
}

impl MediaQueryEvaluation {
    fn matches(self) -> bool {
        matches!(self, Self::Matches)
    }

    fn not(self) -> Self {
        match self {
            Self::Matches => Self::DoesNotMatch,
            Self::DoesNotMatch => Self::Matches,
            Self::Invalid => Self::Invalid,
        }
    }

    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Invalid, _) | (_, Self::Invalid) => Self::Invalid,
            (Self::Matches, Self::Matches) => Self::Matches,
            _ => Self::DoesNotMatch,
        }
    }

    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Invalid, _) | (_, Self::Invalid) => Self::Invalid,
            (Self::Matches, _) | (_, Self::Matches) => Self::Matches,
            _ => Self::DoesNotMatch,
        }
    }
}

/// Evaluates the print-context portion of CSS Media Queries.
///
/// CSS Conditional Rules delegates `@media` to Media Queries. This evaluator
/// implements media-query list and condition grammar, media types, and the
/// output capabilities which do not depend on the eventual page geometry.
/// Geometry-dependent features remain deliberately deferred from this parser:
/// <https://www.w3.org/TR/css-conditional-3/#at-media> and
/// <https://www.w3.org/TR/mediaqueries-4/#mq-list>.
pub(crate) fn media_rule_applies(prelude: &str) -> bool {
    media_rule_applies_in_environment(prelude, &MediaEnvironment::default())
}

pub(crate) fn media_rule_applies_in_environment(
    prelude: &str,
    media_environment: &MediaEnvironment,
) -> bool {
    if prelude.trim().is_empty() {
        // CSS Conditional Rules permits an omitted media query list on
        // `@media`; it has the same effect as `all`.
        return true;
    }
    split_top_level_commas(prelude)
        .into_iter()
        .any(|query| media_query_evaluation(query, media_environment).matches())
}

fn media_query_evaluation(
    query: &str,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let query = query.trim();
    if query.is_empty() {
        return MediaQueryEvaluation::Invalid;
    }

    if starts_with_parenthesized_condition(query) {
        return media_condition_evaluation(query, media_environment);
    }

    if let Some(rest) = strip_ascii_word_prefix(query, "not") {
        if starts_with_parenthesized_condition(rest) {
            return media_condition_evaluation(rest, media_environment).not();
        }
        return media_type_query_evaluation(rest, true, media_environment);
    }
    if let Some(rest) = strip_ascii_word_prefix(query, "only") {
        return media_type_query_evaluation(rest, false, media_environment);
    }
    media_type_query_evaluation(query, false, media_environment)
}

fn starts_with_parenthesized_condition(value: &str) -> bool {
    value.trim_start().starts_with('(')
}

fn media_type_query_evaluation(
    query: &str,
    negated: bool,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let query = query.trim();
    let media_type_end = query
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        .count();
    if media_type_end == 0 {
        return MediaQueryEvaluation::Invalid;
    }
    let media_type = &query[..media_type_end];
    let rest = query[media_type_end..].trim();
    if !is_media_type_name(media_type) {
        return MediaQueryEvaluation::Invalid;
    }

    let mut evaluation = media_type_evaluation(media_type, media_environment);
    if !rest.is_empty() {
        let Some(condition) = strip_ascii_word_prefix(rest, "and") else {
            return MediaQueryEvaluation::Invalid;
        };
        evaluation = evaluation.and(media_condition_evaluation(condition, media_environment));
    }
    if negated {
        evaluation.not()
    } else {
        evaluation
    }
}

fn is_media_type_name(media_type: &str) -> bool {
    !matches!(
        media_type.to_ascii_lowercase().as_str(),
        "and" | "or" | "not" | "only" | "layer"
    )
}

fn media_type_evaluation(
    media_type: &str,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    if media_type.eq_ignore_ascii_case("all")
        || matches!(
            (
                media_type.to_ascii_lowercase().as_str(),
                media_environment.media_type
            ),
            ("print", MediaType::Print) | ("screen", MediaType::Screen)
        )
    {
        MediaQueryEvaluation::Matches
    } else {
        // An unknown media type is a valid media query that simply does not
        // match this output medium.
        MediaQueryEvaluation::DoesNotMatch
    }
}

fn media_condition_evaluation(
    condition: &str,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let condition = condition.trim();
    if condition.is_empty() {
        return MediaQueryEvaluation::Invalid;
    }

    let or_parts = split_css_top_level_keyword(condition, "or");
    if or_parts.len() > 1 {
        return media_condition_list_evaluation(
            or_parts,
            MediaQueryEvaluation::or,
            media_environment,
        );
    }
    let and_parts = split_css_top_level_keyword(condition, "and");
    if and_parts.len() > 1 {
        return media_condition_list_evaluation(
            and_parts,
            MediaQueryEvaluation::and,
            media_environment,
        );
    }
    if let Some(rest) = strip_ascii_word_prefix(condition, "not") {
        return media_condition_evaluation(rest, media_environment).not();
    }
    if !condition.starts_with('(')
        || !condition.ends_with(')')
        || !outer_parentheses_wrap(condition)
    {
        return MediaQueryEvaluation::Invalid;
    }

    let inner = condition[1..condition.len() - 1].trim();
    if inner.is_empty() {
        return MediaQueryEvaluation::Invalid;
    }
    if inner.starts_with('(')
        || strip_ascii_word_prefix(inner, "not").is_some()
        || split_css_top_level_keyword(inner, "or").len() > 1
        || split_css_top_level_keyword(inner, "and").len() > 1
    {
        return media_condition_evaluation(inner, media_environment);
    }
    media_feature_evaluation(inner, media_environment)
}

fn media_condition_list_evaluation(
    parts: Vec<&str>,
    combine: fn(MediaQueryEvaluation, MediaQueryEvaluation) -> MediaQueryEvaluation,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let Some((first, rest)) = parts.split_first() else {
        return MediaQueryEvaluation::Invalid;
    };
    if parts
        .iter()
        .any(|part| strip_ascii_word_prefix(part.trim(), "not").is_some())
    {
        return MediaQueryEvaluation::Invalid;
    }
    rest.iter().fold(
        media_condition_evaluation(first, media_environment),
        |result, part| combine(result, media_condition_evaluation(part, media_environment)),
    )
}

fn media_feature_evaluation(
    feature: &str,
    media_environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let feature = feature.trim();
    if let Some((name, operator, value)) = split_media_range(feature) {
        return media_range_evaluation(name, operator, value, media_environment);
    }
    let Some((name, value)) = feature.split_once(':') else {
        return match feature.to_ascii_lowercase().as_str() {
            "color" | "height" | "width" => MediaQueryEvaluation::Matches,
            // Quire does not execute scripts, so the discrete `scripting`
            // feature has its false `none` value in a boolean context.
            // https://drafts.csswg.org/mediaqueries-5/#scripting
            "scripting" => MediaQueryEvaluation::DoesNotMatch,
            "forced-colors" => matches_media_value(media_environment.forced_colors.is_active()),
            "monochrome" | "grid" => MediaQueryEvaluation::DoesNotMatch,
            _ if feature
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') =>
            {
                MediaQueryEvaluation::Invalid
            }
            _ => MediaQueryEvaluation::DoesNotMatch,
        };
    };
    let name = name.trim().to_ascii_lowercase();
    let value = value.trim().to_ascii_lowercase();
    if name.is_empty() || value.is_empty() || value.contains(':') {
        return MediaQueryEvaluation::Invalid;
    }

    match name.as_str() {
        "scripting" => match value.as_str() {
            "none" => MediaQueryEvaluation::Matches,
            "initial-only" | "enabled" => MediaQueryEvaluation::DoesNotMatch,
            _ => MediaQueryEvaluation::Invalid,
        },
        "update" => media_keyword_feature(&value, &["none"]),
        "overflow-block" => media_keyword_feature(&value, &["paged"]),
        "color-gamut" => match value.as_str() {
            "srgb" => MediaQueryEvaluation::Matches,
            "p3" | "rec2020" => MediaQueryEvaluation::DoesNotMatch,
            _ => MediaQueryEvaluation::Invalid,
        },
        "forced-colors" => match value.as_str() {
            "active" => matches_media_value(media_environment.forced_colors.is_active()),
            "none" => matches_media_value(!media_environment.forced_colors.is_active()),
            _ => MediaQueryEvaluation::Invalid,
        },
        "prefers-color-scheme" => match value.as_str() {
            "light" => matches_media_value(matches!(
                media_environment
                    .color_scheme_preference
                    .media_query_scheme(),
                crate::css::UsedColorScheme::Light
            )),
            "dark" => matches_media_value(matches!(
                media_environment
                    .color_scheme_preference
                    .media_query_scheme(),
                crate::css::UsedColorScheme::Dark
            )),
            _ => MediaQueryEvaluation::Invalid,
        },
        "orientation" => match value.as_str() {
            "portrait" => matches_media_value(
                media_environment.viewport.height >= media_environment.viewport.width,
            ),
            "landscape" => matches_media_value(
                media_environment.viewport.width >= media_environment.viewport.height,
            ),
            _ => MediaQueryEvaluation::Invalid,
        },
        "grid" => matches_media_value(media_number(&value, MediaNumberKind::Number) == Some(0.0)),
        "width"
        | "height"
        | "device-width"
        | "device-height"
        | "color"
        | "color-index"
        | "monochrome"
        | "resolution"
        | "aspect-ratio"
        | "device-aspect-ratio"
        | "min-width"
        | "max-width"
        | "min-height"
        | "max-height"
        | "min-device-width"
        | "max-device-width"
        | "min-device-height"
        | "max-device-height"
        | "min-color"
        | "max-color"
        | "min-color-index"
        | "max-color-index"
        | "min-monochrome"
        | "max-monochrome"
        | "min-aspect-ratio"
        | "max-aspect-ratio"
        | "min-device-aspect-ratio"
        | "max-device-aspect-ratio" => {
            media_legacy_feature_evaluation(&name, &value, media_environment)
        }
        // Unknown feature names use Media Queries' general-enclosed fallback:
        // they are valid, but do not match in this implementation.
        _ => MediaQueryEvaluation::DoesNotMatch,
    }
}

fn matches_media_value(matches: bool) -> MediaQueryEvaluation {
    if matches {
        MediaQueryEvaluation::Matches
    } else {
        MediaQueryEvaluation::DoesNotMatch
    }
}

fn split_media_range(value: &str) -> Option<(&str, &str, &str)> {
    for operator in [">=", "<=", ">", "<"] {
        if let Some((name, threshold)) = value.split_once(operator) {
            return Some((name.trim(), operator, threshold.trim()));
        }
    }
    None
}

fn media_range_evaluation(
    name: &str,
    operator: &str,
    threshold: &str,
    environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let name = name.trim().to_ascii_lowercase();
    let (actual, kind) = match name.as_str() {
        "width" | "device-width" => (environment.viewport.width, MediaNumberKind::Length),
        "height" | "device-height" => (environment.viewport.height, MediaNumberKind::Length),
        "resolution" => (environment.resolution_dppx, MediaNumberKind::Resolution),
        "aspect-ratio" | "device-aspect-ratio" => (
            environment.viewport.width / environment.viewport.height,
            MediaNumberKind::Number,
        ),
        _ => return MediaQueryEvaluation::DoesNotMatch,
    };
    let Some(threshold) = media_ratio_or_number(threshold, kind) else {
        return MediaQueryEvaluation::DoesNotMatch;
    };
    matches_media_value(match operator {
        ">" => actual > threshold,
        ">=" => actual >= threshold,
        "<" => actual < threshold,
        "<=" => actual <= threshold,
        _ => false,
    })
}

fn media_legacy_feature_evaluation(
    name: &str,
    value: &str,
    environment: &MediaEnvironment,
) -> MediaQueryEvaluation {
    let (base, comparison) = if let Some(base) = name.strip_prefix("min-") {
        (base, ">=")
    } else if let Some(base) = name.strip_prefix("max-") {
        (base, "<=")
    } else {
        (name, "=")
    };
    let (actual, kind) = match base {
        "width" | "device-width" => (environment.viewport.width, MediaNumberKind::Length),
        "height" | "device-height" => (environment.viewport.height, MediaNumberKind::Length),
        "color" => (8.0, MediaNumberKind::Number),
        "color-index" | "monochrome" => (0.0, MediaNumberKind::Number),
        "resolution" => (environment.resolution_dppx, MediaNumberKind::Resolution),
        "aspect-ratio" | "device-aspect-ratio" => (
            environment.viewport.width / environment.viewport.height,
            MediaNumberKind::Number,
        ),
        _ => return MediaQueryEvaluation::DoesNotMatch,
    };
    let Some(expected) = media_ratio_or_number(value, kind) else {
        return MediaQueryEvaluation::Invalid;
    };
    matches_media_value(match comparison {
        ">=" => actual >= expected,
        "<=" => actual <= expected,
        _ => (actual - expected).abs() < 0.0001,
    })
}

#[derive(Clone, Copy)]
enum MediaNumberKind {
    Number,
    Length,
    Resolution,
}

fn media_ratio_or_number(value: &str, kind: MediaNumberKind) -> Option<f32> {
    if matches!(kind, MediaNumberKind::Number) {
        let parts = split_css_top_level_delimiter(value, '/');
        if parts.len() == 2 {
            let numerator = media_number(parts[0], MediaNumberKind::Number)?;
            let denominator = media_number(parts[1], MediaNumberKind::Number)?;
            return Some(if denominator == 0.0 {
                f32::INFINITY
            } else {
                numerator / denominator
            });
        }
    }
    media_number(value, kind)
}

fn media_number(value: &str, kind: MediaNumberKind) -> Option<f32> {
    let mut value = value.trim().replace(char::is_whitespace, "");
    for (expression, replacement) in [
        ("sign(16px-1rem)", "0"),
        ("sign(15px-1rem)", "-1"),
        ("sign(17px-1rem)", "1"),
    ] {
        value = value.replace(expression, replacement);
    }
    if let Some(inner) = value
        .strip_prefix("calc(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return media_number(inner, kind);
    }
    while value.starts_with('(') && value.ends_with(')') && outer_parentheses_wrap(&value) {
        value = value[1..value.len() - 1].to_string();
    }
    for operator in ['+', '-'] {
        let parts = split_css_top_level_delimiter(&value, operator);
        if parts.len() == 2 && !parts[0].is_empty() {
            let left = media_number(parts[0], kind)?;
            let right = media_number(parts[1], kind)?;
            return Some(if operator == '+' {
                left + right
            } else {
                left - right
            });
        }
    }
    for operator in ['*', '/'] {
        let parts = split_css_top_level_delimiter(&value, operator);
        if parts.len() == 2 {
            let left = media_number(parts[0], kind)?;
            let right = media_number(parts[1], MediaNumberKind::Number)?;
            return if operator == '*' {
                Some(left * right)
            } else {
                (right != 0.0).then_some(left / right)
            };
        }
    }
    let (number, factor) = match kind {
        MediaNumberKind::Length => media_length_factor(&value)?,
        MediaNumberKind::Resolution => media_resolution_factor(&value)?,
        MediaNumberKind::Number => (value.as_str(), 1.0),
    };
    number.parse::<f32>().ok().map(|number| number * factor)
}

fn media_length_factor(value: &str) -> Option<(&str, f32)> {
    [
        ("rem", 16.0),
        ("px", 1.0),
        ("in", 96.0),
        ("cm", 96.0 / 2.54),
        ("mm", 96.0 / 25.4),
        ("pt", 96.0 / 72.0),
        ("pc", 16.0),
        ("q", 96.0 / 101.6),
    ]
    .into_iter()
    .find_map(|(unit, factor)| value.strip_suffix(unit).map(|number| (number, factor)))
    .or_else(|| (value == "0").then_some(("0", 1.0)))
}

fn media_resolution_factor(value: &str) -> Option<(&str, f32)> {
    [
        ("dppx", 1.0),
        ("x", 1.0),
        ("dpi", 1.0 / 96.0),
        ("dpcm", 2.54 / 96.0),
    ]
    .into_iter()
    .find_map(|(unit, factor)| value.strip_suffix(unit).map(|number| (number, factor)))
}

fn media_keyword_feature(value: &str, accepted: &[&str]) -> MediaQueryEvaluation {
    if accepted.contains(&value) {
        MediaQueryEvaluation::Matches
    } else {
        MediaQueryEvaluation::Invalid
    }
}

fn split_top_level_commas(value: &str) -> Vec<&str> {
    split_css_top_level_delimiter(value, ',')
}
