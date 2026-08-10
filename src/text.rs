use crate::css::{
    ComputedStyle, CssFontFace, Direction, FontFaceSource, FontFamily, FontFeatureValue,
    FontFeatureValues, FontFeatureValuesBlock, FontKerning, FontPalette, FontPaletteValues,
    FontStyle, FontVariantAlternates, FontVariantCaps, FontVariantEastAsian,
    FontVariantEastAsianValue, FontVariantEmoji, FontVariantLigatures, FontVariantNumeric,
    FontVariantNumericValue, FontVariantPosition, FontVariationSettings, FontWeight, FontWidth,
    HyphenateLimitChars, LineBreak as CssLineBreak, OverflowWrap as CssOverflowWrap,
    StylesheetCollection, TextLayoutPolicy, TextOrientation, UnicodeBidi, UnicodeRange,
    WordBreak as CssWordBreak,
};
use crate::document::paint::text::{RenderedGlyph, RenderedGlyphKind, RenderedTextRun};
use crate::document::{DocumentFont, FontProgramKind};
use crate::units::{LayoutLength, SemanticLengthExt, layout_points, layout_pt};
use base64::Engine as _;
use fontique::{
    Attributes as FontiqueAttributes, Blob as FontiqueBlob, FallbackKey as FontiqueFallbackKey,
    FontInfoOverride, FontStyle as FontiqueFontStyle, FontWeight as FontiqueFontWeight,
    FontWidth as FontiqueFontWidth, GenericFamily as FontiqueGenericFamily,
    QueryFamily as FontiqueQueryFamily, QueryFont as FontiqueQueryFont,
    QueryStatus as FontiqueQueryStatus, Script as FontiqueScript,
};
use hyphenation::{Hyphenator, Language, Load, Standard};
use icu_properties::{
    CodePointMapData, CodePointMapDataBorrowed, CodePointSetData, CodePointSetDataBorrowed,
    PropertyNamesShort,
    props::{
        BidiClass, BidiControl, BidiMirroringGlyph, DefaultIgnorableCodePoint, EastAsianWidth,
        Emoji, EmojiPresentation, GeneralCategory, GeneralCategoryGroup, JoinControl, JoiningType,
        LineBreak, Script as IcuScript, VerticalOrientation, WordBreak as IcuWordBreak,
    },
};
use icu_segmenter::options::{
    LineBreakOptions, LineBreakStrictness, LineBreakWordOption, WordBreakInvariantOptions,
};
use icu_segmenter::{GraphemeClusterSegmenter, LineSegmenter, WordSegmenter};
use parley::setting::Tag as ParleyTag;
use parley::style::FontFeature as ParleyFontFeature;
use parley::{
    FontContext as ParleyFontContext, FontFamily as ParleyFontFamily,
    FontFeatures as ParleyFontFeatures, FontStyle as ParleyFontStyle,
    FontVariation as ParleyFontVariation, FontVariations as ParleyFontVariations,
    FontWeight as ParleyFontWeight, FontWidth as ParleyFontWidth, Language as ParleyLanguage,
    Layout as ParleyLayout, LayoutContext as ParleyLayoutContext, LineHeight as ParleyLineHeight,
    OverflowWrap as ParleyOverflowWrap, StyleProperty, TextWrapMode as ParleyTextWrapMode,
    WordBreak as ParleyWordBreak,
};
use read_fonts::types::Tag as OpenTypeTag;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) struct FontSystem {
    parley_font_context: ParleyFontContext,
    parley_layout_context: ParleyLayoutContext<FontPalette>,
    /// Reused temporary layout storage for Parley shaping passes.
    ///
    /// Each pass extracts owned glyph data before returning, so retaining this
    /// layout only keeps Parley's internal vectors available for the next
    /// shape in the same document.
    parley_layout_scratch: ParleyLayout<FontPalette>,
    document_fonts: DocumentFontRegistry,
    family_cache: HashMap<FontRequest, usize>,
    fallback_cache: HashMap<FallbackRequest, Option<usize>>,
    font_feature_values: FontFeatureValues,
    font_palette_values: FontPaletteValues,
}

impl Clone for FontSystem {
    fn clone(&self) -> Self {
        Self {
            parley_font_context: self.parley_font_context.clone(),
            parley_layout_context: self.parley_layout_context.clone(),
            // Scratch capacity belongs to one shaping session. Cloning it
            // would copy temporary glyph/run storage and defeat that purpose.
            parley_layout_scratch: ParleyLayout::default(),
            document_fonts: self.document_fonts.clone(),
            family_cache: self.family_cache.clone(),
            fallback_cache: self.fallback_cache.clone(),
            font_feature_values: self.font_feature_values.clone(),
            font_palette_values: self.font_palette_values.clone(),
        }
    }
}

pub(crate) struct FontSystemLoad {
    #[cfg(not(target_arch = "wasm32"))]
    parley_font_context: tokio::task::JoinHandle<LoadedParleyFontContext>,
    #[cfg(target_arch = "wasm32")]
    parley_font_context: LoadedParleyFontContext,
}

pub(crate) struct FontSystemSeedLoad {
    #[cfg(not(target_arch = "wasm32"))]
    parley_font_context: tokio::task::JoinHandle<LoadedParleyFontContext>,
    #[cfg(target_arch = "wasm32")]
    parley_font_context: LoadedParleyFontContext,
    #[cfg(not(target_arch = "wasm32"))]
    font_faces: tokio::task::JoinHandle<crate::Result<Vec<LoadedFontFace>>>,
    #[cfg(target_arch = "wasm32")]
    font_faces: Vec<CssFontFace>,
    #[cfg(target_arch = "wasm32")]
    resource_fetcher: crate::resource::ResourceFetcher,
    font_feature_values: FontFeatureValues,
    font_palette_values: FontPaletteValues,
}

struct FontSystemSeed {
    parley_font_context: ParleyFontContext,
    registered_font_faces: HashMap<RegisteredFontFaceKey, RegisteredFontFaceMetadata>,
    font_feature_values: FontFeatureValues,
    font_palette_values: FontPaletteValues,
}

struct LoadedParleyFontContext {
    parley_font_context: ParleyFontContext,
}

struct LoadedFontFace {
    font_face: CssFontFace,
    data: Option<FontiqueBlob<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FontFaceFeatureDefaults {
    font_feature_settings: crate::css::FontFeatureSettings,
    font_variant_ligatures: FontVariantLigatures,
    font_variant_position: FontVariantPosition,
    font_variant_caps: FontVariantCaps,
    font_variant_numeric: FontVariantNumeric,
    font_variant_alternates: FontVariantAlternates,
    font_variant_east_asian: FontVariantEastAsian,
}

impl FontFaceFeatureDefaults {
    fn from_font_face(font_face: &CssFontFace) -> Self {
        Self {
            font_feature_settings: font_face.font_feature_settings.clone(),
            font_variant_ligatures: font_face.font_variant_ligatures,
            font_variant_position: font_face.font_variant_position,
            font_variant_caps: font_face.font_variant_caps,
            font_variant_numeric: font_face.font_variant_numeric.clone(),
            font_variant_alternates: font_face.font_variant_alternates.clone(),
            font_variant_east_asian: font_face.font_variant_east_asian.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StyledTextSpan<'a> {
    pub(crate) text: &'a str,
    pub(crate) style: &'a ComputedStyle,
}

/// Block-axis extents around a text baseline in Quire layout units.
///
/// OpenType stores ascenders and descenders in font design units, while CSS
/// inline layout consumes scaled lengths above and below a shared baseline.
/// Keeping the scaled values typed prevents callers from mixing raw font-table
/// integers with CSS/PDF layout lengths:
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FontRunVerticalExtents {
    pub(crate) above_baseline: LayoutLength,
    pub(crate) below_baseline: LayoutLength,
}

impl FontRunVerticalExtents {
    pub(crate) fn from_points(above_baseline: f32, below_baseline: f32) -> Self {
        Self {
            above_baseline: layout_pt(above_baseline),
            below_baseline: layout_pt(below_baseline),
        }
    }

    pub(crate) fn block_size(self) -> LayoutLength {
        self.above_baseline + self.below_baseline
    }

    pub(crate) fn with_symmetric_leading(self, leading: LayoutLength) -> Self {
        Self {
            above_baseline: self.above_baseline + leading,
            below_baseline: self.below_baseline + leading,
        }
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            above_baseline: layout_pt(
                layout_points(self.above_baseline).max(layout_points(other.above_baseline)),
            ),
            below_baseline: layout_pt(
                layout_points(self.below_baseline).max(layout_points(other.below_baseline)),
            ),
        }
    }
}

/// Resolved vertical metrics for one inline text box.
///
/// `content` describes the CSS inline content area used by backgrounds,
/// borders, and padding. It is selected independently of `line-height`.
/// `line` describes the line-height box that participates in baseline
/// alignment and may include fallback-font line extents for `normal`. Both are
/// already scaled into layout units before layout code consumes them.
///
/// CSS 2.2 §10.6.1 separates content-area decoration geometry from line-box
/// sizing: <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResolvedInlineTextMetrics {
    pub(crate) content: FontRunVerticalExtents,
    pub(crate) line: FontRunVerticalExtents,
}

impl ResolvedInlineTextMetrics {
    pub(crate) fn content_block_size(self) -> LayoutLength {
        self.content.block_size()
    }

    pub(crate) fn line_block_size(self) -> LayoutLength {
        self.line.block_size()
    }

    pub(crate) fn block_start_leading(self) -> LayoutLength {
        self.line.above_baseline - self.content.above_baseline
    }

    pub(crate) fn block_end_leading(self) -> LayoutLength {
        self.line.below_baseline - self.content.below_baseline
    }
}

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

#[derive(Debug, Clone)]
struct UnicodeRangeResolvedSpan {
    range: Range<usize>,
    style: ComputedStyle,
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
            text: Rc::from(&self.text[range]),
            width: 0.0,
            offset: self.offset,
            aligned_by_parley: self.aligned_by_parley,
            line_height: self.line_height,
            baseline_adjustment: self.baseline_adjustment,
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

    pub(crate) fn rendered_runs(&self) -> Vec<RenderedTextRun> {
        let runs = self
            .runs
            .iter()
            .filter(|run| run.paints && !run.glyphs.is_empty())
            .map(ShapedInlineRun::rendered_run)
            .collect();
        coalesce_adjacent_rendered_text_runs(runs)
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

/// Combine visual slices of one compatible shaped font run for emission.
///
/// Source slicing preserves the glyph forms chosen while shaping across an
/// inline boundary, but it can leave adjacent slices of the same visual font
/// run as separate PDF text objects.  Coalescing contiguous, otherwise
/// identical runs retains those glyphs and their advances while restoring the
/// CSS Text typographic-run boundary for painting and PDF emission:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>.
fn coalesce_adjacent_rendered_text_runs(runs: Vec<RenderedTextRun>) -> Vec<RenderedTextRun> {
    const EPSILON: f32 = 0.01;
    let mut output = Vec::with_capacity(runs.len());
    for run in runs {
        let Some(previous) = output.last_mut() else {
            output.push(run);
            continue;
        };
        let Some(previous_glyphs) = previous.glyphs.as_ref() else {
            output.push(run);
            continue;
        };
        let Some(run_glyphs) = run.glyphs.as_ref() else {
            output.push(run);
            continue;
        };
        let previous_end = previous.x_offset
            + previous_glyphs
                .iter()
                .map(|glyph| glyph.x_advance)
                .sum::<f32>();
        if previous.font_id != run.font_id
            || previous.font_size != run.font_size
            || previous.y_offset != run.y_offset
            || previous.text_matrix != run.text_matrix
            || (previous_end - run.x_offset).abs() > EPSILON
        {
            output.push(run);
            continue;
        }

        let mut text = String::with_capacity(previous.text.len() + run.text.len());
        text.push_str(&previous.text);
        text.push_str(&run.text);
        let mut glyphs = Vec::with_capacity(previous_glyphs.len() + run_glyphs.len());
        glyphs.extend(previous_glyphs.iter().cloned());
        glyphs.extend(run_glyphs.iter().cloned());
        let actual_text = if previous.actual_text.is_some() || run.actual_text.is_some() {
            let mut actual_text = String::new();
            actual_text.push_str(previous.actual_text.as_deref().unwrap_or(&previous.text));
            actual_text.push_str(run.actual_text.as_deref().unwrap_or(&run.text));
            (!actual_text.is_empty()).then(|| actual_text.into())
        } else {
            None
        };
        previous.text = Rc::from(text);
        previous.glyphs = Some(glyphs.into());
        previous.actual_text = actual_text;
    }
    output
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
    fn rendered_run(&self) -> RenderedTextRun {
        // A shaped glyph sequence is not necessarily a one-to-one spelling of
        // its source text.  In particular, HarfBuzz reports an `fi` ligature
        // at the source `f` cluster, so its glyph-level ToUnicode summary is
        // only `f`.  A PDF ToUnicode CMap is keyed by glyph/CID and cannot
        // restore the omitted source character when that same glyph is also
        // used by another run.  Attach the exact run-local replacement text
        // whenever the glyph summaries cannot reproduce the authored source.
        // ISO 32000-2:2020, 14.9.4.4, "ActualText".
        let glyph_text = self
            .glyphs
            .iter()
            .map(|glyph| glyph.rendered.unicode.as_str())
            .collect::<String>();
        let actual_text = (glyph_text != self.text.as_ref()
            || self
                .glyphs
                .iter()
                .any(|glyph| glyph.rendered.is_advance_only()))
        .then(|| Rc::clone(&self.text))
        .filter(|text| !text.is_empty());
        RenderedTextRun {
            text: Rc::clone(&self.text),
            actual_text,
            x_offset: self.x_offset,
            y_offset: 0.0,
            text_matrix: crate::document::paint::text::RenderedTextMatrix::IDENTITY,
            font_size: self.font_size,
            font_id: self.font_id,
            glyphs: Some(
                self.glyphs
                    .iter()
                    .map(|glyph| glyph.rendered.clone())
                    .collect(),
            ),
        }
    }
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

/// U+FFFC OBJECT REPLACEMENT CHARACTER for atomic inline line breaking.
///
/// CSS Text represents replaced elements and other atomic inline-level boxes
/// as U+FFFC for Unicode line-breaking decisions around the atomic box:
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
pub(crate) const OBJECT_REPLACEMENT_CHARACTER: char = '\u{fffc}';

/// One scalar after CSS Text's control-character classification.
///
/// CSS Text gives Unicode `Cc` scalars a meaning before white-space
/// processing and shaping. Keeping that classification typed makes it
/// impossible for a visible control to accidentally enter a whitespace-only
/// path merely because it is represented as a Rust `char`.
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CssTextScalar {
    DocumentSpace,
    Tab,
    SegmentBreak,
    CarriageReturn,
    VisibleControl(VisibleControlCharacter),
    Text(char),
}

/// A Unicode control character CSS Text requires the UA to render visibly.
///
/// Construction is private to [`classify_css_text_scalar`], which excludes
/// tab, line feed, and carriage return. Those characters retain their
/// separate CSS white-space semantics.
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisibleControlCharacter(char);

impl VisibleControlCharacter {
    /// The portable visible symbol Quire shapes for a control character.
    ///
    /// U+25A0 BLACK SQUARE is an `So` / Common character, matching CSS Text's
    /// required text-processing class without relying on a font-specific
    /// `.notdef` glyph for a C0 or C1 code point. The original scalar remains
    /// in the DOM/computed generated-content value; this is rendering text.
    const fn synthesized_glyph(self) -> char {
        '\u{25a0}'
    }
}

/// Classify one scalar according to CSS Text control and white-space rules.
pub(crate) fn classify_css_text_scalar(character: char) -> CssTextScalar {
    match character {
        ' ' => CssTextScalar::DocumentSpace,
        '\t' => CssTextScalar::Tab,
        '\n' => CssTextScalar::SegmentBreak,
        '\r' => CssTextScalar::CarriageReturn,
        character if character_is_unicode_control(character) => {
            CssTextScalar::VisibleControl(VisibleControlCharacter(character))
        }
        character => CssTextScalar::Text(character),
    }
}

/// Materialize text for CSS whitespace processing and shaping.
///
/// Carriage return becomes an ordinary space. Every other Cc character apart
/// from tab and line feed becomes a visible `So` / Common symbol. This is the
/// one rendering boundary shared by DOM and generated text.
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>
pub(crate) fn css_text_rendering_text(text: &str) -> String {
    text.chars()
        .map(|character| match classify_css_text_scalar(character) {
            CssTextScalar::CarriageReturn => ' ',
            CssTextScalar::VisibleControl(control) => control.synthesized_glyph(),
            CssTextScalar::DocumentSpace => ' ',
            CssTextScalar::Tab => '\t',
            CssTextScalar::SegmentBreak => '\n',
            CssTextScalar::Text(character) => character,
        })
        .collect()
}

/// Return whether a character is CSS document white space that can collapse.
///
/// CSS Text white-space processing operates only on spaces, tabs, and
/// segment breaks; a carriage return is first converted to a space. Other
/// Unicode space separators, including U+3000 IDEOGRAPHIC SPACE, and form
/// feed U+000C remain visible text and are handled through Unicode line
/// breaking:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(crate) fn is_css_collapsible_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r')
}

/// Return whether a character is a CSS document space preserved by `pre-wrap`.
///
/// CSS Text distinguishes space and tab document characters from segment
/// breaks during preserved white-space processing. Unicode space separators
/// such as U+3000 are not part of this CSS document-space set and are handled
/// through Unicode category and line-break data instead:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
pub(crate) fn is_css_preserved_document_space(character: char) -> bool {
    matches!(character, ' ' | '\t')
}

/// Return whether a text run contains only CSS-collapsible white space.
///
/// This intentionally differs from Rust/Unicode `trim` semantics: U+3000 and
/// other non-document space separators remain visible text for CSS Text line
/// layout:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(crate) fn text_is_css_collapsible_whitespace(text: &str) -> bool {
    text.chars().all(is_css_collapsible_whitespace)
}

/// Trim only CSS-collapsible document white space from both text edges.
///
/// CSS Text phase II removes line-edge collapsible spaces, not all Unicode
/// white-space code points:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(crate) fn trim_css_collapsible_whitespace(text: &str) -> &str {
    text.trim_matches(is_css_collapsible_whitespace)
}

/// Trim CSS-collapsible document white space from the start of text.
///
/// CSS Text trims document white-space characters at line edges; Unicode
/// space separators such as U+3000 remain visible content:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(crate) fn trim_start_css_collapsible_whitespace(text: &str) -> &str {
    text.trim_start_matches(is_css_collapsible_whitespace)
}

/// Trim CSS-collapsible document white space from the end of text.
///
/// CSS Text trims document white-space characters at line edges; Unicode
/// space separators such as U+3000 remain visible content:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(crate) fn trim_end_css_collapsible_whitespace(text: &str) -> &str {
    text.trim_end_matches(is_css_collapsible_whitespace)
}

/// Collapse CSS-collapsible document white space to single U+0020 spaces.
///
/// CSS Text collapses only document white-space characters. Other Unicode
/// separators such as U+3000 remain visible characters and must not be
/// collapsed by generic Unicode whitespace APIs:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
#[cfg(test)]
pub(crate) fn collapse_css_collapsible_whitespace(text: &str) -> String {
    let text = trim_css_collapsible_whitespace(text);
    let mut output = String::with_capacity(text.len());
    let mut in_whitespace = false;
    for character in text.chars() {
        if is_css_collapsible_whitespace(character) {
            if !in_whitespace {
                output.push(' ');
            }
            in_whitespace = true;
        } else {
            output.push(character);
            in_whitespace = false;
        }
    }
    output
}

/// Trim trailing CSS Text hanging space separators for line measurement.
///
/// CSS Text phase II says trailing "other space separators" hang for
/// every legacy white-space mode other than `break-spaces`: they remain in the
/// line's text and can paint, but their advance is excluded from line
/// measurement.
/// This helper returns the measured prefix without mutating the source text:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
#[cfg(test)]
pub(crate) fn trim_trailing_css_hanging_space_separators<'a>(
    text: &'a str,
    style: &ComputedStyle,
) -> &'a str {
    if !style.white_space.hangs_trailing_space_separators() {
        return text;
    }
    let end = text
        .char_indices()
        .rev()
        .find_map(|(offset, character)| {
            (!character_is_css_other_space_separator(character))
                .then_some(offset + character.len_utf8())
        })
        .unwrap_or(0);
    &text[..end]
}

/// Return the inline-end `letter-spacing` advance excluded from line measure.
///
/// CSS Text defines `letter-spacing` as tracking between typographic character
/// units and explicitly excludes tracking at the start or end of a line:
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
pub(crate) fn line_end_letter_spacing_width(text: &str, style: &ComputedStyle) -> LayoutLength {
    let letter_spacing = style.used_letter_spacing().points();
    if letter_spacing == 0.0
        || text.is_empty()
        || text.chars().any(|character| {
            character_has_joining_behavior(character) && !character_is_join_control(character)
        })
    {
        return layout_pt(0.0);
    }
    let boundaries = GraphemeClusterSegmenter::new()
        .segment_str(text)
        .collect::<Vec<_>>();
    if boundaries.windows(2).any(|window| {
        text[window[0]..window[1]]
            .chars()
            .any(|character| !character_is_bidi_format_control(character))
    }) {
        layout_pt(letter_spacing)
    } else {
        layout_pt(0.0)
    }
}

/// Used CSS Text Decoration font metrics in CSS px/pt layout units.
///
/// CSS Text Decoration lets `text-decoration-thickness: from-font` use font
/// underline metrics, and CSS Fonts supplies those metrics through OpenType
/// `post`/`OS/2` tables:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-width-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TextDecorationFontMetrics {
    pub(crate) underline_position: f32,
    pub(crate) underline_thickness: f32,
    pub(crate) strikeout_position: f32,
    pub(crate) strikeout_thickness: f32,
    pub(crate) descender_depth: f32,
}

/// A shaped glyph ink box relative to a rendered text line origin.
///
/// CSS Text Decoration's `text-decoration-skip-ink` intersects decoration
/// strokes with glyph ink; OpenType glyph bounding boxes provide the ink
/// geometry used by the PDF painter:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-skip-ink-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlyphInkBox {
    pub(crate) x_min: f32,
    pub(crate) x_max: f32,
    pub(crate) y_min: f32,
    pub(crate) y_max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FontKey {
    family_id: fontique::FamilyId,
    family_index: usize,
    face_index: u32,
    request: FontRequestAttributes,
    synthesize_weight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResolvedFontFaceKey {
    blob_id: u64,
    face_index: u32,
    registered_face: Option<RegisteredFontFaceKey>,
    family_label: Option<String>,
    request: Option<FontRequest>,
    synthesize_weight: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RegisteredFontFaceKey {
    family_id: u64,
    family_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FontRequestAttributes {
    weight: u16,
    style: u8,
    width: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FontRequest {
    family: FontRequestFamily,
    attributes: FontRequestAttributes,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FontRequestFamily {
    Generic(GenericFontRequest),
    Named(String),
    List(Vec<FontRequestFamily>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GenericFontRequest {
    SansSerif,
    Serif,
    Monospace,
    SystemUi,
    UiSerif,
    UiSansSerif,
    UiMonospace,
    UiRounded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FallbackRequest {
    character: char,
    attributes: FontRequestAttributes,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ParleyFontRequestKey {
    blob_id: u64,
    face_index: u32,
    request: FontRequest,
    synthesize_weight: bool,
}

#[derive(Clone)]
struct DocumentFontRegistry {
    fonts: Vec<DocumentFont>,
    registered_font_faces: HashMap<RegisteredFontFaceKey, RegisteredFontFaceMetadata>,
    document_font_faces: HashMap<usize, RegisteredFontFaceKey>,
    font_cache: HashMap<FontKey, usize>,
    font_blob_cache: HashMap<ResolvedFontFaceKey, usize>,
    parley_font_cache: HashMap<ParleyFontRequestKey, usize>,
    font_size_adjust: HashMap<usize, f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredFontFace {
    key: RegisteredFontFaceKey,
    metadata: RegisteredFontFaceMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredFontFaceMetadata {
    family: String,
    weight: FontWeight,
    style: FontStyle,
    weight_is_variable: bool,
    feature_defaults: FontFaceFeatureDefaults,
    unicode_range: Option<Vec<UnicodeRange>>,
    size_adjust: Option<u32>,
    ascent_override: Option<u32>,
    descent_override: Option<u32>,
    line_gap_override: Option<u32>,
    font_variation_settings: FontVariationSettings,
}

mod bidi;
mod breaking;
mod font_matching;
mod phrase;
mod shaping;
mod system;
mod typographic_units;
mod unicode_properties;

pub(crate) use bidi::{
    bidi_control_scope_for_style, resolve_bidi_visual_ranges, text_without_bidi_format_controls,
};
pub(crate) use breaking::contains_bidi_text;
pub(crate) use breaking::{
    DiscretionaryOpportunity, LanguageDiscretionaryReplacement,
    automatic_hyphenation_opportunities, hyphenator_for_language, manual_hyphenation_opportunities,
    text_with_hyphenation_controls,
};
pub(crate) use breaking::{
    TextBreakPolicy, collect_auto_phrase_relaxed_wrap_opportunities,
    collect_grapheme_cluster_inner_boundaries, collect_keep_all_relaxed_wrap_opportunities,
    collect_measured_break_opportunities,
};
#[cfg(test)]
pub(crate) use breaking::{
    inline_atomic_boundary_allows_soft_wrap, manual_suppresses_break_between,
    measured_break_opportunities, text_with_auto_hyphenation,
};
use font_matching::*;
pub(crate) use phrase::{AutoPhraseLanguage, phrase_boundaries};
use shaping::*;
pub(crate) use shaping::{BidiVisualRange, ResolvedBidiDirection};
/// Read one OpenType name record for PDF resource metadata.
pub(crate) fn font_program_opentype_name(
    face: &ttf_parser::Face<'_>,
    name_id: u16,
) -> Option<String> {
    font_matching::opentype_name(face, name_id)
}
pub(crate) use typographic_units::{
    character_is_inter_character_control, inter_character_gap_allowed_between_text,
    keep_all_suppresses_break_between, text_allows_inter_character_gap_after,
    text_allows_inter_character_gap_before, text_is_inter_character_control_only,
    typographic_unit_ranges,
};
use unicode_properties::*;
pub(crate) use unicode_properties::{
    SegmentBreakContext, TextSpacingPunctuationClass, Uax14BoundaryProtection,
    bidi_mirroring_glyph, character_has_joining_behavior, character_is_arabic_tatweel,
    character_is_autospace_alpha, character_is_autospace_ideograph, character_is_autospace_numeric,
    character_is_bidi_format_control, character_is_css_other_space_separator,
    character_is_css_word_separator, character_is_currency_symbol,
    character_is_default_ignorable_code_point, character_is_first_hangable_punctuation,
    character_is_first_letter_associated_space, character_is_first_letter_suffix_punctuation,
    character_is_font_neutral_default_ignorable, character_is_hangable_stop_or_comma,
    character_is_join_control, character_is_last_hangable_punctuation,
    character_is_mandatory_line_break, character_is_ruby_justification_eligible,
    character_is_text_decoration_spacer, character_is_unicode_alphanumeric,
    character_is_unicode_control, character_is_unicode_first_letter_base,
    character_is_unicode_letter, character_is_unicode_mark, character_is_unicode_punctuation,
    character_is_unicode_symbol, character_is_unicode_typographic_letter,
    character_preserves_word_boundary_context, character_receives_text_emphasis_mark,
    plaintext_direction_for_text, segment_break_is_removable, text_spacing_punctuation_class,
    typographic_unit_is_upright_in_mixed_orientation,
    typographic_unit_uses_vertical_forms_in_mixed_orientation, uax14_atomic_boundary_protection,
};

#[cfg(test)]
mod tests;
