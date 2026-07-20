use super::split_1::FlexIntrinsicInlineOuterExtras;
use super::*;
use crate::layout::flex::compute::FlexBaselineSet;
use crate::units::IntoLayoutLength;

/// A replaced flex item's physical content-box dimensions before the legacy
/// aspect-ratio constraint routine. Keeping the axes together prevents a
/// width value being re-used as the height half of a later constraint pass.
#[derive(Debug, Clone, Copy)]
struct FlexReplacedContentSize {
    width: ContentBoxLength,
    height: ContentBoxLength,
}

impl FlexReplacedContentSize {
    fn new(width: ContentBoxLength, height: ContentBoxLength) -> Self {
        Self { width, height }
    }

    fn zero() -> Self {
        Self::new(content_box_pt(0.0), content_box_pt(0.0))
    }

    fn constrain_with_aspect_ratio(
        &mut self,
        aspect_ratio: f32,
        auto_axes: ReplacedAutoAxes,
        constraints: ReplacedSizeConstraints,
    ) {
        // `constrain_replaced_size_with_aspect_ratio` is a shared legacy
        // scalar algorithm. Extract only at this named adapter and restore
        // the content-box marker immediately afterward.
        let mut width = self.width.points();
        let mut height = self.height.points();
        constrain_replaced_size_with_aspect_ratio(
            &mut width,
            &mut height,
            aspect_ratio,
            auto_axes,
            constraints,
        );
        *self = Self::new(content_box_pt(width), content_box_pt(height));
    }

    fn width_at_ratio(width: ContentBoxLength, ratio: f32) -> Self {
        Self::new(width, content_box_pt(width.points() / ratio))
    }

    fn height_at_ratio(height: ContentBoxLength, ratio: f32) -> Self {
        Self::new(content_box_pt(height.points() * ratio), height)
    }
}

/// Estimates a replaced flex item without letting main-size constraints alter flex basis.
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
pub(in crate::layout::flex) fn estimate_replaced_flex_item(
    intrinsic: IntrinsicReplacedSize,
    style: &ComputedStyle,
    containing_width: PhysicalContentWidth,
    available: FlexItemAvailableSpace,
) -> Option<FlexItemEstimate> {
    let attribute_aspect_ratio = intrinsic.attribute_aspect_ratio();
    let aspect_ratio = style.aspect_ratio.preferred_ratio(
        true,
        if style.contain.size {
            attribute_aspect_ratio
        } else {
            intrinsic.natural_aspect_ratio()
        },
    );
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    // A flex item's percentage block-size constraints resolve against the
    // containing block's block-size basis, which is independent from its
    // physical inline measurement width. In particular, a row flex item can
    // have a definite stretched height while its width is larger.
    // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
    let block_constraints = ReplacedSizeConstraints {
        min_width: used_min_width(
            style,
            PercentageBasis::definite(containing_width.content_box_length()),
        )
        .map(SemanticLengthExt::points),
        max_width: used_max_width(
            style,
            PercentageBasis::definite(containing_width.content_box_length()),
        )
        .map(SemanticLengthExt::points),
        min_height: used_length_percentage_or_auto_with_basis(
            style.box_values.min_height.clone(),
            available.height_basis,
        )
        .map(|height| height.points().max(0.0)),
        max_height: used_length_percentage_or_auto_with_basis(
            style.box_values.max_height.clone(),
            available.height_basis,
        )
        .map(|height| height.points().max(0.0)),
    };
    let specified_width = used_content_box_width_or_auto(
        style,
        containing_width.content_box_length().into_layout_length(),
        non_content_pt(horizontal_non_content),
    )
    .or(intrinsic.attr_width)
    .or_else(|| {
        available
            .stretched_width
            .map(|width| content_box_pt((width.points() - horizontal_non_content).max(0.0)))
    });
    let specified_height =
        definite_image_content_height_without_percent(style, vertical_non_content)
            .map(content_box_pt)
            .or(intrinsic.attr_height)
            .or_else(|| {
                available
                    .stretched_height
                    .map(|height| content_box_pt((height.points() - vertical_non_content).max(0.0)))
            });
    let width_is_auto = specified_width.is_none();
    let height_is_auto = specified_height.is_none();
    let contained_intrinsic_width = style
        .contain
        .size
        .then(|| {
            style.contain_intrinsic_size.width.clone().map(|width| {
                used_length_percentage(
                    width,
                    PercentageBasis::definite(containing_width.content_box_length()),
                )
                .cast_unit()
            })
        })
        .flatten()
        .unwrap_or_else(|| content_box_pt(0.0));
    let contained_intrinsic_height = style
        .contain
        .size
        .then(|| {
            style.contain_intrinsic_size.height.clone().map(|height| {
                used_length_percentage(
                    height,
                    PercentageBasis::definite(containing_width.content_box_length()),
                )
                .cast_unit()
            })
        })
        .flatten()
        .unwrap_or_else(|| content_box_pt(0.0));
    let base_size = match (specified_width, specified_height, aspect_ratio) {
        (Some(width), None, Some(ratio)) => FlexReplacedContentSize::width_at_ratio(width, ratio),
        (None, Some(height), Some(ratio)) => {
            FlexReplacedContentSize::height_at_ratio(height, ratio)
        }
        // An inline SVG root with only a `viewBox` provides a preferred
        // aspect ratio but no intrinsic dimensions. Its automatic flex size
        // therefore uses the available content-box inline space, rather than
        // CSS's generic default object dimensions; the ratio then supplies
        // the opposite axis. This is the SVG root sizing case in CSS Sizing's
        // flex-item algorithm.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
        // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
        (None, None, Some(ratio)) if !intrinsic.has_intrinsic_size => {
            let available_width = (available.width.points()
                - horizontal_non_content
                - style.margin.left
                - style.margin.right)
                .max(0.0);
            FlexReplacedContentSize::width_at_ratio(content_box_pt(available_width), ratio)
        }
        // Size containment's fallback is an intrinsic size, not a preferred
        // aspect ratio.  When an authored dimension leaves the other axis
        // auto, retain that axis's fallback intrinsic dimension rather than
        // scaling it from the specified one.
        // <https://drafts.csswg.org/css-sizing-4/#intrinsic-size-override>
        (Some(width), None, None) if style.contain.size => {
            FlexReplacedContentSize::new(width, contained_intrinsic_height)
        }
        (None, Some(height), None) if style.contain.size => {
            FlexReplacedContentSize::new(contained_intrinsic_width, height)
        }
        (Some(width), None, None) => FlexReplacedContentSize::new(width, content_box_pt(0.0)),
        (None, Some(height), None) => FlexReplacedContentSize::new(content_box_pt(0.0), height),
        (None, None, _) if style.contain.size => {
            FlexReplacedContentSize::new(contained_intrinsic_width, contained_intrinsic_height)
        }
        (None, None, _) => FlexReplacedContentSize::new(intrinsic.width, intrinsic.height),
        (Some(width), Some(height), _) => FlexReplacedContentSize::new(width, height),
    };
    let Some(aspect_ratio) = aspect_ratio else {
        let width = constrain_content_width(
            style,
            base_size.width,
            PercentageBasis::definite(content_box_pt(containing_width.points().max(1.0))),
        );
        let height = super::split_1::constrain_flex_item_estimated_height(
            style,
            base_size.height,
            base_size.height,
            base_size.height,
            available.height_basis,
            non_content_pt(vertical_non_content),
        )
        .max(content_box_pt(1.0));
        return Some(FlexItemEstimate::from_physical_intrinsic_metrics(
            FlexPhysicalIntrinsicMetrics {
                width: PhysicalContentWidth::new(width),
                height: PhysicalContentHeight::new(height),
                min_width: PhysicalContentWidth::new(width),
                min_height: PhysicalContentHeight::new(height),
                content_width: PhysicalContentWidth::new(width),
                content_height: PhysicalContentHeight::new(height),
            },
            None,
            FlexItemBaselineEstimate::default(),
        ));
    };
    let mut constrained_size = base_size;
    constrained_size.constrain_with_aspect_ratio(
        aspect_ratio,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        block_constraints,
    );

    let mut width_constrained_size = base_size;
    width_constrained_size.constrain_with_aspect_ratio(
        aspect_ratio,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        ReplacedSizeConstraints {
            min_width: used_min_width(
                style,
                PercentageBasis::definite(containing_width.content_box_length()),
            )
            .map(SemanticLengthExt::points),
            max_width: used_max_width(
                style,
                PercentageBasis::definite(containing_width.content_box_length()),
            )
            .map(SemanticLengthExt::points),
            min_height: None,
            max_height: None,
        },
    );

    let mut height_constrained_size = base_size;
    height_constrained_size.constrain_with_aspect_ratio(
        aspect_ratio,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        ReplacedSizeConstraints {
            min_width: None,
            max_width: None,
            min_height: block_constraints.min_height,
            max_height: block_constraints.max_height,
        },
    );

    // An SVG with only a preferred ratio has no intrinsic content-size
    // suggestion for Flexbox's automatic minimum. It still uses the default
    // object size to establish an auto flex base size, but treating that
    // fallback as min-content would prevent normal flex shrinking and make a
    // ratio-only SVG overflow a definite flex container.
    // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
    // <https://www.w3.org/TR/css-images-3/#default-sizing>
    let min_size = if intrinsic.has_intrinsic_size {
        FlexReplacedContentSize::new(
            constrained_size.width.max(content_box_pt(1.0)),
            constrained_size.height.max(content_box_pt(1.0)),
        )
    } else {
        FlexReplacedContentSize::zero()
    };
    Some(FlexItemEstimate::from_physical_intrinsic_metrics(
        FlexPhysicalIntrinsicMetrics {
            width: PhysicalContentWidth::new(constrained_size.width.max(content_box_pt(1.0))),
            height: PhysicalContentHeight::new(constrained_size.height.max(content_box_pt(1.0))),
            min_width: PhysicalContentWidth::new(min_size.width),
            min_height: PhysicalContentHeight::new(min_size.height),
            content_width: PhysicalContentWidth::new(
                height_constrained_size.width.max(content_box_pt(1.0)),
            ),
            content_height: PhysicalContentHeight::new(
                width_constrained_size.height.max(content_box_pt(1.0)),
            ),
        },
        Some(aspect_ratio),
        FlexItemBaselineEstimate::default(),
    ))
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
        PhysicalSide::Left => Some(FlexHorizontalBaselineOffset::new(
            borders.left + style.padding.left + content_baseline_offset,
        )),
        PhysicalSide::Right => Some(FlexHorizontalBaselineOffset::new(
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

pub(in crate::layout::flex) fn merge_outer_intrinsic_widths(
    contribution: &mut inline_layout::InlineIntrinsicContribution,
    child_contribution: inline_layout::InlineIntrinsicContribution,
    child_style: &ComputedStyle,
    containing_inline_size: LogicalInlineContentSize,
) {
    let outer_edges = intrinsic_horizontal_outer_edges(child_style, containing_inline_size);
    contribution.include_max(inline_layout::InlineIntrinsicContribution::new(
        outer_edges.add_to(child_contribution.min_content),
        outer_edges.add_to(child_contribution.max_content),
    ));
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
            horizontal_text_baseline_offset(
                &style,
                border_box_pt(100.0),
                layout_pt(0.0),
                layout_pt(50.0),
            ),
            Some(FlexHorizontalBaselineOffset::new(50.0))
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

        // A 100pt content box has a 117pt border box. The vertical-rl
        // baseline is measured from the left edge after subtracting the
        // right border and padding, not from the content-box width.
        assert_eq!(
            horizontal_text_baseline_offset(
                &style,
                border_box_pt(117.0),
                layout_pt(0.0),
                layout_pt(10.0),
            ),
            Some(FlexHorizontalBaselineOffset::new(95.0))
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
            Some(FlexHorizontalBaselineOffset::new(12.0))
        );
    }

    #[test]
    fn replaced_flex_estimate_resolves_percentage_max_height_from_block_basis() {
        let mut style = ComputedStyle::initial();
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(1.0),
        );
        let available = FlexItemAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(200.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(200.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(100.0))),
            height_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            stretched_width: None,
            stretched_height: None,
        };
        let estimate = estimate_replaced_flex_item(
            IntrinsicReplacedSize {
                width: content_box_pt(200.0),
                height: content_box_pt(200.0),
                preferred_aspect_ratio: Some(1.0),
                has_intrinsic_size: true,
                attr_width: None,
                attr_height: None,
            },
            &style,
            PhysicalContentWidth::new(content_box_pt(200.0)),
            available,
        )
        .expect("a square intrinsic image is estimable");

        assert_eq!(estimate.width.points(), 100.0);
        assert_eq!(estimate.height.points(), 100.0);
    }

    fn intrinsic_item(main: FlexMainSize, cross: FlexCrossSize) -> FlexIntrinsicItem {
        FlexIntrinsicItem {
            min_main_contribution: main,
            max_main_contribution: main,
            min_cross_contribution: cross,
            max_cross_contribution: cross,
            flex_base_size: main,
            hypothetical_main_size: main,
            grow: FlexGrowFactor::new(0.0),
            shrink: FlexShrinkFactor::new(1.0),
            preferred_aspect_ratio: None,
            automatic_cross_size: false,
            main_outer_extras: FlexMainLength::new(0.0),
            cross_outer_extras: FlexCrossLength::new(0.0),
        }
    }

    #[test]
    fn intrinsic_contribution_merge_keeps_typed_inline_sizes_and_signed_outer_extras() {
        let mut style = ComputedStyle::initial();
        style.padding.left = 10.0;
        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(-30.0),
        );
        let inline_size = LogicalInlineContentSize::new(content_box_pt(200.0));
        let extras = intrinsic_horizontal_outer_edges(&style, inline_size);
        assert_eq!(
            extras
                .add_to(LogicalInlineContentSize::new(content_box_pt(50.0)))
                .points(),
            30.0
        );

        let mut parent = inline_layout::InlineIntrinsicContribution::new(
            LogicalInlineContentSize::new(content_box_pt(10.0)),
            LogicalInlineContentSize::new(content_box_pt(20.0)),
        );
        let child = inline_layout::InlineIntrinsicContribution::new(
            LogicalInlineContentSize::new(content_box_pt(50.0)),
            LogicalInlineContentSize::new(content_box_pt(60.0)),
        );
        parent.include_max(child);
        assert_eq!(parent.min_content.points(), 50.0);
        assert_eq!(parent.max_content.points(), 60.0);

        parent = inline_layout::InlineIntrinsicContribution::new(
            LogicalInlineContentSize::new(content_box_pt(10.0)),
            LogicalInlineContentSize::new(content_box_pt(20.0)),
        );
        merge_outer_intrinsic_widths(&mut parent, child, &style, inline_size);

        assert_eq!(parent.min_content.points(), 30.0);
        assert_eq!(parent.max_content.points(), 40.0);
    }

    #[test]
    fn intrinsic_axis_edges_keep_negative_margins_until_contribution_clamp() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(-30.0),
        );
        style.box_values.padding.left = css::ComputedLengthPercentage::from_points(10.0);
        let edges = FlexIntrinsicAxisEdges::for_style(&style, FlexDirection::Row);

        assert_eq!(edges.main, FlexMainLength::new(-20.0));
        assert_eq!(
            flex_intrinsic_main_size_contribution(
                FlexMainSize::new(20.0) + edges.main,
                None,
                None,
                None,
                None,
                None,
            ),
            FlexMainSize::new(0.0)
        );
    }

    #[test]
    fn flexed_main_size_transfers_aspect_ratio_into_typed_cross_contribution() {
        let mut row_item = intrinsic_item(FlexMainSize::new(50.0), FlexCrossSize::new(10.0));
        row_item.grow = FlexGrowFactor::new(1.0);
        row_item.preferred_aspect_ratio = Some(2.0);
        row_item.automatic_cross_size = true;
        apply_single_line_flexed_main_cross_contributions(
            std::slice::from_mut(&mut row_item),
            FlexDirection::Row,
            Some(FlexMainSize::new(100.0)),
        );
        assert_eq!(row_item.min_cross_contribution, FlexCrossSize::new(50.0));
        assert_eq!(row_item.max_cross_contribution, FlexCrossSize::new(50.0));

        let mut column_item = intrinsic_item(FlexMainSize::new(50.0), FlexCrossSize::new(10.0));
        column_item.grow = FlexGrowFactor::new(1.0);
        column_item.preferred_aspect_ratio = Some(2.0);
        column_item.automatic_cross_size = true;
        apply_single_line_flexed_main_cross_contributions(
            std::slice::from_mut(&mut column_item),
            FlexDirection::Column,
            Some(FlexMainSize::new(100.0)),
        );
        assert_eq!(
            column_item.min_cross_contribution,
            FlexCrossSize::new(200.0)
        );
        assert_eq!(
            column_item.max_cross_contribution,
            FlexCrossSize::new(200.0)
        );
    }

    #[test]
    fn intrinsic_lines_keep_main_and_cross_gaps_on_their_own_axes() {
        let items = [
            intrinsic_item(FlexMainSize::new(10.0), FlexCrossSize::new(10.0)),
            intrinsic_item(FlexMainSize::new(10.0), FlexCrossSize::new(15.0)),
        ];
        let line = intrinsic_flex_line(&items, FlexMainSize::new(5.0));
        assert_eq!(line.min_main, FlexMainSize::new(25.0));

        let lines = [
            IntrinsicFlexLine {
                min_main: FlexMainSize::new(10.0),
                max_main: FlexMainSize::new(10.0),
                min_cross: FlexCrossSize::new(10.0),
                max_cross: FlexCrossSize::new(10.0),
            },
            IntrinsicFlexLine {
                min_main: FlexMainSize::new(10.0),
                max_main: FlexMainSize::new(10.0),
                min_cross: FlexCrossSize::new(15.0),
                max_cross: FlexCrossSize::new(15.0),
            },
        ];
        assert_eq!(
            intrinsic_flex_container_min_cross_size_for_lines(
                FlexDirection::Row,
                &items,
                &lines,
                FlexCrossSize::new(7.0),
            ),
            FlexCrossSize::new(32.0)
        );

        let balanced = intrinsic_balanced_flex_lines(
            &[
                intrinsic_item(FlexMainSize::new(40.0), FlexCrossSize::new(1.0)),
                intrinsic_item(FlexMainSize::new(60.0), FlexCrossSize::new(1.0)),
                intrinsic_item(FlexMainSize::new(40.0), FlexCrossSize::new(1.0)),
            ],
            2,
            FlexMainSize::new(10.0),
        );
        assert_eq!(balanced.len(), 2);
        assert_eq!(balanced[0].max_main, FlexMainSize::new(110.0));
    }

    #[test]
    fn wrapped_intrinsic_minimum_uses_item_minimum_when_main_basis_is_indefinite() {
        let mut style = ComputedStyle::initial();
        style.flex_wrap = FlexWrap::Wrap;
        let mut item = intrinsic_item(FlexMainSize::new(50.0), FlexCrossSize::new(20.0));
        item.max_main_contribution = FlexMainSize::new(100.0);
        item.hypothetical_main_size = FlexMainSize::new(100.0);
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(50.0)),
            width_basis: PercentageBasis::indefinite(),
            height: None,
            height_basis: PercentageBasis::indefinite(),
        };

        assert_eq!(
            intrinsic_flex_container_min_main_size(
                &style,
                FlexDirection::Row,
                &[item],
                FlexMainSize::new(0.0),
                available,
            ),
            FlexMainSize::new(50.0),
        );
    }
}

pub(in crate::layout::flex) fn explicit_child_intrinsic_width(
    child_style: &ComputedStyle,
    containing_inline_size: LogicalInlineContentSize,
) -> Option<inline_layout::InlineIntrinsicContribution> {
    let horizontal_extras = intrinsic_horizontal_non_content(child_style, containing_inline_size);
    used_content_box_width_or_auto(
        child_style,
        layout_pt(containing_inline_size.points()),
        horizontal_extras,
    )
    .map(SemanticLengthExt::points)
    .map(|width| {
        let width = LogicalInlineContentSize::new(content_box_pt(width));
        inline_layout::InlineIntrinsicContribution::new(width, width)
    })
}

/// Whether a table's preferred physical width depends on a percentage basis.
///
/// Flexbox asks for a table's intrinsic automatic minimum with an indefinite
/// basis, then resolves its preferred flex base against the definite flex
/// container main size. This predicate keeps those two table queries distinct:
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>.
pub(in crate::layout::flex) fn table_width_depends_on_percentage_basis(
    style: &ComputedStyle,
) -> bool {
    style.display.is_table()
        && matches!(
            &style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
                if value.contains_percentage()
        )
}

pub(in crate::layout::flex) fn intrinsic_horizontal_non_content(
    style: &ComputedStyle,
    containing_inline_size: LogicalInlineContentSize,
) -> NonContentLength {
    let padding = used_padding_edges(
        style,
        PercentageBasis::definite(layout_pt(containing_inline_size.points())),
    )
    .to_css_edges();
    non_content_pt(padding.left + padding.right + horizontal_border_width(style))
}

pub(in crate::layout::flex) fn intrinsic_horizontal_outer_edges(
    style: &ComputedStyle,
    containing_inline_size: LogicalInlineContentSize,
) -> FlexIntrinsicInlineOuterExtras {
    let metrics = intrinsic_box_metrics(style);
    FlexIntrinsicInlineOuterExtras::new(layout_pt(
        intrinsic_horizontal_non_content(style, containing_inline_size).points()
            + metrics.margin.left.points()
            + metrics.margin.right.points(),
    ))
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
    available: FlexAvailableSpace,
    direction: FlexDirection,
    cross_size: FlexCrossSize,
) -> FlexAvailableSpace {
    available.with_definite_cross_size(direction, cross_size)
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
    pub(in crate::layout::flex) min_main_contribution: FlexMainSize,
    pub(in crate::layout::flex) max_main_contribution: FlexMainSize,
    pub(in crate::layout::flex) min_cross_contribution: FlexCrossSize,
    pub(in crate::layout::flex) max_cross_contribution: FlexCrossSize,
    pub(in crate::layout::flex) flex_base_size: FlexMainSize,
    pub(in crate::layout::flex) hypothetical_main_size: FlexMainSize,
    pub(in crate::layout::flex) grow: FlexGrowFactor,
    pub(in crate::layout::flex) shrink: FlexShrinkFactor,
    pub(in crate::layout::flex) preferred_aspect_ratio: Option<f32>,
    pub(in crate::layout::flex) automatic_cross_size: bool,
    pub(in crate::layout::flex) main_outer_extras: FlexMainLength,
    pub(in crate::layout::flex) cross_outer_extras: FlexCrossLength,
}

impl FlexIntrinsicItem {
    pub(in crate::layout::flex) fn new(
        child: &StyledChild<'_>,
        size: FlexItemEstimate,
        direction: FlexDirection,
        available: FlexAvailableSpace,
    ) -> Self {
        let style = &child.style;
        let edges = FlexIntrinsicAxisEdges::for_style(style, direction);
        let main_percentage_basis = if direction.is_row_axis() {
            available.width_basis
        } else {
            available.height_basis
        };
        let cross_basis = available.cross_basis(direction);
        let definite_main =
            definite_flex_item_main_content_size(style, direction, main_percentage_basis);
        let definite_cross = definite_flex_item_cross_content_size(style, direction, cross_basis);
        let min_main_content = if direction.is_row_axis() {
            size.min_width
        } else {
            size.min_height
        };
        let max_main_content = if direction.is_row_axis() {
            size.content_width
        } else {
            size.content_height
        };
        let min_cross_content = if direction.is_row_axis() {
            size.min_height
        } else {
            size.min_width
        };
        let max_cross_content = if direction.is_row_axis() {
            size.content_height
        } else {
            size.content_width
        };
        let flex_base_content =
            estimated_flex_main_content_size(style, size, direction, main_percentage_basis);
        let flex_base_size =
            (flex_main_size_from_content_box(flex_base_content) + edges.main).non_negative_size();
        let min_main_constraint =
            definite_flex_item_min_main_content_size(style, direction, main_percentage_basis)
                .map(|size| flex_main_size_from_content_box(size) + edges.main)
                .map(FlexMainLength::non_negative_size);
        let max_main_constraint =
            definite_flex_item_max_main_content_size(style, direction, main_percentage_basis)
                .map(|size| flex_main_size_from_content_box(size) + edges.main)
                .map(FlexMainLength::non_negative_size);
        let automatic_main_minimum = flex_min_size_uses_automatic_minimum(
            if direction.is_row_axis() {
                style.box_values.min_width.clone()
            } else {
                style.box_values.min_height.clone()
            },
            style.writing_mode,
            direction,
        );
        let definite_flex_base_size = (!automatic_main_minimum)
            .then(|| {
                definite_intrinsic_flex_base_size(style, flex_base_size, main_percentage_basis)
            })
            .flatten();
        let min_main_contribution = flex_intrinsic_main_size_contribution(
            flex_main_size_from_content_box(min_main_content) + edges.main,
            definite_main
                .map(flex_main_size_from_content_box)
                .map(|size| size + edges.main),
            definite_flex_base_size,
            // A growable item that cannot shrink retains its flex base size in
            // a min-content flex container. The non-growing `auto`-basis
            // case instead derives its contribution from max-content below:
            // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-main-sizes>.
            (style.flex_shrink <= 0.0 && style.flex_grow > 0.0).then_some(flex_base_size),
            min_main_constraint,
            max_main_constraint,
        );
        let max_main_contribution = flex_intrinsic_main_size_contribution(
            flex_main_size_from_content_box(max_main_content) + edges.main,
            definite_main
                .map(flex_main_size_from_content_box)
                .map(|size| size + edges.main),
            definite_flex_base_size,
            (style.flex_shrink <= 0.0).then_some(flex_base_size),
            min_main_constraint,
            max_main_constraint,
        );
        let hypothetical_main_size = flex_base_size
            .max(min_main_contribution)
            .min(max_main_contribution.max(min_main_contribution));

        let (min_cross_contribution, max_cross_contribution) =
            if let Some(definite_cross) = definite_cross {
                let contribution = (flex_cross_size_from_content_box(definite_cross) + edges.cross)
                    .non_negative_size();
                (contribution, contribution)
            } else {
                let (min_cross_content, max_cross_content) =
                    constrained_flex_intrinsic_cross_content_sizes(
                        style,
                        direction,
                        min_cross_content,
                        max_cross_content,
                        cross_basis,
                    );
                (
                    (flex_cross_size_from_content_box(min_cross_content) + edges.cross)
                        .non_negative_size(),
                    (flex_cross_size_from_content_box(max_cross_content) + edges.cross)
                        .non_negative_size(),
                )
            };

        Self {
            min_main_contribution,
            max_main_contribution,
            min_cross_contribution,
            max_cross_contribution,
            flex_base_size,
            hypothetical_main_size,
            grow: FlexGrowFactor::new(style.flex_grow),
            shrink: FlexShrinkFactor::new(style.flex_shrink),
            preferred_aspect_ratio: style
                .aspect_ratio
                .preferred_ratio_for_non_replaced(child.is_replaced_element()),
            automatic_cross_size: if direction.is_row_axis() {
                style.box_values.height.is_auto()
            } else {
                style.box_values.width.is_auto()
            },
            main_outer_extras: edges.main,
            cross_outer_extras: edges.cross,
        }
    }

    pub(in crate::layout::flex) fn resolved_with_flex_fraction(
        self,
        flex_fraction: FlexIntrinsicFraction,
    ) -> FlexMainSize {
        let unclamped = match flex_fraction {
            FlexIntrinsicFraction::Grow(fraction) => {
                self.grow.resolve(self.flex_base_size, fraction)
            }
            FlexIntrinsicFraction::Shrink(fraction) => {
                self.shrink.resolve(self.flex_base_size, fraction)
            }
            FlexIntrinsicFraction::None => self.flex_base_size,
        };
        unclamped
            .max(self.min_main_contribution)
            .min(self.max_main_contribution.max(self.min_main_contribution))
    }
}

/// Transfer resolved flexible main sizes into automatic cross contributions.
///
/// A flex item's intrinsic cross contribution is determined after flexible
/// lengths resolve when the flex container has one line and a definite main
/// size. A preferred aspect ratio then makes the automatic cross axis depend
/// on that resolved main size rather than on the pre-flex base size.
/// <https://www.w3.org/TR/css-flexbox-1/#line-sizing> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
pub(in crate::layout::flex) fn apply_single_line_flexed_main_cross_contributions(
    items: &mut [FlexIntrinsicItem],
    direction: FlexDirection,
    available_main_size: Option<FlexMainSize>,
) {
    let Some(available_main_size) = available_main_size else {
        return;
    };
    let base_size = items.iter().fold(FlexMainSize::new(0.0), |sum, item| {
        sum + item.flex_base_size
    });
    let free_space = available_main_size - base_size;
    let free_space = free_space.non_negative_size();
    if free_space.points() <= 0.0 {
        return;
    }
    let total_grow = FlexGrowFactor::new(items.iter().map(|item| item.grow.value()).sum());
    let Some(grow_fraction) = FlexGrowFraction::from_free_space(free_space, total_grow) else {
        return;
    };
    for item in items {
        if !item.automatic_cross_size {
            continue;
        }
        let Some(ratio) = item.preferred_aspect_ratio.filter(|ratio| *ratio > 0.0) else {
            continue;
        };
        let resolved_main = item.grow.resolve(item.flex_base_size, grow_fraction);
        let main_content = (resolved_main + item.main_outer_extras.negated()).non_negative_size();
        let cross_content = flex_cross_size_from_main_aspect_ratio(main_content, direction, ratio);
        let cross = (cross_content + item.cross_outer_extras).non_negative_size();
        item.min_cross_contribution = item.min_cross_contribution.max(cross);
        item.max_cross_contribution = item.max_cross_contribution.max(cross);
    }
}

/// Applies cross-axis intrinsic min/max constraints in content-box space.
///
/// An automatic preferred cross size does not exempt its intrinsic
/// contributions from `min-width`/`max-width` or their height equivalents.
/// Column-wrap intrinsic sizing sums these constrained contributions into its
/// generated columns:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>.
fn constrained_flex_intrinsic_cross_content_sizes(
    style: &ComputedStyle,
    direction: FlexDirection,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    percentage_basis: FlexAvailablePercentageBasis,
) -> (ContentBoxLength, ContentBoxLength) {
    if direction.is_row_axis() {
        let non_content =
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style));
        (
            constrain_height_with_intrinsic(
                style,
                min_content,
                min_content,
                max_content,
                percentage_basis,
                non_content,
            ),
            constrain_height_with_intrinsic(
                style,
                max_content,
                min_content,
                max_content,
                percentage_basis,
                non_content,
            ),
        )
    } else {
        let non_content = percentage_basis
            .points()
            .map(|basis| {
                let padding =
                    used_padding_edges(style, PercentageBasis::definite(layout_pt(basis)))
                        .to_css_edges();
                non_content_pt(horizontal_border_width(style) + padding.left + padding.right)
            })
            .unwrap_or_else(|| {
                non_content_pt(
                    horizontal_border_width(style) + style.padding.left + style.padding.right,
                )
            });
        (
            constrain_width_with_intrinsic(
                style,
                min_content,
                min_content,
                max_content,
                percentage_basis,
                non_content,
            ),
            constrain_width_with_intrinsic(
                style,
                max_content,
                min_content,
                max_content,
                percentage_basis,
                non_content,
            ),
        )
    }
}

/// Computes a flex item's intrinsic main-size contribution.
///
/// CSS Flexbox derives each contribution from its intrinsic size and a
/// non-auto preferred main size, then clamps it by definite min/max main
/// sizes. A definite flex basis caps a non-growing item's contribution, but
/// an automatic basis does not replace an auto preferred min-content size.
/// An inflexible item still floors its max-content contribution at its flex
/// base size so max-content layout preserves its used base size:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>.
pub(in crate::layout::flex) fn flex_intrinsic_main_size_contribution(
    content_contribution: FlexMainLength,
    preferred_main_size: Option<FlexMainLength>,
    definite_flex_base_size: Option<FlexMainSize>,
    inflexible_flex_base_size: Option<FlexMainSize>,
    min_main_size: Option<FlexMainSize>,
    max_main_size: Option<FlexMainSize>,
) -> FlexMainSize {
    let contribution = preferred_main_size
        .map(|preferred| content_contribution.max(preferred))
        .unwrap_or(content_contribution)
        .non_negative_size();
    let contribution = definite_flex_base_size
        .map(|basis| contribution.min(basis))
        .unwrap_or(contribution);
    let contribution = inflexible_flex_base_size
        .map(|basis| contribution.max(basis))
        .unwrap_or(contribution);
    let contribution = min_main_size
        .map(|minimum| contribution.max(minimum))
        .unwrap_or(contribution);
    max_main_size
        .map(|maximum| contribution.min(maximum))
        .unwrap_or(contribution)
}

/// Return the flex base size only when the authored basis resolves without an
/// indefinite percentage basis. `flex-basis:auto` instead follows the
/// preferred main size and must not constrain intrinsic contributions.
fn definite_intrinsic_flex_base_size(
    style: &ComputedStyle,
    flex_base_size: FlexMainSize,
    main_percentage_basis: FlexAvailablePercentageBasis,
) -> Option<FlexMainSize> {
    match &style.flex_basis {
        css::ComputedFlexBasis::LengthPercentage(length)
            if style.flex_grow <= 0.0
                && (!length.contains_percentage() || main_percentage_basis.is_definite()) =>
        {
            Some(flex_base_size)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexIntrinsicAxisEdges {
    pub(in crate::layout::flex) main: FlexMainLength,
    pub(in crate::layout::flex) cross: FlexCrossLength,
}

impl FlexIntrinsicAxisEdges {
    pub(in crate::layout::flex) fn for_style(
        style: &ComputedStyle,
        direction: FlexDirection,
    ) -> Self {
        let metrics = intrinsic_box_metrics(style);
        let padding = metrics.padding.to_css_edges();
        let margin = metrics.margin.to_css_edges();
        let border = metrics.border.to_css_edges();
        let horizontal =
            padding.left + padding.right + border.left + border.right + margin.left + margin.right;
        let vertical =
            padding.top + padding.bottom + border.top + border.bottom + margin.top + margin.bottom;
        if direction.is_row_axis() {
            Self {
                main: FlexMainLength::new(horizontal),
                cross: FlexCrossLength::new(vertical),
            }
        } else {
            Self {
                main: FlexMainLength::new(vertical),
                cross: FlexCrossLength::new(horizontal),
            }
        }
    }
}

pub(in crate::layout::flex) fn definite_flex_item_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: FlexAvailablePercentageBasis,
) -> Option<ContentBoxLength> {
    if direction.is_row_axis() {
        let horizontal_non_content = main_basis
            .points()
            .map(|basis| {
                let padding =
                    used_padding_edges(style, PercentageBasis::definite(layout_pt(basis)))
                        .to_css_edges();
                horizontal_border_width(style) + padding.left + padding.right
            })
            .unwrap_or_else(|| {
                style.padding.left + style.padding.right + horizontal_border_width(style)
            });
        used_content_box_width_or_auto_with_basis(
            style,
            main_basis,
            non_content_pt(horizontal_non_content),
        )
    } else {
        let vertical_non_content =
            style.padding.top + style.padding.bottom + vertical_border_width(style);
        used_content_box_height_or_auto_with_basis(
            style,
            main_basis,
            non_content_pt(vertical_non_content),
        )
    }
}

pub(in crate::layout::flex) fn definite_flex_item_cross_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    cross_basis: FlexAvailablePercentageBasis,
) -> Option<ContentBoxLength> {
    if direction.is_row_axis() {
        let vertical_non_content =
            style.padding.top + style.padding.bottom + vertical_border_width(style);
        used_content_box_height_or_auto_with_basis(
            style,
            cross_basis,
            non_content_pt(vertical_non_content),
        )
        .map(|height| {
            constrain_height_with_intrinsic(
                style,
                height,
                height,
                height,
                cross_basis,
                non_content_pt(vertical_non_content),
            )
        })
    } else {
        let horizontal_non_content = cross_basis
            .points()
            .map(|basis| {
                let padding =
                    used_padding_edges(style, PercentageBasis::definite(layout_pt(basis)))
                        .to_css_edges();
                horizontal_border_width(style) + padding.left + padding.right
            })
            .unwrap_or_else(|| {
                style.padding.left + style.padding.right + horizontal_border_width(style)
            });
        used_content_box_width_or_auto_with_basis(
            style,
            cross_basis,
            non_content_pt(horizontal_non_content),
        )
        .map(|width| constrain_content_width(style, width, cross_basis))
    }
}

pub(in crate::layout::flex) fn definite_flex_item_min_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: FlexAvailablePercentageBasis,
) -> Option<ContentBoxLength> {
    definite_flex_item_main_axis_content_size(
        style,
        direction,
        if direction.is_row_axis() {
            style.box_values.min_width.clone()
        } else {
            style.box_values.min_height.clone()
        },
        main_basis,
    )
}

pub(in crate::layout::flex) fn definite_flex_item_max_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: FlexAvailablePercentageBasis,
) -> Option<ContentBoxLength> {
    definite_flex_item_main_axis_content_size(
        style,
        direction,
        if direction.is_row_axis() {
            style.box_values.max_width.clone()
        } else {
            style.box_values.max_height.clone()
        },
        main_basis,
    )
}

pub(in crate::layout::flex) fn definite_flex_item_main_axis_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    value: css::ComputedLengthPercentageOrAuto,
    main_basis: FlexAvailablePercentageBasis,
) -> Option<ContentBoxLength> {
    let non_content = if direction.is_row_axis() {
        let padding = main_basis
            .points()
            .map(|basis| {
                used_padding_edges(style, PercentageBasis::definite(layout_pt(basis)))
                    .to_css_edges()
            })
            .unwrap_or(style.padding);
        non_content_pt(padding.left + padding.right + horizontal_border_width(style))
    } else {
        non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style))
    };
    used_content_box_size_with_basis(value, style.box_sizing, main_basis, non_content)
}

pub(in crate::layout::flex) fn intrinsic_flex_container_min_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    gap: FlexMainSize,
    available: FlexAvailableSpace,
) -> FlexMainSize {
    if items.is_empty() {
        return FlexMainSize::new(0.0);
    }
    if style.flex_wrap == FlexWrap::NoWrap {
        return items
            .iter()
            .map(|item| item.min_main_contribution)
            .fold(FlexMainSize::new(0.0), |sum, size| sum + size)
            + intrinsic_gap_total(gap, items.len());
    }

    let intrinsic_min_line_limit = items
        .iter()
        .map(|item| item.min_main_contribution)
        .fold(FlexMainSize::new(0.0), FlexMainSize::max);
    let intrinsic_max_line_limit = intrinsic_flex_container_max_main_size_no_wrap(items, gap);
    let line_limit = intrinsic_flex_container_line_limit(
        style,
        direction,
        available,
        intrinsic_min_line_limit,
        intrinsic_max_line_limit,
    );
    if let Some(line_limit) = line_limit {
        return intrinsic_flex_lines(items, line_limit, gap)
            .iter()
            .map(|line| line.min_main)
            .fold(FlexMainSize::new(0.0), FlexMainSize::max);
    }

    // An indefinite available main size cannot select a wrapping line limit.
    // Min-content sizing nevertheless assumes every flex item may occupy its
    // own line; a hypothetical (often max-content) size would instead turn
    // the automatic minimum into a no-wrap constraint and prevent shrinking
    // in a narrower fragmentainer.
    // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-main-sizes>
    items
        .iter()
        .map(|item| item.min_main_contribution)
        .fold(FlexMainSize::new(0.0), FlexMainSize::max)
}

pub(in crate::layout::flex) fn intrinsic_flex_container_max_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    gap: FlexMainSize,
    available: FlexAvailableSpace,
) -> FlexMainSize {
    if items.is_empty() {
        return FlexMainSize::new(0.0);
    }
    if style.flex_wrap.balances_lines()
        && let Some(line_count) = style.flex_line_count
    {
        return intrinsic_balanced_flex_lines(items, line_count, gap)
            .iter()
            .map(|line| line.max_main)
            .fold(FlexMainSize::new(0.0), FlexMainSize::max);
    }
    let intrinsic_min_line_limit = items
        .iter()
        .map(|item| item.min_main_contribution)
        .fold(FlexMainSize::new(0.0), FlexMainSize::max);
    let intrinsic_max_line_limit = intrinsic_flex_container_max_main_size_no_wrap(items, gap);
    if style.flex_wrap != FlexWrap::NoWrap
        && let Some(line_limit) = intrinsic_flex_container_line_limit(
            style,
            direction,
            available,
            intrinsic_min_line_limit,
            intrinsic_max_line_limit,
        )
    {
        return intrinsic_flex_lines(items, line_limit, gap)
            .iter()
            .map(|line| line.max_main)
            .fold(FlexMainSize::new(0.0), FlexMainSize::max);
    }

    let flex_fraction = intrinsic_max_content_flex_fraction(items);
    items
        .iter()
        .map(|item| item.resolved_with_flex_fraction(flex_fraction))
        .fold(FlexMainSize::new(0.0), |sum, size| sum + size)
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
) -> FlexIntrinsicFraction {
    let value = items
        .iter()
        .map(|item| {
            if item.flex_base_size < item.max_main_contribution {
                if item.grow.value() > 0.0 {
                    (item.max_main_contribution - item.flex_base_size).points() / item.grow.value()
                } else {
                    0.0
                }
            } else if item.flex_base_size > item.max_main_contribution {
                let scaled_shrink = item.shrink.value() * item.flex_base_size.points();
                if scaled_shrink > 0.0 {
                    (item.max_main_contribution - item.flex_base_size).points() / scaled_shrink
                } else {
                    0.0
                }
            } else {
                0.0
            }
        })
        .fold(0.0f32, |largest, fraction| largest.max(fraction));
    FlexIntrinsicFraction::from_algorithm_value(value)
}

/// Typed inputs to Flexbox's intrinsic cross-size calculation.
///
/// Line formation consumes the main-axis gap, while stacking the resulting
/// lines consumes the cross-axis gap. Keeping both values in this composite
/// prevents their formerly scalar call sites from being interchanged.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct IntrinsicFlexCrossSizeInputs {
    pub(in crate::layout::flex) main_gap: FlexMainSize,
    pub(in crate::layout::flex) cross_gap: FlexCrossSize,
    pub(in crate::layout::flex) available: FlexAvailableSpace,
    pub(in crate::layout::flex) min_main: FlexMainSize,
    pub(in crate::layout::flex) max_main: FlexMainSize,
}

pub(in crate::layout::flex) fn intrinsic_flex_container_cross_sizes(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    inputs: IntrinsicFlexCrossSizeInputs,
) -> (FlexCrossSize, FlexCrossSize) {
    if items.is_empty() {
        return (FlexCrossSize::new(0.0), FlexCrossSize::new(0.0));
    }
    if style.flex_wrap == FlexWrap::NoWrap {
        let min_cross = items
            .iter()
            .map(|item| item.min_cross_contribution)
            .fold(FlexCrossSize::new(0.0), FlexCrossSize::max);
        let max_cross = items
            .iter()
            .map(|item| item.max_cross_contribution)
            .fold(FlexCrossSize::new(0.0), FlexCrossSize::max);
        return (min_cross, max_cross.max(min_cross));
    }

    if style.flex_wrap.balances_lines()
        && let Some(line_count) = style.flex_line_count
    {
        let lines = intrinsic_balanced_flex_lines(items, line_count, inputs.main_gap);
        let min_cross = intrinsic_flex_container_min_cross_size_for_lines(
            direction,
            items,
            &lines,
            inputs.cross_gap,
        );
        let max_cross = lines
            .iter()
            .map(|line| line.max_cross)
            .fold(FlexCrossSize::new(0.0), |sum, size| sum + size)
            + intrinsic_gap_total(inputs.cross_gap, lines.len());
        return (min_cross, max_cross.max(min_cross));
    }

    if let Some(line_limit) = intrinsic_flex_container_line_limit(
        style,
        direction,
        inputs.available,
        inputs.min_main,
        inputs.max_main,
    ) {
        let lines = intrinsic_flex_lines(items, line_limit, inputs.main_gap);
        let min_cross = intrinsic_flex_container_min_cross_size_for_lines(
            direction,
            items,
            &lines,
            inputs.cross_gap,
        );
        let max_cross = lines
            .iter()
            .map(|line| line.max_cross)
            .fold(FlexCrossSize::new(0.0), |sum, size| sum + size)
            + intrinsic_gap_total(inputs.cross_gap, lines.len());
        return (min_cross, max_cross.max(min_cross));
    }

    let min_cross = items
        .iter()
        .map(|item| item.min_cross_contribution)
        .fold(FlexCrossSize::new(0.0), FlexCrossSize::max);
    let max_cross = items
        .iter()
        .map(|item| item.max_cross_contribution)
        .fold(FlexCrossSize::new(0.0), FlexCrossSize::max);
    (min_cross, max_cross.max(min_cross))
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
    gap: FlexCrossSize,
) -> FlexCrossSize {
    if direction.is_column_axis() {
        return items
            .iter()
            .map(|item| item.min_cross_contribution)
            .fold(FlexCrossSize::new(0.0), FlexCrossSize::max);
    }

    lines
        .iter()
        .map(|line| line.min_cross)
        .fold(FlexCrossSize::new(0.0), |sum, size| sum + size)
        + intrinsic_gap_total(gap, lines.len())
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct IntrinsicFlexLine {
    pub(in crate::layout::flex) min_main: FlexMainSize,
    pub(in crate::layout::flex) max_main: FlexMainSize,
    pub(in crate::layout::flex) min_cross: FlexCrossSize,
    pub(in crate::layout::flex) max_cross: FlexCrossSize,
}

pub(in crate::layout::flex) fn intrinsic_flex_lines(
    items: &[FlexIntrinsicItem],
    line_limit: FlexMainSize,
    gap: FlexMainSize,
) -> Vec<IntrinsicFlexLine> {
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut line_main = FlexMainSize::new(0.0);

    for (index, item) in items.iter().enumerate() {
        let item_main = item.hypothetical_main_size;
        let candidate = if index == line_start {
            item_main
        } else {
            line_main + gap + item_main
        };
        if index > line_start && candidate.points() > line_limit.points() + 0.01 {
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

/// Partitions an intrinsic balanced flex container into its requested line count.
///
/// With an intrinsic main size, the container's balanced line width is itself
/// what must be determined. The smallest feasible value is the minimum, over
/// contiguous non-empty partitions, of the largest hypothetical outer main
/// extent. This is the intrinsic counterpart of the Level 2 balance algorithm;
/// the resulting lines then compute their normal flex intrinsic contributions:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance> and
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
pub(in crate::layout::flex) fn intrinsic_balanced_flex_lines(
    items: &[FlexIntrinsicItem],
    requested_line_count: usize,
    gap: FlexMainSize,
) -> Vec<IntrinsicFlexLine> {
    if items.is_empty() || requested_line_count == 0 {
        return Vec::new();
    }
    let line_count = requested_line_count.min(items.len());
    let mut prefix = Vec::with_capacity(items.len() + 1);
    prefix.push(FlexMainSize::new(0.0));
    for item in items {
        prefix.push(
            prefix.last().copied().unwrap_or(FlexMainSize::new(0.0)) + item.hypothetical_main_size,
        );
    }
    let line_extent = |start: usize, end: usize| {
        (prefix[end] - prefix[start]).non_negative_size()
            + intrinsic_gap_total(gap, end.saturating_sub(start))
    };
    let mut costs = vec![vec![None; items.len() + 1]; line_count + 1];
    let mut predecessors = vec![vec![None; items.len() + 1]; line_count + 1];
    costs[0][0] = Some(FlexMainSize::new(0.0));
    for line in 1..=line_count {
        for end in line..=items.len() {
            for start in (line - 1)..end {
                let Some(previous) = costs[line - 1][start] else {
                    continue;
                };
                let candidate = previous.max(line_extent(start, end));
                // A later break leaves more items in earlier lines, matching
                // the balance algorithm's required start-biased tie break.
                if costs[line][end].is_none_or(|cost| candidate <= cost) {
                    costs[line][end] = Some(candidate);
                    predecessors[line][end] = Some(start);
                }
            }
        }
    }
    if costs[line_count][items.len()].is_none() {
        return Vec::new();
    }

    let mut ranges = Vec::with_capacity(line_count);
    let mut end = items.len();
    for line in (1..=line_count).rev() {
        let Some(start) = predecessors[line][end] else {
            return Vec::new();
        };
        ranges.push(start..end);
        end = start;
    }
    ranges.reverse();
    ranges
        .into_iter()
        .map(|range| intrinsic_flex_line(&items[range], gap))
        .collect()
}

pub(in crate::layout::flex) fn intrinsic_flex_line(
    items: &[FlexIntrinsicItem],
    gap: FlexMainSize,
) -> IntrinsicFlexLine {
    IntrinsicFlexLine {
        min_main: items
            .iter()
            .map(|item| item.min_main_contribution)
            .fold(FlexMainSize::new(0.0), |sum, size| sum + size)
            + intrinsic_gap_total(gap, items.len()),
        max_main: intrinsic_flex_container_max_main_size_no_wrap(items, gap),
        min_cross: items
            .iter()
            .map(|item| item.min_cross_contribution)
            .fold(FlexCrossSize::new(0.0), FlexCrossSize::max),
        max_cross: items
            .iter()
            .map(|item| item.max_cross_contribution)
            .fold(FlexCrossSize::new(0.0), FlexCrossSize::max),
    }
}

pub(in crate::layout::flex) fn intrinsic_flex_container_max_main_size_no_wrap(
    items: &[FlexIntrinsicItem],
    gap: FlexMainSize,
) -> FlexMainSize {
    let flex_fraction = intrinsic_max_content_flex_fraction(items);
    items
        .iter()
        .map(|item| item.resolved_with_flex_fraction(flex_fraction))
        .fold(FlexMainSize::new(0.0), |sum, size| sum + size)
        + intrinsic_gap_total(gap, items.len())
}

pub(in crate::layout::flex) fn definite_flex_container_axis_size(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: FlexAvailablePercentageBasis,
) -> Option<FlexMainSize> {
    let percentage_basis = percentage_basis.points();
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.is_definitely_absolute() {
                Some(flex_main_size_from_layout_extent(value.length_max_zero()))
            } else {
                value
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        percentage_basis?,
                    )))
                    .map(flex_main_size_from_layout_extent)
            }
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => {
            percentage_basis.map(FlexMainSize::new)
        }
    }
}

pub(in crate::layout::flex) fn intrinsic_flex_container_line_limit(
    style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
    min_main: FlexMainSize,
    max_main: FlexMainSize,
) -> Option<FlexMainSize> {
    let value = if direction.is_row_axis() {
        style.box_values.width.clone()
    } else {
        style.box_values.height.clone()
    };
    let percentage_basis = if direction.is_row_axis() {
        available.width_basis
    } else {
        available.height_basis
    };
    // A wrapping flex container with an automatic main size can still form
    // lines against a definite max main size. This is especially important
    // for column wrapping, whose intrinsic cross size sums the generated
    // columns rather than treating all items as one column:
    // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes>.
    if value.is_auto() {
        let max_value = if direction.is_row_axis() {
            style.box_values.max_width.clone()
        } else {
            style.box_values.max_height.clone()
        };
        if matches!(
            max_value,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
        ) {
            return definite_flex_container_axis_size(max_value, percentage_basis);
        }
    }
    match value {
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_main),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(max_main.max(min_main)),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .and_then(|limit| {
                    if !limit.needs_percentage_basis() {
                        Some(limit.length_max_zero().points())
                    } else {
                        percentage_basis.points().map(|basis| {
                            used_length_percentage(
                                limit,
                                PercentageBasis::definite(layout_pt(basis.max(0.0))),
                            )
                            .points()
                        })
                    }
                })
                .or(percentage_basis.points())
                .map(FlexMainSize::new)
                .unwrap_or(max_main);
            Some(max_main.max(min_main).min(min_main.max(stretch)))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => {
            definite_flex_container_axis_size(value, percentage_basis)
        }
    }
}

pub(in crate::layout::flex) fn intrinsic_gap_total<Axis>(
    gap: FlexAxisSize<Axis>,
    item_count: usize,
) -> FlexAxisSize<Axis> {
    gap.scale(item_count.saturating_sub(1) as f32)
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct EstimatedFlexBaselineItem {
    pub(in crate::layout::flex) outer_main_size: FlexMainSize,
    pub(in crate::layout::flex) outer_cross_size: FlexCrossSize,
    pub(in crate::layout::flex) margin_cross_start: FlexCrossLength,
    pub(in crate::layout::flex) cross_alignment: EstimatedFlexItemCrossAlignment,
    /// The baseline-sharing set this item can contribute to. Baseline-aligned
    /// items with an orthogonal inline axis or an auto cross margin use CSS
    /// Align's fallback alignment and do not establish the flex line's shared
    /// baseline.
    pub(in crate::layout::flex) baseline_set: Option<FlexBaselineSet>,
    pub(in crate::layout::flex) first_baseline: Option<FlexCrossOffset>,
    pub(in crate::layout::flex) last_baseline: Option<FlexCrossOffset>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) enum EstimatedFlexItemCrossAlignment {
    Side(PhysicalSide),
    Center,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct EstimatedFlexLineMetrics {
    pub(in crate::layout::flex) line_count: usize,
    pub(in crate::layout::flex) cross_size: FlexCrossSize,
    pub(in crate::layout::flex) first_baseline: Option<FlexCrossOffset>,
    pub(in crate::layout::flex) last_baseline: Option<FlexCrossOffset>,
}

#[derive(Debug, Clone)]
pub(in crate::layout::flex) struct EstimatedFlexLine {
    pub(in crate::layout::flex) item_indices: Vec<usize>,
    pub(in crate::layout::flex) cross_start: FlexCrossOffset,
    pub(in crate::layout::flex) cross_size: FlexCrossSize,
}

pub(in crate::layout::flex) fn estimated_flex_item_cross_axis_baselines(
    size: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> (Option<FlexCrossOffset>, Option<FlexCrossOffset>) {
    if physical_direction.is_row_axis() {
        (
            size.baselines
                .vertical
                .first
                .map(|baseline| FlexCrossOffset::new(baseline.points())),
            size.baselines
                .vertical
                .last
                .map(|baseline| FlexCrossOffset::new(baseline.points())),
        )
    } else {
        (
            size.baselines
                .horizontal
                .first
                .map(|baseline| FlexCrossOffset::new(baseline.points())),
            size.baselines
                .horizontal
                .last
                .map(|baseline| FlexCrossOffset::new(baseline.points())),
        )
    }
}
