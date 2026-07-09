use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Pre-render a nested table/flex formatting context for split table-cell
    /// replay.
    ///
    /// CSS Fragmentation clips the table row piece, but the nested formatting
    /// context itself must keep its internal paint order and effects. Planning
    /// the child into an off-page fragment lets paint replay only translate and
    /// clip the selected page-local slice:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout::table) fn plan_table_cell_nested_child_fragment(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        child_box: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> Option<TableCellNestedFragmentPlan> {
        if !matches!(
            child_box,
            box_tree::FormattingBox::Table(_) | box_tree::FormattingBox::Flex(_)
        ) {
            return None;
        }

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        self.ancestors = self.table_cell_child_ancestors(cell, row);
        let width = available_width.max(1.0);
        let top = 10_000.0;
        self.current_page = Page::new(width, top);
        self.overflow_clips.clear();
        self.truncate_page_start_margins = false;
        let content_scope = self.enter_table_cell_content_scope_for_rect(
            cell_style,
            PageTopRect::new(0.0, top, width, top),
            self.table_cell_child_ancestors(cell, row),
            None,
        );

        self.layout_formatting_box(child_box, stylesheets);
        self.flush_positioned_layers_since(positioned_layer_start);

        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintTranslation::new(0.0, -top));
        let assignments = self.captured_current_page_assignment_values();
        let height = (top - self.cursor_y).max(0.0);
        self.restore_table_cell_content_scope(content_scope);
        self.restore(snapshot);

        (!fragment.is_empty()).then_some(TableCellNestedFragmentPlan {
            fragment,
            width,
            height,
            metadata: FragmentPageMetadata::empty(self.pages.len()),
            assignments,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn paint_table_cell_planned_child_fragments(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        cell_borders: css::Edges,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        content_offset: f32,
        content_x_offset: f32,
        child_fragments: &[TableCellChildFragmentPlan],
    ) {
        let Some(children) = cell.children.as_deref() else {
            return;
        };
        if child_fragments.is_empty() {
            return;
        }

        let content_box = border_box.content_box(
            placement,
            cell_style.padding,
            cell_borders,
            content_offset,
            content_x_offset,
        );
        let content_scope = self.enter_table_cell_content_scope(
            cell_style,
            content_box,
            self.table_cell_child_ancestors(cell, row),
            PercentageBasis::indefinite(),
        );

        for child_plan in child_fragments {
            if let Some(child_box) = children.get(child_plan.source_child_index) {
                self.paint_table_cell_planned_child_slice(child_box, stylesheets, child_plan);
            }
        }

        self.restore_table_cell_content_scope(content_scope);
    }

    pub(in crate::layout::table) fn paint_table_cell_planned_child_slice(
        &mut self,
        child_box: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        child_plan: &TableCellChildFragmentPlan,
    ) {
        let child_top = child_plan.child_top;
        let child_height = child_plan.child_height;
        let slice_top = child_plan.slice_top;
        let slice_bottom = child_plan.slice_bottom;
        if child_plan.kind != TableCellChildFragmentKind::NestedFormattingContext
            && self.capture_table_cell_child_fragment_assignments(child_box, child_plan)
        {
            return;
        }
        if let Some(inline_sequence) = &child_plan.inline_sequence {
            self.paint_table_cell_nested_inline_sequence_slice(
                inline_sequence,
                child_top,
                slice_top,
                slice_bottom,
            );
            return;
        }
        match child_plan.kind {
            TableCellChildFragmentKind::Block => {
                let box_tree::FormattingBox::Block(box_) = child_box else {
                    return;
                };
                self.paint_table_cell_element_child_slice(
                    box_.element,
                    &box_.style,
                    &box_.children,
                    stylesheets,
                    child_top,
                    child_height,
                    slice_top,
                    slice_bottom,
                );
            }
            TableCellChildFragmentKind::AnonymousBlock => {
                let box_tree::FormattingBox::AnonymousBlock(box_) = child_box else {
                    return;
                };
                self.paint_table_cell_anonymous_child_slice(
                    &box_.style,
                    &box_.children,
                    child_top,
                    slice_top,
                    slice_bottom,
                    stylesheets,
                );
            }
            TableCellChildFragmentKind::Inline => {
                let box_tree::FormattingBox::Inline(box_) = child_box else {
                    return;
                };
                self.paint_table_cell_anonymous_child_slice(
                    &box_.style,
                    &box_.children,
                    child_top,
                    slice_top,
                    slice_bottom,
                    stylesheets,
                );
            }
            TableCellChildFragmentKind::Text => {
                let box_tree::FormattingBox::Text(box_) = child_box else {
                    return;
                };
                let text = normalized_text_for_style(&box_.text, &box_.style);
                if !text.is_empty() {
                    self.paint_text_block_slice(
                        &text,
                        &box_.style,
                        0.0,
                        0.0,
                        None,
                        child_top,
                        slice_top,
                        slice_bottom,
                    );
                }
            }
            TableCellChildFragmentKind::AtomicInline => {
                let box_tree::FormattingBox::AtomicInline(box_) = child_box else {
                    return;
                };
                if replaced_element_kind(box_.element) == Some(ReplacedElementKind::Svg) {
                    self.paint_table_cell_replaced_child_slice(
                        box_.element,
                        &box_.style,
                        child_top,
                        child_height,
                    );
                } else {
                    self.paint_table_cell_element_child_slice(
                        box_.element,
                        &box_.style,
                        &box_.children,
                        stylesheets,
                        child_top,
                        child_height,
                        slice_top,
                        slice_bottom,
                    );
                }
            }
            TableCellChildFragmentKind::Replaced => {
                let box_tree::FormattingBox::Replaced(box_) = child_box else {
                    return;
                };
                self.paint_table_cell_replaced_child_slice(
                    box_.element,
                    &box_.style,
                    child_top,
                    child_height,
                );
            }
            TableCellChildFragmentKind::NestedFormattingContext => {
                self.paint_table_cell_nested_child_fragment(child_plan);
            }
        }
    }

    fn capture_table_cell_child_fragment_assignments(
        &mut self,
        child_box: &box_tree::FormattingBox<'_>,
        child_plan: &TableCellChildFragmentPlan,
    ) -> bool {
        if child_plan.metadata.continues_from_previous_page {
            return false;
        }
        let Some((element, _, style, _)) = child_box.element_parts() else {
            return false;
        };
        self.capture_assignments_for_fragment_source(
            element,
            style,
            child_plan.metadata.assignment_placement(),
        )
    }

    pub(in crate::layout::table) fn paint_table_cell_nested_inline_sequence_slice(
        &mut self,
        inline_sequence: &TableCellNestedInlineSequencePlan,
        child_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        self.paint_inline_line_sequence_slice(
            &inline_sequence.sequence,
            &inline_sequence.style,
            child_top,
            slice_top,
            slice_bottom,
        );
    }

    pub(in crate::layout::table) fn paint_table_cell_nested_child_fragment(
        &mut self,
        child_plan: &TableCellChildFragmentPlan,
    ) {
        let Some(nested) = &child_plan.nested_fragment else {
            return;
        };
        if !child_plan.metadata.continues_from_previous_page {
            self.replay_captured_page_assignments(
                &nested.assignments,
                child_plan.metadata.assignment_placement(),
            );
        }
        let slice_height = (child_plan.slice_top - child_plan.slice_bottom).max(0.0);
        if slice_height <= 0.0 {
            return;
        }

        let x = self.content_left;
        let translated = nested
            .fragment
            .clone()
            .translated(PaintTranslation::new(x, child_plan.child_top));
        let bounds =
            PageTopRect::new(x, child_plan.child_top, nested.width, nested.height).paint_clip();
        let slice_clip = PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(x, child_plan.slice_bottom),
            PaintSize::new(nested.width, slice_height),
        ));
        let context = PaintStackingContext::from_banded_fragment(translated, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(PaintEffects {
                overflow_clip: Some(slice_clip),
                absolute_clip: Some(slice_clip),
                ..PaintEffects::default()
            })
            .with_bounds(bounds);
        let fragment =
            PaintFragment::from_stacking_context_in_band(PaintBand::InFlowBlock, context);
        self.current_page
            .append_paint_fragment_owned(fragment, PaintTranslation::identity());
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn paint_table_cell_element_child_slice(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        child_top: f32,
        child_height: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return;
        }
        let mut used_style = self.style_with_current_used_lengths(style);
        let containing_width = (self.content_right - self.content_left).max(0.0);
        apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(containing_width)),
        );
        let style = &used_style;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let outer_width = (containing_width - style.margin.left - style.margin.right).max(0.0);
        let border_box_top = child_top - style.margin.top;
        let border_box_height = (child_height - style.margin.top - style.margin.bottom).max(0.0);
        let horizontal_non_content = non_content_pt(
            style.padding.left + style.padding.right + horizontal_border_width(style),
        );
        let content_width =
            used_content_box_width(style, layout_pt(outer_width), horizontal_non_content).points();
        let bounds = PageTopRect::new(
            self.content_left + style.margin.left,
            border_box_top,
            content_width + horizontal_non_content.points(),
            border_box_height,
        )
        .paint_clip();
        if border_box_height > 0.0 && style.visibility == Visibility::Visible {
            for primitive in self.box_background_primitives(
                paint_space_rect(bounds.x(), bounds.y(), bounds.width(), bounds.height()),
                style,
            ) {
                self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
            }
        }
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let borders = used_border_widths(style);
        self.content_left += style.margin.left + borders.left + style.padding.left;
        self.content_right = self.content_left + content_width;
        let content_width = (self.content_right - self.content_left).max(1.0);
        if let Some(sequence) = self.table_cell_nested_inline_sequence_for_children(
            style,
            children,
            stylesheets,
            element.attrs.get("href").cloned(),
            content_width,
            PercentageBasis::indefinite(),
        ) {
            let text_top = border_box_top - borders.top - style.padding.top;
            self.paint_table_cell_nested_inline_sequence_slice(
                &sequence,
                text_top,
                slice_top,
                slice_bottom,
            );
        }
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            bounds,
            style,
            Vec::new(),
        );
    }

    pub(in crate::layout::table) fn paint_table_cell_replaced_child_slice(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        child_top: f32,
        child_height: f32,
    ) {
        if matches!(style.position, Position::Absolute | Position::Fixed)
            || style.visibility != Visibility::Visible
        {
            return;
        }
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let outer_width = (self.content_right - self.content_left).max(0.0);
        let border_box_top = child_top - style.margin.top;
        let border_box_height = (child_height - style.margin.top - style.margin.bottom).max(0.0);
        let bounds = PageTopRect::new(
            self.content_left + style.margin.left,
            border_box_top,
            outer_width - style.margin.left - style.margin.right,
            border_box_height,
        )
        .paint_clip();
        for primitive in self.box_background_primitives(
            paint_space_rect(bounds.x(), bounds.y(), bounds.width(), bounds.height()),
            style,
        ) {
            self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
        }

        if let Some(asset) = self.resource_cache.inline_svg_asset(element) {
            let size = asset.intrinsic_size();
            let borders = used_border_widths(style);
            let x = self.content_left + style.margin.left + borders.left + style.padding.left;
            let y_top = border_box_top - borders.top - style.padding.top;
            let rect = PageTopRect::new(x, y_top, size.width, size.height).paint_rect();
            for path in asset.paint_paths(rect) {
                self.push_path_in_band(PaintBand::Inline, path);
            }
        }
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            bounds,
            style,
            Vec::new(),
        );
    }

    pub(in crate::layout::table) fn paint_table_cell_anonymous_child_slice(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        child_top: f32,
        slice_top: f32,
        slice_bottom: f32,
        stylesheets: &[Stylesheet],
    ) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        if let Some(sequence) = self.table_cell_nested_inline_sequence_for_children(
            style,
            children,
            stylesheets,
            None,
            available_width,
            PercentageBasis::indefinite(),
        ) {
            self.paint_table_cell_nested_inline_sequence_slice(
                &sequence,
                child_top,
                slice_top,
                slice_bottom,
            );
        }
        for child_plan in self.table_cell_child_fragment_plans(
            children,
            stylesheets,
            available_width,
            PercentageBasis::indefinite(),
            child_top,
            slice_top,
            slice_bottom,
        ) {
            let child = &children[child_plan.source_child_index];
            if matches!(
                child,
                box_tree::FormattingBox::Replaced(_)
                    | box_tree::FormattingBox::Table(_)
                    | box_tree::FormattingBox::Flex(_)
            ) {
                self.paint_table_cell_planned_child_slice(child, stylesheets, &child_plan);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_body_fragment_structural_primitives(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        columns: &[TableColumn<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        column_plan: &TableColumnPlan,
        fragment: &TableBodyPaintFragment,
    ) -> (Vec<PaintPrimitive>, Vec<PaintPrimitive>) {
        let top = fragment.plan.fragment_top;
        let bottom = fragment.bottom();
        let height = (top - bottom).max(0.0);
        let fragment_has_occupied_row = fragment.plan.body_rows.iter().any(|row| !row.collapsed);
        let vertical_edge_spacing =
            table_vertical_edge_spacing(&[fragment_has_occupied_row], table_metrics);
        let mut backgrounds = Vec::new();
        let mut outlines = Vec::new();
        if height <= 0.0 && vertical_edge_spacing <= 0.0 {
            return (backgrounds, outlines);
        }

        if let Some(fill) = table_style.background_color {
            let background_top = top
                + vertical_edge_spacing
                + table_width.padding.top
                + table_width.border_widths.top;
            let background_bottom = bottom
                - vertical_edge_spacing
                - table_width.padding.bottom
                - table_width.border_widths.bottom;
            let border_rect = paint_space_rect(
                table_x - table_width.padding.left - table_width.border_widths.left,
                background_bottom,
                used_table_width
                    + table_width.padding.left
                    + table_width.padding.right
                    + table_width.border_widths.left
                    + table_width.border_widths.right,
                background_top - background_bottom,
            );
            let clip_rect = crate::layout::paint_helpers::background_rect_area_for_box(
                border_rect,
                table_style,
                table_width.border_widths,
                table_style.background_clip,
            );
            backgrounds.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                clip_rect,
                Some(fill),
            )));
        }
        let fragment_rows = fragment.rows();
        let fragment_row_tops = fragment.row_tops();
        let fragment_row_heights = fragment.row_heights();
        let grid_placement = fragment.grid_placement;
        for (start_column, end_column, column_group) in
            table_column_group_spans(columns, column_plan.column_count())
        {
            let column_group_style =
                self.style_for_table_column_group(&column_group, table_style, stylesheets);
            if let Some(placement) = grid_placement
                && column_group_style.writing_mode.has_vertical_lines()
            {
                backgrounds.extend(table_column_grid_background_primitives(
                    placement,
                    column_plan,
                    start_column,
                    end_column,
                    &column_group_style,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                ));
            } else {
                backgrounds.extend(table_column_fragment_background_primitives(
                    table_x,
                    top,
                    height,
                    column_plan,
                    Some(grid),
                    &fragment_rows,
                    start_column,
                    end_column,
                    &column_group_style,
                    &fragment_row_tops,
                    &fragment_row_heights,
                ));
                backgrounds.extend(table_column_fragment_background_image_primitives(
                    table_x,
                    top,
                    height,
                    column_plan,
                    Some(grid),
                    &fragment_rows,
                    start_column,
                    end_column,
                    &column_group_style,
                    &fragment_row_tops,
                    &fragment_row_heights,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                ));
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
            // A `colgroup` without explicit `col` children is represented by
            // one synthetic column carrying the group's style. The group was
            // already painted above, so replaying that synthetic column would
            // alpha-composite the same structural background twice. Explicit
            // columns remain independent background layers.
            // <https://www.w3.org/TR/css-tables-3/#drawing-backgrounds>
            let is_group_placeholder = column
                .group
                .as_ref()
                .is_some_and(|group| group.signature == column.signature);
            if is_group_placeholder {
                column_index += span;
                continue;
            }
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            if let Some(placement) = grid_placement
                && column_style.writing_mode.has_vertical_lines()
            {
                backgrounds.extend(table_column_grid_background_primitives(
                    placement,
                    column_plan,
                    column_index,
                    column_index + span,
                    &column_style,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                ));
            } else {
                backgrounds.extend(table_column_fragment_background_primitives(
                    table_x,
                    top,
                    height,
                    column_plan,
                    Some(grid),
                    &fragment_rows,
                    column_index,
                    column_index + span,
                    &column_style,
                    &fragment_row_tops,
                    &fragment_row_heights,
                ));
                backgrounds.extend(table_column_fragment_background_image_primitives(
                    table_x,
                    top,
                    height,
                    column_plan,
                    Some(grid),
                    &fragment_rows,
                    column_index,
                    column_index + span,
                    &column_style,
                    &fragment_row_tops,
                    &fragment_row_heights,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                ));
            }
            column_index += span;
        }

        let occupied_inline_bounds = column_plan
            .occupied_inline_bounds()
            .unwrap_or_else(|| TableInlineBounds::new(0.0, used_table_width));
        let occupied_x = table_x + occupied_inline_bounds.start;
        let occupied_width = occupied_inline_bounds.size;

        for (start_row, end_row, row_group) in table_row_group_spans(rows) {
            let row_group_style =
                self.style_for_table_row_group(&row_group, table_style, stylesheets);
            if let Some(fill) = row_group_style.background_color {
                let mut segment_start = None;
                let mut previous_local = None;
                for (local_row, original_row) in fragment_rows.iter().cloned().enumerate() {
                    if original_row >= start_row && original_row < end_row {
                        if segment_start.is_none() {
                            segment_start = Some(local_row);
                        }
                        previous_local = Some(local_row + 1);
                    } else if let (Some(start), Some(end)) =
                        (segment_start.take(), previous_local.take())
                    {
                        push_table_fragment_row_span_background(
                            &mut backgrounds,
                            occupied_x,
                            occupied_width,
                            &fragment_row_tops,
                            &fragment_row_heights,
                            start,
                            end,
                            fill,
                        );
                    }
                }
                if let (Some(start), Some(end)) = (segment_start, previous_local) {
                    push_table_fragment_row_span_background(
                        &mut backgrounds,
                        occupied_x,
                        occupied_width,
                        &fragment_row_tops,
                        &fragment_row_heights,
                        start,
                        end,
                        fill,
                    );
                }
            }
            if row_group_style.visibility == Visibility::Visible {
                self.push_table_fragment_row_group_outline_segments(
                    &mut outlines,
                    occupied_x,
                    occupied_width,
                    &fragment_row_tops,
                    &fragment_row_heights,
                    &fragment_rows,
                    start_row,
                    end_row,
                    &row_group_style,
                );
            }
        }

        for (local_row, original_row) in fragment_rows.iter().cloned().enumerate() {
            let row_style = self.style_for_table_row(&rows[original_row], table_style, stylesheets);
            if let Some(bounds) = table_fragment_row_span_bounds(
                occupied_x,
                occupied_width,
                &fragment_row_tops,
                &fragment_row_heights,
                local_row,
                local_row + 1,
            ) {
                backgrounds.extend(table_row_fragment_background_primitives(
                    table_x,
                    bounds.paint_rect(),
                    column_plan,
                    grid,
                    &fragment_rows,
                    &fragment_row_tops,
                    &fragment_row_heights,
                    original_row,
                    &row_style,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                ));
            }
        }
        (backgrounds, outlines)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn push_table_fragment_row_group_outline_segments(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        table_x: f32,
        used_table_width: f32,
        row_tops: &[f32],
        row_heights: &[f32],
        rows: &[usize],
        start_row: usize,
        end_row: usize,
        row_group_style: &ComputedStyle,
    ) {
        let mut segment_start = None;
        let mut previous_local = None;
        for (local_row, original_row) in rows.iter().cloned().enumerate() {
            if original_row >= start_row && original_row < end_row {
                if segment_start.is_none() {
                    segment_start = Some(local_row);
                }
                previous_local = Some(local_row + 1);
            } else if let (Some(start), Some(end)) = (segment_start.take(), previous_local.take()) {
                self.push_table_fragment_row_span_outline(
                    primitives,
                    table_x,
                    used_table_width,
                    row_tops,
                    row_heights,
                    start,
                    end,
                    row_group_style,
                );
            }
        }
        if let (Some(start), Some(end)) = (segment_start, previous_local) {
            self.push_table_fragment_row_span_outline(
                primitives,
                table_x,
                used_table_width,
                row_tops,
                row_heights,
                start,
                end,
                row_group_style,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn push_table_fragment_row_span_outline(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        table_x: f32,
        used_table_width: f32,
        row_tops: &[f32],
        row_heights: &[f32],
        start: usize,
        end: usize,
        row_group_style: &ComputedStyle,
    ) {
        let Some(bounds) = table_fragment_row_span_bounds(
            table_x,
            used_table_width,
            row_tops,
            row_heights,
            start,
            end,
        ) else {
            return;
        };
        primitives.extend(self.box_outline_primitives(
            paint_space_rect(bounds.x(), bounds.y(), bounds.width(), bounds.height()),
            row_group_style,
        ));
    }

    /// Build collapsed borders for one generated table row fragment.
    ///
    /// CSS 2.2 centers collapsed borders on grid lines, while CSS
    /// Fragmentation requires each page fragment to paint its own visible table
    /// piece. Resolving a page-local grid keeps body-row collapsed borders on
    /// the same durable paint-tree page fragment as the rows they border:
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn collapsed_table_fragment_border_primitives(
        &mut self,
        geometry: &CollapsedTableGeometry,
        table_x: f32,
        column_plan: &TableColumnPlan,
        fragment: &TableBodyPaintFragment,
    ) -> Vec<PaintPrimitive> {
        let rows = fragment.rows();
        let row_tops = fragment.row_tops();
        let row_heights = fragment.row_heights();
        let row_offsets = fragment.row_offsets();
        let original_row_heights = fragment.original_row_heights();
        let placement = TableGridPlacement::new(PageTopPoint::new(table_x, 0.0));
        let (rects, paths) = geometry.grid.paint_fragment_rows(
            placement,
            column_plan,
            &rows,
            &row_tops,
            &row_heights,
            &row_offsets,
            &original_row_heights,
        );
        rects
            .into_iter()
            .map(PaintPrimitive::Rect)
            .chain(paths.into_iter().map(PaintPrimitive::Path))
            .collect()
    }

    pub(in crate::layout::table) fn measure_table_row_height(
        &mut self,
        context: &TableGridLayoutContext<'_, '_>,
        row_index: usize,
        row_style: &ComputedStyle,
    ) -> f32 {
        let row = &context.rows[row_index];
        let placements = &context.grid.rows[row_index];
        let mut row_height: f32 = used_length_percentage_or_auto_with_basis(
            table_root_block_size(row_style),
            percentage_basis_from_points(None),
        )
        .map(|height| height.points())
        .unwrap_or(0.0);
        let mut max_baseline: f32 = 0.0;
        let mut max_after_baseline: f32 = 0.0;
        let mut has_baseline_cells = false;
        for placement in placements {
            let cell = &row.cells[placement.cell];
            let Some(prepared) = self.prepare_table_cell(
                cell,
                row,
                row_style,
                placement,
                row_index,
                0.0,
                context.stylesheets,
                context.table_cellpadding,
                context.column_plan,
                context.table_metrics.clone(),
                context.collapsed_geometry,
            ) else {
                continue;
            };
            if placement.rowspan == 1 {
                // Rows are table-root block tracks.  In a vertical table the
                // logical block axis is physical width, so a cell's ordinary
                // physical-height metric cannot size that track.
                // <https://drafts.csswg.org/css-tables-3/#row-layout>
                // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
                let cell_block_contribution =
                    if context.table_style.writing_mode.has_vertical_lines() {
                        table_cell_content_max_width(
                            self,
                            cell,
                            &prepared.style,
                            context.stylesheets,
                            Some(prepared.borders),
                        )
                    } else {
                        prepared.metrics.border_box_height
                    };
                row_height = row_height.max(cell_block_contribution);
            }
            if table_cell_participates_in_physical_y_row_baseline(
                &prepared.style,
                row_style,
                placement,
            ) && let Some(baseline) = self.table_cell_physical_y_row_baseline_candidate(
                cell,
                &prepared,
                context.stylesheets,
            ) {
                has_baseline_cells = true;
                max_baseline = max_baseline.max(baseline);
                max_after_baseline = max_after_baseline
                    .max((prepared.metrics.border_box_height - baseline).max(0.0));
            }
        }
        if has_baseline_cells {
            row_height = row_height.max(max_baseline + max_after_baseline);
        }
        row_height
    }

    /// Build the CSS Tables 3 height distribution plan for a table grid.
    ///
    /// The planner keeps row baselines tied to the first pass, grows row spans
    /// before final distribution, resolves explicit and percentage row,
    /// row-group, and cell constraints into reference sizes, then assigns final
    /// row heights before pagination and painting consume the row list.
    ///
    /// Spec: <https://drafts.csswg.org/css-tables-3/#row-layout>,
    /// <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>,
    /// and <https://drafts.csswg.org/css-tables-3/#table-cell-content-layout-second-pass>.
    pub(in crate::layout::table) fn table_height_plan(
        &mut self,
        context: &TableGridLayoutContext<'_, '_>,
    ) -> TableHeightPlan {
        // CSS Tables 3 row layout first computes minimum row sizes, applies
        // spanning-cell minimum constraints, then distributes any definite
        // table height against reference sizes:
        // <https://drafts.csswg.org/css-tables-3/#row-layout> and
        // <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>.
        let mut plan_rows = Vec::with_capacity(context.rows.len());
        let mut spanning_cells = Vec::new();
        for (row_index, row) in context.rows.iter().enumerate() {
            let row_style = self.style_for_table_row(row, context.table_style, context.stylesheets);
            if table_row_is_collapsed(&row_style) || row_style.running_element_name.is_some() {
                plan_rows.push(TableRowHeightPlan {
                    base: 0.0,
                    reference: 0.0,
                    final_height: 0.0,
                    auto: false,
                    collapsed: true,
                });
                continue;
            }
            if self.table_row_is_hidden_empty(
                row,
                &context.grid.rows[row_index],
                &row_style,
                context.stylesheets,
                context.table_cellpadding,
                context.column_plan,
                context.table_metrics.clone(),
            ) {
                plan_rows.push(TableRowHeightPlan {
                    base: 0.0,
                    reference: 0.0,
                    final_height: 0.0,
                    auto: false,
                    collapsed: true,
                });
                continue;
            }
            let base = self.measure_table_row_height(context, row_index, &row_style);
            plan_rows.push(TableRowHeightPlan {
                base,
                reference: base,
                final_height: base,
                auto: table_root_block_size(&row_style).is_auto(),
                collapsed: false,
            });
            for placement in &context.grid.rows[row_index] {
                if placement.rowspan > 1 {
                    let cell = &row.cells[placement.cell];
                    let Some(prepared) = self.prepare_table_cell(
                        cell,
                        row,
                        &row_style,
                        placement,
                        row_index,
                        0.0,
                        context.stylesheets,
                        context.table_cellpadding,
                        context.column_plan,
                        context.table_metrics.clone(),
                        context.collapsed_geometry,
                    ) else {
                        continue;
                    };
                    let required_block_size =
                        if context.table_style.writing_mode.has_vertical_lines() {
                            table_cell_content_max_width(
                                self,
                                cell,
                                &prepared.style,
                                context.stylesheets,
                                Some(prepared.borders),
                            )
                        } else {
                            prepared.metrics.border_box_height
                        };
                    spanning_cells.push((row_index, placement.rowspan, required_block_size));
                }
            }
        }

        for (row_index, rowspan, required_height) in spanning_cells {
            distribute_table_span_constraint(
                &mut plan_rows,
                row_index,
                rowspan,
                required_height,
                context.table_metrics.clone(),
                TableHeightTarget::Base,
            );
        }
        for row in &mut plan_rows {
            row.reference = row.base;
            row.final_height = row.base;
        }

        let target_content_height = self.resolve_table_target_content_height(
            context.table_style,
            context.collapsed_geometry,
            context.wrapper_border_box_block_size,
            context.wrapper_non_grid_block_size,
        );
        self.compute_table_reference_heights(
            &mut plan_rows,
            context,
            target_content_height
                .map(|value| {
                    PercentageBasis::definite_from(value, BlockSizeBasisSource::TableWrapper)
                })
                .unwrap_or_else(PercentageBasis::indefinite),
        );
        self.distribute_table_height_plan(
            &mut plan_rows,
            target_content_height,
            context.table_metrics.clone(),
        );
        TableHeightPlan {
            rows: plan_rows,
            target_content_height,
        }
    }
}
