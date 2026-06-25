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

    assert_eq!(document.pages[0].lines.len(), 1);
    assert_eq!(document.pages[0].lines[0].text, "Visible");
    assert!(
        document.pages[0].lines[0].y
            < options.page_size.height - options.page_margins.top - options.line_height
    );
}

#[tokio::test]
async fn supports_bold_and_italic_system_fonts() {
    let document = Html::from_string(
        "<h1>Heading</h1><p style=\"font-style: italic\">Emphasis</p><p style=\"font-weight: bold; font-style: italic\">Both</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(line_font_is_bold(&document, &document.pages[0].lines[0]));
    assert!(line_font_is_italic(&document, &document.pages[0].lines[1]));
    assert!(line_font_is_bold(&document, &document.pages[0].lines[2]));
    assert!(line_font_is_italic(&document, &document.pages[0].lines[2]));
}

#[tokio::test]
async fn supports_generic_system_font_families() {
    let document = Html::from_string(
        "<p style=\"font-family: serif; font-style: italic\">Serif</p><p style=\"font-family: monospace; font-weight: bold\">Mono</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_ne!(
        document.pages[0].lines[0].font_id,
        document.pages[0].lines[1].font_id
    );
    assert!(line_font_is_italic(&document, &document.pages[0].lines[0]));
    assert!(
        line_font_is_monospace(&document, &document.pages[0].lines[1]),
        "resolved monospace font was {}",
        font_label(line_font(&document, &document.pages[0].lines[1]))
    );
    assert!(line_font_is_bold(&document, &document.pages[0].lines[1]));
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
        line_font(&document, &document.pages[0].lines[0]).family,
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
        line_font(&document, &document.pages[0].lines[0]).family,
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
        line_font(&document, &document.pages[0].lines[0]).family,
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
        .lines
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
        .lines
        .iter()
        .filter(|line| line.text == "FillerText")
        .find(|line| line.runs.iter().any(|run| run.font_size > 30.1))
        .expect("reference adjusted line");

    assert!(
        (rendered_line_baseline_top(&document, adjusted)
            - rendered_line_baseline_top(&reference, reference_adjusted))
        .abs()
            < 0.01
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

    let line = &document.pages[0].lines[0];
    let font = line_font(&document, line);
    assert!(!font.data.is_empty());
    assert!(font.units_per_em > 0);
}

#[tokio::test]
async fn ttc_face_index_survives_query_shaping_and_embedding_when_available() {
    let Some((family_name, face_index)) = system_ttc_text_face_fixture() else {
        eprintln!("No system TTC text face with a nonzero face index is available");
        return;
    };
    let family_css = family_name.replace('\\', "\\\\").replace('"', "\\\"");
    let document = Html::from_string(format!(
        "<style>p {{ font-family: \"{family_css}\" }}</style><p>TTC face index</p>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let font = line_font(&document, &document.pages[0].lines[0]);
    assert_eq!(font.face_index, face_index);
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
        .flat_map(|page| &page.lines)
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

fn system_ttc_text_face_fixture() -> Option<(String, u32)> {
    let mut collection = fontique::Collection::default();
    let mut source_cache = fontique::SourceCache::default();
    let family_names = collection
        .family_names()
        .map(str::to_string)
        .collect::<Vec<_>>();

    for family_name in family_names {
        let mut query = collection.query(&mut source_cache);
        query.set_families([fontique::QueryFamily::Named(&family_name)]);
        let mut match_face = None;
        query.matches_with(|font| {
            if font.index > 0
                && font.blob.as_ref().get(..4) == Some(b"ttcf")
                && ttf_parser::Face::parse(font.blob.as_ref(), font.index)
                    .ok()
                    .is_some_and(|face| face.glyph_index('A').is_some())
            {
                match_face = Some(font.index);
                fontique::QueryStatus::Stop
            } else {
                fontique::QueryStatus::Continue
            }
        });
        if let Some(face_index) = match_face {
            return Some((family_name, face_index));
        }
    }

    None
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
        .lines
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
        .lines
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
        .lines
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
            page.lines
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
