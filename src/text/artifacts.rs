//! Durable shaped-line artifacts and their authored-text provenance.
//!
//! This is the boundary between backend shaping and the line-layout/PDF
//! consumers that retain its results. Every optional glyph source range indexes
//! the authored text held by the durable shaped line.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapedGlyphRun {
    pub(crate) text: Rc<str>,
    pub(crate) x_offset: f32,
    pub(crate) y_offset: f32,
    pub(crate) text_matrix: crate::document::paint::text::RenderedTextMatrix,
    pub(crate) font_size: f32,
    pub(crate) font_id: Option<usize>,
    pub(crate) font_palette: crate::css::FontPalette,
    pub(crate) glyphs: Vec<RenderedGlyph>,
    /// Source byte ranges in the formatted input, one per emitted glyph.
    ///
    /// A cluster may emit several glyphs, which therefore share a range.  The
    /// range is kept out of the public PDF glyph record: it is layout-time
    /// provenance used when a selected soft-wrapped line reuses shaping from
    /// its unbroken source run.
    pub(crate) glyph_source_ranges: Vec<Option<Range<usize>>>,
}

/// A durable shaped CSS inline line.
///
/// The line stores the formatted text summary separately from the visual glyph
/// runs. CSS Text owns line breaking and trimming, while Parley owns shaping,
/// bidi visual order, glyph advances, and fallback font selection. Keeping this
/// artifact through painting and PDF emission prevents later reshaping from
/// disagreeing with the line-break decision:
/// <https://www.w3.org/TR/css-text-3/#text-processing-order>,
/// <https://www.w3.org/TR/css-writing-modes-4/#bidi-linebox>, and
/// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapedInlineLine {
    pub(crate) text: Rc<str>,
    pub(crate) width: f32,
    pub(crate) offset: f32,
    pub(crate) aligned_by_parley: bool,
    pub(crate) line_height: f32,
    pub(crate) baseline_adjustment: f32,
    pub(crate) typesetting_plan: TextTypesettingPlan,
    pub(crate) runs: Vec<ShapedInlineRun>,
}

impl ShapedInlineLine {
    pub(crate) fn first_font_id(&self) -> Option<usize> {
        self.runs.iter().find_map(|run| run.font_id)
    }

    pub(crate) fn advance_width(&self) -> f32 {
        self.runs
            .iter()
            .map(|run| {
                run.x_offset
                    + run
                        .glyphs
                        .iter()
                        .map(|glyph| glyph.rendered.x_advance)
                        .sum::<f32>()
            })
            .fold(0.0, f32::max)
    }

    /// Remove the shaper-owned terminal tracking advance from this fragment.
    ///
    /// CSS Text applies `letter-spacing` only between typographic character
    /// units. Graph inline layout resolves those boundaries after UAX #9
    /// reordering, so any advance supplied by the shaping backend at a
    /// fragment's logical end must be removed from the glyph record as well
    /// as from its measured width. Leaving it in the glyph would make paint
    /// disagree with fitting and intrinsic sizing.
    /// <https://drafts.csswg.org/css-text-3/#letter-spacing-property>
    pub(crate) fn remove_terminal_letter_spacing(&mut self, spacing: f32) {
        if spacing == 0.0 {
            return;
        }

        // Format and bidi controls can be retained in the source summary but
        // do not paint. The backend assigns their zero-width cluster after
        // the preceding visible glyph, so select the final paintable glyph
        // rather than assuming the final source range owns the advance.
        // Re-homed fallback glyphs do not necessarily retain source
        // provenance, but still carry the backend terminal advance.
        let mut terminal = None;
        for (run_index, run) in self.runs.iter().enumerate() {
            for (glyph_index, glyph) in run.glyphs.iter().enumerate() {
                if glyph.paints {
                    terminal = Some((run_index, glyph_index));
                }
            }
        }
        if let Some((run_index, glyph_index)) = terminal {
            self.runs[run_index].glyphs[glyph_index].rendered.x_advance -= spacing;
            self.width = self.advance_width();
        }
    }

    /// Extract a selected source range without re-running contextual shaping.
    ///
    /// CSS Text soft wrapping selects source ranges after text shaping
    /// context has been established. Re-shaping just the range would turn a
    /// medial Arabic glyph into an isolated or final form. This retains every
    /// fully selected glyph cluster from the original visual runs, while
    /// rejecting a slice through a cluster so callers can take the normal
    /// shaping path instead:
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
    pub(crate) fn source_slice(&self, range: Range<usize>) -> Option<Self> {
        if range.start >= range.end
            || range.end > self.text.len()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            return None;
        }

        let mut runs = Vec::new();
        let mut selected_source_ranges = Vec::new();
        for run in &self.runs {
            let mut prefix_advance = 0.0;
            let mut glyphs = Vec::new();
            let mut source_start = None;
            let mut source_end = None;
            for glyph in &run.glyphs {
                let Some(glyph_range) = glyph.source_range.as_ref() else {
                    // The fallback shaper has no cluster provenance. It is
                    // still safe to shape the selected source range anew.
                    return None;
                };
                let overlaps = glyph_range.start < range.end && range.start < glyph_range.end;
                if overlaps && (glyph_range.start < range.start || glyph_range.end > range.end) {
                    return None;
                }
                if glyph_range.start >= range.start && glyph_range.end <= range.end {
                    selected_source_ranges.push(glyph_range.clone());
                    source_start = Some(source_start.map_or(glyph_range.start, |start: usize| {
                        start.min(glyph_range.start)
                    }));
                    source_end = Some(
                        source_end.map_or(glyph_range.end, |end: usize| end.max(glyph_range.end)),
                    );
                    let mut glyph = glyph.clone();
                    glyph.source_range =
                        Some(glyph_range.start - range.start..glyph_range.end - range.start);
                    glyphs.push(glyph);
                } else {
                    prefix_advance += glyph.rendered.x_advance;
                }
            }
            if glyphs.is_empty() {
                continue;
            }
            let source_range = source_start.zip(source_end)?;
            if !self.text.is_char_boundary(source_range.0)
                || !self.text.is_char_boundary(source_range.1)
            {
                // Some backend runs are built from a text-normalized input
                // (for example after removing a font-neutral control). Their
                // cluster coordinates cannot safely index this source string,
                // so retain the conventional selected-range shape instead.
                return None;
            }
            runs.push(ShapedInlineRun {
                text: Rc::from(&self.text[source_range.0..source_range.1]),
                x_offset: run.x_offset + prefix_advance,
                font_size: run.font_size,
                font_id: run.font_id,
                font_palette: run.font_palette.clone(),
                glyphs,
                paints: run.paints,
            });
        }
        // Source shaping is only reusable when its glyph provenance covers
        // every paintable character in the selected CSS Text range. A
        // default-ignorable control (notably a soft hyphen) may cause a
        // backend to report discontinuous or truncated cluster ranges. Using
        // such a partial slice would under-measure the selected line and can
        // make later content incorrectly fit after a hyphenation break.
        // Re-shaping is the conservative fallback; normal complete slices
        // retain the original contextual shaping.
        // <https://www.w3.org/TR/css-text-3/#line-breaking>
        for (offset, character) in self.text[range.clone()].char_indices() {
            if character_is_default_ignorable_code_point(character) {
                continue;
            }
            let character_range = range.start + offset..range.start + offset + character.len_utf8();
            if !selected_source_ranges.iter().any(|glyph_range| {
                glyph_range.start <= character_range.start && glyph_range.end >= character_range.end
            }) {
                return None;
            }
        }
        let left_edge = runs.iter().map(|run| run.x_offset).min_by(f32::total_cmp)?;
        for run in &mut runs {
            run.x_offset -= left_edge;
        }
        let mut selected = Self {
            text: Rc::from(&self.text[range.clone()]),
            width: 0.0,
            offset: self.offset,
            aligned_by_parley: self.aligned_by_parley,
            line_height: self.line_height,
            baseline_adjustment: self.baseline_adjustment,
            typesetting_plan: self.typesetting_plan.source_slice(range)?,
            runs,
        };
        selected.width = selected.advance_width();
        Some(selected)
    }

    /// Return the advance of a cluster-aligned source range without copying
    /// text, glyphs, or shaping runs.
    ///
    /// This has the same source-provenance requirements and visual-run
    /// geometry as [`Self::source_slice`], but is intended for speculative
    /// inline-line fitting where a durable shaped slice is not needed.
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>
    pub(crate) fn source_range_advance_width(&self, range: Range<usize>) -> Option<f32> {
        if range.start >= range.end
            || range.end > self.text.len()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            return None;
        }

        let mut left_edge = None::<f32>;
        let mut right_edge = None::<f32>;
        for run in &self.runs {
            let mut skipped_advance = 0.0;
            let mut selected_advance = 0.0;
            let mut has_selected_glyph = false;
            for glyph in &run.glyphs {
                let glyph_range = glyph.source_range.as_ref()?;
                let overlaps = glyph_range.start < range.end && range.start < glyph_range.end;
                if overlaps && (glyph_range.start < range.start || glyph_range.end > range.end) {
                    return None;
                }
                if glyph_range.start >= range.start && glyph_range.end <= range.end {
                    has_selected_glyph = true;
                    selected_advance += glyph.rendered.x_advance;
                } else {
                    skipped_advance += glyph.rendered.x_advance;
                }
            }
            if has_selected_glyph {
                let run_start = run.x_offset + skipped_advance;
                let run_end = run_start + selected_advance;
                left_edge = Some(left_edge.map_or(run_start, |edge| edge.min(run_start)));
                right_edge = Some(right_edge.map_or(run_end, |edge| edge.max(run_end)));
            }
        }

        // Retain the conservative source-slice rule: every non-ignorable
        // source character must have glyph provenance covering its full
        // source span, otherwise callers must use a conventional re-shape.
        for (offset, character) in self.text[range.clone()].char_indices() {
            if character_is_default_ignorable_code_point(character) {
                continue;
            }
            let character_range = range.start + offset..range.start + offset + character.len_utf8();
            if !self.runs.iter().flat_map(|run| &run.glyphs).any(|glyph| {
                glyph.source_range.as_ref().is_some_and(|glyph_range| {
                    glyph_range.start <= character_range.start
                        && glyph_range.end >= character_range.end
                })
            }) {
                return None;
            }
        }

        Some(right_edge? - left_edge?)
    }

    /// Return the visual inline span occupied by an authored source range.
    ///
    /// A decoration can propagate through an inline boundary without making
    /// that boundary a shaping boundary.  The boundary's receiver range must
    /// consequently be recovered from the shared shaped clusters, rather than
    /// by re-shaping either lexical side independently.  A cluster crossing
    /// the source boundary contributes its complete advance to every
    /// overlapping receiver: the glyph is indivisible at this stage, while
    /// the receiver identities remain distinct for decoration propagation.
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping>
    pub(crate) fn source_range_inline_span(&self, range: Range<usize>) -> Option<(f32, f32)> {
        if range.start >= range.end
            || range.end > self.text.len()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            return None;
        }

        let mut start = None::<f32>;
        let mut end = None::<f32>;
        for run in &self.runs {
            let mut pen = 0.0;
            for glyph in &run.glyphs {
                let glyph_range = glyph.source_range.as_ref()?;
                if glyph_range.start < range.end && range.start < glyph_range.end {
                    let glyph_start = run.x_offset + pen + glyph.rendered.x_offset;
                    let glyph_end = run.x_offset + pen + glyph.rendered.x_advance;
                    start = Some(start.map_or(glyph_start, |edge| edge.min(glyph_start)));
                    end = Some(end.map_or(glyph_end, |edge| edge.max(glyph_end)));
                }
                pen += glyph.rendered.x_advance;
            }
        }

        for (offset, character) in self.text[range.clone()].char_indices() {
            if character_is_default_ignorable_code_point(character) {
                continue;
            }
            let character_range = range.start + offset..range.start + offset + character.len_utf8();
            if !self.runs.iter().flat_map(|run| &run.glyphs).any(|glyph| {
                glyph.source_range.as_ref().is_some_and(|glyph_range| {
                    glyph_range.start <= character_range.start
                        && glyph_range.end >= character_range.end
                })
            }) {
                return None;
            }
        }

        Some((start?, end?))
    }

    pub(crate) fn rendered_runs(&self) -> Vec<RenderedTextRun> {
        const EPSILON: f32 = 0.01;

        let mut output = Vec::with_capacity(self.runs.len());
        let mut next_group_start = 0usize;
        while let Some((first_index, first_run)) =
            next_paintable_shaped_run(&self.runs, next_group_start)
        {
            let mut last_index = first_index;
            let mut previous_end = first_run.x_offset + shaped_run_advance(first_run);
            let mut search_index = first_index + 1;
            next_group_start = loop {
                let Some((candidate_index, candidate)) =
                    next_paintable_shaped_run(&self.runs, search_index)
                else {
                    break self.runs.len();
                };
                if !shaped_runs_are_render_compatible(first_run, previous_end, candidate, EPSILON) {
                    break candidate_index;
                }

                last_index = candidate_index;
                previous_end = candidate.x_offset + shaped_run_advance(candidate);
                search_index = candidate_index + 1;
            };
            output.push(materialize_rendered_run(
                &self.runs[first_index..=last_index],
            ));
        }
        output
    }

    /// Expand inter-word justification separators in shaped glyph runs.
    ///
    /// CSS Text applies inter-word justification to word separators in the
    /// shaped line, after bidi reordering and glyph selection. Mutating the
    /// shaped glyph advances keeps PDF emission on the same Parley-selected
    /// glyph runs instead of reconstructing the line from scalar fragment
    /// offsets:
    /// <https://www.w3.org/TR/css-text-3/#valdef-text-justify-inter-word>,
    /// <https://www.w3.org/TR/css-writing-modes-4/#bidi-linebox>, and
    /// ISO 32000-2:2020, 9.4 "Text".
    pub(crate) fn apply_inter_word_justification(
        &mut self,
        extra_per_separator: f32,
        max_separators: usize,
    ) -> f32 {
        if extra_per_separator <= 0.0 || max_separators == 0 {
            return 0.0;
        }

        let mut opportunities = Vec::new();
        for (run_index, run) in self.runs.iter().enumerate() {
            let mut pen_x = 0.0;
            for (glyph_index, glyph) in run.glyphs.iter().enumerate() {
                if shaped_glyph_is_inter_word_separator(glyph) {
                    opportunities.push(ShapedJustificationOpportunity {
                        run_index,
                        glyph_index,
                        visual_end: run.x_offset + pen_x + glyph.rendered.x_advance,
                        separator_count: glyph.source_text().chars().count().max(1),
                    });
                }
                pen_x += glyph.rendered.x_advance;
            }
        }
        opportunities.sort_by(|left, right| {
            left.visual_end
                .total_cmp(&right.visual_end)
                .then(left.run_index.cmp(&right.run_index))
                .then(left.glyph_index.cmp(&right.glyph_index))
        });

        let mut applied = 0usize;
        let mut added_width = 0.0;
        for opportunity in opportunities {
            if applied >= max_separators {
                break;
            }
            let separator_count = opportunity
                .separator_count
                .min(max_separators.saturating_sub(applied));
            let extra = extra_per_separator * separator_count as f32;
            let Some(glyph) = self
                .runs
                .get_mut(opportunity.run_index)
                .and_then(|run| run.glyphs.get_mut(opportunity.glyph_index))
            else {
                continue;
            };
            glyph.rendered.x_advance += extra;
            for (run_index, run) in self.runs.iter_mut().enumerate() {
                if run_index != opportunity.run_index
                    && run.x_offset + 0.01 >= opportunity.visual_end + added_width
                {
                    run.x_offset += extra;
                }
            }
            applied += separator_count;
            added_width += extra;
        }
        self.width += added_width;
        added_width
    }
}

/// Return the next shaped run that contributes paintable glyphs.
fn next_paintable_shaped_run(
    runs: &[ShapedInlineRun],
    start: usize,
) -> Option<(usize, &ShapedInlineRun)> {
    runs.iter()
        .enumerate()
        .skip(start)
        .find(|(_, run)| run.paints && !run.glyphs.is_empty())
}

fn shaped_run_advance(run: &ShapedInlineRun) -> f32 {
    run.glyphs
        .iter()
        .map(|glyph| glyph.rendered.x_advance)
        .sum()
}

/// Return whether two shaped runs can share one emitted PDF text object.
///
/// [`ShapedInlineRun`] materializes every run with an identity text matrix and
/// a zero cross-axis offset. Their font properties and inline continuity are
/// therefore the complete compatibility key at this boundary.
fn shaped_runs_are_render_compatible(
    previous: &ShapedInlineRun,
    previous_end: f32,
    candidate: &ShapedInlineRun,
    epsilon: f32,
) -> bool {
    previous.font_id == candidate.font_id
        && previous.font_size == candidate.font_size
        && previous.font_palette == candidate.font_palette
        && (previous_end - candidate.x_offset).abs() <= epsilon
}

/// Materialize one compatible visual shaped-run sequence for PDF emission.
///
/// Source slicing preserves the glyph forms chosen while shaping across an
/// inline boundary, but it can leave adjacent slices of the same visual font
/// run as separate PDF text objects. Materializing the already-coalesced
/// sequence retains those glyphs and their advances while avoiding temporary
/// per-slice rendered glyph vectors:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>.
fn materialize_rendered_run(runs: &[ShapedInlineRun]) -> RenderedTextRun {
    let first = runs
        .iter()
        .find(|run| run.paints && !run.glyphs.is_empty())
        .expect("rendered run groups always contain a paintable shaped run");
    let (paintable_run_count, text_capacity, glyph_capacity, needs_actual_text) = runs
        .iter()
        .filter(|run| run.paints && !run.glyphs.is_empty())
        .fold(
            (0usize, 0usize, 0usize, false),
            |(count, text, glyphs, actual), run| {
                (
                    count + 1,
                    text + run.text.len(),
                    glyphs + run.glyphs.len(),
                    actual || shaped_run_needs_actual_text(run),
                )
            },
        );
    let mut glyphs = Vec::with_capacity(glyph_capacity);
    let mut glyph_source_ranges = Vec::with_capacity(glyph_capacity);
    for run in runs
        .iter()
        .filter(|run| run.paints && !run.glyphs.is_empty())
    {
        glyphs.extend(run.glyphs.iter().map(|glyph| glyph.rendered.clone()));
        glyph_source_ranges.extend(run.glyphs.iter().map(|glyph| glyph.source_range.clone()));
    }
    let text = if paintable_run_count == 1 {
        Rc::clone(&first.text)
    } else {
        let mut text = String::with_capacity(text_capacity);
        for run in runs
            .iter()
            .filter(|run| run.paints && !run.glyphs.is_empty())
        {
            text.push_str(&run.text);
        }
        text.into()
    };
    let actual_text = needs_actual_text
        .then(|| Rc::clone(&text))
        .filter(|text| !text.is_empty());

    RenderedTextRun {
        text,
        actual_text,
        x_offset: first.x_offset,
        y_offset: 0.0,
        text_matrix: crate::document::paint::text::RenderedTextMatrix::IDENTITY,
        font_size: first.font_size,
        font_id: first.font_id,
        font_palette: first.font_palette.clone(),
        glyphs: Some(glyphs.into()),
        glyph_source_ranges: Some(glyph_source_ranges.into()),
    }
}

#[derive(Debug, Clone, Copy)]
struct ShapedJustificationOpportunity {
    run_index: usize,
    glyph_index: usize,
    visual_end: f32,
    separator_count: usize,
}

fn shaped_glyph_is_inter_word_separator(glyph: &ShapedInlineGlyph) -> bool {
    !glyph.source_text().is_empty()
        && glyph
            .source_text()
            .chars()
            .all(character_is_css_word_separator)
}

/// A shaped visual run inside a CSS line box.
///
/// Runs keep the resolved document font id chosen during shaping, so later PDF
/// embedding uses the same font that measured and positioned the glyphs:
/// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm> and
/// ISO 32000-2:2020, 9.6 "Simple Fonts" / 9.7 "Composite Fonts".
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapedInlineRun {
    pub(crate) text: Rc<str>,
    pub(crate) x_offset: f32,
    pub(crate) font_size: f32,
    pub(crate) font_id: Option<usize>,
    pub(crate) font_palette: crate::css::FontPalette,
    pub(crate) glyphs: Vec<ShapedInlineGlyph>,
    pub(crate) paints: bool,
}

impl ShapedInlineRun {
    /// Materialize this individual shaped run for consumers that do not have
    /// a whole line available to coalesce first.
    #[cfg(test)]
    pub(super) fn rendered_run(&self) -> RenderedTextRun {
        materialize_rendered_run(std::slice::from_ref(self))
    }
}

/// Return whether a shaped run needs PDF `/ActualText` for extraction.
///
/// A shaped glyph sequence is not necessarily a one-to-one spelling of its
/// source text. In particular, HarfBuzz reports an `fi` ligature at the source
/// `f` cluster, so its glyph-level ToUnicode summary is only `f`. A PDF
/// ToUnicode CMap is keyed by glyph/CID and cannot restore the omitted source
/// character when that same glyph is also used by another run. Attach the
/// exact run-local replacement text whenever the glyph summaries cannot
/// reproduce the authored source.
/// ISO 32000-2:2020, 14.9.4.4, "ActualText".
fn shaped_run_needs_actual_text(run: &ShapedInlineRun) -> bool {
    !run.glyphs
        .iter()
        .flat_map(|glyph| glyph.rendered.unicode.chars())
        .eq(run.text.chars())
        || run
            .glyphs
            .iter()
            .any(|glyph| glyph.rendered.is_advance_only())
}

/// A shaped glyph record with its source cluster summary.
///
/// PDF text output uses the glyph id and advance, while ToUnicode extraction
/// uses the source Unicode summary. Default-ignorable or control-only clusters
/// can therefore shape surrounding text without being forced to paint:
/// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
/// ISO 32000-2:2020, 9.10.3 "ToUnicode CMaps".
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapedInlineGlyph {
    pub(crate) rendered: RenderedGlyph,
    pub(crate) paints: bool,
    /// Source byte range in [`ShapedInlineLine::text`], when retained by the
    /// shaping backend.
    pub(crate) source_range: Option<Range<usize>>,
}

impl ShapedInlineGlyph {
    pub(crate) fn source_text(&self) -> &str {
        &self.rendered.unicode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::FontPalette;
    use crate::document::paint::text::{RenderedGlyph, RenderedGlyphKind};

    fn shaped_run(text: &str, x_offset: f32, font_palette: FontPalette) -> ShapedInlineRun {
        ShapedInlineRun {
            text: Rc::from(text),
            x_offset,
            font_size: 12.0,
            font_id: Some(0),
            font_palette,
            glyphs: vec![ShapedInlineGlyph {
                rendered: RenderedGlyph {
                    kind: RenderedGlyphKind::Paint(1),
                    x_advance: 6.0,
                    nominal_x_advance: 6.0,
                    x_offset: 0.0,
                    y_offset: 0.0,
                    unicode: text.to_string(),
                },
                paints: true,
                source_range: Some(0..text.len()),
            }],
            paints: true,
        }
    }

    #[test]
    fn rendered_runs_do_not_coalesce_different_font_palettes() {
        let line = ShapedInlineLine {
            text: Rc::from("AB"),
            width: 12.0,
            offset: 0.0,
            aligned_by_parley: false,
            line_height: 14.4,
            baseline_adjustment: 0.0,
            typesetting_plan: TextTypesettingPlan::Horizontal,
            runs: vec![
                shaped_run("A", 0.0, FontPalette::Index(0)),
                shaped_run("B", 6.0, FontPalette::Index(1)),
            ],
        };

        let rendered = line.rendered_runs();

        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[0].font_palette, FontPalette::Index(0));
        assert_eq!(rendered[1].font_palette, FontPalette::Index(1));
    }

    #[test]
    fn rendered_runs_materialize_a_compatible_sequence_once() {
        let mut ligature_slice = shaped_run("fi", 0.0, FontPalette::Normal);
        ligature_slice.glyphs[0].rendered.unicode = "f".to_owned();
        let line = ShapedInlineLine {
            text: Rc::from("fiX"),
            width: 12.0,
            offset: 0.0,
            aligned_by_parley: false,
            line_height: 14.4,
            baseline_adjustment: 0.0,
            typesetting_plan: TextTypesettingPlan::Horizontal,
            runs: vec![ligature_slice, shaped_run("X", 6.0, FontPalette::Normal)],
        };

        let rendered = line.rendered_runs();

        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0].text.as_ref(), "fiX");
        assert_eq!(rendered[0].actual_text.as_deref(), Some("fiX"));
        let glyphs = rendered[0].glyphs.as_ref().unwrap();
        assert_eq!(glyphs.len(), 2);
        assert_eq!(glyphs[0].unicode, "f");
        assert_eq!(glyphs[1].unicode, "X");
        assert_eq!(
            glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>(),
            12.0
        );
    }
}
