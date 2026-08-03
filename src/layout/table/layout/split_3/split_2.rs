use super::*;
use std::collections::HashSet;

impl<'a> LayoutBuilder<'a> {
    pub(crate) fn layout_table(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        fragment: &box_tree::TableFragment<'_>,
    ) {
        // Keep the source style inside `TableUsedStyle` for reconstructed
        // table parts, but use the normalized side for every table layout
        // metric and paint operation in this pass.
        let table_used_style = self.table_used_style(style);
        let style = &table_used_style;
        let mut table_subtree_ids = HashSet::new();
        collect_table_subtree_element_ids(element, &mut table_subtree_ids);
        // Discard fixed layers emitted while this same table's intrinsic or
        // track-sizing pass did not yet know its transformed table-part
        // containing block. Preserve layers owned by earlier siblings.
        self.fixed_layers
            .retain(|layer| !table_subtree_ids.contains(&layer.source_element));
        // Track sizing may probe cell contents repeatedly. Positioned/fixed
        // descendants discovered by those speculative passes must not escape
        // into the document's fixed paint queue; only the committed row-paint
        // pass below is allowed to materialize them.
        let fixed_layer_start = self.fixed_layers.len();
        let fragmentainer_kind = self.active_fragmentainer_kind();
        self.apply_forced_break_before_box_in(fragmentainer_kind, style);

        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let relative_offset = self.normal_flow_relative_position_offset(style);
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y += relative_offset.y();
        }
        let captions = input.captions.as_slice();
        let wrapper_border_box_block_size = self.take_table_wrapper_block_size_override();
        let columns = input.columns.as_slice();

        let containing_inline_span =
            PageInlineSpan::from_edges(self.content_left, self.content_right);
        // An auto-width table is a block formatting context root. Its
        // shrink-to-fit input is therefore the complete active float band for
        // the wrapper's prospective block range, rather than the containing
        // block's unconstrained width or the band produced by one float.
        // Resolving tracks at the full width and only then avoiding the
        // combined band makes a table that could fit beside two staggered
        // floats move below both of them.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        // <https://www.w3.org/TR/CSS22/tables.html#auto-table-layout>
        let active_float_band_width = (style.box_values.width.is_auto()
            && style.writing_mode == WritingMode::HorizontalTb)
            .then(|| {
                self.float_contexts.last().map(|context| {
                    context
                        .placement_band(
                            self.current_float_page_index(),
                            PageBlockSpan::from_edges(self.cursor_y, self.page_bottom()),
                            containing_inline_span,
                        )
                        .width()
                })
            })
            .flatten();
        // Tables are sized in their logical inline axis.  In vertical writing
        // modes the physical page-area width is the block span, so using it
        // here constrains a table to one column's physical width instead of
        // the containing block's inline size.
        let available_table_inline_size = active_float_band_width.unwrap_or_else(|| {
            if style.writing_mode == WritingMode::HorizontalTb {
                containing_inline_span.width()
            } else {
                self.current_content_logical_inline_size()
            }
        });
        let inline_margins = if style.writing_mode == WritingMode::HorizontalTb {
            style.margin.left + style.margin.right
        } else {
            style.margin.top + style.margin.bottom
        };
        let available_table_width = available_table_inline_size - inline_margins;
        let mut table_width = used_table_width(style, available_table_width, None);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        if rows.is_empty() {
            self.layout_empty_table(
                element,
                captions,
                style,
                stylesheets,
                available_table_width,
                table_width,
                table_metrics,
                relative_offset,
                self.document_canvas_overflow
                    .is_document_canvas_flow_element(element),
                wrapper_border_box_block_size,
            );
            return;
        }
        let grid = table_grid(rows);
        // Intrinsic width, track, and row-height probes may lay out nested
        // formatting contexts to obtain their final used sizes. Those probes
        // are not a table fragment boundary and must not leave page paint,
        // assignments, or float state behind for the committed wrapper pass.
        // <https://www.w3.org/TR/css-tables-3/#table-layout>
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        let table_measurement_snapshot = self.snapshot();
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
        table_width = used_table_width(
            style,
            available_table_width,
            collapsed_geometry
                .as_ref()
                .map(|geometry| geometry.outer_insets),
        );
        self.resolve_table_used_content_width(
            rows,
            &grid,
            style,
            stylesheets,
            columns,
            available_table_width,
            table_cellpadding,
            table_metrics.clone(),
            collapsed_geometry.as_ref(),
            &mut table_width,
        );
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            columns,
            LogicalInlineContentSize::new(table_width.content_width),
            table_root_distributes_extra_inline_space(style),
            table_cellpadding,
            table_metrics.clone(),
            collapsed_geometry.as_ref(),
        );
        let used_table_width = column_plan.total_width();
        let provisional_caption_width =
            PhysicalContentWidth::new(content_box_pt(used_table_width.points()));
        let repeating_header_rows = table_repeating_header_row_indices(rows);
        let repeating_footer_rows = table_repeating_footer_row_indices(rows);
        let wrapper_non_grid_block_size = layout_pt(
            self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                provisional_caption_width,
                CaptionSide::Top,
            ) + self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                provisional_caption_width,
                CaptionSide::Bottom,
            ),
        );

        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics: table_metrics.clone(),
            collapsed_geometry: collapsed_geometry.as_ref(),
            wrapper_border_box_block_size,
            wrapper_non_grid_block_size,
        };
        let table_height_plan = self.table_height_plan(&table_context);
        self.restore(table_measurement_snapshot);
        self.fixed_layers.truncate(fixed_layer_start);
        let planned_row_heights = table_height_plan.final_row_heights();
        let source_row_heights = table_height_plan.source_row_heights();
        let planned_row_occupancy = table_height_plan.row_occupancy();
        let table_height_is_definite = table_height_plan.target_content_height.is_some();
        let repeating_header_height = repeated_table_rows_height(
            &repeating_header_rows,
            &planned_row_heights,
            &planned_row_occupancy,
            table_metrics.clone(),
        );
        let repeating_footer_height = repeated_table_rows_height(
            &repeating_footer_rows,
            &planned_row_heights,
            &planned_row_occupancy,
            table_metrics.clone(),
        );
        let row_group_spans = table_row_group_spans(rows);
        let avoid_break_row_groups = row_group_spans
            .iter()
            .filter_map(|(start, end, row_group)| {
                let row_group_style = self.style_for_table_row_group(row_group, style, stylesheets);
                fragmentainer_kind
                    .avoids_break_inside(&row_group_style)
                    .then_some(TableAvoidRowGroup::new(*start, *end))
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
        let table_content_height = table_content_height(
            &planned_row_heights,
            &planned_row_occupancy,
            table_metrics.clone(),
        );
        // CSS Tables computes its tracks in the table root's logical axes.
        // Floats and the parent block formatting context consume the resulting
        // physical wrapper box, so project once at this boundary rather than
        // treating the logical inline extent as a physical width.
        // <https://drafts.csswg.org/css-tables-3/#table-layout>
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let table_axes = TableAxes::for_style(style);
        let wrapper_geometry = TableWrapperPaintBox {
            table_x: 0.0,
            top: 0.0,
            axes: table_axes,
            grid_size: TableGridLogicalSize::new(
                used_table_width,
                LogicalBlockContentSize::new(content_box_pt(table_content_height)),
            ),
            table_width,
            table_metrics: table_metrics.clone(),
        };
        let physical_grid_box = wrapper_geometry.clone().grid_content_box();
        let physical_grid_width = wrapper_geometry.clone().physical_grid_width();
        let physical_border_box = wrapper_geometry.border_box();
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
        let table_border_box_width = physical_border_box.width();
        let table_border_box_height = physical_border_box.height();
        let mut used_style = style.clone();
        resolve_normal_flow_auto_margins_for_known_width(
            &mut used_style,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
            border_box_pt(table_border_box_width),
            self.containing_block_direction,
        );
        let style = &used_style;
        let table_collision_height = table_wrapper_collision_height_for_border_box(
            style,
            table_border_box_height,
            top_caption_height,
            bottom_caption_height,
        );
        self.cursor_y -= style.margin.top;
        // An orthogonal table owns fragmentation along its logical block
        // axis. Its physical height is not an indivisible block-flow extent,
        // so applying the wrapper prebreak heuristic here would move an
        // otherwise fitting vertical table to a new page.
        // <https://drafts.csswg.org/css-tables-3/#table-layout>
        // <https://www.w3.org/TR/css-break-3/#breaks-between>
        if !style.writing_mode.has_vertical_lines() {
            self.prebreak_table_wrapper_if_needed(
                fragmentainer_kind,
                margin_box_pt(table_collision_height),
                style.margin.top,
            );
        }
        let placement = self.place_float_avoiding_margin_box(
            PageTopBlockPosition::new(self.cursor_y),
            margin_box_size_pt(
                style.margin.left + table_border_box_width + style.margin.right,
                table_collision_height,
            ),
            style.clear,
            style.writing_mode,
            style.used_direction(),
            self.containing_block_direction,
        );
        self.cursor_y = placement.origin.top_y();
        let table_outer_x = placement.origin.x() + style.margin.left + relative_offset.x();
        // CSS table wrappers paint borders/padding around the table grid; the
        // grid itself starts at the content-box inline-start edge.
        let table_x = table_width.content_x(table_outer_x);

        self.push_float_context();
        let table_wrapper_top = self.cursor_y;
        let positioning_containing_block_mode =
            PositionedContainingBlockMode::for_element(element, style);
        let positioned_containing_block_scope = if let Some(mode) =
            positioning_containing_block_mode
        {
            let containing_block =
                ContainingBlock::from_page_top_rect(table_wrapper_positioning_containing_block(
                    table_x,
                    table_wrapper_top,
                    physical_grid_width,
                    physical_grid_box.height(),
                    table_width,
                    top_caption_height,
                    bottom_caption_height,
                ));
            Some(self.push_positioned_containing_block(mode, containing_block))
        } else {
            None
        };
        // A table wrapper owns caption, grid, and positioned-descendant paint
        // as one containment principal box. Keep a checkpoint that spans the
        // whole wrapper so the final padding-edge effect can cover captions
        // as well as the independently-recorded table grid.
        // <https://www.w3.org/TR/css-contain-1/#containment-paint>
        let table_wrapper_paint_checkpoint = self.current_page.paint_checkpoint();
        let table_wrapper_paint_page_index = self.pages.len();
        let paint_containment_applies = paint_containment_applies_to_element(element, style);
        let top_caption_paint_checkpoint = self.current_page.paint_checkpoint();
        let top_caption_paint_page_index = self.pages.len();
        let top_caption_clip = PageTopRect::new(
            table_x - table_width.padding.left,
            table_wrapper_top,
            physical_grid_width.points() + table_width.padding.left + table_width.padding.right,
            top_caption_height,
        );
        let top_caption_clip_active = if paint_containment_applies {
            self.push_overflow_clip(top_caption_clip.overflow_clip());
            true
        } else {
            false
        };
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            table_x - table_width.padding.left - table_width.border_widths.left,
            table_border_box_width,
            CaptionSide::Top,
        );
        self.pop_overflow_clip(top_caption_clip_active);
        let table_box_top = self.cursor_y;
        self.cursor_y -= table_width.border_widths.top + table_width.padding.top;
        let table_edge_spacing =
            table_vertical_edge_spacing(&planned_row_occupancy, table_metrics.clone());
        self.cursor_y -= table_edge_spacing;

        let table_is_document_canvas = self
            .document_canvas_overflow
            .is_document_canvas_flow_element(element);
        let table_structure_paint_checkpoint = self.current_page.paint_checkpoint();
        let table_structure_paint_page_index = self.pages.len();
        let table_paint_box = TableWrapperPaintBox {
            table_x,
            top: table_box_top,
            axes: TableAxes::for_style(style),
            grid_size: TableGridLogicalSize::new(
                used_table_width,
                LogicalBlockContentSize::new(content_box_pt(table_content_height)),
            ),
            table_width,
            table_metrics: table_metrics.clone(),
        };
        let source_grid_placement = table_paint_box.clone().grid_placement();
        if paint_containment_applies && self.pages.len() == top_caption_paint_page_index {
            let fragment = self
                .current_page
                .paint_tree_fragment_since(&top_caption_paint_checkpoint);
            let fragment =
                fragment.with_effect_scoped_to_rect_all_bands(top_caption_clip.paint_clip());
            self.current_page
                .replace_paint_tree_since_with_fragment(&top_caption_paint_checkpoint, fragment);
        }
        if self.pages.len() == table_structure_paint_page_index {
            let bounds = table_paint_box.clone().border_box().paint_clip();
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
        // Captions and structural probes above can also inspect table-cell
        // positioned descendants. Remove only this table's speculative fixed
        // layers immediately before the committed row paint pass.
        self.fixed_layers
            .retain(|layer| !table_subtree_ids.contains(&layer.source_element));
        self.fragment_top_offsets
            .push(self.current_page_context.top() - self.cursor_y);
        let (
            mut table_body_fragment,
            forced_break_after_table_rows,
            current_fragment_repeat_policy,
            continuation_inline_offset,
        ) = self.layout_table_body_rows(TableBodyRowsInput {
            fragmentainer_kind,
            rows,
            grid: &grid,
            columns,
            style,
            stylesheets,
            table_x,
            source_grid_placement,
            logical_inline_extent: used_table_width,
            physical_grid_width,
            table_cellpadding,
            column_plan: &column_plan,
            planned_row_heights: &planned_row_heights,
            source_row_heights: &source_row_heights,
            planned_row_occupancy: &planned_row_occupancy,
            table_height_is_definite,
            table_width,
            table_metrics: table_metrics.clone(),
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
        let table_x = continuation_inline_offset.resolve(self.content_left);
        let table_body_commit_context = TableBodyFragmentCommitContext {
            rows,
            grid: &grid,
            columns,
            style,
            stylesheets,
            table_x,
            continuation_inline_offset,
            logical_inline_extent: used_table_width,
            physical_grid_width,
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
        };
        self.commit_table_body_fragment_boundary(
            &mut table_body_fragment,
            &table_body_commit_context,
            TableFragmentBoundaryDecision::new(
                current_fragment_repeat_policy,
                TableFragmentFooterAction::RecordOnly,
            ),
        );
        self.fragment_top_offsets.pop();
        self.cursor_y -= table_edge_spacing;

        self.cursor_y -= table_width.padding.bottom + table_width.border_widths.bottom;
        let bottom_caption_paint_checkpoint = self.current_page.paint_checkpoint();
        let bottom_caption_paint_page_index = self.pages.len();
        let bottom_caption_clip = PageTopRect::new(
            table_x - table_width.padding.left,
            self.cursor_y,
            physical_grid_width.points() + table_width.padding.left + table_width.padding.right,
            bottom_caption_height,
        );
        let bottom_caption_clip_active = if paint_containment_applies {
            self.push_overflow_clip(bottom_caption_clip.overflow_clip());
            true
        } else {
            false
        };
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            table_x - table_width.padding.left - table_width.border_widths.left,
            table_border_box_width,
            CaptionSide::Bottom,
        );
        self.pop_overflow_clip(bottom_caption_clip_active);
        if paint_containment_applies && self.pages.len() == bottom_caption_paint_page_index {
            let fragment = self
                .current_page
                .paint_tree_fragment_since(&bottom_caption_paint_checkpoint);
            let fragment =
                fragment.with_effect_scoped_to_rect_all_bands(bottom_caption_clip.paint_clip());
            self.current_page
                .replace_paint_tree_since_with_fragment(&bottom_caption_paint_checkpoint, fragment);
        }
        if paint_containment_applies && self.pages.len() == table_wrapper_paint_page_index {
            // The canonical paint tree retains caption ordering and scopes
            // while the wrapper is promoted into its containment context.
            let fragment = self
                .current_page
                .take_paint_fragment_since(table_wrapper_paint_checkpoint);
            let wrapper_clip = PageTopRect::new(
                table_x - table_width.padding.left,
                table_wrapper_top,
                physical_grid_width.points() + table_width.padding.left + table_width.padding.right,
                top_caption_height + table_border_box_height + bottom_caption_height,
            )
            .paint_clip();
            let fragment = fragment.with_effect_scoped_to_rect_all_bands(wrapper_clip);
            self.current_page
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
        }

        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        self.pop_float_context();
        // The row pass still advances its legacy cursor by the logical table
        // block extent. For an unfragmented table without captions, convert
        // that consumed extent back to the physical height occupied in the
        // parent's flow. Fragmented and captioned tables retain their existing
        // cursor path until their fragment/caption geometry is likewise
        // logical.
        if top_caption_height <= 0.0
            && bottom_caption_height <= 0.0
            && self.pages.len() == table_structure_paint_page_index
        {
            let physical_grid_height = FlowAxes::for_style(style)
                .physical_size_from_logical(LogicalSize {
                    inline: used_table_width.points(),
                    block: table_content_height,
                })
                .height;
            self.cursor_y += table_content_height - physical_grid_height;
        }
        self.cursor_y -= style.margin.bottom;
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y();
        }
        self.apply_forced_break_in(
            fragmentainer_kind,
            FragmentBreakContext::for_standalone_box(style)
                .forced_break_after_or_in(fragmentainer_kind, forced_break_after_table_rows),
        );
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Move a fragmentable table wrapper before it paints captions or grid
    /// content when its opening margin box overflows the active fragmentainer.
    ///
    /// A table wrapper can split internally, so the generic BFC prebreak
    /// helper cannot be used here: it only advances page fragmentainers and
    /// rejects a box taller than a page. Tables in multicolumn flow instead
    /// need the ordinary fragmentainer advance gate before their top captions
    /// establish a fragment-local table origin. Once the wrapper begins in the
    /// selected destination, row layout remains free to fragment normally.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    fn prebreak_table_wrapper_if_needed(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        margin_box_height: MarginBoxLength,
        reapplied_margin_top: f32,
    ) {
        const EPSILON: f32 = 0.01;

        let overflows = margin_box_height.points() > EPSILON
            && self.cursor_y - margin_box_height.points() < self.page_bottom() - EPSILON;
        let can_advance = self.fragmentainer_materializes_cursor(fragmentainer_kind)
            && self.out_of_flow_prebreak_suppression_depth == 0
            && self.current_page_has_content()
            // A prebreak never creates an empty opening fragmentainer. This
            // applies to columns as well as pages: the table body owns later
            // row fragmentation once its opening wrapper starts at a column
            // edge.
            && !self.cursor_is_at_page_top();
        let should_advance = FragmentAdvanceDecision::choose(FragmentAdvanceInput {
            break_is_applicable: true,
            overflows,
            can_advance,
        })
        .should_advance;
        if should_advance
            && self
                .materialize_table_fragmentainer_advance(
                    fragmentainer_kind,
                    FragmentainerAdvance::Unforced,
                )
                .is_some()
        {
            self.cursor_y -= reapplied_margin_top;
        }
    }
}

fn collect_table_subtree_element_ids(element: &Element, ids: &mut HashSet<crate::dom::ElementId>) {
    ids.insert(element.id);
    for child in &element.children {
        if let NodeKind::Element(child) = &child.kind {
            collect_table_subtree_element_ids(child, ids);
        }
    }
}
