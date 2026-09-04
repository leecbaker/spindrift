use super::*;

/// Cache key for a table wrapper height probe. The three optional values are
/// the resolved preferred, minimum, and maximum block-size constraints.
pub(in crate::layout) type TableHeightEstimateCacheKey =
    (ElementId, u32, Option<u32>, Option<u32>, Option<u32>);

/// Cache key for a table row-height distribution plan.
///
/// The target distinguishes an intrinsic row plan from one whose rows are
/// distributed against a definite table content-box height. The optional
/// sizes distinguish flex/grid wrapper sizing from an absolutely positioned
/// table's definite logical block-size contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct TableHeightPlanCacheKey {
    pub(in crate::layout) table_element: ElementId,
    pub(in crate::layout) column_width_bits: u32,
    pub(in crate::layout) wrapper_border_box_block_size_bits: Option<u32>,
    pub(in crate::layout) positioned_table_block_content_size_bits: Option<u32>,
    pub(in crate::layout) wrapper_non_grid_block_size_bits: u32,
    pub(in crate::layout) target: table::TableHeightDistributionTargetKey,
}

/// Why paint is deferred to a fragmentainer that normal flow has not reached.
///
/// The destination is not sufficient to determine whether a pending fragment
/// extends an ancestor's principal box. In particular, float contents form a
/// parallel fragmentation flow, while normal-flow overflow and positioned
/// paint can have separate box-fragment ownership rules.
/// <https://drafts.csswg.org/css-break/#parallel-flows>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum PendingPaintFragmentKind {
    /// A continuation of a fragmented CSS float.
    FragmentedFloat,
    /// Paint produced by normal flow after it crossed a fragmentainer edge.
    InFlowOverflow,
    /// Paint deferred by a positioned or explicitly scoped paint operation.
    PositionedOrScoped,
}

/// Paint captured speculatively for a fragmentainer that normal flow has not
/// reached yet.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct PendingPaintFragment {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) fragment: PaintFragment,
    pub(in crate::layout) kind: PendingPaintFragmentKind,
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
    /// Source-local anchor origins retained only while a replay artifact is
    /// projected to committed fragments.
    pub(in crate::layout) anchor_source_positions: Vec<(String, PaintPoint)>,
    pub(in crate::layout) anchor_text: Vec<(String, AnchorText)>,
    /// Counter snapshots belong to the same target placement as the anchor
    /// text. Keeping them together prevents scratch-page target counters from
    /// leaking into a later document-page replay.
    pub(in crate::layout) anchor_counters: Vec<(String, HashMap<String, Vec<i32>>)>,
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
/// CSS Paged Media controls the page size through the `@page` rule, for
/// example `@page { size: 612pt 792pt }`.
pub(crate) struct PageSize {
    pub(crate) width: LayoutLength,
    pub(crate) height: LayoutLength,
}

/// The physical viewport and inherited CSS zoom of an embedded document.
///
/// The viewport is already the embedding iframe's zoomed content box, while
/// the effective zoom seeds the child document's root cascade so child used
/// lengths and nested frames scale exactly once.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IframeEmbeddingContext {
    pub(crate) viewport: PageSize,
    pub(crate) effective_zoom: css::EffectiveZoom,
}

pub(in crate::layout) const LIST_ITEM_COUNTER_NAME: &str = "list-item";

impl PageSize {
    /// The ISO A4 page size in PDF points.
    pub(crate) const A4_POINTS: Self = Self {
        width: layout_pt(595.2756),
        height: layout_pt(841.8898),
    };

    /// Creates a page size from width and height measured in PDF points.
    pub(crate) const fn from_points(width: f32, height: f32) -> Self {
        Self {
            width: layout_pt(width),
            height: layout_pt(height),
        }
    }

    /// Returns the page width in PDF points.
    pub(crate) fn width(&self) -> f32 {
        layout_points(self.width)
    }

    /// Returns the page height in PDF points.
    pub(crate) fn height(&self) -> f32 {
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
/// CSS Paged Media controls page margins through the `@page` rule. This
/// internal type represents the resolved geometry of one page box.
pub(crate) struct PageMargins {
    pub(crate) top: LayoutLength,
    pub(crate) right: LayoutLength,
    pub(crate) bottom: LayoutLength,
    pub(crate) left: LayoutLength,
}

impl PageMargins {
    /// WeasyPrint's default page margin in PDF points.
    pub(crate) const WEASYPRINT_DEFAULT_POINTS: f32 = 56.25;
    /// The default margin on every page edge.
    pub(crate) const DEFAULT: Self = Self {
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
    pub(crate) const fn all_points(value: f32) -> Self {
        Self::all(layout_pt(value))
    }

    /// Creates margins from top, right, bottom, and left PDF-point values.
    pub(crate) const fn from_points(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top: layout_pt(top),
            right: layout_pt(right),
            bottom: layout_pt(bottom),
            left: layout_pt(left),
        }
    }

    /// Returns the top margin in PDF points.
    pub(crate) fn top(&self) -> f32 {
        layout_points(self.top)
    }

    /// Returns the right margin in PDF points.
    pub(crate) fn right(&self) -> f32 {
        layout_points(self.right)
    }

    /// Returns the bottom margin in PDF points.
    pub(crate) fn bottom(&self) -> f32 {
        layout_points(self.bottom)
    }

    /// Returns the left margin in PDF points.
    pub(crate) fn left(&self) -> f32 {
        layout_points(self.left)
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Inputs that control document parsing, cascade, and layout.
///
/// ```no_run
/// use spindrift::{Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> spindrift::Result<()> {
/// let mut render_options = RenderOptions::default();
/// render_options.target_fragment = Some("summary".to_string());
/// let mut output = File::create("document.pdf")?;
/// Html::from_file("document.html")
///     .await?
///     .write_pdf(&mut output, &render_options, &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct RenderOptions {
    /// Output medium and viewport used by CSS Media Queries.
    pub media_type: crate::css::MediaType,
    /// User color-scheme preference exposed to CSS color-scheme selection and
    /// `prefers-color-scheme` media queries.
    pub color_scheme_preference: crate::css::ColorSchemePreference,
    /// Forced-colors palette used for CSS CssColor Adjustment and media queries.
    pub forced_colors: crate::css::ForcedColorsMode,
    /// Device pixel density exposed to CSS resolution media queries and used
    /// to select CSS Images `image-set()` candidates.
    pub(crate) device_resolution_dppx: f32,
    /// The initial physical page box used before `@page` rules select a page
    /// size. This is an implementation fallback, not a public render input:
    /// document page size is controlled by CSS Paged Media's `@page size`.
    pub(crate) page_size: PageSize,
    /// The embedding viewport's initial page margins for an iframe document.
    /// Top-level documents always use [`PageMargins::DEFAULT`]; iframe layout
    /// starts at the embedding content edge until author `@page` rules apply.
    pub(crate) iframe_page_margins: Option<PageMargins>,
    /// The initial font size in layout units.
    pub(crate) font_size: LayoutLength,
    /// The initial line height in layout units.
    pub(crate) line_height: LayoutLength,
    /// URL fragment target used by Selectors `:target` and `:target-within`.
    ///
    /// Static PDF rendering has no browsing session, so the target element is
    /// an explicit render input when callers want fragment-sensitive styling:
    /// <https://www.w3.org/TR/selectors-4/#the-target-pseudo>.
    pub target_fragment: Option<String>,
}

impl RenderOptions {
    /// Returns the initial CSS-pixel viewport used before document `@page`
    /// descriptors establish their page box.
    pub fn initial_viewport_size(&self) -> crate::css::CssViewportSize {
        crate::css::CssViewportSize::new(
            self.page_size.width() / crate::css::CSS_PX_TO_PT,
            self.page_size.height() / crate::css::CSS_PX_TO_PT,
        )
    }

    /// Sets the initial CSS-pixel viewport used for media queries and
    /// viewport-relative page descriptors.
    ///
    /// This is an embedding input: document-authored `@page` rules can still
    /// choose the final physical page size.
    pub fn set_initial_viewport_size(
        &mut self,
        viewport: crate::css::CssViewportSize,
    ) -> crate::Result<()> {
        if !viewport.width.is_finite()
            || !viewport.height.is_finite()
            || viewport.width <= 0.0
            || viewport.height <= 0.0
        {
            return Err(crate::Error::InvalidInput(
                "initial viewport dimensions must be finite and greater than zero".to_string(),
            ));
        }
        self.page_size = PageSize::from_points(
            viewport.width * crate::css::CSS_PX_TO_PT,
            viewport.height * crate::css::CSS_PX_TO_PT,
        );
        Ok(())
    }

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
    /// Returns the initial font size in PDF points.
    ///
    /// ```
    /// let options = spindrift::RenderOptions::default();
    /// assert!(options.font_size() > 0.0);
    /// ```
    pub fn font_size(&self) -> f32 {
        layout_points(self.font_size)
    }

    /// Returns the initial line height in PDF points.
    ///
    /// ```
    /// let options = spindrift::RenderOptions::default();
    /// assert!(options.line_height() > 0.0);
    /// ```
    pub fn line_height(&self) -> f32 {
        layout_points(self.line_height)
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
        .with_color_scheme_preference(self.color_scheme_preference)
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        let font_size = 12.0;
        Self {
            media_type: crate::css::MediaType::Print,
            color_scheme_preference: crate::css::ColorSchemePreference::None,
            forced_colors: crate::css::ForcedColorsMode::Inactive,
            device_resolution_dppx: 1.0,
            page_size: PageSize::A4_POINTS,
            iframe_page_margins: None,
            font_size: layout_pt(font_size),
            line_height: layout_pt(font_size * 1.2),
            target_fragment: None,
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
    pub(crate) document_url: Option<&'a url::Url>,
    pub(crate) base_url: Option<&'a url::Url>,
    pub(crate) root_url: Option<&'a url::Url>,
    pub(crate) resource_cache: &'a ResourceCache,
    pub(crate) iframe_documents: &'a HashMap<crate::dom::ElementId, Document>,
    /// The finite viewport of an iframe whose static contents use an
    /// unfragmented layout canvas.
    pub(crate) iframe_viewport: Option<IframeEmbeddingContext>,
    pub(crate) font_system: FontSystem,
}

pub(crate) fn layout_prepared_dom(config: PreparedDomLayout<'_>) -> Document {
    let PreparedDomLayout {
        root,
        stylesheets,
        options,
        document_url,
        base_url,
        root_url,
        resource_cache,
        iframe_documents,
        iframe_viewport,
        mut font_system,
    } = config;
    let _timer = DebugTimer::start("layout pipeline");
    #[cfg(feature = "layout-profile")]
    let _layout_profile_document = super::layout_profile::begin_document();
    // Fragment targeting is resolved while preparing the DOM.  From this
    // point onward selector-relevant source metadata is immutable, so share a
    // single snapshot tree across all formatting-tree and layout replays.
    prime_selector_snapshots(root, document_url, base_url);
    let default_line_height_multiplier = if options.font_size() > 0.0 {
        options.line_height() / options.font_size()
    } else {
        1.2
    };
    // Build from the stylesheet's typed initial state instead of a struct
    // update. CSS-private cascade snapshots must retain their own initial
    // value at this layout boundary.
    let mut parent_style = ComputedStyle::initial();
    parent_style.font_size = options.font_size();
    parent_style.deferred_font_size = css::DeferredFontSize::Absolute(options.font_size());
    parent_style.line_height_value =
        css::ComputedLineHeight::Number(default_line_height_multiplier);
    parent_style.line_height = options.line_height();
    parent_style.color = CssColor::BLACK;
    if let Some(context) = iframe_viewport {
        parent_style.effective_zoom = context.effective_zoom;
    }
    let parent_style = Box::new(parent_style);
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
    let page_progression_direction = principal_flow.page_progression_direction();
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
    let mut overrides = crate::svg::SvgPresentationOverrides::new();
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
        let signature =
            ElementSignature::from_sibling_snapshot(element_index, sibling_tags.clone())
                .expect("source child must have a cached sibling signature");
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
    let svg_presentation = svg_transform_presentation_declarations(element);
    let style = css::style_for_element_with_signature_and_svg_presentation(
        layout_element_signature(element, signature.clone(), Some(parent_style)),
        element.attrs.get("style").map(String::as_str),
        svg_presentation
            .has_declarations()
            .then_some(&svg_presentation),
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
    if applies_to_svg_scene {
        let transform = if !enters_inline_svg
            && !style.has_transform()
            && svg_presentation.has_valid_transform()
        {
            // A higher-priority CSS `transform: none` suppresses the SVG
            // presentation attribute rather than falling back to it.
            Some(crate::svg::SvgTransformOverride::Scene(
                crate::svg::SvgUsedTransform::None,
            ))
        } else {
            (!enters_inline_svg && style.has_transform())
                .then(|| svg_css_transform_is_resolvable(&style))
                .filter(|resolvable| *resolvable)
                .and_then(|_| svg_css_transform_for_element(&style, element))
                .map(|transform| {
                    crate::svg::SvgTransformOverride::Scene(crate::svg::SvgUsedTransform::Affine(
                        transform,
                    ))
                })
        };
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
            // The standalone SVG parser starts its own UA cascade. Even when
            // the inline SVG root merely inherits the host family unchanged,
            // serialize that used value so SVG text does not fall back to the
            // SVG UA's `serif` default.
            // <https://www.w3.org/TR/SVG2/styling.html#UsingCSS>
            // <https://www.w3.org/TR/css-cascade-5/#inheritance>
            // Text geometry (`em` positions, textLength and chunk
            // normalization) still belongs to usvg. Serialize the final
            // cascade values rather than only parent differences so its
            // geometry inputs agree with the typed shaping side table even
            // when a host rule resets a source SVG attribute to its inherited
            // value.
            font_family: Some(crate::svg::svg_font_family_presentation_attribute(
                &style.font_family,
            )),
            font_size: Some(format!("{}px", style.font_size / css::CSS_PX_TO_PT)),
            font_weight: Some(style.font_weight.0.to_string()),
            font_style: Some({
                match style.font_style {
                    css::FontStyle::Normal => "normal".to_owned(),
                    css::FontStyle::Italic => "italic".to_owned(),
                    css::FontStyle::Oblique(angle) => {
                        format!("oblique {}deg", f32::from_bits(angle))
                    }
                }
            }),
            font_stretch: Some(format!("{}%", style.font_width.0 as f32 / 10.0)),
            font_variation_settings: Some(
                crate::svg::svg_font_variation_settings_presentation_attribute(
                    &style.font_variation_settings,
                ),
            ),
            font_kerning: Some({
                match style.font_kerning {
                    css::FontKerning::Auto => "auto",
                    css::FontKerning::Normal => "normal",
                    css::FontKerning::None => "none",
                }
                .to_owned()
            }),
            letter_spacing: Some(format!(
                "{}px",
                style.used_letter_spacing().points() / css::CSS_PX_TO_PT
            )),
            word_spacing: Some(format!(
                "{}px",
                style.used_word_spacing().points() / css::CSS_PX_TO_PT
            )),
            writing_mode: Some({
                match style.writing_mode {
                    css::WritingMode::HorizontalTb => "horizontal-tb",
                    css::WritingMode::VerticalRl => "vertical-rl",
                    css::WritingMode::VerticalLr => "vertical-lr",
                    css::WritingMode::SidewaysRl => "sideways-rl",
                    css::WritingMode::SidewaysLr => "sideways-lr",
                }
                .to_owned()
            }),
            text_orientation: Some(
                match style.text_orientation {
                    css::TextOrientation::Mixed => "mixed",
                    css::TextOrientation::Upright => "upright",
                    css::TextOrientation::Sideways => "sideways",
                }
                .to_owned(),
            ),
            direction: Some(match style.direction {
                css::Direction::Ltr => "ltr".to_owned(),
                css::Direction::Rtl => "rtl".to_owned(),
            }),
            unicode_bidi: Some({
                match style.unicode_bidi {
                    css::UnicodeBidi::Normal => "normal",
                    css::UnicodeBidi::Embed => "embed",
                    css::UnicodeBidi::Isolate => "isolate",
                    css::UnicodeBidi::BidiOverride => "bidi-override",
                    css::UnicodeBidi::IsolateOverride => "isolate-override",
                    css::UnicodeBidi::Plaintext => "plaintext",
                }
                .to_owned()
            }),
            flood_color: svg_filter_color_element(element)
                .then(|| crate::svg::SvgFilterColorOverride::from(style.svg_flood_color)),
            lighting_color: svg_lighting_color_element(element)
                .then(|| crate::svg::SvgFilterColorOverride::from(style.svg_lighting_color)),
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
            || presentation.font_family.is_some()
            || presentation.font_size.is_some()
            || presentation.font_weight.is_some()
            || presentation.font_style.is_some()
            || presentation.font_stretch.is_some()
            || presentation.font_variation_settings.is_some()
            || presentation.font_kerning.is_some()
            || presentation.letter_spacing.is_some()
            || presentation.word_spacing.is_some()
            || presentation.writing_mode.is_some()
            || presentation.text_orientation.is_some()
            || presentation.direction.is_some()
            || presentation.unicode_bidi.is_some()
            || presentation.flood_color.is_some()
            || presentation.lighting_color.is_some()
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
        let signature =
            ElementSignature::from_sibling_snapshot(element_index, sibling_tags.clone())
                .expect("source child must have a cached sibling signature");
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

fn svg_filter_color_element(element: &Element) -> bool {
    element.namespace_url == "http://www.w3.org/2000/svg"
        && matches!(element.tag.as_str(), "feFlood" | "feDropShadow")
}

fn svg_lighting_color_element(element: &Element) -> bool {
    element.namespace_url == "http://www.w3.org/2000/svg"
        && matches!(
            element.tag.as_str(),
            "feDiffuseLighting" | "feSpecularLighting"
        )
}

/// Translate an SVG transform presentation attribute into the first
/// author-origin CSS declaration for the host cascade. SVG's attribute grammar
/// is normalized to a CSS matrix before it enters the cascade, so its
/// unitless angles and `rotate(angle cx cy)` form never leak into CSS parsing.
fn svg_transform_presentation_declarations(
    element: &Element,
) -> css::SvgPresentationAttributeDeclarations {
    const TEXT_PRESENTATION_ATTRIBUTES: &[&str] = &[
        "font-family",
        "font-size",
        "font-size-adjust",
        "font-weight",
        "font-style",
        "font-stretch",
        "font-language-override",
        "font-synthesis",
        "font-synthesis-weight",
        "font-synthesis-style",
        "font-synthesis-small-caps",
        "font-synthesis-position",
        "font-feature-settings",
        "font-variation-settings",
        "font-kerning",
        "font-variant",
        "font-variant-ligatures",
        "font-variant-position",
        "font-variant-caps",
        "font-variant-numeric",
        "font-variant-alternates",
        "font-variant-east-asian",
        "font-variant-emoji",
        "font-palette",
        "letter-spacing",
        "word-spacing",
        "direction",
        "unicode-bidi",
        "writing-mode",
        "text-orientation",
    ];
    let text_presentation_attributes = TEXT_PRESENTATION_ATTRIBUTES
        .iter()
        .filter_map(|name| {
            element
                .attrs
                .get(*name)
                .map(|value| (*name, value.as_str()))
        })
        .collect::<Vec<_>>();
    css::SvgPresentationAttributeDeclarations::svg_properties(
        element.attrs.get("transform").map(String::as_str),
        element.attrs.get("transform-origin").map(String::as_str),
        element.attrs.get("transform-box").map(String::as_str),
        element.attrs.get("flood-color").map(String::as_str),
        element.attrs.get("lighting-color").map(String::as_str),
        &text_presentation_attributes,
    )
}

/// Resolve CSS transforms on basic SVG graphics using the selected geometry
/// reference box. The scene serializer receives the resulting typed affine
/// SVG matrix rather than a CSS string override, so percentage translations
/// and origins use the same box.
fn svg_css_transform_for_element(
    style: &ComputedStyle,
    element: &Element,
) -> Option<crate::svg::SvgElementTransform> {
    let Some(reference_boxes) = svg_rect_transform_reference_boxes(element, style) else {
        return svg_css_transform_without_reference_box(style);
    };
    let reference_box = reference_boxes
        .select(style.transform_box)
        // SVG graphical elements have a fill-box fallback until their local
        // viewport is represented by the typed SVG geometry tree.
        .or_else(|| reference_boxes.select(css::TransformBox::FillBox))?;
    let rect = reference_box.rect();
    let origin = if style.transform_origin.is_initial {
        // CSS Transforms defines the initial SVG graphics origin as `0 0`.
        reference_box.origin(0.0, 0.0)
    } else {
        reference_box.origin(
            svg_css_used_length_in_user_units(style.transform_origin.x.clone(), rect.width()),
            svg_css_used_length_in_user_units(style.transform_origin.y.clone(), rect.height()),
        )
    };
    let transform = crate::layout::assets::compose_css_transform_matrix(
        origin.point(),
        style.individual_transforms.clone(),
        &style.transform,
        |function| svg_css_transform_function_matrix_for_box(function, rect.width(), rect.height()),
    );
    Some(transform)
}

/// Convert a transform which has no dependency on an SVG reference box.
///
/// Layout does not retain enough SVG viewport geometry at this bridge for all
/// SVG element types.  Absolute transforms with an initial or absolute origin
/// are nevertheless fully determined, including on a root `<svg>` whose
/// width/height attributes use CSS units.  Preserving this path avoids
/// dropping valid transforms merely because its source geometry is not a
/// plain unitless `<rect>`.
fn svg_css_transform_without_reference_box(
    style: &ComputedStyle,
) -> Option<crate::svg::SvgElementTransform> {
    let origin_is_resolvable = style.transform_origin.is_initial
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
    let origin = if style.transform_origin.is_initial {
        crate::svg::SvgElementPoint::new(0.0, 0.0)
    } else {
        crate::svg::SvgElementPoint::new(
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
    Some(transform)
}

/// Basic SVG fill-box support for CSS transforms. `stroke-box` and `view-box`
/// remain scene-level work because they require stroke and nested-viewport
/// geometry that is not preserved by this DOM bridge.
fn svg_rect_transform_reference_boxes(
    element: &Element,
    style: &ComputedStyle,
) -> Option<crate::svg::SvgTransformReferenceBoxes> {
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
            if element.tag != "rect" || width < 0.0 || height < 0.0 {
                return None;
            }
            let fill = crate::svg::SvgElementRect::new(
                crate::svg::SvgElementPoint::new(x, y),
                crate::svg::SvgElementSize::new(width, height),
            );
            // SVG's initial transform-box is view-box. The root viewport's
            // origin is the current SVG user-space origin, not this graphic's
            // fill-box origin; absolute transform origins must therefore not
            // gain the rectangle's x/y offset. Width/height are a temporary
            // local extent until the ancestor viewport geometry tree supplies
            // an exact nested viewBox.
            let view = crate::svg::SvgElementRect::new(
                crate::svg::SvgElementPoint::new(0.0, 0.0),
                crate::svg::SvgElementSize::new(width, height),
            );
            // The current DOM bridge does not yet retain SVG stroke geometry
            // or nested viewports. Retaining the candidates in one typed
            // record keeps those later additions local to this resolver.
            Some(crate::svg::SvgTransformReferenceBoxes::new(
                fill,
                fill,
                Some(view),
            ))
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

type SvgCssTransform = crate::svg::SvgElementTransform;

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
    // lengths are stored in Spindrift points, so convert back at this boundary.
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
    iframe_viewport: Option<IframeEmbeddingContext>,
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
    // The initial page context is constructed before the document root has
    // selected its font. No flow content exists at this point, so rebuild it
    // with the established root metrics before page dimensions become
    // percentage or viewport bases for the document.
    // <https://www.w3.org/TR/css-values-4/#root-relative-fonts>
    builder.rebuild_empty_current_page_context();
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
            // First discover call ownership without reserving a footnote area.
            // A call's page is a result of pagination and cannot be seeded on
            // an arbitrary page: that reservation could itself change every
            // later page assignment. Only measurements from committed lines
            // in this unbiased traversal may initialize the fixed point.
            // <https://www.w3.org/TR/css-gcpm-3/#footnote-policy>
            builder.footnote_layout_mode = FootnoteLayoutMode::Measure;
            builder.footnote_reservations.clear();
            builder.footnote_measurements.clear();
            builder.layout_page_box(page_box.as_ref(), stylesheets);
            let mut measurements = std::mem::take(&mut builder.footnote_measurements);
            let mut reservations =
                LayoutBuilder::footnote_reservations_from_measurements(&measurements);
            builder.restore(initial_snapshot.clone());

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
                builder.constrain_footnote_calls_to_observed_pages(&next_measurements);
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
        let signature = ElementSignature::from_sibling_snapshot(element_index, sibling_tags)
            .expect("source child must have a cached sibling signature");
        let style =
            style_for_layout_element(element, signature, stylesheets, Some(parent_style), &[]);
        return style;
    }
    parent_style.clone()
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

/// One ancestor eligible to provide container-relative length axes.
///
/// The physical content-box axes are retained independently because CSS
/// container-relative units select the nearest eligible container per axis.
/// <https://drafts.csswg.org/css-conditional-5/#container-lengths>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ContainerUnitContext {
    pub(in crate::layout) physical_width: PhysicalContentWidth,
    pub(in crate::layout) physical_height: PhysicalContentHeight,
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) container_type: ContainerType,
}

impl ContainerUnitContext {
    pub(in crate::layout) fn supplies_physical_width(self) -> bool {
        matches!(self.container_type, ContainerType::Size)
            || (matches!(self.container_type, ContainerType::InlineSize)
                && !self.writing_mode.has_vertical_lines())
    }

    pub(in crate::layout) fn supplies_physical_height(self) -> bool {
        matches!(self.container_type, ContainerType::Size)
            || (matches!(self.container_type, ContainerType::InlineSize)
                && self.writing_mode.has_vertical_lines())
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
    style.first_line_overrides = css::ModeledLonghandSet::empty();
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
    style.first_line_overrides = originating_style.first_line_overrides.clone();
    style.first_letter_style = originating_style.first_letter_style.clone();
    Some(style)
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct BlockLayoutOutcome {
    /// The signed bottom margin consumed by the preceding block layout.
    pub(in crate::layout) consumed_bottom_margin: LayoutLength,
    /// Whether the block's own block-start margin remained adjoining to its
    /// parent. This is local layout outcome state, not a global cursor epoch.
    pub(in crate::layout) margin_collapse_boundary: BlockMarginCollapseBoundary,
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
    /// This block selected a local automatic clamp point or captured a local
    /// discard-region break. Its following in-flow siblings are outside that
    /// continuation; this is not a page or column break.
    pub(in crate::layout) has_local_continuation_cutoff: bool,
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

/// Result captured while one block lays out its direct inline line sequence.
///
/// The line count crosses the legacy inline-layout boundary, while the local
/// cutoff flag preserves the distinct Overflow 4 continuation controller.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct ClampLineSlotCapture {
    pub(in crate::layout) line_slots: usize,
    /// Measured content-box advance of the selected inline source. This is
    /// the automatic-clamp traversal's debit at a mixed-flow boundary.
    pub(in crate::layout) block_advance: crate::units::ContentBoxLength,
    pub(in crate::layout) has_local_continuation_cutoff: bool,
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

/// The block-axis insets contributed by an active fragmented box.
///
/// The first fragment has already consumed its block-start border and
/// padding before child layout begins.  A `box-decoration-break: clone`
/// continuation must consume the same start inset and leave its matching end
/// inset available for the cloned decoration.  Keeping this alongside the
/// existing continuation-offset stack makes the reservation apply to child
/// layout rather than merely enlarging paint after fragmentation.
///
/// CSS Fragmentation requires the box's content box to fill the remaining
/// fragmentainer space while leaving room for cloned border and padding.
/// <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentTopOffset {
    first_fragment_start: f32,
    continuation_start: f32,
    continuation_end: f32,
}

impl FragmentTopOffset {
    pub(in crate::layout) const fn unreserved(first_fragment_start: f32) -> Self {
        Self {
            first_fragment_start,
            continuation_start: 0.0,
            continuation_end: 0.0,
        }
    }

    pub(in crate::layout) const fn cloned_block_decoration(
        first_fragment_start: f32,
        continuation_start: f32,
        continuation_end: f32,
    ) -> Self {
        Self {
            first_fragment_start,
            continuation_start,
            continuation_end,
        }
    }

    pub(in crate::layout) const fn first_fragment_start(self) -> f32 {
        self.first_fragment_start
    }

    pub(in crate::layout) const fn continuation_start(self) -> f32 {
        self.continuation_start
    }

    pub(in crate::layout) const fn continuation_end(self) -> f32 {
        self.continuation_end
    }
}

impl FragmentOffsets {
    pub(in crate::layout) const ZERO: Self = Self {
        left: 0.0,
        right: 0.0,
        top: 0.0,
    };

    /// Drops only the source fragment's block-start inset before entering a
    /// destination fragmentainer.
    ///
    /// Fragmentation restarts a continued box at the destination
    /// fragmentainer's block-start edge, but retains its block-end and inline
    /// insets. The stored fields use the page's physical coordinate system;
    /// `FlowAxes` performs the writing-mode projection at this boundary.
    /// <https://drafts.csswg.org/css-break-4/#box-splitting>
    /// <https://drafts.csswg.org/css-writing-modes-4/#abstract-box>
    pub(in crate::layout) fn clear_fragmentainer_block_start(&mut self, axes: FlowAxes) {
        match axes.block_start_side() {
            PhysicalSide::Top => self.top = 0.0,
            PhysicalSide::Right => self.right = 0.0,
            PhysicalSide::Left => self.left = 0.0,
            PhysicalSide::Bottom => {
                // Spindrift's horizontal principal flow is `horizontal-tb`, so
                // no active page continuation stores a physical bottom
                // cursor. Retain this branch to make any future block-upward
                // writing mode an explicit implementation decision.
                unreachable!("page continuation has no bottom-edge cursor");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::assets::DocumentPageIndex;

    #[test]
    fn fragment_offsets_only_clear_the_destination_block_start_inset() {
        let offsets = FragmentOffsets {
            left: 11.0,
            right: 13.0,
            top: 17.0,
        };

        let mut horizontal = offsets;
        horizontal.clear_fragmentainer_block_start(FlowAxes::new(
            WritingMode::HorizontalTb,
            Direction::Ltr,
        ));
        assert_eq!(
            horizontal,
            FragmentOffsets {
                top: 0.0,
                ..offsets
            }
        );

        let mut vertical_rl = offsets;
        vertical_rl.clear_fragmentainer_block_start(FlowAxes::new(
            WritingMode::VerticalRl,
            Direction::Ltr,
        ));
        assert_eq!(
            vertical_rl,
            FragmentOffsets {
                right: 0.0,
                ..offsets
            }
        );

        let mut vertical_lr = offsets;
        vertical_lr.clear_fragmentainer_block_start(FlowAxes::new(
            WritingMode::VerticalLr,
            Direction::Rtl,
        ));
        assert_eq!(
            vertical_lr,
            FragmentOffsets {
                left: 0.0,
                ..offsets
            }
        );
    }

    #[test]
    fn document_canvas_background_sources_remain_distinct() {
        assert_ne!(
            DocumentCanvasBackgroundSource::Root,
            DocumentCanvasBackgroundSource::EligibleBodyFallback
        );
    }

    #[test]
    fn render_options_do_not_expose_document_page_geometry() {
        let options = RenderOptions::default();
        assert_eq!(
            PageContext::from_options(&options).margins,
            PageMargins::DEFAULT
        );
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
    fn render_options_expose_and_validate_initial_viewport() {
        let mut options = RenderOptions::default();
        assert_eq!(
            options.initial_viewport_size(),
            options.media_environment().viewport
        );
        options
            .set_initial_viewport_size(crate::css::CssViewportSize::new(800.0, 600.0))
            .unwrap();
        assert_eq!(
            options.initial_viewport_size(),
            crate::css::CssViewportSize::new(800.0, 600.0)
        );
        assert_eq!(
            options.media_environment().viewport,
            crate::css::CssViewportSize::new(800.0, 600.0)
        );
        assert!(
            options
                .set_initial_viewport_size(crate::css::CssViewportSize::new(0.0, 600.0))
                .is_err()
        );
        assert!(
            options
                .set_initial_viewport_size(crate::css::CssViewportSize::new(f32::NAN, 600.0))
                .is_err()
        );
    }

    #[test]
    fn render_options_carry_color_scheme_preference_into_media_environment() {
        let options = RenderOptions {
            color_scheme_preference: crate::css::ColorSchemePreference::Dark,
            ..RenderOptions::default()
        };
        assert_eq!(
            options.media_environment().color_scheme_preference,
            crate::css::ColorSchemePreference::Dark,
        );
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

    #[test]
    fn completed_document_canvas_projects_root_track_for_vertical_and_sideways_flows() {
        let completion = CompletedDocumentCanvas {
            body: None,
            source_page: DocumentPageIndex::new(0),
            source_block_track: PageInlineSpan::from_edges(0.0, 100.0),
            inline_origin: PageTopBlockPosition::new(12.0),
            inline_end_inset: layout_pt(0.0),
            block_track_occupancy: layout_pt(100.0),
            block_end_inset: layout_pt(7.0),
            trailing_child_block_margin: layout_pt(11.0),
        };

        let vertical_rl = WritingModeAxes::new(WritingMode::VerticalRl, Direction::Ltr);
        let vertical_lr = WritingModeAxes::new(WritingMode::VerticalLr, Direction::Ltr);
        let sideways_rl = WritingModeAxes::new(WritingMode::SidewaysRl, Direction::Ltr);
        let sideways_lr = WritingModeAxes::new(WritingMode::SidewaysLr, Direction::Ltr);

        assert_eq!(
            completion.root_inline_block_track_advance(vertical_rl),
            layout_pt(111.0)
        );
        assert_eq!(
            completion.root_inline_block_track_advance(vertical_lr),
            layout_pt(107.0)
        );
        assert_eq!(
            completion.root_inline_block_track_advance(sideways_rl),
            layout_pt(118.0)
        );
        assert_eq!(
            completion.root_inline_block_track_advance(sideways_lr),
            layout_pt(118.0)
        );
    }

    #[test]
    fn principal_flow_page_progression_follows_writing_mode_not_inline_direction() {
        let principal = |writing_mode, direction| DocumentPrincipalFlow {
            writing_mode,
            direction,
            text_orientation: TextOrientation::Mixed,
            source: PrincipalFlowSource::Root,
        };

        assert_eq!(
            principal(WritingMode::HorizontalTb, Direction::Ltr).page_progression_direction(),
            Direction::Ltr
        );
        assert_eq!(
            principal(WritingMode::HorizontalTb, Direction::Rtl).page_progression_direction(),
            Direction::Rtl
        );
        assert_eq!(
            principal(WritingMode::VerticalRl, Direction::Ltr).page_progression_direction(),
            Direction::Rtl
        );
        assert_eq!(
            principal(WritingMode::VerticalRl, Direction::Rtl).page_progression_direction(),
            Direction::Rtl
        );
        assert_eq!(
            principal(WritingMode::VerticalLr, Direction::Rtl).page_progression_direction(),
            Direction::Ltr
        );
        assert_eq!(
            principal(WritingMode::SidewaysRl, Direction::Ltr).page_progression_direction(),
            Direction::Rtl
        );
        assert_eq!(
            principal(WritingMode::SidewaysLr, Direction::Rtl).page_progression_direction(),
            Direction::Ltr
        );
    }

    #[test]
    fn completed_document_canvas_selects_remaining_or_next_root_track() {
        let completion = CompletedDocumentCanvas {
            body: None,
            source_page: DocumentPageIndex::new(0),
            source_block_track: PageInlineSpan::from_edges(0.0, 100.0),
            inline_origin: PageTopBlockPosition::new(12.0),
            inline_end_inset: layout_pt(0.0),
            block_track_occupancy: layout_pt(100.0),
            block_end_inset: layout_pt(0.0),
            trailing_child_block_margin: layout_pt(0.0),
        };
        let axes = WritingModeAxes::new(WritingMode::VerticalRl, Direction::Ltr);

        assert!(!completion.exhausts_root_block_track(axes, 101.0));
        assert!(completion.exhausts_root_block_track(axes, 100.0));
    }

    #[test]
    fn completed_document_canvas_resolves_a_page_local_root_placement() {
        let completion = CompletedDocumentCanvas {
            body: None,
            source_page: DocumentPageIndex::new(2),
            source_block_track: PageInlineSpan::from_edges(0.0, 100.0),
            inline_origin: PageTopBlockPosition::new(17.0),
            inline_end_inset: layout_pt(0.0),
            block_track_occupancy: layout_pt(40.0),
            block_end_inset: layout_pt(0.0),
            trailing_child_block_margin: layout_pt(0.0),
        };
        let remaining = completion.resolve_root_inline_placement(
            WritingModeAxes::new(WritingMode::VerticalLr, Direction::Ltr),
            PageInlineSpan::from_edges(0.0, 100.0),
        );
        assert_eq!(
            remaining,
            RootInlineCanvasPlacement::RemainingTrack {
                block_track: PageInlineSpan::from_edges(40.0, 100.0),
                inline_origin: PageTopBlockPosition::new(17.0),
            }
        );

        let next_page = completion.resolve_root_inline_placement(
            WritingModeAxes::new(WritingMode::VerticalLr, Direction::Ltr),
            PageInlineSpan::from_edges(0.0, 40.0),
        );
        assert_eq!(
            next_page,
            RootInlineCanvasPlacement::NextPage {
                inline_origin: PageTopBlockPosition::new(17.0),
            }
        );
    }
}
