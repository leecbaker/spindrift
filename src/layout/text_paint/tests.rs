use std::rc::Rc;

use super::*;
use crate::css::ComputedLengthPercentage;

fn glyph(unicode: &str, advance: f32) -> RenderedGlyph {
    RenderedGlyph {
        kind: crate::document::paint::text::RenderedGlyphKind::Paint(1),
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
        CssColor::BLACK,
        vec![RenderedTextRun {
            text: Rc::from(text),
            actual_text: None,
            x_offset: 0.0,
            y_offset: run_y_offset,
            text_matrix: matrix,
            font_size: 10.0,
            font_id: None,
            font_palette: crate::css::FontPalette::Normal,
            glyphs: Some(glyphs.into()),
            glyph_source_ranges: None,
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
    let baseline = PaintPoint::new(10.0, 20.0);
    let inline_span = VerticalInlineAxis::for_style(style)
        .map(|axis| {
            axis.project_span_from_start(
                layout_pt(baseline.y),
                TextInlineSpan::from_start_and_length(0.0, 40.0),
            )
        })
        .unwrap_or_else(|| TextInlineSpan::from_start_and_length(baseline.x, 40.0));
    prepare_text_decoration_strokes(TextDecorationPreparationInput {
        baseline,
        inline_span,
        inset_start: 0.0,
        inset_end: 0.0,
        style,
        inset_style: style,
        inset_inline_axis: TextDecorationInlineAxis::for_style(style),
        decoration,
        phase,
        color: CssColor::BLACK,
        color_override: None,
        geometry: TextDecorationLineGeometry {
            origin_font_size: style.font_size,
            considered_font_size: style.font_size,
            considered_metrics: decoration_metrics(),
        },
    })
}

#[test]
fn decoration_uses_its_origin_font_size_for_auto_thickness() {
    let mut decorated_style = ComputedStyle::initial();
    decorated_style.font_size = 10.0;
    let mut origin_style = ComputedStyle::initial();
    origin_style.font_size = 32.0;
    let mut decoration = origin_style.text_decoration.clone();
    decoration.underline = true;

    let strokes = prepare_text_decoration_strokes(TextDecorationPreparationInput {
        baseline: PaintPoint::new(10.0, 20.0),
        inline_span: TextInlineSpan::from_start_and_length(10.0, 40.0),
        inset_start: 0.0,
        inset_end: 0.0,
        style: &decorated_style,
        inset_style: &origin_style,
        inset_inline_axis: TextDecorationInlineAxis::for_style(&origin_style),
        decoration,
        phase: TextDecorationPaintPhase::BeforeText,
        color: CssColor::BLACK,
        color_override: None,
        geometry: TextDecorationLineGeometry {
            origin_font_size: origin_style.font_size,
            considered_font_size: decorated_style.font_size,
            considered_metrics: decoration_metrics(),
        },
    });

    assert_eq!(strokes.len(), 1);
    assert!(
        (strokes[0].thickness - 0.625).abs() < 0.01,
        "automatic thickness must use the selected line's considered text"
    );
}

#[test]
fn prepared_decoration_horizontal_underline_extends_outward_from_zero_edge() {
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
    assert!((before[0].block_position - 18.375).abs() < 0.01);
    assert!((before[1].block_position - 30.0).abs() < 0.01);
    assert!((after[0].block_position - 23.0).abs() < 0.01);
}

#[test]
fn decoration_split_phases_select_underline_overline_and_line_through_independently() {
    let mut style = ComputedStyle::initial();
    style.font_size = 10.0;
    let mut decoration = style.text_decoration.clone();
    decoration.underline = true;
    decoration.overline = true;
    decoration.line_through = true;

    let underlines = prepared_decoration_strokes_for_style(
        &style,
        decoration.clone(),
        TextDecorationPaintPhase::Underlines,
    );
    let overlines = prepared_decoration_strokes_for_style(
        &style,
        decoration.clone(),
        TextDecorationPaintPhase::Overlines,
    );
    let line_throughs = prepared_decoration_strokes_for_style(
        &style,
        decoration,
        TextDecorationPaintPhase::AfterText,
    );

    assert_eq!(underlines.len(), 1);
    assert_eq!(overlines.len(), 1);
    assert_eq!(line_throughs.len(), 1);
    assert!(underlines[0].block_position < line_throughs[0].block_position);
    assert!(line_throughs[0].block_position < overlines[0].block_position);
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
    assert_eq!(
        VerticalInlineAxis::for_style(&style)
            .unwrap()
            .advance_sign(),
        -1.0
    );
    style.direction = Direction::Rtl;
    assert_eq!(
        VerticalInlineAxis::for_style(&style)
            .unwrap()
            .advance_sign(),
        1.0
    );

    style.writing_mode = WritingMode::SidewaysLr;
    style.direction = Direction::Ltr;
    assert_eq!(
        VerticalInlineAxis::for_style(&style)
            .unwrap()
            .advance_sign(),
        1.0
    );
    style.direction = Direction::Rtl;
    assert_eq!(
        VerticalInlineAxis::for_style(&style)
            .unwrap()
            .advance_sign(),
        -1.0
    );
}

#[test]
fn decoration_insets_follow_the_origin_logical_start_edge() {
    let cases = [
        (WritingMode::HorizontalTb, Direction::Ltr, 70.0, 105.0),
        (WritingMode::HorizontalTb, Direction::Rtl, 55.0, 90.0),
        (WritingMode::SidewaysRl, Direction::Ltr, 55.0, 90.0),
        (WritingMode::SidewaysRl, Direction::Rtl, 70.0, 105.0),
        (WritingMode::SidewaysLr, Direction::Ltr, 70.0, 105.0),
        (WritingMode::SidewaysLr, Direction::Rtl, 55.0, 90.0),
    ];
    for (writing_mode, direction, expected_start, expected_end) in cases {
        let mut style = ComputedStyle::initial();
        style.writing_mode = writing_mode;
        style.direction = direction;
        let inline_axis = TextDecorationInlineAxis::for_style(&style);
        let span = text_decoration_inline_span(
            inline_axis.stroke_axis(),
            TextInlineSpan::new(60.0, 100.0),
            10.0,
            -5.0,
            &style,
            inline_axis,
        )
        .unwrap();
        assert_eq!(
            span,
            TextInlineSpan::new(expected_start, expected_end),
            "{writing_mode:?} {direction:?}",
        );
    }
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
fn vertical_underline_offset_does_not_move_an_overline() {
    let mut style = ComputedStyle::initial();
    style.font_size = 10.0;
    style.writing_mode = WritingMode::VerticalLr;
    let mut decoration = style.text_decoration.clone();
    decoration.overline = true;

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

    assert_eq!(without_offset.len(), 1);
    assert_eq!(with_offset.len(), 1);
    assert!(
        (without_offset[0].block_position - with_offset[0].block_position).abs() < 0.01,
        "overline must ignore text-underline-offset: {without_offset:?} {with_offset:?}"
    );
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
        font_palette: crate::css::FontPalette::Normal,
        glyphs: Some(vec![glyph(" ", 10.0), glyph("A", 10.0)].into()),
        glyph_source_ranges: None,
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
fn skip_ink_auto_keeps_a_short_stroke_when_ink_covers_its_entire_span() {
    let inputs = TextDecorationSegmentInputs {
        axis: TextDecorationStrokeAxis::Horizontal,
        line_x: 0.0,
        line_y: 0.0,
        inline_start: 0.0,
        inline_length: 10.0,
        block_position: 0.0,
        thickness: 10.0,
        skip_ink: TextDecorationSkipInk::Auto,
        skip_spaces: TextDecorationSkipSpaces::NONE,
    };
    let ink = [GlyphInkBox {
        x_min: 0.0,
        x_max: 10.0,
        y_min: 0.0,
        y_max: 10.0,
    }];

    let segments = text_decoration_segments(inputs, &[], &ink);

    assert_eq!(segments.len(), 1);
    assert!((segments[0].start - 0.0).abs() < 0.01, "{segments:?}");
    assert!((segments[0].length - 10.0).abs() < 0.01, "{segments:?}");
}

#[test]
fn origin_wide_decoration_preserves_receiver_gaps() {
    let receiver_spans = [
        TextInlineSpan::new(0.0, 20.0),
        TextInlineSpan::new(35.0, 60.0),
    ];
    let segments = text_decoration_segments_with_selected_glyphs(
        TextDecorationSegmentInputs {
            axis: TextDecorationStrokeAxis::Horizontal,
            line_x: 0.0,
            line_y: 0.0,
            inline_start: 0.0,
            inline_length: 60.0,
            block_position: 0.0,
            thickness: 1.0,
            skip_ink: TextDecorationSkipInk::None,
            skip_spaces: TextDecorationSkipSpaces::NONE,
        },
        &[],
        &[],
        None,
        None,
        Some(&receiver_spans),
    );

    assert_eq!(segments.len(), 2, "{segments:?}");
    assert!((segments[0].start - 0.0).abs() < 0.01, "{segments:?}");
    assert!((segments[0].length - 20.0).abs() < 0.01, "{segments:?}");
    assert!((segments[1].start - 35.0).abs() < 0.01, "{segments:?}");
    assert!((segments[1].length - 25.0).abs() < 0.01, "{segments:?}");
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
            .any(|stroke| stroke.color == CssColor::new(255, 0, 0))
    );
    assert!(
        strokes
            .iter()
            .any(|stroke| stroke.color == CssColor::new(0, 128, 0))
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
