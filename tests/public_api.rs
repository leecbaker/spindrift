//! Integration tests for the public document metadata and bookmark API.

use quire::{Html, PdfCompression, PdfOptions, RenderOptions};

#[tokio::test]
async fn document_metadata_and_bookmarks_remain_public_without_page_paint_data() {
    let document = Html::from_string(concat!(
        r#"<html lang="en-GB"><title>Semantic API</title>"#,
        r#"<meta name="description" content="API guide">"#,
        r#"<meta name="keywords" content="PDF, Rust">"#,
        r#"<meta name="keywords" content=" Rust, layout, ">"#,
        r#"<meta name="dcterms.created" content="1997-07-16T19:20+01:00">"#,
        r#"<meta name="dcterms.modified" content="1998-12-23"><p>Guide</p>"#,
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.metadata().title(), Some("Semantic API"));
    assert_eq!(document.metadata().language(), Some("en-GB"));
    assert_eq!(document.metadata().description(), Some("API guide"));
    assert_eq!(document.metadata().keywords(), ["PDF", "Rust", "layout"]);
    assert_eq!(
        document
            .metadata()
            .created()
            .map(quire::DocumentDate::as_str),
        Some("1997-07-16T19:20+01:00")
    );
    assert_eq!(
        document
            .metadata()
            .modified()
            .map(quire::DocumentDate::as_str),
        Some("1998-12-23")
    );
    assert!(document.bookmarks().is_empty());

    for source in [
        "1997",
        "1997-07",
        "1997-07-16",
        "1997-07-16T19:20+01:00",
        "1997-07-16T19:20:30Z",
        "1997-07-16T19:20:30.45-00:30",
    ] {
        let date_document = Html::from_string(format!(
            r#"<meta name="dcterms.created" content="{source}"><p>Guide</p>"#
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();
        assert_eq!(
            date_document
                .metadata()
                .created()
                .map(quire::DocumentDate::as_str),
            Some(source)
        );
    }

    let mut pdf = Vec::new();
    document
        .write_pdf(
            &mut pdf,
            &PdfOptions {
                compression: PdfCompression::Uncompressed,
                ..PdfOptions::default()
            },
        )
        .unwrap();
    let pdf = String::from_utf8_lossy(&pdf);
    assert!(pdf.contains("/Subject (API guide)"));
    assert!(pdf.contains("/Keywords (PDF, Rust, layout)"));
    assert!(pdf.contains("/CreationDate (D:19970716192000+01'00)"));
    assert!(pdf.contains("/ModDate (D:19981223)"));
    assert!(pdf.contains("<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">API guide"));
    assert!(pdf.contains("<pdf:Keywords>PDF, Rust, layout</pdf:Keywords>"));
    assert!(pdf.contains("<xmp:CreateDate>1997-07-16T19:20+01:00</xmp:CreateDate>"));
    assert!(pdf.contains("<xmp:ModifyDate>1998-12-23</xmp:ModifyDate>"));
}
