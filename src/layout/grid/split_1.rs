use super::*;
use crate::layout::block::{DefiniteBlockBreakContext, should_prebreak_definite_block};

impl<'a> LayoutBuilder<'a> {
    /// Lay out a CSS grid container, preserving a flex item's post-flexing
    /// definiteness when grid is the root formatting context of a replayed
    /// flex item.
    ///
    /// Flex replay materializes its final geometry in a temporary computed
    /// style. That numeric height is only a layout transport value: whether it
    /// is a definite percentage basis is determined by Flexbox, not by the
    /// temporary `height` declaration. A grid root must therefore consume the
    /// scoped basis before resolving its own rows and descendant percentages:
    /// <https://drafts.csswg.org/css-flexbox/#definite-sizes> and
    /// <https://www.w3.org/TR/css-grid-1/#grid-container-size>.
    pub(in crate::layout) fn layout_grid_with_descendant_percentage_height_basis(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
    ) {
        let source_style = style;
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

        let fragmentainer_kind = self.active_fragmentainer_kind();
        self.apply_forced_break_before_box_in(fragmentainer_kind, style);
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let mut used_style =
            GridUsedStyle::from_normalized(self.style_with_current_viewport_lengths(style));
        if layout_containment_applies_to_element(element, &used_style)
            || paint_containment_applies_to_element(element, &used_style)
        {
            used_style.grid_template_rows.resolve_contained_subgrid();
            used_style.grid_template_columns.resolve_contained_subgrid();
        }
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(containing_inline_size)),
        );
        let relative_offset = self.normal_flow_relative_position_offset(&used_style);
        if matches!(used_style.position, Position::Relative | Position::Sticky) {
            self.cursor_y += relative_offset.y();
        }

        let available_outer_width =
            normal_flow_block_available_outer_width(&used_style, layout_pt(containing_inline_size));
        let border_widths = box_metrics.border.to_css_edges();
        let horizontal_extras = box_metrics.horizontal_non_content_length().points();
        let vertical_extras = box_metrics.vertical_non_content_length().points();
        let current_fragmentainer =
            self.fragmentainer_from_page_cursor(PageTopBlockPosition::new(self.cursor_y));
        let available_outer_height = current_fragmentainer
            .available_block_size_after_reservation(layout_pt(
                used_style.margin.top + used_style.margin.bottom,
            ))
            .points();
        let explicit_content_height = descendant_percentage_height_basis
            .map(PercentageBasis::points)
            .unwrap_or_else(|| {
                used_content_box_height_or_auto(
                    &used_style,
                    layout_pt(available_outer_height),
                    non_content_pt(vertical_extras),
                )
                .map(SemanticLengthExt::points)
            })
            .map(|height| {
                PhysicalContentHeight::new(constrain_content_height(
                    &used_style,
                    content_box_pt(height),
                    PercentageBasis::definite(layout_pt(available_outer_height)),
                ))
            });
        let requested_content_width = self.used_block_physical_content_width(
            element,
            &used_style,
            stylesheets,
            child_boxes,
            BlockContentWidthInputs {
                available_outer_width,
                percentage_basis: PercentageBasis::definite(layout_pt(containing_inline_size)),
                horizontal_non_content: non_content_pt(horizontal_extras),
                definite_content_height: explicit_content_height,
            },
        );
        // A grid container participates in normal block sizing before its
        // track-sizing algorithm runs.  Thus an automatic physical width can
        // be transferred from a definite physical height through the
        // preferred aspect ratio, and vice versa.  The resulting dimensions
        // are passed to track sizing as the grid's used content box so tracks
        // and percentage descendants observe the same definite geometry.
        // <https://www.w3.org/TR/css-sizing-4/#aspect-ratio> and
        // <https://www.w3.org/TR/css-grid-1/#grid-container-size>
        let requested_content_width = explicit_content_height
            .and_then(|height| {
                non_replaced_aspect_ratio_content_width(
                    &used_style,
                    height.points(),
                    horizontal_extras,
                    vertical_extras,
                )
            })
            .map(|width| PhysicalContentWidth::new(content_box_pt(width)))
            .unwrap_or(requested_content_width);
        let resolve_auto_margins = used_style.float == Float::None;
        let width = resolve_normal_flow_block_inline_geometry(
            &mut used_style,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
            requested_content_width,
            non_content_pt(horizontal_extras),
            self.containing_block_direction,
            resolve_auto_margins,
        );
        let content_width = width.content_width.points();
        let outer_width = width.border_box_width().points();
        let style = &used_style;
        let mut outer_x = width.border_box_inline_span.left_x() + relative_offset.x();
        let mut inner_x = outer_x + border_widths.left + style.padding.left;
        let inner_width = content_width.max(0.0);
        let definite_content_height = explicit_content_height
            .or_else(|| {
                non_replaced_aspect_ratio_content_height(
                    style,
                    content_width,
                    horizontal_extras,
                    vertical_extras,
                )
                .map(|height| PhysicalContentHeight::new(content_box_pt(height)))
            })
            .map(|height| {
                PhysicalContentHeight::new(constrain_content_height(
                    style,
                    height.content_box_length(),
                    PercentageBasis::definite(layout_pt(available_outer_height)),
                ))
            });

        // A definite-size grid container is a normal-flow block before Grid
        // lays out its tracks. Give it the same class-A prebreak opportunity
        // as an ordinary definite block, including its outer margins, so a
        // final fixed-size grid does not paint past the fragmentainer edge.
        // <https://www.w3.org/TR/css-break-3/#possible-breaks> and
        // <https://www.w3.org/TR/css-grid-1/#grid-containers>.
        let empty_destination_fragmentainer = match fragmentainer_kind {
            FragmentainerKind::Page => {
                let next_context = self.resolved_page_context(self.pages.len() + 2, false);
                Fragmentainer::new(
                    layout_pt(next_context.area_height()),
                    layout_pt(next_context.area_height()),
                )
            }
            FragmentainerKind::Column => {
                let next_capacity = self
                    .fragmentainer_override
                    .map(|override_| {
                        override_
                            .context_for_fragmentainer(self.pages.len() + 1)
                            .area_height()
                    })
                    .unwrap_or_else(|| self.page_area_height());
                Fragmentainer::new(layout_pt(next_capacity), layout_pt(next_capacity))
            }
        };
        if should_prebreak_definite_block(DefiniteBlockBreakContext {
            definite_content_height: definite_content_height.map(PhysicalContentHeight::points),
            vertical_non_content: box_metrics.vertical_non_content_length(),
            style,
            current_fragmentainer,
            empty_destination_fragmentainer,
            fragmentainer_has_occupied_flow: self.current_page_has_content()
                || self.cursor_y < self.page_top() - 0.01,
            at_page_top: self.cursor_is_at_page_top(),
            suppress_for_avoid_retry: self.avoid_inside_retry_depth > 0,
        }) {
            self.push_page();
            self.layout_grid_with_descendant_percentage_height_basis(
                element,
                style,
                stylesheets,
                child_boxes,
                descendant_percentage_height_basis,
            );
            return;
        }

        let built_child_boxes;
        let child_boxes = if let Some(child_boxes) = child_boxes {
            child_boxes
        } else {
            built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                element,
                stylesheets,
                source_style,
            );
            &built_child_boxes
        };
        let (children, positioned_children) = grid_child_lists_from_boxes(child_boxes);
        let children = self.prepare_grid_children(children);
        let positioned_children = self.prepare_grid_children(positioned_children);

        // Size containment is a two-stage operation for grid containers. Size
        // the principal box using an empty grid (explicit tracks and gaps still
        // apply), then lay out the real items into that resulting used size.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        // <https://www.w3.org/TR/css-grid-1/#algo-overview>
        let size_contained_content_height = if style.contain.size {
            let empty_grid_height = self
                .compute_grid_layout(
                    style,
                    &[],
                    stylesheets,
                    PhysicalContentWidth::new(content_box_pt(inner_width)),
                    definite_content_height,
                    GridLayoutPurpose::IntrinsicProbe,
                )
                .map(|layout| layout.height)
                .unwrap_or_else(|| PhysicalContentHeight::new(content_box_pt(0.0)));
            Some(definite_content_height.unwrap_or_else(|| {
                PhysicalContentHeight::new(constrain_content_height(
                    style,
                    empty_grid_height.content_box_length(),
                    PercentageBasis::definite(layout_pt(available_outer_height)),
                ))
            }))
        } else {
            None
        };
        let grid_content_height_basis = size_contained_content_height.or(definite_content_height);
        // The principal box of an auto-sized size-contained grid has the
        // empty-grid height, but its real grid items are still formatted.
        // Do not feed that synthetic principal height back into track sizing:
        // doing so collapses automatic tracks and suppresses their permitted
        // visual overflow. An authored definite height remains a normal track
        // sizing constraint.
        //
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        let grid_item_layout_height_basis = definite_content_height;

        self.cursor_y -= style.margin.top;
        if style.float == Float::None {
            let margin_box_width = style.margin.left + outer_width + style.margin.right;
            let collision_height = grid_content_height_basis
                .map(PhysicalContentHeight::points)
                .unwrap_or(style.line_height)
                + vertical_extras
                + style.margin.top
                + style.margin.bottom;
            let placement = self.place_float_avoiding_margin_box(
                PageTopBlockPosition::new(self.cursor_y),
                margin_box_size_pt(margin_box_width, collision_height),
                style.clear,
                style.writing_mode,
                style.direction,
                self.containing_block_direction,
            );
            self.cursor_y = placement.origin.top_y();
            outer_x = placement.origin.x() + style.margin.left + relative_offset.x();
            inner_x = outer_x + border_widths.left + style.padding.left;
        } else {
            self.cursor_y = self
                .clear_active_floats_top(
                    style.clear,
                    style.writing_mode,
                    style.direction,
                    PageTopBlockPosition::new(self.cursor_y),
                )
                .points();
        }

        let border_box_inline_span = PageInlineSpan::new(outer_x, outer_width);
        let block_top = self.cursor_y;
        let paint_page_index = self.pages.len();
        let paint_checkpoint = self.current_page.paint_checkpoint();
        self.cursor_y -= border_widths.top + style.padding.top;
        let content_top = self.cursor_y;
        let Some(grid_layout) = self.compute_grid_layout(
            style,
            &children,
            stylesheets,
            PhysicalContentWidth::new(content_box_pt(inner_width)),
            grid_item_layout_height_basis,
            GridLayoutPurpose::FinalLayout,
        ) else {
            let mut flow_style = style.clone();
            flow_style.display = Display::BLOCK;
            suppress_replayed_item_margins(&mut flow_style);
            self.layout_block(element, &flow_style, stylesheets, &[], Some(child_boxes));
            return;
        };
        let total_content_height = size_contained_content_height
            .map(PhysicalContentHeight::points)
            .unwrap_or_else(|| {
                constrain_content_height(
                    style,
                    grid_layout.height.content_box_length(),
                    PercentageBasis::definite(layout_pt(available_outer_height)),
                )
                .points()
            });
        let overflow_clip_active = if self.element_used_overflow_clips(element, style) {
            self.push_padding_box_overflow_clip(
                style,
                outer_x,
                block_top,
                border_widths,
                content_width,
                total_content_height,
            )
        } else {
            false
        };
        let suppresses_descendant_fragmentation =
            style.contain.size || (overflow_clip_active && definite_content_height.is_some());
        let grid_fragment_plan = if suppresses_descendant_fragmentation {
            GridFragmentPlan::unfragmented(fragmentainer_kind, total_content_height)
        } else {
            GridFragmentPlan::from_grid_item_boundaries(
                fragmentainer_kind,
                current_fragmentainer,
                total_content_height,
                &grid_layout.row_line_offsets,
                &grid_layout.items,
                &children,
            )
        };
        let grid_fragment_plan_starts_after_break =
            grid_fragment_plan.starts_after_fragmentainer_break();
        debug_assert!(
            !grid_fragment_plan_starts_after_break
                || grid_fragment_plan.requires_multiple_fragments()
        );
        debug_assert!(
            !grid_fragment_plan.requires_multiple_fragments()
                || !grid_fragment_plan.slices().is_empty()
        );
        // Fragment coordinates are calculated by intersecting independently
        // rounded `f32` track and fragmentainer boundaries. Permit the small
        // rounding error that intersection can introduce while retaining the
        // ordering invariant; real negative or inverted slices remain bugs.
        const FRAGMENT_COORDINATE_EPSILON: f32 = 0.01;
        debug_assert!(
            grid_fragment_plan
                .fragment_records()
                .iter()
                .all(|fragment| {
                    fragment
                        .item_fragments(&grid_layout.items)
                        .iter()
                        .all(|item| {
                            item.item_index < grid_layout.items.len()
                                && item.visible.height() >= -FRAGMENT_COORDINATE_EPSILON
                                && item.original.height() + FRAGMENT_COORDINATE_EPSILON
                                    >= item.visible.height()
                                && item.content_slice.block_end.points()
                                    + FRAGMENT_COORDINATE_EPSILON
                                    >= item.content_slice.block_start.points()
                        })
                })
        );
        debug_assert!(
            grid_fragment_plan
                .fragment_records()
                .iter()
                .all(|fragment| {
                    fragment
                        .transition_before_fragment
                        .is_none_or(|transition| {
                            fragment.fragmentainer_offset > 0
                                && transition.next_block_offset.points()
                                    <= fragment.slice.source_block_start.points() + 0.01
                                && matches!(
                                    transition.reason,
                                    GridFragmentTransitionReason::InitialOverflow
                                        | GridFragmentTransitionReason::SliceContinuation
                                )
                        })
                })
        );
        let grid_fragment_records = grid_fragment_plan.fragment_records();
        let can_replay_committed_fragment_records =
            grid_fragment_plan.requires_multiple_fragments() && positioned_children.is_empty();
        let total_height = border_widths.top
            + style.padding.top
            + total_content_height
            + style.padding.bottom
            + border_widths.bottom;
        let positioning_containing_block_mode =
            PositionedContainingBlockMode::for_element(element, style);
        let establishes_positioning_containing_block = positioning_containing_block_mode.is_some();
        let positioned_containing_block_scope =
            if let Some(mode) = positioning_containing_block_mode {
                let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
                    outer_x + border_widths.left,
                    block_top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                ));
                Some(self.push_positioned_containing_block(mode, containing_block))
            } else {
                None
            };
        let pushed_grid_positioning_scope = if let Some(containing_block) =
            positioned_containing_block_scope
                .is_some()
                .then(|| self.containing_blocks.last().cloned())
                .flatten()
        {
            self.grid_positioning_scopes.push(GridPositioningScope::new(
                style,
                GridPositioningGeometry {
                    inner_x,
                    inner_width: PhysicalContentWidth::new(content_box_pt(inner_width)),
                    content_top,
                    content_height: PhysicalContentHeight::new(content_box_pt(
                        total_content_height,
                    )),
                    column_line_offsets: &grid_layout.column_line_offsets,
                    row_line_offsets: &grid_layout.row_line_offsets,
                },
                containing_block,
            ));
            true
        } else {
            false
        };

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        // Grid item replay establishes the grid container as the formatting
        // context that supplies inherited writing-mode and direction to its
        // descendants. In particular, the absolute-positioning equations use
        // the containing block's direction when both inline insets are auto.
        // <https://www.w3.org/TR/css-grid-1/#grid-containers> and
        // <https://www.w3.org/TR/css-position-3/#abspos-insets>
        let previous_containing_block_direction = self.containing_block_direction;
        let previous_containing_block_writing_mode = self.containing_block_writing_mode;
        self.containing_block_direction = style.used_direction();
        self.containing_block_writing_mode = style.writing_mode;
        if suppresses_descendant_fragmentation {
            // Size containment makes the grid principal box monolithic while
            // still laying out real items into its empty-grid used size. Apply
            // the suppression only after principal sizing and fragmentation
            // planning so the grid box itself remains an ordinary participant
            // in the outer flow.
            // <https://www.w3.org/TR/css-contain-1/#containment-size> and
            // <https://www.w3.org/TR/css-break-3/#monolithic>.
            self.fragmentation_suppression_depth += 1;
        }
        let mut committed_gap_fragment_paint_bounds = Vec::new();
        let committed_replay_end_cursor = if can_replay_committed_fragment_records {
            let mut fragment_cursor = GridFragmentCursor::new(
                PageTopBlockPosition::new(content_top),
                GridFragmentBlockOffset::new(0.0),
            );
            self.push_float_context();
            for fragment_record in &grid_fragment_records {
                if let Some(transition) = fragment_record.transition_before_fragment
                    && let Some(content_top) = self.materialize_fragmentainer_advance(
                        transition.fragmentainer_kind,
                        FragmentainerAdvance::Unforced,
                    )
                {
                    fragment_cursor = transition
                        .cursor_after_fragmentainer_advance(PageTopBlockPosition::new(content_top));
                }
                let paint_checkpoint = self.current_page.paint_checkpoint();
                self.replay_grid_fragment_record_items(
                    *fragment_record,
                    style,
                    &grid_layout,
                    &children,
                    &grid_layout.items,
                    stylesheets,
                    inner_x,
                    fragment_cursor,
                );
                if let Some(bounds) = self
                    .current_page
                    .paint_tree_fragment_since(&paint_checkpoint)
                    .bounds()
                {
                    committed_gap_fragment_paint_bounds.push((*fragment_record, bounds));
                }
            }
            self.pop_float_context();
            Some(fragment_cursor)
        } else {
            self.push_float_context();
            for (child, item) in children.iter().zip(&grid_layout.items) {
                self.replay_grid_item_with_resolved_axes(
                    style,
                    &grid_layout,
                    child,
                    item,
                    stylesheets,
                    inner_x,
                    PageTopBlockPosition::new(content_top),
                );
            }
            self.pop_float_context();
            None
        };
        for child in &positioned_children {
            self.layout_positioned_grid_child(
                child,
                &children,
                PositionedGridStaticContext {
                    container_style: style,
                    stylesheets,
                    inner_x,
                    inner_width: PhysicalContentWidth::new(content_box_pt(inner_width)),
                    content_top,
                    definite_content_height: grid_content_height_basis,
                    content_height: PhysicalContentHeight::new(content_box_pt(
                        total_content_height,
                    )),
                    column_line_offsets: &grid_layout.column_line_offsets,
                    row_line_offsets: &grid_layout.row_line_offsets,
                    establishes_positioning_containing_block,
                },
            );
        }
        if pushed_grid_positioning_scope {
            self.grid_positioning_scopes.pop();
        }
        if suppresses_descendant_fragmentation {
            self.fragmentation_suppression_depth -= 1;
        }
        self.pop_overflow_clip(overflow_clip_active);
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.containing_block_direction = previous_containing_block_direction;
        self.containing_block_writing_mode = previous_containing_block_writing_mode;

        self.cursor_y = committed_replay_end_cursor
            .map(|cursor| {
                cursor
                    .source_block_y(GridFragmentBlockOffset::new(total_content_height))
                    .points()
            })
            .unwrap_or(content_top - total_content_height);
        self.cursor_y -= style.padding.bottom + border_widths.bottom;
        let block_bottom = self.cursor_y;
        let block_height = (block_top - block_bottom).max(total_height);
        let contents_overflow_clip = overflow_clip_active.then(|| {
            PageTopRect::new(
                outer_x + border_widths.left,
                block_top - border_widths.top,
                content_width + style.padding.left + style.padding.right,
                total_content_height + style.padding.top + style.padding.bottom,
            )
            .paint_clip()
        });
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
                paint_space_rect(outer_x, block_bottom, outer_width, block_height),
                style,
            );
            own_outline_primitives = self.box_outline_primitives(
                paint_space_rect(outer_x, block_bottom, outer_width, block_height),
                style,
            );
        }
        let fragments = self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        let grid_spanned_pages = self.pages.len() != paint_page_index;
        for (page_index, mut fragment) in fragments {
            // A grid item establishes an independent formatting context, but
            // its background still paints as in-flow descendant content of
            // the grid container. Promote its captured background before
            // applying the container's overflow scope so axis longhands clip
            // stretched item backgrounds just as they do for block children.
            // <https://www.w3.org/TR/css-grid-1/#grid-items> and
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>.
            fragment.promote_background_border_to_in_flow_block();
            if page_index == paint_page_index
                && let Some(clip) = contents_overflow_clip
            {
                if overflow_clip_active {
                    fragment = fragment.with_primitives_clipped_to_rect_preserving_structure(clip);
                }
                fragment = fragment.with_contents_effect_scoped_to_rect(clip);
            }
            if grid_spanned_pages {
                let planned_fragment_record = grid_fragment_plan
                    .fragment_record_for_offset(page_index.saturating_sub(paint_page_index));
                let fallback_fragment_bounds = || {
                    fragment.bounds().map(|bounds| {
                        PaintClip::from_paint_rect(paint_space_rect(
                            outer_x,
                            bounds.y(),
                            outer_width,
                            bounds.height(),
                        ))
                    })
                };
                if let Some(fragment_bounds) = planned_fragment_record
                    .map(|fragment_record| {
                        fragment_record.paint_clip(
                            border_box_inline_span,
                            fragment_record.cursor(PageTopBlockPosition::new(content_top)),
                        )
                    })
                    .or_else(fallback_fragment_bounds)
                {
                    let (source_block_start, source_block_end) = planned_fragment_record
                        .map(GridFragmentRecord::source_range)
                        .unwrap_or_else(|| {
                            grid_fragment_source_range_from_bounds(
                                fragment_bounds,
                                PageTopBlockPosition::new(content_top),
                                total_content_height,
                            )
                        });
                    if style.visibility == Visibility::Visible
                        && (style.background_color.is_some()
                            || style.background_image.is_image()
                            || style.border_image.source.is_image()
                            || used_border_width(style) > layout_pt(0.0))
                    {
                        let page_background_primitives = self.box_background_primitives(
                            paint_space_rect(
                                outer_x,
                                fragment_bounds.y(),
                                outer_width,
                                fragment_bounds.height(),
                            ),
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
                                content_origin: PageTopPoint::new(inner_x, content_top),
                                inner_width: PhysicalContentWidth::new(content_box_pt(inner_width)),
                                total_content_height,
                                // Gap-rule segmentation is defined in the grid's
                                // source block coordinate space.  The committed
                                // fragment range below is its only projection to
                                // this page; pre-clipping item geometry here would
                                // apply that range a second time at a break.
                                items: &grid_gap_items,
                                gutters: &grid_layout.gap_gutters,
                                source_block_start,
                                source_block_end,
                                ends_at_fragment_break: planned_fragment_record.is_some_and(
                                    |record| {
                                        matches!(
                                            record.slice.break_after,
                                            GridFragmentBreak::RowBoundary
                                                | GridFragmentBreak::ForcedRowBoundary
                                        )
                                    },
                                ),
                            }),
                        );
                    }
                    if style.visibility == Visibility::Visible {
                        let page_outline_primitives = self.box_outline_primitives(
                            paint_space_rect(
                                outer_x,
                                fragment_bounds.y(),
                                outer_width,
                                fragment_bounds.height(),
                            ),
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
                    if committed_gap_fragment_paint_bounds.is_empty() {
                        fragment.append_primitives_in_band(
                            PaintBand::BackgroundBorder,
                            grid_gap_decoration_primitives(
                                style,
                                GapDecorationContainer::new(
                                    inner_x,
                                    content_top,
                                    grid_used_track_extent(
                                        &grid_layout.column_line_offsets,
                                        &grid_layout.items,
                                        GridAxis::Column,
                                        inner_width,
                                    ),
                                    grid_used_track_extent(
                                        &grid_layout.row_line_offsets,
                                        &grid_layout.items,
                                        GridAxis::Row,
                                        total_content_height,
                                    ),
                                ),
                                &grid_gap_items,
                                &grid_layout.gap_gutters,
                            ),
                        );
                    } else {
                        for (fragment_record, bounds) in &committed_gap_fragment_paint_bounds {
                            let (source_block_start, source_block_end) =
                                fragment_record.source_range();
                            fragment.append_primitives_in_band(
                                PaintBand::BackgroundBorder,
                                grid_gap_decoration_primitives_for_page(
                                    GridGapFragmentProjection {
                                        style,
                                        content_origin: PageTopPoint::new(bounds.x(), content_top),
                                        inner_width: PhysicalContentWidth::new(content_box_pt(
                                            inner_width,
                                        )),
                                        total_content_height,
                                        items: &grid_gap_items,
                                        gutters: &grid_layout.gap_gutters,
                                        source_block_start,
                                        source_block_end,
                                        ends_at_fragment_break: matches!(
                                            fragment_record.slice.break_after,
                                            GridFragmentBreak::RowBoundary
                                                | GridFragmentBreak::ForcedRowBoundary
                                        ),
                                    },
                                ),
                            );
                        }
                    }
                }
                fragment
                    .append_primitives_in_band(PaintBand::Outline, own_outline_primitives.clone());
            }
            if !fragment.is_empty() {
                let fragment = if grid_spanned_pages {
                    let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
                        .with_source_order(self.next_paint_source_order());
                    PaintFragment::from_stacking_context_in_band(PaintBand::InFlowBlock, context)
                } else {
                    fragment
                };
                if page_index < self.pages.len() {
                    self.pages[page_index]
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                } else {
                    self.current_page
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                }
            }
        }
        self.cursor_y -= style.margin.bottom;
        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y();
        }
        self.apply_forced_break_after_box_in(fragmentainer_kind, style);
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
        let mut used_style =
            GridUsedStyle::from_normalized(self.style_with_current_viewport_lengths(style));
        if layout_containment_applies_to_element(element, &used_style)
            || paint_containment_applies_to_element(element, &used_style)
        {
            used_style.grid_template_rows.resolve_contained_subgrid();
            used_style.grid_template_columns.resolve_contained_subgrid();
        }
        let box_metrics = intrinsic_box_metrics(&used_style);
        used_style.margin = box_metrics.margin.to_css_edges();
        used_style.padding = box_metrics.padding.to_css_edges();
        let available_width = (self.content_right
            - self.content_left
            - box_metrics.margin.left.points()
            - box_metrics.margin.right.points())
        .max(used_style.font_size);
        let style = &used_style;
        let horizontal_extras = box_metrics.horizontal_non_content_length().points();
        let vertical_extras = box_metrics.vertical_non_content_length().points();
        let (children, _) = grid_child_lists_from_boxes(child_boxes);
        let children = self.prepare_grid_children(children);

        let (min_width, max_width) = if intrinsic_physical_width_is_contained(style) {
            self.size_contained_grid_intrinsic_widths(style)
        } else {
            self.estimate_grid_intrinsic_widths(
                element,
                style,
                stylesheets,
                available_width,
                Some(child_boxes),
            )
        };
        let requested_content_width = crate::layout::intrinsic::content_box_width_from_intrinsic(
            style,
            layout_pt(available_width),
            non_content_pt(horizontal_extras),
            content_box_pt(min_width),
            content_box_pt(max_width),
            crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
        );
        let content_width = constrain_content_width(
            style,
            requested_content_width,
            PercentageBasis::definite(layout_pt(available_width.max(0.0))),
        )
        .points();
        let definite_content_height = used_content_box_height_or_auto(
            style,
            layout_pt(style.line_height.max(1.0)),
            non_content_pt(vertical_extras),
        )
        .map(SemanticLengthExt::points)
        .map(|height| {
            PhysicalContentHeight::new(constrain_content_height(
                style,
                content_box_pt(height),
                PercentageBasis::definite(layout_pt(available_width)),
            ))
        });
        let size_contained_content_height = if style.contain.size {
            let empty_grid_height = self
                .compute_grid_layout(
                    style,
                    &[],
                    stylesheets,
                    PhysicalContentWidth::new(content_box_pt(content_width)),
                    definite_content_height,
                    GridLayoutPurpose::IntrinsicProbe,
                )
                .map(|layout| layout.height)
                .unwrap_or_else(|| PhysicalContentHeight::new(content_box_pt(0.0)));
            Some(definite_content_height.unwrap_or_else(|| {
                PhysicalContentHeight::new(constrain_content_height(
                    style,
                    empty_grid_height.content_box_length(),
                    PercentageBasis::definite(layout_pt(available_width)),
                ))
            }))
        } else {
            None
        };
        let grid_layout = self.compute_grid_layout(
            style,
            &children,
            stylesheets,
            PhysicalContentWidth::new(content_box_pt(content_width)),
            size_contained_content_height.or(definite_content_height),
            GridLayoutPurpose::IntrinsicProbe,
        );
        let content_height = size_contained_content_height
            .map(PhysicalContentHeight::points)
            .unwrap_or_else(|| {
                let measured = grid_layout
                    .as_ref()
                    .map(|layout| layout.height.points())
                    .unwrap_or(style.line_height)
                    .max(style.line_height);
                constrain_content_height(
                    style,
                    content_box_pt(measured),
                    PercentageBasis::definite(layout_pt(available_width)),
                )
                .points()
            });
        let border_box_height = content_height + vertical_extras;
        let baseline_offset = grid_layout
            .as_ref()
            .and_then(|layout| layout.first_baseline)
            .map(|baseline| box_metrics.border.top.points() + style.padding.top + baseline)
            .unwrap_or(border_box_height);

        InlineAtom::new(
            InlineAtomContent::Svg { asset: None },
            style.as_computed().clone(),
            None,
            InlineSize::new(
                content_width + horizontal_extras + style.margin.left + style.margin.right,
                border_box_height + style.margin.top + style.margin.bottom,
            ),
            baseline_offset,
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
        let mut used_style =
            GridUsedStyle::from_normalized(self.style_with_current_viewport_lengths(style));
        if layout_containment_applies_to_element(element, &used_style)
            || paint_containment_applies_to_element(element, &used_style)
        {
            used_style.grid_template_rows.resolve_contained_subgrid();
            used_style.grid_template_columns.resolve_contained_subgrid();
        }
        let available_width = (self.content_right
            - self.content_left
            - used_style.margin.left
            - used_style.margin.right)
            .max(used_style.font_size);
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_width)),
        );
        let style = &used_style;
        let border_widths = box_metrics.border.to_css_edges();
        let horizontal_extras = box_metrics.horizontal_non_content_length().points();
        let vertical_extras = box_metrics.vertical_non_content_length().points();
        let (children, positioned_children) = grid_child_lists_from_boxes(child_boxes);
        let children = self.prepare_grid_children(children);
        let positioned_children = self.prepare_grid_children(positioned_children);

        let (min_width, max_width) = if intrinsic_physical_width_is_contained(style) {
            self.size_contained_grid_intrinsic_widths(style)
        } else {
            self.estimate_grid_intrinsic_widths(
                element,
                style,
                stylesheets,
                available_width,
                Some(child_boxes),
            )
        };
        let requested_content_width = crate::layout::intrinsic::content_box_width_from_intrinsic(
            style,
            layout_pt(available_width),
            non_content_pt(horizontal_extras),
            content_box_pt(min_width),
            content_box_pt(max_width),
            crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
        );
        let content_width = constrain_content_width(
            style,
            requested_content_width,
            PercentageBasis::definite(layout_pt(available_width.max(0.0))),
        )
        .points();
        let definite_content_height = used_content_box_height_or_auto(
            style,
            layout_pt(style.line_height.max(1.0)),
            non_content_pt(vertical_extras),
        )
        .map(SemanticLengthExt::points)
        .map(|height| {
            PhysicalContentHeight::new(constrain_content_height(
                style,
                content_box_pt(height),
                PercentageBasis::definite(layout_pt(available_width)),
            ))
        });
        let size_contained_content_height = if style.contain.size {
            let empty_grid_height = self
                .compute_grid_layout(
                    style,
                    &[],
                    stylesheets,
                    PhysicalContentWidth::new(content_box_pt(content_width)),
                    definite_content_height,
                    GridLayoutPurpose::IntrinsicProbe,
                )
                .map(|layout| layout.height)
                .unwrap_or_else(|| PhysicalContentHeight::new(content_box_pt(0.0)));
            Some(definite_content_height.unwrap_or_else(|| {
                PhysicalContentHeight::new(constrain_content_height(
                    style,
                    empty_grid_height.content_box_length(),
                    PercentageBasis::definite(layout_pt(available_width)),
                ))
            }))
        } else {
            None
        };
        let grid_content_height_basis = size_contained_content_height.or(definite_content_height);
        let Some(grid_layout) = self.compute_grid_layout(
            style,
            &children,
            stylesheets,
            PhysicalContentWidth::new(content_box_pt(content_width)),
            grid_content_height_basis,
            GridLayoutPurpose::FinalLayout,
        ) else {
            return self.inline_fragment_atom_for_children(
                None,
                style,
                child_boxes,
                stylesheets,
                baseline_shift,
                link_target,
            );
        };

        let total_content_height = size_contained_content_height
            .map(PhysicalContentHeight::points)
            .unwrap_or_else(|| {
                constrain_content_height(
                    style,
                    grid_layout.height.content_box_length(),
                    PercentageBasis::definite(layout_pt(available_width)),
                )
                .points()
            });
        let border_box_height = total_content_height + vertical_extras;
        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        // Inline-grid uses a temporary page while materializing its atom, but
        // viewport-fixed descendants belong to the outer document and must
        // survive that local builder snapshot.
        // <https://www.w3.org/TR/css-position-3/#fixed-pos>
        let fixed_layer_start = self.fixed_layers.len();
        let top = 10_000.0;
        let content_top = top - border_widths.top - style.padding.top;
        let inner_x = border_widths.left + style.padding.left;
        let inner_width = content_width.max(0.0);
        self.current_page = Page::new(content_width + horizontal_extras, top);
        self.content_left = inner_x;
        self.content_right = inner_x + inner_width;
        self.cursor_y = content_top;
        self.truncate_page_start_margins = false;

        let positioning_containing_block_mode =
            PositionedContainingBlockMode::for_element(element, style);
        let establishes_positioning_containing_block = positioning_containing_block_mode.is_some();
        let positioned_containing_block_scope =
            if let Some(mode) = positioning_containing_block_mode {
                let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
                    border_widths.left,
                    top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                ));
                Some(self.push_positioned_containing_block(mode, containing_block))
            } else {
                None
            };
        let pushed_grid_positioning_scope = if let Some(containing_block) =
            positioned_containing_block_scope
                .is_some()
                .then(|| self.containing_blocks.last().cloned())
                .flatten()
        {
            self.grid_positioning_scopes.push(GridPositioningScope::new(
                style,
                GridPositioningGeometry {
                    inner_x,
                    inner_width: PhysicalContentWidth::new(content_box_pt(inner_width)),
                    content_top,
                    content_height: PhysicalContentHeight::new(content_box_pt(
                        total_content_height,
                    )),
                    column_line_offsets: &grid_layout.column_line_offsets,
                    row_line_offsets: &grid_layout.row_line_offsets,
                },
                containing_block,
            ));
            true
        } else {
            false
        };

        self.push_page_name_scope_suppression();
        let suppresses_descendant_fragmentation = style.contain.size;
        if suppresses_descendant_fragmentation {
            self.fragmentation_suppression_depth += 1;
        }
        self.push_float_context();
        for (child, item) in children.iter().zip(&grid_layout.items) {
            self.replay_grid_item_with_resolved_axes(
                style,
                &grid_layout,
                child,
                item,
                stylesheets,
                inner_x,
                PageTopBlockPosition::new(content_top),
            );
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
                    inner_width: PhysicalContentWidth::new(content_box_pt(inner_width)),
                    content_top,
                    definite_content_height: grid_content_height_basis,
                    content_height: PhysicalContentHeight::new(content_box_pt(
                        total_content_height,
                    )),
                    column_line_offsets: &grid_layout.column_line_offsets,
                    row_line_offsets: &grid_layout.row_line_offsets,
                    establishes_positioning_containing_block,
                },
            );
        }
        if pushed_grid_positioning_scope {
            self.grid_positioning_scopes.pop();
        }
        if suppresses_descendant_fragmentation {
            self.fragmentation_suppression_depth -= 1;
        }
        self.pop_page_name_scope_suppression();

        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        let border_bottom = top - border_box_height;
        self.flush_positioned_layers_since(positioned_layer_start);
        let mut fragment = self.current_page.paint_fragment();
        if style.visibility == Visibility::Visible {
            fragment.prepend_primitives_in_band(
                PaintBand::BackgroundBorder,
                grid_gap_decoration_primitives(
                    style,
                    GapDecorationContainer::new(
                        inner_x,
                        content_top,
                        grid_used_track_extent(
                            &grid_layout.column_line_offsets,
                            &grid_layout.items,
                            GridAxis::Column,
                            inner_width,
                        ),
                        grid_used_track_extent(
                            &grid_layout.row_line_offsets,
                            &grid_layout.items,
                            GridAxis::Row,
                            total_content_height,
                        ),
                    ),
                    &grid_layout
                        .items
                        .iter()
                        .map(GridItemLayout::gap_decoration_item)
                        .collect::<Vec<_>>(),
                    &grid_layout.gap_gutters,
                ),
            );
        }
        let fragment = fragment.translated(PaintTranslation::new(0.0, -border_bottom));
        let baseline_offset = (!style.contain.layout)
            .then_some(grid_layout.first_baseline)
            .flatten()
            .map(|baseline| border_widths.top + style.padding.top + baseline)
            .or_else(|| {
                fragment
                    .first_line_y()
                    .map(|line_y| (border_box_height - line_y).max(0.0))
            })
            .unwrap_or(border_box_height);
        let fixed_layers = self.fixed_layers.split_off(fixed_layer_start);
        self.restore(snapshot);
        self.fixed_layers.extend(fixed_layers);

        InlineAtom::new(
            InlineAtomContent::InlineFragment {
                fragment: Box::new(fragment),
                table_cell_context: None,
            },
            style.as_computed().clone(),
            None,
            InlineSize::new(
                content_width + horizontal_extras + style.margin.left + style.margin.right,
                border_box_height + style.margin.top + style.margin.bottom,
            ),
            baseline_offset,
            baseline_shift,
            link_target,
            None,
        )
    }
}

pub(in crate::layout::grid) struct GridGapFragmentProjection<'a> {
    pub(in crate::layout::grid) style: &'a ComputedStyle,
    pub(in crate::layout::grid) content_origin: PageTopPoint,
    pub(in crate::layout::grid) inner_width: PhysicalContentWidth,
    pub(in crate::layout::grid) total_content_height: f32,
    pub(in crate::layout::grid) items: &'a [GapDecorationItem],
    pub(in crate::layout::grid) gutters: &'a GapDecorationGridGutters,
    pub(in crate::layout::grid) source_block_start: GridFragmentBlockOffset,
    pub(in crate::layout::grid) source_block_end: GridFragmentBlockOffset,
    pub(in crate::layout::grid) ends_at_fragment_break: bool,
}

pub(in crate::layout::grid) fn grid_gap_decoration_primitives_for_page(
    projection: GridGapFragmentProjection<'_>,
) -> Vec<PaintPrimitive> {
    let block_start = projection
        .source_block_start
        .points()
        .clamp(0.0, projection.total_content_height);
    let block_end = projection
        .source_block_end
        .points()
        .clamp(block_start, projection.total_content_height);
    let fragment_height = (block_end - block_start).max(0.0);
    if fragment_height <= 0.01 {
        return Vec::new();
    }

    // Segment rule geometry before fragment projection. In particular,
    // `rule-break` junctions must see neighboring tracks/items in their
    // source coordinate system; clipping those inputs first changes a
    // junction into a cap at the fragment boundary.
    let source_segments = grid_gap_rule_paint_segments(
        projection.style,
        GapDecorationContainer::new(
            projection.content_origin.x(),
            projection.content_origin.top_y(),
            projection.inner_width.points(),
            projection.total_content_height,
        ),
        projection.items,
        projection.gutters,
    );
    let page_container = GapDecorationContainer::new(
        projection.content_origin.x(),
        projection.content_origin.top_y(),
        projection.inner_width.points(),
        fragment_height,
    );
    source_segments
        .into_iter()
        .filter_map(|segment| {
            let crossing_gaps = match segment.kind {
                GapRuleAxisKind::Column => &projection.gutters.rows,
                GapRuleAxisKind::Row => &projection.gutters.columns,
            };
            project_grid_gap_rule_segment_to_block_range(
                segment,
                block_start,
                block_end,
                projection.ends_at_fragment_break,
                crossing_gaps,
            )
        })
        .flat_map(|segment| {
            grid_gap_rule_segment_primitives(projection.style, page_container, segment)
        })
        .collect()
}

/// Intersects a rule's centerline, rather than its already-expanded painted
/// area, with a committed grid fragment source range.
pub(in crate::layout::grid) fn project_grid_gap_rule_segment_to_block_range(
    mut rule_segment: GapRulePaintSegment,
    block_start: f32,
    block_end: f32,
    ends_at_fragment_break: bool,
    crossing_gaps: &[GapDecorationGutter],
) -> Option<GapRulePaintSegment> {
    match rule_segment.kind {
        GapRuleAxisKind::Column => {
            let source_start = rule_segment.segment.start.position;
            let source_end = rule_segment.segment.end.position;
            let fragment_content_start =
                grid_fragment_content_start_after_removed_cross_gap(block_start, crossing_gaps);
            let start = source_start.max(fragment_content_start);
            let end = source_end.min(block_end);
            if end <= start + GAP_RULE_EPSILON {
                return None;
            }
            let end = grid_fragment_terminal_rule_cap_end(
                source_end,
                end,
                block_end,
                rule_segment.width,
                ends_at_fragment_break,
                crossing_gaps,
            )
            .unwrap_or(end);
            let start = start - fragment_content_start;
            let end = end - fragment_content_start;
            if end <= start + GAP_RULE_EPSILON {
                return None;
            }
            rule_segment.segment.start = GapRuleEndpoint::cap(start);
            rule_segment.segment.end = GapRuleEndpoint::cap(end);
            Some(rule_segment)
        }
        GapRuleAxisKind::Row => {
            let center = rule_segment.gap.center();
            if center < block_start - GAP_RULE_EPSILON || center > block_end + GAP_RULE_EPSILON {
                return None;
            }
            if (rule_segment.gap.start - block_start).abs() <= GAP_RULE_EPSILON {
                return None;
            }
            let fragment_content_start =
                grid_fragment_content_start_after_removed_cross_gap(block_start, crossing_gaps);
            rule_segment.gap.start -= fragment_content_start;
            rule_segment.gap.end -= fragment_content_start;
            Some(rule_segment)
        }
    }
}

/// A cross gap that starts at a fragmentation break disappears from the next
/// fragment. Its following source content is rebased to the fragment-local
/// origin before rule geometry is expanded.
/// <https://drafts.csswg.org/css-gaps-1/#fragmentation>
fn grid_fragment_content_start_after_removed_cross_gap(
    block_start: f32,
    crossing_gaps: &[GapDecorationGutter],
) -> f32 {
    crossing_gaps
        .iter()
        .find(|gap| (gap.span.start - block_start).abs() <= GAP_RULE_EPSILON)
        .map_or(block_start, |gap| gap.span.end)
}

/// Returns the painted terminal cap at a fragmented source boundary. The
/// fragment's semantic segment is expanded only after source-range projection,
/// preserving the full square cap that belongs to its final painted endpoint.
/// <https://drafts.csswg.org/css-gaps-1/#fragmentation>
fn grid_fragment_terminal_rule_cap_end(
    source_segment_end: f32,
    projected_segment_end: f32,
    fragment_boundary: f32,
    rule_width: GapRuleWidth,
    ends_at_fragment_break: bool,
    crossing_gaps: &[GapDecorationGutter],
) -> Option<f32> {
    let source_segment_crosses_boundary =
        source_segment_end > projected_segment_end + GAP_RULE_EPSILON;
    let boundary_splits_cross_gap = crossing_gaps
        .iter()
        .any(|gap| (gap.span.start - projected_segment_end).abs() <= GAP_RULE_EPSILON);
    (ends_at_fragment_break
        && (projected_segment_end - fragment_boundary).abs() <= GAP_RULE_EPSILON
        && (source_segment_crosses_boundary || boundary_splits_cross_gap))
        .then(|| rule_width.extend_axis_position(projected_segment_end))
}

fn grid_fragment_source_range_from_bounds(
    fragment_bounds: PaintClip,
    content_top: PageTopBlockPosition,
    total_content_height: f32,
) -> (GridFragmentBlockOffset, GridFragmentBlockOffset) {
    let fragment_top = fragment_bounds.y() + fragment_bounds.height();
    let block_start = (content_top.points() - fragment_top).clamp(0.0, total_content_height);
    let block_end =
        (content_top.points() - fragment_bounds.y()).clamp(block_start, total_content_height);
    (
        GridFragmentBlockOffset::new(block_start),
        GridFragmentBlockOffset::new(block_end),
    )
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct GridLayout {
    pub(in crate::layout) height: PhysicalContentHeight,
    pub(in crate::layout) first_baseline: Option<f32>,
    pub(in crate::layout) last_baseline: Option<f32>,
    pub(in crate::layout) items: Vec<GridItemLayout>,
    pub(in crate::layout) gap_gutters: GapDecorationGridGutters,
    pub(in crate::layout) column_line_offsets: Vec<f32>,
    pub(in crate::layout) row_line_offsets: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::grid) enum GridLayoutPurpose {
    FinalLayout,
    IntrinsicProbe,
}

/// Taffy leaves used by one Grid sizing pass.
///
/// A contribution proxy models normal-flow content inside an inherited
/// subgrid axis. It participates in track sizing but never maps to a returned
/// grid item or a paint/replay record.
#[derive(Debug, Clone)]
enum GridTaffyLeaf {
    Item(GridItemEstimate),
    Contribution(GridItemEstimate),
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct GridItemLayout {
    rect: GridRect,
    pub(in crate::layout::grid) area: Option<GridItemArea>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GridItemArea {
    pub(in crate::layout) row_start: u16,
    pub(in crate::layout) row_end: u16,
    pub(in crate::layout) column_start: u16,
    pub(in crate::layout) column_end: u16,
}

/// Return the used extent of a resolved grid axis for gap decoration painting.
///
/// Grid gap rules cover the resolved grid tracks, not free space remaining in
/// the grid container after fixed tracks have been laid out. Taffy's final
/// line offsets preserve that distinction, including content distribution and
/// implicit tracks; an empty offset record retains the container fallback.
/// <https://www.w3.org/TR/css-grid-1/#grid-definition> and
/// <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>
fn grid_used_track_extent(
    line_offsets: &[f32],
    items: &[GridItemLayout],
    axis: GridAxis,
    fallback: f32,
) -> f32 {
    let line_extent = line_offsets.last().cloned().unwrap_or(0.0);
    let item_extent = items
        .iter()
        .filter(|item| item.area.is_some())
        .map(|item| match axis {
            GridAxis::Column => item.rect.max_x(),
            GridAxis::Row => item.rect.max_y(),
        })
        .fold(0.0_f32, f32::max);
    line_extent.max(item_extent).min(fallback).max(0.0)
}

impl GridItemLayout {
    pub(in crate::layout::grid) fn new(rect: GridRect, area: Option<GridItemArea>) -> Self {
        Self { rect, area }
    }

    pub(in crate::layout::grid) fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub(in crate::layout::grid) fn y(&self) -> f32 {
        self.rect.origin.y
    }

    /// Return Taffy's physical border-box geometry for this placed item.
    ///
    /// This is deliberately not the container's `PhysicalContentWidth`:
    /// converting it to a child content-box width needs the child's logical
    /// percentage basis, particularly in vertical writing modes.
    pub(in crate::layout::grid) fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub(in crate::layout::grid) fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(in crate::layout::grid) fn axis_start(&self, axis: GridAxis) -> f32 {
        match axis {
            GridAxis::Column => self.x(),
            GridAxis::Row => self.y(),
        }
    }

    pub(in crate::layout::grid) fn axis_size(&self, axis: GridAxis) -> f32 {
        match axis {
            GridAxis::Column => self.width(),
            GridAxis::Row => self.height(),
        }
    }

    pub(in crate::layout::grid) fn set_axis_geometry(
        &mut self,
        axis: GridAxis,
        start: f32,
        size: f32,
    ) {
        match axis {
            GridAxis::Column => {
                self.rect.origin.x = start;
                self.rect.size.width = size.max(0.0);
            }
            GridAxis::Row => {
                self.rect.origin.y = start;
                self.rect.size.height = size.max(0.0);
            }
        }
    }

    pub(in crate::layout::grid) fn page_top_rect(
        &self,
        container_origin: PageTopPoint,
    ) -> PageTopRect {
        grid_rect_to_page_top_rect(self.rect, container_origin)
    }

    pub(in crate::layout::grid) fn with_block_slice(
        &self,
        block_start: f32,
        block_end: f32,
    ) -> Self {
        let mut visible = self.clone();
        visible.set_axis_geometry(
            GridAxis::Row,
            block_start,
            (block_end - block_start).max(0.0),
        );
        visible
    }

    pub(in crate::layout::grid) fn gap_decoration_item(&self) -> GapDecorationItem {
        let rect = GapDecorationRect::new(
            GapDecorationPoint::new(self.rect.origin.x, self.rect.origin.y),
            GapDecorationSize::new(self.rect.size.width, self.rect.size.height),
        );
        if let Some(area) = self.area {
            GapDecorationItem::from_rect_with_grid_area(
                rect,
                GapDecorationGridArea {
                    row_start: area.row_start,
                    row_end: area.row_end,
                    column_start: area.column_start,
                    column_end: area.column_end,
                },
            )
        } else {
            GapDecorationItem::from_rect(rect)
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
    ///
    /// `width` is the grid container's physical CSS content-box width.
    pub(in crate::layout::grid) fn compute_grid_layout(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &[Stylesheet],
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
        purpose: GridLayoutPurpose,
    ) -> Option<GridLayout> {
        // A direct subgrid replay installs this one-shot context immediately
        // before entering its formatting context. Consume it here rather than
        // leaving it visible to unrelated nested grids.
        let subgrid_context = self.take_resolved_subgrid_context();
        let preliminary_layout = self.compute_grid_layout_pass(
            style,
            children,
            stylesheets,
            subgrid_context.as_ref(),
            &[],
            GridLayoutPassConfig {
                width,
                root_height: height,
                item_height_basis: grid_percentage_basis(
                    height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                row_gap_basis: grid_percentage_basis(
                    height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                reported_height: None,
            },
        )?;
        let contributions = if purpose == GridLayoutPurpose::FinalLayout
            && subgrid_context.is_none()
        {
            self.collect_subgrid_contributions(style, children, stylesheets, &preliminary_layout)
        } else {
            Vec::new()
        };
        let intrinsic_layout = if contributions.is_empty() {
            preliminary_layout
        } else {
            self.compute_grid_layout_pass(
                style,
                children,
                stylesheets,
                subgrid_context.as_ref(),
                &contributions,
                GridLayoutPassConfig {
                    width,
                    root_height: height,
                    item_height_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    row_gap_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: None,
                },
            )?
        };
        // An auto-height grid can still provide a definite physical block
        // size to its items when its physical row tracks are explicitly fixed.
        // Run the item-placement phase again with that used grid height so
        // percentage heights and aspect-ratio transfers resolve against the
        // grid area, while intrinsically sized tracks continue to treat those
        // percentages as cyclic and unresolved.
        // <https://www.w3.org/TR/css-grid-1/#definite-sizes> and
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        if height.is_none()
            && purpose == GridLayoutPurpose::FinalLayout
            && grid_has_fixed_physical_block_tracks(style)
        {
            return self.compute_grid_layout_pass(
                style,
                children,
                stylesheets,
                subgrid_context.as_ref(),
                &contributions,
                GridLayoutPassConfig {
                    width,
                    root_height: Some(intrinsic_layout.height),
                    item_height_basis: grid_percentage_basis(
                        Some(intrinsic_layout.height.content_box_length()),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    row_gap_basis: grid_percentage_basis(
                        Some(intrinsic_layout.height.content_box_length()),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: Some(intrinsic_layout.height),
                },
            );
        }
        if height.is_none()
            && purpose == GridLayoutPurpose::FinalLayout
            && grid_gap_resolves_differently_with_basis(
                style.row_gap.clone(),
                intrinsic_layout.height,
            )
        {
            return self.compute_grid_layout_pass(
                style,
                children,
                stylesheets,
                subgrid_context.as_ref(),
                &contributions,
                GridLayoutPassConfig {
                    width,
                    root_height: Some(intrinsic_layout.height),
                    item_height_basis: PercentageBasis::indefinite(),
                    row_gap_basis: grid_percentage_basis(
                        Some(intrinsic_layout.height.content_box_length()),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: Some(intrinsic_layout.height),
                },
            );
        }
        if style.display.is_grid_lanes() {
            return Some(self.apply_grid_lanes_placement(
                style,
                children,
                stylesheets,
                width,
                intrinsic_layout,
                subgrid_context.as_ref(),
            ));
        }
        Some(intrinsic_layout)
    }

    /// Run the existing subgrid contribution probe with a placed item's
    /// physical border-box width.
    ///
    /// This is intentionally a narrow legacy boundary. Taffy's placed-item
    /// width is not a CSS content-box width, but converting it correctly needs
    /// the child's logical percentage basis (which may be the physical height
    /// in vertical writing modes). Keep the historical probe arithmetic here
    /// until that conversion has a dedicated representation.
    pub(in crate::layout::grid) fn compute_grid_layout_for_subgrid_contribution_probe(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &[Stylesheet],
        placed_item_border_box_width: f32,
        height: Option<f32>,
        purpose: GridLayoutPurpose,
    ) -> Option<GridLayout> {
        self.compute_grid_layout(
            style,
            children,
            stylesheets,
            PhysicalContentWidth::new(content_box_pt(placed_item_border_box_width)),
            height.map(|height| PhysicalContentHeight::new(content_box_pt(height))),
            purpose,
        )
    }

    /// Compute one Taffy grid layout pass.
    ///
    /// CSS Box Alignment makes Grid cyclic percentage gaps resolve against zero
    /// for intrinsic size contributions, but against the grid container content
    /// box when laying out contents. Callers can therefore provide a definite
    /// `root_height` for final content layout while keeping `item_height_basis`
    /// indefinite for grid item percentage block sizes:
    /// <https://www.w3.org/TR/css-align-3/#gap-percent> and
    /// <https://www.w3.org/TR/css-grid-1/#grid-sizing>.
    fn compute_grid_layout_pass(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &[Stylesheet],
        subgrid_context: Option<&ResolvedSubgridContext>,
        contributions: &[SubgridContribution],
        config: GridLayoutPassConfig,
    ) -> Option<GridLayout> {
        let content_width = config.width;
        let width = content_width.points();
        let root_height = config.root_height;
        let root_height_points = root_height.map(PhysicalContentHeight::points);
        let contained_subgrid_columns = (style.contain.layout || style.contain.paint)
            && matches!(style.grid_template_columns, css::GridTrackList::None);
        let contained_subgrid_rows = (style.contain.layout || style.contain.paint)
            && matches!(style.grid_template_rows, css::GridTrackList::None);
        let item_height_basis = if contained_subgrid_rows {
            GridPercentageBasis::indefinite()
        } else {
            config.item_height_basis
        };
        let row_gap_basis = config.row_gap_basis;
        // A contained subgrid axis resolves to `none`, leaving its automatic
        // implicit tracks to size from intrinsic contributions. Percentages on
        // those grid items are cyclic during that sizing phase and therefore
        // behave as `auto`; feeding the final stretched grid-area width back
        // here would incorrectly make `width: 100%` grow the implicit track.
        // <https://drafts.csswg.org/css-grid-2/#subgrid-listing> and
        // <https://drafts.csswg.org/css-grid-1/#percentage-sizing>
        let item_width_basis = if contained_subgrid_columns {
            GridPercentageBasis::indefinite()
        } else {
            grid_percentage_basis(
                Some(content_width.content_box_length()),
                GridAvailableSizeSource::ContainerInlineSize,
            )
        };
        // Taffy's grid tracks are physical: columns run along x and rows along
        // y. CSS Grid's columns/rows are logical inline/block axes, so vertical
        // writing modes must swap the templates, auto tracks, placement lines,
        // alignment axes, and gaps at this adapter boundary:
        // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
        // <https://www.w3.org/TR/css-grid-2/#track-sizing>.
        let swaps_physical_grid_axes =
            WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes();
        let physical_column_subgrid = subgrid_context
            .and_then(|context| context.physical_axis(GridAxis::Column, swaps_physical_grid_axes));
        let physical_row_subgrid = subgrid_context
            .and_then(|context| context.physical_axis(GridAxis::Row, swaps_physical_grid_axes));
        let resolved_item_placements = subgrid_context
            .map(|context| context.resolve_item_placements(children, style.grid_auto_flow));
        let mut tree: taffy_layout::TaffyTree<GridTaffyLeaf> = taffy_layout::TaffyTree::new();
        tree.disable_rounding();
        let row_adjustment =
            taffy_startward_implicit_row_adjustment(style, children, root_height_points);
        let column_adjustment =
            taffy_startward_implicit_column_adjustment(style, children, content_width);
        let mut nodes = Vec::with_capacity(children.len());
        let mut estimates = Vec::with_capacity(children.len());
        for (index, child) in children.iter().enumerate() {
            let resolved_placement = resolved_item_placements
                .as_ref()
                .map(|placements| placements[index]);
            let estimate = self.estimate_grid_item_size(
                child,
                stylesheets,
                width,
                item_width_basis,
                item_height_basis,
            );
            // A subgrid has no independent intrinsic contribution in an
            // inherited axis. Its explicitly placed descendants are inserted
            // below as projected proxy leaves after preliminary placement.
            // Retain the original estimate for replay/baseline bookkeeping;
            // only the Taffy sizing leaf is made empty in that logical axis.
            // <https://drafts.csswg.org/css-grid-2/#subgrid-track-sizing>
            let sizing_estimate = grid_item_parent_sizing_estimate(estimate, &child.style);
            // Grid's intrinsic contributions are logical, while Taffy's
            // layout tree is physical. Keep the logical estimate for Grid's
            // own track and baseline calculations, and project only the
            // automatic measurement inputs supplied to Taffy.
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
            let physical_estimate = sizing_estimate.physical_measurements();
            estimates.push(estimate);
            let node = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        display: taffy_layout::Display::Block,
                        box_sizing: taffy_box_sizing(child.style.box_sizing),
                        direction: taffy_direction(child.style.used_direction()),
                        size: taffy_layout::Size {
                            width: taffy_grid_item_dimension(
                                child.style.box_values.width.clone(),
                                item_width_basis,
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: taffy_grid_item_dimension(
                                child.style.box_values.height.clone(),
                                item_height_basis,
                                physical_estimate.min_height,
                                physical_estimate.content_height,
                            ),
                        },
                        // CSS Grid's track-sizing and item-layout phases both
                        // need the preferred ratio: a definite grid-area size
                        // in either axis transfers to the automatic opposite
                        // axis before alignment and intrinsic contribution
                        // resolution.
                        // <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
                        aspect_ratio: child
                            .style
                            .aspect_ratio
                            .preferred_ratio_for_non_replaced(false),
                        min_size: taffy_layout::Size {
                            width: taffy_grid_item_min_dimension(
                                child.style.box_values.min_width.clone(),
                                item_width_basis,
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: taffy_grid_item_min_dimension(
                                child.style.box_values.min_height.clone(),
                                item_height_basis,
                                physical_estimate.min_height,
                                physical_estimate.content_height,
                            ),
                        },
                        max_size: taffy_layout::Size {
                            width: taffy_grid_item_constraint_dimension(
                                child.style.box_values.max_width.clone(),
                                item_width_basis,
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: taffy_grid_item_constraint_dimension(
                                child.style.box_values.max_height.clone(),
                                item_height_basis,
                                physical_estimate.min_height,
                                physical_estimate.content_height,
                            ),
                        },
                        margin: taffy_margin(&child.style),
                        padding: taffy_padding(&child.style),
                        border: taffy_edges(used_border_widths(&child.style)),
                        align_self: if swaps_physical_grid_axes {
                            taffy_effective_grid_justify_self(&child.style, style)
                        } else {
                            taffy_effective_grid_align_self(&child.style, style)
                        },
                        justify_self: if swaps_physical_grid_axes {
                            taffy_effective_grid_align_self(&child.style, style)
                        } else {
                            taffy_effective_grid_justify_self(&child.style, style)
                        },
                        grid_row: if swaps_physical_grid_axes {
                            resolved_placement
                                .and_then(|placement| placement.columns)
                                .map(ResolvedSubgridPlacement::taffy_line)
                                .unwrap_or_else(|| {
                                    physical_row_subgrid.map_or_else(
                                        || {
                                            taffy_grid_line_with_startward_adjustment(
                                                &child.style.grid_column_start,
                                                &child.style.grid_column_end,
                                                &column_adjustment,
                                            )
                                        },
                                        |axis| {
                                            axis.clamped_taffy_line(
                                                &child.style.grid_column_start,
                                                &child.style.grid_column_end,
                                            )
                                        },
                                    )
                                })
                        } else {
                            resolved_placement
                                .and_then(|placement| placement.rows)
                                .map(ResolvedSubgridPlacement::taffy_line)
                                .unwrap_or_else(|| {
                                    physical_row_subgrid.map_or_else(
                                        || {
                                            taffy_grid_line_with_startward_adjustment(
                                                &child.style.grid_row_start,
                                                &child.style.grid_row_end,
                                                &row_adjustment,
                                            )
                                        },
                                        |axis| {
                                            axis.clamped_taffy_line(
                                                &child.style.grid_row_start,
                                                &child.style.grid_row_end,
                                            )
                                        },
                                    )
                                })
                        },
                        grid_column: if swaps_physical_grid_axes {
                            resolved_placement
                                .and_then(|placement| placement.rows)
                                .map(ResolvedSubgridPlacement::taffy_line)
                                .unwrap_or_else(|| {
                                    physical_column_subgrid.map_or_else(
                                        || {
                                            taffy_grid_line_with_startward_adjustment(
                                                &child.style.grid_row_start,
                                                &child.style.grid_row_end,
                                                &row_adjustment,
                                            )
                                        },
                                        |axis| {
                                            axis.clamped_taffy_line(
                                                &child.style.grid_row_start,
                                                &child.style.grid_row_end,
                                            )
                                        },
                                    )
                                })
                        } else {
                            resolved_placement
                                .and_then(|placement| placement.columns)
                                .map(ResolvedSubgridPlacement::taffy_line)
                                .unwrap_or_else(|| {
                                    physical_column_subgrid.map_or_else(
                                        || {
                                            taffy_grid_line_with_startward_adjustment(
                                                &child.style.grid_column_start,
                                                &child.style.grid_column_end,
                                                &column_adjustment,
                                            )
                                        },
                                        |axis| {
                                            axis.clamped_taffy_line(
                                                &child.style.grid_column_start,
                                                &child.style.grid_column_end,
                                            )
                                        },
                                    )
                                })
                        },
                        ..Default::default()
                    },
                    GridTaffyLeaf::Item(sizing_estimate),
                )
                .ok()?;
            nodes.push(node);
        }
        let mut contribution_nodes = Vec::with_capacity(contributions.len());
        for contribution in contributions {
            // The collector has already selected the inherited logical axes
            // and applied their outer box/gutter edge adjustments. Project it
            // once here, at the same logical-to-physical boundary as real
            // grid items; proxy leaves must never need ad-hoc axis zeroing.
            let estimate = contribution.adjusted_estimate().physical_measurements();
            let node = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        display: taffy_layout::Display::Block,
                        grid_row: taffy_layout::Line {
                            start: taffy_layout::line(
                                i16::try_from(contribution.area.row_start).ok()?,
                            ),
                            end: taffy_layout::line(i16::try_from(contribution.area.row_end).ok()?),
                        },
                        grid_column: taffy_layout::Line {
                            start: taffy_layout::line(
                                i16::try_from(contribution.area.column_start).ok()?,
                            ),
                            end: taffy_layout::line(
                                i16::try_from(contribution.area.column_end).ok()?,
                            ),
                        },
                        ..Default::default()
                    },
                    GridTaffyLeaf::Contribution(estimate),
                )
                .ok()?;
            contribution_nodes.push(node);
        }
        // Taffy's empty grid currently omits otherwise-valid explicit track
        // geometry. CSS Grid still sizes an empty grid from its explicit
        // tracks and gaps, which is particularly observable when size
        // containment removes every real item from the principal sizing pass.
        // A zero-contribution probe item materializes those tracks without
        // entering Quire's returned item list. Auto-fit grids remain genuinely
        // empty so their unoccupied repeated tracks can collapse.
        // <https://www.w3.org/TR/css-grid-1/#explicit-grids>
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        let mut layout_nodes = nodes.clone();
        layout_nodes.extend(contribution_nodes);
        if layout_nodes.is_empty()
            && !grid_track_list_has_auto_fit(&style.grid_template_columns)
            && !grid_track_list_has_auto_fit(&style.grid_template_rows)
        {
            let zero = taffy_layout::Dimension::length(0.0);
            let placeholder = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        size: taffy_layout::Size {
                            width: zero,
                            height: zero,
                        },
                        min_size: taffy_layout::Size {
                            width: zero,
                            height: zero,
                        },
                        max_size: taffy_layout::Size {
                            width: zero,
                            height: zero,
                        },
                        ..Default::default()
                    },
                    GridTaffyLeaf::Contribution(GridItemEstimate::fixed(0.0, 0.0)),
                )
                .ok()?;
            layout_nodes.push(placeholder);
        }
        let root = tree
            .new_with_children(
                taffy_layout::Style {
                    display: taffy_layout::Display::Grid,
                    box_sizing: taffy_layout::BoxSizing::BorderBox,
                    direction: taffy_direction(style.used_direction()),
                    size: taffy_layout::Size {
                        width: taffy_layout::Dimension::length(width),
                        height: root_height_points
                            .map(taffy_layout::Dimension::length)
                            .unwrap_or_else(taffy_layout::Dimension::auto),
                    },
                    min_size: taffy_layout::Size {
                        width: taffy_dimension(style.box_values.min_width.clone()),
                        height: taffy_dimension(style.box_values.min_height.clone()),
                    },
                    max_size: taffy_layout::Size {
                        width: taffy_dimension(style.box_values.max_width.clone()),
                        height: taffy_dimension(style.box_values.max_height.clone()),
                    },
                    grid_template_columns: physical_column_subgrid
                        .map(ResolvedSubgridAxis::taffy_tracks)
                        .unwrap_or_else(|| {
                            if swaps_physical_grid_axes {
                                taffy_grid_template_rows_with_startward_adjustment(
                                    style,
                                    &row_adjustment,
                                )
                            } else {
                                taffy_grid_template_columns_with_startward_adjustment(
                                    style,
                                    &column_adjustment,
                                )
                            }
                        }),
                    grid_template_rows: physical_row_subgrid
                        .map(ResolvedSubgridAxis::taffy_tracks)
                        .unwrap_or_else(|| {
                            if swaps_physical_grid_axes {
                                taffy_grid_template_columns_with_startward_adjustment(
                                    style,
                                    &column_adjustment,
                                )
                            } else {
                                taffy_grid_template_rows_with_startward_adjustment(
                                    style,
                                    &row_adjustment,
                                )
                            }
                        }),
                    grid_template_areas: taffy_grid_template_areas_with_startward_adjustment(
                        &style.grid_template_areas,
                        &row_adjustment,
                        &column_adjustment,
                    ),
                    grid_template_column_names: physical_column_subgrid
                        .map(|axis| axis.line_names().to_vec())
                        .unwrap_or_else(|| {
                            taffy_grid_template_column_names_with_startward_adjustment(
                                style,
                                &column_adjustment,
                            )
                        }),
                    grid_template_row_names: physical_row_subgrid
                        .map(|axis| axis.line_names().to_vec())
                        .unwrap_or_else(|| {
                            taffy_grid_template_row_names_with_startward_adjustment(
                                style,
                                &row_adjustment,
                            )
                        }),
                    grid_auto_columns: physical_column_subgrid.map_or_else(
                        || {
                            if swaps_physical_grid_axes {
                                taffy_grid_auto_tracks(&style.grid_auto_rows)
                            } else {
                                taffy_grid_auto_tracks(&style.grid_auto_columns)
                            }
                        },
                        |_| Vec::new(),
                    ),
                    grid_auto_rows: physical_row_subgrid.map_or_else(
                        || {
                            if swaps_physical_grid_axes {
                                taffy_grid_auto_tracks(&style.grid_auto_columns)
                            } else {
                                taffy_grid_auto_tracks(&style.grid_auto_rows)
                            }
                        },
                        |_| Vec::new(),
                    ),
                    grid_auto_flow: taffy_grid_auto_flow(style.grid_auto_flow),
                    justify_content: Some(if swaps_physical_grid_axes {
                        taffy_grid_align_content(style.align_content)
                    } else {
                        taffy_grid_justify_content(style.justify_content)
                    }),
                    align_content: Some(if swaps_physical_grid_axes {
                        taffy_grid_justify_content(style.justify_content)
                    } else {
                        taffy_grid_align_content(style.align_content)
                    }),
                    justify_items: Some(if swaps_physical_grid_axes {
                        taffy_grid_align_items(style.align_items)
                    } else {
                        taffy_grid_justify_items(style.justify_items)
                    }),
                    align_items: Some(if swaps_physical_grid_axes {
                        taffy_grid_justify_items(style.justify_items)
                    } else {
                        taffy_grid_align_items(style.align_items)
                    }),
                    gap: taffy_layout::Size {
                        width: physical_column_subgrid.map_or_else(
                            || {
                                taffy_grid_gap(
                                    if swaps_physical_grid_axes {
                                        style.row_gap.clone()
                                    } else {
                                        style.column_gap.clone()
                                    },
                                    item_width_basis,
                                )
                            },
                            |axis| taffy_layout::LengthPercentage::length(axis.taffy_gap()),
                        ),
                        height: physical_row_subgrid.map_or_else(
                            || {
                                taffy_grid_gap(
                                    if swaps_physical_grid_axes {
                                        style.column_gap.clone()
                                    } else {
                                        style.row_gap.clone()
                                    },
                                    row_gap_basis,
                                )
                            },
                            |axis| taffy_layout::LengthPercentage::length(axis.taffy_gap()),
                        ),
                    },
                    ..Default::default()
                },
                &layout_nodes,
            )
            .ok()?;
        tree.compute_layout_with_measure(
            root,
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::Definite(width),
                height: root_height_points
                    .map(taffy_layout::AvailableSpace::Definite)
                    .unwrap_or(taffy_layout::AvailableSpace::MaxContent),
            },
            |known_dimensions, available_space, _node_id, node_context, _style| {
                let estimate = node_context.map(|context| match context {
                    GridTaffyLeaf::Item(estimate) | GridTaffyLeaf::Contribution(estimate) => {
                        estimate
                    }
                });
                measure_grid_item(known_dimensions, available_space, estimate)
            },
        )
        .ok()?;
        let root_layout = tree.layout(root).ok()?;
        let mut grid_item_areas = Vec::new();
        let mut column_line_offsets = Vec::new();
        let mut row_line_offsets = Vec::new();
        let mut track_corrections = GridTrackLayoutCorrections::default();
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
                let column_correction = startward_auto_fit_track_correction(
                    style,
                    GridAxis::Column,
                    &column_adjustment,
                    &info.columns.sizes,
                    &info.columns.gutters,
                    &grid_item_areas,
                );
                let row_correction = startward_auto_fit_track_correction(
                    style,
                    GridAxis::Row,
                    &row_adjustment,
                    &info.rows.sizes,
                    &info.rows.gutters,
                    &grid_item_areas,
                );
                let column_sizes = column_correction
                    .as_ref()
                    .map(|correction| correction.sizes.as_slice())
                    .unwrap_or(&info.columns.sizes);
                let column_gutters = column_correction
                    .as_ref()
                    .map(|correction| correction.gutters.as_slice())
                    .unwrap_or(&info.columns.gutters);
                let row_sizes = row_correction
                    .as_ref()
                    .map(|correction| correction.sizes.as_slice())
                    .unwrap_or(&info.rows.sizes);
                let row_gutters = row_correction
                    .as_ref()
                    .map(|correction| correction.gutters.as_slice())
                    .unwrap_or(&info.rows.gutters);
                column_line_offsets = column_correction
                    .as_ref()
                    .map(|correction| correction.offsets.clone())
                    .unwrap_or_else(|| {
                        grid_line_offsets_from_track_layout(
                            &info.columns.sizes,
                            &info.columns.gutters,
                        )
                    });
                row_line_offsets = row_correction
                    .as_ref()
                    .map(|correction| correction.offsets.clone())
                    .unwrap_or_else(|| {
                        grid_line_offsets_from_track_layout(&info.rows.sizes, &info.rows.gutters)
                    });
                let gap_gutters = grid_gap_decoration_gutters_from_tracks(
                    column_sizes,
                    column_gutters,
                    row_sizes,
                    row_gutters,
                    style,
                    width,
                    root_layout.size.height,
                );
                track_corrections = GridTrackLayoutCorrections {
                    columns: column_correction,
                    rows: row_correction,
                };
                gap_gutters
            }
            taffy::tree::DetailedLayoutInfo::None => GapDecorationGridGutters::default(),
        };
        // Proxy nodes are appended after real child nodes and intentionally
        // have no corresponding `GridItemLayout`: they influence only Taffy's
        // track sizing, never replay, baselines, gap decoration, or fragment
        // planning.
        debug_assert!(grid_item_areas.len() >= nodes.len());
        let mut items = nodes
            .into_iter()
            .enumerate()
            .map(|(index, node)| {
                let layout = tree.layout(node).ok()?;
                Some(GridItemLayout::new(
                    GridRect::new(
                        GridPoint::new(layout.location.x, layout.location.y),
                        GridSize::new(layout.size.width.max(0.0), layout.size.height.max(0.0)),
                    ),
                    grid_item_areas.get(index).cloned(),
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        debug_assert_eq!(items.len(), children.len());
        apply_startward_auto_fit_track_corrections(
            style,
            content_width,
            root_layout.size.height,
            &track_corrections,
            &mut items,
        );
        apply_grid_self_alignment_corrections(
            style,
            children,
            content_width,
            root_layout.size.height,
            &column_line_offsets,
            &row_line_offsets,
            &mut items,
        );
        apply_grid_aspect_ratio_item_size_corrections(
            style,
            children,
            content_width,
            root_layout.size.height,
            &column_line_offsets,
            &row_line_offsets,
            &mut items,
        );
        if contained_subgrid_columns || contained_subgrid_rows {
            apply_grid_deferred_percentage_item_size_corrections(
                style,
                children,
                content_width,
                root_layout.size.height,
                &column_line_offsets,
                &row_line_offsets,
                &mut items,
            );
        }
        // Taffy reports each item's border-box location after resolving its
        // grid-area margins. Replay suppresses those margins in the child
        // style, but must retain the reported border-box origin unchanged.
        // <https://www.w3.org/TR/css-grid-1/#grid-item-placement>.
        apply_grid_baseline_alignment(style, children, &estimates, &row_line_offsets, &mut items);
        let first_baseline =
            grid_container_baseline(style, children, &estimates, &items, GridBaselineSet::First);
        let last_baseline =
            grid_container_baseline(style, children, &estimates, &items, GridBaselineSet::Last);
        Some(GridLayout {
            height: config.reported_height.unwrap_or_else(|| {
                PhysicalContentHeight::new(content_box_pt(root_layout.size.height))
            }),
            first_baseline,
            last_baseline,
            items,
            gap_gutters,
            column_line_offsets,
            row_line_offsets,
        })
    }
}

/// The direct grid item representing a subgrid is empty in an inherited axis;
/// projected descendant proxy leaves supply that axis's track-sizing
/// contribution. `GridItemEstimate` is logical, so this remains independent
/// of the container's physical writing-mode adapter.
fn grid_item_parent_sizing_estimate(
    mut estimate: GridItemEstimate,
    style: &ComputedStyle,
) -> GridItemEstimate {
    if matches!(
        style.grid_template_columns,
        css::GridTrackList::Subgrid { .. }
    ) {
        estimate.metrics.width = content_box_pt(0.0);
        estimate.metrics.min_width = content_box_pt(0.0);
        estimate.metrics.content_width = content_box_pt(0.0);
    }
    if matches!(style.grid_template_rows, css::GridTrackList::Subgrid { .. }) {
        estimate.metrics.height = content_box_pt(0.0);
        estimate.metrics.min_height = content_box_pt(0.0);
        estimate.metrics.content_height = content_box_pt(0.0);
    }
    estimate
}

/// Whether an auto-sized grid's physical rows have fully fixed explicit sizes.
///
/// A final item-placement pass can use an auto-sized grid's resolved physical
/// height as a percentage basis only when the row tracks did not derive that
/// height from the items. At present this recognizes repeated and non-repeated
/// tracks with identical, percentage-free fixed minimum and maximum breadths;
/// more flexible track functions retain the unresolved cyclic basis.
/// <https://www.w3.org/TR/css-grid-1/#definite-sizes>
fn grid_has_fixed_physical_block_tracks(style: &ComputedStyle) -> bool {
    let physical_rows = if WritingModeAxes::new(style.writing_mode, style.direction)
        .physical_axis(LogicalAxis::Block)
        == PhysicalAxis::Vertical
    {
        &style.grid_template_rows
    } else {
        &style.grid_template_columns
    };
    grid_track_list_has_only_fixed_sizes(physical_rows)
}

fn grid_track_list_has_only_fixed_sizes(tracks: &css::GridTrackList) -> bool {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return false;
    };
    !components.is_empty()
        && components
            .iter()
            .all(grid_track_list_component_has_only_fixed_sizes)
}

fn grid_track_list_component_has_only_fixed_sizes(component: &css::GridTrackListComponent) -> bool {
    match component {
        css::GridTrackListComponent::Track(_, size) => grid_track_size_is_fixed(size),
        css::GridTrackListComponent::Repeat(_, repeat) => {
            matches!(repeat.count, css::GridRepeatCount::Number(_))
                && !repeat.tracks.is_empty()
                && repeat
                    .tracks
                    .iter()
                    .all(grid_track_list_component_has_only_fixed_sizes)
        }
    }
}

fn grid_track_size_is_fixed(size: &css::GridTrackSize) -> bool {
    let (
        css::GridMinTrackBreadth::LengthPercentage(min),
        css::GridMaxTrackBreadth::LengthPercentage(max),
    ) = (size.min.clone(), size.max.clone())
    else {
        return false;
    };
    !min.contains_percentage() && min == max
}

fn grid_gap_resolves_differently_with_basis(
    gap: css::ComputedGap,
    container_size: PhysicalContentHeight,
) -> bool {
    let css::ComputedGap::LengthPercentage(value) = gap else {
        return false;
    };
    let intrinsic = value.length_max_zero().points();
    let used = value
        .used_length_with_percentage_basis(PercentageBasis::definite(
            container_size.content_box_length(),
        ))
        .map(layout_points)
        .unwrap_or(value.length_points())
        .max(0.0);
    (used - intrinsic).abs() > 0.01
}

struct GridLayoutPassConfig {
    width: PhysicalContentWidth,
    root_height: Option<PhysicalContentHeight>,
    item_height_basis: GridPercentageBasis,
    row_gap_basis: GridPercentageBasis,
    reported_height: Option<PhysicalContentHeight>,
}

#[derive(Default)]
struct GridTrackLayoutCorrections {
    columns: Option<GridTrackLayoutCorrection>,
    rows: Option<GridTrackLayoutCorrection>,
}

struct GridTrackLayoutCorrection {
    original_offsets: Vec<f32>,
    offsets: Vec<f32>,
    sizes: Vec<f32>,
    gutters: Vec<f32>,
}

/// Collapse empty frozen `auto-fit` tracks after startward implicit expansion.
///
/// Quire freezes a definite auto-repeat count before prepending startward
/// implicit tracks so the authored repeat count is not recomputed from the
/// enlarged Taffy template. CSS Grid still requires empty `auto-fit` repeated
/// tracks to collapse before content alignment, so Quire mirrors that part of
/// the used-track geometry after Taffy returns detailed track data:
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
fn startward_auto_fit_track_correction(
    style: &ComputedStyle,
    axis: GridAxis,
    adjustment: &StartwardImplicitTrackAdjustment,
    sizes: &[f32],
    gutters: &[f32],
    item_areas: &[GridItemArea],
) -> Option<GridTrackLayoutCorrection> {
    let range = startward_adjusted_auto_fit_track_range(style, axis, adjustment)?;
    if range.end > sizes.len() {
        return None;
    }
    let mut collapsed = vec![false; sizes.len()];
    for track_index in range {
        if !grid_track_has_item(axis, track_index, item_areas) {
            collapsed[track_index] = true;
        }
    }
    if !collapsed.iter().any(|collapsed| *collapsed) {
        return None;
    }
    let mut corrected_sizes = sizes.to_vec();
    for (index, size) in corrected_sizes.iter_mut().enumerate() {
        if collapsed[index] {
            *size = 0.0;
        }
    }
    let mut corrected_gutters = gutters.to_vec();
    for (index, gutter) in corrected_gutters.iter_mut().enumerate() {
        if collapsed.get(index).cloned().unwrap_or(false)
            || collapsed.get(index + 1).cloned().unwrap_or(false)
        {
            *gutter = 0.0;
        }
    }
    Some(GridTrackLayoutCorrection {
        original_offsets: grid_line_offsets_from_track_layout(sizes, gutters),
        offsets: grid_line_offsets_from_track_layout(&corrected_sizes, &corrected_gutters),
        sizes: corrected_sizes,
        gutters: corrected_gutters,
    })
}

fn grid_track_has_item(axis: GridAxis, track_index: usize, item_areas: &[GridItemArea]) -> bool {
    item_areas.iter().any(|area| {
        let (start, end) = match axis {
            GridAxis::Column => (usize::from(area.column_start), usize::from(area.column_end)),
            GridAxis::Row => (usize::from(area.row_start), usize::from(area.row_end)),
        };
        let start = start.saturating_sub(1);
        let end = end.saturating_sub(1);
        start <= track_index && track_index < end
    })
}

fn apply_startward_auto_fit_track_corrections(
    style: &ComputedStyle,
    container_width: PhysicalContentWidth,
    container_height: f32,
    corrections: &GridTrackLayoutCorrections,
    items: &mut [GridItemLayout],
) {
    for item in items {
        let Some(area) = item.area else {
            continue;
        };
        if let Some(correction) = &corrections.columns {
            apply_track_layout_correction_axis(
                correction,
                style.justify_content,
                container_width.points(),
                usize::from(area.column_start).saturating_sub(1),
                usize::from(area.column_end).saturating_sub(1),
                item,
                GridAxis::Column,
            );
        }
        if let Some(correction) = &corrections.rows {
            apply_track_layout_correction_axis(
                correction,
                style.align_content,
                container_height,
                usize::from(area.row_start).saturating_sub(1),
                usize::from(area.row_end).saturating_sub(1),
                item,
                GridAxis::Row,
            );
        }
    }
}

fn apply_track_layout_correction_axis(
    correction: &GridTrackLayoutCorrection,
    content_alignment: css::ContentAlignment,
    container_size: f32,
    start_line: usize,
    end_line: usize,
    item: &mut GridItemLayout,
    axis: GridAxis,
) {
    let Some(original_start) = content_aligned_grid_line_offset(
        content_alignment,
        container_size,
        &correction.original_offsets,
        start_line,
    ) else {
        return;
    };
    let Some(original_end) = content_aligned_grid_line_offset(
        content_alignment,
        container_size,
        &correction.original_offsets,
        end_line,
    ) else {
        return;
    };
    let Some(corrected_start) = content_aligned_grid_line_offset(
        content_alignment,
        container_size,
        &correction.offsets,
        start_line,
    ) else {
        return;
    };
    let Some(corrected_end) = content_aligned_grid_line_offset(
        content_alignment,
        container_size,
        &correction.offsets,
        end_line,
    ) else {
        return;
    };
    let original_area_size = (original_end - original_start).max(0.0);
    let corrected_area_size = (corrected_end - corrected_start).max(0.0);
    let offset_in_area = item.axis_start(axis) - original_start;
    let mut size = item.axis_size(axis);
    if (size - original_area_size).abs() < 0.01 {
        size = corrected_area_size;
    }
    item.set_axis_geometry(axis, corrected_start + offset_in_area, size);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GridBaselineSet {
    First,
    Last,
}

struct GridBaselineAlignmentContext<'a, 'box_tree> {
    container_style: &'a ComputedStyle,
    children: &'a [GridChild<'box_tree>],
    estimates: &'a [GridItemEstimate],
    row_line_offsets: &'a [f32],
}

/// Correct same-page grid self-alignment values that Taffy cannot model directly.
///
/// Taffy's grid alignment model treats `self-start`/`self-end` like
/// `start`/`end`, but CSS Box Alignment resolves them from the alignment
/// subject's own writing mode. In horizontal grid containers this pass maps
/// justify-axis and align-axis `self-start`/`self-end` to the item's relevant
/// physical side; `left`/`right` are physical self-position values for
/// justify-axis alignment and therefore also bypass Taffy's direction-sensitive
/// flex-start/flex-end mapping. The correction uses effective `justify-self`
/// and `align-self` values, so container defaults follow the same path:
/// <https://www.w3.org/TR/css-align-3/#self-alignment> and
/// <https://www.w3.org/TR/css-grid-1/#alignment>.
fn apply_grid_self_alignment_corrections(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    container_width: PhysicalContentWidth,
    container_height: f32,
    column_line_offsets: &[f32],
    row_line_offsets: &[f32],
    items: &mut [GridItemLayout],
) {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return;
    }
    for (index, item) in items.iter_mut().enumerate() {
        let Some(area) = item.area else {
            continue;
        };
        let child_style = &children[index].style;
        let justify_self = effective_grid_justify_self(child_style, container_style);
        if let Some(x) = horizontal_self_alignment_offset(
            justify_self,
            child_style,
            container_style.justify_content,
            container_width,
            area,
            column_line_offsets,
            item.width(),
        ) {
            item.set_axis_geometry(GridAxis::Column, x, item.width());
        }
        let align_self = effective_grid_align_self(child_style, container_style);
        if let Some(y) = vertical_self_alignment_offset(
            align_self,
            child_style,
            container_style.align_content,
            container_height,
            area,
            row_line_offsets,
            item.height(),
        ) {
            item.set_axis_geometry(GridAxis::Row, y, item.height());
        }
    }
}

fn horizontal_self_alignment_offset(
    justify_self: JustifySelf,
    child_style: &ComputedStyle,
    justify_content: css::JustifyContent,
    container_width: PhysicalContentWidth,
    area: GridItemArea,
    column_line_offsets: &[f32],
    item_width: f32,
) -> Option<f32> {
    let side = match justify_self.keyword {
        SelfAlignmentKeyword::Left => Some(PhysicalSide::Left),
        SelfAlignmentKeyword::Right => Some(PhysicalSide::Right),
        SelfAlignmentKeyword::SelfStart => {
            grid_subject_self_start_side(child_style, PhysicalAxis::Horizontal)
        }
        SelfAlignmentKeyword::SelfEnd => {
            grid_subject_self_end_side(child_style, PhysicalAxis::Horizontal)
        }
        _ => None,
    }?;
    self_alignment_offset_for_side(
        side,
        SelfAlignmentAxisContext {
            axis: PhysicalAxis::Horizontal,
            content_alignment: justify_content,
            container_size: container_width.points(),
            line_offsets: column_line_offsets,
            start_line: usize::from(area.column_start).saturating_sub(1),
            end_line: usize::from(area.column_end).saturating_sub(1),
        },
        item_width,
    )
}

fn vertical_self_alignment_offset(
    align_self: AlignSelf,
    child_style: &ComputedStyle,
    align_content: css::AlignContent,
    container_height: f32,
    area: GridItemArea,
    row_line_offsets: &[f32],
    item_height: f32,
) -> Option<f32> {
    let side = match align_self.keyword {
        SelfAlignmentKeyword::SelfStart => {
            grid_subject_self_start_side(child_style, PhysicalAxis::Vertical)
        }
        SelfAlignmentKeyword::SelfEnd => {
            grid_subject_self_end_side(child_style, PhysicalAxis::Vertical)
        }
        _ => None,
    }?;
    self_alignment_offset_for_side(
        side,
        SelfAlignmentAxisContext {
            axis: PhysicalAxis::Vertical,
            content_alignment: align_content,
            container_size: container_height,
            line_offsets: row_line_offsets,
            start_line: usize::from(area.row_start).saturating_sub(1),
            end_line: usize::from(area.row_end).saturating_sub(1),
        },
        item_height,
    )
}

/// Apply the final, grid-area-dependent sizing step for aspect-ratio items.
///
/// Grid track sizing first determines each grid area. An item's automatic
/// size is then resolved against that area and its effective self-alignment:
/// `normal` behaves as start for an aspect-ratio box, while explicit `stretch`
/// supplies the corresponding area dimension. This post-track step keeps that
/// distinction at the Grid-to-layout adapter boundary instead of encoding a
/// grid area's final dimensions as a synthetic CSS declaration.
/// <https://www.w3.org/TR/css-grid-1/#grid-item-sizing> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
fn apply_grid_aspect_ratio_item_size_corrections(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    container_width: PhysicalContentWidth,
    container_height: f32,
    column_line_offsets: &[f32],
    row_line_offsets: &[f32],
    items: &mut [GridItemLayout],
) {
    for (child, item) in children.iter().zip(items) {
        let Some(area) = item.area else {
            continue;
        };
        let child_style = &child.style;
        let Some(_) = child_style
            .aspect_ratio
            .preferred_ratio_for_non_replaced(false)
            .filter(|ratio| *ratio > 0.0 && ratio.is_finite())
        else {
            continue;
        };
        let (
            horizontal_alignment,
            vertical_alignment,
            horizontal_content_alignment,
            vertical_content_alignment,
        ) = if !WritingModeAxes::new(container_style.writing_mode, container_style.direction)
            .swaps_physical_axes()
        {
            (
                effective_grid_justify_self(child_style, container_style),
                effective_grid_align_self(child_style, container_style),
                container_style.justify_content,
                container_style.align_content,
            )
        } else {
            (
                effective_grid_align_self(child_style, container_style),
                effective_grid_justify_self(child_style, container_style),
                container_style.align_content,
                container_style.justify_content,
            )
        };
        let Some(area_x) = content_aligned_grid_line_offset(
            horizontal_content_alignment,
            container_width.points(),
            column_line_offsets,
            usize::from(area.column_start).saturating_sub(1),
        ) else {
            continue;
        };
        let Some(area_right) = content_aligned_grid_line_offset(
            horizontal_content_alignment,
            container_width.points(),
            column_line_offsets,
            usize::from(area.column_end).saturating_sub(1),
        ) else {
            continue;
        };
        let Some(area_y) = content_aligned_grid_line_offset(
            vertical_content_alignment,
            container_height,
            row_line_offsets,
            usize::from(area.row_start).saturating_sub(1),
        ) else {
            continue;
        };
        let Some(area_bottom) = content_aligned_grid_line_offset(
            vertical_content_alignment,
            container_height,
            row_line_offsets,
            usize::from(area.row_end).saturating_sub(1),
        ) else {
            continue;
        };
        let area_width = (area_right - area_x).max(0.0);
        let area_height = (area_bottom - area_y).max(0.0);
        let metrics = used_box_metrics(
            child_style,
            PercentageBasis::definite(layout_pt(container_width.points())),
        );
        let horizontal_non_content = metrics.horizontal_non_content_length();
        let vertical_non_content = metrics.vertical_non_content_length();
        let width_is_auto = child_style.box_values.width.is_auto();
        let height_is_auto = child_style.box_values.height.is_auto();
        let width_stretches =
            width_is_auto && grid_item_aspect_axis_stretches(horizontal_alignment.keyword);
        let height_stretches =
            height_is_auto && grid_item_aspect_axis_stretches(vertical_alignment.keyword);
        let mut content_width = used_content_box_size(
            child_style.box_values.width.clone(),
            child_style.box_sizing,
            PercentageBasis::definite(content_box_pt(area_width)),
            horizontal_non_content,
        )
        .map(SemanticLengthExt::points)
        .or_else(|| {
            width_stretches.then_some((area_width - horizontal_non_content.points()).max(0.0))
        });
        let mut content_height = used_content_box_size(
            child_style.box_values.height.clone(),
            child_style.box_sizing,
            PercentageBasis::definite(content_box_pt(area_height)),
            vertical_non_content,
        )
        .map(SemanticLengthExt::points)
        .or_else(|| {
            height_stretches.then_some((area_height - vertical_non_content.points()).max(0.0))
        });
        match (content_width, content_height) {
            (None, Some(height)) => {
                content_width = non_replaced_aspect_ratio_content_width(
                    child_style,
                    height,
                    horizontal_non_content.points(),
                    vertical_non_content.points(),
                )
            }
            (Some(width), None) => {
                content_height = non_replaced_aspect_ratio_content_height(
                    child_style,
                    width,
                    horizontal_non_content.points(),
                    vertical_non_content.points(),
                )
            }
            (None | Some(_), None | Some(_)) => {}
        }
        let (Some(content_width), Some(content_height)) = (content_width, content_height) else {
            continue;
        };
        let width = constrain_content_width(
            child_style,
            content_box_pt(content_width),
            PercentageBasis::definite(layout_pt(area_width)),
        )
        .points()
            + horizontal_non_content.points();
        let height = constrain_content_height(
            child_style,
            content_box_pt(content_height),
            PercentageBasis::definite(layout_pt(area_height)),
        )
        .points()
            + vertical_non_content.points();
        let width = width.max(0.0);
        let height = height.max(0.0);
        let x =
            grid_item_aspect_axis_position(area_x, area_right, width, horizontal_alignment.keyword);
        let y =
            grid_item_aspect_axis_position(area_y, area_bottom, height, vertical_alignment.keyword);
        item.set_axis_geometry(GridAxis::Column, x, width);
        item.set_axis_geometry(GridAxis::Row, y, height);
    }
}

/// Resolve a contained subgrid item's cyclic percentage after track sizing.
///
/// Layout and paint containment turn each subgridded axis into `none`. A
/// percentage item on the resulting implicit track is automatic while sizing,
/// but resolves against its final grid area for item layout.
/// <https://drafts.csswg.org/css-grid-2/#subgrid-listing>
/// <https://www.w3.org/TR/css-grid-1/#grid-item-sizing>
fn apply_grid_deferred_percentage_item_size_corrections(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    container_width: PhysicalContentWidth,
    container_height: f32,
    column_line_offsets: &[f32],
    row_line_offsets: &[f32],
    items: &mut [GridItemLayout],
) {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return;
    }
    for (child, item) in children.iter().zip(items) {
        let Some(area) = item.area else {
            continue;
        };
        let child_style = &child.style;
        let Some(area_x) = content_aligned_grid_line_offset(
            container_style.justify_content,
            container_width.points(),
            column_line_offsets,
            usize::from(area.column_start).saturating_sub(1),
        ) else {
            continue;
        };
        let Some(area_right) = content_aligned_grid_line_offset(
            container_style.justify_content,
            container_width.points(),
            column_line_offsets,
            usize::from(area.column_end).saturating_sub(1),
        ) else {
            continue;
        };
        let Some(area_y) = content_aligned_grid_line_offset(
            container_style.align_content,
            container_height,
            row_line_offsets,
            usize::from(area.row_start).saturating_sub(1),
        ) else {
            continue;
        };
        let Some(area_bottom) = content_aligned_grid_line_offset(
            container_style.align_content,
            container_height,
            row_line_offsets,
            usize::from(area.row_end).saturating_sub(1),
        ) else {
            continue;
        };
        let area_width = (area_right - area_x).max(0.0);
        let area_height = (area_bottom - area_y).max(0.0);
        let metrics = used_box_metrics(
            child_style,
            PercentageBasis::definite(layout_pt(container_width.points())),
        );
        let horizontal_non_content = metrics.horizontal_non_content_length();
        let vertical_non_content = metrics.vertical_non_content_length();
        let justify_self = effective_grid_justify_self(child_style, container_style);
        let align_self = effective_grid_align_self(child_style, container_style);
        if grid_item_size_is_percentage(&child_style.box_values.width)
            && let Some(content_width) = used_content_box_size(
                child_style.box_values.width.clone(),
                child_style.box_sizing,
                PercentageBasis::definite(content_box_pt(area_width)),
                horizontal_non_content,
            )
        {
            let width = constrain_content_width(
                child_style,
                content_width,
                PercentageBasis::definite(layout_pt(area_width)),
            )
            .points()
                + horizontal_non_content.points();
            item.set_axis_geometry(
                GridAxis::Column,
                grid_item_aspect_axis_position(
                    area_x,
                    area_right,
                    width.max(0.0),
                    justify_self.keyword,
                ),
                width.max(0.0),
            );
        }
        if grid_item_size_is_percentage(&child_style.box_values.height)
            && let Some(content_height) = used_content_box_size(
                child_style.box_values.height.clone(),
                child_style.box_sizing,
                PercentageBasis::definite(content_box_pt(area_height)),
                vertical_non_content,
            )
        {
            let height = constrain_content_height(
                child_style,
                content_height,
                PercentageBasis::definite(layout_pt(area_height)),
            )
            .points()
                + vertical_non_content.points();
            item.set_axis_geometry(
                GridAxis::Row,
                grid_item_aspect_axis_position(
                    area_y,
                    area_bottom,
                    height.max(0.0),
                    align_self.keyword,
                ),
                height.max(0.0),
            );
        }
    }
}

fn grid_item_size_is_percentage(value: &css::ComputedLengthPercentageOrAuto) -> bool {
    matches!(
        value,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.contains_percentage()
    )
}

/// Whether an automatic grid-item axis receives its grid-area size.
///
/// For aspect-ratio boxes, `normal` falls back to start rather than stretch;
/// explicit `stretch` remains a definite sizing input.
fn grid_item_aspect_axis_stretches(alignment: SelfAlignmentKeyword) -> bool {
    matches!(alignment, SelfAlignmentKeyword::Stretch)
}

fn grid_item_aspect_axis_position(
    start: f32,
    end: f32,
    item_size: f32,
    alignment: SelfAlignmentKeyword,
) -> f32 {
    match alignment {
        SelfAlignmentKeyword::End
        | SelfAlignmentKeyword::FlexEnd
        | SelfAlignmentKeyword::SelfEnd
        | SelfAlignmentKeyword::Right => end - item_size,
        SelfAlignmentKeyword::Center => start + ((end - start - item_size) / 2.0),
        SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Start
        | SelfAlignmentKeyword::FlexStart
        | SelfAlignmentKeyword::SelfStart
        | SelfAlignmentKeyword::Left
        | SelfAlignmentKeyword::Stretch
        | SelfAlignmentKeyword::Baseline
        | SelfAlignmentKeyword::LastBaseline => start,
    }
}

pub(super) fn grid_subject_self_start_side(
    child_style: &ComputedStyle,
    axis: PhysicalAxis,
) -> Option<PhysicalSide> {
    let block_start = block_start_side(child_style.writing_mode);
    if block_start.axis() == axis {
        Some(block_start)
    } else {
        let inline_start =
            inline_start_side(child_style.writing_mode, child_style.used_direction());
        (inline_start.axis() == axis).then_some(inline_start)
    }
}

pub(super) fn grid_subject_self_end_side(
    child_style: &ComputedStyle,
    axis: PhysicalAxis,
) -> Option<PhysicalSide> {
    let block_end = block_end_side(child_style.writing_mode);
    if block_end.axis() == axis {
        Some(block_end)
    } else {
        let inline_end = inline_end_side(child_style.writing_mode, child_style.used_direction());
        (inline_end.axis() == axis).then_some(inline_end)
    }
}

struct SelfAlignmentAxisContext<'a> {
    axis: PhysicalAxis,
    content_alignment: css::ContentAlignment,
    container_size: f32,
    line_offsets: &'a [f32],
    start_line: usize,
    end_line: usize,
}

fn self_alignment_offset_for_side(
    side: PhysicalSide,
    context: SelfAlignmentAxisContext<'_>,
    item_size: f32,
) -> Option<f32> {
    if side.axis() != context.axis {
        return None;
    }
    let area_start = content_aligned_grid_line_offset(
        context.content_alignment,
        context.container_size,
        context.line_offsets,
        context.start_line,
    )?;
    let area_end = content_aligned_grid_line_offset(
        context.content_alignment,
        context.container_size,
        context.line_offsets,
        context.end_line,
    )?;
    let item_size = item_size.max(0.0);
    match (context.axis, side) {
        (PhysicalAxis::Horizontal, PhysicalSide::Left)
        | (PhysicalAxis::Vertical, PhysicalSide::Top) => Some(area_start),
        (PhysicalAxis::Horizontal, PhysicalSide::Right)
        | (PhysicalAxis::Vertical, PhysicalSide::Bottom) => Some(area_end - item_size),
        _ => None,
    }
}

/// Apply Quire-measured baseline self-alignment for simple grid rows.
///
/// Taffy's grid measure callback does not receive text baseline metadata, so
/// same-row baseline self-alignment would otherwise synthesize from item boxes.
/// This post-layout pass adjusts horizontal writing-mode participants sharing
/// the relevant baseline row edge to share their measured first or last
/// baselines:
/// <https://www.w3.org/TR/css-grid-1/#grid-baselines> and
/// <https://www.w3.org/TR/css-align-3/#baseline-align-self>.
fn apply_grid_baseline_alignment(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    row_line_offsets: &[f32],
    items: &mut [GridItemLayout],
) {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return;
    }
    let context = GridBaselineAlignmentContext {
        container_style,
        children,
        estimates,
        row_line_offsets,
    };
    for baseline_set in [GridBaselineSet::First, GridBaselineSet::Last] {
        let mut row_groups = Vec::<(u16, u16)>::new();
        for item in items.iter() {
            let Some(area) = item.area else {
                continue;
            };
            let key = grid_baseline_group_key(area, baseline_set);
            if !row_groups.contains(&key) {
                row_groups.push(key);
            }
        }
        for (row_start, row_end) in row_groups {
            align_grid_row_baseline_group(&context, items, row_start, row_end, baseline_set);
        }
    }
}

fn grid_baseline_group_key(area: GridItemArea, baseline_set: GridBaselineSet) -> (u16, u16) {
    match baseline_set {
        GridBaselineSet::First => (area.row_start, 0),
        GridBaselineSet::Last => (0, area.row_end),
    }
}

fn align_grid_row_baseline_group(
    context: &GridBaselineAlignmentContext<'_, '_>,
    items: &mut [GridItemLayout],
    row_start: u16,
    row_end: u16,
    baseline_set: GridBaselineSet,
) {
    let mut participant_count = 0_usize;
    let mut largest_distance = None::<f32>;
    for (index, item) in items.iter().enumerate() {
        if !grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
            || grid_child_baseline_set(context.container_style, &context.children[index].style)
                != Some(baseline_set)
        {
            continue;
        }
        let baseline = grid_item_border_box_baseline(&context.estimates[index], item, baseline_set);
        participant_count += 1;
        let distance = match baseline_set {
            GridBaselineSet::First => baseline,
            GridBaselineSet::Last => (item.height() - baseline).max(0.0),
        };
        largest_distance = Some(
            largest_distance
                .map(|target| target.max(distance))
                .unwrap_or(distance),
        );
    }
    if participant_count == 1 {
        // CSS Box Alignment falls a baseline self-alignment request back to
        // safe self-start/self-end when there is no compatible sharing
        // group. Taffy cannot perform this fallback from Quire's measured
        // baseline metadata, so apply it to the border box here.
        // <https://www.w3.org/TR/css-align-3/#baseline-align-self>
        for (index, item) in items.iter_mut().enumerate() {
            if !grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
                || grid_child_baseline_set(context.container_style, &context.children[index].style)
                    != Some(baseline_set)
            {
                continue;
            }
            let child_style = &context.children[index].style;
            let area_start = context
                .row_line_offsets
                .get(usize::from(row_start).saturating_sub(1))
                .cloned()
                .unwrap_or(item.y());
            let area_end = context
                .row_line_offsets
                .get(usize::from(row_end).saturating_sub(1))
                .cloned()
                .unwrap_or(item.y() + item.height());
            let y = match baseline_set {
                GridBaselineSet::First => area_start + child_style.margin.top,
                GridBaselineSet::Last => {
                    area_end - child_style.margin.bottom - item.height().max(0.0)
                }
            };
            item.set_axis_geometry(GridAxis::Row, y, item.height());
        }
        return;
    }
    if participant_count < 2 {
        return;
    }
    let Some(largest_distance) = largest_distance else {
        return;
    };
    let row_edge = grid_row_baseline_edge(
        items,
        context.row_line_offsets,
        row_start,
        row_end,
        baseline_set,
    );
    let target_baseline = match baseline_set {
        GridBaselineSet::First => row_edge + largest_distance,
        GridBaselineSet::Last => row_edge - largest_distance,
    };
    for (index, item) in items.iter_mut().enumerate() {
        if !grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
            || grid_child_baseline_set(context.container_style, &context.children[index].style)
                != Some(baseline_set)
        {
            continue;
        }
        let baseline = grid_item_border_box_baseline(&context.estimates[index], item, baseline_set);
        item.set_axis_geometry(GridAxis::Row, target_baseline - baseline, item.height());
    }
}

fn grid_row_baseline_edge(
    items: &[GridItemLayout],
    row_line_offsets: &[f32],
    row_start: u16,
    row_end: u16,
    baseline_set: GridBaselineSet,
) -> f32 {
    match baseline_set {
        GridBaselineSet::First => row_line_offsets
            .get(usize::from(row_start).saturating_sub(1))
            .cloned()
            .unwrap_or_else(|| {
                items
                    .iter()
                    .filter(|item| {
                        grid_item_in_baseline_group(
                            item,
                            row_start,
                            row_end,
                            GridBaselineSet::First,
                        )
                    })
                    .map(GridItemLayout::y)
                    .reduce(f32::min)
                    .unwrap_or(0.0)
            }),
        GridBaselineSet::Last => row_line_offsets
            .get(usize::from(row_end).saturating_sub(1))
            .cloned()
            .unwrap_or_else(|| {
                items
                    .iter()
                    .filter(|item| {
                        grid_item_in_baseline_group(item, row_start, row_end, GridBaselineSet::Last)
                    })
                    .map(|item| item.y() + item.height())
                    .reduce(f32::max)
                    .unwrap_or(0.0)
            }),
    }
}

fn grid_item_in_baseline_group(
    item: &GridItemLayout,
    row_start: u16,
    row_end: u16,
    baseline_set: GridBaselineSet,
) -> bool {
    item.area.is_some_and(|area| match baseline_set {
        GridBaselineSet::First => area.row_start == row_start,
        GridBaselineSet::Last => area.row_end == row_end,
    })
}

/// Return a same-page grid container baseline in content-box coordinates.
///
/// CSS Grid exports first and last baselines from the first or last row that
/// contains grid items. If that row has a compatible baseline-sharing group,
/// the container baseline comes from the shared alignment baseline; otherwise
/// it comes from the first or last item in row-major grid order, synthesizing a
/// missing item baseline from the item border box:
/// <https://www.w3.org/TR/css-grid-1/#grid-baselines> and
/// <https://www.w3.org/TR/css-align-3/#synthesize-baseline>.
fn grid_container_baseline(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    items: &[GridItemLayout],
    baseline_set: GridBaselineSet,
) -> Option<f32> {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return None;
    }
    let row_index = grid_container_baseline_row(items, baseline_set)?;
    for (index, item) in items.iter().enumerate() {
        if grid_item_intersects_row(item, row_index)
            && grid_child_baseline_set(container_style, &children[index].style)
                == Some(baseline_set)
        {
            return Some(
                item.y() + grid_item_border_box_baseline(&estimates[index], item, baseline_set),
            );
        }
    }
    grid_container_baseline_fallback_item(items, row_index, baseline_set).map(|index| {
        let item = &items[index];
        item.y() + grid_item_border_box_baseline(&estimates[index], item, baseline_set)
    })
}

fn grid_container_baseline_row(
    items: &[GridItemLayout],
    baseline_set: GridBaselineSet,
) -> Option<u16> {
    items
        .iter()
        .filter_map(|item| item.area)
        .map(|area| match baseline_set {
            GridBaselineSet::First => area.row_start,
            GridBaselineSet::Last => area.row_end.saturating_sub(1),
        })
        .reduce(match baseline_set {
            GridBaselineSet::First => u16::min,
            GridBaselineSet::Last => u16::max,
        })
}

fn grid_container_baseline_fallback_item(
    items: &[GridItemLayout],
    row_index: u16,
    baseline_set: GridBaselineSet,
) -> Option<usize> {
    let mut candidate = None::<(usize, u16, u16)>;
    for (index, item) in items.iter().enumerate() {
        let Some(area) = item.area else {
            continue;
        };
        if !grid_area_intersects_row(area, row_index) {
            continue;
        }
        let key = match baseline_set {
            GridBaselineSet::First => (area.row_start, area.column_start),
            GridBaselineSet::Last => (u16::MAX - area.row_end, u16::MAX - area.column_end),
        };
        if candidate.is_none_or(|(_, row, column)| (key.0, key.1) < (row, column)) {
            candidate = Some((index, key.0, key.1));
        }
    }
    candidate.map(|(index, _, _)| index)
}

fn grid_item_intersects_row(item: &GridItemLayout, row_index: u16) -> bool {
    item.area
        .is_some_and(|area| grid_area_intersects_row(area, row_index))
}

fn grid_area_intersects_row(area: GridItemArea, row_index: u16) -> bool {
    area.row_start <= row_index && row_index < area.row_end
}

fn grid_child_baseline_set(
    container_style: &ComputedStyle,
    child_style: &ComputedStyle,
) -> Option<GridBaselineSet> {
    if child_style.writing_mode != WritingMode::HorizontalTb {
        return None;
    }
    let keyword = if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.align_items.keyword
    } else {
        child_style.align_self.keyword
    };
    match keyword {
        SelfAlignmentKeyword::Baseline => Some(GridBaselineSet::First),
        SelfAlignmentKeyword::LastBaseline => Some(GridBaselineSet::Last),
        _ => None,
    }
}

fn grid_item_border_box_baseline(
    estimate: &GridItemEstimate,
    item: &GridItemLayout,
    baseline_set: GridBaselineSet,
) -> f32 {
    match baseline_set {
        GridBaselineSet::First => estimate.first_baseline.unwrap_or(item.height()),
        GridBaselineSet::Last => estimate.last_baseline.unwrap_or(0.0),
    }
}

/// Return used grid-line offsets from Taffy's final track layout.
///
/// CSS Grid absolute static positions are derived from the grid area in the
/// actual grid, including used track sizes, gutters, and collapsed `auto-fit`
/// repeated tracks:
/// <https://www.w3.org/TR/css-grid-1/#abspos-items> and
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
pub(in crate::layout::grid) fn grid_line_offsets_from_track_layout(
    sizes: &[f32],
    gutters: &[f32],
) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(sizes.len() + 1);
    let mut offset = 0.0;
    offsets.push(offset);
    for (index, size) in sizes.iter().enumerate() {
        offset += *size;
        if index + 1 < sizes.len() {
            offset += gutters.get(index).cloned().unwrap_or(0.0);
        }
        offsets.push(offset);
    }
    offsets
}

/// Resolve a Grid gap against a definite content-box dimension.
///
/// Track and line-offset algorithms remain scalar coordinate arithmetic; this
/// CSS used-value boundary retains the semantic layout length until a caller
/// enters one of those algorithms.
/// <https://www.w3.org/TR/css-grid-1/#gutters>
pub(in crate::layout) fn definite_grid_gap_size(
    gap: css::ComputedGap,
    container_size: LayoutLength,
) -> LayoutLength {
    match gap {
        css::ComputedGap::Normal => layout_pt(0.0),
        css::ComputedGap::LengthPercentage(value) => value
            .used_length_with_percentage_basis(PercentageBasis::definite(container_size))
            .unwrap_or_else(|| layout_pt(value.length_points())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_physical_height_drives_percentage_gap_relayout() {
        let gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_percent(0.5));
        let height = PhysicalContentHeight::new(content_box_pt(80.0));

        assert!(grid_gap_resolves_differently_with_basis(gap, height));
    }

    #[test]
    fn grid_layout_retains_a_physical_content_height() {
        let layout = GridLayout {
            height: PhysicalContentHeight::new(content_box_pt(60.0)),
            first_baseline: None,
            last_baseline: None,
            items: Vec::new(),
            gap_gutters: GapDecorationGridGutters::default(),
            column_line_offsets: Vec::new(),
            row_line_offsets: Vec::new(),
        };

        let _: PhysicalContentHeight = layout.height;
        assert_eq!(layout.height.points(), 60.0);
    }
}
