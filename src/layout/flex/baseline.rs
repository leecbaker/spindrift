use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexBaselineSet {
    First,
    Last,
}

impl FlexBaselineSet {
    pub(in crate::layout::flex) fn opposite(self) -> Self {
        match self {
            Self::First => Self::Last,
            Self::Last => Self::First,
        }
    }
}
/// Return whether a flex item can join the container's baseline-sharing group.
///
/// CSS Flexbox only collects baseline-aligned flex items whose inline axis is
/// parallel to the flex container's main axis. Items with an orthogonal inline
/// axis fall back through CSS Align's first/last-baseline self-alignment
/// fallback instead:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line> and
/// <https://drafts.csswg.org/css-align-3/#baseline-align-self>.
pub(in crate::layout::flex) fn flex_item_baseline_axis_is_parallel_to_main_axis(
    child_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    let item_inline_axis =
        inline_start_side(child_style.writing_mode, child_style.used_direction()).axis();
    item_inline_axis
        == if physical_direction.is_row_axis() {
            PhysicalAxis::Horizontal
        } else {
            PhysicalAxis::Vertical
        }
}
pub(in crate::layout::flex) fn flex_baseline_set(
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

pub(in crate::layout::flex) fn vertical_typographic_mode_uses_central_baseline(
    child_style: &ComputedStyle,
    synthesis_writing_mode: WritingMode,
) -> bool {
    matches!(
        synthesis_writing_mode.text_layout_policy(child_style.text_orientation),
        css::TextLayoutPolicy::Vertical(
            css::TextOrientation::Mixed | css::TextOrientation::Upright
        )
    )
}

/// Return the physical axis that flex baseline lines are parallel to.
///
/// CSS Flexbox derives row flex baselines from item baseline sets parallel to
/// the flex container's main axis, and CSS Writing Modes maps that CSS axis
/// into physical page coordinates. Keeping this as CSS-axis metadata prevents
/// baseline synthesis from depending on Taffy's row/column adapter encoding:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(in crate::layout::flex) fn flex_baseline_line_axis(
    container_style: &ComputedStyle,
) -> PhysicalAxis {
    match (
        container_style.flex_direction.is_row_axis(),
        container_style.writing_mode,
    ) {
        (true, WritingMode::HorizontalTb)
        | (
            false,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr,
        ) => PhysicalAxis::Horizontal,
        (
            true,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr,
        )
        | (false, WritingMode::HorizontalTb) => PhysicalAxis::Vertical,
    }
}

pub(in crate::layout::flex) fn synthesis_writing_mode(
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
        (
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr,
            _,
        ) => WritingMode::HorizontalTb,
        (WritingMode::HorizontalTb, Direction::Ltr) => WritingMode::VerticalLr,
        (WritingMode::HorizontalTb, Direction::Rtl) => WritingMode::VerticalRl,
    }
}

pub(in crate::layout::flex) fn line_under_side(writing_mode: WritingMode) -> PhysicalSide {
    css::line_under_side(writing_mode)
}

pub(in crate::layout::flex) fn line_over_side(writing_mode: WritingMode) -> PhysicalSide {
    css::line_over_side(writing_mode)
}
