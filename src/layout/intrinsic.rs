use crate::css::{self, ComputedStyle};
use crate::layout::used_values::{used_content_width, used_length_percentage};
use crate::text::FontSystem;

/// Return the advance of one punctuation glyph that CSS Text has made hangable.
///
/// The `hanging-punctuation` policy decides which glyph may hang before this
/// helper is called. Measuring the selected glyph separately keeps `last`,
/// `force-end`, and `allow-end` from re-checking each other's keywords:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(super) fn hanging_punctuation_character_width(
    font_system: &mut FontSystem,
    character: char,
    style: &ComputedStyle,
) -> f32 {
    font_system.measure_text(&character.to_string(), style)
}

// CSS 2.2 shrink-to-fit width: min(max(preferred-min, available), preferred).
pub(super) fn shrink_to_fit_width(preferred_min: f32, preferred: f32, available: f32) -> f32 {
    preferred_min.max(available).min(preferred)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum IntrinsicAutoWidth {
    FillAvailable,
    ShrinkToFit,
}

/// Resolve an intrinsic `width` keyword from known content-box contributions.
///
/// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
/// sizing keywords that consume a box's intrinsic min/max-content
/// contributions. Returning `None` for ordinary widths lets formatting
/// contexts keep their existing length/percentage and `auto` behavior:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
/// <https://www.w3.org/TR/css-sizing-3/#fit-content-size>.
pub(super) fn intrinsic_width_keyword(
    value: css::ComputedLengthPercentageOrAuto,
    min_content: f32,
    max_content: f32,
    available_outer_width: f32,
    horizontal_non_content: f32,
) -> Option<f32> {
    let min_content = min_content.max(0.0);
    let max_content = max_content.max(min_content).max(0.0);
    match value {
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_content),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(max_content),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = (available_outer_width - horizontal_non_content).max(0.0);
            let limit = limit
                .map(|limit| used_length_percentage(limit, available_outer_width).max(0.0))
                .unwrap_or(stretch);
            Some(max_content.min(min_content.max(limit)))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => None,
    }
}

/// Resolve a content-box width from intrinsic contributions and auto behavior.
///
/// CSS Sizing defines intrinsic width keywords, while CSS 2.2 gives different
/// `auto` width behavior to normal block boxes and shrink-to-fit formatting
/// contexts such as floats and atomic inline boxes:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
pub(super) fn content_width_from_intrinsic(
    style: &ComputedStyle,
    available_outer_width: f32,
    horizontal_non_content: f32,
    min_content: f32,
    max_content: f32,
    auto_width: IntrinsicAutoWidth,
) -> f32 {
    if let Some(width) = intrinsic_width_keyword(
        style.box_values.width,
        min_content,
        max_content,
        available_outer_width,
        horizontal_non_content,
    ) {
        return width;
    }

    match (style.box_values.width, auto_width) {
        (css::ComputedLengthPercentageOrAuto::Auto, IntrinsicAutoWidth::ShrinkToFit) => {
            shrink_to_fit_width(
                min_content.max(0.0),
                max_content.max(min_content).max(0.0),
                (available_outer_width - horizontal_non_content).max(0.0),
            )
        }
        _ => used_content_width(style, available_outer_width, horizontal_non_content),
    }
}
