use super::*;
use std::collections::HashSet;

/// Paint captured speculatively for a fragmentainer that normal flow has not
/// reached yet. Fragmented floats and fixed-size overflow use the same queue.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct PendingPaintFragment {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) fragment: PaintFragment,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(in crate::layout) struct PendingPageSideEffects {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) running_elements: HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) links: Vec<RenderedLink>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(in crate::layout) struct DeferredLayoutSideEffects {
    pub(in crate::layout) bookmarks: Vec<Bookmark>,
    pub(in crate::layout) anchors: Vec<(String, usize)>,
    pub(in crate::layout) anchor_text: Vec<(String, AnchorText)>,
    pub(in crate::layout) page_effects: Vec<PendingPageSideEffects>,
}

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
    let (overflow_x, overflow_y) = resolved_overflow_axes(style);
    match block_start_side(style.writing_mode).axis() {
        PhysicalAxis::Horizontal => !overflow_x.is_scrollable(),
        PhysicalAxis::Vertical => !overflow_y.is_scrollable(),
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
/// A physical page size expressed in PDF points.
///
/// ```
/// let page_size = quire::PageSize::from_points(612.0, 792.0);
/// assert_eq!(page_size.width(), 612.0);
/// ```
pub struct PageSize {
    pub(crate) width: LayoutLength,
    pub(crate) height: LayoutLength,
}

pub(in crate::layout) const LIST_ITEM_COUNTER_NAME: &str = "list-item";

impl PageSize {
    /// The ISO A4 page size in PDF points.
    pub const A4_POINTS: Self = Self {
        width: layout_pt(595.2756),
        height: layout_pt(841.8898),
    };

    /// Creates a page size from width and height measured in PDF points.
    ///
    /// ```
    /// let page_size = quire::PageSize::from_points(595.2756, 841.8898);
    /// assert_eq!(page_size.height(), 841.8898);
    /// ```
    pub const fn from_points(width: f32, height: f32) -> Self {
        Self {
            width: layout_pt(width),
            height: layout_pt(height),
        }
    }

    /// Returns the page width in PDF points.
    ///
    /// ```
    /// let page_size = quire::PageSize::from_points(612.0, 792.0);
    /// assert_eq!(page_size.width(), 612.0);
    /// ```
    pub fn width(&self) -> f32 {
        layout_points(self.width)
    }

    /// Returns the page height in PDF points.
    ///
    /// ```
    /// let page_size = quire::PageSize::from_points(612.0, 792.0);
    /// assert_eq!(page_size.height(), 792.0);
    /// ```
    pub fn height(&self) -> f32 {
        layout_points(self.height)
    }

    /// Returns the physical page size as a layout-space viewport size.
    pub(crate) fn layout_size(self) -> crate::units::LayoutSize {
        crate::units::LayoutSize::new(layout_points(self.width), layout_points(self.height))
    }

    /// A paged-media sheet needs a positive extent on both physical axes.
    ///
    /// CSS Paged Media falls back to the initial page size when an authored
    /// `@page size` resolves to a zero-sized sheet; accepting it would create
    /// a fragmentainer that can never make forward layout progress.
    /// <https://www.w3.org/TR/css-page-3/#page-size-prop>
    pub(crate) fn has_positive_area(self) -> bool {
        self.width().is_finite()
            && self.height().is_finite()
            && self.width() > 0.0
            && self.height() > 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Page margins expressed in PDF points.
///
/// ```
/// let margins = quire::PageMargins::all_points(36.0);
/// assert_eq!(margins.left(), 36.0);
/// ```
pub struct PageMargins {
    pub(crate) top: LayoutLength,
    pub(crate) right: LayoutLength,
    pub(crate) bottom: LayoutLength,
    pub(crate) left: LayoutLength,
}

impl PageMargins {
    /// WeasyPrint's default page margin in PDF points.
    pub const WEASYPRINT_DEFAULT_POINTS: f32 = 56.25;
    /// The default margin on every page edge.
    pub const DEFAULT: Self = Self {
        top: layout_pt(Self::WEASYPRINT_DEFAULT_POINTS),
        right: layout_pt(Self::WEASYPRINT_DEFAULT_POINTS),
        bottom: layout_pt(Self::WEASYPRINT_DEFAULT_POINTS),
        left: layout_pt(Self::WEASYPRINT_DEFAULT_POINTS),
    };

    /// Creates equal margins on all page edges.
    pub(crate) const fn all(length: LayoutLength) -> Self {
        Self {
            top: length,
            right: length,
            bottom: length,
            left: length,
        }
    }

    /// Creates equal margins from a PDF-point value.
    ///
    /// ```
    /// let margins = quire::PageMargins::all_points(18.0);
    /// assert_eq!(margins.top(), 18.0);
    /// ```
    pub const fn all_points(value: f32) -> Self {
        Self::all(layout_pt(value))
    }

    /// Creates margins from top, right, bottom, and left PDF-point values.
    ///
    /// ```
    /// let margins = quire::PageMargins::from_points(18.0, 24.0, 36.0, 48.0);
    /// assert_eq!(margins.bottom(), 36.0);
    /// ```
    pub const fn from_points(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top: layout_pt(top),
            right: layout_pt(right),
            bottom: layout_pt(bottom),
            left: layout_pt(left),
        }
    }

    /// Returns the top margin in PDF points.
    ///
    /// ```
    /// let margins = quire::PageMargins::all_points(18.0);
    /// assert_eq!(margins.top(), 18.0);
    /// ```
    pub fn top(&self) -> f32 {
        layout_points(self.top)
    }

    /// Returns the right margin in PDF points.
    ///
    /// ```
    /// let margins = quire::PageMargins::all_points(18.0);
    /// assert_eq!(margins.right(), 18.0);
    /// ```
    pub fn right(&self) -> f32 {
        layout_points(self.right)
    }

    /// Returns the bottom margin in PDF points.
    ///
    /// ```
    /// let margins = quire::PageMargins::all_points(18.0);
    /// assert_eq!(margins.bottom(), 18.0);
    /// ```
    pub fn bottom(&self) -> f32 {
        layout_points(self.bottom)
    }

    /// Returns the left margin in PDF points.
    ///
    /// ```
    /// let margins = quire::PageMargins::all_points(18.0);
    /// assert_eq!(margins.left(), 18.0);
    /// ```
    pub fn left(&self) -> f32 {
        layout_points(self.left)
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Inputs that control document parsing, cascade, and layout.
///
/// ```
/// let options = quire::RenderOptions::default();
/// assert!(options.font_size() > 0.0);
/// ```
pub struct RenderOptions {
    /// Output medium and viewport used by CSS Media Queries.
    pub media_type: crate::css::MediaType,
    /// Forced-colors palette used for CSS CssColor Adjustment and media queries.
    pub forced_colors: crate::css::ForcedColorsMode,
    /// Device pixel density exposed to CSS resolution media queries and used
    /// to select CSS Images `image-set()` candidates.
    pub(crate) device_resolution_dppx: f32,
    /// The physical page size.
    pub page_size: PageSize,
    /// A compatibility margin applied uniformly unless `page_margins` is set.
    pub(crate) margin: LayoutLength,
    /// The per-edge page margins.
    pub page_margins: PageMargins,
    /// The initial font size in layout units.
    pub(crate) font_size: LayoutLength,
    /// The initial line height in layout units.
    pub(crate) line_height: LayoutLength,
    /// Apply HTML presentational hints as zero-specificity author CSS.
    ///
    /// HTML maps these legacy attributes into the CSS cascade as
    /// presentational hints. They are enabled by default for HTML documents;
    /// callers that need CSS-only compatibility can explicitly disable them:
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
    /// Returns the configured device density in CSS dots per pixel.
    pub fn device_resolution_dppx(&self) -> f32 {
        self.device_resolution_dppx
    }

    /// Sets the device density used by CSS resolution-dependent features.
    ///
    /// A static PDF has no intrinsic screen density, so callers choose this
    /// rendering input explicitly when higher-density image assets are wanted.
    pub fn set_device_resolution_dppx(&mut self, resolution_dppx: f32) -> crate::Result<()> {
        if !resolution_dppx.is_finite() || resolution_dppx <= 0.0 {
            return Err(crate::Error::InvalidInput(
                "device resolution must be finite and greater than zero".to_string(),
            ));
        }
        self.device_resolution_dppx = resolution_dppx;
        Ok(())
    }
    /// Sets an equal margin on all page edges.
    pub(crate) fn set_margin(&mut self, margin: LayoutLength) {
        self.margin = margin;
        self.page_margins = PageMargins::all(margin);
    }

    /// Sets an equal margin on all page edges from a PDF-point value.
    ///
    /// ```
    /// let mut options = quire::RenderOptions::default();
    /// options.set_margin_points(24.0);
    /// assert_eq!(options.margin(), 24.0);
    /// ```
    pub fn set_margin_points(&mut self, margin: f32) {
        self.set_margin(layout_pt(margin));
    }

    /// Sets the per-edge page margins.
    ///
    /// ```
    /// let mut options = quire::RenderOptions::default();
    /// options.set_page_margins(quire::PageMargins::all_points(24.0));
    /// assert_eq!(options.page_margins().top(), 24.0);
    /// ```
    pub fn set_page_margins(&mut self, margins: PageMargins) {
        self.margin = margins.top;
        self.page_margins = margins;
    }

    /// Returns the compatibility margin in PDF points.
    ///
    /// ```
    /// let options = quire::RenderOptions::default();
    /// assert!(options.margin() > 0.0);
    /// ```
    pub fn margin(&self) -> f32 {
        layout_points(self.margin)
    }

    /// Returns the initial font size in PDF points.
    ///
    /// ```
    /// let options = quire::RenderOptions::default();
    /// assert!(options.font_size() > 0.0);
    /// ```
    pub fn font_size(&self) -> f32 {
        layout_points(self.font_size)
    }

    /// Returns the initial line height in PDF points.
    ///
    /// ```
    /// let options = quire::RenderOptions::default();
    /// assert!(options.line_height() > 0.0);
    /// ```
    pub fn line_height(&self) -> f32 {
        layout_points(self.line_height)
    }

    /// Returns the effective per-edge page margins.
    ///
    /// ```
    /// let options = quire::RenderOptions::default();
    /// assert!(options.page_margins().top() > 0.0);
    /// ```
    pub fn page_margins(&self) -> PageMargins {
        if self.page_margins == PageMargins::DEFAULT
            && (self.margin() - PageMargins::WEASYPRINT_DEFAULT_POINTS).abs() > 0.01
        {
            PageMargins::all(self.margin)
        } else {
            self.page_margins
        }
    }

    pub(crate) fn media_environment(&self) -> crate::css::MediaEnvironment {
        crate::css::MediaEnvironment::new(
            self.media_type,
            crate::css::CssViewportSize::new(
                self.page_size.width() / crate::css::CSS_PX_TO_PT,
                self.page_size.height() / crate::css::CSS_PX_TO_PT,
            ),
        )
        .with_resolution_dppx(self.device_resolution_dppx)
        .with_forced_colors(self.forced_colors)
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        let font_size = 12.0;
        Self {
            media_type: crate::css::MediaType::Print,
            forced_colors: crate::css::ForcedColorsMode::Inactive,
            device_resolution_dppx: 1.0,
            page_size: PageSize::A4_POINTS,
            margin: layout_pt(PageMargins::WEASYPRINT_DEFAULT_POINTS),
            page_margins: PageMargins::DEFAULT,
            font_size: layout_pt(font_size),
            line_height: layout_pt(font_size * 1.2),
            presentational_hints: true,
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

    /// Returns the physical page-area extent used as the logical inline size
    /// for a formatting context in `writing_mode`.
    ///
    /// Page boxes are physical rectangles, while a formatting context's
    /// percentage and fragmentation bases are logical. Keeping this mapping
    /// on the page context makes the initial page area explicit rather than
    /// letting vertical flows accidentally inherit the physical width basis:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(in crate::layout) fn logical_inline_size(self, writing_mode: WritingMode) -> f32 {
        if WritingModeAxes::new(writing_mode, Direction::Ltr).swaps_physical_axes() {
            self.area_height()
        } else {
            self.area_width()
        }
    }

    /// Returns the physical page-area extent used as the logical block size
    /// for a formatting context in `writing_mode`.
    ///
    /// This is deliberately separate from `area_height()`: in a vertical or
    /// sideways flow, page fragmentation progresses across physical width.
    /// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
    #[cfg(test)]
    pub(in crate::layout) fn logical_block_size(self, writing_mode: WritingMode) -> f32 {
        if WritingModeAxes::new(writing_mode, Direction::Ltr).swaps_physical_axes() {
            self.area_width()
        } else {
            self.area_height()
        }
    }
}

pub(crate) fn start_font_system_load() -> FontSystemLoad {
    FontSystem::start_loading()
}

pub(in crate::layout) enum LayoutResult {
    Document(LayoutPass),
}

/// Output from one complete fresh layout pass.
pub(in crate::layout) struct LayoutPass {
    pub(in crate::layout) document: Document,
    pub(in crate::layout) target_references: TargetReferenceSnapshot,
    pub(in crate::layout) has_normal_flow_target_references: bool,
}

/// Lay out an already prepared DOM with an already loaded font system.
///
/// This synchronous phase is reusable for nested browsing contexts: the
/// parent first measures an iframe's content-box viewport, then lays out the
/// isolated child document against that concrete viewport before final parent
/// paint composition.
/// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
pub(crate) struct PreparedDomLayout<'a> {
    pub(crate) root: &'a Node,
    pub(crate) stylesheets: Stylesheets<'a>,
    pub(crate) options: &'a RenderOptions,
    pub(crate) base_url: Option<&'a url::Url>,
    pub(crate) root_url: Option<&'a url::Url>,
    pub(crate) resource_cache: &'a ResourceCache,
    pub(crate) iframe_documents: &'a HashMap<crate::dom::ElementId, Document>,
    /// The finite viewport of an iframe whose static contents use an
    /// unfragmented layout canvas.
    pub(crate) iframe_viewport: Option<PageSize>,
    pub(crate) font_system: FontSystem,
}

pub(crate) fn layout_prepared_dom(config: PreparedDomLayout<'_>) -> Document {
    let PreparedDomLayout {
        root,
        stylesheets,
        options,
        base_url,
        root_url,
        resource_cache,
        iframe_documents,
        iframe_viewport,
        mut font_system,
    } = config;
    let _timer = DebugTimer::start("layout pipeline");
    let default_line_height_multiplier = if options.font_size() > 0.0 {
        options.line_height() / options.font_size()
    } else {
        1.2
    };
    let parent_style = Box::new(ComputedStyle {
        font_size: options.font_size(),
        deferred_font_size: css::DeferredFontSize::Absolute(options.font_size()),
        line_height_value: css::ComputedLineHeight::Number(default_line_height_multiplier),
        line_height: options.line_height(),
        color: CssColor::BLACK,
        ..ComputedStyle::initial()
    });
    resource_cache.set_inline_svg_presentation_overrides(inline_svg_presentation_overrides(
        root,
        &stylesheets,
        parent_style.as_ref(),
    ));
    let mut page_margin_inherited_style = {
        let _timer = DebugTimer::start("building deferred page-margin inheritance");
        document_root_style(root, &stylesheets, parent_style.as_ref())
    };
    let page_box = {
        let _timer = DebugTimer::start("building deferred formatting box tree");
        Box::new(box_tree::build_page_box(
            root,
            &stylesheets,
            parent_style.as_ref(),
        ))
    };
    // Resolve every root/body document-canvas special case from the same
    // cascaded formatting tree. This remains immutable while font metrics
    // and layout-only principal-flow values are subsequently resolved.
    let document_canvas_resolution = DocumentCanvasResolution::from_page_box(page_box.as_ref());
    let principal_flow = document_canvas_resolution.principal_flow();
    let page_progression_direction = principal_flow.used_direction();
    let parent_ch_advance = if page_margin_inherited_style
        .deferred_font_size
        .requires_parent_ch_advance(parent_style.font_size)
    {
        font_system.ch_advance(parent_style.as_ref())
    } else {
        css::fallback_ch_advance_for_style(parent_style.as_ref())
    };
    page_margin_inherited_style.resolve_deferred_font_size(css::FontRelativeLengthBasis::new(
        layout_pt(parent_style.font_size),
        parent_ch_advance,
    ));
    let root_ch_advance = if page_margin_inherited_style.requires_ch_advance() {
        font_system.ch_advance(&page_margin_inherited_style)
    } else {
        css::fallback_ch_advance_for_style(&page_margin_inherited_style)
    };
    page_margin_inherited_style.resolve_font_metric_lengths(root_ch_advance);
    // Cross-reference values are determined at the target end of a link and
    // can therefore change the size of normal-flow generated content. Rebuild
    // the immutable formatting tree for each pass so the resolved text takes
    // part in line selection and fragmentation instead of being patched into
    // paint output. CSS Generated Content Level 3 intentionally permits page
    // counters in these references: <https://www.w3.org/TR/css-content-3/#target-counter>.
    const MAX_TARGET_REFERENCE_PASSES: usize = 8;
    let mut target_references = TargetReferenceSnapshot::default();
    let mut seen_target_snapshots = Vec::new();
    let mut last_pass = None;
    for pass_index in 0..MAX_TARGET_REFERENCE_PASSES {
        let page_box = Box::new(box_tree::build_page_box(
            root,
            &stylesheets,
            parent_style.as_ref(),
        ));
        let LayoutResult::Document(pass) = layout_dom_with_font_system(
            root,
            &stylesheets,
            options,
            base_url,
            root_url,
            resource_cache,
            iframe_documents,
            iframe_viewport,
            parent_style.clone(),
            font_system.clone(),
            page_progression_direction,
            principal_flow,
            document_canvas_resolution,
            page_margin_inherited_style.clone(),
            page_box,
            target_references.clone(),
        );
        if !pass.has_normal_flow_target_references || pass.target_references == target_references {
            return pass.document;
        }
        if seen_target_snapshots
            .iter()
            .any(|previous| previous == &pass.target_references)
        {
            log::warn!(
                "normal-flow generated target references did not converge after {} layout passes; retaining the last complete pass",
                pass_index + 1
            );
            return pass.document;
        }
        seen_target_snapshots.push(target_references);
        target_references = pass.target_references.clone();
        last_pass = Some(pass);
    }
    log::warn!(
        "normal-flow generated target references exceeded {MAX_TARGET_REFERENCE_PASSES} layout passes; retaining the last complete pass"
    );
    last_pass
        .expect("a target-reference layout loop always produces a first pass")
        .document
}

/// Cascades document CSS onto inline SVG descendants that are painted by the
/// SVG scene adapter.
///
/// Inline SVG establishes one atomic HTML layout box, but descendants remain
/// elements of the host document for CSS selector matching. The scene adapter
/// receives a standalone SVG payload, so this pass carries the selected CSS
/// `transform` result across that boundary while leaving the source DOM
/// untouched. CSS Transforms Level 1 defines SVG transform application in
/// §7.3; SVG 2 defines `transform` presentation attributes in §6.6.
fn inline_svg_presentation_overrides(
    root: &Node,
    stylesheets: &Stylesheets<'_>,
    parent_style: &ComputedStyle,
) -> crate::svg::SvgPresentationOverrides {
    let NodeKind::Element(root_element) = &root.kind else {
        return crate::svg::SvgPresentationOverrides::new();
    };
    let mut overrides = HashMap::new();
    let forced_color_palette = stylesheets
        .iter()
        .find_map(|stylesheet| stylesheet.forced_colors.palette());
    let svg_source_has_system_color = inline_svg_has_system_color(root);
    let sibling_tags = element_sibling_signature_list(root_element);
    let mut element_index = 0;
    for child in &root_element.children {
        let NodeKind::Element(element) = &child.kind else {
            continue;
        };
        let signature = ElementSignature::with_sibling_list(
            element.tag.clone(),
            element.attrs.clone(),
            element_index,
            sibling_tags.clone(),
        );
        element_index += 1;
        collect_inline_svg_presentation_overrides(
            element,
            signature,
            stylesheets,
            parent_style,
            &[],
            false,
            forced_color_palette,
            svg_source_has_system_color,
            &mut overrides,
        );
    }
    overrides
}

#[allow(clippy::too_many_arguments)]
fn collect_inline_svg_presentation_overrides(
    element: &Element,
    signature: ElementSignature,
    stylesheets: &Stylesheets<'_>,
    parent_style: &ComputedStyle,
    ancestors: &[ElementSignature],
    inside_inline_svg: bool,
    forced_color_palette: Option<css::ForcedColorPalette>,
    svg_source_has_system_color: bool,
    overrides: &mut crate::svg::SvgPresentationOverrides,
) {
    let style = style_for_layout_element(
        element,
        signature.clone(),
        stylesheets,
        Some(parent_style),
        ancestors,
    );
    let signature = layout_element_signature(element, signature, Some(parent_style));
    let enters_inline_svg = !inside_inline_svg
        && element.namespace_url == "http://www.w3.org/2000/svg"
        && element.tag == "svg";
    // The inline SVG root also needs host CSS presentation paint. Its layout
    // box handles CSS transforms separately, but fill/stroke establish the
    // inherited SVG paint for its scene descendants.
    let applies_to_svg_scene = inside_inline_svg || enters_inline_svg;
    if applies_to_svg_scene && svg_transformable_element(element) {
        let transform = (!enters_inline_svg && style.has_transform())
            .then(|| svg_css_transform_is_resolvable(&style))
            .filter(|resolvable| *resolvable)
            .and_then(|_| svg_css_transform_attribute_for_element(&style, element));
        let force_colors = forced_color_palette.filter(|_| {
            style.forced_color_adjust == css::ForcedColorAdjust::Auto
                && !svg_source_has_system_color
        });
        let presentation = crate::svg::SvgPresentationOverride {
            display: svg_display_override(element, &style, enters_inline_svg),
            transform,
            fill: force_colors
                .map(|palette| svg_presentation_paint(Some(palette.canvas_text)))
                .or_else(|| {
                    style
                        .svg_fill
                        .is_overridden()
                        .then(|| svg_presentation_paint(style.svg_fill.paint.resolve(style.color)))
                }),
            stroke: force_colors
                .filter(|_| !matches!(style.svg_stroke.paint, css::SvgPaint::None))
                .map(|palette| svg_presentation_paint(Some(palette.canvas_text)))
                .or_else(|| {
                    style.svg_stroke.is_overridden().then(|| {
                        svg_presentation_paint(style.svg_stroke.paint.resolve(style.color))
                    })
                })
                .or_else(|| {
                    enters_inline_svg.then(|| {
                        svg_presentation_paint(style.svg_stroke.paint.resolve(style.color))
                    })
                }),
            stroke_width: style.svg_stroke_width.is_overridden().then(|| {
                format!(
                    "{}px",
                    style.svg_stroke_width.value().length_points() / css::CSS_PX_TO_PT
                )
            }),
            remove_filter: force_colors.is_some()
                && (element.attrs.contains_key("filter")
                    || element
                        .attrs
                        .get("style")
                        .is_some_and(|style| style.to_ascii_lowercase().contains("filter"))),
        };
        if presentation.display.is_some()
            || presentation.transform.is_some()
            || presentation.fill.is_some()
            || presentation.stroke.is_some()
            || presentation.stroke_width.is_some()
        {
            overrides.insert(element.id, presentation);
        }
    }

    let mut child_ancestors = ancestors.to_vec();
    child_ancestors.push(signature);
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0;
    for child in &element.children {
        let NodeKind::Element(child) = &child.kind else {
            continue;
        };
        let signature = ElementSignature::with_sibling_list(
            child.tag.clone(),
            child.attrs.clone(),
            element_index,
            sibling_tags.clone(),
        );
        element_index += 1;
        collect_inline_svg_presentation_overrides(
            child,
            signature,
            stylesheets,
            &style,
            &child_ancestors,
            inside_inline_svg || enters_inline_svg,
            forced_color_palette,
            svg_source_has_system_color,
            overrides,
        );
    }
}

/// Map CSS `display` onto the standalone inline-SVG scene.
///
/// An inline SVG root in HTML has CSS box layout and is suppressed by
/// `display: contents`; nested SVG containers, SVG text-content children, and
/// `<use>` can instead be unboxed. Other SVG elements are suppressed because
/// hoisting them would change their rendering context.
/// <https://drafts.csswg.org/css-display-3/#unbox-svg>
fn svg_display_override(
    element: &Element,
    style: &ComputedStyle,
    enters_inline_svg: bool,
) -> Option<crate::svg::SvgDisplayOverride> {
    if style.display.is_none() {
        return Some(crate::svg::SvgDisplayOverride::None);
    }
    if !style.display.is_contents() {
        return None;
    }
    if enters_inline_svg {
        return Some(crate::svg::SvgDisplayOverride::None);
    }
    match element.tag.as_str() {
        // Renderable container elements and text-content children can be
        // hoisted into their parent's SVG formatting context.
        "svg" | "g" | "a" | "switch" | "tspan" | "textPath" => {
            Some(crate::svg::SvgDisplayOverride::Contents)
        }
        // `<use>` exposes its shadow-tree content. Retain the reference while
        // discarding the stripped element's own non-inherited style.
        "use" => Some(crate::svg::SvgDisplayOverride::UseContents),
        _ => Some(crate::svg::SvgDisplayOverride::None),
    }
}

fn inline_svg_has_system_color(node: &Node) -> bool {
    const SYSTEM_COLORS: &[&str] = &[
        "accentcolor",
        "accentcolortext",
        "activetext",
        "buttonborder",
        "buttonface",
        "buttontext",
        "canvas",
        "canvastext",
        "field",
        "fieldtext",
        "graytext",
        "highlight",
        "highlighttext",
        "linktext",
        "mark",
        "marktext",
        "selecteditem",
        "selecteditemtext",
        "visitedtext",
        "window",
        "windowtext",
    ];
    let NodeKind::Element(element) = &node.kind else {
        return false;
    };
    element.attrs.values().any(|value| {
        SYSTEM_COLORS
            .iter()
            .any(|system_color| value.eq_ignore_ascii_case(system_color))
    }) || element.children.iter().any(inline_svg_has_system_color)
}

/// Serialize a computed host-CSS SVG paint as a legacy-compatible SVG color.
/// `usvg` resolves the final presentation attributes after CSS cascade, so a
/// concrete RGBA spelling avoids reparsing document selectors in its scene.
fn svg_presentation_paint(color: Option<CssColor>) -> String {
    let Some(color) = color else {
        return "none".to_owned();
    };
    format!(
        "rgba({}, {}, {}, {})",
        (color.components()[0] * 255.0).round().clamp(0.0, 255.0),
        (color.components()[1] * 255.0).round().clamp(0.0, 255.0),
        (color.components()[2] * 255.0).round().clamp(0.0, 255.0),
        color.alpha()
    )
}

fn svg_transformable_element(element: &Element) -> bool {
    element.namespace_url == "http://www.w3.org/2000/svg"
        && matches!(
            element.tag.as_str(),
            "a" | "circle"
                | "ellipse"
                | "foreignObject"
                | "g"
                | "image"
                | "line"
                | "path"
                | "polygon"
                | "polyline"
                | "rect"
                | "svg"
                | "switch"
                | "text"
                | "use"
        )
}

/// Resolve CSS transforms on basic SVG graphics using the selected geometry
/// reference box. The scene serializer receives the resulting typed affine
/// SVG matrix rather than a CSS string override, so percentage translations
/// and origins use the same box.
fn svg_css_transform_attribute_for_element(
    style: &ComputedStyle,
    element: &Element,
) -> Option<String> {
    let Some((x, y, width, height)) = svg_rect_transform_reference_box(element, style) else {
        return svg_css_transform_attribute_without_reference_box(style);
    };
    let origin_x = svg_css_used_length_in_user_units(style.transform_origin.x.clone(), width) + x;
    let origin_y = svg_css_used_length_in_user_units(style.transform_origin.y.clone(), height) + y;
    let transform = crate::layout::assets::compose_css_transform_matrix(
        crate::svg::SvgSourcePoint::new(origin_x, origin_y),
        style.individual_transforms.clone(),
        &style.transform,
        |function| svg_css_transform_function_matrix_for_box(function, width, height),
    );
    Some(format!(
        "matrix({} {} {} {} {} {})",
        transform.m11, transform.m12, transform.m21, transform.m22, transform.m31, transform.m32
    ))
}

/// Convert a transform which has no dependency on an SVG reference box.
///
/// Layout does not retain enough SVG viewport geometry at this bridge for all
/// SVG element types.  Absolute transforms with an initial or absolute origin
/// are nevertheless fully determined, including on a root `<svg>` whose
/// width/height attributes use CSS units.  Preserving this path avoids
/// dropping valid transforms merely because its source geometry is not a
/// plain unitless `<rect>`.
fn svg_css_transform_attribute_without_reference_box(style: &ComputedStyle) -> Option<String> {
    let origin_is_resolvable = style.transform_origin == css::TransformOrigin::INITIAL
        || (svg_css_length_is_absolute(&style.transform_origin.x)
            && svg_css_length_is_absolute(&style.transform_origin.y));
    let transforms_are_resolvable =
        style
            .individual_transforms
            .translate
            .as_ref()
            .is_none_or(|translation| {
                svg_css_length_is_absolute(&translation.x)
                    && svg_css_length_is_absolute(&translation.y)
            })
            && style.transform.iter().all(|function| match function {
                css::TransformFunction::Translate(translation) => {
                    svg_css_length_is_absolute(&translation.x)
                        && svg_css_length_is_absolute(&translation.y)
                }
                css::TransformFunction::Matrix(_)
                | css::TransformFunction::Scale(_)
                | css::TransformFunction::Rotate(..)
                | css::TransformFunction::Skew(_) => true,
                css::TransformFunction::Matrix3D(_)
                | css::TransformFunction::Translate3D(_)
                | css::TransformFunction::Scale3D(_)
                | css::TransformFunction::Rotate3D(_)
                | css::TransformFunction::Perspective(_) => false,
            });
    if !(origin_is_resolvable && transforms_are_resolvable) {
        return None;
    }
    let origin = if style.transform_origin == css::TransformOrigin::INITIAL {
        crate::svg::SvgSourcePoint::new(0.0, 0.0)
    } else {
        crate::svg::SvgSourcePoint::new(
            svg_css_length_in_user_units(style.transform_origin.x.clone()),
            svg_css_length_in_user_units(style.transform_origin.y.clone()),
        )
    };
    let transform = crate::layout::assets::compose_css_transform_matrix(
        origin,
        style.individual_transforms.clone(),
        &style.transform,
        svg_css_transform_function_matrix,
    );
    Some(format!(
        "matrix({} {} {} {} {} {})",
        transform.m11, transform.m12, transform.m21, transform.m22, transform.m31, transform.m32
    ))
}

/// Basic SVG fill-box support for CSS transforms. `stroke-box` and `view-box`
/// remain scene-level work because they require stroke and nested-viewport
/// geometry that is not preserved by this DOM bridge.
fn svg_rect_transform_reference_box(
    element: &Element,
    style: &ComputedStyle,
) -> Option<(f32, f32, f32, f32)> {
    match style.transform_box {
        css::TransformBox::FillBox
        | css::TransformBox::ContentBox
        | css::TransformBox::BorderBox => {
            let x = element
                .attrs
                .get("x")
                .map_or(Some(0.0), |value| value.parse().ok())?;
            let y = element
                .attrs
                .get("y")
                .map_or(Some(0.0), |value| value.parse().ok())?;
            let width = element.attrs.get("width")?.parse().ok()?;
            let height = element.attrs.get("height")?.parse().ok()?;
            (element.tag == "rect" && width >= 0.0 && height >= 0.0)
                .then_some((x, y, width, height))
        }
        css::TransformBox::StrokeBox | css::TransformBox::ViewBox => None,
    }
}

fn svg_css_used_length_in_user_units(value: css::ComputedLengthPercentage, basis: f32) -> f32 {
    used_length_percentage(
        value,
        PercentageBasis::definite(layout_pt(basis * css::CSS_PX_TO_PT)),
    )
    .points()
        / css::CSS_PX_TO_PT
}

fn svg_css_transform_function_matrix_for_box(
    function: css::TransformFunction,
    width: f32,
    height: f32,
) -> SvgCssTransform {
    match function {
        css::TransformFunction::Translate(translation) => SvgCssTransform::translation(
            svg_css_used_length_in_user_units(translation.x, width),
            svg_css_used_length_in_user_units(translation.y, height),
        ),
        function => svg_css_transform_function_matrix(function),
    }
}

/// The SVG scene parser receives an affine matrix in SVG user units. Retain
/// only transform values whose used value is independent of the target SVG
/// geometry; percentage transforms need an element-specific reference box and
/// remain represented as a documented SVG transform divergence.
fn svg_css_transform_is_resolvable(style: &ComputedStyle) -> bool {
    style.transform.iter().all(|function| match function {
        css::TransformFunction::Matrix(_)
        | css::TransformFunction::Translate(_)
        | css::TransformFunction::Scale(_)
        | css::TransformFunction::Rotate(..)
        | css::TransformFunction::Skew(_) => true,
        css::TransformFunction::Matrix3D(_)
        | css::TransformFunction::Translate3D(_)
        | css::TransformFunction::Scale3D(_)
        | css::TransformFunction::Rotate3D(_)
        | css::TransformFunction::Perspective(_) => false,
    })
}

fn svg_css_length_is_absolute(value: &css::ComputedLengthPercentage) -> bool {
    value.is_definitely_absolute()
}

type SvgCssTransform =
    euclid::Transform2D<f32, crate::svg::SvgSourceSpace, crate::svg::SvgSourceSpace>;

fn svg_css_transform_function_matrix(function: css::TransformFunction) -> SvgCssTransform {
    match function {
        // SVG's `matrix()` attribute consumes its numeric translation in the
        // current SVG user coordinate system. This is a separate explicit
        // projection from the CSS-pixel-to-paint-point projection for boxes.
        css::TransformFunction::Matrix(matrix) => matrix.into_space(euclid::Scale::new(1.0)),
        css::TransformFunction::Translate(translation) => SvgCssTransform::translation(
            svg_css_length_in_user_units(translation.x),
            svg_css_length_in_user_units(translation.y),
        ),
        css::TransformFunction::Scale(scale) => SvgCssTransform::scale(scale.x, scale.y),
        css::TransformFunction::Rotate(angle) => SvgCssTransform::rotation(angle),
        css::TransformFunction::Skew(angles) => SvgCssTransform::new(
            1.0,
            angles.y.radians.tan(),
            angles.x.radians.tan(),
            1.0,
            0.0,
            0.0,
        ),
        // The SVG scene path only accepts an affine SVG matrix.  3D CSS
        // transforms stay out of this bridge until SVG reference boxes and
        // projection are represented by the scene builder.
        css::TransformFunction::Matrix3D(_)
        | css::TransformFunction::Translate3D(_)
        | css::TransformFunction::Scale3D(_)
        | css::TransformFunction::Rotate3D(_)
        | css::TransformFunction::Perspective(_) => SvgCssTransform::identity(),
    }
}

fn svg_css_length_in_user_units(value: css::ComputedLengthPercentage) -> f32 {
    // SVG user units are CSS px in an inline SVG viewport. Computed CSS
    // lengths are stored in Quire points, so convert back at this boundary.
    used_length_percentage(value, PercentageBasis::definite(layout_pt(0.0))).points()
        / css::CSS_PX_TO_PT
}

#[cfg(test)]
mod svg_css_transform_tests {
    use super::*;

    #[test]
    fn css_matrix_projects_to_svg_source_units_without_paint_conversion() {
        let transform = svg_css_transform_function_matrix(css::TransformFunction::Matrix(
            css::CssAffineMatrix::new(1.0, 0.0, 0.0, 1.0, 10.0, 20.0),
        ));

        assert_eq!(transform.m31, 10.0);
        assert_eq!(transform.m32, 20.0);
    }

    #[test]
    fn svg_transform_eligibility_rejects_relative_and_deferred_lengths() {
        assert!(svg_css_length_is_absolute(
            &css::ComputedLengthPercentage::from_points(10.0)
        ));
        assert!(!svg_css_length_is_absolute(
            &css::ComputedLengthPercentage::from_percent(0.0)
        ));
        assert!(!svg_css_length_is_absolute(
            &css::ComputedLengthPercentage::from_em(1.0)
        ));
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "This helper carries layout pipeline state out of the async frame."
)]
fn layout_dom_with_font_system(
    _root: &Node,
    stylesheets: &Stylesheets<'_>,
    options: &RenderOptions,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    iframe_documents: &HashMap<crate::dom::ElementId, Document>,
    iframe_viewport: Option<PageSize>,
    parent_style: Box<ComputedStyle>,
    font_system: FontSystem,
    page_progression_direction: Direction,
    principal_flow: DocumentPrincipalFlow,
    document_canvas_resolution: DocumentCanvasResolution,
    page_margin_inherited_style: ComputedStyle,
    mut page_box: Box<box_tree::MutablePageBox<'_>>,
    target_references: TargetReferenceSnapshot,
) -> LayoutResult {
    let _timer = DebugTimer::start("building and flowing page box content");
    let mut builder = Box::new(LayoutBuilder::new(LayoutBuilderConfig {
        options,
        stylesheets: *stylesheets,
        base_url,
        root_url,
        resource_cache,
        iframe_documents,
        iframe_viewport,
        page_progression_direction,
        page_counter_initial_values: HashMap::new(),
        target_references,
        font_system,
    }));
    // The initial page context is created with builder defaults. Rebuild it
    // after installing the document-root inheritance used by page contexts so
    // logical page properties (including the first page's) use the same
    // writing mode and direction as every subsequently generated page.
    // https://www.w3.org/TR/css-page-3/#page-context
    // https://www.w3.org/TR/css-logical-1/#flow-relative-mapping
    builder.page_margin_inherited_style = page_margin_inherited_style;
    builder.principal_flow = principal_flow;
    builder.document_canvas_overflow = document_canvas_resolution;
    builder.initial_containing_block_writing_mode = principal_flow.writing_mode;
    builder.containing_block_writing_mode = principal_flow.writing_mode;
    builder.containing_block_direction = principal_flow.used_direction();
    builder.document_root_generates_box = !builder.page_margin_inherited_style.display.is_none();
    builder.rebuild_empty_current_page_context();
    if inline_start_side(principal_flow.writing_mode, principal_flow.used_direction())
        == PhysicalSide::Bottom
    {
        builder.cursor_y = builder.current_page_context.bottom();
    }
    if !builder.document_root_generates_box {
        // Paged media still has an initial page when the document root does
        // not generate a principal box (for example `html { display: none }`).
        // The page has no propagated root/body background, but it must remain
        // serializable as a valid blank PDF page.
        // <https://www.w3.org/TR/css-page-3/#page-box-page-progression>
        return LayoutResult::Document(builder.finish_boxed());
    }
    {
        let _timer = DebugTimer::start("resolving font-metric lengths");
        builder.resolve_font_metric_lengths_in_page_box(page_box.as_mut(), parent_style.as_ref());
        // Table reconstruction retains styles outside the principal child
        // vectors; resolve their metric components in the established pass.
        for child in &mut page_box.children {
            builder.resolve_font_metric_lengths_in_box(child);
        }
    }
    let page_box = {
        let _timer = DebugTimer::start("freezing formatting box tree");
        Box::new(box_tree::freeze_page_box(*page_box))
    };
    {
        let _timer = DebugTimer::start("planning and flowing page footnotes");
        if page_box.footnotes.is_empty() {
            // The convergence pass only exists to reserve page area for
            // detached footnote bodies. Replaying a document that cannot
            // produce one duplicates every intrinsic and final inline layout,
            // which is especially expensive for large orthogonal text flows.
            builder.layout_page_box(page_box.as_ref(), stylesheets);
        } else {
            let initial_snapshot = builder.snapshot();
            builder.install_footnotes(page_box.as_ref());
            let initial_measurement =
                builder.initial_single_footnote_measurement(page_box.as_ref());
            builder.restore(initial_snapshot.clone());
            // Obtain the first committed call assignment without painting.
            // Each later render pass validates its own committed assignments,
            // so a stable document takes one measure and one paint pass rather
            // than a measure-only confirmation followed by a third full
            // layout.  A changed assignment is rolled back and rendered again
            // with its newly reserved page-local footnote area.
            // <https://www.w3.org/TR/css-gcpm-3/#footnote-policy>
            let (mut measurements, mut reservations) =
                if let Some(measurement) = initial_measurement {
                    let measurements = vec![measurement];
                    let reservations =
                        LayoutBuilder::footnote_reservations_from_measurements(&measurements);
                    (measurements, reservations)
                } else {
                    builder.footnote_layout_mode = FootnoteLayoutMode::Measure;
                    builder.footnote_reservations.clear();
                    builder.footnote_measurements.clear();
                    builder.layout_page_box(page_box.as_ref(), stylesheets);
                    let measurements = std::mem::take(&mut builder.footnote_measurements);
                    let reservations =
                        LayoutBuilder::footnote_reservations_from_measurements(&measurements);
                    builder.restore(initial_snapshot.clone());
                    (measurements, reservations)
                };

            for attempt in 0..8 {
                builder.footnote_layout_mode = FootnoteLayoutMode::Render;
                builder.footnote_reservations = reservations.clone();
                builder.footnote_measurements = measurements.clone();
                builder.rendered_footnote_measurements.clear();
                builder.layout_page_box(page_box.as_ref(), stylesheets);
                let next_measurements = std::mem::take(&mut builder.rendered_footnote_measurements);
                let next_reservations =
                    LayoutBuilder::footnote_reservations_from_measurements(&next_measurements);
                if next_reservations == reservations {
                    break;
                }

                // The rendered page belongs to the rejected reservation
                // state. Restore all paint and page-local event state before
                // retrying from the document's fragmentainer boundary.
                builder.restore(initial_snapshot.clone());
                measurements = next_measurements;
                reservations = next_reservations;

                if attempt == 7 {
                    builder.footnote_layout_mode = FootnoteLayoutMode::Render;
                    builder.footnote_reservations = reservations;
                    builder.footnote_measurements = measurements;
                    builder.rendered_footnote_measurements.clear();
                    builder.layout_page_box(page_box.as_ref(), stylesheets);
                    break;
                }
            }
        }
    }
    if !builder.has_renderable_content() {
        // An empty document is represented by its initial page rather than an
        // invalid zero-page PDF. `finish_boxed` synthesizes that page while
        // retaining the selected page context and page rules.
        LayoutResult::Document(builder.finish_boxed())
    } else {
        let _timer = DebugTimer::start("finalizing laid out document");
        LayoutResult::Document(builder.finish_boxed())
    }
}

/// Resolves the document root style used by generated page-margin boxes.
///
/// CSS Paged Media creates margin boxes in the page context, but their
/// inherited typographic properties begin with the document root. This helper
/// deliberately does not feed that style into physical page-size resolution:
/// those descriptors have their own page-context initial values.
/// <https://www.w3.org/TR/css-page-3/#page-margin-boxes>
/// <https://www.w3.org/TR/css-page-3/#page-context>
pub(in crate::layout) fn document_root_style(
    root: &Node,
    stylesheets: &Stylesheets<'_>,
    parent_style: &ComputedStyle,
) -> ComputedStyle {
    let NodeKind::Element(root_element) = &root.kind else {
        return parent_style.clone();
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
        let style =
            style_for_layout_element(element, signature, stylesheets, Some(parent_style), &[]);
        return style;
    }
    parent_style.clone()
}

/// The writing-mode and direction that establish the document's initial
/// containing block.
///
/// For an HTML document, CSS Writing Modes takes these values from the first
/// eligible `body` child when the root does not have property containment.
/// The root's own style remains the inheritance source for generated page
/// margin boxes, so this deliberately models the principal-flow used values
/// separately from [`document_root_style`].
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct DocumentPrincipalFlow {
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    pub(in crate::layout) text_orientation: TextOrientation,
    pub(in crate::layout) source: PrincipalFlowSource,
}

/// The element that supplies the document's used principal flow.
///
/// A propagated body remains a document-canvas box; this identity lets layout
/// distinguish it from an ordinary root block sibling without changing the
/// root's cascaded style.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum PrincipalFlowSource {
    Root,
    Body(ElementId),
}

/// Logical block progression exported by the propagated body canvas to the
/// HTML root's principal formatting context.
///
/// This deliberately carries a logical advance instead of a physical box
/// width: vertical and sideways document flows project the same completion to
/// opposite physical directions.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct PrincipalFlowContribution {
    pub(in crate::layout) logical_block_advance: LayoutLength,
}

/// Shared state between a propagated document body and root inline content.
///
/// The body stays a real canvas box, but its automatic canvas width must not
/// replace the root's logical block cursor. Instead the body exports one
/// completed logical-flow contribution for following root generated content.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct RootPrincipalFlowContext {
    pub(in crate::layout) active_body: Option<ElementId>,
    /// Whether the root has an inline generated pseudo-element that continues
    /// after the body canvas in the principal formatting context.
    pub(in crate::layout) has_inline_root_pseudo: bool,
    /// The active body's physical inline-end canvas inset, retained while
    /// collecting descendants that belong to the propagated canvas.
    pub(in crate::layout) active_body_inline_end_inset: LayoutLength,
    pub(in crate::layout) completed_body: Option<PrincipalFlowContribution>,
}

/// Placement adjustment for a generated root pseudo-element whose computed
/// writing mode remains distinct from the principal flow used by the initial
/// containing block.
///
/// The element's computed style is never changed. This record only supplies
/// the physical block-start edge and the propagated body's canvas inset used
/// while projecting that one root pseudo box into the page area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct RootPseudoBlockProjection {
    pub(in crate::layout) element: ElementId,
    pub(in crate::layout) block_start: PhysicalSide,
    pub(in crate::layout) block_end_inset: LayoutLength,
}

impl DocumentPrincipalFlow {
    pub(in crate::layout) fn from_style(style: &ComputedStyle) -> Self {
        Self {
            writing_mode: style.writing_mode,
            // Keep the computed direction separately from the used direction:
            // text-orientation can force the latter in vertical flow without
            // changing the root's computed direction.
            // <https://drafts.csswg.org/css-writing-modes-4/#text-orientation>
            direction: style.direction,
            text_orientation: style.text_orientation,
            source: PrincipalFlowSource::Root,
        }
    }

    /// Returns whether `element` supplies the used principal flow.
    pub(in crate::layout) fn is_source_body(self, element: &Element) -> bool {
        self.source == PrincipalFlowSource::Body(element.id)
    }

    /// Returns the used inline base direction of the resolved principal flow.
    pub(in crate::layout) fn used_direction(self) -> Direction {
        if self.writing_mode.has_vertical_lines()
            && self.text_orientation == TextOrientation::Upright
        {
            Direction::Ltr
        } else {
            self.direction
        }
    }

    /// Produces the root's layout-only flow style.
    ///
    /// The propagated values establish the initial containing block and the
    /// root formatting context, but they do not alter the root's computed
    /// style or the inheritance parent used to build root pseudo-elements.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) fn root_layout_style(self, root_style: &ComputedStyle) -> ComputedStyle {
        let mut used = root_style.clone();
        used.writing_mode = self.writing_mode;
        used.direction = self.direction;
        used.text_orientation = self.text_orientation;
        // Inline root generated content participates in the principal inline
        // flow. A block-level pseudo establishes its own formatting context,
        // so it retains its computed writing mode as an orthogonal child.
        // Keep the cached computed pseudo styles on the unmodified root box.
        for pseudo in [
            used.before_style.as_deref_mut(),
            used.after_style.as_deref_mut(),
        ]
        .into_iter()
        .flatten()
        .filter(|pseudo| !pseudo.display.is_block_level())
        {
            pseudo.writing_mode = self.writing_mode;
            pseudo.direction = self.direction;
            pseudo.text_orientation = self.text_orientation;
        }
        used
    }
}

/// The document-root font-metric state during layout.
///
/// Root-relative font units must use the root element's selected font, even
/// when the root itself has no local metric-relative value. Keeping the
/// bootstrap state distinct prevents an ordinary style's fallback metrics from
/// becoming the document-wide root basis.
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum RootMetricState {
    Bootstrapping,
    Resolved(ResolvedRootFontMetrics),
}

impl RootMetricState {
    pub(in crate::layout) const fn font_size_basis(self) -> Option<css::RootFontMetricLengthBasis> {
        match self {
            Self::Bootstrapping => None,
            Self::Resolved(metrics) => Some(metrics.basis()),
        }
    }

    pub(in crate::layout) fn resolved(self) -> ResolvedRootFontMetrics {
        match self {
            Self::Bootstrapping => {
                unreachable!("the document root must establish font metrics before descendants")
            }
            Self::Resolved(metrics) => metrics,
        }
    }

    pub(in crate::layout) fn establish(&mut self, metrics: ResolvedRootFontMetrics) {
        debug_assert!(matches!(self, Self::Bootstrapping));
        *self = Self::Resolved(metrics);
    }
}

/// Root-font metrics measured after resolving the document root's used font.
///
/// The traversal APIs accept this wrapper rather than raw metrics, so a
/// descendant cannot accidentally receive a metric basis from an intervening
/// ancestor.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ResolvedRootFontMetrics(css::RootFontMetricLengthBasis);

impl ResolvedRootFontMetrics {
    /// Records metrics measured from the document root's selected font.
    pub(in crate::layout) const fn measured_for_document_root(
        basis: css::RootFontMetricLengthBasis,
    ) -> Self {
        Self(basis)
    }

    pub(in crate::layout) const fn basis(self) -> css::RootFontMetricLengthBasis {
        self.0
    }
}

/// Captures root counter resets that seed page-context counters.
///
/// CSS Paged Media page counters are independent page-associated counters, but
/// document counters can initialize them before page-context rules increment
/// or reset values for each generated page:
/// <https://www.w3.org/TR/css-page-3/#page-based-counters> and
/// <https://www.w3.org/TR/css-lists-3/#auto-numbering>.
pub(in crate::layout) struct LayoutBuilder<'a> {
    pub(in crate::layout) options: &'a RenderOptions,
    pub(in crate::layout) stylesheets: Stylesheets<'a>,
    pub(in crate::layout) base_url: Option<&'a url::Url>,
    pub(in crate::layout) root_url: Option<&'a url::Url>,
    pub(in crate::layout) resource_cache: &'a ResourceCache,
    pub(in crate::layout) iframe_documents: &'a HashMap<crate::dom::ElementId, Document>,
    pub(in crate::layout) iframe_viewport: Option<PageSize>,
    pub(in crate::layout) pages: Vec<Page>,
    pub(in crate::layout) page_names: Vec<Option<String>>,
    pub(in crate::layout) page_blanks: Vec<bool>,
    pub(in crate::layout) page_name_scope_suppression: usize,
    pub(in crate::layout) page_name_element_scope_suppression: usize,
    /// Lexical used `page` values for active element scopes.
    ///
    /// This is intentionally separate from `current_page_name`: a descendant
    /// can temporarily select a named destination page without changing the
    /// nearest non-`auto` ancestor used to resolve a later sibling's `auto`.
    pub(in crate::layout) page_value_scope_stack: Vec<Option<String>>,
    pub(in crate::layout) page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    /// Non-painting `string-set` sources keyed by the next/previous visible
    /// source boundary that owns their page position.
    pub(in crate::layout) suppressed_named_strings_before:
        HashMap<ElementId, Vec<box_tree::SuppressedNamedStringEvent>>,
    pub(in crate::layout) suppressed_named_strings_after:
        HashMap<ElementId, Vec<box_tree::SuppressedNamedStringEvent>>,
    pub(in crate::layout) page_anchors: HashMap<String, usize>,
    pub(in crate::layout) page_anchor_text: HashMap<String, AnchorText>,
    pub(in crate::layout) page_anchor_counters: HashMap<String, HashMap<String, Vec<i32>>>,
    pub(in crate::layout) target_references: TargetReferenceSnapshot,
    pub(in crate::layout) has_normal_flow_target_references: bool,
    pub(in crate::layout) document_canvas_background: Option<DocumentCanvasBackground>,
    pub(in crate::layout) document_canvas_overflow: DocumentCanvasResolution,
    /// Insets introduced by active document-canvas (`html`/`body`) boxes.
    ///
    /// Their inline components re-enter every page fragment, while their
    /// block-start components belong only to the first fragment. A
    /// destination page recomputes them from its own page area.
    pub(in crate::layout) document_canvas_fragment_insets: Vec<FragmentOffsets>,
    /// Whether the document root generates its principal box.
    ///
    /// A `display: none` root suppresses the document formatting structure,
    /// including page-context painting and generated margin boxes.
    /// <https://drafts.csswg.org/css-display-3/#valdef-display-none>
    pub(in crate::layout) document_root_generates_box: bool,
    pub(in crate::layout) current_page: Page,
    pub(in crate::layout) current_page_has_flow_content: bool,
    /// Whether the current fragment has in-flow content eligible to form a
    /// CSS Fragmentation class-A boundary for named-page selection.
    pub(in crate::layout) current_page_has_named_page_flow_content: bool,
    /// The named page type directly selected by a Class-A source on the
    /// current page.
    ///
    /// This is separate from normal-flow occupancy: a Class-A box can select
    /// a named page whose only document paint is out-of-flow. Empty
    /// continuation pages deliberately clear this marker, even when their
    /// provisional context inherits that page type, so a succeeding Class-A
    /// boundary can replace the provisional context rather than emit an extra
    /// named blank page.
    pub(in crate::layout) current_page_selected_name: Option<String>,
    pub(in crate::layout) last_block_layout_outcome: BlockLayoutOutcome,
    pub(in crate::layout) current_page_name: Option<String>,
    pub(in crate::layout) current_page_context: PageContext,
    /// Immutable initial containing block used by document viewport-relative
    /// lengths. Individual page contexts may change later through `@page`.
    pub(in crate::layout) initial_viewport_context: PageContext,
    /// Immutable renderer-provided page box used by viewport-relative page
    /// descriptors. Unlike the document initial containing block, this does
    /// not change when the first actual page is a differently sized named
    /// page.
    /// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
    pub(in crate::layout) page_descriptor_viewport_size: PageSize,
    pub(in crate::layout) fragmentainer_override: Option<FragmentainerOverride>,
    pub(in crate::layout) footnote_bodies: HashMap<ElementId, box_tree::FootnoteBox<'a>>,
    /// Whether each immutable source subtree contains a ruby formatting
    /// context. This structural fact survives fragmentainer retries.
    pub(in crate::layout) ruby_formatting_descendants: HashMap<ElementId, bool>,
    /// Named-page boundary summaries are a pure property of an immutable
    /// source subtree and its inherited page name. Replaying a rejected
    /// fragmentainer must not re-cascade that subtree.
    pub(in crate::layout) dom_page_boundary_summaries:
        HashMap<(ElementId, Option<String>), (ResolvedPageBoundaryValues, PageBoundaryValues)>,
    /// Pure auto-height table estimates reused by speculative pagination
    /// probes. The key includes the source table and its containing inline
    /// span; rollback never changes either input.
    pub(in crate::layout) speculative_table_height_estimates: HashMap<(ElementId, u32), f32>,
    /// Row-height plans produced while probing an avoid-constrained table.
    /// They are reusable by the accepted table layout when its source rows
    /// and resolved track width are unchanged.
    pub(in crate::layout) speculative_table_height_plans:
        HashMap<(ElementId, u32), table::TableHeightPlan>,
    pub(in crate::layout) footnote_measurements: Vec<FootnoteMeasurement>,
    /// Measurements captured from the paint-producing pagination pass. They
    /// validate that the committed call-to-page assignment matches the page
    /// reservation that selected this pass.
    pub(in crate::layout) rendered_footnote_measurements: Vec<FootnoteMeasurement>,
    /// Calls committed during the current measurement pass. This prevents a
    /// replayed selected line from reserving the same detached body twice.
    pub(in crate::layout) measured_footnotes: HashSet<ElementId>,
    /// Source-order inline floats already committed by inline line selection.
    ///
    /// The ordinary block-child traversal later reaches the same DOM element.
    /// It consumes this record instead of laying out and painting the float a
    /// second time.  The record retains the marker, selected source row, and
    /// the durable page-local exclusion that owns the captured paint subtree.
    pub(in crate::layout) committed_inline_floats: HashMap<ElementId, CommittedInlineFloat>,
    pub(in crate::layout) footnote_reservations: HashMap<usize, f32>,
    pub(in crate::layout) footnote_layout_mode: FootnoteLayoutMode,
    pub(in crate::layout) footnote_measurement_depth: usize,
    pub(in crate::layout) rendered_footnotes: HashSet<ElementId>,
    pub(in crate::layout) pending_page_footnotes: Vec<ElementId>,
    /// Descendants of a monolithic box lay out in an effectively unbounded
    /// fragmentainer so their overflow cannot split the containing box.
    pub(in crate::layout) fragmentation_suppression_depth: usize,
    /// Promoted `column-span:all` boxes fragment in the multicol ancestor's
    /// outer fragmentainer rather than prebreaking as ordinary siblings.
    pub(in crate::layout) multicol_spanner_fragmentation_depth: usize,
    /// Prevent recursive entry while a fixed spanner's overflowing
    /// descendants are laid out speculatively for deferred paint capture.
    pub(in crate::layout) multicol_spanner_speculation_depth: usize,
    /// Nested multicol containers encountered by a balance probe use their
    /// bounded estimate instead of recursively launching another probe tree.
    pub(in crate::layout) multicol_balance_probe_depth: usize,
    /// Pure auto-height float measurements reused by isolated layout replays.
    /// This cache deliberately survives layout snapshots: replays discard
    /// rendering state but not the used size of an unchanged float formatting
    /// context with the same used style and generated-content state.
    pub(in crate::layout) speculative_auto_float_margin_box_heights:
        HashMap<AutoFloatMeasurementKey, MarginBoxLength>,
    /// Floats currently undergoing their isolated auto-height measurement.
    ///
    /// This is deliberately not part of [`LayoutSnapshot`]. A measurement
    /// restores its snapshot before returning, but a nested replay must still
    /// be able to observe the in-progress outer measurement and break the
    /// cycle instead of starting another isolated replay.
    pub(in crate::layout) active_auto_float_measurements: Vec<ElementId>,
    /// Keys for the bounded estimator used when an auto-height float re-enters
    /// its own isolated measurement. This second stack prevents the estimator
    /// from recursively invoking itself through an intervening floated child.
    pub(in crate::layout) active_auto_float_measurement_fallbacks: Vec<ElementId>,
    pub(in crate::layout) cursor_y: f32,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_right: f32,
    /// Final coordinate contexts of active table-cell content scopes.
    pub(in crate::layout) table_cell_content_coordinate_contexts:
        Vec<table::TableCellContentCoordinateContext>,
    /// Physical inset at the bottom inline-start edge of a `sideways-lr`
    /// principal flow, supplied by the propagated body canvas.
    pub(in crate::layout) principal_inline_end_inset: f32,
    /// Physical block-end canvas inset contributed by the propagated body.
    pub(in crate::layout) principal_body_block_end_inset: LayoutLength,
    /// Scoped root-pseudo placement projection. Its presence never changes
    /// the pseudo-element's computed style.
    pub(in crate::layout) root_pseudo_block_projection: Option<RootPseudoBlockProjection>,
    /// Translation used only while querying float exclusions for an in-flow
    /// block fragment generated by block-in-inline splitting.
    ///
    /// Relative positioning paints the fragment at this translated position,
    /// but it must not alter its parent block's normal-flow cursor or its
    /// containing block geometry. The float query is the one layout operation
    /// that needs the visual coordinate space.
    pub(in crate::layout) inline_split_float_exclusion_query_offset: RelativeOffset,
    pub(in crate::layout) content_logical_inline_size_stack: Vec<f32>,
    /// Definite content-box inline sizes of active anonymous multicolumns.
    ///
    /// This is distinct from an element's own logical inline-size stack: a
    /// nested multicol principal can be measured in a temporary fragmentainer
    /// whose physical span is wider than its containing outer column.
    pub(in crate::layout) multicol_column_containing_blocks: Vec<MulticolColumnContainingBlock>,
    pub(in crate::layout) intrinsic_inline_percentage_basis_stack:
        Vec<IntrinsicInlinePercentageBasis>,
    pub(in crate::layout) inline_static_position: Option<InlineStaticPosition>,
    pub(in crate::layout) text_box_line_trim_stack: Vec<TextBoxLineTrim>,
    /// Per-block capture stack for line slots selected by inline layout.
    ///
    /// A nested block owns a nested capture and exports its result through
    /// `BlockLayoutOutcome`, preventing ancestors from double-counting it.
    pub(in crate::layout) clamp_line_slot_captures: Vec<usize>,
    /// Suppress positioned-layer creation while a formatting context collects
    /// line items before a dedicated positioned-descendant pass.
    pub(in crate::layout) positioned_inline_layout_suppression_depth: usize,
    /// Last prepared in-flow line baseline in the active layout coordinate space.
    ///
    /// CSS 2.2 defines an inline-block baseline from its last in-flow line box,
    /// not from whether that line emitted visible glyph paint. Keeping this as
    /// layout state lets transparent text and other non-painting lines still
    /// export the correct atomic inline baseline:
    /// <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>.
    pub(in crate::layout) last_in_flow_line_baseline_y: Option<f32>,
    /// Outside list markers awaiting their first accepted in-flow line.
    pub(in crate::layout) pending_outside_marker_anchors: Vec<PendingOutsideMarkerAnchor>,
    pub(in crate::layout) block_static_position_y_offset: Option<f32>,
    pub(in crate::layout) absolute_static_position: Option<AbsoluteStaticPosition>,
    /// Final-geometry grid scopes used by positioned descendants that retain
    /// a grid container as their absolute containing block.
    pub(in crate::layout) grid_positioning_scopes: Vec<grid::GridPositioningScope>,
    /// One-shot resolved parent-track contexts for directly replayed subgrids.
    /// A context is consumed by the subgrid's own grid formatting context so
    /// unrelated descendant grids cannot accidentally inherit it.
    pub(in crate::layout) pending_subgrid_contexts: Vec<Option<grid::ResolvedSubgridContext>>,
    pub(in crate::layout) escaped_atom_positioning_depth: usize,
    pub(in crate::layout) escaped_atom_containing_block: Option<ContainingBlock>,
    /// The outer absolute-position containing block and scratch-local static
    /// rectangle of the active escaped atomic inline, when applicable.
    pub(in crate::layout) escaped_atom_positioning_context: Option<EscapedAtomPositioningContext>,
    pub(in crate::layout) containing_block_direction: Direction,
    pub(in crate::layout) containing_block_writing_mode: WritingMode,
    /// Principal-flow axes of the initial containing block.
    ///
    /// CSS Writing Modes allows an eligible HTML body to supply these axes
    /// without changing the root element's computed style.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) initial_containing_block_writing_mode: WritingMode,
    /// Used flow axes of the initial containing block. This is intentionally
    /// separate from the HTML root's cascaded style: an eligible body can
    /// establish the principal flow without changing root computed values.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) principal_flow: DocumentPrincipalFlow,
    /// Cursor contribution shared between the propagated body canvas and
    /// anonymous root inline content.
    pub(in crate::layout) root_principal_flow_context: RootPrincipalFlowContext,
    pub(in crate::layout) fragment_top_offsets: Vec<f32>,
    pub(in crate::layout) child_available_space_stack: Vec<ChildAvailableSpace>,
    /// Normal-flow containing blocks provided while replaying flex/grid item
    /// contents. This is intentionally distinct from positioned containing
    /// blocks: relative positioning does not itself establish one.
    pub(in crate::layout) normal_flow_relative_containing_blocks:
        Vec<NormalFlowRelativeContainingBlock>,
    /// Actual block-container geometry used by anonymous inline replay to
    /// construct block-level static-position rectangles.
    pub(in crate::layout) block_static_position_contexts: Vec<BlockStaticPositionContext>,
    pub(in crate::layout) definite_block_size_stack: Vec<BlockSizePercentageBasis>,
    /// One-shot descendant percentage bases for replayed flex items.
    ///
    /// Flex replay materializes a final used height in a temporary style, but
    /// CSS Flexbox does not always make that post-flexing size definite for
    /// descendant percentage resolution. The pending entry is consumed by the
    /// replayed item's root formatting context, before its descendants begin
    /// their ordinary layout:
    /// <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
    pub(in crate::layout) replayed_flex_item_percentage_height_bases:
        Vec<Option<BlockSizePercentageBasis>>,
    /// One-shot wrapper block sizes passed from flex/grid item placement to a
    /// root table formatting context.
    pub(in crate::layout) table_wrapper_block_size_overrides: Vec<Option<BorderBoxLength>>,
    pub(in crate::layout) truncate_page_start_margins: bool,
    pub(in crate::layout) avoid_inside_retry_depth: usize,
    pub(in crate::layout) out_of_flow_prebreak_suppression_depth: usize,
    pub(in crate::layout) element_side_effect_suppression_depth: usize,
    pub(in crate::layout) containing_blocks: Vec<ContainingBlock>,
    /// Ancestor containing blocks that capture fixed-position descendants.
    /// Relative positioning captures absolute descendants only; transforms
    /// and layout/paint containment also capture fixed descendants.
    pub(in crate::layout) fixed_containing_blocks: Vec<ContainingBlock>,
    /// One-shot direct-child endpoints selected by a multicol planner for
    /// per-fragmentainer `text-box-trim: trim-end`.
    pub(in crate::layout) multicol_text_box_trim_end_child_indices: Option<Vec<usize>>,
    pub(in crate::layout) counter_set: CounterSet,
    pub(in crate::layout) counter_plan: CounterPlan,
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
    /// Root inherited style used only to initialize generated page-margin boxes.
    ///
    /// Page-margin boxes inherit typography and writing-mode from the page
    /// context, while physical page geometry remains resolved from `@page`
    /// declarations and render options.
    pub(in crate::layout) page_margin_inherited_style: ComputedStyle,
    pub(in crate::layout) page_declarations: Declarations,
    pub(in crate::layout) counter_styles: HashMap<String, CounterStyleRule>,
    pub(in crate::layout) first_page_declarations: Declarations,
    /// The single document-root snapshot used by eager and lazily built boxes.
    pub(in crate::layout) root_metric_state: RootMetricState,
    /// Whether any eagerly built style consumes root-relative selected-font
    /// metrics, which determines whether the root must intern a font.
    pub(in crate::layout) root_metrics_require_selected_font: bool,
    pub(in crate::layout) font_system: Box<FontSystem>,
    pub(in crate::layout) bookmarks: Vec<Bookmark>,
    pub(in crate::layout) positioned_layers: Vec<PositionedPaintLayer>,
    pub(in crate::layout) fixed_layers: Vec<FixedPaintLayer>,
    /// Positioned flex descendants captured during temporary multicolumn
    /// layout, awaiting replay against the committed containing block.
    pub(in crate::layout) deferred_multicol_positioned_children:
        Vec<DeferredMulticolPositionedChild>,
    /// Nesting depth of temporary multicolumn layout that captures positioned
    /// flex descendants. Only the outermost owner may replay the queue.
    pub(in crate::layout) multicol_positioned_replay_capture_depth: usize,
    /// Furthest page reached by a committed absolutely positioned margin box.
    ///
    /// This is deliberately separate from `pending_positioned_page_span_target`:
    /// transparent abspos geometry normally does not create blank pages, but a
    /// viewport-fixed layer must replay across its complete final page span.
    pub(in crate::layout) absolute_positioned_page_span_target: Option<usize>,
    pub(in crate::layout) pending_positioned_page_span_target: Option<usize>,
    pub(in crate::layout) next_paint_source_order: usize,
    pub(in crate::layout) overflow_clips: Vec<OverflowClip>,
    pub(in crate::layout) active_scroll_snap_scopes: Vec<scroll_snap::ActiveScrollSnapScope>,
    pub(in crate::layout) next_float_id: usize,
    pub(in crate::layout) float_contexts: Vec<FloatContext>,
    /// Parent containing-block spans for floated boxes currently capturing an
    /// isolated paint subtree. A fragmented float must re-enter this parent
    /// span on each destination page rather than carry the source page's
    /// side-by-side float placement.
    ///
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) float_fragment_parent_inline_spans: Vec<PageInlineSpan>,
    pub(in crate::layout) adjoining_float_origin_y: Option<f32>,
    pub(in crate::layout) pending_paint_fragments: Vec<PendingPaintFragment>,
    pub(in crate::layout) pending_page_side_effects: Vec<PendingPageSideEffects>,
    pub(in crate::layout) applied_clearance_count: usize,
    /// Nested floated principal boxes defer replaced-element paint scoping so
    /// the float can capture the complete paint subtree in its Float band.
    pub(in crate::layout) float_paint_capture_depth: usize,
    pub(in crate::layout) preserve_scoped_paint_public_order: bool,
    pub(in crate::layout) defer_next_block_decoration_promotion: bool,
    /// A formatting-context parent has already painted the next principal
    /// box's decoration at resolved geometry that ordinary replay cannot
    /// reconstruct. The principal layout retains its box metrics for
    /// descendants, but does not emit a second background, border, or outline.
    pub(in crate::layout) suppress_next_principal_box_decoration: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::layout) enum FootnoteLayoutMode {
    #[default]
    Measure,
    Render,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FootnoteMeasurement {
    pub(in crate::layout) element: ElementId,
    pub(in crate::layout) page_index: usize,
    /// Used vertical margin, border, and padding for the page's single
    /// footnote area. This is recorded with the first body assigned to a
    /// page, rather than charged once per body.
    ///
    /// <https://www.w3.org/TR/css-gcpm-3/#footnote-area>
    pub(in crate::layout) area_vertical_non_content: f32,
    /// The detached body's used block extent, excluding the enclosing
    /// footnote area's box-model edges.
    pub(in crate::layout) height: f32,
}

/// Propagated root/body background state used to paint the document canvas.
///
/// CSS Backgrounds propagates the root element background to the canvas, or
/// the first body background when the root has no background. The canvas paint
/// area is page-dependent in paged media, but image sizing and positioning stay
/// anchored to the root background positioning area:
/// <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct DocumentCanvasBackground {
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) root_background_defined: bool,
    pub(in crate::layout) root_positioning_area: Option<PaintBackgroundArea>,
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
    pending_first_formatted_line: bool,
    pending_typographic_pseudos: bool,
}

impl FirstFormattedLineState {
    pub(in crate::layout) fn for_style(style: &ComputedStyle) -> Self {
        Self {
            pending_first_formatted_line: true,
            pending_typographic_pseudos: style.first_line_style.is_some()
                || style.first_letter_style.is_some(),
        }
    }

    pub(in crate::layout) fn applies_to_next_inline_run(self) -> bool {
        self.pending_typographic_pseudos
    }

    /// Whether the originating block has not yet produced a formatted line.
    pub(in crate::layout) fn is_pending(self) -> bool {
        self.pending_first_formatted_line
    }

    pub(in crate::layout) fn consume_next_formatted_line(&mut self) {
        self.pending_first_formatted_line = false;
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

/// Apply a block container's typographic pseudo styles to an anonymous inline
/// sequence created by a block-in-inline split.
///
/// The anonymous box inherits ordinary properties from the inline ancestor,
/// but it is not the originating block for `::first-line` or `::first-letter`.
/// Carrying the originating block's already-cascaded pseudo styles to its
/// first anonymous sequence preserves that distinction without making later
/// anonymous sequences restart the pseudo-element:
/// <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo> and
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
pub(in crate::layout) fn style_with_originating_typographic_pseudos(
    anonymous_style: &ComputedStyle,
    originating_style: &ComputedStyle,
) -> Option<ComputedStyle> {
    if originating_style.first_line_style.is_none()
        && originating_style.first_letter_style.is_none()
    {
        return None;
    }
    let mut style = anonymous_style.clone();
    style.first_line_style = originating_style.first_line_style.clone();
    style.first_letter_style = originating_style.first_letter_style.clone();
    Some(style)
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct BlockLayoutOutcome {
    /// The signed bottom margin consumed by the preceding block layout.
    pub(in crate::layout) consumed_bottom_margin: LayoutLength,
    /// Resolved physical inline span of the block's border box.
    ///
    /// A vertical parent consumes this span on its logical block axis even
    /// when the child paints nothing. Keeping it in the layout outcome avoids
    /// deriving flow geometry from optional paint fragments.
    pub(in crate::layout) physical_border_box_inline_span: BorderBoxLength,
    /// The source-fragment, untransformed border box resolved by normal-flow
    /// layout.  Parent special rendering models (notably HTML's rendered
    /// legend) use this layout geometry rather than painted-ink bounds.
    ///
    /// <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
    /// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
    pub(in crate::layout) static_border_box: Option<PaintRect>,
    /// Line-selection slots produced by this block's in-flow contents.
    pub(in crate::layout) clamp_line_slots: usize,
    /// The last destination fragment committed by an in-flow child before
    /// this block applies its own used block-size constraints.
    ///
    /// A parent formatting context can use this endpoint when it owns an
    /// independently fragmenting child's continuation. This is the
    /// authoritative auto-height flow endpoint: a frozen flex-item border
    /// box, positioned descendant, or provisional source-global used height
    /// must not replace the child's final fragmentainer-local cursor before
    /// the parent tests its next normal-flow sibling for overflow.
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>
    pub(in crate::layout) in_flow_child_fragment_end: Option<InFlowFragmentEnd>,
}

/// A destination-page flow endpoint committed by a descendant formatting
/// context before its parent applies a separate used-size constraint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InFlowFragmentEnd {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) cursor: PageTopBlockPosition,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockEndMarginCollapse {
    /// The signed child margin that must be restored before parent-end collapse.
    pub(in crate::layout) child_consumed_margin: LayoutLength,
    /// The signed collapsed margin to consume at the parent block end.
    pub(in crate::layout) collapsed_margin: LayoutLength,
}

pub(in crate::layout) struct LayoutBuilderConfig<'a> {
    pub(in crate::layout) options: &'a RenderOptions,
    pub(in crate::layout) stylesheets: Stylesheets<'a>,
    pub(in crate::layout) base_url: Option<&'a url::Url>,
    pub(in crate::layout) root_url: Option<&'a url::Url>,
    pub(in crate::layout) resource_cache: &'a ResourceCache,
    pub(in crate::layout) iframe_documents: &'a HashMap<crate::dom::ElementId, Document>,
    pub(in crate::layout) iframe_viewport: Option<PageSize>,
    pub(in crate::layout) page_progression_direction: Direction,
    pub(in crate::layout) page_counter_initial_values: HashMap<String, i32>,
    pub(in crate::layout) target_references: TargetReferenceSnapshot,
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

impl FragmentOffsets {
    pub(in crate::layout) const ZERO: Self = Self {
        left: 0.0,
        right: 0.0,
        top: 0.0,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_options_enable_html_presentational_hints_by_default() {
        assert!(RenderOptions::default().presentational_hints);

        let options = RenderOptions {
            presentational_hints: false,
            ..RenderOptions::default()
        };
        assert!(!options.presentational_hints);
    }

    #[test]
    fn render_options_validate_and_expose_device_resolution() {
        let mut options = RenderOptions::default();
        assert_eq!(options.device_resolution_dppx(), 1.0);
        options.set_device_resolution_dppx(2.0).unwrap();
        assert_eq!(options.device_resolution_dppx(), 2.0);
        assert_eq!(options.media_environment().resolution_dppx, 2.0);
        assert!(options.set_device_resolution_dppx(0.0).is_err());
        assert!(options.set_device_resolution_dppx(f32::NAN).is_err());
    }

    #[test]
    fn page_size_converts_to_a_physical_layout_size() {
        assert_eq!(
            PageSize::from_points(300.0, 200.0).layout_size(),
            crate::units::LayoutSize::new(300.0, 200.0)
        );
    }

    #[test]
    fn page_context_maps_logical_axes_for_vertical_fragmentation() {
        let context = PageContext {
            size: PageSize::from_points(600.0, 400.0),
            margins: PageMargins::all_points(0.0),
            edges: PageBoxEdges::ZERO,
            rotation: 0,
        };

        assert_eq!(
            context.logical_inline_size(WritingMode::HorizontalTb),
            600.0
        );
        assert_eq!(context.logical_block_size(WritingMode::HorizontalTb), 400.0);
        assert_eq!(context.logical_inline_size(WritingMode::VerticalRl), 400.0);
        assert_eq!(context.logical_block_size(WritingMode::VerticalRl), 600.0);
        assert_eq!(context.logical_inline_size(WritingMode::SidewaysLr), 400.0);
        assert_eq!(context.logical_block_size(WritingMode::SidewaysLr), 600.0);
    }
}
