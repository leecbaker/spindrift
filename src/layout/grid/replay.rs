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
    pub(in crate::layout::grid) source_item_top: PageTopBlockPosition,
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn replay_grid_fragment_record_items(
        &mut self,
        fragment_record: GridFragmentRecord,
        parent_style: &ComputedStyle,
        parent_layout: &GridLayout,
        children: &[GridChild<'_>],
        items: &[GridItemLayout],
        stylesheets: &Stylesheets<'_>,
        inner_x: f32,
        cursor: GridFragmentCursor,
    ) {
        for mut item_fragment in fragment_record.item_fragments(items) {
            let child = &children[item_fragment.item_index];
            let baseline_resolution = parent_layout
                .baseline_resolutions
                .get(item_fragment.item_index);
            item_fragment.metadata =
                self.grid_item_fragment_metadata(&item_fragment, inner_x, cursor);
            if item_fragment.requires_split_replay() {
                self.replay_split_grid_item_fragment(
                    child,
                    &item_fragment,
                    baseline_resolution,
                    stylesheets,
                    inner_x,
                    cursor,
                );
            } else {
                let mut replay = |layout: &mut Self| {
                    layout.replay_grid_item_at_fragment_cursor(
                        child,
                        &item_fragment.original,
                        stylesheets,
                        inner_x,
                        cursor,
                        Some(&mut item_fragment.metadata),
                        None,
                        baseline_resolution,
                    );
                };
                if let Some(area) = item_fragment.original.area
                    && child.element_parts().is_none_or(|(element, _, _)| {
                        !layout_containment_applies_to_element(element, &child.style)
                            && !paint_containment_applies_to_element(element, &child.style)
                    })
                    && let Some(context) = ResolvedSubgridContext::from_parent(
                        parent_style,
                        parent_layout,
                        &child.style,
                        area,
                    )
                {
                    self.with_resolved_subgrid_context(context, replay);
                } else {
                    replay(self);
                }
            }
        }
    }

    /// Replay one grid item with a scoped view of its parent's resolved axes.
    ///
    /// Taffy has no subgrid API. The child grid therefore consumes the shared
    /// axis context at its own sizing boundary; replay never rewrites the
    /// child style into a synthetic standalone grid. Nested subgrids derive a
    /// fresh context from their immediately enclosing final grid geometry.
    /// <https://drafts.csswg.org/css-grid-2/#subgrids>
    #[allow(clippy::too_many_arguments)]
    pub(super) fn replay_grid_item_with_resolved_axes(
        &mut self,
        parent_style: &ComputedStyle,
        parent_layout: &GridLayout,
        child: &GridChild<'_>,
        item: &GridItemLayout,
        baseline_resolution: Option<&GridBaselineResolution>,
        stylesheets: &Stylesheets<'_>,
        inner_x: f32,
        content_top: PageTopBlockPosition,
    ) {
        let replay = |layout: &mut Self| {
            layout.replay_grid_item_at_fragment_cursor(
                child,
                item,
                stylesheets,
                inner_x,
                GridFragmentCursor::new(content_top, GridFragmentBlockOffset::new(0.0)),
                None,
                None,
                baseline_resolution,
            );
        };
        if let Some(area) = item.area
            && child.element_parts().is_none_or(|(element, _, _)| {
                !layout_containment_applies_to_element(element, &child.style)
                    && !paint_containment_applies_to_element(element, &child.style)
            })
            && let Some(context) =
                ResolvedSubgridContext::from_parent(parent_style, parent_layout, &child.style, area)
        {
            self.with_resolved_subgrid_context(context, replay);
        } else {
            replay(self);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn replay_grid_item_at_fragment_cursor(
        &mut self,
        child: &GridChild<'_>,
        item: &GridItemLayout,
        stylesheets: &Stylesheets<'_>,
        inner_x: f32,
        cursor: GridFragmentCursor,
        metadata: Option<&mut FragmentPageMetadata>,
        materialized_style: Option<&ComputedStyle>,
        baseline_resolution: Option<&GridBaselineResolution>,
    ) {
        let item_width = item.width().max(0.0);
        let item_height = item.height().max(0.0);

        let owned_placed_style;
        let placed_style = if let Some(style) = materialized_style {
            style
        } else {
            let fallback_style = baseline_resolution.and_then(|resolution| {
                grid_baseline_content_fallback_style(&child.style, *resolution)
            });
            owned_placed_style = grid_placed_item_style(
                fallback_style.as_ref().unwrap_or(&child.style),
                item,
                item_width,
                item_height,
            );
            &owned_placed_style
        };
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
                // Anonymous grid items need the grid-assigned content box as
                // their inline formatting context; unlike element items they
                // have no principal-box dispatch to install that basis.
                // <https://www.w3.org/TR/css-grid-1/#grid-items>.
                scope_content_logical_inline_size: child.anonymous_content().is_some(),
                cursor_y: cursor
                    .source_block_y(GridFragmentBlockOffset::new(item.y()))
                    .points(),
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
            },
            placed_style,
            |layout| {
                if metadata.is_some() {
                    layout.begin_assignment_capture_frame();
                }
                layout.layout_formatting_context_item_contents(child, placed_style, stylesheets);
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
            .page_top_rect(cursor.grid_container_origin(inner_x))
            .paint_clip();
        let policy = StackingContextPolicy::for_grid_item(placed_style, item_border_box);
        if !matches!(policy.context_kind, StackingContextKind::None) {
            let child_contexts = self.positioned_child_contexts_since(
                item_positioned_layer_start,
                item_page_index,
                &policy,
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
            .page_top_rect(cursor.grid_container_origin(inner_x))
            .paint_clip();
        let mut metadata = FragmentPageMetadata::new(
            self.pages.len(),
            Some(item_border_box),
            !self.current_page_has_content(),
        );
        metadata.continues_from_previous_page =
            item_fragment.content_slice.block_start.points() > 0.01;
        metadata.continues_to_next_page =
            item_fragment.content_slice.block_end.points() < item_height - 0.01;
        metadata
    }

    fn replay_split_grid_item_fragment(
        &mut self,
        child: &GridChild<'_>,
        item_fragment: &GridItemFragment,
        baseline_resolution: Option<&GridBaselineResolution>,
        stylesheets: &Stylesheets<'_>,
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

        let fallback_style = baseline_resolution
            .and_then(|resolution| grid_baseline_content_fallback_style(&child.style, *resolution));
        let placed_style = grid_placed_item_style(
            fallback_style.as_ref().unwrap_or(&child.style),
            item,
            item_width,
            item_height,
        );
        let slice_border_box = visible
            .page_top_rect(cursor.grid_container_origin(inner_x))
            .paint_clip();
        let source_item_top = cursor.source_block_y(GridFragmentBlockOffset::new(item.y()));
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
        stylesheets: &Stylesheets<'_>,
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
                scope_content_logical_inline_size: child.anonymous_content().is_some(),
                cursor_y: offpage_top,
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
            },
            placed_style,
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
                source_item_top.points() - offpage_top,
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
            source_item_top.points(),
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
    item: &GridItemLayout,
    item_width: f32,
    item_height: f32,
) -> ComputedStyle {
    let mut placed_style =
        replayed_item_fragmentation_base_style(child_style, ReplayedItemFragmentationPolicy::Grid);
    if let Some(metrics) = item.used_box_metrics() {
        // The Taffy placement is already the border-box origin after applying
        // grid-area margins, so replay must keep those margins suppressed.
        // Padding, however, participates in the replayed item's used
        // border-box geometry and must use the same resolved edges as Taffy.
        placed_style.padding = metrics.padding.to_css_edges();
    }
    set_style_used_width(&mut placed_style, item_width);
    set_style_used_height(&mut placed_style, item_height);
    // Both used axes are already definite. Replaying them through additional
    // content-box min/max constraints after switching to border-box sizing
    // would subtract borders and padding a second time.
    // <https://www.w3.org/TR/css-grid-1/#grid-item-sizing>
    placed_style.box_sizing = BoxSizing::BorderBox;
    placed_style
}

/// Materialize the Grid baseline fallback for a grid item's own content.
///
/// A cyclic baseline request is replaced before the item's formatting context
/// is replayed. This keeps its used content alignment equal to an authored
/// `start`/`end` fallback rather than merely excluding it from the grid's
/// measured sharing group:
/// <https://www.w3.org/TR/css-grid-1/#row-align> and
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content>.
fn grid_baseline_content_fallback_style(
    child_style: &ComputedStyle,
    resolution: GridBaselineResolution,
) -> Option<ComputedStyle> {
    let row_fallback = resolution.content_alignment_fallback(GridAxis::Row);
    let column_fallback = resolution.content_alignment_fallback(GridAxis::Column);
    if row_fallback.is_none() && column_fallback.is_none() {
        return None;
    }

    let mut fallback_style = child_style.clone();
    if let Some(baseline_set) = row_fallback {
        fallback_style.align_content = css::AlignContent::safe(match baseline_set {
            GridBaselineSet::First => css::ContentAlignmentKeyword::Start,
            GridBaselineSet::Last => css::ContentAlignmentKeyword::End,
        });
    }
    if let Some(baseline_set) = column_fallback {
        fallback_style.justify_content = css::JustifyContent::safe(match baseline_set {
            GridBaselineSet::First => css::ContentAlignmentKeyword::Start,
            GridBaselineSet::Last => css::ContentAlignmentKeyword::End,
        });
    }
    Some(fallback_style)
}
