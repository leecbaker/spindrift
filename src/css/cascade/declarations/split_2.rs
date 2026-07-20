use super::split_1::split_css_top_level_slashes;
use super::*;

/// Expand `corner-shape` into physical per-corner shape longhands.
///
/// CSS Borders and Box Decorations Level 4 uses the same four-corner expansion
/// order as `border-radius`:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape-shorthand>.
pub(in crate::css) fn expand_corner_shape_shorthand(
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let parts = split_css_component_values(value)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.iter().any(|part| parse_corner_shape(part).is_none()) {
        return None;
    }
    let expanded = expand_four_radius_components(&parts)?;
    Some(vec![
        ("corner-top-left-shape", expanded[0].clone()),
        ("corner-top-right-shape", expanded[1].clone()),
        ("corner-bottom-right-shape", expanded[2].clone()),
        ("corner-bottom-left-shape", expanded[3].clone()),
    ])
}

/// Expand `corner` into radius and shape longhands.
///
/// CSS Borders and Box Decorations Level 4 defines `corner` as a shorthand for
/// all `border-*-radius` and `corner-*-shape` longhands:
/// <https://drafts.csswg.org/css-borders-4/#corner-shorthand>.
pub(in crate::css) fn expand_corner_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let groups = split_css_top_level_slashes(value);
    if groups.is_empty() || groups.len() > 4 {
        return None;
    }
    let parsed = groups
        .iter()
        .map(|group| split_corner_radius_shape_component(group))
        .collect::<Option<Vec<_>>>()?;
    let expanded = match parsed.as_slice() {
        [all] => [all, all, all, all],
        [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
        [top_left, horizontal, bottom_right] => [top_left, horizontal, bottom_right, horizontal],
        [top_left, top_right, bottom_right, bottom_left] => {
            [top_left, top_right, bottom_right, bottom_left]
        }
        _ => return None,
    };
    let corner_names = [
        ("border-top-left-radius", "corner-top-left-shape"),
        ("border-top-right-radius", "corner-top-right-shape"),
        ("border-bottom-right-radius", "corner-bottom-right-shape"),
        ("border-bottom-left-radius", "corner-bottom-left-shape"),
    ];
    let declarations = corner_names
        .into_iter()
        .zip(expanded)
        .flat_map(|((radius_name, shape_name), (radius, shape))| {
            [(radius_name, radius.clone()), (shape_name, shape.clone())]
        })
        .collect::<Vec<_>>();
    Some(declarations)
}

pub(in crate::css) fn split_corner_radius_shape_component(value: &str) -> Option<(String, String)> {
    let mut shape = None;
    let mut radius_parts = Vec::new();
    for part in split_css_component_values(value) {
        if shape.is_none() && parse_corner_shape(part).is_some() {
            shape = Some(part.to_string());
        } else {
            radius_parts.push(part.to_string());
        }
    }
    (!radius_parts.is_empty()).then_some((radius_parts.join(" "), shape.unwrap_or("round".into())))
}

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

pub(in crate::css) fn parse_grid_track_list(value: &str, font_size: f32) -> Option<GridTrackList> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(GridTrackList::None);
    }
    let tokens = split_css_component_values(value);
    if tokens
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("subgrid"))
    {
        let line_names = parse_subgrid_line_name_list(&tokens[1..])?;
        return Some(GridTrackList::Subgrid { line_names });
    }
    let (components, trailing_names) = parse_grid_track_list_components(value, font_size)?;
    (!components.is_empty()).then_some(GridTrackList::Tracks {
        components,
        trailing_names,
    })
}

/// Parse the `<line-name-list>` permitted after `subgrid`.
///
/// Name repeats differ from standalone grid repeats: they contain only line
/// name slots and `auto-fill` resolves against the subgrid's used span.
/// <https://drafts.csswg.org/css-grid-2/#typedef-line-name-list>
fn parse_subgrid_line_name_list(tokens: &[&str]) -> Option<SubgridLineNameList> {
    let mut components = Vec::with_capacity(tokens.len());
    let mut has_auto_fill = false;
    for token in tokens {
        if let Some(names) = parse_subgrid_line_names(token) {
            components.push(SubgridLineNameComponent::LineNames(names));
            continue;
        }
        let repeat = parse_subgrid_name_repeat(token)?;
        if matches!(
            repeat,
            SubgridLineNameComponent::Repeat {
                count: SubgridLineNameRepeatCount::AutoFill,
                ..
            }
        ) {
            if has_auto_fill {
                return None;
            }
            has_auto_fill = true;
        }
        components.push(repeat);
    }
    Some(SubgridLineNameList { components })
}

fn parse_subgrid_name_repeat(token: &str) -> Option<SubgridLineNameComponent> {
    let inner = grid_function_body(trim_css_value(token), "repeat")?;
    let (count, slots) = split_top_level_once(inner, ',')?;
    let count = match trim_css_value(count).to_ascii_lowercase().as_str() {
        "auto-fill" => SubgridLineNameRepeatCount::AutoFill,
        value => SubgridLineNameRepeatCount::Number(
            value.parse::<u16>().ok().filter(|count| *count > 0)?,
        ),
    };
    let line_names = split_css_component_values(slots)
        .into_iter()
        .map(parse_subgrid_line_names)
        .collect::<Option<Vec<_>>>()?;
    (!line_names.is_empty()).then_some(SubgridLineNameComponent::Repeat { count, line_names })
}

/// Subgrid line-name lists preserve empty `[]` slots, which are significant:
/// they advance the local line assignment without contributing a name.
fn parse_subgrid_line_names(token: &str) -> Option<Vec<String>> {
    let token = trim_css_value(token);
    let inner = token.strip_prefix('[')?.strip_suffix(']')?;
    let mut input = cssparser::ParserInput::new(inner);
    let mut parser = cssparser::Parser::new(&mut input);
    let mut names = Vec::new();
    while !parser.is_exhausted() {
        let name = parser.expect_ident_cloned().ok()?.to_string();
        names.push(parse_grid_custom_ident_value(name)?);
    }
    Some(names)
}

pub(in crate::css) fn expand_grid_template_shorthand(
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(vec![
            ("grid-template-rows", "none".to_string()),
            ("grid-template-columns", "none".to_string()),
            ("grid-template-areas", "none".to_string()),
        ]);
    }

    let (rows, columns, has_slash) = split_top_level_once(value, '/')
        .map(|(rows, columns)| (trim_css_value(rows), trim_css_value(columns), true))
        .unwrap_or((value, "none", false));
    if rows.is_empty() || columns.is_empty() {
        return None;
    }
    let rows_have_areas = split_css_component_values(rows)
        .iter()
        .any(|token| parse_css_string_token(trim_css_value(token)).is_some());
    if !rows_have_areas {
        if !has_slash {
            return None;
        }
        parse_grid_track_list(rows, ROOT_FONT_SIZE_PT)?;
        parse_grid_track_list(columns, ROOT_FONT_SIZE_PT)?;
        return Some(vec![
            ("grid-template-rows", rows.to_string()),
            ("grid-template-columns", columns.to_string()),
            ("grid-template-areas", "none".to_string()),
        ]);
    }

    let (row_tracks, areas) = parse_grid_template_ascii_rows(rows)?;
    parse_grid_track_list(&row_tracks, ROOT_FONT_SIZE_PT)?;
    parse_grid_template_areas(&areas)?;
    parse_grid_track_list(columns, ROOT_FONT_SIZE_PT)?;
    Some(vec![
        ("grid-template-rows", row_tracks),
        ("grid-template-columns", columns.to_string()),
        ("grid-template-areas", areas),
    ])
}

pub(in crate::css) fn expand_grid_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let value = trim_css_value(value);
    if matches!(
        parse_grid_track_list(value, ROOT_FONT_SIZE_PT),
        Some(GridTrackList::Subgrid { .. })
    ) {
        return Some(vec![
            ("grid-template-rows", value.to_string()),
            ("grid-template-columns", value.to_string()),
            ("grid-template-areas", "none".to_string()),
            ("grid-auto-flow", "row".to_string()),
            ("grid-auto-rows", "auto".to_string()),
            ("grid-auto-columns", "auto".to_string()),
        ]);
    }
    let (left, right) = split_top_level_once(value, '/')
        .map(|(left, right)| (trim_css_value(left), trim_css_value(right)))
        .unwrap_or((value, ""));
    if left.is_empty() || right.is_empty() {
        let mut expanded = expand_grid_template_shorthand(value)?;
        expanded.extend(grid_implicit_initial_longhands());
        return Some(expanded);
    }
    if let Some((dense, auto_tracks)) = parse_grid_auto_flow_shorthand_side(left) {
        parse_grid_track_list(right, ROOT_FONT_SIZE_PT)?;
        return Some(vec![
            ("grid-template-rows", "none".to_string()),
            ("grid-template-columns", right.to_string()),
            ("grid-template-areas", "none".to_string()),
            (
                "grid-auto-flow",
                if dense { "row dense" } else { "row" }.to_string(),
            ),
            ("grid-auto-rows", auto_tracks),
            ("grid-auto-columns", "auto".to_string()),
        ]);
    }
    if let Some((dense, auto_tracks)) = parse_grid_auto_flow_shorthand_side(right) {
        parse_grid_track_list(left, ROOT_FONT_SIZE_PT)?;
        return Some(vec![
            ("grid-template-rows", left.to_string()),
            ("grid-template-columns", "none".to_string()),
            ("grid-template-areas", "none".to_string()),
            (
                "grid-auto-flow",
                if dense { "column dense" } else { "column" }.to_string(),
            ),
            ("grid-auto-rows", "auto".to_string()),
            ("grid-auto-columns", auto_tracks),
        ]);
    }
    let mut expanded = expand_grid_template_shorthand(value)?;
    expanded.extend(grid_implicit_initial_longhands());
    Some(expanded)
}

pub(in crate::css) fn grid_implicit_initial_longhands() -> Vec<(&'static str, String)> {
    vec![
        ("grid-auto-flow", "row".to_string()),
        ("grid-auto-rows", "auto".to_string()),
        ("grid-auto-columns", "auto".to_string()),
    ]
}

pub(in crate::css) fn parse_grid_auto_flow_shorthand_side(value: &str) -> Option<(bool, String)> {
    let tokens = split_css_component_values(value);
    let auto_flow_index = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("auto-flow"))?;
    let mut dense = false;
    for token in &tokens[..auto_flow_index] {
        if token.eq_ignore_ascii_case("dense") && !dense {
            dense = true;
        } else {
            return None;
        }
    }
    let mut track_start = auto_flow_index + 1;
    if tokens
        .get(track_start)
        .is_some_and(|token| token.eq_ignore_ascii_case("dense"))
    {
        dense = true;
        track_start += 1;
    }
    if tokens[track_start..]
        .iter()
        .any(|token| token.eq_ignore_ascii_case("dense") || token.eq_ignore_ascii_case("auto-flow"))
    {
        return None;
    }
    let auto_tracks = if track_start == tokens.len() {
        "auto".to_string()
    } else {
        let auto_tracks = tokens[track_start..].join(" ");
        parse_grid_auto_track_list(&auto_tracks, ROOT_FONT_SIZE_PT)?;
        auto_tracks
    };
    Some((dense, auto_tracks))
}

pub(in crate::css) fn parse_grid_template_ascii_rows(value: &str) -> Option<(String, String)> {
    let tokens = split_css_component_values(value);
    let mut index = 0usize;
    let mut row_track_tokens = Vec::new();
    let mut area_tokens = Vec::new();

    while index < tokens.len() {
        while index < tokens.len() && parse_grid_line_names(tokens[index]).is_some() {
            row_track_tokens.push(tokens[index].to_string());
            index += 1;
        }

        let (area, tail) = tokens
            .get(index)
            .and_then(|token| parse_css_string_token(trim_css_value(token)))?;
        if !tail.trim().is_empty() {
            return None;
        }
        area_tokens.push(css_quote_string(&area));
        index += 1;

        if index < tokens.len()
            && parse_css_string_token(trim_css_value(tokens[index])).is_none()
            && parse_grid_line_names(tokens[index]).is_none()
        {
            parse_grid_track_size(tokens[index], ROOT_FONT_SIZE_PT)?;
            row_track_tokens.push(tokens[index].to_string());
            index += 1;
        } else {
            row_track_tokens.push("auto".to_string());
        }

        while index < tokens.len() && parse_grid_line_names(tokens[index]).is_some() {
            row_track_tokens.push(tokens[index].to_string());
            index += 1;
        }
    }

    (!area_tokens.is_empty()).then(|| (row_track_tokens.join(" "), area_tokens.join(" ")))
}

pub(in crate::css) fn css_quote_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

pub(in crate::css) fn parse_grid_track_list_components(
    value: &str,
    font_size: f32,
) -> Option<(Vec<GridTrackListComponent>, GridLineNames)> {
    parse_grid_track_list_components_with_options(value, font_size, true)
}

pub(in crate::css) fn parse_grid_track_list_components_with_options(
    value: &str,
    font_size: f32,
    allow_repeat: bool,
) -> Option<(Vec<GridTrackListComponent>, GridLineNames)> {
    let mut components = Vec::new();
    let mut pending_names = Vec::new();
    let mut saw_auto_repeat = false;
    for token in split_css_component_values(value) {
        if let Some(names) = parse_grid_line_names(token) {
            pending_names.extend(names);
            continue;
        }
        if allow_repeat && let Some(repeat) = parse_grid_repeat(token, font_size) {
            if matches!(
                repeat.count,
                GridRepeatCount::AutoFill | GridRepeatCount::AutoFit
            ) {
                if saw_auto_repeat {
                    return None;
                }
                saw_auto_repeat = true;
            }
            components.push(GridTrackListComponent::Repeat(
                std::mem::take(&mut pending_names),
                repeat,
            ));
            continue;
        }
        if let Some(size) = parse_grid_track_size(token, font_size) {
            components.push(GridTrackListComponent::Track(
                std::mem::take(&mut pending_names),
                size,
            ));
            continue;
        }
        return None;
    }
    if !pending_names.is_empty() && components.is_empty() {
        return None;
    }
    Some((components, pending_names))
}

pub(in crate::css) fn parse_grid_line_names(token: &str) -> Option<Vec<String>> {
    let token = trim_css_value(token);
    let inner = token.strip_prefix('[')?.strip_suffix(']')?;
    let mut input = cssparser::ParserInput::new(inner);
    let mut parser = cssparser::Parser::new(&mut input);
    let mut names = Vec::new();
    while !parser.is_exhausted() {
        let name = parser.expect_ident_cloned().ok()?.to_string();
        names.push(parse_grid_custom_ident_value(name)?);
    }
    (!names.is_empty()).then_some(names)
}

pub(in crate::css) fn parse_grid_repeat(token: &str, font_size: f32) -> Option<GridRepeat> {
    let inner = grid_function_body(trim_css_value(token), "repeat")?;
    let (count, tracks) = split_top_level_once(inner, ',')?;
    let count = match trim_css_value(count).to_ascii_lowercase().as_str() {
        "auto-fill" => GridRepeatCount::AutoFill,
        "auto-fit" => GridRepeatCount::AutoFit,
        value => GridRepeatCount::Number(value.parse::<u16>().ok().filter(|count| *count > 0)?),
    };
    let (tracks, trailing_names) =
        parse_grid_track_list_components_with_options(tracks, font_size, false)?;
    if tracks.is_empty() || !grid_repeat_tracks_are_valid(count, &tracks) {
        return None;
    }
    Some(GridRepeat {
        count,
        tracks,
        trailing_names,
    })
}

/// Validate the CSS Grid `repeat()` grammar after parsing its track fragment.
///
/// CSS Grid forbids nested `repeat()` fragments, and `auto-fill`/`auto-fit`
/// use the stricter `<fixed-size>` grammar because auto-repeat counts are
/// derived from definite track breadths:
/// <https://www.w3.org/TR/css-grid-1/#repeat-notation>.
pub(in crate::css) fn grid_repeat_tracks_are_valid(
    count: GridRepeatCount,
    tracks: &[GridTrackListComponent],
) -> bool {
    tracks.iter().all(|component| match component {
        GridTrackListComponent::Track(_, size) => {
            !matches!(count, GridRepeatCount::AutoFill | GridRepeatCount::AutoFit)
                || grid_track_size_is_fixed_for_auto_repeat(size.clone())
                // Grid Lanes extends auto-repeat to intrinsic track breadths:
                // <https://drafts.csswg.org/css-grid-3/#track-sizing>.
                || matches!(
                    size,
                    GridTrackSize {
                        min: GridMinTrackBreadth::Auto,
                        max: GridMaxTrackBreadth::Auto,
                    }
                )
        }
        GridTrackListComponent::Repeat(_, _) => false,
    })
}

pub(in crate::css) fn grid_track_size_is_fixed_for_auto_repeat(size: GridTrackSize) -> bool {
    grid_min_track_breadth_is_fixed(size.min.clone())
        || (grid_min_track_breadth_is_inflexible(size.min)
            && grid_max_track_breadth_is_fixed(size.max))
}

pub(in crate::css) fn grid_min_track_breadth_is_fixed(value: GridMinTrackBreadth) -> bool {
    matches!(value, GridMinTrackBreadth::LengthPercentage(_))
}

pub(in crate::css) fn grid_min_track_breadth_is_inflexible(value: GridMinTrackBreadth) -> bool {
    matches!(
        value,
        GridMinTrackBreadth::Auto
            | GridMinTrackBreadth::MinContent
            | GridMinTrackBreadth::MaxContent
            | GridMinTrackBreadth::LengthPercentage(_)
    )
}

pub(in crate::css) fn grid_max_track_breadth_is_fixed(value: GridMaxTrackBreadth) -> bool {
    matches!(value, GridMaxTrackBreadth::LengthPercentage(_))
}

pub(in crate::css) fn parse_grid_auto_track_list(
    value: &str,
    font_size: f32,
) -> Option<GridAutoTrackList> {
    let tracks = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_grid_track_size(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    (!tracks.is_empty()).then_some(GridAutoTrackList { tracks })
}

pub(in crate::css) fn parse_grid_track_size(value: &str, font_size: f32) -> Option<GridTrackSize> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    if let Some(inner) = grid_function_body(value, "minmax") {
        let (min, max) = split_top_level_once(inner, ',')?;
        return Some(GridTrackSize {
            min: parse_grid_min_track_breadth(min, font_size)?,
            max: parse_grid_max_track_breadth(max, font_size)?,
        });
    }
    if let Some(inner) = grid_function_body(value, "fit-content") {
        return Some(GridTrackSize {
            min: GridMinTrackBreadth::Auto,
            max: GridMaxTrackBreadth::FitContent(parse_nonnegative_grid_track_breadth(
                inner, font_size,
            )?),
        });
    }
    if let Some(flex) = parse_grid_flex(&lower) {
        return Some(GridTrackSize {
            min: GridMinTrackBreadth::Auto,
            max: GridMaxTrackBreadth::Flex(flex),
        });
    }
    match lower.as_str() {
        "auto" => Some(GridTrackSize::AUTO),
        "min-content" => Some(GridTrackSize {
            min: GridMinTrackBreadth::MinContent,
            max: GridMaxTrackBreadth::MinContent,
        }),
        "max-content" => Some(GridTrackSize {
            min: GridMinTrackBreadth::MaxContent,
            max: GridMaxTrackBreadth::MaxContent,
        }),
        _ => {
            let length = parse_nonnegative_grid_track_breadth(value, font_size)?;
            Some(GridTrackSize {
                min: GridMinTrackBreadth::LengthPercentage(length.clone()),
                max: GridMaxTrackBreadth::LengthPercentage(length),
            })
        }
    }
}

pub(in crate::css) fn parse_grid_min_track_breadth(
    value: &str,
    font_size: f32,
) -> Option<GridMinTrackBreadth> {
    let value = trim_css_value(value);
    match value.to_ascii_lowercase().as_str() {
        "auto" => Some(GridMinTrackBreadth::Auto),
        "min-content" => Some(GridMinTrackBreadth::MinContent),
        "max-content" => Some(GridMinTrackBreadth::MaxContent),
        _ => parse_nonnegative_grid_track_breadth(value, font_size)
            .map(GridMinTrackBreadth::LengthPercentage),
    }
}

pub(in crate::css) fn parse_grid_max_track_breadth(
    value: &str,
    font_size: f32,
) -> Option<GridMaxTrackBreadth> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    if let Some(flex) = parse_grid_flex(&lower) {
        return Some(GridMaxTrackBreadth::Flex(flex));
    }
    if let Some(inner) = grid_function_body(value, "fit-content") {
        return parse_nonnegative_grid_track_breadth(inner, font_size)
            .map(GridMaxTrackBreadth::FitContent);
    }
    match lower.as_str() {
        "auto" => Some(GridMaxTrackBreadth::Auto),
        "min-content" => Some(GridMaxTrackBreadth::MinContent),
        "max-content" => Some(GridMaxTrackBreadth::MaxContent),
        _ => parse_nonnegative_grid_track_breadth(value, font_size)
            .map(GridMaxTrackBreadth::LengthPercentage),
    }
}

/// Parse a non-negative CSS Grid track breadth length-percentage.
///
/// CSS Grid track breadths use a non-negative range for `<length-percentage>`
/// arguments, including bare fixed tracks, `minmax()` breadths, and
/// `fit-content()` arguments:
/// <https://www.w3.org/TR/css-grid-1/#typedef-track-breadth>.
fn parse_nonnegative_grid_track_breadth(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    let length = parse_computed_length_percentage(value, font_size)?;
    (!grid_track_breadth_is_definitely_negative(length.clone())).then_some(length)
}

fn grid_track_breadth_is_definitely_negative(value: ComputedLengthPercentage) -> bool {
    value.is_definitely_absolute() && value.length_points() < 0.0
}

pub(in crate::css) fn parse_grid_flex(lower: &str) -> Option<f32> {
    let value = lower.strip_suffix("fr")?;
    value.parse::<f32>().ok().filter(|value| *value >= 0.0)
}

pub(in crate::css) fn grid_function_body<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let value = trim_css_value(value);
    let prefix_len = name.len();
    let prefix = value.get(..prefix_len)?;
    if !prefix.eq_ignore_ascii_case(name) {
        return None;
    }
    value[prefix_len..]
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')
}

pub(in crate::css) fn parse_grid_template_areas(value: &str) -> Option<GridTemplateAreas> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(GridTemplateAreas::None);
    }
    let rows = split_css_component_values(value)
        .into_iter()
        .map(|token| {
            let (row, tail) = parse_css_string_token(trim_css_value(token))?;
            if !tail.trim().is_empty() {
                None
            } else {
                let cells = parse_grid_template_area_row(&row)?;
                (!cells.is_empty()).then_some(GridTemplateAreaRow { cells })
            }
        })
        .collect::<Option<Vec<_>>>()?;
    let width = rows.first()?.cells.len();
    (rows.iter().all(|row| row.cells.len() == width) && grid_template_areas_are_rectangular(&rows))
        .then_some(GridTemplateAreas::Areas(rows))
}
