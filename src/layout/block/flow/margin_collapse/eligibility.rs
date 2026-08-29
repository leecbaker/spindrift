use super::*;

/// Returns whether a block container's start edge can adjoin child margins.
///
/// CSS 2.2 allows parent/child vertical margin collapse through ordinary flow
/// block boxes without border, padding, or inline content. A specified
/// `height` does not prevent the block-start margins from adjoining; it only
/// prevents a last child's block-end margin from collapsing through the
/// parent. A non-auto `min-height` similarly matters to the block-end case.
/// Layout and paint containment establish independent formatting contexts, so
/// their principal boxes never adjoin descendant margins.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
/// <https://www.w3.org/TR/css-display-3/#valdef-display-flow-root>
/// <https://www.w3.org/TR/css-contain-1/#containment-layout>
pub(in crate::layout) fn can_collapse_block_start_margin(
    element: &Element,
    style: &ComputedStyle,
    border_edges: UsedEdges,
    has_direct_inline_content: bool,
    used_overflow: css::Overflow,
) -> bool {
    style.display.is_flow()
        && !style.display.establishes_block_formatting_context()
        && !style_establishes_multicol_formatting_context(style)
        && !style_establishes_line_clamp_formatting_context(style)
        && !property_containment_establishes_independent_formatting_context(element, style)
        && style.float == Float::None
        && !has_direct_inline_content
        && used_overflow == css::Overflow::Visible
        && style.padding.top == 0.0
        && border_edges.top == layout_pt(0.0)
}

/// Returns whether a block container's end edge can adjoin child margins.
///
/// This is the block-end counterpart to `can_collapse_block_start_margin`. The
/// block layout pass later decides whether the final min-height-constrained
/// content height actually keeps this adjoining margin inside the parent.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
/// <https://www.w3.org/TR/css-display-3/#valdef-display-flow-root>
/// <https://www.w3.org/TR/css-contain-1/#containment-layout>
pub(in crate::layout) fn can_collapse_block_end_margin(
    element: &Element,
    style: &ComputedStyle,
    containing_block_height_basis: BlockSizePercentageBasis,
    border_edges: UsedEdges,
    has_direct_inline_content: bool,
    used_overflow: css::Overflow,
) -> bool {
    style.display.is_flow()
        && !style.display.establishes_block_formatting_context()
        && !style_establishes_multicol_formatting_context(style)
        && !style_establishes_line_clamp_formatting_context(style)
        && !property_containment_establishes_independent_formatting_context(element, style)
        && style.float == Float::None
        && !has_direct_inline_content
        && used_overflow == css::Overflow::Visible
        && style.padding.bottom == 0.0
        && border_edges.bottom == layout_pt(0.0)
        && height_behaves_as_auto_for_margin_collapse(style, containing_block_height_basis)
}

/// Returns whether a preferred physical height behaves as `auto` for CSS 2
/// margin-collapse eligibility.
///
/// CSS Sizing updates legacy CSS 2 conditions on computed `height: auto` to
/// include values that behave as if `auto` were specified.  Keep this
/// classification at the margin-collapse used-value boundary: the computed
/// value remains necessary for ordinary sizing, paint, and fragmentation.
///
/// <https://drafts.csswg.org/css-sizing-3/#behave-auto>
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn height_behaves_as_auto_for_margin_collapse(
    style: &ComputedStyle,
    containing_block_height_basis: BlockSizePercentageBasis,
) -> bool {
    match &*style.box_values.height {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => true,
        css::ComputedLengthPercentageOrAuto::Stretch => {
            matches!(containing_block_height_basis, PercentageBasis::Indefinite)
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            matches!(containing_block_height_basis, PercentageBasis::Indefinite)
                && value.needs_percentage_basis()
        }
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => {
            matches!(&value.basis, css::CalcSizeBasis::Auto)
        }
    }
}

/// Returns whether a block's own top and bottom margins may adjoin.
///
/// CSS 2.2 allows a block's own margins to be adjoining when it has no border,
/// padding, line boxes, min-height, or in-flow content separating the edges,
/// and its height is either `auto` or zero.
/// Formatting-context roots, including `flow-root`, and layout/paint-contained
/// boxes cannot be self-collapsing through contained descendants.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/css-contain-1/#containment-layout>
pub(in crate::layout) fn can_collapse_own_block_margins(
    element: &Element,
    style: &ComputedStyle,
    border_widths: css::Edges,
    has_direct_inline_content: bool,
    used_overflow: css::Overflow,
) -> bool {
    style.display.is_flow()
        && style.float == Float::None
        && !style_establishes_multicol_formatting_context(style)
        && !used_property_containment(element, style).establishes_independent_formatting_context()
        && !has_direct_inline_content
        && used_overflow == css::Overflow::Visible
        && style.padding.top == 0.0
        && style.padding.bottom == 0.0
        && border_widths.top == 0.0
        && border_widths.bottom == 0.0
        && height_is_auto_or_zero(style)
        && style.box_values.min_height.is_auto()
}

pub(in crate::layout) fn height_is_auto_or_zero(style: &ComputedStyle) -> bool {
    style.box_values.height.is_auto()
        || style
            .box_values
            .height
            .length_if_no_percent()
            .is_some_and(|height| height == 0.0)
}
