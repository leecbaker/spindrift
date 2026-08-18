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

    pub(in crate::layout::table) fn source_row_heights(&self) -> Vec<f32> {
        self.rows.iter().map(|row| row.source_height).collect()
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
