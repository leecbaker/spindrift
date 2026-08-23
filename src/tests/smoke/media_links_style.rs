use super::*;

/// A propagated vertical body establishes the document's principal flow.
/// Consecutive blocks must therefore consume the physical horizontal block
/// track while their descendants retain the same physical inline-start edge.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[tokio::test]
async fn propagated_vertical_body_advances_block_children_before_laying_out_image() {
    let document = Html::from_string(format!(
        "<style>
           @page {{ size: 200pt 200pt; margin: 10pt }}
           body {{ writing-mode: vertical-rl; margin: 0 }}
           .first {{ width: 40pt; height: 60pt; margin: 0; background: blue }}
           .second {{ width: 30pt; height: 50pt; margin: 0; background: red }}
           img {{ display: block; width: 20pt; height: 30pt }}
         </style>
         <div class=\"first\"></div><div class=\"second\"><img src=\"{GREEN_100_PNG}\"></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1, "document={document:?}");
    let page = &document.pages[0];
    let first = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("expected first fixed-size block");
    let second = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("expected second fixed-size block");
    let image = page.images().first().expect("expected replaced image");

    assert!(
        second.x() + second.width() <= first.x() + 0.01,
        "vertical-rl block progression must move the second child left: first={first:?}, second={second:?}"
    );
    assert!(
        ((second.y() + second.height()) - (image.y() + image.height())).abs() < 0.01,
        "the image must begin at its vertical principal-flow inline-start edge: second={second:?}, image={image:?}"
    );
}

/// A propagated vertical body resolves each direct child's geometry before it
/// is painted. In particular, the UA paragraph block-start margin moves the
/// following replaced block through the horizontal vertical-rl track exactly
/// once; it is not a paint-time translation.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[tokio::test]
async fn propagated_vertical_body_uses_paragraph_block_margin_for_replaced_child() {
    let document = Html::from_string(format!(
        "<style>
           @page {{ size: 300pt 200pt; margin: 0 }}
           body {{ writing-mode: vertical-rl; margin: 0; font-size: 12pt }}
           .blue {{ width: 75pt; height: 75pt; margin: 0; background: blue }}
           p {{ width: 120pt }}
           img {{ display: block; width: 120pt; height: 30pt }}
         </style>
         <div class=\"blue\"></div><p><img src=\"{GREEN_100_PNG}\"></p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1, "document={document:?}");
    let page = &document.pages[0];
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("expected fixed-size blue block");
    let image = page.images().first().expect("expected replaced image");

    assert!(
        ((blue.x() + blue.width()) - 300.0).abs() < 0.01,
        "blue={blue:?}"
    );
    assert!(
        ((blue.y() + blue.height()) - (image.y() + image.height())).abs() < 0.01,
        "both children start at the vertical principal flow inline-start: blue={blue:?}, image={image:?}"
    );
    assert!(
        ((image.x() + image.width()) - (blue.x() - 12.0)).abs() < 0.01,
        "the paragraph's 1em logical block-start margin must move the image once: blue={blue:?}, image={image:?}"
    );
}

const GREEN_100_PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAGQAAABkCAIAAAD/gAIDAAAAkElEQVR42u3QMQ0AAAjAsElHOhb4eJpUQWviSoEsWbJkyUKBLFmyZMlCgSxZsmTJQoEsWbJkyUKBLFmyZMlCgSxZsmTJQoEsWbJkyUKBLFmyZMlCgSxZsmTJQoEsWbJkyUKBLFmyZMlCgSxZsmTJQoEsWbJkyUKBLFmyZMlCgSxZsmTJQoEsWbJkyUKBLFnfFhDniR6UCYQPAAAAAElFTkSuQmCC";

#[tokio::test]
async fn object_uses_fallback_for_missing_empty_and_unsupported_resources() {
    let document = Html::from_string(format!(
        "<object>missing</object><object data=\"\">empty</object><object type=\"application/pdf\" data=\"{GREEN_100_PNG}\">unsupported</object>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let visible_text = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .map(|line| line.text.as_str())
        .collect::<String>();

    assert!(
        visible_text.contains("missingemptyunsupported"),
        "{visible_text:?}"
    );
    assert!(
        document.pages.iter().all(|page| page.images().is_empty()),
        "fallback objects must not emit their unavailable resource"
    );
}

#[tokio::test]
async fn object_with_supported_image_suppresses_fallback_content() {
    let document = Html::from_string(format!(
        "<object data=\"{GREEN_100_PNG}\">fallback text must be hidden</object>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let visible_text = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .map(|line| line.text.as_str())
        .collect::<String>();

    assert!(!visible_text.contains("fallback text"), "{visible_text:?}");
    assert_eq!(
        document
            .pages
            .iter()
            .map(|page| page.images().len())
            .sum::<usize>(),
        1
    );
}

#[tokio::test]
async fn display_contents_on_unusual_html_elements_computes_to_none() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<meta charset="utf-8">
<title>CSS Display: display:contents and unusual HTML elements as display:none</title>
<style>
  html { font-kerning: none; font-feature-settings: "kern" off; }
  body { overflow: hidden }
  br, wbr, meter, progress, canvas, embed, object, audio, iframe, img, video,
  input, textarea, select {
    display: contents;
    border: 10px solid red;
    width: 200px; height: 200px;
  }
</style>
<p>You should see the word PASS below.</p>
<div>
  <meter></meter>
  <progress></progress>
  <canvas></canvas>
  <embed>
  <object>FAIL</object>
  <audio controls></audio>
  <iframe></iframe>
  <img>
  <video></video>
  <input></input>
  <textarea></textarea>
  <select></select>
</div>
P<br>A<wbr>S<br>S"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let visible_text = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .map(|line| line.text.as_str())
        .collect::<String>();

    assert_eq!(
        visible_text.matches("PASS").count(),
        2,
        "instruction and result should each contain PASS: {visible_text:?}"
    );
    assert!(
        visible_text.ends_with("PASS"),
        "rendered result should end with PASS: {visible_text:?}"
    );
    assert!(
        !visible_text.contains("FAIL"),
        "object fallback contents should be suppressed: {visible_text:?}"
    );

    let red = CssColor::new(255, 0, 0);
    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.rects())
            .all(|rect| rect.fill != Some(red)),
        "unusual elements should not paint red rects"
    );
    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.paths())
            .all(|path| path.fill != Some(red)),
        "unusual elements should not paint red border paths"
    );
    assert!(
        document.pages.iter().all(|page| page.images().is_empty()),
        "display:none unusual elements should not emit image output"
    );
}

#[tokio::test]
async fn select_4_option_optgroup_display_none_hides_red_form_content() {
    let document = Html::from_string(
        r#"<!DOCTYPE HTML>
<html><head>
  <meta charset="utf-8">
  <style>
    .none { display:none; }
    .contents { display:contents; }
    .red { color: red; }
    .green { color: green; }
    select { -webkit-appearance: none; }
  </style>
</head>
<body>
<pre>FAIL if there is any red color</pre>

<option class="none red">text</option>
<optgroup class="none red">text</optgroup>

<optgroup class="none red"><option>option</option></optgroup>
<optgroup><option class="none red">option</option></optgroup>
<optgroup class="contents red"><option class="none">option</option></optgroup>
<optgroup class="contents green" label="optgroup"><option class="none red">option</option></optgroup>
<optgroup class="none red" label="optgroup"><option class="red">option</option></optgroup>

<br>

<select class="red" size="4">select</select>
<select size="4" class="red"><optgroup class="none" label="optgroup"></select>
<select size="4" class="red"><option class="none">option</select>
<select size="4" class="red"><optgroup><option class="none">option</select>
<select size="4"><optgroup class="none"><option class="green">option</select>
<select size="4" class="red"><optgroup class="none green" label="optgroup"><option>option</select>
<select size="4" class="red"><optgroup class="none"><option class="none">option</select>
<select size="4" class="red"><optgroup class="none green" label="optgroup"><option class="none">option</select>

</body></html>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = CssColor::new(255, 0, 0);
    let visible_text = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .map(|line| line.text.as_str())
        .collect::<String>();

    assert!(
        visible_text.contains("FAIL if there is any red color"),
        "instruction should render: {visible_text:?}"
    );
    assert!(
        !visible_text.contains("select") && !visible_text.contains("option"),
        "hidden select/option text should not render: {visible_text:?}"
    );
    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .all(|line| line.color != red),
        "no red text or generated select chrome should render: {:?}",
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .collect::<Vec<_>>()
    );
    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.rects())
            .all(|rect| rect.fill != Some(red) && rect.stroke != Some(red)),
        "no red vector rects should render"
    );
    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.paths())
            .all(|path| path.fill != Some(red) && path.stroke != Some(red)),
        "no red vector paths should render"
    );
}

#[tokio::test]
async fn baseline_shift_center_centers_inline_images_in_line_box() {
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 260pt 180pt; margin: 20pt }}\
         body {{ margin: 0 }}\
         #line {{ font: 20pt/20pt sans-serif }}\
         .big {{ baseline-shift: center; font-size: 100pt; line-height: 100pt; background: rgb(255 0 0); color: transparent }}\
         img {{ baseline-shift: center; width: 60pt; height: 60pt }}\
         .small {{ baseline-shift: center; font-size: 20pt; line-height: 20pt; background: rgb(0 0 255); color: transparent }}\
         </style>\
         <span id=\"line\"><span class=\"big\">X</span><img src=\"{GREEN_100_PNG}\"><span class=\"small\">X</span></span>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(page.images().len(), 1, "images={:?}", page.images());
    let image = &page.images()[0];
    assert!((image.width() - 60.0).abs() < 0.01, "image={image:?}");
    assert!((image.height() - 60.0).abs() < 0.01, "image={image:?}");

    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap_or_else(|| panic!("expected big inline background: {:?}", page.rects()));
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap_or_else(|| panic!("expected small inline background: {:?}", page.rects()));

    let image_center = image.y() + image.height() / 2.0;
    let red_center = red.y() + red.height() / 2.0;
    let blue_center = blue.y() + blue.height() / 2.0;
    assert!(
        (image_center - red_center).abs() < 0.5,
        "centered image should match the large centered inline box: image={image:?}, red={red:?}"
    );
    assert!(
        (blue_center - red_center).abs() < 0.5,
        "small centered inline box should match the same line center: blue={blue:?}, red={red:?}"
    );
}

#[tokio::test]
async fn renders_png_data_uri_images() {
    let html = Html::from_string(
        "<img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\" style=\"height: 10pt\">",
    );
    let document = html.render(&RenderOptions::default()).await.unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[0].images()[0].pixel_width(), 1);
    assert_eq!(document.pages[0].images()[0].height(), 10.0);

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    // A uniform opaque raster can be emitted as an equivalent calibrated PDF
    // fill. The retained document image still establishes the replaced
    // element's layout and accessibility semantics; this assertion only
    // verifies that its PDF paint representation is present.
    assert!(
        rendered.contains("/Subtype /Image") || rendered.contains("/CSsRGB cs"),
        "{rendered}"
    );
    if rendered.contains("/Subtype /Image") {
        assert!(rendered.contains("/Interpolate false"));
        assert!(rendered.contains("/Im1 Do"));
    }
}

#[tokio::test]
async fn visible_overflow_allows_an_inline_replaced_image_to_escape_its_content_box() {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 160pt 120pt; margin: 0 }} body {{ margin: 0 }} \
         img {{ width: 25pt; height: 25pt; object-fit: none; object-position: left top; overflow: visible; border-radius: 50% }}</style>\
         <img src=\"{GREEN_100_PNG}\">"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let image = &document.pages[0].images()[0];
    assert_eq!(image.width(), 75.0, "image={image:?}");
    assert_eq!(image.height(), 75.0, "image={image:?}");
    assert!(!image.is_clipped(), "image={image:?}");
}

#[tokio::test]
async fn renders_only_the_first_frame_of_an_animated_gif_data_uri() {
    use image::{Frame, RgbaImage};

    let first = RgbaImage::from_raw(2, 1, vec![230, 32, 16, 255, 0, 0, 0, 0])
        .expect("RGBA dimensions match the sample pixels");
    let second = RgbaImage::from_raw(2, 1, vec![0, 96, 255, 255, 0, 96, 255, 255])
        .expect("RGBA dimensions match the sample pixels");
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        encoder
            .encode_frames([Frame::new(first), Frame::new(second)])
            .expect("GIF sample encodes");
    }
    let image = base64::engine::general_purpose::STANDARD.encode(bytes);
    let document = Html::from_string(format!(
        "<img src=\"data:image/gif;base64,{image}\" height=\"10pt\">"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rendered_image = &document.pages[0].images()[0];
    assert_eq!(rendered_image.pixel_width(), 2);
    assert_eq!(rendered_image.pixel_height(), 1);
    let crate::document::paint::images::RenderedImageSource::Stored { image_id, .. } =
        &rendered_image.source
    else {
        panic!("HTML GIF should retain a store-backed source");
    };
    let raster = document
        .image_store
        .with_rasterized(*image_id, |raster| raster)
        .expect("GIF first frame rasterizes for PDF output");
    assert_eq!(raster.rgb, vec![230, 32, 16, 0, 0, 0]);
    assert_eq!(raster.alpha, Some(vec![255, 0]));

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Subtype /Image"));
    assert!(rendered.contains("/FlateDecode"));
    assert!(!rendered.contains("/DCTDecode"));
}

#[tokio::test]
async fn renders_only_the_first_frame_of_an_animated_webp_data_uri() {
    const ANIMATED_WEBP: &str = "UklGRoYAAABXRUJQVlA4WAoAAAACAAAAAQAAAAAAQU5JTQYAAAD/////AABBTk1GKgAAAAAAAAAAAAEAAAAAAGQAAAJWUDhMEgAAAC8BAAAADzAgYz4Q8x94yIj+B0FOTUYoAAAAAAAAAAAAAQAAAAAAZAAAAFZQOEwQAAAALwEAAAAHULDof/8DEdH/AA==";
    let document = Html::from_string(format!(
        "<img src=\"data:image/webp;base64,{ANIMATED_WEBP}\" height=\"10pt\">"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rendered_image = &document.pages[0].images()[0];
    assert_eq!(rendered_image.pixel_width(), 2);
    assert_eq!(rendered_image.pixel_height(), 1);
    let crate::document::paint::images::RenderedImageSource::Stored { image_id, .. } =
        &rendered_image.source
    else {
        panic!("HTML WebP should retain a store-backed source");
    };
    let raster = document
        .image_store
        .with_rasterized(*image_id, |raster| raster)
        .expect("WebP first frame rasterizes for PDF output");
    assert_eq!(raster.rgb, vec![230, 32, 16, 0, 0, 0]);
    assert_eq!(raster.alpha, None);

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Subtype /Image"));
    assert!(rendered.contains("/FlateDecode"));
    assert!(!rendered.contains("/DCTDecode"));
}

#[tokio::test]
async fn embeds_png_alpha_as_pdf_soft_mask() {
    let html = Html::from_string(
        "<img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DQAAAEgQGALFXOsAAAAABJRU5ErkJggg==\" height=\"10\">",
    );
    let document = html.render(&RenderOptions::default()).await.unwrap();

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/SMask"));
    assert!(rendered.contains("/DeviceGray"));
    assert!(rendered.matches("/Interpolate false").count() >= 2);
}

#[tokio::test]
async fn supports_percentage_image_widths() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 300pt; margin: 10pt } body { margin: 0 } img { width: 100%; }</style><div style=\"margin:0; width:50%\"><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images()[0].width(), 90.0);
}

#[tokio::test]
async fn raster_image_natural_pixels_convert_to_css_layout_points() {
    let document = Html::from_string(format!(
        "<!doctype html><style>@page {{ size: 120pt 120pt; margin: 0 }} body {{ margin: 0 }} img {{ display: block }}</style>\
         <img src=\"{GREEN_100_PNG}\">"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .images()
        .first()
        .expect("expected green raster image paint");
    assert!((green.width() - 75.0).abs() < 0.01, "green={green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "green={green:?}");
}

#[tokio::test]
async fn vertical_writing_flex_image_uses_converted_natural_main_size() {
    let document = Html::from_string(format!(
        "<!doctype html><style>@page {{ size: 140pt 140pt; margin: 0 }} body {{ margin: 0 }} p {{ display: none }}</style>\
         <p>Test passes if there is a filled green square.</p>\
         <div style=\"writing-mode: vertical-lr; display: flex;\"><img src=\"{GREEN_100_PNG}\"></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .images()
        .first()
        .expect("expected green flex image paint");
    assert!((green.width() - 75.0).abs() < 0.01, "green={green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "green={green:?}");
}

#[tokio::test]
async fn floated_percentage_width_replays_resolved_used_width_once() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 120pt; margin: 0 } body { margin: 0 }\
         .test { float: left; width: 33.3333% }\
         p { margin: 0 10pt 0 0; height: 20pt; background: #ccc }</style>\
         <div class=\"test\"><p></p></div><div class=\"test\"><p></p></div><div class=\"test\"><p></p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(204, 204, 204)))
        .collect::<Vec<_>>();
    rects.sort_by(|a, b| a.x().total_cmp(&b.x()));

    assert_eq!(rects.len(), 3);
    for (rect, expected_x) in rects.iter().zip([0.0, 120.0, 240.0]) {
        assert!(
            (rect.x() - expected_x).abs() < 0.01,
            "expected float child at x={expected_x}, got {rect:?}"
        );
        assert!(
            (rect.width() - 110.0).abs() < 0.01,
            "float percentage width should not be resolved twice: {rect:?}"
        );
    }
}

#[tokio::test]
async fn floated_border_box_percentage_width_replay_preserves_content_width() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 0 } body { margin: 0 }\
         .float { float: left; width: 50%; box-sizing: border-box; padding: 0 10pt;\
                  border-left: 5pt solid green; border-right: 5pt solid green }\
         .fill { height: 20pt; background: blue }</style>\
         <div class=\"float\"><div class=\"fill\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let fill = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("blue child should render");

    assert!((fill.x() - 15.0).abs() < 0.01, "blue child: {fill:?}");
    assert!(
        (fill.width() - 70.0).abs() < 0.01,
        "border-box float replay should preserve the resolved content width: {fill:?}"
    );
}

#[tokio::test]
async fn floated_non_replaced_block_intrinsic_width_honors_definite_max_width() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 420pt; margin: 0 } body { margin: 0 }\
         .container { clear: both; padding: 10pt; width: 0 }\
         .infinite { width: 400pt }\
         .float { float: left; border-left: 20pt solid orange; border-right: 20pt solid orange }\
         .child { border-right: 20pt solid aqua }\
         .atom { display: inline-block; width: 80pt; height: 10pt; background: blue }</style>\
         <div class=\"container\"><div class=\"float\"><div class=\"child\"><span class=\"atom\"></span></div></div></div>\
         <div class=\"container\"><div class=\"float\"><div class=\"child\" style=\"max-width: 50%\"><span class=\"atom\"></span></div></div></div>\
         <div class=\"container\"><div class=\"float\"><div class=\"child\" style=\"max-width: calc(45pt + 0%)\"><span class=\"atom\"></span></div></div></div>\
         <div class=\"container\"><div class=\"float\"><div class=\"child\" style=\"max-width: 40pt\"><span class=\"atom\"></span></div></div></div>\
         <div class=\"container infinite\"><div class=\"float\"><div class=\"child\"><span class=\"atom\"></span></div></div></div>\
         <div class=\"container infinite\"><div class=\"float\"><div class=\"child\" style=\"max-width: 50%\"><span class=\"atom\"></span></div></div></div>\
         <div class=\"container infinite\"><div class=\"float\"><div class=\"child\" style=\"max-width: calc(45pt + 0%)\"><span class=\"atom\"></span></div></div></div>\
         <div class=\"container infinite\"><div class=\"float\"><div class=\"child\" style=\"max-width: 40pt\"><span class=\"atom\"></span></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let orange = CssColor::new(255, 165, 0);
    let mut right_borders = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(orange) && (rect.width() - 20.0).abs() < 0.01 && rect.height() > 10.0
        })
        .collect::<Vec<_>>();
    right_borders.sort_by(|a, b| b.y().total_cmp(&a.y()).then(a.x().total_cmp(&b.x())));
    let right_borders = right_borders
        .as_chunks::<2>()
        .0
        .iter()
        .map(|row| row[1].x())
        .collect::<Vec<_>>();

    assert_eq!(right_borders.len(), 8, "orange borders={right_borders:?}");
    for row in [0usize, 1, 2, 4, 5, 6] {
        assert!(
            (right_borders[row] - right_borders[0]).abs() < 0.01,
            "percentage max-width rows should keep the intrinsic float width: {right_borders:?}"
        );
    }
    for (row, reference) in [(3usize, 0usize), (7, 4)] {
        assert!(
            (right_borders[row] - (right_borders[reference] - 40.0)).abs() < 0.01,
            "definite max-width should shrink the parent float: {right_borders:?}"
        );
    }
}

#[tokio::test]
async fn min_content_block_resolves_calc_margin_percent_against_zero() {
    let document = Html::from_string(
        "<!doctype html>\
         <style>@page { size: 200pt 200pt; margin: 0 } body { margin: 0 }</style>\
         <div style=\"width: min-content; height: 100px; background: green\">\
           <div style=\"margin-left: calc(10% + 100px)\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("green min-content background should paint");

    assert!(
        (green.width() - 75.0).abs() < 0.01,
        "green square should be 100 CSS px wide: {green:?}"
    );
    assert!(
        (green.height() - 75.0).abs() < 0.01,
        "green square should be 100 CSS px tall: {green:?}"
    );
}

#[tokio::test]
async fn hidden_float_placeholders_reserve_reference_grid_cells() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>\
         @page { size: 900px 600px; margin: 0 } body { margin: 0 }\
         .flexContainer { height: 60px; width: 60px; font: 10px sans-serif;\
             background: yellow; float: left; border: 1px solid black }\
         .flexContainer > * { border: 1px dotted gray; width: 28px; height: 28px;\
             float: left }\
         .hidden { visibility: hidden }\
         </style>\
         <div class=\"flexContainer\"><div>1</div><div>2</div><div>3</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>2</div><div>1</div><div class=\"hidden\">4</div><div>3</div></div>\
         <div class=\"flexContainer\"><div>1</div><div>3</div><div>2</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>2</div><div class=\"hidden\">4</div><div>1</div><div>3</div></div>\
         <div style=\"clear:both\"></div>\
         <div class=\"flexContainer\"><div>1</div><div>2</div><div>3</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>2</div><div>1</div><div class=\"hidden\">4</div><div>3</div></div>\
         <div class=\"flexContainer\"><div>1</div><div>3</div><div>2</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>2</div><div class=\"hidden\">4</div><div>1</div><div>3</div></div>\
         <div style=\"clear:both\"></div>\
         <div class=\"flexContainer\"><div>3</div><div class=\"hidden\">4</div><div>1</div><div>2</div></div>\
         <div class=\"flexContainer\"><div class=\"hidden\">4</div><div>3</div><div>2</div><div>1</div></div>\
         <div class=\"flexContainer\"><div>3</div><div>1</div><div class=\"hidden\">4</div><div>2</div></div>\
         <div class=\"flexContainer\"><div class=\"hidden\">4</div><div>2</div><div>3</div><div>1</div></div>\
         <div style=\"clear:both\"></div>\
         <div class=\"flexContainer\"><div>3</div><div class=\"hidden\">4</div><div>1</div><div>2</div></div>\
         <div class=\"flexContainer\"><div class=\"hidden\">4</div><div>3</div><div>2</div><div>1</div></div>\
         <div class=\"flexContainer\"><div>3</div><div>1</div><div class=\"hidden\">4</div><div>2</div></div>\
         <div class=\"flexContainer\"><div class=\"hidden\">4</div><div>2</div><div>3</div><div>1</div></div>\
         <div style=\"clear:both\"></div>\
         <div class=\"flexContainer\"><div>1</div><div>2</div><div>3</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>3</div><div class=\"hidden\">4</div><div>1</div><div>2</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut containers = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .collect::<Vec<_>>();
    containers.sort_by(|a, b| {
        b.y()
            .total_cmp(&a.y())
            .then_with(|| a.x().total_cmp(&b.x()))
    });

    let expected = [
        [Some("1"), Some("2"), Some("3"), None],
        [Some("2"), Some("1"), None, Some("3")],
        [Some("1"), Some("3"), Some("2"), None],
        [Some("2"), None, Some("1"), Some("3")],
        [Some("1"), Some("2"), Some("3"), None],
        [Some("2"), Some("1"), None, Some("3")],
        [Some("1"), Some("3"), Some("2"), None],
        [Some("2"), None, Some("1"), Some("3")],
        [Some("3"), None, Some("1"), Some("2")],
        [None, Some("3"), Some("2"), Some("1")],
        [Some("3"), Some("1"), None, Some("2")],
        [None, Some("2"), Some("3"), Some("1")],
        [Some("3"), None, Some("1"), Some("2")],
        [None, Some("3"), Some("2"), Some("1")],
        [Some("3"), Some("1"), None, Some("2")],
        [None, Some("2"), Some("3"), Some("1")],
        [Some("1"), Some("2"), Some("3"), None],
        [Some("3"), None, Some("1"), Some("2")],
    ];

    assert_eq!(
        containers.len(),
        expected.len(),
        "containers={containers:?}"
    );
    for (container_index, (container, expected_cells)) in
        containers.iter().zip(expected).enumerate()
    {
        let mut actual = [None, None, None, None];
        for line in document.pages[0].lines().iter().filter(|line| {
            line.x() >= container.x() - 0.01
                && line.x() <= container.x() + container.width() + 0.01
                && line.y() >= container.y() - 0.01
                && line.y() <= container.y() + container.height() + 0.01
        }) {
            let column = if line.x() < container.x() + container.width() / 2.0 {
                0
            } else {
                1
            };
            let row = if line.y() > container.y() + container.height() / 2.0 {
                0
            } else {
                1
            };
            actual[row * 2 + column] = Some(line.text.as_str());
        }
        assert_eq!(
            actual,
            expected_cells,
            "container {container_index} should match reference grid cells: container={container:?}, lines={:?}",
            document.pages[0].lines()
        );
    }
}

#[tokio::test]
async fn direct_inline_images_reserve_baseline_descent_in_line_box() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 160pt; margin: 20pt } body, div, p { margin: 0; font-size: 12pt; line-height: 12pt } img { height: 20pt }</style>\
         <div><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"></div><p>After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let image = &document.pages[0].images()[0];
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!(image.y() - after.y() >= 12.0 - 0.01);
}

/// An auto-height block that owns only an inline replaced element still owns
/// that element's line box. The next in-flow table must therefore begin below
/// the image, not in the image's painted area.
///
/// <https://www.w3.org/TR/css-inline-3/#line-box>
/// <https://www.w3.org/TR/CSS22/visudet.html#root-height>
#[tokio::test]
async fn inline_replaced_image_advances_nested_block_before_following_table() {
    let document = Html::from_string(format!(
        "<style>
           @page {{ size: 200pt 160pt; margin: 20pt }}
           body, div, p, table {{ margin: 0; padding: 0; border-spacing: 0; font-size: 12pt; line-height: 12pt }}
           .label {{ height: 15pt; padding-bottom: 2pt }}
           .chart {{ width: 100pt }}
           .chart img {{ width: 100%; height: 20pt }}
         </style>
         <div class=\"block\"><p class=\"label\">chr1</p><div><div class=\"chart\"><img src=\"{GREEN_100_PNG}\"></div></div><table><tr><td>Status</td></tr></table></div><p class=\"label\">chr2</p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let image = page.images().first().expect("expected chart image");
    let status = page
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Status")
        .expect("expected table status");
    let next_label = page
        .lines()
        .iter()
        .find(|line| line.text.trim() == "chr2")
        .expect("expected following block label");

    assert!(
        image.y() - status.y() >= 12.0 - 0.01,
        "the table text must follow the image line box: image={image:?}, status={status:?}"
    );
    assert!(
        status.y() - next_label.y() >= 12.0 - 0.01,
        "the following block must remain after the table: status={status:?}, next_label={next_label:?}"
    );
}

/// Break avoidance must use the actual height of a block containing an inline
/// replaced element. This keeps each chart and its table on its own page.
///
/// <https://www.w3.org/TR/css-break-3/#break-within>
#[tokio::test]
async fn inline_replaced_image_height_participates_in_page_break_avoidance() {
    let block = |label: &str, status: &str| {
        format!(
            "<div class=\"block\"><p class=\"label\">{label}</p><div><div class=\"chart\"><img src=\"{GREEN_100_PNG}\"></div></div><table><tr><td>{status}</td></tr></table></div>"
        )
    };
    let document = Html::from_string(format!(
        "<style>
           @page {{ size: 160pt 90pt; margin: 15pt }}
           body, div, p, table {{ margin: 0; padding: 0; border-spacing: 0; font-size: 12pt; line-height: 12pt }}
           .block {{ break-inside: avoid }}
           .label {{ height: 15pt; padding-bottom: 2pt }}
           .chart {{ width: 100pt }}
           .chart img {{ width: 100%; height: 20pt }}
         </style>{}{}",
        block("chr1", "Status 1"),
        block("chr2", "Status 2"),
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2, "document={document:?}");
    for (page, label, status) in [
        (&document.pages[0], "chr1", "Status 1"),
        (&document.pages[1], "chr2", "Status 2"),
    ] {
        let image = page.images().first().expect("expected chart image");
        let label = page
            .lines()
            .iter()
            .find(|line| line.text.trim() == label)
            .expect("expected chart label");
        let status = page
            .lines()
            .iter()
            .find(|line| line.text.trim() == status)
            .expect("expected table status");

        assert_eq!(page.images().len(), 1, "page={page:?}");
        assert!(label.y() > image.y(), "label={label:?}, image={image:?}");
        assert!(
            image.y() - status.y() >= 12.0 - 0.01,
            "the table text must follow the image line box: image={image:?}, status={status:?}"
        );
    }
}

#[tokio::test]
async fn anonymous_inline_runs_layout_replaced_atoms_with_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 160pt; margin: 20pt } body, div, p { margin: 0; font-size: 12pt; line-height: 12pt } img { width: 10pt; height: 10pt }</style>\
         <div>Before <img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"> After<p>Block</p></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let image = &document.pages[0].images()[0];
    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(image.x() > before.x());
    assert!(after.x() > image.x());
    assert!((before.y() - after.y()).abs() < 0.1);
}

#[tokio::test]
async fn inline_formatting_context_places_atomic_image_between_text_fragments() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 20pt } body, div { margin: 0; font-size: 12pt; line-height: 12pt } img { width: 10pt; height: 10pt }</style>\
         <div>Before <img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"> After</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let image = &document.pages[0].images()[0];
    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(image.x() > before.x());
    assert!(after.x() > image.x());
    assert!((before.y() - after.y()).abs() < 0.1);
}

#[tokio::test]
async fn flex_replaced_images_use_border_box_for_flex_distribution() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 260pt 100pt; margin: 10pt }} body {{ margin: 0 }} .flex {{ width: 200pt; display: flex; line-height: 8pt }} img {{ min-width: 0; width: 10pt; height: 20pt; border: 1pt dotted green }}</style>\
         <div class=\"flex\"><img src=\"{image}\" style=\"flex: 5\"><img src=\"{image}\" style=\"flex: 3\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = &document.pages[0].images();
    assert_eq!(images.len(), 2);
    assert!((images[0].width() - 122.5).abs() < 0.01);
    assert!((images[1].width() - 73.5).abs() < 0.01);
    assert!((images[1].x() - images[0].x() - 124.5).abs() < 0.01);
}

#[tokio::test]
async fn column_flex_replaced_image_min_height_transfers_through_aspect_ratio() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 180pt; margin: 10pt }} body {{ margin:0 }}\
         .flex {{ display:flex; flex-direction:column; align-items:flex-start; width:75pt; height:150pt }}\
         img {{ min-height:75pt; flex:1 0 auto }} .spacer {{ flex:1 0 1pt }}\
         </style><div class=\"flex\"><img src=\"{image}\"><div class=\"spacer\"></div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let image = &document.pages[0].images()[0];
    assert!((image.width() - 75.0).abs() < 0.01, "image={image:?}");
    assert!((image.height() - 75.0).abs() < 0.01, "image={image:?}");
}

#[tokio::test]
async fn flex_container_aspect_ratio_height_is_definite_for_stretched_image() {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 460pt 430pt; margin: 0 }} body {{ margin:0 }}\
         .flex {{ display:flex; width:400pt }}\
         .ratio {{ aspect-ratio:2/1 }}\
         .explicit {{ height:200pt }}\
         img {{ display:block }}\
         </style>\
         <div class=\"flex ratio\"><img src=\"{GREEN_100_PNG}\"></div>\
         <div class=\"flex explicit\"><img src=\"{GREEN_100_PNG}\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0].images();
    assert_eq!(images.len(), 2, "images={images:?}");
    for image in images {
        assert!((image.width() - 200.0).abs() < 0.01, "image={image:?}");
        assert!((image.height() - 200.0).abs() < 0.01, "image={image:?}");
    }
}

#[tokio::test]
async fn row_flex_replaced_image_authored_aspect_ratio_sets_auto_cross_size() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAAFCAYAAABvsz2cAAAAFElEQVR4nGNg+A+EDUAMJkAQtwgAfnURcnh7KuYAAAAASUVORK5CYII=";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 200px 200px; margin: 0 }} body {{ margin:0 }}\
         div {{ display:flex; width:100px }}\
         img {{ width:50px; aspect-ratio:1/1; flex:1; min-height:0 }}\
         </style><div><img src=\"{image}\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let has_square_rect = page
        .rects()
        .iter()
        .any(|rect| (rect.width() - 75.0).abs() < 0.01 && (rect.height() - 75.0).abs() < 0.01);
    let has_square_image = page
        .images()
        .iter()
        .any(|image| (image.width() - 75.0).abs() < 0.01 && (image.height() - 75.0).abs() < 0.01);
    assert!(
        has_square_rect || has_square_image,
        "expected 100px by 100px painted image, rects={:?}, images={:?}",
        page.rects(),
        page.images()
    );
}

#[tokio::test]
async fn column_flex_replaced_image_cross_min_width_transfers_to_main_basis() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAAFCAYAAABvsz2cAAAAFElEQVR4nGNg+A+EDUAMJkAQtwgAfnURcnh7KuYAAAAASUVORK5CYII=";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 260pt 160pt; margin: 10pt }} body {{ margin:0 }}\
         .flex {{ display:flex; flex-direction:column; align-items:flex-start; width:200pt }}\
         img {{ min-width:20%; min-height:0 }}\
         </style><div class=\"flex\"><img src=\"{image}\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let image = &document.pages[0].images()[0];
    assert!((image.width() - 40.0).abs() < 0.01, "image={image:?}");
    assert!((image.height() - 100.0).abs() < 0.01, "image={image:?}");
}

#[tokio::test]
async fn inline_floated_image_is_removed_from_text_flow_and_shifted_right() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } div { width: 200pt; height: 22pt } img { width: 10pt; height: 20pt; border: 1pt dotted green; float: right }</style>\
         <div>some words <img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "some words")
        .unwrap();
    let image = &document.pages[0].images()[0];
    assert!((text.x() - 10.0).abs() < 0.01);
    assert!((image.x() - 199.0).abs() < 0.01);
}

#[tokio::test]
async fn inline_block_before_right_float_stays_on_same_line() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>\
         @page { size: 900px 600px; margin: 0 }\
         body { margin: 0 }\
         .container { width: 100px; height: 100px; background-color: red }\
         .inline-block { display: inline-block; width: 50px; height: 100px; background-color: green }\
         .float-right { float: right; width: 50px; height: 100px; background-color: green }\
         .float-left { width: 30px; height: 50px; clear: both; float: left }\
         </style>\
         <div class=\"container\"><div class=\"inline-block\"></div><div class=\"float-right\"></div><div class=\"float-left\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("container background should paint");
    assert!((red.width() - 75.0).abs() < 0.01, "red={red:?}");
    assert!((red.height() - 75.0).abs() < 0.01, "red={red:?}");

    let mut green = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    green.sort_by(|left, right| left.x().total_cmp(&right.x()));

    assert_eq!(green.len(), 2, "green rects={green:?}");
    let left = green[0];
    let right = green[1];
    for rect in [left, right] {
        assert!((rect.width() - 37.5).abs() < 0.01, "green={rect:?}");
        assert!((rect.height() - 75.0).abs() < 0.01, "green={rect:?}");
        assert!(
            (rect.y() - red.y()).abs() < 0.01,
            "red={red:?} green={rect:?}"
        );
    }
    assert!(
        (left.x() - red.x()).abs() < 0.01,
        "left={left:?} red={red:?}"
    );
    assert!(
        ((right.x() + right.width()) - (red.x() + red.width())).abs() < 0.01,
        "right={right:?} red={red:?}"
    );
}

/// A non-fragmentable atomic inline that cannot fit in the residual band beside
/// a float moves to the first later slab where its complete margin box fits.
/// CSS 2.2 §9.5 requires this line-box retry before `text-align` is applied.
#[tokio::test]
async fn atomic_inline_moves_below_float_when_shortened_line_cannot_contain_it() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>\
         @page { size: 200px 120px; margin: 0 }\
         body { margin: 0 }\
         .outer { float: left; width: 4px; background: red }\
         .float { float: right; width: 50px; height: 20px; background: orange }\
         .center-parent { width: 100px; margin-left: -48px; text-align: center; background: yellow }\
         .atom { display: inline-block; width: 50px; height: 10px; background: lime }\
         </style>\
         <div class=\"outer\"><div class=\"float\"></div><div class=\"center-parent\"><div class=\"atom\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let float = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 165, 0)))
        .expect("right float should paint");
    let parent = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("center parent should paint");
    let atom = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 255, 0)))
        .expect("atomic inline should paint");

    assert!(
        atom.y() + atom.height() <= float.y() + 0.01,
        "atomic inline must move below the constraining float: float={float:?}, parent={parent:?}, atom={atom:?}"
    );
    assert!(
        ((atom.x() + atom.width() * 0.5) - (parent.x() + parent.width() * 0.5)).abs() < 0.01,
        "the retried line must be centered in the parent's full measure: parent={parent:?}, atom={atom:?}"
    );
}

/// An atomic inline that does fit in the shortened band remains beside its
/// preceding float; CSS 2.2's retry is not an unconditional float clear.
#[tokio::test]
async fn fitting_atomic_inline_remains_beside_float() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>\
         @page { size: 200px 120px; margin: 0 }\
         body { margin: 0 }\
         .outer { float: left; width: 4px; background: red }\
         .float { float: right; width: 50px; height: 20px; background: orange }\
         .center-parent { width: 100px; margin-left: -48px; text-align: center; background: yellow }\
         .atom { display: inline-block; width: 10px; height: 10px; background: lime }\
         </style>\
         <div class=\"outer\"><div class=\"float\"></div><div class=\"center-parent\"><div class=\"atom\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let float = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 165, 0)))
        .expect("right float should paint");
    let atom = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 255, 0)))
        .expect("atomic inline should paint");

    assert!(
        atom.y() + atom.height() > float.y() + 0.01,
        "a fitting atomic inline must remain in the shortened band: float={float:?}, atom={atom:?}"
    );
}

/// A parent float shortens the outer centered line once, but cannot become an
/// exclusion while intrinsic sizing the atomic inline-block that line contains.
/// CSS 2.2 §9.4.1 gives inline-blocks an independent BFC.
#[tokio::test]
async fn inline_block_intrinsic_layout_does_not_inherit_parent_float_clearance() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>\
         @page { size: 240px 500px; margin: 0 }\
         body { margin: 0; font-family: sans-serif; font-size: 10px }\
         .outer { float: left; width: 4px }\
         .float, .atom { clear: both; margin: 1px 2px 3px 4px; border-width: 2px 3px 4px 5px; border-style: solid; padding: 3px 4px 5px 6px }\
         .left { float: left; background: red }\
         .right { float: right; background: orange }\
         .big { font-size: 18px; width: 50px }\
         .center-parent { width: 100px; margin-left: -48px; text-align: center }\
         .control-parent { clear: both; width: 100px; text-align: center }\
         .atom { display: inline-block; text-align: left }\
         #first { background: rgb(10, 20, 30) }\
         #second { background: rgb(40, 50, 60) }\
         #control { background: rgb(70, 80, 90) }\
         </style>\
         <div class=\"outer\">\
           <div class=\"float left\">start</div><div class=\"float left big\">a b</div>\
           <div class=\"float right\">end</div><div class=\"float right big\">a b</div>\
           <div class=\"center-parent\"><span id=\"first\" class=\"atom\">center</span></div>\
           <div class=\"center-parent\"><span id=\"second\" class=\"atom\">center</span></div>\
         </div>\
         <div class=\"control-parent\"><span id=\"control\" class=\"atom\">center</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect_with_fill = |fill| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(fill))
            .expect("expected colored atomic inline-block background")
    };
    let first = rect_with_fill(CssColor::new(10, 20, 30));
    let second = rect_with_fill(CssColor::new(40, 50, 60));
    let control = rect_with_fill(CssColor::new(70, 80, 90));

    assert!(
        (first.height() - control.height()).abs() < 0.01,
        "the parent float must not inflate the first atom's intrinsic height: first={first:?}, control={control:?}"
    );
    assert!(
        (second.height() - control.height()).abs() < 0.01,
        "the parent float must not inflate the second atom's intrinsic height: second={second:?}, control={control:?}"
    );
    // The atom's authored 1px + 3px vertical margins are 3pt in the PDF
    // coordinate system, so consecutive parent lines advance by its margin box.
    let first_margin_box_height = first.height() + 3.0;
    assert!(
        ((first.y() - second.y()).abs() - first_margin_box_height).abs() < 0.01,
        "the second in-flow atom must begin after normal single-line progression, without an additional parent-float clearance: first={first:?}, second={second:?}"
    );
}

#[tokio::test]
async fn zero_height_float_does_not_shorten_same_top_line_box() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 160px 140px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"width: 100px; height: 100px; background: red; position: relative;\">\
           <div style=\"float: left; width: 20px;\"></div>\
           <div style=\"line-height: 0;\">\
             <div style=\"display: inline-block; width: 100px; height: 20px; background: green;\"></div>\
           </div>\
           <div style=\"float: right; width: 20px; height: 80px; background: green;\"></div>\
           <div style=\"float: right; clear: right; width: 30px; clear: right;\"></div>\
           <div style=\"display: inline-block; width: 60px; height: 60px; background: green;\"></div>\
           <div style=\"position: absolute; width: 20px; height: 80px; background: green; top: 20px; right: 20px;\"></div>\
           <div style=\"position: absolute; width: 60px; height: 20px; background: green; bottom: 0; left: 0;\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("container background should paint");
    assert!((red.width() - 75.0).abs() < 0.01, "red={red:?}");
    assert!((red.height() - 75.0).abs() < 0.01, "red={red:?}");

    let top_strip = page
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - 75.0).abs() < 0.01
                && (rect.height() - 15.0).abs() < 0.01
        })
        .expect("top green strip should paint");
    assert!(
        (top_strip.x() - red.x()).abs() < 0.01,
        "zero-height left float should not offset the top strip: top={top_strip:?} red={red:?}"
    );
    let lower_left = page
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - 45.0).abs() < 0.01
                && (rect.height() - 45.0).abs() < 0.01
        })
        .expect("lower-left inline-block should paint");
    assert!(
        (lower_left.x() - red.x()).abs() < 0.01,
        "lower={lower_left:?}"
    );
    assert!(
        (lower_left.y() - (red.y() + 15.0)).abs() < 0.01,
        "lower-left inline-block should occupy the lower-left band: lower={lower_left:?} red={red:?}"
    );
}

#[tokio::test]
async fn inline_block_intrinsic_width_uses_own_definite_height_for_percentage_canvas() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <title>Intrinsic size of an atomic inline with an anonymous block</title>\
         <style>\
         @page { size: 300px 220px; margin: 0 }\
         body { margin: 0 }\
         #test {\
           display: inline-block;\
           height: 100px;\
           background: green;\
         }\
         #test > canvas {\
           height: 100%;\
           background: red;\
           position: relative;\
           z-index: -1;\
         }\
         </style>\
         <div id=\"test\"><canvas width=\"10\" height=\"10\"></canvas><p></p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("inline-block background should paint");
    assert!((green.width() - 75.0).abs() < 0.01, "green={green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "green={green:?}");

    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("canvas background should paint behind green");
    assert!((red.width() - 75.0).abs() < 0.01, "red={red:?}");
    assert!((red.height() - 75.0).abs() < 0.01, "red={red:?}");
    assert!(
        first_rect_paint_operation_index(page, CssColor::new(255, 0, 0))
            < first_rect_paint_operation_index(page, CssColor::new(0, 128, 0)),
        "the negative-z canvas must paint before the inline-block's in-flow background: {:?}",
        page.paint_operations()
    );
}

#[tokio::test]
async fn abspos_intrinsic_width_uses_own_definite_height_for_percentage_canvas() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 300px 220px; margin: 0 }\
         body { margin: 0 }\
         #container { position: relative; height: 200px }\
         #abs { position: absolute; top: 0; bottom: 100px; background: green }\
         canvas { height: 100% }\
         </style>\
         <div id=\"container\"><div id=\"abs\"><canvas width=\"10\" height=\"10\"></canvas></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolute positioned background should paint");
    assert!((green.width() - 75.0).abs() < 0.01, "green={green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "green={green:?}");
}

#[tokio::test]
async fn orthogonal_abspos_canvas_static_top_keeps_physical_top_edge() {
    for writing_mode in ["vertical-lr", "vertical-rl"] {
        let document = Html::from_string(format!(
            "<!DOCTYPE html>\
             <style>\
             @page {{ size: 300px 220px; margin: 0 }}\
             body {{ margin: 0 }}\
             #container {{ position: relative; width: 200px; height: 200px }}\
             #abs {{ writing-mode: {writing_mode}; position: absolute; left: 0; right: 100px; background: green }}\
             canvas {{ width: 100% }}\
             </style>\
             <div id=\"container\"><div id=\"abs\"><canvas width=\"10\" height=\"10\"></canvas></div></div>",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let green = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .unwrap_or_else(|| {
                panic!("absolute positioned background should paint in {writing_mode}")
            });
        assert!(
            (green.width() - 75.0).abs() < 0.01,
            "green={green:?} writing-mode={writing_mode}"
        );
        assert!(
            (green.height() - 75.0).abs() < 0.01,
            "green={green:?} writing-mode={writing_mode}"
        );
        assert!(
            (green.y() + green.height() - page.height()).abs() < 0.01,
            "orthogonal abspos should keep its static top edge: green={green:?} writing-mode={writing_mode}"
        );
    }
}

#[tokio::test]
async fn orthogonal_abspos_block_content_static_top_keeps_physical_top_edge() {
    for writing_mode in ["vertical-lr", "vertical-rl"] {
        let document = Html::from_string(format!(
            "<!DOCTYPE html>\
             <style>\
             @page {{ size: 300px 220px; margin: 0 }}\
             body {{ margin: 0 }}\
             #container {{ position: relative; width: 200px; height: 200px }}\
             #abs {{ writing-mode: {writing_mode}; position: absolute; left: 0; right: 100px; height: 100px; background: green }}\
             #abs > div {{ width: 40px; height: 40px }}\
             </style>\
             <div id=\"container\"><div id=\"abs\"><div></div></div></div>",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let green = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .unwrap_or_else(|| {
                panic!("absolute positioned background should paint in {writing_mode}")
            });
        assert!(
            (green.width() - 75.0).abs() < 0.01,
            "green={green:?} writing-mode={writing_mode}"
        );
        assert!(
            (green.height() - 75.0).abs() < 0.01,
            "green={green:?} writing-mode={writing_mode}"
        );
        assert!(
            (green.y() + green.height() - page.height()).abs() < 0.01,
            "orthogonal abspos block content should keep its static top edge: green={green:?} writing-mode={writing_mode}"
        );
    }
}

#[tokio::test]
async fn auto_height_inline_block_masks_ancestor_height_for_percentage_canvas() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 300px 220px; margin: 0 }\
         body { margin: 0 }\
         #outer { height: 100px }\
         #test { display: inline-block; background: green }\
         #test > canvas { height: 100%; background: red }\
         </style>\
         <div id=\"outer\"><div id=\"test\"><canvas width=\"10\" height=\"10\"></canvas><p></p></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("canvas background should paint");
    assert!(
        (red.width() - 7.5).abs() < 0.01,
        "auto-height inline-block should not provide the ancestor's height as canvas percentage basis: {red:?}"
    );
    assert!(
        (red.height() - 7.5).abs() < 0.01,
        "auto-height inline-block should leave percentage canvas height unresolved: {red:?}"
    );
}

#[tokio::test]
async fn overflow_scroll_float_preserves_later_inline_block_paint_order() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <title>Overflow:scroll floating paint order</title>\
         <style>\
         @page { size: 300px 200px; margin: 0 }\
         body { margin: 0 }\
         #scroller {\
           float: left;\
           background: red;\
           padding: 20px;\
           box-sizing: border-box;\
           width: 100px;\
           height: 100px;\
           overflow: scroll;\
         }\
         #negative-margin {\
           float: left;\
           width: 100px;\
           height: 100px;\
           background: green;\
           margin-left: -100px;\
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
    let blue = first_rect_paint_operation_index(page, CssColor::new(0, 0, 255));
    let green = first_rect_paint_operation_index(page, CssColor::new(0, 128, 0));
    let magenta = first_rect_paint_operation_index(page, CssColor::new(255, 0, 255));

    assert!(
        red < yellow,
        "scroller background should paint before child background"
    );
    assert!(
        yellow < blue,
        "scroller child background should paint before foreground1"
    );
    assert!(
        blue < green,
        "earlier float contents should paint before later float background"
    );
    assert!(
        green < magenta,
        "later float background should paint before foreground2"
    );
    assert_eq!(
        final_rect_fill_at(page, 10.0, 140.0),
        Some(CssColor::new(255, 0, 255))
    );
}

#[tokio::test]
async fn inline_text_after_left_float_uses_float_exclusion_and_line_end_tracking() {
    let document = Html::from_string(
        "<style>\
         @page { size: 300pt 160pt; margin: 20pt }\
         body, div { margin: 0 }\
         div { font-family: monospace; font-size: 30pt; line-height: 30pt }\
         span { float: left; letter-spacing: 1ch }\
         </style>\
         <div>12345</div><div><span>aa</span>a</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    let ruler = lines.iter().find(|line| line.text == "12345").unwrap();
    let floated = lines.iter().find(|line| line.text == "aa").unwrap();
    let following = lines
        .iter()
        .filter(|line| line.text == "a")
        .max_by(|left, right| {
            left.x()
                .partial_cmp(&right.x())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();
    let ch = rendered_line_advance(ruler) / 5.0;

    assert!((floated.x() - ruler.x()).abs() < 0.01);
    assert!(
        (following.x() - (ruler.x() + ch * 3.0)).abs() < ch * 0.2,
        "expected following a under the fourth ruler column: ruler x={}, ch={}, following x={}",
        ruler.x(),
        ch,
        following.x()
    );
}

#[tokio::test]
async fn block_image_avoids_active_left_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 30pt; height: 30pt; background: green }\
         img { display: block; width: 20pt; height: 10pt }</style>\
         <div class=\"float\"></div><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\">",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let image = &document.pages[0].images()[0];
    assert!(
        image.x() >= 39.0,
        "block image should avoid active float: {image:?}"
    );
}

#[tokio::test]
async fn overflow_hidden_bfc_border_box_avoids_left_float_in_rtl() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 120px 120px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"width: 100px; height: 100px; background: red; direction: rtl;\">\
           <div style=\"float: left; width: 50px; height: 100px; background: green;\"></div>\
           <div style=\"overflow: hidden; height: 100px; margin-left: -20px; background: green;\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("container background should paint");
    assert!((red.width() - 75.0).abs() < 0.01, "red={red:?}");
    assert!((red.height() - 75.0).abs() < 0.01, "red={red:?}");

    let mut green_rects = page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0)) && (rect.height() - 75.0).abs() < 0.01
        })
        .collect::<Vec<_>>();
    green_rects.sort_by(|left, right| left.x().total_cmp(&right.x()));

    assert_eq!(green_rects.len(), 2, "green rects={green_rects:?}");
    let left_green = green_rects[0];
    let right_green = green_rects[1];
    assert!((left_green.x() - red.x()).abs() < 0.01, "{left_green:?}");
    assert!((left_green.width() - 37.5).abs() < 0.01, "{left_green:?}");
    assert!((left_green.y() - red.y()).abs() < 0.01, "{left_green:?}");
    assert!(
        (right_green.x() - (red.x() + 37.5)).abs() < 0.01,
        "{right_green:?}"
    );
    assert!((right_green.width() - 37.5).abs() < 0.01, "{right_green:?}");
    assert!((right_green.y() - red.y()).abs() < 0.01, "{right_green:?}");

    for x in [red.x() + 18.75, red.x() + 56.25] {
        assert_eq!(
            final_rect_fill_at(page, x, red.y() + 37.5),
            Some(CssColor::new(0, 128, 0)),
            "sample at x={x} should be green"
        );
    }
}

#[tokio::test]
async fn overflow_hidden_bfc_reflows_to_avoid_later_float_overlap() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 300px 180px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"overflow: hidden\">\
           <div style=\"width: 300px; height: 100px; margin-left: -200px; background: red\">\
             <div style=\"float: left; clear: left; width: 100px; height: 25px\"></div>\
             <div style=\"float: left; clear: left; width: 200px; height: 25px\"></div>\
             <div style=\"overflow: hidden; background: green\">\
               <div style=\"float: left; width: 100px; height: 50px\"></div>\
               <div style=\"float: left; width: 100px; height: 50px\"></div>\
             </div>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("BFC background should paint");

    assert!((green.x() - 0.0).abs() < 0.01, "green={green:?}");
    assert!(
        (green.y() + green.height() - page.height()).abs() < 0.01,
        "BFC should start in the first float-free band, not leave red exposed above it: {green:?}"
    );
    assert!(
        (green.width() - 75.0).abs() < 0.01,
        "BFC should narrow to the final 100px float band: {green:?}"
    );
    assert!(
        (green.height() - 75.0).abs() < 0.01,
        "BFC auto height should reflect its internal floats after narrowing: {green:?}"
    );
    for y in [5.0, 37.5, 70.0] {
        assert_eq!(
            final_rect_fill_at(page, 37.5, green.y() + y),
            Some(CssColor::new(0, 128, 0)),
            "visible BFC area should be green at y={y}: {green:?}"
        );
    }
}

#[tokio::test]
async fn bfc_adjoining_float_top_margin_pulls_float_down_when_it_fits() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 500px 360px; margin: 0 }</style>\
         <p>Test passes if there is a filled green square and <strong>no red</strong>.</p>\
         <div style=\"overflow:hidden; width:200px; background:green;\">\
           <div style=\"width:300px; margin-top:50px; background:red;\">\
             <div>\
               <div style=\"float:left; width:200px; height:10px; background:green;\"></div>\
             </div>\
             <div style=\"margin-top:190px; overflow:hidden; width:100px; height:10px; background:red;\"></div>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let bfc_background = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .max_by(|left, right| left.height().partial_cmp(&right.height()).unwrap())
        .expect("BFC background should paint");
    assert!(
        (bfc_background.width() - 150.0).abs() < 0.01
            && (bfc_background.height() - 150.0).abs() < 0.01,
        "the BFC should retain the 200px-wide, 200px-tall used box: {bfc_background:?}"
    );
    for x in [24.0, 74.0, 149.0] {
        for y in [
            bfc_background.y() + 24.0,
            bfc_background.y() + 74.0,
            bfc_background.y() + 124.0,
        ] {
            assert_eq!(
                final_rect_fill_at(page, x, y),
                Some(green),
                "sample at ({x}, {y}) should be green; rects={:?} operations={:?}",
                page.rects(),
                page.paint_operations()
            );
        }
    }
}

#[tokio::test]
async fn clear_both_moves_block_image_below_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 40pt; height: 20pt; background: green }\
         img { display: block; clear: both; width: 10pt; height: 10pt }</style>\
         <div class=\"float\"></div><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\">",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let image = &document.pages[0].images()[0];

    assert!(
        image.y() + image.height() <= green.y() + 0.01,
        "clear block image should start below float: green={green:?} image={image:?}"
    );
}

#[tokio::test]
async fn overwide_block_image_moves_below_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 30pt; height: 20pt; background: green }\
         img { display: block; width: 90pt; height: 10pt }</style>\
         <div class=\"float\"></div><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\">",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let image = &document.pages[0].images()[0];

    assert!((image.x() - 10.0).abs() < 0.01, "image={image:?}");
    assert!(
        image.y() + image.height() <= green.y() + 0.01,
        "overwide block image should move below float: green={green:?} image={image:?}"
    );
}

#[tokio::test]
async fn block_canvas_and_svg_avoid_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body, canvas, svg { margin: 0 }\
         .float { float: left; width: 30pt; height: 40pt; background: green }\
         canvas { display: block; width: 20pt; height: 10pt; background: blue }\
         svg { display: block; width: 20pt; height: 10pt }</style>\
         <div class=\"float\"></div><canvas></canvas><svg><rect width=\"20pt\" height=\"10pt\" fill=\"#ff0000\"></rect></svg>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();
    let red = document.pages[0]
        .paths()
        .iter()
        .find(|path| path.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let red_bounds = red.paint_bounds().unwrap();

    assert!(
        blue.x() >= 39.0,
        "block canvas should avoid active float: {blue:?}"
    );
    assert!(
        red_bounds.origin.x >= 39.0,
        "block svg should avoid active float: {red_bounds:?}"
    );
}

#[tokio::test]
async fn flow_root_auto_height_expands_to_contain_internal_float() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 140pt; margin: 10pt }\
         body, div { margin: 0 }\
         .root { display: flow-root; width: 100pt; background: rgb(0 128 0) }\
         .float { float: left; width: 30pt; height: 40pt; background: rgb(0 0 255) }\
         </style>\
         <div class=\"root\"><div class=\"float\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let root = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        root.height() >= 39.99,
        "flow-root background should include its internal float: {root:?}"
    );
}

#[tokio::test]
async fn internal_flow_root_float_does_not_leak_to_following_sibling() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body, div { margin: 0 }\
         .root { display: flow-root; width: 100pt; background: rgb(0 128 0) }\
         .float { float: left; width: 30pt; height: 40pt; background: rgb(0 0 255) }\
         .after { width: 100pt; height: 10pt; background: rgb(255 0 0) }\
         </style>\
         <div class=\"root\"><div class=\"float\"></div></div><div class=\"after\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let root = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let after = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        after.y() + after.height() <= root.y() + 0.01,
        "following sibling should start below the flow-root: root={root:?} after={after:?}"
    );
}

#[tokio::test]
async fn renders_simple_svg_rects_in_table_cells() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:100pt\"><tr><td><svg width=\"15pt\" height=\"15pt\"><rect width=\"15pt\" height=\"15pt\" fill=\"#2292d4\"></rect></svg></td><td>Half Match</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(document.pages[0].paths().iter().any(|path| {
        let Some(bounds) = path.paint_bounds() else {
            return false;
        };
        bounds.size.width == 15.0
            && bounds.size.height == 15.0
            && path.fill == Some(CssColor::new(34, 146, 212))
    }));
    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("Half"));
    assert!(text.contains("Match"));
}

#[tokio::test]
async fn renders_uri_link_annotations() {
    let document = Html::from_string("<a href=\"https://example.com\">Example</a>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].links().len(), 1);
    assert_eq!(
        document.pages[0].links()[0].target.as_ref(),
        "https://example.com"
    );

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.contains("/Subtype /Link"));
    assert!(rendered.contains("/URI (https://example.com)"));
    assert!(rendered.contains("/Annots ["));
}

#[tokio::test]
async fn draws_text_decorations() {
    let document = Html::from_string(
        "<p style=\"margin: 0; color: red; text-decoration: underline line-through\">Decorated</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages[0].rects().len() >= 2);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
    );
}

#[tokio::test]
async fn vertical_filled_text_emphasis_adds_sesame_marks() {
    let document = Html::from_string(
        "<div style=\"writing-mode: vertical-rl; text-emphasis-style: filled\">試験テスト</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"試験テスト"), "{lines:?}");
    assert_eq!(
        lines.iter().filter(|line| **line == "\u{FE45}").count(),
        5,
        "{lines:?}"
    );
}

#[tokio::test]
async fn preserves_basic_styled_inline_runs() {
    let document = Html::from_string(
        "<p style=\"margin:0;font-size:12pt\">A <em>italic</em> <strong>bold</strong> <small>small</small> ref<sup>1</sup></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert!(
        lines
            .iter()
            .any(|line| line_run_font_is_italic(&document, line, "italic"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line_run_font_is_bold(&document, line, "bold"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.text == "small" && (line.font_size - 10.0).abs() < 0.01)
    );

    let reference = lines.iter().find(|line| line.text == "ref").unwrap();
    let superscript = lines.iter().find(|line| line.text == "1").unwrap();
    assert!(superscript.font_size < reference.font_size);
    assert!(superscript.y() > reference.y());
}

#[tokio::test]
async fn supports_authored_vertical_align_super_and_sub() {
    let document = Html::from_string(
        "<p style=\"margin:0;font-size:12pt\">Base<span style=\"vertical-align: super; font-size: 9pt\">up</span><span style=\"vertical-align: sub; font-size: 9pt\">down</span><sup style=\"vertical-align: baseline\">flat</sup></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    let base = lines.iter().find(|line| line.text == "Base").unwrap();
    let up = lines.iter().find(|line| line.text == "up").unwrap();
    let down = lines.iter().find(|line| line.text == "down").unwrap();
    let flat = lines.iter().find(|line| line.text == "flat").unwrap();

    assert!(up.y() > base.y());
    assert!(down.y() < base.y());
    assert!(flat.y() < up.y());
}

#[tokio::test]
async fn regular_inline_top_and_bottom_align_descendant_text_to_line_edges() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 160pt; margin: 20pt }\
         body, p { margin: 0 }\
         .small { font: 10pt/10pt sans-serif }\
         .big { font: 20pt/20pt sans-serif }\
         </style>\
         <p class=\"small\">small<span class=\"big\" style=\"vertical-align:top\">top</span></p>\
         <p class=\"big\">big<span class=\"small\" style=\"vertical-align:bottom\">bottom</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    let line = |text: &str| {
        lines
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("expected rendered line {text:?}: {lines:?}"))
    };
    let small = line("small");
    let top = line("top");
    let big = line("big");
    let bottom = line("bottom");

    assert!(
        top.y() < small.y() - 4.0,
        "top-aligned descendant should sit above the small baseline sibling: top={top:?}, small={small:?}"
    );
    assert!(
        bottom.y() < big.y() - 2.0,
        "bottom-aligned descendant should sit below the big baseline sibling: bottom={bottom:?}, big={big:?}"
    );
}

#[tokio::test]
async fn nested_regular_inline_top_and_bottom_scopes_are_independent() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 20pt }\
         body, p { margin: 0 }\
         .small { font: 10pt/10pt sans-serif }\
         .big { font: 20pt/20pt sans-serif }\
         </style>\
         <p class=\"small\">peer<span class=\"big\" style=\"vertical-align:top\">outer<span class=\"small\" style=\"vertical-align:bottom\">inner</span></span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    let line = |text: &str| {
        lines
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("expected rendered line {text:?}: {lines:?}"))
    };
    let peer = line("peer");
    let outer = line("outer");
    let inner = line("inner");

    assert!(outer.y() < peer.y() - 4.0);
    assert!(
        inner.y() < outer.y() - 2.0,
        "nested bottom scope must not inherit the outer top placement: inner={inner:?}, outer={outer:?}"
    );
}

#[tokio::test]
async fn inline_block_overflow_baseline_uses_css22_fallback_edges() {
    let document = Html::from_string(
        "<style>\
         @page { size: 360pt 360pt; margin: 20pt }\
         body { margin: 0; font: 10pt/10pt sans-serif }\
         .row { margin: 0 0 6pt 0 }\
         .outer { display: inline-block; width: 50pt; padding-bottom: 20pt; vertical-align: baseline }\
         .empty { height: 30pt; margin-bottom: 12pt; padding-bottom: 0; background: rgb(255 0 0) }\
         .empty > div { height: 30pt }\
         .block-text { background: rgb(0 128 0) }\
         .block-text > div { width: 30pt; height: 30pt; overflow: hidden }\
         .self-hidden { height: 30pt; margin-bottom: 12pt; padding-bottom: 0; overflow: hidden; background: rgb(255 0 255) }\
         </style>\
         <div class=\"row\"><div class=\"outer empty\"><div></div></div>EMPTY</div>\
         <div class=\"row\"><div class=\"outer block-text\"><div>INNER</div></div>BLOCK</div>\
         <div class=\"row\"><div class=\"outer self-hidden\">SELF</div>SELFREF</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let line = |text: &str| {
        page.lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("expected rendered line {text:?}: {:?}", page.lines()))
    };
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("expected rect {color:?}: {:?}", page.rects()))
    };

    let empty_outer = rect(CssColor::new(255, 0, 0));
    assert!(
        (line("EMPTY").y() - (empty_outer.y() - 12.0)).abs() < 0.5,
        "inline-block with no in-flow line boxes should use bottom margin edge: rect={empty_outer:?}, line={:?}",
        line("EMPTY")
    );

    assert!(
        (line("BLOCK").y() - line("INNER").y()).abs() < 0.5,
        "visible-overflow inline-block should keep using its last in-flow text baseline: inner={:?}, block={:?}",
        line("INNER"),
        line("BLOCK")
    );

    let self_hidden = rect(CssColor::new(255, 0, 255));
    assert!(
        (line("SELFREF").y() - (self_hidden.y() - 12.0)).abs() < 0.5,
        "overflow:hidden inline-block should ignore its internal text baseline and use bottom margin edge: rect={self_hidden:?}, line={:?}",
        line("SELFREF")
    );
}

#[tokio::test]
async fn vertical_align_length_and_percentage_shift_inline_baselines() {
    let document = Html::from_string(
        "<p style=\"margin:0;font-size:20pt;line-height:20pt\">\
         Base<span style=\"vertical-align:10pt\">up</span>\
         <span style=\"vertical-align:-10pt\">down</span>\
         <span style=\"vertical-align:50%;line-height:20pt\">pct</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    let base = lines.iter().find(|line| line.text == "Base").unwrap();
    let up = lines.iter().find(|line| line.text == "up").unwrap();
    let down = lines.iter().find(|line| line.text == "down").unwrap();
    let pct = lines.iter().find(|line| line.text == "pct").unwrap();

    assert!(
        up.y() > base.y() + 9.0,
        "positive vertical-align length should raise the inline box: base={base:?}, up={up:?}"
    );
    assert!(
        down.y() < base.y() - 9.0,
        "negative vertical-align length should lower the inline box: base={base:?}, down={down:?}"
    );
    assert!(
        (pct.y() - up.y()).abs() < 1.0,
        "50% of a 20pt line-height should match a 10pt shift: up={up:?}, pct={pct:?}"
    );
}

#[tokio::test]
async fn baseline_shift_longhand_moves_inline_baselines() {
    let document = Html::from_string(
        "<p style=\"margin:0;font-size:20pt;line-height:20pt\">\
         Base<span style=\"baseline-shift:10pt\">up</span>\
         <span style=\"baseline-shift:-10pt\">down</span>\
         <span style=\"baseline-shift:50%;line-height:20pt\">pct</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    let base = lines.iter().find(|line| line.text == "Base").unwrap();
    let up = lines.iter().find(|line| line.text == "up").unwrap();
    let down = lines.iter().find(|line| line.text == "down").unwrap();
    let pct = lines.iter().find(|line| line.text == "pct").unwrap();

    assert!(up.y() > base.y() + 9.0);
    assert!(down.y() < base.y() - 9.0);
    assert!((pct.y() - up.y()).abs() < 1.0);
}

#[tokio::test]
async fn baseline_shift_length_percentage_places_text_and_replaced_inline_atoms() {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 260pt 500pt; margin: 20pt }} body {{ margin: 0 }}</style>\
         <span style=\"font:20pt/2 sans-serif\">\
         <img style=\"width:60pt;height:60pt;baseline-shift:0\" src=\"{GREEN_100_PNG}\">\
         <img style=\"width:60pt;height:60pt;baseline-shift:-0.2em\" src=\"{GREEN_100_PNG}\">\
         <span style=\"baseline-shift:0\">ZERO</span>\
         <span style=\"baseline-shift:1em\">UP</span>\
         <span style=\"baseline-shift:-100%\">DOWN</span>\
         <br><span>NEXT</span></span>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(page.images().len(), 2, "images={:?}", page.images());
    let baseline_image = &page.images()[0];
    let shifted_image = &page.images()[1];
    let line = |text: &str| {
        page.lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("expected line {text:?}: {:?}", page.lines()))
    };
    let zero = line("ZERO");
    let up = line("UP");
    let down = line("DOWN");
    let next = line("NEXT");

    assert!(
        (up.y() - zero.y() - 20.0).abs() < 1.0,
        "1em baseline-shift should raise text by 20pt: zero={zero:?}, up={up:?}"
    );
    assert!(
        (down.y() - zero.y() + 40.0).abs() < 1.0,
        "-100% baseline-shift should lower text by one 40pt line-height: zero={zero:?}, down={down:?}"
    );
    assert!(
        (shifted_image.y() - baseline_image.y() + 4.0).abs() < 1.0,
        "-0.2em baseline-shift should lower the replaced atom by 4pt: baseline_y={}, shifted_y={}",
        baseline_image.y(),
        shifted_image.y()
    );
    assert!(
        zero.y() - next.y() > 60.0,
        "line advance should include shifted first-line bounds: zero={zero:?}, next={next:?}"
    );
}

/// A root pseudo keeps its computed horizontal style, but must still consume
/// the propagated body's vertical-lr principal block track before the body
/// starts.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[tokio::test]
async fn root_before_advances_a_propagated_vertical_lr_body() {
    let document = Html::from_string(format!(
        "<style>
           @page {{ size: 300pt 200pt; margin: 0 }}
           html {{ writing-mode: horizontal-tb }}
           html::before {{
             background: orange;
             content: \"\";
             display: block;
             height: 75pt;
             margin-left: 6pt;
             margin-right: 12pt;
             margin-top: 6pt;
             width: 75pt;
           }}
           body {{ margin: 0; writing-mode: vertical-lr }}
           img {{ display: block; height: 30pt; width: 100pt }}
         </style>
         <div><img src=\"{GREEN_100_PNG}\"></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1, "document={document:?}");
    let page = &document.pages[0];
    let pseudo = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 165, 0)))
        .expect("expected the root ::before block");
    let image = page
        .images()
        .first()
        .expect("expected propagated body image");

    assert!((pseudo.x() - 6.0).abs() < 0.01, "pseudo={pseudo:?}");
    assert!(
        ((image.x() - (pseudo.x() + pseudo.width() + 6.0)).abs()) < 0.01,
        "the body must begin from the root track advanced by the pseudo's border box and block-end margin: pseudo={pseudo:?}, image={image:?}"
    );
}
