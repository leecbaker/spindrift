use super::*;

/// Applies CSS min/max size constraints, with minimums overriding maximums.
///
/// CSS 2.2 applies max constraints first and min constraints second, so a
/// larger minimum size wins over a smaller maximum size:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths> and
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn constrain(mut value: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    if let Some(max) = max {
        value = value.min(max);
    }
    if let Some(min) = min {
        value = value.max(min);
    }
    value
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
    let text = match style.white_space {
        WhiteSpace::Normal | WhiteSpace::NoWrap => normalize_inline_text(text),
        WhiteSpace::PreLine => normalize_pre_line_text_for_style(text, style),
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces => {
            normalize_pre_wrap_text_for_style(text, style)
        }
    };
    text_with_visible_control_characters(&text)
}

pub(in crate::layout) fn inline_text_for_style(element: &Element, style: &ComputedStyle) -> String {
    let text = match style.white_space {
        WhiteSpace::Normal | WhiteSpace::NoWrap => inline_text(element),
        WhiteSpace::PreLine => pre_line_inline_text_for_style(element, style),
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces => {
            pre_wrap_inline_text_for_style(element, style)
        }
    };
    text_with_visible_control_characters(&text)
}

pub(in crate::layout) fn own_inline_text(element: &Element) -> String {
    if element_suppresses_direct_text_children(element) {
        return String::new();
    }
    let mut output = String::new();
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                output.push_str(text);
                output.push(' ');
            }
            NodeKind::Element(child) if is_line_break_element(child) => output.push(INLINE_BREAK),
            _ => {}
        }
    }
    normalize_inline_text(&output)
}

pub(in crate::layout) fn own_inline_text_for_style(
    element: &Element,
    style: &ComputedStyle,
) -> String {
    let text = match style.white_space {
        WhiteSpace::Normal | WhiteSpace::NoWrap => own_inline_text(element),
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
    text_with_visible_control_characters(&text)
}

/// Replaces non-whitespace Unicode control characters with a visible glyph.
///
/// CSS Text white-space processing keeps control characters other than
/// document white space visible instead of silently discarding them. Use U+FFFD
/// so PDF output has a font-fallback-visible glyph even when no font maps the
/// original C0/C1 control code:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(in crate::layout) fn text_with_visible_control_characters(text: &str) -> String {
    text.chars()
        .map(|character| {
            if is_visible_control_character(character) {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

pub(in crate::layout) fn is_visible_control_character(character: char) -> bool {
    character_is_unicode_control(character)
        // CSS Text treats TAB, LF, and CR as document white space. Form feed
        // is a visible Cc character in the white-space processing tests and
        // must not be discarded merely because generic CSS token whitespace
        // handling recognizes it.
        && !matches!(character, '\t' | '\n' | '\r')
        && character != INLINE_BREAK
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
            output.push(' ');
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
/// U+000B is an authored Unicode control character and CSS Text requires it
/// to remain visible, so it cannot also be Quire's internal break sentinel.
/// U+FDD0 is a noncharacter and never arises from normal HTML/CSS text input.
/// <https://drafts.csswg.org/css-text-3/#white-space-processing>
pub(in crate::layout) const INLINE_BREAK: char = '\u{FDD0}';

pub(in crate::layout) fn normalize_inline_text(text: &str) -> String {
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
    text.replace("\r\n", "\n").replace('\r', "\n")
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
