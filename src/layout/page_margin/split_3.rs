use super::*;

pub(in crate::layout) fn distribute_two_auto_sizes(
    available: f32,
    measures: [PageMarginBoxMeasure; 2],
) -> [f32; 2] {
    let available = available.max(0.0);
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
    let (rects, rounded_rects, paths, strokes) = block_paint_ops(
        layout.border_x(),
        layout.border_y(),
        layout.border_width(),
        layout.border_height(),
        style,
    );
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
    for image in background_images_for_style(
        BackgroundPaintArea {
            x: layout.border_x(),
            y: layout.border_y(),
            width: layout.border_width(),
            height: layout.border_height(),
        },
        style,
        context.base_url,
        context.root_url,
        context.resource_cache,
    ) {
        page.push_image_in_band(PaintBand::BackgroundBorder, image);
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
    if style.outline_width <= 0.0 || style.outline_style.suppresses_used_width() {
        return Vec::new();
    }
    if layout.border_width() <= 0.0 || layout.border_height() <= 0.0 {
        return Vec::new();
    }

    let mut outline_style = style.clone();
    outline_style.background_color = None;
    outline_style.background_image = None;
    outline_style.background_layers.clear();
    outline_style.border_image = css::BorderImage::initial();
    outline_style.border_width = style.outline_width;
    outline_style.border_widths = css::Edges {
        top: style.outline_width,
        right: style.outline_width,
        bottom: style.outline_width,
        left: style.outline_width,
    };
    outline_style.border_width_values = css::CssEdges::all(
        css::ComputedLengthPercentage::from_points(style.outline_width),
    );
    outline_style.border_color = style.outline_color;
    outline_style.border_colors = css::BorderColors {
        top: style.outline_color,
        right: style.outline_color,
        bottom: style.outline_color,
        left: style.outline_color,
    };
    outline_style.border_styles = css::BorderStyles {
        top: style.outline_style,
        right: style.outline_style,
        bottom: style.outline_style,
        left: style.outline_style,
    };

    let outset = style.outline_offset.length_points() + style.outline_width;
    let (rects, rounded_rects, paths, strokes) = block_paint_ops(
        layout.border_x() - outset,
        layout.border_y() - outset,
        layout.border_width() + outset * 2.0,
        layout.border_height() + outset * 2.0,
        &outline_style,
    );
    let mut primitives = Vec::new();
    primitives.extend(rects.into_iter().map(PaintPrimitive::Rect));
    primitives.extend(rounded_rects.into_iter().map(PaintPrimitive::RoundedRect));
    primitives.extend(paths.into_iter().map(PaintPrimitive::Path));
    primitives.extend(strokes.into_iter().map(PaintPrimitive::Stroke));
    primitives
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

pub(in crate::layout) fn margin_box_intrinsic_inline_sizes(
    font_system: &mut FontSystem,
    content: &ResolvedPageContent,
    style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
    resource_cache: &ResourceCache,
) -> (f32, f32) {
    let mut min_content: f32 = 0.0;
    let mut max_content: f32 = 0.0;
    let mut paragraph = Vec::new();
    for item in page_margin_intrinsic_inline_items(
        content,
        style,
        available_width,
        base_url,
        root_url,
        resource_cache,
    ) {
        if matches!(item, InlineItem::Break(_)) {
            accumulate_page_margin_intrinsic_paragraph(
                font_system,
                &mut paragraph,
                style,
                &mut min_content,
                &mut max_content,
            );
        } else {
            paragraph.push(item);
        }
    }
    accumulate_page_margin_intrinsic_paragraph(
        font_system,
        &mut paragraph,
        style,
        &mut min_content,
        &mut max_content,
    );
    if min_content == 0.0 {
        min_content = max_content;
    }
    (min_content, max_content)
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
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
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
            | PageMarginContentItem::TargetText { .. } => {}
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
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
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
                0.0,
                inline_style.line_height,
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
            if let Some(image) = used_generated_image_value(
                image,
                box_style,
                available_width,
                base_url,
                root_url,
                resource_cache,
            ) {
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                items.push(InlineItem::Atom(Box::new(InlineAtom::new(
                    InlineAtomContent::Image(image.decoded),
                    inline_style.clone(),
                    None,
                    border_box_width,
                    border_box_height,
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
    *min_content = (*min_content).max(contribution.min_content);
    *max_content = (*max_content).max(contribution.max_content);
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
    inline_style.background_image = None;
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
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
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
                0.0,
                inline_style.line_height,
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
            if let Some(image) = used_generated_image_value(
                image,
                box_style,
                available_width,
                base_url,
                root_url,
                resource_cache,
            ) {
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                items.push(InlineItem::Atom(Box::new(InlineAtom::new(
                    InlineAtomContent::Image(image.decoded),
                    inline_style.clone(),
                    None,
                    border_box_width,
                    border_box_height,
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
