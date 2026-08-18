use super::*;
use crate::layout::assets::DocumentPageIndex;
use crate::layout::assets::fixed_background_page_margin_box;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn snapshot(&self) -> LayoutSnapshot {
        LayoutSnapshot {
            rollback: RollbackLayoutState {
                pages: self.pages.clone(),
                page_names: self.page_names.clone(),
                page_blanks: self.page_blanks.clone(),
                page_name_scope_suppression: self.page_name_scope_suppression,
                page_name_element_scope_suppression: self.page_name_element_scope_suppression,
                page_value_scope_stack: self.page_value_scope_stack.clone(),
                page_named_strings: self.page_named_strings.clone(),
                page_running_elements: self.page_running_elements.clone(),
                suppressed_named_strings_before: self.suppressed_named_strings_before.clone(),
                suppressed_named_strings_after: self.suppressed_named_strings_after.clone(),
                page_anchors: self.page_anchors.clone(),
                page_anchor_source_positions: self.page_anchor_source_positions.clone(),
                page_anchor_text: self.page_anchor_text.clone(),
                page_anchor_counters: self.page_anchor_counters.clone(),
                has_normal_flow_target_references: self.has_normal_flow_target_references,
                document_canvas_background: self.document_canvas_background.clone(),
                document_canvas_scroll_translation: self.document_canvas_scroll_translation,
                document_canvas_root_positioning_area: self.document_canvas_root_positioning_area,
                document_canvas_overflow: self.document_canvas_overflow,
                document_canvas_fragment_insets: self.document_canvas_fragment_insets.clone(),
                current_page: self.current_page.clone(),
                current_page_has_flow_content: self.current_page_has_flow_content,
                current_page_has_named_page_flow_content: self
                    .current_page_has_named_page_flow_content,
                current_page_selected_name: self.current_page_selected_name.clone(),
                last_block_layout_outcome: self.last_block_layout_outcome,
                last_principal_transform_box: self.last_principal_transform_box,
                current_page_name: self.current_page_name.clone(),
                current_page_context: self.current_page_context,
                initial_viewport_context: self.initial_viewport_context,
                fragmentainer_override: self.fragmentainer_override,
                footnote_measurements: self.footnote_measurements.clone(),
                rendered_footnote_measurements: self.rendered_footnote_measurements.clone(),
                measured_footnotes: self.measured_footnotes.clone(),
                committed_inline_floats: self.committed_inline_floats.clone(),
                rendered_footnotes: self.rendered_footnotes.clone(),
                footnote_measurement_depth: self.footnote_measurement_depth,
                fragmentation_suppression_depth: self.fragmentation_suppression_depth,
                multicol_spanner_fragmentation_depth: self.multicol_spanner_fragmentation_depth,
                multicol_spanner_speculation_depth: self.multicol_spanner_speculation_depth,
                multicol_balance_probe_depth: self.multicol_balance_probe_depth,
                cursor_y: self.cursor_y,
                content_left: self.content_left,
                content_right: self.content_right,
                table_cell_content_coordinate_contexts: self
                    .table_cell_content_coordinate_contexts
                    .clone(),
                principal_body_block_end_inset: self.principal_body_block_end_inset,
                root_principal_flow_context: self.root_principal_flow_context,
                fragmentainer_transition_recorder_lengths: self
                    .fragmentainer_transition_recorders
                    .iter()
                    .map(FragmentainerTransitionRecorder::len)
                    .collect(),
                root_pseudo_block_projection: self.root_pseudo_block_projection,
                direct_block_layout_constraint: self.direct_block_layout_constraint,
                inline_split_float_exclusion_query_offset: self
                    .inline_split_float_exclusion_query_offset,
                content_logical_inline_size_stack: self.content_logical_inline_size_stack.clone(),
                container_unit_contexts: self.container_unit_contexts.clone(),
                multicol_column_containing_blocks: self.multicol_column_containing_blocks.clone(),
                intrinsic_inline_percentage_basis_stack: self
                    .intrinsic_inline_percentage_basis_stack
                    .clone(),
                inline_static_position: self.inline_static_position,
                text_box_line_trim_stack: self.text_box_line_trim_stack.clone(),
                clamp_line_slot_captures: self.clamp_line_slot_captures.clone(),
                positioned_inline_layout_suppression_depth: self
                    .positioned_inline_layout_suppression_depth,
                last_in_flow_line_baseline_y: self.last_in_flow_line_baseline_y,
                pending_outside_marker_anchors: self.pending_outside_marker_anchors.clone(),
                block_static_position_y_offset: self.block_static_position_y_offset,
                absolute_static_position: self.absolute_static_position,
                grid_positioning_scopes: self.grid_positioning_scopes.clone(),
                pending_subgrid_contexts: self.pending_subgrid_contexts.clone(),
                escaped_atom_positioning_depth: self.escaped_atom_positioning_depth,
                escaped_atom_containing_block: self.escaped_atom_containing_block,
                escaped_atom_positioning_context: self.escaped_atom_positioning_context,
                containing_block_writing_mode: self.containing_block_writing_mode,
                fragment_top_offsets: self.fragment_top_offsets.clone(),
                child_available_space_stack: self.child_available_space_stack.clone(),
                normal_flow_relative_containing_blocks: self
                    .normal_flow_relative_containing_blocks
                    .clone(),
                static_position_containing_blocks: self.static_position_containing_blocks.clone(),
                definite_block_size_stack: self.definite_block_size_stack.clone(),
                replayed_flex_item_percentage_height_bases: self
                    .replayed_flex_item_percentage_height_bases
                    .clone(),
                table_wrapper_block_size_overrides: self.table_wrapper_block_size_overrides.clone(),
                positioned_table_sizing: self.positioned_table_sizing.clone(),
                multicol_text_box_trim_end_child_indices: self
                    .multicol_text_box_trim_end_child_indices
                    .clone(),
                truncate_page_start_margins: self.truncate_page_start_margins,
                avoid_inside_retry_depth: self.avoid_inside_retry_depth,
                out_of_flow_prebreak_suppression_depth: self.out_of_flow_prebreak_suppression_depth,
                layout_pass_kind: self.layout_pass_kind,
                element_side_effect_suppression_depth: self.element_side_effect_suppression_depth,
                containing_blocks: self.containing_blocks.clone(),
                fixed_containing_blocks: self.fixed_containing_blocks.clone(),
                active_multicol_positioned_containing_block_spans: self
                    .active_multicol_positioned_containing_block_spans
                    .clone(),
                counter_set: self.counter_set.clone(),
                counter_plan: self.counter_plan.clone(),
                quote_depth: self.quote_depth,
                current_page_named_strings: self.current_page_named_strings.clone(),
                current_page_running_elements: self.current_page_running_elements.clone(),
                next_assignment_id: self.next_assignment_id,
                assignment_capture_stack: self.assignment_capture_stack.clone(),
                ancestors: self.ancestors.clone(),
                page_counter_initial_values: self.page_counter_initial_values.clone(),
                bookmarks: self.bookmarks.clone(),
                positioned_layers: self.positioned_layers.clone(),
                committed_positioned_paint_identities: self
                    .committed_positioned_paint_identities
                    .clone(),
                positioned_paint_transaction_depth: self.positioned_paint_transaction_depth,
                positioned_scratch_page_limit: self.positioned_scratch_page_limit,
                positioned_scratch_page_origin: self.positioned_scratch_page_origin,
                fixed_layers: self.fixed_layers.clone(),
                absolute_positioned_page_span_target: self.absolute_positioned_page_span_target,
                pending_positioned_fragmentation: self.pending_positioned_fragmentation,
                next_paint_source_order: self.next_paint_source_order,
                overflow_clips: self.overflow_clips.clone(),
                active_scroll_snap_scopes: self.active_scroll_snap_scopes.clone(),
                next_float_id: self.next_float_id,
                float_contexts: self.float_contexts.clone(),
                float_fragment_parent_inline_spans: self.float_fragment_parent_inline_spans.clone(),
                adjoining_float_origin_y: self.adjoining_float_origin_y,
                pending_paint_fragments: self.pending_paint_fragments.clone(),
                pending_page_side_effects: self.pending_page_side_effects.clone(),
                float_paint_capture_depth: self.float_paint_capture_depth,
                preserve_scoped_paint_public_order: self.preserve_scoped_paint_public_order,
                defer_next_block_decoration_promotion: self.defer_next_block_decoration_promotion,
                pending_page_footnotes: self.pending_page_footnotes.clone(),
            },
        }
    }

    pub(in crate::layout) fn restore(&mut self, snapshot: LayoutSnapshot) {
        let LayoutSnapshot { rollback: snapshot } = snapshot;
        self.pages = snapshot.pages;
        self.page_names = snapshot.page_names;
        self.page_blanks = snapshot.page_blanks;
        self.page_name_scope_suppression = snapshot.page_name_scope_suppression;
        self.page_name_element_scope_suppression = snapshot.page_name_element_scope_suppression;
        self.page_value_scope_stack = snapshot.page_value_scope_stack;
        self.page_named_strings = snapshot.page_named_strings;
        self.page_running_elements = snapshot.page_running_elements;
        self.suppressed_named_strings_before = snapshot.suppressed_named_strings_before;
        self.suppressed_named_strings_after = snapshot.suppressed_named_strings_after;
        self.page_anchors = snapshot.page_anchors;
        self.page_anchor_source_positions = snapshot.page_anchor_source_positions;
        self.page_anchor_text = snapshot.page_anchor_text;
        self.page_anchor_counters = snapshot.page_anchor_counters;
        self.has_normal_flow_target_references = snapshot.has_normal_flow_target_references;
        self.document_canvas_background = snapshot.document_canvas_background;
        self.document_canvas_scroll_translation = snapshot.document_canvas_scroll_translation;
        self.document_canvas_root_positioning_area = snapshot.document_canvas_root_positioning_area;
        self.document_canvas_overflow = snapshot.document_canvas_overflow;
        self.document_canvas_fragment_insets = snapshot.document_canvas_fragment_insets;
        self.current_page = snapshot.current_page;
        self.current_page_has_flow_content = snapshot.current_page_has_flow_content;
        self.current_page_has_named_page_flow_content =
            snapshot.current_page_has_named_page_flow_content;
        self.current_page_selected_name = snapshot.current_page_selected_name;
        self.last_block_layout_outcome = snapshot.last_block_layout_outcome;
        self.last_principal_transform_box = snapshot.last_principal_transform_box;
        self.current_page_name = snapshot.current_page_name;
        self.current_page_context = snapshot.current_page_context;
        self.initial_viewport_context = snapshot.initial_viewport_context;
        self.fragmentainer_override = snapshot.fragmentainer_override;
        self.footnote_measurements = snapshot.footnote_measurements;
        self.rendered_footnote_measurements = snapshot.rendered_footnote_measurements;
        self.measured_footnotes = snapshot.measured_footnotes;
        self.committed_inline_floats = snapshot.committed_inline_floats;
        self.rendered_footnotes = snapshot.rendered_footnotes;
        self.footnote_measurement_depth = snapshot.footnote_measurement_depth;
        self.fragmentation_suppression_depth = snapshot.fragmentation_suppression_depth;
        self.multicol_spanner_fragmentation_depth = snapshot.multicol_spanner_fragmentation_depth;
        self.multicol_spanner_speculation_depth = snapshot.multicol_spanner_speculation_depth;
        self.multicol_balance_probe_depth = snapshot.multicol_balance_probe_depth;
        self.cursor_y = snapshot.cursor_y;
        self.content_left = snapshot.content_left;
        self.content_right = snapshot.content_right;
        self.table_cell_content_coordinate_contexts =
            snapshot.table_cell_content_coordinate_contexts;
        self.principal_body_block_end_inset = snapshot.principal_body_block_end_inset;
        self.root_principal_flow_context = snapshot.root_principal_flow_context;
        debug_assert!(
            self.fragmentainer_transition_recorders.len()
                >= snapshot.fragmentainer_transition_recorder_lengths.len(),
            "a speculative restore may not discard an outer transition-recorder scope"
        );
        for (recorder, len) in self
            .fragmentainer_transition_recorders
            .iter()
            .zip(snapshot.fragmentainer_transition_recorder_lengths)
        {
            recorder.truncate(len);
        }
        self.root_pseudo_block_projection = snapshot.root_pseudo_block_projection;
        self.direct_block_layout_constraint = snapshot.direct_block_layout_constraint;
        self.inline_split_float_exclusion_query_offset =
            snapshot.inline_split_float_exclusion_query_offset;
        self.content_logical_inline_size_stack = snapshot.content_logical_inline_size_stack;
        self.container_unit_contexts = snapshot.container_unit_contexts;
        self.multicol_column_containing_blocks = snapshot.multicol_column_containing_blocks;
        self.intrinsic_inline_percentage_basis_stack =
            snapshot.intrinsic_inline_percentage_basis_stack;
        self.inline_static_position = snapshot.inline_static_position;
        self.text_box_line_trim_stack = snapshot.text_box_line_trim_stack;
        self.clamp_line_slot_captures = snapshot.clamp_line_slot_captures;
        self.positioned_inline_layout_suppression_depth =
            snapshot.positioned_inline_layout_suppression_depth;
        self.last_in_flow_line_baseline_y = snapshot.last_in_flow_line_baseline_y;
        self.pending_outside_marker_anchors = snapshot.pending_outside_marker_anchors;
        self.block_static_position_y_offset = snapshot.block_static_position_y_offset;
        self.absolute_static_position = snapshot.absolute_static_position;
        self.grid_positioning_scopes = snapshot.grid_positioning_scopes;
        self.pending_subgrid_contexts = snapshot.pending_subgrid_contexts;
        self.escaped_atom_positioning_depth = snapshot.escaped_atom_positioning_depth;
        self.escaped_atom_containing_block = snapshot.escaped_atom_containing_block;
        self.escaped_atom_positioning_context = snapshot.escaped_atom_positioning_context;
        self.containing_block_writing_mode = snapshot.containing_block_writing_mode;
        self.fragment_top_offsets = snapshot.fragment_top_offsets;
        self.child_available_space_stack = snapshot.child_available_space_stack;
        self.normal_flow_relative_containing_blocks =
            snapshot.normal_flow_relative_containing_blocks;
        self.static_position_containing_blocks = snapshot.static_position_containing_blocks;
        self.definite_block_size_stack = snapshot.definite_block_size_stack;
        self.replayed_flex_item_percentage_height_bases =
            snapshot.replayed_flex_item_percentage_height_bases;
        self.table_wrapper_block_size_overrides = snapshot.table_wrapper_block_size_overrides;
        self.positioned_table_sizing = snapshot.positioned_table_sizing;
        self.multicol_text_box_trim_end_child_indices =
            snapshot.multicol_text_box_trim_end_child_indices;
        self.truncate_page_start_margins = snapshot.truncate_page_start_margins;
        self.avoid_inside_retry_depth = snapshot.avoid_inside_retry_depth;
        self.out_of_flow_prebreak_suppression_depth =
            snapshot.out_of_flow_prebreak_suppression_depth;
        self.layout_pass_kind = snapshot.layout_pass_kind;
        self.element_side_effect_suppression_depth = snapshot.element_side_effect_suppression_depth;
        self.containing_blocks = snapshot.containing_blocks;
        self.fixed_containing_blocks = snapshot.fixed_containing_blocks;
        self.active_multicol_positioned_containing_block_spans =
            snapshot.active_multicol_positioned_containing_block_spans;
        self.counter_set = snapshot.counter_set;
        self.counter_plan = snapshot.counter_plan;
        self.quote_depth = snapshot.quote_depth;
        self.current_page_named_strings = snapshot.current_page_named_strings;
        self.current_page_running_elements = snapshot.current_page_running_elements;
        self.next_assignment_id = snapshot.next_assignment_id;
        self.assignment_capture_stack = snapshot.assignment_capture_stack;
        self.ancestors = snapshot.ancestors;
        self.page_counter_initial_values = snapshot.page_counter_initial_values;
        self.bookmarks = snapshot.bookmarks;
        self.positioned_layers = snapshot.positioned_layers;
        self.committed_positioned_paint_identities = snapshot.committed_positioned_paint_identities;
        self.positioned_paint_transaction_depth = snapshot.positioned_paint_transaction_depth;
        self.positioned_scratch_page_limit = snapshot.positioned_scratch_page_limit;
        self.positioned_scratch_page_origin = snapshot.positioned_scratch_page_origin;
        self.fixed_layers = snapshot.fixed_layers;
        self.absolute_positioned_page_span_target = snapshot.absolute_positioned_page_span_target;
        self.pending_positioned_fragmentation = snapshot.pending_positioned_fragmentation;
        self.next_paint_source_order = snapshot.next_paint_source_order;
        self.overflow_clips = snapshot.overflow_clips;
        self.active_scroll_snap_scopes = snapshot.active_scroll_snap_scopes;
        self.next_float_id = snapshot.next_float_id;
        self.float_contexts = snapshot.float_contexts;
        self.float_fragment_parent_inline_spans = snapshot.float_fragment_parent_inline_spans;
        self.adjoining_float_origin_y = snapshot.adjoining_float_origin_y;
        self.pending_paint_fragments = snapshot.pending_paint_fragments;
        self.pending_page_side_effects = snapshot.pending_page_side_effects;
        self.float_paint_capture_depth = snapshot.float_paint_capture_depth;
        self.preserve_scoped_paint_public_order = snapshot.preserve_scoped_paint_public_order;
        self.defer_next_block_decoration_promotion = snapshot.defer_next_block_decoration_promotion;
        self.pending_page_footnotes = snapshot.pending_page_footnotes;
    }

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
                        css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
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
                    css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
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
                        | PaintPrimitive::Line(_)
                        | PaintPrimitive::OpaqueTextCoverage { .. } => {}
                    }
                }
            }
            self.add_document_canvas_background(page_index, page_size);
        }
    }

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

    pub(in crate::layout) fn add_bookmark(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        position: PaintPoint,
    ) {
        if self.element_side_effect_suppression_depth > 0 {
            return;
        }
        let css::BookmarkLevel::Level(level) = style.bookmark_level else {
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
            level.get(),
            label,
            self.pages.len(),
            position.x,
            position.y,
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
            self.record_page_anchor(id.clone(), element, style);
        }
        if element.tag.eq_ignore_ascii_case("a")
            && let Some(name) = element.attrs.get("name").filter(|value| !value.is_empty())
        {
            self.record_page_anchor(name.clone(), element, style);
        }
    }

    fn record_page_anchor(&mut self, name: String, element: &Element, style: &ComputedStyle) {
        self.page_anchors
            .entry(name.clone())
            .or_insert(self.pages.len());
        self.page_anchor_source_positions
            .entry(name.clone())
            .or_insert_with(|| PaintPoint::new(self.content_left, self.cursor_y));
        if !self.page_anchor_text.contains_key(&name) {
            let anchor_text = self.anchor_text_for_element(element, style);
            self.page_anchor_text.insert(name.clone(), anchor_text);
        }
        let counters =
            self.counter_stacks_at_origin(element, box_tree::CounterEventSource::Principal);
        self.page_anchor_counters.entry(name).or_insert(counters);
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
            before: self.evaluate_generated_pseudo_text_rollback(
                element,
                box_tree::CounterEventSource::Before,
                style.before_style.as_deref(),
            ),
            after: self.evaluate_generated_pseudo_text_rollback(
                element,
                box_tree::CounterEventSource::After,
                style.after_style.as_deref(),
            ),
        }
    }
    pub(in crate::layout) fn flush_positioned_layers(&mut self) {
        if self.positioned_layers.is_empty() || self.positioned_paint_transaction_depth > 0 {
            return;
        }
        let mut future_layers = Vec::new();
        let mut positioned_layers = Vec::new();
        for layer in std::mem::take(&mut self.positioned_layers) {
            if layer.page_index > self.pages.len() {
                future_layers.push(layer);
            } else {
                positioned_layers.push(layer);
            }
        }
        self.positioned_layers = future_layers;
        if positioned_layers.is_empty() {
            return;
        }
        positioned_layers.sort_by_key(|layer| {
            (
                layer.page_index,
                layer.stack_level.sort_key(),
                layer.context.source_order,
            )
        });
        for layer in positioned_layers {
            if let Some(identity) = layer.commit_key() {
                // A positioned principal can be reached through an inline
                // collector's retained source and its ruby-specific overlay.
                // They describe the same page-local principal, which must be
                // committed once even when both collection paths survive to
                // final paint.
                if !self
                    .committed_positioned_paint_identities
                    .insert((DocumentPageIndex::new(layer.page_index), identity))
                {
                    continue;
                }
            }
            let fragment = positioned_layer_fragment(&layer);
            let target_page = if layer.page_index < self.pages.len() {
                &mut self.pages[layer.page_index]
            } else {
                &mut self.current_page
            };
            let recorded =
                target_page.record_paint_fragment_owned(fragment, PaintTranslation::identity());
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
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
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
