use std::collections::HashMap;
use std::num::{NonZeroU32, NonZeroUsize};

use super::values::{
    edge_all, parse_computed_length_percentage, parse_computed_line_height,
    parse_deferred_font_size,
};
use super::*;
use crate::css::page::page_style_for_declarations;
use crate::layout::{PageMargins, PageSize};
use crate::units::{LayoutLength, layout_pt};
use crate::{LayoutSize, RenderOptions};

fn flex_basis_length(value: ComputedLengthPercentage) -> ComputedFlexBasis {
    ComputedFlexBasis::LengthPercentage(ComputedFlexBasisLength::new(value))
}

fn flex_basis_percentage(value: ComputedLengthPercentage) -> ComputedFlexBasis {
    ComputedFlexBasis::LengthPercentage(ComputedFlexBasisLength::new(value))
}

fn list_style_image_url(style: &ComputedStyle) -> Option<&str> {
    match style.list_style_image.as_image()?.selected_image() {
        BackgroundImage::Url(url) => Some(&url.href),
        _ => None,
    }
}

#[test]
fn image_rendering_preserves_each_specified_keyword() {
    assert_eq!(parse_image_rendering("auto"), Some(ImageRendering::Auto));
    assert_eq!(
        parse_image_rendering("smooth"),
        Some(ImageRendering::Smooth)
    );
    assert_eq!(
        parse_image_rendering("high-quality"),
        Some(ImageRendering::HighQuality)
    );
    assert_eq!(
        parse_image_rendering("pixelated"),
        Some(ImageRendering::Pixelated)
    );
    assert_eq!(
        parse_image_rendering("crisp-edges"),
        Some(ImageRendering::CrispEdges)
    );
    assert_eq!(
        parse_image_rendering("optimizeSpeed"),
        Some(ImageRendering::CrispEdges)
    );
    assert_eq!(
        parse_image_rendering("optimizeQuality"),
        Some(ImageRendering::Smooth)
    );
}

#[test]
fn image_rendering_cascades_and_inherits_the_specified_keyword() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("image-rendering: pixelated"),
        &Stylesheets::borrowed(&[html5_user_agent_stylesheet()]),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("img", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[html5_user_agent_stylesheet()]),
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );
    assert_eq!(child.image_rendering, ImageRendering::Pixelated);
}

#[test]
fn nonnegative_fixed_length_clamp_preserves_layout_length_type() {
    let clamped: LayoutLength = ComputedLengthPercentage::from_points(-3.0).length_max_zero();

    assert_eq!(clamped, layout_pt(0.0));
}

#[test]
fn baseline_shift_used_value_preserves_layout_length_type() {
    let shift: LayoutLength =
        BaselineShift::LengthPercentage(ComputedLengthPercentage::from_percent(0.5))
            .length_percentage_shift(layout_pt(20.0));

    assert_eq!(shift, layout_pt(10.0));
}

#[test]
fn background_attachment_longhand_updates_each_image_layer() {
    let declarations = parse_declarations(
        "background-image: url(first.png), url(second.png); background-attachment: fixed, local",
    );
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_attachment,
        BackgroundAttachment::Fixed
    );
    assert_eq!(style.background.background_layers.len(), 2);
    assert_eq!(
        style
            .background
            .background_layers
            .iter()
            .map(|layer| layer.attachment)
            .collect::<Vec<_>>(),
        vec![BackgroundAttachment::Fixed, BackgroundAttachment::Local],
    );
}

#[tokio::test]
async fn stylesheet_options_do_not_eagerly_apply_page_rules() {
    let css = Css::from_string("@page { size: 200px 100px; margin: 10px } p { font-size: 20px }");
    let mut options = RenderOptions::default();
    let initial_page_size = options.page_size;
    apply_stylesheet_options(&css, &mut options);

    // Page rules are page-context inputs, not global render options. Keeping
    // the initial page box intact gives every destination page the same
    // immutable fallback geometry, regardless of whether its declarations
    // use viewport units.
    assert_eq!(options.page_size, initial_page_size);
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
    let stylesheet = parse_stylesheet(&Css::from_string("@page { size: 5in }"));
    let size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);

    assert_eq!(size.width(), 360.0);
    assert_eq!(size.height(), 360.0);
}

#[tokio::test]
async fn page_size_viewport_units_use_the_initial_page_box() {
    let base = PageSize::from_points(360.0, 216.0);
    let stylesheet = parse_stylesheet(&Css::from_string("@page { size: 200vh 100vw }"));

    // CSS Paged Media resolves viewport units in page descriptors before the
    // authored page size replaces the default page box. WPT:
    // css/css-page/page-size-017-print.tentative.html.
    assert_eq!(
        page_size_from(&stylesheet.page_declarations, base),
        PageSize::from_points(432.0, 360.0)
    );
}

#[tokio::test]
async fn page_descriptors_use_the_document_root_font_metric_snapshot() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { font-size: 100pt; size: 2rem 3rch; margin: 1rlh; padding: 1rex }",
    ));
    let root_metrics = RootFontMetricLengthBasis {
        font_size: layout_pt(10.0),
        ch_advance: layout_pt(2.0),
        x_height: layout_pt(3.0),
        cap_height: layout_pt(4.0),
        ic_advance: layout_pt(5.0),
        line_height: layout_pt(6.0),
    };
    let page_size = page_size_from_with_ch_advance_and_root_metrics(
        &stylesheet.page_declarations,
        PageSize::A4_POINTS,
        layout_pt(50.0),
        root_metrics,
    );
    let mut page_style = ComputedStyle::initial();
    apply_declarations(&mut page_style, &stylesheet.page_declarations);
    let margins =
        page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style_and_root_metrics(
            &stylesheet.page_declarations,
            PageMargins::all_points(0.0),
            page_size,
            PageMarginResolutionContext {
                viewport_size: PageSize::A4_POINTS,
                non_margin_edges: Edges::ZERO,
                ch_advance: layout_pt(50.0),
                style: &page_style,
                root_metrics,
            },
        );
    let padding = page_padding_from_for_size_with_ch_advance_and_root_metrics(
        &stylesheet.page_declarations,
        page_size,
        layout_pt(50.0),
        root_metrics,
    );

    assert_eq!(page_size, PageSize::from_points(20.0, 6.0));
    assert_eq!(margins, PageMargins::all_points(6.0));
    assert_eq!(
        padding,
        Edges {
            top: 3.0,
            right: 3.0,
            bottom: 3.0,
            left: 3.0,
        }
    );
}

#[tokio::test]
async fn page_margin_viewport_units_use_the_initial_page_box() {
    let initial = PageSize::from_points(360.0, 216.0);
    let authored = PageSize::from_points(540.0, 432.0);
    let stylesheet = parse_stylesheet(&Css::from_string("@page { margin: 0; margin-top: 20vw }"));
    let style = page_style_for_declarations(&stylesheet.page_declarations);
    let margins = page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        authored,
        initial,
        Edges::ZERO,
        layout_pt(6.0),
        &style,
    );

    assert_eq!(margins.top(), 72.0);
}

#[tokio::test]
async fn page_size_auto_preserves_existing_page_size() {
    let stylesheet = parse_stylesheet(&Css::from_string("@page { size: auto }"));
    let base = PageSize::from_points(240.0, 120.0);

    assert_eq!(page_size_from(&stylesheet.page_declarations, base), base);
}

#[tokio::test]
async fn zero_sized_page_descriptors_fall_back_to_the_initial_page_size() {
    let base = PageSize::from_points(240.0, 120.0);
    for value in ["0", "4in 0", "0 4in"] {
        let stylesheet = parse_stylesheet(&Css::from_string(format!("@page {{ size: {value} }}")));
        assert_eq!(page_size_from(&stylesheet.page_declarations, base), base);
    }
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
async fn percentage_page_area_resizes_an_overconstrained_size_descriptor() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: 500px; width: 40%; height: 60%; margin: 10%; }",
    ));
    let initial_page = PageSize::from_points(360.0, 216.0);
    let page_size = page_size_from(&stylesheet.page_declarations, initial_page);
    let page_style = page_style_for_declarations(&stylesheet.page_declarations);
    let margins = page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        page_size,
        initial_page,
        Edges::ZERO,
        layout_pt(6.0),
        &page_style,
    );

    assert_eq!(page_size, PageSize::from_points(225.0, 300.0));
    assert_eq!(margins, PageMargins::all_points(37.5));
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
async fn page_margin_inherit_uses_document_root_margin() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: 6in; margin: 13px; margin: inherit }",
    ));
    let mut page_style = ComputedStyle::initial();
    page_style.margin = edge_all(36.0);
    apply_declarations(&mut page_style, &stylesheet.page_declarations);
    let margins = page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        PageSize::from_points(432.0, 432.0),
        PageSize::from_points(360.0, 216.0),
        Edges::ZERO,
        layout_pt(6.0),
        &page_style,
    );

    assert_eq!(margins, PageMargins::all_points(36.0));
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
async fn page_border_shorthand_uses_medium_width_when_omitted() {
    let stylesheet = parse_stylesheet(&Css::from_string("@page { border: solid }"));
    let style = page_style_for_declarations(&stylesheet.page_declarations);

    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_widths, edge_all(2.25));
}

#[tokio::test]
async fn border_style_uses_the_initial_medium_specified_width() {
    let mut unset = default_style_for_tag("div");
    unset.resolve_font_metric_lengths(layout_pt(5.0));
    assert_eq!(unset.border_widths, Edges::ZERO);

    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &parse_declarations("border-style: double"));

    assert_eq!(style.border_styles.top, BorderStyle::Double);
    assert_eq!(style.border_styles.right, BorderStyle::Double);
    assert_eq!(style.border_styles.bottom, BorderStyle::Double);
    assert_eq!(style.border_styles.left, BorderStyle::Double);
    assert_eq!(style.border_widths, edge_all(3.0 * CSS_PX_TO_PT));

    let mut explicit_zero = default_style_for_tag("div");
    apply_declarations(
        &mut explicit_zero,
        &parse_declarations("border-style: double; border-width: 0"),
    );
    assert_eq!(explicit_zero.border_widths, Edges::ZERO);
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
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { size: letter; margin: .5in 1in .75in .25in; margin-left: .125in }",
    ));
    let size = page_size_from(&stylesheet.page_declarations, PageSize::A4_POINTS);
    let margins = page_margins_from_for_size(
        &stylesheet.page_declarations,
        PageMargins::all_points(0.0),
        size,
    );

    assert_eq!(margins.top(), 36.0);
    assert_eq!(margins.right(), 72.0);
    assert_eq!(margins.bottom(), 54.0);
    assert_eq!(margins.left(), 9.0);
}

#[tokio::test]
async fn parses_page_margin_boxes() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { @bottom-right { content: \"Page \" counter(page); background-color: black } }",
    ));

    let bottom_right = stylesheet.page_rules[0]
        .margin_boxes
        .get("bottom-right")
        .unwrap();
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
async fn parses_gcpm_footnote_page_area_separately_from_margin_boxes() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { @footnote { border-top: thin solid black; max-height: 30% } \
         @bottom-center { content: counter(page) } }",
    ));

    assert_eq!(stylesheet.page_rules[0].margin_boxes.len(), 1);
    let footnote_area = stylesheet.page_rules[0].footnote_area.as_ref().unwrap();
    assert_eq!(
        footnote_area.get("border-top").map(String::as_str),
        Some("thin solid black")
    );
    assert_eq!(
        footnote_area.get("max-height").map(String::as_str),
        Some("30%")
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

    let bottom_right = stylesheet.page_rules[0]
        .margin_boxes
        .get("bottom-right")
        .unwrap();
    let corner = stylesheet.page_rules[0]
        .margin_boxes
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
async fn page_rules_tokenize_escaped_names_and_nested_component_values() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"@p\61 ge rep\6f rt:first {
             color: red;
             @unknown { content: "ignored; } @top-left {" }
             @top-left invalid { content: "also ignored" }
             @top-\6c eft {
               content: "}; @top-left {";
               background-image: url("data:text/plain,;@top-left{}");
             }
             @footnote { max-height: 30% }
           }"#,
    ));

    assert_eq!(stylesheet.page_rules.len(), 1);
    let rule = &stylesheet.page_rules[0];
    assert_eq!(rule.selectors[0].page_type.as_deref(), Some("report"));
    assert_eq!(
        rule.declarations.get("color").map(String::as_str),
        Some("red")
    );
    let top_left = rule.margin_boxes.get("top-left").unwrap();
    assert_eq!(
        top_left.get("content").map(String::as_str),
        Some("\"}; @top-left {\"")
    );
    assert_eq!(
        top_left.get("background-image").map(String::as_str),
        Some("url(\"data:text/plain,;@top-left{}\")")
    );
    assert_eq!(
        rule.footnote_area
            .as_ref()
            .and_then(|declarations| declarations.get("max-height"))
            .map(String::as_str),
        Some("30%")
    );
}

#[tokio::test]
async fn invalid_page_selector_lists_are_not_partially_accepted() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page :first, :unknown { margin: 4in } \
         @page report :first { margin: 3in } \
         @page :nth(2n+) { margin: 2in } \
         @page { margin: 1in }",
    ));

    assert_eq!(stylesheet.page_rules.len(), 1);
    assert!(stylesheet.page_rules[0].selectors.is_empty());
    assert_eq!(
        page_margins_from(&stylesheet.page_declarations, PageMargins::all_points(0.0)),
        PageMargins::all_points(72.0)
    );
}

#[tokio::test]
async fn parses_invoice_page_margin_boxes_through_main_at_rule_parser() {
    let stylesheet = parse_stylesheet(
        &Css::from_file("weasyprint-samples/invoice/invoice.css")
            .await
            .unwrap(),
    );

    assert_eq!(stylesheet.page_rules.len(), 1);
    assert_eq!(stylesheet.page_rules[0].margin_boxes.len(), 2);
    assert_eq!(
        stylesheet.page_rules[0]
            .margin_boxes
            .get("bottom-left")
            .and_then(|declarations| declarations.get("content"))
            .map(String::as_str),
        Some("'♥ Thank you!'")
    );
    assert_eq!(
        stylesheet.page_rules[0]
            .margin_boxes
            .get("bottom-right")
            .and_then(|declarations| declarations.get("font-size"))
            .map(String::as_str),
        Some("9pt")
    );
}

#[tokio::test]
async fn parses_page_margin_box_page_selectors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@page { @top-left { content: \"base\"; color: red } }\
         @page :right { @top-left { content: \"right\" } }\
         @page :first { @top-left { content: \"first\"; color: blue } }",
    ));

    let base = stylesheet.page_rules[0]
        .margin_boxes
        .get("top-left")
        .unwrap();
    let right = stylesheet.page_rules[1]
        .margin_boxes
        .get("top-left")
        .unwrap();
    let first = stylesheet.page_rules[2]
        .margin_boxes
        .get("top-left")
        .unwrap();
    assert_eq!(base.get("content").map(String::as_str), Some("\"base\""));
    assert_eq!(right.get("content").map(String::as_str), Some("\"right\""));
    assert_eq!(first.get("color").map(String::as_str), Some("blue"));
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
async fn parses_page_margin_box_layers() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme;\
         @layer theme { @page { @top-left { content: \"theme\" } } }\
         @layer base { @page :first { @top-left { content: \"base\" } } }",
    ));

    assert_eq!(stylesheet.page_rules.len(), 2);
    assert_eq!(
        stylesheet.page_rules[0].layer_order.as_ref().unwrap().0,
        vec![1, 0]
    );
    assert_eq!(
        stylesheet.page_rules[1].layer_order.as_ref().unwrap().0,
        vec![0, 0]
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn invalid_media_query_syntax_does_not_apply_when_negated() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: green } \
         @media not and { p { color: red } } \
         @media and { p { color: red } } \
         @media not only { p { color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 128, 0));
}

#[tokio::test]
async fn media_conditions_combine_boolean_features_and_general_enclosed_syntax() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: red } \
         @media (width 500px) or (min-width: 0) { p { color: green } } \
         @media ((not (monochrome)) and (color)) { p { color: blue } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 0, 255));
}

#[test]
fn media_or_condition_uses_the_valid_range_when_a_general_enclosed_term_is_false() {
    let environment = MediaEnvironment::new(MediaType::Screen, CssViewportSize::new(800.0, 600.0));

    assert!(crate::css::media_rule_applies_in_environment(
        "(width 500px) or (min-width: 0)",
        &environment,
    ));
}

#[test]
fn forced_colors_media_feature_tracks_the_rendering_environment() {
    let inactive = MediaEnvironment::new(MediaType::Print, CssViewportSize::new(800.0, 600.0));
    let active = inactive.with_forced_colors(ForcedColorsMode::Active(ForcedColorPalette::LIGHT));

    assert!(!crate::css::media_rule_applies_in_environment(
        "(forced-colors)",
        &inactive
    ));
    assert!(crate::css::media_rule_applies_in_environment(
        "(forced-colors: none)",
        &inactive
    ));
    assert!(crate::css::media_rule_applies_in_environment(
        "(forced-colors)",
        &active
    ));
    assert!(crate::css::media_rule_applies_in_environment(
        "(forced-colors: active)",
        &active
    ));
    assert!(!crate::css::media_rule_applies_in_environment(
        "(forced-colors: invalid)",
        &active
    ));
}

#[test]
fn scripting_media_feature_reports_static_rendering_as_none() {
    let environment = MediaEnvironment::default();

    assert!(!crate::css::media_rule_applies_in_environment(
        "(scripting)",
        &environment
    ));
    assert!(crate::css::media_rule_applies_in_environment(
        "(scripting: none)",
        &environment
    ));
    assert!(!crate::css::media_rule_applies_in_environment(
        "(scripting: initial-only)",
        &environment
    ));
    assert!(!crate::css::media_rule_applies_in_environment(
        "(scripting: enabled)",
        &environment
    ));
    assert!(!crate::css::media_rule_applies_in_environment(
        "(scripting: unsupported)",
        &environment
    ));
}

#[test]
fn prefers_color_scheme_media_feature_uses_light_default_and_explicit_preference() {
    let default_environment =
        MediaEnvironment::new(MediaType::Print, CssViewportSize::new(800.0, 600.0));
    let dark_environment =
        default_environment.with_color_scheme_preference(ColorSchemePreference::Dark);

    assert!(crate::css::media_rule_applies_in_environment(
        "(prefers-color-scheme: light)",
        &default_environment,
    ));
    assert!(!crate::css::media_rule_applies_in_environment(
        "(prefers-color-scheme: dark)",
        &default_environment,
    ));
    assert!(crate::css::media_rule_applies_in_environment(
        "(prefers-color-scheme: dark)",
        &dark_environment,
    ));
    assert!(!crate::css::media_rule_applies_in_environment(
        "(prefers-color-scheme: light)",
        &dark_environment,
    ));
}

#[test]
fn print_media_dimensions_match_the_renderer_viewport() {
    let environment = MediaEnvironment::new(MediaType::Print, CssViewportSize::new(480.0, 288.0));

    assert!(crate::css::media_rule_applies_in_environment(
        "(min-width: 4in) and (max-width: 5in) and (min-height: 2in) and (max-height: 3in)",
        &environment,
    ));
}

#[tokio::test]
async fn matching_print_media_rule_overrides_earlier_background_shorthand() {
    let environment = MediaEnvironment::new(MediaType::Print, CssViewportSize::new(480.0, 288.0));
    let stylesheet = parse_stylesheet_with_media_environment(
        &Css::from_string(
            "body { background: red; } \
             /* An explanatory comment; its semicolon is not a rule delimiter. */ \
             @media (min-width: 4in) and (max-width: 5in) and \
                    (min-height: 2in) and (max-height: 3in) { \
               body { background: green; } \
             }",
        ),
        &environment,
    );
    let style = style_for_element_with_signature(
        ElementSignature::new("body", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(0, 128, 0))
    );
}

#[test]
fn inherited_relative_currentcolor_background_resolves_against_child_color() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div { background-color: rgb(from currentcolor r g b); color: red } \
         div div { color: green; background-color: inherit }",
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );

    assert_eq!(
        parent.background.background_color,
        BackgroundColor::RelativeCurrentColor {
            expression: "rgb(from currentcolor r g b)".to_string(),
            used_color_scheme: parent.used_color_scheme,
        }
    );
    assert_eq!(
        parent.background.background_color,
        BackgroundColor::RelativeCurrentColor {
            expression: "rgb(from currentcolor r g b)".to_string(),
            used_color_scheme: parent.used_color_scheme,
        }
    );
    assert_eq!(child.color, CssColor::new(0, 128, 0));
    assert_eq!(
        crate::css::parse_color_from_currentcolor("rgb(from currentcolor r g b)", child.color),
        Some(CssColor::new(0, 128, 0))
    );
    assert_eq!(
        child.background.background_color,
        BackgroundColor::RelativeCurrentColor {
            expression: "rgb(from currentcolor r g b)".to_string(),
            used_color_scheme: child.used_color_scheme,
        }
    );
    assert_eq!(
        child.background.background_color.visible_color(child.color),
        Some(CssColor::new(0, 128, 0))
    );
}

#[tokio::test]
async fn screen_media_environment_applies_a_valid_or_condition() {
    let environment = MediaEnvironment::new(MediaType::Screen, CssViewportSize::new(800.0, 600.0));
    let stylesheet = parse_stylesheet_with_media_environment(
        &Css::from_string(
            "p { color: red } @media (width 500px) or (min-width: 0) { p { color: green } }",
        ),
        &environment,
    );
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 128, 0));
}

#[test]
fn empty_media_query_list_applies_to_all_media() {
    assert!(crate::css::media_rule_applies_in_environment(
        "",
        &MediaEnvironment::default(),
    ));
}

#[tokio::test]
async fn invalid_known_media_feature_values_do_not_become_true_through_not() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: green } \
         @media (color-gamut: dci-p3), not (color-gamut: rec-2020) { p { color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 128, 0));
}

#[tokio::test]
async fn inherited_background_origin_preserves_the_child_image_layers() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "#parent { background-origin: content-box } \
         #child { background-image: url(image.png); background-origin: inherit }",
    ));
    let parent_signature =
        ElementSignature::new("div", HashMap::from([("id".into(), "parent".into())]));
    let parent = style_for_element_with_signature(
        parent_signature.clone(),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::from([("id".into(), "child".into())])),
        None,
        &[stylesheet],
        Some(&parent),
        &[parent_signature],
    );

    assert_eq!(child.background.background_origin, BackgroundBox::Content);
    assert!(child.background.background_image.is_image());
    assert!(child.background.background_layers[0].image.is_image());
    assert_eq!(
        child.background.background_layers[0].origin,
        BackgroundBox::Content
    );
}

#[tokio::test]
async fn inherited_background_clip_applies_to_a_color_only_child() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "#parent { background-clip: content-box } \
         #child { background-clip: inherit; background-color: red }",
    ));
    let parent_signature =
        ElementSignature::new("div", HashMap::from([("id".into(), "parent".into())]));
    let parent = style_for_element_with_signature(
        parent_signature.clone(),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::from([("id".into(), "child".into())])),
        None,
        &[stylesheet],
        Some(&parent),
        &[parent_signature],
    );

    assert_eq!(parent.background.background_clip, BackgroundBox::Content);
    assert_eq!(child.background.background_clip, BackgroundBox::Content);
    assert_eq!(child.background_color_clip(), BackgroundBox::Content);
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

    assert_eq!(ltr.color, CssColor::new(0, 0, 255));
    assert_eq!(rtl.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn selector_list_cascades_with_the_specificity_of_its_matching_branch() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".start, .slr .start:dir(rtl) { color: red } .h .start { color: blue }",
    ));
    let table = ElementSignature::new("table", HashMap::from([("class".into(), "h".into())]));
    let style = style_for_element_with_signature(
        ElementSignature::new("td", HashMap::from([("class".into(), "start".into())])),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[table],
    );

    assert_eq!(style.color, CssColor::new(0, 0, 255));
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
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

    assert_eq!(auto.color, CssColor::new(255, 0, 0));
    assert_eq!(bdi.color, CssColor::new(0, 0, 255));
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

    assert_eq!(
        implicit_ltr.background.background_color.color(),
        Some(CssColor::new(0, 255, 0))
    );
    assert_eq!(
        explicit_ltr.background.background_color.color(),
        Some(CssColor::new(0, 255, 0))
    );
    assert_eq!(
        ancestor_ltr.background.background_color.color(),
        Some(CssColor::new(0, 255, 0))
    );
    assert_eq!(
        explicit_rtl.background.background_color.color(),
        Some(CssColor::new(0, 255, 0))
    );
    assert_eq!(
        ancestor_rtl.background.background_color.color(),
        Some(CssColor::new(0, 255, 0))
    );
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

    assert_eq!(style.color, CssColor::new(0, 255, 0));
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
        language: ContentLanguage::from_html_attribute("fr"),
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

    assert_eq!(english.color, CssColor::new(0, 0, 255));
    assert_eq!(inherited.color, CssColor::new(255, 0, 0));
    assert!(parent.language.shares_tag_storage_with(&inherited.language));
}

#[tokio::test]
async fn unrecognized_html_language_tags_match_only_their_own_lang_ranges() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: black } p:lang(xyzzy) { color: green } p:lang(abcde) { color: red }",
    ));
    let direct = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "XYZzy".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let parent = ComputedStyle {
        language: ContentLanguage::from_html_attribute("xyzzy"),
        ..ComputedStyle::initial()
    };
    let inherited = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[],
    );

    assert_eq!(direct.color, CssColor::new(0, 128, 0));
    assert_eq!(direct.language.as_deref(), None);
    assert_eq!(inherited.color, CssColor::new(0, 128, 0));
    assert_eq!(inherited.language.as_deref(), None);
    assert!(parent.language.shares_tag_storage_with(&inherited.language));
}

#[tokio::test]
async fn malformed_html_language_tags_do_not_match_lang_or_drive_typography() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: black } p:lang(ja) { color: red }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("lang".to_string(), "ja_Hang".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::BLACK);
    assert_eq!(style.language.as_deref(), None);
    let ContentLanguage::Tagged(tag) = style.language else {
        panic!("a malformed but present HTML lang value remains tagged")
    };
    assert_eq!(tag.as_str(), "ja_Hang");
}

#[tokio::test]
async fn inherited_malformed_html_language_tags_remain_selector_and_typography_ineligible() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: black } p:lang(ja) { color: red }",
    ));
    let parent = ComputedStyle {
        language: ContentLanguage::from_html_attribute("ja_Hang"),
        ..ComputedStyle::initial()
    };
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[],
    );

    assert_eq!(style.color, CssColor::BLACK);
    assert_eq!(style.language.as_deref(), None);
    assert!(parent.language.shares_tag_storage_with(&style.language));
    let ContentLanguage::Tagged(tag) = style.language else {
        panic!("an inherited malformed HTML lang value remains tagged")
    };
    assert_eq!(tag.as_str(), "ja_Hang");
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

    assert_eq!(regional.color, CssColor::new(255, 0, 0));
    assert_eq!(regional.border_colors.top, CssColor::new(0, 128, 0));
    assert_eq!(
        italian.background.background_color.color(),
        Some(CssColor::new(0, 0, 255))
    );
    assert_eq!(unknown.border_colors.right, CssColor::new(128, 0, 128));
    assert_eq!(
        wildcard_subtag.border_colors.bottom,
        CssColor::new(0, 128, 0)
    );
    assert_eq!(
        singleton_extension
            .border_colors
            .bottom
            .resolve(singleton_extension.color),
        CssColor::BLACK
    );
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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

    assert_eq!(
        first_filtered.background.background_color.color(),
        Some(CssColor::new(0, 255, 0))
    );
    assert_eq!(
        second_filtered.background.background_color.color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(
        third_filtered.background.background_color.color(),
        Some(CssColor::new(0, 255, 0))
    );
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

    assert_eq!(first_filtered.color, CssColor::new(0, 255, 0));
    assert_eq!(second_filtered.color, CssColor::BLACK);
    assert_eq!(third_filtered.color, CssColor::new(0, 255, 0));
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

    assert_eq!(style.color, CssColor::new(0, 255, 0));
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

    assert_eq!(style.color, CssColor::new(0, 255, 0));
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

    assert_eq!(style.color, CssColor::BLACK);
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
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

    assert_eq!(style.color, CssColor::new(0, 255, 0));
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

    assert_eq!(empty.color, CssColor::new(0, 255, 0));
    assert_eq!(text_only.color, CssColor::BLACK);
    assert_eq!(
        with_child_and_next.background.background_color.color(),
        Some(CssColor::new(0, 0, 255))
    );
    assert_eq!(
        with_child_and_next.border_colors.top,
        CssColor::new(255, 0, 0)
    );
}

#[tokio::test]
async fn static_link_history_pseudo_classes_keep_links_unvisited() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "a { color: black } \
         a:link { color: lime } \
         a:visited { color: red } \
         a:any-link { border-top-color: blue } \
         .link-parent:has(:link) { color: lime } \
         .visited-parent:has(:visited) { color: red } \
         .any-link-parent:has(:any-link) { color: blue }",
    ));
    let hyperlink = ElementSignature::new(
        "a",
        HashMap::from([("href".to_string(), "destination".to_string())]),
    );
    let non_hyperlink = ElementSignature::new("a", HashMap::new());

    let link_style = style_for_element_with_signature(
        hyperlink,
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let non_link_style = style_for_element_with_signature(
        non_hyperlink,
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let link_parent = |class: &str| {
        style_for_element_with_signature(
            ElementSignature::new(
                "div",
                HashMap::from([("class".to_string(), class.to_string())]),
            )
            .with_children(
                vec![ElementSiblingSignature::new(
                    "a",
                    HashMap::from([("href".to_string(), "destination".to_string())]),
                )],
                false,
            ),
            None,
            std::slice::from_ref(&stylesheet),
            None,
            &[],
        )
    };

    assert_eq!(link_style.color, CssColor::new(0, 255, 0));
    assert_eq!(link_style.border_colors.top, CssColor::new(0, 0, 255));
    assert_eq!(non_link_style.color, CssColor::BLACK);
    assert_ne!(non_link_style.border_colors.top, CssColor::new(0, 0, 255));
    assert_eq!(link_parent("link-parent").color, CssColor::new(0, 255, 0));
    assert_eq!(link_parent("visited-parent").color, CssColor::BLACK);
    assert_eq!(
        link_parent("any-link-parent").color,
        CssColor::new(0, 0, 255)
    );
}

#[tokio::test]
async fn visited_link_colors_use_actual_state_without_exposing_layout_state() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "a:link { color: rgb(255 0 0 / .5); display: block; opacity: .75; column-rule-color: rgb(255 0 0 / .5), rgb(0 128 0 / .25) } \
         a:visited { color: blue; display: none; opacity: .1; column-rule-color: blue, yellow } \
         .parent { background-color: red } \
         .parent:has(:visited) { background-color: blue } \
         a:visited .descendant { border-top-color: blue }",
    ));
    let visited_link =
        ElementSignature::new("a", HashMap::from([("href".to_string(), "".to_string())]))
            .with_link_state(LinkState::Visited);

    let style = style_for_element_with_signature(
        visited_link.clone(),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    assert_eq!(style.color, CssColor::rgba(0, 0, 255, 0.5));
    assert_eq!(style.display, Display::BLOCK);
    assert_eq!(style.opacity, 0.75);
    assert_eq!(
        style.column_rule.colors.value_for_index(0, 2),
        Some(CssColor::rgba(255, 0, 0, 0.5))
    );
    assert_eq!(
        style
            .column_rule
            .visited_colors
            .as_ref()
            .and_then(|colors| colors.value_for_index(0, 2)),
        Some(CssColor::new(0, 0, 255))
    );

    let parent = ElementSignature::new(
        "div",
        HashMap::from([("class".to_string(), "parent".to_string())]),
    )
    .with_children(
        vec![
            ElementSiblingSignature::new(
                "a",
                HashMap::from([("href".to_string(), "".to_string())]),
            )
            .with_link_state(LinkState::Visited),
        ],
        false,
    );
    let parent_rule = stylesheet
        .rules
        .iter()
        .find(|rule| rule.selector_text.contains(":has(:visited)"))
        .expect("fixture contains the visited relational selector");
    assert!(
        crate::css::selector::selector_matches_with_scope_proximity_in_chain_with_link_matching(
            &parent_rule.selector,
            &parent_rule.scopes,
            parent_rule.stylesheet_scope_anchor,
            &crate::css::selector::selector_chain(&parent, &[]),
            0,
            &mut selectors::context::SelectorCaches::default(),
            crate::css::selector::LinkMatching::Actual,
        )
        .is_some()
    );
    let parent_style = style_for_element_with_signature(
        parent,
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    assert_eq!(
        parent_style.background.background_color.color(),
        Some(CssColor::new(0, 0, 255))
    );

    let descendant_style = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([("class".to_string(), "descendant".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent_style),
        std::slice::from_ref(&visited_link),
    );
    assert_eq!(descendant_style.border_colors.top, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn sibling_signatures_keep_child_snapshots_for_ancestor_has_rules() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".container:has(> .a) { border-top-color: red }\
         .container:has(> .a) .b { color: lime }\
         .container:has(> .a) .b .c { background-color: blue }\
         .container:has(> .a) .b::after { content: 'x' }",
    ));
    let c = ElementSiblingSignature::new(
        "span",
        HashMap::from([("class".to_string(), "c".to_string())]),
    );
    let b = ElementSiblingSignature::new(
        "span",
        HashMap::from([("class".to_string(), "b".to_string())]),
    )
    .with_children(vec![c.clone()], false);
    let a = ElementSiblingSignature::new(
        "span",
        HashMap::from([("class".to_string(), "a".to_string())]),
    );

    for children in [vec![b.clone(), a.clone()], vec![a, b]] {
        let container_siblings = vec![
            ElementSiblingSignature::new(
                "div",
                HashMap::from([("class".to_string(), "container".to_string())]),
            )
            .with_children(children.clone(), false),
            ElementSiblingSignature::new("div", HashMap::new()),
        ];
        let container = ElementSignature::with_siblings(
            "div",
            HashMap::from([("class".to_string(), "container".to_string())]),
            0,
            container_siblings,
        );
        assert_eq!(container.children.len(), 2);
        let container_style = style_for_element_with_signature(
            container.clone(),
            None,
            std::slice::from_ref(&stylesheet),
            None,
            &[],
        );
        assert_eq!(container_style.border_colors.top, CssColor::new(255, 0, 0));
        let b_index = children
            .iter()
            .position(|child| child.attrs.get("class").is_some_and(|class| class == "b"))
            .expect("fixture contains the descendant subject");
        let b_signature = ElementSignature::with_siblings(
            "span",
            HashMap::from([("class".to_string(), "b".to_string())]),
            b_index,
            children,
        );
        let b_style = style_for_element_with_signature(
            b_signature.clone(),
            None,
            std::slice::from_ref(&stylesheet),
            None,
            std::slice::from_ref(&container),
        );
        assert_eq!(b_style.color, CssColor::new(0, 255, 0));
        assert!(b_style.after_style.is_some());

        let c_style = style_for_element_with_signature(
            ElementSignature::with_siblings(
                "span",
                HashMap::from([("class".to_string(), "c".to_string())]),
                0,
                vec![c.clone()],
            ),
            None,
            std::slice::from_ref(&stylesheet),
            Some(&b_style),
            &[container, b_signature],
        );
        assert_eq!(
            c_style.background.background_color.color(),
            Some(CssColor::new(0, 0, 255))
        );
    }
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

    assert_eq!(checked_required.color, CssColor::new(0, 255, 0));
    assert_eq!(
        checked_required.background.background_color.color(),
        Some(CssColor::new(0, 0, 255))
    );
    assert_eq!(
        checked_required.border_colors.right,
        CssColor::new(0, 255, 0)
    );
    assert_eq!(
        checked_required.border_colors.bottom,
        CssColor::new(0, 0, 255)
    );
    assert_eq!(checked_required.outline_color, CssColor::new(255, 0, 0));
    assert_eq!(disabled.border_colors.top, CssColor::new(255, 0, 0));
    assert_eq!(
        disabled.border_colors.right.resolve(disabled.color),
        CssColor::BLACK
    );
    assert_eq!(readonly.outline_color, CssColor::new(255, 0, 0));
    assert_eq!(writable.outline_color, CssColor::new(0, 0, 255));
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

    assert_eq!(open_details.color, CssColor::new(0, 255, 0));
    assert_eq!(open_details.margin.left, 60.0);
    assert_eq!(closed_details.color, CssColor::new(255, 0, 0));
    assert_eq!(closed_details.margin.left, 0.0);
    assert_eq!(
        open_div.border_colors.top.resolve(open_div.color),
        CssColor::BLACK
    );
    assert_eq!(
        section.background.background_color.color(),
        Some(CssColor::new(0, 0, 255))
    );
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

    assert_eq!(
        container.background.background_color.color(),
        Some(CssColor::new(0, 0, 255))
    );
    assert_eq!(container.color, CssColor::BLACK);
    assert_eq!(target.color, CssColor::new(0, 255, 0));
    assert_eq!(
        target.background.background_color.color(),
        Some(CssColor::new(0, 0, 255))
    );
}

#[tokio::test]
async fn namespace_selectors_match_namespaced_type_and_attribute_signatures() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@namespace html \"http://www.w3.org/1999/xhtml\";\
         @namespace svg \"http://www.w3.org/2000/svg\";\
         @namespace xlink \"http://www.w3.org/1999/xlink\";\
         html|p { color: lime }\
         svg|use[xlink|href] { color: blue }\
         svg|use[*|href] { background-color: red }\
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

    assert_eq!(html_style.color, CssColor::new(0, 255, 0));
    assert_eq!(use_style.color, CssColor::new(0, 0, 255));
    assert_eq!(
        use_style.background.background_color.color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(
        wrong_namespace_style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
}

#[tokio::test]
async fn parser_first_pseudo_routing_preserves_html_attribute_selector_branches() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "[data-kind=lead], p::before { color: red; content: \"generated\" }\
         [|data-kind=lead] { border-top-color: lime }\
         [dir=RTL i] { background-color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([
                ("data-kind".to_string(), "lead".to_string()),
                ("dir".to_string(), "RTL".to_string()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(stylesheet.rules.len(), 3);
    assert_eq!(stylesheet.before_rules.len(), 1);
    assert_eq!(style.color, CssColor::new(255, 0, 0));
    assert_eq!(style.border_colors.top, CssColor::new(0, 255, 0));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(0, 0, 255))
    );
    assert!(style.before_style.is_some());
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

    assert_eq!(style.color, CssColor::BLACK);
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

    assert_eq!(style.color, CssColor::new(0, 255, 0));
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

    assert_eq!(style.color, CssColor::new(0, 255, 0));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
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
        .with_child_list(first_legend.children.clone(), false);
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

    assert_eq!(disabled_by_fieldset.color, CssColor::new(255, 0, 0));
    assert_eq!(
        enabled_inside_first_legend.border_colors.top,
        CssColor::new(0, 255, 0)
    );
    assert_eq!(disabled_option.color, CssColor::new(255, 0, 0));
    assert_eq!(
        invalid_required.border_colors.right,
        CssColor::new(0, 0, 255)
    );
    assert_eq!(
        invalid_required.border_colors.bottom,
        CssColor::new(255, 0, 0)
    );
    assert_eq!(valid_in_range.outline_color, CssColor::new(0, 255, 0));
    assert_eq!(
        valid_in_range.background.background_color.color(),
        Some(CssColor::new(0, 255, 0))
    );
    assert!(checked_default.text_decoration.underline);
    assert_eq!(unchecked.border_colors.left, CssColor::new(0, 0, 255));
    assert_eq!(invalid_email.border_colors.bottom, CssColor::new(255, 0, 0));
    assert_eq!(invalid_url.border_colors.bottom, CssColor::new(255, 0, 0));
    assert_eq!(
        invalid_length.border_colors.bottom,
        CssColor::new(255, 0, 0)
    );
    assert_eq!(invalid_step.border_colors.bottom, CssColor::new(255, 0, 0));
    assert_eq!(
        fallback_selected_option.outline_color,
        CssColor::new(0, 0, 255)
    );
}

#[tokio::test]
async fn typographic_pseudo_element_rules_create_computed_style_slots() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p::first-line { color: red; display: none }\
         p::first-letter { color: blue; display: none; margin-left: 10px; initial-letter: 2 }",
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
        CssColor::new(255, 0, 0)
    );
    assert_eq!(
        style.first_letter_style.as_ref().unwrap().color,
        CssColor::new(0, 0, 255)
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
    assert_eq!(
        style.first_letter_style.as_ref().unwrap().initial_letter,
        InitialLetter::Specified { size: 2.0, sink: 2 }
    );
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

    assert_eq!(before.color, CssColor::new(255, 0, 0));
    assert_eq!(before.font_size, 18.0);
    assert_eq!(
        before
            .custom_properties
            .get("--accent")
            .map(ComputedCustomPropertyValue::substitution_tokens),
        Some("blue".to_string())
    );
    assert_eq!(before.margin.left, 0.0);
    assert_eq!(
        before.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
    assert_eq!(before.position, Position::Static);
}

#[tokio::test]
async fn transform_style_and_backface_visibility_cascade_without_inheriting() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "body { transform-style: preserve-3d; backface-visibility: hidden }",
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
        &[stylesheet],
        Some(&parent),
        &[ElementSignature::new("body", HashMap::new())],
    );
    assert_eq!(parent.transform_style, TransformStyle::Preserve3d);
    assert_eq!(parent.backface_visibility, BackfaceVisibility::Hidden);
    assert_eq!(child.transform_style, TransformStyle::Flat);
    assert_eq!(child.backface_visibility, BackfaceVisibility::Visible);

    let important_author = parse_stylesheet(&Css::from_string(
        "div { transform-style: flat !important; backface-visibility: visible !important }",
    ));
    let normal_inline = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("transform-style: preserve-3d; backface-visibility: hidden"),
        std::slice::from_ref(&important_author),
        None,
        &[],
    );
    assert_eq!(normal_inline.transform_style, TransformStyle::Flat);
    assert_eq!(
        normal_inline.backface_visibility,
        BackfaceVisibility::Visible
    );

    let important_inline = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("transform-style: preserve-3d !important; backface-visibility: hidden !important"),
        &[important_author],
        None,
        &[],
    );
    assert_eq!(important_inline.transform_style, TransformStyle::Preserve3d);
    assert_eq!(
        important_inline.backface_visibility,
        BackfaceVisibility::Hidden
    );
}

#[tokio::test]
async fn generated_pseudos_are_suppressed_on_html_replaced_form_controls() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"input::before, textarea::after, div::before { content: "probe" }"#,
    ));

    for tag in ["input", "textarea"] {
        let style = style_for_element_with_signature(
            ElementSignature::new(tag, HashMap::new()),
            None,
            std::slice::from_ref(&stylesheet),
            None,
            &[],
        );
        assert!(
            style.before_style.is_none() && style.after_style.is_none(),
            "HTML {tag} controls suppress generated content pseudo-elements"
        );
    }

    let ordinary_element = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    assert!(ordinary_element.before_style.is_some());
}

#[tokio::test]
async fn gcpm_footnote_pseudos_have_counter_defaults_and_author_overrides() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"aside { float: footnote; color: red; margin-left: 20pt }
           aside::footnote-call { content: "[" counter(footnote) "]"; color: blue }
           aside::footnote-marker { content: "* "; color: green }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("aside", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    let call = style
        .footnote_call_style
        .as_deref()
        .expect("footnote call style");
    let marker = style
        .footnote_marker_style
        .as_deref()
        .expect("footnote marker style");

    assert_eq!(call.color, CssColor::new(0, 0, 255));
    assert_eq!(marker.color, CssColor::new(0, 128, 0));
    assert_eq!(call.float, Float::None);
    assert_eq!(marker.float, Float::None);
    assert_eq!(call.margin.left, 0.0);
    assert_eq!(marker.margin.left, 0.0);
    assert_eq!(
        marker.content,
        Content::List {
            parts: vec![GeneratedContentPart::Text("* ".to_string())],
            alt: None,
        }
    );
}

#[tokio::test]
async fn nested_generated_pseudo_declarations_do_not_style_the_originating_element() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"#skills {
             section#target::before {
               content: "";
               float: left;
               height: 2cm;
               width: 2cm;
             }
           }"#,
    ));
    let mut attrs = HashMap::new();
    attrs.insert("id".to_string(), "target".to_string());
    let style = style_for_element_with_signature(
        ElementSignature::new("section", attrs),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new("article", {
            let mut attrs = HashMap::new();
            attrs.insert("id".to_string(), "skills".to_string());
            attrs
        })],
    );
    let before = style.before_style.as_ref().expect("before style");

    assert_eq!(style.float, Float::None);
    assert_eq!(style.content, Content::Normal);
    assert_eq!(before.float, Float::Left);
    assert!(!before.box_values.width.is_auto());
}

#[tokio::test]
async fn marker_style_inherits_without_cloning_non_inherited_properties() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"li {
             display: list-item;
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

    assert_eq!(marker.color, CssColor::new(255, 0, 0));
    assert_eq!(marker.font_size, 18.0);
    assert_eq!(marker.margin.left, 0.0);
    assert_eq!(
        marker.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
    assert_eq!(marker.position, Position::Static);
}

#[tokio::test]
async fn marker_all_property_expands_before_color_and_font_size_prepasses() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"li { display: list-item }
           li::marker { all: initial; color: green; font-size: 18pt; content: "x" }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("li", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    let marker = style.marker_style.as_ref().expect("marker style");

    assert_eq!(marker.color, CssColor::new(0, 128, 0));
    assert_eq!(marker.font_size, 18.0);
}

#[tokio::test]
async fn marker_text_properties_use_the_regular_text_cascade_without_accepting_layout() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"li { display: list-item; writing-mode: vertical-rl }
           li::marker {
             direction: rtl;
             unicode-bidi: plaintext;
             writing-mode: horizontal-tb;
             text-orientation: upright;
             text-combine-upright: all;
             letter-spacing: 2pt;
             word-spacing: 3pt;
             tab-size: 4;
             word-break: break-all;
             overflow-wrap: anywhere;
             line-break: anywhere;
             hyphens: none;
             text-decoration: underline blue;
             text-emphasis: filled dot red;
             text-shadow: 1pt 2pt green;
           }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("li", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    let marker = style.marker_style.as_deref().expect("li has marker style");

    assert_eq!(marker.direction, Direction::Rtl);
    assert_eq!(marker.unicode_bidi, UnicodeBidi::Plaintext);
    assert_eq!(marker.writing_mode, WritingMode::VerticalRl);
    assert_eq!(marker.text_orientation, TextOrientation::Upright);
    assert_eq!(marker.text_combine_upright, TextCombineUpright::All);
    assert_eq!(
        marker.letter_spacing,
        ComputedLengthPercentage::from_points(2.0)
    );
    assert_eq!(
        marker.word_spacing,
        ComputedLengthPercentage::from_points(3.0)
    );
    assert_eq!(marker.tab_size, TabSize::Spaces(4.0));
    assert_eq!(marker.word_break, WordBreak::BreakAll);
    assert_eq!(marker.overflow_wrap, OverflowWrap::Anywhere);
    assert_eq!(marker.line_break, LineBreak::Anywhere);
    assert_eq!(marker.hyphens, Hyphens::None);
    assert!(marker.text_decoration.underline);
    assert_eq!(
        marker.text_decoration.color,
        CssColorOrCurrentColor::Color(CssColor::new(0, 0, 255))
    );
    assert_eq!(
        marker
            .text_emphasis_style
            .mark_for_writing_mode(marker.writing_mode),
        Some("\u{2022}")
    );
    assert_eq!(
        marker.text_emphasis_color,
        CssColorOrCurrentColor::Color(CssColor::new(255, 0, 0))
    );
    assert_eq!(marker.text_shadow.len(), 1);
}

#[tokio::test]
async fn marker_all_unset_allows_later_supported_text_properties_to_win() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"li { display: list-item; writing-mode: vertical-rl }
           li::marker {
             all: unset;
             text-orientation: upright;
             text-combine-upright: all;
           }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("li", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    let marker = style.marker_style.as_deref().expect("li has marker style");

    assert_eq!(marker.text_orientation, TextOrientation::Upright);
    assert_eq!(marker.text_combine_upright, TextCombineUpright::All);
}

#[tokio::test]
async fn marker_text_orientation_cascades_through_a_descendant_selector() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "li { display: list-item } .test ol li::marker { text-orientation: upright }",
    ));
    let mut list_style = ComputedStyle::initial();
    list_style.writing_mode = WritingMode::VerticalRl;
    let style = style_for_element_with_signature(
        ElementSignature::new("li", HashMap::new()),
        None,
        &[stylesheet],
        Some(&list_style),
        &[
            ElementSignature::new("html", HashMap::new()),
            ElementSignature::new("body", HashMap::new()),
            ElementSignature::new(
                "figure",
                HashMap::from([("class".to_string(), "test".to_string())]),
            ),
            ElementSignature::new("ol", HashMap::new()),
        ],
    );
    let marker = style.marker_style.as_deref().expect("li has marker style");

    assert_eq!(marker.text_orientation, TextOrientation::Upright);
}

#[test]
fn anonymous_block_style_inherits_only_inherited_properties() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"div {
             --accent: blue;
             color: red;
             direction: rtl;
             writing-mode: vertical-rl;
             font-size: 18pt;
             white-space: pre-wrap;
             text-decoration: underline;
             margin: 20pt;
             padding: 10pt;
             border: 3pt solid green;
             background-color: blue;
             width: 40pt;
             position: relative;
             float: left;
           }
           div::before { content: "before" }
           div::after { content: "after" }
           div::first-line { color: blue }
           div::first-letter { color: green }"#,
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    let anonymous = anonymous_block_style(&parent);

    assert_eq!(anonymous.display, Display::BLOCK);
    assert_eq!(anonymous.color, parent.color);
    assert_eq!(anonymous.direction, parent.direction);
    assert_eq!(anonymous.writing_mode, parent.writing_mode);
    assert_eq!(anonymous.font_size, parent.font_size);
    assert_eq!(anonymous.white_space, parent.white_space);
    // Text-decoration origins are not inherited. Decorations propagate over
    // in-flow descendants during layout instead, including across anonymous
    // blocks (CSS Text Decoration Level 3 §2).
    assert!(!anonymous.text_decoration_origins.has_effective_layers());
    assert_eq!(
        anonymous
            .custom_properties
            .get("--accent")
            .map(ComputedCustomPropertyValue::substitution_tokens),
        Some("blue".to_string())
    );

    assert_eq!(anonymous.margin, Edges::ZERO);
    assert_eq!(anonymous.padding, Edges::ZERO);
    assert_eq!(anonymous.border_widths, Edges::ZERO);
    assert_eq!(
        anonymous.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
    assert!(anonymous.box_values.width.is_auto());
    assert_eq!(anonymous.position, Position::Static);
    assert_eq!(anonymous.float, Float::None);
    assert!(anonymous.before_style.is_none());
    assert!(anonymous.after_style.is_none());
    assert!(anonymous.first_line_style.is_none());
    assert!(anonymous.first_letter_style.is_none());
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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
    assert!(style.break_before.avoids_page());
    assert!(style.break_before.avoids_column());
    assert_eq!(style.break_after, PageBreak::AvoidPage);
    assert!(style.break_after.avoids_page());
    assert!(!style.break_after.avoids_column());
}

#[tokio::test]
async fn parses_column_break_values_without_page_forcing() {
    let declarations = parse_declarations(
        "page-break-before: column; break-before: column; break-after: avoid-column; break-inside: avoid-column",
    );
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.break_before, PageBreak::Column);
    assert_eq!(style.break_after, PageBreak::AvoidColumn);
    assert!(!style.break_before.is_forced());
    assert!(!style.break_after.avoids_page());
    assert!(style.break_after.avoids_column());
    assert_eq!(style.break_inside, BreakInsideAvoidance::AvoidColumn);
}

#[tokio::test]
async fn legacy_page_break_properties_reject_column_values() {
    let declarations = parse_declarations(
        "page-break-before: column; page-break-after: avoid-column; page-break-inside: avoid-column",
    );
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.break_before, PageBreak::Auto);
    assert_eq!(style.break_after, PageBreak::Auto);
    assert_eq!(style.break_inside, BreakInsideAvoidance::Auto);
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
        style.background.background_image.as_image(),
        Some(BackgroundImage::Url(url)) if url.href == "images/report ) cover.png"
    ));
    assert_eq!(list_style_image_url(&style), Some("markers/a)b.png"));

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

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(gradient.direction, LinearGradientDirection::Angle(180.0));
    assert!(!gradient.repeating);
    assert_eq!(gradient.stops.len(), 4);
    assert_eq!(
        gradient.stops[0].color.as_color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(
        gradient.stops[0].position.clone().unwrap().length_points(),
        0.0
    );
    assert_eq!(
        gradient.stops[1].position.clone().unwrap().length_points(),
        37.5
    );
    assert_eq!(
        gradient.stops[2].color.as_color(),
        Some(CssColor::new(0, 128, 0))
    );
    assert_eq!(
        gradient.stops[2].position.clone().unwrap().length_points(),
        37.5
    );
    assert_eq!(
        gradient.stops[3].position.clone().unwrap().length_points(),
        75.0
    );
}

#[tokio::test]
async fn parses_color_image_background_image() {
    let declarations = parse_declarations("background-image: image(green)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert!(
        matches!(style.background.background_image.as_image(), Some(BackgroundImage::ImageFunction(ImageFunction { source: None, fallback_color: Some(ColorImageColor::CssColor(color)), directionality: None })) if *color == CssColor::new(0, 128, 0))
    );
}

#[test]
fn image_function_parsing_preserves_source_fallback_and_directionality() {
    let source_only = parse_css_image(r#"image("green.png")"#, None, None);
    assert!(matches!(
        source_only,
        ParsedImage::Image(ComputedImage::Image(image))
            if matches!(*image, BackgroundImage::ImageFunction(ImageFunction {
                source: Some(ImageUrl { ref href, .. }),
                fallback_color: None,
                directionality: None,
            }) if href == "green.png")
    ));

    let source_and_fallback =
        parse_css_image("image(rtl url(green.png), currentcolor)", None, None);
    assert!(matches!(
        source_and_fallback,
        ParsedImage::Image(ComputedImage::Image(image))
            if matches!(*image, BackgroundImage::ImageFunction(ImageFunction {
                source: Some(ImageUrl { ref href, .. }),
                fallback_color: Some(ColorImageColor::CurrentColor),
                directionality: Some(ImageDirectionality::Rtl),
            }) if href == "green.png")
    ));

    let directional = parse_css_image("image(rtl url(green.png), blue)", None, None);
    assert!(matches!(
        directional,
        ParsedImage::Image(ComputedImage::Image(image))
            if matches!(*image, BackgroundImage::ImageFunction(ImageFunction {
                source: Some(ImageUrl { ref href, .. }),
                fallback_color: Some(ColorImageColor::CssColor(color)),
                directionality: Some(ImageDirectionality::Rtl),
            }) if href == "green.png" && color == CssColor::new(0, 0, 255))
    ));
}

#[test]
fn image_function_rejects_duplicate_or_misordered_components() {
    for value in [
        "image()",
        "image(url(one.png),)",
        "image(red, blue)",
        "image(url(one.png), url(two.png))",
        "image(ltr, blue)",
        "image(ltr red)",
        "image(url(one.png) blue)",
        "image(url(one.png), blue, red)",
    ] {
        assert!(
            matches!(parse_css_image(value, None, None), ParsedImage::SyntaxError),
            "expected invalid image() grammar: {value}"
        );
    }
}

#[test]
fn light_dark_image_parsing_preserves_typed_branches_and_rejects_nonimages() {
    let ParsedImage::Image(ComputedImage::Image(image)) = parse_css_image(
        "light-dark(url(light.png), linear-gradient(red, blue))",
        None,
        None,
    ) else {
        panic!("expected a typed light-dark() image");
    };
    assert!(matches!(
        *image,
        BackgroundImage::LightDark(LightDarkImage { light, dark })
            if matches!(*light, BackgroundImage::Url(ref url) if url.href == "light.png")
                && matches!(*dark, BackgroundImage::LinearGradient(_))
    ));

    let ParsedImage::Image(ComputedImage::Image(image)) =
        parse_css_image("light-dark(none, url(dark.png))", None, None)
    else {
        panic!("expected a typed light-dark() image");
    };
    assert!(matches!(
        *image,
        BackgroundImage::LightDark(LightDarkImage { light, dark })
            if matches!(*light, BackgroundImage::CssColor(ColorImageColor::CssColor(color)) if color == CssColor::TRANSPARENT)
                && matches!(*dark, BackgroundImage::Url(ref url) if url.href == "dark.png")
    ));

    for value in [
        "light-dark(url(one.png))",
        "light-dark(url(one.png),)",
        "light-dark(url(one.png), blue)",
    ] {
        assert!(matches!(
            parse_css_image(value, None, None),
            ParsedImage::SyntaxError
        ));
    }
    assert!(matches!(
        parse_css_image("light-dark(red, blue)", None, None),
        ParsedImage::NotAnImage
    ));

    let declarations = parse_declarations("background: light-dark(red, blue)");
    let mut style = default_style_for_tag("div");
    style.used_color_scheme = UsedColorScheme::Dark;
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style
            .background
            .background_color
            .resolved_color(style.color),
        CssColor::new(0, 0, 255)
    );
}

#[test]
fn light_dark_image_selection_precedes_image_set_selection() {
    let ParsedImage::Image(mut image) = parse_css_image(
        "light-dark(\
            image-set(url(light-one.png) 1x, url(light-two.png) 2x),\
            image-set(url(dark-one.png) 1x, url(dark-two.png) 2x))",
        None,
        None,
    ) else {
        panic!("expected a light-dark() image");
    };
    image.resolve_for_context(ImageSelectionContext {
        used_color_scheme: UsedColorScheme::Dark,
        resolution_dppx: 1.5,
    });
    assert!(matches!(
        image.as_image(),
        Some(BackgroundImage::SelectedImageSet { image, resolution })
            if *resolution == 2.0
                && matches!(**image, BackgroundImage::Url(ref url) if url.href == "dark-two.png")
    ));

    assert!(matches!(
        parse_css_image(
            "image-set(light-dark(image-set(url(nested.png) 1x), url(dark.png)) 1x)",
            None,
            None,
        ),
        ParsedImage::SyntaxError
    ));
}

#[tokio::test]
async fn light_dark_images_resolve_for_all_typed_image_consumers() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div {\
             color-scheme: dark;\
             background-image: light-dark(url(background-light.png), url(background-dark.png));\
             border-image-source: light-dark(url(border-light.png), url(border-dark.png));\
             mask-border-source: light-dark(url(mask-light.png), url(mask-dark.png));\
             list-style-image: light-dark(url(marker-light.png), url(marker-dark.png));\
             content: light-dark(url(content-light.png), url(content-dark.png));\
             string-set: label light-dark(url(string-light.png), url(string-dark.png));\
             shape-outside: light-dark(url(shape-light.png), url(shape-dark.png));\
           }\
           span {\
             background-image: light-dark(url(inherited-light.png), url(inherited-dark.png));\
           }",
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
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
        &[ElementSignature::new("div", HashMap::new())],
    );
    fn selected_url(image: &ComputedImage) -> Option<&str> {
        match image.as_image()?.selected_image() {
            BackgroundImage::Url(url) => Some(url.href.as_str()),
            _ => None,
        }
    }

    assert_eq!(parent.used_color_scheme, UsedColorScheme::Dark);
    assert_eq!(
        selected_url(&parent.background.background_image),
        Some("background-dark.png")
    );
    assert_eq!(
        selected_url(&parent.border_image.source),
        Some("border-dark.png")
    );
    assert_eq!(
        selected_url(&parent.mask_border_source),
        Some("mask-dark.png")
    );
    assert_eq!(
        selected_url(&parent.list_style_image),
        Some("marker-dark.png")
    );
    let Content::Replacement { image, .. } = &parent.content else {
        panic!("expected image replacement content");
    };
    let GeneratedContentPart::Image { image } = image else {
        panic!("expected generated image content");
    };
    assert_eq!(selected_url(image), Some("content-dark.png"));
    let NamedStringPart::Image(image) = &parent.string_sets[0].parts[0] else {
        panic!("expected named-string image content");
    };
    assert_eq!(selected_url(image), Some("string-dark.png"));
    assert!(matches!(
        parent.shape_outside,
        ShapeOutside::Image(BackgroundImage::Url(ref url)) if url.href == "shape-dark.png"
    ));
    assert_eq!(
        selected_url(&child.background.background_image),
        Some("inherited-dark.png")
    );
}

#[tokio::test]
async fn image_set_keeps_the_selected_candidate_resolution() {
    let declarations = parse_declarations("background-image: image-set(url(green.png) 0.5x)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::ImageSet(set))
            if set.options.len() == 1
                && set.options[0].resolution_dppx == 0.5
                && matches!(*set.options[0].image, BackgroundImage::Url(ref url) if url.href == "green.png")
    ));
}

#[tokio::test]
async fn invalid_image_set_does_not_replace_the_cascaded_background_image() {
    let declarations = parse_declarations(
        "background-image: url(green.png); background-image: image-set(url(red.png) -1x)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::Url(url)) if url.href == "green.png"
    ));
}

#[tokio::test]
async fn calculated_invalid_image_set_candidate_computes_to_no_image() {
    let declarations = parse_declarations(
        "background-image: url(red.png); background-image: image-set(url(red.png) calc(-1 * 1x))",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    style.background.background_image.select_image_set(1.0);

    assert_eq!(style.background.background_image, ComputedImage::Invalid);
}

#[tokio::test]
async fn zero_resolution_image_set_candidate_computes_to_no_image() {
    let declarations = parse_declarations(
        "background-image: url(red.png); background-image: image-set(url(red.png) 0x)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    style.background.background_image.select_image_set(1.0);

    assert_eq!(style.background.background_image, ComputedImage::Invalid);
}

#[tokio::test]
async fn unknown_image_set_type_computes_to_an_invalid_image() {
    let declarations = parse_declarations(
        "background-image: url(red.png); background-image: image-set(url(green.png) type(\"image/unknown\"))",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    style.background.background_image.select_image_set(1.0);

    assert_eq!(style.background.background_image, ComputedImage::Invalid);
}

#[tokio::test]
async fn malformed_image_set_type_does_not_replace_the_cascaded_image() {
    let declarations = parse_declarations(
        "background-image: url(green.png); background-image: image-set(url(red.png) type(image/png))",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::Url(url)) if url.href == "green.png"
    ));
}

#[tokio::test]
async fn supported_image_set_type_selects_its_candidate() {
    let declarations =
        parse_declarations("background-image: image-set(url(green.png) 1x type(\"image/png\"))");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::ImageSet(set))
            if set.options.len() == 1
                && set.options[0].resolution_dppx == 1.0
                && matches!(*set.options[0].image, BackgroundImage::Url(ref url) if url.href == "green.png")
    ));
}

#[tokio::test]
async fn image_set_selection_uses_the_rendering_density_after_mime_filtering() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div { background-image: -webkit-image-set(\
            url(unsupported.png) 2x type(\"image/unsupported\"), \
            url(one.png) 1x, url(two.png) 2x, url(later-two.png) 2x); }",
    ));
    let stylesheets = Stylesheets::document_only(std::slice::from_ref(&stylesheet))
        .with_image_set_resolution_dppx(1.5);
    let style = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        &stylesheets,
        None,
        &[],
    );
    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::SelectedImageSet { image, resolution })
            if *resolution == 2.0
                && matches!(**image, BackgroundImage::Url(ref url) if url.href == "two.png")
    ));
}

#[tokio::test]
async fn image_set_calc_accepts_dimension_aware_sum_and_product() {
    let declarations = parse_declarations(
        "background-image: image-set(url(green.png) calc((96dpi + 1dppx) / 2));",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::ImageSet(set))
            if set.options[0].resolution_dppx == 1.0
    ));
}

#[tokio::test]
async fn image_set_uses_component_value_aliases_and_escaped_string_sources() {
    let declarations = parse_declarations(
        r#"background-image: -webkit-image-set("green\2epng" type("image/png") 1x);"#,
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::ImageSet(set))
            if matches!(*set.options[0].image, BackgroundImage::Url(ref url) if url.href == "green.png")
                && set.options[0].mime_type.as_deref() == Some("image/png")
    ));
}

#[tokio::test]
async fn image_set_resolution_uses_shared_math_and_calculated_range_clamping() {
    let declarations = parse_declarations(
        "background-image: image-set(\
             url(one.png) min(2x, 3x), \
             url(two.png) clamp(0x, calc(-1 * 1x), 2x), \
             url(three.png) calc(1dppx * sign(2)));",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::ImageSet(set))
            if set.options.iter().map(|option| option.resolution_dppx).collect::<Vec<_>>() == [2.0, 0.0, 1.0]
    ));
}

#[tokio::test]
async fn malformed_image_set_mime_parameters_are_filtered_not_syntax_errors() {
    let declarations = parse_declarations(
        "background-image: image-set(url(bad.png) 1x type(\"image/png; charset\"), url(good.png) 1x type(\"image/png\"));",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    style.background.background_image.select_image_set(1.0);

    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::SelectedImageSet { image, .. })
            if matches!(**image, BackgroundImage::Url(ref url) if url.href == "good.png")
    ));
}

#[tokio::test]
async fn css_color_4_gradient_stops_retain_their_computed_color_spaces() {
    let declarations = parse_declarations(
        "background-image: \
         linear-gradient(color(display-p3 1.2 -.1 .3), color(rec2020 1.1 .2 .3)), \
         radial-gradient(lab(50% 120 -110), oklab(.7 .3 -.2)), \
         conic-gradient(color(display-p3 .8 .2 .1), color(display-p3 .1 .2 .8))",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(linear)) =
        style.background.background_layers[0].image.as_image()
    else {
        panic!("expected linear gradient");
    };
    let first = linear.stops[0]
        .color
        .as_color()
        .expect("concrete color stop");
    let second = linear.stops[1]
        .color
        .as_color()
        .expect("concrete color stop");
    assert_eq!(first.space(), CssColorSpace::DisplayP3);
    assert!(first.components()[0] > 1.0);
    assert_eq!(second.space(), CssColorSpace::Rec2020);

    let Some(BackgroundImage::RadialGradient(radial)) =
        style.background.background_layers[1].image.as_image()
    else {
        panic!("expected radial gradient");
    };
    assert!(radial.stops.iter().all(|stop| {
        stop.color
            .as_color()
            .is_some_and(|color| color.space() == CssColorSpace::XyzD50)
    }));

    let Some(BackgroundImage::ConicGradient(conic)) =
        style.background.background_layers[2].image.as_image()
    else {
        panic!("expected conic gradient");
    };
    assert!(conic.stops.iter().all(|stop| {
        stop.color
            .as_color()
            .is_some_and(|color| color.space() == CssColorSpace::DisplayP3)
    }));
}

#[tokio::test]
async fn parses_angle_and_corner_linear_gradient_directions() {
    let declarations = parse_declarations(
        "background-image: linear-gradient(.5turn, red, blue), linear-gradient(to top right, red, blue)",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(first)) =
        style.background.background_layers[0].image.as_image()
    else {
        panic!("expected first linear gradient background image");
    };
    assert_eq!(first.direction, LinearGradientDirection::Angle(180.0));
    assert_eq!(first.stops[0].position, None);
    assert_eq!(first.stops[1].position, None);

    let Some(BackgroundImage::LinearGradient(second)) =
        style.background.background_layers[1].image.as_image()
    else {
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
async fn parses_css_images_4_gradient_interpolation_preludes() {
    let declarations = parse_declarations(
        "background-image: \
         linear-gradient(to right in srgb, red, lime), \
         repeating-radial-gradient(circle at center in display-p3, red, blue), \
         conic-gradient(in oklch longer hue from .25turn at 25% 75%, red, blue)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(linear)) =
        style.background.background_layers[0].image.as_image()
    else {
        panic!("expected linear gradient");
    };
    assert_eq!(linear.interpolation.space, GradientInterpolationSpace::Srgb);

    let Some(BackgroundImage::RadialGradient(radial)) =
        style.background.background_layers[1].image.as_image()
    else {
        panic!("expected radial gradient");
    };
    assert_eq!(
        radial.interpolation.space,
        GradientInterpolationSpace::DisplayP3
    );

    let Some(BackgroundImage::ConicGradient(conic)) =
        style.background.background_layers[2].image.as_image()
    else {
        panic!("expected conic gradient");
    };
    assert_eq!(conic.interpolation.space, GradientInterpolationSpace::Oklch);
    assert_eq!(conic.interpolation.hue, HueInterpolationMethod::Longer);
}

#[tokio::test]
async fn unqualified_gradients_use_css_images_3_srgb_interpolation() {
    let declarations = parse_declarations(
        "background-image: linear-gradient(red, blue), radial-gradient(circle, red, blue), conic-gradient(red, blue)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    for image in style
        .background
        .background_layers
        .iter()
        .map(|layer| &layer.image)
    {
        let interpolation = match image.as_image() {
            Some(BackgroundImage::LinearGradient(gradient)) => gradient.interpolation,
            Some(BackgroundImage::RadialGradient(gradient)) => gradient.interpolation,
            Some(BackgroundImage::ConicGradient(gradient)) => gradient.interpolation,
            _ => panic!("expected gradient image"),
        };
        assert_eq!(interpolation, GradientInterpolationMethod::CSS_IMAGES_3);
    }
}

#[tokio::test]
async fn rejects_invalid_gradient_interpolation_preludes() {
    for value in [
        "linear-gradient(in not-a-space, red, blue)",
        "linear-gradient(in srgb longer hue, red, blue)",
        "radial-gradient(in lch hue, red, blue)",
        "conic-gradient(in hsl sideways hue, red, blue)",
    ] {
        let declarations = parse_declarations(&format!("background-image: {value}"));
        let mut style = default_style_for_tag("div");
        apply_declarations(&mut style, &declarations);
        assert!(style.background.background_image.is_none(), "{value}");
    }
}

#[tokio::test]
async fn gradient_stops_retain_missing_components_and_currentcolor() {
    let declarations = parse_declarations(
        "background-image: linear-gradient(in srgb, color(srgb none .5 none), currentcolor)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient");
    };
    assert_eq!(
        gradient.stops[0]
            .color
            .missing_components_for(GradientInterpolationMethod {
                space: GradientInterpolationSpace::Srgb,
                hue: HueInterpolationMethod::Shorter,
            })
            .bits(),
        0b0101
    );
    assert!(gradient.stops[1].color.is_current_color());
    let resolved = gradient.resolve_current_color(CssColor::new(1, 2, 3));
    assert_eq!(
        resolved.stops[1].color.as_color(),
        Some(CssColor::new(1, 2, 3))
    );
}

#[tokio::test]
async fn background_shorthand_preserves_modern_hsl_missing_components() {
    let declarations = parse_declarations(
        "background: linear-gradient(90deg in srgb, hsl(none none 50%), yellow)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient");
    };
    assert_eq!(
        gradient.interpolation.space,
        GradientInterpolationSpace::Srgb
    );
    assert_eq!(
        gradient.stops[0]
            .color
            .missing_components_for(gradient.interpolation)
            .bits(),
        0
    );
    let GradientColor::ColorWithMissing {
        missing,
        source: GradientMissingComponentSpace::Hsl,
        ..
    } = gradient.stops[0].color
    else {
        panic!("expected HSL missing-component metadata");
    };
    assert_eq!(missing.bits(), 0b0011);
}

#[tokio::test]
async fn rgb_missing_components_do_not_become_oklab_components() {
    let declarations = parse_declarations(
        "background-image: linear-gradient(in oklab, rgb(none 255 0), rgb(255 0 0))",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient");
    };
    assert!(
        gradient.stops[0]
            .color
            .missing_components_for(gradient.interpolation)
            .is_empty()
    );
}

#[tokio::test]
async fn parses_all_polar_hue_directions() {
    for (keyword, expected) in [
        ("shorter", HueInterpolationMethod::Shorter),
        ("longer", HueInterpolationMethod::Longer),
        ("increasing", HueInterpolationMethod::Increasing),
        ("decreasing", HueInterpolationMethod::Decreasing),
    ] {
        let declarations = parse_declarations(&format!(
            "background-image: linear-gradient(to right in hsl {keyword} hue, red, orange)"
        ));
        let mut style = default_style_for_tag("div");
        apply_declarations(&mut style, &declarations);
        let Some(BackgroundImage::LinearGradient(gradient)) =
            style.background.background_image.as_image()
        else {
            panic!("expected linear gradient for {keyword}");
        };
        assert_eq!(gradient.interpolation.hue, expected, "{keyword}");
    }
}

#[tokio::test]
async fn class_rule_retains_a_decreasing_hsl_gradient_method() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div.a1 { background-image: linear-gradient(to right in hsl increasing hue, red, orange) } \
         div.b1 { background-image: linear-gradient(to right in hsl decreasing hue, red, orange) }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([("class".to_string(), "b1".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[],
    );
    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected class background gradient");
    };
    assert_eq!(
        gradient.interpolation.hue,
        HueInterpolationMethod::Decreasing
    );
}

#[tokio::test]
async fn parses_repeating_linear_gradient_stops_and_hints() {
    let declarations =
        parse_declarations("background: repeating-linear-gradient(0, red, 25%, blue 50% 75%)");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(gradient.direction, LinearGradientDirection::Angle(0.0));
    assert!(gradient.repeating);
    assert_eq!(gradient.stops.len(), 3);
    assert_eq!(gradient.stops[0].position, None);
    assert_eq!(
        gradient.stops[1]
            .position
            .as_ref()
            .unwrap()
            .percentage_coefficient_or_zero(),
        0.5
    );
    assert_eq!(
        gradient.stops[2]
            .position
            .as_ref()
            .unwrap()
            .percentage_coefficient_or_zero(),
        0.75
    );
    assert_eq!(gradient.hints.len(), 1);
    assert_eq!(gradient.hints[0].after_stop, 0);
    assert_eq!(
        gradient.hints[0].position.percentage_coefficient_or_zero(),
        0.25
    );
}

#[tokio::test]
async fn parses_radial_gradient_shape_size_position_and_stops() {
    let declarations = parse_declarations(
        "background-image: radial-gradient(circle closest-side at 25% 75%, red, 30%, blue 100%)",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::RadialGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected radial gradient background image");
    };
    assert_eq!(gradient.shape, RadialGradientShape::Circle);
    assert_eq!(
        gradient.size,
        RadialGradientSize::Extent(RadialGradientExtent::ClosestSide)
    );
    assert_eq!(
        gradient.position.x.offset.percentage_coefficient_or_zero(),
        0.25
    );
    assert_eq!(
        gradient.position.y.offset.percentage_coefficient_or_zero(),
        0.75
    );
    assert!(!gradient.repeating);
    assert_eq!(gradient.stops.len(), 2);
    assert_eq!(
        gradient.stops[0].color.as_color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(
        gradient.stops[1].color.as_color(),
        Some(CssColor::new(0, 0, 255))
    );
    assert_eq!(
        gradient.stops[1]
            .position
            .as_ref()
            .unwrap()
            .percentage_coefficient_or_zero(),
        1.0
    );
    assert_eq!(gradient.hints.len(), 1);
    assert_eq!(gradient.hints[0].after_stop, 0);
    assert_eq!(
        gradient.hints[0].position.percentage_coefficient_or_zero(),
        0.3
    );
}

#[tokio::test]
async fn parses_repeating_radial_gradient_explicit_radii() {
    let declarations = parse_declarations(
        "background-image: repeating-radial-gradient(10pt 20pt at center, red 0pt, red 4pt, blue 4pt, blue 8pt)",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::RadialGradient(gradient)) =
        style.background.background_image.as_image()
    else {
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

    let Some(BackgroundImage::LinearGradient(first)) =
        style.background.background_layers[0].image.as_image()
    else {
        panic!("expected first gradient");
    };
    assert!((gradient_angle(first) - 90.0).abs() < 0.001);

    let Some(BackgroundImage::LinearGradient(second)) =
        style.background.background_layers[1].image.as_image()
    else {
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

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(gradient.stops.len(), 2);
    assert_eq!(
        gradient.stops[0].color.as_color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(
        gradient.stops[0]
            .position
            .as_ref()
            .unwrap()
            .percentage_coefficient_or_zero(),
        0.5
    );
    assert_eq!(
        gradient.stops[1].color.as_color(),
        Some(CssColor::new(0, 128, 0))
    );
    assert_eq!(
        gradient.stops[1]
            .position
            .as_ref()
            .unwrap()
            .percentage_coefficient_or_zero(),
        0.5
    );
}

#[tokio::test]
async fn ch_linear_gradient_color_stops_resolve_before_paint() {
    let declarations =
        parse_declarations("background: linear-gradient(to right, red 2ch, blue 10vw)");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(
        gradient.stops[0].position,
        Some(ComputedLengthPercentage::from_ch(2.0))
    );

    style.resolve_font_metric_lengths(layout_pt(6.0));

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(
        gradient.stops[0].position,
        Some(ComputedLengthPercentage::from_points(12.0))
    );
    let Some(BackgroundImage::LinearGradient(layer_gradient)) =
        style.background.background_layers[0].image.as_image()
    else {
        panic!("expected linear gradient background layer");
    };
    assert_eq!(
        layer_gradient.stops[0].position,
        Some(ComputedLengthPercentage::from_points(12.0))
    );

    style.resolve_viewport_lengths_for_viewport(LayoutSize::new(200.0, 100.0));

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(
        gradient.stops[1].position,
        Some(ComputedLengthPercentage::from_points(20.0))
    );
}

#[tokio::test]
async fn gradient_stops_resolve_em_against_the_element_font_size() {
    let declarations =
        parse_declarations("background: linear-gradient(to right, red 4em, blue 3rem)");
    let mut style = default_style_for_tag("div");
    style.font_size = 18.75;
    style.root_font_size = 12.0;

    apply_declarations(&mut style, &declarations);
    style.finalize_computed_font_relative_lengths();

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_image.as_image()
    else {
        panic!("expected linear gradient background image");
    };
    assert_eq!(
        gradient.stops[0].position,
        Some(ComputedLengthPercentage::from_points(75.0))
    );
    assert_eq!(
        gradient.stops[1].position,
        Some(ComputedLengthPercentage::from_points(36.0))
    );

    let Some(BackgroundImage::LinearGradient(gradient)) =
        style.background.background_layers[0].image.as_image()
    else {
        panic!("expected linear gradient background layer");
    };
    assert_eq!(
        gradient.stops[0].position,
        Some(ComputedLengthPercentage::from_points(75.0))
    );
    assert_eq!(
        gradient.stops[1].position,
        Some(ComputedLengthPercentage::from_points(36.0))
    );
}

#[tokio::test]
async fn resolves_viewport_math_against_vertical_writing_mode() {
    let declarations = parse_declarations(
        "writing-mode: vertical-rl; width: min(calc(10vw + 20vh + 30vmin), calc(40vmax + 50vi + 60vb))",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);
    style.resolve_viewport_lengths_for_viewport(LayoutSize::new(200.0, 100.0));

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            70.0
        ))
    );
}

#[tokio::test]
async fn parses_background_origin_and_clip_boxes() {
    let declarations =
        parse_declarations("background-origin: content-box; background-clip: padding-box");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.background.background_origin, BackgroundBox::Content);
    assert_eq!(style.background.background_clip, BackgroundBox::Padding);
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

    assert_eq!(style.background.background_layers.len(), 2);
    assert!(matches!(
        style.background.background_layers[0].image.as_image(),
        Some(BackgroundImage::Url(url)) if url.href == "top.png"
    ));
    assert!(matches!(
        style.background.background_layers[1].image.as_image(),
        Some(BackgroundImage::Url(url)) if url.href == "bottom.png"
    ));
    assert_eq!(
        style.background.background_layers[0].repeat,
        BackgroundRepeat::NoRepeat
    );
    assert_eq!(
        style.background.background_layers[1].repeat,
        BackgroundRepeat::RepeatY
    );
    assert_eq!(
        style.background.background_layers[0].origin,
        BackgroundBox::Content
    );
    assert_eq!(
        style.background.background_layers[0].clip,
        BackgroundBox::Padding
    );
    assert_eq!(
        style.background.background_layers[1].origin,
        BackgroundBox::Border
    );
    assert_eq!(
        style.background.background_layers[1].clip,
        BackgroundBox::Content
    );
}

#[tokio::test]
async fn background_color_uses_the_bottom_most_layer_clip() {
    let declarations = parse_declarations(
        "background-clip: border-box, content-box, border-box; background-image: none, none",
    );
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.background.background_layers.len(), 2);
    assert_eq!(style.background_color_clip(), BackgroundBox::Content);
}

#[tokio::test]
async fn background_shorthand_sets_origin_then_clip_boxes() {
    let declarations = parse_declarations("background: red content-box border-box");
    let mut style = default_style_for_tag("div");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(style.background.background_origin, BackgroundBox::Content);
    assert_eq!(style.background.background_clip, BackgroundBox::Border);
}

#[tokio::test]
async fn parses_background_repeat_aliases_and_two_axis_values() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("background-repeat: repeat-x"),
    );
    assert_eq!(
        style.background.background_repeat,
        BackgroundRepeat::RepeatX
    );

    apply_declarations(
        &mut style,
        &parse_declarations("background-repeat: repeat-y"),
    );
    assert_eq!(
        style.background.background_repeat,
        BackgroundRepeat::RepeatY
    );

    apply_declarations(
        &mut style,
        &parse_declarations("background-repeat: no-repeat repeat"),
    );
    assert_eq!(
        style.background.background_repeat,
        BackgroundRepeat::RepeatY
    );

    apply_declarations(
        &mut style,
        &parse_declarations("background: url(bg.png) repeat no-repeat"),
    );
    assert_eq!(
        style.background.background_repeat,
        BackgroundRepeat::RepeatX
    );

    apply_declarations(&mut style, &parse_declarations("background-repeat: space"));
    assert_eq!(
        style.background.background_repeat,
        BackgroundRepeat::new(BackgroundRepeatAxis::Space, BackgroundRepeatAxis::Space)
    );

    apply_declarations(
        &mut style,
        &parse_declarations("background-repeat: round no-repeat"),
    );
    assert_eq!(
        style.background.background_repeat,
        BackgroundRepeat::new(BackgroundRepeatAxis::Round, BackgroundRepeatAxis::NoRepeat)
    );
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
async fn decodes_a_zero_css_escape_to_the_replacement_character() {
    let declarations = parse_declarations(r#"content: "\0000""#);
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![GeneratedContentPart::Text("\u{fffd}".to_string())],
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
                    style: Some(ListStyleType::Named("upper-roman".to_string())),
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
                    image: ComputedImage::image(BackgroundImage::Url(ImageUrl {
                        href: "icon.png".to_string(),
                        base_url: None,
                        root_url: None,
                        request_modifiers: RequestUrlModifiers::default(),
                    })),
                },
            ],
            alt: None,
        }
    );
}

#[tokio::test]
async fn generated_content_attr_names_preserve_authored_case() {
    let declarations = parse_declarations("content: attr(definitionURL)");
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![GeneratedContentPart::Attr {
                name: "definitionURL".to_string(),
                fallback: None,
            }],
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
    let GeneratedContentPart::Image { image } = &parts[0] else {
        panic!("expected generated image");
    };
    assert!(matches!(
        image.as_image(),
        Some(BackgroundImage::LinearGradient(_))
    ));
    let GeneratedContentPart::Image { image } = &parts[1] else {
        panic!("expected generated image");
    };
    assert!(matches!(
        image.as_image(),
        Some(BackgroundImage::RadialGradient(_))
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
                    target: TargetReference::Fragment("#chapter".to_string()),
                    name: "page".to_string(),
                    style: Some(ListStyleType::Named("lower-roman".to_string())),
                },
                GeneratedContentPart::Text(" ".to_string()),
                GeneratedContentPart::TargetText {
                    target: TargetReference::Fragment("#chapter".to_string()),
                    keyword: NamedStringTargetTextKeyword::After,
                },
            ],
            alt: None,
        }
    );
}

#[tokio::test]
async fn parses_attribute_target_references_for_generated_content() {
    let declarations = parse_declarations(
        "content: target-counter(attr(href), chapter) target-text(attr(href), before)",
    );
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![
                GeneratedContentPart::TargetCounter {
                    target: TargetReference::Attribute("href".to_string()),
                    name: "chapter".to_string(),
                    style: None,
                },
                GeneratedContentPart::TargetText {
                    target: TargetReference::Attribute("href".to_string()),
                    keyword: NamedStringTargetTextKeyword::Before,
                },
            ],
            alt: None,
        }
    );
}

#[tokio::test]
async fn untyped_attr_with_invalid_fallback_preserves_available_attribute_value() {
    let mut style = ComputedStyle::initial();
    apply_declarations(&mut style, &parse_declarations(r#"content: "previous""#));
    apply_declarations(
        &mut style,
        &parse_declarations("content: attr(data-title, 1px)"),
    );

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![GeneratedContentPart::Attr {
                name: "data-title".to_string(),
                fallback: None,
            }],
            alt: None,
        }
    );
}

#[tokio::test]
async fn pseudo_content_keeps_present_attr_when_untyped_fallback_is_invalid() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"#four::after { content: attr(does-exist, invalid) }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([
                ("id".to_string(), "four".to_string()),
                ("does-exist".to_string(), "Not fallback value".to_string()),
            ]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(
        style
            .after_style
            .as_deref()
            .and_then(|after| after.content.generated_parts()),
        Some(
            [GeneratedContentPart::Attr {
                name: "does-exist".to_string(),
                fallback: None,
            }]
            .as_slice()
        )
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
                image: ComputedImage::image(BackgroundImage::Url(ImageUrl {
                    href: "icon.png".to_string(),
                    base_url: None,
                    root_url: None,
                    request_modifiers: RequestUrlModifiers::default(),
                })),
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
    assert_eq!(child.quotes.auto_quote_pair(0), ("“", "”"));
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
    assert_eq!(quoted.quotes.auto_quote_pair(0), ("“", "”"));
    assert_eq!(match_parent.quotes.auto_quote_pair(0), ("“", "”"));
    assert_eq!(auto.quotes.auto_quote_pair(0), ("「", "」"));
}

#[tokio::test]
async fn parses_css_fonts_matching_axes() {
    let declarations = parse_declarations(
        "font-weight: 350.4; font-style: oblique 12deg; font-width: 87.5%; font-stretch: expanded",
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_weight, FontWeight(350));
    assert_eq!(style.font_style, FontStyle::Oblique(12.0_f32.to_bits()));
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
           font-variant-alternates: historical-forms stylistic(alt-a) styleset(alt-b, alt-c) character-variant(cv-a) swash(sw-a) ornaments(orn-a) annotation(ann-a);
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

#[test]
fn font_language_override_preserves_case_and_font_shorthand_resets_it() {
    let mut parent = default_style_for_tag("p");
    apply_declarations(
        &mut parent,
        &parse_declarations("font-language-override: \"TRK\""),
    );
    assert_eq!(
        parent.font_language_override,
        FontLanguageOverride::OpenType(*b"TRK ")
    );
    let lower_case_tag = crate::css::values::parse_font_language_override("\"trk\"");
    assert_eq!(
        lower_case_tag,
        Some(FontLanguageOverride::OpenType(*b"trk "))
    );
    assert_ne!(lower_case_tag, Some(parent.font_language_override));

    let mut child = parent.clone();
    apply_declarations(
        &mut child,
        &parse_declarations("font-language-override: normal"),
    );
    assert_eq!(child.font_language_override, FontLanguageOverride::Normal);

    apply_declarations(
        &mut parent,
        &parse_declarations("font-language-override: \"DEU\"; font: 16px serif"),
    );
    assert_eq!(parent.font_language_override, FontLanguageOverride::Normal);

    assert!(crate::css::values::parse_font_language_override("\"DEU\"").is_some());
    assert!(crate::css::values::parse_font_language_override("\"TOO-LONG\"").is_none());
    assert!(crate::css::values::parse_font_language_override("TRK").is_none());
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
        "li { display: list-item; font-size-adjust: 0.8 }
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
async fn font_shorthand_resets_modeled_reset_only_longhands_and_allows_later_overrides() {
    let declarations = parse_declarations(
        "font-feature-settings: \"liga\" off; font-variation-settings: \"wght\" 700; \
         font-kerning: none; font: italic 12px Ahem; font-weight: 900",
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.font_feature_settings, FontFeatureSettings::NORMAL);
    assert_eq!(style.font_variation_settings, FontVariationSettings::NORMAL);
    assert_eq!(style.font_kerning, FontKerning::Auto);
    assert_eq!(style.font_style, FontStyle::Italic);
    assert_eq!(style.font_weight, FontWeight(900));
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
async fn font_feature_value_aliases_preserve_case_and_decode_css_escapes() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"
        @font-feature-values Font\62  {
            @styleset {
                altW: 4;
                AlTw: 5;
            }
        }
        "#,
    ));

    let values = &stylesheet.font_feature_values;
    assert_eq!(
        values
            .get("fontb", FontFeatureValuesBlock::Styleset, "altW")
            .map(|value| value.feature_index),
        Some(4)
    );
    assert_eq!(
        values
            .get("FontB", FontFeatureValuesBlock::Styleset, "AlTw")
            .map(|value| value.feature_index),
        Some(5)
    );
    assert!(
        values
            .get("FontB", FontFeatureValuesBlock::Styleset, "ALTW")
            .is_none()
    );
}

#[tokio::test]
async fn quoted_empty_font_family_is_retained_for_font_face_resources() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"
        @font-face { font-family: ""; src: url(palette.ttf); }
        @font-palette-values --palette { font-family: ""; base-palette: 1; }
        "#,
    ));

    assert_eq!(stylesheet.font_faces[0].family, "");
    assert_eq!(
        stylesheet
            .font_palette_values
            .get("--palette")
            .expect("palette definition")[0]
            .families,
        [""]
    );
}

#[tokio::test]
async fn parses_font_face_metric_override_descriptors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"
        @font-face {
            font-family: Metrics;
            src: url(metrics.ttf);
            ascent-override: 100%;
            descent-override: 25%;
            line-gap-override: 0%;
        }
        "#,
    ));
    let face = &stylesheet.font_faces[0];
    assert_eq!(face.ascent_override.map(f32::from_bits), Some(1.0));
    assert_eq!(face.descent_override.map(f32::from_bits), Some(0.25));
    assert_eq!(face.line_gap_override.map(f32::from_bits), Some(0.0));
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
           li { display: list-item }
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
        "word-break: break-all; overflow: hidden; overflow-x: clip; overflow-y: overlay; overflow-wrap: anywhere; word-wrap: break-word; line-break: anywhere; hyphens: none; hyphenate-character: \"\\00a0\\0640\"; hyphenate-limit-chars: auto 3 4",
    );
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.word_break, WordBreak::BreakAll);
    assert_eq!(style.overflow, Overflow::Hidden);
    assert_eq!(style.overflow_x, Overflow::Hidden);
    assert_eq!(style.overflow_y, Overflow::Auto);
    assert_eq!(style.overflow_wrap, OverflowWrap::BreakWord);
    assert_eq!(style.line_break, LineBreak::Anywhere);
    assert_eq!(style.hyphens, Hyphens::None);
    assert_eq!(
        style.hyphenate_character,
        HyphenateCharacter::String("\u{a0}\u{640}".to_string())
    );
    assert_eq!(
        style.hyphenate_limit_chars,
        HyphenateLimitChars {
            total: HyphenateLimitChars::AUTO_TOTAL,
            before: 3,
            after: 4,
        }
    );
}

#[test]
fn parses_css_scroll_snap_longhands_shorthands_and_logical_edges() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "scroll-snap-type: block mandatory; scroll-snap-align: start end; \
             scroll-snap-stop: always; scroll-padding: auto 10px 20%; \
             scroll-padding-inline-start: 5px; scroll-margin: 1px 2px 3px 4px",
        ),
    );

    assert_eq!(
        style.scroll_snap_type,
        ScrollSnapType::Block(ScrollSnapStrictness::Mandatory)
    );
    assert_eq!(
        style.scroll_snap_align,
        ScrollSnapAlign {
            block: ScrollSnapAlignment::Start,
            inline: ScrollSnapAlignment::End,
        }
    );
    assert_eq!(style.scroll_snap_stop, ScrollSnapStop::Always);
    assert_eq!(style.direction, Direction::Ltr);
    assert_eq!(style.scroll_padding.top, ScrollPadding::Auto);
    assert_eq!(
        style.scroll_padding.right,
        ScrollPadding::LengthPercentage(ComputedLengthPercentage::from_points(7.5))
    );
    assert_eq!(
        style.scroll_padding.bottom,
        ScrollPadding::LengthPercentage(ComputedLengthPercentage::from_percent(0.2))
    );
    assert_eq!(
        style.scroll_padding.left,
        ScrollPadding::LengthPercentage(ComputedLengthPercentage::from_points(3.75))
    );
    assert_eq!(
        style.scroll_margin.top,
        ComputedLengthPercentage::from_points(0.75)
    );
    assert_eq!(
        style.scroll_margin.right,
        ComputedLengthPercentage::from_points(1.5)
    );
    assert_eq!(
        style.scroll_margin.bottom,
        ComputedLengthPercentage::from_points(2.25)
    );
    assert_eq!(
        style.scroll_margin.left,
        ComputedLengthPercentage::from_points(3.0)
    );
}

#[test]
fn parses_css_scroll_marker_properties() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("scroll-target-group: auto; scroll-marker-group: after tabs"),
    );

    assert_eq!(style.scroll_target_group, ScrollTargetGroup::Auto);
    assert_eq!(
        style.scroll_marker_group,
        Some(ScrollMarkerGroup {
            placement: ScrollMarkerGroupPlacement::After,
            mode: ScrollMarkerGroupMode::Tabs,
        })
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
        TextTransform::Keywords(TextTransformKeywords::new(None, true, false).unwrap())
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-transform: full-size-kana"),
    );
    assert_eq!(
        style.text_transform,
        TextTransform::Keywords(TextTransformKeywords::new(None, false, true).unwrap())
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-transform: uppercase full-width full-size-kana"),
    );
    assert_eq!(
        style.text_transform,
        TextTransform::Keywords(
            TextTransformKeywords::new(Some(TextTransformCase::Uppercase), true, true).unwrap()
        )
    );
}

#[tokio::test]
async fn parses_css_text_transform_math_auto() {
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &parse_declarations("text-transform: math-auto"));

    assert_eq!(style.text_transform, TextTransform::MathAuto);

    apply_declarations(
        &mut style,
        &parse_declarations("text-transform: math-auto uppercase"),
    );
    assert_eq!(style.text_transform, TextTransform::MathAuto);
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

    assert_eq!(style.word_break, WordBreak::BreakWord);
    assert_eq!(style.overflow_wrap, OverflowWrap::Normal);
}

#[tokio::test]
async fn parses_css_text_level_four_word_break_manual() {
    let declarations = parse_declarations("word-break: manual");
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.word_break, WordBreak::Manual);
}

#[tokio::test]
async fn parses_css_text_level_four_word_break_auto_phrase() {
    let declarations = parse_declarations("word-break: auto-phrase");
    let mut style = default_style_for_tag("p");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.word_break, WordBreak::AutoPhrase);
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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_points(1.0),
            ComputedLengthPercentage::from_ch(2.0),
        ))
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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::sum(
            ComputedLengthPercentage::sum(
                ComputedLengthPercentage::sum(
                    ComputedLengthPercentage::sum(
                        ComputedLengthPercentage::from_vh(50.0),
                        ComputedLengthPercentage::from_vmin(-2.0)
                    ),
                    ComputedLengthPercentage::from_vmax(1.0),
                ),
                ComputedLengthPercentage::from_vi(3.0),
            ),
            ComputedLengthPercentage::from_vb(-4.0),
        ))
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
async fn inherited_vertical_writing_mode_maps_inline_size_to_height() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".vertical { writing-mode: vertical-lr } input { inline-size: 0 }",
    ));
    let parent_signature =
        ElementSignature::new("div", HashMap::from([("class".into(), "vertical".into())]));
    let parent = style_for_element_with_signature(
        parent_signature.clone(),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("input", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[parent_signature],
    );

    assert_eq!(child.writing_mode, WritingMode::VerticalLr);
    assert_eq!(
        child.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            0.0
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
async fn parses_percentage_letter_spacing() {
    let declarations = parse_declarations("font-size: 20pt; letter-spacing: 10%");
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.used_letter_spacing().points(), 2.0);
}

#[tokio::test]
async fn parses_css_text_level_five_text_fit() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("text-fit: consistent grow 150%"),
    );
    assert_eq!(
        style.text_fit,
        TextFit::Fit {
            direction: TextFitDirection::Grow,
            strategy: TextFitStrategy::Consistent,
            limit: Some(1.5),
        }
    );

    apply_declarations(
        &mut style,
        &parse_declarations("text-fit: shrink per-line-all 75%"),
    );
    assert_eq!(
        style.text_fit,
        TextFit::Fit {
            direction: TextFitDirection::Shrink,
            strategy: TextFitStrategy::PerLineAll,
            limit: Some(0.75),
        }
    );

    apply_declarations(&mut style, &parse_declarations("text-fit: grow shrink"));
    assert_eq!(
        style.text_fit,
        TextFit::Fit {
            direction: TextFitDirection::Shrink,
            strategy: TextFitStrategy::PerLineAll,
            limit: Some(0.75),
        }
    );

    let parent = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        Some("text-fit: shrink 75%"),
        &[],
        None,
        &[],
    );
    let inherited = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        &[],
        Some(&parent),
        &[ElementSignature::new("p", HashMap::new())],
    );
    assert_eq!(inherited.text_fit, parent.text_fit);
    let reset = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("text-fit: initial"),
        &[],
        Some(&parent),
        &[ElementSignature::new("p", HashMap::new())],
    );
    assert_eq!(reset.text_fit, TextFit::None);
    let explicit_inherit = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("text-fit: inherit"),
        &[],
        Some(&parent),
        &[ElementSignature::new("p", HashMap::new())],
    );
    assert_eq!(explicit_inherit.text_fit, parent.text_fit);
    let unset = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("text-fit: unset"),
        &[],
        Some(&parent),
        &[ElementSignature::new("p", HashMap::new())],
    );
    assert_eq!(unset.text_fit, parent.text_fit);
}

#[tokio::test]
async fn supports_rule_recognizes_valid_text_fit_grammar() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (text-fit: grow per-line 125%) { p { color: blue } }\
         @supports (text-fit: grow shrink) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
}

#[tokio::test]
async fn parses_ch_length_word_spacing() {
    let declarations = parse_declarations("font-size: 20pt; word-spacing: 2ch");
    let mut style = default_style_for_tag("span");

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.word_spacing, ComputedLengthPercentage::from_ch(2.0));
}

#[tokio::test]
async fn parses_percentage_word_spacing() {
    let declarations = parse_declarations("font-size: 20pt; word-spacing: 100%");
    let mut style = default_style_for_tag("span");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.word_spacing,
        ComputedLengthPercentage::from_percent(1.0)
    );
}

#[tokio::test]
async fn parses_mixed_percentage_word_spacing_calc() {
    let declarations = parse_declarations("font-size: 20pt; word-spacing: calc(0.5em + 50%)");
    let mut style = default_style_for_tag("span");

    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.word_spacing,
        ComputedLengthPercentage::from_affine(layout_pt(10.0), 0.5, true)
    );
}

#[tokio::test]
async fn inherited_percentage_word_spacing_resolves_against_current_font_size() {
    let parent_declarations = parse_declarations("font-size: 2pt; word-spacing: 100%");
    let mut parent = default_style_for_tag("div");
    apply_declarations(&mut parent, &parent_declarations);

    let child_declarations = parse_declarations("font-size: 20pt");
    let mut child = default_style_for_tag("span");
    child.word_spacing = parent.word_spacing;
    apply_declarations(&mut child, &child_declarations);

    assert_eq!(child.used_word_spacing(), layout_pt(20.0));
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
        TableCellVerticalAlignKeyword::Middle
    );

    let declarations = parse_declarations("vertical-align: calc(3pt + 25%)");
    apply_declarations(&mut style, &declarations);

    let BaselineShift::LengthPercentage(ref value) = style.vertical_align.baseline_shift else {
        panic!("vertical-align length/percentage should parse as a typed shift");
    };
    assert!((value.length_points() - 3.0).abs() < 0.001);
    assert!((value.percentage_coefficient_or_zero() - 0.25).abs() < 0.001);

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
    let BaselineShift::LengthPercentage(ref value) = style.vertical_align.baseline_shift else {
        panic!("baseline-shift percentage should parse as a typed shift");
    };
    assert!((value.percentage_coefficient_or_zero() - 0.1).abs() < 0.001);

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
        TableCellVerticalAlignKeyword::Top
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

    style.resolve_font_metric_lengths(layout_pt(6.0));

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

    style.resolve_font_metric_lengths(layout_pt(5.0));

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

    assert_eq!(
        style.bookmark_level,
        BookmarkLevel::Level(NonZeroU32::new(3).unwrap())
    );
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

    assert_eq!(style.bookmark_level, BookmarkLevel::None);
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
    assert_eq!(style.border_color, CssColor::new(255, 0, 0));
    assert_eq!(style.border_colors.top, CssColor::new(255, 0, 0));
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(0, 0, 255))
    );
}

#[tokio::test]
async fn parses_multicolumn_declarations() {
    let declarations = parse_declarations("columns: 4 2em; column-gap: normal");
    let mut style = default_style_for_tag("dl");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.column_count,
        ColumnCount::Count(NonZeroUsize::new(4).unwrap())
    );
    assert_eq!(
        style.column_width,
        ComputedColumnWidth::Length(ComputedLengthPercentage::from_points(24.0))
    );
    assert_eq!(style.column_gap, ComputedGap::Normal);
    assert_eq!(style.row_gap, ComputedGap::Normal);
}

#[tokio::test]
async fn parses_column_fill_span_and_containment() {
    let declarations = parse_declarations(
        "column-fill: balance-all; column-span: all; contain: paint size layout",
    );
    let mut style = ComputedStyle::initial();

    apply_declarations(&mut style, &declarations);

    assert_eq!(style.column_fill, ColumnFill::BalanceAll);
    assert_eq!(style.column_span, ColumnSpan::All);
    assert!(style.contain.size);
    assert!(style.contain.layout);
    assert!(!style.contain.style);
    assert!(style.contain.paint);
}

#[tokio::test]
async fn containment_aliases_compute_to_canonical_effects() {
    let mut strict = ComputedStyle::initial();
    apply_declarations(&mut strict, &parse_declarations("contain: strict"));
    assert!(strict.contain.size);
    assert!(strict.contain.layout);
    assert!(strict.contain.paint);
    assert!(strict.contain.style);

    let mut content = ComputedStyle::initial();
    apply_declarations(&mut content, &parse_declarations("contain: content"));
    assert!(!content.contain.size);
    assert!(content.contain.layout);
    assert!(content.contain.paint);
    assert!(content.contain.style);

    let mut style = ComputedStyle::initial();
    apply_declarations(&mut style, &parse_declarations("contain: style"));
    assert_eq!(
        style.contain,
        Contain {
            size: false,
            layout: false,
            style: true,
            paint: false,
            inline_size: false,
        }
    );

    let mut none = ComputedStyle::initial();
    apply_declarations(&mut none, &parse_declarations("contain: none"));
    assert_eq!(none.contain, Contain::NONE);
}

#[tokio::test]
async fn parses_inline_size_containment_and_content_visibility() {
    let mut style = ComputedStyle::initial();
    apply_declarations(
        &mut style,
        &parse_declarations("contain: inline-size layout; content-visibility: hidden"),
    );
    assert!(style.contain.inline_size);
    assert!(style.contain.layout);
    assert!(!style.contain.size);
    assert_eq!(style.content_visibility, ContentVisibility::Hidden);

    apply_declarations(&mut style, &parse_declarations("content-visibility: auto"));
    assert_eq!(style.content_visibility, ContentVisibility::Auto);
}

#[tokio::test]
async fn parses_subgrid_track_lists_for_containment_used_value_resolution() {
    let mut style = ComputedStyle::initial();
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: subgrid [start] [end]; grid-template-rows: subgrid",
        ),
    );

    assert!(matches!(
        style.grid_template_columns,
        GridTrackList::Subgrid { ref line_names }
            if line_names.components
                == vec![
                    SubgridLineNameComponent::LineNames(vec!["start".to_string()]),
                    SubgridLineNameComponent::LineNames(vec!["end".to_string()]),
                ]
    ));
    assert!(matches!(
        style.grid_template_rows,
        GridTrackList::Subgrid { .. }
    ));

    apply_declarations(&mut style, &parse_declarations("grid: subgrid"));
    assert!(matches!(
        style.grid_template_columns,
        GridTrackList::Subgrid { .. }
    ));
    assert!(matches!(
        style.grid_template_rows,
        GridTrackList::Subgrid { .. }
    ));

    style.grid_template_columns.resolve_contained_subgrid();
    style.grid_template_rows.resolve_contained_subgrid();
    assert_eq!(style.grid_template_columns, GridTrackList::None);
    assert_eq!(style.grid_template_rows, GridTrackList::None);
}

#[tokio::test]
async fn parses_and_expands_subgrid_name_repeats_against_the_used_span() {
    let mut style = ComputedStyle::initial();
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-rows: subgrid [x] repeat(auto-fill, [y]) repeat(100, [z])",
        ),
    );
    let GridTrackList::Subgrid { line_names } = &style.grid_template_rows else {
        panic!("expected subgrid line-name list");
    };
    assert_eq!(
        line_names.expand_to_line_count(5),
        vec![
            vec!["x".to_string()],
            vec!["z".to_string()],
            vec!["z".to_string()],
            vec!["z".to_string()],
            vec!["z".to_string()],
        ]
    );

    apply_declarations(
        &mut style,
        &parse_declarations("grid-template-columns: subgrid [] repeat(2, [a] []) [b]"),
    );
    let GridTrackList::Subgrid { line_names } = &style.grid_template_columns else {
        panic!("expected subgrid line-name list with empty slots");
    };
    assert_eq!(
        line_names.expand_to_line_count(6),
        vec![
            Vec::<String>::new(),
            vec!["a".to_string()],
            Vec::new(),
            vec!["a".to_string()],
            Vec::new(),
            vec!["b".to_string()],
        ]
    );
}

#[tokio::test]
async fn rejects_invalid_subgrid_name_repeats() {
    for value in [
        "subgrid repeat(auto-fit, [a])",
        "subgrid repeat(auto-fill, [a]) repeat(auto-fill, [b])",
        "subgrid repeat(auto-fill, 1px)",
        "subgrid repeat(0, [a])",
    ] {
        let mut style = ComputedStyle::initial();
        apply_declarations(
            &mut style,
            &parse_declarations(&format!("grid-template-rows: {value}")),
        );
        assert_eq!(style.grid_template_rows, GridTrackList::None, "{value}");
    }
}

#[tokio::test]
async fn parses_container_longhands_and_container_units_use_viewport_fallback() {
    let mut style = ComputedStyle::initial();
    apply_declarations(
        &mut style,
        &parse_declarations(
            "container: card sidebar / inline-size; width: calc(10cqw + 5cqh); height: 1cqmin",
        ),
    );

    assert_eq!(style.container_type, ContainerType::InlineSize);
    assert_eq!(style.container_names.0, ["card", "sidebar"]);
    style.resolve_viewport_lengths_for_viewport(LayoutSize::new(200.0, 100.0));
    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("expected width");
    };
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = style.box_values.height.value()
    else {
        panic!("expected height");
    };
    assert_eq!(width.length_points(), 25.0);
    assert_eq!(height.length_points(), 1.0);
}

#[tokio::test]
async fn invalid_multicol_and_containment_values_leave_initial_values() {
    let mut style = ComputedStyle::initial();
    apply_declarations(
        &mut style,
        &parse_declarations("column-fill: backwards; column-span: two; contain: size size"),
    );

    assert_eq!(style.column_fill, ColumnFill::Balance);
    assert_eq!(style.column_span, ColumnSpan::None);
    assert_eq!(style.contain, Contain::NONE);

    apply_declarations(&mut style, &parse_declarations("contain: inline-size"));
    assert!(style.contain.inline_size);
    assert!(!style.contain.size);
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

    style.resolve_font_metric_lengths(layout_pt(7.0));

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
        style.background.background_size,
        BackgroundSize::Explicit {
            width: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_percent(
                0.5
            )),
            height: BackgroundSizeAxis::Auto,
        }
    );
    assert_eq!(
        style.background.background_position.x.origin,
        BackgroundPositionOrigin::End
    );
    assert_eq!(
        style.background.background_position.x.offset,
        ComputedLengthPercentage::from_points(3.0)
    );
    assert_eq!(
        style.background.background_position.y.origin,
        BackgroundPositionOrigin::End
    );
    assert_eq!(
        style.background.background_position.y.offset,
        ComputedLengthPercentage::from_percent(0.25)
    );
}

#[tokio::test]
async fn background_shorthand_applies_position_without_a_size_clause() {
    let declarations = parse_declarations("background: url(tile.svg) bottom right");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_position.x.origin,
        BackgroundPositionOrigin::End
    );
    assert_eq!(
        style.background.background_position.y.origin,
        BackgroundPositionOrigin::End
    );
    assert_eq!(
        style.background.background_position.x.offset,
        ComputedLengthPercentage::ZERO
    );
    assert_eq!(
        style.background.background_position.y.offset,
        ComputedLengthPercentage::ZERO
    );
}

#[tokio::test]
async fn parses_numeric_background_position_axes() {
    let declarations = parse_declarations("background-position: -27px 0");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_position.x.origin,
        BackgroundPositionOrigin::Start
    );
    assert_eq!(
        style.background.background_position.x.offset,
        ComputedLengthPercentage::from_points(-27.0 * CSS_PX_TO_PT)
    );
    assert_eq!(
        style.background.background_position.y.origin,
        BackgroundPositionOrigin::Start
    );
    assert_eq!(
        style.background.background_position.y.offset,
        ComputedLengthPercentage::ZERO
    );
}

#[tokio::test]
async fn background_position_defers_min_max_percentages_until_the_free_space_is_known() {
    let declarations = parse_declarations("background-position: min(0%, 100%) max(0%, 100%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    style.resolve_font_metric_lengths(layout_pt(6.0));

    assert_eq!(
        style
            .background
            .background_position
            .x
            .offset
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(-50.0)))
            .map(layout_points),
        Some(-50.0)
    );
    assert_eq!(
        style
            .background
            .background_position
            .y
            .offset
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(-50.0)))
            .map(layout_points),
        Some(0.0)
    );
}

#[tokio::test]
async fn background_position_axis_longhands_replace_only_their_axis() {
    let declarations = parse_declarations(
        "background-position: right top; \
         background-position-x: min(0%, 100%); \
         background-position-y: max(0%, 100%)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_position.x.origin,
        BackgroundPositionOrigin::Start
    );
    assert_eq!(
        style.background.background_position.y.origin,
        BackgroundPositionOrigin::Start
    );
    assert_eq!(
        style
            .background
            .background_position
            .x
            .offset
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(-50.0)))
            .map(layout_points),
        Some(-50.0)
    );
    assert_eq!(
        style
            .background
            .background_position
            .y
            .offset
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(-50.0)))
            .map(layout_points),
        Some(0.0)
    );
}

#[tokio::test]
async fn rejects_invalid_background_size_values_without_discarding_the_cascade() {
    let declarations = parse_declarations(
        "background-size: 50px auto; background-size: -1px -1px; background-size: 1px 2px 3px",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_size,
        BackgroundSize::Explicit {
            width: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                50.0 * CSS_PX_TO_PT
            )),
            height: BackgroundSizeAxis::Auto,
        }
    );
}

#[tokio::test]
async fn background_shorthand_comments_and_invalid_values_preserve_the_cascade() {
    for value in ["/**/limegreen", "limegreen/**/"] {
        let declarations = parse_declarations(&format!("background:{value}"));
        let mut style = default_style_for_tag("div");
        apply_declarations(&mut style, &declarations);
        assert_eq!(
            style.background.background_color.color(),
            Some(CssColor::new(50, 205, 50)),
            "{value}"
        );
    }

    let declarations = parse_declarations("background:limegreen; background:r/**/ed");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(50, 205, 50))
    );
}

#[tokio::test]
async fn background_tokenization_decodes_escaped_keywords_and_gradient_functions() {
    let declarations = parse_declarations(
        r"background: linear\2d gradient(red, blue) no\2d repeat left top / cover",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_repeat,
        BackgroundRepeat::NoRepeat
    );
    assert_eq!(style.background.background_size, BackgroundSize::Cover);
    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::LinearGradient(_))
    ));
}

#[tokio::test]
async fn malformed_background_layer_lists_do_not_apply_a_valid_prefix() {
    let declarations = parse_declarations(
        "background-repeat: no-repeat; \
         background-repeat: repeat, invalid; \
         background-image: linear-gradient(red, blue); \
         background-image: linear-gradient(red, blue),",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_repeat,
        BackgroundRepeat::NoRepeat
    );
    assert!(matches!(
        style.background.background_image.as_image(),
        Some(BackgroundImage::LinearGradient(_))
    ));
}

#[tokio::test]
async fn background_shorthand_size_split_ignores_url_slashes() {
    let declarations = parse_declarations(
        r#"background: url("support/1x1-green.png") 0 0 / 50px 100px no-repeat, red"#,
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.background.background_layers.len(), 2);
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(
        style.background.background_layers[0].repeat,
        BackgroundRepeat::NoRepeat
    );
    assert_eq!(
        style.background.background_layers[0].size,
        BackgroundSize::Explicit {
            width: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                50.0 * CSS_PX_TO_PT
            )),
            height: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                100.0 * CSS_PX_TO_PT
            )),
        }
    );
    assert_eq!(
        style.background.background_layers[1].image,
        ComputedImage::None
    );
}

#[tokio::test]
async fn background_shorthand_extracts_position_after_a_spaced_gradient_function() {
    let declarations =
        parse_declarations("background: linear-gradient(red, red) 1ch 0 / 4ch 1ch no-repeat");
    let mut style = default_style_for_tag("div");
    style.font_size = 18.75;
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_position.x.offset,
        ComputedLengthPercentage::from_ch(1.0)
    );
    assert_eq!(
        style.background.background_position.y.offset,
        ComputedLengthPercentage::ZERO
    );
    assert_eq!(
        style.background.background_size,
        BackgroundSize::Explicit {
            width: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_ch(4.0)),
            height: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_ch(1.0)),
        }
    );
}

#[tokio::test]
async fn background_shorthand_size_split_ignores_data_url_slashes() {
    let declarations =
        parse_declarations("background: url(data:image/png;base64,AAAA) no-repeat 0 0 / 40pt 40pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_size,
        BackgroundSize::Explicit {
            width: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                40.0
            )),
            height: BackgroundSizeAxis::LengthPercentage(ComputedLengthPercentage::from_points(
                40.0
            )),
        }
    );
    assert_eq!(
        style.background.background_repeat,
        BackgroundRepeat::NoRepeat
    );
}

#[tokio::test]
async fn background_shorthand_size_split_ignores_quoted_url_parentheses() {
    let declarations = parse_declarations(
        r#"background: url("support/a(b)/1x1-green.png") no-repeat 0 0 / 40pt 20pt"#,
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.background.background_size,
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
    assert_eq!(missing.color, CssColor::BLACK);

    let mut duplicate = default_style_for_tag("div");
    apply_declarations(
        &mut duplicate,
        &parse_declarations("color: red; color: var(--missing)"),
    );
    assert_eq!(duplicate.color, CssColor::BLACK);

    let mut invalid_specified = default_style_for_tag("div");
    apply_declarations(
        &mut invalid_specified,
        &parse_declarations("color: red; color: definitely-not-a-color"),
    );
    assert_eq!(invalid_specified.color, CssColor::new(255, 0, 0));

    let mut fallback = default_style_for_tag("div");
    apply_declarations(
        &mut fallback,
        &parse_declarations("color: var(--missing, red)"),
    );
    assert_eq!(fallback.color, CssColor::new(255, 0, 0));

    let mut defined_primary = default_style_for_tag("div");
    apply_declarations(
        &mut defined_primary,
        &parse_declarations("--accent: green; color: var(--accent, var(--missing))"),
    );
    assert_eq!(defined_primary.color, CssColor::new(0, 128, 0));

    let mut cyclic_primary = default_style_for_tag("div");
    apply_declarations(
        &mut cyclic_primary,
        &parse_declarations(
            "--first: var(--second); --second: var(--first); \
             color: var(--first, var(--missing, green))",
        ),
    );
    assert_eq!(cyclic_primary.color, CssColor::new(0, 128, 0));

    let mut malformed_fallback = default_style_for_tag("div");
    apply_declarations(
        &mut malformed_fallback,
        &parse_declarations("color: red; --accent: green; color: var(--accent, var(, blue))"),
    );
    assert_eq!(malformed_fallback.color, CssColor::new(255, 0, 0));

    let mut nested = default_style_for_tag("div");
    apply_declarations(
        &mut nested,
        &parse_declarations("--accent: #00ff00; color: var(--accent)"),
    );
    assert_eq!(nested.color, CssColor::new(0, 255, 0));
}

#[tokio::test]
async fn custom_property_component_errors_are_invalid_at_parse_time() {
    let mut unmatched_closer = default_style_for_tag("div");
    apply_declarations(
        &mut unmatched_closer,
        &parse_declarations("--tone: green; --tone: red); color: var(--tone)"),
    );
    assert_eq!(unmatched_closer.color, CssColor::new(0, 128, 0));

    for invalid_fallback in ["var(--tone, \"\n", "var(--tone, url(\"\n"] {
        let mut style = default_style_for_tag("div");
        apply_declarations(
            &mut style,
            &parse_declarations(&format!(
                "color: green; --tone: red; color: {invalid_fallback}"
            )),
        );
        // The malformed declaration is ignored before the cascade, so the
        // preceding specified value—not the variable's primary value—wins.
        assert_eq!(style.color, CssColor::new(0, 128, 0), "{invalid_fallback}");
    }

    // Stylesheet recovery may close an EOF-terminated simple block, but it
    // must retain a newline-terminated BadString token within that block.
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { color: green; --tone: red; color: var(--tone, \"\n",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(style.color, CssColor::new(0, 128, 0));

    let stylesheet = parse_stylesheet(&Css::from_string(
        "body { color: orange } p { color: green; --tone: red; color: var(--tone, \"\n",
    ));
    let body = style_for_element_with_signature(
        ElementSignature::new("body", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let paragraph = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        Some(&body),
        &[ElementSignature::new("body", HashMap::new())],
    );
    assert_eq!(paragraph.color, CssColor::new(0, 128, 0));
}

#[tokio::test]
async fn valid_component_blocks_with_variables_invalidate_at_computed_value_time() {
    let mut style = default_style_for_tag("div");
    style.color = CssColor::new(0, 128, 0);
    apply_declarations(
        &mut style,
        &parse_declarations("color: red; color: { [ var(--missing) ] }"),
    );

    // The declaration is valid at specified-value time because it contains a
    // syntactically valid var(), but its unresolved reference invalidates its
    // consuming property at computed-value time.
    assert_eq!(style.color, CssColor::new(0, 128, 0));

    let stylesheet = parse_stylesheet(&Css::from_string(
        "body { color: green } p { color: red; color: { [ var(--missing) ] } }",
    ));
    let body = style_for_element_with_signature(
        ElementSignature::new("body", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let paragraph = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        Some(&body),
        &[ElementSignature::new("body", HashMap::new())],
    );
    assert_eq!(paragraph.color, CssColor::new(0, 128, 0));
}

#[test]
fn color_scheme_preference_is_an_explicit_rendering_input() {
    let environment = MediaEnvironment::new(MediaType::Screen, CssViewportSize::new(800.0, 600.0))
        .with_color_scheme_preference(ColorSchemePreference::Dark);
    assert_eq!(
        environment.color_scheme_preference,
        ColorSchemePreference::Dark
    );
}

#[tokio::test]
async fn color_scheme_uses_support_order_preference_and_page_scheme() {
    let no_preference =
        MediaEnvironment::new(MediaType::Screen, CssViewportSize::new(800.0, 600.0));
    let stylesheet = parse_stylesheet_with_media_environment(
        &Css::from_string(
            "html { color-scheme: dark } \
             div { color-scheme: normal; background-color: light-dark(red, green) } \
             p { color-scheme: dark light; background-color: light-dark(red, green) } \
             span { color-scheme: initial; background-color: light-dark(red, green) }",
        ),
        &no_preference,
    );
    let html = style_for_element_with_signature(
        ElementSignature::new("html", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let div = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&html),
        &[ElementSignature::new("html", HashMap::new())],
    );
    let paragraph = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&html),
        &[ElementSignature::new("html", HashMap::new())],
    );
    let initial = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&html),
        &[ElementSignature::new("html", HashMap::new())],
    );
    assert_eq!(div.used_color_scheme, UsedColorScheme::Dark);
    assert_eq!(paragraph.used_color_scheme, UsedColorScheme::Dark);
    assert_eq!(initial.used_color_scheme, UsedColorScheme::Dark);

    let dark_preference = no_preference.with_color_scheme_preference(ColorSchemePreference::Dark);
    let stylesheet = parse_stylesheet_with_media_environment(
        &Css::from_string(
            "div { color-scheme: light dark; background-color: light-dark(red, green) } \
             p { color-scheme: light only; background-color: light-dark(red, green) }",
        ),
        &dark_preference,
    );
    let div = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let paragraph = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(div.used_color_scheme, UsedColorScheme::Dark);
    assert_eq!(paragraph.used_color_scheme, UsedColorScheme::Light);
}

#[tokio::test]
async fn registered_color_property_resolves_light_dark_in_its_own_scheme() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"@property --test-color {
              syntax: "<color>";
              inherits: true;
              initial-value: red;
            }
            div {
              color-scheme: dark;
              --test-color: light-dark(red, green);
              background-color: var(--test-color);
            }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.used_color_scheme, UsedColorScheme::Dark);
    assert_eq!(
        style
            .background
            .background_color
            .resolved_color(style.color),
        CssColor::new(0, 128, 0)
    );
    assert_eq!(
        style.custom_properties.get("--test-color"),
        Some(&ComputedCustomPropertyValue::Color(CssColor::new(
            0, 128, 0
        )))
    );
}

#[tokio::test]
async fn registered_color_property_preserves_its_defining_scheme_when_inherited() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"@property --test-color {
              syntax: "<color>";
              inherits: true;
              initial-value: red;
            }
            div { color-scheme: dark; --test-color: light-dark(red, green) }
            span { color-scheme: light; background-color: var(--test-color) }"#,
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
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
        &[ElementSignature::new("div", HashMap::new())],
    );

    assert_eq!(parent.used_color_scheme, UsedColorScheme::Dark);
    assert_eq!(child.used_color_scheme, UsedColorScheme::Light);
    assert_eq!(
        child
            .background
            .background_color
            .resolved_color(child.color),
        CssColor::new(0, 128, 0)
    );
}

#[tokio::test]
async fn registered_color_property_resets_invalid_and_noninherited_values_to_initial() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"@property --tone {
              syntax: "<color>";
              inherits: false;
              initial-value: blue;
            }
            div { --tone: 10px; background-color: var(--tone) }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(
        style
            .background
            .background_color
            .resolved_color(style.color),
        CssColor::new(0, 0, 255)
    );
}

#[tokio::test]
async fn registered_color_property_uses_initial_for_unset_and_noninheritance() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"@property --tone, --other {
              syntax: "<color>";
              inherits: false;
              initial-value: blue;
            }
            @property --tone {
              syntax: "<color>";
              inherits: false;
              initial-value: green;
            }
            div { --tone: red }
            span { --other: unset; background-color: var(--tone) }"#,
    ));
    let parent = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[ElementSignature::new("div", HashMap::new())],
    );
    assert_eq!(
        child
            .background
            .background_color
            .resolved_color(child.color),
        CssColor::new(0, 128, 0)
    );
    assert_eq!(
        child.custom_properties.get("--other"),
        Some(&ComputedCustomPropertyValue::Color(CssColor::new(
            0, 0, 255
        )))
    );
}

#[test]
fn property_rules_use_css_tokens_for_names_and_descriptors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"
            @pr\6f perty --t\6f ne/**/,/**/--other {
                s\79 ntax: "<color>";
                inh\65 rits: /**/ false;
                initial-value: rgb(0 128 0);
            }
            @property --recovered {
                syntax: "<color>" trailing;
                syntax: "<color>";
                inherits: invalid;
                inherits: false;
                unknown-descriptor: "};,@property";
                @unknown { color: red }
                initial-value: blue;
            }
        "#,
    ));

    assert_eq!(stylesheet.property_registrations.len(), 2);
    assert_eq!(
        stylesheet.property_registrations[0].names,
        vec!["--tone", "--other"]
    );
    assert!(!stylesheet.property_registrations[0].registration.inherits);
    assert_eq!(
        stylesheet.property_registrations[0]
            .registration
            .initial_color,
        CssColor::new(0, 128, 0)
    );
    assert_eq!(
        stylesheet.property_registrations[1].names,
        vec!["--recovered"]
    );
    assert!(!stylesheet.property_registrations[1].registration.inherits);
    assert_eq!(
        stylesheet.property_registrations[1]
            .registration
            .initial_color,
        CssColor::new(0, 0, 255)
    );
}

#[test]
fn invalid_property_preludes_and_descriptors_do_not_accept_prefixes() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"
            @property --valid { syntax: "<color>"; inherits: true; initial-value: red }
            @property --invalid, "not-a-name" {
                syntax: "<color>"; inherits: false; initial-value: blue
            }
            @property --bad-syntax {
                syntax: "<color>" trailing; inherits: false; initial-value: blue
            }
            @property --bad-string {
                syntax: "<color>
                inherits: false; initial-value: blue
            }
            @property --later { syntax: "<color>"; inherits: false; initial-value: green }
        "#,
    ));

    assert_eq!(
        stylesheet
            .property_registrations
            .iter()
            .flat_map(|rule| rule.names.iter())
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["--valid", "--later"]
    );
}

#[test]
fn eof_closed_property_rule_keeps_its_complete_descriptors() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@property --eof { syntax: \"<color>\"; inherits: true; initial-value: green",
    ));

    assert_eq!(stylesheet.property_registrations.len(), 1);
    assert_eq!(stylesheet.property_registrations[0].names, vec!["--eof"]);
    assert!(stylesheet.property_registrations[0].registration.inherits);
    assert_eq!(
        stylesheet.property_registrations[0]
            .registration
            .initial_color,
        CssColor::new(0, 128, 0)
    );
}

#[tokio::test]
async fn custom_property_references_use_decoded_css_identifiers() {
    for declarations in [
        r"--0: green; color: var(--\30)",
        r"--\30: green; color: var(--\30)",
        r"--\d800: green; color: var(--\fffd)",
        r"--\ffffff: green; color: var(--\fffd)",
        r"--a-长-name-that-might-be-longer-than-you\27 d: green; color: var(--a-长-name-that-might-be-longer-than-you\27 d)",
    ] {
        let mut style = default_style_for_tag("div");
        apply_declarations(&mut style, &parse_declarations(declarations));
        assert_eq!(style.color, CssColor::new(0, 128, 0), "{declarations}");
    }
}

#[tokio::test]
async fn custom_property_resolution_uses_css_token_boundaries_and_eof_recovery() {
    let mut escaped_function = default_style_for_tag("div");
    apply_declarations(
        &mut escaped_function,
        &parse_declarations(r"--accent: green; color: v\61r(--accent)"),
    );
    assert_eq!(escaped_function.color, CssColor::new(0, 128, 0));

    let mut eof_closed = default_style_for_tag("div");
    apply_declarations(
        &mut eof_closed,
        &parse_declarations("--accent: green; color: var(--accent /* unclosed comment"),
    );
    assert_eq!(eof_closed.color, CssColor::new(0, 128, 0));

    let eof_nested_fallback = parse_stylesheet(&Css::from_string(
        "p { color: red; --accent: green; color: var(--accent, var(--missing)",
    ));
    let eof_nested_fallback = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[eof_nested_fallback],
        None,
        &[],
    );
    assert_eq!(eof_nested_fallback.color, CssColor::new(0, 128, 0));

    let mut quoted_text = default_style_for_tag("div");
    apply_declarations(
        &mut quoted_text,
        &parse_declarations(r#"color: red; color: "var(--missing)""#),
    );
    assert_eq!(quoted_text.color, CssColor::new(255, 0, 0));

    let mut cyclic = default_style_for_tag("div");
    apply_declarations(
        &mut cyclic,
        &parse_declarations(r"--\30: var(--other); --other: var(--\30); color: var(--\30, green)"),
    );
    assert_eq!(cyclic.color, CssColor::new(0, 128, 0));
}

#[tokio::test]
async fn variable_substitution_expands_shorthands_at_computed_value_time() {
    let mut outline = default_style_for_tag("div");
    apply_declarations(
        &mut outline,
        &parse_declarations("--color: green; outline: medium solid var(--color)"),
    );
    assert_eq!(outline.outline_color, CssColor::new(0, 128, 0));
    assert_eq!(outline.outline_style, BorderStyle::Solid);

    let mut logical_border = default_style_for_tag("div");
    apply_declarations(
        &mut logical_border,
        &parse_declarations("--color: green; border-inline: medium solid var(--color)"),
    );
    assert_eq!(logical_border.border_colors.left, CssColor::new(0, 128, 0));
    assert_eq!(logical_border.border_colors.right, CssColor::new(0, 128, 0));
    assert_eq!(logical_border.border_styles.left, BorderStyle::Solid);
    assert_eq!(logical_border.border_styles.right, BorderStyle::Solid);
}

#[tokio::test]
async fn custom_properties_preserve_font_family_token_boundaries() {
    let cases = [
        (
            "--a: Ahem, sans-serif; font-family: var(--a)",
            FontFamily::List(vec![
                FontFamily::Names(vec!["Ahem".to_string()]),
                FontFamily::SansSerif,
            ]),
        ),
        (
            "--a: var(--b), sans-serif; --b: Ahem; font-family: var(--a)",
            FontFamily::List(vec![
                FontFamily::Names(vec!["Ahem".to_string()]),
                FontFamily::SansSerif,
            ]),
        ),
        (
            "--a: SomeUnknownFont, var(--b); --b: Ahem; font-family: var(--a)",
            FontFamily::List(vec![
                FontFamily::Names(vec!["SomeUnknownFont".to_string()]),
                FontFamily::Names(vec!["Ahem".to_string()]),
            ]),
        ),
        (
            "--a: Ahem var(--b) sans-serif; --b: ,; font-family: var(--a)",
            FontFamily::List(vec![
                FontFamily::Names(vec!["Ahem".to_string()]),
                FontFamily::SansSerif,
            ]),
        ),
    ];

    for (declarations, expected) in cases {
        let mut style = default_style_for_tag("p");
        apply_declarations(&mut style, &parse_declarations(declarations));
        assert_eq!(style.font_family, expected, "{declarations}");
    }
}

#[tokio::test]
async fn custom_properties_preserve_generated_content_string_tokens() {
    let mut style = default_style_for_tag("p");
    apply_declarations(
        &mut style,
        &parse_declarations(r#"--a: "hello"; --b: "there"; content: var(--a) " " var(--b)"#),
    );

    assert_eq!(
        style.content,
        Content::List {
            parts: vec![
                GeneratedContentPart::Text("hello".to_string()),
                GeneratedContentPart::Text(" ".to_string()),
                GeneratedContentPart::Text("there".to_string()),
            ],
            alt: None,
        }
    );
}

#[tokio::test]
async fn custom_properties_preserve_scalar_token_boundaries() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "--length: 10px; --opacity: .5; width: var(--length); opacity: var(--opacity)",
        ),
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("width should resolve to a length");
    };
    assert!((width.length_points() - 10.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert_eq!(style.opacity, 0.5);
}

#[tokio::test]
async fn invalid_variable_shorthand_resets_every_affected_longhand() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "outline: medium solid red; outline: medium solid var(--missing); \
             border-left: medium solid red; border: medium solid var(--missing)",
        ),
    );
    assert_eq!(style.outline_style, BorderStyle::None);
    assert_eq!(style.outline_color.resolve(style.color), CssColor::BLACK);
    assert_eq!(style.border_styles.left, BorderStyle::None);
    assert_eq!(
        style.border_colors.left.resolve(style.color),
        CssColor::BLACK
    );
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

    assert_eq!(style.color, CssColor::BLACK);
}

#[tokio::test]
async fn important_declarations_participate_in_cascade_sorting() {
    let mut direct = default_style_for_tag("div");
    apply_declarations(
        &mut direct,
        &parse_declarations("color: red !important; color: blue"),
    );
    assert_eq!(direct.color, CssColor::new(255, 0, 0));

    let stylesheet = parse_stylesheet(&Css::from_string("div { color: red !important }"));
    let inline_normal = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("color: blue"),
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    assert_eq!(inline_normal.color, CssColor::new(255, 0, 0));

    let inline_important = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("color: blue !important"),
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(inline_important.color, CssColor::new(0, 0, 255));
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
async fn parses_initial_letter_properties() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "initial-letter: 3 drop; initial-letter-align: border-box ideographic; initial-letter-wrap: 25%",
        ),
    );

    assert_eq!(
        style.initial_letter,
        InitialLetter::Specified { size: 3.0, sink: 3 }
    );
    assert_eq!(
        style.initial_letter_align,
        InitialLetterAlign {
            border_box: true,
            keyword: InitialLetterAlignKeyword::Ideographic
        }
    );
    assert_eq!(
        style.initial_letter_wrap,
        InitialLetterWrap::Offset(ComputedLengthPercentage::from_percent(0.25))
    );

    apply_declarations(&mut style, &parse_declarations("initial-letter: raise 2.5"));
    assert_eq!(
        style.initial_letter,
        InitialLetter::Specified { size: 2.5, sink: 1 }
    );

    apply_declarations(
        &mut style,
        &parse_declarations("initial-letter: 0; initial-letter-wrap: banana"),
    );
    assert_eq!(
        style.initial_letter,
        InitialLetter::Specified { size: 2.5, sink: 1 }
    );
    assert_eq!(
        style.initial_letter_wrap,
        InitialLetterWrap::Offset(ComputedLengthPercentage::from_percent(0.25))
    );
}

#[tokio::test]
async fn initial_letter_align_and_wrap_inherit() {
    let mut parent = default_style_for_tag("div");
    apply_declarations(
        &mut parent,
        &parse_declarations("initial-letter-align: hanging; initial-letter-wrap: first"),
    );
    let stylesheet = parse_stylesheet(&Css::from_string(
        "span { initial-letter-align: inherit; initial-letter-wrap: inherit }",
    ));
    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[],
    );

    assert_eq!(
        child.initial_letter_align,
        InitialLetterAlign {
            border_box: false,
            keyword: InitialLetterAlignKeyword::Hanging
        }
    );
    assert_eq!(child.initial_letter_wrap, InitialLetterWrap::First);
    assert_eq!(child.initial_letter, InitialLetter::Normal);
}

#[tokio::test]
async fn supports_rule_recognizes_initial_letter_properties() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (initial-letter: 3 drop) { p { color: blue } }\
         @supports (initial-letter-align: hanging) { p { border-top-color: green } }\
         @supports (initial-letter-wrap: grid) { p { border-bottom-color: blue } }\
         @supports (initial-letter: 0) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(style.border_colors.top, CssColor::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, CssColor::new(0, 0, 255));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
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
        ("math", Display::INLINE, false),
        ("inline math", Display::INLINE, false),
        ("math inline", Display::INLINE, false),
        ("block math", Display::BLOCK, false),
        ("math block", Display::BLOCK, false),
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
        ("ruby", Display::RUBY, false),
        ("inline ruby", Display::RUBY, false),
        (
            "block ruby",
            Display::new(DisplayOuter::Block, DisplayInner::Ruby),
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
        ("ruby-base", Display::RUBY_BASE, false),
        ("ruby-text", Display::RUBY_TEXT, false),
        ("ruby-base-container", Display::RUBY_BASE_CONTAINER, false),
        ("ruby-text-container", Display::RUBY_TEXT_CONTAINER, false),
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
async fn display_contents_computes_to_none_for_unusual_html_elements() {
    for tag in [
        "br", "wbr", "meter", "progress", "canvas", "embed", "object", "audio", "iframe", "img",
        "video", "frame", "frameset", "input", "textarea", "select",
    ] {
        let style = style_for_element_with_signature(
            ElementSignature::new(tag, HashMap::new()),
            Some("display: contents"),
            &[],
            None,
            &[],
        );

        assert_eq!(style.display, Display::NONE, "{tag}");
    }
}

#[tokio::test]
async fn display_contents_remains_contents_for_ordinary_html_elements() {
    for tag in ["div", "legend", "button", "details", "fieldset"] {
        let style = style_for_element_with_signature(
            ElementSignature::new(tag, HashMap::new()),
            Some("display: contents"),
            &[],
            None,
            &[],
        );

        assert_eq!(style.display, Display::CONTENTS, "{tag}");
    }
}

#[tokio::test]
async fn display_contents_computes_to_none_for_content_replacement() {
    let style = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("display: contents; content: linear-gradient(red, blue)"),
        &[],
        None,
        &[],
    );

    assert_eq!(style.display, Display::NONE);
}

#[tokio::test]
async fn class_selectors_match_whitespace_separated_tokens_for_display() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".none { display:none } .green { color: green }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "optgroup",
            HashMap::from([("class".to_string(), "none green".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.display, Display::NONE);
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
    assert_eq!(
        style.list_style_type,
        ListStyleType::Named("lower-alpha".to_string())
    );
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
        assert_eq!(style.list_style_image, ComputedImage::None);
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
        assert_eq!(list_style_image_url(&style), expected_image, "{value}");
    }
}

#[tokio::test]
async fn invalid_list_style_values_do_not_partially_apply() {
    let declarations = parse_declarations(
        "list-style: lower-alpha inside url(marker.png); list-style: none disc url(other.png)",
    );
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.list_style_type,
        ListStyleType::Named("lower-alpha".to_string())
    );
    assert_eq!(style.list_style_position, ListStylePosition::Inside);
    assert_eq!(list_style_image_url(&style), Some("marker.png"));

    apply_declarations(
        &mut style,
        &parse_declarations("list-style-type: \"# \" inside"),
    );
    assert_eq!(
        style.list_style_type,
        ListStyleType::Named("lower-alpha".to_string())
    );
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
async fn marker_rules_do_not_generate_marker_styles_for_non_list_items() {
    let stylesheet = parse_stylesheet(&Css::from_string(r#"p::marker { content: "x" }"#));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert!(style.marker_style.is_none());
}

#[tokio::test]
async fn ua_marker_defaults_apply_to_nested_generated_markers() {
    let ua = html5_user_agent_stylesheet();
    let author = parse_stylesheet(&Css::from_string(
        r#"li::before, li::after { content: "x"; display: list-item }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("li", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &author]),
        None,
        &[],
    );
    let expected = FontVariantNumeric::Values(vec![FontVariantNumericValue::TabularNums]);

    assert_eq!(
        style
            .marker_style
            .as_deref()
            .expect("principal marker style")
            .font_variant_numeric,
        expected
    );
    assert_eq!(
        style
            .before_style
            .as_deref()
            .and_then(|pseudo| pseudo.marker_style.as_deref())
            .expect("before marker style")
            .font_variant_numeric,
        expected
    );
    assert_eq!(
        style
            .after_style
            .as_deref()
            .and_then(|pseudo| pseudo.marker_style.as_deref())
            .expect("after marker style")
            .font_variant_numeric,
        expected
    );
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
    assert_eq!(list_style_image_url(&style), Some("other.png"));

    apply_declarations(&mut style, &parse_declarations("list-style-image: none"));
    assert_eq!(style.list_style_image, ComputedImage::None);
}

#[tokio::test]
async fn list_style_image_preserves_image_set_resolution_and_rejects_invalid_values() {
    let mut style = default_style_for_tag("li");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "list-style-image: image-set(url(marker.png) 0.5x); \
             list-style-image: image-set(url(ignored.png) -1x)",
        ),
    );

    assert!(matches!(
        style.list_style_image.as_image(),
        Some(BackgroundImage::ImageSet(set))
            if set.options.len() == 1
                && set.options[0].resolution_dppx == 0.5
                && matches!(*set.options[0].image, BackgroundImage::Url(ref url) if url.href == "marker.png")
    ));

    apply_declarations(
        &mut style,
        &parse_declarations("list-style: square inside image-set(url(shorthand.png) 2x)"),
    );
    assert_eq!(style.list_style_type, ListStyleType::Square);
    assert_eq!(style.list_style_position, ListStylePosition::Inside);
    assert!(matches!(
        style.list_style_image.as_image(),
        Some(BackgroundImage::ImageSet(set))
            if set.options.len() == 1
                && set.options[0].resolution_dppx == 2.0
                && matches!(*set.options[0].image, BackgroundImage::Url(ref url) if url.href == "shorthand.png")
    ));
}

#[tokio::test]
async fn parses_overridable_predefined_counter_style_names_as_named_styles() {
    for (value, expected) in [
        ("decimal-leading-zero", "decimal-leading-zero"),
        ("arabic-indic", "arabic-indic"),
        ("khmer", "khmer"),
        ("cjk-decimal", "cjk-decimal"),
        ("lower-armenian", "lower-armenian"),
        ("lower-greek", "lower-greek"),
        ("hiragana-iroha", "hiragana-iroha"),
        ("cjk-heavenly-stem", "cjk-heavenly-stem"),
        ("upper-roman", "upper-roman"),
    ] {
        let declarations = parse_declarations(&format!("list-style-type: {value}"));
        let mut style = default_style_for_tag("li");
        apply_declarations(&mut style, &declarations);
        assert_eq!(
            style.list_style_type,
            ListStyleType::Named(expected.to_string()),
            "{value}"
        );
    }

    let declarations = parse_declarations("list-style-type: disclosure-closed");
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.list_style_type, ListStyleType::DisclosureClosed);
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
async fn counter_style_strings_decode_css_escapes_like_symbols_function() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"@counter-style escaped { system: numeric; symbols: '\2020' '\2021' '\A7' '\26AA'; prefix: '\2020'; suffix: '\2021'; negative: '\A7' '\26AA'; pad: 2 '\2020' }"#,
    ));
    let rule = &stylesheet.counter_styles[0];

    assert_eq!(rule.symbols, ["†", "‡", "§", "⚪"]);
    assert_eq!(rule.prefix.as_deref(), Some("†"));
    assert_eq!(rule.suffix.as_deref(), Some("‡"));
    assert_eq!(
        rule.negative
            .as_ref()
            .map(|(prefix, suffix)| (prefix.as_str(), suffix.as_str())),
        Some(("§", "⚪"))
    );
    assert_eq!(
        rule.pad
            .as_ref()
            .map(|(width, symbol)| (*width, symbol.as_str())),
        Some((2, "†"))
    );

    let declarations =
        parse_declarations(r#"list-style-type: symbols(numeric '\2020' '\2021' '\A7' '\26AA')"#);
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);
    let ListStyleType::Anonymous(rule) = style.list_style_type else {
        panic!("symbols() should produce an anonymous counter style");
    };
    assert_eq!(rule.symbols, ["†", "‡", "§", "⚪"]);
}

#[tokio::test]
async fn symbols_function_rejects_unquoted_symbols_but_counter_styles_accept_idents() {
    for value in [
        "symbols(a b c)",
        "symbols(alphabetic a b c)",
        "symbols(numeric 0 1 2)",
        "symbols(additive 'a' 'b')",
        "symbols(fixed)",
        "symbols(alphabetic 'a')",
        "symbols(numeric '0')",
    ] {
        assert!(parse_list_style_type(value).is_none(), "{value}");
    }

    let stylesheet = parse_stylesheet(&Css::from_string(
        "@counter-style identifiers { system: cyclic; symbols: first second }",
    ));
    assert_eq!(stylesheet.counter_styles[0].symbols, ["first", "second"]);
}

#[tokio::test]
async fn counter_style_references_decode_custom_identifiers_in_lists_and_content() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r"@counter-style \3BB \3B1 { system: cyclic; symbols: \2023; suffix: '' }",
    ));
    assert_eq!(stylesheet.counter_styles[0].name, "λα");

    let declarations = parse_declarations(
        r"list-style-type: \3BB \3B1; content: counter(item, \3BB \3B1) counters(item, '.', \3BB \3B1)",
    );
    let mut style = default_style_for_tag("li");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.list_style_type,
        ListStyleType::Named("λα".to_string())
    );
    let Content::List { parts, .. } = style.content else {
        panic!("counter() and counters() should parse")
    };
    assert_eq!(
        parts,
        vec![
            GeneratedContentPart::Counter {
                name: "item".to_string(),
                style: Some(ListStyleType::Named("λα".to_string())),
            },
            GeneratedContentPart::Counters {
                name: "item".to_string(),
                separator: ".".to_string(),
                style: Some(ListStyleType::Named("λα".to_string())),
            },
        ]
    );
}

#[test]
fn predefined_counter_styles_other_than_the_six_reserved_names_can_be_redefined() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@counter-style lower-roman { system: cyclic; symbols: r; suffix: ' ' }",
    ));
    assert_eq!(stylesheet.counter_styles.len(), 1);
    assert_eq!(stylesheet.counter_styles[0].name, "lower-roman");
    assert_eq!(stylesheet.counter_styles[0].symbols, ["r"]);
}

#[tokio::test]
async fn parses_counter_style_range_intervals_and_auto() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@counter-style split { system: numeric; symbols: \"0\" \"1\"; range: 1 10, 20 infinite }\
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
async fn parses_scroll_marker_pseudo_element_rules_and_target_state() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "section::scroll-marker { content: \"•\" } \
         section::scroll-marker-group { display: flex } \
         section::scroll-marker:target-current { color: red }",
    ));

    assert_eq!(stylesheet.scroll_marker_rules.len(), 2);
    assert_eq!(stylesheet.scroll_marker_group_rules.len(), 1);
    assert_eq!(
        stylesheet.scroll_marker_rules[1].selector_text,
        "section:target-current"
    );
}

#[tokio::test]
async fn nested_scroll_marker_rules_keep_typed_pseudo_routes() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".scroller { &::scroll-marker-group { display: grid } } \
         .target { &::scroll-marker { content: \"marker\" } \
                   &::scroll-marker:target-current { color: red } } \
         .host { &::scroll-marker, & .ordinary { content: \"mixed\" } } \
         .outer { .inner { &::scroll-marker { content: \"nested\" } } }",
    ));

    assert_eq!(stylesheet.scroll_marker_group_rules.len(), 1);
    assert_eq!(stylesheet.scroll_marker_rules.len(), 4);
    assert_eq!(stylesheet.rules.len(), 1);
    assert!(stylesheet.rules[0].selector_text.contains(".ordinary"));
    assert!(
        stylesheet
            .scroll_marker_rules
            .iter()
            .all(|rule| !rule.selector_text.contains("scroll-marker"))
    );
    assert!(
        stylesheet
            .scroll_marker_rules
            .iter()
            .any(|rule| rule.selector_text.contains(":target-current"))
    );
    assert!(
        stylesheet
            .scroll_marker_rules
            .iter()
            .any(|rule| rule.selector_text.contains(".outer")
                && rule.selector_text.contains(".inner"))
    );

    let scroller = style_for_element_with_signature(
        ElementSignature::new(
            "div",
            HashMap::from([("class".to_string(), "scroller".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    assert!(!scroller.display.is_grid());
    assert!(
        scroller
            .scroll_marker_group_style
            .as_deref()
            .is_some_and(|style| style.display.is_grid())
    );

    let mut target = ElementSignature::new(
        "div",
        HashMap::from([("class".to_string(), "target".to_string())]),
    );
    target.is_target = true;
    let target = style_for_element_with_signature(
        target,
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let marker = target
        .scroll_marker_style
        .as_deref()
        .expect("nested scroll marker style");
    assert!(marker.content.is_generated());
    assert_eq!(marker.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn parses_markers_of_before_and_after_pseudo_elements() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "div::before, div::after { content: \"x\"; display: list-item }\
         div::before::marker { content: \"before-marker\" }\
         div::after::marker { content: \"after-marker\" }",
    ));
    assert_eq!(stylesheet.before_marker_rules.len(), 1);
    assert_eq!(stylesheet.after_marker_rules.len(), 1);
    assert_eq!(stylesheet.before_marker_rules[0].selector_text, "div");
    assert_eq!(stylesheet.after_marker_rules[0].selector_text, "div");
    let style = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    assert_eq!(
        style
            .before_style
            .as_deref()
            .and_then(|style| style.marker_style.as_deref())
            .map(|style| &style.marker_content),
        Some(&MarkerContent::Parts(vec![MarkerContentPart::Text(
            "before-marker".to_string()
        )]))
    );
    assert_eq!(
        style
            .after_style
            .as_deref()
            .and_then(|style| style.marker_style.as_deref())
            .map(|style| &style.marker_content),
        Some(&MarkerContent::Parts(vec![MarkerContentPart::Text(
            "after-marker".to_string()
        )]))
    );
}

#[tokio::test]
async fn chained_marker_fallback_rejects_pseudo_class_suffixes() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p::before::marker:not(.selected) { color: red } q { color: blue }",
    ));

    assert!(stylesheet.before_marker_rules.is_empty());
    assert_eq!(stylesheet.rules.len(), 1);
    assert_eq!(stylesheet.rules[0].selector_text, "q");
}

#[tokio::test]
async fn marker_content_preserves_attr_for_layout_time_evaluation() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"li { display: list-item }
           li::marker { content: attr(icon, "* ") }"#,
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "li",
            HashMap::from([("icon".to_string(), "@ ".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let marker = style.marker_style.as_deref().expect("marker style");
    assert_eq!(marker.marker_content, MarkerContent::Auto);
    assert_eq!(
        marker.content.generated_parts(),
        Some(
            [GeneratedContentPart::Attr {
                name: "icon".to_string(),
                fallback: Some("* ".to_string()),
            }]
            .as_slice()
        )
    );
}

#[tokio::test]
async fn pseudo_after_combinator_uses_implicit_universal_selector() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".list > ::before { content: \"x\"; unicode-bidi: isolate; display: inline-flex }",
    ));
    assert_eq!(stylesheet.before_rules.len(), 1);
    assert_eq!(stylesheet.before_rules[0].selector_text, ".list > *");
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
        vec![
            CounterReset {
                name: "section".to_string(),
                kind: CounterResetKind::Forward(CounterValue::new(2)),
            },
            CounterReset {
                name: "list-item".to_string(),
                kind: CounterResetKind::Forward(CounterValue::new(4)),
            },
        ]
    );
    assert_eq!(
        style.counter_increments,
        vec![
            CounterChange {
                name: "other".to_string(),
                value: CounterValue::new(1),
            },
            CounterChange {
                name: "list-item".to_string(),
                value: CounterValue::new(2),
            },
        ]
    );
    assert_eq!(
        style.counter_sets,
        vec![CounterChange {
            name: "list-item".to_string(),
            value: CounterValue::new(9),
        }]
    );
}

#[tokio::test]
async fn parses_integral_calc_values_in_counter_properties() {
    let declarations = parse_declarations(
        "counter-reset: chapter calc(3 + 5); counter-increment: item calc(4 + 6); counter-set: page calc(12 / 3)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.counter_resets,
        vec![CounterReset {
            name: "chapter".to_string(),
            kind: CounterResetKind::Forward(CounterValue::new(8)),
        }]
    );
    assert_eq!(
        style.counter_increments,
        vec![CounterChange {
            name: "item".to_string(),
            value: CounterValue::new(10),
        }]
    );
    assert_eq!(
        style.counter_sets,
        vec![CounterChange {
            name: "page".to_string(),
            value: CounterValue::new(4),
        }]
    );

    let invalid = parse_declarations("counter-reset: chapter calc(1.5)");
    apply_declarations(&mut style, &invalid);
    assert_eq!(
        style.counter_resets[0].kind,
        CounterResetKind::Forward(CounterValue::new(8))
    );
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
        vec![
            CounterReset {
                name: "section".to_string(),
                kind: CounterResetKind::Forward(CounterValue::new(2)),
            },
            CounterReset {
                name: "chapter".to_string(),
                kind: CounterResetKind::Forward(CounterValue::new(3)),
            },
        ]
    );
    assert_eq!(
        style.counter_increments,
        vec![
            CounterChange {
                name: "item".to_string(),
                value: CounterValue::new(1),
            },
            CounterChange {
                name: "item".to_string(),
                value: CounterValue::new(2),
            },
        ]
    );
    assert_eq!(
        style.counter_sets,
        vec![CounterChange {
            name: "page".to_string(),
            value: CounterValue::new(5),
        }]
    );

    let invalid = parse_declarations(
        "counter-reset: none chapter; counter-increment: none 1; counter-set: item 1.5",
    );
    apply_declarations(&mut style, &invalid);
    assert_eq!(
        style.counter_resets,
        vec![
            CounterReset {
                name: "section".to_string(),
                kind: CounterResetKind::Forward(CounterValue::new(2)),
            },
            CounterReset {
                name: "chapter".to_string(),
                kind: CounterResetKind::Forward(CounterValue::new(3)),
            },
        ]
    );
    assert_eq!(
        style.counter_increments,
        vec![
            CounterChange {
                name: "item".to_string(),
                value: CounterValue::new(1),
            },
            CounterChange {
                name: "item".to_string(),
                value: CounterValue::new(2),
            },
        ]
    );
    assert_eq!(
        style.counter_sets,
        vec![CounterChange {
            name: "page".to_string(),
            value: CounterValue::new(5),
        }]
    );
}

#[tokio::test]
async fn parses_reversed_counter_resets_and_rejects_malformed_functions() {
    let mut style = default_style_for_tag("div");
    let declarations = parse_declarations(
        "counter-reset: chapter 2 reversed(section) reversed(item) -4 chapter 7",
    );
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.counter_resets,
        vec![
            CounterReset {
                name: "section".to_string(),
                kind: CounterResetKind::Reversed(None),
            },
            CounterReset {
                name: "item".to_string(),
                kind: CounterResetKind::Reversed(Some(CounterValue::new(-4))),
            },
            CounterReset {
                name: "chapter".to_string(),
                kind: CounterResetKind::Forward(CounterValue::new(7)),
            },
        ]
    );

    let malformed = parse_declarations("counter-reset: reversed(item extra)");
    apply_declarations(&mut style, &malformed);
    assert_eq!(style.counter_resets.len(), 3);
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
                style: Some(ListStyleType::Named("upper-roman".to_string()))
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
            NamedStringPart::Image(ComputedImage::image(BackgroundImage::Url(ImageUrl {
                href: "icon.png".to_string(),
                base_url: None,
                root_url: None,
                request_modifiers: RequestUrlModifiers::default(),
            })))
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
                target: TargetReference::Fragment("#chapter".to_string()),
                name: "page".to_string(),
                style: Some(ListStyleType::Named("upper-roman".to_string()))
            },
            NamedStringPart::String(" ".to_string()),
            NamedStringPart::TargetText {
                target: TargetReference::Fragment("#chapter".to_string()),
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
        GridPlacement::Line(GridLinePlacement::Number(
            std::num::NonZeroI32::new(2).unwrap()
        ))
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Span(GridSpanPlacement::Named {
            name: "footer".to_string(),
            count: Some(std::num::NonZeroU16::new(3).unwrap())
        })
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "main".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Span(GridSpanPlacement::Count(
            std::num::NonZeroU16::new(2).unwrap()
        ))
    );
}

#[tokio::test]
async fn parses_escaped_grid_custom_idents() {
    let declarations = parse_declarations(
        r#"grid-template-columns: [\31st-start] 10pt [\31st-end];
           grid-template-areas: "\31st \32nd";
           grid-column-start: \31st-start;
           grid-column-end: span \32nd-end 1"#,
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let GridTrackList::Tracks {
        components: columns,
        trailing_names,
    } = &style.grid_template_columns
    else {
        panic!("escaped grid-template-columns line names should parse");
    };
    let GridTrackListComponent::Track(names, _) = &columns[0] else {
        panic!("expected escaped named track");
    };
    assert_eq!(names, &["1st-start".to_string()]);
    assert_eq!(trailing_names, &["1st-end".to_string()]);

    let GridTemplateAreas::Areas(rows) = &style.grid_template_areas else {
        panic!("digit-starting grid-template-areas should parse");
    };
    assert_eq!(
        rows[0].cells,
        [Some("1st".to_string()), Some("2nd".to_string())]
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "1st-start".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Span(GridSpanPlacement::Named {
            name: "2nd-end".to_string(),
            count: Some(std::num::NonZeroU16::new(1).unwrap())
        })
    );
}

#[tokio::test]
async fn parses_bracketed_grid_line_names_with_escape_terminator_whitespace() {
    let declarations = parse_declarations(r#"grid-template-columns: [\31 a alpha] 10pt [\32 b];"#);
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let GridTrackList::Tracks {
        components: columns,
        trailing_names,
    } = &style.grid_template_columns
    else {
        panic!("escaped bracketed grid line names should parse");
    };
    let GridTrackListComponent::Track(names, _) = &columns[0] else {
        panic!("expected named track");
    };
    assert_eq!(names, &["1a".to_string(), "alpha".to_string()]);
    assert_eq!(trailing_names, &["2b".to_string()]);
}

#[tokio::test]
async fn grid_template_shorthand_decodes_escaped_area_strings() {
    let declarations = parse_declarations(r#"grid-template: "\31st \32nd" 10pt / 20pt 30pt"#);
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let GridTemplateAreas::Areas(rows) = &style.grid_template_areas else {
        panic!("escaped shorthand grid-template-areas should parse");
    };
    assert_eq!(
        rows[0].cells,
        [Some("1st".to_string()), Some("2nd".to_string())]
    );
    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("grid-template columns should parse");
    };
    assert_eq!(columns.len(), 2);
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
async fn parses_intrinsic_grid_lanes_auto_repeat_track_lists() {
    let declarations = parse_declarations(
        "grid-template-columns: repeat(auto-fit, max-content);\
         grid-template-rows: repeat(auto-fill, min-content);",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("intrinsic auto-fit columns should parse");
    };
    let GridTrackListComponent::Repeat(_, columns_repeat) = &columns[0] else {
        panic!("expected intrinsic auto-fit repeat");
    };
    assert_eq!(columns_repeat.count, GridRepeatCount::AutoFit);
    assert!(matches!(
        columns_repeat.tracks[0],
        GridTrackListComponent::Track(
            _,
            GridTrackSize {
                min: GridMinTrackBreadth::MaxContent,
                max: GridMaxTrackBreadth::MaxContent,
            }
        )
    ));

    let GridTrackList::Tracks {
        components: rows, ..
    } = &style.grid_template_rows
    else {
        panic!("intrinsic auto-fill rows should parse");
    };
    let GridTrackListComponent::Repeat(_, rows_repeat) = &rows[0] else {
        panic!("expected intrinsic auto-fill repeat");
    };
    assert_eq!(rows_repeat.count, GridRepeatCount::AutoFill);
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
async fn invalid_negative_grid_track_breadths_do_not_apply() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: 20pt;\
             grid-template-rows: minmax(auto, 30pt)",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: -10pt;\
             grid-template-rows: minmax(-1pt, 10pt)",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations(
            "grid-template-columns: fit-content(-5pt);\
             grid-template-rows: minmax(auto, -10pt)",
        ),
    );
    apply_declarations(
        &mut style,
        &parse_declarations("grid-template-columns: repeat(auto-fill, -10pt)"),
    );

    let GridTrackList::Tracks {
        components: columns,
        ..
    } = &style.grid_template_columns
    else {
        panic!("valid initial grid-template-columns should remain");
    };
    assert_eq!(columns.len(), 1);
    let GridTrackListComponent::Track(_, ref column_size) = columns[0] else {
        panic!("valid initial column track should remain");
    };
    assert_eq!(
        column_size.min,
        GridMinTrackBreadth::LengthPercentage(ComputedLengthPercentage::from_points(20.0))
    );

    let GridTrackList::Tracks {
        components: rows, ..
    } = &style.grid_template_rows
    else {
        panic!("valid initial grid-template-rows should remain");
    };
    assert_eq!(rows.len(), 1);
    let GridTrackListComponent::Track(_, ref row_size) = rows[0] else {
        panic!("valid initial row track should remain");
    };
    assert_eq!(row_size.min, GridMinTrackBreadth::Auto);
    assert_eq!(
        row_size.max,
        GridMaxTrackBreadth::LengthPercentage(ComputedLengthPercentage::from_points(30.0))
    );
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
    apply_declarations(
        &mut style,
        &parse_declarations("grid-template-columns: [] 50pt"),
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
        GridPlacement::Line(GridLinePlacement::Named {
            name: "header".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Span(GridSpanPlacement::Count(
            std::num::NonZeroU16::new(2).unwrap()
        ))
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement::Number(
            std::num::NonZeroI32::new(2).unwrap()
        ))
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "main".to_string(),
            occurrence: None
        })
    );

    let declarations = parse_declarations("grid-row-start: auto; grid-column: 2");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.grid_row_start, GridPlacement::Auto);
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement::Number(
            std::num::NonZeroI32::new(2).unwrap()
        ))
    );
    assert_eq!(style.grid_column_end, GridPlacement::Auto);
}

#[tokio::test]
async fn parses_grid_area_shorthand() {
    let declarations = parse_declarations("grid-area: main");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let expected = GridPlacement::Line(GridLinePlacement::Named {
        name: "main".to_string(),
        occurrence: None,
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
        GridPlacement::Line(GridLinePlacement::Number(
            std::num::NonZeroI32::new(2).unwrap()
        ))
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "side".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Span(GridSpanPlacement::Count(
            std::num::NonZeroU16::new(3).unwrap()
        ))
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Line(GridLinePlacement::Number(
            std::num::NonZeroI32::new(4).unwrap()
        ))
    );
}

#[tokio::test]
async fn parses_grid_area_shorthand_omitted_values() {
    let declarations = parse_declarations("grid-area: header / main");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let header = GridPlacement::Line(GridLinePlacement::Named {
        name: "header".to_string(),
        occurrence: None,
    });
    let main = GridPlacement::Line(GridLinePlacement::Named {
        name: "main".to_string(),
        occurrence: None,
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
        GridPlacement::Line(GridLinePlacement::Number(
            std::num::NonZeroI32::new(2).unwrap()
        ))
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "main".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Line(GridLinePlacement::Number(
            std::num::NonZeroI32::new(4).unwrap()
        ))
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "main".to_string(),
            occurrence: None
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
        GridPlacement::Line(GridLinePlacement::Named {
            name: "header".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "main".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "header".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "main".to_string(),
            occurrence: None
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
        GridPlacement::Line(GridLinePlacement::Named {
            name: "header".to_string(),
            occurrence: std::num::NonZeroI32::new(2),
        })
    );
    assert_eq!(
        style.grid_row_end,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "footer".to_string(),
            occurrence: Some(std::num::NonZeroI32::new(3).unwrap())
        })
    );
    let expected_span = GridPlacement::Span(GridSpanPlacement::Named {
        name: "main".to_string(),
        count: Some(std::num::NonZeroU16::new(2).unwrap()),
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
        GridPlacement::Line(GridLinePlacement::Named {
            name: "header".to_string(),
            occurrence: None,
        })
    );
    assert_eq!(
        style.grid_column_start,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "main".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        style.grid_column_end,
        GridPlacement::Span(GridSpanPlacement::Named {
            name: "rail".to_string(),
            count: Some(std::num::NonZeroU16::new(2).unwrap())
        })
    );
}

#[tokio::test]
async fn zero_grid_placements_are_rejected_before_the_computed_value() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("grid-row-start: 2; grid-column-end: span 2"),
    );
    let expected_row_start = style.grid_row_start.clone();
    let expected_column_end = style.grid_column_end.clone();

    apply_declarations(
        &mut style,
        &parse_declarations("grid-row-start: 0; grid-column-end: span 0"),
    );

    assert_eq!(style.grid_row_start, expected_row_start);
    assert_eq!(style.grid_column_end, expected_column_end);
}

#[tokio::test]
async fn grid_lanes_normal_cannot_carry_axis_modifiers() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("grid-lanes-direction: row track-reverse"),
    );
    assert_eq!(
        style.grid_lanes_direction,
        GridLanesDirection::Axis {
            axis: GridLanesAxis::Row,
            track_reverse: true,
            fill_reverse: false,
        }
    );

    apply_declarations(
        &mut style,
        &parse_declarations("grid-lanes-direction: normal track-reverse"),
    );
    assert_eq!(
        style.grid_lanes_direction,
        GridLanesDirection::Axis {
            axis: GridLanesAxis::Row,
            track_reverse: true,
            fill_reverse: false,
        }
    );

    apply_declarations(
        &mut style,
        &parse_declarations("grid-lanes-direction: normal"),
    );
    assert_eq!(style.grid_lanes_direction, GridLanesDirection::Normal);
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
        GridPlacement::Line(GridLinePlacement::Named {
            name: "header".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        row_style.grid_row_end,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "footer".to_string(),
            occurrence: None
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
        GridPlacement::Line(GridLinePlacement::Named {
            name: "card".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        area_style.grid_row_end,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "card".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        area_style.grid_column_start,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "main".to_string(),
            occurrence: None
        })
    );
    assert_eq!(
        area_style.grid_column_end,
        GridPlacement::Line(GridLinePlacement::Named {
            name: "main".to_string(),
            occurrence: None
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
    assert_eq!(style.grid_auto_rows.as_slice(), [GridTrackSize::AUTO]);
    assert_eq!(style.grid_auto_columns.as_slice(), [GridTrackSize::AUTO]);
}

#[tokio::test]
async fn parses_grid_shorthand_auto_flow_forms() {
    let declarations = parse_declarations("grid: auto-flow dense 7pt 8pt / [main] 1fr");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.grid_template_rows, GridTrackList::None);
    assert_eq!(style.grid_auto_flow, GridAutoFlow::RowDense);
    assert_eq!(style.grid_auto_rows.len(), 2);
    assert_eq!(style.grid_auto_columns.as_slice(), [GridTrackSize::AUTO]);
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
    assert_eq!(style.grid_auto_rows.as_slice(), [GridTrackSize::AUTO]);
    assert_eq!(style.grid_auto_columns.len(), 1);
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
async fn parses_text_combine_upright_values() {
    let mut style = default_style_for_tag("span");
    apply_declarations(
        &mut style,
        &parse_declarations("text-combine-upright: digits 3"),
    );
    assert_eq!(style.text_combine_upright, TextCombineUpright::Digits(3));

    apply_declarations(&mut style, &parse_declarations("text-combine-upright: all"));
    assert_eq!(style.text_combine_upright, TextCombineUpright::All);

    apply_declarations(
        &mut style,
        &parse_declarations("text-combine-upright: digits 5"),
    );
    assert_eq!(style.text_combine_upright, TextCombineUpright::All);
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
async fn parses_text_spacing_trim_and_shorthand() {
    let mut style = default_style_for_tag("div");
    for (value, expected) in [
        ("space-all", TextSpacingTrim::SpaceAll),
        ("normal", TextSpacingTrim::Normal),
        ("space-first", TextSpacingTrim::SpaceFirst),
        ("trim-start", TextSpacingTrim::TrimStart),
        ("trim-both", TextSpacingTrim::TrimBoth),
        ("trim-all", TextSpacingTrim::TrimAll),
        ("auto", TextSpacingTrim::Auto),
    ] {
        apply_declarations(
            &mut style,
            &parse_declarations(&format!("text-spacing-trim: {value}")),
        );
        assert_eq!(style.text_spacing_trim, expected);
    }

    apply_declarations(
        &mut style,
        &parse_declarations("text-spacing-trim: trim-start trim-all"),
    );
    assert_eq!(style.text_spacing_trim, TextSpacingTrim::Auto);

    apply_declarations(
        &mut style,
        &parse_declarations("text-spacing: trim-all no-autospace"),
    );
    assert_eq!(style.text_spacing_trim, TextSpacingTrim::TrimAll);
    assert_eq!(style.text_autospace, TextAutospace::NONE);

    apply_declarations(
        &mut style,
        &parse_declarations("text-spacing: trim-start trim-all"),
    );
    assert_eq!(style.text_spacing_trim, TextSpacingTrim::TrimAll);

    apply_declarations(&mut style, &parse_declarations("text-spacing: none"));
    assert_eq!(style.text_spacing_trim, TextSpacingTrim::SpaceAll);
    assert_eq!(style.text_autospace, TextAutospace::NONE);
}

#[tokio::test]
async fn text_spacing_trim_inherits_and_honors_css_wide_keywords() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("text-spacing: trim-all no-autospace"),
        &[],
        None,
        &[],
    );
    let inherited = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[],
        Some(&parent),
        &[],
    );
    assert_eq!(inherited.text_spacing_trim, TextSpacingTrim::TrimAll);
    assert_eq!(inherited.text_autospace, TextAutospace::NONE);

    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { text-spacing: initial } q { text-spacing: inherit } em { text-spacing: unset }",
    ));
    let initial = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[],
    );
    assert_eq!(initial.text_spacing_trim, TextSpacingTrim::Normal);
    assert_eq!(initial.text_autospace, TextAutospace::NORMAL);
    for tag in ["q", "em"] {
        let style = style_for_element_with_signature(
            ElementSignature::new(tag, HashMap::new()),
            None,
            std::slice::from_ref(&stylesheet),
            Some(&parent),
            &[],
        );
        assert_eq!(style.text_spacing_trim, TextSpacingTrim::TrimAll);
        assert_eq!(style.text_autospace, TextAutospace::NONE);
    }
}

#[tokio::test]
async fn parses_inherited_word_space_transform_keyword_set() {
    let declarations = parse_declarations("word-space-transform: auto-phrase ideographic-space");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.word_space_transform.replacement,
        Some(WordSpaceReplacement::IdeographicSpace)
    );
    assert!(style.word_space_transform.auto_phrase);

    apply_declarations(
        &mut style,
        &parse_declarations("word-space-transform: none"),
    );
    assert_eq!(style.word_space_transform, WordSpaceTransform::NONE);
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
    assert!((style.text_indent.amount.percentage_coefficient_or_zero() - 0.25).abs() < 0.001);
    assert!(style.text_indent.hanging);
    assert!(style.text_indent.each_line);
}

#[tokio::test]
async fn text_indent_mixed_calc_keeps_its_signed_fixed_component() {
    let declarations = parse_declarations("text-indent: calc(50% - 3px)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!((style.text_indent.amount.length_points() + 2.25).abs() < 0.001);
    assert!((style.text_indent.amount.percentage_coefficient_or_zero() - 0.5).abs() < 0.001);
}

#[test]
fn calc_absolute_length_matches_equivalent_plain_length() {
    let plain =
        parse_computed_length_percentage("500px", ROOT_FONT_SIZE_PT).expect("plain length parses");
    let calculated = parse_computed_length_percentage("calc(500px)", ROOT_FONT_SIZE_PT)
        .expect("calculated length parses");
    assert_eq!(
        plain.length_if_no_percent(),
        calculated.length_if_no_percent()
    );
}

#[tokio::test]
async fn margin_shorthand_keeps_mixed_length_percentage_edges() {
    let mut style = default_style_for_tag("p");
    apply_declarations(
        &mut style,
        &parse_declarations("margin: calc(10px + 1%) 0 0 0"),
    );

    let margin = &style.box_values.margin;
    let ComputedLengthPercentageOrAuto::LengthPercentage(top) = &margin.top else {
        panic!("mixed margin must retain a typed length-percentage");
    };
    assert!((top.length_points() - 7.5).abs() < 0.001);
    assert!((top.percentage_coefficient_or_zero() - 0.01).abs() < 0.001);
    assert_eq!(margin.right.length_if_no_percent(), Some(0.0));
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
    assert!(!style.line_height_is_normal());
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
    assert!(!style.line_height_is_normal());
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
    assert!(!style.line_height_is_normal());
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
async fn parses_anchor_center_only_for_self_alignment() {
    let declarations = parse_declarations("place-self: anchor-center");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.align_self.keyword, SelfAlignmentKeyword::Center);
    assert_eq!(style.align_self.safety, AlignmentSafety::Default);
    assert_eq!(
        style.justify_self,
        JustifySelf::new(SelfAlignmentKeyword::Center)
    );

    let declarations =
        parse_declarations("align-self: safe anchor-center; justify-self: anchor-center");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.align_self.keyword, SelfAlignmentKeyword::Center);
    assert_eq!(style.align_self.safety, AlignmentSafety::Safe);
    assert_eq!(
        style.justify_self,
        JustifySelf::new(SelfAlignmentKeyword::Center)
    );

    let declarations = parse_declarations(
        "align-self: end; justify-self: start; align-items: end; justify-items: start; \
         align-items: anchor-center; justify-items: anchor-center; place-items: anchor-center",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.align_items,
        AlignItems::new(SelfAlignmentKeyword::End)
    );
    assert_eq!(
        style.justify_items,
        JustifyItems::new(SelfAlignmentKeyword::Start)
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
async fn parses_balanced_flex_wrap_with_cross_axis_direction() {
    assert_eq!(
        default_style_for_tag("div").flex_line_count,
        FlexLineCount::ONE,
        "the initial flex-line-count is the CSS minimum of one"
    );

    let declarations = parse_declarations("flex-wrap: balance wrap-reverse");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.flex_wrap, FlexWrap::BalanceReverse);
    assert!(style.flex_wrap.wraps());
    assert!(style.flex_wrap.reverses_cross_axis());
    assert!(style.flex_wrap.balances_lines());

    let declarations = parse_declarations("flex-flow: column balance");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.flex_direction, FlexDirection::Column);
    assert_eq!(style.flex_wrap, FlexWrap::Balance);

    let declarations = parse_declarations("flex-line-count: 4");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.flex_line_count,
        FlexLineCount::new(NonZeroUsize::new(4).expect("positive line count"))
    );
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

        let (actual_auto, actual_ratio) = style.aspect_ratio.specified();
        assert_eq!(actual_auto, expected_auto, "declaration: {declaration}");
        match (actual_ratio, expected_ratio) {
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
        AspectRatio::from_ratio(1.0)
            .unwrap()
            .preferred_ratio(true, Some(2.5)),
        Some(1.0)
    );
    assert_eq!(
        AspectRatio::auto_with_ratio(1.5)
            .unwrap()
            .preferred_ratio(true, Some(2.5)),
        Some(2.5)
    );
    assert_eq!(
        AspectRatio::auto_with_ratio(1.5)
            .unwrap()
            .preferred_ratio(true, None),
        Some(1.5)
    );
    assert_eq!(
        AspectRatio::auto_with_ratio(1.5)
            .unwrap()
            .preferred_ratio(false, Some(2.5)),
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

        assert_eq!(
            style.aspect_ratio,
            AspectRatio::from_ratio(1.5).unwrap(),
            "invalid: {invalid}"
        );
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

    let declarations = parse_declarations("float: footnote");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.float, Float::Footnote);
}

#[tokio::test]
async fn parses_gcpm_footnote_display_and_policy() {
    let declarations = parse_declarations("footnote-display: compact; footnote-policy: line");
    let mut style = default_style_for_tag("span");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.footnote_display, FootnoteDisplay::Compact);
    assert_eq!(style.footnote_policy, FootnotePolicy::Line);
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

    let mut webkit_alias = default_style_for_tag("div");
    apply_declarations(
        &mut webkit_alias,
        &parse_declarations("-webkit-flex-basis: 50pt"),
    );
    assert_eq!(
        webkit_alias.flex_basis,
        flex_basis_length(ComputedLengthPercentage::from_points(50.0))
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
        line_height_value: ComputedLineHeight::from_points(17.6),
        ..ComputedStyle::initial()
    };
    let style = style_for_element_with_signature(
        ElementSignature::new("dl", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
        Some(&parent),
        &[],
    );

    assert_eq!(style.margin.top, 11.0);
    assert_eq!(style.margin.bottom, 11.0);

    let overridden = style_for_element_with_signature(
        ElementSignature::new("dl", HashMap::new()),
        Some("margin: 0"),
        &Stylesheets::borrowed(&[ua]),
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
    assert!(source.contains("@media (scripting) { noscript { display: none !important } }"));
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
async fn html5_ua_centers_table_headers_only_when_parent_alignment_is_initial() {
    let ua = html5_user_agent_stylesheet();
    let initial_parent = default_style_for_tag("tr");

    let ua_default = style_for_element_with_signature(
        ElementSignature::new("th", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
        Some(&initial_parent),
        &[],
    );
    assert_eq!(ua_default.text_align, TextAlign::Center);

    let non_initial_parent = ComputedStyle {
        text_align: TextAlign::End,
        ..default_style_for_tag("tr")
    };
    let inherited_end = style_for_element_with_signature(
        ElementSignature::new("th", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
        Some(&non_initial_parent),
        &[],
    );
    assert_eq!(inherited_end.text_align, TextAlign::End);

    let author = parse_stylesheet(&Css::from_string("th { text-align: inherit }"));
    let inherited = style_for_element_with_signature(
        ElementSignature::new("th", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &author]),
        Some(&initial_parent),
        &[],
    );
    assert_eq!(inherited.text_align, TextAlign::Start);
}

#[tokio::test]
async fn html5_ua_centers_table_captions_unless_author_css_overrides_it() {
    let ua = html5_user_agent_stylesheet();
    let parent = default_style_for_tag("table");

    let ua_default = style_for_element_with_signature(
        ElementSignature::new("caption", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
        Some(&parent),
        &[],
    );
    assert_eq!(ua_default.text_align, TextAlign::Center);

    let author = parse_stylesheet(&Css::from_string("caption { text-align: end }"));
    let overridden = style_for_element_with_signature(
        ElementSignature::new("caption", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &author]),
        Some(&parent),
        &[],
    );
    assert_eq!(overridden.text_align, TextAlign::End);
}

#[tokio::test]
async fn css_text_ua_preformatted_elements_disable_autospace() {
    let ua = html5_user_agent_stylesheet();
    let parent = default_style_for_tag("body");

    for tag in [
        "code",
        "kbd",
        "listing",
        "plaintext",
        "pre",
        "samp",
        "tt",
        "xmp",
    ] {
        let style = style_for_element_with_signature(
            ElementSignature::new(tag, HashMap::new()),
            None,
            &Stylesheets::borrowed(&[ua]),
            Some(&parent),
            &[],
        );
        assert_eq!(style.text_autospace, TextAutospace::NONE, "{tag}");
    }
}

#[tokio::test]
async fn html5_ua_pre_dir_auto_uses_plaintext_bidi() {
    let ua = html5_user_agent_stylesheet();
    let mut attributes = HashMap::new();
    attributes.insert("dir".to_string(), "auto".to_string());
    let parent = default_style_for_tag("body");
    let style = style_for_element_with_signature(
        ElementSignature::new("pre", attributes),
        None,
        &Stylesheets::borrowed(&[ua]),
        Some(&parent),
        &[],
    );

    assert_eq!(style.unicode_bidi, UnicodeBidi::Plaintext);
}

#[tokio::test]
async fn html5_ua_bdi_is_bidi_isolated() {
    let ua = html5_user_agent_stylesheet();
    let parent = default_style_for_tag("body");
    let style = style_for_element_with_signature(
        ElementSignature::new("bdi", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
        Some(&parent),
        &[],
    );

    assert_eq!(style.unicode_bidi, UnicodeBidi::Isolate);
}

#[tokio::test]
async fn author_rules_override_user_agent_stylesheet_at_equal_specificity() {
    let ua = html5_user_agent_stylesheet();
    let author = parse_stylesheet(&Css::from_string("p { margin: 0; font-size: 10pt }"));
    let parent = default_style_for_tag("body");
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &author]),
        Some(&parent),
        &[],
    );

    assert_eq!(style.margin, Edges::ZERO);
    assert_eq!(style.font_size, 10.0);
}

#[tokio::test]
async fn html_list_type_presentational_hints_follow_element_and_case_rules() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let stylesheets = [ua, &hints];
    let stylesheets = Stylesheets::borrowed(&stylesheets);
    let style_for_type = |tag: &str, value: &str| {
        style_for_element_with_signature(
            ElementSignature::new(
                tag,
                HashMap::from([("type".to_string(), value.to_string())]),
            ),
            None,
            &stylesheets,
            None,
            &[],
        )
    };

    for value in [
        "decimal",
        "DECIMAL",
        "1",
        "lower-alpha",
        "LOWER-ALPHA",
        "a",
        "upper-alpha",
        "UPPER-ALPHA",
        "A",
        "lower-roman",
        "LOWER-ROMAN",
        "i",
        "upper-roman",
        "UPPER-ROMAN",
        "I",
        "disk",
        "DISK",
        "x",
    ] {
        assert_eq!(
            style_for_type("ul", value).list_style_type,
            ListStyleType::Disc,
            "ul[type={value}] must keep the unordered-list default"
        );
    }

    for (value, expected) in [
        ("1", ListStyleType::Decimal),
        ("a", ListStyleType::Named("lower-alpha".to_string())),
        ("A", ListStyleType::Named("upper-alpha".to_string())),
        ("i", ListStyleType::Named("lower-roman".to_string())),
        ("I", ListStyleType::Named("upper-roman".to_string())),
    ] {
        for tag in ["ol", "li"] {
            assert_eq!(
                style_for_type(tag, value).list_style_type,
                expected,
                "{tag}[type={value}]"
            );
        }
    }

    for (value, expected) in [
        ("none", ListStyleType::None),
        ("NONE", ListStyleType::None),
        ("disc", ListStyleType::Disc),
        ("DISC", ListStyleType::Disc),
        ("circle", ListStyleType::Circle),
        ("CIRCLE", ListStyleType::Circle),
        ("square", ListStyleType::Square),
        ("SQUARE", ListStyleType::Square),
    ] {
        for tag in ["ul", "li"] {
            assert_eq!(
                style_for_type(tag, value).list_style_type,
                expected,
                "{tag}[type={value}]"
            );
        }
    }
}

#[tokio::test]
async fn author_overflow_visible_overrides_the_replaced_element_ua_clip() {
    let ua = html5_user_agent_stylesheet();
    let author = parse_stylesheet(&Css::from_string("img.default { overflow: visible }"));
    let parent = default_style_for_tag("body");
    let style = style_for_element_with_signature(
        ElementSignature::new("img", HashMap::from([("class".into(), "default".into())])),
        None,
        &Stylesheets::borrowed(&[ua, &author]),
        Some(&parent),
        &[],
    );

    assert_eq!(style.overflow_x, Overflow::Visible);
    assert_eq!(style.overflow_y, Overflow::Visible);
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
        &Stylesheets::borrowed(&[ua, &hints]),
        Some(&parent),
        &[],
    );
    let overridden = style_for_element_with_signature(
        ElementSignature::new("td", attrs),
        None,
        &Stylesheets::borrowed(&[ua, &hints, &author]),
        Some(&parent),
        &[],
    );

    assert_eq!(
        hinted.vertical_align.table_cell_align,
        TableCellVerticalAlignKeyword::Bottom
    );
    assert_eq!(
        overridden.vertical_align.table_cell_align,
        TableCellVerticalAlignKeyword::Top
    );
}

#[tokio::test]
async fn replaced_element_dimension_hints_map_to_css_and_respect_author_css() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let author = parse_stylesheet(&Css::from_string("img { width: 40px }"));
    let attrs = HashMap::from([
        ("width".to_string(), "100".to_string()),
        ("height".to_string(), "50".to_string()),
    ]);

    let hinted = style_for_element_with_signature(
        ElementSignature::new("img", attrs.clone()),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    let overridden = style_for_element_with_signature(
        ElementSignature::new("img", attrs),
        None,
        &Stylesheets::borrowed(&[ua, &hints, &author]),
        None,
        &[],
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = hinted.box_values.width else {
        panic!("img width attribute should map to CSS width");
    };
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) =
        hinted.box_values.height.value().clone()
    else {
        panic!("img height attribute should map to CSS height");
    };
    assert!((width.length_points() - 100.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert!((height.length_points() - 50.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert_eq!(
        hinted.aspect_ratio,
        AspectRatio::auto_with_ratio(2.0).unwrap()
    );
    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = overridden.box_values.width
    else {
        panic!("author CSS width should remain definite");
    };
    assert!((width.length_points() - 40.0 * CSS_PX_TO_PT).abs() < 0.001);
}

#[tokio::test]
async fn image_button_uses_replaced_element_hints_without_text_control_defaults() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let automatic_style = style_for_element_with_signature(
        ElementSignature::new(
            "input",
            HashMap::from([("type".to_string(), "IMAGE".to_string())]),
        ),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    assert!(automatic_style.box_values.width.is_auto());
    assert!(automatic_style.box_values.height.value().is_auto());
    assert_eq!(automatic_style.padding, Edges::ZERO);
    assert_eq!(automatic_style.border_styles, BorderStyles::NONE);

    let image_button = ElementSignature::new(
        "input",
        HashMap::from([
            ("type".to_string(), "IMAGE".to_string()),
            ("width".to_string(), "100".to_string()),
            ("height".to_string(), "50".to_string()),
        ]),
    );
    let style = style_for_element_with_signature(
        image_button,
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("image button width attribute should map to CSS width");
    };
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) =
        style.box_values.height.value().clone()
    else {
        panic!("image button height attribute should map to CSS height");
    };
    assert!((width.length_points() - 100.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert!((height.length_points() - 50.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert_eq!(style.padding, Edges::ZERO);
    assert_eq!(style.border_styles, BorderStyles::NONE);
    assert_eq!(
        style.aspect_ratio,
        AspectRatio::auto_with_ratio(2.0).unwrap()
    );
}

#[tokio::test]
async fn replaced_element_dimension_hints_apply_to_xhtml_namespace_images() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let image = ElementSignature::new(
        "img",
        HashMap::from([
            ("width".to_string(), "100".to_string()),
            ("height".to_string(), "50".to_string()),
        ]),
    )
    .with_document_is_html(false)
    .with_namespace("http://www.w3.org/1999/xhtml", Vec::new());

    let style = style_for_element_with_signature(
        image,
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("XHTML img width attribute should map to CSS width");
    };
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) =
        style.box_values.height.value().clone()
    else {
        panic!("XHTML img height attribute should map to CSS height");
    };
    assert!((width.length_points() - 100.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert!((height.length_points() - 50.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert_eq!(
        style.aspect_ratio,
        AspectRatio::auto_with_ratio(2.0).unwrap()
    );
}

#[tokio::test]
async fn replaced_element_dimension_hints_preserve_percentages_and_reject_invalid_ratios() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let percent = style_for_element_with_signature(
        ElementSignature::new(
            "img",
            HashMap::from([
                ("width".to_string(), "50%".to_string()),
                ("height".to_string(), "invalid".to_string()),
            ]),
        ),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    let invalid = style_for_element_with_signature(
        ElementSignature::new(
            "img",
            HashMap::from([
                ("width".to_string(), "0".to_string()),
                ("height".to_string(), "40".to_string()),
            ]),
        ),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = percent.box_values.width else {
        panic!("percentage width should be preserved");
    };
    assert_eq!(width.length_points(), 0.0);
    assert!((width.percentage_coefficient_or_zero() - 0.5).abs() < 0.001);
    assert!(percent.box_values.height.is_auto());
    assert_eq!(percent.aspect_ratio, AspectRatio::AUTO);
    assert_eq!(invalid.aspect_ratio, AspectRatio::AUTO);
}

#[tokio::test]
async fn body_margin_presentational_hints_follow_html_source_precedence() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let stylesheets = Stylesheets::for_document(ua, None, std::slice::from_ref(&hints))
        .with_html_container_frame_body_margins(Some(HtmlContainerFrameBodyMargins {
            horizontal: Some(30),
            vertical: Some(40),
        }));
    let mut attrs = HashMap::new();
    attrs.insert("marginwidth".to_string(), "100".to_string());
    attrs.insert("leftmargin".to_string(), "60".to_string());
    attrs.insert("marginheight".to_string(), "120".to_string());
    attrs.insert("topmargin".to_string(), "80".to_string());

    let style = style_for_element_with_signature(
        ElementSignature::new("body", attrs),
        None,
        &stylesheets,
        None,
        &[],
    );

    assert_eq!(style.margin.left, 75.0);
    assert_eq!(style.margin.right, 75.0);
    assert_eq!(style.margin.top, 90.0);
    assert_eq!(style.margin.bottom, 90.0);
}

#[tokio::test]
async fn invalid_body_margin_source_uses_ua_default_without_frame_fallback() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let stylesheets = Stylesheets::for_document(ua, None, std::slice::from_ref(&hints))
        .with_html_container_frame_body_margins(Some(HtmlContainerFrameBodyMargins {
            horizontal: Some(100),
            vertical: Some(60),
        }));
    let mut attrs = HashMap::new();
    attrs.insert("marginwidth".to_string(), "invalid".to_string());
    attrs.insert("leftmargin".to_string(), "20".to_string());

    let style = style_for_element_with_signature(
        ElementSignature::new("body", attrs),
        None,
        &stylesheets,
        None,
        &[],
    );

    // The invalid `marginwidth` blocks both `leftmargin` and the container
    // frame. The UA's `body { margin: 8px }` remains in effect.
    assert_eq!(style.margin.left, 6.0);
    assert_eq!(style.margin.right, 6.0);
    assert_eq!(style.margin.top, 45.0);
    assert_eq!(style.margin.bottom, 45.0);
}

#[tokio::test]
async fn author_and_inline_css_override_body_margin_presentational_hints() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let author = parse_stylesheet(&Css::from_string("body { margin-left: 20px }"));
    let document_stylesheets = [hints, author];
    let stylesheets = Stylesheets::for_document(ua, None, &document_stylesheets)
        .with_html_container_frame_body_margins(Some(HtmlContainerFrameBodyMargins {
            horizontal: Some(100),
            vertical: None,
        }));

    let style = style_for_element_with_signature(
        ElementSignature::new("body", HashMap::new()),
        Some("margin-right: 30px"),
        &stylesheets,
        None,
        &[],
    );

    assert_eq!(style.margin.left, 15.0);
    assert_eq!(style.margin.right, 22.5);
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
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );

    let hinted = style_for_element_with_signature(
        ElementSignature::new("thead", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        Some(&table_style),
        std::slice::from_ref(&table_signature),
    );
    let overridden = style_for_element_with_signature(
        ElementSignature::new("thead", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &hints, &author]),
        Some(&table_style),
        &[table_signature],
    );

    assert_eq!(hinted.border_colors.bottom, CssColor::new(128, 128, 128));
    assert_eq!(hinted.border_styles.bottom, BorderStyle::Solid);
    assert_eq!(hinted.border_widths.bottom, 0.75);
    assert_eq!(overridden.border_colors.bottom, CssColor::new(0, 0, 255));
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
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    let percent = style_for_element_with_signature(
        ElementSignature::new("hr", percent_attrs),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    let invalid = style_for_element_with_signature(
        ElementSignature::new("hr", invalid_attrs),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(px_width) = px.box_values.width else {
        panic!("width=100 should map to a length");
    };
    assert!((px_width.length_points() - 100.0 * CSS_PX_TO_PT).abs() < 0.001);
    assert_eq!(px_width.percentage_coefficient_or_zero(), 0.0);

    let ComputedLengthPercentageOrAuto::LengthPercentage(percent_width) = percent.box_values.width
    else {
        panic!("width=50% should map to a percentage");
    };
    assert_eq!(percent_width.length_points(), 0.0);
    assert!((percent_width.percentage_coefficient_or_zero() - 0.5).abs() < 0.001);

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
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    let size_eight = style_for_element_with_signature(
        ElementSignature::new("hr", size_eight_attrs),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    let solid_size = style_for_element_with_signature(
        ElementSignature::new("hr", solid_size_attrs),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );

    assert_eq!(size_one.border_widths.bottom, 0.0);
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) =
        size_eight.box_values.height.value()
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
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(255, 0, 0));
    assert_eq!(style.border_color, CssColor::new(255, 0, 0));
    assert_eq!(style.border_colors.top, CssColor::new(255, 0, 0));
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
        &Stylesheets::borrowed(&[ua, &hints, &author]),
        None,
        &[],
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("author width should win");
    };
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = style.box_values.height.value()
    else {
        panic!("author height should win");
    };
    assert_eq!(width.length_points(), 25.0);
    assert_eq!(height.length_points(), 2.0);
    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(style.border_widths.top, 1.0);
}

#[tokio::test]
async fn table_dynamic_presentational_hints_use_css_values_and_author_precedence() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let author = parse_stylesheet(&Css::from_string(
        "table { width: 25pt; border-spacing: 2pt; border-color: blue }",
    ));
    let mut attrs = HashMap::new();
    attrs.insert("width".to_string(), "150%".to_string());
    attrs.insert("height".to_string(), "40".to_string());
    attrs.insert("cellspacing".to_string(), "8".to_string());
    attrs.insert("bordercolor".to_string(), "red".to_string());

    let hinted = style_for_element_with_signature(
        ElementSignature::new("table", attrs.clone()),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    let overridden = style_for_element_with_signature(
        ElementSignature::new("table", attrs),
        None,
        &Stylesheets::borrowed(&[ua, &hints, &author]),
        None,
        &[],
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = hinted.box_values.width else {
        panic!("table width hint should map to a CSS percentage");
    };
    assert!((width.percentage_coefficient_or_zero() - 1.5).abs() < 0.001);
    assert_eq!(
        hinted.border_spacing.horizontal.length_points(),
        8.0 * CSS_PX_TO_PT
    );
    assert_eq!(hinted.border_colors.top, CssColor::new(255, 0, 0));
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = hinted.box_values.height.value()
    else {
        panic!("table height hint should map to a CSS length");
    };
    assert!((height.length_points() - 40.0 * CSS_PX_TO_PT).abs() < 0.001);

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = overridden.box_values.width
    else {
        panic!("author table width should win");
    };
    assert_eq!(width.length_points(), 25.0);
    assert_eq!(overridden.border_spacing.horizontal.length_points(), 2.0);
    assert_eq!(overridden.border_colors.top, CssColor::new(0, 0, 255));

    let mut column_attrs = HashMap::new();
    column_attrs.insert("width".to_string(), "40".to_string());
    let column = style_for_element_with_signature(
        ElementSignature::new("col", column_attrs),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = column.box_values.width else {
        panic!("column width hint should map to a CSS length");
    };
    assert!((width.length_points() - 40.0 * CSS_PX_TO_PT).abs() < 0.001);
}

#[tokio::test]
async fn table_cellpadding_is_a_cascaded_hint_from_the_nearest_table() {
    let ua = html5_user_agent_stylesheet();
    let hints = html5_presentational_hints_stylesheet();
    let author = parse_stylesheet(&Css::from_string("td { padding: 3pt }"));
    let mut table_attrs = HashMap::new();
    table_attrs.insert("cellpadding".to_string(), "4".to_string());
    let table = ElementSignature::new("table", table_attrs);
    let row = ElementSignature::new("tr", HashMap::new());
    let table_style = style_for_element_with_signature(
        table.clone(),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        None,
        &[],
    );
    let row_style = style_for_element_with_signature(
        row.clone(),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        Some(&table_style),
        std::slice::from_ref(&table),
    );
    let hinted = style_for_element_with_signature(
        ElementSignature::new("td", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &hints]),
        Some(&row_style),
        &[table.clone(), row.clone()],
    );
    let overridden = style_for_element_with_signature(
        ElementSignature::new("td", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &hints, &author]),
        Some(&row_style),
        &[table, row],
    );

    assert_eq!(hinted.padding.top, 4.0 * CSS_PX_TO_PT);
    assert_eq!(hinted.padding.right, 4.0 * CSS_PX_TO_PT);
    assert_eq!(overridden.padding.top, 3.0);
    assert_eq!(overridden.padding.right, 3.0);
}

#[tokio::test]
async fn table_default_border_color_tracks_the_cascaded_current_color() {
    let ua = html5_user_agent_stylesheet();
    let author = parse_stylesheet(&Css::from_string(
        "* { color: teal; border-width: 2px } table { border-style: solid }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("table", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &author]),
        None,
        &[],
    );

    assert_eq!(
        style.border_colors.top,
        CssColorOrCurrentColor::CurrentColor
    );
    assert_eq!(
        style.border_colors.right,
        CssColorOrCurrentColor::CurrentColor
    );
    assert_eq!(
        style.border_colors.resolve(style.color).top,
        CssColor::new(0, 128, 128)
    );
}

#[tokio::test]
async fn ua_stylesheet_applies_inherited_semantic_font_defaults_once() {
    let ua = html5_user_agent_stylesheet();
    let parent = ComputedStyle {
        font_size: 12.0,
        line_height: 14.4,
        line_height_value: ComputedLineHeight::Normal,
        ..default_style_for_tag("body")
    };

    let sub = style_for_element_with_signature(
        ElementSignature::new("sub", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
        Some(&parent),
        &[],
    );
    let pre = style_for_element_with_signature(
        ElementSignature::new("pre", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
        Some(&parent),
        &[],
    );

    assert_eq!(sub.font_size, 10.0);
    assert_eq!(sub.vertical_align.baseline_shift, BaselineShift::Sub);
    assert!(sub.line_height_is_normal());
    assert_eq!(pre.font_family, FontFamily::Monospace);
    assert_eq!(pre.white_space, WhiteSpace::Pre);
}

#[test]
fn text_wrap_longhands_override_the_legacy_white_space_wrap_component() {
    let mut style = default_style_for_tag("div");
    let declarations = parse_declarations(
        "white-space: pre; text-wrap: wrap balance; text-wrap-mode: nowrap; text-wrap-style: auto",
    );
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.white_space, WhiteSpace::Pre);
    assert_eq!(style.text_wrap_mode, TextWrapMode::NoWrap);
    assert_eq!(style.text_wrap_style, TextWrapStyle::Auto);
    assert!(!style.allows_soft_wrap());

    let declarations = parse_declarations("text-wrap: wrap balance");
    apply_declarations(&mut style, &declarations);
    assert!(style.allows_soft_wrap());
    assert_eq!(style.text_wrap_style, TextWrapStyle::Balance);

    let declarations = parse_declarations("text-wrap-style: stable");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.text_wrap_style, TextWrapStyle::Stable);

    let declarations = parse_declarations("line-clamp: 2");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.max_lines,
        MaxLines::Lines(std::num::NonZeroUsize::new(2).unwrap())
    );
    assert_eq!(style.block_ellipsis, BlockEllipsis::Auto);
    assert_eq!(style.continue_, Continue::Collapse);
}

#[test]
fn legacy_webkit_line_clamp_resolves_after_display_and_orientation_cascade() {
    let mut active = default_style_for_tag("div");
    apply_declarations(
        &mut active,
        &parse_declarations(
            "-webkit-line-clamp: 3; display: -webkit-inline-box; -webkit-box-orient: vertical",
        ),
    );
    assert_eq!(active.legacy_webkit_box, LegacyWebkitBox::Inline);
    assert_eq!(active.webkit_box_orient, WebkitBoxOrient::Vertical);
    assert_eq!(active.display, Display::INLINE_BLOCK);
    assert_eq!(
        active.max_lines,
        MaxLines::Lines(std::num::NonZeroUsize::new(3).unwrap())
    );
    assert_eq!(active.block_ellipsis, BlockEllipsis::Auto);
    assert_eq!(active.continue_, Continue::WebkitLegacy);

    let mut missing_orientation = default_style_for_tag("div");
    apply_declarations(
        &mut missing_orientation,
        &parse_declarations("-webkit-line-clamp: 3; display: -webkit-box"),
    );
    assert_eq!(missing_orientation.display, Display::FLEX);
    assert_eq!(
        missing_orientation.max_lines,
        MaxLines::Lines(std::num::NonZeroUsize::new(3).unwrap())
    );
    // Inactive legacy compatibility is a used-value decision.  The
    // independently cascaded longhand remains observable as authored.
    assert_eq!(missing_orientation.continue_, Continue::WebkitLegacy);
    assert!(matches!(
        missing_orientation.used_continuation(),
        UsedContinuation::Ordinary
    ));

    let mut missing_legacy_display = default_style_for_tag("div");
    apply_declarations(
        &mut missing_legacy_display,
        &parse_declarations("-webkit-box-orient: vertical; -webkit-line-clamp: 3"),
    );
    assert_eq!(
        missing_legacy_display.max_lines,
        MaxLines::Lines(std::num::NonZeroUsize::new(3).unwrap())
    );
    assert_eq!(missing_legacy_display.continue_, Continue::WebkitLegacy);
    assert!(matches!(
        missing_legacy_display.used_continuation(),
        UsedContinuation::Ordinary
    ));

    let mut reset_to_none = default_style_for_tag("div");
    apply_declarations(
        &mut reset_to_none,
        &parse_declarations(
            "display: -webkit-box; -webkit-box-orient: vertical; \
             -webkit-line-clamp: 3; -webkit-line-clamp: none",
        ),
    );
    assert_eq!(reset_to_none.display, Display::FLEX);
    assert_eq!(reset_to_none.max_lines, MaxLines::None);
    assert_eq!(reset_to_none.block_ellipsis, BlockEllipsis::Auto);
    assert_eq!(reset_to_none.continue_, Continue::Auto);

    let mut unprefixed = default_style_for_tag("div");
    apply_declarations(
        &mut unprefixed,
        &parse_declarations("display: -webkit-box; -webkit-box-orient: vertical; line-clamp: 2"),
    );
    assert_eq!(
        unprefixed.display,
        Display::new(DisplayOuter::Block, DisplayInner::FlowRoot)
    );
    assert_eq!(
        unprefixed.max_lines,
        MaxLines::Lines(std::num::NonZeroUsize::new(2).unwrap())
    );
    assert_eq!(unprefixed.continue_, Continue::Collapse);
}

#[test]
fn line_clamp_longhands_cascade_independently_and_preserve_marker_inheritance() {
    let mut parent = default_style_for_tag("div");
    apply_declarations(
        &mut parent,
        &parse_declarations("line-clamp: 3 \"continued\\A page\"; max-lines: 2; continue: discard"),
    );
    assert_eq!(
        parent.max_lines,
        MaxLines::Lines(std::num::NonZeroUsize::new(2).unwrap())
    );
    assert_eq!(parent.continue_, Continue::Discard);
    assert_eq!(
        parent.block_ellipsis,
        BlockEllipsis::String(std::sync::Arc::from("continued page"))
    );

    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        &[],
        Some(&parent),
        &[],
    );
    assert_eq!(child.block_ellipsis, parent.block_ellipsis);
    assert_eq!(child.max_lines, MaxLines::None);
    assert_eq!(child.continue_, Continue::Auto);

    let mut reset = default_style_for_tag("div");
    apply_declarations(
        &mut reset,
        &parse_declarations("line-clamp: 2 \"more\"; line-clamp: none"),
    );
    assert_eq!(reset.max_lines, MaxLines::None);
    assert_eq!(reset.block_ellipsis, BlockEllipsis::NoEllipsis);
    assert_eq!(reset.continue_, Continue::Auto);
}

#[test]
fn positive_line_budget_cannot_represent_zero_available_slots() {
    let available = PositiveLineCount::from_rendered_slots(2).unwrap();
    assert!(PositiveLineCount::from_rendered_slots(0).is_none());
    assert_eq!(
        RemainingLineSlots::Available(available)
            .debit(PositiveLineCount::from_rendered_slots(2).unwrap()),
        RemainingLineSlots::Exhausted
    );
}

#[test]
fn discard_region_traversal_captures_only_its_first_typed_break() {
    assert!(DiscardRegionTraversal::default().first_break().is_none());

    let mut forced = DiscardRegionTraversal::default();
    forced.capture_forced_after_lines(ClampPoint::AtContainerStart);
    forced.capture_overflow(RegionOverflowPoint::after_direct_children(
        std::num::NonZeroUsize::new(2).unwrap(),
    ));
    assert!(matches!(
        forced.first_break(),
        Some(CapturedRegionBreak::ForcedAfterLines(
            ClampPoint::AtContainerStart
        ))
    ));

    let mut overflow = DiscardRegionTraversal::default();
    overflow.capture_overflow(RegionOverflowPoint::after_direct_children(
        std::num::NonZeroUsize::new(3).unwrap(),
    ));
    overflow.capture_forced_after_lines(ClampPoint::AtContainerStart);
    assert!(matches!(
        overflow.first_break(),
        Some(CapturedRegionBreak::Overflow(point)) if point.retained_direct_children().get() == 3
    ));
}

#[test]
fn only_renderable_block_ellipsis_values_can_create_a_marker() {
    assert!(BlockEllipsis::NoEllipsis.renderable().is_none());
    assert!(
        BlockEllipsis::String(std::sync::Arc::from(""))
            .renderable()
            .is_none()
    );
    assert_eq!(BlockEllipsis::Auto.renderable().unwrap().text(), "…");
    assert_eq!(
        BlockEllipsis::String(std::sync::Arc::from("more"))
            .renderable()
            .unwrap()
            .text(),
        "more"
    );

    let eligible_line = EligibleMarkerLine::terminal_inline_line(4);
    assert!(
        BlockEllipsisPlacement::at_terminal_inline_line(eligible_line, &BlockEllipsis::NoEllipsis,)
            .is_none()
    );
    assert!(
        BlockEllipsisPlacement::at_terminal_inline_line(
            eligible_line,
            &BlockEllipsis::String(std::sync::Arc::from("")),
        )
        .is_none()
    );
}

#[test]
fn automatic_line_clamp_keeps_a_graph_source_endpoint_across_reflow() {
    let marker = BlockEllipsis::Auto;
    let endpoint = InlineSourceEndpoint::at_graph_boundary(7, 13);
    let clamp = InlineLineClamp::Automatic(AutomaticLineClamp::after_measured_source_line(
        2, endpoint, &marker,
    ));

    // A balanced reflow may redistribute the source into different visual
    // lines, so the graph boundary—not this ordinal—is the stable cutoff.
    assert_eq!(clamp.inline_source_end(), Some(endpoint));
    assert!(clamp.is_terminal_line(2));
    assert!(clamp.excludes_line(3));
}

#[test]
fn font_relative_max_height_is_a_finite_automatic_clamp_constraint() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("font: 16px / 32px serif; max-height: 4lh; line-clamp: auto"),
    );
    assert!(matches!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(_)
    ));
    style.resolve_line_height_relative_lengths();
    assert_eq!(
        style.box_values.max_height.length_if_no_percent(),
        Some(96.0)
    );
    assert!(matches!(
        style.used_continuation(),
        UsedContinuation::LineClamp(LineClampContainer {
            cutoff: ClampPointRule::AutomaticBlockSize,
            ..
        })
    ));
}

#[test]
fn wrap_inside_parses_as_a_non_inherited_inline_box_property() {
    let mut parent = ComputedStyle::initial();
    apply_declarations(&mut parent, &parse_declarations("wrap-inside: avoid"));
    assert_eq!(parent.wrap_inside, WrapInside::Avoid);

    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        None,
        &[],
        Some(&parent),
        &[],
    );
    assert_eq!(child.wrap_inside, WrapInside::Auto);

    apply_declarations(&mut parent, &parse_declarations("wrap-inside: invalid"));
    assert_eq!(parent.wrap_inside, WrapInside::Avoid);
}

#[tokio::test]
async fn wrap_inside_structural_wbr_selector_targets_only_the_following_span() {
    let stylesheet = parse_stylesheet(&Css::from_string(".jp > wbr + span { wrap-inside: avoid }"));
    let siblings = vec![
        ElementSiblingSignature::new("wbr", HashMap::new()),
        ElementSiblingSignature::new("span", HashMap::new()),
        ElementSiblingSignature::new("span", HashMap::new()),
    ];
    let parent = ElementSignature::new(
        "span",
        HashMap::from([("class".to_string(), "jp".to_string())]),
    );
    let avoided = style_for_element_with_signature(
        ElementSignature::with_siblings("span", HashMap::new(), 1, siblings.clone()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        std::slice::from_ref(&parent),
    );
    let ordinary = style_for_element_with_signature(
        ElementSignature::with_siblings("span", HashMap::new(), 2, siblings),
        None,
        &[stylesheet],
        None,
        &[parent],
    );

    assert_eq!(avoided.wrap_inside, WrapInside::Avoid);
    assert_eq!(ordinary.wrap_inside, WrapInside::Auto);
}

#[test]
fn overflow_clip_margin_parses_visual_box_and_signed_length_in_any_order() {
    let mut style = ComputedStyle::initial();
    let declarations = parse_declarations("overflow-clip-margin: 10px content-box");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.overflow_clip_margin,
        OverflowClipMargin {
            reference_box: OverflowClipMarginBox::Content,
            offset: layout_pt(7.5),
        }
    );

    let declarations = parse_declarations("overflow-clip-margin: border-box -1px");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.overflow_clip_margin,
        OverflowClipMargin {
            reference_box: OverflowClipMarginBox::Border,
            offset: layout_pt(-0.75),
        }
    );

    let declarations = parse_declarations("overflow-clip-margin: calc(-2px - 1px) padding-box");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.overflow_clip_margin,
        OverflowClipMargin {
            reference_box: OverflowClipMarginBox::Padding,
            offset: layout_pt(-2.25),
        }
    );

    let declarations = parse_declarations("overflow-clip-margin: content-box 10%");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.overflow_clip_margin.reference_box,
        OverflowClipMarginBox::Padding
    );
    assert_eq!(style.overflow_clip_margin.offset, layout_pt(-2.25));
}

#[test]
fn scrollbar_gutter_and_width_preserve_their_computed_policies() {
    let mut style = ComputedStyle::initial();
    let declarations =
        parse_declarations("scrollbar-gutter: both-edges stable; scrollbar-width: none");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.scrollbar_gutter,
        ScrollbarGutter::Stable { both_edges: true }
    );
    assert_eq!(style.scrollbar_width, ScrollbarWidth::None);
}

#[test]
fn overflow_longhands_remain_on_their_physical_axes() {
    let declarations = parse_declarations("overflow-y: clip");
    let mut style = ComputedStyle::initial();
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.overflow_x, Overflow::Visible);
    assert_eq!(style.overflow_y, Overflow::Clip);
}

#[tokio::test]
async fn list_item_ua_default_has_no_margin() {
    let style = default_style_for_tag("li");

    assert!(style.display.is_list_item());
    assert_eq!(style.margin, Edges::ZERO);
}

#[tokio::test]
async fn supported_replaced_elements_default_to_content_box_clipping() {
    for tag in ["img", "iframe", "video", "embed", "object", "canvas"] {
        let style = default_style_for_tag(tag);
        assert_eq!(style.overflow_x, Overflow::Clip, "{tag}");
        assert_eq!(style.overflow_y, Overflow::Clip, "{tag}");
        assert_eq!(
            style.overflow_clip_margin.reference_box,
            OverflowClipMarginBox::Content,
            "{tag}"
        );
    }
}

#[tokio::test]
async fn embedded_svg_uses_the_svg_viewport_overflow_default() {
    let ua = html5_user_agent_stylesheet();
    let html = ElementSignature::new("html", HashMap::new());

    let embedded = style_for_element_with_signature(
        ElementSignature::new("svg", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
        None,
        std::slice::from_ref(&html),
    );
    assert_eq!(embedded.overflow_x, Overflow::Hidden);
    assert_eq!(embedded.overflow_y, Overflow::Hidden);

    let author = parse_stylesheet(&Css::from_string("svg { overflow: visible }"));
    let visible = style_for_element_with_signature(
        ElementSignature::new("svg", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua, &author]),
        None,
        std::slice::from_ref(&html),
    );
    assert_eq!(visible.overflow_x, Overflow::Visible);
    assert_eq!(visible.overflow_y, Overflow::Visible);

    // The element of a standalone SVG/XML document is the document root, so
    // SVG 2 leaves its initial `overflow: visible` value intact.
    let standalone = style_for_element_with_signature(
        ElementSignature::new("svg", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
        None,
        &[],
    );
    assert_eq!(standalone.overflow_x, Overflow::Visible);
    assert_eq!(standalone.overflow_y, Overflow::Visible);
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
    assert_eq!(style.border_color, CssColor::new(255, 0, 0));
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_styles.bottom, BorderStyle::None);
    assert_eq!(style.border_colors.top, CssColor::new(255, 0, 0));
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

    style.resolve_font_metric_lengths(layout_pt(5.0));

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
    assert_eq!(style.border_colors.top, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn inline_style_parses_side_specific_dotted_border_shorthand() {
    let style = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("border-top: 2pt dotted blue"),
        &Stylesheets::borrowed(&[html5_user_agent_stylesheet()]),
        None,
        &[],
    );

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Dotted);
    assert_eq!(style.border_colors.top, CssColor::new(0, 0, 255));
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
    assert_eq!(style.border_colors.top, CssColor::new(255, 0, 0));
    assert_eq!(style.border_colors.right, CssColor::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, CssColor::new(0, 0, 255));
    assert_eq!(
        style.border_colors.left.resolve(style.color),
        CssColor::BLACK
    );
}

#[tokio::test]
async fn parses_border_shorthand_color_functions_as_single_components() {
    let declarations = parse_declarations("border: 2pt solid rgb(255 0 0)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_colors.top, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn parses_border_current_color_against_computed_color() {
    let declarations = parse_declarations(
        "border: 2pt solid currentColor; border-left-color: rgb(0 0 255); color: green",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.border_colors.top,
        CssColorOrCurrentColor::CurrentColor
    );
    assert_eq!(
        style.border_colors.right,
        CssColorOrCurrentColor::CurrentColor
    );
    assert_eq!(
        style.border_colors.bottom,
        CssColorOrCurrentColor::CurrentColor
    );
    assert_eq!(style.border_colors.left, CssColor::new(0, 0, 255));
    let resolved = style.border_colors.resolve(style.color);
    assert_eq!(resolved.top, CssColor::new(0, 128, 0));
    assert_eq!(resolved.right, CssColor::new(0, 128, 0));
    assert_eq!(resolved.bottom, CssColor::new(0, 128, 0));
}

#[tokio::test]
async fn color_currentcolor_preserves_the_inherited_computed_color() {
    assert_eq!(
        crate::css::parse::declaration_operation("color", "currentcolor"),
        Some(("color".into(), "currentcolor".into()))
    );
    assert_eq!(
        crate::css::parse_color_from_currentcolor("currentcolor", CssColor::new(0, 128, 0)),
        Some(CssColor::new(0, 128, 0))
    );
    let ua = html5_user_agent_stylesheet();
    let author = parse_stylesheet(&Css::from_string("a { color: currentcolor }"));
    let parent = ComputedStyle {
        color: CssColor::new(0, 128, 0),
        ..default_style_for_tag("div")
    };
    let style = style_for_element_with_signature(
        ElementSignature::new("a", HashMap::from([("href".into(), "unvisited".into())])),
        None,
        &Stylesheets::borrowed(&[ua, &author]),
        Some(&parent),
        &[],
    );

    assert_eq!(style.color, parent.color);
}

#[tokio::test]
async fn parses_border_color_shorthand_with_rgb_functions() {
    let declarations = parse_declarations(
        "border-color: rgb(255 0 0) rgb(0 128 0) rgb(0 0 255) currentColor; color: black",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, CssColor::new(255, 0, 0));
    assert_eq!(style.border_colors.right, CssColor::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, CssColor::new(0, 0, 255));
    assert_eq!(
        style.border_colors.left.resolve(style.color),
        CssColor::BLACK
    );
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
    assert_eq!(style.border_colors.top, CssColor::new(255, 0, 0));
    assert_eq!(style.border_widths.bottom, 3.0);
    assert_eq!(style.border_styles.bottom, BorderStyle::Dashed);
    assert_eq!(style.border_colors.bottom, CssColor::new(0, 0, 255));
    assert_eq!(style.border_widths.left, 4.0);
    assert_eq!(style.border_styles.left, BorderStyle::Dotted);
    assert_eq!(style.border_colors.left, CssColor::new(0, 128, 0));
    assert_eq!(style.border_widths.right, 4.0);
    assert_eq!(style.border_styles.right, BorderStyle::Dotted);
    assert_eq!(style.border_colors.right, CssColor::BLACK);
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
    assert_eq!(style.border_colors.right, CssColor::new(255, 0, 0));
    assert_eq!(style.border_widths.left, 3.0);
    assert_eq!(style.border_styles.left, BorderStyle::Dashed);
    assert_eq!(style.border_colors.left, CssColor::new(0, 0, 255));
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
    assert_eq!(style.border_colors.right, CssColor::new(255, 0, 0));
    assert_eq!(style.border_widths.top, 3.0);
    assert_eq!(style.border_styles.top, BorderStyle::Dashed);
    assert_eq!(style.border_colors.top, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn maps_logical_borders_through_sideways_lr_inline_reversal() {
    let declarations = parse_declarations(
        "writing-mode: sideways-lr; direction: ltr; \
         border-block-start: 2pt solid red; \
         border-inline-start: 3pt dashed blue",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.writing_mode, WritingMode::SidewaysLr);
    assert_eq!(style.border_widths.left, 2.0);
    assert_eq!(style.border_styles.left, BorderStyle::Solid);
    assert_eq!(style.border_colors.left, CssColor::new(255, 0, 0));
    assert_eq!(style.border_widths.bottom, 3.0);
    assert_eq!(style.border_styles.bottom, BorderStyle::Dashed);
    assert_eq!(style.border_colors.bottom, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn writing_mode_preserves_each_modern_keyword() {
    let mut style = default_style_for_tag("div");
    for (keyword, expected) in [
        ("horizontal-tb", WritingMode::HorizontalTb),
        ("vertical-rl", WritingMode::VerticalRl),
        ("vertical-lr", WritingMode::VerticalLr),
        ("sideways-rl", WritingMode::SidewaysRl),
        ("sideways-lr", WritingMode::SidewaysLr),
    ] {
        apply_declarations(
            &mut style,
            &parse_declarations(&format!("writing-mode: {keyword}")),
        );
        assert_eq!(style.writing_mode, expected, "{keyword}");
    }
}

#[tokio::test]
async fn writing_mode_change_promotes_flow_to_flow_root() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("display: block; writing-mode: vertical-lr"),
        &[],
        None,
        &[],
    );
    let sideways_lr = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("display: block; writing-mode: sideways-lr"),
        &[],
        Some(&parent),
        &[],
    );
    assert_eq!(
        sideways_lr.display,
        Display::new(DisplayOuter::Block, DisplayInner::FlowRoot)
    );

    let mut vertical_rl_parent = parent.clone();
    vertical_rl_parent.writing_mode = WritingMode::VerticalRl;
    let sideways_rl = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("display: block; writing-mode: sideways-rl"),
        &[],
        Some(&vertical_rl_parent),
        &[],
    );
    assert_eq!(
        sideways_rl.display,
        Display::new(DisplayOuter::Block, DisplayInner::FlowRoot)
    );

    let matching = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("display: block; writing-mode: vertical-lr"),
        &[],
        Some(&parent),
        &[],
    );
    assert_eq!(matching.display, Display::BLOCK);
}

#[tokio::test]
async fn writing_mode_change_promotes_inline_flow_to_inline_flow_root() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("display: block"),
        &[],
        None,
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("display: inline; writing-mode: vertical-rl"),
        &[],
        Some(&parent),
        &[],
    );

    assert_eq!(child.display, Display::INLINE_BLOCK);
}

#[tokio::test]
async fn writing_mode_display_transform_skips_display_contents_parent() {
    let outer = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("display: block; writing-mode: vertical-lr"),
        &[],
        None,
        &[],
    );
    let contents = style_for_element_with_signature(
        ElementSignature::new("span", HashMap::new()),
        Some("display: contents; writing-mode: horizontal-tb"),
        &[],
        Some(&outer),
        &[],
    );
    let matching_outer = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("display: block; writing-mode: vertical-lr"),
        &[],
        Some(&contents),
        &[],
    );
    let different_from_outer = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("display: block; writing-mode: sideways-lr"),
        &[],
        Some(&contents),
        &[],
    );

    assert_eq!(matching_outer.display, Display::BLOCK);
    assert_eq!(
        different_from_outer.display,
        Display::new(DisplayOuter::Block, DisplayInner::FlowRoot)
    );
}

#[tokio::test]
async fn writing_mode_css_wide_keywords_restore_the_inherited_value_or_initial_value() {
    assert_eq!(
        ComputedStyle::initial().writing_mode,
        WritingMode::HorizontalTb
    );

    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("writing-mode: sideways-lr"),
        &[],
        None,
        &[],
    );
    for keyword in ["inherit", "unset"] {
        let stylesheet = parse_stylesheet(&Css::from_string(format!(
            "p {{ writing-mode: {keyword} }}"
        )));
        let child = style_for_element_with_signature(
            ElementSignature::new("p", HashMap::new()),
            None,
            &[stylesheet],
            Some(&parent),
            &[],
        );
        assert_eq!(child.writing_mode, WritingMode::SidewaysLr, "{keyword}");
    }

    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { writing-mode: sideways-rl; writing-mode: initial }",
    ));
    let child = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[],
    );
    assert_eq!(child.writing_mode, WritingMode::HorizontalTb);

    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme; \
         @layer base { p { writing-mode: sideways-rl } } \
         @layer theme { p { writing-mode: sideways-lr; writing-mode: revert-layer } }",
    ));
    let child = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(child.writing_mode, WritingMode::SidewaysRl);

    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { writing-mode: sideways-lr; writing-mode: revert }",
    ));
    let child = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(child.writing_mode, WritingMode::HorizontalTb);
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
    assert_eq!(style.border_colors.left, CssColor::new(255, 0, 0));
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
    assert_eq!(style.border_colors.right, CssColor::new(255, 0, 0));
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

    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .value
            .length_points(),
        1.0
    );
    assert_eq!(
        style.border_radius.top_left.vertical.value.length_points(),
        2.0
    );
    assert_eq!(
        style
            .border_radius
            .top_right
            .horizontal
            .value
            .length_points(),
        3.0
    );
    assert_eq!(
        style.border_radius.top_right.vertical.value.length_points(),
        3.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .horizontal
            .value
            .length_points(),
        4.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .vertical
            .value
            .length_points(),
        5.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .horizontal
            .value
            .length_points(),
        6.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .vertical
            .value
            .length_points(),
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

    assert_eq!(
        style
            .border_radius
            .top_right
            .horizontal
            .value
            .length_points(),
        1.0
    );
    assert_eq!(
        style.border_radius.top_right.vertical.value.length_points(),
        2.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .horizontal
            .value
            .length_points(),
        3.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .vertical
            .value
            .length_points(),
        3.0
    );
    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .value
            .length_points(),
        4.0
    );
    assert_eq!(
        style.border_radius.top_left.vertical.value.length_points(),
        5.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .horizontal
            .value
            .length_points(),
        6.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .vertical
            .value
            .length_points(),
        6.0
    );
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

    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .value
            .length_points(),
        7.0
    );
    assert_eq!(
        style.border_radius.top_left.vertical.value.length_points(),
        8.0
    );
}

#[tokio::test]
async fn parses_border_image_longhands() {
    let declarations = parse_declarations(
        "border-image-source: url(\"images/border.png\"); border-image-slice: 1 20% 3 40% fill; border-image-width: 2 auto 4pt 25%; border-image-outset: 1 2pt; border-image-repeat: stretch round",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style
            .border_image
            .source
            .as_image()
            .and_then(|source| match source.selected_image() {
                BackgroundImage::Url(url) => Some(url.href.as_str()),
                _ => None,
            }),
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
async fn border_image_source_accepts_generated_images() {
    let declarations =
        parse_declarations("border-image-source: linear-gradient(to top, blue, orange)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert!(matches!(
        style.border_image.source.as_image(),
        Some(BackgroundImage::LinearGradient(_))
    ));
}

#[tokio::test]
async fn invalid_image_set_is_a_valid_border_image_source() {
    let declarations = parse_declarations(
        "border-image-source: image-set(url(border.png) type(\"image/unknown\")); \
         border-image: 1 / 10px image-set(url(border.png) type(\"image/unknown\"))",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    style.border_image.source.select_image_set(1.0);

    assert_eq!(style.border_image.source, ComputedImage::Invalid);
    assert!(matches!(
        style.border_image.width.top,
        BorderImageWidthValue::LengthPercentage(_)
    ));
}

#[tokio::test]
async fn parses_border_image_shorthand_and_resets_omitted_longhands() {
    let declarations = parse_declarations(
        "border-image-width: 4; border-image-outset: 2; border-image: url(\"images/border.png\") 1 20% 3 40% fill / 2 auto 4pt 25% / 1 2pt stretch round",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style
            .border_image
            .source
            .as_image()
            .and_then(|source| match source.selected_image() {
                BackgroundImage::Url(url) => Some(url.href.as_str()),
                _ => None,
            }),
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

    style.resolve_font_metric_lengths(layout_pt(5.0));

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

    assert_eq!(
        style.border_colors.top.resolve(style.color),
        CssColor::BLACK
    );
    assert_eq!(style.border_styles.top, BorderStyle::None);
}

#[tokio::test]
async fn parses_border_radius_shorthand_lengths_and_percentages() {
    let declarations = parse_declarations("border-radius: 4pt 8pt / 10% 20%");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .value
            .length_points(),
        4.0
    );
    assert_eq!(
        style
            .border_radius
            .top_right
            .horizontal
            .value
            .length_points(),
        8.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .horizontal
            .value
            .length_points(),
        4.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .horizontal
            .value
            .length_points(),
        8.0
    );
    assert_eq!(
        style
            .border_radius
            .top_left
            .vertical
            .value
            .percentage_coefficient_or_zero(),
        0.1
    );
    assert_eq!(
        style
            .border_radius
            .top_right
            .vertical
            .value
            .percentage_coefficient_or_zero(),
        0.2
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .vertical
            .value
            .percentage_coefficient_or_zero(),
        0.1
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .vertical
            .value
            .percentage_coefficient_or_zero(),
        0.2
    );
}

#[tokio::test]
async fn border_radius_shorthand_preserves_a_zero_horizontal_radius() {
    let declarations = parse_declarations("border-radius: 0em / 5em");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    for radius in [
        style.border_radius.top_left,
        style.border_radius.top_right,
        style.border_radius.bottom_right,
        style.border_radius.bottom_left,
    ] {
        assert_eq!(radius.horizontal.value.length_points(), 0.0);
        // The initial CSS `medium` font size is 16px, i.e. 12pt at the
        // CSS reference pixel ratio. `5em` therefore resolves to 60pt.
        assert_eq!(radius.vertical.value.length_points(), 60.0);
    }
}

#[tokio::test]
async fn negative_calc_border_radius_is_clamped_at_used_value_time() {
    let declarations = parse_declarations("border-radius: 10pt; border-radius: calc(-10pt)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .resolve(PercentageBasis::definite(layout_pt(100.0))),
        layout_pt(0.0)
    );
}

#[tokio::test]
async fn ch_border_radius_preserves_font_metric_component_until_used_resolution() {
    let declarations = parse_declarations("border-radius: 2ch 1pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.border_radius.top_left.horizontal.value,
        ComputedLengthPercentage::from_ch(2.0)
    );
    assert_eq!(
        style.border_radius.top_right.horizontal.value,
        ComputedLengthPercentage::from_points(1.0)
    );

    style.resolve_font_metric_lengths(layout_pt(6.0));

    assert_eq!(
        style.border_radius.top_left.horizontal.value,
        ComputedLengthPercentage::from_points(12.0)
    );
    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .resolve(PercentageBasis::definite(layout_pt(100.0))),
        layout_pt(12.0)
    );
}

#[tokio::test]
async fn parses_border_radius_corner_longhands() {
    let declarations =
        parse_declarations("border-top-left-radius: 4pt 10%; border-bottom-right-radius: 8pt 12pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .value
            .length_points(),
        4.0
    );
    assert_eq!(
        style
            .border_radius
            .top_left
            .vertical
            .value
            .percentage_coefficient_or_zero(),
        0.1
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .horizontal
            .value
            .length_points(),
        8.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .vertical
            .value
            .length_points(),
        12.0
    );
}

#[tokio::test]
async fn border_side_radius_shorthands_expand_to_adjacent_corners() {
    let declarations = parse_declarations(
        "border-top-radius: 1pt 2pt / 3pt 4pt; border-left-radius: 5pt 6pt / 7pt 8pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .value
            .length_points(),
        5.0
    );
    assert_eq!(
        style.border_radius.top_left.vertical.value.length_points(),
        6.0
    );
    assert_eq!(
        style
            .border_radius
            .top_right
            .horizontal
            .value
            .length_points(),
        3.0
    );
    assert_eq!(
        style.border_radius.top_right.vertical.value.length_points(),
        4.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .horizontal
            .value
            .length_points(),
        7.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .vertical
            .value
            .length_points(),
        8.0
    );
}

#[tokio::test]
async fn logical_border_side_radius_shorthands_follow_writing_mode() {
    let declarations = parse_declarations(
        "border-block-start-radius: 1pt 2pt / 3pt 4pt; border-inline-end-radius: 5pt 6pt / 7pt 8pt",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .value
            .length_points(),
        1.0
    );
    assert_eq!(
        style.border_radius.top_left.vertical.value.length_points(),
        2.0
    );
    assert_eq!(
        style
            .border_radius
            .top_right
            .horizontal
            .value
            .length_points(),
        5.0
    );
    assert_eq!(
        style.border_radius.top_right.vertical.value.length_points(),
        6.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .horizontal
            .value
            .length_points(),
        7.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .vertical
            .value
            .length_points(),
        8.0
    );
}

#[test]
fn parses_polygon_clip_path_as_typed_length_percentages() {
    let declarations =
        parse_declarations("clip-path: polygon(0% 50%, calc(25% + 2pt) 100%, 100% 0%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let ClipPath::Polygon(points) = style.clip_path else {
        panic!("expected a parsed polygon clip path");
    };
    assert_eq!(points.len(), 3);
    assert_eq!(points[0].x.percentage_coefficient(), Some(0.0));
    assert_eq!(points[0].y.percentage_coefficient(), Some(0.5));
    assert_eq!(points[1].x.percentage_coefficient(), Some(0.25));
    assert_eq!(points[1].x.fixed_component(), layout_pt(2.0));
    assert_eq!(points[2].x.percentage_coefficient(), Some(1.0));
    assert_eq!(points[2].y.percentage_coefficient(), Some(0.0));
}

#[test]
fn parses_border_shape_circles_with_typed_geometry() {
    let declarations = parse_declarations(
        "border-shape: circle(45px at 25% 75%) border-box circle(40%) padding-box",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let BorderShape::Pair { outer, inner } = style.border_shape else {
        panic!("expected parsed border-shape circles");
    };
    let BorderShape::Circle(outer) = *outer else {
        panic!("expected outer circle");
    };
    let BorderShape::Circle(inner) = *inner else {
        panic!("expected inner circle");
    };
    assert_eq!(outer.geometry_box, BorderShapeGeometryBox::Border);
    assert_eq!(inner.geometry_box, BorderShapeGeometryBox::Padding);
    assert_eq!(outer.position.x.percentage_coefficient(), Some(0.25));
    assert_eq!(outer.position.y.percentage_coefficient(), Some(0.75));
    assert!(matches!(
        outer.radius,
        BorderShapeCircleRadius::LengthPercentage(ref value) if value.length_points() == 45.0 * CSS_PX_TO_PT
    ));
}

#[test]
fn parses_shape_outside_basic_shapes_with_reference_boxes() {
    let declarations =
        parse_declarations("shape-outside: ellipse(25% farthest-side at 20% 80%) content-box");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let ShapeOutside::Basic {
        shape: BasicShape::Ellipse(ellipse),
        reference_box,
    } = style.shape_outside
    else {
        panic!("expected a parsed shape-outside ellipse");
    };
    assert_eq!(reference_box, ShapeBox::Content);
    assert_eq!(ellipse.position.x.percentage_coefficient(), Some(0.2));
    assert_eq!(ellipse.position.y.percentage_coefficient(), Some(0.8));
    assert!(
        matches!(ellipse.horizontal_radius, ShapeEllipseRadius::LengthPercentage(ref value) if value.percentage_coefficient() == Some(0.25))
    );
    assert_eq!(ellipse.vertical_radius, ShapeEllipseRadius::FarthestSide);
}

#[test]
fn parses_shape_margin_as_a_non_inherited_length_percentage() {
    let declarations = parse_declarations("shape-margin: calc(5% + 3px)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.shape_margin.percentage_coefficient(), Some(0.05));
    assert_eq!(style.shape_margin.length_points(), 3.0 * CSS_PX_TO_PT);

    let declarations = parse_declarations("shape-margin: -1px");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.shape_margin, ComputedLengthPercentage::ZERO);
}

#[test]
fn parses_image_shape_outside_and_alpha_threshold() {
    let declarations =
        parse_declarations("shape-outside: url(half.png); shape-image-threshold: 1.5");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert!(matches!(style.shape_outside, ShapeOutside::Image(_)));
    assert_eq!(style.shape_image_threshold, 1.0);
}

#[test]
fn parses_shape_outside_rounded_inset_and_shape_box() {
    let declarations =
        parse_declarations("shape-outside: border-box inset(10% 2pt 20% 4pt round 5px / 10px)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let ShapeOutside::Basic {
        shape: BasicShape::Inset(inset),
        reference_box,
    } = style.shape_outside
    else {
        panic!("expected a parsed shape-outside inset");
    };
    assert_eq!(reference_box, ShapeBox::Border);
    assert_eq!(inset.top.percentage_coefficient(), Some(0.1));
    assert_eq!(inset.left.length_points(), 4.0);
    assert_eq!(
        inset.radii.top_left.horizontal.value.length_points(),
        5.0 * CSS_PX_TO_PT
    );
    assert_eq!(
        inset.radii.top_left.vertical.value.length_points(),
        10.0 * CSS_PX_TO_PT
    );
}

#[test]
fn parses_shape_outside_edge_offset_position() {
    let declarations = parse_declarations("shape-outside: circle(10px at right 10% bottom 2pt)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ShapeOutside::Basic {
        shape: BasicShape::Circle(circle),
        ..
    } = style.shape_outside
    else {
        panic!("expected a parsed shape-outside circle");
    };
    assert_eq!(circle.position.x.percentage_coefficient(), Some(0.9));
    assert_eq!(circle.position.y.percentage_coefficient(), Some(1.0));
    assert_eq!(circle.position.y.fixed_component(), layout_pt(-2.0));
}

#[test]
fn parses_shape_outside_reordered_and_single_component_positions() {
    let declarations = parse_declarations("shape-outside: margin-box circle(99% at top left)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ShapeOutside::Basic {
        shape: BasicShape::Circle(circle),
        reference_box,
    } = style.shape_outside
    else {
        panic!("expected a parsed shape-outside circle");
    };
    assert_eq!(reference_box, ShapeBox::Margin);
    assert_eq!(circle.position.x, ComputedLengthPercentage::ZERO);
    assert_eq!(circle.position.y, ComputedLengthPercentage::ZERO);

    let declarations = parse_declarations("shape-outside: circle(10px at 40%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ShapeOutside::Basic {
        shape: BasicShape::Circle(circle),
        ..
    } = style.shape_outside
    else {
        panic!("expected a parsed shape-outside circle");
    };
    assert_eq!(circle.position.x.percentage_coefficient(), Some(0.4));
    assert_eq!(circle.position.y.percentage_coefficient(), Some(0.5));
}

#[test]
fn parses_shape_outside_polygon_with_fill_rule_and_percentages() {
    let declarations =
        parse_declarations("shape-outside: polygon(evenodd, 0% 0%, 100% 0%, 50% 100%) padding-box");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ShapeOutside::Basic {
        shape: BasicShape::Polygon(polygon),
        reference_box,
    } = style.shape_outside
    else {
        panic!("expected a parsed shape-outside polygon");
    };
    assert_eq!(reference_box, ShapeBox::Padding);
    assert_eq!(polygon.fill_rule, ShapeFillRule::EvenOdd);
    assert_eq!(polygon.vertices.len(), 3);
    assert_eq!(polygon.vertices[1].x.percentage_coefficient(), Some(1.0));
    assert_eq!(polygon.vertices[2].y.percentage_coefficient(), Some(1.0));
}

#[test]
fn parses_border_shape_ellipses_with_axis_typed_radii() {
    let declarations =
        parse_declarations("border-shape: ellipse(25% farthest-side at 20% 80%) content-box");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let BorderShape::Ellipse(ellipse) = style.border_shape else {
        panic!("expected parsed border-shape ellipses");
    };
    assert_eq!(ellipse.geometry_box, BorderShapeGeometryBox::Content);
    assert_eq!(ellipse.position.x.percentage_coefficient(), Some(0.2));
    assert_eq!(ellipse.position.y.percentage_coefficient(), Some(0.8));
    assert!(matches!(
        ellipse.horizontal_radius,
        BorderShapeEllipseRadius::LengthPercentage(ref value)
            if value.percentage_coefficient() == Some(0.25)
    ));
    assert!(matches!(
        ellipse.vertical_radius,
        BorderShapeEllipseRadius::FarthestSide
    ));
}

#[test]
fn parses_border_shape_polygon_with_typed_geometry_and_mixed_pair() {
    let declarations = parse_declarations(
        "border-shape: polygon(0 0, 100% 0, 50% 100%) margin-box circle(20%) content-box",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let BorderShape::Pair { outer, inner } = style.border_shape else {
        panic!("expected a parsed border-shape pair");
    };
    let BorderShape::Polygon(polygon) = *outer else {
        panic!("expected outer polygon");
    };
    let BorderShape::Circle(circle) = *inner else {
        panic!("expected inner circle");
    };
    assert_eq!(polygon.geometry_box, BorderShapeGeometryBox::Margin);
    assert_eq!(polygon.vertices.len(), 3);
    assert_eq!(polygon.vertices[1].x.percentage_coefficient(), Some(1.0));
    assert_eq!(polygon.vertices[2].y.percentage_coefficient(), Some(1.0));
    assert_eq!(circle.geometry_box, BorderShapeGeometryBox::Content);
}

#[tokio::test]
async fn parses_corner_shape_and_corner_shorthand() {
    let declarations =
        parse_declarations("corner: 36px round / 18px bevel / 28px scoop / 20px notch");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .value
            .length_points(),
        36.0 * CSS_PX_TO_PT
    );
    assert_eq!(
        style
            .border_radius
            .top_right
            .horizontal
            .value
            .length_points(),
        18.0 * CSS_PX_TO_PT
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .horizontal
            .value
            .length_points(),
        28.0 * CSS_PX_TO_PT
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .horizontal
            .value
            .length_points(),
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

    assert_eq!(
        style
            .border_radius
            .top_left
            .horizontal
            .value
            .length_points(),
        3.0
    );
    assert_eq!(
        style.border_radius.top_left.vertical.value.length_points(),
        3.0
    );
    assert_eq!(
        style
            .border_radius
            .top_right
            .horizontal
            .value
            .length_points(),
        10.0
    );
    assert_eq!(
        style.border_radius.top_right.vertical.value.length_points(),
        20.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .horizontal
            .value
            .length_points(),
        10.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_right
            .vertical
            .value
            .length_points(),
        20.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .horizontal
            .value
            .length_points(),
        10.0
    );
    assert_eq!(
        style
            .border_radius
            .bottom_left
            .vertical
            .value
            .length_points(),
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
    assert!(style.border_spacing.is_author_declared());
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

    style.resolve_font_metric_lengths(layout_pt(5.0));

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
    assert!(!table.border_spacing.is_author_declared());
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
    assert_eq!(
        style.text_decoration.color,
        CssColorOrCurrentColor::Color(CssColor::new(255, 0, 0))
    );
    assert!(matches!(
        style.text_decoration.thickness,
        TextDecorationThickness::LengthPercentage(value)
            if (value.length_points() - 2.25).abs() < 0.01 && value.percentage_coefficient_or_zero() == 0.0
    ));
    assert_eq!(style.text_decoration.skip_ink, TextDecorationSkipInk::None);
    assert_eq!(
        style.text_decoration.skip_spaces,
        TextDecorationSkipSpaces::StartEnd
    );
    assert!(matches!(
        style.text_decoration.underline_offset,
        TextUnderlineOffset::LengthPercentage(value)
            if (value.length_points() - 1.5).abs() < 0.01 && value.percentage_coefficient_or_zero() == 0.0
    ));
    assert!(style.text_decoration.underline_position.under);

    let declarations = parse_declarations("text-decoration: underline line-through");
    let mut shorthand_style = default_style_for_tag("div");
    apply_declarations(&mut shorthand_style, &declarations);
    assert!(shorthand_style.text_decoration.underline);
    assert!(shorthand_style.text_decoration.line_through);
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
                && start.percentage_coefficient_or_zero() == 0.0
                && (end.length_points() + 0.75).abs() < 0.01
                && end.percentage_coefficient_or_zero() == 0.0
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
        TextShadowColor::CssColor(CssColor::new(255, 0, 0))
    );
    assert!(!style.text_shadow[0].inset);
    assert!(style.text_shadow[1].inset);
    assert_eq!(style.box_shadow.len(), 2);
    assert!(style.box_shadow[0].inset);
    assert!((style.box_shadow[0].offset_x.length_points() - 45.0).abs() < 0.01);
    assert!((style.box_shadow[0].offset_y.length_points() - 0.0).abs() < 0.01);
    assert_eq!(
        style.box_shadow[0].color,
        BoxShadowColor::CssColor(CssColor::new(0, 128, 0))
    );
    assert_eq!(style.box_shadow[1].color, BoxShadowColor::CurrentColor);
    assert!((style.box_shadow[1].offset_x.length_points() + 1.5).abs() < 0.01);
    assert!((style.box_shadow[1].offset_y.length_points() - 2.25).abs() < 0.01);
    assert!((style.box_shadow[1].spread.length_points() + 0.75).abs() < 0.01);
    assert_eq!(
        style.text_emphasis_color,
        CssColorOrCurrentColor::Color(CssColor::new(0, 128, 0))
    );
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
        TextDecorationInset::Lengths { ref start, ref end }
            if *start == ComputedLengthPercentage::from_ch(2.0)
                && *end == ComputedLengthPercentage::from_points(1.0)
    ));

    style.resolve_font_metric_lengths(layout_pt(6.0));

    assert!(matches!(
        style.text_decoration.inset,
        TextDecorationInset::Lengths { start, end }
            if start == ComputedLengthPercentage::from_points(12.0)
                && end == ComputedLengthPercentage::from_points(1.0)
    ));
}

#[test]
fn text_decoration_origin_layer_refreshes_font_metric_lengths() {
    let declarations = parse_declarations(
        "text-decoration: underline; text-decoration-inset: -0.5ch; text-decoration-thickness: 1ch; text-underline-offset: 0.25ch",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    style.rebuild_own_text_decoration_origin();
    let layers = style.text_decoration_origins.effective_layers_vec();
    assert_eq!(layers.len(), 1);
    assert!(matches!(
        layers[0].decoration.inset,
        TextDecorationInset::Lengths { ref start, ref end }
            if *start == ComputedLengthPercentage::from_ch(-0.5)
                && *end == ComputedLengthPercentage::from_ch(-0.5)
    ));

    style.resolve_font_metric_lengths(layout_pt(6.0));
    style.rebuild_own_text_decoration_origin();
    style.rebuild_own_text_decoration_origin();

    let layers = style.text_decoration_origins.effective_layers_vec();
    assert_eq!(layers.len(), 1);
    let layer = &layers[0];
    assert!(
        !layer
            .origin_style
            .text_decoration_origins
            .has_effective_layers()
    );
    assert!(matches!(
        layer.decoration.inset,
        TextDecorationInset::Lengths { ref start, ref end }
            if *start == ComputedLengthPercentage::from_points(-3.0)
                && *end == ComputedLengthPercentage::from_points(-3.0)
    ));
    assert!(matches!(
        layer.decoration.thickness,
        TextDecorationThickness::LengthPercentage(ref value)
            if *value == ComputedLengthPercentage::from_points(6.0)
    ));
    assert!(matches!(
        layer.decoration.underline_offset,
        TextUnderlineOffset::LengthPercentage(ref value)
            if *value == ComputedLengthPercentage::from_points(1.5)
    ));
}

#[test]
fn rebuilding_own_text_decoration_origin_preserves_propagated_origins() {
    let mut ancestor = ComputedStyle::initial();
    ancestor.text_decoration.underline = true;
    ancestor.rebuild_own_text_decoration_origin();
    let ancestor_origin = ancestor
        .text_decoration_origins
        .effective_layers_vec()
        .pop()
        .unwrap()
        .origin_style;

    let mut child = ComputedStyle::initial();
    child
        .text_decoration_origins
        .set_propagated(ancestor.text_decoration_origins.effective_layers_vec());
    child.resolve_font_metric_lengths(layout_pt(6.0));
    child.rebuild_own_text_decoration_origin();

    let layers = child.text_decoration_origins.effective_layers_vec();
    assert_eq!(layers.len(), 1);
    assert!(std::rc::Rc::ptr_eq(
        &layers[0].origin_style,
        &ancestor_origin
    ));

    child.text_decoration.overline = true;
    child.rebuild_own_text_decoration_origin();
    let layers = child.text_decoration_origins.effective_layers_vec();
    assert_eq!(layers.len(), 2);
    assert!(std::rc::Rc::ptr_eq(
        &layers[0].origin_style,
        &ancestor_origin
    ));
    assert!(!std::rc::Rc::ptr_eq(
        &layers[0].origin_style,
        &layers[1].origin_style,
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

    style.resolve_font_metric_lengths(layout_pt(5.0));

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
async fn text_decoration_origins_do_not_enter_computed_style_inheritance() {
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

    assert_eq!(parent.text_decoration_origins.effective_layers().count(), 1);
    assert!(!child.text_decoration_origins.has_effective_layers());
    assert_eq!(
        child.text_decoration.color,
        CssColorOrCurrentColor::Color(CssColor::new(0, 0, 255))
    );
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

    assert_eq!(child.color, CssColor::new(255, 0, 0));
    assert!(!child.text_decoration_origins.has_effective_layers());
    assert_eq!(
        child.text_emphasis_color,
        CssColorOrCurrentColor::CurrentColor
    );
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
        CssColor::new(0, 128, 0)
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
        &Stylesheets::borrowed(&[ua]),
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
        &Stylesheets::borrowed(&[ua]),
        Some(&parent),
        &[ElementSignature::new("ruby", HashMap::new())],
    );
    assert_eq!(rt.text_emphasis_style, TextEmphasisStyle::None);
}

#[tokio::test]
async fn parses_text_decoration_skip_spaces_full_grammar() {
    let mut style = default_style_for_tag("div");
    assert_eq!(
        style.text_decoration.skip_spaces,
        TextDecorationSkipSpaces::StartEnd
    );

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
    assert_eq!(
        style.text_decoration.skip_spaces,
        TextDecorationSkipSpaces::Start
    );

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
    assert_eq!(
        style.text_decoration.color,
        CssColorOrCurrentColor::Color(CssColor::new(255, 0, 0))
    );

    apply_declarations(&mut style, &parse_declarations("text-decoration: overline"));
    assert!(!style.text_decoration.underline);
    assert!(style.text_decoration.overline);
    assert!(!style.text_decoration.line_through);
    assert_eq!(style.text_decoration.style, TextDecorationStyle::Solid);
    assert_eq!(
        style.text_decoration.color,
        CssColorOrCurrentColor::CurrentColor
    );

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
        &parse_declarations("writing-mode: sideways-lr; text-emphasis-style: filled"),
    );
    assert_eq!(
        style
            .text_emphasis_style
            .mark_for_writing_mode(style.writing_mode),
        Some("\u{25CF}")
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
    assert_eq!(style.color, CssColor::new(102, 51, 153));
    let background = style
        .background
        .background_color
        .color()
        .expect("explicit background color");
    assert_eq!(background.components(), [0.0, 1.0, 0.0]);
    assert!((background.alpha() - 136.0 / 255.0).abs() < 0.000001);
    assert_eq!(style.border_color, CssColor::new(1, 2, 3));
}

#[tokio::test]
async fn parses_alpha_and_transparent_colors() {
    let declarations = parse_declarations(
        "color: rgba(255, 0, 0, 0.5); background-color: rgb(0 0 255 / 25%); border-color: transparent",
    );
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.color, CssColor::rgba(255, 0, 0, 0.5));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::rgba(0, 0, 255, 0.25))
    );
    assert_eq!(style.border_color, CssColor::TRANSPARENT);
}

#[tokio::test]
async fn parses_hsl_border_colors() {
    let declarations =
        parse_declarations("border-color: hsl(120 100% 25% / 50%) hsla(240, 100%, 50%, 0.25)");
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, CssColor::rgba(0, 128, 0, 0.5));
    assert_eq!(style.border_colors.bottom, CssColor::rgba(0, 128, 0, 0.5));
    assert_eq!(style.border_colors.right, CssColor::rgba(0, 0, 255, 0.25));
    assert_eq!(style.border_colors.left, CssColor::rgba(0, 0, 255, 0.25));
}

#[tokio::test]
async fn parses_hwb_border_colors() {
    let declarations = parse_declarations(
        "border-color: hwb(0 0% 0%) hwb(120 0% 50% / 25%) hwb(240 20% 0%) hwb(0, 100%, 100%, 50%)",
    );
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, CssColor::new(255, 0, 0));
    assert_eq!(style.border_colors.right, CssColor::rgba(0, 128, 0, 0.25));
    assert_eq!(style.border_colors.bottom, CssColor::new(51, 51, 255));
    assert_eq!(style.border_colors.left, CssColor::rgba(128, 128, 128, 0.5));
}

#[tokio::test]
async fn parses_hwb_in_border_shorthand_as_single_component() {
    let declarations = parse_declarations("border: 2pt solid hwb(240 20% 0% / 75%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_colors.top, CssColor::rgba(51, 51, 255, 0.75));
}

#[tokio::test]
async fn parses_srgb_color_function_border_colors() {
    let declarations = parse_declarations(
        "border-color: color(srgb 1 0 0) color(srgb 0% 50% 0% / 25%) color(srgb 20% 20% 100%) color(srgb none 1.5 -1 / 50%)",
    );
    let mut style = default_style_for_tag("p");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_colors.top, CssColor::srgb(1.0, 0.0, 0.0, 1.0));
    assert_eq!(
        style.border_colors.right,
        CssColor::srgb(0.0, 0.5, 0.0, 0.25)
    );
    assert_eq!(
        style.border_colors.bottom,
        CssColor::srgb(0.2, 0.2, 1.0, 1.0)
    );
    assert_eq!(
        style.border_colors.left,
        CssColor::in_space(CssColorSpace::Srgb, 0.0, 1.5, -1.0, 0.5)
    );
}

#[tokio::test]
async fn parses_srgb_color_function_in_border_shorthand_as_single_component() {
    let declarations = parse_declarations("border: 2pt solid color(srgb 0.2 0.2 1 / 75%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.border_widths.top, 2.0);
    assert_eq!(style.border_styles.top, BorderStyle::Solid);
    assert_eq!(style.border_colors.top, CssColor::srgb(0.2, 0.2, 1.0, 0.75));
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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_percent(
            0.5
        ))
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            36.0
        ))
    );
    let ComputedLengthPercentageOrAuto::LengthPercentage(ref min_width) =
        style.box_values.min_width
    else {
        panic!("min-width should compute to a length");
    };
    assert!((min_width.length_points() - 56.692913).abs() < 0.001);
    assert_eq!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_rem(3.0))
    );
    style.finalize_computed_font_relative_lengths();
    assert_eq!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            36.0
        ))
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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_affine(
            layout_pt(-24.0),
            0.5,
            true
        ))
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            144.0
        ))
    );
    let ComputedLengthPercentageOrAuto::LengthPercentage(min_width) = style.box_values.min_width
    else {
        panic!("expected deferred min-width");
    };
    assert_eq!(
        min_width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
            .map(layout_points),
        Some(25.0)
    );
    assert_eq!(
        min_width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(-100.0)))
            .map(layout_points),
        Some(-10.0)
    );
    assert_eq!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            60.0
        ))
    );
    assert_eq!(style.font_size, 12.0);
    assert_eq!(style.line_height, 14.0);
}

#[tokio::test]
async fn parses_interpolated_percentage_and_viewport_lengths() {
    let declarations = parse_declarations(
        "width: calc((0%) * 0.5 + (200vw) * 0.5); height: calc((0%) * 0.5 + (200vh) * 0.5)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("expected an interpolated width");
    };
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = style.box_values.height.value()
    else {
        panic!("expected an interpolated height");
    };
    assert_eq!(
        width,
        ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_neutralized_affine(layout_pt(0.0)),
            ComputedLengthPercentage::from_vw(100.0)
        )
    );
    assert!(width.contains_percentage());
    assert!(!width.needs_percentage_basis());
    assert_eq!(
        height.clone(),
        ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_neutralized_affine(layout_pt(0.0)),
            ComputedLengthPercentage::from_vh(100.0)
        )
    );
    assert!(height.contains_percentage());
    assert!(!height.needs_percentage_basis());
}

#[tokio::test]
async fn preserves_authored_zero_percentage_in_calc_sizing() {
    let declarations = parse_declarations("width: calc(5em - 0%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("expected a calculated width");
    };

    assert!(width.contains_percentage());
    assert!(width.needs_percentage_basis(), "{width:#?}");
}

#[tokio::test]
async fn parses_interpolated_comparison_functions() {
    let declarations = parse_declarations(
        "width: calc((min(50px, 30%)) * 0.5 + (max(75%, 100px)) * 0.5); height: calc((min(75%, 160px)) * 0.5 + (max(50px, 20%)) * 0.5)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("expected an interpolated width");
    };
    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = style.box_values.height.value()
    else {
        panic!("expected an interpolated height");
    };
    // A 200 CSS-pixel containing block is 150 PDF points. The midpoint is a
    // 100 CSS-pixel square, i.e. 75 PDF points.
    assert_eq!(
        width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(150.0)))
            .map(layout_points),
        Some(75.0)
    );
    assert_eq!(
        height
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(150.0)))
            .map(layout_points),
        Some(75.0)
    );
}

#[tokio::test]
async fn calc_size_any_without_size_keyword_is_a_definite_length() {
    let declarations = parse_declarations("width: calc-size(any, calc(20px + 30px))");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            37.5
        ))
    );
}

#[tokio::test]
async fn calc_size_retains_an_auto_basis_and_affine_calculation() {
    let declarations = parse_declarations("width: calc-size(auto, size * 0.6 + 23px)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ComputedLengthPercentageOrAuto::CalcSize(value) = style.box_values.width else {
        panic!("expected a retained calc-size value");
    };
    assert_eq!(value.basis, CalcSizeBasis::Auto);
    assert!((value.size_multiplier - 0.6).abs() < 0.000_01);
    assert_eq!(value.additive.length_points(), 17.25);
}

#[test]
fn root_font_size_calc_uses_the_initial_rem_basis() {
    let value = crate::css::values::parse_deferred_font_size("calc(1rem + 1em)")
        .expect("calc font-size should parse");
    let resolved: LayoutLength = value.resolve(FontRelativeLengthBasis::new(
        layout_pt(ROOT_FONT_SIZE_PT),
        layout_pt(ROOT_FONT_SIZE_PT * 0.5),
    ));
    assert_eq!(resolved, layout_pt(24.0));
}

#[test]
fn deferred_font_size_resolves_parent_ex_ch_and_em() {
    let value = crate::css::values::parse_deferred_font_size("calc(1ex + 1ch + 1em)")
        .expect("calc font-size should parse");
    let resolved: LayoutLength = value.resolve(FontRelativeLengthBasis::new(
        layout_pt(ROOT_FONT_SIZE_PT),
        layout_pt(ROOT_FONT_SIZE_PT * 0.5),
    ));
    assert_eq!(resolved, layout_pt(ROOT_FONT_SIZE_PT * 2.0));
}

#[tokio::test]
async fn root_font_size_expression_is_not_reapplied_by_descendants() {
    let stylesheet = parse_stylesheet(&Css::from_string("html { font-size: calc(1rem + 1em) }"));
    let html_signature = ElementSignature::new("html", HashMap::new());
    let html = style_for_element_with_signature(
        html_signature.clone(),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );
    let body_signature = ElementSignature::new("body", HashMap::new());
    let body = style_for_element_with_signature(
        body_signature.clone(),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&html),
        std::slice::from_ref(&html_signature),
    );
    let mut div = style_for_element_with_signature(
        ElementSignature::new("div", HashMap::new()),
        Some("width: 1em"),
        std::slice::from_ref(&stylesheet),
        Some(&body),
        &[html_signature, body_signature],
    );

    assert_eq!(html.font_size, ROOT_FONT_SIZE_PT * 2.0);
    assert_eq!(body.font_size, ROOT_FONT_SIZE_PT * 2.0);
    assert_eq!(div.font_size, ROOT_FONT_SIZE_PT * 2.0);
    assert_eq!(div.deferred_font_size, DeferredFontSize::Inherit);
    assert_eq!(
        div.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_em(1.0))
    );
    div.finalize_computed_font_relative_lengths();
    assert_eq!(
        div.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            ROOT_FONT_SIZE_PT * 2.0,
        ))
    );
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
    assert!(!width.contains_percentage());
    assert_eq!(width.length_if_no_percent(), Some(40.0));

    let ComputedLengthPercentageOrAuto::LengthPercentage(min_width) = style.box_values.min_width
    else {
        panic!("expected percentage min-width");
    };
    assert!(min_width.contains_percentage());
    assert!(min_width.length_if_no_percent().is_none());

    let ComputedLengthPercentageOrAuto::LengthPercentage(max_width) = style.box_values.max_width
    else {
        panic!("expected calc max-width");
    };
    assert!(max_width.contains_percentage());
    assert_eq!(max_width.percentage_coefficient_or_zero(), 0.0);
    assert!(max_width.length_if_no_percent().is_none());
    assert_eq!(
        max_width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(200.0)))
            .map(layout_points),
        Some(40.0)
    );

    let declarations = parse_declarations("max-width: 40pt");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);
    let ComputedLengthPercentageOrAuto::LengthPercentage(max_width) = style.box_values.max_width
    else {
        panic!("expected length max-width");
    };
    assert!(!max_width.contains_percentage());
    assert_eq!(max_width.length_if_no_percent(), Some(40.0));

    let declarations = parse_declarations("max-width: none; max-height: none");
    let mut style = default_style_for_tag("div");
    style.box_values.max_width = ComputedLengthPercentageOrAuto::LengthPercentage(
        ComputedLengthPercentage::from_points(40.0),
    );
    style.box_values.max_height = ComputedLengthPercentageOrAuto::LengthPercentage(
        ComputedLengthPercentage::from_points(60.0),
    );
    apply_declarations(&mut style, &declarations);
    assert_eq!(
        style.box_values.max_width,
        ComputedLengthPercentageOrAuto::Auto
    );
    assert_eq!(
        style.box_values.max_height,
        ComputedLengthPercentageOrAuto::Auto
    );
}

#[tokio::test]
async fn rejects_incomparable_css_math_number_length_values() {
    let declarations = parse_declarations("width: 20pt; width: min(10pt, 2)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            20.0
        ))
    );
}

#[tokio::test]
async fn css_math_defers_ch_comparisons_until_font_metric_resolution() {
    let declarations = parse_declarations(
        "width: calc(min(10pt, 2ch) + 1pt); height: calc(max(10pt, 2ch) - 1pt); min-width: clamp(10pt, 2ch, 20pt); max-height: min(20pt, 2ch, 10pt); line-height: min(10pt, calc(2ch + 1pt))",
    );
    let mut small_ch = default_style_for_tag("div");
    apply_declarations(&mut small_ch, &declarations);

    let ComputedLengthPercentageOrAuto::LengthPercentage(ref width) = small_ch.box_values.width
    else {
        panic!("expected deferred width");
    };
    assert!(width.length_if_no_percent().is_none());
    assert!(small_ch.box_values.height.is_deferred_font_metric());

    let mut large_ch = small_ch.clone();

    small_ch.resolve_font_metric_lengths(layout_pt(4.0));
    large_ch.resolve_font_metric_lengths(layout_pt(6.0));

    assert!(small_ch.box_values.height.is_deferred_font_metric());
    assert!(large_ch.box_values.height.is_deferred_font_metric());

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

    let ComputedLengthPercentageOrAuto::LengthPercentage(ref width) = style.box_values.width else {
        panic!("expected deferred width");
    };
    assert!(width.length_if_no_percent().is_none());
    assert_eq!(
        width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(12.0)))
            .map(layout_points),
        Some(6.0)
    );
    assert_eq!(
        width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(40.0)))
            .map(layout_points),
        Some(10.0)
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = style.box_values.height.value()
    else {
        panic!("expected deferred height");
    };
    assert_eq!(
        height
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(12.0)))
            .map(layout_points),
        Some(10.0)
    );
    assert_eq!(
        height
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(40.0)))
            .map(layout_points),
        Some(20.0)
    );

    style.resolve_font_metric_lengths(layout_pt(4.0));

    let ComputedLengthPercentageOrAuto::LengthPercentage(min_width) = style.box_values.min_width
    else {
        panic!("expected deferred min-width");
    };
    assert_eq!(
        min_width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
            .map(layout_points),
        Some(41.0)
    );
    assert_eq!(
        min_width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(60.0)))
            .map(layout_points),
        Some(31.0)
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(max_width) = style.box_values.max_width
    else {
        panic!("expected deferred max-width");
    };
    assert_eq!(
        max_width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(12.0)))
            .map(layout_points),
        Some(10.0)
    );
    assert_eq!(
        max_width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
            .map(layout_points),
        Some(50.0)
    );
    assert_eq!(
        max_width
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(200.0)))
            .map(layout_points),
        Some(80.0)
    );
}

#[tokio::test]
async fn indefinite_percentage_bases_leave_percentage_math_unresolved() {
    let declarations = parse_declarations("width: 40pt; height: min(10pt, 50%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
        panic!("expected a fixed width");
    };
    assert_eq!(
        width.used_length_with_percentage_basis(PercentageBasis::<LayoutLength>::indefinite()),
        Some(layout_pt(40.0))
    );

    let ComputedLengthPercentageOrAuto::LengthPercentage(height) = style.box_values.height.value()
    else {
        panic!("expected deferred percentage math");
    };
    assert_eq!(
        height.used_length_with_percentage_basis(PercentageBasis::<LayoutLength>::indefinite()),
        None
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

    style.resolve_font_metric_lengths(layout_pt(5.0));

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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_points(1.0),
            ComputedLengthPercentage::from_ch(2.0)
        ))
    );
    assert_eq!(
        style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_points(5.0),
            ComputedLengthPercentage::from_ch(2.0)
        ))
    );
    assert_eq!(
        style.box_values.min_width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_percent(0.1),
            ComputedLengthPercentage::from_ch(1.0)
        ))
    );
    assert_eq!(
        style.line_height_value,
        ComputedLineHeight::Length(ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_points(3.0),
            ComputedLengthPercentage::from_ch(2.0)
        ))
    );

    style.resolve_font_metric_lengths(layout_pt(5.0));

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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_affine(
            layout_pt(5.0),
            0.1,
            true
        ))
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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_percent(
            0.1
        ))
    );
    assert_eq!(
        style.box_values.padding.left,
        ComputedLengthPercentage::from_points(7.5)
    );
    assert_eq!(
        style.box_values.padding.top,
        ComputedLengthPercentage::from_percent(0.05)
    );
    assert_eq!(
        style.box_values.inset_left,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_percent(
            0.25
        ))
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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_em(10.0))
    );
    style.finalize_computed_font_relative_lengths();
    assert_eq!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            75.0
        ))
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
    assert!(child.line_height_is_normal());

    let parent = ComputedStyle {
        line_height: 30.0,
        line_height_value: ComputedLineHeight::Number(1.5),
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

    parent.writing_mode = WritingMode::SidewaysLr;
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

    style.resolve_font_metric_lengths(layout_pt(8.0));

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

    let mut child = style_for_element_with_signature(
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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_em(1.0))
    );
    child.finalize_computed_font_relative_lengths();
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
        FontFamily::List(vec![
            FontFamily::Names(vec!["-apple-system".to_string()]),
            FontFamily::Names(vec!["BlinkMacSystemFont".to_string()]),
            FontFamily::Names(vec!["Segoe UI".to_string()]),
            FontFamily::Names(vec!["Roboto".to_string()]),
            FontFamily::Names(vec!["Helvetica Neue".to_string()]),
            FontFamily::Names(vec!["Arial".to_string()]),
            FontFamily::SansSerif,
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
        FontFamily::List(vec![
            FontFamily::Names(vec!["-apple-system".to_string()]),
            FontFamily::Names(vec!["BlinkMacSystemFont".to_string()]),
            FontFamily::Names(vec!["Segoe UI".to_string()]),
            FontFamily::Names(vec!["Roboto".to_string()]),
            FontFamily::Names(vec!["Helvetica Neue".to_string()]),
            FontFamily::Names(vec!["Arial".to_string()]),
            FontFamily::SansSerif,
            FontFamily::Names(vec!["Segoe UI Symbol".to_string()]),
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
        &Stylesheets::borrowed(&[ua]),
        Some(&parent),
        &[],
    );
    let h4 = style_for_element_with_signature(
        ElementSignature::new("h4", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[ua]),
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn cascade_origin_orders_user_between_ua_and_author_for_normal_declarations() {
    let ua = parse_stylesheet(&Css::from_string("p { color: red }").with_user_agent_origin());
    let user = parse_stylesheet(&Css::from_string("p { color: green }").with_user_origin());
    let author = parse_stylesheet(&Css::from_string("p { color: blue }"));

    let user_over_ua = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[&ua, &user]),
        None,
        &[],
    );
    assert_eq!(user_over_ua.color, CssColor::new(0, 128, 0));

    let author_over_user = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[user, author],
        None,
        &[],
    );
    assert_eq!(author_over_user.color, CssColor::new(0, 0, 255));
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
    assert_eq!(user_over_author.color, CssColor::new(0, 128, 0));

    let ua_over_user = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[user, ua],
        None,
        &[],
    );
    assert_eq!(ua_over_user.color, CssColor::new(255, 0, 0));
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
    assert_eq!(
        stylesheet.layer_names,
        vec![
            LayerName(vec![LayerSegment::Named("theme".to_string())]),
            LayerName(vec![LayerSegment::Named("base".to_string())]),
        ]
    );

    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::from([("id".to_string(), "hero".to_string())])),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn nested_layer_direct_declarations_follow_the_implicit_final_sublayer() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer framework {\
           @layer components { p { color: blue } }\
           p { color: red }\
         }",
    ));

    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn layer_names_keep_escaped_dots_distinct_from_hierarchy_separators() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer a\\.b { p { color: blue } } @layer a.b { p { color: red } }",
    ));
    assert_ne!(stylesheet.layer_names[0], stylesheet.layer_names[1]);
    assert_eq!(stylesheet.layer_names.len(), 2);
}

#[tokio::test]
async fn malformed_layer_delimiters_do_not_create_a_valid_prefix_layer() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer theme /**/ . colors { p { color: red } } p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(style.color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn empty_and_limit_only_scopes_use_the_document_root() {
    let empty = parse_stylesheet(&Css::from_string("@scope { p { color: red } }"));
    let limited = parse_stylesheet(&Css::from_string(
        "@scope to (.stop) { p { color: red } } p { color: blue }",
    ));
    let ancestors = [
        ElementSignature::new("html", HashMap::new()),
        ElementSignature::new("body", HashMap::new()),
    ];
    let empty_style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[empty],
        None,
        &ancestors,
    );
    let limited_style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[limited],
        None,
        &ancestors,
    );
    assert_eq!(empty_style.color, CssColor::new(255, 0, 0));
    assert_eq!(limited_style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn empty_scope_uses_the_embedded_stylesheet_owner_anchor() {
    let owner = crate::dom::Node::element("section");
    let owner_id = owner.as_element().expect("owner element").id;
    let stylesheet = parse_stylesheet(
        &Css::from_string("@scope { p { color: red } }")
            .with_scope_anchor(StylesheetScopeAnchor::Element(owner_id)),
    );
    let mut matching_owner = ElementSignature::new("section", HashMap::new());
    matching_owner.source_element_id = Some(owner_id);
    let inside = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[
            ElementSignature::new("html", HashMap::new()),
            matching_owner,
        ],
    );
    let outside = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new("html", HashMap::new())],
    );
    assert_eq!(inside.color, CssColor::new(255, 0, 0));
    assert_ne!(outside.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn scope_boundaries_reject_pseudo_elements() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@scope (.card::before) { p { color: red } } p { color: blue }",
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
    assert_eq!(style.color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn nested_scope_roots_are_constrained_by_the_outer_scope() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@scope (.outer) { @scope (html) { p { color: red } } } p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[
            ElementSignature::new("html", HashMap::new()),
            ElementSignature::new(
                "section",
                HashMap::from([("class".to_string(), "outer".to_string())]),
            ),
        ],
    );
    assert_eq!(style.color, CssColor::new(0, 0, 255));
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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
    assert_eq!(before.color, CssColor::new(255, 0, 0));
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
        CssColor::new(0, 0, 255)
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(style.border_colors.top, CssColor::new(0, 128, 0));
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
async fn supports_rule_recognizes_wrap_inside_avoid() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (wrap-inside: avoid) { p { color: blue } }\
         @supports (wrap-inside: invalid) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(style.border_colors.top, CssColor::new(0, 128, 0));
    assert_eq!(style.border_colors.right, CssColor::new(0, 0, 255));
    assert_eq!(style.outline_color, CssColor::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, CssColor::new(255, 0, 0));
    assert_eq!(style.border_colors.left, CssColor::new(0, 128, 0));
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(style.border_colors.top, CssColor::new(0, 128, 0));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(style.border_colors.top, CssColor::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, CssColor::new(0, 0, 255));
    assert_eq!(style.border_colors.left, CssColor::new(0, 128, 0));
    assert_eq!(style.border_colors.right.resolve(style.color), style.color);
    assert_eq!(style.border_widths.top, 3.0);
    assert_eq!(style.border_widths.right, 3.0);
    assert_eq!(style.border_widths.bottom, 3.0);
    assert_eq!(style.border_widths.left, 3.0);
    assert_eq!(style.outline_color, CssColor::new(0, 128, 0));
    assert_eq!(style.outline_style, BorderStyle::None);
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(style.border_colors.top, CssColor::new(0, 128, 0));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(255, 0, 0))
    );
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(255, 0, 0))
    );
    assert_eq!(style.border_colors.top, CssColor::new(0, 128, 0));
    assert_eq!(style.border_colors.bottom, CssColor::new(0, 0, 255));
    assert_ne!(style.border_colors.left, CssColor::new(255, 0, 0));
    assert_eq!(style.border_colors.right, CssColor::new(0, 0, 255));
    assert_eq!(style.outline_color, CssColor::new(0, 128, 0));
    assert_eq!(style.border_colors.left.resolve(style.color), style.color);
}

#[tokio::test]
async fn supports_rule_ignores_unsupported_declaration_conditions() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports (unsupported-quire-feature: true) { p { color: blue } } p { color: red }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::new(255, 0, 0))
    );
}

#[test]
fn supports_condition_uses_the_conditional_rules_grammar() {
    // Bare declaration tests, function-token spellings of logical keywords,
    // and mixed `and`/`or` levels are invalid grammar, not false values which
    // may be allowed to alter a cascade.
    assert!(!supports_condition_applies("margin: 0"));
    assert!(!supports_condition_applies("not(foo: baz)"));
    assert!(!supports_condition_applies(
        "((margin: 0) and (display: inline) or (width: 1em))"
    ));
    assert!(!supports_condition_applies(
        "((background-color: red) or(background-color: green))"
    ));
    assert!(!supports_condition_applies("(margin: 0 or padding: 0)"));

    assert!(supports_condition_applies(
        "((margin: 0) and (background: blue) and (padding: inherit))"
    ));
    assert!(supports_condition_applies(
        "(writing-mode: vertical-lr) and (direction: rtl)"
    ));
    assert!(supports_condition_applies("(margin: revert-layer)"));
}

#[test]
fn declaration_operations_are_shared_by_supports_and_the_normal_cascade() {
    let cases = [
        ("width", "calc-size(any, calc(20px + 30px))", true),
        ("margin", "auto 10%", true),
        ("--token", "var(--other, 1px)", true),
        ("border-shape", "circle(20%) content-box", true),
        ("width", "not-a-size", false),
        ("--", "1px", false),
    ];
    for (name, value, expected) in cases {
        assert_eq!(
            crate::css::parse::declaration_operation(name, value).is_some(),
            expected,
            "canonical operation: {name}: {value}"
        );
        assert_eq!(
            supports_condition_applies(&format!("({name}: {value})")),
            expected,
            "feature query: {name}: {value}"
        );
    }

    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("width: calc-size(any, calc(20px + 30px)); margin-left: auto"),
    );
    assert!(matches!(
        style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(_)
    ));
    assert!(style.box_values.margin.left.is_auto());
}

#[test]
fn supports_selector_condition_requires_one_complex_selector() {
    assert!(supports_condition_applies("selector(div > span)"));
    assert!(supports_condition_applies("(selector(div > span))"));
    assert!(!supports_condition_applies("selector(div, span)"));
    assert!(!supports_condition_applies("selector(> .item)"));
    assert!(!supports_condition_applies("(selector(> .item))"));
    assert!(supports_condition_applies("not selector(> .item)"));
    assert!(supports_condition_applies("not (selector(> .item))"));
    assert!(supports_condition_applies("selector(&)"));
    assert!(supports_condition_applies("(selector(&))"));
    assert!(!supports_condition_applies("not selector(&)"));
    assert!(supports_condition_applies("selector(::before::marker)"));
    assert!(supports_condition_applies("selector(li::after::marker)"));
    assert!(!supports_condition_applies(
        "not selector(::before::marker)"
    ));
    assert!(!supports_condition_applies(
        "not (selector(::before::marker) and selector(::after::marker))"
    ));
}

#[test]
fn conditional_groups_activate_stylesheet_scoped_font_and_counter_resources() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@media not all { \
             @font-face { font-family: ignored; src: url(ignored.ttf) } \
             @counter-style ignored { system: cyclic; symbols: 'I' } \
         } \
         @supports (color: blue) { \
             @font-face { font-family: active; src: url(active.ttf) } \
             @counter-style active { system: cyclic; symbols: 'A' } \
         }",
    ));

    assert_eq!(stylesheet.font_faces.len(), 1);
    assert_eq!(stylesheet.font_faces[0].family, "active");
    assert_eq!(stylesheet.counter_styles.len(), 1);
    assert_eq!(stylesheet.counter_styles[0].name, "active");
}

#[test]
fn conditional_groups_activate_font_palette_values() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@media not all { \
             @font-palette-values --ignored { font-family: test; base-palette: dark } \
         } \
         @supports (color: blue) { \
             @font-palette-values --active { font-family: test; base-palette: light } \
         }",
    ));

    assert!(stylesheet.font_palette_values.get("--ignored").is_none());
    let active = stylesheet
        .font_palette_values
        .get("--active")
        .expect("active palette definition");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].base, FontPalette::Light);
}

#[test]
fn conditional_groups_activate_font_feature_values() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@media not all { \
             @font-feature-values ignored { @styleset { ignored: 1 } } \
         } \
         @supports (color: blue) { \
             @font-feature-values active { @styleset { active: 2 } } \
         }",
    ));

    assert!(
        stylesheet
            .font_feature_values
            .get("ignored", FontFeatureValuesBlock::Styleset, "ignored")
            .is_none()
    );
    assert_eq!(
        stylesheet
            .font_feature_values
            .get("active", FontFeatureValuesBlock::Styleset, "active")
            .map(|value| value.feature_index),
        Some(2)
    );
}

#[test]
fn conditional_resources_preserve_nested_layer_precedence() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer lower, higher; \
         @supports (color: blue) { \
             @layer higher { @font-feature-values family { @styleset { choice: 2 } } } \
             @layer lower { @font-feature-values family { @styleset { choice: 1 } } } \
         }",
    ));

    assert_eq!(
        stylesheet
            .font_feature_values
            .get("family", FontFeatureValuesBlock::Styleset, "choice")
            .map(|value| value.feature_index),
        Some(2)
    );
}

#[test]
fn namespace_rules_are_limited_to_the_stylesheet_prelude() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@namespace accepted 'urn:accepted'; \
         @media all { @namespace nested 'urn:nested'; } \
         @namespace late 'urn:late';",
    ));

    assert_eq!(
        stylesheet.namespace_prefixes.get("accepted"),
        Some(&"urn:accepted".to_string())
    );
    assert!(!stylesheet.namespace_prefixes.contains_key("nested"));
    assert!(!stylesheet.namespace_prefixes.contains_key("late"));
}

#[tokio::test]
async fn supports_rule_evaluates_selector_conditions_with_selector_parser() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@supports selector(:scope > p) { p { color: blue } }\
         @supports selector(p:has(> span)) { p { border-top-color: lime } }\
         @supports selector(p:hover) { p { border-right-color: blue } }\
         @supports selector(p::first-line) { p { border-bottom-color: red } }\
         @supports selector(p::unsupported-quire-pseudo) { p { background-color: red } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(style.border_colors.top, CssColor::new(0, 255, 0));
    assert_eq!(style.border_colors.right, CssColor::new(0, 0, 255));
    assert_eq!(style.border_colors.bottom, CssColor::new(255, 0, 0));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    assert_eq!(
        style.background.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
    assert_eq!(style.border_colors.top.resolve(style.color), style.color);
    assert_eq!(style.border_colors.right.resolve(style.color), style.color);
    assert_eq!(style.border_colors.bottom.resolve(style.color), style.color);
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn import_supports_declaration_condition_loads_the_import() {
    let dir = std::env::temp_dir().join(format!(
        "quire-import-supports-declaration-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("imported.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "p { color: green }").unwrap();
    std::fs::write(
        &main_path,
        "@import \"imported.css\" supports(display: block);",
    )
    .unwrap();

    let parsed_stylesheets = Css::from_file(&main_path)
        .await
        .unwrap()
        .with_imports()
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

    assert_eq!(parsed_stylesheets.len(), 2);
    assert_eq!(style.color, CssColor::new(0, 128, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_supports_unknown_declaration_condition_skips_the_import() {
    let dir = std::env::temp_dir().join(format!(
        "quire-import-supports-unknown-declaration-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("imported.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "p { color: red }").unwrap();
    std::fs::write(
        &main_path,
        "@import \"imported.css\" supports(foo: bar); p { color: blue }",
    )
    .unwrap();

    let parsed_stylesheets = Css::from_file(&main_path)
        .await
        .unwrap()
        .with_imports()
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

    assert_eq!(parsed_stylesheets.len(), 1);
    assert_eq!(style.color, CssColor::new(0, 0, 255));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_supports_logical_condition_uses_the_shared_evaluator() {
    let dir = std::env::temp_dir().join(format!(
        "quire-import-supports-logical-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("imported.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "p { color: green }").unwrap();
    std::fs::write(
        &main_path,
        "@import \"imported.css\" supports(not (unsupported: yes));",
    )
    .unwrap();

    let parsed_stylesheets = Css::from_file(&main_path)
        .await
        .unwrap()
        .with_imports()
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

    assert_eq!(parsed_stylesheets.len(), 2);
    assert_eq!(style.color, CssColor::new(0, 128, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_layer_places_imported_rules_in_named_layer() {
    let dir = std::env::temp_dir().join(format!("quire-import-layer-named-{}", std::process::id()));
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

    let css = Css::from_file(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports()
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_media_not_print_keeps_import_out_of_cascade() {
    let dir = std::env::temp_dir().join(format!(
        "quire-import-media-not-print-{}",
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

    let css = Css::from_file(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports()
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_media_not_screen_loads_import_in_print_context() {
    let dir = std::env::temp_dir().join(format!(
        "quire-import-media-not-screen-{}",
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

    let css = Css::from_file(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports()
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn anonymous_import_layer_important_beats_unlayered_important() {
    let dir = std::env::temp_dir().join(format!(
        "quire-import-layer-anonymous-{}",
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

    let css = Css::from_file(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports()
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
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

    assert_eq!(style.color, CssColor::new(255, 0, 0));
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
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

    assert_eq!(style.color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn author_revert_rolls_property_back_to_user_origin() {
    let ua = parse_stylesheet(&Css::from_string("p { color: red }").with_user_agent_origin());
    let user = parse_stylesheet(&Css::from_string("p { color: green }").with_user_origin());
    let author = parse_stylesheet(&Css::from_string("p { color: blue; color: revert }"));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[&ua, &user, &author]),
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 128, 0));
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
        &Stylesheets::borrowed(&[&ua, &author, &user]),
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn ua_origin_revert_behaves_like_unset_for_non_inherited_modeled_property() {
    let ua = parse_stylesheet(
        &Css::from_string("p { margin-left: 12pt; margin-left: revert }").with_user_agent_origin(),
    );
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[&ua]),
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
        &Stylesheets::borrowed(&[&ua, &author]),
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

    assert_eq!(style.color, CssColor::new(0, 128, 0));
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
    assert_eq!(child.writing_mode, WritingMode::VerticalRl);
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

    assert_eq!(style.color, CssColor::BLACK);
    assert_eq!(style.display, Display::INLINE);
    assert_eq!(style.margin.left, 0.0);
}

#[tokio::test]
async fn all_property_expands_before_property_prepasses() {
    for keyword in ["initial", "inherit", "unset"] {
        let stylesheet = parse_stylesheet(&Css::from_string(format!(
            "p {{ all: {keyword}; color: green; font-size: 18pt; color-scheme: dark }}"
        )));
        let style = style_for_element_with_signature(
            ElementSignature::new("p", HashMap::new()),
            None,
            &[stylesheet],
            None,
            &[],
        );

        assert_eq!(style.color, CssColor::new(0, 128, 0), "{keyword}");
        assert_eq!(style.font_size, 18.0, "{keyword}");
        assert_eq!(style.used_color_scheme, UsedColorScheme::Dark, "{keyword}");
    }
}

#[tokio::test]
async fn all_property_excludes_direction_and_unicode_bidi() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { direction: rtl; unicode-bidi: bidi-override; all: initial }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.direction, Direction::Rtl);
    assert_eq!(style.unicode_bidi, UnicodeBidi::BidiOverride);
}

#[tokio::test]
async fn all_revert_rolls_back_each_longhand_to_the_previous_origin() {
    let ua = parse_stylesheet(&Css::from_string("p { color: green }").with_user_agent_origin());
    let author = parse_stylesheet(&Css::from_string("p { color: red; all: revert }"));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[&ua, &author]),
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 128, 0));
}

#[tokio::test]
async fn all_revert_layer_rolls_back_each_longhand_to_the_previous_layer() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base, theme; \
         @layer base { p { color: green } } \
         @layer theme { p { color: red; all: revert-layer } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 128, 0));
}

#[tokio::test]
async fn all_property_variable_expands_before_property_prepasses() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { --reset: initial; all: var(--reset); color: green; font-size: 18pt; color-scheme: dark }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );

    assert_eq!(style.color, CssColor::new(0, 128, 0));
    assert_eq!(style.font_size, 18.0);
    assert_eq!(style.used_color_scheme, UsedColorScheme::Dark);
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
        CssColor::new(255, 0, 0)
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
async fn normal_user_page_declarations_do_not_override_author_preferences() {
    let author = parse_stylesheet(&Css::from_string("@page { margin: 2in }"));
    let user = parse_stylesheet(&Css::from_string("@page { margin: 3in }").with_user_origin());
    let page_rules = author
        .page_rules
        .iter()
        .chain(user.page_rules.iter())
        .cloned()
        .collect::<Vec<_>>();

    let margins = page_margins_from(
        &cascade_page_declarations(&page_rules, 1),
        PageMargins::all_points(0.0),
    );

    // A CLI stylesheet is a user preference: an explicit author page rule
    // remains authoritative under the normal CSS cascade.
    assert_eq!(margins, PageMargins::all_points(144.0));
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
async fn import_layer_applies_to_imported_page_context() {
    let dir = std::env::temp_dir().join(format!("quire-import-layer-page-{}", std::process::id()));
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

    let css = Css::from_file(&main_path).await.unwrap();
    let parsed_stylesheets = css
        .with_imports()
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
    assert!(
        selectors
            .iter()
            .any(|selector| selector.contains(":is(.outer) .inner"))
    );
    assert!(
        selectors
            .iter()
            .any(|selector| selector.contains(":is(.outer):last-child"))
    );
}

#[tokio::test]
async fn top_level_parent_selector_uses_document_root_scope() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "& .target { color: red } & > .target { color: green }",
    ));

    let descendant = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("class".to_string(), "target".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[
            ElementSignature::new("html", HashMap::new()),
            ElementSignature::new("body", HashMap::new()),
        ],
    );
    let direct_child = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("class".to_string(), "target".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new("html", HashMap::new())],
    );

    assert_eq!(descendant.color, CssColor::new(255, 0, 0));
    assert_eq!(direct_child.color, CssColor::new(0, 128, 0));
}

#[tokio::test]
async fn top_level_parent_selector_has_zero_parent_specificity() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "& .target { color: red } .container .target { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("class".to_string(), "target".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[
            ElementSignature::new("html", HashMap::new()),
            ElementSignature::new(
                "section",
                HashMap::from([("class".to_string(), "container".to_string())]),
            ),
        ],
    );

    assert_eq!(style.color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn top_level_parent_selector_uses_embedded_stylesheet_owner_scope() {
    let owner = crate::dom::Node::element("section");
    let owner_id = owner.as_element().expect("owner element").id;
    let stylesheet = parse_stylesheet(
        &Css::from_string(".target { color: blue } & .target { color: red }")
            .with_selector_scope_anchor(StylesheetScopeAnchor::Element(owner_id)),
    );
    let mut matching_owner = ElementSignature::new("section", HashMap::new());
    matching_owner.source_element_id = Some(owner_id);

    let inside = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("class".to_string(), "target".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[
            ElementSignature::new("html", HashMap::new()),
            matching_owner,
        ],
    );
    let outside = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("class".to_string(), "target".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new("html", HashMap::new())],
    );

    assert_eq!(inside.color, CssColor::new(255, 0, 0));
    assert_eq!(outside.color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn parent_selector_inside_scope_uses_the_scope_root() {
    let owner = crate::dom::Node::element("section");
    let owner_id = owner.as_element().expect("owner element").id;
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            "@scope (.scope) { & > .target { color: red !important } } .target { color: blue }",
        )
        .with_scope_anchor(StylesheetScopeAnchor::Element(owner_id)),
    );
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("class".to_string(), "target".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[
            ElementSignature::new("html", HashMap::new()),
            ElementSignature::new(
                "section",
                HashMap::from([("class".to_string(), "scope".to_string())]),
            ),
        ],
    );

    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn pseudo_element_rules_retain_stylesheet_scope_anchor() {
    let owner = crate::dom::Node::element("section");
    let owner_id = owner.as_element().expect("owner element").id;
    let stylesheet = parse_stylesheet(
        &Css::from_string(".target::before { color: red; content: 'x' }")
            .with_selector_scope_anchor(StylesheetScopeAnchor::Element(owner_id)),
    );

    assert_eq!(stylesheet.before_rules.len(), 1);
    assert_eq!(
        stylesheet.before_rules[0].stylesheet_scope_anchor,
        StylesheetScopeAnchor::Element(owner_id)
    );
}

#[tokio::test]
async fn nested_rules_use_css_nesting_selector_semantics() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "#high, .low { & p { color: red } } .container p { color: blue }",
    ));
    let parent = default_style_for_tag("body");
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        Some(&parent),
        &[ElementSignature::new(
            "div",
            HashMap::from([("class".to_string(), "low container".to_string())]),
        )],
    );
    // `&` has the maximum specificity of its parent selector list, like
    // `:is(#high, .low)`, rather than the matched branch's specificity.
    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn declarations_interleaved_with_nested_rules_keep_source_order() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "article { color: green; & { color: blue } color: red }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("article", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[],
    );
    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn nested_group_rules_keep_the_style_rule_context() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".outer { @media print { color: red; > p { color: blue } } @supports (display: block) { background-color: green } @layer nested { border-top-color: black } }",
    ));
    assert!(
        stylesheet
            .rules
            .iter()
            .any(|rule| rule.declarations.get("background-color").is_some())
    );
    assert!(
        stylesheet
            .rules
            .iter()
            .any(|rule| rule.declarations.get("border-top-color").is_some())
    );
    let outer = style_for_element_with_signature(
        ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "outer".to_string())]),
        ),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&default_style_for_tag("body")),
        &[],
    );
    let child = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        Some(&outer),
        &[ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "outer".to_string())]),
        )],
    );
    assert_eq!(outer.color, CssColor::new(255, 0, 0));
    assert_eq!(child.color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn nested_container_rules_retain_their_query_hierarchy() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@container outer (width > 1px) { .host { @container inner (height > 1px) { color: red } } }",
    ));
    assert_eq!(stylesheet.container_rules.len(), 1);
    assert_eq!(stylesheet.container_rules[0].nested().len(), 1);
    assert_eq!(stylesheet.container_rules[0].nested()[0].rules().len(), 1);
}

#[tokio::test]
async fn nested_scope_parent_selector_is_tokenized() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".outer { @scope (&) { & > p { color: red } } } p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "outer".to_string())]),
        )],
    );
    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn nested_scope_limit_parent_selector_has_scope_specificity_behavior() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".outer { @scope (&) to (& .limit) { p { color: red } } } p { color: blue }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "outer".to_string())]),
        )],
    );
    assert_eq!(style.color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn sass_style_parent_suffix_is_not_a_css_nesting_selector() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".card { &Header { color: red } } .card { color: blue }",
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
    assert_eq!(style.color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn malformed_nested_rules_recover_without_rewriting_component_values() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        ".outer { --value: { braces: ';'; text: \"& }\" }; &Header { color: red } .valid { color: green } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("class".to_string(), "valid".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "outer".to_string())]),
        )],
    );
    assert_eq!(style.color, CssColor::new(0, 128, 0));
}

#[tokio::test]
async fn eof_closed_nested_blocks_follow_css_syntax_recovery() {
    let stylesheet = parse_stylesheet(&Css::from_string(".outer { .valid { color: green"));
    let style = style_for_element_with_signature(
        ElementSignature::new(
            "p",
            HashMap::from([("class".to_string(), "valid".to_string())]),
        ),
        None,
        &[stylesheet],
        None,
        &[ElementSignature::new(
            "section",
            HashMap::from([("class".to_string(), "outer".to_string())]),
        )],
    );
    assert_eq!(style.color, CssColor::new(0, 128, 0));
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
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_points(
            18.0 * 28.346_457
        ))
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

    assert!(selectors.contains(&":is(th, td):first-of-type"));
    assert!(selectors.contains(&":is(th, td):last-of-type"));

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
async fn nth_of_type_applies_full_size_kana_to_only_the_second_table_cell() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "td:nth-of-type(2) { text-transform: full-size-kana }",
    ));
    let parent = default_style_for_tag("tr");
    let sibling_tags = vec!["td".to_string(), "td".to_string(), "td".to_string()];

    let cell_styles = (0..3)
        .map(|sibling_index| {
            style_for_element_with_signature(
                ElementSignature::with_siblings(
                    "td",
                    HashMap::new(),
                    sibling_index,
                    sibling_tags.clone(),
                ),
                None,
                std::slice::from_ref(&stylesheet),
                Some(&parent),
                &[],
            )
        })
        .collect::<Vec<_>>();

    assert!(!cell_styles[0].text_transform.applies_full_size_kana());
    assert!(cell_styles[1].text_transform.applies_full_size_kana());
    assert!(!cell_styles[2].text_transform.applies_full_size_kana());
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

    assert!(selectors.any(|s| s == ":is(#ticket) h2"));

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
        TextTransform::Keywords(
            TextTransformKeywords::new(Some(TextTransformCase::Uppercase), false, false).unwrap()
        )
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

    assert!(selectors.contains(&":is(#informations) h1"));
    assert!(selectors.contains(&":is(#informations) #name"));
    assert!(selectors.contains(&":is(#informations) #destination"));

    let ua = html5_user_agent_stylesheet();
    let stylesheet_references = [ua, &stylesheet];
    let stylesheets = Stylesheets::borrowed(&stylesheet_references);
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
        TextTransform::Keywords(
            TextTransformKeywords::new(Some(TextTransformCase::Uppercase), false, false).unwrap()
        )
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

    assert!(selectors.contains(&":is(:is(table) td):last-of-type"));
    assert!(selectors.contains(&":is(:is(table) th, :is(table) td):last-of-type"));
    assert!(selectors.contains(&":is(:is(table) th, :is(table) td):first-of-type"));

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
    assert_eq!(last.color, CssColor::new(30, 228, 148));
}

#[tokio::test]
async fn invoice_nested_aside_margin_uses_three_value_shorthand() {
    let css = Css::from_file("weasyprint-samples/invoice/invoice.css")
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

    assert_eq!(style.break_inside, BreakInsideAvoidance::AvoidPage);
}

#[tokio::test]
async fn parses_modern_break_inside_avoid_for_all_fragmentainers() {
    let declarations = parse_declarations("break-inside: avoid");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.break_inside, BreakInsideAvoidance::Avoid);
}

#[tokio::test]
async fn parses_named_page_property() {
    let declarations = parse_declarations("page: report");
    let mut style = default_style_for_tag("section");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.page,
        PageAssignment::Named(PageName::new("report".to_string()))
    );

    let declarations = parse_declarations("page: auto");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.page, PageAssignment::Auto);
}

#[tokio::test]
async fn parses_running_position_property() {
    let declarations = parse_declarations("position: running(header)");
    let mut style = default_style_for_tag("h1");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.position,
        Position::Running(RunningElementName::new("header".to_string()))
    );

    apply_declarations(&mut style, &parse_declarations("position: relative"));
    assert_eq!(style.position, Position::Relative);
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
    assert_eq!(style.z_index, ZIndex::StackLevel(7));

    let declarations = parse_declarations("z-index: auto");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.z_index, ZIndex::Auto);
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
            TransformFunction::Translate(CssTransformTranslation {
                x: ComputedLengthPercentage::from_points(10.0),
                y: ComputedLengthPercentage::from_percent(0.25),
            }),
            TransformFunction::Scale(CssScaleFactors { x: 2.0, y: 1.0 }),
            TransformFunction::Rotate(euclid::Angle::radians(std::f32::consts::FRAC_PI_2)),
            TransformFunction::Skew(CssSkewAngles {
                x: euclid::Angle::radians(0.0),
                y: euclid::Angle::radians(std::f32::consts::FRAC_PI_2)
            }),
            TransformFunction::Matrix(CssAffineMatrix::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0)),
        ]
    );
    assert_eq!(
        style.transform_origin,
        TransformOrigin {
            x: ComputedLengthPercentage::from_percent(1.0),
            y: ComputedLengthPercentage::from_points(20.0),
            z: ComputedLengthPercentage::ZERO,
            is_initial: false,
        }
    );
}

#[tokio::test]
async fn parses_percentage_scale_factors() {
    let declarations = parse_declarations("transform: scale(50%, 75%) scaleX(25%) scaleY(125%)");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.transform,
        vec![
            TransformFunction::Scale(CssScaleFactors { x: 0.5, y: 0.75 }),
            TransformFunction::Scale(CssScaleFactors { x: 0.25, y: 1.0 }),
            TransformFunction::Scale(CssScaleFactors { x: 1.0, y: 1.25 }),
        ]
    );
}

#[tokio::test]
async fn parses_individual_2d_transforms_and_none() {
    let declarations = parse_declarations(
        "translate: 2ch 25%; rotate: 90deg; scale: 50% 2; transform: translateX(3pt)",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.individual_transforms,
        IndividualTransforms {
            translate: Some(CssTransformTranslation {
                x: ComputedLengthPercentage::from_ch(2.0),
                y: ComputedLengthPercentage::from_percent(0.25),
            }),
            rotate: Some(euclid::Angle::radians(std::f32::consts::FRAC_PI_2)),
            scale: Some(CssScaleFactors { x: 0.5, y: 2.0 }),
        }
    );
    assert!(style.has_transform());

    style.resolve_font_metric_lengths(layout_pt(5.0));
    assert_eq!(
        style.individual_transforms.translate,
        Some(CssTransformTranslation {
            x: ComputedLengthPercentage::from_points(10.0),
            y: ComputedLengthPercentage::from_percent(0.25),
        })
    );

    let declarations =
        parse_declarations("translate: none; rotate: none; scale: none; transform: none");
    apply_declarations(&mut style, &declarations);
    assert_eq!(style.individual_transforms, IndividualTransforms::NONE);
    assert!(!style.has_transform());
}

#[test]
fn parses_object_view_box_rectangles() {
    let mut style = default_style_for_tag("img");
    apply_declarations(
        &mut style,
        &parse_declarations("object-view-box: xywh(10px 20% 30px 40%)"),
    );
    assert!(matches!(
        &style.object_view_box,
        ObjectViewBox::Xywh { x, y, width, height, radii: None }
            if (x.length_points() - 7.5).abs() < 0.01
                && y.percentage_coefficient_or_zero() == 0.2
                && (width.length_points() - 22.5).abs() < 0.01
                && height.percentage_coefficient_or_zero() == 0.4
    ));

    apply_declarations(
        &mut style,
        &parse_declarations("object-view-box: inset(10% 2pt 30% 4pt round 5pt)"),
    );
    assert!(matches!(
        &style.object_view_box,
        ObjectViewBox::Inset { top, right, bottom, left, radii: Some(_) }
            if top.percentage_coefficient_or_zero() == 0.1
                && (right.length_points() - 2.0).abs() < 0.01
                && bottom.percentage_coefficient_or_zero() == 0.3
                && (left.length_points() - 4.0).abs() < 0.01
    ));

    apply_declarations(
        &mut style,
        &parse_declarations("object-view-box: rect(1pt, 20pt, 30pt, 4pt)"),
    );
    assert!(matches!(&style.object_view_box, ObjectViewBox::Rect { .. }));

    apply_declarations(
        &mut style,
        &parse_declarations("object-view-box: rect(1pt 20pt 30pt 4pt)"),
    );
    assert!(matches!(&style.object_view_box, ObjectViewBox::Rect { .. }));

    apply_declarations(&mut style, &parse_declarations("object-view-box: none"));
    assert_eq!(style.object_view_box, ObjectViewBox::None);

    let before = style.object_view_box.clone();
    apply_declarations(
        &mut style,
        &parse_declarations("object-view-box: xywh(1pt 2pt 3pt);"),
    );
    assert_eq!(style.object_view_box, before);
}

#[test]
fn parses_clip_path_inset_offsets() {
    let mut style = default_style_for_tag("div");
    apply_declarations(
        &mut style,
        &parse_declarations("clip-path: inset(10% 2pt 30% 4pt)"),
    );

    assert!(matches!(
        style.clip_path,
        ClipPath::Inset { top, right, bottom, left }
            if top.percentage_coefficient_or_zero() == 0.1
                && (right.length_points() - 2.0).abs() < 0.01
                && bottom.percentage_coefficient_or_zero() == 0.3
                && (left.length_points() - 4.0).abs() < 0.01
    ));
}

#[tokio::test]
async fn rejects_3d_individual_transform_values() {
    let declarations = parse_declarations("translate: 1pt 2pt 3pt; rotate: x 90deg; scale: 1 2 3");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(style.individual_transforms, IndividualTransforms::NONE);
}

#[tokio::test]
async fn ch_transform_lengths_preserve_font_metric_component_until_used_resolution() {
    let declarations =
        parse_declarations("transform: translate(2ch, 25%); transform-origin: 3ch 4ch");
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.transform,
        vec![TransformFunction::Translate(CssTransformTranslation {
            x: ComputedLengthPercentage::from_ch(2.0),
            y: ComputedLengthPercentage::from_percent(0.25),
        })]
    );
    assert_eq!(
        style.transform_origin,
        TransformOrigin {
            x: ComputedLengthPercentage::from_ch(3.0),
            y: ComputedLengthPercentage::from_ch(4.0),
            z: ComputedLengthPercentage::ZERO,
            is_initial: false,
        }
    );

    style.resolve_font_metric_lengths(layout_pt(5.0));

    assert_eq!(
        style.transform,
        vec![TransformFunction::Translate(CssTransformTranslation {
            x: ComputedLengthPercentage::from_points(10.0),
            y: ComputedLengthPercentage::from_percent(0.25),
        })]
    );
    assert_eq!(
        style.transform_origin,
        TransformOrigin {
            x: ComputedLengthPercentage::from_points(15.0),
            y: ComputedLengthPercentage::from_points(20.0),
            z: ComputedLengthPercentage::ZERO,
            is_initial: false,
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
    assert_eq!(
        style.outline_color.resolve(style.color),
        CssColor::new(0x12, 0x34, 0x56)
    );
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

    style.resolve_font_metric_lengths(layout_pt(4.0));

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

    style.resolve_font_metric_lengths(layout_pt(4.0));

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
    assert_eq!(column_colors[0], CssColor::new(255, 0, 0));
    assert_eq!(column_colors[2], CssColor::new(0x12, 0x34, 0x56));
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
        CssColor::new(0, 0, 255)
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
async fn gap_rule_auto_repeater_truncates_trailing_colors_widths_and_styles_in_authored_order() {
    let declarations = parse_declarations(
        "column-rule-color: teal, indigo, violet, repeat(auto, red, green), blue, purple, coral;\
         column-rule-width: 2px, 5px, 2px, repeat(auto, 4px), 10px, 11px, 12px;\
         column-rule-style: solid, dashed, dotted, repeat(auto, double), groove, ridge, inset",
    );
    let mut style = default_style_for_tag("div");
    apply_declarations(&mut style, &declarations);

    assert_eq!(
        style.column_rule.colors.values_for_count(5),
        [
            CssColor::new(0, 128, 128),
            CssColor::new(75, 0, 130),
            CssColor::new(238, 130, 238),
            CssColor::new(0, 0, 255),
            CssColor::new(128, 0, 128),
        ]
    );
    assert_eq!(
        style.column_rule.widths.values_for_count(5),
        [
            ComputedLengthPercentage::from_points(1.5),
            ComputedLengthPercentage::from_points(3.75),
            ComputedLengthPercentage::from_points(1.5),
            ComputedLengthPercentage::from_points(7.5),
            ComputedLengthPercentage::from_points(8.25),
        ]
    );
    assert_eq!(
        style.column_rule.styles.values_for_count(5),
        [
            BorderStyle::Solid,
            BorderStyle::Dashed,
            BorderStyle::Dotted,
            BorderStyle::Groove,
            BorderStyle::Ridge,
        ]
    );
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

#[test]
fn deferred_font_size_detects_only_parent_ch_dependencies() {
    let parent_size = 12.0;

    assert!(!DeferredFontSize::Absolute(18.0).requires_parent_ch_advance(parent_size));
    assert!(!DeferredFontSize::Inherit.requires_parent_ch_advance(parent_size));
    assert!(
        DeferredFontSize::RelativeToParent(ComputedLengthPercentage::from_ch(2.0))
            .requires_parent_ch_advance(parent_size)
    );
}

#[test]
fn deferred_font_size_resolves_lh_against_parent_line_height() {
    let font_size = parse_deferred_font_size("calc(1lh + 2pt)").unwrap();
    let parent = FontRelativeLengthBasis::new(layout_pt(10.0), layout_pt(5.0))
        .with_line_height(layout_pt(24.0));

    assert_eq!(font_size.resolve(parent), layout_pt(26.0));
}

#[test]
fn line_height_uses_inherited_lh_and_own_selected_metrics() {
    let mut line_height = parse_computed_line_height("calc(1lh + 1ex)", 20.0).unwrap();
    line_height.resolve_inherited_line_height_relative_lengths(layout_pt(24.0));
    line_height.resolve_selected_font_metric_lengths(SelectedFontMetricLengthBasis::new(
        layout_pt(10.0),
        layout_pt(10.0),
        layout_pt(8.0),
        layout_pt(14.0),
    ));

    assert_eq!(line_height.projected(20.0).0, 32.0);
}

#[test]
fn object_fit_parses_and_cascades() {
    let mut style = ComputedStyle::initial();
    apply_declarations(
        &mut style,
        &parse_declarations("object-fit: contain; object-fit: cover"),
    );
    assert_eq!(style.object_fit, ObjectFit::Cover);
}

#[test]
fn computed_style_ch_advance_query_matches_font_metric_projection() {
    fn projected_requires_ch_advance(style: &ComputedStyle) -> bool {
        let mut projected = style.clone();
        projected.resolve_font_metric_lengths(layout_pt(0.0));
        style != &projected
    }

    for (declarations, expected) in [
        ("", false),
        (
            "line-height: 2ch; row-gap: 3ch; column-gap: 4ch; column-width: 5ch; column-height: 6ch; letter-spacing: 7ch; word-spacing: 8ch; width: 9ch; border-radius: 10ch; border-width: 11ch; outline-width: 12ch; outline-offset: 13ch; flex-basis: 14ch; text-indent: 15ch; vertical-align: 16ch; tab-size: 17ch",
            true,
        ),
        (
            "grid-template-columns: minmax(1ch, fit-content(2ch)) repeat(2, 3ch); grid-auto-rows: minmax(4ch, 5ch); grid-auto-columns: 6ch",
            true,
        ),
        (
            "background: linear-gradient(to right, red 2ch, blue 10vw); background-size: 3ch 4ch; background-position: 5ch 6ch",
            true,
        ),
        (
            "background: radial-gradient(circle 2ch at 3ch 4ch, red 5ch, 6ch, blue)",
            true,
        ),
        (
            "transform: translate(2ch, 25%); transform-origin: 3ch 4ch; border-image-width: 2ch; border-image-outset: 3ch; text-decoration-thickness: 4ch; text-decoration-inset: 5ch 6ch; text-underline-offset: 7ch; text-shadow: 8ch 9ch; box-shadow: 10ch 11ch black; border-spacing: 12ch",
            true,
        ),
        (
            "row-rule-width: 2ch; column-rule-width: 3ch; rule-inset-start: 4ch",
            true,
        ),
        ("width: calc(1ch + 2pt)", true),
        ("width: 2em; height: 10vw", false),
    ] {
        let mut style = ComputedStyle::initial();
        apply_declarations(&mut style, &parse_declarations(declarations));

        assert_eq!(
            style.requires_ch_advance(),
            expected,
            "declarations: {declarations}",
        );
        assert_eq!(
            style.requires_ch_advance(),
            projected_requires_ch_advance(&style),
            "declarations: {declarations}",
        );
    }

    let mut style = ComputedStyle::initial();
    style.box_values.width =
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_ch(2.0));
    assert!(style.requires_ch_advance());
    style.resolve_font_metric_lengths(layout_pt(5.0));
    assert!(!style.requires_ch_advance());
}

#[test]
fn computed_style_selected_font_metrics_include_ic_ex_and_cap() {
    for declaration in [
        "width: 2ch",
        "width: 2ic",
        "width: calc(100px + 2ic)",
        "width: 2ex",
        "width: 2cap",
        "line-height: 2ch",
        "line-height: calc(100px + 2ic)",
        "line-height: 2ex",
        "line-height: 2cap",
    ] {
        let mut style = ComputedStyle::initial();
        apply_declarations(&mut style, &parse_declarations(declaration));

        assert!(style.requires_selected_font_metrics(), "{declaration}");
    }

    let style = ComputedStyle::initial();
    assert!(!style.requires_selected_font_metrics());
}

#[test]
fn computed_style_root_font_metric_query_matches_root_metric_projection() {
    fn projected_requires_root_font_metrics(style: &ComputedStyle) -> bool {
        let zero = layout_pt(0.0);
        let mut projected = style.clone();
        // `rem` resolves during computed-value finalization, before the
        // document-root selected-font metric snapshot is needed.
        projected.finalize_computed_font_relative_lengths();
        let normalized = projected.clone();
        projected.resolve_root_font_metric_lengths(RootFontMetricLengthBasis {
            font_size: zero,
            ch_advance: zero,
            x_height: zero,
            cap_height: zero,
            ic_advance: zero,
            line_height: zero,
        });
        normalized != projected
            || [
                style.marker_style.as_deref(),
                style.before_style.as_deref(),
                style.after_style.as_deref(),
                style.first_line_style.as_deref(),
                style.first_letter_style.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(projected_requires_root_font_metrics)
    }

    for (declarations, expected) in [
        ("", false),
        ("line-height: 2rex", true),
        ("width: 2rcap", true),
        ("margin-left: 2rch", true),
        ("padding-top: 2ric", true),
        ("height: 2rlh", true),
        ("width: 2rem", false),
    ] {
        let mut style = ComputedStyle::initial();
        apply_declarations(&mut style, &parse_declarations(declarations));

        assert_eq!(
            style.requires_root_font_metrics(),
            expected,
            "declarations: {declarations}",
        );
        assert_eq!(
            style.requires_root_font_metrics(),
            projected_requires_root_font_metrics(&style),
            "declarations: {declarations}",
        );
    }

    let root_metric_calc_sizes = [
        (
            CalcSize {
                basis: CalcSizeBasis::Auto,
                size_multiplier: 0.0,
                additive: ComputedLengthPercentage::from_rex(1.0),
                lower_bound: None,
                upper_bound: None,
            },
            "additive term",
        ),
        (
            CalcSize {
                basis: CalcSizeBasis::Auto,
                size_multiplier: 0.0,
                additive: ComputedLengthPercentage::ZERO,
                lower_bound: Some(CalcSizeAffine {
                    size_multiplier: 0.0,
                    additive: ComputedLengthPercentage::from_rcap(1.0),
                }),
                upper_bound: None,
            },
            "lower bound",
        ),
        (
            CalcSize {
                basis: CalcSizeBasis::Auto,
                size_multiplier: 0.0,
                additive: ComputedLengthPercentage::ZERO,
                lower_bound: None,
                upper_bound: Some(CalcSizeAffine {
                    size_multiplier: 0.0,
                    additive: ComputedLengthPercentage::from_rch(1.0),
                }),
            },
            "upper bound",
        ),
        (
            CalcSize {
                basis: CalcSizeBasis::LengthPercentage(ComputedLengthPercentage::from_ric(1.0)),
                size_multiplier: 0.0,
                additive: ComputedLengthPercentage::ZERO,
                lower_bound: None,
                upper_bound: None,
            },
            "length-percentage basis",
        ),
    ];
    for (value, source) in root_metric_calc_sizes {
        let mut style = ComputedStyle::initial();
        style.box_values.width = ComputedLengthPercentageOrAuto::CalcSize(value);

        assert!(style.requires_root_font_metrics(), "{source}");
        assert!(projected_requires_root_font_metrics(&style), "{source}");
    }

    fn root_metric_child() -> ComputedStyle {
        let mut child = ComputedStyle::initial();
        child.box_values.width = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_rlh(1.0),
        );
        child
    }

    let mut style = ComputedStyle::initial();
    style.marker_style = Some(Box::new(root_metric_child()));
    assert!(style.requires_root_font_metrics());
    let mut style = ComputedStyle::initial();
    style.before_style = Some(Box::new(root_metric_child()));
    assert!(style.requires_root_font_metrics());
    let mut style = ComputedStyle::initial();
    style.after_style = Some(Box::new(root_metric_child()));
    assert!(style.requires_root_font_metrics());
    let mut style = ComputedStyle::initial();
    style.first_line_style = Some(Box::new(root_metric_child()));
    assert!(style.requires_root_font_metrics());
    let mut style = ComputedStyle::initial();
    style.first_letter_style = Some(Box::new(root_metric_child()));
    assert!(style.requires_root_font_metrics());
}

#[test]
fn ruby_alignment_and_overhang_values_cascade_and_inherit() {
    let mut style = ComputedStyle::initial();
    apply_declarations(
        &mut style,
        &parse_declarations(
            "ruby-align: start; ruby-overhang: none; ruby-align: center; ruby-overhang: spaces",
        ),
    );
    assert_eq!(style.ruby_align, RubyAlign::Center);
    assert_eq!(style.ruby_overhang, RubyOverhang::Spaces);

    for (declaration, align) in [
        ("ruby-align: start", RubyAlign::Start),
        ("ruby-align: center", RubyAlign::Center),
        ("ruby-align: space-between", RubyAlign::SpaceBetween),
        ("ruby-align: space-around", RubyAlign::SpaceAround),
    ] {
        let mut style = ComputedStyle::initial();
        apply_declarations(&mut style, &parse_declarations(declaration));
        assert_eq!(style.ruby_align, align, "{declaration}");
    }

    assert_eq!(ComputedStyle::initial().ruby_align, RubyAlign::SpaceAround);
    assert_eq!(ComputedStyle::initial().ruby_overhang, RubyOverhang::Auto);
}

#[tokio::test]
async fn ruby_alignment_and_overhang_honor_css_wide_keywords() {
    let parent = style_for_element_with_signature(
        ElementSignature::new("section", HashMap::new()),
        Some("ruby-align: start; ruby-overhang: spaces"),
        &[],
        None,
        &[],
    );
    let stylesheet = parse_stylesheet(&Css::from_string(
        "p { ruby-align: initial; ruby-overhang: initial } \
         q { ruby-align: inherit; ruby-overhang: inherit } \
         em { ruby-align: unset; ruby-overhang: unset }",
    ));

    let initial = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[],
    );
    assert_eq!(initial.ruby_align, RubyAlign::SpaceAround);
    assert_eq!(initial.ruby_overhang, RubyOverhang::Auto);

    for tag in ["q", "em"] {
        let inherited = style_for_element_with_signature(
            ElementSignature::new(tag, HashMap::new()),
            None,
            std::slice::from_ref(&stylesheet),
            Some(&parent),
            &[],
        );
        assert_eq!(inherited.ruby_align, RubyAlign::Start);
        assert_eq!(inherited.ruby_overhang, RubyOverhang::Spaces);
    }

    let ua = parse_stylesheet(
        &Css::from_string("p { ruby-align: center; ruby-overhang: auto }").with_user_agent_origin(),
    );
    let author = parse_stylesheet(&Css::from_string(
        "p { ruby-align: start; ruby-overhang: spaces; ruby-align: revert; ruby-overhang: revert }",
    ));
    let reverted = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &Stylesheets::borrowed(&[&ua, &author]),
        None,
        &[],
    );
    assert_eq!(reverted.ruby_align, RubyAlign::Center);
    assert_eq!(reverted.ruby_overhang, RubyOverhang::Auto);

    let layered = parse_stylesheet(&Css::from_string(
        "@layer base, theme; \
         @layer base { p { ruby-align: start; ruby-overhang: spaces } } \
         @layer theme { p { ruby-align: center; ruby-overhang: auto; ruby-align: revert-layer; ruby-overhang: revert-layer } }",
    ));
    let reverted_layer = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        &[layered],
        None,
        &[],
    );
    assert_eq!(reverted_layer.ruby_align, RubyAlign::Start);
    assert_eq!(reverted_layer.ruby_overhang, RubyOverhang::Spaces);
}

#[test]
fn keyframes_blocks_follow_css_tokens_and_recover_locally() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        r#"
            @key\66 rames sl\69 de {
                fr\6f m, 50%, to {
                    opacity: 0;
                    content: "};,@top-left";
                    background-image: url("data:text/plain,};,@top-left");
                }
                125%, from { opacity: .9 }
                @unknown { color: red }
                25% { opacity: .25 }
            }
            @keyframes later { from { opacity: 0 } to { opacity: 1 } }
        "#,
    ));

    assert_eq!(stylesheet.keyframes.len(), 2);
    let slide = &stylesheet.keyframes[0];
    assert_eq!(slide.name.as_str(), "slide");
    assert_eq!(
        slide
            .steps
            .iter()
            .map(|step| step.offset)
            .collect::<Vec<_>>(),
        vec![0.0, 0.5, 1.0, 0.25]
    );
    assert_eq!(
        slide.steps[0]
            .declarations
            .get("content")
            .map(String::as_str),
        Some("\"};,@top-left\"")
    );
}

#[test]
fn keyframes_eof_blocks_recover_without_accepting_invalid_selector_lists() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@keyframes invalid { from, 101% { opacity: .9 } 25% { opacity: .25 } } \
         @keyframes recovered { from { opacity: 0 } to { opacity: 1 }",
    ));

    assert_eq!(stylesheet.keyframes.len(), 2);
    assert_eq!(
        stylesheet.keyframes[0]
            .steps
            .iter()
            .map(|step| step.offset)
            .collect::<Vec<_>>(),
        vec![0.25]
    );
    assert_eq!(
        stylesheet.keyframes[1]
            .steps
            .iter()
            .map(|step| step.offset)
            .collect::<Vec<_>>(),
        vec![0.0, 1.0]
    );
}

#[test]
fn keyframes_names_are_decoded_case_sensitive_css_values() {
    let names = parse_stylesheet(&Css::from_string(
        "@keyframes none { from { opacity: 0 } } \
         @keyframes initial { from { opacity: 0 } } \
         @keyframes \"none\" { from { opacity: 0 } } \
         @keyframes FADE { from { opacity: 0 } } \
         @keyframes f\\61 de { from { opacity: 0 } }",
    ));
    assert_eq!(
        names
            .keyframes
            .iter()
            .map(|rule| rule.name.as_str())
            .collect::<Vec<_>>(),
        vec!["none", "FADE", "fade"]
    );

    let stylesheet = parse_stylesheet(&Css::from_string(
        "@keyframes fade { from { opacity: 0 } to { opacity: .2 } } \
         @keyframes FADE { from { opacity: 0 } to { opacity: 1 } } \
         p { animation: fade 1s -0.5s }",
    ));
    assert_eq!(
        stylesheet
            .keyframes
            .iter()
            .map(|rule| rule.name.as_str())
            .collect::<Vec<_>>(),
        vec!["fade", "FADE"]
    );
    let fade = KeyframesName::parse_css("fade").expect("valid unquoted keyframes name");
    assert_eq!(stylesheet.keyframes[0].name, fade);
    assert_ne!(stylesheet.keyframes[1].name, fade);

    let stylesheet = parse_stylesheet(&Css::from_string(
        "@keyframes \"quoted name\" { from { opacity: 0 } to { opacity: 1 } }",
    ));
    assert_eq!(
        stylesheet.keyframes[0].name,
        KeyframesName::parse_css("\"quoted name\"").expect("valid quoted keyframes name")
    );
}

#[test]
fn typed_animation_snapshot_longhands_follow_shorthand_defaulting_and_inheritance() {
    let mut parent = ComputedStyle::initial();
    apply_declarations(
        &mut parent,
        &parse_declarations("animation: parent-fade 2s -0.5s"),
    );

    let stylesheet = parse_stylesheet(&Css::from_string("p { animation: inherit }"));
    let mut child = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        Some(&parent),
        &[],
    );
    assert_eq!(child.animation_snapshot, parent.animation_snapshot);

    apply_declarations(
        &mut child,
        &parse_declarations("animation-duration: 3s; all: initial"),
    );
    assert_eq!(
        child.animation_snapshot,
        ComputedAnimationSnapshot::INITIAL,
        "all must reset the modeled animation longhands"
    );

    apply_declarations(
        &mut child,
        &parse_declarations("--snapshot: fade 4s -1s; animation: var(--snapshot)"),
    );
    assert_eq!(child.animation_snapshot.duration_seconds, 4.0);
    assert_eq!(child.animation_snapshot.delay_seconds, -1.0);
    assert_eq!(
        child
            .animation_snapshot
            .name
            .as_ref()
            .map(KeyframesName::as_str),
        Some("fade")
    );

    apply_declarations(&mut child, &parse_declarations("animation: var(--missing)"));
    assert_eq!(child.animation_snapshot, ComputedAnimationSnapshot::INITIAL);
}

#[tokio::test]
async fn animation_revert_layer_rolls_back_each_typed_snapshot_component() {
    let stylesheet = parse_stylesheet(&Css::from_string(
        "@layer base { p { animation: fade 2s -0.5s } } \
         @layer theme { p { animation: replacement 4s -1s; animation: revert-layer } }",
    ));
    let style = style_for_element_with_signature(
        ElementSignature::new("p", HashMap::new()),
        None,
        std::slice::from_ref(&stylesheet),
        None,
        &[],
    );

    assert_eq!(style.animation_snapshot.duration_seconds, 2.0);
    assert_eq!(style.animation_snapshot.delay_seconds, -0.5);
    assert_eq!(
        style
            .animation_snapshot
            .name
            .as_ref()
            .map(KeyframesName::as_str),
        Some("fade")
    );
}
