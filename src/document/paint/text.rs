use std::ops::{Deref, Range};
use std::rc::Rc;

use super::geometry::{
    PaintClip, PaintDisplacement, PaintPoint, PaintRect, PaintSize, PaintTransform,
    PaintTranslation,
};
use super::paths::RenderedPath;
use crate::CssColor;
use crate::css::FontPalette;

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLine {
    pub text: String,
    pub(in crate::document) origin: PaintPoint,
    pub font_size: f32,
    pub font_id: Option<usize>,
    pub color: CssColor,
    pub runs: Vec<RenderedTextRun>,
    pub(crate) glyph_ink_bounds: Option<PaintClip>,
    /// Displacement applied when converting the CSS layout baseline to the
    /// embedded font program's glyph origin. This remains private renderer
    /// metadata so later layout code can recover the original CSS baseline
    /// without guessing from a fallback run's font metrics.
    pub(crate) glyph_origin_adjustment: PaintDisplacement,
    pub(crate) source: RenderedLineSource,
    /// Source identity carried only by text prepared from inline layout.
    ///
    /// It allows public line metadata to join explicit tracking units from a
    /// single authored run while keeping separately painted artifacts such as
    /// text-emphasis marks distinct.
    pub(crate) source_run: Option<Rc<()>>,
}

/// One full-em glyph whose normal PDF text ink is replaced by an equivalent
/// opaque vector path.
///
/// The indices are into the source [`RenderedLine`]'s visual run and glyph
/// streams. They make the ownership boundary explicit before layout partitions
/// the line into independent text-paint records.
#[derive(Debug)]
pub(crate) struct OpaqueTextGlyphCoverage {
    pub(crate) run_index: usize,
    pub(crate) glyph_index: usize,
    pub(crate) path: RenderedPath,
}

/// An ordered slice of a logical text line at the PDF paint boundary.
///
/// CSS layout, line breaking, and decorations retain the original
/// [`RenderedLine`]. Only glyph realization is partitioned: ordinary slices
/// remain visible PDF text, while an opaque-coverage slice owns every glyph
/// replaced by its vector paths.
#[derive(Debug)]
pub(crate) enum RenderedTextPaintSegment {
    Text(RenderedLine),
    OpaqueCoverage {
        line: RenderedLine,
        paths: Vec<RenderedPath>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderedLineSource {
    Normal,
    /// The generated `block-ellipsis` anonymous inline defined by CSS Overflow.
    BlockEllipsis,
    RunIn,
    InlineAtom,
    Marker,
}

#[allow(dead_code)]
impl RenderedLine {
    pub fn new(
        text: String,
        x: f32,
        y: f32,
        font_size: f32,
        font_id: Option<usize>,
        color: CssColor,
        runs: Vec<RenderedTextRun>,
    ) -> Self {
        Self::from_paint_origin(text, PaintPoint::new(x, y), font_size, font_id, color, runs)
    }

    /// Returns the line origin's horizontal position in PDF points.
    pub fn x(&self) -> f32 {
        self.origin.x
    }

    /// Returns the line origin's vertical position in PDF points.
    pub fn y(&self) -> f32 {
        self.origin.y
    }

    pub(crate) fn from_paint_origin(
        text: String,
        origin: PaintPoint,
        font_size: f32,
        font_id: Option<usize>,
        color: CssColor,
        runs: Vec<RenderedTextRun>,
    ) -> Self {
        Self::from_paint_origin_with_source(
            text,
            origin,
            font_size,
            font_id,
            color,
            runs,
            RenderedLineSource::Normal,
        )
    }

    pub(crate) fn from_paint_origin_with_source(
        text: String,
        origin: PaintPoint,
        font_size: f32,
        font_id: Option<usize>,
        color: CssColor,
        runs: Vec<RenderedTextRun>,
        source: RenderedLineSource,
    ) -> Self {
        Self {
            text,
            origin,
            font_size,
            font_id,
            color,
            runs,
            glyph_ink_bounds: None,
            glyph_origin_adjustment: PaintDisplacement::zero(),
            source,
            source_run: None,
        }
    }

    /// Attach the conversion used to map this line's CSS baseline into PDF
    /// glyph-program coordinates.
    pub(crate) fn with_glyph_origin_adjustment(
        mut self,
        glyph_origin_adjustment: PaintDisplacement,
    ) -> Self {
        self.glyph_origin_adjustment = glyph_origin_adjustment;
        self
    }

    pub(crate) fn with_glyph_ink_bounds(mut self, bounds: Option<PaintClip>) -> Self {
        self.glyph_ink_bounds = bounds;
        self
    }

    pub(crate) fn with_source_run(mut self, source_run: Rc<()>) -> Self {
        self.source_run = Some(source_run);
        self
    }

    pub(crate) fn glyph_origin_adjustment(&self) -> PaintDisplacement {
        self.glyph_origin_adjustment
    }

    pub(crate) fn origin(&self) -> PaintPoint {
        self.origin
    }

    pub(crate) fn paint_bounds(&self) -> PaintClip {
        let origin = self.origin();
        PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(origin.x, origin.y - self.font_size),
            PaintSize::new(rendered_line_width(self), self.font_size * 1.35),
        ))
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.origin = offset.transform_point(self.origin);
        self.glyph_ink_bounds = self
            .glyph_ink_bounds
            .map(|bounds| bounds.translated(offset));
        self
    }

    pub(crate) fn transformed(mut self, transform: PaintTransform) -> Self {
        self.origin = transform.apply_point(self.origin);
        for run in &mut self.runs {
            run.text_matrix = run.text_matrix.transformed_by(transform);
        }
        self
    }

    pub(crate) fn translate_origin(&mut self, offset: PaintTranslation) {
        self.origin = offset.transform_point(self.origin);
    }

    pub(in crate::document) fn append_same_line_with_gap(&mut self, other: &RenderedLine) {
        let offset = other.origin.x - self.origin.x;
        self.text.push(' ');
        self.text.push_str(&other.text);
        self.runs.extend(other.runs.iter().cloned().map(|mut run| {
            run.x_offset += offset;
            run
        }));
    }

    pub(in crate::document) fn append_same_line_continuation(&mut self, other: &RenderedLine) {
        let offset = other.origin.x - self.origin.x;
        self.text.push_str(&other.text);
        self.runs.extend(other.runs.iter().cloned().map(|mut run| {
            run.x_offset += offset;
            run
        }));
    }
}

/// Partition a rendered line into ordered normal-text and opaque-coverage
/// slices.
///
/// A coverage path may replace only the glyph it was derived from. Keeping
/// this relation at glyph granularity prevents a full-em glyph in one fallback
/// run from making unrelated glyphs in a sibling run invisible in the PDF.
/// CSS 2.2 Appendix E defines the retained paint order; ISO 32000-2:2020,
/// 9.3.6 defines the invisible-text realization used by the coverage slice.
pub(crate) fn split_rendered_line_for_opaque_text_coverage(
    line: RenderedLine,
    coverages: Vec<OpaqueTextGlyphCoverage>,
) -> Vec<RenderedTextPaintSegment> {
    if coverages.is_empty() {
        return vec![RenderedTextPaintSegment::Text(line)];
    }

    let mut coverage_paths = line
        .runs
        .iter()
        .map(|run| run.glyphs.as_ref().map(|glyphs| vec![None; glyphs.len()]))
        .collect::<Vec<_>>();
    for coverage in coverages {
        let Some(glyphs) = coverage_paths
            .get_mut(coverage.run_index)
            .and_then(Option::as_mut)
        else {
            continue;
        };
        let Some(slot) = glyphs.get_mut(coverage.glyph_index) else {
            continue;
        };
        if slot.is_none() {
            *slot = Some(coverage.path);
        }
    }

    let mut segments = Vec::new();
    for (run_index, run) in line.runs.iter().enumerate() {
        let Some(glyph_paths) = coverage_paths.get_mut(run_index).and_then(Option::as_mut) else {
            segments.push(RenderedTextPaintSegment::Text(rendered_line_slice(
                &line,
                vec![run.clone()],
            )));
            continue;
        };
        let Some(glyphs) = run.glyphs.as_ref() else {
            segments.push(RenderedTextPaintSegment::Text(rendered_line_slice(
                &line,
                vec![run.clone()],
            )));
            continue;
        };

        if run.actual_text.is_some() {
            // /ActualText belongs to the complete marked-content sequence:
            // slicing it would change the text exposed to assistive technology
            // and PDF extraction. A whole run may nevertheless use opaque
            // coverage, because its marked-content sequence remains intact.
            // ISO 32000-2:2020, 14.9.4 defines this replacement text.
            let fully_covered = glyphs
                .iter()
                .zip(glyph_paths.iter())
                .all(|(glyph, path)| glyph.is_advance_only() || path.is_some());
            if fully_covered {
                let paths = glyph_paths
                    .iter_mut()
                    .filter_map(Option::take)
                    .collect::<Vec<_>>();
                if !paths.is_empty() {
                    segments.push(RenderedTextPaintSegment::OpaqueCoverage {
                        line: rendered_line_slice(&line, vec![run.clone()]),
                        paths,
                    });
                    continue;
                }
            }

            // Partial coverage cannot retain the one-to-one ownership relation
            // without splitting /ActualText, so retain the original text run.
            segments.push(RenderedTextPaintSegment::Text(rendered_line_slice(
                &line,
                vec![run.clone()],
            )));
            continue;
        }

        let mut start = 0usize;
        while start < glyphs.len() {
            let is_coverage = glyph_paths[start].is_some();
            let mut end = start + 1;
            while end < glyphs.len() && glyph_paths[end].is_some() == is_coverage {
                end += 1;
            }
            let Some(slice) = rendered_text_run_glyph_slice(run, start..end) else {
                // A run with incomplete glyph storage cannot prove a
                // per-glyph ownership boundary. Retain it as ordinary text.
                segments.push(RenderedTextPaintSegment::Text(rendered_line_slice(
                    &line,
                    vec![run.clone()],
                )));
                break;
            };
            let segment_line = rendered_line_slice(&line, vec![slice]);
            if is_coverage {
                let paths = glyph_paths[start..end]
                    .iter_mut()
                    .map(|path| path.take().expect("coverage slice owns every glyph path"))
                    .collect();
                segments.push(RenderedTextPaintSegment::OpaqueCoverage {
                    line: segment_line,
                    paths,
                });
            } else {
                segments.push(RenderedTextPaintSegment::Text(segment_line));
            }
            start = end;
        }
    }
    segments
}

fn rendered_line_slice(source: &RenderedLine, runs: Vec<RenderedTextRun>) -> RenderedLine {
    let text = runs.iter().map(|run| run.text.as_ref()).collect();
    RenderedLine {
        text,
        origin: source.origin,
        font_size: source.font_size,
        font_id: runs.first().and_then(|run| run.font_id),
        color: source.color,
        runs,
        // A paint-only slice must not inherit the source line's full ink
        // bounds: PDF hidden-ink elision requires bounds proved for this
        // exact slice, and the coverage paths provide that proof separately.
        glyph_ink_bounds: None,
        glyph_origin_adjustment: source.glyph_origin_adjustment,
        source: source.source,
        source_run: source.source_run.clone(),
    }
}

fn rendered_text_run_glyph_slice(
    source: &RenderedTextRun,
    glyph_range: Range<usize>,
) -> Option<RenderedTextRun> {
    debug_assert!(source.actual_text.is_none());
    let glyphs = source.glyphs.as_ref()?;
    let glyph_slice = glyphs.get(glyph_range.clone())?;
    let preceding_advance: f32 = glyphs[..glyph_range.start]
        .iter()
        .map(|glyph| glyph.x_advance)
        .sum();
    let text = glyph_slice
        .iter()
        .map(|glyph| glyph.unicode.as_str())
        .collect::<String>();
    let glyph_source_ranges = source.glyph_source_ranges.as_ref().map(|ranges| {
        Rc::from(
            ranges
                .get(glyph_range.clone())
                .expect("glyph provenance stays aligned with glyph storage")
                .to_vec()
                .into_boxed_slice(),
        )
    });
    Some(RenderedTextRun {
        text: Rc::from(text),
        actual_text: None,
        x_offset: source.x_offset + preceding_advance,
        y_offset: source.y_offset,
        text_matrix: source.text_matrix,
        font_size: source.font_size,
        font_id: source.font_id,
        font_palette: source.font_palette.clone(),
        glyphs: Some(glyph_slice.to_vec().into()),
        glyph_source_ranges,
    })
}

/// Return whether adjacent rendered text groups are fragments of one inline line.
///
/// CSS Inline allows bidi reordering and whitespace processing to split a
/// visual line into multiple shaped text groups. The public rendered-line
/// metadata should still expose contiguous same-style groups as one line, while
/// preserving distinct styled fragments and deliberate word-gap merges:
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::document) fn rendered_lines_can_merge_as_inline_continuation(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    let y_tolerance = left.font_size.min(right.font_size) * 0.25;
    let has_preserved_tab_text = left
        .text
        .chars()
        .chain(right.text.chars())
        .any(|character| character == '\t');
    let left_text_font_id = rendered_line_first_significant_font_id(left);
    let right_text_font_id = rendered_line_first_significant_font_id(right);
    if (left.origin.y - right.origin.y).abs() > y_tolerance
        || right.origin.x < left.origin.x
        || left.source != right.source
        || left.glyph_origin_adjustment != right.glyph_origin_adjustment
        || (left.font_size - right.font_size).abs() >= 0.01
        || (left_text_font_id != right_text_font_id
            && !has_preserved_tab_text
            && !rendered_lines_can_merge_across_font_fallback_boundary(left, right))
        || left.color != right.color
    {
        return false;
    }
    let has_continuation_space = left.text.chars().last().is_some_and(char::is_whitespace)
        || right.text.chars().next().is_some_and(char::is_whitespace);
    if !has_continuation_space && !has_preserved_tab_text {
        return false;
    }
    let gap = rendered_line_visual_start(right) - rendered_line_visual_end(left);
    gap.abs() <= 0.1
}

/// Return whether two consecutive paint records meet at exactly the same
/// visual text position.
///
/// This is deliberately independent of source provenance and whitespace. CSS
/// layout has already kept the records separate for its own semantic reasons;
/// joining them here only lets the PDF backend preserve its text cursor across
/// that paint boundary instead of serializing a newly rounded absolute origin.
/// The backend rechecks the individual text runs before reusing its cursor.
pub(in crate::document) fn rendered_lines_can_merge_as_exact_paint_continuation(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    if left.origin.y != right.origin.y
        || right.origin.x < left.origin.x
        || left.glyph_origin_adjustment != right.glyph_origin_adjustment
        || left.color != right.color
    {
        return false;
    }
    // Combining page records changes their identity in the retained display
    // list. It is therefore safe only within one source run, except for the
    // generated CSS Overflow marker that must remain a separate anonymous
    // inline but is defined to follow the remaining line content.
    let shares_source_run = left
        .source_run
        .as_ref()
        .zip(right.source_run.as_ref())
        .is_some_and(|(left, right)| Rc::ptr_eq(left, right));
    if !shares_source_run && right.source != RenderedLineSource::BlockEllipsis {
        return false;
    }
    let left_end = rendered_line_visual_end(left);
    let right_start = rendered_line_visual_start(right);
    let tolerance = f32::EPSILON * (left_end.abs() + right_start.abs()).max(1.0) * 8.0;
    (right_start - left_end).abs() <= tolerance
}

fn rendered_line_first_significant_font_id(line: &RenderedLine) -> Option<usize> {
    line.runs
        .iter()
        .find(|run| run.text.chars().any(|character| !character.is_whitespace()))
        .and_then(|run| run.font_id)
        .or(line.font_id)
}

fn rendered_lines_can_merge_across_font_fallback_boundary(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    if !left.text.chars().last().is_some_and(char::is_whitespace)
        && !right.text.chars().next().is_some_and(char::is_whitespace)
    {
        return false;
    }
    rendered_line_trimmed_single_non_ascii_char(left).is_some()
        || rendered_line_trimmed_single_non_ascii_char(right).is_some()
}

fn rendered_line_trimmed_single_non_ascii_char(line: &RenderedLine) -> Option<char> {
    let mut chars = line.text.trim().chars();
    let character = chars.next()?;
    if chars.next().is_none() && !character.is_ascii() && character != '\u{2708}' {
        Some(character)
    } else {
        None
    }
}

pub(in crate::document) fn split_rendered_line_at_font_run_boundaries(
    line: RenderedLine,
) -> Vec<RenderedLine> {
    let [symbol_run, text_run] = line.runs.as_slice() else {
        return vec![line];
    };
    if !symbol_run.text_matrix.is_identity()
        || !text_run.text_matrix.is_identity()
        || !text_run
            .text
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        || text_run.text.trim_start().is_empty()
        || symbol_run.font_id == text_run.font_id
    {
        return vec![line];
    }
    let mut symbol_chars = symbol_run.text.chars();
    let Some(symbol) = symbol_chars.next() else {
        return vec![line];
    };
    if symbol_chars.next().is_some() || symbol.is_whitespace() || symbol.is_alphanumeric() {
        return vec![line];
    }

    vec![
        rendered_line_segment(
            &line,
            vec![symbol_run.clone()],
            symbol_run.font_id,
            symbol_run.x_offset,
            symbol_run.y_offset,
            false,
        ),
        rendered_line_segment(
            &line,
            vec![text_run.clone()],
            text_run.font_id,
            text_run.x_offset,
            text_run.y_offset,
            true,
        ),
    ]
}

fn rendered_line_segment(
    source: &RenderedLine,
    runs: Vec<RenderedTextRun>,
    font_id: Option<usize>,
    x_offset: f32,
    y_offset: f32,
    trim_public_leading_space: bool,
) -> RenderedLine {
    let text = runs.iter().map(|run| run.text.as_ref()).collect::<String>();
    let text = if trim_public_leading_space {
        text.trim_start().to_string()
    } else {
        text
    };
    RenderedLine {
        text,
        origin: PaintPoint::new(source.origin.x + x_offset, source.origin.y + y_offset),
        font_size: source.font_size,
        font_id,
        color: source.color,
        glyph_ink_bounds: source.glyph_ink_bounds,
        runs: runs
            .into_iter()
            .map(|mut run| {
                run.x_offset -= x_offset;
                run.y_offset -= y_offset;
                run
            })
            .collect(),
        glyph_origin_adjustment: source.glyph_origin_adjustment,
        source: source.source,
        source_run: source.source_run.clone(),
    }
}

/// Return whether two adjacent visible fragments are explicit letter-spacing
/// units from the same authored text run.
///
/// Graph layout splits tracked text into typographic units to apply CSS Text's
/// inter-character spacing.  The public paint summary may join those units
/// again only when the opaque source identity proves they came from one
/// authored run; matching paint attributes alone would wrongly join generated
/// emphasis marks or unrelated adjacent inline content.
pub(in crate::document) fn rendered_lines_can_merge_as_tracking_continuation(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    let (Some(left_source_run), Some(right_source_run)) = (&left.source_run, &right.source_run)
    else {
        return false;
    };
    if !Rc::ptr_eq(left_source_run, right_source_run) {
        return false;
    }
    let y_tolerance = left.font_size.min(right.font_size) * 0.25;
    if (left.origin.y - right.origin.y).abs() > y_tolerance
        || right.origin.x < left.origin.x
        || left.source != right.source
        || left.glyph_origin_adjustment != right.glyph_origin_adjustment
        || (left.font_size - right.font_size).abs() >= 0.01
        || rendered_line_first_significant_font_id(left)
            != rendered_line_first_significant_font_id(right)
        || left.color != right.color
        || left.text.chars().last().is_some_and(char::is_whitespace)
        || right.text.chars().next().is_some_and(char::is_whitespace)
    {
        return false;
    }
    let gap = rendered_line_visual_start(right) - rendered_line_visual_end(left);
    gap > 0.1 && gap <= left.font_size
}

pub(in crate::document) fn rendered_lines_can_merge_with_word_gap(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    if (left.origin.y - right.origin.y).abs() >= 0.01
        || right.origin.x < left.origin.x
        || left.glyph_origin_adjustment != right.glyph_origin_adjustment
        || (left.font_size - right.font_size).abs() >= 0.01
        || left.font_id != right.font_id
        || left.color != right.color
        || left.text.chars().count() <= 1
        || !right.text.chars().all(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    let gap = right.origin.x - left.origin.x - rendered_line_width(left);
    gap > 0.1 && gap <= left.font_size
}

pub(in crate::document) fn rendered_line_visual_start(line: &RenderedLine) -> f32 {
    line.origin.x
        + line
            .runs
            .iter()
            .map(|run| run.x_offset)
            .fold(0.0_f32, f32::min)
}

pub(in crate::document) fn rendered_line_visual_end(line: &RenderedLine) -> f32 {
    line.origin.x + rendered_line_width(line)
}

pub(in crate::document) fn rendered_line_width(line: &RenderedLine) -> f32 {
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

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedTextRun {
    /// Logical source text for this visual glyph run.
    pub text: Rc<str>,
    /// Exact logical replacement text for a shaped cluster that cannot be
    /// represented faithfully by the run's individual glyph ToUnicode
    /// entries.
    ///
    /// PDF's `/ActualText` property supplies the authoritative extraction
    /// value for the enclosed glyph sequence. This is needed when OpenType
    /// shaping emits several glyphs for one source cluster (or attributes a
    /// glyph to an authored default-ignorable control), while `glyphs` still
    /// preserves the selected glyph ids and positions for painting.
    /// ISO 32000-2:2020, 14.9.4.4 "ActualText".
    pub(crate) actual_text: Option<Rc<str>>,
    pub x_offset: f32,
    pub y_offset: f32,
    pub text_matrix: RenderedTextMatrix,
    pub font_size: f32,
    pub font_id: Option<usize>,
    /// The CSS palette selected while shaping this run. This is retained only
    /// until color-glyph extraction has converted COLR glyphs to paint paths.
    pub(crate) font_palette: FontPalette,
    pub glyphs: Option<RenderedGlyphs>,
    /// Original line-local ranges for the rendered glyphs, retained while
    /// layout applies vertical writing-mode placement. PDF emission does not
    /// consume this provenance.
    pub(crate) glyph_source_ranges: Option<Rc<[Option<Range<usize>>]>>,
}

/// PDF text matrix orientation for one shaped text run.
///
/// CSS Writing Modes can place the same shaped glyph stream on a horizontal
/// or vertical baseline. Keeping the 2x2 matrix with the run lets layout own
/// writing-mode placement while PDF emission only applies the selected text
/// matrix:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-flow> and
/// ISO 32000-2:2020, 9.4.4 "Text Space Details".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedTextMatrix(euclid::Transform2D<f32, TextRunSpace, TextRunSpace>);

/// Coordinates local to a shaped run before its writing-mode matrix applies.
/// They are deliberately distinct from page-local paint coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRunSpace {}

pub type TextRunPoint = euclid::Point2D<f32, TextRunSpace>;
/// A signed displacement in a run's local text space.
///
/// SVG character-position lists are expressed in SVG user coordinates, while
/// vertical shaping can rotate that local text space. Keeping the displacement
/// distinct from [`TextRunPoint`] makes the required inverse matrix conversion
/// explicit at the SVG-to-PDF-text boundary.
#[allow(
    dead_code,
    reason = "Retained for the text-matrix API's SVG-independent callers."
)]
pub type TextRunDisplacement = euclid::Vector2D<f32, TextRunSpace>;

impl RenderedTextMatrix {
    pub const IDENTITY: Self = Self(euclid::Transform2D::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0));
    pub const ROTATE_CW: Self = Self(euclid::Transform2D::new(0.0, -1.0, 1.0, 0.0, 0.0, 0.0));
    pub const ROTATE_CCW: Self = Self(euclid::Transform2D::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0));

    pub(crate) fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    /// Construct a text-space linear transform from PDF `Tm` components.
    ///
    /// The translation remains owned by [`RenderedLine`] and
    /// [`RenderedTextRun`] offsets; keeping it out of this type prevents an
    /// SVG user-space origin from being mixed with page-local paint geometry.
    /// ISO 32000-2:2020, 9.4.4 "Text Space Details".
    pub(crate) fn from_pdf_linear_components(components: [f32; 4]) -> Option<Self> {
        components.into_iter().all(f32::is_finite).then(|| {
            Self(euclid::Transform2D::new(
                components[0],
                components[1],
                components[2],
                components[3],
                0.0,
                0.0,
            ))
        })
    }

    /// Scale only the text-space inline axis. SVG `lengthAdjust="spacingAndGlyphs"`
    /// changes glyph geometry and advances along the text direction while
    /// preserving the block-axis glyph metrics.
    #[allow(
        dead_code,
        reason = "Retained for future non-HTML text matrix consumers."
    )]
    pub(crate) fn scaled_inline(self, factor: f32) -> Option<Self> {
        (factor.is_finite() && factor > 0.0).then(|| {
            Self(euclid::Transform2D::new(
                self.0.m11 * factor,
                self.0.m12 * factor,
                self.0.m21,
                self.0.m22,
                0.0,
                0.0,
            ))
        })
    }

    /// Scale the text-space block axis while preserving the inline axis.
    ///
    /// SVG vertical `lengthAdjust="spacingAndGlyphs"` changes the physical
    /// vertical extent of upright glyphs. Those glyphs retain an identity
    /// local text matrix, so the SVG adapter must scale this axis explicitly
    /// instead of treating it as a rotated horizontal run.
    #[allow(
        dead_code,
        reason = "Retained for future non-HTML text matrix consumers."
    )]
    pub(crate) fn scaled_block(self, factor: f32) -> Option<Self> {
        (factor.is_finite() && factor > 0.0).then(|| {
            Self(euclid::Transform2D::new(
                self.0.m11,
                self.0.m12,
                self.0.m21 * factor,
                self.0.m22 * factor,
                0.0,
                0.0,
            ))
        })
    }

    /// Apply a page-space SVG transform after this local text matrix.
    pub(crate) fn transformed_by(self, transform: PaintTransform) -> Self {
        let [a, b, c, d] = self.pdf_components();
        Self(euclid::Transform2D::new(
            transform.a() * a + transform.c() * b,
            transform.b() * a + transform.d() * b,
            transform.a() * c + transform.c() * d,
            transform.b() * c + transform.d() * d,
            0.0,
            0.0,
        ))
    }

    /// Rotate glyph-local coordinates before the existing page-space text
    /// matrix. SVG's `rotate` list rotates an individual character around its
    /// current text position, so the SVG adapter composes this local matrix
    /// before its outer SVG CTM.
    /// <https://www.w3.org/TR/SVG2/text.html#TSpanElementRotateAttribute>
    #[allow(
        dead_code,
        reason = "Retained for future non-HTML text matrix consumers."
    )]
    pub(crate) fn rotated_in_text_space(self, degrees: f32) -> Option<Self> {
        if !degrees.is_finite() {
            return None;
        }
        let (sin, cos) = degrees.to_radians().sin_cos();
        let [a, b, c, d] = self.pdf_components();
        Self::from_pdf_linear_components([
            a * cos + c * sin,
            b * cos + d * sin,
            c * cos - a * sin,
            d * cos - b * sin,
        ])
    }

    pub fn transform_local_point(self, point: TextRunPoint) -> TextRunPoint {
        self.0.transform_point(point)
    }

    /// Convert a displacement expressed after this text matrix back into the
    /// local coordinate system used by a glyph's advances and offsets.
    ///
    /// SVG `dx`/`dy` lists are user-coordinate displacements, not logical
    /// inline/block coordinates. The SVG adapter uses this before applying
    /// the outer SVG CTM, so vertical text does not accidentally rotate its
    /// user-axis position list into the inline axis.
    #[allow(
        dead_code,
        reason = "Retained for future non-HTML text matrix consumers."
    )]
    pub(crate) fn inverse_transform_local_displacement(
        self,
        displacement: TextRunDisplacement,
    ) -> Option<TextRunDisplacement> {
        self.0
            .inverse()
            .map(|inverse| inverse.transform_vector(displacement))
    }

    pub(crate) fn pdf_components(self) -> [f32; 4] {
        [self.0.m11, self.0.m12, self.0.m21, self.0.m22]
    }
}

/// How one shaped text record contributes to PDF text output.
///
/// CSS preserved tabs participate in inline layout but are not glyphs.  PDF
/// emission must advance the text cursor for them without encoding a
/// substitute `.notdef` glyph from whichever font happened to shape the
/// control character:
/// <https://drafts.csswg.org/css-text-3/#tab-size-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderedGlyphKind {
    /// An ordinary paintable glyph from the selected font program.
    Paint(u16),
    VectorPath(u16),
    /// A synthetic layout-control advance with no glyph to paint or subset.
    ///
    /// CSS text characters, including Unicode space separators, must use a
    /// paintable glyph (or the selected font's `.notdef` glyph) instead.
    AdvanceOnly,
}

/// Shaped glyph data kept with painted text for PDF emission.
///
/// CSS Fonts requires text to be shaped with the selected font face before
/// glyph emission; PDF text objects then encode glyph IDs with positioning and
/// ToUnicode extraction data.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedGlyph {
    pub kind: RenderedGlyphKind,
    pub x_advance: f32,
    pub nominal_x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    pub unicode: String,
}

impl RenderedGlyph {
    pub const fn painted_id(&self) -> Option<u16> {
        match self.kind {
            RenderedGlyphKind::Paint(id) | RenderedGlyphKind::VectorPath(id) => Some(id),
            RenderedGlyphKind::AdvanceOnly => None,
        }
    }

    pub const fn is_advance_only(&self) -> bool {
        matches!(self.kind, RenderedGlyphKind::AdvanceOnly)
    }

    pub const fn is_painted_by_vector_path(&self) -> bool {
        matches!(self.kind, RenderedGlyphKind::VectorPath(_))
    }
}

/// Shared immutable glyph storage for painted text runs.
///
/// Layout may clone rendered lines and paint fragments while replaying CSS
/// stacking contexts, shadows, and fragmented content. Glyph records are
/// immutable once they reach `RenderedTextRun`, so sharing the slice preserves
/// the exact shaped output without deep-copying glyph data on each replay:
/// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
/// ISO 32000-2:2020, 9.4 "Text".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedGlyphs(Rc<[RenderedGlyph]>);

impl RenderedGlyphs {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, RenderedGlyph> {
        self.0.iter()
    }
}

impl Deref for RenderedGlyphs {
    type Target = [RenderedGlyph];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a RenderedGlyphs {
    type Item = &'a RenderedGlyph;
    type IntoIter = std::slice::Iter<'a, RenderedGlyph>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl From<Vec<RenderedGlyph>> for RenderedGlyphs {
    fn from(glyphs: Vec<RenderedGlyph>) -> Self {
        Self(Rc::from(glyphs.into_boxed_slice()))
    }
}

impl FromIterator<RenderedGlyph> for RenderedGlyphs {
    fn from_iter<T: IntoIterator<Item = RenderedGlyph>>(iter: T) -> Self {
        iter.into_iter().collect::<Vec<_>>().into()
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::{
        OpaqueTextGlyphCoverage, RenderedGlyph, RenderedGlyphKind, RenderedLine,
        RenderedLineSource, RenderedTextMatrix, RenderedTextPaintSegment, RenderedTextRun,
        rendered_lines_can_merge_as_exact_paint_continuation,
        split_rendered_line_for_opaque_text_coverage,
    };
    use crate::document::paint::geometry::{PaintClip, PaintPoint, PaintTranslation};
    use crate::document::paint::paths::{RenderedPath, RenderedPathFillRule};
    use crate::{CssColor, PaintStrokeWidth};

    fn test_rendered_glyph(unicode: &str) -> RenderedGlyph {
        RenderedGlyph {
            kind: RenderedGlyphKind::Paint(42),
            x_advance: 7.0,
            nominal_x_advance: 7.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: unicode.to_string(),
        }
    }

    fn test_rendered_text_run() -> RenderedTextRun {
        RenderedTextRun {
            text: Rc::from("A"),
            actual_text: None,
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(0),
            font_palette: crate::css::FontPalette::Normal,
            glyphs: Some(vec![test_rendered_glyph("A")].into()),
            glyph_source_ranges: None,
        }
    }

    fn test_rendered_line() -> RenderedLine {
        RenderedLine::from_paint_origin(
            "A".to_string(),
            PaintPoint::new(10.0, 20.0),
            12.0,
            Some(0),
            CssColor::BLACK,
            vec![test_rendered_text_run()],
        )
    }

    #[test]
    fn cloned_text_runs_and_lines_share_rendered_glyph_storage() {
        let run = test_rendered_text_run();
        let cloned_run = run.clone();
        assert!(
            run.glyphs
                .as_ref()
                .unwrap()
                .ptr_eq(cloned_run.glyphs.as_ref().unwrap())
        );

        let line = test_rendered_line();
        let cloned_line = line.clone();
        assert!(
            line.runs[0]
                .glyphs
                .as_ref()
                .unwrap()
                .ptr_eq(cloned_line.runs[0].glyphs.as_ref().unwrap())
        );
    }

    #[test]
    fn line_exposes_a_typed_paint_origin() {
        let line = RenderedLine::from_paint_origin(
            "text".to_string(),
            PaintPoint::new(5.0, 6.0),
            10.0,
            None,
            CssColor::BLACK,
            Vec::new(),
        );
        assert_eq!(line.origin(), PaintPoint::new(5.0, 6.0));
    }

    #[test]
    fn translated_line_moves_its_cached_glyph_ink_bounds_with_its_origin() {
        let line = test_rendered_line()
            .with_glyph_ink_bounds(Some(PaintClip::new(11.0, 12.0, 13.0, 14.0)))
            .translated(PaintTranslation::new(3.0, -4.0));

        assert_eq!(line.origin(), PaintPoint::new(13.0, 16.0));
        assert_eq!(
            line.glyph_ink_bounds,
            Some(PaintClip::new(14.0, 8.0, 13.0, 14.0))
        );
    }

    #[test]
    fn exact_paint_continuation_requires_shared_visual_text_boundary() {
        let source_run = Rc::new(());
        let left = test_rendered_line().with_source_run(Rc::clone(&source_run));
        let mut right = test_rendered_line();
        right.source_run = Some(source_run);
        right.origin = PaintPoint::new(17.0, 20.0);
        right.text = "…".to_string();
        right.runs[0].text = Rc::from("…");

        assert!(rendered_lines_can_merge_as_exact_paint_continuation(
            &left, &right
        ));

        right.origin = PaintPoint::new(17.01, 20.0);
        assert!(!rendered_lines_can_merge_as_exact_paint_continuation(
            &left, &right
        ));

        right.origin = PaintPoint::new(17.0, 20.01);
        assert!(!rendered_lines_can_merge_as_exact_paint_continuation(
            &left, &right
        ));

        right.origin = PaintPoint::new(17.0, 20.0);
        right.color = CssColor::TRANSPARENT;
        assert!(!rendered_lines_can_merge_as_exact_paint_continuation(
            &left, &right
        ));

        right.color = CssColor::BLACK;
        right.source_run = Some(Rc::new(()));
        assert!(!rendered_lines_can_merge_as_exact_paint_continuation(
            &left, &right
        ));

        right.source = RenderedLineSource::BlockEllipsis;
        assert!(rendered_lines_can_merge_as_exact_paint_continuation(
            &left, &right
        ));
    }

    fn opaque_coverage_path() -> RenderedPath {
        RenderedPath::new(
            Vec::new(),
            Some(CssColor::BLACK),
            RenderedPathFillRule::NonZero,
            None,
            PaintStrokeWidth::ZERO,
            None,
        )
    }

    #[test]
    fn opaque_text_coverage_splits_only_the_glyphs_it_owns() {
        let mut run = test_rendered_text_run();
        run.text = Rc::from("AbC");
        run.glyphs = Some(
            vec![
                test_rendered_glyph("A"),
                test_rendered_glyph("b"),
                test_rendered_glyph("C"),
            ]
            .into(),
        );
        let line = RenderedLine::from_paint_origin(
            "AbC".to_string(),
            PaintPoint::new(10.0, 20.0),
            12.0,
            Some(0),
            CssColor::BLACK,
            vec![run],
        );

        let segments = split_rendered_line_for_opaque_text_coverage(
            line,
            vec![
                OpaqueTextGlyphCoverage {
                    run_index: 0,
                    glyph_index: 0,
                    path: opaque_coverage_path(),
                },
                OpaqueTextGlyphCoverage {
                    run_index: 0,
                    glyph_index: 2,
                    path: opaque_coverage_path(),
                },
            ],
        );

        assert_eq!(segments.len(), 3);
        let segment_text = |segment: &RenderedTextPaintSegment| match segment {
            RenderedTextPaintSegment::Text(line) => {
                (false, line.text.clone(), line.runs[0].x_offset)
            }
            RenderedTextPaintSegment::OpaqueCoverage { line, .. } => {
                (true, line.text.clone(), line.runs[0].x_offset)
            }
        };
        assert_eq!(segment_text(&segments[0]), (true, "A".to_string(), 0.0));
        assert_eq!(segment_text(&segments[1]), (false, "b".to_string(), 7.0));
        assert_eq!(segment_text(&segments[2]), (true, "C".to_string(), 14.0));
    }

    #[test]
    fn opaque_text_coverage_preserves_fully_covered_actual_text_runs() {
        let mut run = test_rendered_text_run();
        run.actual_text = Some(Rc::from("ab"));
        run.text = Rc::from("ab");
        run.glyphs = Some(vec![test_rendered_glyph("a"), test_rendered_glyph("b")].into());
        let line = RenderedLine::from_paint_origin(
            "ab".to_string(),
            PaintPoint::new(10.0, 20.0),
            12.0,
            Some(0),
            CssColor::BLACK,
            vec![run],
        );

        let segments = split_rendered_line_for_opaque_text_coverage(
            line,
            vec![
                OpaqueTextGlyphCoverage {
                    run_index: 0,
                    glyph_index: 0,
                    path: opaque_coverage_path(),
                },
                OpaqueTextGlyphCoverage {
                    run_index: 0,
                    glyph_index: 1,
                    path: opaque_coverage_path(),
                },
            ],
        );

        assert_eq!(segments.len(), 1);
        let RenderedTextPaintSegment::OpaqueCoverage { line, paths } = &segments[0] else {
            panic!("fully covered ActualText runs must retain opaque coverage");
        };
        assert_eq!(line.runs[0].actual_text.as_deref(), Some("ab"));
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn opaque_text_coverage_preserves_partially_covered_actual_text_runs() {
        let mut run = test_rendered_text_run();
        run.actual_text = Some(Rc::from("ab"));
        run.text = Rc::from("ab");
        run.glyphs = Some(vec![test_rendered_glyph("a"), test_rendered_glyph("b")].into());
        let line = RenderedLine::from_paint_origin(
            "ab".to_string(),
            PaintPoint::new(10.0, 20.0),
            12.0,
            Some(0),
            CssColor::BLACK,
            vec![run],
        );

        let segments = split_rendered_line_for_opaque_text_coverage(
            line,
            vec![OpaqueTextGlyphCoverage {
                run_index: 0,
                glyph_index: 0,
                path: opaque_coverage_path(),
            }],
        );

        assert_eq!(segments.len(), 1);
        let RenderedTextPaintSegment::Text(line) = &segments[0] else {
            panic!("partially covered ActualText runs must remain ordinary text");
        };
        assert_eq!(line.runs[0].actual_text.as_deref(), Some("ab"));
    }
}
