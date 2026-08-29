use super::*;

pub(in crate::layout) fn inline_text_from_formatting_boxes(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> String {
    let mut text = String::new();
    collect_inline_text_from_formatting_boxes(child_boxes, &mut text);
    text
}

pub(in crate::layout) fn formatting_box_has_inline_content(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| match child {
        _ if box_tree::is_out_of_flow_box(child) => true,
        box_tree::FormattingBox::Text(box_) => {
            inline_text_has_non_phantom_content(&box_.text, &box_.style)
        }
        box_tree::FormattingBox::Inline(box_) => {
            // Inline boxes with generated pseudo content must keep the rich
            // inline collector active even when their DOM text is empty. CSS
            // 2.2 also routes floated inline boxes through inline collection so
            // they can be blockified and placed as floats. Empty inline boxes
            // with owned inline-axis margin, border, or padding still generate
            // inline boxes whose decorations and advance must be preserved:
            // <https://www.w3.org/TR/CSS22/box.html#inline-boxes>.
            box_.core
                .style
                .before_style
                .as_deref()
                .is_some_and(|style| style.content.is_generated())
                || box_
                    .core
                    .style
                    .after_style
                    .as_deref()
                    .is_some_and(|style| style.content.is_generated())
                || box_.core.style.content.is_generated()
                || box_.core.style.float != Float::None
                || inline_box_fragment_has_owned_inline_edge(&box_.core.style, box_.fragment_edges)
                || formatting_box_has_inline_content(&box_.core.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            formatting_box_has_inline_content(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(_) => false,
        box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_) => true,
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_) => false,
    })
}

/// Return whether text contributes a line box after CSS white-space trimming.
///
/// A run made solely of collapsible space at an otherwise empty block edge
/// does not create a line box. Keeping that distinction here lets margin
/// adjacency use the same content test as inline collection without hiding
/// preserved whitespace or non-space inline content.
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
pub(in crate::layout) fn inline_text_has_non_phantom_content(
    text: &str,
    style: &ComputedStyle,
) -> bool {
    let text = normalized_text_for_style(text, style);
    if style.white_space.collapses_spaces() {
        !crate::text::trim_css_collapsible_whitespace(&text).is_empty()
    } else {
        !text.is_empty()
    }
}

fn inline_box_fragment_has_owned_inline_edge(
    style: &ComputedStyle,
    fragment_edges: box_tree::InlineBoxFragmentEdges,
) -> bool {
    (fragment_edges.owns_start && inline_box_logical_edge_has_nonzero_component(style, true))
        || (fragment_edges.owns_end && inline_box_logical_edge_has_nonzero_component(style, false))
}

fn inline_box_logical_edge_has_nonzero_component(style: &ComputedStyle, is_start: bool) -> bool {
    let side = if is_start {
        inline_start_side(style.writing_mode, style.used_direction())
    } else {
        inline_end_side(style.writing_mode, style.used_direction())
    };
    let borders = used_border_widths(style);
    let (margin, border, padding) = match side {
        PhysicalSide::Top => (style.margin.top, borders.top, style.padding.top),
        PhysicalSide::Right => (style.margin.right, borders.right, style.padding.right),
        PhysicalSide::Bottom => (style.margin.bottom, borders.bottom, style.padding.bottom),
        PhysicalSide::Left => (style.margin.left, borders.left, style.padding.left),
    };
    margin.abs() > 0.001 || border.abs() > 0.001 || padding.abs() > 0.001
}

pub(in crate::layout) fn collect_inline_text_from_formatting_boxes(
    child_boxes: &[box_tree::FormattingBox<'_>],
    output: &mut String,
) {
    for child in child_boxes {
        match child {
            box_tree::FormattingBox::Text(box_) => output.push_str(&box_.text),
            box_tree::FormattingBox::Inline(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.core.children, output);
            }
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.core.children, output);
            }
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.children, output);
            }
            box_tree::FormattingBox::AtomicInline(box_) => {
                collect_inline_text_from_formatting_boxes(&box_.core.children, output);
            }
            box_tree::FormattingBox::Block(_)
            | box_tree::FormattingBox::Table(_)
            | box_tree::FormattingBox::Flex(_)
            | box_tree::FormattingBox::Replaced(_) => {}
        }
    }
}
