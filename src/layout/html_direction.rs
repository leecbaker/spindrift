use super::*;

/// Return a computed style adjusted for HTML element directionality.
///
/// HTML defines `dir=auto` and default `<bdi>` directionality from the first
/// strong descendant text character, which cannot be expressed as a static CSS
/// rule. The static UA stylesheet still provides the CSS bidi isolation rules;
/// this helper supplies the dynamic `direction` value before layout uses it:
/// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality> and
/// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering>.
pub(super) fn style_for_layout_element(
    element: &Element,
    signature: ElementSignature,
    stylesheets: &[Stylesheet],
    parent: Option<&ComputedStyle>,
    ancestors: &[ElementSignature],
) -> ComputedStyle {
    style_for_layout_element_with_signature_transform(
        element,
        signature,
        stylesheets,
        parent,
        ancestors,
        None,
    )
}

pub(super) fn style_for_layout_element_with_parent_ch_advance(
    element: &Element,
    signature: ElementSignature,
    stylesheets: &[Stylesheet],
    parent: Option<&ComputedStyle>,
    ancestors: &[ElementSignature],
    parent_ch_advance: LayoutLength,
) -> ComputedStyle {
    style_for_layout_element_with_signature_transform(
        element,
        signature,
        stylesheets,
        parent,
        ancestors,
        Some(parent_ch_advance),
    )
}

fn style_for_layout_element_with_signature_transform(
    element: &Element,
    signature: ElementSignature,
    stylesheets: &[Stylesheet],
    parent: Option<&ComputedStyle>,
    ancestors: &[ElementSignature],
    parent_ch_advance: Option<LayoutLength>,
) -> ComputedStyle {
    let signature = layout_element_signature(element, signature, parent);
    let inline_style = element.attrs.get("style").map(String::as_str);
    if let Some(parent_ch_advance) = parent_ch_advance {
        css::style_for_element_with_signature_and_parent_ch_advance(
            signature,
            inline_style,
            stylesheets,
            parent,
            ancestors,
            parent_ch_advance,
        )
    } else {
        css::style_for_element_with_signature(
            signature,
            inline_style,
            stylesheets,
            parent,
            ancestors,
        )
    }
}

pub(super) fn layout_element_signature(
    element: &Element,
    mut signature: ElementSignature,
    parent: Option<&ComputedStyle>,
) -> ElementSignature {
    let selector_signature = element_selector_signature(element);
    signature.namespace_url = selector_signature.namespace_url.clone();
    signature.namespace_attrs = selector_signature.namespace_attrs.clone();
    signature.is_target = selector_signature.is_target;
    signature = signature.with_child_list(
        selector_signature.children,
        selector_signature.has_text_child,
    );
    if let Some(direction) = element_document_direction(element) {
        signature = signature.with_document_direction(direction);
    }
    let html_direction = html_directionality(element);
    if let Some(direction) = html_direction {
        signature = signature.with_html_direction(direction);
    }
    let resolved_direction = html_direction
        .or_else(|| element_dir_attribute_direction(element))
        .or_else(|| parent.map(|style| style.direction))
        .unwrap_or(Direction::Ltr);
    signature = signature.with_resolved_direction(resolved_direction);
    signature
}

/// Return HTML/document directionality determined by the element itself.
///
/// Selectors `:dir()` is defined in terms of the document language, not CSS
/// `direction`. Explicit `dir=ltr`/`dir=rtl`, `dir=auto`, and omitted `<bdi>`
/// establish an element direction; an undefined `dir` inherits from the parent
/// during selector matching:
/// <https://drafts.csswg.org/selectors/#the-dir-pseudo> and
/// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality>.
pub(in crate::layout) fn element_document_direction(element: &Element) -> Option<Direction> {
    element_dir_attribute_direction(element).or_else(|| html_directionality(element))
}

/// Apply HTML's dynamic `dir=auto`/`bdi` directionality to a computed style.
///
/// The HTML `dir` attribute is an enumerated attribute with LTR, RTL, and Auto
/// states. LTR/RTL are covered by the UA cascade; Auto and an omitted `<bdi>`
/// value depend on contained text directionality and fall back to LTR:
/// <https://html.spec.whatwg.org/multipage/dom.html#the-dir-attribute>.
#[cfg(test)]
fn apply_html_directionality(element: &Element, style: &mut ComputedStyle) {
    if let Some(direction) = html_directionality(element) {
        style.direction = direction;
    }
}

fn html_directionality(element: &Element) -> Option<Direction> {
    (element_dir_attribute_state(element) == HtmlDirState::Auto
        || (element.tag == "bdi"
            && element_dir_attribute_state(element) == HtmlDirState::Undefined))
        .then(|| html_auto_directionality(element).unwrap_or(Direction::Ltr))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HtmlDirState {
    Ltr,
    Rtl,
    Auto,
    Undefined,
}

fn element_dir_attribute_state(element: &Element) -> HtmlDirState {
    match element.attrs.get("dir").map(|value| value.trim()) {
        Some(value) if value.eq_ignore_ascii_case("ltr") => HtmlDirState::Ltr,
        Some(value) if value.eq_ignore_ascii_case("rtl") => HtmlDirState::Rtl,
        Some(value) if value.eq_ignore_ascii_case("auto") => HtmlDirState::Auto,
        _ => HtmlDirState::Undefined,
    }
}

fn element_dir_attribute_direction(element: &Element) -> Option<Direction> {
    match element_dir_attribute_state(element) {
        HtmlDirState::Ltr => Some(Direction::Ltr),
        HtmlDirState::Rtl => Some(Direction::Rtl),
        HtmlDirState::Auto | HtmlDirState::Undefined => None,
    }
}

fn html_auto_directionality(element: &Element) -> Option<Direction> {
    contained_text_auto_directionality(element, false)
}

fn contained_text_auto_directionality(
    element: &Element,
    can_exclude_root: bool,
) -> Option<Direction> {
    if can_exclude_root && excludes_auto_directionality_subtree(element) {
        return None;
    }
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                if let Some(direction) = plaintext_direction_for_text(text) {
                    return Some(direction);
                }
            }
            NodeKind::Element(child_element) => {
                if excludes_auto_directionality_subtree(child_element) {
                    continue;
                }
                if let Some(direction) = contained_text_auto_directionality(child_element, false) {
                    return Some(direction);
                }
            }
        }
    }
    None
}

fn excludes_auto_directionality_subtree(element: &Element) -> bool {
    matches!(
        element.tag.as_str(),
        "bdi" | "script" | "style" | "textarea"
    ) || element_dir_attribute_state(element) != HtmlDirState::Undefined
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parse;

    fn first_element_by_tag(source: &str, tag: &str) -> Element {
        let root = parse(source);
        find_element_by_tag(root, tag).expect("expected element")
    }

    fn find_element_by_tag(node: Node, tag: &str) -> Option<Element> {
        match node.kind {
            NodeKind::Text(_) => None,
            NodeKind::Element(element) => {
                if element.tag == tag {
                    return Some(element);
                }
                element
                    .children
                    .into_iter()
                    .find_map(|child| find_element_by_tag(child, tag))
            }
        }
    }

    #[test]
    fn dir_auto_uses_first_strong_descendant_text() {
        let element = first_element_by_tag("<p dir=\"auto\">123 אבג abc</p>", "p");
        let mut style = ComputedStyle::initial();

        apply_html_directionality(&element, &mut style);

        assert_eq!(style.direction, Direction::Rtl);
    }

    #[test]
    fn dir_auto_skips_descendant_with_defined_dir_attribute() {
        let element =
            first_element_by_tag("<p dir=\"auto\"><span dir=\"rtl\">אבג</span> abc</p>", "p");
        let mut style = ComputedStyle::initial();

        apply_html_directionality(&element, &mut style);

        assert_eq!(style.direction, Direction::Ltr);
    }

    #[test]
    fn bdi_without_dir_uses_auto_directionality() {
        let element = first_element_by_tag("<bdi>אבג</bdi>", "bdi");
        let mut style = ComputedStyle::initial();

        apply_html_directionality(&element, &mut style);

        assert_eq!(style.direction, Direction::Rtl);
    }
}
