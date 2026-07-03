use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Return min-content and max-content grid widths for a durable table fragment.
    ///
    /// CSS Tables computes intrinsic table widths from the row/column grid and
    /// cell min/max-content measures. Reusing the durable fragment keeps
    /// inline-table and positioned sizing aligned with the table object
    /// construction used for normal layout:
    /// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>.
    pub(in crate::layout) fn table_intrinsic_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        if rows.is_empty() {
            let width = used_empty_table_grid_width(style, available_table_width, table_width);
            return (width.points(), width.points());
        }

        let grid = table_grid(rows);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
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
        let measures = self.table_column_measures(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            table_width.content_width_points(),
            table_cellpadding,
            table_metrics,
            collapsed_geometry.as_ref(),
        );
        let min_content = measures.table_min_content_width().max(0.0);
        let max_content = measures.table_max_content_width().max(min_content);
        (min_content, max_content)
    }

    /// Return parent-facing content-box intrinsic widths for a table fragment.
    ///
    /// CSS Tables computes grid min/max-content widths from column measures,
    /// but CSS Sizing intrinsic contributions also honor a non-auto preferred
    /// size. For auto-layout tables, the used table content box is clamped so
    /// it is not smaller than the grid min-content width:
    /// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width> and
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(in crate::layout) fn table_parent_intrinsic_content_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        let (min_content, max_content) = self.table_intrinsic_widths_from_fragment(
            element,
            style,
            stylesheets,
            fragment,
            available_outer_width,
        );
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        let horizontal_non_content = table_horizontal_non_content_width(style, table_width);
        let resolved_width =
            used_content_width_or_auto(style, available_table_width, horizontal_non_content)
                .or_else(|| {
                    intrinsic::intrinsic_width_keyword(
                        style.box_values.width,
                        min_content,
                        max_content,
                        available_table_width,
                        horizontal_non_content,
                    )
                })
                .map(|width| {
                    constrain_width(style, width, available_table_width).max(style.font_size)
                });

        if let Some(width) = resolved_width {
            let width = table_content_width_clamped_to_min_content(style, width, min_content);
            (width, width)
        } else {
            (min_content, max_content)
        }
    }

    /// Return parent-facing margin-box intrinsic widths for a table fragment.
    ///
    /// Table parents consume the table wrapper/margin box, while table layout
    /// itself consumes the grid/content width. Keep this conversion separate so
    /// grid sizing remains available for column layout:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(in crate::layout) fn table_outer_intrinsic_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        let (min_content, max_content) = self.table_parent_intrinsic_content_widths_from_fragment(
            element,
            style,
            stylesheets,
            fragment,
            available_outer_width,
        );
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        let horizontal_extras = table_horizontal_non_content_width(style, table_width)
            + style.margin.left
            + style.margin.right;
        (
            min_content + horizontal_extras,
            max_content + horizontal_extras,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn inline_table_atom_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        fragment: &box_tree::TableFragment<'_>,
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> Option<InlineAtom> {
        // CSS Display 3 maps `inline-table` to an inline-level atomic box whose
        // contents establish a table formatting context.
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        if rows.is_empty() {
            return None;
        }
        let grid = table_grid(rows);
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let mut table_width = used_table_width(style, available_width);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
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
        self.resolve_table_used_content_width(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            available_width,
            table_cellpadding,
            table_metrics,
            collapsed_geometry.as_ref(),
            &mut table_width,
        );
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            table_width.content_width_points(),
            !style.box_values.width.is_auto(),
            table_cellpadding,
            table_metrics,
            collapsed_geometry.as_ref(),
        );
        let content_width = column_plan
            .total_width()
            .min(available_width)
            .max(style.font_size);
        let top = 10_000.0;
        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics,
            collapsed_geometry: collapsed_geometry.as_ref(),
        };
        let table_height_plan = self.table_height_plan(&table_context);
        let planned_row_heights = table_height_plan.final_row_heights();
        let planned_row_occupancy = table_height_plan.row_occupancy();
        let top_caption_height = self.estimate_table_captions_height(
            &input.captions,
            style,
            stylesheets,
            content_width,
            CaptionSide::Top,
        );
        let first_row_baseline_range = inline_table_first_occupying_row_range(
            top,
            top_caption_height,
            table_width.border_widths,
            table_width.padding,
            &planned_row_heights,
            &planned_row_occupancy,
            table_metrics,
        );
        let table_strut_baseline_offset =
            self.font_system.rendered_first_line_baseline_offset(style);

        let snapshot = self.snapshot();
        let mut table_style = style.clone();
        table_style.margin = css::Edges::ZERO;
        set_style_used_width(&mut table_style, content_width);
        table_style.break_before = PageBreak::Auto;
        table_style.break_after = PageBreak::Auto;

        self.current_page = Page::new(content_width, top);
        self.content_left = 0.0;
        self.content_right = content_width;
        self.cursor_y = top;
        self.truncate_page_start_margins = false;
        let _ = children;
        self.layout_table(element, &table_style, stylesheets, fragment);
        let content_height = (top - self.cursor_y).max(style.line_height);
        let fragment_bottom = top - content_height;
        // CSS 2.2 defines an `inline-table` baseline as the baseline of the
        // first row. In the current paint model, align against the first
        // rendered row line's actual shaped-font alignment coordinate.
        // https://www.w3.org/TR/CSS22/tables.html#table-display
        let baseline_offset = first_row_baseline_range
            .and_then(|(row_top, row_bottom)| {
                self.inline_table_baseline_offset_from_fragment(
                    row_top,
                    row_bottom,
                    fragment_bottom,
                    content_height,
                    table_strut_baseline_offset,
                )
            })
            .unwrap_or(content_height);
        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintVector::new(0.0, -fragment_bottom));
        self.restore(snapshot);

        let mut atom_style = style.clone();
        atom_style.background_color = None;
        atom_style.border_width = 0.0;
        atom_style.border_widths = css::Edges::ZERO;
        atom_style.border_width_values = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
        atom_style.border_styles = css::BorderStyles::NONE;
        atom_style.padding = css::Edges::ZERO;

        Some(InlineAtom::new(
            InlineAtomContent::InlineFragment(fragment),
            atom_style,
            None,
            content_width + style.margin.left + style.margin.right,
            content_height,
            baseline_offset,
            baseline_shift,
            link_target,
            None,
        ))
    }

    /// Return the inline-table first-row baseline offset from the fragment top edge.
    ///
    /// CSS 2.2 defines an `inline-table` baseline as the baseline of the first
    /// table row. `reasyprint` stores text using PDF baseline-adjusted glyph
    /// coordinates, so this maps the rendered line inside the first occupying
    /// row to the atom baseline offset from the table wrapper top edge used by
    /// the line builder.
    /// https://www.w3.org/TR/CSS22/tables.html#table-display
    pub(in crate::layout::table) fn inline_table_baseline_offset_from_fragment(
        &self,
        row_top: f32,
        row_bottom: f32,
        fragment_bottom: f32,
        content_height: f32,
        table_strut_baseline_offset: f32,
    ) -> Option<f32> {
        self.current_page
            .lines
            .iter()
            .map(|line| self.font_system.rendered_line_alignment_y(line))
            .find(|alignment_y| *alignment_y <= row_top + 0.01 && *alignment_y >= row_bottom - 0.01)
            .map(|alignment_y| {
                (content_height - (alignment_y - fragment_bottom) + table_strut_baseline_offset)
                    .clamp(0.0, content_height + table_strut_baseline_offset)
            })
    }

    pub(crate) fn estimate_table_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        fragment: &box_tree::TableFragment<'_>,
    ) -> f32 {
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let captions = input.captions.as_slice();
        let columns = input.columns.as_slice();

        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        if rows.is_empty() {
            return self.estimate_empty_table_height(
                captions,
                style,
                stylesheets,
                available_table_width,
                table_width,
            );
        }
        let grid = table_grid(rows);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    columns,
                    grid.column_count,
                )
            });
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            columns,
            table_width.content_width_points(),
            !style.box_values.width.is_auto(),
            table_cellpadding,
            table_metrics,
            collapsed_geometry.as_ref(),
        );

        let mut total = style.margin.top;
        total += self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            table_width.content_width_points(),
            CaptionSide::Top,
        );
        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics,
            collapsed_geometry: collapsed_geometry.as_ref(),
        };
        let table_height_plan = self.table_height_plan(&table_context);
        let row_heights = table_height_plan.final_row_heights();
        let row_occupancy = table_height_plan.row_occupancy();
        total += table_content_height(&row_heights, &row_occupancy, table_metrics);
        total += self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            table_width.content_width_points(),
            CaptionSide::Bottom,
        );
        total + style.margin.bottom
    }
}
