use super::*;

const ASCENT_DESCENT_OVERRIDE_WPT: &str =
    "tests/fixtures/wpt/css/css-fonts/ascent-descent-override.html";
const NON_EM_ASCENDER_PAINT_FIXTURE: &str =
    "tests/fixtures/wpt/css/css-fonts/non-em-ascender-paint.html";
const NON_EM_ASCENDER_PREPARED_PAINT_FIXTURE: &str =
    "tests/fixtures/wpt/css/css-fonts/non-em-ascender-prepared-paint.html";
const FONT_SIZE_ADJUST_MIXED_FALLBACK_BASELINE_FIXTURE: &str =
    "tests/fixtures/wpt/css/css-fonts/font-size-adjust-mixed-fallback-baseline.html";
const ROOT_FONT_METRICS_IN_MONOSPACE_FIXTURE: &str =
    "tests/fixtures/wpt/css/css-fonts/root-font-metrics-in-monospace.html";
const FONT_SYNTHESIS_WEIGHT_PDF_FIXTURE: &str =
    "tests/fixtures/wpt/css/css-fonts/font-synthesis-weight-pdf.html";
const FONT_SYNTHESIS_STYLE_PDF_FIXTURE: &str =
    "tests/fixtures/wpt/css/css-fonts/font-synthesis-style-pdf.html";
const FONT_SYNTHESIS_FIRST_LINE_PDF_FIXTURE: &str =
    "tests/fixtures/wpt/css/css-fonts/font-synthesis-first-line-pdf.html";
const VARIABLE_FONT_INSTANCE_PDF_FIXTURE: &str =
    "tests/fixtures/wpt/css/css-fonts/variable-font-instance-pdf.html";
const SIZE_ADJUST_MIXED_OPAQUE_COVERAGE_FIXTURE: &str =
    "tests/fixtures/wpt/css/css-fonts/size-adjust-mixed-opaque-coverage.html";

/// A selected faux-bold face changes PDF paint only when CSS permits weight
/// synthesis; an authored bold face must remain an ordinary fill-only font.
/// <https://www.w3.org/TR/css-fonts-4/#font-synthesis-intro>
#[tokio::test]
async fn font_synthesis_weight_is_retained_per_selected_document_font() {
    let document = Html::from_file(FONT_SYNTHESIS_WEIGHT_PDF_FIXTURE)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let font_for = |text: &str| {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| {
                panic!(
                    "fixture line {text:?} should paint; got {:?}",
                    document.pages[0]
                        .lines()
                        .iter()
                        .map(|line| line.text.as_str())
                        .collect::<Vec<_>>()
                )
            });
        line_font(&document, line)
    };

    assert!(font_for("synthesized").synthesis.embolden);
    // The opaque Ahem glyphs are separate coverage segments on either side
    // of the visible space, so use the first word to inspect the selected
    // document font for this CSS text record.
    assert!(!font_for("not").synthesis.embolden);
    let authored_bold = font_for("authored bold");
    assert!(!authored_bold.synthesis.embolden);
    assert!(
        ttf_parser::Face::parse(&authored_bold.data, authored_bold.face_index)
            .is_ok_and(|face| face.weight().to_number() >= 700),
        "the authored bold stack must select its real bold face, not an unmarked regular face"
    );
}

/// A faux-oblique match must survive into PDF paint state only when CSS
/// permits style synthesis. A real italic face already contains the intended
/// glyph ink and therefore must not receive an additional shear.
/// <https://www.w3.org/TR/css-fonts-4/#font-synthesis-style>
#[tokio::test]
async fn font_synthesis_style_is_retained_per_selected_document_font() {
    let document = Html::from_file(FONT_SYNTHESIS_STYLE_PDF_FIXTURE)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let font_for = |text: &str| {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| {
                panic!(
                    "fixture line {text:?} should paint; got {:?}",
                    document.pages[0]
                        .lines()
                        .iter()
                        .map(|line| line.text.as_str())
                        .collect::<Vec<_>>()
                )
            });
        line_font(&document, line)
    };

    assert!(font_for("synthesized").synthesis.oblique.is_some());
    assert!(font_for("not").synthesis.oblique.is_none());
    let authored = font_for("authored italic");
    assert!(authored.synthesis.oblique.is_none());
    assert!(
        ttf_parser::Face::parse(&authored.data, authored.face_index)
            .is_ok_and(|face| face.is_italic()),
        "the authored italic stack must select its real italic face"
    );
}

/// `::first-line` becomes the inheritance parent for text in its generated
/// line box, including text owned by an otherwise unstyled inline descendant.
/// <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
#[tokio::test]
async fn first_line_font_synthesis_controls_reach_owned_and_nested_text() {
    let document = Html::from_file(FONT_SYNTHESIS_FIRST_LINE_PDF_FIXTURE)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let font_for = |text: &str| {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.contains(text))
            .unwrap_or_else(|| {
                panic!(
                    "fixture line {text:?} should paint; got {:?}",
                    document.pages[0]
                        .lines()
                        .iter()
                        .map(|line| line.text.as_str())
                        .collect::<Vec<_>>()
                )
            });
        line_font(&document, line)
    };

    for text in ["directstyle", "nestedstyle"] {
        assert!(font_for(text).synthesis.oblique.is_none(), "{text}");
    }
    for text in ["directweight", "nestedweight"] {
        assert!(!font_for(text).synthesis.embolden, "{text}");
    }
}

/// Each CSS variation location must retain its own PDF document-font record.
/// The embedded program is materialized later, but this shaping boundary must
/// not merge `wght` instances before the PDF writer can do so.
/// <https://www.w3.org/TR/css-fonts-4/#font-variation-settings-def>
#[tokio::test]
async fn variable_font_instances_retain_their_effective_axis_locations() {
    let document = Html::from_file(VARIABLE_FONT_INSTANCE_PDF_FIXTURE)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let font_for = |text: &str| {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("fixture line {text:?} should paint"));
        line_font(&document, line)
    };
    let weight = |font: &crate::document::DocumentFont| {
        font.variation_coordinates
            .0
            .iter()
            .find(|(tag, _)| tag == b"wght")
            .map(|(_, value)| f32::from_bits(*value))
    };

    let regular = font_for("regular");
    let bold = font_for("bold");
    let override_weight = font_for("override");
    assert_ne!(regular.id, bold.id);
    assert_ne!(bold.id, override_weight.id);
    assert_eq!(weight(regular), Some(400.0));
    assert_eq!(weight(bold), Some(700.0));
    assert_eq!(weight(override_weight), Some(550.0));
}

/// Root-relative font metrics are selected from the document root, not from
/// an enlarged ancestor of the element using the unit.
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
#[tokio::test]
async fn root_font_metric_units_ignore_an_intervening_monospace_ancestor() {
    let document = Html::from_file(ROOT_FONT_METRICS_IN_MONOSPACE_FIXTURE)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];

    for unit in ["rex", "rcap", "ric"] {
        let nested = page
            .lines()
            .iter()
            .find(|line| line.text == format!("nested-{unit}"))
            .expect("nested root-relative metric sample");
        let root = page
            .lines()
            .iter()
            .find(|line| line.text == format!("root-{unit}"))
            .expect("root-relative metric control sample");
        let ordinary_unit = unit.strip_prefix('r').expect("root unit prefix");
        let ordinary = page
            .lines()
            .iter()
            .find(|line| line.text == format!("parent-r{ordinary_unit}"))
            .expect("ordinary parent-metric control sample");
        assert!(
            (nested.font_size - root.font_size).abs() < 0.01,
            "1{unit} must use the root selected font rather than the monospace ancestor: nested={nested:?}, root={root:?}"
        );
        assert!(
            (root.font_size - ordinary.font_size).abs() < 0.01,
            "1{unit} must equal the corresponding parent-font 1{ordinary_unit}: root={root:?}, ordinary={ordinary:?}"
        );
    }
}

#[tokio::test]
async fn native_non_em_ascender_does_not_raise_a_following_block_into_the_previous_one() {
    assert_non_em_ascender_paint_stays_in_its_following_background(NON_EM_ASCENDER_PAINT_FIXTURE)
        .await;
}

#[tokio::test]
async fn native_non_em_ascender_does_not_raise_prepared_inline_text_into_the_previous_line() {
    assert_non_em_ascender_paint_stays_in_its_following_background(
        NON_EM_ASCENDER_PREPARED_PAINT_FIXTURE,
    )
    .await;
}

#[tokio::test]
async fn font_size_adjust_mixed_fallback_preserves_the_painted_baseline_conversion() {
    let document = Html::from_file(FONT_SIZE_ADJUST_MIXED_FALLBACK_BASELINE_FIXTURE)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    // Opaque full-em coverage owns only `X`, so the mixed logical line is
    // deliberately represented by two ordered paint records.
    let first = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "X")
        .expect("first selected-face paint segment");
    let second = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Y")
        .expect("second selected-face paint segment");
    let first_size = first.runs[0].font_size;
    let second_size = second.runs[0].font_size;
    assert!(
        (first_size - second_size).abs() > 0.01,
        "font-size-adjust should produce distinct used sizes for the selected faces: first={first:?}, second={second:?}"
    );
    let first_font = line_font(&document, first);
    let expected_adjustment = (first_font.layout_metrics.ascender
        - first_font.program_metrics.ascender) as f32
        * first_size
        / first_font.units_per_em as f32;
    assert!(
        (first.glyph_origin_adjustment.y - expected_adjustment).abs() < 0.01,
        "the rendered segment must retain the primary metric font's applied CSS-layout-to-program conversion: {first:?}"
    );
}

/// A full-em `@font-face` glyph may use opaque vector coverage, but the
/// coverage record must not make its adjacent unicode-range fallback glyphs
/// invisible. This is the local regression for WPT `size-adjust-01.html`.
#[tokio::test]
async fn size_adjust_opaque_coverage_keeps_mixed_fallback_runs_visible() {
    let document = Html::from_file(SIZE_ADJUST_MIXED_OPAQUE_COVERAGE_FIXTURE)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let lines = document.pages[0].lines();
    let fragments = lines
        .iter()
        .filter(|line| {
            ["T", "he", "Q", "uick", "B", "rown", "F", "ox"].contains(&line.text.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        fragments
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["T", "he", "Q", "uick", "B", "rown", "F", "ox"],
        "mixed text must retain ordered coverage and normal PDF paint records"
    );
    for line in fragments {
        let expected_size = if line.text.len() == 1 { 45.0 } else { 30.0 };
        assert!(
            (line.runs[0].font_size - expected_size).abs() < 0.01,
            "fragment {:?} should retain its selected used font size: {line:?}",
            line.text
        );
    }
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert_eq!(
        rendered.matches("3 Tr").count(),
        4,
        "only the four covered Ahem capitals may use invisible PDF text: {rendered}"
    );
    assert_eq!(
        rendered.matches("0 Tr").count(),
        4,
        "each normal fallback fragment must resume visible PDF text: {rendered}"
    );
}

async fn assert_non_em_ascender_paint_stays_in_its_following_background(fixture: &str) {
    let document = Html::from_file(fixture)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let following_line = page
        .lines()
        .iter()
        .find(|line| line.text == "Y")
        .expect("following Ahem block should paint its glyph");
    let following_background = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("following block background");
    let font = line_font(&document, following_line);
    let native_descent =
        font.program_metrics.descender as f32 * following_line.font_size / font.units_per_em as f32;

    assert!(
        (following_line.y() - (following_background.y() - native_descent)).abs() < 0.01,
        "following glyph must use its native baseline inside its own block, not rise into the preceding block: line={following_line:?}, background={following_background:?}, native_descent={native_descent}"
    );
}

/// Regression derived from WPT `css/css-fonts/ascent-descent-override.html`.
///
/// CSS metric override descriptors alter line layout, but their face's native
/// OpenType metrics must still position the embedded glyph program.
#[tokio::test]
async fn font_metric_overrides_keep_native_glyph_paint_baseline() {
    let document = Html::from_file(ASCENT_DESCENT_OVERRIDE_WPT)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let line = page
        .lines()
        .iter()
        .find(|line| line.text == "X")
        .expect("the Ahem glyph should be emitted as text");
    let font = line_font(&document, line);

    assert_eq!(font.program_metrics.ascender, 800);
    assert_eq!(font.program_metrics.descender, -200);
    assert_eq!(font.layout_metrics.ascender, 1000);
    assert_eq!(font.layout_metrics.descender, -500);

    let top_aligned = page
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - line.font_size).abs() < 0.01
                && (rect.height() - line.font_size).abs() < 0.01
        })
        .expect("top-aligned Ahem-sized box");
    let bottom_aligned = page
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - line.font_size).abs() < 0.01
                && (rect.height() - line.font_size * 0.5).abs() < 0.01
        })
        .expect("bottom-aligned half-em Ahem-sized box");
    assert!(
        (line.x() - (top_aligned.x() + top_aligned.width())).abs() < 0.01,
        "glyph should follow the top-aligned box"
    );
    let native_ascent =
        font.program_metrics.ascender as f32 * line.font_size / font.units_per_em as f32;
    let native_descent =
        font.program_metrics.descender as f32 * line.font_size / font.units_per_em as f32;
    assert!(
        line.y() + native_ascent <= top_aligned.y() + top_aligned.height() + 0.01
            && line.y() + native_descent >= bottom_aligned.y() - 0.01,
        "native Ahem glyph extents must remain inside the line box established by overridden CSS metrics"
    );
}

#[tokio::test]
async fn visibility_hidden_preserves_layout_space() {
    let options = RenderOptions::default();
    let document = Html::from_string(
        "<p style=\"margin: 0; visibility: hidden\">Hidden</p><p style=\"margin: 0\">Visible</p>",
    )
    .render(&options)
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines().len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "Visible");
    assert!(
        document.pages[0].lines()[0].y()
            < options.page_size.height()
                - crate::layout::PageMargins::DEFAULT.top()
                - options.line_height()
    );
}

#[tokio::test]
async fn supports_bold_and_italic_system_fonts() {
    let document = Html::from_string(
        "<h1>Heading</h1><p style=\"font-style: italic\">Emphasis</p><p style=\"font-weight: bold; font-style: italic\">Both</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(line_font_is_bold(&document, &document.pages[0].lines()[0]));
    assert!(line_font_is_italic(
        &document,
        &document.pages[0].lines()[1]
    ));
    assert!(line_font_is_bold(&document, &document.pages[0].lines()[2]));
    assert!(line_font_is_italic(
        &document,
        &document.pages[0].lines()[2]
    ));
}

#[tokio::test]
async fn supports_generic_system_font_families() {
    let document = Html::from_string(
        "<p style=\"font-family: serif; font-style: italic\">Serif</p><p style=\"font-family: monospace; font-weight: bold\">Mono</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_ne!(
        document.pages[0].lines()[0].font_id,
        document.pages[0].lines()[1].font_id
    );
    assert!(line_font_is_italic(
        &document,
        &document.pages[0].lines()[0]
    ));
    assert!(
        line_font_is_monospace(&document, &document.pages[0].lines()[1]),
        "resolved monospace font was {}",
        font_label(line_font(&document, &document.pages[0].lines()[1]))
    );
    let monospace_font = line_font(&document, &document.pages[0].lines()[1]);
    assert!(
        ttf_parser::Face::parse(&monospace_font.data, monospace_font.face_index)
            .is_ok_and(|face| face.is_outline_embedding_allowed()),
        "generic monospace font must permit PDF outline embedding"
    );
    assert!(line_font_is_bold(&document, &document.pages[0].lines()[1]));
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(!rendered.contains("/Subtype /Type1"));
}

#[tokio::test]
async fn supports_font_face_data_uri_fonts() {
    let font_data = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let font_data = base64::engine::general_purpose::STANDARD.encode(font_data);
    let html = format!(
        "<style>@font-face {{ font-family: SmokeFace; src: url(data:font/ttf;base64,{font_data}) format('truetype') }} p {{ font-family: SmokeFace }}</style><p>Font face</p>"
    );

    let document = Html::from_string(html)
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(document.fonts.iter().any(|font| font.family == "SmokeFace"));
    assert_eq!(
        line_font(&document, &document.pages[0].lines()[0]).family,
        "SmokeFace"
    );
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(rendered.contains("/ToUnicode"));
}

#[tokio::test]
async fn font_face_data_uri_falls_back_after_a_malformed_data_source() {
    let font_data = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let font_data = base64::engine::general_purpose::STANDARD.encode(font_data);
    let html = format!(
        "<style>@font-face {{ font-family: DataFallback; src: url(data:font/ttf;base64,%%%) format('truetype'), url(data:font/ttf;base64,{font_data}) format('truetype') }} p {{ font-family: DataFallback }}</style><p>Font face</p>"
    );

    let document = Html::from_string(html)
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        document
            .fonts
            .iter()
            .any(|font| font.family == "DataFallback")
    );
    assert_eq!(
        line_font(&document, &document.pages[0].lines()[0]).family,
        "DataFallback"
    );
}

#[tokio::test]
async fn supports_font_face_data_uri_woff1_fonts() {
    let font_data = woff1_from_sfnt(
        &std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap(),
    );
    let font_data = base64::engine::general_purpose::STANDARD.encode(font_data);
    let html = format!(
        "<style>@font-face {{ font-family: SmokeWoff; src: url(data:font/woff;base64,{font_data}) format('woff') }} p {{ font-family: SmokeWoff }}</style><p>Font face</p>"
    );

    let document = Html::from_string(html)
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(document.fonts.iter().any(|font| font.family == "SmokeWoff"));
    assert_eq!(
        line_font(&document, &document.pages[0].lines()[0]).family,
        "SmokeWoff"
    );
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/FontFile2"));
}

#[tokio::test]
async fn supports_font_face_opentype_cff_fonts() {
    let document = Html::from_file("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        document.fonts.iter().any(|font| {
            font_label_contains_any(font, &["barlow-condensed", "barlow condensed"])
        })
    );
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Subtype /CIDFontType0"));
    assert!(rendered.contains("/FontFile3"));
    // FontFile3 embeds the extracted CFF program rather than its surrounding
    // OpenType SFNT container, so PDF identifies it as CIDFontType0C.
    assert!(rendered.contains("/Subtype /CIDFontType0C"));
}

#[tokio::test]
async fn async_font_seed_loads_local_font_face_sources() {
    let html = Html::from_string(
        r#"<style>
            @font-face {
                font-family: AsyncLocalFace;
                src: url("weasyprint-samples/invoice/SourceSans3-Regular.ttf");
            }
            p { font-family: AsyncLocalFace }
        </style><p>Local font face</p>"#,
    )
    .with_base_path(".")
    .unwrap();

    let document = html.render(&RenderOptions::default()).await.unwrap();

    assert!(
        document
            .fonts
            .iter()
            .any(|font| font.family == "AsyncLocalFace")
    );
    assert_eq!(
        line_font(&document, &document.pages[0].lines()[0]).family,
        "AsyncLocalFace"
    );
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
}

#[tokio::test]
async fn font_size_adjust_greater_than_aspect_value_increases_rendered_size() {
    let html = Html::from_string(
        r#"
        <style>
          @font-face {
            font-family: FontSizeAdjustProbe;
            src: url("WeasyPrint/tests/resources/weasyprint.otf");
          }
          div {
            position: absolute;
            font: 40px/40px FontSizeAdjustProbe;
            color: orange;
          }
          #test {
            color: blue;
            font-size-adjust: 0.9;
          }
        </style>
        <div id="test">FillerText</div>
        <div>FillerText</div>
        "#,
    )
    .with_base_path(".")
    .unwrap();

    let document = html.render(&RenderOptions::default()).await.unwrap();
    let filler_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "FillerText")
        .collect::<Vec<_>>();
    assert_eq!(filler_lines.len(), 2);

    let adjusted = filler_lines
        .iter()
        .find(|line| line.runs.iter().any(|run| run.font_size > 30.1))
        .expect("adjusted line should use a larger rendered font size");
    let normal = filler_lines
        .iter()
        .find(|line| {
            line.runs
                .iter()
                .all(|run| (run.font_size - 30.0).abs() < 0.1)
        })
        .expect("unadjusted line should keep the specified 40px/30pt font size");

    assert!(adjusted.runs[0].font_size > normal.runs[0].font_size);
    assert!(rendered_line_advance(adjusted) > rendered_line_advance(normal));

    let reference_font_size_px = adjusted.runs[0].font_size / 0.75;
    let reference_html = Html::from_string(format!(
        r#"
        <style>
          @font-face {{
            font-family: FontSizeAdjustProbe;
            src: url("WeasyPrint/tests/resources/weasyprint.otf");
          }}
          div {{
            position: absolute;
            font: 40px/40px FontSizeAdjustProbe;
            color: orange;
          }}
          #test {{
            color: blue;
            font-size: {reference_font_size_px}px;
          }}
        </style>
        <div id="test">FillerText</div>
        <div>FillerText</div>
        "#,
    ))
    .with_base_path(".")
    .unwrap();
    let reference = reference_html
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let reference_adjusted = reference.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "FillerText")
        .find(|line| line.runs.iter().any(|run| run.font_size > 30.1))
        .expect("reference adjusted line");

    let adjusted_top = rendered_line_baseline_top(&document, adjusted);
    let reference_top = rendered_line_baseline_top(&reference, reference_adjusted);
    assert!(
        (adjusted_top - reference_top).abs() < 0.01,
        "adjusted_top={adjusted_top}, reference_top={reference_top}, adjusted={adjusted:?}, reference={reference_adjusted:?}"
    );
    assert!(
        (rendered_line_advance(adjusted) - rendered_line_advance(reference_adjusted)).abs() < 0.01
    );
}

#[tokio::test]
async fn async_font_seed_loads_system_font_context() {
    let document = Html::from_string("<p>System font seed</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let line = &document.pages[0].lines()[0];
    let font = line_font(&document, line);
    assert!(!font.data.is_empty());
    assert!(font.units_per_em > 0);
}

#[tokio::test]
async fn ttc_face_index_survives_query_shaping_and_embedding_when_available() {
    let Some(fixture) = system_ttc_text_face_fixture("TTC face index") else {
        eprintln!("No system TTC text face with a nonzero face index is available");
        return;
    };
    let document = Html::from_string(format!(
        "<style>p {{ font-family: \"{}\"; font-style: {}; font-weight: {}; font-width: {} }}</style><p>TTC face index</p>",
        fixture.family_css, fixture.font_style_css, fixture.font_weight, fixture.font_width_css
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let font = line_font(&document, &document.pages[0].lines()[0]);
    assert_eq!(font.face_index, fixture.face_index);
    assert_eq!(font.data.get(..4), Some(b"ttcf".as_slice()));
    assert!(
        document
            .write_pdf_bytes(&crate::PdfOptions::default())
            .unwrap()
            .windows(5)
            .any(|bytes| bytes == b"/Font")
    );
}

#[tokio::test]
async fn ticket_airplane_fallback_prefers_visible_unicode_text_font() {
    let document = Html::from_file("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    if !system_has_font_family("arial unicode") {
        eprintln!("Arial Unicode MS is not available on this system");
        return;
    }

    let airplane_run_font = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .flat_map(|line| &line.runs)
        .find(|run| run.text.contains('✈'))
        .and_then(|run| run.font_id)
        .and_then(|font_id| document.fonts.get(font_id))
        .expect("ticket airplane should be emitted as a fallback run");

    // The fixture requests Arial Unicode MS, but font discovery is permitted
    // to substitute an equivalent Unicode text face. What matters here is
    // that the fallback run remains a real, embeddable text font rather than
    // a missing-glyph placeholder.
    assert!(
        ttf_parser::Face::parse(&airplane_run_font.data, airplane_run_font.face_index).is_ok_and(
            |face| { face.glyph_index('✈').is_some() && face.is_outline_embedding_allowed() }
        ),
        "ticket airplane fallback must be an embeddable font with a visible airplane glyph: {}",
        font_label(airplane_run_font),
    );
}

#[tokio::test]
async fn ticket_pdf_prunes_unused_and_duplicate_embedded_fonts() {
    let pdf = Html::from_file("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
        .await
        .unwrap();

    assert!(
        pdf.len() < 50 * 1024 * 1024,
        "ticket PDF should be far below the previous 365MiB zero-glyph font embedding regression, got {} bytes",
        pdf.len()
    );
}

fn system_has_font_family(needle: &str) -> bool {
    let mut collection = fontique::Collection::default();
    collection
        .family_names()
        .any(|family| family.to_ascii_lowercase().contains(needle))
}

struct SystemTtcTextFaceFixture {
    family_css: String,
    face_index: u32,
    font_style_css: &'static str,
    font_weight: u16,
    font_width_css: &'static str,
}

struct FontQueryAttributes {
    style: fontique::FontStyle,
    style_css: &'static str,
    weight: f32,
    weight_css: u16,
    width_ratio: f32,
    width_css: &'static str,
}

fn system_ttc_text_face_fixture(text: &str) -> Option<SystemTtcTextFaceFixture> {
    let mut collection = fontique::Collection::default();
    let mut source_cache = fontique::SourceCache::default();
    let mut family_names = collection
        .family_names()
        .map(str::to_string)
        .collect::<Vec<_>>();
    family_names.sort_unstable();

    for family_name in family_names {
        for attributes in system_ttc_text_face_fixture_attributes() {
            let mut query = collection.query(&mut source_cache);
            query.set_families([fontique::QueryFamily::Named(&family_name)]);
            query.set_attributes(fontique::Attributes::new(
                fontique::FontWidth::from_ratio(attributes.width_ratio),
                attributes.style,
                fontique::FontWeight::new(attributes.weight),
            ));
            let mut first_usable_match = None;
            query.matches_with(|font| {
                if first_usable_match.is_none()
                    && !font.synthesis.any()
                    && ttc_text_query_font_can_shape(font, text)
                {
                    first_usable_match = Some((font.index, font.blob.clone()));
                    fontique::QueryStatus::Stop
                } else {
                    fontique::QueryStatus::Continue
                }
            });
            if let Some((face_index, blob)) = first_usable_match
                && face_index > 0
                && blob.as_ref().get(..4) == Some(b"ttcf")
            {
                return Some(SystemTtcTextFaceFixture {
                    family_css: css_string_escape(&family_name),
                    face_index,
                    font_style_css: attributes.style_css,
                    font_weight: attributes.weight_css,
                    font_width_css: attributes.width_css,
                });
            }
        }
    }

    None
}

fn system_ttc_text_face_fixture_attributes() -> Vec<FontQueryAttributes> {
    [
        (fontique::FontStyle::Normal, "normal"),
        (fontique::FontStyle::Italic, "italic"),
        (fontique::FontStyle::Oblique(Some(14.0)), "oblique"),
    ]
    .into_iter()
    .flat_map(|(style, style_css)| {
        [(400.0, 400), (700.0, 700), (300.0, 300), (500.0, 500)]
            .into_iter()
            .flat_map(move |(weight, weight_css)| {
                [
                    (1.0, "normal"),
                    (0.75, "condensed"),
                    (1.25, "expanded"),
                    (0.875, "semi-condensed"),
                    (1.125, "semi-expanded"),
                ]
                .into_iter()
                .map(move |(width_ratio, width_css)| FontQueryAttributes {
                    style,
                    style_css,
                    weight,
                    weight_css,
                    width_ratio,
                    width_css,
                })
            })
    })
    .collect()
}

fn ttc_text_query_font_can_shape(font: &fontique::QueryFont, text: &str) -> bool {
    let Ok(face) = ttf_parser::Face::parse(font.blob.as_ref(), font.index) else {
        return false;
    };
    face.is_outline_embedding_allowed()
        && text
            .chars()
            .filter(|character| !character.is_whitespace())
            .all(|character| face.glyph_index(character).is_some())
}

fn css_string_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[tokio::test]
async fn embeds_shaped_system_font_symbols_without_question_mark_fallbacks() {
    let pdf = Html::from_string("<p>© 2018 • Example® ≥7 cM ≤ 0.5</p>")
        .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
        .await
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(rendered.contains("/ToUnicode"));
    assert!(rendered.contains("<00A9>"));
    assert!(rendered.contains("<2022>"));
    assert!(rendered.contains("<00AE>"));
    assert!(rendered.contains("<2265>"));
    assert!(rendered.contains("<2264>"));
    assert!(!rendered.contains("(? 2018"));
    assert!(!rendered.contains("Example?"));
}

#[tokio::test]
async fn rendered_text_lines_preserve_shaped_glyphs_for_pdf() {
    let document = Html::from_string("<p>Example® ≥7</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("Example"))
        .unwrap();
    let glyphs = line
        .runs
        .iter()
        .flat_map(|run| run.glyphs.as_deref().unwrap_or_default())
        .collect::<Vec<_>>();

    assert!(!glyphs.is_empty());
    assert!(glyphs.iter().any(|glyph| glyph.unicode == "®"));
    assert!(glyphs.iter().any(|glyph| glyph.unicode == "≥"));
}

#[tokio::test]
async fn compatible_inline_fragments_shape_as_one_cursive_run() {
    let document = Html::from_string(
        "<style>p { margin: 0; font-size: 20pt; letter-spacing: 10pt }</style><p><span>ت</span><span>ف</span><span>ا</span><span>ح</span>ة</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("تفاحة") || line.text.contains("ةحافت"))
        .unwrap();

    assert!(
        line.runs
            .iter()
            .any(|run| run.text.contains("تفاح") || run.text.contains("حافت"))
    );
}

#[tokio::test]
async fn rtl_inline_margin_boundary_matches_zwnj_shaping_reference() {
    let render = |contents: &str| {
        Html::from_string(format!(
            "<style>\
             @page {{ size: 800px 300px; margin: 0 }}\
             @font-face {{ font-family: Naskh; src: url('tests/resources/fonts/NotoNaskhArabic-regular.woff2') format('woff2') }}\
             body {{ margin: 0 }}\
             div {{ border: 1px solid #02D7F6; margin: 20px; padding: 10px; width: 3em; font-size: 120px; font-family: Naskh }}\
             .margin {{ margin: 0.5em }}\
             </style><div lang=\"ar\" dir=\"rtl\">{contents}</div>"
        ))
        .with_base_path(".")
        .unwrap()
    };
    let target = render("ع<span class=\"margin\">ع</span>ع")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let reference = render("ع&zwnj;<span class=\"margin\">&zwnj;ع&zwnj;</span>&zwnj;ع")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(
        arabic_line_geometry(&target),
        arabic_line_geometry(&reference),
        "an inline-axis margin must isolate Arabic shaping without changing the visual placement of the span's two margin edges"
    );
}

#[tokio::test]
async fn rtl_inline_color_boundary_matches_zwj_shaping_reference() {
    let render = |contents: &str| {
        Html::from_string(format!(
            "<style>\
             @page {{ size: 800px 300px; margin: 0 }}\
             @font-face {{ font-family: Naskh; src: url('tests/resources/fonts/NotoNaskhArabic-regular.woff2') format('woff2') }}\
             body {{ margin: 0 }}\
             div {{ border: 1px solid #02D7F6; margin: 20px; padding: 10px; width: 3em; font-size: 120px; font-family: Naskh }}\
             .color {{ color: blue }}\
             </style><div lang=\"ar\" dir=\"rtl\">{contents}</div>"
        ))
        .with_base_path(".")
        .unwrap()
    };
    let target = render("ع<span class=\"color\">ع</span>ع")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let reference = render("ع&zwj;<span class=\"color\">&zwj;ع&zwj;</span>&zwj;ع")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(
        arabic_line_geometry(&target),
        arabic_line_geometry(&reference),
        "a paint-only color boundary and its ZWJ reference must use the same RTL line measure"
    );
}

type ArabicLineGeometry = (i32, i32, Vec<(u16, i32)>);

fn arabic_line_geometry(document: &quire::Document) -> Vec<ArabicLineGeometry> {
    let mut lines = document.pages[0]
        .lines()
        .iter()
        .filter_map(|line| {
            let glyphs = line
                .runs
                .iter()
                .flat_map(|run| run.glyphs.as_deref().unwrap_or_default())
                .filter(|glyph| glyph.unicode.contains('ع'))
                .map(|glyph| {
                    (
                        glyph.painted_id().expect("paintable glyph"),
                        (glyph.x_advance * 1_000.0).round() as i32,
                    )
                })
                .collect::<Vec<_>>();
            (!glyphs.is_empty()).then(|| {
                (
                    (line.x() * 1_000.0).round() as i32,
                    (line.y() * 1_000.0).round() as i32,
                    glyphs,
                )
            })
        })
        .collect::<Vec<_>>();
    lines.sort_unstable_by_key(|line| (line.1, line.0));
    lines
}

#[tokio::test]
async fn font_style_boundary_preserves_arabic_shaping_context() {
    let document = Html::from_string(
        "<style>\
         div { margin: 0; font-size: 48pt; font-family: sans-serif; direction: rtl }\
         .fontstyle { font-style: italic }\
         </style>\
         <div lang=\"ar\">ع<span class=\"fontstyle\">ع</span>ع</div>\
         <div lang=\"ar\">ع&zwj;<span class=\"fontstyle\">&zwj;ع&zwj;</span>&zwj;ع</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let arabic_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text.contains('ع'))
        .collect::<Vec<_>>();
    assert_eq!(arabic_lines.len(), 2);

    let glyph_ids = arabic_lines
        .iter()
        .map(|line| {
            line.runs
                .iter()
                .flat_map(|run| run.glyphs.as_deref().unwrap_or_default())
                .filter(|glyph| glyph.unicode.contains('ع'))
                .map(|glyph| glyph.painted_id().expect("paintable glyph"))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if glyph_ids.iter().any(|ids| ids.is_empty()) {
        eprintln!("no Arabic shaped glyph IDs available on this system");
        return;
    }

    assert_eq!(glyph_ids[0], glyph_ids[1]);
}

#[tokio::test]
async fn local_alreq_text_encoding_subset_matches_presentation_forms() {
    let variants = [
        AlreqVariant::plain("shaping-join-001", AlreqExpectation::Join),
        AlreqVariant::unicode_range(
            "shaping-join-002",
            AlreqExpectation::Join,
            AlreqSpecialFont::JoinControls,
        ),
        AlreqVariant::explicit(
            "shaping-join-003",
            AlreqExpectation::Join,
            AlreqSpecialFont::JoinControls,
        ),
        AlreqVariant::plain("shaping-no-join-001", AlreqExpectation::NoJoin),
        AlreqVariant::unicode_range(
            "shaping-no-join-002",
            AlreqExpectation::NoJoin,
            AlreqSpecialFont::JoinControls,
        ),
        AlreqVariant::explicit(
            "shaping-no-join-003",
            AlreqExpectation::NoJoin,
            AlreqSpecialFont::JoinControls,
        ),
        AlreqVariant::plain("shaping-tatweel-001", AlreqExpectation::Tatweel),
        AlreqVariant::unicode_range(
            "shaping-tatweel-002",
            AlreqExpectation::Tatweel,
            AlreqSpecialFont::Tatweel,
        ),
        AlreqVariant::explicit(
            "shaping-tatweel-003",
            AlreqExpectation::Tatweel,
            AlreqSpecialFont::Tatweel,
        ),
    ];

    for variant in variants {
        for (index, (actual_text, reference_text)) in variant.cases().into_iter().enumerate() {
            let actual_document = Html::from_string(variant.html_for_text(actual_text))
                .with_base_path(".")
                .unwrap()
                .render(&RenderOptions::default())
                .await
                .unwrap();
            let reference_document = Html::from_string(variant.html_for_text(reference_text))
                .with_base_path(".")
                .unwrap()
                .render(&RenderOptions::default())
                .await
                .unwrap();
            let actual = alreq_first_line_glyphs(&actual_document).unwrap_or_else(|| {
                panic!(
                    "{} row {} actual should produce visible glyphs: {:?}",
                    variant.name,
                    index,
                    actual_document
                        .pages
                        .iter()
                        .flat_map(|page| page.lines().iter())
                        .collect::<Vec<_>>()
                )
            });
            let reference = alreq_first_line_glyphs(&reference_document).unwrap_or_else(|| {
                panic!(
                    "{} row {} reference should produce visible glyphs: {:?}",
                    variant.name,
                    index,
                    reference_document
                        .pages
                        .iter()
                        .flat_map(|page| page.lines().iter())
                        .collect::<Vec<_>>()
                )
            });
            assert_eq!(
                actual.visible_glyphs, reference.visible_glyphs,
                "{} row {} should match its presentation-form reference: actual {:?}, reference {:?}",
                variant.name, index, actual, reference
            );
            if matches!(
                variant.expectation,
                AlreqExpectation::Join | AlreqExpectation::NoJoin
            ) {
                assert!(
                    !actual
                        .unicode
                        .iter()
                        .any(|text| text.contains('\u{200c}') || text.contains('\u{200d}')),
                    "{} row {} should not emit join-control glyphs: {:?}",
                    variant.name,
                    index,
                    actual
                );
            }
        }
    }
}

#[tokio::test]
async fn explicit_cross_font_zwnj_table_cells_match_presentation_forms() {
    let variant = AlreqVariant::explicit(
        "shaping-no-join-003",
        AlreqExpectation::NoJoin,
        AlreqSpecialFont::JoinControls,
    );
    let rows = variant
        .cases()
        .into_iter()
        .map(|(actual, reference)| {
            format!(
                "<tr><td>{}<td>{}",
                variant.markup(actual),
                variant.markup(reference)
            )
        })
        .collect::<String>();
    let html = format!(
        r#"<style>
            @font-face {{ font-family: AlreqArabic; src: url('tests/resources/fonts/NotoNaskhArabic-regular.woff2') format('woff2'); }}
            @font-face {{ font-family: AlreqJoinControls; src: url('tests/resources/fonts/noto-sans-v8-latin-regular.woff') format('woff'); }}
            table {{ font-family: AlreqArabic; font-size: 3em; border-spacing: 0 3px; }}
            td {{ padding: 0 0.5ch; line-height: 1; border: 1px solid; }}
            .special {{ font-family: AlreqJoinControls; line-height: 0; }}
           </style><table dir=rtl lang=ar>{rows}</table>"#,
    );
    let document = Html::from_string(html)
        .with_base_path(".")
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let lines = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
        .filter_map(|line| {
            let visible_glyphs = line
                .runs
                .iter()
                .flat_map(|run| run.glyphs.as_deref().unwrap_or_default())
                .filter(|glyph| glyph.x_advance != 0.0)
                .map(|glyph| glyph.painted_id().expect("paintable glyph"))
                .collect::<Vec<_>>();
            (!visible_glyphs.is_empty()).then(|| {
                let unicode = line
                    .runs
                    .iter()
                    .flat_map(|run| run.glyphs.as_deref().unwrap_or_default())
                    .filter(|glyph| glyph.x_advance != 0.0)
                    .map(|glyph| glyph.unicode.clone())
                    .collect::<Vec<_>>();
                AlreqLineGlyphs {
                    visible_glyphs,
                    unicode,
                }
            })
        })
        .collect::<Vec<_>>();

    assert_eq!(lines.len(), 12, "six actual/reference table-cell pairs");
    for (index, pair) in lines.as_chunks::<2>().0.iter().enumerate() {
        assert_eq!(
            pair[0].visible_glyphs, pair[1].visible_glyphs,
            "table row {index} should match its presentation-form reference"
        );
        assert!(
            !pair[0].unicode.iter().any(|text| text.contains('\u{200c}')),
            "table row {index} must not emit a visible ZWNJ glyph"
        );
    }
}

#[tokio::test]
async fn uses_later_font_family_for_missing_glyph_runs() {
    let Some((primary_font, fallback_font, fallback_character)) = fallback_font_fixture() else {
        eprintln!("no standalone system TrueType fallback-font fixture available");
        return;
    };
    let primary_font = base64::engine::general_purpose::STANDARD.encode(primary_font);
    let fallback_font = base64::engine::general_purpose::STANDARD.encode(fallback_font);
    let html = format!(
        "<style>\
         @font-face {{ font-family: PrimarySmokeFace; src: url(data:font/ttf;base64,{primary_font}) format('truetype') }}\
         @font-face {{ font-family: FallbackSmokeFace; src: url(data:font/ttf;base64,{fallback_font}) format('truetype') }}\
         p {{ margin: 0; font-family: PrimarySmokeFace, FallbackSmokeFace; font-size: 12pt }}\
         </style><p>A {fallback_character}</p>"
    );
    let document = Html::from_string(&html)
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let line = document
        .pages
        .first()
        .and_then(|page| {
            page.lines()
                .iter()
                .find(|line| line.text.contains('A') && line.text.contains(fallback_character))
        })
        .expect("text with fallback glyph should remain one logical rendered line");
    let distinct_font_ids = line
        .runs
        .iter()
        .filter_map(|run| run.font_id)
        .collect::<std::collections::BTreeSet<_>>();

    assert!(
        line.runs
            .iter()
            .any(|run| run.text.contains(fallback_character)),
        "fallback character should be emitted as its own shaped text run"
    );
    assert!(
        distinct_font_ids.len() >= 2,
        "missing glyph should switch to a later CSS font-family face"
    );

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    let encoded_fallback = fallback_character
        .encode_utf16(&mut [0; 2])
        .iter()
        .map(|unit| format!("{unit:04X}"))
        .collect::<String>();
    assert!(
        rendered.contains(&format!("<{encoded_fallback}>")),
        "missing fallback character {encoded_fallback} from PDF text: {rendered}"
    );
}

#[tokio::test]
async fn explicit_line_height_baseline_ignores_fallback_font_runs() {
    let primary = "weasyprint-samples/invoice/SourceSans3-Regular.ttf";
    let fallback = "weasyprint-samples/letter/fonts/Pacifico-Regular.ttf";
    let primary_data = std::fs::read(primary).unwrap();
    let fallback_data = std::fs::read(fallback).unwrap();
    let primary_face = ttf_parser::Face::parse(&primary_data, 0).unwrap();
    let fallback_face = ttf_parser::Face::parse(&fallback_data, 0).unwrap();
    assert_ne!(
        primary_face.ascender(),
        fallback_face.ascender(),
        "fixture fonts should have different ascenders"
    );

    let html = format!(
        r#"
        <style>
          @page {{ size: 240pt 160pt; margin: 0 }}
          body {{ margin: 0 }}
          p {{ margin: 0; height: 20pt }}
          @font-face {{
            font-family: PrimaryAOnly;
            src: url("{primary}") format("truetype");
            unicode-range: U+0020, U+0061;
          }}
          @font-face {{
            font-family: FallbackBOnly;
            src: url("{fallback}") format("truetype");
            unicode-range: U+0062;
          }}
          div {{
            position: absolute;
            top: 20pt;
            left: 0;
            line-height: 75pt;
            font-size: 75pt;
            width: 225pt;
            text-align: right;
            color: transparent;
          }}
          span {{
            display: inline-block;
            width: 15pt;
            height: 15pt;
          }}
          #hd {{ font-family: PrimaryAOnly, FallbackBOnly; }}
          #hd span {{ background: red; }}
          #h {{ font-family: PrimaryAOnly; }}
          #h span {{ background: white; }}
        </style>
        <p>Test passes if there is no red below.</p>
        <div id="hd">ab<span></span></div>
        <div id="h">aa<span></span></div>
        "#
    );
    let document = Html::from_string(html)
        .with_base_path(".")
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("red fallback-baseline probe should paint");
    let white = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::WHITE))
        .expect("white reference probe should paint");

    assert!(
        (red.x() - white.x()).abs() < 0.01,
        "red={red:?} white={white:?}"
    );
    assert!(
        (red.y() - white.y()).abs() < 0.01,
        "red={red:?} white={white:?}"
    );
    assert!(
        (red.width() - white.width()).abs() < 0.01,
        "red={red:?} white={white:?}"
    );
    assert!(
        (red.height() - white.height()).abs() < 0.01,
        "red={red:?} white={white:?}"
    );
    assert!(
        first_rect_paint_operation_index(page, CssColor::WHITE)
            > first_rect_paint_operation_index(page, CssColor::new(255, 0, 0)),
        "white reference should paint over the red fallback-baseline probe"
    );
}

/// A fallback glyph selected through `unicode-range` contributes its face's
/// extents to an auto-height inline-block's normal line box.
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>
#[tokio::test]
async fn normal_line_height_inline_block_includes_selected_fallback_metrics() {
    let primary = "weasyprint-samples/invoice/SourceSans3-Regular.ttf";
    let fallback = "weasyprint-samples/letter/fonts/Pacifico-Regular.ttf";
    let html = format!(
        r#"
        <style>
          @page {{ size: 240pt 160pt; margin: 0 }}
          body {{ margin: 0 }}
          @font-face {{
            font-family: PrimaryAOnly;
            src: url("{primary}") format("truetype");
            unicode-range: U+0020, U+0061;
          }}
          @font-face {{
            font-family: FallbackBOnly;
            src: url("{fallback}") format("truetype");
            unicode-range: U+0062;
          }}
          .probe {{
            position: absolute;
            top: 20pt;
            left: 0;
            display: inline-block;
            width: 225pt;
            font-size: 75pt;
            text-align: right;
            color: transparent;
          }}
          #mixed {{ font-family: PrimaryAOnly, FallbackBOnly; background: red; }}
          #primary {{ font-family: PrimaryAOnly; background: white; }}
        </style>
        <div id="mixed" class="probe"><span>ab</span></div>
        <div id="primary" class="probe"><span>aa</span></div>
        "#
    );
    let document = Html::from_string(html)
        .with_base_path(".")
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("mixed-fallback inline background should paint");
    let white = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::WHITE))
        .expect("primary-only inline background should paint");

    assert!(
        red.y() < white.y(),
        "selected fallback metrics must raise the inline background: red={red:?} white={white:?}"
    );
    assert!(
        red.height() > white.height(),
        "selected fallback metrics must expand the inline background height: red={red:?} white={white:?}"
    );
}

/// A forced empty line has no paintable selected-font run, so its normal line
/// box retains the containing block's strut rather than selected-face extents.
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>
#[tokio::test]
async fn forced_empty_line_retains_parent_strut_without_selected_font_run() {
    let primary = "weasyprint-samples/invoice/SourceSans3-Regular.ttf";
    let fallback = "weasyprint-samples/letter/fonts/Pacifico-Regular.ttf";
    let html = format!(
        r#"
        <style>
          @page {{ size: 240pt 160pt; margin: 0 }}
          body {{ margin: 0 }}
          @font-face {{
            font-family: PrimaryAOnly;
            src: url("{primary}") format("truetype");
            unicode-range: U+0020, U+0061;
          }}
          @font-face {{
            font-family: FallbackBOnly;
            src: url("{fallback}") format("truetype");
            unicode-range: U+0062;
          }}
          div {{
            position: absolute;
            top: 20pt;
            left: 0;
            width: 225pt;
            font-size: 75pt;
            line-height: normal;
            color: transparent;
          }}
          #empty {{
            font-family: PrimaryAOnly, FallbackBOnly;
            background: red;
          }}
          #primary {{ font-family: PrimaryAOnly; background: white; }}
        </style>
        <div id="empty"><br></div>
        <div id="primary">aa</div>
        "#
    );
    let document = Html::from_string(html)
        .with_base_path(".")
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("empty-line background should paint");
    let white = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::WHITE))
        .expect("primary strut reference should paint");

    assert!(
        (red.height() - 90.0).abs() < 0.01,
        "empty line must keep the parent normal-line strut: red={red:?}"
    );
    assert!(
        red.height() < white.height() && red.y() > white.y(),
        "a no-font-run line must not borrow selected-face extents: red={red:?} white={white:?}"
    );
}

#[tokio::test]
async fn css2_explicit_line_height_baseline_wpt_overlay_hides_fallback_probe() {
    let primary = "weasyprint-samples/invoice/SourceSans3-Regular.ttf";
    let fallback = "weasyprint-samples/letter/fonts/Pacifico-Regular.ttf";
    let primary_data = std::fs::read(primary).unwrap();
    let fallback_data = std::fs::read(fallback).unwrap();
    let primary_face = ttf_parser::Face::parse(&primary_data, 0).unwrap();
    let fallback_face = ttf_parser::Face::parse(&fallback_data, 0).unwrap();
    assert_ne!(
        primary_face.ascender(),
        fallback_face.ascender(),
        "fixture fonts should have different ascenders"
    );

    let primary_woff =
        base64::engine::general_purpose::STANDARD.encode(woff1_from_sfnt(&primary_data));
    let fallback_woff =
        base64::engine::general_purpose::STANDARD.encode(woff1_from_sfnt(&fallback_data));
    let html = format!(
        r#"
        <!DOCTYPE html>
        <meta charset="utf-8">
        <style>
        @page {{ size: 720px 400px; margin: 0 }}
        body {{ margin: 8px }}
        @font-face {{
          font-family: 'high-a-only';
          font-style: normal;
          font-weight: 400;
          src: url(data:font/woff;base64,{primary_woff}) format('woff');
          unicode-range: U+0020, U+0061;
        }}
        @font-face {{
          font-family: 'deep-b-only';
          font-style: normal;
          font-weight: 400;
          src: url(data:font/woff;base64,{fallback_woff}) format('woff');
          unicode-range: U+0062;
        }}

        div {{
          position: absolute;
          line-height: 100px;
          font-size: 100px;
          width: 300px;
          text-align: right;
          color: transparent;
        }}
        span {{
          display: inline-block;
          width: 20px;
          height: 20px;
        }}
        #hd {{ font-family: high-a-only, deep-b-only; }}
        #hd span {{ background: red; }}
        #h {{ font-family: high-a-only; }}
        #h span {{ background: white; }}
        </style>

        <p>Test passes if there is <strong>no red</strong> below.
        <div id="hd">ab<span></span></div>
        <div id="h">aa<span></span></div>
        "#
    );
    let document = Html::from_string(html)
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("red fallback-baseline probe should paint");
    let white = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::WHITE))
        .expect("white reference probe should paint");

    assert!(
        (red.x() - white.x()).abs() < 0.01,
        "red={red:?} white={white:?}"
    );
    assert!(
        (red.y() - white.y()).abs() < 0.01,
        "red={red:?} white={white:?}"
    );
    assert!(
        (red.width() - white.width()).abs() < 0.01,
        "red={red:?} white={white:?}"
    );
    assert!(
        (red.height() - white.height()).abs() < 0.01,
        "red={red:?} white={white:?}"
    );
    assert!(
        first_rect_paint_operation_index(page, CssColor::WHITE)
            > first_rect_paint_operation_index(page, CssColor::new(255, 0, 0)),
        "white reference should paint over the red fallback-baseline probe"
    );
}

fn woff1_from_sfnt(sfnt: &[u8]) -> Vec<u8> {
    let table_count = u16::from_be_bytes(sfnt[4..6].try_into().unwrap()) as usize;
    let tables = (0..table_count)
        .map(|index| {
            let record = 12 + index * 16;
            let tag = sfnt[record..record + 4].to_vec();
            let checksum = u32::from_be_bytes(sfnt[record + 4..record + 8].try_into().unwrap());
            let offset =
                u32::from_be_bytes(sfnt[record + 8..record + 12].try_into().unwrap()) as usize;
            let len =
                u32::from_be_bytes(sfnt[record + 12..record + 16].try_into().unwrap()) as usize;
            (tag, checksum, sfnt[offset..offset + len].to_vec())
        })
        .collect::<Vec<_>>();

    let mut output = vec![0; 44 + table_count * 20];
    output[0..4].copy_from_slice(b"wOFF");
    output[4..8].copy_from_slice(&sfnt[0..4]);
    output[12..14].copy_from_slice(&(table_count as u16).to_be_bytes());
    output[16..20].copy_from_slice(&(sfnt.len() as u32).to_be_bytes());
    output[20..22].copy_from_slice(&1u16.to_be_bytes());

    for (index, (tag, checksum, data)) in tables.iter().enumerate() {
        let offset = align_to_four(output.len());
        output.resize(offset, 0);
        output.extend_from_slice(data);
        let record = 44 + index * 20;
        output[record..record + 4].copy_from_slice(tag);
        output[record + 4..record + 8].copy_from_slice(&(offset as u32).to_be_bytes());
        output[record + 8..record + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
        output[record + 12..record + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        output[record + 16..record + 20].copy_from_slice(&checksum.to_be_bytes());
    }
    let len = output.len() as u32;
    output[8..12].copy_from_slice(&len.to_be_bytes());
    output
}

fn align_to_four(value: usize) -> usize {
    (value + 3) & !3
}

fn fallback_font_fixture() -> Option<(Vec<u8>, Vec<u8>, char)> {
    const CHARACTERS: &[char] = &[
        'Ω', 'Ж', '∑', '€', '≤', '≥', '→', '漢', '字', 'अ', 'م', 'א', '♠',
    ];

    let fonts = standalone_system_font_faces()
        .into_iter()
        .filter_map(|(data, face_index)| {
            let parsed = ttf_parser::Face::parse(&data, face_index).ok()?;
            let has_latin = parsed.glyph_index('A').is_some();
            let supported = CHARACTERS
                .iter()
                .copied()
                .filter(|character| parsed.glyph_index(*character).is_some())
                .collect::<std::collections::BTreeSet<_>>();
            Some((data, has_latin, supported))
        })
        .collect::<Vec<_>>();

    for character in CHARACTERS {
        let primary = fonts
            .iter()
            .find(|(_, has_latin, supported)| *has_latin && !supported.contains(character));
        let fallback = fonts
            .iter()
            .find(|(_, _, supported)| supported.contains(character));
        if let (Some((primary, _, _)), Some((fallback, _, _))) = (primary, fallback) {
            return Some((primary.clone(), fallback.clone(), *character));
        }
    }

    None
}

#[derive(Clone, Copy)]
enum AlreqExpectation {
    Join,
    NoJoin,
    Tatweel,
}

#[derive(Clone, Copy)]
enum AlreqSpecialFont {
    JoinControls,
    Tatweel,
}

#[derive(Clone, Copy)]
enum AlreqFontMode {
    Plain,
    UnicodeRange(AlreqSpecialFont),
    Explicit(AlreqSpecialFont),
}

#[derive(Clone, Copy)]
struct AlreqVariant {
    name: &'static str,
    expectation: AlreqExpectation,
    mode: AlreqFontMode,
}

impl AlreqVariant {
    fn plain(name: &'static str, expectation: AlreqExpectation) -> Self {
        Self {
            name,
            expectation,
            mode: AlreqFontMode::Plain,
        }
    }

    fn unicode_range(
        name: &'static str,
        expectation: AlreqExpectation,
        special_font: AlreqSpecialFont,
    ) -> Self {
        Self {
            name,
            expectation,
            mode: AlreqFontMode::UnicodeRange(special_font),
        }
    }

    fn explicit(
        name: &'static str,
        expectation: AlreqExpectation,
        special_font: AlreqSpecialFont,
    ) -> Self {
        Self {
            name,
            expectation,
            mode: AlreqFontMode::Explicit(special_font),
        }
    }

    fn html_for_text(self, text: &'static str) -> String {
        let special_font = match self.mode {
            AlreqFontMode::Plain => AlreqSpecialFont::JoinControls,
            AlreqFontMode::UnicodeRange(special_font) | AlreqFontMode::Explicit(special_font) => {
                special_font
            }
        };
        let stack = match self.mode {
            AlreqFontMode::Plain | AlreqFontMode::Explicit(_) => "AlreqArabic",
            AlreqFontMode::UnicodeRange(_) => "AlreqPrimary, AlreqSpecial, AlreqArabic",
        };
        let case = format!(
            "<p class=\"case\" dir=\"rtl\" lang=\"ar\">{}</p>",
            self.markup(text)
        );
        format!(
            "<style>\
             @font-face {{ font-family: AlreqPrimary; src: url('tests/resources/fonts/NotoNaskhArabic-regular.woff2') format('woff2'); unicode-range: U+20; }}\
             @font-face {{ font-family: AlreqSpecial; src: url('{}') format('{}'); unicode-range: {}; }}\
             @font-face {{ font-family: AlreqArabic; src: url('tests/resources/fonts/NotoNaskhArabic-regular.woff2') format('woff2'); }}\
             body {{ margin: 0; }}\
             .case {{ margin: 0; font-family: {stack}; font-size: 20pt; line-height: 24pt; }}\
             .special {{ font-family: AlreqSpecial; line-height: 0; }}\
             </style>{case}",
            match special_font {
                AlreqSpecialFont::JoinControls => {
                    "tests/resources/fonts/noto-sans-v8-latin-regular.woff"
                }
                AlreqSpecialFont::Tatweel => "tests/resources/fonts/Scheherazade-Regular.woff",
            },
            "woff",
            match special_font {
                AlreqSpecialFont::JoinControls => "U+200C-200D",
                AlreqSpecialFont::Tatweel => "U+0640",
            }
        )
    }

    fn cases(self) -> Vec<(&'static str, &'static str)> {
        match self.expectation {
            AlreqExpectation::Join => vec![
                ("\u{200d}\u{0627}\u{200d}", "\u{fe8e}"),
                ("\u{200d}\u{0627}", "\u{fe8e}"),
                ("\u{0628}\u{200d}", "\u{fe91}"),
                ("\u{200d}\u{0628}\u{200d}", "\u{fe92}"),
                ("\u{200d}\u{0628}", "\u{fe90}"),
            ],
            AlreqExpectation::NoJoin => vec![
                ("\u{0640}\u{200c}\u{0627}", "\u{0640}\u{fe8d}"),
                ("\u{0628}\u{200c}\u{0640}", "\u{fe8f}\u{0640}"),
                (
                    "\u{0640}\u{0628}\u{200c}\u{0640}",
                    "\u{0640}\u{fe90}\u{0640}",
                ),
                (
                    "\u{0640}\u{200c}\u{0628}\u{200c}\u{0640}",
                    "\u{0640}\u{fe8f}\u{0640}",
                ),
                (
                    "\u{0640}\u{200c}\u{0628}\u{0640}",
                    "\u{0640}\u{fe91}\u{0640}",
                ),
                ("\u{0640}\u{200c}\u{0628}", "\u{0640}\u{fe8f}"),
            ],
            AlreqExpectation::Tatweel => vec![
                ("\u{0640}\u{0627}\u{0640}", "\u{0640}\u{fe8e}\u{0640}"),
                ("\u{0640}\u{0627}", "\u{0640}\u{fe8e}"),
                ("\u{0628}\u{0640}", "\u{fe91}\u{0640}"),
                ("\u{0640}\u{0628}\u{0640}", "\u{0640}\u{fe92}\u{0640}"),
                ("\u{0640}\u{0628}", "\u{0640}\u{fe90}"),
            ],
        }
    }

    fn markup(self, text: &'static str) -> String {
        match self.mode {
            AlreqFontMode::Explicit(AlreqSpecialFont::JoinControls) => text
                .chars()
                .map(|character| match character {
                    '\u{200c}' | '\u{200d}' => {
                        format!(
                            "<span class=\"special\">{}</span>",
                            alreq_numeric_character_reference(character)
                        )
                    }
                    _ => alreq_numeric_character_reference(character),
                })
                .collect(),
            AlreqFontMode::Explicit(AlreqSpecialFont::Tatweel) => text
                .chars()
                .map(|character| match character {
                    '\u{0640}' => {
                        format!(
                            "<span class=\"special\">{}</span>",
                            alreq_numeric_character_reference(character)
                        )
                    }
                    _ => alreq_numeric_character_reference(character),
                })
                .collect(),
            AlreqFontMode::Plain | AlreqFontMode::UnicodeRange(_) => text
                .chars()
                .map(alreq_numeric_character_reference)
                .collect(),
        }
    }
}

fn alreq_numeric_character_reference(character: char) -> String {
    format!("&#x{:X};", character as u32)
}

#[derive(Debug)]
struct AlreqLineGlyphs {
    visible_glyphs: Vec<u16>,
    unicode: Vec<String>,
}

fn alreq_first_line_glyphs(document: &quire::Document) -> Option<AlreqLineGlyphs> {
    document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
        .filter(|line| {
            line.runs
                .iter()
                .flat_map(|run| run.glyphs.as_deref().unwrap_or_default())
                .any(|glyph| glyph.x_advance != 0.0)
        })
        .map(|line| AlreqLineGlyphs {
            visible_glyphs: line
                .runs
                .iter()
                .flat_map(|run| {
                    run.glyphs
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .filter(|glyph| glyph.x_advance != 0.0)
                        .map(|glyph| glyph.painted_id().expect("paintable glyph"))
                })
                .collect(),
            unicode: line
                .runs
                .iter()
                .flat_map(|run| run.glyphs.as_deref().unwrap_or_default())
                .filter(|glyph| glyph.x_advance != 0.0)
                .map(|glyph| glyph.unicode.clone())
                .collect(),
        })
        .next()
}

fn standalone_system_font_faces() -> Vec<(Vec<u8>, u32)> {
    let mut collection = fontique::Collection::default();
    let mut source_cache = fontique::SourceCache::default();
    let family_names = collection
        .family_names()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut faces = Vec::new();

    for family_name in family_names {
        let mut query = collection.query(&mut source_cache);
        query.set_families([fontique::QueryFamily::Named(&family_name)]);
        query.matches_with(|font| {
            let data = font.blob.as_ref();
            if matches!(
                data.get(..4),
                Some(b"\x00\x01\x00\x00") | Some(b"true") | Some(b"typ1")
            ) {
                faces.push((data.to_vec(), font.index));
            }
            fontique::QueryStatus::Continue
        });
    }

    // Fontique's collection order is platform-dependent. The fallback fixture
    // selects the first matching primary/fallback pair, so normalize its input
    // order before making that observable test choice.
    faces.sort_unstable_by(|(left_data, left_index), (right_data, right_index)| {
        left_data.cmp(right_data).then(left_index.cmp(right_index))
    });
    faces.dedup();
    faces
}
