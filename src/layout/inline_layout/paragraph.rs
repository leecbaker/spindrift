use super::super::*;
use super::InlineLayoutOutcome;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_inline_paragraph(
        &mut self,
        items: &[InlineItem],
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
        preceding_plaintext_line_direction: &mut Option<Direction>,
    ) -> InlineLayoutOutcome {
        // All inline paragraphs use the same CSS line-fitting engine. Parley
        // remains the text measurement, shaping, and fragment-splitting
        // backend, but layout-owned CSS policy decides where line boxes break:
        // <https://www.w3.org/TR/css-inline-3/#line-layout> and
        // <https://www.w3.org/TR/css-text-3/#line-breaking>.
        self.layout_mixed_inline_paragraph(
            items,
            context,
            line_index,
            starts_after_forced_break,
            preceding_plaintext_line_direction,
        )
    }
}
