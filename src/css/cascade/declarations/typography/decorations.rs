use super::*;

/// Parses CSS `text-decoration-line`.
///
/// CSS Text Decoration defines `none` and a space-separated set of line
/// keywords. Repeated keywords or unknown keywords invalidate the declaration:
/// <https://www.w3.org/TR/css-text-decor-3/#text-decoration-line-property>.
pub(in crate::css) fn parse_text_decoration_line(value: &str) -> Option<TextDecorationLineParts> {
    let parts = split_css_component_values(value);
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("none") {
        return Some(TextDecorationLineParts {
            underline: false,
            overline: false,
            line_through: false,
            blink: false,
            spelling_error: false,
            grammar_error: false,
        });
    }
    let mut line = TextDecorationLineParts {
        underline: false,
        overline: false,
        line_through: false,
        blink: false,
        spelling_error: false,
        grammar_error: false,
    };
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "none" => return None,
            "underline" if !line.underline => line.underline = true,
            "overline" if !line.overline => line.overline = true,
            "line-through" if !line.line_through => line.line_through = true,
            "blink" if !line.blink => line.blink = true,
            "spelling-error" if !line.spelling_error => line.spelling_error = true,
            "grammar-error" if !line.grammar_error => line.grammar_error = true,
            _ => return None,
        }
    }
    Some(line)
}

pub(in crate::css) fn apply_text_decoration_line(
    decoration: &mut TextDecoration,
    line: TextDecorationLineParts,
) {
    decoration.underline = line.underline;
    decoration.overline = line.overline;
    decoration.line_through = line.line_through;
    decoration.blink = line.blink;
    decoration.spelling_error = line.spelling_error;
    decoration.grammar_error = line.grammar_error;
}

/// Parses CSS `text-decoration-style`.
///
/// CSS Text Decoration defines solid, double, dotted, dashed, and wavy
/// decoration styles:
/// <https://www.w3.org/TR/css-text-decor-3/#text-decoration-style-property>.
pub(in crate::css) fn parse_text_decoration_style(value: &str) -> Option<TextDecorationStyle> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "solid" => Some(TextDecorationStyle::Solid),
        "double" => Some(TextDecorationStyle::Double),
        "dotted" => Some(TextDecorationStyle::Dotted),
        "dashed" => Some(TextDecorationStyle::Dashed),
        "wavy" => Some(TextDecorationStyle::Wavy),
        _ => None,
    }
}

/// Parses CSS `text-decoration-thickness`.
///
/// CSS Text Decoration Level 4 defines `auto`, `from-font`, and
/// `<length-percentage>` thickness values:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-width-property>.
pub(in crate::css) fn parse_text_decoration_thickness(
    value: &str,
    font_size: f32,
) -> Option<TextDecorationThickness> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(TextDecorationThickness::Auto),
        "from-font" => Some(TextDecorationThickness::FromFont),
        "thin" | "medium" | "thick" => parse_border_width_with_font_size(value, font_size)
            .map(ComputedLengthPercentage::from_points)
            .map(TextDecorationThickness::LengthPercentage),
        _ => parse_computed_length_percentage(value, font_size)
            .map(TextDecorationThickness::LengthPercentage),
    }
}

pub(in crate::css) fn parse_text_decoration_inset(
    value: &str,
    font_size: f32,
) -> Option<TextDecorationInset> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(TextDecorationInset::Auto);
    }
    let parts = split_css_component_values(value);
    match parts.as_slice() {
        [single] => {
            let length = parse_computed_length_percentage(single, font_size)?;
            Some(TextDecorationInset::Lengths {
                start: length.clone(),
                end: length,
            })
        }
        [start, end] => Some(TextDecorationInset::Lengths {
            start: parse_computed_length_percentage(start, font_size)?,
            end: parse_computed_length_percentage(end, font_size)?,
        }),
        _ => None,
    }
}

pub(in crate::css) fn parse_text_decoration_skip(
    value: &str,
) -> Option<(
    TextDecorationSkipInk,
    TextDecorationSkipSelf,
    TextDecorationSkipBox,
    TextDecorationSkipSpaces,
)> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some((
            TextDecorationSkipInk::Auto,
            TextDecorationSkipSelf::Auto,
            TextDecorationSkipBox::None,
            TextDecorationSkipSpaces::START_END,
        )),
        "none" => Some((
            TextDecorationSkipInk::None,
            TextDecorationSkipSelf::NoSkip,
            TextDecorationSkipBox::None,
            TextDecorationSkipSpaces::NONE,
        )),
        _ => None,
    }
}

pub(in crate::css) fn parse_text_decoration_skip_ink(value: &str) -> Option<TextDecorationSkipInk> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(TextDecorationSkipInk::Auto),
        "all" => Some(TextDecorationSkipInk::All),
        "none" => Some(TextDecorationSkipInk::None),
        _ => None,
    }
}

pub(in crate::css) fn parse_text_decoration_skip_self(
    value: &str,
) -> Option<TextDecorationSkipSelf> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 {
        return match parts[0].to_ascii_lowercase().as_str() {
            "auto" => Some(TextDecorationSkipSelf::Auto),
            "skip-all" => Some(TextDecorationSkipSelf::SkipAll),
            "no-skip" => Some(TextDecorationSkipSelf::NoSkip),
            "skip-underline" => Some(TextDecorationSkipSelf::Lines {
                underline: true,
                overline: false,
                line_through: false,
            }),
            "skip-overline" => Some(TextDecorationSkipSelf::Lines {
                underline: false,
                overline: true,
                line_through: false,
            }),
            "skip-line-through" => Some(TextDecorationSkipSelf::Lines {
                underline: false,
                overline: false,
                line_through: true,
            }),
            _ => None,
        };
    }
    let mut underline = false;
    let mut overline = false;
    let mut line_through = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "skip-underline" if !underline => underline = true,
            "skip-overline" if !overline => overline = true,
            "skip-line-through" if !line_through => line_through = true,
            _ => return None,
        }
    }
    Some(TextDecorationSkipSelf::Lines {
        underline,
        overline,
        line_through,
    })
}

pub(in crate::css) fn parse_text_decoration_skip_box(value: &str) -> Option<TextDecorationSkipBox> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "none" => Some(TextDecorationSkipBox::None),
        "all" => Some(TextDecorationSkipBox::All),
        _ => None,
    }
}

/// Parse CSS `text-decoration-skip-spaces`.
///
/// CSS Text Decoration Level 4 defines the grammar as `none | all |
/// [ start || end ]`, with initial value `start end`:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>.
pub(in crate::css) fn parse_text_decoration_skip_spaces(
    value: &str,
) -> Option<TextDecorationSkipSpaces> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }

    if parts.len() == 1 {
        return match parts[0].to_ascii_lowercase().as_str() {
            "none" => Some(TextDecorationSkipSpaces::NONE),
            "all" => Some(TextDecorationSkipSpaces::ALL),
            "start" => Some(TextDecorationSkipSpaces::Start),
            "end" => Some(TextDecorationSkipSpaces::End),
            _ => None,
        };
    }

    let mut start = false;
    let mut end = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "start" if !start => start = true,
            "end" if !end => end = true,
            _ => return None,
        }
    }
    if start || end {
        Some(match (start, end) {
            (true, true) => TextDecorationSkipSpaces::StartEnd,
            (true, false) => TextDecorationSkipSpaces::Start,
            (false, true) => TextDecorationSkipSpaces::End,
            (false, false) => unreachable!("parser requires at least one skip-space edge"),
        })
    } else {
        None
    }
}

pub(in crate::css) fn parse_text_underline_offset(
    value: &str,
    font_size: f32,
) -> Option<TextUnderlineOffset> {
    if trim_css_value(value).eq_ignore_ascii_case("auto") {
        return Some(TextUnderlineOffset::Auto);
    }
    parse_computed_length_percentage(value, font_size).map(TextUnderlineOffset::LengthPercentage)
}

pub(in crate::css) fn parse_text_underline_position(value: &str) -> Option<TextUnderlinePosition> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    let mut position = TextUnderlinePosition {
        auto: false,
        under: false,
        left: false,
        right: false,
    };
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "auto" if !position.auto && !position.under => position.auto = true,
            "under" if !position.under && !position.auto => position.under = true,
            "left" if !position.left && !position.right => position.left = true,
            "right" if !position.right && !position.left => position.right = true,
            _ => return None,
        }
    }
    Some(position)
}

/// Parses CSS `text-emphasis-style`.
///
/// CSS Text Decoration defines `none`, filled/open shape keywords, and string
/// marks. A missing fill defaults to `filled`; a missing shape is resolved
/// later from the used writing mode:
/// <https://www.w3.org/TR/css-text-decor-3/#text-emphasis-style-property>.
pub(in crate::css) fn parse_text_emphasis_style(value: &str) -> Option<TextEmphasisStyle> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(TextEmphasisStyle::None);
    }
    if let Some((mark, tail)) = parse_css_string_token(value)
        && tail.trim().is_empty()
    {
        return Some(TextEmphasisStyle::String(mark));
    }

    let mut fill = None;
    let mut shape = None;
    for part in split_css_component_values(value) {
        match part.to_ascii_lowercase().as_str() {
            "filled" if fill.is_none() => fill = Some(TextEmphasisFill::Filled),
            "open" if fill.is_none() => fill = Some(TextEmphasisFill::Open),
            "dot" if shape.is_none() => shape = Some(TextEmphasisShape::Dot),
            "circle" if shape.is_none() => shape = Some(TextEmphasisShape::Circle),
            "double-circle" if shape.is_none() => shape = Some(TextEmphasisShape::DoubleCircle),
            "triangle" if shape.is_none() => shape = Some(TextEmphasisShape::Triangle),
            "sesame" if shape.is_none() => shape = Some(TextEmphasisShape::Sesame),
            _ => return None,
        }
    }
    if fill.is_none() && shape.is_none() {
        return None;
    }
    Some(TextEmphasisStyle::Keywords {
        fill: fill.unwrap_or(TextEmphasisFill::Filled),
        shape,
    })
}

pub(in crate::css) fn parse_text_emphasis(
    value: &str,
) -> Option<(TextEmphasisStyle, Option<CssColor>)> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    for split_index in 0..=parts.len() {
        let first = parts[..split_index].join(" ");
        let second = parts[split_index..].join(" ");
        if !first.is_empty()
            && let Some(style) = parse_text_emphasis_style(&first)
        {
            if second.is_empty() {
                return Some((style, None));
            }
            if let Some(color) = parse_color(&second) {
                return Some((style, Some(color)));
            }
        }
        if !second.is_empty()
            && let Some(style) = parse_text_emphasis_style(&second)
        {
            if first.is_empty() {
                return Some((style, None));
            }
            if let Some(color) = parse_color(&first) {
                return Some((style, Some(color)));
            }
        }
    }
    None
}

pub(in crate::css) fn parse_text_emphasis_position(value: &str) -> Option<TextEmphasisPosition> {
    let parts = split_css_component_values(value);
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    let mut over = None;
    let mut right = None;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "over" if over.is_none() => over = Some(true),
            "under" if over.is_none() => over = Some(false),
            "right" if right.is_none() => right = Some(true),
            "left" if right.is_none() => right = Some(false),
            _ => return None,
        }
    }
    Some(TextEmphasisPosition {
        over: over.unwrap_or(true),
        right: right.unwrap_or(true),
    })
}

pub(in crate::css) fn parse_text_emphasis_skip(value: &str) -> Option<TextEmphasisSkip> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    let mut skip = TextEmphasisSkip {
        spaces: false,
        punctuation: false,
        symbols: false,
        narrow: false,
    };
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "spaces" if !skip.spaces => skip.spaces = true,
            "punctuation" if !skip.punctuation => skip.punctuation = true,
            "symbols" if !skip.symbols => skip.symbols = true,
            "narrow" if !skip.narrow => skip.narrow = true,
            _ => return None,
        }
    }
    Some(skip)
}

pub(crate) fn parse_text_shadow(value: &str, font_size: f32) -> Option<Vec<TextShadow>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let shadows = try_split_css_top_level_delimiter(value, ',')?
        .into_iter()
        .map(|layer| parse_text_shadow_layer(layer, font_size))
        .collect::<Option<Vec<_>>>()?;
    (!shadows.is_empty()).then_some(shadows)
}

pub(in crate::css) fn parse_text_shadow_layer(value: &str, font_size: f32) -> Option<TextShadow> {
    let mut color = None;
    let mut inset = false;
    let mut lengths = Vec::new();
    for part in try_split_css_component_values(value)? {
        if part.eq_ignore_ascii_case("inset") && !inset {
            inset = true;
            continue;
        }
        if part.eq_ignore_ascii_case("currentcolor") && color.is_none() {
            color = Some(TextShadowColor::CurrentColor);
            continue;
        }
        if color.is_none()
            && let Some(parsed_color) = parse_color(part)
        {
            color = Some(TextShadowColor::CssColor(parsed_color));
            continue;
        }
        if let Some(length) = parse_shadow_length(part, font_size) {
            lengths.push(length);
            continue;
        }
        return None;
    }
    if !(2..=4).contains(&lengths.len()) {
        return None;
    }
    let spread = lengths
        .get(3)
        .cloned()
        .unwrap_or(ComputedLengthPercentage::ZERO);
    if length_percentage_is_definitely_negative(&spread) {
        return None;
    }
    Some(TextShadow {
        color: color.unwrap_or(TextShadowColor::CurrentColor),
        offset_x: lengths[0].clone(),
        offset_y: lengths[1].clone(),
        blur_radius: lengths
            .get(2)
            .cloned()
            .filter(|length| !length_percentage_is_definitely_negative(length))
            .unwrap_or(ComputedLengthPercentage::ZERO),
        spread,
        inset,
    })
}

pub(in crate::css) fn parse_box_shadow(value: &str, font_size: f32) -> Option<Vec<BoxShadow>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let shadows = try_split_css_top_level_delimiter(value, ',')?
        .into_iter()
        .map(|layer| parse_box_shadow_layer(layer, font_size))
        .collect::<Option<Vec<_>>>()?;
    (!shadows.is_empty()).then_some(shadows)
}

pub(in crate::css) fn parse_box_shadow_layer(value: &str, font_size: f32) -> Option<BoxShadow> {
    let mut color = None;
    let mut inset = false;
    let mut lengths = Vec::new();
    for part in try_split_css_component_values(value)? {
        if part.eq_ignore_ascii_case("inset") && !inset {
            inset = true;
            continue;
        }
        if part.eq_ignore_ascii_case("currentcolor") && color.is_none() {
            color = Some(BoxShadowColor::CurrentColor);
            continue;
        }
        if color.is_none()
            && let Some(parsed_color) = parse_color(part)
        {
            color = Some(BoxShadowColor::CssColor(parsed_color));
            continue;
        }
        if let Some(length) = parse_shadow_length(part, font_size) {
            lengths.push(length);
            continue;
        }
        return None;
    }
    if !(2..=4).contains(&lengths.len())
        || lengths
            .get(2)
            .is_some_and(length_percentage_is_definitely_negative)
    {
        return None;
    }
    Some(BoxShadow {
        color: color.unwrap_or(BoxShadowColor::CurrentColor),
        offset_x: lengths[0].clone(),
        offset_y: lengths[1].clone(),
        blur_radius: lengths
            .get(2)
            .cloned()
            .unwrap_or(ComputedLengthPercentage::ZERO),
        spread: lengths
            .get(3)
            .cloned()
            .unwrap_or(ComputedLengthPercentage::ZERO),
        inset,
    })
}

pub(in crate::css) fn parse_shadow_length(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    let length = parse_computed_length_percentage(value, font_size)?;
    (!length.contains_percentage()).then_some(length)
}

pub(in crate::css) fn length_percentage_is_definitely_negative(
    value: &ComputedLengthPercentage,
) -> bool {
    value.is_definitely_absolute() && value.length_points() < 0.0
}

/// Parses the CSS `text-decoration` shorthand.
///
/// The shorthand accepts line, style, color, and thickness components in any
/// order. Omitted components reset to their initial values:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-property>.
pub(in crate::css) fn parse_text_decoration_shorthand(
    value: &str,
    current_style: &ComputedStyle,
) -> Option<TextDecoration> {
    let mut decoration = ComputedStyle::initial().text_decoration;
    let mut line = TextDecorationLineParts {
        underline: false,
        overline: false,
        line_through: false,
        blink: false,
        spelling_error: false,
        grammar_error: false,
    };
    let mut saw_style = false;
    let mut saw_color = false;
    let mut saw_thickness = false;

    let parts = split_css_component_values(value);
    for part in &parts {
        if let Some(parsed_line) = parse_text_decoration_line(part) {
            let parsed_has_line = parsed_line.underline
                || parsed_line.overline
                || parsed_line.line_through
                || parsed_line.blink
                || parsed_line.spelling_error
                || parsed_line.grammar_error;
            if !parsed_has_line && parts.len() > 1 {
                return None;
            }
            if (parsed_line.underline && line.underline)
                || (parsed_line.overline && line.overline)
                || (parsed_line.line_through && line.line_through)
                || (parsed_line.blink && line.blink)
                || (parsed_line.spelling_error && line.spelling_error)
                || (parsed_line.grammar_error && line.grammar_error)
            {
                return None;
            }
            line.underline |= parsed_line.underline;
            line.overline |= parsed_line.overline;
            line.line_through |= parsed_line.line_through;
            line.blink |= parsed_line.blink;
            line.spelling_error |= parsed_line.spelling_error;
            line.grammar_error |= parsed_line.grammar_error;
            continue;
        }
        if !saw_style && let Some(style) = parse_text_decoration_style(part) {
            decoration.style = style;
            saw_style = true;
            continue;
        }
        if !saw_thickness
            && let Some(thickness) = parse_text_decoration_thickness(part, current_style.font_size)
        {
            decoration.thickness = thickness;
            saw_thickness = true;
            continue;
        }
        if !saw_color && let Some(color) = parse_color(part) {
            decoration.color = CssColorOrCurrentColor::Color(color);
            saw_color = true;
            continue;
        }
        return None;
    }
    apply_text_decoration_line(&mut decoration, line);
    Some(decoration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_lists_split_only_at_top_level_css_commas() {
        assert!(parse_text_shadow("rgb(0, 0, 0) 1px 2px, blue 3px 4px", 12.0).is_some());
        assert!(parse_box_shadow("rgb(0, 0, 0) 1px 2px, blue 3px 4px", 12.0).is_some());
        assert!(parse_text_shadow("red 1px 2px,", 12.0).is_none());
        assert!(parse_box_shadow("red 1px 2px, url(\"\n", 12.0).is_none());
    }
}
