use super::*;
use crate::css::component_values::split_nonempty_css_top_level_delimiter;

pub(in crate::css) fn supports_border_width_list(value: &str, max_count: usize) -> bool {
    let parts = split_css_component_values(trim_css_value(value));
    !parts.is_empty()
        && parts.len() <= max_count
        && parts.iter().all(|part| supports_border_width_value(part))
}

pub(in crate::css) fn supports_gap_value(value: &str) -> bool {
    let parts = split_css_component_values(trim_css_value(value));
    let valid_component = |part: &str| {
        part.eq_ignore_ascii_case("normal")
            || parse_computed_border_width(part, crate::css::ROOT_FONT_SIZE_PT).is_some()
            || parse_computed_length_percentage(part, crate::css::ROOT_FONT_SIZE_PT).is_some()
    };
    match parts.as_slice() {
        [single] => valid_component(single),
        [row, column] => valid_component(row) && valid_component(column),
        _ => false,
    }
}

pub(in crate::css) fn supports_gap_rule_inset_shorthand(value: &str) -> bool {
    let sides = split_nonempty_css_top_level_delimiter(value, '/');
    !sides.is_empty()
        && sides.len() <= 2
        && sides
            .iter()
            .all(|side| supports_gap_rule_inset_value_pair(side))
}

pub(in crate::css) fn supports_gap_rule_inset_value_pair(value: &str) -> bool {
    let parts = split_css_component_values(trim_css_value(value));
    !parts.is_empty()
        && parts.len() <= 2
        && parts
            .iter()
            .all(|part| parse_gap_rule_inset_value(part, crate::css::ROOT_FONT_SIZE_PT).is_some())
}

pub(in crate::css) fn supports_display_value(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "none"
            | "block"
            | "inline"
            | "inline-block"
            | "flex"
            | "inline-flex"
            | "table"
            | "inline-table"
            | "table-caption"
            | "table-column-group"
            | "table-column"
            | "table-header-group"
            | "table-footer-group"
            | "table-row-group"
            | "table-row"
            | "table-cell"
            | "list-item"
    ) {
        return true;
    }
    let display = parse_display(value, Display::NONE);
    display != Display::NONE || display.is_list_item()
}

pub(in crate::css) fn supported_property_name(name: &str) -> bool {
    crate::css::cascade::is_modeled_property_name(name)
}

pub(in crate::css) fn supports_text_align_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "start" | "end" | "left" | "right" | "center" | "justify" | "match-parent" | "justify-all"
    )
}

pub(in crate::css) fn supports_text_align_all_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "start" | "end" | "left" | "right" | "center" | "justify" | "match-parent"
    )
}

/// Return whether a `text-align-last` declaration uses a supported keyword.
///
/// CSS Text Level 3 defines `text-align-last` separately from `text-align`;
/// it supports `justify` but not the `text-align`-only `justify-all` keyword:
/// <https://www.w3.org/TR/css-text-3/#text-align-last-property>.
pub(in crate::css) fn supports_text_align_last_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "auto" | "start" | "end" | "left" | "right" | "center" | "justify" | "match-parent"
    )
}

/// Return whether a `text-autospace` declaration uses a supported keyword set.
///
/// CSS Text Level 4 defines this as a draft unordered keyword set. Support
/// mirrors the computed-value parser so `@supports` does not claim values that
/// the cascade later ignores:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>.
pub(in crate::css) fn supports_text_autospace_value(value: &str) -> bool {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return false;
    }
    if tokens.len() == 1 {
        return matches!(
            tokens[0].to_ascii_lowercase().as_str(),
            "normal"
                | "auto"
                | "no-autospace"
                | "ideograph-alpha"
                | "ideograph-numeric"
                | "punctuation"
        );
    }
    let mut ideograph_alpha = false;
    let mut ideograph_numeric = false;
    let mut punctuation = false;
    let mut mode = false;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "normal" | "auto" | "no-autospace" => return false,
            "ideograph-alpha" if !ideograph_alpha => ideograph_alpha = true,
            "ideograph-numeric" if !ideograph_numeric => ideograph_numeric = true,
            "punctuation" if !punctuation => punctuation = true,
            "insert" | "replace" if !mode => mode = true,
            _ => return false,
        }
    }
    ideograph_alpha || ideograph_numeric || punctuation
}

/// Return whether a `word-space-transform` declaration uses the implemented
/// CSS Text Level 4 keyword grammar.
///
/// This intentionally mirrors the computed-value parser so feature queries
/// cannot expose a value that collection would later ignore:
/// <https://drafts.csswg.org/css-text-4/#word-space-transform>.
pub(in crate::css) fn supports_word_space_transform_value(value: &str) -> bool {
    let tokens = split_css_component_values(value);
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("none") {
        return true;
    }
    let mut replacement = false;
    let mut auto_phrase = false;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "space" | "ideographic-space" if !replacement => replacement = true,
            "auto-phrase" if !auto_phrase => auto_phrase = true,
            _ => return false,
        }
    }
    replacement || auto_phrase
}

/// Return whether a `text-spacing-trim` declaration uses the CSS Text Level 4
/// keyword grammar.
///
/// `auto` is retained as a distinct computed value so that the UA resolution
/// policy is applied in layout, rather than accidentally accepting an
/// unsupported declaration during `@supports` parsing:
/// <https://drafts.csswg.org/css-text-4/#text-spacing-trim-property>.
pub(in crate::css) fn supports_text_spacing_trim_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "space-all" | "normal" | "space-first" | "trim-start" | "trim-both" | "trim-all" | "auto"
    )
}

/// Return whether a `text-spacing` shorthand declaration can set both of its
/// longhands.  The shorthand grammar is an unordered combination of one
/// `text-spacing-trim` keyword and one `text-autospace` value:
/// <https://drafts.csswg.org/css-text-4/#text-spacing-property>.
pub(in crate::css) fn supports_text_spacing_value(value: &str) -> bool {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return false;
    }
    if tokens.len() == 1 {
        return matches!(
            tokens[0].to_ascii_lowercase().as_str(),
            "none" | "normal" | "auto"
        ) || supports_text_spacing_trim_value(tokens[0])
            || supports_text_autospace_value(tokens[0]);
    }

    let mut trim = false;
    let mut autospace = Vec::new();
    for token in tokens {
        if supports_text_spacing_trim_value(token) {
            if trim {
                return false;
            }
            trim = true;
        } else {
            autospace.push(token);
        }
    }
    !autospace.is_empty() && supports_text_autospace_value(&autospace.join(" "))
}

pub(in crate::css) fn supports_text_decoration_line_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("none") {
        return true;
    }
    let mut underline = false;
    let mut overline = false;
    let mut line_through = false;
    let mut blink = false;
    let mut spelling_error = false;
    let mut grammar_error = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "none" => return false,
            "underline" if !underline => underline = true,
            "overline" if !overline => overline = true,
            "line-through" if !line_through => line_through = true,
            "blink" if !blink => blink = true,
            "spelling-error" if !spelling_error => spelling_error = true,
            "grammar-error" if !grammar_error => grammar_error = true,
            _ => return false,
        }
    }
    underline || overline || line_through || blink || spelling_error || grammar_error
}

pub(in crate::css) fn supports_text_decoration_style_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "solid" | "double" | "dotted" | "dashed" | "wavy"
    )
}

pub(in crate::css) fn supports_text_decoration_thickness_value(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "auto" | "from-font" | "thin" | "medium" | "thick"
    ) || parse_computed_length_percentage(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
}

pub(in crate::css) fn supports_text_decoration_inset_value(value: &str) -> bool {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return true;
    }
    let parts = split_css_component_values(value);
    matches!(parts.len(), 1 | 2)
        && parts.iter().all(|part| {
            parse_computed_length_percentage(part, crate::css::ROOT_FONT_SIZE_PT).is_some()
        })
}

pub(in crate::css) fn supports_text_decoration_skip_self_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    if parts.len() == 1 {
        return matches!(
            parts[0].to_ascii_lowercase().as_str(),
            "auto"
                | "skip-all"
                | "no-skip"
                | "skip-underline"
                | "skip-overline"
                | "skip-line-through"
        );
    }
    let mut underline = false;
    let mut overline = false;
    let mut line_through = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "skip-underline" if !underline => underline = true,
            "skip-overline" if !overline => overline = true,
            "skip-line-through" if !line_through => line_through = true,
            _ => return false,
        }
    }
    underline || overline || line_through
}

pub(in crate::css) fn supports_text_decoration_skip_spaces_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    if parts.len() == 1 {
        return matches!(
            parts[0].to_ascii_lowercase().as_str(),
            "none" | "all" | "start" | "end"
        );
    }
    let mut start = false;
    let mut end = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "start" if !start => start = true,
            "end" if !end => end = true,
            _ => return false,
        }
    }
    start || end
}

pub(in crate::css) fn supports_text_underline_position_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    let mut auto = false;
    let mut under = false;
    let mut side = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "auto" if !auto && !under => auto = true,
            "under" if !under && !auto => under = true,
            "left" | "right" if !side => side = true,
            _ => return false,
        }
    }
    auto || under || side
}

pub(in crate::css) fn supports_text_decoration_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    let mut underline = false;
    let mut overline = false;
    let mut line_through = false;
    let mut blink = false;
    let mut spelling_error = false;
    let mut grammar_error = false;
    let mut saw_style = false;
    let mut saw_color = false;
    let mut saw_thickness = false;
    for part in &parts {
        match part.to_ascii_lowercase().as_str() {
            "none" if parts.len() == 1 => return true,
            "underline" if !underline => {
                underline = true;
                continue;
            }
            "overline" if !overline => {
                overline = true;
                continue;
            }
            "line-through" if !line_through => {
                line_through = true;
                continue;
            }
            "blink" if !blink => {
                blink = true;
                continue;
            }
            "spelling-error" if !spelling_error => {
                spelling_error = true;
                continue;
            }
            "grammar-error" if !grammar_error => {
                grammar_error = true;
                continue;
            }
            _ => {}
        }
        if supports_text_decoration_style_value(part) && !saw_style {
            saw_style = true;
            continue;
        }
        if supports_text_decoration_thickness_value(part) && !saw_thickness {
            saw_thickness = true;
            continue;
        }
        if parse_color(part).is_some() && !saw_color {
            saw_color = true;
            continue;
        }
        return false;
    }
    underline
        || overline
        || line_through
        || blink
        || spelling_error
        || grammar_error
        || saw_style
        || saw_color
        || saw_thickness
}

pub(in crate::css) fn supports_text_emphasis_style_value(value: &str) -> bool {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return true;
    }
    if parse_css_string_token(value).is_some_and(|(_, tail)| tail.trim().is_empty()) {
        return true;
    }
    let mut saw_fill = false;
    let mut saw_shape = false;
    for part in split_css_component_values(value) {
        match part.to_ascii_lowercase().as_str() {
            "filled" | "open" if !saw_fill => saw_fill = true,
            "dot" | "circle" | "double-circle" | "triangle" | "sesame" if !saw_shape => {
                saw_shape = true;
            }
            _ => return false,
        }
    }
    saw_fill || saw_shape
}

pub(in crate::css) fn supports_text_emphasis_value(value: &str) -> bool {
    let mut saw_style = false;
    let mut saw_color = false;
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    for split_index in 0..=parts.len() {
        let style_part = parts[..split_index].join(" ");
        let color_part = parts[split_index..].join(" ");
        if (!style_part.is_empty() && supports_text_emphasis_style_value(&style_part))
            && (color_part.is_empty() || parse_color(&color_part).is_some())
        {
            saw_style = true;
            saw_color = !color_part.is_empty();
            break;
        }
        if (!color_part.is_empty() && supports_text_emphasis_style_value(&color_part))
            && (style_part.is_empty() || parse_color(&style_part).is_some())
        {
            saw_style = true;
            saw_color = !style_part.is_empty();
            break;
        }
    }
    saw_style || saw_color
}

pub(in crate::css) fn supports_text_emphasis_position_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() || parts.len() > 2 {
        return false;
    }
    let mut saw_over_under = false;
    let mut saw_side = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "over" | "under" if !saw_over_under => saw_over_under = true,
            "right" | "left" if !saw_side => saw_side = true,
            _ => return false,
        }
    }
    true
}

pub(in crate::css) fn supports_text_emphasis_skip_value(value: &str) -> bool {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return false;
    }
    let mut spaces = false;
    let mut punctuation = false;
    let mut symbols = false;
    let mut narrow = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "spaces" if !spaces => spaces = true,
            "punctuation" if !punctuation => punctuation = true,
            "symbols" if !symbols => symbols = true,
            "narrow" if !narrow => narrow = true,
            _ => return false,
        }
    }
    true
}

pub(in crate::css) fn supports_text_shadow_value(value: &str) -> bool {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return true;
    }
    split_nonempty_css_top_level_delimiter(value, ',')
        .into_iter()
        .all(supports_text_shadow_layer_value)
}

pub(in crate::css) fn supports_text_shadow_layer_value(value: &str) -> bool {
    let mut color = false;
    let mut inset = false;
    let mut lengths = 0usize;
    for part in split_css_component_values(value) {
        if part.eq_ignore_ascii_case("inset") && !inset {
            inset = true;
            continue;
        }
        if !color && (part.eq_ignore_ascii_case("currentcolor") || parse_color(part).is_some()) {
            color = true;
            continue;
        }
        if supports_shadow_length(part) {
            lengths += 1;
            continue;
        }
        return false;
    }
    (2..=4).contains(&lengths)
}

pub(in crate::css) fn supports_box_shadow_value(value: &str) -> bool {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return true;
    }
    split_nonempty_css_top_level_delimiter(value, ',')
        .into_iter()
        .all(supports_box_shadow_layer_value)
}

pub(in crate::css) fn supports_box_shadow_layer_value(value: &str) -> bool {
    let mut color = false;
    let mut inset = false;
    let mut lengths = Vec::new();
    for part in split_css_component_values(value) {
        if part.eq_ignore_ascii_case("inset") && !inset {
            inset = true;
            continue;
        }
        if !color && (part.eq_ignore_ascii_case("currentcolor") || parse_color(part).is_some()) {
            color = true;
            continue;
        }
        if let Some(length) = parse_shadow_support_length(part) {
            lengths.push(length);
            continue;
        }
        return false;
    }
    (2..=4).contains(&lengths.len())
        && !lengths
            .get(2)
            .is_some_and(|blur| length_percentage_is_definitely_negative(blur.clone()))
}

pub(in crate::css) fn supports_shadow_length(value: &str) -> bool {
    parse_shadow_support_length(value).is_some()
}

pub(in crate::css) fn parse_shadow_support_length(
    value: &str,
) -> Option<crate::css::ComputedLengthPercentage> {
    let length = parse_computed_length_percentage(value, crate::css::ROOT_FONT_SIZE_PT)?;
    (!length.needs_percentage_basis()).then_some(length)
}

pub(in crate::css) fn length_percentage_is_definitely_negative(
    value: crate::css::ComputedLengthPercentage,
) -> bool {
    value.is_definitely_negative()
}

/// Returns whether a logical margin/padding axis value has valid arity.
///
/// CSS Logical Properties defines `margin-block`/`margin-inline` and
/// `padding-block`/`padding-inline` as one-or-two-value shorthands for their
/// logical start/end sides:
/// <https://www.w3.org/TR/css-logical-1/#box>.
pub(in crate::css) fn supports_box_edge_axis_value(value: &str, allow_auto: bool) -> bool {
    let parts = split_css_component_values(trim_css_value(value));
    matches!(parts.len(), 1 | 2)
        && parts.iter().all(|part| {
            (allow_auto && part.eq_ignore_ascii_case("auto"))
                || parse_computed_length_percentage(part, crate::css::ROOT_FONT_SIZE_PT).is_some()
        })
}

pub(in crate::css) fn supports_text_transform_value(value: &str) -> bool {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return false;
    }
    if tokens.len() == 1
        && (tokens[0].eq_ignore_ascii_case("none") || tokens[0].eq_ignore_ascii_case("math-auto"))
    {
        return true;
    }

    let mut saw_case = false;
    let mut saw_full_width = false;
    let mut saw_full_size_kana = false;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "none" => return false,
            "uppercase" | "lowercase" | "capitalize" if !saw_case => saw_case = true,
            "full-width" if !saw_full_width => saw_full_width = true,
            "full-size-kana" if !saw_full_size_kana => saw_full_size_kana = true,
            _ => return false,
        }
    }
    saw_case || saw_full_width || saw_full_size_kana
}

pub(in crate::css) fn supports_border_width_value(value: &str) -> bool {
    parse_computed_border_width(trim_css_value(value), crate::css::ROOT_FONT_SIZE_PT).is_some()
}
