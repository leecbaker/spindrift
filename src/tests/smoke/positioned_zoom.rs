use super::*;

fn rect_with_fill(
    page: &spindrift::Page,
    fill: CssColor,
) -> &crate::document::paint::shapes::RenderedRect {
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
    let relative = rect_with_fill(page, CssColor::new(255, 0, 0));
    let absolute = rect_with_fill(page, CssColor::new(0, 255, 0));
    let fixed = rect_with_fill(page, CssColor::new(0, 0, 255));
    for rect in [relative, absolute, fixed] {
        assert!((rect.width() - 10.0).abs() < 0.01, "rect={rect:?}");
        assert!((rect.height() - 10.0).abs() < 0.01, "rect={rect:?}");
    }
    assert!((relative.x() - 10.0).abs() < 0.01, "rect={relative:?}");
}

/// CSS Ruby layout-internal boxes remain part of their ruby formatting
/// context, but a positioned ruby/rbc still establishes the containing block
/// for an absolutely positioned descendant.
/// <https://drafts.csswg.org/css-ruby-1/#formatting-context>
/// <https://drafts.csswg.org/css-position-3/#def-cb>
#[tokio::test]
async fn positioned_descendant_of_ruby_base_container_is_painted() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 200pt 200pt; margin: 0 }
         body { margin: 8px; font: 50px/3 serif }
         .rel { position: relative; unicode-bidi: isolate }
         .abs { position: absolute; left: 0; top: -1em; background: rgb(255 0 0) }
         </style>X<ruby><rbc class=\"rel\"><rb><span class=\"abs\">X</span></rb></rbc></ruby>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let positioned = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("the positioned ruby descendant must produce a paint fragment");
    assert!(
        positioned.x() > 20.0,
        "the ruby/rbc containing block starts after the preceding glyph: {positioned:?}"
    );
}

/// Ruby inlinification turns a direct in-flow block child into an inline
/// flow-root, retaining both its independent formatting context and its box
/// paint.
/// <https://drafts.csswg.org/css-ruby-1/#anon-gen-inlinize>
#[tokio::test]
async fn direct_block_child_of_ruby_is_an_inline_flow_root_atom() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 200pt 200pt; margin: 0 }
         .inline { display: block; background-color: rgb(255 255 0); width: 30px; height: 30px }
         </style><div><ruby>a<div class=\"inline\">b</div>c</ruby></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 255, 0))),
        "the inlinified direct block must retain its background paint"
    );
}
