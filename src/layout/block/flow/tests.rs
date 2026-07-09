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
    assert_eq!(
        vertical_block_align_content_x_offset(&style, 10.0, 80.0, Some(subject)),
        30.0
    );

    style.align_content = AlignContent::new(ContentAlignmentKeyword::End);
    assert_eq!(
        vertical_block_align_content_x_offset(&style, 10.0, 80.0, Some(subject)),
        60.0
    );

    style.writing_mode = WritingMode::VerticalRl;
    assert_eq!(
        vertical_block_align_content_x_offset(&style, 10.0, 80.0, Some(subject)),
        0.0
    );
}

#[test]
fn block_border_box_projects_top_edge_to_paint_space() {
    let border_box = BlockBorderBox::new(12.0, 90.0, 40.0, 25.0);
    let page_top_rect = border_box.page_top_rect();
    assert_eq!(page_top_rect.bottom_y(), 65.0);
    assert_eq!(
        page_top_rect.paint_rect(),
        paint_space_rect(12.0, 65.0, 40.0, 25.0)
    );
}
