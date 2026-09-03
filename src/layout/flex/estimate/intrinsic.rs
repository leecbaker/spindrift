use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_box_main_size_uses_logical_inline_padding_basis() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(50.0),
        );
        style.box_values.padding.left = css::ComputedLengthPercentage::from_percent(0.1);
        let main_basis = PercentageBasis::definite_from(
            content_box_pt(80.0),
            FlexAvailableSizeSource::ContainingBlock,
        );
        let inline_basis = PercentageBasis::definite_from(
            LogicalInlineContentSize::new(content_box_pt(100.0)),
            FlexAvailableSizeSource::ContainingBlock,
        );

        let content = definite_flex_item_main_content_size(
            &style,
            FlexDirection::Row,
            main_basis,
            inline_basis,
        )
        .expect("fixed border-box width resolves");

        assert_eq!(content.points(), 40.0);
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
            min_main_negative_outer_contribution: FlexMainLength::new(0.0),
            max_main_negative_outer_contribution: FlexMainLength::new(0.0),
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
    fn non_growable_definite_flex_basis_caps_intrinsic_contribution() {
        // `min-width: auto` is a used-size automatic minimum; it must not
        // replace this intrinsic flex-base cap.
        assert_eq!(
            flex_intrinsic_main_size_contribution(
                FlexMainLength::new(30.0),
                None,
                Some(FlexMainSize::new(16.5)),
                None,
                None,
                None,
            ),
            FlexMainSize::new(16.5),
        );
    }

    #[test]
    fn automatic_basis_with_definite_preferred_main_size_caps_contribution() {
        let mut style = ComputedStyle::initial();
        style.flex_grow = css::FlexGrowFactor::ZERO;
        style.flex_basis = css::ComputedFlexBasis::Auto;

        assert_eq!(
            definite_intrinsic_flex_base_size(
                &style,
                FlexMainSize::new(16.5),
                PercentageBasis::indefinite(),
                true,
            ),
            Some(FlexMainSize::new(16.5)),
        );
        assert_eq!(
            definite_intrinsic_flex_base_size(
                &style,
                FlexMainSize::new(16.5),
                PercentageBasis::indefinite(),
                false,
            ),
            None,
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
    fn intrinsic_balance_uses_normal_wrap_count_before_the_minimum() {
        let mut style = ComputedStyle::initial();
        style.flex_wrap = FlexWrap::Balance;
        style.flex_direction = FlexDirection::Column;
        style.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(75.0),
            ),
        );
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(100.0)),
            width_basis: PercentageBasis::indefinite(),
            height: Some(PhysicalContentHeight::new(content_box_pt(75.0))),
            height_basis: PercentageBasis::definite_from(
                content_box_pt(75.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
        };
        let items = (0..4)
            .map(|_| intrinsic_item(FlexMainSize::new(18.75), FlexCrossSize::new(18.75)))
            .collect::<Vec<_>>();

        assert_eq!(
            intrinsic_balanced_line_count(
                &style,
                FlexDirection::Column,
                &items,
                FlexMainSize::new(7.5),
                available,
                FlexMainSize::new(18.75),
                FlexMainSize::new(97.5),
            ),
            2,
        );
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
    /// The signed part of an intrinsic outer main contribution which is below
    /// zero because margins may be negative. Item-local used sizes stay
    /// non-negative, but Flexbox sums outer intrinsic contributions before
    /// clamping the container result.
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>
    pub(in crate::layout::flex) min_main_negative_outer_contribution: FlexMainLength,
    pub(in crate::layout::flex) max_main_negative_outer_contribution: FlexMainLength,
}

impl FlexIntrinsicItem {
    pub(in crate::layout::flex) fn new(
        child: &StyledChild<'_>,
        size: FlexItemEstimate,
        direction: FlexDirection,
        available: FlexAvailableSpace,
        containing_style: &ComputedStyle,
    ) -> Self {
        let style = &child.style;
        let edges = FlexIntrinsicAxisEdges::for_style(style, direction);
        let inline_basis = available.logical_inline_basis(containing_style);
        let main_percentage_basis = if direction.is_row_axis() {
            available.width_basis
        } else {
            available.height_basis
        };
        let cross_basis = available.cross_basis(direction);
        let definite_main = definite_flex_item_main_content_size(
            style,
            direction,
            main_percentage_basis,
            inline_basis,
        );
        let definite_cross =
            definite_flex_item_cross_content_size(style, direction, cross_basis, inline_basis);
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
        let min_main_constraint = definite_flex_item_min_main_content_size(
            style,
            direction,
            main_percentage_basis,
            inline_basis,
        )
        .map(|size| flex_main_size_from_content_box(size) + edges.main)
        .map(FlexMainLength::non_negative_size);
        // Flexbox's content-based automatic minimum is a used min-main-size
        // constraint during intrinsic contribution sizing. Keep it in content
        // box space until joining the signed outer edges, so padding, borders,
        // and margins each contribute exactly once.
        // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
        // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>
        let used_min_main_constraint = automatic_minimum_main_content_size(
            child,
            &size,
            containing_style,
            direction,
            available,
        )
        .map(|size| flex_main_size_from_content_box(size) + edges.main)
        .map(FlexMainLength::non_negative_size)
        .or(min_main_constraint);
        let max_main_constraint = definite_flex_item_max_main_content_size(
            style,
            direction,
            main_percentage_basis,
            inline_basis,
        )
        .map(|size| flex_main_size_from_content_box(size) + edges.main)
        .map(FlexMainLength::non_negative_size);
        // Flexbox caps an inflexible contribution by its flex base, then
        // clamps it by the used minimum and maximum main sizes. In particular,
        // a content-based automatic minimum floors the capped result instead
        // of disabling the flex-base cap.
        // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>
        let definite_flex_base_size = definite_intrinsic_flex_base_size(
            style,
            flex_base_size,
            main_percentage_basis,
            definite_main.is_some(),
        );
        let min_main_signed_contribution = flex_intrinsic_main_size_contribution_unclamped(
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
            used_min_main_constraint,
            max_main_constraint,
        );
        let max_main_signed_contribution = flex_intrinsic_main_size_contribution_unclamped(
            flex_main_size_from_content_box(max_main_content) + edges.main,
            definite_main
                .map(flex_main_size_from_content_box)
                .map(|size| size + edges.main),
            definite_flex_base_size,
            (style.flex_shrink <= 0.0).then_some(flex_base_size),
            used_min_main_constraint,
            max_main_constraint,
        );
        let min_main_contribution = min_main_signed_contribution.non_negative_size();
        let max_main_contribution = max_main_signed_contribution.non_negative_size();
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
                        inline_basis,
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
            grow: FlexGrowFactor::new(style.flex_grow.value()),
            shrink: FlexShrinkFactor::new(style.flex_shrink.value()),
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
            min_main_negative_outer_contribution: FlexMainLength::new(
                min_main_signed_contribution.points().min(0.0),
            ),
            max_main_negative_outer_contribution: FlexMainLength::new(
                max_main_signed_contribution.points().min(0.0),
            ),
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
    inline_basis: LogicalInlinePercentageBasis<FlexAvailableSizeSource>,
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
        let non_content = if inline_basis.is_definite() {
            let padding =
                used_padding_edges_for_logical_inline_basis(style, inline_basis).to_css_edges();
            non_content_pt(horizontal_border_width(style) + padding.left + padding.right)
        } else {
            non_content_pt(
                horizontal_border_width(style) + style.padding.left + style.padding.right,
            )
        };
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
#[cfg(test)]
pub(in crate::layout::flex) fn flex_intrinsic_main_size_contribution(
    content_contribution: FlexMainLength,
    preferred_main_size: Option<FlexMainLength>,
    definite_flex_base_size: Option<FlexMainSize>,
    inflexible_flex_base_size: Option<FlexMainSize>,
    min_main_size: Option<FlexMainSize>,
    max_main_size: Option<FlexMainSize>,
) -> FlexMainSize {
    flex_intrinsic_main_size_contribution_unclamped(
        content_contribution,
        preferred_main_size,
        definite_flex_base_size,
        inflexible_flex_base_size,
        min_main_size,
        max_main_size,
    )
    .non_negative_size()
}

/// Resolve an intrinsic item contribution while retaining a negative outer
/// margin contribution for the container-level merge.
fn flex_intrinsic_main_size_contribution_unclamped(
    content_contribution: FlexMainLength,
    preferred_main_size: Option<FlexMainLength>,
    definite_flex_base_size: Option<FlexMainSize>,
    inflexible_flex_base_size: Option<FlexMainSize>,
    min_main_size: Option<FlexMainSize>,
    max_main_size: Option<FlexMainSize>,
) -> FlexMainLength {
    let contribution = preferred_main_size
        .map(|preferred| content_contribution.max(preferred))
        .unwrap_or(content_contribution);
    let contribution = definite_flex_base_size
        .map(|basis| FlexMainLength::new(contribution.points().min(basis.points())))
        .unwrap_or(contribution);
    let contribution = inflexible_flex_base_size
        .map(|basis| FlexMainLength::new(contribution.points().max(basis.points())))
        .unwrap_or(contribution);
    let contribution = min_main_size
        .map(|minimum| FlexMainLength::new(contribution.points().max(minimum.points())))
        .unwrap_or(contribution);
    max_main_size
        .map(|maximum| FlexMainLength::new(contribution.points().min(maximum.points())))
        .unwrap_or(contribution)
}

/// Return the flex base size when it is definite and the item cannot grow.
///
/// `flex-basis:auto` inherits a definite preferred main size, so it is just
/// as definite a cap as a length-valued `flex-basis`. An automatic basis with
/// an automatic preferred size remains content-based instead.
fn definite_intrinsic_flex_base_size(
    style: &ComputedStyle,
    flex_base_size: FlexMainSize,
    main_percentage_basis: FlexAvailablePercentageBasis,
    has_definite_preferred_main_size: bool,
) -> Option<FlexMainSize> {
    match &style.flex_basis {
        css::ComputedFlexBasis::LengthPercentage(length)
            if style.flex_grow <= 0.0
                && (!length.contains_percentage() || main_percentage_basis.is_definite()) =>
        {
            Some(flex_base_size)
        }
        css::ComputedFlexBasis::Auto
            if style.flex_grow <= 0.0 && has_definite_preferred_main_size =>
        {
            Some(flex_base_size)
        }
        _ => None,
    }
}

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
    inline_basis: LogicalInlinePercentageBasis<FlexAvailableSizeSource>,
) -> Option<ContentBoxLength> {
    if direction.is_row_axis() {
        let horizontal_non_content = if inline_basis.is_definite() {
            let padding =
                used_padding_edges_for_logical_inline_basis(style, inline_basis).to_css_edges();
            horizontal_border_width(style) + padding.left + padding.right
        } else {
            style.padding.left + style.padding.right + horizontal_border_width(style)
        };
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
    inline_basis: LogicalInlinePercentageBasis<FlexAvailableSizeSource>,
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
        let horizontal_non_content = if inline_basis.is_definite() {
            let padding =
                used_padding_edges_for_logical_inline_basis(style, inline_basis).to_css_edges();
            horizontal_border_width(style) + padding.left + padding.right
        } else {
            style.padding.left + style.padding.right + horizontal_border_width(style)
        };
        used_content_box_width_or_auto_with_basis(
            style,
            cross_basis,
            non_content_pt(horizontal_non_content),
        )
        .map(|width| {
            constrain_width_with_intrinsic(
                style,
                width,
                width,
                width,
                cross_basis,
                non_content_pt(horizontal_non_content),
            )
        })
    }
}

pub(in crate::layout::flex) fn definite_flex_item_min_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: FlexAvailablePercentageBasis,
    inline_basis: LogicalInlinePercentageBasis<FlexAvailableSizeSource>,
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
        inline_basis,
    )
}

pub(in crate::layout::flex) fn definite_flex_item_max_main_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    main_basis: FlexAvailablePercentageBasis,
    inline_basis: LogicalInlinePercentageBasis<FlexAvailableSizeSource>,
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
        inline_basis,
    )
}

pub(in crate::layout::flex) fn definite_flex_item_main_axis_content_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    value: css::ComputedLengthPercentageOrAuto,
    main_basis: FlexAvailablePercentageBasis,
    inline_basis: LogicalInlinePercentageBasis<FlexAvailableSizeSource>,
) -> Option<ContentBoxLength> {
    let non_content = if direction.is_row_axis() {
        let padding = if inline_basis.is_definite() {
            used_padding_edges_for_logical_inline_basis(style, inline_basis).to_css_edges()
        } else {
            style.padding
        };
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
        return intrinsic_flex_container_main_sum(items, gap, |item| {
            (
                item.min_main_contribution,
                item.min_main_negative_outer_contribution,
            )
        });
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
    let intrinsic_min_line_limit = items
        .iter()
        .map(|item| item.min_main_contribution)
        .fold(FlexMainSize::new(0.0), FlexMainSize::max);
    let intrinsic_max_line_limit = intrinsic_flex_container_max_main_size_no_wrap(items, gap);
    if style.flex_wrap.balances_lines() {
        let line_count = intrinsic_balanced_line_count(
            style,
            direction,
            items,
            gap,
            available,
            intrinsic_min_line_limit,
            intrinsic_max_line_limit,
        );
        return intrinsic_balanced_flex_lines(items, line_count, gap)
            .iter()
            .map(|line| line.max_main)
            .fold(FlexMainSize::new(0.0), FlexMainSize::max);
    }
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
/// partially unresolved. Spindrift therefore implements the concrete ideal
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

    if style.flex_wrap.balances_lines() {
        let line_count = intrinsic_balanced_line_count(
            style,
            direction,
            items,
            inputs.main_gap,
            inputs.available,
            inputs.min_main,
            inputs.max_main,
        );
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

/// Select the number of lines used by an intrinsic balanced flex container.
///
/// Flexbox Level 2 first forms the normal-wrap lines from hypothetical outer
/// main sizes, then raises that count to the authored `flex-line-count`
/// minimum. Intrinsic cross sizing has the same topology requirement: a
/// definite main-axis constraint can require multiple balanced columns even
/// when the computed minimum is the initial value of one.
///
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>
/// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
fn intrinsic_balanced_line_count(
    style: &ComputedStyle,
    direction: FlexDirection,
    items: &[FlexIntrinsicItem],
    gap: FlexMainSize,
    available: FlexAvailableSpace,
    min_main: FlexMainSize,
    max_main: FlexMainSize,
) -> usize {
    debug_assert!(!items.is_empty());
    let normal_line_count =
        intrinsic_flex_container_line_limit(style, direction, available, min_main, max_main)
            .map(|line_limit| intrinsic_flex_lines(items, line_limit, gap).len())
            .unwrap_or(1);
    normal_line_count
        .max(style.flex_line_count.get())
        .min(items.len())
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
    intrinsic_flex_container_main_sum(items, gap, |item| {
        (
            item.resolved_with_flex_fraction(flex_fraction),
            item.max_main_negative_outer_contribution,
        )
    })
}

/// Sum margin-inclusive intrinsic main contributions and clamp only the
/// resulting container extent. This retains an item's negative outer margin
/// long enough for a following item to occupy that recovered space.
fn intrinsic_flex_container_main_sum(
    items: &[FlexIntrinsicItem],
    gap: FlexMainSize,
    contribution: impl Fn(&FlexIntrinsicItem) -> (FlexMainSize, FlexMainLength),
) -> FlexMainSize {
    let sum = items.iter().fold(FlexMainLength::new(0.0), |sum, item| {
        let (size, negative_outer) = contribution(item);
        sum + FlexMainLength::new(size.points()) + negative_outer
    }) + FlexMainLength::new(intrinsic_gap_total(gap, items.len()).points());
    sum.non_negative_size()
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
        style.box_values.height.value().clone()
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
