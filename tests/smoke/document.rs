use super::*;

#[tokio::test]
async fn renders_hello_world_pdf() {
    let pdf = Html::from_string("<p>Hello, world</p>")
        .write_pdf_bytes_async(&RenderOptions::default())
        .await
        .unwrap();

    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.starts_with("%PDF-1.7"));
    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(rendered.contains("/ToUnicode"));
    assert!(rendered.contains("startxref"));
}

#[tokio::test]
async fn exposes_document_pages() {
    let document = Html::from_string("<p>Hello, world</p>")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].lines[0].text, "Hello, world");
}

#[tokio::test]
async fn block_align_content_center_aligns_contents_in_definite_height() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 50pt; height: 80pt; align-content: center; background: red }\
         .item { height: 20pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("block child background should paint");

    assert!(
        (green.y() - red.y() - 30.0).abs() < 0.01,
        "red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn block_align_content_center_aligns_contents_in_definite_min_height() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 50pt; min-height: 80pt; align-content: center; background: red }\
         .item { height: 20pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("block child background should paint");

    assert!(
        (green.y() - red.y() - 30.0).abs() < 0.01,
        "align-content:center should use the min-height-constrained block size: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn vertical_lr_block_align_content_center_uses_horizontal_block_axis() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { writing-mode: vertical-lr; width: 80pt; height: 50pt; align-content: center; background: red }\
         .item { width: 20pt; height: 40pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("vertical block container background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("vertical block child background should paint");

    assert!(
        (green.x() - red.x() - 30.0).abs() < 0.01,
        "vertical-lr align-content:center should center on the horizontal block axis: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn vertical_rl_block_align_content_end_uses_right_to_left_block_axis() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { writing-mode: vertical-rl; width: 80pt; height: 50pt; align-content: end; background: red }\
         .item { width: 20pt; height: 40pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("vertical block container background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("vertical block child background should paint");

    assert!(
        (green.x() - red.x()).abs() < 0.01,
        "vertical-rl align-content:end should pack content against physical left/block-end: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn block_align_content_translates_descendant_bookmark_targets() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 80pt; height: 80pt; align-content: center; background: red }\
         h2 { margin: 0; font-size: 10pt; line-height: 10pt; bookmark-level: 2; background: green }</style>\
         <div class=\"block\"><h2>Target</h2></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let bookmark = document
        .bookmarks
        .iter()
        .find(|bookmark| bookmark.label == "Target")
        .expect("heading bookmark should be exposed");
    let heading_background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("heading background should paint");

    assert!(
        (bookmark.y() - (heading_background.y() + heading_background.height())).abs() < 0.01,
        "bookmark target should follow align-content translation: bookmark={bookmark:?}, heading={heading_background:?}"
    );
}

#[tokio::test]
async fn vertical_block_align_content_translates_descendant_bookmark_targets() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { writing-mode: vertical-lr; width: 80pt; height: 50pt; align-content: center; background: red }\
         h2 { margin: 0; width: 20pt; height: 40pt; font-size: 10pt; line-height: 10pt; bookmark-level: 2; background: green }</style>\
         <div class=\"block\"><h2>Target</h2></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let bookmark = document
        .bookmarks
        .iter()
        .find(|bookmark| bookmark.label == "Target")
        .expect("heading bookmark should be exposed");
    let heading_background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("heading background should paint");

    assert!(
        (bookmark.x() - heading_background.x()).abs() < 0.01,
        "vertical bookmark target should follow align-content translation: bookmark={bookmark:?}, heading={heading_background:?}"
    );
}

#[tokio::test]
async fn block_align_content_translates_descendant_link_annotations() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 80pt; height: 80pt; align-content: center; font-size: 10pt; line-height: 10pt; background: red }\
         a { color: black; text-decoration: none }</style>\
         <div class=\"block\"><a href=\"https://example.com\">Link</a></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Link")
        .expect("linked text should render");
    let link = document.pages[0]
        .links
        .iter()
        .find(|link| link.target == "https://example.com")
        .expect("link annotation should be exposed");

    assert!(
        (link.y() - (line.y() - 2.0)).abs() < 0.01,
        "link annotation should follow align-content translation: link={link:?}, line={line:?}"
    );
}

#[tokio::test]
async fn block_align_content_does_not_translate_absolute_descendants() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { position: relative; width: 80pt; height: 80pt; align-content: center; background: red }\
         .flow { height: 20pt; background: green }\
         .abs { position: absolute; left: 20pt; top: 0; width: 20pt; height: 10pt; background: blue }</style>\
         <div class=\"block\"><div class=\"flow\"></div><div class=\"abs\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("normal-flow child should paint");
    let blue = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("absolute child should paint");

    assert!(
        (green.y() - red.y() - 30.0).abs() < 0.01,
        "normal-flow child should be aligned: red={red:?}, green={green:?}"
    );
    assert!(
        (blue.y() + blue.height() - (red.y() + red.height())).abs() < 0.01,
        "absolute child should stay at its inset position: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn block_align_content_safe_center_overflow_falls_back_to_start() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 50pt; height: 20pt; align-content: safe center; background: red }\
         .item { height: 40pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("block child background should paint");

    assert!(
        (green.y() + green.height() - red.y() - red.height()).abs() < 0.01,
        "safe center overflow should keep the child against block-start: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn block_align_content_center_overflow_defaults_to_safe_start() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 50pt; height: 20pt; align-content: center; background: red }\
         .item { height: 40pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("block child background should paint");

    assert!(
        (green.y() + green.height() - red.y() - red.height()).abs() < 0.01,
        "default center overflow should use safe block-start fallback: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn block_align_content_scroll_container_overflow_defaults_to_unsafe() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 50pt; height: 20pt; overflow-y: auto; align-content: center; background: red }\
         .item { height: 40pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("block child background should paint");

    assert!(
        (green.y() - (red.y() - 10.0)).abs() < 0.01
            && (green.y() + green.height() - (red.y() + red.height() + 10.0)).abs() < 0.01,
        "default center overflow in a scroll container should remain unsafe: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn block_align_content_unsafe_center_allows_symmetric_overflow() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 50pt; height: 20pt; align-content: unsafe center; background: red }\
         .item { height: 40pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("block child background should paint");

    assert!(
        (green.y() - (red.y() - 10.0)).abs() < 0.01
            && (green.y() + green.height() - (red.y() + red.height() + 10.0)).abs() < 0.01,
        "unsafe center should allow equal overflow on both block sides: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn legacy_pages_without_operations_synthesize_paint_order() {
    let mut page = quire::Page::new(100.0, 100.0);
    page.rects = vec![quire::RenderedRect::new(
        0.0,
        0.0,
        10.0,
        10.0,
        Some(Color::BLACK),
        None,
        0.0,
    )];
    let document = quire::Document {
        pages: vec![page],
        metadata: quire::DocumentMetadata::default(),
        fonts: Vec::new(),
        bookmarks: Vec::new(),
    };

    assert_eq!(
        document.pages[0].paint_operations().as_ref(),
        &[quire::PaintOperation::Rect(0)]
    );
    assert!(document.write_pdf_bytes().is_ok());
}

#[tokio::test]
async fn rounded_rects_participate_in_paint_order_and_pdf_serialization() {
    let mut page = quire::Page::new(100.0, 100.0);
    page.operations = vec![quire::PaintOperation::RoundedRect(0)];
    page.rounded_rects = vec![quire::RenderedRoundedRect::new(
        10.0,
        10.0,
        30.0,
        20.0,
        quire::RenderedRoundedRectRadii {
            top_left: quire::RenderedCornerRadius::new(4.0, 4.0),
            top_right: quire::RenderedCornerRadius::new(4.0, 4.0),
            bottom_right: quire::RenderedCornerRadius::new(4.0, 4.0),
            bottom_left: quire::RenderedCornerRadius::new(4.0, 4.0),
        },
        Some(Color::BLACK),
        None,
        0.0,
    )];
    let document = quire::Document {
        pages: vec![page],
        metadata: quire::DocumentMetadata::default(),
        fonts: Vec::new(),
        bookmarks: Vec::new(),
    };

    assert_eq!(
        document.pages[0].paint_operations().as_ref(),
        &[quire::PaintOperation::RoundedRect(0)]
    );
    assert!(document.write_pdf_bytes().is_ok());
}

#[tokio::test]
async fn invalid_paint_operation_indexes_fail_before_pdf_serialization() {
    let mut page = quire::Page::new(100.0, 100.0);
    page.operations = vec![quire::PaintOperation::Rect(1)];
    page.rects = vec![quire::RenderedRect::new(
        0.0,
        0.0,
        10.0,
        10.0,
        Some(Color::BLACK),
        None,
        0.0,
    )];
    let document = quire::Document {
        pages: vec![page],
        metadata: quire::DocumentMetadata::default(),
        fonts: Vec::new(),
        bookmarks: Vec::new(),
    };

    let error = document.write_pdf_bytes().unwrap_err().to_string();
    assert!(error.contains("paint operation 0 references missing rect 1"));
}

#[tokio::test]
async fn incomplete_paint_operation_streams_fail_before_pdf_serialization() {
    let mut page = quire::Page::new(100.0, 100.0);
    page.operations = vec![quire::PaintOperation::Rect(0)];
    page.rects = vec![
        quire::RenderedRect::new(0.0, 0.0, 10.0, 10.0, Some(Color::BLACK), None, 0.0),
        quire::RenderedRect::new(10.0, 10.0, 10.0, 10.0, Some(Color::WHITE), None, 0.0),
    ];
    let document = quire::Document {
        pages: vec![page],
        metadata: quire::DocumentMetadata::default(),
        fonts: Vec::new(),
        bookmarks: Vec::new(),
    };

    let error = document.write_pdf_bytes().unwrap_err().to_string();
    assert!(error.contains("unreferenced rect 1"));
}

#[tokio::test]
async fn exposes_default_heading_bookmarks() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 10pt } body, h2, h4 { margin: 0; font-size: 10pt; line-height: 10pt }</style><h2>Chapter</h2><h4>Section</h4><h2>Next</h2>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.bookmarks.len(), 3);
    assert_eq!(document.bookmarks[0].level, 2);
    assert_eq!(document.bookmarks[0].label, "Chapter");
    assert_eq!(document.bookmarks[0].page_index, 0);
    assert_eq!(document.bookmarks[0].state, BookmarkState::Open);
    assert_eq!(document.bookmarks[1].level, 4);
    assert_eq!(document.bookmarks[1].label, "Section");
    assert_eq!(document.bookmarks[2].level, 2);
    assert_eq!(document.bookmarks[2].label, "Next");
}

#[tokio::test]
async fn supports_authored_css_bookmarks() {
    let document = Html::from_string(
        r#"<style>
        h1 { bookmark-level: none }
        section { display: block; bookmark-level: 2; bookmark-label: "Custom " attr(data-title) ": " content(text); bookmark-state: closed }
        </style>
        <h1>Ignored</h1>
        <section data-title="Part A">Section Label</section>"#,
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.bookmarks.len(), 1);
    assert_eq!(document.bookmarks[0].level, 2);
    assert_eq!(document.bookmarks[0].label, "Custom Part A: Section Label");
    assert_eq!(document.bookmarks[0].state, BookmarkState::Closed);
}

#[tokio::test]
async fn writes_pdf_outline_tree_for_heading_bookmarks() {
    let pdf = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 10pt } body, h2, h4 { margin: 0; font-size: 10pt; line-height: 10pt }</style><h2>Chapter</h2><h4>Section</h4>",
    )
    .write_pdf_bytes_async(&RenderOptions::default()).await
    .unwrap();
    let rendered = String::from_utf8_lossy(&pdf);

    assert!(rendered.contains("/Outlines"));
    assert!(rendered.contains("/Title (Chapter)"));
    assert!(rendered.contains("/Title (Section)"));
    assert!(rendered.contains("/Count 2"));
    assert!(rendered.contains("/Count 1"));
    assert!(rendered.contains("/Dest [4 0 R /XYZ"));
}

#[tokio::test]
async fn applies_minimal_page_css() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string(
            "@page { size: 200pt 100pt; margin: 10pt } body, p { margin: 0 }",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].width(), 200.0);
    assert_eq!(document.pages[0].height(), 100.0);
    assert_eq!(document.pages[0].lines[0].x(), 10.0);
}

#[tokio::test]
async fn css_absolute_lengths_use_spec_ratios_in_layout() {
    let document = Html::from_string(
        r#"<style>
        @page { size: 1000pt 400pt; margin: 0 }
        body { margin: 0 }
        div { display: block; width: 25cm; height: 8cm; background: black }
        </style><div></div>"#,
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let rect = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .expect("black div background should be painted");

    // CSS Values and Units defines 1in = 96px = 72pt and 1cm = 1in / 2.54.
    assert!((rect.width() - (25.0 * 72.0 / 2.54)).abs() < 0.001);
    assert!((rect.height() - (8.0 * 72.0 / 2.54)).abs() < 0.001);
}

#[tokio::test]
async fn applies_asymmetric_page_margins_to_page_area() {
    let document =
        Html::from_string("<p style=\"margin:0;font-size:10pt;line-height:10pt\">Hello</p>")
            .with_stylesheet(Css::from_string(
                "@page { size: 200pt 120pt; margin: 20pt 30pt 40pt 10pt } body { margin: 0 }",
            ))
            .render_async(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(document.pages[0].lines[0].x(), 10.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines[0], 100.0);
}

#[tokio::test]
async fn viewport_units_resolve_to_paged_media_page_area() {
    let wpt_root = std::path::Path::new("/Users/lee/oss/quire-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }
    let source =
        std::fs::read_to_string(wpt_root.join("css/css-page/page-margin-001-print.html")).unwrap();
    let document = Html::from_string(source)
        .with_base_url(wpt_root)
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages.len(), 3);
    for (page, color) in document.pages.iter().zip([
        Color::new(255, 255, 0),
        Color::new(0, 255, 255),
        Color::new(255, 192, 203),
    ]) {
        assert_eq!(final_rect_fill_at(page, 35.0, 30.0), Some(color));
        assert_eq!(
            final_rect_fill_at(page, page.width() - 20.0, page.height() - 10.0),
            Some(color)
        );
        assert_ne!(final_rect_fill_at(page, 20.0, 30.0), Some(color));
        assert_ne!(final_rect_fill_at(page, 35.0, 10.0), Some(color));
    }
}

#[tokio::test]
async fn left_and_right_page_selectors_set_alternating_page_areas() {
    let wpt_root = std::path::Path::new("/Users/lee/oss/quire-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }
    let source =
        std::fs::read_to_string(wpt_root.join("css/css-page/page-left-right-001-print.html"))
            .unwrap();
    let document = Html::from_string(source)
        .with_base_url(wpt_root)
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages.len(), 4);
    for (index, page) in document.pages.iter().enumerate() {
        if index.is_multiple_of(2) {
            assert_eq!(
                final_rect_fill_at(page, 10.0, 10.0),
                Some(Color::new(255, 255, 0))
            );
            assert_ne!(
                final_rect_fill_at(page, page.width() - 10.0, page.height() - 10.0),
                Some(Color::new(255, 255, 0))
            );
        } else {
            assert_eq!(
                final_rect_fill_at(page, page.width() - 10.0, page.height() - 10.0),
                Some(Color::new(255, 255, 0))
            );
            assert_ne!(
                final_rect_fill_at(page, 10.0, 10.0),
                Some(Color::new(255, 255, 0))
            );
        }
    }
}

#[tokio::test]
async fn page_margin_box_default_alignment_matches_wpt() {
    let wpt_root = std::path::Path::new("/Users/lee/oss/quire-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }
    let source = std::fs::read_to_string(
        wpt_root.join("css/css-page/margin-boxes/alignment-001-print.html"),
    )
    .unwrap();
    let document = Html::from_string(source)
        .with_base_url(wpt_root)
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let letters = ('A'..='P')
        .filter_map(|letter| {
            let text = letter.to_string();
            page.lines
                .iter()
                .find(|line| line.text == text)
                .map(|line| (letter, (line.x(), line.y())))
        })
        .collect::<std::collections::HashMap<_, _>>();

    assert_eq!(letters.len(), 16);
    assert!(letters[&'A'].0 < letters[&'B'].0);
    assert!(letters[&'B'].0 < letters[&'C'].0);
    assert!(letters[&'C'].0 < letters[&'D'].0);
    assert!(letters[&'D'].0 < letters[&'E'].0);
    assert!((letters[&'A'].1 - letters[&'E'].1).abs() < 0.01);

    assert!(letters[&'P'].1 > letters[&'O'].1);
    assert!(letters[&'O'].1 > letters[&'N'].1);
    assert!((letters[&'P'].0 - letters[&'N'].0).abs() < 0.01);
    assert!(letters[&'F'].1 > letters[&'G'].1);
    assert!(letters[&'G'].1 > letters[&'H'].1);
    assert!((letters[&'F'].0 - letters[&'H'].0).abs() < 0.01);

    assert!(letters[&'M'].0 < letters[&'L'].0);
    assert!(letters[&'L'].0 < letters[&'K'].0);
    assert!(letters[&'K'].0 < letters[&'J'].0);
    assert!(letters[&'J'].0 < letters[&'I'].0);
    assert!((letters[&'M'].1 - letters[&'I'].1).abs() < 0.01);
}

#[tokio::test]
async fn logical_viewport_units_use_writing_mode_axes() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 0 }\
         body, div { margin: 0; padding: 0 }\
         .horizontal { width: 50vi; height: 50vb; background: red }\
         .vertical { writing-mode: vertical-rl; width: 50vi; height: 50vb; background: blue }\
         </style><div class=\"horizontal\"></div><div class=\"vertical\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("horizontal vi/vb box should paint");
    assert!((red.width() - 100.0).abs() < 0.01);
    assert!((red.height() - 50.0).abs() < 0.01);

    assert_eq!(document.pages.len(), 2);
    let blue = document.pages[1]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("vertical vi/vb box should paint");
    assert!((blue.width() - 50.0).abs() < 0.01);
    assert!((blue.height() - 100.0).abs() < 0.01);
}

#[tokio::test]
async fn page_border_and_padding_shrink_page_content_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; border: 5pt solid green; padding: 7pt }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = &document.pages[0].lines[0];
    assert_eq!(line.x(), 22.0);
    assert_line_baseline_at_top(&document, line, 78.0);
}

#[tokio::test]
async fn logical_page_margins_and_padding_use_page_writing_mode() {
    let document = Html::from_string(
        "<style>\
         @page { writing-mode: vertical-rl; size: 400pt 800pt;\
           margin-inline-start: 2%; margin-block-start: 8%;\
           margin-inline-end: 6%; margin-block-end: 20%;\
           padding-inline-start: 2%; padding-block-start: 8%;\
           padding-inline-end: 6%; padding-block-end: 20% }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = &document.pages[0].lines[0];
    assert_eq!(line.x(), 160.0);
    assert_line_baseline_at_top(&document, line, 768.0);
}

#[tokio::test]
async fn page_border_paints_below_document_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; border: 5pt solid rgb(0 128 0) }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];
    let green = Color::new(0, 128, 0);
    let border_rect_index = page
        .rects
        .iter()
        .position(|rect| rect.fill == Some(green))
        .expect("page border should paint green rect primitives");
    let border_operation = page
        .operations
        .iter()
        .position(|operation| {
            matches!(operation, quire::PaintOperation::Rect(index) if *index == border_rect_index)
        })
        .expect("green page border rect should participate in paint order");
    let line_operation = page
        .operations
        .iter()
        .position(|operation| matches!(operation, quire::PaintOperation::Line(0)))
        .expect("document line should participate in paint order");

    assert!(border_operation < line_operation);
    assert!(page.rects.iter().any(|rect| {
        rect.fill == Some(green)
            && ((rect.width() - 120.0).abs() < 0.01 && (rect.height() - 5.0).abs() < 0.01
                || (rect.width() - 5.0).abs() < 0.01 && (rect.height() - 100.0).abs() < 0.01)
    }));
}

#[tokio::test]
async fn renders_page_margin_box_counters() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string(
            "@page { size: 200pt 100pt; margin: 10pt; @bottom-right { content: \"Page \" counter(page) \" of \" counter(pages); background-color: black; color: white; height: 10pt; width: 50%; font-size: 8pt } }",
        ))
        .render_async(&RenderOptions::default()).await
        .unwrap();

    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.y() == 0.0 && rect.fill == Some(Color::BLACK))
    );
    assert!(document.pages[0].operations.iter().any(|operation| {
        matches!(
            operation,
            quire::PaintOperation::Rect(index)
                if document.pages[0]
                    .rects
                    .get(*index)
                    .is_some_and(|rect| rect.y() == 0.0 && rect.fill == Some(Color::BLACK))
        )
    }));
    let footer = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("Page 1 of 1") && line.color == Color::WHITE)
        .unwrap();
    assert!(
        footer
            .runs
            .iter()
            .any(|run| run.text.contains("Page 1 of 1")
                && run.glyphs.as_ref().is_some_and(|glyphs| !glyphs.is_empty()))
    );
    assert!(document.pages[0].operations.iter().any(|operation| {
        matches!(
            operation,
            quire::PaintOperation::Line(index)
                if document.pages[0]
                    .lines
                    .get(*index)
                    .is_some_and(|line| line.text.contains("Page 1 of 1") && line.color == Color::WHITE)
        )
    }));
}

#[tokio::test]
async fn page_margin_box_page_counters_accept_counter_styles() {
    let document = Html::from_string(
        "<p>One</p><article><p>Two</p></article>\
         <style>body, p, article { margin: 0; font-size: 8pt; line-height: 8pt }\
         article { break-before: page }</style>",
    )
    .with_stylesheet(Css::from_string(
        "@page { size: 120pt 80pt; margin: 10pt; \
         @bottom-center { content: counter(page, upper-roman) \" / \" counter(pages, decimal-leading-zero); font-size: 8pt; height: 10pt } }",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "I / 02")
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "II / 02")
    );
}

#[tokio::test]
async fn page_margin_box_page_counters_use_custom_counter_styles() {
    let document = Html::from_string(
        "<p>One</p><article><p>Two</p></article>\
         <style>body, p, article { margin: 0; font-size: 8pt; line-height: 8pt }\
         article { break-before: page }</style>",
    )
    .with_stylesheet(Css::from_string(
        "@counter-style binary { system: numeric; symbols: \"0\" \"1\" }\
         @page { size: 120pt 80pt; margin: 10pt; \
         @bottom-center { content: counter(page, binary); font-size: 8pt; height: 10pt } }",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(document.pages[0].lines.iter().any(|line| line.text == "1"));
    assert!(document.pages[1].lines.iter().any(|line| line.text == "10"));
}

#[tokio::test]
async fn page_margin_box_generic_counters_use_root_seed_and_page_increment() {
    let document = Html::from_string(
        "<style>html { counter-reset: foo 10 } body, p, article { margin: 0; font-size: 8pt; line-height: 8pt } article { break-before: page }</style>\
         <p>One</p><article><p>Two</p></article><article><p>Three</p></article>",
    )
    .with_stylesheet(Css::from_string(
        "@page { size: 120pt 80pt; margin: 10pt; counter-increment: foo; \
         @bottom-center { content: counter(foo); font-size: 8pt; height: 10pt } }",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(document.pages[0].lines.iter().any(|line| line.text == "11"));
    assert!(document.pages[1].lines.iter().any(|line| line.text == "12"));
    assert!(document.pages[2].lines.iter().any(|line| line.text == "13"));
}

#[tokio::test]
async fn page_margin_box_counters_function_uses_page_counter_value() {
    let document = Html::from_string(
        "<style>html { counter-reset: foo 3 } body, p, article { margin: 0; font-size: 8pt; line-height: 8pt } article { break-before: page }</style>\
         <p>One</p><article><p>Two</p></article>",
    )
    .with_stylesheet(Css::from_string(
        "@page { size: 120pt 80pt; margin: 10pt; counter-increment: foo 2; \
         @bottom-center { content: counters(foo, \".\", upper-roman); font-size: 8pt; height: 10pt } }",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(document.pages[0].lines.iter().any(|line| line.text == "V"));
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "VII")
    );
}

#[tokio::test]
async fn page_margin_box_background_images_repeat_across_margin_area() {
    let document = Html::from_string("<p>Page</p>")
        .with_stylesheet(Css::from_string(
            "@page { size: 80pt 60pt; margin: 10pt; \
             @top-center { content: \"\"; background-image: url(data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==); background-size: 10pt 10pt; background-repeat: repeat; } }\
             body, p { margin: 0 }",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let top_margin_tiles = document.pages[0]
        .images
        .iter()
        .filter(|image| image.background && (image.y() - 50.0).abs() < 0.01)
        .collect::<Vec<_>>();

    assert!(
        top_margin_tiles.len() >= 6,
        "top-center page-margin background should tile across the margin area: {top_margin_tiles:?}"
    );
    assert!(
        top_margin_tiles
            .iter()
            .any(|image| (image.x() - 10.0).abs() < 0.01)
    );
    assert!(
        top_margin_tiles
            .iter()
            .any(|image| (image.x() - 60.0).abs() < 0.01)
    );
}

#[tokio::test]
async fn page_margin_box_target_counter_resolves_anchor_page() {
    let document = Html::from_string(
        "<p>One</p><article id=\"chapter\"><p>Two</p></article>\
         <style>body, p, article { margin: 0; font-size: 8pt; line-height: 8pt }\
         article { break-before: page }</style>",
    )
    .with_stylesheet(Css::from_string(
        "@page { size: 120pt 80pt; margin: 10pt; \
         @bottom-center { content: \"Chapter \" target-counter(url(#chapter), page, upper-roman); font-size: 8pt; height: 10pt } }",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Chapter II")
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Chapter II")
    );
}

#[tokio::test]
async fn page_margin_box_target_text_resolves_anchor_text() {
    let document = Html::from_string(
        "<p>One</p><article id=\"chapter\"><p>Two</p></article>\
         <style>body, p, article { margin: 0; font-size: 8pt; line-height: 8pt }\
         article { break-before: page }\
         article::before { content: \"Before \" }</style>",
    )
    .with_stylesheet(Css::from_string(
        "@page { size: 120pt 80pt; margin: 10pt; \
         @bottom-center { content: target-text(url(#chapter)) \" / \" target-text(url(#chapter), before); font-size: 8pt; height: 10pt } }",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Two / Before")
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Two / Before")
    );
}

#[tokio::test]
async fn target_text_can_capture_generated_counter_and_attr_text() {
    let document = Html::from_string(
        "<h1 id=\"chapter\" data-label=\"Intro\">Heading</h1>\
         <style>body, h1 { margin: 0; font-size: 8pt; line-height: 8pt }\
         body { counter-reset: chapter }\
         h1 { counter-increment: chapter }\
         h1::before { content: \"Chapter \" counter(chapter) \" \" attr(data-label) }\
         h1::after { content: \" done\" }</style>",
    )
    .with_stylesheet(Css::from_string(
        "@page { size: 160pt 80pt; margin: 10pt; \
         @bottom-center { content: target-text(url(#chapter), before) \"/\" target-text(url(#chapter), after); font-size: 8pt; height: 10pt } }",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Chapter 1 Intro/ done")
    );
}

#[tokio::test]
async fn page_margin_generated_forced_breaks_use_inline_line_sequence() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 90pt; margin: 10pt; @top-center { content: \"Head\\A Tail\"; white-space: pre-line; font-size: 8pt; line-height: 8pt; height: 18pt } }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.contains(&"Head"), "{texts:?}");
    assert!(texts.contains(&"Tail"), "{texts:?}");
}

#[tokio::test]
async fn page_margin_box_string_function_uses_named_string_from_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 10pt; @top-center { content: string(chapter); font-size: 8pt; height: 10pt } }\
         body, h1, article { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { string-set: chapter content(text) }\
         article { display: block; break-before: page }\
         </style><h1>Intro</h1><article><h1>Methods</h1></article>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Intro" && line.y() > 65.0)
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Methods" && line.y() > 65.0)
    );
    assert!(
        !document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Methods")
    );
}

#[tokio::test]
async fn named_string_break_before_is_assigned_to_target_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 10pt; @top-center { content: string(chapter); font-size: 8pt; height: 10pt } }\
         body, p, h1 { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { break-before: page; string-set: chapter content(text) }\
         </style><p>Intro</p><h1>Methods</h1>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        !document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Methods" && line.y() > 65.0)
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Methods" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn named_string_start_uses_only_page_start_assignment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 80pt; margin: 10pt; @top-center { content: string(chapter, start); font-size: 8pt; height: 10pt } }\
         body, p, h1, section { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { string-set: chapter content(text) }\
         section { display: block; break-before: page }\
         </style>\
         <h1>First</h1>\
         <p>Intro</p>\
         <section><p>Lead</p><h1>Later</h1><p>Body</p></section>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "First" && line.y() > 65.0)
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "First" && line.y() > 65.0),
        "{:?}",
        document.pages[1].lines
    );
    assert!(
        !document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Later" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn running_element_start_uses_zero_size_source_marker() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 80pt; margin: 10pt; @top-center { content: element(header, start); font-size: 8pt; height: 10pt } }\
         body, p, h1 { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { position: running(header) }\
         .next { break-before: page }\
         </style>\
         <h1>First</h1>\
         <p>Intro</p><p class=\"next\">Lead</p><h1>Later</h1><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "First" && line.y() > 65.0),
        "{:?}",
        document.pages[1].lines
    );
    assert!(
        !document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Later" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn page_margin_box_named_string_keywords_use_page_assignments() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 10pt;\
           @top-left { content: string(chapter, first); font-size: 8pt; height: 10pt }\
           @bottom-left { content: string(chapter, last); font-size: 8pt; height: 10pt }\
         }\
         body, h1 { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { string-set: chapter content(text) }\
         </style><h1>First</h1><h1>Last</h1>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "First" && line.y() > 65.0)
    );
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Last" && line.y() < 15.0)
    );
}

#[tokio::test]
async fn page_margin_box_named_string_first_except_skips_assignment_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 10pt; @top-center { content: string(chapter, first-except); font-size: 8pt; height: 10pt } }\
         body, h1, article { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { string-set: chapter content(text) }\
         article { display: block; break-before: page }\
         </style><h1>Intro</h1><article>Next</article>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        !document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Intro" && line.y() > 65.0)
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Intro" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn page_margin_box_named_strings_are_case_sensitive() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 80pt; margin: 10pt; @top-center { content: string(text_header) \" \" string(TEXT_header); font-size: 8pt; height: 10pt } }\
         body, p, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         p { string-set: text_header content(text) }\
         div { string-set: TEXT_header content(text) }\
         </style><p>lower</p><div>upper</div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "lower upper" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn string_set_can_capture_before_and_after_generated_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 80pt; margin: 10pt; @top-center { content: string(label); font-size: 8pt; height: 10pt } }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         body { counter-reset: section }\
         p { counter-increment: section }\
         p::before { content: \"Before \" counter(section) \" \" attr(data-label) }\
         p::after { content: attr(data-suffix, \"After\") }\
         p { string-set: label content(before) \"-\" content(text) \"-\" content(after) }\
         </style><p data-label=\"Intro\">Text</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Before 1 Intro-Text-After" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn string_set_captures_pseudo_counter_without_double_incrementing_layout() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 80pt; margin: 10pt; @top-center { content: string(label); font-size: 8pt; height: 10pt } }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         body { counter-reset: c }\
         p::before { counter-increment: c; content: counter(c) }\
         p { string-set: label content(before) \"-\" counter(c) }\
         </style><p>Text</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.contains(&"1-0"), "{texts:?}");
    assert!(texts.contains(&"1Text"), "{texts:?}");
}

#[tokio::test]
async fn string_set_can_capture_image_items_for_page_margin_content() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 160pt 80pt; margin: 12pt; @top-left {{ content: string(label); font-size: 8pt; height: 10pt }} }}\
         body, h1 {{ margin: 0; font-size: 10pt; line-height: 10pt }}\
         h1 {{ string-set: label \"Icon\" url({png}) }}\
         </style><h1>Heading</h1>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Icon" && line.y() > 65.0)
    );
    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(document.pages[0].images[0].pixel_width, 1);
    assert_eq!(document.pages[0].images[0].pixel_height, 1);
}

#[tokio::test]
async fn running_element_capture_includes_generated_counter_and_attr_text() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 80pt; margin: 10pt; @top-center { content: element(header); font-size: 8pt; height: 10pt } }\
         body, h1, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         body { counter-reset: chapter }\
         h1 { position: running(header); counter-increment: chapter }\
         h1::before { content: \"Chapter \" counter(chapter) \" \" attr(data-label) \" \" }\
         h1::after { content: url(missing-running-element-image.png) \" done\" }\
         </style><h1 data-label=\"Intro\">Heading</h1><p>Body</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Chapter 1 Intro Heading done" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn running_element_image_replays_into_page_margin_content() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 120pt 80pt; margin: 12pt; @top-center {{ content: element(logo); height: 10pt }} }}\
         body {{ margin: 0 }}\
         img {{ position: running(logo); width: 8pt; height: 8pt }}\
         </style><img src=\"{png}\"><p>Body</p>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(document.pages[0].images[0].pixel_width, 1);
    assert_eq!(document.pages[0].images[0].pixel_height, 1);
}

#[tokio::test]
async fn running_element_replays_source_box_background_and_dimensions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 90pt; margin: 12pt; @top-center { content: element(header); width: 70pt; height: 14pt } }\
         body, h1, p { margin: 0; font-size: 8pt; line-height: 8pt }\
         h1 { position: running(header); width: 40pt; height: 8pt; background: rgb(255, 0, 0); border: 1pt solid rgb(0, 0, 255) }\
         </style><h1>Box Header</h1><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let replayed_background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("running element source background should paint in the page margin");
    assert!((replayed_background.width() - 42.0).abs() < 0.01);
    assert!((replayed_background.height() - 10.0).abs() < 0.01);
}

#[tokio::test]
async fn running_element_keywords_match_page_local_assignments() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 90pt; margin: 12pt; @bottom-left { content: element(title, first); font-size: 8pt; height: 10pt }\
           @bottom-center { content: element(title, last); font-size: 8pt; height: 10pt }\
           @bottom-right { content: element(title, first-except); font-size: 8pt; height: 10pt } }\
         body, article, h1, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { position: running(title) }\
         </style>\
         <h1>Two first</h1><h1>Two last</h1><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let page_margin_text = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.y() < 15.0)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(page_margin_text.contains(&"Two first"));
    assert!(page_margin_text.contains(&"Two last"));
    assert_eq!(
        page_margin_text
            .iter()
            .filter(|text| **text == "Two first" || **text == "Two last")
            .count(),
        2
    );
}

#[tokio::test]
async fn string_set_can_capture_counters() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 80pt; margin: 10pt; @top-center { content: string(label); font-size: 8pt; height: 10pt } }\
         body, h1 { margin: 0; font-size: 10pt; line-height: 10pt }\
         body { counter-reset: chapter }\
         h1 { counter-increment: chapter; string-set: label \"Chapter \" counter(chapter, upper-roman) \": \" content(text) }\
         </style><h1>Methods</h1>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Chapter I: Methods" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn page_margin_box_element_function_uses_running_element_text() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 80pt; margin: 10pt; @top-center { content: element(header); font-size: 8pt; height: 10pt } }\
         body, h1, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { position: running(header) }\
         </style><h1>Running Header</h1><p>Body</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Running Header" && line.y() > 65.0)
    );
    assert!(
        !document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Running Header" && line.y() < 65.0)
    );
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Body")
    );
}

#[tokio::test]
async fn positions_page_margin_boxes_in_page_margins() {
    let document = Html::from_string("<p style=\"margin:0\">Hello</p>")
        .with_stylesheet(Css::from_string(
            "@page { size: 200pt 120pt; margin: 20pt 30pt 40pt 10pt; @bottom-right { content: \"x\"; background-color: black; width: 50%; height: 10pt; font-size: 8pt } } body { margin: 0 }",
        ))
        .render_async(&RenderOptions::default()).await
        .unwrap();

    let footer = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();

    assert_eq!(footer.width(), 80.0);
    assert_eq!(footer.x(), 90.0);
    assert_eq!(footer.y(), 30.0);
}

#[tokio::test]
async fn page_margin_box_auto_margins_center_fixed_axis() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 200pt; margin: 40pt; @top-left { content: \"\"; background-color: green; width: 20pt; height: 10pt; margin-top: auto; margin-bottom: auto } }\
         body, p { margin: 0 }\
         </style><p></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let header = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("generated page-margin box should paint a green background");

    assert_eq!(header.x(), 40.0);
    assert_eq!(header.y(), 175.0);
    assert_eq!(header.width(), 20.0);
    assert_eq!(header.height(), 10.0);
}

#[tokio::test]
async fn page_margin_box_overconstrained_fixed_axis_ignores_outer_margin() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 200pt; margin: 40pt; @top-left { content: \"\"; background-color: green; width: 20pt; height: 30pt; margin: 8pt } }\
         body, p { margin: 0 }\
         </style><p></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let header = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("generated page-margin box should paint a green background");

    assert_eq!(header.x(), 48.0);
    assert_eq!(header.y(), 168.0);
    assert_eq!(header.width(), 20.0);
    assert_eq!(header.height(), 30.0);
}

#[tokio::test]
async fn page_margin_box_fixed_axis_allows_negative_used_margins() {
    let document = Html::from_string(
        "<style>\
         @page { size: 300pt 300pt; margin: 100pt;\
           @left-middle { content: \"\"; background-color: green; width: 130pt; margin: 10pt }\
           @bottom-center { content: \"\"; background-color: blue; height: 150pt; margin: auto 0 }\
         }\
         body, p { margin: 0 }\
         </style><p></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let left = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("left margin box should paint");
    assert_eq!(left.x(), -40.0);
    assert_eq!(left.width(), 130.0);

    let bottom = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("bottom margin box should paint");
    assert_eq!(bottom.y(), -50.0);
    assert_eq!(bottom.height(), 150.0);
}

#[tokio::test]
async fn page_margin_boxes_use_page_border_and_padding_for_page_area_margins() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 200pt; width: 100pt; height: 100pt; margin: auto; border: 10pt solid black; padding: 10pt;\
           @top-left { content: \"\"; background-color: green; width: 10pt; height: 10pt }\
         }\
         body, p { margin: 0 }\
         </style><p></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let header = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("page margin box should paint in the top margin");

    assert_eq!(header.x(), 30.0);
    assert_eq!(header.y(), 170.0);
    assert_eq!(header.width(), 10.0);
    assert_eq!(header.height(), 10.0);
}

#[tokio::test]
async fn nth_page_selector_applies_to_page_margin_boxes() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt; @bottom-center { content: \"base\"; font-size: 8pt; height: 10pt } }\
         @page :nth(2) { @bottom-center { content: \"second\" } }\
         body, p { margin: 0 }\
         article { break-before: page }\
         </style><p>One</p><article>Two</article><article>Three</article>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "base"),
        "base page-margin content should remain on page 1"
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "second"),
        ":nth(2) page-margin content should apply on page 2"
    );
    assert!(
        document.pages[2]
            .lines
            .iter()
            .any(|line| line.text == "base"),
        "base page-margin content should apply again on page 3"
    );
}

#[tokio::test]
async fn page_margin_box_visibility_hidden_suppresses_paint() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 20pt;\
           @top-left { content: \"\"; background-color: green; width: 20pt; height: 10pt; visibility: hidden }\
         }\
         body, p { margin: 0 }\
         </style><p></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        !document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 128, 0)))
    );
}

#[tokio::test]
async fn page_margin_box_outline_paints_without_affecting_layout() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 20pt;\
           @top-left { content: \"\"; background-color: green; width: 20pt; height: 10pt; outline: 2pt solid red }\
         }\
         body, p { margin: 0 }\
         </style><p></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let header = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("page margin box background should paint");
    assert_eq!(header.x(), 20.0);
    assert_eq!(header.width(), 20.0);
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(255, 0, 0))),
        "outline should paint red primitives"
    );
}

#[tokio::test]
async fn page_margin_boxes_paint_in_clockwise_tree_order() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 20pt;\
           @top-left-corner { content: \"\"; background-color: rgb(255 0 0); width: 10pt; height: 10pt }\
           @top-left { content: \"\"; background-color: rgb(0 128 0); width: 10pt; height: 10pt }\
         }\
         body, p { margin: 0 }\
         </style><p></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = first_rect_paint_operation_index(&document.pages[0], Color::new(255, 0, 0));
    let green = first_rect_paint_operation_index(&document.pages[0], Color::new(0, 128, 0));

    assert!(red < green, "top-left-corner must paint before top-left");
}

#[tokio::test]
async fn negative_z_index_page_margin_boxes_paint_below_document_stack() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 20pt;\
           @bottom-left-corner { content: \"\"; background-color: cyan; width: 20pt; height: 20pt; z-index: -1 }\
           @bottom-right-corner { content: \"\"; background-color: yellow; width: 20pt; height: 20pt }\
         }\
         body { margin: 0; background: rgb(221 221 221) }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Text</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let cyan = first_rect_paint_operation_index(&document.pages[0], Color::new(0, 255, 255));
    let gray = first_rect_paint_operation_index(&document.pages[0], Color::new(221, 221, 221));
    let yellow = first_rect_paint_operation_index(&document.pages[0], Color::new(255, 255, 0));

    assert!(
        cyan < gray,
        "negative page-margin z-index paints below document stack"
    );
    assert!(
        gray < yellow,
        "auto page-margin z-index paints above document stack"
    );
}

#[tokio::test]
async fn first_page_rule_size_and_margins_define_first_page_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page :first { size: 120pt 120pt; margin: 20pt }\
         body, p, article { margin: 0; font-size: 10pt; line-height: 10pt }\
         article { display: block; break-before: page }\
         </style><p>One</p><article>Two</article>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].width(), 120.0);
    assert_eq!(document.pages[0].height(), 120.0);
    assert_eq!(document.pages[0].lines[0].x(), 20.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines[0], 100.0);
    assert_eq!(document.pages[1].width(), 100.0);
    assert_eq!(document.pages[1].height(), 100.0);
    assert_eq!(document.pages[1].lines[0].x(), 10.0);
    assert_line_baseline_at_top(&document, &document.pages[1].lines[0], 90.0);
}

#[tokio::test]
async fn generic_first_page_rule_preserves_body_margin_for_named_page_content() {
    let document = Html::from_string(
        "<style>\
         @page :first { size: 120pt 120pt; margin: 20pt }\
         @page { size: 120pt 120pt; margin: 0 }\
         div { display: block; width: 10pt; height: 10pt }\
         </style>\
         <div style=\"page: a; background: lightblue\"></div>\
         <div style=\"page: b; background: pink\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let first_rect = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(173, 216, 230)))
        .expect("lightblue first-page box should be painted");
    let second_rect = document.pages[1]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 192, 203)))
        .expect("pink second-page box should be painted");

    assert_eq!(document.pages.len(), 2);
    assert_eq!(first_rect.x(), 26.0);
    assert_eq!(first_rect.y(), 84.0);
    assert_eq!(second_rect.x(), 6.0);
    assert_eq!(second_rect.y(), 104.0);
}

#[tokio::test]
async fn left_page_rule_margins_define_left_page_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page :left { margin-left: 30pt; margin-top: 25pt }\
         body, p, article { margin: 0; font-size: 10pt; line-height: 10pt }\
         article { display: block; break-before: page }\
         </style><p>One</p><article>Two</article>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[1].lines[0].x(), 30.0);
    assert_line_baseline_at_top(&document, &document.pages[1].lines[0], 75.0);
}

#[tokio::test]
async fn page_specific_margins_define_page_margin_box_regions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt; @bottom-right { content: \"x\"; background-color: black; width: 50%; height: 10pt } }\
         @page :left { margin: 20pt }\
         body, p, article { margin: 0; font-size: 10pt; line-height: 10pt }\
         article { display: block; break-before: page }\
         </style><p>One</p><article>Two</article>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let footer = document.pages[1]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();

    assert_eq!(footer.width(), 30.0);
    assert_eq!(footer.x(), 50.0);
    assert_eq!(footer.y(), 10.0);
}

#[tokio::test]
async fn named_page_rule_size_and_margins_define_named_page_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page report { size: 160pt 120pt; margin: 20pt }\
         body, p, section { margin: 0; font-size: 10pt; line-height: 10pt }\
         section { display: block; page: report }\
         </style><p>One</p><section>Two</section><p>Three</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[1].width(), 160.0);
    assert_eq!(document.pages[1].height(), 120.0);
    assert_eq!(document.pages[1].lines[0].x(), 20.0);
    assert_line_baseline_at_top(&document, &document.pages[1].lines[0], 100.0);
    assert_eq!(document.pages[2].width(), 100.0);
    assert_eq!(document.pages[2].height(), 100.0);
}

#[tokio::test]
async fn page_auto_margins_center_and_pin_specified_page_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 84pt; width: 144pt; height: 36pt; margin: auto }\
         @page aaa { }\
         @page ddd { margin-top: 0; margin-left: 0 }\
         @page eee { margin-top: 0; margin-right: 0 }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         div { display: block }\
         .aaa { page: aaa } .ddd { page: ddd } .eee { page: eee }\
         </style><div class=\"aaa\">center</div><div class=\"ddd\">top left</div><div class=\"eee\">top right</div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].lines[0].x(), 48.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines[0], 60.0);
    assert_eq!(document.pages[1].lines[0].x(), 0.0);
    assert_line_baseline_at_top(&document, &document.pages[1].lines[0], 84.0);
    assert_eq!(document.pages[2].lines[0].x(), 96.0);
    assert_line_baseline_at_top(&document, &document.pages[2].lines[0], 84.0);
}

#[tokio::test]
async fn page_auto_margins_with_border_and_padding_center_specified_page_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 80pt; width: 40pt; height: 20pt; margin: auto; border: 5pt solid green; padding: 5pt }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Centered</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].x(), 30.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines[0], 50.0);
}

#[tokio::test]
async fn page_auto_margins_are_zero_when_page_area_size_is_auto() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 84pt; margin: 30pt }\
         @page aaa { }\
         @page ddd { margin-top: auto; margin-left: auto }\
         @page eee { margin-top: auto; margin-right: auto }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         div { display: block }\
         .aaa { page: aaa } .ddd { page: ddd } .eee { page: eee }\
         </style><div class=\"aaa\">center</div><div class=\"ddd\">top left</div><div class=\"eee\">top right</div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].lines[0].x(), 30.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines[0], 54.0);
    assert_eq!(document.pages[1].lines[0].x(), 0.0);
    assert_line_baseline_at_top(&document, &document.pages[1].lines[0], 84.0);
    assert_eq!(document.pages[2].lines[0].x(), 30.0);
    assert_line_baseline_at_top(&document, &document.pages[2].lines[0], 84.0);
}

#[tokio::test]
async fn negative_page_margins_expand_page_area_beyond_page_box() {
    let document = Html::from_string(
        "<style>\
         @page { size: 225pt; margin: -15pt }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Expanded</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].x(), -15.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines[0], 240.0);
}

#[tokio::test]
async fn leading_inline_named_page_content_matches_named_first_page_rule() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt; @bottom-center { content: \"base\" } }\
         @page report:first { @bottom-center { content: \"report first\" } }\
         body, p, span { margin: 0; font-size: 10pt; line-height: 10pt }\
         span { page: report }\
         </style><p><span>One</span></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "One")
    );
    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "report first"),
        "{lines:?}"
    );
    assert!(
        !document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "base")
    );
}

#[tokio::test]
async fn right_page_break_inserts_blank_page_and_matches_blank_page_selector() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page :blank { @bottom-center { content: \"blank\" } }\
         body, p, article { margin: 0; font-size: 10pt; line-height: 10pt }\
         article { display: block; break-before: right }\
         </style><p>One</p><article>Two</article>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "One")
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "blank")
    );
    assert!(
        !document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Two")
    );
    assert!(
        document.pages[2]
            .lines
            .iter()
            .any(|line| line.text == "Two")
    );
}

#[tokio::test]
async fn left_page_break_uses_next_left_page_without_blank() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page :blank { @bottom-center { content: \"blank\" } }\
         body, p, article { margin: 0; font-size: 10pt; line-height: 10pt }\
         article { display: block; break-before: left }\
         </style><p>One</p><article>Two</article>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Two")
    );
    assert!(
        !document
            .pages
            .iter()
            .any(|page| page.lines.iter().any(|line| line.text == "blank"))
    );
}

#[tokio::test]
async fn page_margin_boxes_inherit_page_font_properties() {
    let document = Html::from_file_async("weasyprint-samples/invoice/invoice.html")
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let thank = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("Thank you!"))
        .unwrap();
    let contact = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("contact@courtbouillon.org"))
        .unwrap();

    assert_eq!(thank.color, Color::new(30, 228, 148));
    assert!(line_has_font_containing(&document, thank, "pacifico"));
    assert_eq!(contact.color, Color::new(170, 153, 170));
    assert!((contact.x() + rendered_line_advance(contact) - 510.2).abs() < 1.0);
}

#[tokio::test]
async fn page_margin_boxes_default_to_document_font_size() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 120pt; margin: 20pt; font-family: serif; \
           @bottom-left { content: \"Footer\" } \
           @bottom-right { content: \"Small\"; font-size: 9pt } }\
         html { font-size: 11pt; line-height: 1.6 }\
         body { margin: 0 }\
         </style><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let footer = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Footer")
        .unwrap();
    let small = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Small")
        .unwrap();

    assert!((footer.font_size - 11.0).abs() < 0.01);
    assert!((small.font_size - 9.0).abs() < 0.01);
}

fn line_has_font_containing(
    document: &quire::Document,
    line: &quire::RenderedLine,
    needle: &str,
) -> bool {
    line.runs.iter().any(|run| {
        run.font_id
            .and_then(|font_id| document.fonts.get(font_id))
            .is_some_and(|font| font_label(font).contains(needle))
    })
}

#[tokio::test]
async fn supports_named_page_size_orientation() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string("@page { size: letter landscape }"))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].width(), 792.0);
    assert_eq!(document.pages[0].height(), 612.0);
}

#[tokio::test]
async fn supports_standard_named_page_sizes() {
    let document = Html::from_string(
        "<style>\
         @page { size: ledger }\
         @page report { size: jis-b5 landscape }\
         body, p, section { margin: 0; font-size: 10pt; line-height: 10pt }\
         section { display: block; page: report }\
         </style><p>Ledger</p><section>JIS B5</section>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].width(), 792.0);
    assert_eq!(document.pages[0].height(), 1224.0);
    assert!((document.pages[1].width() - 257.0 * 72.0 / 25.4).abs() < 0.001);
    assert!((document.pages[1].height() - 182.0 * 72.0 / 25.4).abs() < 0.001);
}

#[tokio::test]
async fn supports_bare_page_orientation() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string("@page { size: landscape }"))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert!((document.pages[0].width() - 841.8898).abs() < 0.001);
    assert!((document.pages[0].height() - 595.2756).abs() < 0.001);
}

#[tokio::test]
async fn invalid_page_size_descriptor_does_not_partially_apply_lengths() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string("@page { size: 5in landscape }"))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert!((document.pages[0].width() - quire::PageSize::A4_POINTS.width()).abs() < 0.001);
    assert!((document.pages[0].height() - quire::PageSize::A4_POINTS.height()).abs() < 0.001);
}

#[tokio::test]
async fn page_orientation_descriptor_emits_pdf_rotation() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string("@page { page-orientation: rotate-left }"))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].rotation, 270);
    let pdf = document.write_pdf_bytes().unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("/Rotate 270"));
}

#[tokio::test]
async fn page_background_paints_margins_while_canvas_background_paints_page_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 400px; margin: 50px; background: blue }\
         @page :first { background: white }\
         body { margin: 0; background: yellow }\
         </style>\
         Yellow background, white page margins.\
         <div style=\"break-before: page\">Yellow background, blue page margins.</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    let white = Color::WHITE;
    let blue = Color::new(0, 0, 255);
    let yellow = Color::new(255, 255, 0);
    for (page, margin_color) in [(&document.pages[0], white), (&document.pages[1], blue)] {
        assert!(page.rects.iter().any(|rect| {
            rect.x() == 0.0
                && rect.y() == 0.0
                && (rect.width() - 300.0).abs() < 0.01
                && (rect.height() - 300.0).abs() < 0.01
                && rect.fill == Some(margin_color)
        }));
        assert!(page.rects.iter().any(|rect| {
            (rect.x() - 37.5).abs() < 0.01
                && (rect.y() - 37.5).abs() < 0.01
                && (rect.width() - 225.0).abs() < 0.01
                && (rect.height() - 225.0).abs() < 0.01
                && rect.fill == Some(yellow)
        }));
    }
}

#[tokio::test]
async fn page_margin_changes_do_not_create_empty_fragments_for_abspos_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 400px 200px; margin: 0 }\
         @page :first { margin-left: 50px }\
         @page :left { margin: 50px }\
         body { margin: 0 }\
         .container { width: 300px; background: gray }\
         .container > div { box-sizing: border-box; border: solid; width: 250px }\
         .left { height: 100px; background: hotpink }\
         .left::before { content: \"Margins on every side.\" }\
         .right { height: 200px; background: cyan }\
         .right::before { content: \"No page margins.\" }\
         .first { height: 200px; background: yellow }\
         </style>\
         <div class=\"container\">\
           <div class=\"first\">\
             Every page should have a colored box as tall as the page area (gray area).<br>\
             This particular page should have a left-margin.<br>\
             There should be 7 pages.\
           </div>\
           <div class=\"left\"></div>\
           <div class=\"right\"></div>\
           <div class=\"left\"></div>\
         </div>\
         <div class=\"container\" style=\"position:absolute\">\
           <div class=\"right\"></div>\
           <div class=\"left\"></div>\
           <div class=\"right\"></div>\
         </div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-page/page-margin-007-print.html.
    assert_eq!(document.pages.len(), 7);
    for page in &document.pages {
        assert!(
            !page.lines.is_empty(),
            "page-margin-007 should not generate empty intermediate pages"
        );
    }
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "There should be 7 pages.")
    );
    for page_index in [1, 3, 5] {
        assert!(
            document.pages[page_index]
                .lines
                .iter()
                .any(|line| line.text == "Margins on every side."),
            "expected left-page content on page {}",
            page_index + 1
        );
    }
    for page_index in [2, 4, 6] {
        let no_margin_lines = document.pages[page_index]
            .lines
            .iter()
            .filter(|line| line.text == "No page margins.")
            .count();
        assert_eq!(
            no_margin_lines,
            1,
            "expected exactly one right-page fragment on page {}",
            page_index + 1
        );
        assert!(
            !document.pages[page_index]
                .lines
                .iter()
                .any(|line| line.text == "Margins on every side."),
            "right page {} should not duplicate a left-page positioned fragment",
            page_index + 1
        );
    }
}

#[tokio::test]
async fn page_margin_box_dimensions_match_wpt_edge_references() {
    let cases = [
        (
            "<style>\
             @page { margin: 100px; size: 500px 400px;\
               @top-left { border: solid; text-align: left; vertical-align: top; width: 20%; height: 20%; content: \"20%\" }\
               @right-middle { border: solid; text-align: left; vertical-align: top; width: 70%; height: 70%; content: \"70%\" }\
               @bottom-right { border: solid; text-align: left; vertical-align: top; content: \"auto\" }\
               @left-bottom { border: solid; text-align: left; vertical-align: top; content: \"auto\" } }\
             </style>",
            "<style>@page { margin: 0; size: 500px 400px } body { margin: 0 }</style>\
             <div style=\"display:flex; margin:0 100px; height:100px; align-items:flex-end\">\
               <div style=\"border:solid; width:20%; height:20%\">20%</div>\
             </div>\
             <div style=\"display:flex; height:200px\">\
               <div style=\"display:flex; width:100px\"><div style=\"flex:1; border:solid\">auto</div></div>\
               <div style=\"flex:1\"></div>\
               <div style=\"display:flex; width:100px\"><div style=\"width:70%; height:70%; margin:auto 0; border:solid\">70%</div></div>\
             </div>\
             <div style=\"margin:0 100px; height:94px; border:solid\">auto</div>",
        ),
        (
            "<style>\
             @page { margin: 100px; size: 500px 400px;\
               @top-left { border: solid; text-align: left; vertical-align: top; width: 20%; height: 20%; content: \"20%\" }\
               @top-right { border: solid; text-align: left; vertical-align: top; content: \"auto\" }\
               @right-top { border: solid; text-align: left; vertical-align: top; content: \"auto\" }\
               @right-bottom { border: solid; text-align: left; vertical-align: top; width: 70%; height: 70%; content: \"70%\" }\
               @bottom-right { border: solid; text-align: left; vertical-align: top; width: 70px; height: 70px; content: \"70px\" }\
               @bottom-left { border: solid; text-align: left; vertical-align: top; content: \"auto\" }\
               @left-bottom { border: solid; text-align: left; vertical-align: top; content: \"auto\" }\
               @left-top { border: solid; text-align: left; vertical-align: top; width: 70px; height: 70px; content: \"70px\" } }\
             </style>",
            "<style>@page { margin: 0; size: 500px 400px } body { margin: 0 }</style>\
             <div style=\"display:flex; margin:0 100px; height:100px\">\
               <div style=\"border:solid; width:20%; height:20%; align-self:flex-end\">20%</div>\
               <div style=\"border:solid; flex:1\">auto</div>\
             </div>\
             <div style=\"display:flex; height:200px\">\
               <div style=\"display:flex; flex-flow:column; width:100px\">\
                 <div style=\"width:70px; height:70px; border:solid; margin-left:auto\">70px</div>\
                 <div style=\"flex:1; border:solid\">auto</div>\
               </div>\
               <div style=\"flex:1\"></div>\
               <div style=\"display:flex; flex-flow:column; width:100px\">\
                 <div style=\"flex:1; border:solid\">auto</div>\
                 <div style=\"width:70%; height:70%; border:solid\">70%</div>\
               </div>\
             </div>\
             <div style=\"display:flex; margin:0 100px; height:100px\">\
               <div style=\"border:solid; flex:1\">auto</div>\
               <div style=\"border:solid; width:70px; height:70px\">70px</div>\
             </div>",
        ),
    ];

    for (target_html, reference_html) in cases {
        let target = Html::from_string(target_html)
            .render_async(&RenderOptions::default())
            .await
            .unwrap();
        let reference = Html::from_string(reference_html)
            .render_async(&RenderOptions::default())
            .await
            .unwrap();

        // WPT: css/css-page/margin-boxes/dimensions-001-print.html and
        // dimensions-002-print.html. Page-margin boxes paint in page-margin
        // tree order, while the references use normal DOM/flex order; compare
        // the resulting visible geometry as unordered rounded primitives.
        assert_eq!(
            rounded_page_rects(&target.pages[0]),
            rounded_page_rects(&reference.pages[0])
        );
        assert_eq!(
            rounded_page_lines(&target.pages[0]),
            rounded_page_lines(&reference.pages[0])
        );
    }
}

#[tokio::test]
async fn page_margin_box_auto_widths_include_margin_border_and_padding_flex() {
    let document = Html::from_string(
        "<style>\
         @page { size: 384pt 336pt; margin: 72pt; font: 12pt/12pt monospace;\
           @top-left { content: \"x\"; background: black; width: 120pt; height: 12pt; margin-right: 12pt }\
           @top-right { content: \"x\"; background: yellow; width: 72pt; height: 72pt; margin-left: 12pt }\
         }\
         body { margin: 0 }\
         </style>x",
    )
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let page = &document.pages[0];
    assert_eq!(page.width(), 384.0);
    assert_eq!(page.height(), 336.0);
    assert!(
        page.rects.iter().any(|rect| {
            (rect.x() - 72.0).abs() < 0.01
                && (rect.y() - 264.0).abs() < 0.01
                && (rect.width() - 120.0).abs() < 0.01
                && (rect.height() - 12.0).abs() < 0.01
                && rect.fill == Some(Color::BLACK)
        }),
        "top-left border box should exclude its trailing margin from background paint"
    );
    assert!(
        page.rects.iter().any(|rect| {
            (rect.x() - 240.0).abs() < 0.01
                && (rect.y() - 264.0).abs() < 0.01
                && (rect.width() - 72.0).abs() < 0.01
                && (rect.height() - 72.0).abs() < 0.01
                && rect.fill == Some(Color::new(255, 255, 0))
        }),
        "top-right border box should exclude its leading margin from background paint"
    );
    assert!(page.lines.iter().any(|line| {
        line.text == "x" && line.x() >= 72.0 && line.x() < 100.0 && line.y() >= 260.0
    }));
    assert!(page.lines.iter().any(|line| {
        line.text == "x" && line.x() >= 300.0 && line.x() < 312.0 && line.y() >= 290.0
    }));
}

#[tokio::test]
async fn page_margin_auto_widths_use_css_text_min_content_opportunities() {
    let document = Html::from_string("<p>Body</p>")
        .with_stylesheet(Css::from_string(
            "@page { size: 100pt 80pt; margin: 10pt; \
             @top-left { content: \"aaaa aaaa aaaa aaaa\"; font: 10pt/10pt monospace; background: rgb(10, 20, 30); height: 10pt }\
             @top-right { content: \"bbbb\"; font: 10pt/10pt monospace; background: rgb(30, 20, 10); height: 10pt } }\
             body, p { margin: 0 }",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let left = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(10, 20, 30)))
        .expect("expected top-left margin box background");
    let right = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(30, 20, 10)))
        .expect("expected top-right margin box background");

    assert!(
        right.width() >= 20.0,
        "right box should keep its unbreakable min-content width; left={left:?} right={right:?}"
    );
    assert!(
        left.width() > right.width(),
        "breakable left text should receive the remaining auto width; left={left:?} right={right:?}"
    );
}

#[tokio::test]
async fn page_margin_box_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string("<p>Body</p>")
        .with_stylesheet(Css::from_string(
            "@page { size: 240pt 120pt; margin: 20pt; font: 10pt/10pt monospace; \
             @top-left { content: \"aa bb\"; width: min-content; background: green; height: 10pt }\
             @top-center { content: \"aa bb\"; width: fit-content(18pt); background: blue; height: 10pt }\
             @top-right { content: \"aa bb\"; width: max-content; background: black; height: 10pt } }\
             body, p { margin: 0 }",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let width_for = |color| {
        document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("expected page-margin rect with color {color:?}"))
            .width()
    };
    let min = width_for(Color::new(0, 128, 0));
    let fit = width_for(Color::new(0, 0, 255));
    let max = width_for(Color::new(0, 0, 0));

    assert!(
        min < fit && fit < max,
        "page-margin intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}

#[tokio::test]
async fn page_margin_box_generated_text_wraps_inside_content_width() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 120pt; margin: 40pt 0 0 0; font: 12pt/12pt monospace;\
           @top-left { content: \"alpha beta gamma\"; width: 48pt; height: 36pt; text-align: left; vertical-align: top }\
         }\
         </style>x",
    )
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let page_margin_lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        page_margin_lines.contains(&"alpha"),
        "first word should stay on the first generated line: {page_margin_lines:?}"
    );
    assert!(
        page_margin_lines.contains(&"beta") || page_margin_lines.contains(&"gamma"),
        "text after the soft wrap opportunity should flow to a second generated line: {page_margin_lines:?}"
    );
    assert!(
        !page_margin_lines.contains(&"alpha beta gamma"),
        "page-margin generated text should be line-broken inside the resolved content width"
    );
}

#[tokio::test]
async fn page_margin_generated_tabs_use_computed_tab_size() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 120pt; margin: 24pt 0; font: 10pt monospace; line-height: 12pt;\
           @top-left { content: \"A\tB\"; white-space: pre; tab-size: 2; width: 100pt; text-align: left }\
           @bottom-left { content: \"A\tB\"; white-space: pre; tab-size: 4; width: 100pt; text-align: left }\
         }\
         body { margin: 0 }\
         </style>x",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut margin_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text == "A\tB")
        .collect::<Vec<_>>();
    assert_eq!(margin_lines.len(), 2, "{:?}", document.pages[0].lines);
    margin_lines.sort_by(|a, b| b.y().total_cmp(&a.y()));
    let top_advance = rendered_line_advance(margin_lines[0]);
    let bottom_advance = rendered_line_advance(margin_lines[1]);
    assert!(
        top_advance < bottom_advance,
        "expected page-margin tab-size:2 advance to be smaller than tab-size:4, got {top_advance} and {bottom_advance}"
    );
}

#[tokio::test]
async fn page_margin_generated_text_uses_sequence_for_forced_breaks_and_zwsp() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 120pt; margin: 30pt 0 10pt 0; font: 10pt/10pt monospace;\
           @top-left { content: \"A\\A\\A B\\200B C\"; white-space: pre-line; width: 80pt; height: 50pt; text-align: left; vertical-align: top }\
         }\
         body { margin: 0 }\
         </style>x",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let margin_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| matches!(line.text.as_str(), "A" | "BC"))
        .collect::<Vec<_>>();
    let texts = margin_lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(texts.contains(&"A"), "{texts:?}");
    assert!(texts.contains(&"BC"), "{texts:?}");
    assert!(
        texts
            .iter()
            .all(|text| !text.chars().any(|character| character == '\u{200b}')),
        "{texts:?}"
    );
    let a = margin_lines
        .iter()
        .find(|line| line.text == "A")
        .expect("expected first margin line");
    let b = margin_lines
        .iter()
        .find(|line| line.text == "BC")
        .expect("expected line after forced empty line");
    assert!(
        a.y() > b.y() + 15.0,
        "forced empty page-margin line should affect line positions: {margin_lines:?}"
    );
}

fn rounded_page_rects(page: &quire::Page) -> Vec<(i32, i32, i32, i32)> {
    let mut rects = page
        .rects
        .iter()
        .map(|rect| {
            (
                rounded_hundredths(rect.x()),
                rounded_hundredths(rect.y()),
                rounded_hundredths(rect.width()),
                rounded_hundredths(rect.height()),
            )
        })
        .collect::<Vec<_>>();
    rects.sort();
    rects
}

fn rounded_page_lines(page: &quire::Page) -> Vec<(String, i32, i32)> {
    let mut lines = page
        .lines
        .iter()
        .filter(|line| !line.text.is_empty())
        .map(|line| {
            (
                line.text.clone(),
                rounded_hundredths(line.x()),
                rounded_hundredths(line.y()),
            )
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

fn rounded_hundredths(value: f32) -> i32 {
    (value * 100.0).round() as i32
}
