use super::*;

pub(in crate::layout::flex) fn flex_container_content_width_from_intrinsic(
    style: &ComputedStyle,
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
    intrinsic: FlexItemEstimate,
    shrink_auto_width: bool,
) -> PhysicalContentWidth {
    let min_content = PhysicalContentWidth::new(intrinsic.min_width);
    let max_content = flex_container_shrink_to_fit_max_content_width(
        style,
        available_outer_width,
        horizontal_non_content,
        min_content,
        PhysicalContentWidth::new(content_box_pt(
            intrinsic.width.points().max(min_content.points()).max(0.0),
        )),
        shrink_auto_width,
    );
    let auto_width = if shrink_auto_width {
        intrinsic::IntrinsicAutoWidth::ShrinkToFit
    } else {
        intrinsic::IntrinsicAutoWidth::FillAvailable
    };
    PhysicalContentWidth::new(intrinsic::content_box_width_from_intrinsic(
        style,
        available_outer_width,
        horizontal_non_content,
        min_content.content_box_length(),
        max_content.content_box_length(),
        auto_width,
    ))
}

/// Return the max-content width used by auto-width flex shrink-to-fit sizing.
///
/// CSS Flexbox defines multi-line column cross-size contributions separately
/// from normal block intrinsic widths. Floated and atomic flex containers then
/// feed those contributions into CSS 2.2 shrink-to-fit width resolution. An
/// explicit balanced line count is different: it fixes the requested set of
/// flex lines, so the cross size must include every balanced line instead of
/// collapsing to the single-line min-content contribution:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes> and
/// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property> and
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
pub(in crate::layout::flex) fn flex_container_shrink_to_fit_max_content_width(
    style: &ComputedStyle,
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
    min_content: PhysicalContentWidth,
    max_content: PhysicalContentWidth,
    shrink_auto_width: bool,
) -> PhysicalContentWidth {
    if !shrink_auto_width
        || !style.box_values.width.is_auto()
        || style.flex_wrap == FlexWrap::NoWrap
        || !physical_flex_direction(style).is_column_axis()
        || (style.flex_wrap.balances_lines() && style.flex_line_count.get() > 1)
    {
        return max_content;
    }

    let available_content_width =
        (available_outer_width.points() - horizontal_non_content.points()).max(0.0);
    if available_content_width > max_content.points() + 0.01 {
        min_content
    } else {
        max_content
    }
}

/// Returns whether a block flex container's auto physical width needs intrinsic sizing.
///
/// CSS Writing Modes sizes orthogonal flow roots with the fit-content rule
/// rather than stretching the block axis to the containing block's physical
/// width. For a vertical-writing flex container in horizontal flow, that means
/// `width:auto` must shrink-wrap the flex cross size while `height` remains
/// the container's logical inline/main size:
/// <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
pub(in crate::layout::flex) fn orthogonal_auto_width_flex_container_needs_intrinsic(
    style: &ComputedStyle,
    containing_space: ChildAvailableSpace,
) -> bool {
    style.box_values.width.is_auto()
        && matches!(
            (containing_space.writing_mode, style.writing_mode),
            (
                WritingMode::HorizontalTb,
                WritingMode::VerticalRl
                    | WritingMode::VerticalLr
                    | WritingMode::SidewaysRl
                    | WritingMode::SidewaysLr
            ) | (
                WritingMode::VerticalRl
                    | WritingMode::VerticalLr
                    | WritingMode::SidewaysRl
                    | WritingMode::SidewaysLr,
                WritingMode::HorizontalTb
            )
        )
}

/// <https://www.w3.org/TR/css-flexbox-1/#algo-line-break>.
pub(in crate::layout::flex) fn flex_available_content_height(
    style: &ComputedStyle,
    definite_content_height: Option<ContentBoxLength>,
    percentage_basis: BlockSizePercentageBasis,
) -> Option<ContentBoxLength> {
    if definite_content_height.is_some() || style.flex_wrap == FlexWrap::NoWrap {
        return definite_content_height;
    }
    if !physical_flex_direction(style).is_column_axis() {
        return definite_content_height;
    }
    used_max_height(style, percentage_basis)
}

/// Projects a block-level flex container's automatic logical inline size into
/// the physical-height input consumed by the Flex adapter.
///
/// In orthogonal writing modes, an automatic inline size can fill a *definite*
/// containing-block inline span. That span is physical height, not an
/// automatic physical block size. Orthogonal fallback space deliberately does
/// not appear here: it selects a fit-content measure but does not establish a
/// used physical height for the box.
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
/// <https://www.w3.org/TR/CSS2/visudet.html#blockwidth>
pub(in crate::layout::flex) fn orthogonal_block_flex_auto_inline_content_height(
    style: &ComputedStyle,
    participates_in_normal_flow: bool,
    containing_block_height: PercentageBasis<PhysicalContentHeight>,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    participates_in_normal_flow
        .then_some(())
        .filter(|_| {
            WritingModeAxes::new(style.writing_mode, style.used_direction()).swaps_physical_axes()
        })
        .and_then(|_| style.box_values.height.is_auto().then_some(()))
        .zip(containing_block_height.value())
        .map(|(_, containing_block_height)| {
            content_box_pt(
                (containing_block_height.points() - vertical_non_content.points()).max(0.0),
            )
        })
}

/// Resolve a flex container's definite content height.
///
/// CSS Flexbox treats a flex container's post-flexing main size as definite,
/// and CSS Sizing lets a preferred aspect ratio transfer a definite width into
/// an automatic height. That ratio-derived height must therefore be visible to
/// flex item cross-size resolution:
/// <https://www.w3.org/TR/css-flexbox-1/#definite-sizes> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(in crate::layout::flex) fn definite_flex_container_content_height(
    style: &ComputedStyle,
    explicit_content_height: Option<ContentBoxLength>,
    content_width: ContentBoxLength,
    percentage_basis: BlockSizePercentageBasis,
    horizontal_non_content: NonContentLength,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    if explicit_content_height.is_some() || !style.box_values.height.is_auto() {
        return explicit_content_height;
    }

    let ratio =
        ResolvedAspectRatio::for_non_replaced(style, horizontal_non_content, vertical_non_content)?;
    let content_height = ratio.height_from_width(content_width);
    Some(constrain_content_height(
        style,
        content_height,
        percentage_basis,
    ))
}

/// Select the used block size of an automatic flex container after measuring
/// its content-based automatic minimum independently of its ratio-derived
/// preferred size.
///
/// CSS Sizing 4 floors a ratio-dependent automatic block size by the
/// content-based automatic minimum on non-scroll containers, then applies the
/// effective maximum. An explicit minimum has already replaced `auto` and is
/// handled by the ordinary constraint calculation:
/// <https://drafts.csswg.org/css-sizing-4/#aspect-ratio-minimum>.
pub(in crate::layout::flex) fn select_ratio_derived_flex_container_height(
    style: &ComputedStyle,
    preferred_height: ContentBoxLength,
    automatic_minimum: ContentBoxLength,
    percentage_basis: BlockSizePercentageBasis,
) -> ContentBoxLength {
    let tentative = if style.box_values.min_height.is_auto() && !style.overflow_y.is_scrollable() {
        content_box_pt(preferred_height.points().max(automatic_minimum.points()))
    } else {
        preferred_height
    };
    constrain_content_height(style, tentative, percentage_basis)
}
