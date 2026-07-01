use super::*;

/// Wraps a raw Taffy layout result in the Taffy coordinate space.
///
/// Taffy returns physical x/y coordinates after Quire has mapped CSS flex axes
/// and writing direction into Taffy's row/column model. The returned rect must
/// be converted to container coordinates before storage in flex layout data:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
pub(super) fn taffy_rect_from_layout(layout: &taffy_layout::Layout) -> TaffyRect {
    TaffyRect::new(
        TaffyPoint::new(layout.location.x, layout.location.y),
        TaffySize::new(layout.size.width, layout.size.height),
    )
}

/// Converts computed CSS margins to Taffy's flex-item margin representation.
///
/// CSS Flexible Box Layout uses margin boxes during flex item sizing and
/// alignment:
/// <https://www.w3.org/TR/css-flexbox-1/#box-model>.
pub(super) fn taffy_margin(
    style: &ComputedStyle,
) -> taffy_layout::Rect<taffy_layout::LengthPercentageAuto> {
    let edges = style.box_values.margin;
    taffy_layout::Rect {
        left: taffy_length_percentage_auto(edges.left),
        right: taffy_length_percentage_auto(edges.right),
        top: taffy_length_percentage_auto(edges.top),
        bottom: taffy_length_percentage_auto(edges.bottom),
    }
}

/// Converts computed CSS padding to Taffy's flex-item padding representation.
///
/// CSS Flexible Box Layout sizes flex items using their box model, and CSS
/// Box Model defines padding edge behavior:
/// <https://www.w3.org/TR/css-flexbox-1/#box-model> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(super) fn taffy_padding(
    style: &ComputedStyle,
) -> taffy_layout::Rect<taffy_layout::LengthPercentage> {
    let edges = style.box_values.padding;
    taffy_layout::Rect {
        left: taffy_length_percentage(edges.left),
        right: taffy_length_percentage(edges.right),
        top: taffy_length_percentage(edges.top),
        bottom: taffy_length_percentage(edges.bottom),
    }
}

/// Converts used border widths to Taffy's length-only edge representation.
///
/// CSS Flexible Box Layout includes borders in flex item sizing through the
/// CSS box model:
/// <https://www.w3.org/TR/css-flexbox-1/#box-model>.
pub(super) fn taffy_edges(edges: css::Edges) -> taffy_layout::Rect<taffy_layout::LengthPercentage> {
    taffy_layout::Rect {
        left: taffy_layout::LengthPercentage::length(edges.left),
        right: taffy_layout::LengthPercentage::length(edges.right),
        top: taffy_layout::LengthPercentage::length(edges.top),
        bottom: taffy_layout::LengthPercentage::length(edges.bottom),
    }
}

/// Converts computed CSS gaps to Taffy's flex gap representation.
///
/// CSS Box Alignment defines `normal` gaps as zero in flex layout and
/// percentages as layout-time values:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(super) fn taffy_gap(value: css::ComputedGap) -> taffy_layout::LengthPercentage {
    match value {
        css::ComputedGap::Normal => taffy_layout::LengthPercentage::length(0.0),
        css::ComputedGap::LengthPercentage(value) => taffy_length_percentage(value),
    }
}

/// Converts a flex item's physical size for Taffy while preserving auto cross sizes.
///
/// CSS Flexbox resolves `auto` main sizes through content sizing for the flex
/// base size, but `align-items: stretch` and `align-content: stretch` require
/// the flex item's cross-size property to remain automatic until flex lines are
/// resolved:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-stretch>.
pub(super) fn flex_item_size_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    fallback: f32,
    min_content: f32,
    max_content: f32,
    flex_direction: FlexDirection,
    dimension_axis: FlexDirection,
    percentage_basis: Option<f32>,
) -> taffy_layout::Dimension {
    if flex_direction.shares_axis_with(dimension_axis) {
        match value {
            css::ComputedLengthPercentageOrAuto::Auto => {
                taffy_layout::Dimension::length(fallback.max(1.0))
            }
            _ => taffy_intrinsic_dimension_with_basis(
                value,
                percentage_basis,
                min_content,
                max_content,
            ),
        }
    } else {
        match value {
            css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
            _ => taffy_intrinsic_dimension_with_basis(
                value,
                percentage_basis,
                min_content,
                max_content,
            ),
        }
    }
}

/// Converts a CSS size to Taffy, resolving mixed length-percentages when possible.
///
/// CSS Values allows `<length-percentage>` math such as `calc(50% + 10pt)`.
/// Taffy 0.11 exposes length-only or percentage-only dimensions, so flex layout
/// resolves mixed values at this bridge when the relevant flex container axis
/// is definite:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
pub(super) fn taffy_optional_dimension_with_basis(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
) -> taffy_layout::Dimension {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if (value.math.is_some() || (value.percent != 0.0 && value.length != 0.0))
                && let Some(basis) = percentage_basis
            {
                return taffy_layout::Dimension::length(
                    used_length_percentage(value, basis).max(0.0),
                );
            }
            taffy_dimension_from_length_percentage(value)
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => taffy_layout::Dimension::auto(),
    }
}

/// Converts a CSS size constraint to Taffy when intrinsic contributions are known.
///
/// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
/// intrinsic size keywords. Flex layout has already estimated each flex item's
/// intrinsic contributions before building the Taffy tree, so min/max
/// constraints can be resolved here instead of being dropped:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
pub(super) fn taffy_intrinsic_dimension_with_basis(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
    min_content: f32,
    max_content: f32,
) -> taffy_layout::Dimension {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => {
            taffy_optional_dimension_with_basis(value, percentage_basis)
        }
        css::ComputedLengthPercentageOrAuto::MinContent => {
            taffy_layout::Dimension::length(min_content.max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::MaxContent => {
            taffy_layout::Dimension::length(max_content.max(min_content).max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .and_then(|value| {
                    if value.math.is_none() && value.percent == 0.0 {
                        Some(value.length.max(0.0))
                    } else {
                        percentage_basis.map(|basis| used_length_percentage(value, basis).max(0.0))
                    }
                })
                .unwrap_or_else(|| max_content.max(min_content).max(0.0));
            taffy_layout::Dimension::length(
                max_content
                    .max(min_content)
                    .min(stretch.max(min_content))
                    .max(0.0),
            )
        }
    }
}

/// Measures a leaf flex item for Taffy's layout algorithm from intrinsic estimates.
///
/// CSS Flexbox lays out each flex item to determine its flex base size and
/// hypothetical cross size, then later may override known dimensions during
/// line sizing and stretch alignment:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>.
pub(super) fn measure_flex_item(
    known_dimensions: taffy_layout::Size<Option<f32>>,
    _available_space: taffy_layout::Size<taffy_layout::AvailableSpace>,
    estimate: Option<&mut FlexItemEstimate>,
) -> taffy_layout::Size<f32> {
    let estimate = estimate.copied().unwrap_or(FlexItemEstimate {
        width: 0.0,
        height: 0.0,
        min_width: 0.0,
        min_height: 0.0,
        content_width: 0.0,
        content_height: 0.0,
        first_baseline: None,
        last_baseline: None,
        first_horizontal_baseline: None,
        last_horizontal_baseline: None,
    });
    taffy_layout::Size {
        width: known_dimensions.width.unwrap_or(estimate.width).max(0.0),
        height: known_dimensions.height.unwrap_or(estimate.height).max(0.0),
    }
}

/// Converts a CSS optional size to Taffy's `Dimension`.
///
/// CSS Values defines the `<length-percentage> | auto` shape used by flex item
/// width, height, and flex-basis:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
pub(super) fn taffy_optional_dimension(
    value: css::ComputedLengthPercentageOrAuto,
) -> taffy_layout::Dimension {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            taffy_dimension_from_length_percentage(value)
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => taffy_layout::Dimension::auto(),
    }
}

fn taffy_dimension_from_length_percentage(
    value: css::ComputedLengthPercentage,
) -> taffy_layout::Dimension {
    if value.percent != 0.0 && value.length == 0.0 {
        taffy_layout::Dimension::percent(value.percent.max(0.0))
    } else {
        taffy_layout::Dimension::length(value.length.max(0.0))
    }
}

/// Converts a CSS min-size value for a flex container root.
///
/// CSS Sizing defines the initial `min-width`/`min-height` as `auto`; for a
/// flex container's own used size, that automatic minimum does not become the
/// flex item automatic minimum from Flexbox 4.5, so the root minimum is zero
/// unless the author supplies a definite length/percentage:
/// <https://www.w3.org/TR/css-sizing-3/#min-size-properties> and
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
pub(super) fn taffy_min_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: f32,
) -> taffy_layout::Dimension {
    used_length_percentage_or_auto(value, percentage_basis)
        .map(taffy_layout::Dimension::length)
        .unwrap_or_else(|| taffy_layout::Dimension::length(0.0))
}

/// Computes Taffy's automatic minimum size for a flex item.
///
/// CSS Flexbox section 4.5 defines the automatic minimum size of flex items:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
pub(super) fn flex_min_size_dimension(
    specified: css::ComputedLengthPercentageOrAuto,
    estimated_min_content: f32,
    estimated_max_content: f32,
    definite_preferred_content_size: Option<f32>,
    is_main_axis: bool,
    overflow: css::Overflow,
    percentage_basis: Option<f32>,
) -> taffy_layout::Dimension {
    if !specified.is_auto() {
        return taffy_intrinsic_dimension_with_basis(
            specified,
            percentage_basis,
            estimated_min_content,
            estimated_max_content,
        );
    }
    if is_main_axis {
        if overflow.is_scrollable() {
            taffy_layout::Dimension::length(0.0)
        } else {
            // CSS Flexbox 4.5: the automatic minimum size of a flex item with
            // non-scroll overflow is its content-based minimum size in the main
            // axis, capped by any definite preferred main size. Cross-axis auto
            // minimums remain automatic.
            let mut automatic_minimum = estimated_min_content.max(0.0);
            if let Some(preferred_size) = definite_preferred_content_size {
                automatic_minimum = automatic_minimum.min(preferred_size.max(0.0));
            }
            taffy_layout::Dimension::length(automatic_minimum)
        }
    } else {
        taffy_layout::Dimension::auto()
    }
}

/// Returns the overflow value for the flex item's main axis.
///
/// CSS Flexbox resolves automatic minimum sizes on the flex main axis, and CSS
/// Overflow exposes independent inline/block overflow controls through
/// `overflow-x` and `overflow-y`:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>.
pub(super) fn flex_item_main_axis_overflow(
    style: &ComputedStyle,
    direction: FlexDirection,
) -> css::Overflow {
    if direction.is_row_axis() {
        style.overflow_x
    } else {
        style.overflow_y
    }
}

/// Computes the Taffy `flex-basis` dimension from CSS flex and main-size values.
///
/// CSS Flexbox defines `flex-basis:auto` as retrieving the main-size property
/// and falling back to content sizing. Percentages resolve against the flex
/// container's inner main size, and if that size is indefinite the used value
/// is content:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
pub(super) fn taffy_flex_basis(
    style: &ComputedStyle,
    estimate: &FlexItemEstimate,
    direction: FlexDirection,
    available_main_size: f32,
    main_size_is_definite: bool,
) -> taffy_layout::Dimension {
    match style.flex_basis {
        css::ComputedFlexBasis::LengthPercentage(value) => {
            if value.percent != 0.0 && !main_size_is_definite {
                return taffy_layout::Dimension::length(flex_auto_content_basis(
                    style,
                    if direction.is_row_axis() {
                        estimate.content_width
                    } else {
                        estimate.content_height
                    },
                    direction,
                ));
            }
            return taffy_optional_dimension_with_basis(
                css::ComputedLengthPercentageOrAuto::LengthPercentage(value),
                main_size_is_definite.then_some(available_main_size),
            );
        }
        css::ComputedFlexBasis::Content | css::ComputedFlexBasis::MaxContent => {
            return taffy_layout::Dimension::length(flex_auto_content_basis(
                style,
                if direction.is_row_axis() {
                    estimate.content_width
                } else {
                    estimate.content_height
                },
                direction,
            ));
        }
        css::ComputedFlexBasis::MinContent => {
            return taffy_layout::Dimension::length(flex_auto_content_basis(
                style,
                if direction.is_row_axis() {
                    estimate.min_width
                } else {
                    estimate.min_height
                },
                direction,
            ));
        }
        css::ComputedFlexBasis::FitContent(limit) => {
            let min_content = if direction.is_row_axis() {
                estimate.min_width
            } else {
                estimate.min_height
            };
            let max_content = if direction.is_row_axis() {
                estimate.content_width
            } else {
                estimate.content_height
            };
            let limit = limit
                .map(|limit| used_length_percentage(limit, available_main_size))
                .unwrap_or(available_main_size);
            return taffy_layout::Dimension::length(flex_auto_content_basis(
                style,
                fit_content_basis(min_content, max_content, limit),
                direction,
            ));
        }
        css::ComputedFlexBasis::Auto => {}
    }

    // CSS Flexbox 7.2.3: `flex-basis:auto` retrieves the main-size property,
    // and if that is also auto the used flex basis is `content`.
    if direction.is_row_axis() {
        if !style.box_values.width.is_auto() {
            taffy_flex_basis_from_main_size(
                style,
                style.box_values.width,
                estimate,
                available_main_size,
                main_size_is_definite,
                FlexDirection::Row,
            )
        } else {
            taffy_layout::Dimension::length(flex_auto_content_basis(
                style,
                estimate.content_width,
                FlexDirection::Row,
            ))
        }
    } else if !style.box_values.height.is_auto() {
        taffy_flex_basis_from_main_size(
            style,
            style.box_values.height,
            estimate,
            available_main_size,
            main_size_is_definite,
            FlexDirection::Column,
        )
    } else {
        taffy_layout::Dimension::length(flex_auto_content_basis(
            style,
            estimate.content_height,
            FlexDirection::Column,
        ))
    }
}

fn taffy_flex_basis_from_main_size(
    style: &ComputedStyle,
    value: css::ComputedLengthPercentageOrAuto,
    estimate: &FlexItemEstimate,
    available_main_size: f32,
    main_size_is_definite: bool,
    direction: FlexDirection,
) -> taffy_layout::Dimension {
    let (min_content, max_content) = if direction.is_row_axis() {
        (estimate.min_width, estimate.content_width)
    } else {
        (estimate.min_height, estimate.content_height)
    };
    if matches!(value, css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.percent != 0.0 && !main_size_is_definite)
    {
        return taffy_layout::Dimension::length(flex_auto_content_basis(
            style,
            max_content,
            direction,
        ));
    }

    taffy_intrinsic_dimension_with_basis(
        value,
        main_size_is_definite.then_some(available_main_size),
        min_content,
        max_content,
    )
}

/// Computes the intrinsic `fit-content` size clamp for `flex-basis`.
///
/// CSS Sizing defines fit-content as
/// `min(max-content, max(min-content, stretch-or-argument))`; Flexbox accepts
/// that width grammar for `flex-basis`:
/// <https://www.w3.org/TR/css-sizing-3/#fit-content-size> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
fn fit_content_basis(min_content: f32, max_content: f32, limit: f32) -> f32 {
    max_content
        .max(0.0)
        .min(min_content.max(0.0).max(limit.max(0.0)))
}

fn main_axis_extras(style: &ComputedStyle, direction: FlexDirection) -> f32 {
    let border_widths = used_border_widths(style);
    if direction.is_row_axis() {
        style.padding.left + style.padding.right + border_widths.left + border_widths.right
    } else {
        style.padding.top + style.padding.bottom + border_widths.top + border_widths.bottom
    }
}

/// Computes the content-derived basis used when `flex-basis:auto` has no main size.
///
/// CSS Flexbox defines content-based flex basis resolution for `auto`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
pub(super) fn flex_auto_content_basis(
    style: &ComputedStyle,
    length: f32,
    direction: FlexDirection,
) -> f32 {
    // The CSS value is the content size. The intrinsic estimator and line
    // breaker both shape text, but through different APIs; round up so a tiny
    // metric disagreement does not create an avoidable flex-item wrap in
    // preserved-newline content such as `white-space: pre-line` address blocks.
    let mut length = if style.white_space.preserves_newlines() {
        length.max(0.0).ceil() + style.font_size.ceil()
    } else {
        length.max(0.0)
    };
    if style.box_sizing == BoxSizing::BorderBox {
        length += main_axis_extras(style, direction);
    }
    length
}
