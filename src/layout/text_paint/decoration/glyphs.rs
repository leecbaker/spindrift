use super::*;

/// Return shaped glyph advances positioned in line visual order.
///
/// CSS Text Decoration clips decorations in the coordinate space of the
/// rendered line, so this preserves run offsets and shaped glyph advances
/// instead of remeasuring flattened source text:
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
pub(in crate::layout) fn text_decoration_positioned_glyphs(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    line_y: f32,
    inline_start: f32,
    inline_length: f32,
    runs: &[RenderedTextRun],
) -> Vec<TextDecorationPositionedGlyph> {
    let inline_end = inline_start + inline_length;
    let mut positioned = Vec::new();
    for run in runs {
        let Some(glyphs) = &run.glyphs else {
            continue;
        };
        let mut pen_x = 0.0;
        for glyph in glyphs {
            let local_start = pen_x + glyph.x_offset;
            let local_end = (pen_x + glyph.x_advance).max(local_start);
            let (start, end) = text_decoration_glyph_inline_range(
                axis,
                line_x,
                line_y,
                run,
                local_start,
                local_end,
            );
            if end > inline_start && start < inline_end {
                positioned.push(TextDecorationPositionedGlyph {
                    unicode: glyph.unicode.clone(),
                    inline_start: start.max(inline_start),
                    inline_end: end.min(inline_end),
                    extra_spacing: (glyph.x_advance - glyph.nominal_x_advance).max(0.0),
                });
            }
            pen_x += glyph.x_advance;
        }
    }
    positioned
}

pub(in crate::layout) fn text_decoration_glyph_inline_range(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    line_y: f32,
    run: &RenderedTextRun,
    local_start: f32,
    local_end: f32,
) -> (f32, f32) {
    let (start, end) = match axis {
        TextDecorationStrokeAxis::Horizontal => {
            let start = run
                .text_matrix
                .transform_local_point(TextRunPoint::new(local_start, 0.0));
            let end = run
                .text_matrix
                .transform_local_point(TextRunPoint::new(local_end, 0.0));
            (
                line_x + run.x_offset + start.x,
                line_x + run.x_offset + end.x,
            )
        }
        TextDecorationStrokeAxis::Vertical if run.text_matrix.is_identity() => {
            let baseline = line_y + run.y_offset;
            (
                baseline - run.font_size * 0.5,
                baseline + run.font_size * 0.5,
            )
        }
        TextDecorationStrokeAxis::Vertical => {
            let start = run
                .text_matrix
                .transform_local_point(TextRunPoint::new(local_start, 0.0));
            let end = run
                .text_matrix
                .transform_local_point(TextRunPoint::new(local_end, 0.0));
            (
                line_y + run.y_offset + start.y,
                line_y + run.y_offset + end.y,
            )
        }
    };
    if start <= end {
        (start, end)
    } else {
        (end, start)
    }
}

pub(in crate::layout) fn text_decoration_glyph_is_spacer(unicode: &str) -> bool {
    !unicode.is_empty() && unicode.chars().all(character_is_text_decoration_spacer)
}

/// Resolve CSS `text-underline-offset` to a used offset.
///
/// CSS Text Decoration Level 4 defines underline offset as `auto` or a
/// length-percentage, applied away from the text in horizontal writing:
/// <https://www.w3.org/TR/css-text-decor-4/#text-underline-offset-property>.
pub(in crate::layout) fn used_text_underline_offset(
    offset: TextUnderlineOffset,
    font_size: f32,
) -> f32 {
    match offset {
        TextUnderlineOffset::Auto => 0.0,
        TextUnderlineOffset::LengthPercentage(value) => value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(font_size)))
            .map(layout_points)
            .unwrap_or(value.length_points()),
    }
}
