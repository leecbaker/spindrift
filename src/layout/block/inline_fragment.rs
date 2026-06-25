use super::super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_inline_fragment_block(
        &mut self,
        nodes: &[Node],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> bool {
        if nodes.is_empty() {
            return false;
        }
        let element = Element {
            tag: "span".to_string(),
            namespace_url: String::new(),
            attrs: HashMap::new(),
            namespace_attrs: Vec::new(),
            children: nodes.to_vec(),
            is_target: false,
        };
        let text = inline_text_for_style(&element, style);
        if text.is_empty() {
            return false;
        }
        if has_styled_inline_descendant(&element, style, stylesheets, &self.ancestors) {
            self.layout_inline_items_block(&element, style, stylesheets, (0.0, 0.0), None, None);
        } else {
            self.layout_text_block(&text, style, 0.0, 0.0, None);
        }
        true
    }
}
