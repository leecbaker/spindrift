use super::{
    BlockSizePercentageBasis, BorderBoxLength, BoxSizing, ComputedStyle, ContentBoxLength,
    DeclaredTableTrackSize, Element, Float, LayoutBuilder, LayoutLength, LogicalBlockContentSize,
    LogicalInlineContentSize, NodeKind, NonContentLength, PercentageBasis, PhysicalContentWidth,
    Position, ReplacedElementKind, SemanticLengthExt, Stylesheets, TableAxes, TableCell,
    TableCellAxisAdapter, TableGridLength, TableInlineTrackSizing, TableRootTrackAxis,
    WritingModeAxes, border_box_pt, border_box_to_content_box_length, box_tree,
    constrain_content_width, content_box_pt, content_box_to_border_box_length, css,
    horizontal_border_width, inline_layout, intrinsic_canvas_size,
    intrinsic_inline_size_is_contained, intrinsic_padding_edges, intrinsic_svg_size, layout_points,
    layout_pt, non_content_pt, replaced_element_kind, table_horizontal_borders,
    table_vertical_borders, used_border_widths, used_box_edges, used_content_box_height_or_auto,
    used_content_box_height_or_auto_with_basis, used_content_box_size,
    used_content_box_width_or_auto, used_length_percentage_or_auto,
    used_length_percentage_or_auto_with_basis, used_padding_edges,
};
use crate::layout::table::layout::{
    table_cell_child_is_in_flow_float, table_cell_style_has_parent_percentage_block_size,
};
use crate::units::IntoLayoutLength;

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
pub(super) struct UsedTableWrapperGeometry {
    pub(super) grid_inline: LogicalInlineContentSize,
    pub(super) axes: TableAxes,
    /// Compatibility field for physical paint paths that have not yet crossed
    /// the logical-grid boundary. New sizing code must use `grid_inline`.
    pub(super) content_width: ContentBoxLength,
    // These are retained at the wrapper paint boundary.  Table sizing must use
    // the logical-axis helpers below rather than selecting physical edges.
    pub(super) border_widths: css::Edges,
    pub(super) padding: css::Edges,
}

impl UsedTableWrapperGeometry {
    pub(super) fn set_grid_inline(&mut self, grid_inline: LogicalInlineContentSize) {
        self.grid_inline = grid_inline;
        self.content_width = grid_inline.content_box_length();
    }

    pub(super) fn content_x(self, outer_x: f32) -> f32 {
        outer_x + self.border_widths.left + self.padding.left
    }

    pub(super) fn inline_non_content(self) -> NonContentLength {
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

    pub(super) fn block_non_content(self) -> NonContentLength {
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
pub(super) type UsedTableWidth = UsedTableWrapperGeometry;

/// Return the table root's authored logical inline-size property.
///
/// CSS `width` and `height` remain physical properties.  CSS Tables computes
/// its column grid on the root table's logical inline axis, which is physical
/// height in vertical writing modes:
/// <https://drafts.csswg.org/css-writing-modes-4/#dimension-mapping> and
/// <https://drafts.csswg.org/css-tables-3/#table-layout>.
pub(super) fn table_root_inline_size(style: &ComputedStyle) -> css::ComputedLengthPercentageOrAuto {
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

/// Return the wrapper max-size property that constrains the table grid's
/// logical inline axis.
pub(super) fn table_root_max_inline_size(
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
pub(super) fn table_root_block_size(style: &ComputedStyle) -> css::ComputedLengthPercentageOrAuto {
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
pub(super) fn used_table_wrapper_geometry(
    style: &ComputedStyle,
    available_outer_inline: f32,
    collapsed_outer_insets: Option<css::Edges>,
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
        used_padding_edges(
            style,
            PercentageBasis::definite(layout_pt(available_outer_inline)),
        )
        .to_css_edges()
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
        PercentageBasis::definite(content_box_pt(available_outer_inline)),
        inline_non_content,
    )
    .unwrap_or_else(|| {
        content_box_pt((available_outer_inline - inline_non_content.points()).max(0.0))
    });
    let grid_inline = constrain_table_root_inline_size(
        style,
        requested_inline,
        PercentageBasis::definite(content_box_pt(available_outer_inline)),
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
pub(super) fn used_table_width(
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
pub(super) fn used_empty_table_grid_inline_size(
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
pub(super) fn table_root_inline_content_box_size<Source>(
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
pub(super) fn constrain_table_root_inline_size<Source>(
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
pub(super) fn used_empty_table_grid_width(
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
) -> Option<DeclaredTableTrackSize> {
    declared_table_track_size_from_computed(style.box_values.width.clone())
}

/// Return a first-row cell's declared contribution to a fixed table track.
///
/// The table root, rather than the cell, selects whether the CSS `width` or
/// `height` property supplies the root inline track. This remains separate
/// from [`declared_table_cell_width`], whose physical-width interpretation is
/// used only by the automatic-layout path.
pub(super) fn declared_table_cell_track_size(
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
pub(super) fn declared_table_column_track_size(
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

pub(super) fn resolve_declared_table_track_size(
    size: DeclaredTableTrackSize,
    table_inline_size: LayoutLength,
) -> LayoutLength {
    match size {
        DeclaredTableTrackSize::Fixed(size) => layout_pt(size),
        DeclaredTableTrackSize::Percent(percent) => layout_pt(table_inline_size.points() * percent),
    }
}

pub(super) fn constrain_declared_table_track_size(
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
pub(super) fn declared_table_cell_track_border_box_size(
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
pub(super) fn declared_table_track_size_length_floor(size: DeclaredTableTrackSize) -> LayoutLength {
    match size {
        DeclaredTableTrackSize::Fixed(size) => layout_pt(size),
        DeclaredTableTrackSize::Percent(_) => layout_pt(0.0),
    }
}

pub(super) fn declared_table_cell_width_length_floor(
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

fn table_cell_track_non_content_size(
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

fn table_cell_track_border_box_size_from_declared_size(
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

pub(super) fn declared_table_track_size_percentage(size: DeclaredTableTrackSize) -> f32 {
    match size {
        DeclaredTableTrackSize::Fixed(_) => 0.0,
        DeclaredTableTrackSize::Percent(percent) => percent,
    }
}

pub(super) fn declared_table_track_size_is_non_percentage(size: DeclaredTableTrackSize) -> bool {
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
struct TableIntrinsicPercentage(f32);

impl TableIntrinsicPercentage {
    fn coefficient(self) -> f32 {
        self.0
    }
}

fn length_percentage_percent(
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

pub(super) fn constrain_table_intrinsic_width_with_floor(
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

fn intrinsic_length_constraint(value: css::ComputedLengthPercentageOrAuto) -> Option<LayoutLength> {
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

pub(super) fn distribute_first_row_fixed_width(
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

/// The paired intrinsic contributions of one table cell to its table-root
/// inline track.
///
/// CSS Tables resolves a cell's min-content and max-content contributions
/// from the same intrinsic formatting context. Keeping them together makes a
/// single sizing pass compute that context once, and prevents a caller from
/// accidentally combining bounds from different percentage-basis scopes:
/// <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>.
#[derive(Debug, Clone, Copy)]
struct TableCellIntrinsicTrackRange {
    min_content: TableGridLength,
    max_content: TableGridLength,
}

impl TableCellIntrinsicTrackRange {
    fn new(min_content: TableGridLength, max_content: TableGridLength) -> Self {
        debug_assert!(min_content.get() >= 0.0);
        debug_assert!(max_content.get() >= 0.0);
        Self {
            min_content,
            max_content: max_content.max(min_content),
        }
    }

    fn min_content(self) -> TableGridLength {
        self.min_content
    }

    fn max_content(self) -> TableGridLength {
        self.max_content
    }
}

/// Measure both intrinsic table-track contributions for one cell.
///
/// This is deliberately an ephemeral per-call value, not a layout cache.
/// Later final-layout operations retain their own constrained measurements,
/// whose percentage bases and fragmentation state can differ from automatic
/// table sizing.
fn table_cell_intrinsic_track_range(
    layout: &mut LayoutBuilder<'_>,
    cell: &TableCell<'_>,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    border_insets: Option<css::Edges>,
) -> TableCellIntrinsicTrackRange {
    let inline_contribution =
        table_cell_inline_intrinsic_contribution(layout, cell, style, stylesheets);
    let replaced_widths = table_cell_replaced_content_width_range(cell, style);
    let block_widths = table_cell_block_child_intrinsic_widths(layout, cell, stylesheets);
    let border_width = border_insets
        .map(|borders| borders.left + borders.right)
        .unwrap_or_else(|| table_horizontal_borders(style).points());
    let padding = intrinsic_padding_edges(style).to_css_edges();
    let non_content = padding.left + padding.right + border_width;

    TableCellIntrinsicTrackRange::new(
        TableGridLength::new(
            inline_contribution
                .min_content
                .points()
                .max(replaced_widths.min_content.get())
                .max(block_widths.0)
                + non_content,
        ),
        TableGridLength::new(
            inline_contribution
                .max_content
                .points()
                .max(replaced_widths.max_content.get())
                .max(block_widths.1)
                + non_content,
        ),
    )
}

pub(super) fn table_cell_content_max_width(
    layout: &mut LayoutBuilder<'_>,
    cell: &TableCell<'_>,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    border_insets: Option<css::Edges>,
) -> f32 {
    table_cell_intrinsic_track_range(layout, cell, style, stylesheets, border_insets)
        .max_content()
        .get()
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
    stylesheets: &Stylesheets<'_>,
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
    stylesheets: &Stylesheets<'_>,
    border_insets: Option<css::Edges>,
) -> inline_layout::InlineIntrinsicContribution {
    let axes = TableCellAxisAdapter::for_table(table_style);
    if axes.root_track_uses_physical_width(TableRootTrackAxis::Inline) {
        let track_range =
            table_cell_intrinsic_track_range(layout, cell, cell_style, stylesheets, border_insets);
        return inline_layout::InlineIntrinsicContribution::new(
            LogicalInlineContentSize::new(content_box_pt(track_range.min_content().get())),
            LogicalInlineContentSize::new(content_box_pt(track_range.max_content().get())),
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
    stylesheets: &Stylesheets<'_>,
) -> inline_layout::InlineIntrinsicContribution {
    let available_inline_size = table_cell_inline_intrinsic_measure(style)
        .map(LogicalInlineContentSize::points)
        .unwrap_or(f32::MAX);
    // Table structure can retain a cell as a DOM-backed source instead of a
    // prebuilt formatting-box list. Intrinsic float runs must see the same
    // frozen child boxes in both cases; otherwise the DOM-backed path drops
    // floated descendants before column measurement.
    let built_children;
    let children = if let Some(children) = cell.children.as_deref() {
        Some(children)
    } else if let Some(element) = cell.element {
        built_children =
            layout.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
        Some(built_children.as_slice())
    } else {
        None
    };
    // CSS Tables computes intrinsic column contributions before it has a
    // table-cell inline containing block. A descendant `width: 100%` must
    // therefore remain cyclic here rather than resolving against the page's
    // current content width. The final table-cell pass receives the committed
    // cell basis separately after column and row layout.
    // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
    // <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
    let measurement =
        layout.with_intrinsic_inline_percentage_basis(PercentageBasis::indefinite(), |layout| {
            if let Some(children) = children {
                layout.intrinsic_inline_measurement_for_boxes(
                    children,
                    style,
                    stylesheets,
                    available_inline_size,
                )
            } else {
                inline_layout::InlineIntrinsicMeasurement::default()
            }
        });

    // The inline intrinsic probe represents floats as zero-advance markers,
    // which is appropriate for line construction but not for a table cell's
    // max-content track contribution. Add the source-ordered float-run
    // margin-box contribution explicitly.
    let mut contribution = measurement.contribution;
    if let Some(children) = children {
        let (float_min, float_max) = layout.inline_float_run_intrinsic_widths_for_boxes(
            children,
            style,
            stylesheets,
            available_inline_size,
        );
        contribution.min_content = contribution
            .min_content
            .max(LogicalInlineContentSize::new(content_box_pt(float_min)));
        contribution.max_content = contribution
            .max_content
            .max(LogicalInlineContentSize::new(content_box_pt(float_max)));
    }

    if !WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes() {
        return contribution;
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
    stylesheets: &Stylesheets<'_>,
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
    stylesheets: &Stylesheets<'_>,
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
    stylesheets: &Stylesheets<'_>,
) -> (f32, f32) {
    // Floats generated by consecutive in-flow children occupy the same
    // hypothetical line for max-content sizing.  Treating every child as a
    // block-stack alternative loses their combined width (two 50px floats
    // incorrectly contribute 50px instead of 100px).  A cleared float starts
    // a new row; conservatively ending the current run for any `clear` value
    // is correct for all same-side runs and never merges incompatible rows.
    // <https://www.w3.org/TR/CSS22/visuren.html#floats>
    // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
    let mut contribution = (0.0_f32, 0.0_f32);
    let mut float_run = (0.0_f32, 0.0_f32);
    let flush_float_run = |contribution: &mut (f32, f32), float_run: &mut (f32, f32)| {
        contribution.0 = contribution.0.max(float_run.0);
        contribution.1 = contribution.1.max(float_run.1);
        *float_run = (0.0, 0.0);
    };

    for child in children {
        let (child_min, child_max) =
            table_cell_formatting_child_intrinsic_widths(layout, child, stylesheets);
        if table_cell_child_is_in_flow_float(child) {
            let clears = child
                .element_parts()
                .is_some_and(|(_, _, style, _)| !matches!(style.clear, css::Clear::None));
            if clears {
                flush_float_run(&mut contribution, &mut float_run);
            }
            float_run.0 = float_run.0.max(child_min);
            float_run.1 += child_max;
            continue;
        }

        flush_float_run(&mut contribution, &mut float_run);
        contribution.0 = contribution.0.max(child_min);
        contribution.1 = contribution.1.max(child_max);
    }
    flush_float_run(&mut contribution, &mut float_run);
    contribution
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
    stylesheets: &Stylesheets<'_>,
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
        layout.intrinsic_inline_contribution_for_boxes(children, &style, &css::EMPTY_STYLESHEETS)
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

/// Return a cell's paired replaced-content intrinsic contributions.
///
/// Replaced items contribute their largest individual width to min-content
/// sizing and their source-order sum to max-content sizing. Both values come
/// from the same descendant traversal so table-track measurement does not
/// inspect the cell twice.
fn table_cell_replaced_content_width_range(
    cell: &TableCell<'_>,
    cell_style: &ComputedStyle,
) -> TableCellIntrinsicTrackRange {
    let widths = table_cell_replaced_content_widths(cell, cell_style);
    let min_content = widths.iter().copied().fold(0.0_f32, f32::max);
    let max_content = widths.into_iter().sum::<f32>();
    TableCellIntrinsicTrackRange::new(
        TableGridLength::new(min_content),
        TableGridLength::new(max_content),
    )
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
        box_tree::FormattingBox::AtomicInline(box_)
            if replaced_element_kind(box_.core.element) == Some(ReplacedElementKind::Image)
                && (box_.core.element.image_rendering == crate::dom::ImageRendering::Empty
                    || crate::dom::selected_img_source(box_.core.element).is_none()) =>
        {
            // An inline image without a selected source has no intrinsic
            // dimensions. Its percentage width resolves only after the table
            // cell width is known and cannot establish a column minimum.
            // <https://html.spec.whatwg.org/multipage/images.html#the-img-element>
            // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
            Vec::new()
        }
        box_tree::FormattingBox::Replaced(box_)
            if replaced_element_kind(box_.core.element) == Some(ReplacedElementKind::Image)
                && (box_.core.element.image_rendering == crate::dom::ImageRendering::Empty
                    || crate::dom::selected_img_source(box_.core.element).is_none()) =>
        {
            // A source-less HTML image has zero intrinsic dimensions. Its
            // percentage width is resolved only during final table-cell
            // layout, never while computing an auto table's column minimum.
            // <https://html.spec.whatwg.org/multipage/images.html#the-img-element>
            // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
            Vec::new()
        }
        box_tree::FormattingBox::Replaced(box_) => replaced_intrinsic_width_with_table_cell_height(
            box_.core.element,
            &box_.core.style,
            cell_style,
        )
        .into_iter()
        .map(PhysicalContentWidth::points)
        .collect(),
        box_tree::FormattingBox::AtomicInline(box_) => {
            replaced_intrinsic_width_with_table_cell_height(
                box_.core.element,
                &box_.core.style,
                cell_style,
            )
            .into_iter()
            .map(PhysicalContentWidth::points)
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
) -> Option<PhysicalContentWidth> {
    let intrinsic_size = match replaced_element_kind(element) {
        Some(ReplacedElementKind::Svg) => intrinsic_svg_size(element),
        Some(ReplacedElementKind::Canvas) => Some(intrinsic_canvas_size(element)),
        Some(ReplacedElementKind::Image) | None => None,
    }?;
    if !style.box_values.width.clone().is_auto() || intrinsic_size.height <= content_box_pt(0.0) {
        return Some(PhysicalContentWidth::new(intrinsic_size.width));
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
    Some(PhysicalContentWidth::new(content_box_pt(
        (used_height.points() * intrinsic_size.width.points() / intrinsic_size.height.points())
            .max(0.0),
    )))
}

fn replaced_descendant_intrinsic_widths(element: &Element) -> Vec<f32> {
    let mut widths: Vec<f32> = replaced_element_intrinsic_width(element)
        .into_iter()
        .map(PhysicalContentWidth::points)
        .collect();
    widths.extend(element.children.iter().flat_map(|child| match &child.kind {
        NodeKind::Element(child) => replaced_descendant_intrinsic_widths(child),
        NodeKind::Text(_) => Vec::new(),
    }));
    widths
}

fn replaced_element_intrinsic_width(element: &Element) -> Option<PhysicalContentWidth> {
    match replaced_element_kind(element) {
        Some(ReplacedElementKind::Svg) => {
            intrinsic_svg_size(element).map(|size| PhysicalContentWidth::new(size.width))
        }
        Some(ReplacedElementKind::Canvas) => Some(PhysicalContentWidth::new(
            intrinsic_canvas_size(element).width,
        )),
        Some(ReplacedElementKind::Image) | None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::WritingMode;
    use crate::layout::BlockSizeBasisSource;

    fn length(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    #[test]
    fn table_root_inline_size_uses_height_in_vertical_writing() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = length(40.0);
        style.box_values.height.replace_with_used(length(80.0));
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

    #[test]
    fn declared_table_column_track_size_uses_root_inline_axis_not_text_orientation() {
        let mut table_style = ComputedStyle::initial();
        let mut column_style = ComputedStyle::initial();
        column_style.box_values.width = length(17.0);
        column_style
            .box_values
            .height
            .replace_with_used(length(43.0));

        let declared_size = |table_style: &ComputedStyle, column_style: &ComputedStyle| {
            match declared_table_column_track_size(
                TableInlineTrackSizing::for_table(table_style),
                column_style,
            ) {
                Some(DeclaredTableTrackSize::Fixed(value)) => value,
                value => panic!("expected fixed column size, got {value:?}"),
            }
        };

        assert_eq!(declared_size(&table_style, &column_style), 17.0);
        for writing_mode in [
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            table_style.writing_mode = writing_mode;
            for text_orientation in [
                css::TextOrientation::Mixed,
                css::TextOrientation::Upright,
                css::TextOrientation::Sideways,
            ] {
                table_style.text_orientation = text_orientation;
                assert_eq!(
                    declared_size(&table_style, &column_style),
                    43.0,
                    "{writing_mode:?} with {text_orientation:?} must use physical height"
                );
            }
        }
    }

    fn horizontal_edges(left: f32, right: f32) -> css::Edges {
        css::Edges {
            top: 0.0,
            right,
            bottom: 0.0,
            left,
        }
    }

    fn vertical_edges(top: f32, bottom: f32) -> css::Edges {
        css::Edges {
            top,
            right: 0.0,
            bottom,
            left: 0.0,
        }
    }

    fn style_with_width(width: css::ComputedLengthPercentageOrAuto) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.box_values.width = width;
        style
    }

    #[test]
    fn mixed_table_width_does_not_contribute_an_intrinsic_percentage() {
        let mixed = css::ComputedLengthPercentage::from_affine(layout_pt(12.0), 0.5, true);
        let pure_percentage = css::ComputedLengthPercentage::from_percent(0.5);

        assert_eq!(
            length_percentage_percent(css::ComputedLengthPercentageOrAuto::LengthPercentage(mixed)),
            None
        );
        assert_eq!(
            length_percentage_percent(css::ComputedLengthPercentageOrAuto::LengthPercentage(
                pure_percentage
            )),
            Some(TableIntrinsicPercentage(0.5))
        );
    }

    #[test]
    fn mixed_min_and_max_widths_do_not_partially_constrain_intrinsic_columns() {
        let mixed = css::ComputedLengthPercentage::from_affine(layout_pt(12.0), 0.5, true);
        let fixed = css::ComputedLengthPercentage::from_points(12.0);

        assert_eq!(
            intrinsic_length_constraint(css::ComputedLengthPercentageOrAuto::LengthPercentage(
                mixed
            )),
            None
        );
        assert_eq!(
            intrinsic_length_constraint(css::ComputedLengthPercentageOrAuto::LengthPercentage(
                fixed
            )),
            Some(layout_pt(12.0))
        );
    }

    #[test]
    fn intrinsic_column_constraints_give_min_width_precedence_over_max_width() {
        let mut style = ComputedStyle::initial();
        style.box_values.min_width = length(100.0);
        style.box_values.max_width = length(0.0);

        assert_eq!(
            constrain_table_intrinsic_width_with_floor(&style, 0.0, 0.0),
            100.0
        );
    }

    #[test]
    fn table_cell_intrinsic_track_range_preserves_ordered_min_and_max_bounds() {
        let range = TableCellIntrinsicTrackRange::new(
            TableGridLength::new(12.0),
            TableGridLength::new(20.0),
        );
        assert_eq!(range.min_content().get(), 12.0);
        assert_eq!(range.max_content().get(), 20.0);

        let clamped = TableCellIntrinsicTrackRange::new(
            TableGridLength::new(20.0),
            TableGridLength::new(12.0),
        );
        assert_eq!(clamped.min_content().get(), 20.0);
        assert_eq!(clamped.max_content().get(), 20.0);
    }

    #[test]
    fn wrapper_geometry_uses_horizontal_edges_for_horizontal_inline_size() {
        let mut style = style_with_width(length(150.0));
        style.border_collapse = css::BorderCollapse::Separate;
        style.box_values.padding.left = css::ComputedLengthPercentage::from_points(10.0);
        style.box_values.padding.right = css::ComputedLengthPercentage::from_points(10.0);
        style.padding.left = 10.0;
        style.padding.right = 10.0;

        let geometry = used_table_wrapper_geometry(&style, 300.0, None);

        assert_eq!(geometry.grid_inline.points(), 150.0);
        assert_eq!(geometry.inline_non_content().points(), 20.0);
        assert_eq!(geometry.block_non_content().points(), 0.0);
    }

    #[test]
    fn wrapper_geometry_applies_min_and_max_inline_constraints() {
        let mut min_style = style_with_width(length(40.0));
        min_style.box_values.min_width = length(80.0);
        assert_eq!(
            used_table_wrapper_geometry(&min_style, 300.0, None)
                .grid_inline
                .points(),
            80.0
        );

        let mut max_style = style_with_width(length(120.0));
        max_style.box_values.max_width = length(60.0);
        assert_eq!(
            used_table_wrapper_geometry(&max_style, 300.0, None)
                .grid_inline
                .points(),
            60.0
        );
    }

    #[test]
    fn collapsed_border_box_width_removes_outer_half_insets_once() {
        let mut style = style_with_width(length(180.0));
        style.border_collapse = css::BorderCollapse::Collapse;
        style.box_sizing = BoxSizing::BorderBox;

        let geometry =
            used_table_wrapper_geometry(&style, 300.0, Some(horizontal_edges(10.0, 10.0)));

        assert_eq!(geometry.grid_inline.points(), 160.0);
        assert_eq!(geometry.inline_non_content().points(), 20.0);
    }

    #[test]
    fn collapsed_content_box_width_keeps_grid_width_inside_outer_half_insets() {
        let mut style = style_with_width(length(180.0));
        style.border_collapse = css::BorderCollapse::Collapse;
        style.box_sizing = BoxSizing::ContentBox;

        let geometry =
            used_table_wrapper_geometry(&style, 300.0, Some(horizontal_edges(10.0, 10.0)));

        assert_eq!(geometry.grid_inline.points(), 180.0);
        assert_eq!(geometry.inline_non_content().points(), 20.0);
    }

    #[test]
    fn wrapper_geometry_uses_vertical_edges_for_vertical_inline_size() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.box_values.height.replace_with_used(length(150.0));
        style.box_values.padding.top = css::ComputedLengthPercentage::from_points(10.0);
        style.box_values.padding.bottom = css::ComputedLengthPercentage::from_points(20.0);
        style.padding.top = 10.0;
        style.padding.bottom = 20.0;

        let geometry = used_table_wrapper_geometry(&style, 300.0, None);

        assert_eq!(geometry.grid_inline.points(), 150.0);
        assert_eq!(geometry.inline_non_content().points(), 30.0);
        assert_eq!(geometry.block_non_content().points(), 0.0);
    }

    #[test]
    fn vertical_wrapper_inline_constraints_use_height_not_width() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.box_values.min_height = length(100.0);
        style.box_values.max_height = length(130.0);
        style.box_values.min_width = length(280.0);
        style.box_values.max_width = length(10.0);

        assert_eq!(
            used_table_wrapper_geometry(&style, 50.0, None)
                .grid_inline
                .points(),
            100.0
        );
        assert_eq!(
            used_table_wrapper_geometry(&style, 300.0, None)
                .grid_inline
                .points(),
            130.0
        );
    }

    #[test]
    fn collapsed_vertical_border_box_uses_top_and_bottom_insets() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.border_collapse = css::BorderCollapse::Collapse;
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.height.replace_with_used(length(180.0));

        let geometry = used_table_wrapper_geometry(&style, 300.0, Some(vertical_edges(10.0, 20.0)));

        assert_eq!(geometry.grid_inline.points(), 150.0);
        assert_eq!(geometry.inline_non_content().points(), 30.0);
    }

    #[test]
    fn resolved_collapsed_wrapper_insets_keep_asymmetric_half_borders_on_both_axes() {
        let insets = ResolvedTableWrapperInsets {
            border_widths: css::Edges {
                top: 72.0 / 2.54,
                right: 108.0 / 2.54,
                bottom: 72.0 / 2.54,
                left: 108.0 / 2.54,
            },
        };

        assert!((insets.border_widths.top - 72.0 / 2.54).abs() < 0.01);
        assert!((insets.border_widths.left - 108.0 / 2.54).abs() < 0.01);
        assert!((insets.vertical_non_content().points() - 2.0 * 72.0 / 2.54).abs() < 0.01);
        assert!((insets.horizontal_non_content().points() - 2.0 * 108.0 / 2.54).abs() < 0.01);
    }

    #[test]
    fn collapsed_border_box_height_removes_outer_half_insets_once() {
        let mut border_box = ComputedStyle::initial();
        border_box.box_sizing = BoxSizing::BorderBox;
        border_box
            .box_values
            .height
            .replace_with_used(length(100.0));
        let vertical_insets = non_content_pt(10.0 + 20.0);

        assert_eq!(
            used_table_target_content_height(
                &border_box,
                PercentageBasis::definite_from(
                    content_box_pt(300.0),
                    BlockSizeBasisSource::TableWrapper,
                ),
                vertical_insets,
            )
            .unwrap()
            .points(),
            70.0
        );

        let mut content_box = border_box;
        content_box.box_sizing = BoxSizing::ContentBox;
        assert_eq!(
            used_table_target_content_height(
                &content_box,
                PercentageBasis::definite_from(
                    content_box_pt(300.0),
                    BlockSizeBasisSource::TableWrapper,
                ),
                vertical_insets,
            )
            .unwrap()
            .points(),
            100.0
        );
    }

    #[test]
    fn empty_table_border_box_width_clamps_content_box_at_zero() {
        let mut style = style_with_width(length(100.0));
        style.box_sizing = BoxSizing::BorderBox;
        let table_width = UsedTableWidth {
            grid_inline: LogicalInlineContentSize::new(content_box_pt(0.0)),
            axes: TableAxes::for_style(&style),
            content_width: content_box_pt(0.0),
            border_widths: css::Edges::ZERO,
            padding: horizontal_edges(75.0, 75.0),
        };

        let content = used_empty_table_grid_width(&style, 300.0, table_width);

        assert_eq!(content.points(), 0.0);
    }

    #[test]
    fn declared_table_cell_track_border_box_size_uses_matching_axis_insets() {
        let mut content_box_style = ComputedStyle::initial();
        content_box_style.box_sizing = BoxSizing::ContentBox;
        content_box_style.box_values.padding.left =
            css::ComputedLengthPercentage::from_points(10.0);
        content_box_style.box_values.padding.right =
            css::ComputedLengthPercentage::from_points(10.0);
        content_box_style.padding = horizontal_edges(10.0, 10.0);
        let border_insets = Some(horizontal_edges(5.0, 5.0));

        for writing_mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl] {
            let mut content_box_style = content_box_style.clone();
            let border_insets = if writing_mode.has_vertical_lines() {
                content_box_style.box_values.padding.top =
                    css::ComputedLengthPercentage::from_points(10.0);
                content_box_style.box_values.padding.bottom =
                    css::ComputedLengthPercentage::from_points(10.0);
                content_box_style.padding = vertical_edges(10.0, 10.0);
                Some(vertical_edges(5.0, 5.0))
            } else {
                border_insets
            };
            content_box_style.writing_mode = writing_mode;
            let track = TableInlineTrackSizing::for_table(&content_box_style);
            let content_box_size = table_cell_track_border_box_size_from_declared_size(
                track,
                &content_box_style,
                layout_pt(100.0),
                layout_pt(300.0),
                table_cell_track_non_content_size(track, &content_box_style, border_insets),
            );

            let mut border_box_style = content_box_style;
            border_box_style.box_sizing = BoxSizing::BorderBox;
            let border_box_size = table_cell_track_border_box_size_from_declared_size(
                track,
                &border_box_style,
                layout_pt(100.0),
                layout_pt(300.0),
                table_cell_track_non_content_size(track, &border_box_style, border_insets),
            );

            assert_eq!(content_box_size.points(), 130.0, "{writing_mode:?}");
            assert_eq!(border_box_size.points(), 100.0, "{writing_mode:?}");
        }
    }

    #[test]
    fn empty_table_auto_grid_width_is_zero_content_box() {
        let style = style_with_width(css::ComputedLengthPercentageOrAuto::Auto);
        let table_width = UsedTableWidth {
            grid_inline: LogicalInlineContentSize::new(content_box_pt(0.0)),
            axes: TableAxes::for_style(&style),
            content_width: content_box_pt(0.0),
            border_widths: css::Edges::ZERO,
            padding: css::Edges::ZERO,
        };

        let content = used_empty_table_grid_width(&style, 300.0, table_width);

        assert_eq!(content.points(), 0.0);
    }

    #[test]
    fn explicit_table_width_is_not_clamped_to_the_font_size() {
        let style = style_with_width(length(2.7));

        let table_width = used_table_width(&style, 300.0, None);

        assert_eq!(table_width.content_width.points(), 2.7);
    }

    #[test]
    fn intrinsic_table_wrapper_probe_does_not_promote_authored_block_sizes_to_grid_minimums() {
        let mut style = ComputedStyle::initial();
        style.box_values.height.replace_with_used(length(500.0));
        style.box_values.min_height = length(80.0);

        let probe = intrinsic_table_wrapper_block_probe_style(&style);

        assert!(probe.box_values.height.is_auto());
        assert_eq!(
            probe.box_values.min_height,
            css::ComputedLengthPercentageOrAuto::Auto
        );
        assert_eq!(probe.box_values.width, style.box_values.width);
    }
}
