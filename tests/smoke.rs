use base64::Engine as _;
use quire::{BookmarkState, Color, Css, Html, RenderOptions};

fn line_font<'a>(
    document: &'a quire::Document,
    line: &quire::RenderedLine,
) -> &'a quire::DocumentFont {
    let font_id = line
        .font_id
        .expect("rendered line should have a resolved font");
    &document.fonts[font_id]
}

fn font_label(font: &quire::DocumentFont) -> String {
    format!("{} {}", font.family, font.post_script_name).to_ascii_lowercase()
}

fn font_label_contains_any(font: &quire::DocumentFont, needles: &[&str]) -> bool {
    let label = font_label(font);
    needles.iter().any(|needle| label.contains(needle))
}

fn line_font_contains_any(
    document: &quire::Document,
    line: &quire::RenderedLine,
    needles: &[&str],
) -> bool {
    font_label_contains_any(line_font(document, line), needles)
}

fn line_font_is_italic(document: &quire::Document, line: &quire::RenderedLine) -> bool {
    let font = line_font(document, line);
    font.italic_angle != 0 || font_label_contains_any(font, &["italic", "oblique"])
}

fn line_font_is_bold(document: &quire::Document, line: &quire::RenderedLine) -> bool {
    font_label_contains_any(line_font(document, line), &["bold", "black", "heavy"])
}

fn line_run_font_is_italic(
    document: &quire::Document,
    line: &quire::RenderedLine,
    text: &str,
) -> bool {
    line.runs.iter().any(|run| {
        run.text == text
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
    line: &quire::RenderedLine,
    text: &str,
) -> bool {
    line.runs.iter().any(|run| {
        run.text == text
            && run
                .font_id
                .and_then(|font_id| document.fonts.get(font_id))
                .is_some_and(|font| font_label_contains_any(font, &["bold", "black", "heavy"]))
    })
}

fn line_font_is_monospace(document: &quire::Document, line: &quire::RenderedLine) -> bool {
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
            rect.fill == Some(Color::BLACK) && rect.height() <= 1.01 && rect.width() > 1.01
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
            rect.fill == Some(Color::BLACK) && rect.width() <= 1.01 && rect.height() > 1.01
        })
        .map(|rect| rect.height())
        .collect()
}

fn first_rect_paint_operation_index(page: &quire::Page, color: Color) -> usize {
    page.paint_operations()
        .iter()
        .position(|operation| {
            matches!(
                operation,
                quire::PaintOperation::Rect(index)
                    if page.rects().get(*index).is_some_and(|rect| rect.fill == Some(color))
            )
        })
        .expect("rect with expected fill should be present in paint operations")
}

fn final_rect_fill_at(page: &quire::Page, x: f32, y: f32) -> Option<Color> {
    page.paint_operations()
        .iter()
        .filter_map(|operation| {
            let quire::PaintOperation::Rect(index) = operation else {
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

fn rendered_line_advance(line: &quire::RenderedLine) -> f32 {
    line.runs
        .iter()
        .flat_map(|run| run.glyphs.iter().flatten())
        .map(|glyph| glyph.x_advance)
        .sum()
}

fn rendered_line_baseline_y_for_top(
    document: &quire::Document,
    line: &quire::RenderedLine,
    top_y: f32,
) -> f32 {
    let adjustment = line
        .font_id
        .and_then(|font_id| document.fonts.get(font_id))
        .map(|font| {
            let ascent = font.ascender as f32 * line.font_size / font.units_per_em as f32;
            line.font_size - ascent
        })
        .unwrap_or(0.0);
    top_y - line.font_size + adjustment
}

fn rendered_line_baseline_top(document: &quire::Document, line: &quire::RenderedLine) -> f32 {
    let adjustment = line
        .font_id
        .and_then(|font_id| document.fonts.get(font_id))
        .map(|font| {
            let ascent = font.ascender as f32 * line.font_size / font.units_per_em as f32;
            line.font_size - ascent
        })
        .unwrap_or(0.0);
    line.y() + line.font_size - adjustment
}

fn assert_line_baseline_at_top(document: &quire::Document, line: &quire::RenderedLine, top_y: f32) {
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
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = first_rect_paint_operation_index(page, Color::new(255, 0, 0));
    let yellow = first_rect_paint_operation_index(page, Color::new(255, 255, 0));
    let green = first_rect_paint_operation_index(page, Color::new(0, 128, 0));
    let blue = first_rect_paint_operation_index(page, Color::new(0, 0, 255));
    let magenta = first_rect_paint_operation_index(page, Color::new(255, 0, 255));

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
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("blue foreground rect should render");
    assert_eq!(
        final_rect_fill_at(
            page,
            blue_rect.x() + blue_rect.width() - 1.0,
            blue_rect.y() + blue_rect.height() / 2.0,
        ),
        Some(Color::new(0, 0, 255))
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
#[path = "smoke/layout_inline_flex.rs"]
mod layout_inline_flex;
#[path = "smoke/media_links_style.rs"]
mod media_links_style;
#[path = "smoke/positioning_fragmentation.rs"]
mod positioning_fragmentation;
#[path = "smoke/selectors_flex_columns.rs"]
mod selectors_flex_columns;
#[path = "smoke/tables.rs"]
mod tables;
#[path = "smoke/text.rs"]
mod text;
