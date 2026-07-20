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
        baseline,
        inline_span,
        inset_start,
        inset_end,
        style,
        decoration,
        phase,
        color,
        color_override,
        metrics,
    } = input;
    if inline_span.length() <= 0.0 {
        return Vec::new();
    }

    let axis = if style.writing_mode == WritingMode::HorizontalTb {
        TextDecorationStrokeAxis::Horizontal
    } else {
        TextDecorationStrokeAxis::Vertical
    };
    let Some(inline_span) =
        text_decoration_inline_span(axis, baseline, inline_span, inset_start, inset_end, style)
    else {
        return Vec::new();
    };

    let underline_thickness = used_text_decoration_thickness(
        decoration.thickness.clone(),
        style.font_size,
        &metrics,
        false,
    );
    let strikeout_thickness = used_text_decoration_thickness(
        decoration.thickness.clone(),
        style.font_size,
        &metrics,
        true,
    );
    let mut strokes = Vec::new();

    if phase.paints_before_text()
        && decoration.underline
        && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::Underline)
    {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            baseline,
            inline_span,
            block_position: text_decoration_block_position(
                axis,
                baseline.x,
                baseline.y,
                style,
                decoration.underline_position,
                decoration.underline_offset.clone(),
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
            baseline,
            inline_span,
            block_position: text_decoration_block_position(
                axis,
                baseline.x,
                baseline.y,
                style,
                decoration.underline_position,
                decoration.underline_offset.clone(),
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
            baseline,
            inline_span,
            block_position: text_decoration_block_position(
                axis,
                baseline.x,
                baseline.y,
                style,
                decoration.underline_position,
                decoration.underline_offset.clone(),
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
            baseline,
            inline_span,
            block_position: text_decoration_block_position(
                axis,
                baseline.x,
                baseline.y,
                style,
                decoration.underline_position,
                decoration.underline_offset.clone(),
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Underline,
            ),
            thickness: underline_thickness,
            color: color_override.unwrap_or(CssColor::new(255, 0, 0)),
            style: TextDecorationStyle::Wavy,
            skip_ink: TextDecorationSkipInk::None,
            skip_spaces: TextDecorationSkipSpaces::NONE,
        });
    }

    if phase.paints_before_text() && decoration.grammar_error {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            baseline,
            inline_span,
            block_position: text_decoration_block_position(
                axis,
                baseline.x,
                baseline.y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Underline,
            ),
            thickness: underline_thickness,
            color: color_override.unwrap_or(CssColor::new(0, 128, 0)),
            style: TextDecorationStyle::Wavy,
            skip_ink: TextDecorationSkipInk::None,
            skip_spaces: TextDecorationSkipSpaces::NONE,
        });
    }

    strokes
}

pub(in crate::layout) fn text_decoration_inline_span(
    axis: TextDecorationStrokeAxis,
    baseline: PaintPoint,
    span: TextInlineSpan,
    inset_start: f32,
    inset_end: f32,
    style: &ComputedStyle,
) -> Option<TextInlineSpan> {
    let length = (span.length() - inset_start - inset_end).max(0.0);
    if length <= 0.0 {
        return None;
    }
    match axis {
        TextDecorationStrokeAxis::Horizontal => {
            let start = match style.direction {
                Direction::Ltr => baseline.x + inset_start,
                Direction::Rtl => baseline.x + inset_end,
            };
            Some(TextInlineSpan::from_start_and_length(start, length))
        }
        TextDecorationStrokeAxis::Vertical => {
            if vertical_text_advance_sign(style) < 0.0 {
                Some(TextInlineSpan::from_start_and_length(
                    baseline.y - span.length() + inset_end,
                    length,
                ))
            } else {
                Some(TextInlineSpan::from_start_and_length(
                    baseline.y + inset_start,
                    length,
                ))
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
    let placement_direction = if matches!(
        style.text_layout_policy(),
        css::TextLayoutPolicy::Vertical(TextOrientation::Upright)
    ) {
        Direction::Ltr
    } else {
        style.direction
    };
    match inline_start_side(style.writing_mode, placement_direction) {
        PhysicalSide::Top => -1.0,
        PhysicalSide::Bottom => 1.0,
        PhysicalSide::Left | PhysicalSide::Right => 0.0,
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
    match css::line_under_side(writing_mode) {
        PhysicalSide::Left => TextDecorationSide::Left,
        PhysicalSide::Right => TextDecorationSide::Right,
        PhysicalSide::Top | PhysicalSide::Bottom => TextDecorationSide::Right,
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
