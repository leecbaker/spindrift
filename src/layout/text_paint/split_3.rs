use super::*;

pub(in crate::layout) fn rendered_text_line_width(line: &RenderedLine) -> f32 {
    line.runs.iter().fold(0.0_f32, |width, run| {
        let run_width = if run.text_matrix.is_identity() {
            run.glyphs
                .as_ref()
                .map(|glyphs| glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>())
                .unwrap_or_else(|| run.text.chars().count() as f32 * run.font_size * 0.5)
        } else {
            run.font_size
        };
        width.max(run.x_offset + run_width)
    })
}

pub(in crate::layout) fn positioned_rendered_runs_for_writing_mode(
    shaped: &ShapedInlineLine,
    style: &ComputedStyle,
) -> Vec<RenderedTextRun> {
    position_rendered_runs_for_writing_mode(shaped.rendered_runs(), style)
}

pub(in crate::layout) fn position_rendered_runs_for_writing_mode(
    runs: Vec<RenderedTextRun>,
    style: &ComputedStyle,
) -> Vec<RenderedTextRun> {
    if style.writing_mode == WritingMode::HorizontalTb {
        return runs;
    }
    let placement_direction = if matches!(style.text_orientation, TextOrientation::Upright) {
        Direction::Ltr
    } else {
        style.direction
    };
    let advance_sign = match placement_direction {
        Direction::Ltr => -1.0,
        Direction::Rtl => 1.0,
    };
    let sideways_matrix = match placement_direction {
        Direction::Ltr => RenderedTextMatrix::ROTATE_CW,
        Direction::Rtl => RenderedTextMatrix::ROTATE_CCW,
    };
    runs.into_iter()
        .flat_map(|run| {
            vertical_positioned_text_runs(
                run,
                style.text_orientation,
                advance_sign,
                sideways_matrix,
            )
        })
        .collect()
}

pub(in crate::layout) fn vertical_positioned_text_runs(
    mut run: RenderedTextRun,
    text_orientation: TextOrientation,
    advance_sign: f32,
    sideways_matrix: RenderedTextMatrix,
) -> Vec<RenderedTextRun> {
    let Some(glyphs) = run.glyphs.take() else {
        let text_matrix = if matches!(text_orientation, TextOrientation::Upright) {
            RenderedTextMatrix::IDENTITY
        } else {
            sideways_matrix
        };
        return vec![RenderedTextRun {
            y_offset: advance_sign * run.x_offset,
            text_matrix,
            glyphs: None,
            ..run
        }];
    };
    if glyphs.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::new();
    let mut pending_sideways: Option<RenderedTextRun> = None;
    let mut cursor = run.x_offset;
    let mut cluster_text = String::new();
    let mut cluster_glyphs = Vec::new();
    let mut consumed_text_bytes = 0usize;
    let unit_ends = typographic_unit_ranges(&run.text)
        .into_iter()
        .map(|range| range.end)
        .collect::<Vec<_>>();
    let mut unit_index = 0usize;
    let mut cluster_end = unit_ends.first().copied().unwrap_or(run.text.len());
    for glyph in glyphs {
        if !glyph.unicode.is_empty()
            && !cluster_glyphs.is_empty()
            && consumed_text_bytes >= cluster_end
        {
            flush_vertical_cluster(
                &run,
                &mut output,
                &mut pending_sideways,
                cursor,
                text_orientation,
                advance_sign,
                sideways_matrix,
                std::mem::take(&mut cluster_text),
                std::mem::take(&mut cluster_glyphs),
            );
            while unit_index + 1 < unit_ends.len() && consumed_text_bytes >= cluster_end {
                unit_index += 1;
                cluster_end = unit_ends[unit_index];
            }
        }
        if !glyph.unicode.is_empty() {
            consumed_text_bytes += glyph.unicode.len();
            cluster_text.push_str(&glyph.unicode);
        }
        cursor += glyph.x_advance;
        cluster_glyphs.push(glyph);
    }
    if !cluster_glyphs.is_empty() {
        flush_vertical_cluster(
            &run,
            &mut output,
            &mut pending_sideways,
            cursor,
            text_orientation,
            advance_sign,
            sideways_matrix,
            cluster_text,
            cluster_glyphs,
        );
    }
    if let Some(run) = pending_sideways {
        output.push(run);
    }
    output
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn flush_vertical_cluster(
    source: &RenderedTextRun,
    output: &mut Vec<RenderedTextRun>,
    pending_sideways: &mut Option<RenderedTextRun>,
    cursor_after_cluster: f32,
    text_orientation: TextOrientation,
    advance_sign: f32,
    sideways_matrix: RenderedTextMatrix,
    text: String,
    glyphs: Vec<RenderedGlyph>,
) {
    let advance = glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>();
    let cluster_start = cursor_after_cluster - advance;
    if vertical_text_cluster_is_upright(text_orientation, &text) {
        if let Some(run) = pending_sideways.take() {
            output.push(run);
        }
        output.push(RenderedTextRun {
            text,
            x_offset: 0.0,
            y_offset: advance_sign * cluster_start,
            text_matrix: RenderedTextMatrix::IDENTITY,
            font_size: source.font_size,
            font_id: source.font_id,
            glyphs: Some(glyphs),
        });
        return;
    }
    match pending_sideways {
        Some(run) => {
            run.text.push_str(&text);
            if let Some(existing_glyphs) = &mut run.glyphs {
                existing_glyphs.extend(glyphs);
            }
        }
        None => {
            *pending_sideways = Some(RenderedTextRun {
                text,
                x_offset: 0.0,
                y_offset: advance_sign * cluster_start,
                text_matrix: sideways_matrix,
                font_size: source.font_size,
                font_id: source.font_id,
                glyphs: Some(glyphs),
            });
        }
    }
}

/// Return whether a shaped text cluster is painted upright in vertical writing.
///
/// CSS Writing Modes defines `text-orientation` as the policy for orienting
/// typographic character units in vertical lines. `mixed` uses Unicode
/// Vertical_Orientation through the shared text property policy:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
pub(in crate::layout) fn vertical_text_cluster_is_upright(
    text_orientation: TextOrientation,
    text: &str,
) -> bool {
    match text_orientation {
        TextOrientation::Sideways => false,
        TextOrientation::Upright => !text.is_empty(),
        TextOrientation::Mixed => typographic_unit_is_upright_in_mixed_orientation(text),
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextShadowPaintPass {
    pub(in crate::layout) x_offset: f32,
    pub(in crate::layout) y_offset: f32,
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
            x_offset: 0.0,
            y_offset: 0.0,
            color,
        }];
    }

    let radius = shadow.blur_radius.length_points_max_zero();
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
            x_offset: x * radius * 0.45,
            y_offset: y * radius * 0.45,
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
            let (x, y) = text_emphasis_mark_position(line, run, &unit, style, mark_width);
            marks.push(PreparedTextEmphasisMark {
                mark: mark.to_string(),
                source_text: unit.text,
                x,
                y,
            });
        }
    }
    marks
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct RenderedTextUnit {
    pub(in crate::layout) text: String,
    pub(in crate::layout) start: f32,
    pub(in crate::layout) end: f32,
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
        start: start_value,
        end: *end,
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
) -> (f32, f32) {
    let vertical = style.writing_mode != WritingMode::HorizontalTb;
    if !vertical {
        let center = (unit.start + unit.end) / 2.0;
        let x = line.x() + run.x_offset + center - mark_width / 2.0;
        let y = if style.text_emphasis_position.over {
            line.y() + style.font_size * 0.55
        } else {
            line.y() - style.font_size * 0.35
        };
        return (x, y);
    }

    let side_offset = if style.text_emphasis_position.right {
        style.font_size * 0.55
    } else {
        -style.font_size * 0.55 - mark_width
    };
    let inline_anchor = if run.text_matrix.is_identity() {
        unit.start
    } else {
        (unit.start + unit.end) / 2.0
    };
    let (x, y) = transformed_text_run_point(line, run, inline_anchor, 0.0);
    (x + side_offset, y)
}

pub(in crate::layout) fn transformed_text_run_point(
    line: &RenderedLine,
    run: &RenderedTextRun,
    x: f32,
    y: f32,
) -> (f32, f32) {
    (
        line.x() + run.x_offset + run.text_matrix.a * x + run.text_matrix.c * y,
        line.y() + run.y_offset + run.text_matrix.b * x + run.text_matrix.d * y,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::ComputedLengthPercentage;

    fn glyph(unicode: &str, advance: f32) -> RenderedGlyph {
        RenderedGlyph {
            id: 1,
            x_advance: advance,
            nominal_x_advance: advance,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: unicode.to_string(),
        }
    }

    fn rendered_line_with_run(
        text: &str,
        glyphs: Vec<RenderedGlyph>,
        run_y_offset: f32,
        matrix: RenderedTextMatrix,
    ) -> RenderedLine {
        RenderedLine::from_paint_origin(
            text.to_string(),
            paint_space_point(10.0, 20.0),
            10.0,
            None,
            Color::BLACK,
            vec![RenderedTextRun {
                text: text.to_string(),
                x_offset: 0.0,
                y_offset: run_y_offset,
                text_matrix: matrix,
                font_size: 10.0,
                font_id: None,
                glyphs: Some(glyphs),
            }],
        )
    }

    fn decoration_metrics() -> TextDecorationFontMetrics {
        TextDecorationFontMetrics {
            underline_position: -1.0,
            underline_thickness: 2.0,
            strikeout_position: 3.0,
            strikeout_thickness: 1.5,
            descender_depth: 2.0,
        }
    }

    fn prepared_decoration_strokes_for_style(
        style: &ComputedStyle,
        decoration: TextDecoration,
        phase: TextDecorationPaintPhase,
    ) -> Vec<PreparedTextDecorationStroke> {
        prepare_text_decoration_strokes(TextDecorationPreparationInput {
            x: 10.0,
            baseline_y: 20.0,
            width: 40.0,
            inset_start: 0.0,
            inset_end: 0.0,
            style,
            decoration,
            phase,
            color: Color::BLACK,
            color_override: None,
            metrics: decoration_metrics(),
        })
    }

    #[test]
    fn prepared_decoration_horizontal_positions_match_legacy_offsets() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        let mut decoration = style.text_decoration;
        decoration.underline = true;
        decoration.overline = true;
        decoration.line_through = true;

        let before = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );
        let after = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::AfterText,
        );

        assert_eq!(before.len(), 2);
        assert_eq!(after.len(), 1);
        assert_eq!(before[0].axis, TextDecorationStrokeAxis::Horizontal);
        assert!((before[0].inline_start - 10.0).abs() < 0.01);
        assert!((before[0].inline_length - 40.0).abs() < 0.01);
        assert!((before[0].block_position - 19.0).abs() < 0.01);
        assert!((before[1].block_position - 30.0).abs() < 0.01);
        assert!((after[0].block_position - 23.0).abs() < 0.01);
    }

    #[test]
    fn prepared_decoration_vertical_underline_resolves_to_logical_side() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        style.writing_mode = WritingMode::VerticalRl;
        let mut decoration = style.text_decoration;
        decoration.underline = true;
        decoration.underline_position.left = true;
        decoration.underline_position.auto = false;

        let left = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );
        decoration.underline_position.left = false;
        decoration.underline_position.right = true;
        let right = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );

        assert_eq!(left.len(), 1);
        assert_eq!(right.len(), 1);
        assert_eq!(left[0].axis, TextDecorationStrokeAxis::Vertical);
        assert!((left[0].inline_start + 20.0).abs() < 0.01, "{left:?}");
        assert!(left[0].block_position < 10.0, "{left:?}");
        assert!(right[0].block_position > 10.0, "{right:?}");
    }

    #[test]
    fn prepared_decoration_vertical_offset_moves_away_from_text() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        style.writing_mode = WritingMode::VerticalRl;
        let mut decoration = style.text_decoration;
        decoration.underline = true;
        decoration.underline_position.left = true;
        decoration.underline_position.auto = false;

        let without_offset = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );
        decoration.underline_offset =
            TextUnderlineOffset::LengthPercentage(ComputedLengthPercentage::from_points(4.0));
        let with_offset = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );

        assert!(with_offset[0].block_position < without_offset[0].block_position);
    }

    #[test]
    fn prepared_decoration_skip_spaces_uses_rotated_run_offsets() {
        let runs = vec![RenderedTextRun {
            text: " A".to_string(),
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: RenderedTextMatrix::ROTATE_CW,
            font_size: 10.0,
            font_id: None,
            glyphs: Some(vec![glyph(" ", 10.0), glyph("A", 10.0)]),
        }];

        let ranges = text_decoration_space_skip_ranges(
            TextDecorationStrokeAxis::Vertical,
            10.0,
            100.0,
            80.0,
            30.0,
            TextDecorationSkipSpaces::START_END,
            &runs,
        );

        assert_eq!(ranges.len(), 1);
        assert!((ranges[0].0 - 90.0).abs() < 0.01, "{ranges:?}");
        assert!((ranges[0].1 - 100.0).abs() < 0.01, "{ranges:?}");
    }

    #[test]
    fn prepared_decoration_errors_use_wavy_annotation_path() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        let mut decoration = style.text_decoration;
        decoration.spelling_error = true;
        decoration.grammar_error = true;

        let strokes = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );

        assert_eq!(strokes.len(), 2);
        assert!(
            strokes
                .iter()
                .all(|stroke| stroke.style == TextDecorationStyle::Wavy)
        );
        assert!(
            strokes
                .iter()
                .any(|stroke| stroke.color == Color::new(255, 0, 0))
        );
        assert!(
            strokes
                .iter()
                .any(|stroke| stroke.color == Color::new(0, 128, 0))
        );
    }

    #[test]
    fn prepared_emphasis_annotations_use_typographic_units() {
        let text = "e\u{301}A";
        let line = rendered_line_with_run(
            text,
            vec![glyph("e", 8.0), glyph("\u{301}", 0.0), glyph("A", 10.0)],
            0.0,
            RenderedTextMatrix::IDENTITY,
        );
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;

        let marks = prepared_text_emphasis_marks_for_line(&line, &style, "•", 2.0);

        assert_eq!(
            marks
                .iter()
                .map(|mark| mark.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["e\u{301}", "A"]
        );
        assert_eq!(marks.len(), 2);
        assert!((marks[0].x - 13.0).abs() < 0.01, "{marks:?}");
        assert!((marks[1].x - 22.0).abs() < 0.01, "{marks:?}");
    }

    #[test]
    fn prepared_emphasis_annotations_apply_unicode_skip_policy() {
        let text = "A!★";
        let line = rendered_line_with_run(
            text,
            vec![glyph("A", 10.0), glyph("!", 10.0), glyph("★", 10.0)],
            0.0,
            RenderedTextMatrix::IDENTITY,
        );
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;

        let default_marks = prepared_text_emphasis_marks_for_line(&line, &style, "•", 2.0);
        assert_eq!(
            default_marks
                .iter()
                .map(|mark| mark.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "★"]
        );

        style.text_emphasis_skip.symbols = true;
        let symbol_skipping_marks = prepared_text_emphasis_marks_for_line(&line, &style, "•", 2.0);
        assert_eq!(
            symbol_skipping_marks
                .iter()
                .map(|mark| mark.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["A"]
        );

        let punctuation_with_mark = rendered_line_with_run(
            "!\u{301}",
            vec![glyph("!", 10.0), glyph("\u{301}", 0.0)],
            0.0,
            RenderedTextMatrix::IDENTITY,
        );
        let marks = prepared_text_emphasis_marks_for_line(&punctuation_with_mark, &style, "•", 2.0);
        assert!(marks.is_empty(), "{marks:?}");
    }

    #[test]
    fn prepared_vertical_emphasis_uses_logical_side_and_run_offset() {
        let line = rendered_line_with_run(
            "中",
            vec![glyph("中", 10.0)],
            -12.0,
            RenderedTextMatrix::IDENTITY,
        );
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        style.writing_mode = WritingMode::VerticalRl;

        let right_marks = prepared_text_emphasis_marks_for_line(&line, &style, "﹅", 2.0);
        style.text_emphasis_position.right = false;
        let left_marks = prepared_text_emphasis_marks_for_line(&line, &style, "﹅", 2.0);

        assert_eq!(right_marks.len(), 1);
        assert_eq!(left_marks.len(), 1);
        assert!(
            right_marks[0].x > left_marks[0].x,
            "{right_marks:?} {left_marks:?}"
        );
        assert!((right_marks[0].y - 8.0).abs() < 0.01, "{right_marks:?}");
        assert!((left_marks[0].y - 8.0).abs() < 0.01, "{left_marks:?}");
    }
}

pub(in crate::layout) fn active_text_decoration_layers(
    style: &ComputedStyle,
) -> Vec<TextDecoration> {
    if !style.text_decoration_layers.is_empty() {
        return style.text_decoration_layers.clone();
    }
    if style.text_decoration.has_visible_line() {
        return vec![style.text_decoration];
    }
    Vec::new()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TextDecorationPaintPhase {
    BeforeText,
    AfterText,
    All,
}

impl TextDecorationPaintPhase {
    pub(in crate::layout) fn paints_before_text(self) -> bool {
        matches!(self, Self::BeforeText | Self::All)
    }

    pub(in crate::layout) fn paints_after_text(self) -> bool {
        matches!(self, Self::AfterText | Self::All)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TextDecorationLineKind {
    Underline,
    Overline,
    LineThrough,
}

pub(in crate::layout) fn text_decoration_skip_self_suppresses(
    style: &ComputedStyle,
    line: TextDecorationLineKind,
) -> bool {
    match style.text_decoration.skip_self {
        TextDecorationSkipSelf::Auto | TextDecorationSkipSelf::NoSkip => false,
        TextDecorationSkipSelf::SkipAll => true,
        TextDecorationSkipSelf::Lines {
            underline,
            overline,
            line_through,
        } => match line {
            TextDecorationLineKind::Underline => underline,
            TextDecorationLineKind::Overline => overline,
            TextDecorationLineKind::LineThrough => line_through,
        },
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextDecorationPreparationInput<'a> {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) baseline_y: f32,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) inset_start: f32,
    pub(in crate::layout) inset_end: f32,
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) decoration: TextDecoration,
    pub(in crate::layout) phase: TextDecorationPaintPhase,
    pub(in crate::layout) color: Color,
    pub(in crate::layout) color_override: Option<Color>,
    pub(in crate::layout) metrics: TextDecorationFontMetrics,
}
