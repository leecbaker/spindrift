use super::assets::{BackgroundPaintArea, background_images_for_style, paint_effects_for_box};
use super::page_generated::{
    PageContentResolveContext, ResolvedPageContent, resolve_page_content_parts,
};
use super::*;
use crate::css::{TextDecoration, TextEmphasisSkip, TextEmphasisStyle, TextShadow};
use crate::text::{character_is_text_decoration_spacer, character_receives_text_emphasis_mark};

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
        let font_system = &mut self.font_system;
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
        for (index, page) in self.pages.iter_mut().enumerate() {
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
            let boxes = page_margin_boxes_for_rules(
                PageMarginCascadeContext {
                    page_rules: &page_rules,
                    page_number,
                    page_name,
                    is_blank,
                    page_progression_direction,
                    fallback: &fallback_margin_boxes,
                    page_declarations: &page_declarations,
                    base_page_style: &base_page_style,
                },
                font_system,
            );
            let page_size = css::page_size_from(&page_declarations, base_page_context.size);
            let page_edges =
                super::builder::page_box_edges_from_declarations(&page_declarations, page_size);
            let page_margins = css::page_margins_from_for_size_and_edges(
                &page_declarations,
                base_page_context.margins,
                page_size,
                page_edges.total(),
            );
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
                page_counters: page_counter_values
                    .get(index)
                    .unwrap_or(&self.page_counter_initial_values),
            };
            let layouts = layout_page_margin_boxes(page, &boxes, context, font_system);
            let mut painted_boxes = Vec::new();
            for layout in &layouts {
                let checkpoint = page.paint_checkpoint();
                paint_page_margin_box(page, layout, context, font_system);
                painted_boxes.push(PageMarginPaintedBox {
                    z_index: layout.spec.style.z_index.unwrap_or(0),
                    order: page_margin_box_paint_order(&layout.spec.name),
                    effects: paint_effects_for_box(&layout.spec.style, layout.border_clip()),
                    bounds: layout.border_clip(),
                    fragment: page.take_paint_fragment_since(checkpoint),
                });
            }
            replay_page_margin_box_fragments(page, painted_boxes);
        }
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

fn page_margin_boxes_for_rules(
    context: PageMarginCascadeContext<'_>,
    font_system: &mut FontSystem,
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
        let font_id = font_system.resolve_style(&style);
        boxes.push(PageMarginBoxSpec {
            name: (*name).to_string(),
            declarations,
            style,
            font_id,
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
        "left-top" | "right-top" => VerticalAlign::Top,
        "left-bottom" | "right-bottom" => VerticalAlign::Bottom,
        _ => VerticalAlign::Middle,
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
    font_id: Option<usize>,
}

struct PageMarginBoxLayout<'a> {
    spec: &'a PageMarginBoxSpec,
    content: ResolvedPageContent,
    border_x: f32,
    border_y: f32,
    border_width: f32,
    border_height: f32,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    content_height: f32,
}

impl PageMarginBoxLayout<'_> {
    fn border_clip(&self) -> PaintClip {
        PaintClip {
            x: self.border_x,
            y: self.border_y,
            width: self.border_width,
            height: self.border_height,
        }
    }
}

struct PageMarginPaintedBox {
    z_index: i32,
    order: usize,
    effects: PaintEffects,
    bounds: PaintClip,
    fragment: PaintFragment,
}

/// Replays page-margin boxes into the page display list in stacking order.
///
/// CSS Paged Media paints generated page-margin boxes using clockwise tree
/// order by default, but each page-margin box establishes a stacking context
/// and honors `z-index` relative to the document canvas/content stack:
/// <https://www.w3.org/TR/css-page-3/#painting>.
fn replay_page_margin_box_fragments(page: &mut Page, mut boxes: Vec<PageMarginPaintedBox>) {
    boxes.sort_by_key(|box_| (box_.z_index, box_.order));

    for box_ in boxes {
        if box_.z_index < 0 {
            let fragment = PaintFragment::from_primitives_in_band(
                PaintBand::BackgroundBorder,
                box_.fragment.flattened_primitives(),
                box_.fragment.links,
            );
            let recorded = page.record_paint_fragment(&fragment, 0.0, 0.0);
            page.prepend_recorded_paint_fragment(recorded);
        } else {
            let context = PaintStackingContext::new(box_.z_index, box_.fragment, Vec::new())
                .with_effects(box_.effects)
                .with_bounds(box_.bounds)
                .with_source_order(box_.order);
            let fragment = PaintFragment::from_stacking_context(context);
            page.append_paint_fragment(&fragment, 0.0, 0.0);
        }
    }
}

fn page_margin_box_paint_order(name: &str) -> usize {
    PAGE_MARGIN_BOX_NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .unwrap_or(PAGE_MARGIN_BOX_NAMES.len())
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
    let page_area_width = (page.width
        - page_margins.left
        - page_margins.right
        - page_edges.left()
        - page_edges.right())
    .max(0.0);
    let page_area_height = (page.height
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
        page.height - page_margins.top,
        page_margins.left,
        page_margins.top,
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        "top-right-corner",
        page.width - page_margins.right,
        page.height - page_margins.top,
        page_margins.right,
        page_margins.top,
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        "bottom-right-corner",
        page.width - page_margins.right,
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
            y: page.height - page_margins.top,
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
            x: page.width - page_margins.right,
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
        border_x,
        border_y,
        border_width,
        border_height,
        content_x: border_x + edges.border.left + edges.padding.left,
        content_y: border_y + edges.border.bottom + edges.padding.bottom,
        content_width: (border_width
            - edges.border.left
            - edges.border.right
            - edges.padding.left
            - edges.padding.right)
            .max(0.0),
        content_height: (border_height
            - edges.border.top
            - edges.border.bottom
            - edges.padding.top
            - edges.padding.bottom)
            .max(0.0),
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
    let intrinsic = margin_box_intrinsic_inline_sizes(
        font_system,
        &box_.content,
        style,
        available_width,
        context.base_url,
        context.root_url,
        context.resource_cache,
    );
    PageMarginBoxMeasure {
        generated: true,
        specified_outer: used_content_width_or_auto(style, available_width, non_content)
            .map(|width| width + non_content),
        min_outer: intrinsic.0 + non_content,
        max_outer: intrinsic.1 + non_content,
        min_constraint: used_min_width(style, available_width).map(|value| value + non_content),
        max_constraint: used_max_width(style, available_width).map(|value| value + non_content),
    }
}

fn vertical_margin_box_measure(
    box_: &GeneratedMarginBox<'_>,
    font_system: &mut FontSystem,
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
    let content = resolved_page_margin_inline_content(
        &box_.content,
        style,
        font_system,
        available_height,
        context.base_url,
        context.root_url,
        context.resource_cache,
    );
    let line_count = content
        .iter()
        .filter(|item| matches!(item, PageMarginInlineItem::Break))
        .count()
        + 1;
    let atomic_height = content.iter().fold(style.line_height, |height, item| {
        height.max(match item {
            PageMarginInlineItem::Image { height, .. } => *height,
            PageMarginInlineItem::Text { .. } | PageMarginInlineItem::Break => style.line_height,
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
    font_system: &mut FontSystem,
) {
    let style = &layout.spec.style;
    if style.visibility != Visibility::Visible {
        return;
    }
    let (rects, rounded_rects, paths, strokes) = block_paint_ops(
        layout.border_x,
        layout.border_y,
        layout.border_width,
        layout.border_height,
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
            x: layout.border_x,
            y: layout.border_y,
            width: layout.border_width,
            height: layout.border_height,
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

    if layout.content.is_empty() {
        return;
    }
    let available_width = layout.content_width.max(1.0);
    let content_items = resolved_page_margin_inline_content(
        &layout.content,
        style,
        font_system,
        available_width,
        context.base_url,
        context.root_url,
        context.resource_cache,
    );
    paint_page_margin_inline_content(page, layout, &content_items, font_system);
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
    if layout.border_width <= 0.0 || layout.border_height <= 0.0 {
        return Vec::new();
    }

    let mut outline_style = style.clone();
    outline_style.background_color = None;
    outline_style.background_image = None;
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
        layout.border_x - outset,
        layout.border_y - outset,
        layout.border_width + outset * 2.0,
        layout.border_height + outset * 2.0,
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
    match vertical_align {
        VerticalAlign::Bottom | VerticalAlign::TextBottom => layout.content_y + total_height,
        VerticalAlign::Top | VerticalAlign::TextTop => layout.content_y + layout.content_height,
        VerticalAlign::Middle
        | VerticalAlign::Baseline
        | VerticalAlign::Sub
        | VerticalAlign::Super => layout.content_y + ((layout.content_height + total_height) / 2.0),
    }
}

/// Returns the rendered first text baseline offset from a page-margin line top.
///
/// Page-margin generated content paints text using the same PDF baseline
/// projection as normal inline content. CSS Inline defines baselines from font
/// metrics inside the line box; the renderer's `RenderedLine::y` stores the PDF
/// text baseline after applying the selected font ascent adjustment:
/// <https://www.w3.org/TR/css-inline-3/#line-box> and
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
fn page_margin_text_baseline_offset(font_system: &mut FontSystem, style: &ComputedStyle) -> f32 {
    let font_id = font_system.resolve_style(style);
    let line_height = font_system.line_height_for_font(font_id, style);
    let adjustment = font_system.font_ascent_baseline_adjustment(font_id, style, line_height);
    style.font_size - adjustment
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
    for part in &content.parts {
        match part {
            GeneratedContentPart::Text(text) | GeneratedContentPart::Leader(text) => {
                text_buffer.push_str(text);
            }
            GeneratedContentPart::Quote(quote) => {
                let text = page_margin_quote_text(*quote, style, &mut quote_depth);
                text_buffer.push_str(&text);
            }
            GeneratedContentPart::Image {
                url,
                base_url: image_base_url,
                root_url: image_root_url,
            } => {
                flush_page_margin_intrinsic_text_buffer(&mut items, &mut text_buffer, style);
                if let Some(image) = used_generated_image(
                    url,
                    style,
                    available_width,
                    image_base_url.as_deref().or(base_url),
                    image_root_url.as_deref().or(root_url),
                    resource_cache,
                ) {
                    items.push(InlineItem::Atom(Box::new(InlineAtom {
                        content: InlineAtomContent::Image(image.decoded),
                        style: style.clone(),
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
    flush_page_margin_intrinsic_text_buffer(&mut items, &mut text_buffer, style);
    items
}

fn flush_page_margin_intrinsic_text_buffer(
    output: &mut Vec<InlineItem>,
    text: &mut String,
    style: &ComputedStyle,
) {
    if !text.is_empty() {
        push_inline_words_for_style(text, style, None, 0.0, output);
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
    let graph = inline_layout::build_inline_opportunity_graph(font_system, paragraph);
    let contribution = graph.intrinsic_contribution(font_system, style);
    *min_content = (*min_content).max(contribution.min_content);
    *max_content = (*max_content).max(contribution.max_content);
    paragraph.clear();
}

#[derive(Debug, Clone)]
enum PageMarginInlineItem {
    Text {
        text: String,
        width: f32,
        runs: Vec<RenderedTextRun>,
    },
    Image {
        image: DecodedPngImage,
        width: f32,
        height: f32,
        alt_text: Option<String>,
    },
    Break,
}

impl PageMarginInlineItem {
    fn width(&self) -> f32 {
        match self {
            Self::Text { width, .. } | Self::Image { width, .. } => *width,
            Self::Break => 0.0,
        }
    }

    fn height(&self, style: &ComputedStyle) -> f32 {
        match self {
            Self::Image { height, .. } => *height,
            Self::Text { .. } | Self::Break => style.line_height,
        }
    }
}

fn resolved_page_margin_inline_content(
    content: &ResolvedPageContent,
    style: &ComputedStyle,
    font_system: &mut FontSystem,
    available_width: f32,
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
    resource_cache: &ResourceCache,
) -> Vec<PageMarginInlineItem> {
    let mut items = Vec::new();
    let mut quote_depth = 0usize;
    let mut text_buffer = String::new();
    for part in &content.parts {
        match part {
            GeneratedContentPart::Text(text) => {
                text_buffer.push_str(text);
            }
            GeneratedContentPart::Leader(text) => {
                text_buffer.push_str(text);
            }
            GeneratedContentPart::Quote(quote) => {
                let text = page_margin_quote_text(*quote, style, &mut quote_depth);
                text_buffer.push_str(&text);
            }
            GeneratedContentPart::Image {
                url,
                base_url: image_base_url,
                root_url: image_root_url,
            } => {
                flush_page_margin_text_buffer(
                    &mut items,
                    &mut text_buffer,
                    style,
                    font_system,
                    available_width,
                );
                if let Some(image) = used_generated_image(
                    url,
                    style,
                    available_width,
                    image_base_url.as_deref().or(base_url),
                    image_root_url.as_deref().or(root_url),
                    resource_cache,
                ) {
                    items.push(PageMarginInlineItem::Image {
                        image: image.decoded,
                        width: image.border_box_width,
                        height: image.border_box_height,
                        alt_text: None,
                    });
                }
            }
            GeneratedContentPart::Contents
            | GeneratedContentPart::Attr { .. }
            | GeneratedContentPart::Counter { .. }
            | GeneratedContentPart::Counters { .. } => {}
        }
    }
    flush_page_margin_text_buffer(
        &mut items,
        &mut text_buffer,
        style,
        font_system,
        available_width,
    );
    items
}

fn flush_page_margin_text_buffer(
    output: &mut Vec<PageMarginInlineItem>,
    text: &mut String,
    style: &ComputedStyle,
    font_system: &mut FontSystem,
    available_width: f32,
) {
    if text.is_empty() {
        return;
    }
    let mut inline_items = Vec::new();
    push_inline_words_for_style(text, style, None, 0.0, &mut inline_items);

    let mut paragraph = Vec::new();
    let mut emitted_line_in_buffer = false;
    for item in inline_items {
        if matches!(item, InlineItem::Break) {
            flush_page_margin_graph_paragraph(
                output,
                &mut paragraph,
                style,
                font_system,
                available_width,
                &mut emitted_line_in_buffer,
            );
            output.push(PageMarginInlineItem::Break);
            emitted_line_in_buffer = false;
        } else {
            paragraph.push(item);
        }
    }
    flush_page_margin_graph_paragraph(
        output,
        &mut paragraph,
        style,
        font_system,
        available_width,
        &mut emitted_line_in_buffer,
    );
    text.clear();
}

/// Select page-margin generated text lines through the normal CSS Text graph.
///
/// CSS Paged Media margin boxes use generated inline content, and CSS Text
/// applies the same white-space processing, transforms, tab stops, soft wraps,
/// hyphenation, and hanging punctuation to generated text as to authored text:
/// <https://www.w3.org/TR/css-page-3/#margin-boxes> and
/// <https://www.w3.org/TR/css-text-3/#text-processing-order>.
fn flush_page_margin_graph_paragraph(
    output: &mut Vec<PageMarginInlineItem>,
    paragraph: &mut Vec<InlineItem>,
    style: &ComputedStyle,
    font_system: &mut FontSystem,
    available_width: f32,
    emitted_line_in_buffer: &mut bool,
) {
    let lines = graph_text_lines_for_paragraph(font_system, paragraph, style, available_width);
    for line in lines {
        if *emitted_line_in_buffer {
            output.push(PageMarginInlineItem::Break);
        }
        let runs = line.shaped.map_or_else(
            || font_system.shape_text_runs_with_parley(&line.text, style),
            |shaped| shaped.rendered_runs(),
        );
        output.push(PageMarginInlineItem::Text {
            text: line.text,
            width: line.width,
            runs,
        });
        *emitted_line_in_buffer = true;
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

fn paint_page_margin_inline_content(
    page: &mut Page,
    layout: &PageMarginBoxLayout<'_>,
    items: &[PageMarginInlineItem],
    font_system: &mut FontSystem,
) {
    let style = &layout.spec.style;
    let available_width = layout.content_width.max(1.0);
    let lines = page_margin_inline_lines(items, available_width, style);
    let total_height = lines
        .iter()
        .map(|line| line.height)
        .sum::<f32>()
        .max(font_system.used_line_height(style));
    let mut line_top = page_margin_text_stack_top(layout, style.vertical_align, total_height);
    for line in lines {
        let line_x = aligned_x_with_width(
            layout.content_x,
            available_width,
            line.width,
            style.text_align.physical(style.direction),
        );
        let mut cursor_x = line_x;
        let baseline_y = line_top - page_margin_text_baseline_offset(font_system, style);
        for item in line.items {
            match item {
                PageMarginInlineItem::Text { text, width, runs } => {
                    let first_font_id = runs.first().and_then(|run| run.font_id);
                    let line = RenderedLine {
                        text,
                        x: cursor_x,
                        y: baseline_y,
                        font_size: style.font_size,
                        font_id: first_font_id.or(layout.spec.font_id),
                        color: style.color,
                        runs,
                    };
                    paint_page_margin_text_shadows(page, &line, width, style);
                    paint_page_margin_text_decoration(
                        page,
                        cursor_x,
                        baseline_y,
                        width,
                        style,
                        PageMarginDecorationPhase::BeforeText,
                        None,
                    );
                    page.push_line_in_band(PaintBand::Inline, line.clone());
                    paint_page_margin_emphasis_marks(page, &line, style, font_system);
                    paint_page_margin_text_decoration(
                        page,
                        cursor_x,
                        baseline_y,
                        width,
                        style,
                        PageMarginDecorationPhase::AfterText,
                        None,
                    );
                    cursor_x += width;
                }
                PageMarginInlineItem::Image {
                    image,
                    width,
                    height,
                    alt_text,
                } => {
                    page.push_image_in_band(
                        PaintBand::Inline,
                        RenderedImage {
                            background: false,
                            x: cursor_x,
                            y: line_top - height,
                            width,
                            height,
                            pixel_width: image.pixel_width,
                            pixel_height: image.pixel_height,
                            source_rect: None,
                            interpolate: false,
                            rgb: image.rgb,
                            alpha: image.alpha,
                            alt_text,
                        },
                    );
                    cursor_x += width;
                }
                PageMarginInlineItem::Break => {}
            }
        }
        line_top -= line.height;
    }
}

#[derive(Debug)]
struct PageMarginInlineLine {
    items: Vec<PageMarginInlineItem>,
    width: f32,
    height: f32,
}

fn page_margin_inline_lines(
    items: &[PageMarginInlineItem],
    available_width: f32,
    style: &ComputedStyle,
) -> Vec<PageMarginInlineLine> {
    let mut lines = Vec::new();
    let mut current = PageMarginInlineLine {
        items: Vec::new(),
        width: 0.0,
        height: style.line_height,
    };
    for item in items {
        if matches!(item, PageMarginInlineItem::Break) {
            lines.push(current);
            current = PageMarginInlineLine {
                items: Vec::new(),
                width: 0.0,
                height: style.line_height,
            };
            continue;
        }
        let item_width = item.width();
        if !current.items.is_empty() && current.width + item_width > available_width {
            lines.push(current);
            current = PageMarginInlineLine {
                items: Vec::new(),
                width: 0.0,
                height: style.line_height,
            };
        }
        current.height = current.height.max(item.height(style));
        current.width += item_width;
        current.items.push(item.clone());
    }
    if !current.items.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageMarginDecorationPhase {
    BeforeText,
    AfterText,
    All,
}

impl PageMarginDecorationPhase {
    fn paints_before_text(self) -> bool {
        matches!(self, Self::BeforeText | Self::All)
    }

    fn paints_after_text(self) -> bool {
        matches!(self, Self::AfterText | Self::All)
    }
}

fn paint_page_margin_text_shadows(
    page: &mut Page,
    line: &RenderedLine,
    width: f32,
    style: &ComputedStyle,
) {
    for shadow in style.text_shadow.iter().rev() {
        let color = shadow.color.resolve(style.color);
        if shadow.inset || !color.is_visible() {
            continue;
        }
        for pass in page_margin_text_shadow_paint_passes(*shadow, color) {
            let mut shadow_line = line.clone();
            shadow_line.x += shadow.offset_x + pass.x_offset;
            shadow_line.y -= shadow.offset_y + pass.y_offset;
            shadow_line.color = pass.color;
            paint_page_margin_text_decoration(
                page,
                shadow_line.x,
                shadow_line.y,
                width,
                style,
                PageMarginDecorationPhase::All,
                Some(pass.color),
            );
            page.push_line_in_band(PaintBand::Inline, shadow_line);
        }
    }
}

fn paint_page_margin_emphasis_marks(
    page: &mut Page,
    line: &RenderedLine,
    style: &ComputedStyle,
    font_system: &mut FontSystem,
) {
    let Some(mark) = style
        .text_emphasis_style
        .mark_for_writing_mode(style.writing_mode)
    else {
        return;
    };
    if mark.is_empty() {
        return;
    }

    let mut mark_style = style.clone();
    mark_style.text_decoration_layers.clear();
    mark_style.text_decoration = ComputedStyle::initial().text_decoration;
    mark_style.text_shadow.clear();
    mark_style.text_emphasis_style = TextEmphasisStyle::None;
    mark_style.color = style.text_emphasis_color.unwrap_or(style.color);
    mark_style.font_size = (style.font_size * 0.5).max(1.0);
    let mark_width = font_system.measure_text(mark, &mark_style);
    let vertical = style.writing_mode != WritingMode::HorizontalTb;
    for run in &line.runs {
        let Some(glyphs) = &run.glyphs else {
            continue;
        };
        let mut pen_x = line.x + run.x_offset;
        for glyph in glyphs {
            if glyph.unicode.chars().any(|character| {
                page_margin_character_receives_text_emphasis_mark(
                    character,
                    style.text_emphasis_skip,
                )
            }) {
                let mark_x = if vertical {
                    let side_offset = if style.text_emphasis_position.right {
                        style.font_size * 0.55
                    } else {
                        -style.font_size * 0.55 - mark_width
                    };
                    line.x + run.x_offset + glyph.x_offset + side_offset
                } else {
                    pen_x + glyph.x_offset + (glyph.x_advance - mark_width) / 2.0
                };
                let mark_y = if vertical {
                    line.y
                } else if style.text_emphasis_position.over {
                    line.y + style.font_size * 0.55
                } else {
                    line.y - style.font_size * 0.35
                };
                push_page_margin_text_run(page, mark, mark_x, mark_y, &mark_style, font_system);
            }
            pen_x += glyph.x_advance;
        }
    }
}

fn push_page_margin_text_run(
    page: &mut Page,
    text: &str,
    x: f32,
    y: f32,
    style: &ComputedStyle,
    font_system: &mut FontSystem,
) {
    let Some(shaped) = font_system.shape_unwrapped_line(text, style, style.line_height) else {
        return;
    };
    let runs = shaped.rendered_runs();
    if runs.is_empty() {
        return;
    }
    let font_id = shaped.first_font_id();
    page.push_line_in_band(
        PaintBand::Inline,
        RenderedLine {
            text: shaped.text,
            x,
            y: y + shaped.baseline_adjustment,
            font_size: style.font_size,
            font_id,
            color: style.color,
            runs,
        },
    );
}

fn paint_page_margin_text_decoration(
    page: &mut Page,
    x: f32,
    baseline_y: f32,
    width: f32,
    style: &ComputedStyle,
    phase: PageMarginDecorationPhase,
    color_override: Option<Color>,
) {
    let decorations = page_margin_active_text_decoration_layers(style);
    if decorations.is_empty() || width <= 0.0 {
        return;
    }
    for decoration in decorations {
        if !decoration.has_visible_line() {
            continue;
        }
        let color = color_override.or(decoration.color).unwrap_or(style.color);
        if !color.is_visible() {
            continue;
        }
        let (inset_start, inset_end) = decoration.inset.used(style.font_size);
        let (x, width) = match style.direction {
            Direction::Ltr => (x + inset_start, width - inset_start - inset_end),
            Direction::Rtl => (x + inset_end, width - inset_start - inset_end),
        };
        let width = width.max(0.0);
        if width <= 0.0 {
            continue;
        }
        let thickness = match decoration.thickness {
            TextDecorationThickness::LengthPercentage(value) => {
                used_length_percentage(value, style.font_size).max(0.5)
            }
            TextDecorationThickness::Auto | TextDecorationThickness::FromFont => {
                (style.font_size / 16.0).max(0.5)
            }
        };
        if phase.paints_before_text() && decoration.underline {
            push_page_margin_text_decoration_stroke(
                page,
                x,
                page_margin_used_underline_y(
                    baseline_y,
                    decoration.underline_position,
                    style.font_size,
                    thickness,
                ),
                width,
                thickness,
                color,
                decoration.style,
            );
        }
        if phase.paints_before_text() && decoration.overline {
            push_page_margin_text_decoration_stroke(
                page,
                x,
                baseline_y + style.font_size,
                width,
                thickness,
                color,
                decoration.style,
            );
        }
        if phase.paints_before_text() && decoration.spelling_error {
            push_page_margin_text_decoration_stroke(
                page,
                x,
                page_margin_used_underline_y(
                    baseline_y,
                    decoration.underline_position,
                    style.font_size,
                    thickness,
                ),
                width,
                thickness,
                color_override.unwrap_or(Color::new(255, 0, 0)),
                TextDecorationStyle::Wavy,
            );
        }
        if phase.paints_before_text() && decoration.grammar_error {
            push_page_margin_text_decoration_stroke(
                page,
                x,
                page_margin_used_underline_y(
                    baseline_y,
                    decoration.underline_position,
                    style.font_size,
                    thickness,
                ),
                width,
                thickness,
                color_override.unwrap_or(Color::new(0, 128, 0)),
                TextDecorationStyle::Wavy,
            );
        }
        if phase.paints_after_text() && decoration.line_through {
            push_page_margin_text_decoration_stroke(
                page,
                x,
                baseline_y + style.font_size * 0.35,
                width,
                thickness,
                color,
                decoration.style,
            );
        }
    }
}

fn push_page_margin_text_decoration_stroke(
    page: &mut Page,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
    style: TextDecorationStyle,
) {
    match style {
        TextDecorationStyle::Double if thickness >= 1.5 => {
            let stripe = (thickness / 3.0).max(0.5);
            push_page_margin_text_decoration_stroke(
                page,
                x,
                y + stripe,
                width,
                stripe,
                color,
                TextDecorationStyle::Solid,
            );
            push_page_margin_text_decoration_stroke(
                page,
                x,
                y - stripe,
                width,
                stripe,
                color,
                TextDecorationStyle::Solid,
            );
        }
        _ => {
            let dash = match style {
                TextDecorationStyle::Dotted => Some((thickness, thickness * 2.0)),
                TextDecorationStyle::Dashed => Some((thickness * 3.0, thickness * 2.0)),
                TextDecorationStyle::Wavy => Some((thickness * 1.5, thickness)),
                TextDecorationStyle::Solid | TextDecorationStyle::Double => None,
            };
            page.push_stroke_in_band(
                PaintBand::Inline,
                RenderedStroke {
                    x1: x,
                    y1: y,
                    x2: x + width,
                    y2: y,
                    width: thickness,
                    color,
                    dash,
                },
            );
        }
    }
}

fn page_margin_active_text_decoration_layers(style: &ComputedStyle) -> Vec<TextDecoration> {
    if !style.text_decoration_layers.is_empty() {
        return style.text_decoration_layers.clone();
    }
    if style.text_decoration.has_visible_line() {
        return vec![style.text_decoration];
    }
    Vec::new()
}

fn page_margin_used_underline_y(
    baseline_y: f32,
    position: TextUnderlinePosition,
    font_size: f32,
    thickness: f32,
) -> f32 {
    if position.under {
        baseline_y - font_size * 0.3 - thickness
    } else {
        baseline_y - thickness * 2.0
    }
}

#[derive(Debug, Clone, Copy)]
struct PageMarginTextShadowPaintPass {
    x_offset: f32,
    y_offset: f32,
    color: Color,
}

fn page_margin_text_shadow_paint_passes(
    shadow: TextShadow,
    color: Color,
) -> Vec<PageMarginTextShadowPaintPass> {
    if shadow.blur_radius <= 0.0 {
        return vec![PageMarginTextShadowPaintPass {
            x_offset: 0.0,
            y_offset: 0.0,
            color,
        }];
    }
    let radius = shadow.blur_radius.max(0.0);
    let samples = [
        (0.0, 0.0, 0.22),
        (1.0, 0.0, 0.08),
        (-1.0, 0.0, 0.08),
        (0.0, 1.0, 0.08),
        (0.0, -1.0, 0.08),
        (0.707, 0.707, 0.06),
        (-0.707, 0.707, 0.06),
        (0.707, -0.707, 0.06),
        (-0.707, -0.707, 0.06),
    ];
    samples
        .into_iter()
        .map(|(x, y, alpha)| PageMarginTextShadowPaintPass {
            x_offset: x * radius * 0.45,
            y_offset: y * radius * 0.45,
            color: Color {
                a: (color.a * alpha).clamp(0.0, 1.0),
                ..color
            },
        })
        .collect()
}

fn page_margin_character_receives_text_emphasis_mark(
    character: char,
    skip: TextEmphasisSkip,
) -> bool {
    if !character_receives_text_emphasis_mark(character) {
        return false;
    }
    if skip.spaces && character_is_text_decoration_spacer(character) {
        return false;
    }
    if skip.punctuation && character.is_ascii_punctuation() {
        return false;
    }
    if skip.symbols && character.is_ascii() && !character.is_ascii_alphanumeric() {
        return false;
    }
    if skip.narrow && character.is_ascii() {
        return false;
    }
    true
}
