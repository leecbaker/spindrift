use super::*;

#[tokio::test]
async fn definition_list_columns_preserve_description_borders() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 100pt; margin: 10pt }\
         html { align-items: center; display: flex; height: 100% }\
         body { display: flex; height: 40pt; margin: 0; width: 160pt }\
         section { flex: 1; position: relative }\
         aside { background: white; width: 40pt }\
         h1 { margin: 0; position: absolute; right: 0; top: 0 }\
         dl { background: black; columns: 4; column-gap: 0; margin: 0; padding: 1cm 0 }\
         dt, dd { box-sizing: border-box; margin: 0 }\
         dt { font-size: 9pt; font-weight: 700 }\
         dd { font-size: 35pt }\
         dl { dd { border-left: 2pt solid white } dd:first-of-type { border-left: 0 } }\
         </style>\
         <section><h1>Heading</h1><dl>\
           <dt>Term one</dt><dd>First description</dd>\
           <dt>Term two</dt><dd>Second description</dd>\
           <dt>Term three</dt><dd>Third description</dd>\
           <dt>Term four</dt><dd>Fourth description</dd>\
         </dl></section><aside></aside>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let borders = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::WHITE)
                && (rect.width() - 2.0).abs() < 0.01
                && rect.height() > 10.0
        })
        .count();

    assert_eq!(borders, 3);
}

#[tokio::test]
async fn supports_structural_pseudo_class_selectors() {
    let document = Html::from_string(
        "<style>p:first-child { color: red } p:nth-child(2) { color: blue } p:last-child { font-size: 16pt }</style><div><p>First</p><p>Second</p></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
    assert_eq!(document.pages[0].lines()[1].color, CssColor::new(0, 0, 255));
    assert_eq!(document.pages[0].lines()[1].font_size, 16.0);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lime_backgrounds = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 255, 0)))
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
    .render(&options)
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 255, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "\"A\"");
    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(0, 0, 255));
    assert_eq!(document.pages[0].lines()[1].color, CssColor::new(255, 0, 0));
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .skip(2)
            .any(|line| line.color == CssColor::BLACK)
    );
}

#[tokio::test]
async fn first_line_inheritance_preserves_equal_valued_descendant_declarations() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 100pt; margin: 10pt }\
         body, p { margin: 0; font-size: 12pt; line-height: 14pt }\
         p { color: black }\
         p::first-line { color: red }\
         .specified { color: black }\
         </style>\
         <p><span>Inherited</span> <span class='specified'>Specified</span><br>Later</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let inherited = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("Inherited"))
        .unwrap_or_else(|| {
            panic!(
                "inherited first-line fragment should render: {:?}",
                document.pages[0].lines()
            )
        });
    let specified = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("Specified"))
        .expect("specified first-line fragment should render");
    assert_eq!(inherited.color, CssColor::new(255, 0, 0));
    assert_eq!(specified.color, CssColor::BLACK);
}

#[tokio::test]
async fn block_in_inline_split_does_not_restart_originating_first_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 520pt 180pt; margin: 10pt }\
         body, div { margin: 0; font-size: 10pt; line-height: 14pt }\
         body > div { width: 480pt }\
         div::first-line { background: orange; color: orange }\
         </style>\
         <div><span>\
           First line.<br>\
           Second line.\
           <div>First line in 1st block box.<br>Second line.</div>\
           <div>First line in 2nd block box.<br>Second line.</div>\
           First line after block-in-inline is not ::first-line.<br>\
           Second line.\
         </span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();
    let line_texts = lines
        .iter()
        .map(|line| line.text.trim().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        line_texts,
        vec![
            "First line.",
            "Second line.",
            "First line in 1st block box.",
            "Second line.",
            "First line in 2nd block box.",
            "Second line.",
            "First line after block-in-inline is not ::first-line.",
            "Second line.",
        ]
    );

    let orange = CssColor::new(255, 165, 0);
    let orange_line_indexes = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.color == orange).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(
        orange_line_indexes,
        vec![0, 2, 4],
        "only the outer first line and each nested div first line should match ::first-line"
    );

    let mut orange_background_rows = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(orange))
        .map(|rect| (rect.y() * 100.0).round() as i32)
        .collect::<Vec<_>>();
    orange_background_rows.sort_unstable();
    orange_background_rows.dedup();
    assert_eq!(
        orange_background_rows.len(),
        3,
        "only three first-line background rows should paint: {orange_background_rows:?}"
    );
}

#[tokio::test]
async fn supports_of_type_structural_pseudo_class_selectors() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body, p, span { margin: 0; font-size: 10pt; line-height: 10pt } p:first-of-type { color: red } p:last-of-type { font-size: 16pt }</style><div><span>Skip</span><p>First</p><span>Middle</span><p>Second</p></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let first = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "First")
        .unwrap();
    let second = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Second")
        .unwrap();

    assert_eq!(first.color, CssColor::new(255, 0, 0));
    assert_eq!(second.font_size, 16.0);
}

#[tokio::test]
async fn supports_nested_css_for_flex_children() {
    let document = Html::from_string(
        "<style>ul.significant-relationships { margin: 0; padding: 0; display: flex; flex-direction: column; li { display: flex; justify-content: space-between; div:first-child, div:last-child { background: #dce0e5; padding: 7pt 5pt; } div:nth-child(2) { flex-grow: 1; height: 5pt; border-bottom: 1pt solid #dce0e5; align-self: center; width: 20pt; } } }</style><ul class=\"significant-relationships\"><li><div>A</div><div></div><div>B</div></li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(220, 224, 229)))
            .count()
            >= 2
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(220, 224, 229))
                && rect.width() > 10.0
                && rect.height() <= 1.0)
    );
}

#[tokio::test]
async fn padding_zero_removes_default_list_indent_for_flex_lists() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 } ul { margin: 0; padding: 0; display: flex; flex-direction: column; } li { display: flex; justify-content: space-between; } li div:first-child, li div:last-child { background: #dce0e5; padding: 2pt 3pt; } li div:nth-child(2) { flex-grow: 1; height: 4pt; border-bottom: 1pt dashed #dce0e5; align-self: center; width: 20pt; } </style><ul><li><div>Left</div><div></div><div>Right</div></li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let left_label = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(220, 224, 229)))
        .min_by(|left, right| left.x().total_cmp(&right.x()))
        .unwrap();

    assert_eq!(left_label.x(), 10.0);
}

#[tokio::test]
async fn flex_column_items_honor_vertical_margins() {
    let document = Html::from_string(
        "<style>ul.significant-relationships { margin: 0; padding: 0; display: flex; flex-direction: column; li { display: flex; justify-content: space-between; margin-top: 3pt; margin-bottom: 3pt; div:first-child, div:last-child { background: #dce0e5; padding: 7pt 5pt; } div:nth-child(2) { flex-grow: 1; height: 5pt; border-bottom: 1pt dashed #dce0e5; align-self: center; width: 20pt; } } }</style><ul class=\"significant-relationships\"><li><div>Parent</div><div></div><div>Child</div></li><li><div>Child</div><div></div><div>Parent</div></li><li><div>Self</div><div></div><div>Self</div></li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    let mut left_box_rows = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(220, 224, 229)))
        .filter(|rect| rect.height() > 5.0 && rect.x() < page.width() / 2.0)
        .map(|rect| (rect.y() * 10.0).round() as i32)
        .collect::<Vec<_>>();
    let mut right_box_rows = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(220, 224, 229)))
        .filter(|rect| rect.height() > 5.0 && rect.x() >= page.width() / 2.0)
        .map(|rect| (rect.y() * 10.0).round() as i32)
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let flex = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Flex")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((flex.y() - after.y() - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn flex_items_keep_small_explicit_cross_size_when_centered() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ul { margin: 0; padding: 0; display: flex; flex-direction: column; } li { display: flex; justify-content: space-between; } li div:first-child, li div:last-child { background: #dce0e5; padding: 7px 5px; } li div:nth-child(2) { flex-grow: 1; height: 5px; border-bottom: 1px dashed #dce0e5; align-self: center; width: 20px; } </style><ul><li><div>Parent</div><div></div><div>Child</div></li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let fill = CssColor::new(220, 224, 229);
    let left_label = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(fill) && rect.height() > 10.0)
        .min_by(|left, right| left.x().total_cmp(&right.x()))
        .unwrap();
    let connector = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(fill)
                && rect.height() <= 1.0
                && rect.width() > 1.0
                && rect.width() < 4.0
        })
        .max_by(|left, right| {
            let left_x = left.x();
            let right_x = right.x();
            left_x.total_cmp(&right_x)
        })
        .unwrap();

    let label_center = left_label.y() + left_label.height() / 2.0;
    let connector_center = connector.y() + connector.height() / 2.0;
    assert!((connector_center - label_center).abs() < 4.0);
}

#[tokio::test]
async fn renders_and_deduplicates_page_margin_background_images() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 100pt 100pt; margin: 10pt; @top-left {{ content: \"\"; background: url({png}) no-repeat 0 0, black; background-size: 5pt 5pt; height: 10pt; width: 100%; }} }} article {{ display: block; break-before: page; }}</style><p>One</p><article>Two</article>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[1].images().len(), 1);
    assert_eq!(document.pages[0].images()[0].width(), 5.0);
    assert_eq!(document.pages[0].images()[0].height(), 5.0);
    assert_eq!(
        document.pages[0].images()[0].sampling,
        crate::document::paint::images::RasterSampling::Auto
    );

    let pdf = document
        .write_pdf_bytes(&crate::PdfOptions::default())
        .unwrap();
    let rendered = pdf_searchable_text(&pdf);
    // The two page-margin images share one source. The PDF writer may retain
    // that source as one Image XObject or emit each uniform opaque draw as a
    // calibrated fill.
    assert!(
        rendered.matches("/Subtype /Image").count() == 1
            || rendered.matches("/CSsRGB cs").count() >= 2,
        "{rendered}"
    );
    if rendered.contains("/Subtype /Image") {
        assert!(rendered.contains("/Interpolate false"));
    }
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let margin_text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(margin_text.contains("[A]"), "{margin_text}");
    assert!(margin_text.contains("..."), "{margin_text}");
    assert!(margin_text.contains("1 fallback"), "{margin_text}");
    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(document.pages[0].images()[0].pixel_width(), 1);
    assert_eq!(document.pages[0].images()[0].pixel_height(), 1);
    assert!(document.pages[0].images()[0].y() >= 68.0);
}

#[tokio::test]
async fn page_margin_text_decoration_paints_text_primitives() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 12pt;\
           @top-left { content: \"Decor\"; text-decoration: underline line-through; color: red; font-size: 10pt; line-height: 10pt }\
         }\
         body { margin: 0 }\
         </style><p>Body</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red_decoration_rects = document.pages[0].rects().iter().filter(|rect| {
        rect.fill == Some(CssColor::new(255, 0, 0))
            && rect.width() > rect.height()
            && rect.height() > 0.0
    });
    assert!(red_decoration_rects.count() >= 2);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let margin_text_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "AB")
        .collect::<Vec<_>>();
    assert!(margin_text_lines.len() > 2);
    assert_eq!(
        margin_text_lines
            .iter()
            .filter(|line| line.color == CssColor::BLACK)
            .count(),
        1
    );
    assert_eq!(
        document.pages[0]
            .lines()
            .iter()
            .filter(|line| line.text == "•" && line.color == CssColor::new(0, 0, 255))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let margin_text_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "AB")
        .collect::<Vec<_>>();
    assert!(
        margin_text_lines
            .iter()
            .any(|line| line.color == CssColor::new(0, 128, 0))
    );
    assert!(
        margin_text_lines
            .iter()
            .any(|line| line.color.components()[0] == 0.0
                && line.color.components()[1] > 0.4
                && line.color.alpha() < 1.0)
    );
    assert!(
        !margin_text_lines
            .iter()
            .any(|line| line.color.components()[0] > 0.9)
    );
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "first")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "left")
    );
    assert!(
        !document.pages[0]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages[0].rects().iter().any(|rect| {
        (rect.x() - 0.0).abs() < 0.01
            && (rect.y() - 40.0).abs() < 0.01
            && (rect.width() - 10.0).abs() < 0.01
            && (rect.height() - 20.0).abs() < 0.01
    }));
    assert!(document.pages[0].rects().iter().any(|rect| {
        (rect.x() - 90.0).abs() < 0.01
            && (rect.y() - 40.0).abs() < 0.01
            && (rect.width() - 10.0).abs() < 0.01
            && (rect.height() - 20.0).abs() < 0.01
    }));
    assert!(document.pages[0].rects().iter().any(|rect| {
        (rect.x() - 0.0).abs() < 0.01
            && (rect.y() - 90.0).abs() < 0.01
            && (rect.width() - 10.0).abs() < 0.01
            && (rect.height() - 10.0).abs() < 0.01
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(!document.pages[0].rects().iter().any(|rect| {
        rect.fill == Some(CssColor::BLACK)
            && ((rect.x() - 0.0).abs() < 0.01 || (rect.x() - 90.0).abs() < 0.01)
            && (rect.y() - 90.0).abs() < 0.01
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "base")
    );
    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "report")
    );
    assert!(
        document.pages[2]
            .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    let lines = document.pages[1]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        lines.contains(&"report left"),
        "expected named left page footer in {lines:?}"
    );
}

#[tokio::test]
async fn named_string_start_inside_moved_flex_item_uses_final_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(chapter, start); font-size: 8pt; line-height: 8pt } }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         .spacer { height: 80pt }\
         .flex { display: flex; flex-direction: column }\
         .item { height: 10pt; string-set: chapter attr(data-title) }\
         </style>\
         <div class=\"spacer\"></div><div class=\"flex\"><div class=\"item\" data-title=\"Flex Chapter\">Body</div></div>",
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

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Flex Chapter"),
        "named string should not be placed before the flex item fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Flex Chapter"),
        "named string should resolve from the moved flex item fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn running_element_start_inside_moved_flex_item_uses_item_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: element(header, start); font-size: 8pt; line-height: 8pt } }\
         body, div { margin: 0; font-size: 10pt; line-height: 10pt }\
         .spacer { height: 80pt }\
         .flex { display: flex; flex-direction: column }\
         .header { position: running(header); height: 10pt }\
         </style>\
         <div class=\"spacer\"></div><div class=\"flex\"><div class=\"header\">Flex Header</div><div>Body</div></div>",
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

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        page_lines[1].contains(&"Flex Header"),
        "running element should resolve from the moved flex item fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn flex_item_inline_multicol_balances_and_paints_rule() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         div { width: 240pt; background: blue; display: flex; justify-content: space-around }\
         p { font-family: monospace; font-size: 12pt; line-height: 12pt; background: yellow;\
             column-gap: 12pt; column-rule: 12pt solid lime; columns: 2; width: 150pt; margin: 0 }\
         </style>\
         <div><p>one two three four five</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let text = page
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    for word in ["one", "two", "three", "four", "five"] {
        assert!(text.split_whitespace().any(|part| part == word), "{text:?}");
    }

    let has_lime_rule_stroke = page.strokes().iter().any(|stroke| {
        stroke.color == CssColor::new(0, 255, 0)
            && (stroke.stroke_width.points() - 12.0).abs() < 0.01
            && (stroke.x1() - stroke.x2()).abs() < 0.01
    });
    let has_lime_rule_fill = page.rects().iter().any(|rect| {
        rect.fill == Some(CssColor::new(0, 255, 0))
            && (rect.width() - 12.0).abs() < 0.01
            && rect.height() > 0.0
    });
    assert!(
        has_lime_rule_stroke || has_lime_rule_fill,
        "expected lime column rule in strokes={:?}, rects={:?}",
        page.strokes(),
        page.rects()
    );
}

#[tokio::test]
async fn definition_lists_can_lay_out_term_groups_in_columns() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, dl, dt, dd { margin: 0; font-size: 10pt; line-height: 10pt } dl { columns: 4; column-gap: 4pt; width: 160pt }</style><dl><dt>Flight</dt><dd>AB123</dd><dt>Gate</dt><dd>17</dd><dt>Seat</dt><dd>4A</dd><dt>Zone</dt><dd>2</dd></dl>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(
        page.lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Flight", "AB123", "Gate", "17", "Seat", "4A", "Zone", "2"]
    );

    let term_lines = page.lines().iter().step_by(2).collect::<Vec<_>>();
    for pair in term_lines.windows(2) {
        assert!((pair[1].x() - pair[0].x() - 41.0).abs() < 0.01);
        assert!((pair[1].y() - pair[0].y()).abs() < 0.01);
    }

    for group in page.lines().chunks(2) {
        assert!((group[0].x() - group[1].x()).abs() < 0.01);
        assert!((group[0].y() - group[1].y() - 10.0).abs() < 0.01);
    }
}

#[tokio::test]
async fn definition_list_multicol_align_content_uses_multicol_overflow_defaults() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt }\
         body, dl, dt, dd { margin: 0; font-size: 10pt; line-height: 10pt }\
         dl { columns: 2; column-gap: 10pt; width: 90pt; height: 10pt; align-content: center; margin-bottom: 20pt; background: red }\
         .safe { align-content: safe center; background: blue }</style>\
         <dl><dt>Default</dt><dd>A</dd><dt>Next</dt><dd>B</dd></dl>\
         <dl class=\"safe\"><dt>Safe</dt><dd>A</dd><dt>Next</dt><dd>B</dd></dl>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("{text} should render"))
    };
    let default = line("Default");
    let safe = line("Safe");
    let default_box = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("default multicol background should paint");
    let safe_box = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("safe multicol background should paint");

    let default_distance_from_top = default_box.y() + default_box.height() - default.y();
    let safe_distance_from_top = safe_box.y() + safe_box.height() - safe.y();
    assert!(
        (safe_distance_from_top - default_distance_from_top - 5.0).abs() < 0.5,
        "default multicol center overflow should remain unsafe while safe center falls back to block-start: default={default:?}, default_box={default_box:?}, safe={safe:?}, safe_box={safe_box:?}"
    );
}

#[tokio::test]
async fn multicol_auto_fill_advances_definite_blocks_sequentially() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 140pt; margin: 10pt }\
         body, div { margin: 0 }\
         .columns { columns: 2; column-fill: auto; column-gap: 0; width: 100pt; height: 100pt }\
         .first { width: 50pt; height: 60pt; background: red }\
         .second { width: 50pt; height: 60pt; background: blue }\
         </style>\
         <div class=\"columns\"><div class=\"first\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("missing {color:?} rectangle"))
    };
    let first = rect(CssColor::new(255, 0, 0));
    let second = rect(CssColor::new(0, 0, 255));
    assert!(
        (second.x() - first.x() - 50.0).abs() < 0.01,
        "{first:?} {second:?}"
    );
    assert!(
        (second.y() - first.y()).abs() < 0.01,
        "{first:?} {second:?}"
    );
}

#[tokio::test]
async fn column_span_all_separates_balanced_column_sets() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 140pt; margin: 10pt }\
         body, div { margin: 0 }\
         .columns { columns: 2; column-gap: 0; width: 100pt }\
         .item { height: 20pt; background: lime }\
         .spanner { column-span: all; width: 100pt; height: 10pt; background: blue }\
         </style>\
         <div class=\"columns\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"spanner\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lime = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 255, 0)))
        .collect::<Vec<_>>();
    let spanner = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("spanner background should paint");
    assert_eq!(lime.len(), 2, "{:?}", document.pages[0].rects());
    assert!((lime[1].x() - lime[0].x()).abs() > 49.0, "{lime:?}");
    assert!((spanner.width() - 100.0).abs() < 0.01, "{spanner:?}");
}

#[tokio::test]
async fn size_containment_sizes_as_empty_but_lays_out_contents_in_place() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 140pt; margin: 10pt }\
         body, div { margin: 0 }\
         .contained { contain: size; width: 40pt }\
         .overflow { width: 40pt; height: 40pt; background: red }\
         .following { width: 40pt; height: 10pt; background: lime }\
         </style>\
         <div class=\"contained\"><div class=\"overflow\"></div></div><div class=\"following\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("missing {color:?} rectangle"))
    };
    let overflow = rect(CssColor::new(255, 0, 0));
    let following = rect(CssColor::new(0, 255, 0));
    assert!((following.y() + following.height() - overflow.y() - overflow.height()).abs() < 0.01);
}

#[tokio::test]
async fn layout_containment_allows_forced_column_break_in_shrink_to_fit_multicol() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 460pt; margin: 10pt }\
         body, article, div { margin: 0 }\
         article { columns: 2 100pt; column-fill: auto; column-gap: 0; float: left; height: 400pt }\
         .yellow { border-top: 100pt solid yellow }\
         .blue { border-top: 100pt solid blue; contain: layout }\
         .orange { border-top: 100pt solid orange; break-before: column }\
         </style>\
         <article><div class=\"yellow\"></div><div class=\"blue\"><div class=\"orange\"></div></div></article>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("missing {color:?} rectangle"))
    };
    let yellow = rect(CssColor::new(255, 255, 0));
    let blue = rect(CssColor::new(0, 0, 255));
    let orange = rect(CssColor::new(255, 165, 0));
    assert!(
        (blue.y() - yellow.y() + 100.0).abs() < 0.01,
        "{yellow:?} {blue:?}"
    );
    assert!(
        orange.x() > blue.x() + 0.01,
        "the contained child must consume its forced column break locally: {blue:?} {orange:?}"
    );
    assert!(orange.y() > blue.y() + 0.01, "{blue:?} {orange:?}");
}

#[tokio::test]
async fn auto_height_multicol_continues_overflow_column_rows_on_later_pages() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         body, section, div { margin: 0 }\
         section { columns: 2; column-fill: auto; column-gap: 0; width: 80pt }\
         div { height: 80pt }\
         .a { background: red } .b { background: blue } .c { background: lime }\
         .d { background: yellow } .e { background: fuchsia }\
         </style>\
         <section><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div><div class=\"d\"></div><div class=\"e\"></div></section>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 3);
    let expected = [
        vec![CssColor::new(255, 0, 0), CssColor::new(0, 0, 255)],
        vec![CssColor::new(0, 255, 0), CssColor::new(255, 255, 0)],
        vec![CssColor::new(255, 0, 255)],
    ];
    for (page, colors) in document.pages.iter().zip(expected) {
        for color in colors {
            assert!(
                page.rects().iter().any(|rect| rect.fill == Some(color)),
                "missing {color:?} on page: {:?}",
                page.rects()
            );
        }
    }
}
