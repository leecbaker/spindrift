use super::*;

/// Estimates a replaced image flex item without letting main-size constraints alter flex basis.
///
/// CSS Flexbox computes the flex base size from the item's used flex-basis
/// while ignoring min/max main-size constraints, but the hypothetical size and
/// cross-size contribution still reflect replaced-element aspect-ratio sizing.
/// For replaced elements with an intrinsic ratio, cross-axis min/max constraints
/// transfer through the ratio into the content-basis candidate used by
/// `flex-basis:auto`:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>,
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>, and
/// <https://www.w3.org/TR/css-sizing-3/#aspect-ratio>.
pub(in crate::layout::flex) fn estimate_replaced_image_flex_item(
    element: &Element,
    style: &ComputedStyle,
    containing_width: f32,
    available: FlexItemAvailableSpace,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Option<FlexItemEstimate> {
    let intrinsic = intrinsic_image_size(element, base_url, root_url, resource_cache)?;
    let natural_aspect_ratio = intrinsic.width / intrinsic.height;
    let aspect_ratio = style
        .aspect_ratio
        .preferred_ratio(true, Some(natural_aspect_ratio))?;
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
            .or(intrinsic.attr_width)
            .or_else(|| {
                available
                    .stretched_width
                    .map(|width| (width - horizontal_non_content).max(0.0))
            });
    let specified_height =
        definite_image_content_height_without_percent(style, vertical_non_content)
            .or(intrinsic.attr_height)
            .or_else(|| {
                available
                    .stretched_height
                    .map(|height| (height - vertical_non_content).max(0.0))
            });
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

    let mut width_constrained_width = base_width;
    let mut height_from_width_constraints = base_height;
    constrain_replaced_size_with_aspect_ratio(
        &mut width_constrained_width,
        &mut height_from_width_constraints,
        aspect_ratio,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        ReplacedSizeConstraints {
            min_width: used_min_width(style, containing_width),
            max_width: used_max_width(style, containing_width),
            min_height: None,
            max_height: None,
        },
    );

    let mut width_from_height_constraints = base_width;
    let mut height_constrained_height = base_height;
    constrain_replaced_size_with_aspect_ratio(
        &mut width_from_height_constraints,
        &mut height_constrained_height,
        aspect_ratio,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        ReplacedSizeConstraints {
            min_width: None,
            max_width: None,
            min_height: used_min_height(style, containing_width),
            max_height: used_max_height(style, containing_width),
        },
    );

    Some(FlexItemEstimate {
        width: content_box_pt(width.max(1.0)),
        height: content_box_pt(height.max(1.0)),
        min_width: content_box_pt(width.max(1.0)),
        min_height: content_box_pt(height.max(1.0)),
        content_width: content_box_pt(width_from_height_constraints.max(1.0)),
        content_height: content_box_pt(height_from_width_constraints.max(1.0)),
        preferred_aspect_ratio: Some(aspect_ratio),
        first_baseline: None,
        last_baseline: None,
        first_horizontal_baseline: None,
        last_horizontal_baseline: None,
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
pub(in crate::layout::flex) fn first_text_baseline_offset(
    font_system: &mut FontSystem,
    style: &ComputedStyle,
) -> f32 {
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
pub(in crate::layout::flex) fn last_text_baseline_offset(
    font_system: &mut FontSystem,
    style: &ComputedStyle,
    line_count: usize,
) -> f32 {
    first_text_baseline_offset(font_system, style)
        + line_count.saturating_sub(1) as f32 * style.line_height
}

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
    border_box_width: f32,
    line_baseline_offset: f32,
) -> Option<f32> {
    horizontal_text_baseline_offset(style, border_box_width, 0.0, line_baseline_offset)
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
    border_box_width: f32,
    preceding_line_height: f32,
    line_baseline_offset: f32,
) -> Option<f32> {
    horizontal_text_baseline_offset(
        style,
        border_box_width,
        preceding_line_height,
        line_baseline_offset,
    )
}

pub(in crate::layout::flex) fn horizontal_text_baseline_offset(
    style: &ComputedStyle,
    border_box_width: f32,
    line_stack_offset: f32,
    line_baseline_offset: f32,
) -> Option<f32> {
    let borders = used_border_widths(style);
    let line_baseline_offset = if vertical_text_uses_central_baseline(style) {
        style.line_height / 2.0
    } else {
        line_baseline_offset
    };
    let content_baseline_offset = line_stack_offset + line_baseline_offset;
    match style.writing_mode {
        WritingMode::HorizontalTb => None,
        WritingMode::VerticalLr => {
            Some(borders.left + style.padding.left + content_baseline_offset)
        }
        WritingMode::VerticalRl => {
            Some(border_box_width - borders.right - style.padding.right - content_baseline_offset)
        }
    }
}

pub(in crate::layout::flex) fn vertical_text_uses_central_baseline(style: &ComputedStyle) -> bool {
    matches!(
        style.writing_mode,
        WritingMode::VerticalRl | WritingMode::VerticalLr
    ) && matches!(
        style.text_orientation,
        css::TextOrientation::Mixed | css::TextOrientation::Upright
    )
}

pub(in crate::layout::flex) fn preceding_line_height_before_last(
    sequence: &inline_layout::InlineLineSequence,
) -> f32 {
    (0..sequence.records.len().saturating_sub(1))
        .map(|index| sequence.line_height(index))
        .sum()
}

pub(in crate::layout::flex) fn first_sequence_line_baseline_offset(
    sequence: &inline_layout::InlineLineSequence,
    fallback: f32,
) -> f32 {
    sequence
        .records
        .first()
        .and_then(|record| record.fragment.as_ref())
        .map(|fragment| fragment.metrics.baseline_offset)
        .unwrap_or(fallback)
}

pub(in crate::layout::flex) fn last_sequence_line_baseline_offset(
    sequence: &inline_layout::InlineLineSequence,
    fallback: f32,
) -> f32 {
    sequence
        .records
        .last()
        .and_then(|record| record.fragment.as_ref())
        .map(|fragment| fragment.metrics.baseline_offset)
        .unwrap_or(fallback)
}

pub(in crate::layout::flex) fn merge_outer_intrinsic_widths(
    contribution: &mut inline_layout::InlineIntrinsicContribution,
    child_contribution: (f32, f32),
    child_style: &ComputedStyle,
    containing_inline_size: f32,
) {
    let outer_edges = intrinsic_horizontal_outer_edges(child_style, containing_inline_size);
    contribution.min_content = contribution
        .min_content
        .max(child_contribution.0 + outer_edges);
    contribution.max_content = contribution
        .max_content
        .max(child_contribution.1 + outer_edges);
}

#[cfg(test)]
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
            horizontal_text_baseline_offset(&style, 100.0, 0.0, 50.0),
            Some(50.0)
        );
    }
}

pub(in crate::layout::flex) fn explicit_child_intrinsic_width(
    child_style: &ComputedStyle,
    containing_inline_size: f32,
) -> Option<(f32, f32)> {
    let horizontal_extras = intrinsic_horizontal_non_content(child_style, containing_inline_size);
    used_content_width_or_auto(child_style, containing_inline_size, horizontal_extras)
        .map(|width| (width, width))
}

pub(in crate::layout::flex) fn intrinsic_horizontal_non_content(
    style: &ComputedStyle,
    containing_inline_size: f32,
) -> f32 {
    let padding = used_padding_edges(style, containing_inline_size).to_css_edges();
    horizontal_border_width(style) + padding.left + padding.right
}

pub(in crate::layout::flex) fn intrinsic_horizontal_outer_edges(
    style: &ComputedStyle,
    containing_inline_size: f32,
) -> f32 {
    let margin = used_margin_edges(style, containing_inline_size).to_css_edges();
    intrinsic_horizontal_non_content(style, containing_inline_size) + margin.left + margin.right
}

pub(in crate::layout::flex) fn flex_min_content_block_child_participates(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    !style.display.is_none()
        && !matches!(style.position, Position::Absolute | Position::Fixed)
        && (style.display.is_block_level()
            || is_document_canvas_element(element)
            || is_replaced_element(element))
}

pub(in crate::layout::flex) fn flex_item_child_boxes_include_float(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child_box| {
        if let Some((_, _, child_style, child_children)) = child_box.element_parts() {
            if !matches!(child_style.position, Position::Absolute | Position::Fixed)
                && child_style.float != Float::None
            {
                return true;
            }
            return flex_item_child_boxes_include_float(child_children);
        }
        match child_box {
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                flex_item_child_boxes_include_float(&box_.children)
            }
            _ => false,
        }
    })
}

/// Return a flex available-space record with a definite cross-axis size.
///
/// CSS Flexbox max-content cross sizing for multi-line column containers lays
/// out each item with the largest max-content cross contribution as its
/// available cross size:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes>.
pub(in crate::layout::flex) fn flex_available_with_definite_cross_size(
    mut available: FlexAvailableSpace,
    direction: FlexDirection,
    cross_size: f32,
) -> FlexAvailableSpace {
    if direction.is_row_axis() {
        available.height = Some(cross_size.max(0.0));
        available.height_is_definite = true;
    } else {
        available.width = cross_size.max(0.0);
        available.width_is_definite = true;
    }
    available
}

/// Intrinsic contribution record for one flex item.
///
/// CSS Flexbox defines flex container intrinsic sizes in terms of each item's
/// outer min/max-content contribution, flex base size, hypothetical main size,
/// and grow/shrink factor. Keeping those values explicit avoids reusing one
/// estimated layout size for several distinct spec concepts:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes> and
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexIntrinsicItem {
    pub(in crate::layout::flex) min_main_contribution: f32,
    pub(in crate::layout::flex) max_main_contribution: f32,
    pub(in crate::layout::flex) min_cross_contribution: f32,
    pub(in crate::layout::flex) max_cross_contribution: f32,
    pub(in crate::layout::flex) flex_base_size: f32,
    pub(in crate::layout::flex) hypothetical_main_size: f32,
    pub(in crate::layout::flex) grow: f32,
    pub(in crate::layout::flex) shrink: f32,
}

impl FlexIntrinsicItem {
    pub(in crate::layout::flex) fn new(
        child: &StyledChild<'_>,
        size: FlexItemEstimate,
        direction: FlexDirection,
        available: FlexAvailableSpace,
        container_inline_size: f32,
    ) -> Self {
        let style = &child.style;
        let edges = FlexIntrinsicAxisEdges::for_style(style, direction, container_inline_size);
        let main_basis = if direction.is_row_axis() {
            available.width_is_definite.then_some(available.width)
        } else {
            available.height.filter(|_| available.height_is_definite)
        };
        let cross_basis = if direction.is_row_axis() {
            available.height.filter(|_| available.height_is_definite)
        } else {
            available.width_is_definite.then_some(available.width)
        };
        let definite_main = definite_flex_item_main_content_size(style, direction, main_basis);
        let definite_cross = definite_flex_item_cross_content_size(style, direction, cross_basis);
        let min_main_content = if direction.is_row_axis() {
            size.min_width
        } else {
            size.min_height
        }
        .points();
        let max_main_content = if direction.is_row_axis() {
            size.content_width
        } else {
            size.content_height
        }
        .points();
        let min_cross_content = if direction.is_row_axis() {
            size.min_height
        } else {
            size.min_width
        }
        .points();
        let max_cross_content = if direction.is_row_axis() {
            size.content_height
        } else {
            size.content_width
        }
        .points();
        let flex_base_content =
            estimated_flex_main_content_size(style, size, direction, main_basis);
        let flex_base_size = (flex_base_content + edges.main).max(0.0);
        let min_main_constraint =
            definite_flex_item_min_main_content_size(style, direction, main_basis)
                .map(|size| size + edges.main);
        let max_main_constraint =
            definite_flex_item_max_main_content_size(style, direction, main_basis)
                .map(|size| size + edges.main);
        let min_main_contribution = flex_intrinsic_main_size_contribution(
            min_main_content + edges.main,
            definite_main.map(|size| size + edges.main),
            flex_base_size,
            style.flex_grow,
            style.flex_shrink,
            min_main_constraint,
            max_main_constraint,
        );
        let max_main_contribution = flex_intrinsic_main_size_contribution(
            max_main_content + edges.main,
            definite_main.map(|size| size + edges.main),
            flex_base_size,
            style.flex_grow,
            style.flex_shrink,
            min_main_constraint,
            max_main_constraint,
        );
        let hypothetical_main_size = flex_base_size
            .max(min_main_contribution)
            .min(max_main_contribution.max(min_main_contribution));

        let (min_cross_contribution, max_cross_contribution) =
            if let Some(definite_cross) = definite_cross {
                let contribution = (definite_cross + edges.cross).max(0.0);
                (contribution, contribution)
            } else {
                (
                    (min_cross_content + edges.cross).max(0.0),
                    (max_cross_content + edges.cross).max(0.0),
                )
            };

        Self {
            min_main_contribution,
            max_main_contribution,
            min_cross_contribution,
            max_cross_contribution,
            flex_base_size,
            hypothetical_main_size,
            grow: style.flex_grow.max(0.0),
            shrink: style.flex_shrink.max(0.0),
        }
    }

    pub(in crate::layout::flex) fn resolved_with_flex_fraction(self, flex_fraction: f32) -> f32 {
        let unclamped = if flex_fraction > 0.0 {
            self.flex_base_size + self.grow * flex_fraction
        } else if flex_fraction < 0.0 {
            self.flex_base_size + self.shrink * self.flex_base_size * flex_fraction
        } else {
            self.flex_base_size
        };
        unclamped
            .max(self.min_main_contribution)
            .min(self.max_main_contribution.max(self.min_main_contribution))
            .max(0.0)
    }
}

/// Computes a flex item's intrinsic main-size contribution.
///
/// CSS Flexbox clamps each item contribution by the outer flex base size when
/// the item cannot grow or cannot shrink, and then by definite min/max main
/// sizes:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>.
pub(in crate::layout::flex) fn flex_intrinsic_main_size_contribution(
    content_contribution: f32,
    preferred_main_size: Option<f32>,
    flex_base_size: f32,
    grow: f32,
    shrink: f32,
    min_main_size: Option<f32>,
    max_main_size: Option<f32>,
) -> f32 {
    let mut contribution = preferred_main_size
        .map(|preferred| content_contribution.max(preferred))
        .unwrap_or(content_contribution)
        .max(0.0);
    if grow <= 0.0 {
        contribution = contribution.min(flex_base_size.max(0.0));
    }
    if shrink <= 0.0 {
        contribution = contribution.max(flex_base_size.max(0.0));
    }
    constrain(
        contribution,
        min_main_size.map(|size| size.max(0.0)),
        max_main_size.map(|size| size.max(0.0)),
    )
    .max(0.0)
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexIntrinsicAxisEdges {
    pub(in crate::layout::flex) main: f32,
    pub(in crate::layout::flex) cross: f32,
}

impl FlexIntrinsicAxisEdges {
    pub(in crate::layout::flex) fn for_style(
        style: &ComputedStyle,
        direction: FlexDirection,
        container_inline_size: f32,
    ) -> Self {
        let padding = used_padding_edges(style, container_inline_size).to_css_edges();
        let margin = used_margin_edges(style, container_inline_size).to_css_edges();
        let border = used_border_widths(style);
        let horizontal =
            padding.left + padding.right + border.left + border.right + margin.left + margin.right;
        let vertical =
            padding.top + padding.bottom + border.top + border.bottom + margin.top + margin.bottom;
        if direction.is_row_axis() {
            Self {
                main: horizontal,
                cross: vertical,
            }
        } else {
            Self {
                main: vertical,
                cross: horizontal,
            }
        }
    }
}

pub(in crate::layout::flex) fn definite_flex_item_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: Option<f32>,
) -> Option<f32> {
    if direction.is_row_axis() {
        let horizontal_non_content = main_basis
            .map(|basis| intrinsic_horizontal_non_content(style, basis))
            .unwrap_or_else(|| {
                style.padding.left + style.padding.right + horizontal_border_width(style)
            });
        used_content_width_or_auto_with_optional_basis(style, main_basis, horizontal_non_content)
    } else {
        let vertical_non_content =
            style.padding.top + style.padding.bottom + vertical_border_width(style);
        used_content_height_or_auto_with_optional_basis(style, main_basis, vertical_non_content)
    }
}

pub(in crate::layout::flex) fn definite_flex_item_cross_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    cross_basis: Option<f32>,
) -> Option<f32> {
    if direction.is_row_axis() {
        let vertical_non_content =
            style.padding.top + style.padding.bottom + vertical_border_width(style);
        used_content_height_or_auto_with_optional_basis(style, cross_basis, vertical_non_content)
    } else {
        let horizontal_non_content = cross_basis
            .map(|basis| intrinsic_horizontal_non_content(style, basis))
            .unwrap_or_else(|| {
                style.padding.left + style.padding.right + horizontal_border_width(style)
            });
        used_content_width_or_auto_with_optional_basis(style, cross_basis, horizontal_non_content)
    }
}

pub(in crate::layout::flex) fn definite_flex_item_min_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: Option<f32>,
) -> Option<f32> {
    definite_flex_item_main_axis_content_size(
        if direction.is_row_axis() {
            style.box_values.min_width
        } else {
            style.box_values.min_height
        },
        main_basis,
    )
}

pub(in crate::layout::flex) fn definite_flex_item_max_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: Option<f32>,
) -> Option<f32> {
    definite_flex_item_main_axis_content_size(
        if direction.is_row_axis() {
            style.box_values.max_width
        } else {
            style.box_values.max_height
        },
        main_basis,
    )
}

pub(in crate::layout::flex) fn definite_flex_item_main_axis_content_size(
    value: css::ComputedLengthPercentageOrAuto,
    main_basis: Option<f32>,
) -> Option<f32> {
    used_length_percentage_or_auto_with_optional_basis(value, main_basis).map(|size| size.max(0.0))
}

pub(in crate::layout::flex) fn intrinsic_flex_container_min_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    gap: f32,
    available: FlexAvailableSpace,
) -> f32 {
    if items.is_empty() {
        return 0.0;
    }
    if style.flex_wrap == FlexWrap::NoWrap {
        return items
            .iter()
            .map(|item| item.min_main_contribution)
            .sum::<f32>()
            + intrinsic_gap_total(gap, items.len());
    }

    let line_limit = definite_flex_container_main_size(style, direction, available);
    if let Some(line_limit) = line_limit {
        return intrinsic_flex_lines(items, line_limit, gap)
            .iter()
            .map(|line| line.min_main)
            .fold(0.0f32, f32::max);
    }

    items
        .iter()
        .map(|item| item.hypothetical_main_size.max(item.min_main_contribution))
        .fold(0.0f32, f32::max)
}

pub(in crate::layout::flex) fn intrinsic_flex_container_max_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    gap: f32,
    available: FlexAvailableSpace,
) -> f32 {
    if items.is_empty() {
        return 0.0;
    }
    if style.flex_wrap != FlexWrap::NoWrap
        && let Some(line_limit) = definite_flex_container_main_size(style, direction, available)
    {
        return intrinsic_flex_lines(items, line_limit, gap)
            .iter()
            .map(|line| line.max_main)
            .fold(0.0f32, f32::max);
    }

    let flex_fraction = intrinsic_max_content_flex_fraction(items);
    items
        .iter()
        .map(|item| item.resolved_with_flex_fraction(flex_fraction))
        .sum::<f32>()
        + intrinsic_gap_total(gap, items.len())
}

/// Return the ideal-algorithm max-content flex fraction from Flexbox 9.9.1.1.
///
/// The current Flexbox draft leaves the web-compatible algorithm in 9.9.1.2
/// partially unresolved. Quire therefore implements the concrete ideal
/// flex-fraction algorithm and records any remaining browser-compatibility
/// mismatch as a spec divergence rather than encoding undefined behavior.
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-main-sizes>.
pub(in crate::layout::flex) fn intrinsic_max_content_flex_fraction(
    items: &[FlexIntrinsicItem],
) -> f32 {
    items
        .iter()
        .map(|item| {
            if item.flex_base_size < item.max_main_contribution {
                if item.grow > 0.0 {
                    (item.max_main_contribution - item.flex_base_size) / item.grow
                } else {
                    0.0
                }
            } else if item.flex_base_size > item.max_main_contribution {
                let scaled_shrink = item.shrink * item.flex_base_size;
                if scaled_shrink > 0.0 {
                    (item.max_main_contribution - item.flex_base_size) / scaled_shrink
                } else {
                    0.0
                }
            } else {
                0.0
            }
        })
        .fold(0.0f32, |largest, fraction| largest.max(fraction))
}

pub(in crate::layout::flex) fn intrinsic_flex_container_cross_sizes(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    gap: f32,
    available: FlexAvailableSpace,
    min_main: f32,
    max_main: f32,
) -> (f32, f32) {
    if items.is_empty() {
        return (0.0, 0.0);
    }
    if style.flex_wrap == FlexWrap::NoWrap {
        let min_cross = items
            .iter()
            .map(|item| item.min_cross_contribution)
            .fold(0.0f32, f32::max);
        let max_cross = items
            .iter()
            .map(|item| item.max_cross_contribution)
            .fold(0.0f32, f32::max);
        return (min_cross, max_cross.max(min_cross));
    }

    if let Some(line_limit) =
        intrinsic_flex_container_line_limit(style, direction, available, min_main, max_main)
    {
        let lines = intrinsic_flex_lines(items, line_limit, gap);
        let min_cross =
            intrinsic_flex_container_min_cross_size_for_lines(direction, items, &lines, gap);
        let max_cross = lines.iter().map(|line| line.max_cross).sum::<f32>()
            + intrinsic_gap_total(gap, lines.len());
        return (min_cross, max_cross.max(min_cross));
    }

    let min_cross = items
        .iter()
        .map(|item| item.min_cross_contribution)
        .fold(0.0f32, f32::max);
    if direction.is_column_axis() {
        let max_cross = items
            .iter()
            .map(|item| item.max_cross_contribution)
            .sum::<f32>()
            + intrinsic_gap_total(gap, items.len());
        (min_cross, max_cross.max(min_cross))
    } else {
        let max_cross = items
            .iter()
            .map(|item| item.max_cross_contribution)
            .fold(0.0f32, f32::max);
        (min_cross, max_cross.max(min_cross))
    }
}

/// Return the min-content cross-size for known intrinsic flex lines.
///
/// CSS Flexbox's multi-line intrinsic cross-size rules are asymmetric: row
/// containers sum the per-line min-content cross sizes, but column containers
/// use the largest flex item min-content cross contribution. A definite column
/// main size can still form multiple lines for max-content sizing, but those
/// lines do not make the container's min-content inline size wider:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes>.
pub(in crate::layout::flex) fn intrinsic_flex_container_min_cross_size_for_lines(
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    lines: &[IntrinsicFlexLine],
    gap: f32,
) -> f32 {
    if direction.is_column_axis() {
        return items
            .iter()
            .map(|item| item.min_cross_contribution)
            .fold(0.0f32, f32::max);
    }

    lines.iter().map(|line| line.min_cross).sum::<f32>() + intrinsic_gap_total(gap, lines.len())
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct IntrinsicFlexLine {
    pub(in crate::layout::flex) min_main: f32,
    pub(in crate::layout::flex) max_main: f32,
    pub(in crate::layout::flex) min_cross: f32,
    pub(in crate::layout::flex) max_cross: f32,
}

pub(in crate::layout::flex) fn intrinsic_flex_lines(
    items: &[FlexIntrinsicItem],
    line_limit: f32,
    gap: f32,
) -> Vec<IntrinsicFlexLine> {
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut line_main = 0.0f32;

    for (index, item) in items.iter().enumerate() {
        let item_main = item.hypothetical_main_size.max(0.0);
        let candidate = if index == line_start {
            item_main
        } else {
            line_main + gap.max(0.0) + item_main
        };
        if index > line_start && candidate > line_limit.max(0.0) + 0.01 {
            lines.push(intrinsic_flex_line(&items[line_start..index], gap));
            line_start = index;
            line_main = item_main;
        } else {
            line_main = candidate;
        }
    }

    lines.push(intrinsic_flex_line(&items[line_start..], gap));
    lines
}

pub(in crate::layout::flex) fn intrinsic_flex_line(
    items: &[FlexIntrinsicItem],
    gap: f32,
) -> IntrinsicFlexLine {
    IntrinsicFlexLine {
        min_main: items
            .iter()
            .map(|item| item.min_main_contribution)
            .sum::<f32>()
            + intrinsic_gap_total(gap, items.len()),
        max_main: intrinsic_flex_container_max_main_size_no_wrap(items, gap),
        min_cross: items
            .iter()
            .map(|item| item.min_cross_contribution)
            .fold(0.0f32, f32::max),
        max_cross: items
            .iter()
            .map(|item| item.max_cross_contribution)
            .fold(0.0f32, f32::max),
    }
}

pub(in crate::layout::flex) fn intrinsic_flex_container_max_main_size_no_wrap(
    items: &[FlexIntrinsicItem],
    gap: f32,
) -> f32 {
    let flex_fraction = intrinsic_max_content_flex_fraction(items);
    items
        .iter()
        .map(|item| item.resolved_with_flex_fraction(flex_fraction))
        .sum::<f32>()
        + intrinsic_gap_total(gap, items.len())
}

pub(in crate::layout::flex) fn definite_flex_container_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    if direction.is_row_axis() {
        definite_flex_container_axis_size(
            style.box_values.width,
            available.width_is_definite.then_some(available.width),
        )
    } else {
        definite_flex_container_axis_size(
            style.box_values.height,
            available.height.filter(|_| available.height_is_definite),
        )
    }
}

pub(in crate::layout::flex) fn definite_flex_container_axis_size(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent == 0.0 {
                Some(value.length_points_max_zero())
            } else {
                Some(used_length_percentage(value, percentage_basis?).max(0.0))
            }
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => percentage_basis,
    }
}

pub(in crate::layout::flex) fn intrinsic_flex_container_line_limit(
    style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
    min_main: f32,
    max_main: f32,
) -> Option<f32> {
    let (value, percentage_basis) = if direction.is_row_axis() {
        (
            style.box_values.width,
            available.width_is_definite.then_some(available.width),
        )
    } else {
        (
            style.box_values.height,
            available.height.filter(|_| available.height_is_definite),
        )
    };
    match value {
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_main.max(0.0)),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(max_main.max(min_main).max(0.0)),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .and_then(|limit| {
                    if limit.percent == 0.0 {
                        Some(limit.length_points_max_zero())
                    } else {
                        percentage_basis.map(|basis| used_length_percentage(limit, basis).max(0.0))
                    }
                })
                .or(percentage_basis)
                .unwrap_or(max_main);
            Some(max_main.max(min_main).min(min_main.max(stretch)).max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => {
            definite_flex_container_axis_size(value, percentage_basis)
        }
    }
}

pub(in crate::layout::flex) fn intrinsic_gap_total(gap: f32, item_count: usize) -> f32 {
    gap.max(0.0) * item_count.saturating_sub(1) as f32
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct EstimatedFlexBaselineItem {
    pub(in crate::layout::flex) outer_main_size: f32,
    pub(in crate::layout::flex) outer_cross_size: f32,
    pub(in crate::layout::flex) margin_cross_start: f32,
    pub(in crate::layout::flex) cross_alignment: EstimatedFlexItemCrossAlignment,
    pub(in crate::layout::flex) first_baseline: Option<f32>,
    pub(in crate::layout::flex) last_baseline: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) enum EstimatedFlexItemCrossAlignment {
    Side(PhysicalSide),
    Center,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct EstimatedFlexLineMetrics {
    pub(in crate::layout::flex) line_count: usize,
    pub(in crate::layout::flex) cross_size: f32,
    pub(in crate::layout::flex) first_baseline: Option<f32>,
    pub(in crate::layout::flex) last_baseline: Option<f32>,
}

#[derive(Debug, Clone)]
pub(in crate::layout::flex) struct EstimatedFlexLine {
    pub(in crate::layout::flex) item_indices: Vec<usize>,
    pub(in crate::layout::flex) cross_start: f32,
    pub(in crate::layout::flex) cross_size: f32,
}

pub(in crate::layout::flex) fn estimated_flex_item_cross_axis_baselines(
    size: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> (Option<f32>, Option<f32>) {
    if physical_direction.is_row_axis() {
        (size.first_baseline, size.last_baseline)
    } else {
        (
            size.first_horizontal_baseline,
            size.last_horizontal_baseline,
        )
    }
}
