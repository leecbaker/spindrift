use super::*;
use crate::layout::block::BlockLayoutInlineConstraint;

impl<'a> LayoutBuilder<'a> {
    /// Resolve a flex container's used content width, including intrinsic keywords.
    ///
    /// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
    /// width values that resolve from the box's intrinsic contributions. CSS
    /// Flexbox defines those contributions for flex containers separately from
    /// normal block flow, so flex layout must not fall back to CSS 2.2
    /// auto-width filling when the author supplied an intrinsic keyword:
    /// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
    pub(in crate::layout::flex) fn used_flex_container_content_width(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        horizontal_extras: f32,
        vertical_extras: f32,
    ) -> f32 {
        let orthogonal_auto_width = orthogonal_auto_width_flex_container_needs_intrinsic(
            style,
            self.current_child_available_space(),
        );
        let needs_intrinsic = matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        ) || (style.float != Float::None && style.box_values.width.is_auto())
            || orthogonal_auto_width;

        if !needs_intrinsic {
            return used_content_box_width(
                style,
                layout_pt(available_outer_width),
                non_content_pt(horizontal_extras),
            )
            .points();
        }

        let height_percentage_basis = self.flex_container_height_percentage_basis();
        let intrinsic_content_height = used_content_box_height_or_auto_with_basis(
            style,
            height_percentage_basis,
            non_content_pt(vertical_extras),
        )
        .map(SemanticLengthExt::points);
        // A shrink-to-fit size-contained flex container has the intrinsic
        // inline sizes of empty content. Authored width/min/max constraints
        // are still resolved by `flex_container_content_width_from_intrinsic`.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        let intrinsic = if intrinsic_physical_width_is_contained(style) {
            FlexItemEstimate::fixed(0.0, 0.0)
        } else {
            self.estimate_intrinsic_flex_container_size(
                children,
                style,
                stylesheets,
                FlexAvailableSpace {
                    width: PhysicalContentWidth::new(content_box_pt(
                        available_outer_width.max(0.0),
                    )),
                    width_basis: flex_available_percentage_basis_from_points(
                        used_content_box_width_or_auto(
                            style,
                            layout_pt(available_outer_width.max(0.0)),
                            non_content_pt(horizontal_extras),
                        )
                        .map(|_| available_outer_width.max(0.0)),
                        FlexAvailableSizeSource::IntrinsicContainerSize,
                    ),
                    height: intrinsic_content_height
                        .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                    height_basis: flex_available_percentage_basis_from_points(
                        intrinsic_content_height,
                        FlexAvailableSizeSource::IntrinsicContainerSize,
                    ),
                },
            )
        };

        flex_container_content_width_from_intrinsic(
            style,
            available_outer_width,
            horizontal_extras,
            intrinsic,
            style.float != Float::None || orthogonal_auto_width,
        )
        .points()
    }

    pub(crate) fn layout_flex(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        self.layout_flex_with_descendant_percentage_height_basis(
            element,
            style,
            stylesheets,
            child_boxes,
            None,
        );
    }

    pub(in crate::layout) fn layout_flex_with_descendant_percentage_height_basis(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
    ) {
        if std::env::var_os("QUIRE_TRACE_FLEX").is_some() {
            eprintln!("flex entry tag={} cursor={}", element.tag, self.cursor_y);
        }
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
        let source_style = style;
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let mut used_style =
            FlexUsedStyle::from_normalized(self.style_with_current_viewport_lengths(style));
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(containing_inline_size)),
        );

        let relative_offset = self.normal_flow_relative_position_offset(&used_style);
        if matches!(used_style.position, Position::Relative | Position::Sticky) {
            self.cursor_y += relative_offset.y();
        }

        let normal_flow_available_outer_width = self.content_right
            - self.content_left
            - used_style.margin.left
            - used_style.margin.right;
        let border_widths = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content_length().points();
        let vertical_extras = box_metrics.vertical_non_content_length().points();

        let built_child_boxes;
        let child_boxes = if let Some(child_boxes) = child_boxes {
            child_boxes
        } else {
            built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                element,
                stylesheets,
                // Descendant cascade must inherit authored/computed values.
                // `used_style` has already materialized this container's
                // effective zoom, so using it as the cascade parent would
                // make a descendant inherit an enlarged font and apply the
                // same effective zoom a second time during its own layout.
                // <https://drafts.csswg.org/css-cascade-5/#computed>
                // <https://drafts.csswg.org/css-viewport/#zoom-property>
                style,
            );
            &built_child_boxes
        };
        let container_signature = self.flex_container_signature(element);
        let (mut children, mut positioned_children) =
            flex_child_lists_from_boxes(element, &container_signature, &used_style, child_boxes);
        self.resolve_styled_children_viewport_lengths(&mut children);
        self.resolve_styled_children_viewport_lengths(&mut positioned_children);

        // A block-level flex container establishes a block formatting context.
        // Consequently, an automatic-width flex container must use the
        // available float-avoidance band rather than first resolving to the
        // full containing-block width and being forced below the float.  The
        // probe mirrors normal block-flow BFC-root placement while retaining
        // the containing block's full inline size as the percentage basis.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        let has_active_float = self
            .float_contexts
            .last()
            .is_some_and(|context| !context.shapes.is_empty());
        let float_avoiding_placement = (has_active_float
            && used_style.float == Float::None
            && used_style.box_values.width.is_auto()
            && self.containing_block_writing_mode == WritingMode::HorizontalTb
            && used_style.writing_mode == WritingMode::HorizontalTb)
            .then(|| {
                let context = self
                    .float_contexts
                    .last()
                    .expect("root float context exists")
                    .clone();
                let page_index = self.current_float_page_index();
                let placement_top = self.cursor_y - used_style.margin.top;
                let clear = used_style.clear;
                let writing_mode = used_style.writing_mode;
                let direction = used_style.direction;
                context.avoiding_bfc_root_position(
                    page_index,
                    placement_top,
                    clear,
                    writing_mode,
                    direction,
                    self.content_left,
                    self.content_right,
                    |band_left, band_width, _candidate_top| {
                        let candidate_geometry = self.block_layout_geometry_in_inline_span(
                            element,
                            &used_style,
                            stylesheets,
                            Some(child_boxes),
                            BlockLayoutInlineConstraint {
                                containing_left: band_left,
                                containing_right: band_left + band_width,
                                percentage_basis: PercentageBasis::definite(
                                    LogicalInlineContentSize::new(content_box_pt(
                                        containing_inline_size,
                                    )),
                                ),
                                physical_width_percentage_basis: PhysicalContentWidth::new(
                                    content_box_pt(containing_inline_size),
                                ),
                                auto_border_box_width: (band_width < containing_inline_size - 0.01)
                                    .then_some(border_box_pt(band_width)),
                            },
                        );
                        let candidate_style = &candidate_geometry.style;
                        let estimated_outer_height = self
                            .estimate_element_height(
                                element,
                                candidate_style,
                                stylesheets,
                                candidate_geometry.outer_width(),
                                Some(child_boxes),
                            )
                            .unwrap_or(
                                candidate_style.margin.top
                                    + candidate_style.line_height
                                    + candidate_style.margin.bottom,
                            );
                        let border_box_height = (estimated_outer_height
                            - candidate_style.margin.top
                            - candidate_style.margin.bottom)
                            .max(0.0);
                        FloatAvoidingBfcMeasurement {
                            border_box_left: candidate_geometry.outer_inline().start()
                                - candidate_geometry.relative_offset.x(),
                            border_box_width: candidate_geometry.outer_width(),
                            border_box_height,
                        }
                    },
                )
            });
        let available_outer_width = float_avoiding_placement
            .map(|placement| {
                (placement.available_width - used_style.margin.left - used_style.margin.right)
                    .max(0.0)
            })
            .unwrap_or(normal_flow_available_outer_width);

        let requested_content_width = self.used_flex_container_content_width(
            &children,
            &used_style,
            stylesheets,
            available_outer_width,
            horizontal_extras,
            vertical_extras,
        );
        // A block-level flex container remains a normal-flow block while its
        // contents use the Flexbox algorithm.  Resolve its outer inline box
        // through the same typed CSS 2.2 width equation as ordinary blocks,
        // including fixed/automatic margins after CSS `zoom` has materialized
        // its fixed components.  The old local calculation duplicated this
        // boundary and could disagree with block layout on auto margins.
        // <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>
        // <https://drafts.csswg.org/css-flexbox-1/#flex-containers>
        // <https://drafts.csswg.org/css-viewport/#zoom-property>
        let resolved_normal_flow_width = (used_style.float == Float::None).then(|| {
            resolve_normal_flow_block_width(
                &mut used_style,
                self.content_left,
                self.content_right,
                PhysicalContentWidth::new(content_box_pt(requested_content_width)),
                non_content_pt(horizontal_extras),
                self.containing_block_direction,
                true,
            )
        });
        let content_width = resolved_normal_flow_width
            .map(|width| width.content_width.points())
            .unwrap_or_else(|| {
                constrain_content_width(
                    &used_style,
                    content_box_pt(requested_content_width),
                    PercentageBasis::definite(layout_pt(available_outer_width)),
                )
                .points()
            });
        let outer_width = resolved_normal_flow_width
            .map(|width| width.border_box_width.points())
            .unwrap_or_else(|| (content_width + horizontal_extras).max(0.0));
        let style = &used_style;
        let mut outer_x = float_avoiding_placement
            .map(|placement| placement.left + style.margin.left)
            .or_else(|| resolved_normal_flow_width.map(|width| width.border_box_x))
            .unwrap_or_else(|| {
                normal_flow_block_outer_x(
                    self.content_left,
                    self.content_right,
                    style,
                    outer_width,
                    self.containing_block_direction,
                )
            })
            + relative_offset.x();
        let mut inner_x = outer_x + border_widths.left + style.padding.left;
        let inner_width = content_width.max(0.0);
        let available_outer_height = Fragmentainer::from_cursor_bounds(
            self.page_area_height(),
            self.cursor_y,
            self.page_bottom(),
        )
        .available_block_size_after_reservation(style.margin.top + style.margin.bottom);
        let height_percentage_basis = self.flex_container_height_percentage_basis();
        let height_constraint_basis = height_percentage_basis
            .points()
            .unwrap_or(available_outer_height);
        let explicit_content_height = used_content_box_height_or_auto_with_basis(
            style,
            height_percentage_basis,
            non_content_pt(vertical_extras),
        )
        .map(|height| {
            constrain_content_height(
                style,
                height,
                PercentageBasis::definite(layout_pt(height_constraint_basis)),
            )
            .points()
        });
        let definite_content_height = definite_flex_container_content_height(
            style,
            explicit_content_height,
            content_width,
            PercentageBasis::definite(layout_pt(height_constraint_basis)),
            horizontal_extras,
            vertical_extras,
        );
        let size_contained_content_height = style.contain.size.then(|| {
            definite_content_height.unwrap_or_else(|| {
                constrain_content_height(
                    style,
                    content_box_pt(0.0),
                    PercentageBasis::definite(layout_pt(height_constraint_basis)),
                )
                .points()
            })
        });
        let flex_available_content_height = flex_available_content_height(
            style,
            definite_content_height,
            PercentageBasis::definite(layout_pt(content_width)),
        );

        self.cursor_y -= style.margin.top;
        if let Some(placement) = float_avoiding_placement {
            self.cursor_y = placement.top;
        } else if style.float == Float::None {
            let margin_box_width = style.margin.left + outer_width + style.margin.right;
            let collision_height = size_contained_content_height
                .or(definite_content_height)
                .unwrap_or(style.line_height)
                + vertical_extras
                + style.margin.top
                + style.margin.bottom;
            let (margin_box_left, avoided_top, _) = self.place_float_avoiding_margin_box(
                self.cursor_y,
                PageTopSize::new(margin_box_width, collision_height),
                style.clear,
                style.writing_mode,
                style.direction,
                self.containing_block_direction,
            );
            self.cursor_y = avoided_top;
            outer_x = margin_box_left + style.margin.left + relative_offset.x();
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
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        self.cursor_y -= border_widths.top + style.padding.top;

        let descendant_height_basis = descendant_percentage_height_basis
            .map(|basis| {
                flex_available_percentage_basis_from_points(
                    basis.points(),
                    FlexAvailableSizeSource::ContainingBlock,
                )
            })
            .unwrap_or_else(|| {
                flex_available_percentage_basis_from_points(
                    definite_content_height,
                    FlexAvailableSizeSource::ContainingBlock,
                )
            });
        let flex_available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(inner_width)),
            width_basis: flex_available_percentage_basis_from_points(
                Some(inner_width),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: flex_available_content_height
                .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
            height_basis: descendant_height_basis,
        };
        let Some(mut flex_layout) =
            self.compute_flex_layout(&children, style, stylesheets, flex_available)
        else {
            let mut flow_style = style.clone();
            flow_style.display = Display::BLOCK;
            suppress_replayed_item_margins(&mut flow_style);
            self.layout_block_with_descendant_percentage_height_basis(
                element,
                &flow_style,
                stylesheets,
                &[],
                Some(child_boxes),
                descendant_percentage_height_basis,
            );
            return;
        };
        // CSS Flexbox collects lines while an automatic main size is
        // indefinite. A `calc-size(auto, …)` preferred height then receives
        // that measured automatic size, making the final main size definite
        // for flexible-length resolution without recollecting the lines.
        // Taffy's combined pass cannot express that split, so re-run sizing
        // with `nowrap` only after retaining the initially selected line.
        // <https://drafts.csswg.org/css-flexbox-1/#algo-line-break> and
        // <https://drafts.csswg.org/css-values-5/#calc-size>.
        let late_calc_height = match &style.box_values.height {
            css::ComputedLengthPercentageOrAuto::CalcSize(value)
                if matches!(value.basis, css::CalcSizeBasis::Auto)
                    && flex_available.height.is_none() =>
            {
                calc_size_intrinsic_constraint(
                    value.clone(),
                    style.box_sizing,
                    PercentageBasis::definite(content_box_pt(content_width)),
                    non_content_pt(vertical_extras),
                    content_box_pt(flex_layout.height),
                    content_box_pt(flex_layout.height),
                )
                .map(SemanticLengthExt::points)
            }
            _ => None,
        };
        if let Some(final_height) = late_calc_height {
            let mut final_sizing_style = style.clone();
            final_sizing_style.flex_wrap = FlexWrap::NoWrap;
            if let Some(final_layout) = self.compute_flex_layout(
                &children,
                &final_sizing_style,
                stylesheets,
                FlexAvailableSpace {
                    height: Some(PhysicalContentHeight::new(content_box_pt(final_height))),
                    height_basis: flex_available_percentage_basis_from_points(
                        Some(final_height),
                        FlexAvailableSizeSource::ContainingBlock,
                    ),
                    ..flex_available
                },
            ) {
                flex_layout = final_layout;
            }
        }
        let flex_content_height = flex_layout.height;
        let mut total_content_height =
            if let Some(size_contained_height) = size_contained_content_height {
                size_contained_height
            } else {
                // Flex line breaking uses an indefinite automatic main size,
                // then the flex container's own `calc-size()` preferred block
                // size substitutes that measured automatic size. Resolving it
                // before `compute_flex_layout` would incorrectly make the
                // available main size definite and change the line count.
                // <https://drafts.csswg.org/css-flexbox-1/#algo-available>
                // and <https://drafts.csswg.org/css-values-5/#calc-size>.
                let requested_height = if let css::ComputedLengthPercentageOrAuto::CalcSize(value) =
                    &style.box_values.height
                {
                    calc_size_intrinsic_constraint(
                        value.clone(),
                        style.box_sizing,
                        PercentageBasis::definite(content_box_pt(content_width)),
                        non_content_pt(vertical_extras),
                        content_box_pt(flex_content_height),
                        content_box_pt(flex_content_height),
                    )
                    .map(SemanticLengthExt::points)
                    .unwrap_or(flex_content_height)
                } else {
                    flex_content_height
                };
                constrain_content_height(
                    style,
                    content_box_pt(requested_height),
                    PercentageBasis::definite(layout_pt(content_width)),
                )
                .points()
            };
        let mut break_units =
            flex_break_units(fragmentainer_kind, &flex_layout, &children, style, false);
        if break_units.is_empty() && total_content_height > 0.01 {
            // A flex container fragments even when it has no in-flow flex
            // items. Its own background, border, and padding box still need
            // one source range that can be sliced across fragmentainers.
            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
            break_units.push(FlexBreakUnit {
                item_indices: Vec::new(),
                line_start: 0,
                line_end: 0,
                block_start: 0.0,
                block_end: total_content_height,
                break_before: PageBreak::Auto,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            });
        }
        debug_assert!(!flex_layout.lines.is_empty() || children.is_empty());
        debug_assert!(
            flex_layout.fragment_plan.is_empty()
                || flex_layout.fragment_plan.planned_item_fragment_count()
                    <= flex_layout.items.len()
        );
        let mut total_height = border_widths.top
            + style.padding.top
            + total_content_height
            + style.padding.bottom
            + border_widths.bottom;
        let flex_has_forced_item_breaks = children.iter().any(|child| {
            let break_context = FragmentBreakContext::for_standalone_box(&child.style);
            break_context
                .forced_break_before_in(fragmentainer_kind)
                .is_some()
                || break_context
                    .forced_break_after_in(fragmentainer_kind)
                    .is_some()
        });
        if !flex_has_forced_item_breaks
            && should_move_flex_container_to_next_page(
                block_top,
                total_height,
                self.page_top(),
                self.page_bottom(),
                self.page_area_height(),
            )
        {
            self.push_page();
            self.layout_flex(element, source_style, stylesheets, Some(child_boxes));
            return;
        }
        let defer_own_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = false;
        let content_top = self.cursor_y;
        let flex_overflows_current_page = block_top - total_height < self.page_bottom() - 0.01;
        // An automatic single-line row has no independently definite cross
        // size. At the end of a multicolumn fragmentainer, its stretched line
        // must therefore be finalized through the fragment plan as well: the
        // next column can enlarge the line's fragment-local cross size. An
        // explicitly sized flex box, wrapped line, or page fragment retains
        // the ordinary exact-fit behavior.
        // <https://drafts.csswg.org/css-flexbox-1/#pagination>
        let auto_row_reaches_column_boundary = fragmentainer_kind == FragmentainerKind::Column
            && physical_flex_direction(style).is_row_axis()
            && style.flex_wrap == FlexWrap::NoWrap
            && style.box_values.height.is_auto()
            && !children.iter().any(|child| {
                fragmentainer_kind.avoids_break_inside(&child.style)
                    || child.element_parts().is_some_and(|(_, _, boxes)| {
                        boxes.is_some_and(|boxes| {
                            flex_item_contents_have_forced_break_in(boxes, fragmentainer_kind)
                        })
                    })
            })
            && total_height >= (block_top - self.page_bottom()) - 0.01;
        let flex_fragmentation_enabled = !self.preserve_scoped_paint_public_order
            && !is_document_canvas_element(element)
            && (flex_overflows_current_page
                || auto_row_reaches_column_boundary
                || flex_has_forced_item_breaks);
        if flex_fragmentation_enabled {
            break_units =
                flex_break_units(fragmentainer_kind, &flex_layout, &children, style, true);
        }
        // A single-line row flex container lays out every continuation in the
        // remaining fragmentainer space and then reruns cross-axis alignment.
        // In particular, a stretched auto-height item fills its final
        // continuation even when only a small tail of source content remains.
        // Model that occupied fragment-space before constructing break units,
        // so later siblings, containing blocks, and decoration all observe
        // the same geometry.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        if flex_fragmentation_enabled
            && physical_flex_direction(style).is_row_axis()
            && style.flex_wrap == FlexWrap::NoWrap
            && children
                .iter()
                .any(|child| row_flex_item_stretches_in_fragment(&child.style, style))
            && let Some(fragmented_cross_size) = {
                let first_fragment_capacity = Fragmentainer::from_cursor_bounds(
                    self.page_area_height(),
                    content_top,
                    self.page_bottom(),
                )
                .available_block_size();
                // At this exact automatic-column boundary, reserve the
                // zero-width continuation before deriving the number of
                // occupied line fragments. The helper intentionally keeps
                // ordinary exact fits unsliced, so only this pagination case
                // supplies the infinitesimal source tail.
                let source_cross_size = if auto_row_reaches_column_boundary
                    && (total_content_height - first_fragment_capacity).abs() <= 0.01
                {
                    total_content_height + 0.02
                } else {
                    total_content_height
                };
                single_line_row_fragmented_cross_size(
                    source_cross_size,
                    first_fragment_capacity,
                    self.page_area_height(),
                )
            }
        {
            total_content_height = fragmented_cross_size;
            flex_layout.height = fragmented_cross_size;
            for (item, child) in flex_layout.items.iter_mut().zip(&children) {
                if row_flex_item_stretches_in_fragment(&child.style, style) {
                    item.set_fragmentation_height(fragmented_cross_size);
                }
            }
            for line in &mut flex_layout.lines {
                line.cross_end =
                    FlexCrossOffset::new(line.cross_start.points() + fragmented_cross_size);
            }
            break_units =
                flex_break_units(fragmentainer_kind, &flex_layout, &children, style, true);
            total_height = border_widths.top
                + style.padding.top
                + total_content_height
                + style.padding.bottom
                + border_widths.bottom;
        }
        if flex_fragmentation_enabled {
            self.fragment_top_offsets
                .push(self.current_page_context.top() - content_top);
        }
        let positioning_containing_block_mode = PositionedContainingBlockMode::for_style(style);
        let establishes_positioning_containing_block = positioning_containing_block_mode.is_some();
        let establishes_fixed_containing_block = matches!(
            positioning_containing_block_mode,
            Some(PositionedContainingBlockMode::FixedAndAbsolute)
        );
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
        // Store the offset from the flex content start, rather than a page
        // coordinate. Every continuation fragment has a different local page
        // top but represents the same source flex container.
        let positioning_containing_block_offset = block_top - border_widths.top - content_top;

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

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        if flex_fragmentation_enabled {
            self.content_left = inner_x;
            self.content_right = inner_x + inner_width;
        }
        self.push_float_context();
        flex_layout.fragment_plan.fragments.clear();
        let mut fragment_cursor = FlexFragmentCursor::new(content_top, 0.0);
        let mut forced_break_carry = ForcedBreakCarryState::new(fragmentainer_kind);
        let mut previous_break_after = PageBreak::Auto;
        let distributed_cross_axis_lines = matches!(
            style.align_content.keyword,
            ContentAlignmentKeyword::SpaceBetween
                | ContentAlignmentKeyword::SpaceAround
                | ContentAlignmentKeyword::SpaceEvenly
        );
        for (unit_index, unit) in break_units.iter().enumerate() {
            let break_context = forced_break_carry.take_box_context(
                unit.break_before,
                unit.break_after,
                PageBreak::Auto,
            );
            let break_is_applicable = flex_fragmentation_enabled;
            if break_is_applicable
                && let Some(break_before) = break_context.forced_break_before_in(fragmentainer_kind)
            {
                let transition =
                    FlexFragmentTransitionDecision::forced(fragmentainer_kind, unit.block_start);
                debug_assert_eq!(transition.reason, FlexFragmentBreakReason::Forced);
                if let Some(content_top) = self.materialize_fragmentainer_advance(
                    transition.fragmentainer_kind,
                    FragmentainerAdvance::Forced(break_before),
                ) {
                    fragment_cursor = transition.cursor_after_fragmentainer_advance(content_top);
                }
            }
            let current_fragmentainer = Fragmentainer::from_cursor_bounds(
                self.page_area_height(),
                fragment_cursor.content_top,
                self.page_bottom(),
            );
            let unit_is_oversized = unit.block_size() > self.page_area_height() + 0.01;
            let prebreak_decision =
                FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
                    fragmentainer_kind,
                    break_is_applicable,
                    unit_is_oversized,
                    has_prior_unit: unit_index > 0,
                    has_later_unit: unit_index + 1 < break_units.len(),
                    cursor: fragment_cursor,
                    unit_block_start: unit.block_start,
                    unit_block_end: unit.block_end,
                    current_fragmentainer,
                    break_opportunity: FragmentBreakOpportunity::before_box_boundary(
                        fragmentainer_kind,
                        unit.block_start,
                        break_context,
                        previous_break_after,
                        unit.break_inside_avoid || distributed_cross_axis_lines,
                    ),
                    can_advance: !self.cursor_is_at_page_top() || self.current_page_has_content(),
                });
            if let Some(transition) = prebreak_decision.transition_before_unit
                && let Some(content_top) = self.materialize_fragmentainer_advance(
                    transition.fragmentainer_kind,
                    FragmentainerAdvance::Unforced,
                )
            {
                fragment_cursor = transition.cursor_after_fragmentainer_advance(content_top);
            }

            let mut slice_start = unit.block_start;
            loop {
                let available_block_end = if break_is_applicable {
                    Fragmentainer::from_cursor_bounds(
                        self.page_area_height(),
                        fragment_cursor.content_top,
                        self.page_bottom(),
                    )
                    .available_block_end_from(fragment_cursor.block_offset)
                } else {
                    unit.block_end
                };
                let slice_decision = FlexUnitSliceDecision::choose(FlexUnitSliceDecisionInput {
                    fragmentainer_kind,
                    break_is_applicable,
                    // CSS Flexbox fragments at flex-line (row) or item
                    // (column) boundaries. A unit may be sliced only when
                    // it cannot fit even in an empty fragmentainer; otherwise
                    // an overflowing unit advances whole to the next one.
                    // https://www.w3.org/TR/css-flexbox-1/#pagination
                    can_slice_at_fragmentainer_boundary: !distributed_cross_axis_lines
                        && unit.block_size() > self.page_area_height() + 0.01,
                    unit_block_end: unit.block_end,
                    slice_start,
                    available_block_end,
                });
                if let Some(transition) = slice_decision.transition_before_paint {
                    debug_assert!(!slice_decision.paints_slice());
                    if let Some(content_top) = self.materialize_fragmentainer_advance(
                        transition.fragmentainer_kind,
                        FragmentainerAdvance::Unforced,
                    ) {
                        fragment_cursor =
                            transition.cursor_after_fragmentainer_advance(content_top);
                    }
                    continue;
                }
                debug_assert!(slice_decision.paints_slice());
                let committed_slice_start = slice_decision.slice_start;
                let slice_end = slice_decision.slice_end;
                let slice_unit = unit.slice(committed_slice_start, slice_end);
                let materialized_fragmentainer = Fragmentainer::from_cursor_bounds(
                    self.page_area_height(),
                    fragment_cursor.content_top,
                    self.page_bottom(),
                );
                let fragment_context = FlexFragmentBuildContext {
                    page_index: self.pages.len(),
                    outer_x,
                    outer_width,
                    content_top: fragment_cursor.content_top,
                    block_offset: fragment_cursor.block_offset,
                    first_fragmentainer_capacity: materialized_fragmentainer.available_block_size(),
                    continuation_fragmentainer_capacity: materialized_fragmentainer
                        .fragmentainer_block_size(),
                    starts_page_fragment: !self.current_page_has_content(),
                };
                let mut planned_fragment = flex_fragment_from_break_unit(
                    &slice_unit,
                    &flex_layout.items,
                    fragment_context,
                    break_is_applicable,
                );
                flex_layout
                    .fragment_plan
                    .prepare_materialized_fragment(&mut planned_fragment);
                if flex_fragmentation_enabled
                    && style.visibility == Visibility::Visible
                    && let Some(fragment_bounds) = planned_fragment.metadata.source_border_box
                    && (style.background_color.is_some()
                        || style.background_image.is_some()
                        || style.border_image.source.is_some()
                        || used_border_width(style) > layout_pt(0.0))
                {
                    let mut background_fragment =
                        PaintFragment::from_primitives(Vec::new(), Vec::new());
                    background_fragment.prepend_primitives_in_band(
                        PaintBand::BackgroundBorder,
                        self.box_background_primitives(
                            paint_space_rect(
                                outer_x,
                                fragment_bounds.y(),
                                outer_width,
                                fragment_bounds.height(),
                            ),
                            style,
                        ),
                    );
                    self.current_page.append_paint_fragment_owned(
                        background_fragment,
                        PaintTranslation::identity(),
                    );
                }
                for item_fragment in &mut planned_fragment.items {
                    let index = item_fragment.item_index;
                    let child = &children[index];
                    if flex_item_is_collapsed(&child.style) {
                        continue;
                    }
                    let item = &item_fragment.bounds;
                    let original_item = &item_fragment.original_bounds;
                    let item_width = original_item.width().max(0.0);
                    let item_x = original_item.x();
                    let item_y = original_item.y();
                    let item_height = original_item.height().max(0.0);
                    let visible_item_height = item.height().max(0.0);
                    let item_content_left = inner_x + item_x;
                    let item_cursor_y =
                        fragment_cursor.content_top - (item_y - fragment_cursor.block_offset);

                    let item_page_index = self.pages.len();
                    let item_starts_page_fragment = !self.current_page_has_content();
                    let visible_item_top =
                        fragment_cursor.content_top - (item.y() - fragment_cursor.block_offset);
                    let item_border_box = PageTopRect::new(
                        item_content_left,
                        visible_item_top,
                        item_width,
                        visible_item_height,
                    )
                    .paint_clip();
                    let mut item_metadata = FragmentPageMetadata::new(
                        item_page_index,
                        Some(item_border_box),
                        item_starts_page_fragment,
                    );
                    item_metadata.continues_from_previous_page =
                        item_fragment.content_slice.block_start.points() > 0.01;
                    item_metadata.continues_to_next_page =
                        item_fragment.content_slice.block_end.points() < item_height - 0.01;
                    let item_paint_checkpoint = self.current_page.paint_checkpoint();
                    let item_positioned_layer_start = self.positioned_layers.len();
                    let item_is_split = item_metadata.continues_from_previous_page
                        || item_metadata.continues_to_next_page;

                    let mut replay_child_style = child.style.clone();
                    freeze_replayed_item_padding(
                        &mut replay_child_style,
                        flex_item_used_padding(&child.style, style, flex_available),
                    );
                    let placed_style = placed_flex_item_style(
                        &replay_child_style,
                        item_width,
                        item_height,
                        style.flex_direction,
                    );
                    let item_was_split = self.with_formatting_context_item_placement(
                        FormattingContextItemPlacement {
                            content_left: item_content_left,
                            content_width: PhysicalContentWidth::new(content_box_pt(item_width)),
                            content_height: Some(PhysicalContentHeight::new(content_box_pt(
                                item_height,
                            ))),
                            table_wrapper_border_box_block_size:
                                auto_table_wrapper_block_size_override(
                                    &child.style,
                                    border_box_pt(item_height),
                                ),
                            writing_mode: placed_style.writing_mode,
                            scope_content_logical_inline_size: child.anonymous_content().is_some()
                                && style.flex_direction.is_column_axis(),
                            cursor_y: item_cursor_y,
                            page_start_margin_policy: PageStartMarginPolicy::Preserve,
                        },
                        |layout| {
                            if item_is_split {
                                let item_top = layout.cursor_y;
                                layout.paint_split_flex_item_fragment(
                                    child,
                                    &placed_style,
                                    stylesheets,
                                    SplitFlexItemPaintContext {
                                        item_width: border_box_pt(item_width),
                                        item_height: border_box_pt(item_height),
                                        percentage_height_basis: original_item
                                            .percentage_height_basis,
                                        slice_border_box: item_border_box,
                                        source_item_top: item_top,
                                        continuation: item_fragment.continuation,
                                        // Row flex lines and wrapped column flex lines retain
                                        // one source coordinate system across their visible
                                        // slices. Replaying them at source start duplicates
                                        // early descendants in later fragmentainers, so carry
                                        // the consumed source offset into the replay. A
                                        // single-line column flex continuation instead needs
                                        // its own fragment-local main-size re-layout; retaining
                                        // its historical non-translated replay avoids treating
                                        // that new main-axis fragment as a source slice.
                                        // <https://www.w3.org/TR/css-break-3/#box-splitting>
                                        replay_source_slice_offset: physical_flex_direction(style)
                                            .is_row_axis()
                                            || style.flex_wrap.wraps(),
                                        positioning_containing_block:
                                            establishes_positioning_containing_block.then(|| {
                                                ContainingBlock::from_page_top_rect(
                                                    PageTopRect::new(
                                                        outer_x + border_widths.left,
                                                        fragment_cursor.content_top
                                                            + positioning_containing_block_offset,
                                                        content_width
                                                            + style.padding.left
                                                            + style.padding.right,
                                                        total_content_height
                                                            + style.padding.top
                                                            + style.padding.bottom,
                                                    ),
                                                )
                                            }),
                                        establishes_fixed_containing_block,
                                        positioned_descendant_clip:
                                            establishes_positioning_containing_block.then(|| {
                                                PageTopRect::new(
                                                    outer_x,
                                                    fragment_cursor.content_top
                                                        + positioning_containing_block_offset,
                                                    outer_width,
                                                    slice_end - committed_slice_start,
                                                )
                                                .paint_clip()
                                            }),
                                    },
                                );
                                // Off-page replay restores its speculative
                                // page-side-effect state. Capture named
                                // strings and running elements from the first
                                // committed source fragment instead, as table
                                // cell replay does. Later continuations must
                                // not reassign the same source element.
                                // <https://www.w3.org/TR/css-gcpm-3/#named-strings>
                                if !item_metadata.continues_from_previous_page
                                    && let Some((element, _, _)) = child.element_parts()
                                {
                                    let (_, assignment_ids) = layout
                                        .capture_assignments_for_fragment_source_with_ids(
                                            element,
                                            &child.style,
                                            item_metadata.assignment_placement(),
                                        );
                                    item_metadata.assignment_ids = assignment_ids;
                                }
                                item_fragment.metadata = item_metadata.clone();
                                flex_layout.items[index].metadata = item_metadata;
                                return true;
                            }

                            layout.begin_assignment_capture_frame();
                            layout.layout_flex_item_contents(
                                child,
                                &placed_style,
                                stylesheets,
                                original_item.percentage_height_basis,
                            );
                            item_metadata.assignment_ids = layout.end_assignment_capture_frame();
                            if !item_metadata.assignment_ids.is_empty() {
                                layout.update_running_assignment_placements(
                                    &item_metadata.assignment_ids,
                                    item_metadata.assignment_placement(),
                                );
                            }
                            item_fragment.metadata = item_metadata.clone();
                            flex_layout.items[index].metadata = item_metadata;
                            let policy = StackingContextPolicy::for_flex_item(
                                &placed_style,
                                item_border_box,
                            );
                            if !matches!(policy.context_kind, StackingContextKind::None) {
                                let child_contexts = layout.positioned_child_contexts_since(
                                    item_positioned_layer_start,
                                    item_page_index,
                                    policy,
                                );
                                layout.scope_current_page_paint_since_with_policy(
                                    &item_paint_checkpoint,
                                    policy,
                                    item_border_box,
                                    child_contexts,
                                );
                            }
                            false
                        },
                    );
                    if item_was_split {
                        continue;
                    }
                }
                flex_layout
                    .fragment_plan
                    .push_materialized_fragment(planned_fragment);
                if slice_end >= unit.block_end - 0.01 {
                    break;
                }
                let transition = FlexFragmentTransitionDecision::slice_continuation(
                    fragmentainer_kind,
                    slice_end,
                );
                if let Some(content_top) = self.materialize_fragmentainer_advance(
                    transition.fragmentainer_kind,
                    FragmentainerAdvance::Unforced,
                ) {
                    fragment_cursor = transition.cursor_after_fragmentainer_advance(content_top);
                }
                slice_start = slice_end;
            }
            if break_is_applicable {
                forced_break_carry.finish_box(break_context, unit_index + 1 < break_units.len());
            }
            previous_break_after = if break_is_applicable {
                break_context
                    .avoid_after_in(fragmentainer_kind)
                    .unwrap_or(PageBreak::Auto)
            } else {
                PageBreak::Auto
            };
        }
        self.pop_float_context();

        for child in &positioned_children {
            self.layout_positioned_flex_child(
                child,
                PositionedFlexStaticContext {
                    container_style: style,
                    stylesheets,
                    available: FlexAvailableSpace {
                        width: PhysicalContentWidth::new(content_box_pt(inner_width)),
                        width_basis: flex_available_percentage_basis_from_points(
                            Some(inner_width),
                            FlexAvailableSizeSource::ContainingBlock,
                        ),
                        height: flex_available_content_height
                            .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                        height_basis: flex_available_percentage_basis_from_points(
                            definite_content_height,
                            FlexAvailableSizeSource::ContainingBlock,
                        ),
                    },
                    inner_x,
                    inner_width,
                    content_top,
                },
            );
        }
        self.pop_overflow_clip(overflow_clip_active);
        self.content_left = previous_left;
        self.content_right = previous_right;

        self.cursor_y =
            fragment_cursor.content_top - (total_content_height - fragment_cursor.block_offset);
        if flex_fragmentation_enabled {
            self.fragment_top_offsets.pop();
        }
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
        let background_page_index = self.pages.len();
        let mut own_background_primitives = Vec::new();
        let mut own_outline_primitives = Vec::new();
        if self.element_propagates_document_canvas_properties(element, style) {
            if style.visibility == Visibility::Visible {
                self.capture_document_canvas_background(element, style);
            }
            // The root background propagates to the canvas, while percentage
            // background geometry still resolves against html's used box.
            // Block-root layout records this area after capture; flex-root
            // layout must do the same before final page-canvas painting.
            // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
            if element.tag.eq_ignore_ascii_case("html") {
                self.record_document_canvas_root_positioning_area(
                    PaintBackgroundArea::from_paint_rect(paint_space_rect(
                        outer_x,
                        block_bottom,
                        outer_width,
                        block_height,
                    )),
                );
            }
        } else if block_height > 0.0
            && (style.background_color.is_some()
                || style.background_image.is_some()
                || style.border_image.source.is_some()
                || used_border_width(style) > layout_pt(0.0))
            && style.visibility == Visibility::Visible
        {
            own_background_primitives = self.box_background_primitives(
                paint_space_rect(outer_x, block_bottom, outer_width, block_height),
                style,
            );
        }
        if block_height > 0.0 && style.visibility == Visibility::Visible {
            let gap_gutters =
                flex_gap_decoration_gutters(&flex_layout, style, inner_width, total_content_height);
            own_background_primitives.extend(flex_gap_decoration_primitives_with_gutters(
                style,
                GapDecorationContainer::new(
                    inner_x,
                    content_top,
                    inner_width,
                    total_content_height,
                ),
                &flex_gap_decoration_items(&flex_layout),
                &gap_gutters,
            ));
        }
        if block_height > 0.0 && style.visibility == Visibility::Visible {
            own_outline_primitives = self.box_outline_primitives(
                paint_space_rect(outer_x, block_bottom, outer_width, block_height),
                style,
            );
        }
        let has_own_background_primitives = !own_background_primitives.is_empty();
        let has_own_outline_primitives = !own_outline_primitives.is_empty();
        if self.preserve_scoped_paint_public_order && self.pages.len() == paint_page_index {
            let mut fragment = self
                .current_page
                .paint_tree_fragment_since(&paint_checkpoint);
            if let Some(clip) = contents_overflow_clip {
                if style.overflow.clips_overflow() {
                    fragment = fragment.with_primitives_clipped_to_rect_preserving_structure(clip);
                }
                fragment = fragment.with_contents_effect_scoped_to_rect(clip);
            }
            if background_page_index == paint_page_index {
                self.current_page.prepend_recorded_primitives_to_fragment(
                    &mut fragment,
                    PaintBand::BackgroundBorder,
                    own_background_primitives.clone(),
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
            if (has_own_background_primitives || has_own_outline_primitives) && !fragment.is_empty()
            {
                self.current_page
                    .replace_paint_tree_since_with_fragment(&paint_checkpoint, fragment);
            }
            self.cursor_y -= style.margin.bottom;
            if let Some(scope) = positioned_containing_block_scope {
                self.pop_positioned_containing_block(scope);
                self.cursor_y -= relative_offset.y();
            }
            self.apply_forced_break_in(
                fragmentainer_kind,
                FragmentBreakContext::for_standalone_box(style).forced_break_after_or_in(
                    fragmentainer_kind,
                    forced_break_carry.outgoing_source_break(),
                ),
            );
            return;
        }
        let fragments = self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        let flex_spanned_pages = self.pages.len() != paint_page_index;
        for (page_index, mut fragment) in fragments {
            if page_index == paint_page_index
                && let Some(clip) = contents_overflow_clip
            {
                if style.overflow.clips_overflow() {
                    fragment = fragment.with_primitives_clipped_to_rect_preserving_structure(clip);
                }
                fragment = fragment.with_contents_effect_scoped_to_rect(clip);
            }
            if flex_fragmentation_enabled || flex_spanned_pages {
                let fragment_bounds =
                    flex_container_page_fragment_bounds(&flex_layout.fragment_plan, page_index)
                        .or_else(|| {
                            flex_spanned_pages.then(|| {
                                fragment.bounds().map(|bounds| {
                                    PaintClip::from_paint_rect(paint_space_rect(
                                        outer_x,
                                        bounds.y(),
                                        outer_width,
                                        bounds.height(),
                                    ))
                                })
                            })?
                        });
                if let Some(fragment_bounds) = fragment_bounds {
                    if style.visibility == Visibility::Visible
                        && (style.background_color.is_some()
                            || style.background_image.is_some()
                            || style.border_image.source.is_some()
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
                            flex_gap_decoration_primitives_for_page(
                                &flex_layout,
                                style,
                                page_index,
                                inner_x,
                                inner_width,
                                total_content_height,
                                fragment_bounds,
                                flex_has_forced_item_breaks,
                            ),
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
        self.cursor_y -= style.margin.bottom;
        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
            self.cursor_y -= relative_offset.y();
        }
        self.apply_forced_break_in(
            fragmentainer_kind,
            FragmentBreakContext::for_standalone_box(style).forced_break_after_or_in(
                fragmentainer_kind,
                forced_break_carry.outgoing_source_break(),
            ),
        );
    }
}
