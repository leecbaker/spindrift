use super::*;
use std::rc::Rc;

#[test]
fn generated_gradient_pixel_size_preserves_asymmetric_paint_size() {
    assert_eq!(
        generated_image_pixel_size(PaintSize::new(30.0, 11.0)),
        RasterPixelSize::new(60, 22)
    );
}

#[test]
fn fractional_background_clip_preserves_source_mapping_and_intersects_rounded_clip() {
    let image = RenderedImage::from_paint_rect(
        paint_space_rect(10.0, 20.0, 100.0, 50.0),
        true,
        200,
        100,
        Some(RenderedImageSourceRect {
            x: 10,
            y: 20,
            width: 200,
            height: 100,
        }),
        true,
        Rc::from(Vec::new().into_boxed_slice()),
        None,
        None,
    );

    let rounded_clip = RenderedPathClip::new(
        paint_rect_path_commands(paint_space_rect(35.0, 35.0, 40.0, 10.0)),
        RenderedPathFillRule::EvenOdd,
        vec![RenderedPathClipPath::new(
            paint_rect_path_commands(paint_space_rect(40.0, 35.0, 20.0, 10.0)),
            RenderedPathFillRule::NonZero,
        )],
    );

    let clipped = clip_background_image_to_paint_area(
        image,
        PaintBackgroundArea::from_paint_rect(paint_space_rect(30.0, 30.0, 50.0, 20.0)),
        Some(rounded_clip.clone()),
    )
    .expect("overlapping paint rectangles should retain an image");

    assert_eq!(
        clipped.paint_rect(),
        paint_space_rect(10.0, 20.0, 100.0, 50.0)
    );
    assert_eq!(
        clipped.source_rect(),
        Some(RenderedImageSourceRect {
            x: 10,
            y: 20,
            width: 200,
            height: 100,
        }),
    );
    let clip = clipped
        .clip()
        .expect("partial tile installs a destination clip");
    assert_eq!(
        clip.commands,
        paint_rect_path_commands(paint_space_rect(30.0, 30.0, 50.0, 20.0))
    );
    assert_eq!(
        clip.additional_clips,
        vec![
            RenderedPathClipPath::new(rounded_clip.commands, rounded_clip.fill_rule),
            rounded_clip.additional_clips.into_iter().next().unwrap(),
        ]
    );
}

#[test]
fn contained_background_tile_keeps_only_its_rounded_clip() {
    let image = RenderedImage::from_paint_rect(
        paint_space_rect(30.0, 30.0, 20.0, 20.0),
        true,
        20,
        20,
        None,
        true,
        Rc::from(Vec::new().into_boxed_slice()),
        None,
        None,
    );
    let rounded_clip = RenderedPathClip::new(
        paint_rect_path_commands(paint_space_rect(30.0, 30.0, 20.0, 20.0)),
        RenderedPathFillRule::NonZero,
        Vec::new(),
    );

    let clipped = clip_background_image_to_paint_area(
        image,
        PaintBackgroundArea::from_paint_rect(paint_space_rect(20.0, 20.0, 40.0, 40.0)),
        Some(rounded_clip.clone()),
    )
    .expect("contained tile remains paintable");

    assert_eq!(
        clipped.paint_rect(),
        paint_space_rect(30.0, 30.0, 20.0, 20.0)
    );
    assert_eq!(clipped.clip(), Some(&rounded_clip));
}

fn containing_block(width: f32) -> ContainingBlock {
    ContainingBlock::from_page_top_rect(PageTopRect::new(0.0, 100.0, width, 100.0))
}

fn length(value: f32) -> css::ComputedLengthPercentageOrAuto {
    css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(value),
    )
}

#[test]
fn physical_horizontal_axis_uses_block_start_for_vertical_writing_modes() {
    assert_eq!(
        physical_horizontal_axis_direction(WritingMode::HorizontalTb, Direction::Rtl),
        Direction::Rtl
    );
    assert_eq!(
        physical_horizontal_axis_direction(WritingMode::VerticalLr, Direction::Rtl),
        Direction::Ltr
    );
    assert_eq!(
        physical_horizontal_axis_direction(WritingMode::VerticalRl, Direction::Ltr),
        Direction::Rtl
    );
}

#[test]
fn absolute_positioned_height_basis_uses_explicit_height() {
    let mut style = ComputedStyle::initial();
    style.box_values.height.replace_with_used(length(40.0));

    let basis =
        absolute_positioned_content_height_percentage_basis(&style, containing_block(100.0), 0.0);

    assert!(basis.is_definite());
    assert!((basis.points().unwrap() - 40.0).abs() < 0.01);
}

#[test]
fn absolute_positioned_height_basis_uses_top_bottom_fill() {
    let mut style = ComputedStyle::initial();
    style.box_values.inset_top = length(10.0);
    style.box_values.inset_bottom = length(20.0);

    let basis =
        absolute_positioned_content_height_percentage_basis(&style, containing_block(100.0), 0.0);

    assert!(basis.is_definite());
    assert!((basis.points().unwrap() - 70.0).abs() < 0.01);
}

#[test]
fn absolute_positioned_height_basis_keeps_one_inset_auto_height_indefinite() {
    let mut style = ComputedStyle::initial();
    style.box_values.inset_top = length(10.0);

    let basis =
        absolute_positioned_content_height_percentage_basis(&style, containing_block(100.0), 0.0);

    assert!(!basis.is_definite());
    assert_eq!(basis.points(), None);
}

#[test]
fn abspos_stretch_does_not_size_an_axis_with_an_auto_inset() {
    let mut style = ComputedStyle::initial();
    style.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Stretch);
    style.box_values.inset_top = length(0.0);

    let axis = resolve_absolute_vertical(&style, containing_block(100.0), 0.0, None, 0.0, 0.0);

    assert_eq!(axis.start, 0.0);
    assert_eq!(axis.size, 0.0);
}

#[test]
fn abspos_stretch_fills_an_axis_with_two_definite_insets() {
    let mut style = ComputedStyle::initial();
    style.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Stretch);
    style.box_values.inset_top = length(10.0);
    style.box_values.inset_bottom = length(20.0);

    let axis = resolve_absolute_vertical(&style, containing_block(100.0), 0.0, None, 0.0, 0.0);

    assert_eq!(axis.start, 10.0);
    assert_eq!(axis.size, 70.0);
}

#[test]
fn rtl_auto_width_absolute_horizontal_uses_static_right() {
    let style = ComputedStyle::initial();
    let axis = resolve_absolute_horizontal(
        &style,
        containing_block(100.0),
        30.0,
        StaticHorizontalPosition::new(0.0, 0.0),
        Direction::Rtl,
    );

    assert!((axis.start - 70.0).abs() < 0.01, "{axis:?}");
    assert!((axis.size - 30.0).abs() < 0.01, "{axis:?}");
}

#[test]
fn rtl_definite_width_absolute_horizontal_uses_static_right() {
    let mut style = ComputedStyle::initial();
    style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(25.0),
    );
    let axis = resolve_absolute_horizontal(
        &style,
        containing_block(100.0),
        30.0,
        StaticHorizontalPosition::new(0.0, 0.0),
        Direction::Rtl,
    );

    assert!((axis.start - 75.0).abs() < 0.01, "{axis:?}");
    assert!((axis.size - 25.0).abs() < 0.01, "{axis:?}");
}

#[test]
fn repeated_border_image_tiles_share_decoded_pixel_storage() {
    let decoded = DecodedPngImage::new(1, 1, vec![20, 40, 60], Some(vec![255]));
    let mut images = Vec::new();

    push_border_image_tiles(
        &mut images,
        &decoded,
        RenderedImageTileRect::from_paint_rect(paint_space_rect(0.0, 0.0, 3.0, 1.0)),
        RenderedImageSourceRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        css::BorderImageRepeatKeyword::Repeat,
        css::BorderImageRepeatKeyword::Stretch,
        true,
    );

    assert!(images.len() > 1, "{images:?}");
    assert!(
        images[1..]
            .iter()
            .all(|image| images[0].pixel_storage_ptr_eq(image))
    );
}

#[test]
fn repeated_border_image_tiles_center_a_clipped_single_tile() {
    assert_eq!(
        repeat_border_image_tile_segments(10.0, 16.0, 160),
        vec![BorderImageTileSegment {
            destination_offset: 0.0,
            destination_size: 10.0,
            source_offset: 30,
            source_size: 100,
        }],
    );
}

#[test]
fn spaced_border_image_tiles_include_gaps_at_both_edges() {
    assert_eq!(
        border_image_tile_segments(css::BorderImageRepeatKeyword::Space, 296.0, 100.0, 50),
        vec![
            BorderImageTileSegment {
                destination_offset: 32.0,
                destination_size: 100.0,
                source_offset: 0,
                source_size: 50,
            },
            BorderImageTileSegment {
                destination_offset: 164.0,
                destination_size: 100.0,
                source_offset: 0,
                source_size: 50,
            },
        ],
    );
}

#[test]
fn round_repeat_rescales_an_auto_opposite_background_size_axis() {
    let decoded = DecodedPngImage::new(100, 100, vec![0; 100 * 100 * 3], None);
    let mut layer = css::BackgroundLayer::initial();
    layer.image = css::ComputedImage::image(css::BackgroundImage::Url {
        src: "image.png".to_string(),
        base_url: None,
        root_url: None,
        request_modifiers: css::RequestUrlModifiers::default(),
    });
    layer.size = css::BackgroundSize::Explicit {
        width: css::BackgroundSizeAxis::LengthPercentage(
            css::ComputedLengthPercentage::from_points(52.0),
        ),
        height: css::BackgroundSizeAxis::Auto,
    };
    layer.repeat = css::BackgroundRepeat::new(
        css::BackgroundRepeatAxis::Round,
        css::BackgroundRepeatAxis::Repeat,
    );

    let size = used_background_layer_size(&decoded, &layer, PaintSize::new(180.0, 180.0));

    assert!((size.width - 60.0).abs() < 0.01, "{}", size.width);
    assert!((size.height - 60.0).abs() < 0.01, "{}", size.height);
}

#[test]
fn no_repeat_retains_the_tile_for_a_zero_sized_positioning_area() {
    assert_eq!(
        background_tile_positions(25.0, 25.0, 0.0, 50.0, css::BackgroundRepeatAxis::NoRepeat,),
        vec![25.0],
    );
    assert!(
        background_tile_positions(25.0, 25.0, 0.0, 50.0, css::BackgroundRepeatAxis::Repeat,)
            .is_empty()
    );
}

#[test]
fn fixed_background_positioning_uses_viewport_and_ignores_origin() {
    let mut style = ComputedStyle::initial();
    style.padding = css::Edges {
        top: 10.0,
        right: 10.0,
        bottom: 10.0,
        left: 10.0,
    };
    style.border_widths = css::Edges {
        top: 5.0,
        right: 5.0,
        bottom: 5.0,
        left: 5.0,
    };
    let border_area =
        PaintBackgroundArea::new(PaintPoint::new(100.0, 50.0), PaintSize::new(80.0, 60.0));
    let viewport =
        PaintBackgroundArea::new(PaintPoint::new(0.0, 0.0), PaintSize::new(500.0, 700.0));
    let mut layer = css::BackgroundLayer::initial();
    layer.origin = css::BackgroundBox::Content;
    layer.attachment = css::BackgroundAttachment::Fixed;

    assert_eq!(
        background_positioning_area_for_layer(border_area, Some(viewport), false, &style, &layer,),
        viewport,
    );

    style
        .transform
        .push(css::TransformFunction::Scale(css::CssScaleFactors {
            x: 1.0,
            y: 1.0,
        }));
    assert_eq!(
        background_positioning_area_for_layer(border_area, Some(viewport), true, &style, &layer,),
        PaintBackgroundArea::new(PaintPoint::new(110.0, 60.0), PaintSize::new(60.0, 40.0),),
    );
    style.transform.clear();

    layer.attachment = css::BackgroundAttachment::Scroll;
    assert_eq!(
        background_positioning_area_for_layer(border_area, Some(viewport), false, &style, &layer,),
        PaintBackgroundArea::new(PaintPoint::new(110.0, 60.0), PaintSize::new(60.0, 40.0),),
    );
}

#[test]
fn background_areas_preserve_bottom_left_insets_and_disjoint_intersections() {
    let area = PaintBackgroundArea::new(PaintPoint::new(10.0, 20.0), PaintSize::new(100.0, 80.0));

    assert_eq!(
        area.inset(css::Edges {
            top: 7.0,
            right: 11.0,
            bottom: 13.0,
            left: 17.0,
        })
        .paint_rect(),
        paint_space_rect(27.0, 33.0, 72.0, 60.0),
    );
    assert!(
        area.intersect(PaintBackgroundArea::new(
            PaintPoint::new(200.0, 300.0),
            PaintSize::new(10.0, 10.0),
        ))
        .is_none()
    );
}

#[test]
fn document_canvas_background_projection_preserves_tile_phase_per_page() {
    let canvas_tile = DocumentCanvasBackgroundArea::new(
        DocumentCanvasPoint::new(25.0, 240.0),
        DocumentCanvasSize::new(40.0, 30.0),
    );
    let first_page = canvas_tile.project_to_paint(200.0);
    let second_page = canvas_tile.project_to_paint(100.0);

    assert_eq!(
        first_page.paint_rect(),
        paint_space_rect(25.0, 40.0, 40.0, 30.0)
    );
    assert_eq!(
        second_page.paint_rect(),
        paint_space_rect(25.0, 140.0, 40.0, 30.0)
    );
    assert_eq!(
        second_page.y() - first_page.y(),
        100.0,
        "projecting positioning, clip, and fixed areas by the same page bottom preserves their relative phase"
    );
}

#[test]
fn repeated_uniform_background_covers_the_clip_beyond_its_positioning_area() {
    assert_eq!(
        color_image_axis_tiles(
            100.0,
            0.0,
            10.0,
            css::BackgroundRepeatAxis::Repeat,
            Vec::new(),
            0.0,
            300.0,
        ),
        vec![(0.0, 300.0)],
    );
}

#[test]
fn opaque_uniform_raster_background_promotes_to_a_vector_color() {
    let image = DecodedPngImage::new(2, 1, vec![0, 128, 0, 0, 128, 0], None);
    assert_eq!(
        opaque_uniform_raster_color(&image, &ResourceCache::default()),
        Some(CssColor::new(0, 128, 0))
    );

    let transparent = DecodedPngImage::new(1, 1, vec![0, 128, 0], Some(vec![254]));
    assert_eq!(
        opaque_uniform_raster_color(&transparent, &ResourceCache::default()),
        None
    );
}

#[test]
fn ltr_absolute_horizontal_static_left_can_fall_after_containing_block() {
    let style = ComputedStyle::initial();
    let axis = resolve_absolute_horizontal(
        &style,
        containing_block(100.0),
        30.0,
        StaticHorizontalPosition::new_unclamped(130.0, -30.0),
        Direction::Ltr,
    );

    assert!((axis.start - 130.0).abs() < 0.01, "{axis:?}");
    assert!((axis.size - 30.0).abs() < 0.01, "{axis:?}");
}

#[test]
fn rtl_absolute_horizontal_static_right_can_fall_after_containing_block() {
    let style = ComputedStyle::initial();
    let axis = resolve_absolute_horizontal(
        &style,
        containing_block(100.0),
        30.0,
        StaticHorizontalPosition::new_unclamped(-60.0, 130.0),
        Direction::Rtl,
    );

    assert!((axis.start + 60.0).abs() < 0.01, "{axis:?}");
    assert!((axis.size - 30.0).abs() < 0.01, "{axis:?}");
}

#[test]
fn raster_hsl_decreasing_matches_longer_hue() {
    let gradient = |hue| css::LinearGradient {
        direction: css::LinearGradientDirection::Angle(90.0),
        interpolation: css::GradientInterpolationMethod {
            space: css::GradientInterpolationSpace::Hsl,
            hue,
        },
        repeating: false,
        stops: vec![
            css::GradientColorStop {
                color: css::GradientColor::CssColor(CssColor::new(255, 0, 0)),
                position: Some(css::ComputedLengthPercentage::from_percent(0.0)),
            },
            css::GradientColorStop {
                color: css::GradientColor::CssColor(CssColor::new(255, 165, 0)),
                position: Some(css::ComputedLengthPercentage::from_percent(1.0)),
            },
        ],
        hints: Vec::new(),
    };
    let size = PaintSize::new(100.0, 20.0);
    let decreasing = rasterize_linear_gradient(
        &gradient(css::HueInterpolationMethod::Decreasing),
        size,
        CssColor::TRANSPARENT,
    )
    .unwrap();
    let longer = rasterize_linear_gradient(
        &gradient(css::HueInterpolationMethod::Longer),
        size,
        CssColor::TRANSPARENT,
    )
    .unwrap();
    assert_eq!(decreasing.rgb, longer.rgb);
}

#[test]
fn raster_gradient_fallback_encodes_css_samples_in_rgb_storage_space() {
    let hard_stop = |color| css::GradientColorStop {
        color: css::GradientColor::CssColor(color),
        position: Some(css::ComputedLengthPercentage::from_percent(0.5)),
    };
    let linear = css::LinearGradient {
        direction: css::LinearGradientDirection::Corner {
            horizontal: css::GradientHorizontalDirection::Left,
            vertical: css::GradientVerticalDirection::Bottom,
        },
        interpolation: css::GradientInterpolationMethod::default(),
        repeating: false,
        stops: vec![
            hard_stop(CssColor::new(255, 0, 0)),
            hard_stop(CssColor::TRANSPARENT),
        ],
        hints: Vec::new(),
    };
    let linear =
        rasterize_linear_gradient(&linear, PaintSize::new(100.0, 100.0), CssColor::TRANSPARENT)
            .unwrap();
    assert_eq!(
        linear.color_space,
        crate::color::RasterColorSpace::BuiltIn(css::CssColorSpace::Srgb)
    );
    assert!(linear.rgb.as_chunks::<3>().0.contains(&[255, 0, 0]));
    assert!(
        linear
            .alpha
            .as_ref()
            .is_some_and(|alpha| alpha.contains(&0))
    );

    let radial = css::RadialGradient {
        shape: css::RadialGradientShape::Circle,
        size: css::RadialGradientSize::Extent(css::RadialGradientExtent::FarthestCorner),
        position: css::BackgroundPosition::INITIAL,
        interpolation: css::GradientInterpolationMethod::default(),
        repeating: false,
        stops: vec![
            css::GradientColorStop {
                color: css::GradientColor::CssColor(CssColor::new(255, 0, 0)),
                position: Some(css::ComputedLengthPercentage::from_percent(0.0)),
            },
            css::GradientColorStop {
                color: css::GradientColor::CssColor(CssColor::TRANSPARENT),
                position: Some(css::ComputedLengthPercentage::from_percent(1.0)),
            },
        ],
        hints: Vec::new(),
    };
    let radial =
        rasterize_radial_gradient(&radial, PaintSize::new(100.0, 100.0), CssColor::TRANSPARENT)
            .unwrap();
    assert_eq!(
        radial.color_space,
        crate::color::RasterColorSpace::BuiltIn(css::CssColorSpace::Srgb)
    );
    assert!(
        radial
            .alpha
            .as_ref()
            .is_some_and(|alpha| alpha.iter().any(|alpha| *alpha < 255))
    );

    let display_p3_green = CssColor::in_space(css::CssColorSpace::DisplayP3, 0.0, 1.0, 0.0, 1.0);
    let conic = css::ConicGradient {
        start_angle: 0.0,
        position: css::BackgroundPosition::INITIAL,
        interpolation: css::GradientInterpolationMethod::default(),
        repeating: false,
        stops: vec![
            css::ConicGradientStop {
                color: css::GradientColor::CssColor(display_p3_green),
                position: Some(0.0),
            },
            css::ConicGradientStop {
                color: css::GradientColor::CssColor(display_p3_green),
                position: Some(360.0),
            },
        ],
    };
    let conic =
        rasterize_conic_gradient(&conic, PaintSize::new(100.0, 100.0), CssColor::TRANSPARENT)
            .unwrap();
    assert_eq!(
        conic.color_space,
        crate::color::RasterColorSpace::BuiltIn(css::CssColorSpace::DisplayP3)
    );
}

#[test]
fn uniform_gradient_detection_resolves_missing_srgb_components() {
    let yellow = CssColor::new(255, 255, 0);
    let uniform = uniform_gradient_stop_color(
        &[
            css::GradientColor::ColorWithMissing {
                color: CssColor::new(0, 255, 0),
                missing: css::GradientMissingComponents::new(0b0101),
                source: css::GradientMissingComponentSpace::Rgb,
            },
            css::GradientColor::CssColor(yellow),
        ],
        css::GradientInterpolationMethod {
            space: css::GradientInterpolationSpace::Srgb,
            hue: css::HueInterpolationMethod::Shorter,
        },
        CssColor::BLACK,
    );
    assert_eq!(uniform, Some(yellow));
}
