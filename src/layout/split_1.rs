use super::*;

#[cfg(test)]
pub(in crate::layout) fn block_align_content_y_offset(
    align_content: AlignContent,
    free_space: f32,
) -> f32 {
    content_alignment_y_offset(align_content, free_space, true)
}

/// Return the page-space block-axis offset for a block container or table cell
/// with a concrete computed style.
///
/// CSS Box Alignment defaults block-container overflow alignment to `safe`
/// unless the alignment container is scrollable:
/// <https://www.w3.org/TR/css-align-3/#overflow-values>.
pub(in crate::layout) fn block_align_content_y_offset_for_style(
    style: &ComputedStyle,
    free_space: f32,
) -> f32 {
    content_alignment_y_offset(
        style.align_content,
        free_space,
        block_align_content_defaults_to_safe_overflow(style),
    )
}

pub(in crate::layout) fn block_align_content_defaults_to_safe_overflow(
    style: &ComputedStyle,
) -> bool {
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
pub(in crate::layout) fn multicol_align_content_y_offset(
    align_content: AlignContent,
    free_space: f32,
) -> f32 {
    content_alignment_y_offset(align_content, free_space, false)
}

pub(in crate::layout) fn content_alignment_y_offset(
    align_content: AlignContent,
    free_space: f32,
    default_safe_overflow: bool,
) -> f32 {
    -content_alignment_offset_toward_end(align_content, free_space, default_safe_overflow)
}

pub(in crate::layout) fn content_alignment_offset_toward_end(
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
pub(in crate::layout) fn block_align_content_establishes_independent_formatting_context(
    align_content: AlignContent,
) -> bool {
    align_content.keyword != ContentAlignmentKeyword::Normal
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    pub width: LayoutLength,
    pub height: LayoutLength,
}

pub(in crate::layout) const LIST_ITEM_COUNTER_NAME: &str = "list-item";

impl PageSize {
    pub const A4_POINTS: Self = Self {
        width: layout_pt(595.2756),
        height: layout_pt(841.8898),
    };

    pub const fn from_points(width: f32, height: f32) -> Self {
        Self {
            width: layout_pt(width),
            height: layout_pt(height),
        }
    }

    pub fn width(&self) -> f32 {
        layout_points(self.width)
    }

    pub fn height(&self) -> f32 {
        layout_points(self.height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMargins {
    pub top: LayoutLength,
    pub right: LayoutLength,
    pub bottom: LayoutLength,
    pub left: LayoutLength,
}

impl PageMargins {
    pub const WEASYPRINT_DEFAULT_POINTS: f32 = 56.25;
    pub const DEFAULT: Self = Self {
        top: layout_pt(Self::WEASYPRINT_DEFAULT_POINTS),
        right: layout_pt(Self::WEASYPRINT_DEFAULT_POINTS),
        bottom: layout_pt(Self::WEASYPRINT_DEFAULT_POINTS),
        left: layout_pt(Self::WEASYPRINT_DEFAULT_POINTS),
    };

    pub const fn all(length: LayoutLength) -> Self {
        Self {
            top: length,
            right: length,
            bottom: length,
            left: length,
        }
    }

    pub const fn all_points(value: f32) -> Self {
        Self::all(layout_pt(value))
    }

    pub const fn from_points(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top: layout_pt(top),
            right: layout_pt(right),
            bottom: layout_pt(bottom),
            left: layout_pt(left),
        }
    }

    pub fn top(&self) -> f32 {
        layout_points(self.top)
    }

    pub fn right(&self) -> f32 {
        layout_points(self.right)
    }

    pub fn bottom(&self) -> f32 {
        layout_points(self.bottom)
    }

    pub fn left(&self) -> f32 {
        layout_points(self.left)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    pub page_size: PageSize,
    pub margin: LayoutLength,
    pub page_margins: PageMargins,
    pub font_size: LayoutLength,
    pub line_height: LayoutLength,
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
    pub fn set_margin(&mut self, margin: LayoutLength) {
        self.margin = margin;
        self.page_margins = PageMargins::all(margin);
    }

    pub fn set_margin_points(&mut self, margin: f32) {
        self.set_margin(layout_pt(margin));
    }

    pub fn set_page_margins(&mut self, margins: PageMargins) {
        self.margin = margins.top;
        self.page_margins = margins;
    }

    pub fn margin(&self) -> f32 {
        layout_points(self.margin)
    }

    pub fn font_size(&self) -> f32 {
        layout_points(self.font_size)
    }

    pub fn line_height(&self) -> f32 {
        layout_points(self.line_height)
    }

    pub fn page_margins(&self) -> PageMargins {
        if self.page_margins == PageMargins::DEFAULT
            && (self.margin() - PageMargins::WEASYPRINT_DEFAULT_POINTS).abs() > 0.01
        {
            PageMargins::all(self.margin)
        } else {
            self.page_margins
        }
    }

    pub(crate) fn page_left(&self) -> f32 {
        self.page_margins().left()
    }

    pub(crate) fn page_top(&self) -> f32 {
        self.page_size.height() - self.page_margins().top()
    }

    pub(crate) fn page_bottom(&self) -> f32 {
        self.page_margins().bottom()
    }

    pub(crate) fn page_area_width(&self) -> f32 {
        (self.page_size.width() - self.page_margins().left() - self.page_margins().right()).max(0.0)
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        let font_size = 12.0;
        Self {
            page_size: PageSize::A4_POINTS,
            margin: layout_pt(PageMargins::WEASYPRINT_DEFAULT_POINTS),
            page_margins: PageMargins::DEFAULT,
            font_size: layout_pt(font_size),
            line_height: layout_pt(font_size * 1.2),
            producer: "reasyprint 0.1.0".to_string(),
            pdf_variant: crate::document::PdfVariant::default(),
            presentational_hints: false,
            target_fragment: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PageContext {
    pub(in crate::layout) size: PageSize,
    pub(in crate::layout) margins: PageMargins,
    pub(in crate::layout) edges: PageBoxEdges,
    pub(in crate::layout) rotation: i32,
}

/// Used page-box border and padding edges for the document page area.
///
/// CSS Paged Media makes page boxes follow the CSS box model: page margins
/// surround the page border, page padding is inside that border, and document
/// content is laid out in the page area/content box:
/// <https://www.w3.org/TR/css-page-3/#page-model> and
/// <https://www.w3.org/TR/css-box-3/#box-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PageBoxEdges {
    pub(in crate::layout) border: css::Edges,
    pub(in crate::layout) padding: css::Edges,
}

impl PageBoxEdges {
    pub(in crate::layout) const ZERO: Self = Self {
        border: css::Edges::ZERO,
        padding: css::Edges::ZERO,
    };

    pub(in crate::layout) fn left(self) -> f32 {
        self.border.left + self.padding.left
    }

    pub(in crate::layout) fn right(self) -> f32 {
        self.border.right + self.padding.right
    }

    pub(in crate::layout) fn top(self) -> f32 {
        self.border.top + self.padding.top
    }

    pub(in crate::layout) fn bottom(self) -> f32 {
        self.border.bottom + self.padding.bottom
    }

    pub(in crate::layout) fn total(self) -> css::Edges {
        css::Edges {
            top: self.top(),
            right: self.right(),
            bottom: self.bottom(),
            left: self.left(),
        }
    }
}

impl PageContext {
    pub(in crate::layout) fn from_options(options: &RenderOptions) -> Self {
        Self {
            size: options.page_size,
            margins: options.page_margins(),
            edges: PageBoxEdges::ZERO,
            rotation: 0,
        }
    }

    pub(in crate::layout) fn left(self) -> f32 {
        self.margins.left() + self.edges.left()
    }

    pub(in crate::layout) fn right(self) -> f32 {
        self.size.width() - self.margins.right() - self.edges.right()
    }

    pub(in crate::layout) fn top(self) -> f32 {
        self.size.height() - self.margins.top() - self.edges.top()
    }

    pub(in crate::layout) fn bottom(self) -> f32 {
        self.margins.bottom() + self.edges.bottom()
    }

    pub(in crate::layout) fn area_width(self) -> f32 {
        (self.size.width()
            - self.margins.left()
            - self.margins.right()
            - self.edges.left()
            - self.edges.right())
        .max(0.0)
    }

    pub(in crate::layout) fn area_height(self) -> f32 {
        (self.size.height()
            - self.margins.top()
            - self.margins.bottom()
            - self.edges.top()
            - self.edges.bottom())
        .max(0.0)
    }
}

pub(in crate::layout) fn layout_text_with_font_system(
    text: &str,
    options: &RenderOptions,
    mut font_system: FontSystem,
) -> Document {
    let mut default_style = ComputedStyle::initial();
    default_style.font_size = options.font_size();
    default_style.line_height_value = css::ComputedLineHeight::from_points(options.line_height());
    default_style.line_height = options.line_height();
    default_style.line_height_multiplier = None;
    default_style.line_height_is_normal = false;
    let font_id = font_system.resolve_style(&default_style);
    let content_width = options.page_area_width().max(options.font_size());
    let approx_char_width = options.font_size() * 0.5;
    let max_chars = (content_width / approx_char_width).floor().max(1.0) as usize;

    let mut pages = Vec::new();
    let mut lines = Vec::new();
    let mut y = options.page_top() - options.font_size();
    let bottom = options.page_bottom();

    for line in wrap_text(text, max_chars) {
        if y < bottom {
            let mut page = Page::new(options.page_size.width(), options.page_size.height());
            for line in lines {
                page.push_line(line);
            }
            pages.push(page);
            lines = Vec::new();
            y = options.page_top() - options.font_size();
        }
        let runs = font_system.shape_text_runs_with_parley(&line, &default_style);
        let line_font_id = runs.first().and_then(|run| run.font_id).or(font_id);
        lines.push(RenderedLine::from_paint_origin(
            line,
            paint_space_point(options.page_left(), y),
            options.font_size(),
            line_font_id,
            Color::BLACK,
            runs,
        ));
        y -= options.line_height();
    }

    if lines.is_empty() && pages.is_empty() {
        lines.push(RenderedLine::from_paint_origin(
            String::new(),
            paint_space_point(options.page_left(), y),
            options.font_size(),
            font_id,
            Color::BLACK,
            Vec::new(),
        ));
    }

    if !lines.is_empty() {
        let mut page = Page::new(options.page_size.width(), options.page_size.height());
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

pub(in crate::layout) enum LayoutResult {
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
    let default_line_height_multiplier = if options.font_size() > 0.0 {
        options.line_height() / options.font_size()
    } else {
        1.2
    };
    let parent_style = Box::new(ComputedStyle {
        font_size: options.font_size(),
        line_height_value: css::ComputedLineHeight::Number(default_line_height_multiplier),
        line_height: options.line_height(),
        line_height_multiplier: Some(default_line_height_multiplier),
        line_height_is_normal: false,
        color: Color::BLACK,
        ..ComputedStyle::initial()
    });
    let mut font_system = {
        let _timer = DebugTimer::start("finishing font system load");
        font_system_load.finish().await
    };
    let page_progression_direction = {
        let _timer = DebugTimer::start("resolving page progression direction");
        document_page_progression_direction(
            root,
            stylesheets,
            parent_style.as_ref(),
            &mut font_system,
        )
    };
    let page_counter_initial_values = {
        let _timer = DebugTimer::start("resolving page counter seeds");
        page_counter_initial_values(root, stylesheets, parent_style.as_ref(), &mut font_system)
    };
    let layout_result = layout_dom_with_font_system(
        root,
        stylesheets,
        options,
        base_url,
        root_url,
        resource_cache,
        parent_style,
        font_system,
        page_progression_direction,
        page_counter_initial_values,
    );
    match layout_result {
        LayoutResult::Document(document) => document,
        LayoutResult::Empty(font_system) => {
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

#[expect(
    clippy::too_many_arguments,
    reason = "This helper carries layout pipeline state out of the async frame."
)]
fn layout_dom_with_font_system(
    root: &Node,
    stylesheets: &[Stylesheet],
    options: &RenderOptions,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    resource_cache: &ResourceCache,
    parent_style: Box<ComputedStyle>,
    mut font_system: FontSystem,
    page_progression_direction: Direction,
    page_counter_initial_values: HashMap<String, i32>,
) -> LayoutResult {
    let _timer = DebugTimer::start("building and flowing page box content");
    let mut page_box = {
        let _timer = DebugTimer::start("building formatting box tree");
        Box::new(box_tree::build_page_box_with_font_metrics(
            root,
            stylesheets,
            parent_style.as_ref(),
            &mut font_system,
        ))
    };
    let mut builder = Box::new(LayoutBuilder::new(LayoutBuilderConfig {
        options,
        stylesheets,
        base_url,
        root_url,
        resource_cache,
        page_progression_direction,
        page_counter_initial_values,
        font_system,
    }));
    {
        let _timer = DebugTimer::start("resolving font-metric lengths");
        builder.resolve_font_metric_lengths_in_page_box(page_box.as_mut());
    }
    let page_box = {
        let _timer = DebugTimer::start("freezing formatting box tree");
        Box::new(box_tree::freeze_page_box(*page_box))
    };
    {
        let _timer = DebugTimer::start("flowing page box content");
        builder.layout_page_box(page_box.as_ref(), stylesheets);
    }
    if !builder.has_renderable_content() {
        LayoutResult::Empty(Box::new(builder.into_font_system()))
    } else {
        let _timer = DebugTimer::start("finalizing laid out document");
        LayoutResult::Document(builder.finish_boxed())
    }
}

/// Returns the document direction used for `@page :left`/`:right` matching.
///
/// CSS Paged Media defines spread pseudo-classes in terms of page progression;
/// for horizontal documents this follows the root element's `direction`:
/// <https://www.w3.org/TR/css-page-3/#spread-pseudos>.
pub(in crate::layout) fn document_page_progression_direction(
    root: &Node,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
    font_system: &mut FontSystem,
) -> Direction {
    let NodeKind::Element(root_element) = &root.kind else {
        return parent_style.direction;
    };
    let sibling_tags = element_sibling_signature_list(root_element);
    let element_index = 0usize;
    for child in &root_element.children {
        let NodeKind::Element(element) = &child.kind else {
            continue;
        };
        let signature = ElementSignature::with_sibling_list(
            element.tag.clone(),
            element.attrs.clone(),
            element_index,
            sibling_tags,
        );
        let parent_ch_advance = font_system.ch_advance(parent_style);
        let style = style_for_layout_element_with_parent_ch_advance(
            element,
            signature,
            stylesheets,
            Some(parent_style),
            &[],
            parent_ch_advance,
        );
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
pub(in crate::layout) fn page_counter_initial_values(
    root: &Node,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
    font_system: &mut FontSystem,
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
    let signature = element_signature(counter_element);
    let parent_ch_advance = font_system.ch_advance(parent_style);
    let style = style_for_layout_element_with_parent_ch_advance(
        counter_element,
        signature,
        stylesheets,
        Some(parent_style),
        &[],
        parent_ch_advance,
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

pub(in crate::layout) struct LayoutBuilder<'a> {
    pub(in crate::layout) options: &'a RenderOptions,
    pub(in crate::layout) stylesheets: &'a [Stylesheet],
    pub(in crate::layout) base_url: Option<&'a Path>,
    pub(in crate::layout) root_url: Option<&'a Path>,
    pub(in crate::layout) resource_cache: &'a ResourceCache,
    pub(in crate::layout) pages: Vec<Page>,
    pub(in crate::layout) page_names: Vec<Option<String>>,
    pub(in crate::layout) page_blanks: Vec<bool>,
    pub(in crate::layout) page_name_scope_suppression: usize,
    pub(in crate::layout) page_name_element_scope_suppression: usize,
    pub(in crate::layout) page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) page_anchors: HashMap<String, usize>,
    pub(in crate::layout) page_anchor_text: HashMap<String, AnchorText>,
    pub(in crate::layout) document_canvas_background: Option<ComputedStyle>,
    pub(in crate::layout) root_canvas_background_defined: bool,
    pub(in crate::layout) current_page: Page,
    pub(in crate::layout) current_page_has_flow_content: bool,
    pub(in crate::layout) last_block_layout_outcome: BlockLayoutOutcome,
    pub(in crate::layout) current_page_name: Option<String>,
    pub(in crate::layout) current_page_context: PageContext,
    pub(in crate::layout) cursor_y: f32,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_right: f32,
    pub(in crate::layout) content_logical_inline_size_stack: Vec<f32>,
    pub(in crate::layout) inline_static_position: Option<InlineStaticPosition>,
    pub(in crate::layout) text_box_line_trim_stack: Vec<TextBoxLineTrim>,
    /// Last prepared in-flow line baseline in the active layout coordinate space.
    ///
    /// CSS 2.2 defines an inline-block baseline from its last in-flow line box,
    /// not from whether that line emitted visible glyph paint. Keeping this as
    /// layout state lets transparent text and other non-painting lines still
    /// export the correct atomic inline baseline:
    /// <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>.
    pub(in crate::layout) last_in_flow_line_baseline_y: Option<f32>,
    pub(in crate::layout) block_static_position_y_offset: Option<f32>,
    pub(in crate::layout) absolute_static_position: Option<AbsoluteStaticPosition>,
    pub(in crate::layout) escaped_atom_positioning_depth: usize,
    pub(in crate::layout) escaped_atom_containing_block: Option<ContainingBlock>,
    pub(in crate::layout) containing_block_direction: Direction,
    pub(in crate::layout) containing_block_writing_mode: WritingMode,
    pub(in crate::layout) fragment_top_offsets: Vec<f32>,
    pub(in crate::layout) child_available_space_stack: Vec<ChildAvailableSpace>,
    pub(in crate::layout) definite_block_size_stack: Vec<Option<f32>>,
    pub(in crate::layout) truncate_page_start_margins: bool,
    pub(in crate::layout) avoid_inside_retry_depth: usize,
    pub(in crate::layout) out_of_flow_prebreak_suppression_depth: usize,
    pub(in crate::layout) element_side_effect_suppression_depth: usize,
    pub(in crate::layout) containing_blocks: Vec<ContainingBlock>,
    pub(in crate::layout) list_stack: Vec<ListState>,
    pub(in crate::layout) counter_set: CounterSet,
    pub(in crate::layout) quote_depth: usize,
    pub(in crate::layout) current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) current_page_running_elements:
        HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) next_assignment_id: usize,
    pub(in crate::layout) assignment_capture_stack: Vec<Vec<AssignmentId>>,
    pub(in crate::layout) ancestors: Vec<ElementSignature>,
    pub(in crate::layout) page_counter_initial_values: HashMap<String, i32>,
    pub(in crate::layout) page_rules: Vec<PageRule>,
    pub(in crate::layout) page_progression_direction: Direction,
    pub(in crate::layout) page_declarations: Declarations,
    pub(in crate::layout) page_margin_boxes: HashMap<String, Declarations>,
    pub(in crate::layout) counter_styles: HashMap<String, CounterStyleRule>,
    pub(in crate::layout) first_page_declarations: Declarations,
    pub(in crate::layout) font_system: Box<FontSystem>,
    pub(in crate::layout) bookmarks: Vec<Bookmark>,
    pub(in crate::layout) positioned_layers: Vec<PositionedPaintLayer>,
    pub(in crate::layout) fixed_layers: Vec<FixedPaintLayer>,
    pub(in crate::layout) next_paint_source_order: usize,
    pub(in crate::layout) overflow_clips: Vec<OverflowClip>,
    pub(in crate::layout) next_float_id: usize,
    pub(in crate::layout) float_contexts: Vec<FloatContext>,
    pub(in crate::layout) adjoining_float_origin_y: Option<f32>,
    pub(in crate::layout) pending_float_fragments: Vec<PendingFloatPaintFragment>,
    pub(in crate::layout) pending_float_side_effects: Vec<PendingFloatSideEffects>,
    pub(in crate::layout) applied_clearance_count: usize,
    pub(in crate::layout) preserve_scoped_paint_public_order: bool,
    pub(in crate::layout) defer_next_block_decoration_promotion: bool,
}

/// Pending block-container text-box trimming for one inline formatting context.
///
/// CSS Inline Layout Level 3 applies block-container `text-box-trim` to the
/// first and/or last formatted line inside that container:
/// <https://drafts.csswg.org/css-inline-3/#text-box-trim>.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct TextBoxLineTrim {
    pub(in crate::layout) trims_block_start: bool,
    pub(in crate::layout) trims_block_end: bool,
    pub(in crate::layout) block_start: f32,
    pub(in crate::layout) block_end: f32,
}

impl TextBoxLineTrim {
    pub(in crate::layout) fn is_empty(self) -> bool {
        !self.trims_block_start && !self.trims_block_end
    }
}

/// Tracks whether a block container's first formatted line is still pending.
///
/// CSS Pseudo-Elements Level 4 applies `::first-line` to the first formatted
/// line of the originating block container. CSS 2 anonymous block boxes created
/// around block-in-inline splits are layout artifacts, so they must not each
/// restart the originating element's typographic pseudo-element:
/// <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo> and
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::layout) struct FirstFormattedLineState {
    pending_typographic_pseudos: bool,
}

impl FirstFormattedLineState {
    pub(in crate::layout) fn for_style(style: &ComputedStyle) -> Self {
        Self {
            pending_typographic_pseudos: style.first_line_style.is_some()
                || style.first_letter_style.is_some(),
        }
    }

    pub(in crate::layout) fn applies_to_next_inline_run(self) -> bool {
        self.pending_typographic_pseudos
    }

    pub(in crate::layout) fn consume_next_formatted_line(&mut self) {
        self.pending_typographic_pseudos = false;
    }
}

/// Return a block style with originating `::first-line`/`::first-letter`
/// styling disabled for an anonymous inline sequence.
///
/// This preserves the normalized box tree while making first-line application a
/// layout-time decision, which is required when CSS 2 anonymous blocks are
/// generated by block-in-inline splitting:
/// <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo> and
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
pub(in crate::layout) fn style_without_typographic_first_line_pseudos(
    style: &ComputedStyle,
) -> Option<ComputedStyle> {
    if style.first_line_style.is_none() && style.first_letter_style.is_none() {
        return None;
    }
    let mut style = style.clone();
    style.first_line_style = None;
    style.first_letter_style = None;
    Some(style)
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct BlockLayoutOutcome {
    pub(in crate::layout) consumed_bottom_margin: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockEndMarginCollapse {
    pub(in crate::layout) child_consumed_margin: f32,
    pub(in crate::layout) collapsed_margin: f32,
}

pub(in crate::layout) struct LayoutBuilderConfig<'a> {
    pub(in crate::layout) options: &'a RenderOptions,
    pub(in crate::layout) stylesheets: &'a [Stylesheet],
    pub(in crate::layout) base_url: Option<&'a Path>,
    pub(in crate::layout) root_url: Option<&'a Path>,
    pub(in crate::layout) resource_cache: &'a ResourceCache,
    pub(in crate::layout) page_progression_direction: Direction,
    pub(in crate::layout) page_counter_initial_values: HashMap<String, i32>,
    pub(in crate::layout) font_system: FontSystem,
}

/// Insets of the active layout fragment from the current page area.
///
/// CSS Fragmentation keeps the fragmented box in its original formatting
/// context when content continues on another page, and CSS Paged Media selects
/// a new page area for each page box:
/// <https://www.w3.org/TR/css-break-3/#breaking-controls> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentOffsets {
    pub(in crate::layout) left: f32,
    pub(in crate::layout) right: f32,
    pub(in crate::layout) top: f32,
}

/// Tracks a legacy immediate float row view for callers that need the first
/// line's already-placed exclusions.
///
/// CSS 2.2 floats are shifted to the line's left or right edge and subsequent
/// floats are placed beside previous floats when space permits:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatRunState {
    /// Full physical row span before same-row floats shorten it.
    ///
    /// CSS 2.2 places consecutive floats beside earlier floats when possible.
    /// This span is page physical `x` in the current block formatting context:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) row_span: PageInlineSpan,
    /// Remaining physical row span after same-row floats have been included.
    ///
    /// This is the immediate line-box availability for legacy float placement
    /// callers; durable later exclusions are stored as [`FloatShape`] entries
    /// in [`FloatContext`].
    pub(in crate::layout) available_span: PageInlineSpan,
    /// Physical block interval occupied by same-row floats.
    ///
    /// The span uses Quire's page top-edge convention: `top_y` is the row top
    /// and `bottom_y` moves downward as floats are added. CSS floats shorten
    /// later line boxes until the lowest same-row float bottom:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) occupied_block_span: PageBlockSpan,
    pub(in crate::layout) active: bool,
}

/// Durable float exclusion list for one block formatting context.
///
/// CSS 2.2 keeps floated margin boxes out of normal flow but shortens later
/// line boxes and formatting contexts around them in the same block formatting
/// context:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FloatContext {
    pub(in crate::layout) shapes: Vec<FloatShape>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct FloatId(pub(in crate::layout) usize);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatShape {
    pub(in crate::layout) id: FloatId,
    pub(in crate::layout) specified_side: Float,
    pub(in crate::layout) side: UsedFloatSide,
    pub(in crate::layout) source_order: usize,
    pub(in crate::layout) fragment_index: usize,
    pub(in crate::layout) starts_on_previous_page: bool,
    pub(in crate::layout) continues_on_next_page: bool,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) rect: PageTopRect,
}

impl FloatShape {
    pub(in crate::layout) fn from_rect(
        id: FloatId,
        specified_side: Float,
        side: UsedFloatSide,
        source_order: usize,
        page_index: usize,
        rect: PageTopRect,
    ) -> Self {
        Self {
            id,
            specified_side,
            side,
            source_order,
            fragment_index: 0,
            starts_on_previous_page: false,
            continues_on_next_page: false,
            page_index,
            rect,
        }
    }

    pub(in crate::layout) fn from_fragment(fragment: &FloatPaintFragment) -> Self {
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
    pub(in crate::layout) fn from_edges(
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

    pub(in crate::layout) fn left(self) -> f32 {
        self.rect.x
    }

    pub(in crate::layout) fn right(self) -> f32 {
        self.rect.x + self.rect.width
    }

    pub(in crate::layout) fn top(self) -> f32 {
        self.rect.top_y
    }

    pub(in crate::layout) fn bottom(self) -> f32 {
        self.rect.bottom_y()
    }

    pub(in crate::layout) fn translated_block(self, delta_y: f32) -> Self {
        Self {
            rect: PageTopRect::new(
                self.rect.x,
                self.rect.top_y + delta_y,
                self.rect.width,
                self.rect.height,
            ),
            ..self
        }
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
pub(in crate::layout) struct FloatPaintFragment {
    pub(in crate::layout) id: FloatId,
    pub(in crate::layout) specified_side: Float,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) side: UsedFloatSide,
    pub(in crate::layout) rect: PageTopRect,
    pub(in crate::layout) source_order: usize,
    pub(in crate::layout) fragment_index: usize,
    pub(in crate::layout) starts_on_previous_page: bool,
    pub(in crate::layout) continues_on_next_page: bool,
    pub(in crate::layout) context: PaintStackingContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum UsedFloatSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl UsedFloatSide {
    pub(in crate::layout) fn from_float(
        float: Float,
        writing_mode: WritingMode,
        direction: Direction,
    ) -> Option<Self> {
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

    pub(in crate::layout) fn from_physical_side(side: PhysicalSide) -> Self {
        match side {
            PhysicalSide::Left => Self::Left,
            PhysicalSide::Right => Self::Right,
            PhysicalSide::Top => Self::Top,
            PhysicalSide::Bottom => Self::Bottom,
        }
    }

    pub(in crate::layout) fn matches_clear(
        self,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
    ) -> bool {
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
pub(in crate::layout) struct PendingFloatPaintFragment {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) fragment: PaintFragment,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(in crate::layout) struct PendingFloatSideEffects {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) running_elements: HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) links: Vec<RenderedLink>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(in crate::layout) struct FloatLayoutSideEffects {
    pub(in crate::layout) bookmarks: Vec<Bookmark>,
    pub(in crate::layout) anchors: Vec<(String, usize)>,
    pub(in crate::layout) anchor_text: Vec<(String, AnchorText)>,
    pub(in crate::layout) page_effects: Vec<PendingFloatSideEffects>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatBand {
    /// The remaining physical line-box span in page coordinates after active
    /// CSS floats have shortened the row.
    ///
    /// CSS 2.2 defines floats as shortening line boxes in the same block
    /// formatting context. The span is physical page `x`, not logical inline
    /// coordinates; vertical writing modes must use [`LogicalFloatBand`]:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) span: PageInlineSpan,
}

impl FloatBand {
    pub(in crate::layout) fn from_edges(left: f32, right: f32) -> Self {
        Self {
            span: PageInlineSpan::from_edges(left, right),
        }
    }

    pub(in crate::layout) fn left(self) -> f32 {
        self.span.left_x()
    }

    pub(in crate::layout) fn right(self) -> f32 {
        self.span.right_x()
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.span.width()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct LogicalFloatBand {
    /// Available logical inline interval after float exclusions.
    ///
    /// CSS Writing Modes defines inline coordinates independently from the
    /// physical page axis. This span is logical inline progress inside the
    /// queried line/slab, after active CSS floats have shortened it:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(in crate::layout) inline_span: LogicalInlineSpan,
    /// Physical page-y interval that corresponds to the available inline slab.
    ///
    /// Vertical writing modes can shorten the physical top or bottom of the
    /// slab while still reporting a logical inline span to inline layout.
    pub(in crate::layout) block_span: PageBlockSpan,
}
