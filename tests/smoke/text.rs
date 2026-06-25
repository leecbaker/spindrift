use super::*;

fn fragments_share_visual_line(lines: &[quire::RenderedLine]) -> bool {
    let Some(first) = lines.first() else {
        return true;
    };
    lines.iter().all(|line| (line.y - first.y).abs() < 3.0)
}

#[tokio::test]
async fn text_shadow_paints_offset_text_without_affecting_layout_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; font-size: 12pt; line-height: 14pt } p { margin: 0; text-shadow: 4pt 2pt red }</style><p>Shadow</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    let shadow_lines = lines
        .iter()
        .filter(|line| line.text == "Shadow")
        .collect::<Vec<_>>();
    assert_eq!(shadow_lines.len(), 2);
    assert!(
        shadow_lines
            .iter()
            .any(|line| line.color == Color::new(255, 0, 0))
    );
    assert!(shadow_lines.iter().any(|line| line.color == Color::BLACK));
    let black = shadow_lines
        .iter()
        .find(|line| line.color == Color::BLACK)
        .unwrap();
    let red = shadow_lines
        .iter()
        .find(|line| line.color == Color::new(255, 0, 0))
        .unwrap();
    assert!((red.x - black.x - 4.0).abs() < 0.1);
    assert!((black.y - red.y - 2.0).abs() < 0.1);
}

#[tokio::test]
async fn blurred_text_shadow_paints_translucent_replay_without_layout_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; font-size: 12pt; line-height: 14pt } p { margin: 0; text-shadow: 2pt 1pt 4pt rgba(255, 0, 0, 0.8) }</style><p>Blur</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let blur_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text == "Blur")
        .collect::<Vec<_>>();
    assert!(blur_lines.len() > 2);
    assert_eq!(
        blur_lines
            .iter()
            .filter(|line| line.color == Color::BLACK)
            .count(),
        1
    );
    assert!(
        blur_lines
            .iter()
            .any(|line| line.color.r > 0.9 && line.color.a < 0.8)
    );
}

#[tokio::test]
async fn inherited_text_shadow_currentcolor_resolves_on_painting_element() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body { margin: 0; color: red; text-shadow: 0 0 10px currentcolor } p { margin: 0; color: green; text-shadow: inherit; font: 18pt Georgia, serif }</style><p>Green shadow</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text == "Green shadow")
        .collect::<Vec<_>>();
    assert!(
        text_lines
            .iter()
            .any(|line| line.color == Color::new(0, 128, 0))
    );
    assert!(
        text_lines
            .iter()
            .any(|line| line.color.r == 0.0 && line.color.g > 0.4 && line.color.a < 1.0)
    );
    assert!(!text_lines.iter().any(|line| line.color.r > 0.9));
}

#[tokio::test]
async fn text_emphasis_marks_are_painted_without_changing_base_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; font-size: 12pt; line-height: 18pt } p { margin: 0; text-emphasis: filled dot red }</style><p>AB</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.contains(&"AB"));
    assert_eq!(texts.iter().filter(|text| **text == "•").count(), 2);
    assert!(!texts.iter().any(|text| text.contains("A•B")));
}

#[tokio::test]
async fn min_content_inline_sizing_counts_edges_and_atoms() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt }\
         p { width: min-content; background: red }\
         .pad { padding-left: 8pt; padding-right: 8pt; border-left: 2pt solid black }\
         .atom { display: inline-block; width: 36pt; height: 4pt; margin-left: 4pt }</style>\
         <p><span class=\"pad\">A</span><span class=\"atom\"></span></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let paragraph = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    assert!(
        paragraph.width >= 39.5,
        "min-content width should include the inline atom: {paragraph:?}"
    );
}

#[tokio::test]
async fn renders_dictionary_run_in_terms() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, dl, dt, dd { margin: 0; font-size: 10pt; line-height: 12pt } dt { display: run-in; font-weight: 700 } dt::after { content: \": \" }</style><dl><dt>alpha</dt><dd>first entry</dd></dl>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[0].text, "alpha: ");
    assert_eq!(lines[1].text, "first entry");
    assert!(fragments_share_visual_line(&lines[..2]));
    assert!(lines[0].x < lines[1].x);
}

#[tokio::test]
async fn run_in_before_flow_root_does_not_merge() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, h3, section { margin: 0; font-size: 10pt; line-height: 12pt } h3 { display: run-in } section { display: flow-root }</style><h3>Term</h3><section>Block</section>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "Term");
    assert_eq!(document.pages[0].lines[1].text, "Block");
}

#[tokio::test]
async fn run_in_with_block_descendant_stays_inline_with_target() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, h3, p, b { margin: 0; font-size: 10pt; line-height: 12pt } h3 { display: run-in } b { display: block }</style><h3>Term <b>block</b> tail </h3><p>Definition</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    let texts = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|text| text.trim() == "Term"));
    assert!(texts.iter().any(|text| text.trim() == "block"));
    assert!(texts.iter().any(|text| text.contains("tail")));
    assert!(texts.iter().any(|text| text.contains("Definition")));
    assert!(fragments_share_visual_line(lines));
}

#[tokio::test]
async fn run_in_list_item_keeps_marker_and_merges_text() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, h3, p { margin: 0; font-size: 10pt; line-height: 12pt } h3 { display: run-in list-item; list-style-position: inside; list-style-type: decimal } h3::after { content: \" \" }</style><h3>One</h3><p>Definition</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    let texts = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.contains(&"1."));
    assert!(texts.iter().any(|text| text.contains("One")));
    assert!(texts.iter().any(|text| text.contains("Definition")));
    assert!(fragments_share_visual_line(lines));
}

#[tokio::test]
async fn renders_basic_unordered_lists() {
    let document = Html::from_string("<ul><li>One</li><li>Two</li></ul>")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "\u{2022}");
    assert_eq!(document.pages[0].lines[1].text, "One");
    assert_eq!(document.pages[0].lines[2].text, "\u{2022}");
    assert_eq!(document.pages[0].lines[3].text, "Two");
    assert!(document.pages[0].lines[0].x < document.pages[0].lines[1].x);
}

#[tokio::test]
async fn renders_basic_ordered_lists() {
    let document = Html::from_string("<ol><li>One</li><li>Two</li></ol>")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "1.");
    assert_eq!(document.pages[0].lines[1].text, "One");
    assert_eq!(document.pages[0].lines[2].text, "2.");
    assert_eq!(document.pages[0].lines[3].text, "Two");
}

#[tokio::test]
async fn supports_basic_list_style_types() {
    let document = Html::from_string(
        "<ol style=\"list-style-type: lower-alpha\"><li>One</li><li>Two</li></ol><ul style=\"list-style-type: none\"><li>Plain</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "a.");
    assert_eq!(document.pages[0].lines[1].text, "One");
    assert_eq!(document.pages[0].lines[2].text, "b.");
    assert_eq!(document.pages[0].lines[3].text, "Two");
    assert_eq!(document.pages[0].lines[4].text, "Plain");
}

#[tokio::test]
async fn supports_common_builtin_list_marker_styles() {
    let document = Html::from_string(
        "<ol style=\"list-style-type: upper-roman\"><li>One</li><li>Two</li></ol><ol style=\"list-style-type: lower-roman\"><li>One</li></ol><ul style=\"list-style-type: circle\"><li>Circle</li></ul><ul style=\"list-style-type: square\"><li>Square</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["I.", "One"]));
    assert!(texts.windows(2).any(|pair| pair == ["II.", "Two"]));
    assert!(texts.windows(2).any(|pair| pair == ["i.", "One"]));
    assert!(texts.windows(2).any(|pair| pair == ["\u{25e6}", "Circle"]));
    assert!(texts.windows(2).any(|pair| pair == ["\u{25aa}", "Square"]));
}

#[tokio::test]
async fn supports_predefined_counter_style_markers() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 260pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ol { margin: 0 0 4pt 18pt; padding-left: 0 }</style>\
         <ol style=\"list-style-type: decimal-leading-zero\"><li>One</li></ol>\
         <ol style=\"list-style-type: lower-greek\"><li>One</li><li>Two</li></ol>\
         <ol start=\"10\" style=\"list-style-type: cjk-decimal\"><li>Ten</li></ol>\
         <ol start=\"12\" style=\"list-style-type: cjk-earthly-branch\"><li>Branch</li></ol>\
         <ol start=\"10\" style=\"list-style-type: cjk-heavenly-stem\"><li>Stem</li></ol>\
         <ol start=\"15\" style=\"list-style-type: hebrew\"><li>Hebrew</li></ol>\
         <ol start=\"100\" style=\"list-style-type: georgian\"><li>Georgian</li></ol>\
         <ol start=\"10\" style=\"list-style-type: armenian\"><li>Armenian</li></ol>\
         <ul style=\"list-style-type: disclosure-closed\"><li>Closed</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    let contains = |needle: &[&str]| texts.windows(needle.len()).any(|pair| pair == needle);
    assert!(contains(&["01.", "One"]));
    assert!(contains(&["α.", "One"]));
    assert!(contains(&["β.", "Two"]));
    assert!(contains(&["一〇、", "Ten"]));
    assert!(contains(&["亥、", "Branch"]));
    assert!(contains(&["癸、", "Stem"]));
    assert!(contains(&["טו.", "Hebrew"]));
    assert!(contains(&["რ.", "Georgian"]));
    assert!(contains(&["Ժ.", "Armenian"]));
    assert!(contains(&["▸", "Closed"]));
}

#[tokio::test]
async fn supports_complex_predefined_counter_style_markers() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 360pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ol { margin: 0 0 4pt 24pt; padding-left: 0 }</style>\
         <ol start=\"19999\" style=\"list-style-type: georgian\"><li>Georgian</li></ol>\
         <ol start=\"20000\" style=\"list-style-type: georgian\"><li>Georgian fallback</li></ol>\
         <ol start=\"0\" style=\"list-style-type: simp-chinese-informal\"><li>Zero</li></ol>\
         <ol start=\"10\" style=\"list-style-type: simp-chinese-informal\"><li>Ten</li></ol>\
         <ol start=\"11\" style=\"list-style-type: simp-chinese-informal\"><li>Eleven</li></ol>\
         <ol start=\"99\" style=\"list-style-type: simp-chinese-formal\"><li>Ninety nine</li></ol>\
         <ol start=\"100\" style=\"list-style-type: trad-chinese-informal\"><li>Hundred</li></ol>\
         <ol start=\"101\" style=\"list-style-type: trad-chinese-formal\"><li>Hundred one</li></ol>\
         <ol start=\"6001\" style=\"list-style-type: cjk-ideographic\"><li>Six thousand one</li></ol>\
         <ol start=\"100\" style=\"list-style-type: ethiopic-numeric\"><li>Hundred</li></ol>\
         <ol start=\"78010092\" style=\"list-style-type: ethiopic-numeric\"><li>Large</li></ol>\
         <ol start=\"0\" style=\"list-style-type: japanese-informal\"><li>Japanese zero</li></ol>\
         <ol start=\"0\" style=\"list-style-type: korean-hangul-formal\"><li>Korean zero</li></ol>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    for marker in [
        "ჵჰშჟთ.",
        "20000.",
        "零、",
        "十、",
        "十一、",
        "玖拾玖、",
        "一百、",
        "壹佰零壹、",
        "六千零一、",
        "፻/ ",
        "፸፰፻፩፼፺፪/ ",
        "〇、",
        "영, ",
    ] {
        assert!(
            texts.contains(&marker),
            "missing marker {marker:?}: {texts:?}"
        );
    }
}

#[tokio::test]
async fn supports_custom_counter_style_markers() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ol { margin: 0 0 4pt 18pt; padding-left: 0 }\
         @counter-style thumbs { system: cyclic; symbols: \"☝\"; suffix: \" \" }\
         @counter-style binary { system: numeric; symbols: \"0\" \"1\"; suffix: \") \" }\
         @counter-style binary-brackets { system: extends binary; prefix: \"[\"; suffix: \"] \" }\
         @counter-style signed-binary { system: numeric; symbols: \"0\" \"1\"; negative: \"(\" \")\"; pad: 4 \"0\"; prefix: \"[\"; suffix: \"] \" }\
         @counter-style fixed-names { system: fixed 3; symbols: \"c\" \"d\"; suffix: \" \" }\
         @counter-style tally { system: additive; additive-symbols: 5 \"V\", 1 \"I\"; suffix: \" \" }\
         .marker-content li::marker { content: counter(list-item, binary) \": \" }</style>\
         <ol style=\"list-style-type: thumbs\"><li>One</li></ol>\
         <ol start=\"5\" style=\"list-style-type: binary\"><li>Five</li></ol>\
         <ol start=\"5\" style=\"list-style-type: binary-brackets\"><li>Bracketed</li></ol>\
         <ol start=\"-3\" style=\"list-style-type: signed-binary\"><li>Negative</li></ol>\
         <ol start=\"3\" style=\"list-style-type: fixed-names\"><li>Three</li><li>Four</li></ol>\
         <ol start=\"6\" style=\"list-style-type: tally\"><li>Six</li></ol>\
         <ol start=\"3\" class=\"marker-content\"><li>Three</li></ol>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    let contains = |needle: &[&str]| texts.windows(needle.len()).any(|pair| pair == needle);
    assert!(contains(&["☝ ", "One"]));
    assert!(contains(&["101) ", "Five"]));
    assert!(contains(&["[101] ", "Bracketed"]));
    assert!(contains(&["[(0011)] ", "Negative"]));
    assert!(contains(&["c ", "Three"]));
    assert!(contains(&["d ", "Four"]));
    assert!(contains(&["VI ", "Six"]));
    assert!(contains(&["11: ", "Three"]));
}

#[tokio::test]
async fn supports_symbols_function_counter_styles() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ol { margin: 0 0 4pt 18pt; padding-left: 0 } .marker-content li::marker { content: counter(list-item, symbols(numeric \"0\" \"1\")) \": \" }</style>\
         <ol style=\"list-style-type: symbols(cyclic '*' '†')\"><li>One</li><li>Two</li><li>Three</li></ol>\
         <ol start=\"5\" class=\"marker-content\"><li>Five</li></ol>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    let contains = |needle: &[&str]| texts.windows(needle.len()).any(|pair| pair == needle);
    assert!(contains(&["*. ", "One"]));
    assert!(contains(&["†. ", "Two"]));
    assert!(contains(&["*. ", "Three"]));
    assert!(contains(&["101: ", "Five"]));
}

#[tokio::test]
async fn supports_named_counter_and_counters_marker_content() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 140pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt; counter-reset: chapter } .chapter { display: list-item; list-style-position: inside; counter-increment: chapter; margin: 0 } .chapter::marker { content: \"Chapter \" counter(chapter) \": \" } .outer { counter-reset: topic 2 } .inner { display: list-item; list-style-position: inside; counter-reset: topic 4; margin: 0 } .inner::marker { content: counters(topic, \".\") \" \" }</style><div class=\"chapter\">Intro</div><div class=\"chapter\">Methods</div><div class=\"outer\"><div class=\"inner\">Nested</div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    let contains = |needle: &[&str]| texts.windows(needle.len()).any(|pair| pair == needle);
    assert!(contains(&["Chapter 1: ", "Intro"]));
    assert!(contains(&["Chapter 2: ", "Methods"]));
    assert!(contains(&["2.4 ", "Nested"]));
}

#[tokio::test]
async fn generated_counter_content_renders_outside_marker() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: item } p { counter-increment: item } p::before { content: counter(item) \". \" }</style><p>Alpha</p><p>Beta</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["1.", "Alpha"]));
    assert!(texts.windows(2).any(|pair| pair == ["2.", "Beta"]));
}

#[tokio::test]
async fn generated_counter_names_preserve_custom_ident_case() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: Item } p { counter-increment: Item } p::before { content: counter(Item) }</style><p>Alpha</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["1", "Alpha"]));
}

#[tokio::test]
async fn generated_pseudo_counter_increment_applies_before_content() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: c } p::before { counter-increment: c; content: counter(c) }</style><p>Alpha</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["1", "Alpha"]));
}

#[tokio::test]
async fn empty_generated_pseudo_content_still_applies_counter_effects() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: c } p::before { content: \"\"; counter-increment: c } p::after { content: counter(c) }</style><p>Alpha</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("Alpha1"), "{text}");
}

#[tokio::test]
async fn non_generated_pseudo_content_does_not_apply_counter_effects() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: c } p::before { content: none; counter-increment: c } p::after { content: counter(c) }</style><p>Alpha</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("Alpha0"), "{text}");
}

#[tokio::test]
async fn generated_pseudo_counters_instantiate_across_following_list_items() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 180pt; margin: 10pt } * { padding: 0; margin: 0 } body, div, ol, li { font-size: 10pt; line-height: 12pt } .test { counter-increment: section; counter-reset: multilevel } ol[multilevel] > li::before { content: counter(section) \".\" counters(multilevel, \".\") \".\"; counter-increment: multilevel } ol[multilevel] { list-style: none !important; clear: both }</style><div class=\"test\"></div><ol multilevel=\"multilevel\"><li>1.1</li><li>1.2</li><li>1.3</li></ol><div class=\"test\"></div><ol multilevel=\"multilevel\"><li>2.1</li><li>2.2</li><li>2.3</li></ol>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    for expected in [
        "1.1.1.1", "1.2.1.2", "1.3.1.3", "2.1.2.1", "2.2.2.2", "2.3.2.3",
    ] {
        assert!(text.contains(expected), "{text}");
    }
}

#[tokio::test]
async fn counter_reset_without_ancestor_scope_is_visible_to_following_siblings() {
    let document = Html::from_string(
        "<style>@page { size: 320pt 160pt; margin: 10pt } body, div { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: chapter } .chapter { counter-increment: chapter; counter-reset: section } .chapter::before { content: counter(chapter) \". \" } .section { counter-increment: section } .section::before { content: counter(chapter) \".\" counter(section) \" \" }</style><div class=\"chapter\">One</div><div class=\"section\">A</div><div class=\"section\">B</div><div class=\"chapter\">Two</div><div class=\"section\">C</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("1.One"), "{text}");
    assert!(text.contains("1.1A"), "{text}");
    assert!(text.contains("1.2B"), "{text}");
    assert!(text.contains("2.Two"), "{text}");
    assert!(text.contains("2.1C"), "{text}");
}

#[tokio::test]
async fn counter_reset_that_shadows_ancestor_does_not_replace_ancestor_counter() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body, div, span { margin: 0; font-size: 10pt; line-height: 12pt } #test { counter-reset: c } #test span { counter-increment: c } #test span::before { content: counter(c, decimal-leading-zero) } .local { counter-reset: c 98 }</style><div id=\"test\"><span></span> <span></span> <span class=\"local\"></span> <span></span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("01 02 99 03"), "{text}");
}

#[tokio::test]
async fn generated_counter_content_uses_exotic_counter_styles() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body, div { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: chapter 99 section 0 } div { counter-increment: chapter section } div::before { content: counter(chapter, trad-chinese-informal) \" \" counters(section, \".\", ethiopic-numeric) \" \" }</style><div>Title</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|text| text.contains("一百")));
    assert!(texts.iter().any(|text| text.contains("፩")));
    assert!(texts.iter().any(|text| text.contains("Title")));
}

#[tokio::test]
async fn inline_generated_counters_use_inline_element_counter_scope() {
    let document = Html::from_string(
        "<style>@page { size: 320pt 120pt; margin: 10pt } body, p, div { margin: 0; font-size: 10pt; line-height: 12pt } #test { counter-reset: c } #test span { counter-increment: c } #test span::before { content: counter(c, decimal-leading-zero) }</style>\
         <p>The following two lines should look the same:</p>\
         <div id=\"test\">\
           <span></span> <span></span> <span></span> <span></span> <span></span> \
           <span></span> <span></span> <span></span> <span></span> <span></span> \
           <span></span> <span></span> <span style=\"counter-reset: c 98\"></span> \
           <span></span> <span></span>\
         </div>\
         <div>01 02 03 04 05 06 07 08 09 10 11 12 99 13 14</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    let expected = "01 02 03 04 05 06 07 08 09 10 11 12 99 13 14";
    let count = texts.iter().filter(|text| **text == expected).count();
    assert_eq!(
        count, 2,
        "generated and reference rows should match: {texts:?}"
    );
}

#[tokio::test]
async fn generated_attr_content_renders_and_transforms() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p::before { content: attr(data-label) \" \"; text-transform: uppercase }</style><p data-label=\"note\">Body</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.contains(&"NOTE"));
    assert!(texts.contains(&"Body"));
}

#[tokio::test]
async fn generated_image_content_renders_inline_atom() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 80pt; margin: 10pt }} body, p {{ margin: 0; font-size: 10pt; line-height: 12pt }} p::before {{ content: url({png}) \" \"; width: 8pt; height: 6pt }}</style><p>Icon</p>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images.len(), 1);
    let image = &document.pages[0].images[0];
    assert!((image.width - 8.0).abs() < 0.01);
    assert!((image.height - 6.0).abs() < 0.01);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Icon")
    );
}

#[tokio::test]
async fn invalid_generated_image_is_skipped_without_suppressing_text() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p::before { content: url(missing-generated-content-image.png) \"Fallback \" }</style><p>Body</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images.len(), 0);
    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["Fallback", "Body"]));
}

#[tokio::test]
async fn element_content_replacement_suppresses_children_and_pseudos() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 80pt; margin: 10pt }} body, p {{ margin: 0; font-size: 10pt; line-height: 12pt }} p {{ content: url({png}) / \"Replacement\"; width: 8pt; height: 6pt }} p::before {{ content: \"Before\" }} p::after {{ content: \"After\" }}</style><p>Hidden</p>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(
        document.pages[0].images[0].alt_text.as_deref(),
        Some("Replacement")
    );
    assert!(
        document.pages[0]
            .lines
            .iter()
            .all(|line| { !matches!(line.text.as_str(), "Hidden" | "Before" | "After") })
    );
}

#[tokio::test]
async fn element_content_none_preserves_children() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p { content: none }</style><p>Visible</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "Visible")
    );
}

#[tokio::test]
async fn element_content_contents_splices_children_once() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p { content: \"A\" contents \"B\" contents }</style><p>Text</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.contains(&"ATextB"), "{texts:?}");
}

#[tokio::test]
async fn q_elements_render_generated_quotes() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { quotes: \"«\" \"»\" \"“\" \"”\" }</style><p><q>Outer <q>Inner</q></q></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("«"));
    assert!(text.contains("»"));
    assert!(text.contains("“"));
    assert!(text.contains("”"));
}

#[tokio::test]
async fn no_quote_keywords_adjust_depth_without_text() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { quotes: \"[\" \"]\" \"{\" \"}\" } .skip::before { content: no-open-quote } .quoted::before { content: open-quote } .quoted::after { content: close-quote }</style><p><span class=\"skip\"></span><span class=\"quoted\">Deep</span></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("{"));
    assert!(text.contains("}"));
    assert!(!text.contains("[Deep]"));
}

#[tokio::test]
async fn auto_quotes_follow_document_language() {
    let document = Html::from_string(
        "<html lang=\"fr\"><style>@page { size: 220pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt }</style><p><q>Outer <q>Inner</q></q></p></html>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(text, "«Outer «Inner»»");
}

#[tokio::test]
async fn auto_quotes_follow_nested_language_override() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt }</style><p lang=\"fr\"><q>French <span lang=\"ja\"><q>Japanese</q></span></q></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(text, "«French 『Japanese』»");
}

#[tokio::test]
async fn auto_quotes_use_q_parent_language_not_q_language() {
    let document = Html::from_string(
        "<style>@page { size: 420pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt }</style><p>One <q>two <q lang=\"ja\">three <q lang=\"fr\">four</q></q></q></p><p>One “two ‘three 『four』’”</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .filter(|line| (line.y - document.pages[0].lines[0].y).abs() < 0.1)
        .map(|line| line.text.as_str())
        .collect::<String>();
    let reference = document.pages[0]
        .lines
        .iter()
        .find(|line| (line.y - document.pages[0].lines[0].y).abs() >= 0.1)
        .map(|line| line.text.as_str())
        .expect("expected literal reference line");
    assert_eq!(texts, "One “two ‘three 『four』’”");
    assert_eq!(reference, "One “two ‘three 『four』’”");
}

#[tokio::test]
async fn auto_quotes_cover_greek_and_farsi_nesting() {
    let document = Html::from_string(
        "<style>@page { size: 280pt 90pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt }</style><p lang=\"el\"><q>Greek <q>inner</q></q></p><p lang=\"fa\"><q>Farsi <q>inner</q></q></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    let text = text.join("");
    assert!(text.contains("“inner”"), "{text}");
    assert!(text.contains("‹inner›"), "{text}");
}

#[tokio::test]
async fn authored_quotes_and_quotes_none_override_language_auto() {
    let explicit = Html::from_string(
        "<style>@page { size: 220pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p { quotes: \"[\" \"]\" }</style><p lang=\"ja\"><q>Text</q></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let explicit_text = explicit.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(explicit_text.contains("["));
    assert!(explicit_text.contains("]"));
    assert!(!explicit_text.contains("「"));

    let none = Html::from_string(
        "<style>@page { size: 220pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p { quotes: none }</style><p lang=\"ja\"><q>Text</q></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let none_text = none.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(none_text, "Text");
}

#[tokio::test]
async fn leader_content_expands_between_inline_items() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p::after { content: leader(dotted) \"2\" }</style><p>Chapter</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("Chapter"));
    assert!(text.contains("2"));
    assert!(text.contains("..."), "{text}");
}

#[tokio::test]
async fn generated_image_alt_text_is_captured() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 80pt; margin: 10pt }} body, p {{ margin: 0; font-size: 10pt; line-height: 12pt }} p::before {{ content: url({png}) / \"Generated\"; width: 8pt; height: 6pt }}</style><p>Icon</p>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(
        document.pages[0].images[0].alt_text.as_deref(),
        Some("Generated")
    );
}

#[tokio::test]
async fn supports_string_list_style_type() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } p { display: list-item; list-style-position: inside; list-style-type: \"Note: \" }</style><p>Alpha</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "Note: ");
    assert_eq!(document.pages[0].lines[1].text.trim_start(), "Alpha");
    assert!((document.pages[0].lines[0].y - document.pages[0].lines[1].y).abs() < 0.01);
    assert!(document.pages[0].lines[0].x < document.pages[0].lines[1].x);
}

#[tokio::test]
async fn supports_string_list_style_type_in_shorthand() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body, ol, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ol, ul { list-style: inside \"# \"; }</style><ol><li>Alpha</li></ol>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    let first_visual_line = lines
        .iter()
        .take_while(|line| (line.y - lines[0].y).abs() < 0.01)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(
        first_visual_line.starts_with("# Alpha"),
        "{first_visual_line}"
    );
    assert!(!first_visual_line.starts_with("1."), "{first_visual_line}");
}

#[tokio::test]
async fn outside_list_markers_do_not_indent_wrapped_content_lines() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } ul { margin: 0; padding-left: 20pt } li { margin: 0 }</style><ul><li>Alpha beta gamma delta epsilon zeta eta theta</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert!(lines.len() > 1);
    assert_eq!(lines[0].text, "\u{2022}");
    assert!(lines[1].text.starts_with("Alpha"));
    assert!(!lines[2].text.starts_with("\u{2022}"));
    assert!((lines[1].x - lines[2].x).abs() < 0.01);
    assert!(lines[0].x < lines[1].x);
}

#[tokio::test]
async fn inside_list_markers_participate_in_first_line() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } ol { margin: 0; padding-left: 20pt; list-style-position: inside } li { margin: 0 }</style><ol><li>Alpha beta gamma delta epsilon zeta eta theta</li></ol>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert!(lines.len() > 1);
    assert_eq!(lines[0].text, "1.");
    assert!(lines[1].text.trim_start().starts_with("Alpha"));
    assert!((lines[0].y - lines[1].y).abs() < 0.01);
    assert!(lines[0].x < lines[1].x);
    assert!(!lines[2].text.starts_with("1."));
}

#[tokio::test]
async fn rtl_outside_list_markers_paint_on_inline_start_right_side() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ul { margin: 0; padding: 0; direction: rtl } li { margin: 0 }</style><ul><li>Alpha</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let marker = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "\u{2022}")
        .expect("expected marker");
    let content = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Alpha")
        .expect("expected content");
    assert!(marker.x > content.x + rendered_line_advance(content));
}

#[tokio::test]
async fn html_dir_rtl_outside_list_markers_match_css_direction_rtl() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ul { margin: 0; padding: 0 } li { margin: 0 }</style><ul dir=\"rtl\"><li>Alpha</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let marker = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "\u{2022}")
        .expect("expected marker");
    let content = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Alpha")
        .expect("expected content");
    assert!(marker.x > content.x + rendered_line_advance(content));
}

#[tokio::test]
async fn rtl_outside_image_markers_paint_on_inline_start_right_side() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 100pt; margin: 10pt }} body {{ margin: 0; font-size: 10pt; line-height: 12pt }} ul {{ margin: 0; padding: 0; direction: rtl; list-style-image: url({png}) }} li {{ margin: 0 }}</style><ul><li>Alpha</li></ul>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let image = document.pages[0]
        .images
        .first()
        .expect("expected marker image");
    let content = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Alpha")
        .expect("expected content");
    assert!(image.x > content.x + rendered_line_advance(content));
}

#[tokio::test]
async fn rtl_outside_list_marker_only_paints_on_first_wrapped_line() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } ul { margin: 0; padding: 0; direction: rtl } li { margin: 0; width: 70pt }</style><ul><li>Alpha beta gamma delta epsilon zeta eta theta</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let markers = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text == "\u{2022}")
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 1);
    let content_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text != "\u{2022}")
        .collect::<Vec<_>>();
    assert!(content_lines.len() > 1);
    let first_right = content_lines[0].x + rendered_line_advance(content_lines[0]);
    let second_right = content_lines[1].x + rendered_line_advance(content_lines[1]);
    assert!((first_right - second_right).abs() < 0.01);
    assert!(markers[0].x > content_lines[0].x + rendered_line_advance(content_lines[0]));
}

#[tokio::test]
async fn rtl_inside_list_markers_start_first_line_before_generated_before() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ol { margin: 0; padding: 0; direction: rtl; list-style-position: inside } li { margin: 0 } li::before { content: \"Before\" }</style><ol><li>Alpha</li></ol>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains('1'));
    assert!(text.contains("Before"));
    assert!(text.find('1').unwrap() < text.find("Before").unwrap());
}

#[tokio::test]
async fn marker_side_controls_mixed_direction_outside_marker_side() {
    let match_self = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, ul, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ul { direction: ltr } li { list-style-type: decimal } .rtl { direction: rtl }</style><ul><li>Left</li><li class=\"rtl\">Right</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let left_marker = match_self.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "1.")
        .expect("expected first marker");
    let left_content = match_self.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Left")
        .expect("expected first content");
    let right_marker = match_self.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "2.")
        .expect("expected second marker");
    let right_content = match_self.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Right")
        .expect("expected second content");
    assert!(left_marker.x < left_content.x);
    assert!(right_marker.x > right_content.x + rendered_line_advance(right_content));

    let match_parent = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, ul, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ul { direction: ltr; marker-side: match-parent } li { list-style-type: decimal } .rtl { direction: rtl }</style><ul><li>Left</li><li class=\"rtl\">Right</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let second_marker = match_parent.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "2.")
        .expect("expected second marker");
    let second_content = match_parent.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Right")
        .expect("expected second content");
    assert!(second_marker.x < second_content.x);
}

#[tokio::test]
async fn capitalize_tailors_dutch_ij_digraphs() {
    let document = Html::from_string(
        "<p lang=\"nl\" style=\"text-transform: capitalize; margin: 0\">ijsland ijssel</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "IJsland IJssel")
    );
}

#[tokio::test]
async fn text_justify_inter_character_distributes_between_letters() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 } p { margin: 0; width: 100pt; font-size: 10pt; line-height: 12pt; text-align: justify; text-align-last: justify; text-justify: inter-character }</style><p>XX</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let x_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text == "X")
        .collect::<Vec<_>>();
    assert_eq!(x_lines.len(), 2);
    assert!(x_lines[1].x - x_lines[0].x > rendered_line_advance(x_lines[0]) + 50.0);
}

#[tokio::test]
async fn supports_display_list_item_on_arbitrary_elements() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } div { display: list-item; margin-left: 20pt; list-style-type: decimal }</style><div>One</div><div>Two</div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "1.");
    assert_eq!(document.pages[0].lines[1].text, "One");
    assert_eq!(document.pages[0].lines[2].text, "2.");
    assert_eq!(document.pages[0].lines[3].text, "Two");
}

#[tokio::test]
async fn supports_inline_display_list_item_markers() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 90pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } span { display: inline list-item; list-style-position: inside; list-style-type: decimal }</style><span>Inline one</span><span>Inline two</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[0].text, "1.");
    assert_eq!(lines[1].text.trim_start(), "Inline one");
    assert!((lines[0].y - lines[1].y).abs() < 0.01);
    assert!(lines[0].x < lines[1].x);
    assert_eq!(lines[2].text, "2.");
    assert_eq!(lines[3].text.trim_start(), "Inline two");
    assert!((lines[2].y - lines[3].y).abs() < 0.01);
    assert!(lines[2].x < lines[3].x);
}

#[tokio::test]
async fn supports_html_ordered_list_start_reversed_and_value() {
    let document = Html::from_string(
        "<ol start=\"3\"><li>Three</li><li value=\"7\">Seven</li><li>Eight</li></ol><ol reversed><li>Two</li><li>One</li></ol>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["3.", "Three"]));
    assert!(texts.windows(2).any(|pair| pair == ["7.", "Seven"]));
    assert!(texts.windows(2).any(|pair| pair == ["8.", "Eight"]));
    assert!(texts.windows(2).any(|pair| pair == ["2.", "Two"]));
    assert!(texts.windows(2).any(|pair| pair == ["1.", "One"]));
}

#[tokio::test]
async fn nested_list_item_counters_use_unified_counter_stack() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 140pt; margin: 10pt } body, ol { margin: 0; font-size: 10pt; line-height: 12pt } ol { padding-left: 18pt; list-style-position: inside } li::marker { content: counters(list-item, \".\") \". \" }</style><ol><li>One<ol><li>Inner</li></ol></li><li>Two</li></ol>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        texts.windows(2).any(|pair| pair == ["1. ", "One"]),
        "{texts:?}"
    );
    assert!(
        texts.windows(2).any(|pair| pair == ["1.1. ", "Inner"]),
        "{texts:?}"
    );
    assert!(
        texts.windows(2).any(|pair| pair == ["2. ", "Two"]),
        "{texts:?}"
    );
}

#[tokio::test]
async fn supports_list_item_counter_increment_reset_and_set() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } .evens li { counter-increment: list-item 2 } .reset { counter-reset: list-item 4 } .set li:first-child { counter-set: list-item 9 } .none li { counter-increment: none }</style><ol class=\"evens\"><li>Two</li><li>Four</li></ol><ol class=\"reset\"><li>Five</li></ol><ol class=\"set\"><li>Nine</li><li>Ten</li></ol><ol class=\"none\"><li>One</li><li>Two</li></ol>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["2.", "Two"]));
    assert!(texts.windows(2).any(|pair| pair == ["4.", "Four"]));
    assert!(texts.windows(2).any(|pair| pair == ["5.", "Five"]));
    assert!(texts.windows(2).any(|pair| pair == ["9.", "Nine"]));
    assert!(texts.windows(2).any(|pair| pair == ["10.", "Ten"]));
    assert!(texts.windows(2).any(|pair| pair == ["1.", "One"]));
    assert!(texts.windows(2).any(|pair| pair == ["2.", "Two"]));
}

#[tokio::test]
async fn marker_pseudo_element_styles_marker_without_affecting_content() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } li::marker { color: red; font-size: 14pt }</style><ul><li>Item</li></ul>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let marker = &document.pages[0].lines[0];
    let content = &document.pages[0].lines[1];
    assert_eq!(marker.text, "\u{2022}");
    assert_eq!(content.text, "Item");
    assert!((marker.font_size - 14.0).abs() < 0.01);
    assert!((content.font_size - 10.0).abs() < 0.01);
    assert_ne!(marker.color, content.color);
}

#[tokio::test]
async fn marker_pseudo_element_content_overrides_automatic_marker() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ol { list-style-position: inside } li::marker { content: counter(list-item, lower-alpha) \") \" }</style><ol><li>One</li><li>Two</li></ol>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[0].text, "a) ");
    assert_eq!(lines[1].text, "One");
    assert!((lines[0].y - lines[1].y).abs() < 0.01);
    assert!(lines[0].x < lines[1].x);
    assert_eq!(lines[2].text, "b) ");
    assert_eq!(lines[3].text, "Two");
    assert!((lines[2].y - lines[3].y).abs() < 0.01);
    assert!(lines[2].x < lines[3].x);
}

#[tokio::test]
async fn supports_outside_list_style_image_markers() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 120pt 80pt; margin: 10pt }} body {{ margin: 0; font-size: 10pt; line-height: 12pt }} ul {{ margin: 0; padding-left: 18pt; list-style-image: url({png}); list-style-type: decimal }} li {{ margin: 0 }}</style><ul><li>Item</li></ul>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images.len(), 1);
    assert!(document.pages[0].lines.iter().all(|line| line.text != "1."));
    let image = &document.pages[0].images[0];
    let item = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim_start() == "Item")
        .unwrap();
    assert!(image.x < item.x);
}

#[tokio::test]
async fn supports_inside_list_style_image_markers() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 120pt 80pt; margin: 10pt }} body {{ margin: 0; font-size: 10pt; line-height: 12pt }} ul {{ margin: 0; padding-left: 0; list-style-position: inside; list-style-image: url({png}) }} li {{ margin: 0 }}</style><ul><li>Item</li></ul>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images.len(), 1);
    let image = &document.pages[0].images[0];
    let item = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim_start() == "Item")
        .unwrap();
    assert!(image.x < item.x);
    assert!((image.y - item.y).abs() < 20.0);
}

#[tokio::test]
async fn marker_content_overrides_list_style_image() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 120pt 80pt; margin: 10pt }} body {{ margin: 0; font-size: 10pt; line-height: 12pt }} ul {{ margin: 0; padding-left: 18pt; list-style-image: url({png}) }} li {{ margin: 0 }} li::marker {{ content: \"x \" }}</style><ul><li>Item</li></ul>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images.len(), 0);
    assert_eq!(document.pages[0].lines[0].text, "x ");
    assert_eq!(document.pages[0].lines[1].text, "Item");
}

#[tokio::test]
async fn supports_text_alignment() {
    let options = RenderOptions::default();
    let document = Html::from_string(
        "<style>body { margin: 0 }</style><p style=\"margin: 0; width: 100pt; text-align: right; font-size: 10pt\">Hi</p>",
    )
    .render_async(&options).await
    .unwrap();

    let aligned_offset = document.pages[0].lines[0].x - options.page_margins().left;
    let expected_offset = 100.0 - rendered_line_advance(&document.pages[0].lines[0]);
    assert!(
        (aligned_offset - expected_offset).abs() < 0.01,
        "expected right-aligned offset {expected_offset}, got {aligned_offset}"
    );
}

#[tokio::test]
async fn supports_text_align_start_from_rtl_dir_attribute() {
    let options = RenderOptions::default();
    let document = Html::from_string(
        "<style>body { margin: 0 }</style><p dir=\"rtl\" style=\"margin: 0; width: 100pt; text-align: start; font-size: 10pt\">Hi</p>",
    )
    .render_async(&options).await
    .unwrap();

    let aligned_offset = document.pages[0].lines[0].x - options.page_margins().left;
    let expected_offset = 100.0 - rendered_line_advance(&document.pages[0].lines[0]);
    assert!(
        (aligned_offset - expected_offset).abs() < 0.01,
        "expected rtl start-aligned offset {expected_offset}, got {aligned_offset}"
    );
}

#[tokio::test]
async fn preserved_tabs_use_computed_tab_size() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body, p { margin: 0; font: 10pt monospace; line-height: 12pt; white-space: pre } .two { tab-size: 2 } .len { tab-size: 18pt } .four { tab-size: 4 }</style><p class=\"two\">A\tB</p><p class=\"len\">A\tB</p><p class=\"four\">A\tB</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[0].text, "A\tB");
    assert_eq!(lines[1].text, "A\tB");
    assert_eq!(lines[2].text, "A\tB");

    let two = rendered_line_advance(&lines[0]);
    let length = rendered_line_advance(&lines[1]);
    let four = rendered_line_advance(&lines[2]);
    assert!(
        two < length && length < four,
        "expected numeric and length tab stops to produce ordered widths, got {two}, {length}, {four}"
    );
}

#[tokio::test]
async fn inline_block_auto_width_uses_graph_tab_size_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body, div { margin: 0 } \
         span { display: inline-block; white-space: pre; font: 10pt/12pt monospace } \
         .two { tab-size: 2; background: rgb(10, 20, 30) } \
         .four { tab-size: 4; background: rgb(30, 20, 10) }</style>\
         <div><span class=\"two\">A\tB</span></div><div><span class=\"four\">A\tB</span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let two = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(10, 20, 30)))
        .expect("expected tab-size:2 inline-block background");
    let four = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(30, 20, 10)))
        .expect("expected tab-size:4 inline-block background");

    assert!(
        four.width > two.width + 5.0,
        "auto inline-block max-content width should use graph tab advances, got {two:?} and {four:?}"
    );
}

#[tokio::test]
async fn hanging_punctuation_last_excludes_rtl_closing_punctuation_from_alignment() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } \
         body { margin: 0; direction: rtl; font-family: monospace; font-size: 10pt; line-height: 12pt } \
         div { margin: 0; white-space: nowrap; text-align: start } \
         .hang { hanging-punctuation: last; width: 4ch } \
         .ref { width: 4ch }</style>\
         <div class=\"hang\">MMMM)</div><div class=\"ref\">MMMM</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let hang = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == ")MMMM")
        .unwrap();
    let reference = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "MMMM")
        .unwrap();
    let hang_measured_x = hang.x + rendered_line_advance(hang) - rendered_line_advance(reference);
    assert!(
        (hang_measured_x - reference.x).abs() < 0.5,
        "expected hanging line and same-width reference to share measured alignment, got {} vs {}",
        hang_measured_x,
        reference.x
    );
}

#[tokio::test]
async fn hanging_punctuation_first_places_opening_quote_outside_line_measure() {
    let normal = Html::from_string(
        "<style>body{margin:0}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt}</style>\
         <p>\"Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let hanging = Html::from_string(
        "<style>body{margin:0}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;hanging-punctuation:first}</style>\
         <p>\"Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        hanging.pages[0].lines[0].x < normal.pages[0].lines[0].x - 1.0,
        "expected first hanging quote to move into the line-start margin"
    );
}

#[tokio::test]
async fn hanging_punctuation_first_places_rtl_opening_punctuation_outside_line_measure() {
    let normal = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt}</style>\
         <p>(Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let hanging = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;hanging-punctuation:first}</style>\
         <p>(Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let blocked = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;hanging-punctuation:first}span{border-right:1em solid blue}</style>\
         <p><span>(</span>Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let blocked_reference = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt}span{border-right:1em solid blue}</style>\
         <p><span>(</span>Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        hanging.pages[0].lines[0].x > normal.pages[0].lines[0].x + 1.0,
        "expected RTL first hanging punctuation to move into the line-start margin: normal={}, hanging={}",
        normal.pages[0].lines[0].x,
        hanging.pages[0].lines[0].x,
    );
    assert!(
        (blocked.pages[0].lines[0].x - blocked_reference.pages[0].lines[0].x).abs() < 1.0,
        "expected nonzero RTL inline-start border to block first hanging punctuation: reference={}, blocked={}, blocked text={:?}",
        blocked_reference.pages[0].lines[0].x,
        blocked.pages[0].lines[0].x,
        blocked.pages[0]
            .lines
            .iter()
            .map(|line| (line.text.as_str(), line.x))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn text_indent_offsets_rtl_inline_start_edge() {
    let normal = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt}</style>\
         <p>Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let negative_indent = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;text-indent:-1em}</style>\
         <p>Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let positive_indent = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;text-indent:1em}</style>\
         <p>Hello</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        negative_indent.pages[0].lines[0].x > normal.pages[0].lines[0].x + 1.0,
        "expected negative RTL text-indent to move the inline-start edge outward: normal={}, indented={}",
        normal.pages[0].lines[0].x,
        negative_indent.pages[0].lines[0].x,
    );
    assert!(
        positive_indent.pages[0].lines[0].x < normal.pages[0].lines[0].x - 1.0,
        "expected positive RTL text-indent to move the inline-start edge inward: normal={}, indented={}",
        normal.pages[0].lines[0].x,
        positive_indent.pages[0].lines[0].x,
    );
}

#[tokio::test]
async fn fixed_width_rtl_block_ignores_overconstrained_left_margin() {
    let both_margins = Html::from_string(
        "<style>body{margin:0;direction:rtl;font-family:monospace;font-size:10pt;line-height:12pt}div{width:100pt;margin:10pt}</style>\
         <div>Hello</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let right_margin = Html::from_string(
        "<style>body{margin:0;direction:rtl;font-family:monospace;font-size:10pt;line-height:12pt}div{width:100pt;margin-right:10pt}</style>\
         <div>Hello</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        (both_margins.pages[0].lines[0].x - right_margin.pages[0].lines[0].x).abs() < 1.0,
        "expected fixed-width RTL block to ignore over-constrained left margin: both={}, right={}",
        both_margins.pages[0].lines[0].x,
        right_margin.pages[0].lines[0].x,
    );
}

#[tokio::test]
async fn hanging_punctuation_force_end_excludes_terminal_stop_from_alignment() {
    let normal = Html::from_string(
        "<style>body{margin:0}p{margin:0;width:120pt;text-align:right;font-family:monospace;font-size:10pt;line-height:12pt}</style>\
         <p>Hello。</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let hanging = Html::from_string(
        "<style>body{margin:0}p{margin:0;width:120pt;text-align:right;font-family:monospace;font-size:10pt;line-height:12pt;hanging-punctuation:force-end}</style>\
         <p>Hello。</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        hanging.pages[0].lines[0].x > normal.pages[0].lines[0].x + 1.0,
        "expected force-end punctuation to hang past the right line edge: normal={}, hanging={}",
        normal.pages[0].lines[0].x,
        hanging.pages[0].lines[0].x,
    );
}

#[tokio::test]
async fn hanging_punctuation_last_is_blocked_by_inline_end_border() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } \
         body { margin: 0; direction: rtl; font-family: monospace; font-size: 10pt; line-height: 12pt } \
         div { margin: 0; white-space: nowrap; text-align: start; width: 4ch } \
         .blocked { hanging-punctuation: last } \
         .blocked span { border-left: 1em solid blue }</style>\
         <div class=\"blocked\">MMMM<span>)</span></div><div>MMMM)</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text == ")MMMM")
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    let border = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("expected inline-end border");
    assert!(
        (border.x - lines[1].x).abs() < 0.5,
        "expected inline-end border to occupy the ordinary rtl edge, got border {} vs reference {}",
        border.x,
        lines[1].x
    );
    assert!(
        lines[0].x >= border.x + border.width - 0.5,
        "expected blocked punctuation text to start after its inline-end border, got text {} border {}..{}",
        lines[0].x,
        border.x,
        border.x + border.width
    );
}

#[tokio::test]
async fn hanging_punctuation_allow_end_ignores_empty_inline_but_respects_bordered_inline() {
    let allowed = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } \
         body { margin: 0; font-family: monospace; font-size: 10pt; line-height: 12pt } \
         div { margin: 0; width: 5ch; hanging-punctuation: allow-end }</style>\
         <div>12 34,<span></span> 1234,</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let blocked = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } \
         body { margin: 0; font-family: monospace; font-size: 10pt; line-height: 12pt } \
         div { margin: 0; width: 5ch; hanging-punctuation: allow-end } \
         span { border-left: 1em solid black }</style>\
         <div>12 34,<span></span> 1234,</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(allowed.pages[0].lines[0].text, "12 34,");
    assert_eq!(blocked.pages[0].lines[0].text, "12");
}

#[tokio::test]
async fn wpt_hanging_punctuation_allow_end_inline_boundaries() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 500pt 500pt; margin: 0 }} body {{ margin: 0; font-family: Ahem; color: green }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .hang {{ hanging-punctuation: allow-end; margin: 1em; width: 5em; border: 2px solid black }}</style>\
         <div style=\"float:left; width:auto\" class=\"hang\">12 34,</div>\
         <div style=\"clear:both\">\
         <div class=\"hang\">12 34,<span></span> 1234,</div>\
         <div class=\"hang\">12 34,<span style=\"border-left:1em solid black\"></span> 1234,</div>\
         <div class=\"hang\">12 34,<span style=\"border-right:1em solid black\"></span> 1234,</div>\
         <div class=\"hang\"><span style=\"border-right:1em solid black\">12 34,</span> 2345</div>\
         <div class=\"hang\">12 34<span style=\"border-left:1em solid black\">,</span> 2345</div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut rendered_lines = document.pages[0].lines.clone();
    rendered_lines.sort_by(|left, right| {
        right
            .y
            .total_cmp(&left.y)
            .then_with(|| left.x.total_cmp(&right.x))
    });
    let mut lines: Vec<(f32, String)> = Vec::new();
    for line in &rendered_lines {
        if let Some((baseline, text)) = lines.last_mut()
            && (line.y - *baseline).abs() < 0.01
        {
            text.push_str(&line.text);
            continue;
        }
        lines.push((line.y, line.text.clone()));
    }
    let lines = lines
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        lines,
        vec![
            "12 34,", "12 34,", "1234,", "12", "34,", "1234,", "12", "34,", "1234,", "12", "34,",
            "2345", "12", "34,", "2345"
        ]
    );
}

#[tokio::test]
async fn wpt_hanging_punctuation_first_and_last_match_negative_margin_reference() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let target = Html::from_string(format!(
        "<style>@page {{ size: 500pt 300pt; margin: 0 }} body {{ margin: 0; font-family: Ahem; color: green }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .hang {{ hanging-punctuation: first last; margin: 1em; border: 1px solid black }}</style>\
         <div style=\"float:left\" class=\"hang\">(Hang test)</div>\
         <div style=\"clear:both\"><div class=\"hang\" style=\"width:10em; text-align:justify;\">(This should hang.<br>(This should also hang.)</div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "<style>@page {{ size: 500pt 300pt; margin: 0 }} body {{ margin: 0; font-family: Ahem; color: green }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .hang {{ margin: 1em; border: 1px solid black }}</style>\
         <div style=\"float:left\" class=\"hang\"><div style=\"margin: 0 -1em\">(Hang test)</div></div>\
         <div style=\"clear:both\"><div class=\"hang\" style=\"width:10em; text-align:justify;\"><span style=\"margin:0 -1em\">(This should hang.<br>(This should also hang.)</span></div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let target_lines = target.pages[0]
        .lines
        .iter()
        .map(|line| (line.text.as_str(), line.x, rendered_line_advance(line)))
        .collect::<Vec<_>>();
    let reference_lines = reference.pages[0]
        .lines
        .iter()
        .map(|line| (line.text.as_str(), line.x, rendered_line_advance(line)))
        .collect::<Vec<_>>();
    assert_eq!(
        target_lines
            .iter()
            .map(|(text, _, _)| *text)
            .collect::<Vec<_>>(),
        reference_lines
            .iter()
            .map(|(text, _, _)| *text)
            .collect::<Vec<_>>()
    );
    for ((text, target_x, target_width), (_, reference_x, reference_width)) in
        target_lines.iter().zip(reference_lines.iter())
    {
        assert!(
            (target_x - reference_x).abs() < 1.0,
            "{text}: target x {target_x}, reference x {reference_x}"
        );
        assert!(
            (target_width - reference_width).abs() < 1.0,
            "{text}: target width {target_width}, reference width {reference_width}"
        );
    }
}

#[tokio::test]
async fn wpt_hanging_punctuation_inline_background_includes_hung_stop() {
    let document = Html::from_string(
        "<style>@page { size: 400pt 400pt; margin: 0 } body { margin: 0 }\
         div { font-family: monospace; font-size: 60px; line-height: 1.5em; hanging-punctuation: allow-end; width: 245px }\
         span { background-color: lime; border: black solid 3px }</style>\
         <div><span>まだよくています。しかし特</span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["まだよく", "ています。", "しかし特"]);

    let green_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 255, 0)))
        .collect::<Vec<_>>();
    assert_eq!(green_rects.len(), 3, "{green_rects:?}");
    assert!(
        green_rects[1].width > green_rects[0].width,
        "second line background should include the hung punctuation: {green_rects:?}"
    );
}

#[tokio::test]
async fn wpt_hanging_punctuation_uses_punctuation_font_size() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let target = Html::from_string(format!(
        "<style>@page {{ size: 500pt 200pt; margin: 0 }} body {{ margin: 0; font-family: Ahem; color: green }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .hang {{ hanging-punctuation: first last; margin: 1em; float: left }}</style>\
         <div class=\"hang\" style=\"font-size:32px\"><span style=\"font-size:16px\">(</span>1234</div>\
         <div class=\"hang\" style=\"font-size:32px\">1234<span style=\"font-size:16px\">)</span></div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "<style>@page {{ size: 500pt 200pt; margin: 0 }} body {{ margin: 0; font-family: Ahem; color: green }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .hang {{ white-space: nowrap; margin: 1em; float: left }}</style>\
         <div class=\"hang\" style=\"font-size:32px\"><span style=\"font-size:16px; margin-left:-1em\">(</span>1234</div>\
         <div class=\"hang\" style=\"font-size:32px\">1234<span style=\"font-size:16px; margin-right:-1em\">)</span></div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    for (target_line, reference_line) in target.pages[0].lines.iter().zip(&reference.pages[0].lines)
    {
        assert_eq!(target_line.text, reference_line.text);
        assert!(
            (target_line.x - reference_line.x).abs() < 1.0,
            "{} target x {}, reference x {}",
            target_line.text,
            target_line.x,
            reference_line.x
        );
        assert!(
            (rendered_line_advance(target_line) - rendered_line_advance(reference_line)).abs()
                < 1.0,
            "{} target width {}, reference width {}",
            target_line.text,
            rendered_line_advance(target_line),
            rendered_line_advance(reference_line)
        );
    }
}

#[tokio::test]
async fn supports_text_align_justify() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } p { margin: 0; width: 45pt; text-align: justify; font-size: 10pt; line-height: 10pt }</style><p>one two three</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let first = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "one two")
        .unwrap();
    let three = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "three")
        .unwrap();

    assert!(rendered_line_advance(first) > 40.0);
    assert!((three.x - first.x).abs() < 0.5);
    assert!(three.y < first.y);
}

#[tokio::test]
async fn text_justify_none_disables_inter_word_distribution() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 360pt 160pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ margin: 0; width: 290px; color: orange; font: 24px/24px Ahem; text-align: justify; text-justify: none }}</style>\
         <div class=\"test\">XXX XXX XXX XXX XXX XXX XXX XXX</div>",
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let space_advances = document.pages[0]
        .lines
        .iter()
        .flat_map(|line| line.runs.iter())
        .flat_map(|run| run.glyphs.as_deref().unwrap_or(&[]))
        .filter(|glyph| glyph.unicode == " ")
        .map(|glyph| glyph.x_advance)
        .collect::<Vec<_>>();
    assert!(
        space_advances
            .iter()
            .all(|advance| (*advance - 18.0).abs() < 1.0),
        "text-justify:none must keep normal Ahem word spaces, got {space_advances:?}"
    );
}

#[tokio::test]
async fn text_justify_ignores_pre_wrap_trailing_space_ltr() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 220pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ width: 7em; font: 15pt/15pt Ahem; white-space: pre-wrap; text-align: justify }}</style>\
         <div class=\"test\"><span>XX XX </span><span>XXX</span></div>",
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let mut groups = lines_grouped_by_y(&document.pages[0].lines);
    groups.sort_by(|left, right| right[0].y.total_cmp(&left[0].y));
    assert_eq!(groups.len(), 2, "{groups:?}");
    assert_eq!(
        groups[0]
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["XX XX"]
    );
    assert_eq!(groups[1].len(), 1);

    let first_span = rendered_fragment_group_span(&groups[0]);
    assert!(
        (first_span - 105.0).abs() < 1.0,
        "first justified line should fill 7em after hanging trailing space, got {first_span}"
    );
    assert_eq!(groups[1][0].text, "XXX");
}

#[tokio::test]
async fn text_justify_ignores_pre_wrap_trailing_space_rtl() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 220pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ width: 7em; font: 15pt/15pt Ahem; white-space: pre-wrap; text-align: justify; direction: rtl }}</style>\
         <div class=\"test\"><span>XX XX </span><span>XXX</span></div>",
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let mut groups = lines_grouped_by_y(&document.pages[0].lines);
    groups.sort_by(|left, right| right[0].y.total_cmp(&left[0].y));
    assert_eq!(groups.len(), 2, "{groups:?}");
    assert_eq!(
        groups[0]
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["XX XX"]
    );
    assert_eq!(groups[1].len(), 1);

    let first_span = rendered_fragment_group_span(&groups[0]);
    assert!(
        (first_span - 105.0).abs() < 1.0,
        "first RTL justified line should fill 7em after hanging trailing space, got {first_span}"
    );
    assert_eq!(groups[1][0].text, "XXX");
}

#[tokio::test]
async fn wpt_text_justify_hangs_pre_wrap_trailing_space_inside_split_fragment() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 220pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ width: 7em; font: 15pt/15pt Ahem; white-space: pre-wrap; text-align: justify }}</style>\
         <div class=\"test\"><span>XX XX XXX</span></div>",
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let mut groups = lines_grouped_by_y(&document.pages[0].lines);
    groups.sort_by(|left, right| right[0].y.total_cmp(&left[0].y));
    assert_eq!(groups.len(), 2, "{groups:?}");
    let first_span = rendered_fragment_group_span(&groups[0]);
    assert!(
        (first_span - 105.0).abs() < 1.0,
        "first split-fragment line should fill 7em after hanging trailing space, got {first_span}; {groups:?}"
    );
    assert_eq!(groups[1][0].text, "XXX");
}

#[tokio::test]
async fn wpt_text_justify_inter_word_expands_unicode_word_separators() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let separators = [
        " ",
        "\u{00a0}",
        "\u{1361}",
        "\u{10100}",
        "\u{10101}",
        "\u{1039f}",
        "\u{1091f}",
    ];
    let rows = separators
        .iter()
        .map(|separator| {
            format!(
                "<div class=\"justified\">XXXX<span class=\"hidden\">{separator}</span>XXXX XXXX</div>"
            )
        })
        .collect::<String>();
    let document = Html::from_string(format!(
        "<style>@page {{ size: 220pt 260pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .justified {{ width: 120pt; font: 10pt/10pt Ahem; text-align: justify; text-justify: inter-word }}\
         .hidden {{ color: transparent }}</style>{rows}",
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let mut groups = visual_line_groups(&document.pages[0].lines);
    groups.sort_by(|left, right| right[0].y.total_cmp(&left[0].y));
    assert_eq!(groups.len(), separators.len() * 2, "{groups:?}");
    for (index, first_line) in groups.iter().step_by(2).enumerate() {
        let span = rendered_fragment_group_span(first_line);
        assert!(
            (span - 120.0).abs() < 1.0,
            "word separator {index} should justify the wrapped first line to 120pt, got {span}; {first_line:?}"
        );
    }
    let hidden_separators = separators.iter().skip(2).collect::<Vec<_>>();
    assert!(
        document.pages[0].lines.iter().all(|line| hidden_separators
            .iter()
            .all(|separator| !line.text.contains(*separator))),
        "transparent word separators should not emit visible text lines: {:?}",
        document.pages[0]
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn local_wpt_text_justify_word_separators_hide_separator_glyphs_if_available() {
    let wpt_root = std::path::Path::new("/Users/lee/oss/quire-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }
    let html = std::fs::read_to_string(
        wpt_root.join("css/css-text/text-justify/text-justify-word-separators.html"),
    )
    .unwrap();
    let document = Html::from_string(html)
        .with_base_url(wpt_root)
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let hidden_separators = [
        "\u{1361}",
        "\u{10100}",
        "\u{10101}",
        "\u{1039f}",
        "\u{1091f}",
    ];
    assert!(
        document.pages[0].lines.iter().all(|line| hidden_separators
            .iter()
            .all(|separator| !line.text.contains(separator))),
        "transparent WPT word separators should not emit visible text lines: {:?}",
        document.pages[0]
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn supports_text_align_last_justify_over_center() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } p { margin: 0; width: 45pt; text-align: center; text-align-last: justify; font-size: 10pt; line-height: 10pt }</style><p>aa aa aa aa aa</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let mut line_groups = visual_line_groups(&document.pages[0].lines);
    line_groups.sort_by(|left, right| right[0].y.total_cmp(&left[0].y));
    assert_eq!(line_groups.len(), 2);
    let first_line = &line_groups[0];
    let last_line = &line_groups[1];
    assert_eq!(first_line.len(), 1);
    assert_eq!(last_line.len(), 1);

    let first_offset = first_line[0].x - 10.0;
    let last_span = rendered_fragment_group_span(last_line);

    assert!(
        first_offset > 2.0,
        "expected ordinary line to remain centered, got offset {first_offset}"
    );
    assert!(
        (last_span - 45.0).abs() < 1.0,
        "expected last-line justification span {last_span} to fill the 45pt line box"
    );
}

#[tokio::test]
async fn supports_text_align_justify_all_on_final_line() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } p { margin: 0; width: 45pt; text-align: justify-all; font-size: 10pt; line-height: 10pt }</style><p>aa aa aa aa aa</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let line_groups = visual_line_groups(&document.pages[0].lines);
    assert_eq!(line_groups.len(), 2);

    let first_span = rendered_fragment_group_span(&line_groups[0]);
    let last_span = rendered_fragment_group_span(&line_groups[1]);

    assert!(
        first_span > 40.0,
        "expected ordinary-line justification span {first_span} to approach the 45pt line box"
    );
    assert!(
        last_span > 40.0 && (last_span - first_span).abs() < 3.0,
        "expected justify-all final-line span {last_span} to match ordinary justified span {first_span}"
    );
}

#[tokio::test]
async fn wpt_text_align_end_rtl_aligns_to_physical_left() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let target = Html::from_string(format!(
        "<style>@page {{ size: 500pt 200pt; margin: 0 }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test, .ref {{ border: 1px solid orange; margin: 20px; width: 300px; color: orange; font: 25px/1 Ahem }}\
         .test {{ text-align: end; direction: rtl; }} .ref {{ text-align: left; }}</style>\
         <div class=\"test\">TESTI</div><div class=\"ref\">REFER</div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let test = target.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "TESTI")
        .unwrap();
    let reference = target.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "REFER")
        .unwrap();

    let (test_left, _) = rendered_line_visual_bounds(test);
    let (reference_left, _) = rendered_line_visual_bounds(reference);
    assert!(
        (test_left - reference_left).abs() < 1.0,
        "text-align:end in RTL should match physical left: test={test_left}, reference={reference_left}"
    );
}

#[tokio::test]
async fn wpt_text_align_justify_rtl_uses_start_on_last_line() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 700pt 300pt; margin: 0 }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ text-align: justify; direction: rtl }}\
         .test {{ border: 1px solid orange; margin: 20px; width: 450px; color: orange; font: 25px/1 Ahem }}</style>\
         <div class=\"test\">TES TES TES TES TES TES TES TES TES TES TES </div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut groups = visual_line_groups(&document.pages[0].lines);
    groups.retain(|group| group.iter().any(|line| line.text.contains("TES")));
    assert_eq!(groups.len(), 3, "{groups:?}");
    assert!(
        rendered_fragment_group_span(&groups[0]) > 330.0,
        "{groups:?}"
    );
    assert!(
        rendered_fragment_group_span(&groups[1]) > 330.0,
        "{groups:?}"
    );
    let last = &groups[2];
    let last_right = last
        .iter()
        .map(|line| line.x + rendered_line_advance(line))
        .fold(f32::NEG_INFINITY, f32::max);
    let previous_right = groups[0]
        .iter()
        .map(|line| line.x + rendered_line_advance(line))
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (last_right - previous_right).abs() < 1.0,
        "last RTL justify line should be start/right aligned: last={last_right}, previous={previous_right}"
    );
}

#[tokio::test]
async fn wpt_text_align_justify_all_ltr_justifies_final_line_in_rtl_parent() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 760pt 360pt; margin: 0 }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ text-align: justify-all; direction: ltr }}\
         .test {{ border: 1px solid orange; margin: 20px; width: 510px; color: orange; font: 30px/1 Ahem }}</style>\
         <div style=\"direction: rtl\"><div class=\"test\">TES TES TES TES TES TES TES TES TES TES TES </div></div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut groups = visual_line_groups(&document.pages[0].lines);
    groups.retain(|group| group.iter().any(|line| line.text.contains("TES")));
    assert_eq!(groups.len(), 3, "{groups:?}");
    let first_span = rendered_fragment_group_span(&groups[0]);
    let last_span = rendered_fragment_group_span(&groups[2]);
    assert!(first_span > 380.0, "{groups:?}");
    assert!(
        last_span > 380.0,
        "justify-all final line should be justified too: first={first_span}, last={last_span}, groups={groups:?}"
    );
}

#[tokio::test]
async fn justified_inline_spans_keep_one_shaped_text_group() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 220pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ width: 120pt; font: 10pt/10pt Ahem; text-align: justify; text-align-last: justify }}</style>\
         <div class=\"test\"><span>XXXX</span><span> </span><span>XXXX</span></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "XXXX XXXX")
        .expect("justified inline spans should paint as one text group");
    let space_advance = line
        .runs
        .iter()
        .flat_map(|run| run.glyphs.as_deref().unwrap_or(&[]))
        .find(|glyph| glyph.unicode == " ")
        .map(|glyph| glyph.x_advance)
        .expect("space glyph");
    assert!(
        rendered_line_advance(line) > 115.0 && space_advance > 30.0,
        "justification should be carried by shaped glyph advances: line={line:?}, space={space_advance}"
    );
}

#[tokio::test]
async fn justified_bidi_line_expands_visual_span() {
    let document = Html::from_string(
        "<style>@page { size: 280pt 120pt; margin: 10pt } body { margin: 0 }\
         .test { width: 220pt; font: 12pt/14pt sans-serif; text-align: justify; text-align-last: justify }</style>\
         <div class=\"test\">abc אבג def</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("גבא"))
        .expect("expected bidi visual text line");
    let (left, right) = rendered_line_visual_bounds(line);
    assert!(
        right - left > 200.0,
        "justified bidi line should expand in visual glyph positions: left={left}, right={right}, line={line:?}"
    );
}

#[tokio::test]
async fn justified_mixed_inline_shifts_atom_and_later_text() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 240pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ width: 140pt; font: 10pt/10pt Ahem; text-align: justify; text-align-last: justify }}\
         .atom {{ display: inline-block; width: 10pt; height: 10pt; background: blue }}</style>\
         <div class=\"test\"><span>XXXX </span><span class=\"atom\"></span><span>XXXX</span></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let group = visual_line_groups(&document.pages[0].lines)
        .into_iter()
        .find(|group| group.iter().any(|line| line.text.contains("XXXX")))
        .expect("expected mixed inline text group");
    assert_eq!(group.len(), 2, "{group:?}");
    let mut ordered = group.clone();
    ordered.sort_by(|left, right| left.x.total_cmp(&right.x));
    let first = ordered[0];
    let second = ordered[1];
    assert!(
        second.x - first.x > 90.0 && rendered_fragment_group_span(&group) > 125.0,
        "justification should shift the atom and later text across the line: {group:?}"
    );
}

#[tokio::test]
async fn supports_word_spacing_in_text_measurement_and_painting() {
    let normal = Html::from_string(
        "<style>@page { size: 140pt 90pt; margin: 10pt } p { margin: 0; font-family: monospace; font-size: 10pt; line-height: 10pt }</style><p>A A</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let spaced = Html::from_string(
        "<style>@page { size: 140pt 90pt; margin: 10pt } p { margin: 0; font-family: monospace; font-size: 10pt; line-height: 10pt; word-spacing: 20pt }</style><p>A A</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let normal_width = rendered_line_advance(&normal.pages[0].lines[0]);
    let spaced_width = rendered_line_advance(&spaced.pages[0].lines[0]);

    assert!(
        spaced_width - normal_width > 19.0,
        "expected word-spacing to add about 20pt, normal={normal_width}, spaced={spaced_width}"
    );
}

#[tokio::test]
async fn wpt_text_autospace_inserts_ideograph_alpha_spacing() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 360pt 140pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ font: 40px/1 Ahem; margin: 0 }}\
         .off {{ text-autospace: no-autospace }} .on {{ text-autospace: normal }}</style>\
         <div class=\"test off\">国国XX国</div><div class=\"test on\">国国XX国</div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let off_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.y > 90.0)
        .collect::<Vec<_>>();
    let on_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.y <= 90.0)
        .collect::<Vec<_>>();
    let off = rendered_fragment_group_span(&off_lines);
    let on = rendered_fragment_group_span(&on_lines);

    assert!(
        (on - off - 7.5).abs() < 1.0,
        "normal autospace should add two 1/8em gaps at 40px/30pt, off={off}, on={on}, off_lines={off_lines:?}, on_lines={on_lines:?}"
    );
}

#[tokio::test]
async fn wpt_text_autospace_does_not_treat_punctuation_as_normal_alpha_spacing() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 360pt 140pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ font: 40px/1 Ahem; margin: 0 }}\
         .off {{ text-autospace: no-autospace }} .on {{ text-autospace: normal }}</style>\
         <div class=\"test off\">国。XX国</div><div class=\"test on\">国。XX国</div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut groups = lines_grouped_by_y(&document.pages[0].lines);
    groups.sort_by(|left, right| right[0].y.total_cmp(&left[0].y));
    assert_eq!(groups.len(), 2);
    let off = rendered_fragment_group_span(&groups[0]);
    let on = rendered_fragment_group_span(&groups[1]);

    assert!(
        (on - off - 3.75).abs() < 1.0,
        "normal autospace should add one 1/8em gap before the final ideograph at 40px/30pt, off={off}, on={on}, groups={groups:?}"
    );
}

#[tokio::test]
async fn nonzero_letter_spacing_disables_common_ligature_shaping() {
    let mplus = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/mplus-1p-regular.woff";
    if !std::path::Path::new(mplus).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 400pt 160pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: mplus; src: url(file://{mplus}) }}\
         p {{ margin: 0; font: 32px/1 mplus; letter-spacing: 20px }}</style><p>office</p>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let glyph_text = document.pages[0].lines[0]
        .runs
        .iter()
        .flat_map(|run| run.glyphs.as_deref().unwrap_or_default())
        .filter(|glyph| !glyph.unicode.is_empty())
        .map(|glyph| glyph.unicode.as_str())
        .collect::<Vec<_>>();
    assert_eq!(glyph_text, vec!["o", "f", "f", "i", "c", "e"]);
}

#[tokio::test]
async fn letter_spacing_crosses_text_empty_inline_boundaries() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 300pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         p {{ margin: 0; font: 25px/1 Ahem; letter-spacing: 25px; white-space: pre-wrap }}</style>\
         <p><span></span>A<span></span><span></span>D<span></span></p>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "AD")
        .expect("expected text-empty inline content to preserve A/D text");
    let width = rendered_line_advance(line);
    assert!(
        (width - 75.0).abs() < 1.0,
        "expected Ahem A+D plus one inter-letter tracking advance, got {width}"
    );
}

fn rendered_fragment_group_span(lines: &[&quire::RenderedLine]) -> f32 {
    let left = lines
        .iter()
        .map(|line| line.x)
        .fold(f32::INFINITY, f32::min);
    let right = lines
        .iter()
        .map(|line| line.x + rendered_line_advance(line))
        .fold(f32::NEG_INFINITY, f32::max);
    right - left
}

fn lines_grouped_by_y(lines: &[quire::RenderedLine]) -> Vec<Vec<&quire::RenderedLine>> {
    let mut groups = Vec::<Vec<&quire::RenderedLine>>::new();
    for line in lines {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| (group[0].y - line.y).abs() < 0.01)
        {
            group.push(line);
        } else {
            groups.push(vec![line]);
        }
    }
    groups
}

fn grouped_line_texts(page: &quire::Page) -> Vec<String> {
    let mut groups = Vec::<Vec<&quire::RenderedLine>>::new();
    for line in &page.lines {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| (group[0].y - line.y).abs() < 4.0)
        {
            group.push(line);
        } else {
            groups.push(vec![line]);
        }
    }
    groups
        .into_iter()
        .map(|mut group| {
            group.sort_by(|left, right| left.x.total_cmp(&right.x));
            group
                .into_iter()
                .map(|line| line.text.as_str())
                .collect::<String>()
        })
        .collect()
}

fn rendered_line_visual_bounds(line: &quire::RenderedLine) -> (f32, f32) {
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for run in &line.runs {
        let mut pen_x = line.x + run.x_offset;
        if let Some(glyphs) = &run.glyphs {
            for glyph in glyphs {
                let start = pen_x + glyph.x_offset;
                let end = start + glyph.x_advance;
                left = left.min(start.min(end));
                right = right.max(start.max(end));
                pen_x += glyph.x_advance;
            }
        }
    }
    if left.is_finite() && right.is_finite() {
        (left, right)
    } else {
        (line.x, line.x + rendered_line_advance(line))
    }
}

fn visual_line_groups(lines: &[quire::RenderedLine]) -> Vec<Vec<&quire::RenderedLine>> {
    let mut groups = Vec::<Vec<&quire::RenderedLine>>::new();
    for line in lines {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| (group[0].y - line.y).abs() < 0.01)
        {
            group.push(line);
        } else {
            groups.push(vec![line]);
        }
    }
    groups
}

#[tokio::test]
async fn supports_first_line_text_indent_lengths() {
    let options = RenderOptions::default();
    let positive = Html::from_string(
        "<style>body { margin: 0; font-size: 10pt; line-height: 10pt } p { margin: 0; width: 40pt; text-indent: 10pt }</style><p>aa aa aa aa aa</p>",
    )
    .render_async(&options).await
    .unwrap();
    let lines = &positive.pages[0].lines;

    assert!(lines.len() > 1);
    assert!((lines[0].x - (options.page_margins.left + 10.0)).abs() < 0.01);
    assert!((lines[1].x - options.page_margins.left).abs() < 0.01);

    let negative = Html::from_string(
        "<style>body { margin: 0; font-size: 10pt; line-height: 10pt } p { margin: 0; width: 40pt; text-indent: -10pt }</style><p>aa aa aa aa aa aa aa aa</p>",
    )
    .render_async(&options).await
    .unwrap();
    let lines = &negative.pages[0].lines;

    assert!(lines.len() > 1);
    assert!((lines[0].x - (options.page_margins.left - 10.0)).abs() < 0.01);
    assert!((lines[1].x - options.page_margins.left).abs() < 0.01);
}

#[tokio::test]
async fn text_indent_each_line_applies_after_forced_breaks_only() {
    let options = RenderOptions::default();
    let document = Html::from_string(
        "<style>body { margin: 0; font-family: monospace; font-size: 10pt; line-height: 12pt } p { margin: 0; width: 44pt; text-indent: 12pt each-line }</style><p>one two<br>red blue green</p>",
    )
    .render_async(&options)
    .await
    .unwrap();
    let lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();

    assert!(
        lines.len() >= 5,
        "expected soft wraps after the forced break: {lines:?}"
    );
    assert_eq!(lines[0].text.trim(), "one");
    assert_eq!(lines[1].text.trim(), "two");
    assert_eq!(lines[2].text.trim(), "red");
    assert_eq!(lines[3].text.trim(), "blue");
    assert_eq!(lines[4].text.trim(), "green");
    assert!((lines[0].x - (options.page_margins.left + 12.0)).abs() < 0.01);
    assert!((lines[1].x - options.page_margins.left).abs() < 0.01);
    assert!((lines[2].x - (options.page_margins.left + 12.0)).abs() < 0.01);
    assert!((lines[3].x - options.page_margins.left).abs() < 0.01);
    assert!((lines[4].x - options.page_margins.left).abs() < 0.01);
}

#[tokio::test]
async fn supports_text_transform() {
    let document = Html::from_string(
        "<div style=\"text-transform: uppercase\"><p style=\"margin: 0\">Hello world</p></div><p style=\"margin: 0; text-transform: capitalize\">mixed CASE words</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "HELLO WORLD");
    assert_eq!(document.pages[0].lines[1].text, "Mixed CASE Words");
}

#[tokio::test]
async fn capitalize_leaves_non_initial_word_characters_unchanged() {
    let document = Html::from_string(
        "<p style=\"margin: 0; text-transform: capitalize\">mIXed caSE 123abc</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "MIXed CaSE 123abc");
}

#[tokio::test]
async fn capitalize_uses_icu_full_case_mapping_for_latin_1() {
    let document =
        Html::from_string("<p style=\"margin: 0; text-transform: capitalize\">aaa µµµ ààà ÿÿÿ</p>")
            .render_async(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "Aaa Μµµ Ààà Ÿÿÿ");
}

#[tokio::test]
async fn text_transform_uses_language_tailored_case_mapping() {
    let document = Html::from_string(
        "<p lang=\"tr\" style=\"margin:0;text-transform:uppercase\">i ı</p>\
         <p lang=\"tr\" style=\"margin:0;text-transform:lowercase\">İ I</p>\
         <p lang=\"el\" style=\"margin:0;text-transform:lowercase\">ΟΣ ΟΣΑ</p>\
         <p lang=\"lt\" style=\"margin:0;text-transform:lowercase\">I\u{0301}</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "İ I");
    assert_eq!(document.pages[0].lines[1].text, "i ı");
    assert_eq!(document.pages[0].lines[2].text, "ος οσα");
    assert_eq!(document.pages[0].lines[3].text, "i\u{0307}\u{0301}");
}

#[tokio::test]
async fn supports_full_width_and_full_size_kana_text_transform() {
    let document = Html::from_string(
        "<p style=\"margin: 0; text-transform: full-width\">A 1 ｶﾞ</p>\
         <p style=\"margin: 0; text-transform: full-size-kana\">ぁァㇷ</p>\
         <p style=\"margin: 0; text-transform: uppercase full-width full-size-kana\">ab ァ</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "Ａ　１　ガ");
    assert_eq!(document.pages[0].lines[1].text, "あアフ");
    assert_eq!(document.pages[0].lines[2].text, "ＡＢ　ア");
}

#[tokio::test]
async fn supports_text_transform_inside_text_only_inline_block() {
    let document = Html::from_string(
        "<p style=\"margin: 0\"><span style=\"display: inline-block; text-transform: uppercase\">Hello world</span></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "HELLO WORLD");
}

#[tokio::test]
async fn capitalize_ignores_out_of_flow_inline_boundaries() {
    let document = Html::from_string(
        "<style>.caps { text-transform: capitalize } .abs { position: absolute }</style>\
         <p style=\"margin: 0\">\
         <span class=\"caps\">abc</span><span class=\"abs\"></span> <span class=\"caps\">abc</span><br>\
         <span class=\"caps\">abc<span class=\"abs\"></span></span> <span class=\"caps\">abc</span><br>\
         <span class=\"caps\">abc</span> <span class=\"abs\"></span><span class=\"caps\">abc</span>\
         </p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["Abc Abc", "Abc Abc", "Abc Abc"]);
}

#[tokio::test]
async fn capitalize_does_not_treat_inline_boundaries_as_word_boundaries() {
    let document = Html::from_string(
        "<p style=\"margin: 0; text-transform: capitalize\"><span>ab</span><span>CD</span> eF</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "AbCD EF");
}

#[tokio::test]
async fn capitalize_uses_unicode_word_boundaries_across_inline_fragments() {
    let document = Html::from_string(
        "<p style=\"margin: 0; text-transform: capitalize\">mark’d ye <span>mark</span><span>’</span><span>d</span></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "Mark’d Ye Mark’d");
}

#[tokio::test]
async fn renders_bidi_text_in_visual_order() {
    let document = Html::from_string(
        "<p style=\"margin: 0; font-size: 12pt; line-height: 12pt\">abc אבג def</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "abc גבא def");
}

#[tokio::test]
async fn renders_styled_bidi_inline_text_in_visual_order() {
    let document = Html::from_string(
        "<p style=\"margin: 0; font-size: 12pt; line-height: 12pt\">abc <em>אבג</em> def</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let y = document.pages[0].lines[0].y;
    let mut line = document.pages[0]
        .lines
        .iter()
        .filter(|line| (line.y - y).abs() < 0.1)
        .collect::<Vec<_>>();
    line.sort_by(|left, right| left.x.total_cmp(&right.x));
    let text = line
        .into_iter()
        .map(|line| line.text.as_str())
        .collect::<String>();

    assert_eq!(text, "abc גבא def");
}

#[tokio::test]
async fn mixed_inline_ltr_atomic_child_inside_rtl_parent_uses_parent_base_direction() {
    let document = Html::from_string(
        "<p style=\"margin:0; font-size:12pt; line-height:12pt; direction:rtl\">אבג \
         <span style=\"display:inline-block; direction:ltr\">abc</span> דהו</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let left_hebrew = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "והד")
        .expect("expected inline-end Hebrew run");
    let atom = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "abc")
        .expect("expected LTR atomic child");
    let right_hebrew = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "גבא")
        .expect("expected inline-start Hebrew run");

    assert!(left_hebrew.x < atom.x);
    assert!(atom.x < right_hebrew.x);
}

#[tokio::test]
async fn mixed_inline_rtl_atomic_child_inside_ltr_parent_uses_parent_base_direction() {
    let document = Html::from_string(
        "<p style=\"margin:0; font-size:12pt; line-height:12pt; direction:ltr\">abc \
         <span style=\"display:inline-block; direction:rtl\">אבג</span> def</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let left_latin = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "abc")
        .expect("expected inline-start Latin run");
    let atom = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "גבא")
        .expect("expected RTL atomic child");
    let right_latin = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "def")
        .expect("expected inline-end Latin run");

    assert!(left_latin.x < atom.x);
    assert!(atom.x < right_latin.x);
}

#[tokio::test]
async fn unicode_bidi_plaintext_aligns_lines_by_first_strong_character() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 180pt; margin: 10pt } body { margin: 0 } \
         div { font-size: 16pt; line-height: 18pt; width: 160pt; white-space: pre; \
               text-align: start; unicode-bidi: plaintext; border: 1pt solid black; padding: 0 6pt }</style>\
         <div>français\nفارسی\nfrançais\nفارسی</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| {
            line.text.contains("français")
                || line.text.contains("فارسی")
                || line.text.contains("یسراف")
        })
        .collect::<Vec<_>>();

    assert_eq!(lines.len(), 4);
    let latin_x = lines
        .iter()
        .filter(|line| line.text.contains("français"))
        .map(|line| line.x)
        .collect::<Vec<_>>();
    let persian_x = lines
        .iter()
        .filter(|line| line.text.contains("فارسی") || line.text.contains("یسراف"))
        .map(|line| line.x)
        .collect::<Vec<_>>();

    assert!(latin_x.iter().all(|x| (*x - latin_x[0]).abs() < 0.1));
    assert!(persian_x.iter().all(|x| *x > latin_x[0] + 60.0));
}

#[tokio::test]
async fn unicode_bidi_override_scopes_inline_visual_order() {
    let document = Html::from_string(
        "<p style=\"margin:0; font-size:12pt; line-height:12pt\">abc \
         <span style=\"direction:rtl; unicode-bidi:bidi-override\">def</span> ghi</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "abc fed ghi");
}

#[tokio::test]
async fn html_dir_auto_sets_direction_from_first_strong_descendant_text() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 10pt } body { margin: 0 } \
         p { margin: 0; width: 160pt; font-size: 12pt; line-height: 14pt; text-align: start }</style>\
         <p dir=\"auto\">abc</p><p dir=\"auto\">אבג</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let latin = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "abc")
        .expect("expected ltr line");
    let hebrew = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "גבא")
        .expect("expected rtl visual line");

    assert!(hebrew.x > latin.x + 120.0);
}

#[tokio::test]
async fn author_direction_overrides_html_dir_auto_directionality() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body { margin: 0 } \
         p { margin: 0; width: 160pt; font-size: 12pt; line-height: 14pt; text-align: start }</style>\
         <p dir=\"auto\" style=\"direction:ltr\">אבג</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "גבא")
        .expect("expected visual rtl text");

    assert!(line.x < RenderOptions::default().page_margins.left + 20.0);
}

#[tokio::test]
async fn html_bdo_uses_ua_isolate_override() {
    let document = Html::from_string(
        "<p style=\"margin:0; font-size:12pt; line-height:12pt\">abc <bdo dir=\"rtl\">def</bdo> ghi</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "abc fed ghi");
}

#[tokio::test]
async fn join_controls_do_not_split_inline_shaping_runs() {
    let document = Html::from_string(
        "<p style=\"margin: 0; font-size: 12pt; line-height: 12pt; font-family: sans-serif\">A<span style=\"font-family: serif\">&#x200c;</span>B</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let line = &document.pages[0].lines[0];
    assert_eq!(line.text, "A\u{200c}B");
    assert_eq!(line.runs.len(), 1);
    assert_eq!(line.runs[0].text, "A\u{200c}B");
}

#[tokio::test]
async fn generated_control_characters_render_as_visible_glyphs() {
    let document = Html::from_string(
        r#"<style>body { margin: 0 } div { font-size: 20pt; line-height: 20pt } div::after { content: "\0099" }</style><div></div>"#,
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let line = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "\u{fffd}")
        .expect("generated control character should be replaced by a visible glyph");
    assert!(rendered_line_advance(line) > 0.0);
}

#[tokio::test]
async fn supports_white_space_pre_wrap_newlines() {
    let document = Html::from_string(
        "<p style=\"white-space: pre-wrap; margin: 0; font-size: 10pt; line-height: 10pt\">One\nTwo</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "One");
    assert_eq!(document.pages[0].lines[1].text, "Two");
    assert!(document.pages[0].lines[1].y < document.pages[0].lines[0].y);
}

#[tokio::test]
async fn html_br_line_break_comes_from_generated_before_content() {
    let document = Html::from_string(
        "<style>p { margin: 0; font-size: 10pt; line-height: 10pt } br::before { content: none }</style><p>One<br>Two</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(
        lines,
        vec!["OneTwo"],
        "author-overridden br::before should suppress the UA generated line break"
    );
}

#[tokio::test]
async fn pre_wrap_final_segment_break_does_not_create_empty_line() {
    let document = Html::from_string(
        "<p style=\"white-space: pre-wrap; margin: 0; font-size: 10pt; line-height: 10pt\">One\n</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["One"]);
}

#[tokio::test]
async fn pre_wrap_break_word_prefers_preserved_space_opportunities() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 120pt; margin: 0 }\
         div { margin: 0; width: 30pt; font-family: monospace; font-size: 10pt; line-height: 10pt;\
               white-space: pre-wrap; word-break: break-word }\
         </style><div> XX XXX </div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| {
            line.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(lines, vec![" XX".to_string(), "XXX ".to_string()]);
}

#[tokio::test]
async fn pre_wrap_tabs_break_after_tab_sequence() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 120pt; margin: 0 }\
         div { margin: 0; width: 60pt; font-family: monospace; font-size: 10pt; line-height: 10pt;\
               white-space: pre-wrap }\
         </style><div>XX\t\tXX</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| {
            line.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(lines, vec!["XX".to_string(), "XX".to_string()]);
}

#[tokio::test]
async fn ch_width_uses_selected_font_zero_advance() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 120pt; margin: 0 }\
         div { margin: 0; width: 4ch; font-family: sans-serif; font-size: 20pt; line-height: 20pt;\
               overflow-wrap: anywhere }\
         </style><div>0000X</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(lines, vec!["0000", "X"]);
}

#[tokio::test]
async fn supports_white_space_nowrap() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } p { white-space: nowrap; margin: 0; width: 40pt; font-size: 10pt; line-height: 10pt }</style><p>one two three four</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines.len(), 1);
    assert_eq!(document.pages[0].lines[0].text, "one two three four");
}

#[tokio::test]
async fn supports_white_space_pre() {
    let document = Html::from_string(
        "<p style=\"white-space: pre; margin: 0; font-size: 10pt; line-height: 10pt\"> A  B\nC</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, " A  B");
    assert_eq!(document.pages[0].lines[1].text, "C");
}

#[tokio::test]
async fn supports_white_space_pre_line() {
    let document = Html::from_string(
        "<p style=\"white-space: pre-line; margin: 0; font-size: 10pt; line-height: 10pt\">A   B\nC</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "A B");
    assert_eq!(document.pages[0].lines[1].text, "C");
}

#[tokio::test]
async fn supports_white_space_break_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 120pt; margin: 10pt } p { white-space: break-spaces; margin: 0; width: 14pt; font-size: 10pt; line-height: 10pt }</style><p>A   B</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "A  ");
    assert_eq!(document.pages[0].lines[1].text, " B");
}

#[tokio::test]
async fn supports_overflow_wrap_anywhere() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } p { margin: 0; width: 18pt; font-size: 10pt; line-height: 10pt; overflow-wrap: anywhere }</style><p>abcdefgh</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(lines.len() > 1);
    assert_eq!(lines.concat(), "abcdefgh");
}

#[tokio::test]
async fn supports_word_break_break_all() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } p { margin: 0; width: 18pt; font-size: 10pt; line-height: 10pt; word-break: break-all }</style><p>mnopqrst</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(lines.len() > 1);
    assert_eq!(lines.concat(), "mnopqrst");
}

#[tokio::test]
async fn min_content_width_uses_css_text_line_break_opportunities() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 180pt; margin: 10pt }\
         p { margin: 0; width: min-content; font-size: 10pt; line-height: 10pt }\
         </style><p>中文english中文</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-text/word-break/word-break-keep-all-011.html.
    // CSS Sizing's min-content inline size must be computed from CSS Text
    // soft-wrap opportunities, not just from document whitespace.
    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(lines, ["中文", "english", "中文"]);
}

#[tokio::test]
async fn word_break_keep_all_min_content_suppresses_letter_unit_breaks() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 220pt; margin: 10pt }\
         p { margin: 0; width: min-content; font-size: 10pt; line-height: 10pt; word-break: keep-all }\
         </style><p>中文english中文english 中文english中文，english中文english</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    // CSS Text `word-break: keep-all` suppresses implicit opportunities
    // between NU/AL/AI/ID units while preserving whitespace and punctuation
    // opportunities:
    // https://www.w3.org/TR/css-text-3/#word-break-property.
    let lines = grouped_line_texts(&document.pages[0]);

    assert_eq!(
        lines,
        [
            "中文english中文english".to_string(),
            "中文english中文，english中文english".to_string()
        ]
    );
}

#[tokio::test]
async fn inherits_css_text_breaking_controls() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } div { word-break: break-all } p { margin: 0; width: 18pt; font-size: 10pt; line-height: 10pt }</style><div><p>mnopqrst</p></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(lines.len() > 1);
    assert_eq!(lines.concat(), "mnopqrst");
}

#[tokio::test]
async fn supports_line_break_anywhere() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } div { line-break: anywhere } p { margin: 0; width: 18pt; font-size: 10pt; line-height: 10pt }</style><div><p>abcdefgh</p></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(lines.len() > 1);
    assert_eq!(lines.concat(), "abcdefgh");
}

#[tokio::test]
async fn cjk_inline_text_wraps_before_ascii_closing_brace() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } div { margin: 0; width: 95px; font-size: 30px; line-height: 1em }</style><div>中中中}文</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(lines, ["中中", "中}文"]);
}

#[tokio::test]
async fn mixed_inline_soft_wrap_uses_hard_break_line_metrics() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 } \
         .box { background: rgb(10, 20, 30); color: black; font-family: named-monospace, monospace; \
                font-size: 10pt; line-height: 10pt; margin: 0; width: 13pt; word-break: break-all }</style>\
         <div class=\"box\">ABCD</div><div class=\"box\">AB<br>CD</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let boxes = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(10, 20, 30)))
        .collect::<Vec<_>>();

    assert_eq!(boxes.len(), 2);
    assert!(
        (boxes[0].height - boxes[1].height).abs() < 0.01,
        "soft and hard line breaks should consume the same line-box height: {boxes:?}"
    );
}

#[tokio::test]
async fn html_wbr_generated_before_creates_soft_wrap_opportunity() {
    let document = Html::from_string(
        "<style>@page { size: 80pt 120pt; margin: 10pt } p { margin: 0; width: 21pt; font-family: monospace; font-size: 10pt; line-height: 10pt }</style><p>abc<wbr>def</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(
        lines,
        vec!["abc", "def"],
        "wbr should contribute a soft wrap opportunity without visible text"
    );
}

#[tokio::test]
async fn hides_unbroken_soft_hyphens() {
    let document = Html::from_string(
        "<p style=\"margin: 0; font-size: 10pt; line-height: 10pt\">hyphen&shy;ation</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "hyphenation");
}

#[tokio::test]
async fn shows_soft_hyphens_when_line_breaks_there() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } p { margin: 0; width: 38pt; font-size: 10pt; line-height: 10pt }</style><p>hyphen&shy;ation</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(document.pages[0].lines.len() > 1);
    assert_eq!(document.pages[0].lines[0].text, "hyphen-");
    assert_eq!(
        document.pages[0]
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>(),
        "hyphen-ation"
    );
}

#[tokio::test]
async fn styled_inline_soft_hyphens_follow_manual_hyphenation() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } p { margin: 0; width: 38pt; font-size: 10pt; line-height: 10pt }</style><p><strong>hyphen&shy;</strong><em>ation</em></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "hyphen-")
    );
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "ation")
    );
}

#[tokio::test]
async fn hyphens_none_suppresses_soft_hyphen_breaks() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } div { hyphens: none } p { margin: 0; width: 38pt; font-size: 10pt; line-height: 10pt }</style><div><p>hyphen&shy;ation</p></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines.len(), 1);
    assert_eq!(document.pages[0].lines[0].text, "hyphenation");
}

#[tokio::test]
async fn hyphens_auto_uses_document_language() {
    let document = Html::from_string(
        "<html lang=\"en\"><style>@page { size: 90pt 120pt; margin: 10pt } p { hyphens: auto; margin: 0; width: 22pt; font-size: 10pt; line-height: 10pt }</style><p>ribonuclease</p></html>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.replace('-', ""))
        .collect::<String>();

    assert!(document.pages[0].lines.len() > 1);
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text.ends_with('-'))
    );
    assert_eq!(text, "ribonuclease");
}

#[tokio::test]
async fn break_spaces_exposes_breaks_before_atomic_inline_boxes() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 120pt; margin: 10pt } p { white-space: break-spaces; margin: 0; width: 14pt; font-size: 10pt; line-height: 10pt } span { display:inline-block; width:5pt }</style><p>A   <span>B</span></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let a = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "A  ")
        .unwrap();
    let b = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "B")
        .unwrap();

    assert!(a.y > b.y);
}

#[tokio::test]
async fn overflow_wrap_applies_before_atomic_inline_boxes() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 120pt; margin: 10pt } p { overflow-wrap: anywhere; margin: 0; width: 18pt; font-size: 10pt; line-height: 10pt } span { display:inline-block; width:5pt }</style><p>abcdefgh<span>B</span></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| line.text != "B")
        .collect::<Vec<_>>();
    let text = text_lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<String>();

    assert_eq!(text, "abcdefgh");
    assert!(text_lines.len() > 1);
}

#[tokio::test]
async fn mixed_inline_text_decorations_paint_with_atomic_inline_boxes() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 10pt } p { margin: 0; font-size: 10pt; line-height: 12pt; text-decoration: underline } span { display:inline-block; width:10pt }</style><p>Text<span>Box</span></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Text")
        .unwrap();

    assert!(document.pages[0].rects.iter().any(|rect| {
        rect.fill == Some(Color::BLACK)
            && (rect.x - text.x).abs() < 0.1
            && rect.y < text.y
            && rect.y > text.y - 4.0
            && rect.width > 1.0
            && rect.height >= 0.5
    }));
}

#[tokio::test]
async fn text_decoration_wavy_paints_as_paths() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 80pt; margin: 10pt } p { margin: 0; font-size: 16pt; text-decoration-line: underline; text-decoration-style: wavy; text-decoration-thickness: 2pt; text-decoration-skip-ink: none }</style><p>Wave</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(document.pages[0].paths.iter().any(|path| {
        path.stroke == Some(Color::BLACK) && path.stroke_width >= 1.9 && path.commands.len() > 2
    }));
}

#[tokio::test]
async fn spelling_and_grammar_error_decorations_paint_wavy_indicators() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } p { margin: 0; font-size: 16pt; line-height: 20pt }</style><p style=\"text-decoration-line: spelling-error\">Spell</p><p style=\"text-decoration-line: grammar-error\">Grammar</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .paths
            .iter()
            .any(|path| path.stroke == Some(Color::new(255, 0, 0)))
    );
    assert!(
        document.pages[0]
            .paths
            .iter()
            .any(|path| path.stroke == Some(Color::new(0, 128, 0)))
    );
}

#[tokio::test]
async fn text_decoration_skip_ink_splits_underlines_around_glyph_ink() {
    let without_skip = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } p { margin: 0; font-size: 18pt; text-decoration-line: underline; text-decoration-thickness: 2pt; text-decoration-skip-ink: none; text-underline-offset: -0.35em }</style><p>gap gap</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let with_skip = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } p { margin: 0; font-size: 18pt; text-decoration-line: underline; text-decoration-thickness: 2pt; text-decoration-skip-ink: auto; text-underline-offset: -0.35em }</style><p>gap gap</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let unskipped_width: f32 = without_skip.pages[0]
        .rects
        .iter()
        .map(|rect| rect.width)
        .sum();
    let skipped_width: f32 = with_skip.pages[0].rects.iter().map(|rect| rect.width).sum();

    assert!(unskipped_width > 0.0);
    assert!(skipped_width < unskipped_width);
}

#[tokio::test]
async fn text_decoration_skip_spaces_trims_preserved_line_edge_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 80pt; margin: 10pt } body { margin: 0 } \
         div { color: orange; font-size: 24pt; line-height: 30pt; white-space: break-spaces; \
         text-decoration-line: underline; text-decoration-color: blue; \
         text-decoration-skip-spaces: start end; text-decoration-skip-ink: none }</style>\
         <div>        ABCDEF        </div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = document
        .pages
        .first()
        .and_then(|page| page.lines.iter().find(|line| line.text.contains("ABCDEF")))
        .unwrap();
    let underline = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .max_by(|left, right| {
            left.width
                .partial_cmp(&right.width)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();

    let line_start = line.x;
    let underline_start = underline.x;
    let underline_end = underline.x + underline.width;
    let line_width = line
        .runs
        .iter()
        .map(|run| {
            run.x_offset
                + run
                    .glyphs
                    .as_ref()
                    .map(|glyphs| glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>())
                    .unwrap_or(0.0)
        })
        .fold(0.0, f32::max);

    assert!(underline_start > line_start + 10.0);
    assert!(underline_end < line_start + line_width - 10.0);
    assert!(underline.width < line_width * 0.75);
}

#[tokio::test]
async fn inline_block_text_decoration_paints_for_transparent_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 } \
         u { display: inline-block; width: 100px; font: 20px/1 sans-serif; color: transparent; \
         text-decoration: green underline; text-decoration-skip-ink: none; \
         text-underline-offset: 0; text-decoration-thickness: 100% }</style><u>X X</u>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0].rects.iter().any(|rect| {
            rect.fill == Some(Color::new(0, 128, 0))
                && rect.width > 20.0
                && (rect.height - 15.0).abs() < 0.1
        }),
        "expected green underline rects, got {:?}",
        (
            document.pages[0].rects.clone(),
            document.pages[0].lines.clone()
        )
    );
}

#[tokio::test]
async fn floated_auto_width_uses_definite_child_width_not_overflow_text() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body { margin: 0 } \
         section { float: left; margin: 0 5pt; color: blue } \
         div { width: 20pt; white-space: nowrap; font: 10pt/10pt monospace }</style>\
         <section><div>XXXXXXXXXX</div></section><section><div>Y</div></section>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let first = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("XXXXXXXXXX"))
        .unwrap();
    let second = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Y")
        .unwrap();

    assert!(
        second.x - first.x < 45.0,
        "overflowing fixed-width child text must not widen the first float: first={}, second={}",
        first.x,
        second.x
    );
}

#[tokio::test]
async fn thick_overline_is_clipped_by_overflow_hidden_block() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 100pt; margin: 10pt } body { margin: 0 }\
         #box { font-size: 20px; line-height: 20px; overflow: hidden; height: 1em; width: 4em; background: red }\
         #text { color: transparent; position: relative; top: 3em; text-decoration: green overline; text-decoration-skip-ink: none; text-decoration-thickness: 4em }\
         </style><div id=\"box\"><div id=\"text\">XXXXXXXXXXXXXXXXXXXX</div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let green_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .collect::<Vec<_>>();

    assert_eq!(green_rects.len(), 1);
    assert!(
        (green_rects[0].width - 60.0).abs() < 0.01,
        "green rect: {:?}",
        green_rects[0]
    );
    assert!((green_rects[0].height - 15.0).abs() < 0.01);
    assert!(
        !document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(255, 0, 0)))
    );
}

#[tokio::test]
async fn font_shorthand_unit_line_height_sets_inline_background_height() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 0 } body { margin: 0 }\
         div { font: 50px/1 sans-serif; width: 2em; background: green; color: green }</style>\
         <div>&#x3000;&#x3000;XX</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("green background should paint");

    assert!((green.width - 75.0).abs() < 0.01, "{green:?}");
    assert!((green.height - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn explicit_line_height_overrides_loaded_font_metrics() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }

    let document = Html::from_string(format!(
        "<style>@page {{ size: 200pt 200pt; margin: 0 }} body {{ margin: 0 }}\
             @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
             div {{ font: 50px/1 Ahem; width: 2em; background: green; color: green }}</style>\
             <div>&#x3000;&#x3000;XX</div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("green background should paint");

    assert!((green.width - 75.0).abs() < 0.01, "{green:?}");
    assert!((green.height - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn trailing_ideographic_space_hangs_and_paints_inline_background() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }

    let document = Html::from_string(format!(
        "<style>@page {{ size: 200pt 200pt; margin: 0 }} body {{ margin: 0 }}\
             @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
             div {{ font: 50px/1 Ahem; width: 1ch }}\
             span {{ background: green; color: transparent; unicode-bidi: plaintext; hyphens: none }}</style>\
             <div><span>X&#x3000;<br>XX</span></div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .collect::<Vec<_>>();
    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    assert!(
        green_rects
            .iter()
            .all(|rect| (rect.width - 75.0).abs() < 0.01 && (rect.height - 37.5).abs() < 0.01),
        "{green_rects:?}"
    );
    assert!(
        ((green_rects[0].y - green_rects[1].y).abs() - 37.5).abs() < 0.01,
        "{green_rects:?}"
    );
}

#[tokio::test]
async fn combining_grapheme_joiner_suppresses_wrap_before_atomic_inline() {
    let ahem = "/Users/lee/oss/quire-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }

    let document = Html::from_string(format!(
        "<style>@page {{ size: 300pt 300pt; margin: 0 }} body {{ margin: 0 }}\
             @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
             div {{ font: 100px/1 Ahem; color: green; width: 100px; background: red }}\
             span {{ display: inline-block; color: transparent }}</style>\
             <div>A&#x034F;<span>B</span></div>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red_background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("div background should paint");
    let visible_green = document.pages[0]
        .lines
        .iter()
        .find(|line| line.color == Color::new(0, 128, 0))
        .expect("visible Ahem A should paint green");
    let (green_left, green_right) = rendered_line_visual_bounds(visible_green);

    assert!(
        (red_background.width - 75.0).abs() < 0.01,
        "{red_background:?}"
    );
    assert!(
        (red_background.height - 75.0).abs() < 0.01,
        "{red_background:?}"
    );
    assert!(
        (green_left - red_background.x).abs() < 0.01
            && (green_right - red_background.x - red_background.width).abs() < 0.01,
        "green glyph should cover the red square horizontally: red={red_background:?}, green_line={visible_green:?}, bounds=({green_left}, {green_right})"
    );
}
