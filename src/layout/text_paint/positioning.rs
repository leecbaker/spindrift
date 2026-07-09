use super::*;
use std::rc::Rc;

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
    if !style.writing_mode.has_vertical_lines() {
        return runs;
    }
    let text_layout_policy = style.text_layout_policy();
    let placement_direction = if matches!(
        text_layout_policy,
        css::TextLayoutPolicy::Vertical(TextOrientation::Upright)
    ) {
        Direction::Ltr
    } else {
        style.direction
    };
    let advance_sign = vertical_text_advance_sign(style);
    let sideways_matrix = match text_layout_policy {
        css::TextLayoutPolicy::Sideways(css::SidewaysOrientation::Right) => {
            RenderedTextMatrix::ROTATE_CW
        }
        css::TextLayoutPolicy::Sideways(css::SidewaysOrientation::Left) => {
            RenderedTextMatrix::ROTATE_CCW
        }
        _ => match placement_direction {
            Direction::Ltr => RenderedTextMatrix::ROTATE_CW,
            Direction::Rtl => RenderedTextMatrix::ROTATE_CCW,
        },
    };
    let text_orientation = match text_layout_policy {
        css::TextLayoutPolicy::Vertical(text_orientation) => text_orientation,
        css::TextLayoutPolicy::Horizontal | css::TextLayoutPolicy::Sideways(_) => {
            TextOrientation::Sideways
        }
    };
    runs.into_iter()
        .flat_map(|run| {
            vertical_positioned_text_runs(run, text_orientation, advance_sign, sideways_matrix)
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
    let mut pending_sideways: Option<PendingVerticalSidewaysRun> = None;
    let mut cursor = run.x_offset;
    let mut cluster_text = String::new();
    let mut cluster_glyphs = Vec::new();
    let mut consumed_text_bytes = 0usize;
    let unit_ends = typographic_unit_ranges(&run.text)
        .into_iter()
        .map(|range| range.end)
        .collect::<Vec<_>>();
    let mut unit_index = 0usize;
    let mut cluster_end = unit_ends.first().cloned().unwrap_or(run.text.len());
    for glyph in glyphs.iter().cloned() {
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
    if let Some(pending) = pending_sideways {
        output.push(pending.into_rendered_run(&run, sideways_matrix));
    }
    output
}

struct PendingVerticalSidewaysRun {
    text: String,
    actual_text: Option<Rc<str>>,
    y_offset: f32,
    glyphs: Vec<RenderedGlyph>,
}

impl PendingVerticalSidewaysRun {
    fn into_rendered_run(
        self,
        source: &RenderedTextRun,
        sideways_matrix: RenderedTextMatrix,
    ) -> RenderedTextRun {
        RenderedTextRun {
            text: self.text.into(),
            actual_text: self.actual_text,
            x_offset: 0.0,
            y_offset: self.y_offset,
            text_matrix: sideways_matrix,
            font_size: source.font_size,
            font_id: source.font_id,
            glyphs: Some(self.glyphs.into()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn flush_vertical_cluster(
    source: &RenderedTextRun,
    output: &mut Vec<RenderedTextRun>,
    pending_sideways: &mut Option<PendingVerticalSidewaysRun>,
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
            output.push(run.into_rendered_run(source, sideways_matrix));
        }
        // PDF text arrays can change horizontal advance but cannot express a
        // per-glyph vertical origin. Keep upright glyphs as independently
        // positioned runs so the vertical-origin correction recorded during
        // shaping reaches the PDF text matrix. This also preserves the
        // cross-axis offsets of marks in a vertical typographic unit.
        output.extend(glyphs.into_iter().map(|glyph| {
            RenderedTextRun {
                text: glyph.unicode.clone().into(),
                actual_text: glyph
                    .unicode
                    .is_empty()
                    .then(|| source.actual_text.clone())
                    .flatten(),
                x_offset: glyph.x_offset,
                y_offset: advance_sign * cluster_start + glyph.y_offset,
                text_matrix: RenderedTextMatrix::IDENTITY,
                font_size: source.font_size,
                font_id: source.font_id,
                glyphs: Some(vec![glyph].into()),
            }
        }));
        return;
    }
    match pending_sideways {
        Some(run) => {
            run.text.push_str(&text);
            run.glyphs.extend(glyphs);
        }
        None => {
            *pending_sideways = Some(PendingVerticalSidewaysRun {
                text,
                actual_text: source.actual_text.clone(),
                y_offset: advance_sign * cluster_start,
                glyphs,
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
