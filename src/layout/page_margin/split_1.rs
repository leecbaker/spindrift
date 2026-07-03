use super::*;

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
            .copied()
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
        if self.page_margin_boxes.is_empty()
            && self
                .page_rules
                .iter()
                .all(|rule| rule.margin_boxes.is_empty())
        {
            return;
        }
        let total_pages = self.pages.len();
        let page_rules = self.page_rules.clone();
        let fallback_page_declarations = self.page_declarations.clone();
        let fallback_margin_boxes = self.page_margin_boxes.clone();
        let page_named_strings = self.page_named_strings.clone();
        let page_running_elements = self.page_running_elements.clone();
        let page_anchors = self.page_anchors.clone();
        let page_anchor_text = self.page_anchor_text.clone();
        let counter_styles = self.counter_styles.clone();
        let page_progression_direction = self.page_progression_direction;
        let base_page_context = PageContext::from_options(self.options);
        let base_page_style = page_context_style_from_options(self.options);
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
            let is_blank = self.page_blanks.get(index).copied().unwrap_or(false);
            let page_declarations = page_declarations_for_rules(
                &page_rules,
                page_number,
                page_name,
                is_blank,
                page_progression_direction,
                &fallback_page_declarations,
            );
            let boxes = page_margin_boxes_for_rules(PageMarginCascadeContext {
                page_rules: &page_rules,
                page_number,
                page_name,
                is_blank,
                page_progression_direction,
                fallback: &fallback_margin_boxes,
                page_declarations: &page_declarations,
                base_page_style: &base_page_style,
            });
            let page_ch_advance = self.page_ch_advance_for_declarations(&page_declarations);
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
            let page_margins = css::page_margins_from_for_size_and_edges_with_ch_advance(
                &page_declarations,
                base_page_context.margins,
                page_size,
                page_edges.total(),
                page_ch_advance,
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
            };
            let layouts = layout_page_margin_boxes(
                &self.pages[index],
                &boxes,
                context,
                &mut self.font_system,
            );
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
                let available_width = layout.content_width().max(1.0);
                self.page_margin_inline_sequence_with_replay(
                    &layout.content,
                    &layout.spec.style,
                    available_width,
                    layout.content_height().max(layout.spec.style.line_height),
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
                | PageMarginContentItem::TargetText { .. } => {}
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
        let total_height = sequence
            .total_height()
            .max(self.font_system.used_line_height(style));
        let line_top = page_margin_text_stack_top(layout, style.vertical_align, total_height);
        std::mem::swap(&mut self.current_page, &mut self.pages[page_index]);
        self.paint_inline_line_sequence_in_fixed_box(
            sequence,
            style,
            layout.content_x(),
            layout.content_width().max(1.0),
            line_top,
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
        let replay_height = available_height.max(capture.style.line_height).max(1.0);
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
        self.root_canvas_background_defined = false;
        self.current_page = super::builder::page_for_context(replay_context);
        self.current_page_has_flow_content = false;
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
        self.list_stack.clear();
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
        self.pending_float_fragments.clear();
        self.pending_float_side_effects.clear();
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
        self.apply_pending_float_fragments_for_current_page();

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
        let mut atom_style = (*capture.style).clone();
        atom_style.background_color = None;
        atom_style.background_image = None;
        atom_style.background_layers.clear();
        atom_style.border_width = 0.0;
        atom_style.border_widths = css::Edges::ZERO;
        atom_style.border_styles = css::BorderStyles::NONE;
        atom_style.border_image = css::BorderImage::initial();
        Some(InlineItem::Atom(Box::new(InlineAtom::new(
            InlineAtomContent::InlineFragment(fragment),
            atom_style,
            None,
            width,
            height,
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
    let mut counters = initial_values.clone();
    let mut values = Vec::with_capacity(total_pages);
    for page_index in 0..total_pages {
        let page_number = page_index + 1;
        let page_name = page_names.get(page_index).and_then(Option::as_deref);
        let is_blank = page_blanks.get(page_index).copied().unwrap_or(false);
        let declarations = page_declarations_for_rules(
            page_rules,
            page_number,
            page_name,
            is_blank,
            page_progression_direction,
            fallback,
        );
        apply_page_counter_declarations(&mut counters, &declarations);
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
    if declarations.is_empty() {
        return;
    }
    let mut style = ComputedStyle::initial();
    css::apply_declarations(&mut style, declarations);
    for (name, value) in style.counter_resets {
        counters.insert(name, value);
    }
    for (name, amount) in style.counter_increments {
        *counters.entry(name).or_insert(0) += amount;
    }
    for (name, value) in style.counter_sets {
        counters.insert(name, value);
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

pub(in crate::layout) struct PageMarginCascadeContext<'a> {
    pub(in crate::layout) page_rules: &'a [PageRule],
    pub(in crate::layout) page_number: usize,
    pub(in crate::layout) page_name: Option<&'a str>,
    pub(in crate::layout) is_blank: bool,
    pub(in crate::layout) page_progression_direction: Direction,
    pub(in crate::layout) fallback: &'a HashMap<String, Declarations>,
    pub(in crate::layout) page_declarations: &'a Declarations,
    pub(in crate::layout) base_page_style: &'a ComputedStyle,
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
        let mut declarations = context
            .fallback
            .get(*name)
            .cloned()
            .unwrap_or_else(Declarations::new);
        declarations.extend(page_specific_declarations);
        if declarations.is_empty() {
            continue;
        }
        // CSS Paged Media 3 gives page-margin boxes normal cascade/inheritance
        // from the page context. The page context starts from the resolved
        // document render options so inherited typography such as the root
        // font-size is visible when @page omits an explicit value.
        // https://www.w3.org/TR/css-page-3/#cascading-and-page-context
        let mut page_style = context.base_page_style.clone();
        css::apply_declarations(&mut page_style, context.page_declarations);
        page_style
            .quotes
            .resolve_auto_language(page_style.language.as_deref());
        let mut style = page_margin_style_inheriting_page_context(&page_style);
        apply_page_margin_box_ua_defaults(&mut style, name);
        css::apply_declarations(&mut style, &declarations);
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
    style.line_height_value = page_style.line_height_value;
    style.line_height_multiplier = page_style.line_height_multiplier;
    style.line_height_is_normal = page_style.line_height_is_normal;
    style.word_spacing = page_style.word_spacing;
    style.text_transform = page_style.text_transform;
    style.tab_size = page_style.tab_size;
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
    style.hyphenate_limit_chars = page_style.hyphenate_limit_chars;
    style.visibility = page_style.visibility;
    style.orphans = page_style.orphans;
    style.widows = page_style.widows;
    style.list_style_type = page_style.list_style_type.clone();
    style.list_style_position = page_style.list_style_position;
    style.list_style_image = page_style.list_style_image.clone();
    style.list_style_image_base_url = page_style.list_style_image_base_url.clone();
    style.list_style_image_root_url = page_style.list_style_image_root_url.clone();
    style.quotes = page_style.quotes.clone();
    style.font_size = page_style.font_size;
    style.line_height = page_style.line_height;
    style.font_weight = page_style.font_weight;
    style.border_collapse = page_style.border_collapse;
    style.caption_side = page_style.caption_side;
    style.empty_cells = page_style.empty_cells;
    style.border_spacing = page_style.border_spacing;
    style.border_spacing_explicit = page_style.border_spacing_explicit;
    style
}

pub(in crate::layout) fn finalize_page_margin_text_decoration_style(style: &mut ComputedStyle) {
    if style.text_emphasis_color.is_none() {
        style.text_emphasis_color = Some(style.color);
    }
    if style.text_decoration.has_visible_line() {
        let mut decoration = style.text_decoration;
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

    pub(in crate::layout) fn border_x(&self) -> f32 {
        self.border_rect.min_x()
    }

    pub(in crate::layout) fn border_y(&self) -> f32 {
        self.border_rect.min_y()
    }

    pub(in crate::layout) fn border_width(&self) -> f32 {
        self.border_rect.width()
    }

    pub(in crate::layout) fn border_height(&self) -> f32 {
        self.border_rect.height()
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
