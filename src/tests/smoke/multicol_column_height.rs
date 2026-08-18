use super::*;

fn column_height_016(rule_declarations: &str) -> String {
    let forced_breaks = "<div class=\"break\"></div>".repeat(8);
    format!(
        "<!doctype html><style>
         @page {{ size: 200px 200px; margin: 0 }}
         body {{ margin: 0 }}
         .outer {{ width: 100px; height: 100px; background: red }}
         .columns {{ columns: 2; column-height: 25px; column-wrap: wrap; gap: 0;
                     background: green; {rule_declarations} }}
         .break {{ height: 1px; break-after: column }}
         </style><div class=\"outer\"><div class=\"columns\">{forced_breaks}</div></div>"
    )
}

/// Regression for WPT `css/css-multicol/column-height-016.html`.
///
/// The eight forced breaks form four committed rows although their measured
/// content height is smaller than one column height.  The default `none`
/// row-rule list must therefore remain safe even with three committed gaps.
#[tokio::test]
async fn column_height_016_default_row_rule_does_not_panic() {
    let document = Html::from_string(column_height_016(""))
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages.len(), 1);
    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("the multicol background should render");
    assert!(
        (green.height() - 75.0).abs() < 0.01,
        "four 25px rows must occupy 75pt; green={green:?}"
    );
}

/// The same committed topology must also assign all three row-rule slots when
/// a list is authored; slot lookup is valid independently of paintability.
#[tokio::test]
async fn column_height_016_explicit_row_rule_list_does_not_panic() {
    let document = Html::from_string(column_height_016(
        "row-rule-width: 1px, 2px, 3px; row-rule-style: solid; \
         row-rule-color: red, blue, green",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
}
