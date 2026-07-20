use super::*;

fn filled_rects(page: &quire::Page, fill: CssColor) -> Vec<&quire::RenderedRect> {
    page.rects()
        .iter()
        .filter(|rect| rect.fill == Some(fill))
        .collect()
}

#[tokio::test]
async fn zoomed_multicol_scales_fixed_columns_gaps_and_rule_widths() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 220pt; margin: 0 }
         body { margin: 0 }
         #columns { width: 100pt; height: 40pt; columns: 20pt; column-gap: 5pt;
                    column-rule: 2pt solid rgb(0 255 0); zoom: 2; background: rgb(0 0 255) }
         .paint { width: 5pt; height: 25pt; background: rgb(255 0 0) }
         </style><div id=\"columns\"><div class=\"paint\"></div><div class=\"paint\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let paints = filled_rects(page, CssColor::new(255, 0, 0));
    assert!(!paints.is_empty(), "operations={:?}", page.operations());
    assert!(
        paints
            .iter()
            .all(|paint| (paint.width() - 10.0).abs() < 0.01)
    );
    let has_rule = page.strokes().iter().any(|stroke| {
        stroke.color == CssColor::new(0, 255, 0)
            && (stroke.stroke_width.points() - 4.0).abs() < 0.01
            && (stroke.x1() - stroke.x2()).abs() < 0.01
    }) || page.rects().iter().any(|rect| {
        rect.fill == Some(CssColor::new(0, 255, 0))
            && (rect.width() - 4.0).abs() < 0.01
            && rect.height() > 0.0
    });
    assert!(
        has_rule,
        "expected a 4pt zoomed column rule; operations={:?}; strokes={:?}; rects={:?}",
        page.operations(),
        page.strokes(),
        page.rects()
    );
}

#[tokio::test]
async fn explicitly_inherited_multicol_values_use_the_container_effective_zoom() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 220pt; margin: 0 }
         body { margin: 0 }
         #parent { column-width: 20pt; column-gap: 5pt; column-rule: 2pt solid rgb(0 255 255);
                   display: contents }
         #columns { width: 100pt; height: 40pt; column-width: inherit; column-gap: inherit;
                    column-rule-width: inherit; column-rule-style: solid; column-rule-color: rgb(0 255 255);
                    zoom: 2 }
         .paint { width: 5pt; height: 25pt; background: rgb(128 0 128) }
         </style><div id=\"parent\"><div id=\"columns\"><div class=\"paint\"></div><div class=\"paint\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let paints = filled_rects(page, CssColor::new(128, 0, 128));
    assert!(!paints.is_empty(), "paints={paints:?}");
    assert!(
        paints
            .iter()
            .all(|paint| (paint.width() - 10.0).abs() < 0.01)
    );
    assert!(
        page.strokes().iter().any(|stroke| {
            stroke.color == CssColor::new(0, 255, 255)
                && (stroke.stroke_width.points() - 4.0).abs() < 0.01
        }) || page.rects().iter().any(|rect| {
            rect.fill == Some(CssColor::new(0, 255, 255)) && (rect.width() - 4.0).abs() < 0.01
        }),
        "strokes={:?}; rects={:?}",
        page.strokes(),
        page.rects()
    );
}

#[tokio::test]
async fn zoomed_nested_multicol_spanner_and_wrapped_rows_keep_one_effective_scale() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 220pt; margin: 0 }
         body { margin: 0 }
         #outer { width: 100pt; height: 40pt; column-width: 20pt; column-height: 20pt;
                  column-wrap: wrap; column-gap: 5pt; zoom: 2 }
         #spanner { column-span: all; width: 5pt; height: 5pt; background: rgb(0 0 255) }
         #nested { width: 20pt; columns: 10pt; column-gap: 5pt; background: rgb(0 255 0) }
         #inner { width: 5pt; height: 5pt; background: rgb(255 128 0) }
         </style><div id=\"outer\"><div id=\"spanner\"></div><div id=\"nested\"><div id=\"inner\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    for (color, expected_width) in [
        (CssColor::new(0, 0, 255), 10.0),
        (CssColor::new(0, 255, 0), 40.0),
        (CssColor::new(255, 128, 0), 10.0),
    ] {
        assert!(
            filled_rects(page, color)
                .iter()
                .any(|rect| (rect.width() - expected_width).abs() < 0.01),
            "expected a {expected_width}pt zoomed {color:?} box; rects={:?}",
            page.rects()
        );
    }
}
