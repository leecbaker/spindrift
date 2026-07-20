use super::*;
use crate::layout::block::float::FLOAT_EPSILON;

impl<'a> LayoutBuilder<'a> {
    /// Measure a block formatting context root in one candidate float band.
    ///
    /// The percentage basis remains the containing block's full content
    /// width: float avoidance changes a BFC root's available inline space,
    /// not the percentage basis inherited by its descendants.
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>
    #[allow(clippy::too_many_arguments)]
    fn measure_float_avoiding_bfc(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_inline_size: f32,
        containing_inline_span: PageInlineSpan,
        band: FloatBand,
    ) -> FloatAvoidingBfcMeasurement {
        let band_left = band.left();
        let band_width = band.width();
        let auto_border_box_width = (band_width < containing_inline_size - FLOAT_EPSILON)
            .then_some(float_avoiding_auto_border_box_width(
                PageInlineSpan::new(band_left, band_width),
                containing_inline_span,
                style.margin.left,
                style.margin.right,
            ));
        let candidate_geometry = self.block_layout_geometry_in_inline_span(
            element,
            style,
            stylesheets,
            child_boxes,
            BlockLayoutInlineConstraint {
                containing_inline_span: PageInlineSpan::new(band_left, band_width),
                percentage_basis: PercentageBasis::definite(LogicalInlineContentSize::new(
                    content_box_pt(containing_inline_size),
                )),
                physical_width_percentage_basis: PhysicalContentWidth::new(content_box_pt(
                    containing_inline_size,
                )),
                auto_border_box_width,
            },
        );
        let candidate_style = &candidate_geometry.style;
        let estimated_outer_height = self
            .estimate_element_height(
                element,
                candidate_style,
                stylesheets,
                candidate_geometry.outer_inline().width().points(),
                child_boxes,
            )
            .unwrap_or(
                candidate_style.margin.top
                    + candidate_style.line_height
                    + candidate_style.margin.bottom,
            );
        let border_box_height =
            (estimated_outer_height - candidate_style.margin.top - candidate_style.margin.bottom)
                .max(0.0);
        FloatAvoidingBfcMeasurement {
            border_box_inline_span: PageInlineSpan::new(
                candidate_geometry.outer_inline().span().left_x()
                    - candidate_geometry.relative_offset.x(),
                candidate_geometry.outer_inline().span().width(),
            ),
            border_box_block_size: border_box_pt(border_box_height),
            permits_inline_start_overflow: match candidate_style.direction {
                Direction::Ltr => candidate_style.margin.left < -FLOAT_EPSILON,
                Direction::Rtl => candidate_style.margin.right < -FLOAT_EPSILON,
            },
            permits_inline_end_overflow: match candidate_style.direction {
                Direction::Ltr => candidate_style.margin.right < -FLOAT_EPSILON,
                Direction::Rtl => candidate_style.margin.left < -FLOAT_EPSILON,
            },
        }
    }

    pub(in crate::layout) fn layout_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        self.layout_block_with_descendant_percentage_height_basis(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            None,
        );
    }

    pub(in crate::layout) fn layout_block_with_descendant_percentage_height_basis(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
    ) {
        // A multicol root may need to construct frozen child boxes below.
        // Build those boxes from the unscaled cascade parent before this block
        // crosses its normal-flow used-value boundary.
        let built_multicol_child_boxes;
        let child_boxes = if child_boxes.is_none()
            && (style.column_count.is_some()
                || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
                || matches!(style.column_height, css::ComputedColumnHeight::Length(_)))
        {
            built_multicol_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            Some(built_multicol_child_boxes.as_slice())
        } else {
            child_boxes
        };
        // Block line layout consumes its own style directly for line-height
        // and baseline geometry, unlike box sizing which clones a used style
        // internally. Normalize this boundary before collecting inline items.
        // <https://drafts.csswg.org/css-viewport/#zoom-property>
        let mut used_style = style.clone();
        used_style.apply_effective_zoom();
        let style = &used_style;
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
        let fragments_as_promoted_spanner = self.multicol_spanner_fragmentation_depth > 0
            && self.fragmentation_suppression_depth == 0
            && style.column_span == css::ColumnSpan::All;
        let mut geometry = self.block_layout_geometry(element, style, stylesheets, child_boxes);
        let definite_principal_fits_current_column = fragmentainer_kind
            == FragmentainerKind::Column
            && !fragments_as_promoted_spanner
            && style.writing_mode == WritingMode::HorizontalTb
            && !style.contain.size
            && !fragmentainer_kind.is_forced_break(style.break_before)
            && !formatting_boxes_have_forced_break_in(child_boxes, fragmentainer_kind)
            && geometry.definite_content_height.is_some_and(|height| {
                let available =
                    (self.cursor_y + geometry.relative_offset.y() - self.page_bottom()).max(0.0);
                let outer_height = style.margin.top
                    + geometry.vertical_non_content.points()
                    + height.points().max(0.0)
                    + style.margin.bottom;
                outer_height <= available + 0.01
            });
        if (fragments_as_promoted_spanner || definite_principal_fits_current_column)
            && self.multicol_spanner_speculation_depth == 0
            && let Some(definite_content_height) = geometry.definite_content_height
            && self.definite_block_descendants_overflow(
                child_boxes,
                stylesheets,
                geometry.content_inline().width().points(),
                definite_content_height.points(),
            )
        {
            self.layout_definite_block_with_deferred_descendant_overflow(
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                descendant_percentage_height_basis,
                definite_content_height.points(),
                geometry.vertical_non_content,
            );
            return;
        }
        self.begin_clamp_line_slot_capture();
        self.apply_forced_break_before_box_in(fragmentainer_kind, style);
        loop {
            let prebreak_content_height = geometry
                .definite_content_height
                .map(PhysicalContentHeight::points)
                .or_else(|| {
                    // A clipped overflow box is a non-fragmenting formatting
                    // context. When `break-inside` avoids this fragmentainer,
                    // measure its auto content so the class-A break before the
                    // box can keep that otherwise monolithic box intact.
                    // <https://www.w3.org/TR/css-break-3/#break-inside>
                    (fragmentainer_kind.avoids_break_inside(&geometry.style)
                        && self.element_used_overflow_clips(element, &geometry.style))
                    .then(|| {
                        self.estimate_block_like_height(
                            element,
                            &geometry.style,
                            stylesheets,
                            geometry.content_inline().width().points(),
                            child_boxes,
                        ) - geometry.style.margin.top
                            - geometry.vertical_non_content.points()
                            - geometry.style.margin.bottom
                    })
                })
                .or_else(|| {
                    let establishes_multicol = geometry.style.column_count.is_some()
                        || matches!(
                            geometry.style.column_width,
                            css::ComputedColumnWidth::Length(_)
                        )
                        || matches!(
                            geometry.style.column_height,
                            css::ComputedColumnHeight::Length(_)
                        );
                    (fragmentainer_kind == FragmentainerKind::Column && establishes_multicol)
                        .then(|| {
                            child_boxes.and_then(|children| {
                                self.estimate_multicol_auto_block_size(
                                    &geometry.style,
                                    stylesheets,
                                    children,
                                    geometry.content_inline().width().points(),
                                )
                            })
                        })
                        .flatten()
                });
            let current_fragmentainer = self.fragmentainer_from_page_cursor(
                PageTopBlockPosition::new(self.cursor_y + geometry.relative_offset.y()),
            );
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
            let should_prebreak = self.out_of_flow_prebreak_suppression_depth == 0
                && !fragments_as_promoted_spanner
                && should_prebreak_definite_block(DefiniteBlockBreakContext {
                    // A multicol formatting context has a measurable row-grid
                    // block size even when its principal `height` is auto. If
                    // its first anonymous column cannot make progress in the
                    // remaining outer column but the complete grid fits in an
                    // empty one, CSS fragmentation places it at that earlier
                    // class-A opportunity instead of creating a subpixel first
                    // column.
                    // <https://www.w3.org/TR/css-break-3/#unforced-breaks>
                    definite_content_height: prebreak_content_height,
                    vertical_non_content: geometry.vertical_non_content,
                    style: &geometry.style,
                    current_fragmentainer,
                    empty_destination_fragmentainer,
                    fragmentainer_has_occupied_flow: self.current_page_has_content()
                        || self.cursor_y < self.page_top() - 0.01,
                    at_page_top: self.cursor_is_at_page_top(),
                    suppress_for_avoid_retry: self.avoid_inside_retry_depth > 0,
                });
            if !should_prebreak {
                break;
            }
            self.push_page();
            geometry = self.block_layout_geometry(element, style, stylesheets, child_boxes);
        }
        // Inline atomic descendants can be measured while preparing the
        // block's child-flow strategy, before the child phase itself begins.
        // A supplied flex replay basis is authoritative, including an
        // explicit indefinite value; otherwise use the principal block's
        // definite content height.
        // <https://drafts.csswg.org/css-flexbox/#definite-sizes> and
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        let preparatory_descendant_percentage_basis = descendant_percentage_height_basis
            .unwrap_or_else(|| {
                block_size_percentage_basis_from_points(
                    geometry
                        .definite_content_height
                        .map(PhysicalContentHeight::points),
                    BlockSizeBasisSource::ContainingBlock,
                )
            });
        self.definite_block_size_stack
            .push(preparatory_descendant_percentage_basis);
        let defer_own_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = false;
        let suppress_own_principal_box_decoration = self.suppress_next_principal_box_decoration;
        self.suppress_next_principal_box_decoration = false;
        let containing_left = self.content_left;
        let containing_right = self.content_right;
        let containing_inline_size = (containing_right - containing_left).max(0.0);
        // Relative positioning normally enters before descendant layout so
        // the descendants paint in the shifted coordinate space. A cleared
        // relative box is the exception: `clear` must first resolve at its
        // unshifted normal-flow border edge (CSS 2.2, 9.5.2).
        if matches!(
            geometry.style.position,
            Position::Relative | Position::Sticky
        ) && geometry.style.clear == Clear::None
        {
            self.cursor_y += geometry.relative_offset.y();
        }
        let mut block_align_content_offset_y = 0.0;
        let starts_at_page_top = self.cursor_is_at_page_top() && self.truncate_page_start_margins;
        // CSS 2.2 defines clearance from the hypothetical border edge after
        // adjoining parent/first-child margins have collapsed. Resolve that
        // complete start-margin set before moving the border edge for `clear`;
        // the child traversal receives an explicit marker so it does not
        // consume that same descendant contribution a second time.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        let (hypothetical_start_margin, clearance_consumed_adjoining_start_margin) =
            if geometry.style.clear != Clear::None {
                if let Some(children) = child_boxes {
                    let can_adjoin = can_collapse_block_start_margin(
                        &geometry.style,
                        geometry.border_edges,
                        has_direct_inline_content_box(children),
                        self.used_overflow_for_element(element, &geometry.style),
                    ) && collapsible_first_child_start_margin_from_boxes(
                        children,
                        element,
                        &geometry.style,
                        self.document_canvas_overflow,
                    )
                    .is_some();
                    (
                        collapsible_start_margin_for_box(
                            element,
                            &geometry.style,
                            children,
                            self.document_canvas_overflow,
                        ),
                        can_adjoin,
                    )
                } else {
                    let has_direct_inline_content = has_direct_inline_content_dom_with_font_metrics(
                        element,
                        &geometry.style,
                        stylesheets,
                        &self.ancestors,
                        &mut self.font_system,
                    );
                    let can_adjoin = can_collapse_block_start_margin(
                        &geometry.style,
                        geometry.border_edges,
                        has_direct_inline_content,
                        self.used_overflow_for_element(element, &geometry.style),
                    )
                        && collapsible_first_child_start_margin_dom_with_font_metrics(
                            element,
                            &geometry.style,
                            stylesheets,
                            &self.ancestors,
                            &mut self.font_system,
                            self.document_canvas_overflow,
                        )
                        .is_some();
                    let mut resolver = DomStyleResolver::with_font_system(&mut self.font_system);
                    (
                        collapsible_start_margin_dom_with_resolver(
                            element,
                            &geometry.style,
                            stylesheets,
                            &self.ancestors,
                            &mut resolver,
                            self.document_canvas_overflow,
                        ),
                        can_adjoin,
                    )
                }
            } else {
                (geometry.style.margin.top, false)
            };
        let applied_start_margin =
            page_start_margin(layout_pt(hypothetical_start_margin), starts_at_page_top);
        let margin_edge_top = self.cursor_y;
        self.cursor_y -= applied_start_margin.points();
        let clearance_count_at_block_entry = self.applied_clearance_count;
        let establishes_independent_bfc = geometry
            .style
            .display
            .establishes_block_formatting_context()
            || layout_containment_applies_to_element(element, &geometry.style)
            || paint_containment_applies_to_element(element, &geometry.style)
            || self.element_used_overflow_clips(element, &geometry.style)
            || block_align_content_establishes_independent_formatting_context(
                geometry.style.align_content,
            );
        if !establishes_independent_bfc {
            let before_clear_page_index = self.pages.len();
            let before_clear_top = self.cursor_y;
            let cleared_top = self.clear_active_floats_top(
                geometry.style.clear,
                geometry.style.writing_mode,
                geometry.style.used_direction(),
                PageTopBlockPosition::new(self.cursor_y),
            );
            if self.pages.len() != before_clear_page_index
                || cleared_top.points() < before_clear_top - 0.01
            {
                self.applied_clearance_count += 1;
            }
            self.cursor_y = cleared_top.points();
        }
        if establishes_independent_bfc && geometry.style.float == Float::None {
            // A BFC root's `clear` is resolved from its hypothetical margin
            // edge.  This permits negative clearance when its start margin
            // would otherwise put the border edge below an adjoining float.
            let bfc_clear_top = self.clear_active_floats_top(
                geometry.style.clear,
                geometry.style.writing_mode,
                geometry.style.used_direction(),
                PageTopBlockPosition::new(margin_edge_top),
            );
            let bfc_clearance_applied = bfc_clear_top.points() < margin_edge_top - FLOAT_EPSILON;
            if bfc_clearance_applied {
                self.cursor_y = bfc_clear_top.points();
                self.applied_clearance_count += 1;
            }
            if self.containing_block_writing_mode == WritingMode::HorizontalTb
                && geometry.style.writing_mode == WritingMode::HorizontalTb
            {
                let context = self
                    .float_contexts
                    .last()
                    .expect("root float context exists")
                    .clone();
                let page_index = self.current_float_page_index();
                let clear = if bfc_clearance_applied {
                    Clear::None
                } else {
                    geometry.style.clear
                };
                let writing_mode = geometry.style.writing_mode;
                let direction = geometry.style.used_direction();
                let containing_inline_span =
                    PageInlineSpan::from_edges(containing_left, containing_right);
                let normal_border_top = self.cursor_y;
                let mut solve_placement = |top| {
                    context.avoiding_bfc_root_position(
                        page_index,
                        top,
                        clear,
                        writing_mode,
                        direction,
                        containing_left,
                        containing_right,
                        |band, _candidate_top| {
                            self.measure_float_avoiding_bfc(
                                element,
                                style,
                                stylesheets,
                                child_boxes,
                                containing_inline_size,
                                containing_inline_span,
                                band,
                            )
                        },
                    )
                };
                // A positive adjoining start margin can cross an active float
                // before it puts the BFC root's border box below that float.
                // Test the margin edge first.  When the root cannot occupy
                // that band, retain only the portion of the margin needed to
                // reach the float's block-end edge, just as clearance does.
                // If it fits beside the float, the normal margin placement is
                // retained and solved at the border edge below.
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                let margin_edge_placement = (applied_start_margin.points() > FLOAT_EPSILON
                    && !bfc_clearance_applied)
                    .then(|| solve_placement(PageTopBlockPosition::new(margin_edge_top)));
                let placement = match margin_edge_placement {
                    Some(placement)
                        if placement.placement.origin.top_y() < margin_edge_top - FLOAT_EPSILON
                            && placement.placement.origin.top_y()
                                > normal_border_top + FLOAT_EPSILON =>
                    {
                        placement
                    }
                    _ => solve_placement(PageTopBlockPosition::new(normal_border_top)),
                };
                self.cursor_y = placement.placement.origin.top_y();
                let available_span = placement.placement.available_span;
                let containing_inline_span =
                    PageInlineSpan::from_edges(containing_left, containing_right);
                let auto_border_box_width = (available_span.width()
                    < containing_inline_size - FLOAT_EPSILON)
                    .then_some(float_avoiding_auto_border_box_width(
                        available_span,
                        containing_inline_span,
                        style.margin.left,
                        style.margin.right,
                    ));
                geometry = self.block_layout_geometry_in_inline_span(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    BlockLayoutInlineConstraint {
                        containing_inline_span: available_span,
                        percentage_basis: PercentageBasis::definite(LogicalInlineContentSize::new(
                            content_box_pt(containing_inline_size),
                        )),
                        physical_width_percentage_basis: PhysicalContentWidth::new(content_box_pt(
                            containing_inline_size,
                        )),
                        auto_border_box_width,
                    },
                );
            } else {
                let style = &geometry.style;
                let margin_box_width = style.margin.left
                    + geometry.outer_inline().width().points()
                    + style.margin.right;
                let collision_height = geometry
                    .definite_content_height
                    .map(PhysicalContentHeight::points)
                    .unwrap_or(style.line_height)
                    + geometry.vertical_non_content.points()
                    + style.margin.top
                    + style.margin.bottom;
                let placement = self.place_float_avoiding_margin_box(
                    PageTopBlockPosition::new(self.cursor_y),
                    margin_box_size_pt(margin_box_width, collision_height),
                    style.clear,
                    style.writing_mode,
                    style.used_direction(),
                    self.containing_block_direction,
                );
                self.cursor_y = placement.origin.top_y();
                let outer_x =
                    placement.origin.x() + style.margin.left + geometry.relative_offset.x();
                geometry.outer_inline = BlockBorderBoxInlineBounds::new(PageInlineSpan::new(
                    outer_x,
                    geometry.outer_inline().span().width(),
                ));
                geometry.content_inline = BlockContentBoxInlineBounds::new(PageInlineSpan::new(
                    outer_x + geometry.border_edges.left.points() + style.padding.left,
                    geometry.content_inline().span().width(),
                ));
            }
        }
        let style = &geometry.style;
        let height_depends_on_intrinsic_content =
            needs_intrinsic_height_contribution(style.box_values.height.clone())
                || needs_intrinsic_height_contribution(style.box_values.min_height.clone())
                || needs_intrinsic_height_contribution(style.box_values.max_height.clone());
        let block_line_trim = self.effective_text_box_line_trim_for_style(style);
        let relative_offset = geometry.relative_offset;
        // Relative positioning shifts the box's painted position and its
        // descendants, but it does not change the box's normal-flow position.
        // Resolve margins, `clear`, and BFC float avoidance first; applying a
        // relative block offset before clearance would let clearance cancel a
        // negative `top` offset and incorrectly change following flow.
        // <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>
        if matches!(style.position, Position::Relative | Position::Sticky)
            && style.clear != Clear::None
        {
            self.cursor_y += relative_offset.y();
        }
        let border_edges = geometry.border_edges;
        let border_widths = border_edges.to_css_edges();
        let vertical_non_content = geometry.vertical_non_content;
        let vertical_extras = vertical_non_content.points();
        let containing_block_content_height = geometry.containing_block_content_height;
        let containing_block_height_basis = containing_block_content_height;
        let definite_content_height_for_children = geometry.definite_content_height;
        let definite_content_height =
            definite_content_height_for_children.map(PhysicalContentHeight::points);
        let multicol_content_height =
            definite_content_height.or_else(|| style.box_values.height.length_if_no_percent());
        let outer_inline = geometry.outer_inline();
        let content_inline = geometry.content_inline();
        let content_width = content_inline.width().points();
        let mut content_logical_inline_size = geometry.content_logical_inline_size().points();
        if element.tag.eq_ignore_ascii_case("body")
            && self.principal_flow.is_source_body(element)
            && style.writing_mode == WritingMode::VerticalRl
        {
            // A propagated body is the initial canvas. Its visual inline-end
            // margin offsets painting, but does not shorten the inline span
            // available to its canvas descendants.
            // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
            content_logical_inline_size +=
                match inline_start_side(style.writing_mode, style.used_direction()) {
                    PhysicalSide::Top => style.margin.bottom,
                    PhysicalSide::Bottom => style.margin.top,
                    PhysicalSide::Left | PhysicalSide::Right => {
                        unreachable!("a vertical writing mode must have a vertical inline axis")
                    }
                };
        }
        let built_multicol_child_boxes;
        let child_boxes = if child_boxes.is_none()
            && (style.column_count.is_some()
                || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
                || matches!(style.column_height, css::ComputedColumnHeight::Length(_)))
        {
            built_multicol_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            Some(built_multicol_child_boxes.as_slice())
        } else {
            child_boxes
        };
        let outer_x = outer_inline.span().left_x();
        let inner_x = content_inline.span().left_x();
        let inner_width = content_inline.width().points();
        if self.principal_flow.is_source_body(element)
            && inline_start_side(style.writing_mode, style.used_direction()) == PhysicalSide::Bottom
        {
            self.principal_inline_end_inset = style.margin.bottom;
        }
        if self.principal_flow.is_source_body(element)
            && self.principal_flow.writing_mode == WritingMode::HorizontalTb
        {
            self.principal_body_block_end_inset = layout_pt(style.margin.right);
        }
        // The active cursor already carries the principal flow's page-inline
        // origin. In particular, `sideways-lr` starts at the physical bottom;
        // re-anchoring its body against the page would move its first child
        // to the opposite edge.
        // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
        let block_top = self.cursor_y;
        let mut fragmented_definite_block = false;
        let positioning_containing_block_mode =
            PositionedContainingBlockMode::for_element(element, style);
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        let pending_paint_fragment_start = self.pending_paint_fragments.len();
        let pending_positioned_page_span_target_at_start = self.pending_positioned_page_span_target;
        let static_scroll_snap_scope =
            self.begin_static_scroll_snap_scope(style, element.tag.eq_ignore_ascii_case("html"));
        let block_start_page_context = self.current_page_context;
        let block_start_page_index = self.pages.len();
        self.cursor_y -= border_widths.top + style.padding.top;
        let content_top = self.cursor_y;
        self.fragment_top_offsets
            .push(self.current_page_context.top() - content_top);
        self.add_bookmark(element, style, paint_space_point(inner_x, block_top));
        self.add_page_anchor(element, style);
        let descendant_bookmark_start = self.bookmarks.len();

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_containing_block_direction = self.containing_block_direction;
        let previous_containing_block_writing_mode = self.containing_block_writing_mode;
        let is_vertical_orthogonal_flow = style.writing_mode.has_vertical_lines()
            && writing_modes_are_orthogonal(
                previous_containing_block_writing_mode,
                style.writing_mode,
            );
        let is_vertical_parallel_flow_auto_inline_size = style.writing_mode.has_vertical_lines()
            && !writing_modes_are_orthogonal(
                previous_containing_block_writing_mode,
                style.writing_mode,
            )
            && style.box_values.height.is_auto()
            && !needs_intrinsic_height_contribution(style.box_values.min_height.clone())
            && !needs_intrinsic_height_contribution(style.box_values.max_height.clone());
        self.content_left = inner_x;
        self.content_right = inner_x + inner_width;
        // A contained root or body keeps its ordinary principal box, but it
        // does not supply the document canvas.  Canvas-only geometry (the
        // fragment insets consumed by descendants and viewport overflow) must
        // follow the used propagation result rather than the element name.
        // <https://drafts.csswg.org/css-contain-1/#containment-layout>
        let is_document_canvas = self.element_propagates_document_canvas_properties(element, style);
        if is_document_canvas {
            self.document_canvas_fragment_insets.push(FragmentOffsets {
                left: inner_x - self.current_page_context.left(),
                right: self.current_page_context.right() - (inner_x + inner_width),
                top: self.current_page_context.top() - content_top,
            });
        }
        // A containing block exports its used inline base direction. In
        // vertical typographic mode, `text-orientation: upright` forces this
        // value to LTR without changing the computed value inherited by an
        // orthogonal descendant.
        // <https://drafts.csswg.org/css-writing-modes-4/#text-orientation>
        self.containing_block_direction = style.used_direction();
        self.containing_block_writing_mode = style.writing_mode;
        self.content_logical_inline_size_stack
            .push(content_logical_inline_size);
        let parent_child_available_space = self.current_child_available_space();
        let inherited_orthogonal_available_height =
            parent_child_available_space.orthogonal_available_height;
        let mut child_available_space = child_available_space_for_block(
            style,
            PhysicalContentWidth::new(content_inline.width()),
            definite_content_height_for_children,
            inherited_orthogonal_available_height,
            PhysicalContentHeight::new(content_box_pt(self.current_page_context.area_height())),
        );
        if is_document_canvas
            || (style.writing_mode.has_vertical_lines() && style.box_values.width.is_auto())
        {
            // The document canvas propagates the initial containing block's
            // available physical width to an orthogonal child. Its visual
            // margins offset the canvas content, but do not shrink the
            // viewport-sized available space used by the orthogonal-flow
            // sizing algorithm. A vertical auto-sized root applies the same
            // preserved basis to its direct horizontal child. Preserve an
            // already-exported basis rather than substituting the vertical
            // root's provisional auto physical width.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
            child_available_space = child_available_space
                .with_orthogonal_physical_width_percentage_basis(
                    parent_child_available_space.orthogonal_physical_width_percentage_basis,
                );
        }
        self.child_available_space_stack.push(child_available_space);
        if establishes_independent_bfc {
            self.push_float_context();
        }
        let positioned_containing_block_scope =
            if let Some(mode) = positioning_containing_block_mode {
                // Absolute descendants resolve percentage block sizes against the
                // positioned ancestor's final padding box. An auto-height block
                // gets that height from its in-flow children, which have not yet
                // been laid out at this point. Estimate the same normal-flow
                // contribution before entering the positioned-descendant scope so
                // those descendants do not observe the line-height placeholder.
                // The ordinary child pass remains authoritative for final flow
                // geometry and fragmentation.
                // <https://www.w3.org/TR/css-position-3/#def-cb>
                let positioning_content_height = definite_content_height.unwrap_or_else(|| {
                    (self.estimate_block_like_height(
                        element,
                        &geometry.style,
                        stylesheets,
                        content_width,
                        child_boxes,
                    ) - geometry.style.margin.top
                        - geometry.style.margin.bottom
                        - vertical_extras)
                        .max(0.0)
                });
                let containing_block = ContainingBlock::from_page_top_rect(
                    geometry.padding_box_top_rect(block_top, positioning_content_height),
                );
                Some(self.push_positioned_containing_block(mode, containing_block))
            } else {
                None
            };
        let overflow_clip_content_height = (!height_depends_on_intrinsic_content)
            .then(|| {
                used_content_box_height_or_auto(
                    style,
                    layout_pt(self.page_area_height()),
                    vertical_non_content,
                )
                .map(SemanticLengthExt::points)
            })
            .flatten()
            .map(|height| {
                constrain_content_height(
                    style,
                    content_box_pt(height),
                    PercentageBasis::definite(layout_pt(content_width)),
                )
                .points()
            });
        let used_overflow_clips = self.element_used_overflow_clips(element, style);
        // A deferred effect is required when the used height is intrinsic,
        // when paint containment supplies the clip, or when a longhand axis
        // creates the scroll container independently of the legacy shorthand
        // field. In the latter two cases primitive clipping would mutate the
        // alignment subject's recorded geometry. Definite shorthand overflow
        // can keep the existing primitive clip, which leaves descendant paint
        // bands public for CSS Appendix E ordering.
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
        // <https://www.w3.org/TR/css-contain-1/#containment-paint>
        let paint_containment_applies = paint_containment_applies_to_element(element, style);
        // A rectangular primitive clip would discard the corners before the
        // final paint effect can apply their border-radius and `corner-shape`
        // contour. Keep those descendants intact until that typed contour is
        // emitted as one effect scope.
        // <https://drafts.csswg.org/css-backgrounds-3/#corner-clipping> and
        // <https://drafts.csswg.org/css-borders-4/#corner-shaping>.
        let has_single_border_shape = match &style.border_shape {
            css::BorderShape::None | css::BorderShape::Pair { .. } => false,
            css::BorderShape::Circle(_)
            | css::BorderShape::Ellipse(_)
            | css::BorderShape::Path(_)
            | css::BorderShape::Inset(_)
            | css::BorderShape::Polygon(_) => true,
        };
        let needs_contoured_overflow_clip = used_overflow_clips
            && (!style.border_radius.clone().is_zero() || has_single_border_shape);
        let needs_deferred_overflow_clip = used_overflow_clips
            && (self.active_fragmentainer_kind() != FragmentainerKind::Column
                || definite_content_height.is_some()
                || paint_containment_applies
                || needs_contoured_overflow_clip);
        let overflow_clip_active = if used_overflow_clips
            && style_clips_overflow(style)
            && !paint_containment_applies
            // The deferred effect scopes one element-level clip around the
            // completed paint fragment. Keeping an eager clip as well turns
            // it into a synthetic clip for each text line, which can cut ink
            // that lies inside the element's real overflow clip edge.
            && !needs_deferred_overflow_clip
        {
            let clip_content_height = overflow_clip_content_height.unwrap_or_else(|| {
                (block_top
                    - border_widths.top
                    - style.padding.top
                    - style.padding.bottom
                    - self.page_bottom())
                .max(0.0)
            });
            let (clip_edge_x, clip_edge_y) = overflow_clip_edge_axes(style);
            let (clip_x, clip_y) = overflow_clipping_axes(style);
            let margin = if clip_edge_x || clip_edge_y {
                style.overflow_clip_margin.length
            } else {
                0.0
            };
            let clip_height = clip_content_height + style.padding.top + style.padding.bottom;
            self.push_overflow_clip(OverflowClip::from_paint_rect_with_axes(
                PageTopRect::new(
                    outer_x + border_widths.left - margin,
                    block_top - border_widths.top + margin,
                    content_width + style.padding.left + style.padding.right + margin * 2.0,
                    clip_height + margin * 2.0,
                )
                .paint_rect(),
                clip_x,
                clip_y,
            ));
            true
        } else {
            false
        };
        let has_single_unbreakable_inline_line = fragmentainer_kind == FragmentainerKind::Column
            && content_top - self.page_bottom() <= css::CSS_PX_TO_PT + 0.01
            && child_boxes.is_some_and(|children| {
                self.block_has_single_unbreakable_inline_line(
                    element,
                    style,
                    children,
                    content_width,
                )
            });
        let propagated_viewport_clip = self
            .document_canvas_overflow
            .is_viewport_overflow_source(element)
            && self
                .document_canvas_overflow
                .viewport_clips_block_fragmentation();
        let suppresses_descendant_fragmentation = (used_overflow_clips
            && definite_content_height.is_some())
            || has_single_unbreakable_inline_line
            || (style.contain.size && fragmentainer_kind == FragmentainerKind::Column)
            || propagated_viewport_clip;
        if suppresses_descendant_fragmentation {
            // Scroll containers with a definite size, an unbreakable line in
            // a zero-height column, and size-contained column subjects
            // establish monolithic outer-flow boxes. An auto-height overflow
            // box instead fragments with its contents, so the used block size
            // can be established from all of its fragments.
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            // <https://www.w3.org/TR/css-break-3/#monolithic>
            self.fragmentation_suppression_depth += 1;
        }

        let list_marker =
            self.marker_for_list_item(element, style, previous_containing_block_direction);

        // Generated pseudo-elements are tree-abiding boxes whose `content`
        // is evaluated by the pseudo-content path. They must never fall back
        // to the originating element's DOM children merely because an atomic
        // inline/float replay did not carry a frozen child list.
        // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
        let has_generated_content = style.content.is_generated();
        // A normalized child stream already owns its inline breaks. Inspecting
        // the raw DOM as well would resurrect a `<br>` whose computed display
        // was suppressed during box-tree construction (notably Appendix B's
        // `display: contents` → `none` rule for HTML controls).
        // <https://drafts.csswg.org/css-display-3/#unbox-html>
        let has_explicit_line_break = !has_generated_content
            && child_boxes.is_none()
            && element_has_direct_line_break(element);
        let use_ordered_mixed_flow = !has_generated_content
            && ((child_boxes.is_none()
                && has_ordered_mixed_flow_content_with_font_metrics(
                    element,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                ))
                // Formatting-tree normalization stores direct `<br>` nodes
                // outside the block-child sequence. When a block also has
                // normal-flow children, replaying all direct inline content
                // before that sequence changes DOM order; retain the ordered
                // mixed-flow path instead.
                // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
                || (has_explicit_line_break
                    && child_boxes.is_some_and(has_non_inline_formatting_box)));
        let has_normalized_flow_children = !has_generated_content
            && child_boxes
                .map(has_non_inline_formatting_box)
                .unwrap_or(false);
        let use_box_inline_items = !use_ordered_mixed_flow
            && !has_generated_content
            && !has_normalized_flow_children
            && child_boxes
                .map(|boxes| {
                    formatting_box_has_inline_content(boxes)
                        && boxes
                            .iter()
                            .any(|box_| !formatting_box_can_only_create_phantom_line_boxes(box_))
                })
                .unwrap_or(false);
        let has_run_in_inline_content = !run_in_children.is_empty();

        // If normalization consumed a run-in source's children, do not replay
        // its original DOM text here. Inline pseudo content that survives in
        // normalized inline boxes is handled through `use_box_inline_items`.
        let normalized_children_empty = child_boxes.is_some_and(|boxes| boxes.is_empty());
        let detached_normalized_text = normalized_children_empty
            && !has_generated_content
            && !inline_text_for_style(element, style).is_empty();
        let text = if normalized_children_empty
            || has_generated_content
            || use_ordered_mixed_flow
            || has_normalized_flow_children
            || use_box_inline_items
        {
            String::new()
        } else if is_document_canvas {
            own_inline_text_for_style(element, style)
        } else {
            inline_text_for_style(element, style)
        };
        // Frozen root children already contain tree-abiding `::after` boxes.
        // Do not also collect the pseudo from the root style in the leading
        // inline run; that run precedes the propagated body canvas and would
        // both duplicate the pseudo and reverse its DOM order.
        // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
        let defer_root_after_pseudo = element.tag.eq_ignore_ascii_case("html")
            && child_boxes.is_some()
            && style
                .after_style
                .as_deref()
                .is_some_and(|after| after.content.is_generated());
        let mut leading_inline_style;
        let inline_style = if defer_root_after_pseudo {
            leading_inline_style = style.clone();
            leading_inline_style.after_style = None;
            &leading_inline_style
        } else {
            style
        };
        let has_generated_inline_content = !detached_normalized_text
            && (generated_content_has_non_phantom_inline_content(inline_style)
                || (child_boxes.is_none()
                    && (inline_style.before_style.is_some()
                        || inline_style.after_style.is_some())));
        let has_styled_inline_descendant = has_styled_inline_descendant_with_font_metrics(
            element,
            style,
            stylesheets,
            &self.ancestors,
            &mut self.font_system,
        );
        // A `<br>` creates a line box even when the surrounding text has
        // collapsed away. Treat it as collectable inline content so its
        // forced boundaries are laid out—and fragmented—through the shared
        // line-record path rather than being discarded as empty text.
        // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
        let has_collectable_inline_content = inline_text_has_non_phantom_content(&text, style)
            || has_generated_inline_content
            || has_explicit_line_break;
        let use_inline_items = has_collectable_inline_content
            && (has_styled_inline_descendant
                || has_generated_inline_content
                || plain_inline_content_needs_inline_items(&text, style)
                || has_explicit_line_break
                || style.text_align.justifies()
                || self.active_float_exclusions_at(PageBlockSpan::new(
                    self.cursor_y,
                    style.line_height,
                )));
        if has_run_in_inline_content
            && !has_normalized_flow_children
            && let Some(child_boxes) = child_boxes
        {
            self.layout_run_in_inline_items_block(
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                element.attrs.get("href").map(String::as_str),
                list_marker.as_ref(),
            );
        // Ordered mixed-flow traversal owns every inline run, including a
        // standalone `<br>` between floated or block-level siblings. Laying
        // the parent inline items here as well would collect the complete DOM
        // subtree and place those floats a second time before the ordered
        // traversal reaches their source positions.
        // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
        } else if has_collectable_inline_content && !use_ordered_mixed_flow {
            let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
            if use_inline_items {
                let laid_out_multicol_inline_items = self.layout_multicol_inline_items_block(
                    element,
                    style,
                    stylesheets,
                    None,
                    (0.0, 0.0),
                    element.attrs.get("href").map(String::as_str),
                    list_marker.as_ref(),
                    multicol_content_height,
                );
                if !laid_out_multicol_inline_items {
                    self.layout_inline_items_block(
                        element,
                        inline_style,
                        stylesheets,
                        (0.0, 0.0),
                        element.attrs.get("href").map(String::as_str),
                        list_marker.as_ref(),
                    );
                }
            } else if style.display.is_list_item() {
                self.layout_list_text_block(
                    &text,
                    inline_style,
                    0.0,
                    0.0,
                    element.attrs.get("href").map(String::as_str),
                    list_marker.as_ref(),
                );
            } else {
                let laid_out_multicol_text = self.layout_multicol_text_block(
                    &text,
                    inline_style,
                    0.0,
                    0.0,
                    element.attrs.get("href").map(String::as_str),
                    multicol_content_height,
                );
                if !laid_out_multicol_text {
                    self.layout_text_block(
                        &text,
                        style,
                        0.0,
                        0.0,
                        element.attrs.get("href").map(String::as_str),
                    );
                }
            }
            self.pop_text_box_line_trim_scope(pushed_text_box_trim);
        }
        let mut box_inline_has_flow_effects = false;
        let mut laid_out_box_inline_multicol = false;
        if !has_run_in_inline_content
            && use_box_inline_items
            && !(has_collectable_inline_content && use_inline_items)
            && let Some(child_boxes) = child_boxes
        {
            let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
            // Collected text, forced-break boxes, and atomic inline fragments
            // go directly through the shared multicol line sequence. Atomic
            // fragments retain their owned paint/positioning state in the
            // sequence, so an ordinary anonymous-block pass here would paint
            // them once before the column planner paints them again.
            // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
            laid_out_box_inline_multicol = list_marker.is_none()
                && self.layout_multicol_inline_items_block(
                    element,
                    style,
                    stylesheets,
                    has_atomic_inline_formatting_box(child_boxes).then_some(child_boxes),
                    (0.0, 0.0),
                    element.attrs.get("href").map(String::as_str),
                    None,
                    multicol_content_height,
                );
            if !laid_out_box_inline_multicol {
                box_inline_has_flow_effects = self.layout_anonymous_block(
                    style,
                    child_boxes,
                    stylesheets,
                    list_marker.as_ref(),
                );
            }
            self.pop_text_box_line_trim_scope(pushed_text_box_trim);
        }
        let laid_out_column_children = laid_out_box_inline_multicol
            || (text.is_empty()
                && (self.layout_definition_list_columns(element, style, stylesheets, child_boxes)
                    || self.layout_simple_block_child_columns(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        multicol_content_height,
                    )));
        if laid_out_column_children
            && let Some(marker) = list_marker.as_ref()
            && marker.paints_outside()
        {
            // A multicol list item still owns one marker at the principal
            // box's block start. The column planner consumes the block
            // children, so paint that marker here rather than falling through
            // to the non-column empty-content fallback below.
            // <https://www.w3.org/TR/css-lists-3/#marker-position>
            self.paint_outside_marker(
                marker,
                style,
                self.content_left,
                self.content_right,
                content_top,
            );
        }
        let has_direct_inline_content = has_run_in_inline_content
            || box_inline_has_flow_effects
            || child_boxes.is_some_and(has_direct_inline_content_box)
            || has_collectable_inline_content
            || laid_out_column_children;
        if style.writing_mode != WritingMode::HorizontalTb && has_direct_inline_content {
            let vertical_inline_height = if use_box_inline_items
                && let Some(marker) = list_marker.as_ref()
                && marker.participates_in_first_line()
            {
                child_boxes
                    .map(|child_boxes| {
                        self.intrinsic_inline_measurement_for_boxes_with_marker(
                            child_boxes,
                            style,
                            marker,
                            stylesheets,
                            content_logical_inline_size,
                        )
                        .physical_height(style)
                    })
                    .unwrap_or(0.0)
            } else if let Some(marker) = list_marker.as_ref()
                && marker.participates_in_first_line()
                && !text.is_empty()
            {
                // In vertical writing, the physical height consumed by an
                // inside marker is part of the line's inline-axis advance.
                // Measuring only the principal text makes successive list
                // items overlap and ignores marker font-size changes.
                // <https://drafts.csswg.org/css-writing-modes-4/#vertical-layout>
                // <https://drafts.csswg.org/css-lists-3/#marker-position>
                let mut items = Vec::new();
                self.push_inside_marker_items(marker, style, None, &mut items);
                self.push_inline_words(
                    &text,
                    style,
                    None,
                    0.0,
                    InlineVisualOffset::zero(),
                    &mut items,
                );
                self.intrinsic_inline_measurement_for_items(
                    items,
                    style,
                    content_logical_inline_size,
                )
                .physical_height(style)
            } else if use_box_inline_items {
                child_boxes
                    .map(|child_boxes| {
                        self.intrinsic_inline_measurement_for_boxes(
                            child_boxes,
                            style,
                            stylesheets,
                            content_logical_inline_size,
                        )
                        .physical_height(style)
                    })
                    .unwrap_or(0.0)
            } else if !text.is_empty() {
                self.estimate_text_physical_height(
                    &text,
                    style,
                    content_logical_inline_size,
                    0.0,
                    0.0,
                )
            } else {
                0.0
            };
            if vertical_inline_height > 0.0 {
                self.cursor_y = self.cursor_y.min(content_top - vertical_inline_height);
            }
        }
        if let Some(marker) = list_marker.as_ref()
            && text.is_empty()
            && !has_collectable_inline_content
            && !use_box_inline_items
            && !laid_out_column_children
        {
            if marker.paints_outside() {
                if self.cursor_y - style.font_size < self.page_bottom() {
                    self.push_page();
                }
                self.paint_outside_marker(
                    marker,
                    style,
                    self.content_left,
                    self.content_right,
                    self.cursor_y,
                );
            } else {
                let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
                self.layout_list_text_block("", style, 0.0, 0.0, None, Some(marker));
                self.pop_text_box_line_trim_scope(pushed_text_box_trim);
            }
        }
        let can_collapse_start_margin = can_collapse_block_start_margin(
            style,
            border_edges,
            has_direct_inline_content,
            self.used_overflow_for_element(element, style),
        );
        let can_collapse_end_margin = can_collapse_block_end_margin(
            style,
            border_edges,
            has_direct_inline_content,
            self.used_overflow_for_element(element, style),
        );
        let self_collapsing_block = !has_direct_inline_content
            && if let Some(child_boxes) = child_boxes {
                is_self_collapsing_block_box(
                    element,
                    style,
                    child_boxes,
                    self.document_canvas_overflow,
                )
            } else {
                is_self_collapsing_block_dom_with_font_metrics(
                    element,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                    self.document_canvas_overflow,
                )
            };
        // Relative positioning percentages resolve against the normal-flow
        // containing block even when that block does not establish an
        // absolute-positioning containing block. Keep this scope separate
        // from `containing_blocks`, which intentionally tracks only the
        // positioned-containing-block chain.
        // <https://www.w3.org/TR/css-position-3/#relative-positioning>.
        self.normal_flow_relative_containing_blocks
            .push(NormalFlowRelativeContainingBlock {
                physical_content_width: PhysicalContentWidth::new(content_box_pt(content_width)),
                physical_content_height: definite_content_height
                    .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
            });
        // Anonymous inline wrappers do not retain their originating block's
        // principal geometry. Preserve that geometry while traversing the
        // children so a blockified positioned descendant can form the CSS
        // static-position rectangle in this block's writing mode.
        // <https://www.w3.org/TR/css-position-3/#static-position>
        let static_content_height = definite_content_height.unwrap_or_else(|| {
            containing_block_content_height
                .points()
                .map(|height| (height - vertical_extras).max(0.0))
                .unwrap_or(0.0)
        });
        self.block_static_position_contexts
            .push(BlockStaticPositionContext {
                writing_mode: style.writing_mode,
                direction: style.used_direction(),
                content_left: self.content_left,
                content_right: self.content_right,
                content_top_y: content_top,
                content_height: static_content_height,
                physical_block_size_is_auto: style.box_values.width.is_auto(),
            });
        let children_outcome =
            self.layout_block_flow_children_phase(Box::new(BlockFlowChildrenPhaseInput {
                fragmentainer_kind,
                element,
                style,
                stylesheets,
                child_boxes,
                can_collapse_start_margin,
                can_collapse_end_margin,
                applied_start_margin,
                clearance_consumed_adjoining_start_margin,
                starts_at_page_top,
                laid_out_column_children,
                use_box_inline_items,
                use_ordered_mixed_flow,
                has_preceding_inline_flow_content: has_collectable_inline_content
                    && !use_ordered_mixed_flow,
                definite_content_height,
                descendant_percentage_height_basis,
            }));
        self.normal_flow_relative_containing_blocks.pop();
        self.block_static_position_contexts.pop();
        self.definite_block_size_stack.pop();
        let pending_end_margin_collapse = children_outcome.pending_end_margin_collapse;
        let collapsed_start_margin_offset = children_outcome.collapsed_start_margin_offset;
        let clamp_line_slots =
            self.finish_clamp_line_slot_capture() + children_outcome.descendant_clamp_line_slots;
        if suppresses_descendant_fragmentation {
            self.fragmentation_suppression_depth -= 1;
        }

        let mut independent_bfc_had_float_content = false;
        if establishes_independent_bfc
            && has_auto_height(style)
            && let Some((float_page_index, float_bottom)) =
                self.current_float_context_last_fragment_end()
        {
            independent_bfc_had_float_content = true;
            while self.pages.len() < float_page_index {
                self.push_page();
            }
            self.cursor_y = self.cursor_y.min(float_bottom.points());
        }
        if establishes_independent_bfc {
            self.pop_float_context();
        }
        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        self.pop_overflow_clip(overflow_clip_active);
        self.child_available_space_stack.pop();
        self.content_logical_inline_size_stack.pop();
        self.restore_page_area_parent_context_after_page_transition(
            previous_left,
            previous_right,
            block_start_page_context,
            block_start_page_index,
        );
        if is_document_canvas {
            self.document_canvas_fragment_insets.pop();
        }
        self.containing_block_direction = previous_containing_block_direction;
        self.containing_block_writing_mode = previous_containing_block_writing_mode;

        if self_collapsing_block
            && !independent_bfc_had_float_content
            && self.pages.len() == paint_page_index
            && self.applied_clearance_count == clearance_count_at_block_entry
        {
            self.cursor_y = content_top;
        }

        let mut block_end_margin_to_consume = style.margin.bottom;
        if let Some(pending) = pending_end_margin_collapse {
            let content_height_with_child_margin = content_top - self.cursor_y;
            let content_height_without_child_margin =
                content_height_with_child_margin - pending.child_consumed_margin.points();
            self.cursor_y += pending.child_consumed_margin.points();
            if block_end_margin_collapse_survives_height_constraints(
                style,
                PhysicalContentWidth::new(content_inline.width()),
                vertical_non_content,
                PhysicalContentHeight::new(content_box_pt(content_height_without_child_margin)),
            ) {
                block_end_margin_to_consume = pending.collapsed_margin.points();
            }
        }

        if is_vertical_orthogonal_flow && definite_content_height.is_none() {
            // Orthogonal auto sizing has selected the vertical box's used
            // logical inline content size through fit-content negotiation.
            // That used size controls its own box geometry, even though the
            // fallback that selected it remains an indefinite percentage
            // basis for descendants.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
            self.cursor_y = content_top - content_logical_inline_size;
        } else if is_vertical_parallel_flow_auto_inline_size {
            // In vertical writing, physical height is logical inline size. A
            // normal-flow block with an automatic inline size fills its
            // containing block's available inline size, just as an automatic
            // physical width does in horizontal block flow. The inline layout
            // pass selected that content-box size, so retain it rather than
            // collapsing to the height contributed by inline contents.
            // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
            self.cursor_y = content_top - content_logical_inline_size;
        } else if definite_content_height.is_some()
            || used_min_height(style, containing_block_height_basis).is_some()
            || used_max_height(style, containing_block_height_basis).is_some()
            || style.box_values.min_height == css::ComputedLengthPercentageOrAuto::Stretch
            || style.box_values.max_height == css::ComputedLengthPercentageOrAuto::Stretch
            || height_depends_on_intrinsic_content
            || intrinsic_physical_height_is_contained(style)
        {
            // Size containment fixes the principal box's used size as if it
            // had no content. Descendants are still laid out and painted in
            // place, but their overflow does not move following siblings.
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            let current_content_height = if intrinsic_physical_height_is_contained(style) {
                style
                    .contain_intrinsic_size
                    .height
                    .clone()
                    .map(|height| {
                        used_length_percentage(
                            height,
                            PercentageBasis::definite(layout_pt(content_width.max(0.0))),
                        )
                        .points()
                    })
                    .unwrap_or(0.0)
            } else {
                content_top - self.cursor_y
            };
            // An intrinsic min/max constraint is resolved only after in-flow
            // content has been measured, but it must constrain the automatic
            // preferred height supplied by aspect-ratio rather than replace
            // it with that content height. This lets calc-size(auto, …) use
            // the content-derived automatic minimum independently from the
            // ratio-derived preferred size.
            // <https://www.w3.org/TR/css-sizing-4/#aspect-ratio> and
            // <https://drafts.csswg.org/css-values-5/#calc-size>.
            let aspect_ratio_preferred_height = style
                .box_values
                .height
                .is_auto()
                .then(|| {
                    non_replaced_aspect_ratio_content_height(
                        style,
                        content_width,
                        border_widths.left
                            + border_widths.right
                            + style.padding.left
                            + style.padding.right,
                        vertical_extras,
                    )
                })
                .flatten();
            let mut requested_content_height = definite_content_height
                .or(aspect_ratio_preferred_height)
                .unwrap_or_else(|| {
                    used_content_box_height_or_auto_with_basis(
                        style,
                        containing_block_content_height,
                        vertical_non_content,
                    )
                    .map(SemanticLengthExt::points)
                    .unwrap_or(current_content_height)
                });
            // A preferred `calc-size()` block size substitutes its automatic
            // basis only after in-flow content has been measured. At this
            // point normal-flow layout has that content contribution, so
            // retain CSS Math bounds such as `min(size, 100px)` instead of
            // treating the preferred size as ordinary `auto`.
            // <https://drafts.csswg.org/css-values-5/#calc-size>.
            if definite_content_height.is_none()
                && let css::ComputedLengthPercentageOrAuto::CalcSize(value) =
                    &style.box_values.height
            {
                requested_content_height = calc_size_intrinsic_constraint(
                    value.clone(),
                    style.box_sizing,
                    PercentageBasis::definite(content_box_pt(content_width)),
                    vertical_non_content,
                    content_box_pt(current_content_height),
                    content_box_pt(current_content_height),
                )
                .map(SemanticLengthExt::points)
                .unwrap_or(requested_content_height);
            }
            // A preferred aspect ratio supplies the automatic preferred block
            // size, but it does not discard the content-based automatic
            // minimum of an ordinary flow box. The final used height must
            // therefore accommodate in-flow content when `min-height:auto`.
            // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
            if style.box_values.height.is_auto()
                && style
                    .aspect_ratio
                    .preferred_ratio_for_non_replaced(false)
                    .is_some()
                && style.box_values.min_height.is_auto()
                && !style.overflow_y.is_scrollable()
                && !intrinsic_physical_height_is_contained(style)
            {
                requested_content_height = requested_content_height.max(current_content_height);
            }
            let height = if height_depends_on_intrinsic_content {
                constrain_height_with_intrinsic(
                    style,
                    content_box_pt(requested_content_height),
                    content_box_pt(current_content_height),
                    content_box_pt(current_content_height),
                    containing_block_height_basis,
                    non_content_pt(vertical_extras),
                )
                .points()
            } else {
                containing_block_content_height.points().map_or_else(
                    || {
                        constrain_content_height(
                            style,
                            content_box_pt(requested_content_height),
                            containing_block_height_basis,
                        )
                        .points()
                    },
                    |basis| {
                        constrain_height_with_stretch_fit(
                            style,
                            content_box_pt(requested_content_height),
                            layout_pt(basis),
                            layout_pt(style.margin.top + style.margin.bottom),
                            vertical_non_content,
                        )
                        .points()
                    },
                )
            };
            if self.pages.len() == paint_page_index
                && style.writing_mode == WritingMode::HorizontalTb
            {
                let free_space = height - current_content_height;
                block_align_content_offset_y = if laid_out_column_children {
                    multicol_align_content_y_offset(style.align_content, free_space)
                } else {
                    block_align_content_y_offset_for_style(style, free_space)
                };
            }
            let definite_block_overflows_fragmentainer = self.fragmentation_suppression_depth == 0
                && style.writing_mode == WritingMode::HorizontalTb
                && !(style.contain.size && fragmentainer_kind == FragmentainerKind::Column)
                && !has_single_unbreakable_inline_line
                // Root/body overflow propagated to the viewport is clipped
                // by the viewport rather than fragmented into additional
                // document pages.
                // <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>
                && !propagated_viewport_clip
                && height > content_top - self.page_bottom() + 0.01;
            if self.pages.len() > paint_page_index && definite_block_overflows_fragmentainer {
                // Descendant layout may already have crossed one or more
                // fragmentainers before the definite principal size is
                // applied (for example, a line starting at an exhausted page
                // edge or nested multicol rows). Consume only the unoccupied
                // remainder of the authored block size; reapplying the full
                // size double-counts those fragments, while using a page-local
                // subtraction drops the remaining continuous extent.
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
                let consumed_content_height = promoted_spanner_paint_slices(
                    paint_page_index,
                    self.pages.len(),
                    content_top,
                    self.cursor_y,
                    block_start_page_context,
                    self.current_page_context,
                    self.fragmentainer_override,
                )
                .iter()
                .map(|slice| (slice.top - slice.bottom).max(0.0))
                .sum::<f32>();
                let remaining_content_height = (height - consumed_content_height).max(0.0);
                if remaining_content_height > 0.01 {
                    self.consume_definite_block_size_through_fragmentainers(
                        self.cursor_y,
                        remaining_content_height,
                    );
                }
                fragmented_definite_block = true;
            } else if self.pages.len() == paint_page_index
                && (fragments_as_promoted_spanner || definite_block_overflows_fragmentainer)
            {
                self.consume_definite_block_size_through_fragmentainers(content_top, height);
                fragmented_definite_block = self.pages.len() > paint_page_index;
            } else {
                self.cursor_y = content_top - height;
            }
        }
        self.fragment_top_offsets.pop();
        self.cursor_y -= style.padding.bottom + border_widths.bottom;
        let block_bottom = self.cursor_y;
        let fragmented_spanner_slices = if (fragments_as_promoted_spanner
            || fragmented_definite_block)
            && self.pages.len() > paint_page_index
        {
            promoted_spanner_paint_slices(
                paint_page_index,
                self.pages.len(),
                block_top,
                block_bottom,
                block_start_page_context,
                self.current_page_context,
                self.fragmentainer_override,
            )
        } else {
            Vec::new()
        };
        // An ordinary auto-sized block that crosses fragmentainers owns one
        // fragment box in every fragmentainer it reaches. Its decoration is
        // therefore painted per fragment, just like a definite promoted
        // spanner, rather than being attached only to the final page on which
        // used-size resolution happens. In particular, a forced column break
        // extends every continued fragment through the remaining column and
        // its background must cover that extent.
        // <https://www.w3.org/TR/css-break-3/#break-decoration>
        // <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>
        // Floats and positioned descendants do not advance the normal-flow
        // cursor, but their committed destination fragmentainer still splits
        // an auto-height ancestor. Include that destination in the ancestor's
        // decoration span so its background is present behind content in each
        // real box fragment.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        // <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>
        let out_of_flow_fragmentainer_end = self
            .pending_paint_fragments
            .get(pending_paint_fragment_start..)
            .unwrap_or_default()
            .iter()
            .map(|fragment| fragment.page_index)
            .chain(
                self.positioned_layers
                    .get(positioned_layer_start..)
                    .unwrap_or_default()
                    .iter()
                    .map(|layer| layer.page_index),
            )
            .chain(self.pending_positioned_page_span_target.filter(|target| {
                pending_positioned_page_span_target_at_start.is_none_or(|initial| *target > initial)
            }))
            .max();
        // A deferred out-of-flow placement can discover a static position
        // beyond the current column set while its in-flow containing block is
        // still committing the next fragment. That placement must not make
        // the auto-height ancestor manufacture further box fragments; its
        // immediate continuation is the committed decoration boundary.
        let block_fragmentainer_end = out_of_flow_fragmentainer_end
            .map_or(self.pages.len(), |end| {
                end.max(self.pages.len()).min(self.pages.len() + 1)
            });
        let out_of_flow_continues_block = block_fragmentainer_end > self.pages.len();
        let block_end_page_context = if out_of_flow_continues_block {
            self.fragmentainer_override
                .map(|override_| override_.context_for_fragmentainer(block_fragmentainer_end))
                .unwrap_or_else(|| self.resolved_page_context(block_fragmentainer_end + 1, false))
        } else {
            self.current_page_context
        };
        let decoration_block_bottom = if out_of_flow_continues_block {
            block_end_page_context.bottom()
        } else {
            block_bottom
        };
        let fragmented_block_slices = if fragmented_spanner_slices.is_empty()
            && block_fragmentainer_end > paint_page_index
            && (definite_content_height.is_none() || fragmented_definite_block)
        {
            promoted_spanner_paint_slices(
                paint_page_index,
                block_fragmentainer_end,
                block_top,
                decoration_block_bottom,
                block_start_page_context,
                block_end_page_context,
                self.fragmentainer_override,
            )
        } else {
            Vec::new()
        };
        let block_height = if fragmented_spanner_slices.is_empty() {
            (block_top - block_bottom).max(0.0)
        } else {
            fragmented_spanner_slices
                .iter()
                .map(|slice| (slice.top - slice.bottom).max(0.0))
                .sum()
        };
        let paint_block_top = block_top - collapsed_start_margin_offset.points();
        let paint_block_height = (block_height - collapsed_start_margin_offset.points()).max(0.0);
        let border_box = geometry.border_box_top_rect(paint_block_top, paint_block_height);
        let border_paint_rect = border_box.page_top_rect().paint_rect();
        self.record_static_scroll_snap_area(element, style, border_paint_rect);
        self.record_static_scroll_target_area(element.is_target, border_paint_rect, style);
        // CSS Break permits an oversized monolithic box to be sliced when it
        // cannot fit a fragmentainer. Keep decoration-only size-contained
        // boxes intact (matching replaced elements), while allowing boxes
        // with fragmentable contents to use Quire's contiguous slice path.
        // <https://www.w3.org/TR/css-break-3/#breaking-rules>
        let retain_size_contained_monolithic_paint = style.contain.size
            && (border_paint_rect.size.height <= self.page_area_height() + 0.01
                || crate::text::trim_css_collapsible_whitespace(&inline_text_for_style(
                    element, style,
                ))
                .is_empty());
        if element.tag.eq_ignore_ascii_case("html") {
            self.record_document_canvas_root_positioning_area(
                PaintBackgroundArea::from_paint_rect(border_paint_rect),
            );
            self.document_canvas_overflow.record_auto_overflow(
                border_paint_rect.size.width,
                border_paint_rect.size.height,
                self.current_page_context.area_width(),
                self.current_page_context.area_height(),
            );
        }
        // Auto-height overflow clips know their inline and block-start edges
        // before child layout, but the block-end edge is only available after
        // resolving the used height. CSS Overflow clips to the used padding box:
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
        let deferred_overflow_clip = needs_deferred_overflow_clip.then(|| {
            let clip_content_height = (block_height - vertical_extras).max(0.0);
            let (x, y, width, height) = match style.overflow_clip_margin.reference_box {
                css::OverflowClipMarginBox::Border => (
                    outer_x,
                    block_top,
                    content_width
                        + style.padding.left
                        + style.padding.right
                        + border_widths.left
                        + border_widths.right,
                    clip_content_height
                        + style.padding.top
                        + style.padding.bottom
                        + border_widths.top
                        + border_widths.bottom,
                ),
                css::OverflowClipMarginBox::Content => (
                    outer_x + border_widths.left + style.padding.left,
                    block_top - border_widths.top - style.padding.top,
                    content_width,
                    clip_content_height,
                ),
                css::OverflowClipMarginBox::Padding => (
                    outer_x + border_widths.left,
                    block_top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    clip_content_height + style.padding.top + style.padding.bottom,
                ),
            };
            let margin = if style.overflow_x == css::Overflow::Clip
                || style.overflow_y == css::Overflow::Clip
            {
                style.overflow_clip_margin.length
            } else {
                0.0
            };
            PageTopRect::new(
                x - margin,
                y + margin,
                width + margin * 2.0,
                height + margin * 2.0,
            )
            .paint_clip()
        });
        let deferred_rounded_overflow_clip = deferred_overflow_clip.and_then(|_| {
            let reference_box = match style.overflow_clip_margin.reference_box {
                css::OverflowClipMarginBox::Border => css::BackgroundBox::Border,
                css::OverflowClipMarginBox::Padding => css::BackgroundBox::Padding,
                css::OverflowClipMarginBox::Content => css::BackgroundBox::Content,
            };
            let outset = if style.overflow_x == css::Overflow::Clip
                || style.overflow_y == css::Overflow::Clip
            {
                style.overflow_clip_margin.length
            } else {
                0.0
            };
            rounded_clip_rect_for_box_with_outset(
                border_paint_rect,
                style,
                border_widths,
                reference_box,
                outset,
            )
        });
        let deferred_border_shape_overflow_clip = deferred_overflow_clip.and_then(|_| {
            single_border_shape_overflow_clip(border_paint_rect, style, border_widths)
        });
        if block_height > 0.0 {
            self.mark_current_page_flow_content();
        }
        // A definite principal box that fits its originating fragmentainer
        // keeps its decoration there even when visible descendant overflow
        // materializes later fragmentainers. Those later pages belong to the
        // descendant paint, not to fragments of the principal box.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        let background_page_index = if definite_content_height.is_some()
            && !fragmented_definite_block
            && !fragments_as_promoted_spanner
        {
            paint_page_index
        } else {
            self.pages.len()
        };
        let propagates_document_canvas_properties =
            self.element_propagates_document_canvas_properties(element, style);
        let mut own_background_primitives = Vec::new();
        let mut own_outline_primitives = Vec::new();
        if propagates_document_canvas_properties {
            if style.visibility == Visibility::Visible {
                self.capture_document_canvas_background(element, style);
            }
            // Capturing the root background creates the canvas-background
            // record. Preserve the used root box after that creation: a
            // position-fixed root can have a much smaller positioning area
            // than the canvas it paints through, and percentages in
            // `background-size` resolve against this box rather than the
            // page canvas.
            // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
            // <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>
            if element.tag.eq_ignore_ascii_case("html") {
                self.record_document_canvas_root_positioning_area(
                    PaintBackgroundArea::from_paint_rect(border_paint_rect),
                );
            }
            if !suppress_own_principal_box_decoration
                && block_height > 0.0
                && (used_border_width(style) > layout_pt(0.0)
                    || style.border_image.source.is_image())
                && style.visibility == Visibility::Visible
            {
                // CSS Backgrounds propagates the root/body background to the
                // canvas, but borders are not canvas backgrounds; they remain
                // ordinary element border painting behind descendants:
                // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>.
                let mut border_style = style.clone();
                border_style.background_color = None;
                border_style.background_image = css::ComputedImage::None;
                border_style.background_layers.clear();
                own_background_primitives =
                    self.box_background_primitives(border_paint_rect, &border_style);
            }
        } else if !suppress_own_principal_box_decoration
            && fragmented_spanner_slices.is_empty()
            && fragmented_block_slices.is_empty()
            && border_paint_rect.size.width > 0.0
            && border_paint_rect.size.height > 0.0
            && (style.background_color.is_some()
                || style.background_image.is_image()
                || style.border_image.source.is_image()
                || used_border_width(style) > layout_pt(0.0))
            && style.visibility == Visibility::Visible
        {
            // A non-propagated body background remains an ordinary child of a
            // transformed root, but the root/body initial canvas is the page
            // media box rather than the page content rectangle. Keep that
            // background inside the root's effect context so the transform
            // still applies, while resolving its own tile geometry against
            // the full canvas.
            // <https://drafts.csswg.org/css-backgrounds-3/#special-backgrounds>
            let background_paint_rect = if element.tag.eq_ignore_ascii_case("body")
                && self.document_canvas_root_background_defined()
                && !self.fixed_containing_blocks.is_empty()
            {
                paint_space_rect(
                    0.0,
                    0.0,
                    self.current_page.width(),
                    self.current_page.height(),
                )
            } else {
                border_paint_rect
            };
            own_background_primitives =
                self.box_background_primitives(background_paint_rect, style);
        }
        if !suppress_own_principal_box_decoration
            && fragmented_spanner_slices.is_empty()
            && fragmented_block_slices.is_empty()
            && border_paint_rect.size.width > 0.0
            && border_paint_rect.size.height > 0.0
            && style.visibility == Visibility::Visible
        {
            own_outline_primitives = self.box_outline_primitives(border_paint_rect, style);
        }
        let has_own_background_primitives = !own_background_primitives.is_empty();
        let has_own_outline_primitives = !own_outline_primitives.is_empty();
        let scroll_content_bounds = self
            .current_page
            .paint_tree_fragment_since(&paint_checkpoint)
            .bounds()
            .map(PaintClip::paint_rect)
            .unwrap_or(border_paint_rect);
        let scroll_padding_box = deferred_overflow_clip
            .map(PaintClip::paint_rect)
            .unwrap_or(border_paint_rect);
        let static_scroll_offset = self.finish_static_scroll_snap_scope(
            static_scroll_snap_scope,
            scroll_padding_box,
            scroll_content_bounds,
        );
        let static_scroll_translation =
            crate::layout::scroll_snap::static_scroll_translation(static_scroll_offset, style);
        // Positioned descendants escape normal-flow paint capture, but remain
        // contents of this scroll container. Apply the same static scroll
        // translation and overflow clip before they are replayed into the
        // ancestor stacking context.
        // <https://www.w3.org/TR/css-overflow-3/#scrollable>
        if static_scroll_translation.x != 0.0 || static_scroll_translation.y != 0.0 {
            for layer in self
                .positioned_layers
                .get_mut(positioned_layer_start..)
                .unwrap_or_default()
            {
                *layer = layer.clone().translated(static_scroll_translation);
            }
        }
        if let Some(overflow_clip) = deferred_overflow_clip {
            let content_overflow_clip = match deferred_border_shape_overflow_clip {
                Some(BorderShapeOverflowClip::Empty) => {
                    PaintClip::new(overflow_clip.x(), overflow_clip.y(), 0.0, 0.0)
                }
                Some(BorderShapeOverflowClip::Path(_)) | None => overflow_clip,
            };
            for layer in self
                .positioned_layers
                .get_mut(positioned_layer_start..)
                .unwrap_or_default()
            {
                // An escaped positioned layer still belongs to this scroll
                // container, but an overflow scope that cannot exclude any
                // of its recorded ink is not a paint-order boundary. In
                // particular, retaining it changes PDF edge coverage where
                // a later opaque in-flow background fully covers the layer.
                // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
                // A rectangular-bounds containment check cannot prove that a
                // layer fits a curved `border-shape` contour.  Keep the
                // scope so positioned descendants are clipped at the shape,
                // even when they lie wholly inside its rectangular bounds.
                let layer_is_wholly_inside_clip = deferred_border_shape_overflow_clip.is_none()
                    && layer
                        .context
                        .bounds
                        .is_some_and(|bounds| content_overflow_clip.contains(bounds));
                if layer_is_wholly_inside_clip {
                    continue;
                }
                layer.context.effects.overflow_clip = Some(
                    layer
                        .context
                        .effects
                        .overflow_clip
                        .and_then(|existing| existing.intersect(content_overflow_clip))
                        .unwrap_or(content_overflow_clip),
                );
                // Out-of-flow positioned descendants escape the ordinary
                // fragment capture, so carry the enclosing contour onto the
                // layer that will later be replayed into the ancestor stack.
                // The rectangular clip above remains the conservative bounds
                // for links and raster culling; this typed contour supplies
                // the visible inner edge.
                if let Some(BorderShapeOverflowClip::Path(shape_clip)) =
                    deferred_border_shape_overflow_clip.clone()
                {
                    layer.context.effects.clip_path = PaintClipPathEffect::Path(shape_clip);
                    layer.context.effects.rounded_overflow_clip = None;
                } else if let Some(rounded_clip) = deferred_rounded_overflow_clip {
                    layer.context.effects.rounded_overflow_clip = Some(rounded_clip);
                }
            }
        }
        self.translate_aligned_block_descendant_bookmarks(
            descendant_bookmark_start,
            paint_page_index,
            0.0,
            block_align_content_offset_y,
        );
        if self.preserve_scoped_paint_public_order
            && self.pages.len() == paint_page_index
            && block_align_content_offset_y.abs() <= 0.01
            && !vertical_block_align_content_needs_fragment_bounds(style)
        {
            let mut fragment = self
                .current_page
                .paint_tree_fragment_since(&paint_checkpoint);
            // Descendant block backgrounds are captured in BackgroundBorder,
            // but that band is reserved for this box's own decoration by the
            // overflow helper. Promote descendants before creating the clip
            // scope so their paint remains part of the scrolling contents.
            fragment.promote_background_border_to_in_flow_block();
            if static_scroll_translation.x != 0.0 || static_scroll_translation.y != 0.0 {
                fragment = fragment.translated(static_scroll_translation);
            }
            if let Some(overflow_clip) = deferred_overflow_clip {
                fragment = if let Some(BorderShapeOverflowClip::Path(shape_clip)) =
                    deferred_border_shape_overflow_clip.clone()
                {
                    fragment.with_contents_effect_scoped_to_path(overflow_clip, shape_clip)
                } else if matches!(
                    deferred_border_shape_overflow_clip,
                    Some(BorderShapeOverflowClip::Empty)
                ) {
                    fragment.with_contents_effect_scoped_to_rect(PaintClip::new(
                        overflow_clip.x(),
                        overflow_clip.y(),
                        0.0,
                        0.0,
                    ))
                } else if let Some(rounded_clip) = deferred_rounded_overflow_clip {
                    fragment
                        .with_contents_effect_scoped_to_rounded_rect(overflow_clip, rounded_clip)
                } else if paint_containment_applies {
                    fragment
                        .with_primitives_clipped_to_rect_preserving_structure(overflow_clip)
                        .with_paint_containment_contents_effect_scoped_to_rect(overflow_clip)
                } else {
                    fragment
                        .with_primitives_clipped_to_rect_preserving_structure(overflow_clip)
                        .with_contents_effect_scoped_to_rect_preserving_contained_ink(
                            &self.current_page,
                            overflow_clip,
                        )
                };
            }
            if background_page_index == paint_page_index {
                self.current_page.prepend_recorded_primitives_to_fragment(
                    &mut fragment,
                    PaintBand::BackgroundBorder,
                    own_background_primitives,
                );
                self.current_page.append_recorded_primitives_to_fragment(
                    &mut fragment,
                    PaintBand::Outline,
                    own_outline_primitives,
                );
            }
            if !defer_own_decoration_promotion {
                fragment.promote_background_border_to_in_flow_block();
            }
            if retain_size_contained_monolithic_paint {
                fragment = fragment.with_monolithic_fragmentation_scope(
                    PaintClip::from_paint_rect(border_paint_rect),
                );
            }
            if ((has_own_background_primitives || has_own_outline_primitives)
                || deferred_overflow_clip.is_some())
                && !fragment.is_empty()
            {
                self.current_page
                    .replace_paint_tree_since_with_fragment(&paint_checkpoint, fragment);
            }
            self.cursor_y -= block_end_margin_to_consume;
            self.last_block_layout_outcome = BlockLayoutOutcome {
                consumed_bottom_margin: layout_pt(block_end_margin_to_consume),
                physical_border_box_inline_span: outer_inline.width(),
                clamp_line_slots,
            };
            if matches!(style.position, Position::Relative | Position::Sticky) {
                self.cursor_y -= relative_offset.y();
            }
            self.apply_forced_break_after_box_in(fragmentainer_kind, style);
            return;
        }
        let fragments = self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        let first_descendant_paint_page = fragments
            .iter()
            .filter(|(_, fragment)| !fragment.is_empty())
            .map(|(page_index, _)| *page_index)
            .min();
        // If an auto-height block has no start-edge material or direct inline
        // content, and its first child prebreaks, the block's first fragment
        // begins with that child. Painting a decoration-only fragment in the
        // preceding remainder would manufacture background before the box has
        // generated any fragment content.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let suppress_leading_empty_block_fragment = out_of_flow_fragmentainer_end.is_none()
            && fragmentainer_kind == FragmentainerKind::Page
            && has_auto_height(style)
            && !has_direct_inline_content
            && border_widths.top <= 0.01
            && style.padding.top <= 0.01
            && first_descendant_paint_page.is_some_and(|page| page > paint_page_index);
        let mut decorated_block_pages = Vec::new();
        let mut translated_vertical_bookmarks = false;
        for (page_index, mut fragment) in fragments {
            // A non-fragmented overflow-clipping box owns only its originating
            // fragmentainer. Descendant overflow must not manufacture later
            // paged-media fragments: those pages are outside the scrollport
            // and are discarded before the container's clip is applied.
            // <https://www.w3.org/TR/css-overflow-3/#scrollable-overflow>
            if deferred_overflow_clip.is_some()
                && !has_auto_height(style)
                && !fragmented_definite_block
                && fragmented_block_slices.is_empty()
                && fragmented_spanner_slices.is_empty()
                && page_index != paint_page_index
            {
                continue;
            }
            if suppress_leading_empty_block_fragment
                && first_descendant_paint_page.is_some_and(|first_page| page_index < first_page)
            {
                continue;
            }
            // This captured fragment contains descendant block decorations in
            // the BackgroundBorder band. Move them into the in-flow phase
            // before applying the container's overflow scope; otherwise the
            // scope deliberately preserves BackgroundBorder for the container
            // itself and descendant backgrounds can escape the clip.
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
            fragment.promote_background_border_to_in_flow_block();
            let mut block_align_content_offset_x = 0.0;
            if page_index == paint_page_index {
                block_align_content_offset_x = vertical_block_align_content_x_offset(
                    style,
                    content_inline.span(),
                    fragment.bounds(),
                );
                if block_align_content_offset_x.abs() > 0.01 && !translated_vertical_bookmarks {
                    self.translate_aligned_block_descendant_bookmarks(
                        descendant_bookmark_start,
                        paint_page_index,
                        block_align_content_offset_x,
                        0.0,
                    );
                    translated_vertical_bookmarks = true;
                }
            }
            if page_index == paint_page_index
                && (block_align_content_offset_x.abs() > 0.01
                    || block_align_content_offset_y.abs() > 0.01)
            {
                fragment = fragment.translated(PaintTranslation::new(
                    block_align_content_offset_x,
                    block_align_content_offset_y,
                ));
            }
            if page_index == paint_page_index
                && (static_scroll_translation.x != 0.0 || static_scroll_translation.y != 0.0)
            {
                fragment = fragment.translated(static_scroll_translation);
            }
            let fragment_overflow_clip = deferred_overflow_clip.map(|overflow_clip| {
                if has_auto_height(style) && page_index != paint_page_index {
                    // An auto-height overflow BFC fragments with its
                    // contents. Its destination fragment owns a fresh
                    // overflow clip at that fragment's painted inline origin,
                    // rather than retaining the source page's clip edge.
                    // <https://www.w3.org/TR/css-break-3/#box-splitting>
                    // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
                    fragment.bounds().map_or(overflow_clip, |bounds| {
                        let destination_height =
                            overflow_clip.height() + (overflow_clip.y() - bounds.y()).max(0.0);
                        let canvas_block_start = self
                            .document_canvas_fragment_insets
                            .iter()
                            .map(|inset| inset.top)
                            .sum::<f32>();
                        PaintClip::new(
                            bounds.x(),
                            bounds.y(),
                            overflow_clip.width(),
                            destination_height + canvas_block_start,
                        )
                    })
                } else {
                    overflow_clip
                }
            });
            if let Some(overflow_clip) = fragment_overflow_clip {
                fragment = if let Some(BorderShapeOverflowClip::Path(shape_clip)) =
                    deferred_border_shape_overflow_clip.clone()
                {
                    fragment.with_contents_effect_scoped_to_path(overflow_clip, shape_clip)
                } else if matches!(
                    deferred_border_shape_overflow_clip,
                    Some(BorderShapeOverflowClip::Empty)
                ) {
                    fragment.with_contents_effect_scoped_to_rect(PaintClip::new(
                        overflow_clip.x(),
                        overflow_clip.y(),
                        0.0,
                        0.0,
                    ))
                } else if let Some(rounded_clip) = deferred_rounded_overflow_clip {
                    fragment
                        .with_contents_effect_scoped_to_rounded_rect(overflow_clip, rounded_clip)
                } else if paint_containment_applies {
                    fragment
                        .with_primitives_clipped_to_rect_preserving_structure(overflow_clip)
                        .with_paint_containment_contents_effect_scoped_to_rect(overflow_clip)
                } else {
                    fragment
                        .with_primitives_clipped_to_rect_preserving_structure(overflow_clip)
                        .with_contents_effect_scoped_to_rect_preserving_contained_ink(
                            &self.current_page,
                            overflow_clip,
                        )
                };
            }
            if let Some(slice) = fragmented_block_slices
                .iter()
                .find(|slice| slice.page_index == page_index)
            {
                let mut fragment_style = style.clone();
                if propagates_document_canvas_properties {
                    suppress_document_canvas_background(&mut fragment_style);
                }
                suppress_fragmented_box_edges(
                    &mut fragment_style,
                    slice.owns_block_start,
                    slice.owns_block_end,
                );
                if style.visibility == Visibility::Visible {
                    let slice_height = (slice.top - slice.bottom).max(0.0);
                    let decoration_height =
                        if style.box_decoration_break == css::BoxDecorationBreak::Clone {
                            let capacity = if slice.page_index == paint_page_index {
                                block_start_page_context.area_height()
                            } else {
                                self.fragmentainer_override
                                    .map(|override_| override_.context.area_height())
                                    .unwrap_or_else(|| self.current_page_context.area_height())
                            };
                            slice_height.min(capacity) + vertical_extras
                        } else {
                            slice_height
                        };
                    let decoration_bottom = slice.top - decoration_height;
                    let decoration_bounds = PaintClip::new(
                        outer_x,
                        decoration_bottom,
                        outer_inline.width().points(),
                        decoration_height,
                    );
                    let backgrounds = self.box_background_primitives(
                        paint_space_rect(
                            outer_x,
                            decoration_bottom,
                            outer_inline.width().points(),
                            decoration_height,
                        ),
                        &fragment_style,
                    );
                    let outlines = self.box_outline_primitives(
                        paint_space_rect(
                            outer_x,
                            decoration_bottom,
                            outer_inline.width().points(),
                            decoration_height,
                        ),
                        &fragment_style,
                    );
                    if style.box_decoration_break == css::BoxDecorationBreak::Clone {
                        fragment.prepend_monolithic_primitives_in_band(
                            PaintBand::BackgroundBorder,
                            decoration_bounds,
                            backgrounds,
                        );
                        fragment.append_monolithic_primitives_in_band(
                            PaintBand::Outline,
                            decoration_bounds,
                            outlines,
                        );
                    } else {
                        fragment
                            .prepend_primitives_in_band(PaintBand::BackgroundBorder, backgrounds);
                        fragment.append_primitives_in_band(PaintBand::Outline, outlines);
                    }
                }
                decorated_block_pages.push(page_index);
            } else if page_index == background_page_index {
                fragment.prepend_primitives_in_band(
                    PaintBand::BackgroundBorder,
                    own_background_primitives.clone(),
                );
                fragment
                    .append_primitives_in_band(PaintBand::Outline, own_outline_primitives.clone());
            }
            if !defer_own_decoration_promotion {
                fragment.promote_background_border_to_in_flow_block();
            }
            if retain_size_contained_monolithic_paint {
                fragment = fragment.with_monolithic_fragmentation_scope(
                    PaintClip::from_paint_rect(border_paint_rect),
                );
            }
            if fragment.is_empty() {
                continue;
            }
            if page_index < self.pages.len() {
                self.pages[page_index]
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            } else {
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            }
        }
        // A fragment can contain only the principal box's decoration (for
        // example when a forced break occurs beside an empty child). Preserve
        // that fragment even when there was no descendant paint tree to which
        // the decoration could be prepended above.
        for slice in fragmented_block_slices
            .iter()
            .filter(|slice| !decorated_block_pages.contains(&slice.page_index))
            .filter(|slice| {
                !suppress_leading_empty_block_fragment
                    || first_descendant_paint_page
                        .is_none_or(|first_page| slice.page_index >= first_page)
            })
        {
            if style.visibility != Visibility::Visible {
                continue;
            }
            let mut fragment_style = style.clone();
            if propagates_document_canvas_properties {
                suppress_document_canvas_background(&mut fragment_style);
            }
            suppress_fragmented_box_edges(
                &mut fragment_style,
                slice.owns_block_start,
                slice.owns_block_end,
            );
            let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
            let slice_height = (slice.top - slice.bottom).max(0.0);
            let decoration_height = if style.box_decoration_break == css::BoxDecorationBreak::Clone
            {
                let capacity = if slice.page_index == paint_page_index {
                    block_start_page_context.area_height()
                } else {
                    self.fragmentainer_override
                        .map(|override_| override_.context.area_height())
                        .unwrap_or_else(|| self.current_page_context.area_height())
                };
                slice_height.min(capacity) + vertical_extras
            } else {
                slice_height
            };
            let decoration_bottom = slice.top - decoration_height;
            let decoration_bounds = PaintClip::new(
                outer_x,
                decoration_bottom,
                outer_inline.width().points(),
                decoration_height,
            );
            let backgrounds = self.box_background_primitives(
                paint_space_rect(
                    outer_x,
                    decoration_bottom,
                    outer_inline.width().points(),
                    decoration_height,
                ),
                &fragment_style,
            );
            let outlines = self.box_outline_primitives(
                paint_space_rect(
                    outer_x,
                    decoration_bottom,
                    outer_inline.width().points(),
                    decoration_height,
                ),
                &fragment_style,
            );
            if style.box_decoration_break == css::BoxDecorationBreak::Clone {
                fragment.prepend_monolithic_primitives_in_band(
                    PaintBand::BackgroundBorder,
                    decoration_bounds,
                    backgrounds,
                );
                fragment.append_monolithic_primitives_in_band(
                    PaintBand::Outline,
                    decoration_bounds,
                    outlines,
                );
            } else {
                fragment.prepend_primitives_in_band(PaintBand::BackgroundBorder, backgrounds);
                fragment.append_primitives_in_band(PaintBand::Outline, outlines);
            }
            if !defer_own_decoration_promotion {
                fragment.promote_background_border_to_in_flow_block();
            }
            if fragment.is_empty() {
                continue;
            }
            if slice.page_index < self.pages.len() {
                self.pages[slice.page_index]
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            } else if slice.page_index == self.pages.len() {
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            } else {
                self.pending_paint_fragments.push(PendingPaintFragment {
                    page_index: slice.page_index,
                    fragment,
                });
            }
        }
        if style.visibility == Visibility::Visible {
            for slice in fragmented_spanner_slices {
                let mut fragment_style = style.clone();
                if propagates_document_canvas_properties {
                    suppress_document_canvas_background(&mut fragment_style);
                }
                suppress_fragmented_box_edges(
                    &mut fragment_style,
                    slice.owns_block_start,
                    slice.owns_block_end,
                );
                let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
                let slice_height = (slice.top - slice.bottom).max(0.0);
                let decoration_height =
                    if style.box_decoration_break == css::BoxDecorationBreak::Clone {
                        let capacity = if slice.page_index == paint_page_index {
                            block_start_page_context.area_height()
                        } else {
                            self.fragmentainer_override
                                .map(|override_| override_.context.area_height())
                                .unwrap_or_else(|| self.current_page_context.area_height())
                        };
                        slice_height.min(capacity) + vertical_extras
                    } else {
                        slice_height
                    };
                let decoration_bottom = slice.top - decoration_height;
                let decoration_bounds = PaintClip::new(
                    outer_x,
                    decoration_bottom,
                    outer_inline.width().points(),
                    decoration_height,
                );
                let backgrounds = self.box_background_primitives(
                    paint_space_rect(
                        outer_x,
                        decoration_bottom,
                        outer_inline.width().points(),
                        decoration_height,
                    ),
                    &fragment_style,
                );
                let outlines = self.box_outline_primitives(
                    paint_space_rect(
                        outer_x,
                        decoration_bottom,
                        outer_inline.width().points(),
                        decoration_height,
                    ),
                    &fragment_style,
                );
                if style.box_decoration_break == css::BoxDecorationBreak::Clone {
                    fragment.prepend_monolithic_primitives_in_band(
                        PaintBand::BackgroundBorder,
                        decoration_bounds,
                        backgrounds,
                    );
                    fragment.append_monolithic_primitives_in_band(
                        PaintBand::Outline,
                        decoration_bounds,
                        outlines,
                    );
                } else {
                    fragment.prepend_primitives_in_band(PaintBand::BackgroundBorder, backgrounds);
                    fragment.append_primitives_in_band(PaintBand::Outline, outlines);
                }
                if !defer_own_decoration_promotion {
                    fragment.promote_background_border_to_in_flow_block();
                }
                if slice.page_index < self.pages.len() {
                    self.pages[slice.page_index]
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                } else {
                    self.current_page
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                }
            }
        }
        self.cursor_y -= block_end_margin_to_consume;
        self.last_block_layout_outcome = BlockLayoutOutcome {
            consumed_bottom_margin: layout_pt(block_end_margin_to_consume),
            physical_border_box_inline_span: outer_inline.width(),
            clamp_line_slots,
        };
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y();
        }
        self.apply_forced_break_after_box_in(fragmentainer_kind, style);
    }

    /// Position the content end of a definite promoted spanner through the
    /// current outer fragmentainer sequence.
    ///
    /// Unlike an ordinary class-A sibling, a promoted spanner may itself be
    /// fragmented by an enclosing page or multicol context. Consuming the
    /// definite size continuously avoids leaving unused remainder space before
    /// moving to the next outer fragmentainer.
    /// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
    /// <https://www.w3.org/TR/css-break-3/#breaking-rules>
    /// Consume a definite physical block size continuously through page or
    /// outer-column fragmentainers.
    ///
    /// Oversized monolithic boxes remain one layout object, but CSS
    /// Fragmentation still places their graphical slices in every crossed
    /// fragmentainer and resumes following flow at the continuous block-end.
    /// This primitive is shared by definite blocks, promoted spanners, and
    /// oversized atomic line boxes.
    /// <https://www.w3.org/TR/css-break-3/#monolithic>
    pub(in crate::layout) fn consume_definite_block_size_through_fragmentainers(
        &mut self,
        content_top: f32,
        height: f32,
    ) {
        self.cursor_y = content_top;
        let mut remaining = height.max(0.0);
        if self.active_fragmentainer_kind() == FragmentainerKind::Column {
            let available = (self.cursor_y - self.page_bottom()).max(0.0);
            if remaining <= available + 0.01 {
                self.cursor_y -= remaining;
                return;
            }
            if available > 0.01 {
                self.cursor_y -= available;
                remaining -= available;
                self.mark_current_page_flow_content();
            }
            let continuation_block_size = self
                .fragmentainer_override
                .map(|override_| override_.context.area_height())
                .unwrap_or_else(|| self.current_page_context.area_height());
            let plan = column_continuation_materialization(
                layout_pt(remaining),
                layout_pt(continuation_block_size),
                self.pages.len() + 1,
            );
            for page_index in 0..plan.pages_to_push {
                self.push_page();
                self.cursor_y = self.page_top();
                let is_last_conceptual_fragment =
                    !plan.has_unmaterialized_tail && page_index + 1 == plan.pages_to_push;
                if is_last_conceptual_fragment {
                    self.cursor_y -= plan.last_fragment_used_block_size.points();
                } else {
                    self.cursor_y = self.page_bottom();
                    self.mark_current_page_flow_content();
                }
            }
            if plan.has_unmaterialized_tail {
                // The retained current page stands in for the last conceptual
                // off-canvas column so following invisible flow preserves its
                // block offset without allocating the skipped prefix.
                self.cursor_y = self.page_top() - plan.last_fragment_used_block_size.points();
            }
            return;
        }
        let available = (self.cursor_y - self.page_bottom()).max(0.0);
        if remaining <= available + 0.01 {
            self.cursor_y -= remaining;
            return;
        }
        if available > 0.01 {
            self.cursor_y -= available;
            remaining -= available;
            // The box occupies this fragment even when its background and
            // descendants are appended after used-size resolution. Mark that
            // occupancy so the empty-page guard does not collapse a real
            // definite block fragment into its continuation.
            self.mark_current_page_flow_content();
        }
        let continuation_block_size = self.current_page_context.area_height();
        let plan = continuation_materialization(
            layout_pt(remaining),
            layout_pt(continuation_block_size),
            self.pages.len() + 1,
            MAX_MATERIALIZED_PAGE_FRAGMENTAINERS,
        );
        for page_index in 0..plan.pages_to_push {
            self.push_page();
            self.cursor_y = self.page_top();
            let is_last_conceptual_fragment =
                !plan.has_unmaterialized_tail && page_index + 1 == plan.pages_to_push;
            if is_last_conceptual_fragment {
                self.cursor_y -= plan.last_fragment_used_block_size.points();
            } else {
                self.cursor_y = self.page_bottom();
                self.mark_current_page_flow_content();
            }
        }
        if plan.has_unmaterialized_tail {
            self.cursor_y = self.page_top() - plan.last_fragment_used_block_size.points();
        }
    }

    fn definite_block_descendants_overflow(
        &mut self,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &[Stylesheet],
        available_width: f32,
        definite_content_height: f32,
    ) -> bool {
        let Some(child_boxes) = child_boxes else {
            return false;
        };
        let estimated = child_boxes
            .iter()
            .filter_map(|child| match child {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    formatting_box_has_inline_content(&box_.children)
                        .then_some(box_.style.line_height)
                }
                _ => child
                    .element_parts()
                    .and_then(|(element, _, style, children)| {
                        style_is_in_normal_flow(style).then(|| {
                            self.estimate_element_height(
                                element,
                                style,
                                stylesheets,
                                available_width,
                                Some(children),
                            )
                        })
                    })
                    .flatten(),
            })
            .sum::<f32>();
        estimated > definite_content_height + 0.01
    }

    /// Lay out overflowing descendants independently from a definite
    /// principal box's normal-flow end position.
    ///
    /// CSS overflow does not enlarge a definite-height box. Descendant paint
    /// can therefore reach later outer fragmentainers while the following
    /// sibling starts at the principal box's authored block-end. This applies
    /// both to a promoted spanner and to an ordinary definite box that fits in
    /// its current column. Quire already uses this speculative/deferred-paint
    /// model for fragmented floats; the same mechanism preserves counters,
    /// bookmarks, links, named strings, and running elements while restoring
    /// normal-flow geometry.
    /// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    #[allow(clippy::too_many_arguments)]
    fn layout_definite_block_with_deferred_descendant_overflow(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
        definite_content_height: f32,
        vertical_non_content: NonContentLength,
    ) {
        let fragmentainer_kind = self.active_fragmentainer_kind();
        let snapshot = self.snapshot();
        let paint_page_index = self.pages.len();
        let paint_checkpoint = self.current_page.paint_checkpoint();

        self.multicol_spanner_speculation_depth += 1;
        self.fragmentation_suppression_depth += 1;
        self.layout_block_with_descendant_percentage_height_basis(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            descendant_percentage_height_basis,
        );
        self.fragmentation_suppression_depth -= 1;
        self.multicol_spanner_speculation_depth -= 1;

        let captured_fragments =
            self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        let fragments = captured_fragments
            .into_iter()
            .flat_map(|(_, fragment)| continuous_fragmentainer_paint_slices(&snapshot, fragment))
            .collect::<Vec<_>>();
        let side_effects = self.deferred_layout_side_effects_since(&snapshot);
        let counter_set = self.counter_set.clone();
        let quote_depth = self.quote_depth;
        let next_assignment_id = self.next_assignment_id;
        let next_paint_source_order = self.next_paint_source_order;
        self.restore(snapshot);
        self.counter_set = counter_set;
        self.quote_depth = quote_depth;
        self.next_assignment_id = next_assignment_id;
        self.next_paint_source_order = next_paint_source_order;
        self.apply_deferred_layout_side_effects(side_effects);

        for (page_index, fragment) in fragments {
            if page_index == self.pages.len() {
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                self.mark_current_page_flow_content();
            } else {
                self.pending_paint_fragments.push(PendingPaintFragment {
                    page_index,
                    fragment,
                });
            }
        }

        self.apply_forced_break_before_box_in(fragmentainer_kind, style);
        let starts_at_page_top = self.cursor_is_at_page_top() && self.truncate_page_start_margins;
        self.cursor_y -=
            page_start_margin(layout_pt(style.margin.top), starts_at_page_top).points();
        let border_box_height = definite_content_height + vertical_non_content.points();
        let block_top = self.cursor_y;
        self.consume_definite_block_size_through_fragmentainers(block_top, border_box_height);
        self.cursor_y -= style.margin.bottom;
        self.last_block_layout_outcome = BlockLayoutOutcome {
            consumed_bottom_margin: layout_pt(style.margin.bottom),
            physical_border_box_inline_span: border_box_pt(0.0),
            clamp_line_slots: 0,
        };
        self.apply_forced_break_after_box_in(fragmentainer_kind, style);
    }
}

/// Whether a descendant contributes a forced boundary to this fragmented
/// flow.
///
/// A definite principal box can defer ordinary visible overflow without
/// moving its normal-flow end, but a forced descendant break establishes a
/// real boundary in the descendant's parallel fragmented flow and must remain
/// materialized by the regular fragmentation algorithm.
/// <https://www.w3.org/TR/css-break-3/#forced-breaks>
fn formatting_boxes_have_forced_break_in(
    boxes: Option<&[box_tree::FormattingBox<'_>]>,
    fragmentainer_kind: FragmentainerKind,
) -> bool {
    boxes.is_some_and(|boxes| {
        boxes.iter().any(|box_| match box_ {
            box_tree::FormattingBox::AnonymousBlock(anonymous) => {
                fragmentainer_kind.is_forced_break(anonymous.style.break_before)
                    || fragmentainer_kind.is_forced_break(anonymous.style.break_after)
                    || formatting_boxes_have_forced_break_in(
                        Some(&anonymous.children),
                        fragmentainer_kind,
                    )
            }
            box_tree::FormattingBox::InlineSplitBlockContext(context) => {
                formatting_boxes_have_forced_break_in(
                    Some(&context.core.children),
                    fragmentainer_kind,
                )
            }
            _ => box_.element_parts().is_some_and(|(_, _, style, children)| {
                style_is_in_normal_flow(style)
                    && (fragmentainer_kind.is_forced_break(style.break_before)
                        || fragmentainer_kind.is_forced_break(style.break_after)
                        || formatting_boxes_have_forced_break_in(
                            Some(children),
                            fragmentainer_kind,
                        ))
            }),
        })
    })
}

#[derive(Debug, Clone, Copy)]
struct PromotedSpannerPaintSlice {
    page_index: usize,
    top: f32,
    bottom: f32,
    owns_block_start: bool,
    owns_block_end: bool,
}

/// Slice continuous paint from one source coordinate space into its outer
/// fragmentainers.
///
/// Overflow is laid out in one unbounded source coordinate space, then clipped
/// and translated into the current remainder and full continuation
/// fragmentainers. This is the paint counterpart of the independently tracked
/// normal-flow block size. Definite spanners and oversized atomic line boxes
/// use this same projection so paint and flow consume identical fragmentainer
/// distances.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
pub(in crate::layout) fn continuous_fragmentainer_paint_slices(
    snapshot: &LayoutSnapshot,
    fragment: PaintFragment,
) -> Vec<(usize, PaintFragment)> {
    let Some(bounds) = fragment.bounds() else {
        return Vec::new();
    };
    let first_context = snapshot.current_page_context;
    let continuation_context = snapshot
        .fragmentainer_override
        .map(|override_| override_.context)
        .unwrap_or(first_context);
    let source_bottom = bounds.y();
    let mut source_top = snapshot.cursor_y.max(source_bottom);
    let mut page_index = snapshot.pages.len();
    let mut first_slice = true;
    let mut slices = Vec::new();
    while source_top > source_bottom + 0.01 {
        let context = if first_slice {
            first_context
        } else {
            continuation_context
        };
        let target_top = if first_slice {
            snapshot.cursor_y
        } else {
            context.top()
        };
        let capacity = if first_slice {
            (snapshot.cursor_y - context.bottom()).max(0.0)
        } else {
            context.area_height().max(0.0)
        };
        if capacity <= 0.01 {
            page_index += 1;
            first_slice = false;
            continue;
        }
        let slice_height = (source_top - source_bottom).min(capacity);
        let source_clip = PageTopRect::new(
            context.left(),
            source_top,
            context.area_width(),
            slice_height,
        )
        .paint_clip();
        let slice = fragment.clone().clipped_to_rect(source_clip);
        if !slice.is_empty() {
            slices.push((
                page_index,
                slice.translated(PaintTranslation::new(0.0, target_top - source_top)),
            ));
        }
        source_top -= slice_height;
        page_index += 1;
        first_slice = false;
    }
    slices
}

fn promoted_spanner_paint_slices(
    first_page_index: usize,
    last_page_index: usize,
    block_top: f32,
    block_bottom: f32,
    first_context: PageContext,
    last_context: PageContext,
    fragmentainer_override: Option<FragmentainerOverride>,
) -> Vec<PromotedSpannerPaintSlice> {
    let continuation_context = fragmentainer_override
        .map(|override_| override_.context)
        .unwrap_or(last_context);
    (first_page_index..=last_page_index)
        .filter_map(|page_index| {
            let context = if page_index == first_page_index {
                first_context
            } else if page_index == last_page_index {
                last_context
            } else {
                continuation_context
            };
            let top = if page_index == first_page_index {
                block_top
            } else {
                context.top()
            };
            let bottom = if page_index == last_page_index {
                block_bottom
            } else {
                context.bottom()
            };
            (top > bottom + 0.01).then_some(PromotedSpannerPaintSlice {
                page_index,
                top,
                bottom,
                owns_block_start: page_index == first_page_index,
                owns_block_end: page_index == last_page_index,
            })
        })
        .collect()
}

pub(in crate::layout) fn suppress_fragmented_box_edges(
    style: &mut ComputedStyle,
    owns_block_start: bool,
    owns_block_end: bool,
) {
    if style.box_decoration_break == css::BoxDecorationBreak::Clone {
        return;
    }
    if !owns_block_start {
        suppress_promoted_spanner_physical_edge(style, block_start_side(style.writing_mode));
    }
    if !owns_block_end {
        suppress_promoted_spanner_physical_edge(style, block_end_side(style.writing_mode));
    }
}

/// Remove the box background that CSS Backgrounds propagates to the document
/// canvas while retaining borders and other fragment-local decoration.
///
/// A propagated root/body background is painted once in the canvas coordinate
/// system; cloning it into every page fragment would re-anchor image layers:
/// <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>.
fn suppress_document_canvas_background(style: &mut ComputedStyle) {
    style.background_color = None;
    style.background_image = css::ComputedImage::None;
    style.background_layers.clear();
}

fn suppress_promoted_spanner_physical_edge(style: &mut ComputedStyle, side: PhysicalSide) {
    let zero = css::ComputedLengthPercentage::ZERO;
    match side {
        PhysicalSide::Top => {
            style.padding.top = 0.0;
            style.border_widths.top = 0.0;
            style.box_values.padding.top = zero.clone();
            style.border_width_values.top = zero;
            style.border_styles.top = css::BorderStyle::None;
        }
        PhysicalSide::Right => {
            style.padding.right = 0.0;
            style.border_widths.right = 0.0;
            style.box_values.padding.right = zero.clone();
            style.border_width_values.right = zero;
            style.border_styles.right = css::BorderStyle::None;
        }
        PhysicalSide::Bottom => {
            style.padding.bottom = 0.0;
            style.border_widths.bottom = 0.0;
            style.box_values.padding.bottom = zero.clone();
            style.border_width_values.bottom = zero;
            style.border_styles.bottom = css::BorderStyle::None;
        }
        PhysicalSide::Left => {
            style.padding.left = 0.0;
            style.border_widths.left = 0.0;
            style.box_values.padding.left = zero.clone();
            style.border_width_values.left = zero;
            style.border_styles.left = css::BorderStyle::None;
        }
    }
}
