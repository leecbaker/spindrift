use super::*;
use crate::css::ComputedLengthPercentage;
use std::rc::Rc;

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
            text: Rc::from(text),
            actual_text: None,
            x_offset: 0.0,
            y_offset: run_y_offset,
            text_matrix: matrix,
            font_size: 10.0,
            font_id: None,
            glyphs: Some(glyphs.into()),
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
        baseline: PaintPoint::new(10.0, 20.0),
        inline_span: TextInlineSpan::from_start_and_length(10.0, 40.0),
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
    let mut decoration = style.text_decoration.clone();
    decoration.underline = true;
    decoration.overline = true;
    decoration.line_through = true;

    let before = prepared_decoration_strokes_for_style(
        &style,
        decoration.clone(),
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
    assert!((before[0].inline_span.start - 10.0).abs() < 0.01);
    assert!((before[0].inline_span.length() - 40.0).abs() < 0.01);
    assert!((before[0].block_position - 19.0).abs() < 0.01);
    assert!((before[1].block_position - 30.0).abs() < 0.01);
    assert!((after[0].block_position - 23.0).abs() < 0.01);
}

#[test]
fn prepared_decoration_vertical_underline_resolves_to_logical_side() {
    let mut style = ComputedStyle::initial();
    style.font_size = 10.0;
    style.writing_mode = WritingMode::VerticalRl;
    let mut decoration = style.text_decoration.clone();
    decoration.underline = true;
    decoration.underline_position.left = true;
    decoration.underline_position.auto = false;

    let left = prepared_decoration_strokes_for_style(
        &style,
        decoration.clone(),
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
    assert!((left[0].inline_span.start + 20.0).abs() < 0.01, "{left:?}");
    assert!(left[0].block_position < 10.0, "{left:?}");
    assert!(right[0].block_position > 10.0, "{right:?}");
}

#[test]
fn sideways_auto_decorations_follow_line_under_and_over_sides() {
    let position = TextUnderlinePosition::AUTO;
    assert_eq!(
        resolve_vertical_underline_side(position, WritingMode::SidewaysRl),
        TextDecorationSide::Left
    );
    assert_eq!(
        resolve_vertical_underline_side(position, WritingMode::SidewaysLr),
        TextDecorationSide::Right
    );
}

#[test]
fn sideways_inline_advance_uses_its_directional_inline_start_side() {
    let mut style = ComputedStyle::initial();
    style.writing_mode = WritingMode::SidewaysRl;
    style.direction = Direction::Ltr;
    assert_eq!(vertical_text_advance_sign(&style), -1.0);
    style.direction = Direction::Rtl;
    assert_eq!(vertical_text_advance_sign(&style), 1.0);

    style.writing_mode = WritingMode::SidewaysLr;
    style.direction = Direction::Ltr;
    assert_eq!(vertical_text_advance_sign(&style), 1.0);
    style.direction = Direction::Rtl;
    assert_eq!(vertical_text_advance_sign(&style), -1.0);
}

#[test]
fn prepared_decoration_vertical_offset_moves_away_from_text() {
    let mut style = ComputedStyle::initial();
    style.font_size = 10.0;
    style.writing_mode = WritingMode::VerticalRl;
    let mut decoration = style.text_decoration.clone();
    decoration.underline = true;
    decoration.underline_position.left = true;
    decoration.underline_position.auto = false;

    let without_offset = prepared_decoration_strokes_for_style(
        &style,
        decoration.clone(),
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
        text: Rc::from(" A"),
        actual_text: None,
        x_offset: 0.0,
        y_offset: 0.0,
        text_matrix: RenderedTextMatrix::ROTATE_CW,
        font_size: 10.0,
        font_id: None,
        glyphs: Some(vec![glyph(" ", 10.0), glyph("A", 10.0)].into()),
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
    let mut decoration = style.text_decoration.clone();
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
    assert!((marks[0].position.x - 13.0).abs() < 0.01, "{marks:?}");
    assert!((marks[1].position.x - 22.0).abs() < 0.01, "{marks:?}");
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
        right_marks[0].position.x > left_marks[0].position.x,
        "{right_marks:?} {left_marks:?}"
    );
    assert!(
        (right_marks[0].position.y - 8.0).abs() < 0.01,
        "{right_marks:?}"
    );
    assert!(
        (left_marks[0].position.y - 8.0).abs() < 0.01,
        "{left_marks:?}"
    );
}

#[test]
fn sideways_lr_emphasis_reverses_the_default_line_right_side() {
    let line = rendered_line_with_run(
        "A",
        vec![glyph("A", 10.0)],
        0.0,
        RenderedTextMatrix::ROTATE_CCW,
    );
    let mut style = ComputedStyle::initial();
    style.font_size = 10.0;
    style.writing_mode = WritingMode::SidewaysLr;

    let line_right = prepared_text_emphasis_marks_for_line(&line, &style, "•", 2.0);
    style.text_emphasis_position.right = false;
    let line_left = prepared_text_emphasis_marks_for_line(&line, &style, "•", 2.0);

    assert_eq!(line_right.len(), 1);
    assert_eq!(line_left.len(), 1);
    assert!(
        line_right[0].position.x < line_left[0].position.x,
        "sideways-lr line-right must be physical left: {line_right:?} {line_left:?}"
    );
}
