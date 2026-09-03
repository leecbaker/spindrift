use super::*;

fn filled_rects(
    page: &spindrift::Page,
    fill: CssColor,
) -> Vec<&crate::document::paint::shapes::RenderedRect> {
    page.rects()
        .iter()
        .filter(|rect| rect.fill == Some(fill))
        .collect()
}

#[tokio::test]
async fn zoomed_separated_table_scales_parts_and_spacing_once() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 200pt; margin: 0 }
         body { margin: 0 }
         table { margin: 0; border-spacing: 5pt; zoom: 2; background: rgb(0 128 0) }
         caption { height: 3pt; background: rgb(0 0 255) }
         td { padding: 2pt; border: 1pt solid black }
         .paint { width: 5pt; height: 5pt; background: rgb(255 0 0) }
         </style><table><caption></caption><tr><td><div class=\"paint\"></div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let paint = filled_rects(page, CssColor::new(255, 0, 0))
        .into_iter()
        .next()
        .expect("zoomed table-cell descendant paint");
    assert!((paint.width() - 10.0).abs() < 0.01, "paint={paint:?}");
    assert!((paint.height() - 10.0).abs() < 0.01, "paint={paint:?}");
    let caption = filled_rects(page, CssColor::new(0, 0, 255))
        .into_iter()
        .next()
        .expect("zoomed caption paint");
    assert!((caption.height() - 6.0).abs() < 0.01, "caption={caption:?}");
}

#[tokio::test]
async fn explicitly_inherited_zoom_on_a_table_cell_is_applied_once() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 200pt; margin: 0 }
         body { margin: 0 }
         #outer { zoom: 2 }
         table { margin: 0; border-spacing: 0 }
         td { zoom: inherit; padding: 0 }
         .paint { width: 5pt; height: 5pt; background: rgb(128 0 128) }
         </style><div id=\"outer\"><table><tr><td><div class=\"paint\"></div></td></tr></table></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let paint = filled_rects(page, CssColor::new(128, 0, 128))
        .into_iter()
        .next()
        .expect("explicitly inherited table-cell descendant paint");
    assert!((paint.width() - 10.0).abs() < 0.01, "paint={paint:?}");
    assert!((paint.height() - 10.0).abs() < 0.01, "paint={paint:?}");
}

#[tokio::test]
async fn zoomed_table_percentages_resolve_against_the_used_table_width() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 200pt; margin: 0 }
         body { margin: 0 }
         table { width: 100pt; margin: 0; border-spacing: 0; zoom: 2 }
         td { padding: 0 }
         .paint { width: 50%; height: 5pt; background: rgb(255 165 0) }
         </style><table><tr><td><div class=\"paint\"></div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let paint = filled_rects(page, CssColor::new(255, 165, 0))
        .into_iter()
        .next()
        .expect("percentage-width table-cell descendant paint");
    assert!((paint.width() - 100.0).abs() < 0.01, "paint={paint:?}");
    assert!((paint.height() - 10.0).abs() < 0.01, "paint={paint:?}");
}

#[tokio::test]
async fn zoomed_html_cellspacing_and_collapsed_borders_use_scaled_geometry() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 200pt; margin: 0 }
         body { margin: 0 }
         #spaced { margin: 0; zoom: 2 }
         #spaced td { padding: 0 }
         #collapsed { width: 50pt; margin: 20pt 0 0; border-collapse: collapse;
                      border: 2pt solid black; zoom: 2; background: rgb(0 128 0) }
         #collapsed td { border: 2pt solid black; padding: 0; height: 5pt }
         .paint { width: 5pt; height: 5pt; background: rgb(0 255 255) }
         </style><table id=\"spaced\" cellspacing=\"5\"><tr><td><div class=\"paint\"></div></td><td><div class=\"paint\"></div></td></tr></table><table id=\"collapsed\"><tr><td></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let paints = filled_rects(page, CssColor::new(0, 255, 255));
    assert_eq!(paints.len(), 2, "paints={paints:?}");
    let (left, right) = if paints[0].x() <= paints[1].x() {
        (paints[0], paints[1])
    } else {
        (paints[1], paints[0])
    };
    // HTML `cellspacing=5` is 5 CSS px = 3.75pt before zoom, therefore
    // 7.5pt after the table's effective zoom of two.
    assert!(
        (right.x() - (left.x() + left.width()) - 7.5).abs() < 0.01,
        "paints={paints:?}"
    );

    let collapsed = filled_rects(page, CssColor::new(0, 128, 0))
        .into_iter()
        .max_by(|left, right| left.width().total_cmp(&right.width()))
        .expect("collapsed-table background");
    // A specified collapsed-table width is its wrapper width. The 50pt width
    // therefore becomes 100pt after zoom; its 2pt outer half-insets reduce
    // the grid width rather than expanding the wrapper background.
    assert!(
        (collapsed.width() - 100.0).abs() < 0.01,
        "collapsed={collapsed:?}"
    );
}
