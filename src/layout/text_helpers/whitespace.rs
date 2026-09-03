use super::*;

fn is_default_block_container_tag(tag: &str) -> bool {
    // CSS Display defines block containers by computed display, while HTML only
    // supplies the default display values through the UA stylesheet. The
    // cascade-derived result is cached per tag because inline-text collection
    // performs this classification for every nested element.
    // https://www.w3.org/TR/css-display-3/#block-container
    css::default_display_is_block_level_for_tag(tag)
}

pub(in crate::layout) fn inline_text(element: &Element) -> String {
    if element_suppresses_direct_text_children(element) {
        return String::new();
    }
    let mut output = String::new();
    for child in &element.children {
        collect_inline_text(child, &mut output);
    }
    normalize_inline_text(&output)
}

pub(in crate::layout) fn normalized_text_for_style(text: &str, style: &ComputedStyle) -> String {
    let text = css_text_rendering_text(text);
    match style.white_space {
        WhiteSpace::Normal | WhiteSpace::NoWrap => normalize_inline_text(&text),
        WhiteSpace::PreLine => normalize_pre_line_text_for_style(&text, style),
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces => {
            normalize_pre_wrap_text_for_style(&text, style)
        }
    }
}

pub(in crate::layout) fn inline_text_for_style(element: &Element, style: &ComputedStyle) -> String {
    let text = match style.white_space {
        WhiteSpace::Normal | WhiteSpace::NoWrap => {
            if element_suppresses_direct_text_children(element) {
                String::new()
            } else {
                let mut output = String::new();
                for child in &element.children {
                    collect_inline_text(child, &mut output);
                }
                normalize_inline_text_for_style(&output, style)
            }
        }
        WhiteSpace::PreLine => pre_line_inline_text_for_style(element, style),
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces => {
            pre_wrap_inline_text_for_style(element, style)
        }
    };
    css_text_rendering_text(&text)
}

pub(in crate::layout) fn own_inline_text_for_style(
    element: &Element,
    style: &ComputedStyle,
) -> String {
    let text = match style.white_space {
        WhiteSpace::Normal | WhiteSpace::NoWrap => {
            let mut output = String::new();
            for child in &element.children {
                match &child.kind {
                    NodeKind::Text(text) => output.push_str(text),
                    NodeKind::Element(child) if is_line_break_element(child) => {
                        output.push(INLINE_BREAK)
                    }
                    _ => {}
                }
            }
            normalize_inline_text_for_style(&output, style)
        }
        WhiteSpace::PreLine => {
            let mut output = String::new();
            for child in &element.children {
                match &child.kind {
                    NodeKind::Text(text) => output.push_str(text),
                    NodeKind::Element(child) if is_line_break_element(child) => output.push('\n'),
                    _ => {}
                }
            }
            normalize_pre_line_text_for_style(&output, style)
        }
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces => {
            let mut output = String::new();
            for child in &element.children {
                match &child.kind {
                    NodeKind::Text(text) => output.push_str(text),
                    NodeKind::Element(child) if is_line_break_element(child) => output.push('\n'),
                    _ => {}
                }
            }
            normalize_pre_wrap_text_for_style(&output, style)
        }
    };
    css_text_rendering_text(&text)
}

/// Collapse normal white space while preserving CSS Text's context-sensitive
/// segment-break transformation. This is the scalar counterpart to the
/// inline-item whitespace processor used when a block selects the direct DOM
/// text path before building line items.
fn normalize_inline_text_for_style(text: &str, style: &ComputedStyle) -> String {
    let text = css_text_rendering_text(text);
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        if character == INLINE_BREAK {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push('\n');
            index += 1;
            continue;
        }
        if !is_css_collapsible_whitespace(character) {
            output.push(character);
            index += 1;
            continue;
        }

        let mut contains_segment_break = false;
        while index < characters.len() && is_css_collapsible_whitespace(characters[index]) {
            contains_segment_break |= characters[index] == '\n';
            index += 1;
        }
        let before = output.chars().rev().find(|character| *character != ' ');
        let after = characters[index..]
            .iter()
            .copied()
            .find(|character| !is_css_collapsible_whitespace(*character));
        let removes_break = contains_segment_break
            && before.zip(after).is_some_and(|(before, after)| {
                crate::text::segment_break_is_removable(crate::text::SegmentBreakContext {
                    before,
                    after,
                    before_is_currency_amount: false,
                    language: style.language.as_deref(),
                })
            });
        if !removes_break && !output.is_empty() && !output.ends_with(' ') {
            output.push(' ');
        }
    }
    crate::text::trim_css_collapsible_whitespace(&output).to_string()
}

pub(in crate::layout) fn pre_wrap_inline_text_for_style(
    element: &Element,
    style: &ComputedStyle,
) -> String {
    if element_suppresses_direct_text_children(element) {
        return String::new();
    }
    let mut output = String::new();
    for child in &element.children {
        collect_pre_wrap_inline_text(child, &mut output);
    }
    normalize_pre_wrap_text_for_style(&output, style)
}

pub(in crate::layout) fn pre_line_inline_text_for_style(
    element: &Element,
    style: &ComputedStyle,
) -> String {
    if element_suppresses_direct_text_children(element) {
        return String::new();
    }
    let mut output = String::new();
    for child in &element.children {
        collect_pre_wrap_inline_text(child, &mut output);
    }
    normalize_pre_line_text_for_style(&output, style)
}

pub(in crate::layout) fn collect_inline_text(node: &Node, output: &mut String) {
    match &node.kind {
        NodeKind::Text(text) => {
            output.push_str(text);
        }
        NodeKind::Element(element) if is_line_break_element(element) => output.push(INLINE_BREAK),
        NodeKind::Element(element) if element_suppresses_direct_text_children(element) => {}
        NodeKind::Element(element) if is_default_block_container_tag(&element.tag) => {}
        NodeKind::Element(element) => {
            for child in &element.children {
                collect_inline_text(child, output);
            }
        }
    }
}

pub(in crate::layout) fn collect_pre_wrap_inline_text(node: &Node, output: &mut String) {
    match &node.kind {
        NodeKind::Text(text) => output.push_str(text),
        NodeKind::Element(element) if is_line_break_element(element) => output.push('\n'),
        NodeKind::Element(element) if element_suppresses_direct_text_children(element) => {}
        NodeKind::Element(element) if is_default_block_container_tag(&element.tag) => {}
        NodeKind::Element(element) => {
            for child in &element.children {
                collect_pre_wrap_inline_text(child, output);
            }
        }
    }
}

/// Private marker used while collecting HTML `<br>` boundaries.
///
/// U+000B is an authored Unicode control character with UAX #14 mandatory
/// break semantics, so it cannot also be Spindrift's internal break sentinel.
/// U+FDD0 is a noncharacter and never arises from normal HTML/CSS text input.
/// <https://drafts.csswg.org/css-text-3/#white-space-processing>
pub(in crate::layout) const INLINE_BREAK: char = '\u{FDD0}';

pub(in crate::layout) fn normalize_inline_text(text: &str) -> String {
    let text = css_text_rendering_text(text);
    let mut output = String::new();
    let mut last_was_space = true;
    for character in text.chars() {
        if character == INLINE_BREAK {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push('\n');
            last_was_space = true;
        } else if is_css_collapsible_whitespace(character) {
            if !last_was_space {
                output.push(' ');
                last_was_space = true;
            }
        } else {
            output.push(character);
            last_was_space = false;
        }
    }
    crate::text::trim_css_collapsible_whitespace(&output).to_string()
}

pub(in crate::layout) fn normalize_pre_wrap_text_for_style(
    text: &str,
    _style: &ComputedStyle,
) -> String {
    css_text_rendering_text(text)
}

pub(in crate::layout) fn normalize_pre_line_text_for_style(
    text: &str,
    style: &ComputedStyle,
) -> String {
    let mut output = String::new();
    let mut last_was_space = true;
    for character in normalize_pre_wrap_text_for_style(text, style).chars() {
        if character == '\n' || character == INLINE_BREAK {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push('\n');
            last_was_space = true;
        } else if is_css_collapsible_whitespace(character) {
            if !last_was_space {
                output.push(' ');
                last_was_space = true;
            }
        } else {
            output.push(character);
            last_was_space = false;
        }
    }
    crate::text::trim_css_collapsible_whitespace(&output).to_string()
}
