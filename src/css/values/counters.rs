use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CounterDuplicatePolicy {
    KeepAll,
    KeepLast,
}

/// Parses CSS Lists 3 counter control properties.
///
/// `counter-reset`, `counter-increment`, and `counter-set` all use the
/// `<counter-name> <integer>?` list grammar, with different default integer
/// values and duplicate handling:
/// <https://www.w3.org/TR/css-lists-3/#auto-numbering>.
pub(crate) fn parse_counter_pairs(
    value: &str,
    default_value: i32,
    duplicate_policy: CounterDuplicatePolicy,
) -> Option<Vec<(String, i32)>> {
    let value = trim_css_value(value);
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let initial_state = parser.state();

    if let Ok(ident) = parser.try_parse(|input| input.expect_ident_cloned())
        && ident.eq_ignore_ascii_case("none")
    {
        return parser.is_exhausted().then(Vec::new);
    }
    parser.reset(&initial_state);

    let mut result = Vec::new();
    while !parser.is_exhausted() {
        let name = parser.expect_ident_cloned().ok()?;
        if !is_counter_name(&name) {
            return None;
        }
        let amount = parser
            .try_parse(|input| input.expect_integer())
            .unwrap_or(default_value);
        result.push((name.to_string(), amount));
    }

    match duplicate_policy {
        CounterDuplicatePolicy::KeepAll => Some(result),
        CounterDuplicatePolicy::KeepLast => {
            let mut collapsed = Vec::<(String, i32)>::new();
            for (name, value) in result {
                if let Some(index) = collapsed
                    .iter()
                    .position(|(existing, _)| existing.as_str() == name.as_str())
                {
                    collapsed.remove(index);
                }
                collapsed.push((name, value));
            }
            Some(collapsed)
        }
    }
}

pub(crate) fn is_counter_name(value: &str) -> bool {
    !is_css_wide_or_counter_keyword(value)
}

fn is_css_wide_or_counter_keyword(value: &str) -> bool {
    value.eq_ignore_ascii_case("none")
        || value.eq_ignore_ascii_case("inherit")
        || value.eq_ignore_ascii_case("initial")
        || value.eq_ignore_ascii_case("unset")
        || value.eq_ignore_ascii_case("revert")
        || value.eq_ignore_ascii_case("revert-layer")
}
