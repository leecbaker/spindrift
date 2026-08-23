use super::*;
use crate::document::paint::effects::ThreeDParticipation;
use crate::document::paint::geometry::Projective3dPaintTransform;

/// Geometry with distinct paint and CSS-transform responsibilities for one
/// principal box. Ink bounds drive stacking/culling; the used reference box
/// drives `transform`, `transform-origin`, and transform percentages.
/// <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PrincipalPaintGeometry {
    pub(in crate::layout) paint_bounds: PaintClip,
    pub(in crate::layout) transform_box: TransformReferenceBox,
}

impl PrincipalPaintGeometry {
    pub(in crate::layout) fn css_layout(border_box: PaintClip) -> Self {
        Self {
            paint_bounds: border_box,
            transform_box: TransformReferenceBox::css_layout(border_box.paint_rect()),
        }
    }

    pub(in crate::layout) fn with_transform_box(
        paint_bounds: PaintClip,
        transform_box: TransformReferenceBox,
    ) -> Self {
        Self {
            paint_bounds,
            transform_box,
        }
    }
}

pub(in crate::layout) fn paint_effects_for_element_box(
    element: &Element,
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintEffects {
    paint_effects_for_principal_box_with_overflow_clip(
        style,
        PrincipalPaintGeometry::css_layout(border_box),
        used_overflow_clips_element(element, style),
    )
}

pub(in crate::layout) fn paint_effects_for_box(
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintEffects {
    paint_effects_for_principal_box_with_overflow_clip(
        style,
        PrincipalPaintGeometry::css_layout(border_box),
        style_clips_overflow(style)
            || (property_containment_applies_to_style(style) && style.contain.paint),
    )
}

pub(in crate::layout) fn paint_effects_for_principal_box(
    style: &ComputedStyle,
    geometry: PrincipalPaintGeometry,
) -> PaintEffects {
    paint_effects_for_principal_box_with_overflow_clip(
        style,
        geometry,
        style_clips_overflow(style)
            || (property_containment_applies_to_style(style) && style.contain.paint),
    )
}

pub(in crate::layout) fn paint_effects_for_principal_box_with_overflow_clip(
    style: &ComputedStyle,
    geometry: PrincipalPaintGeometry,
    clips_overflow: bool,
) -> PaintEffects {
    let border_box = geometry.paint_bounds;
    let used_overflow = UsedOverflowAxes::from_style(style);
    let paint_containment =
        clips_overflow && (style.contain.paint || !used_overflow.clips_any_axis());
    let overflow_clip_effect = clips_overflow
        .then(|| {
            resolve_overflow_clip_edge(
                border_box.paint_rect(),
                style,
                used_border_widths(style),
                used_overflow,
                paint_containment,
                None,
            )
        })
        .flatten()
        .map(|edge| edge.effect());
    let transform_style = used_transform_style(style);
    let is_transparent_3d_bridge = style.anonymous_3d_layout_bridge;
    let has_3d_transform = transform_list_contains_3d(&style.transform);
    // A preserving rendering context retains every affine transform in
    // homogeneous space. This includes a syntactically 2D transform, which
    // is a 3D matrix with z left unchanged.
    let retains_3d_transform =
        has_3d_transform || transform_style == css::TransformStyle::Preserve3d;
    let projective_3d_transform = if !retains_3d_transform {
        None
    } else if style.has_transform() {
        projective_3d_paint_transform_for_reference_box(style, geometry.transform_box)
    } else {
        // `transform-style: preserve-3d` itself establishes a shared 3D
        // rendering context. Retain an explicit identity matrix so a child
        // plane is not mistaken for ordinary flattened paint.
        Some(Projective3dPaintTransform::identity())
    };
    let affine_3d_transform =
        projective_3d_transform.and_then(Projective3dPaintTransform::try_into_affine_pdf_ctm);
    // A preserving element cannot flatten its local 3D transform before its
    // descendants have joined the rendering context. Flat elements retain the
    // existing PDF-ready projection.
    let transform = (!retains_3d_transform || transform_style == css::TransformStyle::Flat)
        .then(|| paint_transform_for_reference_box(style, geometry.transform_box))
        .flatten();
    let suppress_3d = has_3d_transform && projective_3d_transform.is_none();
    PaintEffects {
        opacity: style.opacity.value(),
        transform,
        affine_3d_transform,
        projective_3d_transform: projective_3d_transform.filter(|_| affine_3d_transform.is_none()),
        descendant_projective_3d_transform: perspective_property_3d_transform_for_reference_box(
            style,
            geometry.transform_box,
        ),
        three_d_participation: if is_transparent_3d_bridge {
            ThreeDParticipation::TransparentLayoutBridge
        } else if transform_style == css::TransformStyle::Preserve3d {
            ThreeDParticipation::Preserve3d
        } else {
            ThreeDParticipation::Flat
        },
        hide_backface: style.backface_visibility == css::BackfaceVisibility::Hidden,
        suppress_paint: suppress_3d
            || (transform_style == css::TransformStyle::Flat
                && transform.is_some_and(|transform| !transform.is_invertible())),
        overflow_clip_effect,
        absolute_clip: legacy_absolute_clip(style, border_box),
        scene_plane_clip: None,
        clip_path: paint_clip_path_effect(style, border_box),
        mask: paint_mask_effect(style),
        filter: paint_filter_effect(style),
        blend_mode: paint_blend_mode(style.mix_blend_mode),
        isolation: style.isolation == Isolation::Isolate || style.will_change.isolation,
    }
}

/// Resolve the CSS Transforms used value of `transform-style`.
///
/// Grouping properties require descendants to be flattened before their
/// effects can apply, so they force `preserve-3d` to `flat`.
/// <https://drafts.csswg.org/css-transforms-2/#grouping-property-values>
pub(in crate::layout) fn used_transform_style(style: &ComputedStyle) -> css::TransformStyle {
    if style.transform_style != css::TransformStyle::Preserve3d {
        return css::TransformStyle::Flat;
    }
    let overflow_groups = !matches!(
        style.overflow_x,
        css::Overflow::Visible | css::Overflow::Clip
    ) || !matches!(
        style.overflow_y,
        css::Overflow::Visible | css::Overflow::Clip
    );
    let groups = overflow_groups
        || style.opacity.value() < 1.0
        || !matches!(style.filter, css::FilterValue::None)
        || style.legacy_clip.forces_flattening()
        || style.clip_path != css::ClipPath::None
        || !matches!(style.mask, css::MaskValue::None)
        || !matches!(style.mask_border_source, css::ComputedImage::None)
        || style.isolation == css::Isolation::Isolate
        || style.mix_blend_mode != css::MixBlendMode::Normal
        || style.contain.paint
        // Both `hidden` and `auto` turn on paint containment in the used
        // `contain` value. `auto` retains that containment even while the
        // element is relevant and its contents are painted.
        // <https://drafts.csswg.org/css-contain-2/#content-visibility>
        || matches!(
            style.content_visibility,
            css::ContentVisibility::Auto | css::ContentVisibility::Hidden
        );
    if groups {
        css::TransformStyle::Flat
    } else {
        css::TransformStyle::Preserve3d
    }
}

/// Resolve legacy CSS 2 `clip: rect()` against the painted element's used
/// border box. The property applies only to absolute/fixed positioned boxes;
/// each edge is measured from the physical top or left border edge and an
/// `auto` edge coincides with the matching border-box edge.
/// <https://drafts.csswg.org/css2/#propdef-clip>
fn legacy_absolute_clip(style: &ComputedStyle, border_box: PaintClip) -> Option<PaintClip> {
    if !matches!(
        style.position,
        css::Position::Absolute | css::Position::Fixed
    ) {
        return None;
    }
    let css::LegacyClip::Rect([top, right, bottom, left]) = &style.legacy_clip else {
        return None;
    };
    let edge = |edge: &css::LegacyClipEdge, fallback: f32| match edge {
        css::LegacyClipEdge::Auto => fallback,
        css::LegacyClipEdge::Length(length) => length.length_points(),
    };
    let top = edge(top, 0.0);
    let right = edge(right, border_box.width());
    let bottom = edge(bottom, border_box.height());
    let left = edge(left, 0.0);
    let x = border_box.x() + left;
    let y = border_box.y() + (border_box.height() - bottom);
    Some(PaintClip::new(
        x,
        y,
        (right - left).max(0.0),
        (bottom - top).max(0.0),
    ))
}

pub(in crate::layout) fn paint_clip_path_effect(
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintClipPathEffect {
    match &style.clip_path {
        ClipPath::None if style.will_change.clip_path => PaintClipPathEffect::WillChange,
        ClipPath::None => PaintClipPathEffect::None,
        ClipPath::Polygon(points) => {
            let border_box = border_box.paint_rect();
            let resolve = |value: &css::ComputedLengthPercentage, basis: f32| {
                value
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
                    .map(layout_points)
                    .unwrap_or_else(|| value.length_points())
            };
            let points = points
                .iter()
                .map(|point| {
                    PaintPoint::new(
                        border_box.min_x() + resolve(&point.x, border_box.width()),
                        // CSS basic-shape coordinates use the geometry box's
                        // top-left origin, while page paint coordinates use a
                        // bottom-left origin.
                        border_box.max_y() - resolve(&point.y, border_box.height()),
                    )
                })
                .collect::<Vec<_>>();
            RenderedClipPathPolygon::new(&points)
                .map(|polygon| PaintClipPathEffect::Polygon(Box::new(polygon)))
                .unwrap_or(PaintClipPathEffect::Shape)
        }
        ClipPath::Inset {
            top,
            right,
            bottom,
            left,
        } => {
            let border_box = border_box.paint_rect();
            let resolve = |value: &css::ComputedLengthPercentage, basis: f32| {
                value
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
                    .map(layout_points)
                    .unwrap_or_else(|| value.length_points())
            };
            let left = resolve(left, border_box.width());
            let right = resolve(right, border_box.width());
            let top = resolve(top, border_box.height());
            let bottom = resolve(bottom, border_box.height());
            let min_x = border_box.min_x() + left;
            let max_x = border_box.max_x() - right;
            let min_y = border_box.min_y() + bottom;
            let max_y = border_box.max_y() - top;
            RenderedClipPathPolygon::new(&[
                PaintPoint::new(min_x, min_y),
                PaintPoint::new(max_x, min_y),
                PaintPoint::new(max_x, max_y),
                PaintPoint::new(min_x, max_y),
            ])
            .map(|polygon| PaintClipPathEffect::Polygon(Box::new(polygon)))
            .unwrap_or(PaintClipPathEffect::Shape)
        }
        ClipPath::Shape => PaintClipPathEffect::Shape,
        ClipPath::Url => PaintClipPathEffect::Url,
    }
}

pub(in crate::layout) fn paint_mask_effect(style: &ComputedStyle) -> PaintMaskEffect {
    if !matches!(style.mask, MaskValue::None) {
        PaintMaskEffect::MaskImage
    } else if style.will_change.mask {
        PaintMaskEffect::WillChange
    } else {
        PaintMaskEffect::None
    }
}

pub(in crate::layout) fn paint_filter_effect(style: &ComputedStyle) -> PaintFilterEffect {
    if let Some(lowering) = style.filter.exact_lowering() {
        PaintFilterEffect::Exact(lowering)
    } else if !matches!(style.filter, FilterValue::None) {
        PaintFilterEffect::RequiresRasterBackend
    } else if style.will_change.filter {
        PaintFilterEffect::WillChange
    } else {
        PaintFilterEffect::None
    }
}

pub(in crate::layout) fn paint_blend_mode(mode: MixBlendMode) -> PaintBlendMode {
    match mode {
        MixBlendMode::Normal => PaintBlendMode::Normal,
        MixBlendMode::Multiply => PaintBlendMode::Multiply,
        MixBlendMode::Screen => PaintBlendMode::Screen,
        MixBlendMode::Overlay => PaintBlendMode::Overlay,
        MixBlendMode::Darken => PaintBlendMode::Darken,
        MixBlendMode::Lighten => PaintBlendMode::Lighten,
        MixBlendMode::ColorDodge => PaintBlendMode::ColorDodge,
        MixBlendMode::ColorBurn => PaintBlendMode::ColorBurn,
        MixBlendMode::HardLight => PaintBlendMode::HardLight,
        MixBlendMode::SoftLight => PaintBlendMode::SoftLight,
        MixBlendMode::Difference => PaintBlendMode::Difference,
        MixBlendMode::Exclusion => PaintBlendMode::Exclusion,
        MixBlendMode::Hue => PaintBlendMode::Hue,
        MixBlendMode::Saturation => PaintBlendMode::Saturation,
        MixBlendMode::Color => PaintBlendMode::Color,
        MixBlendMode::Luminosity => PaintBlendMode::Luminosity,
    }
}

pub(in crate::layout) fn positioned_applicable_overflow_clips(
    clips: &[OverflowClip],
    _containing_block: ContainingBlock,
) -> Vec<OverflowClip> {
    // The overflow-clip chain is an ancestry relation, not a containment
    // test between an ancestor clip and the positioned containing block. An
    // absolute containing block can straddle an ancestor's clip edge while
    // every one of its descendant paint fragments remains clipped by that
    // ancestor. Dropping that clip would turn a provably unreachable tail
    // into unbounded scratch pagination.
    // <https://drafts.csswg.org/css-overflow-3/#overflow-clip-edge>
    // <https://drafts.csswg.org/css-position-3/#abspos-containing-block>
    clips.to_vec()
}

pub(in crate::layout) fn paint_rect_contains(outer: PaintRect, inner: PaintRect) -> bool {
    const EPSILON: f32 = 0.01;
    let outer_left = outer.origin.x;
    let outer_right = outer.origin.x + outer.size.width;
    let outer_bottom = outer.origin.y;
    let outer_top = outer.origin.y + outer.size.height;
    let inner_left = inner.origin.x;
    let inner_right = inner.origin.x + inner.size.width;
    let inner_bottom = inner.origin.y;
    let inner_top = inner.origin.y + inner.size.height;
    outer_left <= inner_left + EPSILON
        && outer_right + EPSILON >= inner_right
        && outer_bottom <= inner_bottom + EPSILON
        && outer_top + EPSILON >= inner_top
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{ComputedLengthPercentage, CssTransformTranslation};
    use crate::document::paint::geometry::Affine3dPaintTransform;

    #[test]
    fn principal_effect_uses_transform_box_not_paint_bounds() {
        let mut style = ComputedStyle::initial();
        style
            .transform
            .push(css::TransformFunction::Translate(CssTransformTranslation {
                x: ComputedLengthPercentage::from_percent(0.5),
                y: ComputedLengthPercentage::from_percent(0.5),
            }));
        let effects = paint_effects_for_principal_box(
            &style,
            PrincipalPaintGeometry::with_transform_box(
                PaintClip::from_paint_rect(paint_space_rect(10.0, 20.0, 1.0, 1.0)),
                TransformReferenceBox::css_layout(paint_space_rect(10.0, 20.0, 100.0, 50.0)),
            ),
        );
        let transform = effects
            .transform
            .expect("translate establishes a paint transform");

        assert_eq!(transform.e(), 50.0);
        assert_eq!(transform.f(), -25.0);
    }

    #[test]
    fn preserve_3d_without_transform_establishes_an_identity_3d_context() {
        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;

        let effects = paint_effects_for_box(&style, PaintClip::new(0.0, 0.0, 10.0, 10.0));

        assert_eq!(
            effects.three_d_participation,
            ThreeDParticipation::Preserve3d
        );
        assert_eq!(
            effects.affine_3d_transform,
            Some(Affine3dPaintTransform::identity())
        );
        assert_eq!(effects.transform, None);
    }

    #[test]
    fn every_normative_grouping_property_flattens_preserve_3d_at_used_value_time() {
        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        assert_eq!(
            used_transform_style(&style),
            css::TransformStyle::Preserve3d
        );

        style.opacity = css::Opacity::new_clamped(0.5).expect("finite opacity");
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.overflow_x = css::Overflow::Hidden;
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        style.overflow_x = css::Overflow::Auto;
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        style.overflow_x = css::Overflow::Clip;
        assert_eq!(
            used_transform_style(&style),
            css::TransformStyle::Preserve3d,
            "overflow: clip is explicitly excluded from the grouping values"
        );

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.filter =
            css::FilterValue::Functions(vec![css::FilterFunction::RequiresRasterBackend(
                "blur(0)".to_owned(),
            )]);
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.legacy_clip = css::LegacyClip::Rect([
            css::LegacyClipEdge::Auto,
            css::LegacyClipEdge::Auto,
            css::LegacyClipEdge::Auto,
            css::LegacyClipEdge::Auto,
        ]);
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.clip_path = css::ClipPath::Shape;
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.isolation = css::Isolation::Isolate;
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.mask = css::MaskValue::Image("url(mask.svg)".to_owned());
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.mask_border_source = css::ComputedImage::Invalid;
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.mix_blend_mode = css::MixBlendMode::Multiply;
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.contain.paint = true;
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.content_visibility = css::ContentVisibility::Auto;
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);

        style.content_visibility = css::ContentVisibility::Hidden;
        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);
    }

    #[test]
    fn legacy_clip_flattens_and_clips_an_absolute_border_box() {
        let mut style = ComputedStyle::initial();
        style.transform_style = css::TransformStyle::Preserve3d;
        style.position = css::Position::Absolute;
        style.legacy_clip = css::LegacyClip::Rect([
            css::LegacyClipEdge::Length(ComputedLengthPercentage::from_points(5.0)),
            css::LegacyClipEdge::Length(ComputedLengthPercentage::from_points(40.0)),
            css::LegacyClipEdge::Length(ComputedLengthPercentage::from_points(35.0)),
            css::LegacyClipEdge::Length(ComputedLengthPercentage::from_points(10.0)),
        ]);

        let effects = paint_effects_for_box(&style, PaintClip::new(100.0, 200.0, 50.0, 40.0));

        assert_eq!(used_transform_style(&style), css::TransformStyle::Flat);
        assert_eq!(
            effects.absolute_clip,
            Some(PaintClip::new(110.0, 205.0, 30.0, 30.0))
        );
    }
}
