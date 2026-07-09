use crate::css::{self, ComputedStyle};
use crate::layout::used_values::{
    stretch_fit_content_box_size, used_content_box_size, used_length_percentage,
};
use crate::text::FontSystem;
use crate::units::{
    ContentBoxLength, LayoutLength, NonContentLength, PercentageBasis, SemanticLengthExt,
    content_box_pt, layout_pt,
};

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
) -> LayoutLength {
    layout_pt(font_system.measure_text(&character.to_string(), style))
}

/// Resolve the CSS 2.2 shrink-to-fit content-box width.
///
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>
pub(super) fn shrink_to_fit_width(
    preferred_min: ContentBoxLength,
    preferred: ContentBoxLength,
    available: ContentBoxLength,
) -> ContentBoxLength {
    content_box_pt(
        preferred_min
            .points()
            .max(available.points())
            .min(preferred.points()),
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum IntrinsicAutoWidth {
    FillAvailable,
    ShrinkToFit,
}

/// Resolve an intrinsic `width` keyword into a content-box length.
///
/// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
/// sizing keywords that consume a box's intrinsic content-box contributions.
/// Returning `None` for ordinary widths lets formatting contexts keep their
/// existing length/percentage and `auto` behavior:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
/// <https://www.w3.org/TR/css-sizing-3/#fit-content-size>.
pub(super) fn intrinsic_content_box_width_keyword(
    value: css::ComputedLengthPercentageOrAuto,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    let min_content = min_content.points().max(0.0);
    let max_content = max_content.points().max(min_content).max(0.0);
    match value {
        css::ComputedLengthPercentageOrAuto::MinContent => Some(content_box_pt(min_content)),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(content_box_pt(max_content)),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch =
                (available_outer_width.points() - horizontal_non_content.points()).max(0.0);
            let limit = limit
                .map(|limit| {
                    used_length_percentage(limit, PercentageBasis::definite(available_outer_width))
                        .points()
                })
                .unwrap_or(stretch);
            Some(content_box_pt(max_content.min(min_content.max(limit))))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
        css::ComputedLengthPercentageOrAuto::Stretch => Some(stretch_fit_content_box_size(
            available_outer_width,
            layout_pt(0.0),
            horizontal_non_content,
        )),
    }
}

/// Resolve an intrinsic content-box width from a margin-box availability.
///
/// Margins and padding/border have distinct CSS box-model roles. The helper
/// keeps them typed at the formatting-context boundary, then combines their
/// point extents only for the legacy scalar intrinsic-size algorithm:
/// <https://www.w3.org/TR/css-sizing-3/#box-model> and
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
pub(super) fn content_box_width_from_intrinsic_in_margin_box(
    style: &ComputedStyle,
    available_margin_box_width: LayoutLength,
    horizontal_margin: LayoutLength,
    horizontal_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    auto_width: IntrinsicAutoWidth,
) -> ContentBoxLength {
    let available_outer_width =
        layout_pt((available_margin_box_width.points() - horizontal_margin.points()).max(0.0));
    content_box_width_from_intrinsic(
        style,
        available_outer_width,
        horizontal_non_content,
        min_content,
        max_content,
        auto_width,
    )
}

/// Resolve a content-box width from intrinsic contributions and auto behavior.
///
/// CSS Sizing defines intrinsic width keywords, while CSS 2.2 gives different
/// `auto` width behavior to normal block boxes and shrink-to-fit formatting
/// contexts such as floats and atomic inline boxes. The returned value stays in
/// the CSS content-box coordinate space:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
pub(super) fn content_box_width_from_intrinsic(
    style: &ComputedStyle,
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    auto_width: IntrinsicAutoWidth,
) -> ContentBoxLength {
    if let css::ComputedLengthPercentageOrAuto::CalcSize(calc_size) = &style.box_values.width {
        let min_content = min_content.points().max(0.0);
        let max_content = max_content.points().max(min_content);
        let stretch = (available_outer_width.points() - horizontal_non_content.points()).max(0.0);
        let auto_size = match auto_width {
            IntrinsicAutoWidth::FillAvailable => stretch,
            IntrinsicAutoWidth::ShrinkToFit => shrink_to_fit_width(
                content_box_pt(min_content),
                content_box_pt(max_content),
                content_box_pt(stretch),
            )
            .points(),
        };
        let fit_content = max_content.min(min_content.max(stretch));
        return content_box_pt(
            calc_size
                .used_value(
                    auto_size,
                    min_content,
                    max_content,
                    fit_content,
                    stretch,
                    PercentageBasis::definite(available_outer_width),
                )
                .max(layout_pt(0.0))
                .points(),
        );
    }
    if let Some(width) = intrinsic_content_box_width_keyword(
        style.box_values.width.clone(),
        min_content,
        max_content,
        available_outer_width,
        horizontal_non_content,
    ) {
        return width;
    }

    let min_content = min_content.points().max(0.0);
    let max_content = max_content.points().max(min_content).max(0.0);
    match (&style.box_values.width, auto_width) {
        // During intrinsic sizing a percentage-dependent width has no
        // definite containing-block basis, so it follows the property's
        // automatic-size branch. This includes `calc(5em - 0%)`: its zero
        // percentage is still percentage-dependent at this stage rather than
        // a fixed 5em width.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>.
        (css::ComputedLengthPercentageOrAuto::Auto, IntrinsicAutoWidth::ShrinkToFit) => {
            shrink_to_fit_width(
                content_box_pt(min_content.max(0.0)),
                content_box_pt(max_content.max(min_content).max(0.0)),
                content_box_pt(
                    (available_outer_width.points() - horizontal_non_content.points()).max(0.0),
                ),
            )
        }
        (
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value),
            IntrinsicAutoWidth::ShrinkToFit,
        ) if value.needs_percentage_basis() => shrink_to_fit_width(
            content_box_pt(min_content.max(0.0)),
            content_box_pt(max_content.max(min_content).max(0.0)),
            content_box_pt(
                (available_outer_width.points() - horizontal_non_content.points()).max(0.0),
            ),
        ),
        _ => used_content_box_size(
            style.box_values.width.clone(),
            style.box_sizing,
            PercentageBasis::definite(available_outer_width.cast_unit()),
            horizontal_non_content,
        )
        .unwrap_or_else(|| {
            content_box_pt(
                (available_outer_width.points() - horizontal_non_content.points()).max(0.0),
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::non_content_pt;

    #[test]
    fn intrinsic_margin_box_width_keeps_margin_and_non_content_distinct() {
        let width: ContentBoxLength = content_box_width_from_intrinsic_in_margin_box(
            &ComputedStyle::initial(),
            layout_pt(200.0),
            layout_pt(30.0),
            non_content_pt(20.0),
            content_box_pt(40.0),
            content_box_pt(80.0),
            IntrinsicAutoWidth::FillAvailable,
        );

        assert_eq!(width, content_box_pt(150.0));
    }
}
