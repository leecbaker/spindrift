use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Estimate the block-axis size of a table whose row grid has no rows.
    ///
    /// CSS Tables 3 says that if a table has no slots, its width/height are
    /// computed from the table grid box if definite, otherwise zero; captions,
    /// padding, borders, and margins still contribute to the table wrapper:
    /// <https://drafts.csswg.org/css-tables/#computing-the-table-height>.
    pub(in crate::layout::table) fn estimate_empty_table_height(
        &mut self,
        captions: &[TableCaption<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_table_width: f32,
        table_width: UsedTableWidth,
    ) -> f32 {
        let content_width = used_empty_table_grid_width(style, available_table_width, table_width);
        let content_width_points = content_width.points();
        let content_height = used_empty_table_grid_height(
            style,
            self.definite_block_size_stack.last().copied().flatten(),
            table_width,
        );
        style.margin.top
            + self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                content_width_points,
                CaptionSide::Top,
            )
            + table_width.border_widths.top
            + table_width.padding.top
            + content_height
            + table_width.padding.bottom
            + table_width.border_widths.bottom
            + self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                content_width_points,
                CaptionSide::Bottom,
            )
            + style.margin.bottom
    }

    pub(in crate::layout::table) fn place_empty_table_wrapper(
        &mut self,
        captions: &[TableCaption<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_table_width: f32,
        table_width: UsedTableWidth,
        relative_offset: RelativeOffset,
    ) -> (f32, f32, f32, f32) {
        let content_width = used_empty_table_grid_width(style, available_table_width, table_width);
        let content_width_points = content_width.points();
        let content_height = used_empty_table_grid_height(
            style,
            self.definite_block_size_stack.last().copied().flatten(),
            table_width,
        );
        let border_box_width = table_width.wrapper_border_box_width(content_width).points();
        let mut used_style = style.clone();
        resolve_normal_flow_auto_margins_for_known_width(
            &mut used_style,
            (self.content_right - self.content_left).max(0.0),
            border_box_width,
            self.containing_block_direction,
        );
        let style = &used_style;
        let top_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            content_width_points,
            CaptionSide::Top,
        );
        let bottom_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            content_width_points,
            CaptionSide::Bottom,
        );
        let collision_height = table_wrapper_collision_height(
            style,
            table_width,
            top_caption_height,
            content_height,
            bottom_caption_height,
        );

        self.cursor_y -= style.margin.top;
        self.prebreak_bfc_margin_box_if_needed(collision_height, style.margin.top);
        let (margin_box_left, avoided_top, _) = self.place_float_avoiding_margin_box(
            self.cursor_y,
            style.margin.left + border_box_width + style.margin.right,
            collision_height,
            style.clear,
            style.writing_mode,
            style.direction,
            self.containing_block_direction,
        );
        self.cursor_y = avoided_top;
        (
            margin_box_left + style.margin.left + relative_offset.x,
            content_width_points,
            content_height,
            border_box_width,
        )
    }

    /// Layout and paint a table whose row grid has no rows.
    ///
    /// CSS Tables 3 keeps an empty table wrapper in layout even when the grid
    /// has no slots. The row grid contributes zero auto width/height, while the
    /// wrapper's padding, borders, captions, margins, and definite grid sizes
    /// still affect painting and block progression:
    /// <https://drafts.csswg.org/css-tables/#computing-the-table-height>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn layout_empty_table(
        &mut self,
        captions: &[TableCaption<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_table_width: f32,
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        relative_offset: RelativeOffset,
        table_is_document_canvas: bool,
    ) {
        let (table_outer_x, content_width, content_height, border_box_width) = self
            .place_empty_table_wrapper(
                captions,
                style,
                stylesheets,
                available_table_width,
                table_width,
                relative_offset,
            );
        let border_box_height = table_wrapper_border_box_height(content_height, table_width);
        let table_x = table_width.content_x(table_outer_x);

        self.push_float_context();
        let table_wrapper_top = self.cursor_y;
        let border_box_x = table_x - table_width.padding.left - table_width.border_widths.left;
        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            let top_caption_height = self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                content_width,
                CaptionSide::Top,
            );
            let bottom_caption_height = self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                content_width,
                CaptionSide::Bottom,
            );
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(
                    table_wrapper_positioning_containing_block(
                        table_x,
                        table_wrapper_top,
                        content_width,
                        content_height,
                        table_width,
                        top_caption_height,
                        bottom_caption_height,
                    ),
                ));
        }
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            table_x,
            content_width,
            CaptionSide::Top,
        );

        let table_box_top = self.cursor_y;

        let table_structure_paint_checkpoint = self.current_page.paint_checkpoint();
        let table_structure_paint_page_index = self.pages.len();
        if let Some(fill) = style.background_color {
            self.push_rect_in_band(
                PaintBand::InFlowBlock,
                PageTopRect::new(
                    border_box_x,
                    table_box_top,
                    border_box_width,
                    border_box_height,
                )
                .rendered_rect(Some(fill)),
            );
        }
        let table_paint_box = TableWrapperPaintBox {
            table_x,
            top: table_box_top,
            content_width,
            content_height,
            table_width,
            table_metrics,
        };
        self.paint_separated_table_wrapper_border(style, table_paint_box);
        if self.pages.len() == table_structure_paint_page_index {
            let bounds = table_paint_box.border_box().paint_clip();
            let overflow_clip = table_box_overflow_clip(
                style,
                table_paint_box.padding_box().paint_clip(),
                table_is_document_canvas,
            );
            let policy =
                table_atomic_stacking_policy(style, PaintBand::InFlowBlock, bounds, overflow_clip);
            self.scope_current_page_paint_since(
                &table_structure_paint_checkpoint,
                policy.parent_band,
                bounds,
                Vec::new(),
                policy.effects,
            );
        }

        self.cursor_y -= border_box_height;
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            table_x,
            content_width,
            CaptionSide::Bottom,
        );
        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
        }
        self.pop_float_context();
        self.cursor_y -= style.margin.bottom;
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y;
        }
        self.apply_forced_break(style.break_after);
    }

    /// Paint the border of a separated-border table wrapper.
    ///
    /// CSS 2.2's separated border model gives the table-root its own ordinary
    /// border box, distinct from row and cell borders. Collapsed borders are
    /// resolved through the collapsed-border grid instead:
    /// <https://www.w3.org/TR/CSS22/tables.html#separated-borders> and
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
    pub(in crate::layout::table) fn paint_separated_table_wrapper_border(
        &mut self,
        style: &ComputedStyle,
        wrapper: TableWrapperPaintBox,
    ) {
        if wrapper.table_metrics.border_collapse == css::BorderCollapse::Collapse {
            return;
        }
        let border_box_x = wrapper.table_x
            - wrapper.table_width.padding.left
            - wrapper.table_width.border_widths.left;
        let border_box_width = wrapper.content_width
            + wrapper.table_width.padding.left
            + wrapper.table_width.padding.right
            + wrapper.table_width.border_widths.left
            + wrapper.table_width.border_widths.right;
        let border_box_height = wrapper.content_height
            + wrapper.table_width.padding.top
            + wrapper.table_width.padding.bottom
            + wrapper.table_width.border_widths.top
            + wrapper.table_width.border_widths.bottom;
        let mut border_rects = Vec::new();
        let mut border_paths = Vec::new();
        paint_table_border_edges(
            &mut border_rects,
            &mut border_paths,
            border_box_x,
            wrapper.top,
            border_box_width,
            border_box_height,
            style,
        );
        for rect in border_rects {
            self.push_rect_in_band(PaintBand::InFlowBlock, rect);
        }
        for path in border_paths {
            self.push_path_in_band(PaintBand::InFlowBlock, path);
        }
    }

    /// Paint a repeated `table-footer-group` at the block-end of a page fragment.
    ///
    /// CSS 2.2 allows print user agents to repeat table footer groups on each
    /// page spanned by a table, visually after the body rows in that page
    /// fragment and before bottom captions.
    /// https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn layout_repeated_table_footer_rows_at_page_bottom(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        columns: &[TableColumn<'_>],
        footer_rows: &[usize],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) {
        let footer_height = repeated_table_rows_height(
            footer_rows,
            planned_row_heights,
            planned_row_occupancy,
            table_metrics,
        );
        if footer_rows.is_empty() || footer_height > self.page_area_height() + 0.01 {
            return;
        }

        let previous_cursor_y = self.cursor_y;
        self.cursor_y = self.page_bottom() + footer_height;
        self.layout_repeated_table_rows(
            rows,
            grid,
            columns,
            footer_rows,
            table_style,
            stylesheets,
            table_x,
            used_table_width,
            table_cellpadding,
            column_plan,
            planned_row_heights,
            planned_row_occupancy,
            table_width,
            table_metrics,
            collapsed_geometry,
        );
        self.cursor_y = previous_cursor_y;
    }

    /// Replay measured table row boxes for repeated table header/footer groups.
    ///
    /// CSS 2.2 defines `table-header-group` and `table-footer-group` as row
    /// groups that print user agents may repeat on pages spanned by a table.
    /// https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group
    /// https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn layout_repeated_table_rows(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        columns: &[TableColumn<'_>],
        repeated_rows: &[usize],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) {
        if repeated_rows.is_empty() {
            return;
        }
        let repeated_height = repeated_table_rows_height(
            repeated_rows,
            planned_row_heights,
            planned_row_occupancy,
            table_metrics,
        );
        if repeated_height > self.page_area_height() + 0.01 {
            return;
        }

        let mut repeated_row_tops = Vec::with_capacity(repeated_rows.len());
        let mut repeated_row_heights = Vec::with_capacity(repeated_rows.len());
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        let fragment_top = self.cursor_y;
        let occupied_inline_bounds = column_plan
            .occupied_inline_bounds()
            .unwrap_or_else(|| TableInlineBounds::new(0.0, used_table_width));
        let occupied_x = table_x + occupied_inline_bounds.start;
        let occupied_width = occupied_inline_bounds.size;
        self.paint_repeated_table_fragment_structural_layers(
            rows,
            repeated_rows,
            columns,
            table_style,
            stylesheets,
            table_x,
            used_table_width,
            fragment_top,
            repeated_height,
            table_width,
            column_plan,
            planned_row_heights,
            planned_row_occupancy,
            table_metrics,
        );
        // Repeated header/footer rows are visual copies, not new source boxes.
        // Suppress element side effects while preserving paint replay.
        // <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>
        self.element_side_effect_suppression_depth += 1;
        for (position, row_index) in repeated_rows.iter().copied().enumerate() {
            let row = &rows[row_index];
            let row_style = self.style_for_table_row(row, table_style, stylesheets);
            let row_height = planned_row_heights[row_index];
            let row_occupied = planned_row_occupancy
                .get(row_index)
                .copied()
                .unwrap_or(false);
            let row_top = self.cursor_y;
            repeated_row_tops.push(row_top);
            repeated_row_heights.push(if row_occupied { row_height } else { 0.0 });
            if !row_occupied || table_row_is_collapsed(&row_style) {
                continue;
            }

            let row_baseline_offset = self.table_row_baseline_offset(
                row_index,
                row,
                &grid.rows[row_index],
                &row_style,
                stylesheets,
                table_cellpadding,
                column_plan,
                table_metrics,
                collapsed_geometry,
            );
            // CSS Tables allow repeated table-header-group and
            // table-footer-group boxes on fragmented tables. This replays the
            // row group's visible row content using measured row heights.
            // Collapsed-border conflict resolution for repeated fragments
            // still needs durable per-fragment table border grids.
            // https://www.w3.org/TR/CSS22/tables.html#table-display
            if row_style.background_color.is_some() {
                self.push_rect_in_band(
                    PaintBand::InFlowBlock,
                    PageTopRect::new(occupied_x, row_top, occupied_width, row_height)
                        .rendered_rect(row_style.background_color),
                );
            }
            for placement in &grid.rows[row_index] {
                let cell = &row.cells[placement.cell];
                let Some(prepared) = self.prepare_table_cell(
                    cell,
                    row,
                    &row_style,
                    placement,
                    row_index,
                    table_x,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics,
                    collapsed_geometry,
                ) else {
                    continue;
                };
                let cell_style = &prepared.style;
                let cell_borders = prepared.borders;
                let metrics = prepared.metrics;
                let cell_height = table_row_span_height(
                    planned_row_heights,
                    planned_row_occupancy,
                    row_index,
                    placement.rowspan,
                    table_metrics,
                )
                .max(metrics.border_box_height);
                let cell_placement = TableGridPlacement::new(PageTopPoint::new(table_x, row_top));
                let cell_border_box = column_plan
                    .cell_border_box(prepared.area, TableRowBounds::new(0.0, cell_height));
                let cell_x = cell_border_box.x(cell_placement);
                let cell_width = cell_border_box.width();
                let text = prepared.text;
                let cell_is_empty = text.is_empty() && metrics.content_height <= 0.0;
                let baseline_context = TableCellBaselineAlignmentContext {
                    row_index,
                    row_style: &row_style,
                    table_style,
                    rows,
                    grid,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    planned_row_heights,
                    planned_row_occupancy,
                    table_metrics,
                    collapsed_geometry,
                    row_baseline_offset,
                };
                let cell_row_baseline_offset = self.table_cell_row_baseline_offset_for_alignment(
                    &baseline_context,
                    placement,
                    cell_style,
                );
                let content_offset = table_cell_content_offset(
                    cell_style,
                    metrics.content_height,
                    cell_height,
                    cell_row_baseline_offset,
                    metrics.baseline_offset,
                );
                let content_x_offset = self.table_cell_content_x_offset(
                    cell,
                    cell_style,
                    stylesheets,
                    cell_width,
                    cell_borders,
                );
                let content_clip = self.collapsed_rowspan_cell_content_clip(
                    row_index,
                    placement.rowspan,
                    rows,
                    table_style,
                    stylesheets,
                    planned_row_heights,
                    planned_row_occupancy,
                    table_metrics,
                    cell_border_box,
                    cell_placement,
                );
                let paint_empty_cell = table_metrics.border_collapse
                    == css::BorderCollapse::Collapse
                    || cell_style.empty_cells == EmptyCells::Show
                    || !cell_is_empty;

                if paint_empty_cell {
                    let (rects, rounded_rects, paths, strokes) = block_paint_ops_with_border_insets(
                        cell_x,
                        row_top - cell_height,
                        cell_width,
                        cell_height,
                        cell_style,
                        cell_borders,
                        false,
                    );
                    for rect in rects {
                        self.push_rect_in_band(PaintBand::InFlowBlock, rect);
                    }
                    for rounded_rect in rounded_rects {
                        self.push_rounded_rect_in_band(PaintBand::InFlowBlock, rounded_rect);
                    }
                    for path in paths {
                        self.push_path_in_band(PaintBand::InFlowBlock, path);
                    }
                    for stroke in strokes {
                        self.push_stroke_in_band(PaintBand::InFlowBlock, stroke);
                    }
                }
                if table_metrics.border_collapse != css::BorderCollapse::Collapse
                    && paint_empty_cell
                {
                    let mut border_rects = Vec::new();
                    let mut border_paths = Vec::new();
                    paint_table_border_edges(
                        &mut border_rects,
                        &mut border_paths,
                        cell_x,
                        row_top,
                        cell_width,
                        cell_height,
                        cell_style,
                    );
                    for rect in border_rects {
                        self.push_rect_in_band(PaintBand::InFlowBlock, rect);
                    }
                    for path in border_paths {
                        self.push_path_in_band(PaintBand::InFlowBlock, path);
                    }
                }

                let clip_active = if let Some(clip) = content_clip {
                    self.push_overflow_clip(clip);
                    true
                } else {
                    false
                };

                if !text.is_empty() && cell.children.is_none() {
                    let content_box = cell_border_box.content_box(
                        cell_placement,
                        cell_style.padding,
                        cell_borders,
                        content_offset,
                        content_x_offset,
                    );
                    let content_scope = self.enter_table_cell_content_scope(
                        cell_style,
                        content_box,
                        self.table_cell_child_ancestors(cell, row),
                        None,
                    );
                    self.push_float_context();
                    if let Some(element) = cell.element {
                        self.layout_inline_items_block(
                            element,
                            cell_style,
                            stylesheets,
                            (0.0, 0.0),
                            table_cell_href(cell),
                            None,
                        );
                    } else {
                        self.layout_text_block(&text, cell_style, 0.0, 0.0, table_cell_href(cell));
                    }
                    self.pop_float_context();
                    self.restore_table_cell_content_scope(content_scope);
                }
                self.layout_table_cell_replaced_children(
                    cell,
                    cell_style,
                    cell_borders,
                    cell_border_box,
                    cell_placement,
                    content_offset,
                    content_x_offset,
                );
                self.layout_table_cell_flow_children(
                    cell,
                    row,
                    cell_style,
                    &prepared.row_sizing_style,
                    table_style,
                    stylesheets,
                    cell_borders,
                    cell_border_box,
                    cell_placement,
                    content_offset,
                    content_x_offset,
                );
                self.layout_table_cell_positioned_children(
                    cell,
                    row,
                    cell_style,
                    stylesheets,
                    cell_borders,
                    cell_border_box,
                    cell_placement,
                    content_offset,
                    content_x_offset,
                );
                self.pop_overflow_clip(clip_active);
            }

            if row_occupied {
                self.cursor_y -= row_height;
            }
            if row_occupied
                && repeated_rows[position + 1..]
                    .iter()
                    .any(|row| planned_row_occupancy.get(*row).copied().unwrap_or(false))
            {
                self.cursor_y -= table_metrics.spacing.vertical.length_points();
            }
        }
        self.element_side_effect_suppression_depth -= 1;
        if let Some(geometry) = collapsed_geometry {
            let repeated_row_offsets = vec![0.0; repeated_rows.len()];
            let repeated_original_heights = repeated_rows
                .iter()
                .map(|row| planned_row_heights[*row])
                .collect::<Vec<_>>();
            let placement = TableGridPlacement::new(PageTopPoint::new(table_x, 0.0));
            let (rects, paths) = geometry.grid.paint_fragment_rows(
                placement,
                column_plan,
                repeated_rows,
                &repeated_row_tops,
                &repeated_row_heights,
                &repeated_row_offsets,
                &repeated_original_heights,
            );
            for rect in rects {
                self.push_rect_in_band(PaintBand::InFlowBlock, rect);
            }
            for path in paths {
                self.push_path_in_band(PaintBand::InFlowBlock, path);
            }
        }
        if self.pages.len() == paint_page_index {
            let bounds = PageTopRect::new(
                table_x - table_width.padding.left - table_width.border_widths.left,
                fragment_top + table_width.padding.top + table_width.border_widths.top,
                used_table_width
                    + table_width.padding.left
                    + table_width.padding.right
                    + table_width.border_widths.left
                    + table_width.border_widths.right,
                fragment_top + table_width.padding.top + table_width.border_widths.top
                    - self.cursor_y
                    + table_width.padding.bottom
                    + table_width.border_widths.bottom,
            )
            .paint_clip();
            let overflow_clip = table_box_overflow_clip(
                table_style,
                table_padding_box_clip_from_border_box(bounds, table_width),
                false,
            );
            let policy = table_atomic_stacking_policy(
                table_style,
                PaintBand::InFlowBlock,
                bounds,
                overflow_clip,
            );
            let child_contexts = self.positioned_child_contexts_since(
                positioned_layer_start,
                paint_page_index,
                policy,
            );
            self.scope_current_page_paint_since_with_policy(
                &paint_checkpoint,
                policy,
                bounds,
                child_contexts,
            );
        }
    }

    /// Paint table and column structural layers for one repeated table fragment.
    ///
    /// CSS 2.2 table painting orders structural backgrounds below row, cell,
    /// and border paint, while outlines paint in the final outline band.
    /// Repeated header/footer fragments therefore need their own page-local
    /// table, column, and row-group layers around row replay:
    /// <https://www.w3.org/TR/CSS22/tables.html#table-layers> and
    /// <https://drafts.csswg.org/css-tables-3/#rendering>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn paint_repeated_table_fragment_structural_layers(
        &mut self,
        rows: &[TableRow<'_>],
        repeated_rows: &[usize],
        columns: &[TableColumn<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        fragment_top: f32,
        fragment_height: f32,
        table_width: UsedTableWidth,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_metrics: TableMetrics,
    ) {
        if let Some(fill) = table_style.background_color {
            let background_top =
                fragment_top + table_width.padding.top + table_width.border_widths.top;
            let background_bottom = fragment_top
                - fragment_height
                - table_width.padding.bottom
                - table_width.border_widths.bottom;
            self.push_rect_in_band(
                PaintBand::InFlowBlock,
                PageTopRect::new(
                    table_x - table_width.padding.left - table_width.border_widths.left,
                    background_top,
                    used_table_width
                        + table_width.padding.left
                        + table_width.padding.right
                        + table_width.border_widths.left
                        + table_width.border_widths.right,
                    background_top - background_bottom,
                )
                .rendered_rect(Some(fill)),
            );
        }
        let mut local_row_tops = Vec::with_capacity(repeated_rows.len());
        let mut local_row_heights = Vec::with_capacity(repeated_rows.len());
        let mut cursor_y = fragment_top;
        let occupied_inline_bounds = column_plan
            .occupied_inline_bounds()
            .unwrap_or_else(|| TableInlineBounds::new(0.0, used_table_width));
        let occupied_x = table_x + occupied_inline_bounds.start;
        let occupied_width = occupied_inline_bounds.size;
        for (position, row_index) in repeated_rows.iter().copied().enumerate() {
            local_row_tops.push(cursor_y);
            let row_height = planned_row_heights[row_index];
            let row_occupied = planned_row_occupancy
                .get(row_index)
                .copied()
                .unwrap_or(false);
            local_row_heights.push(if row_occupied { row_height } else { 0.0 });
            if row_occupied {
                cursor_y -= row_height;
            }
            if row_occupied
                && repeated_rows[position + 1..]
                    .iter()
                    .any(|row| planned_row_occupancy.get(*row).copied().unwrap_or(false))
            {
                cursor_y -= table_metrics.spacing.vertical.length_points();
            }
        }
        for (start_column, end_column, column_group) in
            table_column_group_spans(columns, column_plan.column_count())
        {
            let column_group_style =
                self.style_for_table_column_group(&column_group, table_style, stylesheets);
            for primitive in table_column_fragment_background_primitives(
                table_x,
                fragment_top,
                fragment_height,
                column_plan,
                start_column,
                end_column,
                &column_group_style,
                &local_row_tops,
                &local_row_heights,
            ) {
                self.push_primitive_in_band(PaintBand::InFlowBlock, primitive);
            }
        }
        let mut column_index = 0;
        for column in columns {
            if column_index >= column_plan.column_count() {
                break;
            }
            let span = column
                .span
                .min(column_plan.column_count() - column_index)
                .max(1);
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            for primitive in table_column_fragment_background_primitives(
                table_x,
                fragment_top,
                fragment_height,
                column_plan,
                column_index,
                column_index + span,
                &column_style,
                &local_row_tops,
                &local_row_heights,
            ) {
                self.push_primitive_in_band(PaintBand::InFlowBlock, primitive);
            }
            column_index += span;
        }

        for (start_row, end_row, row_group) in table_row_group_spans(rows) {
            let row_group_style =
                self.style_for_table_row_group(&row_group, table_style, stylesheets);
            if let Some(fill) = row_group_style.background_color {
                let mut segment_start = None;
                let mut previous_local = None;
                for (local_row, original_row) in repeated_rows.iter().copied().enumerate() {
                    if original_row >= start_row && original_row < end_row {
                        if segment_start.is_none() {
                            segment_start = Some(local_row);
                        }
                        previous_local = Some(local_row + 1);
                    } else if let (Some(start), Some(end)) =
                        (segment_start.take(), previous_local.take())
                    {
                        self.paint_repeated_table_row_group_background(
                            occupied_x,
                            occupied_width,
                            &local_row_tops,
                            &local_row_heights,
                            start,
                            end,
                            fill,
                        );
                    }
                }
                if let (Some(start), Some(end)) = (segment_start, previous_local) {
                    self.paint_repeated_table_row_group_background(
                        occupied_x,
                        occupied_width,
                        &local_row_tops,
                        &local_row_heights,
                        start,
                        end,
                        fill,
                    );
                }
            }
            if row_group_style.visibility == Visibility::Visible {
                self.paint_repeated_table_row_group_outline(
                    occupied_x,
                    occupied_width,
                    &local_row_tops,
                    &local_row_heights,
                    repeated_rows,
                    start_row,
                    end_row,
                    &row_group_style,
                );
            }
        }
    }
}
