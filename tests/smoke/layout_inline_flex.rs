use super::*;

fn first_visible_glyph_x(line: &quire::RenderedLine) -> f32 {
    for run in &line.runs {
        let mut pen_x = line.x() + run.x_offset;
        if let Some(glyphs) = &run.glyphs {
            for glyph in glyphs {
                if !glyph.unicode.chars().all(char::is_whitespace) {
                    return pen_x + glyph.x_offset;
                }
                pen_x += glyph.x_advance;
            }
        }
    }
    line.x()
}

#[tokio::test]
async fn split_inline_after_block_omits_inline_start_border_for_wpt_case() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<meta charset="utf-8">
<title>CSS 2.1 Test Suite: handling of blocks inside inlines</title>
<style>
  body > span { border: 3px solid blue }
</style>
<body>
  <span
  ><div>One</div>
    Two
  </span>
</body>"#,
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let blue_rects = page
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .collect::<Vec<_>>();
    assert_eq!(
        blue_rects.len(),
        3,
        "split inline final fragment should paint top, bottom, and inline-end edges only: {blue_rects:?}"
    );

    let vertical_edges = blue_rects
        .iter()
        .copied()
        .filter(|rect| rect.width() < rect.height())
        .collect::<Vec<_>>();
    assert_eq!(
        vertical_edges.len(),
        1,
        "only the inline-end border should remain vertical: {blue_rects:?}"
    );

    let two = page
        .lines
        .iter()
        .find(|line| line.text.trim() == "Two")
        .expect("post-block inline text should render");
    let two_start = first_visible_glyph_x(two);
    assert!(
        vertical_edges[0].x() > two_start,
        "vertical split-inline border should be at inline end, not inline start: two={two:?}, border={:?}",
        vertical_edges[0]
    );
}

#[tokio::test]
async fn supports_explicit_block_dimensions() {
    let document = Html::from_string(
        "<div style=\"margin: 0; width: 50pt; height: 20pt; padding: 2pt; border: 1pt solid black; background: red\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rect = &document.pages[0].rects[0];
    assert_eq!(rect.width(), 56.0);
    assert_eq!(rect.height(), 26.0);
}

#[tokio::test]
async fn padded_block_text_uses_content_box_once() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"margin:0;padding-left:10pt;font-size:10pt;line-height:10pt\">Text</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].x(), 20.0);
}

#[tokio::test]
async fn supports_percentage_block_widths() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 }</style><div style=\"margin:0; width:50%; height:10pt; background:red\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rect = &document.pages[0].rects[0];
    assert_eq!(rect.width(), 90.0);
}

#[tokio::test]
async fn supports_flex_row_space_between() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 }</style><div style=\"display:flex; justify-content:space-between; width:100pt; font-size:10pt; line-height:10pt\"><span>A</span><span>B</span></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "A");
    assert_eq!(document.pages[0].lines[1].text, "B");
    assert_eq!(document.pages[0].lines[0].x(), 10.0);
    assert!(document.pages[0].lines[1].x() >= 100.0);
}

#[tokio::test]
async fn rtl_fixed_width_flex_container_uses_physical_right_edge() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; direction: rtl }\
         .flex { display: flex; width: 100pt; height: 10pt; margin: 0; background: red }\
         </style><div class=\"flex\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let flex_background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("flex background should paint");
    assert!(
        (flex_background.x() - 90.0).abs() < 0.01,
        "fixed-width flex container in RTL should align to the containing block's right edge: {flex_background:?}"
    );
}

#[tokio::test]
async fn flex_row_space_between_single_item_falls_back_to_start() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 500pt 220pt; margin: 10pt } body { margin: 0 }\
         div { background: blue; margin: 1em 0; border: 1px solid black; height: 8em; width: 30em; display: flex; justify-content: space-between }\
         span { background: white; margin: 1em; width: 5em; max-width: 6em; display: inline-block; flex: 1 0 0% }</style>\
         <div><span>one</span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let white = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 255, 255)))
        .expect("single flex item background should paint");
    assert!(
        (white.x() - 22.75).abs() < 0.01,
        "space-between single item should use flex-start fallback: {white:?}"
    );
    assert!(
        (white.width() - 72.0).abs() < 0.01,
        "flex item should be clamped by max-width: {white:?}"
    );
}

#[tokio::test]
async fn flex_row_single_item_space_around_and_evenly_fall_back_to_center() {
    for justify_content in ["space-around", "space-evenly"] {
        let document = Html::from_string(format!(
            "<style>@page {{ size: 240pt 100pt; margin: 10pt }} body {{ margin: 0 }}\
             .row {{ display:flex; justify-content:{justify_content}; width:200pt }}\
             .item {{ width:40pt; height:10pt; background:green }}\
             </style><div class=\"row\"><div class=\"item\"></div></div>",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

        let green = document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
            .unwrap();
        assert!(
            (green.x() - 90.0).abs() < 0.01,
            "{justify_content} single item should center: {green:?}"
        );
    }
}

#[tokio::test]
async fn flex_column_rejustifies_after_replaced_auto_minimum_growth() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 140pt 140pt; margin: 10pt }} body {{ margin: 0 }}\
         .col {{ display:flex; flex-direction:column; justify-content:space-around; width:75pt; height:100pt }}\
         img {{ width:75pt; flex:0 1 0% }}\
         </style><div class=\"col\"><img src=\"{image}\"></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let image = document.pages[0]
        .images
        .iter()
        .find(|image| !image.background)
        .expect("replaced flex item should paint");
    assert!((image.height() - 75.0).abs() < 0.01, "image={image:?}");
    assert!(
        (image.y() - 42.5).abs() < 0.01,
        "space-around should center the item after automatic minimum growth: {image:?}"
    );
}

#[tokio::test]
async fn flex_basis_min_content_counts_inline_atoms() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; width: 100pt; font-size: 10pt; line-height: 12pt }\
         .item { flex: 0 0 min-content; background: red }\
         .atom { display: inline-block; width: 34pt; height: 4pt; margin-left: 4pt }</style>\
         <div class=\"row\"><div class=\"item\">A<span class=\"atom\"></span></div><div>B</div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    assert!(
        item.width() >= 37.5,
        "flex min-content basis should include the inline atom: {item:?}"
    );
}

#[tokio::test]
async fn flex_max_content_uses_graph_generated_inline_edges_and_atoms() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .row { display: flex; width: 180pt }\
         .item { flex: 0 0 max-content; background: red }\
         .item::before { content: 'XX' }\
         .edge { padding-left: 20pt; padding-right: 10pt; border-left: 5pt solid transparent; border-right: 5pt solid transparent; text-transform: uppercase }\
         .atom { display: inline-block; width: 30pt; height: 4pt }</style>\
         <div class=\"row\"><div class=\"item\"><span class=\"edge\">z</span><span class=\"atom\"></span></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    assert!(
        item.width() > 68.0,
        "flex max-content should include generated text, inline edges, and the atom: {item:?}"
    );
}

#[tokio::test]
async fn anonymous_flex_text_preserves_graph_measured_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt; white-space: break-spaces }\
         .row { display: flex; width: 200pt }\
         .marker { width: 20pt; height: 10pt; background: green }</style>\
         <div class=\"row\">A     B<div class=\"marker\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let marker = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    assert!(
        marker.x() > 35.0,
        "anonymous flex text should reserve preserved spaces before the marker: {marker:?}"
    );
}

#[tokio::test]
async fn column_flex_min_content_height_uses_graph_selected_atom_lines() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .column { display: flex; flex-direction: column; width: 25pt; height: 80pt }\
         .item { max-height: min-content; flex-basis: 80pt; background: green }\
         .atom { display: inline-block; width: 20pt; height: 10pt }</style>\
         <div class=\"column\"><div class=\"item\"><span class=\"atom\"></span><span class=\"atom\"></span></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    assert!(
        item.height() > 19.0 && item.height() < 31.0,
        "max-height:min-content should clamp to the two graph-selected atom lines: {item:?}"
    );
}

#[tokio::test]
async fn nested_flex_intrinsics_use_styled_inline_graph_contributions() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .row { display: flex; width: 220pt }\
         .nested { display: flex; flex: 0 0 auto; background: red }\
         .item { flex: 0 0 max-content }\
         .item::before { content: 'AA' }\
         .styled { letter-spacing: 4pt; padding-left: 12pt; border-left: 4pt solid transparent; text-transform: uppercase }\
         .atom { display: inline-block; width: 18pt; height: 6pt }</style>\
         <div class=\"row\"><div class=\"nested\"><div class=\"item\"><span class=\"styled\">bb</span><span class=\"atom\"></span></div></div><div>Tail</div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let nested = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    assert!(
        nested.width() > 48.0,
        "nested flex intrinsic width should include generated text, styling, and atoms: {nested:?}"
    );
}

#[tokio::test]
async fn flex_min_content_block_size_uses_wrapped_graph_fragments() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt }\
         .column { display: flex; flex-direction: column; width: 31pt; height: 100pt }\
         .item { max-height: min-content; flex-basis: 100pt; background: green }\
         .word { letter-spacing: 2pt }</style>\
         <div class=\"column\"><div class=\"item\"><span class=\"word\">AB</span> <span class=\"word\">CD</span> <span class=\"word\">EF</span></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    assert!(
        item.height() > 23.0 && item.height() < 40.0,
        "min-content block-size should come from graph-selected wrapped line fragments: {item:?}"
    );
}

#[tokio::test]
async fn direct_inline_replaced_row_height_uses_graph_atomic_metrics() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .row { display: flex; width: 120pt }\
         .item { flex: 0 0 auto; background: red }\
         .atom { display: inline-block; width: 8pt; height: 32pt }</style>\
         <div class=\"row\"><div class=\"item\"><svg width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\" fill=\"blue\" /></svg><span class=\"atom\"></span></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    assert!(
        item.height() > 31.0,
        "direct inline replaced rows should use graph atom metrics for height: {item:?}"
    );
}

#[tokio::test]
async fn justify_content_left_uses_physical_left_in_row_reverse() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; flex-direction:row-reverse; justify-content:left; width:200pt; height:30pt }\
         .item { width:30pt; height:20pt }\
         </style><div class=\"row\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:green\"></div><div class=\"item\" style=\"background:blue\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - 10.0).abs() < 0.01, "blue={blue:?}");
    assert!((green.x() - 40.0).abs() < 0.01, "green={green:?}");
    assert!((red.x() - 70.0).abs() < 0.01, "red={red:?}");
}

#[tokio::test]
async fn justify_content_end_uses_logical_end_in_rtl_row_reverse() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; direction:rtl; flex-direction:row-reverse; justify-content:end; width:200pt; height:30pt }\
         .item { width:30pt; height:20pt }\
         </style><div class=\"row\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:green\"></div><div class=\"item\" style=\"background:blue\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!((red.x() - 10.0).abs() < 0.01, "red={red:?}");
    assert!((green.x() - 40.0).abs() < 0.01, "green={green:?}");
    assert!((blue.x() - 70.0).abs() < 0.01, "blue={blue:?}");
}

#[tokio::test]
async fn justify_content_physical_left_right_fall_back_to_start_on_column_axis() {
    for justify_content in ["left", "right"] {
        let document = Html::from_string(format!(
            "<style>@page {{ size: 180pt 160pt; margin: 10pt }} body {{ margin:0 }}\
             .col {{ display:flex; flex-direction:column-reverse; justify-content:{justify_content}; width:100pt; height:80pt }}\
             .item {{ width:30pt; height:20pt }}\
             </style><div class=\"col\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:green\"></div><div class=\"item\" style=\"background:blue\"></div></div>",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

        let red = document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
            .unwrap();
        let green = document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
            .unwrap();
        let blue = document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
            .unwrap();

        assert!(
            (blue.y() - 130.0).abs() < 0.01,
            "{justify_content}: blue={blue:?}"
        );
        assert!(
            (green.y() - 110.0).abs() < 0.01,
            "{justify_content}: green={green:?}"
        );
        assert!(
            (red.y() - 90.0).abs() < 0.01,
            "{justify_content}: red={red:?}"
        );
    }
}

#[tokio::test]
async fn adjacent_flex_container_vertical_margins_collapse_as_block_siblings() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 180pt; margin: 10pt } body { margin:0 }\
         .flex { display:flex; width:40pt; height:20pt; margin:10pt 0; background:blue }\
         </style><div class=\"flex\"></div><div class=\"flex\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let blue_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .collect::<Vec<_>>();

    assert_eq!(blue_rects.len(), 2);
    let gap = blue_rects[0].y() - (blue_rects[1].y() + blue_rects[1].height());
    assert!(
        (gap - 10.0).abs() < 0.01,
        "adjacent sibling margins should collapse to 10pt, not add to 20pt: {blue_rects:?}"
    );
}

#[tokio::test]
async fn supports_flex_column_space_around() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 160pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } .col { display:flex; flex-direction:column; justify-content:space-around; height:100pt }</style><div class=\"col\"><span>A</span><span>B</span></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let a = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "B")
        .unwrap();

    let a_top = rendered_line_baseline_top(&document, a);
    let b_top = rendered_line_baseline_top(&document, b);
    assert!((a_top - 130.0).abs() < 1.0, "A top={a_top}");
    assert!((b_top - 80.0).abs() < 1.0, "B top={b_top}");
}

#[tokio::test]
async fn column_flex_overflow_hidden_clips_centered_item_border_box() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 } .flex { display:flex; flex-direction:column; align-items:center; overflow:hidden; width:70pt; height:70pt } .big { background:blue; width:10pt; border:solid coral; border-width:2pt 50pt; flex:3 } .small { background:teal; width:20pt; flex:1 }</style>\
         <div class=\"flex\"><div class=\"big\"></div><div class=\"small\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let coral = Color::new(255, 127, 80);
    let coral_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(coral))
        .collect::<Vec<_>>();

    assert!(!coral_rects.is_empty());
    assert!(
        coral_rects
            .iter()
            .all(|rect| rect.x() >= 10.0 && rect.x() + rect.width() <= 80.0)
    );
    assert!(
        coral_rects
            .iter()
            .any(|rect| (rect.x() - 10.0).abs() < 0.01 && (rect.width() - 30.0).abs() < 0.01)
    );
    assert!(
        coral_rects
            .iter()
            .any(|rect| (rect.x() - 50.0).abs() < 0.01 && (rect.width() - 30.0).abs() < 0.01)
    );
}

#[tokio::test]
async fn align_self_self_end_uses_item_writing_mode_on_row_cross_axis() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .row { display:inline-flex; height:100pt; border:1pt dashed blue; vertical-align:top }\
         .item { width:30pt; height:20pt; margin:1pt 2pt 3pt 4pt; border:2pt dotted black; padding:3pt; }\
         .self-start { align-self:self-start; background:yellow }\
         .self-end { align-self:self-end; writing-mode:vertical-lr; direction:rtl; background:purple }\
         </style><div class=\"row\"><div class=\"item self-start\"></div><div class=\"item self-end\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let yellow = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 255, 0)))
        .unwrap();
    let purple = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(128, 0, 128)))
        .unwrap();

    assert!(
        (yellow.y() - purple.y()).abs() < 0.01,
        "vertical-rl/rtl self-end should align its inline-end/top side to the flex row cross-start: yellow={yellow:?}, purple={purple:?}"
    );
}

#[tokio::test]
async fn align_self_self_end_can_target_row_cross_end_from_item_writing_mode() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; height:80pt; width:100pt }\
         .item { width:20pt; height:20pt; margin:0 }\
         .reference { align-self:flex-end; background:green }\
         .target { align-self:self-end; writing-mode:vertical-lr; direction:ltr; background:red }\
         </style><div class=\"row\"><div class=\"item reference\"></div><div class=\"item target\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let reference = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let target = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        (reference.y() - target.y()).abs() < 0.01,
        "vertical-lr/ltr self-end should align its inline-end/bottom side to row cross-end: reference={reference:?}, target={target:?}"
    );
}

#[tokio::test]
async fn align_self_self_end_uses_item_writing_mode_on_column_cross_axis() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .column { display:flex; flex-direction:column; width:80pt; height:80pt }\
         .item { width:20pt; height:20pt; margin:0 }\
         .reference { align-self:flex-end; background:green }\
         .target { align-self:self-end; writing-mode:horizontal-tb; direction:ltr; background:red }\
         </style><div class=\"column\"><div class=\"item reference\"></div><div class=\"item target\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let reference = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let target = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        (reference.x() - target.x()).abs() < 0.01,
        "horizontal/ltr self-end should align its inline-end/right side to column cross-end: reference={reference:?}, target={target:?}"
    );
}

#[tokio::test]
async fn align_items_self_end_is_inherited_by_auto_align_self() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; align-items:self-end; height:80pt; width:100pt }\
         .item { width:20pt; height:20pt; margin:0; writing-mode:vertical-lr; direction:ltr }\
         .reference { align-self:flex-end; background:green }\
         .target { background:red }\
         </style><div class=\"row\"><div class=\"item reference\"></div><div class=\"item target\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let reference = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let target = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        (reference.y() - target.y()).abs() < 0.01,
        "align-self:auto should inherit align-items:self-end and align the vertical item's inline-end/bottom side: reference={reference:?}, target={target:?}"
    );
}

#[tokio::test]
async fn safe_self_end_falls_back_to_cross_start_when_item_overflows() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; height:20pt; width:100pt }\
         .start { width:20pt; height:20pt; background:green }\
         .target { width:20pt; height:40pt; align-self:safe self-end; background:red }\
         </style><div class=\"row\"><div class=\"start\"></div><div class=\"target\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let start = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let target = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        ((start.y() + start.height()) - (target.y() + target.height())).abs() < 0.01,
        "safe self-end should fall back to row cross-start when the item overflows: start={start:?}, target={target:?}"
    );
}

#[tokio::test]
async fn shrink_to_fit_inline_block_includes_consecutive_float_row_width() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin:10pt } body { margin:0 }\
         .box { display:inline-block; background:red; vertical-align:top }\
         .box > div { float:left; width:25pt; height:10pt }\
         </style><div class=\"box\"><div></div><div></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        (red.width() - 50.0).abs() < 0.01,
        "inline-block shrink-to-fit width should include both consecutive floats: {red:?}"
    );
}

#[tokio::test]
async fn inline_block_auto_height_expands_to_contain_internal_float() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 140pt; margin: 10pt }\
         body, div { margin: 0 }\
         .atom { display: inline-block; background: rgb(0 128 0) }\
         .float { float: left; width: 30pt; height: 40pt; background: rgb(0 0 255) }\
         </style>\
         <div><span class=\"atom\"><span class=\"float\"></span></span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let atom = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        atom.height() >= 39.99,
        "inline-block background should include its internal float: {atom:?}"
    );
}

#[tokio::test]
async fn flex_row_height_uses_pre_line_item_line_count() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } .row { display: flex; margin: 0 0 20pt } .item { white-space: pre-line; flex: 1 } p { margin: 0 }</style><div class=\"row\"><div class=\"item\">One\nTwo\nThree</div><div>Side</div></div><p>After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let one = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "One")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((one.y() - after.y() - 56.0).abs() < 0.01);
}

#[tokio::test]
async fn flex_row_height_uses_tallest_pre_line_item_line_count() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 180pt; margin: 10pt } body { margin: 0; font-size: 11pt; line-height: 17.6pt } .row { display: flex; margin: 0 0 44pt } .from, .to { white-space: pre-line } .from { flex: 1 }</style><div class=\"row\"><address class=\"from\">One\nTwo\nThree\nFour</address><address class=\"to\">A\nB\nC</address></div><p style=\"margin:0\">After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let one = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "One")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((one.y() - after.y() - 114.4).abs() < 0.01);
}

#[tokio::test]
async fn flex_row_height_counts_preserved_leading_newline_in_pre_line_item() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } dt::before { content: ''; display: block } .row { display: flex; margin: 0 0 20pt } .item { white-space: pre-line; flex: 1 } p { margin: 0 }</style><div class=\"row\"><address class=\"item\">\nOne\nTwo</address></div><p>After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let one = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "One")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    // CSS Text preserves the leading segment break in `white-space: pre-line`,
    // so this item has three 12pt line boxes: an empty line, `One`, and `Two`.
    // The first visible line is one line below the item top; the following
    // paragraph is separated from it by the two remaining item lines plus the
    // row's 20pt bottom margin.
    assert!(
        (one.y() - after.y() - 44.0).abs() < 0.01,
        "one.y()={} after.y()={}",
        one.y(),
        after.y()
    );
}

#[tokio::test]
async fn supports_flex_grow() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }</style><div style=\"display:flex; width:200pt\"><div style=\"flex-grow:1; height:10pt; background:red\"></div><div style=\"width:50pt; height:10pt; background:blue\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].rects[0].width(), 150.0);
    assert_eq!(document.pages[0].rects[1].width(), 50.0);
}

#[tokio::test]
async fn floats_after_a_block_start_below_that_block() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } p { margin: 0 }</style>\
         <p>Intro\
         <div style=\"float:left; width:25pt; height:20pt; background:green\"></div>\
         <div style=\"float:left; width:25pt; height:20pt; background:green\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let intro = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Intro")
        .expect("paragraph text should render");
    let green_tops = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .map(|rect| rect.y() + rect.height())
        .collect::<Vec<_>>();

    assert_eq!(green_tops.len(), 2);
    assert!(
        green_tops.iter().all(|top| *top <= intro.y() + 0.5),
        "floats should start after the preceding block line: line={intro:?}, tops={green_tops:?}"
    );
}

#[tokio::test]
async fn adjacent_left_floats_share_row_and_overflow_moves_down() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         div { float: left; width: 45pt; height: 20pt; background: green }</style>\
         <div></div><div></div><div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .collect::<Vec<_>>();

    assert_eq!(rects.len(), 3);
    assert!((rects[0].x() - 10.0).abs() < 0.01, "rects={rects:?}");
    assert!((rects[1].x() - 55.0).abs() < 0.01, "rects={rects:?}");
    assert!((rects[2].x() - 10.0).abs() < 0.01, "rects={rects:?}");
    assert!(rects[2].y() < rects[0].y(), "rects={rects:?}");
}

#[tokio::test]
async fn mixed_left_and_right_floats_use_opposite_edges() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 100pt; margin: 10pt } body { margin: 0 }\
         .left { float: left; width: 30pt; height: 20pt; background: green }\
         .right { float: right; width: 30pt; height: 20pt; background: blue }</style>\
         <div class=\"left\"></div><div class=\"right\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!((green.x() - 10.0).abs() < 0.01, "green={green:?}");
    assert!((blue.x() - 100.0).abs() < 0.01, "blue={blue:?}");
    assert!(
        (green.y() - blue.y()).abs() < 0.01,
        "green={green:?} blue={blue:?}"
    );
}

#[tokio::test]
async fn clear_both_moves_block_below_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 40pt; height: 20pt; background: green }\
         .clear { clear: both; width: 40pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"clear\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "clear block should start below float: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn following_text_wraps_around_left_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 30pt; height: 20pt; background: green }</style>\
         <div class=\"float\"></div>one two three four",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let first = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("one"))
        .unwrap();

    assert!(
        first.x() >= 39.0,
        "first text line should be shortened by the float: {first:?}"
    );
}

#[tokio::test]
async fn inline_float_after_text_does_not_shift_previous_text() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         span { float: left; width: 30pt; height: 20pt; background: green }</style>\
         <p style=\"margin:0\">Before <span></span>After after after</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("Before"))
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("After"))
        .unwrap();
    let green_top = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .map(|rect| rect.y() + rect.height())
        .unwrap();

    assert!(
        (before.x() - 10.0).abs() < 0.01,
        "text before the float should keep the original line start: {before:?}"
    );
    assert!(
        green_top > after.y() + 5.0,
        "inline float that fits after prefix text should be placed on the prefix line: before={before:?}, after={after:?}, green_top={green_top}"
    );
    assert!(
        (before.y() - after.y()).abs() < 0.01,
        "suffix text after a fitting inline float should remain on the prefix line: before={before:?}, after={after:?}"
    );
    assert!(
        after.x() >= 39.0,
        "text after the waiting inline float should avoid the float: {after:?}"
    );
}

#[tokio::test]
async fn inline_float_after_text_defers_when_remaining_band_is_too_narrow() {
    let document = Html::from_string(
        "<style>@page { size: 118pt 120pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         span { float: left; width: 80pt; height: 20pt; background: green }</style>\
         <p style=\"margin:0\">Before <span></span>After after after</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("Before"))
        .unwrap();
    let green_top = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .map(|rect| rect.y() + rect.height())
        .unwrap();

    assert!(
        green_top < before.y(),
        "inline float should defer when it cannot fit after prefix text: before={before:?}, green_top={green_top}"
    );
}

#[tokio::test]
async fn inline_float_nowrap_does_not_break_before_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         div { width: 10ch; white-space: nowrap; font: 10pt/10pt monospace }\
         span { float: right; width: 5ch; height: 5ch; background: blue }</style>\
         <div>Some text that <span></span> overflows my parent.</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();
    let line_y = text_lines[0].y();
    assert!(
        text_lines
            .iter()
            .all(|line| (line.y() - line_y).abs() < 0.01),
        "nowrap inline float should keep all text on one visual line: {text_lines:?}"
    );
    let text = text_lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<String>();
    assert!(
        text.contains("Some text that") && text.contains("overflows my parent."),
        "nowrap line should contain prefix and suffix text: {:?}",
        text_lines
    );

    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();
    assert!(
        blue.x() > text_lines[0].x() + 20.0,
        "right float should be placed at the right side of the nowrap band: blue={blue:?}, line={:?}",
        text_lines[0]
    );
    assert!(
        blue.y() + blue.height() > text_lines[0].y() + 5.0,
        "right float should be placed at the nowrap line top: blue={blue:?}, line={:?}",
        text_lines[0]
    );
}

#[tokio::test]
async fn inline_left_float_nowrap_keeps_text_unbroken() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         div { width: 10ch; white-space: nowrap; font: 10pt/10pt monospace }\
         span { float: left; width: 5ch; height: 5ch; background: green }</style>\
         <div>Some text that <span></span> overflows my parent.</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();
    let line_y = text_lines[0].y();
    assert!(
        text_lines
            .iter()
            .all(|line| (line.y() - line_y).abs() < 0.01),
        "left nowrap inline float should keep all text on one visual line: {text_lines:?}"
    );
    let text = text_lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<String>();
    assert!(
        text.contains("Some text that") && text.contains("overflows my parent."),
        "nowrap line should contain prefix and suffix text: {:?}",
        text_lines
    );

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    assert!(
        green.x() <= text_lines[0].x() + 0.01,
        "left float should be placed at the left side of the nowrap band: green={green:?}, line={:?}",
        text_lines[0]
    );
}

#[tokio::test]
async fn multiple_inline_floats_nowrap_preserve_same_side_order() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         div { width: 10ch; white-space: nowrap; font: 10pt/10pt monospace }\
         .first { float: right; width: 2ch; height: 4ch; background: blue }\
         .second { float: right; width: 2ch; height: 4ch; background: red }</style>\
         <div>Some text <span class=\"first\"></span><span class=\"second\"></span> overflows.</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text_lines = document.pages[0]
        .lines
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();
    let line_y = text_lines[0].y();
    assert!(
        text_lines
            .iter()
            .all(|line| (line.y() - line_y).abs() < 0.01),
        "multiple nowrap inline floats should keep all text on one visual line: {text_lines:?}"
    );

    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    assert!(
        blue.x() > red.x(),
        "same-side right floats should keep source-order placement: blue={blue:?}, red={red:?}"
    );
}

#[tokio::test]
async fn flow_root_float_does_not_leak_to_following_text() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         .root { display: flow-root } .float { float: left; width: 30pt; height: 20pt; background: green }</style>\
         <div class=\"root\"><div class=\"float\"></div></div><p style=\"margin:0\">After</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((after.x() - 10.0).abs() < 0.01, "after={after:?}");
}

#[tokio::test]
async fn flex_container_avoids_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 30pt; height: 30pt; background: green }\
         .flex { display: flex; width: 60pt; height: 10pt; background: blue }</style>\
         <div class=\"float\"></div><div class=\"flex\"><span></span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!(blue.x() >= 39.0, "flex should avoid active float: {blue:?}");
}

#[tokio::test]
async fn table_wrapper_avoids_active_left_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: left; width: 30pt; height: 30pt; background: green }\
         table { width: 60pt; height: 10pt; background: blue }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!(
        blue.x() >= 39.0,
        "table should avoid active float: {blue:?}"
    );
}

#[tokio::test]
async fn table_wrapper_moves_below_floats_when_band_is_too_narrow() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .left { float: left; width: 40pt; height: 20pt; background: green }\
         .right { float: right; width: 40pt; height: 20pt; background: blue }\
         table { width: 30pt; height: 10pt; background: red }</style>\
         <div class=\"left\"></div><div class=\"right\"></div><table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "table should move below floats when no band is wide enough: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn clear_both_moves_table_wrapper_below_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: left; width: 40pt; height: 20pt; background: green }\
         table { clear: both; width: 40pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "clear table should start below float: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn clear_left_moves_table_wrapper_below_left_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: left; width: 40pt; height: 20pt; background: green }\
         table { clear: left; width: 40pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "clear-left table should start below left float: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn clear_right_moves_table_wrapper_below_right_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: right; width: 40pt; height: 20pt; background: green }\
         table { clear: right; width: 40pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "clear-right table should start below right float: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn empty_table_wrapper_uses_float_avoidance() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body, table { margin: 0; border-spacing: 0 }\
         .float { float: left; width: 30pt; height: 30pt; background: green }\
         table { width: 50pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><table></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= 39.0,
        "empty table should avoid active float: {red:?}"
    );
}

#[tokio::test]
async fn table_cell_float_does_not_leak_to_following_parent_text() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body, table, td, p { margin: 0; padding: 0; border-spacing: 0; font: 10pt/10pt monospace }\
         .cell-float { float: left; width: 30pt; height: 20pt; background: green }</style>\
         <table><tr><td><div class=\"cell-float\"></div></td></tr></table><p>After</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((after.x() - 10.0).abs() < 0.01, "after={after:?}");
}

#[tokio::test]
async fn float_exclusions_do_not_leak_to_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 30pt; height: 40pt; background: green }\
         .break { break-before: page }</style>\
         <div class=\"float\"></div><div class=\"break\">Next</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let next = document.pages[1]
        .lines
        .iter()
        .find(|line| line.text == "Next")
        .unwrap();

    assert!((next.x() - 10.0).abs() < 0.01, "next={next:?}");
}

#[tokio::test]
async fn fragmented_float_excludes_following_text_on_later_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, p { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 40pt } .chunk { height: 45pt; background: green }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F<br>G<br>H<br>I<br>J</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let continued = document.pages[1]
        .lines
        .iter()
        .find(|line| !line.text.trim().is_empty())
        .unwrap();

    assert!(
        continued.x() >= 49.0,
        "continued text should avoid the fragmented float on page 2: {continued:?}"
    );
}

#[tokio::test]
async fn clear_both_after_fragmented_float_starts_below_current_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, div { margin: 0 }\
         .float { float: left; width: 40pt } .chunk { height: 45pt; background: green }\
         .after { clear: both; width: 20pt; height: 10pt; background: red }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <div class=\"after\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red = document
        .pages
        .iter()
        .flat_map(|page| &page.rects)
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let last_green = document
        .pages
        .iter()
        .flat_map(|page| &page.rects)
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .min_by(|left, right| {
            (left.y() + left.height())
                .partial_cmp(&(right.y() + right.height()))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();

    assert!(
        red.y() + red.height() <= last_green.y() + 0.01,
        "clear after a fragmented float should start below the continued fragment: red={red:?} green={last_green:?}"
    );
}

#[tokio::test]
async fn clear_both_after_three_fragment_float_clears_final_continuation() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, div { margin: 0 }\
         .float { float: left; width: 40pt } .chunk { height: 45pt; background: green }\
         .after { clear: both; width: 20pt; height: 10pt; background: red }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <div class=\"after\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let (red_page, red) = document
        .pages
        .iter()
        .enumerate()
        .flat_map(|(page_index, page)| page.rects.iter().map(move |rect| (page_index, rect)))
        .find(|(_, rect)| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let (green_page, last_green) = document
        .pages
        .iter()
        .enumerate()
        .flat_map(|(page_index, page)| page.rects.iter().map(move |rect| (page_index, rect)))
        .filter(|(_, rect)| rect.fill == Some(Color::new(0, 128, 0)))
        .max_by(|(left_page, left), (right_page, right)| {
            left_page.cmp(right_page).then_with(|| {
                left.y()
                    .partial_cmp(&right.y())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        })
        .unwrap();

    assert_eq!(
        red_page, green_page,
        "clear should wait for the final continued float fragment before placing the following box"
    );
    assert!(
        red.y() + red.height() <= last_green.y() + 0.01,
        "clear after a three-fragment float should start below the final fragment: red={red:?} green={last_green:?}"
    );
}

#[tokio::test]
async fn fragmented_float_preserves_bookmark_side_effects() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body, h2 { margin: 0; font: 10pt/10pt sans-serif }\
         .float { float: left; width: 60pt } .chunk { height: 45pt }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><h2>Float Mark</h2><div class=\"chunk\"></div></div>\
         <p>After</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let bookmark = document
        .bookmarks
        .iter()
        .find(|bookmark| bookmark.label == "Float Mark")
        .unwrap();

    assert_eq!(bookmark.page_index, 1, "bookmark={bookmark:?}");
}

#[tokio::test]
async fn fragmented_float_preserves_anchor_for_generated_page_reference() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 80pt; margin: 10pt; @bottom-center { content: target-counter(url(#float-anchor), page); font-size: 8pt; height: 10pt } }\
         body, div, h2 { margin: 0; font: 10pt/10pt sans-serif }\
         .float { float: left; width: 60pt } .chunk { height: 45pt }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><h2 id=\"float-anchor\">Float Anchor</h2><div class=\"chunk\"></div></div>\
         <p>After</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| &page.lines)
            .any(|line| line.text == "2"),
        "page-margin generated content should resolve the anchor inside the fragmented float"
    );
}

#[tokio::test]
async fn fragmented_float_preserves_named_string_for_page_margin_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 80pt; margin: 10pt; @top-center { content: string(float_title); font-size: 8pt; height: 10pt } }\
         body, h2 { margin: 0; font: 10pt/10pt sans-serif }\
         h2 { string-set: float_title content(text) }\
         .float { float: left; width: 60pt } .chunk { height: 45pt }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><h2>Float String</h2><div class=\"chunk\"></div></div>\
         <div>After<br>Line<br>Line<br>Line<br>Line<br>Line<br>Line<br>Line<br>Line</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[1]
            .lines
            .iter()
            .any(|line| line.text == "Float String" && line.y() > 65.0),
        "page 2 top margin should use the named string captured inside the fragmented float"
    );
}

#[tokio::test]
async fn fragmented_float_preserves_svg_replaced_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 60pt } .chunk { height: 45pt }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><svg width=\"10pt\" height=\"10pt\"><rect width=\"10pt\" height=\"10pt\" fill=\"blue\"/></svg><div class=\"chunk\"></div></div>\
         <p>After</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| &page.rects)
            .any(|rect| rect.fill == Some(Color::new(0, 0, 255))),
        "replaced SVG descendant inside a fragmented float should survive replay"
    );
}

#[tokio::test]
async fn fragmented_float_preserves_generated_before_content() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt; @bottom-center { content: target-text(url(#generated), before) target-text(url(#generated), content); font: 8pt/8pt sans-serif; height: 10pt } }\
         body, div { margin: 0; font: 10pt/10pt sans-serif }\
         .float { float: left; width: 80pt } .chunk { height: 45pt }\
         .generated::before { content: 'Float Generated '; }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div id=\"generated\" class=\"generated\"> Body</div><div class=\"chunk\"></div></div>\
         <p>After</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| &page.lines)
            .any(|line| line.text.contains("Float Generated Body")),
        "generated pseudo text inside a fragmented float should survive anchor-text replay: {:?}",
        document
            .pages
            .iter()
            .flat_map(|page| &page.lines)
            .map(|line| line.text.clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn fragmented_float_preserves_generated_image_content() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 140pt 80pt; margin: 10pt }} body, div {{ margin: 0; font: 10pt/10pt sans-serif }}\
         .float {{ float: left; width: 80pt }} .chunk {{ height: 45pt }}\
         .generated::before {{ content: url({png}) ' '; width: 8pt; height: 6pt }}</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"generated\">Icon</div><div class=\"chunk\"></div></div>\
         <p>After</p>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .map(|page| page.images.len())
            .sum::<usize>()
            >= 1,
        "generated image content inside a fragmented float should survive replay"
    );
}

#[tokio::test]
async fn vertical_writing_inline_start_float_does_not_match_physical_clear_left() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 100pt; margin: 10pt } body { margin: 0; writing-mode: vertical-rl; direction: ltr }\
         .float { float: inline-start; width: 30pt; height: 20pt; background: green }\
         .clear { clear: left; width: 20pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"clear\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() > green.y(),
        "physical clear:left should not match a vertical inline-start top-side float: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_text_avoids_inline_start_top_float() {
    let normal = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body, p { margin: 0; writing-mode: vertical-rl; direction: ltr; font: 10pt/12pt sans-serif }</style>\
         <p>After</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body, p { margin: 0; writing-mode: vertical-rl; direction: ltr; font: 10pt/12pt sans-serif }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }</style>\
         <div class=\"float\"></div><p>After</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("After"))
        .unwrap();
    let normal_after = normal.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("After"))
        .unwrap();

    assert!(
        after.y() < normal_after.y() - 25.0 && after.y() <= green.y() + 0.5,
        "vertical text should start below the top-side logical float: normal={normal_after:?}, green={green:?}, after={after:?}"
    );
}

#[tokio::test]
async fn vertical_writing_over_tall_bfc_moves_past_top_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         .bfc { overflow: hidden; width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"bfc\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "over-tall vertical BFC should move to the next block-axis slab: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn orthogonal_bfc_consumes_parent_vertical_float_band() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         .bfc { writing-mode: horizontal-tb; overflow: hidden; width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"bfc\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "orthogonal horizontal BFC should consume the parent vertical float band: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_bfc_moves_past_bottom_side_insufficient_span() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-end; width: 24pt; height: 30pt; background: green }\
         .bfc { overflow: hidden; width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"bfc\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "vertical BFC should move past a bottom-side float when the remaining span is too small: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_bfc_root_avoids_inline_start_top_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-rl; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         .bfc { overflow: hidden; width: 24pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"bfc\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.5,
        "vertical BFC root should be placed below the top-side logical float: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_table_wrapper_moves_past_over_tall_top_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         table { width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "vertical table wrapper should move to the next block-axis slab: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_flex_container_moves_past_over_tall_top_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         .flex { display: flex; width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"flex\"><span></span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "vertical flex container should move to the next block-axis slab: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_lr_inline_end_float_uses_bottom_side() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-end; width: 24pt; height: 30pt; background: green }</style>\
         <div class=\"float\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 10.0).abs() < 0.5,
        "vertical-lr inline-end float should sit against the physical bottom side: green={green:?}"
    );
}

#[tokio::test]
async fn table_float_exclusions_do_not_leak_to_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: left; width: 30pt; height: 40pt; background: green }\
         table { break-before: page; width: 40pt; height: 10pt; background: blue }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[1]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - 10.0).abs() < 0.01, "blue={blue:?}");
}

#[tokio::test]
async fn broken_left_float_excludes_lines_on_each_visible_fragment_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, p { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 40pt } .chunk { height: 40pt; background: green }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F<br>G<br>H<br>I<br>J<br>K<br>L</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages.len() >= 2);
    for page_index in 0..2 {
        assert!(
            document.pages[page_index].rects.iter().any(|rect| {
                rect.fill == Some(Color::new(0, 128, 0))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
            }),
            "float should paint on page {page_index}"
        );
        let line = document.pages[page_index]
            .lines
            .iter()
            .find(|line| line.text.len() == 1)
            .expect("body text should share the float page");
        assert!(
            line.x() > 45.0,
            "left float should shorten lines on page {page_index}, line={line:?}"
        );
    }
}

#[tokio::test]
async fn broken_left_float_exclusion_ends_after_last_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, p { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 40pt } .chunk { height: 30pt; background: green }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F<br>G<br>H<br>I<br>J<br>K<br>L<br>M<br>N</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line_after_float = document
        .pages
        .iter()
        .flat_map(|page| &page.lines)
        .find(|line| line.text == "G")
        .expect("text should continue after the broken float");
    assert!(
        (line_after_float.x() - 10.0).abs() < 0.01,
        "float exclusion should end after the final fragment: {line_after_float:?}"
    );
}

#[tokio::test]
async fn positioned_descendant_stays_inside_broken_float_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, p { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 40pt }\
         .chunk { height: 40pt; background: blue; position: relative }\
         span { position: absolute; z-index: 1; left: 5pt; top: 0; width: 10pt; height: 10pt; background: red }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"><span></span></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F<br>G<br>H</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[1];
    let float_background = first_rect_paint_operation_index(page, Color::new(0, 0, 255));
    let positioned_child = first_rect_paint_operation_index(page, Color::new(255, 0, 0));

    assert!(
        float_background < positioned_child,
        "positioned child should paint inside the second float fragment stacking context"
    );
}

#[tokio::test]
async fn paginates_wpt_flex_reference_float_prefix_without_looping() {
    let row_widths = [
        "3ch", "3ch", "4ch", "3ch", "3ch", "4ch", "3ch", "0.4ch", "4ch", "3ch", "3ch", "4ch",
        "0.2ch", "0.2ch", "0.2ch", "3ch", "3ch", "4ch", "4.5ch", "4.5ch", "4.5ch", "3ch", "3ch",
        "4ch",
    ];
    let col_heights = ["1em", "1em", "1.5em", "1em", "1em", "1.5em", "1em"];
    let mut html = String::from(
        "<style>\
         body { display: grid; grid-template-columns: repeat(auto-fill, 66px 66px 66px); grid-auto-rows: 50px; font: 10px/1 monospace }\
         .wrap { counter-increment: test }\
         .row, .col { background: blue; padding: 5px; float: left }\
         .item { padding: 3px; border: 2px solid aqua; color: orange }\
         </style>",
    );

    for width in row_widths {
        html.push_str(&format!(
            "<div class=\"wrap\"><div class=\"row\"><div class=\"item\" style=\"width:{width}\">X X</div></div></div>"
        ));
    }
    for (index, height) in col_heights.iter().enumerate() {
        let grid_column = if index == 0 {
            " style=\"counter-reset:test; grid-column:1\""
        } else {
            ""
        };
        html.push_str(&format!(
            "<div class=\"wrap\"{grid_column}><div class=\"col\"><div class=\"item\" style=\"height:{height}\">X</div></div></div>"
        ));
    }

    let document = Html::from_string(html)
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert!(!document.pages.is_empty());
    assert!(
        document.pages.len() < 20,
        "float pagination should make progress, pages={}",
        document.pages.len()
    );
}

#[tokio::test]
async fn local_wpt_flex_intrinsic_reference_floats_finish_if_available() {
    let wpt_root = std::path::Path::new("../quire-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }

    for reference in [
        "css/css-flexbox/flex-container-max-content-001-ref.html",
        "css/css-flexbox/flex-container-min-content-001-ref.html",
    ] {
        let path = wpt_root.join(reference);
        let document = Html::from_file_async(&path)
            .await
            .unwrap()
            .with_base_url(wpt_root)
            .render_async(&RenderOptions::default())
            .await
            .unwrap();
        assert!(
            !document.pages.is_empty() && document.pages.len() < 20,
            "{reference} should render with a finite page count, pages={}",
            document.pages.len()
        );
    }
}

#[tokio::test]
async fn flex_order_sorts_items_and_preserves_source_order_ties() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; width:90pt }\
         .item { width:30pt; height:10pt }\
         .a { background:red; order:2 }\
         .b { background:green; order:-1 }\
         .c { background:blue; order:2 }\
         </style><div class=\"row\"><div class=\"item a\"></div><div class=\"item b\"></div><div class=\"item c\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!((green.x() - 10.0).abs() < 0.01, "green={green:?}");
    assert!((red.x() - 40.0).abs() < 0.01, "red={red:?}");
    assert!((blue.x() - 70.0).abs() < 0.01, "blue={blue:?}");
}

#[tokio::test]
async fn flex_auto_minimum_is_capped_by_definite_width() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 100pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; flex-flow:wrap; width:300pt }\
         .item { width:75pt; height:10pt; background:green }\
         .wide { width:80pt; height:1pt }\
         </style>\
         <div class=\"row\">\
           <div class=\"item\"><div class=\"wide\"></div></div>\
           <div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div>\
         </div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let item_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 128, 0)) && (rect.height() - 10.0).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(item_rects.len(), 5);
    let first_line_count = item_rects
        .iter()
        .filter(|rect| (rect.y() - 80.0).abs() < 0.01)
        .count();
    assert_eq!(
        first_line_count, 4,
        "definite width should cap flex auto minimums so four 75pt items fit in 300pt: {item_rects:?}"
    );
    assert!(
        item_rects
            .iter()
            .all(|rect| (rect.width() - 75.0).abs() < 0.01),
        "flex item backgrounds should use the definite item width: {item_rects:?}"
    );
}

#[tokio::test]
async fn flex_baseline_alignment_aligns_item_text_baselines() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; align-items:baseline; width:160pt }\
         .big { font-size:30pt; line-height:30pt }\
         .small { font-size:10pt; line-height:10pt; align-self:first baseline }\
         p { margin:0 }\
         </style><div class=\"row\"><p class=\"big\">Big</p><p class=\"small\">Small</p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let big = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Big")
        .unwrap();
    let small = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Small")
        .unwrap();
    assert!(
        (big.y() - small.y()).abs() < 0.01,
        "expected flex item text baselines to align: big={}, small={}",
        big.y(),
        small.y()
    );
}

#[tokio::test]
async fn flex_baseline_alignment_reserves_largest_top_margin() {
    let document = Html::from_string(
        "<style>@page { size: 420pt 160pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; align-items:baseline; width:300pt; height:60pt; background:blue }\
         .row span { display:inline-block; flex:none; width:80pt; margin:0 10pt; height:20pt }\
         .row span:nth-child(1) { background:yellow }\
         .row span:nth-child(2) { background:pink; margin-top:10pt; height:30pt }\
         .row span:nth-child(3) { background:lightblue; height:40pt }</style>\
         <div class=\"row\"><span>one</span><span>two</span><span>three</span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let yellow = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 255, 0)))
        .unwrap();
    let pink = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 192, 203)))
        .unwrap();
    let lightblue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(173, 216, 230)))
        .unwrap();

    let yellow_top = yellow.y() + yellow.height();
    let pink_top = pink.y() + pink.height();
    let lightblue_top = lightblue.y() + lightblue.height();
    assert!(
        (yellow_top - pink_top).abs() < 0.01 && (yellow_top - lightblue_top).abs() < 0.01,
        "baseline-aligned flex item border boxes should share the top offset reserved by the largest top margin: yellow={yellow:?}, pink={pink:?}, lightblue={lightblue:?}"
    );
}

#[tokio::test]
async fn flex_last_baseline_alignment_uses_last_text_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; align-items:last baseline; width:160pt }\
         .multi, .peer { font-size:10pt; line-height:12pt; margin:0 }\
         .multi { white-space:pre-line }\
         </style><div class=\"row\"><p class=\"multi\">One\nTwo</p><p class=\"peer\">Peer</p></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let two = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Two")
        .unwrap();
    let peer = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();
    assert!(
        (two.y() - peer.y()).abs() < 0.01,
        "expected flex item last text baselines to align: two={}, peer={}",
        two.y(),
        peer.y()
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_column_flex_item_falls_back_to_inline_start() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flexbox { display:flex; width:100pt; height:100pt; align-items:baseline;\
           flex-direction:column; writing-mode:vertical-lr; direction:rtl;\
           flex-wrap:wrap-reverse; position:relative }\
         .item { width:100pt; height:50pt; background:green }\
         .abspos { position:absolute; bottom:50pt }</style>\
         <div class=\"flexbox\"><div class=\"abspos item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut green_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .cloned()
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| a.y().partial_cmp(&b.y()).unwrap());

    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    let lower = &green_rects[0];
    let upper = &green_rects[1];
    assert!(
        (lower.x() - upper.x()).abs() < 0.01
            && (lower.width() - 100.0).abs() < 0.01
            && (upper.width() - 100.0).abs() < 0.01
            && (lower.height() - 50.0).abs() < 0.01
            && (upper.height() - 50.0).abs() < 0.01
            && (lower.y() + lower.height() - upper.y()).abs() < 0.01,
        "baseline fallback should stack the green halves into one square: {green_rects:?}"
    );
}

#[tokio::test]
async fn column_flex_baseline_items_fall_back_to_inline_start() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 140pt; margin: 10pt } body { margin: 0 }\
         .flex { display:flex; flex-direction:column; align-items:baseline; width:100pt; height:100pt; background:red }\
         .item { width:40pt; height:20pt; background:green }\
         .wide { width:70pt; background:blue }</style>\
         <div class=\"flex\"><div class=\"item\"></div><div class=\"item wide\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("flex background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("first baseline item should paint");
    let blue = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("second baseline item should paint");

    assert!(
        (green.x() - red.x()).abs() < 0.01 && (blue.x() - red.x()).abs() < 0.01,
        "column flex first-baseline self-alignment should fall back to inline-start for every participant: red={red:?}, green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn column_flex_last_baseline_items_fall_back_to_inline_end() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 140pt; margin: 10pt } body { margin: 0 }\
         .flex { display:flex; flex-direction:column; align-items:last baseline; width:100pt; height:100pt; background:red }\
         .item { width:40pt; height:20pt; background:green }\
         .wide { width:70pt; background:blue }</style>\
         <div class=\"flex\"><div class=\"item\"></div><div class=\"item wide\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("flex background should paint");
    let green = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("first last-baseline item should paint");
    let blue = page
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("second last-baseline item should paint");

    assert!(
        (green.x() - red.x() - 60.0).abs() < 0.01 && (blue.x() - red.x() - 30.0).abs() < 0.01,
        "column flex last-baseline self-alignment should fall back to inline-end for every participant: red={red:?}, green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn align_content_last_baseline_single_line_falls_back_to_logical_end() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; flex-wrap:wrap; align-content:last baseline; width:100pt; height:100pt }\
         .item { width:100pt; height:50pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 0.0).abs() < 0.01,
        "last-baseline content fallback should pack the sole line at logical end: {green:?}"
    );
}

#[tokio::test]
async fn align_content_baseline_wrap_reverse_single_line_falls_back_to_logical_start() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; flex-wrap:wrap-reverse; align-content:baseline; width:100pt; height:100pt }\
         .item { width:100pt; height:50pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 50.0).abs() < 0.01,
        "first-baseline content fallback should use logical start, not wrap-reverse flex-start: {green:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_flex_item_falls_back_to_block_start() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:baseline; writing-mode:vertical-rl;\
           direction:ltr; flex-direction:row; width:100pt; height:100pt }\
         .item { width:50pt; height:100pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.x() - 50.0).abs() < 0.01,
        "vertical row first-baseline fallback should align to block-start/right: {green:?}"
    );
}

#[tokio::test]
async fn vertical_lr_row_flex_synthesizes_missing_baseline_from_line_under() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 120px 140px; margin: 0 }
body { margin: 0 }
p { display: none }
</style>
<p>Test passes if there is a filled green square and no red.</p>
<div style="display: flex; align-items: baseline; writing-mode: vertical-lr; text-orientation: sideways; background: red;">
  <div style="height: 50px; width: 100px; background: green;"></div>
  <div style="height: 50px; width: 100px; background: green; line-height: 0;">
    <span style="width: 10px; height: 10px; display: inline-block;"></span>
  </div>
</div>"#,
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let mut green_rects = page
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .cloned()
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| a.y().partial_cmp(&b.y()).unwrap());

    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    let lower = &green_rects[0];
    let upper = &green_rects[1];
    assert!(
        (lower.x() - upper.x()).abs() < 0.01
            && (lower.width() - upper.width()).abs() < 0.01
            && (lower.height() - upper.height()).abs() < 0.01
            && (lower.width() - lower.height() * 2.0).abs() < 0.01
            && (lower.y() + lower.height() - upper.y()).abs() < 0.01,
        "baseline-synthesized vertical-lr flex items should cover one green square: {green_rects:?}"
    );

    for (x, y) in [
        (
            lower.x() + lower.width() * 0.25,
            lower.y() + lower.height() * 0.5,
        ),
        (
            lower.x() + lower.width() * 0.75,
            lower.y() + lower.height() * 0.5,
        ),
        (
            upper.x() + upper.width() * 0.25,
            upper.y() + upper.height() * 0.5,
        ),
        (
            upper.x() + upper.width() * 0.75,
            upper.y() + upper.height() * 0.5,
        ),
    ] {
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(Color::new(0, 128, 0)),
            "green flex items should cover red background at ({x}, {y}): {green_rects:?}"
        );
    }
}

#[tokio::test]
async fn last_baseline_single_item_falls_back_to_self_end() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:last baseline; width:100pt; height:100pt }\
         .item { width:100pt; height:50pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 0.0).abs() < 0.01,
        "last-baseline self-alignment fallback should align to self-end/bottom: {green:?}"
    );
}

#[tokio::test]
async fn explicit_baseline_align_self_uses_same_fallback_sides() {
    let first = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:flex-start; writing-mode:vertical-rl;\
           direction:ltr; flex-direction:row; width:100pt; height:100pt }\
         .item { align-self:first baseline; width:50pt; height:100pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let last = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:flex-start; width:100pt; height:100pt }\
         .item { align-self:last baseline; width:100pt; height:50pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let first_green = first.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let last_green = last.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        (first_green.x() - 50.0).abs() < 0.01 && (last_green.y() - 0.0).abs() < 0.01,
        "explicit baseline align-self should use the same fallback sides: first={first_green:?}, last={last_green:?}"
    );
}

#[tokio::test]
async fn baseline_fallback_does_not_override_auto_cross_margin() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:baseline; width:100pt; height:100pt }\
         .item { width:100pt; height:50pt; margin-top:auto; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 0.0).abs() < 0.01,
        "baseline fallback must not override cross-axis auto margin placement: {green:?}"
    );
}

#[tokio::test]
async fn zero_percent_flex_basis_overrides_authored_main_size_for_empty_item() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .container { background: green; display: flex; height: 75pt; width: 75pt }\
         .item { background: red; flex-basis: 0%; height: 75pt; width: 75pt }\
         </style><div class=\"container\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("container background should paint");

    assert!((green.width() - 75.0).abs() < 0.01);
    assert!((green.height() - 75.0).abs() < 0.01);
    assert!(
        !document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(255, 0, 0)) && rect.width() > 0.01)
    );
}

#[tokio::test]
async fn flex_basis_content_ignores_authored_main_size_for_base_size() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 140pt; margin: 10pt } body { margin: 0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:120pt; margin:0 0 10pt }\
         .content { flex:0 0 content; width:80pt; height:10pt; background:red }\
         .auto { flex:0 0 auto; width:80pt; height:10pt; background:blue }\
         </style><div class=\"row\"><div class=\"content\">Hi</div></div><div class=\"row\"><div class=\"auto\">Hi</div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let content = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let auto = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!(
        content.width() < 30.0,
        "content flex-basis should use intrinsic text width: {content:?}"
    );
    assert!((auto.width() - 80.0).abs() < 0.01, "auto={auto:?}");
}

#[tokio::test]
async fn flex_shorthand_accepts_unitless_zero_basis() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:200pt }\
         .item { flex:4 1 0; width:25pt; height:20pt }\
         </style><div class=\"row\"><div class=\"item\" style=\"background:yellow\"></div><div class=\"item\" style=\"background:pink\"></div><div class=\"item\" style=\"background:lightblue\"></div><div class=\"item\" style=\"background:gray\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let item_widths = document.pages[0]
        .rects
        .iter()
        .filter(|rect| (rect.height() - 20.0).abs() < 0.01)
        .map(|rect| rect.width())
        .collect::<Vec<_>>();

    assert_eq!(item_widths.len(), 4);
    assert!(
        item_widths.iter().all(|width| (*width - 50.0).abs() < 0.01),
        "flex:4 1 0 should use zero flex-basis and distribute the 200pt row equally: {item_widths:?}"
    );
}

#[tokio::test]
async fn column_flex_item_max_height_min_content_clamps_flex_basis() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 220pt; margin: 10pt } body { margin:0 }\
         .container { display:flex; flex-direction:column; width:75pt; height:150pt }\
         .item { max-height:min-content; flex-basis:150pt; background:green }\
         .child { height:75pt }\
         </style><div class=\"container\"><div class=\"item\"><div class=\"child\"></div></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| (rect.width() - 75.0).abs() < 0.01 && (rect.height() - 75.0).abs() < 0.01)
        .is_some();

    assert!(
        green,
        "max-height:min-content should clamp the column flex item to its 75pt child block-size: {:?}",
        document.pages[0].rects
    );
}

#[tokio::test]
async fn column_flex_replaced_item_auto_min_height_uses_transferred_size() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 180pt; margin: 10pt }} body {{ margin:0 }}\
         .before {{ width:75pt; height:37.5pt; background:green }}\
         .flex {{ display:flex; flex-direction:column; width:75pt; height:0 }}\
         img {{ width:75pt }}\
         </style><div class=\"before\"></div><div class=\"flex\"><img src=\"{image}\"></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].images.len(), 1);
    let image = &document.pages[0].images[0];
    assert!((image.width() - 75.0).abs() < 0.01, "image={image:?}");
    assert!(
        (image.height() - 75.0).abs() < 0.01,
        "the image's transferred automatic minimum height should overflow the zero-height column flex container: {image:?}"
    );
}

#[tokio::test]
async fn collapsed_flex_item_before_replaced_item_keeps_source_indexed_auto_minimum() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 180pt; margin: 10pt }} body {{ margin:0 }}\
         .flex {{ display:flex; flex-direction:column; width:75pt; height:0 }}\
         .collapsed {{ visibility:collapse; width:75pt; height:20pt; background:red }}\
         img {{ width:75pt }}\
         </style><div class=\"flex\"><div class=\"collapsed\"></div><img src=\"{image}\"></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        !document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(255, 0, 0))),
        "collapsed flex item must not paint"
    );
    assert_eq!(document.pages[0].images.len(), 1);
    let image = &document.pages[0].images[0];
    assert!(
        (image.height() - 75.0).abs() < 0.01,
        "source-indexed estimates should preserve the image auto minimum after a collapsed sibling: {image:?}"
    );
}

#[tokio::test]
async fn flex_basis_intrinsic_keywords_use_min_and_max_content_sizes() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 160pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:220pt; margin:0 0 10pt }\
         .min { flex:0 0 min-content; width:120pt; height:10pt; background:red }\
         .max { flex:0 0 max-content; width:120pt; height:10pt; background:blue }\
         </style><div class=\"row\"><div class=\"min\">WWWW WWWW</div></div><div class=\"row\"><div class=\"max\">WWWW WWWW</div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let min = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let max = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!(
        min.width() < max.width(),
        "min-content should be narrower than max-content: min={min:?}, max={max:?}"
    );
    assert!(
        max.width() < 120.0,
        "max-content flex-basis should ignore authored width: {max:?}"
    );
}

#[tokio::test]
async fn flex_basis_fit_content_clamps_between_min_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 180pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:220pt; margin:0 0 10pt }\
         .min { flex:0 0 min-content; height:10pt; background:red }\
         .fit { flex:0 0 fit-content(30pt); height:10pt; background:green }\
         .max { flex:0 0 max-content; height:10pt; background:blue }\
         </style>\
         <div class=\"row\"><div class=\"min\">Hi there friend</div></div>\
         <div class=\"row\"><div class=\"fit\">Hi there friend</div></div>\
         <div class=\"row\"><div class=\"max\">Hi there friend</div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let min = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let fit = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let max = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!(
        min.width() < fit.width() && fit.width() < max.width(),
        "fit-content should clamp between min/max content: min={min:?}, fit={fit:?}, max={max:?}"
    );
    assert!(
        (fit.width() - 30.0).abs() < 0.01,
        "fit-content(30pt) should use the argument when it is between intrinsic bounds: {fit:?}"
    );
}

#[tokio::test]
async fn flex_item_mixed_percentage_max_width_resolves_against_container() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; width:200pt }\
         .item { flex:0 0 auto; width:180pt; max-width:calc(50% + 10pt); height:10pt; background:green }\
         </style><div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let item = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!((item.width() - 110.0).abs() < 0.1, "item={item:?}");
}

#[tokio::test]
async fn flex_item_mixed_percentage_min_width_resolves_against_container() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; width:200pt }\
         .item { flex:0 0 auto; width:20pt; min-width:calc(50% + 10pt); height:10pt; background:green }\
         </style><div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let item = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!((item.width() - 110.0).abs() < 0.1, "item={item:?}");
}

#[tokio::test]
async fn flex_basis_mixed_percentage_resolves_against_definite_main_size() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; width:200pt }\
         .item { flex:0 0 calc(50% + 10pt); height:10pt; background:green }\
         </style><div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!((item.width() - 110.0).abs() < 0.1, "item={item:?}");
}

#[tokio::test]
async fn column_flex_basis_mixed_percentage_resolves_against_definite_main_size() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 160pt; margin: 10pt } body { margin: 0 }\
         .col { display:flex; flex-direction:column; width:80pt; height:100pt }\
         .item { flex:0 0 calc(50% + 10pt); width:20pt; background:green }\
         </style><div class=\"col\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!((item.height() - 60.0).abs() < 0.1, "item={item:?}");
}

#[tokio::test]
async fn column_flex_item_definite_flex_basis_resolves_child_percentage_height() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 180pt; margin: 10pt } body { margin:0 }\
         .col { display:flex; flex-direction:column }\
         .item { height:0; flex:0 0 100pt }\
         .item > div { width:100pt; height:100%; background:green }</style>\
         <div class=\"col\"><div class=\"item\"><div></div></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.width() - 100.0).abs() < 0.01 && (green.height() - 100.0).abs() < 0.01,
        "percentage-height child should resolve against the flex item's definite flex-basis height: {green:?}"
    );
}

#[tokio::test]
async fn column_flex_item_mixed_percentage_min_max_height_resolves_against_container() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 260pt; margin: 10pt } body { margin: 0 }\
         .col { display:flex; flex-direction:column; width:80pt; height:100pt; margin-bottom:10pt }\
         .min { flex:0 0 auto; width:20pt; height:10pt; min-height:calc(50% + 10pt); background:green }\
         .max { flex:0 0 auto; width:20pt; height:90pt; max-height:calc(50% + 10pt); background:blue }\
         </style><div class=\"col\"><div class=\"min\"></div></div><div class=\"col\"><div class=\"max\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let min = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let max = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!((min.height() - 60.0).abs() < 0.1, "min={min:?}");
    assert!((max.height() - 60.0).abs() < 0.1, "max={max:?}");
}

#[tokio::test]
async fn flex_auto_minimum_size_is_zero_for_scrollable_overflow() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 160pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:100pt; margin:0 0 10pt }\
         .item { flex:1 1 0; background:red; white-space:nowrap }\
         .fixed { flex:0 0 50pt; height:10pt; background:blue }\
         </style>\
         <div class=\"row\"><div class=\"item\" style=\"overflow:hidden\">WWWWWWWWWWWWWWWWWWWW</div><div class=\"fixed\"></div></div>\
         <div class=\"row\"><div class=\"item\" style=\"overflow:clip\">WWWWWWWWWWWWWWWWWWWW</div><div class=\"fixed\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 2);
    assert!(
        (red_rects[0].width() - 50.0).abs() < 0.01,
        "scrollable overflow should allow auto min-size to shrink to zero: {:?}",
        red_rects[0]
    );
    assert!(
        red_rects[1].width() > 100.0,
        "non-scrollable overflow:clip should keep content-based auto min-size: {:?}",
        red_rects[1]
    );
}

#[tokio::test]
async fn row_flex_auto_minimum_size_uses_overflow_x() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 160pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:100pt; margin:0 0 10pt }\
         .item { flex:1 1 0; background:red; white-space:nowrap }\
         .fixed { flex:0 0 50pt; height:10pt; background:blue }\
         </style>\
         <div class=\"row\"><div class=\"item\" style=\"overflow-x:hidden; overflow-y:clip\">WWWWWWWWWWWWWWWWWWWW</div><div class=\"fixed\"></div></div>\
         <div class=\"row\"><div class=\"item\" style=\"overflow-x:clip; overflow-y:hidden\">WWWWWWWWWWWWWWWWWWWW</div><div class=\"fixed\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 2);
    assert!(
        (red_rects[0].width() - 50.0).abs() < 0.01,
        "row flex should use scrollable overflow-x for main-axis auto min-size: {:?}",
        red_rects[0]
    );
    assert!(
        red_rects[1].width() > 100.0,
        "row flex should ignore scrollable overflow-y for main-axis auto min-size: {:?}",
        red_rects[1]
    );
}

#[tokio::test]
async fn row_flex_min_width_auto_uses_zero_for_non_visible_overflow_x() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 260pt; margin: 10pt } body { margin:0 }\
         .flexbox { display:flex; width:30pt; margin-bottom:2pt }\
         .item { border:2pt dotted purple; background:red }\
         .item > div { width:80pt; height:40pt }\
         </style>\
         <div class=\"flexbox\"><div class=\"item\" style=\"overflow-x:visible\"><div></div></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"overflow-x:hidden\"><div></div></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"overflow-x:scroll\"><div></div></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"overflow-x:auto\"><div></div></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 4);
    assert!(
        (red_rects[0].width() - 84.0).abs() < 0.01,
        "visible overflow should keep the row flex item's content-based auto min-width: {:?}",
        red_rects[0]
    );
    for rect in &red_rects[1..] {
        assert!(
            (rect.width() - 30.0).abs() < 0.01,
            "non-visible overflow-x should resolve min-width:auto to zero and allow shrinkage: {rect:?}"
        );
    }
}

#[tokio::test]
async fn column_flex_auto_minimum_size_uses_overflow_y() {
    let lines = "A\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA";
    let html = format!(
        "<style>@page {{ size: 260pt 320pt; margin: 10pt }} body {{ margin:0; font-size:10pt; line-height:10pt }}\
         .col {{ display:flex; flex-direction:column; height:100pt; width:40pt; margin:0 0 10pt }}\
         .item {{ flex:1 1 0; background:red; white-space:pre-line }}\
         .fixed {{ flex:0 0 50pt; width:40pt; background:blue }}\
         </style>\
         <div class=\"col\"><div class=\"item\" style=\"overflow-y:hidden; overflow-x:clip\">{lines}</div><div class=\"fixed\"></div></div>\
         <div class=\"col\"><div class=\"item\" style=\"overflow-y:clip; overflow-x:hidden\">{lines}</div><div class=\"fixed\"></div></div>"
    );
    let document = Html::from_string(&html)
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let red_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 2);
    assert!(
        (red_rects[0].height() - 50.0).abs() < 0.01,
        "column flex should use scrollable overflow-y for main-axis auto min-size: {:?}",
        red_rects[0]
    );
    assert!(
        red_rects[1].height() > 100.0,
        "column flex should ignore scrollable overflow-x for main-axis auto min-size: {:?}",
        red_rects[1]
    );
}

#[tokio::test]
async fn flex_main_axis_auto_margin_absorbs_free_space() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 } .row { display:flex; width:200pt } .a { width:40pt; height:10pt; background:red } .b { margin-left:auto; width:30pt; height:10pt; background:blue }</style><div class=\"row\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - 180.0).abs() < 0.01);
}

#[tokio::test]
async fn supports_flex_wrap_and_flex_basis() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 140pt; margin: 10pt } body { margin: 0 } .wrap { display:flex; flex-wrap:wrap; align-content:space-between; width:100pt; height:100pt } .wrap div { flex: 1 50%; height:10pt }</style><div class=\"wrap\"><div style=\"background:red\"></div><div style=\"background:blue\"></div><div style=\"background:green\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();
    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert_eq!(red.width(), 50.0);
    assert_eq!(blue.width(), 50.0);
    assert!((red.y() - blue.y()).abs() < 0.01);
    assert!(green.y() < red.y() - 50.0);
}

#[tokio::test]
async fn row_flex_item_page_break_before_does_not_create_standalone_pages() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 180pt; margin: 10pt } body { margin: 0 } .flexbox { display: flex; flex-wrap: wrap; float: left; width: 60pt; height: 20pt; border: 1pt dashed black; margin: 0 2pt 4pt 0 } .item { width: 28pt; border: 1pt solid blue; background: lightblue } .clear { clear: both }</style>\
         <div class=\"flexbox\"><div class=\"item\" style=\"page-break-before: always\"></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"page-break-before: left\"></div></div>\
         <div class=\"clear\"></div>\
         <div class=\"flexbox\"><div class=\"item\"></div><div class=\"item\" style=\"page-break-before: right\"></div></div>\
         <div class=\"flexbox\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\" style=\"page-break-before: always\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let item_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(173, 216, 230)))
        .collect::<Vec<_>>();
    assert_eq!(item_rects.len(), 7);
    assert!((item_rects[0].height() - 20.0).abs() < 0.01);
    assert!((item_rects[1].x() - item_rects[0].x() - 64.0).abs() < 0.01);
    assert!((item_rects[1].y() - item_rects[0].y()).abs() < 0.01);
    assert!((item_rects[3].x() - item_rects[2].x() - 30.0).abs() < 0.01);
    assert!((item_rects[3].y() - item_rects[2].y()).abs() < 0.01);
    assert!((item_rects[2].height() - 20.0).abs() < 0.01);
    assert!((item_rects[3].height() - 20.0).abs() < 0.01);
    assert!((item_rects[5].x() - item_rects[4].x() - 30.0).abs() < 0.01);
    assert!((item_rects[5].y() - item_rects[4].y()).abs() < 0.01);
    assert!((item_rects[6].x() - item_rects[4].x()).abs() < 0.01);
    assert!(
        (item_rects[4].y() - item_rects[6].y() - 10.0).abs() < 0.01,
        "item rects: {item_rects:?}"
    );
    for rect in &item_rects[4..] {
        assert!((rect.height() - 10.0).abs() < 0.01);
    }
}

#[tokio::test]
async fn column_flex_item_page_break_before_does_not_create_standalone_pages() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 180pt; margin: 10pt } body { margin: 0 }\
         .flexbox { display: flex; flex-direction: column; float: left; width: 20pt; height: 60pt; border: 1pt dashed black; margin: 0 2pt 4pt 0 }\
         .item { height: 28pt; border: 1pt solid blue; background: lightblue } .clear { clear: both }</style>\
         <div class=\"flexbox\"><div class=\"item\" style=\"page-break-before: always\"></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"page-break-before: left\"></div></div>\
         <div class=\"clear\"></div>\
         <div class=\"flexbox\"><div class=\"item\"></div><div class=\"item\" style=\"page-break-before: right\"></div></div>\
         <div class=\"flexbox\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\" style=\"page-break-before: always\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let item_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(173, 216, 230)))
        .collect::<Vec<_>>();
    assert_eq!(item_rects.len(), 7);
    assert!(
        (item_rects[0].height() - 30.0).abs() < 0.01,
        "item rects: {item_rects:?}"
    );
    assert!((item_rects[1].x() - item_rects[0].x() - 24.0).abs() < 0.01);
    assert!((item_rects[1].y() - item_rects[0].y()).abs() < 0.01);
    assert!((item_rects[3].x() - item_rects[2].x()).abs() < 0.01);
    assert!((item_rects[2].y() - item_rects[3].y() - 30.0).abs() < 0.01);
    assert!((item_rects[5].x() - item_rects[4].x()).abs() < 0.01);
    assert!((item_rects[4].y() - item_rects[5].y() - 20.0).abs() < 0.01);
    assert!((item_rects[5].y() - item_rects[6].y() - 20.0).abs() < 0.01);
    for rect in &item_rects[4..] {
        assert!((rect.height() - 20.0).abs() < 0.01);
    }
}

#[tokio::test]
async fn oversized_flex_container_at_page_top_does_not_create_leading_blank_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body { margin: 0 }\
         .flexbox { display: flex; width: 40pt; height: 140pt; background: green }\
         .item { width: 20pt; height: 20pt }</style>\
         <div class=\"flexbox\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 128, 0))
                && (rect.height() - 80.0).abs() < 0.01),
        "oversized flex container should start on the first page without a leading blank page"
    );
    assert!(
        document.pages[1]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 128, 0))
                && (rect.height() - 60.0).abs() < 0.01),
        "oversized flex container should continue on the next page"
    );
}

#[tokio::test]
async fn column_wrapped_flex_container_honors_min_height_without_wrapping() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 500pt; margin: 10pt } body { margin: 0 }\
         .flexbox { display: flex; flex-direction: column; flex-wrap: wrap; border: 1px dashed black; width: 12px; min-height: 100px; margin-right: 2px; float: left }\
         .smallItem { height: 30px; border: 1px solid blue; background: lightblue }\
         </style>\
         <div class=\"flexbox\"></div>\
         <div class=\"flexbox\"><div class=\"smallItem\"></div></div>\
         <div class=\"flexbox\"><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div></div>\
         <div class=\"flexbox\" style=\"max-height: 120px\"><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let mut item_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(173, 216, 230)))
        .map(|rect| (rect.x(), rect.y(), rect.width(), rect.height()))
        .collect::<Vec<_>>();
    item_rects.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap()
            .then_with(|| b.1.partial_cmp(&a.1).unwrap())
    });

    let expected = [
        (22.75, 465.25, 9.0, 24.0),
        (34.75, 465.25, 9.0, 24.0),
        (34.75, 441.25, 9.0, 24.0),
        (34.75, 417.25, 9.0, 24.0),
        (34.75, 393.25, 9.0, 24.0),
        (34.75, 369.25, 9.0, 24.0),
        (46.75, 465.25, 4.5, 24.0),
        (46.75, 441.25, 4.5, 24.0),
        (46.75, 417.25, 4.5, 24.0),
        (51.25, 465.25, 4.5, 24.0),
        (51.25, 441.25, 4.5, 24.0),
    ];

    assert_eq!(item_rects.len(), expected.len());
    for (actual, expected) in item_rects.iter().zip(expected) {
        assert!(
            (actual.0 - expected.0).abs() < 0.01
                && (actual.1 - expected.1).abs() < 0.01
                && (actual.2 - expected.2).abs() < 0.01
                && (actual.3 - expected.3).abs() < 0.01,
            "expected item rect {expected:?}, got {actual:?}"
        );
    }
}

#[tokio::test]
async fn column_flex_auto_height_treats_zero_percent_flex_basis_as_content() {
    let target = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"display: flex; flex-direction: column; border: 1px solid purple\">\
         <div>Header</div><div style=\"flex: 1\">Flexible content<br></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"border: 1px solid purple\">\
         <div>Header</div><div>Flexible content<br></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let target_text = target.pages[0]
        .lines
        .iter()
        .map(|line| (line.text.as_str(), line.x(), line.y()))
        .collect::<Vec<_>>();
    let reference_text = reference.pages[0]
        .lines
        .iter()
        .map(|line| (line.text.as_str(), line.x(), line.y()))
        .collect::<Vec<_>>();

    assert_eq!(target_text, reference_text);

    let target_border = target.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(128, 0, 128)))
        .map(|rect| (rect.x(), rect.y(), rect.width(), rect.height()))
        .collect::<Vec<_>>();
    let reference_border = reference.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(128, 0, 128)))
        .map(|rect| (rect.x(), rect.y(), rect.width(), rect.height()))
        .collect::<Vec<_>>();

    assert_eq!(target_border, reference_border);
}

#[tokio::test]
async fn flex_container_creates_anonymous_items_for_nbsp_text_runs() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; justify-content: flex-end; width: 300pt; height: 40pt; font-size: 12pt; line-height: 14.4pt }\
         .item { width: 50pt; height: 30pt } .a { background: red } .b { background: green } .c { background: blue }</style>\
         <div class=\"row\"><div class=\"item a\"></div>&nbsp;<div class=\"item b\"></div>&nbsp;<div class=\"item c\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let red = rect(Color::new(255, 0, 0));
    let green = rect(Color::new(0, 128, 0));
    let blue = rect(Color::new(0, 0, 255));

    assert!((blue.x() - 260.0).abs() < 0.01);
    assert!(red.x() < 160.0, "anonymous NBSP items must consume width");
    assert!(
        green.x() - red.x() - 50.0 > 2.5,
        "gap between flex items should include the NBSP anonymous flex item"
    );
}

#[tokio::test]
async fn inline_block_fragment_lays_out_atomic_inline_children_in_one_line() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 120pt; margin: 10pt } body { margin: 0 }\
         .outer { display: inline-block; text-align: right; width: 300pt; height: 40pt; font-size: 12pt; line-height: 14.4pt }\
         .item { display: inline-block; width: 50pt; height: 30pt } .a { background: red } .b { background: green } .c { background: blue }</style>\
         <div class=\"outer\"><div class=\"item a\"></div> <div class=\"item b\"></div> <div class=\"item c\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let red = rect(Color::new(255, 0, 0));
    let green = rect(Color::new(0, 128, 0));
    let blue = rect(Color::new(0, 0, 255));

    assert!((blue.x() - 260.0).abs() < 0.01);
    assert!(
        red.x() < 160.0,
        "inline spaces should affect right alignment"
    );
    assert!(
        green.x() - red.x() - 50.0 > 2.5,
        "inline-block children should share a line with whitespace gaps"
    );
}

#[tokio::test]
async fn inline_flex_exports_first_item_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 10pt } body { margin: 0 }\
         .flexContainer { display: inline-flex; background: lightblue }\
         .smallFont { font-size: 10px; line-height: 10px }\
         .bigFont { font-size: 20px; line-height: 20px }</style>\
         a <div class=\"flexContainer\"><div class=\"smallFont\">b</div><div class=\"bigFont\">c</div></div>\
         <div class=\"flexContainer\"><div class=\"bigFont\">d</div><div class=\"smallFont\">e</div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render"))
    };

    let a = line("a");
    let b = line("b");
    let d = line("d");
    assert!(
        (b.y() - a.y()).abs() < 0.01,
        "expected b baseline {} to match a baseline {}",
        b.y(),
        a.y()
    );
    assert!(
        (d.y() - a.y()).abs() < 0.01,
        "expected d baseline {} to match a baseline {}",
        d.y(),
        a.y()
    );
}

#[tokio::test]
async fn inline_flex_baseline_uses_first_order_modified_item() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 10pt } body { margin: 0 }\
         .flexContainer { display: inline-flex; background: lightblue }\
         .smallFont { font-size: 10px; line-height: 10px }\
         .bigFont { font-size: 20px; line-height: 20px }\
         .smallOrder { order: -1 } .bigOrder { order: 30 }</style>\
         a <div class=\"flexContainer\"><div class=\"bigFont\">c</div><div class=\"smallFont smallOrder\">b</div></div>\
         <div class=\"flexContainer\"><div class=\"smallFont bigOrder\">e</div><div class=\"bigFont\">d</div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render"))
    };

    let a = line("a");
    let b = line("b");
    let d = line("d");
    assert!(
        (b.y() - a.y()).abs() < 0.01,
        "expected ordered b baseline {} to match a baseline {}",
        b.y(),
        a.y()
    );
    assert!(
        (d.y() - a.y()).abs() < 0.01,
        "expected ordered d baseline {} to match a baseline {}",
        d.y(),
        a.y()
    );
}

#[tokio::test]
async fn flex_flow_wrap_align_content_stretch_stretches_lines() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 140pt; margin: 10pt } body { margin: 0 } #flexbox { background: red; align-content: center; align-content: stretch; display: flex; flex-flow: wrap; height: 75pt; width: 225pt } #flexbox div { background-color: green; width: 112.5pt }</style><div id=\"flexbox\"><div></div><div></div><div></div><div></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();
    let green_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .collect::<Vec<_>>();

    assert_eq!(green_rects.len(), 4);
    for rect in &green_rects {
        assert!((rect.width() - 112.5).abs() < 0.01);
        assert!((rect.height() - 37.5).abs() < 0.01);
    }
    let top = green_rects
        .iter()
        .map(|rect| rect.y() + rect.height())
        .fold(f32::MIN, f32::max);
    let bottom = green_rects
        .iter()
        .map(|rect| rect.y())
        .fold(f32::MAX, f32::min);

    assert!((red.height() - 75.0).abs() < 0.01);
    assert!((top - (red.y() + red.height())).abs() < 0.01);
    assert!((bottom - red.y()).abs() < 0.01);
}

#[tokio::test]
async fn column_reverse_wrap_reverse_places_lines_in_reversed_cross_axis_order() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 160pt; margin: 10pt } body { margin: 0 } .container { display: flex; flex-direction: column-reverse; flex-wrap: wrap-reverse; height: 90pt; width: 150pt } .container > div { width: 40pt } .a, .b, .c { height: 25pt } .d, .e { height: 40pt } .f { height: 85pt } .a { background: red } .b { background: green } .c { background: blue } .d { background: yellow } .e { background: magenta } .f { background: cyan }</style><div class=\"container\"><div class=\"f\"></div><div class=\"e\"></div><div class=\"d\"></div><div class=\"c\"></div><div class=\"b\"></div><div class=\"a\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let a = rect(Color::new(255, 0, 0));
    let b = rect(Color::new(0, 128, 0));
    let c = rect(Color::new(0, 0, 255));
    let d = rect(Color::new(255, 255, 0));
    let e = rect(Color::new(255, 0, 255));
    let f = rect(Color::new(0, 255, 255));

    assert!((a.x() - b.x()).abs() < 0.01);
    assert!((b.x() - c.x()).abs() < 0.01);
    assert!((d.x() - e.x()).abs() < 0.01);
    assert!(a.x() < d.x());
    assert!(d.x() < f.x());
    assert!(a.y() > b.y());
    assert!(b.y() > c.y());
    assert!(d.y() > e.y());
}

#[tokio::test]
async fn row_reverse_wrap_places_multiline_items_in_reverse_main_axis_order() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 260pt; margin: 10pt } body { margin: 0 }\
         .container { display: flex; flex-direction: row-reverse; flex-wrap: wrap; width: 225pt }\
         p { margin: 12pt 7.5pt 12pt 0; background: #ccc; font-size: 12pt; line-height: 14.4pt }\
         .w90 { width: 67.5pt } .w140 { width: 105pt } .w290 { width: 217.5pt }\
         </style><div class=\"container\">\
         <p class=\"w90\">1-3</p><p class=\"w90\">1-2</p><p class=\"w90\">1-1</p>\
         <p class=\"w140\">2-2</p><p class=\"w140\">2-1</p><p class=\"w290\">3-1</p>\
         </div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(204, 204, 204)))
        .collect::<Vec<_>>();

    assert_eq!(rects.len(), 6);
    let expected = [
        (160.0, 223.6, 67.5, 14.4),
        (85.0, 223.6, 67.5, 14.4),
        (10.0, 223.6, 67.5, 14.4),
        (122.5, 185.2, 105.0, 14.4),
        (10.0, 185.2, 105.0, 14.4),
        (10.0, 146.8, 217.5, 14.4),
    ];
    for (index, (rect, (x, y, width, height))) in rects.iter().zip(expected).enumerate() {
        assert!((rect.x() - x).abs() < 0.01, "{index}: x {}", rect.x());
        assert!((rect.y() - y).abs() < 0.01, "{index}: y {}", rect.y());
        assert!(
            (rect.width() - width).abs() < 0.01,
            "{index}: width {}",
            rect.width()
        );
        assert!(
            (rect.height() - height).abs() < 0.01,
            "{index}: height {}",
            rect.height()
        );
    }
}

#[tokio::test]
async fn order_with_row_reverse_matches_right_floated_reference() {
    let style = "<style>@page { size: 800pt 300pt; margin: 10pt } body { margin: 0 }</style>";
    let target = Html::from_string(format!(
        "{style}<style>\
         #test {{ display: flex; flex-direction: row-reverse }}\
         #leftmost {{ order: 1 }} #middle {{ order: 0 }} #rightmost {{ order: -1 }}\
         </style>\
         <p>Test passes if the paragraph below reads 'First,Second,Third' from leftmost.</p>\
         <div id=\"test\"><p id=\"leftmost\">First,</p><p id=\"middle\">Second,</p><p id=\"rightmost\">Third</p></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "{style}<style>#leftmost, #middle, #rightmost {{ float: right }}</style>\
         <p>Test passes if the paragraph below reads 'First,Second,Third' from leftmost.</p>\
         <div id=\"test\"><p id=\"rightmost\">Third</p><p id=\"middle\">Second,</p><p id=\"leftmost\">First,</p></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let line_positions = |document: &quire::Document| {
        ["First,", "Second,", "Third"]
            .into_iter()
            .map(|text| {
                let line = document.pages[0]
                    .lines
                    .iter()
                    .find(|line| line.text == text)
                    .unwrap_or_else(|| panic!("{text} should render"));
                (line.text.clone(), line.x(), line.y())
            })
            .collect::<Vec<_>>()
    };

    let target_lines = line_positions(&target);
    let reference_lines = line_positions(&reference);
    let target_row_y = target_lines
        .first()
        .map(|(_, _, y)| *y)
        .expect("target row should contain text");
    for ((target_text, target_x, target_y), (reference_text, reference_x, _reference_y)) in
        target_lines.iter().zip(&reference_lines)
    {
        assert_eq!(target_text, reference_text);
        assert!(
            (target_x - reference_x).abs() < 0.01,
            "{target_text}: target x {target_x}, reference x {reference_x}"
        );
        assert!(
            (target_y - target_row_y).abs() < 0.01,
            "{target_text}: target y {target_y}, row y {target_row_y}"
        );
    }
}

#[tokio::test]
async fn floated_flex_container_min_content_contains_inflexible_auto_basis_item_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0 } .red { position: absolute; background: red; width: 75pt; height: 75pt; z-index: -1 } .outer { width: 0 } .flex { display: flex; float: left; background: green; height: 75pt } .item { flex: 0 0 auto } .inline-block { float: left; width: 75pt }</style><div class=\"red\"></div><div class=\"outer\"><div class=\"flex\"><div class=\"item\"><div class=\"inline-block\"></div><div class=\"inline-block\"></div></div></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("floated flex container background should paint");
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("absolute red reference should paint behind it");

    assert!((green.width() - 75.0).abs() < 0.01, "green={green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "green={green:?}");
    assert!((green.x() - red.x()).abs() < 0.01);
    assert!((green.y() - red.y()).abs() < 0.01);
}

#[tokio::test]
async fn absolute_flex_children_use_flex_static_position_and_ignore_justify_self() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0 } .container { display: flex; flex-flow: row; padding: 1px 2px; border: 1px solid black; background: yellow; margin: 0 0 5px 0; height: 10px; width: 16px } .container > div { position: absolute; background: teal; height: 6px; width: 8px }</style><div class=\"container\"><div style=\"justify-self: auto\"></div></div><div class=\"container\"><div style=\"justify-self: center\"></div></div><div class=\"container\"><div style=\"justify-self: end\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let yellow_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 255, 0)))
        .collect::<Vec<_>>();
    let teal_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 128)))
        .collect::<Vec<_>>();

    assert_eq!(yellow_rects.len(), 3);
    assert_eq!(teal_rects.len(), 3);
    for (container, child) in yellow_rects.iter().zip(teal_rects.iter()) {
        assert!((child.x() - (container.x() + 2.25)).abs() < 0.01);
        assert!(
            (child.y() + child.height() - (container.y() + container.height() - 1.5)).abs() < 0.01
        );
    }
}

#[tokio::test]
async fn flex_root_honors_align_items_and_percent_height() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } html { display: flex; height: 100%; align-items: center; justify-content: center } body { margin: 0; width: 20pt; height: 20pt; font-size: 10pt; line-height: 10pt }</style><body>X</body>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let line = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "X")
        .unwrap();

    assert!((line.x() - 40.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, line, 60.0);
}

#[tokio::test]
async fn column_flex_indefinite_percentage_flex_basis_uses_content() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 200pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .col { display:flex; flex-direction:column; width:60pt; margin:0 0 10pt }\
         .definite { height:100pt }\
         .item { flex:0 0 50%; background:red }\
         </style>\
         <div class=\"col\"><div class=\"item\">A</div></div>\
         <div class=\"col definite\"><div class=\"item\">A</div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 2);
    assert!(
        (red_rects[0].height() - 10.0).abs() < 0.01,
        "indefinite percentage flex-basis should use content height: {:?}",
        red_rects[0]
    );
    assert!(
        (red_rects[1].height() - 50.0).abs() < 0.01,
        "definite percentage flex-basis should resolve against container height: {:?}",
        red_rects[1]
    );
}

#[tokio::test]
async fn flex_start_items_cover_cross_start_gradient_band() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 140pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; align-items: flex-start; width: 225pt; height: 75pt;\
           background: linear-gradient(to bottom, red 0, red 37.5pt, green 37.5pt, green 75pt) }\
         .item { width: 112.5pt; height: 38.25pt; background: green }\
         </style><div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red_band_index = page
        .rects
        .iter()
        .position(|rect| rect.fill == Some(Color::new(255, 0, 0)) && rect.height() == 37.5)
        .expect("gradient red band should be painted");
    let green_items = page
        .rects
        .iter()
        .enumerate()
        .filter(|(_, rect)| {
            rect.fill == Some(Color::new(0, 128, 0))
                && (rect.width() - 112.5).abs() < 0.01
                && (rect.height() - 38.25).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(green_items.len(), 2);
    for (green_index, green) in green_items {
        let red = &page.rects[red_band_index];
        assert!(green_index > red_band_index);
        assert!(green.y() <= red.y());
        assert!(green.y() + green.height() >= red.y() + red.height());
    }
}

#[tokio::test]
async fn align_content_flex_end_packs_lines_against_cross_end() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 140pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; flex-flow: wrap; align-content: flex-end; width: 225pt; height: 75pt;\
           background: linear-gradient(to bottom, green 0, green 37.5pt, red 37.5pt, red 75pt) }\
         .item { width: 112.5pt; height: 19.5pt; background: green }\
         </style><div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    let red_band_index = page
        .rects
        .iter()
        .position(|rect| rect.fill == Some(Color::new(255, 0, 0)) && rect.height() == 37.5)
        .expect("gradient red band should be painted");
    let red = &page.rects[red_band_index];
    let green_items = page
        .rects
        .iter()
        .enumerate()
        .filter(|(_, rect)| {
            rect.fill == Some(Color::new(0, 128, 0))
                && (rect.width() - 112.5).abs() < 0.01
                && (rect.height() - 19.5).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(green_items.len(), 4);
    let bottom = green_items
        .iter()
        .map(|(_, rect)| rect.y())
        .fold(f32::MAX, f32::min);
    let top = green_items
        .iter()
        .map(|(_, rect)| rect.y() + rect.height())
        .fold(f32::MIN, f32::max);
    assert!(bottom <= red.y());
    assert!(top >= red.y() + red.height());
    for (green_index, _) in green_items {
        assert!(green_index > red_band_index);
    }
}

#[tokio::test]
async fn flex_place_content_expands_to_align_and_justify_content() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 130pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; flex-wrap: wrap; place-content: flex-end space-between; width: 100pt; height: 80pt }\
         .item { width: 40pt; height: 10pt; background: green }\
         </style><div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let green_items = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 128, 0))
                && (rect.width() - 40.0).abs() < 0.01
                && (rect.height() - 10.0).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(green_items.len(), 4);
    let min_x = green_items
        .iter()
        .map(|rect| rect.x())
        .fold(f32::MAX, f32::min);
    let max_x = green_items
        .iter()
        .map(|rect| rect.x())
        .fold(f32::MIN, f32::max);
    assert!((min_x - 10.0).abs() < 0.1, "min_x={min_x}");
    assert!((max_x - 70.0).abs() < 0.1, "max_x={max_x}");
}

#[tokio::test]
async fn flex_gap_accepts_css_math_functions() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; gap: calc(5pt + 5pt); width: 120pt }\
         .item { width: 20pt; height: 10pt }\
         </style><div class=\"flex\"><div class=\"item\" style=\"background: green\"></div><div class=\"item\" style=\"background: blue\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - (green.x() + green.width() + 10.0)).abs() < 0.1);
}

#[tokio::test]
async fn vertical_rl_column_flex_gap_uses_physical_horizontal_axis() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin:0 }\
         .flex { writing-mode: vertical-rl; display:flex; flex-direction:column; gap:10pt; width:80pt; height:20pt; background:green }\
         .item { flex:0 0 auto; width:20pt; height:10pt }\
         </style><div class=\"flex\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:blue\"></div><div class=\"item\" style=\"background:black\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let red = rect(Color::new(255, 0, 0));
    let blue = rect(Color::new(0, 0, 255));
    let black = rect(Color::new(0, 0, 0));

    assert!((red.x() - 70.0).abs() < 0.01, "red={red:?}");
    assert!((blue.x() - 40.0).abs() < 0.01, "blue={blue:?}");
    assert!((black.x() - 10.0).abs() < 0.01, "black={black:?}");
    assert!((red.y() - blue.y()).abs() < 0.01 && (blue.y() - black.y()).abs() < 0.01);
}

#[tokio::test]
async fn vertical_rl_row_wrap_stacks_lines_from_physical_right() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin:0 }\
         .flex { display:flex; writing-mode:vertical-rl; flex-flow:row wrap; align-content:flex-start; width:40pt; height:30pt }\
         .flex > div { width:20pt; height:15pt }\
         .h > div { writing-mode:horizontal-tb }\
         </style>\
         <div class=\"flex\"><div style=\"background:cyan\"></div><div style=\"background:magenta\"></div><div style=\"background:yellow\"></div><div style=\"background:black\"></div></div>\
         <div class=\"flex h\"><div style=\"background:cyan\"></div><div style=\"background:magenta\"></div><div style=\"background:yellow\"></div><div style=\"background:black\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rects = |color| {
        page.rects
            .iter()
            .filter(|rect| rect.fill == Some(color))
            .collect::<Vec<_>>()
    };
    let cyans = rects(Color::new(0, 255, 255));
    let magentas = rects(Color::new(255, 0, 255));
    let yellows = rects(Color::new(255, 255, 0));
    let blacks = rects(Color::new(0, 0, 0));

    assert_eq!(cyans.len(), 2, "{:?}", page.rects);
    assert_eq!(magentas.len(), 2, "{:?}", page.rects);
    assert_eq!(yellows.len(), 2, "{:?}", page.rects);
    assert_eq!(blacks.len(), 2, "{:?}", page.rects);

    for ((cyan, magenta), yellow, black) in cyans
        .into_iter()
        .zip(magentas)
        .zip(yellows)
        .zip(blacks)
        .map(|(((cyan, magenta), yellow), black)| ((cyan, magenta), yellow, black))
    {
        assert!((cyan.width() - 20.0).abs() < 0.01 && (cyan.height() - 15.0).abs() < 0.01);
        assert!((cyan.x() - 20.0).abs() < 0.01, "cyan={cyan:?}");
        assert!((magenta.x() - 20.0).abs() < 0.01, "magenta={magenta:?}");
        assert!((yellow.x() - 0.0).abs() < 0.01, "yellow={yellow:?}");
        assert!((black.x() - 0.0).abs() < 0.01, "black={black:?}");
        assert!(
            (cyan.y() - yellow.y()).abs() < 0.01,
            "cyan={cyan:?}, yellow={yellow:?}"
        );
        assert!(
            (magenta.y() - black.y()).abs() < 0.01,
            "magenta={magenta:?}, black={black:?}"
        );
        assert!(
            ((cyan.y() - magenta.y()).abs() - 15.0).abs() < 0.01,
            "cyan={cyan:?}, magenta={magenta:?}"
        );
    }
}

#[tokio::test]
async fn vertical_rl_row_wrap_reverse_stacks_lines_from_physical_left() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin:0 }\
         .flex { display:flex; writing-mode:vertical-rl; flex-flow:row wrap-reverse; align-content:flex-start; width:40pt; height:30pt }\
         .flex > div { width:20pt; height:15pt }\
         </style><div class=\"flex\"><div style=\"background:cyan\"></div><div style=\"background:magenta\"></div><div style=\"background:yellow\"></div><div style=\"background:black\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let cyan = rect(Color::new(0, 255, 255));
    let magenta = rect(Color::new(255, 0, 255));
    let yellow = rect(Color::new(255, 255, 0));
    let black = rect(Color::new(0, 0, 0));

    assert!((cyan.x() - 0.0).abs() < 0.01, "cyan={cyan:?}");
    assert!((magenta.x() - 0.0).abs() < 0.01, "magenta={magenta:?}");
    assert!((yellow.x() - 20.0).abs() < 0.01, "yellow={yellow:?}");
    assert!((black.x() - 20.0).abs() < 0.01, "black={black:?}");
}

#[tokio::test]
async fn vertical_rl_row_align_items_flex_start_uses_physical_right() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin:0 }\
         .flex { display:flex; writing-mode:vertical-rl; flex-direction:row; align-items:flex-start; width:40pt; height:30pt }\
         .item { width:20pt; height:15pt; background:green }\
         </style><div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!((green.x() - 20.0).abs() < 0.01, "green={green:?}");
}

#[tokio::test]
async fn vertical_rl_row_flex_items_use_vertical_inline_forced_breaks() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 120pt 140pt; margin: 10pt } body { margin: 0 }\
         .container { display: flex; flex-flow: row; writing-mode: vertical-rl; border: 2pt solid black; height: 90pt }\
         .item { line-height: 0; float: right }\
         .color-block { display: inline-block; width: 15pt; height: 45pt }\
         </style><div class=\"container\">\
         <div class=\"item\"><span class=\"color-block\" style=\"background: orange\"></span><br><span class=\"color-block\" style=\"background: grey\"></span></div>\
         <div class=\"item\"><span class=\"color-block\" style=\"background: blue\"></span><br><span class=\"color-block\" style=\"background: yellow\"></span></div>\
         </div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let grey = rect(Color::new(128, 128, 128));
    let orange = rect(Color::new(255, 165, 0));
    let blue = rect(Color::new(0, 0, 255));
    let yellow = rect(Color::new(255, 255, 0));

    assert!(
        (grey.y() - orange.y()).abs() < 0.01 && (yellow.y() - blue.y()).abs() < 0.01,
        "each flex item should keep its forced-break columns on the same inline row: grey={grey:?}, orange={orange:?}, yellow={yellow:?}, blue={blue:?}"
    );
    assert!(
        grey.y() > yellow.y() + 40.0 && orange.y() > blue.y() + 40.0,
        "row flex main axis should place the first item above the second: grey={grey:?}, orange={orange:?}, yellow={yellow:?}, blue={blue:?}"
    );
    assert!(
        grey.x() + grey.width() <= orange.x() + 0.01
            && yellow.x() + yellow.width() <= blue.x() + 0.01,
        "vertical-rl forced breaks should put second-line blocks to the physical left: grey={grey:?}, orange={orange:?}, yellow={yellow:?}, blue={blue:?}"
    );
    assert!(
        (grey.x() - yellow.x()).abs() < 0.01 && (orange.x() - blue.x()).abs() < 0.01,
        "colors should form columns in clockwise WPT order: grey={grey:?}, orange={orange:?}, yellow={yellow:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn vertical_lr_row_wrap_stacks_lines_from_physical_left() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin:0 }\
         .flex { display:flex; writing-mode:vertical-lr; flex-flow:row wrap; align-content:flex-start; width:40pt; height:30pt }\
         .flex > div { width:20pt; height:15pt }\
         </style><div class=\"flex\"><div style=\"background:cyan\"></div><div style=\"background:magenta\"></div><div style=\"background:yellow\"></div><div style=\"background:black\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let cyan = rect(Color::new(0, 255, 255));
    let magenta = rect(Color::new(255, 0, 255));
    let yellow = rect(Color::new(255, 255, 0));
    let black = rect(Color::new(0, 0, 0));

    assert!((cyan.x() - 0.0).abs() < 0.01, "cyan={cyan:?}");
    assert!((magenta.x() - 0.0).abs() < 0.01, "magenta={magenta:?}");
    assert!((yellow.x() - 20.0).abs() < 0.01, "yellow={yellow:?}");
    assert!((black.x() - 20.0).abs() < 0.01, "black={black:?}");
}

#[tokio::test]
async fn vertical_rl_column_flex_ignores_direction_for_block_axis() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 60pt; margin: 0 } body { margin:0 }\
         .flex { writing-mode:vertical-rl; direction:rtl; display:flex; flex-direction:column; gap:10pt; width:80pt; height:20pt }\
         .item { flex:0 0 auto; width:20pt; height:10pt }\
         </style><div class=\"flex\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:blue\"></div><div class=\"item\" style=\"background:black\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let red = rect(Color::new(255, 0, 0));
    let blue = rect(Color::new(0, 0, 255));
    let black = rect(Color::new(0, 0, 0));

    assert!((red.x() - 60.0).abs() < 0.01, "red={red:?}");
    assert!((blue.x() - 30.0).abs() < 0.01, "blue={blue:?}");
    assert!((black.x() - 0.0).abs() < 0.01, "black={black:?}");
}

#[tokio::test]
async fn vertical_inline_block_and_inline_flex_atoms_use_logical_inline_size() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<link rel="help" href="https://drafts.csswg.org/css-align-3/#generate-baselines">
<link rel="help" href="https://www.w3.org/TR/css-inline-3/#valdef-dominant-baseline-auto">
<style>
#inline-block {
  display: inline-block;
  width: 100px;
  height: 50px;
  background: green;
}

#inline-flex {
  display: inline-flex;
}

#inline-flex > div {
  width: 100px;
  height: 50px;
  background: green;
}
</style>
<p>Test passes if there is a filled green square.</p>
<div style="width: 100px; height: 100px; line-height: 0; writing-mode: vertical-rl; background: red;">
  <span id="inline-block"></span><span id="inline-flex"><div></div></span>
</div>"#,
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut green_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .cloned()
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| a.y().partial_cmp(&b.y()).unwrap());

    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    let lower = &green_rects[0];
    let upper = &green_rects[1];
    assert!(
        (lower.x() - upper.x()).abs() < 0.01
            && (lower.width() - upper.width()).abs() < 0.01
            && (lower.height() - upper.height()).abs() < 0.01
            && (lower.width() - lower.height() * 2.0).abs() < 0.01
            && (lower.y() + lower.height() - upper.y()).abs() < 0.01,
        "vertical inline atoms should stack into one square: {green_rects:?}"
    );

    let page = &document.pages[0];
    assert_eq!(
        final_rect_fill_at(
            page,
            lower.x() + lower.width() / 2.0,
            lower.y() + lower.height() / 2.0
        ),
        Some(Color::new(0, 128, 0))
    );
    assert_eq!(
        final_rect_fill_at(
            page,
            upper.x() + upper.width() / 2.0,
            upper.y() + upper.height() / 2.0
        ),
        Some(Color::new(0, 128, 0))
    );
}

#[tokio::test]
async fn column_inline_flex_logical_block_margins_match_gap_spacing() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin:0; direction:rtl }\
         section { display:inline-flex; flex-direction:column; background:green }\
         section > div { width:50pt; height:10pt; background:gray }\
         .spaced { margin-block-end:15pt }\
         </style><section><div class=\"spaced\"></div><div class=\"spaced\"></div><div></div></section>",
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
        (green.height() - 60.0).abs() < 0.01,
        "two 15pt logical block-end margins should create column gaps: {green:?}"
    );

    let gray_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(128, 128, 128)))
        .collect::<Vec<_>>();
    assert_eq!(gray_rects.len(), 3);
    assert!(
        (gray_rects[0].y() - gray_rects[1].y() - 25.0).abs() < 0.01
            && (gray_rects[1].y() - gray_rects[2].y() - 25.0).abs() < 0.01,
        "successive 10pt items should be separated by 15pt logical block-end margins: {gray_rects:?}"
    );
}

#[tokio::test]
async fn flex_visibility_collapse_removes_item_from_main_axis_layout() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; width: 120pt }\
         .item { width: 40pt; height: 10pt }\
         </style><div class=\"flex\"><div class=\"item\" style=\"background: green\"></div><div class=\"item\" style=\"visibility: collapse; background: red\"></div><div class=\"item\" style=\"background: blue\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        !document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(255, 0, 0)))
    );
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - 50.0).abs() < 0.1, "blue.x()={}", blue.x());
}

#[tokio::test]
async fn flex_visibility_collapse_preserves_row_cross_size_strut() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; background: black; width: 120pt }\
         .short { width: 10pt; height: 10pt; background: green }\
         .tall { visibility: collapse; width: 40pt; height: 40pt; background: red }\
         </style><div class=\"flex\"><div class=\"short\"></div><div class=\"tall\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let flex_background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();

    assert!((flex_background.height() - 40.0).abs() < 0.1);
}

#[tokio::test]
async fn flex_root_preserves_subpoint_absolute_lengths() {
    let document = Html::from_string(
        r#"<style>
        @page { size: landscape; margin: 0 }
        body { margin: 0 }
        .root {
          align-items: center;
          background: #eef1f5;
          display: flex;
          height: 595.2756pt;
          justify-content: center;
          width: 841.8898pt;
        }
        .card {
          background: white;
          height: 8cm;
          width: 25cm;
        }
        </style><div class="root"><div class="card"></div></div>"#,
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let body = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::WHITE))
        .unwrap();

    // CSS Values and Units defines 1in = 96px = 72pt; flex layout must not
    // round the used box size/location before PDF painting.
    let expected_width = 25.0 * 72.0 / 2.54;
    let expected_height = 8.0 * 72.0 / 2.54;
    assert!((body.width() - expected_width).abs() < 0.001);
    assert!((body.height() - expected_height).abs() < 0.001);
    assert!((body.x() - ((841.8898 - expected_width) / 2.0)).abs() < 0.001);
}

#[tokio::test]
async fn flex_root_paints_html_background_on_page_canvas() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } :root { --page: #eef1f5 } html { display: flex; height: 100%; background: var(--page) } body { margin: 0 }</style><body>X</body>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(document.pages[0].rects.iter().any(|rect| {
        rect.x() == 0.0
            && rect.y() == 0.0
            && rect.width() == 100.0
            && rect.height() == 100.0
            && rect.fill == Some(Color::new(238, 241, 245))
    }));
}

#[tokio::test]
async fn flex_parent_background_paints_before_child_backgrounds() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body { margin: 0 } .parent { display: flex; background: white; width: 80pt; height: 40pt } .child { background: black; width: 40pt; height: 20pt }</style><div class=\"parent\"><div class=\"child\"></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let white_index = document.pages[0]
        .rects
        .iter()
        .position(|rect| rect.fill == Some(Color::WHITE))
        .unwrap();
    let black_index = document.pages[0]
        .rects
        .iter()
        .position(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();

    assert!(white_index < black_index);
}

#[tokio::test]
async fn nested_column_flex_item_uses_intrinsic_auto_width() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } .row { display: flex; width: 200pt } .fill { flex-grow: 1; height: 20pt } .stub { display: flex; flex-direction: column; background: black; padding: 0 10pt } .stub p { margin: 0 }</style><div class=\"row\"><div class=\"fill\"></div><div class=\"stub\"><p>Stub</p></div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let stub = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();

    assert!(stub.width() > 20.0);
    assert!(stub.width() < 80.0);
    assert!(stub.x() > 120.0);
}

#[tokio::test]
async fn flex_auto_basis_preserves_non_growing_item_content_width() {
    let document = Html::from_file_async("weasyprint-samples/invoice/invoice.html")
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let developers_line = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.contains("Our awesome developers"))
        .or_else(|| {
            let first_line = ["Our", "awesome", "developers"]
                .into_iter()
                .map(|text| {
                    document.pages[0]
                        .lines
                        .iter()
                        .find(|line| line.text == text)
                })
                .collect::<Option<Vec<_>>>()?;
            first_line
                .windows(2)
                .all(|pair| (pair[0].y() - pair[1].y()).abs() < 0.1)
                .then_some(first_line[0])
        })
        .expect("invoice developer text should stay on one line");

    assert!(developers_line.x() > 380.0);
}

#[tokio::test]
async fn flex_auto_basis_border_box_includes_padding_and_border() {
    let document = Html::from_file_async("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let divider = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(42, 50, 57))
                && (rect.width() - 1.0).abs() < 0.01
                && rect.height() > 2.0
                && rect.height() < 4.0
        })
        .min_by(|left, right| left.y().total_cmp(&right.y()))
        .unwrap();

    assert!((divider.x() - 598.84).abs() < 1.0);
}

#[tokio::test]
async fn shrink_to_fit_inline_block_uses_exact_graph_max_content_width() {
    let document = Html::from_file_async("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "CDG ✈ LFLL" && line.font_size == 25.0)
    );
}

#[tokio::test]
async fn flex_item_text_line_fit_uses_sequence_backed_max_content_width() {
    let document = Html::from_file_async("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "THÉODORE MARCELIN" && line.font_size == 18.0)
    );
}

#[tokio::test]
async fn inline_origin_abspos_uses_inline_static_position() {
    let document = Html::from_file_async("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let name = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "THÉODORE MARCELIN" && line.font_size == 25.0)
        .unwrap();
    let destination = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "CDG ✈ LFLL" && line.font_size == 25.0)
        .unwrap();

    assert!(
        (name.y() - destination.y()).abs() < 0.01,
        "name y={} destination y={}",
        name.y(),
        destination.y()
    );
}

#[tokio::test]
async fn absolutely_positioned_inline_block_shrink_wraps_auto_width() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body { margin: 0 } .box { position: relative; width: 160pt; height: 40pt; font-size: 10pt; line-height: 10pt } h1 { display: inline-block; position: absolute; right: 0; margin: 0; font-size: 10pt; line-height: 10pt; font-weight: 400 }</style><div class=\"box\"><h1>Wide Label</h1></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "Wide Label");
    assert!(lines[0].x() > 100.0);
}

#[tokio::test]
async fn inline_block_content_participates_in_parent_inline_line() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt }</style><p>Before <span style=\"display:inline-block\">Box</span> After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let boxed = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Box")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(before.x() < boxed.x());
    assert!(boxed.x() < after.x());
    assert!((before.y() - after.y()).abs() < 0.1);
}

#[tokio::test]
async fn inline_block_does_not_create_implicit_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:20pt; background:black }</style><p>A<span>B</span>C</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "C")
        .unwrap();

    assert!((after.x() - (background.x() + background.width())).abs() < 1.0);
}

#[tokio::test]
async fn inline_block_preserves_explicit_collapsed_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:20pt; background:black }</style><p>A <span>B</span> C</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "C")
        .unwrap();

    assert!(first_visible_glyph_x(after) - (background.x() + background.width()) > 2.0);
}

#[tokio::test]
async fn inline_block_paints_atomic_box_before_following_inline_text() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:40pt; padding:5pt; background:black }</style><p>Before <span>Box</span> After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK) && rect.width() > 40.0)
        .unwrap();
    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let boxed = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Box")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!((background.width() - 50.0).abs() < 0.1);
    assert!(before.x() < background.x());
    assert!(boxed.x() > background.x());
    assert!(first_visible_glyph_x(after) > background.x() + background.width());
}

#[tokio::test]
async fn inline_block_explicit_height_is_not_expanded_by_line_height() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body, p { margin: 0 } span { display:inline-block; width:28.5pt; height:28.5pt; border:0.75pt solid #32cd32; background:green; font-size:12pt; line-height:30pt; color:white }</style><p><span>1</span></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!((background.height() - 30.0).abs() < 0.01);
}

#[tokio::test]
async fn inline_block_lays_out_block_children_as_atomic_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:40pt; padding:4pt; background:black } b { display:block; font-weight:400 }</style><p>Before <span><b>One</b><b>Two</b></span> After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK) && rect.width() > 40.0)
        .unwrap();
    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let one = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "One")
        .unwrap();
    let two = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Two")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!((background.width() - 48.0).abs() < 0.1);
    assert!(background.height() >= 32.0);
    assert!(before.x() < background.x());
    assert!(one.x() > background.x());
    assert!(two.x() > background.x());
    assert!(two.y() < one.y());
    assert!(first_visible_glyph_x(after) > background.x() + background.width());
}

#[tokio::test]
async fn inline_block_fragment_replays_through_paint_operation_stream() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:40pt; padding:4pt; background:black } b { display:block; font-weight:400 }</style><p>Before <span><b>One</b><b>Two</b></span> After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let page = &document.pages[0];

    let background_index = page
        .rects
        .iter()
        .position(|rect| rect.fill == Some(Color::BLACK) && rect.width() > 40.0)
        .unwrap();
    let one_index = page
        .lines
        .iter()
        .position(|line| line.text == "One")
        .unwrap();
    let two_index = page
        .lines
        .iter()
        .position(|line| line.text == "Two")
        .unwrap();

    let background_operation = page
        .operations
        .iter()
        .position(|operation| {
            matches!(operation, quire::PaintOperation::Rect(index) if *index == background_index)
        })
        .unwrap();
    let one_operation = page
        .operations
        .iter()
        .position(|operation| {
            matches!(operation, quire::PaintOperation::Line(index) if *index == one_index)
        })
        .unwrap();
    let two_operation = page
        .operations
        .iter()
        .position(|operation| {
            matches!(operation, quire::PaintOperation::Line(index) if *index == two_index)
        })
        .unwrap();

    assert!(background_operation < one_operation);
    assert!(one_operation < two_operation);
}

#[tokio::test]
async fn flex_items_are_blockified_for_painting() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin: 0 } .flex { display:flex; width:100pt } address { flex:1 50%; height:10pt; background:red }</style><div class=\"flex\"><address>Item</address></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert_eq!(red.width(), 100.0);
    assert_eq!(document.pages[0].lines[0].text, "Item");
}

#[tokio::test]
async fn supports_flex_column() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { margin: 0 }</style><div style=\"display:flex; flex-direction:column; font-size:10pt; line-height:10pt\"><span>One</span><span>Two</span></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "One");
    assert_eq!(document.pages[0].lines[1].text, "Two");
    assert!(document.pages[0].lines[1].y() < document.pages[0].lines[0].y());
}

#[tokio::test]
async fn flex_inline_svg_rows_ignore_formatting_whitespace_for_height() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 20pt } html, body, p { margin: 0; font-size: 10pt; line-height: 10pt }</style><div style=\"display:flex; margin:0\"><div style=\"flex-grow:1\">\n<svg width=\"15\" height=\"15\"><rect width=\"15\" height=\"15\" fill=\"#2292d4\" /></svg>\n<small> Half Match </small>\n<svg width=\"15\" height=\"15\"><rect width=\"15\" height=\"15\" fill=\"#175377\" /></svg>\n<small> Full Match </small>\n</div></div><p>After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert_line_baseline_at_top(&document, after, 165.1668);
}

#[tokio::test]
async fn supports_min_max_block_dimensions() {
    let document = Html::from_string(
        "<div style=\"margin: 0; width: 50pt; min-width: 80pt; height: 50pt; max-height: 20pt; background: red\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rect = &document.pages[0].rects[0];
    assert_eq!(rect.width(), 80.0);
    assert_eq!(rect.height(), 20.0);
}

#[tokio::test]
async fn supports_border_box_sizing() {
    let document = Html::from_string(
        "<div style=\"margin: 0; box-sizing: border-box; width: 50pt; height: 20pt; padding: 2pt; border: 1pt solid black; background: red\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rect = &document.pages[0].rects[0];
    assert_eq!(rect.width(), 50.0);
    assert_eq!(rect.height(), 20.0);
}

#[tokio::test]
async fn collects_inline_children_and_line_breaks() {
    let document = Html::from_string("<p>Hello <span>nested</span><br>line &amp; more</p>")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "Hello nested");
    assert_eq!(document.pages[0].lines[1].text, "line & more");
    assert_eq!(document.pages[0].lines.len(), 2);
}

#[tokio::test]
async fn mixed_block_and_inline_content_keeps_document_order() {
    let document = Html::from_string(
        "<div><div><strong>Othram</strong><br>Address</div><strong>Disclaimer</strong><br>Text</div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(lines, ["Othram", "Address", "Disclaimer", "Text"]);
}

#[tokio::test]
async fn block_parents_do_not_duplicate_heading_text_or_split_plain_spans() {
    let document = Html::from_string(
        "<div><h4>Parameters</h4><p>Segment detection: <span>&ge;7 cM &bull;&nbsp;</span><span>&ge;200 SNPs &bull;&nbsp;</span><span>0 &le; MAF &le; 0.5 &bull;&nbsp;</span><span>MB 100 SNPs</span></p></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        lines.iter().filter(|line| **line == "Parameters").count(),
        1
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Segment detection:") && line.contains("MB 100 SNPs"))
    );
}

#[tokio::test]
async fn wrapped_inline_fragments_keep_line_text_coalesced_and_trimmed() {
    let text = "alpha beta gamma delta epsilon";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 80pt 200pt; margin: 10pt }} body, p {{ margin: 0; font-size: 10pt; line-height: 10pt }}</style><p>{text}</p>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rendered_lines = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(rendered_lines.len() > 1);
    assert_eq!(rendered_lines.join(" "), text);
    assert!(
        rendered_lines
            .iter()
            .all(|line| !line.starts_with(' ') && !line.ends_with(' '))
    );
}

#[tokio::test]
async fn block_outline_paints_after_child_content() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .parent { width: 40pt; height: 40pt; outline: 2pt solid red }\
         .child { width: 20pt; height: 20pt; background: blue }</style>\
         <div class=\"parent\"><div class=\"child\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];

    let child_operation = first_rect_paint_operation_index(page, Color::new(0, 0, 255));
    let outline_operation = first_rect_paint_operation_index(page, Color::new(255, 0, 0));

    assert!(
        outline_operation > child_operation,
        "outline should paint after descendant content: child={child_operation}, outline={outline_operation}"
    );
}

#[tokio::test]
async fn inline_block_outline_paints_after_atomic_content() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 20pt }\
         span { display: inline-block; width: 30pt; height: 30pt; outline: 2pt solid red }\
         b { display: block; width: 15pt; height: 15pt; background: blue }</style>\
         <p><span><b></b></span></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];

    let child_operation = first_rect_paint_operation_index(page, Color::new(0, 0, 255));
    let outline_operation = first_rect_paint_operation_index(page, Color::new(255, 0, 0));

    assert!(
        outline_operation > child_operation,
        "inline-block outline should paint after atomic descendant content"
    );
}

#[tokio::test]
async fn float_band_paints_between_in_flow_block_and_inline_content() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font-size: 0 }\
         .block { width: 20pt; height: 20pt; background: red }\
         .float { float: left; width: 20pt; height: 20pt; background: green }\
         .inline { display: inline-block; width: 20pt; height: 20pt; background: blue }</style>\
         <div class=\"block\"></div><div class=\"float\"></div><span class=\"inline\"></span>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];

    let block_operation = first_rect_paint_operation_index(page, Color::new(255, 0, 0));
    let float_operation = first_rect_paint_operation_index(page, Color::new(0, 128, 0));
    let inline_operation = first_rect_paint_operation_index(page, Color::new(0, 0, 255));

    assert!(block_operation < float_operation);
    assert!(float_operation < inline_operation);
}

fn colored_rect_width(document: &quire::Document, color: Color) -> f32 {
    document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(color))
        .unwrap_or_else(|| {
            panic!(
                "expected rect with color {color:?}: {:?}",
                document.pages[0].rects
            )
        })
        .width()
}

#[tokio::test]
async fn inline_block_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 0 } body { margin: 0; font: 10px/12px sans-serif }\
         div { height: 18pt } span { display: inline-block }\
         .min { width: min-content; background: green }\
         .fit { width: fit-content(14px); background: blue }\
         .max { width: max-content; background: black }</style>\
         <div><span class=\"min\">aa bb</span></div><div><span class=\"fit\">aa bb</span></div><div><span class=\"max\">aa bb</span></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let min = colored_rect_width(&document, Color::new(0, 128, 0));
    let fit = colored_rect_width(&document, Color::new(0, 0, 255));
    let max = colored_rect_width(&document, Color::new(0, 0, 0));
    assert!(
        min < fit && fit < max,
        "inline-block intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}

#[tokio::test]
async fn abspos_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 0 } body { margin: 0; font: 10px/12px sans-serif }\
         .box { position: absolute; left: 0; height: 12px }\
         .min { top: 0; width: min-content; background: green }\
         .fit { top: 20px; width: fit-content(14px); background: blue }\
         .max { top: 40px; width: max-content; background: black }</style>\
         <div class=\"box min\">aa bb</div><div class=\"box fit\">aa bb</div><div class=\"box max\">aa bb</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let min = colored_rect_width(&document, Color::new(0, 128, 0));
    let fit = colored_rect_width(&document, Color::new(0, 0, 255));
    let max = colored_rect_width(&document, Color::new(0, 0, 0));
    assert!(
        min < fit && fit < max,
        "abspos intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}

#[tokio::test]
async fn float_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 160pt; margin: 0 } body { margin: 0; font: 10px/12px sans-serif }\
         .box { float: left; clear: left; height: 12px }\
         .min { width: min-content; background: green }\
         .fit { width: fit-content(14px); background: blue }\
         .max { width: max-content; background: black }</style>\
         <div class=\"box min\">aa bb</div><div class=\"box fit\">aa bb</div><div class=\"box max\">aa bb</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let min = colored_rect_width(&document, Color::new(0, 128, 0));
    let fit = colored_rect_width(&document, Color::new(0, 0, 255));
    let max = colored_rect_width(&document, Color::new(0, 0, 0));
    assert!(
        min < fit && fit < max,
        "float intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}

#[tokio::test]
async fn inline_table_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 160pt; margin: 0 } body { margin: 0; font: 10px/12px sans-serif }\
         div { height: 18pt } table { display: inline-table; border-spacing: 0 } td { padding: 0 }\
         .min { width: min-content; background: green }\
         .fit { width: fit-content(14px); background: blue }\
         .max { width: max-content; background: black }</style>\
         <div><table class=\"min\"><tr><td>aa bb</td></tr></table></div>\
         <div><table class=\"fit\"><tr><td>aa bb</td></tr></table></div>\
         <div><table class=\"max\"><tr><td>aa bb</td></tr></table></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let min = colored_rect_width(&document, Color::new(0, 128, 0));
    let fit = colored_rect_width(&document, Color::new(0, 0, 255));
    let max = colored_rect_width(&document, Color::new(0, 0, 0));
    assert!(
        min < fit && fit < max,
        "inline-table intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}
