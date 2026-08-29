use super::*;

pub(in crate::layout) fn style_is_in_normal_flow(style: &ComputedStyle) -> bool {
    !style.display.is_none()
        && style.position.is_normal_flow()
        && style.float == Float::None
        && !style.position.is_running()
}

/// Returns whether the computed column properties establish a multi-column
/// formatting context.
///
/// A multicol container establishes an independent formatting context, so its
/// contents do not participate in margin collapsing with the container even
/// when its inner display type is ordinary `flow`.
/// <https://www.w3.org/TR/css-multicol-1/#multicol-container>
pub(in crate::layout) fn style_establishes_multicol_formatting_context(
    style: &ComputedStyle,
) -> bool {
    matches!(style.column_count, css::ColumnCount::Count(_))
        || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
        || matches!(style.column_height, css::ComputedColumnHeight::Length(_))
}

/// `continue: collapse` and `continue: discard` establish an independent
/// formatting context when they create a line-clamp container. Multicol
/// containers are explicitly exempt: their `continue` behavior remains
/// `auto`.
/// <https://drafts.csswg.org/css-overflow-4/#continue>
pub(in crate::layout) fn style_establishes_line_clamp_formatting_context(
    style: &ComputedStyle,
) -> bool {
    !style_establishes_multicol_formatting_context(style)
        && matches!(
            style.used_continuation(),
            css::UsedContinuation::LineClamp(_) | css::UsedContinuation::Discard(_)
        )
}

/// Resolve the clamp state consumed by inline layout for this block container.
///
/// The computed declaration stays untouched; an ancestor traversal may supply
/// a smaller layout budget through `line_limit_traversal`. A multicol container is
/// deliberately ineligible because `continue: collapse` behaves as `auto`
/// there.
/// <https://drafts.csswg.org/css-overflow-4/#continue>
pub(in crate::layout) fn used_line_clamp_for_style(
    style: &ComputedStyle,
) -> Option<css::InlineLineClamp<'_>> {
    style
        .line_limit_traversal
        .as_ref()
        .map(css::InlineLineClamp::Used)
        .or_else(|| {
            (!style_establishes_multicol_formatting_context(style))
                .then(|| {
                    style
                        .line_clamp_container()
                        .map(css::InlineLineClamp::Computed)
                })
                .flatten()
        })
}

/// Returns whether a child participates in normal block flow for margin collapse.
///
/// CSS 2.2 defines floated boxes as out of normal flow, so they must not
/// contribute adjoining margins to their block container:
/// <https://www.w3.org/TR/CSS22/visuren.html#positioning-scheme> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
pub(in crate::layout) fn is_normal_block_flow_child(
    _element: &Element,
    style: &ComputedStyle,
) -> bool {
    style.position.is_normal_flow() && style.float == Float::None && style.display.is_block_level()
}

/// Returns whether a normal-flow block child's outer margins can adjoin its parent.
///
/// A flex container prevents its items' margins from collapsing through it,
/// but its own block-start and block-end margins remain eligible to collapse
/// with an adjoining block parent:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-containers> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
pub(in crate::layout) fn is_collapsible_block_child(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    is_normal_block_flow_child(element, style) && !is_replaced_element(element)
}

/// Returns whether a block's outer margins can adjoin an in-flow block sibling.
///
/// An independent formatting context prevents margins from collapsing through
/// its own edges with its children. It does not isolate the principal box's
/// outer margins from an adjacent sibling in the parent's block formatting
/// context. Consequently, block-level Grid, Flex, and flow-root containers
/// remain eligible here; their inner formatting contexts are handled only by
/// the parent/child margin-collapse predicates.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn outer_margins_adjoin_block_siblings(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    is_normal_block_flow_child(element, style) && !is_replaced_element(element)
}

pub(in crate::layout) fn has_later_normal_block_flow_child_with_font_metrics(
    element: &Element,
    start_element_index: usize,
    sibling_tags: &ElementSiblingSignatureList,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    has_later_normal_block_flow_child_with_resolver(
        element,
        start_element_index,
        sibling_tags,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
    )
}

pub(in crate::layout) fn has_later_normal_block_flow_child_with_resolver(
    element: &Element,
    start_element_index: usize,
    sibling_tags: &ElementSiblingSignatureList,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let mut element_index = 0usize;
    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            continue;
        };
        let current_index = element_index;
        element_index += 1;
        if current_index < start_element_index {
            continue;
        }
        let signature = ElementSignature::with_sibling_list(
            child_element.tag.clone(),
            child_element.attrs.clone(),
            current_index,
            sibling_tags.clone(),
        );
        let style = resolver.structural_style_for_element(
            child_element,
            signature,
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        if is_normal_block_flow_child(child_element, &style) || is_replaced_element(child_element) {
            return true;
        }
    }
    false
}

pub(in crate::layout) fn has_later_normal_block_flow_box_child(
    child_boxes: &[box_tree::FormattingBox<'_>],
    start_index: usize,
    parent: &Element,
    document_canvas: DocumentCanvasResolution,
) -> bool {
    child_boxes
        .iter()
        .skip(start_index)
        .any(|child| formatting_box_is_normal_block_flow_sibling(child, parent, document_canvas))
}

/// Return whether a formatting box is a later in-flow block sibling.
///
/// CSS 2.2 mixed-flow normalization creates anonymous block boxes around runs
/// of inline content. Those anonymous boxes are real block-level siblings for
/// normal-flow ordering and margin-adjacency decisions; a preceding block must
/// not collapse its end margin through the parent as if the anonymous block did
/// not exist:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
pub(in crate::layout) fn formatting_box_is_normal_block_flow_sibling(
    child: &box_tree::FormattingBox<'_>,
    parent: &Element,
    document_canvas: DocumentCanvasResolution,
) -> bool {
    match child {
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            style_is_in_normal_flow(&box_.style)
                && !formatting_box_can_only_create_phantom_line_boxes(child)
        }
        _ => child
            .element_parts()
            .is_some_and(|(child_element, _, child_style, _)| {
                // The document canvas can make an in-flow direct child a
                // block-flow sibling, but it cannot pull an absolute or
                // fixed child back into normal flow. In particular, root/body
                // endpoint selection, margin adjacency, and fragmentation
                // must look through positioned children.
                // <https://www.w3.org/TR/css-position-3/#absolute-positioning>
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                style_is_in_normal_flow(child_style)
                    && (is_normal_block_flow_child(child_element, child_style)
                        || document_canvas.is_document_canvas_flow_element(parent)
                        || is_replaced_element(child_element))
            }),
    }
}
