use super::*;

/// Resolves the outer sizes of the two boxes used by a variable-axis step.
///
/// When exactly one box is automatic, it receives the space left by its
/// definite peer. When both are automatic, CSS Paged Media distributes the
/// space by their intrinsic outer dimensions.
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>
pub(in crate::layout) fn resolve_two_outer_sizes(
    available: f32,
    measures: [PageMarginBoxMeasure; 2],
) -> [f32; 2] {
    let available = available.max(0.0);
    let mut sizes = [
        measures[0].resolved_or_zero(),
        measures[1].resolved_or_zero(),
    ];
    match (measures[0].auto_outer(), measures[1].auto_outer()) {
        (false, false) => return sizes,
        (true, false) => {
            sizes[0] = (available - sizes[1]).max(0.0);
            return sizes;
        }
        (false, true) => {
            sizes[1] = (available - sizes[0]).max(0.0);
            return sizes;
        }
        (true, true) => {}
    }
    let max_sum = measures[0].max_outer + measures[1].max_outer;
    let min_sum = measures[0].min_outer + measures[1].min_outer;
    if max_sum < available {
        let flex_space = available - max_sum;
        let factors = normalized_flex_factors([measures[0].max_outer, measures[1].max_outer]);
        [
            measures[0].max_outer + flex_space * factors[0],
            measures[1].max_outer + flex_space * factors[1],
        ]
    } else if min_sum < available {
        let flex_space = available - min_sum;
        let factors = normalized_flex_factors([
            (measures[0].max_outer - measures[0].min_outer).max(0.0),
            (measures[1].max_outer - measures[1].min_outer).max(0.0),
        ]);
        [
            measures[0].min_outer + flex_space * factors[0],
            measures[1].min_outer + flex_space * factors[1],
        ]
    } else {
        let factors = normalized_flex_factors([measures[0].min_outer, measures[1].min_outer]);
        [available * factors[0], available * factors[1]]
    }
}

/// Resolve a two-box CSS Page allocation, repeating it for saturated min/max
/// constraints instead of independently clamping the first result.
///
/// CSS Paged Media §5.3.2 says a violated maximum is used as the computed
/// dimension and the allocation is rerun; minima are handled by the same
/// mechanism after maximums. This is also used for the centre box and each
/// separate imaginary symmetric side candidate.
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>
pub(in crate::layout) fn resolve_two_outer_sizes_with_constraints(
    available: f32,
    measures: [PageMarginBoxMeasure; 2],
) -> [f32; 2] {
    let mut saturated = measures;
    loop {
        let sizes = resolve_two_outer_sizes(available, saturated);
        let Some((index, value)) = saturated.iter().enumerate().find_map(|(index, measure)| {
            measure
                .max_constraint
                .filter(|maximum| sizes[index] > *maximum)
                .map(|maximum| (index, maximum))
        }) else {
            break;
        };
        saturated[index] = saturated[index].with_definite_outer(value);
    }
    loop {
        let sizes = resolve_two_outer_sizes(available, saturated);
        let Some((index, value)) = saturated.iter().enumerate().find_map(|(index, measure)| {
            measure
                .min_constraint
                .filter(|minimum| sizes[index] < *minimum)
                .map(|minimum| (index, minimum))
        }) else {
            return sizes;
        };
        saturated[index] = saturated[index].with_definite_outer(value);
    }
}

pub(in crate::layout) fn normalized_flex_factors(values: [f32; 2]) -> [f32; 2] {
    let sum = values[0] + values[1];
    if sum <= 0.0 {
        [0.5, 0.5]
    } else {
        [values[0] / sum, values[1] / sum]
    }
}

pub(in crate::layout) fn paint_page_margin_box(
    page: &mut Page,
    layout: &PageMarginBoxLayout<'_>,
    context: PageMarginPaintContext<'_>,
) {
    let style = &layout.spec.style;
    if style.visibility != Visibility::Visible {
        return;
    }
    let (rects, rounded_rects, paths, strokes) = block_paint_ops(layout.border_rect, style);
    for rect in rects {
        page.push_rect_in_band(PaintBand::BackgroundBorder, rect);
    }
    for rect in rounded_rects {
        page.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rect);
    }
    for path in paths {
        page.push_path_in_band(PaintBand::BackgroundBorder, path);
    }
    for stroke in strokes {
        page.push_stroke_in_band(PaintBand::BackgroundBorder, stroke);
    }
    for primitive in background_image_primitives_for_style(
        PaintBackgroundArea::from_paint_rect(layout.border_rect),
        style,
        context.base_url,
        context.root_url,
        context.resource_cache,
    ) {
        push_page_margin_primitive(page, PaintBand::BackgroundBorder, primitive);
    }
    for primitive in page_margin_box_outline_primitives(layout, style) {
        push_page_margin_primitive(page, PaintBand::Outline, primitive);
    }
}

pub(in crate::layout) fn push_page_margin_primitive(
    page: &mut Page,
    band: PaintBand,
    primitive: PaintPrimitive,
) {
    match primitive {
        PaintPrimitive::Rect(rect) => page.push_rect_in_band(band, rect),
        PaintPrimitive::RoundedRect(rect) => page.push_rounded_rect_in_band(band, rect),
        PaintPrimitive::Path(path) => page.push_path_in_band(band, path),
        PaintPrimitive::Stroke(stroke) => page.push_stroke_in_band(band, stroke),
        PaintPrimitive::Image(image) => page.push_image_in_band(band, image),
        PaintPrimitive::ImagePattern(pattern) => page.push_image_pattern_in_band(band, pattern),
        PaintPrimitive::GradientPattern(pattern) => {
            page.push_gradient_pattern_in_band(band, pattern)
        }
        PaintPrimitive::SvgPattern(pattern) => page.push_svg_pattern_in_band(band, pattern),
        PaintPrimitive::Line(line) => page.push_line_in_band(band, line),
    };
}

/// Builds outline paint for a generated page-margin box without affecting layout.
///
/// CSS UI defines outlines as visual paint outside the border edge that does not
/// participate in sizing, while CSS Paged Media applies that property set in
/// margin contexts:
/// <https://www.w3.org/TR/css-ui-3/#outline-props> and
/// <https://www.w3.org/TR/css-page-3/#page-properties>.
pub(in crate::layout) fn page_margin_box_outline_primitives(
    layout: &PageMarginBoxLayout<'_>,
    style: &ComputedStyle,
) -> Vec<PaintPrimitive> {
    crate::layout::paint_ops::outline_primitives_for_border_rect(layout.border_rect, style)
}

/// Returns the top edge of the page-margin text line stack.
///
/// CSS Paged Media defines page-margin boxes as generated boxes with their own
/// content area, and CSS Inline positions text through line-box baselines. The
/// `vertical-align` value chooses where the text stack sits inside the margin
/// box content area; baseline placement is then handled from font metrics:
/// <https://www.w3.org/TR/css-page-3/#page-margin-boxes> and
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
pub(in crate::layout) fn page_margin_text_stack_top(
    layout: &PageMarginBoxLayout<'_>,
    vertical_align: VerticalAlign,
    total_height: f32,
) -> f32 {
    if matches!(vertical_align.baseline_shift, BaselineShift::Bottom)
        || matches!(
            vertical_align.alignment_baseline,
            AlignmentBaseline::Metric(BaselineMetric::TextBottom)
        )
    {
        layout.content_y() + total_height
    } else if matches!(vertical_align.baseline_shift, BaselineShift::Top)
        || matches!(
            vertical_align.alignment_baseline,
            AlignmentBaseline::Metric(BaselineMetric::TextTop)
        )
    {
        layout.content_y() + layout.content_height()
    } else {
        layout.content_y() + ((layout.content_height() + total_height) / 2.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginLogicalInlineSize(f32);

impl PageMarginLogicalInlineSize {
    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginLogicalBlockSize(f32);

impl PageMarginLogicalBlockSize {
    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginPhysicalX(f32);

impl PageMarginPhysicalX {
    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginPhysicalY(f32);

impl PageMarginPhysicalY {
    pub(in crate::layout) fn new(points: f32) -> Self {
        Self(points)
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginFixedBoxGeometry {
    line_block_start_x: PageMarginPhysicalX,
    line_inline_start_y: PageMarginPhysicalY,
    inline_size: PageMarginLogicalInlineSize,
    block_size: PageMarginLogicalBlockSize,
}

impl PageMarginFixedBoxGeometry {
    /// Convert a page-margin content rectangle into logical fixed-box axes.
    ///
    /// CSS Paged Media gives margin boxes fixed physical rectangles, while CSS
    /// Writing Modes maps inline layout through logical inline and block axes.
    /// Keeping this conversion typed makes it explicit that vertical writing
    /// uses the content box's physical height as logical inline size and its
    /// physical width as logical block size:
    /// <https://www.w3.org/TR/css-page-3/#page-margin-boxes> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(in crate::layout) fn from_layout(layout: &PageMarginBoxLayout<'_>) -> Self {
        let style = &layout.spec.style;
        let physical_left = PageMarginPhysicalX(layout.content_x());
        let physical_right = PageMarginPhysicalX(layout.content_x() + layout.content_width());
        let physical_top = PageMarginPhysicalY(layout.content_y() + layout.content_height());
        let (inline_size, block_size) = match style.writing_mode {
            WritingMode::HorizontalTb => (
                PageMarginLogicalInlineSize(layout.content_width().max(1.0)),
                PageMarginLogicalBlockSize(layout.content_height().max(0.0)),
            ),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => (
                PageMarginLogicalInlineSize(layout.content_height().max(1.0)),
                PageMarginLogicalBlockSize(layout.content_width().max(0.0)),
            ),
        };
        let line_block_start_x = match style.writing_mode {
            // The block-start edge of a right-to-left vertical writing mode
            // is the physical right edge. Inline line layout subtracts each
            // line's block size from this origin as it advances leftward.
            WritingMode::VerticalRl | WritingMode::SidewaysRl => physical_right,
            WritingMode::HorizontalTb | WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                physical_left
            }
        };
        Self {
            line_block_start_x,
            line_inline_start_y: physical_top,
            inline_size,
            block_size,
        }
    }

    pub(in crate::layout) fn inline_size(self) -> PageMarginLogicalInlineSize {
        self.inline_size
    }

    pub(in crate::layout) fn block_size(self) -> PageMarginLogicalBlockSize {
        self.block_size
    }

    pub(in crate::layout) fn line_block_start_x(self) -> PageMarginPhysicalX {
        self.line_block_start_x
    }

    pub(in crate::layout) fn line_inline_start_y(self) -> PageMarginPhysicalY {
        self.line_inline_start_y
    }

    pub(in crate::layout) fn with_line_inline_start(mut self, y: PageMarginPhysicalY) -> Self {
        self.line_inline_start_y = y;
        self
    }

    pub(in crate::layout) fn with_line_block_alignment(
        mut self,
        _line_stack_block_size: f32,
        first_line_block_size: f32,
        _vertical_align: VerticalAlign,
        writing_mode: WritingMode,
    ) -> Self {
        // Vertical right-to-left line layout starts a column by painting to
        // the left of its block cursor. Place that first cursor one line
        // block-size inside the physical right edge; subsequent columns then
        // advance left without escaping the margin-box content rectangle.
        // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
        if matches!(
            writing_mode,
            WritingMode::VerticalRl | WritingMode::SidewaysRl
        ) {
            self.line_block_start_x.0 -= first_line_block_size;
        }
        self
    }
}

pub(in crate::layout) fn margin_box_intrinsic_inline_sizes(
    font_system: &mut FontSystem,
    content: &ResolvedPageContent,
    style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> (f32, f32) {
    let mut min_content: f32 = 0.0;
    let mut max_content: f32 = 0.0;
    let mut paragraph = Vec::new();
    let items = page_margin_intrinsic_inline_items(
        content,
        style,
        available_width,
        base_url,
        root_url,
        resource_cache,
    );
    for item in &items {
        if matches!(item, InlineItem::Break(_)) {
            accumulate_page_margin_intrinsic_paragraph(
                font_system,
                &mut paragraph,
                style,
                &mut min_content,
                &mut max_content,
            );
        } else {
            paragraph.push(item.clone());
        }
    }
    accumulate_page_margin_intrinsic_paragraph(
        font_system,
        &mut paragraph,
        style,
        &mut min_content,
        &mut max_content,
    );
    if min_content == 0.0 && max_content == 0.0 {
        // CSS Text permits other space separators to hang at an inline end,
        // but a generated page-margin box with only such content still has a
        // non-zero intrinsic contribution for CSS Page's variable-dimension
        // algorithm. Otherwise `content: "\\a0"` becomes indistinguishable
        // from `content: ""` and incorrectly receives no share of a margin
        // triplet. WPT: dimensions-010-print.html.
        // https://www.w3.org/TR/css-page-3/#margin-dimension
        // https://www.w3.org/TR/css-text-3/#white-space-phase-2
        let generated_advance = page_margin_generated_inline_advance(&items, font_system);
        min_content = generated_advance;
        max_content = generated_advance;
    }
    if min_content == 0.0 {
        min_content = max_content;
    }
    (min_content, max_content)
}

/// Returns the widest forced paragraph advance in a generated content list.
///
/// This fallback is deliberately limited to content whose regular intrinsic
/// contribution is zero. It keeps a lone non-breaking space visible to CSS
/// Page sizing without changing normal CSS Text line selection or the
/// treatment of ordinary text and break opportunities.
fn page_margin_generated_inline_advance(items: &[InlineItem], font_system: &mut FontSystem) -> f32 {
    let mut paragraph_advance = 0.0;
    let mut max_advance: f32 = 0.0;
    for item in items {
        match item {
            InlineItem::Word(word) => {
                paragraph_advance += font_system.measure_text(&word.text, &word.style);
            }
            InlineItem::Atom(atom) => {
                paragraph_advance += atom.size.width;
            }
            InlineItem::Break(_) => {
                max_advance = max_advance.max(paragraph_advance);
                paragraph_advance = 0.0;
            }
            InlineItem::Float(_) | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {}
        }
    }
    max_advance.max(paragraph_advance)
}

/// Builds page-margin intrinsic sizing input from generated inline content.
///
/// Page-margin boxes size themselves from CSS Text intrinsic contributions.
/// This helper preserves the generated inline stream before line selection so
/// min/max-content sizing sees the same transformed text, tab stops, soft-wrap
/// opportunities, generated images, and hanging punctuation as final
/// page-margin painting:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
pub(in crate::layout) fn page_margin_intrinsic_inline_items(
    content: &ResolvedPageContent,
    style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<InlineItem> {
    let mut items = Vec::new();
    let mut quote_depth = 0usize;
    let mut text_buffer = String::new();
    let inline_style = page_margin_inline_content_style(style);
    for item in &content.items {
        match item {
            PageMarginContentItem::EmbeddedRunningElement(capture) => {
                let parts = running_element_inline_parts(capture);
                for part in &parts {
                    append_page_margin_intrinsic_part(
                        &mut items,
                        &mut text_buffer,
                        &mut quote_depth,
                        part,
                        &inline_style,
                        style,
                        available_width,
                        base_url,
                        root_url,
                        resource_cache,
                    );
                }
            }
            PageMarginContentItem::Inline(part) => append_page_margin_intrinsic_part(
                &mut items,
                &mut text_buffer,
                &mut quote_depth,
                part,
                &inline_style,
                style,
                available_width,
                base_url,
                root_url,
                resource_cache,
            ),
            PageMarginContentItem::TargetCounter { .. }
            | PageMarginContentItem::TargetText { .. }
            | PageMarginContentItem::NamedStringPageCounter { .. } => {}
        }
    }
    flush_page_margin_intrinsic_text_buffer(&mut items, &mut text_buffer, style);
    items
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn append_page_margin_intrinsic_part(
    items: &mut Vec<InlineItem>,
    text_buffer: &mut String,
    quote_depth: &mut usize,
    part: &GeneratedContentPart,
    inline_style: &ComputedStyle,
    box_style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) {
    match part {
        GeneratedContentPart::Text(text) => {
            text_buffer.push_str(text);
        }
        GeneratedContentPart::Leader(text) => {
            flush_page_margin_intrinsic_text_buffer(items, text_buffer, inline_style);
            items.push(InlineItem::Atom(Box::new(InlineAtom::new(
                InlineAtomContent::Leader(text.clone()),
                inline_style.clone(),
                None,
                InlineSize::new(0.0, inline_style.line_height),
                inline_style.font_size,
                0.0,
                None,
                None,
            ))));
        }
        GeneratedContentPart::Quote(quote) => {
            let text = page_margin_quote_text(*quote, inline_style, quote_depth);
            text_buffer.push_str(&text);
        }
        GeneratedContentPart::Image { image } => {
            flush_page_margin_intrinsic_text_buffer(items, text_buffer, inline_style);
            if let Some(image) = image.as_image().and_then(|image| {
                used_generated_image_value(
                    image,
                    box_style,
                    available_width,
                    base_url,
                    root_url,
                    resource_cache,
                )
            }) {
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                let content = image
                    .svg
                    .map(|asset| InlineAtomContent::Svg { asset: Some(asset) })
                    .unwrap_or(InlineAtomContent::Image(image.decoded));
                items.push(InlineItem::Atom(Box::new(InlineAtom::new(
                    content,
                    inline_style.clone(),
                    None,
                    InlineSize::new(border_box_width, border_box_height),
                    border_box_height,
                    0.0,
                    None,
                    None,
                ))));
            }
        }
        GeneratedContentPart::Contents
        | GeneratedContentPart::Attr { .. }
        | GeneratedContentPart::Counter { .. }
        | GeneratedContentPart::Counters { .. }
        | GeneratedContentPart::TargetCounter { .. }
        | GeneratedContentPart::TargetText { .. } => {}
    }
}

pub(in crate::layout) fn flush_page_margin_intrinsic_text_buffer(
    output: &mut Vec<InlineItem>,
    text: &mut String,
    style: &ComputedStyle,
) {
    if !text.is_empty() {
        push_inline_words_for_style(text, style, None, 0.0, InlineVisualOffset::zero(), output);
        normalize_inline_whitespace_items(output);
        text.clear();
    }
}

pub(in crate::layout) fn accumulate_page_margin_intrinsic_paragraph(
    font_system: &mut FontSystem,
    paragraph: &mut Vec<InlineItem>,
    style: &ComputedStyle,
    min_content: &mut f32,
    max_content: &mut f32,
) {
    trim_inline_item_edges(paragraph);
    if paragraph.is_empty() {
        return;
    }
    let graph = inline_layout::build_inline_opportunity_graph(font_system, paragraph.iter(), style);
    let contribution = graph.intrinsic_contribution(font_system, style);
    *min_content = (*min_content).max(contribution.min_content.points());
    *max_content = (*max_content).max(contribution.max_content.points());
    paragraph.clear();
}

pub(in crate::layout) fn running_element_inline_parts(
    capture: &RunningElementCapture,
) -> Vec<GeneratedContentPart> {
    if !capture.content_parts.is_empty() {
        return capture.content_parts.clone();
    }
    if capture.fallback_text.is_empty() {
        Vec::new()
    } else {
        vec![GeneratedContentPart::Text(capture.fallback_text.clone())]
    }
}

/// Derives the inline content style used by generated page-margin text.
///
/// CSS Paged Media creates a margin box whose background, border, padding, and
/// outline paint on the margin box itself, while CSS Generated Content supplies
/// inline content inside that box. Reusing the box style directly for inline
/// fragments would duplicate the margin-box border/background around each text
/// run:
/// <https://www.w3.org/TR/css-page-3/#page-margin-boxes> and
/// <https://www.w3.org/TR/css-content-3/#content-property>.
pub(in crate::layout) fn page_margin_inline_content_style(style: &ComputedStyle) -> ComputedStyle {
    let mut inline_style = style.clone();
    // Page-margin `vertical-align` positions the generated content stack in
    // the fixed margin rectangle. It is not an inline-level alignment for the
    // anonymous generated text/image runs themselves, which retain their
    // synthesized baselines.
    // https://www.w3.org/TR/css-page-3/#margin-boxes
    // https://www.w3.org/TR/css-inline-3/#valdef-vertical-align-baseline
    inline_style.vertical_align = VerticalAlign::BASELINE;
    inline_style.margin = css::Edges::ZERO;
    inline_style.ua_margin_em = css::OptionalEdges::NONE;
    inline_style.padding = css::Edges::ZERO;
    inline_style.border_width = 0.0;
    inline_style.border_widths = css::Edges::ZERO;
    inline_style.border_width_values = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
    inline_style.border_styles = css::BorderStyles::NONE;
    inline_style.border_radius = css::BorderRadius::ZERO;
    inline_style.corner_shapes = css::CornerShapes::ROUND;
    inline_style.border_image = css::BorderImage::initial();
    inline_style.outline_width = 0.0;
    inline_style.outline_width_value = css::ComputedLengthPercentage::ZERO;
    inline_style.outline_style = css::BorderStyle::None;
    inline_style.outline_offset = css::ComputedLengthPercentage::ZERO;
    inline_style.background_color = None;
    inline_style.background_image = css::ComputedImage::None;
    inline_style.background_layers.clear();
    inline_style
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn append_page_margin_inline_part(
    items: &mut Vec<InlineItem>,
    part: &GeneratedContentPart,
    inline_style: &ComputedStyle,
    box_style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    quote_depth: &mut usize,
) {
    match part {
        GeneratedContentPart::Text(text) => {
            push_inline_words_for_style(
                text,
                inline_style,
                None,
                0.0,
                InlineVisualOffset::zero(),
                items,
            );
        }
        GeneratedContentPart::Leader(text) => {
            items.push(InlineItem::Atom(Box::new(InlineAtom::new(
                InlineAtomContent::Leader(text.clone()),
                inline_style.clone(),
                None,
                InlineSize::new(0.0, inline_style.line_height),
                inline_style.font_size,
                0.0,
                None,
                None,
            ))));
        }
        GeneratedContentPart::Quote(quote) => {
            let text = page_margin_quote_text(*quote, inline_style, quote_depth);
            push_inline_words_for_style(
                &text,
                inline_style,
                None,
                0.0,
                InlineVisualOffset::zero(),
                items,
            );
        }
        GeneratedContentPart::Image { image } => {
            if let Some(image) = image.as_image().and_then(|image| {
                used_generated_image_value(
                    image,
                    box_style,
                    available_width,
                    base_url,
                    root_url,
                    resource_cache,
                )
            }) {
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                let content = image
                    .svg
                    .map(|asset| InlineAtomContent::Svg { asset: Some(asset) })
                    .unwrap_or(InlineAtomContent::Image(image.decoded));
                items.push(InlineItem::Atom(Box::new(InlineAtom::new(
                    content,
                    inline_style.clone(),
                    None,
                    InlineSize::new(border_box_width, border_box_height),
                    border_box_height,
                    0.0,
                    None,
                    None,
                ))));
            }
        }
        GeneratedContentPart::Contents
        | GeneratedContentPart::Attr { .. }
        | GeneratedContentPart::Counter { .. }
        | GeneratedContentPart::Counters { .. }
        | GeneratedContentPart::TargetCounter { .. }
        | GeneratedContentPart::TargetText { .. } => {}
    }
}

pub(in crate::layout) fn page_margin_quote_text(
    quote: GeneratedQuote,
    style: &ComputedStyle,
    quote_depth: &mut usize,
) -> String {
    match quote {
        GeneratedQuote::Open => {
            let text = page_margin_quote_pair(style, *quote_depth).0;
            *quote_depth += 1;
            text
        }
        GeneratedQuote::Close => {
            *quote_depth = quote_depth.saturating_sub(1);
            page_margin_quote_pair(style, *quote_depth).1
        }
        GeneratedQuote::NoOpen => {
            *quote_depth += 1;
            String::new()
        }
        GeneratedQuote::NoClose => {
            *quote_depth = quote_depth.saturating_sub(1);
            String::new()
        }
    }
}

pub(in crate::layout) fn page_margin_quote_pair(
    style: &ComputedStyle,
    depth: usize,
) -> (String, String) {
    match &style.quotes {
        Quotes::None => (String::new(), String::new()),
        Quotes::Pairs(pairs) => pairs
            .get(depth)
            .or_else(|| pairs.last())
            .cloned()
            .unwrap_or_else(|| ("“".to_string(), "”".to_string())),
        Quotes::Auto { .. } => {
            let (open, close) = quotes::language_quote_pair(style.quotes.auto_language(), depth);
            (open.to_string(), close.to_string())
        }
    }
}
