//! Integration tests for the public semantic document-inspection API.

use quire::{Html, PageMargins, RenderOptions};

#[tokio::test]
async fn semantic_document_inspection_exposes_only_rendered_document_data() {
    let mut options = RenderOptions::default();
    options.set_page_margins(PageMargins::all_points(0.0));
    let document = Html::from_string(
        "<title>Semantic API</title><a href=\"https://example.test/guide\">Guide</a>",
    )
    .render(&options)
    .await
    .unwrap();

    assert_eq!(document.metadata().title(), Some("Semantic API"));
    assert!(document.bookmarks().is_empty());

    let page = &document.pages()[0];
    assert!(page.width() > 0.0);
    assert!(page.height() > 0.0);
    assert_eq!(page.rotation(), 0);

    let link = &page.links()[0];
    assert_eq!(link.target(), "https://example.test/guide");
    assert!(link.width() > 0.0);
    assert!(link.height() > 0.0);
}
