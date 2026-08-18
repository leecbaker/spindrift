use super::*;
use crate::layout::inline_collect::normalize_inline_whitespace_items;
use crate::layout::page_generated::{
    PageContentResolveContext, PageMarginContentItem, ResolvedPageContent,
    resolve_page_content_parts,
};

#[derive(Clone, Copy)]
pub(in crate::layout) struct PageMarginPaintContext<'a> {
    pub(in crate::layout) page_margins: PageMargins,
    pub(in crate::layout) page_edges: PageBoxEdges,
    pub(in crate::layout) page_number: usize,
    pub(in crate::layout) total_pages: usize,
    pub(in crate::layout) base_url: Option<&'a url::Url>,
    pub(in crate::layout) root_url: Option<&'a url::Url>,
    pub(in crate::layout) resource_cache: &'a ResourceCache,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) page_named_strings: &'a [HashMap<String, Vec<NamedStringAssignment>>],
    pub(in crate::layout) page_running_elements:
        &'a [HashMap<String, Vec<NamedStringAssignment>>],
    pub(in crate::layout) page_anchors: &'a HashMap<String, usize>,
    pub(in crate::layout) page_anchor_text: &'a HashMap<String, AnchorText>,
    pub(in crate::layout) counter_styles: &'a HashMap<String, CounterStyleRule>,
    pub(in crate::layout) page_counters: &'a HashMap<String, i32>,
    pub(in crate::layout) page_counters_by_page: &'a [HashMap<String, i32>],
    pub(in crate::layout) image_set_resolution_dppx: f32,
}

/// Computes used page-margin box rectangles for one generated page.
///
/// CSS Paged Media Level 3 defines sixteen margin boxes, generation from the
/// `content` property, coordinated variable dimensions for side triplets, and
/// fixed dimensions in the perpendicular axis:
/// <https://www.w3.org/TR/css-page-3/#margin-boxes> and
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>.
pub(in crate::layout) fn resolved_margin_box_content(
    spec: &PageMarginBoxSpec,
    context: PageMarginPaintContext<'_>,
) -> Option<ResolvedPageContent> {
    let value = spec.declarations.get("content")?;
    let trimmed = css::trim_css_value(value);
    if trimmed.eq_ignore_ascii_case("normal") || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    let mut box_counters = context.page_counters.clone();
    apply_page_margin_box_counter_scope(&mut box_counters, &spec.style);
    resolve_page_content_parts(
        value,
        PageContentResolveContext {
            page_number: context.page_number,
            total_pages: context.total_pages,
            page_index: context.page_index,
            base_url: context.base_url,
            root_url: context.root_url,
            page_named_strings: context.page_named_strings,
            page_running_elements: context.page_running_elements,
            page_anchors: context.page_anchors,
            page_anchor_text: context.page_anchor_text,
            counter_styles: context.counter_styles,
            page_counters: &box_counters,
            page_counters_by_page: context.page_counters_by_page,
            used_color_scheme: spec.style.used_color_scheme,
            image_set_resolution_dppx: context.image_set_resolution_dppx,
        },
    )
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
                let content = image.into_inline_atom_content();
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
    inline_style.background.background_color = css::BackgroundColor::TRANSPARENT;
    inline_style.background.background_image = css::ComputedImage::None;
    inline_style.background.background_layers.clear();
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
                let content = image.into_inline_atom_content();
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
        Quotes::Auto(_) => {
            let (open, close) = style.quotes.auto_quote_pair(depth);
            (open.to_string(), close.to_string())
        }
    }
}
