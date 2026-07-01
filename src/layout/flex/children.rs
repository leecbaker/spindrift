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
    let mut in_flow = Vec::new();
    let mut positioned = Vec::new();
    let mut anonymous_run = Vec::new();
    for box_ in child_boxes {
        if matches!(box_, box_tree::FormattingBox::Text(_)) {
            anonymous_run.push(box_.clone());
            continue;
        }
        flush_anonymous_flex_run(&mut in_flow, &mut anonymous_run);
        let Some((element, signature, style, children)) = box_.element_parts() else {
            continue;
        };
        if style.display.is_none() {
            continue;
        }
        let source_display = style.display;
        let mut style = style.clone();
        // Flex item box generation blockifies each child box before layout.
        // https://www.w3.org/TR/css-flexbox-1/#flex-items
        style.display = style.display.blockified();
        let children = flex_item_children(source_display, &style, children);
        let child = StyledChild {
            kind: StyledChildKind::Element {
                element,
                signature: Box::new(signature.clone()),
                children: Some(children),
            },
            style,
        };
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            positioned.push(child);
        } else {
            in_flow.push(child);
        }
    }
    flush_anonymous_flex_run(&mut in_flow, &mut anonymous_run);
    sort_flex_items_by_order(&mut in_flow);
    (in_flow, positioned)
}

/// Returns the child box list appropriate for a generated flex item.
///
/// CSS Flexbox blockifies flex item boxes before layout. If an inline flow box
/// becomes a block flow flex item, its existing children were built for an
/// inline parent and need normal block-container anonymous box fixup before
/// block layout consumes them:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-items> and
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
fn flex_item_children<'a>(
    source_display: Display,
    flex_item_style: &ComputedStyle,
    children: &'a [box_tree::FormattingBox<'a>],
) -> std::borrow::Cow<'a, [box_tree::FormattingBox<'a>]> {
    if source_display.is_inline_or_run_in_level()
        && source_display.is_flow()
        && flex_item_style.display.is_block_level()
        && flex_item_style.display.is_flow()
    {
        return std::borrow::Cow::Owned(box_tree::normalize_block_container_children(
            children.to_vec(),
            flex_item_style,
        ));
    }
    std::borrow::Cow::Borrowed(children)
}

/// Flushes the anonymous flex item required for a contiguous text run.
///
/// CSS Flexbox wraps each contiguous run of child text that is not entirely
/// collapsible whitespace in an anonymous block-container flex item:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-items>.
fn flush_anonymous_flex_run<'a>(
    children: &mut Vec<StyledChild<'a>>,
    anonymous_run: &mut Vec<box_tree::FormattingBox<'a>>,
) {
    if anonymous_run.is_empty() {
        return;
    }
    if anonymous_run
        .iter()
        .all(box_tree::formatting_box_is_collapsible_space)
    {
        anonymous_run.clear();
        return;
    }
    let style = anonymous_flex_item_style(&anonymous_run[0]);
    children.push(StyledChild {
        kind: StyledChildKind::AnonymousContent {
            children: std::mem::take(anonymous_run),
        },
        style,
    });
}

/// Builds the anonymous block-container style for flex text runs.
///
/// The generated flex item is not the suppressed `display: contents` element,
/// so non-inherited paint such as backgrounds must not be copied from the text
/// box style. Descendant text boxes keep their own inherited styles for inline
/// layout and painting.
/// <https://www.w3.org/TR/css-display-3/#valdef-display-contents> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-items>.
fn anonymous_flex_item_style(source: &box_tree::FormattingBox<'_>) -> ComputedStyle {
    let mut style = css::style_for_element_with_signature(
        ElementSignature::new("__reasy_anonymous_flex_item", HashMap::new()),
        None,
        &[],
        Some(formatting_box_style(source)),
        &[],
    );
    style.display = Display::BLOCK;
    style
}

fn formatting_box_style<'a>(box_: &'a box_tree::FormattingBox<'a>) -> &'a ComputedStyle {
    match box_ {
        box_tree::FormattingBox::Block(box_) => &box_.style,
        box_tree::FormattingBox::Inline(box_) => &box_.style,
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => &box_.style,
        box_tree::FormattingBox::AnonymousBlock(box_) => &box_.style,
        box_tree::FormattingBox::AtomicInline(box_) => &box_.style,
        box_tree::FormattingBox::Line(box_) => &box_.style,
        box_tree::FormattingBox::Text(box_) => &box_.style,
        box_tree::FormattingBox::Table(box_) => &box_.style,
        box_tree::FormattingBox::Flex(box_) => &box_.style,
        box_tree::FormattingBox::Replaced(box_) => &box_.style,
    }
}

/// Apply the flex item ordinal group order while preserving source-order ties.
///
/// CSS Flexible Box Layout says flex items are displayed and laid out in
/// order-modified document order, sorting by `order` and preserving document
/// order for equal values:
/// <https://www.w3.org/TR/css-flexbox-1/#order-property>.
fn sort_flex_items_by_order(children: &mut [StyledChild<'_>]) {
    children.sort_by_key(|child| child.style.order);
}
