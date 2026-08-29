use super::*;

/// Resolves CSS page border and padding declarations to used physical edges.
///
/// CSS Paged Media applies border and padding to the page box, and CSS Box
/// Model resolves padding percentages against the containing block's inline
/// size before layout consumes used values:
/// <https://www.w3.org/TR/css-page-3/#page-model> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
#[allow(dead_code)]
pub(in crate::layout) fn page_box_edges_from_declarations_with_ch_advance(
    declarations: &Declarations,
    page_size: PageSize,
    ch_advance: LayoutLength,
) -> PageBoxEdges {
    let mut style = ComputedStyle::initial();
    css::apply_declarations(&mut style, declarations);
    page_box_edges_from_declarations_with_ch_advance_and_root_metrics(
        declarations,
        page_size,
        ch_advance,
        css::RootFontMetricLengthBasis {
            font_size: layout_pt(style.font_size),
            ch_advance,
            x_height: layout_pt(style.font_size * 0.5),
            cap_height: layout_pt(style.font_size * 0.7),
            ic_advance: ch_advance,
            line_height: layout_pt(style.line_height),
        },
    )
}

pub(in crate::layout) fn page_box_edges_from_declarations_with_ch_advance_and_root_metrics(
    declarations: &Declarations,
    page_size: PageSize,
    ch_advance: LayoutLength,
    root_metrics: css::RootFontMetricLengthBasis,
) -> PageBoxEdges {
    if declarations.is_empty() {
        return PageBoxEdges::ZERO;
    }
    let mut style = ComputedStyle::initial();
    css::apply_declarations(&mut style, declarations);
    style.resolve_font_metric_lengths(ch_advance);
    style.resolve_root_font_metric_lengths(root_metrics);
    PageBoxEdges {
        border: used_border_widths(&style),
        padding: css::page_padding_from_for_size_with_ch_advance_and_root_metrics(
            declarations,
            page_size,
            ch_advance,
            root_metrics,
        ),
    }
}

/// Resolves the page background positioning area selected by `background-origin`.
///
/// For a page box, the border box starts inside the page margins, the padding
/// box is inset by page borders, and the content box is additionally inset by
/// page padding:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
#[allow(dead_code)]
pub(in crate::layout) fn page_background_positioning_area(
    declarations: &Declarations,
    base_margins: PageMargins,
    page_size: PageSize,
    origin: css::BackgroundBox,
    ch_advance: LayoutLength,
) -> PaintRect {
    let style = ComputedStyle::initial();
    page_background_positioning_area_with_root_metrics(
        declarations,
        page_size,
        base_margins,
        origin,
        ch_advance,
        css::RootFontMetricLengthBasis {
            font_size: layout_pt(style.font_size),
            ch_advance,
            x_height: layout_pt(style.font_size * 0.5),
            cap_height: layout_pt(style.font_size * 0.7),
            ic_advance: ch_advance,
            line_height: layout_pt(style.line_height),
        },
    )
}

pub(in crate::layout) fn page_background_positioning_area_with_root_metrics(
    declarations: &Declarations,
    page_size: PageSize,
    base_margins: PageMargins,
    origin: css::BackgroundBox,
    ch_advance: LayoutLength,
    root_metrics: css::RootFontMetricLengthBasis,
) -> PaintRect {
    let edges = page_box_edges_from_declarations_with_ch_advance_and_root_metrics(
        declarations,
        page_size,
        ch_advance,
        root_metrics,
    );
    let mut page_style = ComputedStyle::initial();
    css::apply_declarations(&mut page_style, declarations);
    let margins =
        css::page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style_and_root_metrics(
            declarations,
            base_margins,
            page_size,
            css::PageMarginResolutionContext {
                viewport_size: page_size,
                non_margin_edges: edges.total(),
                ch_advance,
                style: &page_style,
                root_metrics,
            },
        );
    let border_box = paint_space_rect(
        margins.left(),
        margins.bottom(),
        (page_size.width() - margins.left() - margins.right()).max(0.0),
        (page_size.height() - margins.top() - margins.bottom()).max(0.0),
    );

    match origin {
        css::BackgroundBox::Border | css::BackgroundBox::BorderArea => border_box,
        css::BackgroundBox::Padding => inset_paint_rect(border_box, edges.border),
        css::BackgroundBox::Content => {
            inset_paint_rect(inset_paint_rect(border_box, edges.border), edges.padding)
        }
    }
}

pub(in crate::layout) fn page_background_layers_for_paint(
    style: &ComputedStyle,
) -> Vec<css::BackgroundLayer> {
    if !style.background.background_layers.is_empty() {
        return style.background.background_layers.clone();
    }
    vec![css::BackgroundLayer {
        image: style.background.background_image.clone(),
        position: style.background.background_position.clone(),
        size: style.background.background_size.clone(),
        repeat: style.background.background_repeat,
        attachment: style.background.background_attachment,
        origin: style.background.background_origin,
        clip: style.background.background_clip,
    }]
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn add_page_backgrounds(&mut self) {
        if self.pages.is_empty() {
            return;
        }
        for page_index in 0..self.pages.len() {
            let page_number = page_index + 1;
            let declarations = self.page_declarations_for(page_number);
            let page_width = self.pages[page_index].width();
            let page_height = self.pages[page_index].height();
            let page_size = PageSize::from_points(page_width, page_height);
            let root_metrics = self.root_metric_state.resolved().basis();
            if !declarations.is_empty() {
                let mut style = ComputedStyle::initial();
                css::apply_declarations(&mut style, &declarations);
                let page_ch_advance =
                    self.ch_advance_for_style(&style, style.requires_ch_advance());
                style.resolve_font_metric_lengths(page_ch_advance);
                style.root_font_size = root_metrics.font_size.points();
                style.resolve_root_font_metric_lengths(root_metrics);
                if style.visibility != Visibility::Visible {
                    // `visibility` applies to the page context's own
                    // background, border, and generated margin boxes, but it
                    // is not inherited by document content. A propagated
                    // document-canvas background therefore remains eligible
                    // to paint behind that content.
                    // <https://www.w3.org/TR/css-page-3/#page-properties>
                    self.add_document_canvas_background(page_index, page_size);
                    continue;
                }
                let page_margins = PageContext::from_options(self.options).margins;
                let mut background_primitives = Vec::new();
                let page_border_area = page_background_positioning_area_with_root_metrics(
                    &declarations,
                    page_size,
                    page_margins,
                    css::BackgroundBox::Border,
                    page_ch_advance,
                    root_metrics,
                );
                for layer in page_background_layers_for_paint(&style).iter().rev() {
                    let mut layer_style = style.clone();
                    layer_style.background.background_image = layer.image.clone();
                    layer_style.background.background_size = layer.size.clone();
                    layer_style.background.background_position = layer.position.clone();
                    layer_style.background.background_repeat = layer.repeat;
                    layer_style.background.background_origin = css::BackgroundBox::Border;
                    layer_style.background.background_clip = css::BackgroundBox::Border;
                    let mut paint_layer = layer.clone();
                    paint_layer.origin = css::BackgroundBox::Border;
                    paint_layer.clip = css::BackgroundBox::Border;
                    layer_style.background.background_layers = vec![paint_layer];
                    layer_style.background.background_image_layer_count = 1;
                    // Page-box geometry above already selected the authored
                    // origin and clip boxes.  The generic background painter
                    // must therefore receive a neutral border model, or it
                    // would inset those selected areas a second time (and
                    // discard images under an opaque page border).
                    layer_style.border_widths = css::Edges::ZERO;
                    layer_style.border_width_values =
                        css::PhysicalEdges::all(css::ComputedLengthPercentage::ZERO);
                    layer_style.border_styles = css::BorderStyles::NONE;
                    layer_style.border_width = 0.0;
                    let image_area = page_background_positioning_area_with_root_metrics(
                        &declarations,
                        page_size,
                        page_margins,
                        layer.origin,
                        page_ch_advance,
                        root_metrics,
                    );
                    let clip_area = page_background_positioning_area_with_root_metrics(
                        &declarations,
                        page_size,
                        page_margins,
                        layer.clip,
                        page_ch_advance,
                        root_metrics,
                    );
                    background_primitives.extend(
                        background_image_primitives_for_style_with_paint_areas(
                            PaintBackgroundArea::from_paint_rect(image_area),
                            PaintBackgroundArea::from_paint_rect(clip_area),
                            &layer_style,
                            self.base_url,
                            self.root_url,
                            self.resource_cache,
                        ),
                    );
                }
                let outline_primitives = self.box_outline_primitives(page_border_area, &style);
                let page = &mut self.pages[page_index];

                let mut background_style = style.clone();
                background_style.background.background_clip = css::BackgroundBox::Border;
                for layer in &mut background_style.background.background_layers {
                    layer.clip = css::BackgroundBox::Border;
                }
                background_style.border_widths = css::Edges::ZERO;
                background_style.border_width_values =
                    css::PhysicalEdges::all(css::ComputedLengthPercentage::ZERO);
                background_style.border_styles = css::BorderStyles::NONE;
                background_style.border_width = 0.0;
                let (rects, rounded_rects, paths, strokes) = block_paint_ops(
                    paint_space_rect(0.0, 0.0, page_width, page_height),
                    &background_style,
                );
                for rect in rects {
                    page.push_rect_in_band(PaintBand::PageBackground, rect);
                }
                for rounded_rect in rounded_rects {
                    page.push_rounded_rect_in_band(PaintBand::PageBackground, rounded_rect);
                }
                for path in paths {
                    page.push_path_in_band(PaintBand::PageBackground, path);
                }
                for stroke in strokes {
                    page.push_stroke_in_band(PaintBand::PageBackground, stroke);
                }
                for primitive in background_primitives {
                    match primitive {
                        PaintPrimitive::Rect(rect) => {
                            page.push_rect_in_band(PaintBand::PageBackground, rect);
                        }
                        PaintPrimitive::RoundedRect(rect) => {
                            page.push_rounded_rect_in_band(PaintBand::PageBackground, rect);
                        }
                        PaintPrimitive::Path(path) => {
                            page.push_path_in_band(PaintBand::PageBackground, path);
                        }
                        PaintPrimitive::Stroke(stroke) => {
                            page.push_stroke_in_band(PaintBand::PageBackground, stroke);
                        }
                        PaintPrimitive::Image(image) => {
                            page.push_image_in_band(PaintBand::PageBackground, image);
                        }
                        PaintPrimitive::ImagePattern(pattern) => {
                            page.push_image_pattern_in_band(PaintBand::PageBackground, pattern);
                        }
                        PaintPrimitive::GradientPattern(pattern) => {
                            page.push_gradient_pattern_in_band(PaintBand::PageBackground, pattern);
                        }
                        PaintPrimitive::SvgPattern(pattern) => {
                            page.push_svg_pattern_in_band(PaintBand::PageBackground, pattern);
                        }
                        PaintPrimitive::Line(line) => {
                            page.push_line_in_band(PaintBand::PageBackground, line);
                        }
                        PaintPrimitive::OpaqueTextCoverage { line, paths } => {
                            page.push_opaque_text_coverage_in_band(
                                PaintBand::PageBackground,
                                line,
                                paths,
                            );
                        }
                        PaintPrimitive::SvgTextOutline {
                            content,
                            actual_text,
                        } => {
                            page.push_svg_text_outline_scope_in_band(
                                PaintBand::PageBackground,
                                *content,
                                actual_text,
                            );
                        }
                        PaintPrimitive::ProjectiveRaster(_) => {
                            unreachable!("projective raster lowering happens in the PDF backend")
                        }
                    }
                }

                let mut border_style = style;
                border_style.background.background_color = css::BackgroundColor::TRANSPARENT;
                border_style.background.background_image = css::ComputedImage::None;
                border_style.background.background_layers.clear();
                let (rects, rounded_rects, paths, strokes) =
                    block_paint_ops(page_border_area, &border_style);
                for rect in rects {
                    page.push_rect_in_band(PaintBand::PageBackground, rect);
                }
                for rounded_rect in rounded_rects {
                    page.push_rounded_rect_in_band(PaintBand::PageBackground, rounded_rect);
                }
                for path in paths {
                    page.push_path_in_band(PaintBand::PageBackground, path);
                }
                for stroke in strokes {
                    page.push_stroke_in_band(PaintBand::PageBackground, stroke);
                }
                for primitive in outline_primitives {
                    match primitive {
                        PaintPrimitive::Rect(rect) => {
                            page.push_rect_in_band(PaintBand::Outline, rect);
                        }
                        PaintPrimitive::RoundedRect(rect) => {
                            page.push_rounded_rect_in_band(PaintBand::Outline, rect);
                        }
                        PaintPrimitive::Path(path) => {
                            page.push_path_in_band(PaintBand::Outline, path);
                        }
                        PaintPrimitive::Stroke(stroke) => {
                            page.push_stroke_in_band(PaintBand::Outline, stroke);
                        }
                        PaintPrimitive::Image(_)
                        | PaintPrimitive::ImagePattern(_)
                        | PaintPrimitive::GradientPattern(_)
                        | PaintPrimitive::SvgPattern(_)
                        | PaintPrimitive::ProjectiveRaster(_)
                        | PaintPrimitive::Line(_)
                        | PaintPrimitive::OpaqueTextCoverage { .. }
                        | PaintPrimitive::SvgTextOutline { .. } => {}
                    }
                }
            }
            self.add_document_canvas_background(page_index, page_size);
        }
    }
}
