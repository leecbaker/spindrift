use super::*;

pub(in crate::layout) fn clear_position_insets(style: &mut ComputedStyle) {
    clear_style_insets(style);
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedAxis {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) size: f32,
    pub(in crate::layout) margin_start: f32,
    pub(in crate::layout) margin_end: f32,
}

impl PositionedAxis {
    pub(in crate::layout) fn new(
        start: f32,
        size: f32,
        margin_start: f32,
        margin_end: f32,
    ) -> Self {
        Self {
            start,
            size,
            margin_start,
            margin_end,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum AbsoluteAxisDirection {
    HorizontalLtr,
    HorizontalRtl,
    Vertical,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AbsoluteDefiniteAxis {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) size: f32,
    pub(in crate::layout) end: f32,
    pub(in crate::layout) margin_start: f32,
    pub(in crate::layout) margin_end: f32,
    pub(in crate::layout) non_content: f32,
    pub(in crate::layout) containing_size: f32,
}

/// Resolve auto margins for a fully definite absolutely positioned axis.
///
/// CSS 2.2 defines absolute-position sizing by a constraint equation over
/// start inset, margins, padding, borders, content size, and end inset. Auto
/// margins remain zero for the other non-replaced absolute-position cases, but
/// when both insets and the used size are definite, auto margins absorb the
/// equation's remaining space before overconstraint handling:
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width> and
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height>.
pub(in crate::layout) fn resolve_absolute_definite_axis_auto_margins(
    start_auto: bool,
    end_auto: bool,
    axis: AbsoluteDefiniteAxis,
    direction: AbsoluteAxisDirection,
) -> PositionedAxis {
    let remaining = axis.containing_size
        - axis.start
        - axis.margin_start
        - axis.non_content
        - axis.size
        - axis.margin_end
        - axis.end;

    match (start_auto, end_auto) {
        (true, true) => {
            if matches!(direction, AbsoluteAxisDirection::HorizontalLtr) && remaining < 0.0 {
                return PositionedAxis::new(axis.start, axis.size, 0.0, remaining);
            }
            if matches!(direction, AbsoluteAxisDirection::HorizontalRtl) && remaining < 0.0 {
                return PositionedAxis::new(axis.start, axis.size, remaining, 0.0);
            }
            PositionedAxis::new(
                axis.start,
                axis.size,
                axis.margin_start + remaining / 2.0,
                axis.margin_end + remaining / 2.0,
            )
        }
        (true, false) => PositionedAxis::new(
            axis.start,
            axis.size,
            axis.margin_start + remaining,
            axis.margin_end,
        ),
        (false, true) => PositionedAxis::new(
            axis.start,
            axis.size,
            axis.margin_start,
            axis.margin_end + remaining,
        ),
        (false, false) => match direction {
            AbsoluteAxisDirection::HorizontalRtl => PositionedAxis::new(
                axis.containing_size
                    - axis.end
                    - axis.margin_start
                    - axis.margin_end
                    - axis.non_content
                    - axis.size,
                axis.size,
                axis.margin_start,
                axis.margin_end,
            ),
            AbsoluteAxisDirection::HorizontalLtr | AbsoluteAxisDirection::Vertical => {
                PositionedAxis::new(axis.start, axis.size, axis.margin_start, axis.margin_end)
            }
        },
    }
}

#[cfg(test)]
pub(in crate::layout) fn resolve_absolute_horizontal(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_or_intrinsic_width: f32,
    static_position: StaticHorizontalPosition,
    containing_direction: Direction,
) -> PositionedAxis {
    resolve_absolute_horizontal_with_non_content(
        style,
        containing_block,
        auto_or_intrinsic_width,
        None,
        static_position,
        containing_direction,
        style.padding.left + style.padding.right + horizontal_border_width(style),
    )
}

/// Resolve the horizontal absolute-position equation with the used box-model
/// inset supplied by the formatting context.
///
/// Ordinary boxes use padding plus border widths. Collapsed tables supply
/// zero because their resolved edge borders belong to the grid rather than to
/// the table wrapper's CSS sizing conversion.
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>
/// <https://www.w3.org/TR/css-tables-3/#table-wrapper-box>
///
/// `containing_direction` names the physical start side of the containing
/// block's horizontal axis. In a vertical writing mode that axis is the
/// logical block axis, so it is determined by `vertical-rl` versus
/// `vertical-lr`, not the inline `direction` value.
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
pub(in crate::layout) fn resolve_absolute_horizontal_with_non_content(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_or_intrinsic_width: f32,
    automatic_minimum_width: Option<f32>,
    static_position: StaticHorizontalPosition,
    containing_direction: Direction,
    horizontal_non_content: f32,
) -> PositionedAxis {
    // CSS 2.2 10.3.7, non-replaced absolutely positioned elements. The
    // static position has separate physical left and right distances; RTL
    // static-position containing blocks seed auto horizontal positioning from
    // the static right side before solving for the used left.
    let left = used_inset_left(style, containing_block);
    let right = used_inset_right(style, containing_block);
    let width = used_content_box_width_or_auto(
        style,
        layout_pt(containing_block.width()),
        non_content_pt(horizontal_non_content),
    )
    .or_else(|| {
        matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        )
        .then_some(content_box_pt(auto_or_intrinsic_width))
    })
    .map(|width| {
        constrain_content_width(
            style,
            width,
            PercentageBasis::definite(layout_pt(containing_block.width())),
        )
        .points()
    })
    .map(|width| automatic_minimum_width.map_or(width, |minimum| width.max(minimum)));
    let shrink_to_fit_width = constrain_content_width(
        style,
        content_box_pt(auto_or_intrinsic_width),
        PercentageBasis::definite(layout_pt(containing_block.width())),
    )
    .points();
    let static_left = if static_position.can_fall_outside {
        static_position.left
    } else {
        static_position.left.clamp(0.0, containing_block.width())
    };
    let static_right = if static_position.can_fall_outside {
        static_position.right
    } else {
        static_position.right.clamp(0.0, containing_block.width())
    };
    let margin_start = style.margin.left;
    let margin_end = style.margin.right;
    let non_content = horizontal_non_content;
    let fill_between = |start: f32, end: f32| {
        (containing_block.width() - start - margin_start - non_content - margin_end - end).max(0.0)
    };
    let border_box_size = |content_size: f32| content_size + non_content;
    let start_for_end = |content_size: f32, end: f32| {
        containing_block.width() - end - margin_start - margin_end - border_box_size(content_size)
    };

    match (left, width, right) {
        (Some(start), Some(size), Some(end)) => match containing_direction {
            Direction::Ltr => resolve_absolute_definite_axis_auto_margins(
                style.box_values.margin.left.is_auto(),
                style.box_values.margin.right.is_auto(),
                AbsoluteDefiniteAxis {
                    start,
                    size,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: containing_block.width(),
                },
                AbsoluteAxisDirection::HorizontalLtr,
            ),
            Direction::Rtl => resolve_absolute_definite_axis_auto_margins(
                style.box_values.margin.left.is_auto(),
                style.box_values.margin.right.is_auto(),
                AbsoluteDefiniteAxis {
                    start,
                    size,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: containing_block.width(),
                },
                AbsoluteAxisDirection::HorizontalRtl,
            ),
        },
        (Some(start), Some(size), None) => {
            PositionedAxis::new(start, size, margin_start, margin_end)
        }
        (Some(start), None, Some(end)) if style.display.is_table() => {
            let axis = AbsoluteDefiniteAxis {
                start,
                size: shrink_to_fit_width,
                end,
                margin_start,
                margin_end,
                non_content,
                containing_size: containing_block.width(),
            };
            match containing_direction {
                Direction::Ltr => resolve_absolute_definite_axis_auto_margins(
                    style.box_values.margin.left.is_auto(),
                    style.box_values.margin.right.is_auto(),
                    axis,
                    AbsoluteAxisDirection::HorizontalLtr,
                ),
                Direction::Rtl => resolve_absolute_definite_axis_auto_margins(
                    style.box_values.margin.left.is_auto(),
                    style.box_values.margin.right.is_auto(),
                    axis,
                    AbsoluteAxisDirection::HorizontalRtl,
                ),
            }
        }
        (Some(start), None, Some(end)) => PositionedAxis::new(
            start,
            constrain_content_width(
                style,
                content_box_pt(fill_between(start, end)),
                PercentageBasis::definite(layout_pt(containing_block.width())),
            )
            .points(),
            margin_start,
            margin_end,
        ),
        (Some(start), None, None) => {
            PositionedAxis::new(start, shrink_to_fit_width, margin_start, margin_end)
        }
        (None, Some(size), Some(end)) => {
            PositionedAxis::new(start_for_end(size, end), size, margin_start, margin_end)
        }
        (None, Some(size), None) => match containing_direction {
            Direction::Ltr => PositionedAxis::new(static_left, size, margin_start, margin_end),
            Direction::Rtl => PositionedAxis::new(
                start_for_end(size, static_right),
                size,
                margin_start,
                margin_end,
            ),
        },
        (None, None, Some(end)) => PositionedAxis::new(
            start_for_end(shrink_to_fit_width, end),
            shrink_to_fit_width,
            margin_start,
            margin_end,
        ),
        (None, None, None) => match containing_direction {
            Direction::Ltr => {
                PositionedAxis::new(static_left, shrink_to_fit_width, margin_start, margin_end)
            }
            Direction::Rtl => PositionedAxis::new(
                start_for_end(shrink_to_fit_width, static_right),
                shrink_to_fit_width,
                margin_start,
                margin_end,
            ),
        },
    }
}

/// Return the start direction for physical horizontal inset equations.
///
/// CSS `left` and `right` are physical, while `direction` reverses only a
/// horizontal writing mode's inline axis. Vertical writing modes project the
/// logical block axis onto physical horizontal, whose start side is fixed by
/// the writing mode.
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
pub(in crate::layout) fn physical_horizontal_axis_direction(
    writing_mode: WritingMode,
    direction: Direction,
) -> Direction {
    match block_start_side(writing_mode) {
        PhysicalSide::Left => Direction::Ltr,
        PhysicalSide::Right => Direction::Rtl,
        PhysicalSide::Top | PhysicalSide::Bottom => direction,
    }
}

/// Return the definite content-height basis an absolutely positioned box can
/// expose to descendants before intrinsic width measurement.
///
/// CSS Positioned Layout makes an absolutely positioned box's own containing
/// block definite for percentage resolution, and CSS 2.2's vertical equation
/// can also make `height: auto` definite when both physical insets are set:
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height> and
/// <https://drafts.csswg.org/css-sizing-3/#definite>.
pub(in crate::layout) fn absolute_positioned_content_height_percentage_basis(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    vertical_border_width: f32,
) -> BlockSizePercentageBasis {
    let vertical_non_content = style.padding.top + style.padding.bottom + vertical_border_width;
    if let Some(height) = used_content_box_height_or_auto(
        style,
        layout_pt(containing_block.height()),
        non_content_pt(vertical_non_content),
    ) {
        return PercentageBasis::definite_from(
            constrain_content_height(
                style,
                height,
                PercentageBasis::definite(layout_pt(containing_block.height())),
            ),
            BlockSizeBasisSource::AbsolutePositioned,
        );
    }

    let Some(top) = used_inset_top(style, containing_block) else {
        return PercentageBasis::indefinite();
    };
    let Some(bottom) = used_inset_bottom(style, containing_block) else {
        return PercentageBasis::indefinite();
    };
    let content_height = (containing_block.height()
        - top
        - style.margin.top
        - vertical_non_content
        - style.margin.bottom
        - bottom)
        .max(0.0);
    PercentageBasis::definite_from(
        constrain_content_height(
            style,
            content_box_pt(content_height),
            PercentageBasis::definite(layout_pt(containing_block.height())),
        ),
        BlockSizeBasisSource::AbsolutePositioned,
    )
}

pub(in crate::layout) fn resolve_absolute_vertical(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_height: f32,
    automatic_minimum_height: Option<f32>,
    static_start: f32,
    vertical_border_width: f32,
) -> PositionedAxis {
    // CSS 2.1 10.6.4, non-replaced absolutely positioned elements. Static
    // position is approximated from the layout cursor at the element's source
    // position until layout carries explicit placeholders.
    let top = used_inset_top(style, containing_block);
    let bottom = used_inset_bottom(style, containing_block);
    let height = used_content_box_height_or_auto(
        style,
        layout_pt(containing_block.height()),
        non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width),
    )
    .map(|height| {
        constrain_content_height(
            style,
            height,
            PercentageBasis::definite(layout_pt(containing_block.height())),
        )
        .points()
    })
    .map(|height| automatic_minimum_height.map_or(height, |minimum| height.max(minimum)));
    let auto_height = constrain_content_height(
        style,
        content_box_pt(auto_height),
        PercentageBasis::definite(layout_pt(containing_block.height())),
    )
    .points();
    // CSS 2.2 defines the static position as the hypothetical normal-flow
    // position. It can fall outside the containing block, especially while a
    // nested formatting context is measured in temporary coordinates.
    let margin_start = style.margin.top;
    let margin_end = style.margin.bottom;
    let non_content = style.padding.top + style.padding.bottom + vertical_border_width;
    let fill_between = |start: f32, end: f32| {
        (containing_block.height() - start - margin_start - non_content - margin_end - end).max(0.0)
    };
    let border_box_size = |content_size: f32| content_size + non_content;
    let start_for_end = |content_size: f32, end: f32| {
        containing_block.height() - end - margin_start - margin_end - border_box_size(content_size)
    };

    match (top, height, bottom) {
        (Some(start), Some(size), Some(end)) => resolve_absolute_definite_axis_auto_margins(
            style.box_values.margin.top.is_auto(),
            style.box_values.margin.bottom.is_auto(),
            AbsoluteDefiniteAxis {
                start,
                size,
                end,
                margin_start,
                margin_end,
                non_content,
                containing_size: containing_block.height(),
            },
            AbsoluteAxisDirection::Vertical,
        ),
        (Some(start), Some(size), None) => {
            PositionedAxis::new(start, size, margin_start, margin_end)
        }
        (Some(start), None, Some(end)) if style.display.is_table() => {
            resolve_absolute_definite_axis_auto_margins(
                style.box_values.margin.top.is_auto(),
                style.box_values.margin.bottom.is_auto(),
                AbsoluteDefiniteAxis {
                    start,
                    size: auto_height,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: containing_block.height(),
                },
                AbsoluteAxisDirection::Vertical,
            )
        }
        (Some(start), None, Some(end)) => PositionedAxis::new(
            start,
            constrain_content_height(
                style,
                content_box_pt(fill_between(start, end)),
                PercentageBasis::definite(layout_pt(containing_block.height())),
            )
            .points(),
            margin_start,
            margin_end,
        ),
        (Some(start), None, None) => {
            PositionedAxis::new(start, auto_height, margin_start, margin_end)
        }
        (None, Some(size), Some(end)) => {
            PositionedAxis::new(start_for_end(size, end), size, margin_start, margin_end)
        }
        (None, Some(size), None) => {
            PositionedAxis::new(static_start, size, margin_start, margin_end)
        }
        (None, None, Some(end)) => PositionedAxis::new(
            start_for_end(auto_height, end),
            auto_height,
            margin_start,
            margin_end,
        ),
        (None, None, None) => {
            PositionedAxis::new(static_start, auto_height, margin_start, margin_end)
        }
    }
}
