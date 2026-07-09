use crate::layout::{
    Clear, Direction, Float, FloatAvoidingBfcMeasurement, FloatBand, FloatClearanceResolution,
    FloatContext, FloatId, FloatPlacement, FloatShape, LogicalFloatBand, PageBlockSpan,
    PageInlineSpan, PageTopSize, UsedFloatSide, WritingMode,
};

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
fn bfc_root_placement_retries_same_top_when_measured_height_narrows_band() {
    let context = FloatContext {
        shapes: vec![
            shape(Float::Left, 0, 0.0, 100.0, 100.0, 75.0),
            shape(Float::Left, 0, 0.0, 200.0, 75.0, 50.0),
        ],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        100.0,
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        300.0,
        |_left, available_width, _top| {
            if available_width >= 200.0 {
                FloatAvoidingBfcMeasurement {
                    border_box_left: _left,
                    border_box_width: 200.0,
                    border_box_height: 50.0,
                }
            } else {
                FloatAvoidingBfcMeasurement {
                    border_box_left: _left,
                    border_box_width: 100.0,
                    border_box_height: 100.0,
                }
            }
        },
    );

    assert_eq!(placement.left, 200.0);
    assert_eq!(placement.top, 100.0);
    assert_eq!(placement.available_width, 100.0);
    assert_eq!(placement.border_box_height, 100.0);
}

#[test]
fn bfc_root_placement_moves_fixed_width_candidate_below_too_narrow_band() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 0.0, 200.0, 100.0, 80.0)],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        100.0,
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        300.0,
        |_left, _available_width, _top| FloatAvoidingBfcMeasurement {
            border_box_left: _left,
            border_box_width: 150.0,
            border_box_height: 20.0,
        },
    );

    assert_eq!(placement.left, 0.0);
    assert_eq!(placement.top, 80.0);
    assert_eq!(placement.available_width, 300.0);
}

#[test]
fn bfc_root_placement_ignores_own_margins_for_float_collision() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 0.0, 50.0, 100.0, 0.0)],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        100.0,
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        100.0,
        |_left, available_width, _top| FloatAvoidingBfcMeasurement {
            border_box_left: _left,
            border_box_width: available_width,
            border_box_height: 100.0,
        },
    );

    assert_eq!(placement.left, 50.0);
    assert_eq!(placement.top, 100.0);
    assert_eq!(placement.available_width, 50.0);
    assert_eq!(placement.border_box_width, 50.0);
}

#[test]
fn bfc_root_placement_uses_resolved_border_box_start() {
    let context = FloatContext {
        shapes: vec![shape(Float::Right, 0, 50.0, 100.0, 100.0, 50.0)],
    };

    let placement = context.avoiding_bfc_root_position(
        0,
        100.0,
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        0.0,
        100.0,
        |left, _available_width, _top| FloatAvoidingBfcMeasurement {
            // A resolved start margin places this zero-width border box just
            // beyond the band beside the right float.  It must therefore move
            // below the float rather than being relocated to `left`.
            border_box_left: left + 51.0,
            border_box_width: 0.0,
            border_box_height: 20.0,
        },
    );

    assert_eq!(placement.left, 0.0);
    assert_eq!(placement.top, 50.0);
    assert_eq!(placement.available_width, 100.0);
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
            95.0
        ),
        60.0
    );
    assert_eq!(
        context.clearance_top(
            Clear::InlineStart,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            0,
            95.0
        ),
        95.0
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
            95.0
        ),
        60.0
    );
    assert_eq!(
        context.clearance_top(
            Clear::Left,
            WritingMode::VerticalRl,
            Direction::Ltr,
            0,
            95.0
        ),
        95.0
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
        10.0,
        20.0,
        100.0,
        90.0,
    );

    assert_eq!(band, LogicalFloatBand::new(30.0, 60.0, 70.0, 10.0));
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
        10.0,
        20.0,
        100.0,
        90.0,
    );

    assert_eq!(band, LogicalFloatBand::new(0.0, 60.0, 100.0, 40.0));
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
        20.0,
        20.0,
        100.0,
        90.0,
    );
    let next_slab = context.logical_band(
        WritingMode::VerticalRl,
        Direction::Ltr,
        0,
        0.0,
        20.0,
        100.0,
        90.0,
    );

    assert_eq!(occupied_slab, LogicalFloatBand::new(20.0, 50.0, 80.0, 30.0));
    assert_eq!(next_slab, LogicalFloatBand::new(0.0, 90.0, 100.0, 10.0));
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
        100.0,
        PageTopSize::new(20.0, 80.0),
        Clear::None,
        WritingMode::VerticalRl,
        WritingMode::VerticalRl,
        Direction::Ltr,
        10.0,
        10.0,
        None,
    );

    assert_eq!(placement, FloatPlacement::new(40.0, 100.0, 90.0));
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
        100.0,
        PageTopSize::new(20.0, 80.0),
        Clear::None,
        WritingMode::VerticalLr,
        WritingMode::VerticalLr,
        Direction::Ltr,
        10.0,
        10.0,
        None,
    );

    assert_eq!(placement, FloatPlacement::new(40.0, 100.0, 90.0));
}

#[test]
fn empty_vertical_avoidance_preserves_physical_top_for_rtl() {
    let placement = FloatContext { shapes: Vec::new() }.vertical_avoiding_position(
        0,
        100.0,
        PageTopSize::new(20.0, 80.0),
        Clear::None,
        WritingMode::VerticalLr,
        WritingMode::VerticalLr,
        Direction::Rtl,
        10.0,
        10.0,
        None,
    );

    assert_eq!(placement, FloatPlacement::new(10.0, 100.0, 90.0));
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

    assert_eq!(context.lowest_bottom_on_page(0), Some(30.0));
    assert_eq!(context.lowest_bottom_on_page(1), Some(10.0));
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
        100.0,
        PageTopSize::new(50.0, 10.0),
        Clear::None,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        10.0,
        110.0,
    );

    assert_eq!(placement, FloatPlacement::new(10.0, 70.0, 100.0));
}

#[test]
fn avoiding_position_applies_clearance_before_collision_search() {
    let context = FloatContext {
        shapes: vec![shape(Float::Left, 0, 10.0, 40.0, 100.0, 60.0)],
    };

    let placement = context.avoiding_position(
        0,
        95.0,
        PageTopSize::new(20.0, 10.0),
        Clear::Left,
        WritingMode::HorizontalTb,
        Direction::Ltr,
        10.0,
        110.0,
    );

    assert_eq!(placement.top(), 60.0);
    assert_eq!(placement.left(), 10.0);
    assert_eq!(placement.available_width(), 100.0);
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
            95.0
        ),
        60.0
    );
    assert_eq!(
        context.clearance_top(
            Clear::Right,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            1,
            95.0
        ),
        95.0
    );
}

#[test]
fn clearance_resolution_reports_future_continuation() {
    let mut first = shape(Float::Left, 0, 10.0, 40.0, 100.0, 10.0);
    first.id = FloatId(9);
    first.continues_on_next_page = true;
    let mut second = continued_shape(Float::Left, 1, 10.0, 40.0, 100.0, 50.0);
    second.id = FloatId(9);
    let context = FloatContext {
        shapes: vec![first, second],
    };

    assert_eq!(
        context.clearance_resolution(
            Clear::Both,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            0,
            95.0
        ),
        FloatClearanceResolution {
            top: 10.0,
            continued_float: Some(FloatId(9))
        }
    );
    assert_eq!(
        context.clearance_resolution(
            Clear::Both,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            1,
            95.0
        ),
        FloatClearanceResolution {
            top: 50.0,
            continued_float: None
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
