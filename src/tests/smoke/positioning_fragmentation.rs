use super::*;

fn assert_no_red_or_hotpink_rects(document: &quire::Document) {
    let red = CssColor::new(255, 0, 0);
    let hotpink = CssColor::new(255, 105, 180);
    let painted = document
        .pages
        .iter()
        .flat_map(|page| page.rects())
        .filter(|rect| matches!(rect.fill, Some(color) if color == red || color == hotpink))
        .collect::<Vec<_>>();
    assert!(
        painted.is_empty(),
        "self-collapsing empty blocks should not paint red/hotpink rects: {painted:?}"
    );
}

fn page_has_line(page: &quire::Page, text: &str) -> bool {
    page.lines().iter().any(|line| line.text == text)
}

#[tokio::test]
async fn layout_containment_captures_nested_fixed_descendant() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 200pt; margin: 10pt } body, div { margin: 0 }\
         .container { contain: layout; margin: 20pt; width: 60pt; height: 60pt; background: red }\
         .fixed { position: fixed; inset: 0; width: 60pt; height: 60pt; background: lime }\
         </style><div class=\"container\"><div><div class=\"fixed\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rect(&document.pages[0], CssColor::new(0, 255, 0));
    assert!((green.x() - 30.0).abs() < 0.01, "{green:?}");
    assert!((green.width() - 60.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn supports_absolute_positioned_blocks() {
    let options = RenderOptions::default();
    let document = Html::from_string(
        "<p style=\"margin: 0\">Flow</p><div style=\"position: absolute; left: 20pt; top: 30pt; margin: 0\">Abs</div><p style=\"margin: 0\">After</p>",
    )
    .render(&options).await
    .unwrap();

    let flow = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Flow")
        .unwrap();
    let abs = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Abs")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert_eq!(flow.text, "Flow");
    assert_eq!(abs.text, "Abs");
    assert_eq!(abs.x(), options.page_margins.left() + 20.0);
    assert_line_baseline_at_top(
        &document,
        abs,
        // CSS Positioned Layout places the block container's border box at
        // `top`; its first line box starts at the content edge. The line
        // height's leading expands that line box rather than moving the
        // positioned box above its used inset.
        options.page_size.height() - options.page_margins.top() - 30.0,
    );
    assert_eq!(after.text, "After");
    assert!(after.y() < flow.y());
}

#[tokio::test]
async fn percentage_block_width_uses_destination_page_size_after_prebreak() {
    let document = Html::from_string(
        "<style>\
         @page { size: 320px 200px; margin: 0 }\
         @page :first { size: 500px }\
         html, body { margin: 0 }\
         </style>\
         <div style=\"width:50%; height:500px; background:yellow\">first page</div>\
         <div style=\"width:50%; height:200px; background:cyan\">second page</div>\
         <div style=\"width:50%; height:200px; background:pink\">third page</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let yellow = filled_rect(&document.pages[0], CssColor::new(255, 255, 0));
    let cyan = filled_rect(&document.pages[1], CssColor::new(0, 255, 255));
    let pink = filled_rect(&document.pages[2], CssColor::new(255, 192, 203));

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].width(), 375.0);
    assert_eq!(document.pages[1].width(), 240.0);
    assert_eq!(document.pages[2].width(), 240.0);
    assert!((yellow.width() - 187.5).abs() < 0.01);
    assert!(
        (cyan.width() - 120.0).abs() < 0.01,
        "viewport width should resolve against destination page: {cyan:?}"
    );
    assert!((pink.width() - 120.0).abs() < 0.01);
}

#[tokio::test]
async fn percentage_flex_width_uses_destination_page_size_after_prebreak() {
    let document = Html::from_string(
        "<style>\
         @page { size: 320px 200px; margin: 0 }\
         @page :first { size: 500px }\
         html, body { margin: 0 }\
         .spacer { height: 450px; background: yellow }\
         .flex { display: flex; width: 50%; height: 200px; background: cyan }\
         </style>\
         <div class=\"spacer\"></div><div class=\"flex\"><span>item</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let cyan = filled_rect(&document.pages[1], CssColor::new(0, 255, 255));

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[1].width(), 240.0);
    assert!((cyan.width() - 120.0).abs() < 0.01);
}

#[tokio::test]
async fn percentage_hr_width_uses_destination_page_size_after_prebreak() {
    let document = Html::from_string(
        "<style>\
         @page { size: 320px 200px; margin: 0 }\
         @page :first { size: 500px }\
         html, body { margin: 0 }\
         .spacer { height: 450px; background: yellow }\
         hr { border: 0; margin: 0; width: 50%; height: 200px; background: cyan }\
         </style>\
         <div class=\"spacer\"></div><hr>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let cyan = filled_rect(&document.pages[1], CssColor::new(0, 255, 255));

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[1].width(), 240.0);
    assert!((cyan.width() - 120.0).abs() < 0.01);
}

#[tokio::test]
async fn viewport_width_units_use_destination_page_size_after_prebreak() {
    let document = Html::from_string(
        "<style>\
         @page { size: 320px 200px; margin: 0 }\
         @page :first { size: 500px }\
         html, body { margin: 0 }\
         </style>\
         <div style=\"width:50%; height:500px; background:yellow\"></div>\
         <div style=\"width:50vw; height:200px; background:cyan\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let cyan = filled_rect(&document.pages[1], CssColor::new(0, 255, 255));

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[1].width(), 240.0);
    assert!(
        (cyan.width() - 120.0).abs() < 0.01,
        "viewport width should resolve against destination page: {cyan:?}"
    );
}

#[tokio::test]
async fn supports_positioned_right_and_bottom_offsets() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 10pt }</style><div style=\"position: absolute; right: 20pt; bottom: 30pt; width: 50pt; height: 20pt; margin: 0; font-size: 10pt; line-height: 10pt\">Box</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].x(), 120.0);
    assert_line_baseline_at_top(&document, &document.pages[0].lines()[0], 60.0);
}

#[tokio::test]
async fn fixed_generated_before_and_after_boxes_paint_at_page_bottom_right() {
    let document = Html::from_string(
        r#"
        <style>
          @page { size: 200pt 200pt; margin: 0 }
          html, body { margin: 0 }
          #test::before,
          #test::after {
            background: blue;
            bottom: 0;
            content: "";
            height: 100pt;
            position: fixed;
            right: 0;
            width: 50pt;
          }
          #test::before {
            right: 50pt;
          }
        </style>
        <p>Test passes if there is a square at the bottom right of the page.</p>
        <div id="test"></div>
        "#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut blue_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();
    blue_rects.sort_by(|left, right| left.x().total_cmp(&right.x()));

    assert_eq!(blue_rects.len(), 2, "blue rects: {blue_rects:?}");
    assert!((blue_rects[0].x() - 100.0).abs() < 0.01);
    assert!((blue_rects[1].x() - 150.0).abs() < 0.01);
    for rect in blue_rects {
        assert!((rect.y() - 0.0).abs() < 0.01, "{rect:?}");
        assert!((rect.width() - 50.0).abs() < 0.01, "{rect:?}");
        assert!((rect.height() - 100.0).abs() < 0.01, "{rect:?}");
    }
}

#[tokio::test]
async fn absolute_positioned_table_bottom_anchors_border_box_to_page_area() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 240pt; margin: 20pt } body { margin: 0; font-size: 10pt; line-height: 10pt } footer { height: 60pt } table#total { position: absolute; bottom: 0; margin: 0; width: 100pt; border: 10pt solid #eeeeee; background: #eeeeee; border-collapse: collapse } td { padding: 0 }</style>\
         <p style=\"margin:0\">Before</p><footer><table id=\"total\"><tr><td>Total</td></tr></table></footer>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let table_background = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(238, 238, 238)))
        .max_by(|left, right| left.width().total_cmp(&right.width()))
        .unwrap();

    assert!(
        (table_background.y() - 20.0).abs() < 0.01,
        "expected table border box bottom at y=20, got {:?}",
        table_background
    );
}

#[tokio::test]
async fn generated_before_and_after_content_participates_in_inline_layout() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 140pt; margin: 10pt } body, dl { margin: 0; font-size: 10pt; line-height: 10pt } dt, dd { display: inline; margin: 0 } dt::before { content: \"\"; display: block } dt::after { content: \":\" }</style>\
         <dl><dt>Invoice number</dt><dd>12345</dd><dt>Date</dt><dd>March 31, 2018</dd></dl>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    let invoice = lines
        .iter()
        .find(|line| line.text == "Invoice number:12345")
        .unwrap();
    let date = lines
        .iter()
        .find(|line| line.text == "Date:March 31, 2018")
        .unwrap();

    assert!(date.y() < invoice.y());
}

#[tokio::test]
async fn absolute_position_static_auto_offsets_start_at_containing_block() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } div { position: absolute; margin: 0 }</style><div>Auto</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "Auto");
    assert!((document.pages[0].lines()[0].x() - 10.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, &document.pages[0].lines()[0], 110.0);
}

#[tokio::test]
async fn absolute_position_applies_non_auto_margins_to_border_edge() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } div { position: absolute; left: 20pt; top: 15pt; margin: 5pt 0 0 7pt }</style><div>Margin</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Margin")
        .unwrap();

    assert!((line.x() - 37.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, line, 90.0);
}

#[tokio::test]
async fn absolute_position_auto_offsets_use_static_position_after_flow() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body, p, div { margin: 0; font-size: 10pt; line-height: 10pt } .abs { position: absolute }</style><p>Flow</p><div class=\"abs\">Auto</div><p>After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let flow = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Flow")
        .unwrap();
    let auto = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Auto")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert_line_baseline_at_top(&document, flow, 110.0);
    assert!((auto.x() - 10.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, auto, 100.0);
    assert_line_baseline_at_top(&document, after, 100.0);
}

#[tokio::test]
async fn block_level_abspos_is_not_dispatched_by_raw_inline_collection() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 400px 300px; margin: 0 }\
         body, p, div { margin: 0; font: 20px/20px monospace }\
         .probe { position: absolute; width: 100px; height: 100px; background: rgb(0, 128, 0); color: red; z-index: -1 }\
         </style>\
         <p>Test <strong>paragraph</strong></p>\
         <div class=\"probe\">X XXX<br>XX</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    assert_eq!(
        green_rects.len(),
        1,
        "green positioned probes: {green_rects:?}"
    );

    let green = green_rects[0];
    // The static position is the start of the block formatting box following
    // the 20px preceding paragraph, expressed in the rendered PDF coordinate
    // system. A raw-inline dispatch would instead place a second probe at the
    // parent's initial cursor.
    assert!((green.y() - 133.10303).abs() < 0.01, "{green:?}");
    assert_eq!(
        page.lines()
            .iter()
            .filter(|line| line.text == "X XXX" || line.text == "XX")
            .count(),
        2,
        "positioned probe text should be emitted once: {:?}",
        page.lines(),
    );
}

#[tokio::test]
async fn absolute_block_level_abspos_static_position_after_inline_content() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 300px 220px; margin: 0 }\
         body, div { margin: 0; font-size: 10px; line-height: 10px }\
         .rtl { direction: rtl }\
         .absolute { position: absolute }\
         </style>\
         <div><div class=\"absolute\">abs before ltr</div><span>inline before ltr</span></div>\
         <div><span>inline after ltr</span><div class=\"absolute\">abs after ltr</div></div>\
         <br>\
         <div class=\"rtl\"><div class=\"absolute\">abs before rtl</div><span>inline before rtl</span></div>\
         <div class=\"rtl\"><span>inline after rtl</span><div class=\"absolute\">abs after rtl</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("missing line {text:?}: {:?}", document.pages[0].lines()))
    };

    let inline_before_ltr = line("inline before ltr");
    let abs_before_ltr = line("abs before ltr");
    let inline_after_ltr = line("inline after ltr");
    let abs_after_ltr = line("abs after ltr");
    let inline_before_rtl = line("inline before rtl");
    let abs_before_rtl = line("abs before rtl");
    let inline_after_rtl = line("inline after rtl");
    let abs_after_rtl = line("abs after rtl");
    let line_right = |line: &quire::RenderedLine| line.x() + rendered_line_advance(line);
    assert!((abs_before_ltr.y() - inline_before_ltr.y()).abs() < 0.01);
    assert!(
        (abs_before_ltr.x() - inline_before_ltr.x()).abs() < 0.01,
        "ltr before abspos should share the inline-start left edge: inline={inline_before_ltr:?} abs={abs_before_ltr:?}",
    );
    assert!(
        abs_after_ltr.y() < inline_after_ltr.y() - 7.0,
        "ltr after abspos should be below inline content: inline={inline_after_ltr:?} abs={abs_after_ltr:?}",
    );
    assert!(
        (abs_after_ltr.x() - inline_after_ltr.x()).abs() < 0.01,
        "ltr after abspos should share the inline-start left edge: inline={inline_after_ltr:?} abs={abs_after_ltr:?}",
    );
    assert!((abs_before_rtl.y() - inline_before_rtl.y()).abs() < 0.01);
    assert!(
        (line_right(abs_before_rtl) - line_right(inline_before_rtl)).abs() < 0.01,
        "rtl before abspos should share the inline-start right edge: inline={inline_before_rtl:?} abs={abs_before_rtl:?}",
    );
    assert!(
        abs_after_rtl.y() < inline_after_rtl.y() - 7.0,
        "rtl after abspos should be below inline content: inline={inline_after_rtl:?} abs={abs_after_rtl:?}",
    );
    assert!(
        (line_right(abs_after_rtl) - line_right(inline_after_rtl)).abs() < 0.01,
        "rtl after abspos should share the inline-start right edge: inline={inline_after_rtl:?} abs={abs_after_rtl:?}",
    );
}

#[tokio::test]
async fn inline_origin_abspos_before_forced_break_uses_placeholder_static_position_ltr_and_rtl() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 360pt 180pt; margin: 0 }\
         body, div { margin: 0; font: 10pt/10pt monospace }\
         .case { width: 220pt }\
         .rtl { direction: rtl }\
         .absolute { position: absolute }\
         .green { background-color: lime; padding: 0 1ch }\
         </style>\
         <div class=\"case\">\
           <span class=\"absolute green\">LTRABS</span>\
           <br>\
         </div>\
         <div class=\"case\">LTRFOLLOW</div>\
         <div class=\"case rtl\">\
           <span class=\"absolute green\">RTLABS</span>\
           <br>\
         </div>\
         <div class=\"case rtl\">RTLFOLLOW</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("missing line {text:?}: {:?}", document.pages[0].lines()))
    };

    let ltr_abs = line("LTRABS");
    let ltr_following = line("LTRFOLLOW");
    let rtl_abs = line("RTLABS");
    let rtl_following = line("RTLFOLLOW");

    assert!(
        ltr_following.y() < ltr_abs.y() - 7.0,
        "ltr following line should be below the terminal br line: abs={ltr_abs:?} following={ltr_following:?}",
    );
    assert!(
        rtl_following.y() < rtl_abs.y() - 7.0,
        "rtl following line should be below the terminal br line: abs={rtl_abs:?} following={rtl_following:?}",
    );
    assert!(
        ltr_abs.x() < 20.0,
        "ltr abspos should use the inline-start placeholder at the left edge: {ltr_abs:?}",
    );
    assert!(
        rtl_abs.x() > 80.0,
        "rtl abspos should use the inline-start placeholder at the right edge: {rtl_abs:?}",
    );
}

#[tokio::test]
async fn terminal_br_after_inline_abspos_keeps_following_inline_content_visible() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 500pt 180pt; margin: 0 }\
         body, div { margin: 0; font: 10pt/10pt sans-serif }\
         .rtl { direction: rtl }\
         .absolute { position: absolute }\
         .green { background-color: lime; padding: 0 1ch }\
         </style>\
         <body>\
           <div>\
             <span class=\"absolute green\">Block-level abspos before inline content</span>\
             <br>\
           </div>\
           <div>\
             <div>Inline content</div>\
             <div>Block-level abspos after inline content</div>\
           </div>\
           <div class=\"rtl\">\
             <span class=\"absolute green\">Block-level abspos before inline content</span>\
             <br>\
           </div>\
           <div class=\"rtl\">\
             <div>Inline content</div>\
             <div>Block-level abspos after inline content</div>\
           </div>\
         </body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut abs_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "Block-level abspos before inline content")
        .collect::<Vec<_>>();
    let mut inline_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "Inline content")
        .collect::<Vec<_>>();
    abs_lines.sort_by(|a, b| b.y().total_cmp(&a.y()));
    inline_lines.sort_by(|a, b| b.y().total_cmp(&a.y()));

    assert_eq!(abs_lines.len(), 2, "absolute lines: {abs_lines:?}");
    assert_eq!(inline_lines.len(), 2, "inline lines: {inline_lines:?}");
    for (abs, inline) in abs_lines.iter().zip(inline_lines.iter()) {
        assert!(
            inline.y() < abs.y() - 7.0,
            "inline content should be below the terminal br line instead of covered: abs={abs:?} inline={inline:?}",
        );
    }
}

#[tokio::test]
async fn auto_positioned_abspos_after_text_before_float() {
    for (case, html) in [
        (
            "normal",
            String::from(
                "<!DOCTYPE html>\
             <style>@page { size: 260px 260px; margin: 0 } body, p, div { margin: 0 }</style>\
             <p style=\"display:none\">Test passes if there is a filled green square and no red.</p>\
             <div style=\"line-height:20px;\">\
               &nbsp;\
               <div style=\"position:absolute; width:200px; height:200px; background:green;\"></div>\
               <div style=\"float:left; margin-top:20px; width:200px; height:200px; background:red;\"></div>\
             </div>",
            ),
        ),
        (
            "negative-margin",
            String::from(
                "<!DOCTYPE html>\
                 <style>@page { size: 800px 600px; margin: 0 }</style>\
                 <p>Test passes if there is a filled green square and <strong>no red</strong>.</p>\
                 <div style=\"line-height:20px; margin-top:-20px;\">\
                   &nbsp;\
                   <div style=\"position:absolute; width:200px; height:200px; background:green;\"></div>\
                   <div style=\"float:left; margin-top:20px; width:200px; height:200px; background:red;\"></div>\
                 </div>",
            ),
        ),
    ] {
        let document = Html::from_string(html)
            .render(&RenderOptions::default())
            .await
            .unwrap();

        let page = &document.pages[0];
        let red_index = page
            .rects()
            .iter()
            .position(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .expect("red float rectangle should be painted");
        let green_index = page
            .rects()
            .iter()
            .position(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .unwrap_or_else(|| {
                panic!(
                    "green abspos rectangle should be painted: {:?}",
                    page.rects()
                )
            });
        let red = &page.rects()[red_index];
        let green = &page.rects()[green_index];

        assert!(
            (green.x() - red.x()).abs() < 0.01
                && (green.y() - red.y()).abs() < 0.01
                && (green.width() - red.width()).abs() < 0.01
                && (green.height() - red.height()).abs() < 0.01,
            "{case}: green abspos should cover the later float: red={red:?} green={green:?}",
        );
        assert!(
            first_rect_paint_operation_index(page, CssColor::new(0, 128, 0))
                > first_rect_paint_operation_index(page, CssColor::new(255, 0, 0)),
            "{case}: green abspos should paint after red: operations={:?} rects={:?}",
            page.paint_operations(),
            page.rects(),
        );
    }
}

#[tokio::test]
async fn inline_abspos_static_position_after_forced_break_uses_second_line() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 300px 160px; margin: 0 } body { margin: 0; font-size: 20px; line-height: 20px }</style>\
         <span>Line 1<br><span style=\"position:absolute\">Line 2</span></span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("missing line {text:?}: {:?}", document.pages[0].lines()))
    };
    let first = line("Line 1");
    let second = line("Line 2");

    assert!(
        (first.y() - second.y() - 15.0).abs() < 0.01,
        "abspos inline static position should use the line after the forced break: first={first:?} second={second:?}",
    );
}

#[tokio::test]
async fn inline_abspos_static_position_after_inline_text_stays_on_same_line() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 300px 160px; margin: 0 } body { margin: 0; font-size: 20px; line-height: 20px }</style>\
         <span>Line 1 <span style=\"position:absolute\">Line 2</span></span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("missing line {text:?}: {:?}", document.pages[0].lines()))
    };
    let first = line("Line 1");
    let second = line("Line 2");

    assert!(
        (first.y() - second.y()).abs() < 0.01,
        "abspos inline without a forced break should keep the existing line static position: first={first:?} second={second:?}",
    );
}

#[tokio::test]
async fn inline_block_abspos_static_position_uses_margin_box_top() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 500px 200px; margin: 0 }\
         body { margin: 0 }\
         div { background: blue; margin: 1em 0; border: 1px solid black; height: 8em; width: 30em; position: relative }\
         span { background: yellow; margin: 1em 0; width: 6em; max-width: 6em; height: 6em; position: absolute; left: 7em; display: inline-block }\
         span:nth-child(2) { background: pink; left: 15em }\
         span:nth-child(3) { background: lightblue; left: 23em }\
         </style>\
         <div><span>one</span><span>two</span><span>three</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color: CssColor| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("missing {color:?} rect: {:?}", page.rects()))
    };
    let parent = rect(CssColor::new(0, 0, 255));
    let parent_content_top = parent.y() + parent.height() - 0.75;

    for color in [
        CssColor::new(255, 255, 0),
        CssColor::new(255, 192, 203),
        CssColor::new(173, 216, 230),
    ] {
        let child = rect(color);
        let child_top = child.y() + child.height();
        assert!(
            (child.height() - 72.0).abs() < 0.01,
            "expected 6em child height, got {child:?}",
        );
        assert!(
            (parent_content_top - child_top - 12.0).abs() < 0.01,
            "inline-block abspos child should start after its 1em top margin: parent={parent:?} child={child:?}",
        );
    }
}

async fn render_inline_abspos_static_position_text_box_trim_case(
    target_extra: &str,
    body: &str,
) -> quire::Document {
    Html::from_string(format!(
        "<!DOCTYPE html>\
         <style>\
         @page {{ size: 300px 220px; margin: 0 }}\
         html, body {{ margin: 0; padding: 0 }}\
         .target {{ position: relative; font: 50px/2 sans-serif; text-box-edge: text; {target_extra} }}\
         .abs {{ position: absolute; text-box-trim: none }}\
         </style>\
         <div class=\"target\">{body}</div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap()
}

#[tokio::test]
async fn inline_abspos_static_position_uses_trimmed_text_box_paint_origin() {
    let body = "A<span class=\"abs\">Abs</span>";
    let untrimmed = render_inline_abspos_static_position_text_box_trim_case("", body).await;
    let trimmed =
        render_inline_abspos_static_position_text_box_trim_case("text-box-trim: trim-start;", body)
            .await;

    let abs_line_y = |document: &quire::Document| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == "Abs")
            .unwrap_or_else(|| panic!("missing abspos line: {:?}", document.pages[0].lines()))
            .y()
    };

    let delta = abs_line_y(&trimmed) - abs_line_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "trim-start should move the inline abspos static position by the removed start leading: delta={delta}",
    );
}

#[tokio::test]
async fn inline_abspos_static_position_advances_by_trimmed_text_box_line_height() {
    let body = "A<br><span class=\"abs\">Abs</span>";
    let untrimmed = render_inline_abspos_static_position_text_box_trim_case("", body).await;
    let trimmed =
        render_inline_abspos_static_position_text_box_trim_case("text-box-trim: trim-start;", body)
            .await;

    let abs_line_y = |document: &quire::Document| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == "Abs")
            .unwrap_or_else(|| panic!("missing abspos line: {:?}", document.pages[0].lines()))
            .y()
    };

    let delta = abs_line_y(&trimmed) - abs_line_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "trimmed first-line height should advance the later inline abspos static position by the removed start leading: delta={delta}",
    );
}

#[tokio::test]
async fn block_abspos_static_position_after_inline_uses_trimmed_text_box_line_height() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            "<!DOCTYPE html>\
             <style>\
             @page {{ size: 300px 220px; margin: 0 }}\
             html, body, div {{ margin: 0; padding: 0 }}\
             .target {{ position: relative; font: 50px/2 sans-serif; text-box-edge: text; {target_extra} }}\
             .abs {{ position: absolute; width: 20px; height: 20px; background: rgb(0, 128, 0); text-box-trim: none }}\
             </style>\
             <div class=\"target\"><span>A<div class=\"abs\"></div>X</span></div>"
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-start;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let abs_rect_y = |document: &quire::Document| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .unwrap_or_else(|| panic!("missing abspos rect: {:?}", document.pages[0].rects()))
            .y()
    };

    let delta = abs_rect_y(&trimmed) - abs_rect_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "trimmed preceding line height should move the block-in-inline abspos static position by the removed start leading: delta={delta}",
    );
}

#[tokio::test]
async fn inline_abspos_static_position_after_forced_break_preserves_inline_padding_context() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 400px 180px; margin: 0 } body { margin: 0; font-size: 20px; line-height: 20px }</style>\
         <span style=\"padding-left:100px;\">Line 1<br><span style=\"position:absolute; padding-left:100px;\">Line 2</span></span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("missing line {text:?}: {:?}", document.pages[0].lines()))
    };
    let first = line("Line 1");
    let second = line("Line 2");

    assert!(
        (first.y() - second.y() - 15.0).abs() < 0.01,
        "nested inline padding case should place the abspos line below the first line: first={first:?} second={second:?}",
    );
    assert!(
        (first.x() - second.x()).abs() < 0.01,
        "nested inline padding should preserve the existing horizontal static position behavior: first={first:?} second={second:?}",
    );
}

#[tokio::test]
async fn anonymous_block_after_flow_matches_inline_abspos_reference_with_default_body_margin() {
    let actual = Html::from_string(
        "<!DOCTYPE html>\
         <p>The second line should be just below the first line.</p>\
         <span style=\"padding-left:100px;\">\
           Line 1<br>\
           <span style=\"position:absolute; padding-left:100px;\">Line 2</span>\
         </span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(
        "<!DOCTYPE html>\
         <p>The second line should be just below the first line.</p>\
         <div style=\"padding-left:100px;\">\
           Line 1<br>\
           Line 2\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    fn line<'a>(document: &'a quire::Document, text: &str) -> &'a quire::RenderedLine {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("missing line {text:?}: {:?}", document.pages[0].lines()))
    }
    for text in [
        "The second line should be just below the first line.",
        "Line 1",
        "Line 2",
    ] {
        let actual_line = line(&actual, text);
        let reference_line = line(&reference, text);
        assert!(
            (actual_line.x() - reference_line.x()).abs() < 0.01
                && (actual_line.y() - reference_line.y()).abs() < 0.01,
            "anonymous-block case should match the block reference for {text:?}: actual={actual_line:?}, reference={reference_line:?}",
        );
    }
}

#[tokio::test]
async fn fixed_static_position_inside_static_position_absolute() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<title>Static position fixed inside static position absolute</title>
<link rel="author" title="Martin Robinson" href="mrobinson@igalia.com">
<link rel="help" href="https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width" title="10.3.7 Absolutely positioned, non-replaced elements">
<link rel="match" href="../../reference/ref-filled-green-100px-square.xht">

<p>Test passes if there is a filled green square and <strong>no red</strong>.</p>

<div style="display: absolute; width: 100px; height: 100px; background: green;">
    <div style="position: absolute; width: 50px; height: 50px; margin-left: 50px; margin-top: 50px; background: red;">
        <div style="position: fixed; width: 50px; height: 50px; background: green;"></div>
    </div>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);

    let emitted_rects = emitted_rects_with_fills(page, &[green, red]);
    assert_eq!(
        emitted_rects.len(),
        3,
        "expected outer green, red absolute, and fixed green rectangles: {emitted_rects:?}",
    );
    assert_eq!(
        emitted_rects
            .iter()
            .map(|(_, rect)| rect.fill)
            .collect::<Vec<_>>(),
        vec![Some(green), Some(red), Some(green)],
        "emitted rectangles should follow tree paint order: {emitted_rects:?}",
    );

    let (_, outer_green_rect) = emitted_rects[0];
    let (red_operation, red_rect) = emitted_rects[1];
    let (small_green_operation, small_green_rect) = emitted_rects[2];

    assert!(
        (outer_green_rect.width() - 75.0).abs() < 0.01
            && (outer_green_rect.height() - 75.0).abs() < 0.01,
        "outer green square should be present: {outer_green_rect:?}",
    );

    assert_eq!(red_rect.x(), small_green_rect.x());
    assert_eq!(red_rect.y(), small_green_rect.y());
    assert_eq!(red_rect.height(), small_green_rect.height());
    assert_eq!(red_rect.width(), small_green_rect.width());

    assert!(
        (red_rect.width() - 37.5).abs() < 0.01 && (red_rect.height() - 37.5).abs() < 0.01,
        "red absolute rect should have the expected emitted size: {red_rect:?}",
    );
    assert!(
        same_rect(small_green_rect, red_rect),
        "fixed green should cover red absolute parent: red={red_rect:?} green={small_green_rect:?}",
    );
    assert_final_rect_fill(page, red_rect, green);

    assert!(
        small_green_operation > red_operation,
        "fixed green should paint after red absolute parent: operations={:?} rects={:?}",
        page.paint_operations(),
        page.rects(),
    );

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let pdf_rects = pdf_emitted_rects_with_fills(&pdf, &[green, red]);
    assert_eq!(
        pdf_rects.len(),
        3,
        "expected three colored PDF rectangles: {pdf_rects:?}",
    );
    assert_eq!(
        pdf_rects.iter().map(|rect| rect.fill).collect::<Vec<_>>(),
        vec![green, red, green],
        "PDF rectangles should be emitted in tree paint order: {pdf_rects:?}",
    );

    let pdf_red_rect = pdf_rects[1].rect;
    let pdf_small_green_rect = pdf_rects[2].rect;
    assert!(
        same_pdf_rect(pdf_red_rect, pdf_small_green_rect),
        "fixed green PDF rectangle should cover red PDF rectangle: red={pdf_red_rect:?} green={pdf_small_green_rect:?}",
    );
}

#[tokio::test]
async fn absolute_auto_width_fills_between_left_and_right() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } div { position: absolute; left: 20pt; right: 30pt; height: 10pt; margin: 0; background: #2292d4 }</style><div>Fill</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 30.0).abs() < 0.01);
    assert!((blue.width() - 130.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_auto_width_between_insets_subtracts_non_auto_margins() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } div { position: absolute; left: 20pt; right: 30pt; height: 10pt; margin-left: 5pt; margin-right: 7pt; background: #2292d4 }</style><div>Fill</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 35.0).abs() < 0.01);
    assert!((blue.width() - 118.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_auto_width_between_insets_subtracts_padding_and_borders() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { margin: 0 } div { position: absolute; left: 20pt; right: 30pt; height: 10pt; padding-left: 5pt; padding-right: 7pt; border-left: 2pt solid black; border-right: 3pt solid black; background: #2292d4 }</style><div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 30.0).abs() < 0.01);
    assert!((blue.width() - 130.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_right_offset_anchors_margin_box_edge() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } div { position: absolute; right: 20pt; width: 50pt; height: 10pt; margin-left: 7pt; margin-right: 5pt; background: #2292d4 }</style><div>Right</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 115.0).abs() < 0.01);
    assert!((blue.width() - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_overconstrained_horizontal_axis_uses_containing_direction() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { direction: rtl; margin: 0 } div { position: absolute; left: 10pt; right: 20pt; width: 50pt; height: 10pt; background: #2292d4 }</style><div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 120.0).abs() < 0.01);
    assert!((blue.width() - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_horizontal_auto_margins_center_definite_width_between_insets() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { margin: 0 } .container { position: relative; width: 160pt; height: 40pt } .target { position: absolute; left: 0; right: 0; width: 100pt; height: 10pt; margin-left: auto; margin-right: auto; background: #2292d4 }</style><div class=\"container\"><div class=\"target\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = filled_rect(&document.pages[0], CssColor::new(34, 146, 212));

    assert!((blue.x() - 40.0).abs() < 0.01);
    assert!((blue.width() - 100.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_right_anchored_width_applies_min_width_before_positioning() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { margin: 0 } div { position: absolute; right: 0; width: 20pt; min-width: 50pt; height: 10pt; background: #2292d4 }</style><div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 140.0).abs() < 0.01);
    assert!((blue.width() - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_auto_height_fills_between_top_and_bottom() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 160pt; margin: 10pt } body { margin: 0 } div { position: absolute; top: 20pt; bottom: 30pt; width: 20pt; margin: 0; background: #2292d4 }</style><div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.y() - 40.0).abs() < 0.01);
    assert!((blue.height() - 90.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_vertical_auto_margins_center_definite_height_between_insets() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 140pt; margin: 10pt } body { margin: 0 } .container { position: relative; width: 40pt; height: 100pt } .target { position: absolute; top: 0; bottom: 0; width: 20pt; height: 40pt; margin-top: auto; margin-bottom: auto; background: #2292d4 }</style><div class=\"container\"><div class=\"target\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = filled_rect(&document.pages[0], CssColor::new(34, 146, 212));

    assert!((blue.y() - 60.0).abs() < 0.01);
    assert!((blue.height() - 40.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_auto_height_between_insets_subtracts_non_auto_margins() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 160pt; margin: 10pt } body { margin: 0 } div { position: absolute; top: 20pt; bottom: 30pt; width: 20pt; margin-top: 5pt; margin-bottom: 7pt; background: #2292d4 }</style><div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.y() - 47.0).abs() < 0.01);
    assert!((blue.height() - 78.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_auto_height_between_insets_subtracts_padding_and_borders() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 160pt; margin: 10pt } body { margin: 0 } div { position: absolute; top: 20pt; bottom: 30pt; width: 20pt; padding-top: 5pt; padding-bottom: 7pt; border-top: 2pt solid black; border-bottom: 3pt solid black; background: #2292d4 }</style><div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.y() - 40.0).abs() < 0.01);
    assert!((blue.height() - 90.0).abs() < 0.01);
}

#[tokio::test]
async fn relative_position_offsets_visual_box_without_affecting_flow() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 10pt } .move { position: relative; left: 15pt; top: 5pt }</style><p class=\"move\">Moved</p><p>After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let moved = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Moved")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((moved.x() - 25.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, moved, 105.0);
    assert!((after.x() - 10.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, after, 100.0);
}

#[tokio::test]
async fn absolute_position_applies_to_replaced_images() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }</style><img style=\"position:absolute; left:20pt; top:15pt\" width=\"10\" height=\"20\" src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\">",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    assert!((document.pages[0].images()[0].x() - 30.0).abs() < 0.01);
    assert!((document.pages[0].images()[0].y() - 80.0).abs() < 0.01);
}

#[tokio::test]
async fn absolute_positioned_image_uses_intrinsic_auto_size() {
    let green_60_png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADwAAAA8CAIAAAC1nk4lAAAAAXNSR0IArs4c6QAAAAlwSFlzAAALEwAACxMBAJqcGAAAAAd0SU1FB9oFFQg4GNq2yCEAAAAZdEVYdENvbW1lbnQAQ3JlYXRlZCB3aXRoIEdJTVBXgQ4XAAAAR0lEQVRo3u3OQQ0AAAgEoNPkRjeFDzdIQGXyTifS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0vcWUo0A+IZA5H8AAAAASUVORK5CYII=";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 160pt 120pt; margin: 10pt }} body {{ margin: 0 }}</style>\
         <img style=\"position:absolute; left:40px; top:20px\" src=\"{green_60_png}\">"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .images()
        .first()
        .expect("expected intrinsic image paint");
    assert!((green.x() - 40.0).abs() < 0.01, "green={green:?}");
    assert!((green.y() - 50.0).abs() < 0.01, "green={green:?}");
    assert!((green.width() - 45.0).abs() < 0.01, "green={green:?}");
    assert!((green.height() - 45.0).abs() < 0.01, "green={green:?}");
}

#[tokio::test]
async fn absolute_position_applies_to_tables() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body, table { margin: 0; font-size: 10pt; line-height: 10pt; border-spacing: 0 } table { position: absolute; left: 30pt; top: 20pt } td { padding: 0 }</style><table><tr><td>Cell</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "Cell");
    assert!((document.pages[0].lines()[0].x() - 40.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, &document.pages[0].lines()[0], 90.0);
}

#[tokio::test]
async fn absolute_table_auto_margins_center_like_block_between_insets() {
    let document = Html::from_string(
        "<style>@page { size: 200px 200px; margin: 0 } body { margin: 0 } .container { position: relative; left: -30px; top: -30px; width: 160px; height: 160px } .target { display: block; background: red } .table { display: table; background: green } .centered { position: absolute; width: 100px; height: 100px; inset: 0; margin: auto }</style><div class=\"container\"><div class=\"centered target\"></div><div class=\"centered table\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    let red = filled_rect(page, CssColor::new(255, 0, 0));
    let green = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .max_by(|left, right| {
            (left.width() * left.height()).total_cmp(&(right.width() * right.height()))
        })
        .expect("green table background should be present");

    assert!((green.x() - 0.0).abs() < 0.01);
    assert!((green.y() - 75.0).abs() < 0.01);
    assert!((green.width() - 75.0).abs() < 0.01);
    assert!((green.height() - 75.0).abs() < 0.01);
    assert!((green.x() - red.x()).abs() < 0.01);
    assert!((green.y() - red.y()).abs() < 0.01);
    assert!((green.width() - red.width()).abs() < 0.01);
    assert!((green.height() - red.height()).abs() < 0.01);
    assert!(
        first_rect_paint_operation_index(page, CssColor::new(0, 128, 0))
            > first_rect_paint_operation_index(page, CssColor::new(255, 0, 0)),
        "green table should paint after and cover the red block: {:?}",
        page.paint_operations()
    );
}

#[tokio::test]
async fn absolute_auto_width_table_uses_fragment_intrinsic_width() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 10pt } body, table { margin:0; font-size:10pt; line-height:10pt; border-spacing:0 } table { position:absolute; left:0; top:20pt; background:#eeeeee } td { padding:0 } .wide { width:80pt } .narrow { width:40pt }</style>\
         <table><tr><td class=\"wide\">Wide</td><td class=\"narrow\">Cell</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let wide = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Wide")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let table_background = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(238, 238, 238)))
        .max_by(|left, right| left.width().total_cmp(&right.width()))
        .unwrap();

    assert!(
        cell.x() - wide.x() > 75.0,
        "positioned table cells should use fragment intrinsic column widths"
    );
    assert!(
        table_background.width() > 115.0,
        "positioned auto-width table should shrink-wrap its fragment grid: {table_background:?}"
    );
}

#[tokio::test]
async fn absolute_collapsed_table_bottom_uses_fragment_border_insets() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 180pt; margin: 20pt } body { margin:0 } table { position:absolute; bottom:0; margin:0; width:80pt; border-collapse:collapse; border:20pt solid #eeeeee; background:#00aa00 } td { padding:0; font-size:10pt; line-height:10pt }</style>\
         <table><tr><td>Bottom</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let table_background = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 170, 0)))
        .max_by(|left, right| left.width().total_cmp(&right.width()))
        .unwrap();

    assert!(
        (table_background.y() - 20.0).abs() < 0.01
            && (table_background.height() - 50.0).abs() < 0.01,
        "collapsed table bottom should use fragment-derived outer border insets near the page bottom: {table_background:?}"
    );
}

#[tokio::test]
async fn relative_position_offsets_flex_and_table_boxes() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body, table, p { margin: 0; font-size: 10pt; line-height: 10pt } .flex { display: flex; position: relative; left: 10pt; top: 5pt } table { position: relative; left: 20pt; top: 5pt; border-spacing: 0 } td { padding: 0 }</style><div class=\"flex\"><p>Flex</p></div><table><tr><td>Table</td></tr></table><p>After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let flex = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Flex")
        .unwrap();
    let table = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Table")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((flex.x() - 20.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, flex, 125.0);
    assert!((table.x() - 30.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, table, 115.0);
    assert!((after.x() - 10.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, after, 110.0);
}

#[tokio::test]
async fn bottom_positioned_auto_height_uses_content_height() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt }</style><div style=\"position:absolute; bottom:0; width:100%\">One<br>Two<br>Three</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].lines().len(), 3);
    assert_eq!(document.pages[0].lines()[0].text, "One");
    assert_eq!(document.pages[0].lines()[2].text, "Three");
    assert!(document.pages[0].lines()[0].y() > document.pages[0].lines()[2].y());
}

#[tokio::test]
async fn positions_absolute_children_against_relative_containing_blocks() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body { margin: 0 } .track { position: relative; width: 100pt; height: 10pt; background: #eee } .bar { position: absolute; left: 25%; width: 50%; height: 10pt; background: #2292d4 }</style><div class=\"track\"><div class=\"bar\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 35.0).abs() < 0.01);
    assert!((blue.width() - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn transformed_block_establishes_containing_block_for_absolute_child() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body { margin: 0 } .track { transform: translate(0pt, 0pt); width: 100pt; height: 10pt; background: #eee } .bar { position: absolute; left: 25%; width: 50%; height: 10pt; background: #2292d4 }</style><div class=\"track\"><div class=\"bar\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 35.0).abs() < 0.01);
    assert!((blue.width() - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn positioned_containing_block_uses_relative_parent_padding_box() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body, div { margin: 0 } .track { position: relative; width: 100pt; height: 40pt; padding: 3pt 5pt; border: 2pt solid black; background: #eee } .bar { position: absolute; left: 0; top: 0; width: 10pt; height: 10pt; background: #2292d4 }</style><div class=\"track\"><div class=\"bar\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 12.0).abs() < 0.01);
    assert!((blue.y() - 138.0).abs() < 0.01);
}

#[tokio::test]
async fn positioned_table_cell_establishes_padding_box_containing_block() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body, table { margin: 0; border-spacing: 0 } td { position: relative; width: 100pt; height: 40pt; padding: 3pt 5pt; border: 2pt solid black } .bar { position: absolute; left: 0; top: 0; width: 10pt; height: 10pt; background: #2292d4 }</style><table><tr><td><div class=\"bar\"></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 12.0).abs() < 0.01);
    assert!((blue.y() - 138.0).abs() < 0.01);
}

#[tokio::test]
async fn positioned_table_wrapper_establishes_containing_block_for_cell_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body, table, p { margin: 0; font-size: 10pt; line-height: 10pt; border-spacing: 0 } table { position: relative; margin-left: 20pt } td { width: 100pt; height: 10pt; padding: 0 } .bar { position: absolute; left: 0; top: 0; width: 10pt; height: 10pt; background: #2292d4 }</style><p>Before</p><table><tr><td><div class=\"bar\"></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 30.0).abs() < 0.01);
    assert!((blue.y() - 130.0).abs() < 0.01);
}

#[tokio::test]
async fn positioned_inline_block_fragment_captures_absolute_descendants() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } .ib { display: inline-block; position: relative; width: 40pt; height: 20pt; padding: 2pt; border: 1pt solid black } .bar { display: block; position: absolute; left: 0; top: 0; width: 10pt; height: 10pt; background: #2292d4 }</style><span class=\"ib\"><span class=\"bar\"></span></span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(34, 146, 212)))
        .unwrap();

    assert!((blue.x() - 11.0).abs() < 0.01);
    assert!((blue.y() - 99.0).abs() < 0.01);
}

#[tokio::test]
async fn abspos_float_inside_positioned_inline_uses_inline_containing_block() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 220px 300px; margin: 0 }\
         body, p, div, span { margin: 0; font-size: 16px; line-height: 20px }\
         #abs { position: absolute; top: 0; left: 0; right: 0; height: 100px; background: green }\
         #float { float: left }\
         span { position: relative; padding-left: 100px }\
         </style>\
         <p>Test passes if there is green square.</p>\
         <div><span><div id=\"float\"><div id=\"abs\"></div></div></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| {
            panic!(
                "green abspos rectangle should be painted: {:?}",
                document.pages[0].rects()
            )
        });

    let expected_css_100px = 75.0;
    assert!(
        (green.width() - expected_css_100px).abs() < 0.01,
        "{green:?}"
    );
    assert!(
        (green.height() - expected_css_100px).abs() < 0.01,
        "{green:?}"
    );
    assert!((green.x() - 0.0).abs() < 0.01, "{green:?}");
    assert!((green.y() - 120.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn abspos_float_inside_transformed_inline_uses_inline_containing_block() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 220px 300px; margin: 0 }\
         body, p, div, span { margin: 0; font-size: 16px; line-height: 20px }\
         #abs { position: absolute; top: 0; left: 0; right: 0; height: 100px; background: green }\
         #float { float: left }\
         span { transform: translate(0); padding-left: 100px }\
         </style>\
         <p>Test passes if there is green square.</p>\
         <div><span><div id=\"float\"><div id=\"abs\"></div></div></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rect(&document.pages[0], CssColor::new(0, 128, 0));
    assert!((green.x() - 0.0).abs() < 0.01, "{green:?}");
    assert!((green.y() - 120.0).abs() < 0.01, "{green:?}");
    assert!((green.width() - 75.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn abspos_float_inside_nested_positioned_inline_uses_nearest_source() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 220px 300px; margin: 0 }\
         body, p, div, span { margin: 0; font-size: 16px; line-height: 20px }\
         #abs { position: absolute; top: 0; left: 0; width: 100px; height: 100px; background: green }\
         #float { float: left }\
         .outer { position: relative; padding-left: 50px }\
         .inner { position: relative; padding-left: 100px }\
         </style>\
         <p>Test passes if there is green square.</p>\
         <div><span class=\"outer\"><span class=\"inner\"><div id=\"float\"><div id=\"abs\"></div></div></span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rect(&document.pages[0], CssColor::new(0, 128, 0));
    assert!((green.x() - 37.5).abs() < 0.01, "{green:?}");
    assert!((green.y() - 120.0).abs() < 0.01, "{green:?}");
    assert!((green.width() - 75.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn abspos_float_inside_positioned_inline_after_text_uses_inline_start() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 220px 300px; margin: 0 }\
         body, p, div, span { margin: 0; font-size: 16px; line-height: 20px }\
         #abs { position: absolute; top: 0; left: 0; width: 100px; height: 100px; background: green }\
         #float { float: left }\
         span { position: relative; padding-left: 100px }\
         </style>\
         <p>Test passes if there is green square.</p>\
         <div><span>text<div id=\"float\"><div id=\"abs\"></div></div></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rect(&document.pages[0], CssColor::new(0, 128, 0));
    assert!((green.x() - 0.0).abs() < 0.01, "{green:?}");
    assert!((green.y() - 120.0).abs() < 0.01, "{green:?}");
    assert!((green.width() - 75.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn absolute_table_cell_descendants_do_not_affect_row_height() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body, table, p { margin: 0; font-size: 10pt; line-height: 10pt; border-spacing: 0 } td { position: relative; width: 100pt; height: 10pt; padding: 0 } .bar { position: absolute; left: 0; top: 0; width: 10pt; height: 40pt; background: #2292d4 }</style><table><tr><td><div class=\"bar\"></div></td></tr></table><p>After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert_line_baseline_at_top(&document, after, 140.0);
}

#[tokio::test]
async fn absolute_positioned_children_do_not_affect_flow_height() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 10pt } .track { position: relative; width: 100pt; height: 10pt; background: #eee } .bar { position: absolute; width: 100%; height: 10pt; background: #2292d4 }</style><div class=\"track\"><div class=\"bar\"></div></div><p>After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert_line_baseline_at_top(&document, after, 140.0);
}

#[tokio::test]
async fn collapses_first_child_top_margin_through_parent_block() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 20pt } html, body { margin: 0 } div { margin-top: 20pt } p { margin: 30pt 0 0 0; font-size: 10pt; line-height: 10pt }</style><div><p>Nested</p></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Nested")
        .unwrap();

    assert_line_baseline_at_top(&document, line, 150.0);
}

#[tokio::test]
async fn collapses_first_descendant_top_margin_through_transparent_wrappers() {
    let style = "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 } .previous { height: 1pt; margin-bottom: 6pt } .wrapper { margin: 0 } p { margin: 0; margin-top: 15pt; font-size: 10pt; line-height: 10pt }</style>";
    let direct = Html::from_string(format!("{style}<div class=\"previous\"></div><p>Text</p>"))
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let wrapped = Html::from_string(format!(
        "{style}<div class=\"previous\"></div><div class=\"wrapper\"><p>Text</p></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        wrapped.pages[0].lines()[0].y(),
        direct.pages[0].lines()[0].y()
    );
}

#[tokio::test]
async fn empty_self_collapsing_child_does_not_give_parent_background_height() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 } p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <p>Before</p><div style=\"background:red\"><div style=\"background:hotpink\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_no_red_or_hotpink_rects(&document);
}

#[tokio::test]
async fn empty_self_collapsing_child_top_margin_does_not_paint_parent_background() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 240pt; margin: 10pt } body { margin: 0 } p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <p>Before</p><div style=\"background:red\"><div style=\"margin-top:150px; background:hotpink\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_no_red_or_hotpink_rects(&document);
}

#[tokio::test]
async fn clear_left_after_collapsed_large_top_margin_does_not_paint_parent_background() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>@page { size: 240pt 260pt; margin: 10pt } body { margin: 0 }</style>
<p>There should be nothing below.</p>
<div style="float:left; width:10px; height:100px;"></div>
<div>
  <div>
    <div style="float:right; width:10px; height:200px;"></div>
  </div>
  <div style="background:red;">
    <div style="margin-top:150px; clear:left; background:hotpink;"></div>
  </div>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_no_red_or_hotpink_rects(&document);
}

#[tokio::test]
async fn block_in_inline_start_margin_collapses_like_unwrapped_block() {
    let style = "<style>@page { size: 200pt 200pt; margin: 0 } body { margin: 0 } .prior { width: 75pt; height: 15pt; background: green } .parent { width: 75pt } .inner { width: 75pt; height: 15pt; margin-top: 30pt; background: green }</style>";
    let wrapped = Html::from_string(format!(
        "{style}<div class=\"prior\"></div><div class=\"parent\"><span><div class=\"inner\"></div></span></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let direct = Html::from_string(format!(
        "{style}<div class=\"prior\"></div><div class=\"parent\"><div class=\"inner\"></div></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(wrapped.pages[0].rects(), direct.pages[0].rects());
}

#[tokio::test]
async fn block_in_inline_zero_height_margins_collapse_to_larger_gap() {
    let style = "<style>@page { size: 200pt 200pt; margin: 0 } body { margin: 0 } .container { width: 75pt } .before, .after { width: 75pt; height: 15pt; background: green } .empty { width: 75pt; height: 0; margin-top: 22.5pt; margin-bottom: 30pt; background: red } .gap { width: 75pt; height: 30pt }</style>";
    let wrapped = Html::from_string(format!(
        "{style}<div class=\"container\"><div class=\"before\"></div><span><div class=\"empty\"></div></span><div class=\"after\"></div></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "{style}<div class=\"container\"><div class=\"before\"></div><div class=\"gap\"></div><div class=\"after\"></div></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(wrapped.pages[0].rects(), reference.pages[0].rects());
}

#[tokio::test]
async fn block_in_inline_only_child_self_collapsing_margins_collapse_through_parent() {
    let style = "<style>@page { size: 200px 200px; margin: 0 } body { margin: 0 } p { display: none } .prior { width: 100px; height: 20px; background: green } .parent { width: 100px; font: 20px/1 serif } .empty { width: 100px; height: 0; margin-top: 30px; margin-bottom: 40px; background: red } .next { width: 100px; height: 20px; background: green } .gap { width: 100px; height: 40px }</style>";
    let wrapped = Html::from_string(format!(
        "{style}<p>Two green squares with a 40px gap; no red.</p><div class=\"prior\"></div><div class=\"parent\"><span><div class=\"empty\"></div></span></div><div class=\"next\"></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let direct = Html::from_string(format!(
        "{style}<p>Two green squares with a 40px gap; no red.</p><div class=\"prior\"></div><div class=\"parent\"><div class=\"empty\"></div></div><div class=\"next\"></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "{style}<p>Two green squares with a 40px gap; no red.</p><div class=\"prior\"></div><div class=\"gap\"></div><div class=\"next\"></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_no_red_or_hotpink_rects(&wrapped);
    assert_eq!(wrapped.pages[0].rects(), direct.pages[0].rects());
    assert_eq!(wrapped.pages[0].rects(), reference.pages[0].rects());

    let green = CssColor::new(0, 128, 0);
    let green_rects = wrapped.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .collect::<Vec<_>>();
    assert_eq!(green_rects.len(), 2);
    let gap = green_rects[0].y() - (green_rects[1].y() + green_rects[1].height());
    assert!(
        (gap - 30.0).abs() < 0.01,
        "40px collapsed margin should create a 30pt gap, got {gap}"
    );
}

#[tokio::test]
async fn phantom_inline_line_boxes_do_not_block_margin_collapse() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 200px 200px; margin: 0 }
body { margin: 0 }
.wrapper { float: left; width: 50px; background: red }
.wrapper.phantom { background: green }
.wrapper > div { line-height: 0; background: green }
.wrapper.phantom > div { background: red }
.wrapper > div::after {
  content: "";
  display: flow-root;
  margin-top: 200px;
}
</style>
<div class="wrapper phantom">
  <div><span style="padding-top: 1px"></span></div>
</div>
<div class="wrapper phantom">
  <div><span style="padding-bottom: 1px"></span></div>
</div>
<div class="wrapper">
  <div><span style="padding-left: 1px"></span></div>
</div>
<div class="wrapper">
  <div><span style="padding-right: 1px"></span></div>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = CssColor::new(0, 128, 0);
    let page = &document.pages[0];
    let green_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .collect::<Vec<_>>();
    assert!(!green_rects.is_empty(), "expected green square paint");
    let left = green_rects
        .iter()
        .map(|rect| rect.x())
        .fold(f32::INFINITY, f32::min);
    let right = green_rects
        .iter()
        .map(|rect| rect.x() + rect.width())
        .fold(f32::NEG_INFINITY, f32::max);
    let bottom = green_rects
        .iter()
        .map(|rect| rect.y())
        .fold(f32::INFINITY, f32::min);
    let top = green_rects
        .iter()
        .map(|rect| rect.y() + rect.height())
        .fold(f32::NEG_INFINITY, f32::max);
    assert!((left - 0.0).abs() < 0.01, "{green_rects:?}");
    assert!((right - 150.0).abs() < 0.01, "{green_rects:?}");
    assert!((bottom - 0.0).abs() < 0.01, "{green_rects:?}");
    assert!((top - 150.0).abs() < 0.01, "{green_rects:?}");

    for x in [18.75, 56.25, 93.75, 131.25] {
        assert_eq!(
            final_rect_fill_at(page, x, 75.0),
            Some(green),
            "expected final visible green at x={x}: {:?}",
            page.rects()
        );
    }
}

#[tokio::test]
async fn inline_axis_margin_and_border_prevent_phantom_line_boxes() {
    let style = r#"
@page { size: 100px 200px; margin: 0 }
body { margin: 0 }
.wrapper { float: left; width: 50px; background: red }
.wrapper > div { line-height: 0; background: green }
.wrapper > div::after {
  content: "";
  display: flow-root;
  margin-top: 200px;
}
"#;
    for inline_edge in [
        "margin-left: 1px",
        "margin-right: 1px",
        "border-left: 1px solid transparent",
        "border-right: 1px solid transparent",
    ] {
        let document = Html::from_string(format!(
            r#"<!DOCTYPE html>
<style>{style}</style>
<div class="wrapper"><div><span style="{inline_edge}"></span></div></div>"#,
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        assert_eq!(
            final_rect_fill_at(page, 18.75, 75.0),
            Some(CssColor::new(0, 128, 0)),
            "{inline_edge} should prevent phantom collapse: {:?}",
            page.rects()
        );
    }
}

#[tokio::test]
async fn block_in_inline_text_align_does_not_move_split_block_child() {
    let base_style = "@page { size: 220pt 220pt; margin: 10pt } body { margin: 0 } section { width: 20ch; font-size: 10pt; line-height: 10pt } .left { text-align: left } .right { text-align: right }";
    let target = Html::from_string(format!(
        "<style>{base_style} div {{ width: 10ch; background: orange }}</style><section class=\"right\"><span>123456789<div>123456789</div>123456789</span></section><section dir=\"rtl\" class=\"left\"><span>123456789<div>123456789</div>123456789</span></section>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "<style>{base_style} .w10 {{ width: 10ch; background: orange }}</style><section class=\"right\"><span><div>123456789</div><div class=\"w10\">123456789</div><div>123456789</div></span></section><section dir=\"rtl\" class=\"left\"><span><div>123456789</div><div class=\"w10\">123456789</div><div>123456789</div></span></section>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let target_lines = target.pages[0]
        .lines()
        .iter()
        .map(|line| {
            (
                line.text.clone(),
                line.x().round() as i32,
                line.y().round() as i32,
            )
        })
        .collect::<Vec<_>>();
    let reference_lines = reference.pages[0]
        .lines()
        .iter()
        .map(|line| {
            (
                line.text.clone(),
                line.x().round() as i32,
                line.y().round() as i32,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(target_lines, reference_lines);
    assert_eq!(target.pages[0].rects(), reference.pages[0].rects());
}

#[tokio::test]
async fn wpt_block_in_inline_align_matches_reference_if_available() {
    let wpt_root = std::path::Path::new("/Users/lee/oss/quire-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }
    let target_source = std::fs::read_to_string(
        wpt_root.join("css/CSS2/normal-flow/block-in-inline-align-001.html"),
    )
    .unwrap();
    let reference_source = std::fs::read_to_string(
        wpt_root.join("css/CSS2/normal-flow/block-in-inline-align-001-ref.html"),
    )
    .unwrap();
    let target = Html::from_string(target_source)
        .with_base_path(wpt_root)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let reference = Html::from_string(reference_source)
        .with_base_path(wpt_root)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let target_lines = target.pages[0]
        .lines()
        .iter()
        .map(|line| {
            (
                line.text.clone(),
                line.x().round() as i32,
                line.y().round() as i32,
            )
        })
        .collect::<Vec<_>>();
    let reference_lines = reference.pages[0]
        .lines()
        .iter()
        .map(|line| {
            (
                line.text.clone(),
                line.x().round() as i32,
                line.y().round() as i32,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(target_lines, reference_lines);
    assert_eq!(target.pages[0].rects(), reference.pages[0].rects());
}

#[tokio::test]
async fn discards_collapsed_top_margin_after_avoid_break_to_page_top() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } html, body { margin: 0 } .spacer { height: 60pt; background: #eee } .keep { page-break-inside: avoid } p { margin: 20pt 0 0 0; font-size: 10pt; line-height: 10pt }</style><div class=\"spacer\"></div><div class=\"keep\"><p>Moved</p></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let moved = document.pages[1]
        .lines()
        .iter()
        .find(|line| line.text == "Moved")
        .unwrap();
    let moved_top = rendered_line_baseline_top(&document, moved);

    assert_eq!(document.pages.len(), 2);
    assert!(
        (moved_top - 90.0).abs() < 1.0,
        "expected collapsed top margin to be discarded at page top, got line top {moved_top:.4}"
    );
}

#[tokio::test]
async fn collapses_adjacent_block_sibling_vertical_margins() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 20pt } html, body, p { margin-left: 0; margin-right: 0; font-size: 10pt; line-height: 10pt } body { margin-top: 0; margin-bottom: 0 } .first { margin-top: 0; margin-bottom: 30pt } .second { margin-top: 20pt; margin-bottom: 0 }</style><p class=\"first\">One</p><p class=\"second\">Two</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let first = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "One")
        .unwrap();
    let second = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Two")
        .unwrap();

    assert_line_baseline_at_top(&document, first, 180.0);
    assert_line_baseline_at_top(&document, second, 140.0);
}

#[tokio::test]
async fn min_height_smaller_than_content_allows_last_child_bottom_margin_to_collapse() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 200px 200px; margin: 0 } body { margin: 0 }\
         .parent { min-height: 5px; width: 100px; background: green }\
         .child { height: 30px; margin-bottom: 50px }\
         .footer { width: 100px; height: 50px; background: green }</style>\
         <div class=\"parent\"><div class=\"child\"></div></div><div class=\"footer\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| b.y().total_cmp(&a.y()));

    assert_eq!(
        green_rects.len(),
        2,
        "expected parent and footer backgrounds: {green_rects:?}"
    );
    let parent = green_rects[0];
    let footer = green_rects[1];
    let gap = parent.y() - (footer.y() + footer.height());
    assert!(
        (parent.height() - 22.5).abs() < 0.01,
        "parent background should only cover the 30px child content: parent={parent:?}"
    );
    assert!(
        (gap - 37.5).abs() < 0.01,
        "50px child bottom margin should collapse through as an external gap: parent={parent:?} footer={footer:?}"
    );
}

#[tokio::test]
async fn min_height_that_grows_parent_keeps_last_child_bottom_margin_inside() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 200px 240px; margin: 0 } body { margin: 0 }\
         .parent { min-height: 100px; width: 100px; background: green }\
         .child { height: 30px; margin-bottom: 50px }\
         .footer { width: 100px; height: 50px; background: green }</style>\
         <div class=\"parent\"><div class=\"child\"></div></div><div class=\"footer\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| b.y().total_cmp(&a.y()));

    assert_eq!(
        green_rects.len(),
        2,
        "expected parent and footer backgrounds: {green_rects:?}"
    );
    let parent = green_rects[0];
    let footer = green_rects[1];
    let gap = parent.y() - (footer.y() + footer.height());
    assert!(
        (parent.height() - 75.0).abs() < 0.01,
        "parent background should grow to the 100px min-height: parent={parent:?}"
    );
    assert!(
        gap.abs() < 0.01,
        "footer should follow the min-height-grown parent without an external collapsed gap: parent={parent:?} footer={footer:?}"
    );
}

#[tokio::test]
async fn min_height_blocks_large_last_child_bottom_margin_from_growing_parent() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 800px 800px; margin: 0 } body { margin: 0 }\
         .wrapper { width: 100px; background: red }\
         .parent { min-height: 100px; background: green }\
         .child { height: 30px; margin-bottom: 550px }\
         .footer { height: 50px; background: green }</style>\
         <div class=\"wrapper\"><div class=\"parent\"><div class=\"child\"></div></div><div class=\"footer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| b.y().total_cmp(&a.y()));

    assert_eq!(
        green_rects.len(),
        2,
        "expected parent and footer backgrounds: {green_rects:?}"
    );
    let parent = green_rects[0];
    let footer = green_rects[1];
    let gap = parent.y() - (footer.y() + footer.height());
    assert!(
        (parent.height() - 75.0).abs() < 0.01,
        "parent background should be the 100px min-height, not include the 550px child margin: parent={parent:?}"
    );
    assert!(
        (footer.height() - 37.5).abs() < 0.01,
        "footer should keep its 50px height: footer={footer:?}"
    );
    assert!(
        gap.abs() < 0.01,
        "footer should follow the min-height-grown parent without an external collapsed gap: parent={parent:?} footer={footer:?}"
    );
}

#[tokio::test]
async fn margin_trim_block_start_trims_collapsed_block_inside_inline_margin() {
    let document = Html::from_string(
        "<style>@page { size: 75pt 75pt; margin: 0 } html, body { margin: 0 }</style>\
         <div style=\"position:relative; width:75pt; height:75pt; background:red\">\
           <div style=\"position:absolute; left:0; top:0; width:75pt; height:7.5pt; background:green\"></div>\
           <div style=\"display:flow-root; background:red\">\
             <div style=\"margin-trim:block-start; margin-top:7.5pt\">\
               <span>\
                 <div style=\"margin:112.5pt 0\"></div>\
                 <div style=\"margin:112.5pt 0\"></div>\
               </span>\
             </div>\
             <div style=\"height:67.5pt; background:green\"></div>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    for (x, y) in [(3.75, 3.75), (37.5, 37.5), (71.25, 71.25)] {
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(CssColor::new(0, 128, 0))
        );
    }
}

#[tokio::test]
async fn paints_body_background_on_page() {
    let document = Html::from_string("<body style=\"background: yellow\"><p>Hello</p></body>")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("the propagated body background should paint the document canvas");
    assert!(background.width() > 0.0 && background.height() > 0.0);
}

#[tokio::test]
async fn supports_forced_page_breaks() {
    let document = Html::from_string(
        "<style>p { margin: 0; page-break-before: always }</style><p>First</p><p>Second</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "First");
    assert_eq!(document.pages[1].lines()[0].text, "Second");
}

#[tokio::test]
async fn empty_absolute_geometry_does_not_materialize_pages() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100px; margin: 0 }\
         html, body { margin: 0 }\
         #container { overflow: overlay; position: absolute; width: 1000px; height: 1000px }\
         #area { position: absolute; left: 1000px; top: -20px; width: 0; height: 1000px }\
         </style><div id=\"container\"><div id=\"area\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
}

#[tokio::test]
async fn absolute_principal_decoration_materializes_continuation_pages() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100px; margin: 0 }\
         html, body { margin: 0 }\
         #box { position: absolute; width: 100px; height: 250px; background: red }\
         </style><div id=\"box\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    for page in &document.pages {
        assert!(
            page.rects()
                .iter()
                .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0))),
            "expected absolute background on every continuation page: {page:?}"
        );
    }
}

#[tokio::test]
async fn positioned_descendant_paint_materializes_its_destination_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100px; margin: 0 }\
         html, body { margin: 0 }\
         #parent { position: absolute; width: 100px; height: 250px }\
         #child { position: absolute; top: 210px; width: 20px; height: 20px; background: lime }\
         </style><div id=\"parent\"><div id=\"child\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[2]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 255, 0)))
    );
}

#[tokio::test]
async fn nested_absolute_overflow_extends_pages_without_leaking_scratch_pagination() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>body { margin: 0; }</style>\
         There should be four pages.\
         <div style=\"position:fixed; bottom:4em;\">\
           This should repeat on every page.\
         </div>\
         <div style=\"position:absolute; top:100vh;\">\
           This should be on the second page.\
           <div style=\"position:fixed; bottom:2em;\">\
             This should also repeat on every page.\
           </div>\
           <div style=\"position:absolute; top:100vh; height:300vh;\">\
             This should be on the fourth page.\
             <div style=\"position:fixed; bottom:0;\">\
               Even this should repeat on every page.\
             </div>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 4);
    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(
        page_has_line(&document.pages[0], "There should be four pages."),
        "{page_lines:?}"
    );
    assert!(
        page_has_line(&document.pages[1], "This should be on the second page."),
        "{page_lines:?}"
    );
    assert!(
        page_has_line(&document.pages[3], "This should be on the fourth page."),
        "{page_lines:?}"
    );

    for (index, page) in document.pages.iter().enumerate() {
        for text in [
            "This should repeat on every page.",
            "This should also repeat on every page.",
            "Even this should repeat on every page.",
        ] {
            assert_eq!(
                page.lines().iter().filter(|line| line.text == text).count(),
                1,
                "expected one fixed-position line {text:?} on page {}: {:?}",
                index + 1,
                page.lines()
            );
        }
    }
}

#[tokio::test]
async fn following_flow_after_nested_absolute_overflow_stays_in_source_flow() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 0 } body, div, p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <p>Before</p>\
         <div style=\"position:absolute; top:100vh;\">\
           Absolute second page\
           <div style=\"position:absolute; top:100vh; height:200vh\">Absolute third page</div>\
         </div>\
         <p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // Transparent absolute margin boxes do not create otherwise blank
    // fragmentainers. Only their paint and positioned descendants materialize
    // document pages, so the two positioned text fragments occupy two pages.
    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(page_has_line(&document.pages[0], "Before"));
    assert!(page_has_line(&document.pages[0], "After"));
    assert!(
        document
            .pages
            .iter()
            .any(|page| page_has_line(page, "Absolute second page")),
        "{page_lines:?}"
    );
    assert!(
        document
            .pages
            .iter()
            .any(|page| page_has_line(page, "Absolute third page")),
        "{page_lines:?}"
    );
}

#[tokio::test]
async fn forced_break_before_defeats_previous_break_after_avoid_without_leading_blank() {
    let wpt_root = std::path::Path::new("/Users/lee/oss/quire-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }
    let source =
        std::fs::read_to_string(wpt_root.join("css/css-page/basic-pagination-003-print.html"))
            .unwrap();
    let document = Html::from_string(source)
        .with_base_path(wpt_root)
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "Page one");
    assert_eq!(document.pages[1].lines()[0].text, "Page two");
    assert!(document.pages.iter().all(|page| !page.lines().is_empty()));
}

#[tokio::test]
async fn forced_breaks_retain_flow_only_min_height_pages() {
    let source = |body: &str| {
        format!(
            "<style>\
             @page {{ size: 293px; margin: 5px }}\
             html, body, div {{ margin: 0; padding: 0; font-size: 16px; line-height: 16px }}\
             div {{ min-height: 10px; break-after: page }}\
             </style>{body}"
        )
    };

    let trailing_empty = Html::from_string(source("<div>Page</div><div></div>"))
        .render(&RenderOptions::default())
        .await
        .unwrap();
    assert_eq!(trailing_empty.pages.len(), 2);
    assert_eq!(trailing_empty.pages[0].lines()[0].text, "Page");
    assert!(trailing_empty.pages[1].lines().is_empty());

    let leading_empty = Html::from_string(source("<div></div><div>Page</div>"))
        .render(&RenderOptions::default())
        .await
        .unwrap();
    assert_eq!(leading_empty.pages.len(), 2);
    assert!(leading_empty.pages[0].lines().is_empty());
    assert_eq!(leading_empty.pages[1].lines()[0].text, "Page");
}

#[tokio::test]
async fn wpt_basic_pagination_page_counts_match_expected_pages() {
    let wpt_root = std::path::Path::new("/Users/lee/oss/quire-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }

    for (fixture, expected_pages) in [
        ("basic-pagination-001-print.html", 1),
        ("basic-pagination-002-print.html", 2),
        ("basic-pagination-003-print.html", 2),
        ("basic-pagination-004-print.html", 2),
        ("basic-pagination-005-print.html", 2),
    ] {
        let source = std::fs::read_to_string(wpt_root.join("css/css-page").join(fixture)).unwrap();
        let document = Html::from_string(source)
            .with_base_path(wpt_root)
            .unwrap()
            .render(&RenderOptions::default())
            .await
            .unwrap();

        assert_eq!(
            document.pages.len(),
            expected_pages,
            "{fixture} should render {expected_pages} page(s)"
        );
    }
}

#[tokio::test]
async fn named_page_change_and_break_before_page_create_one_break() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         html, body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         .a { page: a }\
         .b { page: b; break-before: page }\
         </style><div class=\"a\">AAA</div><div class=\"b\">BBB</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "AAA");
    assert_eq!(document.pages[1].lines()[0].text, "BBB");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn named_page_without_matching_page_rule_preserves_text_metrics_across_the_break() {
    let document = Html::from_string(
        "<div style=\"page:foo\">\
           <div style=\"float:left\">First page</div>\
           <div style=\"clear:both\">Also first page</div>\
           <div style=\"page:bar\">Second page</div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // CSS Paged Media changes only the selected page context.  An unmatched
    // named page must not re-cascade the continuing document box or alter its
    // computed font metrics.  This is the structural path exercised by WPT
    // css/css-page/page-name-000-print.html.
    // <https://www.w3.org/TR/css-page-3/#using-named-pages>
    assert_eq!(document.pages.len(), 2);
    let first = &document.pages[0].lines()[0];
    let second = &document.pages[1].lines()[0];
    assert_eq!(second.text, "Second page");
    assert_eq!(first.font_size, second.font_size);
    assert_eq!(first.font_id, second.font_id);
}

#[tokio::test]
async fn explicit_page_auto_uses_ancestor_named_page_group() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <body style=\"page:a\">\
           <div style=\"page:a\">A</div>\
           <div style=\"page:auto\">B</div>\
           <div style=\"page:b\">C</div>\
         </body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[0].lines()[1].text, "B");
    assert_eq!(document.pages[1].lines()[0].text, "C");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[0].lines()[1].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn propagated_page_auto_resolves_in_its_nearest_formatting_tree_scope() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div, section { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <body style=\"page:a\">\
           <div>A</div>\
           <div style=\"page:b\"><section><div style=\"page:auto\">B</div></section></div>\
         </body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // The nested `auto` is propagated through its wrappers, but it resolves
    // to `b`, its nearest non-auto formatting-tree ancestor, before the body
    // compares the class-A boundary. It must not inherit the output cursor's
    // preceding `a` page type.
    // <https://www.w3.org/TR/css-page-3/#using-named-pages>
    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[1].lines()[0].text, "B");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn vertical_root_named_page_start_preserves_initial_text_metrics() {
    let named = Html::from_string(
        "<html style=\"writing-mode: vertical-rl\"><body><div style=\"page:a\">a</div><div style=\"page:b\">b</div></body></html>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(
        "<html style=\"writing-mode: vertical-rl\"><body><div style=\"margin-block-end:999in\">a</div><div>b</div></body></html>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let named_line = &named.pages[0].lines()[0];
    let reference_line = &reference.pages[0].lines()[0];
    assert_eq!(named_line.text, "a");
    assert_eq!(reference_line.text, "a");
    assert_eq!(named_line.font_id, reference_line.font_id);
    assert_eq!(named_line.font_size, reference_line.font_size);
    assert!(
        (named_line.x() - reference_line.x()).abs() < 0.01
            && (named_line.y() - reference_line.y()).abs() < 0.01,
        "named=({}, {}), reference=({}, {})",
        named_line.x(),
        named_line.y(),
        reference_line.x(),
        reference_line.y(),
    );
}

#[tokio::test]
async fn nested_page_names_use_first_and_last_in_flow_descendant_values() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         @page c { margin-left: 70pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div style=\"page:a\">A</div>\
         <div style=\"page:b\"><div style=\"page:c\"><div style=\"page:a\">B</div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[0].lines()[1].text, "B");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[0].lines()[1].x(), 30.0);
}

#[tokio::test]
async fn nested_page_scope_exit_does_not_split_following_same_named_page_group() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         @page c { margin-left: 70pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div style=\"page:b\"><div style=\"page:c\"><div style=\"page:a\">A</div></div></div>\
         <div style=\"page:a\">C</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[0].lines()[1].text, "C");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[0].lines()[1].x(), 30.0);
}

#[tokio::test]
async fn nested_page_name_boundary_splits_when_sibling_page_values_differ() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page large { margin-left: 30pt }\
         @page small { margin-left: 50pt }\
         body, section, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <section style=\"page:large\">\
           <div>Large before</div>\
           <div style=\"page:small\">Small</div>\
           <div>Large after</div>\
         </section>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(document.pages[0].lines()[0].text.starts_with("Large"));
    assert_eq!(document.pages[1].lines()[0].text, "Small");
    assert!(document.pages[2].lines()[0].text.starts_with("Large"));
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
    assert_eq!(document.pages[2].lines()[0].x(), 30.0);
}

#[tokio::test]
async fn absolutely_positioned_page_name_does_not_create_named_page_group() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div>A</div>\
         <div style=\"page:b; position:absolute; right:0; top:0\">B</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "A")
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "B")
    );
}

#[tokio::test]
async fn page_names_inside_absolutely_positioned_subtree_are_ignored() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div style=\"position:absolute; left:0; top:0\">\
           <div style=\"page:a\">A</div>\
           <div style=\"page:b\">B</div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"A"), "{lines:?}");
    assert!(lines.contains(&"B"), "{lines:?}");
}

#[tokio::test]
async fn fixed_position_page_name_does_not_create_named_page_group() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         .fixed { position: fixed; display: flex; left: 10pt; top: 10pt; width: 20pt; height: 20pt; border: 1pt solid blue }\
         </style>\
         <div class=\"fixed\" style=\"page:b\">fixed</div>\
         <div style=\"page:a\">A</div>\
         <div>B</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    // WPT: css/css-page/page-name-fixed-pos-001-print.html.
    // CSS Paged Media §3.3 considers page-name changes at class-A break
    // points in normal flow; fixed-position boxes are out-of-flow.
    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "fixed")
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "A")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "fixed")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "B")
    );
}

#[tokio::test]
async fn fixed_auto_width_shrink_to_fit_uses_nested_column_flex_intrinsics() {
    let document = Html::from_string(
        "<style>\
         @page { size: 500pt 200pt; margin: 20pt }\
         body, p, div { margin:0; font-size:10pt; line-height:10pt }\
         .fixed-pos { position: fixed; background: red }\
         .inner { width: 100%; background: green }\
         .flexbox { display: flex }\
         .column { flex-direction: column }\
         </style>\
         <p>You should see no red.</p>\
         <div class=\"fixed-pos\">\
           <div class=\"flexbox column\">\
             <div class=\"flexbox\"><div class=\"inner\">XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX</div></div>\
             <div class=\"flexbox\"><div class=\"inner\">YYYY</div></div>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-flexbox/position-fixed-001.html.
    // CSS Position uses shrink-to-fit width for fixed-positioned boxes with
    // auto inline size; nested column flex containers must expose their
    // max-content inline contribution so descendant percentage widths resolve
    // against a definite fixed box width.
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let green_rows = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();

    assert_eq!(green_rows.len(), 2);
    assert!(
        red.width() > 220.0,
        "fixed auto width should use nested flex max-content width: {red:?}"
    );
    for green in green_rows {
        assert!(
            (green.width() - red.width()).abs() < 0.01,
            "percentage flex child should cover fixed background: red={red:?} green={green:?}"
        );
    }
}

#[tokio::test]
async fn floated_page_name_does_not_create_named_page_group_before_first_in_flow_sibling() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div style=\"page:a; float:left\">A</div>\
         <div style=\"page:b\">B</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "A")
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "B")
    );
}

#[tokio::test]
async fn floated_page_name_is_ignored_between_in_flow_named_page_siblings() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page c { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div style=\"page:a\">A</div>\
         <div style=\"page:b; float:left\">B</div>\
         <div style=\"page:c\">C</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "A")
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "B")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "C")
    );
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn inline_block_descendant_page_names_do_not_fragment_atomic_layout() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div style=\"display:inline-block\">\
           <div style=\"page:a\">A</div>\
           <div style=\"page:b\">B</div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"A"), "{lines:?}");
    assert!(lines.contains(&"B"), "{lines:?}");
}

#[tokio::test]
async fn inline_block_page_values_create_boundary_after_atomic_box() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         @page c { margin-left: 70pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div style=\"page:c; display:inline-block\">\
           <div style=\"page:a\">A</div>\
           <div style=\"page:b\">B</div>\
         </div>\
         <div style=\"page:c\">C</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "{:?}",
        document
            .pages
            .iter()
            .map(|page| {
                page.lines()
                    .iter()
                    .map(|line| (line.text.as_str(), line.x(), line.y()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    );
    let page_zero_lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    let page_one_lines = document.pages[1]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(page_zero_lines.contains(&"A"), "{page_zero_lines:?}");
    assert!(page_zero_lines.contains(&"B"), "{page_zero_lines:?}");
    assert!(page_one_lines.contains(&"C"), "{page_one_lines:?}");
    assert_eq!(document.pages[1].lines()[0].x(), 70.0);
}

#[tokio::test]
async fn adjacent_inline_block_page_names_do_not_create_inline_boundary_breaks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div style=\"page:a; display:inline-block\">A</div>\
         <div style=\"page:b; display:inline-block\">B</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"A"), "{lines:?}");
    assert!(lines.contains(&"B"), "{lines:?}");
}

#[tokio::test]
async fn non_leading_inline_page_name_splits_inline_formatting_context() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <body style=\"page:a\"><p>Before <span style=\"page:b\">Named</span> After</p></body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // CSS Paged Media applies `page` to inline boxes as well as block boxes.
    // A non-leading inline page-name change ends the current inline fragment,
    // lays the span out on its named page, and then restores the surrounding
    // page group for following inline content:
    // https://www.w3.org/TR/css-page-3/#using-named-pages
    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].lines()[0].text, "Before");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].text, "Named");
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
    assert_eq!(document.pages[2].lines()[0].text, "After");
    assert_eq!(document.pages[2].lines()[0].x(), 30.0);
}

fn red_png_data_uri() -> &'static str {
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAFAAAABQCAIAAAABc2X6AAAAa0lEQVR42u3QMREAAAwCMfybbl3AEu4NhCQ3abUAAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwOXwbunRwEDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDA3d78VrQ4ODmDDUAAAAASUVORK5CYII="
}

#[tokio::test]
async fn absolute_block_inside_inline_uses_split_inline_static_position() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <title>Static position inside inline</title>\
         <style>\
         @page { size: 200px 200px; margin: 0 }\
         body { margin: 0 }\
         #wrapper { overflow: hidden; width: 100px; height: 100px; margin-top: -100px; }\
         #inline { line-height: 100px; color: transparent; border-left: 100px solid transparent; margin-left: -100px; }\
         #abspos { position: absolute; background-color: green; width: 100px; height: 100px; }\
         #red { position: absolute; width: 100px; height: 100px; background: red; }\
         </style>\
         <p>Instruction text.</p>\
         <div id=\"red\"></div>\
         <div id=\"wrapper\"><span id=\"inline\"><div id=\"abspos\"></div>X</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red_index = page
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("red abspos rectangle should be painted");
    let green_index = page
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| {
            panic!(
                "green abspos rectangle should be painted: {:?}",
                page.rects()
            )
        });
    let red = &page.rects()[red_index];
    let green = &page.rects()[green_index];

    assert!(
        (green.x() - red.x()).abs() < 0.01
            && (green.y() - red.y()).abs() < 0.01
            && (green.width() - red.width()).abs() < 0.01
            && (green.height() - red.height()).abs() < 0.01,
        "green abspos should cover red at the wrapper static position: red={red:?} green={green:?}",
    );
    assert!(
        first_rect_paint_operation_index(page, CssColor::new(0, 128, 0))
            > first_rect_paint_operation_index(page, CssColor::new(255, 0, 0)),
        "green abspos should paint after red: operations={:?} rects={:?}",
        page.paint_operations(),
        page.rects(),
    );
}

#[tokio::test]
async fn inline_replaced_page_name_is_ignored_before_block_sibling_boundary() {
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 120pt 160pt; margin: 10pt }}\
         @page a {{ margin-left: 30pt }}\
         @page b {{ margin-left: 50pt }}\
         body, div {{ margin: 0; font-size: 10pt; line-height: 10pt }}\
         img {{ width: 20pt; height: 20pt }}\
         </style>\
         <body style=\"page:a\">\
           <img style=\"page:b\" src=\"{}\">\
           <div style=\"page:b\">B</div>\
         </body>",
        red_png_data_uri()
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[1].lines()[0].text, "B");
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn inline_replaced_page_name_is_ignored_after_block_sibling_boundary() {
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 120pt 160pt; margin: 10pt }}\
         @page a {{ margin-left: 30pt }}\
         @page b {{ margin-left: 50pt }}\
         body, div {{ margin: 0; font-size: 10pt; line-height: 10pt }}\
         img {{ width: 20pt; height: 20pt }}\
         </style>\
         <body style=\"page:a\">\
           <div style=\"page:b\">A</div>\
           <img style=\"page:b\" src=\"{}\">\
         </body>",
        red_png_data_uri()
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[0].lines()[0].x(), 50.0);
    assert_eq!(document.pages[1].images().len(), 1);
}

#[tokio::test]
async fn block_replaced_page_name_participates_in_named_page_group() {
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 120pt 160pt; margin: 10pt }}\
         @page a {{ margin-left: 30pt }}\
         @page b {{ margin-left: 50pt }}\
         body, div {{ margin: 0; font-size: 10pt; line-height: 10pt }}\
         img {{ width: 20pt; height: 20pt }}\
         </style>\
         <body style=\"page:a\">\
           <img style=\"display:block; page:b\" src=\"{}\">\
           <div style=\"page:b\">B</div>\
         </body>",
        red_png_data_uri()
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "B");
    assert_eq!(document.pages[0].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn block_replaced_page_name_matches_previous_named_block_group() {
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 120pt 160pt; margin: 10pt }}\
         @page a {{ margin-left: 30pt }}\
         @page b {{ margin-left: 50pt }}\
         body, div {{ margin: 0; font-size: 10pt; line-height: 10pt }}\
         img {{ width: 20pt; height: 20pt }}\
         </style>\
         <body style=\"page:a\">\
           <div style=\"page:b\">A</div>\
           <img style=\"display:block; page:b\" src=\"{}\">\
         </body>",
        red_png_data_uri()
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[0].lines()[0].x(), 50.0);
    assert_eq!(document.pages[0].images().len(), 1);
}

#[tokio::test]
async fn inline_canvas_page_name_is_ignored_before_block_sibling_boundary() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 160pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         canvas { border: 1pt solid black }\
         </style>\
         <body style=\"page:a\">\
           <canvas height=\"1\" style=\"page:b\"></canvas>\
           <div style=\"page:b\">B</div>\
         </body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-page/page-name-canvas-001-print.html.
    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[1].lines()[0].text, "B");
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn inline_canvas_page_name_is_ignored_after_block_sibling_boundary() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 160pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         canvas { border: 1pt solid black }\
         </style>\
         <body style=\"page:a\">\
           <div style=\"page:b\">A</div>\
           <canvas height=\"1\" style=\"page:b\"></canvas>\
         </body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-page/page-name-canvas-002-print.html.
    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[0].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn block_canvas_page_name_participates_in_named_page_group() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 160pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         canvas { border: 1pt solid black }\
         </style>\
         <body style=\"page:a\">\
           <canvas height=\"1\" style=\"display:block; page:b\"></canvas>\
           <div style=\"page:b\">B</div>\
         </body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-page/page-name-canvas-003-print.html.
    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "B");
    assert_eq!(document.pages[0].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn block_canvas_page_name_splits_before_following_unnamed_block() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 160pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         canvas { border: 1pt solid black }\
         </style>\
         <body style=\"page:a\">\
           <canvas height=\"1\" style=\"display:block; page:b\"></canvas>\
           <div>B</div>\
         </body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-page/page-name-canvas-004-print.html.
    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[1].lines()[0].text, "B");
    assert_eq!(document.pages[1].lines()[0].x(), 30.0);
}

#[tokio::test]
async fn trailing_unnamed_page_after_named_page_uses_default_page_context() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 160pt; margin: 10pt }\
         @page landscape { margin-left: 40pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         .page { break-after: page }\
         </style>\
         <div class=\"page\"><div>First</div></div>\
         <div class=\"page\"><div style=\"page: landscape\">Named</div></div>\
         <div class=\"page\"><div>Trailing</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // WPT basis: css/css-page/page-name-unnamed-trailing-001-print.html.
    // The third page has no named page owner and must return to the default
    // page context rather than inheriting the previous named page.
    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].lines()[0].text, "First");
    assert_eq!(document.pages[0].lines()[0].x(), 10.0);
    assert_eq!(document.pages[1].lines()[0].text, "Named");
    assert_eq!(document.pages[1].lines()[0].x(), 40.0);
    assert_eq!(document.pages[2].lines()[0].text, "Trailing");
    assert_eq!(document.pages[2].lines()[0].x(), 10.0);
}

#[tokio::test]
async fn flex_item_page_names_do_not_create_container_page_breaks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 160pt; margin: 10pt }\
         @page b { margin-left: 40pt }\
         @page c { margin-left: 60pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div>A</div>\
         <div style=\"display:flex; flex-direction:column\">\
           <div style=\"page:b\">B</div>\
           <div style=\"page:c\">C</div>\
         </div>\
         <div>D</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        1,
        "{:?}",
        document
            .pages
            .iter()
            .map(|page| {
                page.lines()
                    .iter()
                    .map(|line| (line.text.as_str(), line.x(), line.y()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    );
    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(lines, vec!["A", "B", "C", "D"]);
}

#[tokio::test]
async fn flex_container_own_page_name_creates_boundary_around_container() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 160pt; margin: 10pt }\
         @page b { margin-left: 40pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <body style=\"page:a\">\
           <div>A</div>\
           <div style=\"page:b; display:flex; flex-direction:column\">\
             <div>B</div>\
             <div>C</div>\
           </div>\
           <div>D</div>\
         </body>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[1].lines()[0].text, "B");
    assert_eq!(document.pages[1].lines()[1].text, "C");
    assert_eq!(document.pages[2].lines()[0].text, "D");
    assert_eq!(document.pages[1].lines()[0].x(), 40.0);
}

#[tokio::test]
async fn nested_block_page_names_inside_unsplit_flex_item_do_not_fragment_pages() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 160pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div>A</div>\
         <div style=\"display:flex; flex-direction:column\">\
           <div><div><div style=\"page:a\">B</div><div style=\"page:b\">C</div></div></div>\
         </div>\
         <div>D</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // Flexbox owns pagination for an unsplit item. Its independently laid-out
    // contents cannot materialize a page transition before the container has
    // selected an item-fragment boundary.
    // <https://www.w3.org/TR/css-flexbox-1/#pagination>
    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[0].lines()[1].text, "B");
    assert_eq!(document.pages[0].lines()[2].text, "C");
    assert_eq!(document.pages[0].lines()[3].text, "D");
    assert_eq!(document.pages[0].lines()[1].x(), 10.0);
}

#[tokio::test]
async fn table_own_page_name_selects_named_page_context() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 300pt; margin: 20pt }\
         @page square { size: 360pt; margin: 30pt }\
         body, table, caption, td { margin: 0; font-size: 10pt; line-height: 10pt }\
         table { border-spacing: 0 }\
         td { padding: 0 }\
         </style>\
         <table style=\"page:square\">\
           <caption>This should all be on one page.</caption>\
           <thead><tr><td>The width and height of the page should be 5in.</td></tr></thead>\
           <tbody><tr><td>I.e. it should be a square.<br>There should also be no red.</td></tr></tbody>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].width(), 360.0);
    assert_eq!(document.pages[0].height(), 360.0);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .all(|line| (line.x() - 30.0).abs() < 0.01),
        "{:?}",
        document.pages[0]
            .lines()
            .iter()
            .map(|line| (line.text.as_str(), line.x(), line.y()))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn table_row_page_names_select_page_context_per_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page firstrow { margin-left: 30pt }\
         @page secondrow { margin-left: 50pt }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         tr { height: 80pt }\
         </style>\
         <table>\
           <tr style=\"page:firstrow\"><td><div>First</div></td></tr>\
           <tr style=\"page:secondrow\"><td><div>Second</div></td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| (line.text.as_str(), line.x(), line.y()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert_eq!(document.pages[0].lines()[0].text, "First");
    assert_eq!(document.pages[1].lines()[0].text, "Second");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn table_cell_descendant_page_names_select_page_context_per_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page firstrow { margin-left: 30pt }\
         @page secondrow { margin-left: 50pt }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         tr { height: 80pt }\
         </style>\
         <table>\
           <tr><td><div style=\"page:firstrow\">First</div></td></tr>\
           <tr><td><div style=\"page:secondrow\">Second</div></td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| (line.text.as_str(), line.x(), line.y()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert_eq!(document.pages[0].lines()[0].text, "First");
    assert_eq!(document.pages[1].lines()[0].text, "Second");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn rowspanning_table_cell_page_name_persists_across_spanned_rows() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page spanpage { margin-left: 50pt }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 60pt }\
         td { vertical-align: top }\
         </style>\
         <table>\
           <tr><td rowspan=\"2\" style=\"page:spanpage\"><div>SpanTop</div><div style=\"height:70pt\"></div><div>SpanBottom</div></td><td>First</td></tr>\
           <tr><td>Second</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| (line.text.as_str(), line.x(), line.y()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(document.pages.len() >= 2, "{page_lines:?}");
    let bottom = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .find(|line| line.text == "SpanBottom")
        .unwrap_or_else(|| panic!("missing spanning cell continuation: {page_lines:?}"));
    assert_eq!(bottom.x(), 50.0, "{page_lines:?}");
}

#[tokio::test]
async fn explicit_page_auto_row_exits_named_rowspan_context() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin-left: 10pt; margin-top: 10pt; margin-right: 10pt; margin-bottom: 10pt }\
         @page spanpage { margin-left: 50pt }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 60pt }\
         td { vertical-align: top }\
         </style>\
         <table>\
           <tr><td rowspan=\"2\" style=\"page:spanpage\"><div>SpanTop</div><div style=\"height:70pt\"></div><div>SpanBottom</div></td><td>First</td></tr>\
           <tr style=\"page:auto\"><td>Second</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| (line.text.as_str(), line.x(), line.y()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let second = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .find(|line| line.text == "Second")
        .unwrap_or_else(|| panic!("missing explicit auto row text: {page_lines:?}"));
    let first = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .find(|line| line.text == "First")
        .unwrap_or_else(|| panic!("missing named context row text: {page_lines:?}"));
    assert!(
        (first.x() - second.x() - 40.0).abs() < 0.01,
        "explicit page:auto row should use the default page context: {page_lines:?}"
    );
}

#[tokio::test]
async fn repeated_table_header_copy_uses_destination_page_context() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page headerpage { margin-left: 50pt }\
         @page bodypage { margin-left: 30pt }\
         body, table, thead, tbody, tr, td { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 60pt }\
         thead tr { page: headerpage; height: 10pt }\
         tbody tr { page: bodypage; height: 70pt }\
         </style>\
         <table>\
           <thead><tr><td>Head</td></tr></thead>\
           <tbody><tr><td>BodyOne</td></tr><tr><td>BodyTwo</td></tr></tbody>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| (line.text.as_str(), line.x(), line.y()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(document.pages.len() >= 3, "{page_lines:?}");
    let first_header = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Head")
        .unwrap_or_else(|| panic!("missing source header: {page_lines:?}"));
    let body_two_page = document
        .pages
        .iter()
        .find(|page| page.lines().iter().any(|line| line.text == "BodyTwo"))
        .unwrap_or_else(|| panic!("missing second body page: {page_lines:?}"));
    let repeated_header = body_two_page
        .lines()
        .iter()
        .find(|line| line.text == "Head")
        .unwrap_or_else(|| panic!("missing repeated header: {page_lines:?}"));
    let body_two = body_two_page
        .lines()
        .iter()
        .find(|line| line.text == "BodyTwo")
        .unwrap_or_else(|| panic!("missing second body row: {page_lines:?}"));

    assert_eq!(first_header.x(), 50.0, "{page_lines:?}");
    assert_eq!(repeated_header.x(), 30.0, "{page_lines:?}");
    assert_eq!(body_two.x(), 30.0, "{page_lines:?}");
}

#[tokio::test]
async fn repeated_table_footer_copy_uses_destination_page_context() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page bodypage { margin-left: 30pt }\
         @page footerpage { margin-left: 50pt }\
         body, table, thead, tbody, tfoot, tr, td { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 60pt }\
         tbody tr { page: bodypage; height: 70pt }\
         tfoot tr { page: footerpage; height: 10pt }\
         </style>\
         <table>\
           <tbody><tr><td>BodyOne</td></tr><tr><td>BodyTwo</td></tr></tbody>\
           <tfoot><tr><td>Foot</td></tr></tfoot>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| (line.text.as_str(), line.x(), line.y()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let first_body_page = document
        .pages
        .iter()
        .find(|page| page.lines().iter().any(|line| line.text == "BodyOne"))
        .unwrap_or_else(|| panic!("missing first body page: {page_lines:?}"));
    let repeated_footer = first_body_page
        .lines()
        .iter()
        .find(|line| line.text == "Foot")
        .unwrap_or_else(|| panic!("missing repeated footer with first body row: {page_lines:?}"));
    let body_one = first_body_page
        .lines()
        .iter()
        .find(|line| line.text == "BodyOne")
        .unwrap();
    let source_footer = document
        .pages
        .iter()
        .rev()
        .flat_map(|page| page.lines())
        .find(|line| line.text == "Foot")
        .unwrap_or_else(|| panic!("missing source footer: {page_lines:?}"));

    assert_eq!(body_one.x(), 30.0, "{page_lines:?}");
    assert_eq!(repeated_footer.x(), 30.0, "{page_lines:?}");
    assert_eq!(source_footer.x(), 50.0, "{page_lines:?}");
}

#[tokio::test]
async fn table_row_explicit_page_auto_uses_table_named_page_group() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         @page firstrow { margin-left: 30pt }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt; page: firstrow }\
         tr { height: 80pt }\
         </style>\
         <table>\
           <tr><td><div>First</div></td></tr>\
           <tr style=\"page:auto\"><td><div>Second</div></td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| (line.text.as_str(), line.x(), line.y()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert_eq!(document.pages[0].lines()[0].text, "First");
    assert_eq!(document.pages[1].lines()[0].text, "Second");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].x(), 30.0);
}

#[tokio::test]
async fn empty_page_named_block_with_display_none_child_coalesces_with_next_group() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         @page a { margin-left: 30pt }\
         @page b { margin-left: 50pt }\
         @page c { size: 140pt 100pt; margin-left: 70pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div style=\"page:a\">A</div>\
         <div style=\"page:c\"><div style=\"display:none\">C</div></div>\
         <div style=\"page:b\">B</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[1].lines()[0].text, "B");
    assert_eq!(document.pages[0].lines()[0].x(), 30.0);
    assert_eq!(document.pages[1].lines()[0].x(), 50.0);
}

#[tokio::test]
async fn zero_height_named_blocks_do_not_each_force_separate_pages() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 120pt; margin: 10pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 15pt }\
         </style>\
         <div style=\"page:a\">A</div>\
         <div style=\"page:b; height:0\">B</div>\
         <div style=\"page:c; height:0; padding-left:20pt\">C</div>\
         <div style=\"page:d; height:0; padding-left:40pt\">D</div>\
         <div style=\"page:e; padding-left:60pt\">E</div>\
         <div style=\"page:f\">F</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].lines()[0].text, "A");
    let middle_page_lines = document.pages[1]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(middle_page_lines, vec!["B", "C", "D", "E"]);
    assert_eq!(document.pages[2].lines()[0].text, "F");
}

#[tokio::test]
async fn rtl_root_direction_makes_first_page_match_left_page_selector() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 0 }\
         @page :left { margin-left: 40pt; margin-top: 20pt }\
         @page :right { margin-right: 40pt; margin-bottom: 20pt }\
         :root { direction: rtl }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         p { direction: ltr }\
         p { break-after: page }\
         </style><p>One</p><p>Two</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "One");
    assert_eq!(document.pages[1].lines()[0].text, "Two");
    assert_eq!(document.pages[0].lines()[0].x(), 40.0);
    assert_eq!(document.pages[1].lines()[0].x(), 0.0);
    assert!(
        (document.pages[1].lines()[0].y() - document.pages[0].lines()[0].y() - 20.0).abs() < 0.01
    );
}

#[tokio::test]
async fn rtl_forced_left_break_targets_next_left_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 0 }\
         @page :left { margin-left: 40pt }\
         :root { direction: rtl }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         p { direction: ltr }\
         .next-left { break-before: left }\
         </style><p>One</p><p class=\"next-left\">Two</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].lines()[0].text, "One");
    assert!(document.pages[1].lines().is_empty());
    assert_eq!(document.pages[2].lines()[0].text, "Two");
    assert_eq!(document.pages[2].lines()[0].x(), 40.0);
}

#[tokio::test]
async fn paints_parent_block_after_child_forced_page_break() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body { margin: 0 } .before { height: 1pt; background: red } .outer { margin: 0; background: #00ff00; border: 1pt solid black } .spacer { height: 70pt } .breaker { break-before: always; height: 10pt }</style><div class=\"before\"></div><div class=\"outer\"><div class=\"spacer\"></div><div class=\"breaker\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(document.pages.iter().any(|page| !page.rects().is_empty()));
    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    assert!(pdf_searchable_text(&pdf).starts_with("%PDF-1.4"));
}

#[tokio::test]
async fn keeps_break_inside_avoid_blocks_together_when_they_fit_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, p, div { margin: 0; font-size: 10pt; line-height: 10pt } .keep { page-break-inside: avoid; }</style><p>f1<br>f2<br>f3<br>f4<br>f5</p><div class=\"keep\">k1<br>k2<br>k3<br>k4</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .all(|line| !line.text.starts_with('k'))
    );
    assert_eq!(
        document.pages[1]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["k1", "k2", "k3", "k4"]
    );
}

#[tokio::test]
async fn break_after_avoid_moves_sibling_run_when_it_fits_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 110pt 110pt; margin: 10pt } body, div { margin: 0 }\
         .large { height: 60pt; width: 20pt; background: #ff0000 }\
         .small { height: 20pt; width: 20pt; background: #0000ff; break-after: avoid }</style>\
         <div class=\"large\"></div><div class=\"small\"></div><div class=\"large\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .count(),
        1
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(0, 0, 255)))
    );
    assert_eq!(
        document.pages[1]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .count(),
        1
    );
    assert_eq!(
        document.pages[1]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        1
    );
}

#[tokio::test]
async fn break_before_avoid_moves_previous_sibling_when_run_fits_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 110pt 110pt; margin: 10pt } body, div { margin: 0 }\
         .large { height: 60pt; width: 20pt; background: #ff0000 }\
         .small { height: 20pt; width: 20pt; background: #0000ff }\
         .keep { break-before: avoid }</style>\
         <div class=\"large\"></div><div class=\"small\"></div><div class=\"large keep\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(0, 0, 255)))
    );
    assert_eq!(
        document.pages[1]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        1
    );
}

#[tokio::test]
async fn break_inside_avoid_paragraph_moves_all_lines_when_it_fits_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 0 } body, h1, p { margin: 0 }\
         h1 { height: 120pt } p { font-size: 20pt; line-height: 20pt; width: 1pt;\
         orphans: 2; widows: 2; page-break-inside: avoid }</style>\
         <h1>Title</h1><p>one two three four five six seven</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(document.pages[0].lines().iter().all(|line| !matches!(
        line.text.as_str(),
        "one" | "two" | "three" | "four" | "five" | "six" | "seven"
    )));
    assert_eq!(
        document.pages[1]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two", "three", "four", "five", "six", "seven"]
    );
}

#[tokio::test]
async fn break_inside_avoid_estimate_uses_css_text_line_fitting() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt }\
         body, p, div { margin: 0; font-family: monospace; font-size: 10pt; line-height: 10pt }\
         .spacer { height: 55pt }\
         .keep { page-break-inside: avoid; width: 44pt; text-indent: 12pt }</style>\
         <div class=\"spacer\"></div><div class=\"keep\">one two three four</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_text = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_text:?}");
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .all(|line| !["one", "two", "three", "four"].contains(&line.text.as_str())),
        "{page_text:?}"
    );
    assert_eq!(
        document.pages[1]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two", "three", "four"]
    );
}

#[tokio::test]
async fn nested_break_inside_avoid_block_moves_to_next_page_when_it_fits() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 216pt; margin: 36pt } p { height: 72pt; width: 72pt; margin: 0; background-color: blue; font-size: 10pt; line-height: 10pt } .test { page-break-inside: avoid }</style>\
         <div class=\"test\"><p>1</p></div>\
         <div class=\"test\"><div class=\"test\"><div class=\"test\"><p>2</p><p>3</p></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| (line.text.clone(), line.x(), line.y()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert_eq!(
        document.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["1"]
    );
    assert_eq!(
        document.pages[1]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["2", "3"]
    );
}

#[tokio::test]
async fn floated_break_inside_avoid_block_moves_to_next_page_when_it_fits() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <html lang=\"en-US\"><head>\
         <title>CSS Test: CSS 2.1 page-break-inside:avoid</title>\
         <style>\
         @page { size: 5in 3in; margin: 0.5in }\
         p { height: 1in; width: 1in; margin: 0; background-color: blue }\
         .test { float: left; page-break-inside: avoid }\
         </style></head><body>\
         <div style=\"clear: both\"><p>1</p></div>\
         <div class=\"test\"><p>2</p><p>3</p></div>\
         </body></html>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_text = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_text:?}");
    assert_eq!(page_text[0], vec!["1"]);
    assert_eq!(page_text[1], vec!["2", "3"]);

    let page_two_lines = document.pages[1].lines();
    assert!(
        page_two_lines[0].y() > page_two_lines[1].y(),
        "floated avoid lines should be vertically ordered: {page_two_lines:?}",
    );
    assert!(
        (page_two_lines[0].y() - page_two_lines[1].y()).abs() > 0.01,
        "floated avoid lines should occupy distinct vertical positions: {page_two_lines:?}",
    );
}

#[tokio::test]
async fn cleared_avoid_float_moves_to_fresh_page_when_clearance_overflows() {
    // CSS 2.2 clear placement happens before fragmentation. Once the second
    // float has been moved below the first left float, its own unbreakable
    // margin box no longer fits in the current page fragment and must start a
    // new page rather than being clipped into the first page.
    // https://www.w3.org/TR/CSS22/visuren.html#flow-control
    // https://www.w3.org/TR/css-break-3/#break-within
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 5in 3in; margin: .5in }\
         html, body { margin: 0; padding: 0; height: 100% }\
         .test { height: 60%; float: left; clear: left; background: blue; page-break-inside: avoid }\
         </style><br style=\"clear:both\"><div class=\"test\">1</div><div class=\"test\">2</div>X",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_text = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(document.pages.len(), 2, "{page_text:?}");
    assert_eq!(page_text[0], vec!["1", "X"]);
    assert_eq!(page_text[1], vec!["2"]);
}

#[tokio::test]
async fn cleared_nested_avoid_float_defers_without_advancing_parent_flow() {
    // The nested clear-containing block still owns ordinary in-flow text.
    // Moving only its floated child must therefore defer that float's paint
    // to the next fragmentainer rather than moving `X`/`Y` with it.
    // <https://www.w3.org/TR/CSS22/visuren.html#floats>
    // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 5in 3in; margin: .5in }\
         html, body { margin: 0; padding: 0; height: 100% }\
         .test { height: 60%; float: left; clear: left; background: blue; page-break-inside: avoid }\
         </style><br style=\"clear:both\"><div class=\"test\">1</div>\
         <div style=\"height:60%;clear:both\"><div class=\"test\" style=\"height:100%\">2</div>X<br>Y</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_text = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(document.pages.len(), 2, "{page_text:?}");
    assert_eq!(page_text[0], vec!["1", "X", "Y"]);
    assert_eq!(page_text[1], vec!["2"]);
}

#[tokio::test]
async fn deferred_float_replays_body_and_clipped_containing_block_geometry() {
    // The deferred float must resolve its percentage width against the same
    // overflow-clipped containing block on its destination page. Root/body
    // margins are deliberately nonzero so retaining only the old page cursor
    // would shift the second float horizontally.
    // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 20pt }\
         html { margin: 0 } body { margin: 10pt; font-size: 10pt; line-height: 10pt }\
         .clip { overflow: hidden; width: 80pt }\
         .test { box-sizing: border-box; width: 50%; height: 50pt; float: left; clear: left; background: blue; page-break-inside: avoid }</style>\
         <div class=\"clip\"><div class=\"test\">1</div><div class=\"test\">2</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    let float_rect = document.pages[1]
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.width() - 40.0).abs() < 0.01
                && rect.height() > 0.0
        })
        .expect("destination page should retain the float's clipped percentage width");
    assert!(
        (float_rect.x() - 30.0).abs() < 0.01,
        "destination float must retain page + body x-offset: {float_rect:?}"
    );
}

#[tokio::test]
async fn forced_break_before_preserves_break_inside_avoid_for_descendants() {
    let document = Html::from_string(
        "<style>@page { size: 5in 3in; margin: 0.5in }\
         p { height: 1in; width: 1in; margin: 0; background-color: blue }\
         .test { page-break-before: always; page-break-inside: avoid }</style>\
         <p>1</p><div class=\"test\"><p>2</p><p>3</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_text = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_text:?}");
    assert_eq!(page_text[0], vec!["1"]);
    assert_eq!(page_text[1], vec!["2", "3"]);
}

#[tokio::test]
async fn forced_break_after_slices_ancestor_block_start_spacing() {
    let document = Html::from_string(
        "<style>@page { size: 5in 3in; margin: 0.5in }\
         p { height: 1in; width: 1in; margin: 0; background-color: blue }\
         .test { page-break-after: always }</style>\
         <div class=\"test\"><p>1</p></div><p>2</p><p>3</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_text = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_text:?}");
    assert_eq!(page_text[0], vec!["1"]);
    assert_eq!(page_text[1], vec!["2", "3"]);
}

#[tokio::test]
async fn widows_keeps_minimum_lines_after_text_fragment_break() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 60pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 10pt } p { orphans: 1; widows: 2 }</style><p>l1<br>l2<br>l3<br>l4<br>l5</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["l1", "l2", "l3"]
    );
    assert_eq!(
        document.pages[1]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["l4", "l5"]
    );
}

#[tokio::test]
async fn orphans_moves_text_fragment_when_too_few_lines_fit() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 60pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 10pt } .target { orphans: 2; widows: 1 }</style><p>f1<br>f2<br>f3</p><p class=\"target\">a<br>b<br>c</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["f1", "f2", "f3"]
    );
    assert_eq!(
        document.pages[1]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
}

#[tokio::test]
async fn widows_apply_to_styled_inline_text_fragments() {
    let document = Html::from_string(
        "<style>@page { size: 80pt 60pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 10pt } p { width: 10pt; orphans: 1; widows: 2 }</style><p><span style=\"font-weight:bold\">aa bb cc dd ee</span></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines().len(), 3);
    assert_eq!(document.pages[1].lines().len(), 2);
}

#[tokio::test]
async fn widows_apply_to_mixed_inline_atom_fragments() {
    let document = Html::from_string(
        "<style>@page { size: 80pt 60pt; margin: 10pt } body, p, span { margin: 0; font-size: 10pt; line-height: 10pt } p { width: 10pt; orphans: 1; widows: 2 } .atom { display: inline-block; width: 10pt; height: 10pt }</style><p>aa <span class=\"atom\">x</span> bb cc dd</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines().len(), 3);
    assert_eq!(document.pages[1].lines().len(), 2);
}

#[tokio::test]
async fn keeps_break_inside_avoid_table_groups_together_when_they_fit_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body, p, div, table { margin: 0; font-size: 10pt; line-height: 10pt } td { padding: 0 } .keep { page-break-inside: avoid; }</style><p>f1<br>f2<br>f3<br>f4<br>f5</p><div class=\"keep\"><table><tr><td>A</td></tr><tr><td>B</td></tr><tr><td>C</td></tr></table></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .all(|line| !matches!(line.text.as_str(), "A" | "B" | "C"))
    );
    assert_eq!(
        document.pages[1]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "C"]
    );
}

#[tokio::test]
async fn positioned_positive_z_index_paints_after_normal_flow() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .abs { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #0000ff } .flow { width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"abs\"></div><div class=\"flow\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_red_blue_rect_fills(&document.pages[0]),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 0, 255)]
    );
}

#[tokio::test]
async fn positioned_negative_z_index_paints_before_normal_flow() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .abs { position: absolute; z-index: -1; left: 0; top: 0; width: 30pt; height: 30pt; background: #0000ff } .flow { width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"abs\"></div><div class=\"flow\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_red_blue_rect_fills(&document.pages[0]),
        vec![CssColor::new(0, 0, 255), CssColor::new(255, 0, 0)]
    );
}

#[tokio::test]
async fn absolute_z_index_auto_does_not_trap_positive_positioned_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { position: absolute; left: 0; top: 0; width: 30pt; height: 30pt } .child { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 0; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
        ),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
    );
}

#[tokio::test]
async fn positioned_descendant_z_index_stays_inside_parent_stacking_context() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #0000ff } .child { position: absolute; z-index: 999; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 2; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[
                CssColor::new(0, 0, 255),
                CssColor::new(0, 255, 0),
                CssColor::new(255, 0, 0),
            ],
        ),
        vec![
            CssColor::new(0, 0, 255),
            CssColor::new(0, 255, 0),
            CssColor::new(255, 0, 0),
        ]
    );
}

#[tokio::test]
async fn positioned_negative_descendant_stays_inside_parent_stacking_context() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #0000ff } .child { position: absolute; z-index: -1; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 0; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[
                CssColor::new(255, 0, 0),
                CssColor::new(0, 0, 255),
                CssColor::new(0, 255, 0),
            ],
        ),
        vec![
            CssColor::new(255, 0, 0),
            CssColor::new(0, 0, 255),
            CssColor::new(0, 255, 0),
        ]
    );
}

#[tokio::test]
async fn transparent_positioned_parent_keeps_positioned_child_stacking_context() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt } .child { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 0; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
        ),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
    );
}

#[tokio::test]
async fn fixed_position_repeats_on_each_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 90pt; margin: 10pt } body, div, p { margin: 0 } .fixed { position: fixed; left: 0; top: 0; width: 20pt; height: 20pt; background: #0000ff } .break { break-before: page }</style><div class=\"fixed\"></div><p>One</p><p class=\"break\">Two</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(painted_blue_rect_count(&document.pages[0]), 1);
    assert_eq!(painted_blue_rect_count(&document.pages[1]), 1);
}

#[tokio::test]
async fn fixed_position_replays_full_paint_fragment_on_each_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 90pt; margin: 10pt } body, div, p { margin: 0; font-size: 10pt; line-height: 10pt } .fixed { position: fixed; left: 0; top: 0; width: 30pt; height: 15pt; background: #0000ff; color: white } .break { break-before: page }</style><div class=\"fixed\">Pin</div><p>One</p><p class=\"break\">Two</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    for page in &document.pages {
        assert_eq!(painted_blue_rect_count(page), 1);
        assert!(page.operations().iter().any(|operation| matches!(
            operation,
            quire::PaintOperation::Line(index)
                if page.lines().get(*index).is_some_and(|line| line.text == "Pin")
        )));
    }
}

#[tokio::test]
async fn fixed_position_transaction_leaves_no_temporary_paint_primitives() {
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 90pt; margin: 10pt } body, div, p { margin: 0; font-size: 10pt; line-height: 10pt } .fixed { position: fixed; left: 0; top: 0; width: 40pt; height: 16pt; background: #0000ff; color: white } .break { break-before: page }</style><div class=\"fixed\">Pin</div><p>One</p><p class=\"break\">Two</p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default()).await
    .unwrap();

    assert!(pdf_searchable_text(&pdf).starts_with("%PDF-1.4"));
}

#[tokio::test]
async fn absolute_position_transaction_replays_inserted_backgrounds() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .flow { width: 30pt; height: 30pt; background: #ff0000 } .abs { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #0000ff }</style><div class=\"abs\"></div><div class=\"flow\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    document.validate_paint_operations().unwrap();
    assert_eq!(
        painted_red_blue_rect_fills(&document.pages[0]),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 0, 255)]
    );
    assert!(
        document
            .write_pdf_bytes(&crate::PdfOptions::default())
            .is_ok()
    );
}

#[tokio::test]
async fn fixed_position_z_index_applies_on_each_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 90pt; margin: 10pt } body, div { margin: 0 } .fixed { position: fixed; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #0000ff } .flow { width: 30pt; height: 30pt; background: #ff0000 } .break { break-before: page }</style><div class=\"fixed\"></div><div class=\"flow\"></div><div class=\"flow break\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_red_blue_rect_fills(&document.pages[0]),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 0, 255)]
    );
    assert_eq!(
        painted_red_blue_rect_fills(&document.pages[1]),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 0, 255)]
    );
}

#[tokio::test]
async fn multiple_negative_positioned_siblings_sort_by_z_index() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .flow { width: 30pt; height: 30pt; background: #ff0000 } .low { position: absolute; z-index: -3; left: 0; top: 0; width: 30pt; height: 30pt; background: #0000ff } .high { position: absolute; z-index: -1; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 }</style><div class=\"high\"></div><div class=\"flow\"></div><div class=\"low\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[
                CssColor::new(0, 0, 255),
                CssColor::new(0, 255, 0),
                CssColor::new(255, 0, 0),
            ],
        ),
        vec![
            CssColor::new(0, 0, 255),
            CssColor::new(0, 255, 0),
            CssColor::new(255, 0, 0),
        ]
    );
}

#[tokio::test]
async fn z_index_auto_and_zero_share_source_order_level() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .auto { position: absolute; left: 0; top: 0; width: 30pt; height: 30pt; background: #0000ff } .zero { position: absolute; z-index: 0; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"zero\"></div><div class=\"auto\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_red_blue_rect_fills(&document.pages[0]),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 0, 255)]
    );
}

#[tokio::test]
async fn positioned_inline_z_index_offsets_split_block_segment() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 200px 200px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"position: relative; z-index: 1; background: red; width: 100px; height: 100px;\"></div>\
         <span style=\"position: relative; z-index: 2; top: -100px;\">\
           <div style=\"background: green; width: 100px; height: 100px;\"></div>\
         </span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = filled_rect(page, CssColor::new(255, 0, 0));
    let green = filled_rect(page, CssColor::new(0, 128, 0));
    assert!(
        (green.x() - red.x()).abs() < 0.01
            && (green.y() - red.y()).abs() < 0.01
            && (green.width() - red.width()).abs() < 0.01
            && (green.height() - red.height()).abs() < 0.01,
        "split block should move with positioned inline: red={red:?} green={green:?}",
    );
    assert!(
        first_rect_paint_operation_index(page, CssColor::new(0, 128, 0))
            > first_rect_paint_operation_index(page, CssColor::new(255, 0, 0)),
        "positioned inline z-index should paint split block above red: operations={:?}",
        page.operations()
    );
}

#[tokio::test]
async fn positioned_context_preserves_internal_inline_above_negative_child() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { position: absolute; z-index: 1; left: 0; top: 0; width: 80pt; height: 30pt; color: black; font-size: 10pt; line-height: 10pt } .child { position: absolute; z-index: -1; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\">Text<div class=\"child\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red_index = first_rect_paint_operation_index(&document.pages[0], CssColor::new(255, 0, 0));
    let text_index = document.pages[0]
        .operations()
        .iter()
        .position(|operation| {
            matches!(
                operation,
                quire::PaintOperation::Line(index)
                    if document.pages[0].lines().get(*index).is_some_and(|line| line.text == "Text")
            )
        })
        .unwrap();

    assert!(red_index < text_index);
}

#[tokio::test]
async fn flex_item_z_index_paints_above_later_item() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0 } .flex { display: flex; width: 60pt; height: 30pt } .first { z-index: 1; margin-right: -30pt; width: 30pt; height: 30pt; background: #0000ff } .second { width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"flex\"><div class=\"first\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_red_blue_rect_fills(&document.pages[0]),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 0, 255)]
    );
}

#[tokio::test]
async fn fixed_z_index_auto_traps_positive_positioned_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { position: fixed; left: 0; top: 0; width: 30pt; height: 30pt; background: #0000ff } .child { position: absolute; z-index: 999; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[
                CssColor::new(0, 0, 255),
                CssColor::new(0, 255, 0),
                CssColor::new(255, 0, 0),
            ],
        ),
        vec![
            CssColor::new(0, 0, 255),
            CssColor::new(0, 255, 0),
            CssColor::new(255, 0, 0),
        ]
    );
}

#[tokio::test]
async fn sticky_z_index_auto_traps_positive_positioned_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { position: sticky; top: 0; width: 30pt; height: 30pt; background: #0000ff } .child { position: absolute; z-index: 999; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[
                CssColor::new(0, 0, 255),
                CssColor::new(0, 255, 0),
                CssColor::new(255, 0, 0),
            ],
        ),
        vec![
            CssColor::new(0, 0, 255),
            CssColor::new(0, 255, 0),
            CssColor::new(255, 0, 0),
        ]
    );
}

#[tokio::test]
async fn relative_z_index_auto_does_not_trap_positive_positioned_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { position: relative; left: 0; top: 0; width: 30pt; height: 30pt } .child { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 0; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
        ),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
    );
}

#[tokio::test]
async fn float_fake_context_does_not_trap_positioned_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { float: left; width: 30pt; height: 30pt } .child { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 0; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
        ),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
    );
}

#[tokio::test]
async fn inline_block_fake_context_does_not_trap_positioned_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, div { margin: 0 } .parent { display: inline-block; width: 30pt; height: 30pt } .child { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 0; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
        ),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
    );
}

#[tokio::test]
async fn table_fake_context_does_not_trap_positioned_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, table, td { margin: 0; border-spacing: 0; padding: 0 } .parent { display: table; width: 30pt; height: 30pt } .child { position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 } .sibling { position: absolute; z-index: 0; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }</style><div class=\"parent\"><div style=\"display: table-row\"><div style=\"display: table-cell\"><div class=\"child\"></div></div></div></div><div class=\"sibling\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        painted_rect_fills(
            &document.pages[0],
            &[CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
        ),
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 255, 0)]
    );
}

#[tokio::test]
async fn effect_triggers_trap_positive_positioned_descendants() {
    for parent_declaration in [
        "opacity: .999",
        "transform: translate(0)",
        "filter: blur(0)",
        "mix-blend-mode: multiply",
        "isolation: isolate",
        "contain: paint",
        "content-visibility: auto",
        "clip-path: inset(0)",
        "mask-image: linear-gradient(black, black)",
        "will-change: transform, opacity",
    ] {
        assert_eq!(
            stacking_trigger_paint_order(parent_declaration).await,
            vec![
                CssColor::new(0, 0, 255),
                CssColor::new(0, 255, 0),
                CssColor::new(255, 0, 0),
            ],
            "{parent_declaration} should isolate positioned descendants"
        );
    }
}

#[tokio::test]
async fn layout_containment_does_not_trap_positive_positioned_descendants() {
    // Layout containment establishes an independent formatting context and a
    // containing block, but unlike paint containment it does not establish a
    // stacking context. The positive-z-index child therefore remains above
    // the later z-index:1 sibling.
    // <https://www.w3.org/TR/css-contain-1/#containment-layout>
    assert_eq!(
        stacking_trigger_paint_order("contain: layout").await,
        vec![
            CssColor::new(0, 0, 255),
            CssColor::new(255, 0, 0),
            CssColor::new(0, 255, 0),
        ],
    );
}

#[tokio::test]
async fn filter_blend_isolation_and_mask_contexts_write_pdf_groups() {
    let pdf = Html::from_string(
        "<style>@page { size: 160pt 160pt; margin: 10pt } body, div { margin: 0 } .box { width: 20pt; height: 20pt; background: #0000ff } .filter { filter: blur(0) } .blend { mix-blend-mode: multiply } .isolate { isolation: isolate } .mask { mask-image: linear-gradient(black, black) }</style><div class=\"box filter\"></div><div class=\"box blend\"></div><div class=\"box isolate\"></div><div class=\"box mask\"></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default()).await
    .unwrap();
    let pdf = pdf_searchable_text(&pdf);

    assert!(pdf.matches("/Group").count() >= 4);
    assert!(pdf.contains("/BM /Multiply"));
}

#[tokio::test]
async fn opacity_transform_and_overflow_context_writes_pdf_group() {
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 } .box { opacity: .5; transform: translate(5pt, 0); overflow: hidden; width: 20pt; height: 20pt; background: #0000ff } .child { width: 40pt; height: 20pt; background: #ff0000 }</style><div class=\"box\"><div class=\"child\"></div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default()).await
    .unwrap();
    let pdf = pdf_searchable_text(&pdf);

    assert!(pdf.contains("/Group"));
    assert!(pdf.contains("cm"));
    assert!(pdf.contains("W\nn"));
}

#[tokio::test]
async fn positioned_collapsed_table_paints_cell_text_above_late_border_rects() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body { margin: 0 } footer { position: relative; height: 100pt } table { position: absolute; bottom: 0; border-collapse: collapse; border: 20pt solid #eee; background: #eee } td { font-size: 12pt; line-height: 14pt }</style><footer><table><tr><td>Visible</td></tr></table></footer>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let line_index = document.pages[0]
        .lines()
        .iter()
        .position(|line| line.text == "Visible")
        .unwrap();
    let line_operation = document.pages[0]
        .operations()
        .iter()
        .position(|operation| *operation == quire::PaintOperation::Line(line_index))
        .unwrap();
    let line = &document.pages[0].lines()[line_index];
    let covering_rect_after_text = document.pages[0]
        .operations()
        .iter()
        .skip(line_operation + 1)
        .filter_map(|operation| match operation {
            quire::PaintOperation::Rect(index) => document.pages[0].rects().get(*index),
            _ => None,
        })
        .any(|rect| {
            rect.x() <= line.x()
                && rect.x() + rect.width() >= line.x()
                && rect.y() <= line.y()
                && rect.y() + rect.height() >= line.y()
        });

    assert!(!covering_rect_after_text);
}

fn painted_red_blue_rect_fills(page: &quire::Page) -> Vec<CssColor> {
    let red = CssColor::new(255, 0, 0);
    let blue = CssColor::new(0, 0, 255);
    painted_rect_fills(page, &[red, blue])
}

async fn stacking_trigger_paint_order(parent_declaration: &str) -> Vec<CssColor> {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 120pt 120pt; margin: 10pt }} body, div {{ margin: 0 }} .parent {{ {parent_declaration}; width: 30pt; height: 30pt; background: #0000ff }} .child {{ position: absolute; z-index: 999; left: 0; top: 0; width: 30pt; height: 30pt; background: #00ff00 }} .sibling {{ position: absolute; z-index: 1; left: 0; top: 0; width: 30pt; height: 30pt; background: #ff0000 }}</style><div class=\"parent\"><div class=\"child\"></div></div><div class=\"sibling\"></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    painted_rect_fills(
        &document.pages[0],
        &[
            CssColor::new(0, 0, 255),
            CssColor::new(0, 255, 0),
            CssColor::new(255, 0, 0),
        ],
    )
}

fn filled_rect(page: &quire::Page, color: CssColor) -> &quire::RenderedRect {
    page.rects()
        .iter()
        .find(|rect| rect.fill == Some(color))
        .expect("filled rect should be present")
}

fn emitted_rects_with_fills<'a>(
    page: &'a quire::Page,
    colors: &[CssColor],
) -> Vec<(usize, &'a quire::RenderedRect)> {
    page.paint_operations()
        .iter()
        .enumerate()
        .filter_map(|(operation_index, operation)| {
            let quire::PaintOperation::Rect(index) = operation else {
                return None;
            };
            let rect = page.rects().get(*index)?;
            rect.fill
                .is_some_and(|fill| colors.contains(&fill))
                .then_some((operation_index, rect))
        })
        .collect()
}

fn same_rect(left: &quire::RenderedRect, right: &quire::RenderedRect) -> bool {
    (left.x() - right.x()).abs() < 0.01
        && (left.y() - right.y()).abs() < 0.01
        && (left.width() - right.width()).abs() < 0.01
        && (left.height() - right.height()).abs() < 0.01
}

#[derive(Debug)]
struct PdfEmittedRect {
    fill: CssColor,
    rect: (f32, f32, f32, f32),
}

fn pdf_emitted_rects_with_fills(pdf: &[u8], colors: &[CssColor]) -> Vec<PdfEmittedRect> {
    let mut current_fill = None;
    let mut rects = Vec::new();
    let rendered = pdf_searchable_text(pdf);

    for line in rendered.lines().map(str::trim) {
        if let Some(fill) = parse_pdf_fill_rgb(line) {
            current_fill = Some(fill);
            continue;
        }
        let Some(rect) = parse_pdf_rect(line) else {
            continue;
        };
        let Some(fill) = current_fill else {
            continue;
        };
        if colors.contains(&fill) {
            rects.push(PdfEmittedRect { fill, rect });
        }
    }

    rects
}

fn parse_pdf_fill_rgb(line: &str) -> Option<CssColor> {
    let mut parts = line.split_whitespace();
    let red = parts.next()?.parse::<f32>().ok()?;
    let green = parts.next()?.parse::<f32>().ok()?;
    let blue = parts.next()?.parse::<f32>().ok()?;
    (matches!(parts.next()?, "rg" | "scn") && parts.next().is_none()).then_some(CssColor::new(
        pdf_color_component(red),
        pdf_color_component(green),
        pdf_color_component(blue),
    ))
}

fn pdf_color_component(component: f32) -> u8 {
    (component * 255.0).round().clamp(0.0, 255.0) as u8
}

fn parse_pdf_rect(line: &str) -> Option<(f32, f32, f32, f32)> {
    let mut parts = line.split_whitespace();
    let x = parts.next()?.parse::<f32>().ok()?;
    let y = parts.next()?.parse::<f32>().ok()?;
    let width = parts.next()?.parse::<f32>().ok()?;
    let height = parts.next()?.parse::<f32>().ok()?;
    (parts.next()? == "re" && parts.next().is_none()).then_some((x, y, width, height))
}

fn same_pdf_rect(left: (f32, f32, f32, f32), right: (f32, f32, f32, f32)) -> bool {
    (left.0 - right.0).abs() < 0.01
        && (left.1 - right.1).abs() < 0.01
        && (left.2 - right.2).abs() < 0.01
        && (left.3 - right.3).abs() < 0.01
}

fn assert_final_rect_fill(page: &quire::Page, rect: &quire::RenderedRect, expected: CssColor) {
    for x_fraction in [0.125, 0.5, 0.875] {
        for y_fraction in [0.125, 0.5, 0.875] {
            let x = rect.x() + rect.width() * x_fraction;
            let y = rect.y() + rect.height() * y_fraction;
            assert_eq!(
                final_rect_fill_at(page, x, y),
                Some(expected),
                "expected final fill at ({x}, {y}) inside {rect:?}",
            );
        }
    }
}

fn painted_rect_fills(page: &quire::Page, colors: &[CssColor]) -> Vec<CssColor> {
    page.operations()
        .iter()
        .filter_map(|operation| match operation {
            quire::PaintOperation::Rect(index) => page
                .rects()
                .get(*index)
                .and_then(|rect| rect.fill.filter(|fill| colors.contains(fill))),
            _ => None,
        })
        .collect()
}

fn painted_blue_rect_count(page: &quire::Page) -> usize {
    let blue = CssColor::new(0, 0, 255);
    page.operations()
        .iter()
        .filter(|operation| match operation {
            quire::PaintOperation::Rect(index) => page
                .rects()
                .get(*index)
                .is_some_and(|rect| rect.fill == Some(blue)),
            _ => false,
        })
        .count()
}
