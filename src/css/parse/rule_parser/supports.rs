use super::at_rules::{
    matching_parenthesis, parse_parenthesized_selector, parse_scope_selector,
    strip_ascii_word_prefix,
};
use super::declarations::supports_declaration_condition;
use super::*;

/// Evaluates the supported subset of CSS Conditional Rules `@supports`.
///
/// CSS Conditional Rules Level 3 defines support conditions as declaration
/// tests combined with `not`, `and`, and `or`. This evaluator is intentionally
/// conservative: unknown condition forms and unknown properties evaluate false
/// rather than letting unsupported blocks leak into the cascade:
/// <https://www.w3.org/TR/css-conditional-3/#at-supports>.
pub(crate) fn supports_condition_applies(prelude: &str) -> bool {
    supports_condition_applies_with_selector_parser(prelude, &QuireSelectorParser::default())
}

pub(in crate::css) fn supports_condition_applies_with_selector_parser(
    prelude: &str,
    selector_parser: &QuireSelectorParser,
) -> bool {
    parse_supports_condition(prelude, selector_parser).is_some_and(|condition| condition.applies())
}

/// A parsed CSS Conditional Rules support condition.
///
/// Keeping the syntax tree separate from evaluation is important here: an
/// invalid condition is not a false declaration test which may be negated;
/// it invalidates the containing `@supports` rule.
/// <https://www.w3.org/TR/css-conditional-3/#at-supports>
enum SupportsCondition {
    Value(bool),
    Not(Box<Self>),
    And(Vec<Self>),
    Or(Vec<Self>),
}

impl SupportsCondition {
    fn applies(&self) -> bool {
        match self {
            Self::Value(value) => *value,
            Self::Not(condition) => !condition.applies(),
            Self::And(conditions) => conditions.iter().all(Self::applies),
            Self::Or(conditions) => conditions.iter().any(Self::applies),
        }
    }
}

fn parse_supports_condition(
    prelude: &str,
    selector_parser: &QuireSelectorParser,
) -> Option<SupportsCondition> {
    parse_supports_condition_component(prelude.trim(), selector_parser, true)
}

fn parse_supports_condition_component(
    value: &str,
    selector_parser: &QuireSelectorParser,
    allow_selector_function: bool,
) -> Option<SupportsCondition> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Some(rest) = strip_supports_not_prefix(value) {
        return Some(SupportsCondition::Not(Box::new(
            parse_supports_condition_component(rest, selector_parser, false)?,
        )));
    }

    if allow_selector_function
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        return supports_selector_condition(value, selector_parser)
            .then_some(SupportsCondition::Value(true));
    }

    if supports_condition_mixes_logical_keywords(value) {
        return None;
    }
    if let Some(condition) = parse_supports_logical_condition(value, selector_parser) {
        return Some(condition);
    }

    if !value.starts_with('(') {
        return None;
    }
    let close = matching_parenthesis(value, 0)?;
    if !value[close + 1..].trim().is_empty() {
        return None;
    }
    parse_supports_in_parens(&value[1..close], selector_parser)
}

fn parse_supports_in_parens(
    value: &str,
    selector_parser: &QuireSelectorParser,
) -> Option<SupportsCondition> {
    let value = value.trim();
    if supports_condition_mixes_logical_keywords(value) {
        return None;
    }
    if let Some(condition) = parse_supports_logical_condition(value, selector_parser) {
        return Some(condition);
    }
    if let Some(rest) = strip_supports_not_prefix(value) {
        return Some(SupportsCondition::Not(Box::new(
            parse_supports_condition_component(rest, selector_parser, false)?,
        )));
    }
    Some(SupportsCondition::Value(supports_declaration_condition(
        value,
    )))
}

/// Parse one top-level CSS Conditional Rules logical sequence.
///
/// A feature query's outermost `and` or `or` sequence is not itself wrapped
/// in parentheses: `(writing-mode: vertical-lr) and (direction: rtl)` is the
/// standard spelling. Parentheses merely delimit the declaration tests, so
/// recognizing logical keywords only after stripping an enclosing pair
/// incorrectly rejects valid unwrapped sequences.
/// <https://www.w3.org/TR/css-conditional-3/#at-supports>
fn parse_supports_logical_condition(
    value: &str,
    selector_parser: &QuireSelectorParser,
) -> Option<SupportsCondition> {
    let and_parts = split_supports_logical_keyword(value, "and");
    let or_parts = split_supports_logical_keyword(value, "or");
    if and_parts.len() > 1 {
        return and_parts
            .into_iter()
            .map(|part| parse_supports_condition_component(part, selector_parser, false))
            .collect::<Option<Vec<_>>>()
            .map(SupportsCondition::And);
    }
    if or_parts.len() > 1 {
        return or_parts
            .into_iter()
            .map(|part| parse_supports_condition_component(part, selector_parser, false))
            .collect::<Option<Vec<_>>>()
            .map(SupportsCondition::Or);
    }
    None
}

fn supports_condition_mixes_logical_keywords(value: &str) -> bool {
    split_supports_logical_keyword(value, "and").len() > 1
        && split_supports_logical_keyword(value, "or").len() > 1
}

/// CSS Conditional's logical keywords are identifiers, not generic word
/// boundaries.  They therefore need whitespace on both sides; `or(` is a
/// functional notation token rather than the `or` combinator.
fn split_supports_logical_keyword<'a>(value: &'a str, keyword: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let bytes = value.as_bytes();
    let keyword = keyword.as_bytes();
    let mut index = 0;
    while index + keyword.len() <= bytes.len() {
        match bytes[index] {
            b'\'' | b'"' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() && bytes[index] != quote {
                    index += usize::from(bytes[index] == b'\\') + 1;
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ if depth == 0
                && bytes[index..].starts_with(keyword)
                && index > 0
                && bytes[index - 1].is_ascii_whitespace()
                && bytes
                    .get(index + keyword.len())
                    .is_some_and(|byte| byte.is_ascii_whitespace()) =>
            {
                parts.push(value[start..index].trim());
                start = index + keyword.len();
                index = start;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(value[start..].trim());
    parts
}

fn strip_supports_not_prefix(value: &str) -> Option<&str> {
    let rest = value
        .strip_prefix("not")
        .or_else(|| value.strip_prefix("NOT"))?;
    let whitespace = rest.chars().next()?.is_ascii_whitespace();
    whitespace
        .then_some(rest.trim_start())
        .filter(|rest| !rest.is_empty())
}

/// Evaluates CSS Conditional `@supports selector(...)` with the selector parser.
///
/// Conditional Rules defines selector feature queries as true when the selector
/// argument parses as a supported selector; unsupported selectors evaluate
/// false and keep the block out of the cascade:
/// <https://www.w3.org/TR/css-conditional-4/#typedef-supports-selector-fn>.
pub(in crate::css) fn supports_selector_condition(
    condition: &str,
    selector_parser: &QuireSelectorParser,
) -> bool {
    let condition = condition.trim();
    let Some(rest) = strip_ascii_word_prefix(condition, "selector") else {
        return false;
    };
    let Some((selector, after_selector)) = parse_parenthesized_selector(rest) else {
        return false;
    };
    if !after_selector.trim().is_empty() {
        return false;
    }
    parse_scope_selector(selector, selector_parser)
        .is_some_and(|selector| selector.slice().len() == 1)
}

pub(in crate::css) fn strip_enclosing_parentheses(value: &str) -> &str {
    let mut value = value.trim();
    while value.starts_with('(') && value.ends_with(')') && outer_parentheses_wrap(value) {
        value = value[1..value.len() - 1].trim();
    }
    value
}

pub(in crate::css) fn outer_parentheses_wrap(value: &str) -> bool {
    let mut depth = 0usize;
    for (index, byte) in value.as_bytes().iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && index != value.len() - 1 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}
