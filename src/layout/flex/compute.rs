use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlexCollapseMode {
    IncludeCollapsed,
    OmitCollapsed,
}

impl FlexCollapseMode {
    fn omits_collapsed(self) -> bool {
        matches!(self, Self::OmitCollapsed)
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn compute_flex_layout(
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

    fn compute_flex_layout_internal(
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
                                physical_direction,
                                FlexDirection::Row,
                                Some(available.width),
                            ),
                            height: flex_item_size_dimension(
                                child_style.box_values.height,
                                estimated_size.height,
                                estimated_size.min_height,
                                estimated_size.content_height,
                                physical_direction,
                                FlexDirection::Column,
                                available.height.filter(|_| available.height_is_definite),
                            ),
                        },
                        min_size: taffy_layout::Size {
                            width: flex_min_size_dimension(
                                child_style.box_values.min_width,
                                estimated_size.min_width,
                                estimated_size.content_width,
                                used_content_width_or_auto_with_optional_basis(
                                    child_style,
                                    Some(available.width),
                                    child_style.padding.left
                                        + child_style.padding.right
                                        + horizontal_border_width(child_style),
                                ),
                                physical_direction.is_row_axis(),
                                flex_item_main_axis_overflow(child_style, physical_direction),
                                Some(available.width),
                            ),
                            height: flex_min_size_dimension(
                                child_style.box_values.min_height,
                                estimated_size.min_height,
                                estimated_size.content_height,
                                used_content_height_or_auto_with_optional_basis(
                                    child_style,
                                    available.height.filter(|_| available.height_is_definite),
                                    child_style.padding.top
                                        + child_style.padding.bottom
                                        + vertical_border_width(child_style),
                                ),
                                physical_direction.is_column_axis(),
                                flex_item_main_axis_overflow(child_style, physical_direction),
                                available.height.filter(|_| available.height_is_definite),
                            ),
                        },
                        max_size: taffy_layout::Size {
                            width: taffy_intrinsic_dimension_with_basis(
                                child_style.box_values.max_width,
                                Some(available.width),
                                estimated_size.min_width,
                                estimated_size.content_width,
                            ),
                            height: taffy_intrinsic_dimension_with_basis(
                                child_style.box_values.max_height,
                                available.height.filter(|_| available.height_is_definite),
                                estimated_size.min_height,
                                estimated_size.content_height,
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
                            physical_direction,
                            if physical_direction.is_row_axis() {
                                available.width
                            } else {
                                available.height.unwrap_or(estimated_size.content_height)
                            },
                            physical_direction.is_row_axis() || available.height_is_definite,
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
        replace_synthesized_baseline_offsets(
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
        let height = if available.height.is_some() && !available.height_is_definite {
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
        )
        .unwrap_or(height);

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
/// non-scrollable. Taffy remains the primary flex algorithm here, but this
/// guard preserves replaced-element transferred size suggestions when a
/// definite zero-sized flex container would otherwise shrink the final item
/// layout below its automatic minimum:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion>.
fn apply_main_axis_automatic_minimums(
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
            automatic_minimum_main_size(&child.style, estimate, physical_direction, available)
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

/// Returns the physical available size to use while estimating a flex item's
/// descendants for flex base sizing.
///
/// CSS Flexbox treats a stretched flex item's cross size as definite for
/// laying out descendants when computing the flex base size, provided the flex
/// container has a definite cross size:
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
fn flex_item_estimate_available_space(
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

fn stretched_flex_item_cross_size(
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
/// size for non-scrollable overflow, capped by a definite preferred main size.
/// Replaced elements contribute transferred sizes through their intrinsic
/// aspect ratio during the estimate step:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
fn automatic_minimum_main_size(
    child_style: &ComputedStyle,
    estimate: &FlexItemEstimate,
    direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
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
    let mut minimum = estimated_min.max(0.0);
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
fn taffy_safety(safety: AlignmentSafety) -> taffy_layout::AlignmentSafety {
    match safety {
        AlignmentSafety::Default => taffy_layout::AlignmentSafety::Unsafe,
        AlignmentSafety::Unsafe => taffy_layout::AlignmentSafety::Unsafe,
        AlignmentSafety::Safe => taffy_layout::AlignmentSafety::Safe,
    }
}

fn taffy_content_alignment(
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
fn taffy_align_content(align_content: AlignContent) -> taffy_layout::AlignContent {
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
fn taffy_align_items(align_items: AlignItems) -> taffy_layout::AlignItems {
    taffy_self_alignment(align_items, false)
}

/// Maps CSS `align-self` to Taffy's flex item alignment override.
///
/// `auto` computes to itself and defers to the parent `align-items`; all other
/// values share the `align-items` mapping:
/// <https://www.w3.org/TR/css-align-3/#align-self-property>.
fn taffy_effective_align_self(
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

fn effective_align_self(child_style: &ComputedStyle, container_style: &ComputedStyle) -> AlignSelf {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.align_items
    } else {
        child_style.align_self
    }
}

fn taffy_self_alignment(alignment: AlignItems, for_align_self: bool) -> taffy_layout::AlignItems {
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
fn taffy_cross_self_alignment(alignment: AlignSelf) -> taffy_layout::AlignSelf {
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

fn flex_cross_start_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_start_side(style.writing_mode)
    } else {
        inline_start_side(style.writing_mode, style.direction)
    }
}

fn flex_cross_end_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_end_side(style.writing_mode)
    } else {
        inline_end_side(style.writing_mode, style.direction)
    }
}

fn child_self_start_side(
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

fn child_self_end_side(
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

/// Corrects Taffy's placeholder placement for `self-start` and `self-end`.
///
/// CSS Box Alignment defines these keywords from the alignment subject's own
/// writing mode, while flex cross-axis placement aligns the subject within the
/// current flex line. Taffy exposes only the container-axis keyword, so this
/// pass keeps Taffy responsible for sizing and line construction and adjusts
/// only the final cross-axis offset for values that need subject-axis mapping:
/// <https://www.w3.org/TR/css-align-3/#self-position> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>.
fn apply_subject_axis_self_alignment_offsets(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    if !children.iter().any(|child| {
        matches!(
            effective_align_self(&child.style, container_style).keyword,
            SelfAlignmentKeyword::SelfStart | SelfAlignmentKeyword::SelfEnd
        )
    }) {
        return;
    }

    for line in lines {
        for &index in &line.item_indices {
            let child_style = &children[index].style;
            let alignment = effective_align_self(child_style, container_style);
            let subject_side = match alignment.keyword {
                SelfAlignmentKeyword::SelfStart => {
                    child_self_start_side(child_style, container_style)
                }
                SelfAlignmentKeyword::SelfEnd => child_self_end_side(child_style, container_style),
                _ => continue,
            };
            if flex_item_has_auto_cross_margin(child_style, physical_direction) {
                continue;
            }
            let outer_size = item_outer_cross_size(&items[index], child_style, physical_direction);
            let target_side = if alignment.safety == AlignmentSafety::Safe
                && line.cross_size() - outer_size < 0.0
            {
                flex_cross_start_side(container_style)
            } else {
                subject_side
            };
            align_item_cross_side(
                &mut items[index],
                child_style,
                physical_direction,
                line,
                target_side,
            );
        }
    }
}

fn flex_lines_from_items(
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    estimates: &[FlexItemEstimate],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) -> Vec<FlexLineLayout> {
    if container_style.flex_wrap == FlexWrap::NoWrap {
        let (main_start, main_end) =
            flex_items_main_extent(items, children, physical_direction).unwrap_or((0.0, 0.0));
        let item_indices = (0..items.len()).collect::<Vec<_>>();
        return vec![FlexLineLayout {
            item_indices: item_indices.clone(),
            source_start: 0,
            source_end: items.len(),
            main_start,
            main_end,
            cross_start: 0.0,
            cross_end: container_cross_size.max(0.0),
            first_baseline: flex_line_baseline(
                &item_indices,
                items,
                estimates,
                children,
                container_style,
                FlexBaselineSet::First,
                physical_direction,
            ),
            last_baseline: flex_line_baseline(
                &item_indices,
                items,
                estimates,
                children,
                container_style,
                FlexBaselineSet::Last,
                physical_direction,
            ),
            collapsed_struts: Vec::new(),
        }];
    }

    let mut lines: Vec<FlexLineLayout> = Vec::new();
    for index in 0..items.len() {
        let (cross_start, cross_end) =
            item_outer_cross_bounds(&items[index], &children[index].style, physical_direction);
        let (main_start, main_end) =
            item_outer_main_bounds(&items[index], &children[index].style, physical_direction);
        if let Some(line) = lines
            .iter_mut()
            .find(|line| cross_start < line.cross_end - 0.01 && cross_end > line.cross_start + 0.01)
        {
            line.cross_start = line.cross_start.min(cross_start);
            line.cross_end = line.cross_end.max(cross_end);
            line.main_start = line.main_start.min(main_start);
            line.main_end = line.main_end.max(main_end);
            line.source_start = line.source_start.min(index);
            line.source_end = line.source_end.max(index + 1);
            line.item_indices.push(index);
        } else {
            lines.push(FlexLineLayout {
                item_indices: vec![index],
                source_start: index,
                source_end: index + 1,
                main_start,
                main_end,
                cross_start,
                cross_end,
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            });
        }
    }
    refresh_flex_line_metadata(
        &mut lines,
        items,
        estimates,
        children,
        container_style,
        physical_direction,
        container_cross_size,
    );
    lines
}

fn refresh_flex_line_cross_bounds(
    lines: &mut [FlexLineLayout],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) {
    let stretch_wrapped_lines = !lines.is_empty()
        && container_style.flex_wrap != FlexWrap::NoWrap
        && matches!(
            container_style.align_content.keyword,
            ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch
        );
    for line in &mut *lines {
        if container_style.flex_wrap == FlexWrap::NoWrap {
            line.cross_start = 0.0;
            line.cross_end = container_cross_size.max(0.0);
            if let Some(main_extent) = flex_items_main_extent(items, children, physical_direction) {
                line.main_start = main_extent.0;
                line.main_end = main_extent.1;
            }
            line.cross_end = line
                .cross_end
                .max(line.cross_start + line.largest_collapsed_strut());
            continue;
        }
        if line.item_indices.is_empty() {
            line.cross_end = line
                .cross_end
                .max(line.cross_start + line.largest_collapsed_strut());
            continue;
        }
        let mut cross_start = f32::INFINITY;
        let mut cross_end = f32::NEG_INFINITY;
        let mut main_start = f32::INFINITY;
        let mut main_end = f32::NEG_INFINITY;
        for &index in &line.item_indices {
            let (item_cross_start, item_cross_end) =
                item_outer_cross_bounds(&items[index], &children[index].style, physical_direction);
            let (item_main_start, item_main_end) =
                item_outer_main_bounds(&items[index], &children[index].style, physical_direction);
            cross_start = cross_start.min(item_cross_start);
            cross_end = cross_end.max(item_cross_end);
            main_start = main_start.min(item_main_start);
            main_end = main_end.max(item_main_end);
        }
        line.cross_start = cross_start;
        line.cross_end = cross_end.max(cross_start + line.largest_collapsed_strut());
        line.main_start = main_start;
        line.main_end = main_end;
    }
    if stretch_wrapped_lines {
        preserve_stretched_flex_line_cross_bounds(lines, container_cross_size);
    }
}

/// Preserve stretched wrapped flex line boxes after item-bound refresh.
///
/// Taffy owns flex line construction and initial packing, but Quire refreshes
/// line metadata from item bounds for baseline and fragmentation passes. CSS
/// Flexbox stretches flex lines, not just their items, so post-layout
/// alignment corrections need the full line cross-size:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-line-stretch> and
/// <https://www.w3.org/TR/css-align-3/#align-content-property>.
fn preserve_stretched_flex_line_cross_bounds(
    lines: &mut [FlexLineLayout],
    container_cross_size: f32,
) {
    let mut line_order = (0..lines.len()).collect::<Vec<_>>();
    line_order.sort_by(|&a, &b| {
        lines[a]
            .cross_start
            .partial_cmp(&lines[b].cross_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if line_order.len() == 1 {
        let line = &mut lines[line_order[0]];
        line.cross_start = 0.0;
        line.cross_end = container_cross_size
            .max(0.0)
            .max(line.cross_end)
            .max(line.cross_start + line.largest_collapsed_strut());
        return;
    }

    let container_cross_size = container_cross_size.max(0.0);
    for position in 0..line_order.len() {
        let line_index = line_order[position];
        if position == 0 && lines[line_index].cross_start > 0.0 {
            lines[line_index].cross_start = 0.0;
        }
        let next_cross_start = line_order
            .get(position + 1)
            .map(|&next_index| lines[next_index].cross_start)
            .unwrap_or(container_cross_size);
        lines[line_index].cross_end = lines[line_index]
            .cross_end
            .max(next_cross_start)
            .max(lines[line_index].cross_start + lines[line_index].largest_collapsed_strut());
    }
}

fn refresh_flex_line_metadata(
    lines: &mut [FlexLineLayout],
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) {
    refresh_flex_line_cross_bounds(
        lines,
        items,
        children,
        container_style,
        physical_direction,
        container_cross_size,
    );
    for line in lines {
        line.first_baseline = flex_line_baseline(
            &line.item_indices,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::First,
            physical_direction,
        );
        line.last_baseline = flex_line_baseline(
            &line.item_indices,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::Last,
            physical_direction,
        );
    }
}

fn item_outer_cross_bounds(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> (f32, f32) {
    item.outer_cross_bounds(FlexAxes::from_physical_direction(physical_direction), style)
}

fn item_outer_cross_size(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> f32 {
    let (cross_start, cross_end) = item_outer_cross_bounds(item, style, physical_direction);
    (cross_end - cross_start).max(0.0)
}

fn estimated_outer_cross_size(
    style: &ComputedStyle,
    estimate: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> f32 {
    let borders = used_border_widths(style);
    if physical_direction.is_row_axis() {
        estimate.height
            + style.padding.top
            + style.padding.bottom
            + borders.top
            + borders.bottom
            + style.margin.top
            + style.margin.bottom
    } else {
        estimate.width
            + style.padding.left
            + style.padding.right
            + borders.left
            + borders.right
            + style.margin.left
            + style.margin.right
    }
    .max(0.0)
}

fn collapsed_struts_from_visible_layout(
    children: &[StyledChild<'_>],
    style: &ComputedStyle,
    visible_layout: &FlexLayout,
) -> Vec<FlexCollapsedStrut> {
    let physical_direction = physical_flex_direction(style);
    children
        .iter()
        .enumerate()
        .filter(|(_, child)| flex_item_is_collapsed(&child.style))
        .map(|(item_index, child)| {
            let item = &visible_layout.items[item_index];
            let line = visible_layout
                .lines
                .iter()
                .find(|line| line.item_indices.contains(&item_index));
            FlexCollapsedStrut {
                item_index,
                cross_size: item_outer_cross_size(item, &child.style, physical_direction),
                source_start: line.map(|line| line.source_start).unwrap_or(item_index),
                source_end: line.map(|line| line.source_end).unwrap_or(item_index + 1),
            }
        })
        .collect()
}

fn attach_collapsed_struts_to_active_lines(
    lines: &mut Vec<FlexLineLayout>,
    source_indices: &[usize],
    collapsed_struts: &[FlexCollapsedStrut],
) {
    if collapsed_struts.is_empty() {
        return;
    }

    for line in lines.iter_mut() {
        let mut source_start = usize::MAX;
        let mut source_end = 0usize;
        for &active_index in &line.item_indices {
            let Some(&source_index) = source_indices.get(active_index) else {
                continue;
            };
            source_start = source_start.min(source_index);
            source_end = source_end.max(source_index + 1);
        }
        if source_start != usize::MAX {
            line.source_start = source_start;
            line.source_end = source_end;
        }
    }

    for strut in collapsed_struts {
        if lines.is_empty() {
            lines.push(FlexLineLayout {
                item_indices: Vec::new(),
                source_start: strut.item_index,
                source_end: strut.item_index + 1,
                main_start: 0.0,
                main_end: 0.0,
                cross_start: 0.0,
                cross_end: strut.cross_size,
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: vec![strut.clone()],
            });
            continue;
        }

        let target_index = lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| collapsed_strut_line_overlap(strut, line))
            .filter(|(_, line)| collapsed_strut_line_overlap(strut, line) > 0)
            .map(|(index, _)| index)
            .or_else(|| {
                lines
                    .iter()
                    .position(|line| strut.item_index < line.source_end)
            })
            .unwrap_or(lines.len() - 1);
        let line = &mut lines[target_index];
        line.source_start = line.source_start.min(strut.source_start);
        line.source_end = line.source_end.max(strut.source_end);
        line.collapsed_struts.push(strut.clone());
        line.cross_end = line
            .cross_end
            .max(line.cross_start + line.largest_collapsed_strut());
    }
}

fn repack_lines_after_collapsed_struts(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
) {
    if lines.len() <= 1 || !lines.iter().any(|line| !line.collapsed_struts.is_empty()) {
        return;
    }

    let axes = FlexAxes::from_physical_direction(physical_direction);
    let mut line_order = (0..lines.len()).collect::<Vec<_>>();
    line_order.sort_by(|&a, &b| {
        lines[a]
            .cross_start
            .partial_cmp(&lines[b].cross_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut next_cross_start = lines[line_order[0]].cross_start;
    for line_index in line_order {
        let delta = next_cross_start - lines[line_index].cross_start;
        if delta.abs() > 0.01 {
            lines[line_index].cross_start += delta;
            lines[line_index].cross_end += delta;
            for &item_index in &lines[line_index].item_indices {
                items[item_index].translate_cross(axes, delta);
            }
        }
        next_cross_start = lines[line_index].cross_end;
    }
}

/// Repack flex lines after Quire-side main-size corrections.
///
/// Taffy performs flexible length resolution and `justify-content` packing
/// before Quire applies the final automatic minimum-size guard for edge cases
/// Taffy cannot represent. When that guard changes a main size, CSS Flexbox's
/// main-axis alignment must be recomputed from the corrected outer sizes:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-align> and
/// <https://www.w3.org/TR/css-align-3/#distribution-values>.
fn repack_lines_after_main_size_adjustment(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_main_size: f32,
) {
    if !container_main_size.is_finite() {
        return;
    }

    let (physical_gap_width, physical_gap_height) = physical_flex_gaps(container_style);
    let main_gap = used_flex_gap(
        if physical_direction.is_row_axis() {
            physical_gap_width
        } else {
            physical_gap_height
        },
        container_main_size,
    )
    .max(0.0);

    for line in lines {
        if line.item_indices.is_empty() {
            continue;
        }

        let mut physical_order = line.item_indices.clone();
        physical_order.sort_by(|&left, &right| {
            let left_start =
                item_outer_main_bounds(&items[left], &children[left].style, physical_direction).0;
            let right_start =
                item_outer_main_bounds(&items[right], &children[right].style, physical_direction).0;
            left_start.total_cmp(&right_start)
        });

        let item_count = physical_order.len();
        let fixed_outer_size = physical_order
            .iter()
            .map(|&index| {
                item_main_size(&items[index], physical_direction)
                    + fixed_main_before_margin(&children[index].style, physical_direction)
                    + fixed_main_after_margin(&children[index].style, physical_direction)
            })
            .sum::<f32>();
        let total_gap = main_gap * item_count.saturating_sub(1) as f32;
        let free_space = container_main_size - fixed_outer_size - total_gap;
        let auto_margin_count = physical_order
            .iter()
            .map(|&index| main_auto_margin_count(&children[index].style, physical_direction))
            .sum::<usize>();
        let auto_margin = if free_space > 0.0 && auto_margin_count > 0 {
            free_space / auto_margin_count as f32
        } else {
            0.0
        };
        let (initial_offset, extra_gap) = if auto_margin_count > 0 && free_space > 0.0 {
            (0.0, 0.0)
        } else {
            justify_content_offsets(
                container_style.justify_content,
                physical_direction,
                free_space,
                item_count,
            )
        };

        let mut cursor = initial_offset;
        for (position, &item_index) in physical_order.iter().enumerate() {
            cursor +=
                main_before_margin(&children[item_index].style, physical_direction, auto_margin);
            set_item_main_start(&mut items[item_index], physical_direction, cursor);
            cursor += item_main_size(&items[item_index], physical_direction);
            cursor +=
                main_after_margin(&children[item_index].style, physical_direction, auto_margin);
            if position + 1 < item_count {
                cursor += main_gap + extra_gap;
            }
        }

        if let Some((main_start, main_end)) =
            flex_line_items_main_extent(line, items, children, physical_direction)
        {
            line.main_start = main_start;
            line.main_end = main_end;
        }
    }
}

fn item_main_size(item: &FlexItemLayout, physical_direction: FlexDirection) -> f32 {
    item.main_size(FlexAxes::from_physical_direction(physical_direction))
        .max(0.0)
}

fn set_item_main_start(
    item: &mut FlexItemLayout,
    physical_direction: FlexDirection,
    main_start: f32,
) {
    item.set_main_start(
        FlexAxes::from_physical_direction(physical_direction),
        main_start,
    );
}

fn fixed_main_before_margin(style: &ComputedStyle, physical_direction: FlexDirection) -> f32 {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            0.0
        } else {
            style.margin.left
        }
    } else if style.box_values.margin.top.is_auto() {
        0.0
    } else {
        style.margin.top
    }
}

fn fixed_main_after_margin(style: &ComputedStyle, physical_direction: FlexDirection) -> f32 {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            0.0
        } else {
            style.margin.right
        }
    } else if style.box_values.margin.bottom.is_auto() {
        0.0
    } else {
        style.margin.bottom
    }
}

fn main_before_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: f32,
) -> f32 {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            auto_margin
        } else {
            style.margin.left
        }
    } else if style.box_values.margin.top.is_auto() {
        auto_margin
    } else {
        style.margin.top
    }
}

fn main_after_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: f32,
) -> f32 {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            auto_margin
        } else {
            style.margin.right
        }
    } else if style.box_values.margin.bottom.is_auto() {
        auto_margin
    } else {
        style.margin.bottom
    }
}

fn main_auto_margin_count(style: &ComputedStyle, physical_direction: FlexDirection) -> usize {
    if physical_direction.is_row_axis() {
        style.box_values.margin.left.is_auto() as usize
            + style.box_values.margin.right.is_auto() as usize
    } else {
        style.box_values.margin.top.is_auto() as usize
            + style.box_values.margin.bottom.is_auto() as usize
    }
}

fn justify_content_offsets(
    justify_content: JustifyContent,
    physical_direction: FlexDirection,
    free_space: f32,
    item_count: usize,
) -> (f32, f32) {
    let keyword = justify_content_fallback_keyword(justify_content, free_space, item_count);
    let reversed = matches!(
        physical_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let first = match keyword {
        ContentAlignmentKeyword::Normal
        | ContentAlignmentKeyword::Stretch
        | ContentAlignmentKeyword::Start => 0.0,
        ContentAlignmentKeyword::FlexStart => {
            if reversed {
                free_space
            } else {
                0.0
            }
        }
        ContentAlignmentKeyword::End => free_space,
        ContentAlignmentKeyword::FlexEnd => {
            if reversed {
                0.0
            } else {
                free_space
            }
        }
        ContentAlignmentKeyword::Left => 0.0,
        ContentAlignmentKeyword::Right => free_space,
        ContentAlignmentKeyword::Center => free_space / 2.0,
        ContentAlignmentKeyword::SpaceBetween => 0.0,
        ContentAlignmentKeyword::SpaceAround => {
            if free_space >= 0.0 {
                (free_space / item_count as f32) / 2.0
            } else {
                free_space / 2.0
            }
        }
        ContentAlignmentKeyword::SpaceEvenly => {
            if free_space >= 0.0 {
                free_space / (item_count + 1) as f32
            } else {
                free_space / 2.0
            }
        }
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => 0.0,
    };
    let positive_free_space = free_space.max(0.0);
    let between = match keyword {
        ContentAlignmentKeyword::SpaceBetween if item_count > 1 => {
            positive_free_space / (item_count - 1) as f32
        }
        ContentAlignmentKeyword::SpaceAround if item_count > 0 => {
            positive_free_space / item_count as f32
        }
        ContentAlignmentKeyword::SpaceEvenly => positive_free_space / (item_count + 1) as f32,
        _ => 0.0,
    };
    (first, between)
}

fn justify_content_fallback_keyword(
    justify_content: JustifyContent,
    free_space: f32,
    item_count: usize,
) -> ContentAlignmentKeyword {
    let mut keyword = justify_content.keyword;
    let mut safe = justify_content.safety == AlignmentSafety::Safe;
    if item_count <= 1 || free_space <= 0.0 {
        (keyword, safe) = match keyword {
            ContentAlignmentKeyword::Stretch | ContentAlignmentKeyword::SpaceBetween => {
                (ContentAlignmentKeyword::FlexStart, true)
            }
            ContentAlignmentKeyword::SpaceAround | ContentAlignmentKeyword::SpaceEvenly => {
                (ContentAlignmentKeyword::Center, true)
            }
            other => (other, safe),
        };
    }
    if free_space <= 0.0 && safe {
        ContentAlignmentKeyword::Start
    } else {
        keyword
    }
}

/// Apply CSS Box Alignment baseline content-alignment to wrapped row flex lines.
///
/// Taffy 0.11 maps `align-content: baseline` to start packing. CSS Align
/// instead treats flex lines as the alignment subjects and aligns their
/// compatible baseline sets when those sets are available:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content>.
fn apply_baseline_align_content_offsets(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) {
    if lines.is_empty() || container_style.flex_wrap == FlexWrap::NoWrap {
        return;
    }

    let baseline_set = match container_style.align_content.keyword {
        ContentAlignmentKeyword::Baseline => FlexBaselineSet::First,
        ContentAlignmentKeyword::LastBaseline => FlexBaselineSet::Last,
        _ => return,
    };

    let line_baselines = lines
        .iter()
        .map(|line| {
            flex_line_content_baseline(
                line,
                items,
                estimates,
                children,
                container_style,
                baseline_set,
                physical_direction,
            )
        })
        .collect::<Vec<_>>();
    if line_baselines
        .iter()
        .filter(|baseline| baseline.is_some())
        .count()
        <= 1
    {
        apply_baseline_align_content_fallback_offset(
            items,
            lines,
            container_style,
            physical_direction,
            container_cross_size,
            baseline_set,
        );
        return;
    }

    if !container_style.flex_direction.is_row_axis() {
        apply_baseline_align_content_fallback_offset(
            items,
            lines,
            container_style,
            physical_direction,
            container_cross_size,
            baseline_set,
        );
        return;
    }

    let target_baseline = line_baselines
        .iter()
        .flatten()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    if !target_baseline.is_finite() {
        return;
    }

    for (line_index, baseline) in line_baselines.into_iter().enumerate() {
        let Some(baseline) = baseline else {
            continue;
        };
        let delta = target_baseline - baseline;
        if delta.abs() <= 0.01 {
            continue;
        }
        shift_flex_line_cross_axis(&mut lines[line_index], items, physical_direction, delta);
    }
}

/// Applies content-alignment fallback when line baselines cannot be shared.
///
/// CSS Align defines first-baseline content alignment fallback as safe logical
/// start, and last-baseline content alignment fallback as safe logical end.
/// The fallback moves the flex-line group in the flex container cross axis:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
fn apply_baseline_align_content_fallback_offset(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
    baseline_set: FlexBaselineSet,
) {
    let Some((group_start, group_end)) = flex_line_group_cross_bounds(lines) else {
        return;
    };
    let group_size = (group_end - group_start).max(0.0);
    let target_side = if group_size > container_cross_size.max(0.0) {
        flex_cross_start_side(container_style)
    } else {
        match baseline_set {
            FlexBaselineSet::First => flex_cross_start_side(container_style),
            FlexBaselineSet::Last => flex_cross_end_side(container_style),
        }
    };
    align_flex_line_group_cross_side(
        lines,
        items,
        physical_direction,
        target_side,
        container_cross_size,
    );
}

fn flex_line_group_cross_bounds(lines: &[FlexLineLayout]) -> Option<(f32, f32)> {
    lines
        .iter()
        .map(|line| (line.cross_start, line.cross_end))
        .fold(None, |bounds, line_bounds| {
            Some(match bounds {
                Some((start, end)) => (start.min(line_bounds.0), end.max(line_bounds.1)),
                None => line_bounds,
            })
        })
}

/// Applies `align-content: stretch` fallback when stretched flex lines overflow.
///
/// CSS Align defines `stretch` as falling back to `flex-start`, not
/// `safe flex-start`, so an overflowing wrapped line remains packed against
/// the flex cross-start side. Taffy 0.11 applies the older generic
/// distribution fallback, so Quire corrects only the overflow case after
/// recovering flex line metadata:
/// <https://drafts.csswg.org/css-align/#valdef-align-content-stretch> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
fn apply_stretch_align_content_overflow_fallback_offsets(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) {
    if container_style.flex_wrap == FlexWrap::NoWrap
        || !matches!(
            container_style.align_content.keyword,
            ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch
        )
    {
        return;
    }

    let Some((group_start, group_end)) =
        flex_line_alignment_subject_cross_bounds(lines, items, children, physical_direction)
    else {
        return;
    };
    let group_size = (group_end - group_start).max(0.0);
    if group_size <= container_cross_size.max(0.0) + 0.01 {
        return;
    }

    align_flex_line_group_cross_side_from_bounds(
        lines,
        items,
        physical_direction,
        flex_line_packing_flex_start_side(container_style),
        container_cross_size,
        group_start,
        group_end,
    );
}

fn flex_line_packing_flex_start_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_wrap == FlexWrap::WrapReverse {
        flex_cross_end_side(style)
    } else {
        flex_cross_start_side(style)
    }
}

fn flex_line_alignment_subject_cross_bounds(
    lines: &[FlexLineLayout],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> Option<(f32, f32)> {
    lines
        .iter()
        .filter_map(|line| {
            if line.item_indices.is_empty() {
                return Some((line.cross_start, line.cross_end));
            }
            line.item_indices
                .iter()
                .map(|&index| {
                    item_outer_cross_bounds(
                        &items[index],
                        &children[index].style,
                        physical_direction,
                    )
                })
                .fold(None::<(f32, f32)>, |bounds, item_bounds| {
                    Some(match bounds {
                        Some((start, end)) => (start.min(item_bounds.0), end.max(item_bounds.1)),
                        None => item_bounds,
                    })
                })
                .map(|(start, end)| {
                    (
                        start.min(line.cross_start),
                        end.max(line.cross_start + line.largest_collapsed_strut()),
                    )
                })
        })
        .fold(None, |bounds, line_bounds| {
            Some(match bounds {
                Some((start, end)) => (start.min(line_bounds.0), end.max(line_bounds.1)),
                None => line_bounds,
            })
        })
}

fn align_flex_line_group_cross_side(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
    side: PhysicalSide,
    container_cross_size: f32,
) {
    let Some((group_start, group_end)) = flex_line_group_cross_bounds(lines) else {
        return;
    };
    align_flex_line_group_cross_side_from_bounds(
        lines,
        items,
        physical_direction,
        side,
        container_cross_size,
        group_start,
        group_end,
    );
}

fn align_flex_line_group_cross_side_from_bounds(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
    side: PhysicalSide,
    container_cross_size: f32,
    group_start: f32,
    group_end: f32,
) {
    let delta = if side.is_start_edge() {
        -group_start
    } else if side.is_end_edge() {
        container_cross_size.max(0.0) - group_end
    } else {
        0.0
    };
    if delta.abs() <= 0.01 {
        return;
    }
    for line in lines {
        shift_flex_line_cross_axis(line, items, physical_direction, delta);
    }
}

/// Applies baseline self-alignment fallback when a sharing group is unavailable.
///
/// CSS Box Alignment falls back from first-baseline self-alignment to
/// `safe self-start` and from last-baseline self-alignment to `safe self-end`
/// when no compatible baseline-sharing group can be formed. Row flex lines can
/// share compatible baselines, so this fallback only handles single-participant
/// groups there. Column flex self-alignment needs a fallback for every
/// baseline participant until Quire exposes the vertical baseline sets needed
/// for cross-axis sharing. Flexbox then aligns that fallback in the flex
/// line's cross axis:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-self> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>.
fn apply_baseline_self_alignment_fallback_offsets(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    for line in lines {
        for baseline_set in [FlexBaselineSet::First, FlexBaselineSet::Last] {
            let line_indices = line
                .item_indices
                .iter()
                .copied()
                .filter(|&candidate| {
                    flex_baseline_set(&children[candidate].style, container_style)
                        == Some(baseline_set)
                })
                .collect::<Vec<_>>();
            if line_indices.is_empty() {
                continue;
            }
            if container_style.flex_direction.is_row_axis() && line_indices.len() != 1 {
                continue;
            }

            for index in line_indices {
                let child_style = &children[index].style;
                if flex_item_has_auto_cross_margin(child_style, physical_direction) {
                    continue;
                }

                let subject_side = match baseline_set {
                    FlexBaselineSet::First => child_self_start_side(child_style, container_style),
                    FlexBaselineSet::Last => child_self_end_side(child_style, container_style),
                };
                let outer_size =
                    item_outer_cross_size(&items[index], child_style, physical_direction);
                let target_side = if line.cross_size() - outer_size < 0.0 {
                    flex_cross_start_side(container_style)
                } else {
                    subject_side
                };
                align_item_cross_side(
                    &mut items[index],
                    child_style,
                    physical_direction,
                    line,
                    target_side,
                );
            }
        }
    }
}

fn shift_flex_line_cross_axis(
    line: &mut FlexLineLayout,
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
    delta: f32,
) {
    let axes = FlexAxes::from_physical_direction(physical_direction);
    line.cross_start += delta;
    line.cross_end += delta;
    for &item_index in &line.item_indices {
        items[item_index].translate_cross(axes, delta);
    }
}

fn collapsed_strut_line_overlap(strut: &FlexCollapsedStrut, line: &FlexLineLayout) -> usize {
    let start = strut.source_start.max(line.source_start);
    let end = strut.source_end.min(line.source_end);
    end.saturating_sub(start)
}

fn item_outer_main_bounds(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> (f32, f32) {
    item.outer_main_bounds(FlexAxes::from_physical_direction(physical_direction), style)
}

fn flex_items_main_extent(
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> Option<(f32, f32)> {
    items
        .iter()
        .zip(children)
        .map(|(item, child)| item_outer_main_bounds(item, &child.style, physical_direction))
        .fold(None, |bounds, item_bounds| {
            Some(match bounds {
                Some((start, end)) => (start.min(item_bounds.0), end.max(item_bounds.1)),
                None => item_bounds,
            })
        })
}

fn flex_line_items_main_extent(
    line: &FlexLineLayout,
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> Option<(f32, f32)> {
    line.item_indices
        .iter()
        .copied()
        .map(|index| {
            item_outer_main_bounds(&items[index], &children[index].style, physical_direction)
        })
        .fold(None, |bounds, item_bounds| {
            Some(match bounds {
                Some((start, end)) => (start.min(item_bounds.0), end.max(item_bounds.1)),
                None => item_bounds,
            })
        })
}

fn flex_line_baseline(
    line_indices: &[usize],
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> Option<f32> {
    if !container_style.flex_direction.is_row_axis() {
        return None;
    }

    line_indices
        .iter()
        .copied()
        .filter(|&index| {
            flex_baseline_set(&children[index].style, container_style) == Some(baseline_set)
        })
        .map(|index| {
            measured_item_cross_axis_baseline(
                &items[index],
                &estimates[index],
                &children[index].style,
                container_style,
                baseline_set,
                physical_direction,
            )
        })
        .reduce(f32::max)
}

fn flex_line_content_baseline(
    line: &FlexLineLayout,
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> Option<f32> {
    if !container_style.flex_direction.is_row_axis() {
        return None;
    }

    let line_baseline = match baseline_set {
        FlexBaselineSet::First => line.first_baseline,
        FlexBaselineSet::Last => line.last_baseline,
    };
    if line_baseline.is_some() {
        return line_baseline;
    }

    flex_line_baseline_item_index(line, container_style, baseline_set).map(|index| {
        measured_item_cross_axis_baseline(
            &items[index],
            &estimates[index],
            &children[index].style,
            container_style,
            baseline_set,
            physical_direction,
        )
    })
}

fn flex_line_baseline_item_index(
    line: &FlexLineLayout,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> Option<usize> {
    match (baseline_set, container_style.flex_direction) {
        (FlexBaselineSet::First, FlexDirection::Row | FlexDirection::RowReverse) => {
            line.item_indices.iter().copied().min()
        }
        (FlexBaselineSet::First, FlexDirection::Column) => line.item_indices.iter().copied().min(),
        (FlexBaselineSet::First, FlexDirection::ColumnReverse) => {
            line.item_indices.iter().copied().max()
        }
        (FlexBaselineSet::Last, FlexDirection::Row | FlexDirection::RowReverse) => {
            line.item_indices.iter().copied().max()
        }
        (FlexBaselineSet::Last, FlexDirection::Column) => line.item_indices.iter().copied().max(),
        (FlexBaselineSet::Last, FlexDirection::ColumnReverse) => {
            line.item_indices.iter().copied().min()
        }
    }
}

fn flex_item_has_auto_cross_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    if physical_direction.is_row_axis() {
        style.box_values.margin.top.is_auto() || style.box_values.margin.bottom.is_auto()
    } else {
        style.box_values.margin.left.is_auto() || style.box_values.margin.right.is_auto()
    }
}

fn align_item_cross_side(
    item: &mut FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    line: &FlexLineLayout,
    side: PhysicalSide,
) {
    debug_assert_eq!(
        side.axis(),
        if physical_direction.is_row_axis() {
            PhysicalAxis::Vertical
        } else {
            PhysicalAxis::Horizontal
        }
    );
    match (
        physical_direction.is_row_axis(),
        side.is_start_edge(),
        side.is_end_edge(),
    ) {
        (true, true, false) => item.set_y(line.cross_start + style.margin.top),
        (true, false, true) => item.set_y(line.cross_end - style.margin.bottom - item.height()),
        (false, true, false) => item.set_x(line.cross_start + style.margin.left),
        (false, false, true) => item.set_x(line.cross_end - style.margin.right - item.width()),
        _ => {}
    }
}

fn taffy_justify_content(
    justify_content: JustifyContent,
    flex_direction: FlexDirection,
) -> Option<taffy_layout::JustifyContent> {
    let safety = taffy_safety(justify_content.safety);
    match justify_content.keyword {
        ContentAlignmentKeyword::Normal
        | ContentAlignmentKeyword::FlexStart
        | ContentAlignmentKeyword::Stretch => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::FlexStart,
            safety,
        }),
        ContentAlignmentKeyword::FlexEnd => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::FlexEnd,
            safety,
        }),
        ContentAlignmentKeyword::Start => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::Start,
            safety,
        }),
        ContentAlignmentKeyword::End => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::End,
            safety,
        }),
        ContentAlignmentKeyword::Left => Some(
            if matches!(
                flex_direction,
                FlexDirection::RowReverse | FlexDirection::ColumnReverse
            ) {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                    safety,
                }
            } else {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexStart,
                    safety,
                }
            },
        ),
        ContentAlignmentKeyword::Right => {
            Some(if matches!(flex_direction, FlexDirection::Column) {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexStart,
                    safety,
                }
            } else if matches!(flex_direction, FlexDirection::ColumnReverse) {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                    safety,
                }
            } else if matches!(flex_direction, FlexDirection::RowReverse) {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexStart,
                    safety,
                }
            } else {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                    safety,
                }
            })
        }
        ContentAlignmentKeyword::Center => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::Center,
            safety,
        }),
        ContentAlignmentKeyword::SpaceBetween => Some(taffy_layout::JustifyContent::SPACE_BETWEEN),
        ContentAlignmentKeyword::SpaceAround => Some(taffy_layout::JustifyContent::SPACE_AROUND),
        ContentAlignmentKeyword::SpaceEvenly => Some(taffy_layout::JustifyContent::SPACE_EVENLY),
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            Some(taffy_layout::JustifyContent::FLEX_START)
        }
    }
}

/// Maps a CSS flex container's physical axes to Taffy's writing direction.
///
/// Taffy has physical row/column flex directions plus an LTR/RTL switch. For
/// horizontal writing modes that switch is CSS `direction`. For vertical
/// writing modes whose CSS row axis becomes Taffy's physical column axis, the
/// switch must instead represent the horizontal cross-axis start side, so
/// `vertical-rl` row flex lines start at the physical right edge. Vertical
/// writing modes whose CSS column axis becomes Taffy's physical row axis have
/// their right-to-left or left-to-right block flow already encoded in
/// `physical_flex_direction`, so they use LTR here.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>,
/// <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property>, and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
fn taffy_flex_layout_direction(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> ::taffy::Direction {
    if style.writing_mode == WritingMode::HorizontalTb {
        return taffy_direction(style.direction);
    }
    if physical_direction.is_column_axis() && flex_cross_start_side(style) == PhysicalSide::Right {
        ::taffy::Direction::Rtl
    } else {
        ::taffy::Direction::Ltr
    }
}

/// Maps CSS `direction` to Taffy's physical LTR/RTL switch.
fn taffy_direction(direction: Direction) -> ::taffy::Direction {
    match direction {
        Direction::Ltr => ::taffy::Direction::Ltr,
        Direction::Rtl => ::taffy::Direction::Rtl,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlexBaselineSet {
    First,
    Last,
}

/// Replace Taffy's synthesized leaf baselines with renderer text baselines.
///
/// Taffy 0.11's public measure callback returns only leaf sizes, so baseline
/// aligned flex leaves are initially positioned with the CSS fallback baseline
/// synthesized from the item border box. CSS Flexbox aligns participating row
/// flex items by their first or last baseline set; after layout we recover the
/// line cross-start from Taffy's synthesized shift and reapply the measured
/// text baseline from the same intrinsic estimate used for flex sizing:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
fn replace_synthesized_baseline_offsets(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    if !container_style.flex_direction.is_row_axis() {
        return;
    }

    for line in lines {
        for baseline_set in [FlexBaselineSet::First, FlexBaselineSet::Last] {
            let line_indices = line
                .item_indices
                .iter()
                .copied()
                .filter(|&candidate| {
                    flex_baseline_set(&children[candidate].style, container_style)
                        == Some(baseline_set)
                })
                .collect::<Vec<_>>();
            if line_indices.len() <= 1 {
                continue;
            }

            let line_start = line_indices
                .iter()
                .map(|&candidate| {
                    if physical_direction.is_row_axis() {
                        items[candidate].y() - children[candidate].style.margin.top
                    } else {
                        items[candidate].x() - children[candidate].style.margin.left
                    }
                })
                .fold(f32::INFINITY, f32::min)
                .min(0.0);
            let max_margin_before = line_indices
                .iter()
                .map(|&candidate| {
                    if physical_direction.is_row_axis() {
                        children[candidate].style.margin.top
                    } else {
                        children[candidate].style.margin.left
                    }
                })
                .fold(0.0f32, f32::max);
            let max_baseline = line_indices
                .iter()
                .map(|&candidate| {
                    measured_item_cross_axis_border_box_baseline(
                        &items[candidate],
                        &estimates[candidate],
                        &children[candidate].style,
                        container_style,
                        baseline_set,
                        physical_direction,
                    )
                })
                .fold(0.0f32, f32::max);
            for candidate in line_indices {
                let baseline = measured_item_cross_axis_border_box_baseline(
                    &items[candidate],
                    &estimates[candidate],
                    &children[candidate].style,
                    container_style,
                    baseline_set,
                    physical_direction,
                );
                let position = line_start + max_margin_before + (max_baseline - baseline).max(0.0);
                if physical_direction.is_row_axis() {
                    items[candidate].set_y(position);
                } else {
                    items[candidate].set_x(position);
                }
            }
        }
    }
}

fn flex_baseline_set(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> Option<FlexBaselineSet> {
    match child_style.align_self.keyword {
        SelfAlignmentKeyword::Baseline => Some(FlexBaselineSet::First),
        SelfAlignmentKeyword::LastBaseline => Some(FlexBaselineSet::Last),
        SelfAlignmentKeyword::Auto => match container_style.align_items.keyword {
            SelfAlignmentKeyword::Baseline => Some(FlexBaselineSet::First),
            SelfAlignmentKeyword::LastBaseline => Some(FlexBaselineSet::Last),
            _ => None,
        },
        _ => None,
    }
}

fn synthesized_item_baseline(item: &FlexItemLayout, style: &ComputedStyle) -> f32 {
    item.height() + style.margin.top
}

fn measured_item_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> f32 {
    let measured = match baseline_set {
        FlexBaselineSet::First => estimate.first_baseline,
        FlexBaselineSet::Last => estimate.last_baseline,
    };
    measured.unwrap_or_else(|| {
        synthesized_item_border_box_baseline(
            item,
            child_style,
            container_style,
            flex_baseline_line_axis(container_style),
        )
    })
}

fn measured_item_horizontal_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> f32 {
    let measured = match baseline_set {
        FlexBaselineSet::First => estimate.first_horizontal_baseline,
        FlexBaselineSet::Last => estimate.last_horizontal_baseline,
    };
    measured.unwrap_or_else(|| {
        synthesized_item_border_box_baseline(
            item,
            child_style,
            container_style,
            flex_baseline_line_axis(container_style),
        )
    })
}

fn measured_item_cross_axis_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> f32 {
    if physical_direction.is_row_axis() {
        measured_item_border_box_baseline(
            item,
            estimate,
            child_style,
            container_style,
            baseline_set,
        )
    } else {
        measured_item_horizontal_border_box_baseline(
            item,
            estimate,
            child_style,
            container_style,
            baseline_set,
        )
    }
}

/// Synthesizes a missing flex-item baseline from its border box.
///
/// CSS Align synthesizes an alphabetic baseline from the line-under edge of
/// the rectangle, and CSS Flexbox says flex items synthesize from border
/// edges. When the item's block flow is parallel to the baseline-sharing
/// context axis, CSS Align requires an axis-compatible writing mode before
/// choosing line-under/line-over:
/// <https://drafts.csswg.org/css-align-3/#synthesize-baseline> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
fn synthesized_item_border_box_baseline(
    item: &FlexItemLayout,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_line_axis: PhysicalAxis,
) -> f32 {
    match line_under_side(synthesis_writing_mode(
        child_style,
        container_style,
        baseline_line_axis,
    )) {
        PhysicalSide::Top => 0.0,
        PhysicalSide::Right => item.width(),
        PhysicalSide::Bottom => item.height(),
        PhysicalSide::Left => 0.0,
    }
}

/// Return the physical axis that flex baseline lines are parallel to.
///
/// CSS Flexbox derives row flex baselines from item baseline sets parallel to
/// the flex container's main axis, and CSS Writing Modes maps that CSS axis
/// into physical page coordinates. Keeping this as CSS-axis metadata prevents
/// baseline synthesis from depending on Taffy's row/column adapter encoding:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
fn flex_baseline_line_axis(container_style: &ComputedStyle) -> PhysicalAxis {
    match (
        container_style.flex_direction.is_row_axis(),
        container_style.writing_mode,
    ) {
        (true, WritingMode::HorizontalTb)
        | (false, WritingMode::VerticalRl | WritingMode::VerticalLr) => PhysicalAxis::Horizontal,
        (true, WritingMode::VerticalRl | WritingMode::VerticalLr)
        | (false, WritingMode::HorizontalTb) => PhysicalAxis::Vertical,
    }
}

fn synthesis_writing_mode(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_line_axis: PhysicalAxis,
) -> WritingMode {
    if block_start_side(child_style.writing_mode).axis() != baseline_line_axis {
        return child_style.writing_mode;
    }
    if block_start_side(container_style.writing_mode).axis() != baseline_line_axis {
        return container_style.writing_mode;
    }
    match (child_style.writing_mode, child_style.direction) {
        (WritingMode::VerticalRl | WritingMode::VerticalLr, _) => WritingMode::HorizontalTb,
        (WritingMode::HorizontalTb, Direction::Ltr) => WritingMode::VerticalLr,
        (WritingMode::HorizontalTb, Direction::Rtl) => WritingMode::VerticalRl,
    }
}

fn line_under_side(writing_mode: WritingMode) -> PhysicalSide {
    match writing_mode {
        WritingMode::HorizontalTb => PhysicalSide::Bottom,
        WritingMode::VerticalRl | WritingMode::VerticalLr => PhysicalSide::Left,
    }
}

/// Return a flex item's absolute baseline coordinate in the flex line cross
/// axis.
///
/// CSS Flexbox aligns row flex-line baseline sets in the row cross axis. For
/// horizontal writing modes that coordinate is physical y; for vertical
/// writing modes the row cross axis is physical x, so Quire uses the
/// vertical-text horizontal baseline estimates recorded from inline painting:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
fn measured_item_cross_axis_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> f32 {
    if physical_direction.is_row_axis() {
        return item.y()
            + style.margin.top
            + measured_item_border_box_baseline(
                item,
                estimate,
                style,
                container_style,
                baseline_set,
            );
    }
    item.x()
        + style.margin.left
        + measured_item_horizontal_border_box_baseline(
            item,
            estimate,
            style,
            container_style,
            baseline_set,
        )
}

/// Return a horizontal flex container's first exported baseline offset.
///
/// CSS Flexbox says a flex container's first main-axis baseline is generated
/// from the first flex item's first baseline set when that item has a baseline
/// parallel to the main axis; otherwise it is synthesized from the content box:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
fn flex_container_first_baseline(
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
) -> Option<f32> {
    if !container_style.flex_direction.is_row_axis() {
        return None;
    }

    let (item, estimate, child) = items
        .iter()
        .zip(estimates)
        .zip(children)
        .map(|((item, estimate), child)| (item, estimate, child))
        .next()?;

    Some(
        item.y()
            + estimate
                .first_baseline
                .unwrap_or_else(|| synthesized_item_baseline(item, &child.style))
            + child.style.margin.top,
    )
}
