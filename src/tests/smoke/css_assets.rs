use super::*;
use crate::document::paint::geometry::PaintSize;

fn image_xobject_count_with_size(rendered: &str, width: u32, height: u32) -> usize {
    rendered
        .split("/Subtype /Image")
        .skip(1)
        .filter(|object| {
            object.contains(&format!("/Width {width}"))
                && object.contains(&format!("/Height {height}"))
        })
        .count()
}

/// Count PDF image paints independently of whether a uniform decoded raster
/// remains an Image XObject or is emitted as an equivalent calibrated fill.
/// The PDF writer may use the latter representation when it preserves the
/// source's visual output and metadata requirements.
fn image_paint_count(rendered: &str, width: u32, height: u32) -> usize {
    image_xobject_count_with_size(rendered, width, height) + rendered.matches("/CSsRGB cs").count()
}

const GREEN_1X1_PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNg+M8AAAICAQB7CYF4AAAAAElFTkSuQmCC";

const GREEN_50X50_SVG: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI1MCIgaGVpZ2h0PSI1MCIgdmlld0JveD0iMCAwIDUwIDUwIj48cmVjdCB3aWR0aD0iNTAiIGhlaWdodD0iNTAiIGZpbGw9ImdyZWVuIi8+PC9zdmc+";

fn filled_rect(
    page: &quire::Page,
    color: CssColor,
) -> &crate::document::paint::shapes::RenderedRect {
    page.rects()
        .iter()
        .find(|rect| rect.fill == Some(color))
        .unwrap_or_else(|| {
            panic!(
                "expected filled rect with color {color:?} in {:?}",
                page.rects()
            )
        })
}

fn filled_rects(
    page: &quire::Page,
    color: CssColor,
) -> Vec<&crate::document::paint::shapes::RenderedRect> {
    page.rects()
        .iter()
        .filter(|rect| rect.fill == Some(color))
        .collect()
}

fn assert_pdf_clips_image_draw(document: &quire::Document) {
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    let clip_index = rendered
        .find("W\nn")
        .unwrap_or_else(|| panic!("expected PDF clipping operator in {rendered}"));
    let image_index = rendered
        .find(" Do")
        .or_else(|| rendered.find(" re\nf"))
        .unwrap_or_else(|| panic!("expected PDF image or equivalent fill draw in {rendered}"));
    assert!(
        clip_index < image_index,
        "background image should be clipped before drawing"
    );
}

#[tokio::test]
async fn relative_positioned_inline_backgrounds_shift_without_expanding_line_box() {
    let document = Html::from_string(
        "<style>
            @page { size: 260pt 180pt; margin: 0 }
            body { margin: 0 }
            .container {
                margin: 30pt 0;
                color: transparent;
                background: blue;
                line-height: 10pt;
                font-size: 30pt;
            }
            span { background: orange }
            .up10 { position: relative; top: -10pt }
            .down10 { position: relative; top: 10pt }
        </style>
        <div class=\"container\"><span>A</span><span class=\"up10\">B</span><span class=\"down10\">C</span></div>
        <div class=\"container\"><span>D</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let orange = filled_rects(page, CssColor::new(255, 165, 0));
    assert_eq!(orange.len(), 4, "orange rects={orange:?}");

    let mut first_line = orange[..3].to_vec();
    first_line.sort_by(|left, right| left.x().total_cmp(&right.x()));
    let normal = first_line[0];
    let up = first_line[1];
    let down = first_line[2];
    assert!(
        (up.y() - normal.y() - 10.0).abs() < 0.01,
        "top:-10pt should shift inline background up: normal={normal:?}, up={up:?}"
    );
    assert!(
        (down.y() - normal.y() + 10.0).abs() < 0.01,
        "top:10pt should shift inline background down: normal={normal:?}, down={down:?}"
    );

    let blue = filled_rects(page, CssColor::new(0, 0, 255));
    assert_eq!(blue.len(), 2, "blue rects={blue:?}");
    for rect in blue {
        assert!(
            (rect.height() - 10.0).abs() < 0.01,
            "relative inline offsets must not expand the line box: {rect:?}"
        );
    }
}

#[tokio::test]
async fn relative_positioned_inline_horizontal_offset_preserves_flow_advance() {
    let document = Html::from_string(
        "<style>
            @page { size: 260pt 180pt; margin: 0 }
            body { margin: 0 }
            div { margin: 20pt 0; font-size: 20pt; line-height: 20pt; color: transparent }
            .first { background: orange }
            .second { background: green }
            .shift { position: relative; left: 10pt }
        </style>
        <div><span class=\"first\">A</span><span class=\"second\">B</span></div>
        <div><span class=\"first shift\">A</span><span class=\"second\">B</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let orange = filled_rects(page, CssColor::new(255, 165, 0));
    let green = filled_rects(page, CssColor::new(0, 128, 0));
    assert_eq!(orange.len(), 2, "orange rects={orange:?}");
    assert_eq!(green.len(), 2, "green rects={green:?}");

    assert!(
        (orange[1].x() - orange[0].x() - 10.0).abs() < 0.01,
        "left:10pt should shift the first inline background right: orange={orange:?}"
    );
    assert!(
        (green[1].x() - green[0].x()).abs() < 0.01,
        "following inline should keep normal-flow advance: green={green:?}"
    );
}

#[tokio::test]
async fn applies_inline_text_color() {
    let document = Html::from_string("<p style=\"color: red\">Hello</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn inline_content_background_height_is_independent_of_line_height() {
    let document = Html::from_string(
        "<style>
            @page { size: 420pt 260pt; margin: 20pt }
            body { margin: 0 }
            div { font-size: 50pt; display: inline-block; color: transparent }
            span { background: rgb(0 0 255) }
            div { line-height: 200pt }
            div:nth-of-type(2) { line-height: 30pt }
            div:nth-of-type(3) { line-height: normal }
        </style>
        <div><span>aa</span></div><div><span>aa</span></div><div><span>aa</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();
    assert_eq!(blue.len(), 3, "blue rects={blue:?}");

    let first = blue[0];
    for rect in &blue[1..] {
        assert!(
            (rect.y() - first.y()).abs() < 0.01,
            "inline backgrounds should share one content-area bottom: {blue:?}"
        );
        assert!(
            (rect.height() - first.height()).abs() < 0.01,
            "inline background height should ignore line-height: {blue:?}"
        );
    }
}

#[tokio::test]
async fn normal_line_height_uses_primary_metrics_despite_fallback_runs() {
    let dir =
        std::env::temp_dir().join(format!("quire-fallback-line-height-{}", std::process::id()));
    let fonts_dir = dir.join("fonts");
    std::fs::create_dir_all(&fonts_dir).unwrap();
    std::fs::copy(
        "weasyprint-samples/invoice/SourceSans3-Regular.ttf",
        fonts_dir.join("high.ttf"),
    )
    .unwrap();
    std::fs::copy(
        "weasyprint-samples/invoice/pacifico.ttf",
        fonts_dir.join("deep.ttf"),
    )
    .unwrap();
    let html_path = dir.join("line-height.html");
    std::fs::write(
        &html_path,
        r#"<!DOCTYPE html>
        <meta charset="utf-8">
        <style>
        @page { size: 900px 360px; margin: 0 }
        body { margin: 0 }
        @font-face {
          font-family: HighOnly;
          src: url(/fonts/high.ttf);
          unicode-range: U+0020, U+0061;
        }
        @font-face {
          font-family: DeepOnly;
          src: url(/fonts/deep.ttf);
          unicode-range: U+0020, U+0062;
        }
        div {
          position: absolute;
          line-height: normal;
          font-size: 100px;
          font-family: HighOnly, DeepOnly;
          color: transparent;
        }
        .h { font-family: HighOnly; }
        .hd { font-family: HighOnly, DeepOnly; }
        .white { background: white; }
        .red { background: red; }
        </style>
        <p>Test passes if there is no red below.</p>
        <div class="hd red">ab</div>
        <div class="h white">aa</div>"#,
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .with_base_path(&dir)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    let mut red = filled_rects(&document.pages[0], CssColor::new(255, 0, 0))
        .into_iter()
        .filter(|rect| rect.width() > 20.0 && rect.height() > 20.0)
        .collect::<Vec<_>>();
    let mut white = filled_rects(&document.pages[0], CssColor::new(255, 255, 255))
        .into_iter()
        .filter(|rect| rect.width() > 20.0 && rect.height() > 20.0)
        .collect::<Vec<_>>();
    red.sort_by(|left, right| left.x().total_cmp(&right.x()));
    white.sort_by(|left, right| left.x().total_cmp(&right.x()));
    assert_eq!(red.len(), 1, "red backgrounds={red:?}");
    assert_eq!(white.len(), 1, "white backgrounds={white:?}");
    assert!(
        (red[0].y() - white[0].y()).abs() < 0.01,
        "fallback glyph metrics must not shift the normal-line background: red={red:?} white={white:?}"
    );
    assert!(
        (red[0].height() - white[0].height()).abs() < 0.01,
        "fallback glyph metrics must not change the normal-line background height: red={red:?} white={white:?}"
    );
}

#[tokio::test]
async fn inline_empty_start_edge_border_paints_without_text() {
    let document = Html::from_string(
        "<style>
            @page { size: 180pt 120pt; margin: 20pt }
            body { margin: 0 }
            div { font-size: 50pt; display: inline-block }
            span {
                padding-left: 1em;
                color: black;
                border-top: solid 1pt;
                border-bottom: solid 1pt;
            }
        </style>
        <div><span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let borders = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::BLACK)
                && (rect.height() - 1.0).abs() < 0.01
                && (rect.width() - 50.0).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(borders.len(), 2, "empty inline border rects={borders:?}");
}

#[tokio::test]
async fn draws_backgrounds_and_borders() {
    let html = Html::from_string(
        "<div style=\"margin: 0; padding: 2pt; border: 1pt solid blue; background: #ff0000\">Box</div>",
    );
    let document = html.render(&RenderOptions::default()).await.unwrap();
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
    );
    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        4
    );

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(has_srgb_fill(&rendered, "1 0 0"));
    assert!(has_srgb_fill(&rendered, "0 0 1"));
}

#[tokio::test]
async fn hwb_border_color_paints_vector_border() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border:2pt solid hwb(240 20% 0% / 75%)\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let border_color = CssColor::rgba(51, 51, 255, 0.75);
    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(border_color))
            .count(),
        4
    );
}

#[tokio::test]
async fn srgb_color_function_border_color_paints_vector_border() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border:2pt solid color(srgb 0.2 0.2 1 / 75%)\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let border_color = CssColor::srgb(0.2, 0.2, 1.0, 0.75);
    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(border_color))
            .count(),
        4
    );
}

#[tokio::test]
async fn logical_inline_start_border_paints_left_side_in_initial_writing_mode() {
    let document = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"width:20pt;height:10pt;border-inline-start:2pt solid red\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let border = document.pages[0]
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0))
                && (rect.width() - 2.0).abs() < 0.01
                && rect.height() > 9.0
        })
        .unwrap();
    assert!((border.x() - 10.0).abs() < 0.01);
}

#[tokio::test]
async fn logical_inline_start_border_paints_right_side_in_rtl_direction() {
    let document = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"direction:rtl;width:20pt;height:10pt;border-inline-start:2pt solid red\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let border = document.pages[0]
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0))
                && (rect.width() - 2.0).abs() < 0.01
                && rect.height() > 9.0
        })
        .unwrap();
    assert!((border.x() - 30.0).abs() < 0.01);
}

#[tokio::test]
async fn logical_border_corner_radius_paints_initial_top_left_corner() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;background:black;border-start-start-radius:4pt;color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rounded = document.pages[0]
        .rounded_rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .unwrap();

    assert_eq!(rounded.radii.top_left.x(), 4.0);
    assert_eq!(rounded.radii.top_left.y(), 4.0);
    assert_eq!(rounded.radii.top_right.x(), 0.0);
    assert_eq!(rounded.radii.bottom_left.y(), 0.0);
}

#[tokio::test]
async fn border_radius_paints_background_as_rounded_rect() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;background:black;border-radius:4pt;color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rounded = document.pages[0]
        .rounded_rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .unwrap();

    assert_eq!(rounded.radii.top_left.x(), 4.0);
    assert_eq!(rounded.radii.top_left.y(), 4.0);
    assert_eq!(rounded.radii.top_right.x(), 4.0);
    assert_eq!(rounded.radii.bottom_right.y(), 4.0);
}

#[tokio::test]
async fn corner_shape_bevels_background_fill_clip() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 120pt; margin: 0 }\
         body { margin: 0 }\
         .target { background: green; width: 100px; height: 100px;\
           border-radius: 50px; border: 0px solid black; corner-shape: bevel }\
         </style><div class=target></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = CssColor::new(0, 128, 0);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(green)),
        "rounded background fill must not paint as a rectangle"
    );

    let path = document.pages[0]
        .paths()
        .iter()
        .find(|path| path.fill == Some(green))
        .unwrap_or_else(|| {
            panic!(
                "expected green rounded background path, got {:?}",
                document.pages[0].paths()
            )
        });
    let curve_count = path
        .commands
        .iter()
        .filter(|command| {
            matches!(
                command,
                crate::document::paint::paths::RenderedPathCommand::CurveTo { .. }
            )
        })
        .count();

    assert_eq!(
        curve_count, 0,
        "background clipping should follow the CSS Borders 4 bevel contour: {:?}",
        path.commands
    );
    assert!(
        path.clip.is_some(),
        "corner-shaped backgrounds must paint their complete source surface through the contour clip"
    );
}

#[tokio::test]
async fn rounded_background_color_padding_clip_uses_shaped_path() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 0 }\
         body { margin: 0 }\
         div { box-sizing: border-box; width: 50pt; height: 50pt; border: 6pt solid rgb(255, 0, 0); padding: 6pt;\
           border-radius: 16pt; background-color: rgb(0, 0, 0); background-clip: padding-box }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(0, 0, 0))),
        "rounded padding-box background color must not paint as a rectangular fill"
    );
    assert!(
        document.pages[0]
            .paths()
            .iter()
            .any(|path| path.fill == Some(CssColor::new(0, 0, 0)) && path.commands.len() > 5),
        "expected shaped rounded background-color path, got {:?}",
        document.pages[0].paths()
    );
}

#[tokio::test]
async fn rounded_page_margin_background_color_content_clip_uses_shaped_path() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 90pt; margin: 20pt;\
           @top-center { content: \"Header\"; width: 70pt; height: 16pt; border: 4pt solid rgb(255, 0, 0); padding: 4pt;\
             border-radius: 12pt; background-color: rgb(0, 0, 0); background-clip: content-box; color: white; font-size: 8pt } }\
         body { margin: 0; font-size: 10pt }\
         </style><p>Body</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(0, 0, 0))),
        "rounded page-margin content-box background color must not paint as a rectangular fill"
    );
    assert!(
        document.pages[0]
            .paths()
            .iter()
            .any(|path| path.fill == Some(CssColor::new(0, 0, 0)) && path.commands.len() > 5),
        "expected shaped rounded page-margin background-color path, got {:?}",
        document.pages[0].paths()
    );
}

#[tokio::test]
async fn corner_shorthand_matches_equivalent_corner_longhands() {
    let shorthand = Html::from_string(
        "<div style=\"margin:0;width:120pt;height:120pt;border:18pt solid rgb(0 128 0);background:rgb(240 240 240);corner:36px round / 18px bevel / 28px scoop / 20px notch\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let longhands = Html::from_string(
        "<div style=\"margin:0;width:120pt;height:120pt;border:18pt solid rgb(0 128 0);background:rgb(240 240 240);border-top-left-radius:36px;border-top-right-radius:18px;border-bottom-right-radius:28px;border-bottom-left-radius:20px;corner-top-left-shape:round;corner-top-right-shape:bevel;corner-bottom-right-shape:scoop;corner-bottom-left-shape:notch\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(shorthand.pages[0].rounded_rects().len(), 0);
    assert_eq!(shorthand.pages[0].paths(), longhands.pages[0].paths());
    assert!(
        shorthand.pages[0]
            .paths()
            .iter()
            .any(|path| path.fill == Some(CssColor::new(240, 240, 240)))
    );
    assert!(
        shorthand.pages[0]
            .paths()
            .iter()
            .any(|path| path.fill == Some(CssColor::new(0, 128, 0))
                && path.fill_rule == crate::document::paint::paths::RenderedPathFillRule::EvenOdd)
    );
}

#[tokio::test]
async fn uniform_solid_rounded_border_paints_as_rounded_stroke() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;background:red;border:2pt solid blue;border-radius:4pt;color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        0
    );
    let rounded_border = document.pages[0]
        .rounded_rects()
        .iter()
        .find(|rect| rect.stroke == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert_eq!(
        rounded_border.stroke_width,
        crate::PaintStrokeWidth::new(2.0)
    );
    assert_eq!(rounded_border.radii.top_left.x(), 3.0);

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("2 w"));
    assert!(has_srgb_stroke(&rendered, "0 0 1"));
    assert!(rendered.contains("S"));
}

#[tokio::test]
async fn mixed_width_solid_rounded_border_paints_as_even_odd_path() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;background:white;border-style:solid;border-color:blue;border-width:1pt 3pt 5pt 7pt;border-radius:6pt;color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        0
    );
    let border_path = document.pages[0]
        .paths()
        .iter()
        .find(|path| path.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert_eq!(
        border_path.fill_rule,
        crate::document::paint::paths::RenderedPathFillRule::EvenOdd
    );
    assert!(border_path.commands.len() >= 10);

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(has_srgb_fill(&rendered, "0 0 1"));
    assert!(rendered.contains("f*"));
}

#[tokio::test]
async fn mixed_color_solid_rounded_border_paints_clipped_side_paths() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:24pt;height:14pt;background:white;border-style:solid;border-width:3pt;border-color:red green blue black;border-radius:7pt;color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let border_paths = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.clip.is_some())
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 4);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule
                == crate::document::paint::paths::RenderedPathFillRule::EvenOdd)
    );
    assert!(
        border_paths
            .iter()
            .all(|path| path.clip.as_ref().unwrap().commands.len() == 5)
    );
    assert!(
        border_paths
            .iter()
            .any(|path| path.fill == Some(CssColor::new(255, 0, 0)))
    );
    assert!(
        border_paths
            .iter()
            .any(|path| path.fill == Some(CssColor::new(0, 128, 0)))
    );
    assert!(
        border_paths
            .iter()
            .any(|path| path.fill == Some(CssColor::new(0, 0, 255)))
    );
    assert!(
        border_paths
            .iter()
            .any(|path| path.fill == Some(CssColor::new(0, 0, 0)))
    );

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("W\nn"));
    assert!(rendered.contains("f*"));
}

#[tokio::test]
async fn rounded_inset_border_paints_clipped_shaded_side_paths() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:24pt;height:14pt;background:white;border:4pt inset rgb(120 120 120);border-radius:7pt;color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(120, 120, 120)))
    );
    let border_paths = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.clip.is_some())
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 4);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule
                == crate::document::paint::paths::RenderedPathFillRule::EvenOdd)
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(border_dark_gray()))
            .count()
            >= 2
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(border_light_gray()))
            .count()
            >= 2
    );
}

#[tokio::test]
async fn rounded_groove_border_paints_clipped_outer_and_inner_side_paths() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:24pt;height:14pt;background:white;border:6pt groove rgb(120 120 120);border-radius:8pt;color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let border_paths = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.clip.is_some())
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 8);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule
                == crate::document::paint::paths::RenderedPathFillRule::EvenOdd)
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(border_dark_gray()))
            .count()
            >= 4
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(border_light_gray()))
            .count()
            >= 4
    );
}

#[tokio::test]
async fn uniform_double_rounded_border_paints_as_two_path_rings() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:24pt;height:14pt;background:white;border:6pt double blue;border-radius:8pt;color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        0
    );
    let border_paths = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.fill == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 2);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule
                == crate::document::paint::paths::RenderedPathFillRule::EvenOdd)
    );

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("f*"));
}

#[tokio::test]
async fn mixed_double_rounded_border_paints_clipped_outer_and_inner_side_paths() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:28pt;height:18pt;background:white;border-style:double;border-width:3pt 6pt 9pt 12pt;border-color:red green blue black;border-radius:9pt;color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let border_paths = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.clip.is_some())
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 8);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule
                == crate::document::paint::paths::RenderedPathFillRule::EvenOdd)
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(CssColor::new(255, 0, 0)))
            .count()
            >= 2
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(CssColor::new(0, 128, 0)))
            .count()
            >= 2
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(CssColor::new(0, 0, 255)))
            .count()
            >= 2
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(CssColor::new(0, 0, 0)))
            .count()
            >= 2
    );
}

#[tokio::test]
async fn border_none_has_zero_used_width() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border:5pt none red;background:blue\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .count(),
        0
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
    );
}

#[tokio::test]
async fn dashed_borders_render_as_segments() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:40pt;height:10pt;border-top:2pt dashed red\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red_segments = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_segments.len(), 4);
    assert!((red_segments[0].width() - (40.0 / 7.0)).abs() < 0.001);
    assert_eq!(red_segments[0].height(), 2.0);
    assert!(document.pages[0].strokes().is_empty());
}

#[tokio::test]
async fn dotted_borders_render_as_round_dot_paths() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border-top:2pt dotted blue\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        0
    );
    let dots = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.stroke == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();

    assert_eq!(dots.len(), 1);
    assert_eq!(dots[0].stroke_width, crate::PaintStrokeWidth::new(2.0));
}

#[tokio::test]
async fn rounded_dotted_borders_clip_dots_to_side_and_border_ring() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border-top:2pt dotted blue;border-radius:4pt\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        0
    );
    let dots = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.stroke == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();

    assert_eq!(dots.len(), 1);
    assert!(dots.iter().all(|path| {
        path.clip
            .as_ref()
            .is_some_and(|clip| clip.additional_clips.len() == 1)
    }));
}

#[tokio::test]
async fn rounded_dashed_borders_clip_dashes_to_side_and_border_ring() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:40pt;height:10pt;border-top:2pt dashed red;border-radius:4pt\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .count(),
        0
    );
    let dashes = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(dashes.len(), 4);
    assert!(dashes.iter().all(|path| {
        path.commands.len() == 5
            && path.clip.as_ref().is_some_and(|clip| {
                clip.fill_rule == crate::document::paint::paths::RenderedPathFillRule::NonZero
                    && clip.additional_clips.len() == 1
                    && clip.additional_clips[0].fill_rule
                        == crate::document::paint::paths::RenderedPathFillRule::EvenOdd
            })
    }));
}

#[tokio::test]
async fn paints_stretched_border_image_slices_from_source_pixels() {
    let dir = std::env::temp_dir().join(format!("quire-border-image-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let image_path = dir.join("border.png");
    let mut image = image::RgbaImage::new(3, 3);
    for y in 0..3 {
        for x in 0..3 {
            image.put_pixel(
                x,
                y,
                image::Rgba([(x * 80) as u8, (y * 80) as u8, ((x + y) * 40) as u8, 255]),
            );
        }
    }
    image.save(&image_path).unwrap();

    let document = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 10pt } body { margin: 0 } div { width: 20pt; height: 12pt; border: 4pt solid red; border-image: url(border.png) 1; }</style><div></div>",
    )
    .with_base_path(&dir)
    .unwrap()
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .count(),
        0
    );
    let border_images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.is_clipped())
        .collect::<Vec<_>>();

    assert_eq!(border_images.len(), 8);
    assert!(
        border_images
            .iter()
            .all(|image| image.pixel_width() == 3 && image.pixel_height() == 3)
    );
    assert!(border_images.iter().all(|image| image.is_clipped()));

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(image_paint_count(&rendered, 3, 3) >= 1);
}

#[tokio::test]
async fn invalid_or_failed_border_image_uses_the_normal_border() {
    for border_image in [
        "image-set(url('data:image/png;base64,') type('image/unknown')) 1 / 10px",
        "url('data:image/png;base64,') 1 / 10px",
    ] {
        let document = Html::from_string(format!(
            "<style>@page {{ size: 140pt 100pt; margin: 10pt }} body {{ margin: 0 }} \
             div {{ width: 100pt; height: 60pt; box-sizing: border-box; background: red; \
             border: 20pt solid green; border-image: {border_image}; }}</style><div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        assert!(
            document.pages[0]
                .rects()
                .iter()
                .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0))),
            "ordinary green border should paint for {border_image}"
        );
    }
}

#[tokio::test]
async fn external_svg_url_images_paint_as_vectors_for_img_background_and_border_image() {
    let base_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let document = Html::from_string(
        r#"<style>
            @page { size: 120pt 120pt; margin: 10pt }
            body { margin: 0 }
            img { display: block; width: 30pt; height: 15pt }
            .background { width: 30pt; height: 15pt; background-image: url(external-vector.svg); background-repeat: no-repeat; background-size: 30pt 15pt }
            .border { width: 20pt; height: 12pt; border: 4pt solid transparent; border-image: url(external-vector.svg) 2 fill stretch }
        </style>
        <img src="external-vector.svg"><div class="background"></div><div class="border"></div>"#,
    )
    .with_base_path(&base_path)
    .unwrap()
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(page.images().is_empty());
    assert!(
        page.paths()
            .iter()
            .filter(|path| path.fill == Some(CssColor::new(34, 146, 212)))
            .count()
            >= 3
    );
}

/// SVG 2 external `<use>` references are expanded from the visual-resource
/// cache before `usvg` builds its local use shadow trees.
/// <https://www.w3.org/TR/SVG2/struct.html#UseElement>
#[tokio::test]
async fn inline_svg_use_imports_same_origin_html_and_nested_svg_documents() {
    let base_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let document = Html::from_string(
        r#"<style>@page { size: 320pt 120pt; margin: 10pt } body { margin: 0 }</style>
        <svg width="100" height="100"><use href="external-svg-use-symbols.html#green"/></svg>
        <svg width="100" height="100"><use href="external-svg-use-nested.svg#nested-green"/></svg>
        <svg width="100" height="100"><use href="external-svg-use-non-svg-root.xml#green"/></svg>"#,
    )
    .with_base_path(&base_path)
    .unwrap()
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1, "document={document:?}");
    assert_eq!(
        document.pages[0]
            .paths()
            .iter()
            .filter(|path| path.fill == Some(CssColor::new(0, 128, 0)))
            .count(),
        3,
        "external SVG use paths={:?}",
        document.pages[0].paths()
    );
}

#[tokio::test]
async fn optimized_svg_background_paints_below_the_normal_border() {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 120pt 120pt; margin: 0 }} body {{ margin: 0 }} \
         div {{ width: 80pt; height: 60pt; border: 4pt solid black; \
         background: url('{GREEN_50X50_SVG}') no-repeat center / cover }}</style><div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    let background = rendered
        .find("0 0.5019608 0 scn")
        .unwrap_or_else(|| panic!("expected optimized SVG background fill in {rendered}"));
    let border = rendered
        .find("0 0 0 scn")
        .unwrap_or_else(|| panic!("expected normal border fill in {rendered}"));

    assert!(
        background < border,
        "the CSS background image must paint below the border: {rendered}"
    );
}

#[tokio::test]
async fn uniform_svg_source_underlays_an_opaque_border_clip() {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 300pt 650pt; margin: 0 }} body {{ margin: 0 }} \
         div {{ width: 256px; height: 768px; border: 1px solid black; \
         background: url('{GREEN_50X50_SVG}') no-repeat center / cover }}</style><div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = filled_rects(page, CssColor::new(0, 128, 0));
    let black = filled_rects(page, CssColor::BLACK);
    assert_eq!(
        black.len(),
        4,
        "expected four normal-border strips: {black:?}"
    );
    let left = black
        .iter()
        .map(|rect| rect.x())
        .fold(f32::INFINITY, f32::min);
    let bottom = black
        .iter()
        .map(|rect| rect.y())
        .fold(f32::INFINITY, f32::min);
    let right = black
        .iter()
        .map(|rect| rect.x() + rect.width())
        .fold(f32::NEG_INFINITY, f32::max);
    let top = black
        .iter()
        .map(|rect| rect.y() + rect.height())
        .fold(f32::NEG_INFINITY, f32::max);

    let green_left = green
        .iter()
        .map(|rect| rect.x())
        .fold(f32::INFINITY, f32::min);
    let green_bottom = green
        .iter()
        .map(|rect| rect.y())
        .fold(f32::INFINITY, f32::min);
    let green_right = green
        .iter()
        .map(|rect| rect.x() + rect.width())
        .fold(f32::NEG_INFINITY, f32::max);
    let green_top = green
        .iter()
        .map(|rect| rect.y() + rect.height())
        .fold(f32::NEG_INFINITY, f32::max);

    assert!(green_left <= left && green_bottom <= bottom);
    assert!(green_right >= right && green_top >= top);
}

#[tokio::test]
async fn generated_float_svg_background_retains_its_vector_paths() {
    let base_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let document = Html::from_string(
        r#"<style>
            @page { size: 120pt 120pt; margin: 10pt }
            body { margin: 0 }
            section::before {
                background: url(external-vector.svg) no-repeat center / 50%;
                content: "";
                display: block;
                float: left;
                height: 20pt;
                margin-right: 5pt;
                width: 40pt;
            }
        </style>
        <section>Generated float SVG background.</section>"#,
    )
    .with_base_path(&base_path)
    .unwrap()
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .paths()
            .iter()
            .any(|path| path.fill == Some(CssColor::new(34, 146, 212))),
        "a generated floated pseudo's SVG background must remain vector paint"
    );
}

/// CSS Pseudo-Elements §4.1 generated boxes are ordinary tree-abiding boxes;
/// CSS 2.2 float painting must therefore retain their CSS Backgrounds image
/// layers through both DOM and frozen formatting-box traversal.
///
/// <https://drafts.csswg.org/css-pseudo-4/#generated-content>
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
/// <https://www.w3.org/TR/css-backgrounds-3/#layering>
#[tokio::test]
async fn generated_float_svg_background_matches_direct_and_frozen_traversal() {
    let base_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let stylesheet = r#"
        @page { size: 120pt 120pt; margin: 10pt }
        body { margin: 0 }
        section::before {
            background: url(external-vector.svg) no-repeat center / 50%;
            content: "";
            display: block;
            float: left;
            height: 20pt;
            margin-right: 5pt;
            width: 40pt;
        }
    "#;
    let direct = Html::from_string(format!(
        "<style>{stylesheet}</style><section>Generated float SVG background.</section>"
    ))
    .with_base_path(&base_path)
    .unwrap()
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let frozen = Html::from_string(format!(
        "<style>{stylesheet} main {{ column-count: 1; column-gap: 0 }}</style>\
         <main><section>Generated float SVG background.</section></main>"
    ))
    .with_base_path(&base_path)
    .unwrap()
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let icon_color = CssColor::new(34, 146, 212);
    let direct_paths = direct.pages[0]
        .paths()
        .iter()
        .filter(|path| path.fill == Some(icon_color))
        .cloned()
        .collect::<Vec<_>>();
    let frozen_paths = frozen.pages[0]
        .paths()
        .iter()
        .filter(|path| path.fill == Some(icon_color))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(direct_paths, frozen_paths);
}

/// CSS Backgrounds §3.4 repeats an SVG background as a tiled image layer,
/// which remains a vector `SvgPattern` when captured by a generated float.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>
#[tokio::test]
async fn repeated_svg_background_on_generated_float_retains_svg_pattern() {
    let base_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let document = Html::from_string(
        r#"<style>
            @page { size: 120pt 120pt; margin: 10pt }
            body { margin: 0 }
            section::before {
                background-image: url(generated-float-pattern.svg);
                background-repeat: repeat;
                background-size: 10pt 5pt;
                content: "";
                display: block;
                float: left;
                height: 20pt;
                width: 40pt;
            }
        </style><section>Generated float SVG background.</section>"#,
    )
    .with_base_path(&base_path)
    .unwrap()
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(page.svg_patterns.len(), 1);
    assert!(page.operations().iter().any(|operation| matches!(
        operation,
        crate::document::paint::page::PaintOperation::SvgPattern(_)
    )));
}

/// A non-repeating background layer belongs to the generated float's source
/// fragment and is not synthesized for its continuation fragments.
///
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[tokio::test]
async fn fragmented_generated_float_does_not_duplicate_non_repeating_svg_background() {
    let base_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let document = Html::from_string(
        r#"<style>
            @page { size: 100pt 100pt; margin: 10pt }
            body { margin: 0 }
            section::before {
                background: url(external-vector.svg) no-repeat center / 50%;
                content: "";
                display: block;
                float: left;
                height: 150pt;
                width: 40pt;
            }
        </style><section>Generated float SVG background.</section>"#,
    )
    .with_base_path(&base_path)
    .unwrap()
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages.len() >= 2);
    let icon_color = CssColor::new(34, 146, 212);
    assert!(
        document.pages[0]
            .paths()
            .iter()
            .any(|path| path.fill == Some(icon_color) && path.clip.is_none())
    );
    let continuation_paths = document.pages[1]
        .paths()
        .iter()
        .filter(|path| path.fill == Some(icon_color))
        .collect::<Vec<_>>();
    assert_eq!(continuation_paths.len(), 1);
    let continuation = continuation_paths[0];
    assert!(continuation.clip.is_some());
    assert!(
        continuation.paint_bounds().unwrap().min_y() >= 90.0,
        "the source-positioned SVG must fall outside the continuation and be clipped, not be re-positioned there"
    );
}

#[tokio::test]
async fn paints_repeated_border_image_tiles() {
    let dir =
        std::env::temp_dir().join(format!("quire-border-image-repeat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let image_path = dir.join("border.png");
    let mut image = image::RgbaImage::new(3, 3);
    for y in 0..3 {
        for x in 0..3 {
            image.put_pixel(
                x,
                y,
                image::Rgba([(x * 70) as u8, (y * 70) as u8, 180, 255]),
            );
        }
    }
    image.save(&image_path).unwrap();

    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 } div { width: 24pt; height: 8pt; border: 4pt solid red; border-image: url(border.png) 1 repeat; }</style><div></div>",
    )
    .with_base_path(&dir)
    .unwrap()
    .render(&RenderOptions::default()).await
    .unwrap();

    let border_images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.is_clipped())
        .collect::<Vec<_>>();
    assert!(border_images.len() > 8, "{border_images:#?}");
    assert!(
        border_images
            .iter()
            .all(|image| image.pixel_width() == 3 && image.pixel_height() == 3)
    );
    assert!(border_images.iter().all(|image| image.is_clipped()));

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    // Repeated tiles share one raster image XObject in the PDF; the individual
    // placements are represented by distinct transforms and clips. The display
    // list assertion above verifies those placements, while this checks that
    // the full source raster reaches PDF serialization.
    assert!(image_paint_count(&rendered, 3, 3) >= 1);
}

#[tokio::test]
async fn border_image_width_auto_uses_source_slice_size() {
    let dir = std::env::temp_dir().join(format!("quire-border-image-auto-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let image_path = dir.join("border.png");
    let mut image = image::RgbaImage::new(6, 6);
    for y in 0..6 {
        for x in 0..6 {
            image.put_pixel(
                x,
                y,
                image::Rgba([(x * 30) as u8, (y * 30) as u8, 220, 255]),
            );
        }
    }
    image.save(&image_path).unwrap();

    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 } div { width: 20pt; height: 12pt; border: 4pt solid red; border-image: url(border.png) 2 / auto; }</style><div></div>",
    )
    .with_base_path(&dir)
    .unwrap()
    .render(&RenderOptions::default()).await
    .unwrap();

    let border_images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.is_clipped())
        .collect::<Vec<_>>();
    assert_eq!(border_images.len(), 8);
    assert!(
        border_images
            .iter()
            .any(|image| (image_clip_size(image).height - 1.5).abs() < 0.01)
    );
    assert!(
        border_images
            .iter()
            .any(|image| (image_clip_size(image).width - 1.5).abs() < 0.01)
    );
}

#[tokio::test]
async fn border_image_widths_scale_down_before_overlapping() {
    let dir = std::env::temp_dir().join(format!("quire-border-image-fit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let image_path = dir.join("border.png");
    let mut image = image::RgbaImage::new(3, 3);
    for y in 0..3 {
        for x in 0..3 {
            image.put_pixel(
                x,
                y,
                image::Rgba([(x * 80) as u8, (y * 80) as u8, 120, 255]),
            );
        }
    }
    image.save(&image_path).unwrap();

    let document = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 10pt } body { margin: 0 } div { width: 10pt; height: 10pt; border: 2pt solid red; border-image: url(border.png) 1 / 20pt; }</style><div></div>",
    )
    .with_base_path(&dir)
    .unwrap()
    .render(&RenderOptions::default()).await
    .unwrap();

    let border_images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.is_clipped())
        .collect::<Vec<_>>();
    assert_eq!(border_images.len(), 4);
    assert!(border_images.iter().all(|image| {
        let size = image_clip_size(image);
        size.width <= 7.01 && size.height <= 7.01
    }));
    assert!(
        border_images
            .iter()
            .any(|image| (image_clip_size(image).width - 7.0).abs() < 0.01)
    );
}

/// Border-image tiles retain and scale the complete raster source, then clip
/// it to the selected destination slice. The clip, rather than the full
/// source placement rectangle, is therefore the CSS border-image tile size.
fn image_clip_size(image: &crate::document::paint::images::RenderedImage) -> PaintSize {
    use crate::document::paint::paths::RenderedPathCommand::{Close, LineTo, MoveTo};

    let commands = &image.clip().expect("border-image tile clip").commands;
    let [
        MoveTo(origin),
        LineTo(inline_end),
        LineTo(block_end),
        LineTo(_),
        Close,
    ] = commands.as_slice()
    else {
        panic!("border-image clips are rectangular: {commands:?}");
    };
    PaintSize::new(
        (inline_end.x - origin.x).abs(),
        (block_end.y - inline_end.y).abs(),
    )
}

#[tokio::test]
async fn inset_and_groove_borders_use_3d_shading() {
    let inset = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;border:2pt inset rgb(120 120 120);color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let dark = border_dark_gray();
    let light = border_light_gray();
    assert_eq!(
        inset.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(dark))
            .count(),
        2
    );
    assert_eq!(
        inset.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(light))
            .count(),
        2
    );

    let groove = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;border:2pt groove rgb(120 120 120);color:white\">Box</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        groove.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(dark) || rect.fill == Some(light))
            .count(),
        8
    );
}

fn border_dark_gray() -> CssColor {
    CssColor::srgb(80.0 / 255.0, 80.0 / 255.0, 80.0 / 255.0, 1.0)
}

fn border_light_gray() -> CssColor {
    CssColor::srgb(165.0 / 255.0, 165.0 / 255.0, 165.0 / 255.0, 1.0)
}

#[tokio::test]
async fn renders_horizontal_rules() {
    let document =
        Html::from_string("<hr style=\"margin:0;width:100pt;border:0;border-top:2pt solid red\">")
            .render(&RenderOptions::default())
            .await
            .unwrap();

    assert!(document.pages[0].lines().is_empty());
    let red = filled_rect(&document.pages[0], CssColor::new(255, 0, 0));
    assert_eq!(red.width(), 100.0);
    assert_eq!(red.height(), 2.0);
}

#[tokio::test]
async fn horizontal_rules_use_generic_patterned_border_painting() {
    let dashed =
        Html::from_string("<hr style=\"margin:0;width:40pt;border:0;border-top:2pt dashed red\">")
            .render(&RenderOptions::default())
            .await
            .unwrap();

    let red_segments = dashed.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_segments.len(), 4);
    assert!((red_segments[0].width() - (40.0 / 7.0)).abs() < 0.001);
    assert_eq!(red_segments[0].height(), 2.0);

    let dotted =
        Html::from_string("<hr style=\"margin:0;width:20pt;border:0;border-top:2pt dotted blue\">")
            .render(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(
        dotted.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        0
    );
    assert_eq!(
        dotted.pages[0]
            .paths()
            .iter()
            .filter(|path| path.stroke == Some(CssColor::new(0, 0, 255)))
            .count(),
        1
    );
}

#[tokio::test]
async fn horizontal_rules_use_generic_per_side_border_painting() {
    let document = Html::from_string(
        "<hr style=\"margin:0;width:20pt;height:10pt;border-style:solid;border-width:1pt 2pt 3pt 4pt;border-color:red green blue black\">",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    for color in [
        CssColor::new(255, 0, 0),
        CssColor::new(0, 128, 0),
        CssColor::new(0, 0, 255),
        CssColor::new(0, 0, 0),
    ] {
        assert!(
            document.pages[0]
                .rects()
                .iter()
                .any(|rect| rect.fill == Some(color)),
            "expected hr side color {color:?} in {:?}",
            document.pages[0].rects()
        );
    }
}

#[tokio::test]
async fn hr_size_and_width_presentational_hints_render_with_generic_block_layout() {
    let document = Html::from_string(
        "<style>@page{size:160pt 100pt;margin:10pt}body{margin:0}</style>\
         <hr size=\"8\" width=\"100\" style=\"margin:0;border:0;background:cyan\">",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let cyan = filled_rect(&document.pages[0], CssColor::new(0, 255, 255));
    assert!((cyan.width() - 75.0).abs() < 0.01);
    assert!((cyan.height() - 4.5).abs() < 0.01);
}

#[tokio::test]
async fn hr_color_and_size_presentational_hints_render_solid_red_border() {
    let document = Html::from_string(
        "<style>@page{size:160pt 100pt;margin:10pt}body{margin:0}</style>\
         <hr color=\"red\" size=\"10\" style=\"margin:0;width:20pt\">",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red_borders = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();
    assert_eq!(red_borders.len(), 4);
    assert!(
        red_borders
            .iter()
            .any(|rect| (rect.height() - 3.75).abs() < 0.01)
    );
}

#[tokio::test]
async fn normal_block_auto_margins_center_fixed_width() {
    let document = Html::from_string(
        "<style>@page{size:120pt 80pt;margin:10pt}body{margin:0}.box{width:20pt;height:10pt;margin-left:auto;margin-right:auto;background:green}</style><div class=\"box\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rect(&document.pages[0], CssColor::new(0, 128, 0));
    assert!((green.x() - 50.0).abs() < 0.01, "{green:?}");
    assert_eq!(green.width(), 20.0);
}

#[tokio::test]
async fn normal_block_one_sided_auto_margins_absorb_free_space() {
    let right_aligned = Html::from_string(
        "<style>@page{size:120pt 80pt;margin:10pt}body{margin:0}.box{width:20pt;height:10pt;margin-left:auto;margin-right:0;background:green}</style><div class=\"box\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let right_green = filled_rect(&right_aligned.pages[0], CssColor::new(0, 128, 0));
    assert!((right_green.x() - 90.0).abs() < 0.01, "{right_green:?}");

    let left_aligned = Html::from_string(
        "<style>@page{size:120pt 80pt;margin:10pt}body{margin:0}.box{width:20pt;height:10pt;margin-left:0;margin-right:auto;background:green}</style><div class=\"box\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let left_green = filled_rect(&left_aligned.pages[0], CssColor::new(0, 128, 0));
    assert!((left_green.x() - 10.0).abs() < 0.01, "{left_green:?}");
}

#[tokio::test]
async fn normal_block_auto_margins_follow_overconstrained_block_width_equation() {
    let document = Html::from_string(
        "<style>\
         @page{size:500pt 120pt;margin:0}body{margin:0}\
         .wrapper{width:100pt;margin-left:250pt}\
         .test{width:50pt;height:5pt;margin:auto;background:green}\
         .big{width:200pt;background:blue}\
         .fixed-right{margin-left:auto;margin-right:125pt;background:red}\
         </style>\
         <div class=\"wrapper\">\
           <div class=\"test\"></div>\
           <div class=\"test big\"></div>\
           <div class=\"test fixed-right\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rect(&document.pages[0], CssColor::new(0, 128, 0));
    assert!((green.x() - 275.0).abs() < 0.01, "{green:?}");
    assert!((green.width() - 50.0).abs() < 0.01, "{green:?}");

    let blue = filled_rect(&document.pages[0], CssColor::new(0, 0, 255));
    assert!((blue.x() - 250.0).abs() < 0.01, "{blue:?}");
    assert!((blue.width() - 200.0).abs() < 0.01, "{blue:?}");

    let red = filled_rect(&document.pages[0], CssColor::new(255, 0, 0));
    assert!((red.x() - 250.0).abs() < 0.01, "{red:?}");
    assert!((red.width() - 50.0).abs() < 0.01, "{red:?}");
}

#[tokio::test]
async fn rtl_overconstrained_fixed_width_blocks_keep_end_side() {
    let document = Html::from_string(
        "<style>@page{size:120pt 80pt;margin:10pt}body{margin:0;direction:rtl}.box{width:80pt;height:10pt;margin-left:15pt;margin-right:20pt;background:green}</style><div class=\"box\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rect(&document.pages[0], CssColor::new(0, 128, 0));
    assert!((green.x() - 10.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn paginates_simple_text_blocks() {
    let html = Html::from_string(
        "<style>@page { size: 120pt 60pt; margin: 10pt } p { margin: 0; font-size: 10pt; line-height: 10pt }</style><p>one two three four five six seven eight nine ten eleven twelve thirteen fourteen</p>",
    );
    let document = html.render(&RenderOptions::default()).await.unwrap();

    assert!(document.pages.len() > 1);
}

#[tokio::test]
async fn extracts_title_metadata() {
    let document =
        Html::from_string("<title>Example &copy; &#x1f642; &amp;lt;</title><p>Hello</p>")
            .render(&RenderOptions::default())
            .await
            .unwrap();
    assert_eq!(
        document.metadata.title.as_deref(),
        Some("Example © 🙂 &lt;")
    );
    assert_eq!(document.pages[0].lines()[0].text, "Hello");

    let ascii_title_document = Html::from_string("<title>Example PDF</title><p>Hello</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let pdf = ascii_title_document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Title (Example PDF)"));
    assert!(rendered.contains(r#"<rdf:li xml:lang="x-default">Example PDF</rdf:li>"#));
}

#[tokio::test]
async fn page_margin_text_does_not_decode_named_string_attributes_twice() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 10pt; @top-center { content: string(section, last); font-size: 8pt; line-height: 8pt } }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         .source { string-set: section attr(data-title) }\
         </style><div class=\"source\" data-title=\"&amp;lt;\">Body</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "&lt;"),
        "page margin should retain the parser-decoded literal entity: {:?}",
        document.pages[0].lines()
    );
}

#[tokio::test]
async fn extracts_author_metadata() {
    let document = Html::from_string("<meta name=\"author\" content=\"Ada Lovelace\"><p>Hello</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.metadata.author.as_deref(), Some("Ada Lovelace"));
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(pdf_searchable_text(&pdf).contains("/Author (Ada Lovelace)"));
}

#[tokio::test]
async fn extracts_creator_metadata_from_generator_meta() {
    let document =
        Html::from_string("<meta name=\"generator\" content=\"SNPSuite v3.20.0\"><p>Hello</p>")
            .render(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(
        document.metadata.creator.as_deref(),
        Some("SNPSuite v3.20.0")
    );
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(pdf_searchable_text(&pdf).contains("/Creator (SNPSuite v3.20.0)"));
}

#[tokio::test]
async fn accepts_external_stylesheet_api() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string("p { color: #00ff00 }"))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(0, 255, 0));
}

#[tokio::test]
async fn external_stylesheets_resolve_imports() {
    let dir = std::env::temp_dir().join(format!("quire-import-style-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("tokens.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "p { color: red }").unwrap();
    std::fs::write(&main_path, "@import url(tokens.css);").unwrap();

    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_file(&main_path).await.unwrap())
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn resolves_inherited_css_custom_properties() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string(
            ":root { --accent: #00ff00 } p { color: var(--accent, red) }",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(0, 255, 0));
}

#[tokio::test]
async fn applies_print_media_rules() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string(
            "@media print { p { color: red } } @media screen { p { font-family: Courier } }",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
    assert!(!line_font_contains_any(
        &document,
        &document.pages[0].lines()[0],
        &["courier"]
    ));
}

#[tokio::test]
async fn loads_linked_stylesheets_relative_to_html_file() {
    let dir = std::env::temp_dir().join(format!("quire-linked-style-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    std::fs::write(&css_path, "p { color: red }").unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>Hello</p>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn loads_root_relative_stylesheets_from_base_url() {
    let dir = std::env::temp_dir().join(format!("quire-root-linked-style-{}", std::process::id()));
    let document_dir = dir.join("css/css-page");
    let root_fonts = dir.join("fonts");
    std::fs::create_dir_all(&document_dir).unwrap();
    std::fs::create_dir_all(&root_fonts).unwrap();
    let html_path = document_dir.join("document.html");
    let css_path = root_fonts.join("ahem.css");
    std::fs::write(&css_path, "p { color: red }").unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"/fonts/ahem.css\"><p>Hello</p>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .with_base_path(&dir)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn loads_root_relative_font_face_urls_from_base_url() {
    let dir = std::env::temp_dir().join(format!("quire-root-font-face-{}", std::process::id()));
    let document_dir = dir.join("css/css-page");
    let root_fonts = dir.join("fonts");
    std::fs::create_dir_all(&document_dir).unwrap();
    std::fs::create_dir_all(&root_fonts).unwrap();
    let html_path = document_dir.join("document.html");
    let css_path = root_fonts.join("fonts.css");
    let font_path = root_fonts.join("RootFont.ttf");
    std::fs::copy(
        "weasyprint-samples/invoice/SourceSans3-Regular.ttf",
        &font_path,
    )
    .unwrap();
    std::fs::write(
        &css_path,
        "@font-face { font-family: RootFont; src: url('/fonts/RootFont.ttf') } p { font-family: RootFont; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"/fonts/fonts.css\"><p>Hello</p>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .with_base_path(&dir)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(line_font_contains_any(
        &document,
        &document.pages[0].lines()[0],
        &["source", "sans"]
    ));
}

#[tokio::test]
async fn loads_images_relative_to_html_file() {
    let dir = std::env::temp_dir().join(format!("quire-linked-image-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let image_path = dir.join("dot.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &html_path,
        "<body style=\"margin:0\"><img src=\"dot.png\" width=\"10\" height=\"20\"></body>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[0].images()[0].width(), 7.5);
    assert_eq!(document.pages[0].images()[0].height(), 15.0);
}

#[tokio::test]
async fn image_source_attributes_are_not_decoded_twice() {
    let dir =
        std::env::temp_dir().join(format!("quire-escaped-image-source-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let image_path = dir.join("dot&amp;.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &html_path,
        "<body style=\"margin:0\"><img src=\"dot&amp;amp;.png\" width=\"10\" height=\"20\"></body>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].images().len(), 1);
}

#[tokio::test]
async fn loads_root_relative_images_from_base_url() {
    let dir = std::env::temp_dir().join(format!("quire-root-linked-image-{}", std::process::id()));
    let document_dir = dir.join("css/css-page");
    let root_images = dir.join("images");
    std::fs::create_dir_all(&document_dir).unwrap();
    std::fs::create_dir_all(&root_images).unwrap();
    let html_path = document_dir.join("document.html");
    let image_path = root_images.join("dot.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &html_path,
        "<body style=\"margin:0\"><img src=\"/images/dot.png\" width=\"10\" height=\"20\"></body>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .with_base_path(&dir)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[0].images()[0].width(), 7.5);
    assert_eq!(document.pages[0].images()[0].height(), 15.0);
}

#[tokio::test]
async fn paints_background_images_relative_to_stylesheet_file() {
    let dir = std::env::temp_dir().join(format!("quire-background-image-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("bg.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "div { display:block; width:20pt; height:10pt; background: no-repeat top left / 100% 100% url(bg.png); }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><body style=\"margin:0\"><div></div></body>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[0].images()[0].width(), 20.0);
    assert_eq!(document.pages[0].images()[0].height(), 10.0);
}

#[tokio::test]
async fn background_shorthand_url_path_slash_preserves_explicit_size() {
    let dir =
        std::env::temp_dir().join(format!("quire-background-url-slash-{}", std::process::id()));
    let support_dir = dir.join("support");
    std::fs::create_dir_all(&support_dir).unwrap();
    let html_path = dir.join("document.html");
    let image_path = support_dir.join("1x1-green.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &html_path,
        r#"
        <style>
        @page { size: 200px 160px; margin: 0 }
        body { margin: 0 }
        #red {
          position: absolute;
          background: red;
          width: 100px;
          height: 100px;
        }
        div:not(#red) {
          position: absolute;
          width: 50px;
          line-height: 100px;
          background: url("support/1x1-green.png") 0 0 / 50px 100px no-repeat, red;
          color: transparent;
        }
        #test { font-size: 20px }
        #test2 { margin-left: 50px; font-size: 150px }
        </style>
        <div id="red"></div>
        <div id="test">ab</div>
        <div id="test2">ab</div>
        "#,
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    let expected_width = 50.0 * 0.75;
    let expected_height = 100.0 * 0.75;
    let images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.background)
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2, "{images:?}");
    assert!(
        images.iter().all(|image| {
            (image.width() - expected_width).abs() < 0.01
                && (image.height() - expected_height).abs() < 0.01
        }),
        "background images should use explicit 50px by 100px size: {images:?}"
    );
}

#[tokio::test]
async fn root_display_contents_repeated_background_uses_pdf_tiling_pattern() {
    let document = Html::from_string(format!(
        "<!doctype html>\
         <style>\
         @page {{ size: 100pt 80pt; margin: 0 }}\
         :root {{ display: contents; background-image: url({GREEN_1X1_PNG}); }}\
         body {{ margin: 0 }}\
         p {{ margin: 0; color: transparent }}\
         </style><p>Pass if the background is green.</p>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(page.images().is_empty());
    assert_eq!(page.image_patterns().len(), 1);
    let pattern = &page.image_patterns()[0];
    assert_eq!(pattern.width(), 100.0);
    assert_eq!(pattern.height(), 80.0);

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(
        pdf.len() < 20_000,
        "repeated 1x1 page background should not expand into individual PDF image draws"
    );
    let rendered = pdf_searchable_text(&pdf);
    assert_eq!(image_xobject_count_with_size(&rendered, 1, 1), 1);
    assert!(rendered.contains("/PatternType 1"));
    assert!(rendered.contains("/PaintType 1"));
    assert!(rendered.contains("/TilingType 1"));
    assert!(
        rendered.matches(" Do").count() <= 2,
        "expected one tile image draw inside the pattern, got PDF:\n{rendered}"
    );
}

#[tokio::test]
async fn near_zero_generated_gradient_repeats_as_one_solid_paint() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 160pt; margin: 0 } body { margin: 0 }</style>\
         <div style=\"background-image: linear-gradient(green, green); width: 100px; height: 100px; background-size: 0.2px 0.2px\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(page.images().len(), 0);
    assert_eq!(page.image_patterns().len(), 0);
    assert_eq!(
        page.rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .count(),
        1,
        "uniform background should collapse to one solid paint"
    );

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(
        pdf.len() < 50_000,
        "near-zero gradient PDF is {} bytes",
        pdf.len()
    );
    let rendered = pdf_searchable_text(&pdf);
    assert_eq!(srgb_fill_count(&rendered, "0 0.5019608 0"), 1);
    assert!(!rendered.contains("/Subtype /Image"));
}

#[tokio::test]
async fn near_zero_svg_background_collapses_to_one_solid_paint() {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 160pt 160pt; margin: 0 }} body {{ margin: 0 }}</style>\
         <div style=\"background-image: url('{GREEN_50X50_SVG}'); width: 100px; height: 100px; background-size: 0.2px 0.2px\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .count(),
        1
    );
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(
        pdf.len() < 50_000,
        "near-zero SVG PDF is {} bytes",
        pdf.len()
    );
    let rendered = pdf_searchable_text(&pdf);
    assert!(!rendered.contains("/Subtype /Form"));
    assert!(!rendered.contains("/PatternType 1"));
    assert!(!rendered.contains("/Subtype /Image"));
    assert_eq!(srgb_fill_count(&rendered, "0 0.5019608 0"), 1, "{rendered}");
}

#[tokio::test]
async fn inline_style_svg_urls_are_preloaded_and_painted_once() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inline-style-near-zero-svg-background.html");
    let document = Html::from_file(&path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
            .count(),
        1
    );
}

#[tokio::test]
async fn near_zero_png_background_uses_one_image_pattern() {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 160pt 160pt; margin: 0 }} body {{ margin: 0 }}</style>\
         <div style=\"background-image: url('{GREEN_1X1_PNG}'); width: 100px; height: 100px; background-size: 0.2px 0.2px\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].image_patterns().len(), 1);
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(
        pdf.len() < 50_000,
        "near-zero PNG PDF is {} bytes",
        pdf.len()
    );
    let rendered = pdf_searchable_text(&pdf);
    assert_eq!(rendered.matches("/Subtype /Image").count(), 1);
    assert_eq!(rendered.matches("/PatternType 1").count(), 1);
}

#[tokio::test]
async fn near_zero_color_background_remains_an_ordinary_fill() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 160pt; margin: 0 } body { margin: 0 }</style>\
         <div style=\"background-color: green; width: 100px; height: 100px; background-size: 0.2px 0.2px\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].image_patterns().len(), 0);
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(!rendered.contains("/Subtype /Image"));
    assert!(!rendered.contains("/PatternType 1"));
}

#[tokio::test]
async fn repeated_url_background_patterns_preserve_repeat_axis_steps() {
    let cases = [
        ("repeat", 6.0, 4.0),
        ("repeat-x", 6.0, 44.0),
        ("repeat-y", 66.0, 4.0),
    ];
    for (repeat, expected_step_width, expected_step_height) in cases {
        let document = Html::from_string(format!(
            "<style>\
             @page {{ size: 80pt 80pt; margin: 0 }}\
             body {{ margin: 0 }}\
             div {{ display: block; width: 30pt; height: 20pt;\
               background-image: url({GREEN_1X1_PNG});\
               background-size: 6pt 4pt;\
               background-repeat: {repeat};\
             }}\
             </style><div></div>",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let patterns = document.pages[0].image_patterns();
        assert_eq!(patterns.len(), 1, "{repeat}");
        let pattern = &patterns[0];
        assert!(
            (pattern.tiling.tile_size.width - 6.0).abs() < 0.01,
            "{repeat}"
        );
        assert!(
            (pattern.tiling.tile_size.height - 4.0).abs() < 0.01,
            "{repeat}"
        );
        assert!(
            (pattern.tiling.step.width - expected_step_width).abs() < 0.01,
            "{repeat}: {pattern:?}"
        );
        assert!(
            (pattern.tiling.step.height - expected_step_height).abs() < 0.01,
            "{repeat}: {pattern:?}"
        );
    }
}

#[tokio::test]
async fn repeated_url_background_patterns_support_space_and_round() {
    let cases = [
        ("space", 10.0, 6.0, 12.0, 8.0),
        ("round", 34.0 / 3.0, 22.0 / 4.0, 34.0 / 3.0, 22.0 / 4.0),
    ];
    for (
        repeat,
        expected_tile_width,
        expected_tile_height,
        expected_step_width,
        expected_step_height,
    ) in cases
    {
        let document = Html::from_string(format!(
            "<style>\
             @page {{ size: 80pt 80pt; margin: 0 }}\
             body {{ margin: 0 }}\
             div {{ display: block; width: 34pt; height: 22pt;\
               background-image: url({GREEN_1X1_PNG});\
               background-size: 10pt 6pt;\
               background-repeat: {repeat};\
             }}\
             </style><div></div>",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let patterns = document.pages[0].image_patterns();
        assert_eq!(patterns.len(), 1, "{repeat}");
        let pattern = &patterns[0];
        assert!(
            (pattern.tiling.tile_size.width - expected_tile_width).abs() < 0.01,
            "{repeat}: {pattern:?}"
        );
        assert!(
            (pattern.tiling.tile_size.height - expected_tile_height).abs() < 0.01,
            "{repeat}: {pattern:?}"
        );
        assert!(
            (pattern.tiling.step.width - expected_step_width).abs() < 0.01,
            "{repeat}: {pattern:?}"
        );
        assert!(
            (pattern.tiling.step.height - expected_step_height).abs() < 0.01,
            "{repeat}: {pattern:?}"
        );
    }
}

#[tokio::test]
async fn repeated_url_background_pattern_uses_clipped_paint_rect() {
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 80pt 80pt; margin: 0 }}\
         body {{ margin: 0 }}\
         div {{ display: block; width: 40pt; height: 30pt; padding: 5pt;\
           background-image: url({GREEN_1X1_PNG});\
           background-size: 10pt 10pt;\
           background-repeat: repeat;\
           background-origin: border-box;\
           background-clip: content-box;\
         }}\
         </style><div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let patterns = document.pages[0].image_patterns();
    assert_eq!(patterns.len(), 1);
    let pattern = &patterns[0];
    assert_eq!(pattern.x(), 5.0);
    assert_eq!(pattern.y(), 45.0);
    assert_eq!(pattern.width(), 40.0);
    assert_eq!(pattern.height(), 30.0);
}

#[tokio::test]
async fn repeated_url_background_pattern_keeps_rounded_background_clip() {
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 80pt 80pt; margin: 0 }}\
         body {{ margin: 0 }}\
         div {{ display: block; width: 40pt; height: 30pt; padding: 5pt;\
           border-radius: 8pt;\
           background-image: url({GREEN_1X1_PNG});\
           background-size: 10pt 10pt;\
           background-repeat: repeat;\
           background-clip: padding-box;\
         }}\
         </style><div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].image_patterns().len(), 1);
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/PatternType 1"));
    assert!(
        rendered.contains(" c\n"),
        "rounded pattern clip should serialize cubic border-radius commands"
    );
}

#[tokio::test]
async fn paints_first_page_background_image_from_page_rule() {
    let dir = std::env::temp_dir().join(format!("quire-page-background-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("cover.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "@page { size: 40pt 40pt; margin: 0 } @page :first { background: url(cover.png) no-repeat center; background-size: cover; } article { display:block; break-before: page; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>One</p><article>Two</article>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[0].images()[0].width(), 40.0);
    assert_eq!(document.pages[0].images()[0].height(), 40.0);
    assert!(document.pages[1].images().is_empty());
}

#[tokio::test]
async fn page_background_origin_selects_page_box_positioning_area() {
    let dir = std::env::temp_dir().join(format!(
        "quire-page-background-origin-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("cover.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "@page { size: 100pt 80pt; margin: 10pt; border: 5pt solid blue; padding: 7pt; background: url(cover.png) no-repeat top left; background-origin: border-box; }\
         @page padding { background-origin: padding-box; }\
         @page content { background-origin: content-box; }\
         body, p, article { margin: 0; font-size: 10pt; line-height: 10pt; }\
         article { display: block; break-before: page; }\
         .padding { page: padding; }\
         .content { page: content; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>Border</p><article class=\"padding\">Padding</article><article class=\"content\">Content</article>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages.len(), 3);
    let images = document
        .pages
        .iter()
        .map(|page| {
            assert_eq!(page.images().len(), 1);
            &page.images()[0]
        })
        .collect::<Vec<_>>();

    assert_eq!(images[0].x(), 10.0);
    assert_eq!(images[0].y(), 69.25);
    assert_eq!(images[1].x(), 15.0);
    assert_eq!(images[1].y(), 64.25);
    assert_eq!(images[2].x(), 22.0);
    assert_eq!(images[2].y(), 57.25);
}

#[tokio::test]
async fn page_border_paints_inside_page_margin() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 160pt; margin: 20pt; border: 10pt solid green }\
         body { margin: 0 }\
         </style><p>x</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rects(&document.pages[0], CssColor::new(0, 128, 0));
    assert_eq!(green.len(), 4, "expected four page border rects: {green:?}");

    let min_x = green
        .iter()
        .map(|rect| rect.x())
        .min_by(f32::total_cmp)
        .unwrap();
    let min_y = green
        .iter()
        .map(|rect| rect.y())
        .min_by(f32::total_cmp)
        .unwrap();
    let max_x = green
        .iter()
        .map(|rect| rect.x() + rect.width())
        .max_by(f32::total_cmp)
        .unwrap();
    let max_y = green
        .iter()
        .map(|rect| rect.y() + rect.height())
        .max_by(f32::total_cmp)
        .unwrap();

    assert_eq!(min_x, 20.0);
    assert_eq!(min_y, 20.0);
    assert_eq!(max_x, 180.0);
    assert_eq!(max_y, 140.0);
}

#[tokio::test]
async fn page_border_with_zero_margin_paints_at_page_edge() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 160pt; margin: 0; border: 10pt solid green }\
         body { margin: 0 }\
         </style><p>x</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rects(&document.pages[0], CssColor::new(0, 128, 0));
    assert_eq!(green.len(), 4, "expected four page border rects: {green:?}");

    let min_x = green
        .iter()
        .map(|rect| rect.x())
        .min_by(f32::total_cmp)
        .unwrap();
    let min_y = green
        .iter()
        .map(|rect| rect.y())
        .min_by(f32::total_cmp)
        .unwrap();
    let max_x = green
        .iter()
        .map(|rect| rect.x() + rect.width())
        .max_by(f32::total_cmp)
        .unwrap();
    let max_y = green
        .iter()
        .map(|rect| rect.y() + rect.height())
        .max_by(f32::total_cmp)
        .unwrap();

    assert_eq!(min_x, 0.0);
    assert_eq!(min_y, 0.0);
    assert_eq!(max_x, 200.0);
    assert_eq!(max_y, 160.0);
}

#[tokio::test]
async fn page_background_clip_crops_image_to_page_content_box() {
    let dir =
        std::env::temp_dir().join(format!("quire-page-background-clip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("cover.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "@page { size: 100pt 80pt; margin: 10pt; border: 5pt solid blue; padding: 7pt;\
          background: url(cover.png) no-repeat top left / 80pt 60pt border-box content-box; }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>Clip</p>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].images().len(), 1);
    let image = &document.pages[0].images()[0];
    assert!(image.source_rect().is_some());
}

#[tokio::test]
async fn page_background_repeat_y_tiles_from_positioned_image() {
    let dir = std::env::temp_dir().join(format!(
        "quire-page-background-repeat-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("tile.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "@page { size: 30pt 35pt; margin: 0; background-image: url(tile.png); background-size: 10pt 10pt; background-repeat: repeat-y; background-position: top left; }\
         body { margin: 0; font-size: 10pt; line-height: 10pt; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>Tile</p>",
    )
    .unwrap();

    let document = Html::from_file(&html_path)
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    let page = &document.pages[0];
    assert!(page.images().is_empty());
    assert_eq!(page.image_patterns().len(), 1);
    let pattern = &page.image_patterns()[0];
    assert_eq!(pattern.x(), 0.0);
    assert_eq!(pattern.y(), 0.0);
    assert_eq!(pattern.width(), 30.0);
    assert_eq!(pattern.height(), 35.0);
    assert_eq!(pattern.tiling.tile_size.width, 10.0);
    assert_eq!(pattern.tiling.tile_size.height, 10.0);
    // A non-repeating axis uses an expanded pattern step so one PDF pattern
    // cell cannot repeat inside the paint area.
    assert_eq!(pattern.tiling.step.width, 70.0);
    assert_eq!(pattern.tiling.step.height, 10.0);
    assert_eq!(pattern.tiling.origin.x, 0.0);
    // The first full tile starts below the clipped page area; the pattern
    // clips that partial tile to the 5pt strip at the page bottom.
    assert_eq!(pattern.tiling.origin.y, -5.0);
}

#[tokio::test]
async fn page_background_paints_multiple_image_layers_with_independent_geometry() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 40pt 40pt; margin: 0;\
           background-image: url({png}), url({png});\
           background-size: 10pt 10pt, 5pt 5pt;\
           background-repeat: no-repeat, no-repeat;\
           background-position: top left, bottom right;\
         }}\
         body {{ margin: 0; font-size: 10pt; line-height: 10pt }}\
         </style><p>Layers</p>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.background)
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert!(images.iter().any(|image| image.x() == 0.0
        && image.y() == 30.0
        && image.width() == 10.0
        && image.height() == 10.0));
    assert!(images.iter().any(|image| image.x() == 35.0
        && image.y() == 0.0
        && image.width() == 5.0
        && image.height() == 5.0));
}

#[tokio::test]
async fn normal_box_background_layers_use_independent_origin_and_clip() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 80pt 80pt; margin: 0 }}\
         body {{ margin: 0 }}\
         div {{ display: block; width: 40pt; height: 40pt; border: 5pt solid transparent; padding: 5pt;\
           background-image: url({png}), url({png});\
           background-size: 20pt 20pt, 40pt 40pt;\
           background-position: top left, top left;\
           background-repeat: no-repeat, no-repeat;\
           background-origin: content-box, border-box;\
           background-clip: content-box, padding-box;\
         }}\
         </style><div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.background)
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert!(images.iter().any(|image| image.x() == 10.0
        && image.y() == 50.0
        && image.width() == 20.0
        && image.height() == 20.0
        && image.source_rect().is_some()));
    // The second layer keeps its 40pt destination tile and carries the
    // padding-box crop as a paint clip.  Cropping the destination rectangle
    // itself would change `background-position` and resampling semantics.
    assert!(images.iter().any(|image| image.x() == 0.0
        && image.y() == 40.0
        && image.width() == 40.0
        && image.height() == 40.0
        && image.source_rect().is_some()
        && image.is_clipped()));
}

#[tokio::test]
async fn page_margin_background_layers_use_independent_origin_and_clip() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 80pt 80pt; margin: 20pt;\
           @top-center {{ content: \"\"; width: 40pt; height: 20pt; border: 5pt solid transparent; padding: 5pt;\
             background-image: url({png}), url({png});\
             background-size: 10pt 10pt, 40pt 20pt;\
             background-position: top left, top left;\
             background-repeat: no-repeat, no-repeat;\
             background-origin: content-box, border-box;\
             background-clip: content-box, padding-box;\
           }}\
         }}\
         body {{ margin: 0 }}\
         </style><p>x</p>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.background)
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert!(images.iter().any(|image| image.width() == 10.0
        && image.height() == 10.0
        && image.source_rect().is_some()));
    assert!(images.iter().any(|image| image.width() < 40.0
        && image.height() < 20.0
        && image.source_rect().is_some()));
}

#[tokio::test]
async fn background_paints_multiple_linear_gradient_layers() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 0 }\
         body { margin: 0 }\
         div { display: block; width: 40pt; height: 40pt;\
           background-image: linear-gradient(to bottom, red 0pt, red 40pt), linear-gradient(to right, blue 0pt, blue 40pt);\
         }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
    );
}

#[tokio::test]
async fn background_paints_angled_hard_stop_linear_gradient() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 0 }\
         body { margin: 0 }\
         div { display: block; width: 40pt; height: 40pt;\
           background-image: linear-gradient(30deg, red 50%, blue 50%);\
         }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .paths()
            .iter()
            .any(|path| path.fill == Some(CssColor::new(255, 0, 0)))
    );
    assert!(
        document.pages[0]
            .paths()
            .iter()
            .any(|path| path.fill == Some(CssColor::new(0, 0, 255)))
    );
}

#[tokio::test]
async fn background_paints_smooth_linear_gradient_as_vector_pattern() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 0 }\
         body { margin: 0 }\
         div { display: block; width: 40pt; height: 40pt;\
           background-image: linear-gradient(in srgb, red, blue);\
         }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .gradient_patterns()
            .iter()
            .any(|pattern| pattern.width() == 40.0 && pattern.height() == 40.0)
    );
}

#[tokio::test]
async fn background_paints_smooth_radial_gradient_as_vector_pattern() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 0 }\
         body { margin: 0 }\
         div { display: block; width: 40pt; height: 30pt;\
           background-image: radial-gradient(circle at 25% 75% in srgb, red, blue);\
         }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .gradient_patterns()
            .iter()
            .any(|pattern| pattern.width() == 40.0 && pattern.height() == 30.0)
    );
}

#[tokio::test]
async fn background_tiles_sized_repeating_radial_gradient() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 60pt; margin: 0 }\
         body { margin: 0 }\
         div { display: block; width: 60pt; height: 40pt;\
           background-image: repeating-radial-gradient(circle in srgb, red 0pt, red 5pt, blue 5pt, blue 10pt);\
           background-size: 20pt 20pt;\
           background-repeat: repeat;\
         }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages[0].images().is_empty());
    assert_eq!(document.pages[0].gradient_patterns().len(), 1);
}

#[tokio::test]
async fn background_paints_transparent_linear_gradient_with_alpha() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 0 }\
         body { margin: 0 }\
         div { display: block; width: 40pt; height: 40pt;\
           background-image: linear-gradient(to bottom, transparent, red);\
         }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(pdf_searchable_text(&pdf).contains("/SMask"));
}

#[tokio::test]
async fn page_margin_background_paints_radial_gradient_layer() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 20pt;\
           @top-center { content: \"\"; width: 40pt; height: 20pt;\
             background-image: radial-gradient(ellipse farthest-corner at center in srgb, red, blue);\
           }\
         }\
         body { margin: 0 }\
         </style><p>x</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .gradient_patterns()
            .iter()
            .any(|pattern| pattern.width() == 40.0 && pattern.height() == 20.0)
    );
}

#[tokio::test]
async fn page_background_paints_radial_gradient_layer() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 0;\
           background-image: radial-gradient(circle closest-side at 50% 50% in srgb, red, blue);\
         }\
         body { margin: 0 }\
         </style><p>x</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .gradient_patterns()
            .iter()
            .any(|pattern| pattern.width() == 80.0 && pattern.height() == 80.0)
    );
}

#[tokio::test]
async fn rounded_background_linear_gradient_layers_use_clip_paths() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 0 }\
         body { margin: 0 }\
         div { width: 40pt; height: 40pt; border-radius: 10pt;\
           background-image: linear-gradient(90deg, red 0pt, red 20pt, blue 20pt, blue 40pt),\
                             linear-gradient(0deg, lime 0pt, lime 20pt, black 20pt, black 40pt);\
         }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0].rects().iter().all(|rect| !matches!(
            rect.fill,
            Some(color)
                if color == CssColor::new(255, 0, 0)
                    || color == CssColor::new(0, 0, 255)
                    || color == CssColor::new(0, 255, 0)
                    || color == CssColor::new(0, 0, 0)
        )),
        "rounded gradient backgrounds should not paint unclipped rectangular bands"
    );
    let clipped_gradient_paths = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.clip.is_some())
        .collect::<Vec<_>>();
    assert!(
        clipped_gradient_paths.len() >= 4,
        "expected clipped gradient bands, got {:?}",
        document.pages[0].paths()
    );
    assert!(
        clipped_gradient_paths
            .iter()
            .all(|path| path.clip.as_ref().unwrap().commands.len() > 5)
    );
}

#[tokio::test]
async fn rounded_background_url_layer_clips_image_draw() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 80pt 80pt; margin: 0 }}\
         body {{ margin: 0 }}\
         div {{ width: 40pt; height: 40pt; border-radius: 10pt;\
           background: url({png}) no-repeat 0 0 / 40pt 40pt;\
         }}\
         </style><div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.background);
    assert_eq!(images.count(), 1);
    assert_pdf_clips_image_draw(&document);
}

#[tokio::test]
async fn rounded_page_margin_linear_gradient_background_uses_clip_paths() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 20pt;\
           @top-center { content: \"\"; width: 40pt; height: 20pt; border-radius: 8pt;\
             background-image: linear-gradient(90deg, red 0pt, red 20pt, blue 20pt, blue 40pt);\
           }\
         }\
         body { margin: 0 }\
         </style><p>x</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0].rects().iter().all(|rect| !matches!(
            rect.fill,
            Some(color) if color == CssColor::new(255, 0, 0) || color == CssColor::new(0, 0, 255)
        )),
        "rounded page-margin gradient backgrounds should not paint unclipped rectangular bands"
    );
    let clipped_gradient_paths = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.clip.is_some());
    assert!(
        clipped_gradient_paths.count() >= 2,
        "expected clipped page-margin gradient bands, got {:?}",
        document.pages[0].paths()
    );
}

#[tokio::test]
async fn rounded_page_margin_url_background_clips_image_draw() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 80pt 80pt; margin: 20pt;\
           @top-center {{ content: \"\"; width: 40pt; height: 20pt; border-radius: 8pt;\
             background: url({png}) no-repeat 0 0 / 40pt 20pt;\
           }}\
         }}\
         body {{ margin: 0 }}\
         </style><p>x</p>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.background);
    assert_eq!(images.count(), 1);
    assert_pdf_clips_image_draw(&document);
}

#[tokio::test]
async fn rounded_page_background_linear_gradient_uses_clip_paths() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 0; border-radius: 12pt;\
           background-image: linear-gradient(0deg, red 0pt, red 40pt, blue 40pt, blue 80pt);\
         }\
         body { margin: 0 }\
         </style><p>x</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0].rects().iter().all(|rect| !matches!(
            rect.fill,
            Some(color) if color == CssColor::new(255, 0, 0) || color == CssColor::new(0, 0, 255)
        )),
        "rounded page gradient backgrounds should not paint unclipped rectangular bands"
    );
    let clipped_gradient_paths = document.pages[0]
        .paths()
        .iter()
        .filter(|path| path.clip.is_some());
    assert!(
        clipped_gradient_paths.count() >= 2,
        "expected clipped page gradient bands, got {:?}",
        document.pages[0].paths()
    );
}

#[tokio::test]
async fn rounded_page_background_url_layer_clips_image_draw() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 80pt 80pt; margin: 10pt; border-radius: 10pt;\
           background: url({png}) no-repeat 0 0 / 60pt 60pt;\
         }}\
         body {{ margin: 0 }}\
         </style><p>x</p>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0]
        .images()
        .iter()
        .filter(|image| image.background);
    assert_eq!(images.count(), 1);
    assert_pdf_clips_image_draw(&document);
}

#[tokio::test]
async fn background_tiles_sized_repeating_linear_gradient() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 60pt; margin: 0 }\
         body { margin: 0 }\
         div { display: block; width: 60pt; height: 40pt;\
           background-image: repeating-linear-gradient(to right in srgb, red 0pt, red 10pt, blue 10pt, blue 20pt);\
           background-size: 20pt 20pt;\
           background-repeat: repeat;\
         }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages[0].images().is_empty());
    assert_eq!(document.pages[0].gradient_patterns().len(), 1);
}

#[tokio::test]
async fn supports_class_and_id_selectors() {
    let document = Html::from_string("<p class=\"lead\">Lead</p><p id=\"note\">Note</p>")
        .with_stylesheet(Css::from_string(
            ".lead { color: blue } p#note { color: red }",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(0, 0, 255));
    assert_eq!(document.pages[0].lines()[1].color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn supports_servo_attribute_and_link_selectors() {
    let document =
        Html::from_string("<p data-kind=\"lead\">Lead</p><a href=\"https://example.com\">Link</a>")
            .with_stylesheet(Css::from_string(
                "[data-kind=lead] { color: red } a:link { font-family: monospace }",
            ))
            .render(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
    assert!(line_font_is_monospace(
        &document,
        &document.pages[0].lines()[1]
    ));
}

#[tokio::test]
async fn applies_simple_css_specificity() {
    let document = Html::from_string("<p class=\"lead\" id=\"hero\">Hero</p>")
        .with_stylesheet(Css::from_string(
            "#hero { color: red } .lead { color: blue } p { color: green }",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
}

#[tokio::test]
async fn supports_basic_descendant_and_child_selectors() {
    let document =
        Html::from_string("<div class=\"wrapper\"><p>Child</p><div><p>Nested</p></div></div>")
            .with_stylesheet(Css::from_string(
                "div.wrapper p { font-family: monospace } div.wrapper > p { color: red }",
            ))
            .render(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "Child");
    assert!(line_font_is_monospace(
        &document,
        &document.pages[0].lines()[0]
    ));
    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
    assert_eq!(document.pages[0].lines()[1].text, "Nested");
    assert!(line_font_is_monospace(
        &document,
        &document.pages[0].lines()[1]
    ));
    assert_eq!(document.pages[0].lines()[1].color, CssColor::BLACK);
}

#[tokio::test]
async fn child_selectors_keep_nested_inline_descendants_out_of_direct_child_rules() {
    let source = "<style>@page { size: 160pt 80pt; margin: 10pt } body { margin: 0 } div { font: 10pt/10pt monospace } .inner { color: red } .middle { color: blue }</style><div>A<span><span class=\"inner\">B</span><span class=\"middle\">C</span></span>D</div>";
    let baseline = Html::from_string(source)
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let styled = Html::from_string(source)
        .with_stylesheet(Css::from_string("div > span { margin-right: 10pt }"))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let run_x = |document: &quire::Document, text: &str| {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.runs.iter().any(|run| run.text.as_ref() == text))
            .unwrap_or_else(|| {
                panic!(
                    "expected {text:?} inline text run in {:?}",
                    document.pages[0].lines()
                )
            });
        line.runs
            .iter()
            .find(|run| run.text.as_ref() == text)
            .map(|run| line.x() + run.x_offset)
            .unwrap_or_else(|| panic!("expected {text:?} run in {:?}", line.runs))
    };

    assert!((run_x(&styled, "B") - run_x(&baseline, "B")).abs() < 0.01);
    assert!(
        (run_x(&styled, "C") - run_x(&baseline, "C")).abs() < 0.01,
        "a direct-child margin must not apply to the nested span"
    );
    assert!(
        ((run_x(&styled, "D") - run_x(&baseline, "D")) - 10.0).abs() < 0.01,
        "the direct span's margin should appear only after its inline contents"
    );
}
