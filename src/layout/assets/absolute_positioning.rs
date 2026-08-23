use super::*;
use crate::units::IntoLayoutLength;

pub(in crate::layout) fn clear_position_insets(style: &mut ComputedStyle) {
    clear_style_insets(style);
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedAxis {
    pub(in crate::layout) start: LayoutLength,
    pub(in crate::layout) size: ContentBoxLength,
    pub(in crate::layout) margin_start: LayoutLength,
    pub(in crate::layout) margin_end: LayoutLength,
}

impl PositionedAxis {
    pub(in crate::layout) fn new(
        start: LayoutLength,
        size: ContentBoxLength,
        margin_start: LayoutLength,
        margin_end: LayoutLength,
    ) -> Self {
        Self {
            start,
            size,
            margin_start,
            margin_end,
        }
    }
}

/// A positioned axis's preferred size after content measurement but before
/// the absolute-position equation selects stretch-fit sizing.
///
/// Keeping an automatic preferred size distinct from a measured intrinsic
/// preferred size prevents an explicit `min-content`, `max-content`, or
/// `fit-content` value from accidentally taking the `height: auto` stretch
/// branch when both insets are definite.
/// <https://drafts.csswg.org/css-position/#abspos-sizing>
/// <https://drafts.csswg.org/css-sizing-3/#sizing-values>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum AbsoluteAxisSize {
    Automatic { fit_content: ContentBoxLength },
    Definite(ContentBoxLength),
}

/// A positioned-axis margin before the absolute-position equation resolves
/// automatic margins.
///
/// The computed automatic state must not be stored separately from a numeric
/// zero: after the used size is known, that state determines whether remaining
/// space is assigned to this margin.
/// <https://drafts.csswg.org/css-position/#abspos-margins>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum AbsoluteAxisMargin {
    Auto,
    Length(LayoutLength),
}

impl AbsoluteAxisMargin {
    fn used(self) -> LayoutLength {
        match self {
            Self::Auto => layout_pt(0.0),
            Self::Length(value) => value,
        }
    }

    fn is_auto(self) -> bool {
        matches!(self, Self::Auto)
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
    pub(in crate::layout) start: LayoutLength,
    pub(in crate::layout) size: ContentBoxLength,
    pub(in crate::layout) end: LayoutLength,
    pub(in crate::layout) margin_start: AbsoluteAxisMargin,
    pub(in crate::layout) margin_end: AbsoluteAxisMargin,
    pub(in crate::layout) non_content: NonContentLength,
    pub(in crate::layout) containing_size: LayoutLength,
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
    axis: AbsoluteDefiniteAxis,
    direction: AbsoluteAxisDirection,
) -> PositionedAxis {
    let margin_start = axis.margin_start.used();
    let margin_end = axis.margin_end.used();
    let remaining = axis.containing_size
        - axis.start
        - margin_start
        - axis.non_content.into_layout_length()
        - axis.size.into_layout_length()
        - margin_end
        - axis.end;

    match (axis.margin_start.is_auto(), axis.margin_end.is_auto()) {
        (true, true) => {
            if matches!(direction, AbsoluteAxisDirection::HorizontalLtr) && remaining.points() < 0.0
            {
                return PositionedAxis::new(axis.start, axis.size, layout_pt(0.0), remaining);
            }
            if matches!(direction, AbsoluteAxisDirection::HorizontalRtl) && remaining.points() < 0.0
            {
                return PositionedAxis::new(axis.start, axis.size, remaining, layout_pt(0.0));
            }
            PositionedAxis::new(
                axis.start,
                axis.size,
                margin_start + remaining / 2.0,
                margin_end + remaining / 2.0,
            )
        }
        (true, false) => {
            PositionedAxis::new(axis.start, axis.size, margin_start + remaining, margin_end)
        }
        (false, true) => {
            PositionedAxis::new(axis.start, axis.size, margin_start, margin_end + remaining)
        }
        (false, false) => match direction {
            AbsoluteAxisDirection::HorizontalRtl => PositionedAxis::new(
                axis.containing_size
                    - axis.end
                    - margin_start
                    - margin_end
                    - axis.non_content.into_layout_length()
                    - axis.size.into_layout_length(),
                axis.size,
                margin_start,
                margin_end,
            ),
            AbsoluteAxisDirection::HorizontalLtr | AbsoluteAxisDirection::Vertical => {
                PositionedAxis::new(axis.start, axis.size, margin_start, margin_end)
            }
        },
    }
}

#[cfg(test)]
pub(in crate::layout) fn resolve_absolute_horizontal(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_or_intrinsic_width: f32,
    static_position: PhysicalStaticAxisFallback,
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
    static_position: PhysicalStaticAxisFallback,
    containing_direction: Direction,
    horizontal_non_content: f32,
) -> PositionedAxis {
    // CSS 2.2 10.3.7, non-replaced absolutely positioned elements. The
    // static position has separate physical left and right distances; RTL
    // static-position containing blocks seed auto horizontal positioning from
    // the static right side before solving for the used left.
    let left = used_inset_left(style, containing_block).map(layout_pt);
    let right = used_inset_right(style, containing_block).map(layout_pt);
    let definite_width = used_content_box_width_or_auto(
        style,
        layout_pt(containing_block.width()),
        non_content_pt(horizontal_non_content),
    )
    .map(|width| {
        constrain_content_width(
            style,
            width,
            PercentageBasis::definite(layout_pt(containing_block.width())),
        )
    })
    .map(|width| {
        automatic_minimum_width.map_or(width, |minimum| content_box_pt(width.points().max(minimum)))
    });
    let shrink_to_fit_width = constrain_content_width(
        style,
        content_box_pt(auto_or_intrinsic_width),
        PercentageBasis::definite(layout_pt(containing_block.width())),
    );
    let width = definite_width.map_or_else(
        || {
            if style.box_values.width.is_auto()
                && (!style.display.is_table()
                    || style.justify_self.keyword == SelfAlignmentKeyword::Stretch)
            {
                AbsoluteAxisSize::Automatic {
                    fit_content: shrink_to_fit_width,
                }
            } else {
                AbsoluteAxisSize::Definite(shrink_to_fit_width)
            }
        },
        AbsoluteAxisSize::Definite,
    );
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
    let margin_start = if style.box_values.margin.left.is_auto() {
        AbsoluteAxisMargin::Auto
    } else {
        AbsoluteAxisMargin::Length(layout_pt(style.margin.left))
    };
    let margin_end = if style.box_values.margin.right.is_auto() {
        AbsoluteAxisMargin::Auto
    } else {
        AbsoluteAxisMargin::Length(layout_pt(style.margin.right))
    };
    let non_content = non_content_pt(horizontal_non_content);
    let fill_between = |start: LayoutLength, end: LayoutLength| {
        content_box_pt(
            (containing_block.width()
                - start.points()
                - margin_start.used().points()
                - non_content.points()
                - margin_end.used().points()
                - end.points())
            .max(0.0),
        )
    };
    let start_for_end = |content_size: ContentBoxLength, end: LayoutLength| {
        layout_pt(
            containing_block.width()
                - end.points()
                - margin_start.used().points()
                - margin_end.used().points()
                - non_content.points()
                - content_size.points(),
        )
    };

    match (left, width, right) {
        (Some(start), AbsoluteAxisSize::Definite(size), Some(end)) => match containing_direction {
            Direction::Ltr => resolve_absolute_definite_axis_auto_margins(
                AbsoluteDefiniteAxis {
                    start,
                    size,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: layout_pt(containing_block.width()),
                },
                AbsoluteAxisDirection::HorizontalLtr,
            ),
            Direction::Rtl => resolve_absolute_definite_axis_auto_margins(
                AbsoluteDefiniteAxis {
                    start,
                    size,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: layout_pt(containing_block.width()),
                },
                AbsoluteAxisDirection::HorizontalRtl,
            ),
        },
        (Some(start), AbsoluteAxisSize::Definite(size), None) => {
            PositionedAxis::new(start, size, margin_start.used(), margin_end.used())
        }
        (Some(start), AbsoluteAxisSize::Automatic { .. }, Some(end)) => PositionedAxis::new(
            start,
            constrain_content_width(
                style,
                fill_between(start, end),
                PercentageBasis::definite(layout_pt(containing_block.width())),
            ),
            margin_start.used(),
            margin_end.used(),
        ),
        (Some(start), AbsoluteAxisSize::Automatic { fit_content }, None) => {
            PositionedAxis::new(start, fit_content, margin_start.used(), margin_end.used())
        }
        (None, AbsoluteAxisSize::Definite(size), Some(end)) => PositionedAxis::new(
            start_for_end(size, end),
            size,
            margin_start.used(),
            margin_end.used(),
        ),
        (None, AbsoluteAxisSize::Definite(size), None) => match containing_direction {
            Direction::Ltr => PositionedAxis::new(
                layout_pt(static_left),
                size,
                margin_start.used(),
                margin_end.used(),
            ),
            Direction::Rtl => PositionedAxis::new(
                start_for_end(size, layout_pt(static_right)),
                size,
                margin_start.used(),
                margin_end.used(),
            ),
        },
        (None, AbsoluteAxisSize::Automatic { fit_content }, Some(end)) => PositionedAxis::new(
            start_for_end(fit_content, end),
            fit_content,
            margin_start.used(),
            margin_end.used(),
        ),
        (None, AbsoluteAxisSize::Automatic { fit_content }, None) => match containing_direction {
            Direction::Ltr => PositionedAxis::new(
                layout_pt(static_left),
                fit_content,
                margin_start.used(),
                margin_end.used(),
            ),
            Direction::Rtl => PositionedAxis::new(
                start_for_end(fit_content, layout_pt(static_right)),
                fit_content,
                margin_start.used(),
                margin_end.used(),
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
    measured_height: ContentBoxLength,
    automatic_minimum_height: Option<f32>,
    static_start: f32,
    vertical_border_width: f32,
) -> PositionedAxis {
    // CSS 2.1 10.6.4, non-replaced absolutely positioned elements. Static
    // position is approximated from the layout cursor at the element's source
    // position until layout carries explicit placeholders.
    let top = used_inset_top(style, containing_block).map(layout_pt);
    let bottom = used_inset_bottom(style, containing_block).map(layout_pt);
    let vertical_non_content =
        non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width);
    let percentage_basis = PercentageBasis::definite(content_box_pt(containing_block.height()));
    let definite_height = used_content_box_height_or_auto(
        style,
        layout_pt(containing_block.height()),
        vertical_non_content,
    );
    let unresolved_preferred_height = match style.box_values.height.value() {
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => calc_size_intrinsic_constraint(
            value.clone(),
            style.box_sizing,
            percentage_basis,
            vertical_non_content,
            measured_height,
            measured_height,
        )
        .unwrap_or(measured_height),
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => measured_height,
        css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => {
            definite_height.expect("directly resolvable positioned height has a used value")
        }
    };
    let constrain_height = |height: ContentBoxLength| {
        let height = constrain_height_with_intrinsic(
            style,
            height,
            measured_height,
            measured_height,
            percentage_basis,
            vertical_non_content,
        );
        automatic_minimum_height.map_or(height, |minimum| {
            content_box_pt(height.points().max(minimum))
        })
    };
    let height = definite_height.map_or_else(
        || {
            let measured = constrain_height(unresolved_preferred_height);
            if style.box_values.height.is_auto()
                && (!style.display.is_table()
                    || style.align_self.keyword == SelfAlignmentKeyword::Stretch)
            {
                AbsoluteAxisSize::Automatic {
                    fit_content: measured,
                }
            } else {
                AbsoluteAxisSize::Definite(measured)
            }
        },
        |height| AbsoluteAxisSize::Definite(constrain_height(height)),
    );
    // CSS 2.2 defines the static position as the hypothetical normal-flow
    // position. It can fall outside the containing block, especially while a
    // nested formatting context is measured in temporary coordinates.
    let margin_start = if style.box_values.margin.top.is_auto() {
        AbsoluteAxisMargin::Auto
    } else {
        AbsoluteAxisMargin::Length(layout_pt(style.margin.top))
    };
    let margin_end = if style.box_values.margin.bottom.is_auto() {
        AbsoluteAxisMargin::Auto
    } else {
        AbsoluteAxisMargin::Length(layout_pt(style.margin.bottom))
    };
    let fill_between = |start: LayoutLength, end: LayoutLength| {
        content_box_pt(
            (containing_block.height()
                - start.points()
                - margin_start.used().points()
                - vertical_non_content.points()
                - margin_end.used().points()
                - end.points())
            .max(0.0),
        )
    };
    let start_for_end = |content_size: ContentBoxLength, end: LayoutLength| {
        layout_pt(
            containing_block.height()
                - end.points()
                - margin_start.used().points()
                - margin_end.used().points()
                - vertical_non_content.points()
                - content_size.points(),
        )
    };

    match (top, height, bottom) {
        (Some(start), AbsoluteAxisSize::Definite(size), Some(end)) => {
            resolve_absolute_definite_axis_auto_margins(
                AbsoluteDefiniteAxis {
                    start,
                    size,
                    end,
                    margin_start,
                    margin_end,
                    non_content: vertical_non_content,
                    containing_size: layout_pt(containing_block.height()),
                },
                AbsoluteAxisDirection::Vertical,
            )
        }
        (Some(start), AbsoluteAxisSize::Definite(size), None) => {
            PositionedAxis::new(start, size, margin_start.used(), margin_end.used())
        }
        (Some(start), AbsoluteAxisSize::Automatic { .. }, Some(end)) => PositionedAxis::new(
            start,
            constrain_height(fill_between(start, end)),
            margin_start.used(),
            margin_end.used(),
        ),
        (Some(start), AbsoluteAxisSize::Automatic { fit_content }, None) => {
            PositionedAxis::new(start, fit_content, margin_start.used(), margin_end.used())
        }
        (None, AbsoluteAxisSize::Definite(size), Some(end)) => PositionedAxis::new(
            start_for_end(size, end),
            size,
            margin_start.used(),
            margin_end.used(),
        ),
        (None, AbsoluteAxisSize::Definite(size), None) => PositionedAxis::new(
            layout_pt(static_start),
            size,
            margin_start.used(),
            margin_end.used(),
        ),
        (None, AbsoluteAxisSize::Automatic { fit_content }, Some(end)) => PositionedAxis::new(
            start_for_end(fit_content, end),
            fit_content,
            margin_start.used(),
            margin_end.used(),
        ),
        (None, AbsoluteAxisSize::Automatic { fit_content }, None) => PositionedAxis::new(
            layout_pt(static_start),
            fit_content,
            margin_start.used(),
            margin_end.used(),
        ),
    }
}
