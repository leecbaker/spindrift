use super::*;

pub(super) fn taffy_box_sizing(value: BoxSizing) -> taffy_layout::BoxSizing {
    match value {
        BoxSizing::BorderBox => taffy_layout::BoxSizing::BorderBox,
        BoxSizing::ContentBox => taffy_layout::BoxSizing::ContentBox,
    }
}

pub(super) fn taffy_direction(value: Direction) -> taffy::style::Direction {
    match value {
        Direction::Ltr => taffy::style::Direction::Ltr,
        Direction::Rtl => taffy::style::Direction::Rtl,
    }
}

pub(super) fn taffy_dimension(
    value: css::ComputedLengthPercentageOrAuto,
) -> taffy_layout::Dimension {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            taffy_length_percentage(value).into()
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => limit
            .map(taffy_dimension_from_length_percentage)
            .unwrap_or_else(taffy_layout::Dimension::auto),
    }
}

pub(super) fn taffy_grid_item_min_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
    min_content: f32,
    max_content: f32,
) -> taffy_layout::Dimension {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
        _ => taffy_grid_item_dimension(value, percentage_basis, min_content, max_content),
    }
}

pub(super) fn taffy_grid_item_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
    min_content: f32,
    max_content: f32,
) -> taffy_layout::Dimension {
    let min_content = min_content.max(0.0);
    let max_content = max_content.max(min_content);
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            taffy_dimension_from_length_percentage_with_basis(value, percentage_basis)
        }
        css::ComputedLengthPercentageOrAuto::MinContent => {
            taffy_layout::Dimension::length(min_content)
        }
        css::ComputedLengthPercentageOrAuto::MaxContent => {
            taffy_layout::Dimension::length(max_content)
        }
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let limit = limit
                .and_then(|limit| {
                    percentage_basis.map(|basis| used_length_percentage(limit, basis))
                })
                .unwrap_or(max_content);
            taffy_layout::Dimension::length(max_content.min(min_content.max(limit)).max(0.0))
        }
    }
}

pub(super) fn taffy_dimension_from_length_percentage(
    value: css::ComputedLengthPercentage,
) -> taffy_layout::Dimension {
    if value.percent != 0.0 && value.length == 0.0 {
        taffy_layout::Dimension::percent(value.percent)
    } else {
        taffy_layout::Dimension::length(value.length)
    }
}

pub(super) fn taffy_dimension_from_length_percentage_with_basis(
    value: css::ComputedLengthPercentage,
    percentage_basis: Option<f32>,
) -> taffy_layout::Dimension {
    if value.math.is_some()
        && let Some(basis) = percentage_basis
    {
        return taffy_layout::Dimension::length(used_length_percentage(value, basis));
    }
    if value.percent != 0.0 && value.length == 0.0 {
        if let Some(basis) = percentage_basis {
            taffy_layout::Dimension::length(used_length_percentage(value, basis))
        } else {
            taffy_layout::Dimension::percent(value.percent)
        }
    } else {
        taffy_layout::Dimension::length(value.length)
    }
}

pub(super) fn taffy_margin(
    style: &ComputedStyle,
) -> taffy_layout::Rect<taffy_layout::LengthPercentageAuto> {
    let edges = style.box_values.margin;
    taffy_layout::Rect {
        left: taffy_length_percentage_auto(edges.left),
        right: taffy_length_percentage_auto(edges.right),
        top: taffy_length_percentage_auto(edges.top),
        bottom: taffy_length_percentage_auto(edges.bottom),
    }
}

pub(super) fn taffy_padding(
    style: &ComputedStyle,
) -> taffy_layout::Rect<taffy_layout::LengthPercentage> {
    let edges = style.box_values.padding;
    taffy_layout::Rect {
        left: taffy_length_percentage(edges.left),
        right: taffy_length_percentage(edges.right),
        top: taffy_length_percentage(edges.top),
        bottom: taffy_length_percentage(edges.bottom),
    }
}

pub(super) fn taffy_edges(edges: css::Edges) -> taffy_layout::Rect<taffy_layout::LengthPercentage> {
    taffy_layout::Rect {
        left: taffy_layout::LengthPercentage::length(edges.left),
        right: taffy_layout::LengthPercentage::length(edges.right),
        top: taffy_layout::LengthPercentage::length(edges.top),
        bottom: taffy_layout::LengthPercentage::length(edges.bottom),
    }
}

pub(super) fn taffy_length_percentage(
    value: css::ComputedLengthPercentage,
) -> taffy_layout::LengthPercentage {
    if value.percent != 0.0 && value.length == 0.0 {
        taffy_layout::LengthPercentage::percent(value.percent)
    } else {
        taffy_layout::LengthPercentage::length(value.length)
    }
}

pub(super) fn taffy_length_percentage_auto(
    value: css::ComputedLengthPercentageOrAuto,
) -> taffy_layout::LengthPercentageAuto {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::LengthPercentageAuto::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            taffy_length_percentage(value).into()
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => {
            taffy_layout::LengthPercentageAuto::auto()
        }
    }
}

pub(super) fn taffy_gap(value: css::ComputedGap) -> taffy_layout::LengthPercentage {
    match value {
        css::ComputedGap::Normal => taffy_layout::LengthPercentage::length(0.0),
        css::ComputedGap::LengthPercentage(value) => taffy_length_percentage(value),
    }
}

pub(super) fn taffy_grid_safety(safety: AlignmentSafety) -> taffy_layout::AlignmentSafety {
    match safety {
        AlignmentSafety::Default | AlignmentSafety::Unsafe => taffy_layout::AlignmentSafety::Unsafe,
        AlignmentSafety::Safe => taffy_layout::AlignmentSafety::Safe,
    }
}

/// Maps CSS Box Alignment content distribution into Taffy's grid container model.
///
/// CSS Grid consumes `align-content` and `justify-content` to distribute the
/// grid tracks inside the grid container. Taffy models the common distribution
/// and positional keywords; baseline content alignment currently falls back to
/// start-side packing at this adapter boundary:
/// <https://www.w3.org/TR/css-align-3/#content-distribution> and
/// <https://www.w3.org/TR/css-grid-1/#alignment>.
pub(super) fn taffy_grid_content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> taffy_layout::AlignContent {
    let safety = taffy_grid_safety(safety);
    match keyword {
        ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::Stretch,
                safety,
            }
        }
        ContentAlignmentKeyword::Start => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::Start,
            safety,
        },
        ContentAlignmentKeyword::End => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::End,
            safety,
        },
        ContentAlignmentKeyword::FlexStart | ContentAlignmentKeyword::Left => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::FlexStart,
                safety,
            }
        }
        ContentAlignmentKeyword::FlexEnd | ContentAlignmentKeyword::Right => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                safety,
            }
        }
        ContentAlignmentKeyword::Center => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::Center,
            safety,
        },
        ContentAlignmentKeyword::SpaceBetween => taffy_layout::AlignContent::SPACE_BETWEEN,
        ContentAlignmentKeyword::SpaceAround => taffy_layout::AlignContent::SPACE_AROUND,
        ContentAlignmentKeyword::SpaceEvenly => taffy_layout::AlignContent::SPACE_EVENLY,
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            taffy_layout::AlignContent::FLEX_START
        }
    }
}

pub(super) fn taffy_grid_align_content(align_content: AlignContent) -> taffy_layout::AlignContent {
    taffy_grid_content_alignment(align_content.keyword, align_content.safety)
}

pub(super) fn taffy_grid_justify_content(
    justify_content: JustifyContent,
) -> taffy_layout::JustifyContent {
    let alignment = taffy_grid_content_alignment(justify_content.keyword, justify_content.safety);
    taffy_layout::JustifyContent {
        keyword: alignment.keyword,
        safety: alignment.safety,
    }
}

/// Maps CSS self-alignment into Taffy's grid item alignment model.
///
/// CSS Grid applies `align-items`/`justify-items` as defaults for grid items
/// and lets `align-self`/`justify-self` override them. Baseline alignment and
/// writing-mode-sensitive self-start/self-end still need Quire-owned follow-up
/// handling, but the common positional and stretch keywords can be delegated
/// to Taffy:
/// <https://www.w3.org/TR/css-align-3/#self-alignment> and
/// <https://www.w3.org/TR/css-grid-1/#alignment>.
pub(super) fn taffy_grid_items_alignment(alignment: AlignItems) -> taffy_layout::AlignItems {
    let safety = taffy_grid_safety(alignment.safety);
    match alignment.keyword {
        SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Stretch => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Stretch,
            safety,
        },
        SelfAlignmentKeyword::Start | SelfAlignmentKeyword::SelfStart => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Start,
            safety,
        },
        SelfAlignmentKeyword::End | SelfAlignmentKeyword::SelfEnd => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::End,
            safety,
        },
        SelfAlignmentKeyword::FlexStart | SelfAlignmentKeyword::Left => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::FlexStart,
            safety,
        },
        SelfAlignmentKeyword::FlexEnd | SelfAlignmentKeyword::Right => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::FlexEnd,
            safety,
        },
        SelfAlignmentKeyword::Center => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Center,
            safety,
        },
        SelfAlignmentKeyword::Baseline | SelfAlignmentKeyword::LastBaseline => {
            taffy_layout::AlignItems::BASELINE
        }
    }
}

pub(super) fn taffy_grid_self_alignment(alignment: AlignSelf) -> taffy_layout::AlignSelf {
    let items = taffy_grid_items_alignment(alignment);
    taffy_layout::AlignSelf {
        keyword: items.keyword,
        safety: items.safety,
    }
}

pub(super) fn taffy_grid_align_items(align_items: AlignItems) -> taffy_layout::AlignItems {
    taffy_grid_items_alignment(align_items)
}

pub(super) fn taffy_grid_justify_items(justify_items: JustifyItems) -> taffy_layout::AlignItems {
    taffy_grid_items_alignment(justify_items)
}

pub(super) fn taffy_effective_grid_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> Option<taffy_layout::AlignSelf> {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        None
    } else {
        Some(taffy_grid_self_alignment(effective_grid_align_self(
            child_style,
            container_style,
        )))
    }
}

pub(super) fn taffy_effective_grid_justify_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> Option<taffy_layout::AlignSelf> {
    if child_style.justify_self.keyword == SelfAlignmentKeyword::Auto {
        None
    } else {
        Some(taffy_grid_self_alignment(effective_grid_justify_self(
            child_style,
            container_style,
        )))
    }
}

fn effective_grid_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> AlignSelf {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.align_items
    } else {
        child_style.align_self
    }
}

fn effective_grid_justify_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> JustifySelf {
    if child_style.justify_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.justify_items
    } else {
        child_style.justify_self
    }
}

pub(super) fn taffy_grid_template_tracks(
    value: &css::GridTrackList,
) -> Vec<taffy_layout::GridTemplateComponent<String>> {
    match value {
        css::GridTrackList::None => Vec::new(),
        css::GridTrackList::Tracks { components, .. } => components
            .iter()
            .filter_map(taffy_grid_template_component)
            .collect(),
    }
}

pub(super) fn taffy_grid_template_component(
    component: &css::GridTrackListComponent,
) -> Option<taffy_layout::GridTemplateComponent<String>> {
    match component {
        css::GridTrackListComponent::Track(_, size) => Some(
            taffy_layout::GridTemplateComponent::Single(taffy_track_size(size)),
        ),
        css::GridTrackListComponent::Repeat(_, repeat) => Some(
            taffy_layout::GridTemplateComponent::Repeat(taffy::style::GridTemplateRepetition {
                count: match repeat.count {
                    css::GridRepeatCount::Number(count) => {
                        taffy_layout::RepetitionCount::Count(count)
                    }
                    css::GridRepeatCount::AutoFill => taffy_layout::RepetitionCount::AutoFill,
                    css::GridRepeatCount::AutoFit => taffy_layout::RepetitionCount::AutoFit,
                },
                tracks: repeat
                    .tracks
                    .iter()
                    .filter_map(|component| match component {
                        css::GridTrackListComponent::Track(_, size) => Some(taffy_track_size(size)),
                        css::GridTrackListComponent::Repeat(_, _) => None,
                    })
                    .collect(),
                line_names: taffy_grid_repeat_line_names(repeat),
            }),
        ),
    }
}

pub(super) fn taffy_grid_template_line_names(
    tracks: &css::GridTrackList,
    areas: &css::GridTemplateAreas,
    axis: GridAxis,
) -> Vec<Vec<String>> {
    let mut line_names = match tracks {
        css::GridTrackList::None => Vec::new(),
        css::GridTrackList::Tracks {
            components,
            trailing_names,
        } => {
            let mut line_names = Vec::with_capacity(components.len() + 1);
            for component in components {
                match component {
                    css::GridTrackListComponent::Track(names, _)
                    | css::GridTrackListComponent::Repeat(names, _) => {
                        line_names.push(names.clone());
                    }
                }
            }
            line_names.push(trailing_names.clone());
            line_names
        }
    };
    add_generated_area_line_names(&mut line_names, areas, axis);
    line_names
}

pub(super) fn taffy_grid_repeat_line_names(repeat: &css::GridRepeat) -> Vec<Vec<String>> {
    let mut line_names = Vec::with_capacity(repeat.tracks.len() + 1);
    for component in &repeat.tracks {
        match component {
            css::GridTrackListComponent::Track(names, _)
            | css::GridTrackListComponent::Repeat(names, _) => line_names.push(names.clone()),
        }
    }
    line_names.push(repeat.trailing_names.clone());
    line_names
}

pub(super) fn taffy_grid_template_areas(
    value: &css::GridTemplateAreas,
) -> Vec<taffy::style::GridTemplateArea<String>> {
    let css::GridTemplateAreas::Areas(rows) = value else {
        return Vec::new();
    };
    collect_grid_template_area_bounds(rows)
        .into_iter()
        .filter_map(|area| {
            Some(taffy::style::GridTemplateArea {
                name: area.name.clone(),
                row_start: u16::try_from(area.row_start + 1).ok()?,
                row_end: u16::try_from(area.row_end + 2).ok()?,
                column_start: u16::try_from(area.column_start + 1).ok()?,
                column_end: u16::try_from(area.column_end + 2).ok()?,
            })
        })
        .collect()
}

fn collect_grid_template_area_bounds(
    rows: &[css::GridTemplateAreaRow],
) -> Vec<GridTemplateAreaBounds> {
    let mut areas: Vec<GridTemplateAreaBounds> = Vec::new();
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
                areas.push(GridTemplateAreaBounds {
                    name: name.clone(),
                    row_start: row_index,
                    row_end: row_index,
                    column_start: column_index,
                    column_end: column_index,
                });
            }
        }
    }
    areas
        .into_iter()
        .filter(|area| grid_template_area_is_rectangular(rows, area))
        .collect()
}

fn add_generated_area_line_names(
    line_names: &mut Vec<Vec<String>>,
    areas: &css::GridTemplateAreas,
    axis: GridAxis,
) {
    let css::GridTemplateAreas::Areas(rows) = areas else {
        return;
    };
    for area in collect_grid_template_area_bounds(rows) {
        let (start, end) = match axis {
            GridAxis::Row => (area.row_start, area.row_end + 1),
            GridAxis::Column => (area.column_start, area.column_end + 1),
        };
        ensure_grid_line_names_length(line_names, end + 1);
        add_grid_line_name(&mut line_names[start], format!("{}-start", area.name));
        add_grid_line_name(&mut line_names[end], format!("{}-end", area.name));
    }
}

fn ensure_grid_line_names_length(line_names: &mut Vec<Vec<String>>, len: usize) {
    line_names.resize_with(len, Vec::new);
}

fn add_grid_line_name(names: &mut Vec<String>, name: String) {
    if !names.iter().any(|existing| existing == &name) {
        names.push(name);
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum GridAxis {
    Row,
    Column,
}

#[derive(Debug, Clone)]
struct GridTemplateAreaBounds {
    name: String,
    row_start: usize,
    row_end: usize,
    column_start: usize,
    column_end: usize,
}

fn grid_template_area_is_rectangular(
    rows: &[css::GridTemplateAreaRow],
    area: &GridTemplateAreaBounds,
) -> bool {
    (area.row_start..=area.row_end).all(|row_index| {
        (area.column_start..=area.column_end).all(|column_index| {
            rows.get(row_index)
                .and_then(|row| row.cells.get(column_index))
                .is_some_and(|cell| cell.as_ref() == Some(&area.name))
        })
    })
}

pub(super) fn taffy_grid_auto_tracks(
    value: &css::GridAutoTrackList,
) -> Vec<taffy_layout::TrackSizingFunction> {
    value.tracks.iter().map(taffy_track_size).collect()
}

pub(super) fn taffy_track_size(value: &css::GridTrackSize) -> taffy_layout::TrackSizingFunction {
    taffy_layout::TrackSizingFunction {
        min: taffy_min_track_breadth(value.min),
        max: taffy_max_track_breadth(value.max),
    }
}

pub(super) fn taffy_min_track_breadth(
    value: css::GridMinTrackBreadth,
) -> taffy_layout::MinTrackSizingFunction {
    match value {
        css::GridMinTrackBreadth::Auto => taffy_layout::MinTrackSizingFunction::auto(),
        css::GridMinTrackBreadth::MinContent => taffy_layout::MinTrackSizingFunction::min_content(),
        css::GridMinTrackBreadth::MaxContent => taffy_layout::MinTrackSizingFunction::max_content(),
        css::GridMinTrackBreadth::LengthPercentage(value) => taffy_length_percentage(value).into(),
    }
}

pub(super) fn taffy_max_track_breadth(
    value: css::GridMaxTrackBreadth,
) -> taffy_layout::MaxTrackSizingFunction {
    match value {
        css::GridMaxTrackBreadth::Auto => taffy_layout::MaxTrackSizingFunction::auto(),
        css::GridMaxTrackBreadth::MinContent => taffy_layout::MaxTrackSizingFunction::min_content(),
        css::GridMaxTrackBreadth::MaxContent => taffy_layout::MaxTrackSizingFunction::max_content(),
        css::GridMaxTrackBreadth::LengthPercentage(value) => taffy_length_percentage(value).into(),
        css::GridMaxTrackBreadth::Flex(value) => taffy_layout::MaxTrackSizingFunction::fr(value),
        css::GridMaxTrackBreadth::FitContent(value) => {
            if value.percent != 0.0 && value.length == 0.0 {
                taffy_layout::MaxTrackSizingFunction::fit_content_percent(value.percent)
            } else {
                taffy_layout::MaxTrackSizingFunction::fit_content_px(value.length)
            }
        }
    }
}

pub(super) fn taffy_grid_line(
    start: &css::GridPlacement,
    end: &css::GridPlacement,
) -> taffy_layout::Line<taffy_layout::GridPlacement<String>> {
    let mut start = taffy_grid_placement(start);
    let mut end = taffy_grid_placement(end);
    if taffy_grid_placement_is_line(&start) && matches!(end, taffy_layout::GridPlacement::Auto) {
        end = taffy_layout::GridPlacement::Span(1);
    } else if matches!(start, taffy_layout::GridPlacement::Auto)
        && taffy_grid_placement_is_line(&end)
    {
        start = taffy_layout::GridPlacement::Span(1);
    }
    taffy_layout::Line { start, end }
}

pub(super) fn taffy_grid_placement_is_line(value: &taffy_layout::GridPlacement<String>) -> bool {
    matches!(
        value,
        taffy_layout::GridPlacement::Line(_) | taffy_layout::GridPlacement::NamedLine(_, _)
    )
}

pub(super) fn taffy_grid_placement(
    value: &css::GridPlacement,
) -> taffy_layout::GridPlacement<String> {
    match value {
        css::GridPlacement::Auto => taffy_layout::GridPlacement::Auto,
        css::GridPlacement::Line(line) => match (&line.name, line.index) {
            (Some(name), Some(index)) => i16::try_from(index)
                .ok()
                .map(|index| taffy_layout::GridPlacement::NamedLine(name.clone(), index))
                .unwrap_or(taffy_layout::GridPlacement::Auto),
            (Some(name), None) => taffy_layout::GridPlacement::NamedLine(name.clone(), 0),
            (None, Some(index)) => i16::try_from(index)
                .ok()
                .map(taffy_layout::line)
                .unwrap_or(taffy_layout::GridPlacement::Auto),
            (None, None) => taffy_layout::GridPlacement::Auto,
        },
        css::GridPlacement::Span(span) => match (&span.name, span.span) {
            (Some(name), Some(count)) => {
                taffy_layout::GridPlacement::NamedSpan(name.clone(), count)
            }
            (Some(name), None) => taffy_layout::GridPlacement::NamedSpan(name.clone(), 0),
            (None, Some(count)) => taffy_layout::GridPlacement::Span(count),
            (None, None) => taffy_layout::GridPlacement::Auto,
        },
    }
}

pub(super) fn taffy_grid_auto_flow(value: css::GridAutoFlow) -> taffy_layout::GridAutoFlow {
    match value {
        css::GridAutoFlow::Row => taffy_layout::GridAutoFlow::Row,
        css::GridAutoFlow::Column => taffy_layout::GridAutoFlow::Column,
        css::GridAutoFlow::RowDense => taffy_layout::GridAutoFlow::RowDense,
        css::GridAutoFlow::ColumnDense => taffy_layout::GridAutoFlow::ColumnDense,
    }
}
