use super::super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_inline_fragment_block_with_first_line_policy(
        &mut self,
        nodes: &[(usize, Node)],
        parent: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        allow_typographic_first_line: bool,
    ) -> bool {
        if nodes.is_empty() {
            return false;
        }
        let suppressed_style = (!allow_typographic_first_line)
            .then(|| style_without_typographic_first_line_pseudos(style))
            .flatten();
        let style = suppressed_style.as_ref().unwrap_or(style);
        // Preserve source sibling positions while isolating this inline run.
        // CSS selectors are evaluated before anonymous inline grouping, so a
        // synthetic `span` must not renumber `:nth-*` siblings.
        let mut element = parent.clone();
        for (source_index, node) in element.children.iter_mut().enumerate() {
            if nodes
                .iter()
                .any(|(inline_index, _)| *inline_index == source_index)
            {
                continue;
            }
            match &mut node.kind {
                NodeKind::Text(text) => text.clear(),
                NodeKind::Element(element) => {
                    element
                        .attrs
                        .entry("style".to_string())
                        .and_modify(|style| style.push_str(";display:none !important"))
                        .or_insert_with(|| "display:none !important".to_string());
                }
            }
        }
        let has_direct_line_break = element_has_direct_line_break(&element);
        let text = inline_text_for_style(&element, style);
        let has_styled_inline_descendant = has_styled_inline_descendant_with_font_metrics(
            &element,
            style,
            stylesheets,
            &self.ancestors,
            &mut self.font_system,
        );
        if text.is_empty() && !has_direct_line_break && !has_styled_inline_descendant {
            return false;
        }
        // An inline fragment can contain a semantic `<br>` with no text or
        // font-style difference. It still needs the item collector, which
        // carries the element's `clear` value into the forced-break record;
        // the text-only path would reduce it to a newline and lose clearance.
        // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        if has_direct_line_break || has_styled_inline_descendant {
            let child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(&element, stylesheets, style);
            let _ = self.layout_inline_items_block(
                &element,
                style,
                stylesheets,
                Some(&child_boxes),
                (0.0, 0.0),
                None,
                None,
            );
        } else {
            self.layout_text_block(&text, style, 0.0, 0.0, None);
        }
        true
    }
}
