use super::values::edge_all;
use super::*;
use crate::{PageMargins, PageSize, RenderOptions, layout_pt};
use std::collections::HashMap;

fn flex_basis_length(value: ComputedLengthPercentage) -> ComputedFlexBasis {
    ComputedFlexBasis::LengthPercentage(ComputedFlexBasisLength::new(value, false))
}

fn flex_basis_percentage(value: ComputedLengthPercentage) -> ComputedFlexBasis {
    ComputedFlexBasis::LengthPercentage(ComputedFlexBasisLength::new(value, true))
}

#[tokio::test]
async fn applies_page_options() {
    let css = Css::from_string("@page { size: 200px 100px; margin: 10px } p { font-size: 20px }");
    let mut options = RenderOptions::default();
    apply_stylesheet_options(&css, &mut options);
    assert_eq!(options.page_size.width(), 150.0);
    assert_eq!(options.page_size.height(), 75.0);
    assert_eq!(options.margin(), 7.5);
    assert_eq!(options.page_margins, PageMargins::all_points(7.5));
    assert_eq!(options.font_size(), 15.0);
}

#[tokio::test]
async fn stylesheet_options_do_not_flatten_ch_font_metrics_before_fonts_load() {
    let css = Css::from_string(
        "p { font-size: 2ch; line-height: 3ch } body { font-size: calc(1ch + 10pt) }",
    );
    let mut options = RenderOptions::default();

    apply_stylesheet_options(&css, &mut options);

    assert_eq!(options.font_size(), 12.0);
    assert!((options.line_height() - 14.4).abs() < 0.001);
}

#[tokio::test]
async fn stylesheet_options_keep_metric_independent_font_defaults() {
    let css = Css::from_string("p { font-size: 20px; line-height: 24px }");
    let mut options = RenderOptions::default();

    apply_stylesheet_options(&css, &mut options);

    assert_eq!(options.font_size(), 15.0);
    assert_eq!(options.line_height(), 18.0);
}

#[tokio::test]
async fn page_size_one_length_creates_square_page() {
    let css = Css::from_string("@page { size: 5in }");
    let mut options = RenderOptions::default();
    apply_stylesheet_options(&css, &mut options);

    assert_eq!(options.page_size.width(), 360.0);
    assert_eq!(options.page_size.height(), 360.0);
}

#[tokio::test]
async fn page_size_auto_preserves_existing_page_size() {
    let stylesheet = parse_stylesheet(&Css::from_string("@page { size: auto }"));
    let base = PageSize::from_points(240.0, 120.0);

    assert_eq!(page_size_from(&stylesheet.page_declarations, base), base);
}

#[tokio::test]
async fn page_width_height_descriptors_derive_sheet_size_when_size_is_omitted() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { font: 16px/1 Ahem; margin: 4em 5em 8em 7em; width: 20em; height: 15em }",
    ));
    let size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);

    assert_eq!(size.width(), 384.0);
    assert_eq!(size.height(), 324.0);
}

#[tokio::test]
async fn page_context_ch_lengths_use_page_font_metric_fallback() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { font-size: 20pt; size: 10ch 20ch; margin: 2ch; padding: 1ch }",
    ));
    let size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
    let margins = page_margins_from_for_size(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        size,
    );
    let padding = page_padding_from_for_size(&stylesheet.page_declarations, size);

    assert_eq!(size.width(), 100.0);
    assert_eq!(size.height(), 200.0);
    assert_eq!(margins, PageMargins::all_points(20.0));
    assert_eq!(padding, edge_all(10.0));
}

#[tokio::test]
async fn page_width_height_ch_descriptors_include_fixed_ch_margins() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { font-size: 20pt; width: 10ch; height: 5ch; margin: 1ch }",
    ));
    let size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);

    assert_eq!(size.width(), 120.0);
    assert_eq!(size.height(), 70.0);
}

#[tokio::test]
async fn page_size_rejects_invalid_mixed_grammar_without_partial_application() {
    let base = PageSize::from_points(200.0, 100.0);
    for value in [
        "a4 5in",
        "5in landscape",
        "10pt foo 20pt",
        "10pt auto",
        "-1pt 20pt",
        "landscape portrait",
    ] {
        let stylesheet = parse_stylesheet(&Css::from_string(format!("@page {{ size: {value} }}")));
        assert_eq!(
            page_size_from(&stylesheet.page_declarations, base),
            base,
            "{value} should be ignored as an invalid size descriptor"
        );
    }
}

#[tokio::test]
async fn page_size_named_keywords_use_standard_dimensions() {
    let cases = [
        ("a3", 297.0, 420.0),
        ("a4", 210.0, 297.0),
        ("a5", 148.0, 210.0),
        ("b4", 250.0, 353.0),
        ("b5", 176.0, 250.0),
        ("jis-b4", 257.0, 364.0),
        ("jis-b5", 182.0, 257.0),
    ];
    for (name, width_mm, height_mm) in cases {
        let stylesheet = parse_stylesheet(&Css::from_string(format!("@page {{ size: {name} }}")));
        let size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
        assert!(
            (size.width() - width_mm * 72.0 / 25.4).abs() < 0.001,
            "{name} width should match spec"
        );
        assert!(
            (size.height() - height_mm * 72.0 / 25.4).abs() < 0.001,
            "{name} height should match spec"
        );
    }

    let cases = [
        ("letter", 8.5, 11.0),
        ("legal", 8.5, 14.0),
        ("ledger", 11.0, 17.0),
    ];
    for (name, width_in, height_in) in cases {
        let stylesheet = parse_stylesheet(&Css::from_string(format!("@page {{ size: {name} }}")));
        let size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
        assert_eq!(size.width(), width_in * 72.0, "{name} width");
        assert_eq!(size.height(), height_in * 72.0, "{name} height");
    }
}

#[tokio::test]
async fn page_orientation_descriptor_maps_to_pdf_rotation() {
    let stylesheet = parse_stylesheet(&Css::from_string("@page { page-orientation: rotate-left }"));

    assert_eq!(page_rotation_from(&stylesheet.page_declarations, 0), 270);
}

#[tokio::test]
async fn page_margin_auto_resolves_against_page_width_and_height_descriptors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: 240pt 84pt; width: 144pt; height: 36pt; margin: auto }",
    ));
    let page_size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
    let margins = page_margins_from_for_size(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        page_size,
    );

    assert_eq!(margins, PageMargins::from_points(24.0, 48.0, 24.0, 48.0));
}

#[tokio::test]
async fn page_margin_auto_edges_pin_specified_page_area() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: 240pt 84pt; width: 144pt; height: 36pt; margin: auto; margin-top: 0; margin-left: 0 }",
    ));
    let page_size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
    let margins = page_margins_from_for_size(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        page_size,
    );

    assert_eq!(margins, PageMargins::from_points(0.0, 96.0, 48.0, 0.0));
}

#[tokio::test]
async fn page_margin_auto_with_auto_page_area_size_resolves_to_zero() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: 240pt 84pt; margin: 30pt; margin-top: auto; margin-left: auto }",
    ));
    let page_size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
    let margins = page_margins_from_for_size(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        page_size,
    );

    assert_eq!(margins, PageMargins::from_points(0.0, 30.0, 30.0, 0.0));
}

#[tokio::test]
async fn page_margins_can_resolve_to_negative_lengths() {
    let stylesheet = parse_stylesheet(&Css::from_string("@page { size: 225pt; margin: -15pt }"));
    let page_size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
    let margins = page_margins_from_for_size(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        page_size,
    );

    assert_eq!(margins, PageMargins::all_points(-15.0));
}

#[tokio::test]
async fn page_auto_margins_can_resolve_negative_for_oversized_page_area() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: 225pt; width: 255pt; height: 255pt; margin: auto }",
    ));
    let page_size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
    let margins = page_margins_from_for_size(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        page_size,
    );

    assert_eq!(margins, PageMargins::all_points(-15.0));
}

#[tokio::test]
async fn page_auto_margins_include_border_and_padding_in_width_equation() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: 100pt 80pt; width: 40pt; height: 20pt; margin: auto }",
    ));
    let margins = page_margins_from_for_size_and_edges(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        PageSize::from_points(100.0, 80.0),
        Edges {
            top: 10.0,
            right: 10.0,
            bottom: 10.0,
            left: 10.0,
        },
    );

    assert_eq!(margins.left(), 20.0);
    assert_eq!(margins.right(), 20.0);
    assert_eq!(margins.top(), 20.0);
    assert_eq!(margins.bottom(), 20.0);
}

#[tokio::test]
async fn page_margin_percentages_resolve_against_physical_page_dimensions() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: 400pt 800pt; margin: 2% 8% 6% 20% }",
    ));
    let margins = page_margins_from_for_size(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        PageSize::from_points(400.0, 800.0),
    );

    assert_eq!(margins.top(), 16.0);
    assert_eq!(margins.right(), 32.0);
    assert_eq!(margins.bottom(), 48.0);
    assert_eq!(margins.left(), 80.0);
}

#[tokio::test]
async fn logical_page_margin_and_padding_map_through_vertical_writing_mode() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { writing-mode: vertical-rl; size: 400pt 800pt; \
         margin-inline-start: 2%; margin-block-start: 8%; \
         margin-inline-end: 6%; margin-block-end: 20%; \
         padding-inline-start: 2%; padding-block-start: 8%; \
         padding-inline-end: 6%; padding-block-end: 20% }",
    ));
    let size = PageSize::from_points(400.0, 800.0);
    let margins = page_margins_from_for_size(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        size,
    );
    let padding = page_padding_from_for_size(&stylesheet.page_declarations, size);

    assert_eq!(margins.top(), 16.0);
    assert_eq!(margins.right(), 32.0);
    assert_eq!(margins.bottom(), 48.0);
    assert_eq!(margins.left(), 80.0);
    assert_eq!(padding.top, 16.0);
    assert_eq!(padding.right, 32.0);
    assert_eq!(padding.bottom, 48.0);
    assert_eq!(padding.left, 80.0);
}

#[tokio::test]
async fn applies_page_edge_margins_in_source_order() {
    let css = Css::from_string(
        "@page { size: letter; margin: .5in 1in .75in .25in; margin-left: .125in }",
    );
    let mut options = RenderOptions::default();

    apply_stylesheet_options(&css, &mut options);

    assert_eq!(options.page_margins.top(), 36.0);
    assert_eq!(options.page_margins.right(), 72.0);
    assert_eq!(options.page_margins.bottom(), 54.0);
    assert_eq!(options.page_margins.left(), 9.0);
    assert_eq!(options.margin(), 36.0);
}

#[tokio::test]
async fn parses_page_margin_boxes() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { @bottom-right { content: \"Page \" counter(page); background-color: black } }",
    ));

    let bottom_right = stylesheet.page_margin_boxes.get("bottom-right").unwrap();
    assert_eq!(
        bottom_right.get("content").map(String::as_str),
        Some("\"Page \" counter(page)")
    );
    assert_eq!(
        bottom_right.get("background-color").map(String::as_str),
        Some("black")
    );
}

#[tokio::test]
async fn parses_page_rule_closed_by_eof_recovery() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { margin: 6em; width: 20em; height: 16em; font: 16px/1 Ahem; @top-left { content: \"x\" }",
    ));

    assert_eq!(stylesheet.page_rules.len(), 1);
    assert_eq!(
        stylesheet
            .page_rules
            .first()
            .unwrap()
            .margin_boxes
            .get("top-left")
            .and_then(|declarations| declarations.get("content"))
            .map(String::as_str),
        Some("\"x\"")
    );
    let size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
    assert_eq!(size.width(), 384.0);
    assert_eq!(size.height(), 336.0);
}

#[tokio::test]
async fn page_margin_at_rules_match_exact_names() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page {\
           @bottom-right { content: \"right\"; text-align: right }\
           @bottom-right-corner { content: \"corner\"; text-align: left }\
         }",
    ));

    let bottom_right = stylesheet.page_margin_boxes.get("bottom-right").unwrap();
    let corner = stylesheet
        .page_margin_boxes
        .get("bottom-right-corner")
        .unwrap();
    assert_eq!(
        bottom_right.get("content").map(String::as_str),
        Some("\"right\"")
    );
    assert_eq!(
        bottom_right.get("text-align").map(String::as_str),
        Some("right")
    );
    assert_eq!(
        corner.get("content").map(String::as_str),
        Some("\"corner\"")
    );
    assert_eq!(corner.get("text-align").map(String::as_str), Some("left"));
}

#[tokio::test]
async fn parses_invoice_page_margin_boxes_through_main_at_rule_parser() {
    let stylesheet = parse_stylesheet(
        &Css::from_file_async("weasyprint-samples/invoice/invoice.css")
            .await
            .unwrap(),
    );

    assert_eq!(stylesheet.page_rules.len(), 1);
    assert_eq!(stylesheet.page_rules[0].margin_boxes.len(), 2);
    assert_eq!(
        stylesheet
            .page_margin_boxes
            .get("bottom-left")
            .and_then(|declarations| declarations.get("content"))
            .map(String::as_str),
        Some("'♥ Thank you!'")
    );
    assert_eq!(
        stylesheet
            .page_margin_boxes
            .get("bottom-right")
            .and_then(|declarations| declarations.get("font-size"))
            .map(String::as_str),
        Some("9pt")
    );
}

#[tokio::test]
async fn cascades_page_margin_boxes_by_page_selector_specificity() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { @top-left { content: \"base\"; color: red } }\
         @page :right { @top-left { content: \"right\" } }\
         @page :first { @top-left { content: \"first\"; color: blue } }",
    ));

    let top_left = stylesheet.page_margin_boxes.get("top-left").unwrap();
    assert_eq!(
        top_left.get("content").map(String::as_str),
        Some("\"first\"")
    );
    assert_eq!(top_left.get("color").map(String::as_str), Some("blue"));
    assert_eq!(stylesheet.page_rules.len(), 3);
    assert_eq!(
        stylesheet.page_rules[1].selectors[0]
            .specificity()
            .left_or_right,
        1
    );
    assert_eq!(
        stylesheet.page_rules[2].selectors[0]
            .specificity()
            .first_or_blank,
        1
    );
}

#[tokio::test]
async fn parses_named_page_selector_with_first_pseudo_class() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page report:first { @bottom-center { content: \"report first\" } }",
    ));

    assert_eq!(stylesheet.page_rules.len(), 1);
    let selector = &stylesheet.page_rules[0].selectors[0];
    assert_eq!(selector.page_type.as_deref(), Some("report"));
    assert_eq!(selector.specificity().page_type_names, 1);
    assert_eq!(selector.specificity().first_or_blank, 1);
    assert!(
        stylesheet.page_rules[0]
            .matching_specificity(1, Some("report"), false, Direction::Ltr)
            .is_some()
    );
    assert_eq!(
        stylesheet.page_rules[0]
            .margin_boxes
            .get("bottom-center")
            .and_then(|declarations| declarations.get("content"))
            .map(String::as_str),
        Some("\"report first\"")
    );
}

#[tokio::test]
async fn page_side_selectors_follow_page_progression_direction() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page :left { margin-left: 40pt } @page :right { margin-right: 40pt }",
    ));

    let left_rule = &stylesheet.page_rules[0];
    let right_rule = &stylesheet.page_rules[1];
    assert!(
        left_rule
            .matching_specificity(1, None, false, Direction::Rtl)
            .is_some()
    );
    assert!(
        right_rule
            .matching_specificity(1, None, false, Direction::Rtl)
            .is_none()
    );
    assert!(
        left_rule
            .matching_specificity(1, None, false, Direction::Ltr)
            .is_none()
    );
    assert!(
        right_rule
            .matching_specificity(1, None, false, Direction::Ltr)
            .is_some()
    );
}

#[tokio::test]
async fn page_nth_selectors_parse_and_match_generated_page_numbers() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page :nth(2) { margin-left: 20pt }\
         @page :nth(2n + 4) { margin-left: 30pt }\
         @page :nth(3n) { margin-left: 40pt }\
         @page :nth( n+2 ) { margin-left: 50pt }\
         @page :nth(even) { margin-left: 60pt }\
         @page report:nth(2) { margin-left: 70pt }",
    ));

    assert_eq!(stylesheet.page_rules.len(), 6);
    assert!(
        stylesheet.page_rules[0]
            .matching_specificity(2, None, false, Direction::Ltr)
            .is_some()
    );
    assert!(
        stylesheet.page_rules[0]
            .matching_specificity(3, None, false, Direction::Ltr)
            .is_none()
    );
    assert!(
        stylesheet.page_rules[1]
            .matching_specificity(6, None, false, Direction::Ltr)
            .is_some()
    );
    assert!(
        stylesheet.page_rules[2]
            .matching_specificity(6, None, false, Direction::Ltr)
            .is_some()
    );
    assert!(
        stylesheet.page_rules[3]
            .matching_specificity(2, None, false, Direction::Ltr)
            .is_some()
    );
    assert!(
        stylesheet.page_rules[4]
            .matching_specificity(4, None, false, Direction::Ltr)
            .is_some()
    );
    assert!(
        stylesheet.page_rules[5]
            .matching_specificity(2, Some("report"), false, Direction::Ltr)
            .is_some()
    );
    assert_eq!(
        stylesheet.page_rules[0].selectors[0]
            .specificity()
            .first_or_blank,
        1
    );
    assert_eq!(
        stylesheet.page_rules[5].selectors[0]
            .specificity()
            .page_type_names,
        1
    );
}

#[tokio::test]
async fn cascade_layers_apply_to_page_context_before_page_specificity() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer theme { @page { margin: 1in } }\
         @layer base { @page :first { margin: 2in } }",
    ));

    let margins = page_margins_from(
        &stylesheet.first_page_declarations,
        PageMargins::all_points(0.0),
    );

    assert_eq!(margins, PageMargins::all_points(72.0));
}

#[tokio::test]
async fn important_page_context_layers_reverse_layer_order() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { @page { margin: 1in !important } }\
         @layer theme { @page :first { margin: 2in !important } }\
         @page { margin: 3in !important }",
    ));

    let margins = page_margins_from(
        &stylesheet.first_page_declarations,
        PageMargins::all_points(0.0),
    );

    assert_eq!(margins, PageMargins::all_points(72.0));
}

#[tokio::test]
async fn cascade_layers_apply_to_page_margin_boxes() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer theme { @page { @top-left { content: \"theme\" } } }\
         @layer base { @page :first { @top-left { content: \"base\" } } }",
    ));

    let top_left = stylesheet.page_margin_boxes.get("top-left").unwrap();

    assert_eq!(
        top_left.get("content").map(String::as_str),
        Some("\"theme\"")
    );
}

#[tokio::test]
async fn inactive_media_page_rules_do_not_affect_page_context() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@media screen { @page { margin: 4in } } @page { margin: 1in }",
    ));

    let margins = page_margins_from(&stylesheet.page_declarations, PageMargins::all_points(0.0));

    assert_eq!(margins, PageMargins::all_points(72.0));
}

#[tokio::test]
async fn media_not_print_does_not_apply_to_element_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@media not print { p { color: red } } p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn media_not_screen_applies_in_print_context() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: blue } @media not screen { p { color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn media_comma_list_applies_when_one_query_matches() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@media screen, print { p { color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn dir_pseudo_class_matches_document_ltr_and_rtl_direction() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p:dir(ltr) { color: blue } p:dir(rtl) { color: red }",
    ));
    let ltr = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()).with_document_direction(Direction::Ltr),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let rtl = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()).with_document_direction(Direction::Rtl),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(ltr.color, Color::new(0, 0, 255));
    assert_eq!(rtl.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn dir_pseudo_class_ignores_css_direction_property() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p:dir(ltr) { color: blue } p:dir(rtl) { color: red }",
    ));
    let parent = ComputedStyle {
        direction: Direction::Rtl,
        ..ComputedStyle::initial()
    };
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()).with_resolved_direction(Direction::Rtl),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn dir_pseudo_class_matches_html_auto_and_bdi_directionality() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ":dir(ltr) { color: blue } :dir(rtl) { color: red }",
    ));
    let mut auto_attrs = HashMap::new();
    auto_attrs.insert("dir".to_string(), "auto".to_string());
    let auto = style_for_element_with_signature(
        ElementSignature::new("p", auto_attrs)
            .with_html_direction(Direction::Rtl)
            .with_resolved_direction(Direction::Rtl),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let bdi = style_for_element_with_signature(
        ElementSignature::new("bdi", HashMap::new())
            .with_html_direction(Direction::Ltr)
            .with_resolved_direction(Direction::Ltr),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(auto.color, Color::new(255, 0, 0));
    assert_eq!(bdi.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn has_pseudo_class_matches_descendant_dir_inherited_from_document_directionality() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div { background-color: red }\
         .ltr:has(*:dir(ltr)) { background-color: lime }\
         .ltr:has(*:dir(rtl)) { background-color: red }\
         .rtl:has(*:dir(rtl)) { background-color: lime }\
         .rtl:has(*:dir(ltr)) { background-color: red }",
    ));
    let span = ElementSiblingSignature::new("span", HashMap::new());
    let ltr_class = HashMap::from([("class".to_string(), "ltr".to_string())]);
    let rtl_class = HashMap::from([("class".to_string(), "rtl".to_string())]);

    let implicit_ltr = style_for_element_with_signature(
        ElementSignature::new("div", ltr_class.clone()).with_children(vec![span.clone()], false),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let explicit_ltr = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([
                ("class".to_string(), "ltr".to_string()),
                ("dir".to_string(), "ltr".to_string()),
            ]),
        )
        .with_document_direction(Direction::Ltr)
        .with_children(vec![span.clone()], false),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let ancestor_ltr =
        style_for_element_with_signature(
            ElementSignature::new("div", ltr_class).with_children(vec![span.clone()], false),
            None,
            std::slice::from_ref(&stylesheet),
            None,
            &[ElementSignature::new("section", HashMap::new())
                .with_document_direction(Direction::Ltr)],
        );
    let explicit_rtl = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([
                ("class".to_string(), "rtl".to_string()),
                ("dir".to_string(), "rtl".to_string()),
            ]),
        )
        .with_document_direction(Direction::Rtl)
        .with_children(vec![span.clone()], false),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let ancestor_rtl =
        style_for_element_with_signature(
            ElementSignature::new("div", rtl_class).with_children(vec![span], false),
            None,
            std::slice::from_ref(&stylesheet),
            None,
            &[ElementSignature::new("section", HashMap::new())
                .with_document_direction(Direction::Rtl)],
        );

    assert_eq!(implicit_ltr.background_color, Some(Color::new(0, 255, 0)));
    assert_eq!(explicit_ltr.background_color, Some(Color::new(0, 255, 0)));
    assert_eq!(ancestor_ltr.background_color, Some(Color::new(0, 255, 0)));
    assert_eq!(explicit_rtl.background_color, Some(Color::new(0, 255, 0)));
    assert_eq!(ancestor_rtl.background_color, Some(Color::new(0, 255, 0)));
}

#[tokio::test]
async fn has_pseudo_class_matches_sibling_dir_from_selector_snapshot() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: black } p:has(+ p:dir(rtl)) { color: lime }",
    ));
    let siblings = vec![
        ElementSiblingSignature::new("p", HashMap::new()),
        ElementSiblingSignature::new("p", HashMap::new()).with_document_direction(Direction::Rtl),
    ];
    let style = style_for_element_with_signature(
        ElementSignature::with_siblings("p", HashMap::new(), 0, siblings),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 255, 0));
}

#[tokio::test]
async fn lang_pseudo_class_matches_basic_and_inherited_languages() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p:lang(en) { color: blue } section:lang(fr) p { color: red }",
    ));
    let english = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "en-US".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let parent = ComputedStyle {
        language: Some("fr".to_string()),
        ..ComputedStyle::initial()
    };
    let inherited = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[
            ElementSignature::new(
                "html",
                HashMap::from([("lang".to_string(), "fr".to_string())]),
            ),
            ElementSignature::new("section", HashMap::new()),
        ],
    );

    assert_eq!(english.color, Color::new(0, 0, 255));
    assert_eq!(inherited.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn lang_pseudo_class_matches_selectors_four_ranges() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"p:lang("*-FR") { color: red }
           p:lang(de, it) { background-color: blue }
           p:lang(*) { border-top-color: green }
           p:lang("de-*-DE") { border-bottom-color: green }
           p:lang("") { border-right-color: purple }"#,
    ));
    let regional = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "en-Latn-FR".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let italian = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "it-CH".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let unknown = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("lang".to_string(), "".to_string())])),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let wildcard_subtag = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "de-Latn-DE".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let singleton_extension = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "de-x-DE".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(regional.color, Color::new(255, 0, 0));
    assert_eq!(regional.border_colors.top, Color::new(0, 128, 0));
    assert_eq!(italian.background_color, Some(Color::new(0, 0, 255)));
    assert_eq!(unknown.border_colors.right, Color::new(128, 0, 128));
    assert_eq!(wildcard_subtag.border_colors.bottom, Color::new(0, 128, 0));
    assert_eq!(singleton_extension.border_colors.bottom, Color::BLACK);
}

#[tokio::test]
async fn lang_pseudo_class_matches_previous_sibling_language() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p:lang(fr) + span { color: red } p:lang(de) + span { color: blue }",
    ));
    let sibling_signatures = vec![
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "fr-CA".to_string())]),
        ),
        ElementSiblingSignature::new("span", HashMap::new()),
    ];
    let style = style_for_element_with_signature(
        ElementSignature::with_siblings("span", HashMap::new(), 1, sibling_signatures),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn nth_last_child_of_selector_list_counts_filtered_siblings_from_end() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { background: red } p:nth-last-child(even of .webkit, .fast) { background: lime }",
    ));
    let sibling_signatures = vec![
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "webkit".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "other".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "fast".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "webkit".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "fast".to_string())]),
        ),
    ];

    let first_filtered = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "p",
            HashMap::from([("class".to_string(), "webkit".to_string())]),
            0,
            sibling_signatures.clone(),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let second_filtered = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "p",
            HashMap::from([("class".to_string(), "fast".to_string())]),
            2,
            sibling_signatures.clone(),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let third_filtered = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "p",
            HashMap::from([("class".to_string(), "webkit".to_string())]),
            3,
            sibling_signatures,
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(first_filtered.background_color, Some(Color::new(0, 255, 0)));
    assert_eq!(
        second_filtered.background_color,
        Some(Color::new(255, 0, 0))
    );
    assert_eq!(third_filtered.background_color, Some(Color::new(0, 255, 0)));
}

#[tokio::test]
async fn nth_child_of_selector_list_counts_filtered_siblings_from_start() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: black } p:nth-child(odd of .webkit, .fast) { color: lime }",
    ));
    let sibling_signatures = vec![
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "webkit".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "other".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "fast".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "webkit".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "fast".to_string())]),
        ),
    ];

    let first_filtered = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "p",
            HashMap::from([("class".to_string(), "webkit".to_string())]),
            0,
            sibling_signatures.clone(),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let second_filtered = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "p",
            HashMap::from([("class".to_string(), "fast".to_string())]),
            2,
            sibling_signatures.clone(),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let third_filtered = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "p",
            HashMap::from([("class".to_string(), "webkit".to_string())]),
            3,
            sibling_signatures,
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(first_filtered.color, Color::new(0, 255, 0));
    assert_eq!(second_filtered.color, Color::BLACK);
    assert_eq!(third_filtered.color, Color::new(0, 255, 0));
}

#[tokio::test]
async fn nth_child_of_selector_list_accepts_supported_selector_forms() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: black }\
         p:nth-child(5 of article, .direct, #chosen, [data-match], body > p.combinator, :not(.excluded)) { color: lime }",
    ));
    let sibling_signatures = vec![
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "excluded".to_string())]),
        ),
        ElementSiblingSignature::new("article", HashMap::new()),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "direct".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("id".to_string(), "chosen".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "combinator".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("data-match".to_string(), "yes".to_string())]),
        ),
    ];
    let style = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "p",
            HashMap::from([("data-match".to_string(), "yes".to_string())]),
            5,
            sibling_signatures,
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[ElementSignature::new("body", HashMap::new())],
    );

    assert_eq!(style.color, Color::new(0, 255, 0));
}

#[tokio::test]
async fn nth_child_of_selector_list_accepts_no_space_after_of() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: black } p:nth-child(even of.fast) { color: lime }",
    ));
    let sibling_signatures = vec![
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "fast".to_string())]),
        ),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "fast".to_string())]),
        ),
    ];
    let style = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "p",
            HashMap::from([("class".to_string(), "fast".to_string())]),
            1,
            sibling_signatures,
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 255, 0));
}

#[tokio::test]
async fn invalid_nth_child_of_selector_list_rejects_rule_and_supports_selector() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: black }\
         p:nth-child(1 of ::before) { color: lime }\
         @supports selector(p:nth-child(1 of ::before)) { p { background: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::BLACK);
    assert_eq!(style.background_color, None);
}

#[tokio::test]
async fn nth_child_of_selector_list_adds_selector_argument_specificity() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p:nth-child(1 of #chosen, .fallback) { color: lime } #chosen { color: red }",
    ));
    let sibling_signatures = vec![
        ElementSiblingSignature::new("p", HashMap::new()),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("id".to_string(), "chosen".to_string())]),
        ),
    ];
    let style = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "p",
            HashMap::from([("id".to_string(), "chosen".to_string())]),
            1,
            sibling_signatures,
        ),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 255, 0));
}

#[tokio::test]
async fn empty_and_has_pseudo_classes_use_child_and_sibling_snapshots() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: black }\
         p:empty { color: lime }\
         p:has(> span.hit) { background-color: blue }\
         p:has(+ p.next) { border-top-color: red }",
    ));
    let empty = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new())
            .with_children(Vec::<ElementSiblingSignature>::new(), false),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let text_only = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new())
            .with_children(Vec::<ElementSiblingSignature>::new(), true),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = ElementSiblingSignature::new(
        "span",
        HashMap::from([("class".to_string(), "hit".to_string())]),
    );
    let siblings = vec![
        ElementSiblingSignature::new("p", HashMap::new()).with_children(vec![child.clone()], false),
        ElementSiblingSignature::new(
            "p",
            HashMap::from([("class".to_string(), "next".to_string())]),
        ),
    ];
    let with_child_and_next = style_for_element_with_signature(
        ElementSignature::with_siblings("p", HashMap::new(), 0, siblings)
            .with_children(vec![child], false),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(empty.color, Color::new(0, 255, 0));
    assert_eq!(text_only.color, Color::BLACK);
    assert_eq!(
        with_child_and_next.background_color,
        Some(Color::new(0, 0, 255))
    );
    assert_eq!(with_child_and_next.border_colors.top, Color::new(255, 0, 0));
}

#[tokio::test]
async fn static_and_html_state_pseudo_classes_parse_and_match_deterministically() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "input { color: black }\
         input:hover { color: red }\
         input:defined { background-color: blue }\
         input:disabled { border-top-color: red }\
         input:enabled { border-right-color: lime }\
         input:checked { border-bottom-color: blue }\
         input:required { color: lime }\
         input:optional { background-color: red }\
         input:read-only { outline-color: red }\
         input:read-write { outline-color: blue }",
    ));
    let checked_required = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([
                ("type".to_string(), "checkbox".to_string()),
                ("checked".to_string(), String::new()),
                ("required".to_string(), String::new()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let disabled = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([("disabled".to_string(), String::new())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let readonly = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([("readonly".to_string(), String::new())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let writable = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([("type".to_string(), "text".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(checked_required.color, Color::new(0, 255, 0));
    assert_eq!(
        checked_required.background_color,
        Some(Color::new(0, 0, 255))
    );
    assert_eq!(checked_required.border_colors.right, Color::new(0, 255, 0));
    assert_eq!(checked_required.border_colors.bottom, Color::new(0, 0, 255));
    assert_eq!(checked_required.outline_color, Color::new(255, 0, 0));
    assert_eq!(disabled.border_colors.top, Color::new(255, 0, 0));
    assert_eq!(disabled.border_colors.right, Color::BLACK);
    assert_eq!(readonly.outline_color, Color::new(255, 0, 0));
    assert_eq!(writable.outline_color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn open_pseudo_class_matches_static_html_open_state() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "details { color: red }\
         details:open { color: lime; margin-left: 5em }\
         div:open { border-top-color: red }\
         section:has(> details:open) { background-color: blue }",
    ));
    let open_attrs = HashMap::from([("open".to_string(), "true".to_string())]);
    let open_details = style_for_element_with_signature(
        ElementSignature::new("details", open_attrs.clone()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let closed_details = style_for_element_with_signature(
        ElementSignature::new("details", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let open_div = style_for_element_with_signature(
        ElementSignature::new("div", open_attrs.clone()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let section = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()).with_children(
            vec![ElementSiblingSignature::new("details", open_attrs)],
            false,
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(open_details.color, Color::new(0, 255, 0));
    assert_eq!(open_details.margin.left, 60.0);
    assert_eq!(closed_details.color, Color::new(255, 0, 0));
    assert_eq!(closed_details.margin.left, 0.0);
    assert_eq!(open_div.border_colors.top, Color::BLACK);
    assert_eq!(section.background_color, Some(Color::new(0, 0, 255)));
}

#[tokio::test]
async fn target_pseudo_classes_match_signature_target_state() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "section:target { color: lime }\
         section:target-within { background-color: blue }",
    ));
    let mut target_child = ElementSiblingSignature::new(
        "p",
        HashMap::from([("id".to_string(), "chapter".to_string())]),
    );
    target_child.is_target = true;
    let container = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()).with_children(vec![target_child], false),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let mut target_section = ElementSignature::new(
        "section",
        HashMap::from([("id".to_string(), "chapter".to_string())]),
    );
    target_section.is_target = true;
    let target = style_for_element_with_signature(
        target_section,
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(container.background_color, Some(Color::new(0, 0, 255)));
    assert_eq!(container.color, Color::BLACK);
    assert_eq!(target.color, Color::new(0, 255, 0));
    assert_eq!(target.background_color, Some(Color::new(0, 0, 255)));
}

#[tokio::test]
async fn namespace_selectors_match_namespaced_type_and_attribute_signatures() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@namespace html \"http://www.w3.org/1999/xhtml\";\
         @namespace svg \"http://www.w3.org/2000/svg\";\
         @namespace xlink \"http://www.w3.org/1999/xlink\";\
         html|p { color: lime }\
         svg|use[xlink|href] { color: blue }\
         svg|rect { background-color: red }",
    ));
    let mut paragraph = ElementSignature::new("p", HashMap::new());
    paragraph.namespace_url = "http://www.w3.org/1999/xhtml".to_string();
    let html_style = style_for_element_with_signature(
        paragraph,
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let mut use_element = ElementSignature::new(
        "use",
        HashMap::from([("href".to_string(), "#shape".to_string())]),
    );
    use_element.namespace_url = "http://www.w3.org/2000/svg".to_string();
    use_element.namespace_attrs = vec![ElementAttributeSignature::new(
        "http://www.w3.org/1999/xlink",
        "href",
        "#shape",
    )];
    let use_style = style_for_element_with_signature(
        use_element,
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let mut rect = ElementSignature::new("rect", HashMap::new());
    rect.namespace_url = "http://www.w3.org/1999/xhtml".to_string();
    let wrong_namespace_style =
        style_for_element_with_signature(rect, None, std::slice::from_ref(&stylesheet), None, &[]);

    assert_eq!(html_style.color, Color::new(0, 255, 0));
    assert_eq!(use_style.color, Color::new(0, 0, 255));
    assert_eq!(wrong_namespace_style.background_color, None);
}

#[tokio::test]
async fn xml_no_namespace_elements_do_not_match_xhtml_namespace_selectors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@namespace html \"http://www.w3.org/1999/xhtml\";\
         html|p { color: lime }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()).with_document_is_html(false),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(style.color, Color::BLACK);
}

#[tokio::test]
async fn xml_xhtml_namespace_elements_match_xhtml_namespace_selectors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@namespace html \"http://www.w3.org/1999/xhtml\";\
         html|p { color: lime }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new())
            .with_document_is_html(false)
            .with_namespace("http://www.w3.org/1999/xhtml", Vec::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 255, 0));
}

#[tokio::test]
async fn namespace_selectors_work_in_supports_conditions() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@namespace svg \"http://www.w3.org/2000/svg\";\
         @supports selector(svg|rect) { rect { color: lime } }\
         @supports selector(missing|rect) { rect { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("rect", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 255, 0));
    assert_eq!(style.background_color, None);
}

#[tokio::test]
async fn form_state_pseudo_classes_use_html_disabled_and_local_constraint_state() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "input:disabled, option:disabled { color: red }\
         input:enabled { border-top-color: lime }\
         input:placeholder-shown { border-right-color: blue }\
         input:invalid { border-bottom-color: red }\
         input:valid { outline-color: lime }\
         input:in-range { background-color: lime }\
         input:out-of-range { background-color: red }\
         input:default { text-decoration-line: underline }\
         input:unchecked { border-left-color: blue }\
         option:checked { outline-color: blue }",
    ));
    let input_siblings = vec![ElementSiblingSignature::new("input", HashMap::new())];
    let fieldset = ElementSignature::new(
        "fieldset",
        HashMap::from([("disabled".to_string(), String::new())]),
    )
    .with_children(input_siblings.clone(), false);
    let disabled_by_fieldset = style_for_element_with_signature(
        ElementSignature::with_siblings("input", HashMap::new(), 0, input_siblings),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[fieldset],
    );
    let first_legend = ElementSiblingSignature::new("legend", HashMap::new()).with_children(
        vec![ElementSiblingSignature::new("input", HashMap::new())],
        false,
    );
    let fieldset_children = vec![
        first_legend.clone(),
        ElementSiblingSignature::new("input", HashMap::new()),
    ];
    let fieldset_with_legend = ElementSignature::new(
        "fieldset",
        HashMap::from([("disabled".to_string(), String::new())]),
    )
    .with_children(fieldset_children.clone(), false);
    let legend = ElementSignature::with_siblings("legend", HashMap::new(), 0, fieldset_children)
        .with_child_list(first_legend.children, false);
    let enabled_inside_first_legend = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "input",
            HashMap::new(),
            0,
            vec![ElementSiblingSignature::new("input", HashMap::new())],
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[fieldset_with_legend, legend],
    );
    let disabled_option = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "option",
            HashMap::new(),
            0,
            vec![ElementSiblingSignature::new("option", HashMap::new())],
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[ElementSignature::new(
            "optgroup",
            HashMap::from([("disabled".to_string(), String::new())]),
        )],
    );
    let invalid_required = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([
                ("required".to_string(), String::new()),
                ("placeholder".to_string(), "Name".to_string()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let valid_in_range = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([
                ("type".to_string(), "number".to_string()),
                ("value".to_string(), "5".to_string()),
                ("min".to_string(), "1".to_string()),
                ("max".to_string(), "10".to_string()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let checked_default = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([
                ("type".to_string(), "checkbox".to_string()),
                ("checked".to_string(), String::new()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let unchecked = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([("type".to_string(), "checkbox".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let invalid_email = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([
                ("type".to_string(), "email".to_string()),
                ("value".to_string(), "bad".to_string()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let invalid_url = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([
                ("type".to_string(), "url".to_string()),
                ("value".to_string(), "not-a-url".to_string()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let invalid_length = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([
                ("value".to_string(), "abc".to_string()),
                ("minlength".to_string(), "4".to_string()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let invalid_step = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([
                ("type".to_string(), "number".to_string()),
                ("value".to_string(), "3".to_string()),
                ("min".to_string(), "0".to_string()),
                ("step".to_string(), "2".to_string()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let fallback_selected_option = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "option",
            HashMap::new(),
            0,
            vec![
                ElementSiblingSignature::new("option", HashMap::new()),
                ElementSiblingSignature::new("option", HashMap::new()),
            ],
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(disabled_by_fieldset.color, Color::new(255, 0, 0));
    assert_eq!(
        enabled_inside_first_legend.border_colors.top,
        Color::new(0, 255, 0)
    );
    assert_eq!(disabled_option.color, Color::new(255, 0, 0));
    assert_eq!(invalid_required.border_colors.right, Color::new(0, 0, 255));
    assert_eq!(invalid_required.border_colors.bottom, Color::new(255, 0, 0));
    assert_eq!(valid_in_range.outline_color, Color::new(0, 255, 0));
    assert_eq!(valid_in_range.background_color, Some(Color::new(0, 255, 0)));
    assert!(checked_default.text_decoration.underline);
    assert_eq!(unchecked.border_colors.left, Color::new(0, 0, 255));
    assert_eq!(invalid_email.border_colors.bottom, Color::new(255, 0, 0));
    assert_eq!(invalid_url.border_colors.bottom, Color::new(255, 0, 0));
    assert_eq!(invalid_length.border_colors.bottom, Color::new(255, 0, 0));
    assert_eq!(invalid_step.border_colors.bottom, Color::new(255, 0, 0));
    assert_eq!(
        fallback_selected_option.outline_color,
        Color::new(0, 0, 255)
    );
}

#[tokio::test]
async fn typographic_pseudo_element_rules_create_computed_style_slots() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p::first-line { color: red; display: none }\
         p::first-letter { color: blue; display: none; margin-left: 10px }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(
        style.first_line_style.as_ref().unwrap().color,
        Color::new(255, 0, 0)
    );
    assert_eq!(
        style.first_letter_style.as_ref().unwrap().color,
        Color::new(0, 0, 255)
    );
    assert_eq!(
        style.first_line_style.as_ref().unwrap().display,
        style.display
    );
    assert_eq!(
        style.first_letter_style.as_ref().unwrap().display,
        style.display
    );
    assert_eq!(style.first_letter_style.as_ref().unwrap().margin.left, 7.5);
}

#[tokio::test]
async fn generated_pseudo_style_inherits_without_cloning_non_inherited_properties() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"p {
             --accent: blue;
             color: red;
             font-size: 18pt;
             margin-left: 20pt;
             background-color: green;
             position: absolute;
           }
           p::before { content: "x" }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    let before = style.before_style.as_ref().expect("before style");

    assert_eq!(before.color, Color::new(255, 0, 0));
    assert_eq!(before.font_size, 18.0);
    assert_eq!(
        before.custom_properties.get("--accent").map(String::as_str),
        Some("blue")
    );
    assert_eq!(before.margin.left, 0.0);
    assert_eq!(before.background_color, None);
    assert_eq!(before.position, Position::Static);
}

#[tokio::test]
async fn marker_style_inherits_without_cloning_non_inherited_properties() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"li {
             color: red;
             font-size: 18pt;
             margin-left: 20pt;
             background-color: green;
             position: absolute;
           }
           li::marker { content: "x" }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("li", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    let marker = style.marker_style.as_ref().expect("marker style");

    assert_eq!(marker.color, Color::new(255, 0, 0));
    assert_eq!(marker.font_size, 18.0);
    assert_eq!(marker.margin.left, 0.0);
    assert_eq!(marker.background_color, None);
    assert_eq!(marker.position, Position::Static);
}

#[tokio::test]
async fn media_and_requires_all_features_to_match() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@media print and (overflow-block: paged) { p { color: red } }\
         @media print and (unsupported-feature: value) { p { color: blue } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn media_not_print_page_rules_do_not_affect_page_context() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@media not print { @page { margin: 4in } } @page { margin: 1in }",
    ));

    let margins = page_margins_from(&stylesheet.page_declarations, PageMargins::all_points(0.0));

    assert_eq!(margins, PageMargins::all_points(72.0));
}

#[tokio::test]
async fn parses_forced_page_side_break_values() {
    let declarations =
        parse_declarations("break-before: right; break-after: verso; page-break-before: left");
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.break_before, PageBreak::Left);
    assert_eq!(style.break_after, PageBreak::Verso);
}

#[tokio::test]
async fn parses_avoid_page_break_values() {
    let declarations =
        parse_declarations("break-before: avoid; break-after: avoid-page; page-break-after: avoid");
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.break_before, PageBreak::Avoid);
    assert_eq!(style.break_after, PageBreak::Avoid);
}

#[tokio::test]
async fn parses_font_face_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@font-face { font-family: ReportFont; src: url(\"fonts/report.ttf\") format(\"truetype\"); font-weight: 650; font-style: italic; font-stretch: condensed }",
    ));

    assert_eq!(stylesheet.font_faces.len(), 1);
    assert_eq!(stylesheet.font_faces[0].family, "ReportFont");
    assert_eq!(stylesheet.font_faces[0].weight, FontWeight(650));
    assert_eq!(stylesheet.font_faces[0].style, FontStyle::Italic);
    assert_eq!(stylesheet.font_faces[0].width, FontWidth::CONDENSED);
    assert_eq!(
        stylesheet.font_faces[0].sources,
        vec![FontFaceSource::Url {
            value: "fonts/report.ttf".to_string(),
            base_url: None,
            root_url: None
        }]
    );
}

#[tokio::test]
async fn css_url_tokens_handle_quoted_parentheses() {
    let declarations = parse_declarations(
        "background-image: url(\"images/report ) cover.png\"); list-style-image: url('markers/a)b.png')",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.background_image,
        Some(BackgroundImage::Url { ref src, .. }) if src == "images/report ) cover.png"
    ));
    assert_eq!(style.list_style_image.as_deref(), Some("markers/a)b.png"));

    let stylesheet = parse_stylesheet(&Css::from_string(
        "@font-face { font-family: ReportFont; src: url(\"fonts/report ) final.ttf\") }",
    ));
    assert_eq!(
        stylesheet.font_faces[0].sources,
        vec![FontFaceSource::Url {
            value: "fonts/report ) final.ttf".to_string(),
            base_url: None,
            root_url: None
        }]
    );
}

#[tokio::test]
async fn parses_axis_aligned_linear_gradient_background_image() {
    let declarations = parse_declarations(
        "background: linear-gradient(to bottom, red 0, red 50px, green 50px, green 100px)",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(gradient)) = style.background_image else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(gradient.direction, LinearGradientDirection::Angle(180.0));
    assert!(!gradient.repeating);
    assert_eq!(gradient.stops.len(), 4);
    assert_eq!(gradient.stops[0].color, Color::new(255, 0, 0));
    assert_eq!(gradient.stops[0].position.unwrap().length_points(), 0.0);
    assert_eq!(gradient.stops[1].position.unwrap().length_points(), 37.5);
    assert_eq!(gradient.stops[2].color, Color::new(0, 128, 0));
    assert_eq!(gradient.stops[2].position.unwrap().length_points(), 37.5);
    assert_eq!(gradient.stops[3].position.unwrap().length_points(), 75.0);
}

#[tokio::test]
async fn parses_angle_and_corner_linear_gradient_directions() {
    let declarations = parse_declarations(
        "background-image: linear-gradient(.5turn, red, blue), linear-gradient(to top right, red, blue)",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(first)) = &style.background_layers[0].image else {
        panic!("expected first linear gradient background image");
    };
    assert_eq!(first.direction, LinearGradientDirection::Angle(180.0));
    assert_eq!(first.stops[0].position, None);
    assert_eq!(first.stops[1].position, None);

    let Some(BackgroundImage::LinearGradient(second)) = &style.background_layers[1].image else {
        panic!("expected second linear gradient background image");
    };
    assert_eq!(
        second.direction,
        LinearGradientDirection::Corner {
            horizontal: GradientHorizontalDirection::Right,
            vertical: GradientVerticalDirection::Top,
        }
    );
}

#[tokio::test]
async fn parses_repeating_linear_gradient_stops_and_hints() {
    let declarations =
        parse_declarations("background: repeating-linear-gradient(0, red, 25%, blue 50% 75%)");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(gradient)) = style.background_image else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(gradient.direction, LinearGradientDirection::Angle(0.0));
    assert!(gradient.repeating);
    assert_eq!(gradient.stops.len(), 3);
    assert_eq!(gradient.stops[0].position, None);
    assert_eq!(gradient.stops[1].position.unwrap().percent, 0.5);
    assert_eq!(gradient.stops[2].position.unwrap().percent, 0.75);
    assert_eq!(gradient.hints.len(), 1);
    assert_eq!(gradient.hints[0].after_stop, 0);
    assert_eq!(gradient.hints[0].position.percent, 0.25);
}

#[tokio::test]
async fn parses_radial_gradient_shape_size_position_and_stops() {
    let declarations = parse_declarations(
        "background-image: radial-gradient(circle closest-side at 25% 75%, red, 30%, blue 100%)",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::RadialGradient(gradient)) = style.background_image else {
        panic!("expected radial gradient background image");
    };
    assert_eq!(gradient.shape, RadialGradientShape::Circle);
    assert_eq!(
        gradient.size,
        RadialGradientSize::Extent(RadialGradientExtent::ClosestSide)
    );
    assert_eq!(gradient.position.x.offset.percent, 0.25);
    assert_eq!(gradient.position.y.offset.percent, 0.75);
    assert!(!gradient.repeating);
    assert_eq!(gradient.stops.len(), 2);
    assert_eq!(gradient.stops[0].color, Color::new(255, 0, 0));
    assert_eq!(gradient.stops[1].color, Color::new(0, 0, 255));
    assert_eq!(gradient.stops[1].position.unwrap().percent, 1.0);
    assert_eq!(gradient.hints.len(), 1);
    assert_eq!(gradient.hints[0].after_stop, 0);
    assert_eq!(gradient.hints[0].position.percent, 0.3);
}

#[tokio::test]
async fn parses_repeating_radial_gradient_explicit_radii() {
    let declarations = parse_declarations(
        "background-image: repeating-radial-gradient(10pt 20pt at center, red 0pt, red 4pt, blue 4pt, blue 8pt)",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::RadialGradient(gradient)) = style.background_image else {
        panic!("expected radial gradient background image");
    };
    assert_eq!(gradient.shape, RadialGradientShape::Ellipse);
    assert_eq!(
        gradient.size,
        RadialGradientSize::EllipseRadii {
            x: ComputedLengthPercentage::from_points(10.0),
            y: ComputedLengthPercentage::from_points(20.0)
        }
    );
    assert!(gradient.repeating);
    assert_eq!(gradient.stops.len(), 4);
}

#[tokio::test]
async fn parses_linear_gradient_angle_units() {
    let declarations = parse_declarations(
        "background-image: linear-gradient(100grad, red, blue),\
         linear-gradient(3.1415927rad, red, blue)",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(first)) = &style.background_layers[0].image else {
        panic!("expected first gradient");
    };
    assert!((gradient_angle(first) - 90.0).abs() < 0.001);

    let Some(BackgroundImage::LinearGradient(second)) = &style.background_layers[1].image else {
        panic!("expected second gradient");
    };
    assert!((gradient_angle(second) - 180.0).abs() < 0.001);
}

fn gradient_angle(gradient: &LinearGradient) -> f32 {
    let LinearGradientDirection::Angle(angle) = gradient.direction else {
        panic!("expected angle direction");
    };
    angle
}

#[tokio::test]
async fn parses_linear_gradient_percentage_color_stops() {
    let declarations =
        parse_declarations("background: linear-gradient(to bottom, red 50%, green 50%)");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(gradient)) = style.background_image else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(gradient.stops.len(), 2);
    assert_eq!(gradient.stops[0].color, Color::new(255, 0, 0));
    assert_eq!(gradient.stops[0].position.unwrap().percent, 0.5);
    assert_eq!(gradient.stops[1].color, Color::new(0, 128, 0));
    assert_eq!(gradient.stops[1].position.unwrap().percent, 0.5);
}

#[tokio::test]
async fn ch_linear_gradient_color_stops_resolve_before_paint() {
    let declarations =
        parse_declarations("background: linear-gradient(to right, red 2ch, blue 10vw)");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(gradient)) = &style.background_image else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(
        gradient.stops[0].position,
        Some(ComputedLengthPercentage::from_ch(2.0))
    );

    style.resolve_font_metric_lengths(6.0);

    let Some(BackgroundImage::LinearGradient(gradient)) = &style.background_image else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(
        gradient.stops[0].position,
        Some(ComputedLengthPercentage::from_points(12.0))
    );
    let Some(BackgroundImage::LinearGradient(layer_gradient)) = &style.background_layers[0].image
    else {
        panic!("expected linear gradient background layer");
    };
    assert_eq!(
        layer_gradient.stops[0].position,
        Some(ComputedLengthPercentage::from_points(12.0))
    );

    style.resolve_viewport_lengths(200.0, 100.0);

    let Some(BackgroundImage::LinearGradient(gradient)) = &style.background_image else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(
        gradient.stops[1].position,
        Some(ComputedLengthPercentage::from_points(20.0))
    );
}

#[tokio::test]
async fn parses_background_origin_and_clip_boxes() {
    let declarations =
        parse_declarations("background-origin: content-box; background-clip: padding-box");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.background_origin, BackgroundBox::Content);
    assert_eq!(style.background_clip, BackgroundBox::Padding);
}

#[tokio::test]
async fn parses_comma_separated_background_layers() {
    let declarations = parse_declarations(
        "background-image: url(top.png), url(bottom.png);\
         background-size: 10pt 20pt, 5pt 6pt;\
         background-position: left top, right bottom;\
         background-repeat: no-repeat, repeat-y;\
         background-origin: content-box, border-box;\
         background-clip: padding-box, content-box",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.background_layers.len(), 2);
    assert!(matches!(
        style.background_layers[0].image,
        Some(BackgroundImage::Url { ref src, .. }) if src == "top.png"
    ));
    assert!(matches!(
        style.background_layers[1].image,
        Some(BackgroundImage::Url { ref src, .. }) if src == "bottom.png"
    ));
    assert_eq!(
        style.background_layers[0].repeat,
        BackgroundRepeat::NoRepeat
    );
    assert_eq!(style.background_layers[1].repeat, BackgroundRepeat::RepeatY);
    assert_eq!(style.background_layers[0].origin, BackgroundBox::Content);
    assert_eq!(style.background_layers[0].clip, BackgroundBox::Padding);
    assert_eq!(style.background_layers[1].origin, BackgroundBox::Border);
    assert_eq!(style.background_layers[1].clip, BackgroundBox::Content);
}

#[tokio::test]
async fn background_shorthand_sets_origin_then_clip_boxes() {
    let declarations = parse_declarations("background: red content-box border-box");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.background_color, Some(Color::new(255, 0, 0)));
    assert_eq!(style.background_origin, BackgroundBox::Content);
    assert_eq!(style.background_clip, BackgroundBox::Border);
}

#[tokio::test]
async fn parses_background_repeat_aliases_and_two_axis_values() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("background-repeat: repeat-x"),
    );
    assert_eq!(style.background_repeat, BackgroundRepeat::RepeatX);

    apply_declarations(
        &mut style,
        &parse_declarations("background-repeat: repeat-y"),
    );
    assert_eq!(style.background_repeat, BackgroundRepeat::RepeatY);

    apply_declarations(
        &mut style,
        &parse_declarations("background-repeat: no-repeat repeat"),
    );
    assert_eq!(style.background_repeat, BackgroundRepeat::RepeatY);

    apply_declarations(
        &mut style,
        &parse_declarations("background: url(bg.png) repeat no-repeat"),
    );
    assert_eq!(style.background_repeat, BackgroundRepeat::RepeatX);
}

#[tokio::test]
async fn trims_important_without_slicing_unicode_values() {
    let declarations = parse_declarations("content: \"•\" !important");
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![GeneratedContentPart::Text("•".to_string())],
            alt: None,
        }
    );
}

#[tokio::test]
async fn decodes_css_string_escapes_in_generated_content() {
    let declarations = parse_declarations(r#"content: "\0099""#);
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![GeneratedContentPart::Text("\u{0099}".to_string())],
            alt: None,
        }
    );
}

#[tokio::test]
async fn parses_mixed_generated_content_parts() {
    let declarations = parse_declarations(
        r#"content: "Chapter " counter(chapter, upper-roman) ": " attr(data-title) attr(data-missing, "Fallback") url(icon.png)"#,
    );
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![
                GeneratedContentPart::Text("Chapter ".to_string()),
                GeneratedContentPart::Counter {
                    name: "chapter".to_string(),
                    style: Some(ListStyleType::UpperRoman),
                },
                GeneratedContentPart::Text(": ".to_string()),
                GeneratedContentPart::Attr {
                    name: "data-title".to_string(),
                    fallback: None,
                },
                GeneratedContentPart::Attr {
                    name: "data-missing".to_string(),
                    fallback: Some("Fallback".to_string()),
                },
                GeneratedContentPart::Image {
                    image: BackgroundImage::Url {
                        src: "icon.png".to_string(),
                        base_url: None,
                        root_url: None,
                    },
                },
            ],
            alt: None,
        }
    );
}

#[tokio::test]
async fn parses_generated_content_gradient_images() {
    let declarations = parse_declarations(
        "content: linear-gradient(red, blue) radial-gradient(circle, white, black)",
    );
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    let Content::List { parts, .. } = style.content else {
        panic!("expected generated content list");
    };
    assert_eq!(parts.len(), 2);
    assert!(matches!(
        parts[0],
        GeneratedContentPart::Image {
            image: BackgroundImage::LinearGradient(_)
        }
    ));
    assert!(matches!(
        parts[1],
        GeneratedContentPart::Image {
            image: BackgroundImage::RadialGradient(_)
        }
    ));
}

#[tokio::test]
async fn parses_generated_content_target_references() {
    let declarations = parse_declarations(
        r##"content: "See " target-counter(url("#chapter"), page, lower-roman) " " target-text("#chapter", after)"##,
    );
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![
                GeneratedContentPart::Text("See ".to_string()),
                GeneratedContentPart::TargetCounter {
                    target: "#chapter".to_string(),
                    name: "page".to_string(),
                    style: Some(ListStyleType::LowerRoman),
                },
                GeneratedContentPart::Text(" ".to_string()),
                GeneratedContentPart::TargetText {
                    target: "#chapter".to_string(),
                    keyword: NamedStringTargetTextKeyword::After,
                },
            ],
            alt: None,
        }
    );
}

#[tokio::test]
async fn invalid_generated_content_keeps_previous_value() {
    let mut style = ComputedStyle::initial();
    apply_declarations(&mut style, &parse_declarations(r#"content: "previous""#));
    apply_declarations(
        &mut style,
        &parse_declarations("content: attr(data-title, 1px)"),
    );

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![GeneratedContentPart::Text("previous".to_string())],
            alt: None,
        }
    );
}

#[tokio::test]
async fn parses_core_css_content_features() {
    let declarations = parse_declarations(
        r#"content: open-quote "Chapter " leader(dotted) counter(chapter) close-quote / "Chapter " attr(data-title)"#,
    );
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![
                GeneratedContentPart::Quote(GeneratedQuote::Open),
                GeneratedContentPart::Text("Chapter ".to_string()),
                GeneratedContentPart::Leader(".".to_string()),
                GeneratedContentPart::Counter {
                    name: "chapter".to_string(),
                    style: None,
                },
                GeneratedContentPart::Quote(GeneratedQuote::Close),
            ],
            alt: Some(vec![
                GeneratedAltTextPart::Text("Chapter ".to_string()),
                GeneratedAltTextPart::Attr {
                    name: "data-title".to_string(),
                    fallback: None,
                },
            ]),
        }
    );
}

#[tokio::test]
async fn parses_content_replacement_and_quotes_property() {
    let mut style = ComputedStyle::initial();
    apply_declarations(
        &mut style,
        &parse_declarations(r#"content: url(icon.png) / "Icon"; quotes: "«" "»" "“" "”""#),
    );

    assert_eq!(
        style.content,
        Content::Replacement {
            image: GeneratedContentPart::Image {
                image: BackgroundImage::Url {
                    src: "icon.png".to_string(),
                    base_url: None,
                    root_url: None,
                },
            },
            alt: Some(vec![GeneratedAltTextPart::Text("Icon".to_string())]),
        }
    );
    assert_eq!(
        style.quotes,
        Quotes::Pairs(vec![
            ("«".to_string(), "»".to_string()),
            ("“".to_string(), "”".to_string()),
        ])
    );
}

#[tokio::test]
async fn auto_quotes_resolve_from_parent_language() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("lang".to_string(), "en".to_string())])),
        None,
        &[],
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("q", HashMap::from([("lang".to_string(), "ja".to_string())])),
        None,
        &[],
        Some(&parent),
        &[ElementSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "en".to_string())]),
        )],
    );

    assert_eq!(child.language.as_deref(), Some("ja"));
    assert_eq!(child.quotes.auto_language(), Some("en"));
}

#[tokio::test]
async fn match_parent_preserves_resolved_auto_quote_language() {
    let outer = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("lang".to_string(), "en".to_string())])),
        None,
        &[],
        None,
        &[],
    );
    let quoted = style_for_element_with_signature(
        ElementSignature::new("q", HashMap::from([("lang".to_string(), "ja".to_string())])),
        None,
        &[],
        Some(&outer),
        &[ElementSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "en".to_string())]),
        )],
    );
    let match_parent = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("quotes: match-parent"),
        &[],
        Some(&quoted),
        &[
            ElementSignature::new("p", HashMap::from([("lang".to_string(), "en".to_string())])),
            ElementSignature::new("q", HashMap::from([("lang".to_string(), "ja".to_string())])),
        ],
    );
    let auto = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("quotes: auto"),
        &[],
        Some(&quoted),
        &[
            ElementSignature::new("p", HashMap::from([("lang".to_string(), "en".to_string())])),
            ElementSignature::new("q", HashMap::from([("lang".to_string(), "ja".to_string())])),
        ],
    );

    assert_eq!(quoted.language.as_deref(), Some("ja"));
    assert_eq!(quoted.quotes.auto_language(), Some("en"));
    assert_eq!(match_parent.quotes.auto_language(), Some("en"));
    assert_eq!(auto.quotes.auto_language(), Some("ja"));
}

#[tokio::test]
async fn parses_css_fonts_matching_axes() {
    let declarations = parse_declarations(
        "font-weight: 350.4; font-style: oblique 12deg; font-width: 87.5%; font-stretch: expanded",
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_weight, FontWeight(350));
    assert_eq!(style.font_style, FontStyle::Oblique);
    assert_eq!(style.font_width, FontWidth::EXPANDED);
}

#[tokio::test]
async fn parses_css_font_feature_controls() {
    let declarations = parse_declarations(
        r#"font-feature-settings: "kern" off, "liga" 2, "kern" on;
           font-kerning: none;
           font-variant-ligatures: no-common-ligatures discretionary-ligatures contextual;
           font-variant-position: super;
           font-variant-caps: all-small-caps;
           font-variant-numeric: oldstyle-nums tabular-nums diagonal-fractions ordinal slashed-zero;
           font-variant-alternates: historical-forms stylistic(alt-a) styleset(alt-b alt-c) character-variant(cv-a) swash(sw-a) ornaments(orn-a) annotation(ann-a);
           font-variant-east-asian: jis90 proportional-width ruby;
           font-variant-emoji: text"#,
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.font_feature_settings,
        FontFeatureSettings(vec![
            FontFeatureSetting::new(*b"kern", 1),
            FontFeatureSetting::new(*b"liga", 2),
        ])
    );
    assert_eq!(style.font_kerning, FontKerning::None);
    assert_eq!(
        style.font_variant_ligatures,
        FontVariantLigatures::Values {
            common: Some(false),
            discretionary: Some(true),
            historical: None,
            contextual: Some(true),
        }
    );
    assert_eq!(style.font_variant_position, FontVariantPosition::Super);
    assert_eq!(style.font_variant_caps, FontVariantCaps::AllSmallCaps);
    assert_eq!(
        style.font_variant_numeric,
        FontVariantNumeric::Values(vec![
            FontVariantNumericValue::OldstyleNums,
            FontVariantNumericValue::TabularNums,
            FontVariantNumericValue::DiagonalFractions,
            FontVariantNumericValue::Ordinal,
            FontVariantNumericValue::SlashedZero,
        ])
    );
    assert_eq!(
        style.font_variant_alternates,
        FontVariantAlternates::Values {
            historical_forms: true,
            stylistic: vec!["alt-a".to_string()],
            styleset: vec!["alt-b".to_string(), "alt-c".to_string()],
            character_variant: vec!["cv-a".to_string()],
            swash: vec!["sw-a".to_string()],
            ornaments: vec!["orn-a".to_string()],
            annotation: vec!["ann-a".to_string()],
        }
    );
    assert_eq!(
        style.font_variant_east_asian,
        FontVariantEastAsian::Values(vec![
            FontVariantEastAsianValue::Jis90,
            FontVariantEastAsianValue::ProportionalWidth,
            FontVariantEastAsianValue::Ruby,
        ])
    );
    assert_eq!(style.font_variant_emoji, FontVariantEmoji::Text);
}

#[tokio::test]
async fn invalid_css_font_feature_controls_are_ignored() {
    let declarations = parse_declarations(
        r#"font-feature-settings: "kern" off;
           font-feature-settings: "toolong" on;
           font-variant-numeric: tabular-nums;
           font-variant-numeric: tabular-nums proportional-nums;
           font-variant-east-asian: jis78;
           font-variant-east-asian: jis78 jis90;
           font-variant-ligatures: contextual;
           font-variant-ligatures: contextual no-contextual;
           font-variant-emoji: emoji;
           font-variant-emoji: colorful"#,
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.font_feature_settings,
        FontFeatureSettings(vec![FontFeatureSetting::new(*b"kern", 0)])
    );
    assert_eq!(
        style.font_variant_numeric,
        FontVariantNumeric::Values(vec![FontVariantNumericValue::TabularNums])
    );
    assert_eq!(
        style.font_variant_east_asian,
        FontVariantEastAsian::Values(vec![FontVariantEastAsianValue::Jis78])
    );
    assert_eq!(
        style.font_variant_ligatures,
        FontVariantLigatures::Values {
            common: None,
            discretionary: None,
            historical: None,
            contextual: Some(true),
        }
    );
    assert_eq!(style.font_variant_emoji, FontVariantEmoji::Emoji);
}

#[tokio::test]
async fn parses_font_size_adjust_values() {
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &parse_declarations("font-size-adjust: 0.9"));
    assert_eq!(
        style.font_size_adjust,
        FontSizeAdjust::Value {
            metric: FontSizeAdjustMetric::ExHeight,
            value: FontSizeAdjustValue::Number(0.9),
        }
    );

    apply_declarations(
        &mut style,
        &parse_declarations("font-size-adjust: cap-height 0.72"),
    );
    assert_eq!(
        style.font_size_adjust,
        FontSizeAdjust::Value {
            metric: FontSizeAdjustMetric::CapHeight,
            value: FontSizeAdjustValue::Number(0.72),
        }
    );

    apply_declarations(
        &mut style,
        &parse_declarations("font-size-adjust: ch-width from-font"),
    );
    assert_eq!(
        style.font_size_adjust,
        FontSizeAdjust::Value {
            metric: FontSizeAdjustMetric::ChWidth,
            value: FontSizeAdjustValue::FromFont,
        }
    );

    apply_declarations(&mut style, &parse_declarations("font-size-adjust: none"));
    assert_eq!(style.font_size_adjust, FontSizeAdjust::None);
}

#[tokio::test]
async fn invalid_font_size_adjust_values_are_ignored() {
    let declarations = parse_declarations(
        "font-size-adjust: 0.9;
         font-size-adjust: -1;
         font-size-adjust: cap-height;
         font-size-adjust: unknown 1;
         font-size-adjust: ex-height cap-height 1;
         font-size-adjust: from-font 0.9",
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.font_size_adjust,
        FontSizeAdjust::Value {
            metric: FontSizeAdjustMetric::ExHeight,
            value: FontSizeAdjustValue::Number(0.9),
        }
    );
}

#[tokio::test]
async fn font_size_adjust_inherits_and_resets_from_font_shorthand() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        Some("font-size-adjust: 0.8"),
        &[],
        None,
        &[],
    );

    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("font-size-adjust: 0.9; font: small-caps 10px Ahem"),
        &[],
        Some(&parent),
        &[ElementSignature::new("p", HashMap::new())],
    );

    assert_eq!(child.font_size_adjust, FontSizeAdjust::None);

    let inherited = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("font-size-adjust: inherit"),
        &[],
        Some(&parent),
        &[ElementSignature::new("p", HashMap::new())],
    );
    assert_eq!(inherited.font_size_adjust, parent.font_size_adjust);
}

#[tokio::test]
async fn font_size_adjust_applies_to_marker_and_generated_pseudo_styles() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "li { font-size-adjust: 0.8 }
         li::marker { font-size-adjust: cap-height 0.7 }
         li::before { content: \"\"; font-size-adjust: from-font }",
    ));

    let style = style_for_element_with_signature(
        ElementSignature::new("li", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(
        style.font_size_adjust,
        FontSizeAdjust::Value {
            metric: FontSizeAdjustMetric::ExHeight,
            value: FontSizeAdjustValue::Number(0.8),
        }
    );
    assert_eq!(
        style
            .marker_style
            .as_ref()
            .expect("marker style")
            .font_size_adjust,
        FontSizeAdjust::Value {
            metric: FontSizeAdjustMetric::CapHeight,
            value: FontSizeAdjustValue::Number(0.7),
        }
    );
    assert_eq!(
        style
            .before_style
            .as_ref()
            .expect("before style")
            .font_size_adjust,
        FontSizeAdjust::Value {
            metric: FontSizeAdjustMetric::ExHeight,
            value: FontSizeAdjustValue::FromFont,
        }
    );
}

#[tokio::test]
async fn font_variant_shorthand_resets_omitted_subproperties() {
    let declarations = parse_declarations(
        "font-variant-numeric: tabular-nums; font-variant-emoji: emoji; font-variant: historical-forms small-caps",
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_variant_numeric, FontVariantNumeric::Normal);
    assert_eq!(style.font_variant_caps, FontVariantCaps::SmallCaps);
    assert_eq!(
        style.font_variant_alternates,
        FontVariantAlternates::historical_forms()
    );
    assert_eq!(style.font_variant_emoji, FontVariantEmoji::Normal);
}

#[tokio::test]
async fn font_shorthand_accepts_small_caps_and_resets_variant_subproperties() {
    let declarations = parse_declarations(
        "font-variant-numeric: tabular-nums; font: small-caps italic 700 condensed 10px/1 Ahem",
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_variant_numeric, FontVariantNumeric::Normal);
    assert_eq!(style.font_variant_caps, FontVariantCaps::SmallCaps);
    assert_eq!(style.font_style, FontStyle::Italic);
    assert_eq!(style.font_weight, FontWeight::BOLD);
    assert_eq!(style.font_width, FontWidth::CONDENSED);
}

#[tokio::test]
async fn parses_font_face_descriptors_and_font_feature_values() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"
        @font-face {
            font-family: Feature Face;
            src: url(feature.otf);
            font-feature-settings: "kern" off, "liga";
            font-variant: small-caps oldstyle-nums historical-forms;
        }
        @font-feature-values "Feature Face" {
            @styleset { alt-one: 1; alt-two: 2 }
            @character-variant { badge: 12 3; invalid: 100 }
            @annotation { circle: 1 }
            @unknown { skipped: 1 }
        }
        "#,
    ));

    let face = stylesheet.font_faces.first().expect("font face");
    assert_eq!(face.family, "Feature Face");
    assert_eq!(face.unicode_range, None);
    assert_eq!(
        face.font_feature_settings,
        FontFeatureSettings(vec![
            FontFeatureSetting::new(*b"kern", 0),
            FontFeatureSetting::new(*b"liga", 1),
        ])
    );
    assert_eq!(face.font_variant_caps, FontVariantCaps::SmallCaps);
    assert_eq!(
        face.font_variant_numeric,
        FontVariantNumeric::Values(vec![FontVariantNumericValue::OldstyleNums])
    );
    assert_eq!(
        face.font_variant_alternates,
        FontVariantAlternates::historical_forms()
    );
    assert_eq!(
        stylesheet
            .font_feature_values
            .get("Feature Face", FontFeatureValuesBlock::Styleset, "alt-two")
            .map(|value| value.feature_index),
        Some(2)
    );
    assert_eq!(
        stylesheet
            .font_feature_values
            .get(
                "feature face",
                FontFeatureValuesBlock::CharacterVariant,
                "badge"
            )
            .and_then(|value| value.selector),
        Some(3)
    );
    assert!(
        stylesheet
            .font_feature_values
            .get(
                "Feature Face",
                FontFeatureValuesBlock::CharacterVariant,
                "invalid"
            )
            .is_none()
    );
}

#[tokio::test]
async fn parses_font_face_unicode_range_descriptor() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"
        @font-face {
            font-family: Ranged;
            src: url(ranged.otf);
            unicode-range: U+20, U+0640, U+200C-200D, U+4??;
        }
        "#,
    ));

    let face = stylesheet.font_faces.first().expect("font face");
    let ranges = face.unicode_range.as_deref().expect("unicode range");
    assert_eq!(ranges.len(), 4);
    assert!(ranges[0].contains(' '));
    assert!(!ranges[0].contains('A'));
    assert!(ranges[1].contains('\u{0640}'));
    assert!(ranges[2].contains('\u{200c}'));
    assert!(ranges[2].contains('\u{200d}'));
    assert!(ranges[3].contains('\u{04ff}'));
    assert!(!ranges[3].contains('\u{0500}'));
}

#[tokio::test]
async fn font_feature_controls_inherit_and_apply_to_marker_styles() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"p { font-feature-settings: "kern" off; font-variant-numeric: tabular-nums }
           p span { font-feature-settings: inherit; font-variant-numeric: inherit }
           li::marker { font-feature-settings: "liga" off; font-variant-numeric: tabular-nums }"#,
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[ElementSignature::new("p", HashMap::new())],
    );
    let list_item = style_for_element_with_signature(
        ElementSignature::new("li", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    let marker = list_item.marker_style.as_ref().expect("marker style");

    assert_eq!(
        child.font_feature_settings,
        FontFeatureSettings(vec![FontFeatureSetting::new(*b"kern", 0)])
    );
    assert_eq!(
        child.font_variant_numeric,
        FontVariantNumeric::Values(vec![FontVariantNumericValue::TabularNums])
    );
    assert_eq!(
        marker.font_feature_settings,
        FontFeatureSettings(vec![FontFeatureSetting::new(*b"liga", 0)])
    );
    assert_eq!(
        marker.font_variant_numeric,
        FontVariantNumeric::Values(vec![FontVariantNumericValue::TabularNums])
    );
}

#[tokio::test]
async fn page_size_in_size_dependent_media_query_is_ignored() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: 4in 6in }\
         @media (max-width: 6in) { @page { size: letter } }",
    ));
    let size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);

    assert_eq!(size.width(), 288.0);
    assert_eq!(size.height(), 432.0);
}

#[tokio::test]
async fn parses_css_text_breaking_controls() {
    let declarations = parse_declarations(
        "word-break: break-all; overflow: hidden; overflow-x: clip; overflow-y: auto; overflow-wrap: anywhere; word-wrap: break-word; line-break: anywhere; hyphens: none; hyphenate-limit-chars: auto 3 4",
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.word_break, WordBreak::BreakAll);
    assert_eq!(style.overflow, Overflow::Hidden);
    assert_eq!(style.overflow_x, Overflow::Clip);
    assert_eq!(style.overflow_y, Overflow::Auto);
    assert_eq!(style.overflow_wrap, OverflowWrap::BreakWord);
    assert_eq!(style.line_break, LineBreak::Anywhere);
    assert_eq!(style.hyphens, Hyphens::None);
    assert_eq!(
        style.hyphenate_limit_chars,
        HyphenateLimitChars {
            total: HyphenateLimitChars::AUTO_TOTAL,
            before: 3,
            after: 4,
        }
    );
}

#[tokio::test]
async fn parses_css_text_transform_full_width_and_kana_values() {
    let mut style = default_style_for_tag("p");

    apply_declarations(
        &mut style,
        &parse_declarations("text-transform: full-width"),
    );
    assert_eq!(
        style.text_transform,
        TextTransform {
            case: TextTransformCase::None,
            full_width: true,
            full_size_kana: false,
        }
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-transform: full-size-kana"),
    );
    assert_eq!(
        style.text_transform,
        TextTransform {
            case: TextTransformCase::None,
            full_width: false,
            full_size_kana: true,
        }
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-transform: uppercase full-width full-size-kana"),
    );
    assert_eq!(
        style.text_transform,
        TextTransform {
            case: TextTransformCase::Uppercase,
            full_width: true,
            full_size_kana: true,
        }
    );
}

#[tokio::test]
async fn invalid_hyphenate_limit_chars_declarations_are_ignored() {
    let declarations = parse_declarations("hyphenate-limit-chars: 8 2 2 2");
    let mut style = default_style_for_tag("p");
    style.hyphenate_limit_chars = HyphenateLimitChars {
        total: 7,
        before: 3,
        after: 2,
    };

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.hyphenate_limit_chars,
        HyphenateLimitChars {
            total: 7,
            before: 3,
            after: 2,
        }
    );
}

#[tokio::test]
async fn legacy_word_break_break_word_enables_emergency_wrap() {
    let declarations = parse_declarations("overflow-wrap: normal; word-break: break-word");
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.word_break, WordBreak::Normal);
    assert_eq!(style.overflow_wrap, OverflowWrap::BreakWord);
}

#[tokio::test]
async fn parses_ch_lengths_in_box_sizes_and_math() {
    let declarations = parse_declarations("font-size: 20pt; width: 16ch; height: calc(2ch + 1pt)");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_ch(16.0))
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(1.0),
            percent: 0.0,
            ch: 2.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
}

#[tokio::test]
async fn parses_viewport_lengths_in_box_sizes_and_math() {
    let declarations =
        parse_declarations("width: 100vw; height: calc(50vh - 2vmin + 1vmax + 3vi - 4vb)");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_vw(100.0))
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            vh: 50.0,
            vmin: -2.0,
            vmax: 1.0,
            vi: 3.0,
            vb: -4.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
}

#[tokio::test]
async fn maps_logical_size_properties_to_physical_axes() {
    let declarations = parse_declarations(
        "inline-size: 10pt; block-size: 20pt; min-inline-size: 5pt; max-block-size: 30pt",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            10.0
        ))
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            20.0
        ))
    );
    assert_eq!(
        style.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            5.0
        ))
    );
    assert_eq!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            30.0
        ))
    );

    let declarations = parse_declarations(
        "writing-mode: vertical-rl; inline-size: 11pt; block-size: 22pt; min-block-size: 6pt; max-inline-size: 33pt",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            11.0
        ))
    );
    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            22.0
        ))
    );
    assert_eq!(
        style.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            6.0
        ))
    );
    assert_eq!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            33.0
        ))
    );
}

#[tokio::test]
async fn parses_quarter_millimeter_absolute_lengths() {
    let declarations = parse_declarations("width: 40q");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("width should compute to an absolute length");
    };
    assert!((width.length_points() - (10.0 * 72.0 / 25.4)).abs() < 0.001);
}

#[tokio::test]
async fn parses_ch_length_letter_spacing() {
    let declarations = parse_declarations("font-size: 20pt; letter-spacing: 1ch");
    let mut style = default_style_for_tag("span");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.letter_spacing, ComputedLengthPercentage::from_ch(1.0));
}

#[tokio::test]
async fn parses_ch_length_word_spacing() {
    let declarations = parse_declarations("font-size: 20pt; word-spacing: 2ch");
    let mut style = default_style_for_tag("span");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.word_spacing, ComputedLengthPercentage::from_ch(2.0));
}

#[tokio::test]
async fn parses_inline_vertical_align_subset() {
    let declarations = parse_declarations("vertical-align: super");
    let mut style = default_style_for_tag("span");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.vertical_align.baseline_shift, BaselineShift::Super);
    assert_eq!(
        style.vertical_align.alignment_baseline,
        AlignmentBaseline::Baseline
    );

    let declarations = parse_declarations("vertical-align: middle");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.vertical_align.alignment_baseline,
        AlignmentBaseline::Metric(BaselineMetric::Middle)
    );
    assert_eq!(
        style.vertical_align.table_cell_align,
        TableCellVerticalAlign::Middle
    );

    let declarations = parse_declarations("vertical-align: calc(3pt + 25%)");
    apply_declarations(&mut style, &declarations);

    let BaselineShift::LengthPercentage(value) = style.vertical_align.baseline_shift else {
        panic!("vertical-align length/percentage should parse as a typed shift");
    };
    assert!((value.length_points() - 3.0).abs() < 0.001);
    assert!((value.percent - 0.25).abs() < 0.001);

    let declarations = parse_declarations(
        "dominant-baseline: central; alignment-baseline: baseline; baseline-source: last; baseline-shift: 10%",
    );
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.vertical_align.dominant_baseline,
        DominantBaseline::Metric(BaselineMetric::Central)
    );
    assert_eq!(
        style.vertical_align.alignment_baseline,
        AlignmentBaseline::Baseline
    );
    assert_eq!(style.vertical_align.baseline_source, BaselineSource::Last);
    let BaselineShift::LengthPercentage(value) = style.vertical_align.baseline_shift else {
        panic!("baseline-shift percentage should parse as a typed shift");
    };
    assert!((value.percent - 0.1).abs() < 0.001);

    let declarations = parse_declarations("vertical-align: last text-top");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.vertical_align.baseline_source, BaselineSource::Last);
    assert_eq!(
        style.vertical_align.alignment_baseline,
        AlignmentBaseline::Metric(BaselineMetric::TextTop)
    );

    let declarations =
        parse_declarations("vertical-align: top; alignment-baseline: middle; baseline-shift: 5pt");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.vertical_align.table_cell_align,
        TableCellVerticalAlign::Top
    );
    assert_eq!(
        style.vertical_align.alignment_baseline,
        AlignmentBaseline::Metric(BaselineMetric::Middle)
    );
}

#[tokio::test]
async fn ch_baseline_shift_resolves_before_inline_layout() {
    let declarations = parse_declarations("vertical-align: 2ch");
    let mut style = default_style_for_tag("span");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.vertical_align.baseline_shift,
        BaselineShift::LengthPercentage(ComputedLengthPercentage::from_ch(2.0))
    );

    style.resolve_font_metric_lengths(6.0);

    assert_eq!(
        style.vertical_align.baseline_shift,
        BaselineShift::LengthPercentage(ComputedLengthPercentage::from_points(12.0))
    );

    let declarations = parse_declarations("baseline-shift: 3ch");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.vertical_align.baseline_shift,
        BaselineShift::LengthPercentage(ComputedLengthPercentage::from_ch(3.0))
    );

    style.resolve_font_metric_lengths(5.0);

    assert_eq!(
        style.vertical_align.baseline_shift,
        BaselineShift::LengthPercentage(ComputedLengthPercentage::from_points(15.0))
    );
}

#[tokio::test]
async fn parses_css_bookmark_properties() {
    let declarations = parse_declarations(
        r#"bookmark-level: 3; bookmark-label: "Appendix: " attr(data-title) " - " content(text); bookmark-state: closed"#,
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.bookmark_level, Some(3));
    assert_eq!(style.bookmark_state, CssBookmarkState::Closed);
    assert_eq!(
        style.bookmark_label,
        BookmarkLabel {
            parts: vec![
                BookmarkLabelPart::String("Appendix: ".to_string()),
                BookmarkLabelPart::Attr("data-title".to_string()),
                BookmarkLabelPart::String(" - ".to_string()),
                BookmarkLabelPart::ContentText,
            ]
        }
    );
}

#[tokio::test]
async fn bookmark_level_none_suppresses_heading_default() {
    let declarations = parse_declarations("bookmark-level: none");
    let mut style = default_style_for_tag("h1");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.bookmark_level, None);
}

#[tokio::test]
async fn computes_relative_font_weights_from_inherited_weight() {
    let parent = ComputedStyle {
        font_weight: FontWeight(550),
        ..default_style_for_tag("p")
    };

    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("font-weight: bolder"),
        &[],
        Some(&parent),
        &[],
    );
    let light_child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("font-weight: lighter"),
        &[],
        Some(&parent),
        &[],
    );

    assert_eq!(child.font_weight, FontWeight::BLACK);
    assert_eq!(light_child.font_weight, FontWeight::NORMAL);
}

#[tokio::test]
async fn parses_box_declarations() {
    let declarations = parse_declarations(
        "margin: 1px 2px; padding-left: 3px; border: 4px solid #f00; background: blue",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.margin.top, 0.75);
    assert_eq!(style.margin.right, 1.5);
    assert_eq!(style.padding.left, 2.25);
    assert_eq!(style.border_width, 3.0);
    assert_eq!(style.border_widths, edge_all(3.0));
    assert_eq!(style.border_color, Color::new(255, 0, 0));
    assert_eq!(style.border_colors.top, Color::new(255, 0, 0));
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.background_color, Some(Color::new(0, 0, 255)));
}

#[tokio::test]
async fn parses_multicolumn_declarations() {
    let declarations = parse_declarations("columns: 4 2em; column-gap: normal");
    let mut style = default_style_for_tag("dl");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.column_count, Some(4));
    assert_eq!(
        style.column_width,
        ComputedColumnWidth::Length(ComputedLengthPercentage::from_points(24.0))
    );
    assert_eq!(style.column_gap, ComputedGap::Normal);
    assert_eq!(style.row_gap, ComputedGap::Normal);
}

#[tokio::test]
async fn parses_column_width_longhand() {
    let declarations = parse_declarations("column-width: 30%; column-width: 12pt");
    let mut style = default_style_for_tag("dl");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.column_width,
        ComputedColumnWidth::Length(ComputedLengthPercentage::from_points(12.0))
    );
}

#[tokio::test]
async fn ch_column_width_preserves_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("column-width: 5ch");
    let mut style = default_style_for_tag("dl");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.column_width,
        ComputedColumnWidth::Length(ComputedLengthPercentage::from_ch(5.0))
    );

    style.resolve_font_metric_lengths(7.0);

    assert_eq!(
        style.column_width,
        ComputedColumnWidth::Length(ComputedLengthPercentage::from_points(35.0))
    );
}

#[tokio::test]
async fn parses_gap_computed_values() {
    let declarations =
        parse_declarations("gap: calc(1pt + 1pt) clamp(5%, 10%, 15%); row-gap: normal");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.row_gap, ComputedGap::Normal);
    assert_eq!(
        style.column_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_percent(0.1))
    );

    apply_declarations(&mut style, &parse_declarations("gap: calc(5pt + 5pt)"));
    assert_eq!(
        style.row_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(10.0))
    );
    assert_eq!(
        style.column_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(10.0))
    );

    apply_declarations(
        &mut style,
        &parse_declarations("gap: -1pt 20pt; row-gap: -2pt; column-gap: -3pt"),
    );
    assert_eq!(
        style.row_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(10.0))
    );
    assert_eq!(
        style.column_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(10.0))
    );

    apply_declarations(&mut style, &parse_declarations("gap: thick thin"));
    assert_eq!(
        style.row_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(5.0 * CSS_PX_TO_PT))
    );
    assert_eq!(
        style.column_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(1.0 * CSS_PX_TO_PT))
    );
}

#[tokio::test]
async fn parses_legacy_grid_gap_aliases() {
    let declarations = parse_declarations("grid-gap: 4pt 6pt; grid-row-gap: thick");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.row_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(5.0 * CSS_PX_TO_PT))
    );
    assert_eq!(
        style.column_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(6.0))
    );
}

#[tokio::test]
async fn parses_background_size_and_position_as_computed_values() {
    let declarations =
        parse_declarations("background-size: 50% auto; background-position: right 3pt bottom 25%");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background_size,
        BackgroundSize::Explicit {
            width: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_percent(
                0.5
            )),
            height: BackgroundSizeAxis::Auto,
        }
    );
    assert_eq!(
        style.background_position.x.origin,
        BackgroundPositionOrigin::End
    );
    assert_eq!(
        style.background_position.x.offset,
        ComputedLengthPercentage::from_points(3.0)
    );
    assert_eq!(
        style.background_position.y.origin,
        BackgroundPositionOrigin::End
    );
    assert_eq!(
        style.background_position.y.offset,
        ComputedLengthPercentage::from_percent(0.25)
    );
}

#[tokio::test]
async fn background_shorthand_size_split_ignores_url_slashes() {
    let declarations = parse_declarations(
        r#"background: url("support/1x1-green.png") 0 0 / 50px 100px no-repeat, red"#,
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.background_layers.len(), 2);
    assert_eq!(style.background_color, Some(Color::new(255, 0, 0)));
    assert_eq!(
        style.background_layers[0].repeat,
        BackgroundRepeat::NoRepeat
    );
    assert_eq!(
        style.background_layers[0].size,
        BackgroundSize::Explicit {
            width: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                50.0 * CSS_PX_TO_PT
            )),
            height: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                100.0 * CSS_PX_TO_PT
            )),
        }
    );
    assert_eq!(style.background_layers[1].image, None);
}

#[tokio::test]
async fn background_shorthand_size_split_ignores_data_url_slashes() {
    let declarations =
        parse_declarations("background: url(data:image/png;base64,AAAA) no-repeat 0 0 / 40pt 40pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background_size,
        BackgroundSize::Explicit {
            width: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                40.0
            )),
            height: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                40.0
            )),
        }
    );
    assert_eq!(style.background_repeat, BackgroundRepeat::NoRepeat);
}

#[tokio::test]
async fn background_shorthand_size_split_ignores_quoted_url_parentheses() {
    let declarations = parse_declarations(
        r#"background: url("support/a(b)/1x1-green.png") no-repeat 0 0 / 40pt 20pt"#,
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background_size,
        BackgroundSize::Explicit {
            width: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                40.0
            )),
            height: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                20.0
            )),
        }
    );
}

#[tokio::test]
async fn custom_properties_resolve_or_invalidate_declarations() {
    let mut missing = default_style_for_tag("div");
    apply_declarations(&mut missing, &parse_declarations("color: var(--missing)"));
    assert_eq!(missing.color, Color::BLACK);

    let mut duplicate = default_style_for_tag("div");
    apply_declarations(
        &mut duplicate,
        &parse_declarations("color: red; color: var(--missing)"),
    );
    assert_eq!(duplicate.color, Color::BLACK);

    let mut invalid_specified = default_style_for_tag("div");
    apply_declarations(
        &mut invalid_specified,
        &parse_declarations("color: red; color: definitely-not-a-color"),
    );
    assert_eq!(invalid_specified.color, Color::new(255, 0, 0));

    let mut fallback = default_style_for_tag("div");
    apply_declarations(
        &mut fallback,
        &parse_declarations("color: var(--missing, red)"),
    );
    assert_eq!(fallback.color, Color::new(255, 0, 0));

    let mut nested = default_style_for_tag("div");
    apply_declarations(
        &mut nested,
        &parse_declarations("--accent: #00ff00; color: var(--accent)"),
    );
    assert_eq!(nested.color, Color::new(0, 255, 0));
}

#[tokio::test]
async fn invalid_custom_property_winner_does_not_fall_back_to_earlier_rule() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div { color: red } div { color: var(--missing) }",
    ));

    let style = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::BLACK);
}

#[tokio::test]
async fn important_declarations_participate_in_cascade_sorting() {
    let mut direct = default_style_for_tag("div");
    apply_declarations(
        &mut direct,
        &parse_declarations("color: red !important; color: blue"),
    );
    assert_eq!(direct.color, Color::new(255, 0, 0));

    let stylesheet = parse_stylesheet(&Css::from_string("div { color: red !important }"));
    let inline_normal = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("color: blue"),
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    assert_eq!(inline_normal.color, Color::new(255, 0, 0));

    let inline_important = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("color: blue !important"),
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(inline_important.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn font_size_prepass_resolves_custom_properties() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("--scale: 2em; font-size: var(--scale); margin-top: 1em"),
    );

    assert_eq!(style.font_size, 24.0);
    assert_eq!(style.margin.top, 24.0);

    let mut invalid_winner = default_style_for_tag("div");
    apply_declarations(
        &mut invalid_winner,
        &parse_declarations("font-size: 20pt; font-size: var(--missing); margin-top: 1em"),
    );
    assert_eq!(invalid_winner.font_size, 12.0);
    assert_eq!(invalid_winner.margin.top, 12.0);
}

#[tokio::test]
async fn parses_inline_block_display() {
    let declarations = parse_declarations("display: inline-block");
    let mut style = default_style_for_tag("h1");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.display, Display::INLINE_BLOCK);
    assert!(style.display.is_inline_level());
    assert!(style.display.is_atomic_inline());
}

#[tokio::test]
async fn parses_margin_trim_keywords() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("margin-trim: block-start inline-end"),
    );

    assert!(style.margin_trim.block_start);
    assert!(!style.margin_trim.block_end);
    assert!(!style.margin_trim.inline_start);
    assert!(style.margin_trim.inline_end);

    apply_declarations(
        &mut style,
        &parse_declarations("margin-trim: none block-start"),
    );
    assert!(style.margin_trim.block_start);
    assert!(!style.margin_trim.block_end);
    assert!(!style.margin_trim.inline_start);
    assert!(style.margin_trim.inline_end);
}

#[tokio::test]
async fn parses_text_box_trim_and_edge_longhands() {
    for (value, expected) in [
        ("none", TextBoxTrim::None),
        ("trim-start", TextBoxTrim::TrimStart),
        ("trim-end", TextBoxTrim::TrimEnd),
        ("trim-both", TextBoxTrim::TrimBoth),
    ] {
        let mut style = default_style_for_tag("div");
        apply_declarations(
            &mut style,
            &parse_declarations(&format!("text-box-trim: {value}")),
        );
        assert_eq!(style.text_box_trim, expected);
    }

    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &parse_declarations("text-box-edge: text"));
    assert_eq!(style.text_box_edge, TextBoxEdge::Text(TextEdgePair::TEXT));

    apply_declarations(&mut style, &parse_declarations("text-box-edge: cap"));
    assert_eq!(
        style.text_box_edge,
        TextBoxEdge::Text(TextEdgePair::new(TextEdgeMetric::Cap, TextEdgeMetric::Text))
    );

    apply_declarations(&mut style, &parse_declarations("text-box-edge: alphabetic"));
    assert_eq!(
        style.text_box_edge,
        TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Text,
            TextEdgeMetric::Alphabetic
        ))
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-box-edge: cap alphabetic"),
    );
    assert_eq!(
        style.text_box_edge,
        TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Cap,
            TextEdgeMetric::Alphabetic
        ))
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-box-edge: ideographic-ink alphabetic"),
    );
    assert_eq!(
        style.text_box_edge,
        TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::IdeographicInk,
            TextEdgeMetric::Alphabetic
        ))
    );

    apply_declarations(&mut style, &parse_declarations("text-box-edge: auto"));
    assert_eq!(style.text_box_edge, TextBoxEdge::Auto);

    apply_declarations(
        &mut style,
        &parse_declarations("text-box-edge: alphabetic cap"),
    );
    assert_eq!(style.text_box_edge, TextBoxEdge::Auto);

    apply_declarations(&mut style, &parse_declarations("line-fit-edge: leading"));
    assert_eq!(style.line_fit_edge, LineFitEdge::Leading);

    apply_declarations(&mut style, &parse_declarations("line-fit-edge: text"));
    assert_eq!(style.line_fit_edge, LineFitEdge::Text(TextEdgePair::TEXT));

    apply_declarations(
        &mut style,
        &parse_declarations("line-fit-edge: ex alphabetic"),
    );
    assert_eq!(
        style.line_fit_edge,
        LineFitEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Ex,
            TextEdgeMetric::Alphabetic
        ))
    );

    apply_declarations(&mut style, &parse_declarations("line-fit-edge: auto"));
    assert_eq!(
        style.line_fit_edge,
        LineFitEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Ex,
            TextEdgeMetric::Alphabetic
        ))
    );

    apply_declarations(
        &mut style,
        &parse_declarations("box-decoration-break: clone"),
    );
    assert_eq!(style.box_decoration_break, BoxDecorationBreak::Clone);

    apply_declarations(
        &mut style,
        &parse_declarations("box-decoration-break: slice"),
    );
    assert_eq!(style.box_decoration_break, BoxDecorationBreak::Slice);
}

#[tokio::test]
async fn parses_text_box_shorthand() {
    let mut normal = default_style_for_tag("div");
    normal.text_box_trim = TextBoxTrim::TrimEnd;
    normal.text_box_edge = TextBoxEdge::Text(TextEdgePair::TEXT);
    apply_declarations(&mut normal, &parse_declarations("text-box: normal"));
    assert_eq!(normal.text_box_trim, TextBoxTrim::None);
    assert_eq!(normal.text_box_edge, TextBoxEdge::Auto);

    let mut full = default_style_for_tag("div");
    apply_declarations(&mut full, &parse_declarations("text-box: trim-end text"));
    assert_eq!(full.text_box_trim, TextBoxTrim::TrimEnd);
    assert_eq!(full.text_box_edge, TextBoxEdge::Text(TextEdgePair::TEXT));

    let mut edge_only = default_style_for_tag("div");
    apply_declarations(&mut edge_only, &parse_declarations("text-box: text"));
    assert_eq!(edge_only.text_box_trim, TextBoxTrim::TrimBoth);
    assert_eq!(
        edge_only.text_box_edge,
        TextBoxEdge::Text(TextEdgePair::TEXT)
    );

    let mut auto_only = default_style_for_tag("div");
    apply_declarations(&mut auto_only, &parse_declarations("text-box: auto"));
    assert_eq!(auto_only.text_box_trim, TextBoxTrim::TrimBoth);
    assert_eq!(auto_only.text_box_edge, TextBoxEdge::Auto);

    let mut trim_only = default_style_for_tag("div");
    apply_declarations(&mut trim_only, &parse_declarations("text-box: trim-start"));
    assert_eq!(trim_only.text_box_trim, TextBoxTrim::TrimStart);
    assert_eq!(trim_only.text_box_edge, TextBoxEdge::Auto);

    apply_declarations(
        &mut edge_only,
        &parse_declarations("text-box: trim-end cap alphabetic"),
    );
    assert_eq!(edge_only.text_box_trim, TextBoxTrim::TrimEnd);
    assert_eq!(
        edge_only.text_box_edge,
        TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Cap,
            TextEdgeMetric::Alphabetic
        ))
    );

    apply_declarations(
        &mut edge_only,
        &parse_declarations("text-box: trim-start trim-end"),
    );
    assert_eq!(edge_only.text_box_trim, TextBoxTrim::TrimEnd);
    assert_eq!(
        edge_only.text_box_edge,
        TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Cap,
            TextEdgeMetric::Alphabetic
        ))
    );

    apply_declarations(&mut edge_only, &parse_declarations("text-box: text cap"));
    assert_eq!(edge_only.text_box_trim, TextBoxTrim::TrimEnd);
    assert_eq!(
        edge_only.text_box_edge,
        TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Cap,
            TextEdgeMetric::Alphabetic
        ))
    );

    apply_declarations(&mut edge_only, &parse_declarations("text-box: unknown"));
    assert_eq!(edge_only.text_box_trim, TextBoxTrim::TrimEnd);
    assert_eq!(
        edge_only.text_box_edge,
        TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Cap,
            TextEdgeMetric::Alphabetic
        ))
    );
}

#[tokio::test]
async fn parses_display_outer_inner_model() {
    for (value, expected, list_item) in [
        ("none", Display::NONE, false),
        ("contents", Display::CONTENTS, false),
        ("block", Display::BLOCK, false),
        ("inline", Display::INLINE, false),
        ("run-in", Display::RUN_IN, false),
        ("run-in flow", Display::RUN_IN, false),
        (
            "flow-root",
            Display::new(DisplayOuter::Block, DisplayInner::FlowRoot),
            false,
        ),
        ("inline-block", Display::INLINE_BLOCK, false),
        ("flex", Display::FLEX, false),
        ("inline-flex", Display::INLINE_FLEX, false),
        ("grid", Display::GRID, false),
        ("inline-grid", Display::INLINE_GRID, false),
        ("inline grid", Display::INLINE_GRID, false),
        ("block grid", Display::GRID, false),
        (
            "run-in grid",
            Display::new(DisplayOuter::RunIn, DisplayInner::Grid),
            false,
        ),
        ("table", Display::TABLE, false),
        ("inline-table", Display::INLINE_TABLE, false),
        ("inline table", Display::INLINE_TABLE, false),
        ("block table", Display::TABLE, false),
        (
            "run-in table",
            Display::new(DisplayOuter::RunIn, DisplayInner::Table),
            false,
        ),
        ("table-caption", Display::TABLE_CAPTION, false),
        ("table-column-group", Display::TABLE_COLUMN_GROUP, false),
        ("table-column", Display::TABLE_COLUMN, false),
        ("table-row-group", Display::TABLE_ROW_GROUP, false),
        ("table-header-group", Display::TABLE_HEADER_GROUP, false),
        ("table-footer-group", Display::TABLE_FOOTER_GROUP, false),
        ("table-row", Display::TABLE_ROW, false),
        ("table-cell", Display::TABLE_CELL, false),
        ("list-item", Display::BLOCK, true),
        ("inline flow list-item", Display::INLINE, true),
        ("run-in list-item", Display::RUN_IN, true),
        (
            "flow-root list-item",
            Display::new(DisplayOuter::Block, DisplayInner::FlowRoot),
            true,
        ),
        (
            "run-in flow-root list-item",
            Display::new(DisplayOuter::RunIn, DisplayInner::FlowRoot),
            true,
        ),
    ] {
        let expected = expected.with_list_item(list_item);
        let mut style = ComputedStyle::initial();
        apply_declarations(
            &mut style,
            &parse_declarations(&format!("display: {value}")),
        );
        assert_eq!(style.display, expected, "{value}");
        assert_eq!(style.display.is_list_item(), list_item, "{value}");
    }
}

#[tokio::test]
async fn rejects_invalid_display_outer_inner_grammar() {
    for value in [
        "block inline",
        "flow flow-root",
        "flex list-item",
        "inline flex list-item",
        "grid list-item",
        "inline grid list-item",
        "table list-item",
        "run-in table list-item",
        "inline-block flow",
        "block unknown",
        "run-in run-in",
    ] {
        let mut style = ComputedStyle::initial();
        apply_declarations(&mut style, &parse_declarations("display: inline"));
        apply_declarations(
            &mut style,
            &parse_declarations(&format!("display: {value}")),
        );
        assert_eq!(style.display, Display::INLINE, "{value}");
        assert!(!style.display.is_list_item(), "{value}");
    }
}

#[tokio::test]
async fn parses_list_style_position_and_shorthand() {
    let declarations =
        parse_declarations("list-style-position: inside; list-style: lower-alpha outside");
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);

    assert!(style.display.is_list_item());
    assert_eq!(style.list_style_type, ListStyleType::LowerAlpha);
    assert_eq!(style.list_style_position, ListStylePosition::Outside);
}

#[tokio::test]
async fn parses_list_style_shorthand_string_type_in_any_order() {
    for value in ["inside \"# \"", "\"# \" inside"] {
        let declarations = parse_declarations(&format!("list-style: {value}"));
        let mut style = default_style_for_tag("li");
        apply_declarations(&mut style, &declarations);

        assert_eq!(
            style.list_style_type,
            ListStyleType::String("# ".to_string())
        );
        assert_eq!(style.list_style_position, ListStylePosition::Inside);
        assert_eq!(style.list_style_image, None);
    }
}

#[tokio::test]
async fn parses_list_style_shorthand_symbols_function_type() {
    let declarations = parse_declarations("list-style: symbols(cyclic \"*\" \"†\") inside");
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);

    let ListStyleType::Anonymous(rule) = style.list_style_type else {
        panic!("symbols() should produce an anonymous counter style");
    };
    assert_eq!(rule.system, CounterStyleSystem::Cyclic);
    assert_eq!(rule.symbols, ["*", "†"]);
    assert_eq!(style.list_style_position, ListStylePosition::Inside);
}

#[tokio::test]
async fn parses_list_style_shorthand_none_ambiguity() {
    for (value, expected_type, expected_image) in [
        ("none", ListStyleType::None, None),
        ("none disc", ListStyleType::Disc, None),
        (
            "none url(marker.png)",
            ListStyleType::None,
            Some("marker.png"),
        ),
    ] {
        let declarations = parse_declarations(&format!("list-style: {value}"));
        let mut style = default_style_for_tag("li");
        apply_declarations(&mut style, &declarations);

        assert_eq!(style.list_style_type, expected_type, "{value}");
        assert_eq!(style.list_style_image.as_deref(), expected_image, "{value}");
    }
}

#[tokio::test]
async fn invalid_list_style_values_do_not_partially_apply() {
    let declarations = parse_declarations(
        "list-style: lower-alpha inside url(marker.png); list-style: none disc url(other.png)",
    );
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.list_style_type, ListStyleType::LowerAlpha);
    assert_eq!(style.list_style_position, ListStylePosition::Inside);
    assert_eq!(style.list_style_image.as_deref(), Some("marker.png"));

    apply_declarations(
        &mut style,
        &parse_declarations("list-style-type: \"# \" inside"),
    );
    assert_eq!(style.list_style_type, ListStyleType::LowerAlpha);
}

#[tokio::test]
async fn parses_marker_side_and_inherits() {
    let mut parent = ComputedStyle::initial();
    apply_declarations(
        &mut parent,
        &parse_declarations("marker-side: match-parent"),
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("li", HashMap::new()),
        None,
        &[],
        Some(&parent),
        &[],
    );

    assert_eq!(parent.marker_side, MarkerSide::MatchParent);
    assert_eq!(child.marker_side, MarkerSide::MatchParent);

    let mut style = ComputedStyle::initial();
    apply_declarations(
        &mut style,
        &parse_declarations("marker-side: match-parent; marker-side: bad"),
    );
    assert_eq!(style.marker_side, MarkerSide::MatchParent);
    apply_declarations(&mut style, &parse_declarations("marker-side: match-self"));
    assert_eq!(style.marker_side, MarkerSide::MatchSelf);
}

#[tokio::test]
async fn ua_marker_default_is_bidi_isolated() {
    let style = default_style_for_tag("li");
    let marker_style = style.marker_style.as_deref().expect("li has marker style");

    assert_eq!(marker_style.unicode_bidi, UnicodeBidi::Isolate);
}

#[tokio::test]
async fn parses_list_style_image_longhand_and_shorthand() {
    let declarations = parse_declarations(
        "list-style-image: url(marker.png); list-style: square inside url(other.png)",
    );
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.list_style_type, ListStyleType::Square);
    assert_eq!(style.list_style_position, ListStylePosition::Inside);
    assert_eq!(style.list_style_image.as_deref(), Some("other.png"));

    apply_declarations(&mut style, &parse_declarations("list-style-image: none"));
    assert_eq!(style.list_style_image, None);
}

#[tokio::test]
async fn parses_predefined_counter_style_names() {
    for (value, expected) in [
        ("decimal-leading-zero", ListStyleType::DecimalLeadingZero),
        (
            "arabic-indic",
            ListStyleType::Numeric(NumericCounterStyle::ArabicIndic),
        ),
        (
            "khmer",
            ListStyleType::Numeric(NumericCounterStyle::Cambodian),
        ),
        (
            "lower-armenian",
            ListStyleType::Additive(AdditiveCounterStyle::LowerArmenian),
        ),
        ("lower-greek", ListStyleType::LowerGreek),
        ("hiragana-iroha", ListStyleType::HiraganaIroha),
        ("cjk-heavenly-stem", ListStyleType::CjkHeavenlyStem),
        ("disclosure-closed", ListStyleType::DisclosureClosed),
    ] {
        let declarations = parse_declarations(&format!("list-style-type: {value}"));
        let mut style = default_style_for_tag("li");
        apply_declarations(&mut style, &declarations);
        assert_eq!(style.list_style_type, expected, "{value}");
    }
}

#[tokio::test]
async fn parses_symbols_function_list_style_type() {
    let declarations = parse_declarations("list-style-type: symbols(cyclic \"*\" \"†\")");
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);

    let ListStyleType::Anonymous(rule) = style.list_style_type else {
        panic!("symbols() should produce an anonymous counter style");
    };
    assert_eq!(rule.system, CounterStyleSystem::Cyclic);
    assert_eq!(rule.symbols, ["*", "†"]);
}

#[tokio::test]
async fn parses_counter_style_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@counter-style binary { system: numeric; symbols: \"0\" \"1\"; prefix: \"[\"; suffix: \") \"; negative: \"(\" \")\"; pad: 4 \"0\"; range: 0 15; fallback: decimal; speak-as: numbers }",
    ));

    assert_eq!(stylesheet.counter_styles.len(), 1);
    let rule = &stylesheet.counter_styles[0];
    assert_eq!(rule.name, "binary");
    assert_eq!(rule.system, CounterStyleSystem::Numeric);
    assert_eq!(rule.symbols, ["0", "1"]);
    assert_eq!(rule.prefix.as_deref(), Some("["));
    assert_eq!(rule.suffix.as_deref(), Some(") "));
    assert_eq!(
        rule.negative
            .as_ref()
            .map(|(prefix, suffix)| (prefix.as_str(), suffix.as_str())),
        Some(("(", ")"))
    );
    assert_eq!(
        rule.pad
            .as_ref()
            .map(|(width, symbol)| (*width, symbol.as_str())),
        Some((4, "0"))
    );
    assert_eq!(
        rule.range,
        Some(CounterStyleRange::Intervals(vec![
            CounterStyleRangeInterval { start: 0, end: 15 }
        ]))
    );
    assert_eq!(rule.fallback.as_deref(), Some("decimal"));
    assert_eq!(rule.speak_as.as_deref(), Some("numbers"));
}

#[tokio::test]
async fn parses_counter_style_range_intervals_and_auto() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@counter-style split { system: numeric; symbols: 0 1; range: 1 10, 20 infinite }\
         @counter-style automatic { system: symbolic; symbols: x; range: auto }",
    ));

    assert_eq!(
        stylesheet.counter_styles[0].range,
        Some(CounterStyleRange::Intervals(vec![
            CounterStyleRangeInterval { start: 1, end: 10 },
            CounterStyleRangeInterval {
                start: 20,
                end: i64::MAX,
            },
        ]))
    );
    assert_eq!(
        stylesheet.counter_styles[1].range,
        Some(CounterStyleRange::Auto)
    );
}

#[tokio::test]
async fn invalid_additive_symbols_do_not_define_counter_style() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@counter-style bad-order { system: additive; additive-symbols: 1 a, 2 b }\
         @counter-style bad-zero { system: additive; additive-symbols: 2 b, 0 z, 1 a }",
    ));

    assert!(stylesheet.counter_styles.is_empty());
}

#[tokio::test]
async fn parses_extends_counter_style_system() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@counter-style binary-brackets { system: extends binary; suffix: \"] \" }",
    ));

    assert_eq!(stylesheet.counter_styles.len(), 1);
    assert_eq!(
        stylesheet.counter_styles[0].system,
        CounterStyleSystem::Extends("binary".to_string())
    );
    assert_eq!(stylesheet.counter_styles[0].suffix.as_deref(), Some("] "));
}

#[tokio::test]
async fn parses_marker_pseudo_element_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "li::marker { color: red; content: counter(list-item, lower-alpha) \") \" }",
    ));

    assert_eq!(stylesheet.rules.len(), 0);
    assert_eq!(stylesheet.marker_rules.len(), 1);
    assert_eq!(
        stylesheet.marker_rules[0]
            .declarations
            .get("content")
            .map(String::as_str),
        Some("counter(list-item, lower-alpha) \") \"")
    );
}

#[tokio::test]
async fn parses_before_and_after_pseudo_element_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "dt::before { content: \"\"; display: block } dt::after { content: \":\" }",
    ));

    assert_eq!(stylesheet.rules.len(), 0);
    assert_eq!(stylesheet.before_rules.len(), 1);
    assert_eq!(stylesheet.after_rules.len(), 1);
    assert_eq!(stylesheet.before_rules[0].selector_text, "dt");
    assert_eq!(stylesheet.after_rules[0].selector_text, "dt");
    assert_eq!(
        stylesheet.after_rules[0]
            .declarations
            .get("content")
            .map(String::as_str),
        Some("\":\"")
    );

    let style = style_for_element_with_signature(
        ElementSignature::new("dt", HashMap::new()),
        None,
        &[stylesheet],
        Some(&default_style_for_tag("dl")),
        &[],
    );
    assert_eq!(
        style
            .after_style
            .as_ref()
            .and_then(|style| style.content.generated_parts()),
        Some(&[GeneratedContentPart::Text(":".to_string())][..])
    );
    assert_eq!(
        style.before_style.as_ref().map(|style| style.display),
        Some(Display::BLOCK)
    );
    assert_eq!(
        style.after_style.as_ref().map(|style| style.display),
        Some(Display::INLINE)
    );
}

#[tokio::test]
async fn classifies_supported_pseudo_element_rule_lists() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p::before, div::before { content: \"before\" }\
         p::after, div::after { content: \"after\" }\
         li::marker, summary::marker { color: red }\
         p::first-line, div::first-line { color: blue }\
         p::first-letter, div::first-letter { color: green }",
    ));

    assert_eq!(stylesheet.rules.len(), 0);
    assert_eq!(stylesheet.before_rules.len(), 1);
    assert_eq!(stylesheet.after_rules.len(), 1);
    assert_eq!(stylesheet.marker_rules.len(), 1);
    assert_eq!(stylesheet.first_line_rules.len(), 1);
    assert_eq!(stylesheet.first_letter_rules.len(), 1);
    assert_eq!(stylesheet.before_rules[0].selector_text, "p, div");
    assert_eq!(stylesheet.after_rules[0].selector_text, "p, div");
    assert_eq!(stylesheet.marker_rules[0].selector_text, "li, summary");
    assert_eq!(stylesheet.first_line_rules[0].selector_text, "p, div");
    assert_eq!(stylesheet.first_letter_rules[0].selector_text, "p, div");
}

#[tokio::test]
async fn splits_mixed_normal_and_pseudo_element_selector_lists() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div::after, p, span::before { content: \"x\"; width: 10pt }",
    ));

    assert_eq!(stylesheet.rules.len(), 1);
    assert_eq!(stylesheet.before_rules.len(), 1);
    assert_eq!(stylesheet.after_rules.len(), 1);
    assert_eq!(stylesheet.rules[0].selector_text, "p");
    assert_eq!(stylesheet.before_rules[0].selector_text, "span");
    assert_eq!(stylesheet.after_rules[0].selector_text, "div");
    assert_eq!(
        stylesheet.after_rules[0]
            .declarations
            .get("content")
            .map(String::as_str),
        Some("\"x\"")
    );
    assert_eq!(
        stylesheet.before_rules[0]
            .declarations
            .get("width")
            .map(String::as_str),
        Some("10pt")
    );
}

#[tokio::test]
async fn parses_list_item_counter_properties() {
    let declarations = parse_declarations(
        "counter-reset: section 2 list-item 4; counter-increment: other list-item 2; counter-set: list-item 9",
    );
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.counter_resets,
        vec![("section".to_string(), 2), ("list-item".to_string(), 4)]
    );
    assert_eq!(
        style.counter_increments,
        vec![("other".to_string(), 1), ("list-item".to_string(), 2)]
    );
    assert_eq!(style.counter_sets, vec![("list-item".to_string(), 9)]);
}

#[tokio::test]
async fn parses_counter_properties_with_spec_duplicate_and_invalid_handling() {
    let mut style = default_style_for_tag("div");
    let valid = parse_declarations(
        "counter-reset: chapter 1 section 2 chapter 3; counter-increment: item item 2; counter-set: page 4 page 5",
    );
    apply_declarations(&mut style, &valid);

    assert_eq!(
        style.counter_resets,
        vec![("section".to_string(), 2), ("chapter".to_string(), 3)]
    );
    assert_eq!(
        style.counter_increments,
        vec![("item".to_string(), 1), ("item".to_string(), 2)]
    );
    assert_eq!(style.counter_sets, vec![("page".to_string(), 5)]);

    let invalid = parse_declarations(
        "counter-reset: none chapter; counter-increment: none 1; counter-set: item 1.5",
    );
    apply_declarations(&mut style, &invalid);
    assert_eq!(
        style.counter_resets,
        vec![("section".to_string(), 2), ("chapter".to_string(), 3)]
    );
    assert_eq!(
        style.counter_increments,
        vec![("item".to_string(), 1), ("item".to_string(), 2)]
    );
    assert_eq!(style.counter_sets, vec![("page".to_string(), 5)]);
}

#[tokio::test]
async fn parses_named_string_sets() {
    let declarations = parse_declarations(
        r#"string-set: chapter "Chapter: " content(text), short attr(data-short, "Fallback"), decorated content(before) content(after) content(first-letter) content(marker), numbered counter(section, upper-roman) "." counters(item, "|"), illustrated "Icon" url(icon.png)"#,
    );
    let mut style = default_style_for_tag("h1");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.string_sets.len(), 5);
    assert_eq!(style.string_sets[0].name, "chapter");
    assert_eq!(
        style.string_sets[0].parts,
        vec![
            NamedStringPart::String("Chapter: ".to_string()),
            NamedStringPart::ContentText
        ]
    );
    assert_eq!(style.string_sets[1].name, "short");
    assert_eq!(
        style.string_sets[1].parts,
        vec![NamedStringPart::Attr {
            name: "data-short".to_string(),
            fallback: Some("Fallback".to_string())
        }]
    );
    assert_eq!(
        style.string_sets[2].parts,
        vec![
            NamedStringPart::BeforeContent,
            NamedStringPart::AfterContent,
            NamedStringPart::ContentFirstLetter,
            NamedStringPart::ContentMarker
        ]
    );
    assert_eq!(
        style.string_sets[3].parts,
        vec![
            NamedStringPart::Counter {
                name: "section".to_string(),
                style: Some(ListStyleType::UpperRoman)
            },
            NamedStringPart::String(".".to_string()),
            NamedStringPart::Counters {
                name: "item".to_string(),
                separator: "|".to_string(),
                style: None
            }
        ]
    );
    assert_eq!(
        style.string_sets[4].parts,
        vec![
            NamedStringPart::String("Icon".to_string()),
            NamedStringPart::Image(BackgroundImage::Url {
                src: "icon.png".to_string(),
                base_url: None,
                root_url: None
            })
        ]
    );
}

#[tokio::test]
async fn parses_named_string_target_references() {
    let declarations = parse_declarations(
        r##"string-set: label "Page " target-counter(url("#chapter"), page, upper-roman) " " target-text("#chapter", before)"##,
    );
    let mut style = default_style_for_tag("h1");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.string_sets.len(), 1);
    assert_eq!(
        style.string_sets[0].parts,
        vec![
            NamedStringPart::String("Page ".to_string()),
            NamedStringPart::TargetCounter {
                target: "#chapter".to_string(),
                name: "page".to_string(),
                style: Some(ListStyleType::UpperRoman)
            },
            NamedStringPart::String(" ".to_string()),
            NamedStringPart::TargetText {
                target: "#chapter".to_string(),
                keyword: NamedStringTargetTextKeyword::Before
            }
        ]
    );
}

#[tokio::test]
async fn parses_named_string_quote_and_leader_items() {
    let declarations = parse_declarations(
        r#"string-set: label open-quote "Chapter" close-quote leader(dotted) "2""#,
    );
    let mut style = default_style_for_tag("h1");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.string_sets.len(), 1);
    assert_eq!(
        style.string_sets[0].parts,
        vec![
            NamedStringPart::Quote(GeneratedQuote::Open),
            NamedStringPart::String("Chapter".to_string()),
            NamedStringPart::Quote(GeneratedQuote::Close),
            NamedStringPart::Leader(".".to_string()),
            NamedStringPart::String("2".to_string())
        ]
    );
}

#[tokio::test]
async fn classifies_atomic_inline_displays() {
    assert!(Display::INLINE_BLOCK.is_inline_level());
    assert!(Display::INLINE_BLOCK.is_atomic_inline());
    assert!(Display::INLINE_FLEX.is_inline_level());
    assert!(Display::INLINE_FLEX.is_atomic_inline());
    assert!(Display::INLINE_GRID.is_inline_level());
    assert!(Display::INLINE_GRID.is_atomic_inline());
    assert!(!Display::INLINE.is_atomic_inline());
    assert!(Display::FLEX.establishes_block_formatting_context());
    assert!(Display::GRID.establishes_block_formatting_context());
}

#[tokio::test]
async fn parses_grid_display_and_core_longhands() {
    let declarations = parse_declarations(
        "display: grid;\
         grid-template-columns: [start] 10pt 1fr minmax(20pt, 2fr) fit-content(30pt) repeat(2, 5pt);\
         grid-template-rows: none;\
         grid-template-areas: \"head head\" \"side main\";\
         grid-auto-rows: minmax(8pt, auto) 1fr;\
         grid-auto-columns: 12pt;\
         grid-auto-flow: column dense;\
         grid-row-start: 2;\
         grid-row-end: span footer 3;\
         grid-column-start: main;\
         grid-column-end: span 2",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.display, Display::GRID);
    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("grid-template-columns should parse");
    };
    assert_eq!(columns.len(), 5);
    match &columns[0] {
        GridTrackListComponent::Track(names, size) => {
            assert_eq!(names, &["start".to_string()]);
            assert_eq!(
                size.min,
                GridMinTrackBreadth::LengthPercentage(ComputedLengthPercentage::from_points(10.0))
            );
        }
        other => panic!("expected first explicit track, got {other:?}"),
    }
    assert!(matches!(
        columns[1],
        GridTrackListComponent::Track(
            _,
            GridTrackSize {
                min: GridMinTrackBreadth::Auto,
                max: GridMaxTrackBreadth::Flex(1.0)
            }
        )
    ));
    assert!(matches!(columns[4], GridTrackListComponent::Repeat(_, _)));
    assert_eq!(style.grid_template_rows, GridTrackList::None);
    let GridTemplateAreas::Areas(rows) = &style.grid_template_areas else {
        panic!("grid-template-areas should parse");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].cells,
        [Some("head".to_string()), Some("head".to_string())]
    );
    assert_eq!(style.grid_auto_flow, GridAutoFlow::ColumnDense);
    assert_eq!(
        style.grid_row_start,
        GridPlacement::Line(GridLinePlacement {
            name: None,
            index: Some(2)
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Span(GridSpanPlacement {
            name: Some("footer".to_string()),
            span: Some(3)
        })
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("main".to_string()),
            index: None
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Span(GridSpanPlacement {
            name: None,
            span: Some(2)
        })
    );
}

#[tokio::test]
async fn invalid_grid_longhands_do_not_partially_apply() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: 10pt;\
             grid-template-areas: \"head head\" \"side main\";\
             grid-auto-flow: row",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: minmax(1fr, 10pt);\
             grid-template-areas: \"bad bad\" \"bad main\";\
             grid-auto-flow: row column",
        ),
    );

    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("valid initial grid-template-columns should remain");
    };
    assert_eq!(columns.len(), 1);
    let GridTemplateAreas::Areas(rows) = &style.grid_template_areas else {
        panic!("valid initial grid-template-areas should remain");
    };
    assert_eq!(
        rows[0].cells,
        [Some("head".to_string()), Some("head".to_string())]
    );
    assert_eq!(style.grid_auto_flow, GridAutoFlow::Row);
}

#[tokio::test]
async fn parses_grid_auto_repeat_track_lists() {
    let declarations = parse_declarations(
        "grid-template-columns: [outer] repeat(auto-fit, [card] minmax(12pt, 1fr) [end]);\
         grid-template-rows: repeat(auto-fill, minmax(auto, 20pt));",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("grid-template-columns should parse auto-fit");
    };
    let GridTrackListComponent::Repeat(names, repeat) = &columns[0] else {
        panic!("expected auto-fit repeat");
    };
    assert_eq!(names, &["outer".to_string()]);
    assert_eq!(repeat.count, GridRepeatCount::AutoFit);
    assert_eq!(repeat.trailing_names, ["end".to_string()]);
    assert_eq!(repeat.tracks.len(), 1);

    let GridTrackList::Tracks {
        components: rows, ..
    } = &style.grid_template_rows
    else {
        panic!("grid-template-rows should parse auto-fill");
    };
    let GridTrackListComponent::Repeat(_, repeat) = &rows[0] else {
        panic!("expected auto-fill repeat");
    };
    assert_eq!(repeat.count, GridRepeatCount::AutoFill);
}

#[tokio::test]
async fn invalid_grid_auto_repeat_forms_do_not_apply() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: repeat(auto-fill, 20pt);\
             grid-template-rows: 10pt",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: repeat(auto-fit, 1fr);\
             grid-template-rows: repeat(2, repeat(2, 10pt))",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: repeat(auto-fill, 10pt) repeat(auto-fit, 20pt)",
        ),
    );

    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("valid initial grid-template-columns should remain");
    };
    let GridTrackListComponent::Repeat(_, repeat) = &columns[0] else {
        panic!("valid initial auto-fill repeat should remain");
    };
    assert_eq!(repeat.count, GridRepeatCount::AutoFill);

    let GridTrackList::Tracks {
        components: rows, ..
    } = &style.grid_template_rows
    else {
        panic!("valid initial grid-template-rows should remain");
    };
    assert_eq!(rows.len(), 1);
    assert!(matches!(rows[0], GridTrackListComponent::Track(_, _)));
}

#[tokio::test]
async fn invalid_grid_track_line_names_do_not_apply() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: [main edge] 10pt;\
             grid-template-rows: [row-start] 12pt",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: [span] 20pt;\
             grid-template-rows: [main()] 30pt",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations("grid-template-columns: [initial] 40pt"),
    );

    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("valid initial grid-template-columns should remain");
    };
    let GridTrackListComponent::Track(names, _) = &columns[0] else {
        panic!("valid initial column track should remain");
    };
    assert_eq!(names, &["main".to_string(), "edge".to_string()]);

    let GridTrackList::Tracks {
        components: rows, ..
    } = &style.grid_template_rows
    else {
        panic!("valid initial grid-template-rows should remain");
    };
    let GridTrackListComponent::Track(names, _) = &rows[0] else {
        panic!("valid initial row track should remain");
    };
    assert_eq!(names, &["row-start".to_string()]);
}

#[tokio::test]
async fn invalid_grid_template_area_rows_do_not_apply() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("grid-template-areas: \"a a\" \"b c\""),
    );
    apply_declarations(
        &mut style,
        &parse_declarations("grid-template-areas: \"a @\" \"b c\""),
    );
    let GridTemplateAreas::Areas(rows) = &style.grid_template_areas else {
        panic!("valid initial grid-template-areas should remain");
    };
    assert_eq!(
        rows[0].cells,
        [Some("a".to_string()), Some("a".to_string())]
    );
}

#[tokio::test]
async fn parses_grid_row_and_column_shorthands() {
    let declarations = parse_declarations("grid-row: header / span 2; grid-column: 2 / main");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.grid_row_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("header".to_string()),
            index: None
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Span(GridSpanPlacement {
            name: None,
            span: Some(2)
        })
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement {
            name: None,
            index: Some(2)
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Line(GridLinePlacement {
            name: Some("main".to_string()),
            index: None
        })
    );

    let declarations = parse_declarations("grid-row-start: auto; grid-column: 2");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.grid_row_start, GridPlacement::Auto);
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement {
            name: None,
            index: Some(2)
        })
    );
    assert_eq!(style.grid_column_end, GridPlacement::Auto);
}

#[tokio::test]
async fn parses_grid_area_shorthand() {
    let declarations = parse_declarations("grid-area: main");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let expected = GridPlacement::Line(GridLinePlacement {
        name: Some("main".to_string()),
        index: None,
    });
    assert_eq!(style.grid_row_start, expected);
    assert_eq!(style.grid_column_start, expected);
    assert_eq!(style.grid_row_end, expected);
    assert_eq!(style.grid_column_end, expected);

    let declarations = parse_declarations("grid-area: 2 / side / span 3 / 4");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.grid_row_start,
        GridPlacement::Line(GridLinePlacement {
            name: None,
            index: Some(2)
        })
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("side".to_string()),
            index: None
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Span(GridSpanPlacement {
            name: None,
            span: Some(3)
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Line(GridLinePlacement {
            name: None,
            index: Some(4)
        })
    );
}

#[tokio::test]
async fn parses_grid_area_shorthand_omitted_values() {
    let declarations = parse_declarations("grid-area: header / main");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let header = GridPlacement::Line(GridLinePlacement {
        name: Some("header".to_string()),
        index: None,
    });
    let main = GridPlacement::Line(GridLinePlacement {
        name: Some("main".to_string()),
        index: None,
    });
    assert_eq!(style.grid_row_start, header);
    assert_eq!(style.grid_column_start, main);
    assert_eq!(style.grid_row_end, header);
    assert_eq!(style.grid_column_end, main);

    let declarations = parse_declarations("grid-area: 2 / main / 4");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.grid_row_start,
        GridPlacement::Line(GridLinePlacement {
            name: None,
            index: Some(2)
        })
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("main".to_string()),
            index: None
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Line(GridLinePlacement {
            name: None,
            index: Some(4)
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Line(GridLinePlacement {
            name: Some("main".to_string()),
            index: None
        })
    );
}

#[tokio::test]
async fn invalid_grid_area_shorthand_does_not_partially_apply() {
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &parse_declarations("grid-area: header / main"));
    apply_declarations(
        &mut style,
        &parse_declarations("grid-area: 1 / 2 / 3 / 4 / 5"),
    );

    assert_eq!(
        style.grid_row_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("header".to_string()),
            index: None
        })
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("main".to_string()),
            index: None
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Line(GridLinePlacement {
            name: Some("header".to_string()),
            index: None
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Line(GridLinePlacement {
            name: Some("main".to_string()),
            index: None
        })
    );
}

#[tokio::test]
async fn parses_grid_named_line_occurrences() {
    let declarations = parse_declarations(
        "grid-row-start: header 2;\
         grid-row-end: 3 footer;\
         grid-column-start: span main 2;\
         grid-column-end: 2 span main",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.grid_row_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("header".to_string()),
            index: Some(2)
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Line(GridLinePlacement {
            name: Some("footer".to_string()),
            index: Some(3)
        })
    );
    let expected_span = GridPlacement::Span(GridSpanPlacement {
        name: Some("main".to_string()),
        span: Some(2),
    });
    assert_eq!(style.grid_column_start, expected_span);
    assert_eq!(style.grid_column_end, expected_span);
}

#[tokio::test]
async fn invalid_grid_placement_custom_idents_do_not_apply() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-row-start: header;\
             grid-column-start: main;\
             grid-column-end: span rail 2",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-row-start: \"bad\";\
             grid-column-start: main();\
             grid-column-end: span initial 2",
        ),
    );

    assert_eq!(
        style.grid_row_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("header".to_string()),
            index: None
        })
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("main".to_string()),
            index: None
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Span(GridSpanPlacement {
            name: Some("rail".to_string()),
            span: Some(2)
        })
    );
}

#[tokio::test]
async fn invalid_grid_placement_shorthand_custom_idents_do_not_apply() {
    let mut row_style = default_style_for_tag("div");
    apply_declarations(
        &mut row_style,
        &parse_declarations("grid-row: header / footer"),
    );
    apply_declarations(
        &mut row_style,
        &parse_declarations("grid-row: initial / main"),
    );

    assert_eq!(
        row_style.grid_row_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("header".to_string()),
            index: None
        })
    );
    assert_eq!(
        row_style.grid_row_end,
        GridPlacement::Line(GridLinePlacement {
            name: Some("footer".to_string()),
            index: None
        })
    );

    let mut area_style = default_style_for_tag("div");
    apply_declarations(
        &mut area_style,
        &parse_declarations("grid-area: card / main"),
    );
    apply_declarations(
        &mut area_style,
        &parse_declarations("grid-area: card() / main"),
    );

    assert_eq!(
        area_style.grid_row_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("card".to_string()),
            index: None
        })
    );
    assert_eq!(
        area_style.grid_row_end,
        GridPlacement::Line(GridLinePlacement {
            name: Some("card".to_string()),
            index: None
        })
    );
    assert_eq!(
        area_style.grid_column_start,
        GridPlacement::Line(GridLinePlacement {
            name: Some("main".to_string()),
            index: None
        })
    );
    assert_eq!(
        area_style.grid_column_end,
        GridPlacement::Line(GridLinePlacement {
            name: Some("main".to_string()),
            index: None
        })
    );
}

#[tokio::test]
async fn parses_grid_template_shorthand() {
    let declarations = parse_declarations(
        "grid-template: [top] \"head head\" 12pt [middle] \"side main\" minmax(20pt, auto) [bottom] / [left] 30pt [split] 1fr [right]",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let GridTemplateAreas::Areas(rows) = &style.grid_template_areas else {
        panic!("grid-template should set areas");
    };
    assert_eq!(
        rows[0].cells,
        [Some("head".to_string()), Some("head".to_string())]
    );
    assert_eq!(
        rows[1].cells,
        [Some("side".to_string()), Some("main".to_string())]
    );

    let GridTrackList::Tracks {
        components: row_tracks,
        trailing_names: row_trailing_names,
    } = &style.grid_template_rows
    else {
        panic!("grid-template should set row tracks");
    };
    assert_eq!(row_tracks.len(), 2);
    assert_eq!(row_trailing_names, &["bottom".to_string()]);
    match &row_tracks[0] {
        GridTrackListComponent::Track(names, size) => {
            assert_eq!(names, &["top".to_string()]);
            assert_eq!(
                size.min,
                GridMinTrackBreadth::LengthPercentage(ComputedLengthPercentage::from_points(12.0))
            );
        }
        other => panic!("expected first row track, got {other:?}"),
    }
    match &row_tracks[1] {
        GridTrackListComponent::Track(names, _) => {
            assert_eq!(names, &["middle".to_string()]);
        }
        other => panic!("expected second row track, got {other:?}"),
    }

    let GridTrackList::Tracks {
        components: column_tracks,
        trailing_names: column_trailing_names,
    } = &style.grid_template_columns
    else {
        panic!("grid-template should set column tracks");
    };
    assert_eq!(column_tracks.len(), 2);
    assert_eq!(column_trailing_names, &["right".to_string()]);
    match &column_tracks[0] {
        GridTrackListComponent::Track(names, _) => assert_eq!(names, &["left".to_string()]),
        other => panic!("expected first column track, got {other:?}"),
    }
}

#[tokio::test]
async fn parses_grid_template_track_shorthand_and_none() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("grid-template: 10pt 1fr / 20pt 2fr"),
    );
    let GridTrackList::Tracks {
        components: row_tracks,
        ..
    } = &style.grid_template_rows
    else {
        panic!("grid-template should set rows");
    };
    let GridTrackList::Tracks {
        components: column_tracks,
        ..
    } = &style.grid_template_columns
    else {
        panic!("grid-template should set columns");
    };
    assert_eq!(row_tracks.len(), 2);
    assert_eq!(column_tracks.len(), 2);
    assert_eq!(style.grid_template_areas, GridTemplateAreas::None);

    apply_declarations(&mut style, &parse_declarations("grid-template: none"));
    assert_eq!(style.grid_template_rows, GridTrackList::None);
    assert_eq!(style.grid_template_columns, GridTrackList::None);
    assert_eq!(style.grid_template_areas, GridTemplateAreas::None);
}

#[tokio::test]
async fn invalid_grid_template_shorthand_does_not_partially_apply() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("grid-template: \"a a\" 10pt / 20pt 1fr"),
    );
    apply_declarations(
        &mut style,
        &parse_declarations("grid-template: \"bad bad\" \"bad main\" / 1fr 1fr"),
    );

    let GridTemplateAreas::Areas(rows) = &style.grid_template_areas else {
        panic!("valid initial grid-template areas should remain");
    };
    assert_eq!(
        rows[0].cells,
        [Some("a".to_string()), Some("a".to_string())]
    );
    let GridTrackList::Tracks {
        components: row_tracks,
        ..
    } = &style.grid_template_rows
    else {
        panic!("valid initial grid-template rows should remain");
    };
    let GridTrackList::Tracks {
        components: column_tracks,
        ..
    } = &style.grid_template_columns
    else {
        panic!("valid initial grid-template columns should remain");
    };
    assert_eq!(row_tracks.len(), 1);
    assert_eq!(column_tracks.len(), 2);
}

#[tokio::test]
async fn parses_grid_shorthand_template_form_and_resets_implicit_grid() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-auto-flow: column dense; grid-auto-rows: 5pt; grid-auto-columns: 6pt",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations("grid: \"a a\" 10pt / 20pt 1fr"),
    );

    let GridTemplateAreas::Areas(rows) = &style.grid_template_areas else {
        panic!("grid shorthand should set template areas");
    };
    assert_eq!(
        rows[0].cells,
        [Some("a".to_string()), Some("a".to_string())]
    );
    let GridTrackList::Tracks {
        components: row_tracks,
        ..
    } = &style.grid_template_rows
    else {
        panic!("grid shorthand should set template rows");
    };
    let GridTrackList::Tracks {
        components: column_tracks,
        ..
    } = &style.grid_template_columns
    else {
        panic!("grid shorthand should set template columns");
    };
    assert_eq!(row_tracks.len(), 1);
    assert_eq!(column_tracks.len(), 2);
    assert_eq!(style.grid_auto_flow, GridAutoFlow::Row);
    assert_eq!(style.grid_auto_rows.tracks, [GridTrackSize::AUTO]);
    assert_eq!(style.grid_auto_columns.tracks, [GridTrackSize::AUTO]);
}

#[tokio::test]
async fn parses_grid_shorthand_auto_flow_forms() {
    let declarations = parse_declarations("grid: auto-flow dense 7pt 8pt / [main] 1fr");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.grid_template_rows, GridTrackList::None);
    assert_eq!(style.grid_auto_flow, GridAutoFlow::RowDense);
    assert_eq!(style.grid_auto_rows.tracks.len(), 2);
    assert_eq!(style.grid_auto_columns.tracks, [GridTrackSize::AUTO]);
    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("row auto-flow grid shorthand should set columns");
    };
    assert_eq!(columns.len(), 1);

    let declarations = parse_declarations("grid: [row] 10pt / dense auto-flow 9pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.grid_template_columns, GridTrackList::None);
    assert_eq!(style.grid_auto_flow, GridAutoFlow::ColumnDense);
    assert_eq!(style.grid_auto_rows.tracks, [GridTrackSize::AUTO]);
    assert_eq!(style.grid_auto_columns.tracks.len(), 1);
    let GridTrackList::Tracks {
        components: rows, ..
    } = &style.grid_template_rows
    else {
        panic!("column auto-flow grid shorthand should set rows");
    };
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn invalid_grid_shorthand_does_not_partially_apply() {
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &parse_declarations("grid: 10pt / 20pt"));
    apply_declarations(&mut style, &parse_declarations("grid: 10pt 20pt"));

    let GridTrackList::Tracks {
        components: rows, ..
    } = &style.grid_template_rows
    else {
        panic!("valid initial grid rows should remain");
    };
    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("valid initial grid columns should remain");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(columns.len(), 1);
}

#[tokio::test]
async fn parses_flex_cross_axis_alignment() {
    let declarations = parse_declarations(
        "align-items: flex-end; align-self: end; align-content: space-evenly; justify-content: flex-end; flex-flow: wrap",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.align_items,
        AlignItems::new(SelfAlignmentKeyword::FlexEnd)
    );
    assert_eq!(style.align_self, AlignSelf::new(SelfAlignmentKeyword::End));
    assert_eq!(
        style.align_content,
        AlignContent::new(ContentAlignmentKeyword::SpaceEvenly)
    );
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::FlexEnd)
    );
    assert_eq!(style.flex_direction, FlexDirection::Row);
    assert_eq!(style.flex_wrap, FlexWrap::Wrap);
}

#[tokio::test]
async fn parses_flex_distributed_alignment_keywords() {
    let declarations = parse_declarations(
        "align-items: center; align-self: flex-start; align-content: space-around; justify-content: space-around",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.align_items,
        AlignItems::new(SelfAlignmentKeyword::Center)
    );
    assert_eq!(
        style.align_self,
        AlignSelf::new(SelfAlignmentKeyword::FlexStart)
    );
    assert_eq!(
        style.align_content,
        AlignContent::new(ContentAlignmentKeyword::SpaceAround)
    );
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::SpaceAround)
    );
}

#[tokio::test]
async fn parses_physical_justify_content_keywords() {
    let declarations = parse_declarations("justify-content: start");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::Start)
    );

    let declarations = parse_declarations("justify-content: end");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::End)
    );

    let declarations = parse_declarations("justify-content: left");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::Left)
    );

    let declarations = parse_declarations("justify-content: right");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::Right)
    );
}

#[tokio::test]
async fn parses_full_box_alignment_keyword_surface() {
    let mut style = default_style_for_tag("div");

    apply_declarations(
        &mut style,
        &parse_declarations("justify-content: normal; align-content: normal"),
    );
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::Normal)
    );
    assert_eq!(
        style.align_content,
        AlignContent::new(ContentAlignmentKeyword::Normal)
    );

    apply_declarations(
        &mut style,
        &parse_declarations(
            "justify-content: stretch; align-content: first baseline; align-items: self-start; align-self: self-end",
        ),
    );
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::Stretch)
    );
    assert_eq!(
        style.align_content,
        AlignContent::new(ContentAlignmentKeyword::Baseline)
    );
    assert_eq!(
        style.align_items,
        AlignItems::new(SelfAlignmentKeyword::SelfStart)
    );
    assert_eq!(
        style.align_self,
        AlignSelf::new(SelfAlignmentKeyword::SelfEnd)
    );
    assert_eq!(style.justify_content.safety, AlignmentSafety::Default);
    assert_eq!(style.align_content.safety, AlignmentSafety::Default);

    apply_declarations(
        &mut style,
        &parse_declarations(
            "justify-content: safe center; align-content: safe flex-end; align-items: unsafe start; align-self: safe end",
        ),
    );
    assert_eq!(
        style.justify_content.keyword,
        ContentAlignmentKeyword::Center
    );
    assert_eq!(style.justify_content.safety, AlignmentSafety::Safe);
    assert_eq!(
        style.align_content.keyword,
        ContentAlignmentKeyword::FlexEnd
    );
    assert_eq!(style.align_content.safety, AlignmentSafety::Safe);
    assert_eq!(style.align_items.keyword, SelfAlignmentKeyword::Start);
    assert_eq!(style.align_items.safety, AlignmentSafety::Unsafe);
    assert_eq!(style.align_self.keyword, SelfAlignmentKeyword::End);
    assert_eq!(style.align_self.safety, AlignmentSafety::Safe);

    apply_declarations(
        &mut style,
        &parse_declarations(
            "justify-content: unsafe normal; align-content: safe normal; align-items: safe normal; align-self: unsafe normal",
        ),
    );
    assert_eq!(
        style.justify_content.keyword,
        ContentAlignmentKeyword::Normal
    );
    assert_eq!(style.justify_content.safety, AlignmentSafety::Unsafe);
    assert_eq!(style.align_content.keyword, ContentAlignmentKeyword::Normal);
    assert_eq!(style.align_content.safety, AlignmentSafety::Safe);
    assert_eq!(style.align_items.keyword, SelfAlignmentKeyword::Normal);
    assert_eq!(style.align_items.safety, AlignmentSafety::Safe);
    assert_eq!(style.align_self.keyword, SelfAlignmentKeyword::Normal);
    assert_eq!(style.align_self.safety, AlignmentSafety::Unsafe);

    apply_declarations(
        &mut style,
        &parse_declarations(
            "justify-items: left; justify-self: safe right; align-items: safe stretch; justify-content: safe space-between",
        ),
    );
    assert_eq!(
        style.justify_items,
        JustifyItems::new(SelfAlignmentKeyword::Left)
    );
    assert_eq!(style.justify_self.keyword, SelfAlignmentKeyword::Right);
    assert_eq!(style.justify_self.safety, AlignmentSafety::Safe);
    assert_eq!(
        style.align_items.keyword,
        SelfAlignmentKeyword::Normal,
        "invalid safe stretch must not override the previous align-items value"
    );
    assert_eq!(
        style.justify_content.keyword,
        ContentAlignmentKeyword::Normal,
        "invalid safe space-between must not override the previous justify-content value"
    );

    apply_declarations(
        &mut style,
        &parse_declarations("align-content: unsafe last baseline; align-items: unsafe stretch"),
    );
    assert_eq!(
        style.align_content.keyword,
        ContentAlignmentKeyword::Normal,
        "invalid unsafe last baseline must not override the previous align-content value"
    );
    assert_eq!(
        style.align_items.keyword,
        SelfAlignmentKeyword::Normal,
        "invalid unsafe stretch must not override the previous align-items value"
    );
}

#[tokio::test]
async fn parses_text_align_last_keywords() {
    let declarations =
        parse_declarations("direction: rtl; text-align: start; text-align-last: end");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.text_align, TextAlign::Start);
    assert_eq!(style.text_align_last, TextAlignLast::Align(TextAlign::End));
    assert_eq!(style.text_align.physical(style.direction), TextAlign::Right);

    let declarations = parse_declarations("text-align: center; text-align-last: justify");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.text_align, TextAlign::Center);
    assert_eq!(
        style.text_align_last,
        TextAlignLast::Align(TextAlign::Justify)
    );
}

#[tokio::test]
async fn parses_text_align_all_and_shorthand_reset() {
    let declarations =
        parse_declarations("text-align-last: justify; text-align-all: center; text-align: right");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.text_align, TextAlign::Right);
    assert_eq!(style.text_align_last, TextAlignLast::Auto);

    let declarations = parse_declarations("text-align-last: justify; text-align-all: center");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.text_align, TextAlign::Center);
    assert_eq!(
        style.text_align_last,
        TextAlignLast::Align(TextAlign::Justify)
    );
}

#[tokio::test]
async fn text_align_match_parent_resolves_against_parent_direction() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "section { direction: rtl; text-align: start; text-align-last: end }\
         p { direction: ltr; text-align-all: match-parent; text-align-last: match-parent }",
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[ElementSignature::new("section", HashMap::new())],
    );

    assert_eq!(parent.direction, Direction::Rtl);
    assert_eq!(child.direction, Direction::Ltr);
    assert_eq!(child.text_align, TextAlign::Right);
    assert_eq!(child.text_align_last, TextAlignLast::Align(TextAlign::Left));
}

#[tokio::test]
async fn applies_stylesheet_text_align_end_with_rtl_direction() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".test { text-align: end; direction: rtl; }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([("class".to_string(), "test".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.direction, Direction::Rtl);
    assert_eq!(style.text_align, TextAlign::End);
    assert_eq!(style.text_align.physical(style.direction), TextAlign::Left);
}

#[tokio::test]
async fn parses_text_align_justify_all_keyword() {
    let declarations = parse_declarations("text-align: justify-all; text-align-last: auto");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.text_align, TextAlign::JustifyAll);
    assert_eq!(style.text_align_last, TextAlignLast::Auto);
}

#[tokio::test]
async fn parses_and_inherits_tab_size() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "section { tab-size: 4 } p { tab-size: inherit }",
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[ElementSignature::new("section", HashMap::new())],
    );

    assert_eq!(parent.tab_size, TabSize::Spaces(4.0));
    assert_eq!(child.tab_size, TabSize::Spaces(4.0));

    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &parse_declarations("tab-size: 12pt"));
    assert_eq!(
        style.tab_size,
        TabSize::Length(ComputedLengthPercentage::from_points(12.0))
    );
    apply_declarations(&mut style, &parse_declarations("tab-size: normal"));
    assert_eq!(style.tab_size, TabSize::INITIAL);
    apply_declarations(&mut style, &parse_declarations("tab-size: -1"));
    assert_eq!(style.tab_size, TabSize::INITIAL);
}

#[tokio::test]
async fn parses_unicode_bidi_keywords() {
    let declarations = parse_declarations(
        "unicode-bidi: embed; unicode-bidi: isolate; unicode-bidi: bidi-override; unicode-bidi: isolate-override; unicode-bidi: plaintext",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.unicode_bidi, UnicodeBidi::Plaintext);

    apply_declarations(&mut style, &parse_declarations("unicode-bidi: normal"));
    assert_eq!(style.unicode_bidi, UnicodeBidi::Normal);
}

#[tokio::test]
async fn parses_text_justify_keywords() {
    let declarations = parse_declarations("text-justify: inter-character");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.text_justify, TextJustify::InterCharacter);
}

#[tokio::test]
async fn parses_text_orientation_keywords() {
    let declarations = parse_declarations("text-orientation: upright");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.text_orientation, TextOrientation::Upright);

    apply_declarations(
        &mut style,
        &parse_declarations("text-orientation: sideways"),
    );
    assert_eq!(style.text_orientation, TextOrientation::Sideways);

    apply_declarations(&mut style, &parse_declarations("text-orientation: invalid"));
    assert_eq!(style.text_orientation, TextOrientation::Sideways);
}

#[tokio::test]
async fn parses_text_autospace_keyword_set() {
    let declarations =
        parse_declarations("text-autospace: ideograph-alpha ideograph-numeric insert");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(style.text_autospace.ideograph_alpha);
    assert!(style.text_autospace.ideograph_numeric);
    assert!(!style.text_autospace.punctuation);

    apply_declarations(
        &mut style,
        &parse_declarations("text-autospace: no-autospace"),
    );
    assert_eq!(style.text_autospace, TextAutospace::NONE);
}

#[tokio::test]
async fn parses_hanging_punctuation_keyword_set() {
    let declarations = parse_declarations("hanging-punctuation: last first force-end");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(style.hanging_punctuation.last);
    assert!(style.hanging_punctuation.first);
    assert!(style.hanging_punctuation.force_end);
    assert!(!style.hanging_punctuation.allow_end);

    apply_declarations(&mut style, &parse_declarations("hanging-punctuation: none"));
    assert!(!style.hanging_punctuation.last);
    assert!(!style.hanging_punctuation.first);
    assert!(!style.hanging_punctuation.force_end);
    assert!(!style.hanging_punctuation.allow_end);
}

#[tokio::test]
async fn invalid_hanging_punctuation_does_not_override_previous_value() {
    let declarations = parse_declarations(
        "hanging-punctuation: last; hanging-punctuation: last last; hanging-punctuation: force-end allow-end",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(style.hanging_punctuation.last);
    assert!(!style.hanging_punctuation.force_end);
    assert!(!style.hanging_punctuation.allow_end);
}

#[tokio::test]
async fn parses_text_indent_length_percentage_and_modifiers() {
    let declarations = parse_declarations("text-indent: calc(3pt + 25%) hanging each-line");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!((style.text_indent.amount.length_points() - 3.0).abs() < 0.001);
    assert!((style.text_indent.amount.percent - 0.25).abs() < 0.001);
    assert!(style.text_indent.hanging);
    assert!(style.text_indent.each_line);
}

#[tokio::test]
async fn parses_font_shorthand_size_line_height_and_family() {
    let declarations = parse_declarations("font: italic 700 condensed 10px/1 Ahem");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_style, FontStyle::Italic);
    assert_eq!(style.font_weight, FontWeight::BOLD);
    assert_eq!(style.font_width, FontWidth::CONDENSED);
    assert!((style.font_size - 7.5).abs() < 0.001);
    assert_eq!(style.line_height_value, ComputedLineHeight::Number(1.0));
    assert!((style.line_height - 7.5).abs() < 0.001);
    assert!(!style.line_height_is_normal);
    assert_eq!(
        style.font_family,
        FontFamily::Names(vec!["Ahem".to_string()])
    );
}

#[tokio::test]
async fn font_shorthand_number_line_height_projects_from_shorthand_size() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div { font: 50px/1 Ahem; } .test { width: 2em; color: green; background: green; }",
    ));
    let parent = default_style_for_tag("body");
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([("class".to_string(), "test".to_string())]),
        ),
        None,
        &[stylesheet],
        Some(&parent),
        &[ElementSignature::new("body", HashMap::new())],
    );

    assert!((style.font_size - 37.5).abs() < 0.001);
    assert_eq!(style.line_height_value, ComputedLineHeight::Number(1.0));
    assert!((style.line_height - 37.5).abs() < 0.001);
    assert!(!style.line_height_is_normal);
}

#[tokio::test]
async fn font_face_stylesheet_does_not_reset_author_font_shorthand_line_height() {
    let author = parse_stylesheet(&Css::from_string(
        "div { font: 50px/1 Ahem; } .test { width: 2em; color: green; background: green; }",
    ));
    let font_face = parse_stylesheet(&Css::from_string(
        "@font-face { font-family: 'Ahem'; src: url('/fonts/Ahem.ttf'); }",
    ));
    let parent = default_style_for_tag("body");
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([("class".to_string(), "test".to_string())]),
        ),
        None,
        &[author, font_face],
        Some(&parent),
        &[ElementSignature::new("body", HashMap::new())],
    );

    assert_eq!(style.line_height_value, ComputedLineHeight::Number(1.0));
    assert!((style.line_height - 37.5).abs() < 0.001);
    assert!(!style.line_height_is_normal);
}

#[tokio::test]
async fn parses_flex_baseline_alignment_keywords() {
    let declarations = parse_declarations("align-items: baseline; align-self: first baseline");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.align_items,
        AlignItems::new(SelfAlignmentKeyword::Baseline)
    );
    assert_eq!(
        style.align_self,
        AlignSelf::new(SelfAlignmentKeyword::Baseline)
    );

    let declarations = parse_declarations("align-items: last baseline; align-self: last baseline");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.align_items,
        AlignItems::new(SelfAlignmentKeyword::LastBaseline)
    );
    assert_eq!(
        style.align_self,
        AlignSelf::new(SelfAlignmentKeyword::LastBaseline)
    );
}

#[tokio::test]
async fn parses_box_alignment_place_shorthands() {
    let declarations = parse_declarations(
        "place-content: space-between center; place-items: last baseline right; place-self: safe center left",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.align_content,
        AlignContent::new(ContentAlignmentKeyword::SpaceBetween)
    );
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::Center)
    );
    assert_eq!(
        style.align_items,
        AlignItems::new(SelfAlignmentKeyword::LastBaseline)
    );
    assert_eq!(
        style.justify_items,
        JustifyItems::new(SelfAlignmentKeyword::Right)
    );
    assert_eq!(style.align_self.keyword, SelfAlignmentKeyword::Center);
    assert_eq!(style.align_self.safety, AlignmentSafety::Safe);
    assert_eq!(
        style.justify_self,
        JustifySelf::new(SelfAlignmentKeyword::Left)
    );

    let declarations = parse_declarations("place-content: flex-end; place-items: center");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.align_content,
        AlignContent::new(ContentAlignmentKeyword::FlexEnd)
    );
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::FlexEnd)
    );
    assert_eq!(
        style.align_items,
        AlignItems::new(SelfAlignmentKeyword::Center)
    );
    assert_eq!(
        style.justify_items,
        JustifyItems::new(SelfAlignmentKeyword::Center)
    );

    let declarations = parse_declarations(
        "justify-content: end; place-content: first baseline; place-content: last baseline",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.align_content,
        AlignContent::new(ContentAlignmentKeyword::LastBaseline)
    );
    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::Start)
    );

    let declarations = parse_declarations("justify-content: baseline");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.justify_content,
        JustifyContent::new(ContentAlignmentKeyword::Start),
        "baseline remains invalid as a justify-content longhand value"
    );
}

#[tokio::test]
async fn flex_flow_shorthand_resets_omitted_components_to_initial_values() {
    let declarations = parse_declarations("flex-direction: column; flex-flow: wrap");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.flex_direction, FlexDirection::Row);
    assert_eq!(style.flex_wrap, FlexWrap::Wrap);
}

#[tokio::test]
async fn parses_reverse_flex_direction_and_wrap_values() {
    let declarations = parse_declarations(
        "flex-direction: column-reverse; flex-wrap: wrap-reverse; flex-flow: row-reverse wrap-reverse",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.flex_direction, FlexDirection::RowReverse);
    assert_eq!(style.flex_wrap, FlexWrap::WrapReverse);
}

#[tokio::test]
async fn parses_flex_order_property() {
    let declarations = parse_declarations("order: -2");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.order, -2);
}

#[tokio::test]
async fn parses_aspect_ratio_property() {
    let cases = [
        ("aspect-ratio: auto", true, None),
        ("aspect-ratio: 1", false, Some(1.0)),
        ("aspect-ratio: 16 / 9", false, Some(16.0 / 9.0)),
        ("aspect-ratio: auto 1 / 1", true, Some(1.0)),
        ("aspect-ratio: 4 / 3 auto", true, Some(4.0 / 3.0)),
    ];

    for (declaration, expected_auto, expected_ratio) in cases {
        let declarations = parse_declarations(declaration);
        let mut style = default_style_for_tag("div");
        apply_declarations(&mut style, &declarations);

        assert_eq!(
            style.aspect_ratio.auto, expected_auto,
            "declaration: {declaration}"
        );
        match (style.aspect_ratio.ratio, expected_ratio) {
            (Some(actual), Some(expected)) => assert!(
                (actual - expected).abs() < 0.0001,
                "declaration: {declaration}, actual: {actual}, expected: {expected}"
            ),
            (None, None) => {}
            (actual, expected) => {
                panic!("declaration: {declaration}, actual: {actual:?}, expected: {expected:?}")
            }
        }
    }
}

#[test]
fn resolves_replaced_preferred_aspect_ratio_fallback() {
    assert_eq!(
        AspectRatio::AUTO.preferred_ratio(true, Some(2.5)),
        Some(2.5)
    );
    assert_eq!(
        AspectRatio::from_ratio(1.0).preferred_ratio(true, Some(2.5)),
        Some(1.0)
    );
    assert_eq!(
        AspectRatio::auto_with_ratio(1.5).preferred_ratio(true, Some(2.5)),
        Some(2.5)
    );
    assert_eq!(
        AspectRatio::auto_with_ratio(1.5).preferred_ratio(true, None),
        Some(1.5)
    );
    assert_eq!(
        AspectRatio::auto_with_ratio(1.5).preferred_ratio(false, Some(2.5)),
        Some(1.5)
    );
}

#[tokio::test]
async fn invalid_aspect_ratio_declarations_are_ignored() {
    for invalid in [
        "aspect-ratio: 0",
        "aspect-ratio: -1",
        "aspect-ratio: 1 /",
        "aspect-ratio: 1 / 0",
        "aspect-ratio: 1 2",
        "aspect-ratio: auto 1 auto",
    ] {
        let declarations = parse_declarations("aspect-ratio: 3 / 2");
        let mut style = default_style_for_tag("div");
        apply_declarations(&mut style, &declarations);
        apply_declarations(&mut style, &parse_declarations(invalid));

        assert!(!style.aspect_ratio.auto, "invalid: {invalid}");
        assert_eq!(style.aspect_ratio.ratio, Some(1.5), "invalid: {invalid}");
    }
}

#[tokio::test]
async fn parses_float_values() {
    let declarations = parse_declarations("float: left");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.float, Float::Left);

    let declarations = parse_declarations("float: right");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.float, Float::Right);

    let declarations = parse_declarations("float: none");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.float, Float::None);

    let declarations = parse_declarations("float: inline-start");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.float, Float::InlineStart);

    let declarations = parse_declarations("float: inline-end");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.float, Float::InlineEnd);
}

#[tokio::test]
async fn parses_clear_values() {
    let mut style = default_style_for_tag("div");
    for (value, expected) in [
        ("left", Clear::Left),
        ("right", Clear::Right),
        ("both", Clear::Both),
        ("inline-start", Clear::InlineStart),
        ("inline-end", Clear::InlineEnd),
        ("none", Clear::None),
    ] {
        let declarations = parse_declarations(&format!("clear: {value}"));
        apply_declarations(&mut style, &declarations);
        assert_eq!(style.clear, expected);
    }
}

#[tokio::test]
async fn parses_flex_shrink_and_shorthand() {
    let mut explicit = default_style_for_tag("div");
    apply_declarations(&mut explicit, &parse_declarations("flex-shrink: 0"));
    assert_eq!(explicit.flex_shrink, 0.0);
    apply_declarations(
        &mut explicit,
        &parse_declarations("flex-grow: 2; flex-shrink: 3; flex-grow: -1; flex-shrink: -1"),
    );
    assert_eq!(explicit.flex_grow, 2.0);
    assert_eq!(explicit.flex_shrink, 3.0);

    let mut shorthand = default_style_for_tag("div");
    apply_declarations(&mut shorthand, &parse_declarations("flex: 2 3 40pt"));
    assert_eq!(shorthand.flex_grow, 2.0);
    assert_eq!(shorthand.flex_shrink, 3.0);
    assert_eq!(
        shorthand.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(40.0))
    );

    let mut unitless_zero_basis = default_style_for_tag("div");
    apply_declarations(&mut unitless_zero_basis, &parse_declarations("flex: 4 1 0"));
    assert_eq!(unitless_zero_basis.flex_grow, 4.0);
    assert_eq!(unitless_zero_basis.flex_shrink, 1.0);
    assert_eq!(
        unitless_zero_basis.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(0.0))
    );

    let mut single = default_style_for_tag("div");
    apply_declarations(&mut single, &parse_declarations("flex: 1"));
    assert_eq!(single.flex_grow, 1.0);
    assert_eq!(single.flex_shrink, 1.0);
    assert_eq!(
        single.flex_basis,
        flex_basis_percentage(ComputedLengthPercentage::from_percent(0.0))
    );

    let mut explicit_zero_px = default_style_for_tag("div");
    apply_declarations(
        &mut explicit_zero_px,
        &parse_declarations("flex-basis: 0px"),
    );
    assert_eq!(
        explicit_zero_px.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(0.0))
    );

    let mut explicit_zero_percent = default_style_for_tag("div");
    apply_declarations(
        &mut explicit_zero_percent,
        &parse_declarations("flex-basis: 0%"),
    );
    assert_eq!(
        explicit_zero_percent.flex_basis,
        flex_basis_percentage(ComputedLengthPercentage::from_percent(0.0))
    );

    let mut none = default_style_for_tag("div");
    apply_declarations(&mut none, &parse_declarations("flex: none"));
    assert_eq!(none.flex_grow, 0.0);
    assert_eq!(none.flex_shrink, 0.0);
    assert_eq!(none.flex_basis, ComputedFlexBasis::Auto);

    let mut content = default_style_for_tag("div");
    apply_declarations(&mut content, &parse_declarations("flex: 0 1 content"));
    assert_eq!(content.flex_grow, 0.0);
    assert_eq!(content.flex_shrink, 1.0);
    assert_eq!(content.flex_basis, ComputedFlexBasis::Content);

    let mut intrinsic = default_style_for_tag("div");
    apply_declarations(
        &mut intrinsic,
        &parse_declarations("flex-basis: min-content"),
    );
    assert_eq!(intrinsic.flex_basis, ComputedFlexBasis::MinContent);
    apply_declarations(&mut intrinsic, &parse_declarations("flex: 0 0 max-content"));
    assert_eq!(intrinsic.flex_basis, ComputedFlexBasis::MaxContent);
    apply_declarations(
        &mut intrinsic,
        &parse_declarations("flex-basis: fit-content"),
    );
    assert_eq!(intrinsic.flex_basis, ComputedFlexBasis::FitContent(None));
    apply_declarations(
        &mut intrinsic,
        &parse_declarations("flex: 0 0 fit-content(30pt)"),
    );
    assert_eq!(
        intrinsic.flex_basis,
        ComputedFlexBasis::FitContent(Some(ComputedLengthPercentage::from_points(30.0)))
    );
    apply_declarations(
        &mut intrinsic,
        &parse_declarations("flex-basis: calc(20pt + 10pt)"),
    );
    assert_eq!(
        intrinsic.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(30.0))
    );
    apply_declarations(
        &mut intrinsic,
        &parse_declarations("flex: 0 0 calc(20pt + 11pt)"),
    );
    assert_eq!(
        intrinsic.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(31.0))
    );
    apply_declarations(
        &mut intrinsic,
        &parse_declarations("flex: 0 0 clamp(20pt, 40pt, 30pt)"),
    );
    assert_eq!(
        intrinsic.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(30.0))
    );

    let mut basis_only = default_style_for_tag("div");
    apply_declarations(&mut basis_only, &parse_declarations("flex: 20pt"));
    assert_eq!(basis_only.flex_grow, 1.0);
    assert_eq!(basis_only.flex_shrink, 1.0);
    assert_eq!(
        basis_only.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(20.0))
    );

    let mut grow_basis = default_style_for_tag("div");
    apply_declarations(
        &mut grow_basis,
        &parse_declarations("flex-shrink: 0; flex: 2 30pt"),
    );
    assert_eq!(grow_basis.flex_grow, 2.0);
    assert_eq!(grow_basis.flex_shrink, 1.0);
    assert_eq!(
        grow_basis.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(30.0))
    );

    apply_declarations(
        &mut grow_basis,
        &parse_declarations("flex-basis: -10pt; flex: 4 4 -20pt"),
    );
    assert_eq!(grow_basis.flex_grow, 2.0);
    assert_eq!(grow_basis.flex_shrink, 1.0);
    assert_eq!(
        grow_basis.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(30.0))
    );

    apply_declarations(&mut grow_basis, &parse_declarations("flex: 4 1 1"));
    assert_eq!(grow_basis.flex_grow, 2.0);
    assert_eq!(grow_basis.flex_shrink, 1.0);
    assert_eq!(
        grow_basis.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(30.0))
    );
}

#[tokio::test]
async fn semantic_flow_elements_default_to_block_display() {
    for tag in [
        "section",
        "article",
        "header",
        "footer",
        "main",
        "center",
        "dir",
        "dl",
        "dt",
        "dd",
        "hgroup",
        "legend",
        "listing",
        "menu",
        "plaintext",
        "xmp",
    ] {
        assert_eq!(default_style_for_tag(tag).display, Display::BLOCK, "{tag}");
    }
    assert_eq!(default_style_for_tag("dd").margin.left, 30.0);
    assert_eq!(default_style_for_tag("body").margin, edge_all(6.0));
}

#[tokio::test]
async fn ua_em_margins_resolve_against_computed_font_size() {
    let ua = html5_user_agent_stylesheet();
    let parent = ComputedStyle {
        font_size: 11.0,
        line_height: 17.6,
        line_height_multiplier: None,
        line_height_is_normal: false,
        ..ComputedStyle::initial()
    };
    let style = style_for_element_with_signature(
        ElementSignature::new("dl", HashMap::new()),
        None,
        std::slice::from_ref(&ua),
        Some(&parent),
        &[],
    );

    assert_eq!(style.margin.top, 11.0);
    assert_eq!(style.margin.bottom, 11.0);

    let overridden = style_for_element_with_signature(
        ElementSignature::new("dl", HashMap::new()),
        Some("margin: 0"),
        std::slice::from_ref(&ua),
        Some(&parent),
        &[],
    );

    assert_eq!(overridden.margin, Edges::ZERO);
}

#[tokio::test]
async fn embedded_html5_ua_stylesheet_matches_weasyprint_defaults() {
    let stylesheet = html5_user_agent_stylesheet();
    let source = html5_user_agent_source();

    assert_eq!(stylesheet.origin, StylesheetOrigin::UserAgent);
    assert!(source.contains("User agent stylsheet for HTML."));
    assert!(source.contains("body { margin: 8px }"));
    assert!(
        source
            .contains("blockquote, dir, dl, figure, listing, menu, ol, p, plaintext, pre, ul, xmp")
    );
    assert!(source.contains("h1 { bookmark-level: 1 }"));
    assert!(
        stylesheet
            .counter_styles
            .iter()
            .any(|style| style.name == "decimal")
    );
}

#[tokio::test]
async fn author_rules_override_user_agent_stylesheet_at_equal_specificity() {
    let ua = html5_user_agent_stylesheet();
    let author = parse_stylesheet(&Css::from_string("p { margin: 0; font-size: 10pt }"));
    let parent = default_style_for_tag("body");
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[ua, author],
        Some(&parent),
        &[],
    );

    assert_eq!(style.margin, Edges::ZERO);
    assert_eq!(style.font_size, 10.0);
}

#[tokio::test]
async fn presentational_hints_use_author_origin_zero_specificity() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let author = parse_stylesheet(&Css::from_string("td { vertical-align: top }"));
    let parent = default_style_for_tag("tr");
    let mut attrs = HashMap::new();
    attrs.insert("valign".to_string(), "bottom".to_string());

    let hinted = style_for_element_with_signature(
        ElementSignature::new("td", attrs.clone()),
        None,
        &[ua.clone(), hints.clone()],
        Some(&parent),
        &[],
    );
    let overridden = style_for_element_with_signature(
        ElementSignature::new("td", attrs),
        None,
        &[ua, hints, author],
        Some(&parent),
        &[],
    );

    assert_eq!(
        hinted.vertical_align.table_cell_align,
        TableCellVerticalAlign::Bottom
    );
    assert_eq!(
        overridden.vertical_align.table_cell_align,
        TableCellVerticalAlign::Top
    );
}

#[tokio::test]
async fn table_rules_groups_presentational_hint_sets_group_border_color() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let author = parse_stylesheet(&Css::from_string("#b > * { border-block-end-color: blue }"));
    let mut table_attrs = HashMap::new();
    table_attrs.insert("id".to_string(), "b".to_string());
    table_attrs.insert("rules".to_string(), "groups".to_string());
    let table_signature = ElementSignature::new("table", table_attrs);
    let table_style = style_for_element_with_signature(
        table_signature.clone(),
        None,
        &[ua.clone(), hints.clone()],
        None,
        &[],
    );

    let hinted = style_for_element_with_signature(
        ElementSignature::new("thead", HashMap::new()),
        None,
        &[ua.clone(), hints.clone()],
        Some(&table_style),
        std::slice::from_ref(&table_signature),
    );
    let overridden = style_for_element_with_signature(
        ElementSignature::new("thead", HashMap::new()),
        None,
        &[ua, hints, author],
        Some(&table_style),
        &[table_signature],
    );

    assert_eq!(hinted.border_colors.bottom, Color::new(128, 128, 128));
    assert_eq!(hinted.border_styles.bottom, BorderStyle::Solid);
    assert_eq!(hinted.border_widths.bottom, 0.75);
    assert_eq!(overridden.border_colors.bottom, Color::new(0, 0, 255));
}

#[tokio::test]
async fn hr_dynamic_presentational_hints_are_optional() {
    let ua = html5_user_agent_stylesheet();
    let mut attrs = HashMap::new();
    attrs.insert("width".to_string(), "100".to_string());
    attrs.insert("size".to_string(), "8".to_string());
    attrs.insert("color".to_string(), "red".to_string());

    let style = style_for_element_with_signature(
        ElementSignature::new("hr", attrs),
        None,
        &[ua],
        None,
        &[],
    );

    assert!(style.box_values.width.is_auto());
    assert!(style.box_values.height.is_auto());
    assert_eq!(style.color, Color::new(128, 128, 128));
    assert_eq!(style.border_widths.top, CSS_PX_TO_PT);
}

#[tokio::test]
async fn hr_width_presentational_hint_maps_html_dimensions() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let mut px_attrs = HashMap::new();
    px_attrs.insert("width".to_string(), "100".to_string());
    let mut percent_attrs = HashMap::new();
    percent_attrs.insert("width".to_string(), "50%".to_string());
    let mut invalid_attrs = HashMap::new();
    invalid_attrs.insert("width".to_string(), "invalid".to_string());

    let px = style_for_element_with_signature(
        ElementSignature::new("hr", px_attrs),
        None,
        &[ua.clone(), hints.clone()],
        None,
        &[],
    );
    let percent = style_for_element_with_signature(
        ElementSignature::new("hr", percent_attrs),
        None,
        &[ua.clone(), hints.clone()],
        None,
        &[],
    );
    let invalid = style_for_element_with_signature(
        ElementSignature::new("hr", invalid_attrs),
        None,
        &[ua, hints],
        None,
        &[],
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(px_width) = px.box_values.width else {
        panic!("width=100 should map to a length");
    };
    assert!((px_width.length_points() - 100.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert_eq!(px_width.percent, 0.0);

    let ComputedLengthPercentageOrAuto::LengthPercentage(percent_width) = percent.box_values.width
    else {
        panic!("width=50% should map to a percentage");
    };
    assert_eq!(percent_width.length_points(), 0.0);
    assert!((percent_width.percent - 0.5).abs() < 0.001);

    assert!(invalid.box_values.width.is_auto());
}

#[tokio::test]
async fn hr_size_presentational_hint_maps_border_and_height() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let mut size_one_attrs = HashMap::new();
    size_one_attrs.insert("size".to_string(), "1".to_string());
    let mut size_eight_attrs = HashMap::new();
    size_eight_attrs.insert("size".to_string(), "8".to_string());
    let mut solid_size_attrs = HashMap::new();
    solid_size_attrs.insert("size".to_string(), "10".to_string());
    solid_size_attrs.insert("noshade".to_string(), "".to_string());

    let size_one = style_for_element_with_signature(
        ElementSignature::new("hr", size_one_attrs),
        None,
        &[ua.clone(), hints.clone()],
        None,
        &[],
    );
    let size_eight = style_for_element_with_signature(
        ElementSignature::new("hr", size_eight_attrs),
        None,
        &[ua.clone(), hints.clone()],
        None,
        &[],
    );
    let solid_size = style_for_element_with_signature(
        ElementSignature::new("hr", solid_size_attrs),
        None,
        &[ua, hints],
        None,
        &[],
    );

    assert_eq!(size_one.border_widths.bottom, 0.0);
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = size_eight.box_values.height
    else {
        panic!("size=8 should map to height");
    };
    assert!((height.length_points() - 6.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert_eq!(solid_size.border_styles.top, BorderStyle::Solid);
    assert_eq!(solid_size.border_widths.top, 5.0 * CSS_PX_TO_PT);
    assert_eq!(solid_size.border_widths.right, 5.0 * CSS_PX_TO_PT);
    assert_eq!(solid_size.border_widths.bottom, 5.0 * CSS_PX_TO_PT);
    assert_eq!(solid_size.border_widths.left, 5.0 * CSS_PX_TO_PT);
}

#[tokio::test]
async fn hr_color_presentational_hint_maps_color() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let mut attrs = HashMap::new();
    attrs.insert("color".to_string(), "red".to_string());

    let style = style_for_element_with_signature(
        ElementSignature::new("hr", attrs),
        None,
        &[ua, hints],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
    assert_eq!(style.border_color, Color::new(255, 0, 0));
    assert_eq!(style.border_colors.top, Color::new(255, 0, 0));
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
}

#[tokio::test]
async fn author_css_overrides_dynamic_hr_presentational_hints() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let author = parse_stylesheet(&Css::from_string(
        "hr { width: 25pt; color: blue; height: 2pt; border-width: 1pt }",
    ));
    let mut attrs = HashMap::new();
    attrs.insert("width".to_string(), "100".to_string());
    attrs.insert("size".to_string(), "10".to_string());
    attrs.insert("color".to_string(), "red".to_string());

    let style = style_for_element_with_signature(
        ElementSignature::new("hr", attrs),
        None,
        &[ua, hints, author],
        None,
        &[],
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("author width should win");
    };
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = style.box_values.height else {
        panic!("author height should win");
    };
    assert_eq!(width.length_points(), 25.0);
    assert_eq!(height.length_points(), 2.0);
    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.border_widths.top, 1.0);
}

#[tokio::test]
async fn ua_stylesheet_applies_inherited_semantic_font_defaults_once() {
    let ua = html5_user_agent_stylesheet();
    let parent = ComputedStyle {
        font_size: 12.0,
        line_height: 14.4,
        line_height_multiplier: Some(1.2),
        line_height_is_normal: true,
        ..default_style_for_tag("body")
    };

    let sub = style_for_element_with_signature(
        ElementSignature::new("sub", HashMap::new()),
        None,
        std::slice::from_ref(&ua),
        Some(&parent),
        &[],
    );
    let pre = style_for_element_with_signature(
        ElementSignature::new("pre", HashMap::new()),
        None,
        &[ua],
        Some(&parent),
        &[],
    );

    assert_eq!(sub.font_size, 10.0);
    assert_eq!(sub.vertical_align.baseline_shift, BaselineShift::Sub);
    assert!(sub.line_height_is_normal);
    assert_eq!(pre.font_family, FontFamily::Monospace);
    assert_eq!(pre.white_space, WhiteSpace::Pre);
}

#[tokio::test]
async fn list_item_ua_default_has_no_margin() {
    let style = default_style_for_tag("li");

    assert!(style.display.is_list_item());
    assert_eq!(style.margin, Edges::ZERO);
}

#[tokio::test]
async fn parses_auto_margins() {
    let declarations = parse_declarations("margin: 1pt auto 2pt 3pt; margin-left: auto");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.margin.top, 1.0);
    assert_eq!(style.margin.right, 0.0);
    assert_eq!(style.margin.bottom, 2.0);
    assert_eq!(style.margin.left, 0.0);
    assert!(!style.box_values.margin.top.is_auto());
    assert!(style.box_values.margin.right.is_auto());
    assert!(!style.box_values.margin.bottom.is_auto());
    assert!(style.box_values.margin.left.is_auto());
}

#[tokio::test]
async fn parses_side_specific_border_widths() {
    let declarations = parse_declarations("border-top: 2px solid red; border-bottom-width: 3px");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_width, 2.25);
    assert_eq!(style.border_widths.top, 1.5);
    assert_eq!(style.border_widths.right, 0.0);
    assert_eq!(style.border_widths.bottom, 2.25);
    assert_eq!(style.border_widths.left, 0.0);
    assert_eq!(style.border_color, Color::new(255, 0, 0));
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_styles.bottom, BorderStyle::None);
    assert_eq!(style.border_colors.top, Color::new(255, 0, 0));
}

#[tokio::test]
async fn ch_border_width_preserves_font_metric_component_until_used_resolution() {
    let declarations =
        parse_declarations("border-width: 2ch 1pt; border-style: solid; border-color: green");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.border_width_values.top,
        ComputedLengthPercentage::from_ch(2.0)
    );
    assert_eq!(
        style.border_width_values.right,
        ComputedLengthPercentage::from_points(1.0)
    );
    assert_eq!(style.border_widths.top, 0.0);
    assert_eq!(style.border_widths.right, 1.0);

    style.resolve_font_metric_lengths(5.0);

    assert_eq!(
        style.border_width_values.top,
        ComputedLengthPercentage::from_points(10.0)
    );
    assert_eq!(style.border_widths.top, 10.0);
    assert_eq!(style.border_widths.right, 1.0);
    assert_eq!(style.border_width, 10.0);
}

#[tokio::test]
async fn parses_side_specific_dotted_border_shorthand() {
    let declarations = parse_declarations("border-top: 2pt dotted blue");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Dotted);
    assert_eq!(style.border_colors.top, Color::new(0, 0, 255));
}

#[tokio::test]
async fn inline_style_parses_side_specific_dotted_border_shorthand() {
    let style = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("border-top: 2pt dotted blue"),
        &[html5_user_agent_stylesheet()],
        None,
        &[],
    );

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Dotted);
    assert_eq!(style.border_colors.top, Color::new(0, 0, 255));
}

#[tokio::test]
async fn parses_border_styles_and_side_colors() {
    let declarations = parse_declarations(
        "color: blue; border: dashed; border-left: 4pt double; border-color: red green blue black",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_widths.top, 2.25);
    assert_eq!(style.border_styles.top, BorderStyle::Dashed);
    assert_eq!(style.border_widths.left, 4.0);
    assert_eq!(style.border_styles.left, BorderStyle::Double);
    assert_eq!(style.border_colors.top, Color::new(255, 0, 0));
    assert_eq!(style.border_colors.right, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, Color::new(0, 0, 255));
    assert_eq!(style.border_colors.left, Color::BLACK);
}

#[tokio::test]
async fn parses_border_shorthand_color_functions_as_single_components() {
    let declarations = parse_declarations("border: 2pt solid rgb(255 0 0)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_colors.top, Color::new(255, 0, 0));
}

#[tokio::test]
async fn parses_border_current_color_against_computed_color() {
    let declarations = parse_declarations(
        "border: 2pt solid currentColor; border-left-color: rgb(0 0 255); color: green",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.right, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.left, Color::new(0, 0, 255));
}

#[tokio::test]
async fn parses_border_color_shorthand_with_rgb_functions() {
    let declarations = parse_declarations(
        "border-color: rgb(255 0 0) rgb(0 128 0) rgb(0 0 255) currentColor; color: black",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, Color::new(255, 0, 0));
    assert_eq!(style.border_colors.right, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, Color::new(0, 0, 255));
    assert_eq!(style.border_colors.left, Color::BLACK);
}

#[tokio::test]
async fn maps_logical_border_properties_to_initial_physical_sides() {
    let declarations = parse_declarations(
        "border-block-start: 2pt solid red; \
         border-block-end-width: 3pt; border-block-end-style: dashed; border-block-end-color: blue; \
         border-inline: 4pt dotted green; border-inline-end-color: black",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_colors.top, Color::new(255, 0, 0));
    assert_eq!(style.border_widths.bottom, 3.0);
    assert_eq!(style.border_styles.bottom, BorderStyle::Dashed);
    assert_eq!(style.border_colors.bottom, Color::new(0, 0, 255));
    assert_eq!(style.border_widths.left, 4.0);
    assert_eq!(style.border_styles.left, BorderStyle::Dotted);
    assert_eq!(style.border_colors.left, Color::new(0, 128, 0));
    assert_eq!(style.border_widths.right, 4.0);
    assert_eq!(style.border_styles.right, BorderStyle::Dotted);
    assert_eq!(style.border_colors.right, Color::BLACK);
}

#[tokio::test]
async fn maps_logical_border_properties_through_rtl_direction() {
    let declarations = parse_declarations(
        "direction: rtl; \
         border-inline-start: 2pt solid red; \
         border-inline-end-width: 3pt; border-inline-end-style: dashed; border-inline-end-color: blue",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.direction, Direction::Rtl);
    assert_eq!(style.border_widths.right, 2.0);
    assert_eq!(style.border_styles.right, BorderStyle::Solid);
    assert_eq!(style.border_colors.right, Color::new(255, 0, 0));
    assert_eq!(style.border_widths.left, 3.0);
    assert_eq!(style.border_styles.left, BorderStyle::Dashed);
    assert_eq!(style.border_colors.left, Color::new(0, 0, 255));
}

#[tokio::test]
async fn maps_logical_border_properties_through_vertical_writing_mode() {
    let declarations = parse_declarations(
        "writing-mode: vertical-rl; direction: ltr; \
         border-block-start: 2pt solid red; \
         border-inline-start-width: 3pt; border-inline-start-style: dashed; border-inline-start-color: blue",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.writing_mode, WritingMode::VerticalRl);
    assert_eq!(style.border_widths.right, 2.0);
    assert_eq!(style.border_styles.right, BorderStyle::Solid);
    assert_eq!(style.border_colors.right, Color::new(255, 0, 0));
    assert_eq!(style.border_widths.top, 3.0);
    assert_eq!(style.border_styles.top, BorderStyle::Dashed);
    assert_eq!(style.border_colors.top, Color::new(0, 0, 255));
}

#[tokio::test]
async fn logical_border_revert_layer_rolls_back_physical_longhand() {
    let css = Css::from_string(
        "@layer base { p { border-inline: 2pt solid red; } }\
         @layer theme { p { border-left-color: blue; border-inline-start-color: revert-layer; } }",
    );
    let stylesheet = parse_stylesheet(&css);
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.border_widths.left, 2.0);
    assert_eq!(style.border_styles.left, BorderStyle::Solid);
    assert_eq!(style.border_colors.left, Color::new(255, 0, 0));
}

#[tokio::test]
async fn logical_border_revert_layer_uses_directional_physical_side() {
    let css = Css::from_string(
        "@layer base { p { direction: rtl; border-right-color: red; border-right-width: 2pt; border-right-style: solid; } }\
         @layer theme { p { direction: rtl; border-right-color: blue; border-inline-start-color: revert-layer; } }",
    );
    let stylesheet = parse_stylesheet(&css);
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.direction, Direction::Rtl);
    assert_eq!(style.border_widths.right, 2.0);
    assert_eq!(style.border_styles.right, BorderStyle::Solid);
    assert_eq!(style.border_colors.right, Color::new(255, 0, 0));
}

#[tokio::test]
async fn maps_logical_corner_radii_to_initial_physical_corners() {
    let declarations = parse_declarations(
        "border-start-start-radius: 1pt 2pt; \
         border-start-end-radius: 3pt; \
         border-end-start-radius: 4pt 5pt; \
         border-end-end-radius: 6pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_radius.top_left.x.value.length_points(), 1.0);
    assert_eq!(style.border_radius.top_left.y.value.length_points(), 2.0);
    assert_eq!(style.border_radius.top_right.x.value.length_points(), 3.0);
    assert_eq!(style.border_radius.top_right.y.value.length_points(), 3.0);
    assert_eq!(style.border_radius.bottom_left.x.value.length_points(), 4.0);
    assert_eq!(style.border_radius.bottom_left.y.value.length_points(), 5.0);
    assert_eq!(
        style.border_radius.bottom_right.x.value.length_points(),
        6.0
    );
    assert_eq!(
        style.border_radius.bottom_right.y.value.length_points(),
        6.0
    );
}

#[tokio::test]
async fn maps_logical_corner_radii_through_vertical_writing_mode() {
    let declarations = parse_declarations(
        "writing-mode: vertical-rl; direction: ltr; \
         border-start-start-radius: 1pt 2pt; \
         border-start-end-radius: 3pt; \
         border-end-start-radius: 4pt 5pt; \
         border-end-end-radius: 6pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_radius.top_right.x.value.length_points(), 1.0);
    assert_eq!(style.border_radius.top_right.y.value.length_points(), 2.0);
    assert_eq!(
        style.border_radius.bottom_right.x.value.length_points(),
        3.0
    );
    assert_eq!(
        style.border_radius.bottom_right.y.value.length_points(),
        3.0
    );
    assert_eq!(style.border_radius.top_left.x.value.length_points(), 4.0);
    assert_eq!(style.border_radius.top_left.y.value.length_points(), 5.0);
    assert_eq!(style.border_radius.bottom_left.x.value.length_points(), 6.0);
    assert_eq!(style.border_radius.bottom_left.y.value.length_points(), 6.0);
}

#[tokio::test]
async fn logical_corner_radius_revert_layer_rolls_back_physical_corner() {
    let css = Css::from_string(
        "@layer base { p { border-top-left-radius: 7pt 8pt; } }\
         @layer theme { p { border-top-left-radius: 1pt; border-start-start-radius: revert-layer; } }",
    );
    let stylesheet = parse_stylesheet(&css);
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.border_radius.top_left.x.value.length_points(), 7.0);
    assert_eq!(style.border_radius.top_left.y.value.length_points(), 8.0);
}

#[tokio::test]
async fn parses_border_image_longhands() {
    let declarations = parse_declarations(
        "border-image-source: url(\"images/border.png\"); border-image-slice: 1 20% 3 40% fill; border-image-width: 2 auto 4pt 25%; border-image-outset: 1 2pt; border-image-repeat: stretch round",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.border_image.source.as_deref(),
        Some("images/border.png")
    );
    assert_eq!(
        style.border_image.slice.offsets.top,
        BorderImageSliceValue::Number(1.0)
    );
    assert_eq!(
        style.border_image.slice.offsets.right,
        BorderImageSliceValue::Percent(0.2)
    );
    assert!(style.border_image.slice.fill);
    assert_eq!(
        style.border_image.width.top,
        BorderImageWidthValue::Number(2.0)
    );
    assert_eq!(style.border_image.width.right, BorderImageWidthValue::Auto);
    assert_eq!(
        style.border_image.width.bottom,
        BorderImageWidthValue::LengthPercentage(ComputedLengthPercentage::from_points(4.0))
    );
    assert_eq!(
        style.border_image.width.left,
        BorderImageWidthValue::LengthPercentage(ComputedLengthPercentage::from_percent(0.25))
    );
    assert_eq!(
        style.border_image.outset.top,
        BorderImageOutsetValue::Number(1.0)
    );
    assert_eq!(
        style.border_image.outset.right,
        BorderImageOutsetValue::Length(ComputedLengthPercentage::from_points(2.0))
    );
    assert_eq!(
        style.border_image.repeat.horizontal,
        BorderImageRepeatKeyword::Stretch
    );
    assert_eq!(
        style.border_image.repeat.vertical,
        BorderImageRepeatKeyword::Round
    );
}

#[tokio::test]
async fn parses_border_image_shorthand_and_resets_omitted_longhands() {
    let declarations = parse_declarations(
        "border-image-width: 4; border-image-outset: 2; border-image: url(\"images/border.png\") 1 20% 3 40% fill / 2 auto 4pt 25% / 1 2pt stretch round",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.border_image.source.as_deref(),
        Some("images/border.png")
    );
    assert_eq!(
        style.border_image.slice.offsets.right,
        BorderImageSliceValue::Percent(0.2)
    );
    assert!(style.border_image.slice.fill);
    assert_eq!(
        style.border_image.width.top,
        BorderImageWidthValue::Number(2.0)
    );
    assert_eq!(style.border_image.width.right, BorderImageWidthValue::Auto);
    assert_eq!(
        style.border_image.outset.top,
        BorderImageOutsetValue::Number(1.0)
    );
    assert_eq!(
        style.border_image.outset.right,
        BorderImageOutsetValue::Length(ComputedLengthPercentage::from_points(2.0))
    );
    assert_eq!(
        style.border_image.repeat.horizontal,
        BorderImageRepeatKeyword::Stretch
    );
    assert_eq!(
        style.border_image.repeat.vertical,
        BorderImageRepeatKeyword::Round
    );

    let reset = parse_declarations("border-image-width: 4; border-image: url(\"border.png\")");
    let mut reset_style = default_style_for_tag("div");
    apply_declarations(&mut reset_style, &reset);
    assert_eq!(
        reset_style.border_image.width.top,
        BorderImageWidthValue::Number(1.0)
    );
    assert_eq!(
        reset_style.border_image.slice.offsets.top,
        BorderImageSliceValue::Percent(1.0)
    );
}

#[tokio::test]
async fn ch_border_image_outset_preserves_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("border-image-outset: 2ch 1pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.border_image.outset.top,
        BorderImageOutsetValue::Length(ComputedLengthPercentage::from_ch(2.0))
    );
    assert_eq!(
        style.border_image.outset.right,
        BorderImageOutsetValue::Length(ComputedLengthPercentage::from_points(1.0))
    );

    style.resolve_font_metric_lengths(5.0);

    assert_eq!(
        style.border_image.outset.top,
        BorderImageOutsetValue::Length(ComputedLengthPercentage::from_points(10.0))
    );
    assert_eq!(
        style.border_image.outset.right,
        BorderImageOutsetValue::Length(ComputedLengthPercentage::from_points(1.0))
    );
}

#[tokio::test]
async fn rejects_invalid_border_shorthands_without_partial_application() {
    let declarations = parse_declarations("border: 7pt solid unknown-token");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_ne!(style.border_widths.top, 7.0);
    assert_eq!(style.border_styles.top, BorderStyle::None);
}

#[tokio::test]
async fn rejects_invalid_border_color_and_style_component_lists() {
    let declarations =
        parse_declarations("border-color: red unknown-token; border-style: solid unknown-token");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, Color::BLACK);
    assert_eq!(style.border_styles.top, BorderStyle::None);
}

#[tokio::test]
async fn parses_border_radius_shorthand_lengths_and_percentages() {
    let declarations = parse_declarations("border-radius: 4pt 8pt / 10% 20%");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_radius.top_left.x.value.length_points(), 4.0);
    assert_eq!(style.border_radius.top_right.x.value.length_points(), 8.0);
    assert_eq!(
        style.border_radius.bottom_right.x.value.length_points(),
        4.0
    );
    assert_eq!(style.border_radius.bottom_left.x.value.length_points(), 8.0);
    assert_eq!(style.border_radius.top_left.y.value.percent, 0.1);
    assert_eq!(style.border_radius.top_right.y.value.percent, 0.2);
    assert_eq!(style.border_radius.bottom_right.y.value.percent, 0.1);
    assert_eq!(style.border_radius.bottom_left.y.value.percent, 0.2);
}

#[tokio::test]
async fn ch_border_radius_preserves_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("border-radius: 2ch 1pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.border_radius.top_left.x.value,
        ComputedLengthPercentage::from_ch(2.0)
    );
    assert_eq!(
        style.border_radius.top_right.x.value,
        ComputedLengthPercentage::from_points(1.0)
    );

    style.resolve_font_metric_lengths(6.0);

    assert_eq!(
        style.border_radius.top_left.x.value,
        ComputedLengthPercentage::from_points(12.0)
    );
    assert_eq!(style.border_radius.top_left.x.resolve(100.0), 12.0);
}

#[tokio::test]
async fn parses_border_radius_corner_longhands() {
    let declarations =
        parse_declarations("border-top-left-radius: 4pt 10%; border-bottom-right-radius: 8pt 12pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_radius.top_left.x.value.length_points(), 4.0);
    assert_eq!(style.border_radius.top_left.y.value.percent, 0.1);
    assert_eq!(
        style.border_radius.bottom_right.x.value.length_points(),
        8.0
    );
    assert_eq!(
        style.border_radius.bottom_right.y.value.length_points(),
        12.0
    );
}

#[tokio::test]
async fn parses_corner_shape_and_corner_shorthand() {
    let declarations =
        parse_declarations("corner: 36px round / 18px bevel / 28px scoop / 20px notch");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.border_radius.top_left.x.value.length_points(),
        36.0 * CSS_PX_TO_PT
    );
    assert_eq!(
        style.border_radius.top_right.x.value.length_points(),
        18.0 * CSS_PX_TO_PT
    );
    assert_eq!(
        style.border_radius.bottom_right.x.value.length_points(),
        28.0 * CSS_PX_TO_PT
    );
    assert_eq!(
        style.border_radius.bottom_left.x.value.length_points(),
        20.0 * CSS_PX_TO_PT
    );
    assert_eq!(style.corner_shapes.top_left, CornerShape::ROUND);
    assert_eq!(style.corner_shapes.top_right, CornerShape::BEVEL);
    assert_eq!(style.corner_shapes.bottom_right, CornerShape::SCOOP);
    assert_eq!(style.corner_shapes.bottom_left, CornerShape::NOTCH);
}

#[tokio::test]
async fn parses_corner_shape_superellipse_values() {
    let declarations =
        parse_declarations("corner-shape: notch superellipse(-100) round superellipse(-infinity)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.corner_shapes.top_left, CornerShape::NOTCH);
    assert_eq!(
        style.corner_shapes.top_right.superellipse,
        SuperellipseParameter::Number(-100.0)
    );
    assert_eq!(style.corner_shapes.bottom_right, CornerShape::ROUND);
    assert_eq!(style.corner_shapes.bottom_left, CornerShape::NOTCH);

    apply_declarations(
        &mut style,
        &parse_declarations("corner-top-left-shape: square; corner-top-right-shape: squircle"),
    );
    assert_eq!(style.corner_shapes.top_left, CornerShape::SQUARE);
    assert_eq!(style.corner_shapes.top_right, CornerShape::SQUIRCLE);

    apply_declarations(
        &mut style,
        &parse_declarations("corner-bottom-right-shape: superellipse(infinity)"),
    );
    assert_eq!(style.corner_shapes.bottom_right, CornerShape::SQUARE);
}

#[tokio::test]
async fn invalid_corner_shape_shorthand_is_ignored_atomically() {
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &parse_declarations("corner-shape: square"));
    assert_eq!(style.corner_shapes.top_left, CornerShape::SQUARE);
    assert_eq!(style.corner_shapes.top_right, CornerShape::SQUARE);
    assert_eq!(style.corner_shapes.bottom_right, CornerShape::SQUARE);
    assert_eq!(style.corner_shapes.bottom_left, CornerShape::SQUARE);

    apply_declarations(
        &mut style,
        &parse_declarations("corner-shape: notch superellipse(bad) round scoop"),
    );
    assert_eq!(style.corner_shapes.top_left, CornerShape::SQUARE);
    assert_eq!(style.corner_shapes.top_right, CornerShape::SQUARE);
    assert_eq!(style.corner_shapes.bottom_right, CornerShape::SQUARE);
    assert_eq!(style.corner_shapes.bottom_left, CornerShape::SQUARE);
}

#[tokio::test]
async fn revert_layer_border_radius_longhand_preserves_unaffected_shorthand_corners() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p { border-top-left-radius: 3pt } }\
         @layer theme { p { border-radius: 10pt/20pt; border-top-left-radius: revert-layer } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.border_radius.top_left.x.value.length_points(), 3.0);
    assert_eq!(style.border_radius.top_left.y.value.length_points(), 3.0);
    assert_eq!(style.border_radius.top_right.x.value.length_points(), 10.0);
    assert_eq!(style.border_radius.top_right.y.value.length_points(), 20.0);
    assert_eq!(
        style.border_radius.bottom_right.x.value.length_points(),
        10.0
    );
    assert_eq!(
        style.border_radius.bottom_right.y.value.length_points(),
        20.0
    );
    assert_eq!(
        style.border_radius.bottom_left.x.value.length_points(),
        10.0
    );
    assert_eq!(
        style.border_radius.bottom_left.y.value.length_points(),
        20.0
    );
}

#[tokio::test]
async fn parses_table_border_model_properties() {
    let declarations = parse_declarations(
        "border-collapse: collapse; border-spacing: 4pt 6px; caption-side: bottom; table-layout: fixed; visibility: collapse",
    );
    let mut style = default_style_for_tag("table");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_collapse, BorderCollapse::Collapse);
    assert_eq!(style.caption_side, CaptionSide::Bottom);
    assert_eq!(style.table_layout, TableLayout::Fixed);
    assert_eq!(style.empty_cells, EmptyCells::Show);
    assert_eq!(style.visibility, Visibility::Collapse);
    assert_eq!(style.border_spacing.horizontal.length_points(), 4.0);
    assert_eq!(style.border_spacing.vertical.length_points(), 4.5);
    assert!(style.border_spacing_explicit);
}

#[tokio::test]
async fn ch_border_spacing_preserves_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("border-spacing: 2ch 1pt");
    let mut style = default_style_for_tag("table");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.border_spacing.horizontal,
        ComputedLengthPercentage::from_ch(2.0)
    );
    assert_eq!(
        style.border_spacing.vertical,
        ComputedLengthPercentage::from_points(1.0)
    );

    style.resolve_font_metric_lengths(5.0);

    assert_eq!(
        style.border_spacing.horizontal,
        ComputedLengthPercentage::from_points(10.0)
    );
    assert_eq!(
        style.border_spacing.vertical,
        ComputedLengthPercentage::from_points(1.0)
    );
}

#[tokio::test]
async fn border_spacing_initial_value_is_zero_but_html_tables_get_ua_spacing() {
    let initial = ComputedStyle::initial();
    assert_eq!(initial.border_spacing.horizontal.length_points(), 0.0);
    assert_eq!(initial.border_spacing.vertical.length_points(), 0.0);

    let table = default_style_for_tag("table");
    assert_eq!(table.border_spacing.horizontal.length_points(), 1.5);
    assert_eq!(table.border_spacing.vertical.length_points(), 1.5);
    assert!(!table.border_spacing_explicit);
}

#[tokio::test]
async fn parses_empty_cells_property() {
    let declarations = parse_declarations("empty-cells: hide");
    let mut style = default_style_for_tag("table");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.empty_cells, EmptyCells::Hide);
}

#[tokio::test]
async fn parses_text_decoration_longhands_and_shorthand_components() {
    let declarations = parse_declarations(
        "color: blue; text-decoration-line: underline overline line-through; \
         text-decoration-style: dashed; text-decoration-color: red; \
         text-decoration-thickness: 3px; text-underline-offset: 2px; \
         text-decoration-skip-ink: none; text-decoration-skip-spaces: end start; \
         text-underline-position: under",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(style.text_decoration.underline);
    assert!(style.text_decoration.overline);
    assert!(style.text_decoration.line_through);
    assert_eq!(style.text_decoration.style, TextDecorationStyle::Dashed);
    assert_eq!(style.text_decoration.color, Some(Color::new(255, 0, 0)));
    assert!(matches!(
        style.text_decoration.thickness,
        TextDecorationThickness::LengthPercentage(value)
            if (value.length_points() - 2.25).abs() < 0.01 && value.percent == 0.0
    ));
    assert_eq!(style.text_decoration.skip_ink, TextDecorationSkipInk::None);
    assert!(style.text_decoration.skip_spaces.start);
    assert!(style.text_decoration.skip_spaces.end);
    assert!(!style.text_decoration.skip_spaces.all);
    assert!(matches!(
        style.text_decoration.underline_offset,
        TextUnderlineOffset::LengthPercentage(value)
            if (value.length_points() - 1.5).abs() < 0.01 && value.percent == 0.0
    ));
    assert!(style.text_decoration.underline_position.under);
}

#[tokio::test]
async fn parses_css_text_decoration_level_four_properties() {
    let declarations = parse_declarations(
        "text-decoration-inset: 2px -1px; \
         text-decoration-skip-self: skip-underline skip-line-through; \
         text-decoration-skip-box: all; text-decoration-skip: auto; \
         text-decoration-thickness: thick; text-shadow: 1px 2px 3px 4px red, blue -2px 0 inset; \
         box-shadow: inset 60px 0 green, currentcolor -2px 3px 0 -1px; \
         text-emphasis: open dot green; text-emphasis-position: under left; \
         text-emphasis-skip: spaces punctuation symbols narrow",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.text_decoration.inset,
        TextDecorationInset::Lengths { start, end }
            if (start.length_points() - 1.5).abs() < 0.01
                && start.percent == 0.0
                && (end.length_points() + 0.75).abs() < 0.01
                && end.percent == 0.0
    ));
    assert!(matches!(
        style.text_decoration.skip_self,
        TextDecorationSkipSelf::Auto
    ));
    assert_eq!(style.text_decoration.skip_box, TextDecorationSkipBox::None);
    assert!(matches!(
        style.text_decoration.thickness,
        TextDecorationThickness::LengthPercentage(value)
            if (value.length_points() - 3.75).abs() < 0.01
    ));
    assert_eq!(style.text_shadow.len(), 2);
    assert!((style.text_shadow[0].offset_x.length_points() - 0.75).abs() < 0.01);
    assert!((style.text_shadow[0].offset_y.length_points() - 1.5).abs() < 0.01);
    assert!((style.text_shadow[0].blur_radius.length_points() - 2.25).abs() < 0.01);
    assert!((style.text_shadow[0].spread.length_points() - 3.0).abs() < 0.01);
    assert_eq!(
        style.text_shadow[0].color,
        TextShadowColor::Color(Color::new(255, 0, 0))
    );
    assert!(!style.text_shadow[0].inset);
    assert!(style.text_shadow[1].inset);
    assert_eq!(style.box_shadow.len(), 2);
    assert!(style.box_shadow[0].inset);
    assert!((style.box_shadow[0].offset_x.length_points() - 45.0).abs() < 0.01);
    assert!((style.box_shadow[0].offset_y.length_points() - 0.0).abs() < 0.01);
    assert_eq!(
        style.box_shadow[0].color,
        BoxShadowColor::Color(Color::new(0, 128, 0))
    );
    assert_eq!(style.box_shadow[1].color, BoxShadowColor::CurrentColor);
    assert!((style.box_shadow[1].offset_x.length_points() + 1.5).abs() < 0.01);
    assert!((style.box_shadow[1].offset_y.length_points() - 2.25).abs() < 0.01);
    assert!((style.box_shadow[1].spread.length_points() + 0.75).abs() < 0.01);
    assert_eq!(style.text_emphasis_color, Some(Color::new(0, 128, 0)));
    assert_eq!(
        style
            .text_emphasis_style
            .mark_for_writing_mode(style.writing_mode),
        Some("\u{25E6}")
    );
    assert!(!style.text_emphasis_position.over);
    assert!(!style.text_emphasis_position.right);
    assert!(style.text_emphasis_skip.spaces);
    assert!(style.text_emphasis_skip.punctuation);
    assert!(style.text_emphasis_skip.symbols);
    assert!(style.text_emphasis_skip.narrow);
}

#[tokio::test]
async fn ch_text_decoration_inset_preserves_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("text-decoration-inset: 2ch 1pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.text_decoration.inset,
        TextDecorationInset::Lengths { start, end }
            if start == ComputedLengthPercentage::from_ch(2.0)
                && end == ComputedLengthPercentage::from_points(1.0)
    ));

    style.resolve_font_metric_lengths(6.0);

    assert!(matches!(
        style.text_decoration.inset,
        TextDecorationInset::Lengths { start, end }
            if start == ComputedLengthPercentage::from_points(12.0)
                && end == ComputedLengthPercentage::from_points(1.0)
    ));
}

#[tokio::test]
async fn parses_box_shadow_none_and_rejects_negative_blur() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("box-shadow: 1px 2px 0 red; box-shadow: none"),
    );
    assert!(style.box_shadow.is_empty());

    apply_declarations(
        &mut style,
        &parse_declarations("box-shadow: 1px 2px -3px green"),
    );
    assert!(style.box_shadow.is_empty());
}

#[tokio::test]
async fn ch_shadow_lengths_preserve_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("text-shadow: 2ch 1pt; box-shadow: 1pt 3ch 0 2ch");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.text_shadow[0].offset_x,
        ComputedLengthPercentage::from_ch(2.0)
    );
    assert_eq!(
        style.box_shadow[0].offset_y,
        ComputedLengthPercentage::from_ch(3.0)
    );
    assert_eq!(
        style.box_shadow[0].spread,
        ComputedLengthPercentage::from_ch(2.0)
    );

    style.resolve_font_metric_lengths(5.0);

    assert_eq!(
        style.text_shadow[0].offset_x,
        ComputedLengthPercentage::from_points(10.0)
    );
    assert_eq!(
        style.box_shadow[0].offset_y,
        ComputedLengthPercentage::from_points(15.0)
    );
    assert_eq!(
        style.box_shadow[0].spread,
        ComputedLengthPercentage::from_points(10.0)
    );
}

#[tokio::test]
async fn active_text_decoration_layers_preserve_originating_box_values() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { text-decoration: underline red 3px; } \
         span { text-decoration-color: blue; text-decoration-style: wavy; }",
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[ElementSignature::new("p", HashMap::new())],
    );

    assert_eq!(parent.text_decoration_layers.len(), 1);
    assert_eq!(child.text_decoration_layers.len(), 1);
    assert!(child.text_decoration_layers[0].underline);
    assert_eq!(
        child.text_decoration_layers[0].color,
        Some(Color::new(255, 0, 0))
    );
    assert_eq!(
        child.text_decoration_layers[0].style,
        TextDecorationStyle::Solid
    );
    assert_eq!(child.text_decoration.color, Some(Color::new(0, 0, 255)));
    assert_eq!(child.text_decoration.style, TextDecorationStyle::Wavy);
    assert!(!child.text_decoration.underline);
}

#[tokio::test]
async fn css_text_decoration_level_three_currentcolor_is_frozen_on_originating_box() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: blue; text-decoration: underline; } \
         span { color: red; text-emphasis: dot; }",
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[ElementSignature::new("p", HashMap::new())],
    );

    assert_eq!(child.color, Color::new(255, 0, 0));
    assert_eq!(
        child.text_decoration_layers[0].color,
        Some(Color::new(0, 0, 255))
    );
    assert_eq!(child.text_emphasis_color, Some(Color::new(255, 0, 0)));
}

#[tokio::test]
async fn text_shadow_currentcolor_remains_symbolic_through_inheritance() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "body { color: red; text-shadow: 0 0 10px currentcolor; } \
         p { color: green; text-shadow: inherit; }",
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("body", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[ElementSignature::new("body", HashMap::new())],
    );

    assert_eq!(parent.text_shadow[0].color, TextShadowColor::CurrentColor);
    assert_eq!(child.text_shadow[0].color, TextShadowColor::CurrentColor);
    assert_eq!(
        child.text_shadow[0].color.resolve(child.color),
        Color::new(0, 128, 0)
    );

    let mut omitted = default_style_for_tag("div");
    apply_declarations(
        &mut omitted,
        &parse_declarations("color: blue; text-shadow: 0 0 10px"),
    );
    assert_eq!(omitted.text_shadow[0].color, TextShadowColor::CurrentColor);
}

#[tokio::test]
async fn css_text_decoration_level_three_ua_defaults_apply() {
    let ua = html5_user_agent_stylesheet();
    let mut zh_attrs = HashMap::new();
    zh_attrs.insert("lang".to_string(), "zh-Hans".to_string());
    let zh = style_for_element_with_signature(
        ElementSignature::new("p", zh_attrs),
        Some("text-emphasis-style: dot"),
        std::slice::from_ref(&ua),
        None,
        &[],
    );
    assert!(!zh.text_emphasis_position.over);
    assert!(zh.text_emphasis_position.right);
    assert!(zh.text_decoration.underline_position.left);

    let mut parent = default_style_for_tag("span");
    apply_declarations(&mut parent, &parse_declarations("text-emphasis: dot red"));
    let rt = style_for_element_with_signature(
        ElementSignature::new("rt", HashMap::new()),
        None,
        std::slice::from_ref(&ua),
        Some(&parent),
        &[ElementSignature::new("ruby", HashMap::new())],
    );
    assert_eq!(rt.text_emphasis_style, TextEmphasisStyle::None);
}

#[tokio::test]
async fn parses_text_decoration_skip_spaces_full_grammar() {
    let mut style = default_style_for_tag("div");
    assert!(style.text_decoration.skip_spaces.start);
    assert!(style.text_decoration.skip_spaces.end);
    assert!(!style.text_decoration.skip_spaces.all);

    apply_declarations(
        &mut style,
        &parse_declarations("text-decoration-skip-spaces: none"),
    );
    assert_eq!(
        style.text_decoration.skip_spaces,
        TextDecorationSkipSpaces::NONE
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-decoration-skip-spaces: all"),
    );
    assert_eq!(
        style.text_decoration.skip_spaces,
        TextDecorationSkipSpaces::ALL
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-decoration-skip-spaces: start"),
    );
    assert!(style.text_decoration.skip_spaces.start);
    assert!(!style.text_decoration.skip_spaces.end);
    assert!(!style.text_decoration.skip_spaces.all);

    apply_declarations(
        &mut style,
        &parse_declarations("text-decoration-skip-spaces: end start"),
    );
    assert_eq!(
        style.text_decoration.skip_spaces,
        TextDecorationSkipSpaces::START_END
    );

    apply_declarations(
        &mut style,
        &parse_declarations(
            "text-decoration-skip-spaces: all end; text-decoration-skip-spaces: start start",
        ),
    );
    assert_eq!(
        style.text_decoration.skip_spaces,
        TextDecorationSkipSpaces::START_END
    );
}

#[tokio::test]
async fn text_decoration_shorthand_resets_omitted_components_and_rejects_duplicate_lines() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("text-decoration: underline dotted red 2px"),
    );
    assert!(style.text_decoration.underline);
    assert_eq!(style.text_decoration.style, TextDecorationStyle::Dotted);
    assert_eq!(style.text_decoration.color, Some(Color::new(255, 0, 0)));

    apply_declarations(&mut style, &parse_declarations("text-decoration: overline"));
    assert!(!style.text_decoration.underline);
    assert!(style.text_decoration.overline);
    assert!(!style.text_decoration.line_through);
    assert_eq!(style.text_decoration.style, TextDecorationStyle::Solid);
    assert_eq!(style.text_decoration.color, None);

    apply_declarations(
        &mut style,
        &parse_declarations("text-decoration-line: blink underline blink"),
    );
    assert!(style.text_decoration.overline);
    assert!(!style.text_decoration.underline);

    apply_declarations(
        &mut style,
        &parse_declarations("text-underline-position: under; text-underline-position: from-font"),
    );
    assert!(style.text_decoration.underline_position.under);
}

#[tokio::test]
async fn parses_text_emphasis_style_keywords_and_strings() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("writing-mode: vertical-rl; text-emphasis-style: filled"),
    );

    assert_eq!(
        style
            .text_emphasis_style
            .mark_for_writing_mode(style.writing_mode),
        Some("\u{FE45}")
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-emphasis-style: open dot"),
    );
    assert_eq!(
        style
            .text_emphasis_style
            .mark_for_writing_mode(style.writing_mode),
        Some("\u{25E6}")
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-emphasis-style: \"*\""),
    );
    assert_eq!(
        style
            .text_emphasis_style
            .mark_for_writing_mode(style.writing_mode),
        Some("*")
    );
}

#[tokio::test]
async fn text_emphasis_style_inherits_and_unset_restores_parent_value() {
    let mut parent = default_style_for_tag("div");
    apply_declarations(
        &mut parent,
        &parse_declarations("text-emphasis-style: open sesame"),
    );

    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("text-emphasis-style: unset"),
        &[],
        Some(&parent),
        &[],
    );

    assert_eq!(
        child
            .text_emphasis_style
            .mark_for_writing_mode(child.writing_mode),
        Some("\u{FE46}")
    );
}

#[tokio::test]
async fn inline_style_applies_text_emphasis_style() {
    let style = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("writing-mode: vertical-rl; text-emphasis-style: filled"),
        &[],
        None,
        &[],
    );

    assert_eq!(
        style
            .text_emphasis_style
            .mark_for_writing_mode(style.writing_mode),
        Some("\u{FE45}")
    );
}

#[tokio::test]
async fn table_cells_default_to_one_css_px_padding() {
    let td = default_style_for_tag("td");
    let th = default_style_for_tag("th");

    assert_eq!(td.padding, edge_all(CSS_PX_TO_PT));
    assert_eq!(th.padding, edge_all(CSS_PX_TO_PT));
}

#[tokio::test]
async fn parses_cssparser_colors() {
    let declarations =
        parse_declarations("color: rebeccapurple; background: #0f08; border-color: rgb(1, 2, 3)");
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.color, Color::new(102, 51, 153));
    let background = style.background_color.unwrap();
    assert_eq!(background.r, 0.0);
    assert_eq!(background.g, 1.0);
    assert_eq!(background.b, 0.0);
    assert!((background.a - 136.0 / 255.0).abs() < 0.000001);
    assert_eq!(style.border_color, Color::new(1, 2, 3));
}

#[tokio::test]
async fn parses_alpha_and_transparent_colors() {
    let declarations = parse_declarations(
        "color: rgba(255, 0, 0, 0.5); background-color: rgb(0 0 255 / 25%); border-color: transparent",
    );
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.color, Color::rgba(255, 0, 0, 0.5));
    assert_eq!(style.background_color, Some(Color::rgba(0, 0, 255, 0.25)));
    assert_eq!(style.border_color, Color::TRANSPARENT);
}

#[tokio::test]
async fn parses_hsl_border_colors() {
    let declarations =
        parse_declarations("border-color: hsl(120 100% 25% / 50%) hsla(240, 100%, 50%, 0.25)");
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, Color::rgba(0, 128, 0, 0.5));
    assert_eq!(style.border_colors.bottom, Color::rgba(0, 128, 0, 0.5));
    assert_eq!(style.border_colors.right, Color::rgba(0, 0, 255, 0.25));
    assert_eq!(style.border_colors.left, Color::rgba(0, 0, 255, 0.25));
}

#[tokio::test]
async fn parses_hwb_border_colors() {
    let declarations = parse_declarations(
        "border-color: hwb(0 0% 0%) hwb(120 0% 50% / 25%) hwb(240 20% 0%) hwb(0, 100%, 100%, 50%)",
    );
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, Color::new(255, 0, 0));
    assert_eq!(style.border_colors.right, Color::rgba(0, 128, 0, 0.25));
    assert_eq!(style.border_colors.bottom, Color::new(51, 51, 255));
    assert_eq!(style.border_colors.left, Color::rgba(128, 128, 128, 0.5));
}

#[tokio::test]
async fn parses_hwb_in_border_shorthand_as_single_component() {
    let declarations = parse_declarations("border: 2pt solid hwb(240 20% 0% / 75%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_colors.top, Color::rgba(51, 51, 255, 0.75));
}

#[tokio::test]
async fn parses_srgb_color_function_border_colors() {
    let declarations = parse_declarations(
        "border-color: color(srgb 1 0 0) color(srgb 0% 50% 0% / 25%) color(srgb 20% 20% 100%) color(srgb none 1.5 -1 / 50%)",
    );
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, Color::srgb(1.0, 0.0, 0.0, 1.0));
    assert_eq!(style.border_colors.right, Color::srgb(0.0, 0.5, 0.0, 0.25));
    assert_eq!(style.border_colors.bottom, Color::srgb(0.2, 0.2, 1.0, 1.0));
    assert_eq!(style.border_colors.left, Color::srgb(0.0, 1.0, 0.0, 0.5));
}

#[tokio::test]
async fn parses_srgb_color_function_in_border_shorthand_as_single_component() {
    let declarations = parse_declarations("border: 2pt solid color(srgb 0.2 0.2 1 / 75%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_colors.top, Color::srgb(0.2, 0.2, 1.0, 0.75));
}

#[tokio::test]
async fn parses_dimensions() {
    let declarations = parse_declarations(
        "width: 50%; height: 0.5in; min-width: 2cm; max-height: 3rem; line-height: 1.5",
    );
    let mut style = default_style_for_tag("div");
    style.font_size = 10.0;
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(0.0),
            percent: 0.5,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(36.0),
            percent: 0.0,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    let ComputedLengthPercentageOrAuto::LengthPercentage(min_width) = style.box_values.min_width
    else {
        panic!("min-width should compute to a length");
    };
    assert!((min_width.length_points() - 56.692913).abs() < 0.001);
    assert_eq!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(36.0),
            percent: 0.0,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(style.line_height, 15.0);
}

#[tokio::test]
async fn parses_intrinsic_box_size_keywords() {
    let declarations = parse_declarations(
        "width: min-content; height: max-content; min-width: fit-content; min-height: stretch; max-height: fit-content(30pt); margin-left: stretch",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::MinContent
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::MaxContent
    );
    assert_eq!(
        style.box_values.min_width,
        ComputedLengthPercentageOrAuto::FitContent(None)
    );
    assert_eq!(
        style.box_values.min_height,
        ComputedLengthPercentageOrAuto::Stretch
    );
    assert_eq!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::FitContent(Some(ComputedLengthPercentage::from_points(
            30.0
        )))
    );
    assert_eq!(
        style.box_values.margin.left,
        ComputedLengthPercentageOrAuto::ZERO,
        "box sizing keywords are not valid margin values"
    );
}

#[tokio::test]
async fn parses_css_math_length_percentage_values() {
    let declarations = parse_declarations(
        "width: calc(50% - 2em); height: min(2in, 200pt); min-width: max(10%, 25%); max-height: clamp(20pt, 5em, 80pt); font-size: calc(100% + 2pt); line-height: calc(100% + 2pt)",
    );
    let mut style = default_style_for_tag("div");
    style.font_size = 10.0;
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(-24.0),
            percent: 0.5,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(144.0),
            percent: 0.0,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(
        style.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(0.0),
            percent: 0.25,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(60.0),
            percent: 0.0,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(style.font_size, 12.0);
    assert_eq!(style.line_height, 14.0);
}

#[tokio::test]
async fn length_percentages_preserve_authored_percentage_presence() {
    let declarations =
        parse_declarations("width: 40pt; min-width: 50%; max-width: calc(40pt + 0%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("expected length width");
    };
    assert!(!width.has_percentage);
    assert_eq!(width.length_if_no_percent(), Some(40.0));

    let ComputedLengthPercentageOrAuto::LengthPercentage(min_width) = style.box_values.min_width
    else {
        panic!("expected percentage min-width");
    };
    assert!(min_width.has_percentage);
    assert!(min_width.length_if_no_percent().is_none());

    let ComputedLengthPercentageOrAuto::LengthPercentage(max_width) = style.box_values.max_width
    else {
        panic!("expected calc max-width");
    };
    assert!(max_width.has_percentage);
    assert_eq!(max_width.percent, 0.0);
    assert!(max_width.length_if_no_percent().is_none());
    assert_eq!(
        max_width.used_length_with_percentage_basis(200.0),
        Some(40.0)
    );

    let declarations = parse_declarations("max-width: 40pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ComputedLengthPercentageOrAuto::LengthPercentage(max_width) = style.box_values.max_width
    else {
        panic!("expected length max-width");
    };
    assert!(!max_width.has_percentage);
    assert_eq!(max_width.length_if_no_percent(), Some(40.0));
}

#[tokio::test]
async fn rejects_incomparable_css_math_number_length_values() {
    let declarations = parse_declarations("width: 20pt; width: min(10pt, 2)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(20.0),
            percent: 0.0,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
}

#[tokio::test]
async fn css_math_defers_ch_comparisons_until_font_metric_resolution() {
    let declarations = parse_declarations(
        "width: calc(min(10pt, 2ch) + 1pt); height: calc(max(10pt, 2ch) - 1pt); min-width: clamp(10pt, 2ch, 20pt); max-height: min(20pt, 2ch, 10pt); line-height: min(10pt, calc(2ch + 1pt))",
    );
    let mut small_ch = default_style_for_tag("div");
    apply_declarations(&mut small_ch, &declarations);

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = small_ch.box_values.width else {
        panic!("expected deferred width");
    };
    assert!(width.length_if_no_percent().is_none());

    let mut large_ch = small_ch.clone();

    small_ch.resolve_font_metric_lengths(4.0);
    large_ch.resolve_font_metric_lengths(6.0);

    assert_eq!(
        small_ch.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            9.0
        ))
    );
    assert_eq!(
        small_ch.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            9.0
        ))
    );
    assert_eq!(
        small_ch.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            10.0
        ))
    );
    assert_eq!(
        small_ch.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            8.0
        ))
    );
    assert_eq!(small_ch.line_height, 9.0);

    assert_eq!(
        large_ch.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            11.0
        ))
    );
    assert_eq!(
        large_ch.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            11.0
        ))
    );
    assert_eq!(
        large_ch.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            12.0
        ))
    );
    assert_eq!(
        large_ch.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            10.0
        ))
    );
    assert_eq!(large_ch.line_height, 10.0);
}

#[tokio::test]
async fn css_math_defers_length_percentage_comparisons_until_used_basis_resolution() {
    let declarations = parse_declarations(
        "width: min(10pt, 50%); height: max(10pt, 50%); min-width: calc(min(10ch, 50%) + 1pt); max-width: clamp(10pt, 50%, 80pt)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("expected deferred width");
    };
    assert!(width.length_if_no_percent().is_none());
    assert_eq!(width.used_length_with_percentage_basis(12.0), Some(6.0));
    assert_eq!(width.used_length_with_percentage_basis(40.0), Some(10.0));

    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = style.box_values.height else {
        panic!("expected deferred height");
    };
    assert_eq!(height.used_length_with_percentage_basis(12.0), Some(10.0));
    assert_eq!(height.used_length_with_percentage_basis(40.0), Some(20.0));

    style.resolve_font_metric_lengths(4.0);

    let ComputedLengthPercentageOrAuto::LengthPercentage(min_width) = style.box_values.min_width
    else {
        panic!("expected deferred min-width");
    };
    assert_eq!(
        min_width.used_length_with_percentage_basis(100.0),
        Some(41.0)
    );
    assert_eq!(
        min_width.used_length_with_percentage_basis(60.0),
        Some(31.0)
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(max_width) = style.box_values.max_width
    else {
        panic!("expected deferred max-width");
    };
    assert_eq!(
        max_width.used_length_with_percentage_basis(12.0),
        Some(10.0)
    );
    assert_eq!(
        max_width.used_length_with_percentage_basis(100.0),
        Some(50.0)
    );
    assert_eq!(
        max_width.used_length_with_percentage_basis(200.0),
        Some(80.0)
    );
}

#[tokio::test]
async fn css_math_same_unit_ch_values_preserve_font_metric_component() {
    let declarations = parse_declarations(
        "width: min(3ch, 2ch); height: max(1ch, 4ch); min-width: clamp(1ch, 3ch, 2ch); line-height: min(4ch, 2ch)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_ch(2.0))
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_ch(4.0))
    );
    assert_eq!(
        style.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_ch(2.0))
    );
    assert_eq!(
        style.line_height_value,
        ComputedLineHeight::Length(ComputedLengthPercentage::from_ch(2.0))
    );

    style.resolve_font_metric_lengths(5.0);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            10.0
        ))
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            20.0
        ))
    );
    assert_eq!(
        style.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            10.0
        ))
    );
    assert_eq!(style.line_height, 10.0);
}

#[tokio::test]
async fn css_math_affine_ch_values_reduce_when_unknown_components_cancel() {
    let declarations = parse_declarations(
        "width: min(calc(3ch + 1pt), calc(2ch + 1pt)); height: max(calc(2ch + 3pt), calc(2ch + 5pt)); min-width: min(calc(1ch + 10%), calc(3ch + 10%)); line-height: max(calc(2ch + 1pt), calc(2ch + 3pt))",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(1.0),
            ch: 2.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(5.0),
            ch: 2.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(
        style.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            percent: 0.1,
            ch: 1.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(
        style.line_height_value,
        ComputedLineHeight::Length(ComputedLengthPercentage {
            length: layout_pt(3.0),
            ch: 2.0,
            ..ComputedLengthPercentage::ZERO
        })
    );

    style.resolve_font_metric_lengths(5.0);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            11.0
        ))
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            15.0
        ))
    );
    assert_eq!(
        style.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(5.0),
            percent: 0.1,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(style.line_height, 13.0);
}

#[tokio::test]
async fn computed_box_values_keep_typed_percentages_and_auto() {
    let declarations = parse_declarations(
        "margin: auto 10% 2em 3pt; padding: 5% 1em; left: 25%; width: auto; font-size: 10px",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_size, 7.5);
    assert!(style.box_values.margin.top.is_auto());
    assert_eq!(style.margin.bottom, 15.0);
    assert_eq!(
        style.box_values.margin.right,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(0.0),
            percent: 0.1,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(
        style.box_values.padding.left,
        ComputedLengthPercentage {
            length: layout_pt(7.5),
            percent: 0.0,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        }
    );
    assert_eq!(
        style.box_values.padding.top,
        ComputedLengthPercentage {
            length: layout_pt(0.0),
            percent: 0.05,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        }
    );
    assert_eq!(
        style.box_values.inset_left,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(0.0),
            percent: 0.25,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
    assert_eq!(style.box_values.width, ComputedLengthPercentageOrAuto::Auto);
}

#[tokio::test]
async fn em_lengths_use_computed_element_font_size() {
    let declarations =
        parse_declarations("margin-top: 1em; padding-left: 2em; width: 10em; font-size: 10px");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_size, 7.5);
    assert_eq!(style.margin.top, 7.5);
    assert_eq!(style.padding.left, 15.0);
    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(75.0),
            percent: 0.0,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
}

#[tokio::test]
async fn logical_margin_and_padding_edges_map_to_physical_sides() {
    let declarations = parse_declarations(
        "direction: rtl; writing-mode: horizontal-tb; \
         margin-block: 2pt 4pt; margin-inline: 6pt 8pt; \
         padding-block-start: 10pt; padding-inline-start: 12pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.margin.top, 2.0);
    assert_eq!(style.margin.bottom, 4.0);
    assert_eq!(style.margin.right, 6.0);
    assert_eq!(style.margin.left, 8.0);
    assert_eq!(style.padding.top, 10.0);
    assert_eq!(style.padding.right, 12.0);
}

#[tokio::test]
async fn logical_box_edges_follow_vertical_writing_mode() {
    let declarations = parse_declarations(
        "direction: ltr; writing-mode: vertical-rl; \
         margin-block-start: 3pt; margin-block-end: 5pt; \
         padding-inline-start: 7pt; padding-inline-end: 11pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.margin.right, 3.0);
    assert_eq!(style.margin.left, 5.0);
    assert_eq!(style.padding.top, 7.0);
    assert_eq!(style.padding.bottom, 11.0);
}

#[tokio::test]
async fn inset_shorthands_map_to_physical_sides() {
    let declarations = parse_declarations(
        "direction: rtl; writing-mode: horizontal-tb; \
         inset: 1pt 2pt 3pt 4pt; inset-inline: 6pt 8pt; inset-block-start: 10pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.inset_top,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            10.0
        ))
    );
    assert_eq!(
        style.box_values.inset_right,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            6.0
        ))
    );
    assert_eq!(
        style.box_values.inset_bottom,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            3.0
        ))
    );
    assert_eq!(
        style.box_values.inset_left,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            8.0
        ))
    );
}

#[tokio::test]
async fn logical_insets_follow_vertical_writing_mode() {
    let declarations = parse_declarations(
        "direction: ltr; writing-mode: vertical-rl; \
         inset-block: 3pt 5pt; inset-inline-start: 7pt; inset-inline-end: 11pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.inset_right,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            3.0
        ))
    );
    assert_eq!(
        style.box_values.inset_left,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            5.0
        ))
    );
    assert_eq!(
        style.box_values.inset_top,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            7.0
        ))
    );
    assert_eq!(
        style.box_values.inset_bottom,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            11.0
        ))
    );
}

#[tokio::test]
async fn relative_font_size_recomputes_unitless_line_height() {
    let declarations = parse_declarations("font-size: 10px; line-height: 1.5");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_size, 7.5);
    assert_eq!(style.line_height, 11.25);
    assert_eq!(style.line_height_value, ComputedLineHeight::Number(1.5));
    assert_eq!(style.line_height_multiplier, Some(1.5));

    let declarations = parse_declarations("font-size: 1.2em");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_size, 9.0);
    assert_eq!(style.line_height, 13.5);
    assert_eq!(style.line_height_value, ComputedLineHeight::Number(1.5));
}

#[tokio::test]
async fn zero_font_size_reprojects_inherited_line_height() {
    let parent = ComputedStyle {
        font_size: 20.0,
        line_height: 24.0,
        line_height_value: ComputedLineHeight::Normal,
        line_height_multiplier: Some(1.2),
        line_height_is_normal: true,
        ..default_style_for_tag("div")
    };
    let child = style_for_element_with_signature(
        ElementSignature::new("sep", HashMap::new()),
        Some("font-size: 0"),
        &[],
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );

    assert_eq!(child.font_size, 0.0);
    assert_eq!(child.line_height, 0.0);
    assert!(child.line_height_is_normal);

    let parent = ComputedStyle {
        line_height: 30.0,
        line_height_value: ComputedLineHeight::Number(1.5),
        line_height_multiplier: Some(1.5),
        line_height_is_normal: false,
        ..parent
    };
    let child = style_for_element_with_signature(
        ElementSignature::new("sep", HashMap::new()),
        Some("font-size: 0"),
        &[],
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );

    assert_eq!(child.font_size, 0.0);
    assert_eq!(child.line_height, 0.0);
    assert_eq!(child.line_height_value, ComputedLineHeight::Number(1.5));
}

#[tokio::test]
async fn absolute_line_height_is_not_scaled_by_later_font_size() {
    let declarations = parse_declarations("font-size: 10px; line-height: 20px");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.line_height, 15.0);
    assert_eq!(
        style.line_height_value,
        ComputedLineHeight::from_points(15.0)
    );
    assert_eq!(style.line_height_multiplier, None);

    let declarations = parse_declarations("font-size: 2em");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_size, 15.0);
    assert_eq!(style.line_height, 15.0);
}

#[tokio::test]
async fn ch_font_size_uses_parent_metric_fallback() {
    let mut parent = default_style_for_tag("div");
    parent.font_size = 20.0;

    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("font-size: 2ch"),
        &[],
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );

    assert_eq!(child.font_size, 20.0);
    assert_eq!(child.line_height, 24.0);
}

#[tokio::test]
async fn ch_font_size_uses_vertical_upright_parent_fallback() {
    let mut parent = default_style_for_tag("div");
    parent.font_size = 20.0;
    parent.writing_mode = WritingMode::VerticalRl;
    parent.text_orientation = TextOrientation::Upright;

    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("font-size: 2ch"),
        &[],
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );

    assert_eq!(child.font_size, 40.0);
    assert_eq!(child.line_height, 48.0);
}

#[tokio::test]
async fn ch_line_height_preserves_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("font-size: 20px; line-height: 5ch");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_size, 15.0);
    assert_eq!(style.line_height, 75.0);
    assert_eq!(
        style.line_height_value,
        ComputedLineHeight::Length(ComputedLengthPercentage::from_ch(5.0))
    );

    style.resolve_font_metric_lengths(8.0);

    assert_eq!(style.line_height, 40.0);
    assert_eq!(
        style.line_height_value,
        ComputedLineHeight::Length(ComputedLengthPercentage::from_points(40.0))
    );
}

#[tokio::test]
async fn inherited_ch_line_height_keeps_metric_component() {
    let mut parent = default_style_for_tag("div");
    apply_declarations(&mut parent, &parse_declarations("line-height: 5ch"));

    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        &[],
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );

    assert_eq!(
        child.line_height_value,
        ComputedLineHeight::Length(ComputedLengthPercentage::from_ch(5.0))
    );
}

#[tokio::test]
async fn font_shorthand_ch_size_uses_parent_metric_fallback() {
    let mut parent = default_style_for_tag("div");
    parent.font_size = 20.0;

    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("font: 2ch/1 Ahem; width: 1em"),
        &[],
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );

    assert_eq!(child.font_size, 20.0);
    assert_eq!(child.line_height, 20.0);
    assert_eq!(
        child.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            20.0
        ))
    );
}

#[tokio::test]
async fn font_shorthand_em_line_height_uses_winning_computed_font_size() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div div { font: 20px/1em Ahem } #large { font-size: 40px }",
    ));
    let parent = default_style_for_tag("div");
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([("id".to_string(), "large".to_string())]),
        ),
        None,
        &[stylesheet],
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );

    assert_eq!(style.font_size, 30.0);
    assert_eq!(style.line_height, 30.0);
    assert_eq!(
        style.line_height_value,
        ComputedLineHeight::from_points(30.0)
    );
    assert_eq!(style.line_height_multiplier, None);
}

#[tokio::test]
async fn authored_font_family_names_are_preserved_for_system_resolution() {
    let mut style = default_style_for_tag("body");
    apply_declarations(
        &mut style,
        &parse_declarations(
            r#"font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif"#,
        ),
    );

    assert_eq!(
        style.font_family,
        FontFamily::Names(vec![
            "-apple-system".to_string(),
            "BlinkMacSystemFont".to_string(),
            "Segoe UI".to_string(),
            "Roboto".to_string(),
            "Helvetica Neue".to_string(),
            "Arial".to_string(),
            "sans-serif".to_string(),
        ])
    );
}

#[tokio::test]
async fn body_font_stack_survives_stylesheet_cascade() {
    let css = Css::from_string(
        r#"
        html { font-family: "Helvetica Neue", Arial, sans-serif; }
        body {
            font-size: 11px;
            line-height: 1.5;
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif, "Segoe UI Symbol";
        }
        body { color: black; }
        "#,
    );
    let stylesheet = parse_stylesheet(&css);
    let html = style_for_element_with_signature(
        ElementSignature::new("html", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let body = style_for_element_with_signature(
        ElementSignature::with_siblings("body", HashMap::new(), 1, vec!["head", "body"]),
        None,
        &[stylesheet],
        Some(&html),
        &[ElementSignature::new("html", HashMap::new())],
    );

    assert_eq!(
        body.font_family,
        FontFamily::Names(vec![
            "-apple-system".to_string(),
            "BlinkMacSystemFont".to_string(),
            "Segoe UI".to_string(),
            "Roboto".to_string(),
            "Helvetica Neue".to_string(),
            "Arial".to_string(),
            "sans-serif".to_string(),
            "Segoe UI Symbol".to_string(),
        ])
    );
}

#[tokio::test]
async fn heading_font_sizes_are_relative_to_inherited_font_size() {
    let ua = html5_user_agent_stylesheet();
    let mut parent = default_style_for_tag("body");
    apply_declarations(
        &mut parent,
        &parse_declarations("font-size: 11px; line-height: 1.5"),
    );

    let h1 = style_for_element_with_signature(
        ElementSignature::new("h1", HashMap::new()),
        None,
        std::slice::from_ref(&ua),
        Some(&parent),
        &[],
    );
    let h4 = style_for_element_with_signature(
        ElementSignature::new("h4", HashMap::new()),
        None,
        std::slice::from_ref(&ua),
        Some(&parent),
        &[],
    );

    assert_eq!(h1.display, Display::BLOCK);
    assert_eq!(h1.font_weight, FontWeight::BOLD);
    assert!((h1.font_size - 16.5).abs() < 0.001);
    assert!((h1.line_height - 24.75).abs() < 0.001);
    assert_eq!(h4.display, Display::BLOCK);
    assert_eq!(h4.font_weight, FontWeight::BOLD);
    assert_eq!(h4.font_size, 8.25);
    assert_eq!(h4.line_height, 12.375);
}

#[tokio::test]
async fn records_specificity() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: blue } .lead { color: red } #hero { color: green }",
    ));
    assert!(stylesheet.rules[0].specificity < stylesheet.rules[1].specificity);
    assert!(stylesheet.rules[1].specificity < stylesheet.rules[2].specificity);
}

#[tokio::test]
async fn cascade_layers_order_normal_declarations_before_specificity() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer theme { p { color: blue } }\
         @layer base { #hero { color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("id".to_string(), "hero".to_string())])),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn unlayered_normal_declarations_outrank_layered_declarations() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer theme { #hero { color: blue } } p { color: red }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("id".to_string(), "hero".to_string())])),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn important_cascade_layers_reverse_layer_order() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p { color: red !important } }\
         @layer theme { #hero { color: blue !important } }\
         p { color: green !important }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("id".to_string(), "hero".to_string())])),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn cascade_origin_orders_user_between_ua_and_author_for_normal_declarations() {
    let ua = parse_stylesheet(&Css::from_string("p { color: red }").with_user_agent_origin());
    let user = parse_stylesheet(&Css::from_string("p { color: green }").with_user_origin());
    let author = parse_stylesheet(&Css::from_string("p { color: blue }"));

    let user_over_ua = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[ua, user.clone()],
        None,
        &[],
    );
    assert_eq!(user_over_ua.color, Color::new(0, 128, 0));

    let author_over_user = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[user, author],
        None,
        &[],
    );
    assert_eq!(author_over_user.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn cascade_origin_orders_important_declarations_author_user_ua() {
    let author =
        parse_stylesheet(&Css::from_string("p { color: blue !important }").with_author_origin());
    let user =
        parse_stylesheet(&Css::from_string("p { color: green !important }").with_user_origin());
    let ua =
        parse_stylesheet(&Css::from_string("p { color: red !important }").with_user_agent_origin());

    let user_over_author = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[author, user.clone()],
        None,
        &[],
    );
    assert_eq!(user_over_author.color, Color::new(0, 128, 0));

    let ua_over_user = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[user, ua],
        None,
        &[],
    );
    assert_eq!(ua_over_user.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn cascade_origin_applies_to_page_context_declarations() {
    let user = parse_stylesheet(&Css::from_string("@page { margin: 1in }").with_user_origin());
    let author = parse_stylesheet(&Css::from_string("@page { margin: 2in }"));
    let author_important = parse_stylesheet(&Css::from_string("@page { margin: 3in !important }"));
    let user_important =
        parse_stylesheet(&Css::from_string("@page { margin: 4in !important }").with_user_origin());

    let normal_page_rules = user
        .page_rules
        .iter()
        .chain(author.page_rules.iter())
        .cloned()
        .collect::<Vec<_>>();
    let normal = page_margins_from(
        &cascade_page_declarations(&normal_page_rules, 1),
        PageMargins::all_points(0.0),
    );
    assert_eq!(normal, PageMargins::all_points(144.0));

    let important_page_rules = author_important
        .page_rules
        .iter()
        .chain(user_important.page_rules.iter())
        .cloned()
        .collect::<Vec<_>>();
    let important = page_margins_from(
        &cascade_page_declarations(&important_page_rules, 1),
        PageMargins::all_points(0.0),
    );
    assert_eq!(important, PageMargins::all_points(288.0));
}

#[tokio::test]
async fn layer_statement_sets_order_before_layer_blocks() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer theme, base;\
         @layer base { p { color: red } }\
         @layer theme { #hero { color: blue } }",
    ));
    assert_eq!(stylesheet.layer_names, vec!["theme", "base"]);

    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("id".to_string(), "hero".to_string())])),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn scoped_rule_proximity_outranks_later_unscoped_source_order() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@scope (.card) { p { color: red } } p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "card".to_string())]),
        )],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn closer_scoped_rule_outranks_farther_scope_before_source_order() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@scope (.inner) { p { color: blue } } @scope (.outer) { p { color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[
            ElementSignature::new(
                "section",
                HashMap::from([("class".to_string(), "outer".to_string())]),
            ),
            ElementSignature::new(
                "div",
                HashMap::from([("class".to_string(), "inner".to_string())]),
            ),
        ],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn scoped_rule_limit_excludes_descendant_subtree() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@scope (.card) to (.details) { p { color: red } } p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[
            ElementSignature::new(
                "section",
                HashMap::from([("class".to_string(), "card".to_string())]),
            ),
            ElementSignature::new(
                "div",
                HashMap::from([("class".to_string(), "details".to_string())]),
            ),
        ],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn scoped_rule_scope_pseudo_matches_scoping_root() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@scope (.card) { :scope { color: red } } .card { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "card".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn scoped_rule_scope_pseudo_matches_child_combinators_from_scope_root() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@scope (.card) { :scope > p { color: red } } p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "card".to_string())]),
        )],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn scoped_rule_relative_selector_uses_scope_root_as_anchor() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@scope (.card) { > p { color: red } } p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "card".to_string())]),
        )],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn scoped_pseudo_rules_use_originating_element_proximity() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@scope (.card) { p::before { content: \"x\"; color: red } }\
         p::before { content: \"x\"; color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "card".to_string())]),
        )],
    );

    let before = style.before_style.expect("scoped before style");
    assert_eq!(before.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn cascade_layers_apply_to_generated_pseudo_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p::before { content: \"x\"; color: red } }\
         @layer theme { p::before { content: \"x\"; color: blue } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(
        style.before_style.as_ref().unwrap().color,
        Color::new(0, 0, 255)
    );
}

#[tokio::test]
async fn supports_rule_applies_supported_declaration_conditions() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (display: flex) { p { color: blue } }\
         @supports (float: inline-start) { p { background-color: red } }\
         @supports (clear: inline-end) { p { border-top-color: green } }\
         @supports (border-spacing: 1ch 2ch) { p { border-spacing: 1ch 2ch } }\
         @supports (border-width: 1ch 2ch) { p { border-width: 1ch 2ch } }\
         @supports (outline-width: 3ch) { p { outline-width: 3ch } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.background_color, Some(Color::new(255, 0, 0)));
    assert_eq!(style.border_colors.top, Color::new(0, 128, 0));
    assert_eq!(
        style.border_spacing.horizontal,
        ComputedLengthPercentage::from_ch(1.0)
    );
    assert_eq!(
        style.border_spacing.vertical,
        ComputedLengthPercentage::from_ch(2.0)
    );
    assert_eq!(
        style.border_width_values.top,
        ComputedLengthPercentage::from_ch(1.0)
    );
    assert_eq!(
        style.border_width_values.right,
        ComputedLengthPercentage::from_ch(2.0)
    );
    assert_eq!(
        style.outline_width_value,
        ComputedLengthPercentage::from_ch(3.0)
    );
}

#[tokio::test]
async fn supports_rule_recognizes_stacking_context_trigger_properties() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (position: sticky) { p { color: blue } }\
         @supports (isolation: isolate) { p { background-color: red } }\
         @supports (mix-blend-mode: multiply) { p { border-top-color: green } }\
         @supports (filter: blur(0)) { p { border-right-color: blue } }\
         @supports (clip-path: inset(0)) { p { outline-color: green } }\
         @supports (mask-image: linear-gradient(black, black)) { p { border-bottom-color: red } }\
         @supports (contain: paint) { p { border-left-color: green } }\
         @supports (content-visibility: auto) { p { font-size: 20px } }\
         @supports (will-change: transform, opacity) { p { line-height: 24px } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.background_color, Some(Color::new(255, 0, 0)));
    assert_eq!(style.border_colors.top, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.right, Color::new(0, 0, 255));
    assert_eq!(style.outline_color, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, Color::new(255, 0, 0));
    assert_eq!(style.border_colors.left, Color::new(0, 128, 0));
    assert_eq!(style.font_size, 15.0);
    assert_eq!(style.line_height, 18.0);
}

#[tokio::test]
async fn supports_rule_recognizes_font_size_adjust_values() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (font-size-adjust: 0.9) { p { color: blue } }\
         @supports (font-size-adjust: cap-height) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.background_color, None);
}

#[tokio::test]
async fn supports_rule_recognizes_text_orientation_values() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (text-orientation: upright) { p { color: blue } }\
         @supports (text-orientation: sideways) { p { border-top-color: green } }\
         @supports (text-orientation: sideways-right) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.border_colors.top, Color::new(0, 128, 0));
    assert_eq!(style.background_color, None);
}

#[tokio::test]
async fn supports_rule_recognizes_text_align_justify_all_only_on_text_align() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (text-align: justify-all) { p { color: blue } }\
         @supports (text-align-all: match-parent) { p { border-top-color: green } }\
         @supports (text-align-all: justify-all) { p { outline-color: red } }\
         @supports (text-align-last: justify-all) { p { background-color: red } }\
         @supports (tab-size: 4) { p { border-bottom-color: blue } }\
         @supports (tab-size: 12pt) { p { border-left-color: green } }\
         @supports (tab-size: -1) { p { border-right-color: red } }\
         @supports (vertical-align: 25%) { p { outline-color: green } }\
         @supports (dominant-baseline: central) { p { border-top-width: 3pt } }\
         @supports (alignment-baseline: text-top) { p { border-right-width: 3pt } }\
         @supports (baseline-source: last) { p { border-bottom-width: 3pt } }\
         @supports (baseline-shift: super) { p { border-left-width: 3pt } }\
         @supports (baseline-source: middle) { p { outline-style: solid } }\
         @supports (vertical-align: banana) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.border_colors.top, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, Color::new(0, 0, 255));
    assert_eq!(style.border_colors.left, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.right, Color::BLACK);
    assert_eq!(style.border_widths.top, 3.0);
    assert_eq!(style.border_widths.right, 3.0);
    assert_eq!(style.border_widths.bottom, 3.0);
    assert_eq!(style.border_widths.left, 3.0);
    assert_eq!(style.outline_color, Color::new(0, 128, 0));
    assert_eq!(style.outline_style, BorderStyle::None);
    assert_eq!(style.background_color, None);
}

#[tokio::test]
async fn supports_rule_recognizes_text_transform_full_width() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (text-transform: full-width) { p { color: blue } }\
         @supports (text-transform: uppercase full-width full-size-kana) { p { border-top-color: green } }\
         @supports (text-transform: math-auto) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.border_colors.top, Color::new(0, 128, 0));
    assert_eq!(style.background_color, None);
}

#[tokio::test]
async fn supports_rule_recognizes_css_text_decoration_level_three_values() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (text-decoration: underline dotted red) { p { color: blue } }\
         @supports (text-underline-position: under right) { p { background-color: red } }\
         @supports (text-emphasis: open sesame green) { p { border-top-color: green } }\
         @supports (text-shadow: red 1px 2px 3px, 0px 0px blue) { p { border-right-color: blue } }\
         @supports (text-shadow: 0px 0px 10px currentcolor) { p { outline-color: green } }\
         @supports (box-shadow: inset 60px 0 green, currentcolor -2px 3px 0 -1px) { p { border-bottom-color: blue } }\
         @supports (box-shadow: 1px 2px -3px red) { p { border-left-color: red } }\
         @supports (text-decoration-line: underline underline) { p { border-bottom-color: red } }\
         @supports (text-underline-position: from-font) { p { border-left-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.background_color, Some(Color::new(255, 0, 0)));
    assert_eq!(style.border_colors.top, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, Color::new(0, 0, 255));
    assert_ne!(style.border_colors.left, Color::new(255, 0, 0));
    assert_eq!(style.border_colors.right, Color::new(0, 0, 255));
    assert_eq!(style.outline_color, Color::new(0, 128, 0));
    assert_eq!(style.border_colors.left, Color::BLACK);
}

#[tokio::test]
async fn supports_rule_ignores_unsupported_declaration_conditions() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (unsupported-reasyprint-feature: true) { p { color: blue } } p { color: red }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn supports_rule_evaluates_not_and_or_conditions() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports ((display: flex) and (not (unsupported: yes))) { p { color: blue } }\
         @supports ((unsupported: yes) or (display: flex)) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.background_color, Some(Color::new(255, 0, 0)));
}

#[tokio::test]
async fn supports_rule_evaluates_selector_conditions_with_selector_parser() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports selector(:scope > p) { p { color: blue } }\
         @supports selector(p:has(> span)) { p { border-top-color: lime } }\
         @supports selector(p:hover) { p { border-right-color: blue } }\
         @supports selector(p::first-line) { p { border-bottom-color: red } }\
         @supports selector(p::unsupported-reasyprint-pseudo) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.border_colors.top, Color::new(0, 255, 0));
    assert_eq!(style.border_colors.right, Color::new(0, 0, 255));
    assert_eq!(style.border_colors.bottom, Color::new(255, 0, 0));
    assert_eq!(style.background_color, None);
}

#[tokio::test]
async fn supports_rule_rejects_shadow_and_unmodeled_ui_pseudo_selectors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports selector(:host) { p { color: red } }\
         @supports selector(p::slotted(span)) { p { background-color: red } }\
         @supports selector(p::part(label)) { p { border-top-color: red } }\
         @supports selector(p::selection) { p { border-right-color: red } }\
         @supports selector(p::highlight(search)) { p { border-bottom-color: red } }\
         p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    assert_eq!(style.background_color, None);
    assert_eq!(style.border_colors.top, Color::new(0, 0, 0));
    assert_eq!(style.border_colors.right, Color::new(0, 0, 0));
    assert_eq!(style.border_colors.bottom, Color::new(0, 0, 0));
}

#[tokio::test]
async fn supports_rule_preserves_nested_layer_order() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @supports (display: flex) { @layer theme { p { color: blue } } }\
         @layer base { #hero { color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("id".to_string(), "hero".to_string())])),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn import_layer_places_imported_rules_in_named_layer() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-import-layer-named-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("imported.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "#hero { color: blue }").unwrap();
    std::fs::write(
        &main_path,
        "@layer base, theme;\
         @import \"imported.css\" layer(base);\
         @layer theme { p { color: red } }",
    )
    .unwrap();

    let css = Css::from_file_async(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports_async()
        .await
        .unwrap()
        .into_iter()
        .map(|css| parse_stylesheet(&css))
        .collect::<Vec<_>>();
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("id".to_string(), "hero".to_string())])),
        None,
        &parsed_stylesheets,
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_media_not_print_keeps_import_out_of_cascade() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-import-media-not-print-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("imported.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "p { color: red }").unwrap();
    std::fs::write(
        &main_path,
        "@import \"imported.css\" not print; p { color: blue }",
    )
    .unwrap();

    let css = Css::from_file_async(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports_async()
        .await
        .unwrap()
        .into_iter()
        .map(|css| parse_stylesheet(&css))
        .collect::<Vec<_>>();
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &parsed_stylesheets,
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_media_not_screen_loads_import_in_print_context() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-import-media-not-screen-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("imported.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "p { color: red }").unwrap();
    std::fs::write(
        &main_path,
        "@import \"imported.css\" layer(base) not screen;",
    )
    .unwrap();

    let css = Css::from_file_async(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports_async()
        .await
        .unwrap()
        .into_iter()
        .map(|css| parse_stylesheet(&css))
        .collect::<Vec<_>>();
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &parsed_stylesheets,
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn anonymous_import_layer_important_beats_unlayered_important() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-import-layer-anonymous-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("imported.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "p { color: blue !important }").unwrap();
    std::fs::write(
        &main_path,
        "@import \"imported.css\" layer; p { color: red !important }",
    )
    .unwrap();

    let css = Css::from_file_async(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports_async()
        .await
        .unwrap()
        .into_iter()
        .map(|css| parse_stylesheet(&css))
        .collect::<Vec<_>>();
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &parsed_stylesheets,
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn revert_layer_rolls_property_back_to_previous_layer() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p { color: red } }\
         @layer theme { p { color: blue; color: revert-layer } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn revert_layer_shorthand_rolls_back_same_layer_longhands() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p { margin-left: 3pt } }\
         @layer theme { p { margin-left: 20pt; margin: revert-layer } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.margin.left, 3.0);
    assert_eq!(style.margin.top, 0.0);
    assert_eq!(style.margin.right, 0.0);
    assert_eq!(style.margin.bottom, 0.0);
}

#[tokio::test]
async fn revert_layer_longhand_rolls_back_same_layer_shorthands() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p { margin-left: 3pt } }\
         @layer theme { p { margin: 20pt; margin-left: revert-layer } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.margin.left, 3.0);
    assert_eq!(style.margin.top, 20.0);
    assert_eq!(style.margin.right, 20.0);
    assert_eq!(style.margin.bottom, 20.0);
}

#[tokio::test]
async fn cascade_expands_flex_shorthand_basis_only_and_grow_basis_forms() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p { flex: 20pt } }\
         @layer theme { p { flex: 2 30pt } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.flex_grow, 2.0);
    assert_eq!(style.flex_shrink, 1.0);
    assert_eq!(
        style.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(30.0))
    );
}

#[tokio::test]
async fn revert_layer_longhand_preserves_unaffected_shorthand_components() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p { flex-grow: 2; flex-direction: column; row-gap: 3pt } }\
         @layer theme { p { flex: 1 0 20pt; flex-grow: revert-layer; flex-flow: row wrap; flex-direction: revert-layer; gap: 10pt 20pt; row-gap: revert-layer } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.flex_grow, 2.0);
    assert_eq!(style.flex_shrink, 0.0);
    assert_eq!(
        style.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(20.0))
    );
    assert_eq!(style.flex_direction, FlexDirection::Column);
    assert_eq!(style.flex_wrap, FlexWrap::Wrap);
    assert_eq!(
        style.row_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(3.0))
    );
    assert_eq!(
        style.column_gap,
        ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(20.0))
    );
}

#[tokio::test]
async fn later_declaration_in_same_layer_overrides_revert_layer() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p { color: red } }\
         @layer theme { p { color: revert-layer; color: blue } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn unlayered_revert_layer_rolls_back_to_strongest_layered_value() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base { p { color: blue } } p { color: revert-layer }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn author_revert_rolls_property_back_to_user_origin() {
    let ua = parse_stylesheet(&Css::from_string("p { color: red }").with_user_agent_origin());
    let user = parse_stylesheet(&Css::from_string("p { color: green }").with_user_origin());
    let author = parse_stylesheet(&Css::from_string("p { color: blue; color: revert }"));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[ua, user, author],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(0, 128, 0));
}

#[tokio::test]
async fn user_important_revert_rolls_property_back_to_ua_origin() {
    let ua = parse_stylesheet(&Css::from_string("p { color: red }").with_user_agent_origin());
    let author = parse_stylesheet(&Css::from_string("p { color: blue !important }"));
    let user = parse_stylesheet(
        &Css::from_string("p { color: green !important; color: revert !important }")
            .with_user_origin(),
    );
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[ua, author, user],
        None,
        &[],
    );

    assert_eq!(style.color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn ua_origin_revert_behaves_like_unset_for_non_inherited_modeled_property() {
    let ua = parse_stylesheet(
        &Css::from_string("p { margin-left: 12pt; margin-left: revert }").with_user_agent_origin(),
    );
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[ua],
        None,
        &[],
    );

    assert_eq!(style.margin.left, 0.0);
}

#[tokio::test]
async fn revert_shorthand_rolls_back_current_origin_longhands() {
    let ua = parse_stylesheet(&Css::from_string("p { margin-left: 3pt }").with_user_agent_origin());
    let author = parse_stylesheet(&Css::from_string("p { margin-left: 20pt; margin: revert }"));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[ua, author],
        None,
        &[],
    );

    assert_eq!(style.margin.left, 3.0);
    assert_eq!(style.margin.top, 0.0);
    assert_eq!(style.margin.right, 0.0);
    assert_eq!(style.margin.bottom, 0.0);
}

#[tokio::test]
async fn initial_keyword_resets_modeled_property_to_initial_value() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { margin-left: 12pt; margin-left: initial }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.margin.left, 0.0);
}

#[tokio::test]
async fn inherit_keyword_uses_defaulted_parent_value_for_non_inherited_property() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("margin-left: 9pt"),
        &[],
        None,
        &[],
    );
    let stylesheet = parse_stylesheet(&Css::from_string("p { margin-left: inherit }"));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[],
    );

    assert_eq!(style.margin.left, 9.0);
}

#[tokio::test]
async fn unset_keyword_inherits_inherited_properties_and_initializes_others() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("color: green; display: block"),
        &[],
        None,
        &[],
    );
    let stylesheet = parse_stylesheet(&Css::from_string("p { color: unset; display: unset }"));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[],
    );

    assert_eq!(style.color, Color::new(0, 128, 0));
    assert_eq!(style.display, Display::INLINE);
}

#[tokio::test]
async fn normal_inheritance_uses_modeled_inherited_property_metadata() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("letter-spacing: 4pt; writing-mode: vertical-rl; text-orientation: upright"),
        &[],
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[],
        Some(&parent),
        &[],
    );

    assert_eq!(child.letter_spacing, parent.letter_spacing);
    assert_eq!(child.text_orientation, TextOrientation::Upright);
}

#[tokio::test]
async fn unset_keyword_uses_correct_property_inheritance_flags() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("letter-spacing: 5pt; overflow-x: hidden"),
        &[],
        None,
        &[],
    );
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { letter-spacing: unset; overflow-x: unset }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[],
    );

    assert_eq!(style.letter_spacing, parent.letter_spacing);
    assert_eq!(style.overflow_x, Overflow::Visible);
}

#[tokio::test]
async fn all_property_applies_css_wide_keyword_to_modeled_longhands() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: red; display: block; margin-left: 12pt; all: initial }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, Color::BLACK);
    assert_eq!(style.display, Display::INLINE);
    assert_eq!(style.margin.left, 0.0);
}

#[tokio::test]
async fn revert_layer_applies_to_generated_pseudo_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { p::before { content: \"x\"; color: red } }\
         @layer theme { p::before { content: \"x\"; color: blue; color: revert-layer } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(
        style.before_style.as_ref().unwrap().color,
        Color::new(255, 0, 0)
    );
}

#[tokio::test]
async fn revert_layer_applies_to_page_context_declarations() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { @page :first { margin: 1in } }\
         @layer theme { @page { margin: 2in } @page :first { margin: revert-layer } }",
    ));

    let margins = page_margins_from(
        &stylesheet.first_page_declarations,
        PageMargins::all_points(0.0),
    );

    assert_eq!(margins, PageMargins::all_points(72.0));
}

#[tokio::test]
async fn revert_applies_to_page_context_declarations() {
    let ua = parse_stylesheet(&Css::from_string("@page { margin: 1in }").with_user_agent_origin());
    let author = parse_stylesheet(&Css::from_string("@page { margin: 2in; margin: revert }"));
    let page_rules = ua
        .page_rules
        .iter()
        .chain(author.page_rules.iter())
        .cloned()
        .collect::<Vec<_>>();

    let margins = page_margins_from(
        &cascade_page_declarations(&page_rules, 1),
        PageMargins::all_points(0.0),
    );

    assert_eq!(margins, PageMargins::all_points(72.0));
}

#[tokio::test]
async fn user_important_revert_applies_to_page_context_declarations() {
    let ua = parse_stylesheet(&Css::from_string("@page { margin: 1in }").with_user_agent_origin());
    let author = parse_stylesheet(&Css::from_string("@page { margin: 2in !important }"));
    let user = parse_stylesheet(
        &Css::from_string("@page { margin: 3in !important; margin: revert !important }")
            .with_user_origin(),
    );
    let page_rules = ua
        .page_rules
        .iter()
        .chain(author.page_rules.iter())
        .chain(user.page_rules.iter())
        .cloned()
        .collect::<Vec<_>>();

    let margins = page_margins_from(
        &cascade_page_declarations(&page_rules, 1),
        PageMargins::all_points(0.0),
    );

    assert_eq!(margins, PageMargins::all_points(72.0));
}

#[tokio::test]
async fn revert_layer_page_margin_shorthand_rolls_back_same_layer_longhands() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { @page :first { margin-left: 1in } }\
         @layer theme { @page :first { margin-left: 2in; margin: revert-layer } }",
    ));

    let margins = page_margins_from(
        &stylesheet.first_page_declarations,
        PageMargins::all_points(0.0),
    );

    assert_eq!(margins.left(), 72.0);
    assert_eq!(margins.top(), 0.0);
    assert_eq!(margins.right(), 0.0);
    assert_eq!(margins.bottom(), 0.0);
}

#[tokio::test]
async fn page_margin_longhand_override_survives_cascade_output() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { margin: .5in 1in .75in .25in; margin-left: .125in }",
    ));

    let margins = page_margins_from(&stylesheet.page_declarations, PageMargins::all_points(0.0));

    assert_eq!(margins.top(), 36.0);
    assert_eq!(margins.right(), 72.0);
    assert_eq!(margins.bottom(), 54.0);
    assert_eq!(margins.left(), 9.0);
}

#[tokio::test]
async fn revert_layer_applies_to_page_margin_box_declarations() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer base { @page { @top-left { content: \"base\" } } }\
         @layer theme { @page { @top-left { content: \"theme\"; content: revert-layer } } }",
    ));

    let top_left = stylesheet.page_margin_boxes.get("top-left").unwrap();

    assert_eq!(
        top_left.get("content").map(String::as_str),
        Some("\"base\"")
    );
}

#[tokio::test]
async fn import_layer_applies_to_imported_page_context() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-import-layer-page-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("imported.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "@page :first { margin: 1in }").unwrap();
    std::fs::write(
        &main_path,
        "@layer base, theme;\
         @import \"imported.css\" layer(base);\
         @layer theme { @page { margin: 2in } }",
    )
    .unwrap();

    let css = Css::from_file_async(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports_async()
        .await
        .unwrap()
        .into_iter()
        .map(|css| parse_stylesheet(&css))
        .collect::<Vec<_>>();
    let page_rules = parsed_stylesheets
        .iter()
        .flat_map(|stylesheet| stylesheet.page_rules.iter().cloned())
        .collect::<Vec<_>>();
    let declarations = cascade_page_declarations(&page_rules, 1);
    let margins = page_margins_from(&declarations, PageMargins::all_points(0.0));

    assert_eq!(margins, PageMargins::all_points(144.0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn expands_nested_style_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".outer { color: black; .inner, &:last-child { color: red } }",
    ));
    let selectors = stylesheet
        .rules
        .iter()
        .map(|rule| rule.selector_text.as_str())
        .collect::<Vec<_>>();

    assert!(selectors.contains(&".outer"));
    assert!(selectors.contains(&".outer .inner"));
    assert!(selectors.contains(&".outer:last-child"));
}

#[tokio::test]
async fn expands_nested_ampersand_id_rules_with_unitless_zero_offsets() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "table { &#total { bottom: 0; position: absolute; width: 18cm } }",
    ));
    let parent = default_style_for_tag("body");
    let signature = ElementSignature::with_siblings(
        "table",
        HashMap::from([("id".to_string(), "total".to_string())]),
        0,
        vec!["table".to_string()],
    );
    let style =
        style_for_element_with_signature(signature, None, &[stylesheet], Some(&parent), &[]);

    assert_eq!(style.position, Position::Absolute);
    assert_eq!(
        style.box_values.inset_bottom,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::ZERO)
    );
    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage {
            length: layout_pt(18.0 * 28.346_457),
            percent: 0.0,
            ch: 0.0,
            ..ComputedLengthPercentage::ZERO
        })
    );
}

#[tokio::test]
async fn expands_nested_of_type_table_cell_selectors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "th, td { text-align: center; &:first-of-type { text-align: left } &:last-of-type { text-align: right } }",
    ));
    let selectors = stylesheet
        .rules
        .iter()
        .map(|rule| rule.selector_text.as_str())
        .collect::<Vec<_>>();

    assert!(selectors.contains(&"th:first-of-type"));
    assert!(selectors.contains(&"td:first-of-type"));
    assert!(selectors.contains(&"th:last-of-type"));
    assert!(selectors.contains(&"td:last-of-type"));

    let parent = default_style_for_tag("tr");
    let first = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "td",
            HashMap::new(),
            0,
            vec!["td".to_string(), "td".to_string(), "td".to_string()],
        ),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[],
    );
    let last = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "td",
            HashMap::new(),
            2,
            vec!["td".to_string(), "td".to_string(), "td".to_string()],
        ),
        None,
        &[stylesheet],
        Some(&parent),
        &[],
    );

    assert_eq!(first.text_align, TextAlign::Left);
    assert_eq!(last.text_align, TextAlign::Right);
}

#[tokio::test]
async fn expands_nested_id_descendant_heading_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "#ticket { h2 { font-weight: 300; margin: 0; text-transform: uppercase } }",
    ));
    let mut selectors = stylesheet
        .rules
        .iter()
        .map(|rule| rule.selector_text.as_str());

    assert!(selectors.any(|s| s == "#ticket h2"));

    let parent = style_for_element_with_signature(
        ElementSignature::new(
            "section",
            HashMap::from([("id".to_string(), "ticket".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&default_style_for_tag("body")),
        &[],
    );
    let h2 = style_for_element_with_signature(
        ElementSignature::new("h2", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[ElementSignature::new(
            "section",
            HashMap::from([("id".to_string(), "ticket".to_string())]),
        )],
    );

    assert_eq!(h2.font_weight, FontWeight(300));
    assert_eq!(h2.margin, Edges::ZERO);
    assert_eq!(
        h2.text_transform,
        TextTransform {
            case: TextTransformCase::Uppercase,
            full_width: false,
            full_size_kana: false,
        }
    );
}

#[tokio::test]
async fn expands_nested_information_heading_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "#informations { flex: 1; padding: 0; position: relative; h1 { display: inline-block; font-size: 25pt; font-weight: 300; text-transform: uppercase; } #name { margin-left: 1cm; } #destination { position: absolute; right: 1cm; } }",
    ));
    let selectors = stylesheet
        .rules
        .iter()
        .map(|rule| rule.selector_text.as_str())
        .collect::<Vec<_>>();

    assert!(selectors.contains(&"#informations h1"));
    assert!(selectors.contains(&"#informations #name"));
    assert!(selectors.contains(&"#informations #destination"));

    let ua = html5_user_agent_stylesheet();
    let stylesheets = [ua, stylesheet];
    let parent = style_for_element_with_signature(
        ElementSignature::new(
            "section",
            HashMap::from([("id".to_string(), "informations".to_string())]),
        ),
        None,
        &stylesheets,
        Some(&default_style_for_tag("body")),
        &[],
    );
    let h1 = style_for_element_with_signature(
        ElementSignature::new(
            "h1",
            HashMap::from([("id".to_string(), "name".to_string())]),
        ),
        None,
        &stylesheets,
        Some(&parent),
        &[ElementSignature::new(
            "section",
            HashMap::from([("id".to_string(), "informations".to_string())]),
        )],
    );

    assert_eq!(h1.display, Display::INLINE_BLOCK);
    assert_eq!(h1.font_size, 25.0);
    assert_eq!(h1.font_weight, FontWeight(300));
    assert_eq!(
        h1.text_transform,
        TextTransform {
            case: TextTransformCase::Uppercase,
            full_width: false,
            full_size_kana: false,
        }
    );
    assert!(
        (h1.margin.top - 16.75).abs() < 0.01,
        "top margin was {}",
        h1.margin.top
    );
    assert!(
        (h1.margin.bottom - 16.75).abs() < 0.01,
        "bottom margin was {}",
        h1.margin.bottom
    );
    assert!((h1.margin.left - 28.346_457).abs() < 0.01);
}

#[tokio::test]
async fn expands_deep_nested_invoice_table_cell_selectors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "table { td { &:last-of-type { color: #1ee494; font-weight: bold; text-align: right } } th, td { text-align: center; &:first-of-type { text-align: left } &:last-of-type { text-align: right } } }",
    ));
    let selectors = stylesheet
        .rules
        .iter()
        .map(|rule| rule.selector_text.as_str())
        .collect::<Vec<_>>();

    assert!(selectors.contains(&"table td:last-of-type"));
    assert!(selectors.contains(&"table th:last-of-type"));
    assert!(selectors.contains(&"table td:first-of-type"));

    let parent = default_style_for_tag("tr");
    let table = ElementSignature::new("table", HashMap::new());
    let last = style_for_element_with_signature(
        ElementSignature::with_siblings(
            "td",
            HashMap::new(),
            2,
            vec!["td".to_string(), "td".to_string(), "td".to_string()],
        ),
        None,
        &[stylesheet],
        Some(&parent),
        &[table],
    );

    assert_eq!(last.text_align, TextAlign::Right);
    assert_eq!(last.font_weight, FontWeight::BOLD);
    assert_eq!(last.color, Color::new(30, 228, 148));
}

#[tokio::test]
async fn invoice_nested_aside_margin_uses_three_value_shorthand() {
    let css = Css::from_file_async("weasyprint-samples/invoice/invoice.css")
        .await
        .unwrap();
    let stylesheet = parse_stylesheet(&css);
    let html = style_for_element_with_signature(
        ElementSignature::new("html", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&ComputedStyle::initial()),
        &[],
    );
    let parent = style_for_element_with_signature(
        ElementSignature::new("body", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&html),
        &[],
    );
    let aside = style_for_element_with_signature(
        ElementSignature::new("aside", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[],
    );

    assert_eq!(aside.display, Display::FLEX);
    assert!((aside.margin.top - 22.0).abs() < 0.01);
    assert!((aside.margin.right - 0.0).abs() < 0.01);
    assert!((aside.margin.bottom - 44.0).abs() < 0.01);
    assert!((aside.margin.left - 0.0).abs() < 0.01);
}

#[tokio::test]
async fn parses_break_inside_avoid() {
    let declarations = parse_declarations("page-break-inside: avoid");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(style.break_inside_avoid);
}

#[tokio::test]
async fn parses_named_page_property() {
    let declarations = parse_declarations("page: report");
    let mut style = default_style_for_tag("section");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.page_name.as_deref(), Some("report"));
    assert!(style.page_name_specified);

    let declarations = parse_declarations("page: auto");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.page_name, None);
    assert!(style.page_name_specified);
}

#[tokio::test]
async fn parses_running_position_property() {
    let declarations = parse_declarations("position: running(header)");
    let mut style = default_style_for_tag("h1");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.position, Position::Static);
    assert_eq!(style.running_element_name.as_deref(), Some("header"));

    apply_declarations(&mut style, &parse_declarations("position: relative"));
    assert_eq!(style.position, Position::Relative);
    assert_eq!(style.running_element_name, None);
}

#[tokio::test]
async fn parses_orphans_and_widows_fragmentation_controls() {
    let declarations = parse_declarations("orphans: 3; widows: 4");
    let mut style = ComputedStyle::initial();
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.orphans, 3);
    assert_eq!(style.widows, 4);

    let declarations = parse_declarations("orphans: 0; widows: 1.5");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.orphans, 3);
    assert_eq!(style.widows, 4);
}

#[tokio::test]
async fn parses_position_z_index_auto_and_integer_stack_levels() {
    let declarations = parse_declarations("position: absolute; z-index: 7");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.position, Position::Absolute);
    assert_eq!(style.z_index, Some(7));

    let declarations = parse_declarations("z-index: auto");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.z_index, None);
}

#[tokio::test]
async fn parses_opacity_numbers_and_percentages() {
    let declarations = parse_declarations("opacity: 50%; color: red");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.opacity, 0.5);

    let declarations = parse_declarations("opacity: 2");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.opacity, 1.0);
}

#[tokio::test]
async fn parses_transform_functions_and_origin() {
    let declarations = parse_declarations(
        "transform: translate(10pt, 25%) scaleX(2) rotate(90deg) skewY(0.25turn) matrix(1, 2, 3, 4, 5, 6); transform-origin: right 20pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.transform,
        vec![
            TransformFunction::Translate(
                ComputedLengthPercentage::from_points(10.0),
                ComputedLengthPercentage::from_percent(0.25),
            ),
            TransformFunction::Scale(2.0, 1.0),
            TransformFunction::Rotate(std::f32::consts::FRAC_PI_2),
            TransformFunction::Skew(0.0, std::f32::consts::FRAC_PI_2),
            TransformFunction::Matrix(1.0, 2.0, 3.0, 4.0, 5.0, 6.0),
        ]
    );
    assert_eq!(
        style.transform_origin,
        TransformOrigin {
            x: ComputedLengthPercentage::from_percent(1.0),
            y: ComputedLengthPercentage::from_points(20.0),
        }
    );
}

#[tokio::test]
async fn ch_transform_lengths_preserve_font_metric_component_until_used_resolution() {
    let declarations =
        parse_declarations("transform: translate(2ch, 25%); transform-origin: 3ch 4ch");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.transform,
        vec![TransformFunction::Translate(
            ComputedLengthPercentage::from_ch(2.0),
            ComputedLengthPercentage::from_percent(0.25),
        )]
    );
    assert_eq!(
        style.transform_origin,
        TransformOrigin {
            x: ComputedLengthPercentage::from_ch(3.0),
            y: ComputedLengthPercentage::from_ch(4.0),
        }
    );

    style.resolve_font_metric_lengths(5.0);

    assert_eq!(
        style.transform,
        vec![TransformFunction::Translate(
            ComputedLengthPercentage::from_points(10.0),
            ComputedLengthPercentage::from_percent(0.25),
        )]
    );
    assert_eq!(
        style.transform_origin,
        TransformOrigin {
            x: ComputedLengthPercentage::from_points(15.0),
            y: ComputedLengthPercentage::from_points(20.0),
        }
    );
}

#[tokio::test]
async fn parses_outline_properties() {
    let declarations = parse_declarations(
        "color: #123456; outline: thick dashed currentColor; outline-offset: 2pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.outline_width, 5.0 * CSS_PX_TO_PT);
    assert_eq!(style.outline_style, BorderStyle::Dashed);
    assert_eq!(style.outline_color, Color::new(0x12, 0x34, 0x56));
    assert_eq!(
        style.outline_offset,
        ComputedLengthPercentage::from_points(2.0)
    );
}

#[tokio::test]
async fn ch_outline_offset_preserves_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("outline-offset: 2ch");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.outline_offset, ComputedLengthPercentage::from_ch(2.0));

    style.resolve_font_metric_lengths(4.0);

    assert_eq!(
        style.outline_offset,
        ComputedLengthPercentage::from_points(8.0)
    );
}

#[tokio::test]
async fn ch_outline_width_preserves_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("outline-width: 2ch; outline-style: solid");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.outline_width_value,
        ComputedLengthPercentage::from_ch(2.0)
    );
    assert_eq!(style.outline_width, 0.0);

    style.resolve_font_metric_lengths(4.0);

    assert_eq!(
        style.outline_width_value,
        ComputedLengthPercentage::from_points(8.0)
    );
    assert_eq!(style.outline_width, 8.0);
}

#[tokio::test]
async fn parses_gap_decoration_rule_properties() {
    let declarations = parse_declarations(
        "color: #123456;\
         column-rule: repeat(2, 10px solid red), repeat(auto, 4px dashed currentColor);\
         row-rule-width: 30px;\
         row-rule-style: solid;\
         row-rule-color: blue;\
         rule-inset-start: 2pt overlap-join;\
         rule-overlap: column-over-row",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let column_widths = style.column_rule.widths.values_for_count(4);
    let column_styles = style.column_rule.styles.values_for_count(4);
    let column_colors = style.column_rule.colors.values_for_count(4);
    assert_eq!(column_widths[0], ComputedLengthPercentage::from_points(7.5));
    assert_eq!(column_widths[1], ComputedLengthPercentage::from_points(7.5));
    assert_eq!(column_widths[2], ComputedLengthPercentage::from_points(3.0));
    assert_eq!(column_styles[0], BorderStyle::Solid);
    assert_eq!(column_styles[2], BorderStyle::Dashed);
    assert_eq!(column_colors[0], Color::new(255, 0, 0));
    assert_eq!(column_colors[2], Color::new(0x12, 0x34, 0x56));
    assert_eq!(
        style.row_rule.widths.values_for_count(1)[0],
        ComputedLengthPercentage::from_points(22.5)
    );
    assert_eq!(
        style.row_rule.styles.values_for_count(1)[0],
        BorderStyle::Solid
    );
    assert_eq!(
        style.row_rule.colors.values_for_count(1)[0],
        Color::new(0, 0, 255)
    );
    assert_eq!(
        style.column_rule.inset_cap_start,
        GapRuleInsetValue::LengthPercentage(ComputedLengthPercentage::from_points(2.0))
    );
    assert_eq!(
        style.column_rule.inset_junction_start,
        GapRuleInsetValue::OverlapJoin
    );
    assert_eq!(
        style.row_rule.inset_cap_start,
        GapRuleInsetValue::LengthPercentage(ComputedLengthPercentage::from_points(2.0))
    );
    assert_eq!(style.rule_overlap, GapRuleOverlap::ColumnOverRow);
}

#[tokio::test]
async fn parses_gap_rule_inset_shorthand_sides() {
    let declarations = parse_declarations("rule-inset: 1pt 2pt / 3pt 4pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.column_rule.inset_cap_start,
        GapRuleInsetValue::LengthPercentage(ComputedLengthPercentage::from_points(1.0))
    );
    assert_eq!(
        style.column_rule.inset_cap_end,
        GapRuleInsetValue::LengthPercentage(ComputedLengthPercentage::from_points(2.0))
    );
    assert_eq!(
        style.column_rule.inset_junction_start,
        GapRuleInsetValue::LengthPercentage(ComputedLengthPercentage::from_points(3.0))
    );
    assert_eq!(
        style.column_rule.inset_junction_end,
        GapRuleInsetValue::LengthPercentage(ComputedLengthPercentage::from_points(4.0))
    );
    assert_eq!(
        style.row_rule.inset_cap_start,
        style.column_rule.inset_cap_start
    );
    assert_eq!(
        style.row_rule.inset_cap_end,
        style.column_rule.inset_cap_end
    );
    assert_eq!(
        style.row_rule.inset_junction_start,
        style.column_rule.inset_junction_start
    );
    assert_eq!(
        style.row_rule.inset_junction_end,
        style.column_rule.inset_junction_end
    );
}
