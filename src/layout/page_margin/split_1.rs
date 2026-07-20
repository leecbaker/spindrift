use super::*;
use crate::units::LayoutSize;

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
                        rule.layer_order,
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
            for box_ in &mut boxes {
                let ch_advance =
                    self.ch_advance_for_style(&box_.style, box_.style.requires_ch_advance());
                box_.style.resolve_font_metric_lengths(ch_advance);
            }
            let page_size = css::page_size_from_with_ch_advance(
                &page_declarations,
                base_page_context.size,
                page_ch_advance,
            );
            let page_edges = super::builder::page_box_edges_from_declarations_with_ch_advance(
                &page_declarations,
                page_size,
                page_ch_advance,
            );
            let page_margins =
                css::page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style(
                    &page_declarations,
                    base_page_context.margins,
                    page_size,
                    self.page_descriptor_viewport_size,
                    page_edges.total(),
                    page_ch_advance,
                    &page_style,
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
            .fixed_box_physical_inline_extent(style)
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
        self.page_anchor_text.clear();
        self.document_canvas_background = None;
        self.document_canvas_fragment_insets.clear();
        self.current_page = super::builder::page_for_context(replay_context);
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.current_page_name = None;
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
        self.definite_block_size_stack.clear();
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
        replay_style.running_element_name = None;
        replay_style.break_before = PageBreak::Auto;
        replay_style.break_after = PageBreak::Auto;
        replay_style.page_name_specified = false;
        replay_style.page_name = None;
        replay_style.counter_resets.clear();
        replay_style.counter_increments.clear();
        replay_style.counter_sets.clear();

        let stylesheets = self.stylesheets;
        let root_signature = element_signature(&capture.element);
        self.ancestors.push(root_signature.clone());
        let child_boxes = self.build_frozen_child_boxes_with_font_metrics(
            &capture.element,
            stylesheets,
            &replay_style,
            &[root_signature],
        );
        self.layout_element_with_child_boxes(
            &capture.element,
            &replay_style,
            stylesheets,
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
        atom_style.background_color = None;
        atom_style.background_image = css::ComputedImage::None;
        atom_style.background_layers.clear();
        atom_style.border_width = 0.0;
        atom_style.border_widths = css::Edges::ZERO;
        atom_style.border_styles = css::BorderStyles::NONE;
        atom_style.border_image = css::BorderImage::initial();
        Some(InlineItem::Atom(Box::new(InlineAtom::new(
            InlineAtomContent::InlineFragment {
                fragment: Box::new(fragment),
                table_cell_context: None,
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

pub(in crate::layout) fn page_counter_values_for_pages(
    total_pages: usize,
    page_rules: &[PageRule],
    page_progression_direction: Direction,
    fallback: &Declarations,
    page_names: &[Option<String>],
    page_blanks: &[bool],
    initial_values: &HashMap<String, i32>,
) -> Vec<HashMap<String, i32>> {
    // Page-associated counters advance independently in each named page
    // group. Leaving a named group and later returning to the unnamed group
    // resumes that group's counter scope instead of importing resets from the
    // intervening group.
    // <https://www.w3.org/TR/css-page-3/#page-based-counters>
    let mut counters_by_page_name: HashMap<Option<String>, HashMap<String, i32>> = HashMap::new();
    // `page` is the predefined document-wide page counter. Named page groups
    // scope ordinary page-associated counters, but do not restart page
    // numbering when the selected page name changes.
    let mut page_counter = initial_values.get("page").cloned().unwrap_or(0);
    let mut values = Vec::with_capacity(total_pages);
    for page_index in 0..total_pages {
        let page_number = page_index + 1;
        let page_name = page_names.get(page_index).and_then(Option::as_deref);
        let is_blank = page_blanks.get(page_index).cloned().unwrap_or(false);
        let declarations = page_declarations_for_rules(
            page_rules,
            page_number,
            page_name,
            is_blank,
            page_progression_direction,
            fallback,
        );
        let counters = counters_by_page_name
            .entry(page_name.map(str::to_string))
            .or_insert_with(|| initial_values.clone());
        counters.insert("page".to_string(), page_counter);
        apply_page_counter_declarations(counters, &declarations);
        page_counter = counters.get("page").cloned().unwrap_or(page_counter);
        values.push(counters.clone());
    }
    values
}

/// Applies page-context counter operations in reset, increment, then set order.
///
/// CSS Lists defines counter reset/increment/set effects for generated
/// counters, and CSS Paged Media exposes the resulting page-context counters
/// to page-margin generated content:
/// <https://www.w3.org/TR/css-lists-3/#auto-numbering> and
/// <https://www.w3.org/TR/css-page-3/#page-based-counters>.
pub(in crate::layout) fn apply_page_counter_declarations(
    counters: &mut HashMap<String, i32>,
    declarations: &Declarations,
) {
    let mut style = ComputedStyle::initial();
    css::apply_declarations(&mut style, declarations);
    for reset in style.counter_resets {
        counters.insert(
            reset.name,
            reset
                .kind
                .explicit_value()
                .unwrap_or(CounterValue::ZERO)
                .get(),
        );
    }
    let explicitly_increments_page = style
        .counter_increments
        .iter()
        .any(|change| change.name.eq_ignore_ascii_case("page"));
    if !explicitly_increments_page {
        // The page counter automatically advances once for every generated
        // page unless the page context explicitly supplies its increment.
        // <https://www.w3.org/TR/css-page-3/#page-based-counters>
        *counters.entry("page".to_string()).or_insert(0) += 1;
    }
    for change in style.counter_increments {
        let current = counters.entry(change.name).or_insert(0);
        *current = current.saturating_add(change.value.get());
    }
    for change in style.counter_sets {
        counters.insert(change.name, change.value.get());
    }
}

/// Apply the counter scope established by one generated page-margin box.
///
/// Page-margin boxes establish counter scopes just like ordinary generated
/// boxes. Their reset, increment, and set operations obscure the page-context
/// values only while resolving that box's generated content:
/// <https://www.w3.org/TR/css-page-3/#page-based-counters>.
pub(in crate::layout) fn apply_page_margin_box_counter_scope(
    counters: &mut HashMap<String, i32>,
    style: &ComputedStyle,
) {
    for reset in &style.counter_resets {
        counters.insert(
            reset.name.clone(),
            reset
                .kind
                .explicit_value()
                .unwrap_or(CounterValue::ZERO)
                .get(),
        );
    }
    for change in &style.counter_increments {
        let current = counters.entry(change.name.clone()).or_insert(0);
        *current = current.saturating_add(change.value.get());
    }
    for change in &style.counter_sets {
        counters.insert(change.name.clone(), change.value.get());
    }
}

pub(in crate::layout) fn page_declarations_for_rules(
    page_rules: &[PageRule],
    page_number: usize,
    page_name: Option<&str>,
    is_blank: bool,
    page_progression_direction: Direction,
    fallback: &Declarations,
) -> Declarations {
    let mut declarations = fallback.clone();
    declarations.extend(cascade_page_rule_declarations(
        page_rules.iter().filter_map(|rule| {
            rule.matching_specificity(page_number, page_name, is_blank, page_progression_direction)
                .map(|specificity| {
                    (
                        rule.origin,
                        specificity,
                        rule.layer_order,
                        rule.order,
                        &rule.declarations,
                    )
                })
        }),
    ));
    declarations
}

/// Cascades the `@footnote` page-area declarations for one generated page.
///
/// GCPM defines the footnote area in the page context, but it is not one of
/// CSS Paged Media's margin boxes. It therefore shares page-selector, origin,
/// and layer precedence while retaining a separate declaration stream for the
/// footnote layout phase:
/// <https://www.w3.org/TR/css-gcpm-3/#footnote-area> and
/// <https://www.w3.org/TR/css-page-3/#cascading-in-the-page-context>.
pub(in crate::layout) fn page_footnote_area_declarations_for_rules(
    page_rules: &[PageRule],
    page_number: usize,
    page_name: Option<&str>,
    is_blank: bool,
    page_progression_direction: Direction,
) -> Declarations {
    cascade_page_rule_declarations(page_rules.iter().filter_map(|rule| {
        rule.matching_specificity(page_number, page_name, is_blank, page_progression_direction)
            .and_then(|specificity| {
                rule.footnote_area.as_ref().map(|declarations| {
                    (
                        rule.origin,
                        specificity,
                        rule.layer_order,
                        rule.order,
                        declarations,
                    )
                })
            })
    }))
}

pub(in crate::layout) struct PageMarginCascadeContext<'a> {
    pub(in crate::layout) page_rules: &'a [PageRule],
    pub(in crate::layout) page_number: usize,
    pub(in crate::layout) page_name: Option<&'a str>,
    pub(in crate::layout) is_blank: bool,
    pub(in crate::layout) page_progression_direction: Direction,
    pub(in crate::layout) page_declarations: &'a Declarations,
    pub(in crate::layout) base_page_style: &'a ComputedStyle,
    /// The default page box, which is the viewport for page descriptors and
    /// page-margin boxes. It is deliberately not the page area produced by
    /// the active `@page` rule.
    ///
    /// CSS Paged Media resolves viewport units in the page and page-margin
    /// contexts against the default page box:
    /// <https://www.w3.org/TR/css-page-3/#page-model>.
    pub(in crate::layout) initial_page_size: PageSize,
}

pub(in crate::layout) fn page_margin_boxes_for_rules(
    context: PageMarginCascadeContext<'_>,
) -> Vec<PageMarginBoxSpec> {
    let mut boxes = Vec::new();
    for name in PAGE_MARGIN_BOX_NAMES {
        let page_specific_declarations =
            cascade_page_rule_declarations(context.page_rules.iter().filter_map(|rule| {
                let specificity = rule.matching_specificity(
                    context.page_number,
                    context.page_name,
                    context.is_blank,
                    context.page_progression_direction,
                )?;
                rule.margin_boxes.get(*name).map(|declarations| {
                    (
                        rule.origin,
                        specificity,
                        rule.layer_order,
                        rule.order,
                        declarations,
                    )
                })
            }));
        let declarations = applicable_page_margin_box_declarations(page_specific_declarations);
        if declarations.is_empty() {
            continue;
        }
        // CSS Paged Media 3 gives page-margin boxes normal cascade/inheritance
        // from the page context. The page context starts from the resolved
        // document render options so inherited typography such as the root
        // font-size is visible when @page omits an explicit value.
        // https://www.w3.org/TR/css-page-3/#cascading-and-page-context
        let initial_viewport = LayoutSize::new(
            context.initial_page_size.width(),
            context.initial_page_size.height(),
        );
        let mut page_style = context.base_page_style.clone();
        css::apply_declarations(&mut page_style, context.page_declarations);
        page_style.resolve_deferred_font_size_with_viewport(
            css::FontRelativeLengthBasis::new(
                layout_pt(context.base_page_style.font_size),
                layout_pt(0.0),
            ),
            initial_viewport,
        );
        page_style
            .quotes
            .resolve_auto_language(page_style.language.as_deref());
        let mut style = page_margin_style_inheriting_page_context(&page_style);
        apply_page_margin_box_ua_defaults(&mut style, name);
        css::apply_declarations(&mut style, &declarations);
        // Viewport-relative lengths in a page-margin rule use the initial
        // page box, rather than the page area or the page size authored by
        // this `@page` rule. Resolving them while building the page-margin
        // context also keeps empty generated boxes' background geometry
        // independent from whether they have inline content.
        // https://www.w3.org/TR/css-page-3/#page-model
        if declarations.get("font").is_some() || declarations.get("font-size").is_some() {
            style.resolve_deferred_font_size_with_viewport(
                css::FontRelativeLengthBasis::new(layout_pt(page_style.font_size), layout_pt(0.0)),
                initial_viewport,
            );
        }
        style.resolve_viewport_lengths_for_viewport(initial_viewport);
        // Page-margin boxes are not represented in the ordinary box tree, so
        // they do not pass through the normal deferred font-metric traversal.
        // Their ordinary `em`/`rem` box-model values must nevertheless be
        // finalized before the CSS Page sizing equations inspect width,
        // height, margins, padding, or backgrounds.
        // https://www.w3.org/TR/css-page-3/#cascading-and-page-context
        // https://www.w3.org/TR/css-values-4/#font-relative-lengths
        style
            .line_height_value
            .resolve_em_relative_lengths(layout_pt(style.font_size));
        let (line_height, multiplier, is_normal) =
            style.line_height_value.clone().projected(style.font_size);
        style.line_height = line_height;
        style.line_height_multiplier = multiplier;
        style.line_height_is_normal = is_normal;
        style.finalize_computed_font_relative_lengths();
        finalize_page_margin_text_decoration_style(&mut style);
        style
            .quotes
            .resolve_auto_language(page_style.language.as_deref());
        boxes.push(PageMarginBoxSpec {
            name: (*name).to_string(),
            declarations,
            style,
        });
    }
    boxes
}

/// Filters declarations to the property set that CSS Paged Media permits in a
/// page-margin at-rule.
///
/// Page-margin boxes are generated boxes, not ordinary element boxes. In
/// particular, formatting-context, positioning, transform, and fragmentation
/// properties cannot change their generated geometry or establish an extra
/// paint context. CSS Paged Media instead gives margin boxes the page-margin
/// property set and a fixed page-margin tree position:
/// <https://www.w3.org/TR/css-page-3/#margin-at-rules>.
fn applicable_page_margin_box_declarations(declarations: Declarations) -> Declarations {
    let base_url = declarations.base_url().cloned();
    let root_url = declarations.root_url().cloned();
    declarations
        .iter()
        .filter(|(name, _)| page_margin_box_property_applies(name))
        .cloned()
        .collect::<Declarations>()
        .with_urls(base_url.as_ref(), root_url.as_ref())
}

/// Whether an authored declaration belongs to CSS Paged Media's page-margin
/// property set.
///
/// Custom properties remain available because an applicable declaration may
/// reference them through `var()`. The excluded properties are the layout
/// mechanisms that would otherwise make a generated margin box behave like an
/// independently positioned principal box.
fn page_margin_box_property_applies(name: &str) -> bool {
    if name.starts_with("--") {
        return true;
    }

    !matches!(
        name,
        "display"
            | "position"
            | "top"
            | "right"
            | "bottom"
            | "left"
            | "inset"
            | "inset-block"
            | "inset-inline"
            | "inset-block-start"
            | "inset-block-end"
            | "inset-inline-start"
            | "inset-inline-end"
            | "float"
            | "clear"
            | "columns"
            | "column-count"
            | "column-width"
            | "column-gap"
            | "column-rule"
            | "column-rule-color"
            | "column-rule-style"
            | "column-rule-width"
            | "column-fill"
            | "column-span"
            | "break-before"
            | "break-after"
            | "break-inside"
            | "page-break-before"
            | "page-break-after"
            | "page-break-inside"
            | "orphans"
            | "widows"
            | "transform"
            | "transform-origin"
            | "translate"
            | "rotate"
            | "scale"
            | "perspective"
            | "perspective-origin"
            | "transform-style"
            | "backface-visibility"
    )
}

/// Applies the embedded HTML UA defaults for page-margin box alignment.
///
/// CSS Paged Media defines page-margin boxes as generated boxes in fixed
/// regions around the page area, and the UA stylesheet supplies their default
/// horizontal and vertical alignment:
/// <https://www.w3.org/TR/css-page-3/#page-margin-boxes>.
pub(in crate::layout) fn apply_page_margin_box_ua_defaults(style: &mut ComputedStyle, name: &str) {
    match name {
        "top-left-corner" | "top-right" | "bottom-left-corner" | "bottom-right" => {
            style.text_align = TextAlign::Right;
        }
        "top-center" | "left-top" | "left-middle" | "left-bottom" | "right-top"
        | "right-middle" | "right-bottom" | "bottom-center" => {
            style.text_align = TextAlign::Center;
        }
        _ => {
            style.text_align = TextAlign::Left;
        }
    }
    style.vertical_align = match name {
        "left-top" | "right-top" => VerticalAlign::BASELINE.with_baseline_shift(BaselineShift::Top),
        "left-bottom" | "right-bottom" => {
            VerticalAlign::BASELINE.with_baseline_shift(BaselineShift::Bottom)
        }
        _ => VerticalAlign::BASELINE
            .with_alignment_baseline(AlignmentBaseline::Metric(BaselineMetric::Middle)),
    };
}

/// Builds the initial page-margin style from inherited page-context values.
///
/// CSS Paged Media says page-margin boxes inherit from the page context, but
/// page-context declarations such as `margin` and `size` do not become
/// margin-box margins or dimensions:
/// <https://www.w3.org/TR/css-page-3/#page-properties>.
pub(in crate::layout) fn page_margin_style_inheriting_page_context(
    page_style: &ComputedStyle,
) -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    style.custom_properties = page_style.custom_properties.clone();
    style.color = page_style.color;
    style.text_align = page_style.text_align;
    style.text_align_last = page_style.text_align_last;
    style.text_justify = page_style.text_justify;
    style.direction = page_style.direction;
    style.writing_mode = page_style.writing_mode;
    style.font_style = page_style.font_style;
    style.font_width = page_style.font_width;
    style.font_family = page_style.font_family.clone();
    style.language = page_style.language.clone();
    style.line_height_value = page_style.line_height_value.clone();
    style.line_height_multiplier = page_style.line_height_multiplier;
    style.line_height_is_normal = page_style.line_height_is_normal;
    style.word_spacing = page_style.word_spacing.clone();
    style.text_transform = page_style.text_transform;
    style.tab_size = page_style.tab_size.clone();
    style.text_decoration_layers = page_style.text_decoration_layers.clone();
    style.text_decoration.underline_position = page_style.text_decoration.underline_position;
    style.text_shadow = page_style.text_shadow.clone();
    style.text_emphasis_style = page_style.text_emphasis_style.clone();
    style.text_emphasis_color = page_style.text_emphasis_color;
    style.text_emphasis_position = page_style.text_emphasis_position;
    style.text_emphasis_skip = page_style.text_emphasis_skip;
    style.white_space = page_style.white_space;
    style.word_break = page_style.word_break;
    style.overflow_wrap = page_style.overflow_wrap;
    style.line_break = page_style.line_break;
    style.hyphens = page_style.hyphens;
    style.hyphenate_character = page_style.hyphenate_character.clone();
    style.hyphenate_limit_chars = page_style.hyphenate_limit_chars;
    style.visibility = page_style.visibility;
    style.orphans = page_style.orphans;
    style.widows = page_style.widows;
    style.list_style_type = page_style.list_style_type.clone();
    style.list_style_position = page_style.list_style_position;
    style.list_style_image = page_style.list_style_image.clone();
    style.quotes = page_style.quotes.clone();
    style.font_size = page_style.font_size;
    style.root_font_size = page_style.root_font_size;
    style.line_height = page_style.line_height;
    style.font_weight = page_style.font_weight;
    style.border_collapse = page_style.border_collapse;
    style.caption_side = page_style.caption_side;
    style.empty_cells = page_style.empty_cells;
    style.border_spacing = page_style.border_spacing.clone();
    style.border_spacing_explicit = page_style.border_spacing_explicit;
    style
}

pub(in crate::layout) fn finalize_page_margin_text_decoration_style(style: &mut ComputedStyle) {
    if style.text_emphasis_color.is_none() {
        style.text_emphasis_color = Some(style.color);
    }
    if style.text_decoration.clone().has_visible_line() {
        let mut decoration = style.text_decoration.clone();
        decoration.color.get_or_insert(style.color);
        style.text_decoration_layers.push(decoration);
    }
}

pub(in crate::layout) fn page_context_style_from_options(options: &RenderOptions) -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    style.font_size = options.font_size();
    style.line_height_value = css::ComputedLineHeight::from_points(options.line_height());
    style.line_height = options.line_height();
    style.line_height_multiplier = None;
    style.line_height_is_normal = false;
    style
}

pub(in crate::layout) fn cascade_page_rule_declarations<'a>(
    declarations: impl Iterator<
        Item = (
            StylesheetOrigin,
            PageSpecificity,
            Option<usize>,
            usize,
            &'a Declarations,
        ),
    >,
) -> Declarations {
    let mut candidates = Vec::new();
    let mut declaration_order = 0usize;
    for (origin, specificity, layer_order, rule_order, declarations) in declarations {
        for (name, value) in declarations {
            let important = page_declaration_is_important(value);
            let candidate_key = PageCascadeKey {
                important,
                origin,
                origin_rank: css::origin_importance_rank(origin, important),
                layer_order,
                layer_rank: page_layer_precedence_rank(layer_order, important),
                specificity,
                rule_order,
                declaration_order,
            };
            declaration_order += 1;
            candidates.push(PageCascadedDeclaration {
                name: name.clone(),
                value: value.clone(),
                key: candidate_key,
            });
        }
    }
    candidates.sort_by_key(|candidate| candidate.key);

    let mut active: Vec<PageCascadedDeclaration> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if page_declaration_is_revert(&candidate.value) {
            active.retain(|existing| {
                !css::declarations_affect_same_property(&existing.name, &candidate.name)
                    || !same_or_stronger_reverted_page_origin(existing, &candidate)
            });
        } else if page_declaration_is_revert_layer(&candidate.value) {
            active.retain(|existing| {
                !css::declarations_affect_same_property(&existing.name, &candidate.name)
                    || !same_page_cascade_layer(existing, &candidate)
            });
        } else {
            active.push(candidate);
        }
    }

    let mut winners: Vec<(String, String, PageCascadeKey)> = Vec::new();
    for candidate in active {
        if let Some(existing) = winners
            .iter_mut()
            .find(|(existing_name, _, _)| existing_name == &candidate.name)
        {
            *existing = (candidate.name, candidate.value, candidate.key);
        } else {
            winners.push((candidate.name, candidate.value, candidate.key));
        }
    }
    winners
        .into_iter()
        .map(|(name, value, _)| (name, value))
        .collect()
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct PageCascadedDeclaration {
    pub(in crate::layout) name: String,
    pub(in crate::layout) value: String,
    pub(in crate::layout) key: PageCascadeKey,
}

/// Cascading key for page-context declarations.
///
/// CSS Paged Media adds page-selector specificity to page rules; CSS Cascade
/// Level 5 sorts importance and layers before selector specificity:
/// <https://www.w3.org/TR/css-page-3/#cascading-and-page-context> and
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::layout) struct PageCascadeKey {
    pub(in crate::layout) important: bool,
    pub(in crate::layout) origin_rank: u8,
    pub(in crate::layout) layer_rank: usize,
    pub(in crate::layout) specificity: PageSpecificity,
    pub(in crate::layout) rule_order: usize,
    pub(in crate::layout) declaration_order: usize,
    pub(in crate::layout) origin: StylesheetOrigin,
    pub(in crate::layout) layer_order: Option<usize>,
}

pub(in crate::layout) fn page_declaration_is_important(value: &str) -> bool {
    value
        .trim_end()
        .to_ascii_lowercase()
        .ends_with("!important")
}

pub(in crate::layout) fn page_declaration_is_revert_layer(value: &str) -> bool {
    css::trim_css_value(value).eq_ignore_ascii_case("revert-layer")
}

pub(in crate::layout) fn page_declaration_is_revert(value: &str) -> bool {
    css::trim_css_value(value).eq_ignore_ascii_case("revert")
}

/// Returns whether a prior page-margin declaration is erased by `revert`.
///
/// Page-margin boxes inherit page-context cascade mechanics from CSS Paged
/// Media, and CSS Cascade Level 5 defines `revert` by rolling back origins:
/// <https://www.w3.org/TR/css-page-3/#margin-at-rules> and
/// <https://www.w3.org/TR/css-cascade-5/#revert>.
pub(in crate::layout) fn same_or_stronger_reverted_page_origin(
    prior: &PageCascadedDeclaration,
    rollback: &PageCascadedDeclaration,
) -> bool {
    match rollback.key.origin {
        StylesheetOrigin::Author => prior.key.origin == StylesheetOrigin::Author,
        StylesheetOrigin::User => {
            matches!(
                prior.key.origin,
                StylesheetOrigin::User | StylesheetOrigin::Author
            )
        }
        StylesheetOrigin::UserAgent => prior.key.origin == StylesheetOrigin::UserAgent,
    }
}

pub(in crate::layout) fn same_page_cascade_layer(
    left: &PageCascadedDeclaration,
    right: &PageCascadedDeclaration,
) -> bool {
    left.key.origin == right.key.origin
        && left.key.important == right.key.important
        && left.key.layer_order == right.key.layer_order
}

pub(in crate::layout) fn page_layer_precedence_rank(
    layer_order: Option<usize>,
    important: bool,
) -> usize {
    match (important, layer_order) {
        (false, Some(order)) => order,
        (false, None) => usize::MAX,
        (true, None) => 0,
        (true, Some(order)) => usize::MAX.saturating_sub(1).saturating_sub(order),
    }
}

pub(in crate::layout) const PAGE_MARGIN_BOX_NAMES: &[&str] = &[
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

pub(in crate::layout) struct PageMarginBoxSpec {
    pub(in crate::layout) name: String,
    pub(in crate::layout) declarations: Declarations,
    pub(in crate::layout) style: ComputedStyle,
}

pub(in crate::layout) struct PageMarginBoxLayout<'a> {
    pub(in crate::layout) spec: &'a PageMarginBoxSpec,
    pub(in crate::layout) content: ResolvedPageContent,
    /// Border box of a CSS page-margin box in page-local paint coordinates.
    ///
    /// CSS Paged Media defines generated page-margin boxes around the page
    /// area. At this point their used rectangles have already been projected
    /// into Quire paint space: origin at the page bottom-left, `x` increasing
    /// rightward, and `y` increasing upward:
    /// <https://www.w3.org/TR/css-page-3/#page-margin-boxes>.
    pub(in crate::layout) border_rect: PaintRect,
    /// Content box of a CSS page-margin box in page-local paint coordinates.
    ///
    /// This is the containing area for generated margin-box inline content
    /// after margin, border, and padding have been applied according to the CSS
    /// box model:
    /// <https://www.w3.org/TR/CSS22/box.html#box-dimensions>.
    pub(in crate::layout) content_rect: PaintRect,
}

impl PageMarginBoxLayout<'_> {
    pub(in crate::layout) fn border_clip(&self) -> PaintClip {
        PaintClip::from_paint_rect(self.border_rect)
    }

    pub(in crate::layout) fn content_x(&self) -> f32 {
        self.content_rect.min_x()
    }

    pub(in crate::layout) fn content_y(&self) -> f32 {
        self.content_rect.min_y()
    }

    pub(in crate::layout) fn content_width(&self) -> f32 {
        self.content_rect.width()
    }

    pub(in crate::layout) fn content_height(&self) -> f32 {
        self.content_rect.height()
    }
}

pub(in crate::layout) struct PageMarginPaintedBox {
    pub(in crate::layout) z_index: i32,
    pub(in crate::layout) order: usize,
    pub(in crate::layout) effects: PaintEffects,
    pub(in crate::layout) bounds: PaintClip,
    pub(in crate::layout) fragment: PaintFragment,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Css;

    fn cascaded_top_left(source: &str, page_number: usize) -> PageMarginBoxSpec {
        let stylesheet = css::parse_stylesheet(&Css::from_string(source));
        page_margin_boxes_for_rules(PageMarginCascadeContext {
            page_rules: &stylesheet.page_rules,
            page_number,
            page_name: None,
            is_blank: false,
            page_progression_direction: Direction::Ltr,
            page_declarations: &stylesheet.page_declarations,
            base_page_style: &ComputedStyle::initial(),
            initial_page_size: PageSize::A4_POINTS,
        })
        .into_iter()
        .find(|box_| box_.name == "top-left")
        .expect("@top-left declaration should produce a page-margin box")
    }

    #[test]
    fn page_margin_boxes_cascade_by_page_selector_specificity() {
        let first = cascaded_top_left(
            "@page { @top-left { content: \"base\"; color: red } }\
             @page :right { @top-left { content: \"right\" } }\
             @page :first { @top-left { content: \"first\"; color: blue } }",
            1,
        );
        let right = cascaded_top_left(
            "@page { @top-left { content: \"base\"; color: red } }\
             @page :right { @top-left { content: \"right\" } }\
             @page :first { @top-left { content: \"first\"; color: blue } }",
            3,
        );

        assert_eq!(
            first.declarations.get("content").map(String::as_str),
            Some("\"first\"")
        );
        assert_eq!(
            first.declarations.get("color").map(String::as_str),
            Some("blue")
        );
        assert_eq!(
            right.declarations.get("content").map(String::as_str),
            Some("\"right\"")
        );
        assert_eq!(
            right.declarations.get("color").map(String::as_str),
            Some("red")
        );
    }

    #[test]
    fn page_margin_box_margin_auto_survives_the_applicability_filter() {
        let box_ = cascaded_top_left("@page { @top-left { content: \"\"; margin: auto } }", 1);

        assert!(matches!(
            box_.style.box_values.margin.left,
            css::ComputedLengthPercentageOrAuto::Auto
        ));
        assert!(matches!(
            box_.style.box_values.margin.right,
            css::ComputedLengthPercentageOrAuto::Auto
        ));
    }

    #[test]
    fn fixed_corner_axis_centers_retained_auto_margins() {
        let stylesheet = css::parse_stylesheet(&Css::from_string(
            "@page { @top-left-corner { content: \"\"; width: 25px; margin: auto } }",
        ));
        let box_ = page_margin_boxes_for_rules(PageMarginCascadeContext {
            page_rules: &stylesheet.page_rules,
            page_number: 1,
            page_name: None,
            is_blank: false,
            page_progression_direction: Direction::Ltr,
            page_declarations: &stylesheet.page_declarations,
            base_page_style: &ComputedStyle::initial(),
            initial_page_size: PageSize::A4_POINTS,
        })
        .into_iter()
        .find(|box_| box_.name == "top-left-corner")
        .expect("corner should be generated");
        let edges = fixed_width_axis(
            &box_,
            75.0,
            PercentageBasis::definite(layout_pt(75.0)),
            VerticalPageMarginSide::Left,
        );

        assert_eq!(edges.margin.left.points(), edges.margin.right.points());
        assert!(edges.margin.left.points() > 0.0);
    }

    #[test]
    fn vertical_edge_fit_content_centers_auto_cross_axis_margins() {
        let stylesheet = css::parse_stylesheet(&Css::from_string(
            "@page { @right-top { content: \"xxx\\a x\"; writing-mode: vertical-rl; margin: auto; block-size: fit-content } }",
        ));
        let box_ = page_margin_boxes_for_rules(PageMarginCascadeContext {
            page_rules: &stylesheet.page_rules,
            page_number: 1,
            page_name: None,
            is_blank: false,
            page_progression_direction: Direction::Ltr,
            page_declarations: &stylesheet.page_declarations,
            base_page_style: &ComputedStyle::initial(),
            initial_page_size: PageSize::A4_POINTS,
        })
        .into_iter()
        .find(|box_| box_.name == "right-top")
        .expect("right edge should be generated");
        assert!(matches!(
            box_.style.box_values.width,
            css::ComputedLengthPercentageOrAuto::FitContent(_)
        ));
        let edges = fixed_width_axis(
            &box_,
            72.0,
            PercentageBasis::definite(layout_pt(192.0)),
            VerticalPageMarginSide::Right,
        );

        assert_eq!(edges.margin.left.points(), edges.margin.right.points());
    }

    #[test]
    fn page_margin_boxes_honor_cascade_layers_and_revert_layer() {
        let box_ = cascaded_top_left(
            "@layer base, theme;\
             @layer base { @page { @top-left { content: \"base\" } } }\
             @layer theme { @page { @top-left { content: \"theme\"; content: revert-layer } } }",
            1,
        );

        assert_eq!(
            box_.declarations.get("content").map(String::as_str),
            Some("\"base\"")
        );
    }

    #[test]
    fn page_margin_boxes_finalize_font_relative_box_model_lengths() {
        let box_ = cascaded_top_left(
            "@page { font-size: 12pt; @top-left { content: \"\"; width: 5em; margin: -2em } }",
            1,
        );
        let width =
            used_content_box_width_or_auto(&box_.style, layout_pt(100.0), non_content_pt(0.0))
                .expect("an em width must be definite before page-margin sizing");

        assert_eq!(width.points(), 60.0);
        assert_eq!(
            margin_edge_for_page_margin_box(
                box_.style.box_values.margin.left,
                PercentageBasis::definite(layout_pt(100.0)),
            ),
            -24.0
        );
    }

    #[test]
    fn page_margin_box_font_size_sets_its_em_sizing_basis() {
        let box_ = cascaded_top_left(
            "@page { font-size: 12pt; @top-left { content: \"\"; font-size: 2em; width: 5em } }",
            1,
        );
        let width =
            used_content_box_width_or_auto(&box_.style, layout_pt(200.0), non_content_pt(0.0))
                .expect("an em width must be definite before page-margin sizing");

        assert_eq!(box_.style.font_size, 24.0);
        assert_eq!(width.points(), 120.0);
    }

    #[test]
    fn page_margin_boxes_ignore_fragmentation_and_positioning_declarations() {
        let box_ = cascaded_top_left(
            "@page { @top-left { content: \"\"; display: flex; position: absolute; top: 1px; page-break-before: always; width: 10px; z-index: 2 } }",
            1,
        );

        assert!(box_.declarations.get("display").is_none());
        assert!(box_.declarations.get("position").is_none());
        assert!(box_.declarations.get("top").is_none());
        assert!(box_.declarations.get("page-break-before").is_none());
        assert_eq!(
            box_.declarations.get("width").map(String::as_str),
            Some("10px")
        );
        assert_eq!(box_.style.z_index, Some(2));
    }
}
