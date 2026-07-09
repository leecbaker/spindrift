use super::super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_inline_fragment_block_with_first_line_policy(
        &mut self,
        nodes: &[Node],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        allow_typographic_first_line: bool,
    ) -> bool {
        if nodes.is_empty() {
            return false;
        }
        let suppressed_style = (!allow_typographic_first_line)
            .then(|| style_without_typographic_first_line_pseudos(style))
            .flatten();
        let style = suppressed_style.as_ref().unwrap_or(style);
        let element = Element {
            id: crate::dom::ElementId::next(),
            tag: "span".to_string(),
            namespace_url: String::new(),
            document_syntax: dom::DocumentSyntax::Html,
            attrs: HashMap::new(),
            namespace_attrs: Vec::new(),
            children: nodes.to_vec(),
            is_target: false,
        };
        let text = inline_text_for_style(&element, style);
        if text.is_empty() {
            return false;
        }
        if has_styled_inline_descendant_with_font_metrics(
            &element,
            style,
            stylesheets,
            &self.ancestors,
            &mut self.font_system,
        ) {
            self.layout_inline_items_block(&element, style, stylesheets, (0.0, 0.0), None, None);
        } else {
            self.layout_text_block(&text, style, 0.0, 0.0, None);
        }
        true
    }
}
