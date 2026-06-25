use super::*;

pub(super) fn shape_document_text(document: &Document) -> ShapedDocument {
    let pages = document
        .pages
        .iter()
        .map(|page| {
            page.lines
                .iter()
                .map(|line| {
                    let runs = line
                        .runs
                        .iter()
                        .filter_map(|run| {
                            let font_id = run.font_id?;
                            document.fonts.get(font_id)?;
                            let glyphs = run.glyphs.as_ref()?;
                            Some(ShapedRun {
                                document_font_id: font_id,
                                x_offset: run.x_offset,
                                font_size: run.font_size,
                                glyphs: glyphs.iter().map(shaped_glyph_from_rendered).collect(),
                            })
                        })
                        .collect::<Vec<_>>();
                    (!runs.is_empty()).then_some(ShapedLine { runs })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    ShapedDocument { pages }
}

pub(super) fn shaped_glyph_from_rendered(glyph: &crate::RenderedGlyph) -> ShapedGlyph {
    ShapedGlyph {
        id: glyph.id,
        x_advance: glyph.x_advance,
        nominal_x_advance: glyph.nominal_x_advance,
        x_offset: glyph.x_offset,
        y_offset: glyph.y_offset,
        unicode: glyph.unicode.clone(),
    }
}
