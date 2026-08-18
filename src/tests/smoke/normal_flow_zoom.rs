use super::*;

fn filled_rect(
    page: &quire::Page,
    fill: CssColor,
) -> &crate::document::paint::shapes::RenderedRect {
    page.rects()
        .iter()
        .find(|rect| rect.fill == Some(fill))
        .unwrap_or_else(|| panic!("missing {fill:?} rectangle; rects={:?}", page.rects()))
}

/// CSS Viewport `zoom` scales a normal-flow box's fixed used dimensions.
///
/// This mirrors `css/css-viewport/zoom/basic.html`. In particular, the
/// second box crosses its own used-value boundary; it must not inherit an
/// "already zoomed" marker from the document root or body.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
#[tokio::test]
async fn normal_flow_zoom_scales_fixed_dimensions_at_its_own_used_value_boundary() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 400pt; margin: 0 }
         body { margin: 0 }
         #plain, #zoomed { width: 100px; height: 100px }
         #plain { background: rgb(0 128 0) }
         #zoomed { background: rgb(255 0 0); zoom: 2 }
         </style><div id=\"plain\"></div><div id=\"zoomed\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let plain = filled_rect(page, CssColor::new(0, 128, 0));
    let zoomed = filled_rect(page, CssColor::new(255, 0, 0));
    assert!((plain.width() - 75.0).abs() < 0.01, "plain={plain:?}");
    assert!((plain.height() - 75.0).abs() < 0.01, "plain={plain:?}");
    assert!((zoomed.width() - 150.0).abs() < 0.01, "zoomed={zoomed:?}");
    assert!((zoomed.height() - 150.0).abs() < 0.01, "zoomed={zoomed:?}");
}

/// An explicit inherited local zoom composes with the ancestor effective
/// zoom, while a normal descendant consumes the composed factor once.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
#[tokio::test]
async fn normal_flow_explicit_inherited_zoom_composes_without_reusing_used_style() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 300pt 200pt; margin: 0 }
         body { margin: 0 }
         #outer { zoom: 2 }
         #inherited { zoom: inherit }
         #paint { width: 5pt; height: 5pt; background: rgb(128 0 128) }
         </style><div id=\"outer\"><div id=\"inherited\"><div id=\"paint\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let paint = filled_rect(&document.pages[0], CssColor::new(128, 0, 128));
    assert!((paint.width() - 20.0).abs() < 0.01, "paint={paint:?}");
    assert!((paint.height() - 20.0).abs() < 0.01, "paint={paint:?}");
}
