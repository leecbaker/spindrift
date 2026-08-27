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

const RTL_JUSTIFY_CONTENT_LEFT_WPT: &str =
    include_str!("../../../tests/fixtures/wpt/css/css-flexbox/justify-content-left-rtl.html");
const RTL_JUSTIFY_CONTENT_LEFT_REFERENCE: &str =
    include_str!("../../../tests/fixtures/wpt/css/css-flexbox/justify-content-left-rtl-ref.html");

fn rects_overlap_y(
    a: &crate::document::paint::shapes::RenderedRect,
    b: &crate::document::paint::shapes::RenderedRect,
) -> bool {
    a.y() < b.y() + b.height() - 0.01 && b.y() < a.y() + a.height() - 0.01
}

fn rects_have_gap_x(
    a: &crate::document::paint::shapes::RenderedRect,
    b: &crate::document::paint::shapes::RenderedRect,
    expected: f32,
) -> bool {
    let gap = if a.x() <= b.x() {
        b.x() - (a.x() + a.width())
    } else {
        a.x() - (b.x() + b.width())
    };
    (gap - expected).abs() < 0.5
}

fn rects_have_gap_y(
    a: &crate::document::paint::shapes::RenderedRect,
    b: &crate::document::paint::shapes::RenderedRect,
    expected: f32,
) -> bool {
    let gap = if a.y() <= b.y() {
        b.y() - (a.y() + a.height())
    } else {
        a.y() - (b.y() + b.height())
    };
    (gap - expected).abs() < 0.5
}

fn page_rect_with_fill(
    page: &crate::document::Page,
    color: CssColor,
) -> &crate::document::paint::shapes::RenderedRect {
    page.rects()
        .iter()
        .find(|rect| rect.fill == Some(color))
        .unwrap_or_else(|| panic!("expected {color:?} background: {:?}", page.rects()))
}

#[tokio::test]
async fn size_contained_flex_descendants_keep_definite_sizes_and_auto_constraints() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         #flex { display: flex; flex-direction: column; width: 40pt }\
         #definite { background: green } #automatic { background: blue }\
         #definite > div { contain: size; height: 30pt }\
         #automatic > div { contain: size; min-height: 12pt; padding: 2pt 0 3pt }\
         </style><div id=flex><div id=definite><div></div></div><div id=automatic><div></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let definite = page_rect_with_fill(page, CssColor::new(0, 128, 0));
    let automatic = page_rect_with_fill(page, CssColor::new(0, 0, 255));
    assert_eq!(definite.height(), 30.0);
    assert_eq!(automatic.height(), 17.0);
}

#[tokio::test]
async fn rtl_row_justify_content_left_uses_the_physical_left_edge() {
    let document = Html::from_string(RTL_JUSTIFY_CONTENT_LEFT_WPT)
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let reference = Html::from_string(RTL_JUSTIFY_CONTENT_LEFT_REFERENCE)
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let page = &document.pages[0];
    let reference_page = &reference.pages[0];
    let container = page_rect_with_fill(page, CssColor::new(255, 0, 0));
    let item = page_rect_with_fill(page, CssColor::new(0, 128, 0));
    assert!(
        (item.x() - container.x()).abs() < 0.01,
        "`justify-content:left` must remain physical left in an RTL row: container={container:?}, item={item:?}"
    );
    assert_eq!(page.rects(), reference_page.rects());
}

#[tokio::test]
async fn vertical_rl_inline_flex_central_baseline_matches_empty_inline_block_line_extent() {
    let document = Html::from_string(
        "<style>@page { size: 140px 140px; margin: 0 }\
         body { margin: 0 }\
         #container { width: 100px; height: 100px; line-height: 0; writing-mode: vertical-rl; background: rgb(255 0 0) }\
         #inline-block { display: inline-block; width: 100px; height: 50px; background: rgb(0 0 255) }\
         #inline-flex { display: inline-flex }\
         #inline-flex > div { width: 100px; height: 50px; background: rgb(0 128 0) }\
         </style><div id=container><span id=inline-block></span><span id=inline-flex><div></div></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let container = page_rect_with_fill(page, CssColor::new(255, 0, 0));
    let inline_block = page_rect_with_fill(page, CssColor::new(0, 0, 255));
    let inline_flex = page_rect_with_fill(page, CssColor::new(0, 128, 0));

    assert_eq!(container.width(), 75.0);
    assert_eq!(inline_block.width(), 75.0);
    assert_eq!(inline_flex.width(), 75.0);
    assert_eq!(inline_block.x(), container.x());
    assert_eq!(inline_flex.x(), container.x());
}

#[tokio::test]
async fn static_flex_and_grid_items_paint_after_container_decorations() {
    for layout in ["flex", "grid"] {
        let document = Html::from_string(format!(
            "<style>@page {{ size: 160pt 100pt; margin: 0 }} body {{ margin: 0 }}\
             .container {{ display: {layout}; width: 40pt; height: 20pt }}\
             .first {{ background: rgb(255 0 0) }} .second {{ background: rgb(0 0 255) }}\
             .item {{ width: 20pt; height: 20pt }}\
             .green {{ background: rgb(0 128 0); order: 1 }}\
             .yellow {{ background: rgb(255 255 0); order: 0 }}\
             </style>\
             <div class=\"container first\"><div class=\"item green\"></div><div class=\"item yellow\"></div></div>\
             <div class=\"container second\"></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let red = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));
        let blue = first_rect_paint_operation_index(page, CssColor::new(0, 0, 255));
        let green = first_rect_paint_operation_index(page, CssColor::new(0, 128, 0));
        let yellow = first_rect_paint_operation_index(page, CssColor::new(255, 255, 0));
        assert!(
            red < green && red < yellow && blue < green && blue < yellow,
            "{layout} container decorations must paint before their static items: {:?}",
            page.paint_operations()
        );
        assert!(
            yellow < green,
            "{layout} items must retain order-modified document order: {:?}",
            page.paint_operations()
        );
    }
}

#[tokio::test]
async fn static_flex_item_positioned_descendants_interleave_in_parent_stacking_context() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display: flex; width: 90pt; height: 20pt }\
         .first { flex: 0 0 45pt; min-width: 0; height: 20pt; background: lightblue }\
         .second { flex: 0 0 45pt; min-width: 0; height: 20pt; background: yellow }\
         .a, .b { position: relative; width: 70pt; height: 6pt }\
         .a { z-index: 10; background: purple }\
         .b { z-index: 20; background: teal }\
         .c { position: relative; z-index: 15; width: 20pt; height: 20pt; background: lime }\
         </style><div class=\"flex\"><div class=\"first\"><div class=\"a\"></div><div class=\"b\"></div></div><div class=\"second\"><div class=\"c\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(
        final_rect_fill_at(page, 50.0, 97.0),
        Some(CssColor::new(0, 255, 0)),
        "the second item's z-index:15 descendant must paint above the first item's z-index:10 descendant: {:?}",
        page.rects()
    );
    assert_eq!(
        final_rect_fill_at(page, 50.0, 90.0),
        Some(CssColor::new(0, 128, 128)),
        "the first item's z-index:20 descendant must paint above the second item's z-index:15 descendant: {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn relative_auto_flex_item_paints_in_auto_positioned_phase() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display: flex; width: 90pt; height: 20pt }\
         .relative { position: relative; flex: 0 0 60pt; height: 20pt; background: red }\
         .static { flex: 0 0 60pt; height: 20pt; margin-left: -30pt; background: blue }\
         </style><div class=\"flex\"><div class=\"relative\"></div><div class=\"static\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(
        final_rect_fill_at(page, 45.0, 90.0),
        Some(CssColor::new(255, 0, 0)),
        "a relatively positioned flex item with z-index:auto must paint above in-flow siblings: {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn non_auto_z_index_flex_item_captures_positioned_descendants() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display: flex; width: 90pt; height: 20pt }\
         .lower { z-index: 1; flex: 0 0 60pt; height: 20pt; background: red }\
         .lower > div { position: relative; z-index: 999; width: 60pt; height: 20pt; background: purple }\
         .upper { z-index: 2; flex: 0 0 60pt; height: 20pt; margin-left: -30pt; background: blue }\
         </style><div class=\"flex\"><div class=\"lower\"><div></div></div><div class=\"upper\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert_eq!(
        final_rect_fill_at(page, 45.0, 90.0),
        Some(CssColor::new(0, 0, 255)),
        "a positive-z descendant must remain below the later real flex-item stacking context: {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn paged_wrapped_flex_space_between_preserves_definite_cross_size() {
    let document = Html::from_string(
        "<!doctype html><style>\
         @page { size: 200pt 120pt; margin: 0 } body { margin: 0 }\
         .cover { display: flex; flex-wrap: wrap; align-content: space-between; height: 100pt }\
         .title { width: 100%; height: 20pt; background: red }\
         .address { flex: 1 50%; height: 10pt } .left { background: green } .right { background: blue }\
         </style><main class=\"cover\"><div class=\"title\"></div><div class=\"address left\"></div><div class=\"address right\"></div></main>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let page = &document.pages[0];
    let red = page_rect_with_fill(page, CssColor::new(255, 0, 0));
    let green = page_rect_with_fill(page, CssColor::new(0, 128, 0));
    let blue = page_rect_with_fill(page, CssColor::new(0, 0, 255));
    assert!(
        (green.y() - 20.0).abs() < 0.01 && (blue.y() - 20.0).abs() < 0.01,
        "the second flex line must pack against the definite container block-end: red={red:?}, green={green:?}, blue={blue:?}"
    );
    assert!(
        (red.y() - green.y() - 80.0).abs() < 0.01,
        "align-content: space-between must preserve its distributed free space: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn paged_wrapped_flex_auto_cross_size_keeps_intrinsic_line_stacking() {
    let document = Html::from_string(
        "<!doctype html><style>\
         @page { size: 200pt 120pt; margin: 0 } body { margin: 0 }\
         .cover { display: flex; flex-wrap: wrap; align-content: space-between }\
         .title { width: 100%; height: 20pt; background: red }\
         .address { flex: 1 50%; height: 10pt } .left { background: green } .right { background: blue }\
         </style><main class=\"cover\"><div class=\"title\"></div><div class=\"address left\"></div><div class=\"address right\"></div></main>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let page = &document.pages[0];
    let red = page_rect_with_fill(page, CssColor::new(255, 0, 0));
    let green = page_rect_with_fill(page, CssColor::new(0, 128, 0));
    assert!(
        (red.y() - green.y() - 10.0).abs() < 0.01,
        "an auto-height flex container has no cross-axis free space to distribute: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn paged_wrapped_flex_uses_its_own_remaining_definite_height() {
    let document = Html::from_string(
        "<!doctype html><style>\
         @page { size: 200pt 120pt; margin: 0 } body, p { margin: 0 }\
         p { height: 20pt }\
         .cover { display: flex; flex-wrap: wrap; align-content: space-between; height: 100pt }\
         .title { width: 100%; height: 20pt; background: red }\
         .address { flex: 1 50%; height: 10pt } .left { background: green } .right { background: blue }\
         </style><p></p><main class=\"cover\"><div class=\"title\"></div><div class=\"address left\"></div><div class=\"address right\"></div></main>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        1,
        "the definite flex box fits after its predecessor"
    );
    let page = &document.pages[0];
    let green = page_rect_with_fill(page, CssColor::new(0, 128, 0));
    let blue = page_rect_with_fill(page, CssColor::new(0, 0, 255));
    assert!(
        green.y().abs() < 0.01 && blue.y().abs() < 0.01,
        "the final line packs to its own remaining container edge, not a continuation page: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn normal_flow_reference_boxes_keep_definite_sizes_and_siblings() {
    let document = Html::from_string(
        "<!doctype html><style>\
         @page { size: 500px 200px; margin: 0 } body { margin: 0 }\
         #parent { width: 400px; height: 80px; background: blue }\
         .item { display: inline-block; width: 40px; height: 60px; background: yellow }\
         </style><div id=\"parent\"><span class=\"item\"></span><span class=\"item\"></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("normal-flow parent should paint its background");
    assert!(
        (blue.width() - 300.0).abs() < 0.01 && (blue.height() - 60.0).abs() < 0.01,
        "definite normal-flow parent dimensions must survive child layout: {blue:?}"
    );
    let items = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .collect::<Vec<_>>();
    assert_eq!(
        items.len(),
        2,
        "both inline-block siblings must paint: {items:?}"
    );
    assert!(
        items
            .iter()
            .all(|rect| (rect.width() - 30.0).abs() < 0.01 && (rect.height() - 45.0).abs() < 0.01),
        "inline-block dimensions must survive normal-flow replay: {items:?}"
    );
    assert!(
        (items[1].x() - (items[0].x() + items[0].width())).abs() < 0.01,
        "inline-block siblings must retain their shared line placement: {items:?}"
    );
}

#[tokio::test]
async fn normal_flow_column_reference_matches_flex_margin_geometry() {
    const COMMON: &str = "<!doctype html><style>\
        @page { margin: 0 }\
        div { background: blue; margin: 1em 0; border: 1px solid black; }\
        span { background: white; margin: 1em; width: 8em; }\
        </style><div><span>filler</span><span>filler</span><span>filler</span><span>filler</span></div>";
    let flex = Html::from_string(format!(
        "{COMMON}<style>div {{ display: flex; flex-direction: column; }} span {{ display: inline-block; }}</style>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "{COMMON}<style>span {{ display: block; }} span ~ span {{ margin: 2em 1em 1em; }}</style>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let colored_rects = |document: &quire::Document| {
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| {
                matches!(
                    rect.fill,
                    Some(color) if color == CssColor::new(0, 0, 255)
                        || color == CssColor::new(255, 255, 255)
                )
            })
            .map(|rect| (rect.x(), rect.y(), rect.width(), rect.height(), rect.fill))
            .collect::<Vec<_>>()
    };
    let reference_rects = colored_rects(&reference);
    let flex_rects = colored_rects(&flex);
    assert_eq!(
        reference_rects.len(),
        flex_rects.len(),
        "ordinary block flow must preserve the same number of colored boxes as the flex reference"
    );
    for (reference, flex) in reference_rects.iter().zip(&flex_rects) {
        assert_eq!(
            reference.4, flex.4,
            "corresponding boxes must keep their paint color"
        );
        for (reference, flex) in [reference.0, reference.1, reference.2, reference.3]
            .into_iter()
            .zip([flex.0, flex.1, flex.2, flex.3])
        {
            assert!(
                (reference - flex).abs() <= 0.01,
                "ordinary block flow must preserve the same flex margin geometry: reference={reference_rects:?}, flex={flex_rects:?}"
            );
        }
    }
}

#[tokio::test]
async fn generated_flex_container_replays_its_anonymous_content_item() {
    let document = Html::from_string(
        "<!doctype html><style>\
         @page { margin: 0 }\
         div { background: #3366cc; border: 1px solid black }\
         div::after { content: 'xxx'; background: yellow; margin: 1em; width: 200px; height: 2em; display: flex }\
         </style><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.lines().iter().any(|line| line.text == "xxx"),
        "a generated flex container must create and paint its anonymous content item"
    );
    assert!(
        page.rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 255, 0))),
        "the generated flex container's principal background must survive replay"
    );
}

async fn single_x_line_x(target_style: &str) -> f32 {
    let document = Html::from_string(format!(
        "<!DOCTYPE html><style>\
         @page {{ size: 300px 200px; margin: 0 }} body {{ margin: 0 }}\
         #target {{ font: 100px/1 sans-serif; color: green; width: 200px;\
           position: relative; left: -50px; {target_style} }}\
         </style><div id=\"target\">X</div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "X")
        .collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        1,
        "expected one X line: {:?}",
        document.pages[0].lines()
    );
    lines[0].x()
}

async fn assert_vertical_rl_inline_flex_column_wrap_gap(html: &str) {
    let document = Html::from_string(html)
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let gray = CssColor::new(128, 128, 128);
    let container = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(green))
        .unwrap_or_else(|| panic!("expected green inline-flex background: {:?}", page.rects()));
    assert!(
        (container.width() - 100.0).abs() < 0.01 && (container.height() - 200.0).abs() < 0.01,
        "vertical inline-flex should keep block-size as physical width and inline-size as physical height: {container:?}"
    );

    let items = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(gray))
        .collect::<Vec<_>>();
    assert_eq!(
        items.len(),
        4,
        "expected four grey flex item backgrounds: {:?}",
        page.rects()
    );
    assert!(
        items.iter().all(|rect| rect.width() <= 20.0),
        "vertical text flex item physical widths should come from line-height, not text advance: {items:?}"
    );
    assert!(
        items.iter().enumerate().any(|(index, item)| {
            items[index + 1..]
                .iter()
                .any(|other| rects_overlap_y(item, other) && rects_have_gap_x(item, other, 20.0))
        }),
        "same flex line should have a 20pt physical horizontal gap between vertical text items: {items:?}"
    );
    assert!(
        items.iter().enumerate().any(|(index, item)| {
            items[index + 1..]
                .iter()
                .any(|other| !rects_overlap_y(item, other) && rects_have_gap_y(item, other, 20.0))
        }),
        "wrapped flex lines should have a 20pt physical vertical gap: {items:?}"
    );
}

#[tokio::test]
async fn column_flex_centered_anonymous_text_uses_intrinsic_cross_size() {
    let block_center = single_x_line_x("text-align: center").await;
    let flex_center = single_x_line_x(
        "display: flex; flex-direction: column; align-items: center; text-align: center",
    )
    .await;
    let flex_left = single_x_line_x(
        "display: flex; flex-direction: column; align-items: center; text-align: left",
    )
    .await;

    assert!(
        (flex_center - block_center).abs() < 0.01,
        "centered anonymous flex text should match block centering: flex={flex_center}, block={block_center}"
    );
    assert!(
        (flex_center - flex_left).abs() < 0.01,
        "text-align should not add a second offset inside shrinkwrapped anonymous flex text: center={flex_center}, left={flex_left}"
    );
}

#[tokio::test]
async fn inherited_vertical_rl_inline_flex_column_wrap_gap_uses_logical_item_block_sizes() {
    assert_vertical_rl_inline_flex_column_wrap_gap(
        "<!DOCTYPE html><style>@page { size: 400pt 400pt; margin: 0 }\
         body { margin: 0; writing-mode: vertical-rl }\
         section { background-color: green; block-size: 100pt; inline-size: 200pt;\
                   display: inline-flex; flex-direction: column; flex-wrap: wrap;\
                   gap: 20pt; line-height: 18pt; font-size: 12pt; vertical-align: top }\
         section > div { background-color: grey; color: white }</style>\
         <section><div>Black Panther</div><div>Wonder Woman</div><div>Storm</div><div>Flash</div></section>",
    )
    .await;
}

#[tokio::test]
async fn direct_vertical_rl_inline_flex_column_wrap_gap_uses_logical_item_block_sizes() {
    assert_vertical_rl_inline_flex_column_wrap_gap(
        "<!DOCTYPE html><style>@page { size: 400pt 400pt; margin: 0 }\
         body { margin: 0 }\
         section { writing-mode: vertical-rl; background-color: green; block-size: 100pt;\
                   inline-size: 200pt; display: inline-flex; flex-direction: column;\
                   flex-wrap: wrap; gap: 20pt; line-height: 18pt; font-size: 12pt;\
                   vertical-align: top }\
         section > div { background-color: grey; color: white }</style>\
         <section><div>Black Panther</div><div>Wonder Woman</div><div>Storm</div><div>Flash</div></section>",
    )
    .await;
}

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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| {
            panic!(
                "expected green flex item background: {:?}",
                document.pages[0].rects()
            )
        });
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "stretched vertical item should use its 100px inline size for descendant percentage padding: {green:?}"
    );
}

#[tokio::test]
async fn stretched_replaced_descendant_transfers_size_from_stretched_flex_item() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 140px 140px; margin: 0 } body { margin: 0 } p { display: none }\
         </style>\
         <p>Test passes if there is a filled green square.</p>\
         <div style=\"display: inline-flex; height: 100px; background: green;\">\
           <div><canvas width=\"10\" height=\"10\" style=\"height: 100%\"></canvas></div>\
         </div>",
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
                "expected green inline-flex background: {:?}",
                document.pages[0].rects()
            )
        });

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "stretched flex item should give the canvas a definite 100px percentage-height basis and transfer width through its 1:1 ratio: {green:?}"
    );
}

async fn assert_column_flex_post_flexing_percentage_height_descendant_square(doctype: &str) {
    let document = Html::from_string(format!(
        "{doctype}<html><head><style>\
         @page {{ size: 240px 240px; margin: 0 }} body {{ margin: 0 }} p {{ display: none }}\
         </style></head><body>\
         <p style=\"margin-top: 1em\">Test passes if there is a filled green square.</p>\
         <div style=\"display: flex; width: 100px; height: 100px; flex-direction: column;\">\
           <div style=\"flex-grow: 1; height: 0;\">\
             <div style=\"display: flex; flex-direction: column; height: 100%;\">\
               <div style=\"width: 100px; height: 50px; background-color: green;\"></div>\
               <div style=\"flex: 1;\">\
                 <div style=\"height: 100%; background-color: green;\"></div>\
               </div>\
             </div>\
           </div>\
         </div></body></html>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    assert!(
        !green_rects.is_empty(),
        "expected green square paint: {:?}",
        document.pages[0].rects()
    );

    let min_x = green_rects
        .iter()
        .map(|rect| rect.x())
        .fold(f32::INFINITY, f32::min);
    let max_x = green_rects
        .iter()
        .map(|rect| rect.x() + rect.width())
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = green_rects
        .iter()
        .map(|rect| rect.y())
        .fold(f32::INFINITY, f32::min);
    let max_y = green_rects
        .iter()
        .map(|rect| rect.y() + rect.height())
        .fold(f32::NEG_INFINITY, f32::max);
    let green_area = green_rects
        .iter()
        .map(|rect| rect.width() * rect.height())
        .sum::<f32>();
    let expected = 75.0;
    assert!(
        (max_x - min_x - expected).abs() < 0.01
            && (max_y - min_y - expected).abs() < 0.01
            && (green_area - expected * expected).abs() < 0.1,
        "post-flexing main size should define nested percentage heights: rects={green_rects:?}"
    );
}

#[tokio::test]
async fn column_flex_post_flexing_main_size_defines_descendant_percentage_height() {
    assert_column_flex_post_flexing_percentage_height_descendant_square("").await;
}

#[tokio::test]
async fn standards_column_flex_post_flexing_main_size_defines_descendant_percentage_height() {
    assert_column_flex_post_flexing_percentage_height_descendant_square("<!DOCTYPE html>").await;
}

#[tokio::test]
async fn block_flex_item_contains_first_child_margin() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 200pt 200pt; margin: 0 } body { margin: 0 }\
         .flex { display: flex; align-items: flex-start; width: 100pt }\
         .item { background: green; width: 50pt }\
         .item > div { margin-top: 20pt; height: 10pt; background: red }\
         </style><div class=\"flex\"><div class=\"item\"><div></div></div></div>",
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
                "expected flex item background: {:?}",
                document.pages[0].rects()
            )
        });
    assert!(
        (green.height() - 30.0).abs() < 0.01,
        "flex item should establish an independent formatting context and contain first-child margin: {green:?}"
    );
}

#[tokio::test]
async fn block_flex_item_contains_internal_float_height() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 200pt 200pt; margin: 0 } body { margin: 0 }\
         .flex { display: flex; align-items: flex-start; width: 100pt }\
         .item { background: green; width: 50pt }\
         .float { float: left; width: 30pt; height: 25pt; background: red }\
         </style><div class=\"flex\"><div class=\"item\"><div class=\"float\"></div></div><div style=\"width: 10pt; height: 10pt; background: blue\"></div></div>",
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
                "expected flex item background: {:?}",
                document.pages[0].rects()
            )
        });
    assert!(
        (green.height() - 25.0).abs() < 0.01,
        "flex item should contain internal floats in its independent formatting context: {green:?}"
    );
}

#[tokio::test]
async fn inline_flex_item_contains_first_child_margin() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 200pt 200pt; margin: 0 } body { margin: 0 }\
         .flex { display: inline-flex; align-items: flex-start; vertical-align: top }\
         .item { background: green; width: 50pt }\
         .item > span { display: block; margin-top: 20pt; height: 10pt; background: red }\
         </style><span class=\"flex\"><span class=\"item\"><span></span></span></span>",
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
                "expected inline-flex item background: {:?}",
                document.pages[0].rects()
            )
        });
    assert!(
        (green.height() - 30.0).abs() < 0.01,
        "inline-flex item should use the same independent formatting context semantics: {green:?}"
    );
}

#[tokio::test]
async fn flex_blockified_inline_item_paints_source_background_once() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 200pt 200pt; margin: 0 } body { margin: 0 }\
         .flex { display: flex; width: 100pt; font-size: 10pt; line-height: 10pt }\
         .item { display: inline; background: rgb(10, 20, 30); color: transparent }\
         </style><div class=\"flex\"><span class=\"item\">text</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let painted = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(10, 20, 30)))
        .collect::<Vec<_>>();
    assert_eq!(
        painted.len(),
        1,
        "blockified inline flex item should not also paint inline text-fragment background: {painted:?}"
    );
}

#[tokio::test]
async fn flex_replay_keeps_a_grid_item_descendant_visible_without_root_decoration() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 200pt 200pt; margin: 0 } body { margin: 0 }\
         .flex { display: flex; width: 100pt; height: 100pt }\
         .item { display: grid; width: 100pt; height: 100pt }\
         .child { background: rgb(0, 128, 0) }\
         </style><div class=\"flex\"><div class=\"item\"><div class=\"child\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let painted = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    assert_eq!(
        painted.len(),
        1,
        "grid descendant must paint once: {painted:?}"
    );
    assert!(
        (painted[0].width() - 100.0).abs() < 0.01 && (painted[0].height() - 100.0).abs() < 0.01,
        "grid descendant must retain the flex item's placed geometry: {:?}",
        painted[0]
    );
}

#[tokio::test]
async fn flex_replay_paints_grid_root_decoration_and_child_once_each() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 200pt 200pt; margin: 0 } body { margin: 0 }\
         .flex { display: flex; width: 100pt; height: 100pt }\
         .item { display: grid; box-sizing: border-box; width: 100pt; height: 100pt;\
                 background: rgb(255, 0, 0); border: 5pt solid rgb(0, 0, 0) }\
         .child { background: rgb(0, 0, 255) }\
         </style><div class=\"flex\"><div class=\"item\"><div class=\"child\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rects = document.pages[0].rects();
    let red = rects
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();
    let blue = rects
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();
    let black = rects
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 0)))
        .collect::<Vec<_>>();
    assert_eq!(
        red.len(),
        1,
        "grid root background must paint once: {red:?}"
    );
    assert_eq!(
        blue.len(),
        1,
        "grid child background must paint once: {blue:?}"
    );
    assert_eq!(
        black.len(),
        4,
        "grid root border edges must paint once: {black:?}"
    );
}

#[tokio::test]
async fn column_flex_stretched_aspect_ratio_item_keeps_content_auto_minimum() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 240px 180px; margin: 0 } body { margin: 0 } p { display: none }\
         </style>\
         <p>Test passes if there is a filled green square.</p>\
         <div style=\"display: flex; flex-direction: column; width: 100px; height: 0px;\">\
           <div style=\"background: green; aspect-ratio: 2/1;\">\
             <div style=\"height: 100px;\"></div>\
           </div>\
         </div>",
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
                "expected green flex item background: {:?}",
                document.pages[0].rects()
            )
        });

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "stretched column flex item should be a 100px square after content auto-minimum sizing: {green:?}"
    );
}

#[tokio::test]
async fn empty_column_flex_stretched_aspect_ratio_item_keeps_transferred_basis() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 240px 180px; margin: 0 } body { margin: 0 }\
         </style>\
         <div style=\"display: flex; flex-direction: column; width: 100px; height: 0px;\">\
           <div style=\"background: green; aspect-ratio: 2/1;\"></div>\
         </div>",
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
                "expected green flex item background: {:?}",
                document.pages[0].rects()
            )
        });

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 37.5).abs() < 0.01,
        "empty stretched column flex item should keep the 100px by 50px transferred flex basis: {green:?}"
    );
}

#[tokio::test]
async fn stretch_min_cross_size_preserves_non_negative_content_box() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 220px 220px; margin: 0 } body { margin: 0 } p { display: none }\
         #red { position: absolute; z-index: -1; width: 200px; height: 200px; background: red }\
         #flex-container { display: flex; flex-direction: row; height: 0 }\
         #flex-item { min-height: stretch; border: 100px solid green }\
         </style>\
         <p>Test passes if there is a filled green square and no red.</p>\
         <div id=\"red\"></div><div id=\"flex-container\"><div id=\"flex-item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    for (x, y) in [(37.5, 37.5), (112.5, 37.5), (37.5, 112.5), (112.5, 112.5)] {
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(green),
            "stretched flex item border should cover the red reference square at ({x}, {y}): {:?}",
            page.rects()
        );
    }
}

#[tokio::test]
async fn abspos_auto_position_honors_flex_alignment_on_both_axes() {
    let document = Html::from_string(
        "<!doctype html><style>\
         @page { size: 220px 220px; margin: 0 } body { margin: 0 }\
         .parent { position: fixed; top: 0; left: 0; display: flex;\
           align-items: center; justify-content: center; width: 200px; height: 200px;\
           background: yellow }\
         .child { position: absolute; width: 100px; height: 100px; background: green }\
         </style><div class=\"parent\"><div class=\"child\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let yellow = CssColor::new(255, 255, 0);
    let green = CssColor::new(0, 128, 0);
    let parent = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(yellow))
        .unwrap_or_else(|| panic!("expected yellow flex container: {:?}", page.rects()));
    let child = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(green))
        .unwrap_or_else(|| panic!("expected green abspos flex child: {:?}", page.rects()));

    let px = 0.75;
    let expected_offset = 50.0 * px;
    assert!(
        (child.x() - parent.x() - expected_offset).abs() < 0.01
            && (child.y() - parent.y() - expected_offset).abs() < 0.01
            && (child.width() - 100.0 * px).abs() < 0.01
            && (child.height() - 100.0 * px).abs() < 0.01,
        "abspos flex child should be centered by justify-content and align-items: parent={parent:?}, child={child:?}"
    );
}

#[tokio::test]
async fn abspos_auto_position_covers_vertical_writing_flex_content_box() {
    for writing_mode in ["vertical-lr", "vertical-rl"] {
        let document = Html::from_string(format!(
            "<!DOCTYPE html><style>\
             @page {{ size: 140px 140px; margin: 0 }} body {{ margin: 0 }} p {{ display: none }}\
             .flex {{ display: flex; position: relative; writing-mode: {writing_mode}; direction: ltr;\
               width: 100px; height: 100px; border: solid white; border-left-width: 20px; left: -20px;\
               border-top-width: 5px; top: -5px; border-right-width: 10px; border-bottom-width: 15px;\
               background: red }}\
             .flex > div {{ position: absolute; width: 100%; height: 100%; background: green }}\
             </style><p>Test passes if there is a filled green square and no red.</p>\
             <div class=\"flex\"><div></div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let red = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .unwrap_or_else(|| {
                panic!(
                    "expected flex background for {writing_mode}: {:?}",
                    page.rects()
                )
            });
        let green = CssColor::new(0, 128, 0);
        let px = 0.75;
        let content_left = red.x() + 20.0 * px;
        let content_bottom = red.y() + 15.0 * px;
        let content_width = 100.0 * px;
        let content_height = 100.0 * px;
        for (x, y) in [
            (
                content_left + content_width / 2.0,
                content_bottom + content_height / 2.0,
            ),
            (content_left + content_width / 2.0, content_bottom + 1.0),
            (
                content_left + content_width / 2.0,
                content_bottom + content_height - 1.0,
            ),
        ] {
            assert_eq!(
                final_rect_fill_at(page, x, y),
                Some(green),
                "{writing_mode} abspos child should cover red flex background at ({x}, {y}): {:?}",
                page.rects()
            );
        }
    }
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let yellow = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .collect::<Vec<_>>();
    assert_eq!(
        yellow.len(),
        2,
        "real child and generated ::after should both paint yellow flex item backgrounds: {:?}",
        page.rects()
    );
    assert!(
        yellow[1].x() > yellow[0].x() + yellow[0].width(),
        "generated ::after flex item should be laid out after the real child: {yellow:?}"
    );
    assert!(
        page.lines().iter().any(|line| line.text == "yyy"),
        "generated ::after text should render as flex item content: {:?}",
        page.lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut line_text_by_x = document.pages[0]
        .lines()
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
        document.pages[0].lines()
    );
}

#[tokio::test]
async fn flex_order_painting_uses_order_modified_document_order_with_negative_margin() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 200px 160px; margin: 0 } body { margin: 0 } p { display: none }\
         </style>\
         <p>This test passes if there is no red showing.</p>\
         <div style=\"display: flex; width: 100px;\">\
         <div style=\"order: 2; background-color: green; width: 100px; height: 100px; margin-left: -50px;\"></div>\
         <div style=\"order: 1; background-color: red; width: 50px; height: 100px;\"></div>\
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
        .unwrap_or_else(|| panic!("expected red flex item background: {:?}", page.rects()));
    assert_eq!(
        final_rect_fill_at(
            page,
            red.x() + red.width() / 2.0,
            red.y() + red.height() / 2.0
        ),
        Some(CssColor::new(0, 128, 0)),
        "order-modified flex painting should cover the lower-order red item with the negative-margin green item: {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn flex_items_paint_each_decoration_before_its_own_contents() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 80pt; margin: 10pt } body { margin: 0; font-size: 12pt; line-height: 12pt } .flex { display: flex; height: 36pt } .item { flex: 0 0 0; height: 12pt } .first { background: yellow } .second { background: pink }</style><div class=\"flex\"><span class=\"item first\">one</span><span class=\"item second\">two</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let yellow = page
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("first flex item background should paint");
    let pink = page
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::new(255, 192, 203)))
        .expect("second flex item background should paint");
    let one = page
        .lines()
        .iter()
        .position(|line| line.text == "one")
        .expect("first flex item text should paint");
    let two = page
        .lines()
        .iter()
        .position(|line| line.text == "two")
        .expect("second flex item text should paint");

    let yellow_operation = page
        .operations()
        .iter()
        .position(
            |operation| matches!(operation, crate::document::paint::page::PaintOperation::Rect(index) if *index == yellow),
        )
        .expect("first flex item background should have a paint operation");
    let one_operation = page
        .operations()
        .iter()
        .position(
            |operation| matches!(operation, crate::document::paint::page::PaintOperation::Line(index) if *index == one),
        )
        .expect("first flex item text should have a paint operation");
    let pink_operation = page
        .operations()
        .iter()
        .position(
            |operation| matches!(operation, crate::document::paint::page::PaintOperation::Rect(index) if *index == pink),
        )
        .expect("second flex item background should have a paint operation");
    let two_operation = page
        .operations()
        .iter()
        .position(
            |operation| matches!(operation, crate::document::paint::page::PaintOperation::Line(index) if *index == two),
        )
        .expect("second flex item text should have a paint operation");

    assert!(
        yellow_operation < one_operation
            && one_operation < pink_operation
            && pink_operation < two_operation,
        "flex items must paint like inline blocks in order-modified document order: {:?}",
        page.operations()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let container = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("definite-width flex container background should paint");
    assert!(
        (container.width() - 480.0).abs() < 0.01,
        "definite-width block flex container should overflow instead of clamping to the 460pt containing block: {container:?}"
    );

    for color in [
        CssColor::new(255, 255, 0),
        CssColor::new(255, 192, 203),
        CssColor::new(173, 216, 230),
        CssColor::new(128, 128, 128),
    ] {
        let item = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| {
                panic!("expected flex item with fill {color:?}: {:?}", page.rects())
            });
        assert!(
            (item.width() - 60.0).abs() < 0.01,
            "flex: 0 1 auto item should keep its definite width: {item:?}"
        );
    }
}

#[tokio::test]
async fn auto_height_row_flex_container_applies_min_max_height_constraints() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 240pt 220pt; margin: 0 } body { margin: 0 }\
         .flexbox { width: 200px; border: 1px dashed blue; background: lightgreen;\
           font-size: 10px; display: flex; margin-bottom: 5px }\
         .flexbox > div { width: 200px }\
         </style>\
         <div class=\"flexbox\"><div>text</div></div>\
         <div class=\"flexbox\" style=\"min-height: 2px\"><div>text</div></div>\
         <div class=\"flexbox\" style=\"max-height: 300px\"><div>text</div></div>\
         <div class=\"flexbox\" style=\"min-height: 30px\"><div>text</div></div>\
         <div class=\"flexbox\" style=\"max-height: 6px\"><div>text</div></div>\
         <div class=\"flexbox\" style=\"min-height: 30px; max-height: 5px\"><div>text</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut flexboxes = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(144, 238, 144)))
        .collect::<Vec<_>>();
    assert_eq!(
        flexboxes.len(),
        6,
        "expected six flex container backgrounds: {:?}",
        document.pages[0].rects()
    );
    flexboxes.sort_by(|a, b| b.y().partial_cmp(&a.y()).unwrap());

    let px = 0.75;
    let border_box_extra = 2.0 * px;
    let shrinkwrapped_height = flexboxes[0].height();
    for index in 0..3 {
        assert!(
            (flexboxes[index].height() - shrinkwrapped_height).abs() < 0.01,
            "unconstraining min/max-height should preserve shrinkwrapped auto height: {flexboxes:?}"
        );
    }
    assert!(
        (flexboxes[3].height() - (30.0 * px + border_box_extra)).abs() < 0.01,
        "min-height should floor the auto-height flex container: {flexboxes:?}"
    );
    assert!(
        (flexboxes[4].height() - (6.0 * px + border_box_extra)).abs() < 0.01,
        "max-height should clamp the auto-height flex container without using overflowing item extents: {flexboxes:?}"
    );
    assert!(
        (flexboxes[5].height() - (30.0 * px + border_box_extra)).abs() < 0.01,
        "min-height should win when it is larger than max-height: {flexboxes:?}"
    );
}

#[tokio::test]
async fn fixed_width_block_flex_container_resolves_auto_margins() {
    let document = Html::from_string(
        "<style>@page { size: 300pt 120pt; margin: 20pt } body { margin: 0 }\
         .row { display: flex; width: 100pt; height: 20pt; margin-left: auto; margin-right: auto; background: green }\
         .item { width: 20pt; height: 20pt }</style>\
         <div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("centered flex container background should paint");
    assert!(
        (green.x() - 100.0).abs() < 0.01 && (green.width() - 100.0).abs() < 0.01,
        "fixed-width block flex container should resolve auto margins like normal block layout: {green:?}"
    );
}

#[tokio::test]
async fn horizontal_flex_img_items_use_flex_resolved_content_widths() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<!DOCTYPE html><style>\
         @page {{ size: 260px 260px; margin: 0 }} body {{ margin: 0 }}\
         div.flexbox {{ width: 200px; background: lightgreen; display: flex;\
           justify-content: space-between; margin-bottom: 5px; line-height: 8px }}\
         img {{ min-width: 0; width: 10px; height: 20px; border: 1px dotted green }}\
         </style>\
         <div class=\"flexbox\"><img src=\"{image}\"></div>\
         <div class=\"flexbox\">some words <img src=\"{image}\"></div>\
         <div class=\"flexbox\"><img src=\"{image}\" style=\"flex: 5\"><img src=\"{image}\" style=\"flex: 3\"></div>\
         <div class=\"flexbox\"><img src=\"{image}\" style=\"width: 33px; flex: 2 auto\"><img src=\"{image}\" style=\"width: 13px; flex: 3 auto\"></div>\
         <div class=\"flexbox\"><img src=\"{image}\" style=\"width: 150px; flex: 1 4 auto\"><img src=\"{image}\" style=\"width: 100px; flex: 1 3 auto\"></div>\
         <div class=\"flexbox\"><img src=\"{image}\" style=\"width: 33px; flex: 2 auto\"><img src=\"{image}\" style=\"width: 13px; max-width: 90px; flex: 3 auto\"></div>\
         <div class=\"flexbox\"><img src=\"{image}\" style=\"width: 33px; flex: 2 auto\"><img src=\"{image}\" style=\"width: 13px; min-width: 150px; flex: 3 auto\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let px = 0.75;
    let mut images = document.pages[0]
        .images()
        .iter()
        .filter(|image| !image.background)
        .collect::<Vec<_>>();
    images.sort_by(|left, right| {
        right
            .y()
            .partial_cmp(&left.y())
            .unwrap()
            .then_with(|| left.x().partial_cmp(&right.x()).unwrap())
    });
    let rows = images
        .chunk_by(|left, right| (left.y() - right.y()).abs() < 0.5)
        .map(|row| {
            row.iter()
                .map(|image| image.width() / px)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rows.len(),
        7,
        "expected one rendered image row for each flexbox: {images:?}"
    );
    assert!(
        images[1].x() / px > 185.0,
        "text and image should be separate flex items so space-between packs the image at the far edge: {images:?}"
    );
    let expected: &[&[f32]] = &[
        &[10.0],
        &[10.0],
        &[122.5, 73.5],
        &[93.0, 103.0],
        &[114.0, 82.0],
        &[106.0, 90.0],
        &[46.0, 150.0],
    ];
    for (row, expected_row) in rows.iter().zip(expected) {
        assert_eq!(
            row.len(),
            expected_row.len(),
            "unexpected image count in row: rows={rows:?}, images={images:?}"
        );
        for (actual, expected) in row.iter().zip(*expected_row) {
            assert!(
                (actual - expected).abs() < 0.25,
                "expected image content width {expected}px, got {actual}px; rows={rows:?}, images={images:?}"
            );
        }
    }
}

#[tokio::test]
async fn align_content_stretch_overflow_falls_back_to_wrap_reverse_flex_start() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>@page { size: 420px 420px; margin: 0 } body { margin: 0 }\
         #flex { display: flex; width: 200px; height: 200px; flex-wrap: wrap-reverse; align-content: stretch }\
         #item { width: 200px; height: 400px; background: linear-gradient(to bottom, red 50%, green 50%) }</style>\
         <p>Test passes if there is a filled green square and no red.</p>\
         <div style=\"overflow: hidden\"><div id=\"flex\"><div id=\"item\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let green_rect = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(green) && rect.width() > 149.0 && rect.height() > 149.0)
        .unwrap_or_else(|| panic!("expected clipped green square: {:?}", page.rects()));
    // CSS overflow is retained as an effect scope rather than destructively
    // rewriting the source geometry of its descendants. The raw paint tree
    // consequently still contains the red gradient half; verify the PDF
    // output retains the enclosing overflow clip that excludes it visually.
    let rendered = pdf_searchable_text(
        &document
            .write_pdf_bytes(&PdfOptions::default())
            .expect("document should serialize"),
    );
    assert!(
        rendered.contains("W\nn"),
        "align-content:stretch overflow fallback must clip the red half above {green_rect:?}"
    );
    for (x, y) in [(10.0, 10.0), (75.0, 75.0), (140.0, 140.0)] {
        assert_eq!(
            final_rect_fill_at(page, green_rect.x() + x, green_rect.y() + y),
            Some(green),
            "align-content:stretch overflow fallback should expose only the green half at ({x}, {y}) inside {green_rect:?}: {:?}",
            page.rects()
        );
    }
}

#[tokio::test]
async fn flex_shrink_to_fit_item_remeasures_against_column_line_cross_size() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>@page { size: 260px 240px; margin: 0 } body { margin: 0 }\
         #container { display: flex; flex-flow: column wrap; width: 100px; border-right: 100px solid red }\
         #item { align-self: start; background: linear-gradient(to bottom, red 50%, green 50%) }\
         .float { float: left; width: 100px; height: 100px; background: green }</style>\
         <div id=\"container\"><div id=\"item\"><div class=\"float\"></div><div class=\"float\"></div></div><div style=\"width: 200px\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);
    for (x, y) in [(10.0, 50.0), (140.0, 50.0), (10.0, 150.0), (140.0, 150.0)] {
        let x = x * 0.75;
        let y = y * 0.75;
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(green),
            "shrink-to-fit flex item should cover the 200px line cross-size at ({x}, {y}): {:?}",
            page.rects()
        );
        assert_ne!(
            final_rect_fill_at(page, x, y),
            Some(red),
            "final 200px square should not expose red at ({x}, {y}): {:?}",
            page.rects()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
        Some(CssColor::new(0, 128, 0)),
        "green flex container should fully cover the red reference square: {:?}",
        page.rects()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("min-content column flex background should paint");
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "column flex min-content cross size should be the largest item contribution, not the sum of wrapped columns: {green:?}"
    );
}

#[tokio::test]
async fn floated_column_wrap_flex_shrink_to_fit_width_matches_wpt() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <title>CSS Flexbox: multiline column flexboxes and shrink-to-fit.</title>\
         <style>\
         @page { size: 900px 700px; margin: 0 } body { margin: 0 }\
         .flexbox { display: flex; background-color: #aaa; position: relative;\
                    flex-wrap: wrap; flex-direction: column; float: left;\
                    align-content: flex-start }\
         .flexbox > * { flex: none }\
         .flexbox :nth-child(1) { background-color: lightblue }\
         .flexbox :nth-child(2) { background-color: lightgreen }\
         .flexbox :nth-child(3) { background-color: pink }\
         .flexbox :nth-child(4) { background-color: yellow }\
         </style>\
         <div class=\"flexbox\">\
           <div style=\"width: 100px; height: 20px\"></div>\
           <div style=\"width: 100px; height: 10px\"></div>\
           <div style=\"width: 100px; height: 10px\"></div>\
           <div style=\"width: 100px; height: 20px\"></div>\
         </div>\
         <p style=\"clear:left\">The grey background should be 100px wide.</p>\
         <div class=\"flexbox\" style=\"height: 30px\">\
           <div style=\"width: 100px; height: 20px\"></div>\
           <div style=\"width: 100px; height: 10px\"></div>\
           <div style=\"width: 100px; height: 10px\"></div>\
           <div style=\"width: 100px; height: 20px\"></div>\
         </div>\
         <p style=\"clear:left\">The grey background should be 100px wide.</p>\
         <div style=\"width: 150px\">\
           <div class=\"flexbox\">\
             <div style=\"width: 100px; height: 20px\"></div>\
             <div style=\"width: 100px; height: 10px\"></div>\
             <div style=\"width: 100px; height: 10px\"></div>\
             <div style=\"width: 100px; height: 20px\"></div>\
           </div>\
         </div>\
         <p style=\"clear:left\">The grey background should be 100px wide.</p>\
         <div style=\"width: 150px\">\
           <div class=\"flexbox\" style=\"height: 35px\">\
             <div style=\"width: 100px; height: 20px\"></div>\
             <div style=\"width: 100px; height: 10px\"></div>\
             <div style=\"width: 100px; height: 10px\"></div>\
             <div style=\"width: 100px; height: 20px\"></div>\
           </div>\
         </div>\
         <p style=\"clear:left\">The grey background should be 150px wide and 5px should stick out the bottom.</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grey = CssColor::new(170, 170, 170);
    let grey_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(grey))
        .collect::<Vec<_>>();
    assert_eq!(
        grey_rects.len(),
        4,
        "expected four flex container backgrounds: {:?}",
        page.rects()
    );
    for (index, (rect, (expected_width, expected_height))) in grey_rects
        .iter()
        .zip([(75.0, 45.0), (75.0, 22.5), (75.0, 45.0), (112.5, 26.25)])
        .enumerate()
    {
        assert!(
            (rect.width() - expected_width).abs() < 0.01
                && (rect.height() - expected_height).abs() < 0.01,
            "grey background {} should be {expected_width}pt by {expected_height}pt: {rect:?}",
            index + 1
        );
    }

    let last = grey_rects[3];
    assert_eq!(
        final_rect_fill_at(page, last.x() + 10.0, last.y() + 2.0),
        Some(grey),
        "fourth grey background should visibly stick out below the 30px-tall item columns: {:?}",
        page.rects()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
        Some(CssColor::new(0, 128, 0)),
        "green flex container should fully cover the red reference square: {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn intrinsic_row_flex_automatic_minimum_clamps_after_inflexible_basis() {
    let document = Html::from_string(
        "<style>@page { size: 300px 180px; margin: 0 } body { margin: 0 }\
         .flex { display: flex; width: max-content; height: 100px; background: green }\
         .item { flex: 0 0 0px; border: 10px solid transparent }\
         .child { width: 80px }\
         </style><div class=\"flex\"><div class=\"item\"><div class=\"child\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = page_rect_with_fill(&document.pages[0], CssColor::new(0, 128, 0));
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "the automatic minimum must floor the capped 0px flex basis at 80px content plus 20px border: {green:?}"
    );
}

#[tokio::test]
async fn intrinsic_row_flex_automatic_minimum_honors_preferred_size_suggestion() {
    let document = Html::from_string(
        "<style>@page { size: 300px 180px; margin: 0 } body { margin: 0 }\
         .flex { display: flex; width: max-content; height: 100px; background: green }\
         .item { width: 10px; flex: 0 0 0px; border: 10px solid transparent }\
         .child { width: 80px }\
         </style><div class=\"flex\"><div class=\"item\"><div class=\"child\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = page_rect_with_fill(&document.pages[0], CssColor::new(0, 128, 0));
    assert!(
        (green.width() - 22.5).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "the automatic minimum must use the 10px preferred-size suggestion before adding its 20px border: {green:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("max-content flex background should paint");
    assert!(
        (green.width() - 50.0).abs() < 0.01,
        "intrinsic flex width should resolve the cyclic percentage gap against zero: {green:?}"
    );
}

#[tokio::test]
async fn flex_definite_width_percentage_gap_resolves_against_content_box() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; width: 100pt; column-gap: calc(10pt + 50%); background: green }\
         .item { flex: 0 0 auto; width: 20pt; height: 10pt }\
         .a { background: red } .b { background: blue }\
         </style><div class=\"flex\"><div class=\"item a\"></div><div class=\"item b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first flex item should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second flex item should paint");
    assert!(
        rects_have_gap_x(red, blue, 60.0),
        "definite flex content width should resolve calc(10pt + 50%) to a 60pt gap: red={red:?}, blue={blue:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "collapsed flex item should not paint: {:?}",
        page.rects()
    );
    let container = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK) && rect.width() >= 89.0)
        .expect("flex container background should paint");
    assert!(
        container.height() >= 35.9,
        "collapsed flex item should leave a cross-size strut: {container:?}"
    );
}

#[tokio::test]
async fn visibility_collapse_preserves_column_cross_size_strut() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .column { display: inline-flex; flex-direction: column; background: black; align-items: flex-start }\
         .a { width: 10pt; height: 20pt; background: green }\
         .collapsed { visibility: collapse; width: 50pt; height: 20pt; background: red }\
         .b { width: 8pt; height: 20pt; background: blue }</style>\
         <div class=\"column\"><div class=\"a\"></div><div class=\"collapsed\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "collapsed column flex item should not paint: {:?}",
        page.rects()
    );
    let container = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .expect("inline-flex container background should paint");
    assert!(
        container.width() >= 49.9,
        "collapsed column flex item should leave a cross-size strut: {container:?}"
    );
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first flex item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second flex item should paint");
    assert!(
        rects_have_gap_y(green, blue, 0.0),
        "collapsed column flex item should not consume main-axis space: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn visibility_collapse_vertical_row_preserves_cross_size_strut() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display: inline-flex; writing-mode: vertical-rl; flex-direction: row; background: black; align-items: flex-start }\
         .a { width: 10pt; height: 20pt; background: green }\
         .collapsed { visibility: collapse; width: 50pt; height: 20pt; background: red }\
         .b { width: 8pt; height: 20pt; background: blue }</style>\
         <div class=\"row\"><div class=\"a\"></div><div class=\"collapsed\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "collapsed vertical-writing flex item should not paint: {:?}",
        page.rects()
    );
    let container = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .expect("vertical-writing inline-flex container background should paint");
    assert!(
        container.width() >= 49.9,
        "collapsed vertical-writing row flex item should leave a physical cross-size strut: {container:?}"
    );
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first vertical flex item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second vertical flex item should paint");
    assert!(
        rects_have_gap_y(green, blue, 0.0),
        "collapsed vertical-writing row flex item should not consume main-axis space: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn visibility_collapse_replaced_item_keeps_cross_strut_without_painting_image() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         .row {{ display: flex; width: 120pt; background: black; align-items: flex-start }}\
         .a {{ width: 20pt; height: 10pt; background: green }}\
         img {{ visibility: collapse; width: 40pt; height: 50pt }}\
         .b {{ width: 20pt; height: 10pt; background: blue }}</style>\
         <div class=\"row\"><div class=\"a\"></div><img src=\"{image}\"><div class=\"b\"></div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.images().is_empty(),
        "collapsed replaced flex item should not paint its image: {:?}",
        page.images()
    );
    let container = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK) && rect.width() >= 119.0)
        .expect("flex container background should paint");
    assert!(
        container.height() >= 49.9,
        "collapsed replaced flex item should leave a cross-size strut: {container:?}"
    );
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first flex item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second flex item should paint");
    assert!(
        (blue.x() - (green.x() + green.width())).abs() < 0.1,
        "collapsed replaced flex item should not consume main-axis space: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn visibility_collapse_column_replaced_item_keeps_cross_strut_without_painting_image() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 120pt; margin: 10pt }} body {{ margin: 0 }}\
         .column {{ display: inline-flex; flex-direction: column; background: black; align-items: flex-start }}\
         .a {{ width: 10pt; height: 20pt; background: green }}\
         img {{ visibility: collapse; width: 50pt; height: 20pt }}\
         .b {{ width: 8pt; height: 20pt; background: blue }}</style>\
         <div class=\"column\"><div class=\"a\"></div><img src=\"{image}\"><div class=\"b\"></div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.images().is_empty(),
        "collapsed column replaced flex item should not paint its image: {:?}",
        page.images()
    );
    let container = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .expect("inline-flex container background should paint");
    assert!(
        container.width() >= 49.9,
        "collapsed column replaced flex item should leave a cross-size strut: {container:?}"
    );
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first flex item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second flex item should paint");
    assert!(
        rects_have_gap_y(green, blue, 0.0),
        "collapsed column replaced flex item should not consume main-axis space: green={green:?}, blue={blue:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "collapsed flex item should not paint: {:?}",
        page.rects()
    );
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first flex line item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second flex line item should paint");
    assert!(
        green.y() - blue.y() >= 65.0,
        "collapsed strut should repack later wrapped lines: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn visibility_collapse_strut_reflows_wrap_reverse_lines() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-wrap: wrap-reverse; align-content: flex-start; width: 50pt }\
         .item { flex: 0 0 50pt; width: 50pt; height: 10pt }\
         .a { background: green }\
         .collapsed { visibility: collapse; height: 60pt; background: red }\
         .c { background: yellow }\
         .d { background: blue }</style>\
         <div class=\"row\"><div class=\"item a\"></div><div class=\"item collapsed\"></div><div class=\"item c\"></div><div class=\"item d\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "collapsed flex item should not paint: {:?}",
        page.rects()
    );
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first flex line item should paint");
    let yellow = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("strut-expanded flex line item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("last flex line item should paint");
    let first_gap = if green.y() <= yellow.y() {
        yellow.y() - (green.y() + green.height())
    } else {
        green.y() - (yellow.y() + yellow.height())
    };
    let second_gap = if yellow.y() <= blue.y() {
        blue.y() - (yellow.y() + yellow.height())
    } else {
        yellow.y() - (blue.y() + blue.height())
    };
    assert!(
        first_gap >= 45.0 || second_gap >= 45.0,
        "wrap-reverse collapsed strut should expand one adjacent flex line gap: green={green:?}, yellow={yellow:?}, blue={blue:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let big = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("large baseline participant should render");
    let small = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("small baseline participant should render");
    assert!(
        (big.y() - small.y()).abs() < 0.01,
        "baseline-aligned flex items should share a baseline: big={big:?}, small={small:?}"
    );
}

#[tokio::test]
async fn baseline_fallback_preserves_specified_cross_size_with_negative_margin() {
    let document = Html::from_string(
        "<style>@page { size: 80px 80px; margin: 0 } body { margin: 0 }\
         .flex { align-items: baseline; background: red; display: flex; height: 40px; width: 40px }\
         .item { background: yellow; border: 1px solid black; flex: 1; height: 20px; margin-top: -4px }\
         </style><div class=\"flex\"><div class=\"item\">a</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let flex = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("flex container background should paint");
    let item = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("baseline-aligned flex item background should paint");
    let px = 0.75;
    assert!(
        (item.height() - 22.0 * px).abs() < 0.01,
        "the background paints the specified content box and its two border edges: {item:?}"
    );
    assert!(
        ((item.y() + item.height()) - (flex.y() + flex.height()) - 4.0 * px).abs() < 0.01,
        "baseline fallback aligns the item's margin edge at flex cross-start, so its negative cross-start margin lets the border edge extend before that edge: flex={flex:?}, item={item:?}"
    );
}

#[tokio::test]
async fn wrapped_row_align_items_baseline_preserves_line_cross_slots() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>\
         @page { size: 340px 140px; margin: 0 } body { margin: 0 } p { display: none }\
         #flexbox {\
           background-color: red;\
           background-image: linear-gradient(to bottom,\
             green 0, green 16px, red 17px, red 35px,\
             green 36px, green 66px, red 67px, red 85px,\
             green 86px, green 100px);\
           align-items: baseline; display: flex; flex-flow: wrap;\
           height: 100px; width: 300px;\
         }\
         #flexbox > div {\
           background-color: green; color: green;\
           font: 20px/20px monospace; height: 40px; width: 75px;\
         }\
         #div3, #div7 { font-size: 40px; line-height: 40px }\
         </style>\
         <p>Test passes if there is no red visible on the page.</p>\
         <div id=\"flexbox\">\
           <div id=\"div1\">d1</div><div id=\"div2\">d2</div>\
           <div id=\"div3\">d3</div><div id=\"div4\">d4</div>\
           <div id=\"div5\">d5</div><div id=\"div6\">d6</div>\
           <div id=\"div7\">d7</div><div id=\"div8\">d8</div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let px = 0.75;
    let green = Some(CssColor::new(0, 128, 0));
    let flexbox = page
        .rects()
        .iter()
        .find(|rect| {
            (rect.width() - 300.0 * px).abs() < 0.01 && (rect.height() - 100.0 * px).abs() < 0.01
        })
        .unwrap_or_else(|| panic!("expected flex container background: {:?}", page.rects()));
    for y_px in [25.0, 75.0] {
        for x_px in [37.5, 112.5, 187.5, 262.5] {
            assert_eq!(
                final_rect_fill_at(page, flexbox.x() + x_px * px, flexbox.y() + y_px * px),
                green,
                "baseline-aligned wrapped row should cover red band sample at ({x_px}px, {y_px}px): {:?}",
                page.rects()
            );
        }
    }
}

#[tokio::test]
async fn baseline_aligned_row_items_use_block_child_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; align-items: baseline; width: 140pt; font-size: 25pt; line-height: 25pt }\
         .row * { margin: 0 }</style>\
         <div class=\"row\"><span>XX</span><div><div>YY</div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let direct = page
        .lines()
        .iter()
        .find(|line| line.text == "XX")
        .expect("direct flex item text should render");
    let nested = page
        .lines()
        .iter()
        .find(|line| line.text == "YY")
        .expect("block-descendant flex item text should render");
    assert!(
        (direct.y() - nested.y()).abs() < 0.01,
        "baseline-aligned row flex items should use an in-flow block descendant baseline: direct={direct:?}, nested={nested:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_row_items_use_block_child_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; align-items: last baseline; width: 180pt }\
         .row * { margin: 0 }\
         .small { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <div class=\"row\"><div><div class=\"small\">Top</div><div class=\"big\">Last</div></div><span class=\"small\">Peer</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines()
        .iter()
        .find(|line| line.text == "Last")
        .expect("last block-descendant flex item baseline participant should render");
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "Peer")
        .expect("peer last-baseline participant should render");
    assert!(
        (nested_last.y() - peer.y()).abs() < 0.01,
        "last-baseline row flex items should use the last in-flow block descendant baseline: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_column_items_share_vertical_text_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .column { display: flex; flex-direction: column; align-items: baseline; width: 120pt; height: 120pt }\
         p { margin: 0; writing-mode: vertical-rl; text-orientation: mixed }\
         .wide { font-size: 30pt; line-height: 30pt; background: red }\
         .narrow { font-size: 10pt; line-height: 10pt; background: blue }</style>\
         <div class=\"column\"><p class=\"wide\">A</p><p class=\"narrow\">B</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let wide = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("wide column-axis baseline participant should paint");
    let narrow = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("narrow column-axis baseline participant should paint");
    let wide_baseline = wide.x() + wide.width() - 15.0;
    let narrow_baseline = narrow.x() + narrow.width() - 5.0;
    assert!(
        (wide_baseline - narrow_baseline).abs() < 0.01,
        "column-axis baseline sharing should align vertical text baselines: wide={wide:?}, narrow={narrow:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_column_items_share_vertical_text_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 180pt; margin: 10pt } body { margin: 0 }\
         .column { display: flex; flex-direction: column; align-items: last baseline; width: 120pt; height: 140pt }\
         p { margin: 0; writing-mode: vertical-rl; text-orientation: mixed }\
         .wide { font-size: 30pt; line-height: 30pt; background: red }\
         .narrow { font-size: 10pt; line-height: 10pt; background: blue }</style>\
         <div class=\"column\"><p class=\"wide\">A<br>B</p><p class=\"narrow\">C<br>D</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let wide = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("wide last-baseline participant should paint");
    let narrow = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("narrow last-baseline participant should paint");
    let wide_baseline = wide.x() + wide.width() - 45.0;
    let narrow_baseline = narrow.x() + narrow.width() - 15.0;
    assert!(
        (wide_baseline - narrow_baseline).abs() < 0.01,
        "column-axis last-baseline sharing should align last vertical text baselines: wide={wide:?}, narrow={narrow:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("nested flex baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("nested wrapped flex first baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_startmost = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("nested row-reverse first-line startmost baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_startmost = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("nested wrap-reverse startmost line baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("nested max-width wrapped first baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("nested fit-content wrapped first baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("nested flex last baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("nested fit-content wrapped last baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_endmost = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("nested max-content gap baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("nested max-width wrapped last baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_endmost = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("nested wrap-reverse endmost line baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_endmost = page
        .lines()
        .iter()
        .find(|line| line.text == "E")
        .expect("nested row-reverse last-line endmost baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("nested wrapped flex last baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("nested vertical first baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("nested vertical last baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("auto-width nested vertical first baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("auto-width nested vertical last baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("nested vertical wrap-reverse first baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("nested vertical wrap-reverse last baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first =
        page.lines().iter().find(|line| line.text == "B").expect(
            "auto-width nested vertical wrap-reverse first baseline participant should render",
        );
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last =
        page.lines().iter().find(|line| line.text == "A").expect(
            "auto-width nested vertical wrap-reverse last baseline participant should render",
        );
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("percentage-width nested vertical first baseline participant should render");
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("percentage-width nested vertical last baseline participant should render");
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical last baseline participant should render");
    assert!(
        (nested_last.x() - peer.x()).abs() < 0.01,
        "percentage-width nested vertical flex exported last horizontal baseline should join the outer group: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_items_use_indefinite_percentage_width_nested_vertical_exported_baseline()
 {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: inline-flex; flex-direction: row; align-items: baseline; writing-mode: vertical-lr;\
                  height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                   width: 50%; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page.lines().iter().find(|line| line.text == "A").expect(
        "indefinite percentage-width nested vertical first baseline participant should render",
    );
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical first baseline participant should render");
    assert!(
        (nested_first.x() - peer.x()).abs() < 0.01,
        "indefinite percentage-width nested vertical flex exported first horizontal baseline should join the outer group: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_vertical_row_items_use_indefinite_percentage_width_nested_vertical_exported_baseline()
 {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: inline-flex; flex-direction: row; align-items: last baseline; writing-mode: vertical-lr;\
                  height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap; writing-mode: vertical-lr;\
                   width: 50%; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page.lines().iter().find(|line| line.text == "B").expect(
        "indefinite percentage-width nested vertical last baseline participant should render",
    );
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical last baseline participant should render");
    assert!(
        (nested_last.x() - peer.x()).abs() < 0.01,
        "indefinite percentage-width nested vertical flex exported last horizontal baseline should join the outer group: nested_last={nested_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_items_use_indefinite_percentage_width_nested_wrap_reverse_exported_baseline()
 {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: inline-flex; flex-direction: row; align-items: baseline; writing-mode: vertical-lr;\
                  height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap-reverse; writing-mode: vertical-lr;\
                   width: 50%; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page.lines().iter().find(|line| line.text == "B").expect(
        "indefinite percentage-width nested vertical wrap-reverse first baseline participant should render",
    );
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical first baseline participant should render");
    assert!(
        (nested_first.x() - peer.x()).abs() < 0.01,
        "indefinite percentage-width nested vertical wrap-reverse flex exported first horizontal baseline should join the outer group: nested_first={nested_first:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn last_baseline_aligned_vertical_row_items_use_indefinite_percentage_width_nested_wrap_reverse_exported_baseline()
 {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0 }\
         .outer { display: inline-flex; flex-direction: row; align-items: last baseline; writing-mode: vertical-lr;\
                  height: 80pt }\
         .nested { display: flex; flex-direction: row; flex-wrap: wrap-reverse; writing-mode: vertical-lr;\
                   width: 50%; height: 40pt }\
         .item { flex: 0 0 40pt; width: 20pt; margin: 0 }\
         p { margin: 0; font-size: 10pt; line-height: 10pt }</style>\
         <div class=\"outer\"><div class=\"nested\"><p class=\"item\">A</p><p class=\"item\">B</p></div><p>C</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page.lines().iter().find(|line| line.text == "A").expect(
        "indefinite percentage-width nested vertical wrap-reverse last baseline participant should render",
    );
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "C")
        .expect("peer vertical last baseline participant should render");
    assert!(
        (nested_last.x() - peer.x()).abs() < 0.01,
        "indefinite percentage-width nested vertical wrap-reverse flex exported last horizontal baseline should join the outer group: nested_last={nested_last:?}, peer={peer:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_first = page.lines().iter().find(|line| line.text == "B").expect(
        "percentage-width nested vertical wrap-reverse first baseline participant should render",
    );
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let nested_last = page.lines().iter().find(|line| line.text == "A").expect(
        "percentage-width nested vertical wrap-reverse last baseline participant should render",
    );
    let peer = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let big = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("first flex line should render");
    let small = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let first = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("first vertical flex line should render");
    let second = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let first = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("first vertical flex line should render");
    let second = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let first_last = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("last baseline of first flex line should render");
    let second = page
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("flex container background should paint");
    let first_green = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("absolutely positioned flex child should paint");
    assert!(
        (red.x() - 50.0).abs() < 0.5,
        "static position should honor main-axis flex centering: {red:?}"
    );
}

#[tokio::test]
async fn abspos_flex_static_alignment_consumes_fixed_margins_once() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .container { position: relative; display: flex; justify-content: flex-end; align-items: flex-end; width: 20pt; height: 14pt; padding: 1pt 2pt; border: 1pt solid black }\
         .abs { position: absolute; width: 8pt; height: 6pt }\
         .fixed { margin: 1pt 2pt 3pt 4pt; background: red }\
         .auto { margin: auto; background: green }</style>\
         <div class=\"container\"><div class=\"abs fixed\"></div><div class=\"abs auto\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("fixed-margin absolutely positioned flex child should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("auto-margin absolutely positioned flex child should paint");

    assert!(
        (red.x() - 13.0).abs() < 0.01 && (red.y() - 87.0).abs() < 0.01,
        "fixed margins must be consumed once from the flex static rectangle: {red:?}"
    );
    assert!(
        (green.x() - 15.0).abs() < 0.01 && (green.y() - 84.0).abs() < 0.01,
        "auto margins must be zero for the flex static-position probe: {green:?}"
    );
}

#[tokio::test]
async fn inline_source_abspos_flex_child_uses_flex_static_rect_for_auto_horizontal_insets() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .container { position: relative; display: flex; justify-content: center; align-items: center; width: 100pt; height: 40pt }\
         .ref, .abs { position: absolute; width: 20pt; height: 10pt }\
         .ref { background: red } .abs { background: green }</style>\
         <div class=\"container\"><div class=\"ref\"></div><span class=\"abs\"></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("block-source absolutely positioned flex child should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("inline-source absolutely positioned flex child should paint");
    assert!(
        (green.x() - red.x()).abs() < 0.01,
        "inline-source abspos flex child should share the block-source flex static x: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn display_contents_inline_flex_item_normalizes_mixed_flow_children() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0; font: 12pt/1 serif }\
         .flex { display: flex } .contents { display: contents } .inline { display: inline }</style>\
         <div class=\"flex\"><div class=\"contents\"><div class=\"inline\">2a<div>2</div></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.lines().iter().any(|line| line.text == "2a"),
        "leading inline text in a blockified flex item should render: {:?}",
        page.lines()
    );
    assert!(
        page.lines().iter().any(|line| line.text == "2"),
        "block descendant in the same flex item should still render: {:?}",
        page.lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first display: contents child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second display: contents child should paint");
    let yellow = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
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
    .render(&RenderOptions::default())
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_display_contents_wpt_flex_output(&document);
}

fn assert_display_contents_wpt_flex_output(document: &quire::Document) {
    let page = &document.pages[0];
    for text in ["0x", "y", "2a", "3", "4", "5a", "5b", "6", "8", "10"] {
        assert!(
            page.lines().iter().any(|line| line.text == text),
            "expected rendered text {text:?}: {:?}",
            page.lines()
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

fn rendered_line<'a>(
    page: &'a quire::Page,
    text: &str,
) -> &'a crate::document::paint::text::RenderedLine {
    page.lines()
        .iter()
        .find(|line| line.text == text)
        .unwrap_or_else(|| panic!("expected rendered text {text:?}: {:?}", page.lines()))
}

fn glyph_start_x(line: &crate::document::paint::text::RenderedLine, index: usize) -> f32 {
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

fn glyph_center_x(line: &crate::document::paint::text::RenderedLine, index: usize) -> f32 {
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

fn assert_blue_at_text_position(
    page: &quire::Page,
    line: &crate::document::paint::text::RenderedLine,
    x: f32,
) {
    assert!(
        blue_rect_covers_text_position(page, line, x),
        "expected blue background at x={x} for {line:?}; rects={:?}",
        page.rects()
    );
}

fn assert_no_blue_at_text_position(
    page: &quire::Page,
    line: &crate::document::paint::text::RenderedLine,
    x: f32,
) {
    assert!(
        !blue_rect_covers_text_position(page, line, x),
        "suppressed .contents.c2 should not paint blue background at x={x} for {line:?}; rects={:?}",
        page.rects()
    );
}

fn blue_rect_covers_text_position(
    page: &quire::Page,
    line: &crate::document::paint::text::RenderedLine,
    x: f32,
) -> bool {
    page.rects().iter().any(|rect| {
        rect.fill == Some(CssColor::new(0, 0, 255))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let line_x = |text: &str| {
        page.lines()
            .iter()
            .find(|line| line.text == text)
            .map(|line| line.x())
            .unwrap_or_else(|| panic!("expected rendered text {text:?}: {:?}", page.lines()))
    };
    let nine_x = line_x("9");
    let a_x = line_x("a");
    let b_x = line_x("b");
    assert!(
        a_x - nine_x < 12.0 && b_x - a_x > 20.0,
        "9 and a should be contiguous inside one anonymous flex item, with the flex gap before b: {:?}",
        page.lines()
    );
    let blue_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        3,
        "each 40pt column item should move to a fresh 60pt page area"
    );
    for (page_index, page) in document.pages.iter().enumerate() {
        assert!(
            page.rects()
                .iter()
                .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0))
                    && (rect.height() - 40.0).abs() < 0.01),
            "page {page_index} should contain one flex item: {:?}",
            page.rects()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "wrapped row flex lines should fragment at line boundaries"
    );
    for (page_index, page) in document.pages.iter().enumerate() {
        assert!(
            page.rects()
                .iter()
                .any(|rect| rect.fill == Some(CssColor::new(0, 0, 255))
                    && (rect.height() - 40.0).abs() < 0.01),
            "page {page_index} should contain one row flex line: {:?}",
            page.rects()
        );
    }
}

#[tokio::test]
async fn stretched_wrapped_row_uses_nested_wrapped_flex_full_cross_contribution() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt }\
         body { margin: 0; font-size: 12pt; line-height: 12pt }\
         .outer { display: flex; flex-wrap: wrap; width: 180pt }\
         h4, dl, dt, dd, p { margin: 0 }\
         h4 { flex: 1 25% }\
         dl { display: flex; flex: 1 75%; flex-wrap: wrap }\
         dt { width: 30% } dd { flex: 1 70% }\
         .after { margin-top: 6pt }</style>\
         <div class=\"outer\"><h4>Heading</h4><dl>\
         <dt>Alpha</dt><dd>first</dd><dt>Beta</dt><dd>second</dd>\
         <dt>Gamma</dt><dd>third</dd></dl></div><p class=\"after\">After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let alpha = rendered_line(page, "Alpha");
    let beta = rendered_line(page, "Beta");
    let gamma = rendered_line(page, "Gamma");
    let after = rendered_line(page, "After");

    assert!(
        alpha.y() > beta.y() + 0.01 && beta.y() > gamma.y() + 0.01,
        "each nested wrapped row must occupy a later block position: alpha={alpha:?}, beta={beta:?}, gamma={gamma:?}"
    );
    assert!(
        gamma.y() > after.y() + 11.9,
        "following normal-flow content must begin after the nested flex item's final row: gamma={gamma:?}, after={after:?}"
    );
}

#[tokio::test]
async fn fragmented_row_flex_clones_container_background() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-wrap: wrap; width: 60pt; background: black }\
         .item { width: 50pt; height: 40pt; background: blue }</style>\
         <div class=\"row\"><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "wrapped row flex lines should still fragment at line boundaries"
    );
    for (page_index, page) in document.pages.iter().enumerate() {
        assert!(
            page.rects()
                .iter()
                .any(|rect| rect.fill == Some(CssColor::BLACK)
                    && (rect.width() - 60.0).abs() < 0.01
                    && rect.height() >= 39.9),
            "page {page_index} should contain a cloned flex container background: {:?}",
            page.rects()
        );
    }
}

/// An overflowing descendant of a stretched flex item keeps the inline size
/// established in its first multicolumn fragment. The two inputs correspond to
/// WPT `flex-item-content-overflow-001a` and `-001b`; the latter reaches the
/// same used geometry through content-box sizing.
/// <https://drafts.csswg.org/css-flexbox-1/#pagination>
async fn assert_multicol_flex_item_overflow_matches_block_reference(
    case_name: &str,
    box_sizing: &str,
    multicol_inline_size: u16,
    multicol_block_size: u16,
    flexbox_inline_size: u16,
    flexbox_block_size: u16,
    grandchild_block_size: u16,
) {
    let actual = Html::from_string(format!(
        "<style>{box_sizing}\
         .multicol {{ columns: 2; column-gap: 0; column-fill: auto; inline-size: {multicol_inline_size}px; block-size: {multicol_block_size}px; border: 10px solid purple }}\
         .flexbox {{ display: flex; inline-size: {flexbox_inline_size}px; block-size: {flexbox_block_size}px; border: 10px solid black }}\
         .item {{ flex: 1; border: 10px solid teal }}\
         .grandchild {{ border: 10px solid orange; block-size: {grandchild_block_size}px }}\
         </style><div class=\"multicol\"><div class=\"flexbox\"><div class=\"item\"><div class=\"grandchild\"></div></div></div></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // This is the local equivalent of the WPT reference: an ordinary block
    // with the flex item's resolved 50px border-box block size. It makes the
    // expected second-column continuation explicit without depending on a WPT
    // checkout or another renderer.
    let reference = Html::from_string(
        "<style>div { box-sizing: border-box }\
         .multicol { columns: 2; column-gap: 0; column-fill: auto; inline-size: 300px; block-size: 100px; border: 10px solid purple }\
         .flexbox { display: block; inline-size: 130px; block-size: 70px; border: 10px solid black }\
         .item { block-size: 50px; border: 10px solid teal }\
         .grandchild { border: 10px solid orange; block-size: 140px }\
         </style><div class=\"multicol\"><div class=\"flexbox\"><div class=\"item\"><div class=\"grandchild\"></div></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(actual.pages.len(), 1, "{case_name}: unexpected pagination");
    assert_eq!(
        reference.pages.len(),
        1,
        "{case_name}: invalid local reference"
    );

    let border_colors = [
        CssColor::new(128, 0, 128),
        CssColor::BLACK,
        CssColor::new(0, 128, 128),
        CssColor::new(255, 165, 0),
    ];
    let actual_borders = actual.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill.is_some_and(|fill| border_colors.contains(&fill)))
        .collect::<Vec<_>>();
    let reference_borders = reference.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill.is_some_and(|fill| border_colors.contains(&fill)))
        .collect::<Vec<_>>();

    assert_eq!(
        actual_borders, reference_borders,
        "{case_name}: flex overflow must preserve the complete colored border sequence across the column break"
    );
    assert_eq!(
        actual.pages[0].rects(),
        reference.pages[0].rects(),
        "{case_name}: the continuation must retain the first fragment's inline geometry"
    );
}

/// Regression for CSS Break's multicolumn fragmentation of overflowing flex
/// item content, under both border-box and content-box input sizing.
#[tokio::test]
async fn multicol_flex_item_overflow_preserves_first_fragment_inline_size() {
    for (
        case_name,
        box_sizing,
        multicol_inline_size,
        multicol_block_size,
        flexbox_inline_size,
        flexbox_block_size,
        grandchild_block_size,
    ) in [
        (
            "border-box",
            "div { box-sizing: border-box }",
            300,
            100,
            130,
            70,
            140,
        ),
        ("content-box", "", 280, 80, 110, 50, 120),
    ] {
        assert_multicol_flex_item_overflow_matches_block_reference(
            case_name,
            box_sizing,
            multicol_inline_size,
            multicol_block_size,
            flexbox_inline_size,
            flexbox_block_size,
            grandchild_block_size,
        )
        .await;
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "forced break-before on a flex item should break between flex fragments"
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0))),
        "first item should stay on page 1"
    );
    assert!(
        document.pages[1]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0))),
        "second item should render on page 2"
    );
}

#[tokio::test]
async fn wrapped_row_forced_first_item_restarts_and_packs_following_lines() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .intro { width: 140pt; height: 20pt; background: black }\
         .row { display: flex; flex-wrap: wrap; width: 140pt }\
         .full { width: 140pt; height: 20pt; break-after: avoid; background: red }\
         .subtitle { width: 140pt; height: 20pt; background: yellow }\
         .card { width: 40pt; height: 20pt; background: blue }</style>\
         <div class=\"intro\"></div><div class=\"row\">\
         <div class=\"full\" style=\"break-before: page\"></div>\
         <div class=\"subtitle\"></div><div class=\"card\"></div>\
         <div class=\"card\"></div><div class=\"card\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "a forced first flex item must restart wrapped-line packing on its destination page"
    );
    let destination_rects = document.pages[1].rects();
    assert_eq!(
        destination_rects
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .count(),
        1,
        "the forced full-width item should begin the destination page"
    );
    assert!(
        destination_rects
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 255, 0))),
        "the following subtitle line should fit on the forced destination page"
    );
    assert_eq!(
        destination_rects
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        3,
        "the following card line should fit on the forced destination page"
    );
}

#[tokio::test]
async fn wrapped_row_forced_later_item_keeps_prior_line_and_packs_remainder() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .intro { width: 140pt; height: 20pt; background: black }\
         .row { display: flex; flex-wrap: wrap; width: 140pt }\
         .before { width: 40pt; height: 20pt; background: blue }\
         .full { width: 140pt; height: 20pt; break-after: avoid; background: red }\
         .subtitle { width: 140pt; height: 20pt; background: yellow }\
         .card { width: 40pt; height: 20pt; background: green }</style>\
         <div class=\"intro\"></div><div class=\"row\">\
         <div class=\"before\"></div><div class=\"before\"></div><div class=\"before\"></div>\
         <div class=\"full\" style=\"break-before: page\"></div>\
         <div class=\"subtitle\"></div><div class=\"card\"></div>\
         <div class=\"card\"></div><div class=\"card\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "a later forced flex item must not leave blank intermediate pages"
    );
    assert_eq!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        3,
        "the wrapped line before the forced item should remain on page 1"
    );
    let destination_rects = document.pages[1].rects();
    assert!(
        destination_rects
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0))),
        "the forced item should begin page 2"
    );
    assert!(
        destination_rects
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 255, 0))),
        "the subtitle after the forced item should remain on page 2"
    );
    assert_eq!(
        destination_rects
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .count(),
        3,
        "the later card line should remain on page 2"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "forced break-after on the final flex item should break after the flex container"
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0))),
        "flex item should render before the break"
    );
    assert!(
        document.pages[1]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 0, 255))),
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "oversized column flex item should split over two 60pt fragmentainers"
    );
    let first = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first item slice should paint");
    let second = document.pages[1]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("second item slice should paint");
    assert!(
        (first.height() - 60.0).abs() < 0.5 && (second.height() - 40.0).abs() < 0.5,
        "item slices should match fragmentainer remainder: first={first:?}, second={second:?}"
    );
}

#[tokio::test]
async fn split_flex_grid_item_keeps_selected_root_decoration_and_grid_child_paint() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; flex-direction: column; width: 40pt }\
         .item { display: grid; width: 40pt; height: 100pt; background: rgb(255, 0, 0) }\
         .child { height: 100pt; background: rgb(0, 128, 0) }</style>\
         <div class=\"flex\"><div class=\"item\"><div class=\"child\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "the flex item must split across pages"
    );
    for (page_index, page) in document.pages.iter().enumerate() {
        let red = page
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .collect::<Vec<_>>();
        let green = page
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .collect::<Vec<_>>();
        assert_eq!(
            red.len(),
            1,
            "page {page_index} must paint one item root slice: {red:?}"
        );
        assert_eq!(
            green.len(),
            1,
            "page {page_index} must retain its grid child slice: {green:?}"
        );
    }
}

#[tokio::test]
async fn oversized_wrapped_row_flex_line_splits_across_pages() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; flex-wrap: wrap; width: 50pt }\
         .item { width: 50pt; height: 100pt; background: blue }</style>\
         <div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "oversized row flex line should split over two 60pt fragmentainers"
    );
    let first = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("first line slice should paint");
    let second = document.pages[1]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages.len(),
        2,
        "the oversized item should split across the two page areas"
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.height() - 60.0).abs() < 0.5),
        "first page should paint the first child content: {:?}",
        document.pages[0].rects()
    );
    assert!(
        document.pages[1]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.height() - 40.0).abs() < 0.5),
        "second page should paint the later child content: {:?}",
        document.pages[1].rects()
    );
    assert!(
        !document.pages[1]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0))),
        "second page must not replay the start of the flex item: {:?}",
        document.pages[1].rects()
    );
}
