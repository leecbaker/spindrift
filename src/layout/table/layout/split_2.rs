use super::*;

pub(in crate::layout::table) fn table_plan_height(
    row: &TableRowHeightPlan,
    target: TableHeightTarget,
) -> f32 {
    match target {
        TableHeightTarget::Base => row.base,
        TableHeightTarget::Reference => row.reference,
    }
}

pub(in crate::layout::table) fn table_plan_height_mut(
    row: &mut TableRowHeightPlan,
    target: TableHeightTarget,
) -> &mut f32 {
    match target {
        TableHeightTarget::Base => &mut row.base,
        TableHeightTarget::Reference => &mut row.reference,
    }
}

impl TableHeightPlan {
    pub(in crate::layout::table) fn final_row_heights(&self) -> Vec<f32> {
        self.rows.iter().map(|row| row.final_height).collect()
    }

    pub(in crate::layout::table) fn row_occupancy(&self) -> Vec<bool> {
        self.rows.iter().map(|row| !row.collapsed).collect()
    }
}

pub(in crate::layout::table) fn table_content_height_from_plan(
    rows: &[TableRowHeightPlan],
    target: TableHeightTarget,
    table_metrics: TableMetrics,
) -> f32 {
    let heights = rows
        .iter()
        .map(|row| table_plan_height(row, target))
        .collect::<Vec<_>>();
    let occupancy = rows.iter().map(|row| !row.collapsed).collect::<Vec<_>>();
    table_content_height(&heights, &occupancy, table_metrics)
}

pub(in crate::layout::table) fn table_span_height_from_plan(
    rows: &[TableRowHeightPlan],
    row: usize,
    rowspan: usize,
    target: TableHeightTarget,
    table_metrics: TableMetrics,
) -> f32 {
    let heights = rows
        .iter()
        .map(|row| table_plan_height(row, target))
        .collect::<Vec<_>>();
    let occupancy = rows.iter().map(|row| !row.collapsed).collect::<Vec<_>>();
    table_row_span_height(&heights, &occupancy, row, rowspan, table_metrics)
}

pub(in crate::layout::table) fn distribute_table_span_constraint(
    rows: &mut [TableRowHeightPlan],
    row: usize,
    rowspan: usize,
    required_height: f32,
    table_metrics: TableMetrics,
    target: TableHeightTarget,
) {
    if row >= rows.len() {
        return;
    }
    let current_height = table_span_height_from_plan(rows, row, rowspan, target, table_metrics);
    let extra = required_height - current_height;
    if extra <= 0.01 {
        return;
    }

    let end = (row + rowspan.max(1)).min(rows.len());
    let auto_receivers = (row..end)
        .filter(|index| !rows[*index].collapsed && rows[*index].auto)
        .collect::<Vec<_>>();
    let receivers = if auto_receivers.is_empty() {
        (row..end)
            .filter(|index| !rows[*index].collapsed)
            .collect::<Vec<_>>()
    } else {
        auto_receivers
    };
    if receivers.is_empty() {
        return;
    }

    let share = extra / receivers.len() as f32;
    for index in receivers {
        *table_plan_height_mut(&mut rows[index], target) += share;
    }
}

pub(in crate::layout::table) fn distribute_table_height_extra(
    rows: &mut [TableRowHeightPlan],
    extra: f32,
    predicate: impl Fn(&TableRowHeightPlan) -> bool,
) -> f32 {
    if extra <= 0.01 {
        return 0.0;
    }
    let receivers = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| predicate(row).then_some(index))
        .collect::<Vec<_>>();
    if receivers.is_empty() {
        return 0.0;
    }

    let share = extra / receivers.len() as f32;
    for index in &receivers {
        rows[*index].final_height += share;
    }
    extra
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
    let declared_width = declared_table_column_width(style);
    let width_floor = declared_width
        .map(declared_table_width_length_floor)
        .unwrap_or(0.0);
    let min_width = constrain_table_intrinsic_width_with_floor(style, 0.0, width_floor);
    let max_width = constrain_table_intrinsic_width_with_floor(style, min_width, width_floor);
    let percentage = intrinsic_percentage_contribution(style).max(
        declared_width
            .map(declared_table_width_percentage)
            .unwrap_or(0.0),
    );
    for index in column..end {
        measures.min_content_widths[index] = measures.min_content_widths[index].max(min_width);
        measures.max_content_widths[index] = measures.max_content_widths[index].max(max_width);
        measures.intrinsic_percentages[index] =
            measures.intrinsic_percentages[index].max(percentage);
        if declared_width.is_some_and(declared_table_width_is_non_percentage) {
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
    let current = if min_content {
        measures.min_content_widths[start..end].iter().sum::<f32>()
    } else {
        measures.max_content_widths[start..end].iter().sum::<f32>()
    };
    if target_width <= current {
        return;
    }
    let snapshot = measures.clone();
    let widths = if min_content {
        &mut measures.min_content_widths
    } else {
        &mut measures.max_content_widths
    };
    distribute_table_excess_width(&snapshot, widths, target_width - current, start..end);
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
