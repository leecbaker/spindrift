use super::*;
pub(in crate::css) fn expand_gap_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let parts = split_css_component_values(value);
    match parts.as_slice() {
        [row] if parse_gap(row, ROOT_FONT_SIZE_PT).is_some() => Some(vec![
            ("row-gap", (*row).to_string()),
            ("column-gap", (*row).to_string()),
        ]),
        [row, column]
            if parse_gap(row, ROOT_FONT_SIZE_PT).is_some()
                && parse_gap(column, ROOT_FONT_SIZE_PT).is_some() =>
        {
            Some(vec![
                ("row-gap", (*row).to_string()),
                ("column-gap", (*column).to_string()),
            ])
        }
        _ => None,
    }
}

pub(in crate::css) fn expand_gap_rule_shorthand(
    value: &str,
    axis_prefix: &'static str,
) -> Option<Vec<(&'static str, String)>> {
    if !matches!(axis_prefix, "column-rule" | "row-rule") {
        return None;
    }
    parse_gap_rule_shorthand(value, ROOT_FONT_SIZE_PT, CssColor::BLACK)?;
    Some(vec![(axis_prefix, trim_css_value(value).to_string())])
}

pub(in crate::css) fn expand_rule_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    parse_gap_rule_shorthand(value, ROOT_FONT_SIZE_PT, CssColor::BLACK)?;
    Some(vec![("rule", trim_css_value(value).to_string())])
}

pub(in crate::css) fn expand_rule_axis_shorthand(
    value: &str,
    component: &'static str,
) -> Option<Vec<(&'static str, String)>> {
    let value = trim_css_value(value).to_string();
    Some(vec![
        (gap_rule_axis_longhand("column", component)?, value.clone()),
        (gap_rule_axis_longhand("row", component)?, value),
    ])
}

pub(in crate::css) fn gap_rule_axis_longhand(axis: &str, component: &str) -> Option<&'static str> {
    match (axis, component) {
        ("column", "width") => Some("column-rule-width"),
        ("column", "style") => Some("column-rule-style"),
        ("column", "color") => Some("column-rule-color"),
        ("column", "break") => Some("column-rule-break"),
        ("column", "visibility-items") => Some("column-rule-visibility-items"),
        ("column", "inset") => Some("column-rule-inset"),
        ("column", "inset-start") => Some("column-rule-inset-start"),
        ("column", "inset-end") => Some("column-rule-inset-end"),
        ("column", "inset-cap") => Some("column-rule-inset-cap"),
        ("column", "inset-junction") => Some("column-rule-inset-junction"),
        ("row", "width") => Some("row-rule-width"),
        ("row", "style") => Some("row-rule-style"),
        ("row", "color") => Some("row-rule-color"),
        ("row", "break") => Some("row-rule-break"),
        ("row", "visibility-items") => Some("row-rule-visibility-items"),
        ("row", "inset") => Some("row-rule-inset"),
        ("row", "inset-start") => Some("row-rule-inset-start"),
        ("row", "inset-end") => Some("row-rule-inset-end"),
        ("row", "inset-cap") => Some("row-rule-inset-cap"),
        ("row", "inset-junction") => Some("row-rule-inset-junction"),
        _ => None,
    }
}

pub(in crate::css) fn expand_gap_rule_inset_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let prefix = name.strip_suffix("-inset")?;
    let value = trim_css_value(value);
    let (cap, junction) = if let Some((cap, junction)) = split_top_level_once(value, '/') {
        (
            gap_rule_inset_pair_values(cap)?,
            gap_rule_inset_pair_values(junction)?,
        )
    } else {
        let cap = gap_rule_inset_pair_values(value)?;
        (cap.clone(), cap)
    };
    Some(vec![
        (
            gap_rule_inset_longhand(prefix, "cap-start")?,
            cap[0].clone(),
        ),
        (gap_rule_inset_longhand(prefix, "cap-end")?, cap[1].clone()),
        (
            gap_rule_inset_longhand(prefix, "junction-start")?,
            junction[0].clone(),
        ),
        (
            gap_rule_inset_longhand(prefix, "junction-end")?,
            junction[1].clone(),
        ),
    ])
}

pub(in crate::css) fn gap_rule_inset_pair_values(value: &str) -> Option<[String; 2]> {
    let parts = split_css_component_values(trim_css_value(value));
    match parts.as_slice() {
        [all] => Some([all.to_string(), all.to_string()]),
        [start, end] => Some([start.to_string(), end.to_string()]),
        _ => None,
    }
}

pub(in crate::css) fn expand_gap_rule_inset_side_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let prefix = name
        .strip_suffix("-inset-start")
        .or_else(|| name.strip_suffix("-inset-end"))?;
    let side = if name.ends_with("-start") {
        "start"
    } else {
        "end"
    };
    let parts = split_css_component_values(trim_css_value(value));
    let [cap, junction] = match parts.as_slice() {
        [all] => [*all, *all],
        [cap, junction] => [*cap, *junction],
        _ => return None,
    };
    Some(vec![
        (
            gap_rule_inset_longhand(prefix, &format!("cap-{side}"))?,
            cap.to_string(),
        ),
        (
            gap_rule_inset_longhand(prefix, &format!("junction-{side}"))?,
            junction.to_string(),
        ),
    ])
}

pub(in crate::css) fn expand_gap_rule_inset_kind_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let prefix = name
        .strip_suffix("-inset-cap")
        .or_else(|| name.strip_suffix("-inset-junction"))?;
    let kind = if name.ends_with("-cap") {
        "cap"
    } else {
        "junction"
    };
    let parts = split_css_component_values(trim_css_value(value));
    let [start, end] = match parts.as_slice() {
        [all] => [*all, *all],
        [start, end] => [*start, *end],
        _ => return None,
    };
    Some(vec![
        (
            gap_rule_inset_longhand(prefix, &format!("{kind}-start"))?,
            start.to_string(),
        ),
        (
            gap_rule_inset_longhand(prefix, &format!("{kind}-end"))?,
            end.to_string(),
        ),
    ])
}

pub(in crate::css) fn gap_rule_inset_longhand(prefix: &str, suffix: &str) -> Option<&'static str> {
    match (prefix, suffix) {
        ("column-rule", "cap-start") => Some("column-rule-inset-cap-start"),
        ("column-rule", "cap-end") => Some("column-rule-inset-cap-end"),
        ("column-rule", "junction-start") => Some("column-rule-inset-junction-start"),
        ("column-rule", "junction-end") => Some("column-rule-inset-junction-end"),
        ("row-rule", "cap-start") => Some("row-rule-inset-cap-start"),
        ("row-rule", "cap-end") => Some("row-rule-inset-cap-end"),
        ("row-rule", "junction-start") => Some("row-rule-inset-junction-start"),
        ("row-rule", "junction-end") => Some("row-rule-inset-junction-end"),
        _ => None,
    }
}

/// Parses the `gap` shorthand into row and column gap computed values.
///
/// CSS Box Alignment defines `gap` as `<'row-gap'> <'column-gap'>?`; the
/// shorthand is invalid as a whole if either component is invalid:
/// <https://www.w3.org/TR/css-align-3/#gap-shorthand>.
pub(in crate::css) fn parse_gap_shorthand_components(
    parts: &[&str],
    font_size: f32,
) -> Option<(ComputedGap, ComputedGap)> {
    match parts {
        [row] => {
            let row = parse_gap(row, font_size)?;
            Some((row.clone(), row))
        }
        [row, column] => Some((parse_gap(row, font_size)?, parse_gap(column, font_size)?)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(in crate::css) enum GapRuleDeclarationAxis {
    Column,
    Row,
}

pub(in crate::css) fn gap_rule_axis_mut(
    style: &mut ComputedStyle,
    axis: GapRuleDeclarationAxis,
) -> &mut GapRuleAxis {
    match axis {
        GapRuleDeclarationAxis::Column => &mut style.column_rule,
        GapRuleDeclarationAxis::Row => &mut style.row_rule,
    }
}

pub(in crate::css) fn apply_gap_rule_shorthand(
    value: &str,
    style: &mut ComputedStyle,
    axis: GapRuleDeclarationAxis,
) {
    if let Some(rule) = parse_gap_rule_shorthand(value, style.font_size, style.color) {
        let target = gap_rule_axis_mut(style, axis);
        target.widths = rule.widths;
        target.styles = rule.styles;
        target.colors = rule.colors;
    }
}

pub(in crate::css) fn apply_gap_rule_width(
    value: &str,
    style: &mut ComputedStyle,
    axis: GapRuleDeclarationAxis,
) {
    if let Some(widths) = parse_gap_rule_width_list(value, style.font_size) {
        gap_rule_axis_mut(style, axis).widths = widths;
    }
}

pub(in crate::css) fn apply_gap_rule_style(
    value: &str,
    style: &mut ComputedStyle,
    axis: GapRuleDeclarationAxis,
) {
    if let Some(styles) = parse_gap_rule_style_list(value) {
        gap_rule_axis_mut(style, axis).styles = styles;
    }
}

pub(in crate::css) fn apply_gap_rule_color(
    value: &str,
    style: &mut ComputedStyle,
    axis: GapRuleDeclarationAxis,
) {
    if let Some(colors) = parse_gap_rule_color_list(value, style.color) {
        gap_rule_axis_mut(style, axis).colors = colors;
    }
}

pub(in crate::css) fn apply_gap_rule_break(
    value: &str,
    style: &mut ComputedStyle,
    axis: GapRuleDeclarationAxis,
) {
    if let Some(rule_break) = parse_gap_rule_break(value) {
        gap_rule_axis_mut(style, axis).rule_break = rule_break;
    }
}

pub(in crate::css) fn apply_gap_rule_visibility_items(
    value: &str,
    style: &mut ComputedStyle,
    axis: GapRuleDeclarationAxis,
) {
    if let Some(visibility) = parse_gap_rule_visibility_items(value) {
        gap_rule_axis_mut(style, axis).visibility_items = visibility;
    }
}

pub(in crate::css) fn apply_gap_rule_inset_property(
    name: &str,
    value: &str,
    style: &mut ComputedStyle,
) {
    if name == "rule-inset" {
        apply_gap_rule_inset_property("column-rule-inset", value, style);
        apply_gap_rule_inset_property("row-rule-inset", value, style);
        return;
    }
    if name == "rule-inset-start" {
        apply_gap_rule_inset_property("column-rule-inset-start", value, style);
        apply_gap_rule_inset_property("row-rule-inset-start", value, style);
        return;
    }
    if name == "rule-inset-end" {
        apply_gap_rule_inset_property("column-rule-inset-end", value, style);
        apply_gap_rule_inset_property("row-rule-inset-end", value, style);
        return;
    }
    if name == "rule-inset-cap" {
        apply_gap_rule_inset_property("column-rule-inset-cap", value, style);
        apply_gap_rule_inset_property("row-rule-inset-cap", value, style);
        return;
    }
    if name == "rule-inset-junction" {
        apply_gap_rule_inset_property("column-rule-inset-junction", value, style);
        apply_gap_rule_inset_property("row-rule-inset-junction", value, style);
        return;
    }

    if matches!(name, "column-rule-inset" | "row-rule-inset")
        && let Some(expanded) = expand_gap_rule_inset_shorthand(name, value)
    {
        for (longhand, value) in expanded {
            apply_gap_rule_inset_property(longhand, &value, style);
        }
        return;
    }
    if matches!(
        name,
        "column-rule-inset-start"
            | "column-rule-inset-end"
            | "row-rule-inset-start"
            | "row-rule-inset-end"
    ) && let Some(expanded) = expand_gap_rule_inset_side_shorthand(name, value)
    {
        for (longhand, value) in expanded {
            apply_gap_rule_inset_property(longhand, &value, style);
        }
        return;
    }
    if matches!(
        name,
        "column-rule-inset-cap"
            | "column-rule-inset-junction"
            | "row-rule-inset-cap"
            | "row-rule-inset-junction"
    ) && let Some(expanded) = expand_gap_rule_inset_kind_shorthand(name, value)
    {
        for (longhand, value) in expanded {
            apply_gap_rule_inset_property(longhand, &value, style);
        }
        return;
    }

    let Some(inset) = parse_gap_rule_inset_value(value, style.font_size) else {
        return;
    };
    let target = if name.starts_with("column-rule-") {
        &mut style.column_rule
    } else if name.starts_with("row-rule-") {
        &mut style.row_rule
    } else {
        return;
    };
    match name {
        "column-rule-inset-cap-start" | "row-rule-inset-cap-start" => {
            target.inset_cap_start = inset
        }
        "column-rule-inset-cap-end" | "row-rule-inset-cap-end" => target.inset_cap_end = inset,
        "column-rule-inset-junction-start" | "row-rule-inset-junction-start" => {
            target.inset_junction_start = inset
        }
        "column-rule-inset-junction-end" | "row-rule-inset-junction-end" => {
            target.inset_junction_end = inset
        }
        _ => {}
    }
}

pub(in crate::css) fn expand_columns_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let (inline_components, height) = split_top_level_once(trim_css_value(value), '/')
        .map(|(inline, height)| (trim_css_value(inline), trim_css_value(height)))
        .unwrap_or((trim_css_value(value), "auto"));
    if height.is_empty()
        || (!height.eq_ignore_ascii_case("auto")
            && !parse_computed_length_percentage(height, ROOT_FONT_SIZE_PT).is_some_and(|length| {
                !length.contains_percentage()
                    && length
                        .length_if_no_percent()
                        .is_some_and(|value| value >= 0.0)
            }))
    {
        return None;
    }
    let mut count = "auto".to_string();
    let mut width = "auto".to_string();
    let mut saw_component = false;
    for part in try_split_css_component_values(inline_components)? {
        if part.eq_ignore_ascii_case("auto") {
            saw_component = true;
        } else if part
            .parse::<u16>()
            .ok()
            .filter(|count| *count > 0)
            .is_some()
        {
            count = part.to_string();
            saw_component = true;
        } else if parse_computed_length_percentage(part, ROOT_FONT_SIZE_PT)
            .is_some_and(|length| !length.contains_percentage())
        {
            width = part.to_string();
            saw_component = true;
        } else {
            return None;
        }
    }
    saw_component.then(|| {
        vec![
            ("column-count", count),
            ("column-width", width),
            ("column-height", height.to_string()),
            ("column-wrap", "auto".to_string()),
        ]
    })
}
