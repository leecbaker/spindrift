use super::*;

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
    pub(super) fn estimate_flex_item_size(
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
            let (preferred_width, preferred_min_width, intrinsic_content_height) =
                match style.writing_mode {
                    WritingMode::HorizontalTb => (
                        logical_inline_size,
                        logical_min_inline_size,
                        logical_block_size,
                    ),
                    WritingMode::VerticalRl | WritingMode::VerticalLr => {
                        (logical_block_size, logical_block_size, logical_inline_size)
                    }
                };
            let min_content_height = match style.writing_mode {
                WritingMode::HorizontalTb => intrinsic_content_height,
                WritingMode::VerticalRl | WritingMode::VerticalLr => logical_min_inline_size,
            };
            let content_width = used_length_percentage_or_auto_with_optional_basis(
                style.box_values.width,
                containing_width_basis,
            )
            .unwrap_or(preferred_width);
            let content_height =
                used_length_percentage_or_auto(style.box_values.height, used_line_height.max(1.0))
                    .unwrap_or(intrinsic_content_height);
            return FlexItemEstimate {
                width: constrain_width(style, content_width, containing_width),
                height: constrain_height(style, content_height, containing_width),
                min_width: constrain_width(style, preferred_min_width, containing_width),
                min_height: constrain_height(style, min_content_height, containing_width),
                content_width: preferred_width,
                content_height: intrinsic_content_height,
                first_baseline: Some(first_text_baseline_offset(&mut self.font_system, style)),
                last_baseline: Some(last_text_baseline_offset(
                    &mut self.font_system,
                    style,
                    measurement.line_count().max(1),
                )),
                first_horizontal_baseline: first_horizontal_text_baseline_offset(style),
                last_horizontal_baseline: last_horizontal_text_baseline_offset(
                    style,
                    measurement.line_count().max(1),
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
                    built_child_boxes =
                        box_tree::build_child_boxes(element, stylesheets, style, &layout.ancestors);
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
                .unwrap_or(intrinsic_size.width)
                .max(style.font_size);
                let used_line_height = self.font_system.used_line_height(style);
                let content_height =
                    used_length_percentage_or_auto(style.box_values.height, containing_width)
                        .unwrap_or(intrinsic_size.height)
                        .max(used_line_height);
                return FlexItemEstimate {
                    width: constrain_width(style, content_width, containing_width),
                    height: constrain_height(style, content_height, containing_width),
                    min_width: constrain_width(style, intrinsic_size.min_width, containing_width),
                    min_height: constrain_height(
                        style,
                        intrinsic_size.min_height,
                        containing_width,
                    ),
                    content_width: intrinsic_size.content_width,
                    content_height: intrinsic_size.content_height,
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
            return FlexItemEstimate::fixed(
                image.content_width.max(1.0),
                image.content_height.max(1.0),
            );
        }

        if replaced_element_kind(element) == Some(ReplacedElementKind::Svg)
            && let Some((width, height, _)) = svg_rect(element)
        {
            return FlexItemEstimate::fixed(width.max(1.0), height.max(1.0));
        }

        if has_direct_inline_replaced_child(element)
            && !has_direct_flow_child(element, style, stylesheets)
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
            return FlexItemEstimate {
                width: constrain_width(style, content_width, containing_width),
                height: constrain_height(style, content_height, containing_width),
                min_width: constrain_width(style, row_width, containing_width),
                min_height: constrain_height(style, row_height, containing_width),
                content_width: row_width,
                content_height: row_height,
                first_baseline: Some(first_text_baseline_offset(&mut self.font_system, style)),
                last_baseline: Some(last_text_baseline_offset(&mut self.font_system, style, 1)),
                first_horizontal_baseline: first_horizontal_text_baseline_offset(style),
                last_horizontal_baseline: last_horizontal_text_baseline_offset(style, 1),
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
        let (preferred_width, preferred_min_width, intrinsic_content_height, min_content_height) =
            match style.writing_mode {
                WritingMode::HorizontalTb => (
                    logical_inline_size,
                    logical_min_inline_size,
                    logical_block_size,
                    logical_block_size.max(0.0),
                ),
                // Block descendants report physical inline-width
                // contributions. In vertical writing modes that physical
                // width is the flex item's logical block size and therefore
                // its physical main-size contribution in a row flex container:
                // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
                // and <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
                WritingMode::VerticalRl | WritingMode::VerticalLr => (
                    logical_block_size.max(child_intrinsic.max_content),
                    logical_block_size.max(child_intrinsic.min_content),
                    logical_inline_size,
                    logical_min_inline_size.max(0.0),
                ),
            };
        let content_width = used_length_percentage_or_auto_with_optional_basis(
            style.box_values.width,
            containing_inline_basis.or(containing_width_basis),
        )
        .unwrap_or(preferred_width);
        let content_height =
            used_length_percentage_or_auto(style.box_values.height, used_line_height.max(1.0))
                .unwrap_or(intrinsic_content_height);

        FlexItemEstimate {
            width: constrain_width(style, content_width, containing_width),
            height: constrain_height(style, content_height, containing_width),
            min_width: constrain_width(style, preferred_min_width, containing_width),
            min_height: constrain_height(style, min_content_height, containing_width),
            content_width: preferred_width,
            content_height: intrinsic_content_height,
            first_baseline: (inline_measurement.line_count() > 0)
                .then(|| first_text_baseline_offset(&mut self.font_system, style)),
            last_baseline: (inline_measurement.line_count() > 0).then(|| {
                last_text_baseline_offset(
                    &mut self.font_system,
                    style,
                    inline_measurement.line_count(),
                )
            }),
            first_horizontal_baseline: (inline_measurement.line_count() > 0)
                .then(|| first_horizontal_text_baseline_offset(style))
                .flatten(),
            last_horizontal_baseline: (inline_measurement.line_count() > 0)
                .then(|| {
                    last_horizontal_text_baseline_offset(style, inline_measurement.line_count())
                })
                .flatten(),
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
    fn anonymous_flex_inline_measurement(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        self.intrinsic_inline_measurement_for_boxes(children, style, stylesheets, available_width)
    }

    fn estimate_child_inline_measurement(
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

    /// Estimates descendant min/max inline contributions from graph-backed fragments.
    ///
    /// CSS Sizing computes intrinsic inline sizes from the inline formatting
    /// input and CSS Text break opportunities. Flexbox consumes both values
    /// for flex base sizes and automatic minimum sizes:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
    fn estimate_child_intrinsic_widths(
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

    fn estimate_element_flex_intrinsic_widths(
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

    fn merge_box_children_flex_intrinsic_widths(
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

    fn merge_dom_children_flex_intrinsic_widths(
        &mut self,
        element: &Element,
        parent_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        containing_width: f32,
        contribution: &mut inline_layout::InlineIntrinsicContribution,
    ) {
        let sibling_tags = element_sibling_tags(element);
        let mut element_index = 0usize;
        for node in &element.children {
            let NodeKind::Element(child_element) = &node.kind else {
                continue;
            };
            let signature = ElementSignature::with_siblings(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = style_for_layout_element(
                child_element,
                signature.clone(),
                stylesheets,
                Some(parent_style),
                &self.ancestors,
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
    fn estimate_child_min_content_block_size(
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
    fn estimate_element_children_min_content_block_size(
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

        let sibling_tags = element_sibling_tags(element);
        let mut element_index = 0usize;
        for node in &element.children {
            let NodeKind::Element(child_element) = &node.kind else {
                continue;
            };
            let signature = ElementSignature::with_siblings(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = style_for_layout_element(
                child_element,
                signature.clone(),
                stylesheets,
                Some(parent_style),
                &self.ancestors,
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

    fn flex_child_outer_min_content_block_size(
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

    /// Estimate a flex container's intrinsic size and exported row baselines.
    ///
    /// CSS Flexbox defines intrinsic flex container sizes from flex-item
    /// contributions and exports first/last main-axis baselines from row flex
    /// lines for parent baseline alignment:
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
    pub(super) fn estimate_intrinsic_flex_container_size(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
    ) -> FlexItemEstimate {
        let border_widths = used_border_widths(style);
        let mut intrinsic_items = Vec::with_capacity(children.len());
        let mut estimated_baseline_items = Vec::with_capacity(children.len());
        let physical_direction = physical_flex_direction(style);
        let (physical_gap_width, physical_gap_height) = physical_flex_gaps(style);
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
        let (min_cross, max_cross) = intrinsic_flex_container_cross_sizes(
            style,
            physical_direction,
            &intrinsic_items,
            intrinsic_cross_gap,
            available,
            min_main,
            max_main,
        );
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
            width,
            height,
            min_width: min_width.max(style.font_size),
            min_height: min_height.max(style.line_height),
            content_width,
            content_height,
            first_baseline,
            last_baseline,
            first_horizontal_baseline,
            last_horizontal_baseline,
        }
    }

    pub(super) fn estimate_definition_list_column_height(
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
                definition_list_column_groups(element, style, stylesheets, &self.ancestors)
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

    pub(super) fn estimate_flex_column_item_height(
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

/// Estimates a replaced image flex item without letting min-size alter flex basis.
///
/// CSS Flexbox computes the flex base size from the item's used flex-basis
/// while ignoring min/max main-size constraints, but the hypothetical size and
/// cross-size contribution still reflect replaced-element aspect-ratio sizing:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item> and
/// <https://www.w3.org/TR/css-sizing-3/#aspect-ratio>.
fn estimate_replaced_image_flex_item(
    element: &Element,
    style: &ComputedStyle,
    containing_width: f32,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Option<FlexItemEstimate> {
    let intrinsic = intrinsic_image_size(element, base_url, root_url, resource_cache)?;
    let aspect_ratio = intrinsic.width / intrinsic.height;
    if aspect_ratio <= 0.0 {
        return None;
    }
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let specified_width =
        used_content_width_or_auto(style, containing_width, horizontal_non_content)
            .or(intrinsic.attr_width);
    let specified_height =
        definite_image_content_height_without_percent(style, vertical_non_content)
            .or(intrinsic.attr_height);
    let width_is_auto = specified_width.is_none();
    let height_is_auto = specified_height.is_none();
    let (base_width, base_height) = match (specified_width, specified_height) {
        (Some(width), None) => (width, width / aspect_ratio),
        (None, Some(height)) => (height * aspect_ratio, height),
        (None, None) => (intrinsic.width, intrinsic.height),
        (Some(width), Some(height)) => (width, height),
    };
    let mut width = base_width;
    let mut height = base_height;
    constrain_replaced_size_with_aspect_ratio(
        &mut width,
        &mut height,
        aspect_ratio,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        ReplacedSizeConstraints {
            min_width: used_min_width(style, containing_width),
            max_width: used_max_width(style, containing_width),
            min_height: used_min_height(style, containing_width),
            max_height: used_max_height(style, containing_width),
        },
    );
    Some(FlexItemEstimate {
        width: width.max(1.0),
        height: height.max(1.0),
        min_width: width.max(1.0),
        min_height: height.max(1.0),
        content_width: base_width.max(1.0),
        content_height: base_height.max(1.0),
        first_baseline: None,
        last_baseline: None,
        first_horizontal_baseline: None,
        last_horizontal_baseline: None,
    })
}

/// Return a flex item's first text baseline offset from its border-box top.
///
/// CSS Flexbox baseline alignment uses the participating item's first baseline
/// set when the cross axis is parallel to the block axis. Text painting applies
/// the selected font's ascender correction, so flex layout uses the same metric
/// projection as table-cell baseline alignment:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
fn first_text_baseline_offset(font_system: &mut FontSystem, style: &ComputedStyle) -> f32 {
    let borders = used_border_widths(style);
    borders.top + style.padding.top + font_system.rendered_first_line_baseline_offset(style)
}

/// Return a flex item's last text baseline offset from its border-box top.
///
/// CSS Flexbox permits first and last baseline alignment of flex items. For
/// the horizontal writing mode currently implemented here, line boxes are
/// stacked by the used `line-height`, so the last baseline is the first text
/// baseline plus one line advance for each following line:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
fn last_text_baseline_offset(
    font_system: &mut FontSystem,
    style: &ComputedStyle,
    line_count: usize,
) -> f32 {
    first_text_baseline_offset(font_system, style)
        + line_count.saturating_sub(1) as f32 * style.line_height
}

/// Return a vertical-writing flex item's first text baseline offset from its
/// border-box left edge.
///
/// CSS Flexbox baseline alignment can align row flex lines in the horizontal
/// cross axis when the row main axis is vertical. Quire's vertical inline
/// painter records text groups at the content left edge, so the flex estimate
/// exposes the same physical x-coordinate as a border-box offset:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
fn first_horizontal_text_baseline_offset(style: &ComputedStyle) -> Option<f32> {
    if style.writing_mode == WritingMode::HorizontalTb {
        return None;
    }
    let borders = used_border_widths(style);
    Some(borders.left + style.padding.left)
}

/// Return a vertical-writing flex item's last text baseline offset from its
/// border-box left edge.
///
/// Quire currently stores vertical inline lines at their physical text-group
/// x-coordinate; this mirrors that coordinate for flex baseline sharing and
/// keeps multi-line vertical baseline export conservative until vertical line
/// fragmentation exposes durable per-line horizontal positions.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
fn last_horizontal_text_baseline_offset(style: &ComputedStyle, _line_count: usize) -> Option<f32> {
    first_horizontal_text_baseline_offset(style)
}

fn merge_outer_intrinsic_widths(
    contribution: &mut inline_layout::InlineIntrinsicContribution,
    child_contribution: (f32, f32),
    child_style: &ComputedStyle,
    containing_inline_size: f32,
) {
    let outer_edges = intrinsic_horizontal_outer_edges(child_style, containing_inline_size);
    contribution.min_content = contribution
        .min_content
        .max(child_contribution.0 + outer_edges);
    contribution.max_content = contribution
        .max_content
        .max(child_contribution.1 + outer_edges);
}

fn explicit_child_intrinsic_width(
    child_style: &ComputedStyle,
    containing_inline_size: f32,
) -> Option<(f32, f32)> {
    let horizontal_extras = intrinsic_horizontal_non_content(child_style, containing_inline_size);
    used_content_width_or_auto(child_style, containing_inline_size, horizontal_extras)
        .map(|width| (width, width))
}

fn intrinsic_horizontal_non_content(style: &ComputedStyle, containing_inline_size: f32) -> f32 {
    let padding = used_padding_edges(style, containing_inline_size);
    horizontal_border_width(style) + padding.left + padding.right
}

fn intrinsic_horizontal_outer_edges(style: &ComputedStyle, containing_inline_size: f32) -> f32 {
    let margin = used_margin_edges(style, containing_inline_size);
    intrinsic_horizontal_non_content(style, containing_inline_size) + margin.left + margin.right
}

fn flex_min_content_block_child_participates(element: &Element, style: &ComputedStyle) -> bool {
    !style.display.is_none()
        && !matches!(style.position, Position::Absolute | Position::Fixed)
        && (style.display.is_block_level()
            || is_document_canvas_element(element)
            || is_replaced_element(element))
}

/// Intrinsic contribution record for one flex item.
///
/// CSS Flexbox defines flex container intrinsic sizes in terms of each item's
/// outer min/max-content contribution, flex base size, hypothetical main size,
/// and grow/shrink factor. Keeping those values explicit avoids reusing one
/// estimated layout size for several distinct spec concepts:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes> and
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>.
#[derive(Debug, Clone, Copy)]
struct FlexIntrinsicItem {
    min_main_contribution: f32,
    max_main_contribution: f32,
    min_cross_contribution: f32,
    max_cross_contribution: f32,
    flex_base_size: f32,
    hypothetical_main_size: f32,
    grow: f32,
    shrink: f32,
}

impl FlexIntrinsicItem {
    fn new(
        child: &StyledChild<'_>,
        size: FlexItemEstimate,
        direction: FlexDirection,
        available: FlexAvailableSpace,
        container_inline_size: f32,
    ) -> Self {
        let style = &child.style;
        let edges = FlexIntrinsicAxisEdges::for_style(style, direction, container_inline_size);
        let main_basis = if direction.is_row_axis() {
            available.width_is_definite.then_some(available.width)
        } else {
            available.height.filter(|_| available.height_is_definite)
        };
        let cross_basis = if direction.is_row_axis() {
            available.height.filter(|_| available.height_is_definite)
        } else {
            available.width_is_definite.then_some(available.width)
        };
        let definite_main = definite_flex_item_main_content_size(style, direction, main_basis);
        let definite_cross = definite_flex_item_cross_content_size(style, direction, cross_basis);
        let min_main_content = if direction.is_row_axis() {
            size.min_width
        } else {
            size.min_height
        };
        let max_main_content = if direction.is_row_axis() {
            size.content_width
        } else {
            size.content_height
        };
        let min_cross_content = if direction.is_row_axis() {
            size.min_height
        } else {
            size.min_width
        };
        let max_cross_content = if direction.is_row_axis() {
            size.content_height
        } else {
            size.content_width
        };
        let flex_base_content =
            estimated_flex_main_content_size(style, size, direction, main_basis);
        let flex_base_size = (flex_base_content + edges.main).max(0.0);
        let min_main_constraint =
            definite_flex_item_min_main_content_size(style, direction, main_basis)
                .map(|size| size + edges.main);
        let max_main_constraint =
            definite_flex_item_max_main_content_size(style, direction, main_basis)
                .map(|size| size + edges.main);
        let min_main_contribution = flex_intrinsic_main_size_contribution(
            min_main_content + edges.main,
            definite_main.map(|size| size + edges.main),
            flex_base_size,
            style.flex_grow,
            style.flex_shrink,
            min_main_constraint,
            max_main_constraint,
        );
        let max_main_contribution = flex_intrinsic_main_size_contribution(
            max_main_content + edges.main,
            definite_main.map(|size| size + edges.main),
            flex_base_size,
            style.flex_grow,
            style.flex_shrink,
            min_main_constraint,
            max_main_constraint,
        );
        let hypothetical_main_size = flex_base_size
            .max(min_main_contribution)
            .min(max_main_contribution.max(min_main_contribution));

        let (min_cross_contribution, max_cross_contribution) =
            if let Some(definite_cross) = definite_cross {
                let contribution = (definite_cross + edges.cross).max(0.0);
                (contribution, contribution)
            } else {
                (
                    (min_cross_content + edges.cross).max(0.0),
                    (max_cross_content + edges.cross).max(0.0),
                )
            };

        Self {
            min_main_contribution,
            max_main_contribution,
            min_cross_contribution,
            max_cross_contribution,
            flex_base_size,
            hypothetical_main_size,
            grow: style.flex_grow.max(0.0),
            shrink: style.flex_shrink.max(0.0),
        }
    }

    fn resolved_with_flex_fraction(self, flex_fraction: f32) -> f32 {
        let unclamped = if flex_fraction > 0.0 {
            self.flex_base_size + self.grow * flex_fraction
        } else if flex_fraction < 0.0 {
            self.flex_base_size + self.shrink * self.flex_base_size * flex_fraction
        } else {
            self.flex_base_size
        };
        unclamped
            .max(self.min_main_contribution)
            .min(self.max_main_contribution.max(self.min_main_contribution))
            .max(0.0)
    }
}

/// Computes a flex item's intrinsic main-size contribution.
///
/// CSS Flexbox clamps each item contribution by the outer flex base size when
/// the item cannot grow or cannot shrink, and then by definite min/max main
/// sizes:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>.
fn flex_intrinsic_main_size_contribution(
    content_contribution: f32,
    preferred_main_size: Option<f32>,
    flex_base_size: f32,
    grow: f32,
    shrink: f32,
    min_main_size: Option<f32>,
    max_main_size: Option<f32>,
) -> f32 {
    let mut contribution = preferred_main_size
        .map(|preferred| content_contribution.max(preferred))
        .unwrap_or(content_contribution)
        .max(0.0);
    if grow <= 0.0 {
        contribution = contribution.min(flex_base_size.max(0.0));
    }
    if shrink <= 0.0 {
        contribution = contribution.max(flex_base_size.max(0.0));
    }
    constrain(
        contribution,
        min_main_size.map(|size| size.max(0.0)),
        max_main_size.map(|size| size.max(0.0)),
    )
    .max(0.0)
}

#[derive(Debug, Clone, Copy)]
struct FlexIntrinsicAxisEdges {
    main: f32,
    cross: f32,
}

impl FlexIntrinsicAxisEdges {
    fn for_style(
        style: &ComputedStyle,
        direction: FlexDirection,
        container_inline_size: f32,
    ) -> Self {
        let padding = used_padding_edges(style, container_inline_size);
        let margin = used_margin_edges(style, container_inline_size);
        let border = used_border_widths(style);
        let horizontal =
            padding.left + padding.right + border.left + border.right + margin.left + margin.right;
        let vertical =
            padding.top + padding.bottom + border.top + border.bottom + margin.top + margin.bottom;
        if direction.is_row_axis() {
            Self {
                main: horizontal,
                cross: vertical,
            }
        } else {
            Self {
                main: vertical,
                cross: horizontal,
            }
        }
    }
}

fn definite_flex_item_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: Option<f32>,
) -> Option<f32> {
    if direction.is_row_axis() {
        let horizontal_non_content = main_basis
            .map(|basis| intrinsic_horizontal_non_content(style, basis))
            .unwrap_or_else(|| {
                style.padding.left + style.padding.right + horizontal_border_width(style)
            });
        used_content_width_or_auto_with_optional_basis(style, main_basis, horizontal_non_content)
    } else {
        let vertical_non_content =
            style.padding.top + style.padding.bottom + vertical_border_width(style);
        used_content_height_or_auto_with_optional_basis(style, main_basis, vertical_non_content)
    }
}

fn definite_flex_item_cross_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    cross_basis: Option<f32>,
) -> Option<f32> {
    if direction.is_row_axis() {
        let vertical_non_content =
            style.padding.top + style.padding.bottom + vertical_border_width(style);
        used_content_height_or_auto_with_optional_basis(style, cross_basis, vertical_non_content)
    } else {
        let horizontal_non_content = cross_basis
            .map(|basis| intrinsic_horizontal_non_content(style, basis))
            .unwrap_or_else(|| {
                style.padding.left + style.padding.right + horizontal_border_width(style)
            });
        used_content_width_or_auto_with_optional_basis(style, cross_basis, horizontal_non_content)
    }
}

fn definite_flex_item_min_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: Option<f32>,
) -> Option<f32> {
    definite_flex_item_main_axis_content_size(
        if direction.is_row_axis() {
            style.box_values.min_width
        } else {
            style.box_values.min_height
        },
        main_basis,
    )
}

fn definite_flex_item_max_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: Option<f32>,
) -> Option<f32> {
    definite_flex_item_main_axis_content_size(
        if direction.is_row_axis() {
            style.box_values.max_width
        } else {
            style.box_values.max_height
        },
        main_basis,
    )
}

fn definite_flex_item_main_axis_content_size(
    value: css::ComputedLengthPercentageOrAuto,
    main_basis: Option<f32>,
) -> Option<f32> {
    used_length_percentage_or_auto_with_optional_basis(value, main_basis).map(|size| size.max(0.0))
}

fn intrinsic_flex_container_min_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    gap: f32,
    available: FlexAvailableSpace,
) -> f32 {
    if items.is_empty() {
        return 0.0;
    }
    if style.flex_wrap == FlexWrap::NoWrap {
        return items
            .iter()
            .map(|item| item.min_main_contribution)
            .sum::<f32>()
            + intrinsic_gap_total(gap, items.len());
    }

    let line_limit = definite_flex_container_main_size(style, direction, available);
    if let Some(line_limit) = line_limit {
        return intrinsic_flex_lines(items, line_limit, gap)
            .iter()
            .map(|line| line.min_main)
            .fold(0.0f32, f32::max);
    }

    items
        .iter()
        .map(|item| item.hypothetical_main_size.max(item.min_main_contribution))
        .fold(0.0f32, f32::max)
}

fn intrinsic_flex_container_max_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    gap: f32,
    available: FlexAvailableSpace,
) -> f32 {
    if items.is_empty() {
        return 0.0;
    }
    if style.flex_wrap != FlexWrap::NoWrap
        && let Some(line_limit) = definite_flex_container_main_size(style, direction, available)
    {
        return intrinsic_flex_lines(items, line_limit, gap)
            .iter()
            .map(|line| line.max_main)
            .fold(0.0f32, f32::max);
    }

    let flex_fraction = intrinsic_max_content_flex_fraction(items);
    items
        .iter()
        .map(|item| item.resolved_with_flex_fraction(flex_fraction))
        .sum::<f32>()
        + intrinsic_gap_total(gap, items.len())
}

/// Return the ideal-algorithm max-content flex fraction from Flexbox 9.9.1.1.
///
/// The current Flexbox draft leaves the web-compatible algorithm in 9.9.1.2
/// partially unresolved. Quire therefore implements the concrete ideal
/// flex-fraction algorithm and records any remaining browser-compatibility
/// mismatch as a spec divergence rather than encoding undefined behavior.
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-main-sizes>.
fn intrinsic_max_content_flex_fraction(items: &[FlexIntrinsicItem]) -> f32 {
    items
        .iter()
        .map(|item| {
            if item.flex_base_size < item.max_main_contribution {
                if item.grow > 0.0 {
                    (item.max_main_contribution - item.flex_base_size) / item.grow
                } else {
                    0.0
                }
            } else if item.flex_base_size > item.max_main_contribution {
                let scaled_shrink = item.shrink * item.flex_base_size;
                if scaled_shrink > 0.0 {
                    (item.max_main_contribution - item.flex_base_size) / scaled_shrink
                } else {
                    0.0
                }
            } else {
                0.0
            }
        })
        .fold(0.0f32, |largest, fraction| largest.max(fraction))
}

fn intrinsic_flex_container_cross_sizes(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    gap: f32,
    available: FlexAvailableSpace,
    min_main: f32,
    max_main: f32,
) -> (f32, f32) {
    if items.is_empty() {
        return (0.0, 0.0);
    }
    if style.flex_wrap == FlexWrap::NoWrap {
        let min_cross = items
            .iter()
            .map(|item| item.min_cross_contribution)
            .fold(0.0f32, f32::max);
        let max_cross = items
            .iter()
            .map(|item| item.max_cross_contribution)
            .fold(0.0f32, f32::max);
        return (min_cross, max_cross.max(min_cross));
    }

    if let Some(line_limit) =
        intrinsic_flex_container_line_limit(style, direction, available, min_main, max_main)
    {
        let lines = intrinsic_flex_lines(items, line_limit, gap);
        let min_cross = lines.iter().map(|line| line.min_cross).sum::<f32>()
            + intrinsic_gap_total(gap, lines.len());
        let max_cross = lines.iter().map(|line| line.max_cross).sum::<f32>()
            + intrinsic_gap_total(gap, lines.len());
        return (min_cross, max_cross.max(min_cross));
    }

    let min_cross = items
        .iter()
        .map(|item| item.min_cross_contribution)
        .fold(0.0f32, f32::max);
    if direction.is_column_axis() {
        let max_cross = items
            .iter()
            .map(|item| item.max_cross_contribution)
            .sum::<f32>()
            + intrinsic_gap_total(gap, items.len());
        (min_cross, max_cross.max(min_cross))
    } else {
        let max_cross = items
            .iter()
            .map(|item| item.max_cross_contribution)
            .fold(0.0f32, f32::max);
        (min_cross, max_cross.max(min_cross))
    }
}

#[derive(Debug, Clone, Copy)]
struct IntrinsicFlexLine {
    min_main: f32,
    max_main: f32,
    min_cross: f32,
    max_cross: f32,
}

fn intrinsic_flex_lines(
    items: &[FlexIntrinsicItem],
    line_limit: f32,
    gap: f32,
) -> Vec<IntrinsicFlexLine> {
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut line_main = 0.0f32;

    for (index, item) in items.iter().enumerate() {
        let item_main = item.hypothetical_main_size.max(0.0);
        let candidate = if index == line_start {
            item_main
        } else {
            line_main + gap.max(0.0) + item_main
        };
        if index > line_start && candidate > line_limit.max(0.0) + 0.01 {
            lines.push(intrinsic_flex_line(&items[line_start..index], gap));
            line_start = index;
            line_main = item_main;
        } else {
            line_main = candidate;
        }
    }

    lines.push(intrinsic_flex_line(&items[line_start..], gap));
    lines
}

fn intrinsic_flex_line(items: &[FlexIntrinsicItem], gap: f32) -> IntrinsicFlexLine {
    IntrinsicFlexLine {
        min_main: items
            .iter()
            .map(|item| item.min_main_contribution)
            .sum::<f32>()
            + intrinsic_gap_total(gap, items.len()),
        max_main: intrinsic_flex_container_max_main_size_no_wrap(items, gap),
        min_cross: items
            .iter()
            .map(|item| item.min_cross_contribution)
            .fold(0.0f32, f32::max),
        max_cross: items
            .iter()
            .map(|item| item.max_cross_contribution)
            .fold(0.0f32, f32::max),
    }
}

fn intrinsic_flex_container_max_main_size_no_wrap(items: &[FlexIntrinsicItem], gap: f32) -> f32 {
    let flex_fraction = intrinsic_max_content_flex_fraction(items);
    items
        .iter()
        .map(|item| item.resolved_with_flex_fraction(flex_fraction))
        .sum::<f32>()
        + intrinsic_gap_total(gap, items.len())
}

fn definite_flex_container_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    if direction.is_row_axis() {
        definite_flex_container_axis_size(
            style.box_values.width,
            available.width_is_definite.then_some(available.width),
        )
    } else {
        definite_flex_container_axis_size(
            style.box_values.height,
            available.height.filter(|_| available.height_is_definite),
        )
    }
}

fn definite_flex_container_axis_size(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent == 0.0 {
                Some(value.length.max(0.0))
            } else {
                Some(used_length_percentage(value, percentage_basis?).max(0.0))
            }
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => percentage_basis,
    }
}

fn intrinsic_flex_container_line_limit(
    style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
    min_main: f32,
    max_main: f32,
) -> Option<f32> {
    let (value, percentage_basis) = if direction.is_row_axis() {
        (
            style.box_values.width,
            available.width_is_definite.then_some(available.width),
        )
    } else {
        (
            style.box_values.height,
            available.height.filter(|_| available.height_is_definite),
        )
    };
    match value {
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_main.max(0.0)),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(max_main.max(min_main).max(0.0)),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .and_then(|limit| {
                    if limit.percent == 0.0 {
                        Some(limit.length.max(0.0))
                    } else {
                        percentage_basis.map(|basis| used_length_percentage(limit, basis).max(0.0))
                    }
                })
                .or(percentage_basis)
                .unwrap_or(max_main);
            Some(max_main.max(min_main).min(min_main.max(stretch)).max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => {
            definite_flex_container_axis_size(value, percentage_basis)
        }
    }
}

fn intrinsic_gap_total(gap: f32, item_count: usize) -> f32 {
    gap.max(0.0) * item_count.saturating_sub(1) as f32
}

#[derive(Debug, Clone, Copy)]
struct EstimatedFlexBaselineItem {
    outer_main_size: f32,
    outer_cross_size: f32,
    margin_cross_start: f32,
    cross_alignment: EstimatedFlexItemCrossAlignment,
    first_baseline: Option<f32>,
    last_baseline: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
enum EstimatedFlexItemCrossAlignment {
    Side(PhysicalSide),
    Center,
}

#[derive(Debug, Clone, Copy)]
struct EstimatedFlexLineMetrics {
    line_count: usize,
    cross_size: f32,
    first_baseline: Option<f32>,
    last_baseline: Option<f32>,
}

#[derive(Debug, Clone)]
struct EstimatedFlexLine {
    item_indices: Vec<usize>,
    cross_start: f32,
    cross_size: f32,
}

fn estimated_flex_item_cross_axis_baselines(
    size: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> (Option<f32>, Option<f32>) {
    if physical_direction.is_row_axis() {
        (size.first_baseline, size.last_baseline)
    } else {
        (
            size.first_horizontal_baseline,
            size.last_horizontal_baseline,
        )
    }
}

/// Estimate a row flex container's exported baselines from flex lines.
///
/// CSS Flexbox generates a row flex container's first and last main-axis
/// baseline sets from the first and last flex lines, using the startmost or
/// endmost item on those lines when that item has a parallel baseline. In
/// vertical writing modes the CSS row axis is physical y, so the exported
/// baseline is a horizontal x-axis offset:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
fn estimate_row_flex_container_line_metrics(
    style: &ComputedStyle,
    available: FlexAvailableSpace,
    items: &[EstimatedFlexBaselineItem],
) -> Option<EstimatedFlexLineMetrics> {
    if items.is_empty() {
        return None;
    }

    if !style.flex_direction.is_row_axis() {
        return None;
    }

    let physical_direction = physical_flex_direction(style);
    let (physical_gap_width, physical_gap_height) = physical_flex_gaps(style);
    let main_gap_value = if physical_direction.is_row_axis() {
        physical_gap_width
    } else {
        physical_gap_height
    };
    let cross_gap_value = if physical_direction.is_row_axis() {
        physical_gap_height
    } else {
        physical_gap_width
    };
    let available_main_size = if physical_direction.is_row_axis() {
        available.width
    } else {
        available.height.unwrap_or(available.width)
    };
    let available_cross_size = if physical_direction.is_row_axis() {
        available.height.unwrap_or(0.0)
    } else {
        available.width
    };
    let intrinsic_main_gap = estimated_intrinsic_flex_gap(main_gap_value);
    let main_size =
        estimated_row_flex_container_main_size(style, available, items, intrinsic_main_gap);
    let main_gap = used_flex_gap(main_gap_value, main_size.unwrap_or(available_main_size));
    let cross_gap = used_flex_gap(cross_gap_value, available_cross_size);
    let mut lines = if style.flex_wrap == FlexWrap::NoWrap {
        vec![estimated_flex_line(0, items.len(), 0.0, items)]
    } else if let Some(main_size) = main_size {
        estimate_wrapped_row_flex_lines(items, main_size.max(0.0), main_gap, cross_gap)
    } else {
        vec![estimated_flex_line(0, items.len(), 0.0, items)]
    };
    if style.flex_wrap != FlexWrap::NoWrap
        && matches!(
            style.align_content.keyword,
            ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch
        )
        && let Some(container_cross_size) =
            estimated_row_flex_container_cross_size(style, available, physical_direction)
    {
        stretch_estimated_flex_line_cross_positions(
            &mut lines,
            container_cross_size.max(0.0),
            cross_gap,
        );
    }
    if style.flex_wrap == FlexWrap::WrapReverse {
        reverse_estimated_flex_line_cross_positions(&mut lines);
    }
    let first_line = lines.first()?;
    let last_line = lines.last()?;
    let cross_size = lines
        .iter()
        .map(|line| line.cross_start + line.cross_size)
        .fold(0.0f32, f32::max);

    Some(EstimatedFlexLineMetrics {
        line_count: lines.len(),
        cross_size,
        first_baseline: estimated_flex_line_baseline(
            first_line,
            items,
            style.flex_direction,
            EstimatedFlexBaselineSet::First,
        ),
        last_baseline: estimated_flex_line_baseline(
            last_line,
            items,
            style.flex_direction,
            EstimatedFlexBaselineSet::Last,
        ),
    })
}

fn estimate_wrapped_row_flex_lines(
    items: &[EstimatedFlexBaselineItem],
    main_size: f32,
    main_gap: f32,
    cross_gap: f32,
) -> Vec<EstimatedFlexLine> {
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut line_main_size = 0.0f32;

    for (index, item) in items.iter().enumerate() {
        let item_outer_main = item.outer_main_size.max(0.0);
        let candidate_main_size = if index == line_start {
            item_outer_main
        } else {
            line_main_size + main_gap + item_outer_main
        };
        if index > line_start && candidate_main_size > main_size + 0.01 {
            let cross_start = estimated_next_flex_line_cross_start(&lines, cross_gap);
            lines.push(estimated_flex_line(line_start, index, cross_start, items));
            line_start = index;
            line_main_size = item_outer_main;
        } else {
            line_main_size = candidate_main_size;
        }
    }

    let cross_start = estimated_next_flex_line_cross_start(&lines, cross_gap);
    lines.push(estimated_flex_line(
        line_start,
        items.len(),
        cross_start,
        items,
    ));
    lines
}

fn estimated_row_flex_container_main_size(
    style: &ComputedStyle,
    available: FlexAvailableSpace,
    items: &[EstimatedFlexBaselineItem],
    intrinsic_main_gap: f32,
) -> Option<f32> {
    let physical_direction = physical_flex_direction(style);
    let (size_property, min_size_property, max_size_property, percentage_basis) =
        if physical_direction.is_row_axis() {
            (
                style.box_values.width,
                style.box_values.min_width,
                style.box_values.max_width,
                available.width_is_definite.then_some(available.width),
            )
        } else {
            (
                style.box_values.height,
                style.box_values.min_height,
                style.box_values.max_height,
                available.height.filter(|_| available.height_is_definite),
            )
        };
    let (min_content, max_content) =
        estimated_row_flex_container_intrinsic_main_sizes(items, intrinsic_main_gap);
    let size = estimated_intrinsic_length_percentage_or_auto(
        size_property,
        percentage_basis,
        min_content,
        max_content,
    )
    .or(percentage_basis);
    let max_size = estimated_intrinsic_length_percentage_or_auto(
        max_size_property,
        percentage_basis,
        min_content,
        max_content,
    );
    let min_size = estimated_intrinsic_length_percentage_or_auto(
        min_size_property,
        percentage_basis,
        min_content,
        max_content,
    );

    match (size, max_size) {
        (Some(size), Some(max_size)) => Some(
            min_size
                .map_or(size, |min_size| size.max(min_size))
                .min(max_size),
        ),
        (Some(size), None) => Some(min_size.map_or(size, |min_size| size.max(min_size))),
        (None, Some(max_size)) => {
            Some(min_size.map_or(max_size, |min_size| max_size.max(min_size)))
        }
        (None, None) => None,
    }
}

fn estimated_row_flex_container_cross_size(
    style: &ComputedStyle,
    available: FlexAvailableSpace,
    physical_direction: FlexDirection,
) -> Option<f32> {
    if physical_direction.is_row_axis() {
        estimated_intrinsic_length_percentage_or_auto(
            style.box_values.height,
            available.height.filter(|_| available.height_is_definite),
            0.0,
            0.0,
        )
        .or_else(|| available.height.filter(|_| available.height_is_definite))
    } else {
        estimated_intrinsic_length_percentage_or_auto(
            style.box_values.width,
            available.width_is_definite.then_some(available.width),
            0.0,
            0.0,
        )
        .or_else(|| available.width_is_definite.then_some(available.width))
    }
}

fn estimated_row_flex_container_intrinsic_main_sizes(
    items: &[EstimatedFlexBaselineItem],
    intrinsic_main_gap: f32,
) -> (f32, f32) {
    let min_content = items
        .iter()
        .map(|item| item.outer_main_size.max(0.0))
        .fold(0.0f32, f32::max);
    let max_content_items = items
        .iter()
        .map(|item| item.outer_main_size.max(0.0))
        .sum::<f32>();
    let max_content_gaps = intrinsic_main_gap.max(0.0) * items.len().saturating_sub(1) as f32;
    let max_content = max_content_items + max_content_gaps;
    (min_content, max_content.max(min_content))
}

/// Returns the flex gap contribution used by intrinsic max-content estimates.
///
/// CSS Box Alignment resolves cyclic percentage gaps against zero for
/// intrinsic size contributions, while preserving any non-percentage length
/// component:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
fn estimated_intrinsic_flex_gap(value: css::ComputedGap) -> f32 {
    match value {
        css::ComputedGap::Normal => 0.0,
        css::ComputedGap::LengthPercentage(value) => value.length.max(0.0),
    }
}

fn estimated_intrinsic_length_percentage_or_auto(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
    min_content: f32,
    max_content: f32,
) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent == 0.0 {
                Some(value.length.max(0.0))
            } else {
                percentage_basis.map(|basis| used_length_percentage(value, basis).max(0.0))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_content.max(0.0)),
        css::ComputedLengthPercentageOrAuto::MaxContent => {
            Some(max_content.max(min_content).max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .and_then(|limit| {
                    percentage_basis.map(|basis| used_length_percentage(limit, basis))
                })
                .or_else(|| {
                    limit
                        .filter(|limit| limit.percent == 0.0)
                        .map(|limit| limit.length)
                })
                .or(percentage_basis)
                .unwrap_or(max_content);
            Some(
                max_content
                    .max(min_content)
                    .max(0.0)
                    .min(min_content.max(0.0).max(stretch.max(0.0))),
            )
        }
    }
}

fn reverse_estimated_flex_line_cross_positions(lines: &mut [EstimatedFlexLine]) {
    let cross_size = lines
        .iter()
        .map(|line| line.cross_start + line.cross_size)
        .fold(0.0f32, f32::max);
    for line in lines {
        line.cross_start = (cross_size - line.cross_start - line.cross_size).max(0.0);
    }
}

fn stretch_estimated_flex_line_cross_positions(
    lines: &mut [EstimatedFlexLine],
    container_cross_size: f32,
    cross_gap: f32,
) {
    if lines.is_empty() {
        return;
    }
    let total_line_cross_size = lines.iter().map(|line| line.cross_size).sum::<f32>();
    let total_gap = cross_gap.max(0.0) * lines.len().saturating_sub(1) as f32;
    let extra_per_line =
        ((container_cross_size - total_line_cross_size - total_gap) / lines.len() as f32).max(0.0);
    let mut cross_start = 0.0;
    for line in lines {
        line.cross_start = cross_start;
        line.cross_size += extra_per_line;
        cross_start += line.cross_size + cross_gap.max(0.0);
    }
}

fn estimated_next_flex_line_cross_start(lines: &[EstimatedFlexLine], cross_gap: f32) -> f32 {
    lines
        .last()
        .map(|line| line.cross_start + line.cross_size + cross_gap)
        .unwrap_or(0.0)
}

fn estimated_flex_line(
    start: usize,
    end: usize,
    cross_start: f32,
    items: &[EstimatedFlexBaselineItem],
) -> EstimatedFlexLine {
    let item_indices = (start..end).collect::<Vec<_>>();
    let cross_size = item_indices
        .iter()
        .copied()
        .map(|index| items[index].outer_cross_size)
        .fold(0.0f32, f32::max);
    EstimatedFlexLine {
        item_indices,
        cross_start,
        cross_size,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EstimatedFlexBaselineSet {
    First,
    Last,
}

fn estimated_flex_line_baseline(
    line: &EstimatedFlexLine,
    items: &[EstimatedFlexBaselineItem],
    direction: FlexDirection,
    baseline_set: EstimatedFlexBaselineSet,
) -> Option<f32> {
    estimated_flex_line_baseline_item_index(line, direction, baseline_set).and_then(|index| {
        let item = items[index];
        let baseline = match baseline_set {
            EstimatedFlexBaselineSet::First => item.first_baseline,
            EstimatedFlexBaselineSet::Last => item.last_baseline,
        }?;
        Some(
            line.cross_start
                + estimated_flex_item_cross_start_offset(line, item)
                + item.margin_cross_start
                + baseline,
        )
    })
}

fn estimated_flex_item_cross_start_offset(
    line: &EstimatedFlexLine,
    item: EstimatedFlexBaselineItem,
) -> f32 {
    let free_space = (line.cross_size - item.outer_cross_size).max(0.0);
    match item.cross_alignment {
        EstimatedFlexItemCrossAlignment::Side(side) if side.is_end_edge() => free_space,
        EstimatedFlexItemCrossAlignment::Side(_) => 0.0,
        EstimatedFlexItemCrossAlignment::Center => free_space / 2.0,
    }
}

fn estimated_flex_item_cross_alignment(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> EstimatedFlexItemCrossAlignment {
    match estimated_effective_align_self(child_style, container_style).keyword {
        SelfAlignmentKeyword::Center => EstimatedFlexItemCrossAlignment::Center,
        SelfAlignmentKeyword::End => EstimatedFlexItemCrossAlignment::Side(
            estimated_flex_base_cross_end_side(container_style),
        ),
        SelfAlignmentKeyword::FlexEnd => {
            EstimatedFlexItemCrossAlignment::Side(estimated_flex_cross_end_side(container_style))
        }
        SelfAlignmentKeyword::SelfStart => EstimatedFlexItemCrossAlignment::Side(
            estimated_child_self_start_side(child_style, container_style),
        ),
        SelfAlignmentKeyword::SelfEnd => EstimatedFlexItemCrossAlignment::Side(
            estimated_child_self_end_side(child_style, container_style),
        ),
        SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Start
        | SelfAlignmentKeyword::FlexStart
        | SelfAlignmentKeyword::Left
        | SelfAlignmentKeyword::Right
        | SelfAlignmentKeyword::Stretch
        | SelfAlignmentKeyword::Baseline
        | SelfAlignmentKeyword::LastBaseline => {
            EstimatedFlexItemCrossAlignment::Side(estimated_flex_cross_start_side(container_style))
        }
    }
}

fn estimated_effective_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> AlignSelf {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.align_items
    } else {
        child_style.align_self
    }
}

fn estimated_flex_item_available_space(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> FlexItemAvailableSpace {
    let mut item_available = FlexItemAvailableSpace::from_container(available);
    let Some(stretched_cross_size) = estimated_stretched_flex_item_cross_size(
        child_style,
        container_style,
        physical_direction,
        available,
    ) else {
        return item_available;
    };

    if physical_direction.is_row_axis() {
        item_available.height = Some(stretched_cross_size);
        item_available.height_is_definite = true;
    } else {
        item_available.width = stretched_cross_size;
        item_available.width_is_definite = true;
    }
    item_available
}

fn estimated_stretched_flex_item_cross_size(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    if !matches!(
        estimated_effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    ) || estimated_flex_item_has_auto_cross_margin(child_style, physical_direction)
    {
        return None;
    }

    if physical_direction.is_row_axis() {
        if !child_style.box_values.height.is_auto() {
            return None;
        }
        let container_cross_size = available.height.filter(|_| available.height_is_definite)?;
        Some((container_cross_size - child_style.margin.top - child_style.margin.bottom).max(0.0))
    } else {
        if !child_style.box_values.width.is_auto() {
            return None;
        }
        let container_cross_size = available.width_is_definite.then_some(available.width)?;
        Some((container_cross_size - child_style.margin.left - child_style.margin.right).max(0.0))
    }
}

fn estimated_flex_item_has_auto_cross_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    if physical_direction.is_row_axis() {
        style.box_values.margin.top.is_auto() || style.box_values.margin.bottom.is_auto()
    } else {
        style.box_values.margin.left.is_auto() || style.box_values.margin.right.is_auto()
    }
}

fn estimated_flex_base_cross_start_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_start_side(style.writing_mode)
    } else {
        inline_start_side(style.writing_mode, style.direction)
    }
}

fn estimated_flex_base_cross_end_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_end_side(style.writing_mode)
    } else {
        inline_end_side(style.writing_mode, style.direction)
    }
}

fn estimated_flex_cross_start_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_wrap == FlexWrap::WrapReverse {
        estimated_flex_base_cross_end_side(style)
    } else {
        estimated_flex_base_cross_start_side(style)
    }
}

fn estimated_flex_cross_end_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_wrap == FlexWrap::WrapReverse {
        estimated_flex_base_cross_start_side(style)
    } else {
        estimated_flex_base_cross_end_side(style)
    }
}

fn estimated_child_self_start_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_axis = estimated_flex_base_cross_start_side(container_style).axis();
    let block_start = block_start_side(child_style.writing_mode);
    if block_start.axis() == cross_axis {
        block_start
    } else {
        inline_start_side(child_style.writing_mode, child_style.direction)
    }
}

fn estimated_child_self_end_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_axis = estimated_flex_base_cross_start_side(container_style).axis();
    let block_end = block_end_side(child_style.writing_mode);
    if block_end.axis() == cross_axis {
        block_end
    } else {
        inline_end_side(child_style.writing_mode, child_style.direction)
    }
}

fn estimated_flex_line_baseline_item_index(
    line: &EstimatedFlexLine,
    direction: FlexDirection,
    baseline_set: EstimatedFlexBaselineSet,
) -> Option<usize> {
    match (baseline_set, direction) {
        (EstimatedFlexBaselineSet::First, FlexDirection::Row | FlexDirection::RowReverse) => {
            line.item_indices.first().copied()
        }
        (EstimatedFlexBaselineSet::First, FlexDirection::Column) => {
            line.item_indices.first().copied()
        }
        (EstimatedFlexBaselineSet::First, FlexDirection::ColumnReverse) => {
            line.item_indices.last().copied()
        }
        (EstimatedFlexBaselineSet::Last, FlexDirection::Row | FlexDirection::RowReverse) => {
            line.item_indices.last().copied()
        }
        (EstimatedFlexBaselineSet::Last, FlexDirection::Column) => {
            line.item_indices.last().copied()
        }
        (EstimatedFlexBaselineSet::Last, FlexDirection::ColumnReverse) => {
            line.item_indices.first().copied()
        }
    }
}

fn estimated_flex_main_content_size(
    style: &ComputedStyle,
    size: FlexItemEstimate,
    direction: FlexDirection,
    percentage_basis: Option<f32>,
) -> f32 {
    let (preferred_size, min_size, specified_size) = if direction.is_row_axis() {
        (size.content_width, size.min_width, style.box_values.width)
    } else {
        (
            size.content_height,
            size.min_height,
            style.box_values.height,
        )
    };

    match style.flex_basis {
        css::ComputedFlexBasis::LengthPercentage(value) => {
            if value.percent != 0.0 && percentage_basis.is_none() {
                preferred_size
            } else {
                used_length_percentage(value, percentage_basis.unwrap_or(0.0))
            }
        }
        css::ComputedFlexBasis::Content | css::ComputedFlexBasis::MaxContent => preferred_size,
        css::ComputedFlexBasis::MinContent => min_size,
        css::ComputedFlexBasis::FitContent(limit) => {
            let limit = limit
                .map(|limit| used_length_percentage(limit, percentage_basis.unwrap_or(0.0)))
                .or(percentage_basis)
                .unwrap_or(preferred_size);
            preferred_size
                .max(0.0)
                .min(min_size.max(0.0).max(limit.max(0.0)))
        }
        css::ComputedFlexBasis::Auto => {
            used_length_percentage_or_auto_with_optional_basis(specified_size, percentage_basis)
                .unwrap_or(preferred_size)
        }
    }
}
