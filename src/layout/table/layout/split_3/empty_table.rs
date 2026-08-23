use crate::css::{
    self, CaptionSide, ComputedStyle, DisplayInner, ElementSignature, EmptyCells, PercentageBasis,
    Position, SemanticLengthExt, Stylesheets, Visibility, layout_pt,
};
use crate::document::paint::display_list::PaintBand;
use crate::document::paint::geometry::PaintTranslation;
use crate::document::paint::shapes::RenderedRect;
use crate::dom::{Element, NodeKind};
use crate::layout::table::layout::split_3::{
    collapsed_cell_decoration_style, table_columns_paint_in_reverse_page_order,
};
use crate::layout::table::layout::{
    CollapsedTableGeometry, TableCaptionContainingBlock, TableCaptionOuterWidth,
    TableCellBaselineAlignmentContext, TableCellClipRegion, TableWrapperBorderBoxOrigin,
    TableWrapperMarginBoxFootprint, TableWrapperPaintBox, table_atomic_stacking_policy,
    table_box_overflow_clip, table_column_fragment_background_image_primitives,
    table_column_fragment_background_primitives, table_outlines_use_in_flow_phase,
    table_padding_box_clip_from_border_box, table_wrapper_border_box_height,
    table_wrapper_collision_height, table_wrapper_positioning_containing_block,
};
use crate::layout::table::{
    TableAxes, TableCaption, TableCellAxisAdapter, TableCellPadding, TableColumn, TableColumnPlan,
    TableGrid, TableGridContentBoxTopLeft, TableGridLength, TableGridLogicalSize,
    TableGridPlacement, TableInlineBounds, TableMetrics, TableRow, TableRowBounds, UsedTableWidth,
    paint_table_border_edges, repeated_table_rows_height, table_cell_href,
    table_column_group_spans, table_grid_height, table_row_block_start, table_row_group_spans,
    table_row_is_collapsed, table_row_span_height, used_empty_table_grid_height,
    used_empty_table_grid_width,
};
use crate::layout::{
    AbsoluteStaticPosition, ContainingBlock, LayoutBuilder, LogicalBlockContentSize,
    LogicalInlineContentSize, PageInlinePosition, PageInlineSpan, PageTopBlockPosition,
    PageTopPoint, PageTopRect, PhysicalContentWidth, PositionedContainingBlockMode, RelativeOffset,
    assets, block_paint_ops_with_border_insets, element_sibling_signature_list,
    paint_containment_applies_to_element, paint_space_rect,
    resolve_normal_flow_auto_margins_for_known_width,
};
use crate::units::{
    BorderBoxLength, border_box_pt, content_box_pt, margin_box_pt, margin_box_size_pt,
};

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
        stylesheets: &Stylesheets<'_>,
        available_table_width: f32,
        table_width: UsedTableWidth,
    ) -> f32 {
        let content_width = used_empty_table_grid_width(style, available_table_width, table_width);
        let content_height = used_empty_table_grid_height(
            style,
            self.definite_block_size_stack
                .last()
                .cloned()
                .unwrap_or_else(PercentageBasis::indefinite),
            table_width,
            None,
            layout_pt(0.0),
        )
        .points();
        let physical_grid_width = TableGridLogicalSize::new(
            LogicalInlineContentSize::new(content_width),
            LogicalBlockContentSize::new(content_box_pt(content_height)),
        )
        .physical_width(TableAxes::for_style(style));
        style.margin.top
            + self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                physical_grid_width,
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
                physical_grid_width,
                CaptionSide::Bottom,
            )
            + style.margin.bottom
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn place_empty_table_wrapper(
        &mut self,
        captions: &[TableCaption<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_table_width: f32,
        table_width: UsedTableWidth,
        relative_offset: RelativeOffset,
        wrapper_border_box_block_size: Option<BorderBoxLength>,
    ) -> (f32, f32, f32, f32) {
        let content_width = used_empty_table_grid_width(style, available_table_width, table_width);
        let content_width_points = content_width.points();
        let provisional_caption_width = PhysicalContentWidth::new(content_width);
        let top_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            provisional_caption_width,
            CaptionSide::Top,
        );
        let bottom_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            provisional_caption_width,
            CaptionSide::Bottom,
        );
        let content_height = used_empty_table_grid_height(
            style,
            self.definite_block_size_stack
                .last()
                .cloned()
                .unwrap_or_else(PercentageBasis::indefinite),
            table_width,
            wrapper_border_box_block_size,
            layout_pt(top_caption_height + bottom_caption_height),
        )
        .points();
        let physical_grid_width = TableGridLogicalSize::new(
            LogicalInlineContentSize::new(content_width),
            LogicalBlockContentSize::new(content_box_pt(content_height)),
        )
        .physical_width(TableAxes::for_style(style));
        let border_box_width = physical_grid_width.points()
            + table_width.padding.left
            + table_width.padding.right
            + table_width.border_widths.left
            + table_width.border_widths.right;
        let mut used_style = style.clone();
        resolve_normal_flow_auto_margins_for_known_width(
            &mut used_style,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
            border_box_pt(border_box_width),
            self.containing_block_direction,
        );
        let style = &used_style;
        let collision_height = table_wrapper_collision_height(
            style,
            table_width,
            top_caption_height,
            content_height,
            bottom_caption_height,
        );

        self.cursor_y -= style.margin.top;
        self.prebreak_bfc_margin_box_if_needed(margin_box_pt(collision_height), style.margin.top);
        let placement = self.place_float_avoiding_margin_box(
            PageTopBlockPosition::new(self.cursor_y),
            margin_box_size_pt(
                style.margin.left + border_box_width + style.margin.right,
                collision_height,
            ),
            style.clear,
            self.containing_block_direction,
        );
        self.cursor_y = placement.origin.top_y();
        (
            placement.origin.x() + style.margin.left + relative_offset.x(),
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
        element: &Element,
        captions: &[TableCaption<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_table_width: f32,
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        relative_offset: RelativeOffset,
        table_is_document_canvas: bool,
        wrapper_border_box_block_size: Option<BorderBoxLength>,
    ) {
        let (table_outer_x, content_width, content_height, border_box_width) = self
            .place_empty_table_wrapper(
                captions,
                style,
                stylesheets,
                available_table_width,
                table_width,
                relative_offset,
                wrapper_border_box_block_size,
            );
        let border_box_height = table_wrapper_border_box_height(content_height, table_width);
        let table_x = table_width.content_x(table_outer_x);
        let grid_size = TableGridLogicalSize::new(
            LogicalInlineContentSize::new(content_box_pt(content_width)),
            LogicalBlockContentSize::new(content_box_pt(content_height)),
        );
        let physical_grid_width = grid_size.physical_width(TableAxes::for_style(style));

        self.push_float_context();
        let table_wrapper_top = self.cursor_y;
        let border_box_x = table_x - table_width.padding.left - table_width.border_widths.left;
        let positioning_containing_block_mode =
            PositionedContainingBlockMode::for_element(element, style);
        let paint_containment_applies = paint_containment_applies_to_element(element, style);
        let top_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            physical_grid_width,
            CaptionSide::Top,
        );
        let bottom_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            physical_grid_width,
            CaptionSide::Bottom,
        );
        let positioned_containing_block_scope = if let Some(mode) =
            positioning_containing_block_mode
        {
            let containing_block =
                ContainingBlock::from_page_top_rect(table_wrapper_positioning_containing_block(
                    table_x,
                    table_wrapper_top,
                    physical_grid_width,
                    content_height,
                    table_width,
                    top_caption_height,
                    bottom_caption_height,
                ));
            Some(self.push_positioned_containing_block(mode, containing_block))
        } else {
            None
        };
        let top_caption_paint_checkpoint = self.current_page.paint_checkpoint();
        let top_caption_paint_page_index = self.pages.len();
        let top_caption_containing_block = TableCaptionContainingBlock::new(
            PageInlineSpan::new(border_box_x, border_box_width),
            TableCaptionOuterWidth::from_border_box(border_box_pt(border_box_width)),
            TableAxes::for_style(style),
            PageInlinePosition::new(table_x),
        );
        let top_caption_outcome = self.layout_table_captions(
            captions,
            style,
            stylesheets,
            top_caption_containing_block,
            CaptionSide::Top,
        );

        // Even a rowless table retains its wrapper-flow ordering. In
        // particular, a vertical top caption can cross anonymous columns
        // before the table-root border/background is painted. Do not fall
        // back to the opening table X coordinate in this path.
        let top_caption_destination = if style.writing_mode.has_vertical_lines()
            && top_caption_outcome.next_part_requires_successor()
        {
            self.advance_table_wrapper_fragmentainer(style, top_caption_containing_block)
                .expect("an empty table following a caption needs a destination fragmentainer")
        } else {
            top_caption_outcome.final_destination()
        };

        let table_box_top = top_caption_destination.paint_top().points();
        // This is also authoritative for a rowless table: an absent top
        // caption does not make the wrapper's opening cursor a root origin.
        let table_root_border_top = table_box_top;

        let table_structure_paint_checkpoint = self.current_page.paint_checkpoint();
        let table_structure_paint_page_index = self.pages.len();
        if let Some(fill) = style.background.background_color.visible_color(style.color) {
            let border_rect = paint_space_rect(
                border_box_x,
                table_box_top - border_box_height,
                border_box_width,
                border_box_height,
            );
            let background_rect = crate::layout::paint_helpers::background_rect_area_for_box(
                border_rect,
                style,
                table_width.border_widths,
                style.background.background_clip,
            );
            if background_rect.size.width <= 0.0 || background_rect.size.height <= 0.0 {
                // A zero-sized content box has no background painting area.
                // <https://www.w3.org/TR/css-backgrounds-3/#background-clip>
            } else {
                self.push_rect_in_band(
                    PaintBand::InFlowBlock,
                    RenderedRect::from_paint_rect(background_rect, Some(fill)),
                );
            }
        }
        let table_root_paint_box = TableWrapperPaintBox {
            grid_origin: TableWrapperBorderBoxOrigin::new(PageTopPoint::new(
                border_box_x,
                table_root_border_top,
            ))
            .grid_content_box_top_left(TableAxes::for_style(style), table_width),
            axes: TableAxes::for_style(style),
            grid_size,
            table_width,
            table_metrics,
            block_edge_spacing: TableGridLength::new(0.0),
        };
        let table_wrapper_margin_box = TableWrapperMarginBoxFootprint::from_table_root_border_box(
            table_root_paint_box.clone().border_box(),
            PageTopBlockPosition::new(table_wrapper_top),
            layout_pt(top_caption_height),
            layout_pt(bottom_caption_height),
            &style.margin,
        );
        self.paint_separated_table_root_border(style, table_root_paint_box.clone());
        if !paint_containment_applies && self.pages.len() == table_structure_paint_page_index {
            let bounds = table_root_paint_box.clone().border_box().paint_clip();
            let overflow_clip = table_box_overflow_clip(
                style,
                table_root_paint_box.clone().padding_box().paint_clip(),
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

        self.cursor_y = table_root_paint_box.border_box().bottom_y();
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            TableCaptionContainingBlock::new(
                PageInlineSpan::new(border_box_x, border_box_width),
                TableCaptionOuterWidth::from_border_box(border_box_pt(border_box_width)),
                TableAxes::for_style(style),
                PageInlinePosition::new(table_x),
            ),
            CaptionSide::Bottom,
        );
        if paint_containment_applies && self.pages.len() == top_caption_paint_page_index {
            let fragment = self
                .current_page
                .take_paint_fragment_since(top_caption_paint_checkpoint);
            let wrapper_clip = table_wrapper_margin_box.page_top_rect().paint_clip();
            let fragment = fragment.with_effect_scoped_to_rect_all_bands(wrapper_clip);
            self.current_page
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
        }
        // Table fixup can leave a wrapper with no grid rows even though its
        // table-internal descendants contain out-of-flow boxes. Those boxes
        // do not create slots, but they still participate in positioned
        // layout at their hypothetical table position. In particular, paint
        // containment on a row group/header/footer/row is ignored because
        // those internal boxes have no containment-capable principal box.
        // <https://drafts.csswg.org/css-tables-3/#table-structure>
        // <https://drafts.csswg.org/css-contain-1/#containment-principal>
        let positioned_layer_start = self.positioned_layers.len();
        let previous_absolute_static_position = self.absolute_static_position;
        self.absolute_static_position = Some(AbsoluteStaticPosition::from_page_rect(
            table_x,
            table_x + physical_grid_width.points(),
            table_box_top,
        ));
        self.layout_empty_table_positioned_dom_descendants(element, style, stylesheets);
        self.flush_positioned_layers_since(positioned_layer_start);
        self.absolute_static_position = previous_absolute_static_position;
        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        self.pop_float_context();
        self.last_principal_transform_box = Some(assets::TransformReferenceBox::table_wrapper(
            table_wrapper_margin_box
                .page_top_rect()
                .paint_clip()
                .paint_rect(),
        ));
        self.cursor_y = table_wrapper_margin_box
            .horizontal_parent_block_end()
            .points();
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y();
        }
        self.apply_forced_break_after_box_in(self.active_fragmentainer_kind(), style);
    }

    /// Replay positioned descendants which table-grid construction cannot
    /// retain when every table row is empty.
    ///
    /// This walks the DOM only for the empty-grid fallback. Normal table
    /// layout owns positioned descendants through its durable row/cell
    /// fragment, which also supplies cell and row containing blocks.
    fn layout_empty_table_positioned_dom_descendants(
        &mut self,
        parent: &Element,
        parent_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
    ) {
        let sibling_tags = element_sibling_signature_list(parent);
        let mut element_index = 0usize;
        for child in &parent.children {
            let NodeKind::Element(element) = &child.kind else {
                continue;
            };
            let signature = ElementSignature::with_sibling_list(
                element.tag.clone(),
                element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let ancestors = self.ancestors.clone();
            let style = self.style_for_layout_element_with_parent_font_metrics_and_ancestors(
                element,
                signature.clone(),
                stylesheets,
                Some(parent_style),
                &ancestors,
            );
            if style.display.is_none() {
                continue;
            }
            // Captions have already been laid out as table-wrapper children.
            // An empty grid needs this fallback only for descendants the grid
            // could not visit; descending into a caption a second time
            // replays its positioned descendants against unrelated page paint
            // and violates their caption containing block and stacking order.
            // <https://www.w3.org/TR/CSS22/tables.html#model>
            if matches!(style.display.inner, DisplayInner::TableCaption) {
                continue;
            }
            self.push_ancestor_signature(signature);
            if matches!(style.position, Position::Absolute | Position::Fixed) {
                self.layout_element(element, &style, stylesheets);
            } else {
                self.layout_empty_table_positioned_dom_descendants(element, &style, stylesheets);
            }
            self.ancestors.pop();
        }
    }

    /// Paint the border of a separated-border table-root grid box.
    ///
    /// CSS 2.2's separated border model gives the table-root its own ordinary
    /// border box, distinct from row and cell borders. Collapsed borders are
    /// resolved through the collapsed-border grid instead:
    /// <https://www.w3.org/TR/CSS22/tables.html#separated-borders> and
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
    pub(in crate::layout::table) fn paint_separated_table_root_border(
        &mut self,
        style: &ComputedStyle,
        root: TableWrapperPaintBox,
    ) {
        if root.table_metrics.border_collapse == css::BorderCollapse::Collapse {
            return;
        }
        let border_box = root.border_box();
        let border_box_x = border_box.x();
        let border_box_width = border_box.width();
        let border_box_height = border_box.height();
        let mut border_rects = Vec::new();
        let mut border_paths = Vec::new();
        paint_table_border_edges(
            &mut border_rects,
            &mut border_paths,
            PageTopRect::new(
                border_box_x,
                border_box.top_y(),
                border_box_width,
                border_box_height,
            ),
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
        stylesheets: &Stylesheets<'_>,
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<TableCellPadding>,
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
            table_metrics.clone(),
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
            false,
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
        stylesheets: &Stylesheets<'_>,
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<TableCellPadding>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        scope_as_table_fragment: bool,
    ) {
        if repeated_rows.is_empty() {
            return;
        }
        let repeated_height = repeated_table_rows_height(
            repeated_rows,
            planned_row_heights,
            planned_row_occupancy,
            table_metrics.clone(),
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
        let occupied_inline_bounds = column_plan.occupied_inline_bounds().unwrap_or_else(|| {
            TableInlineBounds::new(
                TableGridLength::new(0.0),
                TableGridLength::new(used_table_width),
            )
        });
        let occupied_x = table_x + occupied_inline_bounds.logical_start().get();
        let occupied_width = occupied_inline_bounds.logical_size().get();
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
            table_metrics.clone(),
        );
        // Repeated header/footer rows are visual copies, not new source boxes.
        // Suppress element side effects while preserving paint replay.
        // <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>
        self.element_side_effect_suppression_depth += 1;
        self.out_of_flow_prebreak_suppression_depth += 1;
        // Repeated row groups are paint-only copies of source rows.  Their
        // `page` values established source class-A boundaries already; letting
        // the copies enter page-name scopes would change the destination page
        // selected for the body row that follows them.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        self.push_page_name_scope_suppression();
        for (position, row_index) in repeated_rows.iter().cloned().enumerate() {
            let row = &rows[row_index];
            let row_style = self.style_for_table_row(row, table_style, stylesheets);
            let row_height = planned_row_heights[row_index];
            let row_occupied = planned_row_occupancy
                .get(row_index)
                .cloned()
                .unwrap_or(false);
            let row_top = self.cursor_y;
            repeated_row_tops.push(row_top);
            repeated_row_heights.push(if row_occupied { row_height } else { 0.0 });
            if !row_occupied || table_row_is_collapsed(&row_style) {
                continue;
            }

            let row_baseline_offset = self
                .table_row_baseline_offset(
                    row_index,
                    row,
                    &grid.rows[row_index],
                    &row_style,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics.clone(),
                    collapsed_geometry,
                )
                .map(|baseline| baseline.offset);
            // CSS Tables allow repeated table-header-group and
            // table-footer-group boxes on fragmented tables. This replays the
            // row group's visible row content using measured row heights.
            // Collapsed-border conflict resolution for repeated fragments
            // still needs durable per-fragment table border grids.
            // https://www.w3.org/TR/CSS22/tables.html#table-display
            if let Some(fill) = row_style
                .background
                .background_color
                .visible_color(row_style.color)
            {
                self.push_rect_in_band(
                    PaintBand::InFlowBlock,
                    PageTopRect::new(occupied_x, row_top, occupied_width, row_height)
                        .rendered_rect(Some(fill)),
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
                    table_metrics.clone(),
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
                    table_metrics.clone(),
                )
                .max(metrics.border_box_height);
                let cell_placement = TableGridPlacement::new(TableGridContentBoxTopLeft::new(
                    PageTopPoint::new(table_x, row_top),
                ));
                let cell_border_box = column_plan
                    .cell_border_box(prepared.area, TableRowBounds::new(0.0, cell_height));
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
                    table_metrics: table_metrics.clone(),
                    collapsed_geometry,
                    row_baseline_offset,
                };
                let cell_row_baseline_offset = self.table_cell_row_baseline_offset_for_alignment(
                    &baseline_context,
                    placement,
                    cell_style,
                );
                let cell_axes = TableCellAxisAdapter::for_cell(table_style, cell_style);
                let unaligned_content_box =
                    cell_border_box.content_box(cell_placement, cell_style.padding, cell_borders);
                let unaligned_content_geometry =
                    cell_axes.content_geometry(unaligned_content_box, 0.0);
                let subject_block_size = if cell_axes.cell_inline_uses_physical_width() {
                    metrics.content_height
                } else {
                    self.table_cell_content_alignment_subject_width(
                        cell,
                        cell_style,
                        stylesheets,
                        cell_borders,
                        unaligned_content_geometry.block_size().points(),
                    )
                };
                let content_block_offset = self.table_cell_content_block_offset(
                    cell_style,
                    unaligned_content_geometry,
                    subject_block_size,
                    cell_row_baseline_offset,
                    metrics.baseline_offset,
                );
                let content_geometry =
                    cell_axes.content_geometry(unaligned_content_box, content_block_offset);
                let collapsed_content_clip = self.collapsed_rowspan_cell_content_clip(
                    row_index,
                    placement.rowspan,
                    rows,
                    table_style,
                    stylesheets,
                    planned_row_heights,
                    planned_row_heights,
                    planned_row_occupancy,
                    table_metrics.clone(),
                    cell_border_box,
                    cell_placement,
                );
                let paint_containment_clip = self.table_cell_content_clip(
                    cell.element,
                    cell_style,
                    cell_border_box,
                    cell_placement,
                    cell_borders,
                );
                let content_clip = match (collapsed_content_clip, paint_containment_clip) {
                    (Some(collapsed), Some(containment)) => {
                        collapsed.intersect(&TableCellClipRegion::from_clip(containment))
                    }
                    (Some(clip), None) => Some(clip),
                    (None, Some(clip)) => Some(TableCellClipRegion::from_clip(clip)),
                    (None, None) => None,
                };
                let paint_empty_cell = table_metrics.border_collapse
                    == css::BorderCollapse::Collapse
                    || cell_style.empty_cells == EmptyCells::Show
                    || !cell_is_empty;

                // Cell decoration is painted in physical page coordinates.
                // The table-grid tracks above remain logical, so projecting
                // the typed border box here is required for vertical and
                // sideways table roots as well as horizontal ones.
                // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
                let cell_border_rect = cell_border_box.page_top_rect(cell_placement);

                if paint_empty_cell {
                    let cell_paint_style = collapsed_cell_decoration_style(
                        cell_style,
                        table_metrics.border_collapse == css::BorderCollapse::Collapse,
                    );
                    let (rects, rounded_rects, paths, strokes) = block_paint_ops_with_border_insets(
                        cell_border_rect.paint_rect(),
                        &cell_paint_style,
                        cell_borders,
                        false,
                    );
                    let decoration_band =
                        if table_metrics.border_collapse == css::BorderCollapse::Collapse {
                            PaintBand::TableCollapsedBorder
                        } else {
                            PaintBand::InFlowBlock
                        };
                    for rect in rects {
                        self.push_rect_in_band(decoration_band, rect);
                    }
                    for rounded_rect in rounded_rects {
                        self.push_rounded_rect_in_band(decoration_band, rounded_rect);
                    }
                    for path in paths {
                        self.push_path_in_band(decoration_band, path);
                    }
                    for stroke in strokes {
                        self.push_stroke_in_band(decoration_band, stroke);
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
                        cell_border_rect,
                        cell_style,
                    );
                    for rect in border_rects {
                        self.push_rect_in_band(PaintBand::InFlowBlock, rect);
                    }
                    for path in border_paths {
                        self.push_path_in_band(PaintBand::InFlowBlock, path);
                    }
                }

                let layout_content_clip = content_clip
                    .as_ref()
                    .and_then(TableCellClipRegion::bounding_clip);

                if !text.is_empty() && cell.children.is_none() {
                    let content_box = content_geometry.content_box();
                    let content_scope = self.enter_table_cell_content_scope(
                        cell_style,
                        content_box,
                        layout_content_clip,
                        self.table_cell_child_ancestors(cell, row),
                        PercentageBasis::indefinite(),
                    );
                    self.push_float_context();
                    if let Some(element) = cell.element {
                        let _ = self.layout_inline_items_block(
                            element,
                            cell_style,
                            stylesheets,
                            None,
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
                    content_geometry.content_box(),
                );
                self.layout_table_cell_flow_children(
                    cell,
                    row,
                    cell_style,
                    &prepared.row_sizing_style,
                    table_style,
                    false,
                    stylesheets,
                    cell_borders,
                    content_geometry,
                    layout_content_clip,
                );
                self.layout_table_cell_positioned_children(
                    cell,
                    row,
                    &row_style,
                    Some(ContainingBlock::from_page_top_rect(PageTopRect::new(
                        table_x,
                        row_top,
                        used_table_width,
                        row_height,
                    ))),
                    cell_style,
                    stylesheets,
                    cell_borders,
                    cell_border_box,
                    cell_placement,
                    layout_content_clip,
                );
            }

            if row_occupied {
                self.cursor_y -= row_height;
            }
            if row_occupied
                && repeated_rows[position + 1..]
                    .iter()
                    .any(|row| planned_row_occupancy.get(*row).cloned().unwrap_or(false))
            {
                self.cursor_y -= table_metrics.spacing.vertical.length_points();
            }
        }
        self.pop_page_name_scope_suppression();
        self.out_of_flow_prebreak_suppression_depth -= 1;
        self.element_side_effect_suppression_depth -= 1;
        if let Some(geometry) = collapsed_geometry {
            let repeated_row_offsets = vec![0.0; repeated_rows.len()];
            let repeated_original_heights = repeated_rows
                .iter()
                .map(|row| planned_row_heights[*row])
                .collect::<Vec<_>>();
            let repeated_row_bounds = planned_row_heights
                .iter()
                .enumerate()
                .map(|(row_index, row_height)| {
                    TableRowBounds::new(
                        table_row_block_start(
                            planned_row_heights,
                            planned_row_occupancy,
                            row_index,
                            table_metrics.clone(),
                        ),
                        *row_height,
                    )
                })
                .collect::<Vec<_>>();
            let placement = TableGridPlacement::with_axes(
                TableGridContentBoxTopLeft::new(PageTopPoint::new(table_x, fragment_top)),
                TableAxes::for_style(table_style),
                TableGridLogicalSize::new(
                    column_plan.total_width(),
                    LogicalBlockContentSize::new(content_box_pt(table_grid_height(
                        planned_row_heights,
                        planned_row_occupancy,
                        table_metrics,
                    ))),
                ),
            );
            let (rects, paths) = geometry.grid.paint_fragment_rows(
                placement,
                TableGridPlacement::new(TableGridContentBoxTopLeft::new(PageTopPoint::new(
                    table_x,
                    repeated_row_tops.iter().copied().fold(0.0_f32, f32::max),
                ))),
                column_plan,
                repeated_rows,
                &repeated_row_tops,
                &repeated_row_heights,
                &repeated_row_offsets,
                &repeated_original_heights,
                Some(&repeated_row_bounds),
            );
            for rect in rects {
                self.push_rect_in_band(PaintBand::InFlowBlock, rect);
            }
            for path in paths {
                self.push_path_in_band(PaintBand::InFlowBlock, path);
            }
        }
        if scope_as_table_fragment && self.pages.len() == paint_page_index {
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
            let mut fragment = self
                .current_page
                .take_paint_fragment_since(paint_checkpoint.clone());
            if table_outlines_use_in_flow_phase(table_style, false, &policy) {
                fragment.promote_outline_to_in_flow_outline();
            }
            let child_contexts = self.positioned_child_contexts_since(
                positioned_layer_start,
                paint_page_index,
                &policy,
            );
            self.scope_current_page_fragment_with_policy(
                &paint_checkpoint,
                policy,
                bounds,
                fragment,
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
        stylesheets: &Stylesheets<'_>,
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
        if let Some(fill) = table_style
            .background
            .background_color
            .visible_color(table_style.color)
        {
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
        let occupied_inline_bounds = column_plan.occupied_inline_bounds().unwrap_or_else(|| {
            TableInlineBounds::new(
                TableGridLength::new(0.0),
                TableGridLength::new(used_table_width),
            )
        });
        let occupied_x = table_x + occupied_inline_bounds.logical_start().get();
        let occupied_width = occupied_inline_bounds.logical_size().get();
        for (position, row_index) in repeated_rows.iter().cloned().enumerate() {
            local_row_tops.push(cursor_y);
            let row_height = planned_row_heights[row_index];
            let row_occupied = planned_row_occupancy
                .get(row_index)
                .cloned()
                .unwrap_or(false);
            local_row_heights.push(if row_occupied { row_height } else { 0.0 });
            if row_occupied {
                cursor_y -= row_height;
            }
            if row_occupied
                && repeated_rows[position + 1..]
                    .iter()
                    .any(|row| planned_row_occupancy.get(*row).cloned().unwrap_or(false))
            {
                cursor_y -= table_metrics.spacing.vertical.length_points();
            }
        }
        let mut column_group_spans = table_column_group_spans(columns, column_plan.column_count());
        if table_columns_paint_in_reverse_page_order(table_style) {
            column_group_spans.reverse();
        }
        // A colgroup without explicit columns is represented by a synthetic
        // column solely for sizing. Its visible background has no column
        // layer above it, so retain it for the common physical column pass
        // below. This keeps disjoint group and column fills in the same
        // committed page order without disturbing CSS's overlapping layers.
        let mut synthetic_group_backgrounds = Vec::new();
        for (start_column, end_column, column_group) in column_group_spans {
            let column_group_style =
                self.style_for_table_column_group(&column_group, table_style, stylesheets);
            let mut primitives = table_column_fragment_background_primitives(
                table_x,
                fragment_top,
                fragment_height,
                column_plan,
                None,
                repeated_rows,
                start_column,
                end_column,
                &column_group_style,
                &local_row_tops,
                &local_row_heights,
            );
            primitives.extend(table_column_fragment_background_image_primitives(
                table_x,
                fragment_top,
                fragment_height,
                column_plan,
                None,
                repeated_rows,
                start_column,
                end_column,
                &column_group_style,
                &local_row_tops,
                &local_row_heights,
                self.base_url,
                self.root_url,
                self.resource_cache,
            ));
            let synthetic_group = !table_column_group_has_explicit_columns(
                columns,
                start_column,
                end_column,
                column_plan.column_count(),
            );
            if synthetic_group {
                synthetic_group_backgrounds.push((start_column, primitives));
            } else {
                for primitive in primitives {
                    self.push_primitive_in_band(PaintBand::InFlowBlock, primitive);
                }
            }
        }
        let mut column_index = 0;
        let mut column_spans = Vec::new();
        for column in columns {
            if column_index >= column_plan.column_count() {
                break;
            }
            let span = column
                .span
                .min(column_plan.column_count() - column_index)
                .max(1);
            // Synthetic columns materialized for a `colgroup` without `col`
            // children carry the group style only for grid sizing. Their
            // structural background has already been emitted for the group.
            // <https://www.w3.org/TR/css-tables-3/#drawing-backgrounds>
            let is_group_placeholder = column
                .group
                .as_ref()
                .is_some_and(|group| group.signature == column.signature);
            if is_group_placeholder {
                column_index += span;
                continue;
            }
            column_spans.push((column_index, span, column));
            column_index += span;
        }
        let mut physical_column_backgrounds = Vec::new();
        for (column_index, span, column) in column_spans {
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            let mut primitives = table_column_fragment_background_primitives(
                table_x,
                fragment_top,
                fragment_height,
                column_plan,
                None,
                repeated_rows,
                column_index,
                column_index + span,
                &column_style,
                &local_row_tops,
                &local_row_heights,
            );
            primitives.extend(table_column_fragment_background_image_primitives(
                table_x,
                fragment_top,
                fragment_height,
                column_plan,
                None,
                repeated_rows,
                column_index,
                column_index + span,
                &column_style,
                &local_row_tops,
                &local_row_heights,
                self.base_url,
                self.root_url,
                self.resource_cache,
            ));
            physical_column_backgrounds.push((column_index, primitives));
        }
        physical_column_backgrounds.extend(synthetic_group_backgrounds);
        physical_column_backgrounds.sort_by_key(|(start_column, _)| *start_column);
        if table_columns_paint_in_reverse_page_order(table_style) {
            physical_column_backgrounds.reverse();
        }
        for (_, primitives) in physical_column_backgrounds {
            for primitive in primitives {
                self.push_primitive_in_band(PaintBand::InFlowBlock, primitive);
            }
        }

        for (start_row, end_row, row_group) in table_row_group_spans(rows) {
            let row_group_style =
                self.style_for_table_row_group(&row_group, table_style, stylesheets);
            if let Some(fill) = row_group_style
                .background
                .background_color
                .visible_color(row_group_style.color)
            {
                let mut segment_start = None;
                let mut previous_local = None;
                for (local_row, original_row) in repeated_rows.iter().cloned().enumerate() {
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

/// Return whether a column-group range contains a real `col` descendant.
///
/// Synthetic columns represent a `colgroup` without `col` children for grid
/// sizing only. Their background cannot overlap a later column background, so
/// it may safely join the canonical page-ordered pass for disjoint fills.
fn table_column_group_has_explicit_columns(
    columns: &[TableColumn<'_>],
    start_column: usize,
    end_column: usize,
    column_count: usize,
) -> bool {
    let mut column_index = 0;
    for column in columns {
        if column_index >= column_count {
            break;
        }
        let span = column.span.min(column_count - column_index).max(1);
        let column_end = column_index + span;
        let overlaps_group = column_index < end_column && column_end > start_column;
        let is_group_placeholder = column
            .group
            .as_ref()
            .is_some_and(|group| group.signature == column.signature);
        if overlaps_group && !is_group_placeholder {
            return true;
        }
        column_index = column_end;
    }
    false
}
