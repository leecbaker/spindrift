use super::*;
pub(in crate::css) fn expand_flex_flow_shorthand(
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let mut direction = "row";
    let mut wrap = "nowrap";
    let mut saw_direction = false;
    let mut saw_wrap = false;
    for token in trim_css_value(value).split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "row" | "row-reverse" | "column" | "column-reverse" if !saw_direction => {
                direction = token;
                saw_direction = true;
            }
            "nowrap" | "wrap" | "wrap-reverse" if !saw_wrap => {
                wrap = token;
                saw_wrap = true;
            }
            _ => return None,
        }
    }
    (saw_direction || saw_wrap).then(|| {
        vec![
            ("flex-direction", direction.to_string()),
            ("flex-wrap", wrap.to_string()),
        ]
    })
}

pub(in crate::css) fn expand_flex_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let (grow, shrink, basis) = parse_flex_shorthand_components(value)?;
    Some(vec![
        ("flex-grow", grow),
        ("flex-shrink", shrink),
        ("flex-basis", basis),
    ])
}

/// Parses the CSS `flex` shorthand into its longhand component strings.
///
/// CSS Flexbox defines `flex` as `none | [ <flex-grow> <flex-shrink>? ||
/// <flex-basis> ]`, with omitted shrink defaulting to `1` and omitted basis
/// defaulting to `0%`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-property>.
pub(in crate::css) fn parse_flex_shorthand_components(
    value: &str,
) -> Option<(String, String, String)> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(("0".to_string(), "0".to_string(), "auto".to_string()));
    }
    if value.eq_ignore_ascii_case("auto") {
        return Some(("1".to_string(), "1".to_string(), "auto".to_string()));
    }

    let parts = split_css_component_values(value);
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }

    let mut grow = None;
    let mut shrink = None;
    let mut basis = None;
    for part in parts {
        if grow.is_some() && shrink.is_some() && basis.is_none() && is_unitless_zero(part) {
            basis = Some("0px".to_string());
        } else if let Some(number) = parse_nonnegative_flex_number(part) {
            if grow.is_none() {
                grow = Some(number);
            } else if shrink.is_none() {
                shrink = Some(number);
            } else {
                return None;
            }
        } else if basis.is_none() && parse_computed_flex_basis(part, ROOT_FONT_SIZE_PT).is_some() {
            basis = Some(part.to_string());
        } else {
            return None;
        }
    }

    let grow = grow.unwrap_or_else(|| "1".to_string());
    let shrink = shrink.unwrap_or_else(|| "1".to_string());
    let basis = basis.unwrap_or_else(|| "0%".to_string());
    Some((grow, shrink, basis))
}

/// Returns whether a token is the unitless zero allowed for `flex-basis` in `flex`.
///
/// CSS Flexbox keeps the `flex` shorthand compatible with common authoring by
/// accepting a unitless zero in the flex-basis slot, while nonzero unitless
/// values remain flex factors rather than lengths:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-property>.
pub(in crate::css) fn is_unitless_zero(value: &str) -> bool {
    trim_css_value(value)
        .parse::<f32>()
        .is_ok_and(|number| number == 0.0)
}
/// Parses the `flex-flow` shorthand into `flex-direction` and `flex-wrap`.
///
/// CSS Flexible Box Layout defines `flex-flow` as
/// `<'flex-direction'> || <'flex-wrap'>`; omitted components reset to their
/// initial values (`row` and `nowrap`):
/// <https://www.w3.org/TR/css-flexbox-1/#flex-flow-property>.
pub(in crate::css) fn parse_flex_flow(value: &str) -> Option<(FlexDirection, FlexWrap)> {
    let mut direction = FlexDirection::Row;
    let mut wrap = FlexWrap::NoWrap;
    let mut balance = false;
    let mut saw_direction = false;
    let mut saw_wrap = false;
    for token in trim_css_value(value).split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "row" if !saw_direction => {
                direction = FlexDirection::Row;
                saw_direction = true;
            }
            "row-reverse" if !saw_direction => {
                direction = FlexDirection::RowReverse;
                saw_direction = true;
            }
            "column" if !saw_direction => {
                direction = FlexDirection::Column;
                saw_direction = true;
            }
            "column-reverse" if !saw_direction => {
                direction = FlexDirection::ColumnReverse;
                saw_direction = true;
            }
            "nowrap" if !saw_wrap => {
                wrap = FlexWrap::NoWrap;
                saw_wrap = true;
            }
            "wrap" if !saw_wrap => {
                wrap = FlexWrap::Wrap;
                saw_wrap = true;
            }
            "wrap-reverse" if !saw_wrap => {
                wrap = FlexWrap::WrapReverse;
                saw_wrap = true;
            }
            "balance" if !balance => balance = true,
            _ => return None,
        }
    }
    if balance {
        wrap = match wrap {
            FlexWrap::NoWrap => FlexWrap::Balance,
            FlexWrap::Wrap => FlexWrap::Balance,
            FlexWrap::WrapReverse => FlexWrap::BalanceReverse,
            FlexWrap::Balance | FlexWrap::BalanceReverse => unreachable!(),
        };
    }
    (saw_direction || saw_wrap || balance).then_some((direction, wrap))
}
