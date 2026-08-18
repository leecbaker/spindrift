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
use crate::units::{LayoutLength, SemanticLengthExt, layout_pt};
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
}

/// Resolved vertical metrics for one inline text box.
///
/// `content` describes the CSS inline content area used by backgrounds,
/// borders, and padding. It is selected independently of `line-height`.
/// `line` describes the line-height box that participates in baseline
/// alignment. Both are already scaled into layout units before layout code
/// consumes them.
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

#[derive(Debug, Clone)]
struct UnicodeRangeResolvedSpan {
    range: Range<usize>,
    selected_family: Option<FontFamily>,
}

pub(crate) use artifacts::{ShapedGlyphRun, ShapedInlineGlyph, ShapedInlineLine, ShapedInlineRun};

/// U+FFFC OBJECT REPLACEMENT CHARACTER for atomic inline line breaking.
///
/// CSS Text represents replaced elements and other atomic inline-level boxes
/// as U+FFFC for Unicode line-breaking decisions around the atomic box:
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
pub(crate) const OBJECT_REPLACEMENT_CHARACTER: char = '\u{fffc}';

#[cfg(test)]
pub(crate) use css_text::VisibleControlCharacter;
pub(crate) use css_text::{
    CssTextScalar, classify_css_text_scalar, css_text_rendering_text,
    is_css_collapsible_whitespace, is_css_preserved_document_space, line_end_letter_spacing_width,
    text_is_css_collapsible_whitespace, trim_css_collapsible_whitespace,
    trim_end_css_collapsible_whitespace, trim_start_css_collapsible_whitespace,
};
#[cfg(test)]
pub(crate) use css_text::{
    collapse_css_collapsible_whitespace, trim_trailing_css_hanging_space_separators,
};

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

mod artifacts;
mod bidi;
mod breaking;
mod css_text;
mod font_matching;
mod phrase;
mod shaping;
mod system;
mod typographic_units;
mod unicode_properties;
mod vertical_typesetting;

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
pub(crate) use shaping::InlineBoundaryEffect;
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
    CursiveProtectedUnitRanges, LineBreakAnywhereUnitRanges, character_is_inter_character_control,
    inter_character_gap_allowed_between_text, keep_all_suppresses_break_between,
    text_allows_inter_character_gap_after, text_allows_inter_character_gap_before,
    text_is_inter_character_control_only,
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
    character_is_mandatory_line_break, character_is_native_vertical_script,
    character_is_ruby_justification_eligible, character_is_text_decoration_spacer,
    character_is_unicode_alphanumeric, character_is_unicode_control,
    character_is_unicode_first_letter_base, character_is_unicode_letter, character_is_unicode_mark,
    character_is_unicode_punctuation, character_is_unicode_symbol,
    character_is_unicode_typographic_letter, character_preserves_word_boundary_context,
    character_receives_text_emphasis_mark, plaintext_direction_for_text,
    segment_break_is_removable, text_spacing_punctuation_class,
    typographic_unit_is_upright_in_mixed_orientation, uax14_atomic_boundary_protection,
};
pub(crate) use vertical_typesetting::{TextTypesettingPlan, VerticalUnitTypesetting};

#[cfg(test)]
mod tests;
