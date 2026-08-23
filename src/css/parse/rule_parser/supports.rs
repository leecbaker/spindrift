use cssparser::Token;

use super::at_rules::{matching_parenthesis, parse_scope_selector};
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
    parse_supports_condition_component(prelude.trim(), selector_parser)
}

fn parse_supports_condition_component(
    value: &str,
    selector_parser: &QuireSelectorParser,
) -> Option<SupportsCondition> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Some(rest) = strip_supports_not_prefix(value) {
        return Some(SupportsCondition::Not(Box::new(
            parse_supports_condition_component(rest, selector_parser)?,
        )));
    }

    if let SelectorFeature::Value(value) = parse_selector_feature(value, selector_parser) {
        return Some(SupportsCondition::Value(value));
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
            parse_supports_condition_component(rest, selector_parser)?,
        )));
    }
    if let SelectorFeature::Value(value) = parse_selector_feature(value, selector_parser) {
        return Some(SupportsCondition::Value(value));
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
            .map(|part| parse_supports_condition_component(part, selector_parser))
            .collect::<Option<Vec<_>>>()
            .map(SupportsCondition::And);
    }
    if or_parts.len() > 1 {
        return or_parts
            .into_iter()
            .map(|part| parse_supports_condition_component(part, selector_parser))
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

/// The result of recognizing a CSS Conditional Rules `selector()` feature.
///
/// A function token whose name is `selector` is a valid forward-compatible
/// supports term even when its argument is not a supported `<complex-selector>`.
/// It therefore evaluates false rather than invalidating an enclosing `not`,
/// `and`, or `or` condition:
/// <https://drafts.csswg.org/css-conditional-4/#typedef-supports-selector-fn>
/// <https://drafts.csswg.org/css-conditional-3/#at-supports>
enum SelectorFeature {
    NotSelectorFunction,
    Value(bool),
}

/// Recognize and evaluate CSS Conditional Rules `selector(...)` using CSS
/// tokenization rather than a string prefix. The selector grammar includes the
/// nesting selector, which behaves like `:scope` outside a nested style rule:
/// <https://drafts.csswg.org/css-nesting-1/#nest-selector>
fn parse_selector_feature(
    condition: &str,
    selector_parser: &QuireSelectorParser,
) -> SelectorFeature {
    let mut input = ParserInput::new(condition);
    let mut parser = Parser::new(&mut input);
    let Ok(Token::Function(name)) = parser.next() else {
        return SelectorFeature::NotSelectorFunction;
    };
    if !name.eq_ignore_ascii_case("selector") {
        return SelectorFeature::NotSelectorFunction;
    }
    let selector = parser.parse_nested_block(|input| {
        let start = input.position();
        while input.next_including_whitespace_and_comments().is_ok() {}
        Ok::<_, cssparser::ParseError<'_, ()>>(input.slice_from(start).to_string())
    });
    let Ok(selector) = selector else {
        return SelectorFeature::NotSelectorFunction;
    };
    if !parser.is_exhausted() {
        return SelectorFeature::NotSelectorFunction;
    }

    let selector_parser = selector_parser.clone().with_parent_selector();
    SelectorFeature::Value(selector_is_supported_for_feature_query(
        &selector,
        &selector_parser,
    ))
}

/// Check selector support through the same pseudo-element routing boundary
/// used for ordinary style rules. Nested generated marker selectors are not
/// representable by the underlying Selectors parser alone, but Quire accepts
/// and routes them as `::before`/`::after` marker rules.
/// <https://drafts.csswg.org/css-conditional-4/#typedef-supports-selector-fn>
fn selector_is_supported_for_feature_query(
    selector: &str,
    selector_parser: &QuireSelectorParser,
) -> bool {
    if parse_scope_selector(selector, selector_parser)
        .is_some_and(|selector| selector.slice().len() == 1)
    {
        return true;
    }

    let selectors = super::pseudo_elements::split_selector_list(selector);
    let [selector] = selectors.as_slice() else {
        return false;
    };
    [
        "before::marker",
        "after::marker",
        "marker",
        "before",
        "after",
        "footnote-call",
        "footnote-marker",
        "first-line",
        "first-letter",
    ]
    .into_iter()
    .filter_map(|pseudo| super::pseudo_elements::strip_pseudo_selector(selector, pseudo))
    .any(|base| {
        parse_scope_selector(&base, selector_parser)
            .is_some_and(|selector| selector.slice().len() == 1)
    })
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
