use super::*;
use crate::units::IntoLayoutLength;

pub(in crate::layout::flex) fn flex_estimated_content_width(
    style: &ComputedStyle,
    available_content_width: PhysicalContentWidth,
) -> PhysicalContentWidth {
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    used_content_box_width_or_auto(
        style,
        available_content_width
            .content_box_length()
            .into_layout_length(),
        non_content_pt(horizontal_non_content),
    )
    .map(PhysicalContentWidth::new)
    .unwrap_or_else(|| {
        PhysicalContentWidth::new(content_box_pt(
            (available_content_width.points() - horizontal_non_content).max(1.0),
        ))
    })
}

pub(in crate::layout::flex) fn constrain_flex_item_estimated_height<Source>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    vertical_non_content: NonContentLength,
) -> ContentBoxLength {
    constrain_height_with_intrinsic(
        style,
        value,
        min_content,
        max_content,
        percentage_basis,
        vertical_non_content,
    )
}

/// Returns the flex gap contribution used by intrinsic max-content estimates.
///
/// CSS Box Alignment resolves cyclic percentage gaps against zero for
/// intrinsic size contributions, while preserving any non-percentage length
/// component:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(in crate::layout::flex) fn estimated_intrinsic_flex_gap(
    value: css::ComputedGap,
) -> LayoutLength {
    match value {
        css::ComputedGap::Normal => layout_pt(0.0),
        css::ComputedGap::LengthPercentage(value) => value.length_max_zero(),
    }
}

pub(in crate::layout::flex) fn estimated_intrinsic_length_percentage_or_auto(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: FlexAvailablePercentageBasis,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    let min_content_points = min_content.points();
    let max_content_points = max_content.points();
    let percentage_basis = percentage_basis.points();
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::Stretch => {
            percentage_basis.map(|basis| content_box_pt(basis.max(0.0)))
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.is_definitely_absolute() {
                Some(content_box_pt(value.length_max_zero().points()))
            } else {
                let basis = percentage_basis?;
                value
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        basis.max(0.0),
                    )))
                    .map(|length| content_box_pt(length.points().max(0.0)))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent => {
            Some(content_box_pt(min_content_points.max(0.0)))
        }
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(content_box_pt(
            max_content_points.max(min_content_points).max(0.0),
        )),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .clone()
                .and_then(|limit| {
                    percentage_basis.map(|basis| {
                        used_length_percentage(limit, PercentageBasis::definite(layout_pt(basis)))
                            .points()
                    })
                })
                .or_else(|| {
                    limit
                        .filter(|limit| !limit.needs_percentage_basis())
                        .map(|limit| limit.length_points())
                })
                .or(percentage_basis)
                .unwrap_or(max_content_points);
            Some(content_box_pt(
                max_content_points
                    .max(min_content_points)
                    .max(0.0)
                    .min(min_content_points.max(0.0).max(stretch.max(0.0))),
            ))
        }
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => {
            let percentage_basis = percentage_basis.unwrap_or(0.0);
            let stretch = percentage_basis.max(0.0);
            let fit_content = max_content_points
                .max(min_content_points)
                .min(min_content_points.max(stretch));
            Some(content_box_pt(
                value
                    .used_value(
                        max_content_points,
                        min_content_points,
                        max_content_points,
                        fit_content,
                        stretch,
                        PercentageBasis::definite(layout_pt(percentage_basis)),
                    )
                    .max(layout_pt(0.0))
                    .points(),
            ))
        }
    }
}
