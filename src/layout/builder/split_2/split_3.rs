use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn snapshot(&self) -> LayoutSnapshot {
        LayoutSnapshot {
            pages: self.pages.clone(),
            page_names: self.page_names.clone(),
            page_blanks: self.page_blanks.clone(),
            page_name_scope_suppression: self.page_name_scope_suppression,
            page_name_element_scope_suppression: self.page_name_element_scope_suppression,
            page_named_strings: self.page_named_strings.clone(),
            page_running_elements: self.page_running_elements.clone(),
            page_anchors: self.page_anchors.clone(),
            page_anchor_text: self.page_anchor_text.clone(),
            document_canvas_background: self.document_canvas_background.clone(),
            root_canvas_background_defined: self.root_canvas_background_defined,
            current_page: self.current_page.clone(),
            current_page_has_flow_content: self.current_page_has_flow_content,
            last_block_layout_outcome: self.last_block_layout_outcome,
            current_page_name: self.current_page_name.clone(),
            current_page_context: self.current_page_context,
            cursor_y: self.cursor_y,
            content_left: self.content_left,
            content_right: self.content_right,
            content_logical_inline_size_stack: self.content_logical_inline_size_stack.clone(),
            inline_static_position: self.inline_static_position,
            text_box_line_trim_stack: self.text_box_line_trim_stack.clone(),
            last_in_flow_line_baseline_y: self.last_in_flow_line_baseline_y,
            block_static_position_y_offset: self.block_static_position_y_offset,
            absolute_static_position: self.absolute_static_position,
            escaped_atom_positioning_depth: self.escaped_atom_positioning_depth,
            escaped_atom_containing_block: self.escaped_atom_containing_block,
            containing_block_writing_mode: self.containing_block_writing_mode,
            fragment_top_offsets: self.fragment_top_offsets.clone(),
            child_available_space_stack: self.child_available_space_stack.clone(),
            definite_block_size_stack: self.definite_block_size_stack.clone(),
            truncate_page_start_margins: self.truncate_page_start_margins,
            avoid_inside_retry_depth: self.avoid_inside_retry_depth,
            out_of_flow_prebreak_suppression_depth: self.out_of_flow_prebreak_suppression_depth,
            element_side_effect_suppression_depth: self.element_side_effect_suppression_depth,
            containing_blocks: self.containing_blocks.clone(),
            list_stack: self.list_stack.clone(),
            counter_set: self.counter_set.clone(),
            quote_depth: self.quote_depth,
            current_page_named_strings: self.current_page_named_strings.clone(),
            current_page_running_elements: self.current_page_running_elements.clone(),
            next_assignment_id: self.next_assignment_id,
            assignment_capture_stack: self.assignment_capture_stack.clone(),
            ancestors: self.ancestors.clone(),
            bookmarks: self.bookmarks.clone(),
            positioned_layers: self.positioned_layers.clone(),
            fixed_layers: self.fixed_layers.clone(),
            next_paint_source_order: self.next_paint_source_order,
            next_float_id: self.next_float_id,
            float_contexts: self.float_contexts.clone(),
            adjoining_float_origin_y: self.adjoining_float_origin_y,
            pending_float_fragments: self.pending_float_fragments.clone(),
            pending_float_side_effects: self.pending_float_side_effects.clone(),
            applied_clearance_count: self.applied_clearance_count,
            preserve_scoped_paint_public_order: self.preserve_scoped_paint_public_order,
            defer_next_block_decoration_promotion: self.defer_next_block_decoration_promotion,
        }
    }

    pub(in crate::layout) fn restore(&mut self, snapshot: LayoutSnapshot) {
        self.pages = snapshot.pages;
        self.page_names = snapshot.page_names;
        self.page_blanks = snapshot.page_blanks;
        self.page_name_scope_suppression = snapshot.page_name_scope_suppression;
        self.page_name_element_scope_suppression = snapshot.page_name_element_scope_suppression;
        self.page_named_strings = snapshot.page_named_strings;
        self.page_running_elements = snapshot.page_running_elements;
        self.page_anchors = snapshot.page_anchors;
        self.page_anchor_text = snapshot.page_anchor_text;
        self.document_canvas_background = snapshot.document_canvas_background;
        self.root_canvas_background_defined = snapshot.root_canvas_background_defined;
        self.current_page = snapshot.current_page;
        self.current_page_has_flow_content = snapshot.current_page_has_flow_content;
        self.last_block_layout_outcome = snapshot.last_block_layout_outcome;
        self.current_page_name = snapshot.current_page_name;
        self.current_page_context = snapshot.current_page_context;
        self.cursor_y = snapshot.cursor_y;
        self.content_left = snapshot.content_left;
        self.content_right = snapshot.content_right;
        self.content_logical_inline_size_stack = snapshot.content_logical_inline_size_stack;
        self.inline_static_position = snapshot.inline_static_position;
        self.text_box_line_trim_stack = snapshot.text_box_line_trim_stack;
        self.last_in_flow_line_baseline_y = snapshot.last_in_flow_line_baseline_y;
        self.block_static_position_y_offset = snapshot.block_static_position_y_offset;
        self.absolute_static_position = snapshot.absolute_static_position;
        self.escaped_atom_positioning_depth = snapshot.escaped_atom_positioning_depth;
        self.escaped_atom_containing_block = snapshot.escaped_atom_containing_block;
        self.containing_block_writing_mode = snapshot.containing_block_writing_mode;
        self.fragment_top_offsets = snapshot.fragment_top_offsets;
        self.child_available_space_stack = snapshot.child_available_space_stack;
        self.definite_block_size_stack = snapshot.definite_block_size_stack;
        self.truncate_page_start_margins = snapshot.truncate_page_start_margins;
        self.avoid_inside_retry_depth = snapshot.avoid_inside_retry_depth;
        self.out_of_flow_prebreak_suppression_depth =
            snapshot.out_of_flow_prebreak_suppression_depth;
        self.element_side_effect_suppression_depth = snapshot.element_side_effect_suppression_depth;
        self.containing_blocks = snapshot.containing_blocks;
        self.list_stack = snapshot.list_stack;
        self.counter_set = snapshot.counter_set;
        self.quote_depth = snapshot.quote_depth;
        self.current_page_named_strings = snapshot.current_page_named_strings;
        self.current_page_running_elements = snapshot.current_page_running_elements;
        self.next_assignment_id = snapshot.next_assignment_id;
        self.assignment_capture_stack = snapshot.assignment_capture_stack;
        self.ancestors = snapshot.ancestors;
        self.bookmarks = snapshot.bookmarks;
        self.positioned_layers = snapshot.positioned_layers;
        self.fixed_layers = snapshot.fixed_layers;
        self.next_paint_source_order = snapshot.next_paint_source_order;
        self.next_float_id = snapshot.next_float_id;
        self.float_contexts = snapshot.float_contexts;
        self.adjoining_float_origin_y = snapshot.adjoining_float_origin_y;
        self.pending_float_fragments = snapshot.pending_float_fragments;
        self.pending_float_side_effects = snapshot.pending_float_side_effects;
        self.applied_clearance_count = snapshot.applied_clearance_count;
        self.preserve_scoped_paint_public_order = snapshot.preserve_scoped_paint_public_order;
        self.defer_next_block_decoration_promotion = snapshot.defer_next_block_decoration_promotion;
    }

    pub(in crate::layout) fn into_font_system(self: Box<Self>) -> FontSystem {
        *self.font_system
    }

    pub(in crate::layout) fn finish_boxed(mut self: Box<Self>) -> Document {
        self.flush_positioned_layers();
        self.apply_pending_float_fragments_for_current_page();
        if self.current_page_has_content() {
            self.push_page();
        }
        while !self.pending_float_fragments.is_empty()
            || !self.pending_float_side_effects.is_empty()
        {
            self.apply_pending_float_fragments_for_current_page();
            if self.current_page_has_content() {
                self.push_page();
            } else {
                break;
            }
        }
        let option_font_size = self.options.font_size();
        let option_line_height = self.options.line_height();
        if self.pages.is_empty() {
            let mut page = page_for_context(self.current_page_context);
            page.push_line(RenderedLine::from_paint_origin(
                String::new(),
                paint_space_point(self.page_left(), self.page_top() - option_font_size),
                option_font_size,
                {
                    let mut style = ComputedStyle::initial();
                    style.font_size = option_font_size;
                    style.line_height_value =
                        css::ComputedLineHeight::from_points(option_line_height);
                    style.line_height = option_line_height;
                    style.line_height_multiplier = None;
                    style.line_height_is_normal = false;
                    self.font_system.resolve_style(&style)
                },
                Color::BLACK,
                Vec::new(),
            ));
            self.pages.push(page);
            self.page_names.push(self.current_page_name.clone());
            self.page_blanks.push(false);
            self.page_named_strings
                .push(std::mem::take(&mut self.current_page_named_strings));
            self.page_running_elements
                .push(std::mem::take(&mut self.current_page_running_elements));
        }
        self.apply_fixed_layers_to_pages();
        self.add_page_backgrounds();
        self.add_page_margin_boxes();
        for page in &mut self.pages {
            page.finalize_paint_tree_for_public_view();
        }
        let fonts = (*self.font_system).into_fonts();
        Document {
            pages: self.pages,
            fonts,
            bookmarks: self.bookmarks,
            metadata: DocumentMetadata {
                producer: self.options.producer.clone(),
                ..DocumentMetadata::default()
            },
        }
    }

    /// Inserts page-box background and border paint below document content.
    ///
    /// CSS Paged Media allows backgrounds and borders on the page box, and CSS
    /// Backgrounds and Borders paints backgrounds below borders. These
    /// primitives are inserted at the start of the PDF page paint stream so
    /// normal document content remains above the page underlay:
    /// <https://www.w3.org/TR/css-page-3/#page-properties> and
    /// <https://www.w3.org/TR/css-backgrounds-3/#layering>.
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
            let mut has_visible_page_paint = false;
            if !declarations.is_empty() {
                let mut style = ComputedStyle::initial();
                css::apply_declarations(&mut style, &declarations);
                let page_ch_advance = self.font_system.ch_advance(&style);
                style.resolve_font_metric_lengths(page_ch_advance);
                has_visible_page_paint = page_style_has_visible_paint(&style);
                let page_margins = PageContext::from_options(self.options).margins;
                let mut images = Vec::new();
                let page_border_area = page_background_positioning_area(
                    &declarations,
                    page_margins,
                    page_size,
                    css::BackgroundBox::Border,
                    page_ch_advance,
                );
                for layer in page_background_layers_for_paint(&style).iter().rev() {
                    let mut layer_style = style.clone();
                    layer_style.background_image = layer.image.clone();
                    layer_style.background_size = layer.size;
                    layer_style.background_position = layer.position;
                    layer_style.background_repeat = layer.repeat;
                    layer_style.background_origin = css::BackgroundBox::Border;
                    layer_style.background_clip = css::BackgroundBox::Border;
                    let mut paint_layer = layer.clone();
                    paint_layer.origin = css::BackgroundBox::Border;
                    paint_layer.clip = css::BackgroundBox::Border;
                    layer_style.background_layers = vec![paint_layer];
                    let image_area = page_background_positioning_area(
                        &declarations,
                        page_margins,
                        page_size,
                        layer.origin,
                        page_ch_advance,
                    );
                    let clip_area = page_background_positioning_area(
                        &declarations,
                        page_margins,
                        page_size,
                        layer.clip,
                        page_ch_advance,
                    );
                    let rounded_clip = rounded_background_clip_for_box(
                        page_border_area.x,
                        page_border_area.y,
                        page_border_area.width,
                        page_border_area.height,
                        &style,
                        used_border_widths(&style),
                        layer.clip,
                    );
                    images.extend(
                        clip_background_images_to_area(
                            self.background_images(
                                image_area.x,
                                image_area.y,
                                image_area.width,
                                image_area.height,
                                &layer_style,
                            ),
                            clip_area,
                        )
                        .into_iter()
                        .map(|image| match rounded_clip.clone() {
                            Some(clip) => image.with_clip(clip),
                            None => image,
                        }),
                    );
                }
                let page = &mut self.pages[page_index];

                let mut background_style = style.clone();
                background_style.border_widths = css::Edges::ZERO;
                background_style.border_width_values =
                    css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
                background_style.border_styles = css::BorderStyles::NONE;
                background_style.border_width = 0.0;
                let (rects, rounded_rects, paths, strokes) =
                    block_paint_ops(0.0, 0.0, page_width, page_height, &background_style);
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
                for image in images {
                    page.push_image_in_band(PaintBand::PageBackground, image);
                }

                let mut border_style = style;
                border_style.background_color = None;
                border_style.background_image = None;
                border_style.background_layers.clear();
                let (rects, rounded_rects, paths, strokes) =
                    block_paint_ops(0.0, 0.0, page_width, page_height, &border_style);
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
            }
            self.add_document_canvas_background(
                page_index,
                page_size,
                has_visible_page_paint
                    || (!self.root_canvas_background_defined
                        && self.pages.len() > 1
                        && self.has_authored_page_rules()),
            );
        }
    }

    pub(in crate::layout) fn has_authored_page_rules(&self) -> bool {
        !self.page_rules.is_empty() || !self.first_page_declarations.is_empty()
    }

    pub(in crate::layout) fn add_document_canvas_background(
        &mut self,
        page_index: usize,
        page_size: PageSize,
        has_visible_page_paint: bool,
    ) {
        let Some(style) = self.document_canvas_background.clone() else {
            return;
        };
        let (x, y, width, height) = if has_visible_page_paint {
            let context = self.finished_page_context(page_index + 1, page_size);
            (
                context.left(),
                context.bottom(),
                context.area_width(),
                context.area_height(),
            )
        } else {
            (0.0, 0.0, page_size.width(), page_size.height())
        };
        let images = self.background_images(x, y, width, height, &style);
        let page = &mut self.pages[page_index];
        let (rects, rounded_rects, paths, strokes) = block_paint_ops(x, y, width, height, &style);
        for rect in rects {
            page.push_rect_in_band(PaintBand::BackgroundBorder, rect);
        }
        for rounded_rect in rounded_rects {
            page.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rounded_rect);
        }
        for path in paths {
            page.push_path_in_band(PaintBand::BackgroundBorder, path);
        }
        for stroke in strokes {
            page.push_stroke_in_band(PaintBand::BackgroundBorder, stroke);
        }
        for image in images {
            page.push_image_in_band(PaintBand::BackgroundBorder, image);
        }
    }

    pub(in crate::layout) fn add_bookmark(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        x: f32,
        y: f32,
    ) {
        if self.element_side_effect_suppression_depth > 0 {
            return;
        }
        let Some(level) = style.bookmark_level else {
            return;
        };
        if style.display.is_none() || style.visibility != Visibility::Visible {
            return;
        }
        let label = collapse_whitespace(&evaluate_bookmark_label(element, style));
        if label.is_empty() {
            return;
        }
        self.bookmarks.push(Bookmark::new(
            level,
            label,
            self.pages.len(),
            x,
            y,
            match style.bookmark_state {
                CssBookmarkState::Open => BookmarkState::Open,
                CssBookmarkState::Closed => BookmarkState::Closed,
            },
        ));
    }

    /// Captures the propagated document-canvas background source.
    ///
    /// CSS Backgrounds defines the special root/body background propagation
    /// rule: the root element background paints the canvas; when the root has
    /// no background, the first body background is propagated instead. In
    /// paged media, that propagated canvas background paints each page canvas
    /// unless an explicit visible page background or border owns the margin
    /// paint:
    /// <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds> and
    /// <https://www.w3.org/TR/css-page-3/#painting>.
    pub(in crate::layout) fn capture_document_canvas_background(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) {
        if self.element_side_effect_suppression_depth > 0 {
            return;
        }
        if !is_document_canvas_element(element) {
            return;
        }
        let has_background = style.background_color.is_some_and(Color::is_visible)
            || style.background_image.is_some();
        if element.tag.eq_ignore_ascii_case("html") {
            self.root_canvas_background_defined = has_background;
            if has_background {
                self.document_canvas_background = Some(canvas_background_style(style));
            }
        } else if element.tag.eq_ignore_ascii_case("body")
            && !self.root_canvas_background_defined
            && has_background
        {
            self.document_canvas_background = Some(canvas_background_style(style));
        }
    }

    /// Records the generated page containing an HTML anchor.
    ///
    /// WeasyPrint's UA stylesheet maps `[id]` and `a[name]` to document
    /// anchors, and CSS Generated Content for Paged Media allows generated
    /// content such as `target-counter(..., page)` to resolve those targets:
    /// <https://www.w3.org/TR/css-gcpm-3/#cross-references>.
    pub(in crate::layout) fn add_page_anchor(&mut self, element: &Element, style: &ComputedStyle) {
        if self.element_side_effect_suppression_depth > 0 {
            return;
        }
        if let Some(id) = element.attrs.get("id").filter(|value| !value.is_empty()) {
            self.page_anchors
                .entry(id.clone())
                .or_insert(self.pages.len());
            if !self.page_anchor_text.contains_key(id) {
                let anchor_text = self.anchor_text_for_element(element, style);
                self.page_anchor_text.insert(id.clone(), anchor_text);
            }
        }
        if element.tag.eq_ignore_ascii_case("a")
            && let Some(name) = element.attrs.get("name").filter(|value| !value.is_empty())
        {
            self.page_anchors
                .entry(name.clone())
                .or_insert(self.pages.len());
            if !self.page_anchor_text.contains_key(name) {
                let anchor_text = self.anchor_text_for_element(element, style);
                self.page_anchor_text.insert(name.clone(), anchor_text);
            }
        }
    }

    /// Captures text exposed to generated-content cross references.
    ///
    /// CSS Generated Content for Paged Media defines `target-text()` keywords
    /// for target element content and generated `::before`/`::after` text. This
    /// helper records those values at layout time so page-margin generated
    /// content can resolve them after pagination:
    /// <https://www.w3.org/TR/css-gcpm-3/#target-text>.
    pub(in crate::layout) fn anchor_text_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> AnchorText {
        AnchorText {
            content: target_element_text(element),
            before: self
                .evaluate_generated_pseudo_text_rollback(element, style.before_style.as_deref()),
            after: self
                .evaluate_generated_pseudo_text_rollback(element, style.after_style.as_deref()),
        }
    }
    pub(in crate::layout) fn flush_positioned_layers(&mut self) {
        if self.positioned_layers.is_empty() {
            return;
        }
        let mut positioned_layers = std::mem::take(&mut self.positioned_layers);
        positioned_layers.sort_by_key(|layer| {
            (
                layer.page_index,
                layer.stack_level.sort_key(),
                layer.context.source_order,
            )
        });
        for layer in positioned_layers {
            let fragment = positioned_layer_fragment(&layer);
            let target_page = if layer.page_index < self.pages.len() {
                &mut self.pages[layer.page_index]
            } else {
                &mut self.current_page
            };
            let recorded = target_page.record_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
            target_page.append_recorded_paint_fragment(recorded);
            target_page.sort_paint_tree_stacking_contexts();
        }
    }
    pub(in crate::layout) fn flush_positioned_layers_since(&mut self, start_index: usize) {
        if start_index >= self.positioned_layers.len() {
            return;
        }
        let mut subtree_layers = self.positioned_layers.split_off(start_index);
        subtree_layers.sort_by_key(|layer| layer.stack_level.sort_key());
        for layer in subtree_layers {
            let fragment = positioned_layer_fragment(&layer);
            self.current_page
                .append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
        }
    }

    pub(in crate::layout) fn apply_fixed_layers_to_pages(&mut self) {
        if self.fixed_layers.is_empty() {
            return;
        }
        self.fixed_layers
            .sort_by_key(|layer| (layer.stack_level.sort_key(), layer.context.source_order));
        let fixed_layers = self.fixed_layers.clone();
        for page in &mut self.pages {
            for layer in &fixed_layers {
                append_fixed_layer_to_page(page, layer);
            }
        }
    }
}
