use super::*;
use crate::css::LayerOrder;
use crate::units::LayoutSize;

pub(in crate::layout) struct PageMarginBoxSpec {
    pub(in crate::layout) name: String,
    pub(in crate::layout) declarations: Declarations,
    pub(in crate::layout) style: ComputedStyle,
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
                        rule.layer_order.clone(),
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
                        rule.layer_order.clone(),
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
    let initial_viewport = LayoutSize::new(
        context.initial_page_size.width(),
        context.initial_page_size.height(),
    );
    // Most pages have several generated margin boxes. Resolve their shared
    // page context only once, but keep it lazy so unmatched page-margin rules
    // do not trigger a page-context cascade.
    let mut resolved_page_style = None;
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
                        rule.layer_order.clone(),
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
        let page_style = resolved_page_style.get_or_insert_with(|| {
            let mut page_style = context.base_page_style.clone();
            css::apply_declarations_with_inheritance_source(
                &mut page_style,
                context.page_declarations,
                context.base_page_style,
            );
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
            page_style
        });
        let mut style = page_margin_style_inheriting_page_context(page_style);
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
        let (line_height, _, _) = style.line_height_value.clone().projected(style.font_size);
        style.line_height = line_height;
        style.finalize_computed_font_relative_lengths();
        style.rebuild_own_text_decoration_layer();
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
    style
}

pub(in crate::layout) fn page_context_style_from_options(options: &RenderOptions) -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    style.font_size = options.font_size();
    style.line_height_value = css::ComputedLineHeight::from_points(options.line_height());
    style.line_height = options.line_height();
    style
}

pub(in crate::layout) fn cascade_page_rule_declarations<'a>(
    declarations: impl Iterator<
        Item = (
            StylesheetOrigin,
            PageSpecificity,
            Option<LayerOrder>,
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
                layer_order: layer_order.clone(),
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
    candidates.sort_by(|left, right| compare_page_cascade_keys(&left.key, &right.key));

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct PageCascadeKey {
    pub(in crate::layout) important: bool,
    pub(in crate::layout) origin_rank: u8,
    pub(in crate::layout) specificity: PageSpecificity,
    pub(in crate::layout) rule_order: usize,
    pub(in crate::layout) declaration_order: usize,
    pub(in crate::layout) origin: StylesheetOrigin,
    pub(in crate::layout) layer_order: Option<LayerOrder>,
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

fn compare_page_cascade_keys(left: &PageCascadeKey, right: &PageCascadeKey) -> std::cmp::Ordering {
    left.origin_rank
        .cmp(&right.origin_rank)
        .then_with(|| {
            compare_layer_order(
                left.layer_order.as_ref(),
                right.layer_order.as_ref(),
                left.important,
            )
        })
        .then_with(|| left.specificity.cmp(&right.specificity))
        .then_with(|| left.rule_order.cmp(&right.rule_order))
        .then_with(|| left.declaration_order.cmp(&right.declaration_order))
}

fn compare_layer_order(
    left: Option<&LayerOrder>,
    right: Option<&LayerOrder>,
    important: bool,
) -> std::cmp::Ordering {
    match (important, left, right) {
        (false, Some(left), Some(right)) => left.cmp(right),
        (false, Some(_), None) => std::cmp::Ordering::Less,
        (false, None, Some(_)) => std::cmp::Ordering::Greater,
        (false, None, None) => std::cmp::Ordering::Equal,
        (true, Some(left), Some(right)) => right.cmp(left),
        (true, Some(_), None) => std::cmp::Ordering::Greater,
        (true, None, Some(_)) => std::cmp::Ordering::Less,
        (true, None, None) => std::cmp::Ordering::Equal,
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
