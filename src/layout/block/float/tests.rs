use crate::layout::block::{
    FloatArea, FloatBandQuery, FloatContour, FlowExclusionKind, InitialLetterLayout,
    LogicalFloatPlacement, UsedRoundedRect,
};
use crate::layout::{
    Clear, ClearedFloatOuterBlockEnd, Direction, Float, FloatAvoidanceCandidate,
    FloatAvoidanceInlineContainment, FloatBand, FloatBandPlacement, FloatClearanceTarget,
    FloatContext, FloatId, FloatPlacement, FloatRunState, FloatShape, HypotheticalClearBorderEdge,
    LogicalFloatBand, LogicalInlineSpan, PageBlockSpan, PageInlineSpan, PageTopBlockPosition,
    PageTopPoint, PageTopRect, UsedFloatSide, WritingMode, border_box_pt, margin_box_pt,
    margin_box_size_pt,
};
use crate::units::SemanticLengthExt;

fn top(value: f32) -> PageTopBlockPosition {
    PageTopBlockPosition::new(value)
}

fn hypothetical_top(value: f32) -> HypotheticalClearBorderEdge {
    HypotheticalClearBorderEdge::new(top(value))
}

fn cleared_bottom(value: f32) -> ClearedFloatOuterBlockEnd {
    ClearedFloatOuterBlockEnd::new(top(value))
}

#[test]
fn initial_letter_keeps_margin_geometry_distinct_from_wrapping_geometry() {
    let margin_box = PageTopRect::new(12.0, 100.0, 20.0, 60.0);
    let wrapping_box = PageTopRect::new(8.0, 104.0, 28.0, 68.0);
    let shape = FloatShape::initial_letter_rect(
        FloatId(1),
        UsedFloatSide::Left,
        InitialLetterLayout {
            source_order: 1,
            page_index: 0,
            writing_mode: WritingMode::HorizontalTb,
            direction: Direction::Ltr,
            used_font_size: 60.0,
            provisional: false,
            block_start_alignment_inset: 4.0,
            margin_box,
            wrapping_box,
            impacted_line_range: 0..3,
            contour: FloatContour::Rect,
        },
    );

    assert_eq!(shape.kind, FlowExclusionKind::InitialLetter);
    assert_eq!(shape.rect, wrapping_box);
    assert_eq!(shape.physical_margin_box(), margin_box);
    assert_eq!(
        shape.margin_box_inline_span(),
        PageInlineSpan::new(12.0, 20.0)
    );
    assert_eq!(
        shape.margin_box_block_span(),
        PageBlockSpan::new(100.0, 60.0)
    );
}

#[test]
fn left_float_margin_box_starts_at_available_band_left_edge() {
    let placement = FloatBandPlacement::new(FloatBand::from_edges(10.0, 30.0), top(100.0));

    assert_eq!(
        placement.inline_float_margin_box_left(UsedFloatSide::Left, margin_box_pt(12.0)),
        10.0
    );
}

#[test]
fn fitting_right_float_margin_box_ends_at_available_band_right_edge() {
    let placement = FloatBandPlacement::new(FloatBand::from_edges(10.0, 30.0), top(100.0));
    let outer_inline_extent = margin_box_pt(12.0);
    let left = placement.inline_float_margin_box_left(UsedFloatSide::Right, outer_inline_extent);

    assert_eq!(left, 18.0);
    assert_eq!(left + outer_inline_extent.points(), 30.0);
}

#[test]
fn overwide_right_float_margin_box_overflows_left_to_preserve_outer_right_edge() {
    let placement = FloatBandPlacement::new(FloatBand::from_edges(10.0, 30.0), top(100.0));
    let outer_inline_extent = margin_box_pt(40.0);
    let left = placement.inline_float_margin_box_left(UsedFloatSide::Right, outer_inline_extent);

    assert_eq!(left, -10.0);
    assert!(left < placement.available_span.left_x());
    assert_eq!(left + outer_inline_extent.points(), 30.0);
}

#[test]
fn right_float_with_negative_outer_extent_preserves_outer_right_edge() {
    let placement = FloatBandPlacement::new(FloatBand::from_edges(10.0, 30.0), top(100.0));
    let outer_inline_extent = margin_box_pt(-4.0);
    let left = placement.inline_float_margin_box_left(UsedFloatSide::Right, outer_inline_extent);

    assert_eq!(left, 34.0);
    assert_eq!(left + outer_inline_extent.points(), 30.0);
}

fn bfc_measurement(left: f32, width: f32, height: f32) -> FloatAvoidanceCandidate {
    FloatAvoidanceCandidate {
        normal_flow_border_box_inline_span: PageInlineSpan::new(left, width),
        normal_flow_border_box_block_size: border_box_pt(height),
        inline_start_containment: FloatAvoidanceInlineContainment::Required,
        inline_end_containment: FloatAvoidanceInlineContainment::Required,
    }
}

fn logical_query(
    writing_mode: WritingMode,
    direction: Direction,
    block_start: f32,
    block_size: f32,
    inline_start: f32,
    inline_size: f32,
) -> FloatBandQuery {
    FloatBandQuery {
        horizontal_slab: PageInlineSpan::new(block_start, block_size),
        vertical_slab: super::placement::vertical_physical_inline_span(
            writing_mode,
            direction,
            top(inline_start),
            crate::layout::layout_pt(inline_size),
        ),
    }
}

#[test]
fn logical_float_placement_projects_and_round_trips_every_writing_mode() {
    let containing = PageTopRect::new(10.0, 200.0, 100.0, 80.0);
    for (writing_mode, direction, expected) in [
        (
            WritingMode::HorizontalTb,
            Direction::Ltr,
            PageTopRect::new(20.0, 195.0, 30.0, 20.0),
        ),
        (
            WritingMode::HorizontalTb,
            Direction::Rtl,
            PageTopRect::new(70.0, 195.0, 30.0, 20.0),
        ),
        (
            WritingMode::VerticalRl,
            Direction::Ltr,
            PageTopRect::new(85.0, 190.0, 20.0, 30.0),
        ),
        (
            WritingMode::VerticalRl,
            Direction::Rtl,
            PageTopRect::new(85.0, 160.0, 20.0, 30.0),
        ),
        (
            WritingMode::VerticalLr,
            Direction::Ltr,
            PageTopRect::new(15.0, 190.0, 20.0, 30.0),
        ),
        (
            WritingMode::VerticalLr,
            Direction::Rtl,
            PageTopRect::new(15.0, 160.0, 20.0, 30.0),
        ),
        (
            WritingMode::SidewaysRl,
            Direction::Ltr,
            PageTopRect::new(85.0, 190.0, 20.0, 30.0),
        ),
        (
            WritingMode::SidewaysRl,
            Direction::Rtl,
            PageTopRect::new(85.0, 160.0, 20.0, 30.0),
        ),
        (
            WritingMode::SidewaysLr,
            Direction::Ltr,
            PageTopRect::new(15.0, 160.0, 20.0, 30.0),
        ),
        (
            WritingMode::SidewaysLr,
            Direction::Rtl,
            PageTopRect::new(15.0, 190.0, 20.0, 30.0),
        ),
    ] {
        for side in [UsedFloatSide::Left, UsedFloatSide::Right] {
            let placement = LogicalFloatPlacement::new(
                4,
                writing_mode,
                direction,
                side,
                containing,
                LogicalInlineSpan::new(10.0, 30.0),
                5.0,
                20.0,
            );
            assert_eq!(
                placement.margin_box, expected,
                "{writing_mode:?} {direction:?} {side:?}"
            );
            assert_eq!(
                LogicalFloatPlacement::from_physical_margin_box(
                    4,
                    writing_mode,
                    direction,
                    side,
                    containing,
                    expected,
                ),
                placement,
                "{writing_mode:?} {direction:?} {side:?}"
            );
        }
    }
}

#[test]
fn initial_letter_block_column_avoids_preceding_physical_float() {
    let context = FloatContext {
        shapes: vec![shape_with_used_side(
            Float::Left,
            UsedFloatSide::Left,
            0,
            10.0,
            40.0,
            100.0,
            20.0,
        )],
    };
    assert_eq!(
        context.initial_letter_block_start_avoiding_x(
            0,
            WritingMode::VerticalRl,
            PageTopRect::new(20.0, 90.0, 50.0, 40.0),
        ),
        40.0
    );

    let context = FloatContext {
        shapes: vec![shape_with_used_side(
            Float::Right,
            UsedFloatSide::Right,
            0,
            70.0,
            100.0,
            100.0,
            20.0,
        )],
    };
    assert_eq!(
        context.initial_letter_block_start_avoiding_x(
            0,
            WritingMode::VerticalLr,
            PageTopRect::new(50.0, 90.0, 40.0, 40.0),
        ),
        30.0
    );
}

fn shape(
    side: Float,
    page_index: usize,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
) -> FloatShape {
    shape_with_used_side(
        side,
        UsedFloatSide::from_float(side, WritingMode::HorizontalTb, Direction::Ltr).unwrap(),
        page_index,
        left,
        right,
        top,
        bottom,
    )
}

fn shape_with_used_side(
    specified_side: Float,
    side: UsedFloatSide,
    page_index: usize,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
) -> FloatShape {
    FloatShape::from_edges(
        FloatId(1),
        specified_side,
        side,
        1,
        0,
        false,
        false,
        page_index,
        left,
        right,
        top,
        bottom,
    )
}

fn continued_shape(
    side: Float,
    page_index: usize,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
) -> FloatShape {
    let mut shape = shape(side, page_index, left, right, top, bottom);
    shape.starts_on_previous_page = true;
    shape.fragment_index = 1;
    shape
}

#[test]
fn float_band_combines_active_left_and_right_shapes() {
    let context = FloatContext {
        shapes: vec![
            shape(Float::Left, 0, 10.0, 40.0, 100.0, 60.0),
            shape(Float::Right, 0, 80.0, 110.0, 95.0, 55.0),
            shape(Float::Left, 0, 10.0, 70.0, 40.0, 10.0),
        ],
    };

    let band = context.band(
        0,
        PageBlockSpan::new(90.0, 20.0),
        PageInlineSpan::from_edges(10.0, 110.0),
    );

    assert_eq!(band, FloatBand::from_edges(40.0, 80.0));
}

#[test]
fn float_band_ignores_inactive_and_other_page_shapes() {
    let context = FloatContext {
        shapes: vec![
            shape(Float::Left, 1, 10.0, 80.0, 100.0, 60.0),
            shape(Float::Left, 0, 10.0, 80.0, 40.0, 10.0),
        ],
    };

    let band = context.band(
        0,
        PageBlockSpan::new(90.0, 20.0),
        PageInlineSpan::from_edges(10.0, 110.0),
    );

    assert_eq!(band, FloatBand::from_edges(10.0, 110.0));
}

#[test]
fn float_band_ignores_zero_height_shape_at_same_top() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 10.0, 30.0, 100.0, 100.0)],
    };

    let band = context.band(
        0,
        PageBlockSpan::new(100.0, 20.0),
        PageInlineSpan::from_edges(10.0, 110.0),
    );

    assert_eq!(band, FloatBand::from_edges(10.0, 110.0));
}

#[test]
fn float_shape_exposes_margin_box_spans() {
    let shape = shape(Float::Left, 0, 10.0, 40.0, 100.0, 70.0);

    assert_eq!(
        shape.margin_box_inline_span(),
        PageInlineSpan::from_edges(10.0, 40.0)
    );
    assert_eq!(
        shape.margin_box_block_span(),
        PageBlockSpan::from_edges(100.0, 70.0)
    );
}

#[test]
fn negative_outer_inline_extent_fits_right_band_without_narrowing_exclusion() {
    // The right margin edge is aligned to x=100 while the negative outer
    // width puts the other margin edge beyond it. CSS 2.2 fits this float by
    // its signed outer extent, but it has no positive-area line exclusion.
    let mut shape = FloatShape::from_rect(
        FloatId(9),
        Float::Right,
        UsedFloatSide::Right,
        0,
        0,
        PageTopRect::new(130.0, 100.0, 0.0, 20.0),
    );
    shape.outer_inline_extent = margin_box_pt(-30.0);
    let edges = shape.outer_inline_edges();
    let band = PageInlineSpan::from_edges(80.0, 100.0);

    assert_eq!(edges.signed_extent(), margin_box_pt(-30.0));
    assert!(edges.fits_at_used_side_in_band(
        UsedFloatSide::Right,
        band,
        super::exclusions::FLOAT_EPSILON,
    ));
    assert_eq!(
        shape.margin_box_inline_span(),
        PageInlineSpan::new(130.0, 0.0)
    );
}

#[test]
fn positive_outer_inline_extent_still_requires_space_in_right_band() {
    let shape = FloatShape::from_rect(
        FloatId(10),
        Float::Right,
        UsedFloatSide::Right,
        0,
        0,
        PageTopRect::new(20.0, 100.0, 80.0, 20.0),
    );

    assert!(!shape.outer_inline_edges().fits_at_used_side_in_band(
        UsedFloatSide::Right,
        PageInlineSpan::from_edges(80.0, 100.0),
        super::exclusions::FLOAT_EPSILON,
    ));
}

#[test]
fn float_run_reset_uses_typed_page_span_and_position() {
    let row_span = PageInlineSpan::from_edges(10.0, 110.0);
    let mut run = FloatRunState::new(row_span, top(100.0));
    run.include_shape(shape(Float::Left, 0, 10.0, 40.0, 100.0, 80.0));

    run.reset_for_block(row_span, top(70.0));

    assert_eq!(run.row_span, row_span);
    assert_eq!(run.available_span, row_span);
    assert_eq!(run.occupied_block_span, PageBlockSpan::new(70.0, 0.0));
    assert!(!run.active);
}

#[test]
fn vertical_inline_span_normalizes_physical_top_and_bottom() {
    let span = super::placement::vertical_physical_inline_span(
        WritingMode::VerticalRl,
        Direction::Ltr,
        top(100.0),
        crate::layout::layout_pt(90.0),
    );

    assert_eq!(span, PageBlockSpan::from_edges(100.0, 10.0));
}

#[test]
fn content_band_uses_circle_edge_but_placement_band_keeps_margin_box() {
    let mut circle = shape(Float::Left, 0, 0.0, 100.0, 100.0, 0.0);
    circle.area = FloatArea {
        contour: FloatContour::Circle {
            center_x: 50.0,
            center_y: 50.0,
            radius: 50.0,
        },
        shape_margin: 0.0,
        margin_clip: None,
    };
    let context = FloatContext {
        shapes: vec![circle],
    };
    let slab = PageBlockSpan::new(100.0, 20.0);
    let placement = context.band(0, slab, PageInlineSpan::from_edges(0.0, 300.0));
    let content = context.content_band(0, slab, PageInlineSpan::from_edges(0.0, 300.0));
    assert_eq!(placement, FloatBand::from_edges(100.0, 300.0));
    assert!(content.left() > 75.0 && content.left() < 100.0);
    assert_eq!(content.right(), 300.0);
}

#[test]
fn content_band_uses_polygon_outermost_edge_but_placement_keeps_margin_box() {
    let mut polygon = shape(Float::Left, 0, 0.0, 100.0, 100.0, 0.0);
    polygon.area = FloatArea {
        contour: FloatContour::Polygon {
            vertices: vec![
                PageTopPoint::new(20.0, 80.0),
                PageTopPoint::new(80.0, 80.0),
                PageTopPoint::new(80.0, 20.0),
                PageTopPoint::new(20.0, 20.0),
            ],
            fill_rule: crate::css::ShapeFillRule::NonZero,
        },
        shape_margin: 0.0,
        margin_clip: None,
    };
    let context = FloatContext {
        shapes: vec![polygon],
    };
    let slab = PageBlockSpan::new(80.0, 20.0);
    let placement = context.band(0, slab, PageInlineSpan::from_edges(0.0, 300.0));
    let content = context.content_band(0, slab, PageInlineSpan::from_edges(0.0, 300.0));
    assert_eq!(placement, FloatBand::from_edges(100.0, 300.0));
    assert_eq!(content, FloatBand::from_edges(80.0, 300.0));
}

#[test]
fn raster_alpha_shape_margin_offsets_opaque_pixel_cells() {
    let mut image = shape(Float::Left, 0, 0.0, 100.0, 100.0, 0.0);
    let content_rect = crate::layout::PageTopRect::new(0.0, 100.0, 100.0, 100.0);
    image.area = FloatArea::new(
        FloatContour::RasterAlpha {
            rect: content_rect,
            // The source's left half is opaque and its right half is
            // transparent, matching an ordinary alpha-derived image shape.
            pixel_width: 2,
            pixel_height: 1,
            alpha: vec![255, 0],
            threshold: 0,
        },
        10.0,
    )
    .with_margin_clip(content_rect);
    let context = FloatContext {
        shapes: vec![image],
    };

    let band = context.content_band(
        0,
        PageBlockSpan::new(100.0, 20.0),
        PageInlineSpan::from_edges(0.0, 300.0),
    );
    assert_eq!(band, FloatBand::from_edges(60.0, 300.0));
}

#[test]
fn raster_alpha_shape_margin_is_clipped_to_the_float_margin_box() {
    let mut image = shape(Float::Left, 0, 0.0, 140.0, 100.0, 0.0);
    let content_rect = crate::layout::PageTopRect::new(20.0, 100.0, 100.0, 100.0);
    let margin_rect = crate::layout::PageTopRect::new(0.0, 100.0, 140.0, 100.0);
    image.area = FloatArea::new(
        FloatContour::RasterAlpha {
            rect: content_rect,
            // Only the right source cell is opaque. Its 10pt shape-margin
            // must extend past the image content box, while remaining inside
            // the float margin box.
            pixel_width: 2,
            pixel_height: 1,
            alpha: vec![0, 255],
            threshold: 0,
        },
        10.0,
    )
    .with_margin_clip(margin_rect);
    let context = FloatContext {
        shapes: vec![image],
    };

    let band = context.content_band(
        0,
        PageBlockSpan::new(100.0, 20.0),
        PageInlineSpan::from_edges(0.0, 300.0),
    );
    assert_eq!(band, FloatBand::from_edges(130.0, 300.0));
}

#[test]
fn concave_polygon_uses_each_line_slab_outermost_edge() {
    let mut polygon = shape(Float::Left, 0, 0.0, 200.0, 200.0, 0.0);
    polygon.area = FloatArea {
        contour: FloatContour::Polygon {
            // CSS polygon(0 40, 120 40, 120 80, 80 80, 80 120,
            // 160 120, 160 160, 0 160), expressed in page-top coordinates.
            vertices: vec![
                PageTopPoint::new(0.0, 160.0),
                PageTopPoint::new(120.0, 160.0),
                PageTopPoint::new(120.0, 120.0),
                PageTopPoint::new(80.0, 120.0),
                PageTopPoint::new(80.0, 80.0),
                PageTopPoint::new(160.0, 80.0),
                PageTopPoint::new(160.0, 40.0),
                PageTopPoint::new(0.0, 40.0),
            ],
            fill_rule: crate::css::ShapeFillRule::NonZero,
        },
        shape_margin: 0.0,
        margin_clip: None,
    };
    let context = FloatContext {
        shapes: vec![polygon],
    };
    let span = PageInlineSpan::from_edges(0.0, 200.0);
    assert_eq!(
        context.content_band(0, PageBlockSpan::new(160.0, 20.0), span),
        FloatBand::from_edges(120.0, 200.0)
    );
    assert_eq!(
        context.content_band(0, PageBlockSpan::new(120.0, 20.0), span),
        FloatBand::from_edges(80.0, 200.0)
    );
    assert_eq!(
        context.content_band(0, PageBlockSpan::new(80.0, 20.0), span),
        FloatBand::from_edges(160.0, 200.0)
    );
}

#[test]
fn content_slab_finds_the_first_circle_position_that_fits() {
    let mut circle = shape(Float::Left, 0, 0.0, 100.0, 100.0, 0.0);
    circle.area = FloatArea {
        contour: FloatContour::Circle {
            center_x: 0.0,
            center_y: 100.0,
            radius: 50.0 * 2.0_f32.sqrt(),
        },
        shape_margin: 0.0,
        margin_clip: None,
    };
    let context = FloatContext {
        shapes: vec![circle],
    };

    let top = context
        .next_content_slab_with_width(
            0,
            PageBlockSpan::new(100.0, 20.0),
            PageInlineSpan::from_edges(0.0, 300.0),
            250.0,
        )
        .expect("the line fits before the float margin box ends");

    assert!((top.points() - 50.0).abs() <= 0.01);
}

#[test]
fn content_slab_for_overwide_line_waits_for_the_full_containing_measure() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 0.0, 50.0, 100.0, 0.0)],
    };

    let top = context
        .next_content_slab_with_width(
            0,
            PageBlockSpan::new(100.0, 20.0),
            PageInlineSpan::from_edges(0.0, 100.0),
            100.0,
        )
        .expect("the full containing measure should be available below the float");

    assert_eq!(top, PageTopBlockPosition::new(0.0));
}

#[test]
fn rounded_contour_uses_the_outermost_edge_over_the_complete_slab() {
    let shape = shape(Float::Left, 0, 0.0, 100.0, 100.0, 0.0);
    let area = FloatArea {
        contour: FloatContour::RoundedRect(UsedRoundedRect {
            left: 0.0,
            right: 100.0,
            top: 100.0,
            bottom: 0.0,
            top_left: (0.0, 0.0),
            top_right: (100.0, 100.0),
            bottom_right: (0.0, 0.0),
            bottom_left: (0.0, 0.0),
        }),
        shape_margin: 0.0,
        margin_clip: None,
    };

    let edges = area
        .horizontal_edges(shape.rect, PageBlockSpan::new(100.0, 50.0))
        .expect("the slab intersects the rounded contour");

    assert!((edges.right_x() - 86.60254).abs() <= 0.01);
}

#[test]
fn shape_margin_offsets_circle_and_clips_it_to_the_float_margin_box() {
    let shape = shape(Float::Left, 0, 0.0, 100.0, 100.0, 0.0);
    let area = FloatArea::new(
        FloatContour::Circle {
            center_x: 50.0,
            center_y: 50.0,
            radius: 20.0,
        },
        15.0,
    );
    let edges = area
        .horizontal_edges(shape.rect, PageBlockSpan::new(50.0, 1.0))
        .expect("the offset circle intersects its centre line");
    assert_eq!(edges, PageInlineSpan::from_edges(15.0, 85.0));

    let clipped = FloatArea::new(
        FloatContour::Circle {
            center_x: 10.0,
            center_y: 50.0,
            radius: 20.0,
        },
        20.0,
    )
    .horizontal_edges(shape.rect, PageBlockSpan::new(50.0, 1.0))
    .expect("the offset circle still intersects its centre line");
    assert_eq!(clipped.left_x(), 0.0);
    assert_eq!(clipped.right_x(), 50.0);
}

#[test]
fn shape_margin_offsets_ellipse_and_polygon_in_horizontal_and_vertical_slabs() {
    let shape = shape(Float::Left, 0, 0.0, 100.0, 100.0, 0.0);
    let ellipse = FloatArea::new(
        FloatContour::Ellipse {
            center_x: 50.0,
            center_y: 50.0,
            radius_x: 30.0,
            radius_y: 20.0,
        },
        10.0,
    );
    let horizontal = ellipse
        .horizontal_edges(shape.rect, PageBlockSpan::new(50.0, 1.0))
        .expect("the offset ellipse intersects its centre line");
    assert_eq!(horizontal, PageInlineSpan::from_edges(10.0, 90.0));
    let vertical = ellipse
        .vertical_edges(shape.rect, PageInlineSpan::from_edges(50.0, 51.0))
        .expect("the offset ellipse intersects its centre line");
    assert_eq!(vertical, PageBlockSpan::from_edges(80.0, 20.0));

    let polygon = FloatArea::new(
        FloatContour::Polygon {
            vertices: vec![
                PageTopPoint::new(20.0, 20.0),
                PageTopPoint::new(80.0, 20.0),
                PageTopPoint::new(50.0, 80.0),
            ],
            fill_rule: crate::css::ShapeFillRule::NonZero,
        },
        10.0,
    );
    let edges = polygon
        .horizontal_edges(shape.rect, PageBlockSpan::new(50.0, 1.0))
        .expect("the offset polygon intersects the slab");
    assert!(edges.left_x() < 30.0);
    assert!(edges.right_x() > 70.0);
}

#[test]
fn bfc_root_placement_retries_same_top_when_measured_height_narrows_band() {
    let context = FloatContext {
        shapes: vec![
            shape(Float::Left, 0, 0.0, 100.0, 100.0, 75.0),
            shape(Float::Left, 0, 0.0, 200.0, 75.0, 50.0),
        ],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        top(100.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        300.0,
        |band, _top| {
            let left = band.left();
            let available_width = band.width();
            if available_width >= 200.0 {
                bfc_measurement(left, 200.0, 50.0)
            } else {
                bfc_measurement(left, 100.0, 100.0)
            }
        },
    );

    assert_eq!(placement.placement.origin, PageTopPoint::new(200.0, 100.0));
    assert_eq!(
        placement.placement.available_span,
        PageInlineSpan::new(200.0, 100.0)
    );
    assert_eq!(
        placement.candidate.normal_flow_border_box_block_size,
        border_box_pt(100.0)
    );
}

#[test]
fn bfc_root_placement_moves_fixed_width_candidate_below_too_narrow_band() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 0.0, 200.0, 100.0, 80.0)],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        top(100.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        300.0,
        |band, _top| bfc_measurement(band.left(), 150.0, 20.0),
    );

    assert_eq!(placement.placement.origin, PageTopPoint::new(0.0, 80.0));
    assert_eq!(
        placement.placement.available_span,
        PageInlineSpan::new(0.0, 300.0)
    );
}

#[test]
fn bfc_root_placement_ignores_own_margins_for_float_collision() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 0.0, 50.0, 100.0, 0.0)],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        top(100.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        100.0,
        |band, _top| bfc_measurement(band.left(), band.width(), 100.0),
    );

    assert_eq!(placement.placement.origin, PageTopPoint::new(50.0, 100.0));
    assert_eq!(
        placement.placement.available_span,
        PageInlineSpan::new(50.0, 50.0)
    );
    assert_eq!(
        placement
            .candidate
            .normal_flow_border_box_inline_span
            .width(),
        50.0
    );
}

#[test]
fn bfc_root_placement_keeps_rtl_border_box_at_band_end() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 0.0, 50.0, 100.0, 0.0)],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        top(100.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Rtl,
        0.0,
        100.0,
        |band, _top| bfc_measurement(band.right() - 40.0, 40.0, 20.0),
    );

    // The residual band is the available-space result; the resolved RTL
    // border box is anchored at its physical end and remains the collision
    // geometry used by the BFC fixed point.
    assert_eq!(
        placement.placement.available_span,
        PageInlineSpan::new(50.0, 50.0)
    );
    assert_eq!(
        placement.candidate.normal_flow_border_box_inline_span,
        PageInlineSpan::new(60.0, 40.0)
    );
}

#[test]
fn bfc_root_placement_uses_resolved_border_box_start() {
    let context = FloatContext {
        shapes: vec![shape(Float::Right, 0, 50.0, 100.0, 100.0, 50.0)],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        top(100.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        100.0,
        |band, _top| {
            // A resolved start margin places this zero-width border box just
            // beyond the band beside the right float.  It must therefore move
            // below the float rather than being relocated to `left`.
            bfc_measurement(band.left() + 51.0, 0.0, 20.0)
        },
    );

    assert_eq!(placement.placement.origin, PageTopPoint::new(0.0, 50.0));
    assert_eq!(
        placement.placement.available_span,
        PageInlineSpan::new(0.0, 100.0)
    );
}

#[test]
fn bfc_root_negative_margin_hypothetical_border_top_stays_above_a_float() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 0.0, 50.0, 100.0, 50.0)],
    };

    // A negative block-start margin has already moved the hypothetical border
    // top above the float before avoidance begins. Its border box is disjoint,
    // so CSS 2.2 normal flow must retain that top rather than clearing it.
    let placement = context.avoiding_bfc_root_position(
        0,
        top(125.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        100.0,
        |_band, _top| bfc_measurement(0.0, 50.0, 20.0),
    );

    assert_eq!(placement.placement.origin, PageTopPoint::new(0.0, 125.0));
}

#[test]
fn bfc_root_negative_margin_overlap_uses_the_border_box_to_stay_adjacent() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 0.0, 50.0, 100.0, 50.0)],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        top(75.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        100.0,
        |band, _top| bfc_measurement(band.left(), 50.0, 50.0),
    );

    assert_eq!(placement.placement.origin, PageTopPoint::new(50.0, 75.0));
    assert_eq!(
        placement.candidate.normal_flow_border_box_inline_span,
        PageInlineSpan::new(50.0, 50.0)
    );
}

#[test]
fn bfc_root_negative_margin_overlap_clears_when_its_border_box_cannot_fit() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 0.0, 50.0, 100.0, 50.0)],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        top(75.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        100.0,
        |band, _top| bfc_measurement(band.left(), 75.0, 50.0),
    );

    assert_eq!(placement.placement.origin, PageTopPoint::new(0.0, 50.0));
    assert_eq!(
        placement.placement.available_span,
        PageInlineSpan::new(0.0, 100.0)
    );
}

#[test]
fn bfc_candidate_records_negative_inline_margin_overflow_per_edge() {
    let candidate = FloatAvoidanceCandidate {
        normal_flow_border_box_inline_span: PageInlineSpan::new(-10.0, 50.0),
        normal_flow_border_box_block_size: border_box_pt(20.0),
        inline_start_containment: FloatAvoidanceInlineContainment::PermittedNegativeMarginOverflow,
        inline_end_containment: FloatAvoidanceInlineContainment::Required,
    };

    assert!(candidate.permits_inline_start_overflow());
    assert!(!candidate.permits_inline_end_overflow());
}

#[test]
fn clearance_uses_logical_direction_mapping() {
    let context = FloatContext {
        shapes: vec![shape(Float::Right, 0, 80.0, 110.0, 100.0, 60.0)],
    };

    assert_eq!(
        context.clearance_top(
            Clear::InlineStart,
            WritingMode::HorizontalTb,
            Direction::Rtl,
            0,
            hypothetical_top(95.0)
        ),
        top(60.0)
    );
    assert_eq!(
        context.clearance_top(
            Clear::InlineStart,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            0,
            hypothetical_top(95.0)
        ),
        top(95.0)
    );
}

#[test]
fn clearance_uses_vertical_logical_used_sides() {
    let context = FloatContext {
        shapes: vec![shape_with_used_side(
            Float::InlineStart,
            UsedFloatSide::Top,
            0,
            10.0,
            40.0,
            100.0,
            60.0,
        )],
    };

    assert_eq!(
        context.clearance_top(
            Clear::InlineStart,
            WritingMode::VerticalRl,
            Direction::Ltr,
            0,
            hypothetical_top(95.0)
        ),
        top(60.0)
    );
    assert_eq!(
        context.clearance_top(
            Clear::Left,
            WritingMode::VerticalRl,
            Direction::Ltr,
            0,
            hypothetical_top(95.0)
        ),
        top(95.0)
    );
}

#[test]
fn vertical_top_float_reduces_inline_start_band() {
    let context = FloatContext {
        shapes: vec![shape_with_used_side(
            Float::InlineStart,
            UsedFloatSide::Top,
            0,
            10.0,
            40.0,
            100.0,
            70.0,
        )],
    };

    let band = context.logical_band(
        WritingMode::VerticalRl,
        Direction::Ltr,
        0,
        logical_query(
            WritingMode::VerticalRl,
            Direction::Ltr,
            10.0,
            20.0,
            100.0,
            90.0,
        ),
    );

    assert_eq!(
        band,
        LogicalFloatBand::new(
            LogicalInlineSpan::new(30.0, 60.0),
            PageBlockSpan::from_edges(70.0, 10.0),
        )
    );
}

#[test]
fn vertical_left_float_uses_block_start_contour_edge() {
    let context = FloatContext {
        shapes: vec![shape_with_used_side(
            Float::Left,
            UsedFloatSide::Left,
            0,
            10.0,
            40.0,
            100.0,
            70.0,
        )],
    };

    let band = context.content_logical_band(
        WritingMode::VerticalLr,
        Direction::Ltr,
        0,
        logical_query(
            WritingMode::VerticalLr,
            Direction::Ltr,
            10.0,
            20.0,
            100.0,
            90.0,
        ),
    );

    assert_eq!(
        band,
        LogicalFloatBand::new(
            LogicalInlineSpan::new(30.0, 60.0),
            PageBlockSpan::from_edges(70.0, 10.0),
        )
    );
}

#[test]
fn vertical_bottom_float_reduces_inline_end_band() {
    let context = FloatContext {
        shapes: vec![shape_with_used_side(
            Float::InlineEnd,
            UsedFloatSide::Bottom,
            0,
            10.0,
            40.0,
            40.0,
            10.0,
        )],
    };

    let band = context.logical_band(
        WritingMode::VerticalRl,
        Direction::Ltr,
        0,
        logical_query(
            WritingMode::VerticalRl,
            Direction::Ltr,
            10.0,
            20.0,
            100.0,
            90.0,
        ),
    );

    assert_eq!(
        band,
        LogicalFloatBand::new(
            LogicalInlineSpan::new(0.0, 60.0),
            PageBlockSpan::from_edges(100.0, 40.0),
        )
    );
}

#[test]
fn vertical_logical_float_band_only_narrows_intersecting_block_slabs() {
    let context = FloatContext {
        shapes: vec![
            shape_with_used_side(
                Float::InlineStart,
                UsedFloatSide::Top,
                0,
                20.0,
                40.0,
                100.0,
                80.0,
            ),
            shape_with_used_side(
                Float::InlineEnd,
                UsedFloatSide::Bottom,
                0,
                20.0,
                40.0,
                30.0,
                10.0,
            ),
        ],
    };

    let occupied_slab = context.logical_band(
        WritingMode::VerticalRl,
        Direction::Ltr,
        0,
        logical_query(
            WritingMode::VerticalRl,
            Direction::Ltr,
            20.0,
            20.0,
            100.0,
            90.0,
        ),
    );
    let next_slab = context.logical_band(
        WritingMode::VerticalRl,
        Direction::Ltr,
        0,
        logical_query(
            WritingMode::VerticalRl,
            Direction::Ltr,
            0.0,
            20.0,
            100.0,
            90.0,
        ),
    );

    assert_eq!(
        occupied_slab,
        LogicalFloatBand::new(
            LogicalInlineSpan::new(20.0, 50.0),
            PageBlockSpan::from_edges(80.0, 30.0),
        )
    );
    assert_eq!(
        next_slab,
        LogicalFloatBand::new(
            LogicalInlineSpan::new(0.0, 90.0),
            PageBlockSpan::from_edges(100.0, 10.0),
        )
    );
}

#[test]
fn vertical_avoiding_position_moves_past_over_tall_top_exclusion() {
    let context = FloatContext {
        shapes: vec![shape_with_used_side(
            Float::InlineStart,
            UsedFloatSide::Top,
            0,
            10.0,
            40.0,
            100.0,
            70.0,
        )],
    };

    let placement = context.vertical_avoiding_position(
        0,
        top(100.0),
        margin_box_size_pt(20.0, 80.0),
        Clear::None,
        WritingMode::VerticalRl,
        WritingMode::VerticalRl,
        Direction::Ltr,
        PageInlineSpan::new(10.0, 20.0),
        top(10.0),
        None,
    );

    assert_eq!(
        placement,
        FloatBandPlacement::new(
            FloatBand::from_span(PageInlineSpan::new(40.0, 90.0)),
            top(100.0),
        )
    );
}

#[test]
fn vertical_slab_search_returns_the_next_typed_horizontal_slab() {
    let context = FloatContext {
        shapes: vec![shape_with_used_side(
            Float::InlineStart,
            UsedFloatSide::Top,
            0,
            10.0,
            40.0,
            100.0,
            70.0,
        )],
    };

    assert_eq!(
        context.next_vertical_float_slab_start(
            0,
            PageInlineSpan::new(10.0, 20.0),
            PageBlockSpan::from_edges(100.0, 10.0),
        ),
        Some(PageInlineSpan::new(40.0, 20.0)),
    );
}

#[test]
fn vertical_avoiding_position_moves_past_over_tall_bottom_exclusion() {
    let context = FloatContext {
        shapes: vec![shape_with_used_side(
            Float::InlineEnd,
            UsedFloatSide::Bottom,
            0,
            10.0,
            40.0,
            40.0,
            10.0,
        )],
    };

    let placement = context.vertical_avoiding_position(
        0,
        top(100.0),
        margin_box_size_pt(20.0, 80.0),
        Clear::None,
        WritingMode::VerticalLr,
        WritingMode::VerticalLr,
        Direction::Ltr,
        PageInlineSpan::new(10.0, 20.0),
        top(10.0),
        None,
    );

    assert_eq!(
        placement,
        FloatBandPlacement::new(
            FloatBand::from_span(PageInlineSpan::new(40.0, 90.0)),
            top(100.0),
        )
    );
}

#[test]
fn empty_vertical_avoidance_preserves_physical_top_for_rtl() {
    let placement = FloatContext { shapes: Vec::new() }.vertical_avoiding_position(
        0,
        top(100.0),
        margin_box_size_pt(20.0, 80.0),
        Clear::None,
        WritingMode::VerticalLr,
        WritingMode::VerticalLr,
        Direction::Rtl,
        PageInlineSpan::new(10.0, 20.0),
        top(10.0),
        None,
    );

    assert_eq!(
        placement,
        FloatBandPlacement::new(
            FloatBand::from_span(PageInlineSpan::new(10.0, 90.0)),
            top(100.0),
        )
    );
}

#[test]
fn float_placement_keeps_rtl_margin_box_origin_separate_from_residual_band() {
    let placement = FloatPlacement::new(
        PageTopPoint::new(70.0, 100.0),
        PageInlineSpan::new(10.0, 100.0),
    );

    assert_eq!(placement.origin, PageTopPoint::new(70.0, 100.0));
    assert_eq!(placement.available_span, PageInlineSpan::new(10.0, 100.0));
}

#[test]
fn lowest_bottom_is_page_local() {
    let context = FloatContext {
        shapes: vec![
            shape(Float::Left, 0, 10.0, 40.0, 100.0, 70.0),
            shape(Float::Left, 0, 10.0, 40.0, 80.0, 30.0),
            shape(Float::Left, 1, 10.0, 40.0, 100.0, 10.0),
        ],
    };

    assert_eq!(context.lowest_bottom_on_page(0), Some(top(30.0)));
    assert_eq!(context.lowest_bottom_on_page(1), Some(top(10.0)));
    assert_eq!(context.lowest_bottom_on_page(2), None);
}

#[test]
fn avoiding_position_uses_highest_band_that_fits() {
    let context = FloatContext {
        shapes: vec![
            shape(Float::Left, 0, 10.0, 50.0, 100.0, 70.0),
            shape(Float::Right, 0, 70.0, 110.0, 100.0, 70.0),
        ],
    };

    let placement = context.avoiding_position(
        0,
        top(100.0),
        margin_box_size_pt(50.0, 10.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        PageInlineSpan::from_edges(10.0, 110.0),
    );

    assert_eq!(
        placement,
        FloatBandPlacement::new(
            FloatBand::from_span(PageInlineSpan::new(10.0, 100.0)),
            top(70.0),
        )
    );
}

#[test]
fn avoiding_position_applies_clearance_before_collision_search() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 10.0, 40.0, 100.0, 60.0)],
    };

    let placement = context.avoiding_position(
        0,
        top(95.0),
        margin_box_size_pt(20.0, 10.0),
        Clear::Left,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        PageInlineSpan::from_edges(10.0, 110.0),
    );

    assert_eq!(placement.origin, PageTopPoint::new(10.0, 60.0));
    assert_eq!(placement.available_span, PageInlineSpan::new(10.0, 100.0));
}

#[test]
fn clearance_sees_continued_float_fragment_on_current_page() {
    let context = FloatContext {
        shapes: vec![continued_shape(Float::Left, 1, 10.0, 40.0, 100.0, 60.0)],
    };

    assert_eq!(
        context.clearance_top(
            Clear::Both,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            1,
            hypothetical_top(95.0)
        ),
        top(60.0)
    );
    assert_eq!(
        context.clearance_top(
            Clear::Right,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            1,
            hypothetical_top(95.0)
        ),
        top(95.0)
    );
}

#[test]
fn clearance_target_reports_future_continuation() {
    let mut first = shape(Float::Left, 0, 10.0, 40.0, 100.0, 10.0);
    first.id = FloatId(9);
    first.continues_on_next_page = true;
    let mut second = continued_shape(Float::Left, 1, 10.0, 40.0, 100.0, 50.0);
    second.id = FloatId(9);
    let context = FloatContext {
        shapes: vec![first, second],
    };

    assert_eq!(
        context.clearance_target(
            Clear::Both,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            0,
            hypothetical_top(95.0)
        ),
        FloatClearanceTarget {
            lowest_matching_outer_block_end: Some(cleared_bottom(10.0)),
            continued_float: Some(FloatId(9))
        }
    );
    assert_eq!(
        context.clearance_target(
            Clear::Both,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            1,
            hypothetical_top(95.0)
        ),
        FloatClearanceTarget {
            lowest_matching_outer_block_end: Some(cleared_bottom(50.0)),
            continued_float: None
        }
    );
}

#[test]
fn clearance_target_is_a_pure_query_when_no_float_matches() {
    let context = FloatContext {
        shapes: vec![shape(Float::Right, 0, 10.0, 40.0, 100.0, 60.0)],
    };

    assert_eq!(
        context.clearance_target(
            Clear::Left,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            0,
            hypothetical_top(95.0),
        ),
        FloatClearanceTarget {
            lowest_matching_outer_block_end: None,
            continued_float: None,
        }
    );
}

#[test]
fn clearance_target_does_not_select_a_float_before_the_hypothetical_edge() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 10.0, 40.0, 100.0, 60.0)],
    };

    assert_eq!(
        context.clearance_target(
            Clear::Left,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            0,
            hypothetical_top(50.0),
        ),
        FloatClearanceTarget {
            lowest_matching_outer_block_end: None,
            continued_float: None,
        }
    );
}

#[test]
fn float_shape_keeps_fragment_identity_and_source_order() {
    let mut second = continued_shape(Float::Right, 2, 70.0, 110.0, 100.0, 60.0);
    second.id = FloatId(7);
    second.source_order = 42;
    second.continues_on_next_page = true;

    assert_eq!(second.id, FloatId(7));
    assert_eq!(second.fragment_index, 1);
    assert_eq!(second.source_order, 42);
    assert!(second.starts_on_previous_page);
    assert!(second.continues_on_next_page);
}
