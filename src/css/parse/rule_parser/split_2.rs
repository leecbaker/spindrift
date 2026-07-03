use super::*;

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
    let sides = split_top_level_delimiter(value, '/');
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
    matches!(
        name,
        "direction"
            | "unicode-bidi"
            | "writing-mode"
            | "flex-direction"
            | "justify-content"
            | "justify-items"
            | "justify-self"
            | "align-content"
            | "align-items"
            | "align-self"
            | "place-content"
            | "place-items"
            | "place-self"
            | "flex-wrap"
            | "flex-flow"
            | "flex-grow"
            | "flex-shrink"
            | "flex-basis"
            | "flex"
            | "grid-gap"
            | "grid-row-gap"
            | "grid-column-gap"
            | "columns"
            | "column-count"
            | "column-width"
            | "column-rule"
            | "column-rule-width"
            | "column-rule-style"
            | "column-rule-color"
            | "column-rule-break"
            | "column-rule-visibility-items"
            | "column-rule-inset"
            | "column-rule-inset-start"
            | "column-rule-inset-end"
            | "column-rule-inset-cap"
            | "column-rule-inset-junction"
            | "column-rule-inset-cap-start"
            | "column-rule-inset-cap-end"
            | "column-rule-inset-junction-start"
            | "column-rule-inset-junction-end"
            | "row-rule"
            | "row-rule-width"
            | "row-rule-style"
            | "row-rule-color"
            | "row-rule-break"
            | "row-rule-visibility-items"
            | "row-rule-inset"
            | "row-rule-inset-start"
            | "row-rule-inset-end"
            | "row-rule-inset-cap"
            | "row-rule-inset-junction"
            | "row-rule-inset-cap-start"
            | "row-rule-inset-cap-end"
            | "row-rule-inset-junction-start"
            | "row-rule-inset-junction-end"
            | "rule"
            | "rule-width"
            | "rule-style"
            | "rule-color"
            | "rule-break"
            | "rule-visibility-items"
            | "rule-overlap"
            | "rule-inset"
            | "rule-inset-start"
            | "rule-inset-end"
            | "rule-inset-cap"
            | "rule-inset-junction"
            | "margin-block"
            | "margin-block-start"
            | "margin-block-end"
            | "margin-inline"
            | "margin-inline-start"
            | "margin-inline-end"
            | "padding-block"
            | "padding-block-start"
            | "padding-block-end"
            | "padding-inline"
            | "padding-inline-start"
            | "padding-inline-end"
            | "border"
            | "border-top"
            | "border-right"
            | "border-bottom"
            | "border-left"
            | "border-block"
            | "border-block-start"
            | "border-block-end"
            | "border-inline"
            | "border-inline-start"
            | "border-inline-end"
            | "border-image"
            | "border-image-source"
            | "border-image-slice"
            | "border-image-width"
            | "border-image-outset"
            | "border-image-repeat"
            | "border-style"
            | "border-top-style"
            | "border-right-style"
            | "border-bottom-style"
            | "border-left-style"
            | "border-block-style"
            | "border-block-start-style"
            | "border-block-end-style"
            | "border-inline-style"
            | "border-inline-start-style"
            | "border-inline-end-style"
            | "border-block-color"
            | "border-block-start-color"
            | "border-block-end-color"
            | "border-inline-color"
            | "border-inline-start-color"
            | "border-inline-end-color"
            | "border-block-width"
            | "border-block-start-width"
            | "border-block-end-width"
            | "border-inline-width"
            | "border-inline-start-width"
            | "border-inline-end-width"
            | "border-start-start-radius"
            | "border-start-end-radius"
            | "border-end-start-radius"
            | "border-end-end-radius"
            | "corner"
            | "corner-shape"
            | "corner-top-left-shape"
            | "corner-top-right-shape"
            | "corner-bottom-right-shape"
            | "corner-bottom-left-shape"
            | "border-collapse"
            | "caption-side"
            | "table-layout"
            | "empty-cells"
            | "border-spacing"
            | "background"
            | "background-image"
            | "background-size"
            | "background-position"
            | "background-repeat"
            | "background-origin"
            | "background-clip"
            | "letter-spacing"
            | "line-height"
            | "box-sizing"
            | "z-index"
            | "isolation"
            | "mix-blend-mode"
            | "filter"
            | "clip-path"
            | "mask"
            | "mask-image"
            | "contain"
            | "content-visibility"
            | "will-change"
            | "text-align"
            | "text-align-all"
            | "text-align-last"
            | "text-justify"
            | "text-autospace"
            | "text-orientation"
            | "text-indent"
            | "hanging-punctuation"
            | "vertical-align"
            | "dominant-baseline"
            | "alignment-baseline"
            | "baseline-source"
            | "baseline-shift"
            | "font-weight"
            | "font-style"
            | "font-width"
            | "font-stretch"
            | "font-family"
            | "font"
            | "font-feature-settings"
            | "font-kerning"
            | "font-size-adjust"
            | "font-variant"
            | "font-variant-ligatures"
            | "font-variant-position"
            | "font-variant-caps"
            | "font-variant-numeric"
            | "font-variant-alternates"
            | "font-variant-east-asian"
            | "font-variant-emoji"
            | "bookmark-level"
            | "bookmark-label"
            | "bookmark-state"
            | "text-transform"
            | "tab-size"
            | "visibility"
            | "list-style"
            | "list-style-type"
            | "list-style-position"
            | "list-style-image"
            | "counter-reset"
            | "counter-increment"
            | "counter-set"
            | "string-set"
            | "page"
            | "break-before"
            | "break-after"
            | "break-inside"
            | "page-break-before"
            | "page-break-after"
            | "page-break-inside"
            | "orphans"
            | "widows"
            | "text-decoration"
            | "text-decoration-line"
            | "text-decoration-style"
            | "text-decoration-color"
            | "text-decoration-thickness"
            | "text-decoration-inset"
            | "text-decoration-skip"
            | "text-decoration-skip-ink"
            | "text-decoration-skip-self"
            | "text-decoration-skip-box"
            | "text-decoration-skip-spaces"
            | "text-underline-offset"
            | "text-underline-position"
            | "text-emphasis"
            | "text-emphasis-style"
            | "text-emphasis-color"
            | "text-emphasis-position"
            | "text-emphasis-skip"
            | "text-shadow"
            | "box-shadow"
            | "white-space"
            | "word-break"
            | "overflow"
            | "overflow-x"
            | "overflow-y"
            | "overflow-wrap"
            | "word-wrap"
            | "line-break"
            | "hyphens"
            | "word-spacing"
            | "hyphenate-limit-chars"
    )
}

/// Return whether a `text-align` declaration uses a supported keyword.
///
/// CSS Text Level 3 defines the grammar as `start | end | left | right |
/// center | justify | match-parent | justify-all`.
/// <https://www.w3.org/TR/css-text-3/#text-align-property>.
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
    let mut saw_line = false;
    let mut saw_style = false;
    let mut saw_color = false;
    let mut saw_thickness = false;
    for part in &parts {
        if supports_text_decoration_line_value(part) {
            if part.eq_ignore_ascii_case("none") && parts.len() > 1 {
                return false;
            }
            if saw_line {
                return false;
            }
            saw_line = true;
            continue;
        }
        if supports_text_decoration_style_value(part) {
            if saw_style {
                return false;
            }
            saw_style = true;
            continue;
        }
        if supports_text_decoration_thickness_value(part) {
            if saw_thickness {
                return false;
            }
            saw_thickness = true;
            continue;
        }
        if parse_color(part).is_some() {
            if saw_color {
                return false;
            }
            saw_color = true;
            continue;
        }
        return false;
    }
    saw_line || saw_style || saw_color || saw_thickness
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
    split_css_args(value, ',')
        .into_iter()
        .all(supports_text_shadow_layer_value)
}

pub(in crate::css) fn split_css_args(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            candidate if candidate == delimiter && depth == 0 => {
                let part = trim_css_value(&value[start..index]);
                if !part.is_empty() {
                    parts.push(part);
                }
                start = index + candidate.len_utf8();
            }
            _ => {}
        }
    }
    let part = trim_css_value(&value[start..]);
    if !part.is_empty() {
        parts.push(part);
    }
    parts
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
    split_css_args(value, ',')
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
            .is_some_and(|blur| length_percentage_is_definitely_negative(*blur))
}

pub(in crate::css) fn supports_shadow_length(value: &str) -> bool {
    parse_shadow_support_length(value).is_some()
}

pub(in crate::css) fn parse_shadow_support_length(
    value: &str,
) -> Option<crate::css::ComputedLengthPercentage> {
    let length = parse_computed_length_percentage(value, crate::css::ROOT_FONT_SIZE_PT)?;
    (length.percent == 0.0).then_some(length)
}

pub(in crate::css) fn length_percentage_is_definitely_negative(
    value: crate::css::ComputedLengthPercentage,
) -> bool {
    let components = [
        value.length_points(),
        value.percent,
        value.ch,
        value.vw,
        value.vh,
        value.vmin,
        value.vmax,
        value.vi,
        value.vb,
    ];
    components.iter().any(|component| *component < 0.0)
        && components.iter().all(|component| *component <= 0.0)
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

pub(in crate::css) fn strip_enclosing_parentheses(value: &str) -> &str {
    let mut value = value.trim();
    while value.starts_with('(') && value.ends_with(')') && outer_parentheses_wrap(value) {
        value = value[1..value.len() - 1].trim();
    }
    value
}

pub(in crate::css) fn outer_parentheses_wrap(value: &str) -> bool {
    let mut depth = 0usize;
    for (index, byte) in value.as_bytes().iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && index != value.len() - 1 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

pub(in crate::css) fn strip_ascii_word_prefix<'a>(value: &'a str, word: &str) -> Option<&'a str> {
    let value = value.trim_start();
    let prefix = value.get(..word.len())?;
    if !prefix.eq_ignore_ascii_case(word) {
        return None;
    }
    if !word_boundary_after(value.as_bytes(), word.len()) {
        return None;
    }
    let rest = value[word.len()..].trim_start();
    (!rest.is_empty()).then_some(rest)
}

pub(in crate::css) fn split_top_level_keyword<'a>(value: &'a str, keyword: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let bytes = value.as_bytes();
    let keyword_bytes = keyword.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0
                && ascii_keyword_at(bytes, index, keyword_bytes)
                && word_boundary_before(bytes, index)
                && word_boundary_after(bytes, index + keyword_bytes.len()) =>
            {
                parts.push(value[start..index].trim());
                start = index + keyword_bytes.len();
                index = start;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(value[start..].trim());
    parts
}

pub(in crate::css) fn split_top_level_delimiter(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let delimiter = delimiter as u8;
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && *byte == delimiter => {
                parts.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}

pub(in crate::css) fn ascii_keyword_at(bytes: &[u8], index: usize, keyword: &[u8]) -> bool {
    bytes
        .get(index..index + keyword.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(keyword))
}

pub(in crate::css) fn word_boundary_before(bytes: &[u8], index: usize) -> bool {
    index == 0
        || bytes
            .get(index - 1)
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-' && *byte != b'_')
}

pub(in crate::css) fn word_boundary_after(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index)
        .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-' && *byte != b'_')
}

pub(in crate::css) fn strip_pseudo_selector<'a>(
    selector: &'a str,
    pseudo: &str,
) -> Option<&'a str> {
    let trimmed = selector.trim();
    let double_colon = format!("::{pseudo}");
    let single_colon = format!(":{pseudo}");
    let base = trimmed
        .strip_suffix(&double_colon)
        .or_else(|| trimmed.strip_suffix(&single_colon))?
        .trim();
    (!base.is_empty()).then_some(base)
}
