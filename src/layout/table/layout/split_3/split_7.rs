use super::*;
use crate::layout::inline_collect::InlinePlacement;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::table) fn table_cell_measured_inline_outer_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> Option<f32> {
        if !table_cell_formatting_child_has_parent_percentage_block_size(child) {
            return table_cell_measured_inline_outer_height_without_policy(child, available_width);
        }
        match child {
            box_tree::FormattingBox::Inline(box_) => {
                if matches!(
                    box_.core.style.position,
                    Position::Absolute | Position::Fixed
                ) {
                    Some(0.0)
                } else {
                    Some(table_cell_formatting_child_outer_height(child).points())
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
                Some(table_cell_canvas_first_pass_outer_height(
                    box_.core.element,
                    &style,
                    available_width,
                ))
            }
            box_tree::FormattingBox::Replaced(box_)
                if replaced_element_kind(box_.core.element)
                    == Some(ReplacedElementKind::Canvas) =>
            {
                let style = self.table_cell_content_sizing_style(
                    &box_.core.style,
                    TableCellContentSizingPolicy::RowMinimum,
                );
                Some(table_cell_canvas_first_pass_outer_height(
                    box_.core.element,
                    &style,
                    available_width,
                ))
            }
            box_tree::FormattingBox::AtomicInline(box_) => {
                Some(self.table_cell_row_minimum_atomic_inline_outer_height(
                    &box_.core.style,
                    &box_.core.children,
                    stylesheets,
                    available_width,
                ))
            }
            box_tree::FormattingBox::Replaced(box_) => {
                Some(self.table_cell_row_minimum_atomic_inline_outer_height(
                    &box_.core.style,
                    &box_.core.children,
                    stylesheets,
                    available_width,
                ))
            }
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
        stylesheets: &[Stylesheet],
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> Option<f32> {
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
        if replaced_element_kind(element) != Some(ReplacedElementKind::Canvas) {
            return self.table_cell_measured_inline_outer_height(
                child,
                stylesheets,
                available_width,
            );
        }

        let mut style = self
            .table_cell_content_sizing_style(style, TableCellContentSizingPolicy::FinalRelayout);
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
        Some(
            height
                + box_metrics.vertical_non_content_length().points()
                + style.margin.top
                + style.margin.bottom,
        )
    }

    pub(in crate::layout::table) fn table_cell_row_minimum_atomic_inline_outer_height(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
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
    ) -> ComputedStyle {
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
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        let mut measured_style = style.clone();
        set_style_auto_height(&mut measured_style);
        measured_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
        measured_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;

        // The root/body canvas special case is an anonymous table-cell
        // wrapper, not a text run. Its row contribution is the contained
        // boxes' outer geometry; appending inherited line descent beneath an
        // atomic inline would make `html { display: table }` taller than its
        // shrink-wrapped contents.
        // <https://www.w3.org/TR/css-tables-3/#row-layout>
        let structural_content_height = self.table_cell_children_non_text_content_height(
            children,
            stylesheets,
            available_width,
        );
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
        structural_content_height.max(text_height)
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
    ) -> f32 {
        border_insets.top + cell_style.padding.top + content_height
    }

    pub(in crate::layout::table) fn table_cell_baseline_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        border_insets: css::Edges,
    ) -> Option<f32> {
        if cell_style.contain.layout {
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
                .map(|baseline| border_insets.top + cell_style.padding.top + baseline);
        }

        (!table_cell_inline_text(cell).is_empty())
            .then(|| self.table_cell_first_baseline_offset(cell_style))
    }

    pub(in crate::layout::table) fn table_cell_alignment_baseline_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        border_insets: css::Edges,
    ) -> Option<f32> {
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
        stylesheets: &[Stylesheet],
        available_width: f32,
        border_insets: css::Edges,
    ) -> Option<f32> {
        if cell_style.contain.layout {
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
                .map(|baseline| border_insets.top + cell_style.padding.top + baseline);
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
            return Some(border_insets.top + cell_style.padding.top + baseline);
        }

        let text = table_cell_inline_text(cell);
        (!text.is_empty()).then(|| {
            border_insets.top
                + cell_style.padding.top
                + self.table_cell_text_last_baseline_offset(&text, cell_style, available_width)
        })
    }

    pub(in crate::layout::table) fn table_cell_children_first_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        containing_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> Option<f32> {
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
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
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
                let baseline = block_offset + baseline;
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
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
        match child {
            box_tree::FormattingBox::Text(box_) => {
                (!box_tree::formatting_box_is_collapsible_space(child)).then(|| {
                    self.table_cell_text_baseline_offset(
                        &box_.text,
                        &box_.style,
                        available_width,
                        baseline_set,
                    )
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
                .map(|baseline| box_.core.style.margin.top + baseline),
            box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_) => None,
        }
    }

    pub(in crate::layout::table) fn inline_children_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        inline_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
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
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
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
        .map(|baseline| block_style.margin.top + borders.top + block_style.padding.top + baseline)
    }

    pub(in crate::layout::table) fn table_cell_inline_content_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
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
                    style.text_decoration.clone(),
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
            .inline_text_box_metrics(baseline_style, None, 0.0)
            .line_baseline_offset;
        Some(match baseline_set {
            TableCellBaselineSet::First => {
                table_cell_inline_sequence_first_baseline_offset(&sequence)
                    .unwrap_or(first_baseline)
            }
            TableCellBaselineSet::Last => {
                table_cell_inline_sequence_last_baseline_offset(&sequence).unwrap_or(first_baseline)
            }
        })
    }

    pub(in crate::layout::table) fn table_fragment_baseline_offset(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        fragment: &box_tree::TableFragment<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> Option<f32> {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return None;
        }

        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let table_width = used_table_width(style, available_width.max(style.font_size));
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
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
            return Some(
                table_width.border_widths.top + table_width.padding.top + top_caption_height,
            );
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
            .unwrap_or_else(|| {
                self.measure_table_row_height(&table_context, row_index, &row_style)
            });

        Some(
            top_caption_height
                + table_width.border_widths.top
                + table_width.padding.top
                + table_metrics.spacing.vertical.length_points()
                + row_baseline,
        )
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
                .inline_text_box_metrics(style, None, 0.0)
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
                self.inline_text_box_metrics(style, None, 0.0)
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
                return baseline;
            }
        }
        self.inline_text_box_metrics(style, None, 0.0)
            .line_baseline_offset
    }

    fn table_cell_element_last_baseline_offset(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        link_target: Option<&str>,
    ) -> Option<f32> {
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
        stylesheets: &[Stylesheet],
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
                        TableGridLength::new(border_box.width()),
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
        cell_style: &ComputedStyle,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        cell_borders: css::Edges,
    ) -> Option<OverflowClip> {
        let padding_box = placement.containing_block_for(border_box, cell_borders);
        // CSS table cells grow their used row height for normal in-flow
        // content. Unlike ordinary blocks, `hidden` and `clip` therefore do
        // not establish a shorter used cell scrollport merely because an
        // author supplied `height` or `max-height`; `auto`/`scroll` retain
        // their table-cell scrollport behavior after row sizing.
        // <https://drafts.csswg.org/css-tables-3/#table-height-algorithm>
        let (overflow_x, overflow_y) = resolved_overflow_axes(cell_style);
        let clips_scrollport_x = matches!(overflow_x, css::Overflow::Auto | css::Overflow::Scroll);
        let clips_scrollport_y = matches!(overflow_y, css::Overflow::Auto | css::Overflow::Scroll);
        if !(clips_scrollport_x || clips_scrollport_y || cell_style.contain.paint) {
            return None;
        }
        let rect = padding_box.rect;
        Some(
            OverflowClip::from_page_top_rect(PageTopRect::new(
                rect.x(),
                rect.top_y(),
                rect.width().max(0.0),
                rect.height().max(0.0),
            ))
            .with_axes(
                clips_scrollport_x || cell_style.contain.paint,
                clips_scrollport_y || cell_style.contain.paint,
            ),
        )
    }

    pub(in crate::layout::table) fn layout_table_captions(
        &mut self,
        captions: &[TableCaption<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        table_width: f32,
        side: CaptionSide,
    ) {
        for caption in captions {
            let mut caption_style = self.style_for_table_caption(caption, table_style, stylesheets);
            if caption_style.caption_side != side || caption_style.display.is_none() {
                continue;
            }
            let caption_available_width = if has_auto_width(&caption_style) {
                // An auto-width caption uses the table measure for its outer
                // border box. `width` itself sizes the content box, so remove
                // the caption's padding and borders before freezing that used
                // value; otherwise thick caption borders spuriously widen the
                // table wrapper.
                // <https://www.w3.org/TR/CSS22/tables.html#model>
                let horizontal_non_content = caption_style.padding.left
                    + caption_style.padding.right
                    + horizontal_border_width(&caption_style);
                set_style_used_width(
                    &mut caption_style,
                    (table_width - horizontal_non_content).max(0.0),
                );
                if caption_style.contain.size
                    && caption_style.writing_mode == WritingMode::HorizontalTb
                {
                    // Size containment fixes the caption's principal used
                    // size independently of its descendants. Those
                    // descendants still format as visual overflow, anchored
                    // at the principal border edge rather than after a
                    // zero-extent side border. Preserve the same outer table
                    // measure by transferring that start inset from the
                    // internal overflow origin to the used content width.
                    // <https://www.w3.org/TR/css-contain-1/#containment-size>
                    let start_border = used_border_widths(&caption_style).left;
                    if start_border != 0.0 {
                        // The principal block has zero used block extent, so
                        // this side has no visible block-axis edge. Removing
                        // it from descendant layout exposes the border-box
                        // overflow origin without changing the painted top
                        // and bottom edges.
                        // `style_with_current_used_lengths` resolves the
                        // durable border length values again before block
                        // geometry is built. Keep that source value in sync
                        // with this temporary used-edge adjustment instead of
                        // letting late font-metric resolution restore the
                        // authored left border for descendant overflow.
                        caption_style.border_width_values.left =
                            css::ComputedLengthPercentage::from_points(0.0);
                        caption_style.border_widths.left = 0.0;
                        set_style_used_width(
                            &mut caption_style,
                            (table_width - horizontal_non_content + start_border).max(0.0),
                        );
                    }
                }
                table_width
            } else {
                let horizontal_non_content = caption_style.padding.left
                    + caption_style.padding.right
                    + horizontal_border_width(&caption_style);
                let caption_content_width = used_content_box_width_or_auto(
                    &caption_style,
                    layout_pt(table_width),
                    non_content_pt(horizontal_non_content),
                )
                .map(SemanticLengthExt::points)
                .unwrap_or(table_width);
                table_width.max(
                    caption_style.margin.left
                        + caption_content_width
                        + horizontal_non_content
                        + caption_style.margin.right,
                )
            };
            let previous_left = self.content_left;
            let previous_right = self.content_right;
            self.content_left = table_x;
            self.content_right = table_x + caption_available_width;
            self.push_float_context();
            if let Some(children) = caption.children.as_deref() {
                self.layout_element_box(
                    caption.element,
                    &caption_style,
                    stylesheets,
                    caption.signature.clone(),
                    &box_tree::BoxSource::Principal,
                    &[],
                    children,
                );
            } else {
                self.layout_element(caption.element, &caption_style, stylesheets);
            }
            self.pop_float_context();
            self.content_left = previous_left;
            self.content_right = previous_right;
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Resolve the full collapsed-border geometry for one table grid.
    ///
    /// CSS 2.2 collapsed border conflict resolution produces a single grid of
    /// winning borders. The table wrapper consumes the outer half-widths, and
    /// fragmented paint later samples the same full grid.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    pub(in crate::layout::table) fn collapsed_table_geometry(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        columns: &[TableColumn<'_>],
        column_count: usize,
    ) -> CollapsedTableGeometry {
        let collapsed_columns =
            self.collapsed_table_columns(columns, table_style, stylesheets, column_count);
        let mut collapsed_grid =
            CollapsedBorderGrid::new(rows.len(), column_count, table_style.used_direction());
        collapsed_grid.add_table(table_style, rows.len(), column_count);
        for (row_index, row) in rows.iter().enumerate() {
            let row_style = self.style_for_table_row(row, table_style, stylesheets);
            if table_row_is_collapsed(&row_style) {
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
            collapsed_grid.add_row(row_index, column_count, &row_style);
        }
        for (start_row, end_row, row_group) in table_row_group_spans(rows) {
            let row_group_style =
                self.style_for_table_row_group(&row_group, table_style, stylesheets);
            collapsed_grid.add_row_group(start_row, end_row, column_count, &row_group_style);
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
) -> Option<f32> {
    let records = sequence.fragment_records_for_paint(0, sequence.records.len());
    let mut block_offset = 0.0;
    let mut last_baseline = None;
    for record in &records {
        if let Some(fragment) = &record.fragment {
            last_baseline =
                Some(block_offset + record.block_start_trim + fragment.metrics.baseline_offset);
        }
        block_offset += record.height();
    }
    last_baseline
}

fn table_cell_inline_sequence_first_baseline_offset(
    sequence: &inline_layout::InlineLineSequence,
) -> Option<f32> {
    let records = sequence.fragment_records_for_paint(0, sequence.records.len());
    records.iter().find_map(|record| {
        record
            .fragment
            .as_ref()
            .map(|fragment| record.block_start_trim + fragment.metrics.baseline_offset)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{TextBoxEdge, TextBoxTrim, TextEdgeMetric, TextEdgePair};
    use std::collections::HashMap;

    fn test_layout_builder<'a>(
        options: &'a RenderOptions,
        stylesheets: &'a [Stylesheet],
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
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
            font_system: FontSystem::new(),
        })
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
