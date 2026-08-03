use super::*;
use crate::document::paint::geometry::AxisSelectivePaintClip;

pub(in crate::layout) fn paint_effects_for_element_box(
    element: &Element,
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintEffects {
    paint_effects_for_box_with_overflow_clip(
        style,
        border_box,
        used_overflow_clips_element(element, style),
    )
}

pub(in crate::layout) fn paint_effects_for_box(
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintEffects {
    paint_effects_for_box_with_overflow_clip(
        style,
        border_box,
        style_clips_overflow(style) || style.contain.paint,
    )
}

pub(in crate::layout) fn paint_effects_for_box_with_overflow_clip(
    style: &ComputedStyle,
    border_box: PaintClip,
    clips_overflow: bool,
) -> PaintEffects {
    let borders = used_border_widths(style);
    let used_overflow = UsedOverflowAxes::from_style(style);
    let clips_x = clips_overflow && (used_overflow.clips_x() || style.contain.paint);
    let clips_y = clips_overflow && (used_overflow.clips_y() || style.contain.paint);
    let padding_clip = PaintClip::from_paint_rect(paint_space_rect(
        border_box.x() + borders.left,
        border_box.y() + borders.bottom,
        border_box.width() - borders.left - borders.right,
        border_box.height() - borders.top - borders.bottom,
    ));
    let fully_bounded = clips_x && clips_y;
    let transform = paint_transform_for_box(style, border_box.paint_rect());
    let suppress_3d = transform_3d_suppresses_paint(style, border_box.paint_rect());
    PaintEffects {
        opacity: style.opacity,
        transform,
        suppress_paint: suppress_3d
            || transform.is_some_and(|transform| !transform.is_invertible()),
        overflow_clip: fully_bounded.then_some(padding_clip),
        axis_selective_overflow_clip: (clips_overflow && !fully_bounded)
            .then_some(AxisSelectivePaintClip::new(padding_clip, clips_x, clips_y)),
        overflow_clip_union: None,
        rounded_overflow_clip: fully_bounded
            .then(|| {
                rounded_clip_rect_for_box(
                    paint_space_rect(
                        border_box.x(),
                        border_box.y(),
                        border_box.width(),
                        border_box.height(),
                    ),
                    style,
                    borders,
                    css::BackgroundBox::Padding,
                )
            })
            .flatten(),
        absolute_clip: None,
        clip_path: paint_clip_path_effect(style, border_box),
        mask: paint_mask_effect(style),
        filter: paint_filter_effect(style),
        blend_mode: paint_blend_mode(style.mix_blend_mode),
        isolation: style.isolation == Isolation::Isolate || style.will_change.isolation,
    }
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
    if !matches!(style.filter, FilterValue::None) {
        PaintFilterEffect::FilterList
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
    containing_block: ContainingBlock,
) -> Vec<OverflowClip> {
    let containing_block_rect = PageTopRect::new(
        containing_block.x(),
        containing_block.top_y(),
        containing_block.width(),
        containing_block.height(),
    )
    .paint_rect();
    clips
        .iter()
        .cloned()
        .filter(|clip| paint_rect_contains(clip.paint_rect(), containing_block_rect))
        .collect()
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
