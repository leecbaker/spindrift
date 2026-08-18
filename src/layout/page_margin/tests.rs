use super::*;
use crate::Css;

fn cascaded_top_left(source: &str, page_number: usize) -> PageMarginBoxSpec {
    let stylesheet = css::parse_stylesheet(&Css::from_string(source));
    page_margin_boxes_for_rules(PageMarginCascadeContext {
        page_rules: &stylesheet.page_rules,
        page_number,
        page_name: None,
        is_blank: false,
        page_progression_direction: Direction::Ltr,
        page_declarations: &stylesheet.page_declarations,
        base_page_style: &ComputedStyle::initial(),
        initial_page_size: PageSize::A4_POINTS,
    })
    .into_iter()
    .find(|box_| box_.name == "top-left")
    .expect("@top-left declaration should produce a page-margin box")
}

#[test]
fn page_margin_boxes_cascade_by_page_selector_specificity() {
    let first = cascaded_top_left(
        "@page { @top-left { content: \"base\"; color: red } }\
             @page :right { @top-left { content: \"right\" } }\
             @page :first { @top-left { content: \"first\"; color: blue } }",
        1,
    );
    let right = cascaded_top_left(
        "@page { @top-left { content: \"base\"; color: red } }\
             @page :right { @top-left { content: \"right\" } }\
             @page :first { @top-left { content: \"first\"; color: blue } }",
        3,
    );

    assert_eq!(
        first.declarations.get("content").map(String::as_str),
        Some("\"first\"")
    );
    assert_eq!(
        first.declarations.get("color").map(String::as_str),
        Some("blue")
    );
    assert_eq!(
        right.declarations.get("content").map(String::as_str),
        Some("\"right\"")
    );
    assert_eq!(
        right.declarations.get("color").map(String::as_str),
        Some("red")
    );
}

#[test]
fn page_margin_box_margin_auto_survives_the_applicability_filter() {
    let box_ = cascaded_top_left("@page { @top-left { content: \"\"; margin: auto } }", 1);

    assert!(matches!(
        box_.style.box_values.margin.left,
        css::ComputedLengthPercentageOrAuto::Auto
    ));
    assert!(matches!(
        box_.style.box_values.margin.right,
        css::ComputedLengthPercentageOrAuto::Auto
    ));
}

#[test]
fn fixed_corner_axis_centers_retained_auto_margins() {
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        "@page { @top-left-corner { content: \"\"; width: 25px; margin: auto } }",
    ));
    let box_ = page_margin_boxes_for_rules(PageMarginCascadeContext {
        page_rules: &stylesheet.page_rules,
        page_number: 1,
        page_name: None,
        is_blank: false,
        page_progression_direction: Direction::Ltr,
        page_declarations: &stylesheet.page_declarations,
        base_page_style: &ComputedStyle::initial(),
        initial_page_size: PageSize::A4_POINTS,
    })
    .into_iter()
    .find(|box_| box_.name == "top-left-corner")
    .expect("corner should be generated");
    let edges = fixed_width_axis(
        &box_,
        75.0,
        PercentageBasis::definite(layout_pt(75.0)),
        VerticalPageMarginSide::Left,
    );

    assert_eq!(edges.margin.left.points(), edges.margin.right.points());
    assert!(edges.margin.left.points() > 0.0);
}

#[test]
fn vertical_edge_fit_content_centers_auto_cross_axis_margins() {
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        "@page { @right-top { content: \"xxx\\a x\"; writing-mode: vertical-rl; margin: auto; block-size: fit-content } }",
    ));
    let box_ = page_margin_boxes_for_rules(PageMarginCascadeContext {
        page_rules: &stylesheet.page_rules,
        page_number: 1,
        page_name: None,
        is_blank: false,
        page_progression_direction: Direction::Ltr,
        page_declarations: &stylesheet.page_declarations,
        base_page_style: &ComputedStyle::initial(),
        initial_page_size: PageSize::A4_POINTS,
    })
    .into_iter()
    .find(|box_| box_.name == "right-top")
    .expect("right edge should be generated");
    assert!(matches!(
        box_.style.box_values.width,
        css::ComputedLengthPercentageOrAuto::FitContent(_)
    ));
    let edges = fixed_width_axis(
        &box_,
        72.0,
        PercentageBasis::definite(layout_pt(192.0)),
        VerticalPageMarginSide::Right,
    );

    assert_eq!(edges.margin.left.points(), edges.margin.right.points());
}

#[test]
fn page_margin_boxes_honor_cascade_layers_and_revert_layer() {
    let box_ = cascaded_top_left(
        "@layer base, theme;\
             @layer base { @page { @top-left { content: \"base\" } } }\
             @layer theme { @page { @top-left { content: \"theme\"; content: revert-layer } } }",
        1,
    );

    assert_eq!(
        box_.declarations.get("content").map(String::as_str),
        Some("\"base\"")
    );
}

#[test]
fn page_margin_boxes_finalize_font_relative_box_model_lengths() {
    let box_ = cascaded_top_left(
        "@page { font-size: 12pt; @top-left { content: \"\"; width: 5em; margin: -2em } }",
        1,
    );
    let width = used_content_box_width_or_auto(&box_.style, layout_pt(100.0), non_content_pt(0.0))
        .expect("an em width must be definite before page-margin sizing");

    assert_eq!(width.points(), 60.0);
    assert_eq!(
        margin_edge_for_page_margin_box(
            box_.style.box_values.margin.left,
            PercentageBasis::definite(layout_pt(100.0)),
        ),
        -24.0
    );
}

#[test]
fn page_margin_box_font_size_sets_its_em_sizing_basis() {
    let box_ = cascaded_top_left(
        "@page { font-size: 12pt; @top-left { content: \"\"; font-size: 2em; width: 5em } }",
        1,
    );
    let width = used_content_box_width_or_auto(&box_.style, layout_pt(200.0), non_content_pt(0.0))
        .expect("an em width must be definite before page-margin sizing");

    assert_eq!(box_.style.font_size, 24.0);
    assert_eq!(width.points(), 120.0);
}

#[test]
fn page_context_and_margin_box_inherit_base_typography_and_custom_properties() {
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        "@page { font-size: inherit; @top-left { content: \"\"; font-size: inherit; color: var(--page-accent) } @top-right { content: \"\"; font-size: inherit; color: red } }",
    ));
    let mut base_page_style = ComputedStyle::initial();
    css::apply_declarations(
        &mut base_page_style,
        &css::parse_declarations("--page-accent: rebeccapurple; font-size: 15pt"),
    );
    let mut expected_accent_style = ComputedStyle::initial();
    css::apply_declarations(
        &mut expected_accent_style,
        &css::parse_declarations("color: rebeccapurple"),
    );
    let mut expected_override_style = ComputedStyle::initial();
    css::apply_declarations(
        &mut expected_override_style,
        &css::parse_declarations("color: red"),
    );

    let boxes = page_margin_boxes_for_rules(PageMarginCascadeContext {
        page_rules: &stylesheet.page_rules,
        page_number: 1,
        page_name: None,
        is_blank: false,
        page_progression_direction: Direction::Ltr,
        page_declarations: &stylesheet.page_declarations,
        base_page_style: &base_page_style,
        initial_page_size: PageSize::A4_POINTS,
    });
    let top_left = boxes
        .iter()
        .find(|box_| box_.name == "top-left")
        .expect("@top-left declaration should produce a page-margin box");
    let top_right = boxes
        .iter()
        .find(|box_| box_.name == "top-right")
        .expect("@top-right declaration should produce a page-margin box");

    assert_eq!(top_left.style.font_size, base_page_style.font_size);
    assert_eq!(top_right.style.font_size, base_page_style.font_size);
    assert_eq!(top_left.style.color, expected_accent_style.color);
    assert_eq!(top_right.style.color, expected_override_style.color);
    assert_eq!(
        top_left.style.custom_properties,
        base_page_style.custom_properties
    );
    assert_eq!(
        top_right.style.custom_properties,
        base_page_style.custom_properties
    );
}

#[test]
fn page_margin_boxes_ignore_fragmentation_and_positioning_declarations() {
    let box_ = cascaded_top_left(
        "@page { @top-left { content: \"\"; display: flex; position: absolute; top: 1px; page-break-before: always; width: 10px; z-index: 2 } }",
        1,
    );

    assert!(box_.declarations.get("display").is_none());
    assert!(box_.declarations.get("position").is_none());
    assert!(box_.declarations.get("top").is_none());
    assert!(box_.declarations.get("page-break-before").is_none());
    assert_eq!(
        box_.declarations.get("width").map(String::as_str),
        Some("10px")
    );
    assert_eq!(box_.style.z_index, css::ZIndex::StackLevel(2));
}
fn auto(min_outer: f32, max_outer: f32) -> PageMarginBoxMeasure {
    PageMarginBoxMeasure {
        generated: true,
        specified_outer: None,
        min_outer,
        max_outer,
        min_constraint: None,
        max_constraint: None,
    }
}

#[test]
fn variable_axis_reallocates_after_maximum_saturation() {
    let mut left = auto(10.0, 20.0);
    left.max_constraint = Some(15.0);
    let sizes = resolve_variable_outer_sizes(
        100.0,
        [
            left,
            PageMarginBoxMeasure::not_generated(),
            auto(10.0, 20.0),
        ],
    );

    assert_eq!(sizes, [15.0, 0.0, 85.0]);
}

#[test]
fn variable_axis_keeps_an_auto_center_symmetric() {
    let sizes = resolve_variable_outer_sizes(
        180.0,
        [auto(20.0, 40.0), auto(10.0, 30.0), auto(40.0, 80.0)],
    );

    assert_eq!(sizes[0], sizes[2]);
    assert_eq!(sizes[1] + sizes[0] * 2.0, 180.0);
}

#[test]
fn fixed_axis_centers_two_auto_margins_when_space_is_available() {
    let (start, end) = resolve_fixed_margin_axis(
        100.0,
        10.0,
        Some(20.0),
        css::ComputedLengthPercentageOrAuto::Auto,
        css::ComputedLengthPercentageOrAuto::Auto,
        PercentageBasis::definite(layout_pt(100.0)),
        FixedAxisAutoMargin::Start,
    );

    assert_eq!((start, end), (35.0, 35.0));
}

#[test]
fn fixed_axis_clamps_auto_margins_before_an_overflowing_auto_size() {
    let (start, end) = resolve_fixed_margin_axis(
        50.0,
        10.0,
        Some(50.0),
        css::ComputedLengthPercentageOrAuto::Auto,
        css::ComputedLengthPercentageOrAuto::Auto,
        PercentageBasis::definite(layout_pt(50.0)),
        FixedAxisAutoMargin::End,
    );

    assert_eq!((start, end), (0.0, 0.0));
}

#[test]
fn fixed_axis_assigns_explicit_overconstraint_to_the_away_margin() {
    let length = |points| {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(points),
        )
    };
    let (start, end) = resolve_fixed_margin_axis(
        100.0,
        10.0,
        Some(100.0),
        length(3.0),
        length(3.0),
        PercentageBasis::definite(layout_pt(100.0)),
        FixedAxisAutoMargin::Start,
    );

    assert_eq!((start, end), (-13.0, 3.0));
}

#[test]
fn fixed_axis_reallocates_an_explicit_outside_margin_after_auto_clamping() {
    let start = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(67.5),
    );
    let (start, end) = resolve_fixed_margin_axis(
        75.0,
        4.5,
        Some(18.75),
        start,
        css::ComputedLengthPercentageOrAuto::Auto,
        PercentageBasis::definite(layout_pt(75.0)),
        FixedAxisAutoMargin::Start,
    );

    assert_eq!((start, end), (51.75, 0.0));
}

#[test]
fn fixed_axis_percentages_use_the_margin_area_being_solved() {
    let half = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_percent(0.5),
    );
    let (outside, page_facing) = resolve_fixed_margin_axis(
        72.0,
        0.0,
        None,
        css::ComputedLengthPercentageOrAuto::Auto,
        half,
        PercentageBasis::definite(layout_pt(72.0)),
        FixedAxisAutoMargin::Start,
    );

    assert_eq!((outside, page_facing), (0.0, 36.0));
}

#[test]
fn page_margin_tree_order_is_clockwise() {
    let names = [
        "top-left-corner",
        "top-left",
        "top-center",
        "top-right",
        "top-right-corner",
        "right-top",
        "right-middle",
        "right-bottom",
        "bottom-right-corner",
        "bottom-right",
        "bottom-center",
        "bottom-left",
        "bottom-left-corner",
        "left-bottom",
        "left-middle",
        "left-top",
    ];

    for (expected, name) in names.into_iter().enumerate() {
        assert_eq!(page_margin_box_paint_order(name), expected);
    }
}
