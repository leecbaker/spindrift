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
        containing_width: f32,
        containing_width_is_definite: bool,
    ) -> FlexItemEstimate {
        let style = &child.style;
        let containing_width_basis = containing_width_is_definite.then_some(containing_width);
        if let Some(text) = child.anonymous_text() {
            let measurement =
                self.anonymous_flex_inline_measurement(text, style, containing_width.max(1.0));
            let contribution = measurement.contribution;
            let preferred_width =
                intrinsic::guarded_max_content_width(contribution.max_content, style)
                    .max(style.font_size * 0.25);
            let preferred_min_width = contribution.min_content.max(style.font_size * 0.25);
            let used_line_height = self.font_system.used_line_height(style);
            let intrinsic_content_height = measurement.height.max(used_line_height);
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
                min_height: constrain_height(style, intrinsic_content_height, containing_width),
                content_width: preferred_width,
                content_height: intrinsic_content_height,
                first_baseline: Some(first_text_baseline_offset(&mut self.font_system, style)),
                last_baseline: Some(last_text_baseline_offset(
                    &mut self.font_system,
                    style,
                    measurement.line_count.max(1),
                )),
            };
        }

        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return FlexItemEstimate::fixed(0.0, 0.0);
        };
        if style.display.is_flex() {
            let children = self.with_ancestor_signature(signature.clone(), |layout| {
                child_boxes
                    .map(flex_children_from_boxes)
                    .unwrap_or_else(|| {
                        flex_children(element, style, stylesheets, &layout.ancestors)
                    })
            });
            if !children.is_empty() {
                let intrinsic_size = self.with_ancestor_signature(signature.clone(), |layout| {
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
                });
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
                    first_baseline: None,
                    last_baseline: None,
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
            };
        }

        let definition_list_column_height =
            self.estimate_definition_list_column_height(child, stylesheets, containing_width);
        let inline_content_width =
            (containing_width - style.padding.left - style.padding.right).max(1.0);
        let inline_measurement =
            self.estimate_child_inline_measurement(child, stylesheets, inline_content_width);
        let child_intrinsic = self.estimate_child_intrinsic_widths(
            child,
            stylesheets,
            containing_width,
            inline_measurement.contribution,
        );
        let child_preferred_block_height = self.estimate_child_min_content_block_size(
            child,
            stylesheets,
            containing_width,
            inline_measurement.height,
        );

        let preferred_width = if inline_measurement.line_count > 0 {
            intrinsic::guarded_max_content_width(child_intrinsic.max_content, style)
        } else {
            child_intrinsic.max_content
        };
        let preferred_min_width = child_intrinsic.min_content;
        let content_width = used_length_percentage_or_auto_with_optional_basis(
            style.box_values.width,
            containing_width_basis,
        )
        .unwrap_or(preferred_width);
        let used_line_height = style.line_height;
        let fallback_content_height = if inline_measurement.line_count > 0 {
            inline_measurement
                .height
                .max(inline_measurement.line_count as f32 * used_line_height)
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
        let intrinsic_content_height = definition_list_column_height
            .unwrap_or_else(|| fallback_content_height.max(child_preferred_block_height));
        let content_height =
            used_length_percentage_or_auto(style.box_values.height, used_line_height.max(1.0))
                .unwrap_or(intrinsic_content_height);
        let min_content_height = intrinsic_content_height.max(0.0);

        FlexItemEstimate {
            width: constrain_width(style, content_width, containing_width),
            height: constrain_height(style, content_height, containing_width),
            min_width: constrain_width(style, preferred_min_width, containing_width),
            min_height: constrain_height(style, min_content_height, containing_width),
            content_width: preferred_width,
            content_height: intrinsic_content_height,
            first_baseline: (inline_measurement.line_count > 0)
                .then(|| first_text_baseline_offset(&mut self.font_system, style)),
            last_baseline: (inline_measurement.line_count > 0).then(|| {
                last_text_baseline_offset(
                    &mut self.font_system,
                    style,
                    inline_measurement.line_count,
                )
            }),
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
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        self.intrinsic_inline_measurement_for_text(text, style, available_width)
    }

    fn estimate_child_inline_measurement(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return child
                .anonymous_text()
                .map(|text| {
                    self.anonymous_flex_inline_measurement(text, &child.style, available_width)
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
            merge_outer_intrinsic_widths(contribution, child_contribution, child_style);
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
            merge_outer_intrinsic_widths(contribution, child_contribution, &child_style);
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
                        inline_measurement.height,
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

    pub(super) fn estimate_intrinsic_flex_container_size(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
    ) -> FlexItemEstimate {
        let mut width = 0.0f32;
        let mut height = 0.0f32;
        let mut min_width = 0.0f32;
        let mut min_height = 0.0f32;
        let mut child_count = 0usize;

        for child in children {
            let size = self.estimate_flex_item_size(
                child,
                stylesheets,
                available.width,
                available.width_is_definite,
            );
            let outer_width = size.width
                + child.style.padding.left
                + child.style.padding.right
                + horizontal_border_width(&child.style)
                + child.style.margin.left
                + child.style.margin.right;
            let outer_min_width =
                flex_item_min_content_contribution(style.flex_direction, child, size, outer_width);
            let outer_height = size.height
                + child.style.padding.top
                + child.style.padding.bottom
                + vertical_border_width(&child.style)
                + child.style.margin.top
                + child.style.margin.bottom;
            let outer_min_height = size.min_height
                + child.style.padding.top
                + child.style.padding.bottom
                + vertical_border_width(&child.style)
                + child.style.margin.top
                + child.style.margin.bottom;
            if style.flex_direction.is_column_axis() {
                width = width.max(outer_width);
                min_width = min_width.max(outer_min_width);
                height += outer_height;
                min_height += outer_min_height;
            } else {
                width += outer_width;
                min_width += outer_min_width;
                height = height.max(outer_height);
                min_height = min_height.max(outer_min_height);
            }
            child_count += 1;
        }

        if child_count > 1 {
            let physical_direction = physical_flex_direction(style);
            let (physical_gap_width, physical_gap_height) = physical_flex_gaps(style);
            let main_gap = if physical_direction.is_column_axis() {
                used_flex_gap(physical_gap_height, 0.0)
            } else {
                used_flex_gap(physical_gap_width, available.width)
            };
            let gaps = main_gap * (child_count - 1) as f32;
            if physical_direction.is_column_axis() {
                height += gaps;
                min_height += gaps;
            } else {
                width += gaps;
                min_width += gaps;
            }
        }

        let content_width = width;
        let content_height = height;
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
            first_baseline: None,
            last_baseline: None,
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

fn merge_outer_intrinsic_widths(
    contribution: &mut inline_layout::InlineIntrinsicContribution,
    child_contribution: (f32, f32),
    child_style: &ComputedStyle,
) {
    let outer_edges = child_style.padding.left
        + child_style.padding.right
        + horizontal_border_width(child_style)
        + child_style.margin.left
        + child_style.margin.right;
    contribution.min_content = contribution
        .min_content
        .max(child_contribution.0 + outer_edges);
    contribution.max_content = contribution
        .max_content
        .max(child_contribution.1 + outer_edges);
}

fn explicit_child_intrinsic_width(
    child_style: &ComputedStyle,
    containing_width: f32,
) -> Option<(f32, f32)> {
    let horizontal_extras =
        horizontal_border_width(child_style) + child_style.padding.left + child_style.padding.right;
    used_content_width_or_auto(child_style, containing_width, horizontal_extras)
        .map(|width| (width, width))
}

fn flex_min_content_block_child_participates(element: &Element, style: &ComputedStyle) -> bool {
    !style.display.is_none()
        && !matches!(style.position, Position::Absolute | Position::Fixed)
        && (style.display.is_block_level()
            || is_document_canvas_element(element)
            || is_replaced_element(element))
}

/// Returns the main-axis min-content contribution for a flex item.
///
/// CSS Flexbox says an inflexible item with `flex-basis: auto` contributes its
/// max-content contribution to the flex container's min-content main size:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
fn flex_item_min_content_contribution(
    direction: FlexDirection,
    child: &StyledChild<'_>,
    size: FlexItemEstimate,
    outer_width: f32,
) -> f32 {
    let min_width = size.min_width
        + child.style.padding.left
        + child.style.padding.right
        + horizontal_border_width(&child.style)
        + child.style.margin.left
        + child.style.margin.right;

    if direction.is_row_axis()
        && child.style.flex_grow == 0.0
        && child.style.flex_shrink == 0.0
        && child.style.flex_basis.is_auto()
    {
        outer_width
    } else {
        min_width
    }
}
