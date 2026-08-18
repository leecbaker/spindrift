use super::*;

/// The percentage bases that must stay coupled for one flex item's intrinsic
/// measurement pass.  A winning block constraint is not a property of only
/// inline collection: every subsequent intrinsic query must observe it, or a
/// later content-basis probe can restore a cyclic natural-size contribution.
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
#[derive(Debug, Clone, Copy)]
struct FlexIntrinsicMeasurementContext {
    inline_percentage_basis: IntrinsicInlinePercentageBasis,
    block_basis: IntrinsicBlockBasis,
}

impl FlexIntrinsicMeasurementContext {
    fn measure<R>(
        self,
        layout: &mut LayoutBuilder<'_>,
        measurement: impl FnOnce(&mut LayoutBuilder<'_>) -> R,
    ) -> R {
        layout.with_flex_item_percentage_height_basis(self.block_basis, |layout| {
            layout.with_intrinsic_inline_percentage_basis(self.inline_percentage_basis, measurement)
        })
    }
}

pub(super) fn flex_estimated_border_box_width(
    style: &ComputedStyle,
    content_width: ContentBoxLength,
) -> BorderBoxLength {
    content_box_to_border_box_length(
        content_width,
        non_content_pt(style.padding.left + style.padding.right + horizontal_border_width(style)),
    )
}

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn estimate_normal_flow_flex_item(
        &mut self,
        child: &StyledChild<'_>,
        element: &Element,
        signature: &ElementSignature,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &Stylesheets<'_>,
        context: super::item_special_cases::FlexItemEstimateContext<'_>,
    ) -> FlexItemEstimate {
        let super::item_special_cases::FlexItemEstimateContext {
            style,
            available,
            physical_direction,
            containing_width,
            containing_width_basis: _,
            containing_height_basis,
            containing_inline_size,
            containing_inline_size_points,
            inline_measurement_space,
            preferred_inline_basis,
            vertical_non_content,
        } = context;
        let inline_content_width =
            inline_measurement_space.content_box_inline_width(containing_inline_size_points, style);
        let inline_percentage_basis = if style.box_values.width.is_auto() {
            PercentageBasis::indefinite()
        } else {
            PercentageBasis::definite_from(
                content_box_pt(inline_content_width),
                IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
            )
        };
        let initial_measurement_context = FlexIntrinsicMeasurementContext {
            inline_percentage_basis,
            block_basis: IntrinsicBlockBasis::Indefinite,
        };
        let mut measurement_context = initial_measurement_context;
        let definition_list_column_height = initial_measurement_context.measure(self, |layout| {
            layout.estimate_definition_list_column_height(
                child,
                stylesheets,
                containing_inline_size,
            )
        });
        let mut inline_measurement = initial_measurement_context.measure(self, |layout| {
            layout.estimate_child_inline_measurement(
                child,
                stylesheets,
                LogicalInlineContentSize::new(content_box_pt(inline_content_width)),
            )
        });
        // A definite min/max block constraint can win over an automatic
        // intrinsic block contribution. Once it does, descendants resolve
        // percentage heights against that used item height and a replaced
        // descendant can transfer the result through its preferred ratio.
        // Keep the first probe indefinite, however: a merely content-based
        // automatic size is not a percentage basis during intrinsic sizing.
        // <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
        // <https://drafts.csswg.org/css-flexbox/#min-size-auto>
        let winning_block_constraint = matches!(style.writing_mode, WritingMode::HorizontalTb)
            .then(|| {
                flex_item_winning_intrinsic_block_constraint(
                    style,
                    content_box_pt(inline_measurement.logical_block_span(style)),
                    containing_height_basis,
                    vertical_non_content,
                )
            })
            .unwrap_or(IntrinsicBlockBasis::Indefinite);
        if winning_block_constraint
            .descendant_percentage_basis()
            .is_definite()
        {
            measurement_context.block_basis = winning_block_constraint;
            inline_measurement = measurement_context.measure(self, |layout| {
                layout.estimate_child_inline_measurement(
                    child,
                    stylesheets,
                    LogicalInlineContentSize::new(content_box_pt(inline_content_width)),
                )
            });
        }
        // The same winning constraint governs every intrinsic query for the
        // item, not just inline collection. Otherwise a subsequent width
        // contribution query can revive the child's unconstrained natural
        // width after the first measurement correctly resolved its percent
        // height.
        let child_intrinsic = measurement_context.measure(self, |layout| {
            layout.estimate_child_intrinsic_widths(
                child,
                stylesheets,
                containing_inline_size,
                inline_measurement.contribution,
            )
        });
        let remeasuring_post_flexed_main_size = matches!(
            if physical_direction.is_row_axis() {
                available.width_basis
            } else {
                available.height_basis
            },
            PercentageBasis::Definite {
                source: FlexAvailableSizeSource::PostFlexingMainSize,
                ..
            }
        );
        let content_basis_inline_width =
            flex_basis_uses_content_inline_size(style, physical_direction)
                && !remeasuring_post_flexed_main_size
                && child_intrinsic.max_content.points() > inline_content_width + 0.01;
        if content_basis_inline_width {
            inline_measurement = measurement_context.measure(self, |layout| {
                layout.estimate_child_inline_measurement(
                    child,
                    stylesheets,
                    child_intrinsic.max_content,
                )
            });
        }
        let hypothetical_cross_measure_width = if content_basis_inline_width {
            child_intrinsic.max_content
        } else {
            containing_inline_size
        };
        let child_preferred_block_height = measurement_context.measure(self, |layout| {
            layout.estimate_child_min_content_block_size(
                child,
                stylesheets,
                hypothetical_cross_measure_width,
                LogicalBlockContentSize::new(content_box_pt(
                    inline_measurement.logical_block_span(style),
                )),
            )
        });

        let logical_inline_size = child_intrinsic.max_content;
        let logical_min_inline_size = child_intrinsic.min_content;
        let inline_block_contribution = if inline_measurement.line_count() > 0 {
            // The selected line sequence owns every used line-box extent.
            // In particular, a terminal forced break can leave a zero-height
            // empty record after the final painted line. Reconstructing the
            // block size as `line_count * line-height` would turn that record
            // into spurious flex-item height.
            // <https://www.w3.org/TR/css-inline-3/#line-layout>
            inline_measurement.logical_block_span(style)
        } else {
            // A block-only formatting context has no inline line box or
            // inherited line strut of its own. Its automatic block-size is
            // the block-stack contribution of its in-flow descendants. In
            // particular, an auto-height flex item containing a fixed-height
            // block must not grow to the item's inherited `line-height`.
            // <https://www.w3.org/TR/CSS22/visudet.html#normal-block> and
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
            0.0
        };
        let fallback_logical_block_size = if inline_measurement.line_count() == 0
            && element.children.is_empty()
            && child_intrinsic.max_content.points() == 0.0
        {
            // A genuinely empty block has zero content height, allowing
            // align-content/stretch to distribute a flex line's cross size.
            0.0
        } else {
            inline_block_contribution
        };
        let logical_block_size = definition_list_column_height.unwrap_or_else(|| {
            LogicalBlockContentSize::new(content_box_pt(
                fallback_logical_block_size.max(child_preferred_block_height.points()),
            ))
        });
        let mut physical_intrinsic = flex_item_physical_intrinsic_sizes(
            style.writing_mode,
            FlexItemLogicalIntrinsicSizes {
                preferred_inline: logical_inline_size,
                min_inline: logical_min_inline_size,
                block: logical_block_size,
            },
        );
        if style.display.is_table() && style.writing_mode == WritingMode::HorizontalTb {
            let table_sizing = self.with_ancestor_signature(signature.clone(), |layout| {
                let built_child_boxes;
                let table_child_boxes = if let Some(child_boxes) = child_boxes {
                    child_boxes
                } else {
                    built_child_boxes = layout.build_frozen_child_boxes_with_current_ancestors(
                        element,
                        stylesheets,
                        style,
                    );
                    &built_child_boxes
                };
                let fragment =
                    box_tree::build_frozen_table_fragment(element, signature, table_child_boxes);
                layout.table_wrapper_flex_sizing_from_fragment(
                    element,
                    style,
                    stylesheets,
                    &fragment,
                    containing_width.points(),
                )
            });
            physical_intrinsic.preferred_width = PhysicalContentWidth::new(
                table_sizing.wrapper_preferred_inline.content_box_length(),
            );
            let table_automatic_minimum = used_length_percentage_or_auto_with_basis(
                style.box_values.width.clone(),
                preferred_inline_basis,
            )
            .map(|authored_width| {
                // A specified table width participates in Flexbox's
                // automatic minimum as the preferred table wrapper size;
                // a cell's wider preferred width may expand a standalone
                // table, but must not make the flex item refuse its own
                // specified width before flexible-length resolution.
                // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
                // <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>
                table_sizing
                    .grid_min_content_inline
                    .points()
                    .min(authored_width.points())
            })
            .unwrap_or_else(|| table_sizing.grid_min_content_inline.points());
            physical_intrinsic.preferred_min_width =
                PhysicalContentWidth::new(content_box_pt(table_automatic_minimum));
            // The wrapper's block contribution is a main-axis automatic
            // minimum for a physical column. In a row it is merely the
            // hypothetical cross contribution: an authored table `height`
            // must remain free to establish the used cross size.
            if physical_direction.is_column_axis() {
                physical_intrinsic.min_content_height = PhysicalContentHeight::new(
                    table_sizing.wrapper_intrinsic_block.content_box_length(),
                );
                if style.box_values.height.is_auto() {
                    physical_intrinsic.intrinsic_content_height = PhysicalContentHeight::new(
                        table_sizing.wrapper_intrinsic_block.content_box_length(),
                    );
                }
            }
        }
        // An orthogonal item's own inline formatting context can be empty
        // even when an in-flow descendant gives it a definite physical block
        // extent. The block-stack estimator currently has no orthogonal-child
        // projection, so retain that intrinsic fallback only for a genuinely
        // empty block contribution. In particular, never project a multi-line
        // vertical item's *inline* max-content length onto physical width:
        // vertical inline size maps to physical height.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        if inline_measurement.line_count() == 0
            && logical_block_size.points() <= 0.01
            && matches!(
                style.writing_mode,
                WritingMode::VerticalRl
                    | WritingMode::VerticalLr
                    | WritingMode::SidewaysRl
                    | WritingMode::SidewaysLr
            )
        {
            physical_intrinsic.preferred_width =
                PhysicalContentWidth::new(content_box_pt(child_intrinsic.max_content.points()));
            physical_intrinsic.preferred_min_width =
                PhysicalContentWidth::new(content_box_pt(child_intrinsic.min_content.points()));
        }
        let mut content_width =
            if remeasuring_post_flexed_main_size && physical_direction.is_row_axis() {
                // Flexible-length resolution has already turned this item's
                // physical main size into `available.width`.  Re-resolving an
                // authored percentage width against that item-local width would
                // apply the percentage twice (for example 30% becomes 9% of the
                // flex container) and inflate the resulting cross measurement.
                // The final content width is the correct containing block for
                // this item's descendants during the post-flexing remeasurement.
                // <https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths>
                // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
                available.width.points()
            } else {
                used_length_percentage_or_auto_with_basis(
                    style.box_values.width.clone(),
                    preferred_inline_basis,
                )
                .map(|width| width.points())
                .unwrap_or(physical_intrinsic.preferred_width.points())
            };
        let block_height_probe_width = if style.box_values.width.is_auto()
            && matches!(style.writing_mode, WritingMode::HorizontalTb)
            && physical_direction.is_row_axis()
            && flex_basis_uses_content_inline_size(style, physical_direction)
        {
            // The hypothetical cross size follows a content-based flex base
            // size, not the flex container's constrained available width.
            // Measuring float clearance at that narrower width would wrap
            // independent floats into artificial rows and inflate the
            // item's block contribution.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
            content_width
        } else if style.box_values.width.is_auto() {
            containing_inline_size_points
        } else {
            content_width
        };
        if let Some(block_height) = measurement_context.measure(self, |layout| {
            layout.measure_flex_item_auto_block_height_for_flex_basis(
                element,
                style,
                stylesheets,
                child_boxes,
                PhysicalContentWidth::new(content_box_pt(block_height_probe_width)),
                FlexAutoBlockHeightMeasurement {
                    inline_line_count: inline_measurement.line_count(),
                    purpose: if remeasuring_post_flexed_main_size {
                        FlexAutoBlockHeightMeasurementPurpose::PostFlexingMainSize
                    } else {
                        FlexAutoBlockHeightMeasurementPurpose::IntrinsicFlexBase
                    },
                },
            )
        }) && matches!(style.writing_mode, WritingMode::HorizontalTb)
        {
            physical_intrinsic.intrinsic_content_height = block_height;
            physical_intrinsic.min_content_height =
                PhysicalContentHeight::new(content_box_pt(block_height.points().max(0.0)));
        }
        if style.box_values.height.is_auto()
            && matches!(style.writing_mode, WritingMode::HorizontalTb)
            && let Some(multicol_height) = measurement_context.measure(self, |layout| {
                layout.estimate_child_multicol_inline_height(
                    child,
                    stylesheets,
                    LogicalInlineContentSize::new(constrain_content_width(
                        style,
                        content_box_pt(content_width),
                        PercentageBasis::definite(containing_width),
                    )),
                )
            })
        {
            physical_intrinsic.intrinsic_content_height =
                PhysicalContentHeight::new(multicol_height.content_box_length());
            physical_intrinsic.min_content_height =
                PhysicalContentHeight::new(content_box_pt(multicol_height.points().max(0.0)));
        }
        let mut content_height = used_content_box_height_or_auto_with_basis(
            style,
            containing_height_basis,
            vertical_non_content,
        )
        .map(SemanticLengthExt::points)
        .unwrap_or(physical_intrinsic.intrinsic_content_height.points());
        if let Some(ratio) = style.aspect_ratio.preferred_ratio_for_non_replaced(false) {
            match (
                style.box_values.width.is_auto(),
                style.box_values.height.is_auto(),
            ) {
                (false, true) => {
                    let transferred_height = content_width / ratio;
                    if inline_measurement.line_count() == 0 && element.children.is_empty() {
                        physical_intrinsic.intrinsic_content_height =
                            PhysicalContentHeight::new(content_box_pt(transferred_height));
                        physical_intrinsic.min_content_height =
                            PhysicalContentHeight::new(content_box_pt(transferred_height));
                        content_height = transferred_height;
                    } else {
                        content_height = content_height.max(transferred_height);
                        physical_intrinsic.min_content_height =
                            PhysicalContentHeight::new(content_box_pt(
                                physical_intrinsic
                                    .min_content_height
                                    .points()
                                    .max(transferred_height),
                            ));
                    }
                }
                (true, false) => {
                    let transferred_width = content_height * ratio;
                    if inline_measurement.line_count() == 0 && element.children.is_empty() {
                        content_width = transferred_width;
                    } else {
                        content_width = content_width.max(transferred_width);
                    }
                    // The preferred width exported to Flexbox's intrinsic
                    // sizing phases must include the ratio transfer as well
                    // as this temporary used width. In particular,
                    // `flex-basis: content` and an inline flex container's
                    // shrink-to-fit cross size consume these contributions
                    // rather than `width` above.
                    // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
                    // and <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
                    physical_intrinsic.preferred_width = PhysicalContentWidth::new(content_box_pt(
                        physical_intrinsic
                            .preferred_width
                            .points()
                            .max(transferred_width),
                    ));
                    physical_intrinsic.preferred_min_width =
                        PhysicalContentWidth::new(content_box_pt(
                            physical_intrinsic
                                .preferred_min_width
                                .points()
                                .max(transferred_width),
                        ));
                    if matches!(
                        style.writing_mode,
                        WritingMode::VerticalRl
                            | WritingMode::VerticalLr
                            | WritingMode::SidewaysRl
                            | WritingMode::SidewaysLr
                    ) {
                        if inline_measurement.line_count() == 0 && element.children.is_empty() {
                            physical_intrinsic.min_content_height =
                                PhysicalContentHeight::new(content_box_pt(content_height));
                        } else {
                            physical_intrinsic.min_content_height =
                                PhysicalContentHeight::new(content_box_pt(
                                    physical_intrinsic
                                        .min_content_height
                                        .points()
                                        .max(content_height),
                                ));
                        }
                    } else {
                        physical_intrinsic.min_content_height =
                            PhysicalContentHeight::new(content_box_pt(
                                physical_intrinsic
                                    .min_content_height
                                    .points()
                                    .max(content_height),
                            ));
                    }
                }
                (true, true) => {
                    let stretched_height = available
                        .stretched_height
                        .map(|height| (height.points() - vertical_non_content.points()).max(0.0));
                    let stretched_width = available.stretched_width.map(|width| {
                        let horizontal_non_content = style.padding.left
                            + style.padding.right
                            + horizontal_border_width(style);
                        (width.points() - horizontal_non_content).max(0.0)
                    });
                    if let Some(height) = stretched_height {
                        content_height = height;
                        content_width = height * ratio;
                    } else if let Some(width) = stretched_width {
                        content_width = width;
                        content_height = width / ratio;
                    }
                    if stretched_height.is_some() || stretched_width.is_some() {
                        physical_intrinsic.preferred_width =
                            PhysicalContentWidth::new(content_box_pt(content_width));
                        physical_intrinsic.preferred_min_width =
                            PhysicalContentWidth::new(content_box_pt(content_width));
                        if inline_measurement.line_count() == 0 && element.children.is_empty() {
                            physical_intrinsic.intrinsic_content_height =
                                PhysicalContentHeight::new(content_box_pt(content_height));
                            physical_intrinsic.min_content_height =
                                PhysicalContentHeight::new(content_box_pt(content_height));
                        } else {
                            // A definite stretched cross size transfers through
                            // `aspect-ratio`, but it does not replace a
                            // non-replaced item's content-based automatic
                            // minimum in the main axis.
                            // https://www.w3.org/TR/css-flexbox-1/#min-size-auto
                            physical_intrinsic.intrinsic_content_height =
                                PhysicalContentHeight::new(content_box_pt(
                                    physical_intrinsic
                                        .intrinsic_content_height
                                        .points()
                                        .max(content_height),
                                ));
                            physical_intrinsic.min_content_height =
                                PhysicalContentHeight::new(content_box_pt(
                                    physical_intrinsic
                                        .min_content_height
                                        .points()
                                        .max(content_height),
                                ));
                        }
                    }
                }
                _ => {}
            }
        }

        let mut width = constrain_content_width(
            style,
            content_box_pt(content_width),
            PercentageBasis::definite(containing_width),
        )
        .points();
        let height = constrain_flex_item_estimated_height(
            style,
            content_box_pt(content_height),
            physical_intrinsic.min_content_height.content_box_length(),
            physical_intrinsic
                .intrinsic_content_height
                .content_box_length(),
            containing_height_basis,
            vertical_non_content,
        );
        // An automatic non-replaced box with a preferred aspect ratio can
        // acquire a definite block size from its min/max block constraints.
        // That used main size then transfers into the automatic cross size.
        // This matters while calculating an intrinsic column-flex container:
        // its shrink-to-fit width is the item's transferred width, not the
        // pre-constraint empty-content contribution.  Use the shared transfer
        // helper so `box-sizing:border-box` applies the ratio to the border
        // box while `auto <ratio>` remains content-box based.
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio> and
        // <https://drafts.csswg.org/css-flexbox-1/#intrinsic-sizes>.
        if style.box_values.width.is_auto()
            && style.box_values.height.is_auto()
            && !style.box_values.min_height.is_auto()
            && let Some(ratio) = style.aspect_ratio.preferred_ratio_for_non_replaced(false)
        {
            let transferred_width = flex_aspect_ratio_transferred_content_main_size(
                style,
                height,
                // We are deriving the physical width (the row-axis main
                // size) from the resolved physical height.
                FlexDirection::Row,
                ratio,
            )
            .points();
            width = constrain_content_width(
                style,
                content_box_pt(transferred_width),
                PercentageBasis::definite(containing_width),
            )
            .points();
            physical_intrinsic.preferred_width = PhysicalContentWidth::new(content_box_pt(
                physical_intrinsic
                    .preferred_width
                    .points()
                    .max(transferred_width),
            ));
            physical_intrinsic.preferred_min_width = PhysicalContentWidth::new(content_box_pt(
                physical_intrinsic
                    .preferred_min_width
                    .points()
                    .max(transferred_width),
            ));
        }
        let min_width = constrain_content_width(
            style,
            physical_intrinsic.preferred_min_width.content_box_length(),
            PercentageBasis::definite(containing_width),
        )
        .points();
        let min_height = constrain_flex_item_estimated_height(
            style,
            physical_intrinsic.min_content_height.content_box_length(),
            physical_intrinsic.min_content_height.content_box_length(),
            physical_intrinsic
                .intrinsic_content_height
                .content_box_length(),
            containing_height_basis,
            vertical_non_content,
        );
        let fallback_line_baseline_offset =
            layout_pt(self.inline_box_text_line_layout_baseline_offset(style));
        let first_line_baseline_offset = first_sequence_line_baseline_offset(
            &inline_measurement.sequence,
            fallback_line_baseline_offset,
        );
        let last_line_baseline_offset = last_sequence_line_baseline_offset(
            &inline_measurement.sequence,
            fallback_line_baseline_offset,
        );
        let preceding_line_height = preceding_line_height_before_last(&inline_measurement.sequence);
        let descendant_baselines = if inline_measurement.line_count() == 0 {
            self.estimate_flex_item_descendant_baselines(
                element,
                signature,
                style,
                child_boxes,
                stylesheets,
                available.width,
            )
        } else {
            FlexItemBaselineEstimate::default()
        };

        let baseline_edge = used_border_widths(style).top + style.padding.top;
        let first_text_baseline = baseline_edge + first_line_baseline_offset.points();
        let last_text_baseline = baseline_edge
            + inline_measurement
                .sequence
                .last_line_baseline_offset(fallback_line_baseline_offset.points());
        FlexItemEstimate::new(
            IntrinsicItemMetrics {
                width: content_box_pt(width),
                height,
                min_width: content_box_pt(min_width),
                min_height,
                content_width: physical_intrinsic.preferred_width.content_box_length(),
                content_height: physical_intrinsic
                    .intrinsic_content_height
                    .content_box_length(),
                preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                first_baseline: (inline_measurement.line_count() > 0)
                    .then_some(first_text_baseline)
                    .or(descendant_baselines
                        .vertical
                        .first
                        .map(FlexVerticalBaselineOffset::points)),
                last_baseline: (inline_measurement.line_count() > 0)
                    .then_some(last_text_baseline)
                    .or(descendant_baselines
                        .vertical
                        .last
                        .map(FlexVerticalBaselineOffset::points)),
            },
            FlexItemBaselineEstimate {
                vertical: FlexItemBaselinePair {
                    first: (inline_measurement.line_count() > 0)
                        .then_some(flex_vertical_baseline_from_points(first_text_baseline))
                        .or(descendant_baselines.vertical.first),
                    last: (inline_measurement.line_count() > 0)
                        .then_some(flex_vertical_baseline_from_points(last_text_baseline))
                        .or(descendant_baselines.vertical.last),
                },
                horizontal: FlexItemBaselinePair {
                    first: (inline_measurement.line_count() > 0)
                        .then(|| {
                            first_horizontal_text_baseline_offset(
                                style,
                                flex_estimated_border_box_width(style, content_box_pt(width)),
                                first_line_baseline_offset,
                            )
                        })
                        .flatten()
                        .or(descendant_baselines.horizontal.first),
                    last: (inline_measurement.line_count() > 0)
                        .then(|| {
                            last_horizontal_text_baseline_offset(
                                style,
                                flex_estimated_border_box_width(style, content_box_pt(width)),
                                preceding_line_height,
                                last_line_baseline_offset,
                            )
                        })
                        .flatten()
                        .or(descendant_baselines.horizontal.last),
                },
            },
        )
    }
}

/// The physical containing space available to an intrinsic block-size walk
/// below a flex item.
///
/// The logical inline constraint remains distinct from the block-size
/// percentage basis, which independently controls whether descendant
/// `height` percentages resolve:
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemPhysicalIntrinsicSizes {
    /// Intrinsic values after CSS Writing Modes has projected them onto the
    /// physical axes consumed by flex layout.
    pub(super) preferred_width: PhysicalContentWidth,
    pub(super) preferred_min_width: PhysicalContentWidth,
    pub(super) intrinsic_content_height: PhysicalContentHeight,
    pub(super) min_content_height: PhysicalContentHeight,
}

/// Intrinsic sizes before CSS Writing Modes projects a flex item onto the
/// physical axes consumed by Flexbox.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemLogicalIntrinsicSizes {
    pub(super) preferred_inline: LogicalInlineContentSize,
    pub(super) min_inline: LogicalInlineContentSize,
    pub(super) block: LogicalBlockContentSize,
}

/// The physical intrinsic content-box sizes of a flex container before the
/// scalar `IntrinsicItemMetrics` adapter. Keeping this record typed prevents
/// a main/cross projection from being reassembled as interchangeable width
/// and height floats while line metrics and authored CSS sizes are applied.
pub(super) fn flex_item_physical_intrinsic_sizes(
    writing_mode: WritingMode,
    logical: FlexItemLogicalIntrinsicSizes,
) -> FlexItemPhysicalIntrinsicSizes {
    if !WritingModeAxes::new(writing_mode, Direction::Ltr).swaps_physical_axes() {
        FlexItemPhysicalIntrinsicSizes {
            preferred_width: PhysicalContentWidth::new(
                logical.preferred_inline.content_box_length(),
            ),
            preferred_min_width: PhysicalContentWidth::new(logical.min_inline.content_box_length()),
            intrinsic_content_height: PhysicalContentHeight::new(
                logical.block.content_box_length(),
            ),
            min_content_height: PhysicalContentHeight::new(logical.block.content_box_length()),
        }
    } else {
        FlexItemPhysicalIntrinsicSizes {
            preferred_width: PhysicalContentWidth::new(logical.block.content_box_length()),
            preferred_min_width: PhysicalContentWidth::new(logical.block.content_box_length()),
            intrinsic_content_height: PhysicalContentHeight::new(
                logical.preferred_inline.content_box_length(),
            ),
            min_content_height: PhysicalContentHeight::new(logical.min_inline.content_box_length()),
        }
    }
}

/// Whether a content-based flex basis is resolved along the item's logical
/// inline axis.
///
/// In that case CSS Flexbox lays the item out at its max-content flex base
/// before deriving its hypothetical cross size. Measuring its line boxes at a
/// narrower container width would introduce soft wraps that do not exist in
/// the used flex item.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
/// <https://www.w3.org/TR/css-sizing-3/#max-content>
pub(super) fn flex_basis_uses_content_inline_size(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    let logical_inline_is_main =
        match WritingModeAxes::new(style.writing_mode, style.used_direction())
            .physical_axis(LogicalAxis::Inline)
        {
            PhysicalAxis::Horizontal => physical_direction.is_row_axis(),
            PhysicalAxis::Vertical => physical_direction.is_column_axis(),
        };
    if !logical_inline_is_main {
        return false;
    }
    match style.flex_basis {
        css::ComputedFlexBasis::Content | css::ComputedFlexBasis::MaxContent => true,
        css::ComputedFlexBasis::Auto => {
            if physical_direction.is_row_axis() {
                style.box_values.width.is_auto()
            } else {
                style.box_values.height.is_auto()
            }
        }
        css::ComputedFlexBasis::LengthPercentage(_)
        | css::ComputedFlexBasis::MinContent
        | css::ComputedFlexBasis::FitContent(_) => false,
    }
}
