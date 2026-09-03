use super::*;

fn painted_rect(
    page: &spindrift::Page,
    color: CssColor,
) -> &crate::document::paint::shapes::RenderedRect {
    page.rects()
        .iter()
        .find(|rect| rect.fill == Some(color))
        .expect("painted regression box")
}

/// A column Grid Lanes subgrid borrows every parent track and gutter in its
/// definite `grid-column` span, while its row axis remains an ordinary local
/// Grid axis. CSS Grid Level 2 §9 and Grid Level 3 §3.2.
#[tokio::test]
async fn column_grid_lanes_subgrid_uses_its_full_parent_span_and_local_row_gap() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 400pt; margin: 0 }
         body { margin: 0 }
         #parent { display: grid-lanes; width: 190pt;
                   grid-template-columns: repeat(4, 40pt); gap: 20pt 10pt }
         #subgrid { grid-column: 2 / 5; display: grid; grid-template-columns: subgrid;
                    column-gap: 10pt; row-gap: 100pt; align-content: start;
                    background: rgb(0 0 0) }
         #subgrid > div { height: 20pt }
         #e { background: rgb(255 0 0) } #f { background: rgb(0 255 0) }
         #g { background: rgb(0 0 255) } #h { background: rgb(255 0 255) }
         </style><div id=parent><div id=subgrid>
           <div id=e></div><div id=f></div><div id=g></div><div id=h></div>
         </div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let subgrid = painted_rect(page, CssColor::new(0, 0, 0));
    let e = painted_rect(page, CssColor::new(255, 0, 0));
    let f = painted_rect(page, CssColor::new(0, 255, 0));
    let g = painted_rect(page, CssColor::new(0, 0, 255));
    let h = painted_rect(page, CssColor::new(255, 0, 255));

    // Tracks 2–4 occupy 3 × 40pt plus two parent-owned 10pt gutters.
    assert!((subgrid.x() - 50.0).abs() < 0.01, "subgrid={subgrid:?}");
    assert!(
        (subgrid.width() - 140.0).abs() < 0.01,
        "subgrid={subgrid:?}"
    );
    assert!((f.x() - e.x() - 50.0).abs() < 0.01, "e={e:?}, f={f:?}");
    assert!((g.x() - f.x() - 50.0).abs() < 0.01, "f={f:?}, g={g:?}");
    // The fourth item starts the standalone second row: 20pt item + 100pt local gap.
    assert!((e.y() - h.y() - 120.0).abs() < 0.01, "e={e:?}, h={h:?}");
}

/// The row-axis counterpart keeps parent rows and their gutters while the
/// subgrid's ordinary column axis keeps its own column gap.
#[tokio::test]
async fn row_grid_lanes_subgrid_uses_its_full_parent_span_and_local_column_gap() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 400pt 400pt; margin: 0 }
         body { margin: 0 }
         #parent { display: grid-lanes; width: 300pt; height: 190pt;
                   grid-template-rows: repeat(4, 40pt); gap: 10pt 20pt }
         #subgrid { grid-row: 2 / 5; display: grid; grid-template-rows: subgrid;
                    row-gap: 200pt; grid-template-columns: repeat(2, 20pt); column-gap: 100pt;
                    background: rgb(0 0 0) }
         #subgrid > div { height: 20pt }
         #e { background: rgb(255 0 0) } #f { background: rgb(0 255 0) }
         #g { background: rgb(0 0 255) } #h { background: rgb(255 0 255) }
         </style><div id=parent><div id=subgrid>
           <div id=e></div><div id=f></div><div id=g></div><div id=h></div>
         </div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let subgrid = painted_rect(page, CssColor::new(0, 0, 0));
    let e = painted_rect(page, CssColor::new(255, 0, 0));
    let g = painted_rect(page, CssColor::new(0, 0, 255));
    let h = painted_rect(page, CssColor::new(255, 0, 255));

    // PDF coordinates originate at the page bottom, so a logical 50pt top
    // position for a 140pt subgrid has a physical y origin of 210pt.
    assert!((subgrid.y() - 210.0).abs() < 0.01, "subgrid={subgrid:?}");
    assert!(
        (subgrid.height() - 140.0).abs() < 0.01,
        "subgrid={subgrid:?}"
    );
    // The standalone column axis retains its own 100pt column gap.
    assert!((h.x() - g.x() - 120.0).abs() < 0.01, "g={g:?}, h={h:?}");
    // The inherited row axis retains the parent's 10pt gutter, not 200pt.
    assert!((e.y() - g.y() - 100.0).abs() < 0.01, "e={e:?}, g={g:?}");
}
