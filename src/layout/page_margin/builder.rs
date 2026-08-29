use super::*;
use crate::layout::assets::paint_effects_for_box;
use crate::layout::builder::{
    page_box_edges_from_declarations_with_ch_advance_and_root_metrics, page_for_context,
};
use crate::layout::page_generated::{PageMarginContentItem, ResolvedPageContent};

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn page_name_for_number(&self, page_number: usize) -> Option<&str> {
        page_number
            .checked_sub(1)
            .and_then(|index| self.page_names.get(index))
            .and_then(Option::as_deref)
    }

    pub(in crate::layout) fn page_is_blank_for_number(&self, page_number: usize) -> bool {
        page_number
            .checked_sub(1)
            .and_then(|index| self.page_blanks.get(index))
            .cloned()
            .unwrap_or(false)
    }

    pub(in crate::layout) fn page_declarations_for(&self, page_number: usize) -> Declarations {
        let page_name = self.page_name_for_number(page_number);
        let is_blank = self.page_is_blank_for_number(page_number);
        self.page_declarations_for_page(page_number, page_name, is_blank)
    }

    pub(in crate::layout) fn page_declarations_for_page(
        &self,
        page_number: usize,
        page_name: Option<&str>,
        is_blank: bool,
    ) -> Declarations {
        let mut declarations = self.page_declarations.clone();
        declarations.extend(cascade_page_rule_declarations(
            self.page_rules.iter().filter_map(|rule| {
                rule.matching_specificity(
                    page_number,
                    page_name,
                    is_blank,
                    self.page_progression_direction,
                )
                .map(|specificity| {
                    (
                        rule.origin,
                        specificity,
                        rule.layer_order.clone(),
                        rule.order,
                        &rule.declarations,
                    )
                })
            }),
        ));
        if declarations.is_empty() && page_number == 1 {
            return self.first_page_declarations.clone();
        }
        declarations
    }

    pub(in crate::layout) fn add_page_margin_boxes(&mut self) {
        if self
            .page_rules
            .iter()
            .all(|rule| rule.margin_boxes.is_empty())
        {
            return;
        }
        let total_pages = self.pages.len();
        let page_rules = self.page_rules.clone();
        let fallback_page_declarations = self.page_declarations.clone();
        let page_named_strings = self.page_named_strings.clone();
        let page_running_elements = self.page_running_elements.clone();
        let page_anchors = self.page_anchors.clone();
        let page_anchor_text = self.page_anchor_text.clone();
        let counter_styles = self.counter_styles.clone();
        let page_progression_direction = self.page_progression_direction;
        let base_page_context = PageContext::from_options(self.options);
        let base_page_style = self.page_margin_inherited_style.clone();
        let page_counter_values = page_counter_values_for_pages(
            total_pages,
            &page_rules,
            page_progression_direction,
            &fallback_page_declarations,
            &self.page_names,
            &self.page_blanks,
            &self.page_counter_initial_values,
        );
        for index in 0..self.pages.len() {
            let page_number = index + 1;
            let page_name = self.page_names.get(index).and_then(Option::as_deref);
            let is_blank = self.page_blanks.get(index).cloned().unwrap_or(false);
            let page_declarations = page_declarations_for_rules(
                &page_rules,
                page_number,
                page_name,
                is_blank,
                page_progression_direction,
                &fallback_page_declarations,
            );
            let mut boxes = page_margin_boxes_for_rules(PageMarginCascadeContext {
                page_rules: &page_rules,
                page_number,
                page_name,
                is_blank,
                page_progression_direction,
                page_declarations: &page_declarations,
                base_page_style: &base_page_style,
                initial_page_size: base_page_context.size,
            });
            let page_style = self.page_context_style_for_declarations(&page_declarations);
            let page_ch_advance =
                self.ch_advance_for_style(&page_style, page_style.requires_ch_advance());
            let root_metrics = self.root_metric_state.resolved().basis();
            for box_ in &mut boxes {
                let ch_advance =
                    self.ch_advance_for_style(&box_.style, box_.style.requires_ch_advance());
                box_.style.resolve_font_metric_lengths(ch_advance);
                if let RootMetricState::Resolved(root_metrics) = self.root_metric_state {
                    box_.style.root_font_size = root_metrics.basis().font_size.points();
                    box_.style
                        .resolve_root_font_metric_lengths(root_metrics.basis());
                }
            }
            let page_size = css::page_size_from_with_ch_advance_and_root_metrics(
                &page_declarations,
                base_page_context.size,
                page_ch_advance,
                root_metrics,
            );
            let page_edges = page_box_edges_from_declarations_with_ch_advance_and_root_metrics(
                &page_declarations,
                page_size,
                page_ch_advance,
                root_metrics,
            );
            let page_margins =
                css::page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style_and_root_metrics(
                    &page_declarations,
                    base_page_context.margins,
                    page_size,
                    css::PageMarginResolutionContext {
                        viewport_size: self.page_descriptor_viewport_size,
                        non_margin_edges: page_edges.total(),
                        ch_advance: page_ch_advance,
                        style: &page_style,
                        root_metrics,
                    },
                );
            let page_counters = page_counter_values
                .get(index)
                .cloned()
                .unwrap_or_else(|| self.page_counter_initial_values.clone());
            let context = PageMarginPaintContext {
                page_margins,
                page_edges,
                page_number,
                total_pages,
                base_url: self.base_url,
                root_url: self.root_url,
                resource_cache: self.resource_cache,
                page_index: index,
                page_named_strings: &page_named_strings,
                page_running_elements: &page_running_elements,
                page_anchors: &page_anchors,
                page_anchor_text: &page_anchor_text,
                counter_styles: &counter_styles,
                page_counters: &page_counters,
                page_counters_by_page: &page_counter_values,
                image_set_resolution_dppx: self.options.device_resolution_dppx(),
            };
            let page = self.pages[index].clone();
            let layouts = layout_page_margin_boxes(self, &page, &boxes, context);
            let mut painted_boxes = Vec::new();
            for layout in &layouts {
                let checkpoint = self.pages[index].paint_checkpoint();
                self.paint_page_margin_box_with_replay(index, layout, context);
                painted_boxes.push(PageMarginPaintedBox {
                    z_index: layout.spec.style.z_index.unwrap_or(0),
                    order: page_margin_box_paint_order(&layout.spec.name),
                    effects: paint_effects_for_box(&layout.spec.style, layout.border_clip()),
                    bounds: layout.border_clip(),
                    fragment: self.pages[index].take_paint_fragment_since(checkpoint),
                });
            }
            replay_page_margin_box_fragments(&mut self.pages[index], painted_boxes);
        }
    }

    pub(in crate::layout) fn paint_page_margin_box_with_replay(
        &mut self,
        page_index: usize,
        layout: &PageMarginBoxLayout<'_>,
        context: PageMarginPaintContext<'_>,
    ) {
        let content_sequence =
            if layout.content.is_empty() || layout.spec.style.visibility != Visibility::Visible {
                None
            } else {
                let fixed_box = PageMarginFixedBoxGeometry::from_layout(layout);
                self.page_margin_inline_sequence_with_replay(
                    &layout.content,
                    &layout.spec.style,
                    fixed_box.inline_size().points(),
                    fixed_box
                        .block_size()
                        .points()
                        .max(layout.spec.style.line_height),
                    context,
                )
            };
        paint_page_margin_box(&mut self.pages[page_index], layout, context);
        if let Some(sequence) = content_sequence {
            self.paint_page_margin_inline_sequence(page_index, layout, &sequence);
        }
    }

    pub(in crate::layout) fn page_margin_inline_sequence_with_replay(
        &mut self,
        content: &ResolvedPageContent,
        style: &ComputedStyle,
        available_width: f32,
        available_height: f32,
        context: PageMarginPaintContext<'_>,
    ) -> Option<inline_layout::InlineLineSequence> {
        let mut items = Vec::new();
        let mut quote_depth = 0usize;
        let inline_style = page_margin_inline_content_style(style);
        for item in &content.items {
            match item {
                PageMarginContentItem::EmbeddedRunningElement(capture) => {
                    if let Some(fragment) = self.replay_running_element_capture(
                        capture,
                        available_width,
                        available_height,
                    ) {
                        items.push(fragment);
                    } else {
                        for part in &running_element_inline_parts(capture) {
                            append_page_margin_inline_part(
                                &mut items,
                                part,
                                &inline_style,
                                style,
                                available_width,
                                context.base_url,
                                context.root_url,
                                context.resource_cache,
                                &mut quote_depth,
                            );
                        }
                    }
                }
                PageMarginContentItem::Inline(part) => append_page_margin_inline_part(
                    &mut items,
                    part,
                    &inline_style,
                    style,
                    available_width,
                    context.base_url,
                    context.root_url,
                    context.resource_cache,
                    &mut quote_depth,
                ),
                PageMarginContentItem::TargetCounter { .. }
                | PageMarginContentItem::TargetText { .. }
                // Deferred named-string counters are resolved before margin
                // box layout; retaining this arm makes that phase boundary
                // explicit if a malformed item ever reaches painting.
                | PageMarginContentItem::NamedStringPageCounter { .. } => {}
            }
        }
        (!items.is_empty()).then(|| {
            self.collect_inline_line_sequence_with_text_box_trim(
                items,
                style,
                available_width,
                0.0,
                0.0,
            )
        })
    }

    pub(in crate::layout) fn paint_page_margin_inline_sequence(
        &mut self,
        page_index: usize,
        layout: &PageMarginBoxLayout<'_>,
        sequence: &inline_layout::InlineLineSequence,
    ) {
        let style = &layout.spec.style;
        let inline_extent = sequence
            .occupied_physical_inline_extent(style)
            .points()
            .max(self.font_system.used_line_height(style).points());
        // In vertical writing modes the inline axis is physical y. Page
        // margin-box `vertical-align` therefore selects the inline-stack
        // position on that axis just as it does for horizontal text.
        // https://www.w3.org/TR/css-writing-modes-4/#abstract-box
        let line_inline_start =
            page_margin_text_stack_top(layout, style.vertical_align.clone(), inline_extent);
        let fixed_box = PageMarginFixedBoxGeometry::from_layout(layout)
            .with_line_inline_start(PageMarginPhysicalY::new(line_inline_start))
            .with_line_block_alignment(
                sequence.total_height(),
                sequence
                    .fixed_box_first_line_block_size()
                    .max(self.font_system.used_line_height(style).points()),
                style.vertical_align.clone(),
                style.writing_mode,
            );
        std::mem::swap(&mut self.current_page, &mut self.pages[page_index]);
        self.paint_inline_line_sequence_in_fixed_box(
            sequence,
            style,
            fixed_box.line_block_start_x().points(),
            fixed_box.inline_size().points(),
            fixed_box.line_inline_start_y().points(),
        );
        std::mem::swap(&mut self.current_page, &mut self.pages[page_index]);
    }

    /// Replays a captured running element into an isolated margin-box fragment.
    ///
    /// CSS Generated Content for Paged Media defines `element()` as placing the
    /// captured running element in generated content, not as serializing its
    /// text. The temporary page below gives the captured source a normal block,
    /// table, flex, replaced, float, and positioned layout environment, then
    /// extracts the resulting paint fragment before restoring document layout:
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements>.
    pub(in crate::layout) fn replay_running_element_capture(
        &mut self,
        capture: &RunningElementCapture,
        available_width: f32,
        available_height: f32,
    ) -> Option<InlineItem> {
        let snapshot = self.snapshot();
        let replay_width = available_width.max(1.0);
        let source_vertical_non_content = capture.style.padding.top
            + capture.style.padding.bottom
            + vertical_border_width(&capture.style);
        let source_height = used_content_box_height_or_auto(
            &capture.style,
            layout_pt(self.current_page_context.area_height()),
            non_content_pt(source_vertical_non_content),
        )
        .map(SemanticLengthExt::points)
        .unwrap_or(capture.style.line_height)
            + source_vertical_non_content;
        let replay_height = (available_height + self.current_page_context.area_height())
            .max(source_height)
            .max(capture.style.line_height)
            .max(1.0);
        let replay_context = PageContext {
            size: PageSize::from_points(replay_width, replay_height),
            margins: PageMargins::all_points(0.0),
            edges: PageBoxEdges::ZERO,
            rotation: 0,
        };

        self.pages.clear();
        self.page_names.clear();
        self.page_blanks.clear();
        self.page_named_strings.clear();
        self.page_running_elements.clear();
        self.page_anchors.clear();
        self.page_anchor_source_positions.clear();
        self.page_anchor_text.clear();
        self.document_canvas_background = None;
        self.document_canvas_fragment_insets.clear();
        self.current_page = page_for_context(replay_context);
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.current_page_name = None;
        self.current_page_selected_name = None;
        self.apply_page_context(
            replay_context,
            FragmentOffsets {
                left: 0.0,
                right: 0.0,
                top: 0.0,
            },
        );
        self.inline_static_position = None;
        self.block_static_position_y_offset = None;
        self.fragment_top_offsets.clear();
        self.block_percentage_context_stack.clear();
        self.truncate_page_start_margins = false;
        self.avoid_inside_retry_depth = 0;
        self.containing_blocks.clear();
        self.fixed_containing_blocks.clear();
        self.counter_set = capture.counter_set.clone();
        self.quote_depth = capture.quote_depth;
        self.current_page_named_strings.clear();
        self.current_page_running_elements.clear();
        self.ancestors.clear();
        self.bookmarks.clear();
        self.positioned_layers.clear();
        self.fixed_layers.clear();
        self.overflow_clips.clear();
        self.next_float_id = 1;
        self.float_contexts = vec![FloatContext { shapes: Vec::new() }];
        self.active_auto_float_measurements.clear();
        self.active_auto_float_measurement_fallbacks.clear();
        self.pending_paint_fragments.clear();
        self.pending_page_side_effects.clear();
        self.preserve_scoped_paint_public_order = false;

        let mut replay_style = (*capture.style).clone();
        replay_style.position = css::Position::Static;
        replay_style.break_before = PageBreak::Auto;
        replay_style.break_after = PageBreak::Auto;
        replay_style.page = css::PageAssignment::Unspecified;
        replay_style.counter_resets.clear();
        replay_style.counter_increments.clear();
        replay_style.counter_sets.clear();

        let stylesheets = self.stylesheets;
        let root_signature = element_signature(&capture.element);
        self.ancestors.push(root_signature.clone());
        let child_boxes = self.build_frozen_child_boxes_with_font_metrics(
            &capture.element,
            &stylesheets,
            &replay_style,
            &[root_signature],
        );
        self.layout_element_with_child_boxes(
            &capture.element,
            &replay_style,
            &stylesheets,
            Some(&child_boxes),
        );
        self.flush_positioned_layers_since(0);
        self.apply_pending_fragments_for_current_page();

        let current_fragment = self.current_page.take_paint_fragment();
        let fragment = if !current_fragment.is_empty() {
            current_fragment
        } else if let Some(page) = self.pages.first_mut() {
            page.take_paint_fragment()
        } else {
            PaintFragment::from_primitives(Vec::new(), Vec::new())
        };
        let bounds = fragment.bounds();
        self.restore(snapshot);
        let bounds = bounds?;
        let width = bounds.width().max(0.0);
        let height = bounds.height().max(0.0);
        let fragment = fragment.translated(PaintTranslation::new(-bounds.x(), -bounds.y()));
        let mut atom_style = (*capture.style).clone();
        // The embedded replay fragment is an ordinary generated-content atom;
        // only the source element has `position: running()` semantics.
        atom_style.position = css::Position::Static;
        atom_style.background.background_color = css::BackgroundColor::TRANSPARENT;
        atom_style.background.background_image = css::ComputedImage::None;
        atom_style.background.background_layers.clear();
        atom_style.border_width = 0.0;
        atom_style.border_widths = css::Edges::ZERO;
        atom_style.border_styles = css::BorderStyles::NONE;
        atom_style.border_image = css::BorderImage::initial();
        Some(InlineItem::Atom(Box::new(InlineAtom::new(
            InlineAtomContent::InlineFragment {
                fragment: Box::new(fragment),
                replay_coordinates: AtomicInlineFragmentReplayCoordinates::border_box_local(),
                table_cell_context: None,
                contents_overflow_clip_applied: false,
            },
            atom_style,
            None,
            InlineSize::new(width, height),
            height,
            0.0,
            None,
            None,
        ))))
    }
}
