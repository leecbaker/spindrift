use super::*;
use crate::layout::block::BlockLayoutInlineConstraint;
use crate::layout::block::suppress_fragmented_box_edges;

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
        available_outer_width: LayoutLength,
        horizontal_non_content: NonContentLength,
        vertical_non_content: NonContentLength,
    ) -> PhysicalContentWidth {
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
            return PhysicalContentWidth::new(used_content_box_width(
                style,
                available_outer_width,
                horizontal_non_content,
            ));
        }

        let height_percentage_basis = self.flex_container_height_percentage_basis();
        let intrinsic_content_height = used_content_box_height_or_auto_with_basis(
            style,
            height_percentage_basis,
            vertical_non_content,
        )
        .map(SemanticLengthExt::points);
        // A shrink-to-fit size-contained flex container has the intrinsic
        // inline sizes of empty content. Authored width/min/max constraints
        // are still resolved by `flex_container_content_width_from_intrinsic`.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        let intrinsic = if intrinsic_physical_width_is_contained(style) {
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(0.0)),
                PhysicalContentHeight::new(content_box_pt(0.0)),
            )
        } else {
            self.estimate_intrinsic_flex_container_size(
                children,
                style,
                stylesheets,
                FlexAvailableSpace {
                    width: PhysicalContentWidth::new(content_box_pt(
                        available_outer_width.points().max(0.0),
                    )),
                    width_basis: flex_available_percentage_basis_from_points(
                        used_content_box_width_or_auto(
                            style,
                            layout_pt(available_outer_width.points().max(0.0)),
                            horizontal_non_content,
                        )
                        .map(|_| available_outer_width.points().max(0.0)),
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
            horizontal_non_content,
            intrinsic,
            style.float != Float::None || orthogonal_auto_width,
        )
    }

    pub(in crate::layout) fn layout_flex_with_descendant_percentage_height_basis(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
    ) {
        self.layout_flex_with_descendant_percentage_height_basis_request(
            element,
            style,
            stylesheets,
            child_boxes,
            descendant_percentage_height_basis
                .map(FlexDescendantPercentageHeightBasis::Override)
                .unwrap_or(FlexDescendantPercentageHeightBasis::DeriveFromContainer),
        );
    }

    fn layout_flex_with_descendant_percentage_height_basis_request(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: FlexDescendantPercentageHeightBasis,
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

        let normal_flow_available_outer_width = layout_pt(
            self.content_right
                - self.content_left
                - used_style.margin.left
                - used_style.margin.right,
        );
        let border_widths = box_metrics.border.to_css_edges();
        let horizontal_non_content = box_metrics.horizontal_non_content_length();
        let vertical_non_content = box_metrics.vertical_non_content_length();

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
        // Consequently, an automatic-width flex container, or one whose
        // negative inline margin may fit beside the float, must use the
        // available float-avoidance band rather than first resolving to the
        // full containing-block width and being forced below the float. The
        // probe mirrors normal block-flow BFC-root placement while retaining
        // the containing block's full inline size as the percentage basis.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        let has_active_float = self
            .float_contexts
            .last()
            .is_some_and(|context| !context.shapes.is_empty());
        let float_avoiding_placement = (has_active_float
            && used_style.float == Float::None
            && (used_style.box_values.width.is_auto()
                || used_style.margin.left < -0.01
                || used_style.margin.right < -0.01)
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
                    PageTopBlockPosition::new(placement_top),
                    clear,
                    writing_mode,
                    direction,
                    self.content_left,
                    self.content_right,
                    |band, _candidate_top| {
                        let band_left = band.left();
                        let band_width = band.width();
                        let candidate_geometry = self.block_layout_geometry_in_inline_span(
                            element,
                            &used_style,
                            stylesheets,
                            Some(child_boxes),
                            BlockLayoutInlineConstraint {
                                containing_inline_span: PageInlineSpan::new(band_left, band_width),
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
                                candidate_geometry.outer_inline().width().points(),
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
                            border_box_inline_span: PageInlineSpan::new(
                                candidate_geometry.outer_inline().span().left_x()
                                    - candidate_geometry.relative_offset.x(),
                                candidate_geometry.outer_inline().span().width(),
                            ),
                            border_box_block_size: border_box_pt(border_box_height),
                            permits_inline_start_overflow: match candidate_style.direction {
                                Direction::Ltr => candidate_style.margin.left < -0.01,
                                Direction::Rtl => candidate_style.margin.right < -0.01,
                            },
                            permits_inline_end_overflow: match candidate_style.direction {
                                Direction::Ltr => candidate_style.margin.right < -0.01,
                                Direction::Rtl => candidate_style.margin.left < -0.01,
                            },
                        }
                    },
                )
            });
        let available_outer_width = float_avoiding_placement
            .map(|placement| {
                layout_pt(
                    (placement.placement.available_span.width()
                        - used_style.margin.left
                        - used_style.margin.right)
                        .max(0.0),
                )
            })
            .unwrap_or(normal_flow_available_outer_width);

        let requested_content_width = self.used_flex_container_content_width(
            &children,
            &used_style,
            stylesheets,
            available_outer_width,
            horizontal_non_content,
            vertical_non_content,
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
            resolve_normal_flow_block_inline_geometry(
                &mut used_style,
                PageInlineSpan::from_edges(self.content_left, self.content_right),
                requested_content_width,
                horizontal_non_content,
                self.containing_block_direction,
                true,
            )
        });
        let mut content_width = resolved_normal_flow_width
            .map(|width| PhysicalContentWidth::new(width.content_width))
            .unwrap_or_else(|| {
                PhysicalContentWidth::new(constrain_content_width(
                    &used_style,
                    requested_content_width.content_box_length(),
                    PercentageBasis::definite(available_outer_width),
                ))
            });
        let mut content_width_points = content_width.points();
        let mut outer_width = resolved_normal_flow_width
            .map(|width| width.border_box_width().points())
            .unwrap_or_else(|| (content_width_points + horizontal_non_content.points()).max(0.0));
        let style = &used_style;
        let mut outer_x = float_avoiding_placement
            .map(|placement| placement.placement.available_span.left_x() + style.margin.left)
            .or_else(|| {
                resolved_normal_flow_width.map(|width| width.border_box_inline_span.left_x())
            })
            .unwrap_or_else(|| {
                normal_flow_block_border_box_span(
                    PageInlineSpan::from_edges(self.content_left, self.content_right),
                    style,
                    border_box_pt(outer_width),
                    self.containing_block_direction,
                )
                .left_x()
            })
            + relative_offset.x();
        let mut inner_x = outer_x + border_widths.left + style.padding.left;
        let mut inner_width = content_width_points;
        let available_outer_height = self
            .fragmentainer_from_page_cursor(PageTopBlockPosition::new(self.cursor_y))
            .available_block_size_after_reservation(layout_pt(
                style.margin.top + style.margin.bottom,
            ))
            .points();
        let height_percentage_basis = self.flex_container_height_percentage_basis();
        let height_constraint_basis = height_percentage_basis
            .value()
            .unwrap_or_else(|| content_box_pt(available_outer_height));
        // A replayed flex item may have received a definite used block size
        // from its parent flex line even though its authored `height` is
        // `auto`.  Its root flex formatting context must use that assigned
        // content-box size as its own final cross-size constraint.  Passing
        // it only as a percentage basis lets the nested flex container remain
        // auto-sized and paint its provisional intrinsic source extent.
        //
        // Keep an explicitly-indefinite replay override distinct: it carries
        // the parent's used geometry without granting percentage definiteness
        // to this root formatting context.
        // <https://drafts.csswg.org/css-flexbox-1/#definite-sizes>
        // <https://drafts.csswg.org/css-flexbox-1/#algo-cross-container>
        let replayed_item_content_height = descendant_percentage_height_basis
            .override_basis()
            .and_then(|basis| basis.value())
            .map(|height| {
                constrain_content_height(
                    style,
                    height,
                    PercentageBasis::definite_from(
                        height_constraint_basis,
                        BlockSizeBasisSource::ContainingBlock,
                    ),
                )
            });
        let explicit_content_height = replayed_item_content_height.or_else(|| {
            used_content_box_height_or_auto_with_basis(
                style,
                height_percentage_basis,
                vertical_non_content,
            )
            .map(|height| {
                constrain_content_height(
                    style,
                    height,
                    PercentageBasis::definite_from(
                        height_constraint_basis,
                        BlockSizeBasisSource::ContainingBlock,
                    ),
                )
            })
        });
        // A block-level flex container with an automatic logical inline size
        // fills its containing block's inline axis. In an orthogonal writing
        // mode that is physical height, so it must be supplied to the
        // physical Flex adapter as a definite cross size instead of being
        // mistaken for an automatic physical block size.
        // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
        // <https://www.w3.org/TR/CSS2/visudet.html#blockwidth>
        let orthogonal_inline_auto_height = orthogonal_block_flex_auto_inline_content_height(
            style,
            style.float == Float::None
                && self.float_paint_capture_depth == 0
                && self.fragmentation_suppression_depth == 0,
            self.current_child_available_space()
                .available_physical_height(),
            vertical_non_content,
        );
        // An orthogonal block's automatic inline fill supplies a numeric
        // physical-height constraint to the Taffy adapter, but it is not a
        // definite CSS `height`. In particular, percentage flex gaps on that
        // axis are cyclic and contribute zero. Keep the used constraint and
        // percentage definiteness separate instead of promoting this auto
        // fill through `definite_flex_container_content_height`.
        // <https://www.w3.org/TR/css-sizing-3/#definite> and
        // <https://www.w3.org/TR/css-align-3/#gap-percent>
        let orthogonal_inline_auto_height = orthogonal_inline_auto_height.map(|available_height| {
            constrain_content_height(
                style,
                available_height,
                PercentageBasis::definite_from(
                    height_constraint_basis,
                    BlockSizeBasisSource::ContainingBlock,
                ),
            )
        });
        let definite_content_height = definite_flex_container_content_height(
            style,
            explicit_content_height,
            content_width.content_box_length(),
            PercentageBasis::definite_from(
                height_constraint_basis,
                BlockSizeBasisSource::ContainingBlock,
            ),
            horizontal_non_content,
            vertical_non_content,
        );
        let size_contained_content_height = style.contain.size.then(|| {
            definite_content_height.unwrap_or_else(|| {
                constrain_content_height(
                    style,
                    content_box_pt(0.0),
                    PercentageBasis::definite_from(
                        height_constraint_basis,
                        BlockSizeBasisSource::ContainingBlock,
                    ),
                )
            })
        });
        // A normal-flow orthogonal wrapped flexbox first obtains its automatic
        // logical inline size from intrinsic flex layout. Percentage main-axis
        // gaps are cyclic in that pass and therefore zero; after the used
        // inline size is known, a final pass resolves them and can form more
        // cross-axis lines.
        // <https://drafts.csswg.org/css-align-3/#gap-percent>
        // <https://drafts.csswg.org/css-flexbox-1/#layout-algorithm>
        let cyclic_orthogonal_wrap =
            WritingModeAxes::new(style.writing_mode, style.used_direction()).swaps_physical_axes()
                && physical_flex_direction(style).is_column_axis()
                && style.flex_wrap.wraps()
                && definite_content_height.is_none()
                && style.box_values.height.is_auto();
        let flex_available_content_height = flex_available_content_height(
            style,
            definite_content_height,
            PercentageBasis::definite_from(
                content_width.content_box_length(),
                BlockSizeBasisSource::ContainingBlock,
            ),
        )
        .or((!cyclic_orthogonal_wrap)
            .then_some(orthogonal_inline_auto_height)
            .flatten());
        self.cursor_y -= style.margin.top;
        if let Some(placement) = float_avoiding_placement {
            self.cursor_y = placement.placement.origin.top_y();
        } else if style.float == Float::None {
            let margin_box_width = style.margin.left + outer_width + style.margin.right;
            let collision_height = size_contained_content_height
                .or(definite_content_height)
                .map(SemanticLengthExt::points)
                .unwrap_or(style.line_height)
                + vertical_non_content.points()
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
        let block_top = self.cursor_y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        self.cursor_y -= border_widths.top + style.padding.top;

        let descendant_height_basis =
            descendant_percentage_height_basis.available_height_basis(definite_content_height);
        let flex_width_basis = if WritingModeAxes::new(style.writing_mode, style.used_direction())
            .swaps_physical_axes()
            && style.box_values.width.is_auto()
        {
            // The physical width of an orthogonal normal-flow block is its
            // logical block size. Its automatic used width can constrain the
            // physical solver, but does not make that axis a definite
            // percentage basis. In particular `row-gap` remains cyclic in a
            // vertical writing-mode column flexbox.
            // <https://www.w3.org/TR/css-sizing-3/#definite>
            // <https://drafts.csswg.org/css-align-3/#gap-percent>
            PercentageBasis::indefinite()
        } else {
            flex_available_percentage_basis_from_points(
                Some(inner_width),
                FlexAvailableSizeSource::ContainingBlock,
            )
        };
        let flex_available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(inner_width)),
            width_basis: flex_width_basis,
            height: flex_available_content_height.map(PhysicalContentHeight::new),
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
                descendant_percentage_height_basis.override_basis(),
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
                    PercentageBasis::definite(content_width.content_box_length()),
                    vertical_non_content,
                    flex_layout.height.content_box_length(),
                    flex_layout.height.content_box_length(),
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
                    height_basis: PercentageBasis::definite_from(
                        content_box_pt(final_height),
                        FlexAvailableSizeSource::ContainingBlock,
                    ),
                    ..flex_available
                },
            ) {
                flex_layout = final_layout;
            }
        }
        if cyclic_orthogonal_wrap && flex_available.height.is_none() {
            let final_height = flex_layout.height;
            if let Some(final_layout) = self.compute_flex_layout(
                &children,
                style,
                stylesheets,
                FlexAvailableSpace {
                    // The final auto inline size is definite only on the
                    // physical main axis. The auto physical cross axis still
                    // has a cyclic percentage gap, so preserve its
                    // indefiniteness while collecting the resulting lines.
                    width_basis: PercentageBasis::indefinite(),
                    height: Some(final_height),
                    height_basis: PercentageBasis::definite_from(
                        final_height.content_box_length(),
                        FlexAvailableSizeSource::DefiniteCrossSize,
                    ),
                    ..flex_available
                },
            ) {
                let resolved_cross_width = final_layout
                    .items
                    .iter()
                    .map(|item| item.x().points() + item.width().points())
                    .fold(0.0f32, f32::max);
                flex_layout = final_layout;
                if resolved_cross_width > inner_width + 0.01 {
                    inner_width = resolved_cross_width;
                    content_width_points = resolved_cross_width;
                    content_width = PhysicalContentWidth::new(content_box_pt(resolved_cross_width));
                    outer_width = (resolved_cross_width + horizontal_non_content.points()).max(0.0);
                }
            }
        }
        let flex_content_height = flex_layout.height;
        let total_content_height =
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
                        PercentageBasis::definite(content_width.content_box_length()),
                        vertical_non_content,
                        flex_content_height.content_box_length(),
                        flex_content_height.content_box_length(),
                    )
                    .map(SemanticLengthExt::points)
                    .unwrap_or(flex_content_height.points())
                } else {
                    flex_content_height.points()
                };
                constrain_content_height(
                    style,
                    content_box_pt(requested_height),
                    PercentageBasis::definite_from(
                        content_width.content_box_length(),
                        BlockSizeBasisSource::ContainingBlock,
                    ),
                )
            };
        let mut total_content_height = total_content_height.points();
        let unconstrained_item_cross_height = flex_layout
            .items
            .iter()
            .map(|item| item.height().points())
            .fold(0.0f32, f32::max);
        // An automatic single-line row flex container first derives its
        // content height from its items, then applies min/max block-size
        // constraints.  If that produces a different used cross size, the
        // flex line's stretched items must be laid out again with that final
        // definite height before descendant percentages resolve.  Merely
        // clamping the container after Taffy returns leaves a nested flex
        // item measuring `max-height: 100%` against the original indefinite
        // cross axis.
        //
        // The recomputation deliberately retains the selected single line:
        // this is a cross-axis correction, not a second line-collection pass.
        // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
        let constrained_auto_cross_height = (explicit_content_height.is_none()
            // A fragmentainer can impose a physical layout limit without
            // making the flex container's CSS block size definite.  In that
            // case an automatic row flex container still has to rerun its
            // stretched line after `min-height`/`max-height` establishes the
            // used cross size.  Treating the presence of that limit as a
            // definite size skips the rerun and leaves nested percentage
            // heights resolved against the provisional intrinsic line.
            //
            // <https://drafts.csswg.org/css-sizing-3/#percentage-sizing>
            && !flex_available.height_basis.is_definite()
            && physical_flex_direction(style).is_row_axis()
            && style.flex_wrap == FlexWrap::NoWrap)
            .then(|| {
                // Taffy can clamp its reported root height through
                // `max-height` even though item measurement still observed
                // an indefinite available cross size. Preserve that signal:
                // equality between the clamped root and final used height is
                // not evidence that descendants saw a definite basis.
                used_max_height(
                    style,
                    PercentageBasis::definite_from(
                        height_constraint_basis,
                        BlockSizeBasisSource::ContainingBlock,
                    ),
                )
                .filter(|max_height| flex_content_height.points() >= max_height.points() - 0.01)
                .map(SemanticLengthExt::points)
                .or_else(|| {
                    // Taffy can likewise report a min-height-clamped root
                    // while retaining the pre-clamp line cross size. A
                    // stretched child must be relaid out against that final
                    // used size before its own descendants resolve
                    // percentages.
                    // <https://drafts.csswg.org/css-flexbox-1/#definite-sizes>
                    used_min_height(
                        style,
                        PercentageBasis::definite_from(
                            height_constraint_basis,
                            BlockSizeBasisSource::ContainingBlock,
                        ),
                    )
                    .filter(|min_height| {
                        flex_content_height.points() >= min_height.points() - 0.01
                            // A non-binding minimum has already been
                            // satisfied by the initial line. Replaying that
                            // line against the root's used height changes
                            // ordinary auto-height flex sizing even though
                            // CSS has not established a new stretch target.
                            && min_height.points() > unconstrained_item_cross_height + 0.01
                    })
                    .map(SemanticLengthExt::points)
                })
                .or_else(|| {
                    ((total_content_height - flex_content_height.points()).abs() > 0.01)
                        .then_some(total_content_height)
                })
            })
            .flatten();
        if let Some(constrained_auto_cross_height) = constrained_auto_cross_height {
            let mut constrained_cross_size_style = style.clone();
            constrained_cross_size_style.flex_wrap = FlexWrap::NoWrap;
            if let Some(constrained_layout) = self.compute_flex_layout(
                &children,
                &constrained_cross_size_style,
                stylesheets,
                FlexAvailableSpace {
                    height: Some(PhysicalContentHeight::new(content_box_pt(
                        constrained_auto_cross_height,
                    ))),
                    height_basis: PercentageBasis::definite_from(
                        content_box_pt(constrained_auto_cross_height),
                        FlexAvailableSizeSource::DefiniteCrossSize,
                    ),
                    ..flex_available
                },
            ) {
                flex_layout = constrained_layout;
                // The first pass supplied the intrinsic auto cross size only
                // to discover the final min/max-constrained line.  Once the
                // final pass has laid that line out, every downstream
                // operation—fragment planning, paint bounds, and the
                // container's block advance—must use its final used height
                // rather than the provisional intrinsic height.
                // <https://drafts.csswg.org/css-flexbox-1/#algo-cross-container>
                total_content_height = size_contained_content_height
                    .unwrap_or_else(|| {
                        constrain_content_height(
                            style,
                            flex_layout.height.content_box_length(),
                            PercentageBasis::definite_from(
                                content_width.content_box_length(),
                                BlockSizeBasisSource::ContainingBlock,
                            ),
                        )
                    })
                    .points();
            }
        }
        let flex_lines_overflow_content_block = physical_flex_direction(style).is_row_axis()
            && flex_layout.lines.iter().any(|line| {
                line.item_indices.iter().any(|&item_index| {
                    children[item_index].style.display.is_flex()
                        && flex_item_block_bounds(&flex_layout.items[item_index], true)
                            .end()
                            .points()
                            > total_content_height + 0.01
                })
            });
        let mut break_units = flex_container_break_units(
            fragmentainer_kind,
            &flex_layout,
            &children,
            style,
            false,
            layout_pt(total_content_height),
        );
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
        // The Flexbox pagination algorithm owns a boundary between every
        // flex line (physical rows) or item progression interval (physical
        // columns).  Moving a multi-unit container wholesale before those
        // boundaries are considered loses the available source slice and can
        // turn a two-page line sequence into one empty page followed by the
        // complete container.  Only an atomic one-unit container may use the
        // ordinary block-flow whole-box prebreak.
        // <https://drafts.csswg.org/css-flexbox-1/#pagination>
        let has_fragmentable_flex_boundaries = break_units.len() > 1;
        if flex_container_allows_whole_box_prebreak(
            fragmentainer_kind,
            self.fragmentation_suppression_depth,
            flex_has_forced_item_breaks,
        ) && !has_fragmentable_flex_boundaries
            && should_move_flex_container_to_next_page(
                PageTopBlockPosition::new(block_top),
                layout_pt(style.margin.top),
                layout_pt(total_height),
                PageTopBlockPosition::new(self.page_top()),
                PageTopBlockPosition::new(self.page_bottom()),
                layout_pt(self.page_area_height()),
            )
        {
            self.push_page();
            self.layout_flex_with_descendant_percentage_height_basis_request(
                element,
                source_style,
                stylesheets,
                Some(child_boxes),
                descendant_percentage_height_basis,
            );
            return;
        }
        let defer_own_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = false;
        let suppress_own_principal_box_decoration = self.suppress_next_principal_box_decoration;
        self.suppress_next_principal_box_decoration = false;
        let content_top = self.cursor_y;
        let flex_overflows_current_page = block_top - total_height < self.page_bottom() - 0.01;
        // A flex item's used size can fit while its normal-flow descendants
        // require a longer fragmentable source extent. That overflow is
        // recorded during flex measurement and must itself activate the flex
        // fragment plan; otherwise the descendant paint escapes through the
        // enclosing multicolumn algorithm without the item's continuation.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        let flex_item_overflows_container_source =
            flex_layout
                .items
                .iter()
                .zip(&children)
                .any(|(item, child)| {
                    let overflows = flex_item_block_bounds(item, true).end().points()
                        > total_content_height + 0.01;
                    overflows
                        && (physical_flex_direction(style).is_column_axis()
                            // Wrapped row lines are independently
                            // fragmentable Flexbox boundaries. An item may
                            // extend its line beyond the container's used
                            // cross size even when the container itself fits
                            // exactly in the current column, so that line
                            // must enter the fragment plan instead of losing
                            // its continuation paint.
                            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                            || (physical_flex_direction(style).is_row_axis()
                                && style.flex_wrap.wraps())
                        // A nested row flexbox may acquire additional
                        // fragmentable block extent only after its outer
                        // item receives a resolved narrower main size.
                        // Ordinary row item overflow keeps its established
                        // replay path.
                        || child.style.display.is_flex())
                });
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
                || flex_item_overflows_container_source
                || flex_lines_overflow_content_block
                || auto_row_reaches_column_boundary
                || flex_has_forced_item_breaks);
        if flex_fragmentation_enabled {
            break_units = flex_container_break_units(
                fragmentainer_kind,
                &flex_layout,
                &children,
                style,
                true,
                layout_pt(total_content_height),
            );
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
            && style.box_values.height.is_auto()
            && let Some(fragmented_cross_size) = {
                let first_fragment_capacity = FlexFragmentBlockSize::new(
                    self.fragmentainer_from_page_cursor(PageTopBlockPosition::new(content_top))
                        .available_block_size()
                        .points(),
                );
                // At this exact automatic-column boundary, reserve the
                // zero-width continuation before deriving the number of
                // occupied line fragments. The helper intentionally keeps
                // ordinary exact fits unsliced, so only this pagination case
                // supplies the infinitesimal source tail.
                let source_cross_size = if auto_row_reaches_column_boundary
                    && (total_content_height - first_fragment_capacity.points()).abs() <= 0.01
                {
                    total_content_height + 0.02
                } else {
                    total_content_height
                };
                single_line_row_fragmented_cross_size(
                    FlexCrossSize::new(source_cross_size),
                    first_fragment_capacity,
                    FlexFragmentBlockSize::new(self.page_area_height()),
                )
            }
        {
            total_content_height = fragmented_cross_size.points();
            flex_layout.height =
                PhysicalContentHeight::new(content_box_pt(fragmented_cross_size.points()));
            for (item, child) in flex_layout.items.iter_mut().zip(&children) {
                if row_flex_item_stretches_in_fragment(&child.style, style) {
                    item.set_fragmentation_height(PhysicalContentHeight::new(content_box_pt(
                        fragmented_cross_size.points(),
                    )));
                }
            }
            for line in &mut flex_layout.lines {
                line.cross_end = FlexCrossOffset::new(
                    line.cross_start.points() + fragmented_cross_size.points(),
                );
            }
            break_units = flex_container_break_units(
                fragmentainer_kind,
                &flex_layout,
                &children,
                style,
                true,
                layout_pt(total_content_height),
            );
            total_height = border_widths.top
                + style.padding.top
                + total_content_height
                + style.padding.bottom
                + border_widths.bottom;
        }
        // Fragmenting an auto-height, single-line column flexbox extends its
        // source sequence to the end of its final occupied fragmentainer. The
        // item crossing that boundary owns the additional continuation span,
        // which shifts later main-axis items without changing their ordinary
        // used flex sizes.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        if flex_fragmentation_enabled
            && physical_flex_direction(style).is_column_axis()
            && (style.flex_wrap == FlexWrap::NoWrap || flex_layout.lines.len() == 1)
            && style.box_values.height.is_auto()
        {
            let first_fragment_capacity = FlexFragmentBlockSize::new(
                self.fragmentainer_from_page_cursor(PageTopBlockPosition::new(content_top))
                    .available_block_size()
                    .points(),
            );
            if let Some(fragmented_main_size) = single_line_column_fragmented_main_size(
                FlexFragmentBlockSize::new(total_content_height),
                first_fragment_capacity,
                FlexFragmentBlockSize::new(self.page_area_height()),
            ) {
                let expansion = fragmented_main_size.points() - total_content_height;
                let boundary = first_fragment_capacity.points();
                if expansion > 0.01
                    && let Some(owner_index) =
                        flex_layout
                            .items
                            .iter()
                            .enumerate()
                            .find_map(|(index, item)| {
                                let bounds = flex_item_block_bounds(item, true);
                                (children[index].style.box_values.height.is_auto()
                                    && bounds.start().points() < boundary - 0.01
                                    && bounds.end().points() > boundary + 0.01)
                                    .then_some(index)
                            })
                {
                    let owner_fragmentation_height = flex_layout.items[owner_index]
                        .fragmentation_height()
                        .points();
                    flex_layout.items[owner_index].set_fragmentation_height(
                        PhysicalContentHeight::new(content_box_pt(
                            owner_fragmentation_height + expansion,
                        )),
                    );
                    let owner_block_start = flex_layout.items[owner_index].y().points();
                    for item in flex_layout.items.iter_mut().skip(owner_index + 1) {
                        if item.y().points() >= owner_block_start - 0.01 {
                            item.set_y(FlexPhysicalVerticalOffset::new(
                                item.y().points() + expansion,
                            ));
                        }
                    }
                    total_content_height = fragmented_main_size.points();
                    flex_layout.height =
                        PhysicalContentHeight::new(content_box_pt(total_content_height));
                    break_units = flex_container_break_units(
                        fragmentainer_kind,
                        &flex_layout,
                        &children,
                        style,
                        true,
                        layout_pt(total_content_height),
                    );
                    total_height = border_widths.top
                        + style.padding.top
                        + total_content_height
                        + style.padding.bottom
                        + border_widths.bottom;
                }
            }
        }
        // Wrapped physical-column lines do not share a main-axis sequence.
        // Materialize continuation growth per line before break units are
        // rebuilt, so the committed item record, split replay, and container
        // decoration all observe the same final-fragmentainer span.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        if flex_fragmentation_enabled
            && physical_flex_direction(style).is_column_axis()
            && style.flex_wrap.wraps()
            && flex_layout.lines.len() > 1
            && expand_wrapped_column_items_through_fragmentainers(
                &mut flex_layout.items,
                &flex_layout.lines,
                FlexFragmentBlockSize::new(
                    self.fragmentainer_from_page_cursor(PageTopBlockPosition::new(content_top))
                        .available_block_size()
                        .points(),
                ),
                FlexFragmentBlockSize::new(self.page_area_height()),
            )
        {
            break_units = flex_container_break_units(
                fragmentainer_kind,
                &flex_layout,
                &children,
                style,
                true,
                layout_pt(total_content_height),
            );
        }
        if flex_fragmentation_enabled {
            self.fragment_top_offsets
                .push(self.current_page_context.top() - content_top);
        }
        // A zero-height multicolumn container has only the one-CSS-pixel
        // progress capacity. Its physical-row items are independent atomic
        // overflow subjects: the first remains in the originating column and
        // a later item can advance at its own flex boundary. Do this after
        // every source-layout recomputation so the canonical fragment plan
        // receives the final item intervals rather than provisional line
        // geometry.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        // <https://www.w3.org/TR/css-break-3/#breaking-rules>
        if flex_fragmentation_enabled
            && fragmentainer_kind == FragmentainerKind::Column
            && physical_flex_direction(style).is_row_axis()
            && self
                .fragmentainer_from_page_cursor(PageTopBlockPosition::new(content_top))
                .fragmentainer_block_size()
                .points()
                <= css::CSS_PX_TO_PT + 0.01
        {
            let atomic_item_units =
                flex_zero_capacity_column_item_break_units(&flex_layout, &children, style);
            if !atomic_item_units.is_empty() {
                break_units = atomic_item_units;
            }
        }
        // An auto-sized flexbox acquires the fragmentainer span of an
        // auto-sized item that crosses a fragmentation boundary. This covers
        // column main-axis progression and a row item's post-flex nested
        // overflow extent. If a later whole flex item prebreaks into the
        // final fragmentainer, that span includes the otherwise empty
        // remainder after the item as well.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let auto_item_fragmentation_expands_container = flex_fragmentation_enabled
            && style.box_values.height.is_auto()
            && flex_layout.items.iter().enumerate().any(|(index, item)| {
                if !children[index].style.box_values.height.is_auto() {
                    return false;
                }
                let bounds = flex_item_block_bounds(item, true);
                if physical_flex_direction(style).is_row_axis()
                    && bounds.end().points() > total_content_height + 0.01
                {
                    return true;
                }
                if !physical_flex_direction(style).is_column_axis() {
                    return false;
                }
                fragmented_flex_item_block_end(
                    bounds.start(),
                    bounds.end(),
                    FlexFragmentBlockSize::new(
                        self.fragmentainer_from_page_cursor(PageTopBlockPosition::new(content_top))
                            .available_block_size()
                            .points(),
                    ),
                    FlexFragmentBlockSize::new(self.page_area_height()),
                )
                .is_some()
            });
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
                    content_width_points + style.padding.left + style.padding.right,
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
                content_width_points,
                total_content_height,
            )
        } else {
            false
        };

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        // Flex item replay establishes the flex container as the containing
        // formatting context. In particular, an orthogonal item must resolve
        // its logical axes against the flex container's writing mode, not the
        // outer block formatting context that happened to enter this
        // container.
        // <https://www.w3.org/TR/css-flexbox-1/#flex-items>
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let previous_containing_block_direction = self.containing_block_direction;
        let previous_containing_block_writing_mode = self.containing_block_writing_mode;
        self.containing_block_direction = style.used_direction();
        self.containing_block_writing_mode = style.writing_mode;
        if flex_fragmentation_enabled {
            self.content_left = inner_x;
            self.content_right = inner_x + inner_width;
        }
        self.push_float_context();
        flex_layout.fragment_plan.fragments.clear();
        flex_layout.fragment_plan.materialized_fragments.clear();
        let mut fragment_cursor = FlexFragmentCursor::new(
            PageTopBlockPosition::new(content_top),
            FlexFragmentBlockOffset::new(0.0),
        );
        let mut forced_break_carry = ForcedBreakCarryState::new(fragmentainer_kind);
        let mut previous_break_after = PageBreak::Auto;
        // A split child may itself establish a sequence of fragmentainers.
        // Keep those committed child fragments alongside the flex source item
        // for this layout pass so later flex continuations resume their child
        // state rather than reconstructing it from the current page cursor.
        let mut split_item_replay_fragments = vec![Vec::<PaintFragment>::new(); children.len()];
        let mut split_item_replay_fragment_local_block_ends =
            vec![Vec::<Option<f32>>::new(); children.len()];
        let mut split_item_replay_fragment_bottoms = Vec::<Option<f32>>::new();
        // A child continuation can re-establish a larger destination line box
        // than the source remainder that selected it. Keep that destination
        // end per materialized page so the following flex line resumes from
        // the committed child fragment, not from stale source-only offsets.
        let mut replayed_child_destination_ends = Vec::<Option<(f32, f32)>>::new();
        let distributed_cross_axis_lines = matches!(
            style.align_content.keyword,
            ContentAlignmentKeyword::SpaceBetween
                | ContentAlignmentKeyword::SpaceAround
                | ContentAlignmentKeyword::SpaceEvenly
        );
        for (unit_index, unit) in break_units.iter().enumerate() {
            let force_after_unit = fragmentainer_kind.is_forced_break(unit.break_after)
                || break_units.get(unit_index + 1).is_some_and(|next_unit| {
                    fragmentainer_kind.is_forced_break(next_unit.break_before)
                });
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
                    fragment_cursor = transition
                        .cursor_after_fragmentainer_advance(PageTopBlockPosition::new(content_top));
                }
            }
            let current_fragmentainer =
                self.fragmentainer_from_page_cursor(fragment_cursor.content_top);
            // A flex line/item may fit in an empty fragmentainer while still
            // crossing the current remaining capacity.  It is nevertheless
            // a fragmentable source range: advancing it wholesale would
            // discard the current line slice and make the outer block
            // prebreak rule win over Flexbox pagination.  Keep that case
            // separate from a unit larger than an empty fragmentainer so the
            // slice decision below receives the actual current capacity.
            // <https://drafts.csswg.org/css-flexbox-1/#pagination>
            let unit_crosses_current_fragmentainer = break_is_applicable
                && unit.block_end.points()
                    > flex_source_block_end_after_available_capacity(
                        fragment_cursor.block_offset,
                        current_fragmentainer,
                    )
                    .points()
                        + 0.01;
            let unit_has_source_span_in_current_fragmentainer = break_is_applicable
                && unit.block_start.points()
                    < flex_source_block_end_after_available_capacity(
                        fragment_cursor.block_offset,
                        current_fragmentainer,
                    )
                    .points()
                        - 0.01;
            // Wrapped-column break units are overlapping-line interval
            // partitions. They are fragmentable source ranges even when an
            // individual partition fits in an empty fragmentainer, because a
            // preceding partition may have consumed part of the current one.
            // Feed that fact into the prebreak decision as an oversized-like
            // range so it reaches the slice decision below instead of moving
            // wholesale to the next column.
            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
            let unit_is_oversized = (unit_crosses_current_fragmentainer
                && unit_has_source_span_in_current_fragmentainer)
                || unit.block_size().points() > self.page_area_height() + 0.01
                || (physical_flex_direction(style).is_column_axis()
                    && style.flex_wrap.wraps()
                    && flex_layout.lines.len() > 1);
            let atomic_zero_capacity_column_unit = fragmentainer_kind == FragmentainerKind::Column
                && current_fragmentainer.fragmentainer_block_size().points()
                    <= css::CSS_PX_TO_PT + 0.01
                && !unit.item_indices.is_empty()
                && unit.item_indices.iter().all(|&index| {
                    let child = &children[index];
                    (child.table_fragment().is_none()
                        && child.element_parts().is_some_and(|(_, _, boxes)| {
                            boxes.is_none_or(|boxes| boxes.is_empty())
                        }))
                        || child
                            .anonymous_content()
                            .is_some_and(|boxes| boxes.is_empty())
                });
            let prebreak_decision =
                FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
                    fragmentainer_kind,
                    // The one-CSS-pixel column capacity is a progress guard,
                    // not an authored fragmentainer edge. An atomic flex
                    // item/line in such a column overflows the originating
                    // column; repeatedly prebreaking it would move it to a
                    // different source column and then clip it to that
                    // numerical slack.
                    // <https://www.w3.org/TR/css-break-3/#breaking-rules>
                    break_is_applicable: break_is_applicable
                        && !(atomic_zero_capacity_column_unit && unit_index == 0),
                    // Atomic zero-capacity subjects overflow as whole boxes;
                    // their numerical progress size must not classify a
                    // later sibling as a sliceable oversized range and keep
                    // it in the originating column.
                    unit_is_oversized: unit_is_oversized && !atomic_zero_capacity_column_unit,
                    has_prior_unit: unit_index > 0,
                    has_later_unit: unit_index + 1 < break_units.len(),
                    cursor: fragment_cursor,
                    unit_block_start: unit.block_start,
                    unit_block_end: unit.block_end,
                    current_fragmentainer,
                    break_opportunity: FragmentBreakOpportunity::before_box_boundary(
                        fragmentainer_kind,
                        unit.block_start.points(),
                        break_context,
                        previous_break_after,
                        unit.break_inside_avoid || distributed_cross_axis_lines,
                    ),
                    can_advance: fragmentainer_kind == FragmentainerKind::Column
                        || !self.cursor_is_at_page_top()
                        || self.current_page_has_content(),
                });
            if let Some(transition) = prebreak_decision.transition_before_unit
                && let Some(content_top) = self.materialize_fragmentainer_advance(
                    transition.fragmentainer_kind,
                    FragmentainerAdvance::Unforced,
                )
            {
                fragment_cursor = transition
                    .cursor_after_fragmentainer_advance(PageTopBlockPosition::new(content_top));
            }

            let mut slice_start = unit.block_start;
            loop {
                let current_slice_fragmentainer =
                    self.fragmentainer_from_page_cursor(fragment_cursor.content_top);
                let final_fragment_end_reservation = if physical_flex_direction(style)
                    .is_column_axis()
                    && unit_index + 1 == break_units.len()
                    && style.box_decoration_break == css::BoxDecorationBreak::Slice
                {
                    style.padding.bottom + border_widths.bottom
                } else {
                    0.0
                };
                let available_block_end = if break_is_applicable {
                    let mut available_block_end = flex_source_block_end_after_available_capacity(
                        fragment_cursor.block_offset,
                        current_slice_fragmentainer,
                    );
                    // The final source unit owns this fragment's block-end
                    // border and padding under `box-decoration-break: slice`.
                    // Reserve that edge before testing whether the unit fits;
                    // otherwise a last column item can consume the exact
                    // remaining content capacity and leave no destination
                    // fragment for the required principal-box end.
                    // <https://www.w3.org/TR/css-break-3/#break-decoration>
                    if final_fragment_end_reservation > 0.0 {
                        available_block_end = FlexFragmentBlockOffset::new(
                            (available_block_end.points() - final_fragment_end_reservation)
                                .max(fragment_cursor.block_offset.points()),
                        );
                    }
                    available_block_end
                } else {
                    unit.block_end
                };
                let remaining_unit_size = unit.block_end.points() - slice_start.points();
                let remaining_capacity = available_block_end.points() - slice_start.points();
                let fits_empty_after_final_decoration = remaining_unit_size
                    <= current_slice_fragmentainer
                        .fragmentainer_block_size()
                        .points()
                        - final_fragment_end_reservation
                        + 0.01;
                if break_is_applicable
                    && final_fragment_end_reservation > 0.0
                    && remaining_unit_size > remaining_capacity + 0.01
                    && fits_empty_after_final_decoration
                {
                    let transition = FlexFragmentTransitionDecision {
                        fragmentainer_kind,
                        reason: FlexFragmentBreakReason::OverflowOrAvoid,
                        next_block_offset: slice_start,
                    };
                    if let Some(content_top) = self.materialize_fragmentainer_advance(
                        transition.fragmentainer_kind,
                        FragmentainerAdvance::Unforced,
                    ) {
                        fragment_cursor = transition.cursor_after_fragmentainer_advance(
                            PageTopBlockPosition::new(content_top),
                        );
                    }
                    continue;
                }
                let slice_decision = FlexUnitSliceDecision::choose(FlexUnitSliceDecisionInput {
                    fragmentainer_kind,
                    break_is_applicable,
                    // CSS Flexbox fragments at flex-line (row) or item
                    // (column) boundaries. A unit may be sliced only when
                    // it cannot fit even in an empty fragmentainer; otherwise
                    // an overflowing unit advances whole to the next one.
                    // https://www.w3.org/TR/css-flexbox-1/#pagination
                    can_slice_at_fragmentainer_boundary: !distributed_cross_axis_lines
                        && !atomic_zero_capacity_column_unit
                        && (unit.block_end.points() > available_block_end.points() + 0.01
                            || unit.block_size().points() > self.page_area_height() + 0.01
                            // A wrapped column unit is a partition of the
                            // shared physical main-axis interval, not an
                            // atomic flex line. Items active in that interval
                            // may already continue from an earlier interval,
                            // so the partition itself must be allowed to
                            // cross the remaining fragmentainer boundary.
                            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                            || (physical_flex_direction(style).is_column_axis()
                                && style.flex_wrap.wraps()
                                && flex_layout.lines.len() > 1)),
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
                        fragment_cursor = transition.cursor_after_fragmentainer_advance(
                            PageTopBlockPosition::new(content_top),
                        );
                    }
                    continue;
                }
                debug_assert!(slice_decision.paints_slice());
                let committed_slice_start = slice_decision.slice_start;
                let slice_end = slice_decision.slice_end;
                let slice_unit = unit.slice(committed_slice_start, slice_end);
                let materialized_fragmentainer =
                    self.fragmentainer_from_page_cursor(fragment_cursor.content_top);
                let fragment_context = FlexFragmentBuildContext {
                    page_index: self.pages.len(),
                    outer_inline_span: PageInlineSpan::new(outer_x, outer_width),
                    content_top: fragment_cursor.content_top,
                    block_offset: fragment_cursor.block_offset,
                    first_fragmentainer_capacity: materialized_fragmentainer.available_block_size(),
                    continuation_fragmentainer_capacity: materialized_fragmentainer
                        .fragmentainer_block_size(),
                    // Root/canvas continuation replay can paint structural
                    // primitives on a fresh page before this flex fragment.
                    // That paint is not preceding flow content, so it must
                    // not make the fragment ineligible for GCPM `start`.
                    starts_page_fragment: !self.current_page_has_flow_content,
                };
                let mut planned_fragment = flex_fragment_from_break_unit(
                    &slice_unit,
                    &flex_layout,
                    fragment_context,
                    break_is_applicable,
                );
                // A row-flex line that continues in a later fragmentainer
                // re-establishes its resolved cross size there. Its child
                // content is clipped to the committed source slice, but the
                // container fragment and the line's item decorations own the
                // complete line interval in that destination fragmentainer.
                // Keeping only the trailing source remainder makes the
                // continuation border stop at the slice boundary instead of
                // enclosing the re-established line.
                // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                // <https://www.w3.org/TR/css-break-3/#box-splitting>
                let row_line_reestablishes_cross_size = physical_flex_direction(style)
                    .is_row_axis()
                    && committed_slice_start.points() > unit.block_start.points() + 0.01
                    && committed_slice_start.points() < unit.block_end.points() - 0.01;
                if row_line_reestablishes_cross_size
                    && let Some(content_bounds) = planned_fragment.metadata.source_border_box
                {
                    let line_size = unit.block_size().points();
                    planned_fragment.metadata.source_border_box = Some(PaintClip::new(
                        content_bounds.x(),
                        fragment_cursor.content_top.points() - line_size,
                        content_bounds.width(),
                        line_size,
                    ));
                }
                // A flex container fragment ends at a fragmentainer boundary
                // both for an explicit forced break and when the following
                // whole flex unit must advance there. Its box decorations are
                // painted over that complete box fragment, rather than only
                // the preceding line's source extent. This is particularly
                // visible when an unfragmentable row moves to the next
                // column.
                // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                // <https://www.w3.org/TR/css-break-3/#box-splitting>
                let unit_finishes_here = slice_end.points() >= unit.block_end.points() - 0.01;
                let consumed_fragmentainer_space =
                    slice_end.points() - fragment_cursor.block_offset.points();
                let remaining_fragmentainer_space =
                    materialized_fragmentainer.available_block_size().points()
                        - consumed_fragmentainer_space;
                let next_unit_advances = break_units.get(unit_index + 1).is_some_and(|next_unit| {
                    let next_unit_owns_fragment_end = physical_flex_direction(style)
                        .is_column_axis()
                        && unit_index + 2 == break_units.len()
                        && style.box_decoration_break == css::BoxDecorationBreak::Slice;
                    let next_unit_required_space = next_unit.block_end.points()
                        - slice_end.points()
                        + if next_unit_owns_fragment_end {
                            style.padding.bottom + border_widths.bottom
                        } else {
                            0.0
                        };
                    fragmentainer_kind.is_forced_break(next_unit.break_before)
                            // The following unit may have a source gap before it.
                            // Its complete span, including the destination
                            // fragment's owned block-end decoration, determines
                            // whether it fits in this fragmentainer.
                            || next_unit_required_space
                                > remaining_fragmentainer_space + 0.01
                });
                let trailing_auto_fragment_fill = auto_item_fragmentation_expands_container
                    && unit_finishes_here
                    && break_units.get(unit_index + 1).is_none()
                    // A column item's final span belongs to its first/only
                    // unit slice. A row line with nested fragmentable
                    // overflow reaches that same final span through a later
                    // continuation slice, so do not discard its committed
                    // destination fragmentainer merely because its source
                    // offset is non-zero.
                    && (physical_flex_direction(style).is_row_axis()
                        || (committed_slice_start - unit.block_start).abs() <= 0.01)
                    && remaining_fragmentainer_space > 0.01;
                if unit_finishes_here
                    && (force_after_unit || next_unit_advances)
                    && slice_end.points() < total_content_height - 0.01
                    && let Some(content_bounds) = planned_fragment.metadata.source_border_box
                {
                    let fragment_extent = materialized_fragmentainer.available_block_size();
                    planned_fragment.metadata.source_border_box = Some(PaintClip::new(
                        content_bounds.x(),
                        fragment_cursor.content_top.points() - fragment_extent.points(),
                        content_bounds.width(),
                        fragment_extent.points(),
                    ));
                }
                if trailing_auto_fragment_fill
                    && let Some(content_bounds) = planned_fragment.metadata.source_border_box
                {
                    let fragment_extent = materialized_fragmentainer.available_block_size();
                    planned_fragment.metadata.source_border_box = Some(PaintClip::new(
                        content_bounds.x(),
                        fragment_cursor.content_top.points() - fragment_extent.points(),
                        content_bounds.width(),
                        fragment_extent.points(),
                    ));
                }
                let final_source_unit_finishes = unit_index + 1 == break_units.len()
                    && slice_end.points() >= unit.block_end.points() - 0.01;
                if let Some(content_bounds) = planned_fragment.metadata.source_border_box {
                    let clone_decorations =
                        style.box_decoration_break == css::BoxDecorationBreak::Clone;
                    planned_fragment.metadata.source_border_box =
                        Some(flex_container_fragment_border_box(
                            content_bounds,
                            clone_decorations || committed_slice_start.points() <= 0.01,
                            clone_decorations || final_source_unit_finishes,
                            layout_pt(border_widths.top + style.padding.top),
                            layout_pt(style.padding.bottom + border_widths.bottom),
                        ));
                }
                let decoration = FlexFragmentDecorationState {
                    includes_block_start: style.box_decoration_break
                        == css::BoxDecorationBreak::Clone
                        || committed_slice_start.points() <= 0.01,
                    includes_block_end: style.box_decoration_break
                        == css::BoxDecorationBreak::Clone
                        || final_source_unit_finishes,
                };
                let destination_border_box = planned_fragment.metadata.source_border_box;
                let mut materialized_fragment = MaterializedFlexFragment::new(
                    planned_fragment,
                    destination_border_box,
                    decoration,
                    PaintTranslation::identity(),
                );
                debug_assert_eq!(
                    materialized_fragment.source_bounds.start(),
                    committed_slice_start,
                    "a committed flex fragment must retain its selected source start",
                );
                debug_assert_eq!(
                    materialized_fragment.source_bounds.end(),
                    slice_end,
                    "a committed flex fragment must retain its selected source end",
                );
                debug_assert_eq!(
                    materialized_fragment.decoration.includes_block_start,
                    style.box_decoration_break == css::BoxDecorationBreak::Clone
                        || committed_slice_start.points() <= 0.01,
                );
                debug_assert_eq!(
                    materialized_fragment.decoration.includes_block_end,
                    style.box_decoration_break == css::BoxDecorationBreak::Clone
                        || final_source_unit_finishes,
                );
                let planned_fragment = &mut materialized_fragment.layout;
                flex_layout
                    .fragment_plan
                    .prepare_materialized_fragment(&mut *planned_fragment);
                // A materialized flex fragment is a real fragmentainer
                // occupant even when it has no in-flow item paint. In
                // particular, an empty fixed-size flex container can carry
                // only its own background or border through intermediate
                // columns. Mark that structural occupancy before a following
                // continuation calls `push_page`; otherwise the temporary
                // column cursor replaces this page and drops the fragment.
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                if materialized_fragment.destination_border_box.is_some() {
                    self.mark_current_page_flow_content();
                }
                let mut materialized_items = Vec::with_capacity(planned_fragment.items.len());
                for item_fragment in &mut planned_fragment.items {
                    let index = item_fragment.item_index;
                    let child = &children[index];
                    if flex_item_is_collapsed(&child.style) {
                        continue;
                    }
                    let item = &item_fragment.bounds;
                    // The source rectangle can be extended through
                    // descendant overflow. Materialize the frozen used
                    // geometry with the destination slice so all subsequent
                    // replay reads come from one committed record.
                    let used_item = &item_fragment.used_bounds;
                    let replay_dimensions = used_item.replay_dimensions();
                    let item_width = replay_dimensions.border_box_width();
                    let item_x = used_item.x().points();
                    let item_y = used_item.y().points();
                    let item_height = replay_dimensions.border_box_height();
                    let item_continues_from_previous_fragment =
                        item_fragment.content_slice.block_start.points() > 0.01;
                    let item_continues_to_next_fragment =
                        item_fragment.content_slice.block_end.points()
                            < item_fragment.source_bounds.height().points() - 0.01;
                    let item_is_source_fragment =
                        item_continues_from_previous_fragment || item_continues_to_next_fragment;
                    let visible_item_height = if physical_flex_direction(style).is_row_axis()
                        && item_continues_from_previous_fragment
                        && row_flex_item_stretches_in_fragment(&child.style, style)
                    {
                        // A stretched row-flex item re-establishes its
                        // cross-size alignment in every continuation. Its
                        // source slice can be shorter than the remaining
                        // fragmentainer, but the fragment-local border box
                        // occupies that remaining span.
                        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                        let consumed_before_item =
                            (item.y().points() - fragment_cursor.block_offset.points()).max(0.0);
                        item.height().points().max(
                            materialized_fragmentainer.available_block_size().points()
                                - consumed_before_item,
                        )
                    } else if item_is_source_fragment {
                        // `bounds` is the fragmentainer-selected source
                        // range. The frozen `used_bounds` deliberately
                        // retains the whole source border box for replay, so
                        // it cannot define this page-local paint clip: doing
                        // so would paint and clip every continuation as the
                        // full item height. Keep overflow in this selected
                        // range so descendant replay has the same
                        // fragmentainer clip as the principal box.
                        // <https://drafts.csswg.org/css-flexbox-1/#algo-cross-item>
                        // <https://drafts.csswg.org/css-break-3/#box-splitting>
                        item.height().points()
                    } else {
                        // A descendant's visible overflow can make the
                        // source interval taller than the flex item's used
                        // border box without creating a continuation. The
                        // source interval is retained for descendant replay,
                        // but the unsplit principal box and its background
                        // must remain the resolved Flexbox size.
                        // <https://www.w3.org/TR/css-flexbox-1/#flex-items>
                        item_height.points()
                    };
                    let item_content_left = inner_x + item_x;
                    let item_page_index = self.pages.len();
                    let mut item_cursor_y = fragment_cursor.content_top.points()
                        - (item_y - fragment_cursor.block_offset.points());
                    if let Some(Some((source_block_end, destination_block_end))) =
                        replayed_child_destination_ends.get(item_page_index)
                        && item_y >= *source_block_end - 0.01
                    {
                        item_cursor_y = item_cursor_y.min(*destination_block_end);
                    }
                    // The flex container has already claimed structural
                    // fragmentainer occupancy before its items are replayed.
                    // That bookkeeping must not hide the first committed item
                    // from GCPM `start`: determine the item boundary from the
                    // fragment plan captured before that occupancy, and only
                    // let the first visible flex item inherit it.
                    // <https://www.w3.org/TR/css-gcpm-3/#named-strings>
                    let item_starts_page_fragment = planned_fragment.metadata.starts_page_fragment
                        && materialized_items.is_empty();
                    let visible_item_top = fragment_cursor.content_top.points()
                        - (item.y().points() - fragment_cursor.block_offset.points());
                    let item_page_border_box = PageTopRect::new(
                        item_content_left,
                        visible_item_top,
                        item_width.points(),
                        visible_item_height,
                    );
                    let item_border_box = item_page_border_box.paint_clip();
                    let materialized_item = MaterializedFlexItemFragment::from_planned(
                        item_fragment,
                        item_border_box,
                        materialized_fragment.local_to_page_translation,
                    );
                    // All replay geometry below comes from the committed
                    // item record.  Do not rebuild a local border box from
                    // source coordinates in the split/non-split branches.
                    let item_border_box = materialized_item.local_border_box;
                    let replay_item = &materialized_item.replay_bounds;
                    materialized_items.push(materialized_item.clone());
                    let mut item_metadata = FragmentPageMetadata::new(
                        item_page_index,
                        Some(item_border_box),
                        item_starts_page_fragment,
                    );
                    item_metadata.continues_from_previous_page =
                        item_continues_from_previous_fragment;
                    item_metadata.continues_to_next_page = item_continues_to_next_fragment;
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
                        PhysicalFlexDirection::new(physical_flex_direction(style)),
                    );

                    // Flex item layout owns the principal box geometry. Lay
                    // out that principal decoration from the frozen final
                    // border box before replaying the item's independent
                    // formatting context, which only supplies descendant
                    // content and effects. This is essential for empty flex
                    // items, and keeps orthogonal item backgrounds in the
                    // physical rectangle selected by Flexbox rather than a
                    // block-flow reconstruction.
                    // <https://www.w3.org/TR/css-flexbox-1/#painting>
                    // <https://www.w3.org/TR/css-flexbox-1/#flex-items>
                    // Replaced elements own their concrete-object and
                    // principal-box decoration in their dedicated replay
                    // path. Prepainting them here would duplicate the
                    // background and border (notably dotted borders), unlike
                    // the equivalent float/normal-flow box.
                    // <https://www.w3.org/TR/css-display-3/#replaced-element>
                    let child_is_replaced = child
                        .element_parts()
                        .is_some_and(|(element, _, _)| is_replaced_element(element));
                    // Tables and replaced elements retain dedicated principal
                    // box replay paths. Ordinary unsplit flex items instead
                    // use the resolved flex border box below, then replay only
                    // their descendant formatting context.
                    // A source-split item still owns its resolved principal
                    // box in every committed fragment. Its nested replay is
                    // clipped descendant content; it does not reconstruct
                    // the flex item's background or border. Omitting this
                    // paint for a split item drops otherwise atomic item
                    // decorations (notably a short colored item beside a
                    // taller overflowing sibling in a multicolumn row).
                    // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                    // <https://www.w3.org/TR/css-break-3/#box-splitting>
                    let flex_replay_owns_principal_decoration =
                        !child_is_replaced && !child.style.display.is_table();
                    if flex_replay_owns_principal_decoration
                        && placed_style.visibility == Visibility::Visible
                        && (placed_style.background_color.is_some()
                            || placed_style.background_image.is_image()
                            || placed_style.border_image.source.is_image()
                            || used_border_width(&placed_style) > layout_pt(0.0))
                    {
                        let mut principal_fragment =
                            PaintFragment::from_primitives(Vec::new(), Vec::new());
                        principal_fragment.prepend_primitives_in_band(
                            PaintBand::BackgroundBorder,
                            self.box_background_primitives(
                                item_page_border_box.paint_rect(),
                                &placed_style,
                            ),
                        );
                        // The child replay may restore a speculative paint
                        // checkpoint. Record the resolved flex principal box
                        // in the page's background-band prefix so that
                        // restoration cannot discard its committed owner
                        // decoration, while the replayed descendant still
                        // paints above it.
                        self.current_page.prepend_paint_fragment_owned(
                            principal_fragment,
                            PaintTranslation::identity(),
                        );
                    }

                    let item_was_split = self.with_formatting_context_item_placement(
                        FormattingContextItemPlacement {
                            content_left: item_content_left,
                            content_width: replay_dimensions.available_width_for_replay(),
                            content_height: Some(replay_dimensions.available_height_for_replay()),
                            table_wrapper_border_box_block_size:
                                auto_table_wrapper_block_size_override(&child.style, item_height),
                            writing_mode: placed_style.writing_mode,
                            // Anonymous flex items have no principal element
                            // through which block layout can establish their
                            // definite formatting-context inline size. Scope
                            // their assigned width for line construction so
                            // text wraps against the final flex-item size.
                            // <https://www.w3.org/TR/css-flexbox-1/#flex-items>.
                            scope_content_logical_inline_size: child.anonymous_content().is_some(),
                            cursor_y: item_cursor_y,
                            page_start_margin_policy: PageStartMarginPolicy::Preserve,
                        },
                        &placed_style,
                        |layout| {
                            if item_is_split {
                                let item_top = PageTopBlockPosition::new(layout.cursor_y);
                                let mut replay_destination_block_end = None;
                                layout.paint_split_flex_item_fragment(
                                    child,
                                    &placed_style,
                                    stylesheets,
                                    SplitFlexItemPaintContext {
                                        item_width,
                                        item_height,
                                        percentage_height_basis: replay_item
                                            .percentage_height_basis,
                                        slice_border_box: item_border_box,
                                        source_item_top: item_top,
                                        continuation: materialized_item.continuation,
                                        // A row or wrapped flex item keeps one source layout
                                        // across continuations. The same is true for an
                                        // auto-height single-line column container instead
                                        // advances its source item anchor for each selected
                                        // slice. Applying the item-local offset a second time
                                        // would replay its preceding content on later pages.
                                        // A definite-height single-line column container also
                                        // establishes a fragment-local main-size replay.
                                        // <https://www.w3.org/TR/css-break-3/#box-splitting>
                                        replay_source_slice_offset: physical_flex_direction(style)
                                            .is_row_axis()
                                            || style.flex_wrap.wraps(),
                                        has_descendant_source_overflow: item_fragment
                                            .source_bounds
                                            .height()
                                            .points()
                                            > item_fragment.used_bounds.height().points() + 0.01,
                                        positioning_containing_block:
                                            establishes_positioning_containing_block.then(|| {
                                                ContainingBlock::from_page_top_rect(
                                                    PageTopRect::new(
                                                        outer_x + border_widths.left,
                                                        fragment_cursor.content_top.points()
                                                            + positioning_containing_block_offset,
                                                        content_width_points
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
                                                    fragment_cursor.content_top.points()
                                                        + positioning_containing_block_offset,
                                                    outer_width,
                                                    (slice_end - committed_slice_start)
                                                        .non_negative_size()
                                                        .points(),
                                                )
                                                .paint_clip()
                                            }),
                                    },
                                    inline_flex::SplitFlexItemReplayState {
                                        fragments: &mut split_item_replay_fragments[index],
                                        local_block_ends:
                                            &mut split_item_replay_fragment_local_block_ends[index],
                                        table_fragment_bottoms:
                                            &mut split_item_replay_fragment_bottoms,
                                        destination_block_end: &mut replay_destination_block_end,
                                    },
                                );
                                if item_continues_from_previous_fragment
                                    && let Some(destination_block_end) =
                                        replay_destination_block_end
                                {
                                    if replayed_child_destination_ends.len() <= item_page_index {
                                        replayed_child_destination_ends
                                            .resize(item_page_index + 1, None);
                                    }
                                    replayed_child_destination_ends[item_page_index] = Some((
                                        item_y + item_height.points(),
                                        destination_block_end,
                                    ));
                                }
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
                            let previous_suppress_principal_box_decoration =
                                layout.suppress_next_principal_box_decoration;
                            layout.suppress_next_principal_box_decoration =
                                flex_replay_owns_principal_decoration;
                            layout.layout_flex_item_contents(
                                child,
                                &placed_style,
                                stylesheets,
                                replay_item.percentage_height_basis,
                            );
                            layout.suppress_next_principal_box_decoration =
                                previous_suppress_principal_box_decoration;
                            item_metadata.assignment_ids = layout.end_assignment_capture_frame();
                            if !item_metadata.assignment_ids.is_empty() {
                                layout.update_named_assignment_placements(
                                    &item_metadata.assignment_ids,
                                    item_metadata.assignment_placement(),
                                );
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
                                    &policy,
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
                if let Some(Some(child_bottom)) =
                    split_item_replay_fragment_bottoms.get(materialized_fragment.layout.page_index)
                    && let Some(fragment_bounds) = materialized_fragment.destination_border_box
                    && *child_bottom > fragment_bounds.y() + 0.01
                    && *child_bottom < fragment_bounds.y() + fragment_bounds.height() - 0.01
                {
                    // A nested table can finish a committed flex source slice
                    // before the provisional source range used to reserve its
                    // fragmentainer. The child owns the final visible
                    // continuation edge, so the container must not manufacture
                    // another `slice` end border at the old source estimate.
                    materialized_fragment.destination_border_box = Some(PaintClip::new(
                        fragment_bounds.x(),
                        *child_bottom,
                        fragment_bounds.width(),
                        fragment_bounds.y() + fragment_bounds.height() - *child_bottom,
                    ));
                    materialized_fragment.decoration.includes_block_end = false;
                }
                materialized_fragment.item_fragments = materialized_items;
                flex_layout
                    .fragment_plan
                    .push_materialized_fragment(materialized_fragment);
                if trailing_auto_fragment_fill {
                    total_content_height += remaining_fragmentainer_space;
                }
                if slice_end.points() >= unit.block_end.points() - 0.01 {
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
                    fragment_cursor = transition
                        .cursor_after_fragmentainer_advance(PageTopBlockPosition::new(content_top));
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

        let positioned_source_fragment_block_offset = flex_layout
            .fragment_plan
            .materialized_fragments
            .iter()
            .find(|fragment| fragment.page_index == self.pages.len())
            .map(|fragment| fragment.source_bounds.start())
            .unwrap_or_else(|| FlexFragmentBlockOffset::new(0.0));
        let first_fragment_source_block_size = flex_layout
            .fragment_plan
            .materialized_fragments
            .first()
            .map(|fragment| fragment.source_bounds.size())
            .unwrap_or_else(|| FlexFragmentBlockSize::new(0.0));
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
                        height: flex_available_content_height.map(PhysicalContentHeight::new),
                        height_basis: definite_content_height
                            .map(|height| {
                                PercentageBasis::definite_from(
                                    height,
                                    FlexAvailableSizeSource::ContainingBlock,
                                )
                            })
                            .unwrap_or_else(PercentageBasis::indefinite),
                    },
                    inner_inline_span: PageInlineSpan::new(inner_x, inner_width),
                    content_top: PageTopBlockPosition::new(content_top),
                    source_fragment_block_offset: positioned_source_fragment_block_offset,
                    first_fragment_source_block_size,
                },
            );
        }
        self.pop_overflow_clip(overflow_clip_active);
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.containing_block_direction = previous_containing_block_direction;
        self.containing_block_writing_mode = previous_containing_block_writing_mode;

        self.cursor_y = fragment_cursor
            .content_top
            .toward_block_end(layout_pt(
                total_content_height - fragment_cursor.block_offset.points(),
            ))
            .points();
        if let Some(Some(child_bottom)) = split_item_replay_fragment_bottoms.get(self.pages.len()) {
            self.cursor_y = self.cursor_y.max(*child_bottom);
        }
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
                content_width_points + style.padding.left + style.padding.right,
                total_content_height + style.padding.top + style.padding.bottom,
            )
            .paint_clip()
        });
        if let Some(container_clip) = contents_overflow_clip {
            for fragment in &mut flex_layout.fragment_plan.materialized_fragments {
                fragment.contents_overflow_clip = fragment
                    .destination_border_box
                    .and_then(|fragment_bounds| container_clip.intersect(fragment_bounds));
            }
        }
        if block_height > 0.0 {
            self.mark_current_page_flow_content();
        }
        let background_page_index = self.pages.len();
        let mut own_background_primitives = Vec::new();
        let mut own_outline_primitives = Vec::new();
        if !suppress_own_principal_box_decoration
            && self.element_propagates_document_canvas_properties(element, style)
        {
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
            // CSS Backgrounds propagates the root background to the canvas,
            // but the root's border remains a principal-box decoration.
            // Flex-root layout must therefore retain border paint locally
            // instead of treating canvas propagation as decoration
            // suppression.
            // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
            if block_height > 0.0
                && (style.border_image.source.is_image()
                    || used_border_width(style) > layout_pt(0.0))
                && style.visibility == Visibility::Visible
            {
                let mut border_style = style.clone();
                border_style.background_color = None;
                border_style.background_image = css::ComputedImage::None;
                border_style.background_layers.clear();
                own_background_primitives = self.box_background_primitives(
                    paint_space_rect(outer_x, block_bottom, outer_width, block_height),
                    &border_style,
                );
            }
        } else if !suppress_own_principal_box_decoration
            && block_height > 0.0
            && (style.background_color.is_some()
                || style.background_image.is_image()
                || style.border_image.source.is_image()
                || used_border_width(style) > layout_pt(0.0))
            && style.visibility == Visibility::Visible
        {
            own_background_primitives = self.box_background_primitives(
                paint_space_rect(outer_x, block_bottom, outer_width, block_height),
                style,
            );
        }
        if !suppress_own_principal_box_decoration
            && block_height > 0.0
            && style.visibility == Visibility::Visible
        {
            let gap_gutters = flex_gap_decoration_gutters(
                &flex_layout,
                style,
                PhysicalContentWidth::new(content_box_pt(inner_width)),
                PhysicalContentHeight::new(content_box_pt(total_content_height)),
            );
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
            // Overflow effects belong to the committed flex fragment just as
            // its background and border do. Applying the container-wide clip
            // only to its initial page lets later continuations paint outside
            // their fragmentainer; intersecting with the materialized border
            // range keeps the padding-box clip local without changing any
            // item source interval.
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
            // <https://www.w3.org/TR/css-break-3/#box-splitting>
            let fragment_contents_overflow_clip = contents_overflow_clip.and_then(|clip| {
                if flex_fragmentation_enabled || flex_spanned_pages {
                    flex_container_page_contents_overflow_clip(
                        &flex_layout.fragment_plan,
                        page_index,
                    )
                    .or_else(|| {
                        flex_spanned_pages.then(|| {
                            fragment.bounds().and_then(|bounds| {
                                clip.intersect(PaintClip::from_paint_rect(paint_space_rect(
                                    outer_x,
                                    bounds.y(),
                                    outer_width,
                                    bounds.height(),
                                )))
                            })
                        })?
                    })
                } else {
                    (page_index == paint_page_index).then_some(clip)
                }
            });
            if let Some(clip) = fragment_contents_overflow_clip {
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
                    let (owns_block_start, owns_block_end) = flex_layout
                        .fragment_plan
                        .materialized_fragments
                        .iter()
                        .filter(|record| record.layout.page_index == page_index)
                        .fold((false, false), |(owns_start, owns_end), record| {
                            (
                                owns_start || record.decoration.includes_block_start,
                                owns_end || record.decoration.includes_block_end,
                            )
                        });
                    let mut fragment_style = style.clone();
                    suppress_fragmented_box_edges(
                        &mut fragment_style,
                        owns_block_start,
                        owns_block_end,
                    );
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
                            &fragment_style,
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
                                FlexGapDecorationFragmentContext {
                                    page_index,
                                    content_inline_span: PageInlineSpan::new(inner_x, inner_width),
                                    content_height: PhysicalContentHeight::new(content_box_pt(
                                        total_content_height,
                                    )),
                                    fragment_bounds,
                                    has_forced_item_breaks: flex_has_forced_item_breaks,
                                },
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
                            &fragment_style,
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
