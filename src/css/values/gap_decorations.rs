use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ParsedGapRuleShorthand {
    pub(crate) widths: GapRuleList<ComputedLengthPercentage>,
    pub(crate) styles: GapRuleList<BorderStyle>,
    pub(crate) colors: GapRuleList<Color>,
}

/// Parses CSS Gaps Level 1 `*-rule-width` list values.
///
/// Gap rule widths share the CSS border `<line-width>` grammar, extended with
/// comma-separated lists and `repeat()` patterns:
/// <https://drafts.csswg.org/css-gaps-1/#column-row-rule-width>.
pub(crate) fn parse_gap_rule_width_list(
    value: &str,
    font_size: f32,
) -> Option<GapRuleList<ComputedLengthPercentage>> {
    parse_gap_rule_list(value, |token| parse_computed_border_width(token, font_size))
}

/// Parses CSS Gaps Level 1 `*-rule-style` list values.
///
/// Rule styles apply the same line styles as `border-style`:
/// <https://drafts.csswg.org/css-gaps-1/#column-row-rule-style>.
pub(crate) fn parse_gap_rule_style_list(value: &str) -> Option<GapRuleList<BorderStyle>> {
    parse_gap_rule_list(value, parse_border_style)
}

/// Parses CSS Gaps Level 1 `*-rule-color` list values.
///
/// `currentcolor` is resolved using the current cascaded `color`, matching the
/// rest of Quire's border color model:
/// <https://drafts.csswg.org/css-gaps-1/#column-row-rule-color>.
pub(crate) fn parse_gap_rule_color_list(
    value: &str,
    current_color: Color,
) -> Option<GapRuleList<Color>> {
    parse_gap_rule_list(value, |token| parse_border_color(token, current_color))
}

pub(crate) fn parse_gap_rule_break(value: &str) -> Option<GapRuleBreak> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "none" => Some(GapRuleBreak::None),
        "normal" => Some(GapRuleBreak::Normal),
        "intersection" => Some(GapRuleBreak::Intersection),
        _ => None,
    }
}

pub(crate) fn parse_gap_rule_visibility_items(value: &str) -> Option<GapRuleVisibilityItems> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "normal" => Some(GapRuleVisibilityItems::Normal),
        "all" => Some(GapRuleVisibilityItems::All),
        "around" => Some(GapRuleVisibilityItems::Around),
        "between" => Some(GapRuleVisibilityItems::Between),
        _ => None,
    }
}

pub(crate) fn parse_gap_rule_overlap(value: &str) -> Option<GapRuleOverlap> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "row-over-column" => Some(GapRuleOverlap::RowOverColumn),
        "column-over-row" => Some(GapRuleOverlap::ColumnOverRow),
        _ => None,
    }
}

pub(crate) fn parse_gap_rule_inset_value(value: &str, font_size: f32) -> Option<GapRuleInsetValue> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("overlap-join") {
        Some(GapRuleInsetValue::OverlapJoin)
    } else {
        parse_computed_length_percentage(value, font_size).map(GapRuleInsetValue::LengthPercentage)
    }
}

pub(crate) fn parse_gap_rule_shorthand(
    value: &str,
    font_size: f32,
    current_color: Color,
) -> Option<ParsedGapRuleShorthand> {
    let rule_components = parse_gap_rule_list(value, |token| {
        parse_single_gap_rule(token, font_size, current_color)
    })?;
    let widths = map_gap_rule_list(&rule_components, |rule| {
        rule.width
            .unwrap_or_else(|| ComputedLengthPercentage::from_points(3.0 * CSS_PX_TO_PT))
    });
    let styles = map_gap_rule_list(&rule_components, |rule| {
        rule.style.unwrap_or(BorderStyle::None)
    });
    let colors = map_gap_rule_list(&rule_components, |rule| rule.color.unwrap_or(current_color));
    Some(ParsedGapRuleShorthand {
        widths,
        styles,
        colors,
    })
}

#[derive(Debug, Clone)]
struct SingleGapRule {
    width: Option<ComputedLengthPercentage>,
    style: Option<BorderStyle>,
    color: Option<Color>,
}

fn parse_single_gap_rule(
    value: &str,
    font_size: f32,
    current_color: Color,
) -> Option<SingleGapRule> {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    let mut saw_component = false;
    for part in split_css_component_values(value) {
        if width.is_none()
            && let Some(parsed) = parse_computed_border_width(part, font_size)
        {
            width = Some(parsed);
            saw_component = true;
            continue;
        }
        if style.is_none()
            && let Some(parsed) = parse_border_style(part)
        {
            style = Some(parsed);
            saw_component = true;
            continue;
        }
        if color.is_none()
            && let Some(parsed) = parse_border_color(part, current_color)
        {
            color = Some(parsed);
            saw_component = true;
            continue;
        }
        return None;
    }
    saw_component.then_some(SingleGapRule {
        width,
        style,
        color,
    })
}

fn parse_gap_rule_list<T>(
    value: &str,
    parse_item: impl Fn(&str) -> Option<T> + Copy,
) -> Option<GapRuleList<T>> {
    let parts = split_top_level_commas(trim_css_value(value));
    if parts.is_empty() {
        return None;
    }

    let mut leading = Vec::new();
    let mut trailing = Vec::new();
    let mut auto = None;
    let mut after_auto = false;
    for part in parts {
        if let Some(repeat) = parse_gap_rule_repeat(part, parse_item) {
            match repeat {
                ParsedGapRepeat::Fixed { count, values } => {
                    let component = GapRuleListComponent::Repeat { count, values };
                    if after_auto {
                        trailing.push(component);
                    } else {
                        leading.push(component);
                    }
                }
                ParsedGapRepeat::Auto(values) => {
                    if auto.is_some() {
                        return None;
                    }
                    auto = Some(values);
                    after_auto = true;
                }
            }
            continue;
        }
        let value = parse_item(part)?;
        let component = GapRuleListComponent::Value(value);
        if after_auto {
            trailing.push(component);
        } else {
            leading.push(component);
        }
    }

    if auto.is_none() && leading.is_empty() {
        return None;
    }
    Some(GapRuleList::from_parts(leading, auto, trailing))
}

enum ParsedGapRepeat<T> {
    Fixed { count: usize, values: Vec<T> },
    Auto(Vec<T>),
}

fn parse_gap_rule_repeat<T>(
    value: &str,
    parse_item: impl Fn(&str) -> Option<T> + Copy,
) -> Option<ParsedGapRepeat<T>> {
    let value = trim_css_value(value);
    let inner = strip_gap_repeat_function(value)?;
    let (count, repeated) = split_top_level_once(inner, ',')?;
    let repeated_values = split_top_level_commas(repeated)
        .into_iter()
        .map(parse_item)
        .collect::<Option<Vec<_>>>()?;
    if repeated_values.is_empty() {
        return None;
    }
    let count = trim_css_value(count);
    if count.eq_ignore_ascii_case("auto") {
        return Some(ParsedGapRepeat::Auto(repeated_values));
    }
    let count = count.parse::<usize>().ok().filter(|count| *count > 0)?;
    Some(ParsedGapRepeat::Fixed {
        count,
        values: repeated_values,
    })
}

fn strip_gap_repeat_function(value: &str) -> Option<&str> {
    let value = trim_css_value(value);
    let open = "repeat".len();
    if !value
        .get(..open)
        .is_some_and(|name| name.eq_ignore_ascii_case("repeat"))
    {
        return None;
    }
    value
        .get(open..)?
        .strip_prefix('(')?
        .strip_suffix(')')
        .map(trim_css_value)
}

fn map_gap_rule_list<T, U>(list: &GapRuleList<T>, map: impl Fn(&T) -> U + Copy) -> GapRuleList<U> {
    GapRuleList::from_parts(
        list.leading
            .iter()
            .map(|component| map_gap_rule_component(component, map))
            .collect(),
        list.auto
            .as_ref()
            .map(|values| values.iter().map(map).collect()),
        list.trailing
            .iter()
            .map(|component| map_gap_rule_component(component, map))
            .collect(),
    )
}

fn map_gap_rule_component<T, U>(
    component: &GapRuleListComponent<T>,
    map: impl Fn(&T) -> U + Copy,
) -> GapRuleListComponent<U> {
    match component {
        GapRuleListComponent::Value(value) => GapRuleListComponent::Value(map(value)),
        GapRuleListComponent::Repeat { count, values } => GapRuleListComponent::Repeat {
            count: *count,
            values: values.iter().map(map).collect(),
        },
    }
}

fn split_top_level_commas(value: &str) -> Vec<&str> {
    split_top_level(value, ',')
        .into_iter()
        .map(trim_css_value)
        .filter(|part| !part.is_empty())
        .collect()
}

fn split_top_level_once(value: &str, delimiter: char) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if ch == delimiter && depth == 0 => {
                return Some((&value[..index], &value[index + ch.len_utf8()..]));
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0usize;
    for (index, ch) in value.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if ch == delimiter && depth == 0 => {
                parts.push(&value[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}
