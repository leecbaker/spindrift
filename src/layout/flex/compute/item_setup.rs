use super::*;

/// The source children prepared for Flexbox's used-value and Taffy setup.
///
/// The durable box tree keeps computed styles. Flex owns the one-way used-edge
/// view needed for its item and line geometry, so this record keeps that
/// transient view separate from the source children used for replay.
pub(super) struct PreparedFlexItems<'a> {
    pub(super) children: Vec<StyledChild<'a>>,
}

/// Build Flexbox's item-input view before intrinsic measurement and Taffy
/// style construction.
///
/// CSS resolves every margin and padding percentage against the container's
/// logical inline size. The item setup boundary applies that used-edge view
/// once, while preserving the computed values needed by later percentage
/// provenance adapters.
/// <https://www.w3.org/TR/css-box-3/#padding-physical>
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
pub(super) fn prepare_flex_items<'a>(
    children: &[StyledChild<'a>],
    container_style: &ComputedStyle,
    available: FlexAvailableSpace,
) -> PreparedFlexItems<'a> {
    let sizing_children =
        flex_sizing_children_with_used_box_edges(children, container_style, available);
    PreparedFlexItems {
        children: sizing_children,
    }
}

pub(in crate::layout::flex) fn flex_sizing_children_with_used_box_edges<'a>(
    children: &[StyledChild<'a>],
    container_style: &ComputedStyle,
    available: FlexAvailableSpace,
) -> Vec<StyledChild<'a>> {
    let mut sizing_children = children.to_vec();
    let inline_percentage_basis = available.logical_inline_basis(container_style);
    for child in &mut sizing_children {
        apply_used_box_metrics_for_logical_inline_basis(&mut child.style, inline_percentage_basis);
    }
    sizing_children
}
