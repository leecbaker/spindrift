use cssparser::{Parser, ParserInput, Token};

use super::super::values::{
    canonical_predefined_counter_style_name, is_counter_name, parse_counter_style_integer,
    parse_counter_style_reference_name,
};
use super::*;
use crate::css::component_values::{split_css_top_level_delimiter, try_split_css_component_values};

/// Parse an `@counter-style` rule.
///
/// Counter style descriptors do not cascade with ordinary declaration
/// semantics: for each descriptor, the final *valid* declaration wins and an
/// invalid declaration is ignored.  Keep that rule local instead of using
/// `Declarations::get()`, which intentionally returns the final declaration
/// without knowing the descriptor grammar.
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-rule>
pub(super) fn parse_counter_style_rule(
    name: &str,
    body: &str,
    origin: StylesheetOrigin,
) -> Option<CounterStyleRule> {
    let name = parse_counter_style_definition_name(name, origin)?;
    let declarations = parse_declarations(body);

    let mut system = None;
    let mut symbols = None;
    let mut additive_symbols = None;
    let mut prefix = None;
    let mut suffix = None;
    let mut negative = None;
    let mut pad = None;
    let mut range = None;
    let mut fallback = None;
    let mut speak_as = None;

    for (descriptor, value) in declarations.iter() {
        match descriptor.as_str() {
            "system" => set_if_valid(&mut system, parse_counter_style_system(value)),
            "symbols" => set_if_valid(&mut symbols, parse_counter_symbols(value)),
            "additive-symbols" => {
                set_if_valid(&mut additive_symbols, parse_additive_symbols(value))
            }
            "prefix" => set_if_valid(&mut prefix, parse_single_counter_symbol(value)),
            "suffix" => set_if_valid(&mut suffix, parse_single_counter_symbol(value)),
            "negative" => set_if_valid(&mut negative, parse_counter_negative(value)),
            "pad" => set_if_valid(&mut pad, parse_counter_pad(value)),
            "range" => set_if_valid(&mut range, parse_counter_range(value)),
            "fallback" => set_if_valid(&mut fallback, parse_counter_style_reference(value)),
            "speak-as" => set_if_valid(&mut speak_as, parse_speak_as(value)),
            _ => {}
        }
    }

    let system = system.unwrap_or(CounterStyleSystem::Symbolic);
    let symbols = symbols.unwrap_or_default();
    let additive_symbols = additive_symbols.unwrap_or_default();
    let valid_symbols = match &system {
        CounterStyleSystem::Cyclic
        | CounterStyleSystem::Symbolic
        | CounterStyleSystem::Fixed(_) => !symbols.is_empty(),
        CounterStyleSystem::Numeric | CounterStyleSystem::Alphabetic => symbols.len() >= 2,
        CounterStyleSystem::Additive => !additive_symbols.is_empty(),
        // `symbols` and `additive-symbols` make an extends rule undefined;
        // they are not merely ignored descriptors.
        CounterStyleSystem::Extends(_) => symbols.is_empty() && additive_symbols.is_empty(),
    };
    valid_symbols.then_some(CounterStyleRule {
        name,
        system,
        symbols,
        additive_symbols,
        prefix,
        suffix,
        negative,
        pad,
        range,
        fallback,
        speak_as,
    })
}

fn set_if_valid<T>(target: &mut Option<T>, value: Option<T>) {
    if let Some(value) = value {
        *target = Some(value);
    }
}

/// The names whose counter definitions are supplied by the UA and cannot be
/// redefined by an author `@counter-style` rule.
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-name>
fn is_non_overridable_counter_style_name(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "decimal" | "disc" | "square" | "circle" | "disclosure-open" | "disclosure-closed"
    )
}

fn parse_counter_style_definition_name(value: &str, origin: StylesheetOrigin) -> Option<String> {
    let name = parse_single_custom_ident(value)?;
    (is_counter_style_custom_ident(&name)
        && (origin == StylesheetOrigin::UserAgent || !is_non_overridable_counter_style_name(&name)))
    .then(|| {
        canonical_predefined_counter_style_name(&name)
            .map(str::to_string)
            .unwrap_or(name)
    })
}

/// References may name predefined styles, including non-overridable ones, but
/// retain the author spelling so custom styles remain case-sensitive.
fn parse_counter_style_reference(value: &str) -> Option<String> {
    parse_counter_style_reference_name(value)
}

/// CSS Values' `<custom-ident>` adds `default` to the CSS-wide exclusions.
/// <https://drafts.csswg.org/css-values-4/#custom-idents>
fn is_counter_style_custom_ident(value: &str) -> bool {
    is_counter_name(value) && !value.eq_ignore_ascii_case("default")
}

fn parse_single_custom_ident(value: &str) -> Option<String> {
    let mut input = ParserInput::new(value.trim());
    let mut parser = Parser::new(&mut input);
    let name = parser.expect_ident_cloned().ok()?.to_string();
    parser.is_exhausted().then_some(name)
}

/// Parse the `system` descriptor's exact grammar.
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-system>
pub(super) fn parse_counter_style_system(value: &str) -> Option<CounterStyleSystem> {
    let parts = try_split_css_component_values(value)?;
    let (system, arguments) = parts.split_first()?;
    let system = parse_single_custom_ident(system)?;
    match system.to_ascii_lowercase().as_str() {
        "cyclic" if arguments.is_empty() => Some(CounterStyleSystem::Cyclic),
        "numeric" if arguments.is_empty() => Some(CounterStyleSystem::Numeric),
        "alphabetic" if arguments.is_empty() => Some(CounterStyleSystem::Alphabetic),
        "symbolic" if arguments.is_empty() => Some(CounterStyleSystem::Symbolic),
        "additive" if arguments.is_empty() => Some(CounterStyleSystem::Additive),
        "fixed" if arguments.is_empty() => Some(CounterStyleSystem::Fixed(1)),
        "fixed" if arguments.len() == 1 => {
            parse_counter_style_integer(arguments[0]).map(CounterStyleSystem::Fixed)
        }
        "extends" if arguments.len() == 1 => {
            parse_counter_style_reference(arguments[0]).map(CounterStyleSystem::Extends)
        }
        _ => None,
    }
}

fn parse_counter_negative(value: &str) -> Option<(String, String)> {
    let symbols = parse_counter_symbol_list(value)?;
    match symbols.as_slice() {
        [prefix] => Some((prefix.clone(), String::new())),
        [prefix, suffix] => Some((prefix.clone(), suffix.clone())),
        _ => None,
    }
}

fn parse_counter_pad(value: &str) -> Option<(usize, String)> {
    let parts = try_split_css_component_values(value)?;
    let [first, second] = parts.as_slice() else {
        return None;
    };
    // `<integer> && <symbol>` permits either component order.
    // <https://drafts.csswg.org/css-counter-styles-3/#pad>
    [(first, second), (second, first)]
        .into_iter()
        .find_map(|(width, symbol)| {
            let width = usize::try_from(parse_counter_style_integer(width)?).ok()?;
            let symbol = parse_single_counter_symbol(symbol)?;
            Some((width, symbol))
        })
}

fn parse_counter_symbols(value: &str) -> Option<Vec<String>> {
    let symbols = parse_counter_symbol_list(value)?;
    (!symbols.is_empty()).then_some(symbols)
}

fn parse_counter_symbol_list(value: &str) -> Option<Vec<String>> {
    try_split_css_component_values(value)?
        .into_iter()
        .map(parse_single_counter_symbol)
        .collect()
}

/// Parse the currently text-only part of `<symbol>`.
///
/// Images deliberately return `None` until marker and generated-content image
/// symbols have a typed paint representation.
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-symbols>
fn parse_single_counter_symbol(value: &str) -> Option<String> {
    let mut input = ParserInput::new(value.trim());
    let mut parser = Parser::new(&mut input);
    let token = parser.next().ok()?.clone();
    let symbol = match token {
        Token::QuotedString(value) => value.to_string(),
        Token::Ident(value) if is_counter_style_custom_ident(&value) => value.to_string(),
        _ => return None,
    };
    parser.is_exhausted().then_some(symbol)
}

fn parse_additive_symbols(value: &str) -> Option<Vec<(i32, String)>> {
    let entries = split_css_top_level_delimiter(value, ',');
    if entries.is_empty() || entries.iter().any(|entry| entry.is_empty()) {
        return None;
    }
    let symbols = entries
        .into_iter()
        .map(|entry| {
            let parts = try_split_css_component_values(entry)?;
            let [first, second] = parts.as_slice() else {
                return None;
            };
            // `<integer> && <symbol>` permits either component order.
            // <https://drafts.csswg.org/css-counter-styles-3/#additive-symbols>
            [(first, second), (second, first)]
                .into_iter()
                .find_map(|(weight, symbol)| {
                    Some((
                        parse_counter_style_integer(weight)?,
                        parse_single_counter_symbol(symbol)?,
                    ))
                })
        })
        .collect::<Option<Vec<_>>>()?;
    valid_additive_symbols(&symbols).then_some(symbols)
}

fn valid_additive_symbols(symbols: &[(i32, String)]) -> bool {
    let mut previous = i32::MAX;
    for (index, (weight, _)) in symbols.iter().enumerate() {
        if *weight < 0 || *weight >= previous {
            return false;
        }
        if *weight == 0 && index + 1 != symbols.len() {
            return false;
        }
        previous = *weight;
    }
    !symbols.is_empty()
}

pub(super) fn parse_counter_range(value: &str) -> Option<CounterStyleRange> {
    if value.trim().eq_ignore_ascii_case("auto") {
        return Some(CounterStyleRange::Auto);
    }
    let ranges = split_css_top_level_delimiter(value, ',');
    if ranges.is_empty() || ranges.iter().any(|range| range.is_empty()) {
        return None;
    }
    let intervals = ranges
        .into_iter()
        .map(|range| {
            let parts = try_split_css_component_values(range)?;
            let [start, end] = parts.as_slice() else {
                return None;
            };
            let start = parse_counter_range_bound(start, true)?;
            let end = parse_counter_range_bound(end, false)?;
            (start <= end).then_some(CounterStyleRangeInterval { start, end })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CounterStyleRange::Intervals(intervals))
}

fn parse_counter_range_bound(value: &str, is_start: bool) -> Option<i64> {
    if value.eq_ignore_ascii_case("infinite") {
        return Some(if is_start { i64::MIN } else { i64::MAX });
    }
    parse_counter_style_integer(value).map(i64::from)
}

fn parse_speak_as(value: &str) -> Option<String> {
    let value = parse_single_custom_ident(value)?;
    match value.to_ascii_lowercase().as_str() {
        "auto" | "bullets" | "numbers" | "words" | "spell-out" => Some(value.to_ascii_lowercase()),
        _ if is_counter_style_custom_ident(&value) => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_descriptors_do_not_replace_the_last_valid_value() {
        let rule = parse_counter_style_rule(
            "example",
            r##"
                system: extends decimal;
                prefix: "#";
                prefix: *;
                suffix: ",";
                suffix: '$' '$';
                negative: "(" ")";
                negative: "(" "x" ")";
                pad: 3 "0";
                pad: -1 "X";
                range: 1 2;
                range: 3 1;
                fallback: decimal-leading-zero;
                fallback: decimal cjk-decimal;
            "##,
            StylesheetOrigin::Author,
        )
        .expect("a valid extends rule");

        assert_eq!(rule.prefix.as_deref(), Some("#"));
        assert_eq!(rule.suffix.as_deref(), Some(","));
        assert_eq!(rule.negative, Some(("(".into(), ")".into())));
        assert_eq!(rule.pad, Some((3, "0".into())));
        assert_eq!(
            rule.range,
            Some(CounterStyleRange::Intervals(vec![
                CounterStyleRangeInterval { start: 1, end: 2 }
            ]))
        );
        assert_eq!(rule.fallback.as_deref(), Some("decimal-leading-zero"));
    }

    #[test]
    fn descriptors_accept_calculated_integers_after_dimension_safe_evaluation() {
        let fixed = parse_counter_style_system("fixed calc(1 + sign(100em - 1px))");
        assert_eq!(fixed, Some(CounterStyleSystem::Fixed(2)));
        assert_eq!(
            parse_counter_range("calc(2 - sign(100em - 1px)) calc(5 + sign(100em - 1px))"),
            Some(CounterStyleRange::Intervals(vec![
                CounterStyleRangeInterval { start: 1, end: 6 }
            ]))
        );
        assert_eq!(
            parse_counter_pad("calc(3 + sign(100em - 1px)) '*'"),
            Some((4, "*".into()))
        );
        assert_eq!(parse_counter_pad("'0' 3"), Some((3, "0".into())));
        assert_eq!(
            parse_additive_symbols("calc(2 + sign(100em - 1px)) c, 2 b, 1 a"),
            Some(vec![(3, "c".into()), (2, "b".into()), (1, "a".into())])
        );
        assert_eq!(
            parse_additive_symbols("c 3, 2 b, a 1"),
            Some(vec![(3, "c".into()), (2, "b".into()), (1, "a".into())])
        );
    }

    #[test]
    fn range_list_preserves_top_level_comma_entries() {
        assert_eq!(
            split_css_top_level_delimiter("1 10, 20 infinite", ','),
            ["1 10", "20 infinite"]
        );
        assert_eq!(
            parse_counter_range("1 10, 20 infinite"),
            Some(CounterStyleRange::Intervals(vec![
                CounterStyleRangeInterval { start: 1, end: 10 },
                CounterStyleRangeInterval {
                    start: 20,
                    end: i64::MAX,
                },
            ]))
        );
    }

    #[test]
    fn names_preserve_custom_case_but_normalize_predefined_names() {
        assert_eq!(
            parse_counter_style_definition_name("Custom-Style", StylesheetOrigin::Author),
            Some("Custom-Style".into())
        );
        assert_eq!(
            parse_counter_style_definition_name("HiRaGaNa", StylesheetOrigin::Author),
            Some("hiragana".into())
        );
        for name in ["none", "inherit", "initial", "default", "decimal", "DISC"] {
            assert_eq!(
                parse_counter_style_definition_name(name, StylesheetOrigin::Author),
                None,
                "{name}"
            );
        }
    }

    #[test]
    fn symbols_reject_css_wide_keywords_and_non_symbol_tokens() {
        assert_eq!(parse_counter_symbols("a inherit"), None);
        assert_eq!(parse_counter_symbols("a 0"), None);
        assert_eq!(parse_counter_symbols("a *"), None);
        assert_eq!(
            parse_counter_symbols("a b"),
            Some(vec!["a".into(), "b".into()])
        );
    }
}
