use super::*;

/// Updates the temporary used line-height projection from the computed value.
///
/// CSS Cascade separates computed values from used values; this keeps the
/// legacy numeric layout fields derived from `ComputedLineHeight` until layout
/// can consume the typed value directly:
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
pub(crate) fn project_line_height(style: &mut ComputedStyle) {
    let (line_height, _, _) = style.line_height_value.clone().projected(style.font_size);
    style.line_height = line_height;
}

pub(crate) fn parse_line_height(value: &str, font_size: f32) -> Option<(f32, Option<f32>, bool)> {
    let mut style = ComputedStyle {
        font_size,
        ..ComputedStyle::initial()
    };
    style.line_height_value = parse_computed_line_height(value, font_size)?;
    project_line_height(&mut style);
    let (_, multiplier, is_normal) = style.line_height_value.clone().projected(style.font_size);
    Some((style.line_height, multiplier, is_normal))
}

pub(crate) fn set_font_size(style: &mut ComputedStyle, font_size: f32) {
    style.font_size = clamp_used_layout_length(layout_pt(font_size)).points();
    style.deferred_font_size = DeferredFontSize::Absolute(style.font_size);
    project_line_height(style);
}

pub(crate) fn set_deferred_font_size(
    style: &mut ComputedStyle,
    font_size: DeferredFontSize,
    parent_font_size: f32,
    parent_ch_advance: LayoutLength,
) {
    style.font_size = clamp_used_layout_length(
        font_size.resolve(
            crate::css::FontRelativeLengthBasis::new(
                layout_pt(parent_font_size),
                parent_ch_advance,
            )
            .with_line_height(layout_pt(style.line_height)),
        ),
    )
    .points();
    style.deferred_font_size = font_size;
    project_line_height(style);
}

pub(crate) fn fallback_ch_advance_for_style(style: &ComputedStyle) -> LayoutLength {
    fallback_ch_advance_for_font_metrics(
        style.font_size,
        style.writing_mode,
        style.text_orientation,
    )
}

pub(crate) fn fallback_ch_advance_for_font_metrics(
    font_size: f32,
    writing_mode: WritingMode,
    text_orientation: TextOrientation,
) -> LayoutLength {
    if matches!(
        writing_mode.text_layout_policy(text_orientation),
        TextLayoutPolicy::Vertical(TextOrientation::Upright)
    ) {
        layout_pt(font_size)
    } else {
        layout_pt(font_size * 0.5)
    }
}

pub(crate) fn parse_font_size(value: &str, parent_font_size: f32) -> Option<f32> {
    parse_font_size_with_parent_ch_advance(
        value,
        parent_font_size,
        layout_pt(parent_font_size * 0.5),
    )
}

/// Parses `font-size` without requiring a parent font metric.
///
/// The returned representation is resolved only once the parent's selected
/// font is known. This is the font-specific counterpart to deferred
/// `<length-percentage>` used-value resolution.
pub(crate) fn parse_deferred_font_size(value: &str) -> Option<DeferredFontSize> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    let absolute = match lower.as_str() {
        "xx-small" => Some(7.0),
        "x-small" => Some(8.3),
        "small" => Some(10.0),
        "medium" => Some(12.0),
        "large" => Some(14.4),
        "x-large" => Some(17.3),
        "xx-large" => Some(20.7),
        "xxx-large" => Some(24.9),
        _ => None,
    };
    if let Some(value) = absolute {
        return Some(DeferredFontSize::Absolute(value));
    }
    if lower == "smaller" {
        return Some(DeferredFontSize::RelativeToParent(
            ComputedLengthPercentage::from_em(1.0 / 1.2),
        ));
    }
    if lower == "larger" {
        return Some(DeferredFontSize::RelativeToParent(
            ComputedLengthPercentage::from_em(1.2),
        ));
    }
    if let Some(em) = lower
        .strip_suffix("em")
        .and_then(|value| value.parse::<f32>().ok())
    {
        return Some(DeferredFontSize::RelativeToParent(
            ComputedLengthPercentage::from_em(em),
        ));
    }
    parse_math_length_percentage_with_root(value, 0.0, ROOT_FONT_SIZE_PT)
        .map(DeferredFontSize::RelativeToParent)
        .or_else(|| parse_length(value).map(DeferredFontSize::Absolute))
}

pub(crate) fn parse_font_size_with_parent_ch_advance(
    value: &str,
    parent_font_size: f32,
    parent_ch_advance: LayoutLength,
) -> Option<f32> {
    parse_deferred_font_size(value).map(|value| {
        value
            .resolve(crate::css::FontRelativeLengthBasis::new(
                layout_pt(parent_font_size),
                parent_ch_advance,
            ))
            .points()
    })
}

/// Parses `line-height` into its computed CSS value.
///
/// CSS 2.2 computes `normal` and unitless numbers as keywords/numbers, while
/// lengths and percentages compute to absolute lengths:
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-line-height>.
pub(crate) fn parse_computed_line_height(
    value: &str,
    font_size: f32,
) -> Option<ComputedLineHeight> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(ComputedLineHeight::Normal);
    }
    if let Ok(multiplier) = value.parse::<f32>() {
        return Some(ComputedLineHeight::Number(multiplier));
    }
    if let Some(value) = parse_computed_length_percentage(value, font_size) {
        let value = value
            .clone()
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(font_size)))
            .map(ComputedLengthPercentage::from_layout_length)
            .unwrap_or(value);
        return Some(ComputedLineHeight::Length(value));
    }
    parse_length(value).map(ComputedLineHeight::from_points)
}
