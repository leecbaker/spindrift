use super::{
    StartwardImplicitTrackAdjustment, css, grid_line_index,
    negative_named_implicit_grid_line_index, taffy_layout,
};

pub(in crate::layout::grid) fn taffy_grid_line(
    start: &css::GridPlacement,
    end: &css::GridPlacement,
) -> taffy_layout::Line<taffy_layout::GridPlacement<String>> {
    normalized_taffy_grid_line(taffy_grid_placement(start), taffy_grid_placement(end))
}

/// Fill the omitted side of a definite grid line with the CSS one-track span.
fn normalized_taffy_grid_line(
    mut start: taffy_layout::GridPlacement<String>,
    mut end: taffy_layout::GridPlacement<String>,
) -> taffy_layout::Line<taffy_layout::GridPlacement<String>> {
    if taffy_grid_placement_is_line(&start) && matches!(end, taffy_layout::GridPlacement::Auto) {
        end = taffy_layout::GridPlacement::Span(1);
    } else if matches!(start, taffy_layout::GridPlacement::Auto)
        && taffy_grid_placement_is_line(&end)
    {
        start = taffy_layout::GridPlacement::Span(1);
    }
    taffy_layout::Line { start, end }
}

pub(in crate::layout::grid) fn taffy_grid_line_with_startward_adjustment(
    start: &css::GridPlacement,
    end: &css::GridPlacement,
    adjustment: &StartwardImplicitTrackAdjustment,
) -> taffy_layout::Line<taffy_layout::GridPlacement<String>> {
    if !adjustment.has_startward_tracks() {
        return taffy_grid_line(start, end);
    }
    if let Some(range) =
        backward_named_span_startward_line_range(start, end, &adjustment.explicit_line_names)
    {
        return shifted_taffy_grid_line(range, adjustment.line_shift());
    }
    if let Some(range) =
        negative_named_line_startward_line_range(start, end, &adjustment.explicit_line_names)
    {
        return shifted_taffy_grid_line(range, adjustment.line_shift());
    }
    normalized_taffy_grid_line(
        taffy_grid_placement_with_positive_line_shift(start, adjustment.line_shift()),
        taffy_grid_placement_with_positive_line_shift(end, adjustment.line_shift()),
    )
}

fn shifted_taffy_grid_line(
    range: std::ops::Range<i32>,
    shift: i32,
) -> taffy_layout::Line<taffy_layout::GridPlacement<String>> {
    taffy_layout::Line {
        start: taffy_layout::line(taffy_grid_line_index(range.start.saturating_add(shift))),
        end: taffy_layout::line(taffy_grid_line_index(range.end.saturating_add(shift))),
    }
}

fn taffy_grid_line_index(value: i32) -> i16 {
    let value = value.clamp(1, i32::from(i16::MAX));
    i16::try_from(value).expect("clamped grid line index fits in i16")
}

pub(super) fn backward_named_span_startward_line_range(
    start: &css::GridPlacement,
    end: &css::GridPlacement,
    explicit_line_names: &[Vec<String>],
) -> Option<std::ops::Range<i32>> {
    let css::GridPlacement::Span(span) = start else {
        return None;
    };
    let name = span.name()?;
    let target = span.count().unwrap_or(1);
    if target == 0 {
        return None;
    }
    let end = grid_line_index(end, explicit_line_names)?;
    let mut matches_seen = 0_u16;
    for (index, names) in explicit_line_names.iter().enumerate().rev() {
        let line_index = i32::try_from(index + 1).ok()?;
        if line_index >= end {
            continue;
        }
        if names.iter().any(|line_name| line_name == name) {
            matches_seen += 1;
            if matches_seen == target {
                return Some(line_index..end);
            }
        }
    }
    let missing = i32::from(target - matches_seen);
    let start_line = 1_i32.checked_sub(missing)?;
    Some(start_line..end)
}

/// Resolve a negative named line placement that falls before the explicit grid.
///
/// CSS Grid treats implicit lines on the search side as having the requested
/// name when an occurrence cannot be satisfied by explicit named lines:
/// <https://www.w3.org/TR/css-grid-1/#grid-placement-slot>.
pub(super) fn negative_named_line_startward_line_range(
    start: &css::GridPlacement,
    end: &css::GridPlacement,
    explicit_line_names: &[Vec<String>],
) -> Option<std::ops::Range<i32>> {
    let css::GridPlacement::Line(line) = start else {
        return None;
    };
    let name = line.name()?;
    let occurrence = line.index().unwrap_or(1);
    if occurrence >= 0 {
        return None;
    }
    let start_line =
        negative_named_implicit_grid_line_index(explicit_line_names, name, occurrence)?;
    if start_line >= 1 {
        return None;
    }
    let end_line = match end {
        css::GridPlacement::Auto => start_line.checked_add(1)?,
        css::GridPlacement::Line(_) => grid_line_index(end, explicit_line_names)?,
        css::GridPlacement::Span(span) if span.name().is_none() => {
            start_line.checked_add(i32::from(span.count().unwrap_or(1)))?
        }
        css::GridPlacement::Span(_) => return None,
    };
    Some(start_line..end_line)
}

pub(in crate::layout::grid) fn taffy_grid_placement_is_line(
    value: &taffy_layout::GridPlacement<String>,
) -> bool {
    matches!(
        value,
        taffy_layout::GridPlacement::Line(_) | taffy_layout::GridPlacement::NamedLine(_, _)
    )
}

pub(in crate::layout::grid) fn taffy_grid_placement(
    value: &css::GridPlacement,
) -> taffy_layout::GridPlacement<String> {
    match value {
        css::GridPlacement::Auto => taffy_layout::GridPlacement::Auto,
        css::GridPlacement::Line(line) => match line {
            css::GridLinePlacement::Number(index) => i16::try_from(index.get())
                .ok()
                .map_or(taffy_layout::GridPlacement::Auto, taffy_layout::line),
            css::GridLinePlacement::Named { name, occurrence } => occurrence
                .map(std::num::NonZero::get)
                .map(i16::try_from)
                .transpose()
                .ok()
                .flatten()
                .map_or_else(
                    || taffy_layout::GridPlacement::NamedLine(name.clone(), 0),
                    |index| taffy_layout::GridPlacement::NamedLine(name.clone(), index),
                ),
        },
        css::GridPlacement::Span(span) => match span {
            css::GridSpanPlacement::Count(count) => taffy_layout::GridPlacement::Span(count.get()),
            css::GridSpanPlacement::Named { name, count } => {
                count.map(std::num::NonZero::get).map_or_else(
                    || taffy_layout::GridPlacement::NamedSpan(name.clone(), 0),
                    |count| taffy_layout::GridPlacement::NamedSpan(name.clone(), count),
                )
            }
        },
    }
}

pub(in crate::layout::grid) fn taffy_grid_auto_flow(
    value: css::GridAutoFlow,
) -> taffy_layout::GridAutoFlow {
    match value {
        css::GridAutoFlow::Row => taffy_layout::GridAutoFlow::Row,
        css::GridAutoFlow::Column => taffy_layout::GridAutoFlow::Column,
        css::GridAutoFlow::RowDense => taffy_layout::GridAutoFlow::RowDense,
        css::GridAutoFlow::ColumnDense => taffy_layout::GridAutoFlow::ColumnDense,
    }
}

fn taffy_grid_placement_with_positive_line_shift(
    value: &css::GridPlacement,
    shift: i32,
) -> taffy_layout::GridPlacement<String> {
    match value {
        css::GridPlacement::Line(line) if line.name().is_none() => line
            .index()
            .and_then(|index| {
                let shifted = if index > 0 {
                    index.checked_add(shift)?
                } else {
                    index
                };
                i16::try_from(shifted).ok()
            })
            .map_or(taffy_layout::GridPlacement::Auto, taffy_layout::line),
        _ => taffy_grid_placement(value),
    }
}
