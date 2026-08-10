use super::*;

/// Parses the computed CSS `hanging-punctuation` keyword set.
///
/// CSS Text defines the grammar as
/// `none | [ first || [ force-end | allow-end ] || last ]`, with the computed
/// value preserving the specified keywords:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(crate) fn parse_hanging_punctuation(value: &str) -> Option<HangingPunctuation> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(HangingPunctuation::NONE);
    }

    let mut output = HangingPunctuation::NONE;
    let mut saw_keyword = false;
    for keyword in crate::css::component_values::try_split_css_component_values(value)? {
        match keyword.to_ascii_lowercase().as_str() {
            "first" if !output.first => output.first = true,
            "last" if !output.last => output.last = true,
            "force-end" if !output.force_end && !output.allow_end => output.force_end = true,
            "allow-end" if !output.force_end && !output.allow_end => output.allow_end = true,
            _ => return None,
        }
        saw_keyword = true;
    }
    saw_keyword.then_some(output)
}

pub(in crate::css) fn remove_unordered_text_indent_keyword(
    value: &str,
    keyword: &str,
) -> (String, bool) {
    let Some(range) =
        crate::css::component_values::find_css_top_level_keyword_range(value, keyword)
    else {
        return (value.trim().to_string(), false);
    };
    let mut output = String::with_capacity(value.len().saturating_sub(range.len()));
    output.push_str(value[..range.start].trim_end());
    if !output.is_empty() && !value[range.end..].trim_start().is_empty() {
        output.push(' ');
    }
    output.push_str(value[range.end..].trim_start());
    (output.trim().to_string(), true)
}

/// Parses `letter-spacing` into a computed length projection.
///
/// CSS Text Level 4 defines `letter-spacing` as `normal | <length-percentage>`
/// and makes the property inherited. The percentage remains unresolved until
/// the used font size is known; `normal` remains zero additional spacing until
/// justification-driven spacing is modeled separately:
/// <https://drafts.csswg.org/css-text-4/#letter-spacing-property>.
pub(crate) fn parse_letter_spacing(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(ComputedLengthPercentage::ZERO);
    }
    if let Some(value) = parse_computed_length_percentage(value, font_size) {
        return Some(value);
    }
    parse_length(value).map(ComputedLengthPercentage::from_points)
}

/// Parses `word-spacing` into a computed length projection.
///
/// CSS Text Level 4 defines `word-spacing` as `normal | <length-percentage>`
/// and makes the property inherited. The renderer stores `normal` as zero
/// additional spacing and preserves percentages until used-value resolution
/// against the current element's used font size:
/// <https://www.w3.org/TR/css-text-4/#word-spacing-property>.
pub(crate) fn parse_word_spacing(value: &str, font_size: f32) -> Option<ComputedLengthPercentage> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(ComputedLengthPercentage::ZERO);
    }
    parse_computed_length_percentage(value, font_size)
}

/// Parses CSS Text's `tab-size` property.
///
/// CSS Text Level 3 defines `tab-size` as a non-negative number of spaces or
/// a non-negative length, with an initial value of 8:
/// <https://www.w3.org/TR/css-text-3/#tab-size-property>.
pub(crate) fn parse_tab_size(value: &str, font_size: f32) -> Option<TabSize> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(TabSize::INITIAL);
    }
    if let Ok(number) = value.parse::<f32>()
        && number.is_finite()
        && number >= 0.0
    {
        return Some(TabSize::Spaces(number));
    }
    if let Some(length) = parse_computed_length_percentage(value, font_size)
        && !length.contains_percentage()
        && length.length_points() >= 0.0
    {
        return Some(TabSize::Length(length));
    }
    None
}

/// Parses the computed CSS `text-indent` value.
///
/// CSS Text defines the grammar as `<length-percentage> && hanging? &&
/// each-line?`; the length-percentage is computed now, while percentages
/// remain unresolved until layout has the containing block inline size:
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>.
pub(crate) fn parse_text_indent(value: &str, font_size: f32) -> Option<ComputedTextIndent> {
    let value = trim_css_value(value);
    let (value, hanging) = remove_unordered_text_indent_keyword(value, "hanging");
    let (value, each_line) = remove_unordered_text_indent_keyword(&value, "each-line");
    let amount = parse_computed_length_percentage(&value, font_size)?;
    Some(ComputedTextIndent {
        amount,
        hanging,
        each_line,
    })
}
