use super::*;
use crate::document::paint::geometry::{Affine3dPaintTransform, Projective3dPaintTransform};

/// The untransformed box against which an HTML principal box resolves its
/// transform-origin and percentage translations.
///
/// Paint bounds are intentionally not accepted here: ink can be smaller than
/// the principal box, while CSS Transforms resolves the reference box after
/// layout. Tables are the one CSS-layout exception: their transform reference
/// box is the table wrapper's border box for both `content-box` and
/// `border-box`.
/// <https://drafts.csswg.org/css-transforms-1/#transform-box>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum TransformReferenceBox {
    CssLayoutBorderBox(PaintRect),
    TableWrapperBorderBox(PaintRect),
}

impl TransformReferenceBox {
    pub(in crate::layout) fn css_layout(border_box: PaintRect) -> Self {
        Self::CssLayoutBorderBox(border_box)
    }

    pub(in crate::layout) fn table_wrapper(border_box: PaintRect) -> Self {
        Self::TableWrapperBorderBox(border_box)
    }

    fn resolve(self, style: &ComputedStyle) -> PaintRect {
        match self {
            Self::CssLayoutBorderBox(border_box) => html_transform_reference_box(style, border_box),
            // CSS Transforms deliberately makes both content-box and
            // border-box use the table wrapper border box. Do not run the
            // normal CSS-box content inset adapter here.
            Self::TableWrapperBorderBox(border_box) => border_box,
        }
    }
}

#[cfg(test)]
fn paint_transform_for_box(style: &ComputedStyle, border_box: PaintRect) -> Option<PaintTransform> {
    paint_transform_for_reference_box(style, TransformReferenceBox::css_layout(border_box))
}

pub(in crate::layout) fn paint_transform_for_reference_box(
    style: &ComputedStyle,
    reference_box: TransformReferenceBox,
) -> Option<PaintTransform> {
    if !style.has_transform() {
        return None;
    }
    let reference_box = reference_box.resolve(style);
    if transform_list_contains_3d(&style.transform) {
        return affine_3d_paint_transform_for_resolved_reference_box(style, reference_box)
            .map(Affine3dPaintTransform::flatten_to_paint_transform);
    }
    let origin = style
        .transform_origin
        .clone()
        .resolve_against_paint_rect(reference_box);
    let transform = compose_css_transform_matrix(
        origin,
        style.individual_transforms.clone(),
        &style.transform,
        |function| transform_function_matrix(function, reference_box.size),
    );
    Some(PaintTransform::from_transform(normalize_affine_transform(
        transform,
    )))
}

pub(in crate::layout) fn transform_list_contains_3d(
    transform_list: &[css::TransformFunction],
) -> bool {
    transform_list.iter().any(|function| {
        matches!(
            function,
            css::TransformFunction::Matrix3D(_)
                | css::TransformFunction::Translate3D(_)
                | css::TransformFunction::Scale3D(_)
                | css::TransformFunction::Rotate3D(_)
                | css::TransformFunction::Perspective(_)
        )
    })
}

type PaintTransform3D = euclid::Transform3D<f32, PaintSpace, PaintSpace>;

/// Resolve a local CSS 3D transform into an affine homogeneous paint matrix.
///
/// `None` represents a projective matrix, which the PDF backend deliberately
/// does not approximate. Callers that need to distinguish a 2D style from an
/// unsupported 3D value should first use [`transform_list_contains_3d`].
#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::layout) fn affine_3d_paint_transform_for_reference_box(
    style: &ComputedStyle,
    reference_box: TransformReferenceBox,
) -> Option<Affine3dPaintTransform> {
    if !style.has_transform() {
        return None;
    }
    affine_3d_paint_transform_for_resolved_reference_box(style, reference_box.resolve(style))
}

/// Resolve a CSS 3D transform without discarding perspective terms.
///
/// This is the retained-scene boundary. The caller may subsequently select
/// the affine PDF fast path through `try_into_affine_pdf_ctm`, but projective
/// matrices remain explicit until primitive lowering.
pub(in crate::layout) fn projective_3d_paint_transform_for_reference_box(
    style: &ComputedStyle,
    reference_box: TransformReferenceBox,
) -> Option<Projective3dPaintTransform> {
    style.has_transform().then(|| {
        let reference_box = reference_box.resolve(style);
        let origin = style
            .transform_origin
            .clone()
            .resolve_3d_against_paint_rect(reference_box);
        Projective3dPaintTransform::from_transform(compose_css_transform_3d_matrix(
            origin,
            style.individual_transforms.clone(),
            &style.transform,
            reference_box.size,
        ))
    })
}

/// Matrix introduced by the `perspective` property for an element's
/// descendants.  The owner itself stays in its parent's plane; only a child
/// rendering context is multiplied by this matrix.
/// <https://drafts.csswg.org/css-transforms-2/#perspective-property>
pub(in crate::layout) fn perspective_property_3d_transform_for_reference_box(
    style: &ComputedStyle,
    reference_box: TransformReferenceBox,
) -> Option<Projective3dPaintTransform> {
    let distance = style.perspective.used_for_rendering()?.points();
    let origin = style
        .perspective_origin
        .clone()
        .resolve_against_paint_rect(reference_box.resolve(style));
    let projection = PaintTransform3D::new(
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        -1.0 / distance,
        0.0,
        0.0,
        0.0,
        1.0,
    );
    Some(Projective3dPaintTransform::from_transform(
        PaintTransform3D::translation(-origin.x, -origin.y, 0.0)
            .then(&projection)
            .then(&PaintTransform3D::translation(origin.x, origin.y, 0.0)),
    ))
}

fn affine_3d_paint_transform_for_resolved_reference_box(
    style: &ComputedStyle,
    reference_box: PaintRect,
) -> Option<Affine3dPaintTransform> {
    let origin = style
        .transform_origin
        .clone()
        .resolve_3d_against_paint_rect(reference_box);
    Affine3dPaintTransform::try_from_transform(compose_css_transform_3d_matrix(
        origin,
        style.individual_transforms.clone(),
        &style.transform,
        reference_box.size,
    ))
}

/// Compose a CSS transform list in homogeneous 3D space, including ordinary
/// 2D functions.  CSS Transforms Level 2 defines 2D functions as their 3D
/// matrix equivalents; keeping that identity here prevents mixed 2D/3D lists
/// from being flattened function-by-function in the wrong order:
/// <https://drafts.csswg.org/css-transforms-2/#ctm>.
fn compose_css_transform_3d_matrix(
    origin: euclid::Point3D<f32, PaintSpace>,
    individual: css::IndividualTransforms,
    transform_list: &[css::TransformFunction],
    reference_size: PaintSize,
) -> PaintTransform3D {
    let mut transform = PaintTransform3D::translation(origin.x, origin.y, origin.z);
    if let Some(translation) = individual.translate {
        transform = paint_transform_3d_function_matrix(
            css::TransformFunction::Translate(translation),
            reference_size,
        )
        .then(&transform);
    }
    if let Some(angle) = individual.rotate {
        transform = paint_transform_3d_function_matrix(
            css::TransformFunction::Rotate(angle),
            reference_size,
        )
        .then(&transform);
    }
    if let Some(scale) = individual.scale {
        transform = paint_transform_3d_function_matrix(
            css::TransformFunction::Scale(scale),
            reference_size,
        )
        .then(&transform);
    }
    for function in transform_list {
        transform =
            paint_transform_3d_function_matrix(function.clone(), reference_size).then(&transform);
    }
    PaintTransform3D::translation(-origin.x, -origin.y, -origin.z).then(&transform)
}

/// Convert one typed CSS function into page-paint homogeneous coordinates.
fn paint_transform_3d_function_matrix(
    function: css::TransformFunction,
    reference_size: PaintSize,
) -> PaintTransform3D {
    match function {
        css::TransformFunction::Matrix3D(matrix) => css_matrix_3d_to_paint(matrix.0),
        css::TransformFunction::Translate3D(translation) => PaintTransform3D::translation(
            used_length_percentage(
                translation.x,
                PercentageBasis::definite(layout_pt(reference_size.width)),
            )
            .points(),
            -used_length_percentage(
                translation.y,
                PercentageBasis::definite(layout_pt(reference_size.height)),
            )
            .points(),
            used_length_percentage(translation.z, PercentageBasis::definite(layout_pt(0.0)))
                .points(),
        ),
        css::TransformFunction::Scale3D(scale) => {
            PaintTransform3D::scale(scale.x, scale.y, scale.z)
        }
        css::TransformFunction::Rotate3D(rotation) => {
            css_matrix_3d_to_paint(css::CssTransform3D::rotation(
                rotation.axis_x,
                rotation.axis_y,
                rotation.axis_z,
                rotation.angle,
            ))
        }
        css::TransformFunction::Perspective(perspective) => {
            let Some(distance) = perspective
                .used_for_rendering()
                .map(|distance| distance.points())
            else {
                return PaintTransform3D::identity();
            };
            // A nonzero m34 makes the result projective. The affine extractor
            // below detects this and leaves perspective explicitly unsupported.
            PaintTransform3D::new(
                1.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                -1.0 / distance,
                0.0,
                0.0,
                0.0,
                1.0,
            )
        }
        function => promote_paint_transform(transform_function_matrix(function, reference_size)),
    }
}

/// Project a CSS-pixel, y-down 3D matrix into Quire's point, y-up paint
/// coordinate system. This is `S · M · S⁻¹` for
/// `S = diag(CSS_PX_TO_PT, -CSS_PX_TO_PT, CSS_PX_TO_PT, 1)`.
fn css_matrix_3d_to_paint(matrix: css::CssTransform3D) -> PaintTransform3D {
    PaintTransform3D::new(
        matrix.m11,
        -matrix.m12,
        matrix.m13,
        matrix.m14 / css::CSS_PX_TO_PT,
        -matrix.m21,
        matrix.m22,
        -matrix.m23,
        -matrix.m24 / css::CSS_PX_TO_PT,
        matrix.m31,
        -matrix.m32,
        matrix.m33,
        matrix.m34 / css::CSS_PX_TO_PT,
        matrix.m41 * css::CSS_PX_TO_PT,
        -matrix.m42 * css::CSS_PX_TO_PT,
        matrix.m43 * css::CSS_PX_TO_PT,
        matrix.m44,
    )
}

fn promote_paint_transform(
    transform: euclid::Transform2D<f32, PaintSpace, PaintSpace>,
) -> PaintTransform3D {
    PaintTransform3D::new(
        transform.m11,
        transform.m12,
        0.0,
        0.0,
        transform.m21,
        transform.m22,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        transform.m31,
        transform.m32,
        0.0,
        1.0,
    )
}

/// Resolve the transform reference box for an HTML layout box.
///
/// `view-box`, `fill-box`, and `stroke-box` are SVG concepts.  The CSS
/// Transforms fallback for an HTML box is its border box, while `content-box`
/// changes both transform-origin and percentage translation bases.
/// <https://drafts.csswg.org/css-transforms-1/#transform-box-property>
fn html_transform_reference_box(style: &ComputedStyle, border_box: PaintRect) -> PaintRect {
    if !style.transform_box.html_reference_is_content_box() {
        return border_box;
    }
    let borders = used_border_widths(style);
    let padding = used_padding_for_transform_reference(style, border_box, borders);
    let horizontal_inset = borders.left + padding.left;
    let vertical_inset = borders.bottom + padding.bottom;
    PaintRect::new(
        PaintPoint::new(
            border_box.origin.x + horizontal_inset,
            border_box.origin.y + vertical_inset,
        ),
        PaintSize::new(
            (border_box.size.width - horizontal_inset - borders.right - padding.right).max(0.0),
            (border_box.size.height - vertical_inset - borders.top - padding.top).max(0.0),
        ),
    )
}

/// Return used padding for a transform reference box when a block's declared
/// content width lets us reconstruct its percentage basis from its final
/// border box.
///
/// Percentage padding resolves against the containing block's inline size,
/// not the transformed element's own width. Most layout paths carry the
/// resolved value in `style.padding`; for an ordinary content-sized block
/// with percentage padding, derive that same basis from the final border box
/// and the definite content width. This keeps transform-box at the used-value
/// boundary rather than treating the computed percentage as zero.
/// <https://www.w3.org/TR/css-box-3/#padding-physical>
fn used_padding_for_transform_reference(
    style: &ComputedStyle,
    border_box: PaintRect,
    borders: css::Edges,
) -> css::Edges {
    let padding = &style.box_values.padding;
    if ![&padding.left, &padding.right, &padding.top, &padding.bottom]
        .into_iter()
        .any(|padding| padding.contains_percentage())
    {
        return style.padding;
    }
    let Some(content_width) = style.box_values.width.length_if_no_percent() else {
        return style.padding;
    };
    if style.box_sizing != css::BoxSizing::ContentBox {
        return style.padding;
    }
    let horizontal_percentage = padding.left.percentage_coefficient_or_zero()
        + padding.right.percentage_coefficient_or_zero();
    if horizontal_percentage <= 0.0 {
        return style.padding;
    }
    let zero_basis = PercentageBasis::definite(layout_pt(0.0));
    let fixed_horizontal_padding = used_length_percentage(padding.left.clone(), zero_basis)
        .points()
        + used_length_percentage(padding.right.clone(), zero_basis).points();
    let available_padding = border_box.size.width - borders.left - borders.right - content_width;
    let inline_basis = (available_padding - fixed_horizontal_padding) / horizontal_percentage;
    if !inline_basis.is_finite() || inline_basis < 0.0 {
        return style.padding;
    }
    let inline_basis = PercentageBasis::definite(layout_pt(inline_basis));
    css::Edges {
        top: used_length_percentage(padding.top.clone(), inline_basis).points(),
        right: used_length_percentage(padding.right.clone(), inline_basis).points(),
        bottom: used_length_percentage(padding.bottom.clone(), inline_basis).points(),
        left: used_length_percentage(padding.left.clone(), inline_basis).points(),
    }
}

/// Compose CSS 2D transform components around an already-resolved origin.
///
/// CSS Transforms defines the order as origin translation, independent
/// `translate`/`rotate`/`scale`, the legacy `transform` list, then inverse
/// origin translation. Box and SVG paint resolve their length bases
/// differently, but share this component ordering exactly:
/// <https://www.w3.org/TR/css-transforms-2/#ctm>.
pub(in crate::layout) fn compose_css_transform_matrix<Space>(
    origin: euclid::Point2D<f32, Space>,
    individual: css::IndividualTransforms,
    transform_list: &[css::TransformFunction],
    mut function_matrix: impl FnMut(css::TransformFunction) -> euclid::Transform2D<f32, Space, Space>,
) -> euclid::Transform2D<f32, Space, Space> {
    let mut transform = euclid::Transform2D::translation(origin.x, origin.y);
    if let Some(translation) = individual.translate {
        transform =
            function_matrix(css::TransformFunction::Translate(translation)).then(&transform);
    }
    if let Some(angle) = individual
        .rotate
        .filter(|angle| !is_exact_turn_identity(*angle))
    {
        transform = function_matrix(css::TransformFunction::Rotate(angle)).then(&transform);
    }
    if let Some(scale) = individual.scale {
        transform = function_matrix(css::TransformFunction::Scale(scale)).then(&transform);
    }
    for function in transform_list {
        if matches!(function, css::TransformFunction::Rotate(angle) if is_exact_turn_identity(*angle))
        {
            continue;
        }
        transform = function_matrix(function.clone()).then(&transform);
    }
    normalize_affine_transform(
        euclid::Transform2D::translation(-origin.x, -origin.y).then(&transform),
    )
}

/// Whether an already-computed CSS rotation is exactly an integral number of
/// turns in its finite `f32` representation.
///
/// A complete turn is an identity transform. Eliminating it before matrix
/// multiplication avoids trigonometric round-off changing an otherwise
/// identical transform list; near-turn values intentionally remain rotations.
/// <https://drafts.csswg.org/css-transforms-1/#funcdef-transform-rotate>
fn is_exact_turn_identity(angle: euclid::Angle<f32>) -> bool {
    angle.radians.is_finite() && angle.radians.rem_euclid(std::f32::consts::TAU) == 0.0
}

/// Canonicalize trigonometric identity noise so equivalent CSS transform
/// lists serialize to the same affine CTM (for example `rotate(45deg)
/// rotate(360deg)` and `rotate(45deg)`).
fn normalize_affine_transform<Space>(
    transform: euclid::Transform2D<f32, Space, Space>,
) -> euclid::Transform2D<f32, Space, Space> {
    const EPSILON: f32 = 1e-6;
    fn canonical(value: f32) -> f32 {
        if value.abs() < EPSILON {
            0.0
        } else if (value - 1.0).abs() < EPSILON {
            1.0
        } else if (value + 1.0).abs() < EPSILON {
            -1.0
        } else {
            value
        }
    }
    let mut normalized = euclid::Transform2D::new(
        canonical(transform.m11),
        canonical(transform.m12),
        canonical(transform.m21),
        canonical(transform.m22),
        canonical(transform.m31),
        canonical(transform.m32),
    );
    // A transform list containing an identity rotation can leave independent
    // sin/cos rounding noise in an otherwise pure rotation. Reconstruct its
    // orthonormal basis from one angle so semantically identical lists emit
    // the same PDF CTM. The translation is derived from that basis and only
    // needs sub-millipoint canonicalization to discard multiplication noise.
    let first_column_length = normalized.m11.hypot(normalized.m12);
    let second_column_length = normalized.m21.hypot(normalized.m22);
    let dot = normalized.m11 * normalized.m21 + normalized.m12 * normalized.m22;
    let determinant = normalized.m11 * normalized.m22 - normalized.m12 * normalized.m21;
    if (first_column_length - 1.0).abs() < 1e-4
        && (second_column_length - 1.0).abs() < 1e-4
        && dot.abs() < 1e-4
        && (determinant - 1.0).abs() < 1e-4
    {
        let angle = (-normalized.m12).atan2(normalized.m11);
        let (sin, cos) = angle.sin_cos();
        normalized = euclid::Transform2D::new(
            canonical(cos),
            canonical(-sin),
            canonical(sin),
            canonical(cos),
            (normalized.m31 * 1000.0).round() / 1000.0,
            (normalized.m32 * 1000.0).round() / 1000.0,
        );
    }
    normalized
}

pub(in crate::layout) fn transform_function_matrix(
    function: css::TransformFunction,
    border_box_size: PaintSize,
) -> euclid::Transform2D<f32, PaintSpace, PaintSpace> {
    match function {
        css::TransformFunction::Matrix(matrix) => {
            matrix.into_y_up_space(euclid::Scale::new(css::CSS_PX_TO_PT))
        }
        css::TransformFunction::Translate(translation) => euclid::Transform2D::translation(
            used_length_percentage(
                translation.x,
                PercentageBasis::definite(layout_pt(border_box_size.width)),
            )
            .points(),
            // CSS transform coordinates have a downward-positive block axis,
            // whereas page paint coordinates use PDF's upward-positive axis.
            // The layout-to-paint boundary must therefore invert physical
            // translations before a transformed containing block captures its
            // positioned descendants.
            // <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
            -used_length_percentage(
                translation.y,
                PercentageBasis::definite(layout_pt(border_box_size.height)),
            )
            .points(),
        ),
        css::TransformFunction::Scale(scale) => euclid::Transform2D::scale(scale.x, scale.y),
        // CSS positive angles are clockwise in its y-down coordinate system.
        // Page paint uses y-up PDF coordinates, so the projected angle is
        // counter-clockwise.
        css::TransformFunction::Rotate(angle) => euclid::Transform2D::rotation(-angle),
        css::TransformFunction::Skew(angles) => euclid::Transform2D::new(
            1.0,
            -angles.y.radians.tan(),
            -angles.x.radians.tan(),
            1.0,
            0.0,
            0.0,
        ),
        // SVG uses its own typed scene transform bridge and deliberately
        // rejects 3D CSS transforms until it can supply SVG reference boxes
        // and a 3D viewport projection.
        css::TransformFunction::Matrix3D(_)
        | css::TransformFunction::Translate3D(_)
        | css::TransformFunction::Scale3D(_)
        | css::TransformFunction::Rotate3D(_)
        | css::TransformFunction::Perspective(_) => euclid::Transform2D::identity(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{ComputedLengthPercentage, CssScaleFactors, CssTransformTranslation};

    fn compose_test_rotation(
        individual: css::IndividualTransforms,
        transform_list: &[css::TransformFunction],
    ) -> euclid::Transform2D<f32, PaintSpace, PaintSpace> {
        compose_css_transform_matrix(
            PaintPoint::new(0.0, 0.0),
            individual,
            transform_list,
            |function| transform_function_matrix(function, PaintSize::new(0.0, 0.0)),
        )
    }

    #[test]
    fn exact_full_turn_rotations_do_not_change_composed_affine_matrix() {
        let reference = compose_test_rotation(
            css::IndividualTransforms::NONE,
            &[css::TransformFunction::Rotate(euclid::Angle::degrees(45.0))],
        );
        let full_turns = [
            ("degrees", 360.0_f32.to_radians()),
            ("grads", 400.0 * std::f32::consts::PI / 200.0),
            ("turns", std::f32::consts::TAU),
        ];
        for (unit, full_turn) in full_turns {
            assert!(
                is_exact_turn_identity(euclid::Angle::radians(full_turn)),
                "{unit} full turn must retain its exact parsed f32 representation",
            );
            let actual = compose_test_rotation(
                css::IndividualTransforms::NONE,
                &[
                    css::TransformFunction::Rotate(euclid::Angle::degrees(45.0)),
                    css::TransformFunction::Rotate(euclid::Angle::radians(full_turn)),
                ],
            );
            assert_eq!(actual, reference, "{unit} full turn must be an identity");
        }

        let individual_full_turn = compose_test_rotation(
            css::IndividualTransforms {
                translate: None,
                rotate: Some(euclid::Angle::radians(std::f32::consts::TAU)),
                scale: None,
            },
            &[],
        );
        assert_eq!(individual_full_turn, euclid::Transform2D::identity());
        assert!(!is_exact_turn_identity(euclid::Angle::radians(
            std::f32::consts::TAU - 0.0001,
        )));
    }

    #[test]
    fn typed_border_box_resolves_origin_and_percentage_translation_in_order() {
        let mut style = ComputedStyle::initial();
        style.individual_transforms.translate = Some(CssTransformTranslation {
            x: ComputedLengthPercentage::from_percent(0.25),
            y: ComputedLengthPercentage::from_percent(0.5),
        });
        style
            .transform
            .push(css::TransformFunction::Scale(CssScaleFactors {
                x: 2.0,
                y: 2.0,
            }));
        let border_box = paint_space_rect(10.0, 20.0, 100.0, 50.0);

        let transform = paint_transform_for_box(&style, border_box)
            .expect("individual or legacy transforms make this box transformed");

        // T(origin) · translate(25%, 50%) · scale(2) · T(-origin).
        assert_eq!(transform.a(), 2.0);
        assert_eq!(transform.d(), 2.0);
        assert_eq!(transform.e(), -35.0);
        assert_eq!(transform.f(), -70.0);
    }

    #[test]
    fn css_matrix_is_projected_from_css_y_down_to_paint_y_up() {
        let matrix = css::CssAffineMatrix::new(1.0, 0.0, 0.0, 1.0, 10.0, 20.0);
        let transform = transform_function_matrix(
            css::TransformFunction::Matrix(matrix),
            PaintSize::new(100.0, 100.0),
        );

        assert_eq!(transform.m31, 10.0 * css::CSS_PX_TO_PT);
        assert_eq!(transform.m32, -20.0 * css::CSS_PX_TO_PT);
    }

    #[test]
    fn css_rotation_and_skew_are_projected_from_y_down_to_y_up() {
        let rotation = transform_function_matrix(
            css::TransformFunction::Rotate(euclid::Angle::degrees(90.0)),
            PaintSize::new(100.0, 100.0),
        );
        assert!(rotation.m11.abs() < 1e-6);
        assert_eq!(rotation.m12, -1.0);
        assert_eq!(rotation.m21, 1.0);
        assert!(rotation.m22.abs() < 1e-6);

        let skew = transform_function_matrix(
            css::TransformFunction::Skew(css::CssSkewAngles {
                x: euclid::Angle::degrees(45.0),
                y: euclid::Angle::degrees(45.0),
            }),
            PaintSize::new(100.0, 100.0),
        );
        assert_eq!(skew.m12, -1.0);
        assert_eq!(skew.m21, -1.0);
    }

    #[test]
    fn content_box_changes_transform_origin_and_percentage_translation_basis() {
        let mut style = ComputedStyle::initial();
        style.transform_box = css::TransformBox::ContentBox;
        style.padding.left = 10.0;
        style.padding.right = 10.0;
        style.padding.top = 5.0;
        style.padding.bottom = 5.0;
        style
            .transform
            .push(css::TransformFunction::Translate(CssTransformTranslation {
                x: ComputedLengthPercentage::from_percent(0.5),
                y: ComputedLengthPercentage::from_percent(0.5),
            }));
        let transform = paint_transform_for_box(&style, paint_space_rect(0.0, 0.0, 100.0, 50.0))
            .expect("translate makes the box transformed");

        assert_eq!(transform.e(), 40.0);
        assert_eq!(transform.f(), -20.0);
    }

    #[test]
    fn perspective_property_uses_the_rendering_clamp_and_is_projective() {
        let mut style = ComputedStyle::initial();
        style.perspective = css::ComputedPerspective::Distance(
            css::NonNegativeComputedLength::new(ComputedLengthPercentage::ZERO)
                .expect("zero is a valid computed perspective"),
        );
        let transform = perspective_property_3d_transform_for_reference_box(
            &style,
            TransformReferenceBox::css_layout(paint_space_rect(0.0, 0.0, 100.0, 50.0)),
        )
        .expect("non-none perspective creates a descendant projection");
        assert!(transform.try_into_affine_pdf_ctm().is_none());
        assert!(
            perspective_property_3d_transform_for_reference_box(
                &ComputedStyle::initial(),
                TransformReferenceBox::css_layout(paint_space_rect(0.0, 0.0, 100.0, 50.0)),
            )
            .is_none()
        );
    }

    #[test]
    fn supplied_used_border_box_is_not_reconstructed_from_paint_ink() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(300.0),
        );
        style.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(300.0),
            ),
        );
        style
            .transform
            .push(css::TransformFunction::Translate(CssTransformTranslation {
                x: ComputedLengthPercentage::from_percent(0.1),
                y: ComputedLengthPercentage::from_percent(0.1),
            }));

        let transform = paint_transform_for_reference_box(
            &style,
            TransformReferenceBox::css_layout(paint_space_rect(10.0, 20.0, 300.0, 300.0)),
        )
        .expect("translate establishes a transform");
        assert_eq!(transform.e(), 30.0);
        assert_eq!(transform.f(), -30.0);
    }

    #[test]
    fn table_wrapper_uses_its_border_box_for_content_box_transforms() {
        let mut style = ComputedStyle::initial();
        style.transform_box = css::TransformBox::ContentBox;
        style.padding.left = 10.0;
        style.padding.right = 10.0;
        style.padding.top = 5.0;
        style.padding.bottom = 5.0;
        style
            .transform
            .push(css::TransformFunction::Translate(CssTransformTranslation {
                x: ComputedLengthPercentage::from_percent(0.5),
                y: ComputedLengthPercentage::from_percent(0.5),
            }));

        let transform = paint_transform_for_reference_box(
            &style,
            TransformReferenceBox::table_wrapper(paint_space_rect(0.0, 0.0, 100.0, 50.0)),
        )
        .expect("translate establishes a transform");

        assert_eq!(transform.e(), 50.0);
        assert_eq!(transform.f(), -25.0);
    }

    #[test]
    fn affine_3d_matrix_extracts_the_pdf_paint_plane() {
        let transform = PaintTransform3D::new(
            2.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 5.0, 6.0, 7.0, 1.0,
        );
        let affine = Affine3dPaintTransform::try_from_transform(transform)
            .expect("an affine homogeneous transform projects to PDF");
        let affine = affine.flatten_to_paint_transform();

        assert_eq!(affine.a(), 2.0);
        assert_eq!(affine.d(), 3.0);
        assert_eq!(affine.e(), 5.0);
        assert_eq!(affine.f(), 6.0);
    }

    #[test]
    fn projective_and_singular_3d_transforms_do_not_paint() {
        let mut style = ComputedStyle::initial();
        style
            .transform
            .push(css::TransformFunction::Scale3D(css::CssScaleFactors3D {
                x: 1.0,
                y: 1.0,
                z: 0.0,
            }));
        let transform = affine_3d_paint_transform_for_reference_box(
            &style,
            TransformReferenceBox::css_layout(paint_space_rect(0.0, 0.0, 10.0, 10.0)),
        )
        .expect("scale3d is affine");
        assert!(!transform.is_invertible());

        let projective = PaintTransform3D::new(
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, -0.01, 0.0, 0.0, 0.0, 1.0,
        );
        assert!(Affine3dPaintTransform::try_from_transform(projective).is_none());
    }
}
