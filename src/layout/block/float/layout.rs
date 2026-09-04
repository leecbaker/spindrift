use super::super::super::*;
use super::exclusions::FLOAT_EPSILON;
use super::model::*;
use super::sizing::{float_replay_style, freeze_float_replay_height, freeze_float_replay_width};
use crate::layout::assets::rasterize_generated_css_image;
use crate::layout::inline_collect::has_out_of_flow_formatting_box;

impl<'a> LayoutBuilder<'a> {
    /// Lays out one floated child in the current block formatting context.
    ///
    /// CSS 2.2 blockifies floated boxes, gives auto-width floats a
    /// shrink-to-fit width, and records their margin boxes as exclusions for
    /// later line boxes:
    /// <https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo>,
    /// <https://www.w3.org/TR/CSS22/visudet.html#float-width>, and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_floating_child(
        &mut self,
        child_element: &Element,
        child_signature: ElementSignature,
        child_style: &ComputedStyle,
        child_children: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &Stylesheets<'_>,
        placement_axes: FloatPlacementAxes,
        run: &mut FloatRunState,
    ) -> bool {
        self.layout_floating_child_with_pseudo_source(
            child_element,
            child_signature,
            child_style,
            child_children,
            table_fragment,
            stylesheets,
            placement_axes,
            run,
            None,
        )
    }

    /// Lay out a tree-abiding generated pseudo float without losing the
    /// pseudo's counter scope during its isolated float replay.
    ///
    /// CSS Pseudo-Elements places generated boxes in the originating
    /// element's box tree, and CSS 2.2 still applies float layout at that
    /// source position. <https://www.w3.org/TR/css-pseudo-4/#generated-content>
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_generated_floating_child(
        &mut self,
        child_element: &Element,
        child_signature: ElementSignature,
        child_style: &ComputedStyle,
        child_children: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &Stylesheets<'_>,
        placement_axes: FloatPlacementAxes,
        run: &mut FloatRunState,
        pseudo_source: box_tree::CounterEventSource,
    ) -> bool {
        self.layout_floating_child_with_pseudo_source(
            child_element,
            child_signature,
            child_style,
            child_children,
            table_fragment,
            stylesheets,
            placement_axes,
            run,
            Some(pseudo_source),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_floating_child_with_pseudo_source(
        &mut self,
        child_element: &Element,
        child_signature: ElementSignature,
        child_style: &ComputedStyle,
        child_children: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &Stylesheets<'_>,
        placement_axes: FloatPlacementAxes,
        run: &mut FloatRunState,
        pseudo_source: Option<box_tree::CounterEventSource>,
    ) -> bool {
        debug_assert_ne!(
            child_style.float.layout_role(),
            FloatLayoutRole::Footnote,
            "a detached GCPM footnote body must not enter CSS float placement"
        );
        if child_style.float == Float::None
            || child_style.display.is_none()
            || matches!(child_style.position, Position::Absolute | Position::Fixed)
        {
            return false;
        }
        let mut placed_style = self.style_with_current_used_lengths(child_style);
        if placed_style.display.is_inline_level() {
            placed_style.display = placed_style.display.blockified();
        }
        let specified_side = placed_style.float;
        let float_id = self.next_float_id();
        // Reserve the float's tree order before replaying its descendants.
        // Its captured paint subtree is committed after replay, while a later
        // sibling float must still paint above it in source order.
        // <https://www.w3.org/TR/CSS22/zindex.html>
        let paint_source_order = self.next_paint_source_order();
        // Intrinsic measurement and isolated replay temporarily enter the
        // floated child's formatting context. Preserve the containing block's
        // logical inline extent beside its axes so a vertical inline-end
        // float cannot later anchor against the child's measured inline size.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let placement_logical_inline_size = self.current_content_logical_inline_content_size();
        let Some(float_side) = UsedFloatSide::from_float(specified_side, placement_axes) else {
            return false;
        };
        self.push_ancestor_signature(child_signature);
        placed_style.float = Float::None;
        // The replay style participates inside the float's independent BFC,
        // but the floated principal box is out of normal flow and cannot form
        // a named-page group at the parent's class-A sibling boundaries.
        // Clear the root page value before intrinsic measurement as well as
        // final replay so speculative layout cannot create a page transition.
        // <https://www.w3.org/TR/css-break-3/#possible-breaks> and
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        placed_style.page = css::PageAssignment::Unspecified;
        if placed_style.display.is_flow() {
            placed_style.display.inner = DisplayInner::FlowRoot;
        }
        let built_child_children =
            if child_children.is_none() && !is_replaced_element(child_element) {
                Some(self.build_frozen_child_boxes_with_current_ancestors(
                    child_element,
                    stylesheets,
                    child_style,
                ))
            } else {
                None
            };
        let child_children = child_children.or(built_child_children.as_deref());

        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        apply_used_box_metrics_for_logical_inline_basis(
            &mut placed_style,
            self.current_content_logical_inline_percentage_basis(),
        );
        let _ = freeze_float_replay_height(
            &mut placed_style,
            self.block_percentage_context_stack
                .current_percentage_basis(),
            child_element.document_compatibility_mode == dom::DocumentCompatibilityMode::Quirks,
        );

        let inline_size = self.resolved_float_inline_size(
            child_element,
            &placed_style,
            stylesheets,
            containing_inline_size,
            child_children,
            table_fragment,
        );
        freeze_float_replay_width(&mut placed_style, inline_size);
        let margin_box_height = self.float_margin_box_height(
            child_element,
            &placed_style,
            stylesheets,
            inline_size,
            child_children,
            table_fragment,
        );
        // A normal prebreak is also an out-of-flow transition: retain the
        // parent fragmentainer so later in-flow siblings may still use it.
        // This is especially important for a clear-containing block whose
        // floated child is moved to a fresh page while its ordinary text fits
        // beside the preceding float on this page.
        let mut deferred_destination_snapshot = None;
        if placement_axes.writing_mode() == WritingMode::HorizontalTb
            && self.adjoining_float_origin_y.is_none()
            && margin_box_height.points() > FLOAT_EPSILON
            && margin_box_height.points() <= self.page_area_height() + FLOAT_EPSILON
            && self.cursor_y - margin_box_height.points() < self.page_bottom() - FLOAT_EPSILON
            && !self.cursor_is_at_page_top()
            && self.current_page_has_content()
        {
            deferred_destination_snapshot = Some(self.snapshot());
            let continuation = self.float_page_break_continuation_context();
            self.push_page();
            if self.active_fragmentainer_kind() == FragmentainerKind::Page {
                self.replay_fragment_continuation_on_page(&continuation, self.current_page_context);
            }
            run.reset_for_block(
                PageInlineSpan::from_edges(self.content_left, self.content_right),
                PageTopBlockPosition::new(self.cursor_y),
            );
        }
        let placement_top =
            PageTopBlockPosition::new(self.adjoining_float_origin_y.unwrap_or(self.cursor_y));
        // Negative block-start/end margins can make a float's mathematical
        // margin-box height zero or negative while its border box still
        // occupies a substantial exclusion slab. Float placement must test
        // that occupied border extent; otherwise a later overhanging float
        // is allowed to rise through an earlier float and its paint fragment
        // is clipped to a zero-height margin rectangle.
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
        let border_box_block_size =
            (margin_box_height.points() - placed_style.margin.top - placed_style.margin.bottom)
                .max(0.0);
        let placement_block_size = margin_box_height.points().max(border_box_block_size);
        let margin_box_size =
            margin_box_size_pt(inline_size.margin_box_width.points(), placement_block_size);

        let (mut margin_box_left, mut top) =
            if matches!(float_side, UsedFloatSide::Top | UsedFloatSide::Bottom) {
                {
                    let placement = self.find_vertical_float_avoiding_position(
                        placement_top,
                        margin_box_size,
                        placed_style.clear,
                        placement_axes,
                        placement_logical_inline_size,
                        Some(float_side),
                    );
                    (placement.origin.x(), placement.origin.top_y())
                }
            } else {
                let placement = self.find_inline_float_avoiding_position(
                    placement_top,
                    margin_box_size,
                    placed_style.clear,
                    placement_axes,
                    float_side,
                );
                let margin_box_left = placement
                    .inline_float_margin_box_left(float_side, inline_size.margin_box_width);
                (margin_box_left, placement.origin.top_y())
            };
        let mut logical_placement = LogicalFloatPlacement::from_physical_margin_box(
            self.current_float_page_index(),
            placement_axes.writing_mode(),
            placement_axes.direction(),
            float_side,
            PageTopRect::new(
                self.content_left,
                self.page_top(),
                (self.content_right - self.content_left).max(0.0),
                self.page_area_height(),
            ),
            PageTopRect::new(
                margin_box_left,
                top,
                inline_size.margin_box_width.points(),
                placement_block_size,
            ),
        );

        // A float is shifted downward until it fits beside earlier floats. If
        // the first available band crosses the fragmentainer edge, placement
        // continues at the next fragmentainer's block-start rather than
        // leaving the float (and following inline content) outside the page.
        // <https://www.w3.org/TR/css-break-3/#breaking-rules>
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
        // Keep the surrounding normal-flow cursor in the source
        // fragmentainer when clearance sends this out-of-flow box to the
        // next page.  The isolated replay below is then committed as a
        // deferred float fragment, just like an internally fragmented float.
        // Otherwise a following text node is incorrectly pulled onto the
        // float's destination page merely because its preceding sibling was
        // out of flow.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        if placement_axes.writing_mode() == WritingMode::HorizontalTb
            && deferred_destination_snapshot.is_none()
            && self.fragmentation_suppression_depth == 0
            && margin_box_height.points() <= self.page_area_height() + FLOAT_EPSILON
            && top - margin_box_height.points() < self.page_bottom() - FLOAT_EPSILON
        {
            // A float occupies the old fragmentainer, but as an out-of-flow
            // box it must not make the next in-flow sibling form a named-page
            // boundary there.
            deferred_destination_snapshot = Some(self.snapshot());
            let continuation = self.float_page_break_continuation_context();
            self.current_page_has_flow_content = true;
            self.push_page();
            if self.active_fragmentainer_kind() == FragmentainerKind::Page {
                self.replay_fragment_continuation_on_page(&continuation, self.current_page_context);
            }
            run.reset_for_block(
                PageInlineSpan::from_edges(self.content_left, self.content_right),
                PageTopBlockPosition::new(self.cursor_y),
            );
            let next_placement_top = PageTopBlockPosition::new(self.cursor_y);
            (margin_box_left, top) =
                if matches!(float_side, UsedFloatSide::Top | UsedFloatSide::Bottom) {
                    {
                        let placement = self.find_vertical_float_avoiding_position(
                            next_placement_top,
                            margin_box_size,
                            placed_style.clear,
                            placement_axes,
                            placement_logical_inline_size,
                            Some(float_side),
                        );
                        (placement.origin.x(), placement.origin.top_y())
                    }
                } else {
                    let placement = self.find_inline_float_avoiding_position(
                        next_placement_top,
                        margin_box_size,
                        placed_style.clear,
                        placement_axes,
                        float_side,
                    );
                    let margin_box_left = placement
                        .inline_float_margin_box_left(float_side, inline_size.margin_box_width);
                    (margin_box_left, placement.origin.top_y())
                };
            logical_placement = LogicalFloatPlacement::from_physical_margin_box(
                self.current_float_page_index(),
                placement_axes.writing_mode(),
                placement_axes.direction(),
                float_side,
                PageTopRect::new(
                    self.content_left,
                    self.page_top(),
                    (self.content_right - self.content_left).max(0.0),
                    self.page_area_height(),
                ),
                PageTopRect::new(
                    margin_box_left,
                    top,
                    inline_size.margin_box_width.points(),
                    placement_block_size,
                ),
            );
        }

        let deferred_to_next_page = deferred_destination_snapshot.is_some();
        let layout_snapshot = deferred_destination_snapshot.unwrap_or_else(|| self.snapshot());
        let named_page_flow_content_before_float = self.current_page_has_named_page_flow_content;
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_direction = self.containing_block_direction;
        let previous_writing_mode = self.containing_block_writing_mode;

        // Float placement is expressed in margin-box coordinates, while the
        // replayed principal box is laid out from its border-box origin. The
        // placement algorithm has already consumed the used margins, so the
        // isolated replay must begin at the border box with those margins
        // suppressed.
        let replay_style = float_replay_style(&placed_style);
        let selected_margin_box = logical_placement.margin_box;
        let destination_border_box_left = selected_margin_box.x() + placed_style.margin.left;
        let destination_border_box_top = selected_margin_box.top_y() - placed_style.margin.top;
        let replay_projection = FloatReplayProjection::new(
            placement_axes,
            PageTopBlockPosition::new(previous_cursor_y),
            PageTopPoint::new(destination_border_box_left, destination_border_box_top),
        );
        self.content_left = destination_border_box_left;
        self.content_right = self.content_left + inline_size.border_box_width.points().max(1.0);
        self.cursor_y = replay_projection.scratch_border_box_top().points();
        // Placement has already projected the floated margin box through the
        // parent's axes, and sizing has frozen both physical replay
        // dimensions. Enter the isolated BFC in the float's own axes so block
        // replay cannot treat its already-physical origin as another
        // orthogonal normal-flow placement (which would apply the scratch
        // coordinate translation twice for bottom-to-top inline flow).
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        self.containing_block_direction = placed_style.used_direction();
        self.containing_block_writing_mode = placed_style.writing_mode;
        let scratch_replay_logical_inline_size = (replay_projection.projects_from_scratch()
            && placed_style.writing_mode.has_vertical_lines())
        .then(|| {
            let borders = used_border_widths(&placed_style);
            let vertical_non_content = non_content_pt(
                placed_style.padding.top
                    + placed_style.padding.bottom
                    + borders.top
                    + borders.bottom,
            );
            let measured_border_box_height = border_box_pt(
                (margin_box_height.points() - placed_style.margin.top - placed_style.margin.bottom)
                    .max(0.0),
            );
            LogicalInlineContentSize::new(border_box_to_content_box_length(
                measured_border_box_height,
                vertical_non_content,
            ))
        });
        let principal_publishes_static_border_box = matches!(
            placed_style.display.inner,
            DisplayInner::Flow | DisplayInner::FlowRoot
        );
        let previous_replayed_float_logical_inline_size = std::mem::replace(
            &mut self.replayed_float_logical_inline_size,
            scratch_replay_logical_inline_size.filter(|_| principal_publishes_static_border_box),
        );
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        self.push_page_name_scope_suppression();
        self.float_fragment_parent_inline_spans
            .push(PageInlineSpan::from_edges(previous_left, previous_right));
        self.push_float_context();
        self.float_paint_capture_depth += 1;
        let previous_preserve_scoped_paint_public_order = self.preserve_scoped_paint_public_order;
        self.preserve_scoped_paint_public_order = true;
        self.last_block_layout_outcome = BlockLayoutOutcome::default();
        if let Some(pseudo_source) = pseudo_source {
            self.layout_generated_pseudo_box(
                child_element,
                &replay_style,
                pseudo_source,
                stylesheets,
                &[],
                child_children,
                table_fragment,
                PrincipalBoxPaintMode::RootPaints,
            );
        } else {
            self.layout_element_with_child_boxes_and_table_fragment(
                child_element,
                &replay_style,
                stylesheets,
                child_children,
                table_fragment,
            );
        }
        self.replayed_float_logical_inline_size = previous_replayed_float_logical_inline_size;
        let cursor_replayed_border_box_height =
            (replay_projection.scratch_border_box_top().points() - self.cursor_y).max(0.0);
        let replay_source_border_box = principal_publishes_static_border_box
            .then_some(self.last_block_layout_outcome.static_border_box)
            .flatten();
        if let Some(border_box) = replay_source_border_box {
            if placed_style.writing_mode == WritingMode::HorizontalTb
                && self.pages.len() == paint_page_index
            {
                debug_assert!(
                    (border_box.size.height - cursor_replayed_border_box_height).abs()
                        <= FLOAT_EPSILON,
                    "unfragmented horizontal block-flow float replay geometry must agree with its cursor: border box {}, cursor {}",
                    border_box.size.height,
                    cursor_replayed_border_box_height,
                );
            }
            debug_assert!(
                (border_box.size.width - inline_size.border_box_width.points()).abs()
                    <= FLOAT_EPSILON,
                "block-flow float replay width must agree with its frozen size: border box {}, frozen {}",
                border_box.size.width,
                inline_size.border_box_width.points(),
            );
        }
        let replayed_border_box_height = replay_source_border_box
            .map(|border_box| border_box.size.height)
            .unwrap_or(cursor_replayed_border_box_height);
        let replay_paint_translation = replay_source_border_box
            .map(|border_box| replay_projection.source_to_destination(border_box))
            .unwrap_or_else(|| replay_projection.cursor_source_to_destination());
        let replayed_destination_border_box = replay_source_border_box
            .map(|border_box| replay_paint_translation.transform_rect(&border_box));
        self.preserve_scoped_paint_public_order = previous_preserve_scoped_paint_public_order;
        self.float_paint_capture_depth = self.float_paint_capture_depth.saturating_sub(1);
        self.pop_float_context();
        self.float_fragment_parent_inline_spans
            .pop()
            .expect("float fragment parent scope is balanced");
        self.pop_page_name_scope_suppression();
        self.ancestors.pop();

        // Float placement starts with a speculative margin-box height, but the
        // committed isolated replay is the only authoritative used block
        // geometry. In particular, a BFC-root float containing descendant
        // floats can receive an overlarge recursive probe height even though
        // its final replay establishes the short used extent required by CSS
        // 2.2. Keep the speculative size only for selecting an initial
        // placement; publish the replayed margin-box extent to later `clear`
        // and float-avoidance queries.
        //
        // The replay suppresses the float's margins and starts at the border
        // box, so reconstruct the final margin-box bottom explicitly instead
        // of comparing two differently scoped cursors.
        // <https://www.w3.org/TR/CSS22/visudet.html#root-height>
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        let (float_margin_box, replayed_margin_box_inline_extent) = if let Some(border_box) =
            replayed_destination_border_box
        {
            let margin_box_inline_extent = margin_box_pt(
                placed_style.margin.left + border_box.size.width + placed_style.margin.right,
            );
            (
                PageTopRect::new(
                    border_box.origin.x - placed_style.margin.left,
                    border_box.origin.y + border_box.size.height + placed_style.margin.top,
                    margin_box_inline_extent.points(),
                    placed_style.margin.top + border_box.size.height + placed_style.margin.bottom,
                ),
                margin_box_inline_extent,
            )
        } else {
            (
                PageTopRect::new(
                    margin_box_left,
                    top,
                    inline_size.margin_box_width.points(),
                    placed_style.margin.top
                        + replayed_border_box_height
                        + placed_style.margin.bottom,
                ),
                inline_size.margin_box_width,
            )
        };
        logical_placement =
            logical_placement.with_margin_box(logical_placement.containing, float_margin_box);
        let float_bounds = float_margin_box.paint_clip();
        let mut float_shape = FloatShape::from_rect(
            float_id,
            specified_side,
            float_side,
            paint_source_order,
            paint_page_index,
            float_margin_box,
        );
        float_shape.outer_inline_extent = replayed_margin_box_inline_extent;
        float_shape.placement = Some(logical_placement);
        float_shape.area = resolve_float_area(
            &placed_style,
            float_shape.rect,
            containing_inline_size,
            self.resource_cache,
            self.base_url,
            self.root_url,
        );
        let mut recorded_float_shape = false;
        let mut unpainted_float_fragment_shapes = Vec::new();
        let fragmented_float = deferred_to_next_page || self.pages.len() != paint_page_index;
        let captures_positioned_descendants = StackingContextPolicy::for_atomic(&placed_style, PaintBand::Float, float_bounds)
                .captures_positioned_descendants
                // Fragmented float escape needs page-fragment-aware positioned
                // containing block mapping. Keep existing per-fragment replay
                // until escaped positioned layers can be assigned to the
                // correct destination page without losing Appendix E ordering.
                || fragmented_float;
        let child_layers = if captures_positioned_descendants
            && positioned_layer_start < self.positioned_layers.len()
        {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        let has_positioned_descendants = !child_layers.is_empty();
        let has_out_of_flow_descendants =
            child_children.is_some_and(has_out_of_flow_formatting_box);
        if !fragmented_float {
            // Same-page fragments retain operation nodes that index the
            // page's concrete primitive arrays. Move that recorded suffix at
            // the projection boundary; translating the fragment below then
            // handles only still-unrecorded primitive nodes.
            self.current_page
                .translate_recorded_primitives_since(&paint_checkpoint, replay_paint_translation);
            let fragment = self
                .current_page
                .paint_tree_fragment_since(&paint_checkpoint)
                .translated(replay_paint_translation);
            let child_contexts = child_layers
                .iter()
                .filter(|layer| layer.page_index == paint_page_index)
                .cloned()
                .map(|layer| {
                    let layer = layer.translated(replay_paint_translation);
                    layer.context.with_links(layer.links)
                })
                .collect::<Vec<_>>();
            if let Some(mut float_fragment) = self.build_float_paint_fragment(
                float_id,
                specified_side,
                paint_page_index,
                float_side,
                paint_source_order,
                logical_placement,
                replayed_margin_box_inline_extent,
                float_bounds,
                &replay_style,
                replaced_element_kind(child_element).is_some(),
                fragment,
                child_contexts,
                false,
            ) {
                // The durable exclusion is registered from the paint
                // fragment on the ordinary (non-fragmented) path as well as
                // on fragmented continuations. Preserve the already-resolved
                // used shape instead of falling back to its margin rectangle.
                float_fragment.area = float_shape.area.clone();
                self.current_page.replace_paint_tree_since_with_context(
                    &paint_checkpoint,
                    PaintBand::Float,
                    float_fragment.context.clone(),
                );
                self.push_float_fragment_shape(&float_fragment, run);
                recorded_float_shape = true;
            }
        } else {
            let fragments =
                self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
            let mut float_fragments = Vec::new();
            for (page_index, fragment) in fragments {
                let fragment = fragment.translated(replay_paint_translation);
                let child_contexts = child_layers
                    .iter()
                    .filter(|layer| layer.page_index == page_index)
                    .cloned()
                    .map(|layer| {
                        let layer = layer.translated(replay_paint_translation);
                        layer.context.with_links(layer.links)
                    })
                    .collect::<Vec<_>>();
                if let Some(float_fragment) = self.build_float_paint_fragment(
                    float_id,
                    specified_side,
                    page_index,
                    float_side,
                    paint_source_order,
                    logical_placement,
                    replayed_margin_box_inline_extent,
                    float_bounds,
                    &replay_style,
                    replaced_element_kind(child_element).is_some(),
                    fragment,
                    child_contexts,
                    true,
                ) {
                    float_fragments.push(float_fragment);
                }
            }
            float_fragments.sort_by_key(|fragment| (fragment.page_index, fragment.source_order));
            // A fragmented float with no paint (for example, an empty box
            // whose used height comes from an authored `height`) still owns a
            // page-local exclusion in every fragmentainer it crosses. Later
            // floats must clear those continuations and an auto-height BFC
            // must include their final fragment even though there is no paint
            // tree from which `FloatPaintFragment` can be built.
            // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            if float_fragments.is_empty() {
                let mut remaining_block_size = float_margin_box.height();
                for fragment_page_index in paint_page_index..=self.pages.len() {
                    let fragment_context = if fragment_page_index == paint_page_index {
                        layout_snapshot.current_page_context()
                    } else {
                        self.fragmentainer_override
                            .map(|override_| {
                                override_.context_for_fragmentainer(fragment_page_index)
                            })
                            .unwrap_or(self.current_page_context)
                    };
                    let fragment_top = if fragment_page_index == paint_page_index {
                        float_margin_box.top_y()
                    } else {
                        fragment_context.top()
                    };
                    let fragment_block_size = remaining_block_size
                        .min((fragment_top - fragment_context.bottom()).max(0.0));
                    if fragment_block_size <= FLOAT_EPSILON {
                        continue;
                    }
                    let fragment_rect = PageTopRect::new(
                        float_margin_box.x(),
                        fragment_top,
                        float_margin_box.width(),
                        fragment_block_size,
                    );
                    let mut fragment_shape = float_shape.clone();
                    fragment_shape.fragment_index = unpainted_float_fragment_shapes.len();
                    fragment_shape.starts_on_previous_page = fragment_page_index > paint_page_index;
                    fragment_shape.continues_on_next_page =
                        remaining_block_size > fragment_block_size + FLOAT_EPSILON;
                    fragment_shape.page_index = fragment_page_index;
                    fragment_shape.rect = fragment_rect;
                    fragment_shape.placement = Some(
                        logical_placement
                            .with_margin_box(logical_placement.containing, fragment_rect)
                            .on_page(fragment_page_index),
                    );
                    fragment_shape.area = float_shape.area.clone().with_margin_clip(fragment_rect);
                    unpainted_float_fragment_shapes.push(fragment_shape);
                    remaining_block_size -= fragment_block_size;
                }
                debug_assert!(remaining_block_size <= FLOAT_EPSILON);
            }
            let fragment_count = float_fragments.len();
            for (index, fragment) in float_fragments.iter_mut().enumerate() {
                fragment.fragment_index = index;
                fragment.starts_on_previous_page = index > 0;
                fragment.continues_on_next_page = index + 1 < fragment_count;
                // The fragment rectangle clips the retained used contour at
                // query time, preserving page-local CSS Shapes wrapping.
                fragment.area = float_shape.area.clone();
            }
            let side_effects = self.deferred_layout_side_effects_since(&layout_snapshot);
            let next_paint_source_order = self.next_paint_source_order;
            let escaped_child_layers = if !captures_positioned_descendants
                && positioned_layer_start < self.positioned_layers.len()
            {
                self.positioned_layers.split_off(positioned_layer_start)
            } else {
                Vec::new()
            };
            self.restore(layout_snapshot);
            self.positioned_layers.extend(escaped_child_layers);
            self.next_paint_source_order = next_paint_source_order;
            self.apply_deferred_layout_side_effects(side_effects);
            for float_fragment in float_fragments {
                let paint_fragment = PaintFragment::from_stacking_context_in_band(
                    PaintBand::Float,
                    float_fragment.context.clone(),
                );
                if float_fragment.page_index == self.pages.len() {
                    self.current_page
                        .append_paint_fragment_owned(paint_fragment, PaintTranslation::identity());
                    // The final slice remains in `current_page` until the
                    // document is finalized. It is real page content even
                    // though floats are out of normal flow; otherwise
                    // `finish_boxed` drops a page containing only the final
                    // oversized-float slice.
                    // <https://www.w3.org/TR/css-break-3/#monolithic>
                    self.current_page_has_flow_content = true;
                    self.current_page.mark_fragmentation_content();
                } else {
                    // The float's continuation belongs to a future
                    // fragmentainer, but it must not advance the enclosing
                    // normal-flow cursor. When following flow reaches that
                    // page, `apply_pending_fragments_for_current_page`
                    // attaches the page-local float paint before line layout
                    // queries its registered shape.
                    // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                    self.pending_paint_fragments.push(PendingPaintFragment {
                        page_index: float_fragment.page_index,
                        fragment: paint_fragment,
                        kind: PendingPaintFragmentKind::FragmentedFloat,
                    });
                }
                self.push_float_fragment_shape(&float_fragment, run);
                recorded_float_shape = true;
            }
        }
        // A zero-height float participates in its own `clear` placement but
        // has no block-axis area to exclude from following line boxes.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        // A URL or generated-image `shape-outside` is different: replaced
        // image paint can be retained outside the ordinary float paint scope,
        // leaving the provisional CSS 2.2 rectangle zero-height even though
        // the independently resolved alpha contour has a definite used
        // content box. Keep that contour in the BFC exclusion list.
        // <https://drafts.csswg.org/css-shapes-1/#shapes-from-image>
        let has_image_shape_contour =
            matches!(float_shape.area.contour, FloatContour::RasterAlpha { .. });
        if !recorded_float_shape
            && (float_shape.rect.height() > FLOAT_EPSILON
                || has_image_shape_contour
                || has_positioned_descendants
                || has_out_of_flow_descendants)
        {
            if unpainted_float_fragment_shapes.is_empty() {
                self.push_float_shape(float_shape, run);
            } else {
                for fragment_shape in unpainted_float_fragment_shapes {
                    self.push_float_shape(fragment_shape, run);
                }
            }
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.containing_block_direction = previous_direction;
        self.containing_block_writing_mode = previous_writing_mode;
        // Floats are out of normal flow. Their descendants can paint and
        // participate in float exclusion, but cannot turn the parent BFC's
        // following class-A sibling into a named-page transition boundary.
        // If float fragmentation advanced to a destination page, that page
        // likewise starts without an in-flow named-page group.
        self.current_page_has_named_page_flow_content =
            if deferred_to_next_page || self.pages.len() == paint_page_index {
                named_page_flow_content_before_float
            } else {
                false
            };
        true
    }
}

/// Resolve the CSS Shapes wrapping contour after float metrics and final
/// margin-box placement are known. The returned contour remains clipped by
/// `margin_rect` when queried, as required by CSS Shapes Level 1.
fn resolve_float_area(
    style: &ComputedStyle,
    margin_rect: PageTopRect,
    containing_inline_size: f32,
    resource_cache: &ResourceCache,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> FloatArea {
    let css::ShapeOutside::Basic {
        shape,
        reference_box,
    } = &style.shape_outside
    else {
        return match &style.shape_outside {
            css::ShapeOutside::Box(reference_box) => FloatArea::new(
                FloatContour::RoundedRect(reference_box_contour(
                    style,
                    margin_rect,
                    *reference_box,
                )),
                resolve_shape_margin(style, containing_inline_size),
            ),
            css::ShapeOutside::Image(image) => {
                return resolve_image_float_area(
                    style,
                    margin_rect,
                    image,
                    resource_cache,
                    base_url,
                    root_url,
                    containing_inline_size,
                );
            }
            css::ShapeOutside::None | css::ShapeOutside::Basic { .. } => FloatArea::RECT,
        };
    };
    let reference = reference_box_rect(style, margin_rect, *reference_box);
    let shape_margin = resolve_shape_margin(style, containing_inline_size);
    match shape {
        css::BasicShape::Circle(circle) => {
            let width = reference.width();
            let height = reference.height();
            let cx = reference.x() + resolve_shape_length(&circle.position.x, width);
            let cy = reference.top_y() - resolve_shape_length(&circle.position.y, height);
            let left = cx - reference.x();
            let right = reference.x() + width - cx;
            let top = reference.top_y() - cy;
            let bottom = cy - reference.bottom_y();
            let radius = match &circle.radius {
                css::ShapeCircleRadius::LengthPercentage(value) => {
                    resolve_shape_length(value, diagonal_size(width, height))
                }
                css::ShapeCircleRadius::ClosestSide => left.min(right).min(top).min(bottom),
                css::ShapeCircleRadius::FarthestSide => left.max(right).max(top).max(bottom),
                css::ShapeCircleRadius::ClosestCorner => [
                    left.hypot(top),
                    right.hypot(top),
                    right.hypot(bottom),
                    left.hypot(bottom),
                ]
                .into_iter()
                .fold(f32::INFINITY, f32::min),
                css::ShapeCircleRadius::FarthestCorner => [
                    left.hypot(top),
                    right.hypot(top),
                    right.hypot(bottom),
                    left.hypot(bottom),
                ]
                .into_iter()
                .fold(0.0, f32::max),
            }
            .max(0.0);
            FloatArea::new(
                FloatContour::Circle {
                    center_x: cx,
                    center_y: cy,
                    radius,
                },
                shape_margin,
            )
        }
        css::BasicShape::Ellipse(ellipse) => {
            let width = reference.width();
            let height = reference.height();
            let cx = reference.x() + resolve_shape_length(&ellipse.position.x, width);
            let cy = reference.top_y() - resolve_shape_length(&ellipse.position.y, height);
            let left = cx - reference.x();
            let right = reference.x() + width - cx;
            let top = reference.top_y() - cy;
            let bottom = cy - reference.bottom_y();
            let radius_x = match &ellipse.horizontal_radius {
                css::ShapeEllipseRadius::LengthPercentage(value) => {
                    resolve_shape_length(value, width)
                }
                css::ShapeEllipseRadius::ClosestSide => left.min(right),
                css::ShapeEllipseRadius::FarthestSide => left.max(right),
            }
            .max(0.0);
            let radius_y = match &ellipse.vertical_radius {
                css::ShapeEllipseRadius::LengthPercentage(value) => {
                    resolve_shape_length(value, height)
                }
                css::ShapeEllipseRadius::ClosestSide => top.min(bottom),
                css::ShapeEllipseRadius::FarthestSide => top.max(bottom),
            }
            .max(0.0);
            FloatArea::new(
                FloatContour::Ellipse {
                    center_x: cx,
                    center_y: cy,
                    radius_x,
                    radius_y,
                },
                shape_margin,
            )
        }
        css::BasicShape::Inset(inset) => {
            let width = reference.width();
            let height = reference.height();
            let left = reference.x() + resolve_shape_length(&inset.left, width);
            let right = reference.x() + width - resolve_shape_length(&inset.right, width);
            let top = reference.top_y() - resolve_shape_length(&inset.top, height);
            let bottom = reference.bottom_y() + resolve_shape_length(&inset.bottom, height);
            let rect =
                PageTopRect::new(left, top, (right - left).max(0.0), (top - bottom).max(0.0));
            FloatArea::new(
                FloatContour::RoundedRect(rounded_rect(rect, inset.radii.clone())),
                shape_margin,
            )
        }
        css::BasicShape::Polygon(polygon) => FloatArea::new(
            FloatContour::Polygon {
                vertices: polygon
                    .vertices
                    .iter()
                    .map(|vertex| {
                        PageTopPoint::new(
                            reference.x() + resolve_shape_length(&vertex.x, reference.width()),
                            reference.top_y() - resolve_shape_length(&vertex.y, reference.height()),
                        )
                    })
                    .collect(),
                fill_rule: polygon.fill_rule,
            },
            shape_margin,
        ),
        css::BasicShape::Path(path) => FloatArea::new(
            FloatContour::Path {
                subpaths: flatten_shape_path_data(&path.data, reference),
                fill_rule: path.fill_rule,
            },
            shape_margin,
        ),
        css::BasicShape::Shape(shape) => FloatArea::new(
            FloatContour::Path {
                subpaths: flatten_shape_function(shape, reference),
                fill_rule: shape.fill_rule,
            },
            shape_margin,
        ),
    }
}

/// The maximum page-space distance between the retained CSS curve and its
/// polyline wrap contour. This is substantially below a CSS pixel while
/// avoiding a per-line analytical-curve intersection implementation.
const SHAPE_PATH_FLATTEN_TOLERANCE: f64 = 0.01;

fn flatten_shape_path_data(data: &str, reference: PageTopRect) -> Vec<Vec<PageTopPoint>> {
    let Ok(path) = kurbo::BezPath::from_svg(data) else {
        // The cascade validated this, but invalid input must remain an empty
        // contour rather than accidentally reverting to a rectangle.
        return Vec::new();
    };
    flatten_shape_bez_path(path, reference, css::CSS_PX_TO_PT as f64)
}

fn flatten_shape_function(
    shape: &css::ShapeFunction,
    reference: PageTopRect,
) -> Vec<Vec<PageTopPoint>> {
    let mut path = kurbo::BezPath::new();
    let resolve_point = |point: &css::ShapePosition| {
        kurbo::Point::new(
            resolve_shape_length(&point.x, reference.width()) as f64,
            resolve_shape_length(&point.y, reference.height()) as f64,
        )
    };
    let mut current = resolve_point(&shape.origin);
    let mut subpath_start = current;
    let mut previous_control = None;
    path.move_to(current);
    for command in &shape.commands {
        let absolute = |direction: css::ShapeCommandDirection, point: kurbo::Point| match direction
        {
            css::ShapeCommandDirection::To => point,
            css::ShapeCommandDirection::By => current + (point - kurbo::Point::ORIGIN),
        };
        match command {
            css::ShapeCommand::Move { direction, point } => {
                current = absolute(*direction, resolve_point(point));
                subpath_start = current;
                previous_control = None;
                path.move_to(current);
            }
            css::ShapeCommand::Line { direction, point } => {
                current = absolute(*direction, resolve_point(point));
                previous_control = None;
                path.line_to(current);
            }
            css::ShapeCommand::HorizontalLine { direction, value } => {
                let value = resolve_shape_length(value, reference.width()) as f64;
                current.x = match direction {
                    css::ShapeCommandDirection::To => value,
                    css::ShapeCommandDirection::By => current.x + value,
                };
                previous_control = None;
                path.line_to(current);
            }
            css::ShapeCommand::VerticalLine { direction, value } => {
                let value = resolve_shape_length(value, reference.height()) as f64;
                current.y = match direction {
                    css::ShapeCommandDirection::To => value,
                    css::ShapeCommandDirection::By => current.y + value,
                };
                previous_control = None;
                path.line_to(current);
            }
            css::ShapeCommand::Curve {
                direction,
                point,
                control_start,
                control_end,
            } => {
                let control_start = absolute(*direction, resolve_point(control_start));
                let control_end = absolute(*direction, resolve_point(control_end));
                current = absolute(*direction, resolve_point(point));
                previous_control = Some(control_end);
                path.curve_to(control_start, control_end, current);
            }
            css::ShapeCommand::Smooth {
                direction,
                point,
                control_end,
            } => {
                let control_start = previous_control
                    .map(|control| current + (current - control))
                    .unwrap_or(current);
                let control_end = absolute(*direction, resolve_point(control_end));
                current = absolute(*direction, resolve_point(point));
                previous_control = Some(control_end);
                path.curve_to(control_start, control_end, current);
            }
            css::ShapeCommand::Arc {
                direction,
                point,
                radius_x,
                radius_y,
                rotation_radians,
                large,
                clockwise,
            } => {
                let target = absolute(*direction, resolve_point(point));
                let radius_x = resolve_shape_length(radius_x, reference.width()).max(0.0);
                let radius_y = resolve_shape_length(radius_y, reference.height()).max(0.0);
                if radius_x <= 0.0 || radius_y <= 0.0 || target == current {
                    path.line_to(target);
                } else {
                    // Reuse the SVG arc conversion that kurbo already uses
                    // for `path()`, avoiding a second subtly different arc
                    // implementation. The path lives in CSS physical space
                    // until the shared flatten-and-page transform below.
                    let svg = format!(
                        "M {} {} A {} {} {} {} {} {} {}",
                        current.x,
                        current.y,
                        radius_x,
                        radius_y,
                        rotation_radians.to_degrees(),
                        u8::from(*large),
                        u8::from(*clockwise),
                        target.x,
                        target.y,
                    );
                    if let Ok(arc) = kurbo::BezPath::from_svg(&svg) {
                        path.extend(arc.iter().skip(1));
                    } else {
                        path.line_to(target);
                    }
                }
                current = target;
                previous_control = None;
            }
            css::ShapeCommand::Close => {
                path.close_path();
                current = subpath_start;
                previous_control = None;
            }
        }
    }
    flatten_shape_bez_path(path, reference, 1.0)
}

fn flatten_shape_bez_path(
    path: kurbo::BezPath,
    reference: PageTopRect,
    coordinate_scale: f64,
) -> Vec<Vec<PageTopPoint>> {
    let mut subpaths = Vec::new();
    let mut current = Vec::new();
    kurbo::flatten(
        path.iter(),
        SHAPE_PATH_FLATTEN_TOLERANCE,
        |element| match element {
            kurbo::PathEl::MoveTo(point) => {
                if !current.is_empty() {
                    subpaths.push(std::mem::take(&mut current));
                }
                current.push(PageTopPoint::new(
                    reference.x() + (point.x * coordinate_scale) as f32,
                    reference.top_y() - (point.y * coordinate_scale) as f32,
                ));
            }
            kurbo::PathEl::LineTo(point) => current.push(PageTopPoint::new(
                reference.x() + (point.x * coordinate_scale) as f32,
                reference.top_y() - (point.y * coordinate_scale) as f32,
            )),
            kurbo::PathEl::ClosePath => {}
            kurbo::PathEl::QuadTo(_, _) | kurbo::PathEl::CurveTo(_, _, _) => {
                unreachable!("kurbo::flatten emits lines")
            }
        },
    );
    if !current.is_empty() {
        subpaths.push(current);
    }
    subpaths.retain(|subpath| subpath.len() >= 3);
    subpaths
}

fn resolve_image_float_area(
    style: &ComputedStyle,
    margin_rect: PageTopRect,
    image: &css::BackgroundImage,
    resource_cache: &ResourceCache,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    containing_inline_size: f32,
) -> FloatArea {
    let mut content = reference_box_rect(style, margin_rect, css::ShapeBox::Content);
    let paint_size = PaintSize::new(
        content.width() / css::CSS_PX_TO_PT,
        content.height() / css::CSS_PX_TO_PT,
    );
    let Some(decoded) = rasterize_generated_css_image(
        image,
        paint_size,
        style.color,
        base_url,
        root_url,
        resource_cache,
    ) else {
        return FloatArea::RECT;
    };
    let Some(image_id) = decoded.image_id else {
        return FloatArea::RECT;
    };
    // Replaced floats lay out their descendants without advancing the
    // temporary block cursor, so their provisional margin rectangle can be
    // zero-height even though the image's used content box is definite. CSS
    // Shapes sizes image sources as replaced elements with that content box.
    // <https://drafts.csswg.org/css-shapes-1/#shapes-from-image>
    if content.width() <= FLOAT_EPSILON || content.height() <= FLOAT_EPSILON {
        let natural_size = decoded.natural_layout_size();
        content = PageTopRect::new(
            margin_rect.x() + style.margin.left + style.border_widths.left + style.padding.left,
            margin_rect.top_y() - style.margin.top - style.border_widths.top - style.padding.top,
            natural_size.width,
            natural_size.height,
        );
    }
    let threshold = (style.shape_image_threshold.value() * 255.0).round() as u8;
    let shape_margin = resolve_shape_margin(style, containing_inline_size);
    // The alpha source is sized to the content box, but the resulting shape
    // (including `shape-margin`) is clipped by the float's margin box. A
    // replaced float can temporarily have no principal rectangle during
    // isolated replay; retain the resolved content clip only for that
    // fallback path.
    // <https://drafts.csswg.org/css-shapes-1/#shape-outside-property>
    let shape_margin_clip =
        if margin_rect.width() > FLOAT_EPSILON && margin_rect.height() > FLOAT_EPSILON {
            margin_rect
        } else {
            content
        };
    resource_cache
        .with_rasterized_image(image_id, |raster| {
            let alpha = raster.alpha.unwrap_or_else(|| {
                vec![
                    255;
                    raster.metadata.pixel_size.width as usize
                        * raster.metadata.pixel_size.height as usize
                ]
            });
            FloatArea::new(
                FloatContour::RasterAlpha {
                    rect: content,
                    pixel_width: raster.metadata.pixel_size.width,
                    pixel_height: raster.metadata.pixel_size.height,
                    alpha,
                    threshold,
                },
                shape_margin,
            )
            .with_margin_clip(shape_margin_clip)
        })
        .unwrap_or(FloatArea::RECT)
}

/// CSS Shapes resolves percentage `shape-margin` values against the inline
/// size of the float's containing block, independently of its reference box.
/// <https://drafts.csswg.org/css-shapes-1/#shape-margin-property>
fn resolve_shape_margin(style: &ComputedStyle, containing_inline_size: f32) -> f32 {
    resolve_shape_length(&style.shape_margin, containing_inline_size).max(0.0)
}

fn reference_box_rect(
    style: &ComputedStyle,
    margin_rect: PageTopRect,
    shape_box: css::ShapeBox,
) -> PageTopRect {
    let border = PageTopRect::new(
        margin_rect.x() + style.margin.left,
        margin_rect.top_y() - style.margin.top,
        (margin_rect.width() - style.margin.left - style.margin.right).max(0.0),
        (margin_rect.height() - style.margin.top - style.margin.bottom).max(0.0),
    );
    let insets = match shape_box {
        css::ShapeBox::Margin => return margin_rect,
        css::ShapeBox::Border => return border,
        css::ShapeBox::Padding => style.border_widths,
        css::ShapeBox::Content => css::Edges {
            top: style.border_widths.top + style.padding.top,
            right: style.border_widths.right + style.padding.right,
            bottom: style.border_widths.bottom + style.padding.bottom,
            left: style.border_widths.left + style.padding.left,
        },
    };
    PageTopRect::new(
        border.x() + insets.left,
        border.top_y() - insets.top,
        (border.width() - insets.left - insets.right).max(0.0),
        (border.height() - insets.top - insets.bottom).max(0.0),
    )
}

fn reference_box_contour(
    style: &ComputedStyle,
    margin_rect: PageTopRect,
    shape_box: css::ShapeBox,
) -> UsedRoundedRect {
    let border = reference_box_rect(style, margin_rect, css::ShapeBox::Border);
    let base = rounded_rect(border, style.border_radius.clone());
    let rect = reference_box_rect(style, margin_rect, shape_box);
    let (top, right, bottom, left) = match shape_box {
        css::ShapeBox::Margin => (
            style.margin.top,
            style.margin.right,
            style.margin.bottom,
            style.margin.left,
        ),
        css::ShapeBox::Border => (0.0, 0.0, 0.0, 0.0),
        css::ShapeBox::Padding => (
            -style.border_widths.top,
            -style.border_widths.right,
            -style.border_widths.bottom,
            -style.border_widths.left,
        ),
        css::ShapeBox::Content => (
            -style.border_widths.top - style.padding.top,
            -style.border_widths.right - style.padding.right,
            -style.border_widths.bottom - style.padding.bottom,
            -style.border_widths.left - style.padding.left,
        ),
    };
    normalized_rounded_rect(
        rect,
        (base.top_left.0 + left).max(0.0),
        (base.top_left.1 + top).max(0.0),
        (base.top_right.0 + right).max(0.0),
        (base.top_right.1 + top).max(0.0),
        (base.bottom_right.0 + right).max(0.0),
        (base.bottom_right.1 + bottom).max(0.0),
        (base.bottom_left.0 + left).max(0.0),
        (base.bottom_left.1 + bottom).max(0.0),
    )
}

fn rounded_rect(rect: PageTopRect, radii: css::BorderRadius) -> UsedRoundedRect {
    let width = rect.width();
    let height = rect.height();
    let resolve_x =
        |radius: &css::CornerRadiusComponent| resolve_shape_length(&radius.value, width);
    let resolve_y =
        |radius: &css::CornerRadiusComponent| resolve_shape_length(&radius.value, height);
    let top_left = (
        resolve_x(&radii.top_left.horizontal),
        resolve_y(&radii.top_left.vertical),
    );
    let top_right = (
        resolve_x(&radii.top_right.horizontal),
        resolve_y(&radii.top_right.vertical),
    );
    let bottom_right = (
        resolve_x(&radii.bottom_right.horizontal),
        resolve_y(&radii.bottom_right.vertical),
    );
    let bottom_left = (
        resolve_x(&radii.bottom_left.horizontal),
        resolve_y(&radii.bottom_left.vertical),
    );
    normalized_rounded_rect(
        rect,
        top_left.0,
        top_left.1,
        top_right.0,
        top_right.1,
        bottom_right.0,
        bottom_right.1,
        bottom_left.0,
        bottom_left.1,
    )
}

#[allow(clippy::too_many_arguments)]
fn normalized_rounded_rect(
    rect: PageTopRect,
    tlx: f32,
    tly: f32,
    trx: f32,
    try_: f32,
    brx: f32,
    bry: f32,
    blx: f32,
    bly: f32,
) -> UsedRoundedRect {
    let scale = [
        rect.width() / (tlx + trx).max(rect.width()),
        rect.width() / (blx + brx).max(rect.width()),
        rect.height() / (tly + bly).max(rect.height()),
        rect.height() / (try_ + bry).max(rect.height()),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);
    UsedRoundedRect {
        left: rect.x(),
        right: rect.x() + rect.width(),
        top: rect.top_y(),
        bottom: rect.bottom_y(),
        top_left: (tlx * scale, tly * scale),
        top_right: (trx * scale, try_ * scale),
        bottom_right: (brx * scale, bry * scale),
        bottom_left: (blx * scale, bly * scale),
    }
}

fn resolve_shape_length(value: &css::ComputedLengthPercentage, basis: f32) -> f32 {
    value
        .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis.max(0.0))))
        .unwrap_or(layout_pt(0.0))
        .points()
}

fn diagonal_size(width: f32, height: f32) -> f32 {
    width.hypot(height) / 2.0_f32.sqrt()
}
