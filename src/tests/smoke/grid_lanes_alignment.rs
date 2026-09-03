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
async fn grid_lanes_stacking_content_distribution_moves_only_in_flow_items() {
    for (row_lanes, alignment, stacking_size, expected_offset) in [
        (false, "start", 200.0, 0.0),
        (false, "center", 200.0, 30.0),
        (false, "end", 200.0, 60.0),
        (false, "safe end", 100.0, 0.0),
        (true, "start", 200.0, 0.0),
        (true, "center", 200.0, 30.0),
        (true, "end", 200.0, 60.0),
        (true, "safe end", 100.0, 0.0),
    ] {
        let (
            grid_axis_declaration,
            stacking_axis_declaration,
            content_alignment_property,
            item_size_declaration,
            oof_inset,
        ) = if row_lanes {
            (
                "grid-template-rows: 80pt",
                format!("width: {stacking_size}pt"),
                "justify-content",
                "width: 40pt; height: 80pt",
                "top: 0",
            )
        } else {
            (
                "grid-template-columns: 80pt",
                format!("height: {stacking_size}pt"),
                "align-content",
                "width: 80pt; height: 40pt",
                "left: 0",
            )
        };
        let document = Html::from_string(format!(
            "<!doctype html><style>
             @page {{ size: 400pt 400pt; margin: 0 }}
             body {{ margin: 0 }}
             #grid {{ display: grid-lanes; width: 200pt; height: 200pt;
                      {grid_axis_declaration}; {stacking_axis_declaration};
                      gap: 10pt; {content_alignment_property}: {alignment}; position: relative }}
             .item {{ {item_size_declaration}; background: rgb(0 0 255) }}
             .oof {{ position: absolute; width: 20pt; height: 20pt;
                     background: rgb(255 0 0); {oof_inset} }}
             </style>
             <div id=grid><div class=item></div><div class=item></div><div class=item></div>
             <div class=oof></div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let item = rect_with_fill(page, CssColor::new(0, 0, 255));
        let oof = rect_with_fill(page, CssColor::new(255, 0, 0));
        let observed_offset = if row_lanes {
            (item.x() - oof.x()).abs()
        } else {
            ((item.y() + item.height()) - (oof.y() + oof.height())).abs()
        };
        assert!(
            (observed_offset - expected_offset).abs() < 0.01,
            "row_lanes={row_lanes}, alignment={alignment}, item={item:?}, oof={oof:?}"
        );
    }
}

#[tokio::test]
async fn row_grid_lanes_stretch_simple_auto_tracks() {
    for (alignment, expected_separation) in [("stretch", 150.0), ("normal", 150.0), ("start", 20.0)]
    {
        let document = Html::from_string(format!(
            "<!doctype html><style>
             @page {{ size: 400pt 400pt; margin: 0 }}
             body {{ margin: 0 }}
             #grid {{ display: grid-lanes; width: 300pt; height: 300pt;
                      grid-template-rows: repeat(2, auto); align-content: {alignment} }}
             #grid > div {{ width: 20pt; height: 20pt }}
             #first {{ background: rgb(255 0 0) }}
             #second {{ background: rgb(0 0 255) }}
             </style><div id=\"grid\"><div id=\"first\"></div><div id=\"second\"></div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let first = rect_with_fill(page, CssColor::new(255, 0, 0));
        let second = rect_with_fill(page, CssColor::new(0, 0, 255));
        assert!(
            ((first.y() - second.y()).abs() - expected_separation).abs() < 0.01,
            "align-content: {alignment}; first={first:?}, second={second:?}"
        );
    }
}

async fn assert_mixed_grid_lanes_track_separation(
    row_lanes: bool,
    grid_axis_size: f32,
    content_alignment: &str,
    expected_separation: f32,
) {
    let (grid_dimensions, template_property, alignment_property, first_item_axis_size) =
        if row_lanes {
            (
                format!("width: 300pt; height: {grid_axis_size}pt"),
                "grid-template-rows",
                "align-content",
                "height",
            )
        } else {
            (
                format!("width: {grid_axis_size}pt; height: 300pt"),
                "grid-template-columns",
                "justify-content",
                "width",
            )
        };
    let document = Html::from_string(format!(
        "<!doctype html><style>
         @page {{ size: 500pt 500pt; margin: 0 }}
         body {{ margin: 0 }}
         #grid {{ display: grid-lanes; {grid_dimensions};
                  {template_property}: 100pt auto auto;
                  {alignment_property}: {content_alignment} }}
         #grid > div {{ width: 20pt; height: 20pt }}
         #grid > #first {{ {first_item_axis_size}: 100pt; background: rgb(255 0 0) }}
         #third {{ background: rgb(0 255 0) }}
         </style><div id=\"grid\"><div id=\"first\"></div><div></div><div id=\"third\"></div></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let first = rect_with_fill(page, CssColor::new(255, 0, 0));
    let third = rect_with_fill(page, CssColor::new(0, 255, 0));
    let separation = if row_lanes {
        ((first.y() + first.height()) - (third.y() + third.height())).abs()
    } else {
        (first.x() - third.x()).abs()
    };
    assert!(
        (separation - expected_separation).abs() < 0.01,
        "row_lanes={row_lanes}, align={content_alignment}; first={first:?}, third={third:?}"
    );
}

#[tokio::test]
async fn mixed_grid_lanes_auto_tracks_receive_automatic_item_contributions() {
    for row_lanes in [false, true] {
        assert_mixed_grid_lanes_track_separation(row_lanes, 300.0, "start", 200.0).await;
    }
}

#[tokio::test]
async fn mixed_grid_lanes_stretch_only_auto_tracks() {
    for row_lanes in [false, true] {
        assert_mixed_grid_lanes_track_separation(row_lanes, 400.0, "stretch", 250.0).await;
    }
}

async fn assert_named_grid_lanes_auto_track_start(row_lanes: bool, template: &str) {
    let (
        grid_dimensions,
        template_property,
        alignment_property,
        item_axis_size,
        placement_property,
    ) = if row_lanes {
        (
            "width: 300pt; height: 400pt",
            "grid-template-rows",
            "align-content",
            "height",
            "grid-row",
        )
    } else {
        (
            "width: 400pt; height: 300pt",
            "grid-template-columns",
            "justify-content",
            "width",
            "grid-column",
        )
    };
    let document = Html::from_string(format!(
        "<!doctype html><style>
         @page {{ size: 500pt 500pt; margin: 0 }}
         body {{ margin: 0 }}
         #grid {{ display: grid-lanes; {grid_dimensions};
                  {template_property}: {template}; {alignment_property}: stretch }}
         #grid > div {{ width: 20pt; height: 20pt }}
         #grid > #first {{ {placement_property}: first; {item_axis_size}: 120pt;
                           background: rgb(255 0 0) }}
         #grid > #second {{ {placement_property}: second; {item_axis_size}: 40pt;
                            background: rgb(0 0 255) }}
         </style><div id=\"grid\"><div id=\"first\"></div><div id=\"second\"></div></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let second = rect_with_fill(page, CssColor::new(0, 0, 255));
    let second_track_start = if row_lanes {
        500.0 - (second.y() + second.height())
    } else {
        second.x()
    };
    assert!(
        (second_track_start - 290.0).abs() < 0.01,
        "row_lanes={row_lanes}, template={template}; second={second:?}"
    );
}

#[tokio::test]
async fn named_grid_lanes_tracks_keep_explicit_contributions_local() {
    for row_lanes in [false, true] {
        assert_named_grid_lanes_auto_track_start(row_lanes, "100pt [first] auto [second] auto")
            .await;
    }
}

#[tokio::test]
async fn numbered_repeat_named_grid_lanes_tracks_size_from_named_placement() {
    assert_named_grid_lanes_auto_track_start(false, "100pt repeat(1, [first] auto [second] auto)")
        .await;
}

async fn assert_template_area_grid_lanes_auto_track_start(
    row_lanes: bool,
    template: &str,
    areas: &str,
) {
    let (
        grid_dimensions,
        template_property,
        alignment_property,
        item_axis_size,
        placement_property,
    ) = if row_lanes {
        (
            "width: 300pt; height: 400pt",
            "grid-template-rows",
            "align-content",
            "height",
            "grid-row",
        )
    } else {
        (
            "width: 400pt; height: 300pt",
            "grid-template-columns",
            "justify-content",
            "width",
            "grid-column",
        )
    };
    let document = Html::from_string(format!(
        "<!doctype html><style>
         @page {{ size: 500pt 500pt; margin: 0 }}
         body {{ margin: 0 }}
         #grid {{ display: grid-lanes; {grid_dimensions};
                  {template_property}: {template}; grid-template-areas: {areas};
                  {alignment_property}: stretch }}
         #grid > div {{ width: 20pt; height: 20pt }}
         #grid > #first {{ {placement_property}: first; {item_axis_size}: 120pt;
                           background: rgb(255 0 0) }}
         #grid > #second {{ {placement_property}: second; {item_axis_size}: 40pt;
                            background: rgb(0 0 255) }}
         </style><div id=\"grid\"><div id=\"first\"></div><div id=\"second\"></div></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let second = rect_with_fill(page, CssColor::new(0, 0, 255));
    let second_track_start = if row_lanes {
        500.0 - (second.y() + second.height())
    } else {
        second.x()
    };
    assert!(
        (second_track_start - 290.0).abs() < 0.01,
        "row_lanes={row_lanes}, template={template}, areas={areas}; second={second:?}"
    );
}

#[tokio::test]
async fn template_area_grid_lanes_tracks_keep_named_area_contributions_local() {
    assert_template_area_grid_lanes_auto_track_start(
        false,
        "100pt auto auto",
        "\". first second\"",
    )
    .await;
    assert_template_area_grid_lanes_auto_track_start(
        true,
        "100pt auto auto",
        "\".\" \"first\" \"second\"",
    )
    .await;
}

#[tokio::test]
async fn numbered_repeat_template_area_grid_lanes_tracks_size_from_named_area_placement() {
    assert_template_area_grid_lanes_auto_track_start(
        false,
        "100pt repeat(1, auto auto)",
        "\". first second\"",
    )
    .await;
}

async fn assert_area_created_grid_lanes_track_separation(
    row_lanes: bool,
    template: &str,
    areas: &str,
    auto_tracks: &str,
    expected_separation: f32,
) {
    let (
        grid_dimensions,
        template_property,
        auto_track_property,
        alignment_property,
        item_axis_size,
    ) = if row_lanes {
        (
            "width: 300pt; height: 300pt",
            "grid-template-rows",
            "grid-auto-rows",
            "align-content",
            "height",
        )
    } else {
        (
            "width: 300pt; height: 300pt",
            "grid-template-columns",
            "grid-auto-columns",
            "justify-content",
            "width",
        )
    };
    let document = Html::from_string(format!(
        "<!doctype html><style>
         @page {{ size: 400pt 400pt; margin: 0 }}
         body {{ margin: 0 }}
         #grid {{ display: grid-lanes; {grid_dimensions};
                  {template_property}: {template}; grid-template-areas: {areas};
                  {auto_track_property}: {auto_tracks}; {alignment_property}: start }}
         #grid > div {{ width: 20pt; height: 20pt }}
         #grid > #first {{ {item_axis_size}: 100pt; background: rgb(255 0 0) }}
         #third {{ background: rgb(0 255 0) }}
         </style><div id=\"grid\"><div id=\"first\"></div><div></div><div id=\"third\"></div></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let first = rect_with_fill(page, CssColor::new(255, 0, 0));
    let third = rect_with_fill(page, CssColor::new(0, 255, 0));
    let separation = if row_lanes {
        ((first.y() + first.height()) - (third.y() + third.height())).abs()
    } else {
        (first.x() - third.x()).abs()
    };
    assert!(
        (separation - expected_separation).abs() < 0.01,
        "row_lanes={row_lanes}, template={template}, areas={areas}, \
         auto_tracks={auto_tracks}; first={first:?}, third={third:?}"
    );
}

#[tokio::test]
async fn area_created_grid_lanes_auto_tracks_receive_automatic_item_contributions() {
    for (row_lanes, areas) in [(false, "\". . .\""), (true, "\".\" \".\" \".\"")] {
        assert_area_created_grid_lanes_track_separation(row_lanes, "100pt", areas, "auto", 200.0)
            .await;
    }
}

#[tokio::test]
async fn area_only_column_grid_lanes_tracks_receive_automatic_item_contributions() {
    assert_area_created_grid_lanes_track_separation(false, "none", "\". . .\"", "auto", 200.0)
        .await;
}

#[tokio::test]
async fn area_created_grid_lanes_tracks_cycle_fixed_and_auto_grid_auto_tracks() {
    assert_area_created_grid_lanes_track_separation(false, "50pt", "\". . .\"", "auto 50pt", 150.0)
        .await;
}

#[tokio::test]
async fn area_created_grid_lanes_stretch_only_auto_grid_auto_tracks() {
    let document = Html::from_string(
        "<!doctype html><style>
         @page { size: 500pt 500pt; margin: 0 }
         body { margin: 0 }
         #grid { display: grid-lanes; width: 400pt; height: 300pt;
                 grid-template-columns: 50pt; grid-template-areas: \". . .\";
                 grid-auto-columns: auto 50pt; justify-content: stretch }
         #grid > div { width: 20pt; height: 20pt }
         #first { width: 100pt; background: rgb(255 0 0) }
         #third { background: rgb(0 255 0) }
         </style><div id=\"grid\"><div id=\"first\"></div><div></div><div id=\"third\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let third = rect_with_fill(&document.pages[0], CssColor::new(0, 255, 0));
    assert!((third.x() - 350.0).abs() < 0.01, "third={third:?}");
}
