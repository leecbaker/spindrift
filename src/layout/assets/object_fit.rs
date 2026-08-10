use super::*;
use crate::css::ObjectFit;
use crate::layout::asset_helpers::{
    CssImageNaturalDimensions, ResolvedObjectViewBox, resolved_object_view_box,
    resolved_object_view_box_for_svg,
};
use crate::svg::{SharedSvgAsset, SvgSourcePoint, SvgSourceRect, SvgSourceSize};
use crate::units::LayoutSize;

fn rectangular_object_view_box_clip(rect: PaintRect) -> RenderedPathClip {
    RenderedPathClip::new(
        vec![
            RenderedPathCommand::move_to(rect.origin),
            RenderedPathCommand::line_to(PaintPoint::new(rect.max_x(), rect.min_y())),
            RenderedPathCommand::line_to(PaintPoint::new(rect.max_x(), rect.max_y())),
            RenderedPathCommand::line_to(PaintPoint::new(rect.min_x(), rect.max_y())),
            RenderedPathCommand::Close,
        ],
        RenderedPathFillRule::NonZero,
        Vec::new(),
    )
}

fn object_view_box_clip(
    view_box: &ResolvedObjectViewBox,
    natural_size: LayoutSize,
    geometry: ConcreteObjectGeometry,
) -> Option<RenderedPathClip> {
    // Every concrete object is clipped to the visible portion of its content
    // box. This is independent of `object-view-box`: an ineffective view box
    // leaves source selection alone, but must not remove the object-fit clip.
    // Keeping this as an image-local clip also gives the raster resource the
    // same edge coverage as an equivalent clipped background image.
    if !view_box.applies() {
        return Some(rectangular_object_view_box_clip(geometry.visible));
    }
    let source = view_box.source_rect();
    let Some(radii) = view_box.radii().filter(|radii| !(*radii).clone().is_zero()) else {
        return Some(rectangular_object_view_box_clip(geometry.visible));
    };
    let source_width = natural_size.width * source.size.width;
    let source_height = natural_size.height * source.size.height;
    let source_radii =
        used_rounded_rect_radii(radii.clone(), LayoutSize::new(source_width, source_height));
    let scale_x = geometry.concrete.size.width / source_width;
    let scale_y = geometry.concrete.size.height / source_height;
    let scale_corner = |corner: RenderedCornerRadius| {
        RenderedCornerRadius::new(corner.x() * scale_x, corner.y() * scale_y)
    };
    let destination_radii = RenderedRoundedRectRadii {
        top_left: scale_corner(source_radii.top_left),
        top_right: scale_corner(source_radii.top_right),
        bottom_right: scale_corner(source_radii.bottom_right),
        bottom_left: scale_corner(source_radii.bottom_left),
    };
    let mut clip = RenderedPathClip::new(
        shaped_rect_path_commands(
            geometry.concrete,
            destination_radii,
            css::CornerShapes::ROUND,
        ),
        RenderedPathFillRule::NonZero,
        Vec::new(),
    );
    let rectangular = rectangular_object_view_box_clip(geometry.visible);
    clip.additional_clips.push(RenderedPathClipPath::new(
        rectangular.commands,
        rectangular.fill_rule,
    ));
    Some(clip)
}

/// Resolve the concrete object size and position for a raster replaced element.
///
/// The concrete object is positioned in the element's content box and, when it
/// overflows, cropped through the shared raster paint-area operation. This
/// keeps `object-fit` and `background-size` on one source-to-destination
/// mapping model, including the PDF image resource's pixel coordinate system.
/// <https://www.w3.org/TR/css-images-3/#the-object-fit>
pub(in crate::layout) fn apply_object_fit(
    image: &mut RenderedImage,
    object_fit: ObjectFit,
    object_position: css::BackgroundPosition,
    object_view_box: css::ObjectViewBox,
) -> bool {
    if image.width() <= 0.0 || image.height() <= 0.0 {
        return false;
    }
    let source_width = image.pixel_width();
    let source_height = image.pixel_height();
    if source_width == 0 || source_height == 0 {
        return false;
    }
    let natural_size = LayoutSize::new(
        source_width as f32 * css::CSS_PX_TO_PT,
        source_height as f32 * css::CSS_PX_TO_PT,
    );
    let view_box = resolved_object_view_box(object_view_box, Some(natural_size));
    let source = view_box.source_rect();
    let Some(geometry) = concrete_object_geometry(
        image.paint_rect(),
        CssImageNaturalDimensions::from_layout_size(natural_size)
            .scaled(source.size.width, source.size.height),
        object_fit,
        object_position,
    ) else {
        return false;
    };
    let full_width = geometry.concrete.size.width / source.size.width;
    let full_height = geometry.concrete.size.height / source.size.height;
    image.set_paint_rect(paint_space_rect(
        geometry.concrete.origin.x - source.origin.x * full_width,
        geometry.concrete.origin.y - (1.0 - source.max_y()) * full_height,
        full_width,
        full_height,
    ));
    if let Some(clip) = object_view_box_clip(&view_box, natural_size, geometry) {
        // `fill` maps the complete source directly to the destination. Its
        // visible-area rectangle is therefore already the paint primitive's
        // own boundary. Appending it to an existing shaped content contour
        // creates a second raster edge in the same graphics state without
        // changing the CSS intersection.
        if !view_box.applies() && matches!(object_fit, ObjectFit::Fill) {
            if image.clip().is_none() {
                *image = image.clone().with_destination_rect_clip(clip);
            }
            return true;
        }
        // `object-view-box` crops the source image, but it must not discard
        // an enclosing CSS clip such as `border-shape`. PDF applies multiple
        // clipping paths as their intersection in one graphics state.
        // <https://drafts.csswg.org/css-images-4/#object-view-box>
        let clip = if let Some(existing) = image.clip().cloned() {
            let mut combined = existing;
            combined
                .additional_clips
                .push(RenderedPathClipPath::new(clip.commands, clip.fill_rule));
            combined.additional_clips.extend(clip.additional_clips);
            combined
        } else {
            clip
        };
        *image = image.clone().with_clip(clip);
    }
    true
}

/// The concrete object and visible intersection for a replaced image.
///
/// CSS Images defines `object-fit` as concrete-object sizing followed by
/// `object-position` alignment in the element's content box. Keeping this
/// source-independent geometry lets raster and vector image emitters apply
/// the same sizing and clipping semantics.
/// <https://www.w3.org/TR/css-images-3/#the-object-fit>
#[derive(Clone, Copy)]
struct ConcreteObjectGeometry {
    concrete: crate::document::paint::geometry::PaintRect,
    visible: crate::document::paint::geometry::PaintRect,
}

fn concrete_object_geometry(
    destination: crate::document::paint::geometry::PaintRect,
    natural_dimensions: CssImageNaturalDimensions,
    object_fit: ObjectFit,
    object_position: css::BackgroundPosition,
) -> Option<ConcreteObjectGeometry> {
    if destination.size.width <= 0.0 || destination.size.height <= 0.0 {
        return None;
    }
    let none_size = || natural_dimensions.default_size(destination.size);
    let contain_size = || natural_dimensions.contain_size(destination.size);
    let concrete_size = match object_fit {
        ObjectFit::Fill => destination.size,
        ObjectFit::Contain => contain_size(),
        ObjectFit::Cover => natural_dimensions.cover_size(destination.size),
        ObjectFit::None => none_size(),
        ObjectFit::ScaleDown => {
            let none = none_size();
            let contain = contain_size();
            if none.width <= contain.width && none.height <= contain.height {
                none
            } else {
                contain
            }
        }
    };
    if concrete_size.width <= 0.0
        || concrete_size.height <= 0.0
        || !concrete_size.width.is_finite()
        || !concrete_size.height.is_finite()
    {
        return None;
    }
    let offset_x = used_background_position_axis(
        object_position.x,
        destination.size.width - concrete_size.width,
        false,
    );
    let offset_y = used_background_position_axis(
        object_position.y,
        destination.size.height - concrete_size.height,
        true,
    );
    let concrete = paint_space_rect(
        destination.origin.x + offset_x,
        destination.origin.y + offset_y,
        concrete_size.width,
        concrete_size.height,
    );
    let visible = concrete.intersection(&destination)?;
    Some(ConcreteObjectGeometry { concrete, visible })
}

/// Translate concrete-object geometry into an SVG viewport source rectangle.
///
/// SVG source coordinates start at the top, while paint rectangles start at
/// the bottom. The source Y conversion therefore inverts the visible
/// intersection within the concrete object.
pub(in crate::layout) fn svg_replaced_group(
    asset: &SharedSvgAsset,
    destination: PaintRect,
    object_fit: ObjectFit,
    object_position: css::BackgroundPosition,
    object_view_box: css::ObjectViewBox,
) -> crate::svg::SvgPaintGroup {
    svg_replaced_group_with_viewport_clip(
        asset,
        destination,
        object_fit,
        object_position,
        object_view_box,
        true,
    )
}

/// Paint an SVG replaced object while preserving the root viewport's CSS
/// overflow policy.
///
/// CSS Images consumers always select a finite source rectangle and therefore
/// use [`svg_replaced_group`]'s clipping behavior. An embedded SVG root is a
/// CSS box in its own right: `overflow: visible` lets its SVG descendants
/// extend beyond that viewport.
/// <https://www.w3.org/TR/SVG2/render.html#OverflowAndClipProperties>
pub(in crate::layout) fn svg_replaced_group_with_viewport_clip(
    asset: &SharedSvgAsset,
    destination: PaintRect,
    object_fit: ObjectFit,
    object_position: css::BackgroundPosition,
    object_view_box: css::ObjectViewBox,
    clip_viewport: bool,
) -> crate::svg::SvgPaintGroup {
    let natural_size = asset.replaced_intrinsic_size();
    let view_box = resolved_object_view_box_for_svg(object_view_box, asset);
    let source_view_box = view_box.source_rect();
    let intrinsic = asset.intrinsic_dimensions();
    let natural_dimensions = CssImageNaturalDimensions::from_layout_axes(
        intrinsic.width,
        intrinsic.height,
        intrinsic.aspect_ratio,
    )
    .scaled(source_view_box.size.width, source_view_box.size.height);
    let Some(geometry) =
        concrete_object_geometry(destination, natural_dimensions, object_fit, object_position)
    else {
        return crate::svg::SvgPaintGroup::empty();
    };
    // The SVG root still has its complete source viewport.  The view box only
    // changes CSS Images' effective natural size, so scale that full viewport
    // to the concrete object before selecting the requested source rectangle.
    let viewport_asset = asset.with_replaced_viewport(content_box_size_pt(
        geometry.concrete.size.width / source_view_box.size.width,
        geometry.concrete.size.height / source_view_box.size.height,
    ));
    let source_size = viewport_asset.source_viewport_size();
    let left =
        (geometry.visible.min_x() - geometry.concrete.min_x()) / geometry.concrete.size.width;
    let bottom =
        (geometry.visible.min_y() - geometry.concrete.min_y()) / geometry.concrete.size.height;
    let visible_width = geometry.visible.size.width / geometry.concrete.size.width;
    let visible_height = geometry.visible.size.height / geometry.concrete.size.height;
    let source = SvgSourceRect::new(
        SvgSourcePoint::new(
            source_size.width * (source_view_box.origin.x + source_view_box.size.width * left),
            source_size.height
                * (source_view_box.origin.y
                    + source_view_box.size.height * (1.0 - bottom - visible_height)),
        ),
        SvgSourceSize::new(
            source_size.width * source_view_box.size.width * visible_width,
            source_size.height * source_view_box.size.height * visible_height,
        ),
    );
    let group = viewport_asset.paint_group_for_source_rect_with_viewport_clip(
        geometry.visible,
        source,
        clip_viewport,
    );
    // `paint_group_for_source_rect` already clips the SVG root viewport to
    // `geometry.visible`. Re-applying that same rectangular clip is
    // redundant and can introduce an additional antialiased edge. An effective
    // `object-view-box` is different: it can add a source-crop contour (and
    // rounded corners) that the SVG viewport does not express.
    // <https://www.w3.org/TR/css-images-3/#the-object-fit>
    // <https://drafts.csswg.org/css-images-4/#object-view-box>
    if view_box.applies() {
        group.with_clip(
            object_view_box_clip(&view_box, natural_size, geometry)
                .expect("an effective object-view-box has a destination clip"),
        )
    } else {
        group
    }
}

/// Resolve the CSS Borders content edge for an atomic replaced primitive.
///
/// This is intentionally independent of `overflow`: CSS Borders requires a
/// replaced element's content to follow the curved/shaped content edge even
/// when ordinary descendant overflow remains visible.
pub(in crate::layout) fn replaced_content_contour(
    border_rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Option<ResolvedBoxContentClip> {
    if style.border_radius.clone().is_zero() && matches!(style.border_shape, css::BorderShape::None)
    {
        return None;
    }
    resolve_box_content_contour(
        border_rect,
        style,
        border_insets,
        BoxContentContourRequest::ReplacedContent,
    )
}

/// Emit tiled vector paths for one CSS border-image slice.
///
/// This shares the same segment resolution as raster `border-image`, but maps
/// each selected SVG root-viewport rectangle directly to the tile's
/// destination rectangle.
pub(in crate::layout) fn push_svg_border_image_tiles(
    primitives: &mut Vec<PaintPrimitive>,
    asset: &SharedSvgAsset,
    destination: RenderedImageTileRect,
    source: RenderedImageSourceRect,
    repeat_x: css::BorderImageRepeatKeyword,
    repeat_y: css::BorderImageRepeatKeyword,
) {
    let tile_size = border_image_base_tile_size(destination, source, repeat_x, repeat_y);
    let x_segments =
        border_image_tile_segments(repeat_x, destination.width(), tile_size.width, source.width);
    let y_segments = border_image_tile_segments(
        repeat_y,
        destination.height(),
        tile_size.height,
        source.height,
    );
    for y_segment in &y_segments {
        for x_segment in &x_segments {
            if x_segment.destination_size <= 0.0
                || y_segment.destination_size <= 0.0
                || x_segment.source_size == 0
                || y_segment.source_size == 0
            {
                continue;
            }
            let source = SvgSourceRect::new(
                SvgSourcePoint::new(
                    (source.x + x_segment.source_offset) as f32,
                    (source.y + y_segment.source_offset) as f32,
                ),
                SvgSourceSize::new(x_segment.source_size as f32, y_segment.source_size as f32),
            );
            primitives.extend(
                asset
                    .paint_paths_for_source_rect(
                        paint_space_rect(
                            destination.x() + x_segment.destination_offset,
                            destination.y() + y_segment.destination_offset,
                            x_segment.destination_size,
                            y_segment.destination_size,
                        ),
                        source,
                    )
                    .into_iter()
                    .map(PaintPrimitive::Path),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{ComputedLengthPercentage, ObjectViewBox};
    use std::rc::Rc;

    fn first_svg_path(group: &crate::svg::SvgPaintGroup) -> Option<&RenderedPath> {
        group.items.iter().find_map(|item| match item {
            crate::svg::SvgPaintItem::Path(path) => Some(path.as_ref()),
            crate::svg::SvgPaintItem::Group(group) => first_svg_path(group),
        })
    }

    #[test]
    fn cover_geometry_preserves_the_typed_destination_paint_rect() {
        let destination: PaintRect = paint_space_rect(10.0, 20.0, 100.0, 100.0);
        let geometry = concrete_object_geometry(
            destination,
            CssImageNaturalDimensions::from_layout_size(LayoutSize::new(200.0, 100.0)),
            ObjectFit::Cover,
            css::BackgroundPosition::INITIAL,
        )
        .expect("positive replaced geometry should be paintable");

        assert_eq!(
            geometry.concrete,
            paint_space_rect(10.0, 20.0, 200.0, 100.0)
        );
        assert_eq!(geometry.visible, destination);
    }

    #[test]
    fn object_view_box_uses_its_own_top_left_source_space() {
        let natural = LayoutSize::new(200.0, 100.0);
        let view_box = css::ObjectViewBox::Xywh {
            x: ComputedLengthPercentage::from_points(20.0),
            y: ComputedLengthPercentage::from_points(10.0),
            width: ComputedLengthPercentage::from_points(100.0),
            height: ComputedLengthPercentage::from_points(50.0),
            radii: None,
        };

        let resolved = resolved_object_view_box(view_box, Some(natural));
        assert!(
            resolved.applies(),
            "positive object-view-box source geometry"
        );
        let resolved = resolved.source_rect();

        assert_eq!(resolved.origin.x, 0.1);
        assert_eq!(resolved.origin.y, 0.1);
        assert_eq!(resolved.size.width, 0.5);
        assert_eq!(resolved.size.height, 0.5);
    }

    #[test]
    fn object_view_box_ignores_empty_source_geometry() {
        let view_box = css::ObjectViewBox::Inset {
            top: ComputedLengthPercentage::from_points(50.0),
            right: ComputedLengthPercentage::ZERO,
            bottom: ComputedLengthPercentage::from_points(50.0),
            left: ComputedLengthPercentage::ZERO,
            radii: None,
        };

        let resolved = resolved_object_view_box(view_box, Some(LayoutSize::new(100.0, 100.0)));
        assert!(!resolved.applies());
        assert_eq!(resolved.source_rect().size.width, 1.0);
        assert_eq!(resolved.source_rect().size.height, 1.0);
    }

    #[test]
    fn ineffective_object_view_box_keeps_the_concrete_object_clip() {
        let destination = paint_space_rect(10.0, 20.0, 80.0, 40.0);
        let geometry = concrete_object_geometry(
            destination,
            CssImageNaturalDimensions::from_layout_size(LayoutSize::new(100.0, 100.0)),
            ObjectFit::Cover,
            css::BackgroundPosition::INITIAL,
        )
        .expect("positive replaced geometry should be paintable");
        let no_effect = ResolvedObjectViewBox::NoEffect;

        let clip = object_view_box_clip(&no_effect, LayoutSize::new(100.0, 100.0), geometry)
            .expect("the concrete object must remain clipped");

        assert_eq!(
            clip.commands,
            rectangular_object_view_box_clip(destination).commands
        );
    }

    #[test]
    fn uncropped_object_fit_marks_its_destination_clip_semantically() {
        let destination = paint_space_rect(10.0, 20.0, 80.0, 40.0);
        let mut image = RenderedImage::from_paint_rect(
            destination,
            false,
            107,
            53,
            None,
            false,
            vec![0; 107 * 53 * 3].into(),
            None,
            None,
        );

        assert!(apply_object_fit(
            &mut image,
            ObjectFit::Fill,
            css::BackgroundPosition::INITIAL,
            css::ObjectViewBox::NONE,
        ));
        assert!(image.has_destination_rect_clip());
    }

    #[test]
    fn cropped_object_fit_never_marks_its_clip_as_a_destination_clip() {
        let mut image = RenderedImage::from_paint_rect(
            paint_space_rect(10.0, 20.0, 80.0, 80.0),
            false,
            200,
            100,
            None,
            true,
            vec![0; 200 * 100 * 3].into(),
            None,
            None,
        );

        assert!(apply_object_fit(
            &mut image,
            ObjectFit::Cover,
            css::BackgroundPosition::INITIAL,
            ObjectViewBox::NONE,
        ));
        assert!(image.clip().is_some());
        assert!(!image.has_destination_rect_clip());
    }

    #[test]
    fn raster_object_view_box_maps_the_full_source_then_clips_the_crop() {
        let destination = paint_space_rect(0.0, 0.0, 100.0, 100.0);
        let mut image = RenderedImage::from_paint_rect(
            destination,
            false,
            200,
            100,
            None,
            true,
            vec![0; 200 * 100 * 3].into(),
            None,
            None,
        );
        // CSS dimensions are points, while this raster's 200×100 source is
        // 150×75pt at 96dpi. Select its central quarter in source space.
        let view_box = css::ObjectViewBox::Xywh {
            x: ComputedLengthPercentage::from_points(37.5),
            y: ComputedLengthPercentage::from_points(18.75),
            width: ComputedLengthPercentage::from_points(75.0),
            height: ComputedLengthPercentage::from_points(37.5),
            radii: None,
        };

        assert!(apply_object_fit(
            &mut image,
            ObjectFit::Fill,
            css::BackgroundPosition::INITIAL,
            view_box,
        ));
        assert_eq!(
            image.paint_rect(),
            paint_space_rect(-50.0, -50.0, 200.0, 200.0)
        );
        let clip = image
            .clip()
            .expect("object-view-box installs a destination clip");
        assert_eq!(
            clip.commands,
            rectangular_object_view_box_clip(destination).commands
        );
    }

    #[test]
    fn raster_object_fit_intersects_an_existing_css_content_clip() {
        let destination = paint_space_rect(0.0, 0.0, 100.0, 100.0);
        let border_shape_clip =
            rectangular_object_view_box_clip(paint_space_rect(10.0, 10.0, 80.0, 80.0));
        let mut image = RenderedImage::from_paint_rect(
            destination,
            false,
            200,
            100,
            None,
            true,
            vec![0; 200 * 100 * 3].into(),
            None,
            None,
        )
        .with_clip(border_shape_clip.clone());
        let view_box = css::ObjectViewBox::Xywh {
            x: ComputedLengthPercentage::from_points(37.5),
            y: ComputedLengthPercentage::from_points(18.75),
            width: ComputedLengthPercentage::from_points(75.0),
            height: ComputedLengthPercentage::from_points(37.5),
            radii: None,
        };

        assert!(apply_object_fit(
            &mut image,
            ObjectFit::Fill,
            css::BackgroundPosition::INITIAL,
            view_box,
        ));
        let clip = image.clip().expect("both clips remain attached");
        assert_eq!(clip.commands, border_shape_clip.commands);
        assert_eq!(clip.additional_clips.len(), 1);
        assert_eq!(
            clip.additional_clips[0].commands,
            rectangular_object_view_box_clip(destination).commands
        );
    }

    #[test]
    fn rounded_object_view_box_uses_a_destination_corner_clip() {
        let view_box = crate::css::parse_object_view_box(
            "xywh(10pt 10pt 80pt 80pt round 12pt)",
            crate::css::ROOT_FONT_SIZE_PT,
        )
        .expect("valid rounded basic shape");
        let natural = LayoutSize::new(100.0, 100.0);
        let source = resolved_object_view_box(view_box, Some(natural));
        let geometry = concrete_object_geometry(
            paint_space_rect(10.0, 20.0, 80.0, 80.0),
            CssImageNaturalDimensions::from_layout_size(LayoutSize::new(80.0, 80.0)),
            ObjectFit::Fill,
            css::BackgroundPosition::INITIAL,
        )
        .unwrap();

        let clip = object_view_box_clip(&source, natural, geometry)
            .expect("applied object-view-box creates a clip");

        assert!(
            clip.commands.len() > 5,
            "rounded clip contains curve commands"
        );
        assert_eq!(clip.additional_clips.len(), 1);
    }

    #[test]
    fn ratio_only_object_fit_none_uses_the_content_box_as_its_default_size() {
        let destination = paint_space_rect(10.0, 20.0, 100.0, 100.0);
        let natural = CssImageNaturalDimensions::from_layout_axes(None, None, Some(2.0));

        let geometry = concrete_object_geometry(
            destination,
            natural,
            ObjectFit::None,
            css::BackgroundPosition::INITIAL,
        )
        .expect("ratio-only image has a concrete object size");

        assert_eq!(geometry.concrete, paint_space_rect(10.0, 70.0, 100.0, 50.0));
        assert_eq!(geometry.visible, geometry.concrete);
    }

    #[test]
    fn ratio_only_object_fit_scale_down_uses_the_same_default_sizing_result() {
        let destination = paint_space_rect(10.0, 20.0, 100.0, 100.0);
        let natural = CssImageNaturalDimensions::from_layout_axes(None, None, Some(2.0));

        let geometry = concrete_object_geometry(
            destination,
            natural,
            ObjectFit::ScaleDown,
            css::BackgroundPosition::INITIAL,
        )
        .expect("ratio-only image has a concrete object size");

        assert_eq!(geometry.concrete, paint_space_rect(10.0, 70.0, 100.0, 50.0));
    }

    #[test]
    fn explicit_natural_dimensions_keep_none_size_and_position() {
        let geometry = concrete_object_geometry(
            paint_space_rect(10.0, 20.0, 100.0, 100.0),
            CssImageNaturalDimensions::from_layout_size(LayoutSize::new(16.0, 8.0)),
            ObjectFit::None,
            css::BackgroundPosition::INITIAL,
        )
        .expect("explicit image dimensions are paintable");

        assert_eq!(geometry.concrete, paint_space_rect(10.0, 112.0, 16.0, 8.0));
    }

    #[test]
    fn contain_geometry_keeps_object_position_when_only_one_axis_has_free_space() {
        let top_right = css::BackgroundPosition {
            x: css::BackgroundPositionAxis {
                origin: css::BackgroundPositionOrigin::End,
                offset: ComputedLengthPercentage::ZERO,
            },
            y: css::BackgroundPositionAxis {
                origin: css::BackgroundPositionOrigin::Start,
                offset: ComputedLengthPercentage::ZERO,
            },
        };
        let geometry = concrete_object_geometry(
            paint_space_rect(10.0, 20.0, 100.0, 100.0),
            CssImageNaturalDimensions::from_layout_size(LayoutSize::new(50.0, 100.0)),
            ObjectFit::Contain,
            top_right,
        )
        .expect("positive contained object is paintable");

        assert_eq!(geometry.concrete, paint_space_rect(60.0, 20.0, 50.0, 100.0));
        assert_eq!(geometry.visible, geometry.concrete);
    }

    #[test]
    fn svg_without_object_view_box_does_not_add_a_duplicate_destination_clip() {
        let asset = Rc::new(
            crate::svg::parse_svg_bytes(
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100" preserveAspectRatio="none"><rect width="100" height="100" fill="red"/></svg>"#,
            )
            .expect("simple SVG source"),
        );

        let group = svg_replaced_group(
            &asset,
            paint_space_rect(10.0, 20.0, 100.0, 50.0),
            ObjectFit::Contain,
            css::BackgroundPosition::INITIAL,
            css::ObjectViewBox::NONE,
        );

        let path = first_svg_path(&group).expect("contained SVG produces a vector path");
        assert!(
            path.clip.is_none(),
            "the root viewport clip is redundant for this contained path; object-fit must not add a second one"
        );
    }

    #[test]
    fn object_view_box_scales_present_axes_and_ratio_before_none_sizing() {
        let natural = CssImageNaturalDimensions::from_layout_axes(
            Some(crate::units::layout_pt(80.0)),
            None,
            Some(2.0),
        )
        .scaled(0.5, 0.25);

        let geometry = concrete_object_geometry(
            paint_space_rect(0.0, 0.0, 100.0, 100.0),
            natural,
            ObjectFit::None,
            css::BackgroundPosition::INITIAL,
        )
        .expect("scaled object-view-box dimensions are paintable");

        assert_eq!(geometry.concrete, paint_space_rect(0.0, 90.0, 40.0, 10.0));
    }

    #[test]
    fn svg_object_view_box_projects_source_crop_before_destination_paint() {
        let asset = Rc::new(
            crate::svg::parse_svg_bytes(
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100" preserveAspectRatio="none"><rect width="100" height="100" fill="red"/></svg>"#,
            )
            .expect("simple SVG source"),
        );
        let view_box = css::ObjectViewBox::Xywh {
            x: ComputedLengthPercentage::ZERO,
            y: ComputedLengthPercentage::ZERO,
            width: ComputedLengthPercentage::from_percent(0.5),
            height: ComputedLengthPercentage::from_percent(1.0),
            radii: None,
        };

        let group = svg_replaced_group(
            &asset,
            paint_space_rect(10.0, 20.0, 100.0, 100.0),
            ObjectFit::Fill,
            css::BackgroundPosition::INITIAL,
            view_box,
        );

        let path = first_svg_path(&group).expect("cropped SVG produces a vector path");
        assert!(
            path.clip.is_some(),
            "source crop projects a destination clip"
        );
        let bounds = path.paint_bounds().expect("SVG path has paint bounds");
        // The full SVG source is intentionally painted at twice the target
        // width; the destination clip selects its left-half view box without
        // losing fractional source geometry.
        assert!((bounds.origin.x - 10.0).abs() < 0.01);
        assert!((bounds.origin.y - 20.0).abs() < 0.01);
        assert!((bounds.size.width - 200.0).abs() < 0.01);
        assert!((bounds.size.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn nested_positioned_page_span_keeps_the_furthest_requirement() {
        assert_eq!(
            merged_positioned_page_span_target(Some(2), Some(4)),
            Some(4)
        );
        assert_eq!(
            merged_positioned_page_span_target(Some(4), Some(2)),
            Some(4)
        );
        assert_eq!(merged_positioned_page_span_target(None, Some(3)), Some(3));
        assert_eq!(merged_positioned_page_span_target(Some(3), None), Some(3));
    }
}
