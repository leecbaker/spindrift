use super::{
    PdfFontValidationProfile, embedded_font_plans_with_profile, quantized_pdf_font_size,
    shape_document_text,
};
use crate::document::{DocumentFontData, FontProgramKind};
use crate::{
    Color, Document, DocumentFont, DocumentMetadata, Html, Page, PdfVariant, RenderOptions,
    RenderedGlyph, RenderedLine, RenderedTextRun,
};
use fontique::Blob as FontiqueBlob;
use std::sync::Arc;

fn assert_alpha_ext_gstate(rendered: &str) {
    assert!(rendered.contains("/ExtGState"));
    assert!(rendered.contains("/GSalpha500"));
    assert!(rendered.contains("/ca 0.5"));
    assert!(rendered.contains("/CA 0.5"));
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

#[tokio::test]
async fn emits_pdf_header_and_text() {
    let pdf = Html::from_string("<p>Hello, world</p>")
        .write_pdf_bytes_async(&RenderOptions::default())
        .await
        .unwrap();
    assert!(pdf.starts_with(b"%PDF-1.7"));
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(rendered.contains("/ToUnicode"));
    assert!(rendered.contains("/ID ["));
}

#[test]
fn pdf_metadata_stream_mirrors_document_info_dictionary() {
    let document = metadata_test_document(DocumentMetadata {
        title: Some("Spec Title".to_string()),
        author: Some("Ada Lovelace".to_string()),
        creator: Some("Quire Test Suite".to_string()),
        producer: "quire-test producer".to_string(),
    });

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    let xmp = first_xml_metadata_stream(&rendered).expect("catalog XMP metadata stream");

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
    assert_eq!(first_xmp_lang_alt_text(xmp, "dc:title"), Some("Spec Title"));
}

#[test]
fn pdf_metadata_stream_uses_selected_pdfa_variant_identification() {
    let document = metadata_test_document(DocumentMetadata::default());
    let options = RenderOptions {
        pdf_variant: PdfVariant::PdfA2U,
        ..RenderOptions::default()
    };

    let pdf = document.write_pdf_bytes_with_options(&options).unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    let xmp = first_xml_metadata_stream(&rendered).expect("catalog XMP metadata stream");

    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(xmp.contains(r#"pdfaid:part="2""#));
    assert!(xmp.contains(r#"pdfaid:conformance="U""#));
}

#[test]
fn pdf_metadata_stream_uses_pdfa_1_header_and_identification() {
    let document = metadata_test_document(DocumentMetadata::default());
    let options = RenderOptions {
        pdf_variant: PdfVariant::PdfA1B,
        ..RenderOptions::default()
    };

    let pdf = document.write_pdf_bytes_with_options(&options).unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    let xmp = first_xml_metadata_stream(&rendered).expect("catalog XMP metadata stream");

    assert!(pdf.starts_with(b"%PDF-1.4"));
    assert!(xmp.contains(r#"pdfaid:part="1""#));
    assert!(xmp.contains(r#"pdfaid:conformance="B""#));
}

#[test]
fn plain_pdf_variant_omits_pdfa_identification() {
    let document = metadata_test_document(DocumentMetadata::default());
    let options = RenderOptions {
        pdf_variant: PdfVariant::Pdf,
        ..RenderOptions::default()
    };

    let pdf = document.write_pdf_bytes_with_options(&options).unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    let xmp = first_xml_metadata_stream(&rendered).expect("catalog XMP metadata stream");

    assert!(pdf.starts_with(b"%PDF-1.4"));
    assert!(!xmp.contains("pdfaid"));
}

#[test]
fn pdf_xmp_metadata_escapes_text_values() {
    let document = metadata_test_document(DocumentMetadata {
        title: Some("AT&T <PDF> \"Title\" 'Test' Café".to_string()),
        author: Some("Ada & Bob <Team> \"A\" 'B' Ł".to_string()),
        creator: Some("Tool & Chain <1>".to_string()),
        producer: "Quire & Producer <PDF>".to_string(),
    });

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    let xmp = first_xml_metadata_stream(&rendered).expect("catalog XMP metadata stream");

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
    .write_pdf_bytes_async(&RenderOptions::default()).await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .render_async(&RenderOptions::default())
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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
}

#[tokio::test]
async fn non_positioned_transform_emits_cm_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { width: 20pt; height: 20pt; transform-origin: 0 0;\
         transform: translate(5pt, 7pt); background: red }</style><div></div>",
    )
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn inline_atomic_transform_emits_cm_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, p { margin: 0; font-size: 10pt; line-height: 20pt }\
         span { display: inline-block; width: 20pt; height: 20pt; transform-origin: 0 0;\
         transform: translate(5pt, 7pt); background: red }</style><p><span></span></p>",
    )
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn table_transform_emits_cm_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { width: 30pt; transform-origin: 0 0; transform: translate(5pt, 7pt); background: red }\
         td { width: 30pt; height: 20pt }</style><table><tr><td></td></tr></table>",
    )
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn table_opacity_emits_transparency_group_form_xobject() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { width: 30pt; opacity: 0.5; background: red } td { width: 30pt; height: 20pt }</style>\
         <table><tr><td></td></tr></table>",
    )
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        document.pages.iter().any(|page| !page.links.is_empty()),
        "fragmented float link should survive layout replay"
    );

    let pdf = Html::from_string(html)
        .write_pdf_bytes_async(&RenderOptions::default())
        .await
        .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .render_async(&RenderOptions::default())
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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert_translate_transform(&rendered);
}

#[tokio::test]
async fn block_svg_opacity_emits_transparency_group_form_xobject() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body, svg { margin: 0 }\
         svg { opacity: 0.5; }</style>\
         <svg width=\"20pt\" height=\"20pt\"><rect width=\"20pt\" height=\"20pt\" fill=\"red\"></rect></svg>",
    )
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert!(rendered.contains("/Subtype /Form"));
    assert_transparency_group(&rendered);
    assert!(rendered.contains("/GSalpha500 gs"));
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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert!(translate_transform_count(&rendered) >= 2);
}

#[tokio::test]
async fn non_positioned_overflow_clip_emits_pdf_clip_scope() {
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         div { width: 20pt; height: 20pt; overflow: hidden; background: red }\
         p { margin: 0; width: 40pt; height: 40pt; background: blue }</style><div><p></p></div>",
    )
    .write_pdf_bytes_async(&RenderOptions::default())
    .await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert!(clip_scope_count(&rendered) >= 1);
}

#[tokio::test]
async fn non_positioned_transform_updates_link_annotation_rect() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 0 } body { margin: 0 }\
         div { transform-origin: 0 0; transform: translate(8pt, 0); }\
         a { font-size: 10pt; line-height: 10pt }</style><div><a href=\"#target\">Link</a></div>",
    )
    .render_async(&RenderOptions::default())
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
fn pdf_font_embedding_prunes_unused_fonts_and_merges_duplicate_font_plans() {
    let font_bytes = std::fs::read("weasyprint-samples/invoice/SourceSans3-Regular.ttf").unwrap();
    let face = ttf_parser::Face::parse(&font_bytes, 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap().0;
    let glyph_b = face.glyph_index('B').unwrap().0;
    let blob = FontiqueBlob::new(Arc::new(font_bytes));
    let font = test_document_font(0, blob.clone());
    let duplicate_font = test_document_font(1, blob.clone());
    let unused_font = test_document_font(2, blob);
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
                text: "A".to_string(),
                x_offset: 0.0,
                y_offset: 0.0,
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: Some(0),
                glyphs: Some(vec![test_rendered_glyph(glyph_a, "A")]),
            },
            RenderedTextRun {
                text: "B".to_string(),
                x_offset: 7.0,
                y_offset: 0.0,
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: Some(1),
                glyphs: Some(vec![test_rendered_glyph(glyph_b, "B")]),
            },
        ],
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font, duplicate_font, unused_font],
        bookmarks: Vec::new(),
    };

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert_eq!(rendered.matches("/FontFile2").count(), 1);
    assert!(rendered.contains("/RF1 "));
    assert!(!rendered.contains("/RF2 "));
    assert!(rendered.matches("Tj").count() >= 2);
    assert!(rendered.contains(&format!("<{glyph_a:04X}> <0041>")));
    assert!(rendered.contains(&format!("<{glyph_b:04X}> <0042>")));
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
    };

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert!(rendered.contains(&format!("<{glyph_a:04X}> <0041>")));
    assert!(!rendered.contains(&format!("<{glyph_b:04X}> <0042>")));
}

#[test]
fn pdf_font_embedding_subsets_ttf_while_retaining_original_gids() {
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
    };

    let pdf = document.write_pdf_bytes().unwrap();
    let embedded = first_font_file2_stream(&pdf).expect("embedded TrueType font stream");
    let subset_face = ttf_parser::Face::parse(embedded, 0).expect("subset font parses");

    assert!(embedded.len() < font_bytes.len());
    assert!(
        subset_face
            .glyph_hor_advance(ttf_parser::GlyphId(glyph_a))
            .is_some(),
        "retained-GID subset should keep original glyph id {glyph_a}"
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
    };

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    let base_font = first_pdf_name_after(&rendered, "/BaseFont /").expect("BaseFont name");
    let (prefix, post_script_name) = base_font.split_once('+').expect("subset prefix");

    assert_eq!(prefix.len(), 6);
    assert!(
        prefix
            .chars()
            .all(|character| character.is_ascii_uppercase())
    );
    assert_eq!(post_script_name, "SourceSans3-Regular");
    assert!(!rendered.contains("/BaseFont /REASYP+"));
}

#[test]
fn pdf_full_font_fallback_name_omits_subset_prefix() {
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
    };

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    let base_font = first_pdf_name_after(&rendered, "/BaseFont /").expect("BaseFont name");

    assert_eq!(base_font, "SourceSans3-Regular");
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
    };

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
    };
    let shaped_document = shape_document_text(&document);
    let plans = embedded_font_plans_with_profile(
        &document,
        &shaped_document,
        1,
        PdfFontValidationProfile::PdfA,
    );
    let cid_set = plans.fonts[0]
        .cid_set_data
        .as_ref()
        .expect("PDF/A profile should plan CIDSet");
    let byte = cid_set[usize::from(glyph_a / 8)];
    let mask = 1 << (7 - (glyph_a % 8));

    assert!(plans.fonts[0].cid_set_id.is_some());
    assert_ne!(byte & mask, 0);
}

#[test]
fn pdf_font_plan_strict_rejects_glyphs_without_unicode_mapping() {
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
    };
    let shaped_document = shape_document_text(&document);
    let plans = embedded_font_plans_with_profile(
        &document,
        &shaped_document,
        1,
        PdfFontValidationProfile::StrictPdf,
    );

    assert!(matches!(
        plans.fonts[0].embedding_kind,
        super::FontEmbeddingKind::Rejected { .. }
    ));
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
    };

    let pdf = document.write_pdf_bytes().unwrap();
    let embedded = first_font_file2_stream(&pdf).expect("embedded TrueType font stream");
    let subset_face = ttf_parser::Face::parse(embedded, 0).expect("embedded font parses");

    assert!(
        subset_face
            .glyph_hor_advance(ttf_parser::GlyphId(ligature.0))
            .is_some(),
        "embedded font should keep the shaped ffi ligature glyph or fall back to the full font"
    );
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
            text: "AV".to_string(),
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: crate::RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(0),
            glyphs: Some(vec![
                test_rendered_glyph_with_advances(glyph_a, "A", 5.4, 6.0),
                test_rendered_glyph_with_advances(glyph_v, "V", 6.0, 6.0),
            ]),
        }],
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
    };

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

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
            text: "A".to_string(),
            x_offset: 3.0,
            y_offset: -5.0,
            text_matrix: crate::RenderedTextMatrix::ROTATE_CW,
            font_size: 12.0,
            font_id: Some(0),
            glyphs: Some(vec![test_rendered_glyph(glyph_a, "A")]),
        }],
    ));
    let document = Document {
        pages: vec![page],
        metadata: DocumentMetadata::default(),
        fonts: vec![font],
        bookmarks: Vec::new(),
    };

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert!(rendered.contains("0 -1 1 0 13 35 Tm"));
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
            text: unicode.to_string(),
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: crate::RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(0),
            glyphs: Some(vec![test_rendered_glyph(glyph_id, unicode)]),
        }],
    )
}

fn metadata_test_document(metadata: DocumentMetadata) -> Document {
    Document {
        pages: vec![Page::new(120.0, 80.0)],
        metadata,
        fonts: Vec::new(),
        bookmarks: Vec::new(),
    }
}

fn first_font_file2_stream(pdf: &[u8]) -> Option<&[u8]> {
    let length1 = find_bytes(pdf, b"/Length1 ")?;
    let stream_marker_start = find_bytes_from(pdf, b"stream\n", length1)? + b"stream\n".len();
    let stream_end = find_bytes_from(pdf, b"\nendstream", stream_marker_start)?;
    Some(&pdf[stream_marker_start..stream_end])
}

fn first_xml_metadata_stream(rendered: &str) -> Option<&str> {
    let subtype = rendered.find("/Subtype /XML")?;
    let stream_start = rendered.get(subtype..)?.find("stream\n")? + subtype + "stream\n".len();
    let stream_end = rendered.get(stream_start..)?.find("\nendstream")? + stream_start;
    rendered.get(stream_start..stream_end)
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
