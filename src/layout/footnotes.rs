use super::assets::background_image_primitives_for_style;
use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn install_footnotes(&mut self, page_box: &box_tree::PageBox<'a>) {
        self.footnote_bodies = page_box
            .footnotes
            .iter()
            .cloned()
            .map(|footnote| (footnote.element.id, footnote))
            .collect();
    }

    /// Provides an initial fixed-point reservation for a document with one
    /// detached body. The first page is only a seed: render validates the
    /// committed call assignment and retries from the document boundary if it
    /// belongs on a different page.
    pub(in crate::layout) fn initial_single_footnote_measurement(
        &mut self,
        page_box: &box_tree::PageBox<'a>,
    ) -> Option<FootnoteMeasurement> {
        let [footnote] = page_box.footnotes.as_slice() else {
            return None;
        };
        let area = self.footnote_area_geometry(0);
        Some(FootnoteMeasurement {
            element: footnote.element.id,
            page_index: 0,
            area_vertical_non_content: area.vertical_non_content,
            height: self.measure_footnote_height(footnote, area.content_inline_span),
        })
    }

    /// Assign a footnote to the fragmentainer that committed its call line.
    ///
    /// CSS GCPM footnote calls participate in inline layout, so collection and
    /// intrinsic sizing can encounter them before line fragmentation decides
    /// their page. Callers must therefore invoke this only from committed-line
    /// layout, not while building an inline item stream.
    /// <https://www.w3.org/TR/css-gcpm-3/#footnote-calls>
    pub(in crate::layout) fn handle_footnote_call(&mut self, element: ElementId) {
        if self.footnote_measurement_depth > 0 {
            return;
        }
        let Some(footnote) = self.footnote_bodies.get(&element).cloned() else {
            return;
        };
        match self.footnote_layout_mode {
            FootnoteLayoutMode::Measure => {
                if !self.measured_footnotes.insert(element) {
                    return;
                }
                let area = self.footnote_area_geometry(self.pages.len());
                let height = self.measure_footnote_height(&footnote, area.content_inline_span);
                self.footnote_measurements.push(FootnoteMeasurement {
                    element,
                    page_index: self.pages.len(),
                    area_vertical_non_content: area.vertical_non_content,
                    height,
                });
            }
            FootnoteLayoutMode::Render => {
                if self.rendered_footnotes.insert(element) {
                    let area = self.footnote_area_geometry(self.pages.len());
                    // A fixed-point render starts with measurements for its
                    // reserved page-local areas. Reuse that exact body
                    // measurement when the committed call remained on the
                    // same page; a changed assignment still measures against
                    // its destination page and drives another iteration.
                    let height = self
                        .footnote_measurements
                        .iter()
                        .find(|measurement| {
                            measurement.element == element
                                && measurement.page_index == self.pages.len()
                                && measurement.area_vertical_non_content
                                    == area.vertical_non_content
                        })
                        .map(|measurement| measurement.height)
                        .unwrap_or_else(|| {
                            self.measure_footnote_height(&footnote, area.content_inline_span)
                        });
                    self.rendered_footnote_measurements
                        .push(FootnoteMeasurement {
                            element,
                            page_index: self.pages.len(),
                            area_vertical_non_content: area.vertical_non_content,
                            height,
                        });
                    self.pending_page_footnotes.push(element);
                }
            }
        }
    }

    pub(in crate::layout) fn flush_current_page_footnotes(&mut self) {
        let footnotes = std::mem::take(&mut self.pending_page_footnotes);
        for element in footnotes {
            if let Some(footnote) = self.footnote_bodies.get(&element).cloned() {
                self.render_footnote(&footnote);
            }
        }
    }

    fn measure_footnote_height(
        &mut self,
        footnote: &box_tree::FootnoteBox<'a>,
        content_inline_span: PageInlineSpan,
    ) -> f32 {
        let snapshot = self.snapshot();
        let start = self.page_top();
        self.content_left = content_inline_span.left_x();
        self.content_right = content_inline_span.right_x();
        self.cursor_y = start;
        self.footnote_measurement_depth += 1;
        self.fragmentation_suppression_depth += 1;
        let stylesheets = self.stylesheets;
        self.layout_formatting_box(&footnote.body, &stylesheets);
        let height = (start - self.cursor_y).max(0.0);
        self.footnote_measurement_depth -= 1;
        self.fragmentation_suppression_depth -= 1;
        self.restore(snapshot);
        height
    }

    fn render_footnote(&mut self, footnote: &box_tree::FootnoteBox<'a>) {
        let page_index = self.pages.len();
        let Some(total) = self.footnote_reservations.get(&page_index).copied() else {
            return;
        };
        let area = self.footnote_area_geometry(page_index);
        let (has_preceding, previous) = self
            .footnote_measurements
            .iter()
            .take_while(|entry| entry.element != footnote.element.id)
            .filter(|entry| entry.page_index == page_index)
            .fold((false, 0.0), |(_, height), entry| {
                (true, height + entry.height)
            });
        // GCPM creates one footnote area per page. Paint it before the first
        // body only; later bodies continue in that area's content box.
        if !has_preceding {
            self.paint_footnote_area(&area, total);
        }
        let saved_cursor = self.cursor_y;
        let saved_baseline = self.last_in_flow_line_baseline_y;
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        self.content_left = area.content_inline_span.left_x();
        self.content_right = area.content_inline_span.right_x();
        self.cursor_y = self.current_page_context.bottom() + total
            - previous
            - area.style.margin.top
            - area.metrics.border.top.points()
            - area.metrics.padding.top.points();
        self.footnote_measurement_depth += 1;
        if let Some(marker_style) = footnote.body.style().footnote_marker_style.as_deref() {
            let marker = self.evaluate_generated_pseudo_text_rollback(
                footnote.element,
                box_tree::CounterEventSource::FootnoteMarker,
                Some(marker_style),
            );
            if !marker.is_empty() {
                self.layout_text_block(&marker, marker_style, 0.0, 0.0, None);
            }
        }
        let stylesheets = self.stylesheets;
        self.layout_formatting_box(&footnote.body, &stylesheets);
        self.footnote_measurement_depth -= 1;
        self.cursor_y = saved_cursor;
        self.last_in_flow_line_baseline_y = saved_baseline;
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
    }

    /// Resolves the one page-local footnote area as a normal block-sized box.
    ///
    /// The footnote area's bottom margin edge is anchored to the page area's
    /// bottom edge. Its own margins therefore reduce its border-box width and
    /// contribute to the vertical reservation, while its padding and borders
    /// surround all bodies collectively rather than each body independently:
    /// <https://www.w3.org/TR/css-gcpm-3/#footnote-area> and
    /// <https://www.w3.org/TR/css-gcpm-3/#footnote-area-positioning>.
    fn footnote_area_geometry(&self, page_index: usize) -> FootnoteAreaGeometry {
        let declarations = page_margin::page_footnote_area_declarations_for_rules(
            &self.page_rules,
            page_index + 1,
            self.current_page_name.as_deref(),
            false,
            self.page_progression_direction,
        );
        let mut style = self.footnote_area_style(page_index, &declarations);
        let containing_inline_span =
            PageInlineSpan::from_edges(self.content_left, self.content_right);
        let metrics = apply_used_box_metrics(
            &mut style,
            PercentageBasis::definite(layout_pt(containing_inline_span.width())),
        );
        let horizontal_non_content = metrics.horizontal_non_content_length();
        let content_width = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(containing_inline_span.width()),
            horizontal_non_content,
        );
        let inline_geometry = resolve_normal_flow_block_inline_geometry(
            &mut style,
            containing_inline_span,
            PhysicalContentWidth::new(content_width),
            horizontal_non_content,
            self.containing_block_direction,
            true,
        );
        let border_box_inline_span = inline_geometry.border_box_inline_span;
        let content_inline_span = PageInlineSpan::from_edges(
            border_box_inline_span.left_x()
                + metrics.border.left.points()
                + metrics.padding.left.points(),
            border_box_inline_span.right_x()
                - metrics.border.right.points()
                - metrics.padding.right.points(),
        );
        let vertical_non_content =
            style.margin.top + metrics.vertical_non_content_length().points() + style.margin.bottom;
        FootnoteAreaGeometry {
            style,
            metrics,
            border_box_inline_span,
            content_inline_span,
            vertical_non_content,
        }
    }

    fn footnote_area_style(
        &self,
        page_index: usize,
        declarations: &css::Declarations,
    ) -> ComputedStyle {
        let page_style =
            self.page_context_style_for_declarations(&self.page_declarations_for_page(
                page_index + 1,
                self.current_page_name.as_deref(),
                false,
            ));
        let mut style = page_margin::page_margin_style_inheriting_page_context(&page_style);
        css::apply_declarations(&mut style, declarations);
        style
    }

    fn paint_footnote_area(&mut self, geometry: &FootnoteAreaGeometry, margin_box_height: f32) {
        let style = &geometry.style;
        if style.visibility != Visibility::Visible || margin_box_height <= 0.0 {
            return;
        }
        let border_box_height =
            (margin_box_height - style.margin.top - style.margin.bottom).max(0.0);
        let rect = PageTopRect::new(
            geometry.border_box_inline_span.left_x(),
            self.current_page_context.bottom() + margin_box_height - style.margin.top,
            geometry.border_box_inline_span.width(),
            border_box_height,
        )
        .paint_rect();
        let (rects, rounded_rects, paths, strokes) = block_paint_ops(rect, style);
        for rect in rects {
            self.current_page
                .push_rect_in_band(PaintBand::BackgroundBorder, rect);
        }
        for rect in rounded_rects {
            self.current_page
                .push_rounded_rect_in_band(PaintBand::BackgroundBorder, rect);
        }
        for path in paths {
            self.current_page
                .push_path_in_band(PaintBand::BackgroundBorder, path);
        }
        for stroke in strokes {
            self.current_page
                .push_stroke_in_band(PaintBand::BackgroundBorder, stroke);
        }
        for primitive in background_image_primitives_for_style(
            PaintBackgroundArea::from_paint_rect(rect),
            style,
            self.base_url,
            self.root_url,
            self.resource_cache,
        ) {
            page_margin::push_page_margin_primitive(
                &mut self.current_page,
                PaintBand::BackgroundBorder,
                primitive,
            );
        }
    }

    pub(in crate::layout) fn footnote_reservations_from_measurements(
        measurements: &[FootnoteMeasurement],
    ) -> HashMap<usize, f32> {
        let mut reservations = HashMap::new();
        for measurement in measurements {
            match reservations.entry(measurement.page_index) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(measurement.area_vertical_non_content + measurement.height);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() += measurement.height;
                }
            }
        }
        reservations
    }
}

/// Used geometry for the one GCPM footnote area associated with a page.
///
/// Keeping the area as one composite prevents individual footnote bodies from
/// accidentally treating page-area margins, padding, or borders as their own
/// box-model edges.
struct FootnoteAreaGeometry {
    style: ComputedStyle,
    metrics: UsedBoxMetrics,
    border_box_inline_span: PageInlineSpan,
    content_inline_span: PageInlineSpan,
    vertical_non_content: f32,
}
