//! Planned cell-child replay and structural table-fragment painting.

use crate::Page;
use crate::css::{
    ComputedStyle, PercentageBasis, Position, SemanticLengthExt, Stylesheets, Visibility, layout_pt,
};
use crate::document::paint::display_list::PaintBand;
use crate::document::paint::effects::PaintEffects;
use crate::document::paint::fragments::PaintFragment;
use crate::document::paint::geometry::{
    PaintClip, PaintPoint, PaintRect, PaintSize, PaintTranslation,
};
use crate::document::paint::page::PaintPrimitive;
use crate::document::paint::shapes::RenderedRect;
use crate::document::paint::stacking::PaintStackingContext;
use crate::dom::Element;
use crate::layout::table::layout::fragmentation::{
    table_column_group_has_explicit_columns, table_columns_paint_in_reverse_page_order,
};
use crate::layout::table::layout::{
    CollapsedTableGeometry, RelativeTablePartStructuralPaint, TableBodyPaintFragment,
    TableCellChildFragmentKind, TableCellChildFragmentPlan, TableCellContentPlan,
    TableCellNestedFragmentPlan, TableCellNestedInlineSequencePlan, TableGridLayoutContext,
    TableHeightDistributionTarget, TableHeightTarget, TableRowHeightPlan,
    TableWrapperDecorationViewport, distribute_table_span_constraint,
    push_table_fragment_row_span_background, table_cell_participates_in_physical_y_row_baseline,
    table_column_fragment_background_image_primitives, table_column_fragment_background_primitives,
    table_column_grid_background_primitives, table_fragment_row_span_bounds,
    table_row_fragment_background_primitives, table_row_grid_background_primitives,
    table_row_group_grid_background_primitives,
};
use crate::layout::table::{
    TableCell, TableCellAxisAdapter, TableCellContentGeometry, TableColumn, TableColumnPlan,
    TableGrid, TableGridLength, TableGridPlacement, TableGridPoint, TableGridRect, TableGridSize,
    TableHeightPlan, TableInlineBounds, TableMetrics, TableRootTrackAxis, TableRow, UsedTableWidth,
    table_cell_root_block_track_contribution, table_column_group_spans, table_root_block_size,
    table_row_group_spans, table_row_is_collapsed, table_vertical_edge_spacing,
};
use crate::layout::{
    BlockSizeBasisSource, ContainingBlock, FragmentPageMetadata, LayoutBuilder, OverflowClip,
    PageInlineSpan, PageTopRect, PaintBackgroundArea, RelativeOffset, ReplacedElementKind,
    TableHeightPlanCacheKey, apply_used_box_metrics,
    background_image_primitives_for_style_with_paint_areas, box_tree, horizontal_border_width,
    normalized_text_for_style, paint_space_rect, percentage_basis_from_points,
    relative_position_offset, replaced_element_kind, used_border_widths, used_content_box_width,
    used_length_percentage_or_auto_with_basis,
};
use crate::units::non_content_pt;

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
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> Option<TableCellNestedFragmentPlan> {
        if !matches!(
            child_box,
            box_tree::FormattingBox::Table(_) | box_tree::FormattingBox::Flex(_)
        ) {
            return None;
        }

        // This fragment plan is replayed into a table cell later; its
        // off-page lines are not eligible anchors for an enclosing list
        // item's outside marker.
        self.with_non_principal_line_capture(|layout, snapshot| {
            let positioned_layer_start = layout.positioned_layers.len();
            layout.ancestors = layout.table_cell_child_ancestors(cell, row);
            let width = available_width.max(1.0);
            let top = 10_000.0;
            layout.current_page = Page::new(width, top);
            layout.overflow_clips.clear();
            layout.truncate_page_start_margins = false;
            let content_scope = layout.enter_table_cell_content_scope_for_rect(
                cell_style,
                PageTopRect::new(0.0, top, width, top),
                None,
                layout.table_cell_child_ancestors(cell, row),
                PercentageBasis::indefinite(),
            );

            layout.layout_formatting_box_with_parent_decoration(
                child_box,
                stylesheets,
                Some(cell_style),
            );
            layout.flush_positioned_layers_since(positioned_layer_start);

            let fragment = layout
                .current_page
                .paint_fragment()
                .translated(PaintTranslation::new(0.0, -top));
            let assignments = layout.captured_current_page_assignment_values();
            let height = (top - layout.cursor_y).max(0.0);
            layout.restore_table_cell_content_scope(content_scope);

            (!fragment.is_empty()).then_some(TableCellNestedFragmentPlan {
                fragment,
                width,
                height,
                metadata: FragmentPageMetadata::empty(snapshot.page_count()),
                assignments,
            })
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn paint_table_cell_planned_child_fragments(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        content_geometry: TableCellContentGeometry,
        content_plan: &TableCellContentPlan,
        overflow_clip: Option<OverflowClip>,
    ) {
        let Some(children) = cell.children.as_deref() else {
            return;
        };
        let child_fragments = &content_plan.child_fragments;
        if child_fragments.is_empty() {
            return;
        }
        if let Some(fragment_range) = content_plan.fragment_range {
            debug_assert!(fragment_range.source_child_start < fragment_range.source_child_end);
            debug_assert!(
                fragment_range.source_block_bottom <= fragment_range.source_block_top + 0.01
            );
            debug_assert!(
                fragment_range.painted_block_bottom <= fragment_range.painted_block_top + 0.01
            );
            debug_assert_eq!(
                fragment_range.continues_from_previous,
                child_fragments
                    .iter()
                    .any(|fragment| fragment.metadata.continues_from_previous_page)
            );
            debug_assert_eq!(
                fragment_range.continues_to_next,
                child_fragments
                    .iter()
                    .any(|fragment| fragment.metadata.continues_to_next_page)
            );
            debug_assert!(child_fragments.iter().all(|fragment| {
                (fragment_range.source_child_start..fragment_range.source_child_end)
                    .contains(&fragment.source_child_index)
            }));
        }

        let content_box = content_geometry.content_box();
        let content_scope = self.enter_table_cell_content_scope(
            cell_style,
            content_box,
            overflow_clip,
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
        stylesheets: &Stylesheets<'_>,
        child_plan: &TableCellChildFragmentPlan,
    ) {
        let child_top = child_plan.painted_child_top;
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
                    box_.core.element,
                    &box_.core.style,
                    &box_.core.children,
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
                    &box_.core.style,
                    &box_.core.children,
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
                if replaced_element_kind(box_.core.element) == Some(ReplacedElementKind::Svg) {
                    self.paint_table_cell_replaced_child_slice(
                        box_.core.element,
                        &box_.core.style,
                        child_top,
                        child_height,
                    );
                } else {
                    self.paint_table_cell_element_child_slice(
                        box_.core.element,
                        &box_.core.style,
                        &box_.core.children,
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
                    box_.core.element,
                    &box_.core.style,
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
            .translated(PaintTranslation::new(x, child_plan.painted_child_top));
        let bounds = PageTopRect::new(x, child_plan.painted_child_top, nested.width, nested.height)
            .paint_clip();
        let slice_clip = PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(x, child_plan.slice_bottom),
            PaintSize::new(nested.width, slice_height),
        ));
        let context = PaintStackingContext::from_banded_fragment(translated, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(PaintEffects {
                overflow_clip_effect: Some(
                    crate::document::paint::contours::OverflowClipEffect::Rect(slice_clip),
                ),
                absolute_clip: Some(slice_clip),
                // This is a replay/fragmentation scope rather than an
                // element with a specified `transform-style`, so it must not
                // flatten an enclosing CSS 3D rendering context.
                three_d_participation:
                    crate::document::paint::effects::ThreeDParticipation::TransparentLayoutBridge,
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
        table_x: f32,
        used_table_width: f32,
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        column_plan: &TableColumnPlan,
        fragment: &TableBodyPaintFragment,
    ) -> (
        Vec<PaintPrimitive>,
        Vec<PaintPrimitive>,
        Vec<RelativeTablePartStructuralPaint>,
    ) {
        let top = fragment.plan.placement.paint_top().points();
        let bottom = fragment.bottom();
        let height = (top - bottom).max(0.0);
        let fragment_has_occupied_row = fragment.plan.body_rows.iter().any(|row| !row.collapsed);
        let vertical_edge_spacing =
            table_vertical_edge_spacing(&[fragment_has_occupied_row], table_metrics);
        let mut backgrounds = Vec::new();
        let mut outlines = Vec::new();
        let mut relative_part_paints = Vec::new();
        // A vertical table fragments across physical X. Its later body
        // fragments retain the opening wrapper's physical Y origin, so the
        // legacy `top - bottom` measure can be zero or negative despite a
        // non-empty logical block slice. Structural paint is governed by the
        // committed source rows in that case, not a physical-Y extent.
        // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        let has_logical_row_slice = fragment
            .plan
            .body_rows
            .iter()
            .any(|row| !row.collapsed && row.row_height > 0.0);
        if !has_logical_row_slice && height <= 0.0 && vertical_edge_spacing <= 0.0 {
            return (backgrounds, outlines, relative_part_paints);
        }

        let fragment_rows = fragment.rows();
        let fragment_row_tops = fragment.row_tops();
        let fragment_row_heights = fragment.row_heights();
        let fragment_row_offsets = fragment.row_offsets();
        let fragment_original_row_heights = fragment.original_row_heights();
        let grid_viewport = fragment.grid_viewport.as_ref();
        let grid_projection = grid_viewport.map(|viewport| viewport.projection());
        let root_background_source_placement =
            grid_viewport.map(|viewport| viewport.root_background_source_placement());
        let wrapper_timeline = grid_viewport.map(|viewport| viewport.wrapper_timeline());
        let grid_fragmentainer_placement =
            grid_viewport.map(|viewport| viewport.fragmentainer_placement());
        let grid_placement = grid_viewport.map(|viewport| viewport.destination_placement());
        let grid_row_bounds = grid_viewport.map(|viewport| viewport.row_bounds());
        let background_top =
            top + vertical_edge_spacing + table_width.padding.top + table_width.border_widths.top;
        // `bottom` is the trailing row edge in the fragment's block
        // coordinate system. The table root extends by its separated-border
        // edge spacing beyond it before applying its own padding and border.
        // <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
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
            table_style.background.background_clip,
        );
        if let (
            Some(projection),
            Some(fragmentainer_placement),
            Some(root_source_placement),
            Some(wrapper_timeline),
        ) = (
            grid_projection,
            grid_fragmentainer_placement,
            root_background_source_placement,
            wrapper_timeline,
        ) {
            // Table-root backgrounds belong to the root paint area, not to
            // the cell grid.  Keep the color in destination space below the
            // structural grid layers; images retain their unfragmented root
            // positioning area and are translated into each fragmentainer.
            // <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
            let root_background_viewport = TableWrapperDecorationViewport::new(
                projection,
                fragmentainer_placement,
                fragment.plan.page_index,
                root_source_placement,
                wrapper_timeline,
                table_style,
                table_width,
                vertical_edge_spacing,
            );
            // An unfragmented root already has its exact wrapper border box in
            // destination space.  In particular, collapsed-border outer
            // maxima are represented by `border_rect`, rather than by the
            // cell grid viewport.  Only project the color through row clips
            // once a root genuinely spans fragmentainers.
            let paints_complete_source_grid = fragment_rows.len() == rows.len()
                && fragment_rows.iter().copied().eq(0..rows.len())
                && fragment_row_offsets
                    .iter()
                    .all(|offset| offset.abs() <= 0.01);
            let paints_unfragmented_root = !fragment.plan.metadata.continues_from_previous_page
                && !fragment.plan.metadata.continues_to_next_page;
            if paints_complete_source_grid || paints_unfragmented_root {
                if let Some(fill) = table_style
                    .background
                    .background_color
                    .visible_color(table_style.color)
                {
                    backgrounds.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                        clip_rect,
                        Some(fill),
                    )));
                }
            } else {
                backgrounds
                    .extend(root_background_viewport.color_primitives(table_style, table_width));
            }
            // Root images are table background layers. They must remain below
            // the wrapper border replay; putting them in `Outline` makes a
            // projected gradient cover the border edges of every continuation
            // fragment.
            // <https://www.w3.org/TR/CSS22/tables.html#table-layers>
            backgrounds.extend(root_background_viewport.image_primitives(
                table_style,
                self.base_url,
                self.root_url,
                self.resource_cache,
            ));
        } else {
            if let Some(fill) = table_style
                .background
                .background_color
                .visible_color(table_style.color)
            {
                backgrounds.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                    clip_rect,
                    Some(fill),
                )));
            }
            backgrounds.extend(background_image_primitives_for_style_with_paint_areas(
                PaintBackgroundArea::from_paint_rect(border_rect),
                PaintBackgroundArea::from_paint_rect(clip_rect),
                table_style,
                self.base_url,
                self.root_url,
                self.resource_cache,
            ));
        }
        let mut column_group_spans = table_column_group_spans(columns, column_plan.column_count());
        if let Some(placement) = grid_placement
            && table_style.writing_mode.has_vertical_lines()
        {
            // Column and column-group backgrounds are disjoint within their
            // respective CSS table painting layers.  Their source order can
            // run in the opposite physical direction in a vertical or
            // sideways root, though.  Use the committed page projection to
            // paint them in a stable visual order, so a later background
            // cannot soften the shared antialiased edge of an earlier one.
            // <https://www.w3.org/TR/css-tables-3/#drawing-backgrounds>
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            column_group_spans.sort_by(
                |(first_start, first_end, _), (second_start, second_end, _)| {
                    compare_table_column_page_paint_order(
                        placement,
                        column_plan,
                        *first_start,
                        *first_end,
                        *second_start,
                        *second_end,
                    )
                },
            );
        }
        let mut synthetic_group_backgrounds = Vec::new();
        for (start_column, end_column, column_group) in column_group_spans {
            let column_group_style =
                self.style_for_table_column_group(&column_group, table_style, stylesheets);
            let mut layer_primitives = Vec::new();
            if let (Some(projection), Some(row_bounds)) = (grid_projection, grid_row_bounds) {
                layer_primitives.extend(table_column_grid_background_primitives(
                    projection,
                    column_plan,
                    grid,
                    &fragment_rows,
                    row_bounds,
                    &fragment_row_heights,
                    &fragment_row_offsets,
                    start_column,
                    end_column,
                    &column_group_style,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                ));
            } else {
                layer_primitives.extend(table_column_fragment_background_primitives(
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
                layer_primitives.extend(table_column_fragment_background_image_primitives(
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
            if table_column_group_has_explicit_columns(
                columns,
                start_column,
                end_column,
                column_plan.column_count(),
            ) {
                backgrounds.extend(layer_primitives);
            } else {
                synthetic_group_backgrounds.push((start_column, layer_primitives));
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
            column_spans.push((column_index, span, column));
            column_index += span;
        }
        if let Some(placement) = grid_placement
            && table_style.writing_mode.has_vertical_lines()
        {
            // See the matching column-group ordering above. These spans are
            // disjoint at the column paint layer, so this changes only the
            // rasterization order of shared fractional edges.
            column_spans.sort_by(
                |(first_start, first_span, _), (second_start, second_span, _)| {
                    compare_table_column_page_paint_order(
                        placement,
                        column_plan,
                        *first_start,
                        first_start.saturating_add(*first_span),
                        *second_start,
                        second_start.saturating_add(*second_span),
                    )
                },
            );
        }
        let mut physical_column_backgrounds = Vec::new();
        for (column_index, span, column) in column_spans {
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            let mut layer_primitives = Vec::new();
            if let (Some(projection), Some(row_bounds)) = (grid_projection, grid_row_bounds) {
                layer_primitives.extend(table_column_grid_background_primitives(
                    projection,
                    column_plan,
                    grid,
                    &fragment_rows,
                    row_bounds,
                    &fragment_row_heights,
                    &fragment_row_offsets,
                    column_index,
                    column_index + span,
                    &column_style,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                ));
            } else {
                layer_primitives.extend(table_column_fragment_background_primitives(
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
                layer_primitives.extend(table_column_fragment_background_image_primitives(
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
            physical_column_backgrounds.push((column_index, layer_primitives));
        }
        physical_column_backgrounds.extend(synthetic_group_backgrounds);
        physical_column_backgrounds.sort_by_key(|(start_column, _)| *start_column);
        if table_columns_paint_in_reverse_page_order(table_style) {
            physical_column_backgrounds.reverse();
        }
        for (_, layer_primitives) in physical_column_backgrounds {
            backgrounds.extend(layer_primitives);
        }

        let occupied_inline_bounds = column_plan.occupied_inline_bounds().unwrap_or_else(|| {
            TableInlineBounds::new(
                TableGridLength::new(0.0),
                TableGridLength::new(used_table_width),
            )
        });
        let occupied_x = table_x + occupied_inline_bounds.logical_start().get();
        let occupied_width = occupied_inline_bounds.logical_size().get();
        // Relative offsets of table rows and row groups resolve against the
        // table grid's physical containing block. The grid remains in normal
        // table coordinates; only the generated structural paint below is
        // translated.
        // <https://drafts.csswg.org/css-position-3/#relative-positioning>
        let table_grid_containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
            table_x,
            top,
            used_table_width,
            height,
        ));

        for (start_row, end_row, row_group) in table_row_group_spans(rows) {
            let row_group_style =
                self.style_for_table_row_group(&row_group, table_style, stylesheets);
            let background_start = backgrounds.len();
            let outline_start = outlines.len();
            if let (Some(projection), Some(row_bounds)) = (grid_projection, grid_row_bounds) {
                backgrounds.extend(table_row_group_grid_background_primitives(
                    projection,
                    row_bounds,
                    column_plan,
                    grid,
                    &fragment_rows,
                    &fragment_row_heights,
                    &fragment_row_offsets,
                    start_row,
                    end_row,
                    &row_group_style,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                ));
            } else if let Some(fill) = row_group_style
                .background
                .background_color
                .visible_color(row_group_style.color)
            {
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
                            PageInlineSpan::new(occupied_x, occupied_width),
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
                        PageInlineSpan::new(occupied_x, occupied_width),
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
            let offset = relative_position_offset(&row_group_style, table_grid_containing_block);
            let translation = PaintTranslation::new(offset.x(), offset.y());
            for primitive in &mut backgrounds[background_start..] {
                *primitive = primitive.clone().translated(translation);
            }
            for primitive in &mut outlines[outline_start..] {
                *primitive = primitive.clone().translated(translation);
            }
            if matches!(
                row_group_style.position,
                Position::Relative | Position::Sticky
            ) {
                let mut primitives = backgrounds.split_off(background_start);
                primitives.extend(outlines.split_off(outline_start));
                relative_part_paints.push(RelativeTablePartStructuralPaint {
                    source_style: row_group_style.source().clone(),
                    style: row_group_style.used_style().clone(),
                    bounds: PageTopRect::new(occupied_x, top, occupied_width, height).paint_clip(),
                    primitives,
                });
            }
        }

        for (local_row, original_row) in fragment_rows.iter().cloned().enumerate() {
            let row_style = self.style_for_table_row(&rows[original_row], table_style, stylesheets);
            if let Some(bounds) = table_fragment_row_span_bounds(
                PageInlineSpan::new(occupied_x, occupied_width),
                &fragment_row_tops,
                &fragment_row_heights,
                local_row,
                local_row + 1,
            ) {
                let mut row_backgrounds = if let (Some(projection), Some(row_bounds)) =
                    (grid_projection, grid_row_bounds)
                {
                    table_row_grid_background_primitives(
                        projection,
                        row_bounds,
                        column_plan,
                        grid,
                        &fragment_rows,
                        &fragment_row_heights,
                        &fragment_row_offsets,
                        original_row,
                        &row_style,
                        self.base_url,
                        self.root_url,
                        self.resource_cache,
                    )
                } else {
                    table_row_fragment_background_primitives(
                        table_x,
                        bounds.paint_rect(),
                        column_plan,
                        grid,
                        &fragment_rows,
                        &fragment_row_tops,
                        &fragment_row_heights,
                        &fragment_row_offsets,
                        &fragment_original_row_heights,
                        original_row,
                        &row_style,
                        self.base_url,
                        self.root_url,
                        self.resource_cache,
                    )
                };
                let row_offset = relative_position_offset(&row_style, table_grid_containing_block);
                let row_group_offset = rows[original_row]
                    .row_groups
                    .last()
                    .map(|group| self.style_for_table_row_group(group, table_style, stylesheets))
                    .map(|style| relative_position_offset(&style, table_grid_containing_block))
                    .unwrap_or_else(RelativeOffset::zero);
                let translation = PaintTranslation::new(
                    row_offset.x() + row_group_offset.x(),
                    row_offset.y() + row_group_offset.y(),
                );
                for primitive in &mut row_backgrounds {
                    *primitive = primitive.clone().translated(translation);
                }
                let relative_style =
                    matches!(row_style.position, Position::Relative | Position::Sticky)
                        .then(|| row_style.clone())
                        .or_else(|| {
                            rows[original_row]
                                .row_groups
                                .last()
                                .map(|group| {
                                    self.style_for_table_row_group(group, table_style, stylesheets)
                                })
                                .filter(|style| {
                                    matches!(style.position, Position::Relative | Position::Sticky)
                                })
                        });
                if let Some(relative_style) = relative_style {
                    relative_part_paints.push(RelativeTablePartStructuralPaint {
                        source_style: relative_style.source().clone(),
                        style: relative_style.used_style().clone(),
                        bounds: PaintClip::from_paint_rect(bounds.paint_rect()),
                        primitives: row_backgrounds,
                    });
                } else {
                    backgrounds.extend(row_backgrounds);
                }
            }
        }
        (backgrounds, outlines, relative_part_paints)
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
            PageInlineSpan::new(table_x, used_table_width),
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
        column_plan: &TableColumnPlan,
        fragment: &TableBodyPaintFragment,
    ) -> Vec<PaintPrimitive> {
        let rows = fragment.rows();
        let row_heights = fragment.row_heights();
        let row_offsets = fragment.row_offsets();
        let original_row_heights = fragment.original_row_heights();
        // A collapsed border is table-grid structural paint.  Its destination
        // is consequently the same committed grid viewport that owns row,
        // column, and root-background paint; reconstructing an origin from a
        // physical row cursor drops caption-consumed wrapper progress and is
        // invalid for vertical roots.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        // <https://drafts.csswg.org/css-tables-3/#table-fragmentation>
        let viewport = fragment
            .grid_viewport
            .as_ref()
            .expect("every committed table body fragment has a grid viewport");
        let placement = viewport.destination_placement();
        let (rects, paths) = geometry.grid.paint_fragment_rows(
            placement,
            placement,
            column_plan,
            &rows,
            &fragment.row_tops(),
            &row_heights,
            &row_offsets,
            &original_row_heights,
            Some(viewport.row_bounds()),
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
                let cell_block_contribution = table_cell_root_block_track_contribution(
                    self,
                    cell,
                    &prepared.style,
                    context.table_style,
                    context.stylesheets,
                    Some(prepared.borders),
                    prepared.metrics.border_box_height,
                );
                row_height = row_height.max(cell_block_contribution);
            }
            // This baseline accumulator stores a physical-Y extent.  It can
            // enlarge a row only when the table root's block tracks also run
            // on physical Y; for vertical roots, those tracks run across the
            // page and a cell baseline is an alignment detail rather than a
            // row-width constraint.
            // <https://drafts.csswg.org/css-tables-3/#row-layout>
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            if !TableCellAxisAdapter::for_table(context.table_style)
                .root_track_uses_physical_width(TableRootTrackAxis::Block)
                && table_cell_participates_in_physical_y_row_baseline(
                    &prepared.style,
                    row_style,
                    placement,
                )
                && let Some(baseline) = self.table_cell_physical_y_row_baseline_candidate(
                    cell,
                    &prepared,
                    context.stylesheets,
                )
            {
                has_baseline_cells = true;
                max_baseline = max_baseline.max(baseline.points());
                max_after_baseline = max_after_baseline
                    .max((prepared.metrics.border_box_height - baseline.points()).max(0.0));
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
        let target = self
            .resolve_table_target_content_height(
                context.table_style,
                context.collapsed_geometry,
                context.wrapper_border_box_block_size,
                context.positioned_table_block_content_size,
                context.wrapper_non_grid_block_size,
            )
            .map(TableHeightDistributionTarget::Definite)
            .unwrap_or(TableHeightDistributionTarget::Intrinsic);
        let cache_key = context.rows.first().and_then(|row| {
            row.element.map(|element| TableHeightPlanCacheKey {
                table_element: element.id,
                column_width_bits: context.column_plan.total_width().points().to_bits(),
                wrapper_border_box_block_size_bits: context
                    .wrapper_border_box_block_size
                    .map(|size| size.points().to_bits()),
                positioned_table_block_content_size_bits: context
                    .positioned_table_block_content_size
                    .map(|size| size.points().to_bits()),
                wrapper_non_grid_block_size_bits: context
                    .wrapper_non_grid_block_size
                    .points()
                    .to_bits(),
                target: target.into(),
            })
        });
        if let Some(plan) = cache_key
            .and_then(|key| self.speculative_table_height_plans.get(&key))
            .cloned()
        {
            return plan;
        }
        // CSS Tables 3 row layout first computes minimum row sizes, applies
        // spanning-cell minimum constraints, then distributes any definite
        // table height against reference sizes:
        // <https://drafts.csswg.org/css-tables-3/#row-layout> and
        // <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>.
        let mut plan_rows = Vec::with_capacity(context.rows.len());
        let mut spanning_cells = Vec::new();
        for (row_index, row) in context.rows.iter().enumerate() {
            let row_style = self.style_for_table_row(row, context.table_style, context.stylesheets);
            // A row is auto-height only when neither the row nor a
            // single-row cell supplies a specified block-size constraint.
            // Cell constraints establish the row's reference size during
            // the second pass, and surplus table height belongs only to rows
            // that are genuinely content-sized.
            // <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>
            let has_specified_single_row_cell_height = context.grid.rows[row_index]
                .iter()
                .filter(|placement| placement.rowspan == 1)
                .any(|placement| {
                    let cell = &row.cells[placement.cell];
                    let cell_style =
                        self.style_for_table_cell(cell, row, &row_style, context.stylesheets);
                    !table_root_block_size(&cell_style).is_auto()
                });
            let row_group_has_specified_height = row.row_groups.last().is_some_and(|group| {
                !table_root_block_size(&self.style_for_table_row_group(
                    group,
                    context.table_style,
                    context.stylesheets,
                ))
                .is_auto()
            });
            let auto_height = table_root_block_size(&row_style).is_auto()
                && !has_specified_single_row_cell_height
                // A row group with an authored block-size supplies a
                // reference constraint for its rows. It is therefore not an
                // auto-height receiver when excess table height is assigned.
                && !row_group_has_specified_height;
            let source_height = if row_style.position.is_running() {
                0.0
            } else {
                self.measure_table_row_height(context, row_index, &row_style)
            };
            if table_row_is_collapsed(&row_style) || row_style.position.is_running() {
                plan_rows.push(TableRowHeightPlan {
                    base: 0.0,
                    source_height,
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
                    source_height: 0.0,
                    reference: 0.0,
                    final_height: 0.0,
                    auto: false,
                    collapsed: true,
                });
                continue;
            }
            let base = source_height;
            plan_rows.push(TableRowHeightPlan {
                base,
                source_height,
                reference: base,
                final_height: base,
                auto: auto_height,
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
                    let required_block_size = table_cell_root_block_track_contribution(
                        self,
                        cell,
                        &prepared.style,
                        context.table_style,
                        context.stylesheets,
                        Some(prepared.borders),
                        prepared.metrics.border_box_height,
                    );
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

        // With no resolved wrapper block-size target, the first row-layout
        // pass already contains every cell and row constraint that can affect
        // the table's intrinsic height.  A second pass against an indefinite
        // percentage basis would reproduce those same values.  Row-group
        // heights are the exception because they are group-level constraints
        // rather than cell contributions, so retain the reference pass when
        // any group has an authored block size.
        // <https://drafts.csswg.org/css-tables-3/#row-layout>
        let needs_reference_pass = matches!(target, TableHeightDistributionTarget::Definite(_))
            || context.rows.iter().any(|row| {
                row.row_groups.last().is_some_and(|group| {
                    !table_root_block_size(&self.style_for_table_row_group(
                        group,
                        context.table_style,
                        context.stylesheets,
                    ))
                    .is_auto()
                })
            });
        if needs_reference_pass {
            self.compute_table_reference_heights(
                &mut plan_rows,
                context,
                target
                    .definite_content_height()
                    .map(|value| {
                        PercentageBasis::definite_from(value, BlockSizeBasisSource::TableWrapper)
                    })
                    .unwrap_or_else(PercentageBasis::indefinite),
            );
        }
        self.distribute_table_height_plan(&mut plan_rows, target, context.table_metrics.clone());
        let plan = TableHeightPlan {
            rows: plan_rows,
            target,
        };
        if let Some(key) = cache_key {
            self.speculative_table_height_plans
                .insert(key, plan.clone());
        }
        plan
    }
}

/// Compare two column-layer spans in the stable order of their committed page
/// geometry.
///
/// The table grid owns source logical order, while the PDF surface owns the
/// final physical edge coverage.  Painting from physical top-to-bottom and
/// then left-to-right makes equivalent writing-mode projections share the
/// same antialiased edge ownership:
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
fn compare_table_column_page_paint_order(
    placement: TableGridPlacement,
    column_plan: &TableColumnPlan,
    first_start: usize,
    first_end: usize,
    second_start: usize,
    second_end: usize,
) -> std::cmp::Ordering {
    let first = table_column_page_paint_rect(placement, column_plan, first_start, first_end);
    let second = table_column_page_paint_rect(placement, column_plan, second_start, second_end);
    second
        .top_y()
        .total_cmp(&first.top_y())
        .then_with(|| first.x().total_cmp(&second.x()))
}

fn table_column_page_paint_rect(
    placement: TableGridPlacement,
    column_plan: &TableColumnPlan,
    start_column: usize,
    end_column: usize,
) -> PageTopRect {
    let inline_bounds = column_plan.inline_bounds_for_span(
        start_column,
        end_column
            .min(column_plan.column_count())
            .saturating_sub(start_column),
    );
    placement.page_top_rect_for(TableGridRect::new(
        TableGridPoint::from_lengths(inline_bounds.start, TableGridLength::new(0.0)),
        TableGridSize::from_lengths(inline_bounds.size, placement.logical_block_grid_extent()),
    ))
}
