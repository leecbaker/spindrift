use super::*;

/// Applies CSS min/max size constraints, with minimums overriding maximums.
///
/// CSS 2.2 applies max constraints first and min constraints second, so a
/// larger minimum size wins over a smaller maximum size:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths> and
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn constrain(mut value: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    if let Some(max) = max {
        value = value.min(max);
    }
    if let Some(min) = min {
        value = value.max(min);
    }
    value
}

/// Resolves CSS Sizing Level 4 stretch-fit sizing to a content-box size.
///
/// `stretch` attempts to make the margin box fill the available space. Layout
/// callers pass the already-resolved available margin-box size, used margins,
/// and padding plus border for the relevant axis, and the content box is
/// floored at zero:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
/// Resolves CSS Sizing Level 4 stretch-fit sizing to a content-box length.
///
/// `stretch` attempts to make the margin box fill the available space. The
/// result is a CSS content-box length in Quire's PDF-point layout scalar, with
/// padding and border represented as an explicit semantic non-content length:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn stretch_fit_content_box_size(
    available_margin_box_size: LayoutLength,
    margin_size: LayoutLength,
    non_content_size: NonContentLength,
) -> ContentBoxLength {
    content_box_pt(
        (available_margin_box_size.points() - margin_size.points() - non_content_size.points())
            .max(0.0),
    )
}
/// Resolves used content width, falling back to filling available space for `auto`.
///
/// CSS 2.2 defines block-width used-value resolution and CSS Box Sizing defines
/// how `box-sizing` changes the content-box size:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn used_content_box_width(
    style: &ComputedStyle,
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
) -> ContentBoxLength {
    used_content_box_size(
        style.box_values.width.clone(),
        style.box_sizing,
        PercentageBasis::definite(crate::units::layout_to_content_box_length(
            available_outer_width,
        )),
        horizontal_non_content,
    )
    .unwrap_or_else(|| {
        content_box_pt((available_outer_width.points() - horizontal_non_content.points()).max(0.0))
    })
}

/// Resolves a specified content width against a typed physical availability.
///
/// CSS percentage widths use the containing block's inline size; this entry
/// point preserves the resulting content-box quantity until the caller enters
/// a coordinate or rendering algorithm.
pub(in crate::layout) fn used_content_box_width_or_auto(
    style: &ComputedStyle,
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size(
        style.box_values.width.clone(),
        style.box_sizing,
        PercentageBasis::definite(crate::units::layout_to_content_box_length(
            available_outer_width,
        )),
        horizontal_non_content,
    )
}

/// Resolves a specified content width against a typed percentage basis.
pub(in crate::layout) fn used_content_box_width_or_auto_with_basis<Source>(
    style: &ComputedStyle,
    available_outer_width: PercentageBasis<ContentBoxLength, Source>,
    horizontal_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size_with_basis(
        style.box_values.width.clone(),
        style.box_sizing,
        available_outer_width,
        horizontal_non_content,
    )
}

/// Resolves a specified content height against a typed physical availability.
///
/// CSS percentage heights use the containing block's block-size basis; an
/// automatic height remains unresolved for its formatting context to handle.
pub(in crate::layout) fn used_content_box_height_or_auto(
    style: &ComputedStyle,
    available_outer_height: LayoutLength,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size(
        style.box_values.height.value().clone(),
        style.box_sizing,
        PercentageBasis::definite(crate::units::layout_to_content_box_length(
            available_outer_height,
        )),
        vertical_non_content,
    )
}

/// Resolves a specified content height against a typed percentage basis.
pub(in crate::layout) fn used_content_box_height_or_auto_with_basis<Source>(
    style: &ComputedStyle,
    available_outer_height: PercentageBasis<ContentBoxLength, Source>,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size_with_basis(
        style.box_values.height.value().clone(),
        style.box_sizing,
        available_outer_height,
        vertical_non_content,
    )
}

/// Transfer a definite non-replaced content width through its preferred aspect
/// ratio to obtain an automatic content height.
///
/// The ratio applies in the box defined by `box-sizing`; converting both
/// dimensions through the content box keeps borders and padding from being
/// counted asymmetrically. Constraint application remains with the formatting
/// context because its percentage basis and intrinsic contributions are
/// context-dependent.
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
pub(in crate::layout) fn non_replaced_aspect_ratio_content_height(
    style: &ComputedStyle,
    content_width: f32,
    horizontal_non_content: f32,
    vertical_non_content: f32,
) -> Option<f32> {
    let calc_size = style.box_values.height.calc_size_with_auto_basis();
    if !style.box_values.height.is_auto() && calc_size.is_none() {
        return None;
    }
    let ratio = style.aspect_ratio.preferred_ratio_for_non_replaced(false)?;
    if ratio <= 0.0 || !ratio.is_finite() {
        return None;
    }
    let height = match (
        style.box_sizing,
        style.aspect_ratio.uses_content_box_for_non_replaced(),
    ) {
        (_, true) | (BoxSizing::ContentBox, false) => content_width / ratio,
        (BoxSizing::BorderBox, false) => {
            let border_box_width = content_width + horizontal_non_content;
            (border_box_width / ratio) - vertical_non_content
        }
    };
    Some(
        calc_size
            .map(|value| {
                value
                    .used_value(
                        height,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        PercentageBasis::definite(layout_pt(0.0)),
                    )
                    .points()
            })
            .unwrap_or(height)
            .max(0.0),
    )
}

/// Transfer a definite non-replaced content height through its preferred
/// aspect ratio to obtain an automatic content width.
///
/// This is the reciprocal of [`non_replaced_aspect_ratio_content_height`];
/// keeping both conversions here makes each formatting context choose a used
/// size without reimplementing `box-sizing` arithmetic.
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
pub(in crate::layout) fn non_replaced_aspect_ratio_content_width(
    style: &ComputedStyle,
    content_height: f32,
    horizontal_non_content: f32,
    vertical_non_content: f32,
) -> Option<f32> {
    let calc_size = style.box_values.width.clone().calc_size_with_auto_basis();
    if !style.box_values.width.clone().is_auto() && calc_size.is_none() {
        return None;
    }
    let ratio = style.aspect_ratio.preferred_ratio_for_non_replaced(false)?;
    if ratio <= 0.0 || !ratio.is_finite() {
        return None;
    }
    let width = match (
        style.box_sizing,
        style.aspect_ratio.uses_content_box_for_non_replaced(),
    ) {
        (_, true) | (BoxSizing::ContentBox, false) => content_height * ratio,
        (BoxSizing::BorderBox, false) => {
            let border_box_height = content_height + vertical_non_content;
            (border_box_height * ratio) - horizontal_non_content
        }
    };
    Some(
        calc_size
            .map(|value| {
                value
                    .used_value(
                        width,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        PercentageBasis::definite(layout_pt(0.0)),
                    )
                    .points()
            })
            .unwrap_or(width)
            .max(0.0),
    )
}

/// Resolves a width/height value to a typed content-box used size.
///
/// CSS Box Sizing defines conversion between border-box and content-box sizes:
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
///
/// The returned value is a CSS content-box length in Quire's PDF-point layout
/// scalar. Callers should keep this typed until they cross a layout/paint or
/// external adapter boundary.
pub(in crate::layout) fn used_content_box_size<Source>(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size_with_basis(value, box_sizing, percentage_basis, non_content)
}

/// Resolves a width/height value to a typed content-box used size.
///
/// CSS Sizing treats pure lengths as definite without a percentage basis, while
/// percentage sizes are definite only when the containing block axis is
/// definite. CSS Box Sizing then maps the specified content-box or border-box
/// value into the content-box coordinate space:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn used_content_box_size_with_basis<Source>(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    let specified = match value {
        css::ComputedLengthPercentageOrAuto::Auto => return None,
        css::ComputedLengthPercentageOrAuto::Stretch => {
            return percentage_basis.points().map(|basis| {
                stretch_fit_content_box_size(layout_pt(basis), layout_pt(0.0), non_content)
            });
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            layout_points(used_length_percentage_with_basis(value, percentage_basis)?)
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => return None,
    };
    Some(used_content_box_size_from_specified(
        specified,
        box_sizing,
        non_content,
    ))
}

fn used_content_box_size_from_specified(
    specified: f32,
    box_sizing: BoxSizing,
    non_content: NonContentLength,
) -> ContentBoxLength {
    match box_sizing {
        BoxSizing::BorderBox => {
            border_box_to_content_box_length(border_box_pt(specified), non_content)
        }
        BoxSizing::ContentBox => content_box_pt(specified.max(0.0)),
    }
}

/// Resolves used `min-width`.
///
/// CSS 2.2 defines min/max width constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(in crate::layout) fn used_min_width<T, Source>(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<ContentBoxLength>
where
    T: SemanticLengthExt,
{
    used_length_percentage_or_auto(style.box_values.min_width.clone(), percentage_basis)
        .map(|value| content_box_pt(value.points().max(0.0)))
}

/// Resolves used `max-width`.
///
/// CSS 2.2 defines min/max width constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(in crate::layout) fn used_max_width<T, Source>(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<ContentBoxLength>
where
    T: SemanticLengthExt,
{
    used_length_percentage_or_auto(style.box_values.max_width.clone(), percentage_basis)
        .map(|value| content_box_pt(value.points().max(0.0)))
}

/// Return whether a sizing value needs intrinsic min/max-content contributions.
///
/// CSS Sizing defines these keywords in terms of intrinsic size contributions,
/// so callers must measure contents before resolving them:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>.
pub(in crate::layout) fn needs_intrinsic_width_contribution(
    value: css::ComputedLengthPercentageOrAuto,
) -> bool {
    matches!(
        value,
        css::ComputedLengthPercentageOrAuto::MinContent
            | css::ComputedLengthPercentageOrAuto::MaxContent
            | css::ComputedLengthPercentageOrAuto::FitContent(_)
    ) || matches!(&value, css::ComputedLengthPercentageOrAuto::CalcSize(value) if value.needs_intrinsic_size())
        // A percentage-dependent width has no used value during intrinsic
        // sizing. CSS Sizing therefore measures the contents instead of
        // prematurely using the fixed component of `calc()`, including an
        // authored `0%` component.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>.
        || matches!(&value, css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.needs_percentage_basis())
}

/// Return whether a block-axis sizing value needs intrinsic min/max-content contributions.
///
/// CSS Sizing defines block-axis `min-content` and `max-content` sizes in
/// terms of the content's intrinsic block size. Normal block containers have
/// equivalent min-content and max-content block sizes, but callers still need
/// the laid-out content height before resolving these keywords:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
/// <https://github.com/w3c/csswg-drafts/issues/3973>.
pub(in crate::layout) fn needs_intrinsic_height_contribution(
    value: css::ComputedLengthPercentageOrAuto,
) -> bool {
    matches!(
        value,
        css::ComputedLengthPercentageOrAuto::MinContent
            | css::ComputedLengthPercentageOrAuto::MaxContent
            | css::ComputedLengthPercentageOrAuto::FitContent(_)
    ) || matches!(value, css::ComputedLengthPercentageOrAuto::CalcSize(value) if value.needs_intrinsic_size())
}

/// Resolves used `min-height`.
///
/// CSS 2.2 defines min/max height constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn used_min_height<T, Source>(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<ContentBoxLength>
where
    T: SemanticLengthExt,
{
    match style.box_values.min_height.clone() {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            // A cyclic min-size percentage contributes zero, rather than
            // discarding an accompanying fixed `calc()` term.  For example,
            // `min-height: calc(25px + 50%)` in an auto-height containing
            // block has a used minimum of 25px.
            // <https://www.w3.org/TR/css-sizing-3/#cyclic-percentage-contribution>
            Some(content_box_pt(
                used_length_percentage(value, percentage_basis)
                    .points()
                    .max(0.0),
            ))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
    }
}

/// Resolves used `max-height`.
///
/// CSS 2.2 defines min/max height constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn used_max_height<T, Source>(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<ContentBoxLength>
where
    T: SemanticLengthExt,
{
    used_length_percentage_or_auto(style.box_values.max_height.clone(), percentage_basis)
        .map(|value| content_box_pt(value.points().max(0.0)))
}

/// Applies non-intrinsic used min/max width constraints to a content width.
///
/// CSS 2.2 defines min/max width constraint application. Intrinsic sizing
/// keywords need content measurements and are intentionally ignored here; use
/// `constrain_width_with_intrinsic` when min/max-content contributions are
/// available:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths> and
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>.
pub(in crate::layout) trait ConstraintPercentageBasis {
    fn into_layout_basis(self) -> PercentageBasis<LayoutLength>;
}

impl<T, Source> ConstraintPercentageBasis for PercentageBasis<T, Source>
where
    T: crate::units::IntoLayoutLength,
{
    fn into_layout_basis(self) -> PercentageBasis<LayoutLength> {
        match self {
            PercentageBasis::Definite { value, .. } => {
                PercentageBasis::definite(crate::units::IntoLayoutLength::into_layout_length(value))
            }
            PercentageBasis::Indefinite => PercentageBasis::indefinite(),
        }
    }
}

/// Applies used width constraints to a typed content-box size.
///
pub(in crate::layout) fn constrain_content_width<B>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    percentage_basis: B,
) -> ContentBoxLength
where
    B: ConstraintPercentageBasis,
{
    let percentage_basis = percentage_basis.into_layout_basis();
    content_box_pt(constrain(
        value.points(),
        used_min_width(style, percentage_basis).map(SemanticLengthExt::points),
        used_max_width(style, percentage_basis).map(SemanticLengthExt::points),
    ))
}

/// Apply width constraints, resolving `stretch` against the available margin
/// box rather than treating it as an ordinary length.
///
/// CSS Sizing defines stretch-fit sizing on the margin box. Normal-flow block
/// width resolution reaches this point after margin values are known, which is
/// the shared boundary where a stretch min/max constraint can retain that
/// distinction from a content-box length.
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>
pub(in crate::layout) fn constrain_width_with_stretch_fit(
    style: &ComputedStyle,
    value: ContentBoxLength,
    available_margin_box_width: LayoutLength,
    horizontal_margin: LayoutLength,
    horizontal_non_content: NonContentLength,
) -> ContentBoxLength {
    let stretch = || {
        stretch_fit_content_box_size(
            available_margin_box_width,
            horizontal_margin,
            horizontal_non_content,
        )
        .points()
    };
    let min = if style.box_values.min_width == css::ComputedLengthPercentageOrAuto::Stretch {
        Some(stretch())
    } else {
        used_min_width(style, PercentageBasis::definite(available_margin_box_width))
            .map(SemanticLengthExt::points)
    };
    let max = if style.box_values.max_width == css::ComputedLengthPercentageOrAuto::Stretch {
        Some(stretch())
    } else {
        used_max_width(style, PercentageBasis::definite(available_margin_box_width))
            .map(SemanticLengthExt::points)
    };
    content_box_pt(constrain(value.points(), min, max))
}

/// Applies min/max width constraints, including intrinsic sizing keywords.
///
/// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` in
/// terms of a box's intrinsic contributions. This helper keeps the tentative
/// width in content-box space and converts definite constraint arguments
/// through `box-sizing` before clamping:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>,
/// <https://www.w3.org/TR/css-sizing-3/#fit-content-size>, and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn constrain_width_with_intrinsic<Source>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    horizontal_non_content: NonContentLength,
) -> ContentBoxLength {
    let min_content = content_box_pt(min_content.points().max(0.0));
    let max_content = content_box_pt(max_content.points().max(min_content.points()).max(0.0));
    let percentage_basis = match percentage_basis {
        PercentageBasis::Definite { value, .. } => PercentageBasis::definite(value),
        PercentageBasis::Indefinite => PercentageBasis::indefinite(),
    };
    let min_constraint = intrinsic_width_constraint(
        style.box_values.min_width.clone(),
        style.box_sizing,
        percentage_basis,
        horizontal_non_content,
        min_content,
        max_content,
    );
    let max_constraint = intrinsic_width_constraint(
        style.box_values.max_width.clone(),
        style.box_sizing,
        percentage_basis,
        horizontal_non_content,
        min_content,
        max_content,
    );
    content_box_pt(
        constrain(
            value.points().max(0.0),
            min_constraint.map(SemanticLengthExt::points),
            max_constraint.map(SemanticLengthExt::points),
        )
        .max(0.0),
    )
}

pub(in crate::layout) fn intrinsic_width_constraint(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength>,
    horizontal_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_content),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(max_content),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .map(|limit| {
                    intrinsic_constraint_length(
                        limit,
                        box_sizing,
                        percentage_basis,
                        horizontal_non_content,
                    )
                })
                .unwrap_or_else(|| {
                    percentage_basis.points().map(|basis| {
                        stretch_fit_content_box_size(
                            layout_pt(basis),
                            layout_pt(0.0),
                            horizontal_non_content,
                        )
                    })
                })?;
            Some(content_box_pt(
                max_content
                    .points()
                    .min(min_content.points().max(stretch.points()))
                    .max(0.0),
            ))
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            intrinsic_constraint_length(value, box_sizing, percentage_basis, horizontal_non_content)
        }
        css::ComputedLengthPercentageOrAuto::Stretch => percentage_basis.points().map(|basis| {
            stretch_fit_content_box_size(layout_pt(basis), layout_pt(0.0), horizontal_non_content)
        }),
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => calc_size_intrinsic_constraint(
            value,
            box_sizing,
            percentage_basis,
            horizontal_non_content,
            min_content,
            max_content,
        ),
    }
}

fn intrinsic_constraint_length(
    value: css::ComputedLengthPercentage,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength>,
    horizontal_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    // Intrinsic max-size contributions use an indefinite basis for cyclic
    // percentages. An authored `0%` remains a percentage in CSS syntax, so
    // `calc(45px + 0%)` is just as cyclic as `50%` here and max-width must
    // fall back to its initial value rather than impose a 45px cap. Numeric
    // resolution alone cannot express that distinction after canonicalizing
    // a length-percentage expression.
    // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
    if !percentage_basis.is_definite() && value.contains_percentage() {
        return None;
    }
    let specified = used_length_percentage_with_basis(value, percentage_basis)?.points();
    Some(used_content_box_size_from_specified(
        specified,
        box_sizing,
        horizontal_non_content,
    ))
}

/// Resolves a calc-size constraint after intrinsic contributions are known.
///
/// Intrinsic sizing supplies a zero percentage basis for cyclic percentages,
/// while the retained calc-size basis is selected from the same content-box
/// contributions as CSS Sizing's intrinsic keywords. The calculation itself
/// happens in the property's specified box space before CSS box-sizing maps it
/// back to the content box:
/// <https://drafts.csswg.org/css-values-5/#calc-size> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn calc_size_intrinsic_constraint(
    value: css::CalcSize,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength>,
    non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    let percentage_basis_points = percentage_basis.points().unwrap_or(0.0);
    let to_specified = |content: ContentBoxLength| match box_sizing {
        BoxSizing::ContentBox => content.points(),
        BoxSizing::BorderBox => content.points() + non_content.points(),
    };
    let min_content = to_specified(min_content);
    let max_content = to_specified(max_content).max(min_content);
    let stretch = stretch_fit_content_box_size(
        layout_pt(percentage_basis_points),
        layout_pt(0.0),
        non_content,
    )
    .points();
    let fit_content = max_content.min(min_content.max(stretch));
    let basis = match &value.basis {
        css::CalcSizeBasis::Auto | css::CalcSizeBasis::MinContent => min_content,
        css::CalcSizeBasis::MaxContent => max_content,
        css::CalcSizeBasis::FitContent => fit_content,
        css::CalcSizeBasis::Stretch => stretch,
        css::CalcSizeBasis::LengthPercentage(value) => {
            used_length_percentage_with_basis(value.clone(), percentage_basis)?.points()
        }
    };
    Some(used_content_box_size_from_specified(
        value
            .used_value(
                basis,
                min_content,
                max_content,
                fit_content,
                stretch,
                percentage_basis,
            )
            .max(layout_pt(0.0))
            .points(),
        box_sizing,
        non_content,
    ))
}

/// Return non-replaced intrinsic inline-size contributions in content-box space.
///
/// CSS Sizing defines intrinsic contributions for non-replaced boxes by
/// treating cyclic percentage preferred and max sizes as their initial values
/// while resolving min-size percentages against zero. Preferred sizes such as
/// `fit-content(<length-percentage>)` are applied to the measured content
/// contribution before min/max constraints:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution> and
/// <https://www.w3.org/TR/css-sizing-3/#valdef-width-fit-content-length-percentage>.
pub(in crate::layout) fn non_replaced_intrinsic_width_contributions(
    style: &ComputedStyle,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    horizontal_non_content: NonContentLength,
) -> (ContentBoxLength, ContentBoxLength) {
    let intrinsic_min_content = content_box_pt(min_content.points().max(0.0));
    let intrinsic_max_content = content_box_pt(
        max_content
            .points()
            .max(intrinsic_min_content.points())
            .max(0.0),
    );
    let preferred_width = non_replaced_intrinsic_preferred_width(
        style.box_values.width.clone(),
        style.box_sizing,
        horizontal_non_content,
        intrinsic_min_content,
        intrinsic_max_content,
    );
    // Constraints select their `calc-size()` intrinsic bases from the raw
    // content contributions. A ratio-derived preferred width is instead the
    // tentative size to which those constraints apply. Conflating the two
    // would make `min-width: calc-size(auto, size - 50px)` use the unbounded
    // raw contribution as the final intrinsic contribution.
    // <https://drafts.csswg.org/css-sizing-3/#intrinsic-contribution> and
    // <https://drafts.csswg.org/css-values-5/#calc-size>.
    // CSS Sizing resolves cyclic percentage min sizes against zero, while a
    // cyclic percentage preferred or max size is treated as its initial value.
    // Keep those distinct at the typed percentage-basis boundary: fixed
    // components still resolve through `Indefinite`, but max-width and
    // fit-content percentage components remain absent instead of becoming a
    // spurious zero cap.
    // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
    let intrinsic_min_percentage_basis = PercentageBasis::definite(content_box_pt(0.0));
    let intrinsic_max_percentage_basis = PercentageBasis::indefinite();
    let min_constraint = intrinsic_width_constraint(
        style.box_values.min_width.clone(),
        style.box_sizing,
        intrinsic_min_percentage_basis,
        horizontal_non_content,
        intrinsic_min_content,
        intrinsic_max_content,
    )
    .map(SemanticLengthExt::points);
    let max_constraint = intrinsic_width_constraint(
        style.box_values.max_width.clone(),
        style.box_sizing,
        intrinsic_max_percentage_basis,
        horizontal_non_content,
        intrinsic_min_content,
        intrinsic_max_content,
    )
    .map(SemanticLengthExt::points);
    // A definite preferred block size transfers through a preferred aspect
    // ratio when computing a non-replaced box's intrinsic inline
    // contributions.  Resolve block min/max constraints first, in content-box
    // space, so `box-sizing: border-box` and padding affect the transferred
    // inline size exactly once.
    // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
    let transferred_aspect_width = (!matches!(
        style.box_values.width.clone(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
    ))
    .then(|| {
        let vertical_non_content = intrinsic_box_metrics(style).vertical_non_content_length();
        used_content_box_size(
            style.box_values.height.value().clone(),
            style.box_sizing,
            PercentageBasis::definite(content_box_pt(0.0)),
            vertical_non_content,
        )
        .map(|height| {
            constrain_height_with_intrinsic(
                style,
                height,
                height,
                height,
                PercentageBasis::definite(content_box_pt(0.0)),
                vertical_non_content,
            )
            .points()
        })
        .and_then(|height| {
            non_replaced_aspect_ratio_content_width(
                style,
                height,
                horizontal_non_content.points(),
                vertical_non_content.points(),
            )
        })
    })
    .flatten();
    let preferred_min_content = preferred_width
        .map(|value| value.0)
        .unwrap_or(intrinsic_min_content)
        .points();
    let preferred_max_content = preferred_width
        .map(|value| value.1)
        .unwrap_or(intrinsic_max_content)
        .points();
    // A definite block size and automatic inline size select the aspect-ratio
    // transfer as the inline preferred size. The content contribution remains
    // available above solely for the automatic min-size constraint.
    // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>.
    let (min_content, max_content) = if style.box_values.width.clone().is_auto() {
        transferred_aspect_width
            .map(|width| (width, width))
            .unwrap_or((preferred_min_content, preferred_max_content))
    } else {
        (
            preferred_min_content.max(transferred_aspect_width.unwrap_or(0.0)),
            preferred_max_content.max(transferred_aspect_width.unwrap_or(0.0)),
        )
    };
    (
        content_box_pt(constrain(min_content, min_constraint, max_constraint).max(0.0)),
        content_box_pt(constrain(max_content, min_constraint, max_constraint).max(0.0)),
    )
}

/// Apply non-replaced intrinsic inline-size constraints to content widths.
///
/// Transitional raw compatibility wrapper for
/// `non_replaced_intrinsic_width_contributions`.
pub(in crate::layout) fn constrain_non_replaced_intrinsic_widths(
    style: &ComputedStyle,
    min_content: f32,
    max_content: f32,
    horizontal_non_content: f32,
) -> (f32, f32) {
    let (min_content, max_content) = non_replaced_intrinsic_width_contributions(
        style,
        content_box_pt(min_content),
        content_box_pt(max_content),
        non_content_pt(horizontal_non_content),
    );
    (min_content.points(), max_content.points())
}

fn non_replaced_intrinsic_preferred_width(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    horizontal_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<(ContentBoxLength, ContentBoxLength)> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            non_cyclic_content_box_length(value, box_sizing, horizontal_non_content)
                .map(|value| (value, value))
        }
        css::ComputedLengthPercentageOrAuto::MinContent => Some((min_content, min_content)),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some((max_content, max_content)),
        css::ComputedLengthPercentageOrAuto::FitContent(Some(limit)) => {
            non_replaced_intrinsic_fit_content_width(
                limit,
                box_sizing,
                horizontal_non_content,
                min_content,
                max_content,
            )
            .map(|value| (value, value))
        }
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => {
            let min_content_points = min_content.points();
            let max_content_points = max_content.points();
            let intrinsic_basis = |min_size: f32, max_size: f32| match &value.basis {
                css::CalcSizeBasis::Auto
                | css::CalcSizeBasis::FitContent
                | css::CalcSizeBasis::MinContent => (min_size, max_size),
                css::CalcSizeBasis::MaxContent => (max_size, max_size),
                css::CalcSizeBasis::Stretch => (0.0, 0.0),
                css::CalcSizeBasis::LengthPercentage(basis) => {
                    let basis = used_length_percentage(
                        basis.clone(),
                        PercentageBasis::definite(layout_pt(0.0)),
                    )
                    .points();
                    (basis, basis)
                }
            };
            let (min_basis, max_basis) = intrinsic_basis(min_content_points, max_content_points);
            let min_specified = value
                .used_value(
                    min_basis,
                    min_content_points,
                    max_content_points,
                    min_basis,
                    0.0,
                    PercentageBasis::definite(layout_pt(0.0)),
                )
                .points();
            let max_specified = value
                .used_value(
                    max_basis,
                    min_content_points,
                    max_content_points,
                    max_basis,
                    0.0,
                    PercentageBasis::definite(layout_pt(0.0)),
                )
                .points();
            Some((
                used_content_box_size_from_specified(
                    min_specified.max(0.0),
                    box_sizing,
                    horizontal_non_content,
                ),
                used_content_box_size_from_specified(
                    max_specified.max(0.0),
                    box_sizing,
                    horizontal_non_content,
                ),
            ))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::FitContent(None)
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
    }
}

fn non_replaced_intrinsic_fit_content_width(
    limit: css::ComputedLengthPercentage,
    box_sizing: BoxSizing,
    horizontal_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    let limit = non_cyclic_content_box_length(limit, box_sizing, horizontal_non_content)?;
    Some(content_box_pt(
        max_content
            .points()
            .min(min_content.points().max(limit.points()))
            .max(0.0),
    ))
}

fn non_cyclic_content_box_length(
    value: css::ComputedLengthPercentage,
    box_sizing: BoxSizing,
    horizontal_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    let specified = value.length_if_no_percent()?;
    Some(used_content_box_size_from_specified(
        specified,
        box_sizing,
        horizontal_non_content,
    ))
}

/// Applies used min/max height constraints to a content height.
///
/// CSS 2.2 defines min/max height constraint application:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
/// Applies used height constraints to a typed content-box size.
///
pub(in crate::layout) fn constrain_content_height<B>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    percentage_basis: B,
) -> ContentBoxLength
where
    B: ConstraintPercentageBasis,
{
    let percentage_basis = percentage_basis.into_layout_basis();
    let vertical_non_content = intrinsic_box_metrics(style).vertical_non_content_length();
    let content_box_constraint = |value: ContentBoxLength| {
        used_content_box_size_from_specified(
            value.points().max(0.0),
            style.box_sizing,
            vertical_non_content,
        )
        .points()
    };
    content_box_pt(constrain(
        value.points(),
        used_min_height(style, percentage_basis).map(content_box_constraint),
        used_max_height(style, percentage_basis).map(content_box_constraint),
    ))
}

/// Apply block-size constraints, resolving `stretch` against the available
/// containing-block size before margins are applied.
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>
pub(in crate::layout) fn constrain_height_with_stretch_fit(
    style: &ComputedStyle,
    value: ContentBoxLength,
    available_margin_box_height: LayoutLength,
    _vertical_margin: LayoutLength,
    vertical_non_content: NonContentLength,
) -> ContentBoxLength {
    let stretch = || {
        stretch_fit_content_box_size(
            available_margin_box_height,
            layout_pt(0.0),
            vertical_non_content,
        )
        .points()
    };
    let min = if style.box_values.min_height == css::ComputedLengthPercentageOrAuto::Stretch {
        Some(stretch())
    } else {
        used_min_height(
            style,
            PercentageBasis::definite(available_margin_box_height),
        )
        .map(|value| {
            used_content_box_size_from_specified(
                value.points(),
                style.box_sizing,
                vertical_non_content,
            )
            .points()
        })
    };
    let max = if style.box_values.max_height == css::ComputedLengthPercentageOrAuto::Stretch {
        Some(stretch())
    } else {
        used_max_height(
            style,
            PercentageBasis::definite(available_margin_box_height),
        )
        .map(|value| {
            used_content_box_size_from_specified(
                value.points(),
                style.box_sizing,
                vertical_non_content,
            )
            .points()
        })
    };
    content_box_pt(constrain(value.points(), min, max))
}

/// Applies min/max height constraints, including intrinsic sizing keywords.
///
/// CSS 2.2 applies `min-height`/`max-height` by recalculating the used height
/// with the constraining property substituted for `height`. CSS Sizing extends
/// those properties with intrinsic keywords; for normal block containers the
/// min-content and max-content block sizes are both the laid-out content block
/// size measured with cyclic block-axis percentages unresolved:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>,
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>, and
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>.
pub(in crate::layout) fn constrain_height_with_intrinsic<Source>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    vertical_non_content: NonContentLength,
) -> ContentBoxLength {
    let min_content = content_box_pt(min_content.points().max(0.0));
    let max_content = content_box_pt(max_content.points().max(min_content.points()).max(0.0));
    let percentage_basis = match percentage_basis {
        PercentageBasis::Definite { value, .. } => PercentageBasis::definite(value),
        PercentageBasis::Indefinite => PercentageBasis::indefinite(),
    };
    let min_constraint = intrinsic_height_constraint(
        style.box_values.min_height.clone(),
        style.box_sizing,
        percentage_basis,
        vertical_non_content,
        min_content,
        max_content,
    );
    let max_constraint = intrinsic_height_constraint(
        style.box_values.max_height.clone(),
        style.box_sizing,
        percentage_basis,
        vertical_non_content,
        min_content,
        max_content,
    );
    content_box_pt(
        constrain(
            value.points().max(0.0),
            min_constraint.map(SemanticLengthExt::points),
            max_constraint.map(SemanticLengthExt::points),
        )
        .max(0.0),
    )
}

fn intrinsic_height_constraint(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength>,
    vertical_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_content),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(max_content),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .map(|limit| {
                    intrinsic_constraint_length(
                        limit,
                        box_sizing,
                        percentage_basis,
                        vertical_non_content,
                    )
                })
                .unwrap_or_else(|| {
                    percentage_basis.points().map(|basis| {
                        stretch_fit_content_box_size(
                            layout_pt(basis),
                            layout_pt(0.0),
                            vertical_non_content,
                        )
                    })
                })?;
            Some(content_box_pt(
                max_content
                    .points()
                    .min(min_content.points().max(stretch.points()))
                    .max(0.0),
            ))
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            intrinsic_constraint_length(value, box_sizing, percentage_basis, vertical_non_content)
        }
        css::ComputedLengthPercentageOrAuto::Stretch => percentage_basis.points().map(|basis| {
            stretch_fit_content_box_size(layout_pt(basis), layout_pt(0.0), vertical_non_content)
        }),
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => calc_size_intrinsic_constraint(
            value,
            box_sizing,
            percentage_basis,
            vertical_non_content,
            min_content,
            max_content,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn length_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn percent_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(value),
        )
    }

    fn fit_content(points: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::FitContent(Some(
            css::ComputedLengthPercentage::from_points(points),
        ))
    }

    fn fit_content_percent(percent: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::FitContent(Some(
            css::ComputedLengthPercentage::from_percent(percent),
        ))
    }

    fn intrinsic_width_constraint_result(style: &ComputedStyle, value: f32) -> f32 {
        constrain_width_with_intrinsic(
            style,
            content_box_pt(value),
            content_box_pt(60.0),
            content_box_pt(120.0),
            PercentageBasis::definite(content_box_pt(300.0)),
            non_content_pt(0.0),
        )
        .points()
    }

    #[test]
    fn intrinsic_fit_content_min_width_clamps_tentative_content_width() {
        let mut style = ComputedStyle::initial();
        style.box_values.min_width = fit_content(100.0);

        assert_eq!(intrinsic_width_constraint_result(&style, 10.0), 100.0);
    }

    #[test]
    fn intrinsic_fit_content_max_width_clamps_tentative_content_width() {
        let mut style = ComputedStyle::initial();
        style.box_values.max_width = fit_content(100.0);

        assert_eq!(intrinsic_width_constraint_result(&style, 150.0), 100.0);
    }

    #[test]
    fn intrinsic_min_and_max_content_constraints_clamp_content_width() {
        let mut min_style = ComputedStyle::initial();
        min_style.box_values.min_width = css::ComputedLengthPercentageOrAuto::MinContent;
        let mut max_style = ComputedStyle::initial();
        max_style.box_values.max_width = css::ComputedLengthPercentageOrAuto::MaxContent;

        assert_eq!(intrinsic_width_constraint_result(&min_style, 10.0), 60.0);
        assert_eq!(intrinsic_width_constraint_result(&max_style, 150.0), 120.0);
    }

    #[test]
    fn intrinsic_width_constraints_convert_border_box_limits_to_content_box() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.min_width = fit_content(100.0);

        let constrained = constrain_width_with_intrinsic(
            &style,
            content_box_pt(10.0),
            content_box_pt(60.0),
            content_box_pt(120.0),
            PercentageBasis::definite(content_box_pt(300.0)),
            non_content_pt(20.0),
        );

        assert_eq!(constrained.points(), 80.0);
    }

    #[test]
    fn non_replaced_intrinsic_width_uses_fit_content_length_preferred_size() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = fit_content(100.0);

        let contributions = non_replaced_intrinsic_width_contributions(
            &style,
            content_box_pt(60.0),
            content_box_pt(120.0),
            non_content_pt(0.0),
        );

        assert_eq!(contributions.0.points(), 100.0);
        assert_eq!(contributions.1.points(), 100.0);
    }

    #[test]
    fn non_replaced_intrinsic_width_treats_fit_content_percentage_as_auto() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = fit_content_percent(0.5);

        let contributions = non_replaced_intrinsic_width_contributions(
            &style,
            content_box_pt(100.0),
            content_box_pt(200.0),
            non_content_pt(0.0),
        );

        assert_eq!(contributions.0.points(), 100.0);
        assert_eq!(contributions.1.points(), 200.0);
    }

    #[test]
    fn non_replaced_intrinsic_width_converts_border_box_preferred_size() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.width = length_auto(100.0);

        let contributions = non_replaced_intrinsic_width_contributions(
            &style,
            content_box_pt(60.0),
            content_box_pt(120.0),
            non_content_pt(20.0),
        );

        assert_eq!(contributions.0.points(), 80.0);
        assert_eq!(contributions.1.points(), 80.0);
    }

    #[test]
    fn non_replaced_intrinsic_width_preserves_min_and_cyclic_max_constraints() {
        let mut style = ComputedStyle::initial();
        style.box_values.min_width = percent_auto(0.5);
        style.box_values.max_width = fit_content_percent(0.5);

        let contributions = non_replaced_intrinsic_width_contributions(
            &style,
            content_box_pt(20.0),
            content_box_pt(120.0),
            non_content_pt(0.0),
        );

        assert_eq!(contributions.0.points(), 20.0);
        assert_eq!(contributions.1.points(), 120.0);
    }

    #[test]
    fn cyclic_min_height_keeps_the_fixed_calc_component() {
        let mut style = ComputedStyle::initial();
        style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(18.75), 0.5, true),
        );

        let used = used_min_height(&style, PercentageBasis::<ContentBoxLength>::indefinite())
            .expect("a fixed calc component remains a used min-height");

        assert_eq!(used.points(), 18.75);
    }
    #[test]
    fn typed_constraint_entry_points_preserve_content_box_lengths() {
        let style = ComputedStyle::initial();
        let width: ContentBoxLength = constrain_content_width(
            &style,
            content_box_pt(42.0),
            PercentageBasis::definite(layout_pt(100.0)),
        );
        let height: ContentBoxLength = constrain_content_height(
            &style,
            content_box_pt(24.0),
            PercentageBasis::definite(layout_pt(100.0)),
        );

        assert_eq!(width.points(), 42.0);
        assert_eq!(height.points(), 24.0);
    }
    fn length(points: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(points),
        )
    }

    fn percent(percent: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(percent),
        )
    }

    #[test]
    fn stretch_fit_content_size_is_non_negative_after_borders() {
        let non_content = 200.0;
        let content = stretch_fit_content_box_size(
            layout_pt(0.0),
            layout_pt(0.0),
            non_content_pt(non_content),
        )
        .points();

        assert_eq!(content, 0.0);
        assert_eq!(content + non_content, 200.0);
    }

    #[test]
    fn used_content_box_size_keeps_content_box_specified_size() {
        let content = used_content_box_size(
            length(120.0),
            BoxSizing::ContentBox,
            PercentageBasis::definite(content_box_pt(300.0)),
            non_content_pt(40.0),
        )
        .expect("content-box length should resolve");

        assert_eq!(content.points(), 120.0);
    }

    #[test]
    fn used_content_box_size_subtracts_border_box_non_content_and_clamps() {
        let content = used_content_box_size(
            length(100.0),
            BoxSizing::BorderBox,
            PercentageBasis::definite(content_box_pt(300.0)),
            non_content_pt(150.0),
        )
        .expect("border-box length should resolve");

        assert_eq!(content.points(), 0.0);
    }

    #[test]
    fn stretch_fit_content_box_size_returns_typed_non_negative_content() {
        let content =
            stretch_fit_content_box_size(layout_pt(100.0), layout_pt(30.0), non_content_pt(120.0));

        assert_eq!(content.points(), 0.0);
    }

    #[test]
    fn used_content_box_size_indefinite_basis_resolves_lengths_not_percentages() {
        let length_content = used_content_box_size_with_basis(
            length(42.0),
            BoxSizing::ContentBox,
            PercentageBasis::<ContentBoxLength>::indefinite(),
            non_content_pt(10.0),
        )
        .expect("length-only value should resolve without a basis");
        let percentage_content = used_content_box_size_with_basis(
            percent(0.5),
            BoxSizing::ContentBox,
            PercentageBasis::<ContentBoxLength>::indefinite(),
            non_content_pt(10.0),
        );

        assert_eq!(length_content.points(), 42.0);
        assert!(percentage_content.is_none());
    }
}
