use super::*;
use crate::layout::taffy_bridge;

pub(in crate::layout::flex) fn taffy_safety(
    safety: AlignmentSafety,
) -> taffy_layout::AlignmentSafety {
    taffy_bridge::alignment_safety(safety)
}

pub(in crate::layout::flex) fn taffy_content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> taffy_layout::AlignContent {
    taffy_bridge::content_alignment(keyword, safety)
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
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<taffy_layout::AlignSelf> {
    let alignment = effective_align_self(child_style, container_style);
    // A percentage cross size in an indefinite axis behaves as `auto` for
    // sizing, but it is not a *computed* `auto` size. CSS Flexbox therefore
    // does not stretch it; the normal/stretch alignment falls back to the
    // cross-start position instead.
    // <https://drafts.csswg.org/css-flexbox-1/#valdef-align-items-stretch>.
    let cyclic_percentage_cross_size = if physical_direction.is_row_axis() {
        matches!(
            &*child_style.box_values.height,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.contains_percentage()
        ) && !available.height_basis.is_definite()
    } else {
        matches!(
            &child_style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.contains_percentage()
        ) && !available.width_basis.is_definite()
    };
    if cyclic_percentage_cross_size
        && matches!(
            alignment.keyword,
            SelfAlignmentKeyword::Auto
                | SelfAlignmentKeyword::Normal
                | SelfAlignmentKeyword::Stretch
        )
    {
        return Some(taffy_layout::AlignSelf {
            keyword: taffy_layout::AlignItemsKeyword::FlexStart,
            safety: taffy_safety(alignment.safety),
        });
    }
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

pub(in crate::layout::flex) fn taffy_self_alignment(
    alignment: AlignItems,
    for_align_self: bool,
) -> taffy_layout::AlignItems {
    taffy_bridge::item_alignment(
        alignment,
        if for_align_self {
            taffy_bridge::TaffyAutoAlignment::Preserve
        } else {
            taffy_bridge::TaffyAutoAlignment::Stretch
        },
    )
}

/// Maps CSS self-alignment to Taffy's flex item alignment override.
///
/// CSS Box Alignment defines `self-start` and `self-end` from the alignment
/// subject's writing mode, which Taffy's flex alignment model does not carry.
/// Those values are given a start-side placeholder for sizing and line
/// construction; Quire corrects their final cross-axis offsets after
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

pub(in crate::layout::flex) fn taffy_justify_content(
    justify_content: JustifyContent,
    axes: FlexAxes,
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
        // `left` and `right` are physical horizontal alignment keywords. They
        // are positional whenever the flex main axis is physical horizontal,
        // including a column flex container in vertical or sideways writing
        // modes; otherwise they compute to `start`.
        // <https://drafts.csswg.org/css-align-3/#justify-content-property>
        ContentAlignmentKeyword::Left | ContentAlignmentKeyword::Right
            if !axes.is_main_row_axis() =>
        {
            Some(taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::Start,
                safety,
            })
        }
        ContentAlignmentKeyword::Left => Some(if PhysicalSide::Left == axes.main_start_side() {
            taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::FlexStart,
                safety,
            }
        } else {
            debug_assert_eq!(PhysicalSide::Left, axes.main_end_side());
            taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                safety,
            }
        }),
        ContentAlignmentKeyword::Right => Some(if PhysicalSide::Right == axes.main_start_side() {
            taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::FlexStart,
                safety,
            }
        } else {
            debug_assert_eq!(PhysicalSide::Right, axes.main_end_side());
            taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                safety,
            }
        }),
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

/// Reproject Taffy's flex-item cross-axis rectangles when CSS cross-start is
/// the physical bottom edge.
///
/// Taffy's `Direction` can express a horizontal start side, but it has no
/// top-to-bottom/bottom-to-top equivalent. A vertical-writing column flex
/// container therefore needs a coordinate conversion when its inline axis is
/// RTL. Taffy still forms and sizes the lines; this maps its top-origin cross
/// coordinates to CSS's bottom-origin inline axis before Quire constructs line
/// metadata or performs any CSS Align placement.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>
/// <https://www.w3.org/TR/css-writing-modes-4/#inline-flow>
/// Maps CSS `direction` to Taffy's physical LTR/RTL switch.
pub(in crate::layout::flex) fn taffy_direction(direction: Direction) -> ::taffy::Direction {
    taffy_bridge::direction(direction)
}
