use std::collections::HashMap;

use taffy::prelude as taffy_layout;

use self::assets::{
    DocumentCanvasBackgroundArea, PaintBackgroundArea,
    background_image_primitives_for_style_with_paint_areas,
    background_image_primitives_for_style_with_paint_areas_and_fixed_positioning_area,
    fragmented_table_root_background_image_primitives,
    structural_table_background_image_primitives,
};
use crate::css::{
    self, AlignContent, AlignItems, AlignSelf, AlignmentBaseline, AlignmentSafety, BackgroundImage,
    BaselineMetric, BaselineShift, BookmarkLabelPart, BorderStyle, BoxSizing, CaptionSide, Clear,
    ClipPath, ComputedStyle, ContainerType, Content, ContentAlignmentKeyword, ContentVisibility,
    CounterReset, CounterResetKind, CounterStyleRange, CounterStyleRule, CounterStyleSystem,
    CounterValue, CssBookmarkState, CssColor, Declarations, Direction, Display, DisplayInner,
    DisplayOuter, DominantBaseline, ElementAttributeSignature, ElementSiblingSignature,
    ElementSiblingSignatureList, ElementSignature, FilterValue, FlexDirection, FlexWrap, Float,
    GeneratedAltTextPart, GeneratedContentPart, GeneratedQuote, Isolation, JustifyContent,
    JustifyItems, JustifySelf, LinearGradientDirection, ListStylePosition, ListStyleType,
    LogicalAxis, LogicalSide, MarkerContent, MarkerContentPart, MarkerSide, MaskValue,
    MixBlendMode, NamedStringPart, PageBreak, PageRule, PageSpecificity, PhysicalAxis,
    PhysicalSide, Position, Quotes, SelfAlignmentKeyword, StylesheetOrigin, Stylesheets,
    TableCellVerticalAlignKeyword, TableLayout, TargetReference, TextAlign, TextAlignLast,
    TextAutospace, TextDecorationSkipInk, TextDecorationSkipSpaces, TextDecorationStyle,
    TextDecorationThickness, TextJustify, TextOrientation, TextTransformCase, TextUnderlineOffset,
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
    PaintClip, PaintDisplacement, PaintPoint, PaintRect, PaintSize, PaintSpace, PaintTransform,
    PaintTranslation,
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
    OpaqueTextGlyphCoverage, RenderedGlyph, RenderedLine, RenderedLineSource, RenderedTextMatrix,
    RenderedTextRun, TextRunPoint, split_rendered_line_for_opaque_text_coverage,
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
    TextDecorationFontMetrics, bidi_control_scope_for_style,
    character_has_cursive_shaping_behavior, character_is_arabic_tatweel,
    character_is_bidi_format_control, character_is_default_ignorable_code_point,
    character_is_first_hangable_punctuation, character_is_first_letter_associated_space,
    character_is_first_letter_suffix_punctuation, character_is_hangable_stop_or_comma,
    character_is_join_control, character_is_last_hangable_punctuation,
    character_is_unicode_alphanumeric, character_is_unicode_first_letter_base,
    character_is_unicode_mark, character_is_unicode_punctuation, character_is_unicode_symbol,
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

pub(crate) mod asset_helpers;
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
#[allow(unused_imports)]
pub(in crate::layout) use self::block::flow::{
    AdjoiningBlockStartMargin, InheritedAdjoiningStartMargin, can_collapse_block_end_margin,
    can_collapse_block_start_margin, can_collapse_own_block_margins, collapse_margin_set,
    collapse_margins, collapsed_margin_delta, collapsed_start_margin_delta,
    collapsible_first_child_start_margin_dom_with_font_metrics,
    collapsible_first_child_start_margin_dom_with_resolver,
    collapsible_first_child_start_margin_from_boxes, collapsible_start_margin_dom_with_resolver,
    collapsible_start_margin_for_box, dom_children_keep_self_collapsing_parent,
    formatting_box_can_only_create_phantom_line_boxes, formatting_box_keeps_self_collapsing_parent,
    generated_content_has_non_phantom_inline_content, has_atomic_inline_formatting_box,
    has_direct_inline_content_box, has_direct_inline_content_dom_with_resolver,
    has_non_inline_formatting_box, height_behaves_as_auto_for_margin_collapse,
    height_is_auto_or_zero, is_self_collapsing_block_box,
    is_self_collapsing_block_dom_with_font_metrics, is_self_collapsing_block_dom_with_resolver,
    page_start_margin, self_collapsing_block_margin_set_for_box, trim_adjoining_block_start_margin,
};
pub(in crate::layout) use self::block::{
    AutoFloatMeasurementKey, BlockClearance, BlockClearanceRequest, BlockMarginCollapseBoundary,
    BlockStartMarginArrangement, FloatAvoidanceCandidate, FloatAvoidanceInlineContainment,
    FloatBand, FloatBandQuery, FloatContext, FloatId, FloatPlacementAxes, FloatRunState,
    FloatShape, UsedFloatSide, float_avoiding_auto_border_box_width, vertical_physical_inline_span,
};
#[cfg(test)]
pub(in crate::layout) use self::block::{
    ClearedFloatOuterBlockEnd, FloatBandPlacement, FloatClearanceTarget, FloatPlacement,
    HypotheticalClearBorderEdge, LogicalFloatBand,
};
mod box_tree;
mod builder;
pub(in crate::layout) use self::builder::*;
mod counter_styles;
mod counters;
mod definition_lists;
pub(in crate::layout) use self::definition_lists::*;
mod dom_style;
pub(in crate::layout) use self::dom_style::*;
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
mod page_names;
pub(in crate::layout) use self::page_names::*;
mod page_margin;
mod paint_helpers;
pub(crate) use paint_helpers::block::shaped_rect_path_commands;
mod paint_ops;
mod positioned_child;
mod positioning_context;
mod relative_positioning;
pub(in crate::layout) use self::relative_positioning::*;
mod ruby;
mod scroll_snap;
mod table;
mod table_span;
mod taffy_bridge;
mod text_helpers;
pub(crate) mod text_paint;

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
use counters::*;
use element_semantics::*;
#[allow(unused_imports)]
use flex::flex_gap_decoration_primitives_with_gutters;
use flow_helpers::*;
use fragmentation::*;
use gap_decorations::*;
use geometry::*;
#[allow(unused_imports)]
use grid::{grid_gap_decoration_gutters_from_topologies, grid_gap_decoration_primitives};
use html_direction::*;
use inline_boundary::*;
use inline_collect::{block_bidi_scope_needs_inline_controls, push_inline_words_for_style};
use inline_helpers::*;
use item_content::*;
use item_intrinsic::*;
use itemization::*;
use paint_helpers::*;
use positioned_child::*;
use positioning_context::*;
use table_span::*;
use text_helpers::*;
use used_values::*;

mod render;
pub use self::render::RenderOptions;
pub(in crate::layout) use self::render::*;
pub(crate) use self::render::{
    IframeEmbeddingContext, PageMargins, PageSize, PreparedDomLayout, layout_prepared_dom,
    start_font_system_load,
};
mod layout_models;
pub(in crate::layout) use self::layout_models::*;
mod inline_models;
pub(in crate::layout) use self::inline_models::*;
#[cfg(feature = "layout-profile")]
mod layout_profile;
#[cfg(test)]
mod layout_tests;
#[cfg(all(feature = "stack-profile", target_os = "macos"))]
mod stack_profile;
