use super::split_1::split_css_top_level_slashes;
use super::*;

/// Parses one `grid-template-areas` string token into named and null cells.
///
/// CSS Grid parses each string as a whitespace-separated row of named cell
/// tokens or null cell tokens. Any unrecognized sequence is invalid:
/// <https://www.w3.org/TR/css-grid-1/#typedef-grid-template-areas-string>.
pub(in crate::css) fn parse_grid_template_area_row(row: &str) -> Option<Vec<Option<String>>> {
    let mut cells = Vec::new();
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.peek().copied() {
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
            while let Some(ch) = chars.peek().copied() {
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
    let column_start = parts.get(1).copied().unwrap_or_else(|| {
        if grid_placement_is_custom_ident(row_start) {
            row_start
        } else {
            "auto"
        }
    });
    let row_end = parts.get(2).copied().unwrap_or_else(|| {
        if grid_placement_is_custom_ident(row_start) {
            row_start
        } else {
            "auto"
        }
    });
    let column_end = parts.get(3).copied().unwrap_or_else(|| {
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
        Some(GridPlacement::Line(GridLinePlacement {
            name: Some(_),
            index: None
        }))
    )
}

pub(in crate::css) fn grid_placement_name_is_custom_ident(value: &str) -> bool {
    is_css_identifier(value)
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "auto" | "span" | "initial" | "inherit" | "unset" | "revert" | "revert-layer"
        )
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
        if !grid_placement_name_is_custom_ident(part) {
            return None;
        }
        if name.replace((*part).to_string()).is_some() {
            return None;
        }
    }
    (name.is_some() || index.is_some()).then_some(GridLinePlacement { name, index })
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
        if !grid_placement_name_is_custom_ident(part) {
            return None;
        }
        if name.replace((*part).to_string()).is_some() {
            return None;
        }
    }
    (saw_span && (name.is_some() || span.is_some())).then_some(GridSpanPlacement { name, span })
}

pub(in crate::css) fn expand_flex_flow_shorthand(
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let mut direction = "row";
    let mut wrap = "nowrap";
    let mut saw_direction = false;
    let mut saw_wrap = false;
    for token in trim_css_value(value).split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "row" | "row-reverse" | "column" | "column-reverse" if !saw_direction => {
                direction = token;
                saw_direction = true;
            }
            "nowrap" | "wrap" | "wrap-reverse" if !saw_wrap => {
                wrap = token;
                saw_wrap = true;
            }
            _ => return None,
        }
    }
    (saw_direction || saw_wrap).then(|| {
        vec![
            ("flex-direction", direction.to_string()),
            ("flex-wrap", wrap.to_string()),
        ]
    })
}

pub(in crate::css) fn expand_flex_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let (grow, shrink, basis) = parse_flex_shorthand_components(value)?;
    Some(vec![
        ("flex-grow", grow),
        ("flex-shrink", shrink),
        ("flex-basis", basis),
    ])
}

/// Parses the CSS `flex` shorthand into its longhand component strings.
///
/// CSS Flexbox defines `flex` as `none | [ <flex-grow> <flex-shrink>? ||
/// <flex-basis> ]`, with omitted shrink defaulting to `1` and omitted basis
/// defaulting to `0%`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-property>.
pub(in crate::css) fn parse_flex_shorthand_components(
    value: &str,
) -> Option<(String, String, String)> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(("0".to_string(), "0".to_string(), "auto".to_string()));
    }
    if value.eq_ignore_ascii_case("auto") {
        return Some(("1".to_string(), "1".to_string(), "auto".to_string()));
    }

    let parts = split_css_component_values(value);
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }

    let mut grow = None;
    let mut shrink = None;
    let mut basis = None;
    for part in parts {
        if grow.is_some() && shrink.is_some() && basis.is_none() && is_unitless_zero(part) {
            basis = Some("0px".to_string());
        } else if let Some(number) = parse_nonnegative_flex_number(part) {
            if grow.is_none() {
                grow = Some(number);
            } else if shrink.is_none() {
                shrink = Some(number);
            } else {
                return None;
            }
        } else if basis.is_none() && parse_computed_flex_basis(part, ROOT_FONT_SIZE_PT).is_some() {
            basis = Some(part.to_string());
        } else {
            return None;
        }
    }

    let grow = grow.unwrap_or_else(|| "1".to_string());
    let shrink = shrink.unwrap_or_else(|| "1".to_string());
    let basis = basis.unwrap_or_else(|| "0%".to_string());
    Some((grow, shrink, basis))
}

/// Returns whether a token is the unitless zero allowed for `flex-basis` in `flex`.
///
/// CSS Flexbox keeps the `flex` shorthand compatible with common authoring by
/// accepting a unitless zero in the flex-basis slot, while nonzero unitless
/// values remain flex factors rather than lengths:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-property>.
pub(in crate::css) fn is_unitless_zero(value: &str) -> bool {
    trim_css_value(value)
        .parse::<f32>()
        .is_ok_and(|number| number == 0.0)
}

pub(in crate::css) fn parse_nonnegative_flex_number(value: &str) -> Option<String> {
    let value = trim_css_value(value);
    let number = value.parse::<f32>().ok()?;
    (number >= 0.0).then(|| value.to_string())
}

/// Expands CSS Box Alignment `place-*` shorthands into modeled longhands.
///
/// CSS Box Alignment defines `place-content`, `place-items`, and `place-self`
/// as paired block/inline-axis alignment shorthands:
/// <https://www.w3.org/TR/css-align-3/#place-content-property>,
/// <https://www.w3.org/TR/css-align-3/#place-items-property>, and
/// <https://www.w3.org/TR/css-align-3/#place-self-property>.
pub(in crate::css) fn expand_alignment_place_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    match name {
        "place-content" => {
            let (align, justify) = split_place_content_shorthand(value)?;
            Some(vec![("align-content", align), ("justify-content", justify)])
        }
        "place-items" => {
            let (align, justify) = split_place_shorthand(
                value,
                parse_align_items_keyword,
                parse_justify_items_keyword,
            )?;
            Some(vec![("align-items", align), ("justify-items", justify)])
        }
        "place-self" => {
            let (align, justify) =
                split_place_shorthand(value, parse_align_self_keyword, parse_justify_self_keyword)?;
            Some(vec![("align-self", align), ("justify-self", justify)])
        }
        _ => None,
    }
}

pub(in crate::css) fn split_place_content_shorthand(value: &str) -> Option<(String, String)> {
    let value = trim_css_value(value);
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if let Some(align) = parse_content_alignment_keyword(value, false, true) {
        if parse_justify_content_keyword(value).is_some() {
            return Some((value.to_string(), value.to_string()));
        }
        if matches!(
            align.keyword,
            ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline
        ) {
            return Some((value.to_string(), "start".to_string()));
        }
    }
    for split in 1..tokens.len() {
        let align = tokens[..split].join(" ");
        let justify = tokens[split..].join(" ");
        if parse_align_content_keyword(&align).is_some()
            && parse_justify_content_keyword(&justify).is_some()
        {
            return Some((align, justify));
        }
    }
    None
}

pub(in crate::css) fn split_place_shorthand<A, J>(
    value: &str,
    parse_align: A,
    parse_justify: J,
) -> Option<(String, String)>
where
    A: Fn(&str) -> Option<()>,
    J: Fn(&str) -> Option<()>,
{
    let value = trim_css_value(value);
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if parse_align(value).is_some() && parse_justify(value).is_some() {
        return Some((value.to_string(), value.to_string()));
    }
    for split in 1..tokens.len() {
        let align = tokens[..split].join(" ");
        let justify = tokens[split..].join(" ");
        if parse_align(&align).is_some() && parse_justify(&justify).is_some() {
            return Some((align, justify));
        }
    }
    None
}

pub(in crate::css) fn parse_alignment_safety_and_keyword(value: &str) -> (AlignmentSafety, String) {
    let mut parts = split_css_component_values(value);
    let safety = match parts.first().map(|part| part.to_ascii_lowercase()) {
        Some(keyword) if keyword == "safe" => {
            parts.remove(0);
            AlignmentSafety::Safe
        }
        Some(keyword) if keyword == "unsafe" => {
            parts.remove(0);
            AlignmentSafety::Unsafe
        }
        _ => AlignmentSafety::Default,
    };
    let keyword = parts
        .into_iter()
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    (safety, keyword)
}

pub(in crate::css) fn content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> ContentAlignment {
    match safety {
        AlignmentSafety::Default => ContentAlignment::new(keyword),
        AlignmentSafety::Unsafe => ContentAlignment::unsafe_position(keyword),
        AlignmentSafety::Safe => ContentAlignment::safe(keyword),
    }
}

pub(in crate::css) fn self_alignment(
    keyword: SelfAlignmentKeyword,
    safety: AlignmentSafety,
) -> SelfAlignment {
    match safety {
        AlignmentSafety::Default => SelfAlignment::new(keyword),
        AlignmentSafety::Unsafe => SelfAlignment::unsafe_position(keyword),
        AlignmentSafety::Safe => SelfAlignment::safe(keyword),
    }
}

pub(in crate::css) fn alignment_safety_allowed_for_content(
    keyword: ContentAlignmentKeyword,
) -> bool {
    matches!(
        keyword,
        ContentAlignmentKeyword::Normal
            | ContentAlignmentKeyword::Start
            | ContentAlignmentKeyword::End
            | ContentAlignmentKeyword::FlexStart
            | ContentAlignmentKeyword::FlexEnd
            | ContentAlignmentKeyword::Left
            | ContentAlignmentKeyword::Right
            | ContentAlignmentKeyword::Center
    )
}

pub(in crate::css) fn alignment_safety_allowed_for_self(keyword: SelfAlignmentKeyword) -> bool {
    matches!(
        keyword,
        SelfAlignmentKeyword::Normal
            | SelfAlignmentKeyword::Start
            | SelfAlignmentKeyword::End
            | SelfAlignmentKeyword::SelfStart
            | SelfAlignmentKeyword::SelfEnd
            | SelfAlignmentKeyword::FlexStart
            | SelfAlignmentKeyword::FlexEnd
            | SelfAlignmentKeyword::Left
            | SelfAlignmentKeyword::Right
            | SelfAlignmentKeyword::Center
    )
}

pub(in crate::css) fn parse_content_alignment_keyword(
    value: &str,
    allow_left_right: bool,
    allow_baseline: bool,
) -> Option<ContentAlignment> {
    let (safety, keyword) = parse_alignment_safety_and_keyword(value);
    let keyword = match keyword.as_str() {
        "normal" => ContentAlignmentKeyword::Normal,
        "center" => ContentAlignmentKeyword::Center,
        "space-between" => ContentAlignmentKeyword::SpaceBetween,
        "space-around" => ContentAlignmentKeyword::SpaceAround,
        "space-evenly" => ContentAlignmentKeyword::SpaceEvenly,
        "stretch" => ContentAlignmentKeyword::Stretch,
        "flex-start" => ContentAlignmentKeyword::FlexStart,
        "flex-end" => ContentAlignmentKeyword::FlexEnd,
        "start" => ContentAlignmentKeyword::Start,
        "end" => ContentAlignmentKeyword::End,
        "left" if allow_left_right => ContentAlignmentKeyword::Left,
        "right" if allow_left_right => ContentAlignmentKeyword::Right,
        "baseline" | "first baseline" if allow_baseline => ContentAlignmentKeyword::Baseline,
        "last baseline" if allow_baseline => ContentAlignmentKeyword::LastBaseline,
        _ => return None,
    };
    if safety != AlignmentSafety::Default && !alignment_safety_allowed_for_content(keyword) {
        return None;
    }
    Some(content_alignment(keyword, safety))
}

pub(in crate::css) fn parse_self_alignment_keyword(
    value: &str,
    allow_auto: bool,
    allow_left_right: bool,
) -> Option<SelfAlignment> {
    let (safety, keyword) = parse_alignment_safety_and_keyword(value);
    let keyword = match keyword.as_str() {
        "auto" if allow_auto => SelfAlignmentKeyword::Auto,
        "normal" => SelfAlignmentKeyword::Normal,
        "stretch" => SelfAlignmentKeyword::Stretch,
        "center" => SelfAlignmentKeyword::Center,
        "flex-start" => SelfAlignmentKeyword::FlexStart,
        "flex-end" => SelfAlignmentKeyword::FlexEnd,
        "start" => SelfAlignmentKeyword::Start,
        "end" => SelfAlignmentKeyword::End,
        "self-start" => SelfAlignmentKeyword::SelfStart,
        "self-end" => SelfAlignmentKeyword::SelfEnd,
        "left" if allow_left_right => SelfAlignmentKeyword::Left,
        "right" if allow_left_right => SelfAlignmentKeyword::Right,
        "baseline" | "first baseline" => SelfAlignmentKeyword::Baseline,
        "last baseline" => SelfAlignmentKeyword::LastBaseline,
        _ => return None,
    };
    if safety != AlignmentSafety::Default && !alignment_safety_allowed_for_self(keyword) {
        return None;
    }
    Some(self_alignment(keyword, safety))
}

pub(in crate::css) fn parse_justify_content_keyword(value: &str) -> Option<()> {
    parse_content_alignment_keyword(value, true, false).map(|_| ())
}

pub(in crate::css) fn parse_align_content_keyword(value: &str) -> Option<()> {
    parse_content_alignment_keyword(value, false, true).map(|_| ())
}

pub(in crate::css) fn parse_align_items_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, false, false).map(|_| ())
}

pub(in crate::css) fn parse_align_self_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, true, false).map(|_| ())
}

pub(in crate::css) fn parse_justify_items_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, false, true).map(|_| ())
}

pub(in crate::css) fn parse_justify_self_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, true, true).map(|_| ())
}

pub(in crate::css) fn parse_justify_content(
    value: &str,
    current: JustifyContent,
) -> JustifyContent {
    parse_content_alignment_keyword(value, true, false).unwrap_or(current)
}

pub(in crate::css) fn parse_align_content(value: &str, current: AlignContent) -> AlignContent {
    parse_content_alignment_keyword(value, false, true).unwrap_or(current)
}

pub(in crate::css) fn parse_align_items(value: &str, current: AlignItems) -> AlignItems {
    parse_self_alignment_keyword(value, false, false).unwrap_or(current)
}

pub(in crate::css) fn parse_align_self(value: &str, current: AlignSelf) -> AlignSelf {
    parse_self_alignment_keyword(value, true, false).unwrap_or(current)
}

pub(in crate::css) fn parse_justify_items(value: &str, current: JustifyItems) -> JustifyItems {
    parse_self_alignment_keyword(value, false, true).unwrap_or(current)
}

pub(in crate::css) fn parse_justify_self(value: &str, current: JustifySelf) -> JustifySelf {
    parse_self_alignment_keyword(value, true, true).unwrap_or(current)
}

pub(in crate::css) fn expand_columns_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let mut count = "auto".to_string();
    let mut width = "auto".to_string();
    let mut saw_component = false;
    for part in trim_css_value(value).split_whitespace() {
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
            .is_some_and(|length| length.percent == 0.0)
        {
            width = part.to_string();
            saw_component = true;
        } else {
            return None;
        }
    }
    saw_component.then(|| vec![("column-count", count), ("column-width", width)])
}

/// Returns whether two parsed declarations affect at least one same longhand.
///
/// CSS Cascade Level 5 applies CSS-wide keywords such as `revert-layer` to the
/// longhands represented by a shorthand, so cascade rollback has to compare
/// affected longhands instead of only exact serialized declaration names:
/// <https://www.w3.org/TR/css-cascade-5/#shorthand> and
/// <https://www.w3.org/TR/css-cascade-5/#revert-layer>.
pub(crate) fn declarations_affect_same_property(left: &str, right: &str) -> bool {
    declarations_affect_same_property_in_context(
        left,
        right,
        Direction::Ltr,
        WritingMode::HorizontalTb,
    )
}

pub(in crate::css) fn declarations_affect_same_property_in_context(
    left: &str,
    right: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> bool {
    if left.eq_ignore_ascii_case(right) {
        return true;
    }
    let Some(left_longhands) = affected_longhands(left, direction, writing_mode) else {
        return false;
    };
    let Some(right_longhands) = affected_longhands(right, direction, writing_mode) else {
        return false;
    };
    left_longhands
        .iter()
        .any(|left| right_longhands.iter().any(|right| left == right))
}
