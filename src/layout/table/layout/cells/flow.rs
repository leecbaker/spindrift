//! Table-cell inline layout, baselines, and overflow clipping.

use crate::css::{
    self, CaptionSide, ComputedStyle, PercentageBasis, Position, SemanticLengthExt, Stylesheets,
    layout_pt,
};
use crate::dom::Element;
use crate::layout::inline_collect::InlinePlacement;
use crate::layout::table::layout::{
    CollapsedTableGeometry, TableCellBaselineSet, TableCellClipRegion, TableCellContentPass,
    TableCellContentSizingPolicy, TableGridLayoutContext, apply_table_cell_content_sizing_policy,
    formatting_boxes_have_textual_baseline, table_cell_alignment_baseline_set,
    table_cell_canvas_first_pass_outer_height,
    table_cell_formatting_child_has_parent_percentage_block_size,
    table_cell_has_in_flow_layout_child, table_cell_measured_inline_outer_height_without_policy,
    table_cell_textual_baseline_style, table_cell_textual_children_match_baseline_style,
    visible_column_span,
};
use crate::layout::table::{
    CollapsedBorderGrid, TableAxes, TableCell, TableCellBaselineOffset, TableCellBorderBox,
    TableCellOuterBlockSize, TableCellPadding, TableColumn, TableGrid, TableGridLength,
    TableGridPlacement, TableGridPoint, TableGridRect, TableGridSize, TableLayoutInput,
    TableMetrics, TablePartUsedStyle, TableRow, TableRowBaselineOffset,
    table_cell_formatting_child_outer_height, table_cell_href, table_cell_inline_text,
    table_column_group_spans, table_grid, table_metrics, table_row_group_spans,
    table_row_is_collapsed, table_vertical_borders, used_table_width,
};
use crate::layout::{
    GeneratedPseudoCounterMode, InlineVisualOffset, LayoutBuilder, LogicalInlineContentSize,
    OverflowClip, PageTopRect, PhysicalContentWidth, ReplacedElementKind, UsedOverflowAxes,
    apply_used_box_metrics, box_tree, formatting_box_has_inline_content,
    has_non_inline_formatting_box, inline_layout, inline_text_from_formatting_boxes,
    layout_containment_applies_to_element, paint_containment_applies_to_element, parse_html_length,
    replaced_element_kind, set_style_auto_height, used_border_widths,
    used_canvas_size_with_height_basis, used_content_box_height_or_auto, used_image, used_svg,
};
use crate::units::{content_box_pt, non_content_pt};
impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::table) fn table_cell_measured_inline_outer_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> Option<TableCellOuterBlockSize> {
        if !table_cell_formatting_child_has_parent_percentage_block_size(child) {
            return table_cell_measured_inline_outer_height_without_policy(child, available_width);
        }
        match child {
            box_tree::FormattingBox::Inline(box_) => {
                if matches!(
                    box_.core.style.position,
                    Position::Absolute | Position::Fixed
                ) {
                    Some(TableCellOuterBlockSize::new(layout_pt(0.0)))
                } else {
                    Some(TableCellOuterBlockSize::new(
                        table_cell_formatting_child_outer_height(child),
                    ))
                }
            }
            box_tree::FormattingBox::AtomicInline(box_)
                if replaced_element_kind(box_.core.element)
                    == Some(ReplacedElementKind::Canvas) =>
            {
                let style = self.table_cell_content_sizing_style(
                    &box_.core.style,
                    TableCellContentSizingPolicy::RowMinimum,
                );
                Some(TableCellOuterBlockSize::new(layout_pt(
                    table_cell_canvas_first_pass_outer_height(
                        box_.core.element,
                        &style,
                        available_width,
                    ),
                )))
            }
            box_tree::FormattingBox::Replaced(box_)
                if replaced_element_kind(box_.core.element)
                    == Some(ReplacedElementKind::Canvas) =>
            {
                let style = self.table_cell_content_sizing_style(
                    &box_.core.style,
                    TableCellContentSizingPolicy::RowMinimum,
                );
                Some(TableCellOuterBlockSize::new(layout_pt(
                    table_cell_canvas_first_pass_outer_height(
                        box_.core.element,
                        &style,
                        available_width,
                    ),
                )))
            }
            box_tree::FormattingBox::AtomicInline(box_)
                if replaced_element_kind(box_.core.element) == Some(ReplacedElementKind::Image)
                    && box_
                        .core
                        .element
                        .attrs
                        .get("src")
                        .is_none_or(|source| source.is_empty()) =>
            {
                // An inline `img` without a selected source has no intrinsic
                // replaced object. Its percentage height is ignored while
                // determining the table-row minimum.
                // <https://drafts.csswg.org/css-tables-3/#row-layout>
                Some(TableCellOuterBlockSize::new(layout_pt(0.0)))
            }
            box_tree::FormattingBox::Replaced(box_)
                if replaced_element_kind(box_.core.element) == Some(ReplacedElementKind::Image)
                    && box_
                        .core
                        .element
                        .attrs
                        .get("src")
                        .is_none_or(|source| source.is_empty()) =>
            {
                // An `img` with no image source has no intrinsic replaced
                // object. Its percentage height is ignored for table-row
                // minimum sizing, and its final percentage layout may not
                // feed back into the distributed row plan.
                // <https://drafts.csswg.org/css-tables-3/#row-layout>
                // <https://html.spec.whatwg.org/multipage/images.html#the-img-element>
                Some(TableCellOuterBlockSize::new(layout_pt(0.0)))
            }
            box_tree::FormattingBox::AtomicInline(box_) => Some(TableCellOuterBlockSize::new(
                layout_pt(self.table_cell_row_minimum_atomic_inline_outer_height(
                    &box_.core.style,
                    &box_.core.children,
                    stylesheets,
                    available_width,
                )),
            )),
            box_tree::FormattingBox::Replaced(box_) => Some(TableCellOuterBlockSize::new(
                layout_pt(self.table_cell_row_minimum_atomic_inline_outer_height(
                    &box_.core.style,
                    &box_.core.children,
                    stylesheets,
                    available_width,
                )),
            )),
            box_tree::FormattingBox::AnonymousBlock(_)
            | box_tree::FormattingBox::InlineSplitBlockContext(_)
            | box_tree::FormattingBox::Block(_)
            | box_tree::FormattingBox::Table(_)
            | box_tree::FormattingBox::Flex(_)
            | box_tree::FormattingBox::Text(_) => None,
        }
    }

    /// Measure an atomic inline after CSS Tables has committed the table-cell
    /// content-box block size for its second layout pass.
    ///
    /// CSS Tables resolves percentage block sizes against that committed cell
    /// size, whereas first-pass row minimum sizing must keep them indefinite:
    /// <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout>.
    pub(in crate::layout::table) fn table_cell_measured_inline_outer_height_with_basis(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        content_pass: TableCellContentPass,
    ) -> Option<TableCellOuterBlockSize> {
        let Some(final_basis) = content_pass.final_basis() else {
            return self.table_cell_measured_inline_outer_height(
                child,
                stylesheets,
                available_width,
            );
        };
        let percentage_height_basis = final_basis.percentage_basis();
        let (element, style) = match child {
            box_tree::FormattingBox::AtomicInline(box_) => (box_.core.element, &box_.core.style),
            box_tree::FormattingBox::Replaced(box_) => (box_.core.element, &box_.core.style),
            _ => {
                return self.table_cell_measured_inline_outer_height(
                    child,
                    stylesheets,
                    available_width,
                );
            }
        };
        let mut style = self
            .table_cell_content_sizing_style(style, TableCellContentSizingPolicy::FinalRelayout);
        match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => {
                let box_metrics = apply_used_box_metrics(
                    &mut style,
                    PercentageBasis::definite(layout_pt(available_width.max(0.0))),
                );
                let (_width, height) = used_canvas_size_with_height_basis(
                    element,
                    &style,
                    available_width,
                    percentage_height_basis,
                );
                Some(TableCellOuterBlockSize::new(layout_pt(
                    height
                        + box_metrics.vertical_non_content_length().points()
                        + style.margin.top
                        + style.margin.bottom,
                )))
            }
            Some(ReplacedElementKind::Image) => used_image(
                element,
                &style,
                available_width,
                percentage_height_basis,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|image| {
                TableCellOuterBlockSize::new(layout_pt(
                    style.margin.top + image.border_box_size.height + style.margin.bottom,
                ))
            }),
            Some(ReplacedElementKind::Svg) => {
                used_svg(element, &style, available_width, percentage_height_basis).map(|svg| {
                    TableCellOuterBlockSize::new(layout_pt(
                        style.margin.top + svg.border_box_size.height + style.margin.bottom,
                    ))
                })
            }
            None => {
                self.table_cell_measured_inline_outer_height(child, stylesheets, available_width)
            }
        }
    }

    pub(in crate::layout::table) fn table_cell_row_minimum_atomic_inline_outer_height(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> f32 {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return 0.0;
        }
        let style =
            self.table_cell_content_sizing_style(style, TableCellContentSizingPolicy::RowMinimum);
        let nested_height = self.table_cell_children_non_text_content_height(
            children,
            stylesheets,
            available_width,
        );
        let vertical_non_content = non_content_pt(style.padding.top + style.padding.bottom)
            + table_vertical_borders(&style);
        let preferred_content_height = nested_height.max(style.line_height);
        let content_height = used_content_box_height_or_auto(
            &style,
            layout_pt(preferred_content_height),
            vertical_non_content,
        )
        .map(SemanticLengthExt::points)
        .unwrap_or(preferred_content_height)
        .max(nested_height);
        (content_height + vertical_non_content.points() + style.margin.top + style.margin.bottom)
            .max(0.0)
    }

    pub(in crate::layout::table) fn table_cell_content_sizing_style(
        &self,
        style: &ComputedStyle,
        policy: TableCellContentSizingPolicy,
    ) -> css::ZoomedLayoutStyle {
        let mut style = self.style_with_current_viewport_lengths(style);
        apply_table_cell_content_sizing_policy(&mut style, policy);
        style
    }

    /// Measure a document-canvas element when it appears as table-cell content.
    ///
    /// HTML's root/body canvas rules propagate backgrounds and root block
    /// sizing to the page canvas, but a `body` box inside an authored
    /// `html { display: table }` anonymous cell remains ordinary table-cell
    /// content for row sizing:
    /// <https://html.spec.whatwg.org/multipage/rendering.html#the-page>
    /// and <https://www.w3.org/TR/css-overflow-3/#scrollable>.
    pub(in crate::layout::table) fn table_cell_measured_document_canvas_child_height(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> f32 {
        let mut measured_style = style.clone();
        set_style_auto_height(&mut measured_style);
        measured_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
        measured_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;

        // The root/body canvas special case is an anonymous table-cell
        // wrapper, but its in-flow atomic children still form a line box.
        // Include that line sequence so table row sizing retains the baseline
        // descent below an `inline-block`, just like ordinary block layout.
        // <https://www.w3.org/TR/css-tables-3/#row-layout>
        let structural_content_height = self.table_cell_children_non_text_content_height(
            children,
            stylesheets,
            available_width,
        );
        let inline_sequence_height = self
            .table_cell_inline_sequence_height(
                &measured_style,
                children,
                stylesheets,
                available_width,
                PercentageBasis::indefinite(),
            )
            .map(TableCellOuterBlockSize::points)
            .unwrap_or(0.0);
        let text = inline_text_from_formatting_boxes(children);
        let text_height = if text.is_empty() {
            0.0
        } else {
            self.estimate_text_physical_height(
                &text,
                &measured_style,
                available_width,
                measured_style.padding.left,
                measured_style.padding.right,
            )
        };
        structural_content_height
            .max(inline_sequence_height)
            .max(text_height)
            + measured_style.padding.top
            + measured_style.padding.bottom
            + table_vertical_borders(&measured_style).points()
            + measured_style.margin.top
            + measured_style.margin.bottom
    }

    pub(in crate::layout::table) fn table_cell_content_bottom_baseline(
        &self,
        cell_style: &ComputedStyle,
        content_height: f32,
        border_insets: css::Edges,
    ) -> TableCellBaselineOffset {
        TableCellBaselineOffset::new(layout_pt(
            border_insets.top + cell_style.padding.top + content_height,
        ))
    }

    pub(in crate::layout::table) fn table_cell_baseline_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        border_insets: css::Edges,
    ) -> Option<TableCellBaselineOffset> {
        if cell
            .element
            .is_some_and(|element| layout_containment_applies_to_element(element, cell_style))
        {
            return None;
        }
        if let Some(children) = cell.children.as_deref() {
            return self
                .table_cell_children_first_baseline_offset(
                    children,
                    cell_style,
                    stylesheets,
                    available_width,
                )
                .map(|baseline| {
                    TableCellBaselineOffset::new(layout_pt(
                        border_insets.top + cell_style.padding.top + baseline.points(),
                    ))
                });
        }

        (!table_cell_inline_text(cell).is_empty()).then(|| {
            TableCellBaselineOffset::new(layout_pt(
                self.table_cell_first_baseline_offset(cell_style),
            ))
        })
    }

    pub(in crate::layout::table) fn table_cell_alignment_baseline_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        border_insets: css::Edges,
    ) -> Option<TableCellBaselineOffset> {
        match table_cell_alignment_baseline_set(cell_style) {
            TableCellBaselineSet::First => self.table_cell_baseline_offset(
                cell,
                cell_style,
                stylesheets,
                available_width,
                border_insets,
            ),
            TableCellBaselineSet::Last => self.table_cell_last_baseline_offset(
                cell,
                cell_style,
                stylesheets,
                available_width,
                border_insets,
            ),
        }
    }

    pub(in crate::layout::table) fn table_cell_last_baseline_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        border_insets: css::Edges,
    ) -> Option<TableCellBaselineOffset> {
        if cell
            .element
            .is_some_and(|element| layout_containment_applies_to_element(element, cell_style))
        {
            return None;
        }
        if let Some(children) = cell.children.as_deref() {
            return self
                .table_cell_children_baseline_offset(
                    children,
                    cell_style,
                    stylesheets,
                    available_width,
                    TableCellBaselineSet::Last,
                )
                .map(|baseline| {
                    TableCellBaselineOffset::new(layout_pt(
                        border_insets.top + cell_style.padding.top + baseline.points(),
                    ))
                });
        }

        if let Some(element) = cell.element
            && let Some(baseline) = self.table_cell_element_last_baseline_offset(
                element,
                cell_style,
                stylesheets,
                available_width,
                table_cell_href(cell),
            )
        {
            return Some(TableCellBaselineOffset::new(layout_pt(
                border_insets.top + cell_style.padding.top + baseline.points(),
            )));
        }

        let text = table_cell_inline_text(cell);
        (!text.is_empty()).then(|| {
            TableCellBaselineOffset::new(layout_pt(
                border_insets.top
                    + cell_style.padding.top
                    + self.table_cell_text_last_baseline_offset(&text, cell_style, available_width),
            ))
        })
    }

    pub(in crate::layout::table) fn table_cell_children_first_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        containing_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> Option<TableCellBaselineOffset> {
        self.table_cell_children_baseline_offset(
            children,
            containing_style,
            stylesheets,
            available_width,
            TableCellBaselineSet::First,
        )
    }

    pub(in crate::layout::table) fn table_cell_children_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        containing_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<TableCellBaselineOffset> {
        if formatting_boxes_have_textual_baseline(children)
            && !has_non_inline_formatting_box(children)
        {
            return self.table_cell_inline_content_baseline_offset(
                children,
                containing_style,
                stylesheets,
                available_width,
                baseline_set,
            );
        }

        let mut block_offset = 0.0_f32;
        let mut last_baseline = None;

        for child in children {
            if !table_cell_has_in_flow_layout_child(child) {
                continue;
            }
            let child_baseline = self.table_cell_child_baseline_offset(
                child,
                stylesheets,
                available_width,
                baseline_set,
            );
            if let Some(baseline) = child_baseline {
                let baseline =
                    TableCellBaselineOffset::new(layout_pt(block_offset + baseline.points()));
                if baseline_set == TableCellBaselineSet::First {
                    return Some(baseline);
                }
                last_baseline = Some(baseline);
            }
            block_offset += table_cell_formatting_child_outer_height(child).points();
        }

        last_baseline
    }

    pub(in crate::layout::table) fn table_cell_child_baseline_offset(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<TableCellBaselineOffset> {
        match child {
            box_tree::FormattingBox::Text(box_) => {
                (!box_tree::formatting_box_is_collapsible_space(child)).then(|| {
                    TableCellBaselineOffset::new(layout_pt(self.table_cell_text_baseline_offset(
                        &box_.text,
                        &box_.style,
                        available_width,
                        baseline_set,
                    )))
                })
            }
            box_tree::FormattingBox::Inline(box_) => self.inline_children_baseline_offset(
                &box_.core.children,
                &box_.core.style,
                stylesheets,
                available_width,
                baseline_set,
            ),
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .table_cell_children_baseline_offset(
                    &box_.children,
                    &box_.style,
                    stylesheets,
                    available_width,
                    baseline_set,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .table_cell_children_baseline_offset(
                    &box_.core.children,
                    &box_.core.style,
                    stylesheets,
                    available_width,
                    baseline_set,
                ),
            box_tree::FormattingBox::Block(box_) => self.block_child_baseline_offset(
                &box_.core.style,
                &box_.core.children,
                stylesheets,
                available_width,
                baseline_set,
            ),
            box_tree::FormattingBox::Flex(box_) => self.block_child_baseline_offset(
                &box_.core.style,
                &box_.core.children,
                stylesheets,
                available_width,
                baseline_set,
            ),
            box_tree::FormattingBox::Table(box_) => self
                .table_fragment_baseline_offset(
                    box_.core.element,
                    &box_.core.style,
                    &box_.fragment,
                    stylesheets,
                    available_width,
                )
                .map(|baseline| {
                    TableCellBaselineOffset::new(layout_pt(
                        box_.core.style.margin.top + baseline.points(),
                    ))
                }),
            box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_) => None,
        }
    }

    pub(in crate::layout::table) fn inline_children_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        inline_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<TableCellBaselineOffset> {
        // CSS table-cell baselines come from in-flow line-box baselines; an
        // inline wrapper that only contains atomic/replaced content should
        // expose no textual baseline so the cell can fall back to its bottom
        // content edge.
        // <https://www.w3.org/TR/CSS22/tables.html#height-layout>
        if !formatting_boxes_have_textual_baseline(children) {
            return None;
        }

        self.table_cell_inline_content_baseline_offset(
            children,
            inline_style,
            stylesheets,
            available_width,
            baseline_set,
        )
    }

    pub(in crate::layout::table) fn block_child_baseline_offset(
        &mut self,
        block_style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<TableCellBaselineOffset> {
        if matches!(block_style.position, Position::Absolute | Position::Fixed) {
            return None;
        }
        let borders = used_border_widths(block_style);
        self.table_cell_children_baseline_offset(
            children,
            block_style,
            stylesheets,
            available_width,
            baseline_set,
        )
        .map(|baseline| {
            TableCellBaselineOffset::new(layout_pt(
                block_style.margin.top + borders.top + block_style.padding.top + baseline.points(),
            ))
        })
    }

    pub(in crate::layout::table) fn table_cell_inline_content_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<TableCellBaselineOffset> {
        if !formatting_box_has_inline_content(children) {
            return None;
        }

        let mut items = Vec::new();
        self.with_table_cell_inline_planning_scope(
            style,
            available_width,
            PercentageBasis::indefinite(),
            |layout| {
                layout.collect_inline_box_items(
                    children,
                    stylesheets,
                    None,
                    0.0,
                    InlineVisualOffset::zero(),
                    style,
                    style.text_decoration_origins.effective_layers_vec(),
                    &mut items,
                );
            },
        );
        if items.is_empty() {
            return None;
        }
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_width.max(1.0),
            0.0,
            0.0,
        );
        if sequence.line_count() == 0 {
            return None;
        }

        let baseline_style = if table_cell_textual_children_match_baseline_style(children, style) {
            style
        } else {
            table_cell_textual_baseline_style(children, baseline_set).unwrap_or(style)
        };
        let first_baseline = self
            .inline_text_box_metrics(baseline_style, 0.0)
            .line_baseline_offset;
        Some(TableCellBaselineOffset::new(layout_pt(
            match baseline_set {
                TableCellBaselineSet::First => {
                    table_cell_inline_sequence_first_baseline_offset(&sequence)
                        .map(TableCellBaselineOffset::points)
                        .unwrap_or(first_baseline)
                }
                TableCellBaselineSet::Last => {
                    table_cell_inline_sequence_last_baseline_offset(&sequence)
                        .map(TableCellBaselineOffset::points)
                        .unwrap_or(first_baseline)
                }
            },
        )))
    }

    pub(in crate::layout::table) fn table_fragment_baseline_offset(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        fragment: &box_tree::TableFragment<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> Option<TableCellBaselineOffset> {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return None;
        }

        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.row_ordering.rows.as_slice();
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value))
            .map(|value| TableCellPadding::new(layout_pt(value)));
        let table_metrics = table_metrics(element, style);
        let grid = table_grid(rows);
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    &input.columns,
                    grid.column_count,
                )
            });
        let table_width = used_table_width(
            style,
            available_width.max(style.font_size),
            collapsed_geometry
                .as_ref()
                .map(|geometry| geometry.outer_insets),
        );
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            LogicalInlineContentSize::new(table_width.content_width),
            !style.box_values.width.is_auto(),
            table_cellpadding,
            table_metrics.clone(),
            collapsed_geometry.as_ref(),
        );
        let top_caption_height = self.estimate_table_captions_height(
            &input.captions,
            style,
            stylesheets,
            PhysicalContentWidth::new(content_box_pt(column_plan.total_width().points())),
            CaptionSide::Top,
        );
        let table_used_style = self.table_used_style(style);
        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: &table_used_style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics: table_metrics.clone(),
            collapsed_geometry: collapsed_geometry.as_ref(),
            wrapper_border_box_block_size: None,
            positioned_table_block_content_size: None,
            wrapper_non_grid_block_size: layout_pt(0.0),
        };

        let Some(row_index) = rows.iter().enumerate().find_map(|(index, row)| {
            let row_style = self.style_for_table_row(row, style, stylesheets);
            if table_row_is_collapsed(&row_style) || grid.rows[index].is_empty() {
                return None;
            }
            if self.table_row_is_hidden_empty(
                row,
                &grid.rows[index],
                &row_style,
                stylesheets,
                table_cellpadding,
                &column_plan,
                table_metrics.clone(),
            ) {
                return None;
            }
            Some(index)
        }) else {
            return Some(TableCellBaselineOffset::new(layout_pt(
                table_width.border_widths.top + table_width.padding.top + top_caption_height,
            )));
        };

        let row_style = self.style_for_table_row(&rows[row_index], style, stylesheets);
        let row_baseline = self
            .table_row_baseline_only_offset(
                row_index,
                &rows[row_index],
                &grid.rows[row_index],
                &row_style,
                stylesheets,
                table_cellpadding,
                &column_plan,
                table_metrics.clone(),
                collapsed_geometry.as_ref(),
            )
            .map(TableRowBaselineOffset::points)
            .unwrap_or_else(|| {
                self.measure_table_row_height(&table_context, row_index, &row_style)
            });

        Some(TableCellBaselineOffset::new(layout_pt(
            top_caption_height
                + table_width.border_widths.top
                + table_width.padding.top
                + table_metrics.spacing.vertical.length_points()
                + row_baseline,
        )))
    }

    /// Returns the first rendered text baseline offset from a table cell border-box top.
    ///
    /// CSS 2.2 aligns `vertical-align: baseline` table cells by the baseline of
    /// their first in-flow line box. Text painting applies the selected font's
    /// ascender correction, so table layout must use the same metric:
    /// <https://www.w3.org/TR/CSS22/tables.html#height-layout>.
    pub(in crate::layout::table) fn table_cell_first_baseline_offset(
        &mut self,
        style: &ComputedStyle,
    ) -> f32 {
        let borders = used_border_widths(style);
        borders.top
            + style.padding.top
            + self
                .inline_text_box_metrics(style, 0.0)
                .line_baseline_offset
    }

    pub(in crate::layout::table) fn table_cell_text_baseline_offset(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> f32 {
        match baseline_set {
            TableCellBaselineSet::First => {
                self.inline_text_box_metrics(style, 0.0)
                    .line_baseline_offset
            }
            TableCellBaselineSet::Last => {
                self.table_cell_text_last_baseline_offset(text, style, available_width)
            }
        }
    }

    pub(in crate::layout::table) fn table_cell_text_last_baseline_offset(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> f32 {
        if !text.is_empty() {
            let sequence = self.inline_line_sequence_for_raw_inline_text(
                text,
                style,
                available_width,
                0.0,
                None,
            );
            if let Some(baseline) = table_cell_inline_sequence_last_baseline_offset(&sequence) {
                return baseline.points();
            }
        }
        self.inline_text_box_metrics(style, 0.0)
            .line_baseline_offset
    }

    fn table_cell_element_last_baseline_offset(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        link_target: Option<&str>,
    ) -> Option<TableCellBaselineOffset> {
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        self.with_table_cell_inline_planning_scope(
            style,
            available_width,
            PercentageBasis::indefinite(),
            |layout| {
                layout.push_generated_pseudo_items(
                    element,
                    style,
                    style.before_style.as_deref(),
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    GeneratedPseudoCounterMode::Commit,
                    &mut items,
                );
                layout.collect_element_content_or_inline_items(
                    element,
                    style,
                    stylesheets,
                    link_target.clone(),
                    InlinePlacement::zero(),
                    &mut items,
                );
                layout.push_generated_pseudo_items(
                    element,
                    style,
                    style.after_style.as_deref(),
                    link_target,
                    0.0,
                    InlineVisualOffset::zero(),
                    GeneratedPseudoCounterMode::Commit,
                    &mut items,
                );
            },
        );
        if items.is_empty() {
            return None;
        }
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_width.max(1.0),
            0.0,
            0.0,
        );
        table_cell_inline_sequence_last_baseline_offset(&sequence)
    }

    /// Measure text content height for a table cell using durable child styles.
    ///
    /// CSS Tables 3 computes row heights from cell content, while
    /// `display: contents` can place inherited raw text boxes inside anonymous
    /// table cells whose generated cell style is not the text style:
    /// <https://drafts.csswg.org/css-tables-3/#row-layout> and
    /// <https://www.w3.org/TR/css-display-3/#valdef-display-contents>.
    pub(in crate::layout::table) fn table_cell_text_content_height(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        available_width: f32,
    ) -> f32 {
        if let Some(children) = cell.children.as_deref() {
            return self.table_cell_children_text_content_height(children, available_width);
        }

        let text = table_cell_inline_text(cell);
        if text.is_empty() {
            0.0
        } else {
            self.estimate_text_physical_height(&text, cell_style, available_width, 0.0, 0.0)
        }
    }

    pub(in crate::layout::table) fn table_cell_children_text_content_height(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        available_width: f32,
    ) -> f32 {
        let mut height = 0.0_f32;
        let mut inline_line_height = 0.0_f32;

        for child in children {
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    inline_line_height =
                        inline_line_height.max(self.estimate_text_physical_height(
                            &box_.text,
                            &box_.style,
                            available_width,
                            0.0,
                            0.0,
                        ));
                }
                box_tree::FormattingBox::Inline(box_) => {
                    inline_line_height =
                        inline_line_height.max(self.table_cell_children_text_content_height(
                            &box_.core.children,
                            available_width,
                        ));
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    if inline_line_height > 0.0 {
                        height += inline_line_height;
                        inline_line_height = 0.0;
                    }
                    height += self
                        .table_cell_children_text_content_height(&box_.children, available_width);
                }
                _ => {
                    if inline_line_height > 0.0 {
                        height += inline_line_height;
                        inline_line_height = 0.0;
                    }
                }
            }
        }

        height + inline_line_height
    }

    /// Return a content clip for cells spanning collapsed row tracks.
    ///
    /// CSS Tables 3 says cells crossing collapsed rows are rendered as though
    /// the content in the collapsed track is clipped away while the cell still
    /// participates in the remaining visible rows:
    /// <https://drafts.csswg.org/css-tables-3/#visibility-collapse-cell-rendering>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn collapsed_rowspan_cell_content_clip(
        &mut self,
        row_index: usize,
        rowspan: usize,
        rows: &[TableRow<'_>],
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        row_heights: &[f32],
        source_row_heights: &[f32],
        row_occupancy: &[bool],
        table_metrics: TableMetrics,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
    ) -> Option<TableCellClipRegion> {
        if rowspan <= 1 {
            return None;
        }
        let end = (row_index + rowspan).min(rows.len());
        let collapsed_rows = rows[row_index..end]
            .iter()
            .map(|row| {
                table_row_is_collapsed(&self.style_for_table_row(row, table_style, stylesheets))
            })
            .collect::<Vec<_>>();
        let spans_collapsed_row = collapsed_rows.iter().skip(1).any(|collapsed| *collapsed);
        if !spans_collapsed_row {
            return None;
        }

        // Keep source track advances even where visibility collapse removes a
        // row from final painting. The spanning cell's descendants are laid
        // out in that source space; each surviving row contributes one clip
        // rectangle and collapsed tracks become actual holes.
        let mut source_start = 0.0;
        let mut regions = Vec::new();
        for (offset, collapsed) in collapsed_rows.iter().copied().enumerate() {
            let index = row_index + offset;
            let source_height = source_row_heights
                .get(index)
                .copied()
                .unwrap_or_else(|| row_heights.get(index).copied().unwrap_or(0.0));
            let visible = row_occupancy.get(index).copied().unwrap_or(!collapsed);
            if visible && source_height > 0.0 {
                let rect = TableGridRect::new(
                    TableGridPoint::from_lengths(
                        TableGridLength::new(border_box.rect().origin.x),
                        TableGridLength::new(border_box.rect().origin.y + source_start),
                    ),
                    TableGridSize::from_lengths(
                        border_box.logical_inline_size(),
                        TableGridLength::new(source_height),
                    ),
                );
                regions.push(placement.overflow_clip_for(rect));
            }
            source_start += source_height;
            if offset + 1 < end - row_index {
                source_start += table_metrics.spacing.vertical.length_points();
            }
        }
        TableCellClipRegion::from_clips(regions)
    }

    /// Return the padding-edge clip established by overflow or paint
    /// containment on a table cell.
    ///
    /// A table cell's used height is determined by row layout rather than by
    /// its specified height. Both CSS Overflow and paint containment clip at
    /// the resulting padding edge after the table border model resolves cell
    /// borders.  The CSS overflow axes stay physical at this final paint
    /// boundary, matching ordinary block layout:
    /// <https://www.w3.org/TR/CSS22/tables.html#height-layout>
    /// <https://www.w3.org/TR/css-contain-1/#containment-paint>.
    pub(in crate::layout::table) fn table_cell_content_clip(
        &self,
        cell_element: Option<&Element>,
        cell_style: &ComputedStyle,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        cell_borders: css::Edges,
    ) -> Option<OverflowClip> {
        let padding_box = placement.containing_block_for(border_box, cell_borders);
        // CSS table cells grow their used row height for normal in-flow
        // content before overflow is applied.  Every non-visible overflow
        // value then clips descendants at that resulting padding edge;
        // `hidden`, `auto`, and `scroll` remain scrollable while `clip` does
        // not.  Do not turn this final paint/layout clip into a shorter row
        // sizing constraint.
        // <https://drafts.csswg.org/css-tables-3/#table-height-algorithm>
        // <https://drafts.csswg.org/css-overflow-3/#overflow-clipping>
        let paint_containment_applies = cell_element
            .is_some_and(|element| paint_containment_applies_to_element(element, cell_style));
        let clip_axes =
            TableCellOverflowClipAxes::from_style(cell_style, paint_containment_applies);
        if !clip_axes.clips_any_axis() {
            return None;
        }
        let rect = padding_box.rect;
        Some(OverflowClip::from_paint_rect_with_axes_and_non_scrollable(
            PageTopRect::new(
                rect.x(),
                rect.top_y(),
                rect.width().max(0.0),
                rect.height().max(0.0),
            )
            .paint_rect(),
            clip_axes.clips_x,
            clip_axes.clips_y,
            clip_axes.non_scrollable_x,
            clip_axes.non_scrollable_y,
        ))
    }

    pub(in crate::layout::table) fn collapsed_table_geometry(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        columns: &[TableColumn<'_>],
        column_count: usize,
    ) -> CollapsedTableGeometry {
        let collapsed_columns =
            self.collapsed_table_columns(columns, table_style, stylesheets, column_count);
        let mut collapsed_grid =
            CollapsedBorderGrid::new(rows.len(), column_count, TableAxes::for_style(table_style));
        collapsed_grid.add_table(table_style, rows.len(), column_count);
        for (row_index, row) in rows.iter().enumerate() {
            let row_style = self.style_for_table_row(row, table_style, stylesheets);
            let row_part_style = TablePartUsedStyle::from_table_used(row_style.clone());
            if table_row_is_collapsed(row_part_style.layout()) {
                continue;
            }
            for placement in &grid.rows[row_index] {
                if placement.column >= column_count {
                    break;
                }
                let cell = &row.cells[placement.cell];
                let cell_style = self.style_for_table_cell(cell, row, &row_style, stylesheets);
                collapsed_grid.add_cell(
                    row_index,
                    placement.column,
                    placement.colspan,
                    placement.rowspan,
                    &cell_style,
                );
            }
            if let Some(participant) = row_part_style.collapsed_border_participant() {
                collapsed_grid.add_row(row_index, column_count, participant.style());
            }
        }
        for (start_row, end_row, row_group) in table_row_group_spans(rows) {
            let row_group_style =
                self.style_for_table_row_group(&row_group, table_style, stylesheets);
            let row_group_part_style = TablePartUsedStyle::from_table_used(row_group_style);
            if let Some(participant) = row_group_part_style.collapsed_border_participant() {
                collapsed_grid.add_row_group(start_row, end_row, column_count, participant.style());
            }
        }
        for (start_column, end_column, column_group) in
            table_column_group_spans(columns, column_count)
        {
            let Some((visible_start, visible_end)) =
                visible_column_span(start_column, end_column, &collapsed_columns)
            else {
                continue;
            };
            let column_group_style =
                self.style_for_table_column_group(&column_group, table_style, stylesheets);
            collapsed_grid.add_column_group(
                visible_start,
                visible_end,
                rows.len(),
                &column_group_style,
            );
        }
        let mut column_index = 0;
        for column in columns {
            if column_index >= column_count {
                break;
            }
            let span = column.span.min(column_count - column_index).max(1);
            let start_column = column_index;
            let end_column = column_index + span;
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            if let Some((visible_start, visible_end)) =
                visible_column_span(start_column, end_column, &collapsed_columns)
            {
                collapsed_grid.add_column(visible_start, visible_end, rows.len(), &column_style);
            }
            column_index += span;
        }

        let outer_insets = collapsed_grid.outer_insets();
        CollapsedTableGeometry {
            grid: collapsed_grid,
            outer_insets,
        }
    }
}

fn table_cell_inline_sequence_last_baseline_offset(
    sequence: &inline_layout::InlineLineSequence,
) -> Option<TableCellBaselineOffset> {
    let records = sequence.fragment_records_for_paint(0, sequence.records.len());
    let mut block_offset = 0.0;
    let mut last_baseline = None;
    for record in &records {
        if let Some(fragment) = &record.fragment {
            last_baseline = Some(TableCellBaselineOffset::new(layout_pt(
                block_offset + record.block_start_trim + fragment.metrics.baseline_offset,
            )));
        }
        block_offset += record.height();
    }
    last_baseline
}

fn table_cell_inline_sequence_first_baseline_offset(
    sequence: &inline_layout::InlineLineSequence,
) -> Option<TableCellBaselineOffset> {
    let records = sequence.fragment_records_for_paint(0, sequence.records.len());
    records.iter().find_map(|record| {
        record.fragment.as_ref().map(|fragment| {
            TableCellBaselineOffset::new(layout_pt(
                record.block_start_trim + fragment.metrics.baseline_offset,
            ))
        })
    })
}

/// Axis-specific overflow clipping established by a table cell's final used
/// padding box.
///
/// Table row sizing is complete before this value is used.  It therefore
/// describes only descendant clipping, never a substitute cell-size
/// constraint.
/// <https://drafts.csswg.org/css-overflow-3/#overflow-clipping>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TableCellOverflowClipAxes {
    clips_x: bool,
    clips_y: bool,
    non_scrollable_x: bool,
    non_scrollable_y: bool,
}

impl TableCellOverflowClipAxes {
    fn from_style(cell_style: &ComputedStyle, paint_containment_applies: bool) -> Self {
        let used_overflow = UsedOverflowAxes::from_style(cell_style);
        Self {
            clips_x: used_overflow.clips_x() || paint_containment_applies,
            clips_y: used_overflow.clips_y() || paint_containment_applies,
            non_scrollable_x: used_overflow.non_scrollable_clip_x(),
            non_scrollable_y: used_overflow.non_scrollable_clip_y(),
        }
    }

    fn clips_any_axis(self) -> bool {
        self.clips_x || self.clips_y
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::RenderOptions;
    use crate::css::{Direction, TextBoxEdge, TextBoxTrim, TextEdgeMetric, TextEdgePair};
    use crate::layout::LayoutBuilderConfig;
    use crate::resource::ResourceCache;
    use crate::text::FontSystem;

    fn test_layout_builder<'a, Collection: crate::css::StylesheetCollection + ?Sized>(
        options: &'a RenderOptions,
        stylesheets: &'a Collection,
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
        let stylesheets = crate::css::StylesheetCollection::stylesheet_view(stylesheets);
        LayoutBuilder::new(LayoutBuilderConfig {
            options,
            stylesheets,
            base_url: None,
            root_url: None,
            resource_cache,
            // The builder retains this reference for its lifetime; tests that do
            // not exercise iframes use one immutable empty fixture.
            iframe_documents: Box::leak(Box::new(HashMap::new())),
            iframe_viewport: None,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            target_references: crate::layout::TargetReferenceSnapshot::default(),
            font_system: FontSystem::new(),
        })
    }

    #[test]
    fn table_cell_hidden_and_clip_keep_their_distinct_overflow_axes() {
        let mut hidden = ComputedStyle::initial();
        hidden.overflow_x = css::Overflow::Hidden;
        hidden.overflow_y = css::Overflow::Hidden;
        assert_eq!(
            TableCellOverflowClipAxes::from_style(&hidden, false),
            TableCellOverflowClipAxes {
                clips_x: true,
                clips_y: true,
                non_scrollable_x: false,
                non_scrollable_y: false,
            }
        );

        let mut clip = ComputedStyle::initial();
        clip.overflow_x = css::Overflow::Clip;
        clip.overflow_y = css::Overflow::Clip;
        assert_eq!(
            TableCellOverflowClipAxes::from_style(&clip, false),
            TableCellOverflowClipAxes {
                clips_x: true,
                clips_y: true,
                non_scrollable_x: true,
                non_scrollable_y: true,
            }
        );
    }

    #[test]
    fn table_cell_text_last_baseline_offset_uses_trimmed_preceding_line_height() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 50.0;
        style.line_height = 100.0;
        style.text_box_edge = TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Cap,
            TextEdgeMetric::Alphabetic,
        ));

        let text = "A B";
        let available_width = 30.0;
        let untrimmed = builder.table_cell_text_last_baseline_offset(text, &style, available_width);
        style.text_box_trim = TextBoxTrim::TrimStart;
        let trimmed = builder.table_cell_text_last_baseline_offset(text, &style, available_width);

        assert!(
            untrimmed > trimmed + 1.0,
            "table-cell last-baseline offset should use trimmed preceding line height: untrimmed={untrimmed}, trimmed={trimmed}"
        );
    }
}
