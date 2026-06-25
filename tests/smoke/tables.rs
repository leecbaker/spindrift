use super::*;

#[tokio::test]
async fn renders_basic_tables_in_rows_and_columns() {
    let document = Html::from_string(
        "<table style=\"margin: 0; width: 120pt\"><tr><th style=\"border: 1pt solid black\">A</th><th>B</th></tr><tr><td>C</td><td>D</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "A");
    assert_eq!(document.pages[0].lines[1].text, "B");
    assert_eq!(document.pages[0].lines[2].text, "C");
    assert_eq!(document.pages[0].lines[3].text, "D");
    assert!(document.pages[0].lines[1].x > document.pages[0].lines[0].x);
    assert!(document.pages[0].lines[2].y < document.pages[0].lines[0].y);
    assert_eq!(document.pages[0].rects[0].fill, Some(Color::BLACK));
    assert_eq!(document.pages[0].rects[0].stroke, None);
}

#[tokio::test]
async fn supports_authored_table_internal_display_values() {
    let document = Html::from_string(
        "<div style=\"display:table;margin:0;width:80pt;border-spacing:0\">\
         <div style=\"display:table-row\"><span style=\"display:table-cell;width:40pt\">A</span><span style=\"display:table-cell;width:40pt\">B</span></div>\
         </div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[0].text, "A");
    assert_eq!(lines[1].text, "B");
    assert!(lines[1].x > lines[0].x);
}

#[tokio::test]
async fn auto_table_sizing_uses_inline_edges_for_cell_contributions() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body, table, td { margin: 0; font-size: 10pt; line-height: 10pt }\
         table { border-spacing: 0; table-layout: auto } td { padding: 0 } .pad { padding-left: 42pt; padding-right: 6pt }</style>\
         <table><tr><td><span class=\"pad\">A</span></td><td>Next</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    let next = lines.iter().find(|line| line.text == "Next").unwrap();
    assert!(
        next.x > 55.0,
        "second cell should be shifted by the first cell's inline padding: {next:?}"
    );
}

#[tokio::test]
async fn nested_table_fragment_contributes_to_cell_intrinsic_width() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body, table, td { margin:0; font-size:10pt; line-height:10pt; border-spacing:0; padding:0 } .inner td { width:90pt }</style>\
         <table><tr><td><table class=\"inner\"><tr><td>Inner</td></tr></table></td><td>Next</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let inner = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Inner")
        .unwrap();
    let next = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Next")
        .unwrap();

    assert!(
        next.x - inner.x > 85.0,
        "outer cell should reserve the nested table fragment width: inner={inner:?}, next={next:?}"
    );
}

#[tokio::test]
async fn table_row_groups_use_css_visual_order() {
    let document = Html::from_string(
        "<style>body { margin: 0; font-size: 10pt; line-height: 10pt } table { margin: 0; border-spacing: 0 } th, td { padding: 0; text-align: left }</style>\
         <table>\
         <tfoot><tr><td>Foot</td></tr></tfoot>\
         <tbody><tr><td>Body</td></tr></tbody>\
         <thead><tr><th>Head</th></tr></thead>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[0].text, "Head");
    assert_eq!(lines[1].text, "Body");
    assert_eq!(lines[2].text, "Foot");
    assert!(lines[0].y > lines[1].y);
    assert!(lines[1].y > lines[2].y);
}

#[tokio::test]
async fn authored_table_row_groups_use_css_visual_order() {
    let document = Html::from_string(
        "<style>\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .table { display: table; margin: 0; border-spacing: 0 }\
         .head { display: table-header-group }\
         .body { display: table-row-group }\
         .foot { display: table-footer-group }\
         .row { display: table-row }\
         span { display: table-cell; padding: 0 }\
         </style>\
         <div class=\"table\">\
         <div class=\"foot\"><div class=\"row\"><span>Foot</span></div></div>\
         <div class=\"body\"><div class=\"row\"><span>Body</span></div></div>\
         <div class=\"head\"><div class=\"row\"><span>Head</span></div></div>\
         </div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[0].text, "Head");
    assert_eq!(lines[1].text, "Body");
    assert_eq!(lines[2].text, "Foot");
    assert!(lines[0].y > lines[1].y);
    assert!(lines[1].y > lines[2].y);
}

#[tokio::test]
async fn only_first_header_and_footer_group_are_visually_special() {
    let document = Html::from_string(
        "<style>body { margin: 0; font-size: 10pt; line-height: 10pt } table { margin: 0; border-spacing: 0 } th, td { padding: 0; text-align: left }</style>\
         <table>\
         <tfoot><tr><td>Foot 1</td></tr></tfoot>\
         <tbody><tr><td>Body 1</td></tr></tbody>\
         <thead><tr><th>Head 1</th></tr></thead>\
         <tbody><tr><td>Body 2</td></tr></tbody>\
         <thead><tr><th>Head 2</th></tr></thead>\
         <tbody><tr><td>Body 3</td></tr></tbody>\
         <tfoot><tr><td>Foot 2</td></tr></tfoot>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        [
            "Head 1", "Body 1", "Body 2", "Head 2", "Body 3", "Foot 2", "Foot 1"
        ]
    );
}

#[tokio::test]
async fn repeated_table_header_and_footer_keep_page_fragment_order() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         th, td { padding: 0; text-align: left; width: 60pt; height: 20pt }\
         thead th, tfoot td { height: 10pt }</style>\
         <table>\
         <thead><tr><th>Head</th></tr></thead>\
         <tbody><tr><td>Body 1</td></tr><tr><td>Body 2</td></tr><tr><td>Body 3</td></tr></tbody>\
         <tfoot><tr><td>Foot</td></tr></tfoot>\
         </table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages.len() >= 2);
    let mut repeated_headers = 0;
    let mut repeated_footers = 0;
    let mut body_3_has_header = false;
    for page in &document.pages {
        let header = page.lines.iter().find(|line| line.text == "Head");
        let footer = page.lines.iter().find(|line| line.text == "Foot");
        let bodies = page
            .lines
            .iter()
            .filter(|line| line.text.starts_with("Body "))
            .collect::<Vec<_>>();
        if header.is_some() {
            repeated_headers += 1;
        }
        if footer.is_some() {
            repeated_footers += 1;
        }
        if bodies.iter().any(|line| line.text == "Body 3") && header.is_some() {
            body_3_has_header = true;
        }
        if let Some(header) = header {
            for body in &bodies {
                assert!(header.y > body.y, "header should paint above body rows");
            }
        }
        if let Some(footer) = footer {
            for body in &bodies {
                assert!(body.y > footer.y, "footer should paint below body rows");
            }
        }
    }

    assert!(repeated_headers >= 2);
    assert!(repeated_footers >= 1);
    assert!(body_3_has_header);
}

#[tokio::test]
async fn repeated_collapsed_table_header_paints_fragment_borders() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 60pt }\
         th, td { padding: 0; text-align: left; width: 60pt; height: 20pt }\
         thead th { height: 10pt; border: 2pt solid red }</style>\
         <table>\
         <thead><tr><th>Head</th></tr></thead>\
         <tbody><tr><td>Body 1</td></tr><tr><td>Body 2</td></tr><tr><td>Body 3</td></tr></tbody>\
         </table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let header_pages = document
        .pages
        .iter()
        .filter(|page| page.lines.iter().any(|line| line.text == "Head"))
        .collect::<Vec<_>>();
    assert!(
        header_pages.len() >= 2,
        "header should repeat on later table fragments"
    );
    for page in header_pages.into_iter().skip(1) {
        assert!(
            page.paint_operations().iter().any(|operation| {
                matches!(
                    operation,
                    quire::PaintOperation::Rect(index)
                        if page.rects.get(*index).is_some_and(|rect| {
                            rect.fill == Some(Color::new(255, 0, 0))
                                && rect.width > 0.0
                                && rect.height > 0.0
                        })
                )
            }),
            "repeated collapsed header fragment should include resolved border paint"
        );
    }
}

#[tokio::test]
async fn repeated_table_header_fragment_preserves_structural_background_order() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 60pt; background: red }\
         colgroup { background: green } thead { background: blue } thead tr { background: yellow }\
         th, td { padding: 0; text-align: left; width: 60pt; height: 20pt } thead th { height: 10pt }</style>\
         <table><colgroup><col></colgroup>\
         <thead><tr><th>Head</th></tr></thead>\
         <tbody><tr><td>Body 1</td></tr><tr><td>Body 2</td></tr><tr><td>Body 3</td></tr></tbody>\
         </table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let repeated_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines.iter().any(|line| line.text == "Head"))
        .expect("header should repeat on a later page");
    let table = first_rect_paint_operation_index(repeated_page, Color::new(255, 0, 0));
    let column = first_rect_paint_operation_index(repeated_page, Color::new(0, 128, 0));
    let row_group = first_rect_paint_operation_index(repeated_page, Color::new(0, 0, 255));
    let row = first_rect_paint_operation_index(repeated_page, Color::new(255, 255, 0));

    assert!(table < column);
    assert!(column < row_group);
    assert!(row_group < row);
}

#[tokio::test]
async fn repeated_table_header_traps_positioned_descendants_in_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 60pt } th, td { padding: 0; text-align: left; width: 60pt; height: 20pt }\
         thead th { position: relative; height: 10pt }\
         thead span { position: absolute; left: 30pt; top: 0; width: 10pt; height: 10pt; background: red }</style>\
         <table>\
         <thead><tr><th>Head<span></span></th></tr></thead>\
         <tbody><tr><td>Body 1</td></tr><tr><td>Body 2</td></tr><tr><td>Body 3</td></tr></tbody>\
         </table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let repeated_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines.iter().any(|line| line.text == "Head"))
        .expect("header should repeat on a later page");
    assert!(
        repeated_page.paint_operations().iter().any(|operation| {
            matches!(
                operation,
                quire::PaintOperation::Rect(index)
                    if repeated_page.rects.get(*index).is_some_and(|rect| {
                        rect.fill == Some(Color::new(255, 0, 0))
                            && rect.width > 0.0
                            && rect.height > 0.0
                    })
            )
        }),
        "positioned descendants should replay inside the repeated header fragment"
    );
}

#[tokio::test]
async fn fragmented_collapsed_table_body_paints_borders_on_each_page() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 90pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 60pt }\
         td { padding: 0; text-align: left; width: 60pt; height: 30pt; border: 2pt solid red }</style>\
         <table><tbody>\
         <tr><td>Body 1</td></tr><tr><td>Body 2</td></tr><tr><td>Body 3</td></tr><tr><td>Body 4</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let body_pages = document
        .pages
        .iter()
        .filter(|page| page.lines.iter().any(|line| line.text.starts_with("Body ")))
        .collect::<Vec<_>>();
    assert!(body_pages.len() >= 2);
    for page in body_pages {
        assert!(
            page.paint_operations().iter().any(|operation| {
                matches!(
                    operation,
                    quire::PaintOperation::Rect(index)
                        if page.rects.get(*index).is_some_and(|rect| {
                            rect.fill == Some(Color::new(255, 0, 0))
                                && rect.width > 0.0
                                && rect.height > 0.0
                        })
                )
            }),
            "each fragmented table body page should include collapsed border paint"
        );
    }
}

#[tokio::test]
async fn fragmented_collapsed_table_uses_full_grid_boundary_winners() {
    let document = Html::from_string(
        "<style>@page{size:120pt 90pt;margin:10pt}body{margin:0}table{margin:0;border-collapse:collapse;width:60pt;border-top:8pt solid red}td{padding:0;width:60pt;height:20pt}.second{break-before:page;border-top:2pt solid blue}</style>\
         <table><tr><td>A</td></tr><tr class=\"second\"><td>B</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let second_page = document
        .pages
        .iter()
        .find(|page| page.lines.iter().any(|line| line.text == "B"))
        .expect("second row should be forced to a later page");
    let red_horizontal = second_page.rects.iter().any(|rect| {
        rect.fill == Some(Color::new(255, 0, 0)) && rect.width > 20.0 && rect.height >= 7.9
    });
    let blue_horizontal = second_page.rects.iter().any(|rect| {
        rect.fill == Some(Color::new(0, 0, 255))
            && rect.width > 20.0
            && (rect.height - 2.0).abs() < 0.01
    });

    assert!(
        blue_horizontal,
        "page fragment should paint the original row-boundary winner"
    );
    assert!(
        !red_horizontal,
        "page fragment must not synthesize the table top border at an internal row boundary"
    );
}

#[tokio::test]
async fn fragmented_collapsed_table_background_uses_wrapper_slice_bounds() {
    let document = Html::from_string(
        "<style>@page{size:100pt 90pt;margin:10pt}body{margin:0}table{margin:0;border-collapse:collapse;width:40pt;background:green}td{padding:0;width:40pt;height:50pt;border-left:2pt solid black;border-right:2pt solid black}</style>\
         <table><tr><td>A</td></tr><tr><td>B</td></tr><tr><td>C</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let table_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.lines
                .iter()
                .any(|line| matches!(line.text.as_str(), "A" | "B" | "C"))
        })
        .collect::<Vec<_>>();
    assert!(table_pages.len() >= 2);
    for page in table_pages {
        let green = page
            .rects
            .iter()
            .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
            .expect("table background should paint on each fragment");
        assert!(
            (green.width - 42.0).abs() < 0.01,
            "fragment table background should include collapsed wrapper insets, got {green:?}"
        );
    }
}

#[tokio::test]
async fn repeated_collapsed_header_uses_full_grid_bottom_boundary() {
    let document = Html::from_string(
        "<style>@page{size:100pt 80pt;margin:10pt}body{margin:0}table{margin:0;border-collapse:collapse;width:60pt;border-bottom:8pt solid red;font-size:10pt;line-height:10pt}th,td{padding:0;width:60pt;height:20pt;text-align:left}th{height:10pt;border-bottom:2pt solid blue}</style>\
         <table><thead><tr><th>Head</th></tr></thead><tbody><tr><td>Body 1</td></tr><tr><td>Body 2</td></tr><tr><td>Body 3</td></tr><tr><td>Body 4</td></tr><tr><td>Body 5</td></tr><tr><td>Body 6</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let repeated_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| {
            page.lines.iter().any(|line| line.text == "Head")
                && !page.lines.iter().any(|line| line.text == "Body 6")
        })
        .expect("a non-final page should repeat the header");
    let red_horizontal = repeated_page.rects.iter().any(|rect| {
        rect.fill == Some(Color::new(255, 0, 0)) && rect.width > 20.0 && rect.height >= 7.9
    });
    let blue_horizontal = repeated_page.rects.iter().any(|rect| {
        rect.fill == Some(Color::new(0, 0, 255))
            && rect.width > 20.0
            && (rect.height - 2.0).abs() < 0.01
    });

    assert!(
        blue_horizontal,
        "repeated header should paint the original header/body boundary winner"
    );
    assert!(
        !red_horizontal,
        "repeated header must not synthesize the table bottom border at its fragment bottom"
    );
}

#[tokio::test]
async fn fragmented_collapsed_table_body_keeps_rowspan_border_candidates() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         td { padding: 0; text-align: left; height: 35pt; border: 0 }\
         .span { width: 20pt; border-left: 4pt solid blue; border-right: 4pt solid blue }\
         .normal { width: 60pt }</style>\
         <table><tbody>\
         <tr><td class=\"span\" rowspan=\"2\">Span</td><td class=\"normal\">A</td></tr>\
         <tr><td class=\"normal\">B</td></tr>\
         <tr><td colspan=\"2\">C</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let continuation_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines.iter().any(|line| line.text == "B"))
        .expect("rowspan continuation row should fragment to a later page");
    let blue_vertical_edges = continuation_page
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 0, 255))
                && (rect.width - 4.0).abs() < 0.01
                && rect.height > 0.0
        })
        .count();

    assert!(
        blue_vertical_edges >= 2,
        "rowspan cell borders should contribute to the later page fragment"
    );
}

#[tokio::test]
async fn fragmented_collapsed_table_body_keeps_rowspan_candidates_across_collapsed_rows() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         td { padding: 0; text-align: left; height: 35pt; border: 0 }\
         .span { width: 20pt; border-left: 4pt solid blue; border-right: 4pt solid blue }\
         .normal { width: 60pt }</style>\
         <table><tbody>\
         <tr><td class=\"span\" rowspan=\"3\">Span</td><td class=\"normal\">A</td></tr>\
         <tr style=\"visibility:collapse\"><td class=\"normal\">Hidden</td></tr>\
         <tr><td class=\"normal\">B</td></tr>\
         <tr><td colspan=\"2\">C</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let continuation_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines.iter().any(|line| line.text == "B"))
        .expect("visible row after a collapsed track should fragment to a later page");
    assert!(
        !continuation_page
            .lines
            .iter()
            .any(|line| line.text.contains("Hidden")),
        "collapsed row content should stay suppressed"
    );
    let blue_vertical_edges = continuation_page
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 0, 255))
                && (rect.width - 4.0).abs() < 0.01
                && rect.height > 0.0
        })
        .count();

    assert!(
        blue_vertical_edges >= 2,
        "rowspan cell borders should survive a collapsed track inside the fragmented span"
    );
}

#[tokio::test]
async fn oversized_table_row_splits_into_durable_body_fragments() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         td { padding: 0; text-align: left; width: 80pt; height: 120pt; background: blue; border: 2pt solid red }</style>\
         <table><tbody><tr><td>Tall row</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let painted_fragments = document
        .pages
        .iter()
        .filter(|page| {
            page.rects.iter().any(|rect| {
                rect.fill == Some(Color::new(0, 0, 255)) && rect.width > 0.0 && rect.height > 0.0
            })
        })
        .collect::<Vec<_>>();

    assert!(
        painted_fragments.len() >= 3,
        "120pt row should split across multiple 50pt page areas"
    );
    for page in painted_fragments {
        assert!(
            page.rects.iter().any(|rect| {
                rect.fill == Some(Color::new(255, 0, 0)) && rect.width > 0.0 && rect.height > 0.0
            }),
            "each oversized row fragment should own collapsed border paint"
        );
    }
}

#[tokio::test]
async fn oversized_collapsed_row_pieces_do_not_synthesize_horizontal_slice_borders() {
    let document = Html::from_string(
        "<style>@page{size:120pt 70pt;margin:10pt}body{margin:0}table{margin:0;border-collapse:collapse;width:80pt}td{padding:0;text-align:left;width:80pt;height:120pt;background:blue;border:4pt solid red}</style>\
         <table><tr><td>Tall row</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let row_piece_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects.iter().any(|rect| {
                rect.fill == Some(Color::new(0, 0, 255)) && rect.width > 0.0 && rect.height > 0.0
            })
        })
        .collect::<Vec<_>>();
    assert!(row_piece_pages.len() >= 3);

    let middle_page = row_piece_pages[1];
    let synthetic_horizontal = middle_page.rects.iter().any(|rect| {
        rect.fill == Some(Color::new(255, 0, 0))
            && rect.width > 40.0
            && (rect.height - 4.0).abs() < 0.01
    });
    let vertical_edges = middle_page
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(255, 0, 0))
                && (rect.width - 4.0).abs() < 0.01
                && rect.height > 0.0
        })
        .count();

    assert!(
        !synthetic_horizontal,
        "middle oversized row piece must not paint horizontal borders at artificial slice boundaries"
    );
    assert!(
        vertical_edges >= 2,
        "middle oversized row piece should still paint real vertical collapsed borders"
    );
}

#[tokio::test]
async fn repeated_header_wraps_oversized_table_row_fragments() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         th, td { padding: 0; text-align: left; width: 80pt; border: 1pt solid black }\
         th { height: 10pt } td { height: 120pt; background: blue }</style>\
         <table><thead><tr><th>Head</th></tr></thead><tbody><tr><td>Tall row</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let body_fragment_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects.iter().any(|rect| {
                rect.fill == Some(Color::new(0, 0, 255)) && rect.width > 0.0 && rect.height > 0.0
            })
        })
        .collect::<Vec<_>>();

    assert!(body_fragment_pages.len() >= 3);
    for page in body_fragment_pages {
        assert!(
            page.lines.iter().any(|line| line.text == "Head"),
            "repeated header should replay before each oversized body fragment"
        );
    }
}

#[tokio::test]
async fn oversized_table_rowspan_fragments_keep_collapsed_border_candidates() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         td { padding: 0; text-align: left; border: 0 }\
         .span { width: 20pt; border-left: 4pt solid green; border-right: 4pt solid green }\
         .tall { width: 60pt; height: 120pt; background: blue }</style>\
         <table><tbody>\
         <tr><td class=\"span\" rowspan=\"2\">Span</td><td class=\"tall\">Tall row</td></tr>\
         <tr><td>After</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let oversized_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects.iter().any(|rect| {
                rect.fill == Some(Color::new(0, 0, 255)) && rect.width > 0.0 && rect.height > 0.0
            })
        })
        .collect::<Vec<_>>();

    assert!(oversized_pages.len() >= 3);
    for page in oversized_pages {
        let green_edges = page
            .rects
            .iter()
            .filter(|rect| {
                rect.fill == Some(Color::new(0, 128, 0))
                    && (rect.width - 4.0).abs() < 0.01
                    && rect.height > 0.0
            })
            .count();
        assert!(
            green_edges >= 2,
            "rowspanning cell border candidates should survive each oversized row piece"
        );
    }
}

#[tokio::test]
async fn oversized_table_row_fragments_block_children_per_piece() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, div { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 80pt; border-collapse: collapse } td { width: 80pt; height: 120pt; padding: 0 }\
         div { display: block; height: 40pt } .a { background: red } .b { background: green } .c { background: blue }</style>\
         <table><tbody><tr><td><div class=\"a\">A</div><div class=\"b\">B</div><div class=\"c\">C</div></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter().map(|line| line.text.as_str()))
        .collect::<Vec<_>>();
    assert!(texts.contains(&"A"));
    assert!(texts.contains(&"B"));
    assert!(texts.contains(&"C"));

    let painted_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects.iter().any(|rect| {
                matches!(
                    rect.fill,
                    Some(color)
                        if color == Color::new(255, 0, 0)
                            || color == Color::new(0, 128, 0)
                            || color == Color::new(0, 0, 255)
                )
            })
        })
        .count();
    assert!(
        painted_pages >= 3,
        "block child backgrounds should be captured in page-local row pieces"
    );
}

#[tokio::test]
async fn oversized_table_row_fragments_block_child_links_per_piece() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, a { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 80pt; border-collapse: collapse } td { width: 80pt; height: 120pt; padding: 0 }\
         a { display: block; height: 80pt; color: black }</style>\
         <table><tbody><tr><td><a href=\"https://example.com\">Linked block</a></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages.iter().any(|page| {
            page.links
                .iter()
                .any(|link| link.target == "https://example.com")
        }),
        "linked block child inside an oversized table row should emit page-local annotations"
    );
}

#[tokio::test]
async fn oversized_table_row_fragments_inline_block_atoms_per_piece() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, span { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 80pt; border-collapse: collapse } td { width: 80pt; height: 120pt; padding: 0 }\
         span { display: inline-block; width: 40pt; height: 80pt; background: green; color: black }</style>\
         <table><tbody><tr><td><span>Atom</span></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines.iter().map(|line| line.text.as_str()))
            .any(|text| text == "Atom"),
        "inline-block text should be owned by the split table-cell atom"
    );
    let green_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects.iter().any(|rect| {
                rect.fill == Some(Color::new(0, 128, 0)) && rect.width > 0.0 && rect.height > 0.0
            })
        })
        .count();
    assert!(
        green_pages >= 2,
        "inline-block background should be clipped into each visible row piece"
    );
}

#[tokio::test]
async fn oversized_table_row_fragments_nested_inline_block_atoms_from_plan() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, span { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 80pt; border-collapse: collapse } td { width: 80pt; height: 120pt; padding: 0 }\
         .outer { color: black } .atom { display: inline-block; width: 40pt; height: 80pt; background: green }</style>\
         <table><tbody><tr><td><span class=\"outer\"><span class=\"atom\">Nested</span></span></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines.iter().map(|line| line.text.as_str()))
            .any(|text| text == "Nested"),
        "nested inline-block text should be painted through the planned child fragment path"
    );
    let green_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects.iter().any(|rect| {
                rect.fill == Some(Color::new(0, 128, 0)) && rect.width > 0.0 && rect.height > 0.0
            })
        })
        .count();
    assert!(
        green_pages >= 2,
        "nested inline-block background should be clipped into visible row pieces"
    );
}

#[tokio::test]
async fn oversized_table_row_clips_replaced_children_per_piece() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { width: 80pt; border-collapse: collapse } td { width: 80pt; height: 120pt; padding: 0 }</style>\
         <table><tbody><tr><td><svg width=\"30\" height=\"90\"><rect width=\"30\" height=\"90\" fill=\"blue\"/></svg></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let pages_with_svg_paint = document
        .pages
        .iter()
        .filter(|page| {
            page.rects.iter().any(|rect| {
                rect.fill == Some(Color::new(0, 0, 255)) && rect.width > 0.0 && rect.height > 0.0
            })
        })
        .count();
    assert!(
        pages_with_svg_paint >= 2,
        "replaced SVG child should be clipped into visible oversized row pieces; got {pages_with_svg_paint}"
    );
}

#[tokio::test]
async fn fragmented_table_body_preserves_structural_background_order_per_page() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 60pt; background: red } colgroup { background: green }\
         tbody { background: blue } tbody tr { background: yellow }\
         td { padding: 0; text-align: left; width: 60pt; height: 30pt; background: #00ffff }</style>\
         <table><colgroup><col></colgroup><tbody>\
         <tr><td>Body 1</td></tr><tr><td>Body 2</td></tr><tr><td>Body 3</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let body_pages = document
        .pages
        .iter()
        .filter(|page| page.lines.iter().any(|line| line.text.starts_with("Body ")))
        .collect::<Vec<_>>();
    assert!(body_pages.len() >= 2);

    for page in body_pages {
        let table = first_rect_paint_operation_index(page, Color::new(255, 0, 0));
        let column = first_rect_paint_operation_index(page, Color::new(0, 128, 0));
        let row_group = first_rect_paint_operation_index(page, Color::new(0, 0, 255));
        let row = first_rect_paint_operation_index(page, Color::new(255, 255, 0));
        let cell = first_rect_paint_operation_index(page, Color::new(0, 255, 255));

        assert!(table < column);
        assert!(column < row_group);
        assert!(row_group < row);
        assert!(row < cell);
    }
}

#[tokio::test]
async fn fragmented_table_body_traps_positioned_descendants_in_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt }\
         body, table { margin: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 60pt; background: white } td { padding: 0; text-align: left; width: 60pt; height: 40pt; background: blue; position: relative }\
         span { position: absolute; z-index: -1; left: 10pt; top: 0; width: 20pt; height: 20pt; background: red }</style>\
         <table><tbody>\
         <tr><td>Body 1</td></tr><tr><td>Body 2<span></span></td></tr><tr><td>Body 3</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let positioned_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines.iter().any(|line| line.text == "Body 2"))
        .expect("second body row should fragment to a later page");
    let positioned = first_rect_paint_operation_index(positioned_page, Color::new(255, 0, 0));
    let cell = first_rect_paint_operation_index(positioned_page, Color::new(0, 0, 255));

    assert!(
        positioned < cell,
        "negative z-index child should remain inside the table fragment ordering"
    );
}

#[tokio::test]
async fn separated_table_row_scope_preserves_order_before_following_block() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body, table, td, div { margin: 0; padding: 0; border-spacing: 0 }\
         td { width: 40pt; height: 20pt; background: red } div { width: 40pt; height: 20pt; background: blue }</style>\
         <table><tr><td>A</td></tr></table><div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = first_rect_paint_operation_index(page, Color::new(255, 0, 0));
    let blue = first_rect_paint_operation_index(page, Color::new(0, 0, 255));
    assert!(
        red < blue,
        "table row paint should precede following block paint"
    );
}

#[tokio::test]
async fn auto_table_width_distribution_uses_available_width_for_undesignated_columns() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 180pt; margin: 20pt } body { margin: 0; font-size: 10pt; line-height: 12pt } table { border-collapse: collapse; width: 300pt; margin: 0 } th, td { padding: 0; text-align: left }</style>\
         <table><tr><th>Due by</th><th>Account number</th><th>Total due</th></tr><tr><td>May 10, 2018</td><td>1234-5678-90</td><td>$1,800.00</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "May 10, 2018")
    );
    assert!(
        document.pages[0]
            .lines
            .iter()
            .any(|line| line.text == "1234-5678-90")
    );
}

#[tokio::test]
async fn ua_table_box_sizing_border_box_keeps_border_and_padding_inside_width() {
    let document = Html::from_string(
        "<style>body { margin: 0 } table { margin: 0; width: 60pt; border: 5pt solid transparent; padding: 5pt; background: black; border-spacing: 0 } td { padding: 0 }</style>\
         <table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();

    assert!(
        (background.width - 60.0).abs() < 0.01,
        "table border-box wrapper width should be 60pt, got {background:?}"
    );
}

#[tokio::test]
async fn collapsed_table_decorative_borders_do_not_shrink_grid_width() {
    let document = Html::from_string(
        "<style>body { margin: 0 } table { border-collapse: collapse; margin: 0; width: 100pt; border-left: 30pt solid transparent; border-right: 30pt solid transparent; border-spacing: 0; font-size: 10pt; line-height: 10pt } td { padding: 0 }</style>\
         <table><tr><td style=\"width:50pt\">Left side</td><td style=\"width:50pt\">Right side</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let left = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Left side")
        .unwrap();
    let right = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Right side")
        .unwrap();

    assert!(
        right.x - left.x > 40.0,
        "collapsed table borders should not consume column grid width: left={left:?} right={right:?}"
    );
}

#[tokio::test]
async fn collapsed_table_wrapper_inline_insets_use_first_displayed_row() {
    let document = Html::from_string(
        "<style>@page{size:180pt 140pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;width:40pt;background:green;font-size:0;line-height:0}td{padding:0;width:20pt;height:10pt}.first td:first-child{border-left:2pt solid black}.first td:last-child{border-right:2pt solid black}.wide td:first-child{border-left:30pt solid red}.wide td:last-child{border-right:30pt solid red}</style>\
         <table><tr class=\"first\"><td></td><td></td></tr><tr class=\"wide\"><td></td><td></td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("collapsed table background should paint");
    let wide_red_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)) && rect.width >= 29.9)
        .count();

    assert!(
        (green.width - 42.0).abs() < 0.01,
        "wrapper background should use first-row outer insets, got {green:?}"
    );
    assert!(
        wide_red_edges >= 2,
        "later wide collapsed borders should still paint at their row edges"
    );
}

#[tokio::test]
async fn collapsed_table_wrapper_block_insets_use_top_and_bottom_grid_edges() {
    let document = Html::from_string(
        "<style>@page{size:180pt 140pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;width:40pt;background:green;font-size:0;line-height:0}tbody{border-top:20pt solid blue;border-bottom:12pt solid red}td{padding:0;width:20pt;height:10pt}</style>\
         <table><tbody><tr><td></td><td></td></tr><tr><td></td><td></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("collapsed table background should paint");

    assert!(
        (green.height - 36.0).abs() < 0.01,
        "wrapper background should include half of widest top/bottom grid-edge borders, got {green:?}"
    );
}

#[tokio::test]
async fn invoice_sample_generated_metadata_terms_do_not_wrap() {
    let document = Html::from_file_async("weasyprint-samples/invoice/invoice.html")
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    let invoice_label = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Invoice number:" && line.x > 300.0)
        .unwrap();
    let invoice_value = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "12345")
        .unwrap();
    let date_label = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.starts_with("Date"))
        .unwrap();
    let date_value = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "March 31, 2018")
        .unwrap();

    assert!(
        (invoice_label.y - invoice_value.y).abs() < 0.1,
        "invoice number should share one generated-content line"
    );
    assert!(
        (date_label.y - date_value.y).abs() < 0.1,
        "invoice date should share one generated-content line"
    );
    assert!(invoice_label.x < invoice_value.x);
    assert!(date_label.x < date_value.x);
}

#[tokio::test]
async fn table_cells_honor_nested_of_type_text_alignment_rules() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } table { border-collapse: collapse; width: 180pt; margin: 0 } td { padding: 0; text-align: center; &:first-of-type { text-align: left } &:last-of-type { text-align: right } }</style>\
         <table><tr><td>Left</td><td>Middle</td><td>Right</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let left = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Left")
        .unwrap();
    let middle = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Middle")
        .unwrap();
    let right = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Right")
        .unwrap();

    assert!(left.x < middle.x);
    assert!(middle.x < right.x);
    assert!(right.x > 160.0, "right-aligned cell at x={}", right.x);
}

#[tokio::test]
async fn table_cells_match_table_descendant_of_type_selectors() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } table { border-collapse: collapse; width: 180pt; margin: 0 } td { padding: 0; text-align: left } table td:last-of-type { text-align: right; color: #1ee494; font-weight: bold }</style>\
         <table><tr><td>Left</td><td>Right</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let right = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Right")
        .unwrap();

    assert_eq!(right.color, Color::new(30, 228, 148));
    assert!(right.x > 150.0);
}

#[tokio::test]
async fn inline_table_participates_as_atomic_inline_box() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin:0; font-size:10pt; line-height:12pt } table { display:inline-table; border-spacing:0; margin:0 4pt; width:30pt } td { padding:0 }</style>\
         <p>Before <table><tr><td>Cell</td></tr></table> After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Before")
        .unwrap();
    let cell = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!(before.x < cell.x);
    assert!(cell.x < after.x);
    assert!((before.y - after.y).abs() < 0.1);
}

#[tokio::test]
async fn auto_inline_table_uses_fragment_intrinsic_width() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body, p, table { margin:0; font-size:10pt; line-height:12pt } table { display:inline-table; border-spacing:0 } td { padding:0 } .wide { width:80pt } .narrow { width:40pt }</style>\
         <p>A<table><tr><td class=\"wide\">Wide</td><td class=\"narrow\">Cell</td></tr></table>Z</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let wide = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Wide")
        .unwrap();
    let cell = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Z")
        .unwrap();

    assert!(
        cell.x - wide.x > 75.0,
        "second cell should use first column's intrinsic table width: wide={wide:?}, cell={cell:?}"
    );
    assert!(
        after.x - wide.x > 115.0,
        "trailing inline text should follow the auto inline-table width: wide={wide:?}, after={after:?}"
    );
}

#[tokio::test]
async fn inline_table_fragment_intrinsics_follow_visual_row_order() {
    let document = Html::from_string(
        "<style>@page { size: 300pt 140pt; margin: 10pt } body, p, table { margin:0; font-size:10pt; line-height:20pt } table { display:inline-table; border-spacing:0 } th, td { padding:0; text-align:left } thead td, thead th { width:90pt; font-size:16pt; line-height:20pt } tbody td { width:20pt }</style>\
         <p>Before <table><tfoot><tr><td>Foot</td></tr></tfoot><tbody><tr><td>Body</td></tr></tbody><thead><tr><th>Head</th></tr></thead></table> After</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Before")
        .unwrap();
    let head = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Head")
        .unwrap();
    let body = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Body")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!(head.y > body.y, "header row should be laid out before body");
    assert!(
        after.x - head.x > 85.0,
        "trailing text should follow the visual header row's table width"
    );
    assert!(
        (rendered_line_baseline_top(&document, before)
            - rendered_line_baseline_top(&document, head))
        .abs()
            < 0.01,
        "inline-table baseline should come from the first visual row"
    );
}

#[tokio::test]
async fn inline_table_uses_first_row_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 120pt; margin: 10pt } body, p { margin:0; font-size:10pt; line-height:30pt } table { display:inline-table; border-spacing:0; margin:0; width:30pt } td { padding:0;font-size:20pt;line-height:20pt;vertical-align:baseline }</style>\
         <p>Before <table><tr><td>Cell</td></tr></table> After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Before")
        .unwrap();
    let cell = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    let before_top = rendered_line_baseline_top(&document, before);
    let cell_top = rendered_line_baseline_top(&document, cell);
    let after_top = rendered_line_baseline_top(&document, after);
    assert!(
        (before_top - cell_top).abs() < 0.01,
        "expected parent text and inline-table first-row baselines to align, before={before_top}, cell={cell_top}"
    );
    assert!(
        (after_top - cell_top).abs() < 0.01,
        "expected trailing text and inline-table first-row baselines to align, after={after_top}, cell={cell_top}"
    );
}

#[tokio::test]
async fn inline_table_baseline_ignores_top_caption() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 160pt; margin: 10pt } body, p { margin:0; font-size:10pt; line-height:34pt } table { display:inline-table; border-spacing:0; margin:0; width:40pt } caption { caption-side:top; font-size:8pt; line-height:8pt } td { padding:0;font-size:20pt;line-height:20pt;vertical-align:baseline }</style>\
         <p>Before <table><caption>Cap</caption><tr><td>Cell</td></tr></table> After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Before")
        .unwrap();
    let caption = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Cap")
        .unwrap();
    let cell = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    let before_top = rendered_line_baseline_top(&document, before);
    let caption_top = rendered_line_baseline_top(&document, caption);
    let cell_top = rendered_line_baseline_top(&document, cell);
    let after_top = rendered_line_baseline_top(&document, after);
    assert!(
        (before_top - cell_top).abs() < 0.01,
        "expected inline-table row baseline to align, before={before_top}, cell={cell_top}"
    );
    assert!(
        (after_top - cell_top).abs() < 0.01,
        "expected trailing text and inline-table row baseline to align, after={after_top}, cell={cell_top}"
    );
    assert!(
        (caption_top - cell_top).abs() > 0.1,
        "inline-table baseline should not use the top caption baseline"
    );
}

#[tokio::test]
async fn two_keyword_inline_table_uses_inline_table_layout() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin:0; font-size:10pt; line-height:12pt } table { display:inline table; border-spacing:0; margin:0 4pt; width:30pt } td { padding:0 }</style>\
         <p>Before <table><tr><td>Cell</td></tr></table> After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Before")
        .unwrap();
    let cell = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!(before.x < cell.x);
    assert!(cell.x < after.x);
    assert!((before.y - after.y).abs() < 0.1);
}

#[tokio::test]
async fn wraps_orphan_table_cells_in_anonymous_table() {
    let document = Html::from_string(
        "<div style=\"margin:0;border-spacing:0\">\
         <span style=\"display:table-cell;width:40pt\">A</span>\
         <span style=\"display:table-cell;width:40pt\">B</span>\
         </div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[0].text, "A");
    assert_eq!(lines[1].text, "B");
    assert!(lines[1].x > lines[0].x);
    assert!((lines[1].y - lines[0].y).abs() < 0.01);
}

#[tokio::test]
async fn wraps_non_row_table_children_in_anonymous_cells() {
    let document = Html::from_string(
        "<div style=\"display:table;margin:0;width:80pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <section>Anon</section><span style=\"display:table-cell\">Cell</span>\
         </div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let anon = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Anon")
        .unwrap();
    let cell = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();

    assert!(anon.x < cell.x);
    assert!(
        (anon.y - cell.y).abs() < 2.0,
        "generated anonymous cell should stay in the same visual row: anon_y={}, cell_y={}",
        anon.y,
        cell.y
    );
}

#[tokio::test]
async fn wraps_non_cell_row_children_in_anonymous_cells() {
    let document = Html::from_string(
        "<div style=\"display:table;margin:0;width:80pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <div style=\"display:table-row\"><b>Anon</b><span style=\"display:table-cell\">Cell</span></div>\
         </div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let anon = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Anon")
        .unwrap();
    let cell = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();

    assert!(anon.x < cell.x);
    assert!(
        (anon.y - cell.y).abs() < 2.0,
        "generated anonymous cell should stay in the same visual row: anon_y={}, cell_y={}",
        anon.y,
        cell.y
    );
}

#[tokio::test]
async fn wraps_table_text_children_in_anonymous_rows_and_cells() {
    let document = Html::from_string(
        "<div style=\"display:table;margin:0;width:80pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         Lead<span style=\"display:table-cell\">Cell</span>\
         </div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lead = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Lead")
        .unwrap();
    let cell = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();

    assert!(lead.x < cell.x);
    assert!((lead.y - cell.y).abs() < 0.01);
}

#[tokio::test]
async fn html_display_table_shrink_wraps_body_inline_blocks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 500pt 500pt; margin: 0 }\
         html { display: table; border: 10px solid green; border-spacing: 0; padding: 0; margin: auto }\
         body { padding: 0; margin: 0 }\
         </style>\
         <div style=\"width:200px;height:300px;background:yellow;display:inline-block\"></div><div style=\"width:80px;height:300px;background:yellow;display:inline-block\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let yellow_rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 255, 0)))
        .collect::<Vec<_>>();
    assert_eq!(yellow_rects.len(), 2, "{yellow_rects:?}");
    let yellow_width = yellow_rects.iter().map(|rect| rect.width).sum::<f32>();
    assert!((yellow_width - 210.0).abs() < 0.5, "{yellow_rects:?}");
    assert!(
        yellow_rects
            .iter()
            .all(|rect| (rect.height - 225.0).abs() < 0.5),
        "{yellow_rects:?}"
    );
    assert!(
        (yellow_rects[0].y - yellow_rects[1].y).abs() < 0.5,
        "{yellow_rects:?}"
    );

    let green_horizontal_borders = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 128, 0)) && rect.height < rect.width)
        .collect::<Vec<_>>();
    assert!(
        green_horizontal_borders
            .iter()
            .any(|rect| (rect.width - 225.0).abs() < 0.5),
        "{green_horizontal_borders:?}"
    );
}

#[tokio::test]
async fn wraps_direct_table_cells_in_anonymous_rows() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:120pt\"><td style=\"border:1pt solid black\">A</td><td>B</td><tr><td>C</td><td>D</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[0].text, "A");
    assert_eq!(lines[1].text, "B");
    assert_eq!(lines[2].text, "C");
    assert_eq!(lines[3].text, "D");
    assert!(lines[1].x > lines[0].x);
    assert!(lines[2].y < lines[0].y);
    assert_eq!(document.pages[0].rects[0].fill, Some(Color::BLACK));
}

#[tokio::test]
async fn lays_out_table_captions_above_and_below_table_grid() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } table { margin: 0; width: 100pt; border-spacing: 0 } caption { margin: 0; line-height: 12pt } .bottom { caption-side: bottom }</style>\
         <table><caption>Top caption</caption><tr><td>A</td></tr></table>\
         <table><caption class=\"bottom\">Bottom caption</caption><tr><td>B</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    let top_caption = lines
        .iter()
        .find(|line| line.text.starts_with("Top"))
        .unwrap();
    let a = lines.iter().find(|line| line.text == "A").unwrap();
    let bottom_caption = lines
        .iter()
        .find(|line| line.text.starts_with("Bottom"))
        .unwrap();
    let b = lines.iter().find(|line| line.text == "B").unwrap();

    assert!(top_caption.y > a.y);
    assert!(bottom_caption.y < b.y);
}

#[tokio::test]
async fn table_rows_with_zero_line_height_advance_for_replaced_content() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin: 0; width: 120pt\">\
         <tr style=\"line-height: 0pt\"><td><svg width=\"15pt\" height=\"15pt\"><rect width=\"15pt\" height=\"15pt\" fill=\"blue\" /></svg></td><td>A</td><td>B</td></tr>\
         <tr style=\"line-height: 0pt\"><td><svg width=\"15pt\" height=\"15pt\"><rect width=\"15pt\" height=\"15pt\" fill=\"blue\" /></svg></td><td>C</td><td>D</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "C", "D"]
    );
    assert!((lines[1].y - lines[0].y).abs() < 0.01);
    assert!((lines[3].y - lines[2].y).abs() < 0.01);
    assert!(lines[2].y < lines[0].y - 1.0);
}

#[tokio::test]
async fn empty_table_cells_do_not_synthesize_line_height() {
    let document =
        Html::from_string("<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-spacing:0\"><tr><td style=\"width:40pt;border:1pt solid black\"></td></tr></table>")
            .render_async(&RenderOptions::default()).await
            .unwrap();

    let heights = vertical_table_border_heights(&document);

    assert_eq!(heights.len(), 2);
    assert!(heights.iter().all(|height| (*height - 2.0).abs() < 0.01));
}

#[tokio::test]
async fn table_row_height_is_a_minimum_for_cell_content() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr style=\"height:30pt\"><td style=\"vertical-align:top\">A</td></tr>\
         <tr><td style=\"vertical-align:top\">B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;

    assert_eq!(lines[0].text, "A");
    assert_eq!(lines[1].text, "B");
    let advance = lines[0].y - lines[1].y;
    assert!(
        (advance - 30.0).abs() < 0.01,
        "expected row text advance to be 30pt, got {advance} from y={} and y={}",
        lines[0].y,
        lines[1].y
    );
}

#[tokio::test]
async fn visibility_collapse_removes_table_row_space() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr><td>A</td></tr>\
         <tr style=\"visibility:collapse\"><td>B</td></tr>\
         <tr><td>C</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "C"]
    );
    assert!((lines[0].y - lines[1].y - 10.0).abs() < 0.01);
}

#[tokio::test]
async fn visibility_collapse_clips_rowspan_cell_content_for_collapsed_tracks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         table { border-collapse: collapse; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         td { border: 1pt solid blue; padding: 4pt }\
         p { margin: 0 }\
         </style>\
         <table>\
         <tr><td>A1</td><td>B1</td><td rowspan=\"2\" style=\"width:75pt\"><p>Top</p><p>Hidden row text</p></td></tr>\
         <tr style=\"visibility:collapse\"><td>A2</td><td>B2</td></tr>\
         <tr><td>A3</td><td>B3</td><td>C3</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        texts.iter().any(|text| text.contains("Top")),
        "visible rowspan text should remain: {texts:?}"
    );
    assert!(texts.contains(&"C3"));
    assert!(
        !texts.iter().any(|text| text.contains("Hidden row text")),
        "rowspan content belonging to the collapsed row track should be clipped: {texts:?}"
    );
}

#[tokio::test]
async fn collapsed_rows_do_not_add_extra_estimated_border_spacing() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 160pt; margin: 10pt } body { margin:0 } table { break-inside:avoid; margin:0; width:40pt; border-spacing:0 30pt; font-size:10pt; line-height:10pt } td { padding:0 }</style>\
         <div style=\"height:20pt\"></div>\
         <table><tr><td>A</td></tr><tr style=\"visibility:collapse\"><td>B</td></tr><tr><td>C</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(
        document.pages[0]
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "C"]
    );
}

#[tokio::test]
async fn separated_table_background_includes_vertical_edge_spacing() {
    let document = Html::from_string(
        "<style>body{margin:0} table{margin:0;width:20pt;border-spacing:0 6pt;background:black;font-size:10pt;line-height:10pt}td{padding:0}</style>\
         <table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .expect("table background should paint");

    assert!(
        (background.height - 22.0).abs() < 0.01,
        "separated table background should include top and bottom vertical border-spacing, got {background:?}"
    );
}

#[tokio::test]
async fn definite_table_height_distributes_extra_height_to_row_groups() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;background:green;height:100px}td{padding:0}</style>\
         <table><thead><tr><td><div style=\"display:inline-block;width:100px\"></div></td></tr></thead></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("definite-height table should paint its background");

    assert!((green.width - 75.0).abs() < 0.01, "{green:?}");
    assert!((green.height - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn definite_table_height_distributes_extra_height_across_row_groups() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0}td div{width:100px;height:10px}thead{background:red}tbody{background:blue}</style>\
         <table><thead><tr><td><div></div></td></tr></thead><tbody><tr><td><div></div></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("first row group background should paint");
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("second row group background should paint");

    assert!((red.height - 37.5).abs() < 0.01, "{red:?}");
    assert!((blue.height - 37.5).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn explicit_row_group_height_expands_auto_table_group_only() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse}td{padding:0}td div{width:40px;height:10px}.a{height:80px;background:red}.b{background:blue}</style>\
         <table><tbody class=\"a\"><tr><td><div></div></td></tr></tbody><tbody class=\"b\"><tr><td><div></div></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("first row group background should paint");
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("second row group background should paint");

    assert!((red.height - 60.0).abs() < 0.01, "{red:?}");
    assert!((blue.height - 7.5).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn table_height_interpolates_between_base_and_percentage_reference_rows() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:80px}td{padding:0}td div{width:40px;height:10px}.a{height:100%;background:red}.b{background:blue}</style>\
         <table><tr class=\"a\"><td><div></div></td></tr><tr class=\"b\"><td><div></div></td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("percentage row background should paint");
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("auto row background should paint");

    assert!((red.height - 52.5).abs() < 0.01, "{red:?}");
    assert!((blue.height - 7.5).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn percentage_sizing_of_table_cell_and_row_group_uses_definite_table_height() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0}td div{width:40px;height:10px}.a{height:50%;background:red}.b{height:30px;background:blue}.cell{height:60%}</style>\
         <table><tbody class=\"a\"><tr><td class=\"cell\"><div></div></td></tr></tbody><tbody class=\"b\"><tr><td><div></div></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("percentage row group background should paint");
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("explicit row group background should paint");

    assert!((red.height - 48.75).abs() < 0.01, "{red:?}");
    assert!((blue.height - 26.25).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn extra_table_height_goes_to_auto_rows_after_reference_rows() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0}td div{width:40px;height:10px}.a{height:60px;background:red}.b{background:blue}</style>\
         <table><tr class=\"a\"><td><div></div></td></tr><tr class=\"b\"><td><div></div></td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("explicit row background should paint");
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("auto row background should paint");

    assert!((red.height - 45.0).abs() < 0.01, "{red:?}");
    assert!((blue.height - 30.0).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn rowspan_height_constraint_grows_auto_rows_before_explicit_rows() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse}td{padding:0}.a{height:60px;background:red}.b{background:blue}.short{width:20px;height:10px}.span{width:20px;height:90px}</style>\
         <table><tr class=\"a\"><td rowspan=\"2\"><div class=\"span\"></div></td><td><div class=\"short\"></div></td></tr><tr class=\"b\"><td><div class=\"short\"></div></td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .expect("explicit row background should paint");
    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("auto row background should paint");

    assert!((red.height - 45.0).abs() < 0.01, "{red:?}");
    assert!((blue.height - 22.5).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn percent_height_table_cell_child_uses_final_cell_content_height() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0}.child{width:20px;height:100%;background:green}</style>\
         <table><tr><td><div class=\"child\"></div></td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("percentage-height child should paint");

    assert!((green.height - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn fragmented_table_uses_distributed_final_row_heights() {
    let document = Html::from_string(
        "<style>@page{size:100pt 100pt;margin:10pt}body{margin:0}table{margin:0;border-collapse:collapse;height:160pt;font-size:10pt;line-height:10pt}td{padding:0}</style>\
         <table><tr><td>A</td></tr><tr><td>B</td></tr><tr><td>C</td></tr><tr><td>D</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let a = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "B")
        .unwrap();
    let c = document.pages[1]
        .lines
        .iter()
        .find(|line| line.text == "C")
        .unwrap();
    let d = document.pages[1]
        .lines
        .iter()
        .find(|line| line.text == "D")
        .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!((a.y - b.y - 40.0).abs() < 0.01, "a={a:?} b={b:?}");
    assert!((c.y - d.y - 40.0).abs() < 0.01, "c={c:?} d={d:?}");
}

#[tokio::test]
async fn display_contents_inside_table_preserves_child_styles_and_fixup() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}</style>\
         <div style=\"display:table;font:25px/1 Ahem;color:red\"><div style=\"display:contents;color:green\">X<div style=\"display:table-cell\">X</div>X<div style=\"display:table-row\">X</div>X</div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines
        .iter()
        .map(|line| {
            (
                line.text.as_str(),
                line.color,
                line.font_size,
                line.x,
                line.y,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(lines.iter().all(|line| line.0 == "X"));
    assert!(lines.iter().all(|line| line.1 == Color::new(0, 128, 0)));
    assert!(lines.iter().all(|line| (line.2 - 18.75).abs() < 0.01));
    let mut row_counts = Vec::new();
    for line in lines {
        if row_counts
            .last()
            .is_some_and(|(y, _): &(f32, usize)| (y - line.4).abs() < 0.01)
        {
            row_counts.last_mut().unwrap().1 += 1;
        } else {
            row_counts.push((line.4, 1));
        }
    }
    assert_eq!(
        row_counts
            .iter()
            .map(|(_, count)| *count)
            .collect::<Vec<_>>(),
        vec![3, 1, 1]
    );
    assert!(row_counts[0].0 > row_counts[1].0);
    assert!(row_counts[1].0 > row_counts[2].0);
}

#[tokio::test]
async fn visibility_collapse_removes_table_column_space() {
    let document = Html::from_string(
        "<style>body { margin: 0 }</style>\
         <table cellpadding=\"0\" style=\"margin:0;width:80pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <col style=\"visibility:collapse;width:40pt\"><col style=\"width:40pt\">\
         <tr><td>A</td><td>B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["B"]
    );
    assert!(lines[0].x < RenderOptions::default().page_margins.left + 1.0);
}

#[tokio::test]
async fn visibility_collapse_removes_table_column_group_span_space() {
    let document = Html::from_string(
        "<style>body { margin: 0 }</style>\
         <table cellpadding=\"0\" style=\"margin:0;width:120pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <colgroup span=\"2\" style=\"visibility:collapse;width:40pt\"></colgroup><col style=\"width:40pt\">\
         <tr><td>A</td><td>B</td><td>C</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["C"]
    );
    assert!(lines[0].x < RenderOptions::default().page_margins.left + 1.0);
}

#[tokio::test]
async fn collapsed_table_column_group_overrides_visible_child_columns() {
    let document = Html::from_string(
        "<style>body { margin: 0 }</style>\
         <table cellpadding=\"0\" style=\"margin:0;width:120pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <colgroup style=\"visibility:collapse\"><col style=\"visibility:visible;width:40pt\"><col style=\"visibility:visible;width:40pt\"></colgroup><col style=\"width:40pt\">\
         <tr><td>A</td><td>B</td><td>C</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["C"]
    );
    assert!(lines[0].x < RenderOptions::default().page_margins.left + 1.0);
}

#[tokio::test]
async fn table_rows_inherit_from_row_groups() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tbody style=\"visibility:collapse;color:red\"><tr><td>A</td></tr></tbody>\
         <tbody style=\"color:blue\"><tr><td>B</td></tr></tbody>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "B");
    assert_eq!(lines[0].color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn empty_cells_hide_suppresses_empty_cell_backgrounds_and_borders() {
    let show = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:80pt;border-spacing:0;empty-cells:show\">\
         <tr><td style=\"width:40pt;border:2pt solid black\"></td><td style=\"width:40pt;border:2pt solid black\">X</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let hide = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:80pt;border-spacing:0;empty-cells:hide\">\
         <tr><td style=\"width:40pt;border:2pt solid black\"></td><td style=\"width:40pt;border:2pt solid black\">X</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let show_black_rects = show.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::BLACK))
        .count();
    let hide_black_rects = hide.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::BLACK))
        .count();

    assert_eq!(hide.pages[0].lines[0].text, "X");
    assert!(hide_black_rects < show_black_rects);
}

#[tokio::test]
async fn empty_cells_hide_makes_all_empty_row_zero_height_with_single_spacing() {
    let document = Html::from_string(
        "<style>body { margin:0 } table { margin:0; border-spacing:0 8pt; empty-cells:hide; font-size:10pt; line-height:10pt } td { padding:0 }</style>\
         <table>\
         <tr><td>A</td></tr>\
         <tr><td style=\"padding:8pt;border:2pt solid black;background:black\"></td></tr>\
         <tr><td>C</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "C"]
    );
    assert!(
        (lines[0].y - lines[1].y - 18.0).abs() < 0.01,
        "hidden empty row should leave only one vertical spacing side: lines={lines:?}"
    );
    assert!(
        document.pages[0]
            .rects
            .iter()
            .all(|rect| rect.fill != Some(Color::BLACK)),
        "hidden empty cell background/border should not paint"
    );
}

#[tokio::test]
async fn aligns_table_cell_content_vertically_inside_row_height() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr style=\"height:40pt\"><td style=\"width:30pt;vertical-align:top\">Top</td><td style=\"width:30pt;vertical-align:middle\">Mid</td><td style=\"width:30pt;vertical-align:bottom\">Bot</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let top = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Top")
        .unwrap();
    let middle = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Mid")
        .unwrap();
    let bottom = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Bot")
        .unwrap();

    assert!(top.y > middle.y);
    assert!(middle.y > bottom.y);
    assert!((top.y - middle.y - 15.0).abs() < 0.01);
    assert!((middle.y - bottom.y - 15.0).abs() < 0.01);
}

#[tokio::test]
async fn aligns_table_cell_text_on_explicit_baseline() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:80pt;border-spacing:0;line-height:20pt\">\
         <tr><td style=\"width:40pt;font-size:20pt;vertical-align:baseline\">Big</td><td style=\"width:40pt;font-size:10pt;vertical-align:baseline\">Small</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let big = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Big")
        .unwrap();
    let small = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Small")
        .unwrap();

    assert!(
        (big.y - small.y).abs() < 0.01,
        "expected table-cell baselines to match, got Big y={} and Small y={}",
        big.y,
        small.y
    );
}

#[tokio::test]
async fn aligns_table_cell_multiline_content_on_first_baseline() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:100pt;border-spacing:0\">\
         <tr><td style=\"width:50pt;font-size:10pt;line-height:10pt;vertical-align:baseline\">First<br>Second</td><td style=\"width:50pt;font-size:20pt;line-height:20pt;vertical-align:baseline\">Peer</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let first = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "First")
        .unwrap();
    let second = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Second")
        .unwrap();
    let peer = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();

    assert!(
        (first.y - peer.y).abs() < 0.01,
        "expected multiline cell first baseline to align with peer: first={}, peer={}",
        first.y,
        peer.y
    );
    assert!(
        second.y < first.y,
        "second line should remain below the first line after baseline alignment"
    );
}

#[tokio::test]
async fn aligns_table_cell_block_child_text_baseline() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:100pt;border-spacing:0\">\
         <tr><td style=\"width:50pt;vertical-align:baseline\"><div style=\"margin:0;font-size:20pt;line-height:20pt\">Block</div></td><td style=\"width:50pt;font-size:10pt;line-height:10pt;vertical-align:baseline\">Peer</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let block = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Block")
        .unwrap();
    let peer = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();

    assert!(
        (block.y - peer.y).abs() < 0.01,
        "expected block child baseline to align with peer: block={}, peer={}",
        block.y,
        peer.y
    );
}

#[tokio::test]
async fn aligns_table_cell_nested_table_row_baseline() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:110pt;border-spacing:0\">\
         <tr><td style=\"width:60pt;vertical-align:baseline\"><table cellpadding=\"0\" style=\"margin:0;border-spacing:0\"><tr><td style=\"font-size:20pt;line-height:20pt;vertical-align:baseline\">Inner</td></tr></table></td><td style=\"width:50pt;font-size:10pt;line-height:10pt;vertical-align:baseline\">Peer</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let inner = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Inner")
        .unwrap();
    let peer = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();

    assert!(
        (inner.y - peer.y).abs() < 0.01,
        "expected nested table row baseline to align with peer: inner={}, peer={}",
        inner.y,
        peer.y
    );
}

#[tokio::test]
async fn table_cell_baseline_falls_back_to_non_text_content_bottom() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0\">\
         <tr><td style=\"width:30pt;vertical-align:baseline\"><svg width=\"20pt\" height=\"30pt\"><rect width=\"20pt\" height=\"30pt\" fill=\"blue\" /></svg></td><td style=\"width:30pt;padding-top:12pt;padding-bottom:8pt;vertical-align:baseline\"></td><td style=\"width:30pt;font-size:10pt;line-height:10pt;vertical-align:baseline\">Peer</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let peer = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();
    let svg = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert!(
        (peer.y - svg.y).abs() < 0.01,
        "expected text baseline to align with SVG content-bottom fallback: peer={}, svg_bottom={}",
        peer.y,
        svg.y
    );
}

#[tokio::test]
async fn table_cell_inline_vertical_align_keywords_align_as_baseline() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:150pt;border-spacing:0;line-height:20pt\">\
         <tr><td style=\"width:30pt;font-size:20pt;vertical-align:baseline\">Base</td><td style=\"width:30pt;font-size:10pt;vertical-align:text-top\">TextTop</td><td style=\"width:30pt;font-size:10pt;vertical-align:text-bottom\">TextBottom</td><td style=\"width:30pt;font-size:10pt;vertical-align:sub\">Sub</td><td style=\"width:30pt;font-size:10pt;vertical-align:super\">Super</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let baseline = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Base")
        .map(|line| line.y)
        .unwrap();

    for text in ["TextTop", "TextBottom", "Sub", "Super"] {
        let line = document.pages[0]
            .lines
            .iter()
            .find(|line| line.text == text)
            .unwrap();
        let candidate = line.y;
        assert!(
            (candidate - baseline).abs() < 0.01,
            "{text} should align as a table-cell baseline value: candidate={candidate}, baseline={baseline}"
        );
    }
}

#[tokio::test]
async fn table_valign_presentational_hint_aligns_cell_content_when_enabled() {
    let options = RenderOptions {
        presentational_hints: true,
        ..RenderOptions::default()
    };
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:60pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr style=\"height:40pt\"><td valign=\"top\" style=\"width:30pt\">Top</td><td valign=\"bottom\" style=\"width:30pt\">Bottom</td></tr>\
         </table>",
    )
    .render_async(&options).await
    .unwrap();

    let top = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Top")
        .unwrap();
    let bottom = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Bottom")
        .unwrap();

    assert!(top.y > bottom.y);
    assert!((top.y - bottom.y - 30.0).abs() < 0.01);
}

#[tokio::test]
async fn author_css_overrides_table_valign_presentational_hint() {
    let options = RenderOptions {
        presentational_hints: true,
        ..RenderOptions::default()
    };
    let document = Html::from_string(
        "<style>td { vertical-align: top }</style>\
         <table cellpadding=\"0\" style=\"margin:0;width:60pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr style=\"height:40pt\"><td valign=\"bottom\" style=\"width:30pt\">Hint</td><td style=\"width:30pt\">Author</td></tr>\
         </table>",
    )
    .render_async(&options).await
    .unwrap();

    let hinted = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Hint")
        .unwrap();
    let author = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Author")
        .unwrap();

    assert!((hinted.y - author.y).abs() < 0.01);
}

#[tokio::test]
async fn table_rules_groups_presentational_hint_paints_group_borders() {
    let options = RenderOptions {
        presentational_hints: true,
        ..RenderOptions::default()
    };
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 260pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         table { margin: 0 0 10pt 0 }\
         #b > * { border-block-end-color: blue }\
         #c > * { border-block-end-width: 5px }\
         </style>\
         <table id=\"a\" rules=\"groups\"><thead><tr><td>head</td></tr></thead><tbody><tr><td>body</td><td>one</td></tr><tr><td>body</td><td>two</td></tr></tbody><tfoot><tr><td>foot</td></tr></tfoot></table>\
         <table id=\"b\" rules=\"groups\"><thead><tr><td>head</td></tr></thead><tbody><tr><td>body</td><td>one</td></tr><tr><td>body</td><td>two</td></tr></tbody><tfoot><tr><td>foot</td></tr></tfoot></table>\
         <table id=\"c\" rules=\"groups\"><thead><tr><td>head</td></tr></thead><tbody><tr><td>body</td><td>one</td></tr><tr><td>body</td><td>two</td></tr></tbody><tfoot><tr><td>foot</td></tr></tfoot></table>",
    )
    .render_async(&options).await
    .unwrap();

    let thin_gray = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(128, 128, 128)) && rect.height <= 1.0 && rect.width > 10.0
        })
        .count();
    let thin_blue = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 0, 255)) && rect.height <= 1.0 && rect.width > 10.0
        })
        .count();
    let thick_gray = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(128, 128, 128))
                && (rect.height - 3.75).abs() < 0.01
                && rect.width > 10.0
        })
        .count();

    assert!(thin_gray >= 2);
    assert!(thin_blue >= 2);
    assert!(thick_gray >= 2);
}

#[tokio::test]
async fn paginates_table_rows_using_measured_row_height() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table { margin: 0; font-size: 10pt; line-height: 10pt } td { padding: 10pt 0; }</style>\
         <div style=\"height:55pt\"></div>\
         <table><tr><td>Tall</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(document.pages[0].lines.is_empty());
    assert_eq!(document.pages[1].lines[0].text, "Tall");
}

#[tokio::test]
async fn break_inside_avoid_keeps_table_row_groups_together_when_they_fit() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 } tbody { break-inside: avoid }</style>\
         <div style=\"height:55pt\"></div>\
         <table><tbody><tr><td>A</td></tr><tr><td>B</td></tr><tr><td>C</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(document.pages[0].lines.is_empty());
    assert_eq!(
        document.pages[1]
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "C"]
    );
}

#[tokio::test]
async fn repeats_table_header_group_after_page_break() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table, th, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }</style>\
         <table><thead><tr><th>Head</th></tr></thead><tbody>\
         <tr><td>R1</td></tr><tr><td>R2</td></tr><tr><td>R3</td></tr><tr><td>R4</td></tr>\
         <tr><td>R5</td></tr><tr><td>R6</td></tr><tr><td>R7</td></tr><tr><td>R8</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document
            .pages
            .iter()
            .flat_map(|page| &page.lines)
            .filter(|line| line.text == "Head")
            .count(),
        2
    );
    assert_eq!(document.pages[1].lines[0].text, "Head");
    assert_eq!(document.pages[1].lines[1].text, "R8");
}

#[tokio::test]
async fn repeats_authored_table_header_group_after_page_break() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table, tr, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 } .head { display: table-header-group }</style>\
         <table><tbody class=\"head\"><tr><td>Head</td></tr></tbody><tbody>\
         <tr><td>R1</td></tr><tr><td>R2</td></tr><tr><td>R3</td></tr><tr><td>R4</td></tr>\
         <tr><td>R5</td></tr><tr><td>R6</td></tr><tr><td>R7</td></tr><tr><td>R8</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document
            .pages
            .iter()
            .flat_map(|page| &page.lines)
            .filter(|line| line.text == "Head")
            .count(),
        2
    );
    assert_eq!(document.pages[1].lines[0].text, "Head");
}

#[tokio::test]
async fn repeats_table_footer_group_at_fragment_bottom() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }</style>\
         <table><tfoot><tr><td>Foot</td></tr></tfoot><tbody>\
         <tr><td>R1</td></tr><tr><td>R2</td></tr><tr><td>R3</td></tr><tr><td>R4</td></tr>\
         <tr><td>R5</td></tr><tr><td>R6</td></tr><tr><td>R7</td></tr><tr><td>R8</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document
            .pages
            .iter()
            .flat_map(|page| &page.lines)
            .filter(|line| line.text == "Foot")
            .count(),
        2
    );
    assert_eq!(document.pages[0].lines.last().unwrap().text, "Foot");
    assert_eq!(document.pages[1].lines[0].text, "R8");
    assert_eq!(document.pages[1].lines.last().unwrap().text, "Foot");
}

#[tokio::test]
async fn repeats_authored_table_footer_group_at_fragment_bottom() {
    let document = Html::from_string(
        "<style>\
         @page { size: 100pt 100pt; margin: 10pt }\
         body, .table, .row, span { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }\
         .table { display: table }\
         .body { display: table-row-group }\
         .foot { display: table-footer-group }\
         .row { display: table-row }\
         span { display: table-cell }\
         </style>\
         <div class=\"table\"><div class=\"foot\"><div class=\"row\"><span>Foot</span></div></div><div class=\"body\">\
         <div class=\"row\"><span>R1</span></div><div class=\"row\"><span>R2</span></div><div class=\"row\"><span>R3</span></div><div class=\"row\"><span>R4</span></div>\
         <div class=\"row\"><span>R5</span></div><div class=\"row\"><span>R6</span></div><div class=\"row\"><span>R7</span></div><div class=\"row\"><span>R8</span></div>\
         </div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document
            .pages
            .iter()
            .flat_map(|page| &page.lines)
            .filter(|line| line.text == "Foot")
            .count(),
        2
    );
    assert_eq!(document.pages[0].lines.last().unwrap().text, "Foot");
    assert_eq!(document.pages[1].lines.last().unwrap().text, "Foot");
}

#[tokio::test]
async fn table_row_break_before_repeats_header_on_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table, th, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 } .split { break-before: page }</style>\
         <table><thead><tr><th>Head</th></tr></thead><tbody>\
         <tr><td>R1</td></tr><tr class=\"split\"><td>R2</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines[0].text, "Head");
    assert_eq!(document.pages[0].lines[1].text, "R1");
    assert_eq!(document.pages[1].lines[0].text, "Head");
    assert_eq!(document.pages[1].lines[1].text, "R2");
}

#[tokio::test]
async fn table_row_group_break_after_repeats_header_on_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table, th, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 } .first { break-after: page }</style>\
         <table><thead><tr><th>Head</th></tr></thead><tbody class=\"first\">\
         <tr><td>R1</td></tr></tbody><tbody><tr><td>R2</td></tr>\
         </tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines[0].text, "Head");
    assert_eq!(document.pages[0].lines[1].text, "R1");
    assert_eq!(document.pages[1].lines[0].text, "Head");
    assert_eq!(document.pages[1].lines[1].text, "R2");
}

#[tokio::test]
async fn table_row_break_after_avoid_rewinds_to_earlier_row_boundary() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt }\
         body, table, tr, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }\
         td { height: 20pt } .keep { break-after: avoid }</style>\
         <table><tbody><tr><td>R1</td></tr><tr><td>R2</td></tr><tr><td>R3</td></tr><tr class=\"keep\"><td>R4</td></tr><tr><td>R5</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(page_texts.len(), 2, "{page_texts:?}");
    assert_eq!(page_texts[0], vec!["R1", "R2", "R3"]);
    assert_eq!(page_texts[1], vec!["R4", "R5"]);
}

#[tokio::test]
async fn table_row_break_before_avoid_rewinds_to_earlier_row_boundary() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt }\
         body, table, tr, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }\
         td { height: 20pt } .keep { break-before: avoid }</style>\
         <table><tbody><tr><td>R1</td></tr><tr><td>R2</td></tr><tr><td>R3</td></tr><tr><td>R4</td></tr><tr class=\"keep\"><td>R5</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(page_texts.len(), 2, "{page_texts:?}");
    assert_eq!(page_texts[0], vec!["R1", "R2", "R3"]);
    assert_eq!(page_texts[1], vec!["R4", "R5"]);
}

#[tokio::test]
async fn table_row_group_break_after_avoid_rewinds_before_group_boundary() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt }\
         body, table, tr, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }\
         td { height: 20pt } .first { break-after: avoid }</style>\
         <table><tbody class=\"first\"><tr><td>R1</td></tr><tr><td>R2</td></tr><tr><td>R3</td></tr><tr><td>R4</td></tr></tbody><tbody><tr><td>R5</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(page_texts.len(), 2, "{page_texts:?}");
    assert_eq!(page_texts[0], vec!["R1", "R2", "R3"]);
    assert_eq!(page_texts[1], vec!["R4", "R5"]);
}

#[tokio::test]
async fn forced_table_row_break_overrides_previous_avoid() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt }\
         body, table, tr, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }\
         td { height: 20pt } .avoid { break-after: avoid } .forced { break-before: page }</style>\
         <table><tbody><tr class=\"avoid\"><td>R1</td></tr><tr class=\"forced\"><td>R2</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines[0].text, "R1");
    assert_eq!(document.pages[1].lines[0].text, "R2");
}

#[tokio::test]
async fn oversized_table_row_slices_direct_cell_text_from_inline_plan() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 20pt; border-collapse: collapse } td { width: 20pt; height: 200pt }</style>\
         <table><tbody><tr><td>Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliett Kilo Lima Mike November Oscar Papa Quebec Romeo Sierra Tango</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let pages_with_text = document
        .pages
        .iter()
        .filter(|page| !page.lines.is_empty())
        .count();
    assert!(
        pages_with_text >= 2,
        "direct table-cell text should be sliced across oversized row fragments"
    );
}

#[tokio::test]
async fn table_row_break_inside_avoid_moves_fitting_row_before_splitting() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt }\
         body, table, tr, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }\
         td { height: 20pt } .keep { break-inside: avoid } .keep td { height: 40pt }</style>\
         <table><tbody><tr><td>R1</td></tr><tr><td>R2</td></tr><tr><td>R3</td></tr><tr class=\"keep\"><td>R4</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(page_texts.len(), 2, "{page_texts:?}");
    assert_eq!(page_texts[0], vec!["R1", "R2", "R3"]);
    assert_eq!(page_texts[1], vec!["R4"]);
}

#[tokio::test]
async fn table_cell_break_inside_avoid_moves_fitting_row_before_splitting() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt }\
         body, table, tr, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }\
         td { height: 20pt } .keep { break-inside: avoid; height: 40pt }</style>\
         <table><tbody><tr><td>R1</td></tr><tr><td>R2</td></tr><tr><td>R3</td></tr><tr><td class=\"keep\">R4</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(page_texts.len(), 2, "{page_texts:?}");
    assert_eq!(page_texts[0], vec!["R1", "R2", "R3"]);
    assert_eq!(page_texts[1], vec!["R4"]);
}

#[tokio::test]
async fn oversized_table_row_slices_styled_inline_cell_content_from_plan() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, span { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 20pt; border-collapse: collapse } td { width: 20pt; height: 160pt } span { color: red }</style>\
         <table><tbody><tr><td><span>Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliett Kilo Lima</span></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.lines
                .iter()
                .any(|line| line.color == Color::new(255, 0, 0))
        })
        .count();
    assert!(
        red_pages >= 2,
        "styled inline table-cell content should be planned and sliced across row pieces"
    );
}

#[tokio::test]
async fn oversized_table_row_slices_generated_cell_inline_content_from_plan() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 24pt; border-collapse: collapse } td { width: 24pt; height: 160pt }\
         td::before { content: \"Before\"; color: red } td::after { content: \"After\"; color: blue }</style>\
         <table><tbody><tr><td>Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliett Kilo Lima</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter().map(|line| line.text.as_str()))
        .collect::<Vec<_>>();
    assert!(texts.contains(&"Before"), "{texts:?}");
    assert!(texts.contains(&"After"), "{texts:?}");
}

#[tokio::test]
async fn oversized_table_row_clips_nested_table_child_from_plan() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         .outer { width: 80pt; border-collapse: collapse } .outer > tbody > tr > td { width: 80pt; height: 120pt }\
         .inner td { width: 80pt; height: 40pt; background: green }</style>\
         <table class=\"outer\"><tbody><tr><td><table class=\"inner\"><tr><td>A</td></tr><tr><td>B</td></tr><tr><td>C</td></tr></table></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter().map(|line| line.text.as_str()))
        .collect::<Vec<_>>();
    assert!(texts.contains(&"A"), "{texts:?}");
    assert!(texts.contains(&"B"), "{texts:?}");
    assert!(texts.contains(&"C"), "{texts:?}");
}

#[tokio::test]
async fn oversized_table_row_replays_nested_flex_child_from_plan() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, div, span { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 80pt; border-collapse: collapse } td { width: 80pt; height: 120pt }\
         .flex { display: flex; flex-direction: column; position: relative; width: 80pt; height: 120pt; overflow: hidden; opacity: .8; transform: translateX(0) }\
         .flex > div { display: block; height: 40pt } .a { background: red } .b { background: green } .c { background: blue }\
         .pos { position: absolute; left: 0; top: 70pt; color: black }</style>\
         <table><tbody><tr><td><div class=\"flex\"><div class=\"a\">A</div><div class=\"b\">B</div><div class=\"c\">C</div><span class=\"pos\">P</span></div></td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines.iter().map(|line| line.text.as_str()))
        .collect::<Vec<_>>();
    assert!(texts.contains(&"A"), "{texts:?}");
    assert!(texts.contains(&"B"), "{texts:?}");
    assert!(texts.contains(&"C"), "{texts:?}");
    assert!(texts.contains(&"P"), "{texts:?}");
}

#[tokio::test]
async fn oversized_separated_row_pieces_do_not_clone_horizontal_cell_borders() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 80pt; border-collapse: separate; border-spacing: 0 }\
         td { width: 80pt; height: 120pt; border: 4pt solid red; background: blue }</style>\
         <table><tbody><tr><td>Tall row</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let row_piece_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects.iter().any(|rect| {
                rect.fill == Some(Color::new(0, 0, 255)) && rect.width > 0.0 && rect.height > 0.0
            })
        })
        .collect::<Vec<_>>();
    assert!(row_piece_pages.len() >= 3);

    let middle_page = row_piece_pages[1];
    let synthetic_horizontal = middle_page.rects.iter().any(|rect| {
        rect.fill == Some(Color::new(255, 0, 0))
            && rect.width > 40.0
            && (rect.height - 4.0).abs() < 0.01
    });
    let vertical_edges = middle_page
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(255, 0, 0))
                && (rect.width - 4.0).abs() < 0.01
                && rect.height > 0.0
        })
        .count();

    assert!(
        !synthetic_horizontal,
        "middle separated row piece must not clone horizontal borders at artificial slice boundaries"
    );
    assert!(
        vertical_edges >= 2,
        "middle separated row piece should keep real vertical cell borders"
    );
}

#[tokio::test]
async fn does_not_flatten_table_text_into_parent_blocks() {
    let document = Html::from_string(
        "<div style=\"margin: 0\"><table style=\"margin: 0; width: 120pt\"><tr><td>A</td><td>B</td></tr></table></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["A", "B"]);
}

#[tokio::test]
async fn supports_basic_table_colspan() {
    let document = Html::from_string(
        "<table style=\"margin: 0; width: 120pt\"><tr><td colspan=\"2\" style=\"border: 1pt solid black\">Wide</td></tr><tr><td>Left</td><td>Right</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "Wide");
    assert_eq!(document.pages[0].rects[0].width, 120.0);
    assert_eq!(document.pages[0].lines[1].text, "Left");
    assert_eq!(document.pages[0].lines[2].text, "Right");
    assert!(document.pages[0].lines[2].x - document.pages[0].lines[1].x >= 50.0);
}

#[tokio::test]
async fn supports_basic_table_rowspan_occupancy() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr><td rowspan=\"2\" style=\"width:30pt;border:1pt solid black\">Span</td><td style=\"width:60pt;border:1pt solid black\">A</td></tr>\
         <tr><td style=\"border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let span = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Span")
        .unwrap();
    let a = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "B")
        .unwrap();

    assert!(a.x > span.x);
    assert!((a.x - b.x).abs() < 0.01);
    assert!(b.y < a.y);
}

#[tokio::test]
async fn parses_table_span_attributes_with_html_integer_rules() {
    let colspan = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr><td colspan=\"2px\" style=\"border:1pt solid black\">Wide</td></tr>\
         <tr><td>A</td><td>B</td><td>C</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let rowspan = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr><td rowspan=\"2px\" style=\"width:30pt\">Span</td><td>A</td></tr>\
         <tr><td>B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let col_span = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;table-layout:fixed;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <col span=\"2px\" style=\"width:20pt\"><col style=\"width:50pt\">\
         <tr><td>A</td><td>B</td><td>C</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let wide_border = colspan.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK) && rect.width > 50.0)
        .expect("colspan should span two columns");
    assert!(wide_border.width > 50.0);

    let span = rowspan.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Span")
        .unwrap();
    let a = rowspan.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = rowspan.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "B")
        .unwrap();
    assert!(a.x > span.x);
    assert!((a.x - b.x).abs() < 0.01);

    let lines = &col_span.pages[0].lines;
    assert!(((lines[1].x - lines[0].x) - (lines[2].x - lines[1].x)).abs() < 0.01);
}

#[tokio::test]
async fn rowspan_zero_spans_to_end_of_row_group() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tbody><tr><td rowspan=\"0\" style=\"width:30pt;border:1pt solid black\">Span</td><td style=\"width:60pt;border:1pt solid black\">A</td></tr>\
         <tr><td style=\"border:1pt solid black\">B</td></tr></tbody>\
         <tfoot><tr><td style=\"border:1pt solid black\">Foot</td><td style=\"border:1pt solid black\">C</td></tr></tfoot>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let span = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Span")
        .unwrap();
    let a = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "B")
        .unwrap();
    let foot = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "Foot")
        .unwrap();
    let c = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "C")
        .unwrap();

    assert!((a.x - b.x).abs() < 0.01);
    assert!(a.x > span.x);
    assert!((foot.x - span.x).abs() < 0.01);
    assert!(c.x > foot.x);
}

#[tokio::test]
async fn supports_percentage_table_cell_widths() {
    let document = Html::from_string(
        "<table style=\"margin: 0; width: 200pt; border-spacing: 0\"><tr><td style=\"width: 25%; border: 1pt solid black\">A</td><td>B</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].rects[0].width, 50.0);
}

#[tokio::test]
async fn table_columns_do_not_shrink_below_preferred_widths() {
    let document = Html::from_string(
        "<style>@page { size: 500pt 220pt; margin: 20pt } body { margin: 0 }</style>\
         <table style=\"margin:0;width:300pt\"><tr>\
         <td style=\"width:30pt;border:1pt solid black\">S</td>\
         <td style=\"width:195pt;border:1pt solid black\">chr1: 568,526-249,210,706</td>\
         <td style=\"border:1pt solid black;text-align:right\">284.8 cM</td>\
         <td style=\"border:1pt solid black;text-align:right\">17,245 SNPs</td>\
         <td style=\"width:45%;border:1pt solid black\"></td>\
         </tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 5);
    assert!((widths[0] - 30.0).abs() < 0.01);
    assert!((widths[1] - 195.0).abs() < 0.01);
    assert!(widths[4] < 135.0);
    assert!(widths.iter().sum::<f32>() >= 300.0);
}

#[tokio::test]
async fn auto_table_columns_use_intrinsic_content_widths() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:300pt;border-spacing:0\"><tr>\
         <td style=\"border:1pt solid black\">Frequency</td>\
         <td style=\"border:1pt solid black\">Great-Grandparent, Great-Grandchild, Half Aunt / Uncle</td>\
         </tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 2);
    assert!(widths[0] < 100.0);
    assert!(widths[1] > 200.0);
    assert!((widths.iter().sum::<f32>() - 300.0).abs() < 0.01);
}

#[tokio::test]
async fn plans_table_columns_from_fixed_percentage_and_auto_cells() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:500pt;border-spacing:0\"><tr><td style=\"width:30pt;border:1pt solid black\">S</td><td style=\"width:195pt;border:1pt solid black\">Label</td><td style=\"width:45%;border:1pt solid black\">Fill</td><td style=\"border:1pt solid black\">Auto</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 4);
    assert!((widths[0] - 30.0).abs() < 0.01);
    assert!((widths[1] - 195.0).abs() < 0.01);
    assert!((widths[2] - 225.0).abs() < 0.01);
    assert!((widths[3] - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn table_cell_min_width_is_stable_with_long_sibling_content() {
    let style = "<style>table{margin:0;border-spacing:0}td{padding:0;border:1pt solid black}.key{min-width:80pt}</style>";
    let short = Html::from_string(format!(
        "{style}<table><tr><td class=\"key\">Key</td><td>Value</td></tr></table>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let long = Html::from_string(format!(
        "{style}<table><tr><td class=\"key\">Key</td><td>Long value that wraps across the available width and must not change the key column</td></tr></table>"
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let short_widths = horizontal_table_border_widths(&short);
    let long_widths = horizontal_table_border_widths(&long);

    assert!((short_widths[0] - 80.0).abs() < 0.01);
    assert!((long_widths[0] - short_widths[0]).abs() < 0.01);
}

#[tokio::test]
async fn table_cell_max_width_clamps_auto_layout_width() {
    let document = Html::from_string(
        "<style>table{margin:0;border-spacing:0}td{padding:0;border:1pt solid black;max-width:45pt;white-space:nowrap;overflow:hidden}</style>\
         <table><tr><td>abcdefghijklmnopqrstuv</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 1);
    assert!((widths[0] - 45.0).abs() < 0.01);
}

#[tokio::test]
async fn auto_table_mixed_fixed_and_percentage_widths_share_final_width() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:200pt;border-spacing:0\"><tr>\
         <td style=\"width:40pt;border:1pt solid black\">Fixed</td>\
         <td style=\"width:50%;border:1pt solid black\">Half</td>\
         <td style=\"border:1pt solid black\">Auto</td>\
         </tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 3);
    assert!((widths[0] - 40.0).abs() < 0.01);
    assert!((widths[1] - 100.0).abs() < 0.01);
    assert!((widths[2] - 60.0).abs() < 0.01);
}

#[tokio::test]
async fn colspan_percentage_contribution_is_distributed_before_final_widths() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:200pt;border-spacing:0\"><tr>\
         <td colspan=\"2\" style=\"width:75%;border:1pt solid black\">Wide</td><td style=\"border:1pt solid black\">C</td>\
         </tr><tr><td style=\"border:1pt solid black\">A</td><td style=\"border:1pt solid black\">B</td><td style=\"border:1pt solid black\">C</td></tr></table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 5);
    assert!((widths[0] - 150.0).abs() < 0.01);
    assert!((widths[1] - 50.0).abs() < 0.01);
    assert!((widths[2] - 75.0).abs() < 0.01);
    assert!((widths[3] - 75.0).abs() < 0.01);
    assert!((widths[4] - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn fixed_table_layout_resolves_column_and_first_row_percentages() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:200pt;border-spacing:0;table-layout:fixed\">\
         <col style=\"width:25%\"><col><col>\
         <tr><td style=\"border:1pt solid black\">A</td><td style=\"width:50%;border:1pt solid black\">B</td><td style=\"border:1pt solid black\">C</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 3);
    assert!((widths[0] - 50.0).abs() < 0.01);
    assert!((widths[1] - 100.0).abs() < 0.01);
    assert!((widths[2] - 50.0).abs() < 0.01);
}

#[tokio::test]
async fn applies_table_min_and_max_width_before_column_planning() {
    let min_document = Html::from_string(
        "<table style=\"margin:0;width:40pt;min-width:80pt;border-spacing:0\"><tr><td style=\"border:1pt solid black\">A</td><td style=\"border:1pt solid black\">B</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let max_document = Html::from_string(
        "<table style=\"margin:0;width:120pt;max-width:60pt;border-spacing:0\"><tr><td style=\"border:1pt solid black\">A</td><td style=\"border:1pt solid black\">B</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        (horizontal_table_border_widths(&min_document)
            .iter()
            .sum::<f32>()
            - 80.0)
            .abs()
            < 0.01
    );
    assert!(
        (horizontal_table_border_widths(&max_document)
            .iter()
            .sum::<f32>()
            - 60.0)
            .abs()
            < 0.01
    );
}

#[tokio::test]
async fn fixed_table_layout_uses_first_row_widths_before_later_content() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:120pt;table-layout:fixed;border-spacing:0\">\
         <tr><td style=\"width:30pt;border:1pt solid black\">A</td><td style=\"border:1pt solid black\">B</td></tr>\
         <tr><td style=\"border:1pt solid black\">ExtremelyLongUnbreakableIdentifier</td><td style=\"border:1pt solid black\">C</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 4);
    assert!((widths[0] - 30.0).abs() < 0.01);
    assert!((widths[1] - 90.0).abs() < 0.01);
    assert!((widths[2] - 30.0).abs() < 0.01);
    assert!((widths[3] - 90.0).abs() < 0.01);
}

#[tokio::test]
async fn fixed_table_layout_uses_colgroup_column_widths_before_first_row() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:120pt;table-layout:fixed;border-spacing:0\">\
         <colgroup><col style=\"width:40pt\"><col></colgroup>\
         <tr><td style=\"width:80pt;border:1pt solid black\">A</td><td style=\"border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 2);
    assert!((widths[0] - 40.0).abs() < 0.01);
    assert!((widths[1] - 80.0).abs() < 0.01);
}

#[tokio::test]
async fn parses_important_values_for_non_length_properties() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { margin: 0 } .hidden { display: none !important }</style><p class=\"hidden\">Gone</p><p>Shown</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines.len(), 1);
    assert_eq!(document.pages[0].lines[0].text, "Shown");
}

#[tokio::test]
async fn applies_table_section_row_and_cell_selectors() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:100pt\"><tbody><tr><td>A</td></tr></tbody></table>",
    )
    .with_stylesheet(Css::from_string(
        "tbody tr { border-top: 1pt solid red } tbody tr td { color: blue }",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.width >= 100.0 && rect.fill == Some(Color::new(255, 0, 0)))
    );
    assert_eq!(document.pages[0].lines[0].color, Color::new(0, 0, 255));
}

#[tokio::test]
async fn paints_table_structural_backgrounds_in_spec_layer_order() {
    let document = Html::from_string(
        "<style>table{margin:0;width:40pt;border-spacing:0;background:#111}colgroup{background:#222}col{background:#333}tbody{background:#444}tr{background:#555}td{padding:0;background:#666}</style>\
         <table><colgroup><col></colgroup><tbody><tr><td>A</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let fills = document.pages[0]
        .rects
        .iter()
        .filter_map(|rect| rect.fill)
        .take(6)
        .collect::<Vec<_>>();

    assert_eq!(
        fills,
        vec![
            Color::new(17, 17, 17),
            Color::new(34, 34, 34),
            Color::new(51, 51, 51),
            Color::new(68, 68, 68),
            Color::new(85, 85, 85),
            Color::new(102, 102, 102),
        ]
    );
}

#[tokio::test]
async fn empty_display_table_still_paints_padding_and_border_box() {
    let document = Html::from_string(
        "<style>@page{size:400pt 400pt;margin:20pt}body{margin:0}div{display:table;background:green;border:1px solid black;padding:155px}</style><div></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .expect("empty table should paint its background");

    assert!((green.width - 234.0).abs() < 0.01, "{green:?}");
    assert!((green.height - 234.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn honors_table_cellpadding_zero() {
    let zero =
        Html::from_string("<table cellpadding=\"0\" style=\"margin:0;width:100pt\"><tr><td style=\"border:1pt solid black\">A</td></tr></table>")
            .render_async(&RenderOptions::default()).await
            .unwrap();
    let padded =
        Html::from_string("<table cellpadding=\"4pt\" style=\"margin:0;width:100pt\"><tr><td style=\"border:1pt solid black\">A</td></tr></table>")
            .render_async(&RenderOptions::default()).await
            .unwrap();

    let zero_height = vertical_table_border_heights(&zero)
        .into_iter()
        .next()
        .expect("zero-padding cell should paint a vertical border");
    let padded_height = vertical_table_border_heights(&padded)
        .into_iter()
        .next()
        .expect("padded cell should paint a vertical border");

    assert!(zero_height > 1.0);
    assert!(
        padded_height - zero_height > 7.5,
        "expected explicit cellpadding to increase cell border height, zero={zero_height}, padded={padded_height}"
    );
}

#[tokio::test]
async fn table_cell_percentage_padding_resolves_against_cell_inline_size() {
    let document = Html::from_string(
        "<style>body{margin:0} table{margin:0;width:100pt;border-spacing:0;font-size:10pt;line-height:10pt}td{padding:10%;border:1pt solid black}</style>\
         <table><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border_height = vertical_table_border_heights(&document)
        .into_iter()
        .next()
        .expect("percentage-padded cell should paint a vertical border");

    assert!(
        (border_height - 32.0).abs() < 0.01,
        "10% top/bottom padding on a 100pt cell should add 20pt to 10pt text plus 2pt borders, got {border_height}"
    );
}

#[tokio::test]
async fn honors_table_border_spacing_between_columns() {
    let document = Html::from_string(
        "<style>body { margin:0 }</style>\
         <table cellpadding=\"0\" style=\"margin:0;width:44pt;border-spacing:4pt 6pt\">\
         <tr><td style=\"width:20pt;border:1pt solid black\">A</td><td style=\"width:20pt;border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[1].text, "B");
    assert!((lines[1].x - lines[0].x - 24.0).abs() < 0.01);
    let expected_first_text_x = RenderOptions::default().page_margins.left + 5.0;
    assert!(
        (lines[0].x - expected_first_text_x).abs() < 0.01,
        "expected first text x {expected_first_text_x}, got {}",
        lines[0].x
    );
}

#[tokio::test]
async fn honors_html_cellspacing_for_separated_tables() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" cellspacing=\"8pt\" style=\"margin:0;width:48pt\">\
         <tr><td style=\"width:20pt;border:1pt solid black\">A</td><td style=\"width:20pt;border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[1].text, "B");
    assert!((lines[1].x - lines[0].x - 28.0).abs() < 0.01);
}

#[tokio::test]
async fn css_border_spacing_overrides_html_cellspacing() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" cellspacing=\"8pt\" style=\"margin:0;width:44pt;border-spacing:4pt 0\">\
         <tr><td style=\"width:20pt;border:1pt solid black\">A</td><td style=\"width:20pt;border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert_eq!(lines[1].text, "B");
    assert!((lines[1].x - lines[0].x - 24.0).abs() < 0.01);
}

#[tokio::test]
async fn collapsed_table_borders_share_internal_edges() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-collapse:collapse\">\
         <tr><td style=\"width:20pt;border:1pt solid black\">A</td><td style=\"width:20pt;border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let vertical_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::BLACK) && rect.width <= 1.01 && rect.height > 1.0)
        .count();

    assert_eq!(vertical_edges, 3);
}

#[tokio::test]
async fn collapsed_table_borders_enter_paint_operation_stream() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-collapse:collapse\">\
         <tr><td style=\"width:20pt;border:1pt solid black\">A</td><td style=\"width:20pt;border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let page = &document.pages[0];

    let vertical_edge_indexes: Vec<_> = page
        .rects
        .iter()
        .enumerate()
        .filter(|(_, rect)| {
            rect.fill == Some(Color::BLACK) && rect.width <= 1.01 && rect.height > 1.0
        })
        .map(|(index, _)| index)
        .collect();

    assert_eq!(vertical_edge_indexes.len(), 3);
    for rect_index in vertical_edge_indexes {
        assert!(page.operations.iter().any(|operation| {
            matches!(operation, quire::PaintOperation::Rect(index) if *index == rect_index)
        }));
    }
}

#[tokio::test]
async fn nested_collapsed_table_border_joins_cover_inner_border() {
    let document = Html::from_string(
        r#"<style>@page { size: 140pt 140pt; margin: 20pt } body { margin: 0 }</style>
<table style="border-collapse: collapse">
  <td style="border: 37.5pt solid green; padding: 0">
    <table style="border-collapse: collapse; margin: -37.5pt">
      <td style="border: 37.5pt solid red; padding: 0"></td>
    </table>
  </td>
</table>"#,
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];
    let green = Color::new(0, 128, 0);

    let green_rects = page
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .collect::<Vec<_>>();
    let left = green_rects
        .iter()
        .map(|rect| rect.x)
        .fold(f32::INFINITY, f32::min);
    let right = green_rects
        .iter()
        .map(|rect| rect.x + rect.width)
        .fold(f32::NEG_INFINITY, f32::max);
    let bottom = green_rects
        .iter()
        .map(|rect| rect.y)
        .fold(f32::INFINITY, f32::min);
    let top = green_rects
        .iter()
        .map(|rect| rect.y + rect.height)
        .fold(f32::NEG_INFINITY, f32::max);

    assert!(
        (right - left - 75.0).abs() < 0.01,
        "green bbox width={} left={left} right={right}",
        right - left
    );
    assert!(
        (top - bottom - 75.0).abs() < 0.01,
        "green bbox height={} bottom={bottom} top={top}",
        top - bottom
    );

    for (x, y) in [
        (left + 1.0, bottom + 1.0),
        (right - 1.0, bottom + 1.0),
        ((left + right) / 2.0, (bottom + top) / 2.0),
        (left + 1.0, top - 1.0),
        (right - 1.0, top - 1.0),
    ] {
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(green),
            "nested collapsed border should leave green as final paint at ({x}, {y})"
        );
    }
}

#[tokio::test]
async fn orphan_table_cell_wrapper_lays_out_block_cell_contents() {
    let document = Html::from_string(
        r#"<style>@page { size: 180pt 180pt; margin: 20pt } body { margin: 0 }</style>
<div style="width: 75pt; height: 75pt; border: 2pt solid black">
  <div style="display: table-cell; max-width: 75pt; height: 75pt; background: green; vertical-align: top">
    <div style="width: 90pt; height: 37.5pt; background: hotpink"></div>
  </div>
</div>"#,
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let green = Color::new(0, 128, 0);
    let hotpink = Color::new(255, 105, 180);
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(green) && (rect.width - 75.0).abs() < 0.01)
    );
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(hotpink) && (rect.height - 37.5).abs() < 0.01)
    );
    document.validate_paint_operations().unwrap();
}

#[tokio::test]
async fn collapsed_table_dotted_borders_render_as_round_dot_paths() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:24pt;border-collapse:collapse\">\
         <tr><td style=\"width:24pt;border-top:2pt dotted blue\">A</td></tr>\
         </table>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];

    assert_eq!(
        page.rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
            .count(),
        0
    );
    let dot_indexes = page
        .paths
        .iter()
        .enumerate()
        .filter(|(_, path)| path.fill == Some(Color::new(0, 0, 255)))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    assert_eq!(dot_indexes.len(), 7);
    for path_index in dot_indexes {
        assert!(page.operations.iter().any(|operation| {
            matches!(operation, quire::PaintOperation::Path(index) if *index == path_index)
        }));
    }
}

#[tokio::test]
async fn collapsed_table_row_borders_resolve_to_one_shared_edge() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0}td{padding:0}tr:first-child{border-bottom:3pt solid red}tr:last-child{border-top:3pt solid blue}</style>\
         <table><tr><td>A</td></tr><tr><td>B</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(255, 0, 0))
                && (rect.height - 3.0).abs() < 0.01
                && (rect.width - 40.0).abs() < 0.01
        })
        .count();
    let blue_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .count();

    assert_eq!(red_edges, 1);
    assert_eq!(blue_edges, 0);
}

#[tokio::test]
async fn collapsed_table_cell_border_beats_row_border_at_same_edge() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0}td{padding:0}tr:first-child{border-bottom:2pt solid red}td{border-bottom:2pt solid blue}</style>\
         <table><tr><td>A</td></tr><tr><td>B</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 0, 255))
                && (rect.height - 2.0).abs() < 0.01
                && (rect.width - 40.0).abs() < 0.01
        })
        .count();
    let red_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .count();

    assert_eq!(blue_edges, 2);
    assert_eq!(red_edges, 0);
}

#[tokio::test]
async fn collapsed_table_row_group_border_beats_table_border_at_same_edge() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0;border-top:2pt solid red}td{padding:0}tbody{border-top:2pt solid blue}</style>\
         <table><tbody><tr><td>A</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 0, 255))
                && (rect.height - 2.0).abs() < 0.01
                && (rect.width - 40.0).abs() < 0.01
        })
        .count();
    let red_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .count();

    assert_eq!(blue_edges, 1);
    assert_eq!(red_edges, 0);
}

#[tokio::test]
async fn collapsed_table_row_border_beats_row_group_border_at_same_edge() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0}td{padding:0}tbody{border-top:2pt solid red}tr:first-child{border-top:2pt solid blue}</style>\
         <table><tbody><tr><td>A</td></tr></tbody></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 0, 255))
                && (rect.height - 2.0).abs() < 0.01
                && (rect.width - 40.0).abs() < 0.01
        })
        .count();
    let red_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .count();

    assert_eq!(blue_edges, 1);
    assert_eq!(red_edges, 0);
}

#[tokio::test]
async fn collapsed_table_column_group_border_beats_table_border_at_same_edge() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0;border-left:2pt solid red}td{padding:0}colgroup{border-left:2pt solid green}</style>\
         <table><colgroup><col></colgroup><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let green_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 128, 0))
                && (rect.width - 2.0).abs() < 0.01
                && rect.height > 1.0
        })
        .count();
    let red_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .count();

    assert_eq!(green_edges, 1);
    assert_eq!(red_edges, 0);
}

#[tokio::test]
async fn collapsed_table_column_border_beats_column_group_border_at_same_edge() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0}td{padding:0}colgroup{border-left:2pt solid red}col{border-left:2pt solid blue}</style>\
         <table><colgroup><col></colgroup><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 0, 255))
                && (rect.width - 2.0).abs() < 0.01
                && rect.height > 1.0
        })
        .count();
    let red_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .count();

    assert_eq!(blue_edges, 1);
    assert_eq!(red_edges, 0);
}

#[tokio::test]
async fn collapsed_table_cell_border_beats_column_border_at_same_edge() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0}td{padding:0;border-left:2pt solid blue}col{border-left:2pt solid red}</style>\
         <table><colgroup><col></colgroup><tr><td>A</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| {
            rect.fill == Some(Color::new(0, 0, 255))
                && (rect.width - 2.0).abs() < 0.01
                && rect.height > 1.0
        })
        .count();
    let red_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .count();

    assert_eq!(blue_edges, 1);
    assert_eq!(red_edges, 0);
}

#[tokio::test]
async fn collapsed_table_row_borders_do_not_cross_rowspan_cells() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:60pt;margin:0}td{padding:0}tr:first-child{border-bottom:2pt solid red}</style>\
         <table><tr><td rowspan=\"2\" style=\"width:30pt\">Span</td><td style=\"width:30pt\">A</td></tr><tr><td>B</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let red_edges = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)) && (rect.height - 2.0).abs() < 0.01)
        .collect::<Vec<_>>();

    assert_eq!(red_edges.len(), 1);
    assert!((red_edges[0].width - 30.0).abs() < 0.01);
}

#[tokio::test]
async fn collapsed_table_hidden_border_suppresses_conflicting_edges() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0}td{padding:0}tr:first-child{border-bottom:3pt solid red}tr:last-child{border-top:3pt hidden blue}</style>\
         <table><tr><td>A</td></tr><tr><td>B</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .rects
            .iter()
            .all(|rect| rect.fill != Some(Color::new(255, 0, 0)))
    );
}

#[tokio::test]
async fn collapsed_border_positioned_rows_and_cells_match_static_table() {
    let style = "<style>@page{size:240pt 180pt;margin:10pt}body{margin:0}td{width:50.6px;height:50.3px;background:yellow;padding:0;border:1px solid blue}</style>";
    let target = Html::from_string(format!(
        "{style}<table style=\"border-collapse:collapse\"><tr style=\"position:relative\"><td></td><td></td><td></td></tr><tr><td style=\"position:relative\"></td><td style=\"position:relative\"></td><td style=\"position:relative\"></td></tr></table>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let reference = Html::from_string(format!(
        "{style}<table style=\"border-collapse:collapse\"><tr><td></td><td></td><td></td></tr><tr><td></td><td></td><td></td></tr></table>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let target_page = &target.pages[0];
    let reference_page = &reference.pages[0];
    assert_eq!(target_page.operations, reference_page.operations);
    assert_eq!(target_page.rects, reference_page.rects);
    assert_eq!(target_page.paths, reference_page.paths);
}

#[tokio::test]
async fn table_cell_overflow_auto_matches_scroll_reference() {
    let style = "<style>@page{size:220pt 260pt;margin:10pt}body{margin:0}.outer{width:100px;height:100px;border:solid}.cell{max-width:100px;height:100px;background:green}.child{width:120px;height:50px;background:hotpink}</style>";
    let target = Html::from_string(format!(
        "{style}<div class=\"outer\"><div class=\"cell\" style=\"display:table-cell;overflow-x:auto;vertical-align:top\"><div class=\"child\"></div></div></div><br><div class=\"outer\"><div class=\"cell\" style=\"display:table-cell;overflow-x:auto;vertical-align:middle\"><div class=\"child\"></div></div></div>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let reference = Html::from_string(format!(
        "{style}<div class=\"outer\"><div class=\"cell\" style=\"overflow-x:scroll\"><div class=\"child\"></div></div></div><br><div class=\"outer\"><div class=\"cell\" style=\"display:table-cell;overflow-x:scroll;vertical-align:middle\"><div class=\"child\"></div></div></div>"
    ))
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let target_page = &target.pages[0];
    let reference_page = &reference.pages[0];
    assert_eq!(target_page.operations, reference_page.operations);
    assert_eq!(target_page.rects, reference_page.rects);
    assert_eq!(target_page.paths, reference_page.paths);
}
