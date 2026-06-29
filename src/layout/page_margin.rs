use super::assets::{BackgroundPaintArea, background_images_for_style, paint_effects_for_box};
use super::page_generated::{
    PageContentResolveContext, PageMarginContentItem, ResolvedPageContent,
    resolve_page_content_parts,
};
use super::*;
use crate::layout::inline_collect::normalize_inline_whitespace_items;

mod paint;

use paint::{page_margin_box_paint_order, replay_page_margin_box_fragments};

impl<'a> LayoutBuilder<'a> {
    pub(super) fn page_name_for_number(&self, page_number: usize) -> Option<&str> {
        page_number
            .checked_sub(1)
            .and_then(|index| self.page_names.get(index))
            .and_then(Option::as_deref)
    }

    pub(super) fn page_is_blank_for_number(&self, page_number: usize) -> bool {
        page_number
            .checked_sub(1)
            .and_then(|index| self.page_blanks.get(index))
            .copied()
            .unwrap_or(false)
    }

    pub(super) fn page_declarations_for(&self, page_number: usize) -> Declarations {
        let page_name = self.page_name_for_number(page_number);
        let is_blank = self.page_is_blank_for_number(page_number);
        self.page_declarations_for_page(page_number, page_name, is_blank)
    }

    pub(super) fn page_declarations_for_page(
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

    pub(super) fn add_page_margin_boxes(&mut self) {
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
            let page_size = css::page_size_from(&page_declarations, base_page_context.size);
            let page_edges =
                super::builder::page_box_edges_from_declarations(&page_declarations, page_size);
            let page_margins = css::page_margins_from_for_size_and_edges(
                &page_declarations,
                base_page_context.margins,
                page_size,
                page_edges.total(),
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

    fn paint_page_margin_box_with_replay(
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

    fn page_margin_inline_sequence_with_replay(
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
            }
        }
        (!items.is_empty())
            .then(|| self.collect_inline_line_sequence(items, style, available_width, 0.0, 0.0))
    }

    fn paint_page_margin_inline_sequence(
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
    fn replay_running_element_capture(
        &mut self,
        capture: &RunningElementCapture,
        available_width: f32,
        available_height: f32,
    ) -> Option<InlineItem> {
        let snapshot = self.snapshot();
        let replay_width = available_width.max(1.0);
        let replay_height = available_height.max(capture.style.line_height).max(1.0);
        let replay_context = PageContext {
            size: PageSize {
                width: replay_width,
                height: replay_height,
            },
            margins: PageMargins::all(0.0),
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
        self.inline_static_baseline_y = None;
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
        let root_signature =
            ElementSignature::new(capture.element.tag.clone(), capture.element.attrs.clone());
        self.ancestors.push(root_signature.clone());
        let child_boxes = box_tree::build_child_boxes(
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
        Some(InlineItem::Atom(Box::new(InlineAtom {
            content: InlineAtomContent::InlineFragment(fragment),
            style: (*capture.style).clone(),
            escaped_positioned_layers: None,
            width,
            height,
            baseline_offset: height,
            baseline_shift: 0.0,
            link_target: None,
            alt_text: None,
        })))
    }
}

fn page_counter_values_for_pages(
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
fn apply_page_counter_declarations(
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

fn page_declarations_for_rules(
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

struct PageMarginCascadeContext<'a> {
    page_rules: &'a [PageRule],
    page_number: usize,
    page_name: Option<&'a str>,
    is_blank: bool,
    page_progression_direction: Direction,
    fallback: &'a HashMap<String, Declarations>,
    page_declarations: &'a Declarations,
    base_page_style: &'a ComputedStyle,
}

fn page_margin_boxes_for_rules(context: PageMarginCascadeContext<'_>) -> Vec<PageMarginBoxSpec> {
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
fn apply_page_margin_box_ua_defaults(style: &mut ComputedStyle, name: &str) {
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
fn page_margin_style_inheriting_page_context(page_style: &ComputedStyle) -> ComputedStyle {
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

fn finalize_page_margin_text_decoration_style(style: &mut ComputedStyle) {
    if style.text_emphasis_color.is_none() {
        style.text_emphasis_color = Some(style.color);
    }
    if style.text_decoration.has_visible_line() {
        let mut decoration = style.text_decoration;
        decoration.color.get_or_insert(style.color);
        style.text_decoration_layers.push(decoration);
    }
}

fn page_context_style_from_options(options: &RenderOptions) -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    style.font_size = options.font_size;
    style.line_height_value = css::ComputedLineHeight::Length(options.line_height);
    style.line_height = options.line_height;
    style.line_height_multiplier = None;
    style.line_height_is_normal = false;
    style
}

fn cascade_page_rule_declarations<'a>(
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
struct PageCascadedDeclaration {
    name: String,
    value: String,
    key: PageCascadeKey,
}

/// Cascading key for page-context declarations.
///
/// CSS Paged Media adds page-selector specificity to page rules; CSS Cascade
/// Level 5 sorts importance and layers before selector specificity:
/// <https://www.w3.org/TR/css-page-3/#cascading-and-page-context> and
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PageCascadeKey {
    important: bool,
    origin_rank: u8,
    layer_rank: usize,
    specificity: PageSpecificity,
    rule_order: usize,
    declaration_order: usize,
    origin: StylesheetOrigin,
    layer_order: Option<usize>,
}

fn page_declaration_is_important(value: &str) -> bool {
    value
        .trim_end()
        .to_ascii_lowercase()
        .ends_with("!important")
}

fn page_declaration_is_revert_layer(value: &str) -> bool {
    css::trim_css_value(value).eq_ignore_ascii_case("revert-layer")
}

fn page_declaration_is_revert(value: &str) -> bool {
    css::trim_css_value(value).eq_ignore_ascii_case("revert")
}

/// Returns whether a prior page-margin declaration is erased by `revert`.
///
/// Page-margin boxes inherit page-context cascade mechanics from CSS Paged
/// Media, and CSS Cascade Level 5 defines `revert` by rolling back origins:
/// <https://www.w3.org/TR/css-page-3/#margin-at-rules> and
/// <https://www.w3.org/TR/css-cascade-5/#revert>.
fn same_or_stronger_reverted_page_origin(
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

fn same_page_cascade_layer(
    left: &PageCascadedDeclaration,
    right: &PageCascadedDeclaration,
) -> bool {
    left.key.origin == right.key.origin
        && left.key.important == right.key.important
        && left.key.layer_order == right.key.layer_order
}

fn page_layer_precedence_rank(layer_order: Option<usize>, important: bool) -> usize {
    match (important, layer_order) {
        (false, Some(order)) => order,
        (false, None) => usize::MAX,
        (true, None) => 0,
        (true, Some(order)) => usize::MAX.saturating_sub(1).saturating_sub(order),
    }
}

const PAGE_MARGIN_BOX_NAMES: &[&str] = &[
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

struct PageMarginBoxSpec {
    name: String,
    declarations: Declarations,
    style: ComputedStyle,
}

struct PageMarginBoxLayout<'a> {
    spec: &'a PageMarginBoxSpec,
    content: ResolvedPageContent,
    /// Border box of a CSS page-margin box in page-local paint coordinates.
    ///
    /// CSS Paged Media defines generated page-margin boxes around the page
    /// area. At this point their used rectangles have already been projected
    /// into Quire paint space: origin at the page bottom-left, `x` increasing
    /// rightward, and `y` increasing upward:
    /// <https://www.w3.org/TR/css-page-3/#page-margin-boxes>.
    border_rect: PaintRect,
    /// Content box of a CSS page-margin box in page-local paint coordinates.
    ///
    /// This is the containing area for generated margin-box inline content
    /// after margin, border, and padding have been applied according to the CSS
    /// box model:
    /// <https://www.w3.org/TR/CSS22/box.html#box-dimensions>.
    content_rect: PaintRect,
}

impl PageMarginBoxLayout<'_> {
    fn border_clip(&self) -> PaintClip {
        PaintClip::from_paint_rect(self.border_rect)
    }

    fn border_x(&self) -> f32 {
        self.border_rect.min_x()
    }

    fn border_y(&self) -> f32 {
        self.border_rect.min_y()
    }

    fn border_width(&self) -> f32 {
        self.border_rect.width()
    }

    fn border_height(&self) -> f32 {
        self.border_rect.height()
    }

    fn content_x(&self) -> f32 {
        self.content_rect.min_x()
    }

    fn content_y(&self) -> f32 {
        self.content_rect.min_y()
    }

    fn content_width(&self) -> f32 {
        self.content_rect.width()
    }

    fn content_height(&self) -> f32 {
        self.content_rect.height()
    }
}

struct PageMarginPaintedBox {
    z_index: i32,
    order: usize,
    effects: PaintEffects,
    bounds: PaintClip,
    fragment: PaintFragment,
}

#[derive(Clone, Copy)]
struct PageMarginPaintContext<'a> {
    page_margins: PageMargins,
    page_edges: PageBoxEdges,
    page_number: usize,
    total_pages: usize,
    base_url: Option<&'a std::path::Path>,
    root_url: Option<&'a std::path::Path>,
    resource_cache: &'a ResourceCache,
    page_index: usize,
    page_named_strings: &'a [HashMap<String, Vec<NamedStringAssignment>>],
    page_running_elements: &'a [HashMap<String, Vec<NamedStringAssignment>>],
    page_anchors: &'a HashMap<String, usize>,
    page_anchor_text: &'a HashMap<String, AnchorText>,
    counter_styles: &'a HashMap<String, CounterStyleRule>,
    page_counters: &'a HashMap<String, i32>,
}

/// Computes used page-margin box rectangles for one generated page.
///
/// CSS Paged Media Level 3 defines sixteen margin boxes, generation from the
/// `content` property, coordinated variable dimensions for side triplets, and
/// fixed dimensions in the perpendicular axis:
/// <https://www.w3.org/TR/css-page-3/#margin-boxes> and
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>.
fn layout_page_margin_boxes<'a>(
    page: &Page,
    boxes: &'a [PageMarginBoxSpec],
    context: PageMarginPaintContext<'_>,
    font_system: &mut FontSystem,
) -> Vec<PageMarginBoxLayout<'a>> {
    let mut generated = boxes
        .iter()
        .filter_map(|spec| {
            resolved_margin_box_content(spec, context)
                .map(|content| GeneratedMarginBox { spec, content })
        })
        .collect::<Vec<_>>();
    if generated.is_empty() {
        return Vec::new();
    }
    let page_margins = context.page_margins;
    let page_edges = context.page_edges;
    let page_area_width = (page.width()
        - page_margins.left
        - page_margins.right
        - page_edges.left()
        - page_edges.right())
    .max(0.0);
    let page_area_height = (page.height()
        - page_margins.top
        - page_margins.bottom
        - page_edges.top()
        - page_edges.bottom())
    .max(0.0);
    let available_width = page_area_width + page_edges.left() + page_edges.right();
    let available_height = page_area_height + page_edges.top() + page_edges.bottom();
    let mut layouts = Vec::new();

    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        "top-left-corner",
        0.0,
        page.height() - page_margins.top,
        page_margins.left,
        page_margins.top,
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        "top-right-corner",
        page.width() - page_margins.right,
        page.height() - page_margins.top,
        page_margins.right,
        page_margins.top,
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        "bottom-right-corner",
        page.width() - page_margins.right,
        0.0,
        page_margins.right,
        page_margins.bottom,
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        "bottom-left-corner",
        0.0,
        0.0,
        page_margins.left,
        page_margins.bottom,
    );

    push_horizontal_margin_box_group(
        &mut layouts,
        &mut generated,
        font_system,
        ["top-left", "top-center", "top-right"],
        HorizontalMarginGroupGeometry {
            x: page_margins.left,
            y: page.height() - page_margins.top,
            available_width,
            row_height: page_margins.top,
            side: HorizontalPageMarginSide::Top,
        },
        context,
    );
    push_horizontal_margin_box_group(
        &mut layouts,
        &mut generated,
        font_system,
        ["bottom-left", "bottom-center", "bottom-right"],
        HorizontalMarginGroupGeometry {
            x: page_margins.left,
            y: 0.0,
            available_width,
            row_height: page_margins.bottom,
            side: HorizontalPageMarginSide::Bottom,
        },
        context,
    );
    push_vertical_margin_box_group(
        &mut layouts,
        &mut generated,
        font_system,
        ["left-top", "left-middle", "left-bottom"],
        VerticalMarginGroupGeometry {
            x: 0.0,
            y: page_margins.bottom,
            column_width: page_margins.left,
            available_height,
            side: VerticalPageMarginSide::Left,
        },
        context,
    );
    push_vertical_margin_box_group(
        &mut layouts,
        &mut generated,
        font_system,
        ["right-top", "right-middle", "right-bottom"],
        VerticalMarginGroupGeometry {
            x: page.width() - page_margins.right,
            y: page_margins.bottom,
            column_width: page_margins.right,
            available_height,
            side: VerticalPageMarginSide::Right,
        },
        context,
    );

    layouts
}

struct GeneratedMarginBox<'a> {
    spec: &'a PageMarginBoxSpec,
    content: ResolvedPageContent,
}

fn resolved_margin_box_content(
    spec: &PageMarginBoxSpec,
    context: PageMarginPaintContext<'_>,
) -> Option<ResolvedPageContent> {
    let value = spec.declarations.get("content")?;
    let trimmed = css::trim_css_value(value);
    if trimmed.eq_ignore_ascii_case("normal") || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    resolve_page_content_parts(
        value,
        PageContentResolveContext {
            page_number: context.page_number,
            total_pages: context.total_pages,
            page_index: context.page_index,
            base_url: context.base_url,
            root_url: context.root_url,
            page_named_strings: context.page_named_strings,
            page_running_elements: context.page_running_elements,
            page_anchors: context.page_anchors,
            page_anchor_text: context.page_anchor_text,
            counter_styles: context.counter_styles,
            page_counters: context.page_counters,
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HorizontalPageMarginSide {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy)]
struct HorizontalMarginGroupGeometry {
    x: f32,
    y: f32,
    available_width: f32,
    row_height: f32,
    side: HorizontalPageMarginSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerticalPageMarginSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
struct VerticalMarginGroupGeometry {
    x: f32,
    y: f32,
    column_width: f32,
    available_height: f32,
    side: VerticalPageMarginSide,
}

#[derive(Debug, Clone, Copy)]
struct MarginOuterRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    edges: PageMarginBoxEdges,
}

fn push_corner_margin_box_layout<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    generated: &[GeneratedMarginBox<'a>],
    name: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) {
    let Some(box_) = generated.iter().find(|box_| box_.spec.name == name) else {
        return;
    };
    let horizontal = fixed_width_axis(box_.spec, width, height, corner_horizontal_side(name));
    let vertical = fixed_height_axis(box_.spec, height, width, corner_vertical_side(name));
    push_layout_from_outer_rect(
        layouts,
        box_,
        MarginOuterRect {
            x,
            y,
            width,
            height,
            edges: merge_fixed_axis_edges(horizontal, vertical),
        },
    );
}

fn push_horizontal_margin_box_group<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    generated: &mut [GeneratedMarginBox<'a>],
    font_system: &mut FontSystem,
    names: [&str; 3],
    geometry: HorizontalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) {
    let measures = names.map(|name| {
        generated
            .iter()
            .find(|box_| box_.spec.name == name)
            .map(|box_| {
                horizontal_margin_box_measure(box_, font_system, geometry.available_width, context)
            })
            .unwrap_or_else(PageMarginBoxMeasure::not_generated)
    });
    let widths = resolve_variable_outer_sizes(geometry.available_width, measures);
    for (index, name) in names.iter().enumerate() {
        let Some(box_) = generated.iter().find(|box_| box_.spec.name == *name) else {
            continue;
        };
        let outer_width = widths[index].max(0.0);
        let outer_x = match index {
            0 => geometry.x,
            1 => geometry.x + ((geometry.available_width - outer_width) / 2.0),
            _ => geometry.x + geometry.available_width - outer_width,
        };
        let vertical = fixed_height_axis(
            box_.spec,
            geometry.row_height,
            geometry.available_width,
            geometry.side,
        );
        push_layout_from_outer_rect(
            layouts,
            box_,
            MarginOuterRect {
                x: outer_x,
                y: geometry.y,
                width: outer_width,
                height: geometry.row_height,
                edges: vertical,
            },
        );
    }
}

fn push_vertical_margin_box_group<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    generated: &mut [GeneratedMarginBox<'a>],
    font_system: &mut FontSystem,
    names: [&str; 3],
    geometry: VerticalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) {
    let measures = names.map(|name| {
        generated
            .iter()
            .find(|box_| box_.spec.name == name)
            .map(|box_| {
                vertical_margin_box_measure(box_, font_system, geometry.available_height, context)
            })
            .unwrap_or_else(PageMarginBoxMeasure::not_generated)
    });
    let heights = resolve_variable_outer_sizes(geometry.available_height, measures);
    for (index, name) in names.iter().enumerate() {
        let Some(box_) = generated.iter().find(|box_| box_.spec.name == *name) else {
            continue;
        };
        let outer_height = heights[index].max(0.0);
        let outer_y = match index {
            0 => geometry.y + geometry.available_height - outer_height,
            1 => geometry.y + ((geometry.available_height - outer_height) / 2.0),
            _ => geometry.y,
        };
        let horizontal = fixed_width_axis(
            box_.spec,
            geometry.column_width,
            geometry.available_height,
            geometry.side,
        );
        push_layout_from_outer_rect(
            layouts,
            box_,
            MarginOuterRect {
                x: geometry.x,
                y: outer_y,
                width: geometry.column_width,
                height: outer_height,
                edges: horizontal,
            },
        );
    }
}

fn push_layout_from_outer_rect<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    box_: &GeneratedMarginBox<'a>,
    outer: MarginOuterRect,
) {
    let edges = outer.edges;
    let border_x = outer.x + edges.margin.left;
    let border_y = outer.y + edges.margin.bottom;
    let border_width = (outer.width - edges.margin.left - edges.margin.right).max(0.0);
    let border_height = (outer.height - edges.margin.top - edges.margin.bottom).max(0.0);
    layouts.push(PageMarginBoxLayout {
        spec: box_.spec,
        content: box_.content.clone(),
        border_rect: paint_space_rect(border_x, border_y, border_width, border_height),
        content_rect: paint_space_rect(
            border_x + edges.border.left + edges.padding.left,
            border_y + edges.border.bottom + edges.padding.bottom,
            border_width
                - edges.border.left
                - edges.border.right
                - edges.padding.left
                - edges.padding.right,
            border_height
                - edges.border.top
                - edges.border.bottom
                - edges.padding.top
                - edges.padding.bottom,
        ),
    });
}

#[derive(Debug, Clone, Copy)]
struct PageMarginBoxEdges {
    margin: UsedEdges,
    border: css::Edges,
    padding: UsedEdges,
}

/// Resolves a page-margin box's fixed-height dimension.
///
/// CSS Paged Media Level 3 §5.3.3 gives top/bottom margin boxes a fixed
/// height equation over `margin-top`, borders, padding, `height`, and
/// `margin-bottom`; top boxes ignore `margin-top` when overconstrained, while
/// bottom boxes ignore `margin-bottom`:
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>.
fn fixed_height_axis(
    box_: &PageMarginBoxSpec,
    containing_height: f32,
    horizontal_basis: f32,
    side: HorizontalPageMarginSide,
) -> PageMarginBoxEdges {
    let mut edges = used_margin_box_edges(box_, horizontal_basis, containing_height);
    let style = &box_.style;
    let non_content =
        edges.border.top + edges.border.bottom + edges.padding.top + edges.padding.bottom;
    let content_height = used_content_height_or_auto(style, containing_height, non_content);
    let (top, bottom) = resolve_fixed_margin_axis(
        containing_height,
        non_content,
        content_height,
        style.box_values.margin.top,
        style.margin.top,
        style.box_values.margin.bottom,
        style.margin.bottom,
        containing_height,
        match side {
            HorizontalPageMarginSide::Top => FixedAxisAutoMargin::Start,
            HorizontalPageMarginSide::Bottom => FixedAxisAutoMargin::End,
        },
    );
    edges.margin.top = top;
    edges.margin.bottom = bottom;
    edges
}

/// Resolves a page-margin box's fixed-width dimension.
///
/// CSS Paged Media Level 3 §5.3.3 applies the same fixed-dimension equation to
/// left/right margin boxes with width and horizontal margins; left boxes ignore
/// `margin-left` when overconstrained, while right boxes ignore
/// `margin-right`:
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>.
fn fixed_width_axis(
    box_: &PageMarginBoxSpec,
    containing_width: f32,
    vertical_basis: f32,
    side: VerticalPageMarginSide,
) -> PageMarginBoxEdges {
    let mut edges = used_margin_box_edges(box_, containing_width, vertical_basis);
    let style = &box_.style;
    let non_content =
        edges.border.left + edges.border.right + edges.padding.left + edges.padding.right;
    let content_width = used_content_width_or_auto(style, containing_width, non_content);
    let (left, right) = resolve_fixed_margin_axis(
        containing_width,
        non_content,
        content_width,
        style.box_values.margin.left,
        style.margin.left,
        style.box_values.margin.right,
        style.margin.right,
        containing_width,
        match side {
            VerticalPageMarginSide::Left => FixedAxisAutoMargin::Start,
            VerticalPageMarginSide::Right => FixedAxisAutoMargin::End,
        },
    );
    edges.margin.left = left;
    edges.margin.right = right;
    edges
}

fn corner_horizontal_side(name: &str) -> VerticalPageMarginSide {
    if name.contains("left") {
        VerticalPageMarginSide::Left
    } else {
        VerticalPageMarginSide::Right
    }
}

fn corner_vertical_side(name: &str) -> HorizontalPageMarginSide {
    if name.starts_with("top") {
        HorizontalPageMarginSide::Top
    } else {
        HorizontalPageMarginSide::Bottom
    }
}

fn merge_fixed_axis_edges(
    horizontal: PageMarginBoxEdges,
    vertical: PageMarginBoxEdges,
) -> PageMarginBoxEdges {
    PageMarginBoxEdges {
        margin: UsedEdges {
            top: vertical.margin.top,
            right: horizontal.margin.right,
            bottom: vertical.margin.bottom,
            left: horizontal.margin.left,
        },
        border: horizontal.border,
        padding: UsedEdges {
            top: vertical.padding.top,
            right: horizontal.padding.right,
            bottom: vertical.padding.bottom,
            left: horizontal.padding.left,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixedAxisAutoMargin {
    Start,
    End,
}

/// Solves the fixed page-margin box axis equality.
///
/// CSS Paged Media Level 3 §5.3.3 defines a six-step used-value algorithm for
/// fixed dimensions. Auto margins share remaining space, auto sizes fill after
/// non-auto margins, and overconstrained explicit sizes can force the ignored
/// margin side negative to preserve the specified content size:
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>.
#[allow(clippy::too_many_arguments)]
fn resolve_fixed_margin_axis(
    containing_size: f32,
    non_content: f32,
    content_size: Option<f32>,
    start_margin: css::ComputedLengthPercentageOrAuto,
    start_legacy: f32,
    end_margin: css::ComputedLengthPercentageOrAuto,
    end_legacy: f32,
    margin_basis: f32,
    overconstrained_auto: FixedAxisAutoMargin,
) -> (f32, f32) {
    let containing_size = containing_size.max(0.0);
    let non_content = non_content.max(0.0);
    let size_auto = content_size.is_none();
    let mut size = content_size.unwrap_or(0.0).max(0.0);
    let (mut start_auto, mut start) =
        fixed_axis_margin_component(start_margin, start_legacy, margin_basis);
    let (mut end_auto, mut end) = fixed_axis_margin_component(end_margin, end_legacy, margin_basis);

    let specified_sum = non_content
        + if size_auto { 0.0 } else { size }
        + if start_auto { 0.0 } else { start }
        + if end_auto { 0.0 } else { end };
    if specified_sum > containing_size {
        if start_auto {
            start_auto = false;
            start = 0.0;
        }
        if end_auto {
            end_auto = false;
            end = 0.0;
        }
    }

    if !size_auto && !start_auto && !end_auto {
        match overconstrained_auto {
            FixedAxisAutoMargin::Start => {
                start_auto = true;
                start = 0.0;
            }
            FixedAxisAutoMargin::End => {
                end_auto = true;
                end = 0.0;
            }
        }
    }

    let auto_count = usize::from(size_auto) + usize::from(start_auto) + usize::from(end_auto);
    if auto_count == 1 {
        let remaining = containing_size
            - non_content
            - if size_auto { 0.0 } else { size }
            - if start_auto { 0.0 } else { start }
            - if end_auto { 0.0 } else { end };
        if size_auto {
            size = remaining.max(0.0);
        } else if start_auto {
            start = remaining;
            start_auto = false;
        } else {
            end = remaining;
            end_auto = false;
        }
    }

    if size_auto {
        if start_auto {
            start = 0.0;
            start_auto = false;
        }
        if end_auto {
            end = 0.0;
            end_auto = false;
        }
        size = (containing_size - non_content - start - end).max(0.0);
    }

    if start_auto && end_auto {
        let remaining = containing_size - non_content - size;
        start = remaining / 2.0;
        end = remaining / 2.0;
    }

    (start, end)
}

fn fixed_axis_margin_component(
    value: css::ComputedLengthPercentageOrAuto,
    legacy_length: f32,
    basis: f32,
) -> (bool, f32) {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => (true, 0.0),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => (
            false,
            if value.percent != 0.0 {
                used_length_percentage(value, basis)
            } else {
                legacy_length
            },
        ),
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => (false, legacy_length),
    }
}

fn used_margin_box_edges(
    box_: &PageMarginBoxSpec,
    horizontal_basis: f32,
    vertical_basis: f32,
) -> PageMarginBoxEdges {
    let style = &box_.style;
    let margin = style.box_values.margin;
    PageMarginBoxEdges {
        margin: UsedEdges {
            top: margin_edge_for_page_margin_box(margin.top, style.margin.top, vertical_basis),
            right: margin_edge_for_page_margin_box(
                margin.right,
                style.margin.right,
                horizontal_basis,
            ),
            bottom: margin_edge_for_page_margin_box(
                margin.bottom,
                style.margin.bottom,
                vertical_basis,
            ),
            left: margin_edge_for_page_margin_box(margin.left, style.margin.left, horizontal_basis),
        },
        border: used_border_widths(style),
        padding: UsedEdges {
            top: used_length_percentage(style.box_values.padding.top, vertical_basis).max(0.0),
            right: used_length_percentage(style.box_values.padding.right, horizontal_basis)
                .max(0.0),
            bottom: used_length_percentage(style.box_values.padding.bottom, vertical_basis)
                .max(0.0),
            left: used_length_percentage(style.box_values.padding.left, horizontal_basis).max(0.0),
        },
    }
}

fn margin_edge_for_page_margin_box(
    value: css::ComputedLengthPercentageOrAuto,
    legacy_length: f32,
    basis: f32,
) -> f32 {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => 0.0,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent != 0.0 {
                used_length_percentage(value, basis)
            } else {
                legacy_length
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => legacy_length,
    }
}

#[derive(Debug, Clone, Copy)]
struct PageMarginBoxMeasure {
    generated: bool,
    specified_outer: Option<f32>,
    min_outer: f32,
    max_outer: f32,
    min_constraint: Option<f32>,
    max_constraint: Option<f32>,
}

impl PageMarginBoxMeasure {
    fn not_generated() -> Self {
        Self {
            generated: false,
            specified_outer: Some(0.0),
            min_outer: 0.0,
            max_outer: 0.0,
            min_constraint: Some(0.0),
            max_constraint: Some(0.0),
        }
    }

    fn auto_outer(self) -> bool {
        self.generated && self.specified_outer.is_none()
    }

    fn resolved_or_zero(self) -> f32 {
        if !self.generated {
            0.0
        } else {
            self.specified_outer.unwrap_or(0.0)
        }
    }

    fn clamp(self, value: f32) -> f32 {
        let mut value = value.max(0.0);
        if let Some(max) = self.max_constraint {
            value = value.min(max);
        }
        if let Some(min) = self.min_constraint {
            value = value.max(min);
        }
        value
    }
}

fn horizontal_margin_box_measure(
    box_: &GeneratedMarginBox<'_>,
    font_system: &mut FontSystem,
    available_width: f32,
    context: PageMarginPaintContext<'_>,
) -> PageMarginBoxMeasure {
    let style = &box_.spec.style;
    let edges = used_margin_box_edges(box_.spec, available_width, available_width);
    let non_content = edges.margin.left
        + edges.margin.right
        + edges.border.left
        + edges.border.right
        + edges.padding.left
        + edges.padding.right;
    let intrinsic_widths = margin_box_intrinsic_inline_sizes(
        font_system,
        &box_.content,
        style,
        available_width,
        context.base_url,
        context.root_url,
        context.resource_cache,
    );
    let specified_content = used_content_width_or_auto(style, available_width, non_content)
        .or_else(|| {
            intrinsic::intrinsic_width_keyword(
                style.box_values.width,
                intrinsic_widths.0,
                intrinsic_widths.1,
                available_width,
                non_content,
            )
        });
    PageMarginBoxMeasure {
        generated: true,
        specified_outer: specified_content.map(|width| width + non_content),
        min_outer: intrinsic_widths.0 + non_content,
        max_outer: intrinsic_widths.1 + non_content,
        min_constraint: used_min_width(style, available_width).map(|value| value + non_content),
        max_constraint: used_max_width(style, available_width).map(|value| value + non_content),
    }
}

fn vertical_margin_box_measure(
    box_: &GeneratedMarginBox<'_>,
    _font_system: &mut FontSystem,
    available_height: f32,
    context: PageMarginPaintContext<'_>,
) -> PageMarginBoxMeasure {
    let style = &box_.spec.style;
    let edges = used_margin_box_edges(box_.spec, available_height, available_height);
    let non_content = edges.margin.top
        + edges.margin.bottom
        + edges.border.top
        + edges.border.bottom
        + edges.padding.top
        + edges.padding.bottom;
    let content = page_margin_intrinsic_inline_items(
        &box_.content,
        style,
        available_height,
        context.base_url,
        context.root_url,
        context.resource_cache,
    );
    let line_count = content
        .iter()
        .filter(|item| matches!(item, InlineItem::Break))
        .count()
        + 1;
    let atomic_height = content.iter().fold(style.line_height, |height, item| {
        height.max(match item {
            InlineItem::Atom(atom) => atom.height,
            InlineItem::Word(_)
            | InlineItem::Float(_)
            | InlineItem::Break
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => style.line_height,
        })
    });
    let intrinsic = line_count as f32 * atomic_height;
    PageMarginBoxMeasure {
        generated: true,
        specified_outer: used_content_height_or_auto(style, available_height, non_content)
            .map(|height| height + non_content),
        min_outer: intrinsic + non_content,
        max_outer: intrinsic + non_content,
        min_constraint: used_min_height(style, available_height).map(|value| value + non_content),
        max_constraint: used_max_height(style, available_height).map(|value| value + non_content),
    }
}

/// Resolves the variable dimension for one three-box page-margin side.
///
/// CSS Paged Media Level 3 §5.3.2 coordinates top/bottom and left/right
/// triplets so the center box remains centered when generated, and otherwise
/// the side boxes share the available variable dimension.
fn resolve_variable_outer_sizes(available: f32, measures: [PageMarginBoxMeasure; 3]) -> [f32; 3] {
    let mut sizes = [
        measures[0].resolved_or_zero(),
        measures[1].resolved_or_zero(),
        measures[2].resolved_or_zero(),
    ];
    if !measures[1].generated {
        let fixed_sum = measures
            .iter()
            .filter(|measure| measure.generated && !measure.auto_outer())
            .map(|measure| measure.resolved_or_zero())
            .sum::<f32>();
        let auto_indexes = [0usize, 2usize]
            .into_iter()
            .filter(|index| measures[*index].auto_outer())
            .collect::<Vec<_>>();
        match auto_indexes.as_slice() {
            [index] => sizes[*index] = (available - fixed_sum).max(0.0),
            [left, right] => {
                let distributed = distribute_two_auto_sizes(
                    available - fixed_sum,
                    [measures[*left], measures[*right]],
                );
                sizes[*left] = distributed[0];
                sizes[*right] = distributed[1];
            }
            _ => {}
        }
    } else {
        if measures[1].auto_outer() {
            if !measures[0].generated && !measures[2].generated {
                sizes[1] = available.max(0.0);
            } else {
                let side_max = measures[0].max_outer.max(measures[2].max_outer);
                let side_min = measures[0].min_outer.max(measures[2].min_outer);
                let center_proxy = measures[1];
                let side_proxy = PageMarginBoxMeasure {
                    generated: true,
                    specified_outer: None,
                    min_outer: side_min * 2.0,
                    max_outer: side_max * 2.0,
                    min_constraint: None,
                    max_constraint: None,
                };
                sizes[1] = distribute_two_auto_sizes(available, [center_proxy, side_proxy])[0];
            }
        }
        let remaining_side = ((available - sizes[1]).max(0.0)) / 2.0;
        if measures[0].auto_outer() {
            sizes[0] = remaining_side;
        }
        if measures[2].auto_outer() {
            sizes[2] = remaining_side;
        }
    }
    [
        measures[0].clamp(sizes[0]),
        measures[1].clamp(sizes[1]),
        measures[2].clamp(sizes[2]),
    ]
}

fn distribute_two_auto_sizes(available: f32, measures: [PageMarginBoxMeasure; 2]) -> [f32; 2] {
    let available = available.max(0.0);
    let max_sum = measures[0].max_outer + measures[1].max_outer;
    let min_sum = measures[0].min_outer + measures[1].min_outer;
    if max_sum < available {
        let flex_space = available - max_sum;
        let factors = normalized_flex_factors([measures[0].max_outer, measures[1].max_outer]);
        [
            measures[0].max_outer + flex_space * factors[0],
            measures[1].max_outer + flex_space * factors[1],
        ]
    } else if min_sum < available {
        let flex_space = available - min_sum;
        let factors = normalized_flex_factors([
            (measures[0].max_outer - measures[0].min_outer).max(0.0),
            (measures[1].max_outer - measures[1].min_outer).max(0.0),
        ]);
        [
            measures[0].min_outer + flex_space * factors[0],
            measures[1].min_outer + flex_space * factors[1],
        ]
    } else {
        let factors = normalized_flex_factors([measures[0].min_outer, measures[1].min_outer]);
        [available * factors[0], available * factors[1]]
    }
}

fn normalized_flex_factors(values: [f32; 2]) -> [f32; 2] {
    let sum = values[0] + values[1];
    if sum <= 0.0 {
        [0.5, 0.5]
    } else {
        [values[0] / sum, values[1] / sum]
    }
}

fn paint_page_margin_box(
    page: &mut Page,
    layout: &PageMarginBoxLayout<'_>,
    context: PageMarginPaintContext<'_>,
) {
    let style = &layout.spec.style;
    if style.visibility != Visibility::Visible {
        return;
    }
    let (rects, rounded_rects, paths, strokes) = block_paint_ops(
        layout.border_x(),
        layout.border_y(),
        layout.border_width(),
        layout.border_height(),
        style,
    );
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
    for image in background_images_for_style(
        BackgroundPaintArea {
            x: layout.border_x(),
            y: layout.border_y(),
            width: layout.border_width(),
            height: layout.border_height(),
        },
        style,
        context.base_url,
        context.root_url,
        context.resource_cache,
    ) {
        page.push_image_in_band(PaintBand::BackgroundBorder, image);
    }
    for primitive in page_margin_box_outline_primitives(layout, style) {
        push_page_margin_primitive(page, PaintBand::Outline, primitive);
    }
}

fn push_page_margin_primitive(page: &mut Page, band: PaintBand, primitive: PaintPrimitive) {
    match primitive {
        PaintPrimitive::Rect(rect) => page.push_rect_in_band(band, rect),
        PaintPrimitive::RoundedRect(rect) => page.push_rounded_rect_in_band(band, rect),
        PaintPrimitive::Path(path) => page.push_path_in_band(band, path),
        PaintPrimitive::Stroke(stroke) => page.push_stroke_in_band(band, stroke),
        PaintPrimitive::Image(image) => page.push_image_in_band(band, image),
        PaintPrimitive::Line(line) => page.push_line_in_band(band, line),
    };
}

/// Builds outline paint for a generated page-margin box without affecting layout.
///
/// CSS UI defines outlines as visual paint outside the border edge that does not
/// participate in sizing, while CSS Paged Media applies that property set in
/// margin contexts:
/// <https://www.w3.org/TR/css-ui-3/#outline-props> and
/// <https://www.w3.org/TR/css-page-3/#page-properties>.
fn page_margin_box_outline_primitives(
    layout: &PageMarginBoxLayout<'_>,
    style: &ComputedStyle,
) -> Vec<PaintPrimitive> {
    if style.outline_width <= 0.0 || style.outline_style.suppresses_used_width() {
        return Vec::new();
    }
    if layout.border_width() <= 0.0 || layout.border_height() <= 0.0 {
        return Vec::new();
    }

    let mut outline_style = style.clone();
    outline_style.background_color = None;
    outline_style.background_image = None;
    outline_style.background_layers.clear();
    outline_style.border_image = css::BorderImage::initial();
    outline_style.border_width = style.outline_width;
    outline_style.border_widths = css::Edges {
        top: style.outline_width,
        right: style.outline_width,
        bottom: style.outline_width,
        left: style.outline_width,
    };
    outline_style.border_color = style.outline_color;
    outline_style.border_colors = css::BorderColors {
        top: style.outline_color,
        right: style.outline_color,
        bottom: style.outline_color,
        left: style.outline_color,
    };
    outline_style.border_styles = css::BorderStyles {
        top: style.outline_style,
        right: style.outline_style,
        bottom: style.outline_style,
        left: style.outline_style,
    };

    let outset = style.outline_offset + style.outline_width;
    let (rects, rounded_rects, paths, strokes) = block_paint_ops(
        layout.border_x() - outset,
        layout.border_y() - outset,
        layout.border_width() + outset * 2.0,
        layout.border_height() + outset * 2.0,
        &outline_style,
    );
    let mut primitives = Vec::new();
    primitives.extend(rects.into_iter().map(PaintPrimitive::Rect));
    primitives.extend(rounded_rects.into_iter().map(PaintPrimitive::RoundedRect));
    primitives.extend(paths.into_iter().map(PaintPrimitive::Path));
    primitives.extend(strokes.into_iter().map(PaintPrimitive::Stroke));
    primitives
}

/// Returns the top edge of the page-margin text line stack.
///
/// CSS Paged Media defines page-margin boxes as generated boxes with their own
/// content area, and CSS Inline positions text through line-box baselines. The
/// `vertical-align` value chooses where the text stack sits inside the margin
/// box content area; baseline placement is then handled from font metrics:
/// <https://www.w3.org/TR/css-page-3/#page-margin-boxes> and
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
fn page_margin_text_stack_top(
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

fn margin_box_intrinsic_inline_sizes(
    font_system: &mut FontSystem,
    content: &ResolvedPageContent,
    style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
    resource_cache: &ResourceCache,
) -> (f32, f32) {
    let mut min_content: f32 = 0.0;
    let mut max_content: f32 = 0.0;
    let mut paragraph = Vec::new();
    for item in page_margin_intrinsic_inline_items(
        content,
        style,
        available_width,
        base_url,
        root_url,
        resource_cache,
    ) {
        if matches!(item, InlineItem::Break) {
            accumulate_page_margin_intrinsic_paragraph(
                font_system,
                &mut paragraph,
                style,
                &mut min_content,
                &mut max_content,
            );
        } else {
            paragraph.push(item);
        }
    }
    accumulate_page_margin_intrinsic_paragraph(
        font_system,
        &mut paragraph,
        style,
        &mut min_content,
        &mut max_content,
    );
    if min_content == 0.0 {
        min_content = max_content;
    }
    (min_content, max_content)
}

/// Builds page-margin intrinsic sizing input from generated inline content.
///
/// Page-margin boxes size themselves from CSS Text intrinsic contributions.
/// This helper preserves the generated inline stream before line selection so
/// min/max-content sizing sees the same transformed text, tab stops, soft-wrap
/// opportunities, generated images, and hanging punctuation as final
/// page-margin painting:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
fn page_margin_intrinsic_inline_items(
    content: &ResolvedPageContent,
    style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
    resource_cache: &ResourceCache,
) -> Vec<InlineItem> {
    let mut items = Vec::new();
    let mut quote_depth = 0usize;
    let mut text_buffer = String::new();
    let inline_style = page_margin_inline_content_style(style);
    for item in &content.items {
        match item {
            PageMarginContentItem::EmbeddedRunningElement(capture) => {
                let parts = running_element_inline_parts(capture);
                for part in &parts {
                    append_page_margin_intrinsic_part(
                        &mut items,
                        &mut text_buffer,
                        &mut quote_depth,
                        part,
                        &inline_style,
                        style,
                        available_width,
                        base_url,
                        root_url,
                        resource_cache,
                    );
                }
            }
            PageMarginContentItem::Inline(part) => append_page_margin_intrinsic_part(
                &mut items,
                &mut text_buffer,
                &mut quote_depth,
                part,
                &inline_style,
                style,
                available_width,
                base_url,
                root_url,
                resource_cache,
            ),
        }
    }
    flush_page_margin_intrinsic_text_buffer(&mut items, &mut text_buffer, style);
    items
}

#[allow(clippy::too_many_arguments)]
fn append_page_margin_intrinsic_part(
    items: &mut Vec<InlineItem>,
    text_buffer: &mut String,
    quote_depth: &mut usize,
    part: &GeneratedContentPart,
    inline_style: &ComputedStyle,
    box_style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
    resource_cache: &ResourceCache,
) {
    match part {
        GeneratedContentPart::Text(text) => {
            text_buffer.push_str(text);
        }
        GeneratedContentPart::Leader(text) => {
            flush_page_margin_intrinsic_text_buffer(items, text_buffer, inline_style);
            items.push(InlineItem::Atom(Box::new(InlineAtom {
                content: InlineAtomContent::Leader(text.clone()),
                style: inline_style.clone(),
                escaped_positioned_layers: None,
                width: 0.0,
                height: inline_style.line_height,
                baseline_offset: inline_style.font_size,
                baseline_shift: 0.0,
                link_target: None,
                alt_text: None,
            })));
        }
        GeneratedContentPart::Quote(quote) => {
            let text = page_margin_quote_text(*quote, inline_style, quote_depth);
            text_buffer.push_str(&text);
        }
        GeneratedContentPart::Image {
            url,
            base_url: image_base_url,
            root_url: image_root_url,
        } => {
            flush_page_margin_intrinsic_text_buffer(items, text_buffer, inline_style);
            if let Some(image) = used_generated_image(
                url,
                box_style,
                available_width,
                image_base_url.as_deref().or(base_url),
                image_root_url.as_deref().or(root_url),
                resource_cache,
            ) {
                items.push(InlineItem::Atom(Box::new(InlineAtom {
                    content: InlineAtomContent::Image(image.decoded),
                    style: inline_style.clone(),
                    escaped_positioned_layers: None,
                    width: image.border_box_width,
                    height: image.border_box_height,
                    baseline_offset: image.border_box_height,
                    baseline_shift: 0.0,
                    link_target: None,
                    alt_text: None,
                })));
            }
        }
        GeneratedContentPart::Contents
        | GeneratedContentPart::Attr { .. }
        | GeneratedContentPart::Counter { .. }
        | GeneratedContentPart::Counters { .. } => {}
    }
}

fn flush_page_margin_intrinsic_text_buffer(
    output: &mut Vec<InlineItem>,
    text: &mut String,
    style: &ComputedStyle,
) {
    if !text.is_empty() {
        push_inline_words_for_style(text, style, None, 0.0, output);
        normalize_inline_whitespace_items(output);
        text.clear();
    }
}

fn accumulate_page_margin_intrinsic_paragraph(
    font_system: &mut FontSystem,
    paragraph: &mut Vec<InlineItem>,
    style: &ComputedStyle,
    min_content: &mut f32,
    max_content: &mut f32,
) {
    trim_inline_item_edges(paragraph);
    if paragraph.is_empty() {
        return;
    }
    let graph = inline_layout::build_inline_opportunity_graph(font_system, paragraph, style);
    let contribution = graph.intrinsic_contribution(font_system, style);
    *min_content = (*min_content).max(contribution.min_content);
    *max_content = (*max_content).max(contribution.max_content);
    paragraph.clear();
}

fn running_element_inline_parts(capture: &RunningElementCapture) -> Vec<GeneratedContentPart> {
    if !capture.content_parts.is_empty() {
        return capture.content_parts.clone();
    }
    if capture.fallback_text.is_empty() {
        Vec::new()
    } else {
        vec![GeneratedContentPart::Text(capture.fallback_text.clone())]
    }
}

/// Derives the inline content style used by generated page-margin text.
///
/// CSS Paged Media creates a margin box whose background, border, padding, and
/// outline paint on the margin box itself, while CSS Generated Content supplies
/// inline content inside that box. Reusing the box style directly for inline
/// fragments would duplicate the margin-box border/background around each text
/// run:
/// <https://www.w3.org/TR/css-page-3/#page-margin-boxes> and
/// <https://www.w3.org/TR/css-content-3/#content-property>.
fn page_margin_inline_content_style(style: &ComputedStyle) -> ComputedStyle {
    let mut inline_style = style.clone();
    inline_style.margin = css::Edges::ZERO;
    inline_style.ua_margin_em = css::OptionalEdges::NONE;
    inline_style.padding = css::Edges::ZERO;
    inline_style.border_width = 0.0;
    inline_style.border_widths = css::Edges::ZERO;
    inline_style.border_styles = css::BorderStyles::NONE;
    inline_style.border_radius = css::BorderRadius::ZERO;
    inline_style.corner_shapes = css::CornerShapes::ROUND;
    inline_style.border_image = css::BorderImage::initial();
    inline_style.outline_width = 0.0;
    inline_style.outline_style = css::BorderStyle::None;
    inline_style.outline_offset = 0.0;
    inline_style.background_color = None;
    inline_style.background_image = None;
    inline_style.background_layers.clear();
    inline_style
}

#[allow(clippy::too_many_arguments)]
fn append_page_margin_inline_part(
    items: &mut Vec<InlineItem>,
    part: &GeneratedContentPart,
    inline_style: &ComputedStyle,
    box_style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
    resource_cache: &ResourceCache,
    quote_depth: &mut usize,
) {
    match part {
        GeneratedContentPart::Text(text) => {
            push_inline_words_for_style(text, inline_style, None, 0.0, items);
        }
        GeneratedContentPart::Leader(text) => {
            items.push(InlineItem::Atom(Box::new(InlineAtom {
                content: InlineAtomContent::Leader(text.clone()),
                style: inline_style.clone(),
                escaped_positioned_layers: None,
                width: 0.0,
                height: inline_style.line_height,
                baseline_offset: inline_style.font_size,
                baseline_shift: 0.0,
                link_target: None,
                alt_text: None,
            })));
        }
        GeneratedContentPart::Quote(quote) => {
            let text = page_margin_quote_text(*quote, inline_style, quote_depth);
            push_inline_words_for_style(&text, inline_style, None, 0.0, items);
        }
        GeneratedContentPart::Image {
            url,
            base_url: image_base_url,
            root_url: image_root_url,
        } => {
            if let Some(image) = used_generated_image(
                url,
                box_style,
                available_width,
                image_base_url.as_deref().or(base_url),
                image_root_url.as_deref().or(root_url),
                resource_cache,
            ) {
                items.push(InlineItem::Atom(Box::new(InlineAtom {
                    content: InlineAtomContent::Image(image.decoded),
                    style: inline_style.clone(),
                    escaped_positioned_layers: None,
                    width: image.border_box_width,
                    height: image.border_box_height,
                    baseline_offset: image.border_box_height,
                    baseline_shift: 0.0,
                    link_target: None,
                    alt_text: None,
                })));
            }
        }
        GeneratedContentPart::Contents
        | GeneratedContentPart::Attr { .. }
        | GeneratedContentPart::Counter { .. }
        | GeneratedContentPart::Counters { .. } => {}
    }
}

fn page_margin_quote_text(
    quote: GeneratedQuote,
    style: &ComputedStyle,
    quote_depth: &mut usize,
) -> String {
    match quote {
        GeneratedQuote::Open => {
            let text = page_margin_quote_pair(style, *quote_depth).0;
            *quote_depth += 1;
            text
        }
        GeneratedQuote::Close => {
            *quote_depth = quote_depth.saturating_sub(1);
            page_margin_quote_pair(style, *quote_depth).1
        }
        GeneratedQuote::NoOpen => {
            *quote_depth += 1;
            String::new()
        }
        GeneratedQuote::NoClose => {
            *quote_depth = quote_depth.saturating_sub(1);
            String::new()
        }
    }
}

fn page_margin_quote_pair(style: &ComputedStyle, depth: usize) -> (String, String) {
    match &style.quotes {
        Quotes::None => (String::new(), String::new()),
        Quotes::Pairs(pairs) => pairs
            .get(depth)
            .or_else(|| pairs.last())
            .cloned()
            .unwrap_or_else(|| ("“".to_string(), "”".to_string())),
        Quotes::Auto { .. } => {
            let (open, close) = quotes::language_quote_pair(style.quotes.auto_language(), depth);
            (open.to_string(), close.to_string())
        }
    }
}
