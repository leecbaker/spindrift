use super::*;
use crate::layout::table::layout::{
    table_cell_child_is_in_flow_float, table_cell_style_has_parent_percentage_block_size,
};
use crate::units::IntoLayoutLength;

#[derive(Debug, Clone, Copy)]
pub(super) struct UsedTableWidth {
    pub(super) content_width: ContentBoxLength,
    pub(super) border_widths: css::Edges,
    pub(super) padding: css::Edges,
}

impl UsedTableWidth {
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

/// Return the table root's authored logical inline-size property.
///
/// CSS `width` and `height` remain physical properties.  CSS Tables computes
/// its column grid on the root table's logical inline axis, which is physical
/// height in vertical writing modes:
/// <https://drafts.csswg.org/css-writing-modes-4/#dimension-mapping> and
/// <https://drafts.csswg.org/css-tables-3/#table-layout>.
pub(super) fn table_root_inline_size(style: &ComputedStyle) -> css::ComputedLengthPercentageOrAuto {
    if style.writing_mode.has_vertical_lines() {
        style.box_values.height.clone()
    } else {
        style.box_values.width.clone()
    }
}

/// Return whether the table wrapper supplies a definite lower bound for the
/// grid's logical inline size.
///
/// An `auto` table width ordinarily uses its intrinsic maximum rather than
/// distributing unused available width to tracks. A non-zero logical
/// `min-inline-size` is different: after CSS Sizing resolves that wrapper
/// constraint in content-box space, CSS Tables must preserve it while it
/// distributes the column grid.  This is `min-height` in vertical writing
/// modes and `min-width` otherwise:
/// <https://drafts.csswg.org/css-sizing-3/#min-size-properties> and
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>.
pub(super) fn table_root_distributes_extra_inline_space(style: &ComputedStyle) -> bool {
    if !table_root_inline_size(style).is_auto() {
        return true;
    }
    let min_inline_size = table_root_min_inline_size(style);
    match min_inline_size {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            !value.length_is_zero() || value.percentage_coefficient_or_zero() != 0.0
        }
        css::ComputedLengthPercentageOrAuto::Auto => false,
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => true,
    }
}

/// Return the wrapper min-size property that constrains the table grid's
/// logical inline axis.
pub(super) fn table_root_min_inline_size(
    style: &ComputedStyle,
) -> css::ComputedLengthPercentageOrAuto {
    if style.writing_mode.has_vertical_lines() {
        style.box_values.min_height.clone()
    } else {
        style.box_values.min_width.clone()
    }
}

/// Return the physical CSS property that controls a table root's logical block
/// axis. CSS Tables distributes row tracks on that axis, which is physical
/// width in vertical writing modes.
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping> and
/// <https://drafts.csswg.org/css-tables-3/#row-layout>
pub(super) fn table_root_block_size(style: &ComputedStyle) -> css::ComputedLengthPercentageOrAuto {
    if style.writing_mode.has_vertical_lines() {
        style.box_values.width.clone()
    } else {
        style.box_values.height.clone()
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
        used_padding_edges(
            style,
            PercentageBasis::definite(layout_pt(available_outer_width)),
        )
        .to_css_edges()
    };
    let width = UsedTableWidth {
        content_width: content_box_pt(0.0),
        border_widths,
        padding,
    };
    let horizontal_non_content = width.horizontal_non_content(style);
    let requested_content_width = used_content_box_size(
        table_root_inline_size(style),
        style.box_sizing,
        PercentageBasis::definite(content_box_pt(available_outer_width)),
        horizontal_non_content,
    )
    .unwrap_or_else(|| {
        content_box_pt((available_outer_width - horizontal_non_content.points()).max(0.0))
    });
    let content_width = content_box_pt(
        constrain_content_width(
            style,
            requested_content_width,
            PercentageBasis::definite(layout_pt(available_outer_width)),
        )
        .points()
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
        table_root_inline_size(style),
        style.box_sizing,
        PercentageBasis::definite(content_box_pt(available_outer_width)),
        horizontal_non_content,
    )
    .unwrap_or_else(|| content_box_pt(0.0));
    content_box_pt(
        constrain_content_width(
            style,
            requested_content_width,
            PercentageBasis::definite(layout_pt(available_outer_width)),
        )
        .points()
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
    percentage_height_basis: BlockSizePercentageBasis,
    table_width: UsedTableWidth,
    wrapper_border_box_block_size: Option<BorderBoxLength>,
    wrapper_non_grid_block_size: LayoutLength,
) -> ContentBoxLength {
    let vertical_non_content = non_content_pt(
        table_width.border_widths.top
            + table_width.border_widths.bottom
            + table_width.padding.top
            + table_width.padding.bottom,
    );
    if let Some(wrapper_border_box_block_size) = wrapper_border_box_block_size {
        return content_box_pt(
            (wrapper_border_box_block_size.points()
                - vertical_non_content.points()
                - wrapper_non_grid_block_size.points())
            .max(0.0),
        );
    }
    used_table_target_content_height(style, percentage_height_basis, vertical_non_content)
        .unwrap_or_else(|| content_box_pt(0.0))
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
    percentage_height_basis: BlockSizePercentageBasis,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    let height = table_wrapper_size_to_grid_content_height(
        table_root_block_size(style),
        style.box_sizing,
        percentage_height_basis,
        vertical_non_content,
    );
    let min_height = table_wrapper_size_to_grid_content_height(
        if style.writing_mode.has_vertical_lines() {
            style.box_values.min_width.clone()
        } else {
            style.box_values.min_height.clone()
        },
        style.box_sizing,
        percentage_height_basis,
        vertical_non_content,
    );
    let max_height = table_wrapper_size_to_grid_content_height(
        if style.writing_mode.has_vertical_lines() {
            style.box_values.max_width.clone()
        } else {
            style.box_values.max_height.clone()
        },
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
    Some(target.max(content_box_pt(0.0)))
}

fn table_wrapper_size_to_grid_content_height(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: BlockSizePercentageBasis,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    let specified = used_length_percentage_or_auto_with_basis(value, percentage_basis)?;
    Some(match box_sizing {
        BoxSizing::BorderBox => border_box_to_content_box_length(
            crate::units::layout_to_border_box_length(specified),
            vertical_non_content,
        ),
        BoxSizing::ContentBox => crate::units::layout_to_content_box_length(specified),
    })
}

pub(super) fn declared_table_cell_width(
    _cell: &Element,
    style: &ComputedStyle,
) -> Option<DeclaredTableWidth> {
    match style.box_values.width.clone() {
        // Legacy HTML `width` is converted into a presentational hint during
        // cascade.  Reading it again here would turn `width=0` back into a
        // fixed zero-width column after the hint correctly computed to auto.
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if let Some(percent) = value
                .pure_percentage_coefficient()
                .filter(|percent| *percent != 0.0)
            {
                Some(DeclaredTableWidth::Percent(percent))
            } else if value.needs_percentage_basis() {
                Some(DeclaredTableWidth::LengthPercentage(value))
            } else {
                Some(DeclaredTableWidth::Fixed(value.length_points()))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
    }
}

pub(super) fn declared_table_column_width(style: &ComputedStyle) -> Option<DeclaredTableWidth> {
    let inline_size = match style.writing_mode {
        WritingMode::HorizontalTb => style.box_values.width.clone(),
        WritingMode::VerticalRl | WritingMode::VerticalLr
            if style.text_orientation != css::TextOrientation::Sideways =>
        {
            style.box_values.height.clone()
        }
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => style.box_values.width.clone(),
    };
    declared_table_width_from_computed(inline_size)
}

fn declared_table_width_from_computed(
    value: css::ComputedLengthPercentageOrAuto,
) -> Option<DeclaredTableWidth> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if let Some(percent) = value
                .pure_percentage_coefficient()
                .filter(|percent| *percent != 0.0)
            {
                Some(DeclaredTableWidth::Percent(percent))
            } else if value.needs_percentage_basis() {
                Some(DeclaredTableWidth::LengthPercentage(value))
            } else {
                Some(DeclaredTableWidth::Fixed(value.length_points()))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
    }
}

pub(super) fn resolve_declared_table_width(
    width: DeclaredTableWidth,
    table_width: LayoutLength,
) -> LayoutLength {
    match width {
        DeclaredTableWidth::Fixed(width) => layout_pt(width),
        DeclaredTableWidth::Percent(percent) => layout_pt(table_width.points() * percent),
        DeclaredTableWidth::LengthPercentage(value) => {
            used_length_percentage(value, PercentageBasis::definite(table_width))
        }
    }
}

pub(super) fn constrain_declared_table_width(
    style: &ComputedStyle,
    width: DeclaredTableWidth,
    table_width: ContentBoxLength,
) -> ContentBoxLength {
    constrain_content_width(
        style,
        crate::units::layout_to_content_box_length(resolve_declared_table_width(
            width,
            table_width.into_layout_length(),
        )),
        PercentageBasis::definite(table_width.into_layout_length()),
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
) -> BorderBoxLength {
    let non_content = table_cell_horizontal_non_content_width(style, border_insets);
    let specified = resolve_declared_table_width(width, layout_pt(table_width));
    table_cell_border_box_width_from_declared_size(
        style,
        specified,
        layout_pt(table_width),
        non_content,
    )
}

/// Return the fixed component of a declared table width for intrinsic sizing.
pub(super) fn declared_table_width_length_floor(width: DeclaredTableWidth) -> LayoutLength {
    match width {
        DeclaredTableWidth::Fixed(width) => layout_pt(width),
        DeclaredTableWidth::Percent(_) => layout_pt(0.0),
        DeclaredTableWidth::LengthPercentage(value) => value.length_max_zero(),
    }
}

pub(super) fn declared_table_cell_width_length_floor(
    style: &ComputedStyle,
    width: DeclaredTableWidth,
    border_insets: Option<css::Edges>,
) -> BorderBoxLength {
    let non_content = table_cell_horizontal_non_content_width(style, border_insets);
    match width {
        DeclaredTableWidth::Fixed(width) => table_cell_border_box_width_from_declared_size(
            style,
            layout_pt(width),
            layout_pt(0.0),
            non_content,
        ),
        DeclaredTableWidth::Percent(_) => border_box_pt(0.0),
        DeclaredTableWidth::LengthPercentage(value) => {
            table_cell_border_box_width_from_declared_size(
                style,
                value.length_max_zero(),
                layout_pt(0.0),
                non_content,
            )
        }
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

pub(super) fn declared_table_width_percentage(width: DeclaredTableWidth) -> f32 {
    match width {
        DeclaredTableWidth::Fixed(_) => 0.0,
        DeclaredTableWidth::Percent(percent) => percent,
        DeclaredTableWidth::LengthPercentage(value) => value.percentage_coefficient_or_zero(),
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
    let min_width = length_percentage_percent(style.box_values.min_width.clone()).unwrap_or(0.0);
    let width = length_percentage_percent(style.box_values.width.clone()).unwrap_or(0.0);
    let max_width =
        length_percentage_percent(style.box_values.max_width.clone()).unwrap_or(f32::INFINITY);
    min_width.max(width.min(max_width)).max(0.0)
}

fn length_percentage_percent(value: css::ComputedLengthPercentageOrAuto) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
            if value.percentage_coefficient().is_some() =>
        {
            value.percentage_coefficient()
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

pub(super) fn constrain_table_intrinsic_width_with_floor(
    style: &ComputedStyle,
    value: f32,
    floor: f32,
) -> f32 {
    let min_width = intrinsic_length_constraint(style.box_values.min_width.clone());
    let max_width = intrinsic_length_constraint(style.box_values.max_width.clone());
    constrain(value.max(floor), min_width, max_width)
}

fn intrinsic_length_constraint(value: css::ComputedLengthPercentageOrAuto) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
            if value.percentage_coefficient().is_some() =>
        {
            (!value.length_is_zero() || value.percentage_coefficient_or_zero() == 0.0)
                .then_some(value.length_max_zero().points())
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
    let replaced_width = table_cell_replaced_content_max_width(cell, style);
    let (block_min_width, _) = table_cell_block_child_intrinsic_widths(layout, cell, stylesheets);
    let border_width = border_insets
        .map(|borders| borders.left + borders.right)
        .unwrap_or_else(|| table_horizontal_borders(style).points());

    let padding = intrinsic_padding_edges(style).to_css_edges();

    inline_contribution
        .min_content
        .points()
        .max(replaced_width)
        .max(block_min_width)
        + padding.left
        + padding.right
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
    let replaced_width = table_cell_replaced_content_sum_width(cell, style);
    let (_, block_max_width) = table_cell_block_child_intrinsic_widths(layout, cell, stylesheets);
    let border_width = border_insets
        .map(|borders| borders.left + borders.right)
        .unwrap_or_else(|| table_horizontal_borders(style).points());

    let padding = intrinsic_padding_edges(style).to_css_edges();

    inline_contribution
        .max_content
        .points()
        .max(block_max_width)
        .max(replaced_width)
        + padding.left
        + padding.right
        + border_width
}

/// Return a cell's minimum outer contribution on the physical horizontal axis.
///
/// A vertical or sideways table root uses physical width for its row tracks.
/// That is not the cell's max-content width alone: an authored `width`,
/// `min-width`, or logical `inline-size` resolved to `width` must also keep
/// the root block track wide enough for the cell border box.  This is kept
/// separate from column measure collection, where a preferred cell width is
/// intentionally not a min-content floor:
/// <https://drafts.csswg.org/css-tables-3/#row-layout> and
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-contribution>.
pub(super) fn table_cell_physical_width_minimum(
    style: &ComputedStyle,
    border_insets: Option<css::Edges>,
) -> f32 {
    let border_width = border_insets
        .map(|borders| borders.left + borders.right)
        .unwrap_or_else(|| table_horizontal_borders(style).points());
    let padding = intrinsic_padding_edges(style).to_css_edges();
    let non_content = non_content_pt(padding.left + padding.right + border_width);
    let specified = used_content_box_width_or_auto(style, layout_pt(0.0), non_content)
        .map(SemanticLengthExt::points)
        .unwrap_or(0.0);
    let minimum = used_length_percentage_or_auto(
        style.box_values.min_width.clone(),
        PercentageBasis::definite(layout_pt(0.0)),
    )
    .map(SemanticLengthExt::points)
    .unwrap_or(0.0);
    (specified.max(minimum) + non_content.points()).max(0.0)
}

/// Return a cell's intrinsic contribution to the root table's block track.
///
/// The table root chooses the physical track axis.  For horizontal roots the
/// existing row-layout metric is already a physical-height border box.  For
/// vertical and sideways roots the track is physical width, where the cell's
/// intrinsic content and explicit physical width both participate:
/// <https://drafts.csswg.org/css-writing-modes-4/#dimension-mapping> and
/// <https://drafts.csswg.org/css-tables-3/#row-layout>.
pub(super) fn table_cell_root_block_track_contribution(
    layout: &mut LayoutBuilder<'_>,
    cell: &TableCell<'_>,
    cell_style: &ComputedStyle,
    table_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    border_insets: Option<css::Edges>,
    physical_height_border_box: f32,
) -> f32 {
    let axes = TableCellAxisAdapter::for_table(table_style);
    if axes.root_track_uses_physical_width(TableRootTrackAxis::Block) {
        table_cell_content_max_width(layout, cell, cell_style, stylesheets, border_insets)
            .max(table_cell_physical_width_minimum(cell_style, border_insets))
    } else {
        physical_height_border_box
    }
}

/// Return a cell's intrinsic contribution along the table root inline axis.
///
/// CSS Tables assigns columns on the table root's inline axis, while a cell's
/// own writing mode continues to govern its contents. When those axes are
/// orthogonal, a physical cell width is a table block-axis contribution and
/// must not widen a column; the cell's physical height contributes instead.
/// <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
/// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
pub(super) fn table_cell_content_table_inline_size(
    layout: &mut LayoutBuilder<'_>,
    cell: &TableCell<'_>,
    cell_style: &ComputedStyle,
    table_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    border_insets: Option<css::Edges>,
) -> inline_layout::InlineIntrinsicContribution {
    let axes = TableCellAxisAdapter::for_table(table_style);
    if axes.root_track_uses_physical_width(TableRootTrackAxis::Inline) {
        return inline_layout::InlineIntrinsicContribution::new(
            LogicalInlineContentSize::new(content_box_pt(table_cell_content_min_width(
                layout,
                cell,
                cell_style,
                stylesheets,
                border_insets,
            ))),
            LogicalInlineContentSize::new(content_box_pt(table_cell_content_max_width(
                layout,
                cell,
                cell_style,
                stylesheets,
                border_insets,
            ))),
        );
    }

    let physical_height = if let Some(children) = cell.children.as_deref() {
        // The table root's vertical inline track is a physical height. A
        // block child with an explicit physical `height` therefore
        // contributes through ordinary block layout, not its own inline
        // intrinsic width.
        // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        layout.table_cell_children_non_text_content_height(children, stylesheets, f32::MAX)
    } else if let Some(element) = cell.element {
        layout
            .intrinsic_inline_measurement_for_element(
                element,
                cell_style,
                stylesheets,
                None,
                f32::MAX,
            )
            .physical_height(cell_style)
    } else {
        0.0
    };
    let borders = border_insets
        .map(|borders| borders.top + borders.bottom)
        .unwrap_or_else(|| table_vertical_borders(cell_style).points());
    let vertical_non_content = cell_style.padding.top + cell_style.padding.bottom + borders;
    let declared_physical_height = used_content_box_height_or_auto(
        cell_style,
        layout_pt(0.0),
        non_content_pt(vertical_non_content),
    )
    .map(|height| height.points() + vertical_non_content)
    .unwrap_or(0.0);
    let inline_size = (physical_height + vertical_non_content).max(declared_physical_height);
    inline_layout::InlineIntrinsicContribution::new(
        LogicalInlineContentSize::new(content_box_pt(inline_size)),
        LogicalInlineContentSize::new(content_box_pt(inline_size)),
    )
}

fn table_cell_inline_intrinsic_contribution(
    layout: &mut LayoutBuilder<'_>,
    cell: &TableCell<'_>,
    style: &ComputedStyle,
    stylesheets: &[Stylesheet],
) -> inline_layout::InlineIntrinsicContribution {
    let available_inline_size = table_cell_inline_intrinsic_measure(style)
        .map(LogicalInlineContentSize::points)
        .unwrap_or(f32::MAX);
    let measurement = if let Some(children) = cell.children.as_deref() {
        layout.intrinsic_inline_measurement_for_boxes(
            children,
            style,
            stylesheets,
            available_inline_size,
        )
    } else if let Some(element) = cell.element {
        layout.intrinsic_inline_measurement_for_element(
            element,
            style,
            stylesheets,
            None,
            available_inline_size,
        )
    } else {
        return inline_layout::InlineIntrinsicContribution::default();
    };

    if !WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes() {
        return measurement.contribution;
    }

    let physical_width = measurement.physical_width(style);
    inline_layout::InlineIntrinsicContribution::new(
        LogicalInlineContentSize::new(content_box_pt(physical_width)),
        LogicalInlineContentSize::new(content_box_pt(physical_width)),
    )
}

/// Return the definite intrinsic measurement span on a cell's own inline
/// axis, if the authored cell establishes one.
///
/// The intrinsic-measurement backend accepts a scalar at this boundary, but
/// it still represents the cell's logical inline content size.  In a vertical
/// cell that axis is physical height, so `height` and `max-height` constrain
/// wrapping before the resulting physical width is offered to a horizontal
/// table column.  This is not a table-track constraint and is intentionally
/// independent of the root table writing mode:
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows> and
/// <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>.
pub(super) fn table_cell_inline_intrinsic_measure(
    style: &ComputedStyle,
) -> Option<LogicalInlineContentSize> {
    if !WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes() {
        return None;
    }

    let non_content =
        non_content_pt(style.padding.top + style.padding.bottom) + table_vertical_borders(style);
    let specified = used_content_box_height_or_auto(style, layout_pt(0.0), non_content)
        .map(SemanticLengthExt::points);
    let maximum = used_length_percentage_or_auto(
        style.box_values.max_height.clone(),
        PercentageBasis::<LayoutLength>::indefinite(),
    )
    .map(SemanticLengthExt::points);
    match (specified, maximum) {
        (Some(specified), Some(maximum)) => Some(specified.min(maximum)),
        (Some(specified), None) => Some(specified),
        (None, Some(maximum)) => Some(maximum),
        (None, None) => None,
    }
    .map(|value| LogicalInlineContentSize::new(content_box_pt(value.max(1.0))))
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
                &box_.core.style,
                &box_.core.children,
                stylesheets,
            )
        }
        box_tree::FormattingBox::Inline(box_) => table_cell_formatting_children_intrinsic_widths(
            layout,
            &box_.core.children,
            stylesheets,
        ),
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
    // Durable table fragments intentionally retain source styles. Intrinsic
    // column measurement is a layout consumer, so normalize each child here
    // before its fixed box geometry contributes to an auto table track.
    // <https://drafts.csswg.org/css-viewport/#zoom-property>
    // <https://drafts.csswg.org/css-tables-3/#computing-column-measures>
    let style = layout.style_with_current_viewport_lengths(style);
    if let box_tree::FormattingBox::Table(box_) = child {
        return layout.table_outer_intrinsic_widths_from_fragment(
            box_.core.element,
            &style,
            stylesheets,
            &box_.fragment,
            10_000.0,
        );
    }

    let used_edges = used_box_edges(&style, PercentageBasis::definite(layout_pt(0.0)));
    let used_padding = used_edges.padding.to_css_edges();
    let used_margin = used_edges.margin.to_css_edges();
    let horizontal_non_content =
        used_padding.left + used_padding.right + horizontal_border_width(&style);
    let explicit_width = used_content_box_width_or_auto(
        &style,
        layout_pt(0.0),
        non_content_pt(horizontal_non_content),
    )
    .map(SemanticLengthExt::points);
    let inline_contribution = if intrinsic_inline_size_is_contained(&style) {
        // Size containment replaces every descendant intrinsic contribution
        // with the size of empty content; the box's own explicit size and
        // non-content edges still contribute to the cell measure.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        inline_layout::InlineIntrinsicContribution::default()
    } else {
        layout.intrinsic_inline_contribution_for_boxes(children, &style, &[])
    };
    let (block_min_width, block_max_width) = if !intrinsic_inline_size_is_contained(&style)
        && table_cell_style_has_parent_percentage_block_size(&style)
    {
        table_cell_formatting_children_intrinsic_widths(layout, children, stylesheets)
    } else {
        (0.0, 0.0)
    };
    let intrinsic_min = inline_contribution
        .min_content
        .points()
        .min(inline_contribution.max_content.points())
        .max(block_min_width)
        .max(0.0);
    let intrinsic_max = inline_contribution
        .max_content
        .points()
        .max(block_max_width);
    let preferred_min = explicit_width.unwrap_or(intrinsic_min);
    let preferred = explicit_width.unwrap_or(intrinsic_max.max(preferred_min));
    let min = constrain_content_width(
        &style,
        content_box_pt(preferred_min),
        PercentageBasis::definite(layout_pt(0.0)),
    )
    .points();
    let max = constrain_content_width(
        &style,
        content_box_pt(preferred.max(min)),
        PercentageBasis::definite(layout_pt(0.0)),
    )
    .points();
    (
        min + horizontal_non_content + used_margin.left + used_margin.right,
        max + horizontal_non_content + used_margin.left + used_margin.right,
    )
}

fn table_cell_replaced_content_max_width(cell: &TableCell<'_>, cell_style: &ComputedStyle) -> f32 {
    table_cell_replaced_content_widths(cell, cell_style)
        .into_iter()
        .fold(0.0_f32, f32::max)
}

fn table_cell_replaced_content_sum_width(cell: &TableCell<'_>, cell_style: &ComputedStyle) -> f32 {
    table_cell_replaced_content_widths(cell, cell_style)
        .into_iter()
        .sum()
}

/// Return replaced descendant widths used by table intrinsic sizing.
///
/// CSS 2.2 automatic table layout computes min-content and max-content column
/// constraints from cell contents, including replaced inline content:
/// <https://www.w3.org/TR/CSS22/tables.html#auto-table-layout>.
fn table_cell_replaced_content_widths(
    cell: &TableCell<'_>,
    cell_style: &ComputedStyle,
) -> Vec<f32> {
    if let Some(children) = cell.children.as_deref() {
        return children
            .iter()
            .flat_map(|child| replaced_box_intrinsic_widths(child, cell_style))
            .collect::<Vec<_>>();
    }

    cell.element
        .into_iter()
        .flat_map(replaced_descendant_intrinsic_widths)
        .collect()
}

fn replaced_box_intrinsic_widths(
    box_: &box_tree::FormattingBox<'_>,
    cell_style: &ComputedStyle,
) -> Vec<f32> {
    match box_ {
        box_tree::FormattingBox::Replaced(box_) => replaced_intrinsic_width_with_table_cell_height(
            box_.core.element,
            &box_.core.style,
            cell_style,
        )
        .into_iter()
        .collect(),
        box_tree::FormattingBox::AtomicInline(box_) => {
            replaced_intrinsic_width_with_table_cell_height(
                box_.core.element,
                &box_.core.style,
                cell_style,
            )
            .into_iter()
            .collect()
        }
        box_tree::FormattingBox::Block(box_) => box_
            .core
            .children
            .iter()
            .flat_map(|child| replaced_box_intrinsic_widths(child, cell_style))
            .collect(),
        box_tree::FormattingBox::Inline(box_) => box_
            .core
            .children
            .iter()
            .flat_map(|child| replaced_box_intrinsic_widths(child, cell_style))
            .collect(),
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .flat_map(|child| replaced_box_intrinsic_widths(child, cell_style))
            .collect(),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .core
            .children
            .iter()
            .flat_map(|child| replaced_box_intrinsic_widths(child, cell_style))
            .collect(),
        box_tree::FormattingBox::Table(box_) => box_
            .core
            .children
            .iter()
            .flat_map(|child| replaced_box_intrinsic_widths(child, cell_style))
            .collect(),
        box_tree::FormattingBox::Flex(box_) => box_
            .core
            .children
            .iter()
            .flat_map(|child| replaced_box_intrinsic_widths(child, cell_style))
            .collect(),
        box_tree::FormattingBox::Text(_) => Vec::new(),
    }
}

/// Resolve the intrinsic inline contribution of a replaced table-cell child
/// whose automatic width follows a percentage-resolved height.
///
/// CSS Tables measures cell content for column sizing, while CSS Sizing keeps
/// an auto replaced axis coupled to its intrinsic aspect ratio. If the cell
/// itself has a definite height, a percentage child height is already definite
/// during this measure and must not collapse back to its HTML width attribute.
/// <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
/// <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
fn replaced_intrinsic_width_with_table_cell_height(
    element: &Element,
    style: &ComputedStyle,
    cell_style: &ComputedStyle,
) -> Option<f32> {
    let intrinsic_size = match replaced_element_kind(element) {
        Some(ReplacedElementKind::Svg) => intrinsic_svg_size(element),
        Some(ReplacedElementKind::Canvas) => Some(intrinsic_canvas_size(element)),
        Some(ReplacedElementKind::Image) | None => None,
    }?;
    if !style.box_values.width.clone().is_auto() || intrinsic_size.height <= content_box_pt(0.0) {
        return Some(intrinsic_size.width.points());
    }
    let cell_height = cell_style
        .box_values
        .height
        .clone()
        .length_if_no_percent()?;
    let used_height = used_content_box_height_or_auto_with_basis(
        style,
        PercentageBasis::definite(content_box_pt(cell_height)),
        non_content_pt(0.0),
    )?;
    Some(
        (used_height.points() * intrinsic_size.width.points() / intrinsic_size.height.points())
            .max(0.0),
    )
}

fn replaced_descendant_intrinsic_widths(element: &Element) -> Vec<f32> {
    let mut widths: Vec<f32> = replaced_element_intrinsic_width(element)
        .into_iter()
        .collect();
    widths.extend(element.children.iter().flat_map(|child| match &child.kind {
        NodeKind::Element(child) => replaced_descendant_intrinsic_widths(child),
        NodeKind::Text(_) => Vec::new(),
    }));
    widths
}

fn replaced_element_intrinsic_width(element: &Element) -> Option<f32> {
    match replaced_element_kind(element) {
        Some(ReplacedElementKind::Svg) => {
            intrinsic_svg_size(element).map(|size| size.width.points())
        }
        Some(ReplacedElementKind::Canvas) => Some(intrinsic_canvas_size(element).width.points()),
        Some(ReplacedElementKind::Image) | None => None,
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

    #[test]
    fn table_root_inline_size_uses_height_in_vertical_writing() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = length(40.0);
        style.box_values.height = length(80.0);
        style.writing_mode = WritingMode::VerticalLr;

        assert_eq!(
            table_root_inline_size(&style),
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(80.0)
            )
        );

        style.writing_mode = WritingMode::HorizontalTb;
        assert_eq!(
            table_root_inline_size(&style),
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(40.0)
            )
        );
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
        content_box_style.box_values.padding.left =
            css::ComputedLengthPercentage::from_points(10.0);
        content_box_style.box_values.padding.right =
            css::ComputedLengthPercentage::from_points(10.0);
        content_box_style.padding = horizontal_edges(10.0, 10.0);
        let border_insets = Some(horizontal_edges(5.0, 5.0));

        let content_box_width = table_cell_border_box_width_from_declared_size(
            &content_box_style,
            layout_pt(100.0),
            layout_pt(300.0),
            table_cell_horizontal_non_content_width(&content_box_style, border_insets),
        );

        let mut border_box_style = content_box_style;
        border_box_style.box_sizing = BoxSizing::BorderBox;
        let border_box_width = table_cell_border_box_width_from_declared_size(
            &border_box_style,
            layout_pt(100.0),
            layout_pt(300.0),
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
