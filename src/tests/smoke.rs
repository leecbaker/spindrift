use crate as quire;
use crate::{BookmarkState, Css, CssColor, Html, RenderOptions};
use base64::Engine as _;

const ROW_SUBGRID_AUTO_FILL_WPT: &str = include_str!(
    "../../tests/fixtures/wpt/css/css-grid/grid-lanes/subgrid/grid-subgridded-to-grid-lanes/track-sizing/row-subgrid-auto-fill-002.html"
);
const ROW_SUBGRID_AUTO_FILL_WPT_REFERENCE: &str = include_str!(
    "../../tests/fixtures/wpt/css/css-grid/grid-lanes/subgrid/grid-subgridded-to-grid-lanes/track-sizing/row-subgrid-auto-fill-002-ref.html"
);
const CUSTOM_HIGHLIGHT_PAINTING_IFRAME_REFERENCE: &str = include_str!(
    "../../tests/fixtures/wpt/css/css-highlight-api/painting/custom-highlight-painting-iframe-001-ref.html"
);

/// Local Grid Lanes regression derived from upstream
/// `row-subgrid-auto-fill-002`. Keeping both inputs in the repository makes
/// the reference comparison independent of a developer's WPT checkout.
#[tokio::test]
async fn grid_lanes_row_subgrid_auto_fill_matches_local_reference() {
    let actual = Html::from_string(ROW_SUBGRID_AUTO_FILL_WPT)
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let reference = Html::from_string(ROW_SUBGRID_AUTO_FILL_WPT_REFERENCE)
        .render(&RenderOptions::default())
        .await
        .unwrap();
    assert_eq!(actual.pages.len(), reference.pages.len());
    assert_eq!(actual.pages[0].rects(), reference.pages[0].rects());
    assert_eq!(
        actual.pages[0].rounded_rects(),
        reference.pages[0].rounded_rects()
    );
}

/// WPT reference: css/css-highlight-api/painting/
/// custom-highlight-painting-iframe-001-ref.html.
///
/// The reference itself uses `srcdoc` rather than a fetched `src`, so verify
/// the nested document's paint fragment is composed into the iframe viewport.
#[tokio::test]
async fn iframe_srcdoc_paints_embedded_document() {
    let document = Html::from_string(CUSTOM_HIGHLIGHT_PAINTING_IFRAME_REFERENCE)
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let cyan = CssColor::new(0, 255, 255);
    let blue = CssColor::new(0, 0, 255);

    assert!(
        page.rects().iter().any(|rect| rect.fill == Some(cyan)),
        "srcdoc background was not painted: {:?}",
        page.rects()
    );
    assert!(
        page.lines()
            .iter()
            .any(|line| line.text == "abc" && line.color == blue),
        "srcdoc text was not painted in blue: {:?}",
        page.lines()
    );
}

/// HTML's legacy body margins are resolved inside the child document's
/// cascade, including the immediate iframe's fallback attributes. Compare the
/// two embedding forms to their equivalent child CSS rather than asserting
/// implementation-specific paint coordinates.
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-page>
#[tokio::test]
async fn iframe_legacy_body_margins_match_equivalent_child_css() {
    for (name, source_attribute) in [
        (
            "srcdoc",
            "srcdoc=\"<!doctype html><body>frame margins</body>\"".to_string(),
        ),
        (
            "data URL",
            "src=\"data:text/html,<!doctype html><body>frame margins</body>\"".to_string(),
        ),
    ] {
        let actual = Html::from_string(format!(
            "<style>@page{{size:420px 240px;margin:0}}body{{margin:0}}</style>\
             <iframe width=300 height=160 marginwidth=100 marginheight=60 {source_attribute}></iframe>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();
        let reference = Html::from_string(
            "<style>@page{size:420px 240px;margin:0}body{margin:0}</style>\
             <iframe width=300 height=160 srcdoc=\"<!doctype html><body style='margin-left:100px;margin-right:100px;margin-top:60px;margin-bottom:60px'>frame margins</body>\"></iframe>"
        .to_string())
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let actual_line = actual.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == "frame")
            .expect("iframe child text should paint");
        let reference_line = reference.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == "frame")
            .expect("reference iframe child text should paint");
        assert_eq!(
            (actual_line.x(), actual_line.y()),
            (reference_line.x(), reference_line.y()),
            "{name} iframe margins differed from equivalent child CSS"
        );
    }
}

fn line_font<'a>(
    document: &'a quire::Document,
    line: &crate::document::paint::text::RenderedLine,
) -> &'a crate::document::DocumentFont {
    let font_id = line
        .font_id
        .expect("rendered line should have a resolved font");
    &document.fonts[font_id]
}

fn font_label(font: &crate::document::DocumentFont) -> String {
    format!("{} {}", font.family, font.post_script_name).to_ascii_lowercase()
}

fn font_label_contains_any(font: &crate::document::DocumentFont, needles: &[&str]) -> bool {
    let label = font_label(font);
    needles.iter().any(|needle| label.contains(needle))
}

fn line_font_contains_any(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
    needles: &[&str],
) -> bool {
    font_label_contains_any(line_font(document, line), needles)
}

fn line_font_is_italic(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
) -> bool {
    let font = line_font(document, line);
    font.italic_angle != 0 || font_label_contains_any(font, &["italic", "oblique"])
}

fn line_font_is_bold(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
) -> bool {
    font_label_contains_any(line_font(document, line), &["bold", "black", "heavy"])
}

fn line_run_font_is_italic(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
    text: &str,
) -> bool {
    line.runs.iter().any(|run| {
        run.text.as_ref() == text
            && run
                .font_id
                .and_then(|font_id| document.fonts.get(font_id))
                .is_some_and(|font| {
                    font.italic_angle != 0 || font_label_contains_any(font, &["italic", "oblique"])
                })
    })
}

fn line_run_font_is_bold(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
    text: &str,
) -> bool {
    line.runs.iter().any(|run| {
        run.text.as_ref() == text
            && run
                .font_id
                .and_then(|font_id| document.fonts.get(font_id))
                .is_some_and(|font| font_label_contains_any(font, &["bold", "black", "heavy"]))
    })
}

fn line_font_is_monospace(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
) -> bool {
    let font = line_font(document, line);
    let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
        return false;
    };
    let Some(narrow) = face
        .glyph_index('i')
        .and_then(|glyph| face.glyph_hor_advance(glyph))
    else {
        return false;
    };
    let Some(wide) = face
        .glyph_index('W')
        .and_then(|glyph| face.glyph_hor_advance(glyph))
    else {
        return false;
    };
    narrow == wide
}

fn horizontal_table_border_widths(document: &quire::Document) -> Vec<f32> {
    document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::BLACK) && rect.height() <= 1.01 && rect.width() > 1.01
        })
        .step_by(2)
        .map(|rect| rect.width())
        .collect()
}

fn vertical_table_border_heights(document: &quire::Document) -> Vec<f32> {
    document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::BLACK) && rect.width() <= 1.01 && rect.height() > 1.01
        })
        .map(|rect| rect.height())
        .collect()
}

fn first_rect_paint_operation_index(page: &quire::Page, color: CssColor) -> usize {
    page.paint_operations()
        .iter()
        .position(|operation| {
            matches!(
                operation,
                crate::document::paint::page::PaintOperation::Rect(index)
                    if page.rects().get(*index).is_some_and(|rect| rect.fill == Some(color))
            )
        })
        .expect("rect with expected fill should be present in paint operations")
}

fn final_rect_fill_at(page: &quire::Page, x: f32, y: f32) -> Option<CssColor> {
    page.paint_operations()
        .iter()
        .filter_map(|operation| {
            let crate::document::paint::page::PaintOperation::Rect(index) = operation else {
                return None;
            };
            let rect = page.rects().get(*index)?;
            (x >= rect.x()
                && x <= rect.x() + rect.width()
                && y >= rect.y()
                && y <= rect.y() + rect.height())
            .then_some(rect.fill)
            .flatten()
        })
        .next_back()
}

/// Return PDF syntax together with decoded textual streams for assertions.
///
/// PDF streams may use `/FlateDecode` (ISO 32000-1:2008, 7.4.4). Quire emits
/// direct numeric `/Length` entries, so the test harness can decode its own
/// streams without coupling operator assertions to uncompressed output.
fn pdf_searchable_text(pdf: &[u8]) -> String {
    let mut text = String::new();
    let mut search_start = 0;
    let mut copied_until = 0;

    while let Some(stream_marker) = find_pdf_bytes_from(pdf, b"stream\n", search_start) {
        let stream_start = stream_marker + b"stream\n".len();
        let Some((dictionary_start, stream_end)) = pdf_stream_bounds(pdf, stream_start) else {
            search_start = stream_start;
            continue;
        };
        text.push_str(&String::from_utf8_lossy(&pdf[copied_until..stream_start]));

        let dictionary = &pdf[dictionary_start..stream_marker];
        let stream = &pdf[stream_start..stream_end];
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

/// Return whether a tagged sRGB PDF/A fill uses the requested components.
///
/// PDF/A content uses the page's ICCBased sRGB resource (`cs`) rather than an
/// uncalibrated device-RGB `rg` operator (ISO 32000-2:2020, 8.6.8).
fn has_srgb_fill(rendered: &str, components: &str) -> bool {
    rendered.contains(&format!("/CSsRGB cs\n{components} scn"))
}

fn srgb_fill_count(rendered: &str, components: &str) -> usize {
    rendered
        .matches(&format!("/CSsRGB cs\n{components} scn"))
        .count()
}

fn has_srgb_stroke(rendered: &str, components: &str) -> bool {
    rendered.contains(&format!("/CSsRGB CS\n{components} SCN"))
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

fn find_pdf_bytes_from(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    haystack
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| start + position)
}

fn rendered_line_advance(line: &crate::document::paint::text::RenderedLine) -> f32 {
    line.runs
        .iter()
        .flat_map(|run| run.glyphs.as_deref().unwrap_or_default())
        .map(|glyph| glyph.x_advance)
        .sum()
}

fn rendered_line_baseline_y_for_top(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
    top_y: f32,
) -> f32 {
    let adjustment = line
        .font_id
        .and_then(|font_id| document.fonts.get(font_id))
        .map(|font| {
            let ascent =
                font.layout_metrics.ascender as f32 * line.font_size / font.units_per_em as f32;
            line.font_size - ascent
        })
        .unwrap_or(0.0);
    top_y - line.font_size + adjustment
}

fn rendered_line_baseline_top(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
) -> f32 {
    let adjustment = line
        .font_id
        .and_then(|font_id| document.fonts.get(font_id))
        .map(|font| {
            let ascent =
                font.layout_metrics.ascender as f32 * line.font_size / font.units_per_em as f32;
            line.font_size - ascent
        })
        .unwrap_or(0.0);
    line.y() + line.font_size - adjustment
}

fn assert_line_baseline_at_top(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
    top_y: f32,
) {
    let expected = rendered_line_baseline_y_for_top(document, line, top_y);
    assert!(
        (line.y() - expected).abs() < 0.01,
        "expected {:?} baseline y {:.4} for top {:.4}, got {:.4}",
        line.text,
        expected,
        top_y,
        line.y()
    );
}

#[tokio::test]
async fn min_width_fit_content_length_clamps_block_width() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 200px 160px; margin: 0 }\
         body { margin: 0 }\
         #reference-overlapped-red {\
           position: absolute;\
           left: 0;\
           top: 0;\
           background-color: red;\
           width: 100px;\
           height: 100px;\
           z-index: -1;\
         }\
         </style>\
         <div id=\"reference-overlapped-red\"></div>\
         <div style=\"width: 10px; min-width: fit-content(100px); height: 100px; background: green;\">\
           <div style=\"display: inline-block; width: 60px;\"></div>\
           <div style=\"display: inline-block; width: 60px;\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let green_rect = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(green))
        .unwrap_or_else(|| panic!("expected green block background: {:?}", page.rects()));

    assert!(
        (green_rect.width() - 75.0).abs() < 0.01 && (green_rect.height() - 75.0).abs() < 0.01,
        "fit-content min-width should produce a 100px square: {green_rect:?}",
    );
    for (x, y) in [
        (green_rect.x() + 1.0, green_rect.y() + 1.0),
        (
            green_rect.x() + green_rect.width() / 2.0,
            green_rect.y() + green_rect.height() / 2.0,
        ),
        (
            green_rect.x() + green_rect.width() - 1.0,
            green_rect.y() + green_rect.height() / 2.0,
        ),
    ] {
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(green),
            "green should cover the red backing square at ({x}, {y})",
        );
    }
}

#[tokio::test]
async fn min_height_max_content_clamps_zero_block_height() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 160px 160px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"background: green; width: 100px; min-height: max-content; height: 0px;\">\
           <div style=\"height: 100px;\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| panic!("expected green background: {:?}", document.pages[0].rects()));

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "min-height:max-content should grow the zero-height block to its child height: {green:?}",
    );
}

#[tokio::test]
async fn height_max_content_uses_block_child_height() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 160px 160px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"background: green; width: 100px; height: max-content;\">\
           <div style=\"height: 80px;\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| panic!("expected green background: {:?}", document.pages[0].rects()));

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 60.0).abs() < 0.01,
        "height:max-content should use the block child's content height: {green:?}",
    );
}

#[tokio::test]
async fn max_height_max_content_clamps_definite_block_height() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 160px 160px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"background: green; width: 100px; height: 150px; max-height: max-content;\">\
           <div style=\"height: 80px;\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| panic!("expected green background: {:?}", document.pages[0].rects()));

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 60.0).abs() < 0.01,
        "max-height:max-content should clamp the definite height to the child height: {green:?}",
    );
}

#[tokio::test]
async fn padded_bordered_definite_block_prebreaks_as_one_border_box() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 100px 100px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"height: 30px\"></div>\
         <div style=\"height: 50px; padding: 10px; border: 10px solid red; background: green\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[1]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
    );
}

#[tokio::test]
async fn intrinsic_min_height_keeps_cyclic_percentage_child_height_auto() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 220px 220px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"background: green; width: 100px; height: 100px; min-height: max-content;\">\
           <div style=\"height: 50%;\">\
             <div style=\"height: 150px;\"></div>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| panic!("expected green background: {:?}", document.pages[0].rects()));

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 112.5).abs() < 0.01,
        "intrinsic min-height should measure the percentage child as auto: {green:?}",
    );
}

#[tokio::test]
async fn overflow_scroll_preserves_parent_paint_order() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <title>Overflow:scroll paint order</title>\
         <style>\
         @page { size: 200px 200px; margin: 0 }\
         body { margin: 0 }\
         #scroller {\
           background: red;\
           padding: 20px;\
           box-sizing: border-box;\
           width: 100px;\
           height: 100px;\
           overflow: scroll;\
         }\
         #negative-margin {\
           width: 100px;\
           height: 100px;\
           background: green;\
           margin-top: -100px;\
         }\
         #foreground1, #foreground2 {\
           display: inline-block;\
           width: 50px;\
           height: 50px;\
         }\
         #foreground1 { background: blue }\
         #foreground2 { background: magenta }\
         </style>\
         <div id=\"scroller\">\
           <div style=\"height: 200px; background: yellow\">\
             <div id=\"foreground1\"></div>\
           </div>\
         </div>\
         <div id=\"negative-margin\">\
           <div id=\"foreground2\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));
    let yellow = first_rect_paint_operation_index(page, CssColor::new(255, 255, 0));
    let green = first_rect_paint_operation_index(page, CssColor::new(0, 128, 0));
    let blue = first_rect_paint_operation_index(page, CssColor::new(0, 0, 255));
    let magenta = first_rect_paint_operation_index(page, CssColor::new(255, 0, 255));

    assert!(red < yellow, "scroller background should paint first");
    assert!(yellow < green, "later block background should cover yellow");
    assert!(
        green < blue,
        "earlier inline foreground should paint over green"
    );
    assert!(blue < magenta, "later inline foreground should paint last");

    let blue_rect = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("blue foreground rect should render");
    assert_eq!(
        final_rect_fill_at(
            page,
            blue_rect.x() + blue_rect.width() - 1.0,
            blue_rect.y() + blue_rect.height() / 2.0,
        ),
        Some(CssColor::new(0, 0, 255))
    );
}

#[path = "smoke/css_assets.rs"]
mod css_assets;
#[path = "smoke/document.rs"]
mod document;
#[path = "smoke/flex_conformance.rs"]
mod flex_conformance;
#[path = "smoke/fonts.rs"]
mod fonts;
#[path = "smoke/grid_zoom.rs"]
mod grid_zoom;
#[path = "smoke/layout_inline_flex.rs"]
mod layout_inline_flex;
#[path = "smoke/media_links_style.rs"]
mod media_links_style;
#[path = "smoke/multicol_zoom.rs"]
mod multicol_zoom;
#[path = "smoke/positioned_zoom.rs"]
mod positioned_zoom;
#[path = "smoke/positioning_fragmentation.rs"]
mod positioning_fragmentation;
#[path = "smoke/selectors_flex_columns.rs"]
mod selectors_flex_columns;
#[path = "smoke/table_zoom.rs"]
mod table_zoom;
#[path = "smoke/tables.rs"]
mod tables;
#[path = "smoke/text.rs"]
mod text;
