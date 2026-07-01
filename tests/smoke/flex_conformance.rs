use super::*;

const DISPLAY_CONTENTS_FLEX_ACID_CSS: &str = "\
html, body { color: black; background-color: white; font-size: 16px; padding: 0; margin: 0 }\
body { color: red }\
.flex { display: flex }\
.ib { display: inline-block }\
.inline { display: inline }\
.contents { display: contents; align-items: inherit; justify-items: inherit }\
.c1 { color: lime }\
.c2 { background: blue; color: pink }\
.c3 { color: teal }\
.c4 { color: green }\
.c5 { color: silver }\
.c6 { color: cyan }\
.c7 { color: magenta }\
.c9 { color: grey }\
.c10 { color: black }\
.b { background: inherit }";

#[tokio::test]
async fn stretched_vertical_flex_item_defines_descendant_percentage_padding_basis() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 100px 100px; margin: 0 } body { margin: 0 }\
         x-flexbox { display: flex; height: 100px }\
         x-item { background: green; writing-mode: vertical-lr }\
         x-item > div { padding-right: 70%; width: 30px }\
         </style><x-flexbox><x-item><div></div></x-item></x-flexbox>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap_or_else(|| {
            panic!(
                "expected green flex item background: {:?}",
                document.pages[0].rects
            )
        });
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "stretched vertical item should use its 100px inline size for descendant percentage padding: {green:?}"
    );
}

#[tokio::test]
async fn generated_after_pseudo_participates_as_flex_item() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 420pt 140pt; margin: 10pt }
body { margin: 0 }
div {
    background: #3366cc;
    display: flex;
}
div::after, p {
    content: "xxx";
    background: yellow;
    margin: 1em;
    width: 200px;
    height: 2em;
}
div::after {
    content: "yyy";
    display: block;
}
</style>
<div>
    <p>FAIL</p>
</div>"#,
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let yellow = page
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 255, 0)))
        .collect::<Vec<_>>();
    assert_eq!(
        yellow.len(),
        2,
        "real child and generated ::after should both paint yellow flex item backgrounds: {:?}",
        page.rects
    );
    assert!(
        yellow[1].x() > yellow[0].x() + yellow[0].width(),
        "generated ::after flex item should be laid out after the real child: {yellow:?}"
    );
    assert!(
        page.lines.iter().any(|line| line.text == "yyy"),
        "generated ::after text should render as flex item content: {:?}",
        page.lines
    );
}

#[tokio::test]
async fn generated_pseudo_flex_items_participate_in_order_sorting() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin: 0 }\
         div { display: flex; width: 90pt; font-size: 10pt; line-height: 10pt }\
         span, div::before, div::after { display: block; width: 20pt; height: 10pt }\
         div::before { content: 'A'; order: 3 }\
         span { order: 2 }\
         div::after { content: 'C'; order: 1 }</style>\
         <div><span>B</span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut line_text_by_x = document.pages[0]
        .lines
        .iter()
        .map(|line| (line.x(), line.text.as_str()))
        .collect::<Vec<_>>();
    line_text_by_x.sort_by(|left, right| left.0.total_cmp(&right.0));
    let text = line_text_by_x
        .iter()
        .map(|(_, text)| *text)
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        vec!["C", "B", "A"],
        "pseudo and element flex items should share order-modified document order: {:?}",
        document.pages[0].lines
    );
}

#[tokio::test]
async fn definite_width_block_flex_container_overflows_without_shrinking_items() {
    let document = Html::from_string(
        "<style>@page { size: 500pt 160pt; margin: 20pt } body { margin: 0 }\
         .row { display: flex; width: 480pt; height: 40pt; background: blue }\
         .item { flex: 0 1 auto; width: 60pt; height: 20pt }\
         .a { background: yellow } .b { background: pink }\
         .c { background: lightblue } .d { background: gray }</style>\
         <div class=\"row\"><span class=\"item a\">one</span><span class=\"item b\">two</span><span class=\"item c\">three</span><span class=\"item d\">four</span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let container = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("definite-width flex container background should paint");
    assert!(
        (container.width() - 480.0).abs() < 0.01,
        "definite-width block flex container should overflow instead of clamping to the 460pt containing block: {container:?}"
    );

    for color in [
        Color::new(255, 255, 0),
        Color::new(255, 192, 203),
        Color::new(173, 216, 230),
        Color::new(128, 128, 128),
    ] {
        let item = page
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("expected flex item with fill {color:?}: {:?}", page.rects));
        assert!(
            (item.width() - 60.0).abs() < 0.01,
            "flex: 0 1 auto item should keep its definite width: {item:?}"
        );
    }
}

#[tokio::test]
async fn fixed_width_block_flex_container_resolves_auto_margins() {
    let document = Html::from_string(
        "<style>@page { size: 300pt 120pt; margin: 20pt } body { margin: 0 }\
         .row { display: flex; width: 100pt; height: 20pt; margin-left: auto; margin-right: auto; background: green }\
         .item { width: 20pt; height: 20pt }</style>\
         <div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("centered flex container background should paint");
    assert!(
        (green.x() - 100.0).abs() < 0.01 && (green.width() - 100.0).abs() < 0.01,
        "fixed-width block flex container should resolve auto margins like normal block layout: {green:?}"
    );
}

#[tokio::test]
async fn align_content_stretch_overflow_falls_back_to_wrap_reverse_flex_start() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>@page { size: 200px 200px; margin: 0 } body { margin: 0 } p { display: none }\
         #flex { display: flex; width: 200px; height: 200px; flex-wrap: wrap-reverse; align-content: stretch }\
         #item { width: 200px; height: 400px; background: linear-gradient(to bottom, red 50%, green 50%) }</style>\
         <p>Test passes if there is a filled green square and no red.</p>\
         <div style=\"overflow: hidden\"><div id=\"flex\"><div id=\"item\"></div></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = Color::new(0, 128, 0);
    for (x, y) in [
        (15.0, 15.0),
        (75.0, 15.0),
        (135.0, 15.0),
        (15.0, 75.0),
        (75.0, 75.0),
        (135.0, 75.0),
        (15.0, 135.0),
        (75.0, 135.0),
        (135.0, 135.0),
    ] {
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(green),
            "align-content:stretch overflow fallback should expose only the green half at ({x}, {y}): {:?}",
            page.rects
        );
    }
}

#[tokio::test]
async fn column_wrap_flex_min_content_width_uses_item_cross_contribution() {
    let document = Html::from_string(
        "<style>@page { size: 300px 200px; margin: 0 } body { margin: 0 }\
         #reference-overlapped-red { position: absolute; background-color: red; width: 100px; height: 100px; z-index: -1 }\
         .item { min-height: 0px; width: 100px; flex: 0 0 auto }\
         .flex { display: flex; flex-flow: column wrap; height: 100px; width: min-content; background: green }\
         </style><div id=\"reference-overlapped-red\"></div><div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("min-content flex container background should paint");
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "100px min-content flex container should paint as a 75pt square: {green:?}"
    );
    assert_eq!(
        final_rect_fill_at(
            page,
            green.x() + green.width() / 2.0,
            green.y() + green.height() / 2.0
        ),
        Some(Color::new(0, 128, 0)),
        "green flex container should fully cover the red reference square: {:?}",
        page.rects
    );
}

#[tokio::test]
async fn column_wrap_inline_flex_min_content_width_uses_item_cross_contribution() {
    let document = Html::from_string(
        "<style>@page { size: 300px 200px; margin: 0 } body { margin: 0 }\
         .item { min-height: 0px; width: 100px; flex: 0 0 auto }\
         .flex { display: inline-flex; flex-flow: column wrap; height: 100px; width: min-content; background: green; vertical-align: top }\
         </style><span class=\"flex\"><span class=\"item\"></span><span class=\"item\"></span></span>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("inline-flex background should paint");
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "100px inline-flex min-content width should paint as 75pt: {green:?}"
    );
}

#[tokio::test]
async fn column_wrap_flex_min_content_width_does_not_sum_wrapped_columns() {
    let document = Html::from_string(
        "<style>@page { size: 300px 200px; margin: 0 } body { margin: 0 }\
         .item { width: 100px; height: 100px; flex: 0 0 auto }\
         .flex { display: flex; flex-flow: column wrap; height: 100px; width: min-content; background: green }\
         </style><div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("min-content column flex background should paint");
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "column flex min-content cross size should be the largest item contribution, not the sum of wrapped columns: {green:?}"
    );
}

#[tokio::test]
async fn wrapped_row_flex_min_content_width_uses_largest_item_contribution() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; flex-wrap: wrap; width: min-content; column-gap: 5pt; background: green }\
         .a { width: 40pt; height: 10pt } .b { width: 30pt; height: 10pt }\
         </style><div class=\"flex\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("min-content wrapped flex background should paint");
    assert!(
        (green.width() - 40.0).abs() < 0.01,
        "wrapped row min-content width should be the largest item, not the sum plus gap: {green:?}"
    );
}

#[tokio::test]
async fn floated_row_flex_min_content_caps_non_growing_item_by_flex_base() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 300px 180px; margin: 0 } body { margin: 0 }\
         #reference-overlapped-red { position: absolute; background-color: red; width: 100px; height: 100px; z-index: -1 }\
         </style>\
         <p>Test passes if there is a filled green square and <strong>no red</strong>.</p>\
         <div id=\"reference-overlapped-red\"></div>\
         <div style=\"width: 0px\">\
         <div style=\"display: flex; background: green; height: 100px; float: left\">\
         <div style=\"flex: 0 1 100px; min-width: 0px\"><div style=\"width: 200px\"></div></div>\
         </div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("floated flex container background should paint");
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "non-growing item should cap min-content contribution at its 100px flex base: {green:?}"
    );
    assert_eq!(
        final_rect_fill_at(
            page,
            green.x() + green.width() / 2.0,
            green.y() + green.height() / 2.0
        ),
        Some(Color::new(0, 128, 0)),
        "green flex container should fully cover the red reference square: {:?}",
        page.rects
    );
}

#[tokio::test]
async fn nowrap_row_flex_min_content_width_keeps_all_items_on_one_line() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; flex-wrap: nowrap; width: min-content; column-gap: 5pt; background: green }\
         .a { width: 40pt; height: 10pt } .b { width: 30pt; height: 10pt }\
         </style><div class=\"flex\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("min-content nowrap flex background should paint");
    assert!(
        (green.width() - 75.0).abs() < 0.01,
        "single-line row min-content width should include both item contributions and the gap: {green:?}"
    );
}

#[tokio::test]
async fn flex_intrinsic_width_percentage_gap_contributes_only_length_component() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; width: max-content; column-gap: calc(10pt + 50%); background: green }\
         .item { width: 20pt; height: 10pt }\
         </style><div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("max-content flex background should paint");
    assert!(
        (green.width() - 50.0).abs() < 0.01,
        "intrinsic flex width should resolve the cyclic percentage gap against zero: {green:?}"
    );
}

#[tokio::test]
async fn flex_max_content_width_uses_growing_item_max_content_contributions() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 120pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; width: max-content; background: green }\
         .a { flex: 2 1 20pt; width: 60pt; height: 10pt }\
         .b { flex: 1 1 10pt; width: 40pt; height: 10pt }\
         </style><div class=\"flex\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("max-content flex background should paint");
    assert!(
        (green.width() - 100.0).abs() < 0.01,
        "max-content flex width should use item max-content contributions instead of the 30pt flex-base sum: {green:?}"
    );
}

#[tokio::test]
async fn visibility_collapse_keeps_cross_strut_without_painting_item() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; width: 90pt; background: black; align-items: flex-start }\
         .a { width: 20pt; height: 10pt; background: green }\
         .collapsed { visibility: collapse; width: 20pt; height: 36pt; background: red }\
         .b { width: 20pt; height: 8pt; background: blue }</style>\
         <div class=\"row\"><div class=\"a\"></div><div class=\"collapsed\"></div><div class=\"b\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.rects
            .iter()
            .all(|rect| rect.fill != Some(Color::new(255, 0, 0))),
        "collapsed flex item should not paint: {:?}",
        page.rects
    );
    let container = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK) && rect.width() >= 89.0)
        .expect("flex container background should paint");
    assert!(
        container.height() >= 35.9,
        "collapsed flex item should leave a cross-size strut: {container:?}"
    );
}

#[tokio::test]
async fn visibility_collapse_strut_reflows_wrapped_lines() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-wrap: wrap; align-content: flex-start; width: 50pt }\
         .item { flex: 0 0 50pt; width: 50pt; height: 10pt }\
         .a { background: green }\
         .collapsed { visibility: collapse; height: 60pt; background: red }\
         .c { background: yellow }\
         .d { background: blue }</style>\
         <div class=\"row\"><div class=\"item a\"></div><div class=\"item collapsed\"></div><div class=\"item c\"></div><div class=\"item d\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.rects
            .iter()
            .all(|rect| rect.fill != Some(Color::new(255, 0, 0))),
        "collapsed flex item should not paint: {:?}",
        page.rects
    );
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("first flex line item should paint");
    let blue = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("second flex line item should paint");
    assert!(
        green.y() - blue.y() >= 65.0,
        "collapsed strut should repack later wrapped lines: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_row_items_share_text_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; align-items: baseline; width: 140pt }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt }\
         p { margin: 0 }</style>\
         <div class=\"row\"><p class=\"big\">A</p><p class=\"small\">B</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let big = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("large baseline participant should render");
    let small = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("small baseline participant should render");
    assert!(
        (big.y() - small.y()).abs() < 0.01,
        "baseline-aligned flex items should share a baseline: big={big:?}, small={small:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_row_items_use_nested_flex_exported_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: baseline; width: 140pt }\
         .nested { display: flex }\
         p { margin: 0 }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"big\">A</p></div><p class=\"small\">B</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("nested flex baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("peer baseline participant should render");
    assert!(
        (nested.y() - peer.y()).abs() < 0.01,
        "nested flex exported baseline should join the outer baseline group: nested={nested:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_row_items_use_nested_wrapped_flex_first_line_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: baseline; width: 140pt }\
         .nested { display: flex; flex-wrap: wrap; width: 40pt }\
         .item { flex: 0 0 40pt; margin: 0 }\
         p { margin: 0 }\
         .small { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item small\">A</p><p class=\"item big\">B</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("nested wrapped flex first baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer baseline participant should render");
    assert!(
        (nested_first.y() - peer.y()).abs() < 0.01,
        "nested wrapped flex first exported baseline should come from its first line: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_row_items_use_nested_row_reverse_wrapped_startmost_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 180pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: baseline; width: 160pt }\
         .nested { display: flex; flex-direction: row-reverse; flex-wrap: wrap; width: 60pt }\
         .item { flex: 0 0 30pt; margin: 0 }\
         p { margin: 0 }\
         .small { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item small\">A</p><p class=\"item big\">B</p><p class=\"item small\">D</p><p class=\"item small\">E</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_startmost = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("nested row-reverse first-line startmost baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer baseline participant should render");
    assert!(
        (nested_startmost.y() - peer.y()).abs() < 0.01,
        "nested row-reverse exported first baseline should come from the first line's startmost item: nested_startmost={nested_startmost:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_row_items_use_nested_wrap_reverse_startmost_line_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 180pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: baseline; width: 160pt }\
         .nested { display: flex; flex-wrap: wrap-reverse; width: 40pt }\
         .item { flex: 0 0 40pt; margin: 0 }\
         p { margin: 0 }\
         .small { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item small\">A</p><p class=\"item big\">B</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_startmost = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("nested wrap-reverse startmost line baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer baseline participant should render");
    assert!(
        (nested_startmost.y() - peer.y()).abs() < 0.01,
        "nested wrap-reverse exported first baseline should come from the cross-startmost line: nested_startmost={nested_startmost:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_row_items_use_nested_max_width_wrapped_first_line_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 180pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: baseline; width: 160pt }\
         .nested { display: flex; flex-wrap: wrap; max-width: 40pt }\
         .item { flex: 0 0 40pt; margin: 0 }\
         p { margin: 0 }\
         .small { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item small\">A</p><p class=\"item big\">B</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("nested max-width wrapped first baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer baseline participant should render");
    assert!(
        (nested_first.y() - peer.y()).abs() < 0.01,
        "max-width-constrained nested flex should export the first wrapped line baseline: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_row_items_use_nested_fit_content_wrapped_first_line_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 180pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: baseline; width: 160pt }\
         .nested { display: flex; flex-wrap: wrap; width: fit-content(40pt) }\
         .item { flex: 0 0 40pt; margin: 0 }\
         p { margin: 0 }\
         .small { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item small\">A</p><p class=\"item big\">B</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("nested fit-content wrapped first baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer baseline participant should render");
    assert!(
        (nested_first.y() - peer.y()).abs() < 0.01,
        "fit-content-constrained nested flex should export the first wrapped line baseline: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_row_items_use_nested_flex_exported_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: last baseline; width: 140pt }\
         .nested { display: flex }\
         p { margin: 0 }\
         .two { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"two\">A<br>B</p></div><p class=\"big\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("nested flex last baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer last baseline participant should render");
    assert!(
        (nested_last.y() - peer.y()).abs() < 0.01,
        "nested flex exported last baseline should join the outer baseline group: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_row_items_use_nested_fit_content_wrapped_last_line_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 180pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: last baseline; width: 160pt }\
         .nested { display: flex; flex-wrap: wrap; width: fit-content(40pt) }\
         .item { flex: 0 0 40pt; margin: 0 }\
         p { margin: 0 }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item big\">A</p><p class=\"item small\">B</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("nested fit-content wrapped last baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer last baseline participant should render");
    assert!(
        (nested_last.y() - peer.y()).abs() < 0.01,
        "fit-content-constrained nested flex should export the last wrapped line baseline: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_row_items_use_nested_max_content_gap_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: last baseline; width: 180pt }\
         .nested { display: flex; flex-wrap: wrap; width: max-content; column-gap: 10pt }\
         .item { flex: 0 0 40pt; margin: 0 }\
         p { margin: 0 }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item big\">A</p><p class=\"item small\">B</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_endmost = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("nested max-content gap baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer last baseline participant should render");
    assert!(
        (nested_endmost.y() - peer.y()).abs() < 0.01,
        "max-content nested flex should include main-axis gaps when exporting the same-line last baseline: nested_endmost={nested_endmost:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_row_items_use_nested_max_width_wrapped_last_line_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 180pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: last baseline; width: 160pt }\
         .nested { display: flex; flex-wrap: wrap; max-width: 40pt }\
         .item { flex: 0 0 40pt; margin: 0 }\
         p { margin: 0 }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item big\">A</p><p class=\"item small\">B</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("nested max-width wrapped last baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer last baseline participant should render");
    assert!(
        (nested_last.y() - peer.y()).abs() < 0.01,
        "max-width-constrained nested flex should export the last wrapped line baseline: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_row_items_use_nested_wrap_reverse_endmost_line_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 180pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: last baseline; width: 160pt }\
         .nested { display: flex; flex-wrap: wrap-reverse; width: 40pt }\
         .item { flex: 0 0 40pt; margin: 0 }\
         p { margin: 0 }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item small\">A</p><p class=\"item small\">B</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_endmost = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("nested wrap-reverse endmost line baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer last baseline participant should render");
    assert!(
        (nested_endmost.y() - peer.y()).abs() < 0.01,
        "nested wrap-reverse exported last baseline should come from the cross-endmost line: nested_endmost={nested_endmost:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_row_items_use_nested_row_reverse_wrapped_endmost_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 180pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: last baseline; width: 160pt }\
         .nested { display: flex; flex-direction: row-reverse; flex-wrap: wrap; width: 60pt }\
         .item { flex: 0 0 30pt; margin: 0 }\
         p { margin: 0 }\
         .small { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item small\">A</p><p class=\"item small\">B</p><p class=\"item big\">D</p><p class=\"item small\">E</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_endmost = page
        .lines
        .iter()
        .find(|line| line.text == "E")
        .expect("nested row-reverse last-line endmost baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer last baseline participant should render");
    assert!(
        (nested_endmost.y() - peer.y()).abs() < 0.01,
        "nested row-reverse exported last baseline should come from the last line's endmost item: nested_endmost={nested_endmost:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_row_items_use_nested_wrapped_flex_last_line_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; align-items: last baseline; width: 140pt }\
         .nested { display: flex; flex-wrap: wrap; width: 40pt }\
         .item { flex: 0 0 40pt; margin: 0 }\
         p { margin: 0 }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item big\">A</p><p class=\"item small\">B</p></div><p class=\"small\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("nested wrapped flex last baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer last baseline participant should render");
    assert!(
        (nested_last.y() - peer.y()).abs() < 0.01,
        "nested wrapped flex last exported baseline should come from its last line: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_items_use_nested_vertical_flex_exported_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                   width: 80pt; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("nested vertical first baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical baseline participant should render");
    assert!(
        (nested_first.x() - peer.x()).abs() < 0.01,
        "nested vertical flex exported first horizontal baseline should join the outer group: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_vertical_row_items_use_nested_vertical_flex_exported_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: last baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                   width: 80pt; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("nested vertical last baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical last baseline participant should render");
    assert!(
        (nested_last.x() - peer.x()).abs() < 0.01,
        "nested vertical flex exported last horizontal baseline should join the outer group: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_items_use_auto_width_nested_vertical_exported_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                   height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("auto-width nested vertical first baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical first baseline participant should render");
    assert!(
        (nested_first.x() - peer.x()).abs() < 0.01,
        "auto-width nested vertical flex exported first horizontal baseline should join the outer group: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_vertical_row_items_use_auto_width_nested_vertical_exported_baseline()
{
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: last baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                   height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("auto-width nested vertical last baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical last baseline participant should render");
    assert!(
        (nested_last.x() - peer.x()).abs() < 0.01,
        "auto-width nested vertical flex exported last horizontal baseline should join the outer group: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_items_use_nested_wrap_reverse_exported_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap-reverse; writing-mode: vertical-lr;\
                   width: 80pt; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("nested vertical wrap-reverse first baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical first baseline participant should render");
    assert!(
        (nested_first.x() - peer.x()).abs() < 0.01,
        "nested vertical wrap-reverse flex exported first horizontal baseline should join the outer group: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_vertical_row_items_use_nested_wrap_reverse_exported_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: last baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap-reverse; writing-mode: vertical-lr;\
                   width: 80pt; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("nested vertical wrap-reverse last baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical last baseline participant should render");
    assert!(
        (nested_last.x() - peer.x()).abs() < 0.01,
        "nested vertical wrap-reverse flex exported last horizontal baseline should join the outer group: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_items_use_auto_width_nested_wrap_reverse_exported_baseline()
{
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap-reverse; writing-mode: vertical-lr;\
                   height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first =
        page.lines.iter().find(|line| line.text == "A").expect(
            "auto-width nested vertical wrap-reverse first baseline participant should render",
        );
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical first baseline participant should render");
    assert!(
        (nested_first.x() - peer.x()).abs() < 0.01,
        "auto-width nested vertical wrap-reverse flex exported first horizontal baseline should join the outer group: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_vertical_row_items_use_auto_width_nested_wrap_reverse_exported_baseline()
 {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: last baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap-reverse; writing-mode: vertical-lr;\
                   height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last =
        page.lines.iter().find(|line| line.text == "B").expect(
            "auto-width nested vertical wrap-reverse last baseline participant should render",
        );
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical last baseline participant should render");
    assert!(
        (nested_last.x() - peer.x()).abs() < 0.01,
        "auto-width nested vertical wrap-reverse flex exported last horizontal baseline should join the outer group: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_items_use_percentage_width_nested_vertical_exported_baseline()
 {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                   width: 50%; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("percentage-width nested vertical first baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical first baseline participant should render");
    assert!(
        (nested_first.x() - peer.x()).abs() < 0.01,
        "percentage-width nested vertical flex exported first horizontal baseline should join the outer group: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_vertical_row_items_use_percentage_width_nested_vertical_exported_baseline()
 {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: last baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                   width: 50%; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("percentage-width nested vertical last baseline participant should render");
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical last baseline participant should render");
    assert!(
        (nested_last.x() - peer.x()).abs() < 0.01,
        "percentage-width nested vertical flex exported last horizontal baseline should join the outer group: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_items_use_percentage_width_nested_wrap_reverse_exported_baseline()
 {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap-reverse; writing-mode: vertical-lr;\
                   width: 50%; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page.lines.iter().find(|line| line.text == "A").expect(
        "percentage-width nested vertical wrap-reverse first baseline participant should render",
    );
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical first baseline participant should render");
    assert!(
        (nested_first.x() - peer.x()).abs() < 0.01,
        "percentage-width nested vertical wrap-reverse flex exported first horizontal baseline should join the outer group: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_vertical_row_items_use_percentage_width_nested_wrap_reverse_exported_baseline()
 {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: flex; flex-direction: row; align-items: last baseline; writing-mode: vertical-lr;\
                  width: 120pt; height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap-reverse; writing-mode: vertical-lr;\
                   width: 50%; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page.lines.iter().find(|line| line.text == "B").expect(
        "percentage-width nested vertical wrap-reverse last baseline participant should render",
    );
    let peer = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical last baseline participant should render");
    assert!(
        (nested_last.x() - peer.x()).abs() < 0.01,
        "percentage-width nested vertical wrap-reverse flex exported last horizontal baseline should join the outer group: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn align_content_baseline_packs_wrapped_row_line_baselines() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-wrap: wrap; align-content: baseline; width: 60pt; height: 120pt }\
         .item { flex: 0 0 60pt; margin: 0 }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"row\"><p class=\"item big\">A</p><p class=\"item small\">B</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let big = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("first flex line should render");
    let small = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("second flex line should render");
    assert!(
        (big.y() - small.y()).abs() < 0.01,
        "align-content: baseline should align wrapped line baselines: big={big:?}, small={small:?}"
    );
}

#[tokio::test]
async fn vertical_row_align_content_baseline_packs_wrapped_line_baselines() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                align-content: baseline; width: 80pt; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"row\"><p class=\"item\">A</p><p class=\"item\">B</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let first = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("first vertical flex line should render");
    let second = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("second vertical flex line should render");

    assert!(
        (first.x() - second.x()).abs() < 0.01,
        "vertical align-content:baseline should align wrapped line x baselines: first={first:?}, second={second:?}"
    );
}

#[tokio::test]
async fn vertical_row_align_content_last_baseline_packs_wrapped_line_baselines() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                align-content: last baseline; width: 80pt; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"row\"><p class=\"item\">A</p><p class=\"item\">B</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let first = page
        .lines
        .iter()
        .find(|line| line.text == "A")
        .expect("first vertical flex line should render");
    let second = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("second vertical flex line should render");

    assert!(
        (first.x() - second.x()).abs() < 0.01,
        "vertical align-content:last baseline should align wrapped line x baselines: first={first:?}, second={second:?}"
    );
}

#[tokio::test]
async fn align_content_last_baseline_packs_wrapped_row_line_baselines() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-wrap: wrap; align-content: last baseline; width: 60pt; height: 120pt }\
         .item { flex: 0 0 60pt; margin: 0 }\
         .two { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <div class=\"row\"><p class=\"item two\">A<br>B</p><p class=\"item big\">C</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let first_last = page
        .lines
        .iter()
        .find(|line| line.text == "B")
        .expect("last baseline of first flex line should render");
    let second = page
        .lines
        .iter()
        .find(|line| line.text == "C")
        .expect("second flex line should render");
    assert!(
        (first_last.y() - second.y()).abs() < 0.01,
        "align-content: last baseline should align wrapped line last baselines: first_last={first_last:?}, second={second:?}"
    );
}

#[tokio::test]
async fn column_flex_align_content_last_baseline_uses_safe_end_fallback() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; flex-direction: column; flex-wrap: wrap; align-items: flex-start; align-content: last baseline; width: 100pt; height: 60pt; background: red }\
         .item { width: 20pt; height: 30pt; background: green }</style>\
         <div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("flex container background should paint");
    let first_green = page
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .min_by(|left, right| left.x().total_cmp(&right.x()))
        .expect("flex item backgrounds should paint");

    assert!(
        (first_green.x() - red.x() - 60.0).abs() < 0.01,
        "column flex align-content:last baseline should fall back to safe cross-end packing: red={red:?}, first_green={first_green:?}"
    );
}

#[tokio::test]
async fn abspos_flex_child_static_position_uses_flex_alignment() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .container { position: relative; display: flex; justify-content: center; align-items: center; width: 100pt; height: 40pt }\
         .abs { position: absolute; width: 20pt; height: 10pt; background: red }</style>\
         <div class=\"container\"><div class=\"abs\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("absolutely positioned flex child should paint");
    assert!(
        (red.x() - 50.0).abs() < 0.5,
        "static position should honor main-axis flex centering: {red:?}"
    );
}

#[tokio::test]
async fn display_contents_inline_flex_item_normalizes_mixed_flow_children() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font: 12pt/1 serif }\
         .flex { display: flex } .contents { display: contents } .inline { display: inline }</style>\
         <div class=\"flex\"><div class=\"contents\"><div class=\"inline\">2a<div>2</div></div></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.lines.iter().any(|line| line.text == "2a"),
        "leading inline text in a blockified flex item should render: {:?}",
        page.lines
    );
    assert!(
        page.lines.iter().any(|line| line.text == "2"),
        "block descendant in the same flex item should still render: {:?}",
        page.lines
    );
}

#[tokio::test]
async fn display_contents_children_participate_as_flex_items() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; width: 90pt }\
         .contents { display: contents }\
         .item { display: block; width: 20pt; height: 10pt }\
         .a { background: green }\
         .b { background: blue }\
         .c { background: yellow }</style>\
         <div class=\"row\"><span class=\"contents\"><span class=\"item a\"></span><span class=\"item b\"></span></span><span class=\"item c\"></span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("first display: contents child should paint");
    let blue = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("second display: contents child should paint");
    let yellow = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 255, 0)))
        .expect("following flex item should paint");
    assert!(
        (green.x() - 10.0).abs() < 0.5
            && (blue.x() - 30.0).abs() < 0.5
            && (yellow.x() - 50.0).abs() < 0.5,
        "display: contents children should be direct flex items: green={green:?}, blue={blue:?}, yellow={yellow:?}"
    );
}

#[tokio::test]
async fn display_contents_wpt_flex_direct_contents_children_match_reference_paint() {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 500pt 120pt; margin: 10pt }} {DISPLAY_CONTENTS_FLEX_ACID_CSS}</style>\
         <div class=\"flex c1\">\
           0\
           <div class=\"contents c1\">x</div>\
           <div class=\"contents c1\"><div class=\"contents c2\">y</div></div>\
           <div class=\"contents c1\"><div class=\"contents c2\"><div>1<span class=\"b\">1</span></div></div></div>\
           <div class=\"contents c2\"><div class=\"inline\">2a<div>2<div class=\"contents c2\">b<span class=\"b\">b</span></div></div></div></div>\
           <div class=\"contents c3\"><div class=\"inline\">3</div></div>\
           <div class=\"inline\"><div class=\"contents c4\">4</div></div>\
           <div><div class=\"contents c5\">5a</div></div>\
           <div class=\"c5\">5b</div>\
           <div class=\"contents c6\"><div>6</div></div>\
           <div class=\"ib\"><div class=\"contents c7\"><div class=\"contents c2\">7<span class=\"b\">a</span></div></div></div>\
           <div class=\"contents c9\"><div>8</div></div>\
           <div class=\"contents c9\"><div class=\"contents\">9<div class=\"contents c2\">a<span class=\"b\">b</span>c</div></div></div>\
           <div class=\"contents c10\"><div>10</div></div>\
         </div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_display_contents_wpt_flex_output(&document);
}

#[tokio::test]
async fn display_contents_wpt_flex_single_contents_child_match_reference_paint() {
    let document = Html::from_string(format!(
        "<style>@page {{ size: 500pt 120pt; margin: 10pt }} {DISPLAY_CONTENTS_FLEX_ACID_CSS}</style>\
         <div class=\"flex\"><div class=\"contents c1\">\
           0\
           <div class=\"contents c1\">x</div>\
           <div class=\"contents c1\"><div class=\"contents c2\">y</div></div>\
           <div class=\"contents c1\"><div class=\"contents c2\"><div>1<span class=\"b\">1</span></div></div></div>\
           <div class=\"contents c2\"><div class=\"inline\">2a<div>2<div class=\"contents c2\">b<span class=\"b\">b</span></div></div></div></div>\
           <div class=\"contents c3\"><div class=\"inline\">3</div></div>\
           <div><div class=\"contents c4\">4</div></div>\
           <div><div class=\"contents c5\">5a</div></div>\
           <div class=\"c5\">5b</div>\
           <div class=\"contents c6\"><div>6</div></div>\
           <div class=\"ib\"><div class=\"contents c7\"><div class=\"contents c2\">7<span class=\"b\">a</span></div></div></div>\
           <div class=\"contents c9\"><div>8</div></div>\
           <div class=\"contents c9\"><div class=\"contents\">9<div class=\"contents c2\">a<span class=\"b\">b</span>c</div></div></div>\
           <div class=\"contents c10\"><div>10</div></div>\
         </div></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_display_contents_wpt_flex_output(&document);
}

fn assert_display_contents_wpt_flex_output(document: &quire::Document) {
    let page = &document.pages[0];
    for text in ["0x", "y", "2a", "3", "4", "5a", "5b", "6", "8", "10"] {
        assert!(
            page.lines.iter().any(|line| line.text == text),
            "expected rendered text {text:?}: {:?}",
            page.lines
        );
    }
    let line_2bb = rendered_line(page, "2bb");
    assert_blue_at_text_position(page, line_2bb, glyph_center_x(line_2bb, 2));
    assert_no_blue_at_text_position(page, line_2bb, glyph_center_x(line_2bb, 1));

    let line_7a = rendered_line(page, "7a");
    assert_blue_at_text_position(page, line_7a, glyph_center_x(line_7a, 1));
    assert_no_blue_at_text_position(page, line_7a, glyph_center_x(line_7a, 0));

    let line_b = rendered_line(page, "b");
    assert_blue_at_text_position(page, line_b, glyph_center_x(line_b, 0));
    assert_no_blue_at_text_position(
        page,
        rendered_line(page, "y"),
        glyph_center_x(rendered_line(page, "y"), 0),
    );
    assert_no_blue_at_text_position(
        page,
        rendered_line(page, "a"),
        glyph_center_x(rendered_line(page, "a"), 0),
    );
    assert_no_blue_at_text_position(
        page,
        rendered_line(page, "c"),
        glyph_center_x(rendered_line(page, "c"), 0),
    );
}

fn rendered_line<'a>(page: &'a quire::Page, text: &str) -> &'a quire::RenderedLine {
    page.lines
        .iter()
        .find(|line| line.text == text)
        .unwrap_or_else(|| panic!("expected rendered text {text:?}: {:?}", page.lines))
}

fn glyph_start_x(line: &quire::RenderedLine, index: usize) -> f32 {
    let run = line
        .runs
        .first()
        .unwrap_or_else(|| panic!("expected text run for {line:?}"));
    let glyphs = run
        .glyphs
        .as_ref()
        .unwrap_or_else(|| panic!("expected glyph advances for {line:?}"));
    line.x()
        + run.x_offset
        + glyphs
            .iter()
            .take(index)
            .map(|glyph| glyph.x_advance)
            .sum::<f32>()
}

fn glyph_center_x(line: &quire::RenderedLine, index: usize) -> f32 {
    let run = line
        .runs
        .first()
        .unwrap_or_else(|| panic!("expected text run for {line:?}"));
    let glyphs = run
        .glyphs
        .as_ref()
        .unwrap_or_else(|| panic!("expected glyph advances for {line:?}"));
    glyph_start_x(line, index) + glyphs[index].x_advance / 2.0
}

fn assert_blue_at_text_position(page: &quire::Page, line: &quire::RenderedLine, x: f32) {
    assert!(
        blue_rect_covers_text_position(page, line, x),
        "expected blue background at x={x} for {line:?}; rects={:?}",
        page.rects
    );
}

fn assert_no_blue_at_text_position(page: &quire::Page, line: &quire::RenderedLine, x: f32) {
    assert!(
        !blue_rect_covers_text_position(page, line, x),
        "suppressed .contents.c2 should not paint blue background at x={x} for {line:?}; rects={:?}",
        page.rects
    );
}

fn blue_rect_covers_text_position(page: &quire::Page, line: &quire::RenderedLine, x: f32) -> bool {
    page.rects.iter().any(|rect| {
        rect.fill == Some(Color::new(0, 0, 255))
            && x >= rect.x() - 0.5
            && x <= rect.x() + rect.width() + 0.5
            && line.y() >= rect.y() - 0.5
            && line.y() <= rect.y() + rect.height() + 0.5
    })
}

#[tokio::test]
async fn display_contents_text_runs_form_anonymous_flex_items_without_contents_background() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body { margin: 0; font: 12pt/1 serif }\
         .flex { display: flex; column-gap: 20pt }\
         .contents { display: contents }\
         .c2 { background: blue; color: pink }\
         .b { background: inherit }</style>\
         <div class=\"flex\"><div class=\"contents\">9<div class=\"contents c2\">a<span class=\"b\">b</span>c</div></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let line_x = |text: &str| {
        page.lines
            .iter()
            .find(|line| line.text == text)
            .map(|line| line.x())
            .unwrap_or_else(|| panic!("expected rendered text {text:?}: {:?}", page.lines))
    };
    let nine_x = line_x("9");
    let a_x = line_x("a");
    let b_x = line_x("b");
    assert!(
        a_x - nine_x < 12.0 && b_x - a_x > 20.0,
        "9 and a should be contiguous inside one anonymous flex item, with the flex gap before b: {:?}",
        page.lines
    );
    let blue_rects = page
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .collect::<Vec<_>>();
    assert_eq!(
        blue_rects.len(),
        1,
        "only the real .b span should paint the inherited blue background, not suppressed contents boxes: {blue_rects:?}"
    );
}

#[tokio::test]
async fn column_flex_fragments_by_item_progression() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 }\
         .column { display: flex; flex-direction: column; width: 30pt }\
         .item { width: 30pt; height: 40pt; background: green }</style>\
         <div class=\"column\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        3,
        "each 40pt column item should move to a fresh 60pt page area"
    );
    for (page_index, page) in document.pages.iter().enumerate() {
        assert!(
            page.rects
                .iter()
                .any(|rect| rect.fill == Some(Color::new(0, 128, 0))
                    && (rect.height() - 40.0).abs() < 0.01),
            "page {page_index} should contain one flex item: {:?}",
            page.rects
        );
    }
}

#[tokio::test]
async fn wrapped_row_flex_fragments_by_line() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-wrap: wrap; width: 50pt }\
         .item { width: 50pt; height: 40pt; background: blue }</style>\
         <div class=\"row\"><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "wrapped row flex lines should fragment at line boundaries"
    );
    for (page_index, page) in document.pages.iter().enumerate() {
        assert!(
            page.rects
                .iter()
                .any(|rect| rect.fill == Some(Color::new(0, 0, 255))
                    && (rect.height() - 40.0).abs() < 0.01),
            "page {page_index} should contain one row flex line: {:?}",
            page.rects
        );
    }
}

#[tokio::test]
async fn fragmented_row_flex_clones_container_background() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-wrap: wrap; width: 60pt; background: black }\
         .item { width: 50pt; height: 40pt; background: blue }</style>\
         <div class=\"row\"><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "wrapped row flex lines should still fragment at line boundaries"
    );
    for (page_index, page) in document.pages.iter().enumerate() {
        assert!(
            page.rects.iter().any(|rect| rect.fill == Some(Color::BLACK)
                && (rect.width() - 60.0).abs() < 0.01
                && rect.height() >= 39.9),
            "page {page_index} should contain a cloned flex container background: {:?}",
            page.rects
        );
    }
}

#[tokio::test]
async fn flex_item_break_before_is_consumed_at_container_layer() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body { margin: 0 }\
         .column { display: flex; flex-direction: column; width: 30pt }\
         .item { width: 30pt; height: 20pt; background: red }</style>\
         <div class=\"column\"><div class=\"item\"></div><div class=\"item\" style=\"break-before: page\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "forced break-before on a flex item should break between flex fragments"
    );
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(255, 0, 0))),
        "first item should stay on page 1"
    );
    assert!(
        document.pages[1]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(255, 0, 0))),
        "second item should render on page 2"
    );
}

#[tokio::test]
async fn final_flex_item_break_after_propagates_after_container() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body { margin: 0 }\
         .column { display: flex; flex-direction: column; width: 30pt }\
         .item { width: 30pt; height: 20pt; background: green }\
         .after { width: 30pt; height: 20pt; background: blue }</style>\
         <div class=\"column\"><div class=\"item\" style=\"break-after: page\"></div></div><div class=\"after\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "forced break-after on the final flex item should break after the flex container"
    );
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 128, 0))),
        "flex item should render before the break"
    );
    assert!(
        document.pages[1]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 0, 255))),
        "following block should render after the propagated break"
    );
}

#[tokio::test]
async fn oversized_column_flex_item_splits_across_pages() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 }\
         .column { display: flex; flex-direction: column; width: 30pt }\
         .item { width: 30pt; height: 100pt; background: red }</style>\
         <div class=\"column\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "oversized column flex item should split over two 60pt fragmentainers"
    );
    let first = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("first item slice should paint");
    let second = document.pages[1]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("second item slice should paint");
    assert!(
        (first.height() - 60.0).abs() < 0.5 && (second.height() - 40.0).abs() < 0.5,
        "item slices should match fragmentainer remainder: first={first:?}, second={second:?}"
    );
}

#[tokio::test]
async fn oversized_wrapped_row_flex_line_splits_across_pages() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-wrap: wrap; width: 50pt }\
         .item { width: 50pt; height: 100pt; background: blue }</style>\
         <div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "oversized row flex line should split over two 60pt fragmentainers"
    );
    let first = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("first line slice should paint");
    let second = document.pages[1]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("second line slice should paint");
    assert!(
        (first.height() - 60.0).abs() < 0.5 && (second.height() - 40.0).abs() < 0.5,
        "row line slices should match fragmentainer remainder: first={first:?}, second={second:?}"
    );
}

#[tokio::test]
async fn split_flex_item_continuation_replays_later_child_content() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 }\
         .column { display: flex; flex-direction: column; width: 40pt }\
         .item { width: 40pt; height: 100pt }\
         .first { width: 40pt; height: 60pt; background: green }\
         .second { width: 40pt; height: 40pt; background: blue }</style>\
         <div class=\"column\"><div class=\"item\"><div class=\"first\"></div><div class=\"second\"></div></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "the oversized item should split across the two page areas"
    );
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 128, 0))
                && (rect.height() - 60.0).abs() < 0.5),
        "first page should paint the first child content: {:?}",
        document.pages[0].rects
    );
    assert!(
        document.pages[1]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 0, 255))
                && (rect.height() - 40.0).abs() < 0.5),
        "second page should paint the later child content: {:?}",
        document.pages[1].rects
    );
    assert!(
        !document.pages[1]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 128, 0))),
        "second page must not replay the start of the flex item: {:?}",
        document.pages[1].rects
    );
}
