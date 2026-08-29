use super::*;

/// Repack flex lines after Quire-side main-size corrections.
///
/// Taffy performs flexible length resolution and `justify-content` packing
/// before Quire applies the final automatic minimum-size guard for edge cases
/// Taffy cannot represent. When that guard changes a main size, CSS Flexbox's
/// main-axis alignment must be recomputed from the corrected outer sizes:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-align> and
/// <https://www.w3.org/TR/css-align-3/#distribution-values>.
pub(in crate::layout::flex) fn repack_lines_after_main_size_adjustment(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_main_size: FlexMainSize,
    container_main_percentage_basis: FlexAvailablePercentageBasis,
) {
    if !container_main_size.is_finite() {
        return;
    }

    let PhysicalFlexGaps {
        horizontal: physical_gap_width,
        vertical: physical_gap_height,
    } = physical_flex_gaps(container_style);
    let main_gap = flex_main_gap_size(used_flex_gap_with_basis(
        if physical_direction.is_row_axis() {
            physical_gap_width
        } else {
            physical_gap_height
        },
        container_main_percentage_basis,
    ));

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
            left_start
                .partial_cmp(&right_start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let item_count = physical_order.len();
        let fixed_outer_size =
            physical_order
                .iter()
                .fold(FlexMainLength::new(0.0), |total, &index| {
                    total
                        + (item_main_size(&items[index], physical_direction)
                            + fixed_main_before_margin(&children[index].style, physical_direction)
                            + fixed_main_after_margin(&children[index].style, physical_direction))
                });
        let total_gap = main_gap.scale(item_count.saturating_sub(1) as f32);
        let free_space = container_main_size - fixed_outer_size - total_gap;
        let auto_margin_count = physical_order
            .iter()
            .map(|&index| main_auto_margin_count(&children[index].style, physical_direction))
            .sum::<usize>();
        let auto_margin = if free_space.is_positive() && auto_margin_count > 0 {
            free_space.divide(
                std::num::NonZeroUsize::new(auto_margin_count)
                    .expect("positive auto-margin count is non-zero"),
            )
        } else {
            FlexMainLength::new(0.0)
        };
        let justification = if auto_margin_count > 0 && free_space.is_positive() {
            FlexMainJustificationOffsets {
                initial: FlexMainLength::new(0.0),
                between: FlexMainLength::new(0.0),
            }
        } else {
            justify_content_offsets(
                container_style.justify_content,
                physical_direction,
                free_space,
                item_count,
            )
        };
        let mut cursor = FlexMainOffset::new(0.0) + justification.initial;
        for (position, &item_index) in physical_order.iter().enumerate() {
            cursor = cursor
                + main_before_margin(&children[item_index].style, physical_direction, auto_margin);
            set_item_main_start(&mut items[item_index], physical_direction, cursor);
            cursor = cursor + item_main_size(&items[item_index], physical_direction);
            cursor = cursor
                + main_after_margin(&children[item_index].style, physical_direction, auto_margin);
            if position + 1 < item_count {
                cursor = cursor + main_gap + justification.between;
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
pub(in crate::layout::flex) fn item_main_size(
    item: &FlexItemLayout,
    physical_direction: FlexDirection,
) -> FlexMainSize {
    item.main_size(PhysicalFlexDirection::new(physical_direction))
}

pub(in crate::layout::flex) fn set_item_main_start(
    item: &mut FlexItemLayout,
    physical_direction: FlexDirection,
    main_start: FlexMainOffset,
) {
    item.set_main_start(PhysicalFlexDirection::new(physical_direction), main_start);
}

pub(in crate::layout::flex) fn fixed_main_before_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            FlexMainLength::new(0.0)
        } else {
            FlexMainLength::new(style.margin.left)
        }
    } else if style.box_values.margin.top.is_auto() {
        FlexMainLength::new(0.0)
    } else {
        FlexMainLength::new(style.margin.top)
    }
}

pub(in crate::layout::flex) fn fixed_main_after_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            FlexMainLength::new(0.0)
        } else {
            FlexMainLength::new(style.margin.right)
        }
    } else if style.box_values.margin.bottom.is_auto() {
        FlexMainLength::new(0.0)
    } else {
        FlexMainLength::new(style.margin.bottom)
    }
}

pub(in crate::layout::flex) fn main_before_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: FlexMainLength,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            auto_margin
        } else {
            FlexMainLength::new(style.margin.left)
        }
    } else if style.box_values.margin.top.is_auto() {
        auto_margin
    } else {
        FlexMainLength::new(style.margin.top)
    }
}

pub(in crate::layout::flex) fn main_after_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: FlexMainLength,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            auto_margin
        } else {
            FlexMainLength::new(style.margin.right)
        }
    } else if style.box_values.margin.bottom.is_auto() {
        auto_margin
    } else {
        FlexMainLength::new(style.margin.bottom)
    }
}

pub(in crate::layout::flex) fn main_auto_margin_count(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> usize {
    if physical_direction.is_row_axis() {
        style.box_values.margin.left.is_auto() as usize
            + style.box_values.margin.right.is_auto() as usize
    } else {
        style.box_values.margin.top.is_auto() as usize
            + style.box_values.margin.bottom.is_auto() as usize
    }
}

pub(in crate::layout::flex) fn justify_content_offsets(
    justify_content: JustifyContent,
    physical_direction: FlexDirection,
    free_space: FlexMainLength,
    item_count: usize,
) -> FlexMainJustificationOffsets {
    let keyword = justify_content_fallback_keyword(justify_content, free_space, item_count);
    let reversed = matches!(
        physical_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let first = match keyword {
        // In a flex formatting context, `normal` and `stretch` fall back to
        // `flex-start`, not logical `start`. A reverse main axis therefore
        // consumes the free space before the first physical item.
        // <https://www.w3.org/TR/css-align-3/#distribution-flex>
        ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch => {
            if reversed {
                free_space
            } else {
                FlexMainLength::new(0.0)
            }
        }
        ContentAlignmentKeyword::Start => FlexMainLength::new(0.0),
        ContentAlignmentKeyword::FlexStart => {
            if reversed {
                free_space
            } else {
                FlexMainLength::new(0.0)
            }
        }
        ContentAlignmentKeyword::End => free_space,
        ContentAlignmentKeyword::FlexEnd => {
            if reversed {
                FlexMainLength::new(0.0)
            } else {
                free_space
            }
        }
        // Physical `left` and `right` fall back to logical `start` only when
        // the physical main axis is vertical. A vertical-writing column
        // projects to the horizontal physical row axis and keeps distinct
        // left/right packing offsets.
        // <https://drafts.csswg.org/css-align-3/#justify-content-property>
        ContentAlignmentKeyword::Left | ContentAlignmentKeyword::Right
            if physical_direction.is_column_axis() =>
        {
            FlexMainLength::new(0.0)
        }
        ContentAlignmentKeyword::Left => FlexMainLength::new(0.0),
        ContentAlignmentKeyword::Right => free_space,
        ContentAlignmentKeyword::Center => free_space.half(),
        ContentAlignmentKeyword::SpaceBetween => FlexMainLength::new(0.0),
        ContentAlignmentKeyword::SpaceAround => {
            if !free_space.is_negative() {
                free_space
                    .divide(std::num::NonZeroUsize::new(item_count).expect("non-zero item count"))
                    .half()
            } else {
                free_space.half()
            }
        }
        ContentAlignmentKeyword::SpaceEvenly => {
            if !free_space.is_negative() {
                free_space.divide(
                    std::num::NonZeroUsize::new(item_count + 1)
                        .expect("item count plus one is non-zero"),
                )
            } else {
                free_space.half()
            }
        }
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            FlexMainLength::new(0.0)
        }
    };
    let positive_free_space = free_space.max(FlexMainLength::new(0.0));
    let between = match keyword {
        ContentAlignmentKeyword::SpaceBetween if item_count > 1 => positive_free_space.divide(
            std::num::NonZeroUsize::new(item_count - 1)
                .expect("more than one item leaves a non-zero divisor"),
        ),
        ContentAlignmentKeyword::SpaceAround if item_count > 0 => positive_free_space
            .divide(std::num::NonZeroUsize::new(item_count).expect("non-zero item count")),
        ContentAlignmentKeyword::SpaceEvenly => positive_free_space.divide(
            std::num::NonZeroUsize::new(item_count + 1).expect("item count plus one is non-zero"),
        ),
        _ => FlexMainLength::new(0.0),
    };
    FlexMainJustificationOffsets {
        initial: first,
        between,
    }
}

/// Main-axis offsets selected by CSS `justify-content` for one flex line.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexMainJustificationOffsets {
    pub(in crate::layout::flex) initial: FlexMainLength,
    pub(in crate::layout::flex) between: FlexMainLength,
}

pub(in crate::layout::flex) fn justify_content_fallback_keyword(
    justify_content: JustifyContent,
    free_space: FlexMainLength,
    item_count: usize,
) -> ContentAlignmentKeyword {
    let mut keyword = justify_content.keyword;
    let mut safe = justify_content.safety == AlignmentSafety::Safe;
    let mut distribution_falls_back_to_flex_start = false;
    if item_count <= 1 || free_space.is_non_positive() {
        (keyword, safe) = match keyword {
            ContentAlignmentKeyword::Stretch | ContentAlignmentKeyword::SpaceBetween => {
                distribution_falls_back_to_flex_start = true;
                (ContentAlignmentKeyword::FlexStart, true)
            }
            ContentAlignmentKeyword::SpaceAround | ContentAlignmentKeyword::SpaceEvenly => {
                (ContentAlignmentKeyword::Center, true)
            }
            other => (other, safe),
        };
    }
    if free_space.is_non_positive() && safe && !distribution_falls_back_to_flex_start {
        ContentAlignmentKeyword::Start
    } else {
        keyword
    }
}
