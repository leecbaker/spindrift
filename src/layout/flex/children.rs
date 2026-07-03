use super::*;

pub(super) fn flex_children_from_boxes<'a>(
    container_element: &'a Element,
    container_signature: &ElementSignature,
    container_style: &ComputedStyle,
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> Vec<StyledChild<'a>> {
    flex_child_lists_from_boxes(
        container_element,
        container_signature,
        container_style,
        child_boxes,
    )
    .0
}

/// Splits normalized child boxes into flex items and out-of-flow positioned boxes.
///
/// CSS Positioned Layout makes absolutely positioned boxes out-of-flow, and
/// CSS Flexbox says they do not participate in flex item layout:
/// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
pub(super) fn flex_child_lists_from_boxes<'a>(
    _container_element: &'a Element,
    _container_signature: &ElementSignature,
    _container_style: &ComputedStyle,
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> (Vec<StyledChild<'a>>, Vec<StyledChild<'a>>) {
    itemize_blockified_children(
        child_boxes,
        ItemizationOptions {
            anonymous_item_tag: "__reasy_anonymous_flex_item",
            strip_blockified_inline_text_paint: true,
            establish_independent_formatting_context: true,
        },
    )
}
