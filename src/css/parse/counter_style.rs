use super::*;

pub(super) fn parse_counter_styles(css: &Css) -> Vec<CounterStyleRule> {
    let mut rules = Vec::new();
    let mut rest = css.source();
    while let Some(start) = find_ascii_case_insensitive(rest, "@counter-style") {
        let after_at_rule = &rest[start + "@counter-style".len()..];
        let Some(open_offset) = after_at_rule.find('{') else {
            break;
        };
        let name = after_at_rule[..open_offset].trim().to_ascii_lowercase();
        let open = start + "@counter-style".len() + open_offset;
        let Some(close) = find_matching_brace(rest, open) else {
            break;
        };
        let body = &rest[open + 1..close];
        if let Some(rule) = parse_counter_style_rule(&name, body) {
            rules.push(rule);
        }
        rest = &rest[close + 1..];
    }
    rules
}

pub(super) fn parse_counter_style_rule(name: &str, body: &str) -> Option<CounterStyleRule> {
    if name.is_empty() || name.eq_ignore_ascii_case("none") {
        return None;
    }
    let declarations = parse_declarations(body);
    let system = declarations
        .get("system")
        .and_then(|value| parse_counter_style_system(value))
        .unwrap_or(CounterStyleSystem::Symbolic);
    let symbols = declarations
        .get("symbols")
        .map(|value| parse_counter_symbols(value))
        .unwrap_or_default();
    let additive_symbols = declarations
        .get("additive-symbols")
        .map(|value| parse_additive_symbols(value))
        .unwrap_or_default();
    let prefix = declarations
        .get("prefix")
        .and_then(|value| parse_counter_string(value));
    let suffix = declarations
        .get("suffix")
        .and_then(|value| parse_counter_string(value));
    let negative = declarations
        .get("negative")
        .and_then(|value| parse_counter_negative(value));
    let pad = declarations
        .get("pad")
        .and_then(|value| parse_counter_pad(value));
    let range = declarations
        .get("range")
        .and_then(|value| parse_counter_range(value));
    let fallback = declarations
        .get("fallback")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let speak_as = declarations
        .get("speak-as")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());

    let valid_symbols = match system {
        CounterStyleSystem::Cyclic
        | CounterStyleSystem::Symbolic
        | CounterStyleSystem::Fixed(_) => !symbols.is_empty(),
        CounterStyleSystem::Numeric | CounterStyleSystem::Alphabetic => symbols.len() >= 2,
        CounterStyleSystem::Additive => !additive_symbols.is_empty(),
        CounterStyleSystem::Extends(ref extended) => {
            !extended.eq_ignore_ascii_case(name) && !extended.is_empty()
        }
    };
    valid_symbols.then_some(CounterStyleRule {
        name: name.to_string(),
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

pub(super) fn parse_counter_style_system(value: &str) -> Option<CounterStyleSystem> {
    let lower = value.trim().to_ascii_lowercase();
    if lower == "cyclic" {
        Some(CounterStyleSystem::Cyclic)
    } else if lower == "numeric" {
        Some(CounterStyleSystem::Numeric)
    } else if lower == "alphabetic" {
        Some(CounterStyleSystem::Alphabetic)
    } else if lower == "symbolic" {
        Some(CounterStyleSystem::Symbolic)
    } else if lower == "additive" {
        Some(CounterStyleSystem::Additive)
    } else if let Some(rest) = lower.strip_prefix("extends") {
        let name = rest.split_whitespace().next()?.to_string();
        Some(CounterStyleSystem::Extends(name))
    } else if let Some(rest) = lower.strip_prefix("fixed") {
        let first = rest
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(1);
        Some(CounterStyleSystem::Fixed(first))
    } else {
        None
    }
}

pub(super) fn parse_counter_negative(value: &str) -> Option<(String, String)> {
    let mut symbols = parse_counter_symbols(value);
    let first = symbols.drain(..1).next()?;
    let second = symbols.into_iter().next().unwrap_or_default();
    Some((first, second))
}

pub(super) fn parse_counter_pad(value: &str) -> Option<(usize, String)> {
    let value = value.trim();
    let (width, symbol) = value.split_once(char::is_whitespace)?;
    let width = width.trim().parse::<usize>().ok()?;
    let symbol = parse_counter_symbols(symbol).into_iter().next()?;
    Some((width, symbol))
}

pub(super) fn parse_counter_symbols(value: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut rest = value.trim();
    while !rest.is_empty() {
        if let Some((string, tail)) = parse_counter_string_token(rest) {
            symbols.push(string);
            rest = tail.trim_start();
        } else {
            let (ident, tail) = split_counter_token(rest);
            if ident.is_empty() {
                break;
            }
            symbols.push(unescape_counter_symbol(ident));
            rest = tail.trim_start();
        }
    }
    symbols
}

pub(super) fn parse_additive_symbols(value: &str) -> Vec<(i32, String)> {
    let symbols = value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            let (weight, symbol) = part.split_once(char::is_whitespace)?;
            let weight = weight.trim().parse::<i32>().ok()?;
            let symbol = parse_counter_symbols(symbol).into_iter().next()?;
            Some((weight, symbol))
        })
        .collect::<Vec<_>>();
    if valid_additive_symbols(&symbols) {
        symbols
    } else {
        Vec::new()
    }
}

fn valid_additive_symbols(symbols: &[(i32, String)]) -> bool {
    if symbols.is_empty() {
        return false;
    }
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
    true
}

pub(super) fn parse_counter_range(value: &str) -> Option<CounterStyleRange> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("auto") {
        return Some(CounterStyleRange::Auto);
    }

    let intervals = value
        .split(',')
        .map(|part| {
            let mut parts = part.split_whitespace();
            let start = parse_counter_range_bound(parts.next()?)?;
            let end = parse_counter_range_bound(parts.next()?)?;
            if parts.next().is_some() || start > end {
                return None;
            }
            Some(CounterStyleRangeInterval { start, end })
        })
        .collect::<Option<Vec<_>>>()?;
    (!intervals.is_empty()).then_some(CounterStyleRange::Intervals(intervals))
}

fn parse_counter_range_bound(value: &str) -> Option<i64> {
    if value.eq_ignore_ascii_case("infinite") {
        Some(i64::MAX)
    } else if value.eq_ignore_ascii_case("-infinite") {
        Some(i64::MIN)
    } else {
        value.parse::<i32>().ok().map(i64::from)
    }
}

pub(super) fn parse_counter_string(value: &str) -> Option<String> {
    parse_counter_symbols(value).into_iter().next()
}

pub(super) fn parse_counter_string_token(value: &str) -> Option<(String, &str)> {
    let quote = value.as_bytes().first().copied()?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let mut output = String::new();
    let mut escaped = false;
    for (index, character) in value[1..].char_indices() {
        if escaped {
            output.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character as u8 == quote {
            let tail = &value[index + 2..];
            return Some((output, tail));
        } else {
            output.push(character);
        }
    }
    None
}

pub(super) fn split_counter_token(value: &str) -> (&str, &str) {
    let end = value
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(value.len());
    (&value[..end], &value[end..])
}

pub(super) fn unescape_counter_symbol(value: &str) -> String {
    if let Some(hex) = value.strip_prefix('\\')
        && let Ok(codepoint) = u32::from_str_radix(hex, 16)
        && let Some(character) = char::from_u32(codepoint)
    {
        return character.to_string();
    }
    value.to_string()
}
