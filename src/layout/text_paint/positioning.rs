use std::ops::Range;
use std::rc::Rc;

use super::*;
use crate::text::{TextTypesettingPlan, VerticalUnitTypesetting};

/// The physical edge from which a vertical logical inline coordinate is
/// measured.
///
/// CSS Writing Modes resolves logical inline start from `writing-mode` and
/// `direction`; page paint coordinates then need the corresponding physical
/// edge, not merely a numerically smaller `y` coordinate.  In particular,
/// `sideways-lr` can start at the physical bottom edge.
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct VerticalInlineAxis {
    logical_start_side: PhysicalSide,
}

impl VerticalInlineAxis {
    /// Construct the vertical-inline projection selected by CSS logical axes.
    pub(in crate::layout) fn from_axes(axes: WritingModeAxes) -> Option<Self> {
        match axes.physical_side(LogicalSide::InlineStart) {
            PhysicalSide::Top | PhysicalSide::Bottom => Some(Self {
                logical_start_side: axes.physical_side(LogicalSide::InlineStart),
            }),
            PhysicalSide::Left | PhysicalSide::Right => None,
        }
    }

    /// Resolve the used axis for text placement.
    ///
    /// Upright vertical typographic units retain the vertical writing mode's
    /// fixed inline base direction; sideways and mixed-orientation units use
    /// their computed `direction`.
    pub(in crate::layout) fn for_style(style: &ComputedStyle) -> Option<Self> {
        let placement_direction = if matches!(
            style.text_layout_policy(),
            css::TextLayoutPolicy::Vertical(TextOrientation::Upright)
        ) {
            Direction::Ltr
        } else {
            style.direction
        };
        Self::from_axes(WritingModeAxes::new(
            style.writing_mode,
            placement_direction,
        ))
    }

    /// Return the page-paint displacement for one positive logical inline
    /// length.  This is intentionally the sole scalar adapter for PDF text
    /// positioning; all caller-side span math stays logical.
    pub(in crate::layout) fn advance_sign(self) -> f32 {
        match self.logical_start_side {
            PhysicalSide::Top => -1.0,
            PhysicalSide::Bottom => 1.0,
            PhysicalSide::Left | PhysicalSide::Right => unreachable!(
                "VerticalInlineAxis must be constructed from a vertical physical inline side"
            ),
        }
    }

    /// Project a logical local span from its known physical logical-start
    /// coordinate into an ordered physical paint span.
    pub(in crate::layout) fn project_span_from_start(
        self,
        logical_start: LayoutLength,
        span: TextInlineSpan,
    ) -> TextInlineSpan {
        let start = logical_start.get();
        match self.logical_start_side {
            PhysicalSide::Top => TextInlineSpan::new(start - span.end, start - span.start),
            PhysicalSide::Bottom => TextInlineSpan::new(start + span.start, start + span.end),
            PhysicalSide::Left | PhysicalSide::Right => unreachable!(
                "VerticalInlineAxis must be constructed from a vertical physical inline side"
            ),
        }
    }

    pub(in crate::layout) fn logical_start_for_paint_rect(
        self,
        rect: PaintRect,
    ) -> VerticalInlineStart {
        VerticalInlineStart::new(match self.logical_start_side {
            PhysicalSide::Top => rect.origin.y + rect.height(),
            PhysicalSide::Bottom => rect.origin.y,
            PhysicalSide::Left | PhysicalSide::Right => unreachable!(
                "VerticalInlineAxis must be constructed from a vertical physical inline side"
            ),
        })
    }
}

/// A page-paint `y` coordinate proven to be a vertical logical inline-start
/// edge, rather than the minimum edge of a physical rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct VerticalInlineStart(f32);

impl VerticalInlineStart {
    pub(in crate::layout) const fn new(y: f32) -> Self {
        Self(y)
    }

    pub(in crate::layout) const fn y(self) -> f32 {
        self.0
    }

    pub(in crate::layout) fn as_layout_length(self) -> LayoutLength {
        layout_pt(self.0)
    }
}

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
    let Some(inline_axis) = VerticalInlineAxis::for_style(style) else {
        return runs;
    };
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
            vertical_positioned_text_runs(run, typesetting_plan, inline_axis, sideways_matrix)
        })
        .collect()
}

pub(in crate::layout) fn vertical_positioned_text_runs(
    mut run: RenderedTextRun,
    typesetting_plan: &TextTypesettingPlan,
    inline_axis: VerticalInlineAxis,
    sideways_matrix: RenderedTextMatrix,
) -> Vec<RenderedTextRun> {
    let Some(glyphs) = run.glyphs.take() else {
        let typesetting = typesetting_plan
            .unanimous_typesetting_for_range(&(0..run.text.len()))
            .unwrap_or(VerticalUnitTypesetting::SidewaysHorizontal);
        let text_matrix = if matches!(typesetting, VerticalUnitTypesetting::UprightVertical) {
            RenderedTextMatrix::IDENTITY
        } else {
            sideways_matrix
        };
        return vec![RenderedTextRun {
            y_offset: inline_axis.advance_sign() * run.x_offset,
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
    for cluster in vertical_paint_clusters(
        glyphs.iter().cloned(),
        glyph_source_ranges,
        typesetting_plan,
    ) {
        cursor += cluster
            .glyphs
            .iter()
            .map(|glyph| glyph.x_advance)
            .sum::<f32>();
        flush_vertical_cluster(
            &run,
            &mut output,
            &mut pending_sideways,
            cursor,
            cluster.typesetting,
            inline_axis,
            sideways_matrix,
            cluster.text,
            cluster.glyphs,
            cluster.glyph_source_ranges,
        );
    }
    if let Some(pending) = pending_sideways {
        output.push(pending.into_rendered_run(&run, sideways_matrix));
    }
    output
}

struct VerticalPaintCluster {
    source_unit_range: Option<Range<usize>>,
    typesetting: VerticalUnitTypesetting,
    text: String,
    glyphs: Vec<RenderedGlyph>,
    glyph_source_ranges: Vec<Option<Range<usize>>>,
}

impl VerticalPaintCluster {
    fn new(source_unit_range: Option<Range<usize>>, typesetting: VerticalUnitTypesetting) -> Self {
        Self {
            source_unit_range,
            typesetting,
            text: String::new(),
            glyphs: Vec::new(),
            glyph_source_ranges: Vec::new(),
        }
    }

    fn push(&mut self, glyph: RenderedGlyph, source_range: Option<Range<usize>>) {
        self.text.push_str(&glyph.unicode);
        self.glyphs.push(glyph);
        self.glyph_source_ranges.push(source_range);
    }

    fn append(&mut self, mut other: Self) {
        self.text.push_str(&other.text);
        self.glyphs.append(&mut other.glyphs);
        self.glyph_source_ranges
            .append(&mut other.glyph_source_ranges);
    }
}

/// Groups paint glyphs by the CSS typographic unit from which they were shaped.
///
/// `RenderedGlyph::unicode` is a PDF/ToUnicode-facing summary and is allowed to
/// be empty. Only the retained source range establishes a vertical-unit
/// boundary. A glyph without a resolvable range is auxiliary to its adjacent
/// provenance-backed unit; leading auxiliaries wait for the first such unit.
fn vertical_paint_clusters(
    glyphs: impl IntoIterator<Item = RenderedGlyph>,
    glyph_source_ranges: Option<Rc<[Option<Range<usize>>]>>,
    typesetting_plan: &TextTypesettingPlan,
) -> Vec<VerticalPaintCluster> {
    let glyphs = glyphs.into_iter().collect::<Vec<_>>();
    let Some(source_ranges) = glyph_source_ranges else {
        return whole_run_vertical_paint_cluster(glyphs, typesetting_plan);
    };
    if !source_ranges.iter().any(Option::is_some) {
        return whole_run_vertical_paint_cluster(glyphs, typesetting_plan);
    }

    let mut clusters = Vec::new();
    let mut current: Option<VerticalPaintCluster> = None;
    let mut leading_auxiliaries =
        VerticalPaintCluster::new(None, VerticalUnitTypesetting::SidewaysHorizontal);
    for (glyph_index, glyph) in glyphs.into_iter().enumerate() {
        let source_range = source_ranges.get(glyph_index).cloned().flatten();
        let source_unit = source_range
            .as_ref()
            .and_then(|range| typesetting_plan.resolved_unit_for_range(range));
        let typesetting = source_range
            .as_ref()
            .and_then(|range| typesetting_plan.unanimous_typesetting_for_range(range));
        let Some(typesetting) = typesetting else {
            if let Some(cluster) = current.as_mut() {
                cluster.push(glyph, source_range);
            } else {
                leading_auxiliaries.push(glyph, source_range);
            }
            continue;
        };

        let source_unit_range = source_unit.map(|unit| unit.range.clone());
        // A multi-unit range is provenance-backed but intentionally has no
        // single-unit identity. Keep it distinct from either neighbor rather
        // than attributing it to a source unit it does not fully occupy.
        let starts_new_unit = source_unit_range.as_ref().is_none_or(|range| {
            current
                .as_ref()
                .map(|cluster: &VerticalPaintCluster| {
                    cluster.source_unit_range.as_ref() != Some(range)
                })
                .unwrap_or(true)
        });
        if starts_new_unit {
            if let Some(cluster) = current.take() {
                clusters.push(cluster);
            }
            let mut cluster = VerticalPaintCluster::new(source_unit_range, typesetting);
            cluster.append(std::mem::replace(
                &mut leading_auxiliaries,
                VerticalPaintCluster::new(None, VerticalUnitTypesetting::SidewaysHorizontal),
            ));
            current = Some(cluster);
        }
        current
            .as_mut()
            .expect("a provenance-backed glyph creates its paint cluster")
            .push(glyph, source_range);
    }
    if let Some(mut cluster) = current {
        cluster.append(leading_auxiliaries);
        clusters.push(cluster);
    } else if !leading_auxiliaries.glyphs.is_empty() {
        clusters.push(leading_auxiliaries);
    }
    clusters
}

fn whole_run_vertical_paint_cluster(
    glyphs: Vec<RenderedGlyph>,
    typesetting_plan: &TextTypesettingPlan,
) -> Vec<VerticalPaintCluster> {
    let typesetting = typesetting_plan
        .uniform_typesetting()
        .unwrap_or(VerticalUnitTypesetting::SidewaysHorizontal);
    let mut cluster = VerticalPaintCluster::new(None, typesetting);
    for glyph in glyphs {
        cluster.push(glyph, None);
    }
    vec![cluster]
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
    inline_axis: VerticalInlineAxis,
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
                    let y_offset = inline_axis.advance_sign() * glyph_cursor;
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
                y_offset: inline_axis.advance_sign() * cluster_start,
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
        let inline_axis = VerticalInlineAxis::for_style(&style).unwrap();
        let runs = vertical_positioned_text_runs(
            source,
            &plan,
            inline_axis,
            RenderedTextMatrix::ROTATE_CW,
        );

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
        let inline_axis = VerticalInlineAxis::for_style(&style).unwrap();
        let runs = vertical_positioned_text_runs(
            source,
            &plan,
            inline_axis,
            RenderedTextMatrix::ROTATE_CW,
        );

        assert_eq!(runs.len(), 3);
        assert_eq!(
            runs.iter().map(|run| run.y_offset).collect::<Vec<_>>(),
            vec![0.0, -3.75, -7.5]
        );
    }

    #[test]
    fn upright_provenance_keeps_empty_unicode_glyphs_in_their_source_units() {
        let source = RenderedTextRun {
            text: Rc::from("AB"),
            actual_text: Some(Rc::from("AB")),
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
                ]
                .into(),
            ),
            glyph_source_ranges: Some(vec![Some(0..1), Some(1..2)].into()),
        };

        let mut style = ComputedStyle::initial();
        style.writing_mode = css::WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Upright;
        let plan = TextTypesettingPlan::resolve("AB", &style);
        let runs = vertical_positioned_text_runs(
            source,
            &plan,
            VerticalInlineAxis::for_style(&style).unwrap(),
            RenderedTextMatrix::ROTATE_CW,
        );

        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs.iter().map(|run| run.y_offset).collect::<Vec<_>>(),
            [0.0, -3.75]
        );
        assert!(runs.iter().all(|run| run.text_matrix.is_identity()));
        assert_eq!(
            runs.iter()
                .map(|run| run.glyph_source_ranges.as_ref().unwrap()[0].clone())
                .collect::<Vec<_>>(),
            [Some(0..1), Some(1..2)]
        );
    }

    #[test]
    fn unanimous_multi_unit_provenance_keeps_upright_paint_placement() {
        let source = RenderedTextRun {
            text: Rc::from("AB"),
            actual_text: Some(Rc::from("AB")),
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
                ]
                .into(),
            ),
            // One Parley shaping cluster can own both CSS units.
            glyph_source_ranges: Some(vec![Some(0..2), Some(0..2)].into()),
        };

        let mut style = ComputedStyle::initial();
        style.writing_mode = css::WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Upright;
        let plan = TextTypesettingPlan::resolve("AB", &style);
        let runs = vertical_positioned_text_runs(
            source,
            &plan,
            VerticalInlineAxis::for_style(&style).unwrap(),
            RenderedTextMatrix::ROTATE_CW,
        );

        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|run| run.text_matrix.is_identity()));
        assert_eq!(
            runs.iter().map(|run| run.y_offset).collect::<Vec<_>>(),
            [0.0, -3.75]
        );
    }

    #[test]
    fn mixed_multi_unit_provenance_remains_conservatively_sideways() {
        let source = RenderedTextRun {
            text: Rc::from("a、"),
            actual_text: Some(Rc::from("a、")),
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: None,
            font_palette: crate::css::FontPalette::Normal,
            glyphs: Some(vec![upright_glyph("", 10.0, 0.0)].into()),
            glyph_source_ranges: Some(vec![Some(0..4)].into()),
        };

        let mut style = ComputedStyle::initial();
        style.writing_mode = css::WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Mixed;
        let plan = TextTypesettingPlan::resolve("a、", &style);
        let runs = vertical_positioned_text_runs(
            source,
            &plan,
            VerticalInlineAxis::for_style(&style).unwrap(),
            RenderedTextMatrix::ROTATE_CW,
        );

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text_matrix, RenderedTextMatrix::ROTATE_CW);
    }

    #[test]
    fn mixed_provenance_splits_empty_unicode_glyphs_for_vertical_lr_and_rl() {
        for writing_mode in [css::WritingMode::VerticalLr, css::WritingMode::VerticalRl] {
            let source = RenderedTextRun {
                text: Rc::from("a\u{3000}"),
                actual_text: Some(Rc::from("a\u{3000}")),
                x_offset: 0.0,
                y_offset: 0.0,
                text_matrix: RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: None,
                font_palette: crate::css::FontPalette::Normal,
                glyphs: Some(
                    vec![
                        upright_glyph("a", 10.0, 0.0),
                        upright_glyph("", 2.0, 0.0),
                        upright_glyph("\u{3000}", 10.0, -10.0),
                    ]
                    .into(),
                ),
                glyph_source_ranges: Some(vec![Some(0..1), None, Some(1..4)].into()),
            };

            let mut style = ComputedStyle::initial();
            style.writing_mode = writing_mode;
            style.text_orientation = TextOrientation::Mixed;
            let plan = TextTypesettingPlan::resolve("a\u{3000}", &style);
            let runs = vertical_positioned_text_runs(
                source,
                &plan,
                VerticalInlineAxis::for_style(&style).unwrap(),
                RenderedTextMatrix::ROTATE_CW,
            );

            assert_eq!(runs.len(), 2, "{writing_mode:?}");
            assert_eq!(runs[0].text_matrix, RenderedTextMatrix::ROTATE_CW);
            assert!(runs[1].text_matrix.is_identity());
            assert_eq!(runs[0].y_offset, 0.0);
            assert_eq!(runs[1].y_offset, -12.0);
            assert_eq!(
                runs[0].glyph_source_ranges.as_ref().unwrap().as_ref(),
                &[Some(0..1), None]
            );
            assert_eq!(
                runs[1].glyph_source_ranges.as_ref().unwrap().as_ref(),
                &[Some(1..4)]
            );
        }
    }

    #[test]
    fn sideways_axes_project_logical_spans_from_the_correct_physical_edge() {
        let cases = [
            (
                WritingMode::SidewaysRl,
                Direction::Ltr,
                PhysicalSide::Top,
                -1.0,
            ),
            (
                WritingMode::SidewaysRl,
                Direction::Rtl,
                PhysicalSide::Bottom,
                1.0,
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Ltr,
                PhysicalSide::Bottom,
                1.0,
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Rtl,
                PhysicalSide::Top,
                -1.0,
            ),
        ];

        for (writing_mode, direction, start_side, advance_sign) in cases {
            let axis = VerticalInlineAxis::from_axes(WritingModeAxes::new(writing_mode, direction))
                .unwrap();
            assert_eq!(axis.logical_start_side, start_side);
            assert_eq!(axis.advance_sign(), advance_sign);
            let projected = axis.project_span_from_start(
                layout_pt(100.0),
                TextInlineSpan::from_start_and_length(10.0, 20.0),
            );
            let expected = if advance_sign < 0.0 {
                TextInlineSpan::new(70.0, 90.0)
            } else {
                TextInlineSpan::new(110.0, 130.0)
            };
            assert_eq!(projected, expected);
        }
    }
}
