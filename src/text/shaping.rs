use super::*;

/// Classify an inline boundary by the kind of text behavior it can affect.
/// Paint-only changes must retain one shaping context; shaping-affecting
/// changes may require synthetic join context; layout boundaries are hard
/// shaping breaks:
/// <https://drafts.csswg.org/css-text-3/#boundary-shaping>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineBoundaryEffect {
    PaintOnly,
    ShapingInputChange,
    LayoutShapingBreak,
}

/// The directional level assigned to a source range after UAX #9 resolution.
///
/// This deliberately differs from CSS `direction`: it describes the visual
/// embedding level chosen for one already-reordered cluster, which determines
/// the directional override needed when the cluster is shaped for painting.
/// <https://www.unicode.org/reports/tr9/#Reordering_Resolved_Levels>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolvedBidiDirection {
    Ltr,
    Rtl,
}

/// One source range in UAX #9 visual order together with its resolved level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BidiVisualRange {
    pub(crate) range: Range<usize>,
    pub(crate) direction: ResolvedBidiDirection,
}

pub(super) fn shape_text_with_document_font(
    font: &DocumentFont,
    text: &str,
    font_size: f32,
    letter_spacing: f32,
    word_spacing: f32,
) -> Option<Vec<RenderedGlyph>> {
    if text.is_empty() {
        return Some(Vec::new());
    }
    let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
    let units_per_em = face.units_per_em().max(1) as f32;
    let scale = font_size / units_per_em;
    let used_letter_spacing = used_letter_spacing_for_text(text, letter_spacing);
    let characters = text
        .chars()
        .filter(|character| !character_is_bidi_format_control(*character))
        .collect::<Vec<_>>();
    let glyphs = characters
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, character)| {
            // Tabs are a synthetic layout control. Every Unicode scalar,
            // including CSS "other space separators", keeps the selected
            // font's own glyph and advance.
            let glyph_id = if character == '\t' {
                face.glyph_index(' ')?
            } else {
                face.glyph_index(character)?
            };
            let mut x_advance = face
                .glyph_hor_advance(glyph_id)
                .map(|advance| advance as f32 * scale)
                .unwrap_or(0.0);
            if used_letter_spacing != 0.0 && index + 1 < characters.len() {
                x_advance += used_letter_spacing;
            }
            if word_spacing != 0.0 && character_is_css_word_separator(character) {
                x_advance += word_spacing;
            }
            Some(RenderedGlyph {
                // Preserved tabs use a font's provisional advance only to
                // keep later shaping runs positioned until CSS tab-stop
                // resolution replaces it. They are never paintable glyphs,
                // including when this direct document-font path is used for
                // a fallback run.
                kind: if character == '\t' {
                    RenderedGlyphKind::AdvanceOnly
                } else {
                    RenderedGlyphKind::Paint(glyph_id.0)
                },
                x_advance,
                // Preserved tabs have no font glyph in PDF output. Their
                // nominal advance must therefore agree with their used
                // advance so PDF text positioning does not reintroduce the
                // metric of the placeholder U+0020 glyph.
                nominal_x_advance: if character == '\t' {
                    x_advance
                } else {
                    face.glyph_hor_advance(glyph_id)
                        .map(|advance| advance as f32 * scale)
                        .unwrap_or(x_advance)
                },
                x_offset: 0.0,
                y_offset: 0.0,
                unicode: character.to_string(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(glyphs)
}

/// Returns the spacing that should affect glyph advances for this text.
///
/// CSS Text requires cursive scripts to remain connected under `letter-spacing`.
/// Unicode `Joining_Type` data identifies runs where inserting tracking would
/// disturb cursive shaping:
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property> and
/// <https://www.unicode.org/reports/tr44/#Joining_Type>.
pub(super) fn used_letter_spacing_for_text(text: &str, letter_spacing: f32) -> f32 {
    if letter_spacing == 0.0 || text.chars().any(character_has_cursive_shaping_behavior) {
        0.0
    } else {
        letter_spacing
    }
}
