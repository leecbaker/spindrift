use super::*;

/// Resolve CSS `text-decoration-thickness` to a used line thickness.
///
/// CSS Text Decoration Level 4 defines `auto`, `from-font`, and
/// length-percentage values. `from-font` uses OpenType decoration metrics when
/// available:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-width-property>.
pub(in crate::layout) fn used_text_decoration_thickness(
    thickness: TextDecorationThickness,
    font_size: f32,
    metrics: &TextDecorationFontMetrics,
    line_through: bool,
) -> f32 {
    match thickness {
        TextDecorationThickness::Auto => (font_size / 16.0).max(0.5),
        TextDecorationThickness::FromFont if line_through => metrics.strikeout_thickness,
        TextDecorationThickness::FromFont => metrics.underline_thickness,
        TextDecorationThickness::LengthPercentage(value) => value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(font_size)))
            .map(layout_points)
            .unwrap_or(value.length_points())
            .max(0.5),
    }
}

/// Resolve the CSS underline baseline position for horizontal writing.
///
/// `text-underline-position: under` places the underline below descenders in
/// horizontal writing; vertical-writing side placement is handled separately
/// once vertical text layout exists:
/// <https://www.w3.org/TR/css-text-decor-3/#text-underline-position-property>.
pub(in crate::layout) fn used_underline_y(
    baseline_y: f32,
    position: TextUnderlinePosition,
    offset: TextUnderlineOffset,
    font_size: f32,
    metrics: &TextDecorationFontMetrics,
    thickness: f32,
) -> f32 {
    let font_position = metrics.underline_position;
    let under_position = -metrics.descender_depth - thickness;
    let base_offset = if position.under {
        font_position.min(under_position)
    } else {
        font_position
    };
    baseline_y + base_offset - used_text_underline_offset(offset, font_size)
}

/// Split a text-decoration stroke around skipped spaces and glyph ink.
///
/// CSS Text Decoration Level 4 defines both `text-decoration-skip-spaces` and
/// `text-decoration-skip-ink` as clipping behavior applied to decoration
/// strokes:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>
/// and
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-skip-ink-property>.
pub(in crate::layout) fn text_decoration_segments(
    inputs: TextDecorationSegmentInputs,
    runs: &[RenderedTextRun],
    ink_boxes: &[GlyphInkBox],
) -> Vec<TextDecorationSegment> {
    let TextDecorationSegmentInputs {
        axis,
        line_x,
        line_y,
        inline_start,
        inline_length,
        block_position,
        thickness,
        skip_ink,
        skip_spaces,
    } = inputs;
    if inline_length <= 0.0 {
        return Vec::new();
    }

    let inline_end = inline_start + inline_length;
    let padding = thickness.max(0.5);
    let mut skips = text_decoration_space_skip_ranges(
        axis,
        line_x,
        line_y,
        inline_start,
        inline_length,
        skip_spaces,
        runs,
    );
    if skip_ink != TextDecorationSkipInk::None {
        skips.extend(
            ink_boxes
                .iter()
                .filter(|ink| {
                    text_decoration_ink_intersects_cross_axis(
                        axis,
                        line_x,
                        block_position,
                        thickness,
                        ink,
                    )
                })
                .filter_map(|ink| {
                    let (ink_start, ink_end) = text_decoration_ink_inline_range(axis, line_x, ink);
                    let start = (ink_start - padding).max(inline_start);
                    let end = (ink_end + padding).min(inline_end);
                    (end > start).then_some((start, end))
                }),
        );
    }
    if skips.is_empty() {
        return vec![TextDecorationSegment {
            start: inline_start,
            length: inline_length,
        }];
    }

    skips.sort_by(|left, right| {
        left.0
            .partial_cmp(&right.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut merged = Vec::<(f32, f32)>::new();
    for (start, end) in skips {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
            continue;
        }
        merged.push((start, end));
    }

    let mut segments = Vec::new();
    let mut cursor = inline_start;
    for (start, end) in merged {
        if start > cursor {
            segments.push(TextDecorationSegment {
                start: cursor,
                length: start - cursor,
            });
        }
        cursor = cursor.max(end);
    }
    if cursor < inline_end {
        segments.push(TextDecorationSegment {
            start: cursor,
            length: inline_end - cursor,
        });
    }
    segments
}

pub(in crate::layout) fn text_decoration_ink_intersects_cross_axis(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    block_position: f32,
    thickness: f32,
    ink: &GlyphInkBox,
) -> bool {
    match axis {
        TextDecorationStrokeAxis::Horizontal => {
            ink.y_min <= block_position + thickness && ink.y_max >= block_position
        }
        TextDecorationStrokeAxis::Vertical => {
            line_x + ink.x_min <= block_position + thickness && line_x + ink.x_max >= block_position
        }
    }
}

pub(in crate::layout) fn text_decoration_ink_inline_range(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    ink: &GlyphInkBox,
) -> (f32, f32) {
    match axis {
        TextDecorationStrokeAxis::Horizontal => (line_x + ink.x_min, line_x + ink.x_max),
        TextDecorationStrokeAxis::Vertical => (ink.y_min, ink.y_max),
    }
}

/// Return decoration clipping ranges for CSS `text-decoration-skip-spaces`.
///
/// CSS Text Decoration Level 4 defines spacers as Unicode `Zs` characters
/// except U+202F, and `all` also skips word separators plus adjacent
/// letter/word spacing. Shaped glyph advances are used here so bidi,
/// ligatures, fallback fonts, and letter spacing clip the painted decoration at
/// used-value positions:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>.
pub(in crate::layout) fn text_decoration_space_skip_ranges(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    line_y: f32,
    inline_start: f32,
    inline_length: f32,
    skip_spaces: TextDecorationSkipSpaces,
    runs: &[RenderedTextRun],
) -> Vec<(f32, f32)> {
    if inline_length <= 0.0 || skip_spaces == TextDecorationSkipSpaces::NONE {
        return Vec::new();
    }

    let glyphs =
        text_decoration_positioned_glyphs(axis, line_x, line_y, inline_start, inline_length, runs);
    if glyphs.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    if skip_spaces.skips_all() {
        for (index, glyph) in glyphs.iter().enumerate() {
            if !text_decoration_glyph_is_spacer(&glyph.unicode) {
                continue;
            }
            let previous_extra_spacing = if index > 0 {
                glyphs[index - 1].extra_spacing
            } else {
                0.0
            };
            let start = (glyph.inline_start - previous_extra_spacing).max(inline_start);
            let end = glyph.inline_end.min(inline_start + inline_length);
            if end > start {
                ranges.push((start, end));
            }
        }
        return ranges;
    }

    if skip_spaces.skips_line_start() {
        for glyph in &glyphs {
            if !text_decoration_glyph_is_spacer(&glyph.unicode) {
                break;
            }
            let start = glyph.inline_start.max(inline_start);
            let end = glyph.inline_end.min(inline_start + inline_length);
            if end > start {
                ranges.push((start, end));
            }
        }
    }

    if skip_spaces.skips_line_end() {
        let mut trailing = Vec::new();
        for index in (0..glyphs.len()).rev() {
            let glyph = &glyphs[index];
            if !text_decoration_glyph_is_spacer(&glyph.unicode) {
                break;
            }
            let previous_extra_spacing = if index > 0 && trailing.is_empty() {
                glyphs[index - 1].extra_spacing
            } else {
                0.0
            };
            trailing.push((
                (glyph.inline_start - previous_extra_spacing).max(inline_start),
                glyph.inline_end.min(inline_start + inline_length),
            ));
        }
        for (start, end) in trailing.into_iter().rev() {
            if end > start {
                ranges.push((start, end));
            }
        }
    }

    ranges
}
