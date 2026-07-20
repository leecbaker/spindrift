use crate::css::{
    self, AlignContent, AlignItems, AlignSelf, AlignmentBaseline, AlignmentSafety, BackgroundImage,
    BaselineMetric, BaselineShift, BookmarkLabelPart, BorderStyle, BoxSizing, CaptionSide, Clear,
    ClipPath, ComputedStyle, Content, ContentAlignmentKeyword, ContentVisibility, CounterReset,
    CounterResetKind, CounterStyleRange, CounterStyleRule, CounterStyleSystem, CounterValue,
    CssBookmarkState, CssColor, Declarations, Direction, Display, DisplayInner, DominantBaseline,
    ElementAttributeSignature, ElementSiblingSignature, ElementSiblingSignatureList,
    ElementSignature, EmptyCells, FilterValue, FlexDirection, FlexWrap, Float,
    GeneratedAltTextPart, GeneratedContentPart, GeneratedQuote, Isolation, JustifyContent,
    JustifyItems, JustifySelf, LinearGradientDirection, ListStylePosition, ListStyleType,
    LogicalAxis, LogicalSide, MarkerContent, MarkerContentPart, MarkerSide, MaskValue,
    MixBlendMode, NamedStringPart, PageBreak, PageRule, PageSpecificity, PhysicalAxis,
    PhysicalSide, Position, Quotes, SelfAlignmentKeyword, Stylesheet, StylesheetOrigin,
    TableCellVerticalAlign, TableLayout, TextAlign, TextAlignLast, TextAutospace,
    TextDecorationSkipInk, TextDecorationSkipSpaces, TextDecorationStyle, TextDecorationThickness,
    TextJustify, TextSpacingTrim, TextTransformCase, TextUnderlineOffset, TextUnderlinePosition,
    UnicodeBidi, VerticalAlign, Visibility, WhiteSpace, WritingMode, WritingModeAxes,
    block_end_side, block_start_side, inline_end_side, inline_start_side,
};
use crate::document::{
    Bookmark, BookmarkState, Document, DocumentMetadata, Page, PaintBand, PaintBlendMode,
    PaintCheckpoint, PaintClip, PaintClipPathEffect, PaintClipUnion, PaintDisplacement,
    PaintEffects, PaintFilterEffect, PaintFragment, PaintMaskEffect, PaintPoint, PaintPrimitive,
    PaintRect, PaintSize, PaintSpace, PaintStackingContext, PaintStrokeWidth, PaintTransform,
    PaintTranslation, RenderedClipPathPolygon, RenderedCornerRadius, RenderedGlyph,
    RenderedGradient, RenderedGradientKind, RenderedGradientPattern, RenderedGradientStop,
    RenderedImage, RenderedImagePattern, RenderedImageSourceRect, RenderedLine, RenderedLineSource,
    RenderedLink, RenderedPath, RenderedPathClip, RenderedPathClipPath, RenderedPathCommand,
    RenderedPathFillRule, RenderedPathLineCap, RenderedPathStrokeStyle, RenderedRect,
    RenderedRoundedRect, RenderedRoundedRectRadii, RenderedStroke, RenderedSvgPattern,
    RenderedTextMatrix, RenderedTextRun, StackLevel, TextRunPoint, paint_rect_path_commands,
};
use crate::dom::{self, Element, ElementId, Node, NodeKind};
use crate::resource::ResourceCache;
use crate::svg::SharedSvgAsset;
use crate::text::{
    BidiVisualRange, FontSystem, FontSystemLoad, GlyphInkBox, OBJECT_REPLACEMENT_CHARACTER,
    ResolvedBidiDirection, ShapedInlineLine, StyledTextSpan, TextDecorationFontMetrics,
    bidi_control_scope_for_style, character_has_joining_behavior, character_is_arabic_tatweel,
    character_is_bidi_format_control, character_is_default_ignorable_code_point,
    character_is_first_hangable_punctuation, character_is_hangable_stop_or_comma,
    character_is_join_control, character_is_last_hangable_punctuation,
    character_is_unicode_alphanumeric, character_is_unicode_control, character_is_unicode_mark,
    character_is_unicode_punctuation, character_is_unicode_symbol,
    character_preserves_word_boundary_context, character_receives_text_emphasis_mark,
    contains_bidi_text, is_css_collapsible_whitespace, plaintext_direction_for_text,
    text_with_hyphenation_controls, text_without_bidi_format_controls,
};
use crate::timing::DebugTimer;
use crate::units::{
    BorderBoxLength, BorderBoxSize, ContentBoxLength, ContentBoxSize, LayoutLength,
    MarginBoxLength, MarginBoxSize, NonContentLength, PercentageBasis, RasterPixelSize,
    SemanticLengthExt, border_box_pt, border_box_to_content_box_length, content_box_pt,
    content_box_size_pt, content_box_to_border_box_length, content_box_to_border_box_size,
    layout_points, layout_pt, margin_box_pt, margin_box_size_pt, non_content_pt,
    raster_natural_layout_size,
};
use std::collections::HashMap;
use taffy::prelude as taffy_layout;

use self::assets::{
    DocumentCanvasBackgroundArea, PaintBackgroundArea,
    background_image_primitives_for_style_with_paint_areas,
    background_image_primitives_for_style_with_paint_areas_and_fixed_positioning_area,
};

mod asset_helpers;
mod assets;
mod block;
#[allow(unused_imports)]
pub(in crate::layout) use self::block::{
    AutoFloatMeasurementKey, FloatAvoidingBfcMeasurement, FloatAvoidingBfcPlacement, FloatBand,
    FloatBandPlacement, FloatBandQuery, FloatClearanceResolution, FloatContext, FloatId,
    FloatPaintFragment, FloatPlacement, FloatRunState, FloatShape, LogicalFloatBand, UsedFloatSide,
    float_avoiding_auto_border_box_width, vertical_physical_inline_span,
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
pub(crate) use paint_helpers::shaped_rect_path_commands;
mod paint_ops;
mod positioned_child;
mod quotes;
mod scroll_snap;
mod table;
mod table_span;
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
        metadata: crate::image_store::ImageMetadata {
            pixel_width: decoded.pixel_width,
            pixel_height: decoded.pixel_height,
        },
        color_space: decoded.color_space,
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
pub use self::split_1::*;
mod split_2;
pub(in crate::layout) use self::split_2::*;
mod split_3;
pub(in crate::layout) use self::split_3::*;
mod split_4;
