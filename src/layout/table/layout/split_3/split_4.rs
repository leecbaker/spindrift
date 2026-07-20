use super::*;
use crate::layout::block::child_available_space_for_formatting_context;
use crate::layout::inline_collect::InlinePlacement;

/// Resolve a table part's relative offset against its immediate table parent.
///
/// CSS Tables commits a used track size for painting even when that parent's
/// specified block size is `auto`. CSS percentage insets must retain the
/// latter definiteness distinction rather than using every committed track as
/// a percentage basis:
/// <https://drafts.csswg.org/css-position-3/#relative-positioning> and
/// <https://drafts.csswg.org/css-sizing-3/#definite>.
fn table_part_relative_position_offset(
    style: &ComputedStyle,
    parent_style: &ComputedStyle,
    parent_inline_size: f32,
) -> RelativeOffset {
    let block_basis = parent_style
        .box_values
        .height
        .length_if_no_percent()
        .map(|height| PercentageBasis::definite(content_box_pt(height)))
        .unwrap_or_else(PercentageBasis::indefinite);
    relative_position_offset_with_bases(
        style,
        PercentageBasis::definite(content_box_pt(parent_inline_size)),
        // A table's committed row and row-group track height is not its
        // percentage basis. Only an authored, non-percentage height on the
        // immediate table parent is definite here.
        block_basis,
    )
}

impl<'a> LayoutBuilder<'a> {
    /// Restrict an oversized-row slice to a shared table-cell child boundary.
    ///
    /// Table rows are fragmentation containers, but their cells are block
    /// containers. Selecting a raw remaining-height slice can therefore cut
    /// through a child which would fit intact on the next page. Measure the
    /// same final child block spans used by cell replay and move the outgoing
    /// row boundary before every such child. Inline sequences intentionally
    /// remain eligible for their own line-level break opportunities.
    ///
    /// <https://www.w3.org/TR/css-break-3/#break-within>
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_row_child_boundary_piece_height(
        &mut self,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        grid: &TableGrid,
        row_index: usize,
        stylesheets: &[Stylesheet],
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        piece_offset: f32,
        proposed_piece_height: f32,
        remaining_row_height: f32,
        fresh_fragmentainer_block_size: f32,
    ) -> f32 {
        const EPSILON: f32 = 0.01;
        let mut piece_end = piece_offset + proposed_piece_height;
        if proposed_piece_height <= EPSILON {
            return 0.0;
        }

        // Moving a boundary before one cell child can expose a crossing child
        // in another cell. Iterate to the greatest boundary which crosses no
        // child that could instead fit in a fresh fragmentainer.
        loop {
            let mut restricted_end = piece_end;
            let mut last_shared_boundary = piece_offset;
            let mut source_child_end = piece_offset;
            for placement in &grid.rows[row_index] {
                let cell = &row.cells[placement.cell];
                let Some(prepared) = self.prepare_table_cell(
                    cell,
                    row,
                    row_style,
                    placement,
                    row_index,
                    0.0,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics.clone(),
                    collapsed_geometry,
                ) else {
                    continue;
                };
                let available_width = (prepared.width()
                    - prepared.borders.left
                    - prepared.borders.right
                    - prepared.style.padding.left
                    - prepared.style.padding.right)
                    .max(1.0);
                let Some(children) = cell.children.as_deref() else {
                    continue;
                };
                let mut child_block_start = 0.0;
                for child in children {
                    // HTML block-flow fixup commonly wraps consecutive cell
                    // children in an anonymous block. That wrapper is not a
                    // source-level atomic child, so expose its direct block
                    // children to the fragment-break scheduler.
                    let break_children = match child {
                        box_tree::FormattingBox::AnonymousBlock(box_) => box_.children.as_slice(),
                        _ => std::slice::from_ref(child),
                    };
                    for break_child in break_children {
                        let inline_sequence = self.table_cell_nested_inline_sequence_for_child(
                            break_child,
                            stylesheets,
                            available_width,
                            PercentageBasis::indefinite(),
                        );
                        if inline_sequence.is_none()
                            && !table_cell_has_in_flow_layout_child(break_child)
                        {
                            continue;
                        }
                        let child_height = inline_sequence
                            .as_ref()
                            .map(|plan| plan.sequence.total_height())
                            .unwrap_or_else(|| {
                                self.table_cell_final_relayout_child_height(
                                    break_child,
                                    stylesheets,
                                    available_width,
                                    PercentageBasis::indefinite(),
                                )
                            });
                        if child_height <= EPSILON {
                            continue;
                        }
                        let child_block_end = child_block_start + child_height;
                        if child_block_start > piece_offset + EPSILON
                            && child_block_start < piece_end - EPSILON
                        {
                            last_shared_boundary = last_shared_boundary.max(child_block_start);
                        }
                        source_child_end = source_child_end.max(child_block_end);
                        // A nested inline sequence has class-C line opportunities
                        // inside it. Other source children are replayed atomically
                        // by the table-cell fragment plan, so keep them intact
                        // whenever they fit on a fresh page.
                        if inline_sequence.is_none()
                            && child_block_start + EPSILON < piece_end
                            && child_block_end > piece_end + EPSILON
                            && child_height <= fresh_fragmentainer_block_size + EPSILON
                        {
                            restricted_end = restricted_end.min(child_block_start);
                        }
                        child_block_start = child_block_end;
                    }
                }
                // The row's border/spacing contribution can make its source span
                // larger than the sum of its cell children. Do not consume all
                // children in a slice which would leave only that trailing row
                // contribution for the next page; back up to a real child
                // boundary so the destination receives a paintable fragment.
                if remaining_row_height > proposed_piece_height + EPSILON
                    && source_child_end <= piece_end + EPSILON
                    && last_shared_boundary > piece_offset + EPSILON
                {
                    restricted_end = restricted_end.min(last_shared_boundary);
                }
            }
            if restricted_end >= piece_end - EPSILON {
                return (piece_end - piece_offset).max(0.0);
            }
            piece_end = restricted_end;
            if piece_end <= piece_offset + EPSILON {
                return 0.0;
            }
        }
    }

    /// Defer a relatively positioned table part's recorded paint to the
    /// positioned-auto painting band after applying its visual translation.
    ///
    /// Table-grid track geometry remains in grid space; CSS relative
    /// positioning changes the part's final visual paint position and its
    /// Appendix E stacking slot only.
    /// <https://drafts.csswg.org/css-position-3/#relative-positioning>
    fn scope_relative_table_part_paint(
        &mut self,
        checkpoint: &PaintCheckpoint,
        paint_page_index: usize,
        positioned_layer_start: usize,
        style: &ComputedStyle,
        offset: RelativeOffset,
        bounds: PaintClip,
    ) {
        if offset.is_zero() || self.pages.len() != paint_page_index {
            return;
        }

        let translation = PaintTranslation::new(offset.x(), offset.y());
        for layer in &mut self.positioned_layers[positioned_layer_start..] {
            *layer = layer.clone().translated(translation);
        }
        let bounds = PaintClip::new(
            bounds.x() + offset.x(),
            bounds.y() + offset.y(),
            bounds.width(),
            bounds.height(),
        );
        let fragment = self
            .current_page
            .take_paint_fragment_since(checkpoint.clone())
            .translated(translation);
        if fragment.is_empty() {
            return;
        }
        let policy = StackingContextPolicy::for_non_positioned_style_effect(style, bounds);
        let context = PaintStackingContext::from_banded_fragment_with_stack_level(
            policy.stack_level,
            fragment.clone(),
            Vec::new(),
        )
        .with_source_order(self.next_paint_source_order())
        .with_effects(policy.effects)
        .with_bounds(bounds);
        self.positioned_layers.push(PositionedPaintLayer {
            page_index: paint_page_index,
            source_style: style.clone(),
            source_is_target: false,
            stack_level: policy.stack_level,
            context,
            links: fragment.links,
            escaped_atom_translation: if self.escaped_atom_positioning_depth > 0 {
                EscapedAtomTranslation::normal_flow_fragment()
            } else {
                EscapedAtomTranslation::none()
            },
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn paint_repeated_table_row_group_background(
        &mut self,
        table_x: f32,
        used_table_width: f32,
        local_row_tops: &[f32],
        local_row_heights: &[f32],
        start: usize,
        end: usize,
        fill: CssColor,
    ) {
        if let Some(bounds) = table_fragment_row_span_bounds(
            PageInlineSpan::new(table_x, used_table_width),
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
            PageInlineSpan::new(table_x, used_table_width),
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
        let (structural_backgrounds, structural_outlines, relative_structural_paints) = self
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
        for relative_paint in relative_structural_paints {
            let policy = StackingContextPolicy::for_non_positioned_style_effect(
                &relative_paint.style,
                relative_paint.bounds,
            );
            let paint_fragment =
                PaintFragment::from_primitives(relative_paint.primitives, Vec::new());
            let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                policy.stack_level,
                paint_fragment,
                Vec::new(),
            )
            .with_source_order(self.next_paint_source_order())
            .with_effects(policy.effects)
            .with_bounds(relative_paint.bounds);
            self.positioned_layers.push(PositionedPaintLayer {
                page_index: fragment_state.plan.page_index,
                source_style: relative_paint.style,
                source_is_target: false,
                stack_level: policy.stack_level,
                context,
                links: Vec::new(),
                escaped_atom_translation: if self.escaped_atom_positioning_depth > 0 {
                    EscapedAtomTranslation::normal_flow_fragment()
                } else {
                    EscapedAtomTranslation::none()
                },
            });
        }

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
                        let grid = placement.page_top_rect_with_block_edge_spacing(
                            TableGridLength::new(table_vertical_edge_spacing(
                                &[fragment_has_occupied_row],
                                table_metrics.clone(),
                            )),
                        );
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
            &policy,
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
        source_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_height_is_definite: bool,
        table_metrics: TableMetrics,
        grid_placement: Option<TableGridPlacement>,
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
        // they have no containment principal box. Their positioned and
        // transformed principal boxes still establish containing blocks for
        // absolutely positioned descendants.
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
        let row_group_establishes_containing_block = matches!(
            row_style.position,
            Position::Relative | Position::Absolute | Position::Fixed | Position::Sticky
        ) || row_style.has_transform()
            || row_has_applicable_containment
            || row
                .row_groups
                .last()
                .map(|group| {
                    let style = self.style_for_table_row_group(group, table_style, stylesheets);
                    matches!(
                        style.position,
                        Position::Relative
                            | Position::Absolute
                            | Position::Fixed
                            | Position::Sticky
                    ) || style.has_transform()
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
        let row_group_positioning_style = row
            .row_groups
            .last()
            .map(|group| self.style_for_table_row_group(group, table_style, stylesheets))
            .filter(|style| matches!(style.position, Position::Relative | Position::Sticky));
        let positioned_table_part_style =
            if matches!(row_style.position, Position::Relative | Position::Sticky) {
                row_style
            } else {
                row_group_positioning_style.as_deref().unwrap_or(row_style)
            };

        for placement in &grid.rows[row_index] {
            let cell = &row.cells[placement.cell];
            // Cell metrics are a measurement input for the committed row
            // piece. Their nested relayout must not create a page transition
            // before this row fragment has selected and recorded its own
            // boundary.
            let prepared_snapshot = self.snapshot();
            let prepared = self.prepare_table_cell(
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
            );
            self.restore(prepared_snapshot);
            let Some(prepared) = prepared else {
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
            let cell_min_block_size = table_cell_root_block_track_contribution(
                self,
                cell,
                cell_style,
                table_style,
                stylesheets,
                Some(cell_borders),
                metrics.border_box_height,
            );
            let cell_height = row_span_block_size.max(cell_min_block_size);
            // Keep the fragment's root placement through the cell boundary.
            // In vertical and RTL tables a row track is not a physical-Y
            // offset, so reconstructing an LTR page origin here loses the
            // root's logical block direction.
            let cell_placement = grid_placement.unwrap_or_else(|| {
                TableGridPlacement::with_axes(
                    PageTopPoint::new(table_x, grid_origin_top),
                    table_axes,
                    TableGridLogicalSize::new(
                        logical_inline_extent,
                        LogicalBlockContentSize::new(content_box_pt(logical_block_extent)),
                    ),
                )
            });
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
            let cell_row_baseline_offset =
                if !table_cell_participates_in_baseline(cell_style, row_style) {
                    None
                } else if percentage_height_basis.is_definite()
                    && cell.children.as_deref().is_some_and(|children| {
                        children
                            .iter()
                            .any(table_cell_formatting_child_has_parent_percentage_block_size)
                    })
                {
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
            let cell_axes = TableCellAxisAdapter::for_cell(table_style, cell_style);
            let unaligned_content_box =
                cell_border_box.content_box(cell_placement, cell_style.padding, cell_borders);
            let unaligned_content_geometry = cell_axes.content_geometry(unaligned_content_box, 0.0);
            // Table-cell alignment is defined over the final constrained
            // fragment, not the cell's unconstrained intrinsic contribution.
            // Build that same line plan against the unaligned rectangle, then
            // restore the measurement state before committing the translated
            // content geometry and paint plan below. Moving along the cell
            // block axis does not change this plan's inline constraint.
            // <https://drafts.csswg.org/css-tables-3/#table-cell-content-layout-second-pass>
            let alignment_plan = if cell_axes.cell_inline_uses_physical_width() {
                None
            } else {
                let alignment_snapshot = self.snapshot();
                let plan = self.plan_table_cell_content(
                    cell,
                    row,
                    cell_style,
                    stylesheets,
                    unaligned_content_geometry,
                    &text,
                    row_top,
                    piece_height,
                    piece_offset,
                    row_fragment_mode,
                    percentage_height_basis,
                );
                self.restore(alignment_snapshot);
                Some(plan)
            };
            let subject_block_size = if cell_axes.cell_inline_uses_physical_width() {
                final_metrics.content_height
            } else {
                alignment_plan
                    .as_ref()
                    .map(|plan| plan.logical_block_subject_size(cell_style))
                    .filter(|size| *size > 0.0)
                    .unwrap_or_else(|| {
                        self.table_cell_content_alignment_subject_width(
                            cell,
                            cell_style,
                            stylesheets,
                            cell_borders,
                            unaligned_content_geometry.block_size().points(),
                        )
                    })
            };
            let content_block_offset = self.table_cell_content_block_offset(
                cell_style,
                unaligned_content_geometry,
                subject_block_size,
                cell_row_baseline_offset,
                final_metrics.baseline_offset,
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
                source_row_heights,
                planned_row_occupancy,
                table_metrics.clone(),
                cell_border_box,
                cell_placement,
            );
            let collapsed_column_content_clip = column_plan
                .span_contains_collapsed_column(placement.column, placement.colspan)
                .then(|| {
                    TableCellClipRegion::from_clip(
                        cell_placement.overflow_clip_for(cell_border_box.rect()),
                    )
                });
            let collapsed_content_clip =
                match (collapsed_content_clip, collapsed_column_content_clip) {
                    (Some(rows), Some(columns)) => rows.intersect(&columns),
                    (Some(clip), None) | (None, Some(clip)) => Some(clip),
                    (None, None) => None,
                };
            let paint_containment_clip = self.table_cell_content_clip(
                cell_style,
                cell_border_box,
                cell_placement,
                cell_borders,
            );
            let content_clip = match (collapsed_content_clip.clone(), paint_containment_clip) {
                (Some(collapsed), Some(containment)) => {
                    collapsed.intersect(&TableCellClipRegion::from_clip(containment))
                }
                (Some(clip), None) => Some(clip),
                (None, Some(clip)) => Some(TableCellClipRegion::from_clip(clip)),
                (None, None) => None,
            };
            let mut cell_fragment_plan = TableCellFragmentPlan {
                border_box: cell_border_box,
                placement: cell_placement,
                content_geometry,
                content_clip,
                area: prepared.area,
                content: TableCellContentPlan::empty(),
            };
            cell_fragment_plan.content = self.plan_table_cell_content(
                cell,
                row,
                cell_style,
                stylesheets,
                content_geometry,
                &text,
                row_top,
                piece_height,
                piece_offset,
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
                let mut cell_paint_style = collapsed_cell_decoration_style(
                    cell_style,
                    table_metrics.border_collapse == css::BorderCollapse::Collapse,
                );
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
                let decoration_rect = paint_space_rect(
                    cell_fragment_plan.x(),
                    cell_fragment_plan.top_y() - cell_fragment_plan.height(),
                    cell_fragment_plan.width(),
                    cell_fragment_plan.height(),
                );
                let (rects, rounded_rects, paths, strokes) = block_paint_ops_with_phases(
                    decoration_rect,
                    &cell_paint_style,
                    cell_borders,
                    false,
                    true,
                    true,
                    false,
                );
                let (border_rects, border_rounded_rects, border_paths, border_strokes) =
                    block_paint_ops_with_phases(
                        decoration_rect,
                        &cell_paint_style,
                        cell_borders,
                        false,
                        false,
                        false,
                        table_metrics.border_collapse != css::BorderCollapse::Collapse,
                    );
                // The collapsed grid rules are prepended to this band after
                // all cells have been laid out. Keeping the cell decoration
                // in the same retained band therefore paints the rule first,
                // followed by the cell's padding-edge background. This is
                // the collapsed-border painting order without relying on
                // coincident antialiased edges in separate bands.
                let decoration_band =
                    if table_metrics.border_collapse == css::BorderCollapse::Collapse {
                        PaintBand::TableCollapsedBorder
                    } else {
                        PaintBand::BackgroundBorder
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
                for rect in border_rects {
                    self.push_rect_in_band(PaintBand::TableCellBorder, rect);
                }
                for rounded_rect in border_rounded_rects {
                    self.push_rounded_rect_in_band(PaintBand::TableCellBorder, rounded_rect);
                }
                for path in border_paths {
                    self.push_path_in_band(PaintBand::TableCellBorder, path);
                }
                for stroke in border_strokes {
                    self.push_stroke_in_band(PaintBand::TableCellBorder, stroke);
                }
            }

            // Cell decorations belong to the table background/border phase.
            // A cell overflow clip scopes only descendants, not the cell's
            // own border or background; otherwise an overflowing scrollport
            // clips the border it is painted inside.
            // <https://www.w3.org/TR/CSS22/zindex.html>
            let cell_content_paint_checkpoint = self.current_page.paint_checkpoint();

            // Retain ordinary overflow as a paint effect so descendants keep
            // their unmodified layout geometry.  The PDF clip is installed
            // below when the cell's retained paint subtree is scoped.  Row
            // and column collapse instead removes portions of the table grid
            // itself, so those holes must cull primitives during layout.
            // <https://drafts.csswg.org/css-tables-3/#visibility-collapse-cell-rendering>
            let clip_active = if let Some(clip) = collapsed_content_clip
                .as_ref()
                .and_then(TableCellClipRegion::bounding_clip)
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
                let content_box = cell_fragment_plan.content_box();
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
                        cell_fragment_plan.content_geometry,
                        &cell_fragment_plan.content,
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
                        cell_fragment_plan.content_geometry,
                    );
                }
            }
            if cell_has_paintable_area && !planned_flow_children {
                self.layout_table_cell_replaced_children(
                    cell,
                    cell_style,
                    cell_fragment_plan.content_box(),
                );
            }
            self.layout_table_cell_positioned_children(
                cell,
                row,
                positioned_table_part_style,
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
                    || cell_style.contain.paint
                    // Overflow and collapsed-track clips also have to scope
                    // the retained paint commands. The immediate clip stack
                    // only rejects wholly outside content; without this
                    // scope, glyph runs and positioned descendants that
                    // cross the edge escape when the PDF is emitted.
                    || cell_fragment_plan.content_clip.is_some())
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
                if let Some(content_clip) = &cell_fragment_plan.content_clip {
                    let clips = content_clip.paint_clips();
                    policy.effects.overflow_clip_union = PaintClipUnion::from_clips(&clips);
                }
                let child_contexts = self.positioned_child_contexts_since(
                    cell_positioned_layer_start,
                    cell_paint_page_index,
                    &policy,
                );
                self.scope_current_page_paint_since_with_policy(
                    &cell_content_paint_checkpoint,
                    policy,
                    bounds,
                    child_contexts,
                );
            }
            self.scope_relative_table_part_paint(
                &cell_paint_checkpoint,
                cell_paint_page_index,
                cell_positioned_layer_start,
                cell_style,
                table_part_relative_position_offset(cell_style, row_style, used_table_width),
                PageTopRect::new(
                    cell_fragment_plan.x(),
                    cell_fragment_plan.top_y(),
                    cell_fragment_plan.width(),
                    cell_fragment_plan.height(),
                )
                .paint_clip(),
            );
            if let Some(scope) = cell_containing_block_scope {
                self.pop_positioned_containing_block(scope);
            }
        }
        self.pop_overflow_clip(row_piece_clip_active);
        let row_group_relative_style = row
            .row_groups
            .last()
            .map(|group| self.style_for_table_row_group(group, table_style, stylesheets))
            .filter(|style| matches!(style.position, Position::Relative | Position::Sticky));
        let relative_style = matches!(row_style.position, Position::Relative | Position::Sticky)
            .then_some(row_style)
            .or(row_group_relative_style.as_deref());
        if let Some(relative_style) = relative_style {
            let offset = if std::ptr::eq(relative_style, row_style) {
                if let Some(group) = row.row_groups.last() {
                    let parent_style =
                        self.style_for_table_row_group(group, table_style, stylesheets);
                    table_part_relative_position_offset(
                        relative_style,
                        parent_style.as_computed(),
                        used_table_width,
                    )
                } else {
                    table_part_relative_position_offset(
                        relative_style,
                        table_style,
                        used_table_width,
                    )
                }
            } else {
                table_part_relative_position_offset(relative_style, table_style, used_table_width)
            };
            self.scope_relative_table_part_paint(
                &row_paint_checkpoint,
                row_paint_page_index,
                row_positioned_layer_start,
                relative_style,
                offset,
                PageTopRect::new(table_x, row_top, used_table_width, piece_height).paint_clip(),
            );
        }
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
                &policy,
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
        if matches!(box_, box_tree::FormattingBox::Text(_)) {
            return false;
        }
        let style = box_.style();
        table_cell_block_size_depends_on_parent_percentage(style.box_values.height.clone())
            && style.box_values.min_height.length_if_no_percent().is_some()
            && matches!(
                effective_overflow_for_style(style),
                css::Overflow::Auto | css::Overflow::Scroll
            )
            || Self::table_cell_children_have_cyclic_percentage_scroll_min_height(box_.children())
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn plan_table_cell_content(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        content_geometry: TableCellContentGeometry,
        text: &str,
        row_top: f32,
        piece_height: f32,
        piece_offset: f32,
        row_fragment_mode: TableRowFragmentMode,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> TableCellContentPlan {
        let mut plan = TableCellContentPlan::empty();
        let available_width = content_geometry.inline_size().points().max(1.0);

        if let Some(children) = cell.children.as_deref()
            && table_cell_children_can_use_inline_line_sequence(children)
        {
            let link_target = table_cell_href(cell).map(str::to_string);
            let sequence = self.with_table_cell_inline_planning_geometry(
                cell_style,
                content_geometry,
                percentage_height_basis,
                |layout| {
                    let mut items = Vec::new();
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
                    (!items.is_empty()).then(|| {
                        layout.collect_inline_line_sequence_for_text_box_trimmed_style(
                            items,
                            cell_style,
                            available_width,
                        )
                    })
                },
            );
            if let Some(sequence) = sequence {
                plan.inline_sequence = Some(sequence);
                plan.children_painted_by_inline_sequence = true;
                return plan;
            }
        }

        if cell.children.is_none() {
            if let Some(element) = cell.element {
                let link_target = table_cell_href(cell).map(str::to_string);
                let sequence = self.with_table_cell_inline_planning_geometry(
                    cell_style,
                    content_geometry,
                    percentage_height_basis,
                    |layout| {
                        let mut items = Vec::new();
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
                        (!items.is_empty()).then(|| {
                            layout.collect_inline_line_sequence_for_text_box_trimmed_style(
                                items,
                                cell_style,
                                available_width,
                            )
                        })
                    },
                );
                plan.inline_sequence = sequence;
            } else if !text.is_empty() {
                let cell_text_box_line_trim =
                    self.effective_text_box_line_trim_for_style(cell_style);
                plan.inline_sequence = Some(self.with_table_cell_inline_planning_geometry(
                    cell_style,
                    content_geometry,
                    percentage_height_basis,
                    |layout| {
                        layout.with_text_box_line_trim_scope(cell_text_box_line_trim, |layout| {
                            layout.inline_line_sequence_for_text(
                                text,
                                cell_style,
                                available_width,
                                0.0,
                                table_cell_href(cell),
                            )
                        })
                    },
                ));
            }
        }

        if row_fragment_mode.replays_flow_children_from_plan()
            && let Some(children) = cell.children.as_deref()
        {
            let child_top = content_geometry.content_box().top_y();
            plan.child_fragments = self.table_cell_child_fragment_plans(
                children,
                stylesheets,
                available_width,
                percentage_height_basis,
                child_top,
                // Block coordinates decrease in the direction of flow. A
                // continuation therefore advances *away* from the source
                // row start even though its destination page-local `row_top`
                // has restarted at the new fragmentainer. Select that source
                // interval explicitly; adding the offset instead selects an
                // empty interval above every continuation and drops later
                // in-flow children before a following `clear`.
                row_top - piece_offset,
                row_top - piece_offset - piece_height,
                // Paint remains in the destination fragmentainer's physical
                // coordinate system. Its clip starts at this piece's local
                // row edge, not at the source interval above.
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
            if let (Some(first), Some(last)) =
                (plan.child_fragments.first(), plan.child_fragments.last())
            {
                plan.fragment_range = Some(TableCellFragmentRange {
                    source_child_start: first.source_child_index,
                    source_child_end: last.source_child_index + 1,
                    source_block_top: plan
                        .child_fragments
                        .iter()
                        .map(|fragment| fragment.source_child_top)
                        .fold(f32::NEG_INFINITY, f32::max),
                    source_block_bottom: plan
                        .child_fragments
                        .iter()
                        .map(|fragment| fragment.source_child_top - fragment.child_height)
                        .fold(f32::INFINITY, f32::min),
                    painted_block_top: row_top,
                    painted_block_bottom: row_top - piece_height,
                    continues_from_previous: plan
                        .child_fragments
                        .iter()
                        .any(|fragment| fragment.metadata.continues_from_previous_page),
                    continues_to_next: plan
                        .child_fragments
                        .iter()
                        .any(|fragment| fragment.metadata.continues_to_next_page),
                });
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
            box_tree::FormattingBox::Inline(box_) => &box_.core.style,
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
        let sequence = self.with_table_cell_inline_planning_scope(
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
                layout.collect_inline_line_sequence_for_text_box_trimmed_style(
                    std::mem::take(&mut items),
                    style,
                    available_width,
                )
            },
        );
        (!sequence.records.is_empty()).then(|| TableCellNestedInlineSequencePlan {
            sequence,
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
        // Inline planning lays out floats in order to produce the reusable
        // line sequence. Their exclusions belong to that provisional table
        // cell BFC, never to a following sibling in the table's parent BFC.
        // <https://www.w3.org/TR/CSS22/tables.html#model>
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        let available_width = available_width.max(1.0);
        let content_left = self.content_left;
        let content_right = self.content_right;
        let content_logical_inline_size_stack = self.content_logical_inline_size_stack.clone();
        let child_available_space_stack = self.child_available_space_stack.clone();
        let definite_block_size_stack = self.definite_block_size_stack.clone();

        self.content_left = 0.0;
        self.content_right = available_width;
        self.content_logical_inline_size_stack.push(available_width);
        let inherited_orthogonal_available_height = self
            .current_child_available_space()
            .orthogonal_available_height;
        self.child_available_space_stack
            .push(child_available_space_for_formatting_context(
                style,
                PhysicalContentWidth::new(content_box_pt(available_width)),
                None,
                inherited_orthogonal_available_height,
                PhysicalContentHeight::new(content_box_pt(self.page_area_height())),
            ));
        self.definite_block_size_stack.push(percentage_height_basis);
        self.push_float_context();

        let result = f(self);

        self.pop_float_context();
        self.content_left = content_left;
        self.content_right = content_right;
        self.content_logical_inline_size_stack = content_logical_inline_size_stack;
        self.child_available_space_stack = child_available_space_stack;
        self.definite_block_size_stack = definite_block_size_stack;
        result
    }

    /// Run inline planning with the final physical content rectangle and the
    /// cell's own logical inline basis.  The scalar helper above remains for
    /// provisional row metrics, whose opposite-axis span is not committed
    /// yet; final content planning must use this geometry instead.
    pub(in crate::layout::table) fn with_table_cell_inline_planning_geometry<T>(
        &mut self,
        style: &ComputedStyle,
        content_geometry: TableCellContentGeometry,
        percentage_height_basis: BlockSizePercentageBasis,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        // Like the scalar row-sizing scope above, final cell-plan collection
        // may place floats while selecting lines. Its float exclusions are
        // local to the table cell, even though the collected line sequence
        // retains the resulting paint operations for its later replay.
        // <https://www.w3.org/TR/CSS22/tables.html#model>
        let content_box = content_geometry.content_box();
        let content_left = self.content_left;
        let content_right = self.content_right;
        let cursor_y = self.cursor_y;
        let containing_block_direction = self.containing_block_direction;
        let containing_block_writing_mode = self.containing_block_writing_mode;
        let content_logical_inline_size_stack = self.content_logical_inline_size_stack.clone();
        let child_available_space_stack = self.child_available_space_stack.clone();
        let definite_block_size_stack = self.definite_block_size_stack.clone();

        self.content_left = 0.0;
        self.content_right = content_box.width();
        self.cursor_y = content_box.height();
        // Inline collection consults the active containing-block flow for
        // line-relative placement in addition to the style passed at the
        // call site. A table root and its cell can be orthogonal, so planning
        // the cell's final inline sequence under the table's flow reverses
        // `text-align` and physical line-axis projection for vertical cells.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        self.containing_block_direction = style.used_direction();
        self.containing_block_writing_mode = style.writing_mode;
        self.content_logical_inline_size_stack
            .push(content_geometry.inline_size().points().max(1.0));
        let inherited_orthogonal_available_height = self
            .current_child_available_space()
            .orthogonal_available_height;
        self.child_available_space_stack
            .push(child_available_space_for_formatting_context(
                style,
                PhysicalContentWidth::new(content_box_pt(content_box.width())),
                Some(PhysicalContentHeight::new(content_box_pt(
                    content_box.height(),
                ))),
                inherited_orthogonal_available_height,
                PhysicalContentHeight::new(content_box_pt(content_box.height())),
            ));
        self.definite_block_size_stack.push(percentage_height_basis);
        self.push_float_context();

        let result = f(self);

        self.pop_float_context();
        self.content_left = content_left;
        self.content_right = content_right;
        self.cursor_y = cursor_y;
        self.containing_block_direction = containing_block_direction;
        self.containing_block_writing_mode = containing_block_writing_mode;
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
        source_slice_top: f32,
        source_slice_bottom: f32,
        paint_slice_top: f32,
        paint_slice_bottom: f32,
    ) -> Vec<TableCellChildFragmentPlan> {
        // Final child relayout is a sizing query. Nested table/flex children
        // may materialize provisional pages while determining their used
        // block size, but those effects must not become a table-row fragment
        // before this caller has committed the row boundary.
        let measurement_snapshot = self.snapshot();
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
            // The source child interval must have a positive intersection
            // with the row piece. A child touching only the outgoing boundary
            // belongs to the destination piece; replaying that zero-height
            // slice would lay it out as a complete child and can spuriously
            // advance the page before the table boundary is committed.
            const FRAGMENT_COORDINATE_EPSILON: f32 = 0.01;
            if child_top > source_slice_bottom + FRAGMENT_COORDINATE_EPSILON
                && child_bottom < source_slice_top - FRAGMENT_COORDINATE_EPSILON
                && let Some(kind) = table_cell_child_fragment_kind(child_box)
            {
                // Source and destination fragmentainers have distinct block
                // origins after a row continuation. Keep the source interval
                // for selection, then project its child geometry into the
                // destination before constructing paint bounds.
                let source_to_paint = paint_slice_top - source_slice_top;
                let painted_child_top = child_top + source_to_paint;
                let painted_child_bottom = painted_child_top - child_height;
                let visible_top = painted_child_top.min(paint_slice_top);
                let visible_bottom = painted_child_bottom.max(paint_slice_bottom);
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
                    (child_top - source_slice_top).abs() <= 0.01,
                );
                metadata.continues_from_previous_page = child_top > source_slice_top + 0.01;
                metadata.continues_to_next_page = child_bottom < source_slice_bottom - 0.01;
                plans.push(TableCellChildFragmentPlan {
                    source_child_index,
                    source_child_top: child_top,
                    painted_child_top,
                    child_height,
                    slice_top: paint_slice_top,
                    slice_bottom: paint_slice_bottom,
                    kind,
                    inline_sequence,
                    nested_fragment: None,
                    metadata,
                });
            }
            child_top = child_bottom;
        }
        self.restore(measurement_snapshot);
        plans
    }
}
