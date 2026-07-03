use super::*;

#[derive(Debug, Clone, Copy)]
struct FlexItemPhysicalIntrinsicSizes {
    preferred_width: f32,
    preferred_min_width: f32,
    intrinsic_content_height: f32,
    min_content_height: f32,
}

/// Project a flex item's logical intrinsic sizes into Quire's physical axes.
///
/// CSS Writing Modes maps inline size to physical height in vertical writing
/// modes, while block size maps to physical width. Flexbox consumes those
/// physical sizes when resolving flex base sizes and hypothetical cross sizes:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
fn flex_item_physical_intrinsic_sizes(
    writing_mode: WritingMode,
    logical_inline_size: f32,
    logical_min_inline_size: f32,
    logical_block_size: f32,
) -> FlexItemPhysicalIntrinsicSizes {
    match writing_mode {
        WritingMode::HorizontalTb => FlexItemPhysicalIntrinsicSizes {
            preferred_width: logical_inline_size,
            preferred_min_width: logical_min_inline_size,
            intrinsic_content_height: logical_block_size,
            min_content_height: logical_block_size.max(0.0),
        },
        WritingMode::VerticalRl | WritingMode::VerticalLr => FlexItemPhysicalIntrinsicSizes {
            preferred_width: logical_block_size,
            preferred_min_width: logical_block_size.max(0.0),
            intrinsic_content_height: logical_inline_size,
            min_content_height: logical_min_inline_size.max(0.0),
        },
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Estimates the hypothetical content size of a flex item.
    ///
    /// CSS Flexbox defines flex base sizes and intrinsic contributions before
    /// the flex formatting algorithm distributes free space. Inline
    /// contributions are measured through `InlineOpportunityGraph` so flex
    /// estimates use the same CSS Text break opportunities as inline layout:
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>,
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>, and
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>.
    pub(in crate::layout::flex) fn estimate_flex_item_size(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        available: FlexItemAvailableSpace,
    ) -> FlexItemEstimate {
        let percentage_height_basis =
            flex_item_estimate_percentage_height_basis(&child.style, available);
        self.with_flex_item_percentage_height_basis(percentage_height_basis, |layout| {
            layout.estimate_flex_item_size_with_percentage_basis(child, stylesheets, available)
        })
    }

    pub(in crate::layout::flex) fn estimate_flex_item_size_with_percentage_basis(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        available: FlexItemAvailableSpace,
    ) -> FlexItemEstimate {
        let style = &child.style;
        let containing_width = available.width;
        let containing_width_basis = available.width_is_definite.then_some(containing_width);
        let containing_inline_size = available.inline_size(style);
        let containing_inline_basis = available.inline_basis(style);
        if let Some(children) = child.anonymous_content() {
            let measurement = self.anonymous_flex_inline_measurement(
                children,
                style,
                stylesheets,
                containing_inline_size.max(1.0),
            );
            let contribution = measurement.contribution;
            let logical_inline_size = contribution.max_content.max(style.font_size * 0.25);
            let logical_min_inline_size = contribution.min_content.max(style.font_size * 0.25);
            let used_line_height = self.font_system.used_line_height(style);
            let logical_block_size = measurement.height().max(used_line_height);
            let physical_intrinsic = flex_item_physical_intrinsic_sizes(
                style.writing_mode,
                logical_inline_size,
                logical_min_inline_size,
                logical_block_size,
            );
            let content_width = used_length_percentage_or_auto_with_optional_basis(
                style.box_values.width,
                containing_width_basis,
            )
            .unwrap_or(physical_intrinsic.preferred_width);
            let content_height =
                used_length_percentage_or_auto(style.box_values.height, used_line_height.max(1.0))
                    .unwrap_or(physical_intrinsic.intrinsic_content_height);
            let width = constrain_width(style, content_width, containing_width);
            let height = constrain_height(style, content_height, containing_width);
            let min_width = constrain_width(
                style,
                physical_intrinsic.preferred_min_width,
                containing_width,
            );
            let min_height = constrain_height(
                style,
                physical_intrinsic.min_content_height,
                containing_width,
            );
            let fallback_line_baseline_offset =
                self.inline_box_text_line_layout_baseline_offset(style);
            let first_line_baseline_offset = first_sequence_line_baseline_offset(
                &measurement.sequence,
                fallback_line_baseline_offset,
            );
            let last_line_baseline_offset = last_sequence_line_baseline_offset(
                &measurement.sequence,
                fallback_line_baseline_offset,
            );
            let preceding_line_height = preceding_line_height_before_last(&measurement.sequence);
            return FlexItemEstimate {
                width: content_box_pt(width),
                height: content_box_pt(height),
                min_width: content_box_pt(min_width),
                min_height: content_box_pt(min_height),
                content_width: content_box_pt(physical_intrinsic.preferred_width),
                content_height: content_box_pt(physical_intrinsic.intrinsic_content_height),
                preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                first_baseline: Some(first_text_baseline_offset(&mut self.font_system, style)),
                last_baseline: Some(last_text_baseline_offset(
                    &mut self.font_system,
                    style,
                    measurement.line_count().max(1),
                )),
                first_horizontal_baseline: first_horizontal_text_baseline_offset(
                    style,
                    width,
                    first_line_baseline_offset,
                ),
                last_horizontal_baseline: last_horizontal_text_baseline_offset(
                    style,
                    width,
                    preceding_line_height,
                    last_line_baseline_offset,
                ),
            };
        }

        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return FlexItemEstimate::fixed(0.0, 0.0);
        };
        if style.display.is_flex() {
            let intrinsic_size = self.with_ancestor_signature(signature.clone(), |layout| {
                let built_child_boxes;
                let child_boxes = if let Some(child_boxes) = child_boxes {
                    child_boxes
                } else {
                    built_child_boxes = layout.build_frozen_child_boxes_with_current_ancestors(
                        element,
                        stylesheets,
                        style,
                    );
                    &built_child_boxes
                };
                let children = flex_children_from_boxes(element, signature, style, child_boxes);
                (!children.is_empty()).then(|| {
                    layout.estimate_intrinsic_flex_container_size(
                        &children,
                        style,
                        stylesheets,
                        FlexAvailableSpace {
                            width: containing_width,
                            width_is_definite: used_length_percentage_or_auto_with_optional_basis(
                                style.box_values.width,
                                containing_width_basis,
                            )
                            .is_some(),
                            height: used_length_percentage_or_auto(
                                style.box_values.height,
                                containing_width,
                            ),
                            height_is_definite: !style.box_values.height.is_auto(),
                        },
                    )
                })
            });
            if let Some(intrinsic_size) = intrinsic_size {
                let content_width = used_length_percentage_or_auto_with_optional_basis(
                    style.box_values.width,
                    containing_width_basis,
                )
                .unwrap_or_else(|| intrinsic_size.width.points())
                .max(style.font_size);
                let used_line_height = self.font_system.used_line_height(style);
                let content_height =
                    used_length_percentage_or_auto(style.box_values.height, containing_width)
                        .unwrap_or_else(|| intrinsic_size.height.points())
                        .max(used_line_height);
                return FlexItemEstimate {
                    width: content_box_pt(constrain_width(style, content_width, containing_width)),
                    height: content_box_pt(constrain_height(
                        style,
                        content_height,
                        containing_width,
                    )),
                    min_width: content_box_pt(constrain_width(
                        style,
                        intrinsic_size.min_width.points(),
                        containing_width,
                    )),
                    min_height: content_box_pt(constrain_height(
                        style,
                        intrinsic_size.min_height.points(),
                        containing_width,
                    )),
                    content_width: intrinsic_size.content_width,
                    content_height: intrinsic_size.content_height,
                    preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                    first_baseline: intrinsic_size.first_baseline,
                    last_baseline: intrinsic_size.last_baseline,
                    first_horizontal_baseline: intrinsic_size.first_horizontal_baseline,
                    last_horizontal_baseline: intrinsic_size.last_horizontal_baseline,
                };
            }
        }

        if replaced_element_kind(element) == Some(ReplacedElementKind::Image)
            && let Some(size) = estimate_replaced_image_flex_item(
                element,
                style,
                containing_width,
                available,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
        {
            return size;
        }

        if replaced_element_kind(element) == Some(ReplacedElementKind::Image)
            && let Some(image) = used_image(
                element,
                style,
                containing_width,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
        {
            let content_size = image.content_size;
            return FlexItemEstimate::fixed(
                content_size.width.max(1.0),
                content_size.height.max(1.0),
            );
        }

        if replaced_element_kind(element) == Some(ReplacedElementKind::Svg)
            && let Some((width, height, _)) = svg_rect(element)
        {
            return FlexItemEstimate::fixed(width.max(1.0), height.max(1.0));
        }

        if has_direct_inline_replaced_child(element)
            && !has_direct_flow_child_with_font_metrics(
                element,
                style,
                stylesheets,
                &mut self.font_system,
            )
        {
            let (row_width, row_height) =
                self.measure_direct_inline_row(element, style, stylesheets);
            let content_width = used_length_percentage_or_auto_with_optional_basis(
                style.box_values.width,
                containing_width_basis,
            )
            .unwrap_or(row_width);
            let content_height =
                used_length_percentage_or_auto(style.box_values.height, containing_width)
                    .unwrap_or(row_height);
            let width = constrain_width(style, content_width, containing_width);
            let height = constrain_height(style, content_height, containing_width);
            let min_width = constrain_width(style, row_width, containing_width);
            let min_height = constrain_height(style, row_height, containing_width);
            let line_baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
            return FlexItemEstimate {
                width: content_box_pt(width),
                height: content_box_pt(height),
                min_width: content_box_pt(min_width),
                min_height: content_box_pt(min_height),
                content_width: content_box_pt(row_width),
                content_height: content_box_pt(row_height),
                preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                first_baseline: Some(first_text_baseline_offset(&mut self.font_system, style)),
                last_baseline: Some(last_text_baseline_offset(&mut self.font_system, style, 1)),
                first_horizontal_baseline: first_horizontal_text_baseline_offset(
                    style,
                    width,
                    line_baseline_offset,
                ),
                last_horizontal_baseline: last_horizontal_text_baseline_offset(
                    style,
                    width,
                    0.0,
                    line_baseline_offset,
                ),
            };
        }

        let definition_list_column_height =
            self.estimate_definition_list_column_height(child, stylesheets, containing_inline_size);
        let inline_content_width =
            (containing_inline_size - style.padding.left - style.padding.right).max(1.0);
        let inline_measurement =
            self.estimate_child_inline_measurement(child, stylesheets, inline_content_width);
        let child_intrinsic = self.estimate_child_intrinsic_widths(
            child,
            stylesheets,
            containing_inline_size,
            inline_measurement.contribution,
        );
        let child_preferred_block_height = self.estimate_child_min_content_block_size(
            child,
            stylesheets,
            containing_inline_size,
            inline_measurement.height(),
        );

        let logical_inline_size = child_intrinsic.max_content;
        let logical_min_inline_size = child_intrinsic.min_content;
        let used_line_height = style.line_height;
        let fallback_logical_block_size = if inline_measurement.line_count() > 0 {
            inline_measurement
                .height()
                .max(inline_measurement.line_count() as f32 * used_line_height)
        } else if element.children.is_empty() && child_intrinsic.max_content == 0.0 {
            // CSS Flexbox determines the hypothetical cross size by laying the
            // item out as a block. A genuinely empty block has zero content
            // height, allowing align-content/stretch to distribute the flex
            // container's cross size across wrapped lines.
            // https://www.w3.org/TR/css-flexbox-1/#algo-cross-item
            0.0
        } else {
            used_line_height
        };
        let logical_block_size = definition_list_column_height
            .unwrap_or_else(|| fallback_logical_block_size.max(child_preferred_block_height));
        let mut physical_intrinsic = flex_item_physical_intrinsic_sizes(
            style.writing_mode,
            logical_inline_size,
            logical_min_inline_size,
            logical_block_size,
        );
        if (inline_measurement.line_count() == 0 || inline_measurement.line_count() > 1)
            && matches!(
                style.writing_mode,
                WritingMode::VerticalRl | WritingMode::VerticalLr
            )
        {
            physical_intrinsic.preferred_width = physical_intrinsic
                .preferred_width
                .max(child_intrinsic.max_content);
            physical_intrinsic.preferred_min_width = physical_intrinsic
                .preferred_min_width
                .max(child_intrinsic.min_content);
        }
        let mut content_width = used_length_percentage_or_auto_with_optional_basis(
            style.box_values.width,
            containing_inline_basis.or(containing_width_basis),
        )
        .unwrap_or(physical_intrinsic.preferred_width);
        let block_height_probe_width = if style.box_values.width.is_auto() {
            containing_inline_size
        } else {
            content_width
        };
        if let Some(block_height) = self.measure_flex_item_auto_block_height_for_flex_basis(
            element,
            style,
            stylesheets,
            child_boxes,
            block_height_probe_width,
            inline_measurement.line_count(),
        ) && matches!(style.writing_mode, WritingMode::HorizontalTb)
        {
            physical_intrinsic.intrinsic_content_height = block_height;
            physical_intrinsic.min_content_height = block_height.max(0.0);
        }
        if style.box_values.height.is_auto()
            && matches!(style.writing_mode, WritingMode::HorizontalTb)
            && let Some(multicol_height) = self.estimate_child_multicol_inline_height(
                child,
                stylesheets,
                constrain_width(style, content_width, containing_width),
            )
        {
            physical_intrinsic.intrinsic_content_height = multicol_height;
            physical_intrinsic.min_content_height = multicol_height.max(0.0);
        }
        let mut content_height =
            used_length_percentage_or_auto(style.box_values.height, used_line_height.max(1.0))
                .unwrap_or(physical_intrinsic.intrinsic_content_height);
        if let Some(ratio) = style.aspect_ratio.preferred_ratio_for_non_replaced(false) {
            match (
                style.box_values.width.is_auto(),
                style.box_values.height.is_auto(),
            ) {
                (false, true) => {
                    let transferred_height = content_width / ratio;
                    if inline_measurement.line_count() == 0 && element.children.is_empty() {
                        physical_intrinsic.intrinsic_content_height = transferred_height;
                        physical_intrinsic.min_content_height = transferred_height;
                        content_height = transferred_height;
                    } else {
                        content_height = content_height.max(transferred_height);
                        physical_intrinsic.min_content_height = physical_intrinsic
                            .min_content_height
                            .max(transferred_height);
                    }
                }
                (true, false) => {
                    let transferred_width = content_height * ratio;
                    if inline_measurement.line_count() == 0 && element.children.is_empty() {
                        content_width = transferred_width;
                    } else {
                        content_width = content_width.max(transferred_width);
                    }
                    if matches!(
                        style.writing_mode,
                        WritingMode::VerticalRl | WritingMode::VerticalLr
                    ) {
                        if inline_measurement.line_count() == 0 && element.children.is_empty() {
                            physical_intrinsic.min_content_height = content_height;
                        } else {
                            physical_intrinsic.min_content_height =
                                physical_intrinsic.min_content_height.max(content_height);
                        }
                    } else {
                        physical_intrinsic.min_content_height =
                            physical_intrinsic.min_content_height.max(content_height);
                    }
                }
                _ => {}
            }
        }

        let width = constrain_width(style, content_width, containing_width);
        let height = constrain_height(style, content_height, containing_width);
        let min_width = constrain_width(
            style,
            physical_intrinsic.preferred_min_width,
            containing_width,
        );
        let min_height = constrain_height(
            style,
            physical_intrinsic.min_content_height,
            containing_width,
        );
        let fallback_line_baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
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
                containing_width,
            )
        } else {
            FlexItemBaselineEstimate::default()
        };

        FlexItemEstimate {
            width: content_box_pt(width),
            height: content_box_pt(height),
            min_width: content_box_pt(min_width),
            min_height: content_box_pt(min_height),
            content_width: content_box_pt(physical_intrinsic.preferred_width),
            content_height: content_box_pt(physical_intrinsic.intrinsic_content_height),
            preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
            first_baseline: (inline_measurement.line_count() > 0)
                .then(|| first_text_baseline_offset(&mut self.font_system, style))
                .or(descendant_baselines.first_baseline),
            last_baseline: (inline_measurement.line_count() > 0)
                .then(|| {
                    last_text_baseline_offset(
                        &mut self.font_system,
                        style,
                        inline_measurement.line_count(),
                    )
                })
                .or(descendant_baselines.last_baseline),
            first_horizontal_baseline: (inline_measurement.line_count() > 0)
                .then(|| {
                    first_horizontal_text_baseline_offset(style, width, first_line_baseline_offset)
                })
                .flatten()
                .or(descendant_baselines.first_horizontal_baseline),
            last_horizontal_baseline: (inline_measurement.line_count() > 0)
                .then(|| {
                    last_horizontal_text_baseline_offset(
                        style,
                        width,
                        preceding_line_height,
                        last_line_baseline_offset,
                    )
                })
                .flatten()
                .or(descendant_baselines.last_horizontal_baseline),
        }
    }

    /// Estimate an anonymous flex text item's graph-backed inline measurement.
    ///
    /// CSS Flexbox wraps non-collapsible text in anonymous flex items, and CSS
    /// Sizing defines those items' min/max-content contributions from the same
    /// CSS Text break opportunities and line fragments used for line layout:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-items>,
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic>, and
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>.
    pub(in crate::layout::flex) fn anonymous_flex_inline_measurement(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        self.intrinsic_inline_measurement_for_boxes(children, style, stylesheets, available_width)
    }

    pub(in crate::layout::flex) fn estimate_child_inline_measurement(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return child
                .anonymous_content()
                .map(|children| {
                    self.anonymous_flex_inline_measurement(
                        children,
                        &child.style,
                        stylesheets,
                        available_width,
                    )
                })
                .unwrap_or_default();
        };
        self.with_ancestor_signature(signature.clone(), |layout| {
            layout.intrinsic_inline_measurement_for_element(
                element,
                &child.style,
                stylesheets,
                child_boxes,
                available_width,
            )
        })
    }

    /// Estimate the balanced content block-size of simple inline multicol flex items.
    ///
    /// CSS Flexbox computes a flex item's hypothetical cross size by laying the
    /// item out as an in-flow block-level box, while CSS Multi-column layout
    /// balances auto-height columns across their used column count:
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
    /// <https://www.w3.org/TR/css-multicol-1/#filling-columns>.
    pub(in crate::layout::flex) fn estimate_child_multicol_inline_height(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        content_width: f32,
    ) -> Option<f32> {
        let style = &child.style;
        let gap = used_multicol_column_gap(style.column_gap, content_width, style.font_size);
        let column_count =
            used_multicol_column_count(style, content_width, gap).filter(|count| *count > 1)?;
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((content_width - total_gap) / column_count as f32).max(1.0);
        let available_column_width =
            (column_width - style.padding.left - style.padding.right).max(1.0);
        let measurement =
            self.estimate_child_inline_measurement(child, stylesheets, available_column_width);

        (measurement.line_count() > 0).then(|| {
            measurement
                .sequence
                .balanced_multicolumn_height(column_count, style)
                .max(style.line_height)
        })
    }

    /// Estimates descendant min/max inline contributions from graph-backed fragments.
    ///
    /// CSS Sizing computes intrinsic inline sizes from the inline formatting
    /// input and CSS Text break opportunities. Flexbox consumes both values
    /// for flex base sizes and automatic minimum sizes:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
    pub(in crate::layout::flex) fn estimate_child_intrinsic_widths(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        containing_width: f32,
        inline_contribution: inline_layout::InlineIntrinsicContribution,
    ) -> inline_layout::InlineIntrinsicContribution {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return inline_contribution;
        };
        self.with_ancestor_signature(signature.clone(), |layout| {
            layout.estimate_element_flex_intrinsic_widths(
                element,
                &child.style,
                stylesheets,
                child_boxes,
                containing_width,
                inline_contribution,
            )
        })
    }

    pub(in crate::layout::flex) fn estimate_element_flex_intrinsic_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_width: f32,
        inline_contribution: inline_layout::InlineIntrinsicContribution,
    ) -> inline_layout::InlineIntrinsicContribution {
        let mut contribution = inline_contribution;
        if let Some(child_boxes) = child_boxes {
            let (float_min, float_max) = self.inline_float_run_intrinsic_widths_for_boxes(
                child_boxes,
                style,
                stylesheets,
                containing_width,
            );
            contribution.min_content = contribution.min_content.max(float_min);
            contribution.max_content = contribution.max_content.max(float_max);
            self.merge_box_children_flex_intrinsic_widths(
                child_boxes,
                stylesheets,
                containing_width,
                &mut contribution,
            );
        } else {
            self.merge_dom_children_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                containing_width,
                &mut contribution,
            );
        }
        contribution
    }

    pub(in crate::layout::flex) fn merge_box_children_flex_intrinsic_widths(
        &mut self,
        child_boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        containing_width: f32,
        contribution: &mut inline_layout::InlineIntrinsicContribution,
    ) {
        for child_box in child_boxes {
            let Some((child_element, signature, child_style, child_children)) =
                child_box.element_parts()
            else {
                continue;
            };
            if child_style.display.is_none()
                || matches!(child_style.position, Position::Absolute | Position::Fixed)
                || child_style.display.is_inline_level()
            {
                continue;
            }
            let child_contribution = explicit_child_intrinsic_width(child_style, containing_width)
                .unwrap_or_else(|| {
                    self.with_ancestor_signature(signature.clone(), |layout| {
                        layout.block_intrinsic_content_widths(
                            child_element,
                            child_style,
                            stylesheets,
                            Some(child_children),
                            containing_width,
                        )
                    })
                });
            merge_outer_intrinsic_widths(
                contribution,
                child_contribution,
                child_style,
                containing_width,
            );
        }
    }

    pub(in crate::layout::flex) fn merge_dom_children_flex_intrinsic_widths(
        &mut self,
        element: &Element,
        parent_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        containing_width: f32,
        contribution: &mut inline_layout::InlineIntrinsicContribution,
    ) {
        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        for node in &element.children {
            let NodeKind::Element(child_element) = &node.kind else {
                continue;
            };
            let signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                signature.clone(),
                stylesheets,
                Some(parent_style),
            );
            if child_style.display.is_none()
                || matches!(child_style.position, Position::Absolute | Position::Fixed)
                || child_style.display.is_inline_level()
            {
                continue;
            }
            let child_contribution = explicit_child_intrinsic_width(&child_style, containing_width)
                .unwrap_or_else(|| {
                    self.with_ancestor_signature(signature, |layout| {
                        layout.block_intrinsic_content_widths(
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                            containing_width,
                        )
                    })
                });
            merge_outer_intrinsic_widths(
                contribution,
                child_contribution,
                &child_style,
                containing_width,
            );
        }
    }

    /// Estimates the min-content block-size contribution from descendant boxes.
    ///
    /// CSS Sizing defines min-content sizes as intrinsic contributions, and CSS
    /// Flexbox uses those contributions when resolving intrinsic flex item size
    /// constraints such as `max-height: min-content`:
    /// <https://www.w3.org/TR/css-sizing-3/#min-content> and
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
    pub(in crate::layout::flex) fn estimate_child_min_content_block_size(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        containing_width: f32,
        inline_content_height: f32,
    ) -> f32 {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return inline_content_height;
        };
        self.with_ancestor_signature(signature.clone(), |layout| {
            layout.estimate_element_children_min_content_block_size(
                element,
                &child.style,
                stylesheets,
                child_boxes,
                containing_width,
                inline_content_height,
            )
        })
    }

    /// Recursively estimates descendant min-content block-size contributions.
    ///
    /// CSS Sizing's block-axis min-content contribution for normal block flow
    /// is the sum of in-flow block child outer sizes, with inline runs measured
    /// by graph-selected line fragments:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>,
    /// <https://www.w3.org/TR/css-inline-3/#line-box>, and
    /// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
    pub(in crate::layout::flex) fn estimate_element_children_min_content_block_size(
        &mut self,
        element: &Element,
        parent_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_width: f32,
        inline_content_height: f32,
    ) -> f32 {
        let mut block_size = inline_content_height;

        if let Some(child_boxes) = child_boxes {
            for child_box in child_boxes {
                let Some((child_element, signature, child_style, child_children)) =
                    child_box.element_parts()
                else {
                    continue;
                };
                if !flex_min_content_block_child_participates(child_element, child_style) {
                    continue;
                }
                block_size += self.with_ancestor_signature(signature.clone(), |layout| {
                    layout.flex_child_outer_min_content_block_size(
                        child_element,
                        child_style,
                        stylesheets,
                        Some(child_children),
                        containing_width,
                    )
                });
            }
            return block_size;
        }

        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        for node in &element.children {
            let NodeKind::Element(child_element) = &node.kind else {
                continue;
            };
            let signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                signature.clone(),
                stylesheets,
                Some(parent_style),
            );
            if !flex_min_content_block_child_participates(child_element, &child_style) {
                continue;
            }
            block_size += self.with_ancestor_signature(signature, |layout| {
                layout.flex_child_outer_min_content_block_size(
                    child_element,
                    &child_style,
                    stylesheets,
                    None,
                    containing_width,
                )
            });
        }
        block_size
    }

    pub(in crate::layout::flex) fn flex_child_outer_min_content_block_size(
        &mut self,
        child_element: &Element,
        child_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_width: f32,
    ) -> f32 {
        let vertical_non_content = child_style.padding.top
            + child_style.padding.bottom
            + vertical_border_width(child_style);
        let content_size =
            used_content_height_or_auto(child_style, containing_width, vertical_non_content)
                .unwrap_or_else(|| {
                    let inline_width =
                        (containing_width - child_style.padding.left - child_style.padding.right)
                            .max(1.0);
                    let inline_measurement = self.intrinsic_inline_measurement_for_element(
                        child_element,
                        child_style,
                        stylesheets,
                        child_boxes,
                        inline_width,
                    );
                    self.estimate_element_children_min_content_block_size(
                        child_element,
                        child_style,
                        stylesheets,
                        child_boxes,
                        containing_width,
                        inline_measurement.height(),
                    )
                });
        let constrained_content_size =
            constrain_height(child_style, content_size, containing_width);
        let border_widths = used_border_widths(child_style);
        child_style.margin.top
            + child_style.padding.top
            + border_widths.top
            + constrained_content_size
            + child_style.padding.bottom
            + border_widths.bottom
            + child_style.margin.bottom
    }

    /// Measure an auto-height flex item as block layout when float placement affects flex basis.
    ///
    /// CSS Flexbox computes `flex-basis:auto` from the item's max-content size
    /// when the main-size property is automatic. For a column flex item, that
    /// means the content block size after laying out floats in the available
    /// inline size, not just the sum of each float's standalone block size:
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout::flex) fn measure_flex_item_auto_block_height_for_flex_basis(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        content_width: f32,
        inline_line_count: usize,
    ) -> Option<f32> {
        if !style.box_values.height.is_auto()
            || !style.display.is_block_level()
            || !matches!(
                style.display.inner,
                DisplayInner::Flow | DisplayInner::FlowRoot
            )
            || inline_line_count > 0
        {
            return None;
        }
        if !child_boxes
            .map(flex_item_child_boxes_include_float)
            .unwrap_or(false)
        {
            return None;
        }

        let mut measured_style = style.clone();
        measured_style.display.inner = DisplayInner::FlowRoot;
        Some(self.measure_auto_positioned_block_height(
            element,
            &measured_style,
            stylesheets,
            content_width.max(0.0),
            child_boxes,
            None,
        ))
    }

    /// Estimate a flex container's intrinsic size and exported row baselines.
    ///
    /// CSS Flexbox defines intrinsic flex container sizes from flex-item
    /// contributions and exports first/last main-axis baselines from row flex
    /// lines for parent baseline alignment:
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
    pub(in crate::layout::flex) fn estimate_intrinsic_flex_container_size(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
    ) -> FlexItemEstimate {
        let physical_direction = physical_flex_direction(style);
        let border_widths = used_border_widths(style);
        let (physical_gap_width, physical_gap_height) = physical_flex_gaps(style);
        let (intrinsic_items, estimated_baseline_items) = self.estimate_flex_intrinsic_items(
            children,
            style,
            stylesheets,
            available,
            physical_direction,
        );

        let intrinsic_main_gap =
            estimated_intrinsic_flex_gap(if physical_direction.is_column_axis() {
                physical_gap_height
            } else {
                physical_gap_width
            });
        let intrinsic_cross_gap =
            estimated_intrinsic_flex_gap(if physical_direction.is_column_axis() {
                physical_gap_width
            } else {
                physical_gap_height
            });
        let min_main = intrinsic_flex_container_min_main_size(
            style,
            physical_direction,
            &intrinsic_items,
            intrinsic_main_gap,
            available,
        );
        let max_main = intrinsic_flex_container_max_main_size(
            style,
            physical_direction,
            &intrinsic_items,
            intrinsic_main_gap,
            available,
        );
        let (min_cross, mut max_cross) = intrinsic_flex_container_cross_sizes(
            style,
            physical_direction,
            &intrinsic_items,
            intrinsic_cross_gap,
            available,
            min_main,
            max_main,
        );
        if style.flex_direction.is_column_axis() && style.flex_wrap != FlexWrap::NoWrap {
            let available_cross_size = intrinsic_items
                .iter()
                .map(|item| item.max_cross_contribution)
                .fold(0.0f32, f32::max);
            if available_cross_size > 0.0 {
                let constrained_available = flex_available_with_definite_cross_size(
                    available,
                    physical_direction,
                    available_cross_size,
                );
                let (max_content_items, _) = self.estimate_flex_intrinsic_items(
                    children,
                    style,
                    stylesheets,
                    constrained_available,
                    physical_direction,
                );
                let max_content_min_main = intrinsic_flex_container_min_main_size(
                    style,
                    physical_direction,
                    &max_content_items,
                    intrinsic_main_gap,
                    constrained_available,
                );
                let max_content_max_main = intrinsic_flex_container_max_main_size(
                    style,
                    physical_direction,
                    &max_content_items,
                    intrinsic_main_gap,
                    constrained_available,
                );
                let (_, max_content_cross) = intrinsic_flex_container_cross_sizes(
                    style,
                    physical_direction,
                    &max_content_items,
                    intrinsic_cross_gap,
                    constrained_available,
                    max_content_min_main,
                    max_content_max_main,
                );
                max_cross = max_content_cross;
            }
        }
        let (mut min_width, mut width, mut min_height, mut height) =
            if physical_direction.is_column_axis() {
                (min_cross, max_cross, min_main, max_main)
            } else {
                (min_main, max_main, min_cross, max_cross)
            };

        let line_metrics =
            estimate_row_flex_container_line_metrics(style, available, &estimated_baseline_items);
        if let Some(metrics) = line_metrics
            && style.flex_wrap != FlexWrap::NoWrap
            && metrics.line_count > 1
        {
            if physical_direction.is_row_axis() {
                height = height.max(metrics.cross_size);
                min_height = min_height.max(metrics.cross_size);
            } else {
                width = width.max(metrics.cross_size);
                min_width = min_width.max(metrics.cross_size);
            }
        }

        let content_width = width;
        let content_height = height;
        let (first_baseline, last_baseline) = line_metrics
            .map(|metrics| (metrics.first_baseline, metrics.last_baseline))
            .unwrap_or((None, None));
        let (first_baseline, last_baseline, first_horizontal_baseline, last_horizontal_baseline) =
            if physical_direction.is_row_axis() {
                (
                    first_baseline.map(|baseline| border_widths.top + style.padding.top + baseline),
                    last_baseline.map(|baseline| border_widths.top + style.padding.top + baseline),
                    None,
                    None,
                )
            } else if style.flex_direction.is_row_axis() {
                (
                    None,
                    None,
                    first_baseline
                        .map(|baseline| border_widths.left + style.padding.left + baseline),
                    last_baseline
                        .map(|baseline| border_widths.left + style.padding.left + baseline),
                )
            } else {
                (None, None, None, None)
            };
        let width = used_length_percentage_or_auto_with_optional_basis(
            style.box_values.width,
            available.width_is_definite.then_some(available.width),
        )
        .unwrap_or(width)
        .max(style.font_size);
        let height = used_length_percentage_or_auto(
            style.box_values.height,
            available.height.unwrap_or(available.width),
        )
        .or_else(|| available.height.map(|height| height.max(0.0)))
        .unwrap_or(height)
        .max(style.line_height);

        FlexItemEstimate {
            width: content_box_pt(width),
            height: content_box_pt(height),
            min_width: content_box_pt(min_width.max(style.font_size)),
            min_height: content_box_pt(min_height.max(style.line_height)),
            content_width: content_box_pt(content_width),
            content_height: content_box_pt(content_height),
            preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
            first_baseline,
            last_baseline,
            first_horizontal_baseline,
            last_horizontal_baseline,
        }
    }

    fn estimate_flex_intrinsic_items(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
        physical_direction: FlexDirection,
    ) -> (Vec<FlexIntrinsicItem>, Vec<EstimatedFlexBaselineItem>) {
        let mut intrinsic_items = Vec::with_capacity(children.len());
        let mut estimated_baseline_items = Vec::with_capacity(children.len());
        let container_inline_size = FlexItemAvailableSpace::from_container(available)
            .inline_size(style)
            .max(0.0);

        for child in children {
            let item_available = estimated_flex_item_available_space(
                &child.style,
                style,
                physical_direction,
                available,
            );
            let size = self.estimate_flex_item_size(child, stylesheets, item_available);
            let item = FlexIntrinsicItem::new(
                child,
                size,
                physical_direction,
                available,
                container_inline_size,
            );
            let (first_baseline, last_baseline) =
                estimated_flex_item_cross_axis_baselines(size, physical_direction);
            estimated_baseline_items.push(EstimatedFlexBaselineItem {
                outer_main_size: item.flex_base_size,
                outer_cross_size: item.max_cross_contribution,
                margin_cross_start: if physical_direction.is_row_axis() {
                    child.style.margin.top
                } else {
                    child.style.margin.left
                },
                cross_alignment: estimated_flex_item_cross_alignment(&child.style, style),
                first_baseline,
                last_baseline,
            });
            intrinsic_items.push(item);
        }

        (intrinsic_items, estimated_baseline_items)
    }

    pub(in crate::layout::flex) fn estimate_definition_list_column_height(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        containing_width: f32,
    ) -> Option<f32> {
        let style = &child.style;
        let (element, _, child_boxes) = child.element_parts()?;
        if !is_definition_list_element(element) {
            return None;
        }

        let groups = child_boxes
            .map(definition_list_column_groups_from_boxes)
            .unwrap_or_else(|| {
                definition_list_column_groups_with_font_metrics(
                    element,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                )
            });
        if groups.is_empty() {
            return None;
        }

        let gap = used_multicol_column_gap(style.column_gap, containing_width, style.font_size);
        let column_count =
            used_multicol_column_count(style, containing_width, gap).filter(|count| *count > 1)?;
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((containing_width - total_gap) / column_count as f32).max(1.0);
        let mut column_heights = vec![0.0f32; column_count];

        for (group_index, group) in groups.iter().enumerate() {
            let column_index = group_index % column_count;
            for item in group {
                column_heights[column_index] +=
                    self.estimate_flex_column_item_height(item, stylesheets, column_width);
            }
        }

        column_heights.into_iter().reduce(f32::max)
    }

    pub(in crate::layout::flex) fn estimate_flex_column_item_height(
        &mut self,
        item: &DefinitionListColumnItem<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        let content_width =
            (available_width - item.style.padding.left - item.style.padding.right).max(1.0);
        let content_height = item
            .children
            .map(|children| {
                self.intrinsic_inline_block_metrics_for_boxes(
                    children,
                    &item.style,
                    stylesheets,
                    content_width,
                )
                .0
            })
            .unwrap_or_else(|| {
                self.intrinsic_inline_block_metrics_for_element(
                    item.element,
                    &item.style,
                    stylesheets,
                    None,
                    content_width,
                )
                .0
            })
            .max(self.font_system.used_line_height(&item.style));

        item.style.margin.top
            + vertical_border_width(&item.style)
            + item.style.padding.top
            + content_height
            + item.style.padding.bottom
            + item.style.margin.bottom
    }
}
