use super::*;

pub(super) fn flex_children<'a>(
    element: &'a Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
) -> Vec<StyledChild<'a>> {
    flex_child_lists(element, parent_style, stylesheets, ancestors).0
}

/// Splits flex container children into in-flow flex items and positioned descendants.
///
/// CSS Flexbox excludes absolutely positioned descendants from flex layout, but
/// still defines their static-position rectangle from the flex container:
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
pub(super) fn flex_child_lists<'a>(
    element: &'a Element,
    parent_style: &ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
) -> (Vec<StyledChild<'a>>, Vec<StyledChild<'a>>) {
    let sibling_tags = element_sibling_tags(element);
    let mut element_index = 0usize;
    let mut in_flow = Vec::new();
    let mut positioned = Vec::new();
    for node in &element.children {
        let Some(child) = (match &node.kind {
            NodeKind::Element(child) => {
                let signature = ElementSignature::with_siblings(
                    child.tag.clone(),
                    child.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = style_for_layout_element(
                    child,
                    signature.clone(),
                    stylesheets,
                    Some(parent_style),
                    ancestors,
                );
                (!child_style.display.is_none()).then_some(StyledChild {
                    kind: StyledChildKind::Element {
                        element: child,
                        signature,
                        children: None,
                    },
                    style: child_style,
                })
            }
            NodeKind::Text(text) => anonymous_text_flex_child(text, parent_style),
        }) else {
            continue;
        };
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            positioned.push(child);
        } else {
            in_flow.push(child);
        }
    }
    sort_flex_items_by_order(&mut in_flow);
    (in_flow, positioned)
}

pub(super) fn flex_children_from_boxes<'a>(
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> Vec<StyledChild<'a>> {
    flex_child_lists_from_boxes(child_boxes).0
}

/// Splits normalized child boxes into flex items and out-of-flow positioned boxes.
///
/// CSS Positioned Layout makes absolutely positioned boxes out-of-flow, and
/// CSS Flexbox says they do not participate in flex item layout:
/// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
pub(super) fn flex_child_lists_from_boxes<'a>(
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> (Vec<StyledChild<'a>>, Vec<StyledChild<'a>>) {
    let mut in_flow = Vec::new();
    let mut positioned = Vec::new();
    child_boxes
        .iter()
        .filter_map(|box_| {
            if let Some((element, signature, style, children)) = box_.element_parts() {
                return (!style.display.is_none()).then_some(StyledChild {
                    kind: StyledChildKind::Element {
                        element,
                        signature: signature.clone(),
                        children: Some(children),
                    },
                    style: style.clone(),
                });
            }
            if let box_tree::FormattingBox::Text(text_box) = box_ {
                return anonymous_text_flex_child(&text_box.text, &text_box.style);
            }
            None
        })
        .for_each(|child| {
            if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                positioned.push(child);
            } else {
                in_flow.push(child);
            }
        });
    sort_flex_items_by_order(&mut in_flow);
    (in_flow, positioned)
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

/// Builds the anonymous flex item required for non-collapsible text runs.
///
/// CSS Flexbox wraps contiguous text that is not entirely collapsible
/// whitespace in an anonymous block-container flex item:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-items>.
fn anonymous_text_flex_child<'a>(
    text: &str,
    parent_style: &ComputedStyle,
) -> Option<StyledChild<'a>> {
    let text = anonymous_flex_text(text, parent_style)?;
    let mut style = css::style_for_element_with_signature(
        ElementSignature::new("__reasy_anonymous_flex_text", HashMap::new()),
        None,
        &[],
        Some(parent_style),
        &[],
    );
    style.display = Display::BLOCK;
    Some(StyledChild {
        kind: StyledChildKind::AnonymousText { text },
        style,
    })
}

/// Normalizes anonymous flex text using CSS collapsible whitespace rules.
///
/// CSS Text defines collapsible document white space independently from NBSP;
/// NBSP must survive so that it contributes an anonymous flex item's width:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
fn anonymous_flex_text(text: &str, style: &ComputedStyle) -> Option<String> {
    if !style.white_space.collapses_spaces() {
        return (!text.is_empty()).then(|| text.to_string());
    }

    let mut output = String::new();
    let mut pending_space = false;
    for character in text.chars() {
        if is_css_collapsible_whitespace(character) {
            pending_space = !output.is_empty();
        } else {
            if pending_space {
                output.push(' ');
                pending_space = false;
            }
            output.push(character);
        }
    }

    (!output.is_empty()).then_some(output)
}
