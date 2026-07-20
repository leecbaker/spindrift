use super::*;

fn rects_with_fill(page: &quire::Page, fill: CssColor) -> Vec<&quire::RenderedRect> {
    page.rects()
        .iter()
        .filter(|rect| rect.fill == Some(fill))
        .collect()
}

#[tokio::test]
async fn zoomed_grid_scales_fixed_tracks_gaps_and_item_margins() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 200pt; margin: 0 }
         body { margin: 0 }
         #grid { display: grid; width: 100pt; grid-template-columns: 10pt 50%;
                 grid-auto-rows: 20pt; column-gap: 5pt; zoom: 2;
                 background: rgb(0 128 0) }
         #grid > div { margin: 2pt; background: rgb(255 0 0); height: 4pt }
         </style><div id=\"grid\"><div></div><div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = rects_with_fill(page, CssColor::new(0, 128, 0))
        .into_iter()
        .max_by(|left, right| left.width().total_cmp(&right.width()))
        .expect("zoomed grid background");
    assert!((grid.width() - 200.0).abs() < 0.01, "grid={grid:?}");

    let items = rects_with_fill(page, CssColor::new(255, 0, 0));
    assert_eq!(items.len(), 2, "items={items:?}");
    assert!(
        items.iter().any(|item| (item.width() - 12.0).abs() < 0.01),
        "fixed track should be 20pt less two 4pt used margins: {items:?}"
    );
    assert!(
        items.iter().any(|item| (item.width() - 92.0).abs() < 0.01),
        "percentage track should resolve against the 200pt zoomed grid: {items:?}"
    );
}

#[tokio::test]
async fn zoomed_inline_grid_uses_scaled_outer_geometry() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 300pt 120pt; margin: 0 }
         body { margin: 0 }
         #grid { display: inline-grid; width: 30pt; grid-template-columns: 10pt 10pt;
                 column-gap: 5pt; zoom: 2; background: rgb(0 0 255) }
         #grid > div { height: 10pt; background: rgb(255 255 0) }
         </style><span id=\"grid\"><div></div><div></div></span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = rects_with_fill(page, CssColor::new(0, 0, 255))
        .into_iter()
        .max_by(|left, right| left.width().total_cmp(&right.width()))
        .expect("zoomed inline-grid background");
    assert!((grid.width() - 60.0).abs() < 0.01, "grid={grid:?}");
}

#[tokio::test]
async fn inherited_zoom_on_grid_item_is_applied_once() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 300pt 160pt; margin: 0 }
         body { margin: 0 }
         #grid { display: grid; width: 80pt; grid-template-columns: 40pt; zoom: 2 }
         #item { zoom: inherit }
         #paint { width: 5pt; height: 5pt; background: rgb(128 0 128) }
         </style><div id=\"grid\"><div id=\"item\"><div id=\"paint\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let paint = rects_with_fill(page, CssColor::new(128, 0, 128))
        .into_iter()
        .next()
        .expect("inherited-zoom descendant background");
    assert!((paint.width() - 20.0).abs() < 0.01, "paint={paint:?}");
    assert!((paint.height() - 20.0).abs() < 0.01, "paint={paint:?}");
}
