use super::gap_decorations::{
    GridGapFragmentProjection, grid_fragment_source_range_from_bounds,
    grid_gap_decoration_primitives_for_page, grid_used_track_extent,
};
use super::lanes::grid_lanes_stacking_axis_is_block;
use super::model::{GridItemLayout, GridLayout};
use super::*;
use crate::layout::assets::FragmentainerOrdinal;
use crate::layout::baseline::PhysicalTopBaselineOffset;
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
        principal_box_paint_mode: PrincipalBoxPaintMode,
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
        // An enclosing transform, opacity, containment, or positioned effect
        // context owns this grid's final local outline phase. Ordinary grid
        // containers otherwise promote their outline into the parent
        // normal-flow phase below.
        let defer_own_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = false;

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
        // Grid remains a normal-flow block before its tracks are sized. A
        // vertical grid's physical `width:auto` is its automatic logical
        // block-size, so it must use the shared content-derived measurement
        // rather than the horizontal fill-available equation.
        // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
        // <https://www.w3.org/TR/css-grid-2/#grid-container-size>
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
                auto_width_role: BlockAutoWidthRole::NormalFlow,
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
                let next_context = self.resolved_page_context(
                    self.destination_document_page_number(self.pages.len() + 2),
                    false,
                );
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
                principal_box_paint_mode,
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
                self.containing_block_direction,
            );
            self.cursor_y = placement.origin.top_y();
            outer_x = placement.origin.x() + style.margin.left + relative_offset.x();
            inner_x = outer_x + border_widths.left + style.padding.left;
        } else {
            self.cursor_y = self
                .resolve_block_clearance(BlockClearanceRequest::coincident_edges(
                    style.clear,
                    FloatPlacementAxes::new(
                        self.containing_block_writing_mode,
                        self.containing_block_direction,
                    ),
                    PageTopBlockPosition::new(self.cursor_y),
                ))
                .used_border_edge
                .points();
        }

        let border_box_inline_span = PageInlineSpan::new(outer_x, outer_width);
        let block_top = self.cursor_y;
        let paint_page_index = self.pages.len();
        let paint_checkpoint = self.current_page.paint_checkpoint();
        self.cursor_y -= border_widths.top + style.padding.top;
        let content_top = self.cursor_y;
        let fragment_decoration_reservation = FragmentDecorationReservation::new(
            FragmentDecoration::for_box_decoration_break(style.box_decoration_break, false, false),
            non_content_pt(border_widths.top + style.padding.top),
            non_content_pt(style.padding.bottom + border_widths.bottom),
        );
        let Some(mut grid_layout) = self.compute_grid_layout(
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
        let content_fragmentainer =
            self.fragmentainer_from_page_cursor(PageTopBlockPosition::new(content_top));
        let initial_raw_fragmentainer_extent = content_fragmentainer.available_block_size();
        let continuation_raw_fragmentainer_extent =
            content_fragmentainer.fragmentainer_block_size();
        // The container plan owns destination fragmentainers. A cloned grid
        // item expands its own source span into that destination coordinate
        // system, but must not reduce the grid container's capacity: doing
        // both would reserve the item's clone edges twice.
        // <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
        let initial_grid_content_capacity = fragment_decoration_reservation
            .remaining_content_extent(initial_raw_fragmentainer_extent);
        let continuation_grid_content_capacity = fragment_decoration_reservation
            .fresh_content_extent(continuation_raw_fragmentainer_extent);
        for (item, child) in grid_layout.items.iter_mut().zip(&children) {
            let decoration = FragmentDecoration::for_box_decoration_break(
                child.style.box_decoration_break,
                false,
                false,
            );
            if !decoration.is_clone() {
                continue;
            }
            let borders = used_border_widths(&child.style);
            let reservation = FragmentDecorationReservation::new(
                decoration,
                non_content_pt(borders.top + child.style.padding.top),
                non_content_pt(child.style.padding.bottom + borders.bottom),
            );
            let source_height = (item.height()
                - reservation.block_start().points()
                - reservation.block_end().points())
            .max(0.0);
            item.configure_cloned_fragment_source(source_height, reservation);
            item.project_cloned_fragment_destinations(
                initial_raw_fragmentainer_extent,
                continuation_raw_fragmentainer_extent,
            );
        }
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
        let fragmentation_content_height = grid_layout
            .items
            .iter()
            .map(|item| item.y() + item.fragmentation_height())
            .fold(total_content_height, f32::max);
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
            GridFragmentPlan::from_grid_item_boundaries_with_content_capacity(
                fragmentainer_kind,
                GridFragmentContentCapacity::new(
                    GridFragmentBlockSize::new(initial_grid_content_capacity.points()),
                    GridFragmentBlockSize::new(continuation_grid_content_capacity.points()),
                ),
                fragmentation_content_height,
                &grid_layout.rows.line_offsets(),
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
        // A grid container owns its principal fragments independently of its
        // out-of-flow descendants.  An absolutely positioned child does not
        // contribute a grid item, but it must not make a definite grid box
        // monolithic or suppress its continuation fragments.
        // <https://www.w3.org/TR/css-grid-1/#abspos-items>
        // <https://www.w3.org/TR/css-grid-1/#pagination>
        let can_replay_committed_fragment_records =
            grid_fragment_plan.requires_multiple_fragments();
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
                    column_line_offsets: &grid_layout.columns.line_offsets(),
                    row_line_offsets: &grid_layout.rows.line_offsets(),
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
        // Resolve direct positioned children while the first grid fragment is
        // current. Their static-position rectangle is in the continuous grid
        // source coordinate system, and their positioned paint transaction
        // retains its owning destination page while the normal-flow grid
        // replay materializes later fragments below.
        // <https://www.w3.org/TR/css-position-3/#static-position>
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
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
                    column_line_offsets: &grid_layout.columns.line_offsets(),
                    row_line_offsets: &grid_layout.rows.line_offsets(),
                    positioning_containing_block: establishes_positioning_containing_block
                        .then(|| self.containing_blocks.last().copied())
                        .flatten(),
                },
            );
        }
        let mut committed_gap_fragment_paint_bounds = Vec::new();
        let committed_replay_end_cursor = if can_replay_committed_fragment_records {
            // A sliced item has one continuous source paint tree. Keep it
            // across committed fragment records so a long grid item is not
            // repeatedly laid out and does not repeatedly clone document
            // pages through a rollback snapshot.
            let mut split_item_replay = std::iter::repeat_with(|| None)
                .take(grid_layout.items.len())
                .collect::<Vec<Option<ContinuousSourceReplay>>>();
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
                    // A continuation starts below the grid container's own
                    // cloned block-start border and padding. Ancestor
                    // continuation insets are already present in
                    // `content_top`; subtract only the grid's reservation.
                    fragment_cursor =
                        transition.cursor_after_fragmentainer_advance(PageTopBlockPosition::new(
                            content_top - fragment_decoration_reservation.block_start().points(),
                        ));
                }
                // A committed grid slice owns a principal fragment even when
                // it has no in-flow item ink.  Mark it before the next
                // fragmentainer transition so `push_page` does not coalesce
                // this otherwise paintless source fragment away; the
                // principal background and border are attached below.
                // <https://www.w3.org/TR/css-break-3/#box-fragment>
                self.mark_current_page_flow_content();
                let paint_checkpoint = self.current_page.paint_checkpoint();
                self.replay_grid_fragment_record_items(
                    *fragment_record,
                    style,
                    &grid_layout,
                    &children,
                    &grid_layout.items,
                    stylesheets,
                    inner_x,
                    PageInlineSpan::new(inner_x, inner_width),
                    fragment_cursor,
                    &mut split_item_replay,
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
        let contents_overflow_edge = (overflow_clip_active || needs_contoured_overflow_clip)
            .then(|| {
                resolve_overflow_clip_edge(
                    paint_space_rect(outer_x, block_bottom, outer_width, block_height),
                    style,
                    border_widths,
                    self.used_overflow_axes_for_element(element, style),
                    containment.clips_descendant_paint(),
                    None,
                )
            })
            .flatten();
        let contents_overflow_clip = contents_overflow_edge.as_ref().map(|edge| edge.clip.bounds);
        let contoured_contents_overflow_clip = contents_overflow_edge
            .as_ref()
            .filter(|edge| edge.clips_x && edge.clips_y)
            .map(|edge| edge.clip.clone())
            .filter(|clip| !matches!(clip.contour, BoxContentContour::Rect));
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
        if principal_box_paint_mode.root_paints()
            && style.visibility == Visibility::Visible
            && block_height > 0.0
        {
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
        let mut decorated_grid_fragment_pages = Vec::new();
        for (page_index, mut fragment) in fragments {
            // A grid item establishes an independent formatting context, but
            // its background still paints as in-flow descendant content of
            // the grid container. Promote its captured background before
            // applying the container's overflow scope so axis longhands clip
            // stretched item backgrounds just as they do for block children.
            // <https://www.w3.org/TR/css-grid-1/#grid-items> and
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>.
            fragment.promote_background_border_to_in_flow_block();
            fragment.promote_outline_to_in_flow_outline();
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
                let committed_principal_fragment = planned_fragment_record.map(|fragment_record| {
                    let is_first = fragment_record.slice.source_block_start.points() <= 0.01;
                    let is_last = !matches!(
                        fragment_record.slice.break_after,
                        GridFragmentBreak::RowBoundary | GridFragmentBreak::ForcedRowBoundary
                    );
                    let decoration = FragmentDecoration::for_box_decoration_break(
                        style.box_decoration_break,
                        is_first,
                        is_last,
                    );
                    let cursor = fragment_record.cursor(PageTopBlockPosition::new(content_top));
                    let border_box = if decoration.is_clone() {
                        cursor.decorated_paint_clip(
                            fragment_record.slice,
                            border_box_inline_span,
                            fragment_decoration_reservation,
                        )
                    } else {
                        fragment_record.paint_clip(border_box_inline_span, cursor)
                    };
                    fragment_record.principal_box_fragment(
                        FragmentainerOrdinal::new(page_index),
                        border_box,
                        decoration,
                    )
                });
                if committed_principal_fragment.is_some() {
                    decorated_grid_fragment_pages.push(page_index);
                }
                if let Some(fragment_bounds) = committed_principal_fragment
                    .as_ref()
                    .and_then(|fragment| fragment.kind().principal_box())
                    .map(DecoratedBoxFragment::border_box)
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
                    if principal_box_paint_mode.root_paints()
                        && style.visibility == Visibility::Visible
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
                    if principal_box_paint_mode.root_paints()
                        && style.visibility == Visibility::Visible
                    {
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
                                gutters: &grid_layout.gap_decoration_gutters(style),
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
                    if principal_box_paint_mode.root_paints()
                        && style.visibility == Visibility::Visible
                    {
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
                if principal_box_paint_mode.root_paints() && style.visibility == Visibility::Visible
                {
                    if committed_gap_fragment_paint_bounds.is_empty() {
                        fragment.append_primitives_in_band(
                            PaintBand::BackgroundBorder,
                            grid_gap_decoration_primitives(
                                style,
                                GapDecorationContainer::new(
                                    inner_x,
                                    content_top,
                                    grid_used_track_extent(
                                        &grid_layout.columns.line_offsets(),
                                        &grid_layout.items,
                                        GridAxis::Column,
                                        inner_width,
                                    ),
                                    grid_used_track_extent(
                                        &grid_layout.rows.line_offsets(),
                                        &grid_layout.items,
                                        GridAxis::Row,
                                        total_content_height,
                                    ),
                                ),
                                &grid_gap_items,
                                &grid_layout.gap_decoration_gutters(style),
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
                                        gutters: &grid_layout.gap_decoration_gutters(style),
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
            if !defer_own_decoration_promotion {
                fragment.promote_outline_to_in_flow_outline();
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
        // Committing a grid source slice creates a principal box fragment even
        // when that slice has no item ink. In particular, a fixed-size empty
        // grid and an empty continuation still paint cloned background,
        // border, shadow, and outline. Descendant capture cannot be the
        // authority for that ownership: an empty paint tree is not a
        // descendant-overflow-only continuation.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        // <https://www.w3.org/TR/css-break-3/#break-decoration>
        if grid_spanned_pages
            && principal_box_paint_mode.root_paints()
            && style.visibility == Visibility::Visible
        {
            for fragment_record in &grid_fragment_records {
                let page_index = paint_page_index + fragment_record.fragmentainer_offset;
                if decorated_grid_fragment_pages.contains(&page_index) {
                    continue;
                }
                let is_first = fragment_record.slice.source_block_start.points() <= 0.01;
                let is_last = !matches!(
                    fragment_record.slice.break_after,
                    GridFragmentBreak::RowBoundary | GridFragmentBreak::ForcedRowBoundary
                );
                let decoration = FragmentDecoration::for_box_decoration_break(
                    style.box_decoration_break,
                    is_first,
                    is_last,
                );
                let cursor = fragment_record.cursor(PageTopBlockPosition::new(content_top));
                let border_box = if decoration.is_clone() {
                    cursor.decorated_paint_clip(
                        fragment_record.slice,
                        border_box_inline_span,
                        fragment_decoration_reservation,
                    )
                } else {
                    fragment_record.paint_clip(border_box_inline_span, cursor)
                };
                let committed_fragment = fragment_record.principal_box_fragment(
                    FragmentainerOrdinal::new(page_index),
                    border_box,
                    decoration,
                );
                let principal = committed_fragment
                    .kind()
                    .principal_box()
                    .expect("committed grid source slice owns a principal box");
                let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
                let border_rect = principal.border_box().paint_rect();
                fragment.prepend_primitives_in_band(
                    PaintBand::BackgroundBorder,
                    self.box_background_primitives(border_rect, style),
                );
                fragment.append_primitives_in_band(
                    PaintBand::Outline,
                    self.box_outline_primitives(border_rect, style),
                );
                if !defer_own_decoration_promotion {
                    fragment.promote_outline_to_in_flow_outline();
                }
                let fragment = PaintFragment::from_stacking_context_in_band(
                    PaintBand::InFlowBlock,
                    PaintStackingContext::from_banded_fragment(fragment, Vec::new())
                        .with_source_order(self.next_paint_source_order()),
                );
                if page_index < self.pages.len() {
                    self.pages[page_index]
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                } else if page_index == self.pages.len() {
                    self.current_page
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                } else {
                    self.pending_paint_fragments.push(PendingPaintFragment {
                        page_index,
                        fragment,
                        kind: PendingPaintFragmentKind::InFlowOverflow,
                    });
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

        // A definite block size is available before the inline-grid's
        // shrink-to-fit width is selected.  Use it when refining the
        // intrinsic inline contribution below: resolved rows can change a
        // child's inline contribution, and therefore the atomic inline-grid
        // width itself.
        // <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
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

        let (mut min_width, mut max_width) = if intrinsic_physical_width_is_contained(style) {
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
        if let Some(width) = self.grid_inline_intrinsic_width_with_resolved_rows(
            style,
            &children,
            stylesheets,
            definite_content_height,
        ) {
            min_width = min_width.max(width);
            max_width = max_width.max(width).max(min_width);
        }
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
            style.clone_used_style(),
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

    /// Refine an inline-grid's intrinsic physical width after resolving its
    /// definite row tracks.
    ///
    /// The scalar intrinsic-track probe is necessarily cyclic for an
    /// auto-sized inline-grid. When its physical block size is definite, the
    /// Grid algorithm requires one row-to-column correction before that
    /// scalar result becomes the atomic inline size.  A zero inline available
    /// size keeps automatic tracks in their intrinsic sizing mode; the final
    /// line offset is the corrected intrinsic extent rather than a
    /// fill-available container width.
    /// <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
    fn grid_inline_intrinsic_width_with_resolved_rows(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        block_size: Option<PhysicalContentHeight>,
    ) -> Option<f32> {
        let block_size = block_size?;
        let layout = self.compute_grid_layout(
            style,
            children,
            stylesheets,
            PhysicalContentWidth::new(content_box_pt(0.0)),
            Some(block_size),
            GridLayoutPurpose::IntrinsicProbe,
        )?;
        // The inline-grid atomic path currently consumes a physical width.
        // In horizontal writing modes, that is exactly the resolved column
        // extent. Vertical writing modes retain the existing logical-axis
        // intrinsic sizing path, which does not use this physical correction.
        (!WritingModeAxes::new(style.writing_mode, style.used_direction()).swaps_physical_axes())
            .then(|| {
                layout
                    .columns
                    .line_offsets()
                    .last()
                    .copied()
                    .unwrap_or_default()
                    .max(0.0)
            })
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

        // Resolve an authored block size before selecting the inline-grid's
        // shrink-to-fit inline size.  The Grid sizing algorithm's bounded
        // row-to-column feedback can make that intrinsic size larger.
        // <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
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

        let (mut min_width, mut max_width) = if intrinsic_physical_width_is_contained(style) {
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
        if let Some(width) = self.grid_inline_intrinsic_width_with_resolved_rows(
            style,
            &children,
            stylesheets,
            definite_content_height,
        ) {
            min_width = min_width.max(width);
            max_width = max_width.max(width).max(min_width);
        }
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
        // An inline-grid's off-page atom capture is not an ancestor list
        // item's principal line layout. Its descendant lines must not consume
        // an outside marker awaiting the real parent line.
        let pending_outside_marker_anchors = self.pending_outside_marker_anchors.suspend();
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
                    column_line_offsets: &grid_layout.columns.line_offsets(),
                    row_line_offsets: &grid_layout.rows.line_offsets(),
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
                    column_line_offsets: &grid_layout.columns.line_offsets(),
                    row_line_offsets: &grid_layout.rows.line_offsets(),
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
                            &grid_layout.columns.line_offsets(),
                            &grid_layout.items,
                            GridAxis::Column,
                            inner_width,
                        ),
                        grid_used_track_extent(
                            &grid_layout.rows.line_offsets(),
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
                    &grid_layout.gap_decoration_gutters(style),
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
            style.used_style(),
            containment.layout,
            border_box_height,
            descendant_baseline,
        );
        let inline_lanes_overflow_clearance =
            grid_lanes_inline_overflow_clearance(style.used_style(), &grid_layout);
        let fixed_layers = self.fixed_layers.split_off(fixed_layer_start);
        self.restore(snapshot);
        self.pending_outside_marker_anchors
            .restore(pending_outside_marker_anchors);
        self.fixed_layers.extend(fixed_layers);
        // Grid Lanes exports its packed baseline through `grid_layout` just
        // like an ordinary grid container.  Do not rewrite an authored
        // baseline alignment here: doing so changes the line box that owns an
        // inline-grid-lanes atom rather than its exported baseline.
        // <https://drafts.csswg.org/css-grid-3/#grid-lanes-baseline-alignment>
        let mut atom_style = style.clone_used_style();
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
}
