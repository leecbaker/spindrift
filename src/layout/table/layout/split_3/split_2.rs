use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(crate) fn layout_table(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
    ) {
        self.apply_forced_break(style.break_before);

        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let relative_offset = relative_position_offset(style, self.current_containing_block());
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y += relative_offset.y;
        }
        let captions = input.captions.as_slice();
        let columns = input.columns.as_slice();

        let available_table_width =
            self.content_right - self.content_left - style.margin.left - style.margin.right;
        let mut table_width = used_table_width(style, available_table_width);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        if rows.is_empty() {
            self.layout_empty_table(
                captions,
                style,
                stylesheets,
                available_table_width,
                table_width,
                table_metrics,
                relative_offset,
                is_document_canvas_element(element),
            );
            return;
        }
        let grid = table_grid(rows);
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
        self.resolve_table_used_content_width(
            rows,
            &grid,
            style,
            stylesheets,
            columns,
            available_table_width,
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
            columns,
            table_width.content_width_points(),
            !style.box_values.width.is_auto(),
            table_cellpadding,
            table_metrics,
            collapsed_geometry.as_ref(),
        );
        if let Some(geometry) = &collapsed_geometry {
            table_width.border_widths = geometry.outer_insets;
        }
        let used_table_width = column_plan.total_width();
        let table_border_box_width = table_wrapper_border_box_width(used_table_width, table_width);
        let mut used_style = style.clone();
        resolve_normal_flow_auto_margins_for_known_width(
            &mut used_style,
            (self.content_right - self.content_left).max(0.0),
            table_border_box_width,
            self.containing_block_direction,
        );
        let style = &used_style;
        let repeating_header_rows = table_repeating_header_row_indices(rows);
        let repeating_footer_rows = table_repeating_footer_row_indices(rows);

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
        let repeating_header_height = repeated_table_rows_height(
            &repeating_header_rows,
            &planned_row_heights,
            &planned_row_occupancy,
            table_metrics,
        );
        let repeating_footer_height = repeated_table_rows_height(
            &repeating_footer_rows,
            &planned_row_heights,
            &planned_row_occupancy,
            table_metrics,
        );
        let row_group_spans = table_row_group_spans(rows);
        let avoid_break_row_groups = row_group_spans
            .iter()
            .filter_map(|(start, end, row_group)| {
                let row_group_style = self.style_for_table_row_group(row_group, style, stylesheets);
                row_group_style
                    .break_inside_avoid
                    .then_some((*start, *end, row_group_style))
            })
            .collect::<Vec<_>>();
        let mut row_group_break_before = vec![PageBreak::Auto; rows.len()];
        let mut row_group_break_after = vec![PageBreak::Auto; rows.len()];
        for (start, end, row_group) in &row_group_spans {
            let row_group_style = self.style_for_table_row_group(row_group, style, stylesheets);
            if row_group_style.break_before != PageBreak::Auto {
                row_group_break_before[*start] = row_group_style.break_before;
            }
            if row_group_style.break_after != PageBreak::Auto && end > start {
                row_group_break_after[end - 1] = row_group_style.break_after;
            }
        }
        let top_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            used_table_width,
            CaptionSide::Top,
        );
        let bottom_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            used_table_width,
            CaptionSide::Bottom,
        );
        let table_content_height =
            table_content_height(&planned_row_heights, &planned_row_occupancy, table_metrics);
        let table_collision_height = table_wrapper_collision_height(
            style,
            table_width,
            top_caption_height,
            table_content_height,
            bottom_caption_height,
        );
        self.cursor_y -= style.margin.top;
        self.prebreak_bfc_margin_box_if_needed(table_collision_height, style.margin.top);
        let (margin_box_left, avoided_top, _) = self.place_float_avoiding_margin_box(
            self.cursor_y,
            style.margin.left + table_border_box_width + style.margin.right,
            table_collision_height,
            style.clear,
            style.writing_mode,
            style.direction,
            self.containing_block_direction,
        );
        self.cursor_y = avoided_top;
        let table_outer_x = margin_box_left + style.margin.left + relative_offset.x;
        // CSS table wrappers paint borders/padding around the table grid; the
        // grid itself starts at the content-box inline-start edge.
        let table_x = table_width.content_x(table_outer_x);

        self.push_float_context();
        let table_wrapper_top = self.cursor_y;
        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(
                    table_wrapper_positioning_containing_block(
                        table_x,
                        table_wrapper_top,
                        used_table_width,
                        table_content_height,
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
            used_table_width,
            CaptionSide::Top,
        );
        let table_box_top = self.cursor_y;
        self.cursor_y -= table_width.border_widths.top + table_width.padding.top;
        let table_edge_spacing = table_vertical_edge_spacing(&planned_row_occupancy, table_metrics);
        self.cursor_y -= table_edge_spacing;

        let table_is_document_canvas = is_document_canvas_element(element);
        let table_structure_paint_checkpoint = self.current_page.paint_checkpoint();
        let table_structure_paint_page_index = self.pages.len();
        let table_paint_box = TableWrapperPaintBox {
            table_x,
            top: table_box_top,
            content_width: used_table_width,
            content_height: table_content_height,
            table_width,
            table_metrics,
        };
        if self.pages.len() == table_structure_paint_page_index {
            let bounds = table_paint_box.border_box().paint_clip();
            let overflow_clip = table_box_overflow_clip(
                style,
                table_paint_box.padding_box().paint_clip(),
                table_is_document_canvas,
            );
            let policy =
                table_atomic_stacking_policy(style, PaintBand::InFlowBlock, bounds, overflow_clip);
            self.scope_current_page_paint_since_with_policy(
                &table_structure_paint_checkpoint,
                policy,
                bounds,
                Vec::new(),
            );
        }

        // Table row-grid fragments split independently of the source row's
        // previous block position. Recording the grid's fragment offset lets
        // `push_page` continue table rows at the page-start position of the
        // surrounding formatting context, instead of at the consumed position
        // of the row that triggered the break.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        self.fragment_top_offsets
            .push(self.current_page_context.top() - self.cursor_y);
        let (
            mut table_body_fragment,
            forced_break_after_table_rows,
            current_fragment_repeat_policy,
        ) = self.layout_table_body_rows(TableBodyRowsInput {
            rows,
            grid: &grid,
            columns,
            style,
            stylesheets,
            table_x,
            used_table_width,
            table_cellpadding,
            column_plan: &column_plan,
            planned_row_heights: &planned_row_heights,
            planned_row_occupancy: &planned_row_occupancy,
            table_width,
            table_metrics,
            collapsed_geometry: collapsed_geometry.as_ref(),
            table_is_document_canvas,
            repeating_header_rows: &repeating_header_rows,
            repeating_footer_rows: &repeating_footer_rows,
            repeating_header_height,
            repeating_footer_height,
            avoid_break_row_groups: &avoid_break_row_groups,
            row_group_break_before: &row_group_break_before,
            row_group_break_after: &row_group_break_after,
        });
        self.mark_table_body_fragment_repeated_footers(
            &mut table_body_fragment,
            current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
            &planned_row_heights,
            &planned_row_occupancy,
            table_metrics,
        );
        self.finalize_table_body_paint_fragment(
            &mut table_body_fragment,
            rows,
            &grid,
            columns,
            style,
            stylesheets,
            table_x,
            used_table_width,
            table_cellpadding,
            &column_plan,
            table_width,
            table_metrics,
            collapsed_geometry.as_ref(),
            table_is_document_canvas,
        );
        self.fragment_top_offsets.pop();
        self.cursor_y -= table_edge_spacing;

        self.cursor_y -= table_width.padding.bottom + table_width.border_widths.bottom;
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            table_x,
            used_table_width,
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
        self.apply_forced_break(if style.break_after.is_forced() {
            style.break_after
        } else {
            forced_break_after_table_rows
        });
    }
}
