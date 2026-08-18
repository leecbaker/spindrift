use super::*;
use crate::text::{TextTypesettingPlan, VerticalUnitTypesetting};
use std::ops::Range;
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
    position_rendered_runs_for_writing_mode_with_plan(
        shaped.rendered_runs(),
        style,
        &shaped.typesetting_plan,
    )
}

fn position_rendered_runs_for_writing_mode_with_plan(
    runs: Vec<RenderedTextRun>,
    style: &ComputedStyle,
    typesetting_plan: &TextTypesettingPlan,
) -> Vec<RenderedTextRun> {
    if !style.writing_mode.has_vertical_lines() {
        return runs;
    }
    let text_layout_policy = style.text_layout_policy();
    let advance_sign = vertical_text_advance_sign(style);
    let sideways_matrix = match text_layout_policy {
        css::TextLayoutPolicy::Sideways(css::SidewaysOrientation::Right) => {
            RenderedTextMatrix::ROTATE_CW
        }
        css::TextLayoutPolicy::Sideways(css::SidewaysOrientation::Left) => {
            RenderedTextMatrix::ROTATE_CCW
        }
        _ => match style.direction {
            Direction::Ltr => RenderedTextMatrix::ROTATE_CW,
            Direction::Rtl => RenderedTextMatrix::ROTATE_CCW,
        },
    };
    runs.into_iter()
        .flat_map(|run| {
            vertical_positioned_text_runs(run, typesetting_plan, advance_sign, sideways_matrix)
        })
        .collect()
}

pub(in crate::layout) fn vertical_positioned_text_runs(
    mut run: RenderedTextRun,
    typesetting_plan: &TextTypesettingPlan,
    advance_sign: f32,
    sideways_matrix: RenderedTextMatrix,
) -> Vec<RenderedTextRun> {
    let Some(glyphs) = run.glyphs.take() else {
        let typesetting = typesetting_plan
            .typesetting_for_range(&(0..run.text.len()))
            .unwrap_or(VerticalUnitTypesetting::SidewaysHorizontal);
        let text_matrix = if matches!(typesetting, VerticalUnitTypesetting::UprightVertical) {
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
    let glyph_source_ranges = run.glyph_source_ranges.take();
    if glyphs.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::new();
    let mut pending_sideways: Option<PendingVerticalSidewaysRun> = None;
    let mut cursor = run.x_offset;
    let mut cluster_text = String::new();
    let mut cluster_glyphs = Vec::new();
    let mut cluster_source_ranges = Vec::new();
    let mut consumed_text_bytes = 0usize;
    let unit_ends = CursiveProtectedUnitRanges::new(&run.text)
        .into_iter()
        .map(|range| range.end)
        .collect::<Vec<_>>();
    let mut unit_index = 0usize;
    let mut cluster_start = 0usize;
    let mut cluster_typesetting = None;
    let mut cluster_end = unit_ends.first().cloned().unwrap_or(run.text.len());
    for (glyph_index, glyph) in glyphs.iter().cloned().enumerate() {
        if !glyph.unicode.is_empty()
            && !cluster_glyphs.is_empty()
            && consumed_text_bytes >= cluster_end
        {
            flush_vertical_cluster(
                &run,
                &mut output,
                &mut pending_sideways,
                cursor,
                cluster_typesetting.unwrap_or_else(|| {
                    typesetting_plan
                        .typesetting_for_range(&(cluster_start..consumed_text_bytes))
                        .unwrap_or(VerticalUnitTypesetting::SidewaysHorizontal)
                }),
                advance_sign,
                sideways_matrix,
                std::mem::take(&mut cluster_text),
                std::mem::take(&mut cluster_glyphs),
                std::mem::take(&mut cluster_source_ranges),
            );
            while unit_index + 1 < unit_ends.len() && consumed_text_bytes >= cluster_end {
                unit_index += 1;
                cluster_end = unit_ends[unit_index];
            }
            cluster_start = consumed_text_bytes;
            cluster_typesetting = None;
        }
        cluster_typesetting.get_or_insert_with(|| {
            glyph_source_ranges
                .as_ref()
                .and_then(|ranges| ranges.get(glyph_index))
                .and_then(Option::as_ref)
                .and_then(|range| typesetting_plan.typesetting_for_range(range))
                .unwrap_or_else(|| {
                    typesetting_plan
                        .typesetting_for_range(&(cluster_start..cluster_end))
                        .unwrap_or(VerticalUnitTypesetting::SidewaysHorizontal)
                })
        });
        if !glyph.unicode.is_empty() {
            consumed_text_bytes += glyph.unicode.len();
            cluster_text.push_str(&glyph.unicode);
        }
        cursor += glyph.x_advance;
        cluster_glyphs.push(glyph);
        cluster_source_ranges.push(
            glyph_source_ranges
                .as_ref()
                .and_then(|ranges| ranges.get(glyph_index))
                .cloned()
                .flatten(),
        );
    }
    if !cluster_glyphs.is_empty() {
        flush_vertical_cluster(
            &run,
            &mut output,
            &mut pending_sideways,
            cursor,
            cluster_typesetting.unwrap_or_else(|| {
                typesetting_plan
                    .typesetting_for_range(&(cluster_start..consumed_text_bytes))
                    .unwrap_or(VerticalUnitTypesetting::SidewaysHorizontal)
            }),
            advance_sign,
            sideways_matrix,
            cluster_text,
            cluster_glyphs,
            cluster_source_ranges,
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
    glyph_source_ranges: Vec<Option<Range<usize>>>,
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
            font_palette: source.font_palette.clone(),
            glyphs: Some(self.glyphs.into()),
            glyph_source_ranges: Some(self.glyph_source_ranges.into()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn flush_vertical_cluster(
    source: &RenderedTextRun,
    output: &mut Vec<RenderedTextRun>,
    pending_sideways: &mut Option<PendingVerticalSidewaysRun>,
    cursor_after_cluster: f32,
    typesetting: VerticalUnitTypesetting,
    advance_sign: f32,
    sideways_matrix: RenderedTextMatrix,
    text: String,
    glyphs: Vec<RenderedGlyph>,
    glyph_source_ranges: Vec<Option<Range<usize>>>,
) {
    let advance = glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>();
    let cluster_start = cursor_after_cluster - advance;
    if matches!(typesetting, VerticalUnitTypesetting::UprightVertical) {
        if let Some(run) = pending_sideways.take() {
            output.push(run.into_rendered_run(source, sideways_matrix));
        }
        // PDF text arrays can change horizontal advance but cannot express a
        // per-glyph vertical origin. Keep upright glyphs as independently
        // positioned runs so the vertical-origin correction recorded during
        // shaping reaches the PDF text matrix. This also preserves the
        // cross-axis offsets of marks in a vertical typographic unit.
        // One typographic unit can contain several paintable glyphs, and
        // some of those glyphs deliberately have no standalone Unicode
        // summary (for example a vertical-form alternate that shares the
        // cluster's ToUnicode value). Each glyph still owns one used advance.
        // Keep their individual vertical origins in the same coordinate
        // system as the cluster start instead of stacking them all at the
        // first glyph's origin.
        let mut glyph_cursor = cluster_start;
        output.extend(
            glyphs
                .into_iter()
                .zip(glyph_source_ranges)
                .map(|(glyph, source_range)| {
                    let y_offset = advance_sign * glyph_cursor;
                    glyph_cursor += glyph.x_advance;
                    RenderedTextRun {
                        text: glyph.unicode.clone().into(),
                        actual_text: glyph
                            .unicode
                            .is_empty()
                            .then(|| source.actual_text.clone())
                            .flatten(),
                        x_offset: glyph.x_offset,
                        y_offset,
                        text_matrix: source.text_matrix,
                        font_size: source.font_size,
                        font_id: source.font_id,
                        font_palette: source.font_palette.clone(),
                        glyphs: Some(vec![glyph].into()),
                        glyph_source_ranges: Some(vec![source_range].into()),
                    }
                }),
        );
        return;
    }
    match pending_sideways {
        Some(run) => {
            run.text.push_str(&text);
            run.glyphs.extend(glyphs);
            run.glyph_source_ranges.extend(glyph_source_ranges);
        }
        None => {
            *pending_sideways = Some(PendingVerticalSidewaysRun {
                text,
                actual_text: source.actual_text.clone(),
                y_offset: advance_sign * cluster_start,
                glyphs,
                glyph_source_ranges,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::paint::text::RenderedGlyphKind;

    fn upright_glyph(text: &str, advance: f32, vertical_origin: f32) -> RenderedGlyph {
        RenderedGlyph {
            kind: RenderedGlyphKind::Paint(1),
            x_advance: advance,
            nominal_x_advance: advance,
            x_offset: 0.0,
            y_offset: vertical_origin,
            unicode: text.to_string(),
        }
    }

    #[test]
    fn upright_vertical_runs_apply_vertical_origin_once() {
        let source = RenderedTextRun {
            text: Rc::from("AB"),
            actual_text: None,
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: None,
            font_palette: crate::css::FontPalette::Normal,
            glyphs: Some(
                vec![
                    upright_glyph("A", 12.0, -10.0),
                    upright_glyph("B", 12.0, -10.0),
                ]
                .into(),
            ),
            glyph_source_ranges: None,
        };

        let mut style = ComputedStyle::initial();
        style.writing_mode = css::WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Upright;
        let plan = TextTypesettingPlan::resolve("AB", &style);
        let runs =
            vertical_positioned_text_runs(source, &plan, -1.0, RenderedTextMatrix::ROTATE_CW);

        assert_eq!(runs.len(), 2);
        for (index, run) in runs.iter().enumerate() {
            let glyph = run.glyphs.as_ref().unwrap().first().unwrap();
            assert!(
                (run.y_offset + glyph.y_offset - (-10.0 - index as f32 * 12.0)).abs() < 0.01,
                "upright glyph {index} should receive its OpenType vertical origin exactly once: {run:?}"
            );
            assert!(
                (run.y_offset - -(index as f32 * 12.0)).abs() < 0.01,
                "run origin contains only normal-flow vertical advance: {run:?}"
            );
        }
    }

    #[test]
    fn upright_vertical_runs_advance_glyphs_without_individual_unicode() {
        let source = RenderedTextRun {
            text: Rc::from("\u{3000}\u{3001}\u{3002}"),
            actual_text: Some(Rc::from("\u{3000}\u{3001}")),
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: None,
            font_palette: crate::css::FontPalette::Normal,
            glyphs: Some(
                vec![
                    upright_glyph("", 3.75, -10.0),
                    upright_glyph("", 3.75, -10.0),
                    upright_glyph("\u{3002}", 3.75, -10.0),
                ]
                .into(),
            ),
            glyph_source_ranges: None,
        };

        let mut style = ComputedStyle::initial();
        style.writing_mode = css::WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Upright;
        let plan = TextTypesettingPlan::resolve("\u{3000}\u{3001}\u{3002}", &style);
        let runs =
            vertical_positioned_text_runs(source, &plan, -1.0, RenderedTextMatrix::ROTATE_CW);

        assert_eq!(runs.len(), 3);
        assert_eq!(
            runs.iter().map(|run| run.y_offset).collect::<Vec<_>>(),
            vec![0.0, -3.75, -7.5]
        );
    }
}
