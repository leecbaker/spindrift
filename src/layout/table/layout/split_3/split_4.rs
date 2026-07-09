use super::*;
use crate::layout::inline_collect::InlinePlacement;

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn paint_repeated_table_row_group_background(
        &mut self,
        table_x: f32,
        used_table_width: f32,
        local_row_tops: &[f32],
        local_row_heights: &[f32],
        start: usize,
        end: usize,
        fill: Color,
    ) {
        if let Some(bounds) = table_fragment_row_span_bounds(
            table_x,
            used_table_width,
            local_row_tops,
            local_row_heights,
            start,
            end,
        ) {
            self.push_rect_in_band(
                PaintBand::InFlowBlock,
                RenderedRect::from_paint_rect(bounds.paint_rect(), Some(fill)),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn paint_repeated_table_row_group_outline(
        &mut self,
        table_x: f32,
        used_table_width: f32,
        local_row_tops: &[f32],
        local_row_heights: &[f32],
        repeated_rows: &[usize],
        start_row: usize,
        end_row: usize,
        row_group_style: &ComputedStyle,
    ) {
        let mut segment_start = None;
        let mut previous_local = None;
        for (local_row, original_row) in repeated_rows.iter().cloned().enumerate() {
            if original_row >= start_row && original_row < end_row {
                if segment_start.is_none() {
                    segment_start = Some(local_row);
                }
                previous_local = Some(local_row + 1);
            } else if let (Some(start), Some(end)) = (segment_start.take(), previous_local.take()) {
                self.push_repeated_table_row_group_outline(
                    table_x,
                    used_table_width,
                    local_row_tops,
                    local_row_heights,
                    start,
                    end,
                    row_group_style,
                );
            }
        }
        if let (Some(start), Some(end)) = (segment_start, previous_local) {
            self.push_repeated_table_row_group_outline(
                table_x,
                used_table_width,
                local_row_tops,
                local_row_heights,
                start,
                end,
                row_group_style,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn push_repeated_table_row_group_outline(
        &mut self,
        table_x: f32,
        used_table_width: f32,
        local_row_tops: &[f32],
        local_row_heights: &[f32],
        start: usize,
        end: usize,
        row_group_style: &ComputedStyle,
    ) {
        let Some(bounds) = table_fragment_row_span_bounds(
            table_x,
            used_table_width,
            local_row_tops,
            local_row_heights,
            start,
            end,
        ) else {
            return;
        };
        for primitive in self.box_outline_primitives(
            paint_space_rect(bounds.x(), bounds.y(), bounds.width(), bounds.height()),
            row_group_style,
        ) {
            self.push_primitive_in_band(PaintBand::Outline, primitive);
        }
    }

    pub(in crate::layout::table) fn ensure_table_body_paint_fragment(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        fragmentainer_kind: FragmentainerKind,
        fragment_top: f32,
        start: TableFragmentStartDecision,
        repeating_header_rows: &[usize],
    ) -> bool {
        if fragment.is_none() {
            let mut new_fragment = TableBodyPaintFragment::new(
                fragmentainer_kind,
                self.current_page.paint_checkpoint(),
                self.pages.len(),
                self.positioned_layers.len(),
                fragment_top,
                start,
            );
            new_fragment.mark_repeated_headers(start.repeated_header_rows(repeating_header_rows));
            *fragment = Some(new_fragment);
            true
        } else {
            false
        }
    }

    pub(in crate::layout::table) fn mark_table_body_fragment_repeated_footers(
        &self,
        fragment: &mut Option<TableBodyPaintFragment>,
        footer_rows: &[usize],
        planned_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_metrics: TableMetrics,
    ) {
        if footer_rows.is_empty() {
            return;
        }
        let footer_height = repeated_table_rows_height(
            footer_rows,
            planned_row_heights,
            planned_row_occupancy,
            table_metrics,
        );
        if footer_height <= self.page_area_height() + 0.01
            && let Some(fragment) = fragment
        {
            fragment.mark_repeated_footers(footer_rows);
        }
    }

    /// Finalize one table-body page piece as a durable scoped paint context.
    ///
    /// CSS 2.2 table painting has internal layer order, and CSS Fragmentation
    /// repeats that painting model for each page fragment. The finalized
    /// context preserves that table-local order until final PDF emission:
    /// <https://www.w3.org/TR/CSS22/tables.html#table-layers> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn finalize_table_body_paint_fragment(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        columns: &[TableColumn<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        _table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        table_is_document_canvas: bool,
    ) {
        let Some(fragment_state) = fragment.take() else {
            return;
        };
        debug_assert!(
            self.fragmentainer_materializes_cursor(fragment_state.plan.fragmentainer_kind)
        );
        if fragment_state.plan.body_rows.is_empty()
            || fragment_state.plan.page_index != self.pages.len()
        {
            return;
        }
        let repeated_rows = fragment_state.repeated_rows();
        debug_assert!(repeated_rows.iter().all(|row| *row < rows.len()));
        debug_assert!(
            !fragment_state.starts_after_break()
                || fragment_state.plan.break_reason() != TableFragmentBreakReason::TableStart
        );
        debug_assert!(
            !fragment_state.has_split_or_collapsed_rows()
                || fragment_state
                    .plan
                    .body_rows
                    .iter()
                    .any(|row| row.collapsed || row.fragment_mode != TableRowFragmentMode::Whole)
        );
        let mut fragment = self
            .current_page
            .paint_tree_fragment_since(&fragment_state.checkpoint);

        let bottom = fragment_state.bottom();
        let fragment_has_occupied_row = fragment_state
            .plan
            .body_rows
            .iter()
            .any(|row| !row.collapsed);
        let vertical_edge_spacing =
            table_vertical_edge_spacing(&[fragment_has_occupied_row], table_metrics.clone());
        let (structural_backgrounds, structural_outlines) = self
            .table_body_fragment_structural_primitives(
                rows,
                grid,
                columns,
                table_style,
                stylesheets,
                table_x,
                used_table_width,
                table_width,
                table_metrics.clone(),
                column_plan,
                &fragment_state,
            );
        self.current_page.prepend_recorded_primitives_to_fragment(
            &mut fragment,
            PaintBand::BackgroundBorder,
            structural_backgrounds,
        );
        self.current_page.append_recorded_primitives_to_fragment(
            &mut fragment,
            PaintBand::Outline,
            structural_outlines,
        );

        if table_metrics.border_collapse != css::BorderCollapse::Collapse {
            let border_box_top = fragment_state.plan.fragment_top
                + vertical_edge_spacing
                + table_width.padding.top
                + table_width.border_widths.top;
            let border_box_bottom = bottom
                - vertical_edge_spacing
                - table_width.padding.bottom
                - table_width.border_widths.bottom;
            let border_box_height = border_box_top - border_box_bottom;
            if border_box_height > 0.0 {
                let border_box_x =
                    table_x - table_width.padding.left - table_width.border_widths.left;
                let border_box_width = used_table_width
                    + table_width.padding.left
                    + table_width.padding.right
                    + table_width.border_widths.left
                    + table_width.border_widths.right;
                let mut border_rects = Vec::new();
                let mut border_paths = Vec::new();
                let unfragmented_grid = fragment_state.plan.body_rows.len() == rows.len()
                    && fragment_state
                        .plan
                        .body_rows
                        .iter()
                        .enumerate()
                        .all(|(index, row)| {
                            row.row_index == index
                                && row.fragment_mode == TableRowFragmentMode::Whole
                        });
                let border_box = if unfragmented_grid {
                    fragment_state.grid_placement.map(|placement| {
                        let grid = placement.full_page_top_rect();
                        let padding = PageTopRect::new(
                            grid.x() - table_width.padding.left,
                            grid.top_y() + table_width.padding.top,
                            grid.width() + table_width.padding.left + table_width.padding.right,
                            grid.height() + table_width.padding.top + table_width.padding.bottom,
                        );
                        PageTopRect::new(
                            padding.x() - table_width.border_widths.left,
                            padding.top_y() + table_width.border_widths.top,
                            padding.width()
                                + table_width.border_widths.left
                                + table_width.border_widths.right,
                            padding.height()
                                + table_width.border_widths.top
                                + table_width.border_widths.bottom,
                        )
                    })
                } else {
                    Some(PageTopRect::new(
                        border_box_x,
                        border_box_top,
                        border_box_width,
                        border_box_height,
                    ))
                };
                if let Some(border_box) = border_box {
                    paint_table_border_edges(
                        &mut border_rects,
                        &mut border_paths,
                        border_box,
                        table_style,
                    );
                    let mut border_primitives = Vec::new();
                    border_primitives.extend(border_rects.into_iter().map(PaintPrimitive::Rect));
                    border_primitives.extend(border_paths.into_iter().map(PaintPrimitive::Path));
                    self.current_page.append_recorded_primitives_to_fragment(
                        &mut fragment,
                        PaintBand::BackgroundBorder,
                        border_primitives,
                    );
                }
            }
        }

        if table_metrics.border_collapse == css::BorderCollapse::Collapse
            && table_style.visibility == Visibility::Visible
        {
            let borders = collapsed_geometry
                .map(|geometry| {
                    self.collapsed_table_fragment_border_primitives(
                        geometry,
                        table_x,
                        column_plan,
                        &fragment_state,
                    )
                })
                .unwrap_or_default();
            // CSS 2.2 Appendix E paints collapsed table borders in the table's
            // background/border phase, before foreground cell contents:
            // <https://www.w3.org/TR/CSS22/zindex.html>.
            self.current_page.prepend_recorded_primitives_to_fragment(
                &mut fragment,
                PaintBand::TableCollapsedBorder,
                borders,
            );
        }

        let bounds_x = table_x - table_width.padding.left - table_width.border_widths.left;
        let bounds_top = fragment_state.plan.fragment_top
            + vertical_edge_spacing
            + table_width.padding.top
            + table_width.border_widths.top;
        let bounds_bottom = bottom
            - vertical_edge_spacing
            - table_width.padding.bottom
            - table_width.border_widths.bottom;
        let bounds = PageTopRect::new(
            bounds_x,
            bounds_top,
            used_table_width
                + table_width.padding.left
                + table_width.padding.right
                + table_width.border_widths.left
                + table_width.border_widths.right,
            bounds_top - bounds_bottom,
        )
        .paint_clip();
        let overflow_clip = table_box_overflow_clip(
            table_style,
            table_padding_box_clip_from_border_box(bounds, table_width),
            table_is_document_canvas,
        );
        let parent_band = if self
            .ancestors
            .iter()
            .rev()
            .skip(1)
            .any(|ancestor| ancestor.tag.eq_ignore_ascii_case("td"))
        {
            PaintBand::Inline
        } else {
            PaintBand::InFlowBlock
        };
        let mut policy =
            table_atomic_stacking_policy(table_style, parent_band, bounds, overflow_clip);
        let mut child_contexts = self.positioned_child_contexts_since(
            fragment_state.positioned_layer_start,
            fragment_state.plan.page_index,
            policy,
        );
        let fragment = if let Some(overflow_clip) = policy.effects.overflow_clip.take() {
            // CSS table overflow clips the grid and its descendants, but not
            // the table's own background, border, collapsed-border phase, or
            // outline. Keep decoration bands outside an in-band clip scope so
            // each committed table fragment remains independent.
            // <https://www.w3.org/Style/css2-updates/REC-CSS2-20110607-errata.html#s.11.1.1b>
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
            fragment.with_contents_effect_scoped_to_rect_and_child_contexts(
                overflow_clip,
                std::mem::take(&mut child_contexts),
            )
        } else {
            fragment
        };
        // A table nested in a cell is emitted as the cell's foreground
        // content, after the enclosing table's collapsed-border phase. Other
        // non-positioned tables remain in the ordinary block band, so their
        // own collapsed border remains after earlier in-flow block backgrounds.
        // <https://www.w3.org/TR/CSS22/zindex.html>
        self.scope_current_page_fragment_with_policy(
            &fragment_state.checkpoint,
            policy,
            bounds,
            fragment,
            child_contexts,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn layout_table_row_paint_piece(
        &mut self,
        row_index: usize,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_height_is_definite: bool,
        table_metrics: TableMetrics,
        row_top: f32,
        row_height: f32,
        piece_height: f32,
        piece_offset: f32,
        row_fragment_mode: TableRowFragmentMode,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        row_baseline_offset: Option<f32>,
    ) {
        let row_paint_checkpoint = self.current_page.paint_checkpoint();
        let row_paint_page_index = self.pages.len();
        let row_positioned_layer_start = self.positioned_layers.len();
        debug_assert!(
            row_fragment_mode != TableRowFragmentMode::Sliced
                || (piece_height > 0.0 && row_height > 0.0)
        );
        let row_piece_clip_active = if row_fragment_mode.clips_to_row_piece() {
            self.push_overflow_clip(
                PageTopRect::new(table_x, row_top, used_table_width, piece_height).overflow_clip(),
            );
            true
        } else {
            false
        };
        let content_row_top = row_top + piece_offset;
        // CSS Containment does not apply to table row tracks or row groups:
        // they have no containment principal box. Keep transforms independent
        // because they do establish their own containing-block effects.
        // <https://www.w3.org/TR/css-contain-1/#containment-principal>
        let row_has_applicable_containment = row.element.is_some_and(|element| {
            property_containment_applies_to_element(element, row_style)
                && (row_style.contain.layout || row_style.contain.paint)
        });
        let row_group_has_applicable_containment = row
            .row_groups
            .last()
            .map(|group| {
                let style = self.style_for_table_row_group(group, table_style, stylesheets);
                property_containment_applies_to_element(group.element, &style)
                    && (style.contain.layout || style.contain.paint)
            })
            .unwrap_or(false);
        let row_group_establishes_containing_block = row_style.has_transform()
            || row_has_applicable_containment
            || row
                .row_groups
                .last()
                .map(|group| {
                    self.style_for_table_row_group(group, table_style, stylesheets)
                        .has_transform()
                })
                .unwrap_or(false)
            || row_group_has_applicable_containment;
        let row_group_containing_block_scope = if row_group_establishes_containing_block {
            let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
                table_x,
                row_top,
                used_table_width,
                piece_height,
            ));
            Some(self.push_positioned_containing_block(
                PositionedContainingBlockMode::FixedAndAbsolute,
                containing_block,
            ))
        } else {
            None
        };
        // Row and column tracks are logical table-grid coordinates.  The
        // existing row-layout pass supplies their used sizes; this is the
        // single boundary where those tracks become physical page geometry.
        // <https://drafts.csswg.org/css-tables-3/#table-layout>
        // <https://drafts.csswg.org/css-writing-modes-4/#abstract-box>
        let table_axes = TableAxes::for_style(table_style);
        let logical_inline_extent = column_plan.total_width();
        let logical_block_extent = table_grid_height(
            planned_row_heights,
            planned_row_occupancy,
            table_metrics.clone(),
        );
        let row_block_start = table_row_block_start(
            planned_row_heights,
            planned_row_occupancy,
            row_index,
            table_metrics.clone(),
        );
        let grid_origin_top = content_row_top + row_block_start;

        for placement in &grid.rows[row_index] {
            let cell = &row.cells[placement.cell];
            let Some(prepared) = self.prepare_table_cell(
                cell,
                row,
                row_style,
                placement,
                row_index,
                table_x,
                stylesheets,
                table_cellpadding,
                column_plan,
                table_metrics.clone(),
                collapsed_geometry,
            ) else {
                continue;
            };
            let cell_style = &prepared.style;
            let cell_paint_checkpoint = self.current_page.paint_checkpoint();
            let cell_paint_page_index = self.pages.len();
            let cell_positioned_layer_start = self.positioned_layers.len();
            let cell_borders = prepared.borders;
            let metrics = prepared.metrics;
            let row_span_block_size = table_row_span_height(
                planned_row_heights,
                planned_row_occupancy,
                row_index,
                placement.rowspan,
                table_metrics.clone(),
            );
            // `TableCellLayoutMetrics::border_box_height` is a physical Y
            // measurement. A vertical table's row track instead occupies the
            // physical X axis, so its final cell block size must remain in
            // the table grid's logical block coordinate system.
            // <https://drafts.csswg.org/css-tables-3/#row-layout>
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            let cell_min_block_size = if table_style.writing_mode.has_vertical_lines() {
                table_cell_content_max_width(
                    self,
                    cell,
                    cell_style,
                    stylesheets,
                    Some(cell_borders),
                )
            } else {
                metrics.border_box_height
            };
            let cell_height = row_span_block_size.max(cell_min_block_size);
            let cell_placement = TableGridPlacement::with_axes(
                PageTopPoint::new(table_x, grid_origin_top),
                table_axes,
                logical_inline_extent,
                logical_block_extent,
            );
            let cell_border_box = column_plan.cell_border_box(
                prepared.area,
                TableRowBounds::new(row_block_start + piece_offset, cell_height),
            );
            let cell_width = cell_border_box.width();
            let text = prepared.text;
            let final_cell_content_height = (cell_height
                - cell_borders.top
                - cell_borders.bottom
                - cell_style.padding.top
                - cell_style.padding.bottom)
                .max(0.0);
            let table_height_is_definite_for_cell = table_height_is_definite
                || matches!(
                    table_style.box_values.height,
                    css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
                ) && cell.children.as_deref().is_some_and(
                    Self::table_cell_children_have_cyclic_percentage_scroll_min_height,
                );
            let percentage_height_basis = table_cell_percentage_height_basis(
                &prepared.row_sizing_style,
                table_style,
                final_cell_content_height,
                cell_borders,
                table_height_is_definite_for_cell,
            );
            let final_metrics = self.table_cell_final_relayout_metrics(
                cell,
                cell_style,
                stylesheets,
                cell_width,
                cell_borders,
                metrics,
                percentage_height_basis,
            );
            let cell_is_empty = text.is_empty() && final_metrics.content_height <= 0.0;
            let baseline_context = TableCellBaselineAlignmentContext {
                row_index,
                row_style,
                table_style,
                rows,
                grid,
                stylesheets,
                table_cellpadding,
                column_plan,
                planned_row_heights,
                planned_row_occupancy,
                table_metrics: table_metrics.clone(),
                collapsed_geometry,
                row_baseline_offset,
            };
            let cell_row_baseline_offset = if percentage_height_basis.is_definite()
                && cell.children.as_deref().is_some_and(|children| {
                    children
                        .iter()
                        .any(table_cell_formatting_child_has_parent_percentage_block_size)
                }) {
                // A cell whose percentage-dependent content was relaid out
                // has a new in-flow baseline. Use that committed content
                // metric instead of the provisional row-minimum baseline.
                Some(final_metrics.baseline_offset)
            } else {
                self.table_cell_row_baseline_offset_for_alignment(
                    &baseline_context,
                    placement,
                    cell_style,
                )
            };
            let content_offset = table_cell_content_offset(
                cell_style,
                cell_borders,
                final_metrics.content_height,
                cell_height,
                cell_row_baseline_offset,
                final_metrics.baseline_offset,
            );
            let content_x_offset = self.table_cell_content_x_offset(
                cell,
                cell_style,
                stylesheets,
                cell_width,
                cell_borders,
            );
            let collapsed_content_clip = self.collapsed_rowspan_cell_content_clip(
                row_index,
                placement.rowspan,
                rows,
                table_style,
                stylesheets,
                planned_row_heights,
                planned_row_occupancy,
                table_metrics.clone(),
                cell_border_box,
                cell_placement,
            );
            let collapsed_column_content_clip = column_plan
                .span_contains_collapsed_column(placement.column, placement.colspan)
                .then(|| cell_placement.overflow_clip_for(cell_border_box.rect()));
            let collapsed_content_clip =
                match (collapsed_content_clip, collapsed_column_content_clip) {
                    (Some(rows), Some(columns)) => rows.intersect(columns),
                    (Some(clip), None) | (None, Some(clip)) => Some(clip),
                    (None, None) => None,
                };
            let paint_containment_clip = self.table_cell_content_clip(
                cell_style,
                cell_border_box,
                cell_placement,
                cell_borders,
            );
            let content_clip = match (collapsed_content_clip, paint_containment_clip) {
                (Some(collapsed), Some(containment)) => collapsed.intersect(containment),
                (Some(clip), None) | (None, Some(clip)) => Some(clip),
                (None, None) => None,
            };
            let mut cell_fragment_plan = TableCellFragmentPlan {
                border_box: cell_border_box,
                placement: cell_placement,
                content_offset,
                content_x_offset,
                content_clip,
                area: prepared.area,
                content: TableCellContentPlan::empty(),
            };
            cell_fragment_plan.content = self.plan_table_cell_content(
                cell,
                row,
                cell_style,
                stylesheets,
                cell_width,
                cell_borders,
                &text,
                content_row_top,
                content_offset,
                row_top,
                piece_height,
                row_fragment_mode,
                percentage_height_basis,
            );
            debug_assert_eq!(cell_fragment_plan.area.row, row_index);
            debug_assert_eq!(cell_fragment_plan.area.column, placement.column);
            debug_assert_eq!(cell_fragment_plan.area.colspan, placement.colspan.max(1));
            debug_assert_eq!(cell_fragment_plan.area.rowspan, placement.rowspan.max(1));

            if self.capture_table_cell_fragment_assignments(
                cell,
                cell_style,
                &cell_fragment_plan,
                piece_offset,
            ) {
                continue;
            }

            // A transformed table cell establishes both the absolute and
            // fixed positioning containing block from its border box, just
            // like any other transformed CSS box. Table cell content scopes
            // otherwise only carry their content-area geometry.
            // <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
            let cell_establishes_containing_block =
                cell_style.has_transform() || cell_style.contain.layout || cell_style.contain.paint;
            let cell_containing_block_scope = if cell_establishes_containing_block {
                let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
                    cell_fragment_plan.x(),
                    cell_fragment_plan.top_y(),
                    cell_fragment_plan.width(),
                    cell_fragment_plan.height(),
                ));
                Some(self.push_positioned_containing_block(
                    PositionedContainingBlockMode::FixedAndAbsolute,
                    containing_block,
                ))
            } else {
                None
            };

            let paint_empty_cell = table_metrics.border_collapse == css::BorderCollapse::Collapse
                || cell_style.empty_cells == EmptyCells::Show
                || !cell_is_empty;

            let cell_has_paintable_area =
                cell_fragment_plan.width() > 0.0 && cell_fragment_plan.height() > 0.0;

            if paint_empty_cell && cell_has_paintable_area {
                // Table-cell backgrounds and borders are table decorations in
                // CSS 2.2 Appendix E; cell foreground content paints later.
                // <https://www.w3.org/TR/CSS22/zindex.html>.
                let mut cell_paint_style = cell_style.clone();
                if row_fragment_mode.clips_to_row_piece() {
                    // `box-decoration-break` initially slices a cell's
                    // decoration through an oversized row. Only the source
                    // row's real block-start/end edges paint horizontal
                    // borders; continuation boundaries retain the vertical
                    // sides but must not manufacture table rules.
                    // <https://www.w3.org/TR/css-break-3/#box-decoration-break>
                    if piece_offset > 0.01 {
                        cell_paint_style.border_widths.top = 0.0;
                        cell_paint_style.border_styles.top = css::BorderStyle::None;
                    }
                    if piece_offset + piece_height < row_height - 0.01 {
                        cell_paint_style.border_widths.bottom = 0.0;
                        cell_paint_style.border_styles.bottom = css::BorderStyle::None;
                    }
                }
                let (rects, rounded_rects, paths, strokes) = block_paint_ops_with_border_insets(
                    paint_space_rect(
                        cell_fragment_plan.x(),
                        cell_fragment_plan.top_y() - cell_fragment_plan.height(),
                        cell_fragment_plan.width(),
                        cell_fragment_plan.height(),
                    ),
                    &cell_paint_style,
                    cell_borders,
                    table_metrics.border_collapse != css::BorderCollapse::Collapse,
                );
                for rect in rects {
                    self.push_rect_in_band(PaintBand::BackgroundBorder, rect);
                }
                for rounded_rect in rounded_rects {
                    self.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rounded_rect);
                }
                for path in paths {
                    self.push_path_in_band(PaintBand::BackgroundBorder, path);
                }
                for stroke in strokes {
                    self.push_stroke_in_band(PaintBand::BackgroundBorder, stroke);
                }
            }

            // Paint containment is applied once when the cell's retained
            // paint subtree is scoped below.  Applying the same rectangle to
            // the immediate primitive stack as well produces two independent
            // antialiased clip edges at an inline background boundary.
            // Ordinary overflow and row-fragment clipping still needs the
            // immediate stack to cull content while it is laid out.
            let clip_active = if !cell_style.contain.paint
                && let Some(clip) = cell_fragment_plan.content_clip
            {
                self.push_overflow_clip(clip);
                true
            } else {
                false
            };

            let inline_sequence_paints_cell_children = cell_fragment_plan
                .content
                .children_painted_by_inline_sequence;
            let planned_flow_children = row_fragment_mode.replays_flow_children_from_plan();
            if cell_has_paintable_area
                && (cell_fragment_plan.content.inline_sequence.is_some()
                    || (cell.children.is_none() && !text.is_empty()))
            {
                let content_box = cell_fragment_plan.content_box(cell_style, cell_borders);
                let content_scope = self.enter_table_cell_content_scope(
                    cell_style,
                    content_box,
                    self.table_cell_child_ancestors(cell, row),
                    percentage_height_basis,
                );
                self.push_float_context();
                if let Some(sequence) = &cell_fragment_plan.content.inline_sequence {
                    if row_fragment_mode.clips_to_row_piece() {
                        // Each row piece occupies the same fragment-local
                        // cell geometry, while its inline sequence remains in
                        // source-row coordinates. Project the source origin
                        // by the amount already consumed before selecting the
                        // visible lines for this piece.
                        // <https://www.w3.org/TR/css-break-3/#box-splitting>
                        self.paint_inline_line_sequence_slice(
                            sequence,
                            cell_style,
                            self.cursor_y + piece_offset,
                            row_top,
                            row_top - piece_height,
                        );
                    } else {
                        self.paint_inline_line_sequence(sequence, cell_style);
                    }
                } else if row_fragment_mode.clips_to_row_piece() {
                    if let Some(element) = cell.element {
                        self.paint_element_inline_block_slice(
                            element,
                            cell_style,
                            stylesheets,
                            0.0,
                            0.0,
                            table_cell_href(cell),
                            self.cursor_y,
                            row_top,
                            row_top - piece_height,
                        );
                    } else {
                        self.paint_text_block_slice(
                            &text,
                            cell_style,
                            0.0,
                            0.0,
                            table_cell_href(cell),
                            self.cursor_y,
                            row_top,
                            row_top - piece_height,
                        );
                    }
                } else if let Some(element) = cell.element {
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
            if cell_has_paintable_area && !inline_sequence_paints_cell_children {
                if planned_flow_children {
                    self.paint_table_cell_planned_child_fragments(
                        cell,
                        row,
                        cell_style,
                        stylesheets,
                        cell_borders,
                        cell_fragment_plan.border_box,
                        cell_fragment_plan.placement,
                        cell_fragment_plan.content_offset,
                        cell_fragment_plan.content_x_offset,
                        &cell_fragment_plan.content.child_fragments,
                    );
                } else {
                    self.layout_table_cell_flow_children(
                        cell,
                        row,
                        cell_style,
                        &prepared.row_sizing_style,
                        table_style,
                        table_height_is_definite_for_cell,
                        stylesheets,
                        cell_borders,
                        cell_fragment_plan.border_box,
                        cell_fragment_plan.placement,
                        cell_fragment_plan.content_offset,
                        cell_fragment_plan.content_x_offset,
                    );
                }
            }
            if cell_has_paintable_area && !planned_flow_children {
                self.layout_table_cell_replaced_children(
                    cell,
                    cell_style,
                    cell_borders,
                    cell_fragment_plan.border_box,
                    cell_fragment_plan.placement,
                    cell_fragment_plan.content_offset,
                    cell_fragment_plan.content_x_offset,
                );
            }
            self.layout_table_cell_positioned_children(
                cell,
                row,
                row_style,
                Some(ContainingBlock::from_page_top_rect(PageTopRect::new(
                    table_x,
                    content_row_top,
                    used_table_width,
                    piece_height,
                ))),
                cell_style,
                stylesheets,
                cell_borders,
                cell_fragment_plan.border_box,
                cell_fragment_plan.placement,
                cell_fragment_plan.content_offset,
                cell_fragment_plan.content_x_offset,
            );
            self.pop_overflow_clip(clip_active);

            // Anonymous cells inherit formatting structure from a transformed
            // row/row-group, but do not own that element's effects. A source
            // cell owns its transform and containment scope.  The immediate
            // overflow stack culls fully outside lines, while this retained
            // paint scope clips partially intersecting glyph runs in the PDF
            // output.
            // <https://www.w3.org/TR/css-contain-1/#containment-paint>
            if cell.element.is_some()
                && (cell_style.has_transform()
                    || cell_style.contain.layout
                    || cell_style.contain.paint)
                && self.pages.len() == cell_paint_page_index
            {
                let bounds = PaintClip::from_paint_rect(paint_space_rect(
                    cell_fragment_plan.x(),
                    cell_fragment_plan.top_y() - cell_fragment_plan.height(),
                    cell_fragment_plan.width(),
                    cell_fragment_plan.height(),
                ));
                let mut policy =
                    StackingContextPolicy::for_atomic(cell_style, PaintBand::InFlowBlock, bounds);
                if let Some(content_clip) = cell_fragment_plan.content_clip {
                    policy.effects.overflow_clip =
                        Some(PaintClip::from_paint_rect(content_clip.paint_rect()));
                }
                let child_contexts = self.positioned_child_contexts_since(
                    cell_positioned_layer_start,
                    cell_paint_page_index,
                    policy,
                );
                self.scope_current_page_paint_since_with_policy(
                    &cell_paint_checkpoint,
                    policy,
                    bounds,
                    child_contexts,
                );
            }
            if let Some(scope) = cell_containing_block_scope {
                self.pop_positioned_containing_block(scope);
            }
        }
        self.pop_overflow_clip(row_piece_clip_active);
        if let Some(scope) = row_group_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        let row_group_effect = row
            .row_groups
            .last()
            .map(|group| self.style_for_table_row_group(group, table_style, stylesheets))
            .filter(|style| style.has_transform());
        if (row_style.has_transform() || row_group_effect.is_some())
            && self.pages.len() == row_paint_page_index
        {
            let (effect_style, bounds) = if row_style.has_transform() {
                (
                    row_style,
                    PageTopRect::new(table_x, row_top, used_table_width, piece_height).paint_clip(),
                )
            } else {
                let start = row
                    .row_groups
                    .last()
                    .and_then(|group| {
                        rows.iter().position(|candidate| {
                            candidate.row_groups.last().is_some_and(|candidate_group| {
                                candidate_group.signature == group.signature
                            })
                        })
                    })
                    .unwrap_or(row_index);
                let end = rows
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take_while(|(_, candidate)| {
                        candidate.row_groups.last().is_some_and(|candidate_group| {
                            candidate_group.signature
                                == row.row_groups.last().expect("row group exists").signature
                        })
                    })
                    .count()
                    + start;
                let group_height = table_row_span_height(
                    planned_row_heights,
                    planned_row_occupancy,
                    start,
                    end.saturating_sub(start),
                    table_metrics,
                );
                (
                    row_group_effect
                        .as_ref()
                        .expect("group effect exists")
                        .as_computed(),
                    PageTopRect::new(
                        table_x,
                        row_top - row_block_start,
                        used_table_width,
                        group_height,
                    )
                    .paint_clip(),
                )
            };
            let policy =
                StackingContextPolicy::for_atomic(effect_style, PaintBand::InFlowBlock, bounds);
            let child_contexts = self.positioned_child_contexts_since(
                row_positioned_layer_start,
                row_paint_page_index,
                policy,
            );
            self.scope_current_page_paint_since_with_policy(
                &row_paint_checkpoint,
                policy,
                bounds,
                child_contexts,
            );
        }
    }

    /// Detect a cyclic percentage-height scroll container with an independent
    /// minimum block-size constraint below a table cell.
    ///
    /// CSS Tables first obtains the row minimum from that constraint, then
    /// resolves the descendant percentage during the final cell-content pass:
    /// <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout>.
    fn table_cell_children_have_cyclic_percentage_scroll_min_height(
        children: &[box_tree::FormattingBox<'_>],
    ) -> bool {
        children
            .iter()
            .any(Self::table_cell_box_has_cyclic_percentage_scroll_min_height)
    }

    fn table_cell_box_has_cyclic_percentage_scroll_min_height(
        box_: &box_tree::FormattingBox<'_>,
    ) -> bool {
        let (style, children) = match box_ {
            box_tree::FormattingBox::Block(box_) => (box_.style.as_ref(), &box_.children),
            box_tree::FormattingBox::Inline(box_) => (box_.style.as_ref(), &box_.children),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                (box_.style.as_ref(), &box_.children)
            }
            box_tree::FormattingBox::AnonymousBlock(box_) => (box_.style.as_ref(), &box_.children),
            box_tree::FormattingBox::AtomicInline(box_) => (box_.style.as_ref(), &box_.children),
            box_tree::FormattingBox::Table(box_) => (box_.style.as_ref(), &box_.children),
            box_tree::FormattingBox::Flex(box_) => (box_.style.as_ref(), &box_.children),
            box_tree::FormattingBox::Replaced(box_) => (box_.style.as_ref(), &box_.children),
            box_tree::FormattingBox::Text(_) => return false,
        };
        table_cell_block_size_depends_on_parent_percentage(style.box_values.height.clone())
            && style.box_values.min_height.length_if_no_percent().is_some()
            && matches!(
                effective_overflow_for_style(style),
                css::Overflow::Auto | css::Overflow::Scroll
            )
            || Self::table_cell_children_have_cyclic_percentage_scroll_min_height(children)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn plan_table_cell_content(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        cell_width: f32,
        cell_borders: css::Edges,
        text: &str,
        content_row_top: f32,
        content_offset: f32,
        row_top: f32,
        piece_height: f32,
        row_fragment_mode: TableRowFragmentMode,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> TableCellContentPlan {
        let mut plan = TableCellContentPlan::empty();
        let available_width = (cell_width
            - cell_borders.left
            - cell_borders.right
            - cell_style.padding.left
            - cell_style.padding.right)
            .max(1.0);

        if let Some(children) = cell.children.as_deref()
            && table_cell_children_can_use_inline_line_sequence(children)
        {
            let mut items = Vec::new();
            let link_target = table_cell_href(cell).map(str::to_string);
            self.with_table_cell_inline_planning_scope(
                cell_style,
                available_width,
                percentage_height_basis,
                |layout| {
                    layout.collect_inline_box_items(
                        children,
                        stylesheets,
                        link_target.clone(),
                        0.0,
                        InlineVisualOffset::zero(),
                        cell_style,
                        cell_style.text_decoration.clone(),
                        &mut items,
                    );
                },
            );
            if !items.is_empty() {
                plan.inline_sequence = Some(
                    self.collect_inline_line_sequence_for_text_box_trimmed_style(
                        items,
                        cell_style,
                        available_width,
                    ),
                );
                plan.children_painted_by_inline_sequence = true;
                return plan;
            }
        }

        if cell.children.is_none() {
            if let Some(element) = cell.element {
                let mut items = Vec::new();
                let link_target = table_cell_href(cell).map(str::to_string);
                self.with_table_cell_inline_planning_scope(
                    cell_style,
                    available_width,
                    percentage_height_basis,
                    |layout| {
                        layout.push_generated_pseudo_items(
                            element,
                            cell_style,
                            cell_style.before_style.as_deref(),
                            link_target.clone(),
                            0.0,
                            InlineVisualOffset::zero(),
                            GeneratedPseudoCounterMode::Commit,
                            &mut items,
                        );
                        layout.collect_element_content_or_inline_items(
                            element,
                            cell_style,
                            stylesheets,
                            link_target.clone(),
                            InlinePlacement::zero(),
                            &mut items,
                        );
                        layout.push_generated_pseudo_items(
                            element,
                            cell_style,
                            cell_style.after_style.as_deref(),
                            link_target,
                            0.0,
                            InlineVisualOffset::zero(),
                            GeneratedPseudoCounterMode::Commit,
                            &mut items,
                        );
                    },
                );
                if !items.is_empty() {
                    plan.inline_sequence = Some(
                        self.collect_inline_line_sequence_for_text_box_trimmed_style(
                            items,
                            cell_style,
                            available_width,
                        ),
                    );
                }
            } else if !text.is_empty() {
                let cell_text_box_line_trim =
                    self.effective_text_box_line_trim_for_style(cell_style);
                plan.inline_sequence = Some(self.with_text_box_line_trim_scope(
                    cell_text_box_line_trim,
                    |layout| {
                        layout.inline_line_sequence_for_text(
                            text,
                            cell_style,
                            available_width,
                            0.0,
                            table_cell_href(cell),
                        )
                    },
                ));
            }
        }

        if row_fragment_mode.replays_flow_children_from_plan()
            && let Some(children) = cell.children.as_deref()
        {
            let child_top =
                content_row_top - cell_borders.top - cell_style.padding.top - content_offset;
            plan.child_fragments = self.table_cell_child_fragment_plans(
                children,
                stylesheets,
                available_width,
                percentage_height_basis,
                child_top,
                row_top,
                row_top - piece_height,
            );
            for child_plan in &mut plan.child_fragments {
                if child_plan.kind == TableCellChildFragmentKind::NestedFormattingContext
                    && let Some(child_box) = children.get(child_plan.source_child_index)
                {
                    child_plan.nested_fragment = self
                        .plan_table_cell_nested_child_fragment(
                            cell,
                            row,
                            cell_style,
                            child_box,
                            stylesheets,
                            available_width,
                        )
                        .map(|mut nested_fragment| {
                            nested_fragment.metadata = child_plan.metadata.clone();
                            nested_fragment
                        });
                }
            }
        }

        plan
    }

    /// Captures GCPM assignments from a table-cell source's final visible fragment.
    ///
    /// Table cells are internal table boxes and do not pass through the normal
    /// block element wrapper, but CSS GCPM named strings and running elements
    /// are set by the source element's generated box. Use the page-local cell
    /// fragment as the source position for `string(..., start)` and
    /// `element(..., start)`, and skip source-cell paint when `position:
    /// running()` removes the cell from normal flow:
    /// <https://www.w3.org/TR/css-gcpm-3/#setting-named-strings>,
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements>, and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn capture_table_cell_fragment_assignments(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        cell_fragment_plan: &TableCellFragmentPlan,
        piece_offset: f32,
    ) -> bool {
        if piece_offset > 0.01 {
            return false;
        }
        let Some(element) = cell.element else {
            return false;
        };
        let placement = AssignmentPlacement {
            page_index: self.pages.len(),
            starts_page_fragment: !self.current_page_has_content(),
            border_box: Some(
                PageTopRect::new(
                    cell_fragment_plan.x(),
                    cell_fragment_plan.top_y(),
                    cell_fragment_plan.width(),
                    cell_fragment_plan.height(),
                )
                .paint_clip(),
            ),
        };
        self.capture_assignments_for_fragment_source(element, cell_style, placement)
    }

    pub(in crate::layout::table) fn table_cell_nested_inline_sequence_for_child(
        &mut self,
        child_box: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> Option<TableCellNestedInlineSequencePlan> {
        let style = match child_box {
            box_tree::FormattingBox::Text(box_) => &box_.style,
            box_tree::FormattingBox::Inline(box_) => &box_.style,
            box_tree::FormattingBox::AnonymousBlock(box_) => &box_.style,
            box_tree::FormattingBox::Block(_)
            | box_tree::FormattingBox::InlineSplitBlockContext(_)
            | box_tree::FormattingBox::AtomicInline(_)
            | box_tree::FormattingBox::Table(_)
            | box_tree::FormattingBox::Flex(_)
            | box_tree::FormattingBox::Replaced(_) => return None,
        };
        self.table_cell_nested_inline_sequence_for_children(
            style,
            std::slice::from_ref(child_box),
            stylesheets,
            None,
            available_width,
            percentage_height_basis,
        )
    }

    pub(in crate::layout::table) fn table_cell_nested_inline_sequence_for_children(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> Option<TableCellNestedInlineSequencePlan> {
        let mut items = Vec::new();
        self.with_table_cell_inline_planning_scope(
            style,
            available_width,
            percentage_height_basis,
            |layout| {
                layout.collect_inline_box_items(
                    children,
                    stylesheets,
                    inherited_link,
                    0.0,
                    InlineVisualOffset::zero(),
                    style,
                    style.text_decoration.clone(),
                    &mut items,
                );
            },
        );
        (!items.is_empty()).then(|| TableCellNestedInlineSequencePlan {
            sequence: self.collect_inline_line_sequence_for_text_box_trimmed_style(
                items,
                style,
                available_width,
            ),
            style: style.clone(),
        })
    }

    /// Collect a table-cell inline sequence with the source block container's
    /// own CSS Inline `text-box-trim` request active.
    ///
    /// Table cells and nested table-cell child slices bypass normal block-flow
    /// inline layout, but CSS Inline still applies `text-box-trim` to their
    /// first and/or last formatted lines:
    /// <https://drafts.csswg.org/css-inline-3/#text-box-trim>.
    fn collect_inline_line_sequence_for_text_box_trimmed_style(
        &mut self,
        items: Vec<InlineItem>,
        style: &ComputedStyle,
        available_width: f32,
    ) -> inline_layout::InlineLineSequence {
        self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_width.max(1.0),
            0.0,
            0.0,
        )
    }

    /// Run table-cell inline planning with the cell content box as containing block.
    ///
    /// Inline atom construction resolves percentage inline sizes against the
    /// active containing block while items are collected, before line selection
    /// sees the final `available_width`. Table cells bypass ordinary block
    /// layout during row planning, so install the same content-width basis that
    /// final cell painting uses:
    /// <https://www.w3.org/TR/CSS22/box.html#the-width-property>,
    /// <https://www.w3.org/TR/CSS22/tables.html#model>, and
    /// <https://drafts.csswg.org/css-tables/#row-layout>.
    pub(in crate::layout::table) fn with_table_cell_inline_planning_scope<T>(
        &mut self,
        style: &ComputedStyle,
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let available_width = available_width.max(1.0);
        let content_left = self.content_left;
        let content_right = self.content_right;
        let content_logical_inline_size_stack = self.content_logical_inline_size_stack.clone();
        let child_available_space_stack = self.child_available_space_stack.clone();
        let definite_block_size_stack = self.definite_block_size_stack.clone();

        self.content_left = 0.0;
        self.content_right = available_width;
        self.content_logical_inline_size_stack.push(available_width);
        self.child_available_space_stack
            .push(ChildAvailableSpace::new(
                style.writing_mode,
                PhysicalContentWidth::new(content_box_pt(available_width)),
                None,
                PhysicalContentHeight::new(content_box_pt(self.page_area_height())),
            ));
        self.definite_block_size_stack.push(percentage_height_basis);

        let result = f(self);

        self.content_left = content_left;
        self.content_right = content_right;
        self.content_logical_inline_size_stack = content_logical_inline_size_stack;
        self.child_available_space_stack = child_available_space_stack;
        self.definite_block_size_stack = definite_block_size_stack;
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_cell_child_fragment_plans(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
        mut child_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) -> Vec<TableCellChildFragmentPlan> {
        let mut plans = Vec::new();
        for (source_child_index, child_box) in children.iter().enumerate() {
            let inline_sequence = self.table_cell_nested_inline_sequence_for_child(
                child_box,
                stylesheets,
                available_width,
                percentage_height_basis,
            );
            if inline_sequence.is_none() && !table_cell_has_in_flow_layout_child(child_box) {
                continue;
            }
            let child_height = inline_sequence
                .as_ref()
                .map(|plan| plan.sequence.total_height())
                .unwrap_or_else(|| {
                    // Row-piece planning must use the same final used-size
                    // measurement as replay. A frozen box-tree style may
                    // still contain viewport-relative values, so deriving a
                    // slice height directly from its cached outer metric can
                    // discard a size-contained child's visible overflow.
                    // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
                    // <https://www.w3.org/TR/css-tables-3/#table-cell-content-relayout>
                    self.table_cell_final_relayout_child_height(
                        child_box,
                        stylesheets,
                        available_width,
                        percentage_height_basis,
                    )
                });
            if child_height <= 0.0 {
                continue;
            }
            let child_bottom = child_top - child_height;
            // Row-piece boundaries are independently accumulated while a
            // cell's child positions are measured. Treat positions that
            // differ only by the layout-coordinate tolerance as touching so
            // that a following child at a fragment boundary is replayed on
            // its destination page instead of being dropped between slices.
            const FRAGMENT_COORDINATE_EPSILON: f32 = 0.01;
            if child_top + FRAGMENT_COORDINATE_EPSILON >= slice_bottom
                && child_bottom <= slice_top + FRAGMENT_COORDINATE_EPSILON
                && let Some(kind) = table_cell_child_fragment_kind(child_box)
            {
                let visible_top = child_top.min(slice_top);
                let visible_bottom = child_bottom.max(slice_bottom);
                let mut metadata = FragmentPageMetadata::new(
                    self.pages.len(),
                    Some(
                        PageTopRect::new(
                            0.0,
                            visible_top,
                            available_width,
                            visible_top - visible_bottom,
                        )
                        .paint_clip(),
                    ),
                    (child_top - slice_top).abs() <= 0.01,
                );
                metadata.continues_from_previous_page = child_top > slice_top + 0.01;
                metadata.continues_to_next_page = child_bottom < slice_bottom - 0.01;
                plans.push(TableCellChildFragmentPlan {
                    source_child_index,
                    child_top,
                    child_height,
                    slice_top,
                    slice_bottom,
                    kind,
                    inline_sequence,
                    nested_fragment: None,
                    metadata,
                });
            }
            child_top = child_bottom;
        }
        plans
    }
}
