use super::*;

fn rect_with_fill(
    page: &spindrift::Page,
    fill: CssColor,
) -> &crate::document::paint::shapes::RenderedRect {
    page.rects()
        .iter()
        .find(|rect| rect.fill == Some(fill))
        .expect("painted item background")
}

#[tokio::test]
async fn row_grid_lanes_percentage_flow_tolerance_uses_grid_axis_height() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 700pt 400pt; margin: 0 }
         body { margin: 0 }
         #grid { display: grid-lanes; width: 600pt; height: 300pt;
                 grid-template-rows: repeat(3, 100pt); flow-tolerance: 17% }
         #grid > div { height: 20pt }
         #second { background: rgb(0 255 0) }
         #fourth { width: 100pt; background: rgb(255 0 0) }
         </style><div id=\"grid\">
         <div style=\"width: 150pt\"></div><div id=\"second\" style=\"width: 100pt\"></div>
         <div style=\"width: 90pt\"></div><div id=\"fourth\"></div>
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let second = rect_with_fill(page, CssColor::new(0, 255, 0));
    let fourth = rect_with_fill(page, CssColor::new(255, 0, 0));
    // 17% is 51pt from the 300pt row-grid axis. At 51pt the first row is
    // outside the 60pt difference, so the fourth item continues in row two.
    assert!(
        (fourth.y() - second.y()).abs() < 0.01,
        "second={second:?}, fourth={fourth:?}"
    );
}

#[tokio::test]
async fn zoomed_column_grid_lanes_scale_fixed_flow_tolerance() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 700pt 500pt; margin: 0 }
         body { margin: 0 }
         #grid { display: grid-lanes; width: 300pt; grid-template-columns: repeat(3, 100pt);
                 flow-tolerance: 51pt; zoom: 2 }
         #grid > div { width: 20pt }
         #fourth { height: 100pt; background: rgb(0 0 255) }
         </style><div id=\"grid\">
         <div style=\"height: 130pt\"></div><div style=\"height: 100pt\"></div>
         <div style=\"height: 90pt\"></div><div id=\"fourth\"></div>
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let fourth = rect_with_fill(&document.pages[0], CssColor::new(0, 0, 255));
    // Zoom doubles the lane ends and the fixed 51pt tolerance to 102pt. The
    // first lane is then tied with the shortest lane and wins cursor fallback.
    assert!((fourth.x() - 0.0).abs() < 0.01, "fourth={fourth:?}");
}
