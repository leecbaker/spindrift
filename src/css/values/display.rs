use super::*;

/// Parses CSS Display outer/inner display values.
///
/// CSS Display Level 3 defines both legacy single-keyword values such as
/// `inline-block` and multi-keyword values such as `inline table`; standalone
/// `flow-root` computes to an outer block box with an inner flow-root
/// formatting context.
///
/// <https://www.w3.org/TR/css-display-3/#the-display-properties>
pub(crate) fn parse_display(value: &str, current: Display) -> Display {
    let lower = value.trim().to_ascii_lowercase();
    let parts = lower.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() {
        return current;
    }

    if let Some(parsed) = parse_display_legacy(&lower) {
        return parsed;
    }

    let mut outer = None;
    let mut inner = None;
    let mut list_item = false;
    for part in parts {
        match part {
            "block" => {
                if outer.replace(DisplayOuter::Block).is_some() {
                    return current;
                }
            }
            "inline" => {
                if outer.replace(DisplayOuter::Inline).is_some() {
                    return current;
                }
            }
            "run-in" => {
                if outer.replace(DisplayOuter::RunIn).is_some() {
                    return current;
                }
            }
            "flow" => {
                if inner.replace(DisplayInner::Flow).is_some() {
                    return current;
                }
            }
            "flow-root" => {
                if inner.replace(DisplayInner::FlowRoot).is_some() {
                    return current;
                }
            }
            "table" => {
                if inner.replace(DisplayInner::Table).is_some() {
                    return current;
                }
            }
            "flex" => {
                if inner.replace(DisplayInner::Flex).is_some() {
                    return current;
                }
            }
            "grid" => {
                if inner.replace(DisplayInner::Grid).is_some() {
                    return current;
                }
            }
            "list-item" => {
                if list_item {
                    return current;
                }
                list_item = true;
            }
            _ => return current,
        }
    }

    let outer = outer.unwrap_or(DisplayOuter::Block);
    let inner = inner.unwrap_or(DisplayInner::Flow);
    if list_item && !matches!(inner, DisplayInner::Flow | DisplayInner::FlowRoot) {
        return current;
    }

    if list_item {
        Display::list_item(outer, inner)
    } else {
        Display::new(outer, inner)
    }
}

fn parse_display_legacy(lower: &str) -> Option<Display> {
    match lower {
        "none" => Some(Display::NONE),
        "contents" => Some(Display::CONTENTS),
        "block" => Some(Display::BLOCK),
        "inline" => Some(Display::INLINE),
        "run-in" => Some(Display::RUN_IN),
        "flow-root" => Some(Display::new(DisplayOuter::Block, DisplayInner::FlowRoot)),
        "inline-block" => Some(Display::INLINE_BLOCK),
        "flex" => Some(Display::FLEX),
        "inline-flex" => Some(Display::INLINE_FLEX),
        "grid" => Some(Display::GRID),
        "inline-grid" => Some(Display::INLINE_GRID),
        "table" => Some(Display::TABLE),
        "inline-table" => Some(Display::INLINE_TABLE),
        "table-caption" => Some(Display::TABLE_CAPTION),
        "table-column-group" => Some(Display::TABLE_COLUMN_GROUP),
        "table-column" => Some(Display::TABLE_COLUMN),
        "table-header-group" => Some(Display::TABLE_HEADER_GROUP),
        "table-footer-group" => Some(Display::TABLE_FOOTER_GROUP),
        "table-row-group" => Some(Display::TABLE_ROW_GROUP),
        "table-row" => Some(Display::TABLE_ROW),
        "table-cell" => Some(Display::TABLE_CELL),
        _ => None,
    }
}
