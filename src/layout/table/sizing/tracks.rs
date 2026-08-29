//! Table column track constraints and width distribution.

use super::*;
use crate::units::IntoLayoutLength;

pub(in crate::layout::table) fn declared_table_cell_width(
    _cell: &Element,
    style: &ComputedStyle,
) -> Option<DeclaredTableTrackSize> {
    declared_table_track_size_from_computed(style.box_values.width.clone())
}

/// Return a first-row cell's declared contribution to a fixed table track.
///
/// The table root, rather than the cell, selects whether the CSS `width` or
/// `height` property supplies the root inline track. This remains separate
/// from [`declared_table_cell_width`], whose physical-width interpretation is
/// used only by the automatic-layout path.
pub(in crate::layout::table) fn declared_table_cell_track_size(
    table_inline_track: TableInlineTrackSizing,
    _cell: &Element,
    style: &ComputedStyle,
) -> Option<DeclaredTableTrackSize> {
    declared_table_track_size_from_computed(table_inline_track.declared_size(style))
}

/// Return a table-column's specified size on the table root's logical inline axis.
///
/// CSS Writing Modes keeps `width` and `height` physical, but applies the CSS
/// width-sizing rules to the logical inline dimension. Therefore a vertical
/// table's column tracks use physical `height`. `text-orientation` only
/// affects text inside line boxes (and does not apply to table columns), so it
/// must not alter this axis selection.
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
/// <https://drafts.csswg.org/css-writing-modes-4/#text-orientation>
/// <https://drafts.csswg.org/css-tables-3/#computing-column-measures>
pub(in crate::layout::table) fn declared_table_column_track_size(
    table_inline_track: TableInlineTrackSizing,
    style: &ComputedStyle,
) -> Option<DeclaredTableTrackSize> {
    declared_table_track_size_from_computed(table_inline_track.declared_size(style))
}

fn declared_table_track_size_from_computed(
    value: css::ComputedLengthPercentageOrAuto,
) -> Option<DeclaredTableTrackSize> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if let Some(percent) = value
                .pure_percentage_coefficient()
                .filter(|percent| *percent != 0.0)
            {
                Some(DeclaredTableTrackSize::Percent(percent))
            } else if value.needs_percentage_basis() {
                None
            } else {
                Some(DeclaredTableTrackSize::Fixed(value.length_points()))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
    }
}

pub(in crate::layout::table) fn resolve_declared_table_track_size(
    size: DeclaredTableTrackSize,
    table_inline_size: LayoutLength,
) -> LayoutLength {
    match size {
        DeclaredTableTrackSize::Fixed(size) => layout_pt(size),
        DeclaredTableTrackSize::Percent(percent) => layout_pt(table_inline_size.points() * percent),
    }
}

pub(in crate::layout::table) fn constrain_declared_table_track_size(
    table_inline_track: TableInlineTrackSizing,
    style: &ComputedStyle,
    size: DeclaredTableTrackSize,
    table_inline_size: ContentBoxLength,
) -> ContentBoxLength {
    table_inline_track.constrain_content_box_size(
        style,
        crate::units::layout_to_content_box_length(resolve_declared_table_track_size(
            size,
            table_inline_size.into_layout_length(),
        )),
        PercentageBasis::definite(table_inline_size.into_layout_length()),
    )
}

/// Resolve a declared table-cell track size to its column-space border-box size.
///
/// CSS Tables uses cell border boxes as column constraints, while CSS Sizing
/// applies a physical size to the cell content box unless `box-sizing`
/// says otherwise. Collapsed-border cells contribute the resolved half-border
/// insets on their outside grid edges, not their authored full border widths:
/// <https://drafts.csswg.org/css-tables-3/#computing-column-measures>
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
pub(in crate::layout::table) fn declared_table_cell_track_border_box_size(
    table_inline_track: TableInlineTrackSizing,
    style: &ComputedStyle,
    size: DeclaredTableTrackSize,
    table_inline_size: f32,
    border_insets: Option<css::Edges>,
) -> BorderBoxLength {
    let non_content = table_cell_track_non_content_size(table_inline_track, style, border_insets);
    let specified = resolve_declared_table_track_size(size, layout_pt(table_inline_size));
    table_cell_track_border_box_size_from_declared_size(
        table_inline_track,
        style,
        specified,
        layout_pt(table_inline_size),
        non_content,
    )
}

/// Return the fixed component of a declared table track size for intrinsic sizing.
pub(in crate::layout::table) fn declared_table_track_size_length_floor(
    size: DeclaredTableTrackSize,
) -> LayoutLength {
    match size {
        DeclaredTableTrackSize::Fixed(size) => layout_pt(size),
        DeclaredTableTrackSize::Percent(_) => layout_pt(0.0),
    }
}

pub(in crate::layout::table) fn declared_table_cell_width_length_floor(
    style: &ComputedStyle,
    width: DeclaredTableTrackSize,
    border_insets: Option<css::Edges>,
) -> BorderBoxLength {
    let non_content = table_cell_horizontal_non_content_width(style, border_insets);
    match width {
        DeclaredTableTrackSize::Fixed(width) => table_cell_border_box_width_from_declared_size(
            style,
            layout_pt(width),
            layout_pt(0.0),
            non_content,
        ),
        DeclaredTableTrackSize::Percent(_) => border_box_pt(0.0),
    }
}

fn table_cell_horizontal_non_content_width(
    style: &ComputedStyle,
    border_insets: Option<css::Edges>,
) -> NonContentLength {
    let border_width = border_insets
        .map(|borders| borders.left + borders.right)
        .map(non_content_pt)
        .unwrap_or_else(|| table_horizontal_borders(style));
    let padding = intrinsic_padding_edges(style).to_css_edges();
    non_content_pt(padding.left + padding.right) + border_width
}

pub(in crate::layout::table) fn table_cell_track_non_content_size(
    table_inline_track: TableInlineTrackSizing,
    style: &ComputedStyle,
    border_insets: Option<css::Edges>,
) -> NonContentLength {
    let border_width = border_insets
        .map(|borders| table_inline_track.parallel_insets(borders))
        .unwrap_or_else(|| {
            if table_inline_track.uses_physical_width() {
                table_horizontal_borders(style)
            } else {
                table_vertical_borders(style)
            }
        });
    let padding = table_inline_track.parallel_insets(intrinsic_padding_edges(style).to_css_edges());
    padding + border_width
}

fn table_cell_border_box_width_from_declared_size(
    style: &ComputedStyle,
    specified: LayoutLength,
    percentage_basis: LayoutLength,
    non_content: NonContentLength,
) -> BorderBoxLength {
    let content_width = match style.box_sizing {
        BoxSizing::BorderBox => border_box_to_content_box_length(
            crate::units::layout_to_border_box_length(specified),
            non_content,
        ),
        BoxSizing::ContentBox => content_box_pt(specified.points().max(0.0)),
    };
    content_box_to_border_box_length(
        constrain_content_width(
            style,
            content_width,
            PercentageBasis::definite(layout_pt(layout_points(percentage_basis).max(0.0))),
        ),
        non_content,
    )
}

pub(in crate::layout::table) fn table_cell_track_border_box_size_from_declared_size(
    table_inline_track: TableInlineTrackSizing,
    style: &ComputedStyle,
    specified: LayoutLength,
    percentage_basis: LayoutLength,
    non_content: NonContentLength,
) -> BorderBoxLength {
    let content_size = match style.box_sizing {
        BoxSizing::BorderBox => border_box_to_content_box_length(
            crate::units::layout_to_border_box_length(specified),
            non_content,
        ),
        BoxSizing::ContentBox => content_box_pt(specified.points().max(0.0)),
    };
    content_box_to_border_box_length(
        table_inline_track.constrain_content_box_size(
            style,
            content_size,
            PercentageBasis::definite(layout_pt(layout_points(percentage_basis).max(0.0))),
        ),
        non_content,
    )
}

pub(in crate::layout::table) fn declared_table_track_size_percentage(
    size: DeclaredTableTrackSize,
) -> f32 {
    match size {
        DeclaredTableTrackSize::Fixed(_) => 0.0,
        DeclaredTableTrackSize::Percent(percent) => percent,
    }
}

pub(in crate::layout::table) fn declared_table_track_size_is_non_percentage(
    size: DeclaredTableTrackSize,
) -> bool {
    declared_table_track_size_percentage(size) == 0.0
}

/// Column width inputs collected before the final table width is known.
///
/// CSS Tables 3 separates column min-content widths, max-content widths,
/// intrinsic percentage contributions, and constrainedness before running the
/// width distribution algorithm:
/// <https://drafts.csswg.org/css-tables-3/#computing-column-measures> and
/// <https://drafts.csswg.org/css-tables-3/#width-distribution-algorithm>.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableColumnMeasures {
    pub(in crate::layout::table) min_content_widths: Vec<f32>,
    pub(in crate::layout::table) max_content_widths: Vec<f32>,
    pub(in crate::layout::table) intrinsic_percentages: Vec<f32>,
    pub(in crate::layout::table) constrained: Vec<bool>,
    pub(in crate::layout::table) occupied: Vec<bool>,
    pub(in crate::layout::table) total_horizontal_spacing: f32,
}

impl TableColumnMeasures {
    pub(in crate::layout::table) fn table_min_content_width(&self) -> f32 {
        self.total_horizontal_spacing + self.min_content_widths.iter().sum::<f32>()
    }

    pub(in crate::layout::table) fn table_max_content_width(&self) -> f32 {
        let small_percentage_contribution = self
            .max_content_widths
            .iter()
            .zip(&self.intrinsic_percentages)
            .filter_map(|(width, percent)| {
                (*percent > 0.0).then_some(width / percent.max(f32::EPSILON))
            })
            .fold(0.0_f32, f32::max);
        let non_percentage_width = self
            .max_content_widths
            .iter()
            .zip(&self.intrinsic_percentages)
            .filter_map(|(width, percent)| (*percent == 0.0).then_some(*width))
            .sum::<f32>();
        let remaining_percentage = (1.0 - self.intrinsic_percentages.iter().sum::<f32>()).max(0.0);
        let large_percentage_contribution =
            if remaining_percentage == 0.0 && non_percentage_width > 0.0 {
                f32::MAX / 2.0
            } else if remaining_percentage == 0.0 {
                0.0
            } else {
                non_percentage_width / remaining_percentage
            };
        self.total_horizontal_spacing
            + self
                .max_content_widths
                .iter()
                .sum::<f32>()
                .max(small_percentage_contribution)
                .max(large_percentage_contribution)
    }
}

pub(in crate::layout::table) fn intrinsic_percentage_contribution(style: &ComputedStyle) -> f32 {
    let width = length_percentage_percent(style.box_values.width.clone())
        .map(TableIntrinsicPercentage::coefficient)
        .unwrap_or(0.0);
    let max_width = length_percentage_percent(style.box_values.max_width.clone())
        .map(TableIntrinsicPercentage::coefficient)
        .unwrap_or(f32::INFINITY);
    // CSS Tables intentionally excludes `min-width` from a column's
    // intrinsic percentage contribution. `width` already acts as a minimum
    // during table layout, while a percentage min-width must not turn an
    // otherwise auto table into a percentage-sized grid.
    // <https://drafts.csswg.org/css-tables-3/#computing-column-measures>
    width.min(max_width).max(0.0)
}

/// A pure percentage contribution to intrinsic table track sizing.
///
/// This stays distinct from a physical length: it is a unitless ratio that
/// only becomes a length once the table grid has a definite inline size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableIntrinsicPercentage(pub(in crate::layout::table) f32);

impl TableIntrinsicPercentage {
    fn coefficient(self) -> f32 {
        self.0
    }
}

pub(in crate::layout::table) fn length_percentage_percent(
    value: css::ComputedLengthPercentageOrAuto,
) -> Option<TableIntrinsicPercentage> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
            // Auto-table intrinsic sizing has no table grid width to resolve
            // a mixed length/percentage expression against. Only a pure
            // percentage establishes the column's percentage contribution;
            // `calc(50% + 1px)` stays cyclic at this stage.
            // <https://drafts.csswg.org/css-tables-3/#computing-column-measures>
            if value.pure_percentage_coefficient().is_some() =>
        {
            value
                .pure_percentage_coefficient()
                .map(TableIntrinsicPercentage)
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => None,
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
    }
}

pub(in crate::layout::table) fn constrain_table_intrinsic_width_with_floor(
    style: &ComputedStyle,
    value: f32,
    floor: f32,
) -> f32 {
    let min_width = intrinsic_length_constraint(style.box_values.min_width.clone());
    // CSS Sizing resolves a contradictory min/max pair in favour of the
    // minimum. Preserve that ordering before applying the outer table measure
    // rules, so `min-width: 100px; max-width: 0` contributes 100px rather
    // than disappearing from the table grid.
    // <https://www.w3.org/TR/css-sizing-3/#min-size-auto>
    let max_width = intrinsic_length_constraint(style.box_values.max_width.clone())
        .map(|maximum| maximum.max(min_width.unwrap_or_else(|| layout_pt(0.0))));
    constrain(value.max(floor), min_width, max_width)
}

pub(in crate::layout::table) fn intrinsic_length_constraint(
    value: css::ComputedLengthPercentageOrAuto,
) -> Option<LayoutLength> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
            if !value.needs_percentage_basis() =>
        {
            Some(layout_pt(value.length_points()))
        }
        // A mixed length-percentage min/max value is cyclic while the table's
        // intrinsic width is unknown. Its fixed component must not be used as
        // a partial min/max constraint: that would make `calc(100px + 1%)`
        // create a definite missing-column track.
        // <https://drafts.csswg.org/css-tables-3/#computing-column-measures>
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => None,
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
    }
}

fn constrain(value: f32, min: Option<LayoutLength>, max: Option<LayoutLength>) -> f32 {
    let value = min.map(|min| value.max(min.points())).unwrap_or(value);
    max.map(|max| value.min(max.points())).unwrap_or(value)
}

/// Distribute extra assignable table width across a column range.
///
/// CSS Tables 3 defines ordered receiver groups for width distribution:
/// unconstrained non-percentage columns, unconstrained zero-base columns,
/// constrained non-percentage columns, percentage columns, occupied columns,
/// then all columns.
/// <https://drafts.csswg.org/css-tables-3/#distributing-width-to-columns>.
pub(in crate::layout::table) fn distribute_table_excess_width(
    measures: &TableColumnMeasures,
    widths: &mut [f32],
    excess_width: f32,
    column_range: std::ops::Range<usize>,
) {
    if excess_width <= 0.0 || column_range.is_empty() {
        return;
    }

    let columns = column_range
        .clone()
        .filter(|index| {
            !measures.constrained[*index]
                && measures.intrinsic_percentages[*index] == 0.0
                && measures.max_content_widths[*index] > 0.0
        })
        .collect::<Vec<_>>();
    if !columns.is_empty() {
        distribute_proportional(widths, excess_width, &columns, |index| {
            measures.max_content_widths[index]
        });
        return;
    }

    let columns = column_range
        .clone()
        .filter(|index| {
            !measures.constrained[*index] && measures.intrinsic_percentages[*index] == 0.0
        })
        .collect::<Vec<_>>();
    if !columns.is_empty() {
        distribute_evenly(widths, excess_width, &columns);
        return;
    }

    let columns = column_range
        .clone()
        .filter(|index| {
            measures.constrained[*index]
                && measures.intrinsic_percentages[*index] == 0.0
                && measures.max_content_widths[*index] > 0.0
        })
        .collect::<Vec<_>>();
    if !columns.is_empty() {
        distribute_proportional(widths, excess_width, &columns, |index| {
            measures.max_content_widths[index]
        });
        return;
    }

    let columns = column_range
        .clone()
        .filter(|index| {
            measures.intrinsic_percentages[*index] > 0.0
                && measures.max_content_widths[*index] > 0.0
        })
        .collect::<Vec<_>>();
    if !columns.is_empty() {
        distribute_proportional(widths, excess_width, &columns, |index| {
            measures.intrinsic_percentages[index]
        });
        return;
    }

    let columns = column_range
        .clone()
        .filter(|index| measures.occupied[*index])
        .collect::<Vec<_>>();
    if !columns.is_empty() {
        distribute_evenly(widths, excess_width, &columns);
        return;
    }

    distribute_evenly(widths, excess_width, &column_range.collect::<Vec<_>>());
}

fn distribute_proportional(
    widths: &mut [f32],
    extra_width: f32,
    columns: &[usize],
    weight: impl Fn(usize) -> f32,
) {
    let total = columns
        .iter()
        .map(|index| weight(*index).max(0.0))
        .sum::<f32>();
    if total <= 0.0 {
        distribute_evenly(widths, extra_width, columns);
        return;
    }
    for index in columns {
        widths[*index] += extra_width * (weight(*index).max(0.0) / total);
    }
}

fn distribute_evenly(widths: &mut [f32], extra_width: f32, columns: &[usize]) {
    if columns.is_empty() {
        return;
    }
    let extra_per_column = extra_width / columns.len() as f32;
    for index in columns {
        widths[*index] += extra_per_column;
    }
}

pub(in crate::layout::table) fn distribute_fixed_width(
    widths: &mut [TableGridLength],
    declared: &mut [bool],
    column: usize,
    colspan: usize,
    target_width: TableGridLength,
) {
    let end = (column + colspan.max(1)).min(widths.len());
    if column >= end {
        return;
    }
    let current = widths[column..end]
        .iter()
        .copied()
        .fold(TableGridLength::new(0.0), |sum, width| sum + width);
    if target_width > current {
        let extra = TableGridLength::new((target_width - current).get() / (end - column) as f32);
        for width in &mut widths[column..end] {
            *width += extra;
        }
    }
    for is_declared in &mut declared[column..end] {
        *is_declared = true;
    }
}

pub(in crate::layout::table) fn distribute_first_row_fixed_width(
    widths: &mut [TableGridLength],
    declared: &mut [bool],
    column: usize,
    colspan: usize,
    target_width: TableGridLength,
) {
    let end = (column + colspan.max(1)).min(widths.len());
    if column >= end {
        return;
    }
    let current = widths[column..end]
        .iter()
        .copied()
        .fold(TableGridLength::new(0.0), |sum, width| sum + width);
    let receivers = (column..end)
        .filter(|index| !declared[*index])
        .collect::<Vec<_>>();
    if receivers.is_empty() {
        return;
    }
    if target_width > current {
        let extra = TableGridLength::new((target_width - current).get() / receivers.len() as f32);
        for index in &receivers {
            widths[*index] += extra;
        }
    }
    for index in receivers {
        declared[index] = true;
    }
}

pub(in crate::layout::table) fn apply_table_column_style_measures(
    measures: &mut TableColumnMeasures,
    column: usize,
    colspan: usize,
    style: &ComputedStyle,
) {
    let end = (column + colspan).min(measures.min_content_widths.len());
    if column >= end {
        return;
    }
    let declared_width =
        declared_table_column_track_size(TableInlineTrackSizing::for_table(style), style);
    let width_floor = declared_width
        .clone()
        .map(declared_table_track_size_length_floor)
        // Intrinsic table-width constraint distribution is scalar arithmetic.
        .unwrap_or_else(|| layout_pt(0.0))
        .points();
    let min_width = constrain_table_intrinsic_width_with_floor(style, 0.0, width_floor);
    let max_width = constrain_table_intrinsic_width_with_floor(style, min_width, width_floor);
    let percentage = intrinsic_percentage_contribution(style).max(
        declared_width
            .clone()
            .map(declared_table_track_size_percentage)
            .unwrap_or(0.0),
    );
    for index in column..end {
        measures.min_content_widths[index] = measures.min_content_widths[index].max(min_width);
        measures.max_content_widths[index] = measures.max_content_widths[index].max(max_width);
        measures.intrinsic_percentages[index] =
            measures.intrinsic_percentages[index].max(percentage);
        if declared_width
            .clone()
            .is_some_and(declared_table_track_size_is_non_percentage)
        {
            measures.constrained[index] = true;
        }
    }
}

pub(in crate::layout::table) fn distribute_spanned_percentage(
    measures: &mut TableColumnMeasures,
    start: usize,
    end: usize,
    percentage: f32,
) {
    if percentage <= 0.0 || start >= end {
        return;
    }
    let current = measures.intrinsic_percentages[start..end]
        .iter()
        .sum::<f32>();
    if percentage <= current {
        return;
    }
    let extra = percentage - current;
    let receivers = (start..end)
        .filter(|index| measures.intrinsic_percentages[*index] == 0.0)
        .collect::<Vec<_>>();
    let receivers = if receivers.is_empty() {
        (start..end).collect::<Vec<_>>()
    } else {
        receivers
    };
    let max_content_sum = receivers
        .iter()
        .map(|index| measures.max_content_widths[*index].max(0.0))
        .sum::<f32>();
    let receiver_count = receivers.len().max(1) as f32;
    for index in receivers {
        let ratio = if max_content_sum > 0.0 {
            measures.max_content_widths[index].max(0.0) / max_content_sum
        } else {
            1.0 / receiver_count
        };
        measures.intrinsic_percentages[index] += extra * ratio;
    }
}

pub(in crate::layout::table) fn distribute_spanned_measure(
    measures: &mut TableColumnMeasures,
    start: usize,
    end: usize,
    target_width: f32,
    min_content: bool,
) {
    if target_width <= 0.0 || start >= end {
        return;
    }
    let baseline_max_content = measures.max_content_widths[start..end].iter().sum::<f32>();
    let baseline_measure = if min_content {
        measures.min_content_widths[start..end].iter().sum::<f32>()
    } else {
        baseline_max_content
    };
    if target_width <= baseline_measure {
        return;
    }

    let has_percentage_column = measures.intrinsic_percentages[start..end]
        .iter()
        .any(|percentage| *percentage > 0.0);
    if !has_percentage_column {
        let snapshot = measures.clone();
        let widths = if min_content {
            &mut measures.min_content_widths
        } else {
            &mut measures.max_content_widths
        };
        distribute_table_excess_width(
            &snapshot,
            widths,
            target_width - baseline_measure,
            start..end,
        );
        return;
    }

    // A spanning cell contributes to intrinsic measures through the measures
    // that existed before that span is considered. In particular, percentage
    // columns must retain their proportional share: applying the used-width
    // excess-distribution rules would transfer all of a spanning cell's
    // intrinsic width to an auto column beside a percentage column.
    //
    // CSS Tables 3 § 3.8.3, “min-content width of a column based on cells of
    // span up to N” and “max-content width of a column based on cells of span
    // up to N”: <https://drafts.csswg.org/css-tables-3/#computing-column-measures>
    let baseline_min_content = measures.min_content_widths[start..end].iter().sum::<f32>();
    let additional_above_max = (target_width - baseline_max_content).max(0.0);
    let within_min_to_max = (target_width - baseline_min_content)
        .clamp(0.0, (baseline_max_content - baseline_min_content).max(0.0));
    let column_count = (end - start) as f32;

    for index in start..end {
        let prior_max = measures.max_content_widths[index];
        let ratio_of_max = if baseline_max_content > 0.0 {
            prior_max / baseline_max_content
        } else {
            1.0 / column_count
        };
        if min_content {
            let prior_min = measures.min_content_widths[index];
            let ratio_within_range = if baseline_max_content > baseline_min_content {
                (prior_max - prior_min) / (baseline_max_content - baseline_min_content)
            } else {
                0.0
            };
            let contribution = prior_min
                + ratio_within_range * within_min_to_max
                + ratio_of_max * additional_above_max;
            measures.min_content_widths[index] =
                measures.min_content_widths[index].max(contribution);
        } else {
            let contribution = prior_max + ratio_of_max * additional_above_max;
            measures.max_content_widths[index] =
                measures.max_content_widths[index].max(contribution);
        }
    }
}

pub(in crate::layout::table) fn cap_intrinsic_percentages(percentages: &mut [f32]) {
    let mut used = 0.0_f32;
    for percentage in percentages {
        let remaining = (1.0 - used).max(0.0);
        *percentage = percentage.max(0.0).min(remaining);
        used += *percentage;
    }
}

/// Resolve final auto-layout column widths from precomputed table measures.
///
/// CSS Tables 3 chooses among min-content, percentage, specified, and
/// max-content guesses, interpolates when the assignable width falls between
/// guesses, and distributes remaining width after max-content:
/// <https://drafts.csswg.org/css-tables-3/#width-distribution-algorithm>.
pub(in crate::layout::table) fn auto_table_column_widths(
    measures: &TableColumnMeasures,
    assignable_width: f32,
) -> Vec<f32> {
    let min_content_guess = measures.min_content_widths.clone();
    let mut min_content_percentage_guess = measures.min_content_widths.clone();
    let mut min_content_specified_guess = measures.min_content_widths.clone();
    let mut max_content_guess = measures.max_content_widths.clone();

    for index in 0..measures.min_content_widths.len() {
        if measures.intrinsic_percentages[index] > 0.0 {
            let percentage_width = (measures.intrinsic_percentages[index] * assignable_width)
                .max(measures.min_content_widths[index]);
            min_content_percentage_guess[index] = percentage_width;
            min_content_specified_guess[index] = percentage_width;
            max_content_guess[index] = percentage_width;
        } else if measures.constrained[index] {
            min_content_specified_guess[index] = measures.max_content_widths[index];
        }
    }

    if assignable_width < max_content_guess.iter().sum::<f32>() {
        let guesses = [
            min_content_guess.as_slice(),
            min_content_percentage_guess.as_slice(),
            min_content_specified_guess.as_slice(),
            max_content_guess.as_slice(),
        ];
        let mut lower_guess = guesses[0];
        let mut upper_guess = guesses[guesses.len() - 1];
        for guess in guesses {
            if guess.iter().sum::<f32>() <= assignable_width * (1.0 + 1e-6) {
                lower_guess = guess;
            } else {
                upper_guess = guess;
                break;
            }
        }
        let lower_sum = lower_guess.iter().sum::<f32>();
        let upper_sum = upper_guess.iter().sum::<f32>();
        if (upper_sum - lower_sum).abs() <= 0.01 {
            return upper_guess.to_vec();
        }
        let ratio = ((assignable_width - lower_sum) / (upper_sum - lower_sum)).clamp(0.0, 1.0);
        return lower_guess
            .iter()
            .zip(upper_guess)
            .map(|(lower, upper)| lower + (upper - lower) * ratio)
            .collect();
    }

    let mut widths = max_content_guess;
    let excess_width = assignable_width - widths.iter().sum::<f32>();
    let width_count = widths.len();
    distribute_table_excess_width(measures, &mut widths, excess_width, 0..width_count);
    widths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentage_columns_preserve_fractional_assignable_widths() {
        let measures = TableColumnMeasures {
            min_content_widths: vec![0.0, 0.0],
            max_content_widths: vec![0.0, 0.0],
            intrinsic_percentages: vec![0.5, 0.5],
            constrained: vec![false, false],
            occupied: vec![true, true],
            total_horizontal_spacing: 0.0,
        };

        let widths = auto_table_column_widths(&measures, 5.4);

        assert_eq!(widths, vec![2.7, 2.7]);
        assert!((widths.iter().sum::<f32>() - 5.4).abs() < 1e-6);
    }
}
