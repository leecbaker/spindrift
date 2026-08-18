use super::*;
use crate::document::paint::geometry::AxisSelectivePaintClip;
use crate::layout::block::BlockLayoutInlineConstraint;
use crate::layout::block::suppress_fragmented_box_edges;
use crate::units::Definite;

/// Resolve Flexbox's scrollport reservation for Quire's static PDF user
/// agent.
///
/// PDF output has no native interactive scrollbar chrome, so even
/// `overflow: scroll` retains the padding-box scrollport rather than
/// reserving classic gutter space. The same policy is used by Grid.
/// <https://drafts.csswg.org/css-overflow-3/#scrollbars-layout>
fn flex_scrollbar_reservation(_: &ComputedStyle) -> ScrollbarGutterReservation {
    ScrollbarGutterReservation::static_pdf_overlay()
}

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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        self.layout_flex_with_descendant_percentage_height_basis_request(
            element,
            style,
            stylesheets,
            child_boxes,
            descendant_percentage_height_basis
                .map(FlexDescendantPercentageHeightBasis::Override)
                .unwrap_or(FlexDescendantPercentageHeightBasis::DeriveFromContainer),
            principal_box_paint_mode,
        );
    }

    fn layout_flex_with_descendant_percentage_height_basis_request(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: FlexDescendantPercentageHeightBasis,
        principal_box_paint_mode: PrincipalBoxPaintMode,
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
        let containment = used_property_containment(element, style);
        let source_style = style;
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let mut used_style =
            FlexUsedStyle::from_normalized(self.style_with_current_used_lengths(style));
        let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            self.current_content_logical_inline_percentage_basis(),
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
        self.resolve_styled_children_used_lengths(&mut children);
        self.resolve_styled_children_used_lengths(&mut positioned_children);

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
                        candidate_geometry
                            .float_avoidance_candidate(border_box_pt(border_box_height))
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

        let orthogonal_auto_width = orthogonal_auto_width_flex_container_needs_intrinsic(
            &used_style,
            self.current_child_available_space(),
        );
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
        // An orthogonal normal-flow flex root resolves its automatic physical
        // width with the Writing Modes fit-content rule.  The intrinsic
        // adapter above has already performed that resolution; feeding the
        // result through the horizontal CSS 2 block-width equation would
        // incorrectly replace it with the containing block's full physical
        // width.
        // <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto>
        let resolved_normal_flow_width =
            (used_style.float == Float::None && !orthogonal_auto_width).then(|| {
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
        // A block-axis intrinsic size is a used container size, not merely
        // an estimate for its parent. Resolve it before the final Flexbox
        // pass so the flexible-length algorithm sees the selected
        // min-/max-content main size. Leaving this as `None` asks Taffy for
        // max-content layout and lets a fixed flex basis escape a
        // `height:min-content` container.
        //
        // This deliberately keeps the resulting percentage basis separate
        // until `definite_flex_container_content_height` classifies it below.
        // <https://www.w3.org/TR/css-sizing-3/#sizing-values>
        // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>
        let intrinsic_content_height = (!containment.size
            && needs_intrinsic_height_contribution(style.box_values.height.value().clone()))
        .then(|| {
            self.estimate_intrinsic_flex_container_size(
                &children,
                style,
                stylesheets,
                FlexAvailableSpace {
                    width: content_width,
                    width_basis: flex_available_percentage_basis_from_points(
                        Some(content_width.points()),
                        FlexAvailableSizeSource::IntrinsicContainerSize,
                    ),
                    height: None,
                    height_basis: PercentageBasis::indefinite(),
                },
            )
        })
        .and_then(|intrinsic| {
            crate::layout::intrinsic::intrinsic_content_box_height_keyword(
                style.box_values.height.value().clone(),
                intrinsic.min_height,
                intrinsic.height,
                layout_pt(available_outer_height),
                vertical_non_content,
            )
        });
        let explicit_content_height = replayed_item_content_height
            .or_else(|| {
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
            })
            .or(intrinsic_content_height);
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
                .physical_height_percentage_basis(),
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
        let size_contained_content_height = containment.size.then(|| {
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
                .resolve_block_clearance(BlockClearanceRequest::coincident_edges(
                    style.clear,
                    style.writing_mode,
                    style.direction,
                    PageTopBlockPosition::new(self.cursor_y),
                ))
                .used_border_edge
                .points();
        }
        let block_top = self.cursor_y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let flex_start_page_context = self.current_page_context;
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
                principal_box_paint_mode,
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
        let late_calc_height = match &*style.box_values.height {
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
                    &*style.box_values.height
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
        let flex_has_forced_item_breaks = children.iter().any(|child| {
            let break_context = FragmentBreakContext::for_standalone_box(&child.style);
            break_context
                .forced_break_before_in(fragmentainer_kind)
                .is_some()
                || break_context
                    .forced_break_after_in(fragmentainer_kind)
                    .is_some()
        });
        // A forced break inside an item is owned by that item's formatting
        // context, not by a synthetic flex-item boundary.  It nevertheless
        // splits the flex container's principal decoration across the pages
        // that child layout commits.
        let flex_has_forced_descendant_breaks = children.iter().any(|child| {
            child.element_parts().is_some_and(|(_, _, boxes)| {
                boxes.is_some_and(|boxes| {
                    flex_item_contents_have_forced_break_in(boxes, fragmentainer_kind)
                })
            })
        });
        // Do not turn normal two-dimensional flex placement into synthetic
        // source fragments. In particular, wrapped physical columns overlap
        // on physical Y, while `column-reverse` still replays in
        // order-modified main-axis order. Actual and forced fragmentation
        // retain the physical boundary planner below.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        let mut break_units = if flex_has_forced_item_breaks || flex_has_forced_descendant_breaks {
            flex_container_break_units(
                fragmentainer_kind,
                &flex_layout,
                &children,
                style,
                false,
                layout_pt(total_content_height),
            )
        } else {
            vec![unfragmented_flex_container_break_unit(
                &flex_layout,
                &children,
                layout_pt(total_content_height),
            )]
        };
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
                principal_box_paint_mode,
            );
            return;
        }
        let defer_own_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = false;
        let suppress_own_principal_box_decoration = !principal_box_paint_mode.root_paints();
        // Flex containers do not pass through block-flow's principal-box
        // entry point, but an `id` on one is still a generated-content target
        // at its first fragment. CSS Generated Content Level 3 cross
        // references are independent of the target's formatting context.
        // <https://www.w3.org/TR/css-content-3/#cross-references>
        self.add_page_anchor(element, style);
        let content_top = self.cursor_y;
        // The first content cursor has already passed the initial border and
        // padding. Every cloned continuation reserves the same pair of
        // principal-box edges before flex content makes source progress.
        // Keep that capacity rule alongside decoration ownership so sizing,
        // source slicing, and destination painting cannot disagree.
        // <https://www.w3.org/TR/css-break-3/#breaks>
        // <https://www.w3.org/TR/css-break-3/#break-decoration>
        let fragment_decoration_reservation = FragmentDecorationReservation::new(
            FragmentDecoration::for_box_decoration_break(style.box_decoration_break, false, false),
            non_content_pt(border_widths.top + style.padding.top),
            non_content_pt(style.padding.bottom + border_widths.bottom),
        );
        // A cloned flex item has two block extents: its source content and
        // the larger destination sequence formed by repeating its own border
        // and padding in every fragmentainer. Expand the flex-line planner in
        // destination space now, while preserving the item-local source map
        // for replay below.
        // <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        let item_fragmentainer =
            self.fragmentainer_from_page_cursor(PageTopBlockPosition::new(content_top));
        let mut cloned_item_destination_height = total_content_height;
        let mut cloned_item_projection_changed = false;
        for item in &mut flex_layout.items {
            cloned_item_projection_changed |= item.project_cloned_fragment_destinations(
                item_fragmentainer.available_block_size(),
                item_fragmentainer.fragmentainer_block_size(),
            );
            cloned_item_destination_height = cloned_item_destination_height
                .max(item.y().points() + item.fragmentation_height().points());
        }
        if cloned_item_projection_changed {
            total_content_height = cloned_item_destination_height;
            flex_layout.height = PhysicalContentHeight::new(content_box_pt(total_content_height));
            for line in &mut flex_layout.lines {
                let line_destination_end = line
                    .item_indices
                    .iter()
                    .map(|&item_index| {
                        flex_layout.items[item_index].y().points()
                            + flex_layout.items[item_index]
                                .fragmentation_height()
                                .points()
                    })
                    .fold(line.cross_end.points(), f32::max);
                line.cross_end = FlexCrossOffset::new(line_destination_end);
            }
            total_height = border_widths.top
                + style.padding.top
                + total_content_height
                + style.padding.bottom
                + border_widths.bottom;
        }
        let flex_overflows_current_page = block_top - total_height < self.page_bottom() - 0.01;
        // A flex item's used size can fit while its normal-flow descendants
        // require a longer fragmentable source extent. That overflow is
        // recorded during flex measurement and must itself activate the flex
        // fragment plan unless the container's own overflow clip contains
        // it. A clipped scrollport owns the descendant paint locally; turning
        // that clipped ink into a flex continuation would replay it outside
        // the final used padding box.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        // <https://drafts.csswg.org/css-overflow-3/#overflow-clipping>
        let flex_used_overflow = self.used_overflow_axes_for_element(element, style);
        let flex_overflow_is_clipped = self.element_used_overflow_clips(element, style)
            && flex_overflow_is_clipped_in_fragmentation_axis(
                flex_used_overflow,
                paint_containment_applies_to_element(element, style),
            )
            || self.table_cell_context_clips_block_fragmentation();
        let flex_visible_overflow = (!flex_overflow_is_clipped)
            .then(|| {
                flex_visible_overflow(
                    &flex_layout,
                    &children,
                    style,
                    layout_pt(total_content_height),
                )
            })
            .flatten();
        let flex_item_overflows_container_source = flex_visible_overflow.is_some();
        // Overflow past a definite flex box is not itself pagination.  It
        // remains visual overflow in the current page/column unless its
        // source interval actually reaches the active fragmentainer edge.
        // <https://drafts.csswg.org/css-flexbox-1/#pagination>
        let flex_item_overflow_reaches_fragmentainer =
            flex_visible_overflow.is_some_and(|overflow| {
                overflow.reaches_fragmentainer(FlexFragmentBlockSize::new(
                    item_fragmentainer.available_block_size().points(),
                ))
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
            && !self.element_uses_document_canvas_flow(element)
            && (flex_overflows_current_page
                || flex_item_overflow_reaches_fragmentainer
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
                    fragment_decoration_reservation
                        .remaining_content_extent(
                            self.fragmentainer_from_page_cursor(PageTopBlockPosition::new(
                                content_top,
                            ))
                            .available_block_size(),
                        )
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
                    FlexFragmentBlockSize::new(
                        fragment_decoration_reservation
                            .fresh_content_extent(layout_pt(self.page_area_height()))
                            .points(),
                    ),
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
        // A flex item's used size and its fragmentable visual-overflow span
        // are distinct. A size-contained descendant can end halfway through
        // the final page, leaving the item's text and following normal-flow
        // sibling in that same fragmentainer. Do not round the container's
        // source size up to a page boundary: that turns unused visual tail
        // into principal-box ownership and manufactures an extra page.
        // `flex_item_block_bounds(..., true)` remains the durable source
        // span for materializing visual-overflow slices; it must not feed
        // back into the flex container's used block size.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
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
                .push(FragmentTopOffset::unreserved(
                    self.current_page_context.top() - content_top,
                ));
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

        let needs_contoured_overflow_clip = self.element_used_overflow_clips(element, style)
            && flex_used_overflow.clips_x()
            && flex_used_overflow.clips_y()
            && box_content_contour_is_non_rectangular(style);
        let overflow_clip_active =
            if self.element_used_overflow_clips(element, style) && !needs_contoured_overflow_clip {
                self.push_padding_box_overflow_clip(
                    element,
                    style,
                    Some(flex_scrollbar_reservation(style)),
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
        if flex_fragmentation_enabled || flex_has_forced_descendant_breaks {
            // Item replay is a formatting context rooted at the flex
            // container's content box. An independently forced child can
            // materialize a page even when no flex break unit was selected,
            // so it needs this context just as an ordinary flex continuation
            // does. Otherwise following flow on the child's destination page
            // incorrectly restores the outer page area instead of the
            // committed flex-content context.
            // <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
            // <https://www.w3.org/TR/css-break-3/#box-splitting>
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
        // An otherwise-unsplit item may still have a child-driven forced page
        // transition. Retain the child's final local endpoint separately from
        // the frozen flex-item border box: the latter is source geometry and
        // must not overwrite following-flow placement on the destination
        // page.
        let mut forced_child_fragment_ends = vec![None; children.len()];
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
                    fragment_cursor =
                        transition.cursor_after_fragmentainer_advance(PageTopBlockPosition::new(
                            content_top - fragment_decoration_reservation.block_start().points(),
                        ));
                }
            }
            let current_fragmentainer =
                self.fragmentainer_from_page_cursor(fragment_cursor.content_top);
            let current_content_capacity = fragment_decoration_reservation
                .remaining_content_extent(current_fragmentainer.available_block_size());
            let fresh_content_capacity = fragment_decoration_reservation
                .fresh_content_extent(current_fragmentainer.fragmentainer_block_size());
            // A unit that fits in a fresh fragmentainer moves at the flex
            // break boundary. Only a unit larger than a page (or an
            // overlapping wrapped-column partition) is sliced here.
            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
            let unit_is_oversized = unit.block_size().points()
                > fresh_content_capacity.points() + 0.01
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
                    available_content_block_size: current_content_capacity,
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
                fragment_cursor =
                    transition.cursor_after_fragmentainer_advance(PageTopBlockPosition::new(
                        content_top - fragment_decoration_reservation.block_start().points(),
                    ));
            }

            let mut slice_start = unit.block_start;
            loop {
                let current_slice_fragmentainer =
                    self.fragmentainer_from_page_cursor(fragment_cursor.content_top);
                // Descendant overflow extends an item's source replay range,
                // not the flex container's used block size. Reserving the
                // container's block-end decoration from that overflow range
                // shortens every final continuation and can manufacture an
                // otherwise empty extra column.
                let unit_carries_descendant_overflow =
                    unit.block_end.points() > total_content_height + 0.01;
                let final_fragment_end_reservation = if !unit_carries_descendant_overflow
                    && physical_flex_direction(style).is_column_axis()
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
                        fragment_decoration_reservation.remaining_content_extent(
                            current_slice_fragmentainer.available_block_size(),
                        ),
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
                    <= fragment_decoration_reservation
                        .fresh_content_extent(
                            current_slice_fragmentainer.fragmentainer_block_size(),
                        )
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
                            PageTopBlockPosition::new(
                                content_top
                                    - fragment_decoration_reservation.block_start().points(),
                            ),
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
                            || unit.block_size().points()
                                > fragment_decoration_reservation
                                    .fresh_content_extent(layout_pt(self.page_area_height()))
                                    .points()
                                    + 0.01
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
                            PageTopBlockPosition::new(
                                content_top
                                    - fragment_decoration_reservation.block_start().points(),
                            ),
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
                    first_fragmentainer_capacity: fragment_decoration_reservation
                        .remaining_content_extent(
                            materialized_fragmentainer.available_block_size(),
                        ),
                    continuation_fragmentainer_capacity: fragment_decoration_reservation
                        .fresh_content_extent(
                            materialized_fragmentainer.fragmentainer_block_size(),
                        ),
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
                    && committed_slice_start.points() < unit.block_end.points() - 0.01
                    && unit.item_indices.iter().any(|&index| {
                        row_flex_item_stretches_in_fragment(&children[index].style, style)
                    });
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
                // Descendant overflow does not split the flex container's
                // principal box. Its final used content edge can therefore
                // occur inside a longer replay source range; that first
                // fragment owns the container block-end decoration while
                // later fragments carry only descendant paint.
                // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                // <https://www.w3.org/TR/css-break-3/#box-splitting>
                let descendant_overflow_principal_finishes_here =
                    flex_item_overflows_container_source
                        && committed_slice_start.points() < total_content_height - 0.01
                        && slice_end.points() >= total_content_height - 0.01;
                if descendant_overflow_principal_finishes_here
                    && let Some(content_bounds) = planned_fragment.metadata.source_border_box
                {
                    let principal_content_height =
                        total_content_height - committed_slice_start.points();
                    planned_fragment.metadata.source_border_box = Some(PaintClip::new(
                        content_bounds.x(),
                        fragment_cursor.content_top.points() - principal_content_height,
                        content_bounds.width(),
                        principal_content_height,
                    ));
                }
                let final_source_unit_finishes = (unit_index + 1 == break_units.len()
                    && slice_end.points() >= unit.block_end.points() - 0.01)
                    || descendant_overflow_principal_finishes_here;
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
                let decoration = FragmentDecoration::for_box_decoration_break(
                    style.box_decoration_break,
                    committed_slice_start.points() <= 0.01,
                    final_source_unit_finishes,
                );
                debug_assert_eq!(
                    decoration.is_clone(),
                    style.box_decoration_break == css::BoxDecorationBreak::Clone,
                );
                // A descendant-overflow-only continuation owns child paint,
                // but not another flex container principal box. The used
                // flex container has already ended in an earlier
                // fragmentainer; retaining its source-overflow range here
                // would clone its background and border into later columns.
                let principal_fragment =
                    committed_slice_start.points() < total_content_height - 0.01;
                let mut materialized_fragment = if principal_fragment {
                    let border_box = planned_fragment.metadata.source_border_box.expect(
                        "a committed principal flex fragment must retain destination border geometry",
                    );
                    MaterializedFlexFragment::principal(
                        planned_fragment,
                        border_box,
                        decoration,
                        PaintTranslation::identity(),
                    )
                } else {
                    MaterializedFlexFragment::descendant_overflow_only(
                        planned_fragment,
                        PaintTranslation::identity(),
                    )
                };
                debug_assert_eq!(
                    materialized_fragment.source_bounds().start(),
                    committed_slice_start,
                    "a committed flex fragment must retain its selected source start",
                );
                debug_assert_eq!(
                    materialized_fragment.source_bounds().end(),
                    slice_end,
                    "a committed flex fragment must retain its selected source end",
                );
                debug_assert_eq!(
                    materialized_fragment
                        .principal_box()
                        .is_none_or(|fragment| fragment.decoration().owns_block_start()),
                    !principal_fragment || decoration.owns_block_start(),
                );
                debug_assert_eq!(
                    materialized_fragment
                        .principal_box()
                        .is_none_or(|fragment| fragment.decoration().owns_block_end()),
                    !principal_fragment || decoration.owns_block_end(),
                );
                let owns_principal_box = materialized_fragment.principal_box().is_some();
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
                if owns_principal_box {
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
                    let selected_source_block_start = item_fragment.selected_source_block_start();
                    let item_uses_cloned_fragment_projection = child.style.box_decoration_break
                        == css::BoxDecorationBreak::Clone
                        && item_fragment.used_bounds.has_cloned_fragment_projection();
                    let selected_source_offset_in_fragment = if item_uses_cloned_fragment_projection
                    {
                        item.y().points() - fragment_cursor.block_offset.points()
                    } else {
                        selected_source_block_start.points() - fragment_cursor.block_offset.points()
                    };
                    // Source-slice offsets describe clipping, not whether an
                    // earlier item fragment exists. The committed plan keeps
                    // leading paint overflow distinct from a true preceding
                    // slice, so negative cross-start margins cannot shorten
                    // the frozen principal border box during replay.
                    // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                    // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                    let item_continues_from_previous_fragment = item_fragment
                        .continuation
                        .continues_from_previous_fragment();
                    let item_continues_to_next_fragment =
                        item_fragment.content_slice.block_end.points()
                            < item_fragment.source_bounds.height().points() - 0.01;
                    let item_is_source_fragment =
                        item_continues_from_previous_fragment || item_continues_to_next_fragment;
                    let item_has_descendant_source_overflow =
                        item_fragment.source_bounds.height().points()
                            > item_fragment.used_bounds.height().points() + 0.01;
                    let visible_item_height = if item_uses_cloned_fragment_projection {
                        // The committed item bounds are destination geometry:
                        // cloned border and padding enlarge every fragment,
                        // while `content_slice` remains in source-content
                        // coordinates for descendant replay.
                        // <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
                        item.height().points()
                    } else if physical_flex_direction(style).is_row_axis()
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
                    } else if item_continues_from_previous_fragment
                        && item_has_descendant_source_overflow
                    {
                        // Descendant overflow has no flex-item principal box
                        // in this continuation, but its source canvas still
                        // occupies the full committed destination fragment
                        // span. Keep that visual clip separate from the
                        // frozen used border-box height.
                        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                        // <https://www.w3.org/TR/css-break-3/#box-splitting>
                        let consumed_before_item =
                            (item.y().points() - fragment_cursor.block_offset.points()).max(0.0);
                        (materialized_fragmentainer.available_block_size().points()
                            - consumed_before_item)
                            .max(0.0)
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
                    let mut item_cursor_y =
                        fragment_cursor.content_top.points() - selected_source_offset_in_fragment;
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
                    // `bounds` is the intersection with this fragment's
                    // source slice, so its block start is clamped to the
                    // line/fragmentainer boundary. Principal decoration,
                    // however, belongs to the frozen used flex item and can
                    // legitimately begin before that boundary (for example
                    // through a negative cross-start margin). Use the used
                    // origin here; the independently computed replay clip
                    // below continues to constrain descendant source paint.
                    // <https://www.w3.org/TR/css-flexbox-1/#flex-items>
                    // <https://www.w3.org/TR/css-break-3/#box-splitting>
                    let visible_item_top =
                        fragment_cursor.content_top.points() - selected_source_offset_in_fragment;
                    // Visible descendant source overflow selects a replay
                    // clip, not a larger flex principal box. Keep the frozen
                    // used border box in the materialized item record so its
                    // background and border stop at the Flexbox-resolved
                    // height even while descendant paint continues.
                    // <https://www.w3.org/TR/css-flexbox-1/#flex-items>
                    // <https://www.w3.org/TR/css-break-3/#box-splitting>
                    let principal_item_height = if item_has_descendant_source_overflow {
                        item_height.points()
                    } else {
                        visible_item_height
                    };
                    let mut item_page_border_box = PageTopRect::new(
                        item_content_left,
                        visible_item_top,
                        item_width.points(),
                        principal_item_height,
                    );
                    let mut item_replay_clip = PageTopRect::new(
                        item_content_left,
                        visible_item_top,
                        item_width.points(),
                        visible_item_height,
                    );
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
                    // Relative positioning moves the visual principal box
                    // without changing the flex line's normal-flow
                    // allocation. Descendant replay applies this offset when
                    // it enters the placed formatting context; keep the
                    // parent-owned decoration and its frozen paint bounds in
                    // the same visual coordinate space.
                    // <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>
                    // <https://drafts.csswg.org/css-flexbox/#painting>
                    let item_relative_offset =
                        self.normal_flow_relative_position_offset(&placed_style);
                    if item_relative_offset.x() != 0.0 || item_relative_offset.y() != 0.0 {
                        item_page_border_box = PageTopRect::new(
                            item_page_border_box.x() + item_relative_offset.x(),
                            item_page_border_box.top_y() + item_relative_offset.y(),
                            item_page_border_box.width(),
                            item_page_border_box.height(),
                        );
                        item_replay_clip = PageTopRect::new(
                            item_replay_clip.x() + item_relative_offset.x(),
                            item_replay_clip.top_y() + item_relative_offset.y(),
                            item_replay_clip.width(),
                            item_replay_clip.height(),
                        );
                    }
                    let item_border_box = item_page_border_box.paint_clip();
                    let item_replay_clip = item_replay_clip.paint_clip();
                    // This source interval is now committed against the
                    // item's frozen used border box. Source-slice replay must
                    // retain that block-start offset: a monolithic item is
                    // one canvas exposed through successive page clips, not
                    // independently laid-out item fragments.
                    // <https://www.w3.org/TR/css-contain-2/#size-containment>
                    // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                    item_fragment
                        .continuation
                        .materialize_source_canvas_block_start(used_item);
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
                    // Reserve this item's tree-order token before replaying
                    // descendants. A pseudo stacking context can later
                    // export those descendants to the parent paint tree, but
                    // it must still sort before them.
                    // <https://www.w3.org/TR/CSS22/zindex.html>
                    let item_paint_source_order = self.next_paint_source_order();
                    let item_positioned_layer_start = self.positioned_layers.len();
                    // Fragmented and nested paint ordering remains on the
                    // conservative in-flow replay path. In particular, a
                    // descendant can continue into a later multicolumn while
                    // its flex item's frozen border box remains in this
                    // column.
                    let mut item_replays_in_fragmented_paint_context =
                        self.active_fragmentainer_kind() == FragmentainerKind::Column;
                    let item_is_split = item_metadata.continues_from_previous_page
                        || item_metadata.continues_to_next_page;

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
                    let principal_decoration_intersects_slice =
                        item_fragment.decoration_slice.block_end.points()
                            > item_fragment.decoration_slice.block_start.points() + 0.01;
                    let principal_decoration_has_ink = placed_style
                        .background
                        .background_color
                        .is_potentially_visible()
                        || placed_style.background.background_image.is_image()
                        || placed_style.border_image.source.is_image()
                        || used_border_width(&placed_style) > layout_pt(0.0)
                        || !placed_style.box_shadow.is_empty()
                        || placed_style.outline_style != css::BorderStyle::None;
                    let flex_replay_owns_principal_decoration = !child_is_replaced
                        && !child.style.display.is_table()
                        && principal_decoration_intersects_slice;
                    // A flex item's principal decoration belongs to the
                    // item's atomic paint unit. In particular, a static
                    // flex item with a non-auto `z-index` establishes a
                    // stacking context, so its background must be captured
                    // by that context rather than remaining in the parent's
                    // ordinary background band.
                    // <https://drafts.csswg.org/css-flexbox/#painting>
                    // <https://drafts.csswg.org/css2/#z-index>
                    let item_paint_checkpoint = self.current_page.paint_checkpoint();
                    if flex_replay_owns_principal_decoration
                        && placed_style.visibility == Visibility::Visible
                        && principal_decoration_has_ink
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
                        principal_fragment.append_primitives_in_band(
                            PaintBand::Outline,
                            self.box_outline_primitives(
                                item_page_border_box.paint_rect(),
                                &placed_style,
                            ),
                        );
                        // The child replay may restore a speculative paint
                        // checkpoint. Commit the resolved flex principal box
                        // before that replay, while preserving Flexbox's
                        // order-modified document order within the shared
                        // background band.
                        self.current_page.append_paint_fragment_owned(
                            principal_fragment,
                            PaintTranslation::identity(),
                        );
                    }

                    // Principal decoration is parent-owned flex-item paint,
                    // but remains inside the item scope opened above. The
                    // descendant replay below uses `ParentPaints`, so it
                    // cannot duplicate or independently clip this decoration.
                    // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
                    let replay_content_height = Some(Definite::new(
                        replay_dimensions.available_height_for_replay(),
                    ));
                    let replay_logical_inline_size = child
                        .anonymous_content()
                        .is_some()
                        .then(|| {
                            replay_dimensions
                                .logical_inline_size_for_replay(WritingMode::HorizontalTb, None)
                        })
                        .flatten()
                        .or_else(|| {
                            placed_style
                                .writing_mode
                                .has_vertical_lines()
                                .then(|| {
                                    replay_dimensions.logical_inline_size_for_replay(
                                        placed_style.writing_mode,
                                        replay_content_height,
                                    )
                                })
                                .flatten()
                        });

                    let item_was_split = self.with_placed_formatting_context(
                        PlacedFormattingContext {
                            content_left: item_content_left,
                            content_width: replay_dimensions.available_width_for_replay(),
                            content_height: replay_content_height,
                            table_wrapper_border_box_block_size:
                                auto_table_wrapper_block_size_override(&child.style, item_height),
                            // Anonymous items preserve their historical
                            // width-derived replay basis. Vertical element
                            // items instead replay against their definite
                            // physical-height logical inline size.
                            // <https://www.w3.org/TR/css-flexbox-1/#flex-items>.
                            replay_logical_inline_size,
                            cursor_y: item_cursor_y,
                            page_start_margin_policy: PageStartMarginPolicy::Preserve,
                            float_scope: ReplayFloatScope::IsolatedFormattingContext,
                        },
                        &placed_style,
                        |layout| {
                            if item_is_split {
                                let item_top = PageTopBlockPosition::new(layout.cursor_y);
                                let mut replay_destination_block_end = None;
                                let has_descendant_source_overflow =
                                    item_fragment.source_bounds.height().points()
                                        > item_fragment.used_bounds.height().points() + 0.01
                                        // Size containment suppresses an
                                        // item's intrinsic contribution, not
                                        // its visible descendant paint. The
                                        // frozen used item box can therefore
                                        // equal its recorded source range
                                        // while its contained descendants
                                        // still overflow and must replay as
                                        // one continuous source canvas.
                                        // <https://www.w3.org/TR/css-contain-2/#size-containment>
                                        || child.element_parts().is_some_and(
                                            |(element, _, _)| {
                                                used_property_containment(
                                                    element,
                                                    &child.style,
                                                )
                                                .size
                                            },
                                        );
                                layout.paint_split_flex_item_fragment(
                                    child,
                                    &placed_style,
                                    stylesheets,
                                    SplitFlexItemPaintContext {
                                        item_width,
                                        item_height,
                                        percentage_height_basis: replay_item
                                            .percentage_height_basis,
                                        slice_border_box: item_replay_clip,
                                        fragment_content_clip: PaintClip::new(
                                            inner_x,
                                            item_replay_clip.y(),
                                            inner_width,
                                            item_replay_clip.height(),
                                        ),
                                        source_item_top: item_top,
                                        continuation: materialized_item.continuation,
                                        replay_origin: materialized_item.continuation.replay_origin,
                                        has_descendant_source_overflow,
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
                            let child_fragment_end = layout.layout_flex_item_contents(
                                child,
                                &placed_style,
                                stylesheets,
                                replay_item.percentage_height_basis,
                                if flex_replay_owns_principal_decoration {
                                    PrincipalBoxPaintMode::ParentPaints
                                } else {
                                    PrincipalBoxPaintMode::RootPaints
                                },
                            );
                            item_replays_in_fragmented_paint_context |=
                                child_fragment_end.is_some();
                            if let Some(child_fragment_end) = child_fragment_end
                                && child_fragment_end.page_index > item_page_index
                            {
                                let child_fragment_ordinal =
                                    child_fragment_end.page_index - item_page_index;
                                item_fragment.continuation.child_fragment_ordinal =
                                    Some(child_fragment_ordinal);
                                materialized_items
                                    .last_mut()
                                    .expect(
                                        "a replayed flex item is materialized before its contents",
                                    )
                                    .continuation
                                    .child_fragment_ordinal = Some(child_fragment_ordinal);
                                forced_child_fragment_ends[index] = Some(child_fragment_end);
                            }
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
                            false
                        },
                    );
                    // CSS Flexbox paints flex items as inline blocks in
                    // order-modified document order.  The principal
                    // decoration above and this item's independently replayed
                    // formatting context must therefore remain one paint
                    // unit: promoting every item's background into the page
                    // background band would let later item text paint above
                    // it.  This scopes static `z-index: auto` items only as
                    // an internal paint-order unit; the policy still lets
                    // positioned descendants escape unless CSS creates a real
                    // stacking context for the item.
                    // <https://drafts.csswg.org/css-flexbox/#painting>
                    // <https://drafts.csswg.org/css2/#elaborate-stacking-contexts>
                    let policy = if item_replays_in_fragmented_paint_context {
                        StackingContextPolicy::for_fragmented_flex_item(
                            &placed_style,
                            item_border_box,
                        )
                    } else {
                        StackingContextPolicy::for_flex_item(&placed_style, item_border_box)
                    };
                    // A static flex item needs a paint-order scope only when
                    // it owns a principal decoration. Plain items remain in
                    // the enclosing normal-flow band so their named-page and
                    // assignment side effects cannot be reordered.
                    // <https://www.w3.org/TR/css-flexbox-1/#painting>
                    let item_owns_principal_decoration = flex_replay_owns_principal_decoration
                        && placed_style.visibility == Visibility::Visible
                        && principal_decoration_has_ink;
                    if !matches!(policy.context_kind, StackingContextKind::None)
                        || item_owns_principal_decoration
                    {
                        // A static flex item is an atomic in-flow paint unit,
                        // not a real stacking context. Likewise, a relatively
                        // positioned `z-index: auto` item is only a pseudo
                        // context in the parent's auto/zero phase. Extract
                        // direct positioned contexts before scoping the item
                        // fragment so they remain ordered with positioned
                        // descendants of the item's siblings.
                        // <https://drafts.csswg.org/css-flexbox/#painting>
                        // <https://www.w3.org/TR/css-position-3/#painting-order>
                        let mut fragment = self
                            .current_page
                            .take_paint_fragment_since(item_paint_checkpoint.clone());
                        if !policy.captures_positioned_descendants {
                            let escaped_contexts = fragment.take_positioned_stacking_contexts();
                            self.positioned_layers
                                .extend(escaped_contexts.into_iter().map(|context| {
                                    PositionedPaintLayer {
                                        page_index: item_page_index,
                                        transaction_depth: self.positioned_paint_transaction_depth,
                                        source_element: None,
                                        source_style: placed_style.clone(),
                                        source_style_identity: &placed_style as *const ComputedStyle
                                            as usize,
                                        multicol_fragment_index: None,
                                        source_is_target: false,
                                        stack_level: context.stack_level,
                                        context,
                                        links: Vec::new(),
                                        // Flex replay already uses final page
                                        // coordinates, unlike atomic inline
                                        // scratch layout.
                                        escaped_atom_translation: EscapedAtomTranslation::none(),
                                    }
                                }));
                        }
                        let child_contexts = self.positioned_child_contexts_since(
                            item_positioned_layer_start,
                            item_page_index,
                            &policy,
                        );
                        self.scope_current_page_fragment_with_policy_and_source_order(
                            &item_paint_checkpoint,
                            policy,
                            item_border_box,
                            fragment,
                            child_contexts,
                            item_paint_source_order,
                        );
                    }
                    if item_was_split {
                        continue;
                    }
                }
                if materialized_fragment.is_descendant_overflow_only()
                    && !materialized_items.is_empty()
                {
                    // A descendant-overflow continuation has no principal
                    // flex-box decoration, but it is still real flow content
                    // in this destination fragmentainer.
                    self.mark_current_page_flow_content();
                }
                if let Some(Some(child_bottom)) =
                    split_item_replay_fragment_bottoms.get(materialized_fragment.layout.page_index)
                    && let Some(fragment_bounds) = materialized_fragment
                        .principal_box()
                        .map(DecoratedBoxFragment::border_box)
                    && *child_bottom > fragment_bounds.y() + 0.01
                    && *child_bottom < fragment_bounds.y() + fragment_bounds.height() - 0.01
                {
                    // A nested table can finish a committed flex source slice
                    // before the provisional source range used to reserve its
                    // fragmentainer. The child owns the final visible
                    // continuation edge, so the container must not manufacture
                    // another `slice` end border at the old source estimate.
                    let fragment = materialized_fragment
                        .principal_box_mut()
                        .expect("the matched principal flex fragment must retain its box");
                    fragment.set_border_box(PaintClip::new(
                        fragment_bounds.x(),
                        *child_bottom,
                        fragment_bounds.width(),
                        fragment_bounds.y() + fragment_bounds.height() - *child_bottom,
                    ));
                    fragment.decoration_mut().clear_block_end_for_slice();
                }
                materialized_fragment.item_fragments = materialized_items;
                flex_layout
                    .fragment_plan
                    .push_materialized_fragment(materialized_fragment);
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
                    fragment_cursor =
                        transition.cursor_after_fragmentainer_advance(PageTopBlockPosition::new(
                            content_top - fragment_decoration_reservation.block_start().points(),
                        ));
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
            .map(|fragment| fragment.source_bounds().start())
            .unwrap_or_else(|| FlexFragmentBlockOffset::new(0.0));
        let first_fragment_source_block_size = flex_layout
            .fragment_plan
            .materialized_fragments
            .first()
            .map(|fragment| fragment.source_bounds().size())
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
                    content_height: PhysicalContentHeight::new(content_box_pt(
                        total_content_height,
                    )),
                    content_top: PageTopBlockPosition::new(content_top),
                    source_fragment_block_offset: positioned_source_fragment_block_offset,
                    first_fragment_source_block_size,
                },
            );
        }
        self.pop_overflow_clip(overflow_clip_active);
        let flex_advanced_to_later_page = self.pages.len() > paint_page_index;
        let child_committed_destination_flow =
            flex_advanced_to_later_page && flex_has_forced_descendant_breaks;
        self.restore_page_area_parent_context_after_page_transition(
            previous_left,
            previous_right,
            flex_start_page_context,
            paint_page_index,
        );
        self.containing_block_direction = previous_containing_block_direction;
        self.containing_block_writing_mode = previous_containing_block_writing_mode;

        let final_fragment_consumed_block_size =
            layout_pt(total_content_height - fragment_cursor.block_offset.points());
        let committed_child_continuation_cursor = forced_child_fragment_ends
            .iter()
            .flatten()
            .filter(|end| end.page_index == self.pages.len())
            .map(|end| end.cursor.points())
            .max_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
            .or_else(|| child_committed_destination_flow.then_some(self.cursor_y));
        self.cursor_y = if let Some(cursor) = committed_child_continuation_cursor {
            // The in-flow child has just committed its own final
            // fragment. Its page-local cursor, rather than the source
            // flex interval, is the outer box's continuation endpoint.
            cursor
        } else if fragmentainer_kind == FragmentainerKind::Page && flex_advanced_to_later_page {
            // `fragment_cursor` retains the source sequence's accumulated
            // page transition offset. Once the final source slice has been
            // committed, normal-flow siblings must instead resume in the
            // active destination page's local fragmentainer coordinate
            // system. Leaving the accumulated offset here places the next
            // anonymous block beyond the physical page and silently drops
            // it during paint clipping.
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            self.page_top() - final_fragment_consumed_block_size.points()
        } else {
            fragment_cursor
                .content_top
                .toward_block_end(final_fragment_consumed_block_size)
                .points()
        };
        if let Some(Some((source_block_end, destination_block_end))) =
            replayed_child_destination_ends.get(self.pages.len())
            && *source_block_end >= total_content_height - 0.01
            && *destination_block_end <= self.page_top() + 0.01
        {
            // A split child can finish before the provisional source slice
            // used to reserve this final flex fragmentainer. The child's
            // durable destination end is the authoritative continuation
            // cursor for following normal flow; retaining the source span
            // would turn the unused tail into a synthetic extra page.
            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            self.cursor_y = self.cursor_y.max(*destination_block_end);
        }
        if let Some(Some(child_bottom)) = split_item_replay_fragment_bottoms.get(self.pages.len()) {
            self.cursor_y = self.cursor_y.max(*child_bottom);
        }
        if flex_fragmentation_enabled {
            self.fragment_top_offsets.pop();
        }
        self.cursor_y -= style.padding.bottom + border_widths.bottom;
        // Visible descendant overflow may occupy later fragmentainers, but it
        // cannot enlarge a flex container with a definite used block size or
        // move following normal flow. The materialized continuation plan owns
        // that descendant paint independently of this principal box.
        // <https://www.w3.org/TR/css-flexbox-1/#flex-items>
        // <https://www.w3.org/TR/css-overflow-3/#ink-overflow>
        let definite_principal_with_descendant_overflow =
            definite_content_height.is_some() && flex_item_overflows_container_source;
        let block_bottom = if definite_principal_with_descendant_overflow {
            block_top - total_height
        } else {
            self.cursor_y
        };
        if definite_principal_with_descendant_overflow {
            self.cursor_y = block_bottom;
        }
        let block_height = if definite_principal_with_descendant_overflow {
            total_height
        } else {
            (block_top - block_bottom).max(total_height)
        };
        let contents_overflow_clip =
            (overflow_clip_active || needs_contoured_overflow_clip).then(|| {
                let bounds = PageTopRect::new(
                    outer_x + border_widths.left,
                    block_top - border_widths.top,
                    content_width_points + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                )
                .paint_clip();
                AxisSelectivePaintClip::new(
                    bounds,
                    flex_used_overflow.clips_x() || containment.clips_descendant_paint(),
                    flex_used_overflow.clips_y() || containment.clips_descendant_paint(),
                )
            });
        let contoured_contents_overflow_clip = contents_overflow_clip
            .filter(|clip| clip.clips_x() && clip.clips_y() && needs_contoured_overflow_clip)
            .and_then(|clip| {
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
                    contour.bounds = clip.bounds();
                    contour
                })
            });
        if let Some(container_clip) = contents_overflow_clip {
            for fragment in &mut flex_layout.fragment_plan.materialized_fragments {
                fragment.contents_overflow_clip = fragment.principal_box().and_then(|fragment| {
                    let fragment_bounds = fragment.border_box();
                    container_clip
                        .bounds()
                        .intersect(fragment_bounds)
                        .map(|bounds| {
                            AxisSelectivePaintClip::new(
                                bounds,
                                container_clip.clips_x(),
                                container_clip.clips_y(),
                            )
                        })
                });
            }
        }
        if block_height > 0.0 {
            self.mark_current_page_flow_content();
        }
        let background_page_index = self.pages.len();
        let mut own_background_primitives = Vec::new();
        let mut own_outline_primitives = Vec::new();
        // Gap rules are positioned from finalized flex items and line boxes.
        // An auto-sized container can retain a provisional content height
        // here even though those finalized items already establish a larger
        // used cross extent (including the row gap between wrapped lines).
        // Use that final geometry for the paint boundary rather than
        // suppressing decoration solely because the pre-paint height is zero.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-container>
        let gap_decoration_content_height = flex_layout
            .items
            .iter()
            .map(|item| item.y().points() + item.height().points())
            .fold(
                flex_layout.height.points().max(total_content_height),
                f32::max,
            );
        let propagates_document_canvas_properties =
            self.element_propagates_document_canvas_properties(element, style);
        if propagates_document_canvas_properties && style.visibility == Visibility::Visible {
            self.capture_document_canvas_background(element, style);
        }
        // A propagated body participates in document-canvas property
        // resolution even when the root already supplies the canvas
        // background.  In that case the body keeps its ordinary principal-box
        // background.  Suppress local background paint only for the source
        // selected by CSS Backgrounds, just as block-flow layout does.
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
        let propagates_document_canvas_background = propagates_document_canvas_properties
            && self.element_paints_document_canvas_background(element);
        if !suppress_own_principal_box_decoration && propagates_document_canvas_background {
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
                border_style.background.background_color = css::BackgroundColor::TRANSPARENT;
                border_style.background.background_image = css::ComputedImage::None;
                border_style.background.background_layers.clear();
                own_background_primitives = self.box_background_primitives(
                    paint_space_rect(outer_x, block_bottom, outer_width, block_height),
                    &border_style,
                );
            }
        } else if !suppress_own_principal_box_decoration
            && block_height > 0.0
            && (style.background.background_color.is_potentially_visible()
                || style.background.background_image.is_image()
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
            // Flex gap topology is derived from the finalized flex layout.
            // An auto-height flex container can have a zero pre-layout block
            // height while its resolved lines occupy positive content space.
            // Suppressing decoration in that case loses ordinary row gaps.
            // <https://drafts.csswg.org/css-flexbox-1/#layout-algorithm>
            && gap_decoration_content_height > 0.0
            && style.visibility == Visibility::Visible
        {
            let gap_gutters = flex_gap_decoration_gutters(
                &flex_layout,
                style,
                PhysicalContentWidth::new(content_box_pt(inner_width)),
                PhysicalContentHeight::new(content_box_pt(gap_decoration_content_height)),
            );
            own_background_primitives.extend(flex_gap_decoration_primitives_with_gutters(
                style,
                GapDecorationContainer::new(
                    inner_x,
                    content_top,
                    inner_width,
                    gap_decoration_content_height,
                ),
                &flex_gap_decoration_items(&flex_layout),
                &gap_gutters,
            ));
        }
        if !suppress_own_principal_box_decoration
            && block_height > 0.0
            && style.visibility == Visibility::Visible
        {
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
            if let Some(contour) = contoured_contents_overflow_clip {
                fragment =
                    fragment.with_descendant_paint_effect_scoped_to_box_content_contour(contour);
            } else if let Some(clip) = contents_overflow_clip {
                if clip.clips_x() && clip.clips_y() {
                    fragment = fragment.with_descendant_paint_effect_scoped_to_rect(clip.bounds());
                } else {
                    fragment =
                        fragment.with_descendant_paint_effect_scoped_to_axis_selective_rect(clip);
                }
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
                fragment.promote_outline_to_in_flow_outline();
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
                                clip.bounds()
                                    .intersect(PaintClip::from_paint_rect(paint_space_rect(
                                        outer_x,
                                        bounds.y(),
                                        outer_width,
                                        bounds.height(),
                                    )))
                                    .map(|bounds| {
                                        AxisSelectivePaintClip::new(
                                            bounds,
                                            clip.clips_x(),
                                            clip.clips_y(),
                                        )
                                    })
                            })
                        })?
                    })
                } else {
                    (page_index == paint_page_index).then_some(clip)
                }
            });
            if let Some(contour) = contoured_contents_overflow_clip.clone() {
                fragment =
                    fragment.with_descendant_paint_effect_scoped_to_box_content_contour(contour);
            } else if let Some(clip) = fragment_contents_overflow_clip {
                if clip.clips_x() && clip.clips_y() {
                    fragment = fragment.with_descendant_paint_effect_scoped_to_rect(clip.bounds());
                } else {
                    fragment =
                        fragment.with_descendant_paint_effect_scoped_to_axis_selective_rect(clip);
                }
            }
            if flex_fragmentation_enabled || flex_spanned_pages {
                let page_has_materialized_flex_fragment = flex_layout
                    .fragment_plan
                    .materialized_fragments
                    .iter()
                    .any(|record| record.layout.page_index == page_index);
                let child_owns_unsplit_flex_continuation = flex_spanned_pages
                    && flex_has_forced_descendant_breaks
                    && flex_layout
                        .fragment_plan
                        .materialized_fragments
                        .iter()
                        .all(|record| record.layout.page_index == paint_page_index);
                let fragment_bounds = if child_owns_unsplit_flex_continuation {
                    // An in-flow forced break commits a continuation of the
                    // enclosing flex formatting box even if Flexbox itself
                    // did not split a line. Its outer fragment spans from
                    // the original box edge to the first fragmentainer end,
                    // across any intervening fragmentainers, and from the
                    // final fragmentainer start to the cursor consumed by
                    // the child. Do not infer those edges from descendant
                    // paint bounds: an empty tail/head is still owned box
                    // geometry.
                    // <https://www.w3.org/TR/css-break-3/#box-splitting>
                    let block_start = if page_index == paint_page_index {
                        flex_container_page_fragment_bounds(&flex_layout.fragment_plan, page_index)
                            .map(|bounds| bounds.y() + bounds.height())
                            .unwrap_or(block_top)
                    } else {
                        self.page_top()
                    };
                    let block_end = if page_index == self.pages.len() {
                        block_bottom
                    } else {
                        self.page_bottom()
                    };
                    (block_start >= block_end - 0.01).then(|| {
                        PaintClip::new(outer_x, block_end, outer_width, block_start - block_end)
                    })
                } else {
                    flex_container_page_fragment_bounds(&flex_layout.fragment_plan, page_index)
                        .or_else(|| {
                            // When a flex continuation record exists but has
                            // no destination principal box, it is carrying
                            // descendant overflow only. Do not fall back to
                            // that child paint's bounds and recreate the
                            // flex container decoration in this fragment.
                            (!page_has_materialized_flex_fragment && flex_spanned_pages)
                                .then(|| {
                                    fragment.bounds().map(|bounds| {
                                        PaintClip::from_paint_rect(paint_space_rect(
                                            outer_x,
                                            bounds.y(),
                                            outer_width,
                                            bounds.height(),
                                        ))
                                    })
                                })
                                .flatten()
                        })
                };
                if let Some(fragment_bounds) = fragment_bounds {
                    // An unsplit flex plan may have already materialized its
                    // initial item before an in-flow descendant commits a
                    // forced child page break. In that case the child owns
                    // the outer continuation span: the initial record must
                    // not prematurely claim the flex box's block end.
                    // <https://www.w3.org/TR/css-break-3/#forced-breaks>
                    // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                    let (owns_block_start, owns_block_end) = if child_owns_unsplit_flex_continuation
                    {
                        (
                            page_index == paint_page_index,
                            page_index == self.pages.len(),
                        )
                    } else if flex_layout
                        .fragment_plan
                        .materialized_fragments
                        .iter()
                        .any(|record| record.layout.page_index == page_index)
                    {
                        flex_layout
                            .fragment_plan
                            .materialized_fragments
                            .iter()
                            .filter(|record| record.layout.page_index == page_index)
                            .fold((false, false), |(owns_start, owns_end), record| {
                                (
                                    owns_start
                                        || record.principal_box().is_some_and(|fragment| {
                                            fragment.decoration().owns_block_start()
                                        }),
                                    owns_end
                                        || record.principal_box().is_some_and(|fragment| {
                                            fragment.decoration().owns_block_end()
                                        }),
                                )
                            })
                    } else if flex_spanned_pages && flex_has_forced_descendant_breaks {
                        // No flex break unit was split: the child itself
                        // committed this continuation. Project that durable
                        // child page sequence onto the enclosing flex box so
                        // `box-decoration-break: slice` owns only the first
                        // block-start and final block-end edges.
                        (
                            page_index == paint_page_index,
                            page_index == self.pages.len(),
                        )
                    } else {
                        (false, false)
                    };
                    let mut fragment_style = style.clone();
                    suppress_fragmented_box_edges(
                        &mut fragment_style,
                        owns_block_start,
                        owns_block_end,
                    );
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
                fragment.promote_outline_to_in_flow_outline();
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
