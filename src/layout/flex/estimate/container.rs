use super::*;
use crate::layout::flex::compute::estimated_outer_cross_size;
use crate::layout::flex::compute::{
    flex_baseline_set, flex_item_baseline_axis_is_parallel_to_main_axis,
};

#[derive(Debug, Clone, Copy)]
struct FlexIntrinsicContainerPhysicalSizes {
    min_width: PhysicalContentWidth,
    width: PhysicalContentWidth,
    min_height: PhysicalContentHeight,
    height: PhysicalContentHeight,
}

impl FlexIntrinsicContainerPhysicalSizes {
    fn from_axis_contributions(
        physical_direction: FlexDirection,
        min_main: FlexMainSize,
        max_main: FlexMainSize,
        min_cross: FlexCrossSize,
        max_cross: FlexCrossSize,
    ) -> Self {
        if physical_direction.is_column_axis() {
            Self {
                min_width: PhysicalContentWidth::new(flex_cross_content_box_length(min_cross)),
                width: PhysicalContentWidth::new(flex_cross_content_box_length(max_cross)),
                min_height: PhysicalContentHeight::new(flex_main_content_box_length(min_main)),
                height: PhysicalContentHeight::new(flex_main_content_box_length(max_main)),
            }
        } else {
            Self {
                min_width: PhysicalContentWidth::new(flex_main_content_box_length(min_main)),
                width: PhysicalContentWidth::new(flex_main_content_box_length(max_main)),
                min_height: PhysicalContentHeight::new(flex_cross_content_box_length(min_cross)),
                height: PhysicalContentHeight::new(flex_cross_content_box_length(max_cross)),
            }
        }
    }

    fn include_multiline_cross_size(
        &mut self,
        physical_direction: FlexDirection,
        cross_size: FlexCrossSize,
    ) {
        if physical_direction.is_row_axis() {
            let cross_size = PhysicalContentHeight::new(flex_cross_content_box_length(cross_size));
            self.height = self.height.max(cross_size);
            self.min_height = self.min_height.max(cross_size);
        } else {
            let cross_size = PhysicalContentWidth::new(flex_cross_content_box_length(cross_size));
            self.width = self.width.max(cross_size);
            self.min_width = self.min_width.max(cross_size);
        }
    }

    fn resolved_width(
        self,
        style: &ComputedStyle,
        available: FlexAvailableSpace,
    ) -> PhysicalContentWidth {
        used_length_percentage_or_auto_with_basis(
            style.box_values.width.clone(),
            available.width_basis,
        )
        .map(layout_to_content_box_length)
        .map(PhysicalContentWidth::new)
        .unwrap_or(self.width)
    }

    fn resolved_height(
        self,
        style: &ComputedStyle,
        available: FlexAvailableSpace,
    ) -> PhysicalContentHeight {
        let percentage_basis = available
            .height
            .map(PhysicalContentHeight::content_box_length)
            .unwrap_or_else(|| available.width.content_box_length())
            .into_layout_length();
        used_length_percentage_or_auto(
            style.box_values.height.value().clone(),
            PercentageBasis::definite(percentage_basis),
        )
        .map(layout_to_content_box_length)
        .map(PhysicalContentHeight::new)
        .or(available.height)
        .unwrap_or(self.height)
    }

    fn intrinsic_metrics(
        self,
        width: PhysicalContentWidth,
        height: PhysicalContentHeight,
    ) -> FlexPhysicalIntrinsicMetrics {
        FlexPhysicalIntrinsicMetrics {
            width,
            height,
            min_width: self.min_width,
            min_height: self.min_height,
            content_width: self.width,
            content_height: self.height,
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::flex) fn estimate_intrinsic_flex_container_size(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available: FlexAvailableSpace,
    ) -> FlexItemEstimate {
        // A flex container's intrinsic dimensions come only from flex-item
        // contributions. Unlike an inline formatting context, it has no line
        // strut to contribute a font-size or line-height floor, including when
        // it is empty. Such a floor would feed a synthetic size into
        // shrink-to-fit ancestors and incorrectly expose a container's
        // background around its flex items.
        // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
        let physical_direction = physical_flex_direction(style);
        let border_widths = used_border_widths(style);
        let PhysicalFlexGaps {
            horizontal: physical_gap_width,
            vertical: physical_gap_height,
        } = physical_flex_gaps(style);
        // A definite cross size on the flex container is available while
        // measuring its items for intrinsic main-size contributions. In
        // particular, stretch alignment can transfer that size through a
        // preferred aspect ratio before the inline flex container's own main
        // size is known:
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
        // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
        let intrinsic_item_available = definite_flex_item_cross_content_size(
            style,
            physical_direction,
            available.cross_basis(physical_direction),
            available.logical_inline_basis(style),
        )
        .map(|cross_size| {
            flex_available_with_definite_cross_size(
                available,
                physical_direction,
                flex_cross_size_from_content_box(cross_size),
            )
        })
        .unwrap_or(available);
        let (mut intrinsic_items, estimated_baseline_items) = self.estimate_flex_intrinsic_items(
            children,
            style,
            stylesheets,
            intrinsic_item_available,
            physical_direction,
        );
        // A collapsed item does not contribute to the flex main-size
        // calculation, but Flexbox preserves its hypothetical cross size as
        // a strut.  Keep that distinction while computing an inline flex
        // container's intrinsic cross size; otherwise a column flex box can
        // shrink to its visible 10pt child despite a collapsed 50pt child.
        // <https://www.w3.org/TR/css-flexbox-1/#visibility-collapse>
        let collapsed_cross_strut = children
            .iter()
            .filter(|child| flex_item_is_collapsed(&child.style))
            .map(|child| {
                let item_available = estimated_flex_item_available_space(
                    &child.style,
                    style,
                    physical_direction,
                    intrinsic_item_available,
                );
                let estimate = self.estimate_flex_item_size(
                    child,
                    stylesheets,
                    item_available,
                    physical_direction,
                );
                flex_cross_size_from_layout_extent(estimated_outer_cross_size(
                    &child.style,
                    estimate,
                    physical_direction,
                ))
            })
            .fold(FlexCrossSize::new(0.0), FlexCrossSize::max);
        if style.flex_wrap == FlexWrap::NoWrap || intrinsic_items.len() == 1 {
            let available_main_size =
                intrinsic_item_available.definite_main_size(physical_direction);
            apply_single_line_flexed_main_cross_contributions(
                &mut intrinsic_items,
                physical_direction,
                available_main_size,
            );
        }

        let intrinsic_main_gap =
            estimated_intrinsic_flex_gap(if physical_direction.is_column_axis() {
                physical_gap_height.clone()
            } else {
                physical_gap_width.clone()
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
            flex_main_gap_size(intrinsic_main_gap),
            intrinsic_item_available,
        );
        let max_main = intrinsic_flex_container_max_main_size(
            style,
            physical_direction,
            &intrinsic_items,
            flex_main_gap_size(intrinsic_main_gap),
            intrinsic_item_available,
        );
        let (mut min_cross, mut max_cross) = intrinsic_flex_container_cross_sizes(
            style,
            physical_direction,
            &intrinsic_items,
            IntrinsicFlexCrossSizeInputs {
                main_gap: flex_main_gap_size(intrinsic_main_gap),
                cross_gap: flex_cross_gap_size(intrinsic_cross_gap),
                available: intrinsic_item_available,
                min_main,
                max_main,
            },
        );
        if physical_direction.is_column_axis()
            && style.flex_wrap != FlexWrap::NoWrap
            && !style.flex_wrap.balances_lines()
        {
            let available_cross_size = intrinsic_items
                .iter()
                .map(|item| item.max_cross_contribution)
                .fold(FlexCrossSize::new(0.0), FlexCrossSize::max);
            if available_cross_size.is_positive() {
                let constrained_available = flex_available_with_definite_cross_size(
                    intrinsic_item_available,
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
                    flex_main_gap_size(intrinsic_main_gap),
                    constrained_available,
                );
                let max_content_max_main = intrinsic_flex_container_max_main_size(
                    style,
                    physical_direction,
                    &max_content_items,
                    flex_main_gap_size(intrinsic_main_gap),
                    constrained_available,
                );
                let (_, max_content_cross) = intrinsic_flex_container_cross_sizes(
                    style,
                    physical_direction,
                    &max_content_items,
                    IntrinsicFlexCrossSizeInputs {
                        main_gap: flex_main_gap_size(intrinsic_main_gap),
                        cross_gap: flex_cross_gap_size(intrinsic_cross_gap),
                        available: constrained_available,
                        min_main: max_content_min_main,
                        max_main: max_content_max_main,
                    },
                );
                max_cross = max_content_cross;
            }
        }
        min_cross = min_cross.max(collapsed_cross_strut);
        max_cross = max_cross.max(collapsed_cross_strut);
        let mut physical_sizes = FlexIntrinsicContainerPhysicalSizes::from_axis_contributions(
            physical_direction,
            min_main,
            max_main,
            min_cross,
            max_cross,
        );

        let line_metrics = estimate_row_flex_container_line_metrics(
            style,
            intrinsic_item_available,
            &estimated_baseline_items,
        );
        if let Some(metrics) = line_metrics
            && style.flex_wrap != FlexWrap::NoWrap
            && metrics.line_count > 1
        {
            physical_sizes.include_multiline_cross_size(physical_direction, metrics.cross_size);
        }

        let (first_baseline, last_baseline) = line_metrics
            .map(|metrics| (metrics.first_baseline, metrics.last_baseline))
            .unwrap_or((None, None));
        let baselines = if physical_direction.is_row_axis() {
            let border_box_cross_inset =
                FlexCrossLength::new(border_widths.top + style.padding.top);
            FlexItemBaselineEstimate {
                vertical: FlexItemBaselinePair {
                    first: first_baseline.map(|baseline| {
                        flex_vertical_baseline_from_cross_offset(baseline + border_box_cross_inset)
                    }),
                    last: last_baseline.map(|baseline| {
                        flex_vertical_baseline_from_cross_offset(baseline + border_box_cross_inset)
                    }),
                },
                horizontal: FlexItemBaselinePair::default(),
            }
        } else if style.flex_direction.is_row_axis() {
            let border_box_cross_inset =
                FlexCrossLength::new(border_widths.left + style.padding.left);
            FlexItemBaselineEstimate {
                vertical: FlexItemBaselinePair::default(),
                horizontal: FlexItemBaselinePair {
                    first: first_baseline.map(|baseline| {
                        flex_horizontal_baseline_from_cross_offset(
                            baseline + border_box_cross_inset,
                        )
                    }),
                    last: last_baseline.map(|baseline| {
                        flex_horizontal_baseline_from_cross_offset(
                            baseline + border_box_cross_inset,
                        )
                    }),
                },
            }
        } else {
            FlexItemBaselineEstimate::default()
        };
        let width = physical_sizes.resolved_width(style, available);
        let height = physical_sizes.resolved_height(style, available);
        FlexItemEstimate::from_physical_intrinsic_metrics(
            physical_sizes.intrinsic_metrics(width, height),
            style.aspect_ratio.preferred_ratio(false, None),
            baselines,
        )
    }

    fn estimate_flex_intrinsic_items(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available: FlexAvailableSpace,
        physical_direction: FlexDirection,
    ) -> (Vec<FlexIntrinsicItem>, Vec<EstimatedFlexBaselineItem>) {
        let mut intrinsic_items = Vec::with_capacity(children.len());
        let mut estimated_baseline_items = Vec::with_capacity(children.len());

        for child in children {
            // Collapsed flex items are removed from flex layout before the
            // container's intrinsic main-size contributions are calculated.
            // Their original cross size survives only as a strut in the
            // final flex layout pass; including their main contribution here
            // incorrectly widens an auto-sized container and leaves a gap
            // where the collapsed item used to be.
            // <https://drafts.csswg.org/css-flexbox-1/#visibility-collapse>.
            if flex_item_is_collapsed(&child.style) {
                continue;
            }
            let item_available = estimated_flex_item_available_space(
                &child.style,
                style,
                physical_direction,
                available,
            );
            let size = self.estimate_flex_item_size(
                child,
                stylesheets,
                item_available,
                physical_direction,
            );
            let item = FlexIntrinsicItem::new(child, size, physical_direction, available, style);
            let (first_baseline, last_baseline) =
                estimated_flex_item_cross_axis_baselines(size, physical_direction);
            estimated_baseline_items.push(EstimatedFlexBaselineItem {
                outer_main_size: item.flex_base_size,
                outer_cross_size: item.max_cross_contribution,
                margin_cross_start: if physical_direction.is_row_axis() {
                    FlexCrossLength::new(child.style.margin.top)
                } else {
                    FlexCrossLength::new(child.style.margin.left)
                },
                cross_alignment: estimated_flex_item_cross_alignment(&child.style, style),
                baseline_set: flex_baseline_set(&child.style, style).filter(|_| {
                    flex_item_baseline_axis_is_parallel_to_main_axis(
                        &child.style,
                        physical_direction,
                    ) && !estimated_flex_item_has_auto_cross_margin(
                        &child.style,
                        physical_direction,
                    )
                }),
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
        stylesheets: &Stylesheets<'_>,
        containing_inline_size: LogicalInlineContentSize,
    ) -> Option<LogicalBlockContentSize> {
        let containing_width = containing_inline_size.points();
        let multicol_style = self.multicol_used_style(&child.style);
        let style = &multicol_style;
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

        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(containing_width)),
            style.font_size,
        )
        .points();
        let column_count =
            used_multicol_column_count(style, containing_width, gap).filter(|count| *count > 1)?;
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((containing_width - total_gap) / column_count as f32).max(1.0);
        let mut column_heights = vec![FlexBlockStackContribution::zero(); column_count];

        for (group_index, group) in groups.iter().enumerate() {
            let column_index = group_index % column_count;
            for item in group {
                let item_height = self.estimate_flex_column_item_height(
                    item,
                    stylesheets,
                    LogicalInlineContentSize::new(content_box_pt(column_width)),
                );
                column_heights[column_index] = column_heights[column_index].plus(item_height);
            }
        }

        column_heights
            .into_iter()
            .reduce(FlexBlockStackContribution::max)
            .map(FlexBlockStackContribution::as_content_size)
    }

    fn estimate_flex_column_item_height(
        &mut self,
        item: &DefinitionListColumnItem<'_>,
        stylesheets: &Stylesheets<'_>,
        available_inline_size: LogicalInlineContentSize,
    ) -> FlexBlockStackContribution {
        let content_width =
            (available_inline_size.points() - item.style.padding.left - item.style.padding.right)
                .max(1.0);
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
            .max(self.font_system.used_line_height(&item.style).points());

        FlexBlockStackContribution::from_outer_extent(layout_pt(
            item.style.margin.top
                + vertical_border_width(&item.style)
                + item.style.padding.top
                + content_height
                + item.style.padding.bottom
                + item.style.margin.bottom,
        ))
    }
}
