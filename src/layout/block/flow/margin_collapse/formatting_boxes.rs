use super::*;

/// Returns whether a block's own start and end margins are adjoining.
///
/// CSS 2.2 defines the margins of a box with no border, padding, min-height,
/// fixed height, line box, or in-flow non-self-collapsing children as
/// adjoining. Such a child keeps its parent's block-start collapsed margin set
/// open, which matters for `margin-trim: block-start`.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://drafts.csswg.org/css-box-4/#margin-trim>
pub(in crate::layout) fn is_self_collapsing_block_box(
    element: &Element,
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
    overflow_context: DocumentCanvasResolution,
) -> bool {
    // Generated content participates in the generated pseudo-element's box
    // just like an inline child, even though it is collected from `content`
    // during inline layout rather than stored in this formatting-box slice.
    // It therefore prevents the pseudo-element from being self-collapsing.
    // <https://drafts.csswg.org/css-pseudo-4/#generated-content>
    let has_line_box_content = has_direct_inline_content_box(child_boxes)
        || has_atomic_inline_formatting_box(child_boxes)
        || generated_content_has_non_phantom_inline_content(style)
        || super::dom_backed::style_has_non_phantom_generated_pseudo_content(style)
        || super::dom_backed::style_has_in_flow_marker_line(style);
    // Clearance changes the placement of an otherwise empty box without
    // creating an own block-size. Its margins therefore remain adjoining for
    // self-collapsing classification; the block-flow cursor independently
    // retains clearance for following in-flow content.
    // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
    is_collapsible_block_child(element, style)
        && can_collapse_own_block_margins(
            element,
            style,
            used_border_widths(style),
            has_line_box_content,
            overflow_context.used_overflow(element, style),
        )
        && !(style.display.establishes_block_formatting_context()
            && child_boxes.iter().any(|child| {
                formatting_box_is_in_normal_flow(child)
                    && !formatting_box_can_only_create_phantom_line_boxes(child)
            }))
        && child_boxes
            .iter()
            .all(|child| formatting_box_keeps_self_collapsing_parent(child, overflow_context))
}

pub(in crate::layout) fn self_collapsing_block_margin_set_for_box(
    style: &ComputedStyle,
    descendant_start_margin: Option<AdjoiningMarginSet>,
) -> AdjoiningMarginSet {
    let mut set = AdjoiningMarginSet::from_margin(layout_pt(style.margin.top));
    if let Some(descendant) = descendant_start_margin {
        set.merge(descendant);
    }
    set.include(layout_pt(style.margin.bottom));
    set
}

pub(in crate::layout) fn formatting_box_keeps_self_collapsing_parent(
    box_: &box_tree::FormattingBox<'_>,
    overflow_context: DocumentCanvasResolution,
) -> bool {
    if box_tree::is_out_of_flow_box(box_) || box_tree::is_floated_box(box_) {
        return true;
    }
    if formatting_box_can_only_create_phantom_line_boxes(box_) {
        return true;
    }
    // CSS 2.2 block-in-inline splitting materializes this transparent
    // context solely to retain the originating inline box's painting and
    // positioning state. Its block child still participates directly in the
    // enclosing block formatting context, including adjoining margin sets.
    // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
    // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    if let box_tree::FormattingBox::InlineSplitBlockContext(context) = box_ {
        return context
            .core
            .children
            .iter()
            .all(|child| formatting_box_keeps_self_collapsing_parent(child, overflow_context));
    }
    if let box_tree::FormattingBox::AnonymousBlock(box_) = box_ {
        return box_
            .children
            .iter()
            .all(|child| formatting_box_keeps_self_collapsing_parent(child, overflow_context));
    }
    let Some((element, _, style, children)) = box_.element_parts() else {
        return false;
    };
    is_normal_block_flow_child(element, style)
        && is_self_collapsing_block_box(element, style, children, overflow_context)
}

pub(in crate::layout) fn collapsible_first_child_start_margin_from_boxes(
    child_boxes: &[box_tree::FormattingBox<'_>],
    parent: &Element,
    parent_style: &ComputedStyle,
    overflow_context: DocumentCanvasResolution,
) -> Option<AdjoiningMarginSet> {
    let mut has_preceding_css_float = false;
    for child_box in child_boxes {
        // An inline split context is transparent to the parent block's
        // margin-collapse chain. Recurse into its generated block segment
        // instead of treating the originating inline wrapper as a separator.
        // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        if let box_tree::FormattingBox::InlineSplitBlockContext(context) = child_box {
            if let Some(margin) = collapsible_first_child_start_margin_from_boxes(
                &context.core.children,
                parent,
                parent_style,
                overflow_context,
            ) {
                return Some(margin);
            }
            continue;
        }
        let Some((child_element, _, child_style, child_children)) = child_box.element_parts()
        else {
            if matches!(
                child_box,
                box_tree::FormattingBox::Inline(_) | box_tree::FormattingBox::Text(_)
            ) && !formatting_box_can_only_create_phantom_line_boxes(child_box)
            {
                return None;
            }
            continue;
        };
        // Out-of-flow descendants do not participate in the parent's
        // adjoining-margin chain. In particular, the document-canvas
        // exception below must not make a floated first child look like the
        // first in-flow child and collapse its margin through the body.
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        if child_style.float != Float::None {
            has_preceding_css_float = true;
            continue;
        }
        if matches!(child_style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let is_flow_child = is_normal_block_flow_child(child_element, child_style)
            || overflow_context.is_document_canvas_flow_element(parent)
            || is_replaced_element(child_element);
        if !is_flow_child {
            if !inline_text(child_element).is_empty() {
                return None;
            }
            continue;
        }
        // A cleared first in-flow child after an adjoining float needs the
        // parent-start `clear:none` hypothesis rather than a pre-collapsed
        // used start margin. Other cleared descendants retain the ordinary
        // margin-collapse probe; block layout resolves whether they actually
        // introduce clearance in their containing float context.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        if child_style.clear != Clear::None && has_preceding_css_float {
            return None;
        }
        if parent_style.margin_trim.block_start {
            return Some(AdjoiningMarginSet::from_margin(layout_pt(0.0)));
        }
        return Some(collapsible_start_margin_for_box(
            child_element,
            child_style,
            child_children,
            overflow_context,
        ));
    }
    None
}

/// Return the complete adjoining start-margin set used for a box's CSS2
/// `clear:none` hypothetical position.
///
/// Unlike ordinary child placement, this probe must continue through a
/// self-collapsing first child and its following siblings. Floats and
/// positioned boxes are outside normal flow and do not close that adjoining
/// chain.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
pub(in crate::layout) fn clear_none_hypothetical_start_margin_from_boxes(
    element: &Element,
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
    overflow_context: DocumentCanvasResolution,
) -> Option<LayoutLength> {
    if style.margin_trim.block_start {
        return None;
    }
    let mut set = AdjoiningMarginSet::from_margin(layout_pt(style.margin.top));
    complete_adjoining_child_margin_set_from_boxes(&mut set, child_boxes, element, overflow_context)
        .found_adjoining_child()
        .then(|| set.collapsed())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdjoiningChildMarginScan {
    NoAdjoiningChild,
    Open,
    Closed,
}

impl AdjoiningChildMarginScan {
    fn found_adjoining_child(self) -> bool {
        self != Self::NoAdjoiningChild
    }
}

fn complete_adjoining_child_margin_set_from_boxes(
    set: &mut AdjoiningMarginSet,
    child_boxes: &[box_tree::FormattingBox<'_>],
    parent: &Element,
    overflow_context: DocumentCanvasResolution,
) -> AdjoiningChildMarginScan {
    let mut found_in_flow_child = false;
    let mut has_preceding_css_float = false;
    for child_box in child_boxes {
        if let box_tree::FormattingBox::InlineSplitBlockContext(context) = child_box {
            match complete_adjoining_child_margin_set_from_boxes(
                set,
                &context.core.children,
                parent,
                overflow_context,
            ) {
                AdjoiningChildMarginScan::NoAdjoiningChild => {}
                AdjoiningChildMarginScan::Open => found_in_flow_child = true,
                AdjoiningChildMarginScan::Closed => {
                    return AdjoiningChildMarginScan::Closed;
                }
            }
            continue;
        }
        let Some((child_element, _, child_style, child_children)) = child_box.element_parts()
        else {
            if matches!(
                child_box,
                box_tree::FormattingBox::Inline(_) | box_tree::FormattingBox::Text(_)
            ) && !formatting_box_can_only_create_phantom_line_boxes(child_box)
            {
                return if found_in_flow_child {
                    AdjoiningChildMarginScan::Closed
                } else {
                    AdjoiningChildMarginScan::NoAdjoiningChild
                };
            }
            continue;
        };
        if child_style.float != Float::None {
            has_preceding_css_float = true;
            continue;
        }
        if matches!(child_style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let is_flow_child = is_normal_block_flow_child(child_element, child_style)
            || overflow_context.is_document_canvas_flow_element(parent)
            || is_replaced_element(child_element);
        if !is_flow_child {
            if !inline_text(child_element).is_empty() {
                return if found_in_flow_child {
                    AdjoiningChildMarginScan::Closed
                } else {
                    AdjoiningChildMarginScan::NoAdjoiningChild
                };
            }
            continue;
        }
        if child_style.clear != Clear::None && has_preceding_css_float {
            return if found_in_flow_child {
                AdjoiningChildMarginScan::Closed
            } else {
                AdjoiningChildMarginScan::NoAdjoiningChild
            };
        }

        found_in_flow_child = true;
        let child_collapses_through = is_self_collapsing_block_box(
            child_element,
            child_style,
            child_children,
            overflow_context,
        );
        let mut child_set = AdjoiningMarginSet::from_margin(layout_pt(child_style.margin.top));
        if !child_style.margin_trim.block_start
            && can_collapse_block_start_margin(
                child_element,
                child_style,
                UsedEdges::from_css_edges(used_border_widths(child_style)),
                has_direct_inline_content_box(child_children),
                overflow_context.used_overflow(child_element, child_style),
            )
        {
            complete_adjoining_child_margin_set_from_boxes(
                &mut child_set,
                child_children,
                child_element,
                overflow_context,
            );
        }
        if child_collapses_through {
            child_set.include(layout_pt(child_style.margin.bottom));
        }
        set.merge(child_set);
        if !child_collapses_through {
            return AdjoiningChildMarginScan::Closed;
        }
    }
    if found_in_flow_child {
        AdjoiningChildMarginScan::Open
    } else {
        AdjoiningChildMarginScan::NoAdjoiningChild
    }
}

pub(in crate::layout) fn collapsible_start_margin_for_box(
    element: &Element,
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
    overflow_context: DocumentCanvasResolution,
) -> AdjoiningMarginSet {
    if can_collapse_block_start_margin(
        element,
        style,
        UsedEdges::from_css_edges(used_border_widths(style)),
        has_direct_inline_content_box(child_boxes),
        overflow_context.used_overflow(element, style),
    ) && let Some(descendant_margin) = collapsible_first_child_start_margin_from_boxes(
        child_boxes,
        element,
        style,
        overflow_context,
    ) {
        if is_self_collapsing_block_box(element, style, child_boxes, overflow_context) {
            return self_collapsing_block_margin_set_for_box(style, Some(descendant_margin));
        }
        AdjoiningMarginSet::from_margin(layout_pt(style.margin.top)).merged(descendant_margin)
    } else if is_self_collapsing_block_box(element, style, child_boxes, overflow_context) {
        self_collapsing_block_margin_set_for_box(style, None)
    } else {
        AdjoiningMarginSet::from_margin(layout_pt(style.margin.top))
    }
}

/// Return whether a block container has direct inline content that prevents
/// its own block-start/end edges from adjoining child margins.
///
/// Anonymous blocks are block-level siblings, even though they contain inline
/// content, so they do not make the enclosing block container have direct
/// inline content: <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
pub(in crate::layout) fn has_direct_inline_content_box(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| {
        matches!(
            child,
            box_tree::FormattingBox::Inline(_) | box_tree::FormattingBox::Text(_)
        ) && !formatting_box_can_only_create_phantom_line_boxes(child)
    })
}

pub(in crate::layout) fn has_non_inline_formatting_box(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| {
        if box_tree::is_out_of_flow_box(child) {
            return false;
        }
        match child {
            box_tree::FormattingBox::AnonymousBlock(_) => {
                !formatting_box_can_only_create_phantom_line_boxes(child)
            }
            box_tree::FormattingBox::Table(_) => child.style().display.is_block_level(),
            box_tree::FormattingBox::InlineSplitBlockContext(_)
            | box_tree::FormattingBox::Block(_)
            | box_tree::FormattingBox::Flex(_) => true,
            // Replaced elements retain their own durable formatting-box
            // variant, but their outer display decides whether they form a
            // block-flow child or one atomic participant in an inline line.
            // <https://drafts.csswg.org/css-display-3/#replaced-elements>
            box_tree::FormattingBox::Replaced(_) => child.style().display.is_block_level(),
            _ => false,
        }
    })
}

pub(in crate::layout) fn formatting_box_can_only_create_phantom_line_boxes(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    match box_ {
        box_tree::FormattingBox::Text(text) => {
            text.text.is_empty()
                || (text.style.white_space.collapses_spaces()
                    && text.text.chars().all(is_css_collapsible_whitespace))
        }
        box_tree::FormattingBox::Inline(box_) => {
            inline_box_has_no_nonzero_inline_axis_component(&box_.core.style)
                && !box_.core.style.content.is_generated()
                && !box_
                    .core
                    .style
                    .before_style
                    .as_deref()
                    .is_some_and(|style| style.content.is_generated())
                && !box_
                    .core
                    .style
                    .after_style
                    .as_deref()
                    .is_some_and(|style| style.content.is_generated())
                && box_
                    .core
                    .children
                    .iter()
                    .all(formatting_box_can_only_create_phantom_line_boxes)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .all(formatting_box_can_only_create_phantom_line_boxes),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .core
            .children
            .iter()
            .all(formatting_box_can_only_create_phantom_line_boxes),
        box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    }
}

/// Whether generated `content` contributes a non-phantom inline line box.
///
/// An empty (or collapsible-whitespace-only) generated string still creates a
/// pseudo-element box, but it does not make a line box non-adjoining for CSS
/// margin collapse. Other generated content can resolve to visible text or an
/// atomic inline, so it remains a line-box contribution.
/// <https://drafts.csswg.org/css-inline/#invisible-line-boxes>
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn generated_content_has_non_phantom_inline_content(
    style: &ComputedStyle,
) -> bool {
    style.content.generated_parts().is_some_and(|parts| {
        parts.iter().any(|part| match part {
            css::GeneratedContentPart::Text(text) => {
                !(text.is_empty()
                    || style.white_space.collapses_spaces()
                        && text.chars().all(is_css_collapsible_whitespace))
            }
            _ => true,
        })
    })
}

pub(super) fn inline_box_has_no_nonzero_inline_axis_component(style: &ComputedStyle) -> bool {
    let borders = used_border_widths(style);
    [
        inline_start_side(style.writing_mode, style.used_direction()),
        inline_end_side(style.writing_mode, style.used_direction()),
    ]
    .into_iter()
    .all(|side| {
        edge_value(style.margin, side).abs() <= 0.001
            && edge_value(borders, side).abs() <= 0.001
            && edge_value(style.padding, side).abs() <= 0.001
    })
}

fn edge_value(edges: css::Edges, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    }
}

pub(in crate::layout) fn has_atomic_inline_formatting_box(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| match child {
        box_tree::FormattingBox::AtomicInline(_) => true,
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            has_atomic_inline_formatting_box(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            has_atomic_inline_formatting_box(&box_.core.children)
        }
        box_tree::FormattingBox::Inline(box_) => {
            has_atomic_inline_formatting_box(&box_.core.children)
        }
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Text(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    })
}
