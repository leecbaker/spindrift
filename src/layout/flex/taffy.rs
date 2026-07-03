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
/// alignment, and CSS Box Model permits negative margins to shift and overlap
/// boxes:
/// <https://www.w3.org/TR/css-flexbox-1/#box-model> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(super) fn taffy_margin(
    style: &ComputedStyle,
) -> taffy_layout::Rect<taffy_layout::LengthPercentageAuto> {
    let edges = style.box_values.margin;
    taffy_layout::Rect {
        left: taffy_margin_length_percentage_auto(edges.left),
        right: taffy_margin_length_percentage_auto(edges.right),
        top: taffy_margin_length_percentage_auto(edges.top),
        bottom: taffy_margin_length_percentage_auto(edges.bottom),
    }
}

/// Converts CSS flex item margins without applying size non-negativity clamps.
///
/// CSS Flexbox lays out and paints flex items in order-modified document order,
/// so negative margins must survive the Taffy bridge in order for overlapping
/// flex items to paint in the correct `order` sequence:
/// <https://www.w3.org/TR/css-flexbox-1/#order-property>.
fn taffy_margin_length_percentage_auto(
    value: css::ComputedLengthPercentageOrAuto,
) -> taffy_layout::LengthPercentageAuto {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::LengthPercentageAuto::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent != 0.0 && value.length_is_zero() {
                taffy_layout::LengthPercentageAuto::percent(value.percent)
            } else {
                taffy_layout::LengthPercentageAuto::length(value.length_points())
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => {
            taffy_layout::LengthPercentageAuto::auto()
        }
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
/// resolved. When `flex-basis` is not `auto`, it is used in place of the
/// main-size property for flex base sizing, so the authored main-size property
/// must not be supplied to Taffy as a known main-axis size:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-stretch> and
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
pub(super) fn flex_item_size_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    fallback: ContentBoxLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    context: FlexItemSizeDimensionContext,
) -> taffy_layout::Dimension {
    if context
        .flex_direction
        .shares_axis_with(context.dimension_axis)
    {
        if context.flex_basis_overrides_main_size {
            return taffy_layout::Dimension::auto();
        }
        match value {
            css::ComputedLengthPercentageOrAuto::Auto => {
                taffy_layout::Dimension::length(fallback.points().max(1.0))
            }
            _ => taffy_intrinsic_dimension_with_basis_and_stretch(
                value,
                context.percentage_basis,
                min_content,
                max_content,
                context.stretch,
            ),
        }
    } else {
        match value {
            css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
            _ => taffy_intrinsic_dimension_with_basis_and_stretch(
                value,
                context.percentage_basis,
                min_content,
                max_content,
                context.stretch,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemSizeDimensionContext {
    pub(super) flex_direction: FlexDirection,
    pub(super) dimension_axis: FlexDirection,
    pub(super) percentage_basis: Option<f32>,
    pub(super) stretch: FlexStretchFitContext,
    pub(super) flex_basis_overrides_main_size: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexStretchFitContext {
    pub(super) available_margin_box_size: Option<LayoutLength>,
    pub(super) margin_size: NonContentLength,
    pub(super) non_content_size: NonContentLength,
    pub(super) box_sizing: BoxSizing,
}

fn taffy_stretch_fit_dimension(context: FlexStretchFitContext) -> taffy_layout::Dimension {
    let Some(available) = context.available_margin_box_size else {
        return taffy_layout::Dimension::auto();
    };
    let content_size = stretch_fit_content_box_size(
        available.points(),
        context.margin_size.points(),
        context.non_content_size,
    );
    let size = match context.box_sizing {
        BoxSizing::ContentBox => content_size.points(),
        BoxSizing::BorderBox => {
            content_box_to_border_box_length(content_size, context.non_content_size).points()
        }
    };
    taffy_layout::Dimension::length(size.max(0.0))
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
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if (value.math.is_some() || (value.percent != 0.0 && !value.length_is_zero()))
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
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> taffy_layout::Dimension {
    taffy_intrinsic_dimension_with_basis_and_stretch(
        value,
        percentage_basis,
        min_content,
        max_content,
        FlexStretchFitContext {
            available_margin_box_size: None,
            margin_size: non_content_pt(0.0),
            non_content_size: non_content_pt(0.0),
            box_sizing: BoxSizing::ContentBox,
        },
    )
}

pub(super) fn taffy_intrinsic_dimension_with_basis_and_stretch(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    stretch: FlexStretchFitContext,
) -> taffy_layout::Dimension {
    let min_content = min_content.points().max(0.0);
    let max_content = max_content.points().max(min_content);
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::Stretch => taffy_stretch_fit_dimension(stretch),
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
                        Some(value.length_points_max_zero())
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
        width: content_box_pt(0.0),
        height: content_box_pt(0.0),
        min_width: content_box_pt(0.0),
        min_height: content_box_pt(0.0),
        content_width: content_box_pt(0.0),
        content_height: content_box_pt(0.0),
        preferred_aspect_ratio: None,
        first_baseline: None,
        last_baseline: None,
        first_horizontal_baseline: None,
        last_horizontal_baseline: None,
    });
    let preferred_aspect_ratio = estimate.preferred_aspect_ratio.filter(|ratio| *ratio > 0.0);
    let measured_width = known_dimensions
        .width
        .or_else(|| {
            preferred_aspect_ratio
                .and_then(|ratio| known_dimensions.height.map(|height| height * ratio))
        })
        .unwrap_or_else(|| estimate.width.points())
        .max(0.0);
    let measured_height = known_dimensions
        .height
        .or_else(|| {
            preferred_aspect_ratio
                .and_then(|ratio| known_dimensions.width.map(|width| width / ratio))
        })
        .unwrap_or_else(|| estimate.height.points())
        .max(0.0);
    taffy_layout::Size {
        width: measured_width,
        height: measured_height,
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
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => taffy_layout::Dimension::auto(),
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
    if value.percent != 0.0 && value.length_is_zero() {
        taffy_layout::Dimension::percent(value.percent.max(0.0))
    } else {
        taffy_layout::Dimension::length(value.length_points_max_zero())
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
/// CSS Flexbox section 4.5 defines the automatic minimum size of flex items as
/// a content-based minimum size. For flex items with a preferred aspect ratio,
/// that content-based minimum combines the content size suggestion and the
/// transferred size suggestion from CSS Sizing:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>,
/// <https://www.w3.org/TR/css-flexbox-1/#content-based-minimum-size>, and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(super) fn flex_min_size_dimension(
    specified: css::ComputedLengthPercentageOrAuto,
    estimated_min_content: ContentBoxLength,
    estimated_max_content: ContentBoxLength,
    context: FlexMinSizeDimensionContext,
) -> taffy_layout::Dimension {
    if !specified.is_auto() {
        return taffy_intrinsic_dimension_with_basis_and_stretch(
            specified,
            context.percentage_basis,
            estimated_min_content,
            estimated_max_content,
            context.stretch,
        );
    }
    if context.is_main_axis {
        if context.overflow.is_scrollable() {
            taffy_layout::Dimension::length(0.0)
        } else {
            // CSS Flexbox 4.5: non-scrollable flex items use the content-based
            // minimum size in the main axis, capped by a definite preferred main
            // size. Cross-axis auto minimums remain automatic.
            let mut automatic_minimum = estimated_min_content.points().max(0.0);
            if let Some(transferred) = context.transferred_size_suggestion {
                let transferred = transferred.points().max(0.0);
                automatic_minimum = if context.is_replaced {
                    automatic_minimum.min(transferred)
                } else {
                    automatic_minimum.max(transferred)
                };
            }
            if let Some(preferred_size) = context.definite_preferred_content_size {
                automatic_minimum = automatic_minimum.min(preferred_size.points().max(0.0));
            }
            taffy_layout::Dimension::length(automatic_minimum)
        }
    } else {
        taffy_layout::Dimension::auto()
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexMinSizeDimensionContext {
    pub(super) definite_preferred_content_size: Option<ContentBoxLength>,
    pub(super) transferred_size_suggestion: Option<ContentBoxLength>,
    pub(super) is_replaced: bool,
    pub(super) is_main_axis: bool,
    pub(super) overflow: css::Overflow,
    pub(super) percentage_basis: Option<f32>,
    pub(super) stretch: FlexStretchFitContext,
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
    context: FlexBasisContext,
) -> taffy_layout::Dimension {
    match style.flex_basis {
        css::ComputedFlexBasis::LengthPercentage(length) => {
            if length.has_percentage && !context.main_size_is_definite {
                return taffy_layout::Dimension::length(flex_auto_content_basis(
                    style,
                    if context.direction.is_row_axis() {
                        estimate.content_width
                    } else {
                        estimate.content_height
                    },
                    context.direction,
                ));
            }
            return taffy_optional_dimension_with_basis(
                css::ComputedLengthPercentageOrAuto::LengthPercentage(length.value),
                context
                    .main_size_is_definite
                    .then_some(context.available_main_size),
            );
        }
        css::ComputedFlexBasis::Content | css::ComputedFlexBasis::MaxContent => {
            return taffy_layout::Dimension::length(flex_auto_content_basis(
                style,
                if context.direction.is_row_axis() {
                    estimate.content_width
                } else {
                    estimate.content_height
                },
                context.direction,
            ));
        }
        css::ComputedFlexBasis::MinContent => {
            return taffy_layout::Dimension::length(flex_auto_content_basis(
                style,
                if context.direction.is_row_axis() {
                    estimate.min_width
                } else {
                    estimate.min_height
                },
                context.direction,
            ));
        }
        css::ComputedFlexBasis::FitContent(limit) => {
            let min_content = if context.direction.is_row_axis() {
                estimate.min_width
            } else {
                estimate.min_height
            };
            let max_content = if context.direction.is_row_axis() {
                estimate.content_width
            } else {
                estimate.content_height
            };
            let limit = limit
                .map(|limit| used_length_percentage(limit, context.available_main_size))
                .unwrap_or(context.available_main_size);
            return taffy_layout::Dimension::length(flex_auto_content_basis(
                style,
                fit_content_basis(min_content, max_content, limit),
                context.direction,
            ));
        }
        css::ComputedFlexBasis::Auto => {}
    }

    // CSS Flexbox 7.2.3: `flex-basis:auto` retrieves the main-size property,
    // and if that is also auto the used flex basis is `content`. CSS Flexbox
    // 9.2 transfers a preferred aspect ratio through a definite cross size
    // before falling back to content sizing.
    if context.direction.is_row_axis() {
        if !style.box_values.width.is_auto() {
            taffy_flex_basis_from_main_size(
                style,
                style.box_values.width,
                estimate,
                context.available_main_size,
                context.main_size_is_definite,
                FlexDirection::Row,
            )
        } else if let Some(transferred) = aspect_ratio_transferred_flex_basis(
            style,
            context.direction,
            context.available_cross_size,
            context.stretched_cross_size,
            context.preferred_aspect_ratio,
        ) {
            taffy_layout::Dimension::length(transferred.points())
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
            context.available_main_size,
            context.main_size_is_definite,
            FlexDirection::Column,
        )
    } else if let Some(transferred) = aspect_ratio_transferred_flex_basis(
        style,
        context.direction,
        context.available_cross_size,
        context.stretched_cross_size,
        context.preferred_aspect_ratio,
    ) {
        taffy_layout::Dimension::length(transferred.points())
    } else {
        taffy_layout::Dimension::length(flex_auto_content_basis(
            style,
            estimate.content_height,
            FlexDirection::Column,
        ))
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexBasisContext {
    pub(super) direction: FlexDirection,
    pub(super) available_main_size: f32,
    pub(super) available_cross_size: Option<f32>,
    pub(super) stretched_cross_size: Option<f32>,
    pub(super) main_size_is_definite: bool,
    pub(super) preferred_aspect_ratio: Option<f32>,
}

/// Computes the flex base-size transfer from a definite cross size.
///
/// Flexbox section 9.2 lets a flex item with a preferred aspect ratio use a
/// definite cross size to resolve its flex base size before falling back to
/// content sizing:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
fn aspect_ratio_transferred_flex_basis(
    style: &ComputedStyle,
    direction: FlexDirection,
    available_cross_size: Option<f32>,
    stretched_cross_size: Option<f32>,
    preferred_aspect_ratio: Option<f32>,
) -> Option<LayoutLength> {
    aspect_ratio_transferred_content_main_size(
        style,
        direction,
        available_cross_size,
        stretched_cross_size,
        preferred_aspect_ratio,
    )
    .map(|size| flex_auto_content_basis_from_content_box(style, size, direction))
}

/// Computes the content-box transferred size suggestion for a flex item's main axis.
///
/// CSS Flexbox 4.5 and 9.2 both use CSS Sizing preferred aspect ratios to
/// transfer a definite cross size into a main-axis content size. The flex basis
/// adds border/padding separately, but automatic minimum size calculations use
/// this content-box suggestion directly:
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(super) fn aspect_ratio_transferred_content_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    available_cross_size: Option<f32>,
    stretched_cross_size: Option<f32>,
    preferred_aspect_ratio: Option<f32>,
) -> Option<ContentBoxLength> {
    let ratio = preferred_aspect_ratio?;
    if direction.is_row_axis() {
        let cross_non_content =
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style));
        let cross_content_height = used_content_height_or_auto_with_optional_basis(
            style,
            available_cross_size,
            cross_non_content.points(),
        )
        .or_else(|| {
            stretched_cross_size.map(|size| (size - cross_non_content.points()).max(0.0))
        })?;
        Some(flex_aspect_ratio_transferred_content_main_size(
            content_box_pt(cross_content_height),
            direction,
            ratio,
        ))
    } else {
        let cross_non_content = non_content_pt(
            style.padding.left + style.padding.right + horizontal_border_width(style),
        );
        let cross_content_width = used_content_width_or_auto_with_optional_basis(
            style,
            available_cross_size,
            cross_non_content.points(),
        )
        .or_else(|| {
            stretched_cross_size.map(|size| (size - cross_non_content.points()).max(0.0))
        })?;
        Some(flex_aspect_ratio_transferred_content_main_size(
            content_box_pt(cross_content_width),
            direction,
            ratio,
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
fn fit_content_basis(
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    limit: f32,
) -> ContentBoxLength {
    content_box_pt(
        max_content
            .points()
            .max(0.0)
            .min(min_content.points().max(0.0).max(limit.max(0.0))),
    )
}

fn flex_aspect_ratio_transferred_content_main_size(
    cross_content_size: ContentBoxLength,
    direction: FlexDirection,
    ratio: f32,
) -> ContentBoxLength {
    if direction.is_row_axis() {
        content_box_pt(cross_content_size.points() * ratio)
    } else {
        content_box_pt(cross_content_size.points() / ratio)
    }
}

fn main_axis_extras(style: &ComputedStyle, direction: FlexDirection) -> NonContentLength {
    let border_widths = used_border_widths(style);
    non_content_pt(if direction.is_row_axis() {
        style.padding.left + style.padding.right + border_widths.left + border_widths.right
    } else {
        style.padding.top + style.padding.bottom + border_widths.top + border_widths.bottom
    })
}

/// Computes the content-derived basis used when `flex-basis:auto` has no main size.
///
/// CSS Flexbox defines content-based flex basis resolution for `auto`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
pub(super) fn flex_auto_content_basis(
    style: &ComputedStyle,
    length: ContentBoxLength,
    direction: FlexDirection,
) -> f32 {
    flex_auto_content_basis_from_content_box(style, length, direction).points()
}

fn flex_auto_content_basis_from_content_box(
    style: &ComputedStyle,
    length: ContentBoxLength,
    direction: FlexDirection,
) -> LayoutLength {
    // The CSS value is the content size. The intrinsic estimator and line
    // breaker both shape text, but through different APIs; round up so a tiny
    // metric disagreement does not create an avoidable flex-item wrap in
    // preserved-newline content such as `white-space: pre-line` address blocks.
    let length = content_box_pt(if style.white_space.preserves_newlines() {
        length.points().max(0.0).ceil() + style.font_size.ceil()
    } else {
        length.points().max(0.0)
    });
    if style.box_sizing == BoxSizing::BorderBox {
        layout_pt(
            content_box_to_border_box_length(length, main_axis_extras(style, direction)).points(),
        )
    } else {
        layout_pt(length.points())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_margin_adapter_preserves_negative_lengths_and_percentages() {
        let length = taffy_margin_length_percentage_auto(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(-50.0),
            ),
        );
        assert_eq!(length.resolve_to_option(200.0, |_, _| 0.0), Some(-50.0));

        let percentage = taffy_margin_length_percentage_auto(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(-0.25),
            ),
        );
        assert_eq!(percentage.resolve_to_option(200.0, |_, _| 0.0), Some(-50.0));

        let auto = taffy_margin_length_percentage_auto(css::ComputedLengthPercentageOrAuto::Auto);
        assert!(auto.is_auto());
    }

    #[test]
    fn flex_size_adapter_still_clamps_negative_lengths_and_percentages() {
        assert_eq!(
            taffy_dimension_from_length_percentage(css::ComputedLengthPercentage::from_points(
                -50.0
            )),
            taffy_layout::Dimension::length(0.0)
        );
        assert_eq!(
            taffy_dimension_from_length_percentage(css::ComputedLengthPercentage::from_percent(
                -0.25
            )),
            taffy_layout::Dimension::percent(0.0)
        );
    }

    fn test_flex_basis_context() -> FlexBasisContext {
        FlexBasisContext {
            direction: FlexDirection::Row,
            available_main_size: 200.0,
            available_cross_size: None,
            stretched_cross_size: None,
            main_size_is_definite: true,
            preferred_aspect_ratio: None,
        }
    }

    fn test_flex_estimate() -> FlexItemEstimate {
        FlexItemEstimate {
            width: content_box_pt(60.0),
            height: content_box_pt(30.0),
            min_width: content_box_pt(20.0),
            min_height: content_box_pt(10.0),
            content_width: content_box_pt(80.0),
            content_height: content_box_pt(40.0),
            preferred_aspect_ratio: None,
            first_baseline: None,
            last_baseline: None,
            first_horizontal_baseline: None,
            last_horizontal_baseline: None,
        }
    }

    #[test]
    fn flex_item_measurement_extracts_typed_content_box_lengths() {
        let mut estimate = test_flex_estimate();

        let measured = measure_flex_item(
            taffy_layout::Size {
                width: None,
                height: None,
            },
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::Definite(200.0),
                height: taffy_layout::AvailableSpace::Definite(200.0),
            },
            Some(&mut estimate),
        );

        assert_eq!(measured.width, 60.0);
        assert_eq!(measured.height, 30.0);
    }

    #[test]
    fn flex_basis_uses_typed_content_and_min_content_estimates() {
        let estimate = test_flex_estimate();
        let mut style = ComputedStyle::initial();

        style.flex_basis = css::ComputedFlexBasis::Content;
        assert_eq!(
            taffy_flex_basis(&style, &estimate, test_flex_basis_context()),
            taffy_layout::Dimension::length(80.0)
        );

        style.flex_basis = css::ComputedFlexBasis::MinContent;
        assert_eq!(
            taffy_flex_basis(&style, &estimate, test_flex_basis_context()),
            taffy_layout::Dimension::length(20.0)
        );

        style.flex_basis = css::ComputedFlexBasis::MaxContent;
        assert_eq!(
            taffy_flex_basis(&style, &estimate, test_flex_basis_context()),
            taffy_layout::Dimension::length(80.0)
        );
    }

    #[test]
    fn flex_basis_indefinite_percentage_falls_back_to_typed_content_estimate() {
        let estimate = test_flex_estimate();
        let mut style = ComputedStyle::initial();
        style.flex_basis =
            css::ComputedFlexBasis::LengthPercentage(css::ComputedFlexBasisLength::new(
                css::ComputedLengthPercentage::from_percent(0.5),
                true,
            ));
        let context = FlexBasisContext {
            main_size_is_definite: false,
            ..test_flex_basis_context()
        };

        assert_eq!(
            taffy_flex_basis(&style, &estimate, context),
            taffy_layout::Dimension::length(80.0)
        );
    }

    #[test]
    fn content_box_aspect_ratio_transfer_keeps_content_box_flex_basis() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::ContentBox;
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(150.0),
        );
        style.padding.left = 75.0;
        style.padding.right = 75.0;

        let basis = aspect_ratio_transferred_flex_basis(
            &style,
            FlexDirection::Column,
            Some(300.0),
            None,
            Some(1.0),
        )
        .expect("definite cross size should transfer through aspect ratio");

        assert_eq!(basis.points(), 150.0);
    }

    #[test]
    fn border_box_aspect_ratio_transfer_adds_extras_once() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(300.0),
        );
        style.padding.left = 75.0;
        style.padding.right = 75.0;
        style.padding.top = 75.0;
        style.padding.bottom = 75.0;

        let basis = aspect_ratio_transferred_flex_basis(
            &style,
            FlexDirection::Column,
            Some(300.0),
            None,
            Some(1.0),
        )
        .expect("definite cross size should transfer through aspect ratio");

        assert_eq!(basis.points(), 300.0);
    }
}
