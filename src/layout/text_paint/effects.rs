use super::*;

pub(in crate::layout) struct TextShadowPaintPass {
    /// Blur-sample displacement in bottom-left page paint space.
    pub(in crate::layout) offset: PaintDisplacement,
    pub(in crate::layout) color: Color,
}

/// Build vector replay passes for a CSS `text-shadow`.
///
/// CSS Text Decoration Level 3 defines shadow layers as applying to the
/// composited text and decoration ink. PDF has no portable text-blur primitive,
/// so blurred shadows are approximated by bounded translucent vector replays
/// while zero-blur shadows remain crisp single-pass text:
/// <https://www.w3.org/TR/css-text-decor-3/#text-shadow-property>.
pub(in crate::layout) fn text_shadow_paint_passes(
    shadow: crate::css::TextShadow,
    color: Color,
) -> Vec<TextShadowPaintPass> {
    if shadow.blur_radius.length_points() <= 0.0 {
        return vec![TextShadowPaintPass {
            offset: PaintDisplacement::zero(),
            color,
        }];
    }

    let radius = shadow.blur_radius.length_max_zero().points();
    let samples = [
        (0.0, 0.0, 0.22),
        (1.0, 0.0, 0.08),
        (-1.0, 0.0, 0.08),
        (0.0, 1.0, 0.08),
        (0.0, -1.0, 0.08),
        (0.707, 0.707, 0.06),
        (-0.707, 0.707, 0.06),
        (0.707, -0.707, 0.06),
        (-0.707, -0.707, 0.06),
        (1.0, 1.0, 0.04),
        (-1.0, 1.0, 0.04),
        (1.0, -1.0, 0.04),
        (-1.0, -1.0, 0.04),
    ];
    samples
        .into_iter()
        .map(|(x, y, alpha)| TextShadowPaintPass {
            offset: PaintDisplacement::new(x * radius * 0.45, y * radius * 0.45),
            color: color_with_alpha_factor(color, alpha),
        })
        .collect()
}

pub(in crate::layout) fn color_with_alpha_factor(color: Color, factor: f32) -> Color {
    Color {
        a: (color.a * factor).clamp(0.0, 1.0),
        ..color
    }
}

/// Build prepared CSS text-emphasis annotations for one rendered line.
///
/// CSS Text Decoration attaches one emphasis mark to each eligible
/// typographic character unit. Building annotation records before paint keeps
/// mark selection aligned with CSS Text unit policy and lets writing-mode
/// placement use the same positioned rendered runs as normal text:
/// <https://www.w3.org/TR/css-text-decor-3/#text-emphasis-style-property> and
/// <https://www.w3.org/TR/css-text-3/#typographic-character-unit>.
pub(in crate::layout) fn prepared_text_emphasis_marks_for_line(
    line: &RenderedLine,
    style: &ComputedStyle,
    mark: &str,
    mark_width: f32,
) -> Vec<PreparedTextEmphasisMark> {
    if mark.is_empty() {
        return Vec::new();
    }
    let mut marks = Vec::new();
    for run in &line.runs {
        let Some(glyphs) = &run.glyphs else {
            continue;
        };
        for unit in rendered_text_run_typographic_units(&run.text, glyphs) {
            if !text_emphasis_unit_receives_mark(&unit.text, style.text_emphasis_skip) {
                continue;
            }
            let position = text_emphasis_mark_position(line, run, &unit, style, mark_width);
            marks.push(PreparedTextEmphasisMark {
                mark: mark.to_string(),
                #[cfg(test)]
                source_text: unit.text,
                position,
            });
        }
    }
    marks
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct RenderedTextUnit {
    pub(in crate::layout) text: String,
    pub(in crate::layout) span: TextInlineSpan,
}

pub(in crate::layout) fn rendered_text_run_typographic_units(
    text: &str,
    glyphs: &[RenderedGlyph],
) -> Vec<RenderedTextUnit> {
    let unit_ranges = typographic_unit_ranges(text);
    let Some(first_range) = unit_ranges.first() else {
        return Vec::new();
    };
    let mut units = Vec::new();
    let mut unit_index = 0usize;
    let mut unit_end = first_range.end;
    let mut consumed_text_bytes = 0usize;
    let mut pending_text = String::new();
    let mut pending_start: Option<f32> = None;
    let mut pending_end = 0.0;
    let mut cursor = 0.0;

    for glyph in glyphs {
        if !glyph.unicode.is_empty() && pending_start.is_some() && consumed_text_bytes >= unit_end {
            push_rendered_text_unit(
                &mut units,
                &mut pending_text,
                &mut pending_start,
                &mut pending_end,
            );
            while unit_index + 1 < unit_ranges.len() && consumed_text_bytes >= unit_end {
                unit_index += 1;
                unit_end = unit_ranges[unit_index].end;
            }
        }

        let glyph_start = cursor + glyph.x_offset;
        let glyph_end = glyph_start + glyph.x_advance;
        pending_start = Some(pending_start.map_or(glyph_start, |start| start.min(glyph_start)));
        pending_end = pending_end.max(glyph_end);
        if !glyph.unicode.is_empty() {
            consumed_text_bytes += glyph.unicode.len();
            pending_text.push_str(&glyph.unicode);
        }
        cursor += glyph.x_advance;
    }

    push_rendered_text_unit(
        &mut units,
        &mut pending_text,
        &mut pending_start,
        &mut pending_end,
    );
    units
}

pub(in crate::layout) fn push_rendered_text_unit(
    units: &mut Vec<RenderedTextUnit>,
    text: &mut String,
    start: &mut Option<f32>,
    end: &mut f32,
) {
    let Some(start_value) = start.take() else {
        return;
    };
    units.push(RenderedTextUnit {
        text: std::mem::take(text),
        span: TextInlineSpan::new(start_value, *end),
    });
    *end = 0.0;
}

pub(in crate::layout) fn text_emphasis_unit_receives_mark(
    text: &str,
    skip: TextEmphasisSkip,
) -> bool {
    text.chars()
        .find(|character| {
            !character_is_unicode_mark(*character)
                && !character_is_default_ignorable_code_point(*character)
        })
        .is_some_and(|character| character_receives_text_emphasis_mark_with_skip(character, skip))
}

pub(in crate::layout) fn text_emphasis_mark_position(
    line: &RenderedLine,
    run: &RenderedTextRun,
    unit: &RenderedTextUnit,
    style: &ComputedStyle,
    mark_width: f32,
) -> PaintPoint {
    let vertical = style.writing_mode != WritingMode::HorizontalTb;
    if !vertical {
        let center = (unit.span.start + unit.span.end) / 2.0;
        let x = line.x() + run.x_offset + center - mark_width / 2.0;
        let y = if style.text_emphasis_position.over {
            line.y() + style.font_size * 0.55
        } else {
            line.y() - style.font_size * 0.35
        };
        return PaintPoint::new(x, y);
    }

    let side = if style.text_emphasis_position.right {
        css::line_over_side(style.writing_mode)
    } else {
        css::line_under_side(style.writing_mode)
    };
    let side_offset = match side {
        PhysicalSide::Right => style.font_size * 0.55,
        PhysicalSide::Left => -style.font_size * 0.55 - mark_width,
        PhysicalSide::Top | PhysicalSide::Bottom => 0.0,
    };
    let inline_anchor = if run.text_matrix.is_identity() {
        unit.span.start
    } else {
        (unit.span.start + unit.span.end) / 2.0
    };
    let position = transformed_text_run_point(line, run, TextRunPoint::new(inline_anchor, 0.0));
    PaintPoint::new(position.x + side_offset, position.y)
}

pub(in crate::layout) fn transformed_text_run_point(
    line: &RenderedLine,
    run: &RenderedTextRun,
    local_point: TextRunPoint,
) -> PaintPoint {
    let local_point = run.text_matrix.transform_local_point(local_point);
    PaintPoint::new(
        line.x() + run.x_offset + local_point.x,
        line.y() + run.y_offset + local_point.y,
    )
}

pub(in crate::layout) fn character_receives_text_emphasis_mark_with_skip(
    character: char,
    skip: TextEmphasisSkip,
) -> bool {
    if !character_receives_text_emphasis_mark(character) {
        return false;
    }
    if skip.spaces && character_is_text_decoration_spacer(character) {
        return false;
    }
    if skip.punctuation && character_is_unicode_punctuation(character) {
        return false;
    }
    if skip.symbols && character_is_unicode_symbol(character) {
        return false;
    }
    if skip.narrow && character.is_ascii() {
        return false;
    }
    true
}
