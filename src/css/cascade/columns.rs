use super::*;

pub(super) fn apply_columns(value: &str, style: &mut ComputedStyle) {
    for part in value.split_whitespace() {
        if part.eq_ignore_ascii_case("auto") {
            continue;
        }
        if let Some(count) = parse_column_count(part) {
            style.column_count = Some(count);
        } else if let Some(width) = parse_column_width(part, style.font_size) {
            style.column_width = width;
        }
    }
}

pub(super) fn parse_column_count(value: &str) -> Option<usize> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return None;
    }
    value
        .parse::<u16>()
        .ok()
        .filter(|count| *count > 0)
        .map(usize::from)
}

/// Parses `column-width` into its computed CSS value.
///
/// CSS Multi-column Layout defines `column-width` as `auto | <length>` and
/// computes font-relative lengths before used column balancing:
/// <https://www.w3.org/TR/css-multicol-1/#cw>.
pub(super) fn parse_column_width(value: &str, font_size: f32) -> Option<ComputedColumnWidth> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        Some(ComputedColumnWidth::Auto)
    } else {
        parse_computed_length_percentage(value, font_size)
            .filter(|length| length.percent == 0.0)
            .filter(|length| !length_percentage_is_definitely_negative(*length))
            .map(ComputedColumnWidth::Length)
    }
}

/// Parses `column-gap` into its computed CSS value.
///
/// CSS Box Alignment defines `column-gap` as `normal | <length-percentage>`;
/// Multi-column layout gives `normal` a used value of `1em`, so the computed
/// keyword must be preserved until layout:
/// <https://www.w3.org/TR/css-align-3/#column-row-gap> and
/// <https://www.w3.org/TR/css-multicol-1/#cgap>.
pub(super) fn parse_column_gap(value: &str, font_size: f32) -> Option<ComputedGap> {
    if trim_css_value(value).eq_ignore_ascii_case("normal") {
        Some(ComputedGap::Normal)
    } else {
        let gap = parse_computed_length_percentage(value, font_size)?;
        (!length_percentage_is_definitely_negative(gap))
            .then_some(ComputedGap::LengthPercentage(gap))
    }
}

/// Parses `row-gap`/`gap` components into computed CSS gap values.
///
/// CSS Box Alignment defines gap properties as `normal | <length-percentage>`
/// and requires negative values to be invalid:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(super) fn parse_gap(value: &str, font_size: f32) -> Option<ComputedGap> {
    if trim_css_value(value).eq_ignore_ascii_case("normal") {
        Some(ComputedGap::Normal)
    } else {
        let gap = parse_computed_length_percentage(value, font_size)?;
        (!length_percentage_is_definitely_negative(gap))
            .then_some(ComputedGap::LengthPercentage(gap))
    }
}

/// Returns whether a computed gap cannot resolve to a non-negative used value.
///
/// CSS Box Alignment grammar marks gap lengths and percentages as
/// non-negative. Mixed length-percentage math may need a used percentage
/// basis, but values with no positive component are definitely negative and
/// invalid:
/// <https://www.w3.org/TR/css-align-3/#gaps> and
/// <https://www.w3.org/TR/css-values-4/#calc-range>.
fn length_percentage_is_definitely_negative(value: ComputedLengthPercentage) -> bool {
    let components = [value.length, value.percent, value.ch];
    components.iter().any(|component| *component < 0.0)
        && components.iter().all(|component| *component <= 0.0)
}
