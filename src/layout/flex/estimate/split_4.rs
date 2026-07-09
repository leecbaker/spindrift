use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout::flex) struct FlexItemBaselineEstimate {
    pub(in crate::layout::flex) first_baseline: Option<f32>,
    pub(in crate::layout::flex) last_baseline: Option<f32>,
    pub(in crate::layout::flex) first_horizontal_baseline: Option<f32>,
    pub(in crate::layout::flex) last_horizontal_baseline: Option<f32>,
}

impl<'a> LayoutBuilder<'a> {
    /// Estimate a block-container flex item's baselines from in-flow descendants.
    ///
    /// CSS Flexbox baseline alignment uses a flex item's first/last baseline
    /// sets when available, and CSS Box Alignment lets a block container derive
    /// those baselines from its in-flow line boxes before falling back to
    /// synthesized border-box baselines:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
    /// <https://drafts.csswg.org/css-align-3/#baseline-export>.
    pub(in crate::layout::flex) fn estimate_flex_item_descendant_baselines(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> FlexItemBaselineEstimate {
        let content_width = flex_estimated_content_width(style, available_width);
        let baselines = self.with_ancestor_signature(signature.clone(), |layout| {
            layout.estimate_flex_children_baselines(
                element,
                style,
                child_boxes,
                stylesheets,
                content_width,
            )
        });
        let borders = used_border_widths(style);
        FlexItemBaselineEstimate {
            first_baseline: baselines
                .first_baseline
                .map(|baseline| borders.top + style.padding.top + baseline),
            last_baseline: baselines
                .last_baseline
                .map(|baseline| borders.top + style.padding.top + baseline),
            first_horizontal_baseline: baselines
                .first_horizontal_baseline
                .map(|baseline| borders.left + style.padding.left + baseline),
            last_horizontal_baseline: baselines
                .last_horizontal_baseline
                .map(|baseline| borders.left + style.padding.left + baseline),
        }
    }

    fn estimate_flex_children_baselines(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> FlexItemBaselineEstimate {
        if let Some(child_boxes) = child_boxes {
            return self.estimate_flex_box_children_baselines(
                child_boxes,
                style,
                stylesheets,
                available_width,
            );
        }
        self.estimate_flex_dom_children_baselines(element, style, stylesheets, available_width)
    }

    fn estimate_flex_box_children_baselines(
        &mut self,
        child_boxes: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> FlexItemBaselineEstimate {
        if formatting_box_has_inline_content(child_boxes)
            && !has_non_inline_formatting_box(child_boxes)
        {
            return self.estimate_flex_inline_children_baselines(
                child_boxes,
                style,
                stylesheets,
                available_width,
            );
        }

        let mut block_offset = 0.0_f32;
        let mut first_baseline = None;
        let mut last_baseline = None;

        for child in child_boxes {
            if !formatting_box_is_in_normal_flow(child) {
                continue;
            }
            let child_baselines =
                self.estimate_flex_formatting_child_baselines(child, stylesheets, available_width);
            if let Some(baseline) = child_baselines.first_baseline {
                first_baseline.get_or_insert(block_offset + baseline);
            }
            if let Some(baseline) = child_baselines.last_baseline {
                last_baseline = Some(block_offset + baseline);
            }
            block_offset += self.estimate_flex_formatting_child_outer_height(
                child,
                stylesheets,
                available_width,
            );
        }

        FlexItemBaselineEstimate {
            first_baseline,
            last_baseline,
            ..Default::default()
        }
    }

    fn estimate_flex_dom_children_baselines(
        &mut self,
        element: &Element,
        parent_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> FlexItemBaselineEstimate {
        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        let mut block_offset = 0.0_f32;
        let mut first_baseline = None;
        let mut last_baseline = None;

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
            if !style_is_in_normal_flow(&child_style) {
                continue;
            }
            let child_baselines = self.estimate_flex_element_baselines(
                child_element,
                &signature,
                &child_style,
                None,
                stylesheets,
                available_width,
            );
            if let Some(baseline) = child_baselines.first_baseline {
                first_baseline.get_or_insert(block_offset + baseline);
            }
            if let Some(baseline) = child_baselines.last_baseline {
                last_baseline = Some(block_offset + baseline);
            }
            block_offset += self
                .estimate_element_height(
                    child_element,
                    &child_style,
                    stylesheets,
                    available_width,
                    None,
                )
                .unwrap_or(0.0);
        }

        FlexItemBaselineEstimate {
            first_baseline,
            last_baseline,
            ..Default::default()
        }
    }

    fn estimate_flex_formatting_child_baselines(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> FlexItemBaselineEstimate {
        match child {
            box_tree::FormattingBox::Text(box_) => {
                if box_tree::formatting_box_is_collapsible_space(child) {
                    return FlexItemBaselineEstimate::default();
                }
                let baseline = self.inline_box_text_line_layout_baseline_offset(&box_.style);
                FlexItemBaselineEstimate {
                    first_baseline: Some(baseline),
                    last_baseline: Some(baseline),
                    ..Default::default()
                }
            }
            box_tree::FormattingBox::Inline(box_) => self.estimate_flex_inline_children_baselines(
                &box_.children,
                &box_.style,
                stylesheets,
                available_width,
            ),
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .estimate_flex_box_children_baselines(
                    &box_.children,
                    &box_.style,
                    stylesheets,
                    available_width,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .estimate_flex_box_children_baselines(
                    &box_.children,
                    &box_.style,
                    stylesheets,
                    available_width,
                ),
            box_tree::FormattingBox::Block(box_) => self.estimate_flex_element_baselines(
                box_.element,
                &box_.signature,
                &box_.style,
                Some(&box_.children),
                stylesheets,
                available_width,
            ),
            box_tree::FormattingBox::Flex(box_) => self.estimate_flex_element_baselines(
                box_.element,
                &box_.signature,
                &box_.style,
                Some(&box_.children),
                stylesheets,
                available_width,
            ),
            box_tree::FormattingBox::Table(box_) => self.estimate_flex_element_baselines(
                box_.element,
                &box_.signature,
                &box_.style,
                Some(&box_.children),
                stylesheets,
                available_width,
            ),
            box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_) => {
                FlexItemBaselineEstimate::default()
            }
        }
    }

    fn estimate_flex_element_baselines(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> FlexItemBaselineEstimate {
        if !style_is_in_normal_flow(style) {
            return FlexItemBaselineEstimate::default();
        }

        if style.display.is_flex()
            && let Some(baselines) = self.estimate_nested_flex_baselines(
                element,
                signature,
                style,
                child_boxes,
                stylesheets,
                available_width,
            )
        {
            return baselines;
        }

        let child_width = flex_estimated_content_width(style, available_width);
        let borders = used_border_widths(style);
        let child_baselines = self.with_ancestor_signature(signature.clone(), |layout| {
            layout.estimate_flex_children_baselines(
                element,
                style,
                child_boxes,
                stylesheets,
                child_width,
            )
        });

        FlexItemBaselineEstimate {
            first_baseline: child_baselines
                .first_baseline
                .map(|baseline| style.margin.top + borders.top + style.padding.top + baseline),
            last_baseline: child_baselines
                .last_baseline
                .map(|baseline| style.margin.top + borders.top + style.padding.top + baseline),
            first_horizontal_baseline: child_baselines
                .first_horizontal_baseline
                .map(|baseline| style.margin.left + borders.left + style.padding.left + baseline),
            last_horizontal_baseline: child_baselines
                .last_horizontal_baseline
                .map(|baseline| style.margin.left + borders.left + style.padding.left + baseline),
        }
    }

    fn estimate_nested_flex_baselines(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> Option<FlexItemBaselineEstimate> {
        let intrinsic = self.with_ancestor_signature(signature.clone(), |layout| {
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
                        width: PhysicalContentWidth::new(content_box_pt(available_width)),
                        width_basis: flex_available_percentage_basis_from_points(
                            used_length_percentage_or_auto_with_basis(
                                style.box_values.width.clone(),
                                PercentageBasis::definite(content_box_pt(available_width)),
                            )
                            .map(|width| width.points())
                            .map(|_| available_width),
                            FlexAvailableSizeSource::IntrinsicContainerSize,
                        ),
                        height: used_length_percentage_or_auto(
                            style.box_values.height.clone(),
                            PercentageBasis::definite(layout_pt(available_width)),
                        )
                        .map(|height| {
                            PhysicalContentHeight::new(crate::units::layout_to_content_box_length(
                                height,
                            ))
                        }),
                        height_basis: flex_available_percentage_basis_from_points(
                            used_length_percentage_or_auto(
                                style.box_values.height.clone(),
                                PercentageBasis::definite(layout_pt(available_width)),
                            )
                            .map(|height| height.points()),
                            FlexAvailableSizeSource::IntrinsicContainerSize,
                        ),
                    },
                )
            })
        })?;
        Some(FlexItemBaselineEstimate {
            first_baseline: intrinsic
                .first_baseline
                .map(|baseline| style.margin.top + baseline),
            last_baseline: intrinsic
                .last_baseline
                .map(|baseline| style.margin.top + baseline),
            first_horizontal_baseline: intrinsic
                .first_horizontal_baseline
                .map(|baseline| style.margin.left + baseline),
            last_horizontal_baseline: intrinsic
                .last_horizontal_baseline
                .map(|baseline| style.margin.left + baseline),
        })
    }

    fn estimate_flex_inline_children_baselines(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> FlexItemBaselineEstimate {
        if !formatting_box_has_inline_content(children) {
            return FlexItemBaselineEstimate::default();
        }

        let measurement = self.intrinsic_inline_measurement_for_boxes(
            children,
            style,
            stylesheets,
            available_width,
        );
        if measurement.line_count() == 0 {
            return FlexItemBaselineEstimate::default();
        }

        let first_baseline = self.inline_box_text_line_layout_baseline_offset(style);
        let last_baseline =
            first_baseline + measurement.line_count().saturating_sub(1) as f32 * style.line_height;
        let fallback_line_baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
        let first_line_baseline_offset = first_sequence_line_baseline_offset(
            &measurement.sequence,
            fallback_line_baseline_offset,
        );
        let last_line_baseline_offset = last_sequence_line_baseline_offset(
            &measurement.sequence,
            fallback_line_baseline_offset,
        );
        let preceding_line_height = preceding_line_height_before_last(&measurement.sequence);

        FlexItemBaselineEstimate {
            first_baseline: Some(first_baseline),
            last_baseline: Some(last_baseline),
            first_horizontal_baseline: first_horizontal_text_baseline_offset(
                style,
                available_width,
                first_line_baseline_offset,
            ),
            last_horizontal_baseline: last_horizontal_text_baseline_offset(
                style,
                available_width,
                preceding_line_height,
                last_line_baseline_offset,
            ),
        }
    }

    fn estimate_flex_formatting_child_outer_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        match child {
            box_tree::FormattingBox::Block(box_) => self
                .estimate_element_height(
                    box_.element,
                    &box_.style,
                    stylesheets,
                    available_width,
                    Some(&box_.children),
                )
                .unwrap_or(0.0),
            box_tree::FormattingBox::Flex(box_) => self
                .estimate_element_height(
                    box_.element,
                    &box_.style,
                    stylesheets,
                    available_width,
                    Some(&box_.children),
                )
                .unwrap_or(0.0),
            box_tree::FormattingBox::Table(box_) => self
                .estimate_element_height(
                    box_.element,
                    &box_.style,
                    stylesheets,
                    available_width,
                    Some(&box_.children),
                )
                .unwrap_or(0.0),
            box_tree::FormattingBox::Replaced(box_) => self
                .estimate_element_height(
                    box_.element,
                    &box_.style,
                    stylesheets,
                    available_width,
                    Some(&box_.children),
                )
                .unwrap_or(0.0),
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .estimate_flex_anonymous_outer_height(
                    &box_.children,
                    &box_.style,
                    stylesheets,
                    available_width,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .estimate_flex_anonymous_outer_height(
                    &box_.children,
                    &box_.style,
                    stylesheets,
                    available_width,
                ),
            box_tree::FormattingBox::Inline(_)
            | box_tree::FormattingBox::AtomicInline(_)
            | box_tree::FormattingBox::Text(_) => 0.0,
        }
    }

    fn estimate_flex_anonymous_outer_height(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        let inline_height = if formatting_box_has_inline_content(children)
            && !has_non_inline_formatting_box(children)
        {
            self.intrinsic_inline_measurement_for_boxes(
                children,
                style,
                stylesheets,
                available_width,
            )
            .height()
        } else {
            0.0
        };
        let block_height = children
            .iter()
            .filter(|child| formatting_box_is_in_normal_flow(child))
            .map(|child| {
                self.estimate_flex_formatting_child_outer_height(
                    child,
                    stylesheets,
                    available_width,
                )
            })
            .sum::<f32>();
        inline_height + block_height
    }
}

pub(in crate::layout::flex) fn flex_estimated_content_width(
    style: &ComputedStyle,
    available_width: f32,
) -> f32 {
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    used_content_box_width_or_auto(
        style,
        layout_pt(available_width),
        non_content_pt(horizontal_non_content),
    )
    .map(SemanticLengthExt::points)
    .unwrap_or_else(|| (available_width - horizontal_non_content).max(1.0))
    .max(1.0)
}
