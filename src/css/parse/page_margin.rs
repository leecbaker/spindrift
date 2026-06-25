use super::*;

/// Parses CSS Paged Media rules while preserving full raw `@page` bodies.
///
/// CSS Paged Media allows nested margin at-rules inside `@page`, which the
/// generic stylesheet parser does not expose as a single declaration block.
/// This scanner walks top-level/group-rule blocks, honoring CSS Conditional
/// `@media`/`@supports` and CSS Cascade layers before extracting page rules:
/// <https://www.w3.org/TR/css-page-3/#at-page-rule>,
/// <https://www.w3.org/TR/css-conditional-3/#condition-apis>, and
/// <https://www.w3.org/TR/css-cascade-5/#layering>.
pub(super) fn parse_page_rules(
    source: &str,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    origin: StylesheetOrigin,
    layer_names: &[String],
    initial_layer: Option<&str>,
) -> Vec<PageRule> {
    let layer_order = layer_names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut rules = Vec::new();
    let mut state = PageRuleScanState {
        anonymous_layer_count: 0,
        order: 0,
    };
    parse_page_rules_in_block(
        source,
        PageRuleScanContext {
            base_url,
            root_url,
            origin,
            layer_order: &layer_order,
        },
        initial_layer,
        &mut state,
        &mut rules,
    );
    rules
}

#[derive(Clone, Copy)]
struct PageRuleScanContext<'a> {
    base_url: Option<&'a Path>,
    root_url: Option<&'a Path>,
    origin: StylesheetOrigin,
    layer_order: &'a HashMap<&'a str, usize>,
}

struct PageRuleScanState {
    anonymous_layer_count: usize,
    order: usize,
}

fn parse_page_rules_in_block<'a>(
    source: &'a str,
    context: PageRuleScanContext<'a>,
    current_layer: Option<&str>,
    state: &mut PageRuleScanState,
    rules: &mut Vec<PageRule>,
) {
    let mut position = 0usize;
    while let Some(open) = find_next_top_level_open_brace(source, position) {
        let Some(close) = find_matching_brace_or_eof(source, open) else {
            break;
        };
        let segment_start = source[position..open]
            .rfind(['}', ';'])
            .map(|index| position + index + 1)
            .unwrap_or(position);
        let prelude_start = source[segment_start..open]
            .rfind('@')
            .map(|index| segment_start + index)
            .unwrap_or(segment_start);
        let prelude = source[prelude_start..open].trim();
        let body = &source[open + 1..close];
        parse_page_at_rule(prelude, body, context, current_layer, state, rules);
        position = close.saturating_add(1).min(source.len());
    }
}

fn parse_page_at_rule<'a>(
    prelude: &'a str,
    body: &'a str,
    context: PageRuleScanContext<'a>,
    current_layer: Option<&str>,
    state: &mut PageRuleScanState,
    rules: &mut Vec<PageRule>,
) {
    let Some(rest) = prelude.trim().strip_prefix('@') else {
        return;
    };
    let (name, at_prelude) = split_at_rule_name(rest);
    if name.eq_ignore_ascii_case("page") {
        rules.push(PageRule {
            origin: context.origin,
            selectors: parse_page_selectors(at_prelude),
            declarations: parse_declarations_with_urls(
                &strip_nested_page_rules(body),
                context.base_url,
                context.root_url,
            ),
            margin_boxes: parse_page_rule_margin_boxes(body, context.base_url, context.root_url),
            order: state.order,
            layer_order: current_layer
                .and_then(|name| context.layer_order.get(name))
                .copied(),
        });
        state.order = state.order.saturating_add(1);
    } else if name.eq_ignore_ascii_case("media") {
        if media_rule_applies(at_prelude) {
            parse_page_rules_in_block(body, context, current_layer, state, rules);
        }
    } else if name.eq_ignore_ascii_case("supports") {
        if supports_condition_applies(at_prelude) {
            parse_page_rules_in_block(body, context, current_layer, state, rules);
        }
    } else if name.eq_ignore_ascii_case("layer") && !at_prelude.contains(',') {
        let layer_name = if at_prelude.trim().is_empty() {
            let anonymous_name = format!("__anonymous_layer_{}", state.anonymous_layer_count);
            state.anonymous_layer_count = state.anonymous_layer_count.saturating_add(1);
            Some(anonymous_name)
        } else {
            qualify_page_layer_name(current_layer, at_prelude)
        };
        parse_page_rules_in_block(body, context, layer_name.as_deref(), state, rules);
    }
}

fn split_at_rule_name(rest: &str) -> (&str, &str) {
    let trimmed = rest.trim_start();
    let end = trimmed
        .find(|character: char| character.is_whitespace())
        .unwrap_or(trimmed.len());
    (&trimmed[..end], trimmed[end..].trim())
}

fn qualify_page_layer_name(parent: Option<&str>, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if let Some(parent) = parent
        && !parent.is_empty()
    {
        return Some(format!("{parent}.{name}"));
    }
    Some(name.to_string())
}

pub(super) fn parse_page_rule_margin_boxes(
    page_body: &str,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
) -> HashMap<String, Declarations> {
    let mut boxes = HashMap::new();
    for name in PAGE_MARGIN_BOX_NAMES {
        let at_name = format!("@{name}");
        let mut body_rest = page_body;
        while let Some(box_start) = find_margin_at_rule(body_rest, &at_name) {
            let box_rest = &body_rest[box_start + at_name.len()..];
            let Some(box_open_offset) = box_rest.find('{') else {
                break;
            };
            let box_open = box_start + at_name.len() + box_open_offset;
            let Some(box_close) = find_matching_brace(body_rest, box_open) else {
                break;
            };
            boxes
                .entry(name.to_string())
                .or_insert_with(Declarations::new)
                .extend(parse_declarations_with_urls(
                    &body_rest[box_open + 1..box_close],
                    base_url,
                    root_url,
                ));
            body_rest = &body_rest[box_close + 1..];
        }
    }
    boxes
}

/// Finds an exact page-margin at-rule name.
///
/// CSS Paged Media defines distinct at-rules such as `@bottom-right` and
/// `@bottom-right-corner`; matching must therefore stop at the at-keyword
/// boundary rather than using a simple substring prefix:
/// <https://www.w3.org/TR/css-page-3/#syntax-page-margin-box>.
fn find_margin_at_rule(source: &str, at_name: &str) -> Option<usize> {
    let mut search_start = 0usize;
    while let Some(relative) = source[search_start..].find(at_name) {
        let start = search_start + relative;
        let after = start + at_name.len();
        let boundary = source[after..]
            .chars()
            .next()
            .is_none_or(|character| character.is_whitespace() || character == '{');
        if boundary {
            return Some(start);
        }
        search_start = after;
    }
    None
}

pub(super) const PAGE_MARGIN_BOX_NAMES: &[&str] = &[
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

pub(super) fn parse_page_selectors(prelude: &str) -> Vec<PageSelector> {
    split_selector_list(prelude)
        .into_iter()
        .filter_map(parse_page_selector)
        .collect()
}

pub(super) fn parse_page_selector(selector: &str) -> Option<PageSelector> {
    let mut page_type = None;
    let mut pseudos = Vec::new();
    let mut rest = selector.trim();
    if rest.is_empty() {
        return Some(PageSelector { page_type, pseudos });
    }
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix(':') {
            let (pseudo, next) = split_page_selector_token(stripped);
            let pseudo = match pseudo.to_ascii_lowercase().as_str() {
                "first" => PagePseudo::First,
                "left" => PagePseudo::Left,
                "right" => PagePseudo::Right,
                "blank" => PagePseudo::Blank,
                value => {
                    let (a, b) = parse_page_nth_pseudo(value)?;
                    PagePseudo::Nth { a, b }
                }
            };
            pseudos.push(pseudo);
            rest = next;
            continue;
        }
        if page_type.is_some() {
            return None;
        }
        let (name, next) = split_page_selector_token(rest);
        if name.is_empty() || !name.chars().all(is_css_identifier_character) {
            return None;
        }
        page_type = Some(name.to_string());
        rest = next;
    }
    Some(PageSelector { page_type, pseudos })
}

/// Parses the GCPM `:nth()` page selector argument as an `an+b` sequence.
///
/// The grammar is the same sequence form used by structural pseudo-classes, but
/// page matching is evaluated against one-based generated page numbers:
/// <https://www.w3.org/TR/css-gcpm-3/#document-page-selectors>.
fn parse_page_nth_pseudo(value: &str) -> Option<(i32, i32)> {
    let argument = value.strip_prefix("nth(")?.strip_suffix(')')?;
    parse_an_plus_b(argument)
}

fn parse_an_plus_b(value: &str) -> Option<(i32, i32)> {
    let compact = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    match compact.as_str() {
        "even" => return Some((2, 0)),
        "odd" => return Some((2, 1)),
        _ => {}
    }
    let Some(n_index) = compact.find('n') else {
        return compact.parse::<i32>().ok().map(|b| (0, b));
    };
    if compact[n_index + 1..].contains('n') {
        return None;
    }
    let a = match &compact[..n_index] {
        "" | "+" => 1,
        "-" => -1,
        value => value.parse::<i32>().ok()?,
    };
    let b = match &compact[n_index + 1..] {
        "" => 0,
        value => value.parse::<i32>().ok()?,
    };
    Some((a, b))
}

pub(super) fn split_page_selector_token(value: &str) -> (&str, &str) {
    let end = value.find(':').unwrap_or(value.len());
    (&value[..end], &value[end..])
}

pub(super) fn is_css_identifier_character(character: char) -> bool {
    character == '-' || character == '_' || character.is_ascii_alphanumeric()
}

pub(crate) fn cascade_page_declarations(
    page_rules: &[PageRule],
    page_number: usize,
) -> Declarations {
    cascade_page_rule_declarations(page_rules.iter().filter_map(|rule| {
        rule.matching_specificity(page_number, None, false, Direction::Ltr)
            .map(|specificity| {
                (
                    rule.origin,
                    specificity,
                    rule.layer_order,
                    rule.order,
                    &rule.declarations,
                )
            })
    }))
}

pub(super) fn cascade_page_margin_boxes(
    page_rules: &[PageRule],
    page_number: usize,
) -> HashMap<String, Declarations> {
    let mut boxes = HashMap::new();
    for name in PAGE_MARGIN_BOX_NAMES {
        let declarations = cascade_page_rule_declarations(page_rules.iter().filter_map(|rule| {
            let specificity =
                rule.matching_specificity(page_number, None, false, Direction::Ltr)?;
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
        if !declarations.is_empty() {
            boxes.insert((*name).to_string(), declarations);
        }
    }
    boxes
}

/// Cascades declarations in the CSS page context.
///
/// CSS Paged Media 3 adds page-selector specificity to the page context, while
/// CSS Cascade Level 5 keeps origin/importance/layers before specificity:
/// <https://www.w3.org/TR/css-page-3/#cascading-and-page-context> and
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
pub(super) fn cascade_page_rule_declarations<'a>(
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
                origin_rank: super::super::cascade::origin_importance_rank(origin, important),
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
                !super::super::cascade::declarations_affect_same_property(
                    &existing.name,
                    &candidate.name,
                ) || !same_or_stronger_reverted_page_origin(existing, &candidate)
            });
        } else if page_declaration_is_revert_layer(&candidate.value) {
            active.retain(|existing| {
                !super::super::cascade::declarations_affect_same_property(
                    &existing.name,
                    &candidate.name,
                ) || !same_page_cascade_layer(existing, &candidate)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PageCascadeKey {
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
    trim_css_value(value).eq_ignore_ascii_case("revert-layer")
}

fn page_declaration_is_revert(value: &str) -> bool {
    trim_css_value(value).eq_ignore_ascii_case("revert")
}

/// Returns whether a prior page declaration is erased by a later `revert`.
///
/// CSS Paged Media uses normal cascade origins for page-context declarations,
/// and CSS Cascade Level 5 defines `revert` in terms of those origins:
/// <https://www.w3.org/TR/css-page-3/#cascading-and-page-context> and
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

pub(super) fn strip_nested_page_rules(body: &str) -> String {
    let mut output = String::with_capacity(body.len());
    let mut position = 0usize;
    while let Some(at_start) = body[position..].find('@') {
        let at_start = position + at_start;
        output.push_str(&body[position..at_start]);
        let Some(open_offset) = body[at_start..].find('{') else {
            position = at_start + 1;
            continue;
        };
        let open = at_start + open_offset;
        let Some(close) = find_matching_brace(body, open) else {
            position = at_start + 1;
            continue;
        };
        position = close + 1;
    }
    output.push_str(&body[position..]);
    output
}
