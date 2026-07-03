use super::*;
use crate::layout::table::layout::{
    table_cell_child_is_in_flow_float, table_cell_style_has_parent_percentage_block_size,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct UsedTableWidth {
    pub(super) content_width: ContentBoxLength,
    pub(super) border_widths: css::Edges,
    pub(super) padding: css::Edges,
}

impl UsedTableWidth {
    pub(super) fn content_width_points(self) -> f32 {
        self.content_width.points()
    }

    pub(super) fn content_x(self, outer_x: f32) -> f32 {
        outer_x + self.border_widths.left + self.padding.left
    }

    pub(super) fn horizontal_non_content(self, style: &ComputedStyle) -> NonContentLength {
        let border_width = if style.border_collapse == css::BorderCollapse::Collapse {
            0.0
        } else {
            self.border_widths.left + self.border_widths.right
        };
        non_content_pt(border_width + self.padding.left + self.padding.right)
    }

    pub(super) fn wrapper_border_box_width(
        self,
        content_width: ContentBoxLength,
    ) -> BorderBoxLength {
        content_box_to_border_box_length(
            content_width,
            non_content_pt(
                self.padding.left
                    + self.padding.right
                    + self.border_widths.left
                    + self.border_widths.right,
            ),
        )
    }
}

/// Resolves the table wrapper's used width into the content/grid width.
///
/// CSS Tables lays out columns in the table grid, while CSS Box Sizing defines
/// whether the authored `width` applies to the content box or border box. In
/// the collapsed border model, table borders are conflict-resolved grid-edge
/// borders rather than ordinary separated wrapper borders, so they are not
/// subtracted from the grid width here:
/// <https://www.w3.org/TR/css-tables-3/#layout> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing> and
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
pub(super) fn used_table_width(
    style: &ComputedStyle,
    available_outer_width: f32,
) -> UsedTableWidth {
    let collapsed = style.border_collapse == css::BorderCollapse::Collapse;
    let border_widths = if collapsed {
        css::Edges::ZERO
    } else {
        used_border_widths(style)
    };
    let padding = if collapsed {
        css::Edges::ZERO
    } else {
        used_padding_edges(style, available_outer_width).to_css_edges()
    };
    let width = UsedTableWidth {
        content_width: content_box_pt(0.0),
        border_widths,
        padding,
    };
    let horizontal_non_content = width.horizontal_non_content(style);
    let requested_content_width = used_content_box_size(
        style.box_values.width,
        style.box_sizing,
        available_outer_width,
        horizontal_non_content,
    )
    .unwrap_or_else(|| {
        content_box_pt((available_outer_width - horizontal_non_content.points()).max(0.0))
    });
    let content_width = content_box_pt(
        constrain_width(
            style,
            requested_content_width.points(),
            available_outer_width,
        )
        .max(style.font_size),
    );

    UsedTableWidth {
        content_width,
        ..width
    }
}

/// Resolves the row-grid content width for a table with no rows or cells.
///
/// CSS Tables 3 keeps an empty table's grid box in layout: if the grid has no
/// slots and `width` is auto, the grid content width is zero. In collapsed
/// border mode CSS 2.2 derives wrapper border insets from the collapsed grid;
/// with no slots that grid contributes no padding or border inset.
/// <https://drafts.csswg.org/css-tables/#computing-the-table-width> and
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
pub(super) fn used_empty_table_grid_width(
    style: &ComputedStyle,
    available_outer_width: f32,
    table_width: UsedTableWidth,
) -> ContentBoxLength {
    let horizontal_non_content = table_width.horizontal_non_content(style);
    let requested_content_width = used_content_box_size(
        style.box_values.width,
        style.box_sizing,
        available_outer_width,
        horizontal_non_content,
    )
    .unwrap_or_else(|| content_box_pt(0.0));
    content_box_pt(
        constrain_width(
            style,
            requested_content_width.points(),
            available_outer_width,
        )
        .max(0.0),
    )
}

/// Resolves the row-grid content height for a table with no rows or cells.
///
/// CSS Tables 3 treats a definite table grid box height as the table's minimum
/// row-grid height. With no rows and auto height, that grid content height is
/// zero; collapsed tables have no separated wrapper padding or border around
/// that empty grid:
/// <https://drafts.csswg.org/css-tables/#computing-the-table-height>.
pub(super) fn used_empty_table_grid_height(
    style: &ComputedStyle,
    percentage_height_basis: Option<f32>,
    table_width: UsedTableWidth,
) -> f32 {
    let vertical_non_content = table_width.border_widths.top
        + table_width.border_widths.bottom
        + table_width.padding.top
        + table_width.padding.bottom;
    used_table_target_content_height(style, percentage_height_basis, vertical_non_content)
        .unwrap_or(0.0)
}

/// Resolve a table wrapper's definite block-size constraints to a grid target.
///
/// CSS Tables computes row heights inside the table grid box, while `height`,
/// `min-height`, and `max-height` apply to the table wrapper box. In separated
/// border mode, wrapper padding and border sit outside the grid and must be
/// removed from definite wrapper sizes before row height distribution sees
/// them; collapsed borders do not contribute ordinary wrapper non-content.
/// `max-height` caps a target created by `height` or `min-height`, but does
/// not create a target by itself and shrink intrinsic rows:
/// <https://drafts.csswg.org/css-tables/#computing-the-table-height> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(super) fn used_table_target_content_height(
    style: &ComputedStyle,
    percentage_height_basis: Option<f32>,
    vertical_non_content: f32,
) -> Option<f32> {
    let height = table_wrapper_size_to_grid_content_height(
        style.box_values.height,
        style.box_sizing,
        percentage_height_basis,
        vertical_non_content,
    );
    let min_height = table_wrapper_size_to_grid_content_height(
        style.box_values.min_height,
        style.box_sizing,
        percentage_height_basis,
        vertical_non_content,
    );
    let max_height = table_wrapper_size_to_grid_content_height(
        style.box_values.max_height,
        style.box_sizing,
        percentage_height_basis,
        vertical_non_content,
    );

    let mut target = height.or(min_height)?;
    if let Some(max_height) = max_height {
        target = target.min(max_height);
    }
    if let Some(min_height) = min_height {
        target = target.max(min_height);
    }
    Some(target.max(0.0))
}

fn table_wrapper_size_to_grid_content_height(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: Option<f32>,
    vertical_non_content: f32,
) -> Option<f32> {
    let specified = used_length_percentage_or_auto_with_optional_basis(value, percentage_basis)?;
    Some(match box_sizing {
        BoxSizing::BorderBox => (specified - vertical_non_content).max(0.0),
        BoxSizing::ContentBox => specified.max(0.0),
    })
}

pub(super) fn declared_table_cell_width(
    cell: &Element,
    style: &ComputedStyle,
) -> Option<DeclaredTableWidth> {
    match style.box_values.width {
        css::ComputedLengthPercentageOrAuto::Auto => html_width_for_table_cell(cell),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.math.is_some() {
                Some(DeclaredTableWidth::LengthPercentage(value))
            } else if value.percent != 0.0 && value.length_is_zero() {
                Some(DeclaredTableWidth::Percent(value.percent))
            } else if value.percent != 0.0 {
                Some(DeclaredTableWidth::LengthPercentage(value))
            } else {
                Some(DeclaredTableWidth::Fixed(value.length_points()))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
    }
}

pub(super) fn declared_table_column_width(style: &ComputedStyle) -> Option<DeclaredTableWidth> {
    let inline_size = match style.writing_mode {
        WritingMode::HorizontalTb => style.box_values.width,
        WritingMode::VerticalRl | WritingMode::VerticalLr
            if style.text_orientation != css::TextOrientation::Sideways =>
        {
            style.box_values.height
        }
        WritingMode::VerticalRl | WritingMode::VerticalLr => style.box_values.width,
    };
    declared_table_width_from_computed(inline_size)
}

fn declared_table_width_from_computed(
    value: css::ComputedLengthPercentageOrAuto,
) -> Option<DeclaredTableWidth> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.math.is_some() {
                Some(DeclaredTableWidth::LengthPercentage(value))
            } else if value.percent != 0.0 && value.length_is_zero() {
                Some(DeclaredTableWidth::Percent(value.percent))
            } else if value.percent != 0.0 {
                Some(DeclaredTableWidth::LengthPercentage(value))
            } else {
                Some(DeclaredTableWidth::Fixed(value.length_points()))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
    }
}

pub(super) fn html_width_for_table_cell(cell: &Element) -> Option<DeclaredTableWidth> {
    let value = cell.attrs.get("width")?.trim();
    if let Some(percent) = value.strip_suffix('%') {
        return percent
            .trim()
            .parse::<f32>()
            .ok()
            .map(|percent| DeclaredTableWidth::Percent(percent / 100.0));
    }
    parse_html_length(value).map(DeclaredTableWidth::Fixed)
}

pub(super) fn resolve_declared_table_width(width: DeclaredTableWidth, table_width: f32) -> f32 {
    match width {
        DeclaredTableWidth::Fixed(width) => width,
        DeclaredTableWidth::Percent(percent) => table_width * percent,
        DeclaredTableWidth::LengthPercentage(value) => used_length_percentage(value, table_width),
    }
}

pub(super) fn constrain_declared_table_width(
    style: &ComputedStyle,
    width: DeclaredTableWidth,
    table_width: f32,
) -> f32 {
    constrain_width(
        style,
        resolve_declared_table_width(width, table_width),
        table_width,
    )
}

/// Resolve a declared table-cell width to its column-space border-box width.
///
/// CSS Tables uses cell border boxes as column constraints, while CSS Sizing
/// applies a table-cell `width` to the cell content box unless `box-sizing`
/// says otherwise. Collapsed-border cells contribute the resolved half-border
/// insets on their outside grid edges, not their authored full border widths:
/// <https://drafts.csswg.org/css-tables-3/#computing-column-measures>
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
pub(super) fn declared_table_cell_border_box_width(
    style: &ComputedStyle,
    width: DeclaredTableWidth,
    table_width: f32,
    border_insets: Option<css::Edges>,
) -> f32 {
    let non_content = table_cell_horizontal_non_content_width(style, border_insets);
    let specified = resolve_declared_table_width(width, table_width);
    table_cell_border_box_width_from_declared_size(style, specified, table_width, non_content)
        .points()
}

pub(super) fn declared_table_width_length_floor(width: DeclaredTableWidth) -> f32 {
    match width {
        DeclaredTableWidth::Fixed(width) => width,
        DeclaredTableWidth::Percent(_) => 0.0,
        DeclaredTableWidth::LengthPercentage(value) => value.length_points_max_zero(),
    }
}

pub(super) fn declared_table_cell_width_length_floor(
    style: &ComputedStyle,
    width: DeclaredTableWidth,
    border_insets: Option<css::Edges>,
) -> f32 {
    let non_content = table_cell_horizontal_non_content_width(style, border_insets);
    match width {
        DeclaredTableWidth::Fixed(width) => {
            table_cell_border_box_width_from_declared_size(style, width, 0.0, non_content).points()
        }
        DeclaredTableWidth::Percent(_) => 0.0,
        DeclaredTableWidth::LengthPercentage(value) => {
            table_cell_border_box_width_from_declared_size(
                style,
                value.length_points_max_zero(),
                0.0,
                non_content,
            )
            .points()
        }
    }
}

fn table_cell_horizontal_non_content_width(
    style: &ComputedStyle,
    border_insets: Option<css::Edges>,
) -> f32 {
    let border_width = border_insets
        .map(|borders| borders.left + borders.right)
        .unwrap_or_else(|| table_horizontal_borders(style));
    style.padding.left + style.padding.right + border_width
}

fn table_cell_border_box_width_from_declared_size(
    style: &ComputedStyle,
    specified: f32,
    percentage_basis: f32,
    non_content: f32,
) -> BorderBoxLength {
    let non_content = non_content_pt(non_content);
    let content_width = match style.box_sizing {
        BoxSizing::BorderBox => {
            border_box_to_content_box_length(border_box_pt(specified), non_content)
        }
        BoxSizing::ContentBox => content_box_pt(specified.max(0.0)),
    };
    content_box_to_border_box_length(
        content_box_pt(constrain_width(style, content_width.points(), percentage_basis).max(0.0)),
        non_content,
    )
}

pub(super) fn declared_table_width_percentage(width: DeclaredTableWidth) -> f32 {
    match width {
        DeclaredTableWidth::Fixed(_) => 0.0,
        DeclaredTableWidth::Percent(percent) => percent,
        DeclaredTableWidth::LengthPercentage(value) => value.percent,
    }
}

pub(super) fn declared_table_width_is_non_percentage(width: DeclaredTableWidth) -> bool {
    declared_table_width_percentage(width) == 0.0
}

/// Column width inputs collected before the final table width is known.
///
/// CSS Tables 3 separates column min-content widths, max-content widths,
/// intrinsic percentage contributions, and constrainedness before running the
/// width distribution algorithm:
/// <https://drafts.csswg.org/css-tables-3/#computing-column-measures> and
/// <https://drafts.csswg.org/css-tables-3/#width-distribution-algorithm>.
#[derive(Debug, Clone)]
pub(super) struct TableColumnMeasures {
    pub(super) min_content_widths: Vec<f32>,
    pub(super) max_content_widths: Vec<f32>,
    pub(super) intrinsic_percentages: Vec<f32>,
    pub(super) constrained: Vec<bool>,
    pub(super) occupied: Vec<bool>,
    pub(super) total_horizontal_spacing: f32,
}

impl TableColumnMeasures {
    pub(super) fn table_min_content_width(&self) -> f32 {
        self.total_horizontal_spacing + self.min_content_widths.iter().sum::<f32>()
    }

    pub(super) fn table_max_content_width(&self) -> f32 {
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

pub(super) fn intrinsic_percentage_contribution(style: &ComputedStyle) -> f32 {
    let min_width = length_percentage_percent(style.box_values.min_width).unwrap_or(0.0);
    let width = length_percentage_percent(style.box_values.width).unwrap_or(0.0);
    let max_width = length_percentage_percent(style.box_values.max_width).unwrap_or(f32::INFINITY);
    min_width.max(width.min(max_width)).max(0.0)
}

fn length_percentage_percent(value: css::ComputedLengthPercentageOrAuto) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.math.is_none() => {
            Some(value.percent)
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => None,
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
    }
}

pub(super) fn constrain_table_intrinsic_width_with_floor(
    style: &ComputedStyle,
    value: f32,
    floor: f32,
) -> f32 {
    let min_width = intrinsic_length_constraint(style.box_values.min_width);
    let max_width = intrinsic_length_constraint(style.box_values.max_width);
    constrain(value.max(floor), min_width, max_width)
}

fn intrinsic_length_constraint(value: css::ComputedLengthPercentageOrAuto) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.math.is_none() => {
            (!value.length_is_zero() || value.percent == 0.0)
                .then_some(value.length_points_max_zero())
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => None,
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
    }
}

fn constrain(value: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    let value = min.map(|min| value.max(min)).unwrap_or(value);
    max.map(|max| value.min(max)).unwrap_or(value)
}

/// Distribute extra assignable table width across a column range.
///
/// CSS Tables 3 defines ordered receiver groups for width distribution:
/// unconstrained non-percentage columns, unconstrained zero-base columns,
/// constrained non-percentage columns, percentage columns, occupied columns,
/// then all columns.
/// <https://drafts.csswg.org/css-tables-3/#distributing-width-to-columns>.
pub(super) fn distribute_table_excess_width(
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

pub(super) fn distribute_fixed_width(
    widths: &mut [f32],
    declared: &mut [bool],
    column: usize,
    colspan: usize,
    target_width: f32,
) {
    let end = (column + colspan.max(1)).min(widths.len());
    if column >= end {
        return;
    }
    let current = widths[column..end].iter().sum::<f32>();
    if target_width > current {
        let extra = (target_width - current) / (end - column) as f32;
        for width in &mut widths[column..end] {
            *width += extra;
        }
    }
    for is_declared in &mut declared[column..end] {
        *is_declared = true;
    }
}

pub(super) fn distribute_first_row_fixed_width(
    widths: &mut [f32],
    declared: &mut [bool],
    column: usize,
    colspan: usize,
    target_width: f32,
) {
    let end = (column + colspan.max(1)).min(widths.len());
    if column >= end {
        return;
    }
    let current = widths[column..end].iter().sum::<f32>();
    let receivers = (column..end)
        .filter(|index| !declared[*index])
        .collect::<Vec<_>>();
    if receivers.is_empty() {
        return;
    }
    if target_width > current {
        let extra = (target_width - current) / receivers.len() as f32;
        for index in &receivers {
            widths[*index] += extra;
        }
    }
    for index in receivers {
        declared[index] = true;
    }
}

pub(super) fn table_cell_content_min_width(
    layout: &mut LayoutBuilder<'_>,
    cell: &TableCell<'_>,
    style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    border_insets: Option<css::Edges>,
) -> f32 {
    let inline_contribution =
        table_cell_inline_intrinsic_contribution(layout, cell, style, stylesheets);
    let replaced_width = table_cell_replaced_content_max_width(cell);
    let (block_min_width, _) = table_cell_block_child_intrinsic_widths(layout, cell, stylesheets);
    let border_width = border_insets
        .map(|borders| borders.left + borders.right)
        .unwrap_or_else(|| table_horizontal_borders(style));

    inline_contribution
        .min_content
        .max(replaced_width)
        .max(block_min_width)
        + style.padding.left
        + style.padding.right
        + border_width
}

pub(super) fn table_cell_content_max_width(
    layout: &mut LayoutBuilder<'_>,
    cell: &TableCell<'_>,
    style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    border_insets: Option<css::Edges>,
) -> f32 {
    let inline_contribution =
        table_cell_inline_intrinsic_contribution(layout, cell, style, stylesheets);
    let replaced_width = table_cell_replaced_content_sum_width(cell);
    let (_, block_max_width) = table_cell_block_child_intrinsic_widths(layout, cell, stylesheets);
    let border_width = border_insets
        .map(|borders| borders.left + borders.right)
        .unwrap_or_else(|| table_horizontal_borders(style));

    inline_contribution
        .max_content
        .max(block_max_width)
        .max(replaced_width)
        + style.padding.left
        + style.padding.right
        + border_width
}

fn table_cell_inline_intrinsic_contribution(
    layout: &mut LayoutBuilder<'_>,
    cell: &TableCell<'_>,
    style: &ComputedStyle,
    stylesheets: &[Stylesheet],
) -> inline_layout::InlineIntrinsicContribution {
    let measurement = if let Some(children) = cell.children.as_deref() {
        layout.intrinsic_inline_measurement_for_boxes(children, style, stylesheets, f32::MAX)
    } else if let Some(element) = cell.element {
        layout.intrinsic_inline_measurement_for_element(element, style, stylesheets, None, f32::MAX)
    } else {
        return inline_layout::InlineIntrinsicContribution::default();
    };

    if style.writing_mode == WritingMode::HorizontalTb {
        return measurement.contribution;
    }

    let physical_width = measurement.physical_width(style);
    inline_layout::InlineIntrinsicContribution {
        min_content: physical_width,
        max_content: physical_width,
    }
}

/// Return min/max-content width contributions from block-level and floated cell children.
///
/// CSS Tables 3 computes cell min-content and max-content measures from the
/// contents of the table cell, including nested block formatting contexts.
/// CSS 2.2 blockifies floated boxes before layout, so direct floated inline
/// children contribute through their own shrink-to-fit/explicit inline size
/// rather than through an empty child list:
/// <https://drafts.csswg.org/css-tables-3/#computing-cell-measures> and
/// <https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo>.
fn table_cell_block_child_intrinsic_widths(
    layout: &mut LayoutBuilder<'_>,
    cell: &TableCell<'_>,
    stylesheets: &[Stylesheet],
) -> (f32, f32) {
    let Some(children) = cell.children.as_deref() else {
        return (0.0, 0.0);
    };

    children
        .iter()
        .fold((0.0_f32, 0.0_f32), |(min, max), child| {
            let (child_min, child_max) =
                table_cell_formatting_child_intrinsic_widths(layout, child, stylesheets);
            (min.max(child_min), max.max(child_max))
        })
}

fn table_cell_formatting_child_intrinsic_widths(
    layout: &mut LayoutBuilder<'_>,
    child: &box_tree::FormattingBox<'_>,
    stylesheets: &[Stylesheet],
) -> (f32, f32) {
    match child {
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            table_cell_formatting_children_intrinsic_widths(layout, &box_.children, stylesheets)
        }
        box_tree::FormattingBox::Inline(box_) if table_cell_child_is_in_flow_float(child) => {
            table_cell_formatting_box_intrinsic_width(
                layout,
                child,
                &box_.style,
                &box_.children,
                stylesheets,
            )
        }
        box_tree::FormattingBox::Inline(box_) => {
            table_cell_formatting_children_intrinsic_widths(layout, &box_.children, stylesheets)
        }
        _ => {
            let Some((_, _, style, child_children)) = child.element_parts() else {
                return (0.0, 0.0);
            };
            if !table_cell_block_child_contributes_to_intrinsic_width(child, style) {
                return (0.0, 0.0);
            }
            table_cell_formatting_box_intrinsic_width(
                layout,
                child,
                style,
                child_children,
                stylesheets,
            )
        }
    }
}

fn table_cell_formatting_children_intrinsic_widths(
    layout: &mut LayoutBuilder<'_>,
    children: &[box_tree::FormattingBox<'_>],
    stylesheets: &[Stylesheet],
) -> (f32, f32) {
    children
        .iter()
        .fold((0.0_f32, 0.0_f32), |(min, max), child| {
            let (child_min, child_max) =
                table_cell_formatting_child_intrinsic_widths(layout, child, stylesheets);
            (min.max(child_min), max.max(child_max))
        })
}

fn table_cell_block_child_contributes_to_intrinsic_width(
    child: &box_tree::FormattingBox<'_>,
    style: &ComputedStyle,
) -> bool {
    !matches!(style.position, Position::Absolute | Position::Fixed)
        && (style.float != Float::None
            || matches!(
                child,
                box_tree::FormattingBox::Block(_)
                    | box_tree::FormattingBox::Table(_)
                    | box_tree::FormattingBox::Flex(_)
            ))
}

/// Resolve a block-level child box's intrinsic outer inline sizes.
///
/// CSS Sizing defines min-content/max-content contributions, and CSS Tables
/// uses those contributions for auto table layout cell measures:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution> and
/// <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>.
fn table_cell_formatting_box_intrinsic_width(
    layout: &mut LayoutBuilder<'_>,
    child: &box_tree::FormattingBox<'_>,
    style: &ComputedStyle,
    children: &[box_tree::FormattingBox<'_>],
    stylesheets: &[Stylesheet],
) -> (f32, f32) {
    if let box_tree::FormattingBox::Table(box_) = child {
        return layout.table_outer_intrinsic_widths_from_fragment(
            box_.element,
            style,
            stylesheets,
            &box_.fragment,
            10_000.0,
        );
    }

    let used_edges = used_box_edges(style, 0.0);
    let used_padding = used_edges.padding.to_css_edges();
    let used_margin = used_edges.margin.to_css_edges();
    let horizontal_non_content =
        used_padding.left + used_padding.right + horizontal_border_width(style);
    let explicit_width = used_content_width_or_auto(style, 0.0, horizontal_non_content);
    let inline_contribution = layout.intrinsic_inline_contribution_for_boxes(children, style, &[]);
    let (block_min_width, block_max_width) =
        if table_cell_style_has_parent_percentage_block_size(style) {
            table_cell_formatting_children_intrinsic_widths(layout, children, stylesheets)
        } else {
            (0.0, 0.0)
        };
    let intrinsic_min = inline_contribution
        .min_content
        .min(inline_contribution.max_content)
        .max(block_min_width)
        .max(0.0);
    let intrinsic_max = inline_contribution.max_content.max(block_max_width);
    let preferred_min = explicit_width.unwrap_or(intrinsic_min);
    let preferred = explicit_width.unwrap_or(intrinsic_max.max(preferred_min));
    let min = constrain_width(style, preferred_min, 0.0);
    let max = constrain_width(style, preferred.max(min), 0.0);
    (
        min + horizontal_non_content + used_margin.left + used_margin.right,
        max + horizontal_non_content + used_margin.left + used_margin.right,
    )
}

fn table_cell_replaced_content_max_width(cell: &TableCell<'_>) -> f32 {
    table_cell_replaced_content_widths(cell)
        .into_iter()
        .fold(0.0_f32, f32::max)
}

fn table_cell_replaced_content_sum_width(cell: &TableCell<'_>) -> f32 {
    table_cell_replaced_content_widths(cell).into_iter().sum()
}

/// Return replaced descendant widths used by table intrinsic sizing.
///
/// CSS 2.2 automatic table layout computes min-content and max-content column
/// constraints from cell contents, including replaced inline content:
/// <https://www.w3.org/TR/CSS22/tables.html#auto-table-layout>.
fn table_cell_replaced_content_widths(cell: &TableCell<'_>) -> Vec<f32> {
    if let Some(children) = cell.children.as_deref() {
        return children
            .iter()
            .filter_map(replaced_box_svg_width)
            .collect::<Vec<_>>();
    }

    cell.element
        .into_iter()
        .flat_map(|element| element.children.iter())
        .filter_map(|child| match &child.kind {
            NodeKind::Element(element)
                if replaced_element_kind(element) == Some(ReplacedElementKind::Svg) =>
            {
                svg_rect(element).map(|(width, _, _)| width)
            }
            _ => None,
        })
        .collect()
}

fn replaced_box_svg_width(box_: &box_tree::FormattingBox<'_>) -> Option<f32> {
    match box_ {
        box_tree::FormattingBox::Replaced(box_) => (replaced_element_kind(box_.element)
            == Some(ReplacedElementKind::Svg))
        .then(|| svg_rect(box_.element).map(|(width, _, _)| width))
        .flatten(),
        box_tree::FormattingBox::AtomicInline(box_) => (replaced_element_kind(box_.element)
            == Some(ReplacedElementKind::Svg))
        .then(|| svg_rect(box_.element).map(|(width, _, _)| width))
        .flatten(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn horizontal_edges(left: f32, right: f32) -> css::Edges {
        css::Edges {
            top: 0.0,
            right,
            bottom: 0.0,
            left,
        }
    }

    fn style_with_width(width: css::ComputedLengthPercentageOrAuto) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.box_values.width = width;
        style
    }

    #[test]
    fn table_width_content_box_expands_to_wrapper_border_box() {
        let mut style = style_with_width(length(150.0));
        style.border_collapse = css::BorderCollapse::Separate;
        style.box_values.padding.left = css::ComputedLengthPercentage::from_points(10.0);
        style.box_values.padding.right = css::ComputedLengthPercentage::from_points(10.0);
        style.padding.left = 10.0;
        style.padding.right = 10.0;

        let width = used_table_width(&style, 300.0);

        assert_eq!(width.content_width.points(), 150.0);
        assert_eq!(
            width.wrapper_border_box_width(width.content_width).points(),
            170.0
        );
    }

    #[test]
    fn empty_table_border_box_width_clamps_content_box_at_zero() {
        let mut style = style_with_width(length(100.0));
        style.box_sizing = BoxSizing::BorderBox;
        let table_width = UsedTableWidth {
            content_width: content_box_pt(0.0),
            border_widths: css::Edges::ZERO,
            padding: horizontal_edges(75.0, 75.0),
        };

        let content = used_empty_table_grid_width(&style, 300.0, table_width);

        assert_eq!(content.points(), 0.0);
    }

    #[test]
    fn declared_table_cell_border_box_width_counts_extras_once() {
        let mut content_box_style = ComputedStyle::initial();
        content_box_style.box_sizing = BoxSizing::ContentBox;
        content_box_style.padding = horizontal_edges(10.0, 10.0);
        let border_insets = Some(horizontal_edges(5.0, 5.0));

        let content_box_width = table_cell_border_box_width_from_declared_size(
            &content_box_style,
            100.0,
            300.0,
            table_cell_horizontal_non_content_width(&content_box_style, border_insets),
        );

        let mut border_box_style = content_box_style;
        border_box_style.box_sizing = BoxSizing::BorderBox;
        let border_box_width = table_cell_border_box_width_from_declared_size(
            &border_box_style,
            100.0,
            300.0,
            table_cell_horizontal_non_content_width(&border_box_style, border_insets),
        );

        assert_eq!(content_box_width.points(), 130.0);
        assert_eq!(border_box_width.points(), 100.0);
    }

    #[test]
    fn empty_table_auto_grid_width_is_zero_content_box() {
        let style = style_with_width(css::ComputedLengthPercentageOrAuto::Auto);
        let table_width = UsedTableWidth {
            content_width: content_box_pt(0.0),
            border_widths: css::Edges::ZERO,
            padding: css::Edges::ZERO,
        };

        let content = used_empty_table_grid_width(&style, 300.0, table_width);

        assert_eq!(content.points(), 0.0);
    }
}
