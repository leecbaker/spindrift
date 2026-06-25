use super::*;

pub(super) fn expand_nested_rules(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut position = 0usize;
    while let Some(open) = find_next_top_level_open_brace(source, position) {
        let Some(close) = find_matching_brace(source, open) else {
            break;
        };
        let prelude_start = source[position..open]
            .rfind(['}', ';'])
            .map(|index| position + index + 1)
            .unwrap_or(position);
        output.push_str(&source[position..prelude_start]);
        let prelude = source[prelude_start..open].trim();
        let body = &source[open + 1..close];
        if prelude.starts_with('@') || !block_contains_nested_rule(body) {
            output.push_str(&source[prelude_start..=close]);
        } else {
            expand_rule_block(prelude, body, &mut output);
        }
        position = close + 1;
    }
    output.push_str(&source[position..]);
    output
}

pub(super) fn expand_rule_block(selector: &str, body: &str, output: &mut String) {
    let (declarations, nested_rules) = split_nested_rules(body);
    if !declarations.trim().is_empty() {
        output.push_str(selector);
        output.push_str(" { ");
        output.push_str(declarations.trim());
        output.push_str(" }\n");
    }

    for nested in nested_rules {
        for combined in combine_nested_selectors(selector, &nested.selector) {
            expand_rule_block(&combined, &nested.body, output);
        }
    }
}

#[derive(Debug)]
pub(super) struct NestedRule {
    selector: String,
    body: String,
}

pub(super) fn split_nested_rules(body: &str) -> (String, Vec<NestedRule>) {
    let mut declarations = String::with_capacity(body.len());
    let mut nested_rules = Vec::new();
    let mut segment_start = 0usize;
    let mut position = 0usize;

    while let Some(open) = find_next_top_level_open_brace(body, position) {
        let Some(close) = find_matching_brace(body, open) else {
            break;
        };
        let selector_start = body[segment_start..open]
            .rfind(';')
            .map(|index| segment_start + index + 1)
            .unwrap_or(segment_start);
        declarations.push_str(&body[segment_start..selector_start]);
        nested_rules.push(NestedRule {
            selector: body[selector_start..open].trim().to_string(),
            body: body[open + 1..close].to_string(),
        });
        segment_start = close + 1;
        position = close + 1;
    }

    declarations.push_str(&body[segment_start..]);
    (declarations, nested_rules)
}

pub(super) fn block_contains_nested_rule(body: &str) -> bool {
    find_next_top_level_open_brace(body, 0).is_some()
}

pub(super) fn combine_nested_selectors(parent: &str, nested: &str) -> Vec<String> {
    let parents = split_selector_list(parent);
    let nested_selectors = split_selector_list(nested);
    let mut selectors = Vec::new();
    for parent in parents {
        for nested in &nested_selectors {
            if nested.contains('&') {
                selectors.push(nested.replace('&', parent));
            } else if nested.starts_with(':') {
                selectors.push(format!("{parent}{nested}"));
            } else {
                selectors.push(format!("{parent} {nested}"));
            }
        }
    }
    selectors
}

pub(super) fn split_selector_list(selectors: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let bytes = selectors.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b',' if paren_depth == 0 && bracket_depth == 0 => {
                let part = selectors[start..index].trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    let part = selectors[start..].trim();
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

pub(super) fn find_next_top_level_open_brace(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = start;
    let mut string_quote = None;
    let mut paren_depth = 0usize;
    while index < bytes.len() {
        if let Some(quote) = string_quote {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index] == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            if let Some(end) = source[index + 2..].find("*/") {
                index += end + 4;
                continue;
            }
            return None;
        }
        match bytes[index] {
            b'\'' | b'"' => string_quote = Some(bytes[index]),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'{' if paren_depth == 0 => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

pub(super) fn flatten_rule(
    rule: ParsedCssRule,
    rules: &mut Vec<StyleRule>,
    marker_rules: &mut Vec<StyleRule>,
    before_rules: &mut Vec<StyleRule>,
    after_rules: &mut Vec<StyleRule>,
    first_line_rules: &mut Vec<StyleRule>,
    first_letter_rules: &mut Vec<StyleRule>,
) {
    match rule {
        ParsedCssRule::Style(rule) => rules.push(rule),
        ParsedCssRule::Marker(rule) => marker_rules.push(rule),
        ParsedCssRule::Before(rule) => before_rules.push(rule),
        ParsedCssRule::After(rule) => after_rules.push(rule),
        ParsedCssRule::FirstLine(rule) => first_line_rules.push(rule),
        ParsedCssRule::FirstLetter(rule) => first_letter_rules.push(rule),
        ParsedCssRule::Nested(nested) => {
            for rule in nested {
                flatten_rule(
                    rule,
                    rules,
                    marker_rules,
                    before_rules,
                    after_rules,
                    first_line_rules,
                    first_letter_rules,
                );
            }
        }
        ParsedCssRule::Ignored => {}
    }
}
