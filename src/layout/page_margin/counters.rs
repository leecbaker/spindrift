use super::*;

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
