use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn finish_boxed(mut self: Box<Self>) -> LayoutPass {
        self.materialize_pending_positioned_page_span();
        self.flush_positioned_layers();
        self.apply_pending_fragments_for_current_page();
        if self.current_page_has_content() {
            self.push_page();
        }
        while !self.pending_paint_fragments.is_empty() || !self.pending_page_side_effects.is_empty()
        {
            self.apply_pending_fragments_for_current_page();
            if self.current_page_has_content() {
                self.push_page();
            } else {
                // Speculative overflow paint can target a later page even
                // when ordinary flow never occupies the intervening page.
                // Materialize that empty page so the next iteration reaches
                // the queued destination fragment.
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                self.materialize_empty_current_page_for_deferred_fragment();
            }
        }
        // `push_page` delivers deferred paint for the next page after moving
        // the current one into `pages`. If that delivery resolved the final
        // pending fragment, the loop above exits with a real, populated
        // current page that still needs committing. This commonly occurs for
        // the final fragment of a floated or overflowed box when no normal
        // flow later reaches that page.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        if self.current_page_has_content() {
            self.push_page();
        }
        let option_font_size = self.options.font_size();
        if self.pages.is_empty() {
            let mut page = page_for_context(self.current_page_context);
            // This synthesized line exists only to retain an empty page in the
            // public document model. It has neither text nor glyph runs, so
            // selecting a font would be needless work and would retain an
            // unused system font entry in an otherwise font-free document.
            page.push_line(RenderedLine::from_paint_origin(
                String::new(),
                paint_space_point(self.page_left(), self.page_top() - option_font_size),
                option_font_size,
                None,
                CssColor::BLACK,
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
        // Fixed-position descendants replay over the final page sequence.
        // Their paint is a retention reason for pages established by actual
        // out-of-flow fragmentation, so replay them before finalization.
        // <https://www.w3.org/TR/css-position-3/#fixed-pos>
        self.apply_fixed_layers_to_pages();
        self.discard_trailing_geometry_only_pages();
        let target_references = TargetReferenceSnapshot {
            anchors: self
                .page_anchors
                .iter()
                .filter_map(|(name, page_index)| {
                    Some((
                        name.clone(),
                        TargetAnchor {
                            page_index: *page_index,
                            text: self.page_anchor_text.get(name)?.clone(),
                            counters: self.page_anchor_counters.get(name)?.clone(),
                        },
                    ))
                })
                .collect(),
            total_pages: self.pages.len(),
        };
        if self.document_root_generates_box {
            self.add_page_backgrounds();
            self.add_page_margin_boxes();
        }
        for page in &mut self.pages {
            page.finalize_paint_tree_for_public_view();
        }
        let fonts = (*self.font_system).into_fonts();
        LayoutPass {
            document: Document {
                pages: self.pages,
                fonts,
                bookmarks: self.bookmarks,
                image_store: Box::default(),
                metadata: DocumentMetadata::default(),
            },
            target_references,
            has_normal_flow_target_references: self.has_normal_flow_target_references,
        }
    }

    pub(in crate::layout) fn materialize_empty_current_page_for_deferred_fragment(&mut self) {
        let next_context = self.resolved_page_context(
            self.destination_document_page_number(self.pages.len() + 2),
            false,
        );
        let next_page = page_for_context(next_context);
        let page = std::mem::replace(&mut self.current_page, next_page);
        self.pages.push(page);
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(false);
        self.page_named_strings
            .push(std::mem::take(&mut self.current_page_named_strings));
        self.page_running_elements
            .push(std::mem::take(&mut self.current_page_running_elements));
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.apply_page_context(next_context, FragmentOffsets::ZERO);
        self.current_page_selected_name = None;
        self.truncate_page_start_margins = true;
    }

    /// Discard trailing page fragments that exist only to carry normal-flow
    /// geometry during layout.
    ///
    /// CSS Fragmentation still requires a definite box to advance the logical
    /// block cursor through every crossed fragmentainer. A static PDF need not
    /// serialize trailing page boxes when no fragment paints, owns an anchor
    /// or bookmark, or carries generated-page state. A forced blank page
    /// exists only to satisfy a break before a following fragment, so a
    /// trailing run of such pages has no generated box to retain. Selecting a
    /// named type alone is likewise not observable without content. This runs
    /// before fixed and page-context paint so those effects
    /// repeat only over pages established by actual paint or structural
    /// pagination.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://www.w3.org/TR/css-page-3/#page-based-counters>
    fn discard_trailing_geometry_only_pages(&mut self) {
        // A propagated root/body background is painted on pages that survive
        // layout finalization; it does not itself establish a fragmentainer.
        // In particular, it must not turn trailing geometry-only fragments
        // into serialized pages merely because canvas paint is attached after
        // normal-flow layout has completed.
        // The first page also owns deferred document-canvas painting, which
        // is attached after normal-flow layout. Keep it even when its body
        // contributed only geometry while the later trailing fragments did
        // not establish any paint or structural page state.
        while self.pages.len() > 1 {
            let page_index = self.pages.len() - 1;
            let page_has_retention_reason = self.pages[page_index].has_paint_content()
                || self.pages[page_index].has_fragmentation_content()
                || !self.pages[page_index].links().is_empty()
                || self
                    .page_named_strings
                    .get(page_index)
                    .is_some_and(|assignments| !assignments.is_empty())
                || self
                    .page_running_elements
                    .get(page_index)
                    .is_some_and(|assignments| !assignments.is_empty())
                || self.page_anchors.values().any(|index| *index == page_index)
                || self
                    .bookmarks
                    .iter()
                    .any(|bookmark| bookmark.page_index == page_index);
            if page_has_retention_reason {
                break;
            }
            self.pages.pop();
            self.page_names.pop();
            self.page_blanks.pop();
            self.page_named_strings.pop();
            self.page_running_elements.pop();
        }
    }
}
