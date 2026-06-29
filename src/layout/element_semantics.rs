use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplacedElementKind {
    Canvas,
    Image,
    Svg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ElementLayoutKind {
    None,
    Positioned,
    Canvas,
    Image,
    GeneratedImage,
    Svg,
    Flex,
    Table,
    InlineFlow,
    BlockFlow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DefinitionListItemKind {
    Term,
    Description,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ListContainerKind {
    Ordered,
    Unordered,
    Other,
}

pub(super) fn element_layout_kind(element: &Element, style: &ComputedStyle) -> ElementLayoutKind {
    if style.display.is_none() {
        return ElementLayoutKind::None;
    }
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        return ElementLayoutKind::Positioned;
    }
    if matches!(style.content, Content::Replacement { .. }) {
        return ElementLayoutKind::GeneratedImage;
    }
    match replaced_element_kind(element) {
        Some(ReplacedElementKind::Canvas) => return ElementLayoutKind::Canvas,
        Some(ReplacedElementKind::Image) => return ElementLayoutKind::Image,
        Some(ReplacedElementKind::Svg) => return ElementLayoutKind::Svg,
        None => {}
    }
    if style.display.is_flex() {
        return ElementLayoutKind::Flex;
    }
    if style.display.is_table() || is_html_table_element(element) {
        return ElementLayoutKind::Table;
    }
    if style.display.is_inline_level() && style.display.is_flow() {
        ElementLayoutKind::InlineFlow
    } else {
        ElementLayoutKind::BlockFlow
    }
}

pub(super) fn replaced_element_kind(element: &Element) -> Option<ReplacedElementKind> {
    // CSS Display treats embedded document/media elements as replaced
    // elements. HTML defines `<canvas>` with intrinsic dimensions and CSS
    // Images/Sizing treats external images as replaced elements; this port also
    // treats the target's inline root `<svg>` snippets as replaced atoms until
    // full SVG layout integration exists.
    // https://www.w3.org/TR/css-display-3/#replaced-element
    // https://html.spec.whatwg.org/multipage/canvas.html#the-canvas-element
    match element.tag.as_str() {
        "canvas" => Some(ReplacedElementKind::Canvas),
        "img" => Some(ReplacedElementKind::Image),
        "svg" => Some(ReplacedElementKind::Svg),
        _ => None,
    }
}

pub(super) fn is_replaced_element(element: &Element) -> bool {
    replaced_element_kind(element).is_some()
}

pub(super) fn is_horizontal_rule_element(element: &Element) -> bool {
    // HTML `hr` is a thematic break, not a CSS replaced element. Keep this as
    // a semantic hook for void/childless box-tree construction; layout and
    // painting are ordinary CSS block behavior from the UA stylesheet.
    // https://html.spec.whatwg.org/multipage/grouping-content.html#the-hr-element
    element.tag == "hr"
}

pub(super) fn is_line_break_element(element: &Element) -> bool {
    // HTML `br` creates a forced line break in inline formatting.
    // https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element
    element.tag == "br"
}

pub(super) fn is_document_canvas_element(element: &Element) -> bool {
    matches!(element.tag.as_str(), "html" | "body")
}

pub(super) fn is_html_table_element(element: &Element) -> bool {
    element.tag == "table"
}

pub(super) fn is_table_or_replaced_element(element: &Element) -> bool {
    is_html_table_element(element) || is_replaced_element(element)
}

pub(super) fn suppresses_ordered_mixed_flow_detection(element: &Element) -> bool {
    // These elements either paint the document canvas, manage list markers, or
    // run table construction; the ordered mixed-flow fallback would duplicate
    // those formatting-context-specific layout paths.
    is_document_canvas_element(element)
        || matches!(
            list_container_kind(element),
            ListContainerKind::Ordered | ListContainerKind::Unordered
        )
        || is_html_table_element(element)
}

pub(super) fn is_html_table_caption_element(element: &Element) -> bool {
    element.tag == "caption"
}

pub(super) fn is_html_table_column_group_element(element: &Element) -> bool {
    element.tag == "colgroup"
}

pub(super) fn is_html_table_column_element(element: &Element) -> bool {
    element.tag == "col"
}

pub(super) fn is_html_table_row_group_element(element: &Element) -> bool {
    matches!(element.tag.as_str(), "thead" | "tbody" | "tfoot")
}

pub(super) fn is_html_table_header_group_element(element: &Element) -> bool {
    element.tag == "thead"
}

pub(super) fn is_html_table_footer_group_element(element: &Element) -> bool {
    element.tag == "tfoot"
}

pub(super) fn is_html_table_row_element(element: &Element) -> bool {
    element.tag == "tr"
}

pub(super) fn is_html_table_cell_element(element: &Element) -> bool {
    matches!(element.tag.as_str(), "td" | "th")
}

pub(super) fn definition_list_item_kind(element: &Element) -> DefinitionListItemKind {
    match element.tag.as_str() {
        "dt" => DefinitionListItemKind::Term,
        "dd" => DefinitionListItemKind::Description,
        _ => DefinitionListItemKind::Other,
    }
}

pub(super) fn is_definition_list_element(element: &Element) -> bool {
    element.tag == "dl"
}

pub(super) fn list_container_kind(element: &Element) -> ListContainerKind {
    match element.tag.as_str() {
        "ol" => ListContainerKind::Ordered,
        "ul" => ListContainerKind::Unordered,
        _ => ListContainerKind::Other,
    }
}

pub(super) fn is_list_item_element(element: &Element) -> bool {
    element.tag == "li"
}
