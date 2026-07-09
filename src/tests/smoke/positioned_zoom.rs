use super::*;

fn rect_with_fill(page: &quire::Page, fill: Color) -> &quire::RenderedRect {
    page.rects()
        .iter()
        .find(|rect| rect.fill == Some(fill))
        .unwrap_or_else(|| panic!("missing {fill:?} rectangle; rects={:?}", page.rects()))
}

#[tokio::test]
async fn zoomed_relative_absolute_and_fixed_boxes_use_one_positioned_scale() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 300pt 220pt; margin: 0 }
         body { margin: 0 }
         #relative { position: relative; left: 5pt; top: 5pt; width: 5pt; height: 5pt;
                     background: rgb(255 0 0); zoom: 2 }
         #containing { contain: layout; width: 50pt; height: 30pt; translate: 0 -10pt }
         #absolute { position: absolute; top: 5pt; width: 5pt; height: 5pt;
                     background: rgb(0 255 0); zoom: 2 }
         #fixed { position: fixed; left: 5pt; top: 5pt; width: 5pt; height: 5pt;
                  background: rgb(0 0 255); zoom: 2 }
         </style><div id=\"relative\"></div><div id=\"containing\"><div id=\"absolute\"></div></div><div id=\"fixed\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let relative = rect_with_fill(page, Color::new(255, 0, 0));
    let absolute = rect_with_fill(page, Color::new(0, 255, 0));
    let fixed = rect_with_fill(page, Color::new(0, 0, 255));
    for rect in [relative, absolute, fixed] {
        assert!((rect.width() - 10.0).abs() < 0.01, "rect={rect:?}");
        assert!((rect.height() - 10.0).abs() < 0.01, "rect={rect:?}");
    }
    assert!((relative.x() - 10.0).abs() < 0.01, "rect={relative:?}");
}
