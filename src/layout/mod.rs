use crate::css::{
    self, AdditiveCounterStyle, AlignContent, AlignItems, AlignSelf, AlignmentSafety,
    BackgroundImage, BookmarkLabelPart, BorderStyle, BoxSizing, CaptionSide, Clear, Color,
    ComputedStyle, Content, ContentAlignmentKeyword, CounterStyleRange, CounterStyleRule,
    CounterStyleSystem, CssBookmarkState, Declarations, Direction, Display, DisplayInner,
    ElementAttributeSignature, ElementSiblingSignature, ElementSignature, EmptyCells,
    FlexDirection, FlexWrap, Float, GeneratedAltTextPart, GeneratedContentPart, GeneratedQuote,
    Hyphens, JustifyContent, LinearGradientDirection, ListStylePosition, ListStyleType,
    MarkerContent, MarkerContentPart, MarkerSide, NamedStringPart, NumericCounterStyle, PageBreak,
    PageRule, PageSpecificity, PhysicalAxis, PhysicalSide, Position, Quotes, SelfAlignmentKeyword,
    Stylesheet, StylesheetOrigin, TableLayout, TextAlign, TextAutospace, TextDecorationSkipInk,
    TextDecorationSkipSpaces, TextDecorationStyle, TextDecorationThickness, TextJustify,
    TextTransformCase, TextUnderlineOffset, TextUnderlinePosition, UnicodeBidi, VerticalAlign,
    Visibility, WhiteSpace, WritingMode, block_end_side, block_start_side, inline_end_side,
    inline_start_side,
};
use crate::document::{
    Bookmark, BookmarkState, Document, DocumentMetadata, Page, PaintBand, PaintCheckpoint,
    PaintClip, PaintEffects, PaintFragment, PaintPrimitive, PaintStackingContext, PaintTransform,
    RenderedCornerRadius, RenderedImage, RenderedImageSourceRect, RenderedLine, RenderedLink,
    RenderedPath, RenderedPathClip, RenderedPathClipPath, RenderedPathCommand,
    RenderedPathFillRule, RenderedRect, RenderedRoundedRect, RenderedRoundedRectRadii,
    RenderedStroke, RenderedTextRun,
};
use crate::dom::{self, Element, Node, NodeKind};
use crate::resource::ResourceCache;
use crate::text::{
    FontSystem, FontSystemLoad, FontSystemSeedLoad, GlyphInkBox, OBJECT_REPLACEMENT_CHARACTER,
    ShapedInlineLine, StyledTextSpan, TextDecorationFontMetrics, TextLine,
    bidi_control_scope_for_style, character_is_bidi_format_control,
    character_is_first_hangable_punctuation, character_is_hangable_stop_or_comma,
    character_is_join_control, character_is_last_hangable_punctuation,
    character_is_unicode_alphanumeric, character_is_unicode_control,
    character_is_unicode_punctuation, character_preserves_word_boundary_context,
    character_receives_text_emphasis_mark, contains_bidi_text,
    inline_atomic_boundary_allows_soft_wrap, is_css_collapsible_whitespace,
    plaintext_direction_for_text, text_without_bidi_format_controls,
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
mod html_direction;
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
mod text_helpers;
mod text_paint;
mod used_values;

use asset_helpers::*;
use element_semantics::*;
use flow_helpers::*;
use html_direction::*;
use inline_collect::{block_bidi_scope_needs_inline_controls, push_inline_words_for_style};
use inline_helpers::*;
use paint_helpers::*;
use text_helpers::*;
use used_values::*;

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
        lines.push(RenderedLine {
            text: line,
            x: options.page_left(),
            y,
            font_size: options.font_size,
            font_id: line_font_id,
            color: Color::BLACK,
            runs,
        });
        y -= options.line_height;
    }

    if lines.is_empty() && pages.is_empty() {
        lines.push(RenderedLine {
            text: String::new(),
            x: options.page_left(),
            y,
            font_size: options.font_size,
            font_id,
            color: Color::BLACK,
            runs: Vec::new(),
        });
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
    let mut page_box = {
        let _timer = DebugTimer::start("building formatting box tree");
        box_tree::build_page_box(root, stylesheets, &parent_style)
    };
    let font_system = {
        let _timer = DebugTimer::start("finishing font system load");
        font_system_load.finish().await
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
        let text = dom::text_content(root);
        if !text.is_empty() {
            log::debug!("falling back to plain text layout");
            return layout_text_with_font_system(&text, options, builder.font_system);
        }
    }
    {
        let _timer = DebugTimer::start("finalizing laid out document");
        builder.finish()
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
    containing_block_direction: Direction,
    fragment_top_offsets: Vec<f32>,
    definite_block_size_stack: Vec<Option<f32>>,
    truncate_page_start_margins: bool,
    avoid_inside_retry_depth: usize,
    containing_blocks: Vec<ContainingBlock>,
    list_stack: Vec<ListState>,
    counter_set: CounterSet,
    quote_depth: usize,
    current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    current_page_running_elements: HashMap<String, Vec<NamedStringAssignment>>,
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
    float_contexts: Vec<FloatContext>,
    pending_float_fragments: Vec<PendingFloatPaintFragment>,
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
    row_left: f32,
    row_right: f32,
    left_x: f32,
    right_x: f32,
    row_top: f32,
    row_bottom: f32,
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct FloatShape {
    side: Float,
    page_index: usize,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
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
    page_index: usize,
    side: Float,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    source_order: usize,
    context: PaintStackingContext,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingFloatPaintFragment {
    page_index: usize,
    fragment: PaintFragment,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FloatBand {
    left: f32,
    right: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ContainingBlock {
    x: f32,
    top_y: f32,
    width: f32,
    height: f32,
}

/// Active axis-aligned overflow clipping rectangle.
///
/// CSS Overflow clips non-visible overflow to the box's overflow clip edge,
/// which defaults to the padding box for `overflow: hidden`:
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OverflowClip {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
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
    z_index: i32,
    context: PaintStackingContext,
    links: Vec<RenderedLink>,
}

#[derive(Debug, Clone, PartialEq)]
struct FixedPaintLayer {
    z_index: i32,
    context: PaintStackingContext,
    links: Vec<RenderedLink>,
}

#[derive(Debug, Clone, PartialEq)]
struct NamedStringAssignment {
    value: String,
    at_page_start: bool,
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
    hanging_edges: InlineHangingEdges,
}

#[derive(Debug, Clone)]
struct InlineFragment {
    text: String,
    style: ComputedStyle,
    baseline_shift: f32,
    link_target: Option<String>,
    mergeable: bool,
    hanging_edges: InlineHangingEdges,
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
    offset: f32,
    aligned_by_parley: bool,
    height: f32,
    baseline_offset: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct HangingPunctuationWidths {
    start: f32,
    end: f32,
}

/// A positioned mixed inline line ready for painting.
///
/// CSS Inline Layout constructs a line box before painting its inline
/// fragments. This prepared line stores the resolved line metrics and ordered
/// paint items so text shaping, atom placement, backgrounds, links, and
/// decorations consume one reusable line artifact:
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
#[derive(Debug, Clone)]
struct PreparedMixedInlineLine {
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
    x: f32,
    background_y: f32,
    width: f32,
    height: f32,
}

/// A positioned atomic inline box with resolved content geometry.
///
/// CSS 2.2 treats inline-blocks, replaced elements, and similar atomic inline
/// boxes as a single inline-level box participating in the parent line box:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>.
#[derive(Debug, Clone)]
struct PreparedInlineAtom {
    atom: InlineAtom,
    content_x: f32,
    y: f32,
    content_width: f32,
    content_height: f32,
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
    x: f32,
    y: f32,
    width: f32,
    style: ComputedStyle,
    link_target: Option<String>,
    shaped: ShapedInlineLine,
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
    available_width: f32,
    padding_left: f32,
    hanging_indent: f32,
    hanging_punctuation_reserve: f32,
}

#[derive(Debug, Clone, Copy)]
struct InlinePaintContext<'a> {
    block_style: &'a ComputedStyle,
    available_width: f32,
    padding_left: f32,
    line_indent: f32,
    text_align: TextAlign,
    is_first_line: bool,
    is_last_line: bool,
}

#[derive(Debug, Clone)]
struct InlineAtom {
    content: InlineAtomContent,
    style: ComputedStyle,
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
    Svg { fill: Color },
    InlineBox { lines: Vec<TextLine> },
    InlineFragment(PaintFragment),
    InlineEdge,
    Leader(String),
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

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum InlineLineItem {
    Fragment(InlineFragment),
    Atom(InlineAtom),
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
    fragment_top_offsets: Vec<f32>,
    definite_block_size_stack: Vec<Option<f32>>,
    truncate_page_start_margins: bool,
    avoid_inside_retry_depth: usize,
    containing_blocks: Vec<ContainingBlock>,
    list_stack: Vec<ListState>,
    counter_set: CounterSet,
    current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    current_page_running_elements: HashMap<String, Vec<NamedStringAssignment>>,
    ancestors: Vec<ElementSignature>,
    bookmarks: Vec<Bookmark>,
    positioned_layers: Vec<PositionedPaintLayer>,
    fixed_layers: Vec<FixedPaintLayer>,
    next_paint_source_order: usize,
    float_contexts: Vec<FloatContext>,
    pending_float_fragments: Vec<PendingFloatPaintFragment>,
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
            hanging_edges: InlineHangingEdges::default(),
        }
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
            hanging_edges: InlineHangingEdges::default(),
        };
        let mut right = left.clone();
        right.style.font_style = css::FontStyle::Italic;

        assert!(can_shape_inline_fragments_together(&left, &right));
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
        cell_style.vertical_align = VerticalAlign::Middle;
        cell_style.unicode_bidi = css::UnicodeBidi::Isolate;

        let normalized = normalized_anonymous_inline_content_style(&cell_style);

        assert_eq!(normalized.vertical_align, VerticalAlign::Baseline);
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

        assert_eq!(group.x, 12.0);
        assert_eq!(group.shaped.text, "AB");
        assert!(group.shaped.first_font_id().is_some());
        assert!((group.width - group.shaped.advance_width()).abs() < 0.01);
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
    async fn prepared_mixed_inline_line_splits_text_groups_at_atoms() {
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
            content: InlineAtomContent::InlineEdge,
            style: style.clone(),
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
                offset: 0.0,
                aligned_by_parley: false,
                height: 20.0,
                baseline_offset: 16.0,
            },
            hanging_widths: HangingPunctuationWidths::default(),
            indent: 0.0,
            available_width: 200.0,
            text: "AB".to_string(),
        };
        let prepared = builder
            .prepare_mixed_inline_line(
                &line_fragment,
                InlinePaintContext {
                    block_style: &style,
                    available_width: 200.0,
                    padding_left: 0.0,
                    line_indent: 0.0,
                    text_align: TextAlign::Left,
                    is_first_line: true,
                    is_last_line: true,
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
                PreparedInlinePaintItem::Atom(atom) => Some(atom.content_x),
                _ => None,
            })
            .expect("atom should be prepared");
        assert!(
            (atom_x - (line_left + carried_left_width)).abs() < 0.01,
            "mixed inline painting should advance with the carried graph width"
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
        assert_eq!(measurement.line_count, 2);
        assert!((measurement.height - 40.0).abs() < 0.01);
        assert!((measurement.contribution.min_content - alpha.max(beta).max(gamma)).abs() < 0.01);
        assert!((measurement.contribution.max_content - first_line.max(gamma)).abs() < 0.01);
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
                hanging_edges: InlineHangingEdges::default(),
            })),
            InlineItem::Word(Box::new(InlineWord {
                text: " ".to_string(),
                style: style.clone(),
                baseline_shift: 0.0,
                link_target: None,
                mergeable: true,
                hanging_edges: InlineHangingEdges::default(),
            })),
            InlineItem::Atom(Box::new(InlineAtom {
                content: InlineAtomContent::InlineEdge,
                style: style.clone(),
                width: 5.0,
                height: 0.0,
                baseline_offset: 0.0,
                baseline_shift: 0.0,
                link_target: None,
                alt_text: None,
            })),
            InlineItem::Word(Box::new(InlineWord {
                text: "B".to_string(),
                style,
                baseline_shift: 0.0,
                link_target: None,
                mergeable: true,
                hanging_edges: InlineHangingEdges::default(),
            })),
        ];

        let graph = builder.build_inline_opportunity_graph(&items);

        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::BreakSpaces
        }));
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::AtomicBoundary
        }));
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
                hanging_edges: InlineHangingEdges::default(),
            })),
            InlineItem::Atom(Box::new(InlineAtom {
                content: InlineAtomContent::InlineEdge,
                style: style.clone(),
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
                hanging_edges: InlineHangingEdges::default(),
            })),
        ];

        let graph = builder.build_inline_opportunity_graph(&items);
        let contribution = graph.intrinsic_contribution(&mut builder.font_system, &style);

        assert!(contribution.max_content > contribution.min_content);
        assert!(contribution.min_content >= 28.0);
        assert!(
            contribution.max_content > 28.0 + builder.font_system.measure_text("gamma", &style)
        );
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
            hanging_edges: InlineHangingEdges::default(),
        }))];
        let graph = builder.build_inline_opportunity_graph(&items);
        let context = InlineParagraphContext {
            block_style: &style,
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
