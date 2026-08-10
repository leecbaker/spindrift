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
    let Some(parts) = crate::css::component_values::try_split_css_component_values(value) else {
        return current;
    };
    let Some(parts) = parts
        .into_iter()
        .map(crate::css::component_values::css_single_ident)
        .collect::<Option<Vec<_>>>()
    else {
        return current;
    };
    if parts.is_empty() {
        return current;
    }

    if let [part] = parts.as_slice()
        && let Some(parsed) = parse_display_legacy(&part.to_ascii_lowercase())
    {
        return parsed;
    }

    let mut outer = None;
    let mut inner = None;
    // CSS Display Level 4 adds `math` as an inner display type. Quire does
    // not implement MathML layout yet, but on non-MathML elements it computes
    // to `flow` while retaining the specified/default outer display type.
    // Keeping this distinction here makes `display: math` preserve an
    // inline pseudo-element's outer role, whereas `block math` is block flow.
    // <https://drafts.csswg.org/css-display-4/#math>
    let mut math_inner = false;
    let mut list_item = false;
    for part in &parts {
        match part.to_ascii_lowercase().as_str() {
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
            "math" => {
                if inner.replace(DisplayInner::Flow).is_some() {
                    return current;
                }
                math_inner = true;
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
            "grid-lanes" => {
                if inner.replace(DisplayInner::GridLanes).is_some() {
                    return current;
                }
            }
            "ruby" => {
                if inner.replace(DisplayInner::Ruby).is_some() {
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

    let outer = outer.unwrap_or({
        if math_inner || inner == Some(DisplayInner::Ruby) {
            current.outer
        } else {
            DisplayOuter::Block
        }
    });
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
        // The legacy WebKit box model is a block-level flex formatting
        // context. Its legacy behavior is retained separately on the
        // computed style because properties such as `flex-wrap: balance` do
        // not apply to it.
        "-webkit-box" => Some(Display::FLEX),
        "-webkit-inline-box" => Some(Display::INLINE_FLEX),
        "grid" => Some(Display::GRID),
        "inline-grid" => Some(Display::INLINE_GRID),
        "grid-lanes" => Some(Display::GRID_LANES),
        "inline-grid-lanes" => Some(Display::INLINE_GRID_LANES),
        "ruby" => Some(Display::RUBY),
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
        "ruby-base" => Some(Display::RUBY_BASE),
        "ruby-text" => Some(Display::RUBY_TEXT),
        "ruby-base-container" => Some(Display::RUBY_BASE_CONTAINER),
        "ruby-text-container" => Some(Display::RUBY_TEXT_CONTAINER),
        _ => None,
    }
}
