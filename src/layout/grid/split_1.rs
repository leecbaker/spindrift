use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Lay out a CSS grid container.
    ///
    /// CSS Grid Layout defines grid containers as independent formatting
    /// contexts whose children are grid items placed into explicit and
    /// implicit tracks:
    /// <https://www.w3.org/TR/css-grid-1/#grid-containers>.
    ///
    /// This entrypoint is intentionally separate from block layout so the
    /// Taffy-backed grid algorithm can replace the temporary block-flow
    /// fallback without changing element dispatch or box-tree construction.
    pub(in crate::layout) fn layout_grid(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            self.layout_positioned_block_with_static_source(
                element,
                style,
                stylesheets,
                child_boxes,
                None,
            );
            return;
        }

        self.apply_forced_break(style.break_before);
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, containing_inline_size);
        let relative_offset =
            relative_position_offset(&used_style, self.current_containing_block());
        if matches!(used_style.position, Position::Relative | Position::Sticky) {
            self.cursor_y += relative_offset.y;
        }

        let available_outer_width =
            normal_flow_block_available_outer_width(&used_style, containing_inline_size);
        let border_widths = box_metrics.border;
        let horizontal_extras = non_content_pt(box_metrics.horizontal_non_content());
        let vertical_extras = box_metrics.vertical_non_content();
        let requested_content_width = self.used_block_content_width(
            element,
            &used_style,
            stylesheets,
            child_boxes,
            BlockContentWidthInputs {
                available_outer_width,
                percentage_basis: containing_inline_size,
                horizontal_non_content: horizontal_extras,
            },
        );
        let resolve_auto_margins = used_style.float == Float::None;
        let width = resolve_normal_flow_block_width(
            &mut used_style,
            self.content_left,
            self.content_right,
            requested_content_width,
            horizontal_extras,
            self.containing_block_direction,
            resolve_auto_margins,
        );
        let content_width = width.content_width.points();
        let outer_width = width.border_box_width.points();
        let style = &used_style;
        let mut outer_x = width.border_box_x + relative_offset.x;
        let mut inner_x = outer_x + border_widths.left + style.padding.left;
        let inner_width = content_width.max(0.0);
        let available_outer_height =
            (self.cursor_y - self.page_bottom() - style.margin.top - style.margin.bottom).max(0.0);
        let definite_content_height =
            used_content_height_or_auto(style, available_outer_height, vertical_extras)
                .map(|height| constrain_height(style, height, available_outer_height));

        let built_child_boxes;
        let child_boxes = if let Some(child_boxes) = child_boxes {
            child_boxes
        } else {
            built_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            &built_child_boxes
        };
        let (mut children, mut positioned_children) = grid_child_lists_from_boxes(child_boxes);
        self.resolve_grid_children_viewport_lengths(&mut children);
        self.resolve_grid_children_viewport_lengths(&mut positioned_children);

        self.cursor_y -= style.margin.top;
        if style.float == Float::None {
            let margin_box_width = style.margin.left + outer_width + style.margin.right;
            let collision_height = definite_content_height.unwrap_or(style.line_height)
                + vertical_extras
                + style.margin.top
                + style.margin.bottom;
            let (margin_box_left, avoided_top, _) = self.place_float_avoiding_margin_box(
                self.cursor_y,
                margin_box_width,
                collision_height,
                style.clear,
                style.writing_mode,
                style.direction,
                self.containing_block_direction,
            );
            self.cursor_y = avoided_top;
            outer_x = margin_box_left + style.margin.left + relative_offset.x;
            inner_x = outer_x + border_widths.left + style.padding.left;
        } else {
            self.cursor_y = self.clear_active_floats_top(
                style.clear,
                style.writing_mode,
                style.direction,
                self.cursor_y,
            );
        }

        let block_top = self.cursor_y;
        let paint_page_index = self.pages.len();
        let paint_checkpoint = self.current_page.paint_checkpoint();
        self.cursor_y -= border_widths.top + style.padding.top;
        let content_top = self.cursor_y;
        let Some(grid_layout) = self.compute_grid_layout(
            style,
            &children,
            stylesheets,
            inner_width,
            definite_content_height,
        ) else {
            let mut flow_style = style.clone();
            flow_style.display = Display::BLOCK;
            flow_style.margin = css::Edges::ZERO;
            self.layout_block(element, &flow_style, stylesheets, &[], Some(child_boxes));
            return;
        };
        let total_content_height =
            constrain_height(style, grid_layout.height, available_outer_height);
        let total_height = border_widths.top
            + style.padding.top
            + total_content_height
            + style.padding.bottom
            + border_widths.bottom;
        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                    outer_x + border_widths.left,
                    block_top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                )));
        }

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        self.push_float_context();
        for (child, item) in children.iter().zip(&grid_layout.items) {
            self.replay_grid_item(child, item, stylesheets, inner_x, content_top);
        }
        self.pop_float_context();
        for child in &positioned_children {
            self.layout_positioned_grid_child(
                child,
                &children,
                PositionedGridStaticContext {
                    container_style: style,
                    stylesheets,
                    inner_x,
                    inner_width,
                    content_top,
                    definite_content_height,
                },
            );
        }
        self.content_left = previous_left;
        self.content_right = previous_right;

        self.cursor_y = content_top - total_content_height;
        self.cursor_y -= style.padding.bottom + border_widths.bottom;
        let block_bottom = self.cursor_y;
        let block_height = (block_top - block_bottom).max(total_height);
        if block_height > 0.0 {
            self.mark_current_page_flow_content();
        }
        let grid_gap_items = grid_layout
            .items
            .iter()
            .map(GridItemLayout::gap_decoration_item)
            .collect::<Vec<_>>();
        let mut own_background_primitives = Vec::new();
        let mut own_outline_primitives = Vec::new();
        if style.visibility == Visibility::Visible && block_height > 0.0 {
            own_background_primitives = self.box_background_primitives(
                outer_x,
                block_bottom,
                outer_width,
                block_height,
                style,
            );
            own_outline_primitives = self.box_outline_primitives(
                outer_x,
                block_bottom,
                outer_width,
                block_height,
                style,
            );
        }
        let has_own_background_primitives = !own_background_primitives.is_empty();
        let has_own_outline_primitives = !own_outline_primitives.is_empty();
        let fragments = self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        let grid_spanned_pages = self.pages.len() != paint_page_index;
        for (page_index, mut fragment) in fragments {
            if grid_spanned_pages {
                if let Some(fragment_bounds) = fragment.bounds().map(|bounds| {
                    PaintClip::from_paint_rect(paint_space_rect(
                        outer_x,
                        bounds.y(),
                        outer_width,
                        bounds.height(),
                    ))
                }) {
                    if style.visibility == Visibility::Visible
                        && (style.background_color.is_some()
                            || style.background_image.is_some()
                            || style.border_image.source.is_some()
                            || used_border_width(style) > 0.0)
                    {
                        let page_background_primitives = self.box_background_primitives(
                            outer_x,
                            fragment_bounds.y(),
                            outer_width,
                            fragment_bounds.height(),
                            style,
                        );
                        fragment.prepend_primitives_in_band(
                            PaintBand::BackgroundBorder,
                            page_background_primitives,
                        );
                    }
                    if style.visibility == Visibility::Visible {
                        fragment.append_primitives_in_band(
                            PaintBand::BackgroundBorder,
                            grid_gap_decoration_primitives_for_page(GridGapFragmentProjection {
                                style,
                                inner_x,
                                content_top,
                                inner_width,
                                total_content_height,
                                items: &grid_gap_items,
                                gutters: &grid_layout.gap_gutters,
                                fragment_bounds,
                            }),
                        );
                    }
                    if style.visibility == Visibility::Visible {
                        let page_outline_primitives = self.box_outline_primitives(
                            outer_x,
                            fragment_bounds.y(),
                            outer_width,
                            fragment_bounds.height(),
                            style,
                        );
                        fragment
                            .append_primitives_in_band(PaintBand::Outline, page_outline_primitives);
                    }
                }
            } else if page_index == paint_page_index {
                fragment.prepend_primitives_in_band(
                    PaintBand::BackgroundBorder,
                    own_background_primitives.clone(),
                );
                if style.visibility == Visibility::Visible {
                    fragment.append_primitives_in_band(
                        PaintBand::BackgroundBorder,
                        grid_gap_decoration_primitives(
                            style,
                            inner_x,
                            content_top,
                            inner_width,
                            total_content_height,
                            &grid_gap_items,
                            &grid_layout.gap_gutters,
                        ),
                    );
                }
                fragment
                    .append_primitives_in_band(PaintBand::Outline, own_outline_primitives.clone());
            }
            if !fragment.is_empty() {
                let fragment = if has_own_background_primitives
                    || has_own_outline_primitives
                    || grid_spanned_pages
                {
                    let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
                        .with_source_order(self.next_paint_source_order());
                    PaintFragment::from_stacking_context_in_band(PaintBand::InFlowBlock, context)
                } else {
                    fragment
                };
                if page_index < self.pages.len() {
                    self.pages[page_index]
                        .append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
                } else {
                    self.current_page
                        .append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
                }
            }
        }
        self.cursor_y -= style.margin.bottom;
        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
        }
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y;
        }
        self.apply_forced_break(style.break_after);
    }

    /// Estimate an atomic `inline-grid` box for intrinsic inline measurement.
    ///
    /// CSS Display makes `inline-grid` an inline-level atomic grid container,
    /// while CSS Grid defines its track sizing and baseline contribution:
    /// <https://www.w3.org/TR/css-display-3/#the-display-properties>,
    /// <https://www.w3.org/TR/css-grid-1/#grid-containers>, and
    /// <https://www.w3.org/TR/css-grid-1/#grid-baselines>.
    pub(in crate::layout) fn intrinsic_inline_grid_atom_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        child_boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> InlineAtom {
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let horizontal_extras = box_metrics.horizontal_non_content();
        let vertical_extras = box_metrics.vertical_non_content();
        let (mut children, _) = grid_child_lists_from_boxes(child_boxes);
        self.resolve_grid_children_viewport_lengths(&mut children);

        let (min_width, max_width) = self.estimate_grid_intrinsic_widths(
            element,
            style,
            stylesheets,
            available_width,
            Some(child_boxes),
        );
        let requested_content_width = crate::layout::intrinsic::content_width_from_intrinsic(
            style,
            available_width,
            horizontal_extras,
            min_width,
            max_width,
            crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
        );
        let content_width =
            constrain_width(style, requested_content_width, available_width).max(0.0);
        let definite_content_height =
            used_content_height_or_auto(style, style.line_height.max(1.0), vertical_extras)
                .map(|height| constrain_height(style, height, available_width));
        let content_height = self
            .compute_grid_layout(
                style,
                &children,
                stylesheets,
                content_width,
                definite_content_height,
            )
            .map(|layout| layout.height)
            .unwrap_or(style.line_height)
            .max(style.line_height);
        let content_height = constrain_height(style, content_height, available_width);
        let border_box_height = content_height + vertical_extras;

        InlineAtom::new(
            InlineAtomContent::Svg {
                fill: Color::TRANSPARENT,
            },
            style.clone(),
            None,
            content_width + horizontal_extras + style.margin.left + style.margin.right,
            border_box_height + style.margin.top + style.margin.bottom,
            border_box_height,
            baseline_shift,
            link_target,
            None,
        )
    }

    /// Build an atomic inline fragment for an `inline-grid` container.
    ///
    /// CSS Display makes `inline-grid` participate in inline layout as an
    /// atomic inline, and CSS Grid then lays out its contents as a grid
    /// formatting context:
    /// <https://www.w3.org/TR/css-display-3/#the-display-properties> and
    /// <https://www.w3.org/TR/css-grid-1/#grid-containers>.
    pub(in crate::layout) fn inline_grid_atom_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        child_boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> InlineAtom {
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let border_widths = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content();
        let vertical_extras = box_metrics.vertical_non_content();
        let (mut children, mut positioned_children) = grid_child_lists_from_boxes(child_boxes);
        self.resolve_grid_children_viewport_lengths(&mut children);
        self.resolve_grid_children_viewport_lengths(&mut positioned_children);

        let (min_width, max_width) = self.estimate_grid_intrinsic_widths(
            element,
            style,
            stylesheets,
            available_width,
            Some(child_boxes),
        );
        let requested_content_width = crate::layout::intrinsic::content_width_from_intrinsic(
            style,
            available_width,
            horizontal_extras,
            min_width,
            max_width,
            crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
        );
        let content_width =
            constrain_width(style, requested_content_width, available_width).max(0.0);
        let definite_content_height =
            used_content_height_or_auto(style, style.line_height.max(1.0), vertical_extras)
                .map(|height| constrain_height(style, height, available_width));
        let Some(grid_layout) = self.compute_grid_layout(
            style,
            &children,
            stylesheets,
            content_width,
            definite_content_height,
        ) else {
            return self.inline_fragment_atom_for_children(
                style,
                child_boxes,
                stylesheets,
                baseline_shift,
                link_target,
            );
        };

        let total_content_height = constrain_height(style, grid_layout.height, available_width);
        let border_box_height = total_content_height + vertical_extras;
        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let top = 10_000.0;
        let content_top = top - border_widths.top - style.padding.top;
        let inner_x = border_widths.left + style.padding.left;
        let inner_width = content_width.max(0.0);
        self.current_page = Page::new(content_width + horizontal_extras, top);
        self.content_left = inner_x;
        self.content_right = inner_x + inner_width;
        self.cursor_y = content_top;
        self.truncate_page_start_margins = false;

        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                    border_widths.left,
                    top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                )));
        }

        self.push_page_name_scope_suppression();
        self.push_float_context();
        for (child, item) in children.iter().zip(&grid_layout.items) {
            self.replay_grid_item(child, item, stylesheets, inner_x, content_top);
        }
        self.pop_float_context();

        for child in &positioned_children {
            self.layout_positioned_grid_child(
                child,
                &children,
                PositionedGridStaticContext {
                    container_style: style,
                    stylesheets,
                    inner_x,
                    inner_width,
                    content_top,
                    definite_content_height,
                },
            );
        }
        self.pop_page_name_scope_suppression();

        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
        }
        let border_bottom = top - border_box_height;
        self.flush_positioned_layers_since(positioned_layer_start);
        let mut fragment = self.current_page.paint_fragment();
        if style.visibility == Visibility::Visible {
            fragment.prepend_primitives_in_band(
                PaintBand::BackgroundBorder,
                grid_gap_decoration_primitives(
                    style,
                    inner_x,
                    content_top,
                    inner_width,
                    total_content_height,
                    &grid_layout
                        .items
                        .iter()
                        .map(GridItemLayout::gap_decoration_item)
                        .collect::<Vec<_>>(),
                    &grid_layout.gap_gutters,
                ),
            );
        }
        let fragment = fragment.translated(PaintVector::new(0.0, -border_bottom));
        let baseline_offset = fragment
            .first_line_y()
            .map(|line_y| (border_box_height - line_y).max(0.0))
            .unwrap_or(border_box_height);
        self.restore(snapshot);

        InlineAtom::new(
            InlineAtomContent::InlineFragment(fragment),
            style.clone(),
            None,
            content_width + horizontal_extras + style.margin.left + style.margin.right,
            border_box_height + style.margin.top + style.margin.bottom,
            baseline_offset,
            baseline_shift,
            link_target,
            None,
        )
    }

    pub(in crate::layout::grid) fn resolve_grid_children_viewport_lengths(
        &mut self,
        children: &mut [GridChild<'_>],
    ) {
        for child in children {
            self.resolve_style_current_viewport_lengths(&mut child.style);
        }
    }
}

pub(in crate::layout) struct GridGapFragmentProjection<'a> {
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) inner_x: f32,
    pub(in crate::layout) content_top: f32,
    pub(in crate::layout) inner_width: f32,
    pub(in crate::layout) total_content_height: f32,
    pub(in crate::layout) items: &'a [GapDecorationItem],
    pub(in crate::layout) gutters: &'a GapDecorationGridGutters,
    pub(in crate::layout) fragment_bounds: PaintClip,
}

pub(in crate::layout) fn grid_gap_decoration_primitives_for_page(
    projection: GridGapFragmentProjection<'_>,
) -> Vec<PaintPrimitive> {
    let fragment_top = projection.fragment_bounds.y() + projection.fragment_bounds.height();
    let block_start =
        (projection.content_top - fragment_top).clamp(0.0, projection.total_content_height);
    let block_end = (projection.content_top - projection.fragment_bounds.y())
        .clamp(0.0, projection.total_content_height);
    let fragment_height = (block_end - block_start).max(0.0);
    if fragment_height <= 0.01 {
        return Vec::new();
    }

    let page_gutters = GapDecorationGridGutters {
        columns: projection.gutters.columns.clone(),
        rows: projection
            .gutters
            .rows
            .iter()
            .filter_map(|gutter| gutter.clipped_to_block_range(block_start, block_end))
            .collect(),
    };
    let page_items = projection
        .items
        .iter()
        .filter_map(|item| item.clipped_to_block_range(block_start, block_end))
        .collect::<Vec<_>>();

    grid_gap_decoration_primitives(
        projection.style,
        projection.inner_x,
        fragment_top,
        projection.inner_width,
        fragment_height,
        &page_items,
        &page_gutters,
    )
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct GridLayout {
    pub(in crate::layout) height: f32,
    pub(in crate::layout) items: Vec<GridItemLayout>,
    pub(in crate::layout) gap_gutters: GapDecorationGridGutters,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct GridItemLayout {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
    pub(in crate::layout) area: Option<GridItemArea>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GridItemArea {
    pub(in crate::layout) row_start: u16,
    pub(in crate::layout) row_end: u16,
    pub(in crate::layout) column_start: u16,
    pub(in crate::layout) column_end: u16,
}

impl GridItemLayout {
    pub(in crate::layout) fn gap_decoration_item(&self) -> GapDecorationItem {
        if let Some(area) = self.area {
            GapDecorationItem::with_grid_area(
                self.x,
                self.y,
                self.width,
                self.height,
                GapDecorationGridArea {
                    row_start: area.row_start,
                    row_end: area.row_end,
                    column_start: area.column_start,
                    column_end: area.column_end,
                },
            )
        } else {
            GapDecorationItem::new(self.x, self.y, self.width, self.height)
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Compute same-page grid item geometry with Quire-measured leaf estimates.
    ///
    /// CSS Grid track sizing consumes each item's min-content, max-content, and
    /// preferred size contributions. Taffy owns the Grid Level 1 placement and
    /// track-sizing algorithm here, while Quire supplies leaf measurements from
    /// the same inline, block, flex, table, and replaced-element paths used by
    /// other layout modes:
    /// <https://www.w3.org/TR/css-grid-1/#algo-overview> and
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic>.
    pub(in crate::layout::grid) fn compute_grid_layout(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &[Stylesheet],
        width: f32,
        height: Option<f32>,
    ) -> Option<GridLayout> {
        let mut tree: taffy_layout::TaffyTree<GridItemEstimate> = taffy_layout::TaffyTree::new();
        tree.disable_rounding();
        let mut nodes = Vec::with_capacity(children.len());
        for child in children {
            let estimate = self.estimate_grid_item_size(child, stylesheets, width, height);
            let node = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        display: taffy_layout::Display::Block,
                        box_sizing: taffy_box_sizing(child.style.box_sizing),
                        direction: taffy_direction(child.style.direction),
                        size: taffy_layout::Size {
                            width: taffy_grid_item_dimension(
                                child.style.box_values.width,
                                Some(width),
                                estimate.min_width,
                                estimate.content_width,
                            ),
                            height: taffy_grid_item_dimension(
                                child.style.box_values.height,
                                height,
                                estimate.min_height,
                                estimate.content_height,
                            ),
                        },
                        min_size: taffy_layout::Size {
                            width: taffy_grid_item_min_dimension(
                                child.style.box_values.min_width,
                                Some(width),
                                estimate.min_width,
                                estimate.content_width,
                            ),
                            height: taffy_grid_item_min_dimension(
                                child.style.box_values.min_height,
                                height,
                                estimate.min_height,
                                estimate.content_height,
                            ),
                        },
                        max_size: taffy_layout::Size {
                            width: taffy_grid_item_dimension(
                                child.style.box_values.max_width,
                                Some(width),
                                estimate.min_width,
                                estimate.content_width,
                            ),
                            height: taffy_grid_item_dimension(
                                child.style.box_values.max_height,
                                height,
                                estimate.min_height,
                                estimate.content_height,
                            ),
                        },
                        margin: taffy_margin(&child.style),
                        padding: taffy_padding(&child.style),
                        border: taffy_edges(used_border_widths(&child.style)),
                        align_self: taffy_effective_grid_align_self(&child.style, style),
                        justify_self: taffy_effective_grid_justify_self(&child.style, style),
                        grid_row: taffy_grid_line(
                            &child.style.grid_row_start,
                            &child.style.grid_row_end,
                        ),
                        grid_column: taffy_grid_line(
                            &child.style.grid_column_start,
                            &child.style.grid_column_end,
                        ),
                        ..Default::default()
                    },
                    estimate,
                )
                .ok()?;
            nodes.push(node);
        }
        let root = tree
            .new_with_children(
                taffy_layout::Style {
                    display: taffy_layout::Display::Grid,
                    box_sizing: taffy_layout::BoxSizing::BorderBox,
                    direction: taffy_direction(style.direction),
                    size: taffy_layout::Size {
                        width: taffy_layout::Dimension::length(width),
                        height: height
                            .map(taffy_layout::Dimension::length)
                            .unwrap_or_else(taffy_layout::Dimension::auto),
                    },
                    min_size: taffy_layout::Size {
                        width: taffy_dimension(style.box_values.min_width),
                        height: taffy_dimension(style.box_values.min_height),
                    },
                    max_size: taffy_layout::Size {
                        width: taffy_dimension(style.box_values.max_width),
                        height: taffy_dimension(style.box_values.max_height),
                    },
                    grid_template_columns: taffy_grid_template_tracks(&style.grid_template_columns),
                    grid_template_rows: taffy_grid_template_tracks(&style.grid_template_rows),
                    grid_template_areas: taffy_grid_template_areas(&style.grid_template_areas),
                    grid_template_column_names: taffy_grid_template_line_names(
                        &style.grid_template_columns,
                        &style.grid_template_areas,
                        GridAxis::Column,
                    ),
                    grid_template_row_names: taffy_grid_template_line_names(
                        &style.grid_template_rows,
                        &style.grid_template_areas,
                        GridAxis::Row,
                    ),
                    grid_auto_columns: taffy_grid_auto_tracks(&style.grid_auto_columns),
                    grid_auto_rows: taffy_grid_auto_tracks(&style.grid_auto_rows),
                    grid_auto_flow: taffy_grid_auto_flow(style.grid_auto_flow),
                    justify_content: Some(taffy_grid_justify_content(style.justify_content)),
                    align_content: Some(taffy_grid_align_content(style.align_content)),
                    justify_items: Some(taffy_grid_justify_items(style.justify_items)),
                    align_items: Some(taffy_grid_align_items(style.align_items)),
                    gap: taffy_layout::Size {
                        width: taffy_gap(style.column_gap),
                        height: taffy_gap(style.row_gap),
                    },
                    ..Default::default()
                },
                &nodes,
            )
            .ok()?;
        tree.compute_layout_with_measure(
            root,
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::Definite(width),
                height: height
                    .map(taffy_layout::AvailableSpace::Definite)
                    .unwrap_or(taffy_layout::AvailableSpace::MaxContent),
            },
            |known_dimensions, available_space, _node_id, node_context, _style| {
                measure_grid_item(known_dimensions, available_space, node_context)
            },
        )
        .ok()?;
        let root_layout = tree.layout(root).ok()?;
        let mut grid_item_areas = Vec::new();
        let gap_gutters = match tree.detailed_layout_info(root) {
            taffy::tree::DetailedLayoutInfo::Grid(info) => {
                grid_item_areas = info
                    .items
                    .iter()
                    .map(|item| GridItemArea {
                        row_start: item.row_start,
                        row_end: item.row_end,
                        column_start: item.column_start,
                        column_end: item.column_end,
                    })
                    .collect();
                grid_gap_decoration_gutters_from_tracks(
                    &info.columns.sizes,
                    &info.columns.gutters,
                    &info.rows.sizes,
                    &info.rows.gutters,
                    style,
                    width,
                    root_layout.size.height,
                )
            }
            taffy::tree::DetailedLayoutInfo::None => GapDecorationGridGutters::default(),
        };
        let mut items = Vec::with_capacity(nodes.len());
        for (index, node) in nodes.into_iter().enumerate() {
            let layout = tree.layout(node).ok()?;
            items.push(GridItemLayout {
                x: layout.location.x,
                y: layout.location.y,
                width: layout.size.width,
                height: layout.size.height,
                area: grid_item_areas.get(index).copied(),
            });
        }
        Some(GridLayout {
            height: root_layout.size.height,
            items,
            gap_gutters,
        })
    }
}

pub(in crate::layout) fn definite_grid_gap_size(gap: css::ComputedGap, container_size: f32) -> f32 {
    match gap {
        css::ComputedGap::Normal => 0.0,
        css::ComputedGap::LengthPercentage(value) => value
            .used_length_with_percentage_basis(container_size)
            .unwrap_or(value.length_with_percentage_basis(container_size)),
    }
}
