use super::super::shorthands::split_css_top_level_slashes;
use super::*;
use std::num::{NonZeroI32, NonZeroU16};

/// Parses one `grid-template-areas` string token into named and null cells.
///
/// CSS Grid parses each string as a whitespace-separated row of named cell
/// tokens or null cell tokens. Any unrecognized sequence is invalid:
/// <https://www.w3.org/TR/css-grid-1/#typedef-grid-template-areas-string>.
pub(in crate::css) fn parse_grid_template_area_row(row: &str) -> Option<Vec<Option<String>>> {
    let mut cells = Vec::new();
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.peek().cloned() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        if ch == '.' {
            while matches!(chars.peek(), Some('.')) {
                chars.next();
            }
            cells.push(None);
            continue;
        }
        if grid_template_area_name_code_point(ch) {
            let mut name = String::new();
            while let Some(ch) = chars.peek().cloned() {
                if !grid_template_area_name_code_point(ch) {
                    break;
                }
                name.push(ch);
                chars.next();
            }
            cells.push(Some(name));
            continue;
        }
        return None;
    }
    Some(cells)
}

pub(in crate::css) fn grid_template_area_name_code_point(ch: char) -> bool {
    ch == '-' || ch == '_' || ch.is_ascii_alphanumeric() || !ch.is_ascii()
}

/// Validates the CSS Grid requirement that named area cells form rectangles.
///
/// If any named grid area spans multiple cells, those cells must define a
/// single filled-in rectangle and no disconnected fragments:
/// <https://www.w3.org/TR/css-grid-1/#grid-template-areas-property>.
pub(in crate::css) fn grid_template_areas_are_rectangular(rows: &[GridTemplateAreaRow]) -> bool {
    let mut areas: Vec<GridTemplateAreaParseBounds> = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            let Some(name) = cell else {
                continue;
            };
            if let Some(area) = areas.iter_mut().find(|area| area.name == *name) {
                area.row_start = area.row_start.min(row_index);
                area.row_end = area.row_end.max(row_index);
                area.column_start = area.column_start.min(column_index);
                area.column_end = area.column_end.max(column_index);
            } else {
                areas.push(GridTemplateAreaParseBounds {
                    name: name.clone(),
                    row_start: row_index,
                    row_end: row_index,
                    column_start: column_index,
                    column_end: column_index,
                });
            }
        }
    }
    areas.into_iter().all(|area| {
        (area.row_start..=area.row_end).all(|row_index| {
            (area.column_start..=area.column_end).all(|column_index| {
                rows.get(row_index)
                    .and_then(|row| row.cells.get(column_index))
                    .is_some_and(|cell| cell.as_ref() == Some(&area.name))
            })
        })
    })
}

#[derive(Debug, Clone)]
pub(in crate::css) struct GridTemplateAreaParseBounds {
    pub(in crate::css) name: String,
    pub(in crate::css) row_start: usize,
    pub(in crate::css) row_end: usize,
    pub(in crate::css) column_start: usize,
    pub(in crate::css) column_end: usize,
}

pub(in crate::css) fn parse_grid_auto_flow(value: &str) -> Option<GridAutoFlow> {
    let mut axis = None;
    let mut dense = false;
    for token in split_css_component_values(value) {
        match token.to_ascii_lowercase().as_str() {
            "row" if axis.replace("row").is_none() => {}
            "column" if axis.replace("column").is_none() => {}
            "dense" if !dense => dense = true,
            _ => return None,
        }
    }
    match (axis, dense) {
        (None, false) => None,
        (Some("row") | None, true) => Some(GridAutoFlow::RowDense),
        (Some("row"), false) => Some(GridAutoFlow::Row),
        (Some("column"), false) => Some(GridAutoFlow::Column),
        (Some("column"), true) => Some(GridAutoFlow::ColumnDense),
        _ => None,
    }
}

pub(in crate::css) fn parse_grid_lanes_direction(value: &str) -> Option<GridLanesDirection> {
    if value.trim().eq_ignore_ascii_case("normal") {
        return Some(GridLanesDirection::Normal);
    }
    let mut axis = None;
    let mut track_reverse = false;
    let mut fill_reverse = false;
    for token in split_css_component_values(value) {
        match token.to_ascii_lowercase().as_str() {
            "row" if axis.is_none() => axis = Some(GridLanesAxis::Row),
            "column" if axis.is_none() => axis = Some(GridLanesAxis::Column),
            "track-reverse" if !track_reverse => track_reverse = true,
            "fill-reverse" if !fill_reverse => fill_reverse = true,
            _ => return None,
        }
    }
    Some(GridLanesDirection::Axis {
        axis: axis?,
        track_reverse,
        fill_reverse,
    })
}

pub(in crate::css) fn parse_grid_lanes_flow_tolerance(
    value: &str,
    font_size: f32,
) -> Option<GridLanesFlowTolerance> {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(GridLanesFlowTolerance::Normal),
        "infinite" => Some(GridLanesFlowTolerance::Infinite),
        _ => parse_computed_length_percentage(value, font_size)
            .filter(|value| {
                value
                    .used_length_with_percentage_basis(crate::units::PercentageBasis::definite(
                        crate::units::layout_pt(0.0),
                    ))
                    .map(crate::units::layout_points)
                    .unwrap_or(0.0)
                    >= 0.0
            })
            .map(GridLanesFlowTolerance::LengthPercentage),
    }
}

pub(in crate::css) fn parse_grid_placement(value: &str) -> Option<GridPlacement> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("auto") {
        return Some(GridPlacement::Auto);
    }
    if parts.iter().any(|part| part.eq_ignore_ascii_case("span")) {
        parse_grid_span_placement(&parts).map(GridPlacement::Span)
    } else {
        parse_grid_line_placement(&parts).map(GridPlacement::Line)
    }
}

pub(in crate::css) fn expand_grid_placement_shorthand(
    value: &str,
    start_name: &'static str,
    end_name: &'static str,
) -> Option<Vec<(&'static str, String)>> {
    let (start, end) = split_top_level_once(value, '/')
        .map(|(start, end)| (trim_css_value(start), trim_css_value(end).to_string()))
        .unwrap_or_else(|| {
            let start = trim_css_value(value);
            let end = if grid_placement_is_custom_ident(start) {
                start.to_string()
            } else {
                "auto".to_string()
            };
            (start, end)
        });
    if start.is_empty()
        || end.is_empty()
        || parse_grid_placement(start).is_none()
        || parse_grid_placement(&end).is_none()
    {
        return None;
    }
    Some(vec![(start_name, start.to_string()), (end_name, end)])
}

pub(in crate::css) fn expand_grid_area_shorthand(
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let parts = split_css_top_level_slashes(value);
    if parts.is_empty() || parts.len() > 4 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    if parts
        .iter()
        .any(|part| parse_grid_placement(part).is_none())
    {
        return None;
    }
    let row_start = parts[0];
    let column_start = parts.get(1).cloned().unwrap_or_else(|| {
        if grid_placement_is_custom_ident(row_start) {
            row_start
        } else {
            "auto"
        }
    });
    let row_end = parts.get(2).cloned().unwrap_or_else(|| {
        if grid_placement_is_custom_ident(row_start) {
            row_start
        } else {
            "auto"
        }
    });
    let column_end = parts.get(3).cloned().unwrap_or_else(|| {
        if grid_placement_is_custom_ident(column_start) {
            column_start
        } else {
            "auto"
        }
    });
    Some(vec![
        ("grid-row-start", row_start.to_string()),
        ("grid-column-start", column_start.to_string()),
        ("grid-row-end", row_end.to_string()),
        ("grid-column-end", column_end.to_string()),
    ])
}

pub(in crate::css) fn grid_placement_is_custom_ident(value: &str) -> bool {
    matches!(
        parse_grid_placement(value),
        Some(GridPlacement::Line(GridLinePlacement::Named {
            occurrence: None,
            ..
        }))
    )
}

/// Parse a CSS Grid `<custom-ident>` and return its canonical ident value.
///
/// Grid placement accepts normal CSS identifiers, including escaped names such
/// as `\31st` for template-area names that are not plain identifiers:
/// <https://www.w3.org/TR/css-grid-1/#grid-template-areas-property> and
/// <https://www.w3.org/TR/css-grid-1/#typedef-grid-line>.
pub(in crate::css) fn parse_grid_custom_ident(value: &str) -> Option<String> {
    let mut input = cssparser::ParserInput::new(trim_css_value(value));
    let mut parser = cssparser::Parser::new(&mut input);
    let ident = parser.expect_ident_cloned().ok()?.to_string();
    if !parser.is_exhausted() {
        return None;
    }
    parse_grid_custom_ident_value(ident)
}

pub(in crate::css) fn parse_grid_custom_ident_value(ident: String) -> Option<String> {
    (!matches!(
        ident.to_ascii_lowercase().as_str(),
        "auto" | "span" | "initial" | "inherit" | "unset" | "revert" | "revert-layer"
    ))
    .then_some(ident)
}

pub(in crate::css) fn parse_grid_line_placement(parts: &[&str]) -> Option<GridLinePlacement> {
    let mut name = None;
    let mut index = None;
    for part in parts {
        if part.eq_ignore_ascii_case("auto") || part.eq_ignore_ascii_case("span") {
            return None;
        }
        if let Ok(value) = part.parse::<i32>() {
            if value == 0 || index.replace(value).is_some() {
                return None;
            }
            continue;
        }
        let custom_ident = parse_grid_custom_ident(part)?;
        if name.replace(custom_ident).is_some() {
            return None;
        }
    }
    match (name, index) {
        (Some(name), index) => Some(GridLinePlacement::Named {
            name,
            occurrence: index.and_then(NonZeroI32::new),
        }),
        (None, Some(index)) => NonZeroI32::new(index).map(GridLinePlacement::Number),
        (None, None) => None,
    }
}

pub(in crate::css) fn parse_grid_span_placement(parts: &[&str]) -> Option<GridSpanPlacement> {
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut saw_span = false;
    let mut name = None;
    let mut span = None;
    for part in parts {
        if part.eq_ignore_ascii_case("span") {
            if saw_span {
                return None;
            }
            saw_span = true;
            continue;
        }
        if part.eq_ignore_ascii_case("auto") {
            return None;
        }
        if let Ok(value) = part.parse::<u16>() {
            if value == 0 || span.replace(value).is_some() {
                return None;
            }
            continue;
        }
        let custom_ident = parse_grid_custom_ident(part)?;
        if name.replace(custom_ident).is_some() {
            return None;
        }
    }
    if !saw_span {
        return None;
    }
    match (name, span) {
        (Some(name), span) => Some(GridSpanPlacement::Named {
            name,
            count: span.and_then(NonZeroU16::new),
        }),
        (None, Some(span)) => NonZeroU16::new(span).map(GridSpanPlacement::Count),
        (None, None) => None,
    }
}
