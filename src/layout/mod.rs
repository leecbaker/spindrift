use crate::css::{
    self, AdditiveCounterStyle, AlignContent, AlignItems, AlignSelf, AlignmentBaseline,
    AlignmentSafety, BackgroundImage, BaselineMetric, BaselineShift, BookmarkLabelPart,
    BorderStyle, BoxSizing, CaptionSide, Clear, ClipPath, Color, ComputedStyle, Content,
    ContentAlignmentKeyword, ContentVisibility, CounterStyleRange, CounterStyleRule,
    CounterStyleSystem, CssBookmarkState, Declarations, Direction, Display, DisplayInner,
    DominantBaseline, ElementAttributeSignature, ElementSiblingSignature, ElementSignature,
    EmptyCells, FilterValue, FlexDirection, FlexWrap, Float, GeneratedAltTextPart,
    GeneratedContentPart, GeneratedQuote, Isolation, JustifyContent, LinearGradientDirection,
    ListStylePosition, ListStyleType, MarkerContent, MarkerContentPart, MarkerSide, MaskValue,
    MixBlendMode, NamedStringPart, NumericCounterStyle, PageBreak, PageRule, PageSpecificity,
    PhysicalAxis, PhysicalSide, Position, Quotes, SelfAlignmentKeyword, Stylesheet,
    StylesheetOrigin, TableCellVerticalAlign, TableLayout, TextAlign, TextAlignLast, TextAutospace,
    TextDecorationSkipInk, TextDecorationSkipSpaces, TextDecorationStyle, TextDecorationThickness,
    TextJustify, TextTransformCase, TextUnderlineOffset, TextUnderlinePosition, UnicodeBidi,
    VerticalAlign, Visibility, WhiteSpace, WritingMode, block_end_side, block_start_side,
    inline_end_side, inline_start_side,
};
use crate::document::{
    Bookmark, BookmarkState, Document, DocumentMetadata, Page, PaintBand, PaintBlendMode,
    PaintCheckpoint, PaintClip, PaintClipPathEffect, PaintEffects, PaintFilterEffect,
    PaintFragment, PaintMaskEffect, PaintPoint, PaintPrimitive, PaintRect, PaintSize,
    PaintStackingContext, PaintTransform, PaintVector, RenderedCornerRadius, RenderedGlyph,
    RenderedImage, RenderedImageSourceRect, RenderedLine, RenderedLink, RenderedPath,
    RenderedPathClip, RenderedPathClipPath, RenderedPathCommand, RenderedPathFillRule,
    RenderedRect, RenderedRoundedRect, RenderedRoundedRectRadii, RenderedStroke,
    RenderedTextMatrix, RenderedTextRun, StackLevel,
};
use crate::dom::{self, Element, Node, NodeKind};
use crate::resource::ResourceCache;
use crate::text::{
    FontSystem, FontSystemLoad, FontSystemSeedLoad, GlyphInkBox, OBJECT_REPLACEMENT_CHARACTER,
    ShapedInlineLine, StyledTextSpan, TextDecorationFontMetrics, bidi_control_scope_for_style,
    character_is_arabic_tatweel, character_is_bidi_format_control,
    character_is_default_ignorable_code_point, character_is_first_hangable_punctuation,
    character_is_hangable_stop_or_comma, character_is_join_control,
    character_is_last_hangable_punctuation, character_is_unicode_alphanumeric,
    character_is_unicode_control, character_is_unicode_mark, character_is_unicode_punctuation,
    character_is_unicode_symbol, character_preserves_word_boundary_context,
    character_receives_text_emphasis_mark, contains_bidi_text, is_css_collapsible_whitespace,
    plaintext_direction_for_text, text_with_hyphenation_controls,
    text_without_bidi_format_controls,
};
use crate::timing::DebugTimer;
use base64::Engine as _;
use image::GenericImageView;
use std::collections::HashMap;
use std::path::Path;
use taffy::prelude as taffy_layout;

mod asset_helpers;
mod assets;
mod block;
mod box_tree;
mod builder;
mod counters;
mod element_semantics;
mod flex;
mod flow_helpers;
mod geometry;
mod grid;
mod html_direction;
mod inline_boundary;
mod inline_collect;
mod inline_helpers;
mod inline_layout;
mod inline_row;
mod intrinsic;
mod list;
mod page_generated;
mod page_margin;
mod paint_helpers;
mod paint_ops;
mod quotes;
mod table;
mod table_span;
mod text_helpers;
mod text_paint;
mod used_values;

use asset_helpers::*;
use element_semantics::*;
use flow_helpers::*;
use geometry::*;
use html_direction::*;
use inline_boundary::*;
use inline_collect::{block_bidi_scope_needs_inline_controls, push_inline_words_for_style};
use inline_helpers::*;
use paint_helpers::*;
use table_span::*;
use text_helpers::*;
use used_values::*;

#[cfg(test)]
fn block_align_content_y_offset(align_content: AlignContent, free_space: f32) -> f32 {
    content_alignment_y_offset(align_content, free_space, true)
}

/// Return the page-space block-axis offset for a block container or table cell
/// with a concrete computed style.
///
/// CSS Box Alignment defaults block-container overflow alignment to `safe`
/// unless the alignment container is scrollable:
/// <https://www.w3.org/TR/css-align-3/#overflow-values>.
fn block_align_content_y_offset_for_style(style: &ComputedStyle, free_space: f32) -> f32 {
    content_alignment_y_offset(
        style.align_content,
        free_space,
        block_align_content_defaults_to_safe_overflow(style),
    )
}

fn block_align_content_defaults_to_safe_overflow(style: &ComputedStyle) -> bool {
    match block_start_side(style.writing_mode).axis() {
        PhysicalAxis::Horizontal => !style.overflow_x.is_scrollable(),
        PhysicalAxis::Vertical => !style.overflow_y.is_scrollable(),
    }
}

/// Return the page-space block-axis offset for a multi-column
/// `align-content` alignment subject.
///
/// CSS Box Alignment gives block containers a default safe overflow position,
/// but other alignment contexts use unsafe overflow unless `safe` is explicit:
/// <https://www.w3.org/TR/css-align-3/#overflow-values>.
fn multicol_align_content_y_offset(align_content: AlignContent, free_space: f32) -> f32 {
    content_alignment_y_offset(align_content, free_space, false)
}

fn content_alignment_y_offset(
    align_content: AlignContent,
    free_space: f32,
    default_safe_overflow: bool,
) -> f32 {
    -content_alignment_offset_toward_end(align_content, free_space, default_safe_overflow)
}

fn content_alignment_offset_toward_end(
    align_content: AlignContent,
    free_space: f32,
    default_safe_overflow: bool,
) -> f32 {
    let has_overflow = free_space <= 0.0;
    let implicit_safe = matches!(
        align_content.keyword,
        ContentAlignmentKeyword::Stretch
            | ContentAlignmentKeyword::SpaceBetween
            | ContentAlignmentKeyword::SpaceAround
            | ContentAlignmentKeyword::SpaceEvenly
            | ContentAlignmentKeyword::Baseline
            | ContentAlignmentKeyword::LastBaseline
    );
    let safety = match align_content.safety {
        AlignmentSafety::Default if default_safe_overflow => AlignmentSafety::Safe,
        AlignmentSafety::Default => AlignmentSafety::Unsafe,
        explicit => explicit,
    };
    if has_overflow && (safety == AlignmentSafety::Safe || implicit_safe) {
        return 0.0;
    }

    let factor = match align_content.keyword {
        ContentAlignmentKeyword::Normal
        | ContentAlignmentKeyword::Start
        | ContentAlignmentKeyword::FlexStart
        | ContentAlignmentKeyword::Stretch
        | ContentAlignmentKeyword::SpaceBetween
        | ContentAlignmentKeyword::Baseline => 0.0,
        ContentAlignmentKeyword::End
        | ContentAlignmentKeyword::FlexEnd
        | ContentAlignmentKeyword::LastBaseline => 1.0,
        ContentAlignmentKeyword::Center
        | ContentAlignmentKeyword::SpaceAround
        | ContentAlignmentKeyword::SpaceEvenly => 0.5,
        ContentAlignmentKeyword::Left | ContentAlignmentKeyword::Right => 0.0,
    };
    free_space * factor
}

/// Returns whether block `align-content` forces an independent formatting
/// context.
///
/// CSS Align defines `align-content` values other than `normal` on block
/// containers as establishing an independent formatting context, so outside
/// floats cannot intrude into the aligned contents:
/// <https://www.w3.org/TR/css-align-3/#align-content-property>.
fn block_align_content_establishes_independent_formatting_context(
    align_content: AlignContent,
) -> bool {
    align_content.keyword != ContentAlignmentKeyword::Normal
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

const LIST_ITEM_COUNTER_NAME: &str = "list-item";

impl PageSize {
    pub const A4_POINTS: Self = Self {
        width: 595.2756,
        height: 841.8898,
    };

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMargins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl PageMargins {
    pub const WEASYPRINT_DEFAULT_POINTS: f32 = 56.25;
    pub const DEFAULT: Self = Self {
        top: Self::WEASYPRINT_DEFAULT_POINTS,
        right: Self::WEASYPRINT_DEFAULT_POINTS,
        bottom: Self::WEASYPRINT_DEFAULT_POINTS,
        left: Self::WEASYPRINT_DEFAULT_POINTS,
    };

    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    pub page_size: PageSize,
    pub margin: f32,
    pub page_margins: PageMargins,
    pub font_size: f32,
    pub line_height: f32,
    pub producer: String,
    /// PDF variant used for serialization and conformance-identification metadata.
    pub pdf_variant: crate::document::PdfVariant,
    /// Enable HTML presentational hints as zero-specificity author CSS.
    ///
    /// WeasyPrint exposes this as an opt-in compatibility feature. HTML maps
    /// these attributes into the CSS cascade as presentational hints:
    /// <https://html.spec.whatwg.org/multipage/rendering.html#presentational-hints>.
    pub presentational_hints: bool,
    /// URL fragment target used by Selectors `:target` and `:target-within`.
    ///
    /// Static PDF rendering has no browsing session, so the target element is
    /// an explicit render input when callers want fragment-sensitive styling:
    /// <https://www.w3.org/TR/selectors-4/#the-target-pseudo>.
    pub target_fragment: Option<String>,
}

impl RenderOptions {
    pub fn set_margin(&mut self, margin: f32) {
        self.margin = margin;
        self.page_margins = PageMargins::all(margin);
    }

    pub fn set_page_margins(&mut self, margins: PageMargins) {
        self.margin = margins.top;
        self.page_margins = margins;
    }

    pub fn page_margins(&self) -> PageMargins {
        if self.page_margins == PageMargins::DEFAULT
            && (self.margin - PageMargins::WEASYPRINT_DEFAULT_POINTS).abs() > 0.01
        {
            PageMargins::all(self.margin)
        } else {
            self.page_margins
        }
    }

    pub(crate) fn page_left(&self) -> f32 {
        self.page_margins().left
    }

    pub(crate) fn page_top(&self) -> f32 {
        self.page_size.height - self.page_margins().top
    }

    pub(crate) fn page_bottom(&self) -> f32 {
        self.page_margins().bottom
    }

    pub(crate) fn page_area_width(&self) -> f32 {
        (self.page_size.width - self.page_margins().left - self.page_margins().right).max(0.0)
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        let font_size = 12.0;
        Self {
            page_size: PageSize::A4_POINTS,
            margin: PageMargins::WEASYPRINT_DEFAULT_POINTS,
            page_margins: PageMargins::DEFAULT,
            font_size,
            line_height: font_size * 1.2,
            producer: "reasyprint 0.1.0".to_string(),
            pdf_variant: crate::document::PdfVariant::default(),
            presentational_hints: false,
            target_fragment: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PageContext {
    size: PageSize,
    margins: PageMargins,
    edges: PageBoxEdges,
    rotation: i32,
}

/// Used page-box border and padding edges for the document page area.
///
/// CSS Paged Media makes page boxes follow the CSS box model: page margins
/// surround the page border, page padding is inside that border, and document
/// content is laid out in the page area/content box:
/// <https://www.w3.org/TR/css-page-3/#page-model> and
/// <https://www.w3.org/TR/css-box-3/#box-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PageBoxEdges {
    border: css::Edges,
    padding: css::Edges,
}

impl PageBoxEdges {
    const ZERO: Self = Self {
        border: css::Edges::ZERO,
        padding: css::Edges::ZERO,
    };

    fn left(self) -> f32 {
        self.border.left + self.padding.left
    }

    fn right(self) -> f32 {
        self.border.right + self.padding.right
    }

    fn top(self) -> f32 {
        self.border.top + self.padding.top
    }

    fn bottom(self) -> f32 {
        self.border.bottom + self.padding.bottom
    }

    fn total(self) -> css::Edges {
        css::Edges {
            top: self.top(),
            right: self.right(),
            bottom: self.bottom(),
            left: self.left(),
        }
    }
}

impl PageContext {
    fn from_options(options: &RenderOptions) -> Self {
        Self {
            size: options.page_size,
            margins: options.page_margins(),
            edges: PageBoxEdges::ZERO,
            rotation: 0,
        }
    }

    fn left(self) -> f32 {
        self.margins.left + self.edges.left()
    }

    fn right(self) -> f32 {
        self.size.width - self.margins.right - self.edges.right()
    }

    fn top(self) -> f32 {
        self.size.height - self.margins.top - self.edges.top()
    }

    fn bottom(self) -> f32 {
        self.margins.bottom + self.edges.bottom()
    }

    fn area_width(self) -> f32 {
        (self.size.width
            - self.margins.left
            - self.margins.right
            - self.edges.left()
            - self.edges.right())
        .max(0.0)
    }

    fn area_height(self) -> f32 {
        (self.size.height
            - self.margins.top
            - self.margins.bottom
            - self.edges.top()
            - self.edges.bottom())
        .max(0.0)
    }
}

fn layout_text_with_font_system(
    text: &str,
    options: &RenderOptions,
    mut font_system: FontSystem,
) -> Document {
    let mut default_style = ComputedStyle::initial();
    default_style.font_size = options.font_size;
    default_style.line_height_value = css::ComputedLineHeight::Length(options.line_height);
    default_style.line_height = options.line_height;
    default_style.line_height_multiplier = None;
    default_style.line_height_is_normal = false;
    let font_id = font_system.resolve_style(&default_style);
    let content_width = options.page_area_width().max(options.font_size);
    let approx_char_width = options.font_size * 0.5;
    let max_chars = (content_width / approx_char_width).floor().max(1.0) as usize;

    let mut pages = Vec::new();
    let mut lines = Vec::new();
    let mut y = options.page_top() - options.font_size;
    let bottom = options.page_bottom();

    for line in wrap_text(text, max_chars) {
        if y < bottom {
            let mut page = Page::new(options.page_size.width, options.page_size.height);
            for line in lines {
                page.push_line(line);
            }
            pages.push(page);
            lines = Vec::new();
            y = options.page_top() - options.font_size;
        }
        let runs = font_system.shape_text_runs_with_parley(&line, &default_style);
        let line_font_id = runs.first().and_then(|run| run.font_id).or(font_id);
        lines.push(RenderedLine::from_paint_origin(
            line,
            paint_space_point(options.page_left(), y),
            options.font_size,
            line_font_id,
            Color::BLACK,
            runs,
        ));
        y -= options.line_height;
    }

    if lines.is_empty() && pages.is_empty() {
        lines.push(RenderedLine::from_paint_origin(
            String::new(),
            paint_space_point(options.page_left(), y),
            options.font_size,
            font_id,
            Color::BLACK,
            Vec::new(),
        ));
    }

    if !lines.is_empty() {
        let mut page = Page::new(options.page_size.width, options.page_size.height);
        for line in lines {
            page.push_line(line);
        }
        pages.push(page);
    }

    Document {
        pages,
        fonts: font_system.into_fonts(),
        bookmarks: Vec::new(),
        metadata: DocumentMetadata {
            producer: options.producer.clone(),
            ..DocumentMetadata::default()
        },
    }
}

pub(crate) fn start_font_system_load() -> FontSystemLoad {
    FontSystem::start_loading()
}

const LAYOUT_THREAD_STACK_SIZE: usize = 8 * 1024 * 1024;

enum LayoutWorkerResult {
    Document(Document),
    Empty(Box<FontSystem>),
}

pub(crate) async fn layout_dom_async(
    root: &Node,
    stylesheets: &[Stylesheet],
    options: &RenderOptions,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    resource_cache: &ResourceCache,
    font_system_load: FontSystemSeedLoad,
) -> Document {
    let _timer = DebugTimer::start("layout pipeline");
    let parent_style = ComputedStyle {
        font_size: options.font_size,
        line_height_value: css::ComputedLineHeight::Length(options.line_height),
        line_height: options.line_height,
        color: Color::BLACK,
        ..ComputedStyle::initial()
    };
    let page_progression_direction = {
        let _timer = DebugTimer::start("resolving page progression direction");
        document_page_progression_direction(root, stylesheets, &parent_style)
    };
    let page_counter_initial_values = {
        let _timer = DebugTimer::start("resolving page counter seeds");
        page_counter_initial_values(root, stylesheets, &parent_style)
    };
    let font_system = {
        let _timer = DebugTimer::start("finishing font system load");
        font_system_load.finish().await
    };
    let worker_result = {
        let _timer = DebugTimer::start("building and flowing page box content");
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("quire-layout".to_string())
                .stack_size(LAYOUT_THREAD_STACK_SIZE)
                .spawn_scoped(scope, move || {
                    let mut page_box = {
                        let _timer = DebugTimer::start("building formatting box tree");
                        box_tree::build_page_box(root, stylesheets, &parent_style)
                    };
                    let mut builder = LayoutBuilder::new(LayoutBuilderConfig {
                        options,
                        stylesheets,
                        base_url,
                        root_url,
                        resource_cache,
                        page_progression_direction,
                        page_counter_initial_values,
                        font_system,
                    });
                    {
                        let _timer = DebugTimer::start("resolving font-metric lengths");
                        builder.resolve_font_metric_lengths_in_page_box(&mut page_box);
                    }
                    {
                        let _timer = DebugTimer::start("flowing page box content");
                        builder.layout_page_box(&page_box, stylesheets);
                    }
                    if !builder.has_renderable_content() {
                        LayoutWorkerResult::Empty(Box::new(builder.font_system))
                    } else {
                        let _timer = DebugTimer::start("finalizing laid out document");
                        LayoutWorkerResult::Document(builder.finish())
                    }
                })
                .expect("failed to spawn layout worker")
                .join()
                .expect("layout worker panicked")
        })
    };
    match worker_result {
        LayoutWorkerResult::Document(document) => document,
        LayoutWorkerResult::Empty(font_system) => {
            let font_system = *font_system;
            let text = dom::text_content(root);
            if !text.is_empty() {
                log::debug!("falling back to plain text layout");
                layout_text_with_font_system(&text, options, font_system)
            } else {
                let _timer = DebugTimer::start("finalizing empty laid out document");
                Document {
                    pages: Vec::new(),
                    fonts: font_system.into_fonts(),
                    bookmarks: Vec::new(),
                    metadata: DocumentMetadata {
                        producer: options.producer.clone(),
                        ..DocumentMetadata::default()
                    },
                }
            }
        }
    }
}

/// Returns the document direction used for `@page :left`/`:right` matching.
///
/// CSS Paged Media defines spread pseudo-classes in terms of page progression;
/// for horizontal documents this follows the root element's `direction`:
/// <https://www.w3.org/TR/css-page-3/#spread-pseudos>.
fn document_page_progression_direction(
    root: &Node,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
) -> Direction {
    let NodeKind::Element(root_element) = &root.kind else {
        return parent_style.direction;
    };
    let sibling_tags = element_sibling_tags(root_element);
    let element_index = 0usize;
    for child in &root_element.children {
        let NodeKind::Element(element) = &child.kind else {
            continue;
        };
        let signature = ElementSignature::with_siblings(
            element.tag.clone(),
            element.attrs.clone(),
            element_index,
            sibling_tags.clone(),
        );
        let style =
            style_for_layout_element(element, signature, stylesheets, Some(parent_style), &[]);
        return style.direction;
    }
    parent_style.direction
}

/// Captures root counter resets that seed page-context counters.
///
/// CSS Paged Media page counters are independent page-associated counters, but
/// document counters can initialize them before page-context rules increment
/// or reset values for each generated page:
/// <https://www.w3.org/TR/css-page-3/#page-based-counters> and
/// <https://www.w3.org/TR/css-lists-3/#auto-numbering>.
fn page_counter_initial_values(
    root: &Node,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
) -> HashMap<String, i32> {
    let NodeKind::Element(root_element) = &root.kind else {
        return HashMap::new();
    };
    let counter_element = root_element
        .children
        .iter()
        .find_map(|child| match &child.kind {
            NodeKind::Element(element) => Some(element),
            NodeKind::Text(_) => None,
        })
        .unwrap_or(root_element);
    let signature =
        ElementSignature::new(counter_element.tag.clone(), counter_element.attrs.clone());
    let style = style_for_layout_element(
        counter_element,
        signature,
        stylesheets,
        Some(parent_style),
        &[],
    );
    let mut values = HashMap::new();
    for (name, value) in style.counter_resets {
        values.insert(name, value);
    }
    for (name, amount) in style.counter_increments {
        *values.entry(name).or_insert(0) += amount;
    }
    for (name, value) in style.counter_sets {
        values.insert(name, value);
    }
    values
}

struct LayoutBuilder<'a> {
    options: &'a RenderOptions,
    stylesheets: &'a [Stylesheet],
    base_url: Option<&'a Path>,
    root_url: Option<&'a Path>,
    resource_cache: &'a ResourceCache,
    pages: Vec<Page>,
    page_names: Vec<Option<String>>,
    page_blanks: Vec<bool>,
    page_name_scope_suppression: usize,
    page_name_element_scope_suppression: usize,
    page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    page_anchors: HashMap<String, usize>,
    page_anchor_text: HashMap<String, AnchorText>,
    document_canvas_background: Option<ComputedStyle>,
    root_canvas_background_defined: bool,
    current_page: Page,
    current_page_has_flow_content: bool,
    current_page_name: Option<String>,
    current_page_context: PageContext,
    cursor_y: f32,
    content_left: f32,
    content_right: f32,
    inline_static_baseline_y: Option<f32>,
    block_static_position_y_offset: Option<f32>,
    containing_block_direction: Direction,
    containing_block_writing_mode: WritingMode,
    fragment_top_offsets: Vec<f32>,
    definite_block_size_stack: Vec<Option<f32>>,
    truncate_page_start_margins: bool,
    avoid_inside_retry_depth: usize,
    out_of_flow_prebreak_suppression_depth: usize,
    containing_blocks: Vec<ContainingBlock>,
    list_stack: Vec<ListState>,
    counter_set: CounterSet,
    quote_depth: usize,
    current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    current_page_running_elements: HashMap<String, Vec<NamedStringAssignment>>,
    next_assignment_id: usize,
    assignment_capture_stack: Vec<Vec<AssignmentId>>,
    ancestors: Vec<ElementSignature>,
    page_counter_initial_values: HashMap<String, i32>,
    page_rules: Vec<PageRule>,
    page_progression_direction: Direction,
    page_declarations: Declarations,
    page_margin_boxes: HashMap<String, Declarations>,
    counter_styles: HashMap<String, CounterStyleRule>,
    first_page_declarations: Declarations,
    font_system: FontSystem,
    bookmarks: Vec<Bookmark>,
    positioned_layers: Vec<PositionedPaintLayer>,
    fixed_layers: Vec<FixedPaintLayer>,
    next_paint_source_order: usize,
    overflow_clips: Vec<OverflowClip>,
    next_float_id: usize,
    float_contexts: Vec<FloatContext>,
    pending_float_fragments: Vec<PendingFloatPaintFragment>,
    pending_float_side_effects: Vec<PendingFloatSideEffects>,
    applied_clearance_count: usize,
    preserve_scoped_paint_public_order: bool,
}

struct LayoutBuilderConfig<'a> {
    options: &'a RenderOptions,
    stylesheets: &'a [Stylesheet],
    base_url: Option<&'a Path>,
    root_url: Option<&'a Path>,
    resource_cache: &'a ResourceCache,
    page_progression_direction: Direction,
    page_counter_initial_values: HashMap<String, i32>,
    font_system: FontSystem,
}

/// Insets of the active layout fragment from the current page area.
///
/// CSS Fragmentation keeps the fragmented box in its original formatting
/// context when content continues on another page, and CSS Paged Media selects
/// a new page area for each page box:
/// <https://www.w3.org/TR/css-break-3/#breaking-controls> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FragmentOffsets {
    left: f32,
    right: f32,
    top: f32,
}

/// Tracks a legacy immediate float row view for callers that need the first
/// line's already-placed exclusions.
///
/// CSS 2.2 floats are shifted to the line's left or right edge and subsequent
/// floats are placed beside previous floats when space permits:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FloatRunState {
    /// Full physical row span before same-row floats shorten it.
    ///
    /// CSS 2.2 places consecutive floats beside earlier floats when possible.
    /// This span is page physical `x` in the current block formatting context:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    row_span: PageInlineSpan,
    /// Remaining physical row span after same-row floats have been included.
    ///
    /// This is the immediate line-box availability for legacy float placement
    /// callers; durable later exclusions are stored as [`FloatShape`] entries
    /// in [`FloatContext`].
    available_span: PageInlineSpan,
    /// Physical block interval occupied by same-row floats.
    ///
    /// The span uses Quire's page top-edge convention: `top_y` is the row top
    /// and `bottom_y` moves downward as floats are added. CSS floats shorten
    /// later line boxes until the lowest same-row float bottom:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    occupied_block_span: PageBlockSpan,
    active: bool,
}

/// Durable float exclusion list for one block formatting context.
///
/// CSS 2.2 keeps floated margin boxes out of normal flow but shortens later
/// line boxes and formatting contexts around them in the same block formatting
/// context:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, PartialEq)]
struct FloatContext {
    shapes: Vec<FloatShape>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FloatId(usize);

#[derive(Debug, Clone, Copy, PartialEq)]
struct FloatShape {
    id: FloatId,
    specified_side: Float,
    side: UsedFloatSide,
    source_order: usize,
    fragment_index: usize,
    starts_on_previous_page: bool,
    continues_on_next_page: bool,
    page_index: usize,
    rect: PageTopRect,
}

impl FloatShape {
    fn from_fragment(fragment: &FloatPaintFragment) -> Self {
        Self {
            id: fragment.id,
            specified_side: fragment.specified_side,
            side: fragment.side,
            source_order: fragment.source_order,
            fragment_index: fragment.fragment_index,
            starts_on_previous_page: fragment.starts_on_previous_page,
            continues_on_next_page: fragment.continues_on_next_page,
            page_index: fragment.page_index,
            rect: fragment.rect,
        }
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn from_edges(
        id: FloatId,
        specified_side: Float,
        side: UsedFloatSide,
        source_order: usize,
        fragment_index: usize,
        starts_on_previous_page: bool,
        continues_on_next_page: bool,
        page_index: usize,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> Self {
        Self {
            id,
            specified_side,
            side,
            source_order,
            fragment_index,
            starts_on_previous_page,
            continues_on_next_page,
            page_index,
            rect: PageTopRect::new(left, top, (right - left).max(0.0), (top - bottom).max(0.0)),
        }
    }

    fn left(self) -> f32 {
        self.rect.x
    }

    fn right(self) -> f32 {
        self.rect.x + self.rect.width
    }

    fn top(self) -> f32 {
        self.rect.top_y
    }

    fn bottom(self) -> f32 {
        self.rect.bottom_y()
    }
}

/// Durable page-local representation of one floated box fragment.
///
/// CSS 2.2 floats exclude later content using their margin boxes, while CSS
/// Fragmentation can split a floated box across page fragmentainers. Each
/// visible fragment therefore needs both a paint-tree context and a page-local
/// exclusion shape:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, PartialEq)]
struct FloatPaintFragment {
    id: FloatId,
    specified_side: Float,
    page_index: usize,
    side: UsedFloatSide,
    rect: PageTopRect,
    source_order: usize,
    fragment_index: usize,
    starts_on_previous_page: bool,
    continues_on_next_page: bool,
    context: PaintStackingContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsedFloatSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl UsedFloatSide {
    fn from_float(float: Float, writing_mode: WritingMode, direction: Direction) -> Option<Self> {
        match float {
            Float::None => None,
            Float::Left => Some(Self::Left),
            Float::Right => Some(Self::Right),
            Float::InlineStart => Some(Self::from_physical_side(inline_start_side(
                writing_mode,
                direction,
            ))),
            Float::InlineEnd => Some(Self::from_physical_side(inline_end_side(
                writing_mode,
                direction,
            ))),
        }
    }

    fn from_physical_side(side: PhysicalSide) -> Self {
        match side {
            PhysicalSide::Left => Self::Left,
            PhysicalSide::Right => Self::Right,
            PhysicalSide::Top => Self::Top,
            PhysicalSide::Bottom => Self::Bottom,
        }
    }

    fn matches_clear(self, clear: Clear, writing_mode: WritingMode, direction: Direction) -> bool {
        let clear_side = match clear {
            Clear::None => return false,
            Clear::Both => return true,
            Clear::Left => Self::Left,
            Clear::Right => Self::Right,
            Clear::InlineStart => {
                Self::from_physical_side(inline_start_side(writing_mode, direction))
            }
            Clear::InlineEnd => Self::from_physical_side(inline_end_side(writing_mode, direction)),
        };
        self == clear_side
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PendingFloatPaintFragment {
    page_index: usize,
    fragment: PaintFragment,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct PendingFloatSideEffects {
    page_index: usize,
    named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    running_elements: HashMap<String, Vec<NamedStringAssignment>>,
    links: Vec<RenderedLink>,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct FloatLayoutSideEffects {
    bookmarks: Vec<Bookmark>,
    anchors: Vec<(String, usize)>,
    anchor_text: Vec<(String, AnchorText)>,
    page_effects: Vec<PendingFloatSideEffects>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FloatBand {
    /// The remaining physical line-box span in page coordinates after active
    /// CSS floats have shortened the row.
    ///
    /// CSS 2.2 defines floats as shortening line boxes in the same block
    /// formatting context. The span is physical page `x`, not logical inline
    /// coordinates; vertical writing modes must use [`LogicalFloatBand`]:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    span: PageInlineSpan,
}

impl FloatBand {
    fn from_edges(left: f32, right: f32) -> Self {
        Self {
            span: PageInlineSpan::from_edges(left, right),
        }
    }

    fn left(self) -> f32 {
        self.span.left_x()
    }

    fn right(self) -> f32 {
        self.span.right_x()
    }

    fn width(self) -> f32 {
        self.span.width()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LogicalFloatBand {
    /// Available logical inline interval after float exclusions.
    ///
    /// CSS Writing Modes defines inline coordinates independently from the
    /// physical page axis. This span is logical inline progress inside the
    /// queried line/slab, after active CSS floats have shortened it:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    inline_span: LogicalInlineSpan,
    /// Physical page-y interval that corresponds to the available inline slab.
    ///
    /// Vertical writing modes can shorten the physical top or bottom of the
    /// slab while still reporting a logical inline span to inline layout.
    block_span: PageBlockSpan,
}

impl LogicalFloatBand {
    fn new(inline_start: f32, inline_size: f32, physical_top: f32, physical_bottom: f32) -> Self {
        Self {
            inline_span: LogicalInlineSpan::new(inline_start, inline_size),
            block_span: PageBlockSpan::from_edges(physical_top, physical_bottom),
        }
    }

    fn inline_start(self) -> f32 {
        self.inline_span.start()
    }

    fn inline_end(self) -> f32 {
        self.inline_span.end()
    }

    fn available_inline_size(self) -> f32 {
        self.inline_span.size()
    }

    fn physical_top(self) -> f32 {
        self.block_span.top_y()
    }

    fn physical_bottom(self) -> f32 {
        self.block_span.bottom_y()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FloatPlacement {
    /// Physical top-left placement of the float margin box in page-top space.
    ///
    /// CSS 2.2 places a float as far left or right as possible while its top
    /// edge is at or below the current line, after `clear` and active float
    /// exclusions are applied. The top-edge convention matches block layout's
    /// downward cursor before paint conversion:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    origin: PageTopPoint,
    /// Physical line-box span available at this float's block position.
    ///
    /// CSS floats shorten later line boxes in the same block formatting
    /// context. This span is the page-local horizontal band that accepted the
    /// float, not a CSS logical inline interval; vertical-writing float
    /// avoidance maps its logical inline availability into this typed result.
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    available_span: PageInlineSpan,
}

impl FloatPlacement {
    fn new(left: f32, top: f32, available_width: f32) -> Self {
        Self {
            origin: PageTopPoint::new(left, top),
            available_span: PageInlineSpan::new(left, available_width),
        }
    }

    fn left(self) -> f32 {
        self.origin.x()
    }

    fn top(self) -> f32 {
        self.origin.top_y()
    }

    fn available_width(self) -> f32 {
        self.available_span.width()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FloatClearanceResolution {
    top: f32,
    continued_float: Option<FloatId>,
}

/// Page-local containing block for positioned descendants.
///
/// CSS Positioned Layout resolves absolute and fixed offsets against a
/// containing block. Quire stores that box in physical page coordinates using
/// a top edge (`top_y`) because layout cursors advance downward, while the
/// `height` remains the physical block extent used for percentage resolution:
/// <https://www.w3.org/TR/css-position-3/#def-cb>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ContainingBlock {
    rect: PageTopRect,
}

impl ContainingBlock {
    fn from_page_top_rect(rect: PageTopRect) -> Self {
        Self { rect }
    }

    fn x(self) -> f32 {
        self.rect.x
    }

    fn top_y(self) -> f32 {
        self.rect.top_y
    }

    fn width(self) -> f32 {
        self.rect.width
    }

    fn height(self) -> f32 {
        self.rect.height
    }

    #[allow(dead_code)]
    fn page_top_rect(self) -> PageTopRect {
        self.rect
    }
}

/// Active axis-aligned overflow clipping rectangle.
///
/// CSS Overflow clips non-visible overflow to the box's overflow clip edge,
/// which defaults to the padding box for `overflow: hidden`:
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OverflowClip {
    rect: PaintRect,
}

impl OverflowClip {
    fn from_paint_rect(rect: PaintRect) -> Self {
        Self { rect }
    }

    fn from_page_top_rect(rect: PageTopRect) -> Self {
        Self::from_paint_rect(rect.paint_rect())
    }

    fn width(self) -> f32 {
        self.rect.size.width
    }

    fn height(self) -> f32 {
        self.rect.size.height
    }

    fn paint_rect(self) -> PaintRect {
        self.rect
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DecodedPngImage {
    pixel_width: u32,
    pixel_height: u32,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ListState {
    step: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct CounterSet {
    values: HashMap<String, Vec<i32>>,
    frames: Vec<CounterFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CounterFrame {
    base_lengths: HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CounterScopeState {
    temporary_counters: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedPseudoCounterMode {
    Commit,
    Rollback,
}

#[derive(Debug, Clone, PartialEq)]
struct PositionedPaintLayer {
    page_index: usize,
    stack_level: StackLevel,
    context: PaintStackingContext,
    links: Vec<RenderedLink>,
}

impl PositionedPaintLayer {
    fn translated(mut self, offset: PaintVector) -> Self {
        self.context = self.context.translated(offset);
        self.links = self
            .links
            .into_iter()
            .map(|link| link.translated(offset))
            .collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
struct FixedPaintLayer {
    stack_level: StackLevel,
    context: PaintStackingContext,
    links: Vec<RenderedLink>,
}

/// Internal CSS stacking-context decision for one laid-out box fragment.
///
/// CSS Positioned Layout and CSS 2.2 Appendix E decide paint placement from
/// stack level, while CSS Transforms, CSS Color opacity, and CSS Overflow add
/// group effects. Keeping this classification in one value prevents layout
/// paths from independently deciding which positioned descendants are captured:
/// <https://www.w3.org/TR/css-position-3/#painting-order>,
/// <https://www.w3.org/TR/CSS22/zindex.html>,
/// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>,
/// <https://www.w3.org/TR/css-color-4/#transparency>, and
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StackingContextPolicy {
    parent_band: PaintBand,
    stack_level: StackLevel,
    context_kind: StackingContextKind,
    child_layer_policy: ChildLayerPolicy,
    is_real_stacking_context: bool,
    is_fake_context: bool,
    creates_compositing_group: bool,
    establishes_containing_block: bool,
    captures_positioned_descendants: bool,
    effects: PaintEffects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackingContextKind {
    None,
    Real,
    FakeAtomic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildLayerPolicy {
    CaptureAll,
    CaptureAutoLevel,
    EscapeAll,
}

impl StackingContextPolicy {
    fn for_positioned(style: &ComputedStyle, bounds: PaintClip) -> Self {
        let effects = assets::paint_effects_for_box(style, bounds);
        let is_real_stacking_context = matches!(style.position, Position::Fixed | Position::Sticky)
            || style.z_index.is_some()
            || style_creates_effect_stacking_context(style, effects);
        let is_fake_context = !is_real_stacking_context
            && matches!(style.position, Position::Relative | Position::Absolute);
        Self {
            parent_band: StackLevel::from_optional_z_index(style.z_index).paint_band(),
            stack_level: StackLevel::from_optional_z_index(style.z_index),
            context_kind: if is_real_stacking_context {
                StackingContextKind::Real
            } else if is_fake_context {
                StackingContextKind::FakeAtomic
            } else {
                StackingContextKind::None
            },
            child_layer_policy: if is_real_stacking_context {
                ChildLayerPolicy::CaptureAll
            } else {
                ChildLayerPolicy::EscapeAll
            },
            is_real_stacking_context,
            is_fake_context,
            creates_compositing_group: effects.needs_group(),
            establishes_containing_block: matches!(
                style.position,
                Position::Relative | Position::Absolute | Position::Fixed | Position::Sticky
            ) || !style.transform.is_empty(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    fn for_non_positioned_effect(style: &ComputedStyle, bounds: PaintClip) -> Self {
        let effects = assets::paint_effects_for_box(style, bounds);
        let in_flow_positioned = matches!(style.position, Position::Relative | Position::Sticky);
        let stack_level = if in_flow_positioned {
            StackLevel::from_optional_z_index(style.z_index)
        } else {
            StackLevel::Auto
        };
        let is_real_stacking_context = matches!(style.position, Position::Sticky)
            || (style.position == Position::Relative && style.z_index.is_some())
            || style_creates_effect_stacking_context(style, effects);
        let is_fake_context = style.position == Position::Relative && !is_real_stacking_context;
        Self {
            parent_band: if in_flow_positioned {
                stack_level.paint_band()
            } else {
                PaintBand::InFlowBlock
            },
            stack_level,
            context_kind: if is_real_stacking_context {
                StackingContextKind::Real
            } else if is_fake_context {
                StackingContextKind::FakeAtomic
            } else {
                StackingContextKind::None
            },
            child_layer_policy: if is_real_stacking_context {
                ChildLayerPolicy::CaptureAll
            } else if is_fake_context {
                ChildLayerPolicy::CaptureAutoLevel
            } else {
                ChildLayerPolicy::EscapeAll
            },
            is_real_stacking_context,
            is_fake_context,
            creates_compositing_group: effects.needs_group(),
            establishes_containing_block: !style.transform.is_empty(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    fn for_atomic(style: &ComputedStyle, parent_band: PaintBand, bounds: PaintClip) -> Self {
        let effects = assets::paint_effects_for_box(style, bounds);
        let is_real_stacking_context = style_creates_effect_stacking_context(style, effects);
        Self {
            parent_band,
            stack_level: StackLevel::Auto,
            context_kind: if is_real_stacking_context {
                StackingContextKind::Real
            } else {
                StackingContextKind::FakeAtomic
            },
            child_layer_policy: if is_real_stacking_context {
                ChildLayerPolicy::CaptureAll
            } else {
                ChildLayerPolicy::EscapeAll
            },
            is_real_stacking_context,
            is_fake_context: true,
            creates_compositing_group: effects.needs_group(),
            establishes_containing_block: !style.transform.is_empty(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    fn for_flex_item(style: &ComputedStyle, bounds: PaintClip) -> Self {
        let stack_level = StackLevel::from_optional_z_index(style.z_index);
        let effects = assets::paint_effects_for_box(style, bounds);
        let is_real_stacking_context =
            style.z_index.is_some() || style_creates_effect_stacking_context(style, effects);
        Self {
            parent_band: stack_level.paint_band(),
            stack_level,
            context_kind: if is_real_stacking_context {
                StackingContextKind::Real
            } else {
                StackingContextKind::None
            },
            child_layer_policy: if is_real_stacking_context {
                ChildLayerPolicy::CaptureAll
            } else {
                ChildLayerPolicy::EscapeAll
            },
            is_real_stacking_context,
            is_fake_context: false,
            creates_compositing_group: effects.needs_group(),
            establishes_containing_block: !style.transform.is_empty(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    fn style_needs_non_positioned_scope(style: &ComputedStyle) -> bool {
        matches!(style.position, Position::Relative | Position::Sticky)
            || style_creates_effect_stacking_context(
                style,
                assets::paint_effects_for_box(
                    style,
                    PaintClip::from_paint_rect(paint_space_rect(0.0, 0.0, 0.0, 0.0)),
                ),
            )
            || style.overflow.clips_overflow()
    }
}

fn style_creates_effect_stacking_context(style: &ComputedStyle, effects: PaintEffects) -> bool {
    effects.opacity < 1.0
        || effects.transform.is_some()
        || effects.clip_path.is_active()
        || effects.mask.is_active()
        || effects.filter.is_active()
        || effects.blend_mode != PaintBlendMode::Normal
        || effects.isolation
        || style.isolation == Isolation::Isolate
        || style.mix_blend_mode != MixBlendMode::Normal
        || !matches!(style.filter, FilterValue::None)
        || style.clip_path != ClipPath::None
        || !matches!(style.mask, MaskValue::None)
        || style.contain.paint
        || matches!(
            style.content_visibility,
            ContentVisibility::Auto | ContentVisibility::Hidden
        )
        || style.will_change.opacity
        || style.will_change.transform
        || style.will_change.filter
        || style.will_change.clip_path
        || style.will_change.mask
        || style.will_change.mix_blend_mode
        || style.will_change.isolation
        || style.will_change.contain
}

type NamedStringAssignment = PageAssignment<PageAssignmentValue>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AssignmentId(usize);

/// Page-local captured value for named strings and running elements.
///
/// CSS GCPM resolves `string()` and `element()` against assignments made by
/// source elements during pagination. Keeping placement with the value lets
/// page-margin resolution distinguish `first`/`last` from exact page-start
/// lookups:
/// <https://www.w3.org/TR/css-gcpm-3/#named-strings> and
/// <https://www.w3.org/TR/css-gcpm-3/#running-elements>.
#[derive(Debug, Clone, PartialEq)]
struct PageAssignment<T> {
    id: AssignmentId,
    value: T,
    placement: AssignmentPlacement,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AssignmentPlacement {
    page_index: usize,
    starts_page_fragment: bool,
    border_box: Option<PaintClip>,
}

#[derive(Debug, Clone, PartialEq)]
struct FragmentPageValue {
    page_name: Option<String>,
    specified: bool,
}

impl FragmentPageValue {
    fn unspecified() -> Self {
        Self {
            page_name: None,
            specified: false,
        }
    }
}

/// Final page-local metadata for a visible layout fragment.
///
/// CSS Fragmentation defines fragments as the durable pieces of a source box,
/// while CSS Paged Media and GCPM resolve named pages, named strings, and
/// running elements from the page fragment that actually contains the source:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>,
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>, and
/// <https://www.w3.org/TR/css-gcpm-3/#named-strings>.
#[derive(Debug, Clone, PartialEq)]
struct FragmentPageMetadata {
    page_index: usize,
    source_border_box: Option<PaintClip>,
    starts_page_fragment: bool,
    continues_from_previous_page: bool,
    continues_to_next_page: bool,
    first_page_value: FragmentPageValue,
    last_page_value: FragmentPageValue,
    assignment_ids: Vec<AssignmentId>,
}

impl FragmentPageMetadata {
    fn new(
        page_index: usize,
        source_border_box: Option<PaintClip>,
        starts_page_fragment: bool,
    ) -> Self {
        Self {
            page_index,
            source_border_box,
            starts_page_fragment,
            continues_from_previous_page: false,
            continues_to_next_page: false,
            first_page_value: FragmentPageValue::unspecified(),
            last_page_value: FragmentPageValue::unspecified(),
            assignment_ids: Vec::new(),
        }
    }

    fn empty(page_index: usize) -> Self {
        Self::new(page_index, None, false)
    }

    fn assignment_placement(&self) -> AssignmentPlacement {
        AssignmentPlacement {
            page_index: self.page_index,
            starts_page_fragment: self.starts_page_fragment,
            border_box: self.source_border_box,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum PageAssignmentValue {
    GeneratedContent(Vec<page_generated::PageMarginContentItem>),
    RunningElement(Box<RunningElementCapture>),
}

#[derive(Debug, Clone, PartialEq)]
struct RunningElementCapture {
    fallback_text: String,
    content_parts: Vec<GeneratedContentPart>,
    element: Element,
    style: Box<ComputedStyle>,
    counter_set: CounterSet,
    quote_depth: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct ListMarker {
    text: String,
    image: Option<MarkerImage>,
    style: ComputedStyle,
    position: ListStylePosition,
    positioning_direction: Direction,
    suffix_space: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct MarkerImage {
    decoded: DecodedPngImage,
    width: f32,
    height: f32,
}

#[derive(Debug, Clone)]
struct InlineWord {
    text: String,
    style: ComputedStyle,
    baseline_shift: f32,
    link_target: Option<String>,
    mergeable: bool,
    source: InlineTextSource,
    hanging_edges: InlineHangingEdges,
}

#[derive(Debug, Clone)]
struct InlineFragment {
    text: String,
    style: ComputedStyle,
    baseline_shift: f32,
    link_target: Option<String>,
    mergeable: bool,
    source: InlineTextSource,
    generated_leader: bool,
    hanging_edges: InlineHangingEdges,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineTextSource {
    Normal,
    Marker,
}

/// Inline edge decorations that affect CSS Text hanging punctuation.
///
/// CSS Text prevents hanging punctuation when nonzero inline-axis padding or
/// border separates the punctuation from the line edge, even when that edge
/// belongs to an ancestor inline box rather than the text fragment itself:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct InlineHangingEdges {
    blocks_start: bool,
    blocks_end: bool,
}

#[derive(Debug, Clone)]
struct DefinitionListColumnItem<'a> {
    element: &'a Element,
    signature: ElementSignature,
    style: ComputedStyle,
    children: Option<&'a [box_tree::FormattingBox<'a>]>,
}

#[derive(Debug, Clone, Copy)]
struct InlineLineMetrics {
    width: f32,
    height: f32,
    baseline_offset: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct HangingPunctuationWidths {
    start: f32,
    end: f32,
}

/// A positioned inline line ready for painting.
///
/// CSS Inline Layout constructs a line box before painting its inline
/// fragments. This prepared line stores the resolved line metrics and ordered
/// paint items so text shaping, atom placement, backgrounds, links, and
/// decorations consume one reusable line artifact:
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
#[derive(Debug, Clone)]
struct PreparedInlineLine {
    metrics: InlineLineMetrics,
    paint_items: Vec<PreparedInlinePaintItem>,
}

/// A positioned inline paint item inside a prepared line box.
///
/// CSS painting observes source/line-box order: inline fragment backgrounds
/// are painted for each line fragment, shaped text groups are emitted on the
/// same baseline, and atomic inline boxes paint as indivisible margin boxes:
/// <https://www.w3.org/TR/CSS22/zindex.html> and
/// <https://www.w3.org/TR/css-inline-3/#model>.
#[derive(Debug, Clone)]
enum PreparedInlinePaintItem {
    FragmentBackground(PreparedInlineFragment),
    TextGroup(PreparedInlineTextGroup),
    Atom(PreparedInlineAtom),
}

/// A positioned inline text fragment with its line-fragment geometry.
///
/// CSS Backgrounds and Borders paints inline backgrounds and borders per
/// generated line fragment, while CSS Text may shape adjacent fragments as one
/// typographic context. Keeping both the original fragment and its used
/// geometry lets background/decor/link painting stay fragment-specific:
/// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break> and
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
#[derive(Debug, Clone)]
struct PreparedInlineFragment {
    fragment: InlineFragment,
    rect: PhysicalInlineRect,
}

/// A positioned atomic inline box with resolved content geometry.
///
/// CSS 2.2 treats inline-blocks, replaced elements, and similar atomic inline
/// boxes as a single inline-level box participating in the parent line box:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>.
#[derive(Debug, Clone)]
struct PreparedInlineAtom {
    atom: InlineAtom,
    content_rect: PhysicalInlineRect,
}

/// A shaped group of adjacent inline text fragments.
///
/// CSS Text requires boundary shaping to preserve cursive and complex-script
/// context across eligible inline boxes. This group stores the exact Parley
/// shaped runs selected before painting, including resolved font ids and
/// glyph advances used later by PDF text emission:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>,
/// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>, and
/// ISO 32000-2:2020, 9.10.3 "ToUnicode CMaps".
#[derive(Debug, Clone)]
struct PreparedInlineTextGroup {
    bounds: PhysicalInlineTextBounds,
    style: ComputedStyle,
    link_target: Option<String>,
    shaped: ShapedInlineLine,
}

impl PreparedInlineTextGroup {
    fn x(&self) -> f32 {
        self.bounds.x()
    }

    fn y(&self) -> f32 {
        self.bounds.y()
    }

    fn width(&self) -> f32 {
        self.bounds.width()
    }

    fn set_x(&mut self, x: f32) {
        self.bounds.set_x(x);
    }

    fn set_y(&mut self, y: f32) {
        self.bounds.set_y(y);
    }

    fn set_width(&mut self, width: f32) {
        self.bounds.set_width(width);
    }

    fn link_paint_rect(&self) -> PaintRect {
        self.bounds.link_paint_rect(self.style.font_size)
    }
}

/// Logical inline-axis geometry for one prepared line.
///
/// CSS Writing Modes defines inline layout in logical inline/block axes, while
/// PDF painting consumes physical coordinates. This geometry keeps CSS Text
/// alignment, indentation, and hanging punctuation in logical inline space
/// until each fragment, text group, or atomic inline box is converted to a
/// physical paint artifact:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-text-3/#text-align-property>.
#[derive(Debug, Clone, Copy)]
struct InlineLineGeometry {
    writing_mode: WritingMode,
    direction: Direction,
    inline_start: f32,
    inline_size: f32,
    block_start: f32,
}

/// Physical rectangle for an inline line-fragment paint item.
///
/// CSS Inline Layout first positions fragments in logical inline/block axes,
/// then CSS Writing Modes maps those fragments to physical coordinates. This
/// rectangle stores that resolved physical box in the current layout container
/// before it is projected into paint primitives:
/// <https://www.w3.org/TR/css-inline-3/#line-layout> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy)]
struct PhysicalInlineRect {
    rect: InlineRect,
}

impl PhysicalInlineRect {
    fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            rect: InlineRect::new(
                InlinePoint::new(x, y),
                InlineSize::new(width.max(0.0), height.max(0.0)),
            ),
        }
    }

    fn x(self) -> f32 {
        self.rect.origin.x
    }

    fn y(self) -> f32 {
        self.rect.origin.y
    }

    fn width(self) -> f32 {
        self.rect.size.width
    }

    fn height(self) -> f32 {
        self.rect.size.height
    }

    fn paint_rect(self) -> PaintRect {
        PaintRect::new(
            PaintPoint::new(self.x(), self.y()),
            PaintSize::new(self.width(), self.height()),
        )
    }

    fn paint_clip(self) -> PaintClip {
        PaintClip::from_paint_rect(self.paint_rect())
    }
}

/// Baseline-origin geometry for one prepared shaped inline text group.
///
/// CSS Inline Layout positions text by a baseline point and an inline advance,
/// not by a full border box. The baseline origin is stored in resolved physical
/// inline formatting coordinates so painting, decoration, and link annotation
/// code all project through one typed boundary:
/// <https://www.w3.org/TR/css-inline-3/#baseline-tables> and
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
#[derive(Debug, Clone, Copy)]
struct PhysicalInlineTextBounds {
    baseline_origin: InlinePoint,
    inline_size: f32,
}

impl PhysicalInlineTextBounds {
    fn new(x: f32, y: f32, inline_size: f32) -> Self {
        Self {
            baseline_origin: InlinePoint::new(x, y),
            inline_size: inline_size.max(0.0),
        }
    }

    fn x(self) -> f32 {
        self.baseline_origin.x
    }

    fn y(self) -> f32 {
        self.baseline_origin.y
    }

    fn width(self) -> f32 {
        self.inline_size
    }

    fn set_x(&mut self, x: f32) {
        self.baseline_origin.x = x;
    }

    fn set_y(&mut self, y: f32) {
        self.baseline_origin.y = y;
    }

    fn set_width(&mut self, width: f32) {
        self.inline_size = width.max(0.0);
    }

    fn text_origin(self) -> PaintPoint {
        PaintPoint::new(self.x(), self.y())
    }

    fn link_paint_rect(self, font_size: f32) -> PaintRect {
        paint_space_rect(self.x(), self.y() - 2.0, self.width(), font_size + 4.0)
    }
}

impl InlineLineGeometry {
    fn new(content_left: f32, cursor_y: f32, context: InlinePaintContext<'_>) -> Self {
        let style = context.block_style;
        let direction = context.direction;
        let inline_size = (context.available_width - context.line_indent).max(1.0);
        let content_inline_start = content_left + context.padding_left;
        let inline_start = match (style.writing_mode, direction) {
            (WritingMode::HorizontalTb, Direction::Ltr) => {
                content_inline_start + context.line_indent
            }
            (WritingMode::HorizontalTb, Direction::Rtl) => content_inline_start + inline_size,
            (_, Direction::Ltr) => cursor_y - context.line_indent,
            (_, Direction::Rtl) => cursor_y - inline_size,
        };
        let block_start = match style.writing_mode {
            WritingMode::HorizontalTb => cursor_y,
            WritingMode::VerticalRl | WritingMode::VerticalLr => content_inline_start,
        };
        Self {
            writing_mode: style.writing_mode,
            direction,
            inline_start,
            inline_size,
            block_start,
        }
    }

    fn alignment_offset(self, content_inline_size: f32, align: TextAlign) -> f32 {
        let free_space = (self.inline_size - content_inline_size).max(0.0);
        match align {
            TextAlign::Left if self.physical_left_is_inline_end() => free_space,
            TextAlign::Right if self.physical_right_is_inline_end() => free_space,
            TextAlign::Center => free_space / 2.0,
            TextAlign::End => free_space,
            TextAlign::Left
            | TextAlign::Right
            | TextAlign::Start
            | TextAlign::Justify
            | TextAlign::JustifyAll => 0.0,
        }
    }

    fn hanging_punctuation_offset(self, hanging_widths: HangingPunctuationWidths) -> f32 {
        match self.direction {
            Direction::Ltr => -hanging_widths.start,
            Direction::Rtl => hanging_widths.end,
        }
    }

    fn visual_line_origin(self, logical_inline_start: f32, line_inline_size: f32) -> f32 {
        self.physical_inline_origin(logical_inline_start, line_inline_size)
    }

    fn visual_line_item_rect(
        self,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
        inline_size: f32,
        horizontal_y: f32,
        block_size: f32,
    ) -> PhysicalInlineRect {
        match self.writing_mode {
            WritingMode::HorizontalTb => PhysicalInlineRect::new(
                line_physical_origin + visual_inline_start,
                horizontal_y,
                inline_size,
                block_size,
            ),
            WritingMode::VerticalRl | WritingMode::VerticalLr => PhysicalInlineRect::new(
                self.block_start,
                self.physical_inline_origin(
                    line_logical_inline_start + visual_inline_start,
                    inline_size,
                ),
                block_size,
                inline_size,
            ),
        }
    }

    fn position_visual_text_group(
        self,
        group: &mut PreparedInlineTextGroup,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
    ) {
        match self.writing_mode {
            WritingMode::HorizontalTb => {
                group.set_x(line_physical_origin + visual_inline_start);
            }
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                group.set_x(self.block_start);
                group.set_y(self.vertical_text_inline_origin(
                    line_logical_inline_start + visual_inline_start,
                    group.width(),
                ));
            }
        }
    }

    fn physical_inline_origin(self, logical_inline_start: f32, inline_size: f32) -> f32 {
        match inline_start_side(self.writing_mode, self.direction) {
            PhysicalSide::Left | PhysicalSide::Bottom => self.inline_start + logical_inline_start,
            PhysicalSide::Right | PhysicalSide::Top => {
                self.inline_start - logical_inline_start - inline_size
            }
        }
    }

    fn vertical_text_inline_origin(self, logical_inline_start: f32, inline_size: f32) -> f32 {
        let origin = self.physical_inline_origin(logical_inline_start, inline_size);
        if inline_start_side(self.writing_mode, self.direction) == PhysicalSide::Top {
            origin + inline_size
        } else {
            origin
        }
    }

    fn physical_left_is_inline_end(self) -> bool {
        inline_end_side(self.writing_mode, self.direction) == PhysicalSide::Left
    }

    fn physical_right_is_inline_end(self) -> bool {
        inline_end_side(self.writing_mode, self.direction) == PhysicalSide::Right
    }
}

#[derive(Debug, Clone, Copy)]
/// Shared inputs for laying out one inline paragraph run.
///
/// CSS Inline Layout forms line boxes from consecutive inline-level content
/// inside a block container; these values are the containing line box measure
/// and block style used by the word and mixed inline layout paths:
/// <https://www.w3.org/TR/css-inline-3/#line-layout>.
struct InlineParagraphContext<'a> {
    block_style: &'a ComputedStyle,
    stylesheets: &'a [Stylesheet],
    available_width: f32,
    padding_left: f32,
    hanging_indent: f32,
    hanging_punctuation_reserve: f32,
}

#[derive(Debug, Clone, Copy)]
struct InlinePaintContext<'a> {
    block_style: &'a ComputedStyle,
    direction: Direction,
    available_width: f32,
    padding_left: f32,
    line_indent: f32,
    text_align: TextAlign,
    is_first_line: bool,
}

#[derive(Debug, Clone)]
struct InlineAtom {
    content: InlineAtomContent,
    style: ComputedStyle,
    escaped_positioned_layers: Option<Box<[PositionedPaintLayer]>>,
    width: f32,
    height: f32,
    baseline_offset: f32,
    baseline_shift: f32,
    link_target: Option<String>,
    alt_text: Option<String>,
}

#[derive(Debug, Clone)]
struct InlineFloat {
    element: Element,
    signature: ElementSignature,
    style: ComputedStyle,
}

#[derive(Debug, Clone)]
enum InlineAtomContent {
    Canvas,
    Image(DecodedPngImage),
    Svg {
        fill: Color,
    },
    InlineBox {
        sequence: inline_layout::InlineLineSequence,
    },
    InlineFragment(PaintFragment),
    InlineEdge(InlineEdgeRole),
    Leader(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineEdgeRole {
    BoxEdge,
    TextAutospace,
}

impl InlineAtomContent {
    fn is_inline_edge(&self) -> bool {
        matches!(self, Self::InlineEdge(_))
    }

    fn is_box_edge(&self) -> bool {
        matches!(self, Self::InlineEdge(InlineEdgeRole::BoxEdge))
    }
}

#[derive(Debug, Clone)]
enum InlineItem {
    Word(Box<InlineWord>),
    Atom(Box<InlineAtom>),
    Float(Box<InlineFloat>),
    Break,
    PageScopeStart(Option<String>),
    PageScopeEnd,
}

impl AsRef<InlineItem> for InlineItem {
    fn as_ref(&self) -> &InlineItem {
        self
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum InlineLineItem {
    Fragment(InlineFragment),
    Atom(InlineAtom),
    Float(InlineFloat),
}

impl AsRef<InlineLineItem> for InlineLineItem {
    fn as_ref(&self) -> &InlineLineItem {
        self
    }
}

#[derive(Debug, Clone)]
struct LayoutSnapshot {
    pages: Vec<Page>,
    page_names: Vec<Option<String>>,
    page_blanks: Vec<bool>,
    page_name_scope_suppression: usize,
    page_name_element_scope_suppression: usize,
    page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    page_anchors: HashMap<String, usize>,
    page_anchor_text: HashMap<String, AnchorText>,
    document_canvas_background: Option<ComputedStyle>,
    root_canvas_background_defined: bool,
    current_page: Page,
    current_page_has_flow_content: bool,
    current_page_name: Option<String>,
    current_page_context: PageContext,
    cursor_y: f32,
    content_left: f32,
    content_right: f32,
    inline_static_baseline_y: Option<f32>,
    block_static_position_y_offset: Option<f32>,
    containing_block_writing_mode: WritingMode,
    fragment_top_offsets: Vec<f32>,
    definite_block_size_stack: Vec<Option<f32>>,
    truncate_page_start_margins: bool,
    avoid_inside_retry_depth: usize,
    out_of_flow_prebreak_suppression_depth: usize,
    containing_blocks: Vec<ContainingBlock>,
    list_stack: Vec<ListState>,
    counter_set: CounterSet,
    current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    current_page_running_elements: HashMap<String, Vec<NamedStringAssignment>>,
    next_assignment_id: usize,
    assignment_capture_stack: Vec<Vec<AssignmentId>>,
    quote_depth: usize,
    ancestors: Vec<ElementSignature>,
    bookmarks: Vec<Bookmark>,
    positioned_layers: Vec<PositionedPaintLayer>,
    fixed_layers: Vec<FixedPaintLayer>,
    next_paint_source_order: usize,
    next_float_id: usize,
    float_contexts: Vec<FloatContext>,
    pending_float_fragments: Vec<PendingFloatPaintFragment>,
    pending_float_side_effects: Vec<PendingFloatSideEffects>,
    applied_clearance_count: usize,
    preserve_scoped_paint_public_order: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AnchorText {
    content: String,
    before: String,
    after: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{ComputedLengthPercentage, Hyphens, TextAlignLast, TextOrientation};

    fn test_layout_builder<'a>(
        options: &'a RenderOptions,
        stylesheets: &'a [Stylesheet],
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
        LayoutBuilder::new(LayoutBuilderConfig {
            options,
            stylesheets,
            base_url: None,
            root_url: None,
            resource_cache,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            font_system: FontSystem::new(),
        })
    }

    fn inline_fragment(text: &str, style: ComputedStyle) -> InlineFragment {
        InlineFragment {
            text: text.to_string(),
            style,
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            generated_leader: false,
            hanging_edges: InlineHangingEdges::default(),
        }
    }

    fn inline_word(text: &str, style: &ComputedStyle) -> InlineItem {
        InlineItem::Word(Box::new(InlineWord {
            text: text.to_string(),
            style: style.clone(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
        }))
    }

    fn inline_box_edge(width: f32, style: &ComputedStyle) -> InlineItem {
        InlineItem::Atom(Box::new(InlineAtom {
            content: InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge),
            style: style.clone(),
            escaped_positioned_layers: None,
            width,
            height: style.line_height,
            baseline_offset: style.font_size,
            baseline_shift: 0.0,
            link_target: None,
            alt_text: None,
        }))
    }

    fn inline_test_atom(width: f32, style: &ComputedStyle) -> InlineItem {
        InlineItem::Atom(Box::new(InlineAtom {
            content: InlineAtomContent::InlineBox {
                sequence: empty_inline_sequence(),
            },
            style: style.clone(),
            escaped_positioned_layers: None,
            width,
            height: 0.0,
            baseline_offset: 0.0,
            baseline_shift: 0.0,
            link_target: None,
            alt_text: None,
        }))
    }

    fn inline_test_float(style: &ComputedStyle) -> InlineItem {
        let mut style = style.clone();
        style.float = Float::Left;
        let NodeKind::Element(element) = Node::element("span").kind else {
            unreachable!("element constructor should produce an element")
        };
        let signature = ElementSignature::new(element.tag.clone(), element.attrs.clone());
        InlineItem::Float(Box::new(InlineFloat {
            element,
            signature,
            style,
        }))
    }

    fn inline_leader(pattern: &str, style: &ComputedStyle) -> InlineItem {
        InlineItem::Atom(Box::new(InlineAtom {
            content: InlineAtomContent::Leader(pattern.to_string()),
            style: style.clone(),
            escaped_positioned_layers: None,
            width: 0.0,
            height: style.line_height,
            baseline_offset: style.font_size,
            baseline_shift: 0.0,
            link_target: Some("https://example.test/".to_string()),
            alt_text: None,
        }))
    }

    fn list_marker_text(text: &str, style: &ComputedStyle, suffix_space: bool) -> ListMarker {
        ListMarker {
            text: text.to_string(),
            image: None,
            style: style.clone(),
            position: ListStylePosition::Inside,
            positioning_direction: style.direction,
            suffix_space,
        }
    }

    fn list_marker_image(width: f32, height: f32, style: &ComputedStyle) -> ListMarker {
        ListMarker {
            text: String::new(),
            image: Some(MarkerImage {
                decoded: DecodedPngImage {
                    pixel_width: 1,
                    pixel_height: 1,
                    rgb: vec![0, 0, 0],
                    alpha: None,
                },
                width,
                height,
            }),
            style: style.clone(),
            position: ListStylePosition::Inside,
            positioning_direction: style.direction,
            suffix_space: true,
        }
    }

    fn empty_inline_sequence() -> inline_layout::InlineLineSequence {
        inline_layout::InlineLineSequence {
            records: Vec::new(),
            available_width: 0.0,
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
        }
    }

    fn inline_item_boundary_roles(items: &[InlineItem]) -> Vec<InlineBoundaryRole> {
        items.iter().map(inline_item_boundary_role).collect()
    }

    fn normalized_inline_item_text(items: &mut Vec<InlineItem>) -> String {
        inline_collect::normalize_inline_whitespace_items(items);
        items
            .iter()
            .map(|item| match item {
                InlineItem::Word(word) => word.text.clone(),
                InlineItem::Break => "|".to_string(),
                InlineItem::Atom(_) => "\u{fffc}".to_string(),
                InlineItem::Float(_) | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {
                    String::new()
                }
            })
            .collect()
    }

    fn normalized_inline_word_text(items: &mut Vec<InlineItem>) -> String {
        inline_collect::normalize_inline_whitespace_items(items);
        items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn raw_text_sequence(
        builder: &mut LayoutBuilder<'_>,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> inline_layout::InlineLineSequence {
        builder.inline_line_sequence_for_raw_inline_text(text, style, available_width, 0.0, None)
    }

    fn sequence_fragment_texts(sequence: &inline_layout::InlineLineSequence) -> Vec<String> {
        sequence
            .records
            .iter()
            .map(|record| {
                record
                    .fragment
                    .as_ref()
                    .map(|fragment| fragment.text.clone())
                    .unwrap_or_default()
            })
            .collect()
    }

    fn first_sequence_line_width(sequence: &inline_layout::InlineLineSequence) -> f32 {
        sequence.records[0]
            .fragment
            .as_ref()
            .expect("first selected line should carry a fragment")
            .metrics
            .width
    }

    #[test]
    fn inline_boundary_policy_classifies_text_transparent_and_opaque_boundaries() {
        let style = ComputedStyle::initial();
        let bidi_control = InlineItem::Word(Box::new(InlineWord {
            text: "\u{2067}".to_string(),
            style: style.clone(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
        }));

        assert_eq!(
            inline_item_boundary_role(&inline_word("text", &style)),
            InlineBoundaryRole::Text
        );
        assert_eq!(
            inline_item_boundary_role(&bidi_control),
            InlineBoundaryRole::TransparentTextBoundary
        );
        assert_eq!(
            inline_item_boundary_role(&inline_box_edge(0.0, &style)),
            InlineBoundaryRole::TransparentTextBoundary
        );
        assert_eq!(
            inline_item_boundary_role(&InlineItem::PageScopeStart(Some("chapter".to_string()))),
            InlineBoundaryRole::PageScopeStart
        );
        assert_eq!(
            inline_item_boundary_role(&inline_test_atom(5.0, &style)),
            InlineBoundaryRole::IndependentFormattingContext
        );
        assert_eq!(
            inline_atom_boundary_role(&InlineAtomContent::Leader(".".to_string())),
            InlineBoundaryRole::OpaqueAtomic
        );
        assert!(InlineBoundaryRole::PageScopeStart.is_transparent_to_whitespace());
        assert!(InlineBoundaryRole::OpaqueAtomic.resets_text_context());
    }

    #[test]
    fn whitespace_normalization_uses_boundary_policy_for_transparent_boundaries() {
        let style = ComputedStyle::initial();
        let mut latin_items = vec![
            inline_word("A\n", &style),
            InlineItem::PageScopeStart(Some("chapter".to_string())),
            inline_box_edge(0.0, &style),
            InlineItem::PageScopeEnd,
            inline_word("B", &style),
        ];
        let mut cjk_items = vec![
            inline_word("中\n", &style),
            InlineItem::PageScopeStart(Some("chapter".to_string())),
            inline_box_edge(0.0, &style),
            InlineItem::PageScopeEnd,
            inline_word("文", &style),
        ];

        assert_eq!(normalized_inline_word_text(&mut latin_items), "A B");
        assert_eq!(normalized_inline_word_text(&mut cjk_items), "中文");
    }

    #[test]
    fn whitespace_normalization_resets_context_at_opaque_atomic_boundaries() {
        let style = ComputedStyle::initial();
        let mut items = vec![
            inline_word("A\n", &style),
            inline_test_atom(5.0, &style),
            inline_word("  B", &style),
        ];

        assert_eq!(
            normalized_inline_item_text(&mut items),
            format!("A \u{fffc} B")
        );
    }

    #[tokio::test]
    async fn inline_line_sequence_splits_at_page_scope_boundaries_without_graphing_controls() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("alpha", &style),
            InlineItem::PageScopeStart(Some("chapter".to_string())),
            inline_word("beta", &style),
            InlineItem::PageScopeEnd,
            inline_word("gamma", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 400.0, 0.0, 0.0);

        assert_eq!(
            sequence_fragment_texts(&sequence),
            vec!["alpha", "beta", "gamma"]
        );
        assert_eq!(sequence.records[0].paragraph_index, 0);
        assert_eq!(sequence.records[1].paragraph_index, 1);
        assert_eq!(sequence.records[2].paragraph_index, 2);
    }

    fn prepared_visual_texts_for_sequence(
        builder: &mut LayoutBuilder<'_>,
        sequence: &inline_layout::InlineLineSequence,
        style: &ComputedStyle,
    ) -> Vec<String> {
        let context = inline_paragraph_context(style, sequence.available_width);
        let mut plaintext_state = None;
        sequence
            .records
            .iter()
            .filter_map(|record| {
                builder.prepare_inline_line_record(record, context, &mut plaintext_state)
            })
            .flat_map(|prepared| {
                prepared_text_groups(&prepared)
                    .into_iter()
                    .map(|group| group.shaped.text.clone())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[tokio::test]
    async fn wraps_at_word_boundaries() {
        assert_eq!(
            wrap_text("one two three", 7),
            vec!["one two".to_string(), "three".to_string()]
        );
        assert_eq!(wrap_text("Parent Child", 4), vec!["Parent", "Child"]);
    }

    #[tokio::test]
    async fn production_sequence_wraps_text_with_shaped_widths() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.4;
        let available_width = builder.font_system.measure_text("one two", &style) + 0.1;
        assert!(builder.font_system.measure_text("one two three", &style) > available_width);

        let sequence = raw_text_sequence(&mut builder, "one two three", &style, available_width);

        assert_eq!(sequence_fragment_texts(&sequence), vec!["one two", "three"]);
        let prepared = prepared_visual_texts_for_sequence(&mut builder, &sequence, &style);
        assert_eq!(prepared, vec!["one two", "three"]);
    }

    #[tokio::test]
    async fn production_sequence_wraps_break_spaces_and_preserves_trailing_advance() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.4;
        style.white_space = WhiteSpace::BreakSpaces;
        let available_width = builder.font_system.measure_text("A  ", &style) + 0.1;
        assert!(builder.font_system.measure_text("A   ", &style) > available_width);

        let wrapped = raw_text_sequence(&mut builder, "A   B", &style, available_width);
        let unwrapped = raw_text_sequence(&mut builder, "A  ", &style, 100.0);

        assert_eq!(sequence_fragment_texts(&wrapped).concat(), "A   B");
        assert!(wrapped.records.len() > 1);
        assert_eq!(sequence_fragment_texts(&unwrapped), vec!["A  "]);
        assert!(
            first_sequence_line_width(&unwrapped) > builder.font_system.measure_text("A", &style)
        );
    }

    #[tokio::test]
    async fn production_sequence_keeps_css_text_hanging_width_effects() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut normal = ComputedStyle::initial();
        normal.font_family = css::FontFamily::SansSerif;
        normal.font_size = 12.0;
        normal.line_height = 14.4;
        let available_width = builder.font_system.measure_text("X", &normal) + 0.1;

        let normal_sequence =
            raw_text_sequence(&mut builder, "X\u{3000}", &normal, available_width);
        assert_eq!(sequence_fragment_texts(&normal_sequence), vec!["X\u{3000}"]);
        assert!(
            (first_sequence_line_width(&normal_sequence)
                - builder.font_system.measure_text("X", &normal))
            .abs()
                < 0.01
        );

        let mut break_spaces = normal.clone();
        break_spaces.white_space = WhiteSpace::BreakSpaces;
        let break_spaces_sequence =
            raw_text_sequence(&mut builder, "X\u{3000}", &break_spaces, 500.0);
        assert_eq!(
            sequence_fragment_texts(&break_spaces_sequence),
            vec!["X\u{3000}"]
        );
        assert!(
            (first_sequence_line_width(&break_spaces_sequence)
                - builder.font_system.measure_text("X\u{3000}", &break_spaces))
            .abs()
                < 0.01
        );
    }

    #[tokio::test]
    async fn production_prepared_lines_apply_bidi_visual_order() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.cursor_y = 100.0;
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.4;

        let ltr_sequence = raw_text_sequence(&mut builder, "abc אבג def", &style, 500.0);
        assert_eq!(
            prepared_visual_texts_for_sequence(&mut builder, &ltr_sequence, &style),
            vec!["abc גבא def"]
        );

        style.direction = Direction::Rtl;
        style.unicode_bidi = UnicodeBidi::BidiOverride;
        let override_sequence = raw_text_sequence(&mut builder, "abc def", &style, 500.0);
        let visual = prepared_visual_texts_for_sequence(&mut builder, &override_sequence, &style);
        assert_eq!(visual, vec!["fed cba"]);
        assert!(
            visual
                .iter()
                .all(|text| !text.chars().any(character_is_bidi_format_control))
        );
    }

    #[tokio::test]
    async fn production_sequence_uses_css_text_emergency_break_controls() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut normal = ComputedStyle::initial();
        normal.font_family = css::FontFamily::SansSerif;
        normal.font_size = 12.0;
        normal.line_height = 14.4;
        let available_width = builder.font_system.measure_text("abc", &normal) + 0.1;

        let normal_sequence = raw_text_sequence(&mut builder, "abcdefgh", &normal, available_width);
        assert_eq!(normal_sequence.records.len(), 1);

        let mut anywhere = normal.clone();
        anywhere.overflow_wrap = css::OverflowWrap::Anywhere;
        let anywhere_sequence =
            raw_text_sequence(&mut builder, "abcdefgh", &anywhere, available_width);
        assert!(anywhere_sequence.records.len() > 1);
        assert_eq!(
            sequence_fragment_texts(&anywhere_sequence).concat(),
            "abcdefgh"
        );

        let mut break_all = normal.clone();
        break_all.word_break = css::WordBreak::BreakAll;
        let break_all_sequence =
            raw_text_sequence(&mut builder, "abcdefgh", &break_all, available_width);
        assert!(break_all_sequence.records.len() > 1);
        assert_eq!(
            sequence_fragment_texts(&break_all_sequence).concat(),
            "abcdefgh"
        );

        let mut line_break_anywhere = normal.clone();
        line_break_anywhere.line_break = css::LineBreak::Anywhere;
        let anywhere_line_break_sequence = raw_text_sequence(
            &mut builder,
            "abcdefgh",
            &line_break_anywhere,
            available_width,
        );
        assert!(anywhere_line_break_sequence.records.len() > 1);
        assert_eq!(
            sequence_fragment_texts(&anywhere_line_break_sequence).concat(),
            "abcdefgh"
        );

        line_break_anywhere.white_space = WhiteSpace::Pre;
        assert_eq!(
            raw_text_sequence(&mut builder, " XXX", &line_break_anywhere, available_width)
                .records
                .len(),
            1
        );
        line_break_anywhere.white_space = WhiteSpace::NoWrap;
        assert_eq!(
            raw_text_sequence(
                &mut builder,
                "XXXX XX",
                &line_break_anywhere,
                available_width
            )
            .records
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn production_sequence_handles_soft_hyphen_and_auto_hyphenation() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.4;

        let unbroken = raw_text_sequence(&mut builder, "hyphen\u{00ad}ation", &style, 500.0);
        assert_eq!(sequence_fragment_texts(&unbroken), vec!["hyphenation"]);

        let available_width = builder.font_system.measure_text("hyphen", &style) + 0.1;
        let broken =
            raw_text_sequence(&mut builder, "hyphen\u{00ad}ation", &style, available_width);
        assert_eq!(sequence_fragment_texts(&broken).concat(), "hyphen-ation");

        style.hyphens = Hyphens::None;
        let suppressed =
            raw_text_sequence(&mut builder, "hyphen\u{00ad}ation", &style, available_width);
        assert_eq!(sequence_fragment_texts(&suppressed), vec!["hyphenation"]);

        let mut auto = style.clone();
        auto.hyphens = Hyphens::Auto;
        auto.language = Some("en".to_string());
        let auto_available_width = builder.font_system.measure_text("ribo", &auto) + 0.1;
        let auto_sequence =
            raw_text_sequence(&mut builder, "ribonuclease", &auto, auto_available_width);
        assert!(auto_sequence.records.len() > 1);
        assert!(
            sequence_fragment_texts(&auto_sequence)
                .iter()
                .any(|text| text.ends_with('-'))
        );
        assert_eq!(
            sequence_fragment_texts(&auto_sequence)
                .iter()
                .map(|text| text.replace('-', ""))
                .collect::<String>(),
            "ribonuclease"
        );

        auto.language = None;
        let unknown_language =
            raw_text_sequence(&mut builder, "ribonuclease", &auto, auto_available_width);
        assert_eq!(
            sequence_fragment_texts(&unknown_language),
            vec!["ribonuclease"]
        );
    }

    #[tokio::test]
    async fn production_sequence_handles_break_spaces_with_break_all() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::BreakSpaces;
        style.word_break = css::WordBreak::BreakAll;
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;

        let available_width = builder.font_system.measure_text("  A", &style) + 0.1;
        let sequence = raw_text_sequence(&mut builder, "  AB", &style, available_width);
        assert_eq!(sequence_fragment_texts(&sequence).concat(), "  AB");
        assert!(sequence.records.len() > 1);

        let available_width = builder.font_system.measure_text("X XX", &style) + 0.1;
        let sequence = raw_text_sequence(&mut builder, "X XX X", &style, available_width);
        assert_eq!(sequence_fragment_texts(&sequence).concat(), "X XX X");
        assert!(sequence.records.len() > 1);

        style.line_break = css::LineBreak::Anywhere;
        let sequence = raw_text_sequence(&mut builder, "X XX X", &style, available_width);
        assert_eq!(sequence_fragment_texts(&sequence).concat(), "X XX X");
        assert!(sequence.records.len() > 1);
    }

    #[tokio::test]
    async fn css_whitespace_normalization_preserves_ideographic_spaces() {
        assert_eq!(
            collapse_whitespace("\u{3000}\u{3000}XX"),
            "\u{3000}\u{3000}XX"
        );
        assert_eq!(
            normalize_inline_text("\u{3000}\u{3000}XX"),
            "\u{3000}\u{3000}XX"
        );
        assert_eq!(normalize_inline_text("  XX  "), "XX");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_collapses_across_transparent_edges() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut items = vec![
            inline_word("A ", &style),
            inline_box_edge(1.0, &style),
            inline_word("  B", &style),
        ];

        assert_eq!(normalized_inline_item_text(&mut items), "A \u{fffc}B");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_treats_page_scopes_as_text_transparent() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut spaced = vec![
            inline_word("A ", &style),
            InlineItem::PageScopeStart(Some("named".to_string())),
            InlineItem::PageScopeEnd,
            inline_word("  B", &style),
        ];
        assert_eq!(normalized_inline_item_text(&mut spaced), "A B");

        let mut cjk = vec![
            inline_word("中\n", &style),
            InlineItem::PageScopeStart(Some("named".to_string())),
            InlineItem::PageScopeEnd,
            inline_word("文", &style),
        ];
        assert_eq!(normalized_inline_item_text(&mut cjk), "中文");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_resets_across_real_atoms() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut items = vec![
            inline_word("A ", &style),
            inline_test_atom(4.0, &style),
            inline_word(" B", &style),
        ];

        assert_eq!(normalized_inline_item_text(&mut items), "A \u{fffc} B");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_handles_pre_line_and_preserved_modes() {
        let mut pre_line = ComputedStyle::initial();
        pre_line.font_family = css::FontFamily::SansSerif;
        pre_line.white_space = WhiteSpace::PreLine;
        let mut pre_line_items = vec![inline_word("A   B\nC", &pre_line)];
        assert_eq!(normalized_inline_item_text(&mut pre_line_items), "A B|C");
        let mut consecutive_pre_line_items = vec![inline_word("A\n\nB", &pre_line)];
        assert_eq!(
            normalized_inline_item_text(&mut consecutive_pre_line_items),
            "A||B"
        );

        let mut pre_wrap = pre_line.clone();
        pre_wrap.white_space = WhiteSpace::PreWrap;
        let mut pre_wrap_items = vec![inline_word("A\t B\n", &pre_wrap)];
        assert_eq!(normalized_inline_item_text(&mut pre_wrap_items), "A\t B");

        let mut break_spaces = pre_line.clone();
        break_spaces.white_space = WhiteSpace::BreakSpaces;
        let mut break_spaces_items = vec![inline_word("A  ", &break_spaces)];
        inline_collect::normalize_inline_whitespace_items(&mut break_spaces_items);
        assert_eq!(
            break_spaces_items
                .iter()
                .filter(|item| matches!(item, InlineItem::Word(_)))
                .count(),
            3
        );
    }

    #[tokio::test]
    async fn inline_whitespace_processor_replaces_visible_controls() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut items = vec![inline_word("A\u{0099}B", &style)];

        assert_eq!(normalized_inline_item_text(&mut items), "A\u{fffd}B");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_transforms_segment_breaks_by_context() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut cjk = vec![inline_word("中文\n中文", &style)];
        let mut mixed = vec![inline_word("中文\nenglish", &style)];
        let mut latin = vec![inline_word("word\nword", &style)];

        assert_eq!(normalized_inline_item_text(&mut cjk), "中文中文");
        assert_eq!(normalized_inline_item_text(&mut mixed), "中文 english");
        assert_eq!(normalized_inline_item_text(&mut latin), "word word");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_keeps_bidi_controls_transparent() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut items = vec![
            inline_word("中\n", &style),
            inline_word("\u{2066}", &style),
            inline_word("文", &style),
        ];

        assert_eq!(normalized_inline_item_text(&mut items), "中\u{2066}文");
    }

    #[tokio::test]
    async fn inside_marker_text_participates_in_shared_whitespace_context() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let marker = list_marker_text("中\n", &style, false);
        let mut items = Vec::new();

        builder.push_inside_marker_items(&marker, &style, None, &mut items);
        items.push(inline_word("文", &style));

        assert_eq!(
            inline_item_boundary_roles(&items),
            vec![InlineBoundaryRole::Text, InlineBoundaryRole::Text]
        );
        assert_eq!(normalized_inline_item_text(&mut items), "中文");
    }

    #[tokio::test]
    async fn inside_marker_image_resets_whitespace_context_as_atomic_boundary() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let marker = list_marker_image(6.0, 6.0, &style);
        let mut items = Vec::new();

        builder.push_inside_marker_items(&marker, &style, None, &mut items);
        items.push(inline_word(" B", &style));

        assert_eq!(
            inline_item_boundary_roles(&items),
            vec![
                InlineBoundaryRole::OpaqueAtomic,
                InlineBoundaryRole::Text,
                InlineBoundaryRole::Text
            ]
        );
        assert_eq!(normalized_inline_item_text(&mut items), "\u{fffc} B");
    }

    #[tokio::test]
    async fn generated_marker_forced_breaks_survive_sequence_records() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        style.white_space = WhiteSpace::PreLine;
        let marker = list_marker_text("M\n\n", &style, false);
        let mut items = Vec::new();

        builder.push_inside_marker_items(&marker, &style, None, &mut items);
        items.push(inline_word("B", &style));
        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence_fragment_texts(&sequence), vec!["M", "", "B"]);
        assert!(sequence.records[1].is_forced_empty);
        assert!(sequence.records[1].fragment.is_none());
    }

    #[tokio::test]
    async fn bidi_controls_around_forced_breaks_stay_invisible_in_prepared_lines() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.cursor_y = 100.0;
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.direction = Direction::Rtl;
        style.unicode_bidi = UnicodeBidi::BidiOverride;
        let mut items = Vec::new();

        builder.push_bidi_scope_start(&style, None, 0.0, &mut items);
        items.push(inline_word("abc", &style));
        items.push(InlineItem::Break);
        items.push(inline_word("def", &style));
        builder.push_bidi_scope_end(&style, None, 0.0, &mut items);

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);
        let visual = prepared_visual_texts_for_sequence(&mut builder, &sequence, &style);

        assert_eq!(
            sequence_fragment_texts(&sequence),
            vec!["\u{202e}abc", "def\u{202c}"]
        );
        assert_eq!(visual, vec!["cba", "fed"]);
        assert!(
            visual
                .iter()
                .all(|text| !text.chars().any(character_is_bidi_format_control))
        );
    }

    #[tokio::test]
    async fn plaintext_alignment_resolves_per_forced_sequence_line() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.cursor_y = 100.0;
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Start;
        style.unicode_bidi = UnicodeBidi::Plaintext;
        style.direction = Direction::Ltr;
        let items = vec![
            inline_word("אב", &style),
            InlineItem::Break,
            inline_word("abc", &style),
        ];
        let sequence = builder.collect_inline_line_sequence(items, &style, 120.0, 0.0, 0.0);
        let context = inline_paragraph_context(&style, 120.0);
        let mut plaintext_state = None;

        let rtl_prepared = builder
            .prepare_inline_line_record(&sequence.records[0], context, &mut plaintext_state)
            .expect("rtl plaintext line should prepare");
        let ltr_prepared = builder
            .prepare_inline_line_record(&sequence.records[1], context, &mut plaintext_state)
            .expect("ltr plaintext line should prepare");
        let rtl_group = prepared_text_groups(&rtl_prepared)[0];
        let ltr_group = prepared_text_groups(&ltr_prepared)[0];

        assert_eq!(sequence_fragment_texts(&sequence), vec!["אב", "abc"]);
        assert_eq!(plaintext_state, Some(Direction::Ltr));
        assert!(rtl_group.x() > ltr_group.x() + 60.0);
        assert_eq!(sequence.records[0].fragment.as_ref().unwrap().text, "אב");
        assert_eq!(sequence.records[1].fragment.as_ref().unwrap().text, "abc");
    }

    #[tokio::test]
    async fn inline_line_sequence_keeps_forced_empty_lines_as_records() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        let items = vec![
            inline_word("A", &style),
            InlineItem::Break,
            InlineItem::Break,
            inline_word("B", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 3);
        assert!(sequence.records[1].fragment.is_none());
        assert!(sequence.records[1].is_forced_empty);
        assert_eq!(sequence.records[1].paragraph_index, 1);
        assert_eq!(sequence.records[1].block_line_index, 1);
        assert_eq!(sequence.total_height(), 30.0);
    }

    #[tokio::test]
    async fn zero_advance_inline_box_edge_creates_line_with_owner_line_height() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 100.0;
        let items = vec![inline_box_edge(0.0, &style)];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 1);
        assert!((sequence.total_height() - 100.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn inline_line_sequence_fitting_applies_orphans_and_widows() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        let items = vec![
            inline_word("A", &style),
            InlineItem::Break,
            inline_word("B", &style),
            InlineItem::Break,
            inline_word("C", &style),
            InlineItem::Break,
            inline_word("D", &style),
            InlineItem::Break,
            inline_word("E", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 5);
        assert_eq!(sequence.fitting_line_count(0, 25.0, false, 2, 2), 2);
        assert_eq!(sequence.fitting_line_count(0, 35.0, false, 2, 3), 2);
        assert_eq!(sequence.fitting_line_count(0, 5.0, false, 2, 2), 0);
        assert_eq!(sequence.fitting_line_count(0, 5.0, true, 2, 2), 1);
    }

    #[tokio::test]
    async fn inline_line_sequence_flags_are_paragraph_local() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        let items = vec![
            inline_word("A", &style),
            InlineItem::Break,
            inline_word("B", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 2);
        assert_eq!(sequence.records[0].paragraph_index, 0);
        assert_eq!(sequence.records[0].paragraph_line_index, 0);
        assert!(sequence.records[0].is_first_formatted_line);
        assert!(sequence.records[0].is_last_line_in_paragraph);
        assert_eq!(sequence.records[1].paragraph_index, 1);
        assert_eq!(sequence.records[1].paragraph_line_index, 0);
        assert!(!sequence.records[1].is_first_formatted_line);
        assert!(sequence.records[1].is_last_line_in_paragraph);
    }

    #[tokio::test]
    async fn inline_line_sequence_shares_paragraph_last_hanging_width() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        style.hanging_punctuation.force_end = true;
        let items = vec![inline_word("Alpha beta gamma.", &style)];

        let sequence = builder.collect_inline_line_sequence(items, &style, 34.0, 0.0, 0.0);

        assert!(sequence.records.len() > 1);
        let paragraph_width = sequence.records[0].paragraph_last_hanging_width;
        assert!(paragraph_width > 0.0);
        assert!(
            sequence
                .records
                .iter()
                .all(|record| (record.paragraph_last_hanging_width - paragraph_width).abs() < 0.01)
        );
    }

    #[tokio::test]
    async fn inline_line_sequence_keeps_plaintext_bidi_logical_text() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        style.unicode_bidi = css::UnicodeBidi::Plaintext;
        let items = vec![inline_word("אבג", &style)];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        let fragment = sequence.records[0].fragment.as_ref().unwrap();
        assert_eq!(fragment.text, "אבג");
    }

    #[tokio::test]
    async fn inline_box_padding_breaks_boundary_shaping() {
        let mut left_style = ComputedStyle::initial();
        left_style.display = Display::BLOCK;
        left_style.padding.left = 10.0;
        let left = InlineFragment {
            text: "ع".to_string(),
            style: left_style,
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            generated_leader: false,
            hanging_edges: InlineHangingEdges::default(),
        };
        let mut right = left.clone();
        right.style = ComputedStyle::initial();

        assert!(can_shape_inline_fragments_together(&left, &right));

        right.style.padding.left = 1.0;
        assert!(!can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn inline_box_margin_breaks_boundary_shaping() {
        let left = InlineFragment {
            text: "ع".to_string(),
            style: ComputedStyle::initial(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            generated_leader: false,
            hanging_edges: InlineHangingEdges::default(),
        };
        let mut right = left.clone();

        assert!(can_shape_inline_fragments_together(&left, &right));

        right.style.margin.left = 1.0;
        assert!(!can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn inline_box_used_border_breaks_boundary_shaping() {
        let left = InlineFragment {
            text: "ع".to_string(),
            style: ComputedStyle::initial(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            generated_leader: false,
            hanging_edges: InlineHangingEdges::default(),
        };
        let mut right = left.clone();
        right.style.border_widths.left = 1.0;
        right.style.border_styles.left = BorderStyle::None;

        assert!(can_shape_inline_fragments_together(&left, &right));

        right.style.border_styles.left = BorderStyle::Solid;
        assert!(!can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn font_style_change_does_not_break_boundary_shaping() {
        let left = InlineFragment {
            text: "ع".to_string(),
            style: ComputedStyle::initial(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            generated_leader: false,
            hanging_edges: InlineHangingEdges::default(),
        };
        let mut right = left.clone();
        right.style.font_style = css::FontStyle::Italic;

        assert!(can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn tatweel_only_inline_fragments_preserve_shaping_group() {
        let left = InlineFragment {
            text: "\u{0640}".to_string(),
            style: ComputedStyle::initial(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            generated_leader: false,
            hanging_edges: InlineHangingEdges::default(),
        };
        let mut right = left.clone();
        right.text = "ب".to_string();
        right.style.font_family = css::FontFamily::Serif;

        assert!(can_shape_inline_fragments_together(&left, &right));
        assert!(inline_fragment_is_arabic_tatweel_only(&left));
        assert!(can_queue_inline_fragments_for_shaping(&left, &right));
    }

    #[tokio::test]
    async fn direction_change_alone_does_not_break_boundary_shaping() {
        let mut left_style = ComputedStyle::initial();
        left_style.direction = Direction::Rtl;
        let left = InlineFragment {
            text: "ع".to_string(),
            style: left_style,
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            generated_leader: false,
            hanging_edges: InlineHangingEdges::default(),
        };
        let mut right = left.clone();
        right.style.direction = Direction::Ltr;

        assert!(can_shape_inline_fragments_together(&left, &right));

        right.style.unicode_bidi = css::UnicodeBidi::Isolate;
        assert!(!can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn table_cell_anonymous_inline_text_uses_baseline_vertical_align() {
        let mut cell_style = ComputedStyle::initial();
        cell_style.display = Display::TABLE_CELL;
        cell_style.vertical_align =
            VerticalAlign::BASELINE.with_table_cell_align(TableCellVerticalAlign::Middle);
        cell_style.unicode_bidi = css::UnicodeBidi::Isolate;

        let normalized = normalized_anonymous_inline_content_style(&cell_style);

        assert_eq!(normalized.vertical_align, VerticalAlign::BASELINE);
        assert_eq!(normalized.unicode_bidi, css::UnicodeBidi::Normal);
    }

    #[tokio::test]
    async fn join_control_inline_fragments_do_not_break_boundary_shaping() {
        let mut left_style = ComputedStyle::initial();
        left_style.font_family = css::FontFamily::SansSerif;
        let left = InlineFragment {
            text: "ع".to_string(),
            style: left_style,
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            generated_leader: false,
            hanging_edges: InlineHangingEdges::default(),
        };
        let mut joiner = left.clone();
        joiner.text = "\u{200c}".to_string();
        joiner.style.font_family = css::FontFamily::Serif;

        assert!(inline_fragment_is_join_control_only(&joiner));
        assert!(can_shape_inline_fragments_together(&left, &joiner));

        joiner.style.padding.left = 1.0;
        assert!(can_shape_inline_fragments_together(&left, &joiner));
        let mut visible_right = joiner.clone();
        visible_right.text = "ب".to_string();
        assert!(!can_shape_inline_fragments_together(&left, &visible_right));
    }

    #[tokio::test]
    async fn prepared_inline_text_group_preserves_shaped_runs() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;

        let fragments = vec![
            inline_fragment("A", style.clone()),
            inline_fragment("B", style.clone()),
        ];
        let group = builder
            .prepare_inline_text_group(&fragments, 12.0)
            .expect("text group should shape");

        assert_eq!(group.x(), 12.0);
        assert_eq!(group.shaped.text, "AB");
        assert!(group.shaped.first_font_id().is_some());
        assert!((group.width() - group.shaped.advance_width()).abs() < 0.01);
        assert!(
            group
                .shaped
                .runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .any(|glyph| glyph.paints && !glyph.source_text.is_empty())
        );
    }

    #[tokio::test]
    async fn prepared_inline_text_group_keeps_join_controls_out_of_paint() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;

        let fragments = vec![
            inline_fragment("ع", style.clone()),
            inline_fragment("\u{200d}", style.clone()),
            inline_fragment("ب", style.clone()),
        ];
        let group = builder
            .prepare_inline_text_group(&fragments, 0.0)
            .expect("join-control group should shape");

        assert!(group.shaped.text.contains('\u{200d}'));
        assert!(
            group
                .shaped
                .rendered_runs()
                .iter()
                .flat_map(|run| run.glyphs.iter().flatten())
                .all(|glyph| !glyph.unicode.chars().any(character_is_join_control)),
            "{:?}",
            group.shaped
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_shapes_across_styled_tatweel_fragment() {
        let stylesheet = css::parse_stylesheet(
            &crate::css::Css::from_string(
                r#"@font-face {
                    font-family: AlreqNaskh;
                    src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
                }
                @font-face {
                    font-family: AlreqTatweel;
                    src: url("tests/resources/fonts/Scheherazade-Regular.woff");
                }"#,
            )
            .with_base_url(Some(std::path::PathBuf::from("."))),
        );
        let font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&[stylesheet])
            .finish()
            .await;
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = LayoutBuilder::new(LayoutBuilderConfig {
            options: &options,
            stylesheets: &stylesheets,
            base_url: None,
            root_url: None,
            resource_cache: &resource_cache,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            font_system,
        });
        builder.cursor_y = 100.0;

        let mut arabic = ComputedStyle::initial();
        arabic.font_family = css::FontFamily::Names(vec!["AlreqNaskh".to_string()]);
        arabic.font_size = 20.0;
        arabic.line_height = 24.0;
        arabic.direction = Direction::Rtl;
        let mut tatweel = arabic.clone();
        tatweel.font_family = css::FontFamily::Names(vec!["AlreqTatweel".to_string()]);

        let isolated_beh = builder
            .font_system
            .shape_unwrapped_line("\u{0628}", &arabic, arabic.line_height)
            .expect("isolated beh should shape")
            .runs
            .into_iter()
            .flat_map(|run| run.glyphs)
            .find(|glyph| glyph.source_text == "\u{0628}")
            .expect("isolated beh glyph")
            .rendered
            .id;
        let beh_fragment = inline_fragment("\u{0628}", arabic.clone());
        let tatweel_fragment = inline_fragment("\u{0640}", tatweel.clone());
        let beh_width = builder.font_system.measure_text("\u{0628}", &arabic);
        let tatweel_width = builder.font_system.measure_text("\u{0640}", &tatweel);
        let line_fragment = inline_layout::InlineLineFragment {
            items: vec![
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(beh_fragment),
                    width: beh_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(tatweel_fragment),
                    width: tatweel_width,
                    shaped: None,
                },
            ],
            metrics: InlineLineMetrics {
                width: beh_width + tatweel_width,
                height: arabic.line_height,
                baseline_offset: arabic.font_size,
            },
            hanging_widths: HangingPunctuationWidths::default(),
            indent: 0.0,
            available_width: 200.0,
            suppress_float_adjust: false,
            text: "\u{0628}\u{0640}".to_string(),
        };

        let prepared = builder
            .prepare_inline_line_fragment(
                &line_fragment,
                InlinePaintContext {
                    block_style: &arabic,
                    direction: Direction::Rtl,
                    available_width: 200.0,
                    padding_left: 0.0,
                    line_indent: 0.0,
                    text_align: TextAlign::Left,
                    is_first_line: true,
                },
            )
            .expect("prepared line");
        let groups = prepared_text_groups(&prepared);

        assert_eq!(groups.len(), 1, "{prepared:?}");
        let group = groups[0];
        let beh_glyph = group
            .shaped
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .find(|glyph| glyph.source_text == "\u{0628}")
            .expect("joined beh glyph");
        assert_ne!(beh_glyph.rendered.id, isolated_beh, "{prepared:?}");
        assert!(
            group
                .shaped
                .runs
                .iter()
                .filter_map(|run| run.font_id)
                .count()
                >= 2,
            "{prepared:?}"
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_splits_text_groups_at_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;
        let left = InlineLineItem::Fragment(inline_fragment("A", style.clone()));
        let atom = InlineLineItem::Atom(InlineAtom {
            content: InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge),
            style: style.clone(),
            escaped_positioned_layers: None,
            width: 10.0,
            height: 0.0,
            baseline_offset: 0.0,
            baseline_shift: 0.0,
            link_target: None,
            alt_text: None,
        });
        let right = InlineLineItem::Fragment(inline_fragment("B", style.clone()));
        let left_width = builder.font_system.measure_text("A", &style);
        let right_width = builder.font_system.measure_text("B", &style);
        let carried_left_width = left_width + 5.0;
        let line_left = builder.content_left;
        let line_fragment = inline_layout::InlineLineFragment {
            items: vec![
                inline_layout::MeasuredInlineItem {
                    item: left,
                    width: carried_left_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: atom,
                    width: 10.0,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: right,
                    width: right_width,
                    shaped: None,
                },
            ],
            metrics: InlineLineMetrics {
                width: carried_left_width + 10.0 + right_width,
                height: 20.0,
                baseline_offset: 16.0,
            },
            hanging_widths: HangingPunctuationWidths::default(),
            indent: 0.0,
            available_width: 200.0,
            suppress_float_adjust: false,
            text: "AB".to_string(),
        };
        let prepared = builder
            .prepare_inline_line_fragment(
                &line_fragment,
                InlinePaintContext {
                    block_style: &style,
                    direction: style.direction,
                    available_width: 200.0,
                    padding_left: 0.0,
                    line_indent: 0.0,
                    text_align: TextAlign::Left,
                    is_first_line: true,
                },
            )
            .expect("mixed line should prepare");

        let text_groups = prepared
            .paint_items
            .iter()
            .filter(|item| matches!(item, PreparedInlinePaintItem::TextGroup(_)))
            .count();
        let atoms = prepared
            .paint_items
            .iter()
            .filter(|item| matches!(item, PreparedInlinePaintItem::Atom(_)))
            .count();
        assert_eq!(text_groups, 2);
        assert_eq!(atoms, 1);
        let atom_x = prepared
            .paint_items
            .iter()
            .find_map(|item| match item {
                PreparedInlinePaintItem::Atom(atom) => Some(atom.content_rect.x()),
                _ => None,
            })
            .expect("atom should be prepared");
        assert!(
            (atom_x - (line_left + carried_left_width)).abs() < 0.01,
            "mixed inline painting should advance with the carried graph width"
        );
    }

    #[tokio::test]
    async fn sequence_materialization_preserves_internal_spaces_around_opaque_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;

        let sequence = builder.collect_inline_line_sequence(
            vec![
                inline_word("A\n", &style),
                inline_test_atom(5.0, &style),
                inline_word(" B", &style),
            ],
            &style,
            200.0,
            0.0,
            0.0,
        );
        let fragment = sequence.records[0].fragment.as_ref().unwrap();
        let fragment_texts = fragment
            .items
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Fragment(fragment) => Some(fragment.text.clone()),
                InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(fragment.text, "A  B");
        assert_eq!(fragment_texts, vec!["A", " ", " ", "B"]);
    }

    #[tokio::test]
    async fn prepared_inline_line_emits_space_text_groups_around_opaque_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;

        let sequence = builder.collect_inline_line_sequence(
            vec![
                inline_word("A\n", &style),
                inline_test_atom(5.0, &style),
                inline_word(" B", &style),
            ],
            &style,
            200.0,
            0.0,
            0.0,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &sequence.records[0],
                inline_paragraph_context(&style, 200.0),
                &mut plaintext_state,
            )
            .expect("atom-adjacent line should prepare");
        let groups = prepared_text_groups(&prepared);

        assert_eq!(groups.len(), 2, "{prepared:?}");
        assert_eq!(groups[0].shaped.text, "A ");
        assert_eq!(groups[1].shaped.text, " B");
        assert!(groups[0].width() > builder.font_system.measure_text("A", &style));
        assert!(groups[1].width() > builder.font_system.measure_text("B", &style));
    }

    #[tokio::test]
    async fn prepared_inline_line_keeps_transparent_edge_spaces_in_text_context() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;

        let sequence = builder.collect_inline_line_sequence(
            vec![
                inline_word("A\n", &style),
                inline_box_edge(0.0, &style),
                inline_word("B", &style),
            ],
            &style,
            200.0,
            0.0,
            0.0,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &sequence.records[0],
                inline_paragraph_context(&style, 200.0),
                &mut plaintext_state,
            )
            .expect("transparent-edge line should prepare");
        let group_text = prepared_text_groups(&prepared)
            .into_iter()
            .map(|group| group.shaped.text.as_str())
            .collect::<String>();

        assert_eq!(sequence.records[0].fragment.as_ref().unwrap().text, "A B");
        assert_eq!(group_text, "A B");
    }

    fn prepared_text_groups(prepared: &PreparedInlineLine) -> Vec<&PreparedInlineTextGroup> {
        prepared
            .paint_items
            .iter()
            .filter_map(|item| match item {
                PreparedInlinePaintItem::TextGroup(group) => Some(group),
                _ => None,
            })
            .collect()
    }

    fn prepared_fragment_backgrounds(
        prepared: &PreparedInlineLine,
    ) -> Vec<&PreparedInlineFragment> {
        prepared
            .paint_items
            .iter()
            .filter_map(|item| match item {
                PreparedInlinePaintItem::FragmentBackground(fragment) => Some(fragment),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn split_inline_after_block_paints_only_inline_end_edge() {
        let root = dom::parse(
            r#"<!DOCTYPE html>
            <html><body><span><div>One</div>Two</span></body></html>"#,
        );
        let author = css::parse_stylesheet(&crate::css::Css::from_string(
            "body > span { border: 3px solid blue }",
        ));
        let stylesheets = vec![css::html5_user_agent_stylesheet(), author];
        let parent_style = ComputedStyle {
            font_size: 12.0,
            line_height: 14.4,
            color: Color::BLACK,
            ..ComputedStyle::initial()
        };
        let page = box_tree::build_page_box(&root, &stylesheets, &parent_style);
        let body = &page.children[0].children()[0];
        let box_tree::FormattingBox::AnonymousBlock(anonymous) = &body.children()[1] else {
            panic!("span text after the block should be wrapped in an anonymous block");
        };
        let options = RenderOptions::default();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.cursor_y = 100.0;
        let mut items = Vec::new();
        builder.collect_inline_box_items(
            &anonymous.children,
            &stylesheets,
            None,
            0.0,
            &anonymous.style,
            anonymous.style.text_decoration,
            &mut items,
        );

        assert_eq!(
            items
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        InlineItem::Atom(atom)
                            if matches!(
                                &atom.content,
                                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge)
                            )
                    )
                })
                .count(),
            1,
            "{items:?}"
        );
        assert!(matches!(
            items.last(),
            Some(InlineItem::Atom(atom))
                if matches!(
                    &atom.content,
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge)
                )
        ));

        let sequence =
            builder.collect_inline_line_sequence(items, &anonymous.style, 200.0, 0.0, 0.0);
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &sequence.records[0],
                inline_paragraph_context(&anonymous.style, 200.0),
                &mut plaintext_state,
            )
            .expect("split inline line should prepare");
        let backgrounds = prepared_fragment_backgrounds(&prepared);
        assert_eq!(backgrounds.len(), 1, "{prepared:?}");
        assert!(!backgrounds[0].fragment.hanging_edges.blocks_start);
        assert!(backgrounds[0].fragment.hanging_edges.blocks_end);
    }

    fn inline_line_record_for_items(
        items: Vec<inline_layout::MeasuredInlineItem>,
        text: &str,
        width: f32,
        available_width: f32,
        style: &ComputedStyle,
    ) -> inline_layout::InlineLineRecord {
        inline_layout::InlineLineRecord {
            paragraph_index: 0,
            block_line_index: 0,
            paragraph_line_index: 0,
            fragment: Some(inline_layout::InlineLineFragment {
                items,
                metrics: InlineLineMetrics {
                    width,
                    height: style.line_height,
                    baseline_offset: style.font_size,
                },
                hanging_widths: HangingPunctuationWidths::default(),
                indent: 0.0,
                available_width,
                suppress_float_adjust: false,
                text: text.to_string(),
            }),
            is_first_formatted_line: true,
            is_last_line_in_paragraph: true,
            is_forced_empty: false,
            paragraph_last_hanging_width: 0.0,
            used_indent: 0.0,
            available_width,
            line_height: style.line_height,
        }
    }

    fn inline_paragraph_context<'a>(
        style: &'a ComputedStyle,
        available_width: f32,
    ) -> InlineParagraphContext<'a> {
        InlineParagraphContext {
            block_style: style,
            stylesheets: &[],
            available_width,
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
        }
    }

    #[test]
    fn inline_line_geometry_maps_horizontal_ltr_and_rtl_indents() {
        let mut style = ComputedStyle::initial();
        style.direction = Direction::Ltr;
        let ltr = InlineLineGeometry::new(
            20.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 120.0,
                padding_left: 2.0,
                line_indent: 10.0,
                text_align: TextAlign::Left,
                is_first_line: true,
            },
        );
        let ltr_origin = ltr.visual_line_origin(0.0, 12.0);
        let ltr_rect = ltr.visual_line_item_rect(0.0, ltr_origin, 0.0, 12.0, 80.0, 20.0);
        assert!((ltr_rect.x() - 32.0).abs() < 0.01);
        assert!((ltr_rect.y() - 80.0).abs() < 0.01);
        assert!((ltr_rect.width() - 12.0).abs() < 0.01);
        assert!((ltr_rect.height() - 20.0).abs() < 0.01);

        style.direction = Direction::Rtl;
        let rtl = InlineLineGeometry::new(
            20.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 120.0,
                padding_left: 2.0,
                line_indent: 10.0,
                text_align: TextAlign::Right,
                is_first_line: true,
            },
        );
        let rtl_origin = rtl.visual_line_origin(0.0, 12.0);
        let rtl_rect = rtl.visual_line_item_rect(0.0, rtl_origin, 0.0, 12.0, 80.0, 20.0);
        assert!((rtl_rect.x() - 120.0).abs() < 0.01);
        assert!((rtl_rect.y() - 80.0).abs() < 0.01);
        let rtl_next = rtl.visual_line_item_rect(0.0, rtl_origin, 12.0, 8.0, 80.0, 20.0);
        assert!((rtl_next.x() - 132.0).abs() < 0.01);
    }

    #[test]
    fn inline_line_geometry_maps_vertical_inline_axis() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.direction = Direction::Ltr;
        let geometry = InlineLineGeometry::new(
            20.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 120.0,
                padding_left: 2.0,
                line_indent: 10.0,
                text_align: TextAlign::Left,
                is_first_line: true,
            },
        );
        let origin = geometry.visual_line_origin(5.0, 12.0);
        let rect = geometry.visual_line_item_rect(5.0, origin, 0.0, 12.0, 80.0, 20.0);
        assert!((rect.x() - 22.0).abs() < 0.01);
        assert!((rect.y() - 73.0).abs() < 0.01);
        assert!((rect.width() - 20.0).abs() < 0.01);
        assert!((rect.height() - 12.0).abs() < 0.01);
    }

    #[test]
    fn inline_line_geometry_resolves_alignment_and_hanging_logically() {
        let mut style = ComputedStyle::initial();
        let ltr = InlineLineGeometry::new(
            0.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 100.0,
                padding_left: 0.0,
                line_indent: 0.0,
                text_align: TextAlign::Left,
                is_first_line: true,
            },
        );
        assert!((ltr.alignment_offset(30.0, TextAlign::Left) - 0.0).abs() < 0.01);
        assert!((ltr.alignment_offset(30.0, TextAlign::Center) - 35.0).abs() < 0.01);
        assert!((ltr.alignment_offset(30.0, TextAlign::Right) - 70.0).abs() < 0.01);
        assert!(
            (ltr.hanging_punctuation_offset(HangingPunctuationWidths {
                start: 5.0,
                end: 7.0
            }) + 5.0)
                .abs()
                < 0.01
        );

        style.direction = Direction::Rtl;
        let rtl = InlineLineGeometry::new(
            0.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 100.0,
                padding_left: 0.0,
                line_indent: 0.0,
                text_align: TextAlign::Right,
                is_first_line: true,
            },
        );
        assert!(
            (rtl.hanging_punctuation_offset(HangingPunctuationWidths {
                start: 5.0,
                end: 7.0
            }) - 7.0)
                .abs()
                < 0.01
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_unifies_split_and_unsplit_text() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Justify;
        style.text_align_last = TextAlignLast::Align(TextAlign::Justify);
        builder.cursor_y = 100.0;

        let whole_width = builder.font_system.measure_text("A B", &style);
        let whole = inline_layout::MeasuredInlineItem {
            item: InlineLineItem::Fragment(inline_fragment("A B", style.clone())),
            width: whole_width,
            shaped: None,
        };
        let split_left_width = builder.font_system.measure_text("A", &style);
        let split_right_width = builder.font_system.measure_text(" B", &style);
        let split = vec![
            inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment("A", style.clone())),
                width: split_left_width,
                shaped: None,
            },
            inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment(" B", style.clone())),
                width: split_right_width,
                shaped: None,
            },
        ];
        let available_width = 120.0;
        let whole_record =
            inline_line_record_for_items(vec![whole], "A B", whole_width, available_width, &style);
        let split_record =
            inline_line_record_for_items(split, "A B", whole_width, available_width, &style);
        let context = inline_paragraph_context(&style, available_width);
        let mut whole_plaintext_state = None;
        let mut split_plaintext_state = None;

        let whole_prepared = builder
            .prepare_inline_line_record(&whole_record, context, &mut whole_plaintext_state)
            .expect("whole line should prepare");
        let split_prepared = builder
            .prepare_inline_line_record(&split_record, context, &mut split_plaintext_state)
            .expect("split line should prepare");

        let whole_group = prepared_text_groups(&whole_prepared)[0];
        let split_group = prepared_text_groups(&split_prepared)[0];
        assert_eq!(whole_group.shaped.text, "A B");
        assert_eq!(split_group.shaped.text, "A B");
        assert!((whole_group.x() - split_group.x()).abs() < 0.01);
        assert!((whole_group.width() - split_group.width()).abs() < 0.01);
    }

    #[tokio::test]
    async fn prepared_inline_line_record_excludes_trailing_tracking_once() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.letter_spacing = ComputedLengthPercentage::from_length(5.0);
        builder.cursor_y = 100.0;

        let measured_width = builder.font_system.measure_text("AB", &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment("AB", style.clone())),
                width: measured_width,
                shaped: None,
            }],
            "AB",
            measured_width - style.used_letter_spacing(),
            100.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 100.0),
                &mut plaintext_state,
            )
            .expect("tracked line should prepare");

        let background = prepared_fragment_backgrounds(&prepared)[0];
        assert!(
            (background.rect.width() - (measured_width - style.used_letter_spacing())).abs() < 0.01
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_vertical_indent_moves_logical_inline_start() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;
        builder.cursor_y = 100.0;

        let measured_width = builder.font_system.measure_text("A", &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment("A", style.clone())),
                width: measured_width,
                shaped: None,
            }],
            "A",
            measured_width,
            100.0,
            &style,
        );
        let mut indented_record = record.clone();
        indented_record.used_indent = 10.0;
        let context = inline_paragraph_context(&style, 100.0);

        let mut plaintext_state = None;
        let unindented = builder
            .prepare_inline_line_record(&record, context, &mut plaintext_state)
            .expect("vertical line should prepare");
        let unindented_y = prepared_text_groups(&unindented)[0].y();

        builder.cursor_y = 100.0;
        let mut plaintext_state = None;
        let indented = builder
            .prepare_inline_line_record(&indented_record, context, &mut plaintext_state)
            .expect("indented vertical line should prepare");
        let indented_y = prepared_text_groups(&indented)[0].y();

        assert!(
            indented_y < unindented_y - 9.0,
            "vertical text-indent should move along the inline axis: {indented_y} vs {unindented_y}"
        );
    }

    #[tokio::test]
    async fn vertical_writing_positions_cjk_upright_and_latin_sideways() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("中文AB", &style, style.line_height)
            .expect("vertical text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(runs.iter().any(|run| {
            run.text.contains('中') && run.text_matrix == RenderedTextMatrix::IDENTITY
        }));
        assert!(runs.iter().any(|run| {
            run.text.contains("AB") && run.text_matrix == RenderedTextMatrix::ROTATE_CW
        }));
        let cjk_offsets = runs
            .iter()
            .filter(|run| run.text.contains('中') || run.text.contains('文'))
            .map(|run| run.y_offset)
            .collect::<Vec<_>>();
        assert!(
            cjk_offsets.windows(2).all(|window| window[1] < window[0]),
            "vertical LTR CJK offsets should advance downward: {cjk_offsets:?}"
        );
    }

    #[tokio::test]
    async fn vertical_mixed_orientation_uses_unicode_vertical_orientation() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("a§、\u{2329}", &style, style.line_height)
            .expect("mixed vertical text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(runs.iter().any(|run| {
            run.text.contains('a') && run.text_matrix == RenderedTextMatrix::ROTATE_CW
        }));
        assert!(runs.iter().any(|run| {
            run.text.contains('§') && run.text_matrix == RenderedTextMatrix::IDENTITY
        }));
        assert!(runs.iter().any(|run| {
            run.text.contains('、') && run.text_matrix == RenderedTextMatrix::IDENTITY
        }));
        assert!(runs.iter().any(|run| {
            run.text.contains('\u{2329}') && run.text_matrix == RenderedTextMatrix::ROTATE_CW
        }));
    }

    #[tokio::test]
    async fn horizontal_writing_ignores_text_orientation_for_run_matrices() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_orientation = TextOrientation::Sideways;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("AB中文", &style, style.line_height)
            .expect("horizontal text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(
            runs.iter()
                .all(|run| run.text_matrix == RenderedTextMatrix::IDENTITY && run.y_offset == 0.0)
        );
    }

    #[tokio::test]
    async fn vertical_text_orientation_upright_keeps_text_units_upright() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Upright;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("AB中", &style, style.line_height)
            .expect("upright vertical text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(
            runs.iter()
                .filter(|run| !run.text.is_empty())
                .all(|run| run.text_matrix == RenderedTextMatrix::IDENTITY)
        );
        assert!(runs.iter().any(|run| run.text.contains("A")));
        assert!(runs.iter().any(|run| run.text.contains('中')));
    }

    #[tokio::test]
    async fn vertical_text_orientation_sideways_rotates_all_text_units() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Sideways;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("中文AB", &style, style.line_height)
            .expect("sideways vertical text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(runs.iter().any(|run| run.text.contains("中文")));
        assert!(runs.iter().any(|run| run.text.contains("AB")));
        assert!(
            runs.iter()
                .filter(|run| !run.text.is_empty())
                .all(|run| run.text_matrix == RenderedTextMatrix::ROTATE_CW)
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_inter_character_preserves_fragment_metadata() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Justify;
        style.text_align_last = TextAlignLast::Align(TextAlign::Justify);
        style.text_justify = TextJustify::InterCharacter;
        builder.cursor_y = 100.0;

        let mut fragment = inline_fragment("AB", style.clone());
        fragment.link_target = Some("#target".to_string());
        fragment.baseline_shift = 2.0;
        let measured_width = builder.font_system.measure_text("AB", &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(fragment),
                width: measured_width,
                shaped: None,
            }],
            "AB",
            measured_width,
            120.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 120.0),
                &mut plaintext_state,
            )
            .expect("inter-character line should prepare");

        let groups = prepared_text_groups(&prepared);
        assert_eq!(groups.len(), 2);
        assert!(
            groups
                .iter()
                .all(|group| group.link_target.as_deref() == Some("#target"))
        );
        assert!(
            groups
                .iter()
                .all(|group| (group.y() - (100.0 - 16.0 + 2.0)).abs() < 5.0)
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_inter_character_avoids_joining_sequences() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Justify;
        style.text_align_last = TextAlignLast::Align(TextAlign::Justify);
        style.text_justify = TextJustify::InterCharacter;
        builder.cursor_y = 100.0;

        let text = "سلام";
        let measured_width = builder.font_system.measure_text(text, &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment(text, style.clone())),
                width: measured_width,
                shaped: None,
            }],
            text,
            measured_width,
            160.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 160.0),
                &mut plaintext_state,
            )
            .expect("inter-character line should prepare");

        let groups = prepared_text_groups(&prepared);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].shaped.text.chars().count(), text.chars().count());
        assert!(groups[0].width() < 80.0);
    }

    #[tokio::test]
    async fn prepared_inline_line_record_inter_character_blocks_atom_boundaries() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Justify;
        style.text_align_last = TextAlignLast::Align(TextAlign::Justify);
        style.text_justify = TextJustify::InterCharacter;
        builder.cursor_y = 100.0;

        let left_width = builder.font_system.measure_text("A", &style);
        let right_width = builder.font_system.measure_text("B", &style);
        let atom_width = 10.0;
        let line_left = builder.content_left;
        let record = inline_line_record_for_items(
            vec![
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(inline_fragment("A", style.clone())),
                    width: left_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Atom(InlineAtom {
                        content: InlineAtomContent::Canvas,
                        style: style.clone(),
                        escaped_positioned_layers: None,
                        width: atom_width,
                        height: 10.0,
                        baseline_offset: 8.0,
                        baseline_shift: 0.0,
                        link_target: None,
                        alt_text: None,
                    }),
                    width: atom_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(inline_fragment("B", style.clone())),
                    width: right_width,
                    shaped: None,
                },
            ],
            "AB",
            left_width + atom_width + right_width,
            200.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 200.0),
                &mut plaintext_state,
            )
            .expect("mixed inter-character line should prepare");

        let atom_x = prepared
            .paint_items
            .iter()
            .find_map(|item| match item {
                PreparedInlinePaintItem::Atom(atom) => Some(atom.content_rect.x()),
                _ => None,
            })
            .expect("atom should be prepared");
        assert!(
            (atom_x - (line_left + left_width)).abs() < 0.01,
            "inter-character justification must not expand across opaque atom boundaries"
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_keeps_plaintext_alignment_paint_local() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Start;
        style.unicode_bidi = UnicodeBidi::Plaintext;
        style.direction = Direction::Ltr;
        builder.cursor_y = 100.0;

        let text = "אב";
        let measured_width = builder.font_system.measure_text(text, &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment(text, style.clone())),
                width: measured_width,
                shaped: None,
            }],
            text,
            measured_width,
            120.0,
            &style,
        );
        let original_text = record.fragment.as_ref().unwrap().text.clone();
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 120.0),
                &mut plaintext_state,
            )
            .expect("plaintext line should prepare");

        let group = prepared_text_groups(&prepared)[0];
        assert_eq!(plaintext_state, Some(Direction::Rtl));
        assert_eq!(record.fragment.as_ref().unwrap().text, original_text);
        assert!(
            group.x() > builder.content_left + 80.0,
            "RTL plaintext start should align right"
        );
    }

    #[tokio::test]
    async fn inline_text_measurement_splits_pre_line_paragraphs() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.white_space = WhiteSpace::PreLine;
        let text = "alpha beta\ngamma";

        let alpha = builder.font_system.measure_line_text("alpha", &style);
        let beta = builder.font_system.measure_line_text("beta", &style);
        let gamma = builder.font_system.measure_line_text("gamma", &style);
        let first_line = builder.font_system.measure_line_text("alpha beta", &style);
        let measurement = builder.intrinsic_inline_measurement_for_text(text, &style, f32::MAX);

        assert_eq!(measurement.paragraphs.len(), 2);
        assert_eq!(measurement.line_count(), 2);
        assert_eq!(measurement.sequence.records.len(), 2);
        assert_eq!(
            measurement.sequence.records[0]
                .fragment
                .as_ref()
                .unwrap()
                .text,
            "alpha beta"
        );
        assert_eq!(
            measurement.sequence.records[1]
                .fragment
                .as_ref()
                .unwrap()
                .text,
            "gamma"
        );
        assert!((measurement.height() - 40.0).abs() < 0.01);
        assert!((measurement.contribution.min_content - alpha.max(beta).max(gamma)).abs() < 0.01);
        assert!((measurement.contribution.max_content - first_line.max(gamma)).abs() < 0.01);
    }

    #[tokio::test]
    async fn intrinsic_inline_measurement_uses_sequence_for_forced_empty_lines() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.white_space = WhiteSpace::PreLine;

        let measurement =
            builder.intrinsic_inline_measurement_for_text("alpha\n\nbeta", &style, 200.0);

        assert_eq!(measurement.line_count(), 3);
        assert_eq!(measurement.sequence.records.len(), 3);
        assert_eq!(measurement.forced_empty_line_count(), 1);
        assert_eq!(
            measurement.sequence.records[0]
                .fragment
                .as_ref()
                .unwrap()
                .text,
            "alpha"
        );
        assert!(measurement.sequence.records[1].is_forced_empty);
        assert_eq!(
            measurement.sequence.records[2]
                .fragment
                .as_ref()
                .unwrap()
                .text,
            "beta"
        );
        assert!((measurement.height() - 42.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn raw_text_sequence_preserves_forced_empty_lines() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.white_space = WhiteSpace::PreLine;

        let sequence = builder.inline_line_sequence_for_raw_inline_text(
            "alpha\n\nbeta",
            &style,
            200.0,
            0.0,
            None,
        );

        assert_eq!(sequence.records.len(), 3);
        assert_eq!(sequence.records[0].fragment.as_ref().unwrap().text, "alpha");
        assert!(sequence.records[1].is_forced_empty);
        assert_eq!(sequence.records[2].fragment.as_ref().unwrap().text, "beta");
        assert!((sequence.total_height() - 42.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn inline_line_sequence_keeps_generated_like_forced_break_records() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("prefix", &style),
            InlineItem::Break,
            InlineItem::Break,
            inline_word("suffix", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 200.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 3);
        assert_eq!(
            sequence_fragment_texts(&sequence),
            vec!["prefix", "", "suffix"]
        );
        assert!(sequence.records[1].is_forced_empty);
        assert!((sequence.total_height() - 42.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn inline_line_sequence_resolves_generated_leaders_before_painting() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::Monospace;
        style.font_size = 10.0;
        style.line_height = 12.0;
        let items = vec![
            inline_word("Chapter", &style),
            inline_leader(".", &style),
            inline_word("2", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 120.0, 0.0, 0.0);
        let fragment = sequence.records[0].fragment.as_ref().unwrap();
        let leader_fragments = fragment
            .items
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Fragment(fragment) if fragment.generated_leader => Some(fragment),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(leader_fragments.len(), 1);
        assert!(
            leader_fragments[0]
                .text
                .chars()
                .all(|character| character == '.')
        );
        assert!(leader_fragments[0].text.len() > 1);
        assert_eq!(
            leader_fragments[0].link_target.as_deref(),
            Some("https://example.test/")
        );
        assert!(
            fragment
                .items
                .iter()
                .all(|item| !matches!(&item.item, InlineLineItem::Atom(atom) if matches!(atom.content, InlineAtomContent::Leader(_))))
        );
        assert_eq!(
            fragment.text,
            format!("Chapter{}2", leader_fragments[0].text)
        );
    }

    #[tokio::test]
    async fn inline_line_sequence_divides_multiple_generated_leaders() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::Monospace;
        style.font_size = 10.0;
        style.line_height = 12.0;
        let items = vec![
            inline_word("A", &style),
            inline_leader(".", &style),
            inline_word("B", &style),
            inline_leader("_", &style),
            inline_word("C", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 120.0, 0.0, 0.0);
        let fragment = sequence.records[0].fragment.as_ref().unwrap();
        let leader_texts = fragment
            .items
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Fragment(fragment) if fragment.generated_leader => {
                    Some(fragment.text.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(leader_texts.len(), 2);
        assert!(leader_texts[0].chars().all(|character| character == '.'));
        assert!(leader_texts[1].chars().all(|character| character == '_'));
        assert!(leader_texts[0].len().abs_diff(leader_texts[1].len()) <= 1);
        assert_eq!(
            fragment.text,
            format!("A{}B{}C", leader_texts[0], leader_texts[1])
        );
    }

    #[tokio::test]
    async fn inline_line_sequence_drops_empty_generated_leaders() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::Monospace;
        style.font_size = 10.0;
        style.line_height = 12.0;
        let items = vec![
            inline_word("A", &style),
            inline_leader("", &style),
            inline_word("C", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 120.0, 0.0, 0.0);
        let fragment = sequence.records[0].fragment.as_ref().unwrap();

        assert_eq!(fragment.text, "AC");
        assert!(
            fragment
                .items
                .iter()
                .all(|item| !matches!(&item.item, InlineLineItem::Fragment(fragment) if fragment.generated_leader))
        );
    }

    #[tokio::test]
    async fn generated_leader_fragments_are_not_justification_opportunities() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::Monospace;
        let normal_space = InlineFragment {
            text: "   ".to_string(),
            style: style.clone(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            generated_leader: false,
            hanging_edges: InlineHangingEdges::default(),
        };
        let mut leader_space = normal_space.clone();
        leader_space.mergeable = false;
        leader_space.generated_leader = true;

        assert!(inline_fragment_is_inter_word_justification_space(
            &normal_space
        ));
        assert!(!inline_fragment_is_inter_word_justification_space(
            &leader_space
        ));
        let plan = InlineJustificationPlan::for_line(
            &[InlineLineItem::Fragment(leader_space)],
            TextJustify::InterCharacter,
            true,
        );
        assert_eq!(plan.expansion_opportunity_count(), 0);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_records_break_spaces_before_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::BreakSpaces;
        style.font_family = css::FontFamily::SansSerif;
        let items = vec![
            InlineItem::Word(Box::new(InlineWord {
                text: "A".to_string(),
                style: style.clone(),
                baseline_shift: 0.0,
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
            })),
            InlineItem::Word(Box::new(InlineWord {
                text: " ".to_string(),
                style: style.clone(),
                baseline_shift: 0.0,
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
            })),
            InlineItem::Atom(Box::new(InlineAtom {
                content: InlineAtomContent::InlineBox {
                    sequence: empty_inline_sequence(),
                },
                style: style.clone(),
                escaped_positioned_layers: None,
                width: 5.0,
                height: 0.0,
                baseline_offset: 0.0,
                baseline_shift: 0.0,
                link_target: None,
                alt_text: None,
            })),
            InlineItem::Word(Box::new(InlineWord {
                text: "B".to_string(),
                style: style.clone(),
                baseline_shift: 0.0,
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
            })),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::BreakSpaces
        }));
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::AtomicBoundary
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_preserves_float_marker_source_order_without_width() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("A", &style),
            inline_test_float(&style),
            inline_word("B", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 3);
        assert!(matches!(graph.runs[1].item, InlineLineItem::Float(_)));
        assert_eq!(graph.runs[1].width, 0.0);
        assert_eq!(
            graph.first_float_position_in_range(inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            }),
            Some(inline_layout::InlineGraphPosition::at_run_start(1))
        );
    }

    #[tokio::test]
    async fn inline_opportunity_graph_intrinsic_contribution_uses_segments_and_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            InlineItem::Word(Box::new(InlineWord {
                text: "alpha beta".to_string(),
                style: style.clone(),
                baseline_shift: 0.0,
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
            })),
            InlineItem::Atom(Box::new(InlineAtom {
                content: InlineAtomContent::InlineBox {
                    sequence: empty_inline_sequence(),
                },
                style: style.clone(),
                escaped_positioned_layers: None,
                width: 28.0,
                height: 0.0,
                baseline_offset: 0.0,
                baseline_shift: 0.0,
                link_target: None,
                alt_text: None,
            })),
            InlineItem::Word(Box::new(InlineWord {
                text: "gamma".to_string(),
                style: style.clone(),
                baseline_shift: 0.0,
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
            })),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let contribution = graph.intrinsic_contribution(&mut builder.font_system, &style);

        assert!(contribution.max_content > contribution.min_content);
        assert!(contribution.min_content >= 28.0);
        assert!(
            contribution.max_content > 28.0 + builder.font_system.measure_text("gamma", &style)
        );
    }

    #[tokio::test]
    async fn inline_opportunity_graph_records_soft_hyphen_inside_text_run() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "hyphen\u{00ad}ation".to_string(),
            style: style.clone(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
        }))];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 1);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::Hyphenation
                && opportunity.soft_hyphen
                && opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_records_zero_width_space_inside_text_run() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "abc\u{200b}def".to_string(),
            style: style.clone(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
        }))];

        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let contribution = graph.intrinsic_contribution(&mut builder.font_system, &style);

        assert_eq!(graph.runs.len(), 1);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::SoftWrap
                && opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
        }));
        assert!(contribution.max_content > contribution.min_content);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materializes_soft_hyphen_visibility() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![inline_word("hyphen\u{00ad}ation", &style)];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let hyphen_break = graph
            .opportunities
            .iter()
            .copied()
            .find(|opportunity| opportunity.soft_hyphen)
            .expect("soft hyphen should create a graph opportunity");

        let unbroken = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );
        let broken = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: hyphen_break.position,
            },
            Some(hyphen_break),
            &mut builder.font_system,
            &style,
        );

        assert_eq!(unbroken.text, "hyphenation");
        assert_eq!(broken.text, "hyphen-");
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_strips_zero_width_space() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![inline_word("abc\u{200b}def", &style)];
        let graph = builder.build_inline_opportunity_graph(&items, &style);

        let materialized = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        assert_eq!(materialized.text, "abcdef");
        assert!(!materialized.text.contains('\u{200b}'));
        assert!(materialized.content_width > 0.0);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_trims_collapsed_trailing_space() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![inline_word("A", &style), inline_word(" ", &style)];
        let graph = builder.build_inline_opportunity_graph(&items, &style);

        let materialized = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        assert_eq!(materialized.text, "A");
        assert!(materialized.trimmed_width > 0.0);
        assert_eq!(materialized.items.len(), 1);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_hangs_pre_wrap_spaces_only_at_break() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.white_space = WhiteSpace::PreWrap;
        let items = vec![
            inline_word("A", &style),
            inline_word("   ", &style),
            inline_word("B", &style),
        ];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let space_break = graph
            .opportunities
            .iter()
            .copied()
            .find(|opportunity| opportunity.hangs && opportunity.position.run_index == 2)
            .expect("pre-wrap trailing spaces should create a hanging break");

        let broken = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: space_break.position,
            },
            Some(space_break),
            &mut builder.font_system,
            &style,
        );
        let unbroken = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        assert_eq!(broken.text, "A");
        assert!(broken.hanging_space_width > 0.0);
        assert_eq!(unbroken.text, "A   B");
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_preserves_break_spaces() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.white_space = WhiteSpace::BreakSpaces;
        let items = vec![inline_word("A", &style), inline_word(" ", &style)];
        let graph = builder.build_inline_opportunity_graph(&items, &style);

        let materialized = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        assert_eq!(materialized.text, "A ");
        assert_eq!(materialized.items.len(), 2);
        assert_eq!(materialized.trimmed_width, 0.0);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_preserves_metadata_after_control_stripping() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "a\u{200b}bc".to_string(),
            style: style.clone(),
            baseline_shift: 2.0,
            link_target: Some("https://example.test/".to_string()),
            mergeable: false,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges {
                blocks_start: true,
                blocks_end: true,
            },
        }))];
        let graph = builder.build_inline_opportunity_graph(&items, &style);

        let materialized = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        let [item] = materialized.items.as_slice() else {
            panic!("control stripping should leave one text fragment");
        };
        let InlineLineItem::Fragment(fragment) = &item.item else {
            panic!("materialized item should remain a text fragment");
        };
        assert_eq!(fragment.text, "abc");
        assert_eq!(fragment.baseline_shift, 2.0);
        assert_eq!(
            fragment.link_target.as_deref(),
            Some("https://example.test/")
        );
        assert!(!fragment.mergeable);
        assert!(fragment.hanging_edges.blocks_start);
        assert!(fragment.hanging_edges.blocks_end);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_records_uax14_breaks_without_splitting_text_run() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "中文english中文".to_string(),
            style: style.clone(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
        }))];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 1);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::SoftWrap
                && opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
                && opportunity.position.byte_offset < "中文english中文".len()
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_distinguishes_anywhere_from_break_word_min_content() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut anywhere = ComputedStyle::initial();
        anywhere.font_family = css::FontFamily::SansSerif;
        anywhere.font_size = 12.0;
        anywhere.line_height = 14.0;
        anywhere.overflow_wrap = css::OverflowWrap::Anywhere;
        let anywhere_items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "abcdefgh".to_string(),
            style: anywhere.clone(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
        }))];

        let anywhere_graph = builder.build_inline_opportunity_graph(&anywhere_items, &anywhere);
        let anywhere_contribution =
            anywhere_graph.intrinsic_contribution(&mut builder.font_system, &anywhere);

        assert!(anywhere_graph.opportunities.iter().any(|opportunity| {
            opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
                && opportunity.min_content
        }));
        assert!(anywhere_contribution.max_content > anywhere_contribution.min_content);

        let mut break_word = anywhere.clone();
        break_word.overflow_wrap = css::OverflowWrap::BreakWord;
        let break_word_items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "abcdefgh".to_string(),
            style: break_word.clone(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
        }))];

        let break_word_graph =
            builder.build_inline_opportunity_graph(&break_word_items, &break_word);
        let break_word_contribution =
            break_word_graph.intrinsic_contribution(&mut builder.font_system, &break_word);

        assert!(break_word_graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::Emergency
                && opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
                && !opportunity.min_content
        }));
        assert!(
            (break_word_contribution.max_content - break_word_contribution.min_content).abs()
                < 0.01
        );
    }

    #[tokio::test]
    async fn inline_opportunity_graph_partial_run_materialization_preserves_fragment_metadata() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.overflow_wrap = css::OverflowWrap::Anywhere;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "abcdef".to_string(),
            style: style.clone(),
            baseline_shift: 3.0,
            link_target: Some("https://example.test/".to_string()),
            mergeable: false,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges {
                blocks_start: true,
                blocks_end: true,
            },
        }))];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let opportunity = graph
            .opportunities
            .iter()
            .copied()
            .find(|opportunity| {
                opportunity.position.run_index == 0 && opportunity.position.byte_offset > 0
            })
            .expect("anywhere should expose an internal graph opportunity");

        let measured = graph.line_measured_items_for_graph_range(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: opportunity.position,
            },
            &mut builder.font_system,
        );

        let [item] = measured.as_slice() else {
            panic!("partial graph range should materialize one fragment");
        };
        let InlineLineItem::Fragment(fragment) = &item.item else {
            panic!("partial text range should remain a text fragment");
        };
        assert!(fragment.text.len() < "abcdef".len());
        assert_eq!(fragment.style.font_size, style.font_size);
        assert_eq!(fragment.baseline_shift, 3.0);
        assert_eq!(
            fragment.link_target.as_deref(),
            Some("https://example.test/")
        );
        assert!(!fragment.mergeable);
        assert!(fragment.hanging_edges.blocks_start);
        assert!(!fragment.hanging_edges.blocks_end);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_breaks_across_transparent_box_edges() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("中文", &style),
            inline_box_edge(3.0, &style),
            inline_word("english", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 3);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::SoftWrap
                && opportunity.position.run_index == 1
                && opportunity.position.byte_offset == 0
        }));
        assert!(!graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::AtomicBoundary
                && opportunity.position.run_index == 1
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_preserves_space_breaks_after_transparent_box_edges() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("ab", &style),
            inline_box_edge(2.0, &style),
            inline_word(" cd", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 3);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.position.run_index == 2 && opportunity.position.byte_offset > 0
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_keeps_real_atoms_atomic() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("A", &style),
            inline_test_atom(8.0, &style),
            inline_word("B", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::AtomicBoundary
                && opportunity.position.run_index == 1
        }));
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::AtomicBoundary
                && opportunity.position.run_index == 2
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_tracks_across_transparent_box_edges() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.letter_spacing = css::ComputedLengthPercentage::from_length(1.0);
        let items = vec![
            inline_word("A", &style),
            inline_box_edge(2.0, &style),
            inline_word("B", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::SoftWrap
                && opportunity.position.run_index == 1
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materializes_ranges_with_transparent_box_edges() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            InlineItem::Word(Box::new(InlineWord {
                text: "ab".to_string(),
                style: style.clone(),
                baseline_shift: 2.0,
                link_target: Some("https://example.test/".to_string()),
                mergeable: false,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
            })),
            inline_box_edge(2.0, &style),
            inline_word(" cd", &style),
        ];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let opportunity = graph
            .opportunities
            .iter()
            .copied()
            .find(|opportunity| {
                opportunity.position.run_index == 2 && opportunity.position.byte_offset > 0
            })
            .expect("space break after a transparent edge should be graph-backed");

        let measured = graph.line_measured_items_for_graph_range(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: opportunity.position,
            },
            &mut builder.font_system,
        );

        assert_eq!(measured.len(), 3);
        assert!(matches!(
            measured[1].item,
            InlineLineItem::Atom(InlineAtom {
                content: InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge),
                ..
            })
        ));
        let InlineLineItem::Fragment(fragment) = &measured[0].item else {
            panic!("first item should remain the original text fragment");
        };
        assert_eq!(fragment.baseline_shift, 2.0);
        assert_eq!(
            fragment.link_target.as_deref(),
            Some("https://example.test/")
        );
        assert!(!fragment.mergeable);
    }

    #[tokio::test]
    async fn inline_line_fragment_preserves_graph_text_summary() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "Hello".to_string(),
            style: style.clone(),
            baseline_shift: 0.0,
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
        }))];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let context = InlineParagraphContext {
            block_style: &style,
            stylesheets: &[],
            available_width: 200.0,
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
        };

        let (lines, _) = builder.select_inline_lines_from_graph(&graph, context, 0, false);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello");
        assert_eq!(lines[0].items.len(), 1);
    }

    #[tokio::test]
    async fn used_border_preserves_layout_width_but_hides_non_painting_sides() {
        let mut style = ComputedStyle::initial();
        style.border_widths.top = 4.0;
        style.border_widths.right = 3.0;
        style.border_widths.bottom = 5.0;
        style.border_styles.top = BorderStyle::Hidden;
        style.border_styles.right = BorderStyle::Solid;
        style.border_styles.bottom = BorderStyle::Solid;
        style.border_colors.top = Color::new(255, 0, 0);
        style.border_colors.bottom = Color::TRANSPARENT;

        let border = used_border(&style);

        assert_eq!(border.top.specified_width, 4.0);
        assert_eq!(border.top.used_width, 0.0);
        assert!(!border.top.is_visible());
        assert_eq!(border.right.used_width, 3.0);
        assert!(border.right.is_visible());
        assert_eq!(border.bottom.used_width, 5.0);
        assert!(!border.bottom.is_visible());
    }
}
