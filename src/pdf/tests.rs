use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::{
    PdfFontValidationProfile, embedded_font_candidate_key, embedded_font_plans_with_profile,
    embedded_font_plans_with_profile_and_mode, quantized_pdf_font_size, same_embedded_font_program,
};
use crate::document::{DocumentFontData, FontProgramKind, PaintBand};
use crate::{
    Color, Document, DocumentFont, DocumentMetadata, Error, FontEmbeddingMode, Html, Page,
    PdfCompression, PdfOptions, PdfProfile, RenderOptions, RenderedGlyph, RenderedLine,
    RenderedTextRun,
};
use fontique::Blob as FontiqueBlob;

struct CountingFontData {
    bytes: Vec<u8>,
    reads: Arc<AtomicUsize>,
}

impl AsRef<[u8]> for CountingFontData {
    fn as_ref(&self) -> &[u8] {
        self.reads.fetch_add(1, Ordering::Relaxed);
        &self.bytes
    }
}

fn assert_alpha_ext_gstate(rendered: &str) {
    assert!(rendered.contains("/ExtGState"));
    assert!(rendered.contains("/GSalpha500"));
    assert!(rendered.contains("/ca 0.5"));
    assert!(rendered.contains("/CA 0.5"));
}

#[test]
fn pdf_profiles_round_trip_and_select_their_writer_metadata() {
    let profiles = [
        ("pdf", PdfProfile::Pdf, (1, 4), None),
        ("pdf/a-1b", PdfProfile::PdfA1B, (1, 4), Some((1, "B"))),
        ("pdf/a-2b", PdfProfile::PdfA2B, (1, 7), Some((2, "B"))),
        ("pdf/a-3b", PdfProfile::PdfA3B, (1, 7), Some((3, "B"))),
        ("pdf/a-2u", PdfProfile::PdfA2U, (1, 7), Some((2, "U"))),
        ("pdf/a-3u", PdfProfile::PdfA3U, (1, 7), Some((3, "U"))),
    ];

    assert_eq!(PdfProfile::default(), PdfProfile::PdfA2B);
    for (name, expected, pdf_version, pdfa_identification) in profiles {
        let profile = name.parse::<PdfProfile>().unwrap();

        assert_eq!(profile, expected);
        assert_eq!(profile.to_string(), name);
        assert_eq!(profile.pdf_version(), pdf_version);
        assert_eq!(
            profile
                .pdfa_identification()
                .map(|identification| (identification.part, identification.conformance)),
            pdfa_identification
        );
    }
}

fn named_resource_ref(rendered: &str, dictionary: &str, resource: &str) -> Option<usize> {
    let (_, after_dictionary) = rendered.split_once(dictionary)?;
    let (_, after_resource) = after_dictionary.split_once(resource)?;
    let after_resource = after_resource.trim_start();
    let (id, generation_ref) = after_resource.split_once(' ')?;
    let id = id.parse().ok()?;
    if generation_ref.trim_start().starts_with("0 R") {
        Some(id)
    } else {
        None
    }
}

fn assert_transparency_group(rendered: &str) {
    assert!(rendered.contains("/Group"));
    assert!(rendered.contains("/S /Transparency"));
}

fn assert_translate_transform(rendered: &str) {
    assert!(rendered.contains("1 0 0 1 5 7 cm"));
}

fn translate_transform_count(rendered: &str) -> usize {
    rendered.matches("1 0 0 1 5 7 cm").count()
}

fn clip_scope_count(rendered: &str) -> usize {
    rendered.matches("W\nn").count()
}

fn filled_rect_count(rendered: &str) -> usize {
    rendered.matches(" re\nf").count()
}

/// Return whether a tagged sRGB PDF/A fill uses the requested components.
///
/// PDF/A content may not depend on uncalibrated device RGB. Quire therefore
/// selects the page's ICCBased sRGB resource with `cs` before setting a color
/// with `scn` (ISO 32000-2:2020, 8.6.8).
fn has_srgb_fill(rendered: &str, components: &str) -> bool {
    rendered.contains(&format!("/CSsRGB cs\n{components} scn"))
}

fn srgb_fill_count(rendered: &str, components: &str) -> usize {
    rendered
        .matches(&format!("/CSsRGB cs\n{components} scn"))
        .count()
}

fn rendered_pdf_for_page(page: Page) -> String {
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: Vec::new(),
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };
    pdf_searchable_text(
        &document
            .write_pdf_bytes(&crate::PdfOptions::default())
            .unwrap(),
    )
}

/// Return the PDF syntax plus decoded text streams for structural assertions.
///
/// Production PDFs Flate-compress every stream. Tests still need to inspect
/// PDF operators and XMP text, so this deliberately small reader handles the
/// direct `/Length` streams produced by Quire's writer rather than relying on
/// uncompressed implementation details.
fn pdf_searchable_text(pdf: &[u8]) -> String {
    let mut text = String::new();
    let mut search_start = 0;
    let mut copied_until = 0;
    while let Some(stream_marker) = find_bytes_from(pdf, b"stream\n", search_start) {
        let stream_start = stream_marker + b"stream\n".len();
        let Some((dictionary_start, stream_end)) = pdf_stream_bounds(pdf, stream_start) else {
            search_start = stream_start;
            continue;
        };
        text.push_str(&String::from_utf8_lossy(&pdf[copied_until..stream_start]));
        let stream = &pdf[stream_start..stream_end];
        let dictionary = &pdf[dictionary_start..stream_marker];
        let decoded = if dictionary
            .windows(b"/Filter /FlateDecode".len())
            .any(|window| window == b"/Filter /FlateDecode")
        {
            miniz_oxide::inflate::decompress_to_vec_zlib(stream).ok()
        } else {
            Some(stream.to_vec())
        };
        if let Some(decoded) = decoded
            && let Ok(decoded) = String::from_utf8(decoded)
        {
            text.push_str(&decoded);
        }
        copied_until = stream_end;
        search_start = stream_end.saturating_add(b"\nendstream".len());
    }
    text.push_str(&String::from_utf8_lossy(&pdf[copied_until..]));
    text
}

fn pdf_stream_bounds(pdf: &[u8], stream_start: usize) -> Option<(usize, usize)> {
    let length_marker = b"/Length ";
    let dictionary_start = pdf[..stream_start]
        .windows(length_marker.len())
        .rposition(|window| window == length_marker)?;
    let length_start = dictionary_start + length_marker.len();
    let length_end = pdf[length_start..]
        .iter()
        .position(|byte| !byte.is_ascii_digit())?
        + length_start;
    let length = std::str::from_utf8(&pdf[length_start..length_end])
        .ok()?
        .parse::<usize>()
        .ok()?;
    let stream_end = stream_start.checked_add(length)?;
    (stream_end <= pdf.len()).then_some((dictionary_start, stream_end))
}

fn decoded_stream_after(pdf: &[u8], marker: &[u8]) -> Option<Vec<u8>> {
    let marker_start = find_bytes(pdf, marker)?;
    let stream_marker = find_bytes_from(pdf, b"stream\n", marker_start)?;
    let stream_start = stream_marker + b"stream\n".len();
    let (dictionary_start, stream_end) = pdf_stream_bounds(pdf, stream_start)?;
    let stream = &pdf[stream_start..stream_end];
    let dictionary = &pdf[dictionary_start..stream_marker];
    if dictionary
        .windows(b"/Filter /FlateDecode".len())
        .any(|window| window == b"/Filter /FlateDecode")
    {
        miniz_oxide::inflate::decompress_to_vec_zlib(stream).ok()
    } else {
        Some(stream.to_vec())
    }
}

fn assert_all_pdf_streams_use_flate(pdf: &[u8]) {
    let mut stream_count = 0;
    let mut search_start = 0;
    while let Some(stream_marker) = find_bytes_from(pdf, b"stream\n", search_start) {
        let stream_start = stream_marker + b"stream\n".len();
        let (dictionary_start, stream_end) =
            pdf_stream_bounds(pdf, stream_start).expect("Quire stream has a direct /Length");
        let dictionary = &pdf[dictionary_start..stream_marker];
        assert!(
            dictionary
                .windows(b"/Filter /FlateDecode".len())
                .any(|window| window == b"/Filter /FlateDecode"),
            "all generated PDF streams must be Flate encoded"
        );
        stream_count += 1;
        search_start = stream_end.saturating_add(b"\nendstream".len());
    }
    assert!(stream_count > 0, "test document must emit PDF streams");
}

fn assert_no_pdf_streams_use_flate(pdf: &[u8]) {
    let mut stream_count = 0;
    let mut search_start = 0;
    while let Some(stream_marker) = find_bytes_from(pdf, b"stream\n", search_start) {
        let stream_start = stream_marker + b"stream\n".len();
        let (dictionary_start, stream_end) =
            pdf_stream_bounds(pdf, stream_start).expect("Quire stream has a direct /Length");
        let dictionary = &pdf[dictionary_start..stream_marker];
        assert!(
            !dictionary
                .windows(b"/Filter /FlateDecode".len())
                .any(|window| window == b"/Filter /FlateDecode"),
            "uncompressed PDF streams must not use FlateDecode"
        );
        stream_count += 1;
        search_start = stream_end.saturating_add(b"\nendstream".len());
    }
    assert!(stream_count > 0, "test document must emit PDF streams");
}

/// Return the Form resource names from every emitted `/XObject` dictionary.
///
/// The writer emits direct dictionaries, so a small delimiter-aware scanner is
/// enough for this structural regression check without introducing a PDF
/// parser into the test build.
fn form_names_in_xobject_resource_dictionaries(pdf: &[u8]) -> Vec<Vec<String>> {
    let marker = b"/XObject <<";
    let mut dictionaries = Vec::new();
    let mut search_start = 0;
    while let Some(marker_start) = find_bytes_from(pdf, marker, search_start) {
        let dictionary_start = marker_start + b"/XObject ".len();
        let Some(dictionary_end) = pdf_dictionary_end(pdf, dictionary_start) else {
            break;
        };
        let dictionary = &pdf[dictionary_start..dictionary_end];
        let mut names = Vec::new();
        let mut entry_start = 0;
        while let Some(form_start) = find_bytes_from(dictionary, b"/Fm", entry_start) {
            let name_end = dictionary[form_start + 1..]
                .iter()
                .position(|byte| !byte.is_ascii_alphanumeric())
                .map(|offset| form_start + 1 + offset)
                .unwrap_or(dictionary.len());
            names.push(String::from_utf8_lossy(&dictionary[form_start + 1..name_end]).into());
            entry_start = name_end;
        }
        dictionaries.push(names);
        search_start = dictionary_end;
    }
    dictionaries
}

fn pdf_dictionary_end(pdf: &[u8], start: usize) -> Option<usize> {
    (pdf.get(start..start + 2)? == b"<<").then_some(())?;
    let mut depth = 0;
    let mut position = start;
    while position + 1 < pdf.len() {
        match &pdf[position..position + 2] {
            b"<<" => {
                depth += 1;
                position += 2;
            }
            b">>" => {
                depth -= 1;
                position += 2;
                if depth == 0 {
                    return Some(position);
                }
            }
            _ => position += 1,
        }
    }
    None
}

fn assert_unique_form_resource_names(pdf: &[u8]) {
    for names in form_names_in_xobject_resource_dictionaries(pdf) {
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            names.len(),
            "each Form resource dictionary must have unique /Fm keys: {names:?}"
        );
    }
}

fn green_rect(x: f32, y: f32, width: f32, height: f32) -> crate::RenderedRect {
    crate::RenderedRect::new(x, y, width, height, Some(Color::new(0, 255, 0)), None, 0.0)
}

#[test]
fn rounded_rects_participate_in_paint_order_and_pdf_serialization() {
    let mut page = Page::new(100.0, 100.0);
    page.push_rounded_rect_in_band(
        PaintBand::InFlowBlock,
        crate::RenderedRoundedRect::new(
            10.0,
            10.0,
            30.0,
            20.0,
            crate::RenderedRoundedRectRadii {
                top_left: crate::RenderedCornerRadius::new(4.0, 4.0),
                top_right: crate::RenderedCornerRadius::new(4.0, 4.0),
                bottom_right: crate::RenderedCornerRadius::new(4.0, 4.0),
                bottom_left: crate::RenderedCornerRadius::new(4.0, 4.0),
            },
            Some(Color::BLACK),
            None,
            0.0,
        ),
    );
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: Vec::new(),
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    assert_eq!(
        document.pages[0].paint_operations().as_ref(),
        &[crate::PaintOperation::RoundedRect(0)]
    );
    assert!(
        document
            .write_pdf_bytes(&crate::PdfOptions::default())
            .is_ok()
    );
}

#[test]
fn paint_tree_coalesces_adjacent_same_fill_rectangles() {
    let mut page = Page::new(100.0, 100.0);
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 0.0, 10.0, 10.0));
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 10.0, 10.0, 10.0));

    let rendered = rendered_pdf_for_page(page);

    assert_eq!(filled_rect_count(&rendered), 1);
    assert!(rendered.contains("0 0 10 20 re\nf"));
}

#[test]
fn paint_tree_batches_disjoint_same_fill_rectangles_into_one_path() {
    let mut page = Page::new(100.0, 100.0);
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 0.0, 10.0, 10.0));
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(20.0, 0.0, 10.0, 10.0));

    let rendered = rendered_pdf_for_page(page);

    assert_eq!(filled_rect_count(&rendered), 1);
    assert!(rendered.contains("0 0 10 10 re\n20 0 10 10 re\nf"));
}

#[test]
fn paint_tree_drops_opaque_underpaint_covered_by_later_rectangles() {
    let mut page = Page::new(100.0, 100.0);
    page.push_rect_in_band(
        PaintBand::InFlowBlock,
        crate::RenderedRect::new(0.0, 0.0, 10.0, 10.0, Some(Color::new(255, 0, 0)), None, 0.0),
    );
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 0.0, 10.0, 10.0));

    let rendered = rendered_pdf_for_page(page);

    assert_eq!(filled_rect_count(&rendered), 1);
    assert!(rendered.contains("/CSsRGB cs\n0 1 0 scn\n0 0 10 10 re\nf"));
}

#[test]
fn paint_tree_rect_coalescing_flushes_before_lines() {
    let mut page = Page::new(100.0, 100.0);
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 0.0, 10.0, 10.0));
    page.push_line(RenderedLine::new(
        "unshaped".to_string(),
        0.0,
        20.0,
        10.0,
        None,
        Color::BLACK,
        Vec::new(),
    ));
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 10.0, 10.0, 10.0));

    let rendered = rendered_pdf_for_page(page);

    assert_eq!(filled_rect_count(&rendered), 2);
}

#[test]
fn paint_tree_rect_coalescing_flushes_before_vector_paths() {
    let mut page = Page::new(100.0, 100.0);
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 0.0, 10.0, 10.0));
    page.push_path_in_band(
        PaintBand::InFlowBlock,
        crate::RenderedPath::new(
            vec![
                crate::RenderedPathCommand::move_to(crate::PaintPoint::new(20.0, 20.0)),
                crate::RenderedPathCommand::line_to(crate::PaintPoint::new(25.0, 20.0)),
                crate::RenderedPathCommand::line_to(crate::PaintPoint::new(20.0, 25.0)),
                crate::RenderedPathCommand::Close,
            ],
            Some(Color::BLACK),
            crate::RenderedPathFillRule::NonZero,
            None,
            0.0,
            None,
        ),
    );
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 10.0, 10.0, 10.0));

    let rendered = rendered_pdf_for_page(page);

    assert_eq!(filled_rect_count(&rendered), 2);
}

#[test]
fn paint_tree_rect_coalescing_flushes_before_rounded_rectangles() {
    let mut page = Page::new(100.0, 100.0);
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 0.0, 10.0, 10.0));
    page.push_rounded_rect_in_band(
        PaintBand::InFlowBlock,
        crate::RenderedRoundedRect::new(
            20.0,
            20.0,
            10.0,
            10.0,
            crate::RenderedRoundedRectRadii::ZERO,
            Some(Color::BLACK),
            None,
            0.0,
        ),
    );
    page.push_rect_in_band(PaintBand::InFlowBlock, green_rect(0.0, 10.0, 10.0, 10.0));

    let rendered = rendered_pdf_for_page(page);

    assert_eq!(filled_rect_count(&rendered), 2);
}

#[tokio::test]
async fn paint_tree_rect_coalescing_does_not_cross_transform_contexts() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { width: 10pt; height: 10pt; background: rgb(0 255 0) }\
         .shift { transform-origin: 0 0; transform: translate(0, 0) }</style>\
         <div></div><div class=\"shift\"></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_eq!(filled_rect_count(&rendered), 2);
}

#[tokio::test]
async fn paint_tree_rect_coalescing_does_not_cross_clip_contexts() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .plain, .inner { width: 10pt; height: 10pt; background: rgb(0 255 0) }\
         .clip { width: 10pt; height: 10pt; overflow: hidden }</style>\
         <div class=\"plain\"></div><div class=\"clip\"><div class=\"inner\"></div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_eq!(filled_rect_count(&rendered), 2);
}

#[tokio::test]
async fn paint_tree_rect_coalescing_does_not_cross_opacity_groups() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { width: 10pt; height: 10pt; background: rgb(0 255 0) }\
         .faded { opacity: 0.5 }</style><div></div><div class=\"faded\"></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_eq!(filled_rect_count(&rendered), 2);
    assert_transparency_group(&rendered);
}

#[tokio::test]
async fn emits_pdf_header_and_text() {
    let pdf = Html::from_string("<p>Hello, world</p>")
        .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
        .await
        .unwrap();
    assert!(pdf.starts_with(b"%PDF-1.7"));
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(rendered.contains("/ToUnicode"));
    assert!(rendered.contains("/ID ["));
}

#[tokio::test]
async fn generated_pdf_streams_use_flate_decode() {
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         .raster { width: 40pt; height: 30pt; background-image: conic-gradient(red, blue) }\
         .tile { width: 240pt; height: 120pt; background-image: linear-gradient(90deg, red, blue);\
                  background-size: 40pt 30pt; background-repeat: space round }</style>\
         <p>Hello</p><div class=\"raster\"></div><div class=\"tile\"></div>\
         <svg width=\"20pt\" height=\"20pt\" xmlns=\"http://www.w3.org/2000/svg\">\
           <g opacity=\"0.5\"><rect width=\"20\" height=\"20\" fill=\"red\"/></g>\
         </svg>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();

    assert!(String::from_utf8_lossy(&pdf).contains("/Subtype /Image"));
    assert!(String::from_utf8_lossy(&pdf).contains("/PatternType 1"));
    assert!(String::from_utf8_lossy(&pdf).contains("/Subtype /Form"));
    assert!(String::from_utf8_lossy(&pdf).contains("/FontFile2"));
    assert!(String::from_utf8_lossy(&pdf).contains("/Type /CMap"));
    assert!(String::from_utf8_lossy(&pdf).contains("/Subtype /XML"));
    assert_all_pdf_streams_use_flate(&pdf);
    assert!(pdf_searchable_text(&pdf).contains("BT\n"));
    assert!(
        first_xml_metadata_stream(&pdf)
            .expect("decoded XMP metadata")
            .contains("<pdf:Producer>")
    );
}

#[tokio::test]
async fn uncompressed_pdf_streams_omit_flate_decode() {
    let pdf_options = PdfOptions {
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         .raster { width: 40pt; height: 30pt; background-image: conic-gradient(red, blue) }\
         .tile { width: 240pt; height: 120pt; background-image: linear-gradient(90deg, red, blue);\
                  background-size: 40pt 30pt; background-repeat: space round }</style>\
         <p>Hello</p><div class=\"raster\"></div><div class=\"tile\"></div>\
         <svg width=\"20pt\" height=\"20pt\" xmlns=\"http://www.w3.org/2000/svg\">\
           <g opacity=\"0.5\"><rect width=\"20\" height=\"20\" fill=\"red\"/></g>\
         </svg>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &pdf_options)
    .await
    .unwrap();

    assert!(String::from_utf8_lossy(&pdf).contains("/Subtype /Image"));
    assert!(String::from_utf8_lossy(&pdf).contains("/PatternType 1"));
    assert!(String::from_utf8_lossy(&pdf).contains("/Subtype /Form"));
    assert!(String::from_utf8_lossy(&pdf).contains("/FontFile2"));
    assert!(String::from_utf8_lossy(&pdf).contains("/Type /CMap"));
    assert!(String::from_utf8_lossy(&pdf).contains("/Subtype /XML"));
    assert_no_pdf_streams_use_flate(&pdf);
    assert!(pdf_searchable_text(&pdf).contains("BT\n"));
    assert!(
        first_xml_metadata_stream(&pdf)
            .expect("uncompressed XMP metadata")
            .contains("<pdf:Producer>")
    );
}

#[test]
fn pdf_metadata_stream_mirrors_document_info_dictionary() {
    let document = metadata_test_document(DocumentMetadata {
        title: Some("Spec Title".to_string()),
        author: Some("Ada Lovelace".to_string()),
        creator: Some("Quire Test Suite".to_string()),
    });

    let pdf_options = PdfOptions {
        producer: "quire-test producer".to_string(),
        ..PdfOptions::default()
    };
    let pdf = document.write_pdf_bytes(&pdf_options).unwrap();
    let rendered = pdf_searchable_text(&pdf);
    let xmp = first_xml_metadata_stream(&pdf).expect("catalog XMP metadata stream");

    assert!(rendered.contains("/Metadata"));
    assert!(rendered.contains("/Subtype /XML"));
    assert!(rendered.contains("/Title (Spec Title)"));
    assert!(rendered.contains("/Author (Ada Lovelace)"));
    assert!(rendered.contains("/Creator (Quire Test Suite)"));
    assert!(rendered.contains("/Producer (quire-test producer)"));
    assert!(xmp.contains("<pdf:Producer>quire-test producer</pdf:Producer>"));
    assert!(xmp.contains(r#"xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/""#));
    assert!(xmp.contains(r#"pdfaid:part="2""#));
    assert!(xmp.contains(r#"pdfaid:conformance="B""#));
    assert!(xmp.contains("<xmp:CreatorTool>Quire Test Suite</xmp:CreatorTool>"));
    assert!(xmp.contains("<dc:creator><rdf:Seq><rdf:li>Ada Lovelace</rdf:li>"));
    assert!(xmp.contains("<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Spec Title</rdf:li>"));
    assert_eq!(
        first_xmp_lang_alt_text(&xmp, "dc:title"),
        Some("Spec Title")
    );
}

#[test]
fn pdf_metadata_stream_uses_selected_pdfa_profile_identification() {
    let document = metadata_test_document(DocumentMetadata::default());
    let options = PdfOptions {
        profile: PdfProfile::PdfA2U,
        ..PdfOptions::default()
    };

    let pdf = document.write_pdf_bytes(&options).unwrap();
    let xmp = first_xml_metadata_stream(&pdf).expect("catalog XMP metadata stream");

    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(xmp.contains(r#"pdfaid:part="2""#));
    assert!(xmp.contains(r#"pdfaid:conformance="U""#));
}

#[test]
fn pdf_metadata_stream_uses_pdfa_1_header_and_identification() {
    let document = metadata_test_document(DocumentMetadata::default());
    let options = PdfOptions {
        profile: PdfProfile::PdfA1B,
        ..PdfOptions::default()
    };

    let pdf = document.write_pdf_bytes(&options).unwrap();
    let xmp = first_xml_metadata_stream(&pdf).expect("catalog XMP metadata stream");

    assert!(pdf.starts_with(b"%PDF-1.4"));
    assert!(xmp.contains(r#"pdfaid:part="1""#));
    assert!(xmp.contains(r#"pdfaid:conformance="B""#));
}

#[test]
fn plain_pdf_profile_omits_pdfa_identification() {
    let document = metadata_test_document(DocumentMetadata::default());
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        ..PdfOptions::default()
    };

    let pdf = document.write_pdf_bytes(&options).unwrap();
    let xmp = first_xml_metadata_stream(&pdf).expect("catalog XMP metadata stream");

    assert!(pdf.starts_with(b"%PDF-1.4"));
    assert!(!xmp.contains("pdfaid"));
}

#[test]
fn pdf_xmp_metadata_escapes_text_values() {
    let document = metadata_test_document(DocumentMetadata {
        title: Some("AT&T <PDF> \"Title\" 'Test' Café".to_string()),
        author: Some("Ada & Bob <Team> \"A\" 'B' Ł".to_string()),
        creator: Some("Tool & Chain <1>".to_string()),
    });

    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        producer: "Quire & Producer <PDF>".to_string(),
        ..PdfOptions::default()
    };
    let pdf = document.write_pdf_bytes(&options).unwrap();
    let xmp = first_xml_metadata_stream(&pdf).expect("catalog XMP metadata stream");

    assert!(xmp.contains("AT&amp;T &lt;PDF&gt; &quot;Title&quot; &#39;Test&#39; Café"));
    assert!(xmp.contains("Ada &amp; Bob &lt;Team&gt; &quot;A&quot; &#39;B&#39; Ł"));
    assert!(xmp.contains("Tool &amp; Chain &lt;1&gt;"));
    assert!(xmp.contains("Quire &amp; Producer &lt;PDF&gt;"));
}

#[tokio::test]
async fn emits_ext_gstate_for_alpha_paint() {
    let pdf = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 0 } body { margin: 0 } div { width: 20pt; height: 20pt; background: rgba(255, 0, 0, 0.5) }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default()).await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_alpha_ext_gstate(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
    let ext_gstate_id = named_resource_ref(&rendered, "/ExtGState", "/GSalpha500")
        .expect("alpha ExtGState resource should be an indirect object reference");
    assert!(rendered.contains(&format!("{ext_gstate_id} 0 obj")));
}

#[tokio::test]
async fn positioned_opacity_emits_transparency_group_form_xobject() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { position: absolute; left: 10pt; top: 10pt; width: 30pt; height: 30pt;\
         opacity: 0.5; background: red }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
    assert!(rendered.contains("/Fm1 Do"));
}

#[tokio::test]
async fn negative_page_margin_opacity_emits_transparency_group_form_xobject() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 20pt;\
           @bottom-left-corner { content: \"\"; z-index: -1; opacity: 0.5;\
             background: red; width: 20pt; height: 20pt } }\
         body { margin: 0; background: white }</style><p></p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
}

#[tokio::test]
async fn positioned_transform_emits_cm_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { position: absolute; left: 10pt; top: 10pt; width: 20pt; height: 20pt;\
         transform-origin: 0 0; transform: translate(5pt, 7pt); background: red }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn positioned_transform_updates_link_annotation_rect() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 0 } body { margin: 0 }\
         div { position: absolute; left: 10pt; top: 10pt; transform-origin: 0 0;\
         transform: translate(8pt, 0); } a { font-size: 10pt; line-height: 10pt }</style>\
         <div><a href=\"#target\">Link</a></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].links.len(), 1);
    assert!(document.pages[0].links[0].x() >= 18.0);
}

#[tokio::test]
async fn non_positioned_opacity_emits_transparency_group_form_xobject() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { width: 30pt; height: 30pt; opacity: 0.5; background: red }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
}

#[tokio::test]
async fn nested_transparency_forms_have_unique_scoped_resources() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .outer { width: 50pt; height: 50pt; opacity: 0.5; background: red }\
         .inner { width: 25pt; height: 25pt; opacity: 0.5; background: blue }</style>\
         <div class=\"outer\"><div class=\"inner\"></div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    let resource_sets = form_names_in_xobject_resource_dictionaries(&pdf);

    assert_unique_form_resource_names(&pdf);
    assert!(rendered.contains("/Fm1 Do"));
    assert!(rendered.contains("/Fm2 Do"));
    assert!(resource_sets.iter().any(|names| names == &["Fm2"]));
    assert!(
        resource_sets.iter().any(|names| {
            names.len() == 2
                && names.iter().any(|name| name == "Fm1")
                && names.iter().any(|name| name == "Fm2")
        }),
        "page resources retain every page-level form for page content: {resource_sets:?}"
    );
}

#[tokio::test]
async fn opacity_grid_form_resources_are_compact_and_unique() {
    let items = "<div class=\"item\"></div>".repeat(36);
    let pdf = Html::from_string(format!(
        "<style>@page {{ size: 540pt 540pt; margin: 0 }} body {{ margin: 0 }}\
         .grid {{ display: grid; grid-gap: 10px; grid-template-columns: repeat(6, 100px);\
         height: 650px; width: 650px; column-rule-color: blue;\
         column-rule-style: solid, repeat(auto, groove, double), repeat(2, dotted);\
         column-rule-width: 5px; row-rule-color: red;\
         row-rule-style: repeat(auto, double, solid), dotted, repeat(2, ridge);\
         row-rule-width: 5px }} .item {{ background: gray; opacity: 0.5 }}</style>\
         <div class=\"grid\">{items}</div>"
    ))
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_unique_form_resource_names(&pdf);
    assert_eq!(rendered.matches("/Subtype /Form").count(), 36);
    assert_eq!(rendered.matches("/GSalpha500 gs").count(), 36);
    assert!(
        has_srgb_fill(&rendered, "0 0 1"),
        "column rules remain blue"
    );
    assert!(has_srgb_fill(&rendered, "1 0 0"), "row rules remain red");
    assert!(
        pdf.len() < 24 * 1024,
        "6×6 opacity grid should not duplicate every form in every form resource table: {} bytes",
        pdf.len()
    );
}

#[tokio::test]
async fn geometry_only_normal_flow_continuations_do_not_emit_blank_pdf_pages() {
    for html in [
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .wide { aspect-ratio: 1 / 0.00000000000001 }\
         .tall { aspect-ratio: 0.00000000000001 / 1 }</style>\
         <div class=wide></div><div class=tall></div>",
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { height: 1000000pt }</style><div></div>",
    ] {
        let document = Html::from_string(html)
            .render(&RenderOptions::default())
            .await
            .unwrap();
        assert_eq!(
            document.pages.len(),
            1,
            "empty definite geometry must not produce blank PDF pages"
        );
        let pdf = document
            .write_pdf_bytes(&crate::PdfOptions::default())
            .unwrap();
        let rendered = pdf_searchable_text(&pdf);
        assert!(
            !rendered.contains(" Tj") && !rendered.contains(" re\n"),
            "the regression fixture must not paint box content"
        );
        assert!(
            pdf.len() < 4 * 1024,
            "a one-page geometry-only document should remain compact: {} bytes",
            pdf.len()
        );
    }
}

#[tokio::test]
async fn normal_flow_continuations_keep_decorations_and_later_paint_pages() {
    let decorated = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .box { height: 250pt; background: red; border: 2pt solid blue }</style>\
         <div class=box></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    assert!(
        decorated.pages.len() >= 3,
        "visible block decoration must retain its continuation pages"
    );
    assert!(
        decorated.pages.iter().all(Page::has_paint_content),
        "each decorated continuation must retain paint"
    );

    let later_paint = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .spacer { height: 220pt } .later { height: 10pt; background: blue }\
         .fixed { position: fixed; top: 0; left: 0; width: 5pt; height: 5pt; background: green }</style>\
         <div class=spacer></div><div class=later></div><div class=fixed></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    assert!(
        later_paint.pages.len() >= 3,
        "later normal-flow paint must retain the finite page sequence before it"
    );
    let pdf = later_paint
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(
        has_srgb_fill(&rendered, "0 0 1"),
        "later sibling remains painted"
    );
    assert_eq!(
        srgb_fill_count(&rendered, "0 0.5019608 0"),
        later_paint.pages.len(),
        "fixed paint repeats only across retained pages"
    );

    let named_geometry = Html::from_string(
        "<style>@page named { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .spacer { page: named; height: 220pt }</style><div class=spacer></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    assert!(
        named_geometry.pages.len() >= 3,
        "named-page geometry is an explicit page-owning side effect"
    );

    let forced_break = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .spacer { height: 220pt; break-after: page }\
         .later { height: 10pt; background: blue }</style>\
         <div class=spacer></div><div class=later></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    assert!(
        forced_break.pages.len() >= 4,
        "a forced break remains a structural boundary before later paint"
    );
}

#[tokio::test]
async fn grid_gap_auto_repeater_keeps_the_first_excess_trailing_rules() {
    let items = "<div></div>".repeat(36);
    let pdf = Html::from_string(format!(
        "<style>@page {{ size: 540pt 540pt; margin: 0 }} body {{ margin: 0 }}\
         .grid {{ display: grid; grid-gap: 10px; grid-template-columns: repeat(6, 100px);\
         height: 650px; width: 650px;\
         column-rule-color: teal, indigo, violet, repeat(auto, red, green), blue, purple, coral;\
         column-rule-style: solid;\
         column-rule-width: 2px, 5px, 2px, repeat(auto, 10px), 10px, 11px, 12px;\
         row-rule-color: repeat(auto, red), repeat(2, yellow);\
         row-rule-style: solid; row-rule-width: 5px }}</style>\
         <div class=\"grid\">{items}</div>"
    ))
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    // The five resolved column gutters use leading values, the auto segment,
    // then the first two authored trailing values. In particular, the last
    // two rules must not be right-aligned to purple/coral.
    let mut offset = 0;
    for color in [
        "0 0.5019608 0.5019608",            // teal
        "0.29411766 0 0.50980395",          // indigo
        "0.93333334 0.50980395 0.93333334", // violet
        "0 0 1",                            // blue
        "0.5019608 0 0.5019608",            // purple
    ] {
        let color = format!("/CSsRGB cs\n{color} scn");
        let relative = rendered[offset..]
            .find(&color)
            .unwrap_or_else(|| panic!("missing column-rule color {color}: {rendered}"));
        offset += relative + color.len();
    }
    assert!(rendered.contains("1.5 487.5 re"), "2px rules are emitted");
    assert!(rendered.contains("7.5 487.5 re"), "10px rules are emitted");
}

#[tokio::test]
async fn replayed_flex_and_grid_item_opacity_emit_one_group_per_item() {
    for display in ["flex", "grid"] {
        let pdf = Html::from_string(format!(
            "<style>@page {{ size: 160pt 80pt; margin: 0 }} body {{ margin: 0 }}\
             .container {{ display: {display}; grid-template-columns: repeat(2, 50pt) }}\
             .item {{ width: 50pt; height: 50pt; opacity: 0.5; background: gray }}</style>\
             <div class=\"container\"><div class=\"item\"></div><div class=\"item\"></div></div>"
        ))
        .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
        .await
        .unwrap();
        let rendered = pdf_searchable_text(&pdf);

        assert_eq!(
            rendered.matches("/Subtype /Form").count(),
            2,
            "{display} items should each have one transparency form"
        );
        assert_eq!(
            rendered.matches("/GSalpha500 gs").count(),
            2,
            "{display} should apply each item's opacity once"
        );
    }
}

#[tokio::test]
async fn replayed_grid_item_keeps_nested_descendant_opacity() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 50pt }\
         .outer { width: 50pt; height: 50pt; opacity: 0.5; background: red }\
         .inner { width: 25pt; height: 25pt; opacity: 0.5; background: blue }</style>\
         <div class=\"grid\"><div class=\"outer\"><div class=\"inner\"></div></div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    let resource_sets = form_names_in_xobject_resource_dictionaries(&pdf);

    assert_eq!(rendered.matches("/Subtype /Form").count(), 2);
    assert_eq!(rendered.matches("/GSalpha500 gs").count(), 2);
    assert_unique_form_resource_names(&pdf);
    assert!(resource_sets.iter().any(|names| names == &["Fm2"]));
}

#[tokio::test]
async fn replayed_grid_item_keeps_transform_clip_blend_and_positioned_child_in_one_group() {
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 0 } body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 60pt }\
         .item { position: relative; width: 60pt; height: 50pt; overflow: hidden;\
         opacity: 0.5; transform-origin: 0 0; transform: translate(5pt, 7pt);\
         mix-blend-mode: multiply; isolation: isolate; background: red }\
         .child { position: absolute; left: 10pt; top: 10pt; width: 20pt; height: 20pt;\
         background: blue }</style><div class=\"grid\"><div class=\"item\">\
         <div class=\"child\"></div></div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert_eq!(rendered.matches("/Subtype /Form").count(), 1);
    assert_eq!(rendered.matches("/GSalpha500 gs").count(), 1);
    assert_translate_transform(&rendered);
    assert!(clip_scope_count(&rendered) >= 1);
    assert!(rendered.contains("/GSblendMultiply gs"));
    assert!(has_srgb_fill(&rendered, "0 0 1"));
}

#[tokio::test]
async fn fragmented_replayed_flex_and_grid_item_opacity_uses_one_group_per_fragment() {
    for display in ["flex", "grid"] {
        let container_style = match display {
            "flex" => "flex-direction: column",
            "grid" => "grid-template-columns: 60pt",
            _ => unreachable!(),
        };
        let pdf = Html::from_string(format!(
            "<style>@page {{ size: 80pt 80pt; margin: 10pt }} body {{ margin: 0 }}\
             .container {{ display: {display}; {container_style} }}\
             .item {{ width: 60pt; height: 160pt; opacity: 0.5; background: gray }}</style>\
             <div class=\"container\"><div class=\"item\"></div></div>"
        ))
        .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
        .await
        .unwrap();
        let rendered = pdf_searchable_text(&pdf);
        let form_count = rendered.matches("/Subtype /Form").count();

        assert!(
            form_count >= 2,
            "{display} item should fragment across pages"
        );
        assert_eq!(rendered.matches("/GSalpha500 gs").count(), form_count);
    }
}

#[tokio::test]
async fn non_positioned_transform_emits_cm_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { width: 20pt; height: 20pt; transform-origin: 0 0;\
         transform: translate(5pt, 7pt); background: red }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn inline_atomic_transform_emits_cm_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, p { margin: 0; font-size: 10pt; line-height: 20pt }\
         span { display: inline-block; width: 20pt; height: 20pt; transform-origin: 0 0;\
         transform: translate(5pt, 7pt); background: red }</style><p><span></span></p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn table_transform_emits_cm_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { width: 30pt; transform-origin: 0 0; transform: translate(5pt, 7pt); background: red }\
         td { width: 30pt; height: 20pt }</style><table><tr><td></td></tr></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn table_opacity_emits_transparency_group_form_xobject() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { width: 30pt; opacity: 0.5; background: red } td { width: 30pt; height: 20pt }</style>\
         <table><tr><td></td></tr></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
}

#[tokio::test]
async fn fragmented_table_opacity_emits_one_transparency_group_per_page_fragment() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { width: 60pt; opacity: 0.5; background: red } td { width: 60pt; height: 40pt }</style>\
         <table><tbody><tr><td>One</td></tr><tr><td>Two</td></tr><tr><td>Three</td></tr></tbody></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.matches("/Subtype /Form").count() >= 2);
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
}

#[tokio::test]
async fn fragmented_table_transform_emits_cm_scope_per_page_fragment() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { width: 60pt; transform-origin: 0 0; transform: translate(5pt, 7pt); background: red }\
         td { width: 60pt; height: 40pt }</style>\
         <table><tbody><tr><td>One</td></tr><tr><td>Two</td></tr><tr><td>Three</td></tr></tbody></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(translate_transform_count(&rendered) >= 2);
}

#[tokio::test]
async fn fragmented_table_overflow_clip_emits_clip_scope_per_page_fragment() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { width: 60pt; overflow: hidden; background: red } td { width: 60pt; height: 40pt }\
         div { width: 120pt; height: 20pt; background: blue }</style>\
         <table><tbody><tr><td><div></div></td></tr><tr><td><div></div></td></tr><tr><td><div></div></td></tr></tbody></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(clip_scope_count(&rendered) >= 2);
}

#[tokio::test]
async fn fragmented_float_opacity_emits_one_transparency_group_per_page_fragment() {
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 40pt; opacity: 0.5 }\
         .chunk { height: 40pt; background: red }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F</p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.matches("/Subtype /Form").count() >= 2);
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
}

#[tokio::test]
async fn fragmented_float_opacity_preserves_link_annotation() {
    let html = "<style>@page { size: 120pt 80pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 45pt; opacity: 0.5 }\
         .chunk { height: 40pt; background: red }\
         p, a { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><p><a href=\"https://example.com\">Link</a></p><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F</p>";
    let document = Html::from_string(html)
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        document.pages.iter().any(|page| !page.links.is_empty()),
        "fragmented float link should survive layout replay"
    );

    let pdf = Html::from_string(html)
        .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
        .await
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Link"));
    assert!(rendered.contains("/URI (https://example.com)"));
    assert_transparency_group(&rendered);
}

#[tokio::test]
async fn fragmented_float_transform_emits_cm_scope_per_page_fragment() {
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 40pt; transform-origin: 0 0; transform: translate(5pt, 7pt) }\
         .chunk { height: 40pt; background: red }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F</p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(translate_transform_count(&rendered) >= 2);
}

#[tokio::test]
async fn fragmented_float_overflow_clip_emits_clip_scope_per_page_fragment() {
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 40pt; overflow: hidden }\
         .chunk { height: 40pt; width: 80pt; background: red }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F</p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(clip_scope_count(&rendered) >= 2);
}

#[tokio::test]
async fn table_transform_updates_link_annotation_rect() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 0 } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { transform-origin: 0 0; transform: translate(8pt, 0); }\
         a { font-size: 10pt; line-height: 10pt }</style>\
         <table><tr><td><a href=\"#target\">Link</a></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].links.len(), 1);
    assert!(document.pages[0].links[0].x() >= 8.0);
}

#[tokio::test]
async fn block_svg_transform_emits_cm_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, svg { margin: 0 }\
         svg { transform-origin: 0 0; transform: translate(5pt, 7pt); }</style>\
         <svg width=\"20pt\" height=\"20pt\"><rect width=\"20pt\" height=\"20pt\" fill=\"red\"></rect></svg>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn block_svg_opacity_emits_transparency_group_form_xobject() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, svg { margin: 0 }\
         svg { opacity: 0.5; }</style>\
         <svg width=\"20pt\" height=\"20pt\"><rect width=\"20pt\" height=\"20pt\" fill=\"red\"></rect></svg>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
}

#[tokio::test]
async fn svg_group_opacity_emits_transparency_group_form_xobject() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, svg { margin: 0 } svg { display: block }</style>\
         <svg width=\"20pt\" height=\"20pt\"><g opacity=\"0.5\"><rect width=\"20pt\" height=\"20pt\" fill=\"red\"/></g></svg>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
}

#[tokio::test]
async fn svg_gradient_emits_native_pdf_shading_pattern() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, svg { margin: 0 }</style>\
         <svg width=\"20pt\" height=\"20pt\" xmlns=\"http://www.w3.org/2000/svg\">\
           <defs><linearGradient id=\"g\"><stop stop-color=\"red\"/><stop offset=\"1\" stop-color=\"blue\"/></linearGradient></defs>\
           <rect width=\"20\" height=\"20\" fill=\"url(#g)\"/>\
         </svg>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/PatternType 2"));
    assert!(rendered.contains("/ShadingType 2"));
    assert!(rendered.contains("/SG1 "));
}

#[tokio::test]
async fn svg_solid_vector_pattern_emits_native_pdf_tiling_pattern() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, svg { margin: 0 }</style>\
         <svg width=\"40pt\" height=\"40pt\" xmlns=\"http://www.w3.org/2000/svg\">\
           <defs><pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"20\" height=\"20\">\
             <rect width=\"10\" height=\"20\" fill=\"red\"/><rect x=\"10\" width=\"10\" height=\"20\" fill=\"blue\"/>\
           </pattern></defs><rect width=\"20\" height=\"20\" fill=\"url(#p)\" fill-opacity=\"0.5\" transform=\"matrix(2 0 0 2 0 0)\"/>\
         </svg>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/PatternType 1"));
    assert!(rendered.contains("/SVP1 "));
    assert!(rendered.contains("/GSalpha500 gs"));
    assert!(!rendered.contains("/Subtype /Image"));
}

#[tokio::test]
async fn body_canvas_constant_gradients_with_explicit_tiles_emit_solid_paths() {
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } html { margin: 8px }\
         body { background-image: linear-gradient(green, green), linear-gradient(red, red);\
         background-size: 100px 100px, 100px 100px;\
         background-position: right 0% top 0%, left 0% top 0%;\
         background-repeat: no-repeat }</style><body></body>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"));
    assert!(has_srgb_fill(&rendered, "1 0 0"));
    assert!(has_srgb_fill(&rendered, "0 0.5019608 0"));
}

#[tokio::test]
async fn opaque_css_background_gradients_emit_native_shadings_at_tile_geometry() {
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         .linear { width: 200pt; height: 60pt; background-image: linear-gradient(90deg, red, blue);\
         background-size: 100pt 40pt; background-position: 30pt 10pt; background-repeat: no-repeat }\
         .radial { width: 200pt; height: 100pt; background-image: radial-gradient(ellipse, red, blue);\
         background-size: 100pt 50pt; background-position: 40pt 20pt; background-repeat: no-repeat }</style>\
         <div class=linear></div><div class=radial></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"));
    assert!(rendered.contains("/PatternType 2"));
    assert!(rendered.contains("/ShadingType 2"));
    assert!(rendered.contains("/ShadingType 3"));
}

#[tokio::test]
async fn page_box_css_background_gradient_uses_native_pdf_shading() {
    let pdf = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 0;\
         background-image: linear-gradient(90deg, red, blue);\
         background-size: 100pt 40pt; background-position: 20pt 10pt; background-repeat: no-repeat }\
         body { margin: 0 }</style><p></p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"));
    assert!(rendered.contains("/PatternType 2"));
    assert!(rendered.contains("/ShadingType 2"));
}

#[tokio::test]
async fn page_margin_box_css_background_gradient_uses_native_pdf_shading() {
    let pdf = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 20pt;\
         @top-left { content: \"margin\"; width: 80pt; height: 20pt;\
         background-image: radial-gradient(ellipse, red, blue);\
         background-size: 40pt 20pt; background-repeat: no-repeat } }\
         body { margin: 0 }</style><p>body</p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"));
    assert!(rendered.contains("/PatternType 2"));
    assert!(rendered.contains("/ShadingType 3"));
}

#[tokio::test]
async fn repeated_opaque_css_gradient_uses_a_shared_pdf_tiling_shading() {
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         div { width: 240pt; height: 120pt; background-image: linear-gradient(90deg, red, blue);\
         background-size: 40pt 30pt; background-repeat: space round }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"));
    assert!(rendered.contains("/PatternType 1"));
    assert!(rendered.contains("/PatternType 2"));
    assert_eq!(rendered.matches("/ShadingType 2").count(), 1);
}

#[tokio::test]
async fn repeating_css_linear_gradient_uses_vector_shading_without_image_tiles() {
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         div { width: 240pt; height: 120pt; padding: 20pt;\
         background-origin: content-box;\
         background-image: repeating-linear-gradient(to bottom right, white 0pt, black 15pt, white 30pt); }\
         </style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"), "{rendered}");
    assert!(rendered.contains("/PatternType 1"), "{rendered}");
    assert!(rendered.contains("/PatternType 2"), "{rendered}");
}

#[tokio::test]
async fn repeating_css_gradient_cycle_boundary_discontinuity_remains_vector() {
    let options = crate::PdfOptions {
        compression: crate::PdfCompression::Uncompressed,
        ..crate::PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         div { width: 200pt; height: 100pt;\
         background-image: repeating-linear-gradient(90deg, red 0pt, blue 10pt, green 20pt); }\
         </style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"), "{rendered}");
    assert!(rendered.contains("/FunctionType 4"), "{rendered}");
}

#[tokio::test]
async fn repeating_css_radial_gradient_uses_vector_shading_without_image_tiles() {
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         div { width: 180pt; height: 120pt;\
         background-image: repeating-radial-gradient(ellipse at 30% 40%, red 0pt, blue 12pt, red 24pt); }\
         </style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"), "{rendered}");
    assert!(rendered.contains("/PatternType 2"), "{rendered}");
    assert!(rendered.contains("/ShadingType 3"), "{rendered}");
}

#[tokio::test]
async fn long_repeating_css_gradient_domains_use_one_periodic_function_per_color_line() {
    let options = crate::PdfOptions {
        compression: crate::PdfCompression::Uncompressed,
        ..crate::PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>@page { size: 2400pt 800pt; margin: 0 } body { margin: 0 }\
         .linear { width: 2200pt; height: 180pt;\
         background-image: repeating-linear-gradient(90deg, red 0pt, blue 10pt, red 20pt); }\
         .radial { width: 1200pt; height: 500pt;\
         background-image: repeating-radial-gradient(ellipse, red 0pt, blue 10pt, red 20pt); }\
         </style><div class=linear></div><div class=radial></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"), "{rendered}");
    assert_eq!(rendered.matches("/FunctionType 4").count(), 2, "{rendered}");
    assert!(!rendered.contains("/FunctionType 3"), "{rendered}");
    assert!(!rendered.contains("sub 0 div"), "{rendered}");
}

#[tokio::test]
async fn repeating_css_gradient_hints_hard_stops_and_alpha_remain_vector() {
    for profile in [PdfProfile::Pdf, PdfProfile::PdfA2B] {
        let options = crate::PdfOptions {
            profile,
            compression: crate::PdfCompression::Uncompressed,
            ..crate::PdfOptions::default()
        };
        let pdf = Html::from_string(
            "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
             div { width: 200pt; height: 100pt;\
             background-image: repeating-linear-gradient(90deg, rgb(255 0 0 / .2) 0pt, 3pt, blue 12pt, green 12pt, rgb(255 0 0 / .2) 24pt); }\
             </style><div></div>",
        )
        .write_pdf_bytes(&RenderOptions::default(), &options)
        .await
        .unwrap();
        let rendered = pdf_searchable_text(&pdf);

        assert!(
            !rendered.contains("/Subtype /Image"),
            "{profile:?}: {rendered}"
        );
        assert!(rendered.contains("/SMask"), "{profile:?}: {rendered}");
        assert!(rendered.contains("/ExtGState"), "{profile:?}: {rendered}");
        assert!(
            rendered.contains("/FunctionType 4"),
            "{profile:?}: {rendered}"
        );
        assert!(rendered.contains("0.5 exp"), "{profile:?}: {rendered}");
        assert!(rendered.contains("/ICCBased"), "{profile:?}: {rendered}");
    }
}

#[tokio::test]
async fn zero_period_repeating_css_gradient_uses_its_vector_average_color() {
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         div { width: 200pt; height: 100pt;\
         background-image: repeating-linear-gradient(90deg, red 0pt, transparent 0pt, blue 0pt); }\
         </style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"), "{rendered}");
    assert!(rendered.contains("/PatternType 2"), "{rendered}");
    assert!(rendered.contains("/FunctionType 2"), "{rendered}");
}

#[tokio::test]
async fn physically_unresolvable_repeating_css_gradient_uses_a_bounded_vector_average() {
    let options = crate::PdfOptions {
        compression: crate::PdfCompression::Uncompressed,
        ..crate::PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         div { width: 240pt; height: 100pt;\
         background-image: repeating-linear-gradient(90deg, red 0pt, blue .01pt); }\
         </style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"), "{rendered}");
    assert_eq!(rendered.matches("/FunctionType 2").count(), 1, "{rendered}");
    assert!(!rendered.contains("/FunctionType 3"), "{rendered}");
}

#[tokio::test]
async fn physically_unresolvable_repeating_radial_gradient_uses_a_bounded_vector_average() {
    let options = crate::PdfOptions {
        compression: crate::PdfCompression::Uncompressed,
        ..crate::PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         div { width: 180pt; height: 120pt;\
         background-image: repeating-radial-gradient(red 0pt, blue .01pt); }\
         </style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(!rendered.contains("/Subtype /Image"), "{rendered}");
    assert_eq!(rendered.matches("/FunctionType 2").count(), 1, "{rendered}");
    assert!(!rendered.contains("/FunctionType 3"), "{rendered}");
    assert!(rendered.contains("/ShadingType 3"), "{rendered}");
}

#[tokio::test]
async fn unsupported_css_gradient_raster_fallback_uses_the_resolved_tile_size_and_flate() {
    let pdf = Html::from_string(
        "<style>@page { size: 300pt 200pt; margin: 0 } body { margin: 0 }\
         div { width: 200pt; height: 100pt; background-image: conic-gradient(red, blue);\
         background-size: 100px 100px; background-repeat: no-repeat }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Image"));
    assert!(rendered.contains("/Width 150"));
    assert!(rendered.contains("/Height 150"));
    assert!(rendered.contains("/Filter /FlateDecode"));
}

#[tokio::test]
async fn svg_gradient_stop_alpha_emits_a_vector_soft_mask() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, svg { margin: 0 }</style>\
         <svg width=\"20pt\" height=\"20pt\" xmlns=\"http://www.w3.org/2000/svg\">\
           <defs><linearGradient id=\"g\"><stop stop-color=\"red\" stop-opacity=\"0.2\"/><stop offset=\"1\" stop-color=\"blue\" stop-opacity=\"0.8\"/></linearGradient></defs>\
           <rect width=\"20\" height=\"20\" fill=\"url(#g)\"/>\
         </svg>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/SMask <<"));
    assert!(rendered.contains("/S /Luminosity"));
    assert!(rendered.contains("/DeviceGray"));
    assert!(rendered.contains("/GSsvgAlpha1 gs"));
}

#[tokio::test]
async fn block_image_transform_and_opacity_emit_atomic_effect_context() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let pdf = Html::from_string(format!(
        "<style>@page {{ size: 100pt 100pt; margin: 0 }} body, img {{ margin: 0 }}\
         img {{ display: block; width: 20pt; height: 20pt; opacity: 0.5;\
         transform-origin: 0 0; transform: translate(5pt, 7pt) }}</style>\
         <img src=\"{image}\">",
    ))
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn inline_table_opacity_transform_and_clip_emit_atomic_effect_context() {
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 0 } body, p, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         body { font-size: 10pt; line-height: 20pt }\
         table { display: inline-table; width: 30pt; opacity: 0.5; overflow: hidden;\
         transform-origin: 0 0; transform: translate(5pt, 7pt); background: red }\
         td { width: 30pt; height: 20pt }</style><p>Before <table><tr><td></td></tr></table> After</p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
    assert_translate_transform(&rendered);
    assert!(clip_scope_count(&rendered) >= 1);
}

#[tokio::test]
async fn split_table_row_inline_block_opacity_emits_group_per_visible_piece() {
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, span { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 80pt; border-collapse: collapse } td { width: 80pt; height: 120pt; padding: 0 }\
         span { display: inline-block; width: 40pt; height: 80pt; opacity: 0.5; background: green }</style>\
         <table><tbody><tr><td><span>Atom</span></td></tr></tbody></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.matches("/Subtype /Form").count() >= 2);
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
}

#[tokio::test]
async fn split_table_row_svg_transform_emits_cm_per_visible_piece() {
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, svg { margin: 0; padding: 0; border-spacing: 0 }\
         table { width: 80pt; border-collapse: collapse } td { width: 80pt; height: 120pt; padding: 0 }\
         svg { transform-origin: 0 0; transform: translate(5pt, 7pt) }</style>\
         <table><tbody><tr><td><svg width=\"30\" height=\"90\"><rect width=\"30\" height=\"90\" fill=\"blue\"/></svg></td></tr></tbody></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(translate_transform_count(&rendered) >= 2);
}

#[tokio::test]
async fn non_positioned_overflow_clip_emits_pdf_clip_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { width: 20pt; height: 20pt; overflow: hidden; background: red }\
         p { margin: 0; width: 40pt; height: 40pt; background: blue }</style><div><p></p></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(clip_scope_count(&rendered) >= 1);
}

#[tokio::test]
async fn zero_sized_paint_containment_emits_empty_pdf_clip_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { contain: paint size; width: 40pt; border: 5pt solid green; color: red }</style>\
         <div>clipped descendant text</div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(clip_scope_count(&rendered) >= 1);
}

#[tokio::test]
async fn rounded_paint_containment_emits_curved_pdf_clip_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { contain: paint; width: 40pt; height: 40pt; border-radius: 50%; background: red }\
         p { margin: 0; width: 60pt; height: 60pt; background: blue }</style>\
         <div><p></p></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(clip_scope_count(&rendered) >= 1);
    assert!(
        rendered.matches(" c\n").count() >= 4,
        "rounded padding-edge clip should emit cubic corner segments"
    );
}

#[tokio::test]
async fn non_positioned_transform_updates_link_annotation_rect() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 0 } body { margin: 0 }\
         div { transform-origin: 0 0; transform: translate(8pt, 0); }\
         a { font-size: 10pt; line-height: 10pt }</style><div><a href=\"#target\">Link</a></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].links.len(), 1);
    assert!(document.pages[0].links[0].x() >= 8.0);
}

#[tokio::test]
async fn pdf_font_size_uses_pango_css_pixel_quantization() {
    assert!((quantized_pdf_font_size(12.0) - 12.0).abs() < 0.000001);
    assert!((quantized_pdf_font_size(25.0) - 24.999756).abs() < 0.000001);
}

#[test]
fn pdf_font_embedding_prunes_unused_fonts_and_merges_byte_identical_font_plans() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let glyph_b = face.glyph_index('B').unwrap().0;
    let font = test_document_font(0, FontiqueBlob::new(Arc::new(font_bytes.clone())));
    let mut duplicate_font = test_document_font(1, FontiqueBlob::new(Arc::new(font_bytes.clone())));
    duplicate_font.post_script_name = "SourceSans3-Italic".to_string();
    let unused_font = test_document_font(2, FontiqueBlob::new(Arc::new(font_bytes)));
    let mut page = Page::new(120.0, 80.0);
    page.push_line(RenderedLine::new(
        "AB".to_string(),
        10.0,
        40.0,
        12.0,
        Some(0),
        Color::BLACK,
        vec![
            RenderedTextRun {
                text: Rc::from("A"),
                actual_text: None,
                x_offset: 0.0,
                y_offset: 0.0,
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: Some(0),
                glyphs: Some(vec![test_rendered_glyph(glyph_a, "A")].into()),
            },
            RenderedTextRun {
                text: Rc::from("B"),
                actual_text: None,
                x_offset: 7.0,
                y_offset: 0.0,
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: Some(1),
                glyphs: Some(vec![test_rendered_glyph(glyph_b, "B")].into()),
            },
        ],
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font, duplicate_font, unused_font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    let plans = embedded_font_plans_with_profile(&document, 1, PdfFontValidationProfile::Default);
    let cid_a = plans.fonts[0].source_gid_to_cid[&glyph_a];
    let cid_b = plans.fonts[0].source_gid_to_cid[&glyph_b];

    assert_eq!(rendered.matches("/FontFile2").count(), 1);
    assert!(rendered.contains("/RF1 "));
    assert!(!rendered.contains("/RF2 "));
    assert!(rendered.matches("Tj").count() >= 2);
    assert!(rendered.contains(&format!("<{cid_a:04X}> <0041>")));
    assert!(rendered.contains(&format!("<{cid_b:04X}> <0042>")));
}

#[test]
fn pdf_font_deduplication_matches_shared_blobs_without_reading_font_bytes() {
    let reads = Arc::new(AtomicUsize::new(0));
    let blob = FontiqueBlob::new(Arc::new(CountingFontData {
        bytes: vec![1, 2, 3],
        reads: Arc::clone(&reads),
    }));
    let left = test_document_font(0, blob.clone());
    let right = test_document_font(1, blob);

    assert!(same_embedded_font_program(&left, &right));
    assert_eq!(reads.load(Ordering::Relaxed), 0);
}

#[test]
fn pdf_font_deduplication_keeps_distinct_same_length_programs_separate() {
    let left = test_document_font(0, FontiqueBlob::new(Arc::new(vec![1, 2, 3])));
    let right = test_document_font(1, FontiqueBlob::new(Arc::new(vec![1, 2, 4])));

    assert_eq!(
        embedded_font_candidate_key(&left),
        embedded_font_candidate_key(&right)
    );
    assert!(!same_embedded_font_program(&left, &right));
}

#[test]
fn pdf_font_embedding_uses_only_visible_text_glyphs() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let glyph_b = face.glyph_index('B').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "A", Color::BLACK));
    page.push_line(test_rendered_line(
        glyph_b,
        "B",
        Color {
            a: 0.0,
            ..Color::BLACK
        },
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    let plans = embedded_font_plans_with_profile(&document, 1, PdfFontValidationProfile::Default);
    let cid_a = plans.fonts[0].source_gid_to_cid[&glyph_a];

    assert!(rendered.contains(&format!("<{cid_a:04X}> <0041>")));
    assert!(!rendered.contains(&format!("<{glyph_b:04X}> <0042>")));
}

#[test]
fn pdf_font_embedding_subsets_ttf_with_compact_cids() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes.clone()));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "A", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let plans = embedded_font_plans_with_profile(&document, 1, PdfFontValidationProfile::Default);
    let cid_a = plans.fonts[0].source_gid_to_cid[&glyph_a];
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let embedded = first_font_file2_stream(&pdf).expect("embedded TrueType font stream");
    let subset_face = ttf_parser::Face::parse(&embedded, 0).expect("subset font parses");

    assert!(embedded.len() < font_bytes.len());
    assert_ne!(cid_a, glyph_a);
    assert_eq!(cid_a, 1, "the first painted glyph should use CID 1");
    assert!(
        subset_face
            .glyph_hor_advance(ttf_parser::GlyphId(cid_a))
            .is_some(),
        "compact subset should retain remapped CID {cid_a}"
    );
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains(&format!("<{cid_a:04X}> <0041>")));
}

#[test]
fn pdf_full_font_embedding_uses_original_ttf_identity_cids_and_full_cmap() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let glyph_b = face.glyph_index('B').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes.clone()));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "A", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let plans = embedded_font_plans_with_profile_and_mode(
        &document,
        1,
        PdfFontValidationProfile::Default,
        FontEmbeddingMode::Full,
    );
    let plan = &plans.fonts[0];
    assert!(matches!(
        plan.embedding_kind,
        super::FontEmbeddingKind::FullStandaloneFont
    ));
    assert_eq!(plan.source_gid_to_cid[&glyph_a], glyph_a);
    assert_eq!(plan.source_gid_to_cid[&glyph_b], glyph_b);
    assert_eq!(plan.used_cids[&glyph_b], "B");

    let options = PdfOptions {
        font_embedding: FontEmbeddingMode::Full,
        ..PdfOptions::default()
    };
    let pdf = document.write_pdf_bytes(&options).unwrap();
    assert_eq!(
        first_font_file2_stream(&pdf).expect("embedded TrueType font stream"),
        font_bytes
    );
    let rendered = pdf_searchable_text(&pdf);
    let base_font = first_pdf_name_after(&rendered, "/BaseFont /").expect("BaseFont name");
    assert_eq!(base_font, "SourceSans3-Regular");
    assert!(rendered.contains(&format!("<{glyph_b:04X}> <0042>")));
}

#[test]
fn pdf_font_embedding_subsets_cff_with_compact_cids() {
    let font_bytes = std::fs::read("weasyprint-samples/ticket/barlowcondensed-regular.otf")
        .expect("local CFF regression font");
    let face = ttf_parser::Face::parse(&font_bytes, 0).expect("CFF fixture parses");
    let source_cff_len = face
        .raw_face()
        .table(ttf_parser::Tag::from_bytes(b"CFF "))
        .expect("fixture has a CFF table")
        .len();
    let glyph_a = face.glyph_index('A').expect("fixture has A").0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let mut font = test_document_font(0, blob);
    font.post_script_name = "BarlowCondensed-Regular".to_string();
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "A", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let plans = embedded_font_plans_with_profile(&document, 1, PdfFontValidationProfile::Default);
    let plan = &plans.fonts[0];
    let cid_a = plan.source_gid_to_cid[&glyph_a];
    assert!(matches!(
        plan.embedding_kind,
        super::FontEmbeddingKind::SubsetCompactGids
    ));
    assert_ne!(cid_a, glyph_a);
    assert!(plan.font_file_data.len() < source_cff_len);

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let embedded = first_font_file3_stream(&pdf).expect("embedded CFF font stream");
    assert_eq!(embedded, plan.font_file_data);
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Subtype /CIDFontType0C"));
    assert!(rendered.contains(&format!("<{cid_a:04X}> <0041>")));
}

#[test]
fn pdfa_cff_subset_fallback_embeds_the_full_cff_program() {
    let font_bytes = std::fs::read("weasyprint-samples/ticket/barlowcondensed-regular.otf")
        .expect("local CFF regression font");
    let face = ttf_parser::Face::parse(&font_bytes, 0).expect("CFF fixture parses");
    let source_cff = face
        .raw_face()
        .table(ttf_parser::Tag::from_bytes(b"CFF "))
        .expect("fixture has a CFF table")
        .to_vec();
    let glyph_a = face.glyph_index('A').expect("fixture has A").0;
    let glyphs = (0..face.number_of_glyphs())
        .map(|glyph_id| test_rendered_glyph(glyph_id, "A"))
        .collect::<Vec<_>>();
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let mut font = test_document_font(0, blob);
    font.post_script_name = "BarlowCondensed-Regular".to_string();
    let mut page = Page::new(120.0, 80.0);
    page.push_line(RenderedLine::new(
        "A".to_string(),
        10.0,
        40.0,
        12.0,
        Some(0),
        Color::BLACK,
        vec![RenderedTextRun {
            text: Rc::from("A"),
            actual_text: None,
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: crate::RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(0),
            glyphs: Some(glyphs.into()),
        }],
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let plans = embedded_font_plans_with_profile(&document, 1, PdfFontValidationProfile::PdfA);
    let plan = &plans.fonts[0];
    assert!(matches!(
        plan.embedding_kind,
        super::FontEmbeddingKind::FullStandaloneFont
    ));
    assert_eq!(plan.source_gid_to_cid[&glyph_a], glyph_a);
    assert_eq!(plan.font_file_data, source_cff);
    assert!(plan.cid_set_data.is_some());

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert_eq!(
        first_font_file3_stream(&pdf).expect("embedded full CFF stream"),
        source_cff
    );
    let rendered = pdf_searchable_text(&pdf);
    assert!(!rendered.contains("REJECT+"), "{rendered}");
}

#[test]
fn pdf_full_font_embedding_uses_original_cff_program_and_identity_cids() {
    let font_bytes = std::fs::read("weasyprint-samples/ticket/barlowcondensed-regular.otf")
        .expect("local CFF regression font");
    let face = ttf_parser::Face::parse(&font_bytes, 0).expect("CFF fixture parses");
    let source_cff = face
        .raw_face()
        .table(ttf_parser::Tag::from_bytes(b"CFF "))
        .expect("fixture has a CFF table")
        .to_vec();
    let glyph_a = face.glyph_index('A').expect("fixture has A").0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let mut font = test_document_font(0, blob);
    font.post_script_name = "BarlowCondensed-Regular".to_string();
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "A", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let plans = embedded_font_plans_with_profile_and_mode(
        &document,
        1,
        PdfFontValidationProfile::Default,
        FontEmbeddingMode::Full,
    );
    let plan = &plans.fonts[0];
    assert!(matches!(
        plan.embedding_kind,
        super::FontEmbeddingKind::FullStandaloneFont
    ));
    assert_eq!(plan.source_gid_to_cid[&glyph_a], glyph_a);
    assert_eq!(plan.font_file_data, source_cff);

    let options = PdfOptions {
        font_embedding: FontEmbeddingMode::Full,
        ..PdfOptions::default()
    };
    let pdf = document.write_pdf_bytes(&options).unwrap();
    assert_eq!(
        first_font_file3_stream(&pdf).expect("embedded CFF font stream"),
        source_cff
    );
}

#[test]
fn pdf_subset_font_names_use_six_uppercase_prefix() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "A", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    let base_font = first_pdf_name_after(&rendered, "/BaseFont /").expect("BaseFont name");
    let (prefix, post_script_name) = base_font.split_once('+').expect("subset prefix");

    assert_eq!(prefix.len(), 6);
    assert!(
        prefix
            .chars()
            .all(|character| character.is_ascii_uppercase())
    );
    assert_eq!(post_script_name, "SourceSans3-Regular");
    assert!(!rendered.contains("/BaseFont /QUIREP+"));
}

#[test]
fn pdf_font_embedding_errors_when_a_standalone_font_has_an_invalid_face_index() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let mut font = test_document_font(0, blob);
    font.face_index = 1;
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "A", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    assert!(matches!(
        document.write_pdf_bytes(&PdfOptions {
            profile: PdfProfile::Pdf,
            ..PdfOptions::default()
        }),
        Err(Error::FontEmbedding { .. })
    ));
}

#[test]
fn pdf_cid_font_writes_default_width_and_real_font_bbox() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let notdef_width = face.glyph_hor_advance(ttf_parser::GlyphId(0)).unwrap();
    let expected_bbox = face.global_bounding_box();
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "A", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains(&format!("/DW {notdef_width}")));
    let bbox = first_pdf_array_after(&rendered, "/FontBBox [").expect("FontBBox");
    let values = bbox
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            expected_bbox.x_min.to_string(),
            expected_bbox.y_min.to_string(),
            expected_bbox.x_max.to_string(),
            expected_bbox.y_max.to_string(),
        ]
    );
}

#[test]
fn pdf_font_plan_pdfa_includes_cid_set_bits_for_used_cids() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "A", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };
    let plans = embedded_font_plans_with_profile(&document, 1, PdfFontValidationProfile::PdfA);
    let cid_set = plans.fonts[0]
        .cid_set_data
        .as_ref()
        .expect("PDF/A profile should plan CIDSet");
    let cid_a = plans.fonts[0].source_gid_to_cid[&glyph_a];
    let byte = cid_set[usize::from(cid_a / 8)];
    let mask = 1 << (7 - (cid_a % 8));

    assert!(plans.fonts[0].cid_set_id.is_some());
    assert_ne!(cid_a, glyph_a);
    assert_ne!(byte & mask, 0);
}

#[test]
fn pdfa_font_embedding_uses_full_font_when_subsetting_is_forbidden_and_errors_without_outline_rights()
 {
    let source = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let glyph_a = ttf_parser::Face::parse(&source, 0)
        .unwrap()
        .glyph_index('A')
        .unwrap()
        .0;
    let mut no_subsetting = source.clone();
    set_os2_fs_type(&mut no_subsetting, 0x0100);
    let mut no_outline = source;
    set_os2_fs_type(&mut no_outline, 0x0200);

    let full_document = document_with_single_glyph_font(no_subsetting, glyph_a);
    let full_plan = embedded_font_plans_with_profile_and_mode(
        &full_document,
        1,
        PdfFontValidationProfile::PdfA,
        FontEmbeddingMode::Subset,
    );
    assert!(matches!(
        full_plan.fonts[0].embedding_kind,
        super::FontEmbeddingKind::FullStandaloneFont
    ));

    let rejected_document = document_with_single_glyph_font(no_outline, glyph_a);
    assert!(matches!(
        rejected_document.write_pdf_bytes(&crate::PdfOptions::default()),
        Err(Error::FontEmbedding { .. })
    ));
}

#[test]
fn pdfa_font_embedding_errors_for_painted_glyphs_without_unicode_mapping() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_a, "", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };
    assert!(matches!(
        document.write_pdf_bytes(&crate::PdfOptions::default()),
        Err(Error::FontEmbedding { .. })
    ));
}

#[test]
fn pdfa_font_embedding_accepts_empty_glyph_summary_covered_by_actual_text() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let font = test_document_font(0, FontiqueBlob::new(Arc::new(font_bytes)));
    let mut page = Page::new(120.0, 80.0);
    page.push_line(RenderedLine::new(
        "A".to_string(),
        10.0,
        40.0,
        12.0,
        Some(0),
        Color::BLACK,
        vec![RenderedTextRun {
            text: Rc::from("A"),
            actual_text: Some(Rc::from("A")),
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: crate::RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(0),
            glyphs: Some(vec![test_rendered_glyph(glyph_a, "")].into()),
        }],
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };
    let pdf = document
        .write_pdf_bytes(&PdfOptions {
            compression: PdfCompression::Uncompressed,
            ..PdfOptions::default()
        })
        .expect("ActualText supplies extraction coverage for this glyph");
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/ActualText"));
    assert!(rendered.contains("(A)"));
}

#[test]
fn pdf_font_embedding_keeps_shaped_ligature_glyphs_or_falls_back() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let Some(ligature) = face.glyph_index('\u{FB03}') else {
        eprintln!("SourceSans3 fixture does not expose an ffi ligature glyph");
        return;
    };
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(ligature.0, "ffi", Color::BLACK));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let plans = embedded_font_plans_with_profile(&document, 1, PdfFontValidationProfile::Default);
    let cid_ligature = plans.fonts[0].source_gid_to_cid[&ligature.0];
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let embedded = first_font_file2_stream(&pdf).expect("embedded TrueType font stream");
    let subset_face = ttf_parser::Face::parse(&embedded, 0).expect("embedded font parses");

    assert!(
        subset_face
            .glyph_hor_advance(ttf_parser::GlyphId(cid_ligature))
            .is_some(),
        "embedded font should keep the remapped shaped ffi ligature glyph"
    );
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains(&format!("<{cid_ligature:04X}> <006600660069>")));
}

#[test]
fn pdf_text_runs_emit_positioned_show_for_advance_adjustments() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let glyph_v = face.glyph_index('V').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(RenderedLine::new(
        "AV".to_string(),
        10.0,
        40.0,
        12.0,
        Some(0),
        Color::BLACK,
        vec![RenderedTextRun {
            text: Rc::from("AV"),
            actual_text: None,
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: crate::RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(0),
            glyphs: Some(
                vec![
                    test_rendered_glyph_with_advances(glyph_a, "A", 5.4, 6.0),
                    test_rendered_glyph_with_advances(glyph_v, "V", 6.0, 6.0),
                ]
                .into(),
            ),
        }],
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("TJ"));
}

#[test]
fn pdf_text_runs_emit_selected_text_matrix_and_offsets() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(RenderedLine::new(
        "A".to_string(),
        10.0,
        40.0,
        12.0,
        Some(0),
        Color::BLACK,
        vec![RenderedTextRun {
            text: Rc::from("A"),
            actual_text: None,
            x_offset: 3.0,
            y_offset: -5.0,
            text_matrix: crate::RenderedTextMatrix::ROTATE_CW,
            font_size: 12.0,
            font_id: Some(0),
            glyphs: Some(vec![test_rendered_glyph(glyph_a, "A")].into()),
        }],
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("0 -1 1 0 13 35 Tm"));
}

#[test]
fn pdf_identity_text_runs_reuse_text_state_with_relative_positioning() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let glyph_b = face.glyph_index('B').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(RenderedLine::new(
        "AB".to_string(),
        10.0,
        40.0,
        12.0,
        Some(0),
        Color::BLACK,
        vec![
            RenderedTextRun {
                text: Rc::from("A"),
                actual_text: None,
                x_offset: 0.0,
                y_offset: 0.0,
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: Some(0),
                glyphs: Some(vec![test_rendered_glyph(glyph_a, "A")].into()),
            },
            RenderedTextRun {
                text: Rc::from("B"),
                actual_text: None,
                x_offset: 10.0,
                y_offset: 0.0,
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: Some(0),
                glyphs: Some(vec![test_rendered_glyph(glyph_b, "B")].into()),
            },
        ],
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    };

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert_eq!(rendered.matches(" Tm").count(), 1, "{rendered}");
    assert!(rendered.contains("10 0 Td"), "{rendered}");
    assert_eq!(rendered.matches("/RF1 12 Tf").count(), 1, "{rendered}");
    assert!(rendered.contains("<0001> <0041>"), "{rendered}");
    assert!(rendered.contains("<0002> <0042>"), "{rendered}");
}

fn test_document_font(id: usize, blob: FontiqueBlob<u8>) -> DocumentFont {
    DocumentFont {
        id,
        family: "Source Sans 3".to_string(),
        post_script_name: "SourceSans3-Regular".to_string(),
        program_kind: FontProgramKind::TrueType,
        data: DocumentFontData::from_blob(blob),
        face_index: 0,
        units_per_em: 1000,
        ascender: 984,
        descender: -273,
        cap_height: 660,
        italic_angle: 0,
        bbox: [-438, -293, 1142, 1034],
    }
}

fn document_with_single_glyph_font(font_bytes: Vec<u8>, glyph_id: u16) -> Document {
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob);
    let mut page = Page::new(120.0, 80.0);
    page.push_line(test_rendered_line(glyph_id, "A", Color::BLACK));
    Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
        image_store: Box::default(),
    }
}

fn set_os2_fs_type(font_data: &mut [u8], fs_type: u16) {
    let table_count = u16::from_be_bytes([font_data[4], font_data[5]]) as usize;
    for index in 0..table_count {
        let record = 12 + index * 16;
        if font_data[record..record + 4] != *b"OS/2" {
            continue;
        }
        let offset = u32::from_be_bytes([
            font_data[record + 8],
            font_data[record + 9],
            font_data[record + 10],
            font_data[record + 11],
        ]) as usize;
        font_data[offset + 8..offset + 10].copy_from_slice(&fs_type.to_be_bytes());
        return;
    }
    panic!("font fixture must include an OS/2 table");
}

fn test_rendered_glyph(id: u16, unicode: &str) -> RenderedGlyph {
    test_rendered_glyph_with_advances(id, unicode, 7.0, 7.0)
}

fn test_rendered_line(glyph_id: u16, unicode: &str, color: Color) -> RenderedLine {
    RenderedLine::new(
        unicode.to_string(),
        10.0,
        40.0,
        12.0,
        Some(0),
        color,
        vec![RenderedTextRun {
            text: Rc::from(unicode),
            actual_text: None,
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: crate::RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(0),
            glyphs: Some(vec![test_rendered_glyph(glyph_id, unicode)].into()),
        }],
    )
}

fn metadata_test_document(metadata: DocumentMetadata) -> Document {
    Document {
        pages: vec![Page::new(120.0, 80.0)],
        metadata,
        fonts: Vec::new(),
        bookmarks: Vec::new(),
        image_store: Box::default(),
    }
}

fn first_font_file2_stream(pdf: &[u8]) -> Option<Vec<u8>> {
    decoded_stream_after(pdf, b"/Length1 ")
}

fn first_font_file3_stream(pdf: &[u8]) -> Option<Vec<u8>> {
    decoded_stream_after(pdf, b"/Subtype /CIDFontType0C")
}

fn first_xml_metadata_stream(pdf: &[u8]) -> Option<String> {
    String::from_utf8(decoded_stream_after(pdf, b"/Subtype /XML")?).ok()
}

fn first_xmp_lang_alt_text<'a>(xmp: &'a str, element: &str) -> Option<&'a str> {
    let marker = format!(r#"<{element}><rdf:Alt><rdf:li xml:lang="x-default">"#);
    let start = xmp.find(&marker)? + marker.len();
    let rest = xmp.get(start..)?;
    let end = rest.find("</rdf:li>")?;
    rest.get(..end)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    find_bytes_from(haystack, needle, 0)
}

fn find_bytes_from(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    haystack
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| start + position)
}

fn first_pdf_name_after<'a>(rendered: &'a str, marker: &str) -> Option<&'a str> {
    let start = rendered.find(marker)? + marker.len();
    let rest = rendered.get(start..)?;
    let end = rest.find(|character: char| character.is_whitespace() || character == '/')?;
    rest.get(..end)
}

fn first_pdf_array_after<'a>(rendered: &'a str, marker: &str) -> Option<&'a str> {
    let start = rendered.find(marker)? + marker.len();
    let rest = rendered.get(start..)?;
    let end = rest.find(']')?;
    rest.get(..end)
}

fn test_rendered_glyph_with_advances(
    id: u16,
    unicode: &str,
    x_advance: f32,
    nominal_x_advance: f32,
) -> RenderedGlyph {
    RenderedGlyph {
        id,
        x_advance,
        nominal_x_advance,
        x_offset: 0.0,
        y_offset: 0.0,
        unicode: unicode.to_string(),
    }
}
