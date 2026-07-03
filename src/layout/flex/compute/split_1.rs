use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexCollapseMode {
    IncludeCollapsed,
    OmitCollapsed,
}

impl FlexCollapseMode {
    pub(in crate::layout::flex) fn omits_collapsed(self) -> bool {
        matches!(self, Self::OmitCollapsed)
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::flex) fn compute_flex_layout(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
    ) -> Option<FlexLayout> {
        let collapsed_struts = if children
            .iter()
            .any(|child| flex_item_is_collapsed(&child.style))
        {
            let visible_layout = self.compute_flex_layout_internal(
                children,
                style,
                stylesheets,
                available,
                FlexCollapseMode::IncludeCollapsed,
                &[],
            )?;
            collapsed_struts_from_visible_layout(children, style, &visible_layout)
        } else {
            Vec::new()
        };
        self.compute_flex_layout_internal(
            children,
            style,
            stylesheets,
            available,
            FlexCollapseMode::OmitCollapsed,
            &collapsed_struts,
        )
    }

    pub(in crate::layout::flex) fn compute_flex_layout_internal(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
        collapse_mode: FlexCollapseMode,
        collapsed_struts: &[FlexCollapsedStrut],
    ) -> Option<FlexLayout> {
        let mut tree: taffy_layout::TaffyTree<FlexItemEstimate> = taffy_layout::TaffyTree::new();
        // CSS Flexbox used sizes are real-valued CSS lengths. Taffy rounds final
        // layouts by default for screen pixels; PDF emission must preserve the
        // unrounded layout and let rasterizers antialias at their output DPI.
        tree.disable_rounding();
        let flex_axes = FlexAxes::for_style(style);
        let physical_direction = flex_axes.physical_direction;
        let (physical_gap_width, physical_gap_height) = physical_flex_gaps(style);
        let mut nodes = Vec::with_capacity(children.len());
        let mut estimates = vec![FlexItemEstimate::fixed(0.0, 0.0); children.len()];
        let mut active_estimates = Vec::with_capacity(children.len());
        let mut source_indices = Vec::with_capacity(children.len());
        let mut estimated_collapsed_struts = Vec::new();
        for (source_index, child) in children.iter().enumerate() {
            let child_style = &child.style;
            let item_available = flex_item_estimate_available_space(
                child_style,
                style,
                physical_direction,
                available,
            );
            let estimated_size = self.estimate_flex_item_size(child, stylesheets, item_available);
            estimates[source_index] = estimated_size;
            if collapse_mode.omits_collapsed() && flex_item_is_collapsed(child_style) {
                estimated_collapsed_struts.push(FlexCollapsedStrut {
                    item_index: source_index,
                    cross_size: estimated_outer_cross_size(
                        child_style,
                        estimated_size,
                        physical_direction,
                    ),
                    source_start: source_index,
                    source_end: source_index + 1,
                });
                continue;
            }
            let flex_basis_overrides_main_size =
                !matches!(child_style.flex_basis, css::ComputedFlexBasis::Auto);
            let preferred_aspect_ratio = child_style.aspect_ratio.preferred_ratio(
                child.is_replaced_element(),
                estimated_size.preferred_aspect_ratio,
            );
            let stretched_cross_size =
                stretched_flex_item_cross_size(child_style, style, physical_direction, available);
            let child_borders = used_border_widths(child_style);
            let horizontal_non_content = child_style.padding.left
                + child_style.padding.right
                + child_borders.left
                + child_borders.right;
            let vertical_non_content = child_style.padding.top
                + child_style.padding.bottom
                + child_borders.top
                + child_borders.bottom;
            let horizontal_stretch = FlexStretchFitContext {
                available_margin_box_size: Some(layout_pt(available.width)),
                margin_size: non_content_pt(child_style.margin.left + child_style.margin.right),
                non_content_size: non_content_pt(horizontal_non_content),
                box_sizing: child_style.box_sizing,
            };
            let vertical_stretch = FlexStretchFitContext {
                available_margin_box_size: available
                    .height
                    .filter(|_| available.height_is_definite)
                    .map(layout_pt),
                margin_size: non_content_pt(child_style.margin.top + child_style.margin.bottom),
                non_content_size: non_content_pt(vertical_non_content),
                box_sizing: child_style.box_sizing,
            };
            let node = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        display: taffy_layout::Display::Flex,
                        box_sizing: match child_style.box_sizing {
                            BoxSizing::BorderBox => taffy_layout::BoxSizing::BorderBox,
                            BoxSizing::ContentBox => taffy_layout::BoxSizing::ContentBox,
                        },
                        direction: taffy_direction(child_style.direction),
                        size: taffy_layout::Size {
                            width: flex_item_size_dimension(
                                child_style.box_values.width,
                                estimated_size.width,
                                estimated_size.min_width,
                                estimated_size.content_width,
                                FlexItemSizeDimensionContext {
                                    flex_direction: physical_direction,
                                    dimension_axis: FlexDirection::Row,
                                    percentage_basis: Some(available.width),
                                    stretch: horizontal_stretch,
                                    flex_basis_overrides_main_size,
                                },
                            ),
                            height: flex_item_size_dimension(
                                child_style.box_values.height,
                                estimated_size.height,
                                estimated_size.min_height,
                                estimated_size.content_height,
                                FlexItemSizeDimensionContext {
                                    flex_direction: physical_direction,
                                    dimension_axis: FlexDirection::Column,
                                    percentage_basis: available
                                        .height
                                        .filter(|_| available.height_is_definite),
                                    stretch: vertical_stretch,
                                    flex_basis_overrides_main_size,
                                },
                            ),
                        },
                        aspect_ratio: preferred_aspect_ratio,
                        min_size: taffy_layout::Size {
                            width: flex_min_size_dimension(
                                child_style.box_values.min_width,
                                estimated_size.min_width,
                                estimated_size.content_width,
                                FlexMinSizeDimensionContext {
                                    definite_preferred_content_size:
                                        used_content_width_or_auto_with_optional_basis(
                                            child_style,
                                            Some(available.width),
                                            horizontal_non_content,
                                        )
                                        .map(content_box_pt),
                                    transferred_size_suggestion:
                                        aspect_ratio_transferred_content_main_size(
                                            child_style,
                                            FlexDirection::Row,
                                            available
                                                .height
                                                .filter(|_| available.height_is_definite),
                                            physical_direction
                                                .is_row_axis()
                                                .then_some(stretched_cross_size)
                                                .flatten(),
                                            preferred_aspect_ratio,
                                        ),
                                    is_replaced: child.is_replaced_element(),
                                    is_main_axis: physical_direction.is_row_axis(),
                                    overflow: flex_item_main_axis_overflow(
                                        child_style,
                                        physical_direction,
                                    ),
                                    percentage_basis: Some(available.width),
                                    stretch: horizontal_stretch,
                                },
                            ),
                            height: flex_min_size_dimension(
                                child_style.box_values.min_height,
                                estimated_size.min_height,
                                estimated_size.content_height,
                                FlexMinSizeDimensionContext {
                                    definite_preferred_content_size:
                                        used_content_height_or_auto_with_optional_basis(
                                            child_style,
                                            available
                                                .height
                                                .filter(|_| available.height_is_definite),
                                            vertical_non_content,
                                        )
                                        .map(content_box_pt),
                                    transferred_size_suggestion:
                                        aspect_ratio_transferred_content_main_size(
                                            child_style,
                                            FlexDirection::Column,
                                            Some(available.width),
                                            physical_direction
                                                .is_column_axis()
                                                .then_some(stretched_cross_size)
                                                .flatten(),
                                            preferred_aspect_ratio,
                                        ),
                                    is_replaced: child.is_replaced_element(),
                                    is_main_axis: physical_direction.is_column_axis(),
                                    overflow: flex_item_main_axis_overflow(
                                        child_style,
                                        physical_direction,
                                    ),
                                    percentage_basis: available
                                        .height
                                        .filter(|_| available.height_is_definite),
                                    stretch: vertical_stretch,
                                },
                            ),
                        },
                        max_size: taffy_layout::Size {
                            width: taffy_intrinsic_dimension_with_basis_and_stretch(
                                child_style.box_values.max_width,
                                Some(available.width),
                                estimated_size.min_width,
                                estimated_size.content_width,
                                horizontal_stretch,
                            ),
                            height: taffy_intrinsic_dimension_with_basis_and_stretch(
                                child_style.box_values.max_height,
                                available.height.filter(|_| available.height_is_definite),
                                estimated_size.min_height,
                                estimated_size.content_height,
                                vertical_stretch,
                            ),
                        },
                        margin: taffy_margin(child_style),
                        padding: taffy_padding(child_style),
                        border: taffy_edges(used_border_widths(child_style)),
                        flex_grow: child_style.flex_grow,
                        flex_shrink: child_style.flex_shrink,
                        flex_basis: taffy_flex_basis(
                            child_style,
                            &estimated_size,
                            FlexBasisContext {
                                direction: physical_direction,
                                available_main_size: if physical_direction.is_row_axis() {
                                    available.width
                                } else {
                                    available
                                        .height
                                        .unwrap_or_else(|| estimated_size.content_height.points())
                                },
                                available_cross_size: if physical_direction.is_row_axis() {
                                    available.height.filter(|_| available.height_is_definite)
                                } else {
                                    Some(available.width)
                                },
                                stretched_cross_size,
                                main_size_is_definite: physical_direction.is_row_axis()
                                    || available.height_is_definite,
                                preferred_aspect_ratio,
                            },
                        ),
                        align_self: taffy_effective_align_self(child_style, style),
                        ..Default::default()
                    },
                    estimated_size,
                )
                .ok()?;
            nodes.push(node);
            active_estimates.push(estimated_size);
            source_indices.push(source_index);
        }

        let root = tree
            .new_with_children(
                taffy_layout::Style {
                    display: taffy_layout::Display::Flex,
                    box_sizing: taffy_layout::BoxSizing::BorderBox,
                    direction: taffy_flex_layout_direction(style, physical_direction),
                    size: taffy_layout::Size {
                        width: taffy_layout::Dimension::length(available.width),
                        height: if available.height_is_definite {
                            available
                                .height
                                .map(taffy_layout::Dimension::length)
                                .unwrap_or_else(taffy_layout::Dimension::auto)
                        } else {
                            taffy_layout::Dimension::auto()
                        },
                    },
                    min_size: taffy_layout::Size {
                        width: taffy_min_dimension(style.box_values.min_width, available.width),
                        height: taffy_min_dimension(style.box_values.min_height, available.width),
                    },
                    max_size: taffy_layout::Size {
                        width: taffy_optional_dimension(style.box_values.max_width),
                        height: taffy_optional_dimension(style.box_values.max_height),
                    },
                    flex_direction: match physical_direction {
                        FlexDirection::Row => taffy_layout::FlexDirection::Row,
                        FlexDirection::RowReverse => taffy_layout::FlexDirection::RowReverse,
                        FlexDirection::Column => taffy_layout::FlexDirection::Column,
                        FlexDirection::ColumnReverse => taffy_layout::FlexDirection::ColumnReverse,
                    },
                    flex_wrap: match style.flex_wrap {
                        FlexWrap::NoWrap => taffy_layout::FlexWrap::NoWrap,
                        FlexWrap::Wrap => taffy_layout::FlexWrap::Wrap,
                        FlexWrap::WrapReverse => taffy_layout::FlexWrap::WrapReverse,
                    },
                    justify_content: taffy_justify_content(
                        style.justify_content,
                        physical_direction,
                    ),
                    align_content: Some(taffy_align_content(style.align_content)),
                    align_items: Some(taffy_align_items(style.align_items)),
                    gap: taffy_layout::Size {
                        width: taffy_gap(physical_gap_width),
                        height: taffy_gap(physical_gap_height),
                    },
                    ..Default::default()
                },
                &nodes,
            )
            .ok()?;

        tree.compute_layout_with_measure(
            root,
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::Definite(available.width),
                height: available
                    .height
                    .map(taffy_layout::AvailableSpace::Definite)
                    .unwrap_or(taffy_layout::AvailableSpace::MaxContent),
            },
            |known_dimensions, available_space, _node_id, node_context, _style| {
                measure_flex_item(known_dimensions, available_space, node_context)
            },
        )
        .ok()?;

        let root_layout = tree.layout(root).ok()?;
        let root_rect = taffy_rect_from_layout(root_layout);
        let mut items = vec![FlexItemLayout::new(0.0, 0.0, 0.0, 0.0); children.len()];
        let mut active_items = Vec::with_capacity(nodes.len());
        for node in nodes {
            let layout = tree.layout(node).ok()?;
            let rect = taffy_rect_from_layout(layout);
            active_items.push(FlexItemLayout::from_taffy_rect(rect, flex_axes));
        }
        let active_children = source_indices
            .iter()
            .map(|&index| children[index].clone())
            .collect::<Vec<_>>();
        let container_cross_size = if physical_direction.is_row_axis() {
            root_rect.size.height
        } else {
            root_rect.size.width
        };
        let mut active_lines = flex_lines_from_items(
            &active_items,
            &active_children,
            &active_estimates,
            style,
            physical_direction,
            container_cross_size,
        );
        let collapsed_struts = if collapsed_struts.is_empty() {
            estimated_collapsed_struts.as_slice()
        } else {
            collapsed_struts
        };
        attach_collapsed_struts_to_active_lines(
            &mut active_lines,
            &source_indices,
            collapsed_struts,
        );
        repack_lines_after_collapsed_struts(
            &mut active_lines,
            &mut active_items,
            physical_direction,
        );
        if apply_line_cross_size_dependent_item_remeasurements(
            self,
            &mut active_items,
            &mut active_estimates,
            &active_children,
            &active_lines,
            FlexLineCrossRemeasureContext {
                container_style: style,
                stylesheets,
                physical_direction,
                available,
            },
        ) {
            refresh_flex_line_metadata(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
        }
        if apply_main_size_aspect_ratio_cross_size_corrections(
            &mut active_items,
            &mut active_estimates,
            &active_children,
            style,
            physical_direction,
            available,
        ) {
            refresh_flex_line_metadata(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
        }
        replace_synthesized_baseline_offsets(
            &mut active_items,
            &active_estimates,
            &active_children,
            &active_lines,
            style,
            physical_direction,
        );
        apply_column_baseline_self_alignment_offsets(
            &mut active_items,
            &active_estimates,
            &active_children,
            &active_lines,
            style,
            physical_direction,
        );
        refresh_flex_line_cross_bounds(
            &mut active_lines,
            &active_items,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        refresh_flex_line_metadata(
            &mut active_lines,
            &active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        apply_baseline_align_content_offsets(
            &mut active_items,
            &mut active_lines,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        refresh_flex_line_metadata(
            &mut active_lines,
            &active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        apply_baseline_self_alignment_fallback_offsets(
            &mut active_items,
            &active_children,
            &active_lines,
            style,
            physical_direction,
        );
        apply_subject_axis_self_alignment_offsets(
            &mut active_items,
            &active_children,
            &active_lines,
            style,
            physical_direction,
        );
        refresh_flex_line_cross_bounds(
            &mut active_lines,
            &active_items,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        refresh_flex_line_metadata(
            &mut active_lines,
            &active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        if apply_main_axis_automatic_minimums(
            &mut active_items,
            &active_estimates,
            &active_children,
            physical_direction,
            available,
        ) {
            repack_lines_after_main_size_adjustment(
                &mut active_lines,
                &mut active_items,
                &active_children,
                style,
                physical_direction,
                if physical_direction.is_row_axis() {
                    root_rect.size.width
                } else {
                    root_rect.size.height
                },
            );
            refresh_flex_line_cross_bounds(
                &mut active_lines,
                &active_items,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
            refresh_flex_line_metadata(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
        }
        apply_stretch_align_content_overflow_fallback_offsets(
            &mut active_items,
            &mut active_lines,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        if apply_non_negative_flex_item_content_box_minimums(&mut active_items, &active_children) {
            refresh_flex_line_metadata(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
        }
        expand_flex_line_cross_bounds_for_item_overflow(
            &mut active_lines,
            &active_items,
            &active_children,
            physical_direction,
        );
        let source_lines = active_lines
            .iter()
            .map(|line| {
                let item_indices = line
                    .item_indices
                    .iter()
                    .map(|&active_index| source_indices[active_index])
                    .collect::<Vec<_>>();
                FlexLineLayout {
                    source_start: item_indices
                        .iter()
                        .copied()
                        .min()
                        .unwrap_or(line.source_start),
                    source_end: item_indices
                        .iter()
                        .copied()
                        .max()
                        .map(|index| index + 1)
                        .unwrap_or(line.source_end),
                    item_indices,
                    main_start: line.main_start,
                    main_end: line.main_end,
                    cross_start: line.cross_start,
                    cross_end: line.cross_end,
                    first_baseline: line.first_baseline,
                    last_baseline: line.last_baseline,
                    collapsed_struts: line.collapsed_struts.clone(),
                }
            })
            .collect::<Vec<_>>();
        for (active_index, source_index) in source_indices.iter().copied().enumerate() {
            items[source_index] = active_items[active_index].clone();
        }
        let item_extent_height = items
            .iter()
            .map(|item| item.y() + item.height())
            .fold(0.0f32, f32::max);
        let collapsed_cross_height = if physical_direction.is_row_axis() {
            source_lines
                .iter()
                .map(|line| line.largest_collapsed_strut())
                .fold(0.0f32, f32::max)
        } else {
            0.0
        };
        let height = if available.height_is_definite {
            root_rect.size.height
        } else if available.height.is_some() {
            item_extent_height.max(collapsed_cross_height)
        } else {
            root_rect
                .size
                .height
                .max(item_extent_height)
                .max(collapsed_cross_height)
        };

        let first_baseline = flex_container_first_baseline(
            &active_items,
            &active_estimates,
            &active_children,
            style,
        );

        let fragment_plan = FlexFragmentPlan::from_unfragmented_lines(&source_lines, &items);

        Some(FlexLayout {
            height,
            first_baseline,
            items,
            lines: source_lines,
            fragment_plan,
        })
    }
}

/// Applies Flexbox's automatic minimum main size to final item layouts.
///
/// CSS Flexbox section 4.5 defines `min-width:auto`/`min-height:auto` on flex
/// items as a content-based automatic minimum in the main axis when overflow is
/// non-scrollable. Taffy remains the primary flex algorithm here, but this guard
/// preserves content and transferred size suggestions when a definite zero-sized
/// flex container would otherwise shrink the final item layout below its
/// automatic minimum:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion>.
pub(in crate::layout::flex) fn apply_main_axis_automatic_minimums(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> bool {
    let mut changed = false;
    let axes = FlexAxes::from_physical_direction(physical_direction);
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        let Some(minimum) =
            automatic_minimum_main_size(child, estimate, physical_direction, available)
        else {
            continue;
        };
        if physical_direction.is_row_axis() {
            if item.width() >= minimum {
                continue;
            }
            let delta = minimum - item.width();
            if matches!(physical_direction, FlexDirection::RowReverse) {
                item.set_main_start(axes, item.main_start(axes) - delta);
            }
            item.set_main_size(axes, minimum);
            changed = true;
        } else {
            if item.height() >= minimum {
                continue;
            }
            let delta = minimum - item.height();
            if matches!(physical_direction, FlexDirection::ColumnReverse) {
                item.set_main_start(axes, item.main_start(axes) - delta);
            }
            item.set_main_size(axes, minimum);
            changed = true;
        }
    }
    changed
}

/// Ensures final flex item border boxes can contain their non-content edges.
///
/// CSS Sizing floors the content box at zero, including stretch-fit sizing
/// where a small target margin box can be smaller than the item's padding and
/// border. Taffy may report a zero final border-box cross size for these cases,
/// so Quire restores the minimum border-box size before painting/replay:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-stretch>.
pub(in crate::layout::flex) fn apply_non_negative_flex_item_content_box_minimums(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
) -> bool {
    let mut changed = false;
    for (item, child) in items.iter_mut().zip(children) {
        let borders = used_border_widths(&child.style);
        let min_width =
            child.style.padding.left + child.style.padding.right + borders.left + borders.right;
        if item.width() < min_width {
            item.set_width(min_width);
            changed = true;
        }

        let min_height =
            child.style.padding.top + child.style.padding.bottom + borders.top + borders.bottom;
        if item.height() < min_height {
            item.set_height(min_height);
            changed = true;
        }
    }
    changed
}

pub(in crate::layout::flex) fn expand_flex_line_cross_bounds_for_item_overflow(
    lines: &mut [FlexLineLayout],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) {
    for line in lines {
        for &index in &line.item_indices {
            let Some(item) = items.get(index) else {
                continue;
            };
            let Some(child) = children.get(index) else {
                continue;
            };
            let (cross_start, cross_end) =
                item_outer_cross_bounds(item, &child.style, physical_direction);
            line.cross_start = line.cross_start.min(cross_start);
            line.cross_end = line.cross_end.max(cross_end);
        }
    }
}

/// Returns the physical available size to use while estimating a flex item's
/// descendants for flex base sizing.
///
/// CSS Flexbox treats a stretched flex item's cross size as definite for
/// laying out descendants when computing the flex base size, provided the flex
/// container has a definite cross size:
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
pub(in crate::layout::flex) fn flex_item_estimate_available_space(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> FlexItemAvailableSpace {
    let mut item_available = FlexItemAvailableSpace::from_container(available);
    let Some(stretched_cross_size) =
        stretched_flex_item_cross_size(child_style, container_style, physical_direction, available)
    else {
        return item_available;
    };

    if physical_direction.is_row_axis() {
        item_available.height = Some(stretched_cross_size);
        item_available.height_is_definite = true;
        item_available.stretched_height = Some(stretched_cross_size);
    } else {
        item_available.width = stretched_cross_size;
        item_available.width_is_definite = true;
        item_available.stretched_width = Some(stretched_cross_size);
    }
    item_available
}

pub(in crate::layout::flex) fn stretched_flex_item_cross_size(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    if !matches!(
        effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    ) || flex_item_has_auto_cross_margin(child_style, physical_direction)
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

/// Resolves the automatic minimum main size of a flex item.
///
/// CSS Flexbox computes automatic minimum sizes from the content-based minimum
/// size for non-scrollable overflow. A preferred aspect ratio can transfer a
/// definite cross size into that minimum; non-replaced items use the larger of
/// the content and transferred suggestions, while replaced items use the smaller:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion>.
pub(in crate::layout::flex) fn automatic_minimum_main_size(
    child: &StyledChild<'_>,
    estimate: &FlexItemEstimate,
    direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    let child_style = &child.style;
    let preferred_aspect_ratio = child_style
        .aspect_ratio
        .preferred_ratio(child.is_replaced_element(), estimate.preferred_aspect_ratio);
    let (specified_min, estimated_min, preferred_size, overflow) = if direction.is_row_axis() {
        (
            child_style.box_values.min_width,
            estimate.min_width,
            used_content_width_or_auto_with_optional_basis(
                child_style,
                Some(available.width),
                child_style.padding.left
                    + child_style.padding.right
                    + horizontal_border_width(child_style),
            ),
            flex_item_main_axis_overflow(child_style, direction),
        )
    } else {
        (
            child_style.box_values.min_height,
            estimate.min_height,
            used_content_height_or_auto_with_optional_basis(
                child_style,
                available.height.filter(|_| available.height_is_definite),
                child_style.padding.top
                    + child_style.padding.bottom
                    + vertical_border_width(child_style),
            ),
            flex_item_main_axis_overflow(child_style, direction),
        )
    };
    if !specified_min.is_auto() || overflow.is_scrollable() {
        return None;
    }
    let mut minimum = estimated_min.points().max(0.0);
    let transferred = if direction.is_row_axis() {
        aspect_ratio_transferred_content_main_size(
            child_style,
            FlexDirection::Row,
            available.height.filter(|_| available.height_is_definite),
            None,
            preferred_aspect_ratio,
        )
    } else {
        aspect_ratio_transferred_content_main_size(
            child_style,
            FlexDirection::Column,
            Some(available.width),
            None,
            preferred_aspect_ratio,
        )
    };
    if let Some(transferred) = transferred {
        let transferred = transferred.points().max(0.0);
        minimum = if child.is_replaced_element() {
            minimum.min(transferred)
        } else {
            minimum.max(transferred)
        };
    }
    if let Some(preferred_size) = preferred_size {
        minimum = minimum.min(preferred_size.max(0.0));
    }
    Some(minimum)
}

/// Maps CSS `justify-content` to Taffy's flex alignment keywords.
///
/// CSS Box Alignment distinguishes logical `start`/`end`, flex-relative
/// `flex-start`/`flex-end`, and physical `left`/`right` keywords. Taffy's
/// flexbox algorithm supports logical and flex-relative keywords directly
/// when `Style.direction` is set. Physical `left`/`right` keywords affect a
/// horizontal main axis; on a vertical main axis they fall back to the
/// physical block-start side, and otherwise they must be converted through the
/// current flex direction before layout:
/// <https://www.w3.org/TR/css-align-3/#typedef-content-position> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>.
pub(in crate::layout::flex) fn taffy_safety(
    safety: AlignmentSafety,
) -> taffy_layout::AlignmentSafety {
    match safety {
        AlignmentSafety::Default => taffy_layout::AlignmentSafety::Unsafe,
        AlignmentSafety::Unsafe => taffy_layout::AlignmentSafety::Unsafe,
        AlignmentSafety::Safe => taffy_layout::AlignmentSafety::Safe,
    }
}

pub(in crate::layout::flex) fn taffy_content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> taffy_layout::AlignContent {
    let safety = taffy_safety(safety);
    match keyword {
        ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::Stretch,
                safety,
            }
        }
        ContentAlignmentKeyword::Start => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::Start,
            safety,
        },
        ContentAlignmentKeyword::End => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::End,
            safety,
        },
        ContentAlignmentKeyword::FlexStart | ContentAlignmentKeyword::Left => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::FlexStart,
                safety,
            }
        }
        ContentAlignmentKeyword::FlexEnd | ContentAlignmentKeyword::Right => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                safety,
            }
        }
        ContentAlignmentKeyword::Center => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::Center,
            safety,
        },
        ContentAlignmentKeyword::SpaceBetween => taffy_layout::AlignContent::SPACE_BETWEEN,
        ContentAlignmentKeyword::SpaceAround => taffy_layout::AlignContent::SPACE_AROUND,
        ContentAlignmentKeyword::SpaceEvenly => taffy_layout::AlignContent::SPACE_EVENLY,
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            taffy_layout::AlignContent::FLEX_START
        }
    }
}

/// Maps CSS `align-content` to Taffy's flex line-packing value.
///
/// CSS Align allows `normal`, baseline positions, overflow-safe positions, and
/// distribution keywords. In flex layout, `normal` behaves as `stretch`; Taffy
/// does not model content baseline packing, so baseline values currently use
/// the spec fallback start-side packing at this boundary:
/// <https://www.w3.org/TR/css-align-3/#align-content-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
pub(in crate::layout::flex) fn taffy_align_content(
    align_content: AlignContent,
) -> taffy_layout::AlignContent {
    taffy_content_alignment(align_content.keyword, align_content.safety)
}

/// Maps CSS `align-items` to Taffy's flex cross-axis item alignment.
///
/// CSS Align defines `normal` as layout-mode dependent; for flex items it
/// behaves as `stretch`. `align-items:self-start`/`self-end` is represented
/// for each affected item through an explicit `align-self` override, because
/// those values depend on the alignment subject's own writing mode:
/// <https://www.w3.org/TR/css-align-3/#align-items-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-items-property>.
pub(in crate::layout::flex) fn taffy_align_items(
    align_items: AlignItems,
) -> taffy_layout::AlignItems {
    taffy_self_alignment(align_items, false)
}

/// Maps CSS `align-self` to Taffy's flex item alignment override.
///
/// `auto` computes to itself and defers to the parent `align-items`; all other
/// values share the `align-items` mapping:
/// <https://www.w3.org/TR/css-align-3/#align-self-property>.
pub(in crate::layout::flex) fn taffy_effective_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> Option<taffy_layout::AlignSelf> {
    let alignment = effective_align_self(child_style, container_style);
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto
        && !matches!(
            container_style.align_items.keyword,
            SelfAlignmentKeyword::SelfStart | SelfAlignmentKeyword::SelfEnd
        )
    {
        return None;
    }
    Some(taffy_cross_self_alignment(alignment))
}

pub(in crate::layout::flex) fn effective_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> AlignSelf {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.align_items
    } else {
        child_style.align_self
    }
}

pub(in crate::layout::flex) fn taffy_self_alignment(
    alignment: AlignItems,
    for_align_self: bool,
) -> taffy_layout::AlignItems {
    let safety = taffy_safety(alignment.safety);
    match alignment.keyword {
        SelfAlignmentKeyword::Auto if for_align_self => taffy_layout::AlignItems::STRETCH,
        SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Stretch => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Stretch,
            safety,
        },
        SelfAlignmentKeyword::Start | SelfAlignmentKeyword::SelfStart => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Start,
            safety,
        },
        SelfAlignmentKeyword::End | SelfAlignmentKeyword::SelfEnd => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::End,
            safety,
        },
        SelfAlignmentKeyword::FlexStart | SelfAlignmentKeyword::Left => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::FlexStart,
            safety,
        },
        SelfAlignmentKeyword::FlexEnd | SelfAlignmentKeyword::Right => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::FlexEnd,
            safety,
        },
        SelfAlignmentKeyword::Center => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Center,
            safety,
        },
        SelfAlignmentKeyword::Baseline | SelfAlignmentKeyword::LastBaseline => {
            taffy_layout::AlignItems::BASELINE
        }
    }
}

/// Maps CSS self-alignment to Taffy's flex item alignment override.
///
/// CSS Box Alignment defines `self-start` and `self-end` from the alignment
/// subject's writing mode, which Taffy's flex alignment model does not carry.
/// Those values are given a start-side placeholder for sizing and line
/// construction; Reasyprint corrects their final cross-axis offsets after
/// Taffy returns item geometry:
/// <https://www.w3.org/TR/css-align-3/#self-position> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-items-property>.
pub(in crate::layout::flex) fn taffy_cross_self_alignment(
    alignment: AlignSelf,
) -> taffy_layout::AlignSelf {
    match alignment.keyword {
        SelfAlignmentKeyword::SelfStart | SelfAlignmentKeyword::SelfEnd => {
            taffy_layout::AlignSelf {
                keyword: taffy_layout::AlignItemsKeyword::FlexStart,
                safety: taffy_safety(alignment.safety),
            }
        }
        _ => taffy_self_alignment(alignment, true),
    }
}

pub(in crate::layout::flex) fn flex_cross_start_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_start_side(style.writing_mode)
    } else {
        inline_start_side(style.writing_mode, style.direction)
    }
}

pub(in crate::layout::flex) fn flex_cross_end_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_end_side(style.writing_mode)
    } else {
        inline_end_side(style.writing_mode, style.direction)
    }
}

pub(in crate::layout::flex) fn child_self_start_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_start = flex_cross_start_side(container_style);
    let cross_axis = cross_start.axis();
    let block_start = block_start_side(child_style.writing_mode);
    if block_start.axis() == cross_axis {
        block_start
    } else {
        inline_start_side(child_style.writing_mode, child_style.direction)
    }
}

pub(in crate::layout::flex) fn child_self_end_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_start = flex_cross_start_side(container_style);
    let cross_axis = cross_start.axis();
    let block_end = block_end_side(child_style.writing_mode);
    if block_end.axis() == cross_axis {
        block_end
    } else {
        inline_end_side(child_style.writing_mode, child_style.direction)
    }
}
