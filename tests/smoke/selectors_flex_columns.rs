use super::*;

#[tokio::test]
async fn supports_structural_pseudo_class_selectors() {
    let document = Html::from_string(
        "<style>p:first-child { color: red } p:nth-child(2) { color: blue } p:last-child { font-size: 16pt }</style><div><p>First</p><p>Second</p></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].color, Color::new(255, 0, 0));
    assert_eq!(document.pages[0].lines[1].color, Color::new(0, 0, 255));
    assert_eq!(document.pages[0].lines[1].font_size, 16.0);
}

#[tokio::test]
async fn supports_nth_last_child_of_selector_list() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 160pt; margin: 10pt }\
         body, p { margin: 0; font-size: 10pt; line-height: 12pt }\
         p:nth-last-child(even of .webkit, .fast) { background-color: lime }\
         </style>\
         <p class=\"webkit\">First</p>\
         <p class=\"other\">Other</p>\
         <p class=\"fast\">Second</p>\
         <p class=\"webkit\">Third</p>\
         <p class=\"fast\">Fourth</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lime_backgrounds = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 255, 0)))
        .count();
    assert_eq!(lime_backgrounds, 2);
}

#[tokio::test]
async fn target_fragment_option_styles_target_and_target_within() {
    let options = RenderOptions {
        target_fragment: Some("hit".to_string()),
        ..RenderOptions::default()
    };
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 120pt; margin: 10pt }\
         section, p { margin: 0; width: 80pt; height: 20pt }\
         section:target-within { border-top: 2pt solid blue }\
         p:target { background: lime }\
         </style>\
         <section><p id=\"hit\">Target</p></section>",
    )
    .render_async(&options)
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 0, 255)))
    );
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 255, 0)))
    );
}

#[tokio::test]
async fn supports_first_line_and_first_letter_pseudo_elements() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 120pt; margin: 10pt }\
         body, p { margin: 0; font-size: 12pt; line-height: 14pt }\
         p { width: 60pt; color: black }\
         p::first-line { color: red }\
         p::first-letter { color: blue }\
         </style>\
         <p>&quot;A&quot;lpha beta gamma delta epsilon</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "\"A\"");
    assert_eq!(document.pages[0].lines[0].color, Color::new(0, 0, 255));
    assert_eq!(document.pages[0].lines[1].color, Color::new(255, 0, 0));
    assert!(
        document.pages[0]
            .lines
            .iter()
            .skip(2)
            .any(|line| line.color == Color::BLACK)
    );
}

#[tokio::test]
async fn supports_of_type_structural_pseudo_class_selectors() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body, p, span { margin: 0; font-size: 10pt; line-height: 10pt } p:first-of-type { color: red } p:last-of-type { font-size: 16pt }</style><div><span>Skip</span><p>First</p><span>Middle</span><p>Second</p></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let first = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "First")
        .unwrap();
    let second = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Second")
        .unwrap();

    assert_eq!(first.color, Color::new(255, 0, 0));
    assert_eq!(second.font_size, 16.0);
}

#[tokio::test]
async fn supports_nested_css_for_flex_children() {
    let document = Html::from_string(
        "<style>ul.significant-relationships { margin: 0; padding: 0; display: flex; flex-direction: column; li { display: flex; justify-content: space-between; div:first-child, div:last-child { background: #dce0e5; padding: 7pt 5pt; } div:nth-child(2) { flex-grow: 1; height: 5pt; border-bottom: 1pt solid #dce0e5; align-self: center; width: 20pt; } } }</style><ul class=\"significant-relationships\"><li><div>A</div><div></div><div>B</div></li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(220, 224, 229)))
            .count()
            >= 2
    );
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(220, 224, 229))
                && rect.width > 10.0
                && rect.height <= 1.0)
    );
}

#[tokio::test]
async fn padding_zero_removes_default_list_indent_for_flex_lists() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 } ul { margin: 0; padding: 0; display: flex; flex-direction: column; } li { display: flex; justify-content: space-between; } li div:first-child, li div:last-child { background: #dce0e5; padding: 2pt 3pt; } li div:nth-child(2) { flex-grow: 1; height: 4pt; border-bottom: 1pt dashed #dce0e5; align-self: center; width: 20pt; } </style><ul><li><div>Left</div><div></div><div>Right</div></li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let left_label = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(220, 224, 229)))
        .min_by(|left, right| left.x.total_cmp(&right.x))
        .unwrap();

    assert_eq!(left_label.x, 10.0);
}

#[tokio::test]
async fn flex_column_items_honor_vertical_margins() {
    let document = Html::from_string(
        "<style>ul.significant-relationships { margin: 0; padding: 0; display: flex; flex-direction: column; li { display: flex; justify-content: space-between; margin-top: 3pt; margin-bottom: 3pt; div:first-child, div:last-child { background: #dce0e5; padding: 7pt 5pt; } div:nth-child(2) { flex-grow: 1; height: 5pt; border-bottom: 1pt dashed #dce0e5; align-self: center; width: 20pt; } } }</style><ul class=\"significant-relationships\"><li><div>Parent</div><div></div><div>Child</div></li><li><div>Child</div><div></div><div>Parent</div></li><li><div>Self</div><div></div><div>Self</div></li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    let mut left_box_rows = page
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(220, 224, 229)))
        .filter(|rect| rect.height > 5.0 && rect.x < page.width / 2.0)
        .map(|rect| (rect.y * 10.0).round() as i32)
        .collect::<Vec<_>>();
    let mut right_box_rows = page
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(220, 224, 229)))
        .filter(|rect| rect.height > 5.0 && rect.x >= page.width / 2.0)
        .map(|rect| (rect.y * 10.0).round() as i32)
        .collect::<Vec<_>>();
    left_box_rows.sort_unstable();
    left_box_rows.dedup();
    right_box_rows.sort_unstable();
    right_box_rows.dedup();

    assert_eq!(left_box_rows.len(), 3);
    assert_eq!(right_box_rows.len(), 3);
}

#[tokio::test]
async fn flex_container_margin_bottom_separates_following_block() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 20pt } body, p, div { margin: 0; font-size: 10pt; line-height: 10pt } .flex { display: flex; margin-bottom: 40pt } .flex div { margin: 0 }</style><div class=\"flex\"><div>Flex</div></div><p>After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let flex = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Flex")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((flex.y - after.y - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn flex_items_keep_small_explicit_cross_size_when_centered() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ul { margin: 0; padding: 0; display: flex; flex-direction: column; } li { display: flex; justify-content: space-between; } li div:first-child, li div:last-child { background: #dce0e5; padding: 7px 5px; } li div:nth-child(2) { flex-grow: 1; height: 5px; border-bottom: 1px dashed #dce0e5; align-self: center; width: 20px; } </style><ul><li><div>Parent</div><div></div><div>Child</div></li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let fill = Color::new(220, 224, 229);
    let left_label = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(fill) && rect.height > 10.0)
        .min_by(|left, right| left.x.total_cmp(&right.x))
        .unwrap();
    let connector = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(fill) && rect.height <= 1.0 && rect.width > 1.0 && rect.width < 4.0
        })
        .max_by(|left, right| {
            let left_x = left.x;
            let right_x = right.x;
            left_x.total_cmp(&right_x)
        })
        .unwrap();

    let label_center = left_label.y + left_label.height / 2.0;
    let connector_center = connector.y + connector.height / 2.0;
    assert!((connector_center - label_center).abs() < 4.0);
}

#[tokio::test]
async fn renders_and_deduplicates_page_margin_background_images() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 100pt 100pt; margin: 10pt; @top-left {{ content: \"\"; background: url({png}) no-repeat 0 0, black; background-size: 5pt 5pt; height: 10pt; width: 100%; }} }} article {{ display: block; break-before: page; }}</style><p>One</p><article>Two</article>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(document.pages[1].images.len(), 1);
    assert_eq!(document.pages[0].images[0].width, 5.0);
    assert_eq!(document.pages[0].images[0].height, 5.0);
    assert!(document.pages[0].images[0].interpolate);

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert_eq!(rendered.matches("/Subtype /Image").count(), 1);
    assert!(rendered.contains("/Interpolate true"));
}

#[tokio::test]
async fn page_margin_content_supports_mixed_generated_items_and_images() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
         "<style>\
         @page {{ size: 120pt 80pt; margin: 12pt;\
           @top-left {{ content: open-quote \"A\" close-quote leader(dotted) counter(page) attr(data-title, \" fallback\") url({png}); quotes: \"[\" \"]\"; font-size: 10pt; line-height: 10pt }}\
         }}\
         body {{ margin: 0 }}\
         </style><p>Body</p>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "[A].1 fallback")
    );
    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(document.pages[0].images[0].pixel_width, 1);
    assert_eq!(document.pages[0].images[0].pixel_height, 1);
    assert!(document.pages[0].images[0].y >= 68.0);
}

#[tokio::test]
async fn page_margin_text_decoration_paints_text_strokes() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 12pt;\
           @top-left { content: \"Decor\"; text-decoration: underline line-through; color: red; font-size: 10pt; line-height: 10pt }\
         }\
         body { margin: 0 }\
         </style><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red_strokes = document.pages[0]
        .strokes
        .iter()
        .filter(|stroke| stroke.color == Color::new(255, 0, 0))
        .collect::<Vec<_>>();
    assert_eq!(red_strokes.len(), 2);
    assert!(red_strokes.iter().all(|stroke| stroke.x2 > stroke.x1));
}

#[tokio::test]
async fn page_margin_text_shadow_and_emphasis_use_inline_paint_order() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 90pt; margin: 14pt;\
           @top-left { content: \"AB\"; color: black; font-size: 10pt; line-height: 12pt; text-shadow: 2pt 1pt 3pt rgba(255, 0, 0, 0.8); text-emphasis: filled dot blue }\
         }\
         body { margin: 0 }\
         </style><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let margin_text_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text == "AB")
        .collect::<Vec<_>>();
    assert!(margin_text_lines.len() > 2);
    assert_eq!(
        margin_text_lines
            .iter()
            .filter(|line| line.color == Color::BLACK)
            .count(),
        1
    );
    assert_eq!(
        document.pages[0]
            .lines
            .iter()
            .filter(|line| line.text == "•" && line.color == Color::new(0, 0, 255))
            .count(),
        2
    );
}

#[tokio::test]
async fn page_margin_text_shadow_currentcolor_resolves_against_margin_box_color() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 90pt; margin: 14pt; color: red; text-shadow: 0 0 3pt currentcolor;\
           @top-left { content: \"AB\"; color: green; font-size: 10pt; line-height: 12pt }\
         }\
         body { margin: 0 }\
         </style><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let margin_text_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text == "AB")
        .collect::<Vec<_>>();
    assert!(
        margin_text_lines
            .iter()
            .any(|line| line.color == Color::new(0, 128, 0))
    );
    assert!(
        margin_text_lines
            .iter()
            .any(|line| line.color.r == 0.0 && line.color.g > 0.4 && line.color.a < 1.0)
    );
    assert!(!margin_text_lines.iter().any(|line| line.color.r > 0.9));
}

#[tokio::test]
async fn page_margin_boxes_match_first_left_and_right_page_selectors() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt; @bottom-center { content: \"base\" } }\
         @page :right { @bottom-center { content: \"right\" } }\
         @page :left { @bottom-center { content: \"left\" } }\
         @page :first { @bottom-center { content: \"first\" } }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt } article { display: block; break-before: page }\
         </style><p>One</p><article>Two</article>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "first")
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "left")
    );
    assert!(
        !document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "right")
    );
}

#[tokio::test]
async fn side_and_corner_page_margin_boxes_use_page_margin_regions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt;\
           @left-middle { content: \"L\"; background-color: black; height: 20pt }\
           @right-middle { content: \"R\"; background-color: black; height: 20pt }\
           @top-left-corner { content: \"\"; background-color: black }\
         }\
         body { margin: 0 }\
         </style><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages[0].rects.iter().any(|rect| {
        (rect.x - 0.0).abs() < 0.01
            && (rect.y - 40.0).abs() < 0.01
            && (rect.width - 10.0).abs() < 0.01
            && (rect.height - 20.0).abs() < 0.01
    }));
    assert!(document.pages[0].rects.iter().any(|rect| {
        (rect.x - 90.0).abs() < 0.01
            && (rect.y - 40.0).abs() < 0.01
            && (rect.width - 10.0).abs() < 0.01
            && (rect.height - 20.0).abs() < 0.01
    }));
    assert!(document.pages[0].rects.iter().any(|rect| {
        (rect.x - 0.0).abs() < 0.01
            && (rect.y - 90.0).abs() < 0.01
            && (rect.width - 10.0).abs() < 0.01
            && (rect.height - 10.0).abs() < 0.01
    }));
}

#[tokio::test]
async fn page_margin_boxes_without_generated_content_are_not_created() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt;\
           @top-left-corner { background-color: black }\
           @top-right-corner { content: normal; background-color: black }\
         }\
         body { margin: 0 }\
         </style><p>Body</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(!document.pages[0].rects.iter().any(|rect| {
        rect.fill == Some(Color::BLACK)
            && ((rect.x - 0.0).abs() < 0.01 || (rect.x - 90.0).abs() < 0.01)
            && (rect.y - 90.0).abs() < 0.01
    }));
}

#[tokio::test]
async fn named_pages_select_named_page_margin_boxes() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt; @bottom-center { content: \"base\" } }\
         @page report { @bottom-center { content: \"report\" } }\
         body, p, section { margin: 0; font-size: 10pt; line-height: 10pt }\
         section { page: report }\
         </style><p>One</p><section>Two</section><p>Three</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "base")
    );
    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "report")
    );
    assert!(
        document.pages[2]
            .lines
            .iter()
            .any(|line| line.text == "base")
    );
}

#[tokio::test]
async fn named_pages_combine_with_left_page_pseudo_class() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt; @bottom-center { content: \"base\" } }\
         @page report { @bottom-center { content: \"report\" } }\
         @page report:left { @bottom-center { content: \"report left\" } }\
         body, p, section { margin: 0; font-size: 10pt; line-height: 10pt }\
         section { page: report }\
         </style><p>One</p><section>Two</section><p>Three</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    let lines = document.pages[1]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        lines.contains(&"report left"),
        "expected named left page footer in {lines:?}"
    );
}

#[tokio::test]
async fn definition_lists_can_lay_out_term_groups_in_columns() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, dl, dt, dd { margin: 0; font-size: 10pt; line-height: 10pt } dl { columns: 4; column-gap: 4pt; width: 160pt }</style><dl><dt>Flight</dt><dd>AB123</dd><dt>Gate</dt><dd>17</dd><dt>Seat</dt><dd>4A</dd><dt>Zone</dt><dd>2</dd></dl>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(
        page.lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Flight", "AB123", "Gate", "17", "Seat", "4A", "Zone", "2"]
    );

    let term_lines = page.lines.iter().step_by(2).collect::<Vec<_>>();
    for pair in term_lines.windows(2) {
        assert!((pair[1].x - pair[0].x - 41.0).abs() < 0.01);
        assert!((pair[1].y - pair[0].y).abs() < 0.01);
    }

    for group in page.lines.chunks(2) {
        assert!((group[0].x - group[1].x).abs() < 0.01);
        assert!((group[0].y - group[1].y - 10.0).abs() < 0.01);
    }
}
