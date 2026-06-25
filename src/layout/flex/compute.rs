use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(super) fn compute_flex_layout(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
    ) -> Option<FlexLayout> {
        let mut tree: taffy_layout::TaffyTree<FlexItemEstimate> = taffy_layout::TaffyTree::new();
        // CSS Flexbox used sizes are real-valued CSS lengths. Taffy rounds final
        // layouts by default for screen pixels; PDF emission must preserve the
        // unrounded layout and let rasterizers antialias at their output DPI.
        tree.disable_rounding();
        let physical_direction = physical_flex_direction(style);
        let (physical_gap_width, physical_gap_height) = physical_flex_gaps(style);
        let mut nodes = Vec::with_capacity(children.len());
        let mut estimates = Vec::with_capacity(children.len());
        let mut source_indices = Vec::with_capacity(children.len());
        let mut collapsed_cross_strut = 0.0f32;
        for (source_index, child) in children.iter().enumerate() {
            let child_style = &child.style;
            let estimated_size = self.estimate_flex_item_size(
                child,
                stylesheets,
                available.width,
                available.width_is_definite,
            );
            if flex_item_is_collapsed(child_style) {
                collapsed_cross_strut =
                    collapsed_cross_strut.max(if physical_direction.is_row_axis() {
                        estimated_size.height
                    } else {
                        estimated_size.width
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
                                physical_direction,
                                FlexDirection::Row,
                                Some(available.width),
                            ),
                            height: flex_item_size_dimension(
                                child_style.box_values.height,
                                estimated_size.height,
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
            estimates.push(estimated_size);
            source_indices.push(source_index);
        }

        let root = tree
            .new_with_children(
                taffy_layout::Style {
                    display: taffy_layout::Display::Flex,
                    box_sizing: taffy_layout::BoxSizing::BorderBox,
                    direction: taffy_direction(style.direction),
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
        let mut items = vec![
            FlexItemLayout {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            };
            children.len()
        ];
        let mut active_items = Vec::with_capacity(nodes.len());
        for node in nodes {
            let layout = tree.layout(node).ok()?;
            active_items.push(FlexItemLayout {
                x: layout.location.x,
                y: layout.location.y,
                width: layout.size.width,
                height: layout.size.height,
            });
        }
        let active_children = source_indices
            .iter()
            .map(|&index| children[index].clone())
            .collect::<Vec<_>>();
        replace_synthesized_baseline_offsets(
            &mut active_items,
            &estimates,
            &active_children,
            style,
            physical_direction,
        );
        apply_subject_axis_self_alignment_offsets(
            &mut active_items,
            &active_children,
            style,
            physical_direction,
            if physical_direction.is_row_axis() {
                root_layout.size.height
            } else {
                root_layout.size.width
            },
        );
        for (active_index, source_index) in source_indices.into_iter().enumerate() {
            items[source_index] = active_items[active_index].clone();
        }
        apply_main_axis_automatic_minimums(
            &mut items,
            &estimates,
            children,
            physical_direction,
            available,
        );
        let item_extent_height = items
            .iter()
            .map(|item| item.y + item.height)
            .fold(0.0f32, f32::max);
        let height = if available.height.is_some() && !available.height_is_definite {
            item_extent_height
        } else {
            root_layout.size.height.max(item_extent_height).max(
                if physical_direction.is_row_axis() {
                    collapsed_cross_strut
                } else {
                    0.0
                },
            )
        };

        let first_baseline =
            flex_container_first_baseline(&active_items, &estimates, &active_children, style)
                .unwrap_or(height);

        Some(FlexLayout {
            height,
            first_baseline,
            items,
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
) {
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        let Some(minimum) =
            automatic_minimum_main_size(&child.style, estimate, physical_direction, available)
        else {
            continue;
        };
        if physical_direction.is_row_axis() {
            if item.width >= minimum {
                continue;
            }
            let delta = minimum - item.width;
            if matches!(physical_direction, FlexDirection::RowReverse) {
                item.x -= delta;
            }
            item.width = minimum;
        } else {
            if item.height >= minimum {
                continue;
            }
            let delta = minimum - item.height;
            if matches!(physical_direction, FlexDirection::ColumnReverse) {
                item.y -= delta;
            }
            item.height = minimum;
        }
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

#[derive(Debug, Clone, Copy)]
struct FlexLineCrossBounds {
    low: f32,
    high: f32,
}

impl FlexLineCrossBounds {
    fn size(self) -> f32 {
        (self.high - self.low).max(0.0)
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
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) {
    if !children.iter().any(|child| {
        matches!(
            effective_align_self(&child.style, container_style).keyword,
            SelfAlignmentKeyword::SelfStart | SelfAlignmentKeyword::SelfEnd
        )
    }) {
        return;
    }

    for line in flex_cross_alignment_lines(items, children, container_style, physical_direction) {
        let bounds = flex_line_cross_bounds(
            &line,
            items,
            children,
            container_style,
            physical_direction,
            container_cross_size,
        );
        for index in line {
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
            let target_side =
                if alignment.safety == AlignmentSafety::Safe && bounds.size() - outer_size < 0.0 {
                    flex_cross_start_side(container_style)
                } else {
                    subject_side
                };
            align_item_cross_side(
                &mut items[index],
                child_style,
                physical_direction,
                bounds,
                target_side,
            );
        }
    }
}

fn flex_cross_alignment_lines(
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> Vec<Vec<usize>> {
    if container_style.flex_wrap == FlexWrap::NoWrap {
        return vec![(0..items.len()).collect()];
    }

    let mut lines: Vec<(FlexLineCrossBounds, Vec<usize>)> = Vec::new();
    for index in 0..items.len() {
        let bounds =
            item_outer_cross_bounds(&items[index], &children[index].style, physical_direction);
        if let Some((line_bounds, line_indices)) = lines.iter_mut().find(|(line_bounds, _)| {
            bounds.low <= line_bounds.high + 0.01 && bounds.high + 0.01 >= line_bounds.low
        }) {
            line_bounds.low = line_bounds.low.min(bounds.low);
            line_bounds.high = line_bounds.high.max(bounds.high);
            line_indices.push(index);
        } else {
            lines.push((bounds, vec![index]));
        }
    }
    lines.into_iter().map(|(_, indices)| indices).collect()
}

fn flex_line_cross_bounds(
    line: &[usize],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) -> FlexLineCrossBounds {
    if container_style.flex_wrap == FlexWrap::NoWrap {
        return FlexLineCrossBounds {
            low: 0.0,
            high: container_cross_size.max(0.0),
        };
    }

    line.iter()
        .map(|&index| {
            item_outer_cross_bounds(&items[index], &children[index].style, physical_direction)
        })
        .fold(
            FlexLineCrossBounds {
                low: f32::INFINITY,
                high: f32::NEG_INFINITY,
            },
            |bounds, item| FlexLineCrossBounds {
                low: bounds.low.min(item.low),
                high: bounds.high.max(item.high),
            },
        )
}

fn item_outer_cross_bounds(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexLineCrossBounds {
    if physical_direction.is_row_axis() {
        FlexLineCrossBounds {
            low: item.y - style.margin.top,
            high: item.y + item.height + style.margin.bottom,
        }
    } else {
        FlexLineCrossBounds {
            low: item.x - style.margin.left,
            high: item.x + item.width + style.margin.right,
        }
    }
}

fn item_outer_cross_size(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> f32 {
    item_outer_cross_bounds(item, style, physical_direction).size()
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
    bounds: FlexLineCrossBounds,
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
        (true, true, false) => item.y = bounds.low + style.margin.top,
        (true, false, true) => item.y = bounds.high - style.margin.bottom - item.height,
        (false, true, false) => item.x = bounds.low + style.margin.left,
        (false, false, true) => item.x = bounds.high - style.margin.right - item.width,
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

/// Maps CSS `direction` to Taffy's writing direction for flex layout.
///
/// CSS Flexbox resolves `row` and `row-reverse` against the inline base
/// direction, and CSS Box Alignment resolves logical `start`/`end` against the
/// same writing context:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property> and
/// <https://www.w3.org/TR/css-align-3/#positional-values>.
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
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    if !physical_direction.is_row_axis() {
        return;
    }

    let mut visited = vec![false; items.len()];
    for index in 0..items.len() {
        let Some(baseline_set) = flex_baseline_set(&children[index].style, container_style) else {
            continue;
        };
        if visited[index] {
            continue;
        }

        let line_indices =
            baseline_line_indices(index, items, children, visited.as_slice(), container_style);
        if line_indices.len() <= 1 {
            for candidate in line_indices {
                visited[candidate] = true;
            }
            continue;
        }

        let line_start = line_indices
            .iter()
            .map(|&candidate| items[candidate].y - children[candidate].style.margin.top)
            .fold(f32::INFINITY, f32::min)
            .min(0.0);
        let max_margin_before = line_indices
            .iter()
            .map(|&candidate| children[candidate].style.margin.top)
            .fold(0.0f32, f32::max);
        let max_baseline = line_indices
            .iter()
            .map(|&candidate| {
                measured_item_border_box_baseline(
                    &items[candidate],
                    &estimates[candidate],
                    baseline_set,
                )
            })
            .fold(0.0f32, f32::max);
        for candidate in line_indices {
            visited[candidate] = true;
            let baseline = measured_item_border_box_baseline(
                &items[candidate],
                &estimates[candidate],
                baseline_set,
            );
            items[candidate].y =
                line_start + max_margin_before + (max_baseline - baseline).max(0.0);
        }
    }
}

/// Return baseline-sharing flex items for one row flex line.
///
/// CSS Flexbox aligns baseline-participating flex items within each flex line.
/// For `flex-wrap: nowrap`, every in-flow row-axis item is in the same line;
/// for wrapped rows, Taffy does not expose line objects, so we keep using the
/// synthesized line-baseline coordinate as a best-effort grouping key:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-lines> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
fn baseline_line_indices(
    index: usize,
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    visited: &[bool],
    container_style: &ComputedStyle,
) -> Vec<usize> {
    let Some(baseline_set) = flex_baseline_set(&children[index].style, container_style) else {
        return Vec::new();
    };
    let line_baseline = synthesized_baseline_coordinate(&items[index], &children[index].style);
    let mut line_indices = Vec::new();
    for candidate in index..items.len() {
        if visited[candidate]
            || flex_baseline_set(&children[candidate].style, container_style) != Some(baseline_set)
        {
            continue;
        }
        if container_style.flex_wrap == FlexWrap::NoWrap {
            line_indices.push(candidate);
            continue;
        }
        let candidate_line_baseline =
            synthesized_baseline_coordinate(&items[candidate], &children[candidate].style);
        if (candidate_line_baseline - line_baseline).abs() < 0.01 {
            line_indices.push(candidate);
        }
    }
    line_indices
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

fn synthesized_baseline_coordinate(item: &FlexItemLayout, style: &ComputedStyle) -> f32 {
    item.y + synthesized_item_baseline(item, style)
}

fn synthesized_item_baseline(item: &FlexItemLayout, style: &ComputedStyle) -> f32 {
    item.height + style.margin.top
}

fn measured_item_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    baseline_set: FlexBaselineSet,
) -> f32 {
    let measured = match baseline_set {
        FlexBaselineSet::First => estimate.first_baseline,
        FlexBaselineSet::Last => estimate.last_baseline,
    };
    measured.unwrap_or(item.height)
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
        item.y
            + estimate
                .first_baseline
                .unwrap_or_else(|| synthesized_item_baseline(item, &child.style))
            + child.style.margin.top,
    )
}
