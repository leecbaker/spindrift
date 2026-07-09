use super::*;

pub(super) struct PdfTextRun<'a> {
    pub(super) document_font_id: usize,
    pub(super) actual_text: Option<&'a str>,
    pub(super) x_offset: f32,
    pub(super) y_offset: f32,
    pub(super) text_matrix: RenderedTextMatrix,
    pub(super) font_size: f32,
    pub(super) glyphs: &'a [RenderedGlyph],
}

pub(super) fn pdf_text_runs(
    line: &crate::RenderedLine,
    document_font_count: usize,
) -> impl Iterator<Item = PdfTextRun<'_>> {
    line.runs.iter().filter_map(move |run| {
        let document_font_id = run.font_id?;
        if document_font_id >= document_font_count {
            return None;
        }
        Some(PdfTextRun {
            document_font_id,
            actual_text: run.actual_text.as_deref(),
            x_offset: run.x_offset,
            y_offset: run.y_offset,
            text_matrix: run.text_matrix,
            font_size: run.font_size,
            glyphs: run.glyphs.as_deref()?,
        })
    })
}
