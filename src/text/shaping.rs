use super::*;

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

impl ResolvedBidiDirection {
    pub(super) const fn from_parley_cluster_is_rtl(is_rtl: bool) -> Self {
        if is_rtl { Self::Rtl } else { Self::Ltr }
    }
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
    let mut glyphs = Vec::with_capacity(characters.len());
    for (index, character) in characters.iter().cloned().enumerate() {
        let glyph_id = if character == '\t' {
            face.glyph_index(' ')?
        } else {
            css_space_separator_blank_glyph(&face, character)
                .or_else(|| face.glyph_index(character))?
        };
        let mut x_advance = css_space_separator_advance(&face, character, font_size, scale)
            .or_else(|| {
                face.glyph_hor_advance(glyph_id)
                    .map(|advance| advance as f32 * scale)
            })
            .unwrap_or(0.0);
        if used_letter_spacing != 0.0 && index + 1 < characters.len() {
            x_advance += used_letter_spacing;
        }
        if word_spacing != 0.0 && character_is_css_word_separator(character) {
            x_advance += word_spacing;
        }
        glyphs.push(RenderedGlyph {
            id: glyph_id.0,
            x_advance,
            nominal_x_advance: face
                .glyph_hor_advance(glyph_id)
                .map(|advance| advance as f32 * scale)
                .unwrap_or(x_advance),
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: character.to_string(),
        });
    }
    Some(glyphs)
}

/// Return a blank glyph for a Unicode space separator.
///
/// CSS Text treats Unicode space separators as visible text with advance, but
/// they are spacing characters rather than inked fallback glyphs. When a
/// selected font exposes a visible `.notdef` or missing-glyph outline for
/// characters such as U+2002 EN SPACE, PDF emission should preserve the
/// separator advance and ToUnicode mapping while painting a blank space glyph:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.unicode.org/reports/tr44/#General_Category_Values>.
pub(super) fn css_space_separator_blank_glyph(
    face: &ttf_parser::Face<'_>,
    character: char,
) -> Option<ttf_parser::GlyphId> {
    character_is_css_other_space_separator(character)
        .then(|| face.glyph_index(' '))
        .flatten()
}

/// Return the typographic advance for fixed Unicode space separators.
///
/// Unicode defines several `Space_Separator` characters by their nominal em
/// width. CSS Text keeps these characters in the text stream, so the renderer
/// synthesizes their blank advance when the selected font lacks a reliable
/// glyph-specific metric:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.unicode.org/charts/PDF/U2000.pdf>.
pub(super) fn css_space_separator_advance(
    face: &ttf_parser::Face<'_>,
    character: char,
    font_size: f32,
    scale: f32,
) -> Option<f32> {
    match character {
        '\u{00a0}' | '\u{1680}' => face
            .glyph_index(' ')
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .map(|advance| advance as f32 * scale)
            .or(Some(font_size * 0.25)),
        '\u{2000}' | '\u{2002}' => Some(font_size * 0.5),
        '\u{2001}' | '\u{2003}' | '\u{3000}' => Some(font_size),
        '\u{2004}' => Some(font_size / 3.0),
        '\u{2005}' => Some(font_size / 4.0),
        '\u{2006}' => Some(font_size / 6.0),
        '\u{2007}' => face
            .glyph_index('0')
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .map(|advance| advance as f32 * scale)
            .or(Some(font_size * 0.5)),
        '\u{2008}' => face
            .glyph_index('.')
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .map(|advance| advance as f32 * scale)
            .or(Some(font_size * 0.25)),
        '\u{2009}' | '\u{202f}' => Some(font_size / 5.0),
        '\u{200a}' => Some(font_size / 10.0),
        '\u{205f}' => Some(font_size * 4.0 / 18.0),
        _ => None,
    }
}

/// Returns the spacing that should affect glyph advances for this text.
///
/// CSS Text requires cursive scripts to remain connected under `letter-spacing`.
/// Unicode `Joining_Type` data identifies runs where inserting tracking would
/// disturb cursive shaping:
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property> and
/// <https://www.unicode.org/reports/tr44/#Joining_Type>.
pub(super) fn used_letter_spacing_for_text(text: &str, letter_spacing: f32) -> f32 {
    if letter_spacing == 0.0 || text.chars().any(character_has_joining_behavior) {
        0.0
    } else {
        letter_spacing
    }
}

pub(super) fn visual_ranges_for_line<B: parley::style::Brush>(
    line: parley::Line<'_, B>,
) -> Vec<BidiVisualRange> {
    let mut ranges = Vec::<BidiVisualRange>::new();
    for run in line.runs() {
        for cluster in run.visual_clusters() {
            push_visual_range(
                &mut ranges,
                cluster.text_range(),
                ResolvedBidiDirection::from_parley_cluster_is_rtl(cluster.is_rtl()),
            );
        }
    }
    if ranges.is_empty() {
        ranges.push(BidiVisualRange {
            range: line.text_range(),
            direction: ResolvedBidiDirection::Ltr,
        });
    }
    ranges
}

pub(super) fn push_visual_range(
    ranges: &mut Vec<BidiVisualRange>,
    range: Range<usize>,
    direction: ResolvedBidiDirection,
) {
    if range.is_empty() {
        return;
    }
    if let Some(previous) = ranges.last_mut()
        && previous.direction == direction
        && previous.range.end == range.start
    {
        previous.range.end = range.end;
        return;
    }
    ranges.push(BidiVisualRange { range, direction });
}
