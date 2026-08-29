use super::*;
use crate::layout::flex::estimate::constrain_flex_item_estimated_height;
use crate::units::{IntoLayoutLength, content_box_to_border_box_length};

/// Applies Flexbox's automatic minimum main size to final item layouts.
///
/// CSS Flexbox section 4.5 defines `min-width:auto`/`min-height:auto` on flex
/// items as a content-based automatic minimum in the main axis when overflow is
/// non-scrollable. Taffy remains the primary flex algorithm here, but this guard
/// preserves content and transferred size suggestions when a definite zero-sized
/// flex container would otherwise shrink the final item layout below its
/// automatic minimum:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion>.
pub(in crate::layout::flex) fn apply_main_axis_automatic_minimums(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> bool {
    let mut changed = false;
    let axes = PhysicalFlexDirection::new(physical_direction);
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        let Some(minimum) = automatic_minimum_main_size(
            child,
            estimate,
            container_style,
            physical_direction,
            available,
        ) else {
            continue;
        };
        let current = item.main_size(axes);
        if current >= minimum {
            continue;
        }
        if matches!(
            physical_direction,
            FlexDirection::RowReverse | FlexDirection::ColumnReverse
        ) {
            item.set_main_start(axes, item.main_start(axes) - (minimum - current));
        }
        item.set_main_size(axes, minimum);
        changed = true;
    }
    changed
}

/// Ensures final flex item border boxes can contain their non-content edges.
///
/// CSS Sizing floors the content box at zero, including stretch-fit sizing
/// where a small target margin box can be smaller than the item's padding and
/// border. Taffy may report a zero final border-box cross size for these cases,
/// so Quire restores the minimum border-box size before painting/replay:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-stretch>.
pub(in crate::layout::flex) fn apply_non_negative_flex_item_content_box_minimums(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
) -> bool {
    let mut changed = false;
    for (item, child) in items.iter_mut().zip(children) {
        let borders = used_border_widths(&child.style);
        let min_width =
            child.style.padding.left + child.style.padding.right + borders.left + borders.right;
        if item.width() < FlexPhysicalHorizontalSize::new(min_width) {
            item.set_width(FlexPhysicalHorizontalSize::new(min_width));
            changed = true;
        }

        let min_height =
            child.style.padding.top + child.style.padding.bottom + borders.top + borders.bottom;
        if item.height() < FlexPhysicalVerticalSize::new(min_height) {
            item.set_height(FlexPhysicalVerticalSize::new(min_height));
            changed = true;
        }
    }
    changed
}

pub(in crate::layout::flex) fn expand_flex_line_cross_bounds_for_item_overflow(
    lines: &mut [FlexLineLayout],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) {
    for line in lines {
        for &index in &line.item_indices {
            let Some(item) = items.get(index) else {
                continue;
            };
            let Some(child) = children.get(index) else {
                continue;
            };
            let (cross_start, cross_end) =
                item_outer_cross_bounds(item, &child.style, physical_direction);
            line.cross_start = line.cross_start.min(cross_start);
            line.cross_end = line.cross_end.max(cross_end);
        }
    }
}

/// Returns the physical available size to use while estimating a flex item's
/// descendants for flex base sizing.
///
/// CSS Flexbox treats a stretched flex item's cross size as definite for
/// laying out descendants when computing the flex base size, provided the flex
/// container has a definite cross size:
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
pub(in crate::layout::flex) fn flex_item_estimate_available_space(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> FlexItemAvailableSpace {
    let mut item_available = FlexItemAvailableSpace::from_container(available);
    // A column flex item's authored physical width is its cross size. When it
    // is a non-percentage length, its contents lay out against that definite
    // width while the automatic main-size flex basis is measured. Keep this
    // item-local descendant constraint separate from the container percentage
    // basis used to resolve percentage-valued widths:
    // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item> and
    // <https://www.w3.org/TR/css-sizing-3/#definite>.
    let horizontal_non_content =
        child_style.padding.left + child_style.padding.right + horizontal_border_width(child_style);
    if physical_direction.is_column_axis()
        && child_style
            .box_values
            .width
            .length_if_no_percent()
            .is_some()
        && let Some(width) = used_content_box_width_or_auto_with_basis(
            child_style,
            available.width_basis,
            non_content_pt(horizontal_non_content),
        )
    {
        item_available.set_definite_width(
            PhysicalContentWidth::new(width),
            FlexAvailableSizeSource::DefinitePreferredCrossSize,
        );
    }
    // A specified physical height is a definite percentage basis for the
    // item's descendants regardless of whether it happens to be flex's main
    // or cross axis. In particular, a row flex item's `height` must resolve a
    // child's percentage height while its automatic minimum is measured.
    // Column items additionally fall back to a definite flex base size when
    // their preferred main height is automatic. Do not replace a row item's
    // physical width: its own percentage-valued `width` still resolves
    // against the flex container rather than its flex basis.
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>.
    let vertical_non_content =
        child_style.padding.top + child_style.padding.bottom + vertical_border_width(child_style);
    let preferred_height = used_content_box_height_or_auto_with_basis(
        child_style,
        available.height_basis,
        non_content_pt(vertical_non_content),
    );
    let definite_height = preferred_height
        .map(|height| {
            (
                height,
                if physical_direction.is_column_axis() {
                    FlexAvailableSizeSource::DefinitePreferredMainSize
                } else {
                    FlexAvailableSizeSource::DefinitePreferredCrossSize
                },
            )
        })
        .or_else(|| {
            physical_direction
                .is_column_axis()
                .then(|| {
                    definite_post_flexing_main_size(child_style, physical_direction, available).map(
                        |height| {
                            (
                                content_box_pt(height.points()),
                                FlexAvailableSizeSource::DefiniteFlexBase,
                            )
                        },
                    )
                })
                .flatten()
        });
    if let Some((height, source)) = definite_height {
        item_available.set_definite_height(PhysicalContentHeight::new(height), source);
    }
    let Some(premeasure_cross_size) = flex_item_premeasure_stretched_cross_size(
        child_style,
        container_style,
        physical_direction,
        available,
    ) else {
        return item_available;
    };
    let stretched_cross_size = premeasure_cross_size.size();

    item_available.set_definite_cross_size(
        physical_direction,
        stretched_cross_size,
        premeasure_cross_size.available_size_source(),
    );
    item_available.set_stretched_cross_size(physical_direction, stretched_cross_size);
    item_available
}

pub(in crate::layout::flex) fn stretched_flex_item_cross_size(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexCrossSize> {
    if !matches!(
        effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    ) || flex_item_has_auto_cross_margin(child_style, physical_direction)
    {
        return None;
    }

    if physical_direction.is_row_axis() {
        if !child_style.box_values.height.is_auto() {
            return None;
        }
        let container_cross_size =
            balanced_flex_cross_measurement_size(container_style, physical_direction, available)?;
        Some(
            (container_cross_size
                - FlexCrossLength::new(child_style.margin.top + child_style.margin.bottom))
            .non_negative_size(),
        )
    } else {
        if !child_style.box_values.width.is_auto() {
            return None;
        }
        let container_cross_size =
            balanced_flex_cross_measurement_size(container_style, physical_direction, available)?;
        Some(
            (container_cross_size
                - FlexCrossLength::new(child_style.margin.left + child_style.margin.right))
            .non_negative_size(),
        )
    }
}

/// Return a stretch cross size that is known before flex-base calculation.
///
/// An explicit balanced line slot is known early, as is the cross size of a
/// definite single-line container. Other stretch sizes belong to final replay
/// and must not feed an item's own content-based flex base back into line
/// formation.
/// <https://drafts.csswg.org/css-flexbox/#algo-main-item>
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes>
pub(super) fn flex_item_premeasure_stretched_cross_size(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexPremeasureCrossSize> {
    if !matches!(
        effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    ) || flex_item_has_auto_cross_margin(child_style, physical_direction)
    {
        return None;
    }

    let container_cross_size = if container_style.flex_wrap.balances_lines()
        && container_style.flex_line_count.get() > 1
    {
        balanced_flex_cross_measurement_size(container_style, physical_direction, available)
            .map(FlexPremeasureCrossSize::BalancedLineSlot)
    } else if !container_style.flex_wrap.wraps() {
        let size = if physical_direction.is_row_axis() {
            flex_cross_size_from_content_box(available.height_basis_content_box_length()?)
        } else {
            flex_cross_size_from_content_box(available.width_basis_content_box_length()?)
        };
        Some(FlexPremeasureCrossSize::DefiniteSingleLineContainer(size))
    } else {
        None
    }?;

    let item_cross_size = if physical_direction.is_row_axis() {
        if !child_style.box_values.height.is_auto() {
            return None;
        }
        let stretch_size = (container_cross_size.size()
            - FlexCrossLength::new(child_style.margin.top + child_style.margin.bottom))
        .non_negative_size();
        // Flexbox clamps the stretched used cross size by the item's min/max
        // cross constraints before using it as the definite cross size for
        // flex-base measurement.
        // <https://drafts.csswg.org/css-flexbox/#algo-cross-item>
        flex_cross_size_from_content_box(constrain_flex_item_estimated_height(
            child_style,
            flex_cross_content_box_length(stretch_size),
            flex_cross_content_box_length(stretch_size),
            flex_cross_content_box_length(stretch_size),
            available.height_basis,
            non_content_pt(
                child_style.padding.top
                    + child_style.padding.bottom
                    + vertical_border_width(child_style),
            ),
        ))
    } else {
        if !child_style.box_values.width.is_auto() {
            return None;
        }
        let stretch_size = (container_cross_size.size()
            - FlexCrossLength::new(child_style.margin.left + child_style.margin.right))
        .non_negative_size();
        flex_cross_size_from_content_box(constrain_content_width(
            child_style,
            flex_cross_content_box_length(stretch_size),
            available.width_basis,
        ))
    };
    Some(match container_cross_size {
        FlexPremeasureCrossSize::BalancedLineSlot(_) => {
            FlexPremeasureCrossSize::BalancedLineSlot(item_cross_size)
        }
        FlexPremeasureCrossSize::DefiniteSingleLineContainer(_) => {
            FlexPremeasureCrossSize::DefiniteSingleLineContainer(item_cross_size)
        }
    })
}

/// Select the physical cross-size constraint that flex-item measurement may
/// consume for stretching.
///
/// A balanced container with an explicit line count reserves one equal
/// cross-axis slot for each planned line before item measurement.  That slot
/// is a layout constraint, whereas the unmodified container content box
/// remains the percentage basis for percentage-valued item properties.  Do
/// not recover the slot from `FlexAvailableSpace::cross_basis`: doing so
/// would discard the balanced constraint and make every item measure against
/// the whole container again.
/// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
fn balanced_flex_cross_measurement_size(
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexCrossSize> {
    let has_explicit_balanced_line_count =
        container_style.flex_wrap.balances_lines() && container_style.flex_line_count.get() > 1;
    if has_explicit_balanced_line_count {
        return if physical_direction.is_row_axis() {
            available
                .height_constraint()
                .map(|height| FlexCrossSize::new(height.points()))
        } else {
            Some(FlexCrossSize::new(available.width.points()))
        };
    }
    available.definite_cross_size(physical_direction)
}
fn definite_flex_basis_main_size(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexMainSize> {
    let css::ComputedFlexBasis::LengthPercentage(ref length) = style.flex_basis else {
        return None;
    };
    used_length_percentage_or_auto_with_basis(
        css::ComputedLengthPercentageOrAuto::LengthPercentage(length.value.clone()),
        available.main_basis(physical_direction),
    )
    .map(flex_main_size_from_layout_extent)
}

/// Returns a flex item's main size when Flexbox makes its post-flexing size
/// definite independently of the container's main size.
///
/// A definite `flex-basis` qualifies directly. `flex-basis:auto` instead
/// retrieves the preferred main size, so an explicit definite main-size also
/// qualifies; `flex-basis:content` deliberately does not retrieve that size:
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes> and
/// <https://drafts.csswg.org/css-flexbox/#flex-basis-property>.
pub(super) fn definite_post_flexing_main_size(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexMainSize> {
    definite_flex_basis_main_size(style, physical_direction, available).or_else(|| {
        if !matches!(style.flex_basis, css::ComputedFlexBasis::Auto) {
            return None;
        }
        if physical_direction.is_row_axis() {
            used_content_box_width_or_auto_with_basis(
                style,
                available.width_basis,
                non_content_pt(
                    style.padding.left + style.padding.right + horizontal_border_width(style),
                ),
            )
            .map(flex_main_size_from_content_box)
        } else {
            used_content_box_height_or_auto_with_basis(
                style,
                available.height_basis,
                non_content_pt(
                    style.padding.top + style.padding.bottom + vertical_border_width(style),
                ),
            )
            .map(flex_main_size_from_content_box)
        }
    })
}

/// Resolves the content-box automatic minimum main size of a flex item.
///
/// CSS Flexbox computes automatic minimum sizes from the content-based minimum
/// size for non-scrollable overflow. A preferred aspect ratio can transfer a
/// definite cross size into that minimum; non-replaced items use the larger of
/// the content and transferred suggestions, while replaced items use the smaller:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion>.
///
/// This exposes the shared content-box result consumed by both intrinsic
/// contribution sizing and final flex layout. Callers apply their own outer
/// box-model conversion at the boundary where it is required.
pub(in crate::layout::flex) fn automatic_minimum_main_content_size(
    child: &StyledChild<'_>,
    estimate: &FlexItemEstimate,
    container_style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<ContentBoxLength> {
    let child_style = &child.style;
    // The post-layout guard must use the same definite stretched cross size as
    // Taffy's primary flex calculation. Otherwise an automatic minimum of a
    // replaced item can be recomputed from its specified main size alone and
    // overwrite the smaller content-size suggestion transferred from a
    // definite cross size.
    // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
    let item_available =
        flex_item_estimate_available_space(child_style, container_style, direction, available);
    let stretched_cross_size = if direction.is_row_axis() {
        item_available
            .stretched_height
            .map(PhysicalContentHeight::content_box_length)
            .map(flex_cross_size_from_content_box)
    } else {
        item_available
            .stretched_width
            .map(PhysicalContentWidth::content_box_length)
            .map(flex_cross_size_from_content_box)
    };
    let child_padding = flex_item_used_padding(child_style, container_style, available);
    let child_margin = flex_item_used_margin(child_style, container_style, available);
    let child_borders = used_border_widths(child_style);
    let horizontal_stretch = FlexStretchFitContext {
        available_margin_box_size: available
            .width_basis_content_box_length()
            .map(IntoLayoutLength::into_layout_length),
        margin_size: layout_pt(child_margin.left + child_margin.right),
        non_content_size: non_content_pt(
            child_padding.left + child_padding.right + child_borders.left + child_borders.right,
        ),
        box_sizing: child_style.box_sizing,
    };
    let vertical_stretch = FlexStretchFitContext {
        available_margin_box_size: available
            .height_basis_content_box_length()
            .map(IntoLayoutLength::into_layout_length),
        margin_size: layout_pt(child_margin.top + child_margin.bottom),
        non_content_size: non_content_pt(
            child_padding.top + child_padding.bottom + child_borders.top + child_borders.bottom,
        ),
        box_sizing: child_style.box_sizing,
    };
    let (specified_min, percentage_basis, stretch, cross_stretch) = if direction.is_row_axis() {
        (
            child_style.box_values.min_width.clone(),
            available.width_basis,
            horizontal_stretch,
            vertical_stretch,
        )
    } else {
        (
            child_style.box_values.min_height.clone(),
            available.height_basis,
            vertical_stretch,
            horizontal_stretch,
        )
    };
    resolve_automatic_flex_minimum(
        specified_min,
        FlexMinSizeDimensionContext {
            style: child_style,
            direction,
            automatic_minimum_inputs: estimate.automatic_main_minimum_inputs,
            available_cross_size: if direction.is_row_axis() {
                available
                    .height_basis_content_box_length()
                    .map(flex_cross_size_from_content_box)
            } else {
                available
                    .width_basis_content_box_length()
                    .map(flex_cross_size_from_content_box)
            },
            cross_stretch,
            stretched_cross_size,
            is_main_axis: true,
            overflow: flex_item_main_axis_overflow(child_style, direction),
            percentage_basis,
            stretch,
        },
    )
    .map(|minimum| minimum.used_content_box)
}

/// Resolves the automatic minimum border-box main size of a final flex item.
///
/// Taffy's final item rectangle is border-box geometry, while the shared
/// automatic-minimum resolver produces content-box geometry. Keep that
/// conversion at this final-layout boundary; intrinsic contributions instead
/// add their signed outer edges directly.
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>.
pub(in crate::layout::flex) fn automatic_minimum_main_size(
    child: &StyledChild<'_>,
    estimate: &FlexItemEstimate,
    container_style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexMainSize> {
    let child_style = &child.style;
    let minimum = automatic_minimum_main_content_size(
        child,
        estimate,
        container_style,
        direction,
        available,
    )?;
    // Taffy's item layout is a border-box size, while all content, specified,
    // and transferred size suggestions above are content-box sizes. Convert at
    // this boundary so the post-layout safeguard does not compare unlike box
    // model spaces and accidentally permit a flex item to shrink through its
    // automatic minimum by its padding or borders:
    // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
    // <https://www.w3.org/TR/css-sizing-3/#box-model>.
    let non_content = if direction.is_row_axis() {
        child_style.padding.left + child_style.padding.right + horizontal_border_width(child_style)
    } else {
        child_style.padding.top + child_style.padding.bottom + vertical_border_width(child_style)
    };
    Some(flex_main_size_from_layout_extent(
        content_box_to_border_box_length(minimum, non_content_pt(non_content)).into_layout_length(),
    ))
}
