use super::*;
use crate::layout::assets::fixed_background_page_margin_box;

/// Propagated root/body background state used to paint the document canvas.
///
/// CSS Backgrounds propagates the root element background to the canvas, or
/// the first body background when the root has no background. The canvas paint
/// area is page-dependent in paged media, but image sizing and positioning stay
/// anchored to the root background positioning area:
/// <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum DocumentCanvasBackgroundSource {
    /// The HTML root supplied the propagated canvas background.
    Root,
    /// The eligible first `body` supplied the fallback canvas background.
    EligibleBodyFallback,
}

/// The selected propagated canvas background and its CSS-defined source.
///
/// The source is significant: a root background prevents the eligible body's
/// background from propagating, while a body fallback has initial used
/// background values on the body itself.
/// <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct DocumentCanvasBackground {
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) source: DocumentCanvasBackgroundSource,
}

pub(in crate::layout) fn canvas_background_style(style: &ComputedStyle) -> ComputedStyle {
    let mut style = style.clone();
    style.border_widths = css::Edges::ZERO;
    style.border_width_values = css::PhysicalEdges::all(css::ComputedLengthPercentage::ZERO);
    style.border_styles = css::BorderStyles::NONE;
    style.border_width = 0.0;
    style.border_image = css::BorderImage::initial();
    style
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn add_document_canvas_background(
        &mut self,
        page_index: usize,
        page_size: PageSize,
    ) {
        let Some(background) = self.document_canvas_background.clone() else {
            return;
        };
        let style = &background.style;
        // The propagated root/body background paints the document canvas,
        // which is the page area. Page-margin boxes occupy the surrounding
        // page margin; a negative margin-box stacking context is therefore
        // exposed there but is covered when it overlaps the document canvas.
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds> and
        // <https://www.w3.org/TR/css-page-3/#page-area>
        let context = self.finished_page_context(page_index + 1, page_size);
        // In paged media the propagated document canvas is the page area.
        // Page margins remain outside the root/body background regardless of
        // whether the page box itself has an authored background or border.
        // The page box's own paint is emitted separately above.
        // https://www.w3.org/TR/css-page-3/#page-area
        // https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds
        let (x, y, width, height) = (
            context.left(),
            context.bottom(),
            context.area_width(),
            context.area_height(),
        );
        let page_document_bottom = self.document_canvas_page_bottom(page_index);
        let clip_area =
            DocumentCanvasBackgroundArea::from_document_canvas_rect(DocumentCanvasRect::new(
                DocumentCanvasPoint::new(x, page_document_bottom + y),
                DocumentCanvasSize::new(width, height),
            ));
        let positioning_area = self.document_canvas_root_positioning_area();
        let fixed_positioning_area = fixed_background_page_margin_box(
            DocumentCanvasPoint::new(0.0, page_document_bottom),
            page_size,
        );
        let background_primitives =
            background_image_primitives_for_style_with_paint_areas_and_fixed_positioning_area(
                positioning_area.project_to_paint(page_document_bottom),
                clip_area.project_to_paint(page_document_bottom),
                Some(fixed_positioning_area.project_to_paint(page_document_bottom)),
                false,
                style,
                self.base_url,
                self.root_url,
                self.resource_cache,
            );
        let canvas_scroll_translation = self.document_canvas_scroll_translation;
        let page = &mut self.pages[page_index];
        let canvas_checkpoint = (canvas_scroll_translation.x != 0.0
            || canvas_scroll_translation.y != 0.0)
            .then(|| page.paint_checkpoint());
        // Root/background propagation paints the solid color layer over the
        // page canvas. Image positioning may remain relative to html's used
        // box, but that box's padding must not clip the canvas color layer.
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
        if let Some(fill) = style.background.background_color.visible_color(style.color) {
            page.push_document_canvas_rect(RenderedRect::new(
                x,
                y,
                width,
                height,
                Some(fill),
                None,
                PaintStrokeWidth::ZERO,
            ));
        }
        let mut non_color_style = style.clone();
        non_color_style.background.background_color = css::BackgroundColor::TRANSPARENT;
        // Image layers above are projected from the root positioning area by
        // `background_image_primitives…`. Re-running the generic box painter
        // with those layers would replay a propagated gradient/image in the
        // page-local coordinate system (and double-composite translucent
        // layers).
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
        non_color_style.background.background_image = css::ComputedImage::None;
        non_color_style.background.background_layers.clear();
        let (rects, rounded_rects, paths, strokes) =
            block_paint_ops(paint_space_rect(x, y, width, height), &non_color_style);
        for rect in rects {
            page.push_document_canvas_rect(rect);
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
                    page.push_document_canvas_rect(rect);
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
                    page.push_opaque_text_coverage_in_band(PaintBand::PageBackground, line, paths);
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
        if let Some(checkpoint) = canvas_checkpoint {
            page.translate_recorded_primitives_since(&checkpoint, canvas_scroll_translation);
        }
    }

    pub(in crate::layout) fn selected_document_canvas_background_source(
        &self,
    ) -> Option<DocumentCanvasBackgroundSource> {
        self.document_canvas_background
            .as_ref()
            .map(|background| background.source)
    }

    /// Whether this element supplies the document canvas' propagated
    /// background. The selected canvas background is always painted outside
    /// element effect contexts, including root transforms.
    /// <https://drafts.csswg.org/css-backgrounds-3/#special-backgrounds>
    pub(in crate::layout) fn element_paints_document_canvas_background(
        &self,
        element: &Element,
    ) -> bool {
        match self.selected_document_canvas_background_source() {
            Some(DocumentCanvasBackgroundSource::Root) => self
                .document_canvas_overflow
                .is_root_canvas_background_source(element),
            Some(DocumentCanvasBackgroundSource::EligibleBodyFallback) => self
                .document_canvas_overflow
                .is_body_canvas_background_fallback_source(element),
            None => false,
        }
    }

    fn document_canvas_total_height(&self) -> f32 {
        self.pages.iter().map(Page::height).sum()
    }

    fn document_canvas_page_bottom(&self, page_index: usize) -> f32 {
        let total_height = self.document_canvas_total_height();
        let height_through_page: f32 = self
            .pages
            .iter()
            .take(page_index + 1)
            .map(Page::height)
            .sum();
        total_height - height_through_page
    }

    fn document_canvas_root_positioning_area(&self) -> DocumentCanvasBackgroundArea {
        let total_height = self.document_canvas_total_height();
        let first_page_bottom = self
            .pages
            .first()
            .map(|page| total_height - page.height())
            .unwrap_or(0.0);
        self.document_canvas_root_positioning_area
            .map(|area| {
                let mapped_y = first_page_bottom + area.y();
                // The root/background propagation rule expands the painting
                // area to the canvas, but leaves image sizing and positioning
                // relative to the root element's own box. In particular,
                // `background-size: auto` for a generated image must not turn
                // an otherwise 300px root into a document-height image.
                // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
                let mapped_height = area.height();
                let mapped_top = mapped_y + area.height();
                DocumentCanvasBackgroundArea::new(
                    DocumentCanvasPoint::new(area.x(), mapped_top - mapped_height),
                    DocumentCanvasSize::new(area.width(), mapped_height),
                )
            })
            .unwrap_or_else(|| {
                DocumentCanvasBackgroundArea::new(
                    DocumentCanvasPoint::new(0.0, 0.0),
                    DocumentCanvasSize::new(
                        self.pages.first().map(Page::width).unwrap_or(0.0),
                        total_height,
                    ),
                )
            })
    }

    pub(in crate::layout) fn capture_document_canvas_background(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) {
        if self.element_side_effect_suppression_depth > 0 {
            return;
        }
        if !self.element_propagates_document_canvas_properties(element, style) {
            return;
        }
        let has_background = style
            .background
            .background_color
            .visible_color(style.color)
            .is_some()
            || style.background.background_image.is_image()
            || style
                .background
                .background_layers
                .iter()
                .any(|layer| layer.image.is_image());
        if self
            .document_canvas_overflow
            .is_root_canvas_background_source(element)
        {
            if has_background {
                self.document_canvas_background = Some(DocumentCanvasBackground {
                    style: canvas_background_style(style),
                    source: DocumentCanvasBackgroundSource::Root,
                });
            }
        } else if self
            .document_canvas_overflow
            .is_body_canvas_background_fallback_source(element)
            && self.selected_document_canvas_background_source().is_none()
            && has_background
        {
            let mut canvas_style = canvas_background_style(style);
            // `forced-color-adjust` on the body affects its own box, but not
            // the document canvas. The root is the canvas's adjustment
            // subject, so a body opting out cannot carry its authored
            // background into an otherwise auto-adjusted canvas.
            // <https://www.w3.org/TR/css-color-adjust-1/#forced-colors-mode>
            if style.forced_color_adjust == css::ForcedColorAdjust::None
                && let Some(palette) = self.options.forced_colors.palette()
            {
                canvas_style.background.background_color =
                    css::BackgroundColor::Color(palette.canvas);
                canvas_style.background.background_image = css::ComputedImage::None;
                canvas_style.background.background_layers.clear();
            }
            self.document_canvas_background = Some(DocumentCanvasBackground {
                style: canvas_style,
                source: DocumentCanvasBackgroundSource::EligibleBodyFallback,
            });
        }
    }

    pub(in crate::layout) fn record_document_canvas_root_positioning_area(
        &mut self,
        area: PaintBackgroundArea,
    ) {
        if self.element_side_effect_suppression_depth > 0 {
            return;
        }
        // An embedded document's internal layout surface may remain taller
        // than its finite browsing-context viewport. A zero-height root box
        // in that surface still positions its propagated background at the
        // viewport's block-start edge. Rebase that zero-height root to the
        // child page's local viewport: its internal surface coordinate is not
        // a paint coordinate in the replaced element. Using its literal zero
        // height would also resolve `background-size: 100% 100%` to an empty
        // image.
        // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
        let area = self
            .iframe_viewport
            .filter(|_| area.height() <= 0.01)
            .map(|context| {
                let viewport = context.viewport;
                PaintBackgroundArea::new(
                    PaintPoint::new(area.x(), area.y() - viewport.height()),
                    PaintSize::new(area.width(), viewport.height()),
                )
            })
            .unwrap_or(area);
        self.document_canvas_overflow.record_auto_overflow(
            area.width(),
            area.height(),
            self.current_page_context.area_width(),
            self.current_page_context.area_height(),
        );
        // An eligible body background is treated as if it were specified on
        // the root, so both propagation sources size and position images in
        // this root area while painting the page canvas.
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
        self.document_canvas_root_positioning_area = Some(area);
    }
}
