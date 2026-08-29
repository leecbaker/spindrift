//! Intrinsic inline contributions from table-cell contents.

use super::*;
use crate::layout::table::layout::{
    table_cell_child_is_in_flow_float, table_cell_style_has_parent_percentage_block_size,
};

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

pub(in crate::layout::table) fn table_cell_content_max_width(
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
pub(in crate::layout::table) fn table_cell_physical_width_minimum(
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
pub(in crate::layout::table) fn table_cell_root_block_track_contribution(
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
pub(in crate::layout::table) fn table_cell_content_table_inline_size(
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
        let (float_min, float_max) = layout.with_intrinsic_inline_percentage_basis(
            PercentageBasis::indefinite(),
            |layout| {
                layout.inline_float_run_intrinsic_widths_for_boxes(
                    children,
                    style,
                    stylesheets,
                    available_inline_size,
                )
            },
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
pub(in crate::layout::table) fn table_cell_inline_intrinsic_measure(
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
        let available_outer_width = layout.current_content_logical_inline_size();
        return layout.table_outer_intrinsic_widths_with_indefinite_percentage_basis_from_fragment(
            box_.core.element,
            &style,
            stylesheets,
            &box_.fragment,
            // This supports non-percentage table mechanics such as
            // `fit-content`, but is not a cell percentage basis. The real
            // containing block is the outer cell, whose track width is being
            // measured here, so cyclic inline percentages remain indefinite.
            available_outer_width,
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
