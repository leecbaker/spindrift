use super::*;
use crate::units::IntoLayoutLength;

/// Return a vertical-writing flex item's first text baseline offset from the
/// border-box left edge.
///
/// CSS Flexbox baseline alignment can align row flex lines in the horizontal
/// cross axis when the row main axis is vertical. CSS Writing Modes makes the
/// central baseline dominant for vertical `text-orientation:mixed` and
/// `upright`; `sideways` uses the alphabetic baseline of rotated horizontal
/// text:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>,
/// <https://www.w3.org/TR/css-writing-modes-4/#text-baselines>, and
/// <https://drafts.csswg.org/css-align-3/#synthesize-baseline>.
pub(in crate::layout::flex) fn first_horizontal_text_baseline_offset(
    style: &ComputedStyle,
    border_box_width: BorderBoxLength,
    line_baseline_offset: LayoutLength,
) -> Option<FlexHorizontalBaselineOffset> {
    horizontal_text_baseline_offset(
        style,
        border_box_width,
        layout_pt(0.0),
        line_baseline_offset,
    )
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn vertical_rl_sideways_horizontal_baseline_uses_line_box_offset() {
        let mut style = ComputedStyle {
            writing_mode: WritingMode::VerticalRl,
            line_height: 100.0,
            font_size: 0.0,
            ..ComputedStyle::initial()
        };
        style.text_orientation = css::TextOrientation::Sideways;

        assert_eq!(
            horizontal_text_baseline_offset(
                &style,
                border_box_pt(100.0),
                layout_pt(0.0),
                layout_pt(50.0),
            ),
            Some(flex_horizontal_baseline_from_points(50.0))
        );
    }

    #[test]
    fn vertical_rl_horizontal_baseline_uses_border_box_width() {
        let mut style = ComputedStyle {
            writing_mode: WritingMode::VerticalRl,
            line_height: 20.0,
            font_size: 0.0,
            ..ComputedStyle::initial()
        };
        style.padding.left = 3.0;
        style.padding.right = 7.0;
        style.border_widths.left = 2.0;
        style.border_widths.right = 5.0;
        style.border_styles.left = BorderStyle::Solid;
        style.border_styles.right = BorderStyle::Solid;

        assert_eq!(
            horizontal_text_baseline_offset(
                &style,
                border_box_pt(117.0),
                layout_pt(0.0),
                layout_pt(10.0),
            ),
            Some(flex_horizontal_baseline_from_points(95.0))
        );
    }

    #[test]
    fn sideways_baselines_remain_alphabetic_when_text_orientation_is_upright() {
        let mut style = ComputedStyle {
            writing_mode: WritingMode::SidewaysLr,
            line_height: 100.0,
            font_size: 0.0,
            ..ComputedStyle::initial()
        };
        style.text_orientation = css::TextOrientation::Upright;

        assert!(!vertical_text_uses_central_baseline(&style));
        assert_eq!(
            horizontal_text_baseline_offset(
                &style,
                border_box_pt(100.0),
                layout_pt(0.0),
                layout_pt(12.0),
            ),
            Some(flex_horizontal_baseline_from_points(12.0))
        );
    }
}

/// Return a vertical-writing flex item's last text baseline offset from its
/// border-box left edge.
///
/// The line stack advances in the block direction. `vertical-lr` measures that
/// advance from the left content edge, while `vertical-rl` mirrors it from the
/// right content edge:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
pub(in crate::layout::flex) fn last_horizontal_text_baseline_offset(
    style: &ComputedStyle,
    border_box_width: BorderBoxLength,
    preceding_line_height: LayoutLength,
    line_baseline_offset: LayoutLength,
) -> Option<FlexHorizontalBaselineOffset> {
    horizontal_text_baseline_offset(
        style,
        border_box_width,
        preceding_line_height,
        line_baseline_offset,
    )
}

pub(in crate::layout::flex) fn horizontal_text_baseline_offset(
    style: &ComputedStyle,
    border_box_width: BorderBoxLength,
    line_stack_offset: LayoutLength,
    line_baseline_offset: LayoutLength,
) -> Option<FlexHorizontalBaselineOffset> {
    let borders = used_border_widths(style);
    let line_baseline_offset = if vertical_text_uses_central_baseline(style) {
        layout_pt(style.line_height / 2.0)
    } else {
        line_baseline_offset
    };
    let content_baseline_offset = line_stack_offset.points() + line_baseline_offset.points();
    match WritingModeAxes::new(style.writing_mode, style.used_direction())
        .physical_side(LogicalSide::BlockStart)
    {
        PhysicalSide::Top | PhysicalSide::Bottom => None,
        PhysicalSide::Left => Some(flex_horizontal_baseline_from_points(
            borders.left + style.padding.left + content_baseline_offset,
        )),
        PhysicalSide::Right => Some(flex_horizontal_baseline_from_points(
            border_box_width.points()
                - borders.right
                - style.padding.right
                - content_baseline_offset,
        )),
    }
}

pub(in crate::layout::flex) fn vertical_text_uses_central_baseline(style: &ComputedStyle) -> bool {
    matches!(
        style.text_layout_policy(),
        css::TextLayoutPolicy::Vertical(
            css::TextOrientation::Mixed | css::TextOrientation::Upright
        )
    )
}

pub(in crate::layout::flex) fn preceding_line_height_before_last(
    sequence: &inline_layout::InlineLineSequence,
) -> LayoutLength {
    (0..sequence.records.len().saturating_sub(1))
        .map(|index| sequence.line_height(index))
        .map(layout_pt)
        .fold(layout_pt(0.0), |sum, height| {
            layout_pt(sum.points() + height.points())
        })
}

pub(in crate::layout::flex) fn first_sequence_line_baseline_offset(
    sequence: &inline_layout::InlineLineSequence,
    fallback: LayoutLength,
) -> LayoutLength {
    sequence
        .records
        .first()
        .and_then(|record| record.fragment.as_ref())
        .map(|fragment| layout_pt(fragment.metrics.baseline_offset))
        .unwrap_or(fallback)
}

pub(in crate::layout::flex) fn last_sequence_line_baseline_offset(
    sequence: &inline_layout::InlineLineSequence,
    fallback: LayoutLength,
) -> LayoutLength {
    sequence
        .records
        .last()
        .and_then(|record| record.fragment.as_ref())
        .map(|fragment| layout_pt(fragment.metrics.baseline_offset))
        .unwrap_or(fallback)
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
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
    ) -> FlexItemBaselineEstimate {
        let content_width = flex_estimated_content_width(style, available_content_width);
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
            vertical: FlexItemBaselinePair {
                first: baselines
                    .vertical
                    .first
                    .map(|baseline| baseline.offset_by(layout_pt(borders.top + style.padding.top))),
                last: baselines
                    .vertical
                    .last
                    .map(|baseline| baseline.offset_by(layout_pt(borders.top + style.padding.top))),
            },
            horizontal: FlexItemBaselinePair {
                first: baselines.horizontal.first.map(|baseline| {
                    baseline.offset_by(layout_pt(borders.left + style.padding.left))
                }),
                last: baselines.horizontal.last.map(|baseline| {
                    baseline.offset_by(layout_pt(borders.left + style.padding.left))
                }),
            },
        }
    }

    fn estimate_flex_children_baselines(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
    ) -> FlexItemBaselineEstimate {
        if let Some(child_boxes) = child_boxes {
            return self.estimate_flex_box_children_baselines(
                child_boxes,
                style,
                stylesheets,
                available_content_width,
            );
        }
        self.estimate_flex_dom_children_baselines(
            element,
            style,
            stylesheets,
            available_content_width,
        )
    }

    fn estimate_flex_box_children_baselines(
        &mut self,
        child_boxes: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
    ) -> FlexItemBaselineEstimate {
        if formatting_box_has_inline_content(child_boxes)
            && !has_non_inline_formatting_box(child_boxes)
        {
            return self.estimate_flex_inline_children_baselines(
                child_boxes,
                style,
                stylesheets,
                available_content_width,
            );
        }

        let mut block_offset = layout_pt(0.0);
        let mut first_baseline = None;
        let mut last_baseline = None;

        for child in child_boxes {
            if !formatting_box_is_in_normal_flow(child) {
                continue;
            }
            let child_baselines = self.estimate_flex_formatting_child_baselines(
                child,
                stylesheets,
                available_content_width,
            );
            if let Some(baseline) = child_baselines.vertical.first {
                first_baseline.get_or_insert(baseline.offset_by(block_offset));
            }
            if let Some(baseline) = child_baselines.vertical.last {
                last_baseline = Some(baseline.offset_by(block_offset));
            }
            block_offset += self
                .estimate_flex_formatting_child_outer_height(
                    child,
                    stylesheets,
                    available_content_width,
                )
                .into_layout_length();
        }

        FlexItemBaselineEstimate {
            vertical: FlexItemBaselinePair {
                first: first_baseline,
                last: last_baseline,
            },
            ..Default::default()
        }
    }

    fn estimate_flex_dom_children_baselines(
        &mut self,
        element: &Element,
        parent_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
    ) -> FlexItemBaselineEstimate {
        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        let mut block_offset = layout_pt(0.0);
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
                available_content_width,
            );
            if let Some(baseline) = child_baselines.vertical.first {
                first_baseline.get_or_insert(baseline.offset_by(block_offset));
            }
            if let Some(baseline) = child_baselines.vertical.last {
                last_baseline = Some(baseline.offset_by(block_offset));
            }
            block_offset += layout_pt(
                self.estimate_element_height(
                    child_element,
                    &child_style,
                    stylesheets,
                    available_content_width.points(),
                    None,
                )
                .unwrap_or(0.0),
            );
        }

        FlexItemBaselineEstimate {
            vertical: FlexItemBaselinePair {
                first: first_baseline,
                last: last_baseline,
            },
            ..Default::default()
        }
    }

    fn estimate_flex_formatting_child_baselines(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
    ) -> FlexItemBaselineEstimate {
        match child {
            box_tree::FormattingBox::Text(box_) => {
                if box_tree::formatting_box_is_collapsible_space(child) {
                    return FlexItemBaselineEstimate::default();
                }
                let baseline = self.inline_box_text_line_layout_baseline_offset(&box_.style);
                FlexItemBaselineEstimate {
                    vertical: FlexItemBaselinePair {
                        first: Some(flex_vertical_baseline_from_points(baseline)),
                        last: Some(flex_vertical_baseline_from_points(baseline)),
                    },
                    ..Default::default()
                }
            }
            box_tree::FormattingBox::Inline(box_) => self.estimate_flex_inline_children_baselines(
                &box_.core.children,
                &box_.core.style,
                stylesheets,
                available_content_width,
            ),
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .estimate_flex_box_children_baselines(
                    &box_.children,
                    &box_.style,
                    stylesheets,
                    available_content_width,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .estimate_flex_box_children_baselines(
                    &box_.core.children,
                    &box_.core.style,
                    stylesheets,
                    available_content_width,
                ),
            box_tree::FormattingBox::Block(box_) => self.estimate_flex_element_baselines(
                box_.core.element,
                &box_.core.signature,
                &box_.core.style,
                Some(&box_.core.children),
                stylesheets,
                available_content_width,
            ),
            box_tree::FormattingBox::Flex(box_) => self.estimate_flex_element_baselines(
                box_.core.element,
                &box_.core.signature,
                &box_.core.style,
                Some(&box_.core.children),
                stylesheets,
                available_content_width,
            ),
            box_tree::FormattingBox::Table(box_) => self.estimate_flex_element_baselines(
                box_.core.element,
                &box_.core.signature,
                &box_.core.style,
                Some(&box_.core.children),
                stylesheets,
                available_content_width,
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
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
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
                available_content_width,
            )
        {
            return baselines;
        }

        let child_width = flex_estimated_content_width(style, available_content_width);
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
            vertical: FlexItemBaselinePair {
                first: child_baselines.vertical.first.map(|baseline| {
                    baseline.offset_by(layout_pt(
                        style.margin.top + borders.top + style.padding.top,
                    ))
                }),
                last: child_baselines.vertical.last.map(|baseline| {
                    baseline.offset_by(layout_pt(
                        style.margin.top + borders.top + style.padding.top,
                    ))
                }),
            },
            horizontal: FlexItemBaselinePair {
                first: child_baselines.horizontal.first.map(|baseline| {
                    baseline.offset_by(layout_pt(
                        style.margin.left + borders.left + style.padding.left,
                    ))
                }),
                last: child_baselines.horizontal.last.map(|baseline| {
                    baseline.offset_by(layout_pt(
                        style.margin.left + borders.left + style.padding.left,
                    ))
                }),
            },
        }
    }

    fn estimate_nested_flex_baselines(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
    ) -> Option<FlexItemBaselineEstimate> {
        // A nested flex container's exported baseline depends on its used
        // inline size: that size determines wrapped-line placement. Resolve
        // an authored width before making the intrinsic baseline probe;
        // probing against the parent's width can select the wrong
        // wrap-reverse line.
        // <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        let borders = used_border_widths(style);
        let horizontal_non_content =
            non_content_pt(style.padding.left + style.padding.right + borders.left + borders.right);
        let resolved_content_width = used_content_box_width_or_auto_with_basis(
            style,
            PercentageBasis::definite(available_content_width.content_box_length()),
            horizontal_non_content,
        );
        let nested_available_width = resolved_content_width
            .map(PhysicalContentWidth::new)
            .unwrap_or(available_content_width);
        let nested_width_basis = resolved_content_width
            .map(|width| {
                PercentageBasis::definite_from(
                    width,
                    FlexAvailableSizeSource::IntrinsicContainerSize,
                )
            })
            .unwrap_or_else(PercentageBasis::indefinite);
        let baselines = self.with_ancestor_signature(signature.clone(), |layout| {
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
                // Baseline export is a final-placement property.  Intrinsic
                // line metrics can select a different wrapped line from the
                // used nested flex layout (notably with `wrap-reverse`), so
                // compute the nested flex record at its resolved available
                // size here rather than reusing only its intrinsic metrics.
                layout
                    .compute_flex_layout(
                        &children,
                        style,
                        stylesheets,
                        FlexAvailableSpace {
                            width: nested_available_width,
                            width_basis: nested_width_basis,
                            height: used_length_percentage_or_auto(
                                style.box_values.height.value().clone(),
                                PercentageBasis::definite(layout_pt(
                                    available_content_width.points(),
                                )),
                            )
                            .map(|height| {
                                PhysicalContentHeight::new(
                                    crate::units::layout_to_content_box_length(height),
                                )
                            }),
                            height_basis: flex_available_percentage_basis_from_points(
                                used_length_percentage_or_auto(
                                    style.box_values.height.value().clone(),
                                    PercentageBasis::definite(layout_pt(
                                        available_content_width.points(),
                                    )),
                                )
                                .map(|height| height.points()),
                                FlexAvailableSizeSource::IntrinsicContainerSize,
                            ),
                        },
                    )
                    .map(|layout| layout.baselines)
            })
        })??;
        Some(FlexItemBaselineEstimate {
            vertical: FlexItemBaselinePair {
                first: baselines
                    .vertical
                    .first
                    .map(|baseline| baseline.offset_by(layout_pt(style.margin.top))),
                last: baselines
                    .vertical
                    .last
                    .map(|baseline| baseline.offset_by(layout_pt(style.margin.top))),
            },
            horizontal: FlexItemBaselinePair {
                first: baselines
                    .horizontal
                    .first
                    .map(|baseline| baseline.offset_by(layout_pt(style.margin.left))),
                last: baselines
                    .horizontal
                    .last
                    .map(|baseline| baseline.offset_by(layout_pt(style.margin.left))),
            },
        })
    }

    fn estimate_flex_inline_children_baselines(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
    ) -> FlexItemBaselineEstimate {
        if !formatting_box_has_inline_content(children) {
            return FlexItemBaselineEstimate::default();
        }

        let measurement = self.intrinsic_inline_measurement_for_boxes(
            children,
            style,
            stylesheets,
            available_content_width.points(),
        );
        if measurement.line_count() == 0 {
            return FlexItemBaselineEstimate::default();
        }

        let first_baseline = self.inline_box_text_line_layout_baseline_offset(style);
        let last_baseline =
            first_baseline + measurement.line_count().saturating_sub(1) as f32 * style.line_height;
        let fallback_line_baseline_offset =
            layout_pt(self.inline_box_text_line_layout_baseline_offset(style));
        let first_line_baseline_offset = first_sequence_line_baseline_offset(
            &measurement.sequence,
            fallback_line_baseline_offset,
        );
        let last_line_baseline_offset = last_sequence_line_baseline_offset(
            &measurement.sequence,
            fallback_line_baseline_offset,
        );
        let preceding_line_height = preceding_line_height_before_last(&measurement.sequence);
        let borders = used_border_widths(style);
        let horizontal_non_content =
            non_content_pt(borders.left + borders.right + style.padding.left + style.padding.right);
        let border_box_width = content_box_to_border_box_length(
            available_content_width.content_box_length(),
            horizontal_non_content,
        );

        FlexItemBaselineEstimate {
            vertical: FlexItemBaselinePair {
                first: Some(flex_vertical_baseline_from_points(first_baseline)),
                last: Some(flex_vertical_baseline_from_points(last_baseline)),
            },
            horizontal: FlexItemBaselinePair {
                first: first_horizontal_text_baseline_offset(
                    style,
                    border_box_width,
                    first_line_baseline_offset,
                ),
                last: last_horizontal_text_baseline_offset(
                    style,
                    border_box_width,
                    preceding_line_height,
                    last_line_baseline_offset,
                ),
            },
        }
    }

    fn estimate_flex_formatting_child_outer_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
    ) -> MarginBoxLength {
        match child {
            box_tree::FormattingBox::Block(box_) => self
                .estimate_element_height(
                    box_.core.element,
                    &box_.core.style,
                    stylesheets,
                    available_content_width.points(),
                    Some(&box_.core.children),
                )
                .map(margin_box_pt)
                .unwrap_or_else(|| margin_box_pt(0.0)),
            box_tree::FormattingBox::Flex(box_) => self
                .estimate_element_height(
                    box_.core.element,
                    &box_.core.style,
                    stylesheets,
                    available_content_width.points(),
                    Some(&box_.core.children),
                )
                .map(margin_box_pt)
                .unwrap_or_else(|| margin_box_pt(0.0)),
            box_tree::FormattingBox::Table(box_) => self
                .estimate_element_height(
                    box_.core.element,
                    &box_.core.style,
                    stylesheets,
                    available_content_width.points(),
                    Some(&box_.core.children),
                )
                .map(margin_box_pt)
                .unwrap_or_else(|| margin_box_pt(0.0)),
            box_tree::FormattingBox::Replaced(box_) => self
                .estimate_element_height(
                    box_.core.element,
                    &box_.core.style,
                    stylesheets,
                    available_content_width.points(),
                    Some(&box_.core.children),
                )
                .map(margin_box_pt)
                .unwrap_or_else(|| margin_box_pt(0.0)),
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .estimate_flex_anonymous_outer_height(
                    &box_.children,
                    &box_.style,
                    stylesheets,
                    available_content_width,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .estimate_flex_anonymous_outer_height(
                    &box_.core.children,
                    &box_.core.style,
                    stylesheets,
                    available_content_width,
                ),
            box_tree::FormattingBox::Inline(_)
            | box_tree::FormattingBox::AtomicInline(_)
            | box_tree::FormattingBox::Text(_) => margin_box_pt(0.0),
        }
    }

    fn estimate_flex_anonymous_outer_height(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_content_width: PhysicalContentWidth,
    ) -> MarginBoxLength {
        // Inline measurement remains a legacy scalar API; immediately label
        // its result as a parent block-stack extent before combining it with
        // in-flow children's margin boxes.
        let inline_stack_height = if formatting_box_has_inline_content(children)
            && !has_non_inline_formatting_box(children)
        {
            layout_pt(
                self.intrinsic_inline_measurement_for_boxes(
                    children,
                    style,
                    stylesheets,
                    available_content_width.points(),
                )
                .height(),
            )
        } else {
            layout_pt(0.0)
        };
        let block_stack_height = children
            .iter()
            .filter(|child| formatting_box_is_in_normal_flow(child))
            .map(|child| {
                self.estimate_flex_formatting_child_outer_height(
                    child,
                    stylesheets,
                    available_content_width,
                )
            })
            .fold(layout_pt(0.0), |sum, height| {
                sum + height.into_layout_length()
            });
        margin_box_pt((inline_stack_height + block_stack_height).points())
    }
}
