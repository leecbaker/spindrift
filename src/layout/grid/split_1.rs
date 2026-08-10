use super::lanes::grid_lanes_stacking_axis_is_block;
use super::lanes::{GridLanesItemPlacement, GridLanesLayoutContext};
use super::resolved::physical_grid_line_names;
use super::*;
use crate::layout::baseline::{BaselinePair, PhysicalBaselineSets, PhysicalTopBaselineOffset};
use crate::layout::block::{DefiniteBlockBreakContext, should_prebreak_definite_block};

/// Resolve the pre-layout scrollport reservation shared by Grid's intrinsic,
/// final sizing, and overflow-clip phases. Quire's static PDF UA uses overlay
/// scrollbars, so no native scrollbar chrome consumes Grid track space.
/// <https://drafts.csswg.org/css-overflow-3/#scrollbars-layout>
fn grid_scrollbar_reservation(_: &ComputedStyle) -> ScrollbarGutterReservation {
    ScrollbarGutterReservation::static_pdf_overlay()
}

/// Return the physical space available to Grid tracks after a reserved
/// vertical scrollbar has consumed horizontal content space.
fn grid_scrollport_content_width(
    content_width: PhysicalContentWidth,
    reservation: ScrollbarGutterReservation,
) -> PhysicalContentWidth {
    PhysicalContentWidth::new(content_box_pt(
        (content_width.points() - reservation.horizontal_extent().points()).max(0.0),
    ))
}

/// A definite physical height keeps its outer content box; a horizontal
/// scrollbar consumes only the Grid track-sizing space inside it.
fn grid_scrollport_content_height_basis(
    content_height: Option<PhysicalContentHeight>,
    reservation: ScrollbarGutterReservation,
) -> Option<PhysicalContentHeight> {
    content_height.map(|height| {
        PhysicalContentHeight::new(content_box_pt(
            (height.points() - reservation.vertical_extent().points()).max(0.0),
        ))
    })
}

/// An automatic Grid block size grows around a reserved horizontal scrollbar,
/// whereas a definite size has already allocated that scrollbar inside itself.
fn grid_total_content_height(
    track_height: PhysicalContentHeight,
    definite_content_height: Option<PhysicalContentHeight>,
    reservation: ScrollbarGutterReservation,
) -> PhysicalContentHeight {
    if let Some(height) = definite_content_height {
        return height;
    }
    PhysicalContentHeight::new(content_box_pt(
        track_height.points() + reservation.vertical_extent().points(),
    ))
}

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
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
    ) {
        let source_style = style;
        if style.position.is_out_of_flow_positioned() {
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
        // A normal-flow block-level Grid retains the usual block-width
        // fill-available behavior even when its writing mode makes physical
        // width the logical block axis. Its track-derived intrinsic block
        // contribution is for intrinsic sizing keywords and floated/atomic
        // participation, not a replacement for `width: auto` here.
        // <https://drafts.csswg.org/css-grid-2/#grid-container-size>
        let requested_content_width = if used_style.writing_mode.has_vertical_lines()
            && used_style.box_values.width.is_auto()
            && used_style.float == Float::None
        {
            PhysicalContentWidth::new(used_normal_flow_block_content_box_width(
                &used_style,
                layout_pt(containing_inline_size),
                non_content_pt(horizontal_extras),
            ))
        } else {
            self.used_block_physical_content_width(
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
            )
        };
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
        let scrollbar_reservation = grid_scrollbar_reservation(style);
        let containment = used_property_containment(element, style);
        let mut outer_x = width.border_box_inline_span.left_x() + relative_offset.x();
        let mut inner_x = outer_x + border_widths.left + style.padding.left;
        let inner_width = grid_scrollport_content_width(
            PhysicalContentWidth::new(content_box_pt(content_width)),
            scrollbar_reservation,
        )
        .points();
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
        let size_contained_content_height = if containment.size {
            let empty_grid_height = self
                .compute_grid_layout(
                    style,
                    &[],
                    stylesheets,
                    PhysicalContentWidth::new(content_box_pt(inner_width)),
                    grid_scrollport_content_height_basis(
                        definite_content_height,
                        scrollbar_reservation,
                    ),
                    GridLayoutPurpose::IntrinsicProbe,
                )
                .map(|layout| layout.height)
                .unwrap_or_else(|| PhysicalContentHeight::new(content_box_pt(0.0)));
            Some(
                grid_scrollport_content_height_basis(
                    definite_content_height,
                    scrollbar_reservation,
                )
                .unwrap_or_else(|| {
                    PhysicalContentHeight::new(constrain_content_height(
                        style,
                        empty_grid_height.content_box_length(),
                        PercentageBasis::definite(layout_pt(available_outer_height)),
                    ))
                }),
            )
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
        let grid_item_layout_height_basis =
            grid_scrollport_content_height_basis(definite_content_height, scrollbar_reservation);

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
            .or_else(|| {
                Some(PhysicalContentHeight::new(constrain_content_height(
                    style,
                    grid_layout.height.content_box_length(),
                    PercentageBasis::definite(layout_pt(available_outer_height)),
                )))
            })
            .map(|height| {
                grid_total_content_height(height, definite_content_height, scrollbar_reservation)
                    .points()
            })
            .expect("the Grid layout always supplies a content height");
        // Keep curved overflow contours until descendant paint has been
        // captured. An eager padding-box rectangle would irreversibly erase
        // the CSS Borders contour before the shared resolver can retain it.
        let needs_contoured_overflow_clip = self.element_used_overflow_clips(element, style)
            && box_content_contour_is_non_rectangular(style);
        let overflow_clip_active =
            if self.element_used_overflow_clips(element, style) && !needs_contoured_overflow_clip {
                self.push_padding_box_overflow_clip(
                    element,
                    style,
                    Some(scrollbar_reservation),
                    outer_x,
                    block_top,
                    border_widths,
                    content_width,
                    total_content_height,
                )
            } else {
                false
            };
        let suppresses_descendant_fragmentation = used_property_containment(element, style).size
            || (overflow_clip_active && definite_content_height.is_some());
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
            for (index, (child, item)) in children.iter().zip(&grid_layout.items).enumerate() {
                self.replay_grid_item_with_resolved_axes(
                    style,
                    &grid_layout,
                    child,
                    item,
                    grid_layout.baseline_resolutions.get(index),
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
                    positioning_containing_block: establishes_positioning_containing_block
                        .then(|| self.containing_blocks.last().copied())
                        .flatten(),
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
        let contents_overflow_clip =
            (overflow_clip_active || needs_contoured_overflow_clip).then(|| {
                PageTopRect::new(
                    outer_x + border_widths.left,
                    block_top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                )
                .paint_clip()
            });
        let contoured_contents_overflow_clip = contents_overflow_clip
            .filter(|_| needs_contoured_overflow_clip)
            .and_then(|bounds| {
                resolve_box_content_contour(
                    paint_space_rect(outer_x, block_bottom, outer_width, block_height),
                    style,
                    border_widths,
                    BoxContentContourRequest::Overflow {
                        reference_box: css::BackgroundBox::Padding,
                        outset: 0.0,
                    },
                )
                .map(|mut contour| {
                    // Grid fragmentation owns the used padding-box span;
                    // contour resolution owns only its precise edge.
                    contour.bounds = bounds;
                    contour
                })
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
            if page_index == paint_page_index {
                if let Some(contour) = contoured_contents_overflow_clip.clone() {
                    fragment = fragment.with_contents_effect_scoped_to_box_content_contour(contour);
                } else if let Some(clip) = contents_overflow_clip {
                    fragment = fragment.with_contents_effect_scoped_to_rect(clip);
                }
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
                        && (style.background.background_color.is_potentially_visible()
                            || style.background.background_image.is_image()
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
        if style.position.is_in_flow_positioned() {
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
        stylesheets: &Stylesheets<'_>,
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
        let scrollbar_reservation = grid_scrollbar_reservation(style);
        let containment = used_property_containment(element, style);
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
        let grid_content_width = grid_scrollport_content_width(
            PhysicalContentWidth::new(content_box_pt(content_width)),
            scrollbar_reservation,
        );
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
        let size_contained_content_height = if containment.size {
            let empty_grid_height = self
                .compute_grid_layout(
                    style,
                    &[],
                    stylesheets,
                    grid_content_width,
                    grid_scrollport_content_height_basis(
                        definite_content_height,
                        scrollbar_reservation,
                    ),
                    GridLayoutPurpose::IntrinsicProbe,
                )
                .map(|layout| layout.height)
                .unwrap_or_else(|| PhysicalContentHeight::new(content_box_pt(0.0)));
            Some(
                grid_scrollport_content_height_basis(
                    definite_content_height,
                    scrollbar_reservation,
                )
                .unwrap_or_else(|| {
                    PhysicalContentHeight::new(constrain_content_height(
                        style,
                        empty_grid_height.content_box_length(),
                        PercentageBasis::definite(layout_pt(available_width)),
                    ))
                }),
            )
        } else {
            None
        };
        let grid_layout = self.compute_grid_layout(
            style,
            &children,
            stylesheets,
            grid_content_width,
            size_contained_content_height.or_else(|| {
                grid_scrollport_content_height_basis(definite_content_height, scrollbar_reservation)
            }),
            GridLayoutPurpose::IntrinsicProbe,
        );
        let content_height = size_contained_content_height.unwrap_or_else(|| {
            let measured = grid_layout
                .as_ref()
                .map(|layout| layout.height.points())
                .unwrap_or(style.line_height)
                .max(style.line_height);
            PhysicalContentHeight::new(constrain_content_height(
                style,
                content_box_pt(measured),
                PercentageBasis::definite(layout_pt(available_width)),
            ))
        });
        let content_height = grid_total_content_height(
            content_height,
            definite_content_height,
            scrollbar_reservation,
        )
        .points();
        let border_box_height = content_height + vertical_extras;
        let baseline_offset = grid_layout
            .as_ref()
            .and_then(|layout| layout.baselines.vertical.first)
            .map(PhysicalTopBaselineOffset::points)
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
        stylesheets: &Stylesheets<'_>,
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
        let scrollbar_reservation = grid_scrollbar_reservation(style);
        let containment = used_property_containment(element, style);
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
        let grid_content_width = grid_scrollport_content_width(
            PhysicalContentWidth::new(content_box_pt(content_width)),
            scrollbar_reservation,
        );
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
        let size_contained_content_height = if containment.size {
            let empty_grid_height = self
                .compute_grid_layout(
                    style,
                    &[],
                    stylesheets,
                    grid_content_width,
                    grid_scrollport_content_height_basis(
                        definite_content_height,
                        scrollbar_reservation,
                    ),
                    GridLayoutPurpose::IntrinsicProbe,
                )
                .map(|layout| layout.height)
                .unwrap_or_else(|| PhysicalContentHeight::new(content_box_pt(0.0)));
            Some(
                grid_scrollport_content_height_basis(
                    definite_content_height,
                    scrollbar_reservation,
                )
                .unwrap_or_else(|| {
                    PhysicalContentHeight::new(constrain_content_height(
                        style,
                        empty_grid_height.content_box_length(),
                        PercentageBasis::definite(layout_pt(available_width)),
                    ))
                }),
            )
        } else {
            None
        };
        let grid_content_height_basis = size_contained_content_height.or_else(|| {
            grid_scrollport_content_height_basis(definite_content_height, scrollbar_reservation)
        });
        let Some(grid_layout) = self.compute_grid_layout(
            style,
            &children,
            stylesheets,
            grid_content_width,
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

        let total_content_height = size_contained_content_height.unwrap_or_else(|| {
            PhysicalContentHeight::new(constrain_content_height(
                style,
                grid_layout.height.content_box_length(),
                PercentageBasis::definite(layout_pt(available_width)),
            ))
        });
        let total_content_height = grid_total_content_height(
            total_content_height,
            definite_content_height,
            scrollbar_reservation,
        )
        .points();
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
        let inner_width = grid_content_width.points();
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
        let suppresses_descendant_fragmentation = containment.size;
        if suppresses_descendant_fragmentation {
            self.fragmentation_suppression_depth += 1;
        }
        self.push_float_context();
        for (index, (child, item)) in children.iter().zip(&grid_layout.items).enumerate() {
            self.replay_grid_item_with_resolved_axes(
                style,
                &grid_layout,
                child,
                item,
                grid_layout.baseline_resolutions.get(index),
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
                    positioning_containing_block: establishes_positioning_containing_block
                        .then(|| self.containing_blocks.last().copied())
                        .flatten(),
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
        // Layout containment suppresses every descendant-provided baseline,
        // including the fallback recovered from captured fragment paint. Do
        // not let `first_line_y()` re-export the grid item's internal line
        // after the Grid baseline has been correctly suppressed.
        // <https://www.w3.org/TR/css-contain-1/#containment-layout>
        let descendant_baseline = (!containment.layout)
            .then_some(grid_layout.baselines.vertical.first)
            .flatten()
            .map(PhysicalTopBaselineOffset::points)
            .map(|baseline| border_widths.top + style.padding.top + baseline)
            .or_else(|| {
                (!containment.layout)
                    .then(|| fragment.first_line_y())
                    .flatten()
                    .map(|line_y| (border_box_height - line_y).max(0.0))
            });
        let baseline_offset = LayoutBuilder::inline_block_baseline_offset_with_containment(
            style.as_computed(),
            containment.layout,
            border_box_height,
            descendant_baseline,
        );
        let inline_lanes_overflow_clearance =
            grid_lanes_inline_overflow_clearance(style.as_computed(), &grid_layout);
        let fixed_layers = self.fixed_layers.split_off(fixed_layer_start);
        self.restore(snapshot);
        self.fixed_layers.extend(fixed_layers);
        // Grid Lanes exports its packed baseline through `grid_layout` just
        // like an ordinary grid container.  Do not rewrite an authored
        // baseline alignment here: doing so changes the line box that owns an
        // inline-grid-lanes atom rather than its exported baseline.
        // <https://drafts.csswg.org/css-grid-3/#grid-lanes-baseline-alignment>
        let mut atom_style = style.as_computed().clone();
        // The clearance is a margin-box contribution, not a larger border
        // box. Keeping those spaces distinct preserves the captured grid
        // fragment's top edge while reserving endward line space for its
        // visible stacking overflow.
        atom_style.margin.bottom += inline_lanes_overflow_clearance;

        InlineAtom::new(
            InlineAtomContent::InlineFragment {
                fragment: Box::new(fragment),
                replay_coordinates: AtomicInlineFragmentReplayCoordinates::border_box_local(),
                table_cell_context: None,
                contents_overflow_clip_applied: false,
            },
            atom_style,
            None,
            InlineSize::new(
                content_width + horizontal_extras + style.margin.left + style.margin.right,
                border_box_height
                    + inline_lanes_overflow_clearance
                    + style.margin.top
                    + style.margin.bottom,
            ),
            baseline_offset,
            baseline_shift,
            link_target,
            None,
        )
    }
}

/// Reserve the leading grid-axis quantum below an inline column-lanes box
/// when a final percentage-sized item visibly overflows its fixed stacking
/// size. This keeps following line boxes out of that overflow while leaving
/// the container's specified border box unchanged.
///
/// Grid Lanes' stacking range may exceed the content box, while its inline
/// formatting participation remains atomic. The clearance is the lead-in to
/// the first overflowing final item, preserving the grid-axis placement
/// quantum without making the principal border box taller.
/// <https://drafts.csswg.org/css-grid-3/#sizing-grid-containers>
fn grid_lanes_inline_overflow_clearance(style: &ComputedStyle, layout: &GridLayout) -> f32 {
    if !style.display.is_grid_lanes()
        || !grid_lanes_stacking_axis_is_block(style)
        || style.vertical_align != VerticalAlign::BASELINE
        || !layout.items.iter().any(|item| {
            item.final_percentage_axes().height && item.height() > layout.height.points() + 0.01
        })
    {
        return 0.0;
    }
    layout
        .items
        .iter()
        .filter(|item| {
            item.final_percentage_axes().height && item.height() > layout.height.points() + 0.01
        })
        .map(GridItemLayout::x)
        .filter(|offset| *offset > 0.01)
        .min_by(|left, right| left.total_cmp(right))
        .unwrap_or(0.0)
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
    /// Grid's exported baselines in physical content-box coordinates.  The
    /// legacy scalar pair below remains a temporary adapter for callers that
    /// are known to consume only horizontal writing-mode baselines.
    pub(in crate::layout) baselines: PhysicalBaselineSets,
    pub(in crate::layout) first_baseline: Option<f32>,
    pub(in crate::layout) last_baseline: Option<f32>,
    pub(in crate::layout) items: Vec<GridItemLayout>,
    pub(super) baseline_resolutions: Vec<GridBaselineResolution>,
    pub(in crate::layout) gap_gutters: GapDecorationGridGutters,
    pub(in crate::layout) column_line_offsets: Vec<f32>,
    pub(in crate::layout) row_line_offsets: Vec<f32>,
    /// Final Grid line names in physical Taffy order. This carries inherited
    /// and locally-added subgrid names through nested replay.
    pub(in crate::layout) column_line_names: Vec<css::GridLineNames>,
    pub(in crate::layout) row_line_names: Vec<css::GridLineNames>,
    /// Used physical track sizes retained for edge-track consumers such as
    /// `margin-trim`; zero-sized auto-fit tracks are collapsed tracks.
    column_track_sizes: Vec<f32>,
    row_track_sizes: Vec<f32>,
}

impl GridLayout {
    /// Used physical track sizes reported by the shared Grid sizing pass.
    ///
    /// Grid Lanes uses these only while resolving its Level 3 intrinsic
    /// auto-repeat hypothesis, before it performs its distinct packing pass.
    pub(super) fn physical_track_sizes(&self, axis: GridAxis) -> &[f32] {
        match axis {
            GridAxis::Column => &self.column_track_sizes,
            GridAxis::Row => &self.row_track_sizes,
        }
    }

    /// Replace one physical axis with the final Grid Lanes topology before
    /// measuring packed children. This keeps subgrid probes and final replay
    /// on the same used tracks and line-name map.
    pub(super) fn set_physical_grid_axis_topology(
        &mut self,
        axis: GridAxis,
        line_offsets: Vec<f32>,
        track_sizes: Vec<f32>,
        line_names: Vec<css::GridLineNames>,
    ) {
        debug_assert_eq!(line_offsets.len(), track_sizes.len().saturating_add(1));
        debug_assert_eq!(line_offsets.len(), line_names.len());
        match axis {
            GridAxis::Column => {
                self.column_line_offsets = line_offsets;
                self.column_track_sizes = track_sizes;
                self.column_line_names = line_names;
            }
            GridAxis::Row => {
                self.row_line_offsets = line_offsets;
                self.row_track_sizes = track_sizes;
                self.row_line_names = line_names;
            }
        }
    }
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
    /// Whether a Grid Lanes item was placed from a definite grid-axis line or
    /// by the lanes cursor.  A final numeric area alone cannot preserve this:
    /// automatic subgrids remain track-aligned, but do not inherit parent line
    /// names. <https://drafts.csswg.org/css-grid-3/#subgrids>
    grid_lanes_placement: Option<GridLanesItemPlacement>,
    used_box_metrics: Option<UsedBoxMetrics>,
    final_percentage_axes: GridItemFinalPercentageAxes,
    /// A stretched vertical grid item retains a cyclic physical-height
    /// percentage during replay when the container's corresponding grid axis
    /// was indefinite. Its final grid area is still used for placement and
    /// painting; only its own content sizing must not be re-resolved against
    /// that area as a newly definite authored height.
    /// <https://drafts.csswg.org/css-grid-2/#grid-item-sizing>
    replay_cyclic_physical_height: bool,
}

/// Physical axes whose used size came from the post-track percentage phase.
/// Replay must retain those bounds instead of resolving the authored value
/// again against its temporary item formatting context.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout::grid) struct GridItemFinalPercentageAxes {
    pub(in crate::layout::grid) width: bool,
    pub(in crate::layout::grid) height: bool,
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

/// Replace Taffy's emulated subgrid area with the selected parent track area.
///
/// Taffy supplies placement order only: it cannot represent a borrowed Grid
/// axis, so final geometry must remain tied to the parent’s used tracks.
/// <https://www.w3.org/TR/css-grid-2/#subgrids>
fn apply_resolved_subgrid_axis_item_geometry(
    axis: Option<&ResolvedSubgridAxis>,
    physical_axis: GridAxis,
    items: &mut [GridItemLayout],
) {
    let Some(axis) = axis else {
        return;
    };
    for item in items {
        let Some(area) = item.area else {
            continue;
        };
        let (start_line, end_line) = match physical_axis {
            GridAxis::Column => (area.column_start, area.column_end),
            GridAxis::Row => (area.row_start, area.row_end),
        };
        if let Some((start, end)) = axis.track_area_span(start_line, end_line) {
            item.set_axis_geometry(physical_axis, start, (end - start).max(0.0));
        }
    }
}

impl GridItemLayout {
    pub(in crate::layout::grid) fn new(rect: GridRect, area: Option<GridItemArea>) -> Self {
        Self {
            rect,
            area,
            grid_lanes_placement: None,
            used_box_metrics: None,
            final_percentage_axes: GridItemFinalPercentageAxes::default(),
            replay_cyclic_physical_height: false,
        }
    }

    pub(in crate::layout::grid) fn with_used_box_metrics(
        mut self,
        used_box_metrics: UsedBoxMetrics,
    ) -> Self {
        self.used_box_metrics = Some(used_box_metrics);
        self
    }

    pub(in crate::layout::grid) fn used_box_metrics(&self) -> Option<UsedBoxMetrics> {
        self.used_box_metrics
    }

    pub(in crate::layout::grid) fn final_percentage_axes(&self) -> GridItemFinalPercentageAxes {
        self.final_percentage_axes
    }

    pub(in crate::layout::grid) fn preserves_cyclic_physical_height_on_replay(&self) -> bool {
        self.replay_cyclic_physical_height
    }

    pub(in crate::layout::grid) fn preserve_cyclic_physical_height_on_replay(&mut self) {
        self.replay_cyclic_physical_height = true;
    }

    pub(in crate::layout::grid) fn mark_final_percentage_axis(&mut self, axis: GridAxis) {
        match axis {
            GridAxis::Column => self.final_percentage_axes.width = true,
            GridAxis::Row => self.final_percentage_axes.height = true,
        }
    }

    pub(in crate::layout::grid) fn grid_lanes_placement(&self) -> Option<GridLanesItemPlacement> {
        self.grid_lanes_placement
    }

    pub(in crate::layout::grid) fn set_grid_lanes_placement(
        &mut self,
        placement: GridLanesItemPlacement,
    ) {
        self.grid_lanes_placement = Some(placement);
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
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
        purpose: GridLayoutPurpose,
    ) -> Option<GridLayout> {
        // A direct subgrid replay installs this context immediately before
        // entering its formatting context. Intrinsic probes may run first,
        // but they must only borrow that resolved topology: consuming it
        // there would make the subsequent final replay fall back to an
        // unresolved standalone grid. The final layout owns the one-shot
        // consumption.
        // <https://drafts.csswg.org/css-grid-2/#subgrids>
        let subgrid_context = match purpose {
            GridLayoutPurpose::IntrinsicProbe => self.resolved_subgrid_context_for_probe(),
            GridLayoutPurpose::FinalLayout => self.take_resolved_subgrid_context(),
        };
        self.compute_grid_layout_with_margin_trim(
            style,
            children,
            stylesheets,
            width,
            height,
            purpose,
            subgrid_context,
            true,
        )
    }

    /// Run Grid sizing after deriving any container-owned `margin-trim` used
    /// margins from a placement probe.  Placement is independent of an item's
    /// margins, while track sizing is not, so the probe must not become the
    /// final sizing pass.
    /// <https://drafts.csswg.org/css-box-4/#margin-trim-grid>.
    #[allow(clippy::too_many_arguments)]
    fn compute_grid_layout_with_margin_trim(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
        purpose: GridLayoutPurpose,
        subgrid_context: Option<ResolvedSubgridContext>,
        derive_margin_trim: bool,
    ) -> Option<GridLayout> {
        if derive_margin_trim && grid_has_margin_trim(style) && !children.is_empty() {
            let placement_probe = self.compute_grid_layout_with_margin_trim(
                style,
                children,
                stylesheets,
                width,
                height,
                purpose,
                subgrid_context.clone(),
                false,
            )?;
            let plan = grid_margin_trim_plan(style, &placement_probe);
            if !plan.is_empty() {
                let mut trimmed_children = children.to_vec();
                for (index, child) in trimmed_children.iter_mut().enumerate() {
                    plan.apply_to_style(index, &mut child.style);
                }
                return self.compute_grid_layout_with_margin_trim(
                    style,
                    &trimmed_children,
                    stylesheets,
                    width,
                    height,
                    purpose,
                    subgrid_context,
                    false,
                );
            }
        }
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
                item_placement_overrides: Vec::new(),
                baseline_plan: None,
            },
        )?;
        // Grid baseline alignment affects intrinsic track sizes before the
        // ordinary track-sizing algorithm runs.  Taffy does not expose an
        // item-baseline measurement channel, so first obtain its placement
        // topology, derive the spec's sizing-only baseline shims from that
        // topology, and run the real sizing pass with those shims installed.
        // The probe's placement is independent of track sizes; only the
        // resulting track contributions change in the second pass.
        // <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
        let baseline_plan = self.grid_baseline_sizing_plan(
            style,
            children,
            stylesheets,
            width,
            height,
            &preliminary_layout,
        );
        let preliminary_layout = if baseline_plan.is_empty() {
            preliminary_layout
        } else {
            self.compute_grid_layout_pass(
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
                    item_placement_overrides: Vec::new(),
                    baseline_plan: Some(baseline_plan.clone()),
                },
            )?
        };
        let contributions = if purpose == GridLayoutPurpose::FinalLayout
            && subgrid_context.is_none()
        {
            self.collect_subgrid_contributions(style, children, stylesheets, &preliminary_layout)
        } else {
            Vec::new()
        };
        // Descendant contribution proxies take part in parent track sizing,
        // but they are not Grid items and must not displace automatic items
        // during the proxy pass. Preserve the preliminary placement while
        // resolving the shared inherited tracks.
        // <https://drafts.csswg.org/css-grid-2/#subgrid-contributions>
        let contribution_item_placement_overrides = (!contributions.is_empty()).then(|| {
            preliminary_layout
                .items
                .iter()
                .map(|item| item.area)
                .collect::<Vec<_>>()
        });
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
                    item_placement_overrides: contribution_item_placement_overrides
                        .clone()
                        .unwrap_or_default(),
                    baseline_plan: Some(baseline_plan.clone()),
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
                    item_placement_overrides: contribution_item_placement_overrides
                        .unwrap_or_default(),
                    baseline_plan: Some(baseline_plan),
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
                    item_placement_overrides: contribution_item_placement_overrides
                        .unwrap_or_default(),
                    baseline_plan: Some(baseline_plan),
                },
            );
        }
        if style.display.is_grid_lanes() {
            return Some(self.apply_grid_lanes_placement(
                style,
                children,
                stylesheets,
                GridLanesLayoutContext {
                    width,
                    block_percentage_basis:
                        height.map_or_else(PercentageBasis::indefinite, |height| {
                            PercentageBasis::definite(layout_pt(height.points()))
                        }),
                    subgrid_context: subgrid_context.as_ref(),
                },
                intrinsic_layout,
            ));
        }
        Some(intrinsic_layout)
    }

    /// Resolve the baseline sizing data from an already-placed Grid topology.
    ///
    /// Grid placement does not depend on the used track sizes, so the
    /// preliminary pass supplies stable row/column membership while this pass
    /// supplies the measured item baselines used to construct intrinsic-size
    /// shims.  The shims are installed only in the following Taffy sizing
    /// pass, never in the replay style.
    /// <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
    fn grid_baseline_sizing_plan(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
        topology: &GridLayout,
    ) -> GridBaselinePlan {
        let available_space = GridPhysicalAvailableSpace {
            width_basis: grid_percentage_basis(
                Some(width.content_box_length()),
                GridAvailableSizeSource::ContainerInlineSize,
            ),
            height_basis: grid_percentage_basis(
                height.map(PhysicalContentHeight::content_box_length),
                GridAvailableSizeSource::ContainerBlockSize,
            ),
        };
        let estimates = children
            .iter()
            .map(|child| {
                self.estimate_grid_item_size(
                    child,
                    stylesheets,
                    width.points(),
                    available_space.width_basis,
                    available_space.height_basis,
                )
            })
            .collect::<Vec<_>>();
        grid_baseline_plan(
            style,
            children,
            &estimates,
            &topology.baseline_resolutions,
            &topology.items,
        )
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
        stylesheets: &Stylesheets<'_>,
        placed_item_border_box_width: f32,
        height: Option<f32>,
        purpose: GridLayoutPurpose,
    ) -> Option<GridLayout> {
        // This temporary probe owns the context installed by the contribution
        // collector. Unlike an intrinsic probe encountered while replaying a
        // real subgrid item, it has no later formatting pass that must retain
        // the context.
        let subgrid_context = self.take_resolved_subgrid_context();
        self.compute_grid_layout_with_margin_trim(
            style,
            children,
            stylesheets,
            PhysicalContentWidth::new(content_box_pt(placed_item_border_box_width)),
            height.map(|height| PhysicalContentHeight::new(content_box_pt(height))),
            purpose,
            subgrid_context,
            true,
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
    pub(super) fn compute_grid_layout_pass(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
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
        let item_available_space = GridPhysicalAvailableSpace {
            width_basis: item_width_basis,
            height_basis: item_height_basis,
        };
        let item_inline_basis = item_available_space.logical_inline_basis(style);
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
        let mut item_box_metrics = Vec::with_capacity(children.len());
        for (index, child) in children.iter().enumerate() {
            let placement_override = config
                .item_placement_overrides
                .get(index)
                .copied()
                .flatten();
            let overridden_row = placement_override.and_then(|area| {
                taffy_grid_area_line(
                    area,
                    if swaps_physical_grid_axes {
                        GridAxis::Column
                    } else {
                        GridAxis::Row
                    },
                )
            });
            let overridden_column = placement_override.and_then(|area| {
                taffy_grid_area_line(
                    area,
                    if swaps_physical_grid_axes {
                        GridAxis::Row
                    } else {
                        GridAxis::Column
                    },
                )
            });
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
            item_box_metrics.push(used_box_metrics_for_logical_inline_basis(
                &child.style,
                item_inline_basis.map_source(|_| ()),
            ));
            let node = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        display: taffy_layout::Display::Block,
                        box_sizing: taffy_bridge::box_sizing(child.style.box_sizing),
                        direction: taffy_bridge::direction(child.style.used_direction()),
                        size: taffy_layout::Size {
                            width: taffy_grid_item_dimension(
                                child.style.box_values.width.clone(),
                                item_width_basis,
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: taffy_grid_item_dimension(
                                child.style.box_values.height.value().clone(),
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
                            width: grid_item_taffy_min_dimension(
                                child.style.box_values.min_width.clone(),
                                GridAxis::Column,
                                style,
                                &child.style,
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: grid_item_taffy_min_dimension(
                                child.style.box_values.min_height.clone(),
                                GridAxis::Row,
                                style,
                                &child.style,
                                physical_estimate.min_height,
                                physical_estimate.content_height,
                            ),
                        },
                        max_size: taffy_layout::Size {
                            width: taffy_grid_item_constraint_dimension(
                                child.style.box_values.max_width.clone(),
                                GridPercentageBasis::indefinite(),
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: taffy_grid_item_constraint_dimension(
                                child.style.box_values.max_height.clone(),
                                GridPercentageBasis::indefinite(),
                                physical_estimate.min_height,
                                physical_estimate.content_height,
                            ),
                        },
                        margin: grid_taffy_margin_with_baseline_shim(
                            &child.style,
                            item_inline_basis,
                            config
                                .baseline_plan
                                .as_ref()
                                .and_then(|plan| plan.shim(index)),
                        ),
                        padding: taffy_bridge::padding(&child.style, item_inline_basis),
                        border: taffy_bridge::border_edges(used_border_widths(&child.style)),
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
                        grid_row: overridden_row.unwrap_or_else(|| {
                            if swaps_physical_grid_axes {
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
                            }
                        }),
                        grid_column: overridden_column.unwrap_or_else(|| {
                            if swaps_physical_grid_axes {
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
                            }
                        }),
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
                    direction: taffy_bridge::direction(style.used_direction()),
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
                                taffy_bridge::gap(
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
                                taffy_bridge::gap(
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
        let mut column_track_sizes = Vec::new();
        let mut row_track_sizes = Vec::new();
        let mut track_corrections = GridTrackLayoutCorrections::default();
        let mut gap_gutters = match tree.detailed_layout_info(root) {
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
                column_track_sizes = column_sizes.to_vec();
                row_track_sizes = row_sizes.to_vec();
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
        // Taffy is used to resolve placement topology, but it has no subgrid
        // model. Once placement is known, inherited axes must retain the
        // parent-owned track and gutter geometry exactly.
        // <https://www.w3.org/TR/css-grid-2/#subgrids>
        if let Some(axis) = physical_column_subgrid {
            column_line_offsets = axis.line_offsets().to_vec();
            column_track_sizes = axis
                .track_starts()
                .iter()
                .zip(axis.track_ends())
                .map(|(start, end)| (end - start).max(0.0))
                .collect();
            gap_gutters.columns = axis.gap_gutters();
        }
        if let Some(axis) = physical_row_subgrid {
            row_line_offsets = axis.line_offsets().to_vec();
            row_track_sizes = axis
                .track_starts()
                .iter()
                .zip(axis.track_ends())
                .map(|(start, end)| (end - start).max(0.0))
                .collect();
            gap_gutters.rows = axis.gap_gutters();
        }
        let column_line_names = physical_column_subgrid.map_or_else(
            || physical_grid_line_names(style, GridAxis::Column, column_line_offsets.len()),
            |axis| axis.physical_line_names().to_vec(),
        );
        let row_line_names = physical_row_subgrid.map_or_else(
            || physical_grid_line_names(style, GridAxis::Row, row_line_offsets.len()),
            |axis| axis.physical_line_names().to_vec(),
        );
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
                Some(
                    GridItemLayout::new(
                        GridRect::new(
                            GridPoint::new(layout.location.x, layout.location.y),
                            GridSize::new(layout.size.width.max(0.0), layout.size.height.max(0.0)),
                        ),
                        grid_item_areas.get(index).cloned(),
                    )
                    .with_used_box_metrics(item_box_metrics[index]),
                )
            })
            .collect::<Option<Vec<_>>>()?;
        debug_assert_eq!(items.len(), children.len());
        apply_resolved_subgrid_axis_item_geometry(
            physical_column_subgrid,
            GridAxis::Column,
            &mut items,
        );
        apply_resolved_subgrid_axis_item_geometry(physical_row_subgrid, GridAxis::Row, &mut items);
        let final_grid_height = physical_row_subgrid
            .map(ResolvedSubgridAxis::outer_extent)
            .unwrap_or(root_layout.size.height);
        // Stretch computes a final grid-area rectangle without converting an
        // automatic item inline size into an authored definite size. When a
        // vertical grid's physical height was indefinite during track sizing,
        // preserve that cycle at replay so the item's content uses the same
        // percentage behavior that participated in intrinsic sizing.
        // <https://drafts.csswg.org/css-grid-2/#grid-item-sizing>
        if swaps_physical_grid_axes && !item_height_basis.is_definite() {
            for (item, child) in items.iter_mut().zip(children) {
                let inline_self = effective_grid_justify_self(&child.style, style).keyword;
                if child.style.box_values.height.is_auto()
                    && matches!(
                        inline_self,
                        SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
                    )
                {
                    item.preserve_cyclic_physical_height_on_replay();
                }
            }
        }
        apply_startward_auto_fit_track_corrections(
            style,
            content_width,
            final_grid_height,
            &track_corrections,
            &mut items,
        );
        apply_grid_self_alignment_corrections(
            style,
            children,
            content_width,
            final_grid_height,
            &column_line_offsets,
            &row_line_offsets,
            &mut items,
        );
        apply_grid_aspect_ratio_item_size_corrections(
            style,
            children,
            content_width,
            final_grid_height,
            &column_line_offsets,
            &row_line_offsets,
            &mut items,
        );
        apply_grid_replaced_item_size_corrections(style, children, &estimates, &mut items);
        apply_grid_deferred_percentage_item_size_corrections(
            GridFinalItemPercentagePlacement {
                container_style: style,
                container_width: content_width,
                container_height: final_grid_height,
                column_line_offsets: &column_line_offsets,
                row_line_offsets: &row_line_offsets,
            },
            children,
            &estimates,
            &mut items,
        );
        let baseline_resolutions =
            resolve_grid_baseline_participation(style, children, &items, item_available_space);
        // Taffy reports each item's border-box location after resolving its
        // grid-area margins. Replay suppresses those margins in the child
        // style, but must retain the reported border-box origin unchanged.
        // <https://www.w3.org/TR/css-grid-1/#grid-item-placement>.
        apply_grid_baseline_alignment(
            style,
            children,
            &estimates,
            &baseline_resolutions,
            &row_line_offsets,
            &mut items,
        );
        let first_baseline = grid_container_baseline(
            style,
            &estimates,
            &baseline_resolutions,
            &items,
            GridBaselineSet::First,
        );
        let last_baseline = grid_container_baseline(
            style,
            &estimates,
            &baseline_resolutions,
            &items,
            GridBaselineSet::Last,
        );
        let baselines = PhysicalBaselineSets {
            vertical: BaselinePair {
                first: first_baseline
                    .map(|baseline| PhysicalTopBaselineOffset::new(layout_pt(baseline))),
                last: last_baseline
                    .map(|baseline| PhysicalTopBaselineOffset::new(layout_pt(baseline))),
            },
            ..PhysicalBaselineSets::default()
        };
        Some(GridLayout {
            height: config
                .reported_height
                .unwrap_or_else(|| PhysicalContentHeight::new(content_box_pt(final_grid_height))),
            baselines,
            first_baseline,
            last_baseline,
            items,
            baseline_resolutions,
            gap_gutters,
            column_line_offsets,
            row_line_offsets,
            column_line_names,
            row_line_names,
            column_track_sizes,
            row_track_sizes,
        })
    }
}

fn grid_has_margin_trim(style: &ComputedStyle) -> bool {
    let trim = style.margin_trim;
    trim.block_start || trim.block_end || trim.inline_start || trim.inline_end
}

/// Derive a Grid item's trimmed physical margins from its placed logical area.
///
/// CSS Grid trims every item in the edge track.  Taffy reports one-indexed
/// physical row/column line numbers, so translate those back through the
/// container writing mode before comparing the outer tracks. Empty tracks are
/// retained, while only zero-sized unoccupied `auto-fit` tracks are ignored.
/// <https://drafts.csswg.org/css-box-4/#margin-trim-grid>.
fn grid_margin_trim_plan(style: &ComputedStyle, layout: &GridLayout) -> MarginTrimPlan {
    let mut plan = MarginTrimPlan::for_item_count(layout.items.len());
    let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
    let swaps_axes = axes.swaps_physical_axes();
    let physical_spans = layout
        .items
        .iter()
        .filter_map(|item| item.area)
        .collect::<Vec<_>>();
    let (inline_spans, block_spans, inline_sizes, block_sizes) = if swaps_axes {
        (
            physical_spans
                .iter()
                .map(|area| (area.row_start, area.row_end))
                .collect::<Vec<_>>(),
            physical_spans
                .iter()
                .map(|area| (area.column_start, area.column_end))
                .collect::<Vec<_>>(),
            layout.row_track_sizes.as_slice(),
            layout.column_track_sizes.as_slice(),
        )
    } else {
        (
            physical_spans
                .iter()
                .map(|area| (area.column_start, area.column_end))
                .collect::<Vec<_>>(),
            physical_spans
                .iter()
                .map(|area| (area.row_start, area.row_end))
                .collect::<Vec<_>>(),
            layout.column_track_sizes.as_slice(),
            layout.row_track_sizes.as_slice(),
        )
    };
    let (inline_first, inline_last) =
        grid_margin_trim_edge_lines(&style.grid_template_columns, inline_sizes, &inline_spans);
    let (block_first, block_last) =
        grid_margin_trim_edge_lines(&style.grid_template_rows, block_sizes, &block_spans);

    for (index, item) in layout.items.iter().enumerate() {
        let Some(area) = item.area else {
            continue;
        };
        let (inline_start, inline_end, block_start, block_end) = if swaps_axes {
            (
                area.row_start,
                area.row_end,
                area.column_start,
                area.column_end,
            )
        } else {
            (
                area.column_start,
                area.column_end,
                area.row_start,
                area.row_end,
            )
        };
        if style.margin_trim.inline_start && inline_start == inline_first {
            plan.trim(index, axes.physical_side(LogicalSide::InlineStart));
        }
        if style.margin_trim.inline_end && inline_end == inline_last {
            plan.trim(index, axes.physical_side(LogicalSide::InlineEnd));
        }
        if style.margin_trim.block_start && block_start == block_first {
            plan.trim(index, axes.physical_side(LogicalSide::BlockStart));
        }
        if style.margin_trim.block_end && block_end == block_last {
            plan.trim(index, axes.physical_side(LogicalSide::BlockEnd));
        }
    }
    plan
}

/// Return the first and last non-collapsed grid lines for one logical axis.
///
/// Outside `auto-fit`, even a zero-sized or empty track remains relevant to
/// margin adjacency. In an `auto-fit` repetition, CSS Grid collapses only
/// empty repeated tracks; an occupied zero-sized track must still be kept.
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
fn grid_margin_trim_edge_lines(
    tracks: &css::GridTrackList,
    sizes: &[f32],
    areas: &[(u16, u16)],
) -> (u16, u16) {
    let last_line = u16::try_from(sizes.len().saturating_add(1)).unwrap_or(u16::MAX);
    if !grid_track_list_has_auto_fit(tracks) {
        return (1, last_line);
    }
    let occupied = |track_index: usize| {
        let line = u16::try_from(track_index.saturating_add(1)).unwrap_or(u16::MAX);
        areas
            .iter()
            .any(|(start, end)| *start <= line && line < *end)
    };
    let first = sizes
        .iter()
        .enumerate()
        .find(|(index, size)| size.abs() > 0.01 || occupied(*index))
        .and_then(|(index, _)| u16::try_from(index.saturating_add(1)).ok())
        .unwrap_or(1);
    let last = sizes
        .iter()
        .enumerate()
        .rev()
        .find(|(index, size)| size.abs() > 0.01 || occupied(*index))
        .and_then(|(index, _)| u16::try_from(index.saturating_add(2)).ok())
        .unwrap_or(last_line);
    (first, last)
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

/// Convert a Grid item's automatic minimum for Taffy after applying Grid's
/// eligibility conditions. Taffy's generic `auto` minimum is content based,
/// but Grid explicitly zeroes that minimum for a multi-track span containing
/// a flexible track.
/// <https://www.w3.org/TR/css-grid-1/#min-size-auto>
fn grid_item_taffy_min_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    physical_axis: GridAxis,
    container_style: &ComputedStyle,
    item_style: &ComputedStyle,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> taffy_layout::Dimension {
    if value.is_auto()
        && grid_item_spans_flexible_track_on_physical_axis(
            container_style,
            item_style,
            physical_axis,
        )
    {
        return taffy_layout::Dimension::length(0.0);
    }
    taffy_grid_item_min_dimension(
        value,
        GridPercentageBasis::indefinite(),
        min_content,
        max_content,
    )
}

/// Whether an explicitly counted multi-track span crosses a flexible track.
///
/// The Grid automatic-minimum rule is deliberately based on track sizing
/// functions, not the final numeric track sizes. The full placement engine
/// remains Taffy's responsibility; this early decision is only needed for a
/// span encoded directly in the item style, before Taffy consumes the leaf's
/// minimum contribution.
fn grid_item_spans_flexible_track_on_physical_axis(
    container_style: &ComputedStyle,
    item_style: &ComputedStyle,
    physical_axis: GridAxis,
) -> bool {
    let swaps_axes = WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes();
    let (start, end, tracks) = match (physical_axis, swaps_axes) {
        (GridAxis::Column, false) => (
            &item_style.grid_column_start,
            &item_style.grid_column_end,
            &container_style.grid_template_columns,
        ),
        (GridAxis::Row, false) => (
            &item_style.grid_row_start,
            &item_style.grid_row_end,
            &container_style.grid_template_rows,
        ),
        (GridAxis::Column, true) => (
            &item_style.grid_row_start,
            &item_style.grid_row_end,
            &container_style.grid_template_rows,
        ),
        (GridAxis::Row, true) => (
            &item_style.grid_column_start,
            &item_style.grid_column_end,
            &container_style.grid_template_columns,
        ),
    };
    grid_placement_explicit_span(start, end).is_some_and(|span| span > 1)
        && grid_track_list_has_flexible_track(tracks)
}

fn grid_placement_explicit_span(
    start: &css::GridPlacement,
    end: &css::GridPlacement,
) -> Option<u16> {
    match (start, end) {
        (css::GridPlacement::Line(_), css::GridPlacement::Span(span))
        | (css::GridPlacement::Span(span), css::GridPlacement::Line(_)) => span.count(),
        _ => None,
    }
}

fn grid_track_list_has_flexible_track(tracks: &css::GridTrackList) -> bool {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return false;
    };
    components
        .iter()
        .any(grid_track_component_has_flexible_track)
}

fn grid_track_component_has_flexible_track(component: &css::GridTrackListComponent) -> bool {
    match component {
        css::GridTrackListComponent::Track(_, track) => {
            matches!(track.max, css::GridMaxTrackBreadth::Flex(_))
        }
        css::GridTrackListComponent::Repeat(_, repeat) => repeat
            .tracks
            .iter()
            .any(grid_track_component_has_flexible_track),
    }
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

pub(super) struct GridLayoutPassConfig {
    pub(super) width: PhysicalContentWidth,
    pub(super) root_height: Option<PhysicalContentHeight>,
    pub(super) item_height_basis: GridPercentageBasis,
    pub(super) row_gap_basis: GridPercentageBasis,
    pub(super) reported_height: Option<PhysicalContentHeight>,
    /// Resolved real-item areas retained while sizing-only descendant proxy
    /// leaves are included in a second parent track-sizing pass.
    pub(super) item_placement_overrides: Vec<Option<GridItemArea>>,
    /// Baseline shims derived from the placement topology. These affect only
    /// Taffy's intrinsic Grid sizing margins, never the replayed item style.
    pub(super) baseline_plan: Option<GridBaselinePlan>,
}

/// Convert a preliminary physical Grid area into a fixed Taffy placement for
/// the proxy sizing pass. The area was produced by the preceding placement
/// pass, so its lines are valid Taffy grid lines.
fn taffy_grid_area_line(
    area: GridItemArea,
    axis: GridAxis,
) -> Option<taffy_layout::Line<taffy_layout::GridPlacement<String>>> {
    let (start, end) = match axis {
        GridAxis::Column => (area.column_start, area.column_end),
        GridAxis::Row => (area.row_start, area.row_end),
    };
    Some(taffy_layout::Line {
        start: taffy_layout::line(i16::try_from(start).ok()?),
        end: taffy_layout::line(i16::try_from(end).ok()?),
    })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GridBaselineSet {
    First,
    Last,
}

/// The result of resolving one Grid baseline-alignment request before
/// measured baseline geometry is applied.
///
/// CSS Grid excludes a baseline-aligned item when the item's size in the
/// relevant axis depends on an intrinsically sized track in the same axis.
/// Recording that decision separately from the eventual baseline coordinate
/// keeps Grid's track sizing, self-alignment, and exported baselines from
/// disagreeing after tracks have been stretched:
/// <https://www.w3.org/TR/css-grid-1/#row-align> and
/// <https://www.w3.org/TR/css-grid-1/#algo-content-alignment>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridBaselineParticipation {
    NotRequested,
    Shares(GridBaselineSet),
    Fallback {
        baseline_set: GridBaselineSet,
        reason: GridBaselineFallbackReason,
    },
}

impl GridBaselineParticipation {
    fn shares(self, baseline_set: GridBaselineSet) -> bool {
        self == Self::Shares(baseline_set)
    }

    fn requests(self, baseline_set: GridBaselineSet) -> bool {
        matches!(
            self,
            Self::Shares(requested)
                | Self::Fallback {
                    baseline_set: requested,
                    ..
                } if requested == baseline_set
        )
    }

    fn fallback_set(self) -> Option<GridBaselineSet> {
        match self {
            Self::Fallback { baseline_set, .. } => Some(baseline_set),
            Self::NotRequested | Self::Shares(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridBaselineFallbackReason {
    CyclicTrackSizing,
    IncompatibleWritingMode,
}

/// All baseline requests associated with one item, retained in the grid's
/// logical axes even when the physical Taffy adapter swaps those axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GridBaselineResolution {
    row_self: GridBaselineParticipation,
    column_self: GridBaselineParticipation,
    row_content: GridBaselineParticipation,
    column_content: GridBaselineParticipation,
}

/// A virtual margin used only while Grid sizes intrinsic tracks for a
/// baseline-sharing group.  CSS Grid calls this a baseline "shim"; keeping it
/// distinct from used margins prevents the sizing contribution from leaking
/// into item replay.
/// <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
#[derive(Debug, Clone, Copy, Default)]
struct GridBaselineSizingShim {
    top: f32,
    bottom: f32,
    left: f32,
    right: f32,
}

/// Baseline decisions shared by the Grid sizing and final-alignment phases.
///
/// The vector is indexed by the real Grid item index, so sizing-only subgrid
/// contribution leaves cannot accidentally acquire an item's baseline shim.
#[derive(Debug, Clone, Default)]
pub(super) struct GridBaselinePlan {
    shims: Vec<GridBaselineSizingShim>,
}

impl GridBaselinePlan {
    fn shim(&self, index: usize) -> Option<GridBaselineSizingShim> {
        self.shims.get(index).copied()
    }

    fn is_empty(&self) -> bool {
        self.shims.iter().all(|shim| {
            shim.top == 0.0 && shim.bottom == 0.0 && shim.left == 0.0 && shim.right == 0.0
        })
    }
}

/// Construct the Grid track-sizing shims for baseline-aligned row groups.
///
/// Taffy's Grid implementation uses physical rows/columns, while the
/// measured text baselines currently available to this adapter are physical
/// top-edge offsets.  Therefore this phase contributes row shims for a
/// horizontal grid; vertical/column baseline geometry remains represented by
/// the participation resolution and falls back safely until its physical
/// baseline table is measured by the inline adapter.
fn grid_baseline_plan(
    container_style: &ComputedStyle,
    _children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    resolutions: &[GridBaselineResolution],
    items: &[GridItemLayout],
) -> GridBaselinePlan {
    let mut plan = GridBaselinePlan {
        shims: vec![GridBaselineSizingShim::default(); items.len()],
    };
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return plan;
    }
    for baseline_set in [GridBaselineSet::First, GridBaselineSet::Last] {
        let mut groups = Vec::<(u16, u16)>::new();
        for item in items {
            let Some(area) = item.area else {
                continue;
            };
            let group = grid_baseline_group_key(area, baseline_set);
            if !groups.contains(&group) {
                groups.push(group);
            }
        }
        for (row_start, row_end) in groups {
            let participants = items
                .iter()
                .enumerate()
                .filter(|(index, item)| {
                    grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
                        && resolutions[*index]
                            .self_alignment(GridAxis::Row)
                            .shares(baseline_set)
                })
                .collect::<Vec<_>>();
            if participants.len() < 2 {
                continue;
            }
            let greatest_distance = participants
                .iter()
                .map(|(index, item)| {
                    let baseline =
                        grid_item_border_box_baseline(&estimates[*index], item, baseline_set);
                    match baseline_set {
                        GridBaselineSet::First => baseline,
                        GridBaselineSet::Last => (item.height() - baseline).max(0.0),
                    }
                })
                .fold(0.0_f32, f32::max);
            for (index, item) in participants {
                let baseline = grid_item_border_box_baseline(&estimates[index], item, baseline_set);
                let distance = match baseline_set {
                    GridBaselineSet::First => baseline,
                    GridBaselineSet::Last => (item.height() - baseline).max(0.0),
                };
                let shim = (greatest_distance - distance).max(0.0);
                match baseline_set {
                    GridBaselineSet::First => plan.shims[index].top = shim,
                    GridBaselineSet::Last => plan.shims[index].bottom = shim,
                }
            }
        }
    }
    plan
}

/// Convert authored margins to the sizing-only margin model with an optional
/// Grid baseline shim.  Grid resolves cyclic margin percentages before this
/// boundary, so any non-auto edge can safely become one fixed Taffy length.
fn grid_taffy_margin_with_baseline_shim<Source: Copy>(
    style: &ComputedStyle,
    percentage_basis: LogicalInlinePercentageBasis<Source>,
    shim: Option<GridBaselineSizingShim>,
) -> taffy_layout::Rect<taffy_layout::LengthPercentageAuto> {
    let mut margin = taffy_bridge::margin(
        style,
        percentage_basis,
        taffy_bridge::TaffyCyclicPercentage::ResolveToLengthComponent,
    );
    let Some(shim) = shim else {
        return margin;
    };
    fn add(
        value: taffy_layout::LengthPercentageAuto,
        amount: f32,
    ) -> taffy_layout::LengthPercentageAuto {
        if amount == 0.0 || value.is_auto() {
            return value;
        }
        taffy_layout::LengthPercentageAuto::length(
            value.resolve_to_option(0.0, |_, _| 0.0).unwrap_or(0.0) + amount,
        )
    }
    margin.top = add(margin.top, shim.top);
    margin.bottom = add(margin.bottom, shim.bottom);
    margin.left = add(margin.left, shim.left);
    margin.right = add(margin.right, shim.right);
    margin
}

impl GridBaselineResolution {
    fn self_alignment(self, axis: GridAxis) -> GridBaselineParticipation {
        match axis {
            GridAxis::Row => self.row_self,
            GridAxis::Column => self.column_self,
        }
    }

    /// Return the fallback required for the grid item's own content alignment.
    ///
    /// The cyclic exclusion is a used-value decision, so replay must receive
    /// the fallback rather than the authored baseline keyword:
    /// <https://www.w3.org/TR/css-grid-1/#row-align>.
    pub(super) fn content_alignment_fallback(self, axis: GridAxis) -> Option<GridBaselineSet> {
        match axis {
            GridAxis::Row => self.row_content,
            GridAxis::Column => self.column_content,
        }
        .fallback_set()
    }
}

#[derive(Clone, Copy)]
enum GridBaselineAlignmentSource {
    SelfAlignment,
    ContentAlignment,
}

fn resolve_grid_baseline_participation(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    items: &[GridItemLayout],
    available_space: GridPhysicalAvailableSpace,
) -> Vec<GridBaselineResolution> {
    children
        .iter()
        .zip(items)
        .map(|(child, item)| GridBaselineResolution {
            row_self: resolve_grid_item_baseline_participation(
                container_style,
                &child.style,
                item.area,
                GridAxis::Row,
                GridBaselineAlignmentSource::SelfAlignment,
                available_space,
            ),
            column_self: resolve_grid_item_baseline_participation(
                container_style,
                &child.style,
                item.area,
                GridAxis::Column,
                GridBaselineAlignmentSource::SelfAlignment,
                available_space,
            ),
            row_content: resolve_grid_item_baseline_participation(
                container_style,
                &child.style,
                item.area,
                GridAxis::Row,
                GridBaselineAlignmentSource::ContentAlignment,
                available_space,
            ),
            column_content: resolve_grid_item_baseline_participation(
                container_style,
                &child.style,
                item.area,
                GridAxis::Column,
                GridBaselineAlignmentSource::ContentAlignment,
                available_space,
            ),
        })
        .collect()
}

fn resolve_grid_item_baseline_participation(
    container_style: &ComputedStyle,
    child_style: &ComputedStyle,
    area: Option<GridItemArea>,
    axis: GridAxis,
    source: GridBaselineAlignmentSource,
    available_space: GridPhysicalAvailableSpace,
) -> GridBaselineParticipation {
    let Some(baseline_set) =
        grid_requested_baseline_set(container_style, child_style, axis, source)
    else {
        return GridBaselineParticipation::NotRequested;
    };
    let Some(area) = area else {
        return GridBaselineParticipation::Fallback {
            baseline_set,
            reason: GridBaselineFallbackReason::CyclicTrackSizing,
        };
    };
    if grid_item_axis_depends_on_intrinsic_track(
        container_style,
        child_style,
        area,
        axis,
        available_space,
    ) {
        return GridBaselineParticipation::Fallback {
            baseline_set,
            reason: GridBaselineFallbackReason::CyclicTrackSizing,
        };
    }
    if child_style.writing_mode != WritingMode::HorizontalTb {
        return GridBaselineParticipation::Fallback {
            baseline_set,
            reason: GridBaselineFallbackReason::IncompatibleWritingMode,
        };
    }
    GridBaselineParticipation::Shares(baseline_set)
}

fn grid_requested_baseline_set(
    container_style: &ComputedStyle,
    child_style: &ComputedStyle,
    axis: GridAxis,
    source: GridBaselineAlignmentSource,
) -> Option<GridBaselineSet> {
    match source {
        GridBaselineAlignmentSource::SelfAlignment => match axis {
            GridAxis::Row => grid_self_alignment_baseline_set(
                effective_grid_align_self(child_style, container_style).keyword,
            ),
            GridAxis::Column => grid_self_alignment_baseline_set(
                effective_grid_justify_self(child_style, container_style).keyword,
            ),
        },
        GridBaselineAlignmentSource::ContentAlignment => match axis {
            GridAxis::Row => grid_content_alignment_baseline_set(child_style.align_content.keyword),
            GridAxis::Column => {
                grid_content_alignment_baseline_set(child_style.justify_content.keyword)
            }
        },
    }
}

fn grid_self_alignment_baseline_set(keyword: SelfAlignmentKeyword) -> Option<GridBaselineSet> {
    match keyword {
        SelfAlignmentKeyword::Baseline => Some(GridBaselineSet::First),
        SelfAlignmentKeyword::LastBaseline => Some(GridBaselineSet::Last),
        _ => None,
    }
}

fn grid_content_alignment_baseline_set(
    keyword: ContentAlignmentKeyword,
) -> Option<GridBaselineSet> {
    match keyword {
        ContentAlignmentKeyword::Baseline => Some(GridBaselineSet::First),
        ContentAlignmentKeyword::LastBaseline => Some(GridBaselineSet::Last),
        _ => None,
    }
}

/// Returns whether the item must know the resolved size of an intrinsic track
/// before it can determine its own size in the requested logical axis.
///
/// This is deliberately based on the pre-alignment track functions and the
/// original grid area, never on the final stretched track size. CSS Grid says
/// the presence of this cycle is invariant over the course of layout:
/// <https://www.w3.org/TR/css-grid-1/#row-align>.
fn grid_item_axis_depends_on_intrinsic_track(
    container_style: &ComputedStyle,
    child_style: &ComputedStyle,
    area: GridItemArea,
    axis: GridAxis,
    available_space: GridPhysicalAvailableSpace,
) -> bool {
    let physical_axis = grid_physical_axis(container_style, axis);
    grid_item_size_depends_on_track(child_style, physical_axis)
        && grid_area_has_intrinsic_track(
            container_style,
            area,
            axis,
            grid_physical_axis_is_definite(physical_axis, available_space),
        )
}

fn grid_physical_axis(container_style: &ComputedStyle, axis: GridAxis) -> PhysicalAxis {
    let logical_axis = match axis {
        GridAxis::Column => LogicalAxis::Inline,
        GridAxis::Row => LogicalAxis::Block,
    };
    WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .physical_axis(logical_axis)
}

fn grid_physical_axis_is_definite(
    axis: PhysicalAxis,
    available_space: GridPhysicalAvailableSpace,
) -> bool {
    match axis {
        PhysicalAxis::Horizontal => available_space.width_basis.points().is_some(),
        PhysicalAxis::Vertical => available_space.height_basis.points().is_some(),
    }
}

fn grid_item_size_depends_on_track(style: &ComputedStyle, axis: PhysicalAxis) -> bool {
    let values = match axis {
        PhysicalAxis::Horizontal => [
            &style.box_values.width,
            &style.box_values.min_width,
            &style.box_values.max_width,
        ],
        PhysicalAxis::Vertical => [
            style.box_values.height.value(),
            &style.box_values.min_height,
            &style.box_values.max_height,
        ],
    };
    values.into_iter().any(grid_box_size_value_depends_on_track)
}

fn grid_box_size_value_depends_on_track(value: &css::ComputedLengthPercentageOrAuto) -> bool {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => value.contains_percentage(),
        css::ComputedLengthPercentageOrAuto::FitContent(Some(value)) => value.contains_percentage(),
        // `calc-size()` can retain either a percentage or an intrinsic basis
        // until Grid has selected the item's track-sized box. Treating it as
        // track-dependent is conservative and prevents a cycle from being
        // reintroduced as this syntax gains more used-value support.
        css::ComputedLengthPercentageOrAuto::CalcSize(_) => true,
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(None)
        | css::ComputedLengthPercentageOrAuto::Stretch => false,
    }
}

fn grid_area_has_intrinsic_track(
    style: &ComputedStyle,
    area: GridItemArea,
    axis: GridAxis,
    axis_is_definite: bool,
) -> bool {
    let (start, end) = match axis {
        GridAxis::Column => (area.column_start, area.column_end),
        GridAxis::Row => (area.row_start, area.row_end),
    };
    (usize::from(start).saturating_sub(1)..usize::from(end).saturating_sub(1)).any(|index| {
        grid_track_at(style, axis, index)
            .is_some_and(|track| grid_track_is_intrinsic(track, axis_is_definite))
    })
}

fn grid_track_at(
    style: &ComputedStyle,
    axis: GridAxis,
    index: usize,
) -> Option<&css::GridTrackSize> {
    let (tracks, auto_tracks) = match axis {
        GridAxis::Column => (&style.grid_template_columns, &style.grid_auto_columns),
        GridAxis::Row => (&style.grid_template_rows, &style.grid_auto_rows),
    };
    let explicit_count = grid_track_list_count(tracks)?;
    if index < explicit_count {
        return grid_explicit_track_at(tracks, index);
    }
    auto_tracks.get((index - explicit_count) % auto_tracks.len().max(1))
}

fn grid_track_list_count(tracks: &css::GridTrackList) -> Option<usize> {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return Some(0);
    };
    grid_track_component_count(components)
}

fn grid_track_component_count(components: &[css::GridTrackListComponent]) -> Option<usize> {
    components
        .iter()
        .try_fold(0_usize, |count, component| match component {
            css::GridTrackListComponent::Track(_, _) => count.checked_add(1),
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let css::GridRepeatCount::Number(repetitions) = repeat.count else {
                    return None;
                };
                count.checked_add(
                    grid_track_component_count(&repeat.tracks)?
                        .checked_mul(usize::from(repetitions))?,
                )
            }
        })
}

fn grid_explicit_track_at(
    tracks: &css::GridTrackList,
    index: usize,
) -> Option<&css::GridTrackSize> {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return None;
    };
    grid_track_component_at(components, index)
}

fn grid_track_component_at(
    components: &[css::GridTrackListComponent],
    mut index: usize,
) -> Option<&css::GridTrackSize> {
    for component in components {
        match component {
            css::GridTrackListComponent::Track(_, track) => {
                if index == 0 {
                    return Some(track);
                }
                index -= 1;
            }
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let css::GridRepeatCount::Number(repetitions) = repeat.count else {
                    return None;
                };
                let repeated_count = grid_track_component_count(&repeat.tracks)?;
                let total_count = repeated_count.checked_mul(usize::from(repetitions))?;
                if index < total_count {
                    return grid_track_component_at(&repeat.tracks, index % repeated_count);
                }
                index -= total_count;
            }
        }
    }
    None
}

fn grid_track_is_intrinsic(track: &css::GridTrackSize, axis_is_definite: bool) -> bool {
    matches!(
        track.min,
        css::GridMinTrackBreadth::Auto
            | css::GridMinTrackBreadth::MinContent
            | css::GridMinTrackBreadth::MaxContent
    ) || matches!(
        track.max,
        css::GridMaxTrackBreadth::Auto
            | css::GridMaxTrackBreadth::MinContent
            | css::GridMaxTrackBreadth::MaxContent
            | css::GridMaxTrackBreadth::FitContent(_)
            | css::GridMaxTrackBreadth::Flex(_) if !axis_is_definite
    ) || matches!(
        (&track.min, &track.max),
        (
            css::GridMinTrackBreadth::LengthPercentage(min),
            css::GridMaxTrackBreadth::LengthPercentage(max),
        ) if !axis_is_definite && (min.contains_percentage() || max.contains_percentage())
    )
}

struct GridBaselineAlignmentContext<'a, 'box_tree> {
    children: &'a [GridChild<'box_tree>],
    estimates: &'a [GridItemEstimate],
    resolutions: &'a [GridBaselineResolution],
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
        let metrics = item.used_box_metrics().unwrap_or_else(|| {
            used_box_metrics(
                child_style,
                PercentageBasis::definite(layout_pt(container_width.points())),
            )
        });
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
            child_style.box_values.height.value().clone(),
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

/// Restore the intrinsic used size of an automatically sized replaced Grid
/// item after track sizing. A replaced item with `align-self: normal` is not
/// stretch-fit like an ordinary block; its intrinsic dimensions may overflow a
/// zero-breadth `minmax(auto, 0)` track without contributing that size to the
/// track itself.
/// <https://www.w3.org/TR/css-grid-1/#algo-single-span-items>
/// <https://www.w3.org/TR/css-align-3/#valdef-justify-self-normal>
pub(super) fn apply_grid_replaced_item_size_corrections(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    items: &mut [GridItemLayout],
) {
    for ((child, estimate), item) in children.iter().zip(estimates).zip(items) {
        if !child.style.box_values.width.is_auto()
            || !child.style.box_values.height.value().is_auto()
        {
            continue;
        }
        let Some(used_size) = estimate.replaced_used_size else {
            continue;
        };
        // Unlike `normal`, an explicit (or inherited) stretch alignment
        // supplies the final grid-area size to a replaced item. An intrinsic
        // fallback must not restore the image after that zero-sized area was
        // deliberately selected by `minmax(auto, 0)`.
        // <https://www.w3.org/TR/css-align-3/#valdef-justify-self-stretch>
        if matches!(
            effective_grid_justify_self(&child.style, container_style).keyword,
            SelfAlignmentKeyword::Stretch
        ) || matches!(
            effective_grid_align_self(&child.style, container_style).keyword,
            SelfAlignmentKeyword::Stretch
        ) {
            continue;
        }
        let width = used_size.width.points().max(0.0);
        let height = used_size.height.points().max(0.0);
        if width == 0.0 || height == 0.0 || (item.width() > 0.0 && item.height() > 0.0) {
            continue;
        }
        // A zero-breadth track can omit its trailing Taffy line from the
        // detailed layout record. The item's resolved start position remains
        // authoritative, however, and is precisely where a non-stretch
        // replaced item is aligned by the preceding self-alignment phase.
        item.set_axis_geometry(GridAxis::Column, item.x(), width);
        item.set_axis_geometry(GridAxis::Row, item.y(), height);
    }
}

/// Resolve cyclic grid-item percentage sizes after final grid-area placement.
///
/// Grid must treat a percentage that depends on an intrinsic track as `auto`
/// while determining track contributions. Once tracks are placed, the same
/// preferred, minimum, and maximum size values resolve against the final grid
/// area. Keep that phase transition here rather than using the grid container
/// as Taffy's percentage basis.
/// <https://www.w3.org/TR/css-grid-1/#percentage-sizing>
/// <https://www.w3.org/TR/css-grid-1/#grid-item-sizing>
struct GridFinalItemPercentagePlacement<'a> {
    container_style: &'a ComputedStyle,
    container_width: PhysicalContentWidth,
    container_height: f32,
    column_line_offsets: &'a [f32],
    row_line_offsets: &'a [f32],
}

fn apply_grid_deferred_percentage_item_size_corrections(
    placement: GridFinalItemPercentagePlacement<'_>,
    children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    items: &mut [GridItemLayout],
) {
    for ((child, estimate), item) in children.iter().zip(estimates).zip(items) {
        let Some(area) = item.area else {
            continue;
        };
        let child_style = &child.style;
        let (
            horizontal_alignment,
            vertical_alignment,
            horizontal_content_alignment,
            vertical_content_alignment,
        ) = if WritingModeAxes::new(
            placement.container_style.writing_mode,
            placement.container_style.direction,
        )
        .swaps_physical_axes()
        {
            (
                effective_grid_align_self(child_style, placement.container_style),
                effective_grid_justify_self(child_style, placement.container_style),
                placement.container_style.align_content,
                placement.container_style.justify_content,
            )
        } else {
            (
                effective_grid_justify_self(child_style, placement.container_style),
                effective_grid_align_self(child_style, placement.container_style),
                placement.container_style.justify_content,
                placement.container_style.align_content,
            )
        };
        let Some(area_x) = content_aligned_grid_line_offset(
            horizontal_content_alignment,
            placement.container_width.points(),
            placement.column_line_offsets,
            usize::from(area.column_start).saturating_sub(1),
        ) else {
            continue;
        };
        let Some(area_right) = content_aligned_grid_line_offset(
            horizontal_content_alignment,
            placement.container_width.points(),
            placement.column_line_offsets,
            usize::from(area.column_end).saturating_sub(1),
        ) else {
            continue;
        };
        let Some(area_y) = content_aligned_grid_line_offset(
            vertical_content_alignment,
            placement.container_height,
            placement.row_line_offsets,
            usize::from(area.row_start).saturating_sub(1),
        ) else {
            continue;
        };
        let Some(area_bottom) = content_aligned_grid_line_offset(
            vertical_content_alignment,
            placement.container_height,
            placement.row_line_offsets,
            usize::from(area.row_end).saturating_sub(1),
        ) else {
            continue;
        };
        let area_width = (area_right - area_x).max(0.0);
        let area_height = (area_bottom - area_y).max(0.0);
        let final_size = resolve_grid_item_final_percentage_size(
            child,
            estimate,
            item,
            area_width,
            area_height,
            placement.container_width,
        );
        if let Some(width) = final_size.width {
            item.mark_final_percentage_axis(GridAxis::Column);
            item.set_axis_geometry(
                GridAxis::Column,
                grid_item_aspect_axis_position(
                    area_x,
                    area_right,
                    width.points(),
                    horizontal_alignment.keyword,
                ),
                width.points(),
            );
        }
        if let Some(height) = final_size.height {
            item.mark_final_percentage_axis(GridAxis::Row);
            item.set_axis_geometry(
                GridAxis::Row,
                grid_item_aspect_axis_position(
                    area_y,
                    area_bottom,
                    height.points(),
                    vertical_alignment.keyword,
                ),
                height.points(),
            );
        }
    }
}

/// Border-box dimensions resolved after a Grid item's final area is known.
///
/// A cyclic percentage contributes as `auto` while tracks are intrinsic-sized,
/// but its preferred/minimum/maximum constraint applies to the final area.
/// This value is deliberately independent of an item's final origin so Grid
/// Lanes can use it while determining its perpendicular packing extent.
/// <https://www.w3.org/TR/css-grid-1/#percentage-sizing>
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct GridItemFinalPercentageSize {
    pub(super) width: Option<BorderBoxLength>,
    pub(super) height: Option<BorderBoxLength>,
}

pub(super) fn resolve_grid_item_final_percentage_size(
    child: &GridChild<'_>,
    estimate: &GridItemEstimate,
    item: &GridItemLayout,
    area_width: f32,
    area_height: f32,
    container_width: PhysicalContentWidth,
) -> GridItemFinalPercentageSize {
    let child_style = &child.style;
    let metrics = item.used_box_metrics().unwrap_or_else(|| {
        used_box_metrics(
            child_style,
            PercentageBasis::definite(layout_pt(container_width.points())),
        )
    });
    let horizontal_non_content = metrics.horizontal_non_content_length();
    let vertical_non_content = metrics.vertical_non_content_length();
    let width = grid_item_axis_has_percentage(
        &child_style.box_values.width,
        &child_style.box_values.min_width,
        &child_style.box_values.max_width,
    )
    .then(|| {
        let content_width = used_content_box_size(
            child_style.box_values.width.clone(),
            child_style.box_sizing,
            PercentageBasis::definite(content_box_pt(area_width)),
            horizontal_non_content,
        )
        .map(SemanticLengthExt::points)
        .unwrap_or_else(|| {
            (estimate.physical_measurements().content_width.points()
                - horizontal_non_content.points())
            .max(0.0)
        });
        BorderBoxLength::new(
            constrain_content_width(
                child_style,
                content_box_pt(content_width),
                PercentageBasis::definite(layout_pt(area_width)),
            )
            .points()
                + horizontal_non_content.points(),
        )
    });
    let height = grid_item_axis_has_percentage(
        child_style.box_values.height.value(),
        &child_style.box_values.min_height,
        &child_style.box_values.max_height,
    )
    .then(|| {
        let content_height = used_content_box_size(
            child_style.box_values.height.value().clone(),
            child_style.box_sizing,
            PercentageBasis::definite(content_box_pt(area_height)),
            vertical_non_content,
        )
        .map(SemanticLengthExt::points)
        .unwrap_or_else(|| {
            (estimate.physical_measurements().content_height.points()
                - vertical_non_content.points())
            .max(0.0)
        });
        BorderBoxLength::new(
            constrain_content_height(
                child_style,
                content_box_pt(content_height),
                PercentageBasis::definite(layout_pt(area_height)),
            )
            .points()
                + vertical_non_content.points(),
        )
    });
    GridItemFinalPercentageSize { width, height }
}

fn grid_item_axis_has_percentage(
    preferred: &css::ComputedLengthPercentageOrAuto,
    minimum: &css::ComputedLengthPercentageOrAuto,
    maximum: &css::ComputedLengthPercentageOrAuto,
) -> bool {
    [preferred, minimum, maximum].into_iter().any(|value| {
        matches!(
            value,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.contains_percentage()
        )
    })
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
    resolutions: &[GridBaselineResolution],
    row_line_offsets: &[f32],
    items: &mut [GridItemLayout],
) {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return;
    }
    let context = GridBaselineAlignmentContext {
        children,
        estimates,
        resolutions,
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
            || !context.resolutions[index]
                .self_alignment(GridAxis::Row)
                .shares(baseline_set)
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
    if participant_count < 2 {
        // CSS Box Alignment falls a baseline self-alignment request back to
        // safe self-start/self-end when no compatible sharing group remains.
        // That includes items excluded by Grid's intrinsic-track cycle rule.
        // Taffy cannot perform this fallback from Quire's measured baseline
        // metadata, so apply it to every requesting border box here.
        // <https://www.w3.org/TR/css-align-3/#baseline-align-self>
        for (index, item) in items.iter_mut().enumerate() {
            if !grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
                || !context.resolutions[index]
                    .self_alignment(GridAxis::Row)
                    .requests(baseline_set)
            {
                continue;
            }
            apply_grid_baseline_self_alignment_fallback(
                item,
                &context.children[index].style,
                context.row_line_offsets,
                row_start,
                row_end,
                baseline_set,
            );
        }
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
            || !context.resolutions[index]
                .self_alignment(GridAxis::Row)
                .shares(baseline_set)
        {
            continue;
        }
        let baseline = grid_item_border_box_baseline(&context.estimates[index], item, baseline_set);
        item.set_axis_geometry(GridAxis::Row, target_baseline - baseline, item.height());
    }
    // Cyclic and otherwise incompatible baseline requests remain fallback
    // aligned even when their row still has a compatible sharing group.
    for (index, item) in items.iter_mut().enumerate() {
        if !grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
            || !context.resolutions[index]
                .self_alignment(GridAxis::Row)
                .requests(baseline_set)
            || context.resolutions[index]
                .self_alignment(GridAxis::Row)
                .shares(baseline_set)
        {
            continue;
        }
        apply_grid_baseline_self_alignment_fallback(
            item,
            &context.children[index].style,
            context.row_line_offsets,
            row_start,
            row_end,
            baseline_set,
        );
    }
}

fn apply_grid_baseline_self_alignment_fallback(
    item: &mut GridItemLayout,
    child_style: &ComputedStyle,
    row_line_offsets: &[f32],
    row_start: u16,
    row_end: u16,
    baseline_set: GridBaselineSet,
) {
    let area_start = row_line_offsets
        .get(usize::from(row_start).saturating_sub(1))
        .copied()
        .unwrap_or(item.y());
    let area_end = row_line_offsets
        .get(usize::from(row_end).saturating_sub(1))
        .copied()
        .unwrap_or(item.y() + item.height());
    let y = match baseline_set {
        GridBaselineSet::First => {
            area_start
                + item
                    .used_box_metrics()
                    .map(|metrics| metrics.margin.top.points())
                    .unwrap_or(child_style.margin.top)
        }
        GridBaselineSet::Last => {
            area_end
                - item
                    .used_box_metrics()
                    .map(|metrics| metrics.margin.bottom.points())
                    .unwrap_or(child_style.margin.bottom)
                - item.height().max(0.0)
        }
    };
    item.set_axis_geometry(GridAxis::Row, y, item.height());
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
    estimates: &[GridItemEstimate],
    resolutions: &[GridBaselineResolution],
    items: &[GridItemLayout],
    baseline_set: GridBaselineSet,
) -> Option<f32> {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return None;
    }
    let row_index = grid_container_baseline_row(items, baseline_set)?;
    // CSS Grid's exported baseline has an ordered fallback chain.  In
    // particular, a last-baseline sharing group is still preferable to an
    // arbitrary item baseline when no first-baseline group exists in the
    // relevant edge track.
    // <https://drafts.csswg.org/css-grid-2/#grid-baselines>
    for requested_set in [baseline_set, grid_opposite_baseline_set(baseline_set)] {
        if let Some((index, item)) = items.iter().enumerate().find(|(index, item)| {
            grid_item_is_container_baseline_eligible(item, row_index, requested_set)
                && resolutions[*index]
                    .self_alignment(GridAxis::Row)
                    .shares(requested_set)
        }) {
            return Some(
                item.y() + grid_item_border_box_baseline(&estimates[index], item, requested_set),
            );
        }
    }
    if let Some((index, item)) = items.iter().enumerate().find(|(index, item)| {
        grid_item_is_container_baseline_eligible(item, row_index, baseline_set)
            && grid_item_has_baseline(&estimates[*index], baseline_set)
    }) {
        return Some(
            item.y() + grid_item_border_box_baseline(&estimates[index], item, baseline_set),
        );
    }
    // The final fallback is the first Grid item in grid order, not merely an
    // item intersecting the edge track. Its absent baseline is synthesized
    // from its border box by `grid_item_border_box_baseline`.
    items.iter().enumerate().find_map(|(index, item)| {
        item.area.map(|_| {
            item.y() + grid_item_border_box_baseline(&estimates[index], item, baseline_set)
        })
    })
}

fn grid_opposite_baseline_set(baseline_set: GridBaselineSet) -> GridBaselineSet {
    match baseline_set {
        GridBaselineSet::First => GridBaselineSet::Last,
        GridBaselineSet::Last => GridBaselineSet::First,
    }
}

fn grid_item_is_container_baseline_eligible(
    item: &GridItemLayout,
    row_index: u16,
    baseline_set: GridBaselineSet,
) -> bool {
    item.area.is_some_and(|area| match baseline_set {
        GridBaselineSet::First => area.row_start == row_index,
        GridBaselineSet::Last => area.row_end.saturating_sub(1) == row_index,
    })
}

fn grid_item_has_baseline(estimate: &GridItemEstimate, baseline_set: GridBaselineSet) -> bool {
    match baseline_set {
        GridBaselineSet::First => estimate.first_baseline.is_some(),
        GridBaselineSet::Last => estimate.last_baseline.is_some(),
    }
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
    fn static_pdf_grid_overlay_keeps_track_and_outer_sizes_identical() {
        let mut style = ComputedStyle::initial();
        style.overflow_x = css::Overflow::Scroll;
        style.overflow_y = css::Overflow::Scroll;
        let reservation = grid_scrollbar_reservation(&style);

        assert_eq!(
            grid_scrollport_content_width(
                PhysicalContentWidth::new(content_box_pt(100.0)),
                reservation,
            )
            .points(),
            100.0,
        );
        assert_eq!(
            grid_scrollport_content_height_basis(
                Some(PhysicalContentHeight::new(content_box_pt(100.0))),
                reservation,
            )
            .unwrap()
            .points(),
            100.0,
        );
        assert_eq!(
            grid_total_content_height(
                PhysicalContentHeight::new(content_box_pt(100.0)),
                None,
                reservation,
            )
            .points(),
            100.0,
        );
        assert_eq!(
            grid_total_content_height(
                PhysicalContentHeight::new(content_box_pt(85.0)),
                Some(PhysicalContentHeight::new(content_box_pt(100.0))),
                reservation,
            )
            .points(),
            100.0,
        );
    }

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
            baselines: PhysicalBaselineSets::default(),
            first_baseline: None,
            last_baseline: None,
            items: Vec::new(),
            baseline_resolutions: Vec::new(),
            gap_gutters: GapDecorationGridGutters::default(),
            column_line_offsets: Vec::new(),
            row_line_offsets: Vec::new(),
            column_line_names: Vec::new(),
            row_line_names: Vec::new(),
            column_track_sizes: Vec::new(),
            row_track_sizes: Vec::new(),
        };

        let _: PhysicalContentHeight = layout.height;
        assert_eq!(layout.height.points(), 60.0);
    }

    fn track(min: css::GridMinTrackBreadth, max: css::GridMaxTrackBreadth) -> css::GridTrackSize {
        css::GridTrackSize { min, max }
    }

    fn grid_tracks(tracks: Vec<css::GridTrackSize>) -> css::GridTrackList {
        css::GridTrackList::Tracks {
            components: tracks
                .into_iter()
                .map(|track| css::GridTrackListComponent::Track(Vec::new(), track))
                .collect(),
            trailing_names: Vec::new(),
        }
    }

    fn baseline_child_with_height_percent() -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Baseline);
        style.box_values.height = css::PhysicalHeight::from_computed(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(0.2),
            ),
        );
        style
    }

    fn first_row_area() -> GridItemArea {
        GridItemArea {
            row_start: 1,
            row_end: 2,
            column_start: 1,
            column_end: 2,
        }
    }

    fn available_grid_space(block_size: Option<f32>) -> GridPhysicalAvailableSpace {
        GridPhysicalAvailableSpace {
            width_basis: grid_percentage_basis(
                Some(content_box_pt(100.0)),
                GridAvailableSizeSource::ContainerInlineSize,
            ),
            height_basis: grid_percentage_basis(
                block_size.map(content_box_pt),
                GridAvailableSizeSource::ContainerBlockSize,
            ),
        }
    }

    fn anonymous_grid_child_with_style(style: ComputedStyle) -> GridChild<'static> {
        let source = FormattingContextChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: style.clone(),
        };
        let used_style = css::LayoutStyle::from_computed(&style).into_zoomed();
        GridUsedItem::from_source(source, used_style)
    }

    #[test]
    fn final_grid_area_resolves_mixed_percentage_sizes_before_constraints() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(2.0), 1.0, true),
        );
        style.box_values.min_width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );
        style.box_values.max_width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.75),
        );
        let child = anonymous_grid_child_with_style(style);
        let item = GridItemLayout::new(
            GridRect::new(GridPoint::new(0.0, 0.0), GridSize::new(10.0, 10.0)),
            None,
        );

        let size = resolve_grid_item_final_percentage_size(
            &child,
            &GridItemEstimate::fixed(10.0, 10.0),
            &item,
            100.0,
            80.0,
            PhysicalContentWidth::new(content_box_pt(100.0)),
        );

        // `calc(2pt + 100%)` resolves to 102pt against the final area, then
        // the final 75% maximum constrains the used border-box width.
        assert_eq!(size.width.map(SemanticLengthExt::points), Some(75.0));
        assert_eq!(size.height, None);
    }

    #[test]
    fn flexible_track_span_zeros_only_the_automatic_grid_minimum() {
        let mut container = ComputedStyle::initial();
        container.grid_template_columns = grid_tracks(vec![track(
            css::GridMinTrackBreadth::Auto,
            css::GridMaxTrackBreadth::Flex(1.0),
        )]);
        let mut item = ComputedStyle::initial();
        item.grid_column_start = css::GridPlacement::Line(css::GridLinePlacement::Number(
            std::num::NonZeroI32::new(1).unwrap(),
        ));
        item.grid_column_end = css::GridPlacement::Span(css::GridSpanPlacement::Count(
            std::num::NonZeroU16::new(2).unwrap(),
        ));

        assert_eq!(
            grid_item_taffy_min_dimension(
                css::ComputedLengthPercentageOrAuto::Auto,
                GridAxis::Column,
                &container,
                &item,
                content_box_pt(20.0),
                content_box_pt(40.0),
            ),
            taffy_layout::Dimension::length(0.0),
        );
        assert_eq!(
            grid_item_taffy_min_dimension(
                css::ComputedLengthPercentageOrAuto::LengthPercentage(
                    css::ComputedLengthPercentage::from_points(12.0),
                ),
                GridAxis::Column,
                &container,
                &item,
                content_box_pt(20.0),
                content_box_pt(40.0),
            ),
            taffy_layout::Dimension::length(12.0),
        );
    }

    #[test]
    fn percentage_item_in_intrinsic_row_uses_permanent_baseline_fallback() {
        let container = ComputedStyle::initial();
        let mut child = baseline_child_with_height_percent();
        child.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Auto);
        child.align_content = css::AlignContent::new(ContentAlignmentKeyword::Baseline);

        let resolution = resolve_grid_item_baseline_participation(
            &container,
            &child,
            Some(first_row_area()),
            GridAxis::Row,
            GridBaselineAlignmentSource::ContentAlignment,
            available_grid_space(Some(100.0)),
        );

        assert_eq!(
            resolution,
            GridBaselineParticipation::Fallback {
                baseline_set: GridBaselineSet::First,
                reason: GridBaselineFallbackReason::CyclicTrackSizing,
            }
        );
    }

    #[test]
    fn percentage_item_in_fixed_row_can_share_its_baseline() {
        let mut container = ComputedStyle::initial();
        container.grid_template_rows = grid_tracks(vec![track(
            css::GridMinTrackBreadth::LengthPercentage(css::ComputedLengthPercentage::from_points(
                40.0,
            )),
            css::GridMaxTrackBreadth::LengthPercentage(css::ComputedLengthPercentage::from_points(
                40.0,
            )),
        )]);
        let child = baseline_child_with_height_percent();

        assert_eq!(
            resolve_grid_item_baseline_participation(
                &container,
                &child,
                Some(first_row_area()),
                GridAxis::Row,
                GridBaselineAlignmentSource::SelfAlignment,
                available_grid_space(Some(100.0)),
            ),
            GridBaselineParticipation::Shares(GridBaselineSet::First)
        );
    }

    #[test]
    fn indefinite_flexible_row_is_intrinsic_for_baseline_cycle_detection() {
        let mut container = ComputedStyle::initial();
        container.grid_template_rows = grid_tracks(vec![track(
            css::GridMinTrackBreadth::Auto,
            css::GridMaxTrackBreadth::Flex(1.0),
        )]);
        let child = baseline_child_with_height_percent();

        assert!(matches!(
            resolve_grid_item_baseline_participation(
                &container,
                &child,
                Some(first_row_area()),
                GridAxis::Row,
                GridBaselineAlignmentSource::SelfAlignment,
                available_grid_space(None),
            ),
            GridBaselineParticipation::Fallback {
                reason: GridBaselineFallbackReason::CyclicTrackSizing,
                ..
            }
        ));
    }

    #[test]
    fn first_and_last_baseline_requests_retain_their_fallback_edges() {
        let container = ComputedStyle::initial();
        let mut child = baseline_child_with_height_percent();
        let first = resolve_grid_item_baseline_participation(
            &container,
            &child,
            Some(first_row_area()),
            GridAxis::Row,
            GridBaselineAlignmentSource::SelfAlignment,
            available_grid_space(Some(100.0)),
        );
        child.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::LastBaseline);
        let last = resolve_grid_item_baseline_participation(
            &container,
            &child,
            Some(first_row_area()),
            GridAxis::Row,
            GridBaselineAlignmentSource::SelfAlignment,
            available_grid_space(Some(100.0)),
        );

        assert!(first.requests(GridBaselineSet::First));
        assert!(!first.requests(GridBaselineSet::Last));
        assert!(last.requests(GridBaselineSet::Last));
        assert!(!last.requests(GridBaselineSet::First));
        assert_eq!(first.fallback_set(), Some(GridBaselineSet::First));
        assert_eq!(last.fallback_set(), Some(GridBaselineSet::Last));
    }

    #[test]
    fn vertical_grid_projects_row_dependency_to_physical_width() {
        let mut container = ComputedStyle::initial();
        container.writing_mode = WritingMode::VerticalLr;
        let mut child = ComputedStyle::initial();
        child.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Baseline);
        child.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.2),
        );

        assert!(matches!(
            resolve_grid_item_baseline_participation(
                &container,
                &child,
                Some(first_row_area()),
                GridAxis::Row,
                GridBaselineAlignmentSource::SelfAlignment,
                available_grid_space(Some(100.0)),
            ),
            GridBaselineParticipation::Fallback {
                reason: GridBaselineFallbackReason::CyclicTrackSizing,
                ..
            }
        ));
    }

    fn baseline_test_item(area: GridItemArea, y: f32, height: f32) -> GridItemLayout {
        GridItemLayout::new(
            GridRect::new(GridPoint::new(0.0, y), GridSize::new(20.0, height)),
            Some(area),
        )
    }

    fn baseline_test_resolution(baseline_set: GridBaselineSet) -> GridBaselineResolution {
        GridBaselineResolution {
            row_self: GridBaselineParticipation::Shares(baseline_set),
            column_self: GridBaselineParticipation::NotRequested,
            row_content: GridBaselineParticipation::NotRequested,
            column_content: GridBaselineParticipation::NotRequested,
        }
    }

    #[test]
    fn baseline_shims_equalize_first_baselines_without_used_margins() {
        let style = ComputedStyle::initial();
        let items = vec![
            baseline_test_item(
                GridItemArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 1,
                    column_end: 2,
                },
                0.0,
                30.0,
            ),
            baseline_test_item(
                GridItemArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 2,
                    column_end: 3,
                },
                0.0,
                30.0,
            ),
        ];
        let mut first = GridItemEstimate::fixed(20.0, 30.0);
        first.first_baseline = Some(8.0);
        let mut second = GridItemEstimate::fixed(20.0, 30.0);
        second.first_baseline = Some(14.0);
        let plan = grid_baseline_plan(
            &style,
            &[],
            &[first, second],
            &[
                baseline_test_resolution(GridBaselineSet::First),
                baseline_test_resolution(GridBaselineSet::First),
            ],
            &items,
        );

        assert_eq!(plan.shim(0).unwrap().top, 6.0);
        assert_eq!(plan.shim(1).unwrap().top, 0.0);
        assert_eq!(plan.shim(0).unwrap().bottom, 0.0);
    }

    #[test]
    fn grid_container_baseline_prefers_last_group_before_item_baseline() {
        let style = ComputedStyle::initial();
        let items = vec![baseline_test_item(
            GridItemArea {
                row_start: 1,
                row_end: 2,
                column_start: 1,
                column_end: 2,
            },
            10.0,
            30.0,
        )];
        let mut estimate = GridItemEstimate::fixed(20.0, 30.0);
        estimate.first_baseline = Some(4.0);
        estimate.last_baseline = Some(20.0);
        let resolution = baseline_test_resolution(GridBaselineSet::Last);

        assert_eq!(
            grid_container_baseline(
                &style,
                &[estimate],
                &[resolution],
                &items,
                GridBaselineSet::First,
            ),
            Some(30.0)
        );
    }
}
