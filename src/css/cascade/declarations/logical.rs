use super::*;

/// Computes the writing context used to resolve logical properties.
///
/// CSS Logical Properties maps flow-relative properties through the computed
/// `direction` and `writing-mode` values. This prepass runs before shorthand
/// expansion and Cascade 5 rollback so logical and physical border longhands
/// compare in the right physical space:
/// <https://www.w3.org/TR/css-logical-1/#flow-relative> and
/// <https://www.w3.org/TR/css-cascade-5/#cascade>.
pub(in crate::css) fn logical_mapping_context(
    base_style: &ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
) -> (Direction, WritingMode) {
    let mut direction = base_style.direction;
    let mut writing_mode = base_style.writing_mode;
    for declaration in declarations {
        let name = declaration.name.as_ref();
        let value = trim_css_value(&declaration.value);
        if contains_css_variable_reference(value)
            || declaration_is_revert(value)
            || declaration_is_revert_layer(value)
        {
            continue;
        }
        if name.eq_ignore_ascii_case("all") {
            if let Some(keyword) = CssWideDefaultKeyword::parse(value) {
                writing_mode = defaulted_writing_mode(keyword, inheritance_source);
            }
            continue;
        }
        if let Some(keyword) = CssWideDefaultKeyword::parse(value) {
            match name {
                "direction" => direction = defaulted_direction(keyword, inheritance_source),
                "writing-mode" => {
                    writing_mode = defaulted_writing_mode(keyword, inheritance_source);
                }
                _ => {}
            }
            continue;
        }
        match name {
            "direction" => {
                if let Some(parsed) = parse_direction(value) {
                    direction = parsed;
                }
            }
            "writing-mode" => {
                if let Some(parsed) = parse_writing_mode(value) {
                    writing_mode = parsed;
                }
            }
            _ => {}
        }
    }
    (direction, writing_mode)
}

pub(in crate::css) fn defaulted_direction(
    keyword: CssWideDefaultKeyword,
    inheritance_source: &ComputedStyle,
) -> Direction {
    match keyword {
        CssWideDefaultKeyword::Initial => ComputedStyle::initial().direction,
        CssWideDefaultKeyword::Inherit | CssWideDefaultKeyword::Unset => {
            inheritance_source.direction
        }
    }
}

pub(in crate::css) fn defaulted_writing_mode(
    keyword: CssWideDefaultKeyword,
    inheritance_source: &ComputedStyle,
) -> WritingMode {
    match keyword {
        CssWideDefaultKeyword::Initial => ComputedStyle::initial().writing_mode,
        CssWideDefaultKeyword::Inherit | CssWideDefaultKeyword::Unset => {
            inheritance_source.writing_mode
        }
    }
}

/// Parses CSS `direction`.
///
/// CSS Writing Modes defines `direction` keywords as `ltr` and `rtl`:
/// <https://www.w3.org/TR/css-writing-modes-4/#direction>.
pub(in crate::css) fn parse_direction(value: &str) -> Option<Direction> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "ltr" => Some(Direction::Ltr),
        "rtl" => Some(Direction::Rtl),
        _ => None,
    }
}

/// Parses CSS `writing-mode` values without collapsing their specified value.
///
/// Sideways modes share physical block-flow geometry with their corresponding
/// vertical modes but select horizontal typographic mode, so they remain
/// distinct through layout and text painting:
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
pub(in crate::css) fn parse_writing_mode(value: &str) -> Option<WritingMode> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "horizontal-tb" => Some(WritingMode::HorizontalTb),
        "vertical-rl" => Some(WritingMode::VerticalRl),
        "vertical-lr" => Some(WritingMode::VerticalLr),
        "sideways-rl" => Some(WritingMode::SidewaysRl),
        "sideways-lr" => Some(WritingMode::SidewaysLr),
        _ => None,
    }
}

/// Parses CSS `text-orientation` values supported by vertical text placement.
///
/// CSS Writing Modes defines `mixed`, `upright`, and `sideways` as the modern
/// orientation keywords. Deprecated SVG aliases are intentionally left
/// unsupported until compatibility tests require them:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
pub(in crate::css) fn parse_text_orientation(value: &str) -> Option<TextOrientation> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "mixed" => Some(TextOrientation::Mixed),
        "upright" => Some(TextOrientation::Upright),
        "sideways" => Some(TextOrientation::Sideways),
        _ => None,
    }
}

/// Parse CSS Writing Modes `text-combine-upright`.
///
/// `digits` accepts the required integer range 2 through 4.  The grammar is
/// intentionally strict so invalid values leave the prior cascaded value in
/// place rather than becoming a test-specific rendering mode.
/// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-upright>
pub(in crate::css) fn parse_text_combine_upright(value: &str) -> Option<TextCombineUpright> {
    let tokens = split_css_component_values(trim_css_value(value));
    match tokens.as_slice() {
        [keyword] if keyword.eq_ignore_ascii_case("none") => Some(TextCombineUpright::None),
        [keyword] if keyword.eq_ignore_ascii_case("all") => Some(TextCombineUpright::All),
        [keyword, digits] if keyword.eq_ignore_ascii_case("digits") => digits
            .parse::<u8>()
            .ok()
            .filter(|digits| (2..=4).contains(digits))
            .map(TextCombineUpright::Digits),
        _ => None,
    }
}
