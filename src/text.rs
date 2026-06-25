use crate::css::{
    ComputedStyle, CssFontFace, Direction, FontFaceSource, FontFamily, FontFeatureValue,
    FontFeatureValues, FontFeatureValuesBlock, FontKerning, FontStyle, FontVariantAlternates,
    FontVariantCaps, FontVariantEastAsian, FontVariantEastAsianValue, FontVariantEmoji,
    FontVariantLigatures, FontVariantNumeric, FontVariantNumericValue, FontVariantPosition,
    FontWeight, FontWidth, HyphenateLimitChars, Hyphens, LineBreak as CssLineBreak,
    OverflowWrap as CssOverflowWrap, Stylesheet, UnicodeBidi, WordBreak as CssWordBreak,
    WritingMode, known_font_family,
};
use crate::document::{DocumentFont, FontProgramKind, RenderedGlyph, RenderedTextRun};
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
    props::{
        BidiClass, BidiControl, DefaultIgnorableCodePoint, GeneralCategory, GeneralCategoryGroup,
        JoinControl, JoiningType, LineBreak, WordBreak as IcuWordBreak,
    },
};
use icu_segmenter::options::{
    LineBreakOptions, LineBreakStrictness, LineBreakWordOption, WordBreakInvariantOptions,
};
use icu_segmenter::{GraphemeClusterSegmenter, LineSegmenter, WordSegmenter};
use parley::setting::Tag as ParleyTag;
use parley::style::FontFeature as ParleyFontFeature;
use parley::{
    BreakReason, FontContext as ParleyFontContext, FontFamily as ParleyFontFamily,
    FontFeatures as ParleyFontFeatures, FontStyle as ParleyFontStyle,
    FontWeight as ParleyFontWeight, FontWidth as ParleyFontWidth, Language as ParleyLanguage,
    LayoutContext as ParleyLayoutContext, LineHeight as ParleyLineHeight,
    OverflowWrap as ParleyOverflowWrap, StyleProperty, TextWrapMode as ParleyTextWrapMode,
    WordBreak as ParleyWordBreak,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone)]
pub(crate) struct FontSystem {
    parley_font_context: ParleyFontContext,
    parley_layout_context: ParleyLayoutContext,
    document_fonts: DocumentFontRegistry,
    family_cache: HashMap<FontRequest, usize>,
    fallback_cache: HashMap<FallbackRequest, Option<usize>>,
    font_feature_values: FontFeatureValues,
    font_feature_defaults_by_family: HashMap<String, FontFaceFeatureDefaults>,
    visible_fallback_families: Vec<String>,
}

pub(crate) struct FontSystemLoad {
    parley_font_context: tokio::task::JoinHandle<LoadedParleyFontContext>,
}

pub(crate) struct FontSystemSeedLoad {
    parley_font_context: tokio::task::JoinHandle<LoadedParleyFontContext>,
    font_faces: tokio::task::JoinHandle<Vec<LoadedFontFace>>,
    font_feature_values: FontFeatureValues,
}

struct FontSystemSeed {
    parley_font_context: ParleyFontContext,
    registered_font_faces: HashMap<FontBlobFaceKey, RegisteredFontFaceMetadata>,
    font_feature_values: FontFeatureValues,
    font_feature_defaults_by_family: HashMap<String, FontFaceFeatureDefaults>,
    visible_fallback_families: Vec<String>,
}

struct LoadedParleyFontContext {
    parley_font_context: ParleyFontContext,
    visible_fallback_families: Vec<String>,
}

struct LoadedFontFace {
    font_face: CssFontFace,
    data: Option<Vec<u8>>,
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

    fn is_normal(&self) -> bool {
        self.font_feature_settings.0.is_empty()
            && self.font_variant_ligatures == FontVariantLigatures::Normal
            && self.font_variant_position == FontVariantPosition::Normal
            && self.font_variant_caps == FontVariantCaps::Normal
            && self.font_variant_numeric == FontVariantNumeric::Normal
            && self.font_variant_alternates == FontVariantAlternates::Normal
            && self.font_variant_east_asian == FontVariantEastAsian::Normal
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StyledTextSpan<'a> {
    pub(crate) text: &'a str,
    pub(crate) style: &'a ComputedStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextLine {
    pub(crate) text: String,
    pub(crate) width: f32,
    pub(crate) offset: f32,
    pub(crate) aligned_by_parley: bool,
    pub(crate) line_height: f32,
    pub(crate) shaped: Option<ShapedInlineLine>,
    pub(crate) starts_after_forced_break: bool,
}

impl TextLine {
    /// Create a CSS text line whose shaped payload can be filled by `FontSystem`.
    ///
    /// CSS Text line breaking first determines the formatted line text and
    /// used measure; CSS Fonts then shapes that exact line with the selected
    /// font faces before painting or PDF glyph emission:
    /// <https://www.w3.org/TR/css-text-3/#line-breaking> and
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>.
    pub(crate) fn new(text: String, width: f32, line_height: f32) -> Self {
        Self {
            text,
            width,
            offset: 0.0,
            aligned_by_parley: false,
            line_height,
            shaped: None,
            starts_after_forced_break: false,
        }
    }

    pub(crate) fn with_shaped(mut self, shaped: Option<ShapedInlineLine>) -> Self {
        self.shaped = shaped;
        self
    }

    pub(crate) fn starting_after_forced_break(mut self) -> Self {
        self.starts_after_forced_break = true;
        self
    }
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
    pub(crate) text: String,
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

    pub(crate) fn rendered_runs(&self) -> Vec<RenderedTextRun> {
        self.runs
            .iter()
            .filter(|run| run.paints && !run.glyphs.is_empty())
            .map(ShapedInlineRun::rendered_run)
            .collect()
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
                        separator_count: glyph.source_text.chars().count().max(1),
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

#[derive(Debug, Clone, Copy)]
struct ShapedJustificationOpportunity {
    run_index: usize,
    glyph_index: usize,
    visual_end: f32,
    separator_count: usize,
}

fn shaped_glyph_is_inter_word_separator(glyph: &ShapedInlineGlyph) -> bool {
    !glyph.source_text.is_empty()
        && glyph
            .source_text
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
    pub(crate) text: String,
    pub(crate) x_offset: f32,
    pub(crate) font_size: f32,
    pub(crate) font_id: Option<usize>,
    pub(crate) glyphs: Vec<ShapedInlineGlyph>,
    pub(crate) paints: bool,
}

impl ShapedInlineRun {
    fn rendered_run(&self) -> RenderedTextRun {
        RenderedTextRun {
            text: self.text.clone(),
            x_offset: self.x_offset,
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
    pub(crate) source_text: String,
    pub(crate) paints: bool,
}

/// U+FFFC OBJECT REPLACEMENT CHARACTER for atomic inline line breaking.
///
/// CSS Text represents replaced elements and other atomic inline-level boxes
/// as U+FFFC for Unicode line-breaking decisions around the atomic box:
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
pub(crate) const OBJECT_REPLACEMENT_CHARACTER: char = '\u{fffc}';

/// Return whether a character is CSS document white space that can collapse.
///
/// CSS Text white-space processing operates on document white-space
/// characters: spaces, tabs, segment breaks, and form feeds. Other Unicode
/// space separators, including U+3000 IDEOGRAPHIC SPACE, remain normal
/// visible characters and are handled by Unicode line breaking:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(crate) fn is_css_collapsible_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{000C}')
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
/// `white-space: normal`, `nowrap`, and `pre-line`: they remain in the line's
/// text and can paint, but their advance is excluded from line measurement.
/// This helper returns the measured prefix without mutating the source text:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
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
pub(crate) fn line_end_letter_spacing_width(text: &str, style: &ComputedStyle) -> f32 {
    let letter_spacing = style.used_letter_spacing();
    if letter_spacing == 0.0 || text.is_empty() || text.chars().any(character_has_joining_behavior)
    {
        return 0.0;
    }
    let boundaries = GraphemeClusterSegmenter::new()
        .segment_str(text)
        .collect::<Vec<_>>();
    if boundaries.windows(2).any(|window| {
        text[window[0]..window[1]]
            .chars()
            .any(|character| !character_is_bidi_format_control(character))
    }) {
        letter_spacing
    } else {
        0.0
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
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResolvedFontFaceKey {
    blob_id: u64,
    face_index: u32,
    family_label: Option<String>,
    request: Option<FontRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FontBlobFaceKey {
    blob_id: u64,
    face_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FontRequestAttributes {
    weight: u16,
    style: u8,
    width: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FontRequest {
    family_list: Vec<FontFamilyRequest>,
    attributes: FontRequestAttributes,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FontFamilyRequest {
    Named(String),
    Generic(GenericFontRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GenericFontRequest {
    SansSerif,
    Serif,
    Monospace,
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
}

#[derive(Clone)]
struct DocumentFontRegistry {
    fonts: Vec<DocumentFont>,
    registered_font_faces: HashMap<FontBlobFaceKey, RegisteredFontFaceMetadata>,
    font_cache: HashMap<FontKey, usize>,
    font_blob_cache: HashMap<ResolvedFontFaceKey, usize>,
    parley_font_cache: HashMap<ParleyFontRequestKey, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredFontFace {
    key: FontBlobFaceKey,
    metadata: RegisteredFontFaceMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredFontFaceMetadata {
    family: String,
    feature_defaults: FontFaceFeatureDefaults,
}

mod bidi;
mod breaking;
mod font_matching;
mod shaping;
mod system;
mod unicode_properties;

pub(crate) use bidi::{
    bidi_control_scope_for_style, text_with_css_bidi_controls, text_without_bidi_format_controls,
};
pub(crate) use breaking::contains_bidi_text;
pub(crate) use breaking::inline_atomic_boundary_allows_soft_wrap;
pub(crate) use breaking::measured_break_opportunities;
use breaking::*;
use font_matching::*;
use shaping::*;
use unicode_properties::*;
pub(crate) use unicode_properties::{
    character_is_autospace_alpha, character_is_autospace_ideograph, character_is_autospace_numeric,
    character_is_bidi_format_control, character_is_css_other_space_separator,
    character_is_css_word_separator, character_is_default_ignorable_code_point,
    character_is_first_hangable_punctuation, character_is_font_neutral_default_ignorable,
    character_is_hangable_stop_or_comma, character_is_join_control,
    character_is_last_hangable_punctuation, character_is_text_decoration_spacer,
    character_is_unicode_alphanumeric, character_is_unicode_control, character_is_unicode_letter,
    character_is_unicode_punctuation, character_preserves_word_boundary_context,
    character_receives_text_emphasis_mark, plaintext_direction_for_text,
};

#[cfg(test)]
mod tests;
