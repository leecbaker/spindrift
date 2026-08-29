use base64::Engine;

use super::*;

#[tokio::test]
async fn renders_hello_world_pdf() {
    let pdf = Html::from_string("<p>Hello, world</p>")
        .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
        .await
        .unwrap();

    let rendered = pdf_searchable_text(&pdf);
    assert!(rendered.starts_with("%PDF-1.4"));
    assert!(rendered.contains("/Subtype /Type0"));
    assert!(rendered.contains("/FontFile2"));
    assert!(rendered.contains("/ToUnicode"));
    assert!(rendered.contains("startxref"));
}

#[tokio::test]
async fn nested_scroll_marker_rules_do_not_turn_a_vertical_scroller_into_a_grid() {
    Html::from_string(
        r#"<style>
            .scroller {
              width: 200px; height: 200px; position: relative;
              scroll-marker-group: after; overflow: scroll;
              &::scroll-marker-group {
                height: 100px; width: 200px; display: grid;
                grid-auto-flow: column;
              }
            }
            .vertical { writing-mode: vertical-lr }
            .target {
              position: absolute; width: 50px; height: 50px;
              &::scroll-marker {
                content: ""; display: inline-block; width: 40px; height: 40px;
              }
            }
          </style>
          <div class="scroller vertical"><div class="target">target</div></div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .expect("nested scroll-marker rules must render without recursive layout");
}

#[tokio::test]
async fn inline_svg_text_is_an_outlined_actual_text_fallback() {
    let document = Html::from_string(
        r#"<style>@page { size: 160pt 80pt; margin: 0 } body { margin: 0 }</style>
           <svg width="120" height="30" xmlns="http://www.w3.org/2000/svg">
             <text x="4" y="20" font-family="Ahem" font-size="16">SVG shared text</text>
           </svg>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages[0].lines().is_empty());
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(
        rendered.contains("/ActualText (SVG shared text)"),
        "{rendered}"
    );
    assert!(!rendered.contains("BT\n"), "{rendered}");
}

#[tokio::test]
async fn svg_actual_text_concatenates_tspan_chunks_in_document_order() {
    let document = Html::from_string(
        r#"<style>@page { size: 160pt 80pt; margin: 0 } body { margin: 0 }</style>
           <svg width="120" height="30" xmlns="http://www.w3.org/2000/svg">
             <text x="4" y="20" font-family="Ahem" font-size="16">one<tspan> two</tspan><tspan> three</tspan></text>
           </svg>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages[0].lines().is_empty());
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert_eq!(
        rendered.matches("/ActualText (one two three)").count(),
        1,
        "{rendered}"
    );
    assert!(!rendered.contains("BT\n"), "{rendered}");
}

#[tokio::test]
async fn gradient_svg_text_uses_one_actual_text_outline_fallback() {
    let document = Html::from_string(
        r#"<style>@page { size: 160pt 80pt; margin: 0 } body { margin: 0 }</style>
           <svg width="120" height="30" xmlns="http://www.w3.org/2000/svg">
             <linearGradient id="gradient"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>
             <text x="4" y="20" font-size="16" fill="url(#gradient)">Gradient text</text>
           </svg>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .all(|line| line.text != "Gradient text"),
        "gradient SVG text must not add an invisible native-text duplicate"
    );
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(
        rendered.contains("/ActualText (Gradient text)"),
        "{rendered}"
    );
    assert!(rendered.contains("/Shading"), "{rendered}");
}

#[tokio::test]
async fn replaced_svg_image_text_uses_an_actual_text_outline_fallback() {
    let svg = base64::engine::general_purpose::STANDARD.encode(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="30"><text x="4" y="20" font-size="16">SVG image text</text></svg>"#,
    );
    let document = Html::from_string(format!(
        "<style>@page {{ size: 160pt 80pt; margin: 0 }} body {{ margin: 0 }}</style><img src=\"data:image/svg+xml;base64,{svg}\">"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages[0].lines().is_empty());
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(pdf_searchable_text(&pdf).contains("/ActualText (SVG image text)"));
}

#[tokio::test]
async fn exposes_document_pages() {
    let document = Html::from_string("<p>Hello, world</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "Hello, world");
}

#[tokio::test]
async fn gcpm_footnote_call_marker_and_body_render_once_on_its_page() {
    let document = Html::from_string(
        r#"<style>
              @page { size: 200pt 200pt; margin: 20pt;
                      @footnote { border-top: 2pt solid red; padding-top: 6pt } }
              .note { float: footnote }
              .note::footnote-marker { content: "* " }
            </style>
            <p>Lead <span class="note">footnote body</span> tail</p>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Lead"), "lines={text:?}");
    assert!(text.contains("tail"), "lines={text:?}");
    assert!(text.contains("*"), "lines={text:?}");
    assert_eq!(text.matches("footnote body").count(), 1, "lines={text:?}");
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
    );
}

#[tokio::test]
async fn gcpm_multiple_footnotes_keep_source_order_in_one_page_area() {
    let document = Html::from_string(
        r#"<style>
              @page { size: 250pt 250pt; margin: 20pt;
                      @footnote { background: lime } }
              .note { float: footnote }
            </style>
            <p>Alpha <span class="note">first note</span>
               beta <span class="note">second note</span>.</p>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text.matches("first note").count(), 1, "lines={text:?}");
    assert_eq!(text.matches("second note").count(), 1, "lines={text:?}");
    assert!(text.find("first note") < text.find("second note"));
    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 255, 0)))
            .count(),
        1,
        "a page has one footnote area regardless of the number of bodies"
    );
}

#[tokio::test]
async fn gcpm_footnote_area_margins_anchor_its_border_box_and_body_content() {
    let document = Html::from_string(
        r#"<style>
              @page { size: 200pt 200pt; margin: 20pt;
                      @footnote {
                        margin: 11pt 13pt 17pt 19pt;
                        padding: 5pt 7pt;
                        background: lime;
                      } }
              .note { float: footnote }
            </style>
            <p>Lead <span class="note">footnote body</span> tail</p>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let area = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 255, 0)))
        .expect("footnote area background");
    // GCPM anchors the margin box to the page-area bottom (y = 20). The
    // painted border box therefore starts after the 19pt left and 17pt bottom
    // margins, and is narrowed by both horizontal margins.
    assert!((area.x() - 39.0).abs() < 0.01, "area={area:?}");
    assert!((area.y() - 37.0).abs() < 0.01, "area={area:?}");
    assert!((area.width() - 128.0).abs() < 0.01, "area={area:?}");
}

#[tokio::test]
async fn display_none_document_root_has_one_blank_page_without_fallback_text() {
    let document = Html::from_string(
        "<style>html { display: none }</style><p>This text must not be rendered.</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "");
}

#[tokio::test]
async fn display_none_root_does_not_propagate_body_scroll_overflow() {
    let document = Html::from_string(
        "<style>html { display: none; scrollbar-color: red red } body { overflow: scroll }</style><body></body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert!(document.pages[0].rects().is_empty());
    assert_eq!(document.pages[0].lines()[0].text, "");
}

#[tokio::test]
async fn non_propagated_body_overflow_hidden_clips_its_descendants() {
    let document = Html::from_string(
        "<style>@page { size: 800px 800px; margin: 0 }\
         html { overflow: hidden; height: 500px }\
         body { overflow: hidden; width: 0; height: 0; border: solid 200px green }\
         div { background: red; width: 200px; height: 200px }</style><body><div></div></body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "a non-propagated body must clip its red child: {:?}",
        document.pages[0].rects()
    );
}

#[tokio::test]
async fn document_without_text_or_ch_lengths_does_not_load_a_document_font() {
    let document = Html::from_string("<style>html { display: none }</style>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(document.fonts.is_empty(), "fonts={:?}", document.fonts);
}

#[tokio::test]
async fn visible_empty_block_with_normal_line_height_does_not_load_a_document_font() {
    let document = Html::from_string("<div></div>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(document.fonts.is_empty(), "fonts={:?}", document.fonts);
}

#[tokio::test]
async fn forced_empty_line_with_normal_line_height_loads_its_font() {
    let document = Html::from_string("<div><br></div>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        !document.fonts.is_empty(),
        "a forced line still needs selected-font line metrics"
    );
}

#[tokio::test]
async fn visible_text_still_loads_a_document_font() {
    let document = Html::from_string("<p>font loading remains demand-driven</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(!document.fonts.is_empty());
}

#[tokio::test]
async fn parsed_character_references_are_not_decoded_twice_in_normal_inline_text() {
    let document = Html::from_string("<p>&amp;lt; &copy; &#x1f642;</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "&lt; © 🙂");
}

#[tokio::test]
async fn parsed_character_references_are_not_decoded_twice_in_pre_wrap_text() {
    let document = Html::from_string(
        "<style>p { margin: 0; white-space: pre-wrap }</style><p>&amp;lt; &copy; &#x1f642;</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "&lt; © 🙂");
}

#[tokio::test]
async fn absolutely_positioned_body_overflow_hidden_does_not_clip_own_contents() {
    let document = Html::from_string(
        "<!doctype html>\
         <meta charset=utf-8>\
         <style>@page { size: 400px 400px; margin: 0 }</style>\
         <body style=\"overflow: hidden; margin: 100px; width: 100px; height: 100px; border: 1px solid green; position: absolute; top: 0; left: 0\">\
           The body should have visible overflow of the text that totally doesn't fit in the little box.\
         </body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "box."),
        "overflowing body text should remain laid out visibly: {:?}",
        document.pages[0].lines()
    );

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    assert!(
        !rendered.contains("\nW\nn"),
        "propagated body overflow should not emit an element clip scope: {rendered}"
    );
}

#[tokio::test]
async fn block_align_content_center_aligns_contents_in_definite_height() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 50pt; height: 80pt; align-content: center; background: red }\
         .item { height: 20pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("vertical block container background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("vertical block container background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("vertical block child background should paint");

    assert!(
        (green.x() - red.x()).abs() < 0.01,
        "vertical-rl align-content:end should pack content against physical left/block-end: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn vertical_width_intrinsic_keywords_use_logical_block_size() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 0 } html, body { margin: 0 }\
         .container { width: 70pt; border: 1pt solid black; font-size: 10pt; line-height: 20pt }\
         .container > div { writing-mode: vertical-lr; margin: 0; height: 30pt }\
         .min { width: min-content; background: blue }\
         .max { width: max-content; background: cyan }</style>\
         <div class=\"container\"><div class=\"min\">A<br>B<br>C</div><div class=\"max\">A<br>B<br>C</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("vertical min-content block should paint");
    let cyan = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 255, 255)))
        .expect("vertical max-content block should paint");

    assert!(
        (blue.x() - 1.0).abs() < 0.01,
        "vertical-lr min-content block should start at the container content edge: {blue:?}"
    );
    assert!(
        (blue.width() - 60.0).abs() < 0.01,
        "vertical-lr min-content physical width should use three logical block-axis line columns: {blue:?}"
    );
    assert!(
        (cyan.x() - 1.0).abs() < 0.01,
        "vertical-lr max-content block should start at the container content edge: {cyan:?}"
    );
    assert!(
        (cyan.width() - 60.0).abs() < 0.01,
        "vertical-lr max-content physical width should use three logical block-axis line columns: {cyan:?}"
    );
}

#[tokio::test]
async fn parallel_vertical_and_sideways_auto_width_uses_logical_block_contribution() {
    for writing_mode in ["vertical-lr", "vertical-rl", "sideways-lr", "sideways-rl"] {
        let document = Html::from_string(format!(
            r#"<style>@page {{ size: 180pt 140pt; margin: 0 }} html, body {{ margin: 0 }}
               .block {{ writing-mode: {writing_mode}; width: auto; height: 30pt; font: 10pt/20pt Ahem; background: blue }}</style>
               <div class="block">A<br>B<br>C</div>"#
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let block = document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .expect("vertical auto-width block background should paint");
        assert!(
            (block.width() - 60.0).abs() < 0.01,
            "{writing_mode} width:auto should use three 20pt logical block columns: {block:?}"
        );
        assert!(
            (block.height() - 30.0).abs() < 0.01,
            "{writing_mode} should retain its definite physical height: {block:?}"
        );
    }
}

#[tokio::test]
async fn parallel_vertical_auto_width_uses_the_auto_height_line_fitting_fallback() {
    let document = Html::from_string(
        r#"<style>@page { size: 180pt 140pt; margin: 0 } html, body { margin: 0 }
           .block { writing-mode: vertical-lr; width: auto; height: auto; font: 10pt/20pt Ahem; background: blue }</style>
           <div class="block">A<br>B</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let block = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("vertical auto-sized block background should paint");
    assert!(
        (block.width() - 40.0).abs() < 0.01,
        "two forced logical lines should contribute two physical columns: {block:?}"
    );
}

#[tokio::test]
async fn vertical_block_estimate_and_final_layout_agree_on_auto_width() {
    let document = Html::from_string(
        r#"<style>@page { size: 180pt 140pt; margin: 0 } html, body { margin: 0 }
           .outer, .inner { writing-mode: vertical-lr; width: auto; height: 30pt; font: 10pt/20pt Ahem }
           .outer { background: red } .inner { background: blue }</style>
           <div class="outer"><div class="inner">A<br>B<br>C</div></div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let outer = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("estimated vertical parent background should paint");
    let inner = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("final vertical child background should paint");
    assert!(
        (outer.width() - inner.width()).abs() < 0.01 && (inner.width() - 60.0).abs() < 0.01,
        "the speculative parent estimate and committed child layout must use the same logical block contribution: outer={outer:?}, inner={inner:?}"
    );
}

/// An inline static-position placeholder must advance by its hypothetical
/// source text, rather than by the positioned source's physical width (the
/// logical block extent in a vertical writing mode).  The green abspos
/// background must consequently cover the following red in-flow text.
#[tokio::test]
async fn vertical_rtl_static_position_placeholders_cover_the_hypothetical_source() {
    for writing_mode in ["vertical-lr", "vertical-rl"] {
        for container_direction in ["ltr", "rtl"] {
            for source_direction in ["ltr", "rtl"] {
                for (source_display, text_indent, relative_inline_ancestor) in [
                    ("inline", "0", ""),
                    ("inline", "20pt", ""),
                    ("inline", "0", "position: relative; inset-inline-start: 2pt"),
                    (
                        "inline",
                        "20pt",
                        "position: relative; inset-inline-start: 2pt",
                    ),
                    ("block", "0", ""),
                    ("block", "20pt", ""),
                    ("block", "0", "position: relative; inset-inline-start: 2pt"),
                    (
                        "block",
                        "20pt",
                        "position: relative; inset-inline-start: 2pt",
                    ),
                ] {
                    let source_break = if source_display == "block" {
                        "<br>"
                    } else {
                        ""
                    };
                    let document = Html::from_string(format!(
            r#"<style>
                @page {{ size: 180pt 140pt; margin: 0 }}
                html, body {{ margin: 0 }}
                .container {{ position: relative; writing-mode: {writing_mode}; direction: {container_direction};
                    font: 16pt/1 Ahem; height: 120pt; color: green }}
                .ancestor {{ direction: {source_direction}; {relative_inline_ancestor} }}
                .abs {{ position: absolute; display: {source_display}; background: green; color: green }}
                .red {{ background: red; color: red }}
                .indented {{ text-indent: {text_indent} }}
            </style>
            <div class=\"container indented\">XXX<span class=\"ancestor\">XX<div class=\"abs\">XXXXX</div>{source_break}<span class=\"red\">XXXXX</span></span></div>"#
                    ))
                    .render(&RenderOptions::default())
                    .await
                    .unwrap();

                    let red = CssColor::new(255, 0, 0);
                    assert!(
                        document.pages[0]
                            .rects()
                            .iter()
                            .all(|rect| rect.fill != Some(red)),
                        "{writing_mode} container={container_direction} source={source_direction} {source_display} static placeholder with indent={text_indent:?} and relative ancestor={relative_inline_ancestor:?} must cover its red hypothetical source: rects={:?}",
                        document.pages[0].rects(),
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn propagated_vertical_body_auto_width_remains_the_document_canvas() {
    let document = Html::from_string(
        r#"<style>@page { size: 180pt 140pt; margin: 0 }
           html, body { margin: 0; writing-mode: vertical-lr } body { background: purple }</style>text"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let canvas = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(128, 0, 128)))
        .expect("propagated body canvas background should paint");
    assert!(
        (canvas.width() - 180.0).abs() < 0.01,
        "the propagated vertical body must retain the ICB's physical width: {canvas:?}"
    );
}

#[tokio::test]
async fn vertical_grid_auto_width_uses_its_logical_row_tracks() {
    let document = Html::from_string(
        r#"<style>@page { size: 180pt 140pt; margin: 0 } html, body { margin: 0 }
           #grid { display: grid; writing-mode: vertical-lr; width: auto; height: 30pt;
                   grid-template-columns: 30pt; grid-template-rows: 20pt 20pt 20pt; background: green }</style>
           <div id="grid"><i></i><i></i><i></i></div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let grid = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("vertical grid background should paint");
    assert!(
        (grid.width() - 60.0).abs() < 0.01,
        "vertical grid width:auto should use the three logical row tracks: {grid:?}"
    );
}

#[tokio::test]
async fn vertical_absolute_auto_width_uses_wrapped_logical_columns() {
    let document = Html::from_string(
        r#"<style>@page { size: 180pt 140pt; margin: 0 } html, body { margin: 0 }
           #containing { position: relative; width: 160pt; height: 100pt }
           #abs { position: absolute; writing-mode: vertical-lr; width: auto; height: 30pt;
                  font: 10pt/20pt Ahem; background: blue }</style>
           <div id="containing"><div id="abs">A<br>B<br>C</div></div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let abspos = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("vertical absolute block background should paint");
    assert!(
        (abspos.width() - 60.0).abs() < 0.01,
        "vertical abspos width:auto should use three fitted logical columns: {abspos:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let bookmark = document
        .bookmarks
        .iter()
        .find(|bookmark| bookmark.label == "Target")
        .expect("heading bookmark should be exposed");
    let heading_background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let bookmark = document
        .bookmarks
        .iter()
        .find(|bookmark| bookmark.label == "Target")
        .expect("heading bookmark should be exposed");
    let heading_background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Link")
        .expect("linked text should render");
    let link = document.pages[0]
        .links()
        .iter()
        .find(|link| link.target.as_ref() == "https://example.com")
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("normal-flow child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("block child background should paint");

    assert!(
        (green.y() + green.height() - red.y() - red.height()).abs() < 0.01,
        "default center overflow should use safe block-start fallback: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn block_align_content_scroll_container_retains_unsafe_overflow_geometry() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .block { width: 50pt; height: 20pt; overflow-y: auto; align-content: center; background: red }\
         .item { height: 40pt; background: green }</style>\
         <div class=\"block\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("block child background should paint");

    assert!(
        (green.y() - (red.y() - 10.0)).abs() < 0.01 && (green.height() - 40.0).abs() < 0.01,
        "the retained overflow clip must not destructively trim the unsafe centered child: red={red:?}, green={green:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("block container background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("block child background should paint");

    assert!(
        (green.y() - (red.y() - 10.0)).abs() < 0.01
            && (green.y() + green.height() - (red.y() + red.height() + 10.0)).abs() < 0.01,
        "unsafe center should allow equal overflow on both block sides: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn auto_height_overflow_hidden_retains_negative_margin_child_geometry() {
    let document = Html::from_string(
        "<style>@page { size: 140px 140px; margin: 0 } body { margin: 0 }\
         .before, .clip, .item { width: 100px }\
         .before { height: 50px; background: green }\
         .clip { overflow: hidden }\
         .item { height: 100px; margin-top: -50px; background: red }</style>\
         <div class=\"before\"></div><div class=\"clip\"><div class=\"item\"></div></div>",
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
        .unwrap_or_else(|| panic!("expected previous green block: {:?}", page.rects()));
    let red_rect = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap_or_else(|| panic!("expected negative-margin child: {:?}", page.rects()));
    assert!(
        red_rect.y() < green_rect.y() + green_rect.height()
            && (red_rect.height() - 75.0).abs() < 0.01,
        "the retained overflow clip must leave the negative-margin child's source geometry intact: green={green_rect:?}, red={red_rect:?}"
    );
}

#[tokio::test]
async fn exposes_default_heading_bookmarks() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 10pt } body, h2, h4 { margin: 0; font-size: 10pt; line-height: 10pt }</style><h2>Chapter</h2><h4>Section</h4><h2>Next</h2>",
    )
    .render(&RenderOptions::default()).await
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
    .render(&RenderOptions::default()).await
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
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default()).await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(rendered.contains("/Outlines"));
    assert!(rendered.contains("/Title (Chapter)"));
    assert!(rendered.contains("/Title (Section)"));
    assert!(rendered.contains("/Count 2"));
    assert!(rendered.contains("/Count 1"));
    // The first page is object 3 after removing the obsolete reserved font
    // resource object from the PDF allocation schedule.
    assert!(rendered.contains("/Dest [3 0 R /XYZ"));
}

#[tokio::test]
async fn applies_minimal_page_css() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string(
            "@page { size: 200pt 100pt; margin: 10pt } body, p { margin: 0 }",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].width(), 200.0);
    assert_eq!(document.pages[0].height(), 100.0);
    assert_eq!(document.pages[0].lines()[0].x(), 10.0);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
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
            .render(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(document.pages[0].lines()[0].x(), 10.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines()[0], 100.0);
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
        .with_base_path(wpt_root)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages.len(), 3);
    for (page, color) in document.pages.iter().zip([
        CssColor::new(255, 255, 0),
        CssColor::new(0, 255, 255),
        CssColor::new(255, 192, 203),
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
        .with_base_path(wpt_root)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages.len(), 4);
    for (index, page) in document.pages.iter().enumerate() {
        if index.is_multiple_of(2) {
            assert_eq!(
                final_rect_fill_at(page, 10.0, 10.0),
                Some(CssColor::new(255, 255, 0))
            );
            assert_ne!(
                final_rect_fill_at(page, page.width() - 10.0, page.height() - 10.0),
                Some(CssColor::new(255, 255, 0))
            );
        } else {
            assert_eq!(
                final_rect_fill_at(page, page.width() - 10.0, page.height() - 10.0),
                Some(CssColor::new(255, 255, 0))
            );
            assert_ne!(
                final_rect_fill_at(page, 10.0, 10.0),
                Some(CssColor::new(255, 255, 0))
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
        .with_base_path(wpt_root)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let letters = ('A'..='P')
        .filter_map(|letter| {
            let text = letter.to_string();
            page.lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("horizontal vi/vb box should paint");
    assert!((red.width() - 100.0).abs() < 0.01);
    assert!((red.height() - 50.0).abs() < 0.01);

    assert_eq!(document.pages.len(), 2);
    let blue = document.pages[1]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = &document.pages[0].lines()[0];
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = &document.pages[0].lines()[0];
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
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let border_rect_index = page
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(green))
        .expect("page border should paint green rect primitives");
    let border_operation = page
        .operations()
        .iter()
        .position(|operation| {
            matches!(operation, crate::document::paint::page::PaintOperation::Rect(index) if *index == border_rect_index)
        })
        .expect("green page border rect should participate in paint order");
    let line_operation = page
        .operations()
        .iter()
        .position(|operation| {
            matches!(
                operation,
                crate::document::paint::page::PaintOperation::Line(0)
            )
        })
        .expect("document line should participate in paint order");

    assert!(border_operation < line_operation);
    assert!(page.rects().iter().any(|rect| {
        rect.fill == Some(green)
            && ((rect.width() - 100.0).abs() < 0.01 && (rect.height() - 5.0).abs() < 0.01
                || (rect.width() - 5.0).abs() < 0.01 && (rect.height() - 80.0).abs() < 0.01)
    }));
}

#[tokio::test]
async fn renders_page_margin_box_counters() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string(
            "@page { size: 200pt 100pt; margin: 10pt; @bottom-right { content: \"Page \" counter(page) \" of \" counter(pages); background-color: black; color: white; height: 10pt; width: 50%; font-size: 8pt } }",
        ))
        .render(&RenderOptions::default()).await
        .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.y() == 0.0 && rect.fill == Some(CssColor::BLACK))
    );
    assert!(document.pages[0].operations().iter().any(|operation| {
        matches!(
            operation,
            crate::document::paint::page::PaintOperation::Rect(index)
                if document.pages[0]
                    .rects()
                    .get(*index)
                    .is_some_and(|rect| rect.y() == 0.0 && rect.fill == Some(CssColor::BLACK))
        )
    }));
    let footer = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("Page 1 of 1") && line.color == CssColor::WHITE)
        .unwrap();
    assert!(
        footer
            .runs
            .iter()
            .any(|run| run.text.contains("Page 1 of 1")
                && run.glyphs.as_ref().is_some_and(|glyphs| !glyphs.is_empty()))
    );
    assert!(document.pages[0].operations().iter().any(|operation| {
        matches!(
            operation,
            crate::document::paint::page::PaintOperation::Line(index)
                if document.pages[0]
                    .lines()
                    .get(*index)
                    .is_some_and(|line| line.text.contains("Page 1 of 1") && line.color == CssColor::WHITE)
        )
    }));
}

#[tokio::test]
async fn page_margin_counters_follow_forced_breaks_in_absolute_subtree() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
           @page {\
             margin: 4em;\
             @top-center {\
               text-align: left;\
               vertical-align: top;\
               content: \"Page \" counter(page) \" of \" counter(pages);\
             }\
             @bottom-center {\
               text-align: left;\
               vertical-align: top;\
               content: \"Page \" counter(page) \" of \" counter(pages);\
             }\
           }\
           @page :first {\
             @top-center { content: none; }\
             @bottom-center { content: none; }\
           }\
           body { margin: 0; }\
         </style>\
         <div style=\"position:absolute;\">\
           All pages except this one should display the current page and the total page\
           count in both the header and footer.\
           <div style=\"break-before:page;\">Another page</div>\
           <div style=\"break-before:page;\">Yet another page</div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-page/content-005-print.html.
    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text.contains("All pages except this one"))
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .all(|line| !line.text.starts_with("Page "))
    );

    for (page_index, text) in [(1, "Another page"), (2, "Yet another page")] {
        let page = &document.pages[page_index];
        assert!(page.lines().iter().any(|line| line.text == text));
        let counter = format!("Page {} of 3", page_index + 1);
        assert_eq!(
            page.lines()
                .iter()
                .filter(|line| line.text == counter)
                .count(),
            2,
            "expected header and footer counters on page {}",
            page_index + 1
        );
    }
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "I / 02")
    );
    assert!(
        document.pages[1]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "1")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "10")
    );
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "11")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "12")
    );
    assert!(
        document.pages[2]
            .lines()
            .iter()
            .any(|line| line.text == "13")
    );
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "V")
    );
    assert!(
        document.pages[1]
            .lines()
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
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let top_margin_patterns = document.pages[0]
        .image_patterns()
        .iter()
        .filter(|pattern| pattern.background && (pattern.y() - 50.0).abs() < 0.01)
        .collect::<Vec<_>>();

    assert_eq!(
        top_margin_patterns.len(),
        1,
        "top-center page-margin background should use one repeated pattern: {top_margin_patterns:?}"
    );
    let pattern = top_margin_patterns[0];
    assert_eq!(pattern.x(), 10.0);
    assert_eq!(pattern.width(), 60.0);
    assert_eq!(pattern.height(), 10.0);
    assert_eq!(pattern.tiling.tile_size.width, 10.0);
    assert_eq!(pattern.tiling.tile_size.height, 10.0);
    assert_eq!(pattern.tiling.step.width, 10.0);
    assert_eq!(pattern.tiling.step.height, 10.0);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Chapter II")
    );
    assert!(
        document.pages[1]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Two / Before")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "Two / Before")
    );
}

#[tokio::test]
async fn string_set_target_counter_resolves_after_pagination() {
    let document = Html::from_string(
        "<p>One</p><h1>Header</h1><article id=\"chapter\"><p>Two</p></article>\
         <style>body, p, h1, article { margin: 0; font-size: 8pt; line-height: 8pt }\
         h1 { string-set: label \"Chapter \" target-counter(url(#chapter), page, upper-roman) }\
         article { break-before: page }</style>",
    )
    .with_stylesheet(Css::from_string(
        "@page { size: 120pt 80pt; margin: 10pt; \
         @bottom-center { content: string(label); font-size: 8pt; height: 10pt } }",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Chapter II")
    );
}

#[tokio::test]
async fn string_set_target_text_resolves_after_anchor_text_capture() {
    let document = Html::from_string(
        "<h1>Header</h1><article id=\"chapter\"><p>Two</p></article>\
         <style>body, p, h1, article { margin: 0; font-size: 8pt; line-height: 8pt }\
         h1 { string-set: label target-text(url(#chapter), before) \"/\" target-text(url(#chapter), content) }\
         article::before { content: \"Before\" }</style>",
    )
    .with_stylesheet(Css::from_string(
        "@page { size: 140pt 80pt; margin: 10pt; \
         @bottom-center { content: string(label); font-size: 8pt; height: 10pt } }",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Before/Two")
    );
}

#[tokio::test]
async fn string_set_content_before_preserves_target_cross_references() {
    let document = Html::from_string(
        "<h1>Header</h1><article id=\"chapter\"><p>Two</p></article>\
         <style>body, p, h1, article { margin: 0; font-size: 8pt; line-height: 8pt }\
         h1 { string-set: label content(before) }\
         h1::before { content: \"Chapter \" target-counter(url(#chapter), page, upper-roman) \": \" target-text(url(#chapter), before) }\
         article { break-before: page }\
         article::before { content: \"Before\" }</style>",
    )
    .with_stylesheet(Css::from_string(
        "@page { size: 140pt 80pt; margin: 10pt; \
         @bottom-center { content: string(label); font-size: 8pt; height: 10pt } }",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Chapter II: Before")
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.contains(&"Head"), "{texts:?}");
    assert!(texts.contains(&"Tail"), "{texts:?}");
}

#[tokio::test]
async fn page_margin_text_box_trim_start_adjusts_fixed_box_paint_origin() {
    let render = |margin_extra: &str| {
        Html::from_string(format!(
            "<style>\
             @page {{ size: 400px 240px; margin: 100px 0 0 0;\
               @top-left {{ content: \"Header\"; font: 50px/2 sans-serif; height: 100px; text-box-edge: text; {margin_extra} }}\
             }}\
             body {{ margin: 0 }}\
             </style>"
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-start;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let header_y = |document: &quire::Document| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == "Header")
            .expect("page-margin header should render")
            .y()
    };
    let delta = header_y(&trimmed) - header_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "trim-start should move page-margin fixed-box text into removed leading: delta={delta}"
    );
}

#[tokio::test]
async fn vertical_page_margin_text_box_trim_start_uses_margin_box_writing_mode() {
    let render = |margin_extra: &str| {
        Html::from_string(format!(
            "<style>\
             @page {{ size: 400px 240px; margin: 100px 0 0 0;\
               @top-left {{ content: \"A\"; writing-mode: vertical-rl; text-orientation: upright; font: 50px/2 sans-serif; width: 100px; height: 100px; text-box-edge: text; {margin_extra} }}\
             }}\
             body {{ margin: 0 }}\
             </style>"
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-start;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let line_position = |document: &quire::Document| {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == "A")
            .expect("vertical page-margin text should render");
        (line.x(), line.y())
    };
    let (untrimmed_x, untrimmed_y) = line_position(&untrimmed);
    let (trimmed_x, trimmed_y) = line_position(&trimmed);
    let x_delta = trimmed_x - untrimmed_x;
    let y_delta = trimmed_y - untrimmed_y;
    assert!(
        x_delta > 0.0 && y_delta.abs() < 0.5,
        "vertical trim-start should move page-margin text along logical block-start: x_delta={x_delta}, y_delta={y_delta}"
    );
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Intro" && line.y() > 65.0)
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "Methods" && line.y() > 65.0)
    );
    assert!(
        !document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Methods")
    );
}

#[tokio::test]
async fn named_string_page_counters_resolve_in_the_source_page_context() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 10pt; @top-center { content: string(label); font-size: 8pt; height: 10pt } }\
         body, h1, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { string-set: label counter(page) ' of ' counter(pages) }\
         .next { break-before: page }\
         </style><h1>First</h1><p class=\"next\">Second page</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    for page in &document.pages {
        assert!(
            page.lines().iter().any(|line| line.text == "1 of 2"),
            "named-string page counters must retain the source page: {:?}",
            page.lines()
        );
    }
}

#[tokio::test]
async fn display_none_named_strings_assign_at_their_source_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 10pt; @top-center { content: string(chapter); font-size: 8pt; height: 10pt } }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         .source { display: none; string-set: chapter content(text); counter-increment: hidden }\
         .next { break-before: page }\
         </style><h1 class=\"source\">One</h1><p>First page</p><div class=\"next\"><h1 class=\"source\">Two</h1><p>Second page</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "One")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "Two")
    );
    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .all(|line| line.text != "hidden"),
        "the suppressed source must not create normal counter/layout output"
    );
}

#[tokio::test]
async fn inline_display_none_named_string_preserves_inline_source_order() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 10pt; @top-center { content: string(label); font-size: 8pt; height: 10pt } }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         .source { display: none; string-set: label content(text) }\
         </style><p><span class=\"source\">Inline title</span><span>Visible</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Inline title")
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Visible"),
        "the hidden inline source must not suppress its visible sibling"
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        !document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Methods" && line.y() > 65.0)
    );
    assert!(
        document.pages[1]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "First" && line.y() > 65.0)
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "First" && line.y() > 65.0),
        "{:?}",
        document.pages[1].lines()
    );
    assert!(
        !document.pages[1]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "First" && line.y() > 65.0),
        "{:?}",
        document.pages[1].lines()
    );
    assert!(
        !document.pages[1]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "First" && line.y() > 65.0)
    );
    assert!(
        document.pages[0]
            .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        !document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Intro" && line.y() > 65.0)
    );
    assert!(
        document.pages[1]
            .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Icon" && line.y() > 65.0)
    );
    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[0].images()[0].pixel_width(), 1);
    assert_eq!(document.pages[0].images()[0].pixel_height(), 1);
}

#[tokio::test]
async fn string_set_can_capture_generated_gradient_images_for_page_margin_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 80pt; margin: 12pt; @top-left { content: string(label); font-size: 8pt; height: 10pt } }\
         body, h1 { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { string-set: label \"Grad\" linear-gradient(in srgb, red, blue) radial-gradient(in srgb circle, white, black) }\
         </style><h1>Heading</h1>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Grad" && line.y() > 65.0),
        "{:?}",
        document.pages[0].lines()
    );
    assert_eq!(document.pages[0].gradient_patterns().len(), 2);
    assert_eq!(document.pages[0].images().len(), 0);
}

#[tokio::test]
async fn page_margin_content_supports_generated_gradient_images() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 80pt; margin: 12pt;\
           @top-left { content: \"G\" linear-gradient(in srgb, red, blue) radial-gradient(in srgb circle, white, black); font-size: 8pt; height: 10pt } }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Body</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "G" && line.y() > 65.0)
    );
    assert_eq!(document.pages[0].gradient_patterns().len(), 2);
    assert_eq!(document.pages[0].images().len(), 0);
}

#[tokio::test]
async fn string_set_preserves_quote_and_leader_items_for_page_margin_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 90pt; margin: 20pt;\
           @top-center { content: string(label); quotes: \"[\" \"]\"; font-size: 10pt; line-height: 10pt; width: 160pt } }\
         body, h1 { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { string-set: label open-quote \"Chapter\" close-quote leader(dotted) \"2\" }\
         </style><h1>Heading</h1>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let margin_text = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.y() > 65.0)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(margin_text.contains("[Chapter]"), "{margin_text}");
    assert!(margin_text.contains("..."), "{margin_text}");
    assert!(margin_text.contains("2"), "{margin_text}");
}

#[tokio::test]
async fn string_set_attr_uses_string_fallback_when_attribute_is_missing() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 80pt; margin: 10pt; @top-center { content: string(label); font-size: 8pt; height: 10pt } }\
         body, h1 { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { string-set: label attr(data-title, \"Fallback Title\") }\
         </style><h1>Heading</h1>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Fallback Title" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn string_set_can_capture_content_first_letter() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 80pt; margin: 10pt; @top-center { content: string(label); font-size: 8pt; height: 10pt } }\
         body, h1 { margin: 0; font-size: 10pt; line-height: 10pt }\
         h1 { string-set: label content(first-letter) }\
         </style><h1>“Alpha” title</h1>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "“A" && line.y() > 65.0)
    );
}

#[tokio::test]
async fn string_set_can_capture_list_marker_text() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 90pt; margin: 12pt; @top-center { content: string(label); font-size: 8pt; height: 10pt } }\
         body, ol, li { margin: 0; padding: 0; font-size: 10pt; line-height: 10pt }\
         ol { list-style-position: inside }\
         li { string-set: label content(marker) \"-\" content(text) }\
         </style><ol><li>Alpha</li></ol>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "1.-Alpha" && line.y() > 70.0)
    );
}

#[tokio::test]
async fn string_set_can_capture_custom_marker_text() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 90pt; margin: 12pt; @top-center { content: string(label); font-size: 8pt; height: 10pt } }\
         body, ol, li { margin: 0; padding: 0; font-size: 10pt; line-height: 10pt }\
         ol { list-style-position: inside }\
         li::marker { content: \"Item \" counter(list-item, lower-alpha) \")\" }\
         li { string-set: label content(marker) }\
         </style><ol><li>Alpha</li></ol>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Item a)" && line.y() > 70.0)
    );
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[0].images()[0].pixel_width(), 1);
    assert_eq!(document.pages[0].images()[0].pixel_height(), 1);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let replayed_background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("running element source background should paint in the page margin");
    assert!(
        (replayed_background.width() - 42.0).abs() < 0.01,
        "{replayed_background:?}"
    );
    assert!(
        (replayed_background.height() - 10.0).abs() < 0.01,
        "{replayed_background:?}"
    );
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let page_margin_text = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Running Header" && line.y() > 65.0)
    );
    assert!(
        !document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Running Header" && line.y() < 65.0)
    );
    assert!(
        document.pages[0]
            .lines()
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
        .render(&RenderOptions::default()).await
        .unwrap();

    let footer = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let header = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("generated page-margin box should paint a green background");

    assert_eq!(header.x(), 40.0);
    assert_eq!(header.y(), 175.0);
    assert_eq!(header.width(), 20.0);
    assert_eq!(header.height(), 10.0);
}

#[tokio::test]
async fn page_margin_center_and_middle_auto_sizes_respect_definite_neighbors() {
    let document = Html::from_string(
        "<style>\
         @page { size: 500pt 400pt; margin: 100pt;\
           @top-left { content: \"\"; width: 25pt; height: 25pt; margin: auto; }\
           @top-center { content: \"\"; background: rgb(0, 128, 0); height: 25pt; margin: auto; }\
           @top-right { content: \"\"; width: 25pt; height: 25pt; margin: auto; }\
           @right-top { content: \"\"; width: 25pt; height: 25pt; margin: auto; }\
           @right-middle { content: \"\"; background: rgb(0, 0, 255); width: 25pt; margin: auto; }\
           @right-bottom { content: \"\"; width: 25pt; height: 25pt; margin: auto; }\
         }\
         body { margin: 0 }\
         </style>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let top_center = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("top-center margin box should paint");
    let right_middle = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("right-middle margin box should paint");

    assert_eq!(top_center.width(), 250.0);
    assert_eq!(top_center.height(), 25.0);
    assert_eq!(top_center.x(), 125.0);
    assert_eq!(top_center.y(), 337.5);
    assert_eq!(right_middle.width(), 25.0);
    assert_eq!(right_middle.height(), 150.0);
    assert_eq!(right_middle.x(), 437.5);
    assert_eq!(right_middle.y(), 125.0);
}

#[tokio::test]
async fn page_margin_box_overconstrained_fixed_axis_ignores_outer_margin() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 200pt; margin: 40pt; @top-left { content: \"\"; background-color: green; width: 20pt; height: 30pt; margin: 8pt } }\
         body, p { margin: 0 }\
         </style><p></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let header = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("generated page-margin box should paint a green background");

    assert_eq!(header.x(), 48.0);
    assert_eq!(header.y(), 168.0);
    assert_eq!(header.width(), 20.0);
    assert_eq!(header.height(), 30.0);
}

#[tokio::test]
async fn page_margin_box_fixed_axis_clamps_authored_auto_margins_before_overflow() {
    let document = Html::from_string(
        "<style>\
         @page { size: 300pt 300pt; margin: 100pt;\
           @left-middle { content: \"\"; background-color: green; width: 130pt; margin: 10pt }\
           @bottom-center { content: \"\"; background-color: blue; height: 150pt; margin: auto 0 }\
         }\
         body, p { margin: 0 }\
         </style><p></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let left = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("left margin box should paint");
    assert_eq!(left.x(), -40.0);
    assert_eq!(left.width(), 130.0);

    let bottom = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("bottom margin box should paint");
    // An authored auto margin is clamped to zero when the fixed axis
    // overflows. Only a fully explicit equation may make the ignored outer
    // margin negative.
    assert_eq!(bottom.y(), 0.0);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let header = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "base"),
        "base page-margin content should remain on page 1"
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "second"),
        ":nth(2) page-margin content should apply on page 2"
    );
    assert!(
        document.pages[2]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        !document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let header = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("page margin box background should paint");
    assert_eq!(header.x(), 20.0);
    assert_eq!(header.width(), 20.0);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0))),
        "outline should paint red primitives"
    );
}

/// Page-margin boxes paint in their own stacking contexts above document
/// contents. Their local final outline phase must not be promoted into the
/// document's normal-flow outline phase.
#[tokio::test]
async fn page_margin_outline_remains_above_document_positioned_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 20pt;\
           @top-left { content: \"\"; width: 20pt; height: 10pt; outline: 2pt solid red }\
         }\
         body { margin: 0 } .outer { position: relative; width: 40pt; height: 20pt }\
         .absolute { position: absolute; inset: 0; width: 20pt; height: 10pt; background: rgb(0 128 0) }\
         </style><div class=\"outer\"><div class=\"absolute\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let positioned = first_rect_paint_operation_index(page, CssColor::new(0, 128, 0));
    let margin_outline = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));
    assert!(
        positioned < margin_outline,
        "page-margin outline must remain in its later independent context: {:?}",
        page.paint_operations()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = first_rect_paint_operation_index(&document.pages[0], CssColor::new(255, 0, 0));
    let green = first_rect_paint_operation_index(&document.pages[0], CssColor::new(0, 128, 0));

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
    .render(&RenderOptions::default()).await
    .unwrap();

    let cyan = first_rect_paint_operation_index(&document.pages[0], CssColor::new(0, 255, 255));
    let gray = first_rect_paint_operation_index(&document.pages[0], CssColor::new(221, 221, 221));
    let yellow = first_rect_paint_operation_index(&document.pages[0], CssColor::new(255, 255, 0));

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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].width(), 120.0);
    assert_eq!(document.pages[0].height(), 120.0);
    assert_eq!(document.pages[0].lines()[0].x(), 20.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines()[0], 100.0);
    assert_eq!(document.pages[1].width(), 100.0);
    assert_eq!(document.pages[1].height(), 100.0);
    assert_eq!(document.pages[1].lines()[0].x(), 10.0);
    assert_line_baseline_at_top(&document, &document.pages[1].lines()[0], 90.0);
}

#[tokio::test]
async fn named_page_transition_reenters_normalized_destination_canvas() {
    let document = Html::from_string(
        "<style>\
         @page :first { size: 120pt 120pt; margin: 20pt }\
         @page { size: 120pt 120pt; margin: 0 }\
         div { display: block; width: 10pt; height: 10pt }\
         </style>\
         <div style=\"page: a; background: lightblue\"></div>\
         <div style=\"page: b; background: pink\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let first_rect = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(173, 216, 230)))
        .expect("lightblue first-page box should be painted");
    let second_rect = document.pages[1]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 192, 203)))
        .expect("pink second-page box should be painted");

    assert_eq!(document.pages.len(), 2);
    assert_eq!(first_rect.x(), 26.0);
    assert_eq!(first_rect.y(), 84.0);
    // A named-page transition re-enters the destination page's canvas rather
    // than retaining the first page's page inset. The UA body's 8px margin
    // still applies inside that zero-margin page area.
    // <https://www.w3.org/TR/css-break-3/#box-splitting>
    assert_eq!(second_rect.x(), 6.0);
    assert_eq!(second_rect.y(), 110.0);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[1].lines()[0].x(), 30.0);
    assert_line_baseline_at_top(&document, &document.pages[1].lines()[0], 75.0);
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let footer = document.pages[1]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[1].width(), 160.0);
    assert_eq!(document.pages[1].height(), 120.0);
    assert_eq!(document.pages[1].lines()[0].x(), 20.0);
    // The named page area's block-start edge is 20pt below its 120pt page
    // top. Derive the glyph baseline from the selected font's metrics rather
    // than freezing one platform face's ascender into a page-geometry test.
    assert_line_baseline_at_top(&document, &document.pages[1].lines()[0], 100.0);
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].lines()[0].x(), 48.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines()[0], 60.0);
    assert_eq!(document.pages[1].lines()[0].x(), 0.0);
    assert_line_baseline_at_top(&document, &document.pages[1].lines()[0], 84.0);
    assert_eq!(document.pages[2].lines()[0].x(), 96.0);
    assert_line_baseline_at_top(&document, &document.pages[2].lines()[0], 84.0);
}

#[tokio::test]
async fn page_sized_flex_item_preserves_definite_size_and_auto_margins() {
    let document = Html::from_string(
        "<style>\
         @page { size: 20em 7em; margin: 0 }\
         body { margin: 0 }\
         .pagebox { display: flex; width: 20em; height: 7em }\
         .pagebox > div { width: 12em; height: 3em; margin: auto; background: yellow }\
         </style><div class=\"pagebox\"><div>center / middle</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let yellow = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .unwrap();
    assert_eq!(
        (yellow.x(), yellow.y(), yellow.width(), yellow.height()),
        (48.0, 24.0, 144.0, 36.0)
    );
}

#[tokio::test]
async fn definite_viewport_height_block_fragments_its_background_and_following_flow() {
    let document = Html::from_string(
        "<style>\
         @page { margin: 0 }\
         body { margin: 0 }\
         .outer { display: flex; flex-flow: column; background: yellow }\
         .tall { contain: size; width: 20pt; height: 350vh; background: hotpink }\
         </style><div class=\"outer\"><div class=\"tall\"></div>Yellow</div>White",
    );
    let document = document.render(&RenderOptions::default()).await.unwrap();

    assert_eq!(document.pages.len(), 4);
    let last_page_rects = document.pages[3].rects();
    assert!(
        last_page_rects.iter().any(|rect| {
            rect.fill == Some(CssColor::new(255, 105, 180))
                && (rect.width() - 20.0).abs() < 0.01
                && rect.height() > 400.0
        }),
        "last-page rectangles: {last_page_rects:?}"
    );
    assert!(
        document.pages[3]
            .lines()
            .iter()
            .any(|line| line.text.contains("Yellow"))
    );
    assert!(
        document.pages[3]
            .lines()
            .iter()
            .any(|line| line.text.contains("White"))
    );
}

#[tokio::test]
async fn nested_size_contained_visible_overflow_extends_flex_source_without_used_size() {
    let document = Html::from_string(
        "<style>\
         @page { margin: 0 }\
         body { margin: 0 }\
         .outer { display: flex; flex-flow: column; background: yellow }\
         .contained { contain: size; height: 350vh; width: 20pt; background: hotpink }\
         .clipped { contain: size; height: 0; overflow: hidden }\
         .clipped > div { height: 350vh }\
         .out { position: absolute; height: 350vh }\
         </style>\
         <div class=\"outer\"><div><div class=\"contained\"></div><div class=\"clipped\"><div></div></div><div class=\"out\"></div>Yellow</div></div>White",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 4);
    let last_page = &document.pages[3];
    assert!(
        last_page
            .lines()
            .iter()
            .any(|line| line.text.contains("Yellow"))
    );
    assert!(
        last_page
            .lines()
            .iter()
            .any(|line| line.text.contains("White"))
    );
}

#[tokio::test]
async fn nested_size_contained_visible_overflow_extends_grid_fragment_source() {
    let document = Html::from_string(
        "<style>\
         @page { margin: 0 }\
         body { margin: 0 }\
         .outer { display: grid; background: yellow }\
         .contained { contain: size; height: 350vh; width: 20pt; background: hotpink }\
         </style>\
         <div class=\"outer\"><div><div class=\"contained\"></div>Yellow</div></div>White",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 4);
    let last_page = &document.pages[3];
    assert!(
        last_page
            .rects()
            .iter()
            .any(|rect| { rect.fill == Some(CssColor::new(255, 105, 180)) && rect.height() > 0.0 })
    );
    assert!(
        last_page
            .lines()
            .iter()
            .any(|line| line.text.contains("White"))
    );
}

#[tokio::test]
async fn empty_size_contained_flex_item_replays_one_principal_slice_per_page() {
    let document = Html::from_string(
        r#"<style>:root { print-color-adjust: exact } body { margin: 0 }</style>
        <div style="display:flex; flex-flow:column; background:yellow">
          <div style="contain:size; height:350vh; width:50px; background:hotpink"></div>
          Yellow background, page 4.
        </div>
        White background, page 4."#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    assert_eq!(document.pages.len(), 4);
    for page in &document.pages {
        assert_eq!(
            page.rects()
                .iter()
                .filter(|rect| rect.fill == Some(CssColor::new(255, 105, 180)))
                .count(),
            1,
            "the flex fragment span, rather than its scratch replay, owns the empty item's principal background",
        );
    }
}

#[tokio::test]
async fn page_auto_margins_with_border_and_padding_center_specified_page_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 80pt; width: 40pt; height: 20pt; margin: auto; border: 5pt solid green; padding: 5pt }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Centered</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines()[0], 50.0);
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines()[0], 54.0);
    assert_eq!(document.pages[1].lines()[0].x(), 0.0);
    assert_line_baseline_at_top(&document, &document.pages[1].lines()[0], 84.0);
    assert_eq!(document.pages[2].lines()[0].x(), 30.0);
    assert_line_baseline_at_top(&document, &document.pages[2].lines()[0], 84.0);
}

#[tokio::test]
async fn negative_page_margins_expand_page_area_beyond_page_box() {
    let document = Html::from_string(
        "<style>\
         @page { size: 225pt; margin: -15pt }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style><p>Expanded</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].x(), -15.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines()[0], 240.0);
}

#[tokio::test]
async fn inline_page_assignment_does_not_select_a_named_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt; @bottom-center { content: \"base\" } }\
         @page report:first { @bottom-center { content: \"report first\" } }\
         body, p, span { margin: 0; font-size: 10pt; line-height: 10pt }\
         span { page: report }\
         </style><p><span>One</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "One")
    );
    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "base"),
        "{lines:?}"
    );
    assert!(
        !document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "report first")
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "One")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "blank")
    );
    assert!(
        !document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "Two")
    );
    assert!(
        document.pages[2]
            .lines()
            .iter()
            .any(|line| line.text == "Two")
    );
}

#[tokio::test]
async fn positioned_only_box_before_right_break_owns_its_source_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 0 }\
         body, section { margin: 0 }\
         .source { height: 0; break-after: right }\
         .source div { position: absolute; top: 0; left: 0; width: 100pt; height: 100pt; background: black }\
         </style><section class=\"source\"><div></div></section><section>After</section>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::BLACK))
    );
    assert!(
        document.pages[2]
            .lines()
            .iter()
            .any(|line| line.text == "After")
    );
}

#[tokio::test]
async fn named_page_with_only_positioned_content_selects_its_page_context() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page full { background: black; margin: 0 }\
         body, section, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         .cover { height: 0; page: full; break-after: right }\
         .cover > div { position: absolute; top: 0; left: 0; width: 10pt; height: 10pt; background: red }\
         </style><section class=\"cover\"><div></div></section><p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::BLACK)),
        "the out-of-flow-only Class-A box must still select @page full"
    );
    assert!(
        document.pages[2]
            .lines()
            .iter()
            .any(|line| line.text == "After")
    );
}

#[tokio::test]
async fn root_absolute_explicit_insets_remain_on_initial_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 0 }\
         body, p, section { margin: 0; font-size: 10pt; line-height: 10pt }\
         .source { break-before: page; height: 0 }\
         .source > div { position: absolute; top: 0; left: 0; width: 10pt; height: 10pt; background: red }\
         </style><p>First</p><section class=\"source\"><div></div></section><p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = CssColor::rgba(255, 0, 0, 1.0);
    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(red))
    );
    assert!(
        !document.pages[1]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(red)),
        "an explicit root absolute inset remains anchored to the first page's initial containing block"
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "After")
    );
}

#[tokio::test]
async fn positioned_auto_height_measurement_ignores_breaks_and_nested_positioned_children() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 0 }\
         body, div { margin: 0 }\
         .outer { position: absolute; top: 0; left: 0; width: 80pt; background: red }\
         .first { height: 20pt; background: black; break-after: right }\
         .second { height: 20pt; background: blue; break-inside: avoid }\
         .nested { position: absolute; top: 0; left: 0; width: 10pt; height: 80pt; background: green }\
         </style><div class=\"outer\"><div class=\"first\"></div><div class=\"second\"></div><div class=\"nested\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = CssColor::rgba(255, 0, 0, 1.0);
    let green = CssColor::rgba(0, 128, 0, 1.0);
    assert_eq!(
        document.pages.len(),
        3,
        "the final positioned layout, not its auto-size measurement, owns the right-page break"
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| { rect.fill == Some(red) && (rect.height() - 40.0).abs() < 0.01 }),
        "expected the positioned auto-height box to span its two in-flow children: {:#?}",
        document.pages[0].rects(),
    );
    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(green))
            .count(),
        1,
        "the nested positioned child is emitted only by final positioned layout"
    );
}

#[tokio::test]
#[ignore]
async fn book_sample_companion_stylesheet_preserves_right_page_breaks() {
    let stylesheet = Css::from_file("weasyprint-samples/book/book.css")
        .await
        .unwrap();
    let document = Html::from_file("weasyprint-samples/book/book.html")
        .await
        .unwrap()
        .with_stylesheet(stylesheet)
        .render(&RenderOptions::default())
        .await
        .unwrap();

    // Correctly including the chapter figure caption in its flex line removes
    // the prior overlap. The remaining two-page delta from the reference is
    // the separately tracked text-flow / font-metric divergence.
    assert_eq!(document.pages.len(), 58);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::BLACK)),
        "the first full-page source must materialize @page full"
    );
    assert!(
        !document.pages[0].images().is_empty(),
        "the cover's positioned image must remain paintable on the selected full page"
    );
    assert!(
        document.pages[2]
            .lines()
            .iter()
            .any(|line| line.text.contains("Écritures collaboratives"))
    );
    assert!(
        document.pages[6]
            .lines()
            .iter()
            .any(|line| line.text.contains("La course"))
    );
    let chapter_caption = document.pages[6]
        .lines()
        .iter()
        .find(|line| line.text == "Poire")
        .expect("chapter character caption");
    let first_paragraph = document.pages[6]
        .lines()
        .iter()
        .find(|line| line.text.starts_with("Et dire que je dois encore"))
        .expect("first chapter paragraph line");
    assert!(
        first_paragraph.y() <= chapter_caption.y() - 9.0,
        "caption_y={}, paragraph_y={}",
        chapter_caption.y(),
        first_paragraph.y()
    );
    let contents = document.pages[4]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for entry in [
        "1. La course",
        "2. Dos au mur",
        "3. Le temple",
        "4. Le grondement",
    ] {
        assert!(contents.contains(entry), "missing {entry:?}: {contents:?}");
    }
    let rendered_lines = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    for phrase in ["moi\u{202f}?", "Mince\u{202f}!"] {
        assert!(
            rendered_lines.iter().any(|line| line.contains(phrase)),
            "U+202F must keep French punctuation with its preceding text: {phrase:?}"
        );
    }
}

#[tokio::test]
async fn auto_height_row_flex_uses_final_block_stack_cross_contribution() {
    let document = Html::from_string(
        r#"<style>
             @page { size: 160pt 160pt; margin: 0 }
             html, body { margin: 0; font: 10pt/10pt sans-serif }
             #row { display: flex; width: 100pt }
             figure { margin: 0; padding: 0 }
             .portrait { width: 20pt; height: 40pt }
             figcaption, p { display: block; margin: 0; line-height: 10pt }
           </style>
           <div id="row"><figure><div class="portrait"></div><figcaption>Caption</figcaption></figure></div>
           <p>Following</p>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let caption = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Caption")
        .expect("caption line");
    let following = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Following")
        .expect("following line");
    assert!(
        following.y() <= caption.y() - 9.9,
        "caption_y={}, following_y={}",
        caption.y(),
        following.y()
    );
}

#[tokio::test]
async fn auto_height_row_flex_remeasures_wrapped_caption_after_flexing() {
    let document = Html::from_string(
        r#"<style>
             @page { size: 160pt 160pt; margin: 0 }
             html, body { margin: 0; font: 10pt/10pt sans-serif }
             #row { display: flex; width: 90pt }
             figure { flex: 1; margin: 0; padding: 0 }
             .side { flex: none; width: 40pt; height: 1pt }
             figcaption, p { display: block; margin: 0; line-height: 10pt }
           </style>
           <div id="row"><figure><figcaption>one two three four five</figcaption></figure><div class="side"></div></div>
           <p>Following</p>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let lines = document.pages[0].lines();
    let caption_lines = lines
        .iter()
        .filter(|line| matches!(line.text.as_str(), "one two" | "three four" | "five"))
        .collect::<Vec<_>>();
    let following = lines
        .iter()
        .find(|line| line.text == "Following")
        .expect("following line");
    assert!(caption_lines.len() >= 2, "lines={lines:?}");
    let last_caption = caption_lines
        .iter()
        .min_by(|left, right| left.y().partial_cmp(&right.y()).unwrap())
        .expect("caption lines");
    assert!(
        following.y() <= last_caption.y() - 9.9,
        "the following sibling must begin below the wrapped caption: lines={lines:?}"
    );
}

#[tokio::test]
async fn normal_generated_target_references_converge_with_target_page_and_counter() {
    let document = Html::from_string(
        "<nav><a href=\"#chapter\"></a><a class=\"external\" href=\"https://example.test/chapter\"></a></nav><section><h2 id=\"chapter\">Destination</h2></section>\
         <style>@page { size: 120pt 80pt; margin: 10pt }\
         body, nav, section, h2 { margin: 0; font: 10pt/10pt sans-serif }\
         section { counter-reset: chapter 10 }\
         h2 { counter-increment: chapter; break-before: page }\
         h2::before { content: \"Before\" } h2::after { content: \"After\" }\
         a::before { content: target-counter(attr(href), chapter) \": \" target-text(attr(href)) \" / \" target-text(attr(href), before) \" / \" target-text(attr(href), after) }\
         a.external::before { content: \"external=\" target-text(attr(href)) }\
         a::after { content: \" @\" target-counter(attr(href), page) }</style>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    let contents = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(
        contents.contains("11: Destination /")
            && contents.contains("Before / After")
            && contents.contains("@2")
            && contents.contains("external= @"),
        "contents={contents:?}"
    );
}

#[tokio::test]
async fn report_sample_cover_preserves_page_height_flex_line_packing() {
    let document = Html::from_file("weasyprint-samples/report/report.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages.len(), 8);
    let contents = document.pages[2]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for entry in [
        "Big title on left page, with text on columns",
        "This is another big title, on a page full of work presentation",
        "Big title on the first right page",
        "About some typography features",
    ] {
        assert!(contents.contains(entry), "missing {entry:?}: {contents:?}");
    }
    let page = &document.pages[0];
    assert!((page.width() - 595.2756).abs() < 0.01, "page={page:?}");
    assert!((page.height() - 841.8898).abs() < 0.01, "page={page:?}");

    let orange_addresses = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(251, 200, 71)) && rect.height() > 100.0)
        .collect::<Vec<_>>();
    assert_eq!(
        orange_addresses.len(),
        2,
        "the cover has two independently painted address backgrounds: {orange_addresses:?}"
    );
    assert!(
        orange_addresses.iter().all(|rect| rect.y().abs() < 0.01),
        "the address line must retain its align-content: space-between block-end placement: {orange_addresses:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "Two")
    );
    assert!(
        !document
            .pages
            .iter()
            .any(|page| page.lines().iter().any(|line| line.text == "blank"))
    );
}

#[tokio::test]
async fn page_margin_boxes_inherit_page_font_properties() {
    let document = Html::from_file("weasyprint-samples/invoice/invoice.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let thank = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("Thank you!"))
        .unwrap();
    let contact = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("contact@courtbouillon.org"))
        .unwrap();

    assert_eq!(thank.color, CssColor::new(30, 228, 148));
    assert!(line_has_font_containing(&document, thank, "pacifico"));
    assert_eq!(contact.color, CssColor::new(170, 153, 170));
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let footer = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Footer")
        .unwrap();
    let small = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Small")
        .unwrap();

    assert!((footer.font_size - 11.0).abs() < 0.01);
    assert!((small.font_size - 9.0).abs() < 0.01);
}

fn line_has_font_containing(
    document: &quire::Document,
    line: &crate::document::paint::text::RenderedLine,
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
        .render(&RenderOptions::default())
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
    .render(&RenderOptions::default())
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
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!((document.pages[0].width() - 841.8898).abs() < 0.001);
    assert!((document.pages[0].height() - 595.2756).abs() < 0.001);
}

#[tokio::test]
async fn invalid_page_size_descriptor_does_not_partially_apply_lengths() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string("@page { size: 5in landscape }"))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!((document.pages[0].width() - crate::layout::PageSize::A4_POINTS.width()).abs() < 0.001);
    assert!(
        (document.pages[0].height() - crate::layout::PageSize::A4_POINTS.height()).abs() < 0.001
    );
}

#[tokio::test]
async fn page_orientation_descriptor_emits_pdf_rotation() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string("@page { page-orientation: rotate-left }"))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].rotation, 270);
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(pdf_searchable_text(&pdf).contains("/Rotate 270"));
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    let white = CssColor::WHITE;
    let blue = CssColor::new(0, 0, 255);
    let yellow = CssColor::new(255, 255, 0);
    for (page, margin_color) in [(&document.pages[0], white), (&document.pages[1], blue)] {
        assert!(page.rects().iter().any(|rect| {
            rect.x() == 0.0
                && rect.y() == 0.0
                && (rect.width() - 300.0).abs() < 0.01
                && (rect.height() - 300.0).abs() < 0.01
                && rect.fill == Some(margin_color)
        }));
        assert!(page.rects().iter().any(|rect| {
            (rect.x() - 37.5).abs() < 0.01
                && (rect.y() - 37.5).abs() < 0.01
                && (rect.width() - 225.0).abs() < 0.01
                && (rect.height() - 225.0).abs() < 0.01
                && rect.fill == Some(yellow)
        }));
    }
}

#[tokio::test]
async fn propagated_body_canvas_background_image_does_not_reanchor_on_forced_page() {
    let document = Html::from_string(
        "<!doctype html>\
         <style>\
         @page { size: 120pt 120pt; margin: 0 }\
         body { margin-left: 100px; background: url(data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC) no-repeat }\
         </style>\
         A<div style=\"break-before: page\">B</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document.pages[0]
            .images()
            .iter()
            .filter(|image| image.background)
            .count(),
        1
    );
    assert!(
        !document.pages[1]
            .images()
            .iter()
            .any(|image| image.background)
    );
}

#[tokio::test]
async fn root_canvas_background_image_does_not_reanchor_on_forced_page() {
    let document = Html::from_string(
        "<!doctype html>\
         <style>\
         @page { size: 120pt 120pt; margin: 0 }\
         html { background: url(data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC) no-repeat }\
         body { margin: 0 }\
         </style>\
         A<div style=\"break-before: page\">B</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document.pages[0]
            .images()
            .iter()
            .filter(|image| image.background)
            .count(),
        1
    );
    assert!(
        !document.pages[1]
            .images()
            .iter()
            .any(|image| image.background)
    );
}

#[tokio::test]
async fn repeated_canvas_background_image_continues_onto_forced_page() {
    let document = Html::from_string(
        "<!doctype html>\
         <style>\
         @page { size: 120pt 120pt; margin: 0 }\
         body { margin: 0; background-image: url(data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC); background-repeat: repeat-y; background-size: 10pt 10pt }\
         </style>\
         A<div style=\"break-before: page\">B</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .image_patterns()
            .iter()
            .any(|pattern| pattern.background)
    );
    assert!(
        document.pages[1]
            .image_patterns()
            .iter()
            .any(|pattern| pattern.background)
    );
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-page/page-margin-007-print.html.
    assert_eq!(document.pages.len(), 7);
    for page in &document.pages {
        assert!(
            !page.lines().is_empty(),
            "page-margin-007 should not generate empty intermediate pages"
        );
    }
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "There should be 7 pages.")
    );
    for page_index in [1, 3, 5] {
        assert!(
            document.pages[page_index]
                .lines()
                .iter()
                .any(|line| line.text == "Margins on every side."),
            "expected left-page content on page {}",
            page_index + 1
        );
    }
    for page_index in [2, 4, 6] {
        let no_margin_lines = document.pages[page_index]
            .lines()
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
                .lines()
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
            .render(&RenderOptions::default())
            .await
            .unwrap();
        let reference = Html::from_string(reference_html)
            .render(&RenderOptions::default())
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
async fn page_margin_vertical_fixed_box_uses_logical_inline_axis() {
    let render = |writing_mode: &str, vertical_align: &str| {
        Html::from_string(format!(
            "<style>\
             @page {{ size: 120px 120px; margin: 40px;\
               @top-left-corner {{ content: \"A\"; writing-mode: {writing_mode}; text-orientation: upright;\
                 font: 20px/1 sans-serif; text-align: left; vertical-align: {vertical_align}; }}\
             }}\
             body {{ margin: 0 }}\
             </style>"
        ))
    };
    let line_position = |document: &quire::Document| {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == "A")
            .expect("vertical page-margin text should render");
        (line.x(), line.y())
    };

    let rl_top = render("vertical-rl", "top")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let rl_bottom = render("vertical-rl", "bottom")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let lr_top = render("vertical-lr", "top")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let lr_bottom = render("vertical-lr", "bottom")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let (rl_top_x, rl_top_y) = line_position(&rl_top);
    let (_, rl_bottom_y) = line_position(&rl_bottom);
    let (lr_top_x, lr_top_y) = line_position(&lr_top);
    let (_, lr_bottom_y) = line_position(&lr_bottom);

    assert!(
        rl_top_x > lr_top_x + 10.0,
        "vertical-rl should start at the physical right side of the corner; rl={rl_top_x} lr={lr_top_x}"
    );
    assert!(
        rl_top_y > rl_bottom_y + 10.0,
        "vertical-align should move vertical-rl content along the physical inline axis; top={rl_top_y} bottom={rl_bottom_y}"
    );
    assert!(
        lr_top_y > lr_bottom_y + 10.0,
        "vertical-align should move vertical-lr content along the physical inline axis; top={lr_top_y} bottom={lr_bottom_y}"
    );
}

#[tokio::test]
async fn page_margin_box_dimensions_match_wpt_auto_lengths_with_corners() {
    let document = Html::from_string(
        "<style>\
         @page { margin: 4em 5em 8em 7em; width: 20em; height: 15em; font: 16px/1 sans-serif; white-space: pre-wrap;\
           @top-left-corner { writing-mode: vertical-rl; text-align: left; vertical-align: bottom; content: \"x\\ax\"; border: solid thin; }\
           @top-right-corner { text-align: left; vertical-align: top; content: \"xx\"; border: solid thin; }\
           @bottom-right-corner { text-align: left; vertical-align: top; content: \"xxx\"; border: solid thin; }\
           @bottom-left-corner { text-align: left; vertical-align: top; content: \"xxxx\"; border: solid thin; }\
           @top-left { text-align: left; vertical-align: top; margin-bottom: 2em; content: \"x\"; background: hotpink; }\
           @top-right { text-align: left; vertical-align: top; margin-top: 2em; content: \"xxx\"; background: yellow; }\
           @left-top { text-align: left; vertical-align: top; margin-left: 4em; content: \"x\\ax\\ax\\a\"; background: yellow; }\
           @left-bottom { text-align: left; vertical-align: top; margin-right: 4em; content: \"x\\ax\\a\"; background: hotpink; }\
           @right-top { text-align: left; vertical-align: top; margin-left: 3em; content: \"x\\ax\\a\"; background: hotpink; }\
           @right-bottom { text-align: left; vertical-align: top; margin-right: 3em; content: \"xxx\"; background: yellow; }\
           @bottom-left { text-align: left; vertical-align: top; margin-top: 4em; content: \"x x x x\"; background: yellow; }\
           @bottom-right { text-align: left; vertical-align: top; margin-bottom: 4em; content: \"x\"; background: hotpink; }\
         }\
         body { margin: 0 }\
         </style>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let top_left_corner_lines = page
        .lines()
        .iter()
        .filter(|line| line.text == "x" && line.x() < 120.0 && (280.0..300.0).contains(&line.y()))
        .count();
    assert_eq!(
        top_left_corner_lines,
        2,
        "top-left-corner vertical content should stay inside the upper-left corner: {:?}",
        page.lines()
    );
    assert!(
        page.rects().iter().any(|rect| {
            rect.fill == Some(CssColor::new(255, 255, 0))
                && (rect.x() - 48.0).abs() < 0.5
                && (rect.y() - 168.0).abs() < 0.5
                && (rect.height() - 108.0).abs() < 0.5
        }),
        "left-top page-margin box should retain its expected top-side placement: {:?}",
        page.rects()
    );
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
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let page = &document.pages[0];
    assert_eq!(page.width(), 384.0);
    assert_eq!(page.height(), 336.0);
    assert!(
        page.rects().iter().any(|rect| {
            (rect.x() - 72.0).abs() < 0.01
                && (rect.y() - 264.0).abs() < 0.01
                && (rect.width() - 120.0).abs() < 0.01
                && (rect.height() - 12.0).abs() < 0.01
                && rect.fill == Some(CssColor::BLACK)
        }),
        "top-left border box should exclude its trailing margin from background paint"
    );
    assert!(
        page.rects().iter().any(|rect| {
            (rect.x() - 240.0).abs() < 0.01
                && (rect.y() - 264.0).abs() < 0.01
                && (rect.width() - 72.0).abs() < 0.01
                && (rect.height() - 72.0).abs() < 0.01
                && rect.fill == Some(CssColor::new(255, 255, 0))
        }),
        "top-right border box should exclude its leading margin from background paint"
    );
    assert!(page.lines().iter().any(|line| {
        line.text == "x" && line.x() >= 72.0 && line.x() < 100.0 && line.y() >= 260.0
    }));
    assert!(page.lines().iter().any(|line| {
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
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let left = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(10, 20, 30)))
        .expect("expected top-left margin box background");
    let right = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(30, 20, 10)))
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
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let width_for = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("expected page-margin rect with color {color:?}"))
            .width()
    };
    let min = width_for(CssColor::new(0, 128, 0));
    let fit = width_for(CssColor::new(0, 0, 255));
    let max = width_for(CssColor::new(0, 0, 0));

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
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let page_margin_lines = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut margin_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "A\tB")
        .collect::<Vec<_>>();
    assert_eq!(margin_lines.len(), 2, "{:?}", document.pages[0].lines());
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let margin_lines = document.pages[0]
        .lines()
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

#[tokio::test]
async fn footnote_follows_the_fragment_containing_its_committed_call_line() {
    let document = Html::from_string(
        r#"<style>
              @page { size: 200pt 200pt; margin: 20pt; }
              body, p { margin: 0 }
              .lead { height: 145pt }
              .note { float: footnote }
              .note::footnote-call { content: "*" }
            </style>
            <p class="lead">lead</p><p>Call<span class="note">second-page note</span></p>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    let first_page_text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let second_page_text = document.pages[1]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !first_page_text.contains("second-page note"),
        "{first_page_text:?}"
    );
    assert_eq!(second_page_text.matches("second-page note").count(), 1);
}

#[tokio::test]
async fn table_footnote_commits_once_after_speculative_cell_layout() {
    let document = Html::from_string(
        r#"<style>
              @page { size: 200pt 200pt; margin: 20pt; }
              body, table { margin: 0; border-spacing: 0 }
              td { padding: 0; font: 10pt/12pt sans-serif }
              .note { float: footnote }
              .note::footnote-call { content: "*" }
            </style>
            <table>
              <tr><td>Alpha<span class="note">table note</span></td></tr>
              <tr><td>Beta</td></tr>
              <tr><td>Gamma</td></tr>
            </table>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text.matches("table note").count(), 1, "{text:?}");
}

#[tokio::test]
async fn taiwanese_numerals_footnote_does_not_fragment_the_table() {
    let document = Html::from_string(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/taiwanese-numerals.html"
    )))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(
        text.matches(
            "表中熟蕃種族名ノ左肩ニ＊ノ記號ヲ施セルハ其ノ言語ノ殆ンド死語トナルコトヲ示ス"
        )
        .count(),
        1
    );
}

fn rounded_page_rects(page: &quire::Page) -> Vec<(i32, i32, i32, i32)> {
    let mut rects = page
        .rects()
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
        .lines()
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
