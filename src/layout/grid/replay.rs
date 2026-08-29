use std::collections::{HashMap, HashSet};

use super::*;
use crate::layout::builder::SpeculativeLayoutTransaction;
use crate::units::Definite;

/// Replay context for one split grid item fragment.
///
/// CSS Fragmentation slices the visual fragment while keeping the source
/// item's internal layout in its original coordinate system:
/// <https://www.w3.org/TR/css-break-3/#box-splitting> and
/// <https://www.w3.org/TR/css-grid-1/#pagination>.
pub(in crate::layout::grid) struct SplitGridItemPaintContext {
    /// The used physical border-box height from CSS Grid placement.
    pub(in crate::layout::grid) item_height: BorderBoxLength,
    /// The item border box selects the continuous source paint origin.
    pub(in crate::layout::grid) item_border_box: PaintClip,
    /// The committed grid-content fragment clips visible descendant overflow.
    /// It is deliberately wider than `item_border_box` when an item has
    /// visible inline overflow such as a relatively positioned descendant.
    pub(in crate::layout::grid) fragment_content_clip: PaintClip,
    pub(in crate::layout::grid) source_item_top: PageTopBlockPosition,
    /// Item-local continuous-source offset selected by this destination
    /// fragment. This is distinct from the grid container's source cursor
    /// when a row boundary falls after an item-local trailing line.
    pub(in crate::layout::grid) source_content_start: GridFragmentBlockOffset,
}

const fn offpage_source_offset() -> f32 {
    10_000.0
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
        grid_content_inline_span: PageInlineSpan,
        cursor: GridFragmentCursor,
        split_item_replay: &mut [Option<ContinuousSourceReplay>],
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
                    fragment_record.paint_clip(grid_content_inline_span, cursor),
                    split_item_replay
                        .get_mut(item_fragment.item_index)
                        .expect("grid item replay cache matches item layout"),
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
                        item_fragment.original.grid_lanes_placement(),
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
            && let Some(context) = ResolvedSubgridContext::from_parent(
                parent_style,
                parent_layout,
                &child.style,
                area,
                item.grid_lanes_placement(),
            )
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
        #[cfg(feature = "layout-profile")]
        let _profile = crate::layout::layout_profile::grid_item_replay_scope();
        let item_height = item.height().max(0.0);
        let replay_dimensions = item.replay_dimensions();

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
                replay_dimensions,
            );
            &owned_placed_style
        };
        let item_paint_checkpoint = self.current_page.paint_checkpoint();
        let item_positioned_layer_start = self.positioned_layers.len();
        let item_page_index = self.pages.len();
        self.with_placed_formatting_context(
            PlacedFormattingContext {
                content_left: inner_x + item.x(),
                content_width: replay_dimensions.physical_content_width_for_replay(placed_style),
                content_height: (!item.preserves_cyclic_physical_height_on_replay()).then_some(
                    Definite::new(
                        replay_dimensions.physical_content_height_for_replay(placed_style),
                    ),
                ),
                table_wrapper_border_box_block_size: auto_table_wrapper_block_size_override(
                    &child.style,
                    border_box_pt(item_height),
                ),
                // A vertical Grid item's logical inline axis is its resolved
                // physical height. Replay must receive that content-box
                // measure rather than reconstructing it from the grid row.
                // <https://www.w3.org/TR/css-grid-1/#grid-items>
                // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
                replay_logical_inline_size: (!item.preserves_cyclic_physical_height_on_replay())
                    .then(|| {
                        replay_dimensions.logical_inline_content_size_for_replay(placed_style)
                    }),
                cursor_y: cursor
                    .source_block_y(GridFragmentBlockOffset::new(item.y()))
                    .points(),
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
                float_scope: ReplayFloatScope::IsolatedFormattingContext,
            },
            placed_style,
            |layout| {
                if metadata.is_some() {
                    layout.begin_assignment_capture_frame();
                }
                layout.layout_formatting_context_item_contents(
                    child,
                    placed_style,
                    stylesheets,
                    PrincipalBoxPaintMode::RootPaints,
                );
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
        let policy = if self.active_fragmentainer_kind() == FragmentainerKind::Column {
            StackingContextPolicy::for_fragmented_grid_item(placed_style, item_border_box)
        } else {
            StackingContextPolicy::for_grid_item(placed_style, item_border_box)
        };
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
        let item_height = item.fragmentation_source_height().max(0.0);
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

    #[allow(clippy::too_many_arguments)]
    fn replay_split_grid_item_fragment(
        &mut self,
        child: &GridChild<'_>,
        item_fragment: &GridItemFragment,
        baseline_resolution: Option<&GridBaselineResolution>,
        stylesheets: &Stylesheets<'_>,
        inner_x: f32,
        cursor: GridFragmentCursor,
        fragment_content_clip: PaintClip,
        cached_source_replay: &mut Option<ContinuousSourceReplay>,
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
        let replay_dimensions = item.replay_dimensions();
        let placed_style = grid_placed_item_style(
            fallback_style.as_ref().unwrap_or(&child.style),
            item,
            replay_dimensions,
        );
        let item_border_box = visible
            .page_top_rect(cursor.grid_container_origin(inner_x))
            .paint_clip();
        // A cloned item's `visible` rectangle is destination geometry. Its
        // continuous source box must restart at the current destination
        // fragment rather than being translated by the larger source offset
        // selected for earlier cloned fragments.
        // <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
        let source_item_top = cursor.source_block_y(GridFragmentBlockOffset::new(
            if item.has_cloned_fragment_projection() {
                visible.y()
            } else {
                item.y()
            },
        ));
        let source_replay = if let Some(source_replay) = cached_source_replay.as_ref() {
            source_replay
        } else {
            let source_replay = self.capture_split_grid_item_source_replay(
                child,
                &placed_style,
                stylesheets,
                replay_dimensions,
            );
            *cached_source_replay = Some(source_replay);
            cached_source_replay
                .as_ref()
                .expect("grid source replay was stored before projection")
        };
        self.replay_split_grid_item_effects(
            &source_replay.effects,
            item_fragment,
            offpage_source_offset(),
        );
        self.paint_split_grid_item_fragment(
            &source_replay.paint,
            &placed_style,
            SplitGridItemPaintContext {
                item_height: border_box_pt(item_height),
                item_border_box,
                fragment_content_clip,
                source_item_top,
                source_content_start: item_fragment.content_slice.block_start,
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
    fn capture_split_grid_item_source_replay(
        &mut self,
        child: &GridChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        replay_dimensions: GridItemReplayDimensions,
    ) -> ContinuousSourceReplay {
        let transaction = SpeculativeLayoutTransaction::begin(self);
        let positioned_layer_start = 0;
        let offpage_top = offpage_source_offset();
        self.current_page = Page::new(
            replay_dimensions.border_box_width().points().max(1.0),
            offpage_top,
        );
        self.overflow_clips.clear();
        self.fragment_top_offsets.clear();

        self.with_placed_formatting_context(
            PlacedFormattingContext {
                content_left: 0.0,
                content_width: replay_dimensions.physical_content_width_for_replay(placed_style),
                content_height: Some(Definite::new(
                    replay_dimensions.physical_content_height_for_replay(placed_style),
                )),
                table_wrapper_border_box_block_size: auto_table_wrapper_block_size_override(
                    &child.style,
                    replay_dimensions.border_box_height(),
                ),
                replay_logical_inline_size: Some(
                    replay_dimensions.logical_inline_content_size_for_replay(placed_style),
                ),
                cursor_y: offpage_top,
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
                float_scope: ReplayFloatScope::IsolatedFormattingContext,
            },
            placed_style,
            |layout| {
                layout.layout_formatting_context_item_contents(
                    child,
                    placed_style,
                    stylesheets,
                    PrincipalBoxPaintMode::RootPaints,
                );
                layout.flush_positioned_layers_since(positioned_layer_start);
            },
        );

        let paint = self.current_page.paint_fragment();
        let effects = self
            .take_positioned_scratch_side_effects()
            .into_continuous_source_effects();
        transaction.restore(self);

        ContinuousSourceReplay {
            paint,
            effects,
            source_height: replay_dimensions.physical_content_height_for_replay(placed_style),
            scratch_top: offpage_top,
        }
    }

    /// Project the source subtree's observable effects onto the committed grid
    /// fragment that contains each effect's source marker.
    ///
    /// Assignment placements are recorded in the continuous scratch item
    /// coordinate space. A half-open source interval gives a shared boundary
    /// to the later fragment only once, preventing duplicated named strings
    /// and running elements when a grid row splits exactly at a marker.
    fn replay_split_grid_item_effects(
        &mut self,
        source_effects: &DeferredLayoutSideEffects,
        item_fragment: &GridItemFragment,
        scratch_top: f32,
    ) {
        let source_slice = item_fragment.content_slice;
        let source_start = source_slice.block_start.points();
        let source_end = source_slice.block_end.points();
        let is_final_slice = !item_fragment.metadata.continues_to_next_page;
        let placement = item_fragment.metadata.assignment_placement();
        let mut assignments = Vec::new();
        for page_effects in &source_effects.page_effects {
            collect_grid_source_assignments(
                &mut assignments,
                &page_effects.named_strings,
                source_start,
                source_end,
                is_final_slice,
                scratch_top,
            );
            collect_grid_source_assignments(
                &mut assignments,
                &page_effects.running_elements,
                source_start,
                source_end,
                is_final_slice,
                scratch_top,
            );
        }
        self.replay_captured_page_assignments(&assignments, placement);

        let mut effects = source_effects.clone();
        effects.page_effects.clear();
        effects.bookmarks.retain(|bookmark| {
            source_position_is_in_grid_slice(
                bookmark.replay_target().y - scratch_top,
                source_start,
                source_end,
                is_final_slice,
            )
        });
        let selected_anchor_targets: HashSet<_> = effects
            .anchor_source_positions
            .iter()
            .filter(|(_, position)| {
                source_position_is_in_grid_slice(
                    position.y - scratch_top,
                    source_start,
                    source_end,
                    is_final_slice,
                )
            })
            .map(|(target, _)| target.as_str())
            .collect();
        // Legacy anchor producers that do not yet supply a source point keep
        // the conservative first-fragment behavior. New anchors have a
        // typed source coordinate and are emitted by the slice that owns it.
        effects.anchors.retain(|(target, _)| {
            selected_anchor_targets.contains(target.as_str())
                || (!item_fragment.metadata.continues_from_previous_page
                    && !effects
                        .anchor_source_positions
                        .iter()
                        .any(|(positioned_target, _)| positioned_target == target))
        });
        let retained_anchor_targets: HashSet<_> = effects
            .anchors
            .iter()
            .map(|(target, _)| target.as_str())
            .collect();
        effects
            .anchor_source_positions
            .retain(|(target, _)| retained_anchor_targets.contains(target.as_str()));
        effects
            .anchor_text
            .retain(|(target, _)| retained_anchor_targets.contains(target.as_str()));
        effects
            .anchor_counters
            .retain(|(target, _)| retained_anchor_targets.contains(target.as_str()));
        if effects.anchors.is_empty() && effects.bookmarks.is_empty() {
            return;
        }
        for bookmark in &mut effects.bookmarks {
            let source_target = bookmark.replay_target();
            let source_y = source_target.y - scratch_top;
            let destination = placement.border_box.map_or(source_target, |border_box| {
                paint_space_point(
                    border_box.x() + source_target.x,
                    border_box.y() + source_y - source_start,
                )
            });
            bookmark.set_replay_destination(placement.page_index, destination);
        }
        for (_, page_index) in &mut effects.anchors {
            *page_index = placement.page_index;
        }
        self.apply_deferred_layout_side_effects(effects);
    }

    fn paint_split_grid_item_fragment(
        &mut self,
        source_paint: &PaintFragment,
        placed_style: &ComputedStyle,
        context: SplitGridItemPaintContext,
    ) {
        let item_height = context.item_height.points();
        let item_border_box = context.item_border_box;
        // The container contributes the inline extent of the destination
        // fragment, while the committed item fragment alone owns its
        // block-axis source interval. Using the full grid slice here lets a
        // final short item source overpaint the following anonymous grid row.
        let fragment_content_clip = PaintClip::new(
            context.fragment_content_clip.x(),
            item_border_box.y(),
            context.fragment_content_clip.width(),
            item_border_box.height(),
        );
        let source_item_top = context.source_item_top;
        if item_border_box.width() <= 0.0 || fragment_content_clip.height() <= 0.0 {
            return;
        }
        let fragment = source_paint
            .clone()
            .translated(PaintTranslation::new(
                item_border_box.x(),
                // The continuous source canvas is item-local. Reconstruct
                // its destination top from the committed item rectangle and
                // its own source interval rather than reusing a grid-global
                // cursor that may include a preceding anonymous line.
                item_border_box.y()
                    + item_border_box.height()
                    + context.source_content_start.points()
                    - 10_000.0,
            ))
            .clipped_to_rect(fragment_content_clip);

        if fragment.is_empty() {
            return;
        }

        // The item establishes its stacking context, but visible descendant
        // overflow is clipped by the committed grid fragmentainer, not by
        // the item's own border box.
        let policy =
            StackingContextPolicy::for_fragmented_grid_item(placed_style, fragment_content_clip);
        let mut effects = policy.effects;
        effects.set_rectangular_overflow_clip(Some(fragment_content_clip));
        effects.absolute_clip = Some(fragment_content_clip);
        let source_bounds = PageTopRect::new(
            item_border_box.x(),
            source_item_top.points(),
            item_border_box.width(),
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

fn source_position_is_in_grid_slice(
    source_y: f32,
    source_start: f32,
    source_end: f32,
    is_final_slice: bool,
) -> bool {
    source_y + 0.01 >= source_start
        && if is_final_slice {
            source_y <= source_end + 0.01
        } else {
            source_y < source_end - 0.01
        }
}

fn collect_grid_source_assignments(
    output: &mut Vec<CapturedPageAssignment>,
    assignments: &HashMap<String, Vec<NamedStringAssignment>>,
    source_start: f32,
    source_end: f32,
    is_final_slice: bool,
    scratch_top: f32,
) {
    for (name, values) in assignments {
        for assignment in values {
            let source_y = assignment
                .placement
                .border_box
                .map_or(0.0, |border_box| border_box.y() - scratch_top);
            if source_position_is_in_grid_slice(source_y, source_start, source_end, is_final_slice)
            {
                output.push(CapturedPageAssignment {
                    name: name.clone(),
                    value: assignment.value.clone(),
                });
            }
        }
    }
}

fn grid_placed_item_style(
    child_style: &ComputedStyle,
    item: &GridItemLayout,
    replay_dimensions: GridItemReplayDimensions,
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
    let content_width = replay_dimensions.physical_content_width_for_replay(&placed_style);
    let content_height = replay_dimensions.physical_content_height_for_replay(&placed_style);
    set_style_used_width(&mut placed_style, content_width.points());
    if item.preserves_cyclic_physical_height_on_replay() {
        // Grid stretch has resolved the item's physical border-box placement,
        // but an auto-sized container has not supplied a definite percentage
        // basis for the item's logical inline axis. Preserve `100%` as a
        // cyclic used value instead of freezing the final grid-area height.
        // <https://drafts.csswg.org/css-grid-2/#grid-item-sizing>
        placed_style.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(1.0),
            ),
        );
    } else {
        set_style_used_height(&mut placed_style, content_height.points());
    }
    // The resolved Grid rectangle is converted to content-box used values at
    // this boundary. Further replay must retain that same box-model space.
    // <https://www.w3.org/TR/css-grid-1/#grid-item-sizing>
    placed_style.box_sizing = BoxSizing::ContentBox;
    let final_percentage_axes = item.final_percentage_axes();
    if final_percentage_axes.width {
        set_style_used_content_box_width_bounds(
            &mut placed_style,
            content_width.content_box_length(),
        );
    }
    if final_percentage_axes.height {
        set_style_used_content_box_height_bounds(
            &mut placed_style,
            content_height.content_box_length(),
        );
    }
    // Grid has already resolved this item's fixed lengths at the used-value
    // boundary. The replay dispatcher consumes the style as a geometry
    // transport, not as a cascade source, so it must not apply the item's
    // effective zoom again.
    placed_style.effective_zoom = css::EffectiveZoom::NORMAL;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment(id: usize, source_y: f32) -> NamedStringAssignment {
        NamedStringAssignment {
            id: AssignmentId(id),
            value: PageAssignmentValue::GeneratedContent(Vec::new()),
            placement: AssignmentPlacement {
                page_index: 0,
                starts_page_fragment: false,
                border_box: Some(PaintClip::new(0.0, source_y, 1.0, 1.0)),
            },
        }
    }

    #[test]
    fn split_grid_source_assignments_use_a_half_open_slice_boundary() {
        let assignments = HashMap::from([(
            "section".to_owned(),
            vec![assignment(1, 10_000.0), assignment(2, 10_050.0)],
        )]);
        let mut first = Vec::new();
        collect_grid_source_assignments(&mut first, &assignments, 0.0, 50.0, false, 10_000.0);
        let mut second = Vec::new();
        collect_grid_source_assignments(&mut second, &assignments, 50.0, 100.0, true, 10_000.0);

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].name, "section");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].name, "section");
    }
}
