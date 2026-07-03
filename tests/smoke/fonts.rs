use super::*;

#[tokio::test]
async fn visibility_hidden_preserves_layout_space() {
    let options = RenderOptions::default();
    let document = Html::from_string(
        "<p style=\"margin: 0; visibility: hidden\">Hidden</p><p style=\"margin: 0\">Visible</p>",
    )
    .render_async(&options)
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines().len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "Visible");
    assert!(
        document.pages[0].lines()[0].y()
            < options.page_size.height() - options.page_margins.top() - options.line_height()
    );
}

#[tokio::test]
async fn supports_bold_and_italic_system_fonts() {
    let document = Html::from_string(
        "<h1>Heading</h1><p style=\"font-style: italic\">Emphasis</p><p style=\"font-weight: bold; font-style: italic\">Both</p>",
    )
    .render_async(&RenderOptions::default()).await
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
    .render_async(&RenderOptions::default()).await
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
    assert!(line_font_is_bold(&document, &document.pages[0].lines()[1]));
    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
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
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert!(document.fonts.iter().any(|font| font.family == "SmokeFace"));
    assert_eq!(
        line_font(&document, &document.pages[0].lines()[0]).family,
        "SmokeFace"
    );
    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(rendered.contains("/ToUnicode"));
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
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert!(document.fonts.iter().any(|font| font.family == "SmokeWoff"));
    assert_eq!(
        line_font(&document, &document.pages[0].lines()[0]).family,
        "SmokeWoff"
    );
    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/FontFile2"));
}

#[tokio::test]
async fn supports_font_face_opentype_cff_fonts() {
    let document = Html::from_file_async("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        document.fonts.iter().any(|font| {
            font_label_contains_any(font, &["barlow-condensed", "barlow condensed"])
        })
    );
    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/Subtype /CIDFontType0"));
    assert!(rendered.contains("/FontFile3"));
    assert!(rendered.contains("/Subtype /OpenType"));
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
    .with_base_url(".");

    let document = html.render_async(&RenderOptions::default()).await.unwrap();

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
    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
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
    .with_base_url(".");

    let document = html.render_async(&RenderOptions::default()).await.unwrap();
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
    .with_base_url(".");
    let reference = reference_html
        .render_async(&RenderOptions::default())
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
        .render_async(&RenderOptions::default())
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
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let font = line_font(&document, &document.pages[0].lines()[0]);
    assert_eq!(font.face_index, fixture.face_index);
    assert_eq!(font.data.get(..4), Some(b"ttcf".as_slice()));
    assert!(
        document
            .write_pdf_bytes()
            .unwrap()
            .windows(5)
            .any(|bytes| bytes == b"/Font")
    );
}

#[tokio::test]
async fn ticket_airplane_fallback_prefers_visible_unicode_text_font() {
    let document = Html::from_file_async("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
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

    assert!(font_label_contains_any(
        airplane_run_font,
        &["arial unicode"]
    ));
}

#[tokio::test]
async fn ticket_pdf_prunes_unused_and_duplicate_embedded_fonts() {
    let pdf = Html::from_file_async("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .write_pdf_bytes_async(&RenderOptions::default())
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
    let family_names = collection
        .family_names()
        .map(str::to_string)
        .collect::<Vec<_>>();

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
    let mut attributes = Vec::new();
    for (style, style_css) in [
        (fontique::FontStyle::Normal, "normal"),
        (fontique::FontStyle::Italic, "italic"),
        (fontique::FontStyle::Oblique(Some(14.0)), "oblique"),
    ] {
        for (weight, weight_css) in [(400.0, 400), (700.0, 700), (300.0, 300), (500.0, 500)] {
            for (width_ratio, width_css) in [
                (1.0, "normal"),
                (0.75, "condensed"),
                (1.25, "expanded"),
                (0.875, "semi-condensed"),
                (1.125, "semi-expanded"),
            ] {
                attributes.push(FontQueryAttributes {
                    style,
                    style_css,
                    weight,
                    weight_css,
                    width_ratio,
                    width_css,
                });
            }
        }
    }
    attributes
}

fn ttc_text_query_font_can_shape(font: &fontique::QueryFont, text: &str) -> bool {
    let Ok(face) = ttf_parser::Face::parse(font.blob.as_ref(), font.index) else {
        return false;
    };
    text.chars()
        .filter(|character| !character.is_whitespace())
        .all(|character| face.glyph_index(character).is_some())
}

fn css_string_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[tokio::test]
async fn embeds_shaped_system_font_symbols_without_question_mark_fallbacks() {
    let pdf = Html::from_string("<p>© 2018 • KinSNP® ≥7 cM ≤ 0.5</p>")
        .write_pdf_bytes_async(&RenderOptions::default())
        .await
        .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(rendered.contains("/ToUnicode"));
    assert!(rendered.contains("<00A9>"));
    assert!(rendered.contains("<2022>"));
    assert!(rendered.contains("<00AE>"));
    assert!(rendered.contains("<2265>"));
    assert!(rendered.contains("<2264>"));
    assert!(!rendered.contains("(? 2018"));
    assert!(!rendered.contains("KinSNP?"));
}

#[tokio::test]
async fn rendered_text_lines_preserve_shaped_glyphs_for_pdf() {
    let document = Html::from_string("<p>KinSNP® ≥7</p>")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("KinSNP"))
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
    .render_async(&RenderOptions::default()).await
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
async fn font_style_boundary_preserves_arabic_shaping_context() {
    let document = Html::from_string(
        "<style>\
         div { margin: 0; font-size: 48pt; font-family: sans-serif; direction: rtl }\
         .fontstyle { font-style: italic }\
         </style>\
         <div lang=\"ar\">ع<span class=\"fontstyle\">ع</span>ع</div>\
         <div lang=\"ar\">ع&zwj;<span class=\"fontstyle\">&zwj;ع&zwj;</span>&zwj;ع</div>",
    )
    .render_async(&RenderOptions::default())
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
                .map(|glyph| glyph.id)
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
                .with_base_url(".")
                .render_async(&RenderOptions::default())
                .await
                .unwrap();
            let reference_document = Html::from_string(variant.html_for_text(reference_text))
                .with_base_url(".")
                .render_async(&RenderOptions::default())
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
        .render_async(&RenderOptions::default())
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

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains(&format!(
        "<{}>",
        fallback_character
            .encode_utf16(&mut [0; 2])
            .iter()
            .map(|unit| format!("{unit:04X}"))
            .collect::<String>()
    )));
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
        .with_base_url(".")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("red fallback-baseline probe should paint");
    let white = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(Color::WHITE))
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
        first_rect_paint_operation_index(page, Color::WHITE)
            > first_rect_paint_operation_index(page, Color::new(255, 0, 0)),
        "white reference should paint over the red fallback-baseline probe"
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
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("red fallback-baseline probe should paint");
    let white = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(Color::WHITE))
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
        first_rect_paint_operation_index(page, Color::WHITE)
            > first_rect_paint_operation_index(page, Color::new(255, 0, 0)),
        "white reference should paint over the red fallback-baseline probe"
    );
}

fn woff1_from_sfnt(sfnt: &[u8]) -> Vec<u8> {
    let table_count = u16::from_be_bytes(sfnt[4..6].try_into().unwrap()) as usize;
    let mut tables = Vec::new();
    for index in 0..table_count {
        let record = 12 + index * 16;
        let tag = sfnt[record..record + 4].to_vec();
        let checksum = u32::from_be_bytes(sfnt[record + 4..record + 8].try_into().unwrap());
        let offset = u32::from_be_bytes(sfnt[record + 8..record + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(sfnt[record + 12..record + 16].try_into().unwrap()) as usize;
        tables.push((tag, checksum, sfnt[offset..offset + len].to_vec()));
    }

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
             .special {{ font-family: AlreqSpecial; }}\
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
                        .map(|glyph| glyph.id)
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

    faces
}
