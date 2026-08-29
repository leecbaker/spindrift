//! Table wrapper sizing and authored-size resolution.

use super::*;
/// Parent-facing intrinsic sizing data for a table wrapper used as a flex item.
///
/// A table has two relevant boxes at the flex boundary: its grid supplies the
/// table-specific automatic minimum, while its wrapper (including captions)
/// supplies preferred and block-size contributions.  Keeping them together
/// prevents flex estimation, stretch remeasurement, and replay from choosing
/// incompatible table representations.
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>
#[allow(dead_code)] // Additional consumers are added at table-wrapper boundaries incrementally.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TableWrapperFlexSizing {
    pub(in crate::layout) grid_min_content_inline: LogicalInlineContentSize,
    pub(in crate::layout) grid_max_content_inline: TableMaxContentInline,
    pub(in crate::layout) wrapper_preferred_inline: TableMaxContentInline,
    pub(in crate::layout) wrapper_intrinsic_block: LogicalBlockContentSize,
    /// Decoration belongs to the wrapper, not the grid content contribution.
    pub(in crate::layout) inline_non_content: NonContentLength,
    pub(in crate::layout) block_non_content: NonContentLength,
    /// Margins remain outside table-grid sizing and are consumed only by the
    /// parent flex outer-size calculation.
    pub(in crate::layout) margins: css::Edges,
}

/// A table max-content contribution may be genuinely unbounded when
/// percentage columns consume the full percentage budget alongside a
/// non-percentage column.  Keep that semantic state out of scalar Flex/Taffy
/// adapters; `f32::MAX` is an implementation sentinel, not a CSS length.
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum TableMaxContentInline {
    Finite(LogicalInlineContentSize),
    Unbounded,
}

impl TableMaxContentInline {
    pub(in crate::layout) fn from_table_measurement(value: f32) -> Self {
        if value >= f32::MAX / 4.0 {
            Self::Unbounded
        } else {
            Self::Finite(LogicalInlineContentSize::new(content_box_pt(
                value.max(0.0),
            )))
        }
    }

    /// Resolve an unbounded intrinsic query in the definite available slot
    /// required by Flexbox's content-sizing step.
    pub(in crate::layout) fn resolve_against(
        self,
        available: LogicalInlineContentSize,
    ) -> LogicalInlineContentSize {
        match self {
            Self::Finite(value) => value,
            Self::Unbounded => available,
        }
    }
}

/// Prepare a table-wrapper probe for Flexbox's intrinsic automatic minimum.
///
/// The probe must not inherit the table's authored block preferred size or
/// minimum. Flexbox consumes those at their own specified-size and minimum
/// phases; using either while measuring the grid would turn them into a
/// second automatic minimum.
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
pub(in crate::layout) fn intrinsic_table_wrapper_block_probe_style(
    style: &ComputedStyle,
) -> ComputedStyle {
    let mut probe = style.clone();
    probe
        .box_values
        .height
        .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
    probe.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
    probe
}

/// Conflict-resolved table-wrapper border insets.
///
/// In the collapsed border model CSS Tables assigns the table root half of
/// each winning outer grid-edge border. These insets are therefore ordinary
/// wrapper border-box contributions at every sizing and positioning boundary,
/// even though cells remain the sole painters of the full centered rules.
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
/// <https://drafts.csswg.org/css-tables-3/#border-collapse>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ResolvedTableWrapperInsets {
    pub(in crate::layout) border_widths: css::Edges,
}

impl ResolvedTableWrapperInsets {
    pub(in crate::layout) const ZERO: Self = Self {
        border_widths: css::Edges::ZERO,
    };

    pub(in crate::layout) fn horizontal_non_content(self) -> NonContentLength {
        non_content_pt(self.border_widths.left + self.border_widths.right)
    }

    pub(in crate::layout) fn vertical_non_content(self) -> NonContentLength {
        non_content_pt(self.border_widths.top + self.border_widths.bottom)
    }
}

/// Resolved wrapper decoration and grid size at the table logical-axis boundary.
///
/// CSS Tables sizes the grid in the root table's logical inline axis.  The
/// physical padding and border edges remain here only for wrapper painting and
/// are projected through `axes`; sizing callers must use `inline_non_content`.
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct UsedTableWrapperGeometry {
    pub(in crate::layout::table) grid_inline: LogicalInlineContentSize,
    pub(in crate::layout::table) axes: TableAxes,
    /// Compatibility field for physical paint paths that have not yet crossed
    /// the logical-grid boundary. New sizing code must use `grid_inline`.
    pub(in crate::layout::table) content_width: ContentBoxLength,
    // These are retained at the wrapper paint boundary.  Table sizing must use
    // the logical-axis helpers below rather than selecting physical edges.
    pub(in crate::layout::table) border_widths: css::Edges,
    pub(in crate::layout::table) padding: css::Edges,
}

impl UsedTableWrapperGeometry {
    pub(in crate::layout::table) fn set_grid_inline(
        &mut self,
        grid_inline: LogicalInlineContentSize,
    ) {
        self.grid_inline = grid_inline;
        self.content_width = grid_inline.content_box_length();
    }

    pub(in crate::layout::table) fn content_x(self, outer_x: f32) -> f32 {
        outer_x + self.border_widths.left + self.padding.left
    }

    pub(in crate::layout::table) fn inline_non_content(self) -> NonContentLength {
        if self.axes.flow.writing_mode().has_vertical_lines() {
            non_content_pt(
                self.border_widths.top
                    + self.border_widths.bottom
                    + self.padding.top
                    + self.padding.bottom,
            )
        } else {
            non_content_pt(
                self.border_widths.left
                    + self.border_widths.right
                    + self.padding.left
                    + self.padding.right,
            )
        }
    }

    pub(in crate::layout::table) fn block_non_content(self) -> NonContentLength {
        if self.axes.flow.writing_mode().has_vertical_lines() {
            non_content_pt(
                self.border_widths.left
                    + self.border_widths.right
                    + self.padding.left
                    + self.padding.right,
            )
        } else {
            non_content_pt(
                self.border_widths.top
                    + self.border_widths.bottom
                    + self.padding.top
                    + self.padding.bottom,
            )
        }
    }
}

// The table paint pipeline still carries this record under its historical
// local name.  Keep the alias while its physical paint call sites are migrated;
// all sizing entry points use `UsedTableWrapperGeometry` directly.
pub(in crate::layout::table) type UsedTableWidth = UsedTableWrapperGeometry;

/// Return the table root's authored logical inline-size property.
///
/// CSS `width` and `height` remain physical properties.  CSS Tables computes
/// its column grid on the root table's logical inline axis, which is physical
/// height in vertical writing modes:
/// <https://drafts.csswg.org/css-writing-modes-4/#dimension-mapping> and
/// <https://drafts.csswg.org/css-tables-3/#table-layout>.
pub(in crate::layout::table) fn table_root_inline_size(
    style: &ComputedStyle,
) -> css::ComputedLengthPercentageOrAuto {
    if style.writing_mode.has_vertical_lines() {
        style.box_values.height.value().clone()
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
pub(in crate::layout::table) fn table_root_distributes_extra_inline_space(
    style: &ComputedStyle,
) -> bool {
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
pub(in crate::layout::table) fn table_root_min_inline_size(
    style: &ComputedStyle,
) -> css::ComputedLengthPercentageOrAuto {
    if style.writing_mode.has_vertical_lines() {
        style.box_values.min_height.clone()
    } else {
        style.box_values.min_width.clone()
    }
}

/// Return the wrapper max-size property that constrains the table grid's
/// logical inline axis.
pub(in crate::layout::table) fn table_root_max_inline_size(
    style: &ComputedStyle,
) -> css::ComputedLengthPercentageOrAuto {
    if style.writing_mode.has_vertical_lines() {
        style.box_values.max_height.clone()
    } else {
        style.box_values.max_width.clone()
    }
}

/// Return the physical CSS property that controls a table root's logical block
/// axis. CSS Tables distributes row tracks on that axis, which is physical
/// width in vertical writing modes.
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping> and
/// <https://drafts.csswg.org/css-tables-3/#row-layout>
pub(in crate::layout::table) fn table_root_block_size(
    style: &ComputedStyle,
) -> css::ComputedLengthPercentageOrAuto {
    if style.writing_mode.has_vertical_lines() {
        style.box_values.width.clone()
    } else {
        style.box_values.height.value().clone()
    }
}

/// Resolves the table wrapper's used width into the content/grid width.
///
/// CSS Tables lays out columns in the table grid, while CSS Box Sizing defines
/// whether the authored `width` applies to the content box or border box. In
/// the collapsed border model, table borders are conflict-resolved grid-edge
/// borders. Once conflict resolution has produced the outer half-widths, they
/// are the table wrapper's border-box contribution and must participate in
/// this conversion exactly once:
/// <https://www.w3.org/TR/css-tables-3/#layout> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing> and
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
pub(in crate::layout::table) fn used_table_wrapper_geometry(
    style: &ComputedStyle,
    available_outer_inline: f32,
    collapsed_outer_insets: Option<css::Edges>,
) -> UsedTableWrapperGeometry {
    used_table_wrapper_geometry_with_percentage_basis(
        style,
        available_outer_inline,
        collapsed_outer_insets,
        PercentageBasis::definite(layout_pt(available_outer_inline)),
    )
}

/// Resolve table wrapper geometry for a known or cyclic inline percentage
/// basis.
///
/// Intrinsic callers use an indefinite basis so percentage padding and
/// min/max constraints follow CSS Sizing's cyclic-percentage rules instead
/// of resolving against their numeric measurement scratch space.
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-contribution>
pub(in crate::layout::table) fn used_table_wrapper_geometry_with_percentage_basis(
    style: &ComputedStyle,
    available_outer_inline: f32,
    collapsed_outer_insets: Option<css::Edges>,
    percentage_basis: PercentageBasis<LayoutLength>,
) -> UsedTableWrapperGeometry {
    let collapsed = style.border_collapse == css::BorderCollapse::Collapse;
    let wrapper_insets = if collapsed {
        ResolvedTableWrapperInsets {
            border_widths: collapsed_outer_insets.unwrap_or(css::Edges::ZERO),
        }
    } else {
        ResolvedTableWrapperInsets {
            border_widths: used_border_widths(style),
        }
    };
    let border_widths = wrapper_insets.border_widths;
    let padding = if collapsed {
        css::Edges::ZERO
    } else {
        used_padding_edges(style, percentage_basis).to_css_edges()
    };
    let geometry = UsedTableWrapperGeometry {
        grid_inline: LogicalInlineContentSize::new(content_box_pt(0.0)),
        axes: TableAxes::for_style(style),
        content_width: content_box_pt(0.0),
        border_widths,
        padding,
    };
    let inline_non_content = geometry.inline_non_content();
    let requested_inline = table_root_inline_content_box_size(
        table_root_inline_size(style),
        style.box_sizing,
        percentage_basis.map_value(|basis| content_box_pt(basis.points())),
        inline_non_content,
    )
    .unwrap_or_else(|| {
        content_box_pt((available_outer_inline - inline_non_content.points()).max(0.0))
    });
    let grid_inline = constrain_table_root_inline_size(
        style,
        requested_inline,
        percentage_basis.map_value(|basis| content_box_pt(basis.points())),
        inline_non_content,
    );

    UsedTableWrapperGeometry {
        grid_inline,
        content_width: grid_inline.content_box_length(),
        ..geometry
    }
}

/// Transitional compatibility entry point for wrapper paint and legacy
/// intrinsic probes. New table sizing boundaries use
/// [`used_table_wrapper_geometry`].
pub(in crate::layout::table) fn used_table_width(
    style: &ComputedStyle,
    available_outer_inline: f32,
    collapsed_outer_insets: Option<css::Edges>,
) -> UsedTableWrapperGeometry {
    used_table_wrapper_geometry(style, available_outer_inline, collapsed_outer_insets)
}

/// Resolves the row-grid content width for a table with no rows or cells.
///
/// CSS Tables 3 keeps an empty table's grid box in layout: if the grid has no
/// slots and `width` is auto, the grid content width is zero. In collapsed
/// border mode CSS 2.2 derives wrapper border insets from the collapsed grid;
/// with no slots that grid contributes no padding or border inset.
/// <https://drafts.csswg.org/css-tables/#computing-the-table-width> and
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
pub(in crate::layout::table) fn used_empty_table_grid_inline_size(
    style: &ComputedStyle,
    available_outer_inline: f32,
    table_geometry: UsedTableWrapperGeometry,
) -> ContentBoxLength {
    let inline_non_content = table_geometry.inline_non_content();
    let requested_inline = table_root_inline_content_box_size(
        table_root_inline_size(style),
        style.box_sizing,
        PercentageBasis::definite(content_box_pt(available_outer_inline)),
        inline_non_content,
    )
    .unwrap_or_else(|| content_box_pt(0.0));
    constrain_table_root_inline_size(
        style,
        requested_inline,
        PercentageBasis::definite(content_box_pt(available_outer_inline)),
        inline_non_content,
    )
    .content_box_length()
}

/// Resolve one logical-inline table-root size property into grid content-box
/// space.  The supplied non-content is intentionally logical-axis specific:
/// a vertical root subtracts top/bottom decoration, never left/right.
pub(in crate::layout::table) fn table_root_inline_content_box_size<Source>(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    inline_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size(value, box_sizing, percentage_basis, inline_non_content)
}

/// Apply the table root's logical-inline min/max constraints to a grid size.
///
/// This is deliberately separate from `constrain_content_width`: CSS physical
/// `height` / `min-height` / `max-height` constrain a vertical root's logical
/// inline axis.  Resolving all three through the same box-sizing conversion
/// also makes CSS's min-over-max precedence explicit.
pub(in crate::layout::table) fn constrain_table_root_inline_size<Source>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    inline_non_content: NonContentLength,
) -> LogicalInlineContentSize {
    let max_percentage_basis = match percentage_basis {
        PercentageBasis::Definite { value, .. } => PercentageBasis::definite(value),
        PercentageBasis::Indefinite => PercentageBasis::indefinite(),
    };
    let min = table_root_inline_content_box_size(
        table_root_min_inline_size(style),
        style.box_sizing,
        max_percentage_basis,
        inline_non_content,
    );
    let max = table_root_inline_content_box_size(
        table_root_max_inline_size(style),
        style.box_sizing,
        percentage_basis,
        inline_non_content,
    );
    let min = min.unwrap_or_else(|| content_box_pt(0.0));
    let max = max.map(|max| max.max(min));
    let constrained = value.max(min);
    LogicalInlineContentSize::new(content_box_pt(
        max.map_or(constrained, |maximum| constrained.min(maximum))
            .points()
            .max(0.0),
    ))
}

/// Transitional compatibility entry point. New table sizing code uses
/// [`used_empty_table_grid_inline_size`].
pub(in crate::layout::table) fn used_empty_table_grid_width(
    style: &ComputedStyle,
    available_outer_inline: f32,
    table_geometry: UsedTableWrapperGeometry,
) -> ContentBoxLength {
    used_empty_table_grid_inline_size(style, available_outer_inline, table_geometry)
}

/// Resolves the row-grid content height for a table with no rows or cells.
///
/// CSS Tables 3 treats a definite table grid box height as the table's minimum
/// row-grid height. With no rows and auto height, that grid content height is
/// zero; collapsed tables have no separated wrapper padding or border around
/// that empty grid:
/// <https://drafts.csswg.org/css-tables/#computing-the-table-height>.
pub(in crate::layout::table) fn used_empty_table_grid_height(
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
pub(in crate::layout::table) fn used_table_target_content_height(
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
