use super::*;
use crate::css::component_values::{
    css_leading_function_matching, css_leading_ident, parse_css_string_token,
    split_css_top_level_delimiter,
};
use crate::text::is_css_collapsible_whitespace;
use crate::text::trim_css_collapsible_whitespace;

#[derive(Debug, Clone, PartialEq)]
enum PageContentPart {
    Text(String),
    Contents,
    Attr {
        fallback: Option<String>,
    },
    Image {
        image: BackgroundImage,
    },
    Quote(GeneratedQuote),
    Leader(String),
    PageCounter {
        style: Option<ListStyleType>,
    },
    PagesCounter {
        style: Option<ListStyleType>,
    },
    Counter {
        name: String,
        style: Option<ListStyleType>,
    },
    Counters {
        name: String,
        separator: String,
        style: Option<ListStyleType>,
    },
    TargetCounter {
        target: css::TargetReference,
        name: String,
        style: Option<ListStyleType>,
    },
    TargetText {
        target: css::TargetReference,
        keyword: TargetTextKeyword,
    },
    NamedString {
        name: String,
        keyword: String,
    },
    RunningElement {
        name: String,
        keyword: String,
    },
}

/// Resolved generated content for one page-margin box.
///
/// CSS Paged Media creates page-margin boxes from the `content` property, and
/// CSS GCPM adds page-local functions such as `string()` and `element()`.
/// Resolving those page-only functions before layout keeps inline content as
/// CSS Content primitives while preserving `element()` as an embedded
/// running-element item:
/// <https://www.w3.org/TR/css-page-3/#margin-boxes> and
/// <https://www.w3.org/TR/css-gcpm-3/#content-list>.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum PageMarginContentItem {
    Inline(GeneratedContentPart),
    EmbeddedRunningElement(Box<RunningElementCapture>),
    /// A page-associated counter captured by `string-set`.
    ///
    /// Unlike ordinary counters, `page` and `pages` are only known after
    /// pagination. Keep their authored syntax until the named string is
    /// resolved, then evaluate it in the page context of its source
    /// assignment rather than the page margin box consuming `string()`.
    /// <https://www.w3.org/TR/css-gcpm-3/#setting-named-strings>
    NamedStringPageCounter {
        name: String,
        separator: Option<String>,
        style: Option<ListStyleType>,
    },
    TargetCounter {
        target: css::TargetReference,
        name: String,
        style: Option<ListStyleType>,
    },
    TargetText {
        target: css::TargetReference,
        keyword: css::NamedStringTargetTextKeyword,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ResolvedPageContent {
    pub(super) items: Vec<PageMarginContentItem>,
}

impl ResolvedPageContent {
    pub(super) fn is_empty(&self) -> bool {
        self.items.iter().all(|item| match item {
            PageMarginContentItem::Inline(part) => match part {
                GeneratedContentPart::Text(text) | GeneratedContentPart::Leader(text) => {
                    trim_css_collapsible_whitespace(text).is_empty()
                }
                GeneratedContentPart::Quote(_) => true,
                GeneratedContentPart::Contents
                | GeneratedContentPart::Attr { .. }
                | GeneratedContentPart::Counter { .. }
                | GeneratedContentPart::Counters { .. }
                | GeneratedContentPart::Image { .. }
                | GeneratedContentPart::TargetCounter { .. }
                | GeneratedContentPart::TargetText { .. } => false,
            },
            PageMarginContentItem::EmbeddedRunningElement(_) => false,
            PageMarginContentItem::NamedStringPageCounter { .. } => false,
            PageMarginContentItem::TargetCounter { .. }
            | PageMarginContentItem::TargetText { .. } => false,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetTextKeyword {
    Content,
    Before,
    After,
    FirstLetter,
}

/// Page-local state used to resolve page-margin generated content.
///
/// CSS Paged Media and GCPM generated-content functions are resolved after
/// pagination for page counters, named strings, running elements, and target
/// cross references:
/// <https://www.w3.org/TR/css-page-3/#page-based-counters> and
/// <https://www.w3.org/TR/css-gcpm-3/#cross-references>.
pub(super) struct PageContentResolveContext<'a> {
    pub page_number: usize,
    pub total_pages: usize,
    pub page_index: usize,
    pub base_url: Option<&'a url::Url>,
    pub root_url: Option<&'a url::Url>,
    pub page_named_strings: &'a [HashMap<String, Vec<NamedStringAssignment>>],
    pub page_running_elements: &'a [HashMap<String, Vec<NamedStringAssignment>>],
    pub page_anchors: &'a HashMap<String, usize>,
    pub page_anchor_text: &'a HashMap<String, AnchorText>,
    pub counter_styles: &'a HashMap<String, CounterStyleRule>,
    pub page_counters: &'a HashMap<String, i32>,
    pub page_counters_by_page: &'a [HashMap<String, i32>],
    pub used_color_scheme: css::UsedColorScheme,
    pub image_set_resolution_dppx: f32,
}

/// Resolves a page-margin `content` value to paintable generated-content parts.
///
/// Page-margin boxes use the normal CSS `content` grammar plus GCPM page
/// functions. This function resolves page-dependent values to text while
/// preserving ordinary generated-content items such as `url()`, quotes, and
/// leaders for margin-box layout/paint:
/// <https://www.w3.org/TR/css-page-3/#page-margin-boxes> and
/// <https://www.w3.org/TR/css-content-3/#typedef-content-list>.
pub(super) fn resolve_page_content_parts(
    value: &str,
    context: PageContentResolveContext<'_>,
) -> Option<ResolvedPageContent> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("normal") || value.eq_ignore_ascii_case("none") {
        return None;
    }

    // CSS Generated Content for Paged Media uses a content-list in page margin
    // boxes. Keep it typed here so new functions such as target-counter() can
    // be added without modifying the painting code.
    // https://www.w3.org/TR/css-gcpm-3/#content-list
    let parts = parse_page_content(value)?;
    let mut output = Vec::new();
    for part in parts {
        match part {
            PageContentPart::Text(text) => push_resolved_text(&mut output, &text),
            PageContentPart::Contents => {}
            PageContentPart::Attr { fallback } => {
                if let Some(fallback) = fallback {
                    push_resolved_text(&mut output, &fallback);
                }
            }
            PageContentPart::Image { image } => {
                let mut image =
                    page_content_image_with_context_urls(image, context.base_url, context.root_url);
                let image = if image.resolve_for_context(css::ImageSelectionContext {
                    used_color_scheme: context.used_color_scheme,
                    resolution_dppx: context.image_set_resolution_dppx,
                }) {
                    css::ComputedImage::image(image)
                } else {
                    css::ComputedImage::Invalid
                };
                output.push(PageMarginContentItem::Inline(GeneratedContentPart::Image {
                    image,
                }))
            }
            PageContentPart::Quote(quote) => output.push(PageMarginContentItem::Inline(
                GeneratedContentPart::Quote(quote),
            )),
            PageContentPart::Leader(text) => output.push(PageMarginContentItem::Inline(
                GeneratedContentPart::Leader(text),
            )),
            PageContentPart::PageCounter { style } => {
                let value = context
                    .page_counters
                    .get("page")
                    .cloned()
                    .unwrap_or(context.page_number as i32);
                push_resolved_text(
                    &mut output,
                    &format_page_counter_i32(value, style, context.counter_styles),
                );
            }
            PageContentPart::PagesCounter { style } => {
                push_resolved_text(
                    &mut output,
                    &format_page_counter_value(context.total_pages, style, context.counter_styles),
                );
            }
            PageContentPart::Counter { name, style } => {
                let value = if name.eq_ignore_ascii_case("page") {
                    context
                        .page_counters
                        .get("page")
                        .cloned()
                        .unwrap_or(context.page_number as i32)
                } else if name.eq_ignore_ascii_case("pages") {
                    context.total_pages as i32
                } else {
                    context.page_counters.get(&name).cloned().unwrap_or(0)
                };
                push_resolved_text(
                    &mut output,
                    &format_page_counter_i32(value, style, context.counter_styles),
                );
            }
            PageContentPart::Counters {
                name,
                separator,
                style,
            } => {
                let value = if name.eq_ignore_ascii_case("page") {
                    context
                        .page_counters
                        .get("page")
                        .cloned()
                        .unwrap_or(context.page_number as i32)
                } else if name.eq_ignore_ascii_case("pages") {
                    context.total_pages as i32
                } else {
                    context.page_counters.get(&name).cloned().unwrap_or(0)
                };
                let counter = format_page_counter_i32(value, style, context.counter_styles);
                if !counter.is_empty() {
                    push_resolved_text(&mut output, &counter);
                } else {
                    push_resolved_text(&mut output, &separator);
                }
            }
            PageContentPart::TargetCounter {
                target,
                name,
                style,
            } => {
                if let Some(value) = resolve_target_counter_value(
                    &target,
                    &name,
                    style,
                    context.page_anchors,
                    context.total_pages,
                    context.counter_styles,
                ) {
                    push_resolved_text(&mut output, &value);
                }
            }
            PageContentPart::TargetText { target, keyword } => {
                if let Some(value) =
                    resolve_target_text_value(&target, keyword, context.page_anchor_text)
                {
                    push_resolved_text(&mut output, &value);
                }
            }
            PageContentPart::NamedString { name, keyword } => {
                if let Some(assignment) = resolve_page_assignment(
                    &name,
                    &keyword,
                    context.page_index,
                    context.page_named_strings,
                ) {
                    append_assignment_generated_content(&mut output, assignment, &context);
                }
            }
            PageContentPart::RunningElement { name, keyword } => {
                if let Some(assignment) = resolve_page_assignment(
                    &name,
                    &keyword,
                    context.page_index,
                    context.page_running_elements,
                ) {
                    append_assignment_generated_content(&mut output, assignment, &context);
                }
            }
        }
    }
    Some(ResolvedPageContent { items: output })
}

fn append_assignment_generated_content(
    output: &mut Vec<PageMarginContentItem>,
    assignment: &NamedStringAssignment,
    context: &PageContentResolveContext<'_>,
) {
    match &assignment.value {
        PageAssignmentValue::GeneratedContent(parts) => {
            append_resolved_items(output, parts, assignment.placement.page_index, context);
        }
        PageAssignmentValue::RunningElement(capture) => {
            output.push(PageMarginContentItem::EmbeddedRunningElement(
                capture.clone(),
            ));
        }
    }
}

fn append_resolved_items(
    output: &mut Vec<PageMarginContentItem>,
    items: &[PageMarginContentItem],
    source_page_index: usize,
    context: &PageContentResolveContext<'_>,
) {
    for item in items {
        match item {
            PageMarginContentItem::Inline(GeneratedContentPart::Text(text)) => {
                push_resolved_text(output, text)
            }
            PageMarginContentItem::Inline(GeneratedContentPart::TargetCounter {
                target,
                name,
                style,
            }) => {
                if let Some(value) = resolve_target_counter_value(
                    target,
                    name,
                    style.clone(),
                    context.page_anchors,
                    context.total_pages,
                    context.counter_styles,
                ) {
                    push_resolved_text(output, &value);
                }
            }
            PageMarginContentItem::Inline(GeneratedContentPart::TargetText { target, keyword }) => {
                if let Some(value) = resolve_named_string_target_text_value(
                    target,
                    *keyword,
                    context.page_anchor_text,
                ) {
                    push_resolved_text(output, &value);
                }
            }
            PageMarginContentItem::TargetCounter {
                target,
                name,
                style,
            } => {
                if let Some(value) = resolve_target_counter_value(
                    target,
                    name,
                    style.clone(),
                    context.page_anchors,
                    context.total_pages,
                    context.counter_styles,
                ) {
                    push_resolved_text(output, &value);
                }
            }
            PageMarginContentItem::TargetText { target, keyword } => {
                if let Some(value) = resolve_named_string_target_text_value(
                    target,
                    *keyword,
                    context.page_anchor_text,
                ) {
                    push_resolved_text(output, &value);
                }
            }
            PageMarginContentItem::NamedStringPageCounter {
                name,
                separator,
                style,
            } => {
                let value = if name.eq_ignore_ascii_case("pages") {
                    context.total_pages as i32
                } else {
                    context
                        .page_counters_by_page
                        .get(source_page_index)
                        .and_then(|counters| counters.get(name))
                        .cloned()
                        .unwrap_or(source_page_index.saturating_add(1) as i32)
                };
                let counter = format_page_counter_i32(value, style.clone(), context.counter_styles);
                if let Some(separator) = separator {
                    push_resolved_text(output, &counter);
                    if counter.is_empty() {
                        push_resolved_text(output, separator);
                    }
                } else {
                    push_resolved_text(output, &counter);
                }
            }
            _ => output.push(item.clone()),
        }
    }
}

fn push_resolved_text(output: &mut Vec<PageMarginContentItem>, value: &str) {
    let text = decode_css_escapes(value);
    if text.is_empty() {
        return;
    }
    match output.last_mut() {
        Some(PageMarginContentItem::Inline(GeneratedContentPart::Text(previous))) => {
            previous.push_str(&text)
        }
        _ => output.push(PageMarginContentItem::Inline(GeneratedContentPart::Text(
            text,
        ))),
    }
}

fn page_content_image_with_context_urls(
    mut image: BackgroundImage,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> BackgroundImage {
    apply_page_content_image_urls(&mut image, base_url, root_url);
    image
}

fn apply_page_content_image_urls(
    image: &mut BackgroundImage,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) {
    match image {
        BackgroundImage::LightDark(branches) => {
            apply_page_content_image_urls(&mut branches.light, base_url, root_url);
            apply_page_content_image_urls(&mut branches.dark, base_url, root_url);
        }
        BackgroundImage::ImageSet(set) => {
            for option in &mut set.options {
                apply_page_content_image_urls(&mut option.image, base_url, root_url);
            }
        }
        BackgroundImage::SelectedImageSet { image, .. } => {
            apply_page_content_image_urls(image, base_url, root_url)
        }
        BackgroundImage::Url(css::ImageUrl {
            base_url: image_base_url,
            root_url: image_root_url,
            ..
        }) => {
            if image_base_url.is_none() {
                *image_base_url = base_url.cloned();
            }
            if image_root_url.is_none() {
                *image_root_url = root_url.cloned();
            }
        }
        BackgroundImage::ImageFunction(function) => {
            if let Some(source) = &mut function.source {
                if source.base_url.is_none() {
                    source.base_url = base_url.cloned();
                }
                if source.root_url.is_none() {
                    source.root_url = root_url.cloned();
                }
            }
        }
        BackgroundImage::LinearGradient(_)
        | BackgroundImage::RadialGradient(_)
        | BackgroundImage::ConicGradient(_)
        | BackgroundImage::CssColor(_) => {}
    }
}

fn format_page_counter_value(
    value: usize,
    style: Option<ListStyleType>,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> String {
    let value = i32::try_from(value).unwrap_or(i32::MAX);
    format_page_counter_i32(value, style, counter_styles)
}

fn format_page_counter_i32(
    value: i32,
    style: Option<ListStyleType>,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> String {
    let style = style.unwrap_or(ListStyleType::Decimal);
    list::counter_text(style, value, counter_styles).unwrap_or_else(|| value.to_string())
}

fn resolve_target_counter_value(
    target: &css::TargetReference,
    name: &str,
    style: Option<ListStyleType>,
    page_anchors: &HashMap<String, usize>,
    total_pages: usize,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<String> {
    if name.eq_ignore_ascii_case("pages") {
        return Some(format_page_counter_value(
            total_pages,
            style,
            counter_styles,
        ));
    }
    if !name.eq_ignore_ascii_case("page") {
        return None;
    }
    let target = target.literal_fragment_id()?;
    let page_index = *page_anchors.get(target)?;
    Some(format_page_counter_value(
        page_index + 1,
        style,
        counter_styles,
    ))
}

fn resolve_target_text_value(
    target: &css::TargetReference,
    keyword: TargetTextKeyword,
    page_anchor_text: &HashMap<String, AnchorText>,
) -> Option<String> {
    let target = target.literal_fragment_id()?;
    let text = page_anchor_text.get(target)?;
    Some(match keyword {
        TargetTextKeyword::Content => text.content.clone(),
        TargetTextKeyword::Before => text.before.clone(),
        TargetTextKeyword::After => text.after.clone(),
        TargetTextKeyword::FirstLetter => text
            .content
            .chars()
            .next()
            .map(|character| character.to_string())
            .unwrap_or_default(),
    })
}

fn resolve_named_string_target_text_value(
    target: &css::TargetReference,
    keyword: css::NamedStringTargetTextKeyword,
    page_anchor_text: &HashMap<String, AnchorText>,
) -> Option<String> {
    let keyword = match keyword {
        css::NamedStringTargetTextKeyword::Content => TargetTextKeyword::Content,
        css::NamedStringTargetTextKeyword::Before => TargetTextKeyword::Before,
        css::NamedStringTargetTextKeyword::After => TargetTextKeyword::After,
        css::NamedStringTargetTextKeyword::FirstLetter => TargetTextKeyword::FirstLetter,
    };
    resolve_target_text_value(target, keyword, page_anchor_text)
}

fn parse_page_content(value: &str) -> Option<Vec<PageContentPart>> {
    let mut rest = value.trim();
    let mut parts = Vec::new();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some((text, tail)) = parse_css_string_token(rest) {
            parts.push(PageContentPart::Text(text));
            rest = tail;
        } else if let Some((image, tail)) = parse_page_image_token(rest) {
            parts.push(PageContentPart::Image { image });
            rest = tail;
        } else if let Some((attr, tail)) = parse_page_attr_function(rest) {
            parts.push(attr);
            rest = tail;
        } else if starts_with_ident(rest, "contents") {
            parts.push(PageContentPart::Contents);
            rest = &rest["contents".len()..];
        } else if let Some((quote, tail)) = parse_generated_quote_token(rest) {
            parts.push(PageContentPart::Quote(quote));
            rest = tail;
        } else if let Some((leader, tail)) = parse_generated_leader_function(rest) {
            parts.push(PageContentPart::Leader(leader));
            rest = tail;
        } else if let Some((part, tail)) = parse_content_function(rest) {
            parts.push(part);
            rest = tail;
        } else {
            return None;
        }
    }
    Some(parts)
}

fn parse_page_attr_function(value: &str) -> Option<(PageContentPart, &str)> {
    let (argument, tail) = css_leading_function_matching(value, "attr")?;
    let mut parts = split_css_top_level_delimiter(argument, ',');
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    if parts.remove(0).trim().is_empty() {
        return None;
    }
    let fallback = parts
        .first()
        .and_then(|fallback| parse_css_string_token(fallback.trim()).map(|(text, _)| text));
    Some((PageContentPart::Attr { fallback }, tail))
}

fn parse_page_image_token(value: &str) -> Option<(BackgroundImage, &str)> {
    if let Some((src, tail)) = css::parse_css_url_token(value) {
        return Some((
            BackgroundImage::Url(css::ImageUrl {
                href: src,
                base_url: None,
                root_url: None,
                request_modifiers: css::RequestUrlModifiers::default(),
            }),
            tail,
        ));
    }
    let (name, arguments, tail) = crate::css::component_values::css_leading_function(value)?;
    if [
        "image-set",
        "-webkit-image-set",
        "light-dark",
        "image",
        "linear-gradient",
        "repeating-linear-gradient",
        "radial-gradient",
        "repeating-radial-gradient",
        "conic-gradient",
        "repeating-conic-gradient",
    ]
    .iter()
    .any(|known| name.eq_ignore_ascii_case(known))
    {
        let image_text = format!("{name}({arguments})");
        let image = css::parse_background_image(&image_text, None, None)?;
        return Some((image, tail));
    }
    None
}

fn starts_with_ident(value: &str, ident: &str) -> bool {
    css_leading_ident(value).is_some_and(|(found, _)| found.eq_ignore_ascii_case(ident))
}

fn parse_generated_quote_token(value: &str) -> Option<(GeneratedQuote, &str)> {
    let (ident, tail) = css_leading_ident(value)?;
    let quote = match ident.to_ascii_lowercase().as_str() {
        "open-quote" => GeneratedQuote::Open,
        "close-quote" => GeneratedQuote::Close,
        "no-open-quote" => GeneratedQuote::NoOpen,
        "no-close-quote" => GeneratedQuote::NoClose,
        _ => return None,
    };
    Some((quote, tail))
}

fn parse_generated_leader_function(value: &str) -> Option<(String, &str)> {
    let (argument, tail) = css_leading_function_matching(value, "leader")?;
    let argument = argument.trim();
    let leader = if let Some((text, string_tail)) = parse_css_string_token(argument) {
        if !string_tail.trim().is_empty() {
            return None;
        }
        text
    } else {
        match argument.to_ascii_lowercase().as_str() {
            "dotted" => ".".to_string(),
            "solid" => "_".to_string(),
            "space" => " ".to_string(),
            _ => return None,
        }
    };
    Some((leader, tail))
}

fn parse_content_function(value: &str) -> Option<(PageContentPart, &str)> {
    if let Some((argument, tail)) = css_leading_function_matching(value, "counter") {
        return parse_counter_function(argument).map(|part| (part, tail));
    }
    if let Some((argument, tail)) = css_leading_function_matching(value, "counters") {
        return parse_counters_function(argument).map(|part| (part, tail));
    }
    if let Some((argument, tail)) = css_leading_function_matching(value, "target-counter") {
        return parse_target_counter_function(argument).map(|part| (part, tail));
    }
    if let Some((argument, tail)) = css_leading_function_matching(value, "target-text") {
        return parse_target_text_function(argument).map(|part| (part, tail));
    }
    if let Some((argument, tail)) = css_leading_function_matching(value, "string") {
        let (name, keyword) = parse_named_assignment_arguments(argument)?;
        return Some((PageContentPart::NamedString { name, keyword }, tail));
    }
    if let Some((argument, tail)) = css_leading_function_matching(value, "element") {
        let (name, keyword) = parse_named_assignment_arguments(argument)?;
        return Some((PageContentPart::RunningElement { name, keyword }, tail));
    }
    None
}

fn parse_counters_function(argument: &str) -> Option<PageContentPart> {
    let arguments = split_css_top_level_delimiter(argument, ',');
    if !(2..=3).contains(&arguments.len()) {
        return None;
    }
    let name = arguments[0].trim();
    if name.is_empty() {
        return None;
    }
    let separator = parse_css_string_token(arguments[1].trim())?.0;
    let style = if let Some(argument) = arguments.get(2) {
        Some(css::parse_list_style_type(argument.trim())?)
    } else {
        None
    };
    Some(PageContentPart::Counters {
        name: name.to_string(),
        separator,
        style,
    })
}

fn parse_counter_function(argument: &str) -> Option<PageContentPart> {
    let arguments = split_css_top_level_delimiter(argument, ',');
    let name = arguments.first()?.trim();
    let style = if let Some(argument) = arguments.get(1) {
        Some(css::parse_list_style_type(argument.trim())?)
    } else {
        None
    };
    if arguments.len() > 2 {
        return None;
    }
    if name.eq_ignore_ascii_case("page") {
        Some(PageContentPart::PageCounter { style })
    } else if name.eq_ignore_ascii_case("pages") {
        Some(PageContentPart::PagesCounter { style })
    } else {
        Some(PageContentPart::Counter {
            name: name.to_string(),
            style,
        })
    }
}

/// Parses a generated-content `target-counter()` cross-reference.
///
/// CSS Generated Content for Paged Media defines
/// `target-counter(<target>, <counter-name>, <counter-style>?)`. This renderer
/// resolves page-associated counters for page-margin content while preserving
/// the typed target for the later, post-pagination resolution step:
/// <https://www.w3.org/TR/css-gcpm-3/#target-counter>.
fn parse_target_counter_function(argument: &str) -> Option<PageContentPart> {
    let arguments = split_css_top_level_delimiter(argument, ',');
    if !(2..=3).contains(&arguments.len()) {
        return None;
    }
    let target = parse_target_reference(arguments[0].trim())?;
    let name = arguments[1].trim();
    if name.is_empty() {
        return None;
    }
    let style = if let Some(argument) = arguments.get(2) {
        Some(css::parse_list_style_type(argument.trim())?)
    } else {
        None
    };
    Some(PageContentPart::TargetCounter {
        target,
        name: name.to_string(),
        style,
    })
}

/// Parses a generated-content `target-text()` cross-reference.
///
/// CSS Generated Content for Paged Media defines
/// `target-text(<target>, content | before | after | first-letter?)` for text
/// taken from a target element:
/// <https://www.w3.org/TR/css-gcpm-3/#target-text>.
fn parse_target_text_function(argument: &str) -> Option<PageContentPart> {
    let arguments = split_css_top_level_delimiter(argument, ',');
    if !(1..=2).contains(&arguments.len()) {
        return None;
    }
    let target = parse_target_reference(arguments[0].trim())?;
    let keyword = arguments
        .get(1)
        .map(|argument| parse_target_text_keyword(argument.trim()))
        .unwrap_or(Some(TargetTextKeyword::Content))?;
    Some(PageContentPart::TargetText { target, keyword })
}

fn parse_target_text_keyword(value: &str) -> Option<TargetTextKeyword> {
    match value.to_ascii_lowercase().as_str() {
        "content" => Some(TargetTextKeyword::Content),
        "before" => Some(TargetTextKeyword::Before),
        "after" => Some(TargetTextKeyword::After),
        "first-letter" => Some(TargetTextKeyword::FirstLetter),
        _ => None,
    }
}

fn parse_target_reference(value: &str) -> Option<css::TargetReference> {
    if let Some((text, tail)) = parse_css_string_token(value)
        && tail.trim().is_empty()
    {
        return Some(css::TargetReference::Fragment(text));
    }
    if let Some((target, tail)) = css::parse_css_url_token(value) {
        if !tail.trim().is_empty() {
            return None;
        }
        return Some(css::TargetReference::Fragment(target));
    }
    value
        .strip_prefix('#')
        .filter(|target| !target.trim().is_empty())
        .map(|target| css::TargetReference::Fragment(format!("#{target}")))
}

fn parse_named_assignment_arguments(argument: &str) -> Option<(String, String)> {
    let arguments = split_css_top_level_delimiter(argument, ',');
    let name = arguments
        .first()?
        .trim()
        .trim_matches('"')
        .trim_matches('\'');
    if name.is_empty() {
        return None;
    }
    let keyword = arguments
        .get(1)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("first");
    Some((name.to_string(), keyword.to_string()))
}

fn resolve_page_assignment<'a>(
    name: &str,
    keyword: &str,
    page_index: usize,
    page_assignments: &'a [HashMap<String, Vec<NamedStringAssignment>>],
) -> Option<&'a NamedStringAssignment> {
    if let Some(assignments) = page_assignments
        .get(page_index)
        .and_then(|strings| strings.get(name))
        .filter(|assignments| !assignments.is_empty())
    {
        return match keyword.to_ascii_lowercase().as_str() {
            "first" => assignments.first(),
            "last" => assignments.last(),
            "start" => assignments
                .first()
                .filter(|assignment| {
                    assignment.placement.page_index == page_index
                        && assignment_starts_page_fragment(assignment)
                })
                .or_else(|| previous_page_assignment(name, page_index, page_assignments)),
            "first-except" => None,
            _ => assignments.first(),
        };
    }
    previous_page_assignment(name, page_index, page_assignments)
}

fn previous_page_assignment<'a>(
    name: &str,
    page_index: usize,
    page_assignments: &'a [HashMap<String, Vec<NamedStringAssignment>>],
) -> Option<&'a NamedStringAssignment> {
    page_assignments
        .iter()
        .take(page_index)
        .rev()
        .find_map(|strings| strings.get(name).and_then(|assignments| assignments.last()))
}

fn assignment_starts_page_fragment(assignment: &NamedStringAssignment) -> bool {
    assignment.placement.starts_page_fragment && assignment.placement.border_box.is_some()
}

/// Decode escapes in already-selected page-generated text.
///
/// This operates on stored text fragments after CSS declaration parsing, not
/// on CSS component values. It remains local to the page-generated-text
/// grammar because feeding arbitrary text back through a CSS tokenizer would
/// incorrectly assign identifier or string-token semantics to it.
fn decode_css_escapes(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 {
            let Some(next) = chars.peek().cloned() else {
                break;
            };
            if next.is_ascii_hexdigit() {
                hex.push(next);
                chars.next();
            } else {
                break;
            }
        }
        if !hex.is_empty() {
            if let Ok(codepoint) = u32::from_str_radix(&hex, 16)
                && let Some(decoded) = char::from_u32(codepoint)
            {
                output.push(decoded);
            }
            if chars
                .peek()
                .is_some_and(|next| is_css_collapsible_whitespace(*next))
            {
                chars.next();
            }
        } else if let Some(next) = chars.next() {
            output.push(match next {
                'A' | 'a' => '\n',
                other => other,
            });
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typed_page_content_parts() {
        assert_eq!(
            parse_page_content(r#""Page " counter(page) " / " counter(pages)"#).unwrap(),
            vec![
                PageContentPart::Text("Page ".to_string()),
                PageContentPart::PageCounter { style: None },
                PageContentPart::Text(" / ".to_string()),
                PageContentPart::PagesCounter { style: None }
            ]
        );
        assert_eq!(
            parse_page_content(
                r#"counter(page, upper-roman) "/" counter(pages, decimal-leading-zero)"#
            )
            .unwrap(),
            vec![
                PageContentPart::PageCounter {
                    style: Some(ListStyleType::Named("upper-roman".to_string())),
                },
                PageContentPart::Text("/".to_string()),
                PageContentPart::PagesCounter {
                    style: Some(ListStyleType::Named("decimal-leading-zero".to_string())),
                }
            ]
        );
        assert_eq!(
            parse_page_content(r#"counter(foo, lower-alpha)"#).unwrap(),
            vec![PageContentPart::Counter {
                name: "foo".to_string(),
                style: Some(ListStyleType::Named("lower-alpha".to_string())),
            }]
        );
        assert_eq!(
            parse_page_content(r#"counters(foo, ".", lower-roman)"#).unwrap(),
            vec![PageContentPart::Counters {
                name: "foo".to_string(),
                separator: ".".to_string(),
                style: Some(ListStyleType::Named("lower-roman".to_string())),
            }]
        );
        assert_eq!(
            parse_page_content(r#"target-counter(url(#chapter), page, lower-roman)"#).unwrap(),
            vec![PageContentPart::TargetCounter {
                target: css::TargetReference::Fragment("#chapter".to_string()),
                name: "page".to_string(),
                style: Some(ListStyleType::Named("lower-roman".to_string())),
            }]
        );
        assert_eq!(
            parse_page_content(r##"target-text("#chapter", before)"##).unwrap(),
            vec![PageContentPart::TargetText {
                target: css::TargetReference::Fragment("#chapter".to_string()),
                keyword: TargetTextKeyword::Before,
            }]
        );
        assert_eq!(
            parse_page_content(r#"string(heading, last) " " element(header)"#).unwrap(),
            vec![
                PageContentPart::NamedString {
                    name: "heading".to_string(),
                    keyword: "last".to_string()
                },
                PageContentPart::Text(" ".to_string()),
                PageContentPart::RunningElement {
                    name: "header".to_string(),
                    keyword: "first".to_string()
                }
            ]
        );
    }
}
