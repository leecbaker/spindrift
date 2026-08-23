use std::num::NonZeroUsize;

use super::*;

pub(super) fn apply_columns(value: &str, style: &mut ComputedStyle) {
    let Some(longhands) = crate::css::cascade::declarations::expand_columns_shorthand(value) else {
        return;
    };
    for (name, value) in longhands {
        match name {
            "column-count" => {
                if let Some(count) = parse_column_count(&value) {
                    style.column_count = count;
                }
            }
            "column-width" => {
                if let Some(width) = parse_column_width(&value, style.font_size) {
                    style.column_width = width;
                }
            }
            "column-height" => {
                if let Some(height) = parse_column_height(&value, style.font_size) {
                    style.column_height = height;
                }
            }
            "column-wrap" => {
                if let Some(wrap) = parse_column_wrap(&value) {
                    style.column_wrap = wrap;
                }
            }
            _ => {}
        }
    }
}

pub(super) fn parse_column_count(value: &str) -> Option<ColumnCount> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(ColumnCount::Auto);
    }
    value
        .parse::<u16>()
        .ok()
        .filter(|count| *count > 0)
        .and_then(|count| NonZeroUsize::new(usize::from(count)))
        .map(ColumnCount::Count)
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
            .filter(|length| !length.contains_percentage())
            .filter(|length| !length_percentage_is_definitely_negative(length))
            .map(ComputedColumnWidth::Length)
    }
}

/// Parses CSS Multi-column Level 2 `column-height`.
/// <https://drafts.csswg.org/css-multicol-2/#column-height>
pub(super) fn parse_column_height(value: &str, font_size: f32) -> Option<ComputedColumnHeight> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        Some(ComputedColumnHeight::Auto)
    } else {
        parse_computed_length_percentage(value, font_size)
            .filter(|length| !length.contains_percentage())
            .filter(|length| !length_percentage_is_definitely_negative(length))
            .map(ComputedColumnHeight::Length)
    }
}

pub(super) fn parse_column_wrap(value: &str) -> Option<ColumnWrap> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(ColumnWrap::Auto),
        "nowrap" => Some(ColumnWrap::Nowrap),
        "wrap" => Some(ColumnWrap::Wrap),
        _ => None,
    }
}

/// Parses `column-gap` into its computed CSS value.
///
/// CSS Gaps defines `column-gap` as `normal | <length-percentage> |
/// <line-width>`. Multi-column layout gives `normal` a used value of `1em`,
/// so the computed keyword must be preserved until layout:
/// <https://drafts.csswg.org/css-gaps-1/#column-row-gap> and
/// <https://www.w3.org/TR/css-multicol-1/#cgap>.
pub(super) fn parse_column_gap(value: &str, font_size: f32) -> Option<ComputedGap> {
    if trim_css_value(value).eq_ignore_ascii_case("normal") {
        Some(ComputedGap::Normal)
    } else {
        let gap = parse_computed_border_width(value, font_size)
            .or_else(|| parse_computed_length_percentage(value, font_size))?;
        (!length_percentage_is_definitely_negative(&gap))
            .then_some(ComputedGap::LengthPercentage(gap))
    }
}

pub(super) fn parse_column_fill(value: &str) -> Option<ColumnFill> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "balance" => Some(ColumnFill::Balance),
        "balance-all" => Some(ColumnFill::BalanceAll),
        "auto" => Some(ColumnFill::Auto),
        _ => None,
    }
}

pub(super) fn parse_column_span(value: &str) -> Option<ColumnSpan> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "none" => Some(ColumnSpan::None),
        "all" => Some(ColumnSpan::All),
        _ => None,
    }
}

/// Parses `row-gap`/`gap` components into computed CSS gap values.
///
/// CSS Gaps defines gap properties as `normal | <length-percentage> |
/// <line-width>` and requires negative values to be invalid:
/// <https://drafts.csswg.org/css-gaps-1/#column-row-gap>.
pub(super) fn parse_gap(value: &str, font_size: f32) -> Option<ComputedGap> {
    if trim_css_value(value).eq_ignore_ascii_case("normal") {
        Some(ComputedGap::Normal)
    } else {
        let gap = parse_computed_border_width(value, font_size)
            .or_else(|| parse_computed_length_percentage(value, font_size))?;
        (!length_percentage_is_definitely_negative(&gap))
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
fn length_percentage_is_definitely_negative(value: &ComputedLengthPercentage) -> bool {
    value.is_definitely_absolute() && value.length_points() < 0.0
}
