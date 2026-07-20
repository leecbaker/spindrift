use super::*;

#[test]
fn orthogonality_depends_on_line_geometry_for_all_writing_modes() {
    let modes = [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ];
    for containing in modes {
        for descendant in modes {
            assert_eq!(
                writing_modes_are_orthogonal(containing, descendant),
                containing.has_vertical_lines() != descendant.has_vertical_lines(),
                "{containing:?} / {descendant:?}"
            );
        }
    }
}

#[test]
fn block_align_content_offset_uses_single_subject_fallbacks() {
    assert_eq!(
        block_align_content_y_offset(AlignContent::new(ContentAlignmentKeyword::End), 30.0),
        -30.0
    );
    assert_eq!(
        block_align_content_y_offset(
            AlignContent::new(ContentAlignmentKeyword::SpaceAround),
            30.0
        ),
        -15.0
    );
    assert_eq!(
        block_align_content_y_offset(AlignContent::safe(ContentAlignmentKeyword::Center), -20.0,),
        0.0
    );
    assert_eq!(
        block_align_content_y_offset(
            AlignContent::unsafe_position(ContentAlignmentKeyword::Center),
            -20.0,
        ),
        10.0
    );
    assert_eq!(
        block_align_content_y_offset(
            AlignContent::new(ContentAlignmentKeyword::LastBaseline),
            -20.0
        ),
        0.0
    );
    let mut scroll_container_style = ComputedStyle::initial();
    scroll_container_style.align_content = AlignContent::new(ContentAlignmentKeyword::Center);
    scroll_container_style.overflow_y = css::Overflow::Auto;
    assert_eq!(
        block_align_content_y_offset_for_style(&scroll_container_style, -20.0),
        10.0
    );
    assert!(
        block_align_content_establishes_independent_formatting_context(AlignContent::new(
            ContentAlignmentKeyword::Center
        ))
    );
    assert!(
        !block_align_content_establishes_independent_formatting_context(AlignContent::new(
            ContentAlignmentKeyword::Normal
        ))
    );
}

#[test]
fn vertical_block_align_content_offsets_use_logical_block_axis() {
    let mut style = ComputedStyle::initial();
    style.writing_mode = WritingMode::VerticalLr;
    style.align_content = AlignContent::new(ContentAlignmentKeyword::Center);
    let subject = PaintClip::from_paint_rect(paint_space_rect(10.0, 20.0, 20.0, 40.0));
    let content_inline_span = PageInlineSpan::new(10.0, 80.0);
    assert_eq!(
        vertical_block_align_content_x_offset(&style, content_inline_span, Some(subject)),
        30.0
    );

    style.align_content = AlignContent::new(ContentAlignmentKeyword::End);
    assert_eq!(
        vertical_block_align_content_x_offset(&style, content_inline_span, Some(subject)),
        60.0
    );

    style.writing_mode = WritingMode::VerticalRl;
    assert_eq!(
        vertical_block_align_content_x_offset(&style, content_inline_span, Some(subject)),
        0.0
    );
}

#[test]
fn block_border_box_projects_top_edge_to_paint_space() {
    let border_box = BlockBorderBox::from_rect(BlockRect::new(
        BlockPoint::new(12.0, 90.0),
        BlockSize::new(40.0, 25.0),
    ));
    let page_top_rect = border_box.page_top_rect();
    assert_eq!(page_top_rect.bottom_y(), 65.0);
    assert_eq!(
        page_top_rect.paint_rect(),
        paint_space_rect(12.0, 65.0, 40.0, 25.0)
    );
}

#[test]
fn typed_border_edges_control_block_margin_collapse() {
    let style = ComputedStyle::initial();
    let zero_edges = UsedEdges {
        top: layout_pt(0.0),
        right: layout_pt(0.0),
        bottom: layout_pt(0.0),
        left: layout_pt(0.0),
    };

    assert!(can_collapse_block_start_margin(
        &style,
        zero_edges,
        false,
        css::Overflow::Visible,
    ));
    assert!(can_collapse_block_end_margin(
        &style,
        zero_edges,
        false,
        css::Overflow::Visible,
    ));
    assert!(!can_collapse_block_start_margin(
        &style,
        UsedEdges {
            top: layout_pt(1.0),
            ..zero_edges
        },
        false,
        css::Overflow::Visible,
    ));
    assert!(!can_collapse_block_end_margin(
        &style,
        UsedEdges {
            bottom: layout_pt(1.0),
            ..zero_edges
        },
        false,
        css::Overflow::Visible,
    ));
}

#[test]
fn typed_vertical_non_content_preserves_auto_height_margin_collapse() {
    let style = ComputedStyle::initial();

    assert!(block_end_margin_collapse_survives_height_constraints(
        &style,
        PhysicalContentWidth::new(content_box_pt(100.0)),
        non_content_pt(24.0),
        PhysicalContentHeight::new(content_box_pt(40.0)),
    ));
}

#[test]
fn orthogonal_fallback_is_a_typed_physical_content_height() {
    let mut style = ComputedStyle::initial();
    style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(120.0),
    );
    style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(80.0),
    );

    let fallback = orthogonal_fallback_physical_content_height(
        &style,
        PercentageBasis::definite(content_box_pt(300.0)),
    );
    assert_eq!(
        fallback,
        Some(PhysicalContentHeight::new(content_box_pt(120.0)))
    );
}

#[test]
fn vertical_and_sideways_children_keep_the_initial_physical_height_when_clipped() {
    for writing_mode in [
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        let mut style = ComputedStyle::initial();
        style.writing_mode = writing_mode;
        style.overflow_x = css::Overflow::Hidden;
        let available = child_available_space_for_block(
            &style,
            PhysicalContentWidth::new(content_box_pt(300.0)),
            None,
            OrthogonalAvailableHeight::initial_containing_block(PhysicalContentHeight::new(
                content_box_pt(480.0),
            )),
            // This is the physical page-area height, intentionally distinct
            // from a vertical writing mode's logical block size (its width).
            PhysicalContentHeight::new(content_box_pt(720.0)),
        );

        assert_eq!(
            available.orthogonal_available_height.value(),
            PhysicalContentHeight::new(content_box_pt(720.0)),
            "{writing_mode:?}"
        );
    }
}

#[test]
fn nearest_scroll_container_selects_and_caps_orthogonal_fallback() {
    let mut style = ComputedStyle::initial();
    style.overflow_x = css::Overflow::Hidden;
    style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(720.0),
    );
    style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(480.0),
    );

    let available = child_available_space_for_formatting_context(
        &style,
        PhysicalContentWidth::new(content_box_pt(300.0)),
        None,
        OrthogonalAvailableHeight::nearest_scroll_container(PhysicalContentHeight::new(
            content_box_pt(240.0),
        )),
        PhysicalContentHeight::new(content_box_pt(600.0)),
    );

    assert!(matches!(
        available.orthogonal_available_height,
        OrthogonalAvailableHeight::NearestScrollContainer(_)
    ));
    assert_eq!(available.available_physical_height().points(), 600.0);
    assert!(!available.physical_height_percentage_basis().is_definite());
}

#[test]
fn unconstrained_scroll_container_stops_outer_fallback_while_non_scroller_constrains_child() {
    let inherited = OrthogonalAvailableHeight::nearest_scroll_container(
        PhysicalContentHeight::new(content_box_pt(240.0)),
    );
    let initial = PhysicalContentHeight::new(content_box_pt(600.0));
    let mut scroller = ComputedStyle::initial();
    scroller.overflow_x = css::Overflow::Hidden;

    let reset = child_available_space_for_formatting_context(
        &scroller,
        PhysicalContentWidth::new(content_box_pt(300.0)),
        None,
        inherited,
        initial,
    );
    assert!(matches!(
        reset.orthogonal_available_height,
        OrthogonalAvailableHeight::InitialContainingBlock(_)
    ));
    assert_eq!(reset.available_physical_height().points(), 600.0);

    scroller.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(240.0),
    );
    let min_only = child_available_space_for_formatting_context(
        &scroller,
        PhysicalContentWidth::new(content_box_pt(300.0)),
        None,
        inherited,
        initial,
    );
    assert!(matches!(
        min_only.orthogonal_available_height,
        OrthogonalAvailableHeight::InitialContainingBlock(_)
    ));
    assert_eq!(min_only.available_physical_height().points(), 600.0);

    let mut non_scroller = ComputedStyle::initial();
    non_scroller.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(480.0),
    );
    let preserved = child_available_space_for_formatting_context(
        &non_scroller,
        PhysicalContentWidth::new(content_box_pt(300.0)),
        None,
        inherited,
        initial,
    );
    assert_eq!(preserved.available_physical_height().points(), 480.0);
    assert_eq!(
        preserved.direct_orthogonal_available_height,
        Some(DirectOrthogonalAvailableHeight::Maximum(
            PhysicalContentHeight::new(content_box_pt(480.0))
        ))
    );

    non_scroller.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(520.0),
    );
    let minimum_floor = child_available_space_for_formatting_context(
        &non_scroller,
        PhysicalContentWidth::new(content_box_pt(300.0)),
        None,
        inherited,
        initial,
    );
    assert_eq!(
        minimum_floor.direct_orthogonal_available_height,
        Some(DirectOrthogonalAvailableHeight::MinimumFloor(
            PhysicalContentHeight::new(content_box_pt(520.0))
        ))
    );

    // A used `height` is the immediate containing block's actual constraint,
    // not the auto-height fallback. It therefore selects the direct
    // orthogonal line measure even when it is larger than the ICB fallback.
    // It must not, however, make percentage heights definite.
    let mut fixed_height = ComputedStyle::initial();
    fixed_height.box_values.height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(720.0),
    );
    let fixed = child_available_space_for_formatting_context(
        &fixed_height,
        PhysicalContentWidth::new(content_box_pt(300.0)),
        None,
        inherited,
        initial,
    );
    assert_eq!(fixed.available_physical_height().points(), 720.0);
    assert_eq!(
        fixed.direct_orthogonal_available_height,
        Some(DirectOrthogonalAvailableHeight::Definite(
            PhysicalContentHeight::new(content_box_pt(720.0))
        ))
    );
    assert!(!fixed.physical_height_percentage_basis().is_definite());

    non_scroller.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(720.0),
    );
    non_scroller.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
    let capped_direct = child_available_space_for_formatting_context(
        &non_scroller,
        PhysicalContentWidth::new(content_box_pt(300.0)),
        None,
        inherited,
        initial,
    );
    assert_eq!(capped_direct.available_physical_height().points(), 240.0);
    assert_eq!(capped_direct.direct_orthogonal_available_height, None);
    assert!(
        !capped_direct
            .physical_height_percentage_basis()
            .is_definite()
    );

    let intermediate = child_available_space_for_formatting_context(
        &ComputedStyle::initial(),
        PhysicalContentWidth::new(content_box_pt(300.0)),
        None,
        preserved.orthogonal_available_height,
        initial,
    );
    assert_eq!(intermediate.available_physical_height().points(), 240.0);
    assert!(matches!(
        intermediate.orthogonal_available_height,
        OrthogonalAvailableHeight::NearestScrollContainer(_)
    ));
    assert_eq!(intermediate.direct_orthogonal_available_height, None);
}
