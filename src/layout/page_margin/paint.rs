use super::*;
use crate::layout::assets::background_image_primitives_for_style;

/// Replays page-margin boxes into the page display list in stacking order.
///
/// CSS Paged Media paints generated page-margin boxes using clockwise tree
/// order by default, but each page-margin box establishes a stacking context
/// and honors `z-index` relative to the document canvas/content stack:
/// <https://www.w3.org/TR/css-page-3/#painting>.
pub(super) fn replay_page_margin_box_fragments(
    page: &mut Page,
    mut boxes: Vec<PageMarginPaintedBox>,
) {
    // Margin boxes are generated after the document page tree has been laid
    // out.  Non-negative boxes occupy the post-document generated-content
    // slot, while their relative ordering remains the page-margin tree order.
    // Give that slot source-order values after any document stacking context;
    // `order` is only sixteen entries, so the subtraction cannot underflow.
    const POST_DOCUMENT_SOURCE_ORDER: usize = usize::MAX - 16;
    boxes.sort_by_key(|box_| (box_.z_index, box_.order));

    for box_ in boxes {
        let context = PaintStackingContext::new(box_.z_index, box_.fragment, Vec::new())
            .with_effects(box_.effects)
            .with_bounds(box_.bounds)
            .with_source_order(POST_DOCUMENT_SOURCE_ORDER + box_.order);
        if box_.z_index < 0 {
            let fragment =
                PaintFragment::from_stacking_context_in_band(PaintBand::PageBackground, context);
            page.prepend_paint_fragment_owned(fragment, PaintTranslation::identity());
        } else {
            let fragment = PaintFragment::from_stacking_context(context);
            page.append_paint_fragment_owned(fragment, PaintTranslation::identity());
        }
    }
    // Margin boxes are replayed after normal document layout, so their
    // stacking contexts must be ordered here rather than inheriting the
    // declaration/hash-map insertion order used while collecting them.
    // https://www.w3.org/TR/css-page-3/#painting
    page.sort_paint_tree_stacking_contexts();
}

pub(super) fn page_margin_box_paint_order(name: &str) -> usize {
    // CSS Paged Media's page-margin tree paints in one continuous clockwise
    // walk. This is independent of author declaration order.
    // <https://www.w3.org/TR/css-page-3/#painting>
    const PAINT_ORDER: &[&str] = &[
        "top-left-corner",
        "top-left",
        "top-center",
        "top-right",
        "top-right-corner",
        "right-top",
        "right-middle",
        "right-bottom",
        "bottom-right-corner",
        "bottom-right",
        "bottom-center",
        "bottom-left",
        "bottom-left-corner",
        "left-bottom",
        "left-middle",
        "left-top",
    ];
    PAINT_ORDER
        .iter()
        .position(|candidate| *candidate == name)
        .unwrap_or(PAINT_ORDER.len())
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
        PaintPrimitive::OpaqueTextCoverage { line, paths } => {
            page.push_opaque_text_coverage_in_band(band, line, paths)
        }
        PaintPrimitive::SvgTextOutline { paths, actual_text } => {
            page.push_svg_text_outline_in_band(band, paths, actual_text)
        }
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
