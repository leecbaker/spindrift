use super::{ShapedGlyph, quantized_pdf_font_size, shaped_text_operator};
use crate::document::{DocumentFontData, FontProgramKind};
use crate::{
    Color, Document, DocumentFont, DocumentMetadata, Html, Page, RenderOptions, RenderedGlyph,
    RenderedLine, RenderedTextRun,
};
use fontique::Blob as FontiqueBlob;
use std::sync::Arc;

#[tokio::test]
async fn emits_pdf_header_and_text() {
    let pdf = Html::from_string("<p>Hello, world</p>")
        .write_pdf_bytes_async(&RenderOptions::default())
        .await
        .unwrap();
    assert!(pdf.starts_with(b"%PDF-1.4"));
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(rendered.contains("/ToUnicode"));
}

#[tokio::test]
async fn emits_ext_gstate_for_alpha_paint() {
    let pdf = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 0 } body { margin: 0 } div { width: 20pt; height: 20pt; background: rgba(255, 0, 0, 0.5) }</style><div></div>",
    )
    .write_pdf_bytes_async(&RenderOptions::default()).await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert!(rendered.contains("/ExtGState << /GSalpha500"));
    assert!(rendered.contains("/ca 0.500 /CA 0.500"));
    assert!(rendered.contains("/GSalpha500 gs"));
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
    assert!(rendered.contains("/Group << /S /Transparency >>"));
    assert!(rendered.contains("/GSalpha500 gs"));
    assert!(rendered.contains("/Fm1 Do"));
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

    assert!(rendered.contains("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm"));
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
    assert!(document.pages[0].links[0].x >= 18.0);
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
    assert!(rendered.contains("/Group << /S /Transparency >>"));
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

    assert!(rendered.contains("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm"));
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

    assert!(rendered.contains("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm"));
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

    assert!(rendered.contains("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm"));
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
    assert!(rendered.contains("/Group << /S /Transparency >>"));
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
    assert!(rendered.contains("/Group << /S /Transparency >>"));
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

    assert!(
        rendered
            .matches("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm")
            .count()
            >= 2
    );
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

    assert!(rendered.matches(" W n").count() >= 2);
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
    assert!(rendered.contains("/Group << /S /Transparency >>"));
    assert!(rendered.contains("/GSalpha500 gs"));
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

    assert!(
        rendered
            .matches("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm")
            .count()
            >= 2
    );
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

    assert!(rendered.matches(" W n").count() >= 2);
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
    assert!(document.pages[0].links[0].x >= 8.0);
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

    assert!(rendered.contains("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm"));
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
    assert!(rendered.contains("/Group << /S /Transparency >>"));
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
    assert!(rendered.contains("/Group << /S /Transparency >>"));
    assert!(rendered.contains("/GSalpha500 gs"));
    assert!(rendered.contains("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm"));
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
    assert!(rendered.contains("/Group << /S /Transparency >>"));
    assert!(rendered.contains("/GSalpha500 gs"));
    assert!(rendered.contains("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm"));
    assert!(rendered.contains(" W n"));
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
    assert!(rendered.contains("/Group << /S /Transparency >>"));
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

    assert!(
        rendered
            .matches("1.000000 0.000000 0.000000 1.000000 5.000000 7.000000 cm")
            .count()
            >= 2
    );
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

    assert!(rendered.contains(" W n"));
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
    assert!(document.pages[0].links[0].x >= 8.0);
}

#[tokio::test]
async fn shaped_text_operator_uses_simple_show_for_unadjusted_glyphs() {
    let glyphs = vec![
        ShapedGlyph {
            id: 0x0041,
            x_advance: 6.0,
            nominal_x_advance: 6.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: "A".to_string(),
        },
        ShapedGlyph {
            id: 0x0056,
            x_advance: 6.0,
            nominal_x_advance: 6.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: "V".to_string(),
        },
    ];

    assert_eq!(shaped_text_operator(12.0, &glyphs), "<00410056> Tj");
}

#[tokio::test]
async fn shaped_text_operator_uses_tj_for_shaped_advance_deltas() {
    let glyphs = vec![
        ShapedGlyph {
            id: 0x0041,
            x_advance: 5.4,
            nominal_x_advance: 6.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: "A".to_string(),
        },
        ShapedGlyph {
            id: 0x0056,
            x_advance: 6.0,
            nominal_x_advance: 6.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: "V".to_string(),
        },
    ];

    assert_eq!(
        shaped_text_operator(12.0, &glyphs),
        "[<0041> 50.000 <0056>] TJ"
    );
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
    page.push_line(RenderedLine {
        text: "AB".to_string(),
        x: 10.0,
        y: 40.0,
        font_size: 12.0,
        font_id: Some(0),
        color: Color::BLACK,
        runs: vec![
            RenderedTextRun {
                text: "A".to_string(),
                x_offset: 0.0,
                font_size: 12.0,
                font_id: Some(0),
                glyphs: Some(vec![test_rendered_glyph(glyph_a, "A")]),
            },
            RenderedTextRun {
                text: "B".to_string(),
                x_offset: 7.0,
                font_size: 12.0,
                font_id: Some(1),
                glyphs: Some(vec![test_rendered_glyph(glyph_b, "B")]),
            },
        ],
    });
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
    assert!(rendered.contains(&format!("<{glyph_a:04X}> Tj")));
    assert!(rendered.contains(&format!("<{glyph_b:04X}> Tj")));
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
    RenderedGlyph {
        id,
        x_advance: 7.0,
        nominal_x_advance: 7.0,
        x_offset: 0.0,
        y_offset: 0.0,
        unicode: unicode.to_string(),
    }
}
