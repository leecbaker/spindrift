use super::*;

/// Replay context for one split grid item fragment.
///
/// CSS Fragmentation slices the visual fragment while keeping the source
/// item's internal layout in its original coordinate system:
/// <https://www.w3.org/TR/css-break-3/#box-splitting> and
/// <https://www.w3.org/TR/css-grid-1/#pagination>.
pub(in crate::layout::grid) struct SplitGridItemPaintContext {
    /// The used physical border-box dimensions from CSS Grid placement.
    pub(in crate::layout::grid) item_width: BorderBoxLength,
    pub(in crate::layout::grid) item_height: BorderBoxLength,
    pub(in crate::layout::grid) slice_border_box: PaintClip,
    pub(in crate::layout::grid) source_item_top: f32,
}

impl<'a> LayoutBuilder<'a> {
    /// Replay grid items that belong to one committed grid fragment record.
    ///
    /// Whole item fragments use normal formatting-context replay and update
    /// captured running-element assignments to this fragment's committed
    /// page-local placement. Items crossing the committed slice boundary are
    /// replayed from their original source item layout and clipped to the
    /// selected page-local slice:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/css-grid-1/#pagination>.
    pub(super) fn replay_grid_fragment_record_items(
        &mut self,
        fragment_record: GridFragmentRecord,
        children: &[GridChild<'_>],
        items: &[GridItemLayout],
        stylesheets: &[Stylesheet],
        inner_x: f32,
        cursor: GridFragmentCursor,
    ) {
        for mut item_fragment in fragment_record.item_fragments(items) {
            let child = &children[item_fragment.item_index];
            item_fragment.metadata =
                self.grid_item_fragment_metadata(&item_fragment, inner_x, cursor);
            if item_fragment.requires_split_replay() {
                self.replay_split_grid_item_fragment(
                    child,
                    &item_fragment,
                    stylesheets,
                    inner_x,
                    cursor,
                );
            } else {
                self.replay_grid_item_at_fragment_cursor(
                    child,
                    &item_fragment.original,
                    stylesheets,
                    inner_x,
                    cursor,
                    Some(&mut item_fragment.metadata),
                );
            }
        }
    }

    /// Replay one laid-out grid item through Quire's existing child layout path.
    ///
    /// CSS Grid computes a grid area's geometry, then the grid item establishes
    /// its own formatting context inside that area:
    /// <https://www.w3.org/TR/css-grid-1/#grid-items>.
    pub(super) fn replay_grid_item(
        &mut self,
        child: &GridChild<'_>,
        item: &GridItemLayout,
        stylesheets: &[Stylesheet],
        inner_x: f32,
        content_top: f32,
    ) {
        self.replay_grid_item_at_fragment_cursor(
            child,
            item,
            stylesheets,
            inner_x,
            GridFragmentCursor::new(content_top, 0.0),
            None,
        );
    }

    fn replay_grid_item_at_fragment_cursor(
        &mut self,
        child: &GridChild<'_>,
        item: &GridItemLayout,
        stylesheets: &[Stylesheet],
        inner_x: f32,
        cursor: GridFragmentCursor,
        metadata: Option<&mut FragmentPageMetadata>,
    ) {
        let item_width = item.width().max(0.0);
        let item_height = item.height().max(0.0);

        let placed_style = grid_placed_item_style(&child.style, item_width, item_height);
        let item_paint_checkpoint = self.current_page.paint_checkpoint();
        let item_positioned_layer_start = self.positioned_layers.len();
        let item_page_index = self.pages.len();
        self.with_formatting_context_item_placement(
            FormattingContextItemPlacement {
                content_left: inner_x + item.x(),
                content_width: PhysicalContentWidth::new(content_box_pt(item_width)),
                content_height: Some(PhysicalContentHeight::new(content_box_pt(item_height))),
                table_wrapper_border_box_block_size: auto_table_wrapper_block_size_override(
                    &child.style,
                    border_box_pt(item_height),
                ),
                writing_mode: placed_style.writing_mode,
                scope_content_logical_inline_size: false,
                cursor_y: cursor.source_block_y(item.y()),
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
            },
            |layout| {
                if metadata.is_some() {
                    layout.begin_assignment_capture_frame();
                }
                layout.layout_formatting_context_item_contents(child, &placed_style, stylesheets);
                if let Some(metadata) = metadata {
                    metadata.assignment_ids = layout.end_assignment_capture_frame();
                    if !metadata.assignment_ids.is_empty() {
                        layout.update_running_assignment_placements(
                            &metadata.assignment_ids,
                            metadata.assignment_placement(),
                        );
                    }
                }
            },
        );
        // Static grid items with a non-auto z-index establish stacking
        // contexts. Same-page replay must therefore scope their emitted paint
        // tree just as fragmented grid items and flex items do, so source
        // order cannot paint an auto-level sibling above a positive stack
        // level:
        // <https://www.w3.org/TR/css-grid-1/#z-order>.
        let item_border_box = item
            .page_top_rect(inner_x, cursor.content_top + cursor.block_offset)
            .paint_clip();
        let policy = StackingContextPolicy::for_grid_item(&placed_style, item_border_box);
        if !matches!(policy.context_kind, StackingContextKind::None) {
            let child_contexts = self.positioned_child_contexts_since(
                item_positioned_layer_start,
                item_page_index,
                policy,
            );
            self.scope_current_page_paint_since_with_policy(
                &item_paint_checkpoint,
                policy,
                item_border_box,
                child_contexts,
            );
        }
    }

    fn grid_item_fragment_metadata(
        &self,
        item_fragment: &GridItemFragment,
        inner_x: f32,
        cursor: GridFragmentCursor,
    ) -> FragmentPageMetadata {
        let item = &item_fragment.original;
        let visible = &item_fragment.visible;
        let item_height = item.height().max(0.0);
        let item_border_box = visible
            .page_top_rect(inner_x, cursor.content_top + cursor.block_offset)
            .paint_clip();
        let mut metadata = FragmentPageMetadata::new(
            self.pages.len(),
            Some(item_border_box),
            !self.current_page_has_content(),
        );
        metadata.continues_from_previous_page = item_fragment.content_slice.block_start > 0.01;
        metadata.continues_to_next_page =
            item_fragment.content_slice.block_end < item_height - 0.01;
        metadata
    }

    fn replay_split_grid_item_fragment(
        &mut self,
        child: &GridChild<'_>,
        item_fragment: &GridItemFragment,
        stylesheets: &[Stylesheet],
        inner_x: f32,
        cursor: GridFragmentCursor,
    ) {
        let item = &item_fragment.original;
        let item_width = item.width().max(0.0);
        let item_height = item.height().max(0.0);
        let visible = &item_fragment.visible;
        let slice_height = visible.height().max(0.0);
        if item_width <= 0.0 || item_height <= 0.0 || slice_height <= 0.0 {
            return;
        }

        let placed_style = grid_placed_item_style(&child.style, item_width, item_height);
        let slice_border_box = visible
            .page_top_rect(inner_x, cursor.content_top + cursor.block_offset)
            .paint_clip();
        let source_item_top = cursor.source_block_y(item.y());
        self.paint_split_grid_item_fragment(
            child,
            &placed_style,
            stylesheets,
            SplitGridItemPaintContext {
                item_width: border_box_pt(item_width),
                item_height: border_box_pt(item_height),
                slice_border_box,
                source_item_top,
            },
        );
    }

    /// Replay a split grid item from its original item layout and clip the
    /// selected page-local slice.
    ///
    /// CSS Fragmentation preserves source layout for continuations, while the
    /// grid fragment plan commits the visible slice for the current
    /// fragmentainer:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/css-grid-1/#pagination>.
    fn paint_split_grid_item_fragment(
        &mut self,
        child: &GridChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        context: SplitGridItemPaintContext,
    ) {
        let item_width = context.item_width.points();
        let item_height = context.item_height.points();
        let slice_border_box = context.slice_border_box;
        let source_item_top = context.source_item_top;
        if slice_border_box.width() <= 0.0 || slice_border_box.height() <= 0.0 {
            return;
        }

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let offpage_top = 10_000.0;
        self.current_page = Page::new(item_width.max(1.0), offpage_top);
        self.overflow_clips.clear();
        self.fragment_top_offsets.clear();

        self.with_formatting_context_item_placement(
            FormattingContextItemPlacement {
                content_left: 0.0,
                content_width: PhysicalContentWidth::new(content_box_pt(item_width)),
                content_height: Some(PhysicalContentHeight::new(content_box_pt(item_height))),
                table_wrapper_border_box_block_size: auto_table_wrapper_block_size_override(
                    &child.style,
                    border_box_pt(item_height),
                ),
                writing_mode: placed_style.writing_mode,
                scope_content_logical_inline_size: false,
                cursor_y: offpage_top,
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
            },
            |layout| {
                layout.layout_formatting_context_item_contents(child, placed_style, stylesheets);
                layout.flush_positioned_layers_since(positioned_layer_start);
            },
        );

        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintTranslation::new(
                slice_border_box.x(),
                source_item_top - offpage_top,
            ))
            .clipped_to_rect(slice_border_box);
        self.restore(snapshot);

        if fragment.is_empty() {
            return;
        }

        let policy = StackingContextPolicy::for_grid_item(placed_style, slice_border_box);
        let mut effects = policy.effects;
        effects.overflow_clip = Some(slice_border_box);
        effects.absolute_clip = Some(slice_border_box);
        let source_bounds = PageTopRect::new(
            slice_border_box.x(),
            source_item_top,
            slice_border_box.width(),
            item_height,
        )
        .paint_clip();
        let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(effects)
            .with_bounds(source_bounds);
        let fragment = PaintFragment::from_stacking_context_in_band(policy.parent_band, context);
        self.current_page
            .append_paint_fragment_owned(fragment, PaintTranslation::identity());
    }
}

fn grid_placed_item_style(
    child_style: &ComputedStyle,
    item_width: f32,
    item_height: f32,
) -> ComputedStyle {
    let mut placed_style =
        replayed_item_fragmentation_base_style(child_style, ReplayedItemFragmentationPolicy::Grid);
    set_style_used_width(&mut placed_style, item_width);
    set_style_used_height(&mut placed_style, item_height);
    // Both used axes are already definite. Replaying them through additional
    // content-box min/max constraints after switching to border-box sizing
    // would subtract borders and padding a second time.
    // <https://www.w3.org/TR/css-grid-1/#grid-item-sizing>
    placed_style.box_sizing = BoxSizing::BorderBox;
    placed_style
}
