use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CounterDuplicatePolicy {
    KeepAll,
    KeepLast,
}

/// Parses `counter-increment` or `counter-set`.
pub(crate) fn parse_counter_changes(
    value: &str,
    default_value: i32,
    duplicate_policy: CounterDuplicatePolicy,
) -> Option<Vec<CounterChange>> {
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
        result.push(CounterChange {
            name: name.to_string(),
            value: CounterValue::new(amount),
        });
    }

    match duplicate_policy {
        CounterDuplicatePolicy::KeepAll => Some(result),
        CounterDuplicatePolicy::KeepLast => {
            let mut collapsed = Vec::<CounterChange>::new();
            for change in result {
                if let Some(index) = collapsed
                    .iter()
                    .position(|existing| existing.name == change.name)
                {
                    collapsed.remove(index);
                }
                collapsed.push(change);
            }
            Some(collapsed)
        }
    }
}

/// Parses the full CSS Lists 3 `counter-reset` grammar, including reversed
/// counters whose omitted initial value is calculated at layout time:
/// <https://drafts.csswg.org/css-lists-3/#propdef-counter-reset>.
pub(crate) fn parse_counter_resets(value: &str) -> Option<Vec<CounterReset>> {
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

    let mut result = Vec::<CounterReset>::new();
    while !parser.is_exhausted() {
        let token = parser.next().ok()?.clone();
        let (name, reversed) = match token {
            cssparser::Token::Ident(name) if is_counter_name(&name) => (name.to_string(), false),
            cssparser::Token::Function(function) if function.eq_ignore_ascii_case("reversed") => {
                let name = parser
                    .parse_nested_block(|input| -> Result<String, cssparser::ParseError<'_, ()>> {
                        let name = input.expect_ident_cloned()?;
                        if !is_counter_name(&name) || !input.is_exhausted() {
                            return Err(input.new_custom_error(()));
                        }
                        Ok(name.to_string())
                    })
                    .ok()?;
                (name, true)
            }
            _ => return None,
        };
        let explicit = parser.try_parse(|input| input.expect_integer()).ok();
        let kind = if reversed {
            CounterResetKind::Reversed(explicit.map(CounterValue::new))
        } else {
            CounterResetKind::Forward(CounterValue::new(explicit.unwrap_or(0)))
        };
        let reset = CounterReset { name, kind };
        if let Some(index) = result
            .iter()
            .position(|existing| existing.name == reset.name)
        {
            result.remove(index);
        }
        result.push(reset);
    }
    Some(result)
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
