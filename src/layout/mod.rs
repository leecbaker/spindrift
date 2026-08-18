use crate::css::{
    self, AlignContent, AlignItems, AlignSelf, AlignmentBaseline, AlignmentSafety, BackgroundImage,
    BaselineMetric, BaselineShift, BookmarkLabelPart, BorderStyle, BoxSizing, CaptionSide, Clear,
    ClipPath, ComputedStyle, ContainerType, Content, ContentAlignmentKeyword, ContentVisibility,
    CounterReset, CounterResetKind, CounterStyleRange, CounterStyleRule, CounterStyleSystem,
    CounterValue, CssBookmarkState, CssColor, Declarations, Direction, Display, DisplayInner,
    DisplayOuter, DominantBaseline, ElementAttributeSignature, ElementSiblingSignature,
    ElementSiblingSignatureList, ElementSignature, EmptyCells, FilterValue, FlexDirection,
    FlexWrap, Float, GeneratedAltTextPart, GeneratedContentPart, GeneratedQuote, Isolation,
    JustifyContent, JustifyItems, JustifySelf, LinearGradientDirection, ListStylePosition,
    ListStyleType, LogicalAxis, LogicalSide, MarkerContent, MarkerContentPart, MarkerSide,
    MaskValue, MixBlendMode, NamedStringPart, PageBreak, PageRule, PageSpecificity, PhysicalAxis,
    PhysicalSide, Position, Quotes, SelfAlignmentKeyword, StylesheetOrigin, Stylesheets,
    TableCellVerticalAlign, TableLayout, TargetReference, TextAlign, TextAlignLast, TextAutospace,
    TextDecorationSkipInk, TextDecorationSkipSpaces, TextDecorationStyle, TextDecorationThickness,
    TextJustify, TextOrientation, TextSpacingTrim, TextTransformCase, TextUnderlineOffset,
    TextUnderlinePosition, UnicodeBidi, VerticalAlign, Visibility, WhiteSpace, WritingMode,
    WritingModeAxes, block_end_side, block_start_side, inline_end_side, inline_start_side,
};
use crate::document::paint::annotations::RenderedLink;
use crate::document::paint::contours::{BoxContentContour, ResolvedBoxContentClip};
use crate::document::paint::display_list::PaintBand;
use crate::document::paint::effects::{
    PaintBlendMode, PaintClipPathEffect, PaintEffects, PaintFilterEffect, PaintMaskEffect,
    RenderedClipPathPolygon,
};
use crate::document::paint::fragments::PaintFragment;
use crate::document::paint::geometry::{
    PaintClip, PaintClipUnion, PaintDisplacement, PaintPoint, PaintRect, PaintSize, PaintSpace,
    PaintTransform, PaintTranslation,
};
use crate::document::paint::images::RenderedImage;
use crate::document::paint::page::{PaintCheckpoint, PaintPrimitive};
use crate::document::paint::paths::{
    RenderedGradient, RenderedGradientKind, RenderedGradientStop, RenderedPath, RenderedPathClip,
    RenderedPathClipPath, RenderedPathCommand, RenderedPathFillRule, RenderedPathLineCap,
    RenderedPathStrokeStyle, paint_rect_path_commands,
};
#[cfg(test)]
use crate::document::paint::patterns::RenderedImageSourceRect;
use crate::document::paint::patterns::{
    RenderedGradientPattern, RenderedImagePattern, RenderedSvgPattern,
};
use crate::document::paint::shapes::{
    RenderedCornerRadius, RenderedRect, RenderedRoundedRect, RenderedRoundedRectRadii,
    RenderedStroke,
};
use crate::document::paint::stacking::{PaintStackingContext, StackLevel};
use crate::document::paint::text::{
    RenderedGlyph, RenderedLine, RenderedLineSource, RenderedTextMatrix, RenderedTextRun,
    TextRunPoint,
};
use crate::document::{
    Bookmark, BookmarkState, Document, DocumentMetadata, Page, PaintStrokeWidth,
};
use crate::dom::{self, Element, ElementId, Node, NodeKind};
use crate::resource::ResourceCache;
use crate::svg::SharedSvgAsset;
use crate::text::{
    BidiVisualRange, FontSystem, FontSystemLoad, GlyphInkBox, InlineBoundaryEffect,
    OBJECT_REPLACEMENT_CHARACTER, ResolvedBidiDirection, ShapedInlineLine, StyledTextSpan,
    TextDecorationFontMetrics, bidi_control_scope_for_style, character_has_joining_behavior,
    character_is_arabic_tatweel, character_is_bidi_format_control,
    character_is_default_ignorable_code_point, character_is_first_hangable_punctuation,
    character_is_first_letter_associated_space, character_is_first_letter_suffix_punctuation,
    character_is_hangable_stop_or_comma, character_is_join_control,
    character_is_last_hangable_punctuation, character_is_unicode_alphanumeric,
    character_is_unicode_first_letter_base, character_is_unicode_mark,
    character_is_unicode_punctuation, character_is_unicode_symbol,
    character_preserves_word_boundary_context, character_receives_text_emphasis_mark,
    contains_bidi_text, css_text_rendering_text, is_css_collapsible_whitespace,
    plaintext_direction_for_text, text_with_hyphenation_controls,
    text_without_bidi_format_controls,
};
use crate::timing::DebugTimer;
use crate::units::{
    AtomicInlineBaselineSourceOffset, AtomicInlineMarginBoxBaselineOffset,
    AtomicInlinePaintPlacementBaselineOffset, BorderBoxLength, BorderBoxSize, ContentBoxLength,
    ContentBoxSize, Definite, LayoutLength, MarginBoxLength, MarginBoxSize, NonContentLength,
    PercentageBasis, RasterPixelSize, SemanticLengthExt, atomic_inline_baseline_source_pt,
    atomic_inline_margin_box_baseline_pt, atomic_inline_paint_placement_baseline_pt, border_box_pt,
    border_box_to_content_box_length, content_box_pt, content_box_size_pt,
    content_box_to_border_box_length, content_box_to_border_box_size, layout_points, layout_pt,
    margin_box_pt, margin_box_size_pt, non_content_pt,
};
use std::collections::HashMap;
use taffy::prelude as taffy_layout;

use self::assets::{
    DocumentCanvasBackgroundArea, PaintBackgroundArea,
    background_image_primitives_for_style_with_paint_areas,
    background_image_primitives_for_style_with_paint_areas_and_fixed_positioning_area,
    fragmented_table_root_background_image_primitives,
    structural_table_background_image_primitives,
};

mod asset_helpers;
mod assets;
mod baseline;
pub(crate) fn generated_linear_gradient_raster_color_space(
    gradient: &crate::css::LinearGradient,
    size: crate::document::paint::geometry::PaintSize,
    current_color: crate::CssColor,
) -> Option<crate::css::CssColorSpace> {
    assets::background_gradients::generated_linear_gradient_raster_color_space(
        gradient,
        size,
        current_color,
    )
}

pub(crate) fn generated_radial_gradient_raster_color_space(
    gradient: &crate::css::RadialGradient,
    size: crate::document::paint::geometry::PaintSize,
    current_color: crate::CssColor,
) -> Option<crate::css::CssColorSpace> {
    assets::background_gradients::generated_radial_gradient_raster_color_space(
        gradient,
        size,
        current_color,
    )
}
mod block;
pub(in crate::layout) use self::block::{
    AutoFloatMeasurementKey, BlockClearance, BlockClearanceRequest, BlockMarginCollapseBoundary,
    BlockStartMarginArrangement, FloatAvoidanceCandidate, FloatAvoidanceInlineContainment,
    FloatBand, FloatBandQuery, FloatContext, FloatId, FloatRunState, FloatShape, UsedFloatSide,
    float_avoiding_auto_border_box_width, vertical_physical_inline_span,
};
#[cfg(test)]
pub(in crate::layout) use self::block::{
    ClearedFloatOuterBlockEnd, FloatBandPlacement, FloatClearanceTarget, FloatPlacement,
    HypotheticalClearBorderEdge, LogicalFloatBand,
};
mod box_tree;
mod builder;
mod counters;
mod element_semantics;
mod flex;
mod flow_helpers;
mod footnotes;
mod fragmentation;
mod gap_decorations;
mod geometry;
mod grid;
mod html_direction;
mod inline_boundary;
mod inline_collect;
mod inline_helpers;
mod inline_layout;
mod inline_row;
mod intrinsic;
mod item_content;
mod item_intrinsic;
mod itemization;
mod list;
mod page_generated;
mod page_margin;
mod paint_helpers;
pub(crate) use paint_helpers::block::shaped_rect_path_commands;
mod paint_ops;
mod positioned_child;
mod ruby;
mod scroll_snap;
mod table;
mod table_span;
mod taffy_bridge;
mod text_helpers;
mod text_paint;

/// Rasterize one generated CSS image only when the PDF writer needs it.
pub(crate) fn rasterize_generated_image(
    image: &crate::image_store::GeneratedRasterImage,
) -> Option<crate::image_store::RasterImage> {
    let decoded = match image {
        crate::image_store::GeneratedRasterImage::Linear { gradient, size, .. } => {
            // Generated recipes are resolved at their element paint boundary,
            // so a symbolic currentcolor cannot remain here.
            assets::rasterize_linear_gradient(gradient, *size, crate::css::CssColor::TRANSPARENT)
        }
        crate::image_store::GeneratedRasterImage::Radial { gradient, size, .. } => {
            assets::rasterize_radial_gradient(gradient, *size, crate::css::CssColor::TRANSPARENT)
        }
    }?;
    Some(crate::image_store::RasterImage {
        metadata: crate::image_store::ImageMetadata::from_pixel_size(decoded.pixel_size),
        color_space: decoded.color_space,
        sample_depth: crate::image_store::RasterSampleDepth::Eight,
        rgb: decoded.rgb.to_vec(),
        alpha: decoded.alpha.as_deref().map(ToOwned::to_owned),
    })
}
mod used_values;

use asset_helpers::*;
use element_semantics::*;
use flow_helpers::*;
use fragmentation::*;
use gap_decorations::*;
use geometry::*;
use html_direction::*;
use inline_boundary::*;
use inline_collect::{block_bidi_scope_needs_inline_controls, push_inline_words_for_style};
use inline_helpers::*;
use item_content::*;
use item_intrinsic::*;
use itemization::*;
use paint_helpers::*;
use positioned_child::*;
use table_span::*;
use text_helpers::*;
use used_values::*;

mod split_1;
pub use self::split_1::RenderOptions;
pub(in crate::layout) use self::split_1::*;
pub(crate) use self::split_1::{IframeEmbeddingContext, PageMargins, PageSize};
pub(crate) use self::split_1::{PreparedDomLayout, layout_prepared_dom, start_font_system_load};
mod split_2;
pub(in crate::layout) use self::split_2::*;
mod split_3;
pub(in crate::layout) use self::split_3::*;
mod split_4;
