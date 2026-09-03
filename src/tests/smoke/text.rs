use super::*;

fn fragments_share_visual_line(lines: &[crate::document::paint::text::RenderedLine]) -> bool {
    let Some(first) = lines.first() else {
        return true;
    };
    lines.iter().all(|line| (line.y() - first.y()).abs() < 3.0)
}

fn rendered_text_occurrences(document: &spindrift::Document, needle: &str) -> usize {
    document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .map(|line| line.text.matches(needle).count())
        .sum()
}

#[tokio::test]
async fn nowrap_left_float_after_prefix_reflows_the_prefix_once() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240px 120px; margin: 0 }\
         body { margin: 0; font: 16px/20px monospace }\
         div { white-space: nowrap }\
         span { float: left; width: 48px; height: 20px; background: blue }\
         </style><div>Kittie<span>Hello&nbsp;</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let blue = CssColor::new(0, 0, 255);
    assert_eq!(
        page.rects()
            .iter()
            .filter(|rect| rect.fill == Some(blue))
            .count(),
        1,
        "the selected inline float must own one paint subtree"
    );
    assert_eq!(rendered_text_occurrences(&document, "Hello"), 1);
    assert_eq!(rendered_text_occurrences(&document, "Kittie"), 1);
    let hello = page
        .lines()
        .iter()
        .find(|line| line.text.contains("Hello"))
        .expect("floating prefix should be painted");
    let kittie = page
        .lines()
        .iter()
        .find(|line| line.text.contains("Kittie"))
        .expect("in-flow prefix should be painted");
    assert!(
        hello.x() < kittie.x(),
        "a same-row left float should reflow the earlier source prefix: {hello:?}, {kittie:?}"
    );
}

#[tokio::test]
async fn overflowing_nowrap_prefix_keeps_one_text_source_line_before_lower_float() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240px 160px; margin: 0 }\
         body { margin: 0; font: 16px/20px monospace }\
         div { width: 10ch; white-space: nowrap }\
         span { float: right; width: 5ch; height: 5ch; background: blue }\
         </style><div>Some text that overflows my parent.<span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(
        rendered_text_occurrences(&document, "Some text that overflows my parent."),
        1
    );
    assert_eq!(
        page.rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        1,
        "the lower float-placement row must not duplicate its paint subtree"
    );
}

#[tokio::test]
async fn start_and_post_prefix_inline_floats_each_commit_once() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240px 160px; margin: 0 }\
         body { margin: 0; font: 16px/20px monospace }\
         div { white-space: nowrap }\
         .first { float: left; width: 40px; height: 20px; background: blue }\
         .second { float: left; width: 40px; height: 20px; background: red }\
         </style><div><span class=\"first\">Hello</span>Kittie<span class=\"second\">World</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    for color in [CssColor::new(0, 0, 255), CssColor::new(255, 0, 0)] {
        assert_eq!(
            page.rects()
                .iter()
                .filter(|rect| rect.fill == Some(color))
                .count(),
            1,
            "each inline float must create one exclusion/paint subtree"
        );
    }
    for text in ["Hello", "Kittie", "World"] {
        assert_eq!(rendered_text_occurrences(&document, text), 1, "{text}");
    }
}

#[tokio::test]
async fn segment_break_between_inline_blocks_forces_wrap_in_block_container() {
    let document = Html::from_string(
        r#"<!doctype html>
        <style>
        @page { size: 600px 140px; margin: 0 }
        body { margin: 0; font-size: 16px; line-height: 20px }
        .outer {
          display: block;
          width: 500px;
          background: purple;
          border: 1px solid green;
        }
        .half {
          display: inline-block;
          width: 50%;
          background: blue;
        }
        .half + .half {
          background: yellow;
        }
        </style>
        <div class="outer">
          <div class="half">A</div>
          <!-- White space here should take up space -->
          <div class="half">B</div>
        </div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let a = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .unwrap();

    assert!(
        b.y() < a.y() - 1.0,
        "collapsed segment-break whitespace should force B onto the next line: a={a:?}, b={b:?}"
    );
    assert!(
        (a.x() - b.x()).abs() < 1.0,
        "wrapped inline-block should restart near the same inline position: a={a:?}, b={b:?}"
    );
}

#[tokio::test]
async fn inline_block_with_only_preserved_newline_uses_forced_line_baseline() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 240px 240px; margin: 0 }\
         body { margin: 0 }\
         .wrapper { width: 200px; font-size: 0; line-height: 0; background: red }\
         .inline-block { display: inline-block; width: 100px; height: 200px; background: green }\
         </style>\
         <div class=\"wrapper\">\
           <div class=\"inline-block\">text</div>\
           <div class=\"inline-block\" style=\"white-space: pre\">&#10;</div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let red = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .max_by(|left, right| {
            (left.width() * left.height()).total_cmp(&(right.width() * right.height()))
        })
        .expect("wrapper background should paint");
    for (x, y) in [
        (red.x() + red.width() * 0.25, red.y() + red.height() * 0.25),
        (red.x() + red.width() * 0.75, red.y() + red.height() * 0.25),
        (red.x() + red.width() * 0.25, red.y() + red.height() * 0.75),
        (red.x() + red.width() * 0.75, red.y() + red.height() * 0.75),
    ] {
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(green),
            "sample at ({x}, {y}) should be green; rects={:?}",
            page.rects()
        );
    }

    let mut green_blocks = page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(green)
                && (rect.width() - red.width() / 2.0).abs() < 0.01
                && (rect.height() - red.height()).abs() < 0.01
        })
        .collect::<Vec<_>>();
    green_blocks.sort_by(|left, right| left.x().total_cmp(&right.x()));
    assert_eq!(green_blocks.len(), 2, "rects={:?}", page.rects());
    assert!(
        (green_blocks[0].y() - green_blocks[1].y()).abs() < 0.01,
        "inline-block top edges should align: {green_blocks:?}"
    );
}

#[tokio::test]
async fn zero_font_text_float_has_no_intrinsic_inline_size() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>@page { size: 240px 240px; margin: 0 } body { margin: 0 }\
         .zero { float: left; height: 200px; font-size: 0; background: red }\
         .target { width: 200px; height: 200px; background: green }</style>\
         <div class=\"zero\">Text that has no intrinsic contribution.</div><div class=\"target\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("following fixed-size block background");
    assert!(
        green.x().abs() < 0.01,
        "zero-size float displaced green: {green:?}"
    );
    assert!(
        page.rects().iter().all(|rect| {
            rect.fill != Some(CssColor::new(255, 0, 0)) || rect.width() * rect.height() == 0.0
        }),
        "zero-size text float painted a non-empty red background: {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn orthogonal_flow_uses_min_height_floored_available_inline_size() {
    let document = Html::from_string(
        "<style>\
         body > div { font-family: monospace; font-size: 20px; max-height: 4ch;\
         min-height: 8ch; color: transparent; position: relative }\
         div > div { writing-mode: vertical-rl }\
         span { background: green; display: inline-block }\
         #red { position: absolute; background: red; left: 0; writing-mode: vertical-rl;\
         z-index: -1 }</style>\
         <p>Test passes if there is a <strong>green rectangle</strong> below and \
         <strong>no red</strong>.</p>\
         <div><aside id=\"red\">0</aside><div>0 0 0 0 <span>0</span> 0 0 0</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("absolute red reference should paint behind the span");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("green inline-block reference should paint");
    // This is WPT css-writing-modes/available-size-018: the parent’s larger
    // `min-height` supplies the orthogonal child’s available inline size, so
    // the green atomic inline occupies the same column as the red reference.
    // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
    assert!(
        (green.x() - red.x()).abs() < 0.1
            && (green.y() - red.y()).abs() < 0.1
            && (green.width() - red.width()).abs() < 0.1
            && (green.height() - red.height()).abs() < 0.1,
        "orthogonal child should use the min-height-floored inline size and overlap the red reference: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn mixed_generic_bold_fragments_share_text_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 320pt 120pt; margin: 20pt } body { margin: 0 } p { margin: 0; font: 40px sans-serif }</style><p>normal <strong>bold</strong> normal</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut baselines = Vec::new();
    let mut saw_bold = false;
    for line in document.pages[0].lines() {
        if line.color != CssColor::BLACK
            || !(line.text.contains("normal") || line.text.contains("bold"))
        {
            continue;
        }
        saw_bold |= line.text.contains("bold");
        for run in &line.runs {
            baselines.push(line.y() + run.y_offset);
        }
    }

    assert!(saw_bold, "expected strong text to render as a text run");
    assert!(
        baselines.len() >= 2,
        "expected mixed inline text to produce comparable baselines"
    );
    let min = baselines.iter().copied().fold(f32::INFINITY, f32::min);
    let max = baselines.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max - min < 0.05,
        "expected mixed normal/strong text baselines to match, got {baselines:?}"
    );
}

#[tokio::test]
async fn text_shadow_paints_offset_text_without_affecting_layout_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; font-size: 12pt; line-height: 14pt } p { margin: 0; text-shadow: 4pt 2pt red }</style><p>Shadow</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines();
    let shadow_lines = lines
        .iter()
        .filter(|line| line.text == "Shadow")
        .collect::<Vec<_>>();
    assert_eq!(shadow_lines.len(), 2);
    assert!(
        shadow_lines
            .iter()
            .any(|line| line.color == CssColor::new(255, 0, 0))
    );
    assert!(
        shadow_lines
            .iter()
            .any(|line| line.color == CssColor::BLACK)
    );
    let black = shadow_lines
        .iter()
        .find(|line| line.color == CssColor::BLACK)
        .unwrap();
    let red = shadow_lines
        .iter()
        .find(|line| line.color == CssColor::new(255, 0, 0))
        .unwrap();
    assert!((red.x() - black.x() - 4.0).abs() < 0.1);
    assert!((black.y() - red.y() - 2.0).abs() < 0.1);
}

#[tokio::test]
async fn visible_text_shadow_paints_when_the_source_text_is_transparent() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; font-size: 12pt; line-height: 14pt } p { margin: 0; color: transparent; text-shadow: 4pt 2pt red }</style><p>Shadow</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let shadow = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Shadow" && line.color == CssColor::new(255, 0, 0))
        .expect("visible shadow should retain the transparent source glyph outline");
    let source = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Shadow" && !line.color.is_visible())
        .expect("transparent source text remains in the paint record");
    assert!((shadow.x() - source.x() - 4.0).abs() < 0.1);
    assert!((source.y() - shadow.y() - 2.0).abs() < 0.1);
}

#[tokio::test]
async fn blurred_text_shadow_paints_translucent_replay_without_layout_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; font-size: 12pt; line-height: 14pt } p { margin: 0; text-shadow: 2pt 1pt 4pt rgba(255, 0, 0, 0.8) }</style><p>Blur</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let blur_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "Blur")
        .collect::<Vec<_>>();
    assert!(blur_lines.len() > 2);
    assert_eq!(
        blur_lines
            .iter()
            .filter(|line| line.color == CssColor::BLACK)
            .count(),
        1
    );
    assert!(
        blur_lines
            .iter()
            .any(|line| line.color.components()[0] > 0.9 && line.color.alpha() < 0.8)
    );
}

#[tokio::test]
async fn inherited_text_shadow_currentcolor_resolves_on_painting_element() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body { margin: 0; color: red; text-shadow: 0 0 10px currentcolor } p { margin: 0; color: green; text-shadow: inherit; font: 18pt Georgia, serif }</style><p>Green shadow</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "Green shadow")
        .collect::<Vec<_>>();
    assert!(
        text_lines
            .iter()
            .any(|line| line.color == CssColor::new(0, 128, 0))
    );
    assert!(
        text_lines
            .iter()
            .any(|line| line.color.components()[0] == 0.0
                && line.color.components()[1] > 0.4
                && line.color.alpha() < 1.0)
    );
    assert!(
        !text_lines
            .iter()
            .any(|line| line.color.components()[0] > 0.9)
    );
}

#[tokio::test]
async fn text_emphasis_marks_are_painted_without_changing_base_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; font-size: 12pt; line-height: 18pt } p { margin: 0; text-emphasis: filled dot red }</style><p>AB</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.contains(&"AB"));
    assert_eq!(texts.iter().filter(|text| **text == "•").count(), 2);
    assert!(!texts.iter().any(|text| text.contains("A•B")));
}

#[tokio::test]
async fn vertical_text_emphasis_marks_follow_prepared_run_offsets() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         p { margin: 0; width: 40pt; height: 80pt; writing-mode: vertical-rl;\
             font-size: 12pt; line-height: 14pt; text-emphasis: filled sesame red }\
         </style><p>中文</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let marks = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "\u{FE45}")
        .collect::<Vec<_>>();
    assert_eq!(marks.len(), 2, "{marks:?}");
    assert!(
        (marks[0].y() - marks[1].y()).abs() > 5.0,
        "vertical emphasis marks should be positioned per prepared text unit: {marks:?}"
    );
}

#[tokio::test]
async fn page_margin_text_emphasis_uses_prepared_annotations() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 20pt;\
           @top-left { content: \"中\"; writing-mode: vertical-rl; font-size: 10pt; text-emphasis: filled sesame red } }\
         body { margin: 0 }</style>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "\u{FE45}"),
        "{:?}",
        document.pages[0].lines()
    );
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let paragraph = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    assert!(
        paragraph.width() >= 39.5,
        "min-content width should include the inline atom: {paragraph:?}"
    );
}

#[tokio::test]
async fn renders_dictionary_run_in_terms() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, dl, dt, dd { margin: 0; font-size: 10pt; line-height: 12pt } dt { display: run-in; font-weight: 700 } dt::after { content: \": \" }</style><dl><dt>alpha</dt><dd>first entry</dd></dl>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines();
    assert_eq!(lines[0].text, "alpha: ");
    assert_eq!(lines[1].text, "first entry");
    assert!(fragments_share_visual_line(&lines[..2]));
    assert!(lines[0].x() < lines[1].x());
}

#[tokio::test]
async fn run_in_before_flow_root_does_not_merge() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, h3, section { margin: 0; font-size: 10pt; line-height: 12pt } h3 { display: run-in } section { display: flow-root }</style><h3>Term</h3><section>Block</section>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "Term");
    assert_eq!(document.pages[0].lines()[1].text, "Block");
}

#[tokio::test]
async fn run_in_with_block_descendant_stays_inline_with_target() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, h3, p, b { margin: 0; font-size: 10pt; line-height: 12pt } h3 { display: run-in } b { display: block }</style><h3>Term <b>block</b> tail </h3><p>Definition</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines();
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines();
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
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "\u{2022} ");
    assert_eq!(document.pages[0].lines()[1].text, "One");
    assert_eq!(document.pages[0].lines()[2].text, "\u{2022} ");
    assert_eq!(document.pages[0].lines()[3].text, "Two");
    assert!(document.pages[0].lines()[0].x() < document.pages[0].lines()[1].x());
}

#[tokio::test]
async fn renders_basic_ordered_lists() {
    let document = Html::from_string("<ol><li>One</li><li>Two</li></ol>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "1. ");
    assert_eq!(document.pages[0].lines()[1].text, "One");
    assert_eq!(document.pages[0].lines()[2].text, "2. ");
    assert_eq!(document.pages[0].lines()[3].text, "Two");
}

#[tokio::test]
async fn supports_basic_list_style_types() {
    let document = Html::from_string(
        "<ol style=\"list-style-type: lower-alpha\"><li>One</li><li>Two</li></ol><ul style=\"list-style-type: none\"><li>Plain</li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "a. ");
    assert_eq!(document.pages[0].lines()[1].text, "One");
    assert_eq!(document.pages[0].lines()[2].text, "b. ");
    assert_eq!(document.pages[0].lines()[3].text, "Two");
    assert_eq!(document.pages[0].lines()[4].text, "Plain");
}

#[tokio::test]
async fn supports_common_builtin_list_marker_styles() {
    let document = Html::from_string(
        "<ol style=\"list-style-type: upper-roman\"><li>One</li><li>Two</li></ol><ol style=\"list-style-type: lower-roman\"><li>One</li></ol><ul style=\"list-style-type: circle\"><li>Circle</li></ul><ul style=\"list-style-type: square\"><li>Square</li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["I. ", "One"]));
    assert!(texts.windows(2).any(|pair| pair == ["II. ", "Two"]));
    assert!(texts.windows(2).any(|pair| pair == ["i. ", "One"]));
    assert!(texts.windows(2).any(|pair| pair == ["\u{25e6} ", "Circle"]));
    // The marker glyph can use a fallback font while its mandatory following
    // space remains in the primary font, so PDF text runs need not coincide
    // with the CSS marker boundary.
    assert!(
        texts.windows(2).any(|pair| pair == ["\u{25aa} ", "Square"])
            || texts
                .windows(3)
                .any(|triple| triple == ["\u{25aa}", " ", "Square"])
    );
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let text = document
        .pages
        .iter()
        .flat_map(grouped_line_texts)
        .collect::<String>();
    for marker in [
        "01.",
        "α.",
        "β.",
        "一〇、",
        "亥、",
        "癸、",
        "וט.",
        "რ.",
        "Ժ.",
        "▸",
    ] {
        assert!(text.contains(marker), "missing marker {marker:?}: {text:?}");
    }
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    for marker in [
        "ჵჰშჟთ. ",
        "20000. ",
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
async fn disclosure_counter_styles_follow_generated_content_context() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, p, ul, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: item 0 } p { counter-increment: item } p::before { content: counter(item, disclosure-closed) \" \" counters(item, \"/\", disclosure-open) \" \" } ul { list-style-position: inside } li::marker { content: counter(list-item, disclosure-closed) \" \" }</style><p dir=rtl>RTL</p><p style=\"writing-mode: vertical-lr\">Vertical</p><ul style=\"writing-mode: vertical-rl; direction: rtl\"><li>Marker</ul>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .map(|line| line.text.as_str())
        .collect::<String>();
    // Line text is recorded in logical source order. UAX #9 reorders the RTL
    // disclosure controls visually, so assert the generated counter glyphs
    // in their respective content contexts rather than their paint order.
    assert!(
        text.contains("RTL") && text.contains('▾') && text.contains('◂'),
        "{text}"
    );
    assert!(text.contains("Vertical") && text.contains('▸'), "{text}");
    assert!(text.contains("Marker") && text.contains('▴'), "{text}");
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    let contains = |needle: &[&str]| texts.windows(needle.len()).any(|pair| pair == needle);
    assert!(contains(&["☝ ", "One"]));
    assert!(contains(&["101) ", "Five"]));
    assert!(contains(&["[101] ", "Bracketed"]));
    assert!(contains(&["[(11)] ", "Negative"]));
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    let text = texts.join("");
    assert!(text.contains("1. Alpha"), "{text}");
    assert!(text.contains("2. Beta"), "{text}");
}

#[tokio::test]
async fn generated_counter_names_preserve_custom_ident_case() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: Item } p { counter-increment: Item } p::before { content: counter(Item) }</style><p>Alpha</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.join("").contains("1Alpha"), "{texts:?}");
}

#[tokio::test]
async fn generated_pseudo_counter_increment_applies_before_content() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: c } p::before { counter-increment: c; content: counter(c) }</style><p>Alpha</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.join("").contains("1Alpha"), "{texts:?}");
}

#[tokio::test]
async fn empty_generated_pseudo_content_still_applies_counter_effects() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: c } p::before { content: \"\"; counter-increment: c } p::after { content: counter(c) }</style><p>Alpha</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("1. One"), "{text}");
    assert!(text.contains("1.1 A"), "{text}");
    assert!(text.contains("1.2 B"), "{text}");
    assert!(text.contains("2. Two"), "{text}");
    assert!(text.contains("2.1 C"), "{text}");
}

#[tokio::test]
async fn counter_reset_that_shadows_ancestor_does_not_replace_ancestor_counter() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body, div, span { margin: 0; font-size: 10pt; line-height: 12pt } #test { counter-reset: c } #test span { counter-increment: c } #test span::before { content: counter(c, decimal-leading-zero) } .local { counter-reset: c 98 }</style><div id=\"test\"><span></span> <span></span> <span class=\"local\"></span> <span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("01 02 99 100"), "{text}");
}

#[tokio::test]
async fn nested_counter_reset_replaces_a_previous_sibling_and_remains_in_scope() {
    let document = Html::from_string(
        "<style>@page { size: 320pt 120pt; margin: 10pt } body, p, div, span { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: c } p, #test span { counter-increment: c } #test span:first-child { counter-reset: c } #test span::before { content: counters(c, \".\", decimal) \" \" } </style><p></p><div id=\"test\"><span></span><span></span><span style=\"counter-reset: c 98\"></span><span></span><span style=\"counter-reset: c 999998\"></span><span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(
        text.contains("1.1 1.2 1.99 1.100 1.999999 1.1000000"),
        "{text}"
    );
}

#[tokio::test]
async fn nested_hebrew_counters_fall_back_after_a_sibling_reset() {
    let document = Html::from_string(
        "<style>@page { size: 320pt 120pt; margin: 10pt } body, p, div, span { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: c } p, #test span { counter-increment: c } #test span:first-child { counter-reset: c } #test span::before { content: counters(c, \".\", hebrew) \" \" } </style><p></p><div id=\"test\"><span></span><span style=\"counter-reset: c 999998\"></span><span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    // The line text is in visual order under the surrounding LTR paragraph,
    // so the decimal fallback follows the outer Hebrew counter here.
    assert!(text.contains("א.א"), "{text}");
    assert!(text.contains("999999.א"), "{text}");
    assert!(text.contains("1000000.א"), "{text}");
}

#[tokio::test]
async fn generated_counter_content_uses_exotic_counter_styles() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body, div { margin: 0; font-size: 10pt; line-height: 12pt } body { counter-reset: chapter 99 section 0 } div { counter-increment: chapter section } div::before { content: counter(chapter, trad-chinese-informal) \" \" counters(section, \".\", ethiopic-numeric) \" \" }</style><div>Title</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines()
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
         <div>01 02 03 04 05 06 07 08 09 10 11 12 99 100 101</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    // A counter reset on a sibling remains in scope for following siblings,
    // so their increments continue from the reset value.
    // <https://www.w3.org/TR/css-lists-3/#nested-counters-and-scope>
    let expected = "01 02 03 04 05 06 07 08 09 10 11 12 99 100 101";
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.join("").contains("NOTE Body"), "{texts:?}");
}

#[tokio::test]
async fn generated_image_content_renders_inline_atom() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQMAAADO7O3JAAAAA1BMVEUAgACc+aWRAAAACklEQVQI12NgAAAAAgAB4iG8MwAAAABJRU5ErkJggg==";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 80pt; margin: 10pt }} body, p {{ margin: 0; font-size: 10pt; line-height: 12pt }} p::before {{ content: url({png}) \" \"; width: 8pt; height: 6pt }}</style><p>Icon</p>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    let image = &document.pages[0].images()[0];
    // A generated pseudo's authored box remains 8×6pt, but its image payload
    // uses the image's intrinsic CSS size (2×1 CSS pixels = 1.5×0.75pt).
    // <https://www.w3.org/TR/css-content-3/#content-property>
    assert!((image.width() - 1.5).abs() < 0.01);
    assert!((image.height() - 0.75).abs() < 0.01);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == " Icon")
    );
}

#[tokio::test]
async fn generated_gradient_content_renders_inline_atom() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 80pt; margin: 10pt }\
         body, p { margin: 0; font-size: 10pt; line-height: 12pt }\
         p::before { content: linear-gradient(in srgb, red, blue) \" \"; width: 8pt; height: 6pt }\
         p::after { content: radial-gradient(in srgb circle, red, blue); width: 6pt; height: 6pt }\
         </style><p>Grad</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0].images();
    assert_eq!(images.len(), 0);
    assert!(
        document.pages[0]
            .gradient_patterns()
            .iter()
            .any(|gradient| (gradient.width() - 8.0).abs() < 0.01
                && (gradient.height() - 6.0).abs() < 0.01)
    );
    assert!(
        document.pages[0]
            .gradient_patterns()
            .iter()
            .any(|gradient| {
                (gradient.width() - 6.0).abs() < 0.01 && (gradient.height() - 6.0).abs() < 0.01
            })
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == " Grad")
    );
}

#[tokio::test]
async fn invalid_generated_image_is_skipped_without_suppressing_text() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p::before { content: url(missing-generated-content-image.png) \"Fallback \" }</style><p>Body</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 0);
    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.join("").contains("Fallback Body"), "{texts:?}");
}

#[tokio::test]
async fn element_content_replacement_suppresses_children_and_pseudos() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 80pt; margin: 10pt }} body, p {{ margin: 0; font-size: 10pt; line-height: 12pt }} p {{ content: url({png}) / \"Replacement\"; width: 8pt; height: 6pt }} p::before {{ content: \"Before\" }} p::after {{ content: \"After\" }}</style><p>Hidden</p>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(
        document.pages[0].images()[0].alt_text.as_deref(),
        Some("Replacement")
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .all(|line| { !matches!(line.text.as_str(), "Hidden" | "Before" | "After") })
    );
}

#[tokio::test]
async fn element_content_none_preserves_children() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p { content: none }</style><p>Visible</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Visible")
    );
}

#[tokio::test]
async fn element_content_contents_splices_children_once() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 80pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } p { content: \"A\" contents \"B\" contents }</style><p>Text</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let first_y = document.pages[0].lines()[0].y();
    let texts = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.y() > first_y - 0.1)
        .map(|line| line.text.as_str())
        .collect::<String>();
    let reference = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.y() < first_y - 0.1)
        .map(|line| line.text.as_str())
        .collect::<String>();
    assert_eq!(
        texts,
        "One “two ‘three 『four』’”",
        "lines={:?}",
        document.pages[0].lines()
    );
    assert_eq!(
        reference,
        "One “two ‘three 『four』’”",
        "lines={:?}",
        document.pages[0].lines()
    );
}

#[tokio::test]
async fn auto_quotes_cover_greek_and_farsi_nesting() {
    let document = Html::from_string(
        "<style>@page { size: 280pt 90pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt }</style><p lang=\"el\"><q>Greek <q>inner</q></q></p><p lang=\"fa\"><q>Farsi <q>inner</q></q></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();
    let explicit_text = explicit.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();
    let none_text = none.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("Chapter"));
    assert!(text.contains("2"));
    assert!(text.contains("..."), "{text}");
}

#[tokio::test]
async fn html_input_and_textarea_suppress_generated_content_pseudos() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body { margin: 0; font: 10pt/12pt monospace } input, textarea { display: block; width: 100pt; height: 12pt } input::before { content: \"input-generated\" } textarea::after { content: \"textarea-generated\" } div::before { content: \"ordinary-generated\" }</style><input><textarea></textarea><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(!text.contains("input-generated"), "{text}");
    assert!(!text.contains("textarea-generated"), "{text}");
    assert!(text.contains("ordinary-generated"), "{text}");
}

#[tokio::test]
async fn page_margin_leader_content_uses_sequence_owned_resolution() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 90pt; margin: 20pt; @top-center { content: \"Chapter\" leader(dotted) \"2\"; font-size: 10pt; line-height: 10pt; width: 160pt } }\
         body { margin: 0 }\
         </style><p></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("Chapter"), "{text}");
    assert!(text.contains("2"), "{text}");
    assert!(text.contains("..."), "{text}");
}

#[tokio::test]
async fn page_margin_forced_breaks_use_plaintext_alignment_without_controls() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 120pt; margin: 20pt;\
           @top-center { content: \"אב\\A abc\"; white-space: pre-line; unicode-bidi: plaintext; text-align: start; font-size: 10pt; line-height: 10pt; width: 150pt }\
         }\
         body { margin: 0 }\
         </style><p></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let margin_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text.contains('א') || line.text.contains('ב') || line.text == "abc")
        .collect::<Vec<_>>();
    assert_eq!(margin_lines.len(), 2, "{:?}", document.pages[0].lines());
    assert!(
        margin_lines
            .iter()
            .all(|line| !line.text.chars().any(char::is_control)),
        "{margin_lines:?}"
    );
    let hebrew = margin_lines
        .iter()
        .find(|line| line.text.contains('א') || line.text.contains('ב'))
        .expect("expected Hebrew plaintext line");
    let latin = margin_lines
        .iter()
        .find(|line| line.text == "abc")
        .expect("expected Latin plaintext line");
    assert!(
        hebrew.x() > latin.x() + 50.0,
        "hebrew={hebrew:?}, latin={latin:?}"
    );
}

#[tokio::test]
async fn inline_block_leader_content_matches_normal_inline_resolution() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 100pt; margin: 10pt }\
         body { margin: 0; font: 10pt/12pt monospace }\
         span { display: inline-block; width: 120pt; content: \"Chapter\" leader(dotted) \"2\" }\
         </style><span>Ignored</span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("Chapter"), "{text}");
    assert!(text.contains("2"), "{text}");
    assert!(text.contains("..."), "{text}");
}

#[tokio::test]
async fn rtl_and_vertical_leaders_use_prepared_text_placement() {
    let rtl = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body { margin: 0; font: 10pt/12pt monospace } p { margin: 0; width: 160pt; direction: rtl } p::after { content: leader(dotted) \"2\" }</style><p>Chapter</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let vertical = Html::from_string(
        "<style>@page { size: 160pt 180pt; margin: 10pt } body { margin: 0; font: 10pt/12pt monospace } p { margin: 0; writing-mode: vertical-rl; width: 80pt; height: 140pt } p::after { content: leader(dotted) \"2\" }</style><p>章</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rtl_text = rtl.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    let vertical_text = vertical.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("");

    assert!(rtl_text.contains("..."), "{rtl_text}");
    assert!(vertical_text.contains("..."), "{vertical_text}");
    assert!(
        vertical.pages[0]
            .lines()
            .iter()
            .flat_map(|line| line.runs.iter())
            .any(|run| {
                run.text.contains('.')
                    && run.text_matrix
                        == crate::document::paint::text::RenderedTextMatrix::ROTATE_CW
            }),
        "{:?}",
        vertical.pages[0].lines()
    );
}

#[tokio::test]
async fn generated_image_alt_text_is_captured() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 80pt; margin: 10pt }} body, p {{ margin: 0; font-size: 10pt; line-height: 12pt }} p::before {{ content: url({png}) / \"Generated\"; width: 8pt; height: 6pt }}</style><p>Icon</p>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    assert_eq!(
        document.pages[0].images()[0].alt_text.as_deref(),
        Some("Generated")
    );
}

#[tokio::test]
async fn supports_string_list_style_type() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } p { display: list-item; list-style-position: inside; list-style-type: \"Note: \" }</style><p>Alpha</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "Note: ");
    assert_eq!(document.pages[0].lines()[1].text.trim_start(), "Alpha");
    assert!((document.pages[0].lines()[0].y() - document.pages[0].lines()[1].y()).abs() < 0.01);
    assert!(document.pages[0].lines()[0].x() < document.pages[0].lines()[1].x());
}

#[tokio::test]
async fn supports_string_list_style_type_in_shorthand() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body, ol, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ol, ul { list-style: inside \"# \"; }</style><ol><li>Alpha</li></ol>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0].lines();
    let first_visual_line = lines
        .iter()
        .take_while(|line| (line.y() - lines[0].y()).abs() < 0.01)
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0].lines();
    assert!(lines.len() > 1);
    assert_eq!(lines[0].text, "\u{2022} ");
    assert!(lines[1].text.starts_with("Alpha"));
    assert!(!lines[2].text.starts_with("\u{2022}"));
    assert!((lines[1].x() - lines[2].x()).abs() < 0.01);
    assert!(lines[0].x() < lines[1].x());
}

#[tokio::test]
async fn inside_list_markers_participate_in_first_line() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } ol { margin: 0; padding-left: 20pt; list-style-position: inside } li { margin: 0 }</style><ol><li>Alpha beta gamma delta epsilon zeta eta theta</li></ol>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0].lines();
    assert!(lines.len() > 1);
    assert_eq!(lines[0].text, "1.");
    assert!(lines[1].text.trim_start().starts_with("Alpha"));
    assert!((lines[0].y() - lines[1].y()).abs() < 0.01);
    assert!(lines[0].x() < lines[1].x());
    assert!(!lines[2].text.starts_with("1."));
}

#[tokio::test]
async fn generated_inside_list_item_markers_contribute_to_float_widths() {
    let document = Html::from_string(
        r#"<style>
            @page { size: 180pt 120pt; margin: 10pt }
            body, ol, li { margin: 0; padding: 0; font-size: 12pt; line-height: 16pt }
            ol { float: left; width: 42pt; list-style-position: inside }
            li { display: block }
            li::before, li::after { content: "\200B"; display: list-item; float: left }
            span { display: inline-block; width: 12pt; height: 16pt }
            .before li::after, .after li::before { content: normal }
        </style>
        <ol class="before"><li><span></span></li><li><span></span></li></ol>
        <ol class="after"><li><span></span></li><li><span></span></li></ol>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let markers = document.pages[0]
        .lines()
        .iter()
        .filter(|line| matches!(line.text.trim(), "1." | "2."))
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 4, "{:#?}", document.pages[0].lines());
    assert!(markers.iter().any(|line| line.text.trim() == "1."));
    assert!(markers.iter().any(|line| line.text.trim() == "2."));
}

#[tokio::test]
async fn outside_marker_uses_empty_styled_inline_strut_baseline() {
    let document = Html::from_string(
        r#"<style>
            @page { size: 140pt 100pt; margin: 10pt }
            body, ol, li { margin: 0; font-size: 16pt; line-height: 30pt }
            ol { padding-left: 32pt }
            li { list-style-type: lower-alpha }
            li.reference { font: italic bold 24pt/30pt sans-serif }
            li::marker, span { font: italic bold 24pt/30pt sans-serif }
        </style>
        <ol><li><span></span></li><li class="reference"></li></ol>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let markers = document.pages[0]
        .lines()
        .iter()
        .filter(|line| matches!(line.text.trim(), "a." | "b."))
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 2, "{:#?}", document.pages[0].lines());
    assert!(
        (30.0..33.0).contains(&(markers[0].y() - markers[1].y())),
        "empty styled inline did not establish its line strut: {markers:?}"
    );
}

#[tokio::test]
async fn empty_inside_list_items_keep_their_marker_line_boxes() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body, ol, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ol { list-style-position: inside }</style><ol>\n  <li>\n  <li>\n  <li>\n</ol>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let markers = document.pages[0]
        .lines()
        .iter()
        .filter(|line| matches!(line.text.as_str(), "1." | "2." | "3."))
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 3);
    assert!(
        markers[0].y() > markers[1].y() && markers[1].y() > markers[2].y(),
        "{markers:?}"
    );
}

#[tokio::test]
async fn marker_only_inside_items_advance_their_parent_block_flow() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body, ol, li, p { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ol { list-style-position: inside }</style><ol><li><li><li></ol><ol><li><li></ol><p>after</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let markers = page
        .lines()
        .iter()
        .filter(|line| matches!(line.text.as_str(), "1." | "2." | "3."))
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 5, "{:#?}", page.lines());
    assert!(
        markers.windows(2).all(|pair| pair[0].y() > pair[1].y()),
        "marker-only list items must remain distinct block-flow lines: {markers:?}"
    );
    let after = page
        .lines()
        .iter()
        .find(|line| line.text.contains("after"))
        .expect("following block should be painted");
    assert!(
        markers.last().unwrap().y() > after.y(),
        "following block overlapped the marker-only lists: {markers:?}, {after:?}"
    );
}

#[tokio::test]
async fn custom_marker_only_inside_items_advance_their_parent_block_flow() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body, ol, li, p { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ol { list-style-position: inside; list-style-type: chapter } @counter-style chapter { system: fixed 1; symbols: \"A\" \"B\"; prefix: \"Appendix \"; suffix: \"! \"; }</style><ol><li><li></ol><p>after</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let markers = page
        .lines()
        .iter()
        .filter(|line| line.text.contains("Appendix "))
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 2, "{:#?}", page.lines());
    assert!(markers[0].text.contains('A') && markers[1].text.contains('B'));
    assert!(markers[0].y() > markers[1].y(), "{markers:?}");
    let after = page
        .lines()
        .iter()
        .find(|line| line.text.contains("after"))
        .expect("following block should be painted");
    assert!(markers[1].y() > after.y(), "{markers:?}, {after:?}");
}

#[tokio::test]
async fn empty_inside_marker_lines_use_the_marker_line_height() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 160pt; margin: 10pt } body, ol, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ol { list-style-position: inside } li::marker { font-size: 24pt; line-height: 24pt }</style><ol><li><li></ol>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let markers = document.pages[0]
        .lines()
        .iter()
        .filter(|line| matches!(line.text.as_str(), "1." | "2."))
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 2);
    assert!(
        markers[0].y() - markers[1].y() > 20.0,
        "marker line height did not advance layout: {markers:?}"
    );
}

#[tokio::test]
async fn empty_inside_marker_lines_paginate() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 48pt; margin: 10pt } body, ol, li { margin: 0; padding: 0; font-size: 10pt; line-height: 16pt } ol { list-style-position: inside }</style><ol><li><li><li></ol>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages.len() >= 2, "{:#?}", document.pages);
    let markers = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .filter(|line| matches!(line.text.as_str(), "1." | "2." | "3."))
        .count();
    assert_eq!(markers, 3);
}

#[tokio::test]
async fn empty_inside_marker_representation_does_not_paint_marker_text() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body, ul, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ul { list-style-position: inside; list-style-type: \"\" }</style><ul><li></ul>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .all(|line| line.text.is_empty())
    );
}

#[tokio::test]
async fn empty_inside_marker_representation_does_not_advance_parent_flow() {
    let style = "<style>@page { size: 140pt 120pt; margin: 10pt } body, ul, li, p { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ul { list-style-position: inside; list-style-type: \"\" }</style>";
    let with_empty_marker = Html::from_string(format!("{style}<ul><li></ul><p>after</p>"))
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let without_list = Html::from_string(format!("{style}<p>after</p>"))
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let after_y = |document: &spindrift::Document| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.contains("after"))
            .expect("following block should be painted")
            .y()
    };
    assert!(
        (after_y(&with_empty_marker) - after_y(&without_list)).abs() < 0.01,
        "an empty marker representation must not create a line"
    );
}

#[tokio::test]
async fn html_type_hints_use_author_redefined_counter_styles() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body, ol, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ol { list-style-position: inside } @counter-style lower-roman { system: cyclic; symbols: r; suffix: \" \" } @counter-style upper-alpha { system: cyclic; symbols: A; suffix: \" \" }</style><ol type=\"i\"><li>one</li></ol><ol><li type=\"A\">two</li></ol>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<String>();
    assert!(text.contains('r'), "{text}");
    assert!(text.contains('A'), "{text}");
}

#[tokio::test]
async fn inside_generated_marker_segment_breaks_use_shared_whitespace_context() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 100pt; margin: 10pt }\
         body, ul, li { margin: 0; padding: 0; font-size: 10pt; line-height: 10pt }\
         ul { list-style-position: inside }\
         li::marker { content: \"中文\\A\"; white-space: normal }\
         </style><ul><li>english</li></ul>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["中文 english"]);
}

#[tokio::test]
async fn inside_image_marker_keeps_following_space_extractable() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 180pt 100pt; margin: 10pt }}\
         body, ul, li {{ margin: 0; padding: 0; font-size: 10pt; line-height: 10pt }}\
         ul {{ list-style-position: inside; list-style-image: url({png}) }}\
         </style><ul><li>Item</li></ul>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec![" Item"]);
}

#[tokio::test]
async fn rtl_outside_list_markers_paint_on_inline_start_right_side() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ul { margin: 0; padding: 0; direction: rtl } li { margin: 0 }</style><ul><li>Alpha</li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let marker = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains('\u{2022}'))
        .expect("expected marker");
    let content = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Alpha")
        .expect("expected content");
    assert!(
        marker.x() + rendered_line_advance(marker) > content.x() + rendered_line_advance(content)
    );
}

#[tokio::test]
async fn html_dir_rtl_outside_list_markers_match_css_direction_rtl() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ul { margin: 0; padding: 0 } li { margin: 0 }</style><ul dir=\"rtl\"><li>Alpha</li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let marker = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains('\u{2022}'))
        .expect("expected marker");
    let content = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Alpha")
        .expect("expected content");
    assert!(
        marker.x() + rendered_line_advance(marker) > content.x() + rendered_line_advance(content)
    );
}

#[tokio::test]
async fn rtl_outside_image_markers_paint_on_inline_start_right_side() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 100pt; margin: 10pt }} body {{ margin: 0; font-size: 10pt; line-height: 12pt }} ul {{ margin: 0; padding: 0; direction: rtl; list-style-image: url({png}) }} li {{ margin: 0 }}</style><ul><li>Alpha</li></ul>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    let image = document.pages[0]
        .images()
        .first()
        .expect("expected marker image");
    let content = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Alpha")
        .expect("expected content");
    assert!(image.x() > content.x() + rendered_line_advance(content));
}

#[tokio::test]
async fn rtl_outside_list_marker_only_paints_on_first_wrapped_line() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } ul { margin: 0; padding: 0; direction: rtl } li { margin: 0; width: 70pt }</style><ul><li>Alpha beta gamma delta epsilon zeta eta theta</li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let markers = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text.contains('\u{2022}'))
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 1);
    let content_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| !line.text.contains('\u{2022}'))
        .collect::<Vec<_>>();
    assert!(content_lines.len() > 1);
    let first_right = content_lines[0].x() + rendered_line_advance(content_lines[0]);
    let second_right = content_lines[1].x() + rendered_line_advance(content_lines[1]);
    assert!((first_right - second_right).abs() < 0.01);
    assert!(
        markers[0].x() + rendered_line_advance(markers[0])
            > content_lines[0].x() + rendered_line_advance(content_lines[0])
    );
}

#[tokio::test]
async fn outside_list_marker_only_paints_on_first_fragmented_sequence_line() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 44pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } ul { margin: 0; padding: 0 } li { margin: 0; width: 70pt }</style><ul><li>Alpha beta gamma delta epsilon zeta eta theta iota kappa</li></ul>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let markers = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter())
        .filter(|line| line.text.contains('\u{2022}'));
    assert_eq!(markers.count(), 1);
    let pages_with_content = document
        .pages
        .iter()
        .filter(|page| {
            page.lines()
                .iter()
                .any(|line| !line.text.contains('\u{2022}'))
        })
        .count();
    assert!(pages_with_content > 1);
}

#[tokio::test]
async fn rtl_inside_list_markers_share_first_line_with_generated_before() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } ol { margin: 0; padding: 0; direction: rtl; list-style-position: inside } li { margin: 0 } li::before { content: \"Before\" }</style><ol><li>Alpha</li></ol>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains('1'), "{lines:?}");
    assert!(lines[0].contains("Before"), "{lines:?}");
    assert!(lines[0].contains("Alpha"), "{lines:?}");
}

#[tokio::test]
async fn marker_side_controls_mixed_direction_outside_marker_side() {
    let match_self = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, ul, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ul { direction: ltr } li { list-style-type: decimal } .rtl { direction: rtl }</style><ul><li>Left</li><li class=\"rtl\">Right</li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let left_marker = match_self.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("1."))
        .expect("expected first marker");
    let left_content = match_self.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Left")
        .expect("expected first content");
    let right_marker = match_self.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains('2'))
        .expect("expected second marker");
    let right_content = match_self.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Right")
        .expect("expected second content");
    assert!(left_marker.x() < left_content.x());
    assert!(
        right_marker.x() + rendered_line_advance(right_marker)
            > right_content.x() + rendered_line_advance(right_content)
    );

    let match_parent = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body, ul, li { margin: 0; padding: 0; font-size: 10pt; line-height: 12pt } ul { direction: ltr; marker-side: match-parent } li { list-style-type: decimal } .rtl { direction: rtl }</style><ul><li>Left</li><li class=\"rtl\">Right</li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let second_marker = match_parent.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains('2'))
        .expect("expected second marker");
    let second_content = match_parent.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Right")
        .expect("expected second content");
    assert!(second_marker.x() < second_content.x());
}

#[tokio::test]
async fn capitalize_tailors_dutch_ij_digraphs() {
    let document = Html::from_string(
        "<p lang=\"nl\" style=\"text-transform: capitalize; margin: 0\">ijsland ijssel</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "IJsland IJssel")
    );
}

#[tokio::test]
async fn text_justify_inter_character_distributes_between_letters() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 } p { margin: 0; width: 100pt; font-size: 10pt; line-height: 12pt; text-align: justify; text-align-last: justify; text-justify: inter-character }</style><p>XX</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let x_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "X")
        .collect::<Vec<_>>();
    assert_eq!(x_lines.len(), 2);
    assert!(x_lines[1].x() - x_lines[0].x() > rendered_line_advance(x_lines[0]) + 50.0);
}

#[tokio::test]
async fn text_justify_inter_character_preserves_arabic_joining_sequences() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 } p { margin: 0; width: 100pt; font-size: 10pt; line-height: 12pt; text-align: justify; text-align-last: justify; text-justify: inter-character }</style><p>سلام</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "سلام" || line.text.chars().rev().collect::<String>() == "سلام")
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);
    assert!(rendered_line_advance(lines[0]) < 80.0);
}

#[tokio::test]
async fn text_justify_auto_treats_bidi_controls_as_zero_width_cjk_boundaries() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 } p { margin: 0; width: 3.9em; font-size: 12pt; line-height: 14pt; text-align: justify; text-align-last: justify }</style><p>東\u{2066}京都東京\u{2069}都東京都</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let visible = document.pages[0]
        .lines()
        .iter()
        .map(|line| (line.text.as_str(), line.x(), line.y()))
        .collect::<Vec<_>>();
    assert!(
        visible
            .iter()
            .all(|(text, _, _)| !text.contains('\u{2066}') && !text.contains('\u{2069}')),
        "bidi controls must not reach painted/extracted text: {visible:?}"
    );
    let mut lines = std::collections::BTreeMap::<i32, Vec<&str>>::new();
    for (text, _, y) in &visible {
        lines.entry(y.round() as i32).or_default().push(*text);
    }
    assert_eq!(
        lines.values().cloned().collect::<Vec<_>>(),
        vec![
            vec!["東", "京", "都"],
            vec!["東", "京", "都"],
            vec!["東", "京", "都"]
        ],
        "controls must not change the selected CJK line boundaries: {visible:?}"
    );
}

#[tokio::test]
async fn text_justify_inter_character_treats_consecutive_atoms_as_one_unit() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 }\
         p { margin: 0; width: 100pt; font: 20pt/20pt sans-serif; text-align-last: justify; text-justify: inter-character }\
         .atom { display: inline-block; width: 20pt; height: 20pt; background: blue; vertical-align: top }</style>\
         <p>X<span class=\"atom\"></span><span class=\"atom\"></span>X</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut x_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "X")
        .collect::<Vec<_>>();
    x_lines.sort_by(|left, right| left.x().total_cmp(&right.x()));
    assert_eq!(x_lines.len(), 2, "{:?}", document.pages[0].lines());
    assert!(
        x_lines[1].x() - x_lines[0].x() > 70.0,
        "inter-character justification should spread text around the atomic run: {x_lines:?}"
    );

    let mut atoms = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();
    atoms.sort_by(|left, right| left.x().total_cmp(&right.x()));
    assert_eq!(atoms.len(), 2, "{atoms:?}");
    assert!(
        (atoms[1].x() - atoms[0].x() - atoms[0].width()).abs() < 0.5,
        "consecutive atomic inlines should stay adjacent as one typographic unit: {atoms:?}"
    );
}

#[tokio::test]
async fn supports_display_list_item_on_arbitrary_elements() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } div { display: list-item; margin-left: 20pt; list-style-type: decimal }</style><div>One</div><div>Two</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "1. ");
    assert_eq!(document.pages[0].lines()[1].text, "One");
    assert_eq!(document.pages[0].lines()[2].text, "2. ");
    assert_eq!(document.pages[0].lines()[3].text, "Two");
}

#[tokio::test]
async fn supports_inline_display_list_item_markers() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 90pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } span { display: inline list-item; list-style-position: inside; list-style-type: decimal }</style><span>Inline one</span><span>Inline two</span>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0].lines();
    assert_eq!(lines[0].text, "1.");
    assert_eq!(lines[1].text.trim_start(), "Inline one");
    assert!((lines[0].y() - lines[1].y()).abs() < 0.01);
    assert!(lines[0].x() < lines[1].x());
    assert_eq!(lines[2].text, "2.");
    assert_eq!(lines[3].text.trim_start(), "Inline two");
    assert!((lines[2].y() - lines[3].y()).abs() < 0.01);
    assert!(lines[2].x() < lines[3].x());
}

#[tokio::test]
async fn supports_html_ordered_list_start_reversed_and_value() {
    let document = Html::from_string(
        "<ol start=\"3\"><li>Three</li><li value=\"7\">Seven</li><li>Eight</li></ol><ol reversed><li>Two</li><li>One</li></ol>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["3. ", "Three"]));
    assert!(texts.windows(2).any(|pair| pair == ["7. ", "Seven"]));
    assert!(texts.windows(2).any(|pair| pair == ["8. ", "Eight"]));
    assert!(texts.windows(2).any(|pair| pair == ["2. ", "Two"]));
    assert!(texts.windows(2).any(|pair| pair == ["1. ", "One"]));
}

#[tokio::test]
async fn nested_list_item_counters_use_unified_counter_stack() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 140pt; margin: 10pt } body, ol { margin: 0; font-size: 10pt; line-height: 12pt } ol { padding-left: 18pt; list-style-position: inside } li::marker { content: counters(list-item, \".\") \". \" }</style><ol><li>One<ol><li>Inner</li></ol></li><li>Two</li></ol>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(texts.windows(2).any(|pair| pair == ["2. ", "Two"]));
    assert!(texts.windows(2).any(|pair| pair == ["4. ", "Four"]));
    assert!(texts.windows(2).any(|pair| pair == ["5. ", "Five"]));
    assert!(texts.windows(2).any(|pair| pair == ["9. ", "Nine"]));
    assert!(texts.windows(2).any(|pair| pair == ["10. ", "Ten"]));
    assert!(texts.windows(2).any(|pair| pair == ["1. ", "One"]));
    assert!(texts.windows(2).any(|pair| pair == ["2. ", "Two"]));
}

#[tokio::test]
async fn marker_pseudo_element_styles_marker_without_affecting_content() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } li::marker { color: red; font-size: 14pt }</style><ul><li>Item</li></ul>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let marker = &document.pages[0].lines()[0];
    let content = &document.pages[0].lines()[1];
    assert_eq!(marker.text, "\u{2022} ");
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0].lines();
    assert_eq!(lines[0].text, "a) ");
    assert_eq!(lines[1].text, "One");
    assert!((lines[0].y() - lines[1].y()).abs() < 0.01);
    assert!(lines[0].x() < lines[1].x());
    assert_eq!(lines[2].text, "b) ");
    assert_eq!(lines[3].text, "Two");
    assert!((lines[2].y() - lines[3].y()).abs() < 0.01);
    assert!(lines[2].x() < lines[3].x());
}

#[tokio::test]
async fn supports_outside_list_style_image_markers() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 120pt 80pt; margin: 10pt }} body {{ margin: 0; font-size: 10pt; line-height: 12pt }} ul {{ margin: 0; padding-left: 18pt; list-style-image: url({png}); list-style-type: decimal }} li {{ margin: 0 }}</style><ul><li>Item</li></ul>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .all(|line| line.text != "1.")
    );
    let image = &document.pages[0].images()[0];
    let item = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim_start() == "Item")
        .unwrap();
    assert!(image.x() < item.x());
}

#[tokio::test]
async fn supports_inside_list_style_image_markers() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 120pt 80pt; margin: 10pt }} body {{ margin: 0; font-size: 10pt; line-height: 12pt }} ul {{ margin: 0; padding-left: 0; list-style-position: inside; list-style-image: url({png}) }} li {{ margin: 0 }}</style><ul><li>Item</li></ul>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    let image = &document.pages[0].images()[0];
    let item = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim_start() == "Item")
        .unwrap();
    assert!(image.x() < item.x());
    assert!((image.y() - item.y()).abs() < 20.0);
}

#[tokio::test]
async fn image_set_list_markers_scale_intrinsic_size_for_inside_and_outside_positions() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQMAAADO7O3JAAAAA1BMVEUAgACc+aWRAAAACklEQVQI12NgAAAAAgAB4iG8MwAAAABJRU5ErkJggg==";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 120pt 100pt; margin: 10pt }}\
         body, ul, li {{ margin: 0; font-size: 10pt; line-height: 12pt }}\
         .outside {{ padding-left: 18pt }}\
         .inside {{ padding-left: 0; list-style-position: inside }}\
         .full {{ list-style-image: url({png}) }}\
         .half {{ list-style-image: image-set(url({png}) 0.5x) }}\
         </style>\
         <ul class=\"outside full\"><li></li></ul>\
         <ul class=\"outside half\"><li></li></ul>\
         <ul class=\"inside full\"><li></li></ul>\
         <ul class=\"inside half\"><li></li></ul>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0].images();
    assert_eq!(images.len(), 4);
    for [full, half] in [[&images[0], &images[1]], [&images[2], &images[3]]] {
        // The 0.5x selected candidate doubles both used marker dimensions,
        // regardless of inside/outside marker participation.
        assert!(
            (half.width() - full.width() * 2.0).abs() < 0.01,
            "{images:?}"
        );
        assert!(
            (half.height() - full.height() * 2.0).abs() < 0.01,
            "{images:?}"
        );
    }
}

#[tokio::test]
async fn marker_content_overrides_list_style_image() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 120pt 80pt; margin: 10pt }} body {{ margin: 0; font-size: 10pt; line-height: 12pt }} ul {{ margin: 0; padding-left: 18pt; list-style-image: url({png}) }} li {{ margin: 0 }} li::marker {{ content: \"x \" }}</style><ul><li>Item</li></ul>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 0);
    assert_eq!(document.pages[0].lines()[0].text, "x ");
    assert_eq!(document.pages[0].lines()[1].text, "Item");
}

#[tokio::test]
async fn auto_height_floated_lists_use_replay_equivalent_height() {
    let items = (1..=9)
        .map(|index| format!("<li><span>item {index}</span></li>"))
        .collect::<String>();
    let lists = ["ordinary", "string", "explicit", "before", "after"]
        .into_iter()
        .map(|class| format!("<ol class=\"{class}\">{items}</ol>"))
        .collect::<String>();
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 900px 900px; margin: 0 }}\
         body {{ margin: 0; font-size: 16px; line-height: 20px }}\
         ol {{ float: left; width: 100px; margin: 0 5px 0 0; padding: 0; list-style-position: inside }}\
         li {{ margin: 0; padding: 0 }}\
         span {{ display: inline-block }}\
         .ordinary {{ background: red }}\
         .string {{ background: green; list-style-type: none }}\
         .string li::marker {{ content: \"string \" }}\
         .explicit {{ background: blue }}\
         .explicit li::marker {{ content: \"marker \" }}\
         .before {{ background: yellow }}\
         .before li {{ list-style-type: none }}\
         .before li::before {{ content: \"before\"; display: list-item }}\
         .after {{ background: purple }}\
         .after li {{ list-style-type: none }}\
         .after li::after {{ content: \"after\"; display: list-item }}\
         </style>{lists}"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        1,
        "all floated lists should share one page"
    );
    let page = &document.pages[0];
    let float_tops = [
        CssColor::new(255, 0, 0),
        CssColor::new(0, 128, 0),
        CssColor::new(0, 0, 255),
        CssColor::new(255, 255, 0),
        CssColor::new(128, 0, 128),
    ]
    .into_iter()
    .map(|color| {
        page.rects()
            .iter()
            .filter(|rect| rect.fill == Some(color))
            .map(|rect| rect.y() + rect.height())
            .max_by(f32::total_cmp)
            .expect("float background should paint")
    })
    .collect::<Vec<_>>();
    assert!(
        float_tops
            .iter()
            .all(|top| (*top - float_tops[0]).abs() < 0.01),
        "floats should share their block-start: {float_tops:?}"
    );
}

#[tokio::test]
async fn supports_text_alignment() {
    let options = RenderOptions::default();
    let document = Html::from_string(
        "<style>body { margin: 0 }</style><p style=\"margin: 0; width: 100pt; text-align: right; font-size: 10pt\">Hi</p>",
    )
    .render(&options).await
    .unwrap();

    let aligned_offset =
        document.pages[0].lines()[0].x() - crate::layout::PageMargins::DEFAULT.left();
    let expected_offset = 100.0 - rendered_line_advance(&document.pages[0].lines()[0]);
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
    .render(&options).await
    .unwrap();

    let aligned_offset =
        document.pages[0].lines()[0].x() - crate::layout::PageMargins::DEFAULT.left();
    let expected_offset = 100.0 - rendered_line_advance(&document.pages[0].lines()[0]);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines();
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
async fn preserved_tabs_from_no_space_font_are_advance_only() {
    let document = Html::from_string(
        r#"<style>
            @page { size: 180pt 120pt; margin: 10pt }
            @font-face {
                font-family: NoSpace;
                src: url("tests/resources/fonts/CanvasTest-nospace.ttf");
            }
            @font-face {
                font-family: WithSpace;
                src: url("tests/resources/fonts/noto-sans-v8-latin-regular.woff");
            }
            p {
                margin: 0;
                font: 40px/1 NoSpace, WithSpace;
                white-space: pre;
                tab-size: 8;
            }
            span { font-family: WithSpace; }
        </style><p><span>&nbsp;</span>&#9;E</p><p><span>&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;</span>E</p>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let tabbed = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "\u{a0}\tE")
        .expect("the preserved-tab line is rendered");
    assert!(
        tabbed
            .runs
            .iter()
            .filter_map(|run| run.glyphs.as_ref())
            .flat_map(|glyphs| glyphs.iter())
            .all(|glyph| glyph.painted_id() != Some(0)),
        "a preserved tab must not emit a `.notdef` glyph"
    );
    let equivalent_spaces = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}E")
        .unwrap_or_else(|| {
            panic!(
                "the equivalent-space line is rendered: {:?}",
                document.pages[0].lines()
            )
        });
    let e_x = |line: &crate::document::paint::text::RenderedLine| {
        line.x()
            + line
                .runs
                .iter()
                .find(|run| {
                    run.glyphs
                        .as_ref()
                        .is_some_and(|glyphs| glyphs.iter().any(|glyph| glyph.unicode == "E"))
                })
                .expect("painted E run")
                .x_offset
    };
    assert!(
        (e_x(tabbed) - e_x(equivalent_spaces)).abs() < 0.01,
        "the tab must align with eight block-font spaces: tabbed={tabbed:?}, equivalent={equivalent_spaces:?}"
    );
}

#[tokio::test]
async fn preserved_tabs_restart_at_each_forced_line() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body, p { margin: 0; font: 10pt monospace; line-height: 12pt; white-space: pre }</style><p>123456\n   abc\tZ</p><p>123456789</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines();
    let tabbed = lines
        .iter()
        .find(|line| line.text == "   abc\tZ")
        .expect("preserved second line");
    let control = lines
        .iter()
        .find(|line| line.text == "123456789")
        .expect("nine-column control line");
    assert!(
        (rendered_line_advance(tabbed) - rendered_line_advance(control)).abs() < 0.01,
        "tab must restart at the forced line break: tabbed={tabbed:?}, control={control:?}"
    );
}

#[tokio::test]
async fn preserved_tabs_do_not_change_normal_line_metrics() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 180pt; margin: 10pt } body, p { margin: 0; font: 32px monospace } .pre { white-space: pre }</style><p class=\"pre\">Lorem\n   sit\tamet</p><p>Lorem<br>&nbsp;&nbsp;&nbsp;sit&nbsp;&nbsp;amet</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines();
    let actual_first = lines
        .iter()
        .find(|line| line.text == "Lorem")
        .expect("preserved line's first row");
    let actual_second = lines
        .iter()
        .find(|line| line.text == "   sit\tamet")
        .expect("preserved line with tab");
    let reference_first = lines
        .iter()
        .rfind(|line| line.text == "Lorem")
        .expect("reference line's first row");
    let reference_second = lines
        .iter()
        .find(|line| line.text == "\u{a0}\u{a0}\u{a0}sit\u{a0}\u{a0}amet")
        .expect("reference second row");
    let actual_advance = actual_first.y() - actual_second.y();
    let reference_advance = reference_first.y() - reference_second.y();
    assert!(
        (actual_advance - reference_advance).abs() < 0.01,
        "a preserved tab must not change normal-line metrics: actual={actual_advance}, reference={reference_advance}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let two = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(10, 20, 30)))
        .expect("expected tab-size:2 inline-block background");
    let four = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(30, 20, 10)))
        .expect("expected tab-size:4 inline-block background");

    assert!(
        four.width() > two.width() + 5.0,
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let hang = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == ")MMMM")
        .unwrap();
    let reference = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "MMMM")
        .unwrap();
    let hang_measured_x = hang.x() + rendered_line_advance(hang) - rendered_line_advance(reference);
    assert!(
        (hang_measured_x - reference.x()).abs() < 0.5,
        "expected hanging line and same-width reference to share measured alignment, got {} vs {}",
        hang_measured_x,
        reference.x()
    );
}

#[tokio::test]
async fn hanging_punctuation_first_places_opening_quote_outside_line_measure() {
    let normal = Html::from_string(
        "<style>body{margin:0}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt}</style>\
         <p>\"Hello</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let hanging = Html::from_string(
        "<style>body{margin:0}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;hanging-punctuation:first}</style>\
         <p>\"Hello</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        hanging.pages[0].lines()[0].x() < normal.pages[0].lines()[0].x() - 1.0,
        "expected first hanging quote to move into the line-start margin"
    );
}

#[tokio::test]
async fn hanging_punctuation_first_places_rtl_opening_punctuation_outside_line_measure() {
    let normal = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt}</style>\
         <p>(Hello</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let hanging = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;hanging-punctuation:first}</style>\
         <p>(Hello</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let blocked = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;hanging-punctuation:first}span{border-right:1em solid blue}</style>\
         <p><span>(</span>Hello</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let blocked_reference = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt}span{border-right:1em solid blue}</style>\
         <p><span>(</span>Hello</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        hanging.pages[0].lines()[0].x() > normal.pages[0].lines()[0].x() + 1.0,
        "expected RTL first hanging punctuation to move into the line-start margin: normal={}, hanging={}",
        normal.pages[0].lines()[0].x(),
        hanging.pages[0].lines()[0].x(),
    );
    assert!(
        (blocked.pages[0].lines()[0].x() - blocked_reference.pages[0].lines()[0].x()).abs() < 1.0,
        "expected nonzero RTL inline-start border to block first hanging punctuation: reference={}, blocked={}, blocked text={:?}",
        blocked_reference.pages[0].lines()[0].x(),
        blocked.pages[0].lines()[0].x(),
        blocked.pages[0]
            .lines()
            .iter()
            .map(|line| (line.text.as_str(), line.x()))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn text_indent_offsets_rtl_inline_start_edge() {
    let normal = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt}</style>\
         <p>Hello</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let negative_indent = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;text-indent:-1em}</style>\
         <p>Hello</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let positive_indent = Html::from_string(
        "<style>body{margin:0;direction:rtl}p{margin:0;width:120pt;font-family:monospace;font-size:10pt;line-height:12pt;text-indent:1em}</style>\
         <p>Hello</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        negative_indent.pages[0].lines()[0].x() > normal.pages[0].lines()[0].x() + 1.0,
        "expected negative RTL text-indent to move the inline-start edge outward: normal={}, indented={}",
        normal.pages[0].lines()[0].x(),
        negative_indent.pages[0].lines()[0].x(),
    );
    assert!(
        positive_indent.pages[0].lines()[0].x() < normal.pages[0].lines()[0].x() - 1.0,
        "expected positive RTL text-indent to move the inline-start edge inward: normal={}, indented={}",
        normal.pages[0].lines()[0].x(),
        positive_indent.pages[0].lines()[0].x(),
    );
}

#[tokio::test]
async fn text_indent_on_blank_rtl_left_aligned_line_does_not_indent_following_line() {
    let document = Html::from_string(
        "<style>@page{size:400pt 400pt;margin:0} body{margin:0}</style>\
         <div style=\"text-align:left; direction:rtl; text-indent:300pt; line-height:100pt; width:200pt; background:blue\">\
           <br>\
           <div style=\"vertical-align:bottom; display:inline-block; width:100pt; height:100pt; background:hotpink\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("expected container background");
    let hotpink = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 105, 180)))
        .expect("expected inline-block background");

    assert!(
        (hotpink.x() - blue.x()).abs() < 0.01,
        "hotpink square should align to the physical left edge of the blue square: blue={blue:?}, hotpink={hotpink:?}"
    );
    assert!(
        (hotpink.y() - blue.y()).abs() < 0.01,
        "hotpink square should align to the physical bottom edge of the blue square: blue={blue:?}, hotpink={hotpink:?}"
    );
}

#[tokio::test]
async fn fixed_width_rtl_block_ignores_overconstrained_left_margin() {
    let both_margins = Html::from_string(
        "<style>body{margin:0;direction:rtl;font-family:monospace;font-size:10pt;line-height:12pt}div{width:100pt;margin:10pt}</style>\
         <div>Hello</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let right_margin = Html::from_string(
        "<style>body{margin:0;direction:rtl;font-family:monospace;font-size:10pt;line-height:12pt}div{width:100pt;margin-right:10pt}</style>\
         <div>Hello</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        (both_margins.pages[0].lines()[0].x() - right_margin.pages[0].lines()[0].x()).abs() < 1.0,
        "expected fixed-width RTL block to ignore over-constrained left margin: both={}, right={}",
        both_margins.pages[0].lines()[0].x(),
        right_margin.pages[0].lines()[0].x(),
    );
}

#[tokio::test]
async fn hanging_punctuation_force_end_excludes_terminal_stop_from_alignment() {
    let normal = Html::from_string(
        "<style>body{margin:0}p{margin:0;width:120pt;text-align:right;font-family:monospace;font-size:10pt;line-height:12pt}</style>\
         <p>Hello。</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let hanging = Html::from_string(
        "<style>body{margin:0}p{margin:0;width:120pt;text-align:right;font-family:monospace;font-size:10pt;line-height:12pt;hanging-punctuation:force-end}</style>\
         <p>Hello。</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        hanging.pages[0].lines()[0].x() > normal.pages[0].lines()[0].x() + 1.0,
        "expected force-end punctuation to hang past the right line edge: normal={}, hanging={}",
        normal.pages[0].lines()[0].x(),
        hanging.pages[0].lines()[0].x(),
    );
}

#[tokio::test]
async fn hanging_punctuation_last_is_blocked_by_inline_end_border() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } \
         body { margin: 0; direction: rtl; font-family: monospace; font-size: 10pt; line-height: 12pt } \
         div { margin: 0; white-space: nowrap; text-align: start; width: 4ch } \
         .hanging { hanging-punctuation: last } \
         .blocked span { border-left: 1em solid blue }</style>\
         <div class=\"hanging blocked\">MMMM<span>)</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let first_y = document.pages[0].lines()[0].y();
    let blocked_parts = document.pages[0]
        .lines()
        .iter()
        .filter(|line| (line.y() - first_y).abs() < 0.1)
        .collect::<Vec<_>>();
    let blocked_text = blocked_parts
        .iter()
        .map(|line| line.text.as_str())
        .collect::<String>();
    assert_eq!(
        blocked_text,
        ")MMMM",
        "lines={:?}",
        document.pages[0].lines()
    );
    let border = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("expected inline-end border");
    assert!(
        blocked_parts[0].x() >= border.x() + border.width() - 0.5,
        "expected blocked punctuation text to start after its inline-end border, got text {} border {}..{}",
        blocked_parts[0].x(),
        border.x(),
        border.x() + border.width()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let blocked = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } \
         body { margin: 0; font-family: monospace; font-size: 10pt; line-height: 12pt } \
         div { margin: 0; width: 5ch; hanging-punctuation: allow-end } \
         span { border-left: 1em solid black }</style>\
         <div>12 34,<span></span> 1234,</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(allowed.pages[0].lines()[0].text, "12 34,");
    assert_eq!(blocked.pages[0].lines()[0].text, "12");
}

#[tokio::test]
async fn wpt_hanging_punctuation_allow_end_inline_boundaries() {
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut rendered_lines = document.pages[0].lines().to_vec();
    rendered_lines.sort_by(|left, right| {
        right
            .y()
            .total_cmp(&left.y())
            .then_with(|| left.x().total_cmp(&right.x()))
    });
    let mut lines: Vec<(f32, String)> = Vec::new();
    for line in &rendered_lines {
        if let Some((baseline, text)) = lines.last_mut()
            && (line.y() - *baseline).abs() < 0.01
        {
            text.push_str(&line.text);
            continue;
        }
        lines.push((line.y(), line.text.clone()));
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
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "<style>@page {{ size: 500pt 300pt; margin: 0 }} body {{ margin: 0; font-family: Ahem; color: green }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .hang {{ margin: 1em; border: 1px solid black }}</style>\
         <div style=\"float:left\" class=\"hang\"><div style=\"margin: 0 -1em\">(Hang test)</div></div>\
         <div style=\"clear:both\"><div class=\"hang\" style=\"width:10em; text-align:justify;\"><span style=\"margin:0 -1em\">(This should hang.<br>(This should also hang.)</span></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let target_lines = target.pages[0]
        .lines()
        .iter()
        .map(|line| (line.text.as_str(), line.x(), rendered_line_advance(line)))
        .collect::<Vec<_>>();
    let reference_lines = reference.pages[0]
        .lines()
        .iter()
        .map(|line| (line.text.as_str(), line.x(), rendered_line_advance(line)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["まだよく", "ています。", "しかし特"]);

    let green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 255, 0)))
        .collect::<Vec<_>>();
    let mut line_backgrounds = Vec::<(f32, f32, f32, f32)>::new();
    for rect in green_rects {
        let min_x = rect.x();
        let max_x = min_x + rect.width();
        if let Some((_, line_min_x, line_max_x, covered_width)) = line_backgrounds
            .iter_mut()
            .find(|(y, ..)| (*y - rect.y()).abs() < 0.1)
        {
            *line_min_x = line_min_x.min(min_x);
            *line_max_x = line_max_x.max(max_x);
            *covered_width += rect.width();
        } else {
            line_backgrounds.push((rect.y(), min_x, max_x, rect.width()));
        }
    }
    line_backgrounds.sort_by(|left, right| right.0.total_cmp(&left.0));
    assert_eq!(line_backgrounds.len(), 3, "{line_backgrounds:?}");
    for (_, min_x, max_x, covered_width) in &line_backgrounds {
        assert!(
            ((max_x - min_x) - covered_width).abs() < 0.1,
            "background fragments on one line must meet without a gap: {line_backgrounds:?}"
        );
    }
    assert!(
        line_backgrounds[1].2 - line_backgrounds[1].1
            > line_backgrounds[0].2 - line_backgrounds[0].1,
        "second line background should include the hung punctuation: {line_backgrounds:?}"
    );
}

#[tokio::test]
async fn wpt_hanging_punctuation_uses_punctuation_font_size() {
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "<style>@page {{ size: 500pt 200pt; margin: 0 }} body {{ margin: 0; font-family: Ahem; color: green }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .hang {{ white-space: nowrap; margin: 1em; float: left }}</style>\
         <div class=\"hang\" style=\"font-size:32px\"><span style=\"font-size:16px; margin-left:-1em\">(</span>1234</div>\
         <div class=\"hang\" style=\"font-size:32px\">1234<span style=\"font-size:16px; margin-right:-1em\">)</span></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    for (target_line, reference_line) in target.pages[0]
        .lines()
        .iter()
        .zip(reference.pages[0].lines())
    {
        assert_eq!(target_line.text, reference_line.text);
        assert!(
            (target_line.x() - reference_line.x()).abs() < 1.0,
            "{} target x {}, reference x {}",
            target_line.text,
            target_line.x(),
            reference_line.x()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let first = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "one two")
        .unwrap();
    let three = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "three")
        .unwrap();

    assert!(rendered_line_advance(first) > 40.0);
    assert!((three.x() - first.x()).abs() < 0.5);
    assert!(three.y() < first.y());
}

#[tokio::test]
async fn text_indent_justify_uses_stable_line_widths() {
    let text = "This is a long piece of text that will wrap to multiple lines.  ".repeat(12);
    let document = Html::from_string(format!(
        "<style>@page {{ size: 700pt 240pt; margin: 20pt }}\
         body {{ margin: 0 }}\
         p {{ margin: 0; width: 600pt; font-size: 12pt; line-height: 14pt;\
              text-indent: 100px; text-align: justify }}\
         span {{ background: yellow }}</style><p><span>{text}</span></p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text.contains("long piece") || line.text.contains("multiple lines"))
        .collect::<Vec<_>>();
    assert!(lines.len() > 3, "{:?}", document.pages[0].lines());

    let paragraph_left = 20.0;
    let paragraph_width = 600.0;
    let first_indent = 75.0;
    assert!((lines[0].x() - (paragraph_left + first_indent)).abs() < 0.1);
    assert!(
        (rendered_line_advance(lines[0]) - (paragraph_width - first_indent)).abs() < 1.0,
        "first justified line should fill the indented measure: x={}, width={}",
        lines[0].x(),
        rendered_line_advance(lines[0])
    );
    for line in lines.iter().skip(1).take(lines.len().saturating_sub(2)) {
        assert!((line.x() - paragraph_left).abs() < 0.1, "{line:?}");
        assert!(
            (rendered_line_advance(line) - paragraph_width).abs() < 1.0,
            "wrapped justified line should fill the paragraph measure: x={}, width={}",
            line.x(),
            rendered_line_advance(line)
        );
    }
}

#[tokio::test]
async fn text_justify_none_disables_inter_word_distribution() {
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 360pt 160pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ margin: 0; width: 290px; color: orange; font: 24px/24px Ahem; text-align: justify; text-justify: none }}</style>\
         <div class=\"test\">XXX XXX XXX XXX XXX XXX XXX XXX</div>",
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    let space_advances = document.pages[0]
        .lines()
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
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 220pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ width: 7em; font: 15pt/15pt Ahem; white-space: pre-wrap; text-align: justify }}</style>\
         <div class=\"test\"><span>XX XX </span><span>XXX</span></div>",
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    let mut groups = lines_grouped_by_y(document.pages[0].lines());
    groups.sort_by(|left, right| right[0].y().total_cmp(&left[0].y()));
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
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 220pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ width: 7em; font: 15pt/15pt Ahem; white-space: pre-wrap; text-align: justify; direction: rtl }}</style>\
         <div class=\"test\"><span>XX XX </span><span>XXX</span></div>",
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    let mut groups = lines_grouped_by_y(document.pages[0].lines());
    groups.sort_by(|left, right| right[0].y().total_cmp(&left[0].y()));
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

/// CSS Text keeps a soft-wrapped `pre-wrap` space in the paint stream but
/// excludes its advance when positioning the remaining line content. This
/// must survive the final visual-width reconciliation performed after bidi
/// reordering and shaping.
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
#[tokio::test]
async fn pre_wrap_hanging_spaces_do_not_affect_final_text_alignment() {
    let ahem = format!(
        "file://{}/tests/fixtures/wpt/css/css-fonts/Ahem.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let cases = [
        ("ltr", "normal", "center"),
        ("ltr", "normal", "right"),
        ("rtl", "normal", "left"),
        ("rtl", "normal", "right"),
        ("rtl", "normal", "start"),
        ("rtl", "normal", "end"),
        ("rtl", "normal", "center"),
        ("rtl", "bidi-override", "left"),
        ("rtl", "bidi-override", "right"),
        ("rtl", "bidi-override", "start"),
        ("rtl", "bidi-override", "end"),
        ("rtl", "bidi-override", "center"),
    ];

    for (direction, unicode_bidi, text_align) in cases {
        let stylesheet = format!(
            "<style>@page {{ size: 400px 160px; margin: 0 }}\
             @font-face {{ font-family: Ahem; src: url({ahem}) }}\
             body {{ margin: 0 }}\
             .test {{ margin: 0; width: 15ch; font: 20px/20px Ahem;\
                      direction: {direction}; unicode-bidi: {unicode_bidi};\
                      text-align: {text_align} }}</style>"
        );
        let target = Html::from_string(format!(
            "{stylesheet}<div class=\"test\" style=\"white-space:pre-wrap\">one two three four five\nsix seven eight nine</div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();
        let reference = Html::from_string(format!(
            "{stylesheet}<div class=\"test\">one two three<br>four five<br>six seven eight<br>nine</div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let mut target_lines = visual_line_groups(target.pages[0].lines());
        let mut reference_lines = visual_line_groups(reference.pages[0].lines());
        target_lines.sort_by(|left, right| right[0].y().total_cmp(&left[0].y()));
        reference_lines.sort_by(|left, right| right[0].y().total_cmp(&left[0].y()));
        assert_eq!(
            target_lines.len(),
            4,
            "target {direction} {unicode_bidi} {text_align}"
        );
        assert_eq!(
            reference_lines.len(),
            4,
            "reference {direction} {unicode_bidi} {text_align}"
        );

        for line_index in [0, 2] {
            let target_bounds = rendered_non_whitespace_group_bounds(&target_lines[line_index]);
            let reference_bounds =
                rendered_non_whitespace_group_bounds(&reference_lines[line_index]);
            assert!(
                (target_bounds.0 - reference_bounds.0).abs() < 0.5
                    && (target_bounds.1 - reference_bounds.1).abs() < 0.5,
                "line {line_index} must ignore its hanging space for {direction} \
                 {unicode_bidi} {text_align}: target={target_bounds:?}, \
                 reference={reference_bounds:?}"
            );
        }
    }
}

#[tokio::test]
async fn wpt_text_justify_hangs_pre_wrap_trailing_space_inside_split_fragment() {
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 220pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ width: 7em; font: 15pt/15pt Ahem; white-space: pre-wrap; text-align: justify }}</style>\
         <div class=\"test\"><span>XX XX XXX</span></div>",
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    let mut groups = lines_grouped_by_y(document.pages[0].lines());
    groups.sort_by(|left, right| right[0].y().total_cmp(&left[0].y()));
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
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let mut groups = visual_line_groups(document.pages[0].lines());
    groups.sort_by(|left, right| right[0].y().total_cmp(&left[0].y()));
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
        document.pages[0]
            .lines()
            .iter()
            .all(|line| hidden_separators
                .iter()
                .all(|separator| !line.text.contains(*separator))),
        "transparent word separators should not emit visible text lines: {:?}",
        document.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn local_wpt_text_justify_word_separators_hide_separator_glyphs_if_available() {
    let wpt_root = std::path::Path::new("/Users/lee/oss/spindrift-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }
    let html = std::fs::read_to_string(
        wpt_root.join("css/css-text/text-justify/text-justify-word-separators.html"),
    )
    .unwrap();
    let document = Html::from_string(html)
        .with_base_path(wpt_root)
        .unwrap()
        .render(&RenderOptions::default())
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
        document.pages[0]
            .lines()
            .iter()
            .all(|line| hidden_separators
                .iter()
                .all(|separator| !line.text.contains(separator))),
        "transparent WPT word separators should not emit visible text lines: {:?}",
        document.pages[0]
            .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let mut line_groups = visual_line_groups(document.pages[0].lines());
    line_groups.sort_by(|left, right| right[0].y().total_cmp(&left[0].y()));
    assert_eq!(line_groups.len(), 2);
    let first_line = &line_groups[0];
    let last_line = &line_groups[1];
    assert_eq!(first_line.len(), 1);
    assert_eq!(last_line.len(), 1);

    let first_offset = first_line[0].x() - 10.0;
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
async fn text_align_last_start_overrides_center_on_final_ltr_and_rtl_lines() {
    let document = Html::from_string(
        "<style>@page { size: 300pt 240pt; margin: 10pt }\
         body { margin: 0 }\
         p { margin: 0; width: 180pt; font: 10pt/10pt sans-serif; text-align: center; text-align-last: start }</style>\
         <p>Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron</p>\
         <p dir=\"rtl\">אבג דהו זחט יכל מנס עפצ קרש תאב גדל המז וטנ יכס למע נסע פצק</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut line_groups = visual_line_groups(document.pages[0].lines());
    line_groups.sort_by(|left, right| right[0].y().total_cmp(&left[0].y()));
    let ltr_groups = line_groups
        .iter()
        .filter(|group| {
            group.iter().any(|line| {
                line.text
                    .chars()
                    .any(|character| character.is_ascii_alphabetic())
            })
        })
        .collect::<Vec<_>>();
    let rtl_groups = line_groups
        .iter()
        .filter(|group| {
            group.iter().any(|line| {
                line.text
                    .chars()
                    .any(|character| ('\u{0590}'..='\u{05ff}').contains(&character))
            })
        })
        .collect::<Vec<_>>();
    assert!(ltr_groups.len() >= 2, "{line_groups:?}");
    assert!(rtl_groups.len() >= 2, "{line_groups:?}");

    let content_left = 10.0;
    let content_right = 190.0;
    let group_bounds = |group: &&Vec<&crate::document::paint::text::RenderedLine>| {
        let left = group
            .iter()
            .map(|line| rendered_line_visual_bounds(line).0)
            .fold(f32::INFINITY, f32::min);
        let right = group
            .iter()
            .map(|line| rendered_line_visual_bounds(line).1)
            .fold(f32::NEG_INFINITY, f32::max);
        (left, right)
    };
    let has_centered_non_final_line =
        |groups: &[&Vec<&crate::document::paint::text::RenderedLine>]| {
            groups[..groups.len() - 1].iter().any(|group| {
                let (left, right) = group_bounds(group);
                let start_space = left - content_left;
                let end_space = content_right - right;
                start_space > 2.0 && end_space > 2.0 && (start_space - end_space).abs() < 2.0
            })
        };

    assert!(
        has_centered_non_final_line(&ltr_groups),
        "expected an ordinary LTR line to remain centered: {ltr_groups:?}"
    );
    assert!(
        has_centered_non_final_line(&rtl_groups),
        "expected an ordinary RTL line to remain centered: {rtl_groups:?}"
    );

    let (ltr_last_left, _) = group_bounds(ltr_groups.last().unwrap());
    let (_, rtl_last_right) = group_bounds(rtl_groups.last().unwrap());
    assert!(
        (ltr_last_left - content_left).abs() < 1.0,
        "text-align-last:start should align the final LTR line to physical left: {ltr_groups:?}"
    );
    assert!(
        (rtl_last_right - content_right).abs() < 1.0,
        "text-align-last:start should align the final RTL line to physical right: {rtl_groups:?}"
    );
}

#[tokio::test]
async fn supports_text_align_justify_all_on_final_line() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } p { margin: 0; width: 45pt; text-align: justify-all; font-size: 10pt; line-height: 10pt }</style><p>aa aa aa aa aa</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let line_groups = visual_line_groups(document.pages[0].lines());
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
    let ahem = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wpt/css/css-fonts/Ahem.ttf"
    );
    let target = Html::from_string(format!(
        "<style>@page {{ size: 500pt 200pt; margin: 0 }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test, .ref {{ border: 1px solid orange; margin: 20px; width: 300px; color: orange; font: 25px/1 Ahem }}\
         .test {{ text-align: end; }} .ref {{ text-align: left; }}</style>\
         <div class=\"test\" dir=\"rtl\">TESTI</div><div class=\"ref\">REFER</div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let test = target.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "TESTI")
        .unwrap();
    let reference = target.pages[0]
        .lines()
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
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut groups = visual_line_groups(document.pages[0].lines());
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
        .map(|line| line.x() + rendered_line_advance(line))
        .fold(f32::NEG_INFINITY, f32::max);
    let previous_right = groups[0]
        .iter()
        .map(|line| line.x() + rendered_line_advance(line))
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (last_right - previous_right).abs() < 1.0,
        "last RTL justify line should be start/right aligned: last={last_right}, previous={previous_right}"
    );
}

#[tokio::test]
async fn wpt_text_align_justify_all_ltr_justifies_final_line_in_rtl_parent() {
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut groups = visual_line_groups(document.pages[0].lines());
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
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 220pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         .test {{ width: 120pt; font: 10pt/10pt Ahem; text-align: justify; text-align-last: justify }}</style>\
         <div class=\"test\"><span>XXXX</span><span> </span><span>XXXX</span></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines()
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
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let group = visual_line_groups(document.pages[0].lines())
        .into_iter()
        .find(|group| group.iter().any(|line| line.text.contains("XXXX")))
        .expect("expected mixed inline text group");
    assert_eq!(group.len(), 2, "{group:?}");
    let mut ordered = group.clone();
    ordered.sort_by(|left, right| left.x().total_cmp(&right.x()));
    let first = ordered[0];
    let second = ordered[1];
    assert!(
        second.x() - first.x() > 90.0 && rendered_fragment_group_span(&group) > 125.0,
        "justification should shift the atom and later text across the line: {group:?}"
    );
}

#[tokio::test]
async fn justified_text_and_styled_spans_share_inline_paint_adjustment() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 140pt; margin: 10pt } body { margin: 0 }\
         .test { margin: 0; width: 170pt; font: 12pt/14pt sans-serif; text-align: justify; text-align-last: justify }</style>\
         <p class=\"test\">Alpha Beta</p><p class=\"test\"><span>Alpha</span><span> </span><span>Beta</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "Alpha Beta")
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2, "{lines:?}");
    let first_span = {
        let (left, right) = rendered_line_visual_bounds(lines[0]);
        right - left
    };
    let second_span = {
        let (left, right) = rendered_line_visual_bounds(lines[1]);
        right - left
    };
    assert!(first_span > 160.0, "{first_span}");
    assert!(
        (first_span - second_span).abs() < 0.5,
        "{first_span} vs {second_span}"
    );
}

#[tokio::test]
async fn generated_inline_text_uses_shared_justified_paint_adjustment() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body { margin: 0 }\
         .test { margin: 0; width: 170pt; font: 12pt/14pt sans-serif; text-align: justify; text-align-last: justify }\
         .test::before { content: \"Alpha Beta\" }</style><p class=\"test\"></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Alpha Beta")
        .expect("generated inline text should paint");
    let (left, right) = rendered_line_visual_bounds(line);
    assert!(right - left > 160.0, "{line:?}");
}

#[tokio::test]
async fn page_margin_text_uses_shared_justified_paint_adjustment() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 20pt;\
           @top-left { content: \"Alpha Beta\"; width: 170pt; font: 12pt/14pt sans-serif; text-align: justify; text-align-last: justify } }\
         body { margin: 0 }</style><p></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Alpha Beta")
        .expect("page-margin generated inline text should paint");
    let (left, right) = rendered_line_visual_bounds(line);
    assert!(right - left > 160.0, "{line:?}");
}

#[tokio::test]
async fn supports_word_spacing_in_text_measurement_and_painting() {
    let normal = Html::from_string(
        "<style>@page { size: 140pt 90pt; margin: 10pt } p { margin: 0; font-family: monospace; font-size: 10pt; line-height: 10pt }</style><p>A A</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let spaced = Html::from_string(
        "<style>@page { size: 140pt 90pt; margin: 10pt } p { margin: 0; font-family: monospace; font-size: 10pt; line-height: 10pt; word-spacing: 20pt }</style><p>A A</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let normal_width = rendered_line_advance(&normal.pages[0].lines()[0]);
    let spaced_width = rendered_line_advance(&spaced.pages[0].lines()[0]);

    assert!(
        spaced_width - normal_width > 19.0,
        "expected word-spacing to add about 20pt, normal={normal_width}, spaced={spaced_width}"
    );
}

#[tokio::test]
async fn percentage_word_spacing_matches_em_across_current_font_sizes() {
    let document = Html::from_string(
        "<style>@page { size: 500pt 140pt; margin: 10pt }\
         body { margin: 0 }\
         div { font-size: 20px; line-height: 1; font-family: sans-serif }\
         small { font-size: 50% }</style>\
         <div style=\"word-spacing: 1em\">A A A <small style=\"word-spacing: 1em\">A A A</small></div>\
         <div style=\"word-spacing: 100%\">A A A <small>A A A</small></div>\
         <div style=\"word-spacing: calc(0.5em + 50%)\">A A A <small style=\"word-spacing: calc(0.5em + 50%)\">A A A</small></div>\
         <div style=\"word-spacing: 100%; font-size: 0.1em\"><div style=\"font-size: 20px\">A A A <small>A A A</small></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines();
    assert_eq!(lines.len(), 8, "{lines:?}");
    let groups = lines
        .chunks(2)
        .map(|chunk| chunk.iter().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let spans = groups
        .iter()
        .map(|group| rendered_fragment_group_span(group))
        .collect::<Vec<_>>();
    let reference = spans[0];
    for span in &spans[1..] {
        assert!(
            (span - reference).abs() < 1.0,
            "percentage word-spacing should match em spacing across font sizes: spans={spans:?}, groups={groups:?}"
        );
    }
}

#[tokio::test]
async fn wpt_text_autospace_inserts_ideograph_alpha_spacing() {
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let off_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.y() > 90.0)
        .collect::<Vec<_>>();
    let on_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.y() <= 90.0)
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
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut groups = lines_grouped_by_y(document.pages[0].lines());
    groups.sort_by(|left, right| right[0].y().total_cmp(&left[0].y()));
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
    let mplus = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/mplus-1p-regular.woff";
    if !std::path::Path::new(mplus).exists() {
        return;
    }
    let document = Html::from_string(format!(
        "<style>@page {{ size: 400pt 160pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: mplus; src: url(file://{mplus}) }}\
         p {{ margin: 0; font: 32px/1 mplus; letter-spacing: 20px }}</style><p>office</p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let glyph_text = document.pages[0].lines()[0]
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
    let ahem = format!(
        "{}/tests/fixtures/wpt/css/css-fonts/Ahem.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let document = Html::from_string(format!(
        "<style>@page {{ size: 300pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         p {{ margin: 0; font: 25px/1 Ahem; letter-spacing: 25px; white-space: pre-wrap }}\
         .tail {{ border-left: 1px solid orange }}</style>\
         <p><span></span>A<span></span><span></span>D<span class=tail></span></p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let letter_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| {
            line.text
                .chars()
                .all(|character| matches!(character, 'A' | 'D'))
        })
        .collect::<Vec<_>>();
    assert!(
        !letter_lines.is_empty(),
        "expected text-empty inline content to preserve A/D text: {:?}",
        document.pages[0].lines()
    );
    let width = rendered_fragment_group_span(&letter_lines);
    let line_start = letter_lines
        .iter()
        .map(|line| line.x())
        .fold(f32::INFINITY, f32::min);
    assert!(
        (width - 56.25).abs() < 1.0,
        "expected Ahem A+D plus one inter-letter tracking advance, got {width}"
    );
    let trailing_edge = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 165, 0)))
        .filter(|rect| rect.width() <= 2.0 && rect.height() > 10.0)
        .max_by(|left, right| left.x().total_cmp(&right.x()))
        .expect("the trailing empty inline should paint its border");
    assert!(
        (trailing_edge.x() - (line_start + width)).abs() < 1.5,
        "trailing empty-inline border must follow D's base advance: lines={letter_lines:?}, edge={trailing_edge:?}"
    );
}

#[tokio::test]
async fn letter_spacing_preserves_each_typographic_unit_base_advance() {
    let ahem = format!(
        "{}/tests/fixtures/wpt/css/css-fonts/Ahem.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let document = Html::from_string(format!(
        "<style>@page {{ size: 300pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         p {{ margin: 0; font: 20px/1 Ahem; letter-spacing: 20px }}\
         .wrap {{ width: 100px; word-break: break-all }}\
         .emph {{ text-emphasis: dot }}</style>\
         <p>1 2</p><p class=wrap>123456789</p><p class=emph>ABC</p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines().iter().collect::<Vec<_>>();
    let first_row = lines
        .iter()
        .copied()
        .filter(|line| (line.y() - lines[0].y()).abs() < 0.1)
        .collect::<Vec<_>>();
    let width = rendered_fragment_group_span(&first_row);
    assert!(
        (width - 75.0).abs() < 1.0,
        "three 20px Ahem units plus two 20px boundaries should span 100px: {lines:#?}"
    );
    let mut wrapped_rows = std::collections::BTreeMap::<i32, String>::new();
    for line in lines.iter().filter(|line| {
        line.y() < lines[0].y() - 0.1
            && line
                .text
                .chars()
                .all(|character| character.is_ascii_digit())
    }) {
        wrapped_rows
            .entry((line.y() * 10.0).round() as i32)
            .or_default()
            .push_str(&line.text);
    }
    assert_eq!(
        wrapped_rows.into_values().rev().collect::<Vec<_>>(),
        ["123", "456", "789"],
        "{lines:#?}"
    );
    let letters = ['A', 'B', 'C']
        .into_iter()
        .map(|letter| {
            lines
                .iter()
                .find(|line| line.text == letter.to_string())
                .unwrap_or_else(|| panic!("missing {letter}: {lines:#?}"))
                .x()
        })
        .collect::<Vec<_>>();
    let emphasis = lines
        .iter()
        .filter(|line| line.text == "•")
        .map(|line| line.x())
        .collect::<Vec<_>>();
    assert_eq!(emphasis.len(), 3, "{lines:#?}");
    for positions in [&letters, &emphasis] {
        assert!((positions[1] - positions[0] - 30.0).abs() < 0.1);
        assert!((positions[2] - positions[1] - 30.0).abs() < 0.1);
    }
}

#[tokio::test]
async fn letter_spacing_tracks_ruby_base_columns_without_shifting_annotations() {
    let ahem = format!(
        "{}/tests/fixtures/wpt/css/css-fonts/Ahem.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let document = Html::from_string(format!(
        "<style>@page {{ size: 300pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
         p {{ margin: 0; font: 20px/1 Ahem; letter-spacing: 20px }}</style>\
         <p><ruby>A<rt>a</rt>BB<rt>b</rt></ruby></p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0].lines();
    let bases = lines
        .iter()
        .filter(|line| matches!(line.text.as_str(), "A" | "B"))
        .map(|line| line.x())
        .collect::<Vec<_>>();
    assert_eq!(bases.len(), 3, "{lines:#?}");
    assert!(
        (bases[1] - bases[0] - 30.0).abs() < 0.1,
        "bases={bases:?}, lines={lines:#?}"
    );
    assert!(
        (bases[2] - bases[1] - 30.0).abs() < 0.1,
        "bases={bases:?}, lines={lines:#?}"
    );

    let annotations = lines
        .iter()
        .filter(|line| matches!(line.text.as_str(), "a" | "b"))
        .map(|line| line.x())
        .collect::<Vec<_>>();
    assert_eq!(annotations.len(), 2, "{lines:#?}");
    assert!((annotations[1] - annotations[0] - 45.0).abs() < 0.1);
}

fn rendered_fragment_group_span(lines: &[&crate::document::paint::text::RenderedLine]) -> f32 {
    let left = lines
        .iter()
        .map(|line| line.x())
        .fold(f32::INFINITY, f32::min);
    let right = lines
        .iter()
        .map(|line| line.x() + rendered_line_advance(line))
        .fold(f32::NEG_INFINITY, f32::max);
    right - left
}

fn rendered_non_whitespace_group_bounds(
    lines: &[&crate::document::paint::text::RenderedLine],
) -> (f32, f32) {
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for line in lines {
        for run in &line.runs {
            let mut pen_x = line.x() + run.x_offset;
            for glyph in run.glyphs.as_deref().unwrap_or_default() {
                if !glyph.unicode.chars().all(char::is_whitespace) {
                    let start = pen_x + glyph.x_offset;
                    let end = start + glyph.x_advance;
                    left = left.min(start.min(end));
                    right = right.max(start.max(end));
                }
                pen_x += glyph.x_advance;
            }
        }
    }
    assert!(left.is_finite() && right.is_finite());
    (left, right)
}

fn lines_grouped_by_y(
    lines: &[crate::document::paint::text::RenderedLine],
) -> Vec<Vec<&crate::document::paint::text::RenderedLine>> {
    let mut groups = Vec::<Vec<&crate::document::paint::text::RenderedLine>>::new();
    for line in lines {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| (group[0].y() - line.y()).abs() < 0.01)
        {
            group.push(line);
        } else {
            groups.push(vec![line]);
        }
    }
    groups
}

fn grouped_line_texts(page: &spindrift::Page) -> Vec<String> {
    let mut groups = Vec::<Vec<&crate::document::paint::text::RenderedLine>>::new();
    for line in page.lines() {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| (group[0].y() - line.y()).abs() < 4.0)
        {
            group.push(line);
        } else {
            groups.push(vec![line]);
        }
    }
    groups
        .into_iter()
        .map(|mut group| {
            group.sort_by(|left, right| left.x().total_cmp(&right.x()));
            group
                .into_iter()
                .map(|line| line.text.as_str())
                .collect::<String>()
        })
        .collect()
}

fn rendered_rects_share_row(
    left: &crate::document::paint::shapes::RenderedRect,
    right: &crate::document::paint::shapes::RenderedRect,
) -> bool {
    let left_center = left.y() + left.height() / 2.0;
    let right_center = right.y() + right.height() / 2.0;
    (left_center - right_center).abs() < 24.0
}

fn rendered_line_visual_bounds(line: &crate::document::paint::text::RenderedLine) -> (f32, f32) {
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for run in &line.runs {
        let mut pen_x = line.x() + run.x_offset;
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
        (line.x(), line.x() + rendered_line_advance(line))
    }
}

fn visual_line_groups(
    lines: &[crate::document::paint::text::RenderedLine],
) -> Vec<Vec<&crate::document::paint::text::RenderedLine>> {
    let mut groups = Vec::<Vec<&crate::document::paint::text::RenderedLine>>::new();
    for line in lines {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| (group[0].y() - line.y()).abs() < 0.01)
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
    .render(&options).await
    .unwrap();
    let lines = &positive.pages[0].lines();

    assert!(lines.len() > 1);
    assert!((lines[0].x() - (crate::layout::PageMargins::DEFAULT.left() + 10.0)).abs() < 0.01);
    assert!((lines[1].x() - crate::layout::PageMargins::DEFAULT.left()).abs() < 0.01);

    let negative = Html::from_string(
        "<style>body { margin: 0; font-size: 10pt; line-height: 10pt } p { margin: 0; width: 40pt; text-indent: -10pt }</style><p>aa aa aa aa aa aa aa aa</p>",
    )
    .render(&options).await
    .unwrap();
    let lines = &negative.pages[0].lines();

    assert!(lines.len() > 1);
    assert!((lines[0].x() - (crate::layout::PageMargins::DEFAULT.left() - 10.0)).abs() < 0.01);
    assert!((lines[1].x() - crate::layout::PageMargins::DEFAULT.left()).abs() < 0.01);
}

#[tokio::test]
async fn vertical_writing_text_indent_moves_logical_inline_start() {
    let options = RenderOptions::default();
    let normal = Html::from_string(
        "<style>body { margin: 0 } p { margin: 0; writing-mode: vertical-rl; width: 50pt; height: 50pt; font-size: 10pt; line-height: 12pt }</style><p>A</p>",
    )
    .render(&options)
    .await
    .unwrap();
    let indented = Html::from_string(
        "<style>body { margin: 0 } p { margin: 0; writing-mode: vertical-rl; width: 50pt; height: 50pt; font-size: 10pt; line-height: 12pt; text-indent: 10pt }</style><p>A</p>",
    )
    .render(&options)
    .await
    .unwrap();

    let normal_line = normal.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("normal vertical line should render");
    let indented_line = indented.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("indented vertical line should render");

    assert!(
        indented_line.y() < normal_line.y() - 9.0,
        "vertical text-indent should move down the inline axis: normal={normal_line:?}, indented={indented_line:?}"
    );
}

#[tokio::test]
async fn vertical_writing_mixed_text_emits_writing_mode_aware_runs() {
    let document = Html::from_string(
        "<style>body { margin: 0 } p { margin: 0; writing-mode: vertical-rl; width: 60pt; height: 80pt; font-size: 10pt; line-height: 12pt }</style><p>中文AB</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let runs = document.pages[0]
        .lines()
        .iter()
        .flat_map(|line| line.runs.iter())
        .collect::<Vec<_>>();

    assert!(runs.iter().any(|run| {
        run.text.contains('中')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
    }));
    assert!(runs.iter().any(|run| {
        run.text.contains("AB")
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::ROTATE_CW
    }));
}

#[tokio::test]
async fn vertical_inside_list_markers_with_mixed_font_sizes_complete() {
    let document = Html::from_string(
        r#"<style>
            html { writing-mode: vertical-lr; }
            ol { list-style-position: inside; padding: 0; margin: 0; }
            .content::marker { content: "1. "; }
            .sz1 { font-size: 40px; }
            .sz1::marker { font-size: 20px; }
            .sz2 { font-size: 20px; }
            .sz2::marker { font-size: 40px; }
        </style>
        <ol><li>1.</li></ol>
        <ol><li class="content">1.</li></ol>
        <ol><li class="sz1">1.</li></ol>
        <ol><li class="content sz1">1.</li></ol>
        <ol><li class="sz2">1.</li></ol>
        <ol><li class="content sz2">1.</li></ol>"#,
    )
    .render(&RenderOptions::default())
    .await
    .expect("vertical inside list markers should lay out");

    let lines = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .collect::<Vec<_>>();
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.matches("1.").count())
            .sum::<usize>(),
        12,
        "each marker and principal text run should paint once: {lines:#?}"
    );
}

#[tokio::test]
async fn vertical_mixed_text_uses_unicode_vertical_orientation() {
    let document = Html::from_string(
        "<style>body { margin: 0 } p { margin: 0; writing-mode: vertical-rl; width: 60pt; height: 90pt; font-size: 10pt; line-height: 12pt }</style><p>a§、〈</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let runs = document.pages[0]
        .lines()
        .iter()
        .flat_map(|line| line.runs.iter())
        .collect::<Vec<_>>();

    assert!(runs.iter().any(|run| {
        run.text.contains('a')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::ROTATE_CW
    }));
    assert!(runs.iter().any(|run| {
        run.text.contains('§')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
    }));
    assert!(runs.iter().any(|run| {
        run.text.contains('、')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
    }));
    assert!(runs.iter().any(|run| {
        run.text.contains('〈')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
    }));
}

#[tokio::test]
async fn vertical_text_orientation_upright_paints_latin_upright() {
    let document = Html::from_string(
        "<style>body { margin: 0 } p { margin: 0; writing-mode: vertical-rl; text-orientation: upright; width: 60pt; height: 80pt; font-size: 10pt; line-height: 12pt }</style><p>AB中文</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let runs = document.pages[0]
        .lines()
        .iter()
        .flat_map(|line| line.runs.iter())
        .collect::<Vec<_>>();

    assert!(runs.iter().any(|run| {
        run.text.contains('A')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
    }));
    assert!(runs.iter().any(|run| {
        run.text.contains('B')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
    }));
    assert!(runs.iter().any(|run| {
        run.text.contains('中')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
    }));
}

#[tokio::test]
async fn vertical_lr_upright_preserved_leading_space_keeps_sibling_line_origin() {
    let ahem = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wpt/css/css-fonts/Ahem.ttf");
    let render = |leading: &str| {
        Html::from_string(format!(
            "<style>@page {{ size: 160px 160px; margin: 0 }}\
             @font-face {{ font-family: Ahem; src: url(file://{}) }}\
             html {{ writing-mode: vertical-lr }}\
             body {{ margin: 0 }}\
             .test {{ font: 20px/1 Ahem; height: 3em; text-orientation: upright }}\
             .line {{ white-space: pre }}</style>\
             <div class=test><div class=line>{leading}A</div><div class=line>B</div></div>",
            ahem.display(),
        ))
    };
    let without_leading_space = render("").render(&RenderOptions::default()).await.unwrap();
    let with_ascii_space = render(" ").render(&RenderOptions::default()).await.unwrap();
    let with_ideographic_space = render("\u{3000}")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let text_origin = |document: &spindrift::Document, text: char| {
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .find(|line| line.text.contains(text))
            .map(|line| {
                line.y()
                    + line
                        .runs
                        .iter()
                        .find(|run| run.text.contains(text))
                        .map_or(0.0, |run| run.y_offset)
            })
            .expect("each test character should produce a rendered line")
    };
    let line_origin = |document: &spindrift::Document, text: char| {
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .find(|line| line.text.contains(text))
            .map(|line| line.x())
            .expect("each test character should produce a rendered line")
    };

    let plain_a = text_origin(&without_leading_space, 'A');
    let ascii_a = text_origin(&with_ascii_space, 'A');
    let ideographic_a = text_origin(&with_ideographic_space, 'A');
    assert!(
        (ascii_a - plain_a).abs() > 0.01,
        "a preserved U+0020 must retain a shaped inline advance"
    );
    assert!(
        (ideographic_a - plain_a).abs() > 0.01,
        "U+3000 must retain its independently shaped inline advance"
    );

    let plain_b = line_origin(&without_leading_space, 'B');
    let ascii_b = line_origin(&with_ascii_space, 'B');
    let ideographic_b = line_origin(&with_ideographic_space, 'B');
    assert!(
        (ascii_b - plain_b).abs() < 0.01,
        "a leading preserved U+0020 must not move a sibling line origin"
    );
    assert!(
        (ideographic_b - plain_b).abs() < 0.01,
        "the U+0020 correction must not alter U+3000 line placement"
    );
}

#[tokio::test]
async fn vertical_text_orientation_sideways_rotates_cjk_and_latin() {
    let document = Html::from_string(
        "<style>body { margin: 0 } p { margin: 0; writing-mode: vertical-rl; text-orientation: sideways; width: 60pt; height: 80pt; font-size: 10pt; line-height: 12pt }</style><p>中文AB</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let runs = document.pages[0]
        .lines()
        .iter()
        .flat_map(|line| line.runs.iter())
        .collect::<Vec<_>>();

    assert!(runs.iter().any(|run| {
        run.text.contains("中文")
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::ROTATE_CW
    }));
    assert!(runs.iter().any(|run| {
        run.text.contains("AB")
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::ROTATE_CW
    }));
    assert!(
        runs.iter()
            .filter(|run| !run.text.is_empty())
            .all(|run| run.text_matrix
                == crate::document::paint::text::RenderedTextMatrix::ROTATE_CW)
    );
}

#[tokio::test]
async fn page_margin_text_orientation_uses_shared_vertical_placement() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 20pt; @top-left { content: \"AB\"; writing-mode: vertical-rl; text-orientation: upright; font-size: 10pt } } body { margin: 0 }</style>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .flat_map(|line| line.runs.iter())
            .any(|run| {
                run.text.contains('A')
                    && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
            })
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .flat_map(|line| line.runs.iter())
            .any(|run| {
                run.text.contains('B')
                    && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
            })
    );
}

#[tokio::test]
async fn page_margin_mixed_text_uses_unicode_vertical_orientation() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 20pt; @top-left { content: \"a§\"; writing-mode: vertical-rl; font-size: 10pt } } body { margin: 0 }</style>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let runs = document.pages[0]
        .lines()
        .iter()
        .flat_map(|line| line.runs.iter())
        .collect::<Vec<_>>();

    assert!(runs.iter().any(|run| {
        run.text.contains('a')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::ROTATE_CW
    }));
    assert!(runs.iter().any(|run| {
        run.text.contains('§')
            && run.text_matrix == crate::document::paint::text::RenderedTextMatrix::IDENTITY
    }));
}

#[tokio::test]
async fn inline_block_text_orientation_uses_shared_vertical_placement() {
    let document = Html::from_string(
        "<style>body { margin: 0 } span { display: inline-block; writing-mode: vertical-rl; text-orientation: sideways; width: 50pt; height: 80pt; font-size: 10pt; line-height: 12pt }</style><span>中文</span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .flat_map(|line| line.runs.iter())
            .any(|run| {
                run.text.contains("中文")
                    && run.text_matrix
                        == crate::document::paint::text::RenderedTextMatrix::ROTATE_CW
            })
    );
}

#[tokio::test]
async fn vertical_inline_forced_break_stacks_atomic_lines_in_block_axis() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body { margin: 0 }\
         div { writing-mode: vertical-rl; line-height: 0; width: 30pt; height: 45pt }\
         span { display: inline-block; width: 15pt; height: 45pt }\
         </style><div><span style=\"background:green\"></span><br><span style=\"background:blue\"></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (green.y() - blue.y()).abs() < 0.01,
        "forced-break vertical inline-block lines should share inline-start: green={green:?}, blue={blue:?}"
    );
    assert!(
        blue.x() + blue.width() <= green.x() + 0.01,
        "vertical-rl forced break should stack the next line to the physical left: green={green:?}, blue={blue:?}"
    );
}

/// CSS Text reftest behavior: a selected U+00AD in vertical writing must use
/// the same fixed physical height and following-sibling position as the
/// equivalent explicit conditional hyphen plus forced break.
#[tokio::test]
async fn vertical_soft_hyphen_preserves_definite_height_and_sibling_position() {
    let stylesheet = "\
        @page { size: 180pt 320pt; margin: 0 }\
        body { margin: 0; font: 16px monospace }\
        div { writing-mode: vertical-rl; border: 1px solid black; margin: 10px; \
              padding: 2px; hyphens: manual; width: 3em; height: 9ch }\
        .first { background: red } .second { background: blue }";
    let actual = Html::from_string(format!(
        "<style>{stylesheet}</style><div class=\"first\">hyphen&shy;ation</div>\
         <div class=\"second\">hyphen&#x2010;<br>ation</div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "<style>{stylesheet}</style><div class=\"first\">hyphen&#x2010;<br>ation</div>\
         <div class=\"second\">hyphen&#x2010;<br>ation</div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let background_geometry = |document: &spindrift::Document, color| {
        let rect = document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .expect("each vertical test box paints its background");
        (rect.y(), rect.height())
    };
    for color in [CssColor::new(255, 0, 0), CssColor::new(0, 0, 255)] {
        let actual_geometry = background_geometry(&actual, color);
        let reference_geometry = background_geometry(&reference, color);
        assert!(
            (actual_geometry.0 - reference_geometry.0).abs() < 0.01
                && (actual_geometry.1 - reference_geometry.1).abs() < 0.01,
            "selected soft-hyphen layout must retain the reference box geometry: \
             color={color:?}, actual={actual_geometry:?}, reference={reference_geometry:?}"
        );
    }
}

#[tokio::test]
async fn inline_block_text_uses_sequence_for_forced_empty_lines() {
    let document = Html::from_string(
        "<style>body { margin: 0; font-size: 10pt; line-height: 12pt } span { display: inline-block; white-space: pre-line; width: 80pt }</style><span>alpha\n\nbeta</span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let alpha = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "alpha")
        .expect("inline-block first line should render");
    let beta = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "beta")
        .expect("inline-block second visible line should render");

    assert!(
        alpha.y() > beta.y() + 20.0,
        "forced empty line should separate inline-block text lines: alpha={alpha:?}, beta={beta:?}"
    );
}

#[tokio::test]
async fn inline_block_text_atom_uses_sequence_for_zwsp_and_soft_hyphen() {
    let document = Html::from_string(
        "<style>\
         @page { size: 90pt 140pt; margin: 10pt }\
         body { margin: 0; font-family: monospace; font-size: 10pt; line-height: 10pt }\
         span { display: inline-block; width: 22pt; font: inherit; line-height: 10pt }\
         </style><span>abc&#x200b;def<br>hyphen&shy;ation</span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(lines.contains(&"abc"), "{lines:?}");
    assert!(lines.contains(&"def"), "{lines:?}");
    assert!(lines.iter().any(|line| line.ends_with('‐')), "{lines:?}");
    assert_eq!(
        lines
            .iter()
            .map(|line| line.replace('‐', ""))
            .collect::<String>(),
        "abcdefhyphenation"
    );
}

#[tokio::test]
async fn text_indent_each_line_applies_after_forced_breaks_only() {
    let options = RenderOptions::default();
    let document = Html::from_string(
        "<style>body { margin: 0; font-family: monospace; font-size: 10pt; line-height: 12pt } p { margin: 0; width: 44pt; text-indent: 12pt each-line }</style><p>one two<br>red blue green</p>",
    )
    .render(&options)
    .await
    .unwrap();
    let lines = document.pages[0]
        .lines()
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
    assert!((lines[0].x() - (crate::layout::PageMargins::DEFAULT.left() + 12.0)).abs() < 0.01);
    assert!((lines[1].x() - crate::layout::PageMargins::DEFAULT.left()).abs() < 0.01);
    assert!((lines[2].x() - (crate::layout::PageMargins::DEFAULT.left() + 12.0)).abs() < 0.01);
    assert!((lines[3].x() - crate::layout::PageMargins::DEFAULT.left()).abs() < 0.01);
    assert!((lines[4].x() - crate::layout::PageMargins::DEFAULT.left()).abs() < 0.01);
}

#[tokio::test]
async fn supports_text_transform() {
    let document = Html::from_string(
        "<div style=\"text-transform: uppercase\"><p style=\"margin: 0\">Hello world</p></div><p style=\"margin: 0; text-transform: capitalize\">mixed CASE words</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "HELLO WORLD");
    assert_eq!(document.pages[0].lines()[1].text, "Mixed CASE Words");
}

#[tokio::test]
async fn capitalize_leaves_non_initial_word_characters_unchanged() {
    let document = Html::from_string(
        "<p style=\"margin: 0; text-transform: capitalize\">mIXed caSE 123abc</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "MIXed CaSE 123abc");
}

#[tokio::test]
async fn capitalize_uses_icu_full_case_mapping_for_latin_1() {
    let document =
        Html::from_string("<p style=\"margin: 0; text-transform: capitalize\">aaa µµµ ààà ÿÿÿ</p>")
            .render(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "Aaa Μµµ Ààà Ÿÿÿ");
}

#[tokio::test]
async fn text_transform_uses_language_tailored_case_mapping() {
    let document = Html::from_string(
        "<p lang=\"tr\" style=\"margin:0;text-transform:uppercase\">i ı</p>\
         <p lang=\"tr\" style=\"margin:0;text-transform:lowercase\">İ I</p>\
         <p lang=\"el\" style=\"margin:0;text-transform:lowercase\">ΟΣ ΟΣΑ</p>\
         <p lang=\"lt\" style=\"margin:0;text-transform:lowercase\">I\u{0301}</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "İ I");
    assert_eq!(document.pages[0].lines()[1].text, "i ı");
    assert_eq!(document.pages[0].lines()[2].text, "ος οσα");
    assert_eq!(document.pages[0].lines()[3].text, "i\u{0307}\u{0301}");
}

#[tokio::test]
async fn supports_full_width_and_full_size_kana_text_transform() {
    let document = Html::from_string(
        "<p style=\"margin: 0; text-transform: full-width\">A 1 ｶﾞ</p>\
         <p style=\"margin: 0; text-transform: full-size-kana\">ぁァㇷ</p>\
         <p style=\"margin: 0; text-transform: uppercase full-width full-size-kana\">ab ァ</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    // A font fallback boundary may expose the combining dakuten in a separate
    // PDF text run. The CSS transform itself is covered directly below; this
    // smoke assertion verifies the rendered text sequence.
    assert!(
        texts.windows(2).any(|pair| {
            pair[0] == "Ａ　１　ガ" || pair == ["Ａ　１　カ", "\u{3099}"]
        }),
        "{texts:?}"
    );
    assert!(texts.contains(&"あアフ"), "{texts:?}");
    assert!(texts.contains(&"ＡＢ　ア"), "{texts:?}");
}

#[tokio::test]
async fn supports_text_transform_inside_text_only_inline_block() {
    let document = Html::from_string(
        "<p style=\"margin: 0\"><span style=\"display: inline-block; text-transform: uppercase\">Hello world</span></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "HELLO WORLD");
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
    .render(&RenderOptions::default())
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "AbCD EF");
}

#[tokio::test]
async fn capitalize_uses_unicode_word_boundaries_across_inline_fragments() {
    let document = Html::from_string(
        "<p style=\"margin: 0; text-transform: capitalize\">mark’d ye <span>mark</span><span>’</span><span>d</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "Mark’d Ye Mark’d");
}

#[tokio::test]
async fn renders_bidi_text_in_visual_order() {
    let document = Html::from_string(
        "<p style=\"margin: 0; font-size: 12pt; line-height: 12pt\">abc אבג def</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "abc גבא def");
}

#[tokio::test]
async fn renders_styled_bidi_inline_text_in_visual_order() {
    let document = Html::from_string(
        "<p style=\"margin: 0; font-size: 12pt; line-height: 12pt\">abc <em>אבג</em> def</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let y = document.pages[0].lines()[0].y();
    let mut line = document.pages[0]
        .lines()
        .iter()
        .filter(|line| (line.y() - y).abs() < 0.1)
        .collect::<Vec<_>>();
    line.sort_by(|left, right| left.x().total_cmp(&right.x()));
    let text = line
        .into_iter()
        .map(|line| line.text.as_str())
        .collect::<String>();

    assert_eq!(text, "abc גבא def");
}

#[tokio::test]
async fn bdi_is_neutral_to_its_outer_rtl_paragraph() {
    let document = Html::from_string(
        "<div dir=\"rtl\" style=\"margin: 0; font-size: 12pt; line-height: 12pt\">a - <bdi>[1]</bdi>...</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let y = document.pages[0].lines()[0].y();
    let mut line = document.pages[0]
        .lines()
        .iter()
        .filter(|line| (line.y() - y).abs() < 0.1)
        .collect::<Vec<_>>();
    line.sort_by(|left, right| left.x().total_cmp(&right.x()));
    let text = line
        .into_iter()
        .map(|line| line.text.as_str())
        .collect::<String>();

    assert_eq!(text, "...[1] - a");
}

#[tokio::test]
async fn bdi_is_neutral_in_a_constrained_rtl_block() {
    let document = Html::from_string(
        "<style>body { font-size: 2em } .test { width: 400px }</style>\
         <div class=\"test\">\
           <div dir=\"rtl\">a - <bdi dir=\"ltr\">[1]</bdi>...</div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        grouped_line_texts(&document.pages[0])
            .last()
            .expect("expected the RTL bdi line"),
        "...[1] - a"
    );
}

#[tokio::test]
async fn bidi_reorders_inline_box_decorations_with_neutral_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 420pt 220pt; margin: 20pt } body { margin: 0 }\
         .container { width: 300px; background: pink; font-size: 30px; line-height: 42px }\
         div { margin-bottom: 10px }\
         .purple { border: purple solid 5px }\
         .orange { background: orange }</style>\
         <div class=\"container\">\
           <div dir=\"rtl\"><span class=\"purple\">inspect</span><span class=\"orange\">pause</span></div>\
           <div dir=\"rtl\"><span class=\"purple\">inspect</span> <span class=\"orange\">pause</span></div>\
           <div dir=\"rtl\"><span class=\"purple\">inspect<span class=\"orange\">pause</span></span></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let text_rows = grouped_line_texts(page)
        .into_iter()
        .filter(|text| text.contains("inspect") || text.contains("pause"))
        .collect::<Vec<_>>();
    assert_eq!(
        text_rows,
        vec!["inspectpause", "inspect pause", "inspectpause"]
    );

    let mut orange_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 165, 0)))
        .collect::<Vec<_>>();
    orange_rects.sort_by(|left, right| right.y().total_cmp(&left.y()));
    assert_eq!(orange_rects.len(), 3, "{orange_rects:?}");

    let purple_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(128, 0, 128)))
        .collect::<Vec<_>>();
    assert!(
        purple_rects.len() >= 9,
        "expected purple border rectangles for all three rows: {purple_rects:?}"
    );

    for (index, orange) in orange_rects.iter().enumerate() {
        let row_purple = purple_rects
            .iter()
            .copied()
            .filter(|rect| rendered_rects_share_row(rect, orange))
            .collect::<Vec<_>>();
        assert!(
            row_purple.len() >= 2,
            "row {index} should have purple border fragments near orange={orange:?}: {row_purple:?}"
        );
        let vertical_edges = row_purple
            .iter()
            .copied()
            .filter(|rect| rect.width() <= 5.5 && rect.height() > 10.0)
            .collect::<Vec<_>>();
        assert_eq!(
            vertical_edges.len(),
            2,
            "row {index} should have exactly two purple vertical border edges near orange={orange:?}: {row_purple:?}"
        );
        let purple_left = row_purple
            .iter()
            .map(|rect| rect.x())
            .fold(f32::INFINITY, f32::min);
        let purple_right = row_purple
            .iter()
            .map(|rect| rect.x() + rect.width())
            .fold(f32::NEG_INFINITY, f32::max);
        let vertical_left = vertical_edges
            .iter()
            .map(|rect| rect.x())
            .fold(f32::INFINITY, f32::min);
        let vertical_right = vertical_edges
            .iter()
            .map(|rect| rect.x() + rect.width())
            .fold(f32::NEG_INFINITY, f32::max);
        let orange_left = orange.x();
        let orange_right = orange.x() + orange.width();

        if index < 2 {
            assert!(
                orange_left >= purple_right - 0.5,
                "sibling orange span should paint after the purple border in row {index}: orange={orange:?}, purple=({purple_left}, {purple_right})"
            );
            assert!(
                vertical_right <= orange_left + 0.5,
                "sibling purple right edge should border inspect before orange in row {index}: orange={orange:?}, vertical_edges={vertical_edges:?}"
            );
            assert!(
                vertical_edges
                    .iter()
                    .all(|edge| edge.x() + edge.width() < orange_right - 0.5),
                "sibling purple vertical edge should not be painted at the far right of pause in row {index}: orange={orange:?}, vertical_edges={vertical_edges:?}"
            );
        } else {
            assert!(
                orange_left >= purple_left - 0.5 && orange_right <= purple_right + 0.5,
                "nested orange span should stay inside the purple outer span: orange={orange:?}, purple=({purple_left}, {purple_right})"
            );
            assert!(
                vertical_left <= orange_left + 0.5 && vertical_right >= orange_right - 0.5,
                "nested purple vertical edges should wrap the orange child span: orange={orange:?}, vertical_edges={vertical_edges:?}"
            );
        }
    }
}

#[tokio::test]
async fn mixed_inline_ltr_atomic_child_inside_rtl_parent_uses_parent_base_direction() {
    let document = Html::from_string(
        "<p style=\"margin:0; font-size:12pt; line-height:12pt; direction:rtl\">אבג \
         <span style=\"display:inline-block; direction:ltr\">abc</span> דהו</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let left_hebrew = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "והד")
        .expect("expected inline-end Hebrew run");
    let atom = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "abc")
        .expect("expected LTR atomic child");
    let right_hebrew = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "גבא")
        .expect("expected inline-start Hebrew run");

    assert!(left_hebrew.x() < atom.x());
    assert!(atom.x() < right_hebrew.x());
}

#[tokio::test]
async fn mixed_inline_rtl_atomic_child_inside_ltr_parent_uses_parent_base_direction() {
    let document = Html::from_string(
        "<p style=\"margin:0; font-size:12pt; line-height:12pt; direction:ltr\">abc \
         <span style=\"display:inline-block; direction:rtl\">אבג</span> def</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let left_latin = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "abc")
        .expect("expected inline-start Latin run");
    let atom = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "גבא")
        .expect("expected RTL atomic child");
    let right_latin = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "def")
        .expect("expected inline-end Latin run");

    assert!(left_latin.x() < atom.x());
    assert!(atom.x() < right_latin.x());
}

#[tokio::test]
async fn unicode_bidi_plaintext_aligns_lines_by_first_strong_character() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 180pt; margin: 10pt } body { margin: 0 } \
         div { font-size: 16pt; line-height: 18pt; width: 160pt; white-space: pre; \
               text-align: start; unicode-bidi: plaintext; border: 1pt solid black; padding: 0 6pt }</style>\
         <div>français\nفارسی\nfrançais\nفارسی</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
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
        .map(|line| line.x())
        .collect::<Vec<_>>();
    let persian_x = lines
        .iter()
        .filter(|line| line.text.contains("فارسی") || line.text.contains("یسراف"))
        .map(|line| line.x())
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "abc fed ghi");
}

/// `unicode-bidi: isolate-override` contributes an independent LTR visual
/// sequence to its surrounding RTL line. Its final shaped advance must place
/// the following outer text and the isolate's background from the same visual
/// geometry, rather than from pre-bidi source slices.
/// <https://drafts.csswg.org/css-writing-modes-4/#unicode-bidi>
#[tokio::test]
async fn isolate_override_uses_final_visual_group_advances_for_following_text_and_background() {
    let style = "<style>@page { size: 320px 100px; margin: 0 }\
                 body { margin: 0 }\
                 p { margin: 0; width: 240px; font: 24px/30px serif }\
                 .target span { direction: ltr; unicode-bidi: isolate-override; background: rgb(0, 255, 0) }\
                 .reference span { background: rgb(0, 255, 0) }</style>";
    let target = Html::from_string(format!(
        "{style}<p class=\"target\" dir=\"rtl\">&gt; <span>&#x5d0;&#x5d1;&#x5d2;&#x5d3; &gt; abcd</span> &gt;</p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "{style}<p class=\"reference\" dir=\"rtl\"><bdo dir=\"ltr\">&lt; <span>&#x5d0;&#x5d1;&#x5d2;&#x5d3; &gt; abcd</span> &lt;</bdo></p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let target_lines = visual_line_groups(target.pages[0].lines());
    let reference_lines = visual_line_groups(reference.pages[0].lines());
    assert_eq!(target_lines.len(), 1, "{target_lines:#?}");
    assert_eq!(reference_lines.len(), 1, "{reference_lines:#?}");
    let target_bounds = rendered_non_whitespace_group_bounds(&target_lines[0]);
    let reference_bounds = rendered_non_whitespace_group_bounds(&reference_lines[0]);
    assert!(
        (target_bounds.0 - reference_bounds.0).abs() < 0.01
            && (target_bounds.1 - reference_bounds.1).abs() < 0.01,
        "target={target_bounds:?}, reference={reference_bounds:?}"
    );

    let green = CssColor::new(0, 255, 0);
    let target_backgrounds = target.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .collect::<Vec<_>>();
    let reference_backgrounds = reference.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .collect::<Vec<_>>();
    assert!(
        target_backgrounds.len() > 1,
        "bidi fragmentation must retain source-owned background pieces: {target_backgrounds:#?}"
    );
    assert!(
        !reference_backgrounds.is_empty(),
        "the equivalent override must paint its isolate background: {reference_backgrounds:#?}"
    );
    let background_bounds = |rects: &[&crate::document::paint::shapes::RenderedRect]| {
        let start = rects
            .iter()
            .map(|rect| rect.x())
            .fold(f32::INFINITY, f32::min);
        let end = rects
            .iter()
            .map(|rect| rect.x() + rect.width())
            .fold(f32::NEG_INFINITY, f32::max);
        (start, end)
    };
    let target_background_bounds = background_bounds(&target_backgrounds);
    let reference_background_bounds = background_bounds(&reference_backgrounds);
    assert!(
        (target_background_bounds.0 - reference_background_bounds.0).abs() < 0.01
            && (target_background_bounds.1 - reference_background_bounds.1).abs() < 0.01,
        "target={:?}, reference={:?}",
        target_background_bounds,
        reference_background_bounds
    );
}

#[tokio::test]
async fn wpt_empty_inline_spans_preserve_bidi_scope_across_forced_breaks() {
    const TARGET_ROWS: [&str; 12] = [
        "1;234;56א;",
        "<span></span>1;234;56א;",
        "1<span></span>;234;56א;",
        "1;<span></span>234;56א;",
        "1;2<span></span>34;56א;",
        "1;23<span></span>4;56א;",
        "1;234<span></span>;56א;",
        "1;234;<span></span>56א;",
        "1;234;5<span></span>6א;",
        "1;234;56<span></span>א;",
        "1;234;56א<span></span>;",
        "1;234;56א;<span></span>",
    ];
    const REFERENCE_ROW: &str = ";א56;234;1";

    let target_rows = TARGET_ROWS
        .iter()
        .map(|row| format!("<span dir=\"auto\">{row}</span><br>"))
        .collect::<String>();
    let reference_rows = std::iter::repeat_n(
        format!("<span>{REFERENCE_ROW}</span><br>"),
        TARGET_ROWS.len(),
    )
    .collect::<String>();
    let common_style = "<style>@page{size:240pt 220pt;margin:10pt}body{margin:0;font-size:12pt;line-height:14pt}</style>";

    let target = Html::from_string(format!("{common_style}{target_rows}"))
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let reference = Html::from_string(format!(
        "{common_style}<style>span{{unicode-bidi:bidi-override}}</style>{reference_rows}"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let bidi_rows = |document: &spindrift::Document| {
        document.pages[0]
            .lines()
            .iter()
            .filter(|line| line.text.contains('א'))
            .map(|line| (line.text.clone(), line.x(), line.y()))
            .collect::<Vec<_>>()
    };
    let target_rows = bidi_rows(&target);
    let reference_rows = bidi_rows(&reference);

    assert_eq!(target_rows.len(), TARGET_ROWS.len(), "{target_rows:?}");
    assert_eq!(
        reference_rows.len(),
        TARGET_ROWS.len(),
        "{reference_rows:?}"
    );
    for (index, (target, reference)) in target_rows.iter().zip(&reference_rows).enumerate() {
        assert_eq!(target.0, reference.0, "row {index}");
        assert!((target.1 - reference.1).abs() < 0.01, "row {index}");
        assert!((target.2 - reference.2).abs() < 0.01, "row {index}");
    }
}

#[tokio::test]
async fn html_dir_auto_sets_direction_from_first_strong_descendant_text() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 10pt } body { margin: 0 } \
         p { margin: 0; width: 160pt; font-size: 12pt; line-height: 14pt; text-align: start }</style>\
         <p dir=\"auto\">abc</p><p dir=\"auto\">אבג</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let latin = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "abc")
        .expect("expected ltr line");
    let hebrew = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "גבא")
        .expect("expected rtl visual line");

    assert!(hebrew.x() > latin.x() + 120.0);
}

#[tokio::test]
async fn author_direction_overrides_html_dir_auto_directionality() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body { margin: 0 } \
         p { margin: 0; width: 160pt; font-size: 12pt; line-height: 14pt; text-align: start }</style>\
         <p dir=\"auto\" style=\"direction:ltr\">אבג</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "גבא")
        .expect("expected visual rtl text");

    assert!(line.x() < crate::layout::PageMargins::DEFAULT.left() + 20.0);
}

#[tokio::test]
async fn html_bdo_uses_ua_isolate_override() {
    let document = Html::from_string(
        "<p style=\"margin:0; font-size:12pt; line-height:12pt\">abc <bdo dir=\"rtl\">def</bdo> ghi</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "abc fed ghi");
}

#[tokio::test]
async fn html_bdo_ltr_overrides_astral_adlam_intrinsic_rtl_order() {
    let adlam = "\u{1e900}\u{1e901}\u{1e902}\u{1e901}\u{1e904}";
    let document = Html::from_string(format!(
        "<p style=\"margin:0; font-family: serif; font-size:12pt; line-height:12pt\"><bdo dir=\"ltr\">{adlam}</bdo></p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, adlam);
}

#[tokio::test]
async fn join_controls_do_not_split_inline_shaping_runs() {
    let document = Html::from_string(
        "<p style=\"margin: 0; font-size: 12pt; line-height: 12pt; font-family: sans-serif\">A<span style=\"font-family: serif\">&#x200c;</span>B</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let line = &document.pages[0].lines()[0];
    assert_eq!(line.text, "A\u{200c}B");
    assert_eq!(line.runs.len(), 1);
    assert_eq!(line.runs[0].text.as_ref(), "A\u{200c}B");
}

#[tokio::test]
async fn generated_control_characters_render_as_visible_glyphs() {
    let document = Html::from_string(
        r#"<style>body { margin: 0 } div { font-size: 20pt; line-height: 20pt } div::after { content: "\0099" }</style><div></div>"#,
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "\u{25a0}")
        .expect("generated control character should be replaced by a visible glyph");
    assert!(rendered_line_advance(line) > 0.0);
}

#[tokio::test]
async fn literal_nel_forces_a_line_break_in_normal_and_nowrap() {
    for white_space in ["normal", "nowrap"] {
        let document = Html::from_string(format!(
            "<style>body, p {{ margin: 0 }} p {{ white-space: {white_space}; font-size: 20pt; line-height: 20pt }}</style><p>One\u{0085}Two</p>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        assert_eq!(
            grouped_line_texts(&document.pages[0]),
            ["One", "Two"],
            "literal NEL must remain a forced break with white-space: {white_space}"
        );
    }
}

#[tokio::test]
async fn supports_white_space_pre_wrap_newlines() {
    let document = Html::from_string(
        "<p style=\"white-space: pre-wrap; margin: 0; font-size: 10pt; line-height: 10pt\">One\nTwo</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "One");
    assert_eq!(document.pages[0].lines()[1].text, "Two");
    assert!(document.pages[0].lines()[1].y() < document.pages[0].lines()[0].y());
}

#[tokio::test]
async fn html_br_line_break_comes_from_generated_before_content() {
    let document = Html::from_string(
        "<style>p { margin: 0; font-size: 10pt; line-height: 10pt } br::before { content: none }</style><p>One<br>Two</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(
        lines,
        vec!["One", "Two"],
        "the HTML br element remains a forced line break when its UA pseudo-element is restyled"
    );
}

#[tokio::test]
async fn html_br_clear_both_clears_prior_floats() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 200pt; margin: 0 }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .box { float: left; width: 20pt; height: 20pt; background: green }\
         </style>\
         <div class=\"box\"></div><br style=\"clear:both\"><div class=\"box\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - 20.0).abs() < 0.01
                && (rect.height() - 20.0).abs() < 0.01
        })
        .collect::<Vec<_>>();
    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    assert!(
        green_rects[1].y() < green_rects[0].y() - 19.0,
        "second float should clear below the first: {green_rects:?}"
    );
}

#[tokio::test]
async fn pre_wrap_final_segment_break_does_not_create_empty_line() {
    let document = Html::from_string(
        "<p style=\"white-space: pre-wrap; margin: 0; font-size: 10pt; line-height: 10pt\">One\n</p>",
    )
    .render(&RenderOptions::default())
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);

    assert_eq!(lines, vec![" XX ".to_string(), "XXX ".to_string()]);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| {
            line.runs
                .iter()
                .map(|run| run.text.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(lines, vec!["XX\t\t".to_string(), "XX".to_string()]);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines().len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "one two three four");
}

#[tokio::test]
async fn supports_white_space_pre() {
    let document = Html::from_string(
        "<p style=\"white-space: pre; margin: 0; font-size: 10pt; line-height: 10pt\"> A  B\nC</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, " A  B");
    assert_eq!(document.pages[0].lines()[1].text, "C");
}

#[tokio::test]
async fn supports_white_space_pre_line() {
    let document = Html::from_string(
        "<p style=\"white-space: pre-line; margin: 0; font-size: 10pt; line-height: 10pt\">A   B\nC</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "A B");
    assert_eq!(document.pages[0].lines()[1].text, "C");
}

#[tokio::test]
async fn pre_line_consecutive_forced_breaks_keep_empty_sequence_line() {
    let document = Html::from_string(
        "<style>p { margin: 0; white-space: pre-line; font-size: 10pt; line-height: 10pt }</style>\
         <p>A\n\nB</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["A", "B"]);
    let y_gap = document.pages[0].lines()[0].y() - document.pages[0].lines()[1].y();
    assert!(
        (y_gap - 20.0).abs() < 0.01,
        "expected an empty sequence line between A and B, got y gap {y_gap}"
    );
}

#[tokio::test]
async fn supports_white_space_break_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 120pt; margin: 10pt } p { white-space: break-spaces; margin: 0; width: 14pt; font-size: 10pt; line-height: 10pt }</style><p>A   B</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "A  ");
    assert_eq!(document.pages[0].lines()[1].text, " B");
}

#[tokio::test]
async fn normal_white_space_transforms_segment_breaks_by_context() {
    let document = Html::from_string(
        "<style>body, p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <p>中文\n中文</p><p>中文\nenglish</p><p>word\nword</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["中文中文", "中文 english", "word word"]);
}

#[tokio::test]
async fn white_space_collapses_across_inline_generated_and_padding_edges() {
    let document = Html::from_string(
        "<style>\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         .generated::before { content: \"  \" }\
         .padded { padding-left: 1pt }\
         </style>\
         <p>A <span></span>  B</p>\
         <p>A<span class=\"generated\"></span>  B</p>\
         <p>A <span class=\"padded\">  B</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["A B", "A B", "A B"]);
}

#[tokio::test]
async fn atom_adjacent_collapsed_spaces_are_preserved_in_rendered_text() {
    let document = Html::from_string(
        "<style>\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt }\
         span { display: inline-block; width: 5pt; height: 5pt }\
         </style>\
         <p>A\n<span></span> B</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["A  B"]);
}

#[tokio::test]
async fn atom_adjacent_preserved_spaces_are_preserved_in_rendered_text() {
    let document = Html::from_string(
        "<style>\
         body, p { margin: 0; font: 10pt/10pt monospace }\
         p { white-space: pre-wrap }\
         span { display: inline-block; width: 5pt; height: 5pt }\
         </style>\
         <p>A <span></span> B</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["A  B"]);
}

#[tokio::test]
async fn generated_inline_content_uses_shared_css_text_edge_processing() {
    let document = Html::from_string(
        "<style>\
         @page { size: 90pt 120pt; margin: 10pt }\
         body, p { margin: 0; font-family: monospace; font-size: 10pt; line-height: 10pt }\
         p { width: 100pt; overflow-wrap: anywhere }\
         p::before { content: \"A  \" }\
         p::after { content: \"\\200B C\" }\
         </style><p>BBBB</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines.concat(), "A BBBBC");
    assert!(
        lines
            .iter()
            .all(|line| !line.chars().any(|character| character == '\u{200b}')),
        "{lines:?}"
    );
}

#[tokio::test]
async fn inside_marker_generated_content_collapses_whitespace_on_first_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 100pt; margin: 10pt }\
         body, ol, li { margin: 0; padding: 0; font-size: 10pt; line-height: 10pt }\
         ol { padding-left: 20pt; list-style-position: inside }\
         li::marker { content: \"# \" }\
         li::before { content: \"  before \" }\
         </style><ol><li>  text</li></ol>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["# before text"]);
}

#[tokio::test]
async fn pre_line_preserves_segment_breaks_across_inline_boundaries() {
    let document = Html::from_string(
        "<style>p { margin: 0; white-space: pre-line; font-size: 10pt; line-height: 10pt }</style>\
         <p>A   <span>B\nC</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["A B", "C"]);
}

#[tokio::test]
async fn pre_wrap_final_segment_break_trims_across_inline_boundary() {
    let document = Html::from_string(
        "<style>p { margin: 0; white-space: pre-wrap; font-size: 10pt; line-height: 10pt }</style>\
         <p>One<span>\n</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["One"]);
}

#[tokio::test]
async fn supports_overflow_wrap_anywhere() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } p { margin: 0; width: 18pt; font-size: 10pt; line-height: 10pt; overflow-wrap: anywhere }</style><p>abcdefgh</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // WPT: css/css-text/word-break/word-break-keep-all-011.html.
    // CSS Sizing's min-content inline size must be computed from CSS Text
    // soft-wrap opportunities, not just from document whitespace.
    let lines = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
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
            "中文english中文，".to_string(),
            "english中文english".to_string()
        ]
    );
}

#[tokio::test]
async fn transparent_inline_padding_edge_allows_cjk_latin_wrap() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 120pt; margin: 10pt }\
         p { margin: 0; width: 40pt; font-size: 10pt; line-height: 10pt }\
         span { padding-left: 1pt; background: #ddd }\
         </style><p>中文<span>english</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(lines, ["中文", "english"]);
}

#[tokio::test]
async fn transparent_inline_padding_edge_preserves_wbr_opportunity() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt }\
         p { margin: 0; width: 20pt; font-size: 10pt; line-height: 10pt }\
         span { padding-left: 1pt }\
         </style><p>abc<wbr><span>def</span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(lines, ["abc", "def"]);
}

#[tokio::test]
async fn generated_zero_width_space_wraps_without_visible_text() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt }\
         p { margin: 0; width: 21pt; font-family: monospace; font-size: 10pt; line-height: 10pt }\
         span::before { content: \"\\200B\" }\
         </style><p>abc<span></span>def</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["abc", "def"]);
}

#[tokio::test]
async fn thai_wbr_min_content_uses_complete_named_entity_units() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 180pt; margin: 10pt }\
         p { margin: 0; width: min-content; font-size: 20pt; line-height: 24pt; word-break: normal }\
         </style><p lang=th>กรุงเทพ<wbr>คือ<wbr>สวยงาม</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        grouped_line_texts(&document.pages[0]),
        vec!["กรุงเทพ", "คือ", "สวยงาม"],
        "explicit virtual separators must split min-content at whole Thai units"
    );
}

#[tokio::test]
async fn pre_wrap_styled_boundary_trailing_spaces_hang_at_graph_break() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt }\
         p { margin: 0; width: 15pt; white-space: pre-wrap; font-family: monospace; font-size: 10pt; line-height: 10pt }\
         span { background: #ddd }\
         </style><p>AA<span>   </span>BB</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines, vec!["AA   ", "BB"]);
}

#[tokio::test]
async fn pre_wrap_terminal_space_sequence_uses_conditional_hanging_measure() {
    let ahem = format!(
        "file://{}/tests/fixtures/wpt/css/css-fonts/Ahem.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let document = Html::from_string(format!(
        "<style>@page {{ size: 200px 200px; margin: 0 }} @font-face {{ font-family: Ahem; src: url({ahem}) }} body {{ margin: 0 }} div {{ font: 10px/1 Ahem }} .test {{ color: green; width: 5ch; white-space: pre-wrap }}</style><div class=\"test\">XX<span>    </span><span>X  X  </span></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    assert_eq!(
        grouped_line_texts(&document.pages[0]),
        ["XX    ", "X  X  "],
        "the final preserved-space run must remain with the preceding X"
    );
}

#[tokio::test]
async fn inherits_css_text_breaking_controls() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } div { word-break: break-all } p { margin: 0; width: 18pt; font-size: 10pt; line-height: 10pt }</style><div><p>mnopqrst</p></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let boxes = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(10, 20, 30)))
        .collect::<Vec<_>>();

    assert_eq!(boxes.len(), 2);
    assert!(
        (boxes[0].height() - boxes[1].height()).abs() < 0.01,
        "soft and hard line breaks should consume the same line-box height: {boxes:?}"
    );
}

#[tokio::test]
async fn html_wbr_generated_before_creates_soft_wrap_opportunity() {
    let document = Html::from_string(
        "<style>@page { size: 80pt 120pt; margin: 10pt } p { margin: 0; width: 21pt; font-family: monospace; font-size: 10pt; line-height: 10pt }</style><p>abc<wbr>def</p>",
    )
    .render(&RenderOptions::default())
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
async fn wrap_inside_avoid_prefers_an_external_break_for_a_parenthetical_unit() {
    let ahem = format!(
        "file://{}/tests/fixtures/wpt/css/css-fonts/Ahem.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let document = Html::from_string(format!(
        "<style>@page {{ size: 90pt 120pt; margin: 10pt }} \
         @font-face {{ font-family: Ahem; src: url({ahem}) }} \
         p {{ margin: 0; width: 61pt; font: 10pt/10pt Ahem; word-break: break-all }} \
         .parenthetical {{ wrap-inside: avoid }}</style>\
         <p>aa<wbr><span class=\"parenthetical\">(bbbb)</span></p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(grouped_line_texts(&document.pages[0]), ["aa", "(bbbb)"]);
}

#[tokio::test]
async fn wrap_inside_avoid_relaxes_when_its_unit_cannot_fit_on_an_empty_line() {
    let ahem = format!(
        "file://{}/tests/fixtures/wpt/css/css-fonts/Ahem.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let document = Html::from_string(format!(
        "<style>@page {{ size: 90pt 120pt; margin: 10pt }} \
         @font-face {{ font-family: Ahem; src: url({ahem}) }} \
         p {{ margin: 0; width: 19pt; font: 10pt/10pt Ahem; word-break: break-all }} \
         .parenthetical {{ wrap-inside: avoid }}</style>\
         <p>aa<wbr><span class=\"parenthetical\">(bbbb)</span></p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = grouped_line_texts(&document.pages[0]);
    assert_eq!(lines.concat(), "aa(bbbb)");
    assert!(
        lines.len() > 2,
        "the over-wide avoided unit must relax: {lines:?}"
    );
}

#[tokio::test]
async fn nested_wrap_inside_avoid_prefers_breaking_the_outer_scope() {
    let ahem = format!(
        "file://{}/tests/fixtures/wpt/css/css-fonts/Ahem.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let document = Html::from_string(format!(
        "<style>@page {{ size: 90pt 140pt; margin: 10pt }} \
         @font-face {{ font-family: Ahem; src: url({ahem}) }} \
         p {{ margin: 0; width: 41pt; font: 10pt/10pt Ahem; word-break: break-all }} \
         .outer, .inner {{ wrap-inside: avoid }}</style>\
         <p>aa<br><span class=\"outer\">bb<wbr><span class=\"inner\">cccc</span></span></p>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(grouped_line_texts(&document.pages[0]), ["aa", "bb", "cccc"]);
}

#[tokio::test]
async fn hides_unbroken_soft_hyphens() {
    let document = Html::from_string(
        "<p style=\"margin: 0; font-size: 10pt; line-height: 10pt\">hyphen&shy;ation</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "hyphenation");
}

#[tokio::test]
async fn shows_soft_hyphens_when_line_breaks_there() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } p { margin: 0; width: 38pt; font-size: 10pt; line-height: 10pt }</style><p>hyphen&shy;ation</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(document.pages[0].lines().len() > 1);
    assert_eq!(document.pages[0].lines()[0].text, "hyphen‐");
    assert_eq!(
        document.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>(),
        "hyphen‐ation"
    );
}

#[tokio::test]
async fn auto_phrase_relaxes_authored_soft_hyphens_to_prevent_overflow() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } p { margin: 0; width: 0; font-size: 10pt; line-height: 10pt; word-break: auto-phrase; hyphens: manual }</style><p lang=en>con&shy;sid&shy;era&shy;tion</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["con‐", "sid‐", "era‐", "tion"]
    );
}

#[tokio::test]
async fn styled_inline_soft_hyphens_follow_manual_hyphenation() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } p { margin: 0; width: 38pt; font-size: 10pt; line-height: 10pt }</style><p><strong>hyphen&shy;</strong><em>ation</em></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "hyphen‐")
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "ation")
    );
}

#[tokio::test]
async fn hyphens_none_suppresses_soft_hyphen_breaks() {
    let document = Html::from_string(
        "<style>@page { size: 90pt 120pt; margin: 10pt } div { hyphens: none } p { margin: 0; width: 38pt; font-size: 10pt; line-height: 10pt }</style><div><p>hyphen&shy;ation</p></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines().len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "hyphenation");
}

#[tokio::test]
async fn hyphens_auto_uses_document_language() {
    let document = Html::from_string(
        "<html lang=\"en\"><style>@page { size: 90pt 120pt; margin: 10pt } p { hyphens: auto; margin: 0; width: 22pt; font-size: 10pt; line-height: 10pt }</style><p>ribonuclease</p></html>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.replace('‐', ""))
        .collect::<String>();

    assert!(document.pages[0].lines().len() > 1);
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text.ends_with('‐'))
    );
    assert_eq!(text, "ribonuclease");
}

#[tokio::test]
async fn break_spaces_exposes_breaks_before_atomic_inline_boxes() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 120pt; margin: 10pt } p { white-space: break-spaces; margin: 0; width: 14pt; font-size: 10pt; line-height: 10pt } span { display:inline-block; width:5pt }</style><p>A   <span>B</span></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let a = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A  ")
        .unwrap();
    let b = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .unwrap();

    assert!(a.y() > b.y());
}

#[tokio::test]
async fn overflow_wrap_applies_before_atomic_inline_boxes() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 120pt; margin: 10pt } p { overflow-wrap: anywhere; margin: 0; width: 18pt; font-size: 10pt; line-height: 10pt } span { display:inline-block; width:5pt }</style><p>abcdefgh<span>B</span></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let text_lines = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Text")
        .unwrap();

    assert!(document.pages[0].rects().iter().any(|rect| {
        rect.fill == Some(CssColor::BLACK)
            && (rect.x() - text.x()).abs() < 0.1
            && rect.y() < text.y()
            && rect.y() > text.y() - 4.0
            && rect.width() > 1.0
            && rect.height() >= 0.5
    }));
}

#[tokio::test]
async fn text_decoration_wavy_paints_as_paths() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 80pt; margin: 10pt } p { margin: 0; font-size: 16pt; text-decoration-line: underline; text-decoration-style: wavy; text-decoration-thickness: 2pt; text-decoration-skip-ink: none }</style><p>Wave</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(document.pages[0].paths().iter().any(|path| {
        path.stroke == Some(CssColor::BLACK)
            && path.stroke_width.points() >= 1.9
            && path.commands.len() > 2
    }));
}

#[tokio::test]
async fn spelling_and_grammar_error_decorations_paint_wavy_indicators() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } p { margin: 0; font-size: 16pt; line-height: 20pt }</style><p style=\"text-decoration-line: spelling-error\">Spell</p><p style=\"text-decoration-line: grammar-error\">Grammar</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .paths()
            .iter()
            .any(|path| path.stroke == Some(CssColor::new(255, 0, 0)))
    );
    assert!(
        document.pages[0]
            .paths()
            .iter()
            .any(|path| path.stroke == Some(CssColor::new(0, 128, 0)))
    );
}

#[tokio::test]
async fn text_decoration_skip_ink_splits_underlines_around_glyph_ink() {
    let without_skip = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } p { margin: 0; font-size: 18pt; text-decoration-line: underline; text-decoration-thickness: 2pt; text-decoration-skip-ink: none; text-underline-offset: -0.35em }</style><p>gap gap</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let with_skip = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } p { margin: 0; font-size: 18pt; text-decoration-line: underline; text-decoration-thickness: 2pt; text-decoration-skip-ink: auto; text-underline-offset: -0.35em }</style><p>gap gap</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let unskipped_width: f32 = without_skip.pages[0]
        .rects()
        .iter()
        .map(|rect| rect.width())
        .sum();
    let skipped_width: f32 = with_skip.pages[0]
        .rects()
        .iter()
        .map(|rect| rect.width())
        .sum();

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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = document
        .pages
        .first()
        .and_then(|page| {
            page.lines()
                .iter()
                .find(|line| line.text.contains("ABCDEF"))
        })
        .unwrap();
    let underline = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .max_by(|left, right| {
            left.width()
                .partial_cmp(&right.width())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();

    let line_start = line.x();
    let underline_start = underline.x();
    let underline_end = underline.x() + underline.width();
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
    assert!(underline.width() < line_width * 0.75);
}

#[tokio::test]
async fn vertical_text_decoration_underline_uses_logical_side() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 } \
         p { margin: 0; writing-mode: vertical-rl; width: 40pt; height: 80pt; \
         font-size: 12pt; line-height: 14pt; text-decoration-line: underline; \
         text-decoration-color: red; text-decoration-thickness: 2pt; \
         text-decoration-skip-ink: none; text-underline-position: left }</style><p>中文</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains('中'))
        .unwrap();
    assert!(
        document.pages[0].rects().iter().any(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0))
                && rect.height() > 10.0
                && rect.width() >= 1.5
                && rect.x() < text.x()
        }),
        "{:?}",
        (document.pages[0].rects().to_vec(), text)
    );
}

#[tokio::test]
async fn page_margin_text_decoration_uses_prepared_strokes() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 20pt; \
         @top-left { content: \"AB\"; font-size: 10pt; text-decoration-line: underline; \
         text-decoration-color: blue; text-decoration-skip-ink: none } } body { margin: 0 }</style>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 0, 255)) && rect.width() > 5.0),
        "{:?}",
        document.pages[0].rects()
    );
}

#[tokio::test]
async fn inline_block_vertical_text_decoration_uses_prepared_strokes() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 } \
         span { display: inline-block; writing-mode: vertical-rl; width: 40pt; height: 70pt; \
         font-size: 12pt; text-decoration-line: underline; text-decoration-color: green; \
         text-decoration-thickness: 2pt; text-decoration-skip-ink: none }</style><span>中A</span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0].rects().iter().any(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0)) && rect.height() > rect.width()
        }),
        "{:?}",
        (
            document.pages[0].rects().to_vec(),
            document.pages[0].lines().to_vec()
        )
    );
}

#[tokio::test]
async fn inline_block_text_decoration_paints_for_transparent_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 } \
         u { display: inline-block; width: 100px; font: 20px/1 sans-serif; color: transparent; \
         text-decoration: green underline; text-decoration-skip-ink: none; \
         text-underline-offset: 0; text-decoration-thickness: 100% }</style><u>X X</u>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0].rects().iter().any(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && rect.width() > 20.0
                && (rect.height() - 15.0).abs() < 0.1
        }),
        "expected green underline rects, got {:?}",
        (
            document.pages[0].rects().to_vec(),
            document.pages[0].lines().to_vec()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let first = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("XXXXXXXXXX"))
        .unwrap();
    let second = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Y")
        .unwrap();

    assert!(
        second.x() - first.x() < 45.0,
        "overflowing fixed-width child text must not widen the first float: first={}, second={}",
        first.x(),
        second.x()
    );
}

#[tokio::test]
async fn thick_overline_overflow_clip_retains_source_geometry() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 100pt; margin: 10pt } body { margin: 0 }\
         #box { font-size: 20px; line-height: 20px; overflow: hidden; height: 1em; width: 4em; background: red }\
         #text { color: transparent; position: relative; top: 3em; text-decoration: green overline; text-decoration-skip-ink: none; text-decoration-thickness: 4em }\
         </style><div id=\"box\"><div id=\"text\">XXXXXXXXXXXXXXXXXXXX</div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();

    assert_eq!(green_rects.len(), 1);
    assert!(
        green_rects[0].width() > 200.0 && (green_rects[0].height() - 60.0).abs() < 0.01,
        "the retained overflow clip must leave the overline source geometry intact: {:?}",
        green_rects[0]
    );
}

#[tokio::test]
async fn font_shorthand_unit_line_height_sets_inline_background_height() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 0 } body { margin: 0 }\
         div { font: 50px/1 sans-serif; width: 2em; background: green; color: green }</style>\
         <div>&#x3000;&#x3000;XX</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("green background should paint");

    assert!((green.width() - 75.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn max_content_inline_intrinsic_size_keeps_zero_percent_calc_edges() {
    let document = Html::from_string(
        "<!doctype html>\
         <style>@page { size: 240pt 120pt; margin: 0 } body { margin: 0 }\
         div { background-color: green; width: max-content; font: 20px/1 sans-serif }\
         span { margin-left: calc(0% + 30px); padding-left: calc(0% + 50px) }</style>\
         <div><span>ABCD</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("green max-content background should paint");
    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "ABCD")
        .expect("text should paint");
    let text_width = rendered_line_advance(line);
    let expected_edge_space = 60.0;

    assert!(
        (line.x() - green.x() - expected_edge_space).abs() < 0.01,
        "text should start after the calc margin and padding: green={green:?}, line={line:?}"
    );
    assert!(
        (green.width() - text_width - expected_edge_space).abs() < 0.01,
        "max-content background should include calc margin and padding: green={green:?}, line={line:?}, text_width={text_width}"
    );
}

#[tokio::test]
async fn negative_leading_baseline_inline_uses_line_height_for_line_box() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 80pt; margin: 0 } html, body { margin: 0 }\
         div { margin: 20pt 0 0 0; color: transparent; background: blue;\
             line-height: 10pt; font-size: 30pt; font-family: monospace }\
         span { background: purple }</style>\
         <div><span>XX</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap_or_else(|| panic!("expected block line box background: {:?}", page.rects()));
    assert!(
        (blue.height() - 10.0).abs() < 0.01,
        "baseline-aligned negative-leading text should contribute its 10pt line-height, not ink/content height: {blue:?}"
    );

    let purple = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(128, 0, 128)))
        .unwrap_or_else(|| panic!("expected inline content background: {:?}", page.rects()));
    assert!(
        purple.height() > 25.0,
        "inline content background should still overflow the smaller line box: {purple:?}"
    );
}

#[tokio::test]
async fn vertical_align_edge_values_do_not_expand_negative_leading_line_boxes() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 240pt; margin: 0 } html, body { margin: 0 }\
         .container { margin: 20pt 0 0 0; width: 200pt; color: orange;\
             background: blue; line-height: 10pt; font-size: 30pt; font-family: monospace }\
         span { background: purple }\
         .top { vertical-align: top }\
         .bottom { vertical-align: bottom }\
         .text-top { vertical-align: text-top }\
         .text-bottom { vertical-align: text-bottom }</style>\
         <div class=\"container\"><span class=\"top\">XX</span> <span>XX</span> <span class=\"bottom\">XX</span></div>\
         <div class=\"container\"><span class=\"text-top\">XX</span> <span>XX</span> <span class=\"text-bottom\">XX</span></div>\
         <div class=\"container\"><span class=\"top\">XX</span></div>\
         <div class=\"container\"><span class=\"bottom\">XX</span></div>\
         <div class=\"container\"><span class=\"text-top\">XX</span></div>\
         <div class=\"container\"><span class=\"text-bottom\">XX</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let blue_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();
    assert_eq!(blue_rects.len(), 6, "{blue_rects:?}");
    let mut blue_rows = blue_rects;
    blue_rows.sort_by(|left, right| right.y().total_cmp(&left.y()));
    let expected_blue_heights = [10.0, 30.0, 10.0, 10.0, 20.0, 20.0];
    for (row_index, (rect, expected_height)) in
        blue_rows.iter().zip(expected_blue_heights).enumerate()
    {
        assert!(
            (rect.height() - expected_height).abs() < 0.01,
            "row {} should match WPT reference line height {expected_height}: {rect:?}",
            row_index + 1
        );
    }

    let purple_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(128, 0, 128)))
        .collect::<Vec<_>>();
    assert!(
        purple_rects.len() >= 10,
        "expected span backgrounds for every aligned run: {purple_rects:?}"
    );
    assert!(
        purple_rects.iter().all(|rect| rect.height() > 25.0),
        "negative-leading inline content should overflow the 10pt line boxes: {purple_rects:?}"
    );
    let second_row = blue_rows[1];
    let second_row_mid_y = second_row.y() + second_row.height() / 2.0;
    let mut second_row_purple = purple_rects
        .iter()
        .copied()
        .filter(|rect| {
            rect.y() - 0.01 <= second_row_mid_y
                && second_row_mid_y <= rect.y() + rect.height() + 0.01
        })
        .collect::<Vec<_>>();
    second_row_purple.sort_by(|left, right| left.x().total_cmp(&right.x()));
    assert_eq!(
        second_row_purple.len(),
        3,
        "expected three span backgrounds crossing the second row midpoint: second_row={second_row:?}, purple_rects={purple_rects:?}"
    );
    let reference = second_row_purple[0];
    for (index, rect) in second_row_purple.iter().enumerate().skip(1) {
        assert!(
            (rect.y() - reference.y()).abs() < 0.01
                && ((rect.y() + rect.height()) - (reference.y() + reference.height())).abs() < 0.01,
            "second row span {} should share vertical edges with the first span: reference={reference:?}, rect={rect:?}, second_row={second_row:?}",
            index + 1
        );
    }
    let single_span_lines = page
        .lines()
        .iter()
        .filter(|line| {
            line.color == CssColor::new(255, 165, 0)
                && line.text.trim() == "XX"
                && rendered_line_baseline_top(&document, line) < 180.0
        })
        .collect::<Vec<_>>();
    assert!(
        single_span_lines.len() >= 4,
        "expected visible text runs for the single-span aligned rows: {single_span_lines:?}"
    );
}

#[tokio::test]
async fn text_top_bottom_paint_to_parent_content_edges_with_mixed_font_sizes() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 140pt; margin: 0 } html, body { margin: 0 }\
         .container { margin: 20pt 0 0 0; width: 180pt; color: transparent;\
             background: blue; line-height: 10pt; font-size: 30pt; font-family: monospace }\
         .reference { background: green }\
         .small { background: purple; font-size: 10pt; line-height: 10pt }\
         .text-top { vertical-align: text-top }\
         .text-bottom { vertical-align: text-bottom }</style>\
         <div class=\"container\"><span class=\"reference\">XX</span><span class=\"small text-top\">XX</span></div>\
         <div class=\"container\"><span class=\"reference\">XX</span><span class=\"small text-bottom\">XX</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let blue_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();
    assert_eq!(blue_rects.len(), 2, "{blue_rects:?}");

    let mut reference_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    let mut small_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(128, 0, 128)))
        .collect::<Vec<_>>();
    reference_rects.sort_by(|left, right| right.y().total_cmp(&left.y()));
    small_rects.sort_by(|left, right| right.y().total_cmp(&left.y()));
    small_rects.dedup_by(|left, right| {
        (left.x() - right.x()).abs() < 0.01
            && (left.y() - right.y()).abs() < 0.01
            && (left.width() - right.width()).abs() < 0.01
            && (left.height() - right.height()).abs() < 0.01
    });
    assert_eq!(reference_rects.len(), 2, "{reference_rects:?}");
    assert_eq!(small_rects.len(), 2, "{small_rects:?}");

    let top_reference = reference_rects[0];
    let top_small = small_rects[0];
    assert!(
        ((top_small.y() + top_small.height()) - (top_reference.y() + top_reference.height())).abs()
            < 0.01,
        "text-top child content top should match parent content top: reference={top_reference:?}, small={top_small:?}"
    );

    let bottom_reference = reference_rects[1];
    let bottom_small = small_rects[1];
    assert!(
        (bottom_small.y() - bottom_reference.y()).abs() < 0.01,
        "text-bottom child content bottom should match parent content bottom: reference={bottom_reference:?}, small={bottom_small:?}"
    );
}

#[tokio::test]
async fn explicit_line_height_overrides_loaded_font_metrics() {
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
    if !std::path::Path::new(ahem).exists() {
        return;
    }

    let document = Html::from_string(format!(
        "<style>@page {{ size: 200pt 200pt; margin: 0 }} body {{ margin: 0 }}\
             @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
             div {{ font: 50px/1 Ahem; width: 2em; background: green; color: green }}</style>\
             <div>&#x3000;&#x3000;XX</div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("green background should paint");

    assert!((green.width() - 75.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn trailing_ideographic_space_hangs_and_paints_inline_background() {
    let ahem = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wpt/css/css-fonts/Ahem.ttf"
    );

    let document = Html::from_string(format!(
        "<style>@page {{ size: 200pt 200pt; margin: 0 }} body {{ margin: 0 }}\
             @font-face {{ font-family: Ahem; src: url(file://{ahem}) }}\
             div {{ font: 50px/1 Ahem; width: 1ch }}\
             span {{ background: green; color: transparent; unicode-bidi: plaintext; hyphens: none }}</style>\
             <div><span>X&#x3000;<br>XX</span></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    assert!(
        green_rects
            .iter()
            .all(|rect| (rect.width() - 75.0).abs() < 0.01 && (rect.height() - 37.5).abs() < 0.01),
        "{green_rects:?}"
    );
    assert!(
        ((green_rects[0].y() - green_rects[1].y()).abs() - 37.5).abs() < 0.01,
        "{green_rects:?}"
    );
}

#[tokio::test]
async fn combining_grapheme_joiner_suppresses_wrap_before_atomic_inline() {
    let ahem = "/Users/lee/oss/spindrift-wpt/third_party/wpt/fonts/Ahem.ttf";
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red_background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("div background should paint");
    let visible_green = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.color == CssColor::new(0, 128, 0))
        .expect("visible Ahem A should paint green");
    let (green_left, green_right) = rendered_line_visual_bounds(visible_green);

    assert!(
        (red_background.width() - 75.0).abs() < 0.01,
        "{red_background:?}"
    );
    assert!(
        (red_background.height() - 75.0).abs() < 0.01,
        "{red_background:?}"
    );
    assert!(
        (green_left - red_background.x()).abs() < 0.01
            && (green_right - red_background.x() - red_background.width()).abs() < 0.01,
        "green glyph should cover the red square horizontally: red={red_background:?}, green_line={visible_green:?}, bounds=({green_left}, {green_right})"
    );
}
