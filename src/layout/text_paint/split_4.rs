use super::*;

/// Prepare CSS text-decoration strokes for one rendered inline line.
///
/// CSS Text Decoration paints decoration lines relative to the decorated text's
/// inline axis. Preparing strokes in an axis-aware form before emitting PDF
/// primitives lets horizontal and vertical writing share the same skip and
/// style pipeline:
/// <https://www.w3.org/TR/css-text-decor-3/#line-decoration> and
/// <https://www.w3.org/TR/css-writing-modes-4/#line-directions>.
pub(in crate::layout) fn prepare_text_decoration_strokes(
    input: TextDecorationPreparationInput<'_>,
) -> Vec<PreparedTextDecorationStroke> {
    let TextDecorationPreparationInput {
        x,
        baseline_y,
        width,
        inset_start,
        inset_end,
        style,
        decoration,
        phase,
        color,
        color_override,
        metrics,
    } = input;
    if width <= 0.0 {
        return Vec::new();
    }

    let axis = if style.writing_mode == WritingMode::HorizontalTb {
        TextDecorationStrokeAxis::Horizontal
    } else {
        TextDecorationStrokeAxis::Vertical
    };
    let Some((inline_start, inline_length)) =
        text_decoration_inline_span(axis, x, baseline_y, width, inset_start, inset_end, style)
    else {
        return Vec::new();
    };

    let underline_thickness =
        used_text_decoration_thickness(decoration.thickness, style.font_size, &metrics, false);
    let strikeout_thickness =
        used_text_decoration_thickness(decoration.thickness, style.font_size, &metrics, true);
    let mut strokes = Vec::new();

    if phase.paints_before_text()
        && decoration.underline
        && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::Underline)
    {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Underline,
            ),
            thickness: underline_thickness,
            color,
            style: decoration.style,
            skip_ink: decoration.skip_ink,
            skip_spaces: decoration.skip_spaces,
        });
    }

    if phase.paints_before_text()
        && decoration.overline
        && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::Overline)
    {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Overline,
            ),
            thickness: underline_thickness,
            color,
            style: decoration.style,
            skip_ink: decoration.skip_ink,
            skip_spaces: decoration.skip_spaces,
        });
    }

    if phase.paints_after_text()
        && decoration.line_through
        && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::LineThrough)
    {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                strikeout_thickness,
                TextDecorationPreparedLineKind::LineThrough,
            ),
            thickness: strikeout_thickness,
            color,
            style: decoration.style,
            skip_ink: decoration.skip_ink,
            skip_spaces: decoration.skip_spaces,
        });
    }

    if phase.paints_before_text() && decoration.spelling_error {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Underline,
            ),
            thickness: underline_thickness,
            color: color_override.unwrap_or(Color::new(255, 0, 0)),
            style: TextDecorationStyle::Wavy,
            skip_ink: TextDecorationSkipInk::None,
            skip_spaces: TextDecorationSkipSpaces::NONE,
        });
    }

    if phase.paints_before_text() && decoration.grammar_error {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Underline,
            ),
            thickness: underline_thickness,
            color: color_override.unwrap_or(Color::new(0, 128, 0)),
            style: TextDecorationStyle::Wavy,
            skip_ink: TextDecorationSkipInk::None,
            skip_spaces: TextDecorationSkipSpaces::NONE,
        });
    }

    strokes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TextDecorationPreparedLineKind {
    Underline,
    Overline,
    LineThrough,
}

pub(in crate::layout) fn text_decoration_inline_span(
    axis: TextDecorationStrokeAxis,
    x: f32,
    baseline_y: f32,
    width: f32,
    inset_start: f32,
    inset_end: f32,
    style: &ComputedStyle,
) -> Option<(f32, f32)> {
    let length = (width - inset_start - inset_end).max(0.0);
    if length <= 0.0 {
        return None;
    }
    match axis {
        TextDecorationStrokeAxis::Horizontal => {
            let start = match style.direction {
                Direction::Ltr => x + inset_start,
                Direction::Rtl => x + inset_end,
            };
            Some((start, length))
        }
        TextDecorationStrokeAxis::Vertical => {
            if vertical_text_advance_sign(style) < 0.0 {
                Some((baseline_y - width + inset_end, length))
            } else {
                Some((baseline_y + inset_start, length))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn text_decoration_block_position(
    axis: TextDecorationStrokeAxis,
    x: f32,
    baseline_y: f32,
    style: &ComputedStyle,
    underline_position: TextUnderlinePosition,
    underline_offset: TextUnderlineOffset,
    metrics: &TextDecorationFontMetrics,
    thickness: f32,
    kind: TextDecorationPreparedLineKind,
) -> f32 {
    match axis {
        TextDecorationStrokeAxis::Horizontal => match kind {
            TextDecorationPreparedLineKind::Underline => used_underline_y(
                baseline_y,
                underline_position,
                underline_offset,
                style.font_size,
                metrics,
                thickness,
            ),
            TextDecorationPreparedLineKind::Overline => baseline_y + style.font_size,
            TextDecorationPreparedLineKind::LineThrough => baseline_y + metrics.strikeout_position,
        },
        TextDecorationStrokeAxis::Vertical => {
            let offset = used_text_underline_offset(underline_offset, style.font_size).max(0.0);
            match kind {
                TextDecorationPreparedLineKind::Underline => {
                    vertical_text_decoration_side_position(
                        x,
                        style,
                        resolve_vertical_underline_side(underline_position, style.writing_mode),
                        thickness,
                        offset,
                    )
                }
                TextDecorationPreparedLineKind::Overline => vertical_text_decoration_side_position(
                    x,
                    style,
                    opposite_text_decoration_side(resolve_vertical_underline_side(
                        underline_position,
                        style.writing_mode,
                    )),
                    thickness,
                    offset,
                ),
                TextDecorationPreparedLineKind::LineThrough => x + style.font_size * 0.5,
            }
        }
    }
}

pub(in crate::layout) fn vertical_text_advance_sign(style: &ComputedStyle) -> f32 {
    let placement_direction = if matches!(style.text_orientation, TextOrientation::Upright) {
        Direction::Ltr
    } else {
        style.direction
    };
    match placement_direction {
        Direction::Ltr => -1.0,
        Direction::Rtl => 1.0,
    }
}

pub(in crate::layout) fn resolve_vertical_underline_side(
    position: TextUnderlinePosition,
    writing_mode: WritingMode,
) -> TextDecorationSide {
    if position.left {
        return TextDecorationSide::Left;
    }
    if position.right {
        return TextDecorationSide::Right;
    }
    match writing_mode {
        WritingMode::HorizontalTb | WritingMode::VerticalRl => TextDecorationSide::Right,
        WritingMode::VerticalLr => TextDecorationSide::Left,
    }
}

pub(in crate::layout) fn opposite_text_decoration_side(
    side: TextDecorationSide,
) -> TextDecorationSide {
    match side {
        TextDecorationSide::Left => TextDecorationSide::Right,
        TextDecorationSide::Right => TextDecorationSide::Left,
    }
}

pub(in crate::layout) fn vertical_text_decoration_side_position(
    x: f32,
    style: &ComputedStyle,
    side: TextDecorationSide,
    thickness: f32,
    offset: f32,
) -> f32 {
    match side {
        TextDecorationSide::Left => x - thickness - offset,
        TextDecorationSide::Right => x + style.font_size + offset,
    }
}

/// Adjust inline fragment background/border painting for sliced inline boxes.
///
/// CSS Fragmentation defines `box-decoration-break: slice` as the initial
/// behavior: inline-start decorations are painted only on the first fragment,
/// inline-end decorations only on the last fragment, while top/bottom
/// decorations continue on every line fragment. CSS 2.2 positions non-replaced
/// inline padding and borders from the content-area edges, not from the
/// line-height box:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>,
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>, and
/// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>.
pub(in crate::layout) fn apply_inline_fragment_edge_painting(
    style: &mut ComputedStyle,
    edges: InlineHangingEdges,
    x: &mut f32,
    y: &mut f32,
    width: &mut f32,
    height: &mut f32,
) {
    let borders = used_border_widths(style);
    let top_extra = borders.top + style.padding.top;
    let bottom_extra = borders.bottom + style.padding.bottom;
    *y -= bottom_extra;
    *height += top_extra + bottom_extra;
    let start_extra = match style.direction {
        Direction::Ltr => borders.left + style.padding.left,
        Direction::Rtl => borders.right + style.padding.right,
    };
    let end_extra = match style.direction {
        Direction::Ltr => borders.right + style.padding.right,
        Direction::Rtl => borders.left + style.padding.left,
    };
    if edges.blocks_start {
        match style.direction {
            Direction::Ltr => *x -= start_extra,
            Direction::Rtl => {}
        }
        *width += start_extra;
    } else {
        match style.direction {
            Direction::Ltr => {
                style.border_widths.left = 0.0;
                style.border_styles.left = BorderStyle::None;
                style.padding.left = 0.0;
            }
            Direction::Rtl => {
                style.border_widths.right = 0.0;
                style.border_styles.right = BorderStyle::None;
                style.padding.right = 0.0;
            }
        }
    }
    if edges.blocks_end {
        match style.direction {
            Direction::Ltr => {}
            Direction::Rtl => *x -= end_extra,
        }
        *width += end_extra;
    } else {
        match style.direction {
            Direction::Ltr => {
                style.border_widths.right = 0.0;
                style.border_styles.right = BorderStyle::None;
                style.padding.right = 0.0;
            }
            Direction::Rtl => {
                style.border_widths.left = 0.0;
                style.border_styles.left = BorderStyle::None;
                style.padding.left = 0.0;
            }
        }
    }
}

/// Build the debug/extraction summary for a painted inline-fragment group.
///
/// CSS Text collapses document white space at inline box boundaries before
/// paint groups are prepared, while Parley-shaped glyph runs preserve the
/// actual Unicode clusters emitted to PDF. `RenderedLine::text` is a line
/// summary used by layout tests and diagnostics, so it keeps internal
/// collapsed spaces even when style or bidi boundaries split one line into
/// several PDF text objects:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::layout) fn inline_fragment_text_summary<F: InlineFragmentAccess>(
    fragments: &[F],
    preserve_leading_summary_space: bool,
) -> String {
    let mut summary = String::new();
    for (index, fragment) in fragments.iter().enumerate() {
        if index == 0
            && !preserve_leading_summary_space
            && fragment.style().white_space.collapses_spaces()
        {
            summary.push_str(trim_start_css_collapsible_whitespace(fragment.text()));
        } else {
            summary.push_str(fragment.text());
        }
    }
    summary
}

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
            .used_length_with_percentage_basis(font_size)
            .unwrap_or(value.length_with_percentage_basis(font_size))
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

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextDecorationSegmentInputs {
    pub(in crate::layout) axis: TextDecorationStrokeAxis,
    pub(in crate::layout) line_x: f32,
    pub(in crate::layout) line_y: f32,
    pub(in crate::layout) inline_start: f32,
    pub(in crate::layout) inline_length: f32,
    pub(in crate::layout) block_position: f32,
    pub(in crate::layout) thickness: f32,
    pub(in crate::layout) skip_ink: TextDecorationSkipInk,
    pub(in crate::layout) skip_spaces: TextDecorationSkipSpaces,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextDecorationSegment {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) length: f32,
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

#[derive(Debug, Clone)]
pub(in crate::layout) struct TextDecorationPositionedGlyph {
    pub(in crate::layout) unicode: String,
    pub(in crate::layout) inline_start: f32,
    pub(in crate::layout) inline_end: f32,
    pub(in crate::layout) extra_spacing: f32,
}

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
        TextDecorationStrokeAxis::Horizontal => (
            line_x + run.x_offset + run.text_matrix.a * local_start,
            line_x + run.x_offset + run.text_matrix.a * local_end,
        ),
        TextDecorationStrokeAxis::Vertical if run.text_matrix.is_identity() => {
            let baseline = line_y + run.y_offset;
            (
                baseline - run.font_size * 0.5,
                baseline + run.font_size * 0.5,
            )
        }
        TextDecorationStrokeAxis::Vertical => (
            line_y + run.y_offset + run.text_matrix.b * local_start,
            line_y + run.y_offset + run.text_matrix.b * local_end,
        ),
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
            .used_length_with_percentage_basis(font_size)
            .unwrap_or(value.length_with_percentage_basis(font_size)),
    }
}
