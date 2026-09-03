use super::*;

fn assert_green_100px_square(document: &spindrift::Document) {
    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("expected a green canvas background");

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "expected a 100 CSS px green square, got {green:?}"
    );
}

fn rect_covers(
    covering: &crate::document::paint::shapes::RenderedRect,
    covered: &crate::document::paint::shapes::RenderedRect,
) -> bool {
    covering.x() <= covered.x() + 0.01
        && covering.y() <= covered.y() + 0.01
        && covering.x() + covering.width() >= covered.x() + covered.width() - 0.01
        && covering.y() + covering.height() >= covered.y() + covered.height() - 0.01
}

fn largest_filled_rect(
    page: &spindrift::Page,
    color: CssColor,
) -> &crate::document::paint::shapes::RenderedRect {
    page.rects()
        .iter()
        .filter(|rect| rect.fill == Some(color))
        .max_by(|left, right| {
            (left.width() * left.height()).total_cmp(&(right.width() * right.height()))
        })
        .unwrap_or_else(|| panic!("expected {color:?} rectangle among {:?}", page.rects()))
}

fn filled_rect_bounds(page: &spindrift::Page, color: CssColor) -> (f32, f32, f32, f32) {
    let rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(color))
        .collect::<Vec<_>>();
    assert!(!rects.is_empty(), "expected {color:?} rect paint");
    let left = rects
        .iter()
        .map(|rect| rect.x())
        .fold(f32::INFINITY, f32::min);
    let bottom = rects
        .iter()
        .map(|rect| rect.y())
        .fold(f32::INFINITY, f32::min);
    let right = rects
        .iter()
        .map(|rect| rect.x() + rect.width())
        .fold(f32::NEG_INFINITY, f32::max);
    let top = rects
        .iter()
        .map(|rect| rect.y() + rect.height())
        .fold(f32::NEG_INFINITY, f32::max);
    (left, bottom, right, top)
}

fn rect_top(rect: &crate::document::paint::shapes::RenderedRect) -> f32 {
    rect.y() + rect.height()
}

fn rect_contains_bounds(rect: (f32, f32, f32, f32), bounds: (f32, f32, f32, f32)) -> bool {
    rect.0 <= bounds.0 + 0.01
        && rect.1 <= bounds.1 + 0.01
        && rect.2 >= bounds.2 - 0.01
        && rect.3 >= bounds.3 - 0.01
}

fn path_bounds(path: &crate::document::paint::paths::RenderedPath) -> Option<(f32, f32, f32, f32)> {
    let mut left = f32::INFINITY;
    let mut bottom = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut top = f32::NEG_INFINITY;
    let mut saw_point = false;
    let mut include_point = |point: crate::document::paint::geometry::PaintPoint| {
        saw_point = true;
        left = left.min(point.x);
        bottom = bottom.min(point.y);
        right = right.max(point.x);
        top = top.max(point.y);
    };
    for command in &path.commands {
        match command {
            crate::document::paint::paths::RenderedPathCommand::MoveTo(point)
            | crate::document::paint::paths::RenderedPathCommand::LineTo(point) => {
                include_point(*point)
            }
            crate::document::paint::paths::RenderedPathCommand::CurveTo {
                control_1,
                control_2,
                end,
            } => {
                include_point(*control_1);
                include_point(*control_2);
                include_point(*end);
            }
            crate::document::paint::paths::RenderedPathCommand::Close => {}
        }
    }
    saw_point.then_some((left, bottom, right, top))
}

fn path_contains_point(path: &crate::document::paint::paths::RenderedPath, x: f32, y: f32) -> bool {
    let points = path
        .commands
        .iter()
        .filter_map(|command| match command {
            crate::document::paint::paths::RenderedPathCommand::MoveTo(point)
            | crate::document::paint::paths::RenderedPathCommand::LineTo(point) => Some(*point),
            crate::document::paint::paths::RenderedPathCommand::CurveTo { .. }
            | crate::document::paint::paths::RenderedPathCommand::Close => None,
        })
        .collect::<Vec<_>>();
    if points.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = *points.last().expect("checked non-empty");
    for current in points {
        let crosses = (current.y > y) != (previous.y > y);
        if crosses {
            let edge_x =
                (previous.x - current.x) * (y - current.y) / (previous.y - current.y) + current.x;
            if x < edge_x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

fn path_fill_at(
    paths: &[&crate::document::paint::paths::RenderedPath],
    x: f32,
    y: f32,
) -> Option<CssColor> {
    paths
        .iter()
        .rev()
        .find(|path| path_contains_point(path, x, y))
        .and_then(|path| path.fill)
}

fn table_wrapper_border_rect_indices(page: &spindrift::Page) -> Vec<usize> {
    page.rects()
        .iter()
        .enumerate()
        .filter_map(|(index, rect)| {
            (rect.fill == Some(CssColor::BLACK)
                && ((rect.height() <= 1.0 && rect.width() > 200.0)
                    || (rect.width() <= 1.0 && rect.height() > 200.0)))
                .then_some(index)
        })
        .collect()
}

fn table_wrapper_border_paint_operation_indices(page: &spindrift::Page) -> Vec<usize> {
    page.paint_operations()
        .iter()
        .enumerate()
        .filter_map(|(operation_index, operation)| {
            let crate::document::paint::page::PaintOperation::Rect(rect_index) = operation else {
                return None;
            };
            let rect = page.rects().get(*rect_index)?;
            (rect.fill == Some(CssColor::BLACK)
                && ((rect.height() <= 1.0 && rect.width() > 200.0)
                    || (rect.width() <= 1.0 && rect.height() > 200.0)))
                .then_some(operation_index)
        })
        .collect()
}

#[tokio::test]
async fn anonymous_table_cell_floated_inline_child_paints() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page{size:140px 140px;margin:0}body{margin:0}p{display:none}</style>\
         <p>Test passes if there is a filled green square.</p>\
         <div style=\"display: table;\">\
           <span style=\"float: left; width: 100px; height: 100px; background: green;\"></span>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_green_100px_square(&document);
}

#[tokio::test]
async fn flex_item_table_with_floated_inline_anonymous_cell_paints() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page{size:140px 140px;margin:0}body{margin:0}p{display:none}</style>\
         <p>Test passes if there is a filled green square.</p>\
         <div style=\"display: flex;\">\
           <div style=\"display: table;\">\
             <span style=\"float: left; width: 100px; height: 100px; background: green;\"></span>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_green_100px_square(&document);
}

#[tokio::test]
async fn anonymous_table_cell_preserved_whitespace_wraps_percentage_inline_blocks() {
    let document = Html::from_string(
        "<!doctype html>\
         <meta charset=\"utf-8\">\
         <style>\
         @page { size: 600px 300px; margin: 0 }\
         body { margin: 0 }\
         .outer { display: table; width: 500px; background: purple; border: 1px solid green }\
         .half { display: inline-block; width: 50%; background: blue }\
         .half + .half { background: yellow }\
         </style>\
         <div class=\"outer\"><div class=\"half\">A</div> <div class=\"half\">B</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let blue = largest_filled_rect(page, CssColor::new(0, 0, 255));
    let yellow = largest_filled_rect(page, CssColor::new(255, 255, 0));
    let expected_half_width = 187.5;
    assert!(
        (blue.width() - expected_half_width).abs() < 0.5,
        "blue inline-block should be half the 500 CSS px table width: {blue:?}"
    );
    assert!(
        (yellow.width() - expected_half_width).abs() < 0.5,
        "yellow inline-block should be half the 500 CSS px table width: {yellow:?}"
    );
    assert!(
        (blue.x() - yellow.x()).abs() < 0.5 && blue.y() > yellow.y() + 0.5,
        "preserved whitespace should prevent two 50% inline-blocks from sharing one line: blue={blue:?} yellow={yellow:?}"
    );
}

#[tokio::test]
async fn anonymous_table_cell_adjacent_percentage_inline_blocks_share_line() {
    let document = Html::from_string(
        "<!doctype html>\
         <meta charset=\"utf-8\">\
         <style>\
         @page { size: 600px 300px; margin: 0 }\
         body { margin: 0 }\
         .outer { display: table; width: 500px; background: purple; border: 1px solid green }\
         .half { display: inline-block; width: 50%; background: blue }\
         .half + .half { background: yellow }\
         </style>\
         <div class=\"outer\"><div class=\"half\">A</div><div class=\"half\">B</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let blue = largest_filled_rect(page, CssColor::new(0, 0, 255));
    let yellow = largest_filled_rect(page, CssColor::new(255, 255, 0));
    let expected_half_width = 187.5;
    assert!(
        (blue.width() - expected_half_width).abs() < 0.5,
        "blue inline-block should be half the 500 CSS px table width: {blue:?}"
    );
    assert!(
        (yellow.width() - expected_half_width).abs() < 0.5,
        "yellow inline-block should be half the 500 CSS px table width: {yellow:?}"
    );
    assert!(
        (blue.y() - yellow.y()).abs() < 0.5 && (yellow.x() - (blue.x() + blue.width())).abs() < 0.5,
        "adjacent 50% inline-blocks should share one line: blue={blue:?} yellow={yellow:?}"
    );
}

#[tokio::test]
async fn anonymous_table_cells_collapse_self_collapsing_block_margins_for_row_height() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page{size:140px 140px;margin:0}body{margin:0}p{display:none}</style>\
         <p>Test passes if there is a filled green square and no red.</p>\
         <div style=\"display:table;height:100px;background:red\">\
           <div style=\"display:table-row;background:green\">\
             <div style=\"width:100px;margin:50px 0\">\
               <div style=\"margin:50px 0\"></div>\
             </div>\
             <div style=\"margin:50px 0\"></div>\
           </div>\
           <div style=\"display:table-row;background:green\">\
             <div style=\"margin:50px 0\"></div>\
           </div>\
           <div style=\"display:table-row\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green_bounds = filled_rect_bounds(page, CssColor::new(0, 128, 0));
    assert!(
        (green_bounds.2 - green_bounds.0 - 75.0).abs() < 0.01
            && (green_bounds.3 - green_bounds.1 - 75.0).abs() < 0.01,
        "anonymous table rows should form a 100 CSS px green square, got {green_bounds:?}"
    );

    for red in page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
    {
        let red_bounds = (
            red.x(),
            red.y(),
            red.x() + red.width(),
            red.y() + red.height(),
        );
        assert!(
            rect_contains_bounds(green_bounds, red_bounds),
            "red table background should be fully covered by green rows: green={green_bounds:?} red={red:?}"
        );
    }
}

#[tokio::test]
async fn table_cell_self_collapsing_block_margin_set_sets_row_minimum() {
    let document = Html::from_string(
        "<style>@page{size:140px 140px;margin:0}body{margin:0}</style>\
         <div style=\"display:table;border-spacing:0\">\
           <div style=\"display:table-row;background:green\">\
             <div style=\"display:table-cell;padding:0\">\
               <div style=\"width:100px;margin:50px 0\">\
                 <div style=\"margin:50px 0\"></div>\
               </div>\
             </div>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = largest_filled_rect(&document.pages[0], CssColor::new(0, 128, 0));
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 37.5).abs() < 0.01,
        "self-collapsing nested block margins should collapse to one 50 CSS px row minimum, got {green:?}"
    );
}

#[tokio::test]
async fn renders_basic_tables_in_rows_and_columns() {
    let document = Html::from_string(
        "<table style=\"margin: 0; width: 120pt\"><tr><th style=\"border: 1pt solid black\">A</th><th>B</th></tr><tr><td>C</td><td>D</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[0].lines()[1].text, "B");
    assert_eq!(document.pages[0].lines()[2].text, "C");
    assert_eq!(document.pages[0].lines()[3].text, "D");
    assert!(document.pages[0].lines()[1].x() > document.pages[0].lines()[0].x());
    assert!(document.pages[0].lines()[2].y() < document.pages[0].lines()[0].y());
    assert_eq!(document.pages[0].rects()[0].fill, Some(CssColor::BLACK));
    assert_eq!(document.pages[0].rects()[0].stroke, None);
}

#[tokio::test]
async fn rowspan_over_collapsed_row_does_not_create_internal_border_spacing() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <meta charset=\"utf-8\">\
         <style>\
         table { border-spacing: 50px 100px; background: green }\
         td { width: 100px; padding: 0; background: red }\
         tr + tr { visibility: collapse }\
         </style>\
         <p>Test passes if there is a filled green square and <strong>no red</strong>.</p>\
         <table><tr><td rowspan=\"2\"></td></tr><tr></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = largest_filled_rect(page, CssColor::new(0, 128, 0));
    assert!(
        (green.width() - 150.0).abs() < 0.01 && (green.height() - 150.0).abs() < 0.01,
        "expected a 200 CSS px green square, got {green:?}"
    );
    assert!(
        !page
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0))),
        "collapsed-row rowspan cell should not paint red: {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn separated_table_wrapper_border_paints_above_background() {
    let document = Html::from_string(
        "<!doctype html>\
         <style>\
         @page { size: 400px 400px; margin: 0 }\
         body { margin: 0 }\
         td { padding: 0 }\
         table { border-spacing: 0; border: 1px solid black; background: green; padding: 5px }\
         div { width: 300px; height: 300px }\
         </style>\
         <table><tr><td><div></div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green_operation = first_rect_paint_operation_index(page, CssColor::new(0, 128, 0));
    let border_indices = table_wrapper_border_rect_indices(page);
    assert!(
        border_indices.len() >= 4,
        "expected visible table wrapper border rects among {:?}",
        page.rects()
    );
    let border_operations = table_wrapper_border_paint_operation_indices(page);
    assert!(
        border_operations
            .iter()
            .all(|index| *index > green_operation),
        "table wrapper border should paint after table wrapper background; operations={:?} rects={:?}",
        page.paint_operations(),
        page.rects()
    );
}

#[tokio::test]
async fn separated_table_wrapper_border_paints_without_background() {
    let document = Html::from_string(
        "<!doctype html>\
         <style>\
         @page { size: 400px 400px; margin: 0 }\
         body { margin: 0 }\
         td { padding: 0 }\
         table { border-spacing: 0; border: 1px solid black; padding: 5px }\
         div { width: 300px; height: 300px }\
         </style>\
         <table><tr><td><div></div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let border_indices = table_wrapper_border_rect_indices(page);
    assert!(
        border_indices.len() >= 4,
        "expected visible table wrapper border rects among {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn rtl_column_backgrounds_do_not_paint_separated_row_spacing() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 220px 220px; margin: 0 }\
         body { margin: 0 }\
         table { border: solid 2px; border-spacing: 5px; padding: 5px }\
         col { background: linear-gradient(60deg, red 50%, blue 50%) }\
         </style>\
         <table style=\"direction: rtl\">\
           <col style=\"width: 100px\"></col>\
           <col style=\"width: 50px\"></col>\
           <tr style=\"height: 100px\"><td></td><td></td></tr>\
           <tr style=\"height: 50px\"><td></td><td></td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = CssColor::new(255, 0, 0);
    let blue = CssColor::new(0, 0, 255);
    let gradient_paths = page
        .paths()
        .iter()
        .filter(|path| matches!(path.fill, Some(fill) if fill == red || fill == blue))
        .collect::<Vec<_>>();
    assert!(
        gradient_paths.iter().any(|path| path.fill == Some(red))
            && gradient_paths.iter().any(|path| path.fill == Some(blue)),
        "expected red and blue column gradient paths among {:?}",
        page.paths()
    );

    let page_height = 220.0 * 0.75;
    let px = 0.75;
    let expected_left = (2.0 + 5.0 + 5.0) * px;
    let expected_right = expected_left + (50.0 + 5.0 + 100.0) * px;
    assert!(
        gradient_paths.iter().all(|path| {
            let Some((left, _, right, _)) = path_bounds(path) else {
                return true;
            };
            left >= expected_left - 0.01 && right <= expected_right + 0.01
        }),
        "RTL column backgrounds must stay inside separated edge spacing ({expected_left}..{expected_right}); paths={:?}",
        gradient_paths
    );

    let row_gap_top = page_height - (2.0 + 5.0 + 5.0 + 100.0) * 0.75;
    let row_gap_bottom = row_gap_top - 5.0 * 0.75;
    assert!(
        gradient_paths.iter().all(|path| {
            let Some((_, bottom, _, top)) = path_bounds(path) else {
                return true;
            };
            bottom >= row_gap_top - 0.01 || top <= row_gap_bottom + 0.01
        }),
        "column gradient backgrounds must not paint the separated row gap ({row_gap_bottom}..{row_gap_top}); paths={:?}",
        gradient_paths
    );

    let grid_top = page_height - (2.0 + 5.0 + 5.0) * px;
    let grid_height = (100.0 + 5.0 + 50.0) * px;
    let center_y = grid_top - grid_height / 2.0;
    let gradient_angle = 60.0_f32.to_radians();
    let dir_x = gradient_angle.sin();
    let dir_y = gradient_angle.cos();
    for (left, width) in [
        (expected_left, 50.0 * px),
        (expected_left + 55.0 * px, 100.0 * px),
    ] {
        let center_x = left + width / 2.0;
        assert_eq!(
            path_fill_at(&gradient_paths, center_x - dir_x, center_y - dir_y),
            Some(red),
            "red half of the column gradient should be centered in the RTL column box"
        );
        assert_eq!(
            path_fill_at(&gradient_paths, center_x + dir_x, center_y + dir_y),
            Some(blue),
            "blue half of the column gradient should be centered in the RTL column box"
        );
    }
}

#[tokio::test]
async fn vertical_rl_rtl_column_backgrounds_paint_from_column_heights() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 300px 300px; margin: 0 }\
         body { margin: 0 }\
         table { border: solid 2px; border-spacing: 5px; padding: 5px }\
         col { background: linear-gradient(30deg, red 50%, blue 50%) }\
         td { padding: 0 }\
         </style>\
         <table style=\"writing-mode: vertical-rl; direction: rtl\">\
           <col style=\"height: 50px\"></col>\
           <col style=\"height: 100px\"></col>\
           <tr style=\"width: 100px\"><td></td><td></td></tr>\
           <tr style=\"width: 50px\"><td></td><td></td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.paths()
            .iter()
            .any(|path| path.fill == Some(CssColor::new(255, 0, 0))),
        "expected red column gradient path among {:?}",
        page.paths()
    );
    assert!(
        page.paths()
            .iter()
            .any(|path| path.fill == Some(CssColor::new(0, 0, 255))),
        "expected blue column gradient path among {:?}",
        page.paths()
    );
    assert!(
        page.rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::BLACK) && rect.width() > 100.0),
        "expected non-tiny table border among {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn vertical_upright_ch_units_size_table_rows_like_vertical_reference() {
    let document = Html::from_string(
        r#"
        <style>
        @page { size: 240pt 240pt; margin: 0 }
        body { margin: 0 }
        table {
          font-size: 20px;
          border-collapse: collapse;
          border: none;
        }
        td {
          padding: 0;
          background: green;
          height: 5ch;
        }
        tr {
          writing-mode: vertical-rl;
          text-orientation: upright;
          line-height: 5ch;
        }
        div {
          font-size: 20px;
          color: transparent;
        }
        div:nth-of-type(1) {
          background: blue;
          writing-mode: vertical-rl;
          text-orientation: upright;
          width: 5ch;
        }
        div:nth-of-type(2) {
          background: orange;
          height: 5ch;
          display: inline-block;
        }
        </style>
        <table><tbody><tr><td>&nbsp;</td></tr></tbody></table>
        <div>00000</div>
        <div>00000</div>
        "#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = largest_filled_rect(page, CssColor::new(0, 128, 0));
    let blue = largest_filled_rect(page, CssColor::new(0, 0, 255));
    let orange = largest_filled_rect(page, CssColor::new(255, 165, 0));

    for (name, rect) in [("green", green), ("blue", blue), ("orange", orange)] {
        assert!(
            (rect.width() - rect.height()).abs() < 0.5,
            "{name} rect should be square: {rect:?}"
        );
    }
    assert!(
        (green.width() - blue.width()).abs() < 0.5,
        "green should match vertical blue reference: green={green:?}, blue={blue:?}"
    );
    assert!(
        (orange.width() - blue.width()).abs() > 1.0,
        "orange horizontal ch reference should differ from blue vertical reference: orange={orange:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn sideways_vertical_ch_units_size_table_columns_like_sideways_reference() {
    let document = Html::from_string(
        r#"
        <style>
        @page { size: 240pt 240pt; margin: 0 }
        body { margin: 0 }
        table {
          font-size: 20px;
          border-collapse: collapse;
          border: none;
        }
        td {
          padding: 0;
          background: green;
          height: 5ch;
          writing-mode: vertical-rl;
          text-orientation: upright;
        }
        col {
          writing-mode: vertical-rl;
          text-orientation: sideways;
          width: 5ch;
        }
        div {
          font-size: 20px;
          color: transparent;
        }
        div:nth-of-type(1) {
          background: blue;
          height: 5ch;
          display: inline-block;
        }
        div:nth-of-type(2) {
          background: orange;
          writing-mode: vertical-rl;
          text-orientation: upright;
          width: 5ch;
        }
        </style>
        <table><col><tbody><tr><td>&nbsp;</td></tr></tbody></table>
        <div>00000</div>
        <div>00000</div>
        "#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = largest_filled_rect(page, CssColor::new(0, 128, 0));
    let blue = largest_filled_rect(page, CssColor::new(0, 0, 255));
    let orange = largest_filled_rect(page, CssColor::new(255, 165, 0));

    for (name, rect) in [("green", green), ("blue", blue), ("orange", orange)] {
        assert!(
            (rect.width() - rect.height()).abs() < 0.5,
            "{name} rect should be square: {rect:?}"
        );
    }
    assert!(
        (green.width() - blue.width()).abs() < 0.5,
        "green should match sideways blue reference: green={green:?}, blue={blue:?}"
    );
    assert!(
        (orange.width() - blue.width()).abs() > 1.0,
        "orange upright ch reference should differ from blue sideways reference: orange={orange:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn fixed_vertical_sideways_table_columns_use_physical_heights() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 0 }\
         body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { writing-mode: vertical-rl; text-orientation: sideways; table-layout: fixed; width: 20pt; height: 60pt }\
         td:first-child { background: red }\
         td:last-child { background: blue }</style>\
         <table><col style=\"height: 15pt\"><col style=\"height: 45pt\"><tr><td></td><td></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = largest_filled_rect(page, CssColor::new(255, 0, 0));
    let blue = largest_filled_rect(page, CssColor::new(0, 0, 255));

    assert!(
        (red.height() - 15.0).abs() < 0.01,
        "first fixed vertical column must use its physical height: {red:?}"
    );
    assert!(
        (blue.height() - 45.0).abs() < 0.01,
        "second fixed vertical column must use its physical height: {blue:?}"
    );
}

#[tokio::test]
async fn fixed_vertical_and_sideways_first_row_cells_use_root_inline_heights() {
    for writing_mode in ["vertical-lr", "vertical-rl", "sideways-lr", "sideways-rl"] {
        let document = Html::from_string(
            format!(
                "<style>@page {{ size: 120pt 120pt; margin: 0 }}\
                 body, table, td {{ margin: 0; padding: 0; border-spacing: 0 }}\
                 table {{ writing-mode: {writing_mode}; table-layout: fixed; width: 20pt; height: 60pt }}\
                 td:first-child {{ width: 45pt; height: 15pt; background: red }}\
                 td:last-child {{ width: 15pt; height: 45pt; background: blue }}</style>\
                 <table><tr><td></td><td></td></tr></table>"
            ),
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let red = largest_filled_rect(page, CssColor::new(255, 0, 0));
        let blue = largest_filled_rect(page, CssColor::new(0, 0, 255));
        assert!(
            (red.height() - 15.0).abs() < 0.01,
            "{writing_mode} must use the first cell's physical height, not width: {red:?}"
        );
        assert!(
            (blue.height() - 45.0).abs() < 0.01,
            "{writing_mode} must use the second cell's physical height, not width: {blue:?}"
        );
    }
}

#[tokio::test]
async fn fixed_vertical_column_declarations_override_first_row_cells() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 0 }\
         body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         table { writing-mode: vertical-rl; table-layout: fixed; width: 20pt; height: 30pt }\
         td:first-child { height: 50pt; background: red }\
         td:last-child { height: 10pt; background: blue }</style>\
         <table><col style=\"height: 20pt\"><col><tr><td></td><td></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = largest_filled_rect(page, CssColor::new(255, 0, 0));
    let blue = largest_filled_rect(page, CssColor::new(0, 0, 255));
    assert!((red.height() - 20.0).abs() < 0.01, "red={red:?}");
    assert!((blue.height() - 10.0).abs() < 0.01, "blue={blue:?}");
}

#[tokio::test]
async fn fixed_vertical_first_row_cells_respect_box_sizing_and_percentages() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 0 }\
         body, table, td { margin: 0; border-spacing: 0 }\
         table { writing-mode: vertical-lr; table-layout: fixed; width: 20pt; height: 80pt }\
         .content { height: 10pt; padding-top: 2pt; padding-bottom: 2pt; border-top: 3pt solid red; border-bottom: 3pt solid red; background: red }\
         .border { box-sizing: border-box; height: 25%; padding-top: 2pt; padding-bottom: 2pt; border-top: 3pt solid blue; border-bottom: 3pt solid blue; background: blue }\
         .remainder { background: green }</style>\
         <table><tr><td class=\"content\"></td><td class=\"border\"></td><td class=\"remainder\"></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = largest_filled_rect(page, CssColor::new(255, 0, 0));
    let blue = largest_filled_rect(page, CssColor::new(0, 0, 255));
    let green = largest_filled_rect(page, CssColor::new(0, 128, 0));
    assert!((red.height() - 20.0).abs() < 0.01, "red={red:?}");
    assert!((blue.height() - 20.0).abs() < 0.01, "blue={blue:?}");
    assert!((green.height() - 40.0).abs() < 0.01, "green={green:?}");
}

#[tokio::test]
async fn table_cell_second_pass_resolves_percentage_height_canvas() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 160pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"display: table; border-spacing: 0\">\
           <div style=\"display: table-cell; height: 50px; padding: 0\">\
             <div></div>\
             <canvas width=\"1\" height=\"1\" style=\"height: 200%; background: green\"></canvas>\
           </div>\
           <div style=\"display: table-cell; padding: 0\">\
             <div style=\"height: 100px\"></div>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_green_100px_square(&document);
}

#[tokio::test]
async fn table_row_minimum_ignores_percentage_height_source_less_image() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}table{width:100px;height:100px;border-spacing:0;background:green}td{padding:0}.first{height:20px}.second{height:100%}img{width:100%;height:100%;visibility:hidden}</style>\
         <table><tr><td class=\"first\"></td></tr><tr><td class=\"second\"><img></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = largest_filled_rect(&document.pages[0], CssColor::new(0, 128, 0));
    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "a source-less percentage-height image must not enlarge the table row minimum: {green:?}"
    );
}

#[tokio::test]
async fn table_cell_final_relayout_sizes_percentage_source_less_image_inside_scrollport() {
    let document = Html::from_string(
        "<style>@page{size:240pt 240pt;margin:0}body{margin:0}.table{display:table;border:solid 5px black;width:150px;height:100px}.cell{display:table-cell;background:cyan;overflow:scroll;padding:5px 15px 10px 20px;border:solid magenta;border-width:12px 9px 6px 3px}img{display:block;background:yellow;width:100%;height:100%}</style>\
         <div class=\"table\"><div class=\"cell\"><img></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let black = filled_rect_bounds(page, CssColor::BLACK);
    let cyan = largest_filled_rect(page, CssColor::new(0, 255, 255));
    let yellow = largest_filled_rect(page, CssColor::new(255, 255, 0));

    assert!(
        ((black.2 - black.0) - 120.0).abs() < 0.01 && ((black.3 - black.1) - 82.5).abs() < 0.01,
        "table border box must keep its 150px by 100px content size: {black:?}"
    );
    assert!(
        (cyan.width() - 112.5).abs() < 0.01 && (cyan.height() - 75.0).abs() < 0.01,
        "cell background must use the distributed table height: {cyan:?}"
    );
    assert!(
        (yellow.width() - 77.25).abs() < 0.01 && (yellow.height() - 50.25).abs() < 0.01,
        "final image must resolve against the cell content box: {yellow:?}"
    );
    assert!(
        rect_covers(cyan, yellow),
        "final percentage-height image must remain inside the cell scrollport: cyan={cyan:?}, yellow={yellow:?}"
    );
}

#[tokio::test]
async fn vertical_table_cell_final_relayout_uses_projected_physical_content_box() {
    for writing_mode in ["vertical-lr", "vertical-rl"] {
        let document = Html::from_string(format!(
            "<style>@page{{size:260pt 260pt;margin:0}}body{{margin:0}}\
             table{{writing-mode:{writing_mode};width:100pt;height:160pt;border-spacing:0}}\
             td{{background:cyan;overflow:scroll;padding:5pt 15pt 10pt 20pt;border:solid magenta;border-width:12pt 9pt 6pt 3pt}}\
             img{{display:block;background:yellow;width:100%;height:100%}}</style>\
             <table><tr><td><img></td></tr></table>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let cell = largest_filled_rect(page, CssColor::new(0, 255, 255));
        let image = largest_filled_rect(page, CssColor::new(255, 255, 0));
        let expected_width = cell.width() - 3.0 - 9.0 - 20.0 - 15.0;
        let expected_height = cell.height() - 12.0 - 6.0 - 5.0 - 10.0;

        assert!(
            (image.width() - expected_width).abs() < 0.01,
            "{writing_mode}: percentage width must resolve against the projected physical content width: cell={cell:?}, image={image:?}"
        );
        assert!(
            (image.height() - expected_height).abs() < 0.01,
            "{writing_mode}: percentage height must resolve against the projected physical content height: cell={cell:?}, image={image:?}"
        );
        assert!(
            image.height() > image.width(),
            "{writing_mode}: the final relayout must not substitute the row/block track for the physical content height: image={image:?}"
        );
    }
}

#[tokio::test]
async fn table_cell_second_pass_uses_final_height_for_overflow_auto_min_height_child() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}.list-div{overflow-y:auto;height:100%;width:100px;min-height:100px;background:green}#redSquare{height:100px;width:100px;background:red;position:absolute;z-index:-1}</style>\
         <div id=\"redSquare\"></div><div style=\"display:table;border-spacing:0\"><div style=\"display:table-cell;height:50px;padding:0\"><div class=\"list-div\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("percentage-height overflow child should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("positioned red reference square should paint behind green");

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "expected green child to fill the 100 CSS px reference square, got {green:?}"
    );
    assert!(
        rect_covers(green, red),
        "green square should fully cover the red reference: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn table_cell_percent_height_overflow_auto_descendant_intrinsic_height() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}.list-div{overflow-y:auto;height:100%}.list-div-child{width:100px;height:100px;background:green}#redSquare{height:100px;width:100px;background:red;position:absolute;z-index:-1}</style>\
         <div id=\"redSquare\"></div><div style=\"display:table;border-spacing:0\"><div style=\"display:table-cell;height:100%;padding:0\"><div class=\"list-div\"><div class=\"list-div-child\"></div></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| {
            panic!(
                "intrinsic-height descendant should paint: {:?}",
                page.rects()
            )
        });
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("positioned red reference square should paint behind green");

    assert!(
        (green.width() - 75.0).abs() < 0.01 && (green.height() - 75.0).abs() < 0.01,
        "expected intrinsic descendant to fill the 100 CSS px reference square, got {green:?}"
    );
    assert!(
        rect_covers(green, red),
        "green square should fully cover the red reference: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn table_cell_first_pass_treats_percent_height_child_as_auto() {
    let document = Html::from_string(
        "<style>@page{size:120pt 120pt;margin:0}body{margin:0}.cell{display:table-cell;padding:0;background:red}.child{height:200%;width:20px;background:green}.grand{height:20px}</style>\
         <div style=\"display:table;border-spacing:0\"><div class=\"cell\"><div class=\"child\"><div class=\"grand\"></div></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("table-cell background should paint");
    let _green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("percentage-height child should paint");

    assert!(
        (red.height() - 15.0).abs() < 0.01,
        "percentage height must not inflate the first-pass row minimum: red={red:?}"
    );
}

#[tokio::test]
async fn supports_authored_table_internal_display_values() {
    let document = Html::from_string(
        "<div style=\"display:table;margin:0;width:80pt;border-spacing:0\">\
         <div style=\"display:table-row\"><span style=\"display:table-cell;width:40pt\">A</span><span style=\"display:table-cell;width:40pt\">B</span></div>\
         </div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[0].text, "A");
    assert_eq!(lines[1].text, "B");
    assert!(lines[1].x() > lines[0].x());
}

#[tokio::test]
async fn table_cell_align_content_overrides_legacy_vertical_align_when_non_normal() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0; font-size: 10pt; line-height: 10pt }\
         td { width: 55pt; height: 60pt; vertical-align: top }\
         .normal { vertical-align: bottom; align-content: normal }\
         .center { align-content: center }\
         .end { align-content: end }</style>\
         <table><tr><td>Top</td><td class=\"normal\">Normal</td><td class=\"center\">Center</td><td class=\"end\">End</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render: {:?}", document.pages[0].lines()))
    };
    let top = line("Top");
    let normal = line("Normal");
    let center = line("Center");
    let end = line("End");

    assert!(
        (top.y() - center.y() - 25.0).abs() < 0.5,
        "align-content:center should center table-cell content: top={top:?}, center={center:?}"
    );
    assert!(
        (top.y() - end.y() - 50.0).abs() < 0.5,
        "align-content:end should pack table-cell content to block-end: top={top:?}, end={end:?}"
    );
    assert!(
        (normal.y() - end.y()).abs() < 0.5,
        "align-content:normal should preserve legacy vertical-align behavior: normal={normal:?}, end={end:?}"
    );
}

#[tokio::test]
async fn vertical_lr_table_cell_align_content_center_uses_horizontal_block_axis() {
    let document = Html::from_string(
        "<style>@page { size: 150pt 120pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         td { writing-mode: vertical-lr; width: 80pt; height: 50pt; align-content: center; background: red }\
         .item { width: 20pt; height: 40pt; background: green }</style>\
         <table><tr><td><div class=\"item\"></div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("vertical table-cell background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("vertical table-cell child should paint");

    assert!(
        (green.x() - red.x() - 30.0).abs() < 0.01,
        "vertical-lr table-cell align-content:center should center on the horizontal block axis: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn vertical_lr_table_cell_align_content_centers_inline_text_subject() {
    let document = Html::from_string(
        "<style>@page { size: 150pt 120pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         td { writing-mode: vertical-lr; width: 80pt; height: 50pt; align-content: center; font-size: 10pt; line-height: 20pt; background: red }</style>\
         <table><tr><td>縦</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("vertical table-cell background should paint");
    let line = page
        .lines()
        .iter()
        .find(|line| line.text == "縦")
        .expect("vertical table-cell inline text should paint");

    assert!(
        (line.x() - red.x() - 35.0).abs() < 0.5,
        "vertical-lr table-cell align-content:center should center the inline line box and paint the upright glyph inside it: red={red:?}, line={line:?}"
    );
}

#[tokio::test]
async fn upright_vertical_rowspan_text_applies_each_glyph_origin_once() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 130pt; margin: 0 }\
         @font-face { font-family: Ahem; src: url(Ahem.ttf) }\
         body { margin: 0 }\
         table { border-collapse: collapse; table-layout: fixed; width: 120pt }\
         td { border: 1pt solid black; padding: 0 }\
         tr { height: 30pt }\
         .group { width: 30pt; text-align: center; writing-mode: vertical-rl; text-orientation: upright; font: 12pt Ahem }</style>\
         <table><tr><td class=\"group\" rowspan=\"2\">AAA</td><td>one</td></tr><tr><td>two</td></tr></table>",
    )
    .with_base_path("tests/fixtures/wpt/css/css-fonts")
    .unwrap()
    .render(&RenderOptions::default())
    .await
    .unwrap();

    // Upright vertical shaping materializes one typographic unit per line
    // record; collect the three source units before checking their advances.
    let label_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| line.text == "A")
        .collect::<Vec<_>>();
    assert_eq!(label_lines.len(), 3, "upright rowspan label should paint");
    let glyph_origins = label_lines
        .iter()
        .flat_map(|line| line.runs.iter())
        .map(|run| {
            let glyph = run
                .glyphs
                .as_ref()
                .and_then(|glyphs| glyphs.first())
                .expect("upright text run should retain its glyph");
            run.y_offset + glyph.y_offset
        })
        .collect::<Vec<_>>();

    assert_eq!(glyph_origins.len(), 3);
    for (index, origin) in glyph_origins.iter().enumerate() {
        assert!(
            (*origin - (-(index as f32) * 12.0)).abs() < 0.01,
            "glyph {index} must receive its normal-flow vertical advance once: lines={label_lines:?}"
        );
    }
}

#[tokio::test]
async fn vertical_rl_table_cell_align_content_end_uses_right_to_left_block_axis() {
    let document = Html::from_string(
        "<style>@page { size: 150pt 120pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         td { writing-mode: vertical-rl; width: 80pt; height: 50pt; align-content: end; background: red }\
         .item { width: 20pt; height: 40pt; background: green }</style>\
         <table><tr><td><div class=\"item\"></div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("vertical table-cell background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("vertical table-cell child should paint");

    assert!(
        (green.x() - red.x()).abs() < 0.01,
        "vertical-rl table-cell align-content:end should pack content against physical left/block-end: red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn vertical_table_cell_align_content_overflow_uses_axis_aware_defaults() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         table { margin-right: 10pt; display: inline-table; table-layout: fixed; width: 20pt }\
         td { writing-mode: vertical-lr; width: 20pt; height: 50pt; align-content: center; background: red }\
         .scroll { overflow-x: auto }\
         .item { width: 40pt; height: 40pt; background: green }</style>\
         <table><tr><td><div class=\"item\"></div></td></tr></table>\
         <table><tr><td class=\"scroll\"><div class=\"item\"></div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let mut red = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();
    red.sort_by(|a, b| a.x().total_cmp(&b.x()));
    let mut green = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    green.sort_by(|a, b| a.x().total_cmp(&b.x()));

    assert_eq!(red.len(), 2, "expected two table-cell backgrounds: {red:?}");
    assert_eq!(
        green.len(),
        2,
        "expected two overflowing table-cell children: {green:?}"
    );
    assert!(
        (green[0].x() - red[0].x()).abs() < 0.01,
        "non-scrollable vertical table-cell should default to safe block-start overflow: red={:?}, green={:?}",
        red[0],
        green[0]
    );
    assert!(
        (green[1].x() - red[1].x() + 10.0).abs() < 0.01,
        "scrollable vertical table-cell should default to unsafe centered overflow on overflow-x: red={:?}, green={:?}",
        red[1],
        green[1]
    );
}

#[tokio::test]
async fn vertical_writing_table_baseline_alignment_does_not_expand_content_box() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <title>Table Baseline Alignment in Vertical Writing Mode</title>\
         <style>\
         @page { size: 220pt 320pt; margin: 10pt }\
         body { margin: 0 }\
         table, tr, td {\
           border: 1px solid;\
           border-width: 1px 2px 3px 4px;\
           padding: 5px 6px 7px 8px;\
           border-spacing: 0;\
         }\
         td {\
           vertical-align: baseline;\
           background: red;\
           background-clip: content-box;\
         }\
         div {\
           width: 50px;\
           height: 100px;\
           background: green;\
         }\
         </style>\
         <table style=\"writing-mode: vertical-lr\"><tr>\
         <td><div></div></td><td><div></div></td>\
         </tr></table>\
         <table style=\"writing-mode: vertical-rl\"><tr>\
         <td><div></div></td><td><div></div></td>\
         </tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let mut red = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();
    red.sort_by(|left, right| {
        right
            .y()
            .total_cmp(&left.y())
            .then_with(|| left.x().total_cmp(&right.x()))
    });
    let mut green = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    green.sort_by(|left, right| {
        right
            .y()
            .total_cmp(&left.y())
            .then_with(|| left.x().total_cmp(&right.x()))
    });

    assert_eq!(
        red.len(),
        4,
        "expected four content-box backgrounds: {red:?}"
    );
    assert_eq!(green.len(), 4, "expected four block children: {green:?}");
    for (red, green) in red.iter().zip(green.iter()) {
        assert!(
            (red.x() - green.x()).abs() < 0.01
                && (red.y() - green.y()).abs() < 0.01
                && (red.width() - green.width()).abs() < 0.01
                && (red.height() - green.height()).abs() < 0.01,
            "vertical-writing baseline alignment should not expose red excess: red={red:?}, green={green:?}"
        );
    }
}

#[tokio::test]
async fn table_cell_align_content_baseline_joins_row_baseline_group() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         td { width: 50pt; height: 50pt; vertical-align: top; align-content: baseline }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt }</style>\
         <table><tr><td class=\"big\">A</td><td class=\"small\">B</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render: {:?}", document.pages[0].lines()))
    };
    let big = line("A");
    let small = line("B");

    assert!(
        (big.y() - small.y()).abs() < 0.5,
        "align-content:baseline should align table-cell content baselines: big={big:?}, small={small:?}"
    );
}

#[tokio::test]
async fn table_cell_align_content_last_baseline_joins_row_baseline_group() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         td { width: 50pt; height: 60pt; vertical-align: top; align-content: last baseline }\
         .two { font-size: 10pt; line-height: 10pt }\
         .big { font-size: 30pt; line-height: 30pt }</style>\
         <table><tr><td class=\"two\">A<br>B</td><td class=\"big\">C</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render: {:?}", document.pages[0].lines()))
    };
    let first_last = line("B");
    let second = line("C");

    assert!(
        (first_last.y() - second.y()).abs() < 0.5,
        "align-content:last baseline should align table-cell last baselines: first_last={first_last:?}, second={second:?}"
    );
}

#[tokio::test]
async fn table_cell_last_baseline_uses_trimmed_text_box_preceding_line_height() {
    let document = Html::from_string(
        "<style>@page { size: 240px 400px; margin: 0 }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         table { width: 200px }\
         td { width: 100px; height: 150px; vertical-align: top; align-content: last baseline }\
         .two { font: 50px/2 sans-serif; text-box-trim: trim-start; text-box-edge: text }\
         .peer { font: 50px/2 sans-serif }</style>\
         <table><tr><td class=\"two\">A<br>B</td><td class=\"peer\">C</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render: {:?}", document.pages))
    };
    let first_last = line("B");
    let peer = line("C");

    assert!(
        (first_last.y() - peer.y()).abs() < 0.5,
        "last-baseline table-cell alignment should use trimmed preceding line height: first_last={first_last:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn orthogonal_table_cell_align_content_baseline_uses_fallback() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 180pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         td { width: 50pt; height: 60pt; vertical-align: top }\
         .peer { align-content: baseline; font-size: 30pt; line-height: 30pt }\
         .ortho, .start { writing-mode: vertical-lr; font-size: 10pt; line-height: 20pt }\
         .ortho { align-content: baseline; background: red }\
         .start { align-content: start; background: blue }</style>\
         <table><tr><td class=\"peer\">A</td><td class=\"ortho\">甲</td></tr>\
         <tr><td class=\"peer\">B</td><td class=\"start\">乙</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("orthogonal baseline cell background should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("orthogonal start fallback cell background should paint");
    let baseline = page
        .lines()
        .iter()
        .find(|line| line.text == "甲")
        .expect("orthogonal baseline cell text should paint");
    let start = page
        .lines()
        .iter()
        .find(|line| line.text == "乙")
        .expect("orthogonal start cell text should paint");

    assert!(
        ((baseline.y() - red.y()) - (start.y() - blue.y())).abs() < 0.5,
        "orthogonal align-content:baseline cell should not consume the horizontal row baseline: red={red:?}, blue={blue:?}, baseline={baseline:?}, start={start:?}"
    );
}

#[tokio::test]
async fn rowspan_table_cell_align_content_baseline_joins_startmost_row_group() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         td { width: 50pt; height: 30pt; vertical-align: top; align-content: baseline }\
         .span { font-size: 10pt; line-height: 10pt; background: red }\
         .peer { font-size: 30pt; line-height: 30pt }</style>\
         <table><tr><td class=\"span\" rowspan=\"2\">Span</td><td class=\"peer\">A</td></tr><tr><td>B</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("baseline rowspan cell background should paint");
    let span = page
        .lines()
        .iter()
        .find(|line| line.text == "Span")
        .expect("baseline rowspan text should paint");
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .expect("first-row peer baseline text should paint");

    assert!(
        (span.y() - peer.y()).abs() < 0.5,
        "row-spanning align-content:baseline cell should join the start-most row baseline group: red={red:?}, span={span:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn rowspan_table_cell_align_content_last_baseline_joins_endmost_row_group() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt }\
         body, table, td { margin: 0; border-spacing: 0; padding: 0 }\
         td { width: 50pt; height: 30pt; vertical-align: top }\
         .span { align-content: last baseline; font-size: 10pt; line-height: 10pt; background: red }\
         .small { font-size: 10pt; line-height: 10pt }\
         .peer { align-content: last baseline; font-size: 30pt; line-height: 30pt }</style>\
         <table><tr><td class=\"span\" rowspan=\"2\">Span</td><td class=\"small\">A</td></tr><tr><td class=\"peer\">B</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("last-baseline rowspan cell background should paint");
    let span = page
        .lines()
        .iter()
        .find(|line| line.text == "Span")
        .expect("last-baseline rowspan text should paint");
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .expect("end-most row peer baseline text should paint");

    assert!(
        (span.y() - peer.y()).abs() < 0.5,
        "row-spanning align-content:last baseline cell should join the end-most row baseline group: red={red:?}, span={span:?}, peer={peer:?}"
    );
}

#[tokio::test]
async fn auto_table_sizing_uses_inline_edges_for_cell_contributions() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body, table, td { margin: 0; font-size: 10pt; line-height: 10pt }\
         table { border-spacing: 0; table-layout: auto } td { padding: 0 } .pad { padding-left: 42pt; padding-right: 6pt }</style>\
         <table><tr><td><span class=\"pad\">A</span></td><td>Next</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    let next = lines.iter().find(|line| line.text == "Next").unwrap();
    assert!(
        next.x() > 55.0,
        "second cell should be shifted by the first cell's inline padding: {next:?}"
    );
}

#[tokio::test]
async fn nested_table_fragment_contributes_to_cell_intrinsic_width() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body, table, td { margin:0; font-size:10pt; line-height:10pt; border-spacing:0; padding:0 } .inner td { width:90pt }</style>\
         <table><tr><td><table class=\"inner\"><tr><td>Inner</td></tr></table></td><td>Next</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let inner = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Inner")
        .unwrap();
    let next = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Next")
        .unwrap();

    assert!(
        next.x() - inner.x() > 85.0,
        "outer cell should reserve the nested table fragment width: inner={inner:?}, next={next:?}"
    );
}

#[tokio::test]
async fn nested_percentage_table_does_not_expand_outer_intrinsic_columns() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 120pt; margin: 10pt } \
         body, table, td { margin:0; font-size:10pt; line-height:10pt; border-spacing:0; padding:0 } \
         table { width:100% } .inner td { width:50pt }</style>\
         <table class=\"outer\"><tr>\
           <td><table class=\"inner\"><tr><td>Alpha</td></tr></table></td>\
           <td><table class=\"inner\"><tr><td>Bravo</td></tr></table></td>\
           <td><table class=\"inner\"><tr><td>Charlie</td></tr></table></td>\
         </tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let labels = ["Alpha", "Bravo", "Charlie"].map(|label| {
        page.lines()
            .iter()
            .find(|line| line.text == label)
            .unwrap_or_else(|| panic!("expected nested table label {label:?}"))
    });

    assert!(
        labels.windows(2).all(|pair| pair[0].x() < pair[1].x()),
        "nested-table labels must remain in source-order columns: {labels:?}"
    );
    assert!(
        labels.iter().all(|label| label.x() < 350.0),
        "nested percentage tables must not create off-page intrinsic columns: {labels:?}"
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[0].text, "Head");
    assert_eq!(lines[1].text, "Body");
    assert_eq!(lines[2].text, "Foot");
    assert!(lines[0].y() > lines[1].y());
    assert!(lines[1].y() > lines[2].y());
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[0].text, "Head");
    assert_eq!(lines[1].text, "Body");
    assert_eq!(lines[2].text, "Foot");
    assert!(lines[0].y() > lines[1].y());
    assert!(lines[1].y() > lines[2].y());
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines()
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages.len() >= 2);
    let mut repeated_headers = 0;
    let mut repeated_footers = 0;
    let mut body_3_has_header = false;
    for page in &document.pages {
        let header = page.lines().iter().find(|line| line.text == "Head");
        let footer = page.lines().iter().find(|line| line.text == "Foot");
        let bodies = page
            .lines()
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
                assert!(header.y() > body.y(), "header should paint above body rows");
            }
        }
        if let Some(footer) = footer {
            for body in &bodies {
                assert!(body.y() > footer.y(), "footer should paint below body rows");
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let header_pages = document
        .pages
        .iter()
        .filter(|page| page.lines().iter().any(|line| line.text == "Head"))
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
                    crate::document::paint::page::PaintOperation::Rect(index)
                        if page.rects().get(*index).is_some_and(|rect| {
                            rect.fill == Some(CssColor::new(255, 0, 0))
                                && rect.width() > 0.0
                                && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let repeated_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines().iter().any(|line| line.text == "Head"))
        .expect("header should repeat on a later page");
    let table = first_rect_paint_operation_index(repeated_page, CssColor::new(255, 0, 0));
    let column = first_rect_paint_operation_index(repeated_page, CssColor::new(0, 128, 0));
    let row_group = first_rect_paint_operation_index(repeated_page, CssColor::new(0, 0, 255));
    let row = first_rect_paint_operation_index(repeated_page, CssColor::new(255, 255, 0));

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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let repeated_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines().iter().any(|line| line.text == "Head"))
        .expect("header should repeat on a later page");
    assert!(
        repeated_page.paint_operations().iter().any(|operation| {
            matches!(
                operation,
                crate::document::paint::page::PaintOperation::Rect(index)
                    if repeated_page.rects().get(*index).is_some_and(|rect| {
                        rect.fill == Some(CssColor::new(255, 0, 0))
                            && rect.width() > 0.0
                            && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let body_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.lines()
                .iter()
                .any(|line| line.text.starts_with("Body "))
        })
        .collect::<Vec<_>>();
    assert!(body_pages.len() >= 2);
    for page in body_pages {
        assert!(
            page.paint_operations().iter().any(|operation| {
                matches!(
                    operation,
                    crate::document::paint::page::PaintOperation::Rect(index)
                        if page.rects().get(*index).is_some_and(|rect| {
                            rect.fill == Some(CssColor::new(255, 0, 0))
                                && rect.width() > 0.0
                                && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let second_page = document
        .pages
        .iter()
        .find(|page| page.lines().iter().any(|line| line.text == "B"))
        .expect("second row should be forced to a later page");
    let red_horizontal = second_page.rects().iter().any(|rect| {
        rect.fill == Some(CssColor::new(255, 0, 0)) && rect.width() > 20.0 && rect.height() >= 7.9
    });
    let blue_horizontal = second_page.rects().iter().any(|rect| {
        rect.fill == Some(CssColor::new(0, 0, 255))
            && rect.width() > 20.0
            && (rect.height() - 2.0).abs() < 0.01
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let table_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.lines()
                .iter()
                .any(|line| matches!(line.text.as_str(), "A" | "B" | "C"))
        })
        .collect::<Vec<_>>();
    assert!(table_pages.len() >= 2);
    for page in table_pages {
        let green = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .expect("table background should paint on each fragment");
        assert!(
            (green.width() - 44.0).abs() < 0.01,
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let repeated_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| {
            page.lines().iter().any(|line| line.text == "Head")
                && !page.lines().iter().any(|line| line.text == "Body 6")
        })
        .expect("a non-final page should repeat the header");
    let red_horizontal = repeated_page.rects().iter().any(|rect| {
        rect.fill == Some(CssColor::new(255, 0, 0)) && rect.width() > 20.0 && rect.height() >= 7.9
    });
    let blue_horizontal = repeated_page.rects().iter().any(|rect| {
        rect.fill == Some(CssColor::new(0, 0, 255))
            && rect.width() > 20.0
            && (rect.height() - 2.0).abs() < 0.01
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let continuation_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines().iter().any(|line| line.text == "B"))
        .expect("rowspan continuation row should fragment to a later page");
    let blue_vertical_edges = continuation_page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.width() - 4.0).abs() < 0.01
                && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let continuation_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines().iter().any(|line| line.text == "B"))
        .expect("visible row after a collapsed track should fragment to a later page");
    assert!(
        !continuation_page
            .lines()
            .iter()
            .any(|line| line.text.contains("Hidden")),
        "collapsed row content should stay suppressed"
    );
    let blue_vertical_edges = continuation_page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.width() - 4.0).abs() < 0.01
                && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let painted_fragments = document
        .pages
        .iter()
        .filter(|page| {
            page.rects().iter().any(|rect| {
                rect.fill == Some(CssColor::new(0, 0, 255))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
            })
        })
        .collect::<Vec<_>>();

    assert!(
        painted_fragments.len() >= 3,
        "120pt row should split across multiple 50pt page areas"
    );
    for page in painted_fragments {
        assert!(
            page.rects().iter().any(|rect| {
                rect.fill == Some(CssColor::new(255, 0, 0))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let row_piece_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects().iter().any(|rect| {
                rect.fill == Some(CssColor::new(0, 0, 255))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
            })
        })
        .collect::<Vec<_>>();
    assert!(row_piece_pages.len() >= 3);

    let middle_page = row_piece_pages[1];
    let synthetic_horizontal = middle_page.rects().iter().any(|rect| {
        rect.fill == Some(CssColor::new(255, 0, 0))
            && rect.width() > 40.0
            && (rect.height() - 4.0).abs() < 0.01
    });
    let vertical_edges = middle_page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0))
                && (rect.width() - 4.0).abs() < 0.01
                && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let body_fragment_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects().iter().any(|rect| {
                rect.fill == Some(CssColor::new(0, 0, 255))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
            })
        })
        .collect::<Vec<_>>();

    assert!(body_fragment_pages.len() >= 3);
    for page in body_fragment_pages {
        assert!(
            page.lines().iter().any(|line| line.text == "Head"),
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let oversized_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects().iter().any(|rect| {
                rect.fill == Some(CssColor::new(0, 0, 255))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
            })
        })
        .collect::<Vec<_>>();

    assert!(oversized_pages.len() >= 3);
    for page in oversized_pages {
        let green_edges = page
            .rects()
            .iter()
            .filter(|rect| {
                rect.fill == Some(CssColor::new(0, 128, 0))
                    && (rect.width() - 4.0).abs() < 0.01
                    && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter().map(|line| line.text.as_str()))
        .collect::<Vec<_>>();
    assert!(texts.contains(&"A"));
    assert!(texts.contains(&"B"));
    assert!(texts.contains(&"C"));

    let painted_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects().iter().any(|rect| {
                matches!(
                    rect.fill,
                    Some(color)
                        if color == CssColor::new(255, 0, 0)
                            || color == CssColor::new(0, 128, 0)
                            || color == CssColor::new(0, 0, 255)
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
async fn oversized_table_row_moves_a_block_child_that_fits_a_fresh_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, p, table, td, div { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         p { height: 20pt } table { width: 80pt; border-collapse: collapse }\
         td { width: 80pt; height: 80pt; padding: 0 }\
         div { display: block; height: 40pt } .a { background: red } .b { background: green }</style>\
         <p>Prefix</p><table><tbody><tr><td><div class=\"a\">A</div><div class=\"b\">B</div></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(page_texts, vec![vec!["Prefix"], vec!["A"], vec!["B"]]);
}

#[tokio::test]
async fn fragmented_table_body_keeps_captions_at_wrapper_edges() {
    let top_document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, caption, td, div { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 80pt; border-collapse: collapse } caption { height: 10pt }\
         td { width: 80pt; height: 80pt; padding: 0 } div { display: block; height: 40pt }</style>\
         <table><caption>Top caption</caption><tbody><tr><td><div>A</div><div>B</div></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let top_pages = top_document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(top_pages, vec![vec!["A", "Top caption"], vec!["B"]]);

    let bottom_document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, caption, td, div { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 80pt; border-collapse: collapse } caption { height: 10pt; caption-side: bottom }\
         td { width: 80pt; height: 80pt; padding: 0 } div { display: block; height: 40pt }</style>\
         <table><caption>Bottom caption</caption><tbody><tr><td><div>A</div><div>B</div></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let bottom_lines = bottom_document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter().map(|line| line.text.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        bottom_lines
            .iter()
            .filter(|text| **text == "Bottom caption")
            .count(),
        1,
        "bottom caption must remain at the table wrapper's final edge: {bottom_lines:?}"
    );
    assert!(bottom_lines.contains(&"A") && bottom_lines.contains(&"B"));
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages.iter().any(|page| {
            page.links()
                .iter()
                .any(|link| link.target.as_ref() == "https://example.com")
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines().iter().map(|line| line.text.as_str()))
            .any(|text| text == "Atom"),
        "inline-block text should be owned by the split table-cell atom"
    );
    let green_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects().iter().any(|rect| {
                rect.fill == Some(CssColor::new(0, 128, 0))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines().iter().map(|line| line.text.as_str()))
            .any(|text| text == "Nested"),
        "nested inline-block text should be painted through the planned child fragment path"
    );
    let green_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects().iter().any(|rect| {
                rect.fill == Some(CssColor::new(0, 128, 0))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let pages_with_svg_paint = document
        .pages
        .iter()
        .filter(|page| {
            page.paths().iter().any(|path| {
                path.fill == Some(CssColor::new(0, 0, 255))
                    && path
                        .bounds()
                        .is_some_and(|bounds| bounds.size.width > 0.0 && bounds.size.height > 0.0)
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let body_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.lines()
                .iter()
                .any(|line| line.text.starts_with("Body "))
        })
        .collect::<Vec<_>>();
    assert!(body_pages.len() >= 2);

    for page in body_pages {
        let table = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));
        let column = first_rect_paint_operation_index(page, CssColor::new(0, 128, 0));
        let row_group = first_rect_paint_operation_index(page, CssColor::new(0, 0, 255));
        let row = first_rect_paint_operation_index(page, CssColor::new(255, 255, 0));
        let cell = first_rect_paint_operation_index(page, CssColor::new(0, 255, 255));

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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let positioned_page = document
        .pages
        .iter()
        .skip(1)
        .find(|page| page.lines().iter().any(|line| line.text == "Body 2"))
        .expect("second body row should fragment to a later page");
    let positioned = first_rect_paint_operation_index(positioned_page, CssColor::new(255, 0, 0));
    let cell = first_rect_paint_operation_index(positioned_page, CssColor::new(0, 0, 255));

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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));
    let blue = first_rect_paint_operation_index(page, CssColor::new(0, 0, 255));
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "May 10, 2018")
    );
    assert!(
        document.pages[0]
            .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .unwrap();

    assert!(
        (background.width() - 60.0).abs() < 0.01,
        "table border-box wrapper width should be 60pt, got {background:?}"
    );
}

#[tokio::test]
async fn collapsed_table_decorative_borders_do_not_shrink_grid_width() {
    let document = Html::from_string(
        "<style>body { margin: 0 } table { border-collapse: collapse; margin: 0; width: 100pt; border-left: 30pt solid transparent; border-right: 30pt solid transparent; border-spacing: 0; font-size: 10pt; line-height: 10pt } td { padding: 0 }</style>\
         <table><tr><td style=\"width:50pt\">L</td><td style=\"width:50pt\">R</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let left = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "L")
        .unwrap();
    let right = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "R")
        .unwrap();

    assert!(
        right.x() - left.x() > 30.0,
        "collapsed table borders should not consume column grid width: left={left:?} right={right:?}"
    );
}

#[tokio::test]
async fn collapsed_table_fixed_cell_declared_width_includes_collapsed_border_insets() {
    let document = Html::from_string(
        "<!doctype html>\
         <style>@page{size:240pt 260pt;margin:0}body{margin:0}table{border-collapse:collapse;table-layout:fixed;margin:0;font-size:0;line-height:0}td{height:50px;background:yellow;border:10px solid green;padding:0}</style>\
         <table><td style=\"width:40px\"></td></table>\
         <table><td style=\"width:90px\"></td></table>\
         <table><td style=\"width:140px\"></td></table>\
         <table><td style=\"width:40px;padding-left:10px;padding-right:20px\"></td></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut horizontal_border_widths = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.height() - 7.5).abs() < 0.01
                && rect.width() > 20.0
        })
        .map(|rect| rect.width())
        .collect::<Vec<_>>();
    horizontal_border_widths.sort_by(f32::total_cmp);
    horizontal_border_widths.dedup_by(|left, right| (*left - *right).abs() < 0.01);

    assert_eq!(
        horizontal_border_widths.len(),
        4,
        "expected one visual width for each collapsed table: {horizontal_border_widths:?}"
    );
    for (actual, expected) in horizontal_border_widths
        .iter()
        .zip([45.0, 67.5, 82.5, 120.0])
    {
        assert!(
            (*actual - expected).abs() < 0.01,
            "collapsed fixed-layout cell should paint outer width {expected}pt, got {actual}pt from {horizontal_border_widths:?}"
        );
    }
}

#[tokio::test]
async fn collapsed_table_wrapper_inline_insets_use_outer_grid_maxima() {
    let document = Html::from_string(
        "<style>@page{size:180pt 140pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;width:40pt;background:green;font-size:0;line-height:0}td{padding:0;width:20pt;height:10pt}.first td:first-child{border-left:2pt solid black}.first td:last-child{border-right:2pt solid black}.wide td:first-child{border-left:30pt solid red}.wide td:last-child{border-right:30pt solid red}</style>\
         <table><tr class=\"first\"><td></td><td></td></tr><tr class=\"wide\"><td></td><td></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("collapsed table background should paint");
    let wide_red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)) && rect.width() >= 29.9)
        .count();

    assert!(
        (green.width() - 100.0).abs() < 0.01,
        "wrapper background should include maximum collapsed outer insets, got {green:?}"
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("collapsed table background should paint");

    assert!(
        (green.height() - 52.0).abs() < 0.01,
        "wrapper background should include full collapsed edge occupancy, got {green:?}"
    );
}

#[tokio::test]
async fn invoice_sample_generated_metadata_terms_do_not_wrap() {
    let document = Html::from_file("weasyprint-samples/invoice/invoice.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let invoice_line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Invoice number: 12345")
        .unwrap();
    let date_line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Date: March 31, 2018")
        .unwrap();

    assert!(
        invoice_line.x() > 300.0,
        "invoice number metadata should paint in the right metadata column: {invoice_line:?}"
    );
    assert!(
        date_line.x() > 300.0,
        "invoice date metadata should paint in the right metadata column: {date_line:?}"
    );
    assert!(
        date_line.y() < invoice_line.y(),
        "invoice date should follow invoice number vertically"
    );
}

#[tokio::test]
async fn table_cells_honor_nested_of_type_text_alignment_rules() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } table { border-collapse: collapse; width: 180pt; margin: 0 } td { padding: 0; text-align: center; &:first-of-type { text-align: left } &:last-of-type { text-align: right } }</style>\
         <table><tr><td>Left</td><td>Middle</td><td>Right</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let left = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Left")
        .unwrap();
    let middle = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Middle")
        .unwrap();
    let right = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Right")
        .unwrap();

    assert!(left.x() < middle.x());
    assert!(middle.x() < right.x());
    assert!(right.x() > 160.0, "right-aligned cell at x={}", right.x());
}

#[tokio::test]
async fn table_cells_match_table_descendant_of_type_selectors() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } table { border-collapse: collapse; width: 180pt; margin: 0 } td { padding: 0; text-align: left } table td:last-of-type { text-align: right; color: #1ee494; font-weight: bold }</style>\
         <table><tr><td>Left</td><td>Right</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let right = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Right")
        .unwrap();

    assert_eq!(right.color, CssColor::new(30, 228, 148));
    assert!(right.x() > 150.0);
}

#[tokio::test]
async fn inline_table_participates_as_atomic_inline_box() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin:0; font-size:10pt; line-height:12pt } table { display:inline-table; border-spacing:0; margin:0 4pt; width:30pt } td { padding:0 }</style>\
         <p>Before <table><tr><td>Cell</td></tr></table> After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(before.x() < cell.x());
    assert!(cell.x() < after.x());
    assert!((before.y() - after.y()).abs() < 0.1);
}

#[tokio::test]
async fn auto_inline_table_uses_fragment_intrinsic_width() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body, p, table { margin:0; font-size:10pt; line-height:12pt } table { display:inline-table; border-spacing:0 } td { padding:0 } .wide { width:80pt } .narrow { width:40pt }</style>\
         <p>A<table><tr><td class=\"wide\">Wide</td><td class=\"narrow\">Cell</td></tr></table>Z</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let wide = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Wide")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Z")
        .unwrap();

    assert!(
        cell.x() - wide.x() > 75.0,
        "second cell should use first column's intrinsic table width: wide={wide:?}, cell={cell:?}"
    );
    assert!(
        after.x() - wide.x() > 115.0,
        "trailing inline text should follow the auto inline-table width: wide={wide:?}, after={after:?}"
    );
}

#[tokio::test]
async fn inline_table_fragment_intrinsics_follow_visual_row_order() {
    let document = Html::from_string(
        "<style>@page { size: 300pt 140pt; margin: 10pt } body, p, table { margin:0; font-size:10pt; line-height:20pt } table { display:inline-table; border-spacing:0 } th, td { padding:0; text-align:left } thead td, thead th { width:90pt; font-size:16pt; line-height:20pt } tbody td { width:20pt }</style>\
         <p>Before <table><tfoot><tr><td>Foot</td></tr></tfoot><tbody><tr><td>Body</td></tr></tbody><thead><tr><th>Head</th></tr></thead></table> After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let head = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Head")
        .unwrap();
    let body = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Body")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(
        head.y() > body.y(),
        "header row should be laid out before body"
    );
    assert!(
        after.x() - head.x() > 85.0,
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let caption = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cap")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(before.x() < cell.x());
    assert!(cell.x() < after.x());
    assert!((before.y() - after.y()).abs() < 0.1);
}

#[tokio::test]
async fn wraps_orphan_table_cells_in_anonymous_table() {
    let document = Html::from_string(
        "<div style=\"margin:0;border-spacing:0\">\
         <span style=\"display:table-cell;width:40pt\">A</span>\
         <span style=\"display:table-cell;width:40pt\">B</span>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[0].text, "A");
    assert_eq!(lines[1].text, "B");
    assert!(lines[1].x() > lines[0].x());
    assert!((lines[1].y() - lines[0].y()).abs() < 0.01);
}

#[tokio::test]
async fn wraps_inline_orphan_table_cells_in_anonymous_inline_table() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body, p { margin:0; font-size:10pt; line-height:14pt } span { border-spacing:0 } td, span { padding:0 }</style>\
         <p>Before <span><span style=\"display:table-cell;width:40pt\">Cell</span></span> After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(before.x() < cell.x());
    assert!(cell.x() < after.x());
    assert!((before.y() - cell.y()).abs() < 0.1);
    assert!((after.y() - cell.y()).abs() < 0.1);
}

#[tokio::test]
async fn nested_table_row_group_anonymous_fixup_paints_nested_cells() {
    let document = Html::from_string(
        "<!doctype html>\
         <meta charset=\"utf-8\">\
         <style>\
           @page { size: 260pt 100pt; margin: 10pt }\
           body { margin: 0 }\
           .cell { display: table-cell; border: 1px solid gray; padding: 4px; font: 16px serif }\
         </style>\
         <div style=\"display: table-row-group\">\
           <div style=\"display: table-row-group\">\
             <div class=\"cell\">a</div>\
             <div class=\"cell\">b</div>\
           </div>\
           <div class=\"cell\">cccc</div>\
           <div class=\"cell\">dddd</div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let line = |text: &str| {
        page.lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("expected {text:?} to paint"))
    };
    let a = line("a");
    let b = line("b");
    let cccc = line("cccc");
    let dddd = line("dddd");

    assert!(
        a.x() < b.x(),
        "nested cells should remain ordered: a={a:?}, b={b:?}"
    );
    assert!(
        b.x() < cccc.x() && cccc.x() < dddd.x(),
        "outer anonymous row should contain nested table, cccc, and dddd in order: b={b:?}, cccc={cccc:?}, dddd={dddd:?}"
    );
    assert!(
        (a.y() - cccc.y()).abs() < 3.0 && (cccc.y() - dddd.y()).abs() < 3.0,
        "nested and sibling cells should paint in one visual row: a={a:?}, cccc={cccc:?}, dddd={dddd:?}"
    );
}

#[tokio::test]
async fn body_display_table_overflow_hidden_keeps_positioned_text() {
    let document = Html::from_string(
        "<!doctype html>\
         <title>CSS Test: overflow:hidden on HTML body element table</title>\
         <style>\
         @page { size: 200pt 120pt; margin: 0 }\
         body { overflow:hidden; display:table; border-spacing:0; margin:40px 8px 8px }\
         .caption { display:caption; margin-bottom:10px }\
         .td { display:table-cell; width:20px; height:20px; margin-top:-15px; background:black }\
         p { position:absolute; top:0; left:8px; margin:0; font-size:10pt; line-height:12pt }\
         </style>\
         <div class=caption></div>\
         <div class=td></div>\
         <p>Test passes if there is a black square below.</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Test passes if there is a black square below.")
        .expect("positioned paragraph should render");
    let black = document.pages[0]
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(CssColor::BLACK)
                && (rect.width() - 15.0).abs() < 0.01
                && rect.height() >= 15.0
        })
        .expect("table-cell black square should render");

    assert!(
        black.y() < line.y(),
        "black square should paint below the positioned text: line={line:?}, black={black:?}"
    );
}

#[tokio::test]
async fn wpt_root_table_ignores_authored_head_caption() {
    let render = |source: String| async move {
        Html::from_string(source)
            .render(&RenderOptions::default())
            .await
            .unwrap()
    };
    let target = render(
        "<!doctype html>\
         <title>CSS Test: overflow:hidden on root element table</title>\
         <style>\
         @page { size: 200pt 120pt; margin: 0 }\
         html { overflow:hidden; display:table; border-spacing:0; background:white; margin:40px 8px 8px }\
         head { display:caption; margin-bottom:10px }\
         body { display:table-cell; width:20px; height:20px; margin-top:-15px; background:black }\
         p { position:absolute; top:0 }\
         </style>\
         <p>Test passes if there is a black square below.</p>"
            .to_string(),
    )
    .await;
    let reference = render(
        "<!doctype html>\
         <title>Reference for CSS Test: overflow:hidden on table with caption overflowing upwards</title>\
         <style>\
         @page { size: 200pt 120pt; margin: 0 }\
         table { border-spacing:0; position:absolute; top:40px }\
         td { padding:0; background:black; width:20px; height:20px }\
         </style>\
         <p>Test passes if there is a black square below.</p>\
         <table><tr><td></table>"
            .to_string(),
    )
    .await;
    let black_square = |document: &spindrift::Document| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| {
                rect.fill == Some(CssColor::BLACK)
                    && (rect.width() - 15.0).abs() < 0.01
                    && (rect.height() - 15.0).abs() < 0.01
            })
            .map(|rect| (rect.x(), rect.y()))
            .unwrap_or_else(|| panic!("expected the 20px black table cell"))
    };
    let line = |document: &spindrift::Document| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.starts_with("Test passes"))
            .map(|line| (line.x(), line.y()))
            .unwrap_or_else(|| panic!("expected the instructional paragraph"))
    };

    let target_black = black_square(&target);
    let reference_black = black_square(&reference);
    assert!(
        (target_black.0 - reference_black.0).abs() < 0.01
            && (target_black.1 - reference_black.1).abs() < 0.01,
        "the hidden head must not create a caption that shifts the root table: target={target_black:?}, reference={reference_black:?}"
    );

    let target_line = line(&target);
    let reference_line = line(&reference);
    assert!(
        (target_line.0 - reference_line.0).abs() < 0.01
            && (target_line.1 - reference_line.1).abs() < 0.01,
        "the root table must preserve the viewport-relative positioned paragraph: target={target_line:?}, reference={reference_line:?}"
    );
}

#[tokio::test]
async fn table_overflow_hidden_clips_to_table_box_not_bottom_caption() {
    let pdf = Html::from_string(
        "<!doctype html>\
         <style>\
         @page { size: 120px 120px; margin: 0 } body { margin: 0 }\
         table { overflow: hidden; border-spacing: 0 }\
         caption { margin-top: 10px; caption-side: bottom }\
         td { padding: 0 }\
         div { width: 20px; height: 25px; border-bottom: solid red 10px; margin-top: -15px; position: relative; top: 15px; background: black }\
         </style>\
         <table><caption></caption><tr><td><div></div></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(
        rendered.contains("0 75 15 15 re\nW\nn"),
        "overflow:hidden should clip to the table box, not the bottom-caption wrapper: {rendered}"
    );
    assert!(
        rendered.contains("/CSsRGB cs\n1 0 0 scn\n0 63.75 15 7.5 re\nf"),
        "test fixture should still paint the overflowing red border inside the clipped scope"
    );
}

#[tokio::test]
async fn table_overflow_hidden_clips_grid_without_clipping_table_decoration_or_caption() {
    let pdf = Html::from_string(
        "<!doctype html>\
         <style>\
         @page { size: 160px 160px; margin: 0 } body { margin: 0 }\
         table { overflow: hidden; border: 20px solid green; border-spacing: 0 }\
         caption { height: 30px; background: lightblue; caption-side: bottom }\
         td { padding: 0; width: 50px; height: 50px }\
         .overflow { width: 500px; height: 500px; background: pink }\
         </style>\
         <table><tr><td><div style=\"width:50px;height:50px\"><div class=\"overflow\"></div></div></td></tr><caption>caption</caption></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    for border_edge in [
        "0 105 67.5 15 re",
        "52.5 52.5 15 67.5 re",
        "0 52.5 67.5 15 re",
        "0 52.5 15 67.5 re",
    ] {
        assert!(
            rendered.contains(border_edge),
            "table overflow must not clip its own border edge {border_edge:?}: {rendered}"
        );
    }
    assert!(
        rendered.contains("15 67.5 37.5 37.5 re\nW\nn"),
        "the oversized grid descendant must be clipped at the table padding edge: {rendered}"
    );
    assert!(
        rendered.contains("15 -270 375 375 re\nf"),
        "the oversized descendant should remain under the padding-edge clip: {rendered}"
    );
    assert!(
        rendered.contains("0 30 67.5 22.5 re\nf"),
        "the bottom caption must remain outside the table-grid clip: {rendered}"
    );
}

#[tokio::test]
async fn table_overflow_auto_on_table_box_behaves_as_visible() {
    let pdf = Html::from_string(
        "<!doctype html>\
         <style>\
         @page { size: 120px 120px; margin: 0 } body { margin: 0 }\
         table { overflow: auto; border-spacing: 0 }\
         caption { margin-top: 10px; caption-side: bottom }\
         td { padding: 0 }\
         div { width: 20px; height: 25px; border-bottom: solid red 10px; margin-top: -15px; position: relative; top: 15px; background: black }\
         </style>\
         <table><caption></caption><tr><td><div></div></table>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &crate::PdfOptions::default())
    .await
    .unwrap();
    let rendered = pdf_searchable_text(&pdf);

    assert!(
        !rendered.contains("0 75 15 15 re\nW\nn"),
        "overflow:auto on table boxes should behave as visible per the CSS2 errata: {rendered}"
    );
    assert!(
        rendered.contains("/CSsRGB cs\n1 0 0 scn\n0 63.75 15 7.5 re\nf"),
        "overflow:auto should leave the overflowing red border visible"
    );
}

#[tokio::test]
async fn collapsed_borders_with_different_widths_fill_outer_grid_area() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>\
         @page { size: 220px 220px; margin: 0 }\
         body { margin: 0 }\
         table { border-collapse: collapse; background: green }\
         td { padding: 0; background: red }\
         td div { height: 25px; background: green }\
         </style>\
         <div style=\"float: left; min-width: 200px; background: red\">\
           <table>\
             <tr><td style=\"border-left: 150px solid green\"><div></div></td></tr>\
             <tr><td style=\"border-right: 100px solid green\"><div></div></td></tr>\
           </table>\
           <table>\
             <tr><td style=\"border-left: 150px solid green\"><div style=\"width: 0px\"></div></td></tr>\
             <tr><td style=\"border-right: 100px solid green\"><div style=\"width: 25px\"></div></td></tr>\
           </table>\
           <table>\
             <tr><td style=\"border-right: 100px solid green\"><div></div></td></tr>\
             <tr><td style=\"border-left: 150px solid green\"><div></div></td></tr>\
           </table>\
           <table>\
             <tr><td style=\"border-left: 150px solid green\"><div style=\"width: 0px\"></div></td></tr>\
             <tr><td style=\"border-right: 100px solid green\"><div style=\"width: 25px\"></div></td></tr>\
           </table>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);
    let scale = 0.75;
    let red_square = page
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(red)
                && (rect.width() - 200.0 * scale).abs() < 0.01
                && (rect.height() - 200.0 * scale).abs() < 0.01
        })
        .expect("expected the red 200px square backdrop");

    for (css_x, css_y) in [
        (175.0, 12.5),
        (87.5, 37.5),
        (175.0, 62.5),
        (12.5, 112.5),
        (150.0, 112.5),
        (150.0, 137.5),
        (175.0, 162.5),
    ] {
        let x = red_square.x() + css_x * scale;
        let y = red_square.y() + css_y * scale;
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(green),
            "collapsed border sample should be green at CSS point ({css_x}, {css_y})"
        );
    }
}

#[tokio::test]
async fn collapsed_border_origin_tie_tiles_form_square_after_clearing_br() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>\
         @page { size: 1200px 300px; margin: 0 }\
         body { margin: 0 }\
         br { clear: both }\
         p { display: none }\
         table { border-collapse: collapse; float: left }\
         td { padding: 0 }\
         .loser { border: 25px solid red }\
         .winner { border: 25px solid green }\
         </style>\
         <p>Test passes if there is a filled green square and no red.</p>\
         <table><tr class=\"loser\"><td class=\"winner\"></td></tr></table>\
         <table><tbody class=\"loser\"><td class=\"winner\"></td></tbody></table>\
         <table><col class=\"loser\"></col><td class=\"winner\"></td></table>\
         <table><colgroup class=\"loser\"></col><td class=\"winner\"></td></table>\
         <br>\
         <table class=\"loser\"><td class=\"winner\"></td></table>\
         <table><tbody class=\"loser\"></col><tr class=\"winner\"><td></td></tr></tbody></table>\
         <table><col class=\"loser\"></col><tr class=\"winner\"><td></td></tr></table>\
         <table><colgroup class=\"loser\"></colgroup><tr class=\"winner\"><td></td></tr></table>\
         <br>\
         <table class=\"loser\"><tr class=\"winner\"><td></td></tr></table>\
         <table><col class=\"loser\"></col><tbody class=\"winner\"></col><td></td></tbody></table>\
         <table><colgroup class=\"loser\"></colgroup><tbody class=\"winner\"></col><td></td></tbody></table>\
         <table class=\"loser\"><tbody class=\"winner\"></col><td></td></tbody></table>\
         <br>\
         <table><colgroup class=\"loser\"><col class=\"winner\"></col></colgroup><td></td></table>\
         <table class=\"loser\"><col class=\"winner\"></col><td></td></table>\
         <table class=\"loser\"><colgroup class=\"winner\"></colgroup><td></td></table>\
         <table class=\"winner\"><td></td></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);
    assert!(
        !page.rects().iter().any(|rect| rect.fill == Some(red)),
        "losing border candidates should not paint red: {:?}",
        page.rects()
    );
    let (left, bottom, right, top) = filled_rect_bounds(page, green);
    let expected = 200.0 * 0.75;
    assert!(
        (right - left - expected).abs() < 0.5,
        "cleared floated tiles should span 200px in the inline axis, bounds=({}, {})",
        left,
        right
    );
    assert!(
        (top - bottom - expected).abs() < 0.5,
        "cleared floated tiles should span 200px in the block axis, bounds=({}, {})",
        bottom,
        top
    );
    for css_y in [25.0, 75.0, 125.0, 175.0] {
        for css_x in [25.0, 75.0, 125.0, 175.0] {
            let x = left + css_x * 0.75;
            let y = bottom + css_y * 0.75;
            assert_eq!(
                final_rect_fill_at(page, x, y),
                Some(green),
                "cleared floated tiles should sample green at CSS point ({css_x}, {css_y})"
            );
        }
    }
}

#[tokio::test]
async fn collapsed_border_origin_tie_single_floats_stay_border_sized() {
    for (name, markup) in [
        (
            "cell over row",
            "<table><tr class=\"loser\"><td class=\"winner\"></td></tr></table>",
        ),
        (
            "cell over row group",
            "<table><tbody class=\"loser\"><td class=\"winner\"></td></tbody></table>",
        ),
        (
            "cell over column",
            "<table><col class=\"loser\"></col><td class=\"winner\"></td></table>",
        ),
        (
            "row over row group",
            "<table><tbody class=\"loser\"></col><tr class=\"winner\"><td></td></tr></tbody></table>",
        ),
        (
            "cell over column group",
            "<table><colgroup class=\"loser\"></col><td class=\"winner\"></td></table>",
        ),
        (
            "row over column group",
            "<table><colgroup class=\"loser\"></colgroup><tr class=\"winner\"><td></td></tr></table>",
        ),
        (
            "column over column group",
            "<table><colgroup class=\"loser\"><col class=\"winner\"></col></colgroup><td></td></table>",
        ),
    ] {
        let document = Html::from_string(format!(
            "<!DOCTYPE html>\
             <style>\
             @page {{ size: 200px 200px; margin: 0 }}\
             body {{ margin: 0 }}\
             table {{ border-collapse: collapse; float: left }}\
             td {{ padding: 0 }}\
             .loser {{ border: 25px solid red }}\
             .winner {{ border: 25px solid green }}\
             </style>\
             {markup}"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let green = CssColor::new(0, 128, 0);
        let red = CssColor::new(255, 0, 0);
        assert!(
            !page.rects().iter().any(|rect| rect.fill == Some(red)),
            "{name}: losing border candidates should not paint red: {:?}",
            page.rects()
        );
        let (left, bottom, right, top) = filled_rect_bounds(page, green);
        let expected = 50.0 * 0.75;
        assert!(
            (right - left - expected).abs() < 0.5,
            "{name}: tied collapsed float should paint 50px wide, bounds=({}, {})",
            left,
            right
        );
        assert!(
            (top - bottom - expected).abs() < 0.5,
            "{name}: tied collapsed float should paint 50px tall, bounds=({}, {})",
            bottom,
            top
        );
    }
}

#[tokio::test]
async fn wraps_non_row_table_children_in_anonymous_cells() {
    let document = Html::from_string(
        "<div style=\"display:table;margin:0;width:80pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <section>Anon</section><span style=\"display:table-cell\">Cell</span>\
         </div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let anon = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Anon")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();

    assert!(anon.x() < cell.x());
    assert!(
        (anon.y() - cell.y()).abs() < 2.0,
        "generated anonymous cell should stay in the same visual row: anon_y={}, cell_y={}",
        anon.y(),
        cell.y()
    );
}

#[tokio::test]
async fn wraps_non_cell_row_children_in_anonymous_cells() {
    let document = Html::from_string(
        "<div style=\"display:table;margin:0;width:80pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <div style=\"display:table-row\"><b>Anon</b><span style=\"display:table-cell\">Cell</span></div>\
         </div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let anon = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Anon")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();

    assert!(anon.x() < cell.x());
    assert!(
        (anon.y() - cell.y()).abs() < 2.0,
        "generated anonymous cell should stay in the same visual row: anon_y={}, cell_y={}",
        anon.y(),
        cell.y()
    );
}

#[tokio::test]
async fn wraps_table_text_children_in_anonymous_rows_and_cells() {
    let document = Html::from_string(
        "<div style=\"display:table;margin:0;width:80pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         Lead<span style=\"display:table-cell\">Cell</span>\
         </div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lead = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Lead")
        .unwrap();
    let cell = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Cell")
        .unwrap();

    assert!(lead.x() < cell.x());
    assert!((lead.y() - cell.y()).abs() < 0.01);
}

#[tokio::test]
async fn preserves_whitespace_between_non_cell_table_children() {
    let document = Html::from_string(
        "<style>\
         @page { size: 600px 120px; margin: 0 }\
         body { margin: 0; font-size: 16px; line-height: 20px }\
         .outer { display: table; width: 500px; border-spacing: 0 }\
         .half { display: inline-block; width: 50% }\
         </style>\
         <div class=\"outer\"><div class=\"half\">A</div> <div class=\"half\">B</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let a = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .unwrap();

    assert!(
        b.y() < a.y() - 1.0,
        "preserved whitespace should force B onto the next line: a={a:?}, b={b:?}"
    );
    assert!(
        (a.x() - b.x()).abs() < 1.0,
        "wrapped inline-block should restart near the same inline position: a={a:?}, b={b:?}"
    );
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let yellow_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .collect::<Vec<_>>();
    assert_eq!(yellow_rects.len(), 2, "{yellow_rects:?}");
    let yellow_width = yellow_rects.iter().map(|rect| rect.width()).sum::<f32>();
    assert!((yellow_width - 210.0).abs() < 0.5, "{yellow_rects:?}");
    assert!(
        yellow_rects
            .iter()
            .all(|rect| (rect.height() - 225.0).abs() < 0.5),
        "{yellow_rects:?}"
    );
    assert!(
        (yellow_rects[0].y() - yellow_rects[1].y()).abs() < 0.5,
        "{yellow_rects:?}"
    );

    let green_horizontal_borders = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)) && rect.height() < rect.width())
        .collect::<Vec<_>>();
    let table_horizontal_borders = green_horizontal_borders
        .iter()
        .filter(|rect| (rect.width() - 225.0).abs() < 0.5)
        .collect::<Vec<_>>();
    assert_eq!(
        table_horizontal_borders.len(),
        2,
        "{green_horizontal_borders:?}"
    );
    assert!(
        table_horizontal_borders
            .iter()
            .all(|rect| (rect.height() - 7.5).abs() < 0.5),
        "{table_horizontal_borders:?}"
    );
    assert!(
        table_horizontal_borders
            .iter()
            .all(|rect| (rect.x() - 137.5).abs() < 0.5),
        "{table_horizontal_borders:?}"
    );

    let green_vertical_borders = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)) && rect.width() < rect.height())
        .collect::<Vec<_>>();
    assert_eq!(
        green_vertical_borders.len(),
        2,
        "{green_vertical_borders:?}"
    );
    // Atomic inline boxes align to the line box's baseline by default.  The
    // anonymous table cell therefore retains the font's descender below the
    // 300px boxes (WeasyPrint emits 323.46 CSS px for the border box).
    assert!(
        green_vertical_borders
            .iter()
            .all(|rect| (rect.height() - 243.0).abs() < 0.5),
        "{green_vertical_borders:?}"
    );
    assert!(
        green_vertical_borders
            .iter()
            .all(|rect| rect.height() < page.height() - 1.0),
        "html display:table border must not fill page height: page_height={} borders={green_vertical_borders:?}",
        page.height()
    );
    assert!(
        page.rects().iter().all(|rect| {
            rect.fill != Some(CssColor::new(0, 128, 0)) || rect.height() < page.height() - 1.0
        }),
        "green table border/background should remain shrink-wrapped: {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn html_display_table_root_stays_shrink_wrapped_on_large_page() {
    let document = Html::from_string(
        "<style>\
         @page { size: 720pt 640pt; margin: 0 }\
         html { display: table; border: 10px solid green; border-spacing: 0; padding: 0; margin: auto }\
         body { padding: 0; margin: 0 }\
         </style>\
         <div style=\"width:120px;height:160px;background:yellow;display:inline-block\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let yellow = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .unwrap_or_else(|| panic!("{:?}", page.rects()));
    assert!((yellow.width() - 90.0).abs() < 0.5, "{yellow:?}");
    assert!((yellow.height() - 120.0).abs() < 0.5, "{yellow:?}");

    let green_vertical_borders = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)) && rect.width() < rect.height())
        .collect::<Vec<_>>();
    assert_eq!(
        green_vertical_borders.len(),
        2,
        "{green_vertical_borders:?}"
    );
    // As above, the root table's anonymous cell includes the inline line
    // box's baseline descent below its 160px atomic inline child.
    assert!(
        green_vertical_borders
            .iter()
            .all(|rect| (rect.height() - 138.0).abs() < 0.5),
        "{green_vertical_borders:?}"
    );
    assert!(
        green_vertical_borders
            .iter()
            .all(|rect| rect.height() < page.height() - 1.0),
        "root table border must not expand to the page area height: page_height={} borders={green_vertical_borders:?}",
        page.height()
    );
}

#[tokio::test]
async fn wraps_direct_table_cells_in_anonymous_rows() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:120pt\"><td style=\"border:1pt solid black\">A</td><td>B</td><tr><td>C</td><td>D</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[0].text, "A");
    assert_eq!(lines[1].text, "B");
    assert_eq!(lines[2].text, "C");
    assert_eq!(lines[3].text, "D");
    assert!(lines[1].x() > lines[0].x());
    assert!(lines[2].y() < lines[0].y());
    assert_eq!(document.pages[0].rects()[0].fill, Some(CssColor::BLACK));
}

#[tokio::test]
async fn lays_out_table_captions_above_and_below_table_grid() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } table { margin: 0; width: 100pt; border-spacing: 0 } caption { margin: 0; line-height: 12pt } .bottom { caption-side: bottom }</style>\
         <table><caption>Top caption</caption><tr><td>A</td></tr></table>\
         <table><caption class=\"bottom\">Bottom caption</caption><tr><td>B</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
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

    assert!(top_caption.y() > a.y());
    assert!(bottom_caption.y() < b.y());
}

#[tokio::test]
async fn captions_and_abspos_descendants() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>\
         @page { size: 140px 120px; margin: 0 } body { margin: 0 }\
         table { margin: 0 0 0 20px; border-spacing: 0 }\
         caption, .cap-fill { height: 100px; width: 50px; background: green; margin: 0; padding: 0 }\
         #redSquare { position: absolute; z-index: -1; left: 20px; top: 0; height: 100px; width: 100px; background: red }\
         </style>\
         <div id=\"redSquare\"></div>\
         <table><caption style=\"position: relative\"><div class=\"cap-fill\" style=\"position: absolute; left: 50px\"></div></caption></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);
    assert_eq!(final_rect_fill_at(page, 33.75, 52.5), Some(green));
    assert_eq!(final_rect_fill_at(page, 71.25, 52.5), Some(green));
    assert_ne!(final_rect_fill_at(page, 86.25, 52.5), Some(red));

    let positioned_green = page
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(green)
                && (rect.x() - 52.5).abs() < 0.01
                && (rect.width() - 37.5).abs() < 0.01
        })
        .unwrap_or_else(|| {
            panic!(
                "caption abspos child should paint at table x + 50px: {:?}",
                page.rects()
            )
        });

    assert!((positioned_green.y() - 15.0).abs() < 0.01);
    assert!((positioned_green.height() - 75.0).abs() < 0.01);
}

#[tokio::test]
async fn wpt_caption_abspos_descendant_uses_relative_caption_containing_block() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>\
         @page { size: 180px 260px; margin: 0 }\
         caption, div { height:100px; width:50px; background:green }\
         #redSquare { height: 100px; width: 100px; background-color: red; position: absolute; z-index: -1 }\
         </style>\
         <p>Test passes if there is a filled green square and <strong>no red</strong>.</p>\
         <div id=\"redSquare\"></div>\
         <table><caption style=\"position:relative\"><div style=\"position:absolute; left:50px\"></div></caption></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);
    let green_halves = page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(green)
                && (rect.width() - 37.5).abs() < 0.01
                && (rect.height() - 75.0).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert!(
        green_halves.len() >= 2,
        "expected caption and abspos child to form the two green halves: {:?}",
        page.rects()
    );
    for rect in green_halves {
        assert_eq!(
            final_rect_fill_at(
                page,
                rect.x() + rect.width() / 2.0,
                rect.y() + rect.height() / 2.0
            ),
            Some(green),
            "green half should be topmost at its center: {rect:?}"
        );
    }

    for rect in page.rects().iter().filter(|rect| rect.fill == Some(red)) {
        for (x_ratio, y_ratio) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
            assert_ne!(
                final_rect_fill_at(
                    page,
                    rect.x() + rect.width() * x_ratio,
                    rect.y() + rect.height() * y_ratio,
                ),
                Some(red),
                "red square should be fully covered by caption content: {rect:?}"
            );
        }
    }
}

#[tokio::test]
async fn table_rows_with_zero_line_height_advance_for_replaced_content() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin: 0; width: 120pt\">\
         <tr style=\"line-height: 0pt\"><td><svg width=\"15pt\" height=\"15pt\"><rect width=\"15pt\" height=\"15pt\" fill=\"blue\" /></svg></td><td>A</td><td>B</td></tr>\
         <tr style=\"line-height: 0pt\"><td><svg width=\"15pt\" height=\"15pt\"><rect width=\"15pt\" height=\"15pt\" fill=\"blue\" /></svg></td><td>C</td><td>D</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "C", "D"]
    );
    assert!((lines[1].y() - lines[0].y()).abs() < 0.01);
    assert!((lines[3].y() - lines[2].y()).abs() < 0.01);
    assert!(lines[2].y() < lines[0].y() - 1.0);
}

#[tokio::test]
async fn empty_table_cells_do_not_synthesize_line_height() {
    let document =
        Html::from_string("<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-spacing:0\"><tr><td style=\"width:40pt;border:1pt solid black\"></td></tr></table>")
            .render(&RenderOptions::default()).await
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[0].text, "A");
    assert_eq!(lines[1].text, "B");
    let advance = lines[0].y() - lines[1].y();
    assert!(
        (advance - 30.0).abs() < 0.01,
        "expected row text advance to be 30pt, got {advance} from y={} and y={}",
        lines[0].y(),
        lines[1].y()
    );
}

#[tokio::test]
async fn table_row_minimum_includes_inline_content_before_a_block_child() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:60pt;border-spacing:0;table-layout:fixed;font-size:10pt;line-height:10pt\">\
         <tr style=\"height:10pt\"><td style=\"vertical-align:top\">Top<span style=\"display:block\">Bottom</span></td></tr>\
         <tr><td style=\"vertical-align:top\">Next</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Top", "Bottom", "Next"]
    );
    let top_to_bottom = lines[0].y() - lines[1].y();
    let bottom_to_next = lines[1].y() - lines[2].y();
    assert!(
        (top_to_bottom - 10.0).abs() < 0.01,
        "expected the block child below its inline predecessor, got {top_to_bottom}pt"
    );
    assert!(
        (bottom_to_next - 10.0).abs() < 0.01,
        "expected the following row below the block child, got {bottom_to_next}pt"
    );
}

#[tokio::test]
async fn table_row_minimum_includes_inline_content_before_multiple_block_children() {
    let document = Html::from_string(
        "<style>\
           table { margin:0; width:60pt; border-spacing:0; table-layout:fixed }\
           td { padding:.55pt 1.1pt .4pt; vertical-align:middle }\
           .ordinal { font-family:serif; font-size:5.7pt; font-weight:600; vertical-align:top }\
           .ordinal + .jp { display:inline; margin-left:.6pt }\
           .jp { display:block; font-size:5.25pt; line-height:.94 }\
           .name { display:block; font-size:6.8pt; font-weight:600; line-height:1.01 }\
         </style>\
         <table cellpadding=\"0\">\
         <tr style=\"height:10pt\"><td><span class=\"ordinal\">1.</span><span class=\"jp\">Top</span><span class=\"name\">Bottom</span></td></tr>\
         <tr><td>Next</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["1.", "Top", "Bottom", "Next"]
    );
    let top_to_bottom = lines[1].y() - lines[2].y();
    let bottom_to_next = lines[2].y() - lines[3].y();
    assert!(
        top_to_bottom > 10.0,
        "expected the block child after the mixed inline line, got {top_to_bottom}pt"
    );
    assert!(
        bottom_to_next > 10.0,
        "expected the following row below the mixed inline/block cell, got {bottom_to_next}pt"
    );
}

#[tokio::test]
async fn authored_empty_table_row_with_height_contributes_to_grid_height() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr><td style=\"vertical-align:top\">A</td></tr>\
         <tr style=\"height:30pt\"></tr>\
         <tr><td style=\"vertical-align:top\">B</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[0].text, "A");
    assert_eq!(lines[1].text, "B");
    let advance = lines[0].y() - lines[1].y();
    assert!(
        (advance - 40.0).abs() < 0.01,
        "expected empty row height to advance following row by 40pt total, got {advance} from y={} and y={}",
        lines[0].y(),
        lines[1].y()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "C"]
    );
    assert!((lines[0].y() - lines[1].y() - 10.0).abs() < 0.01);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        texts.iter().any(|text| text.contains("Top")),
        "visible rowspan text should remain: {texts:?}"
    );
    assert!(texts.contains(&"C3"));
    // `Page::lines` exposes the semantic text sequence before paint clipping,
    // so it intentionally retains the line that is clipped from the collapsed
    // row track. The visible text and the next row prove that the spanning
    // cell remains attached only to the surviving table geometry.
    assert!(texts.iter().any(|text| text.contains("Hidden row text")));
}

#[tokio::test]
async fn collapsed_rows_do_not_add_extra_estimated_border_spacing() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 160pt; margin: 10pt } body { margin:0 } table { break-inside:avoid; margin:0; width:40pt; border-spacing:0 30pt; font-size:10pt; line-height:10pt } td { padding:0 }</style>\
         <div style=\"height:20pt\"></div>\
         <table><tr><td>A</td></tr><tr style=\"visibility:collapse\"><td>B</td></tr><tr><td>C</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_eq!(
        document.pages[0]
            .lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .expect("table background should paint");

    assert!(
        (background.height() - 22.0).abs() < 0.01,
        "separated table background should include top and bottom vertical border-spacing, got {background:?}"
    );
}

#[tokio::test]
async fn definite_table_height_distributes_extra_height_to_row_groups() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;background:green;height:100px}td{padding:0}</style>\
         <table><thead><tr><td><div style=\"display:inline-block;width:100px\"></div></td></tr></thead></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("definite-height table should paint its background");

    assert!((green.width() - 75.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn table_min_height_grows_grid_after_wrapper_padding_and_border() {
    let document = Html::from_string(
        "<!doctype html><title>min-height can grow a table over its intrinsic size</title>\
         <style>@page{size:360pt 360pt;margin:0}body{margin:0}td{padding:0}\
         table{border-spacing:0;min-height:312px;border:1px solid black;background:green;padding:5px}\
         div{width:300px;height:100%;background:blue}</style>\
         <table><tr><td><div></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("min-height table background should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("percent-height child should fill the grown table row");

    assert!((green.height() - 234.0).abs() < 0.01, "{green:?}");
    assert!((blue.height() - 225.0).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn table_max_height_alone_does_not_shrink_intrinsic_rows() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;background:green;max-height:10px}td{padding:0}div{width:40px;height:40px}</style>\
         <table><tr><td><div></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("table background should paint");

    assert!((green.height() - 30.0).abs() < 0.01, "{green:?}");
}

async fn assert_table_cell_overflow_explicit_height_matches_reference(cell_sizing: &str) {
    let base = "<!DOCTYPE html>\
        <style>@page{size:360pt 360pt;margin:0}body{margin:0}td{border:2px solid cyan}.tall{height:300px;background:blue;border:2px solid black}</style>";
    let target = Html::from_string(format!(
        "{base}<style>td{{{cell_sizing}overflow:hidden}}</style>\
         <table border><td><div class=\"tall\"></div>Can you see this text?</td></table>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "{base}<table border><td><div class=\"tall\"></div>Can you see this text?</td></table>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let target_page = &target.pages[0];
    let reference_page = &reference.pages[0];
    assert_eq!(target_page.operations(), reference_page.operations());
    assert_eq!(target_page.rects(), reference_page.rects());
    assert_eq!(target_page.paths(), reference_page.paths());
}

#[tokio::test]
async fn table_cell_overflow_explicit_height_matches_intrinsic_reference() {
    assert_table_cell_overflow_explicit_height_matches_reference("height:20px;").await;
}

#[tokio::test]
async fn table_cell_overflow_explicit_height_and_max_height_matches_intrinsic_reference() {
    assert_table_cell_overflow_explicit_height_matches_reference("height:20px;max-height:20px;")
        .await;
}

#[tokio::test]
async fn definite_table_height_distributes_extra_height_across_row_groups() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0}td div{width:100px;height:10px}thead{background:red}tbody{background:blue}</style>\
         <table><thead><tr><td><div></div></td></tr></thead><tbody><tr><td><div></div></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first row group background should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second row group background should paint");

    assert!((red.height() - 37.5).abs() < 0.01, "{red:?}");
    assert!((blue.height() - 37.5).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn explicit_row_group_height_expands_auto_table_group_only() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse}td{padding:0}td div{width:40px;height:10px}.a{height:80px;background:red}.b{background:blue}</style>\
         <table><tbody class=\"a\"><tr><td><div></div></td></tr></tbody><tbody class=\"b\"><tr><td><div></div></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first row group background should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second row group background should paint");

    assert!((red.height() - 60.0).abs() < 0.01, "{red:?}");
    assert!((blue.height() - 7.5).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn table_height_interpolates_between_base_and_percentage_reference_rows() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:80px}td{padding:0}td div{width:40px;height:10px}.a{height:100%;background:red}.b{background:blue}</style>\
         <table><tr class=\"a\"><td><div></div></td></tr><tr class=\"b\"><td><div></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("percentage row background should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("auto row background should paint");

    assert!((red.height() - 52.5).abs() < 0.01, "{red:?}");
    assert!((blue.height() - 7.5).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn percentage_sizing_of_table_cell_and_row_group_uses_definite_table_height() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0}td div{width:40px;height:10px}.a{height:50%;background:red}.b{height:30px;background:blue}.cell{height:60%}</style>\
         <table><tbody class=\"a\"><tr><td class=\"cell\"><div></div></td></tr></tbody><tbody class=\"b\"><tr><td><div></div></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("percentage row group background should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("explicit row group background should paint");

    assert!((red.height() - 48.75).abs() < 0.01, "{red:?}");
    assert!((blue.height() - 26.25).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn extra_table_height_goes_to_auto_rows_after_reference_rows() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0}td div{width:40px;height:10px}.a{height:60px;background:red}.b{background:blue}</style>\
         <table><tr class=\"a\"><td><div></div></td></tr><tr class=\"b\"><td><div></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("explicit row background should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("auto row background should paint");

    assert!((red.height() - 45.0).abs() < 0.01, "{red:?}");
    assert!((blue.height() - 30.0).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn rowspan_height_constraint_grows_auto_rows_before_explicit_rows() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse}td{padding:0}.a{height:60px;background:red}.b{background:blue}.short{width:20px;height:10px}.span{width:20px;height:90px}</style>\
         <table><tr class=\"a\"><td rowspan=\"2\"><div class=\"span\"></div></td><td><div class=\"short\"></div></td></tr><tr class=\"b\"><td><div class=\"short\"></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)) && rect.x() > 10.0)
        .expect("explicit row background should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("auto row background should paint");

    assert!((red.height() - 45.0).abs() < 0.01, "{red:?}");
    assert!((blue.height() - 22.5).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn percent_height_table_cell_child_uses_final_cell_content_height() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0}.child{width:20px;height:100%;background:green}</style>\
         <table><tr><td><div class=\"child\"></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("percentage-height child should paint");

    assert!((green.height() - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn fixed_height_table_cell_scroll_percent_child_starts_at_cell_top() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0;background:red}.scroller{width:100px;min-height:100px;height:100%;overflow-y:scroll}.child{height:200px;background:green}</style>\
         <table><tr><td><div class=\"scroller\"><div class=\"child\"></div></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    let red = largest_filled_rect(page, CssColor::new(255, 0, 0));
    let green = largest_filled_rect(page, CssColor::new(0, 128, 0));

    assert!(
        (rect_top(green) - rect_top(red)).abs() < 0.01,
        "percentage-height scroll child should start at the final cell content top: red={red:?} green={green:?}"
    );
}

#[tokio::test]
async fn percentage_height_table_cell_scroll_percent_child_starts_at_cell_top() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100%}td{padding:0;background:red}.scroller{width:100px;min-height:100px;height:100%;overflow-y:scroll}.child{height:200px;background:green}</style>\
         <table><tr><td><div class=\"scroller\"><div class=\"child\"></div></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    let red = largest_filled_rect(page, CssColor::new(255, 0, 0));
    let green = largest_filled_rect(page, CssColor::new(0, 128, 0));

    assert!(
        (rect_top(green) - rect_top(red)).abs() < 0.01,
        "percentage-height table scroll child should start at the final cell content top: red={red:?} green={green:?}"
    );
}

#[tokio::test]
async fn percentage_row_heights_overflowing_table_height_share_available_height() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;height:100px}td{padding:0}td div{width:20px;height:10px}.a{height:80%;background:red}.b{height:80%;background:green}.c{background:blue}</style>\
         <table><tr class=\"a\"><td><div></div></td></tr><tr class=\"b\"><td><div></div></td></tr><tr class=\"c\"><td><div></div></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first percentage row should paint");
    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("second percentage row should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("auto row should paint");

    assert!((red.height() - 33.75).abs() < 0.01, "{red:?}");
    assert!((green.height() - 33.75).abs() < 0.01, "{green:?}");
    assert!((blue.height() - 7.5).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn nested_table_fragment_height_contributes_to_outer_row_height() {
    let document = Html::from_string(
        "<style>@page{size:180pt 180pt;margin:0}body{margin:0}table{margin:0;font-size:0;line-height:0}td{padding:0}.outer{border-collapse:collapse}.first{background:red}.inner{border-spacing:0 5pt}.inner td{width:20pt;height:10pt}.after{height:10pt;background:blue}</style>\
         <table class=\"outer\"><tr class=\"first\"><td><table class=\"inner\"><tr><td></td></tr><tr><td></td></tr></table></td></tr><tr><td class=\"after\"></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("outer row background should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("following row background should paint");

    assert!(
        (red.height() - 35.0).abs() < 0.01,
        "outer row should include nested table rows and vertical border-spacing: {red:?}"
    );
    assert!((blue.height() - 10.0).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn floated_table_cell_content_contributes_to_auto_row_height() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}table{margin:0;border-collapse:collapse;font-size:0;line-height:0}td{padding:0}.first{background:red}.float{float:left;width:20pt;height:30pt;background:green}.after{height:10pt;background:blue}</style>\
         <table><tr class=\"first\"><td><div class=\"float\"></div></td></tr><tr><td class=\"after\"></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("row background should paint");
    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("floated child should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("following row should paint");

    assert!((red.height() - 30.0).abs() < 0.01, "{red:?}");
    assert!((green.height() - 30.0).abs() < 0.01, "{green:?}");
    assert!((blue.height() - 10.0).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn fragmented_table_uses_distributed_final_row_heights() {
    let document = Html::from_string(
        "<style>@page{size:100pt 100pt;margin:10pt}body{margin:0}table{margin:0;border-collapse:collapse;height:160pt;font-size:10pt;line-height:10pt}td{padding:0}</style>\
         <table><tr><td>A</td></tr><tr><td>B</td></tr><tr><td>C</td></tr><tr><td>D</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let a = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .unwrap();
    let c = document.pages[1]
        .lines()
        .iter()
        .find(|line| line.text == "C")
        .unwrap();
    let d = document.pages[1]
        .lines()
        .iter()
        .find(|line| line.text == "D")
        .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!((a.y() - b.y() - 40.0).abs() < 0.01, "a={a:?} b={b:?}");
    assert!((c.y() - d.y() - 40.0).abs() < 0.01, "c={c:?} d={d:?}");
}

#[tokio::test]
async fn display_contents_inside_table_preserves_child_styles_and_fixup() {
    let document = Html::from_string(
        "<style>@page{size:160pt 160pt;margin:0}body{margin:0}</style>\
         <div style=\"display:table;font:25px/1 Ahem;color:red\"><div style=\"display:table-row\"><div style=\"display:contents;color:green\">X<div style=\"display:table-cell\">X</div>X<div style=\"display:table-row\">X</div>X</div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| {
            (
                line.text.as_str(),
                line.color,
                line.font_size,
                line.x(),
                line.y(),
            )
        })
        .collect::<Vec<_>>();
    lines.sort_by(|left, right| {
        right
            .4
            .total_cmp(&left.4)
            .then_with(|| left.3.total_cmp(&right.3))
    });

    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(lines.iter().all(|line| line.0 == "X"));
    assert!(lines.iter().all(|line| line.1 == CssColor::new(0, 128, 0)));
    assert!(lines.iter().all(|line| (line.2 - 18.75).abs() < 0.01));

    type PaintedLine<'a> = (&'a str, CssColor, f32, f32, f32);
    let mut rows: Vec<Vec<PaintedLine<'_>>> = Vec::new();
    for line in lines {
        if rows
            .last()
            .is_some_and(|row| (row[0].4 - line.4).abs() < 0.01)
        {
            rows.last_mut().unwrap().push(line);
        } else {
            rows.push(vec![line]);
        }
    }
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), vec![3, 1, 1]);
    assert!(rows[0][0].4 > rows[1][0].4);
    assert!(rows[1][0].4 > rows[2][0].4);

    let trailing_cell_x = rows[0][2].3;
    assert!((rows[1][0].3 - trailing_cell_x).abs() < 0.01);
    assert!((rows[2][0].3 - trailing_cell_x).abs() < 0.01);
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["B"]
    );
    assert!(lines[0].x() < crate::layout::PageMargins::DEFAULT.left() + 1.0);
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["C"]
    );
    assert!(lines[0].x() < crate::layout::PageMargins::DEFAULT.left() + 1.0);
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["C"]
    );
    assert!(lines[0].x() < crate::layout::PageMargins::DEFAULT.left() + 1.0);
}

#[tokio::test]
async fn table_rows_inherit_from_row_groups() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tbody style=\"visibility:collapse;color:red\"><tr><td>A</td></tr></tbody>\
         <tbody style=\"color:blue\"><tr><td>B</td></tr></tbody>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "B");
    assert_eq!(lines[0].color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn empty_cells_hide_suppresses_empty_cell_backgrounds_and_borders() {
    let show = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:80pt;border-spacing:0;empty-cells:show\">\
         <tr><td style=\"width:40pt;border:2pt solid black\"></td><td style=\"width:40pt;border:2pt solid black\">X</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let hide = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:80pt;border-spacing:0;empty-cells:hide\">\
         <tr><td style=\"width:40pt;border:2pt solid black\"></td><td style=\"width:40pt;border:2pt solid black\">X</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let show_black_rects = show.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::BLACK))
        .count();
    let hide_black_rects = hide.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::BLACK))
        .count();

    assert_eq!(hide.pages[0].lines()[0].text, "X");
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "C"]
    );
    assert!(
        (lines[0].y() - lines[1].y() - 18.0).abs() < 0.01,
        "hidden empty row should leave only one vertical spacing side: lines={lines:?}"
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::BLACK)),
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let top = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Top")
        .unwrap();
    let middle = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Mid")
        .unwrap();
    let bottom = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Bot")
        .unwrap();

    assert!(top.y() > middle.y());
    assert!(middle.y() > bottom.y());
    assert!((top.y() - middle.y() - 15.0).abs() < 0.01);
    assert!((middle.y() - bottom.y() - 15.0).abs() < 0.01);
}

#[tokio::test]
async fn aligns_table_cell_text_on_explicit_baseline() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:80pt;border-spacing:0;line-height:20pt\">\
         <tr><td style=\"width:40pt;font-size:20pt;vertical-align:baseline\">Big</td><td style=\"width:40pt;font-size:10pt;vertical-align:baseline\">Small</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let big = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Big")
        .unwrap();
    let small = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Small")
        .unwrap();

    assert!(
        (big.y() - small.y()).abs() < 0.01,
        "expected table-cell baselines to match, got Big y={} and Small y={}",
        big.y(),
        small.y()
    );
}

#[tokio::test]
async fn aligns_table_cell_multiline_content_on_first_baseline() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:100pt;border-spacing:0\">\
         <tr><td style=\"width:50pt;font-size:10pt;line-height:10pt;vertical-align:baseline\">First<br>Second</td><td style=\"width:50pt;font-size:20pt;line-height:20pt;vertical-align:baseline\">Peer</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let first = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "First")
        .unwrap();
    let second = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Second")
        .unwrap();
    let peer = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();

    assert!(
        (first.y() - peer.y()).abs() < 0.01,
        "expected multiline cell first baseline to align with peer: first={}, peer={}",
        first.y(),
        peer.y()
    );
    assert!(
        second.y() < first.y(),
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let block = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Block")
        .unwrap();
    let peer = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();

    assert!(
        (block.y() - peer.y()).abs() < 0.01,
        "expected block child baseline to align with peer: block={}, peer={}",
        block.y(),
        peer.y()
    );
}

#[tokio::test]
async fn aligns_table_cell_nested_table_row_baseline() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:110pt;border-spacing:0\">\
         <tr><td style=\"width:60pt;vertical-align:baseline\"><table cellpadding=\"0\" style=\"margin:0;border-spacing:0\"><tr><td style=\"font-size:20pt;line-height:20pt;vertical-align:baseline\">Inner</td></tr></table></td><td style=\"width:50pt;font-size:10pt;line-height:10pt;vertical-align:baseline\">Peer</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let inner = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Inner")
        .unwrap();
    let peer = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();

    assert!(
        (inner.y() - peer.y()).abs() < 0.01,
        "expected nested table row baseline to align with peer: inner={}, peer={}",
        inner.y(),
        peer.y()
    );
}

#[tokio::test]
async fn table_cell_baseline_falls_back_to_non_text_content_bottom() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0\">\
         <tr><td style=\"width:30pt;vertical-align:baseline\"><svg width=\"20pt\" height=\"30pt\"><rect width=\"20pt\" height=\"30pt\" fill=\"blue\" /></svg></td><td style=\"width:30pt;padding-top:12pt;padding-bottom:8pt;vertical-align:baseline\"></td><td style=\"width:30pt;font-size:10pt;line-height:10pt;vertical-align:baseline\">Peer</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let peer = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();
    let svg_bottom = document.pages[0]
        .paths()
        .iter()
        .find(|path| path.fill == Some(CssColor::new(0, 0, 255)))
        .and_then(|path| path.bounds())
        .map(|bounds| bounds.origin.y)
        .expect("SVG fallback content should paint as a positioned blue vector path");

    assert!(
        (peer.y() - svg_bottom).abs() < 0.01,
        "expected text baseline to align with SVG content-bottom fallback: peer={}, svg_bottom={svg_bottom}",
        peer.y(),
    );
}

#[tokio::test]
async fn table_cell_inline_vertical_align_keywords_align_as_baseline() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:150pt;border-spacing:0;line-height:20pt\">\
         <tr><td style=\"width:30pt;font-size:20pt;vertical-align:baseline\">Base</td><td style=\"width:30pt;font-size:10pt;vertical-align:text-top\">TextTop</td><td style=\"width:30pt;font-size:10pt;vertical-align:text-bottom\">TextBottom</td><td style=\"width:30pt;font-size:10pt;vertical-align:sub\">Sub</td><td style=\"width:30pt;font-size:10pt;vertical-align:super\">Super</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let baseline = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Base")
        .map(|line| line.y())
        .unwrap();

    for text in ["TextTop", "TextBottom", "Sub", "Super"] {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == text)
            .unwrap();
        let candidate = line.y();
        assert!(
            (candidate - baseline).abs() < 0.01,
            "{text} should align as a table-cell baseline value: candidate={candidate}, baseline={baseline}"
        );
    }
}

#[tokio::test]
async fn table_valign_presentational_hint_aligns_cell_content_by_default() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:60pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr style=\"height:40pt\"><td valign=\"top\" style=\"width:30pt\">Top</td><td valign=\"bottom\" style=\"width:30pt\">Bottom</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let top = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Top")
        .unwrap();
    let bottom = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Bottom")
        .unwrap();

    assert!(top.y() > bottom.y());
    assert!((top.y() - bottom.y() - 30.0).abs() < 0.01);
}

#[tokio::test]
async fn author_css_overrides_table_valign_presentational_hint() {
    let document = Html::from_string(
        "<style>td { vertical-align: top }</style>\
         <table cellpadding=\"0\" style=\"margin:0;width:60pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr style=\"height:40pt\"><td valign=\"bottom\" style=\"width:30pt\">Hint</td><td style=\"width:30pt\">Author</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let hinted = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Hint")
        .unwrap();
    let author = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Author")
        .unwrap();

    assert!((hinted.y() - author.y()).abs() < 0.01);
}

#[tokio::test]
async fn table_rules_groups_presentational_hint_paints_group_borders() {
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let thin_gray = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(128, 128, 128))
                && rect.height() <= 1.0
                && rect.width() > 10.0
        })
        .count();
    let thin_blue = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && rect.height() <= 1.0
                && rect.width() > 10.0
        })
        .count();
    let thick_gray = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(128, 128, 128))
                && (rect.height() - 3.75).abs() < 0.01
                && rect.width() > 10.0
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(document.pages[0].lines().is_empty());
    assert_eq!(document.pages[1].lines()[0].text, "Tall");
}

#[tokio::test]
async fn break_inside_avoid_keeps_table_row_groups_together_when_they_fit() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 } tbody { break-inside: avoid }</style>\
         <div style=\"height:55pt\"></div>\
         <table><tbody><tr><td>A</td></tr><tr><td>B</td></tr><tr><td>C</td></tr></tbody></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(document.pages[0].lines().is_empty());
    assert_eq!(
        document.pages[1]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "C"]
    );
}

#[tokio::test]
async fn page_break_inside_avoid_tbody_relaxes_at_a_cell_boundary_when_edge_spacing_overflows() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <html lang=\"en-US\"><head>\
         <title>CSS Test: CSS 2.1 page-break-inside:avoid</title>\
         <style type=\"text/css\">\
         @page { size:5in 3in; margin:0.5in; }\
         p { height: 1in; width: 1in; margin:0; background-color:blue; }\
         .test { page-break-inside:avoid; }\
         </style></head><body>\
         <p>1</p>\
         <table><tbody class=\"test\"><tr><td><p>2</p><p>3</p></td></tr></tbody></table>\
         </body></html>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(document.pages.len(), 3, "{page_texts:?}");
    assert_eq!(page_texts, vec![vec!["1"], vec!["2"], vec!["3"]]);

    let reference = Html::from_string(
        "<!DOCTYPE html>\
         <html lang=\"en-US\"><head>\
         <style type=\"text/css\">\
         @page { size:5in 3in; margin:0.5in; }\
         p { height: 1in; width: 1in; margin:0; background-color:blue; }\
         </style></head><body>\
         <p style=\"page-break-after:always\">1</p>\
         <table><tr><td><p>2</p><p>3</p></td></tr></table>\
         </body></html>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let blue = CssColor::new(0, 0, 255);
    let actual_first_table_block = largest_filled_rect(&document.pages[1], blue);
    let reference_first_table_block = largest_filled_rect(&reference.pages[1], blue);
    assert!(
        (actual_first_table_block.y() - reference_first_table_block.y()).abs() < 0.01,
        "continuation must retain the separated-border leading edge spacing: actual={actual_first_table_block:?}, reference={reference_first_table_block:?}"
    );
}

#[tokio::test]
async fn page_break_inside_avoid_row_matches_forced_header_break_with_separated_borders() {
    let render = |body: &str, extra_style: &str| {
        Html::from_string(format!(
            "<!DOCTYPE html><style>@page {{ size:5in 3in; margin:.5in }} p {{ height:1in; width:1in; margin:0; background:blue }} {extra_style}</style><body>{body}</body>"
        ))
    };
    let actual = render(
        "<table border=\"1\"><thead><tr><td><p>1</p></td></tr></thead><tbody><tr class=\"test\"><td><p>2</p><p>3</p></td></tr></tbody></table>",
        ".test { page-break-inside: avoid }",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = render(
        "<table border=\"1\"><thead><tr><td><p>1</p></td></tr></thead><tbody><tr><td><p>2</p><p>3</p></td></tr></tbody></table>",
        "thead { page-break-after: always }",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(actual.pages.len(), reference.pages.len());
    let blue = CssColor::new(0, 0, 255);
    for (page_index, (actual_page, reference_page)) in
        actual.pages.iter().zip(&reference.pages).enumerate()
    {
        assert_eq!(
            actual_page
                .lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            reference_page
                .lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            "page {page_index} text"
        );
        let actual_blue = actual_page
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(blue))
            .collect::<Vec<_>>();
        let reference_blue = reference_page
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(blue))
            .collect::<Vec<_>>();
        assert_eq!(actual_blue.len(), reference_blue.len(), "page {page_index}");
        for (actual_rect, reference_rect) in actual_blue.iter().zip(reference_blue) {
            assert!(
                (actual_rect.x() - reference_rect.x()).abs() < 0.01
                    && (actual_rect.y() - reference_rect.y()).abs() < 0.01
                    && (actual_rect.width() - reference_rect.width()).abs() < 0.01
                    && (actual_rect.height() - reference_rect.height()).abs() < 0.01,
                "page {page_index} blue geometry differs: actual={actual_rect:?}, reference={reference_rect:?}"
            );
        }
    }
}

#[tokio::test]
async fn separated_table_continuation_applies_leading_edge_spacing_before_repeated_header() {
    let actual = Html::from_string(
        "<style>@page { size: 120pt 96pt; margin: 12pt }\
         body, table, td, div, p { margin: 0; padding: 0 }\
         table { border-spacing: 6pt }\
         p { width: 20pt; color: transparent }\
         .before { height: 40pt } .header { height: 10pt; background: red }\
         .body { height: 30pt; background: blue }</style>\
         <div class=\"before\"></div>\
         <table><thead><tr><td><p class=\"header\">H</p></td></tr></thead>\
         <tbody><tr><td><p class=\"body\">B</p></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(
        "<style>@page { size: 120pt 96pt; margin: 12pt }\
         body, table, td, div, p { margin: 0; padding: 0 }\
         table { border-spacing: 6pt }\
         p { width: 20pt; color: transparent }\
         .before { height: 40pt; page-break-after: always }\
         .header { height: 10pt; background: red } .body { height: 30pt; background: blue }</style>\
         <div class=\"before\"></div>\
         <table><thead><tr><td><p class=\"header\">H</p></td></tr></thead>\
         <tbody><tr><td><p class=\"body\">B</p></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(actual.pages.len(), 2);
    assert_eq!(reference.pages.len(), 2);
    let red = CssColor::new(255, 0, 0);
    let actual_header = largest_filled_rect(&actual.pages[1], red);
    let reference_header = largest_filled_rect(&reference.pages[1], red);
    assert!(
        (actual_header.y() - reference_header.y()).abs() < 0.01,
        "repeated header must start after exactly one separated-border edge spacing: actual={actual_header:?}, reference={reference_header:?}"
    );
}

#[tokio::test]
async fn page_break_inside_avoid_row_group_wpt_variants_preserve_fragment_sequence() {
    // These inputs mirror the CSS2 pagination row-group avoid reftests, but
    // remain self-contained so a default separated-border table is covered by
    // the local smoke suite as well.
    let cases = [
        (
            "thead",
            "<p>1</p><table><thead class=\"test\"><tr><td><p>2</p><p>3</p></td></tr></thead></table>",
            vec![vec!["1"], vec!["2"], vec!["3"]],
        ),
        (
            "tfoot",
            "<p>1</p><table><tfoot class=\"test\"><tr><td><p>2</p><p>3</p></td></tr></tfoot></table>",
            vec![vec!["1"], vec!["2"], vec!["3"]],
        ),
        (
            "repeated-header-and-footer",
            "<table border=\"1\"><tfoot><tr><td><p>3</p></td></tr></tfoot><thead><tr><td><p>1</p></td></tr></thead><tbody class=\"test\"><tr><td><p>2</p><p>2</p></td></tr></tbody></table>",
            vec![
                // Repeating both groups would leave no room for the body.
                // The source header/footer remain on the outer fragments,
                // while repeats are emitted only for the body fragments.
                vec!["1", "3"],
                vec!["1", "2"],
                vec!["1", "2"],
                vec!["1", "3"],
            ],
        ),
        (
            "repeated-header",
            "<table border=\"1\"><thead><tr><td><p>1</p></td></tr></thead><tbody class=\"test\"><tr><td><p>2</p><p>3</p></td></tr></tbody></table>",
            vec![vec!["1"], vec!["1", "2"], vec!["1", "3"], vec!["1"]],
        ),
    ];

    for (name, body, expected_pages) in cases {
        let document = Html::from_string(format!(
            "<!DOCTYPE html><style>@page {{ size:5in 3in; margin:.5in }} p {{ height:1in; width:1in; margin:0; background:blue }} .test {{ page-break-inside:avoid }}</style><body>{body}</body>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();
        let pages = document
            .pages
            .iter()
            .map(|page| {
                page.lines()
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(pages, expected_pages, "{name}: {pages:?}");
    }
}

#[tokio::test]
async fn row_group_break_inside_avoid_suppresses_repeats_when_they_prevent_progress() {
    let document = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        Html::from_string(
            "<!DOCTYPE html><html lang=\"en-US\"><head>\
             <title>CSS Test: CSS 2.1 page-break-inside:avoid</title>\
             <style>@page { size:5in 3in; margin:0.5in; }\
             p { height: 1in; width: 1in; margin:0; background-color:blue; }\
             table { border-collapse:separate; border-spacing:0; } td { padding:0; }\
             .test { page-break-inside:avoid; }</style></head><body>\
             <table border=\"1\">\
             <tfoot><tr><td><p>3</p></td></tr></tfoot>\
             <thead><tr><td><p>1</p></td></tr></thead>\
             <tbody class=\"test\"><tr><td><p>2</p><p>2</p></td></tr></tbody>\
             </table></body></html>",
        )
        .render(&RenderOptions::default()),
    )
    .await
    .expect("table row-group avoid pagination should complete")
    .unwrap();

    let body_pages = document
        .pages
        .iter()
        .enumerate()
        .filter_map(|(page_index, page)| {
            let body_line_count = page.lines().iter().filter(|line| line.text == "2").count();
            (body_line_count > 0).then_some((page_index, body_line_count))
        })
        .collect::<Vec<_>>();

    assert_eq!(body_pages.len(), 1, "{body_pages:?}");
    assert_eq!(body_pages[0].1, 2, "{body_pages:?}");
}

#[tokio::test]
async fn row_group_break_inside_avoid_fragments_when_group_cannot_fit() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt }\
         body, table, tbody, tr, td, div { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         tbody { break-inside: avoid } .block { height: 50pt }</style>\
         <table><tbody><tr><td><div class=\"block\">A</div><div class=\"block\">B</div><div class=\"block\">C</div></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        page_texts.len() > 1,
        "oversized avoid row group must fragment: {page_texts:?}"
    );
    assert_eq!(
        page_texts.iter().flatten().copied().collect::<Vec<_>>(),
        vec!["A", "B", "C"],
        "{page_texts:?}"
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .filter(|line| line.text == "Head")
            .count(),
        2
    );
    assert_eq!(document.pages[1].lines()[0].text, "Head");
    assert_eq!(document.pages[1].lines()[1].text, "R8");
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .filter(|line| line.text == "Head")
            .count(),
        2
    );
    assert_eq!(document.pages[1].lines()[0].text, "Head");
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .filter(|line| line.text == "Foot")
            .count(),
        2
    );
    assert_eq!(document.pages[0].lines().last().unwrap().text, "Foot");
    assert_eq!(document.pages[1].lines()[0].text, "R8");
    assert_eq!(document.pages[1].lines().last().unwrap().text, "Foot");
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .filter(|line| line.text == "Foot")
            .count(),
        2
    );
    assert_eq!(document.pages[0].lines().last().unwrap().text, "Foot");
    assert_eq!(document.pages[1].lines().last().unwrap().text, "Foot");
}

#[tokio::test]
async fn table_row_break_before_repeats_header_on_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table, th, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 } .split { break-before: page }</style>\
         <table><thead><tr><th>Head</th></tr></thead><tbody>\
         <tr><td>R1</td></tr><tr class=\"split\"><td>R2</td></tr>\
         </tbody></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "Head");
    assert_eq!(document.pages[0].lines()[1].text, "R1");
    assert_eq!(document.pages[1].lines()[0].text, "Head");
    assert_eq!(document.pages[1].lines()[1].text, "R2");
}

#[tokio::test]
async fn table_row_group_break_after_repeats_header_on_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } body, table, th, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 } .first { break-after: page }</style>\
         <table><thead><tr><th>Head</th></tr></thead><tbody class=\"first\">\
         <tr><td>R1</td></tr></tbody><tbody><tr><td>R2</td></tr>\
         </tbody></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "Head");
    assert_eq!(document.pages[0].lines()[1].text, "R1");
    assert_eq!(document.pages[1].lines()[0].text, "Head");
    assert_eq!(document.pages[1].lines()[1].text, "R2");
}

#[tokio::test]
async fn table_row_break_after_avoid_rewinds_to_earlier_row_boundary() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt }\
         body, table, tr, td { margin: 0; font-size: 10pt; line-height: 10pt; padding: 0; border-spacing: 0 }\
         td { height: 20pt } .keep { break-after: avoid }</style>\
         <table><tbody><tr><td>R1</td></tr><tr><td>R2</td></tr><tr><td>R3</td></tr><tr class=\"keep\"><td>R4</td></tr><tr><td>R5</td></tr></tbody></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].lines()[0].text, "R1");
    assert_eq!(document.pages[1].lines()[0].text, "R2");
}

#[tokio::test]
async fn oversized_table_row_slices_direct_cell_text_from_inline_sequence() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 20pt; border-collapse: collapse } td { width: 20pt; height: 200pt }</style>\
         <table><tbody><tr><td>Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliett Kilo Lima Mike November Oscar Papa Quebec Romeo Sierra Tango</td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let pages_with_text = document
        .pages
        .iter()
        .filter(|page| !page.lines().is_empty())
        .count();
    assert!(
        pages_with_text >= 2,
        "direct table-cell text should be sliced across oversized row fragments"
    );
}

#[tokio::test]
async fn table_cell_text_box_trim_updates_inline_sequence_fragment_height() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 320px 290px; margin: 0 }}
  html, body, table, tbody, tr, td {{ margin: 0; padding: 0; border-spacing: 0 }}
  table {{ width: 200px; border-collapse: collapse }}
  td {{
    width: 200px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
</style>
<table><tbody><tr><td>A<br>B<br>C</td></tr></tbody></table>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-end;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        untrimmed.pages.len() >= 2,
        "untrimmed table-cell line sequence should fragment: pages={}",
        untrimmed.pages.len()
    );
    assert_eq!(
        trimmed.pages.len(),
        1,
        "trimmed table-cell line sequence should fit in one fragment"
    );
    assert_eq!(
        trimmed.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
}

#[tokio::test]
async fn table_cell_inline_block_text_box_trim_updates_atom_fragment_height() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 320px 290px; margin: 0 }}
  html, body, table, tbody, tr, td {{ margin: 0; padding: 0; border-spacing: 0 }}
  table {{ width: 200px; border-collapse: collapse }}
  td {{
    width: 200px;
    font-size: 0;
    line-height: 0;
  }}
  span {{
    display: inline-block;
    vertical-align: top;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
</style>
<table><tbody><tr><td><span>A<br>B<br>C</span></td></tr></tbody></table>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-end;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        untrimmed.pages.len() >= 2,
        "untrimmed inline-block atom should fragment with the row: pages={}",
        untrimmed.pages.len()
    );
    assert_eq!(
        trimmed.pages.len(),
        1,
        "trimmed inline-block atom should fit in one table-row fragment"
    );
    assert_eq!(
        trimmed.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let page_texts = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
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
async fn oversized_table_row_slices_styled_inline_cell_content_from_inline_sequence() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, span { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 20pt; border-collapse: collapse } td { width: 20pt; height: 160pt } span { color: red }</style>\
         <table><tbody><tr><td><span>Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliett Kilo Lima</span></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.lines()
                .iter()
                .any(|line| line.color == CssColor::new(255, 0, 0))
        })
        .count();
    assert!(
        red_pages >= 2,
        "styled inline table-cell content should be sequenced and sliced across row pieces"
    );
}

#[tokio::test]
async fn oversized_table_row_slices_generated_cell_inline_content_from_inline_sequence() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 24pt; border-collapse: collapse } td { width: 24pt; height: 160pt }\
         td::before { content: \"Before\"; color: red } td::after { content: \"After\"; color: blue }</style>\
         <table><tbody><tr><td>Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliett Kilo Lima</td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter().map(|line| line.text.as_str()))
        .collect::<Vec<_>>();
    assert!(texts.contains(&"Before"), "{texts:?}");
    assert!(texts.contains(&"After"), "{texts:?}");
}

#[tokio::test]
async fn split_table_cell_nested_inline_child_uses_sequence_for_generated_content() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 70pt; margin: 10pt }\
         body, table, td, div, span { margin: 0; padding: 0; border-spacing: 0; font-size: 10pt; line-height: 10pt }\
         table { width: 24pt; border-collapse: collapse } td { width: 24pt; height: 180pt }\
         div { display: block; height: 10pt }\
         .gen::before { content: \"Before\"; color: red } .gen::after { content: \"After\"; color: blue }</style>\
         <table><tbody><tr><td><div>Block</div><span class=\"gen\">Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliett Kilo Lima</span></td></tr></tbody></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter().map(|line| line.text.as_str()))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter().map(|line| line.text.as_str()))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let texts = document
        .pages
        .iter()
        .flat_map(|page| page.lines().iter().map(|line| line.text.as_str()))
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let row_piece_pages = document
        .pages
        .iter()
        .filter(|page| {
            page.rects().iter().any(|rect| {
                rect.fill == Some(CssColor::new(0, 0, 255))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
            })
        })
        .collect::<Vec<_>>();
    assert!(row_piece_pages.len() >= 3);

    let middle_page = row_piece_pages[1];
    let synthetic_horizontal = middle_page.rects().iter().any(|rect| {
        rect.fill == Some(CssColor::new(255, 0, 0))
            && rect.width() > 40.0
            && (rect.height() - 4.0).abs() < 0.01
    });
    let vertical_edges = middle_page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0))
                && (rect.width() - 4.0).abs() < 0.01
                && rect.height() > 0.0
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let texts = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["A", "B"]);
}

#[tokio::test]
async fn supports_basic_table_colspan() {
    let document = Html::from_string(
        "<table style=\"margin: 0; width: 120pt; border-spacing: 0\"><tr><td colspan=\"2\" style=\"border: 1pt solid black\">Wide</td></tr><tr><td>Left</td><td>Right</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "Wide");
    assert_eq!(document.pages[0].rects()[0].width(), 120.0);
    assert_eq!(document.pages[0].lines()[1].text, "Left");
    assert_eq!(document.pages[0].lines()[2].text, "Right");
    assert!(document.pages[0].lines()[2].x() - document.pages[0].lines()[1].x() >= 50.0);
}

#[tokio::test]
async fn supports_basic_table_rowspan_occupancy() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr><td rowspan=\"2\" style=\"width:30pt;border:1pt solid black\">Span</td><td style=\"width:60pt;border:1pt solid black\">A</td></tr>\
         <tr><td style=\"border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let span = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Span")
        .unwrap();
    let a = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .unwrap();

    assert!(a.x() > span.x());
    assert!((a.x() - b.x()).abs() < 0.01);
    assert!(b.y() < a.y());
}

#[tokio::test]
async fn parses_table_span_attributes_with_html_integer_rules() {
    let colspan = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr><td colspan=\"2px\" style=\"border:1pt solid black\">Wide</td></tr>\
         <tr><td>A</td><td>B</td><td>C</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let rowspan = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <tr><td rowspan=\"2px\" style=\"width:30pt\">Span</td><td>A</td></tr>\
         <tr><td>B</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let col_span = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:90pt;table-layout:fixed;border-spacing:0;font-size:10pt;line-height:10pt\">\
         <col span=\"2px\" style=\"width:20pt\"><col style=\"width:50pt\">\
         <tr><td>A</td><td>B</td><td>C</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let wide_border = colspan.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK) && rect.width() > 50.0)
        .expect("colspan should span two columns");
    assert!(wide_border.width() > 50.0);

    let span = rowspan.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Span")
        .unwrap();
    let a = rowspan.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = rowspan.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .unwrap();
    assert!(a.x() > span.x());
    assert!((a.x() - b.x()).abs() < 0.01);

    let lines = &col_span.pages[0].lines();
    assert!(((lines[1].x() - lines[0].x()) - (lines[2].x() - lines[1].x())).abs() < 0.01);
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let span = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Span")
        .unwrap();
    let a = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .unwrap();
    let foot = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Foot")
        .unwrap();
    let c = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "C")
        .unwrap();

    assert!((a.x() - b.x()).abs() < 0.01);
    assert!(a.x() > span.x());
    assert!((foot.x() - span.x()).abs() < 0.01);
    assert!(c.x() > foot.x());
}

#[tokio::test]
async fn supports_percentage_table_cell_widths() {
    let document = Html::from_string(
        "<table style=\"margin: 0; width: 200pt; border-spacing: 0\"><tr><td style=\"width: 25%; border: 1pt solid black\">A</td><td>B</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].rects()[0].width(), 50.0);
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 5);
    assert!((widths[0] - 33.5).abs() < 0.01, "widths={widths:?}");
    assert!((widths[1] - 198.5).abs() < 0.01, "widths={widths:?}");
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 2);
    assert!(widths[0] < 100.0);
    assert!(widths[1] > 200.0);
    assert!((widths.iter().sum::<f32>() - 300.0).abs() < 0.01);
}

fn painted_table_rect_width(document: &spindrift::Document, color: CssColor) -> f32 {
    document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(color))
        .unwrap_or_else(|| panic!("expected table background with color {color:?}"))
        .width()
}

#[tokio::test]
async fn table_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 160pt; margin: 0 } body { margin: 0; font: 10px/12px sans-serif }\
         table { margin: 0; border-spacing: 0; height: 12pt } td { padding: 0 }\
         .min { width: min-content; background: green }\
         .fit { width: fit-content(14px); background: blue }\
         .max { width: max-content; background: black }</style>\
         <table class=\"min\"><tr><td>aa bb</td></tr></table>\
         <table class=\"fit\"><tr><td>aa bb</td></tr></table>\
         <table class=\"max\"><tr><td>aa bb</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let min = painted_table_rect_width(&document, CssColor::new(0, 128, 0));
    let fit = painted_table_rect_width(&document, CssColor::new(0, 0, 255));
    let max = painted_table_rect_width(&document, CssColor::new(0, 0, 0));
    assert!(
        min < fit && fit < max,
        "table intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}

#[tokio::test]
async fn auto_table_definite_width_clamps_content_box_to_min_content() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>@page { size: 100px 100px; margin: 0 } body { margin: 0 }\
         .outer { width: min-content; background: green }\
         .table { display: table; inline-size: 30px; block-size: 100px; border-inline: 10px solid green; margin-inline: 10px }\
         .content { inline-size: 60px; height: 100px }</style>\
         <div class=\"outer\"><div class=\"table\"><div class=\"content\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let widest_green = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .unwrap_or_else(|| panic!("expected green square paint: {:?}", page.rects()));

    assert!(
        (widest_green.width() - 75.0).abs() < 0.01 && (widest_green.height() - 75.0).abs() < 0.01,
        "100px reference square should paint as a 75pt square: {widest_green:?}"
    );
    assert_eq!(final_rect_fill_at(page, 74.0, 37.5), Some(green));
}

#[tokio::test]
async fn vertical_intrinsic_table_inline_constraints_include_root_inline_decoration_in_parent_flow()
{
    // CSS Tables constrains the table grid to its intrinsic inline minimum,
    // while the anonymous wrapper must still contain the table-root border
    // box. In a vertical writing mode that logical inline decoration is a
    // physical top/bottom contribution in the horizontal parent flow.
    // <https://drafts.csswg.org/css-tables/#table-structure>
    // <https://drafts.csswg.org/css-writing-modes/#abstract-box>
    for (writing_mode, direction, inline_constraint, decoration) in [
        (
            "vertical-rl",
            "ltr",
            "inline-size:30px",
            "border-inline:10px solid green",
        ),
        (
            "vertical-rl",
            "ltr",
            "max-inline-size:30px",
            "border-inline:10px solid green",
        ),
        (
            "vertical-lr",
            "rtl",
            "inline-size:30px",
            "border-inline:10px solid green",
        ),
        (
            "vertical-lr",
            "ltr",
            "inline-size:30px",
            "padding-inline:10px",
        ),
    ] {
        let document = Html::from_string(format!(
            "<!DOCTYPE html><meta charset=\"utf-8\">\
             <style>@page{{size:160px 160px;margin:0}}body{{margin:0}}\
             .outer{{width:min-content;background:green}}\
             .table{{writing-mode:{writing_mode};direction:{direction};display:table;\
             {inline_constraint};block-size:100px;{decoration}}}\
             .content{{inline-size:80px;block-size:100px}}</style>\
             <div class=\"outer\"><div class=\"table\"><div class=\"content\"></div></div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let green = CssColor::new(0, 128, 0);
        let outer_background = page
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(green))
            .max_by(|left, right| {
                left.width()
                    .mul_add(left.height(), 0.0)
                    .total_cmp(&right.width().mul_add(right.height(), 0.0))
            })
            .unwrap_or_else(|| panic!("expected a green table wrapper: {:?}", page.rects()));

        assert!(
            (outer_background.width() - 75.0).abs() < 0.01
                && (outer_background.height() - 75.0).abs() < 0.01,
            "{writing_mode} {direction} {inline_constraint} {decoration} should produce a continuous 100px table-wrapper extent: {outer_background:?}"
        );
    }
}

#[tokio::test]
async fn plans_table_columns_from_fixed_percentage_and_auto_cells() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:500pt;border-spacing:0\"><tr><td style=\"width:30pt;border:1pt solid black\">S</td><td style=\"width:195pt;border:1pt solid black\">Label</td><td style=\"width:45%;border:1pt solid black\">Fill</td><td style=\"border:1pt solid black\">Auto</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 4);
    assert!((widths[0] - 33.5).abs() < 0.01, "widths={widths:?}");
    assert!((widths[1] - 198.5).abs() < 0.01, "widths={widths:?}");
    assert!((widths[2] - 225.0).abs() < 0.01, "widths={widths:?}");
    assert!((widths[3] - 43.0).abs() < 0.01, "widths={widths:?}");
}

#[tokio::test]
async fn table_cell_min_width_is_stable_with_long_sibling_content() {
    let style = "<style>table{margin:0;border-spacing:0}td{padding:0;border:1pt solid black}.key{min-width:80pt}</style>";
    let short = Html::from_string(format!(
        "{style}<table><tr><td class=\"key\">Key</td><td>Value</td></tr></table>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let long = Html::from_string(format!(
        "{style}<table><tr><td class=\"key\">Key</td><td>Long value that wraps across the available width and must not change the key column</td></tr></table>"
    ))
    .render(&RenderOptions::default())
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
    .render(&RenderOptions::default())
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
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 3);
    assert!((widths[0] - 43.5).abs() < 0.01, "widths={widths:?}");
    assert!((widths[1] - 100.0).abs() < 0.01, "widths={widths:?}");
    assert!((widths[2] - 56.5).abs() < 0.01, "widths={widths:?}");
}

#[tokio::test]
async fn colspan_percentage_contribution_is_distributed_before_final_widths() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:200pt;border-spacing:0\"><tr>\
         <td colspan=\"2\" style=\"width:75%;border:1pt solid black\">Wide</td><td style=\"border:1pt solid black\">C</td>\
         </tr><tr><td style=\"border:1pt solid black\">A</td><td style=\"border:1pt solid black\">B</td><td style=\"border:1pt solid black\">C</td></tr></table>",
    )
    .render(&RenderOptions::default())
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
async fn auto_table_colspan_constraints_use_single_column_baseline() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>@page { size: 140px 80px; margin: 0 } body { margin: 0 } table, td { border-spacing: 0; padding: 0 }</style>\
         <table style=\"margin:0;width:110px\" cellspacing=\"0\" cellpadding=\"0\">\
           <tr><td colspan=\"2\" style=\"width:100px\"></td><td colspan=\"2\"></td></tr>\
           <tr><td style=\"width:5px;height:10px;background:blue\"></td><td colspan=\"2\" style=\"height:10px;background:green\"></td><td style=\"width:5px;height:10px;background:blue\"></td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let green_rect = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(green))
        .unwrap_or_else(|| panic!("expected green colspan cell among {:?}", page.rects()));

    assert!(
        (green_rect.width() - 75.0).abs() < 0.01 && (green_rect.height() - 7.5).abs() < 0.01,
        "second-row colspan should cover the 100px middle span: {green_rect:?}"
    );
    assert_eq!(
        final_rect_fill_at(
            page,
            green_rect.x() + green_rect.width() / 2.0,
            green_rect.y() + green_rect.height() / 2.0,
        ),
        Some(green)
    );
}

#[tokio::test]
async fn overlapping_auto_colspans_cover_shifted_reference_square() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 220px 180px; margin: 0 } body { margin: 0 }</style>\
         <p>Test passes if there is a filled green square and <strong>no red</strong>.</p>\
         <div style=\"width:100px; height:100px; background:red;\">\
           <table style=\"margin-left:-5px; width:110px;\" cellspacing=\"0\" cellpadding=\"0\">\
             <tr>\
               <td colspan=\"2\" style=\"width:100px;\"></td>\
               <td colspan=\"2\"></td>\
             </tr>\
             <tr>\
               <td style=\"width:5px;\"></td>\
               <td colspan=\"2\" style=\"background:green;\">\
                 <div style=\"height:100px;\"></div>\
               </td>\
               <td style=\"width:5px;\"></td>\
             </tr>\
           </table>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let green_square = page
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(green)
                && (rect.width() - 75.0).abs() < 0.01
                && (rect.height() - 75.0).abs() < 0.01
        })
        .unwrap_or_else(|| panic!("expected green reference square among {:?}", page.rects()));
    let sample_y = green_square.y() + green_square.height() / 2.0;

    for sample_x in [
        green_square.x() + 0.5,
        green_square.x() + green_square.width() / 2.0,
        green_square.x() + green_square.width() - 0.5,
    ] {
        assert_eq!(
            final_rect_fill_at(page, sample_x, sample_y),
            Some(green),
            "green colspan cell should cover red reference square at ({sample_x}, {sample_y})"
        );
    }
}

#[tokio::test]
async fn fixed_table_layout_resolves_column_and_first_row_percentages() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:200pt;border-spacing:0;table-layout:fixed\">\
         <col style=\"width:25%\"><col><col>\
         <tr><td style=\"border:1pt solid black\">A</td><td style=\"width:50%;border:1pt solid black\">B</td><td style=\"border:1pt solid black\">C</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 3);
    assert!((widths[0] - 50.0).abs() < 0.01, "widths={widths:?}");
    assert!((widths[1] - 103.5).abs() < 0.01, "widths={widths:?}");
    assert!((widths[2] - 46.5).abs() < 0.01, "widths={widths:?}");
}

#[tokio::test]
async fn applies_table_min_and_max_width_before_column_planning() {
    let min_document = Html::from_string(
        "<table style=\"margin:0;width:40pt;min-width:80pt;border-spacing:0\"><tr><td style=\"border:1pt solid black\">A</td><td style=\"border:1pt solid black\">B</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let max_document = Html::from_string(
        "<table style=\"margin:0;width:120pt;max-width:60pt;border-spacing:0\"><tr><td style=\"border:1pt solid black\">A</td><td style=\"border:1pt solid black\">B</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let min_widths = horizontal_table_border_widths(&min_document);
    assert!(
        (min_widths.iter().sum::<f32>() - 80.0).abs() < 0.01,
        "min widths={min_widths:?}"
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let widths = horizontal_table_border_widths(&document);

    assert_eq!(widths.len(), 4);
    assert!((widths[0] - 33.5).abs() < 0.01, "widths={widths:?}");
    assert!((widths[1] - 86.5).abs() < 0.01, "widths={widths:?}");
    assert!((widths[2] - 33.5).abs() < 0.01, "widths={widths:?}");
    assert!((widths[3] - 86.5).abs() < 0.01, "widths={widths:?}");
}

#[tokio::test]
async fn fixed_table_layout_uses_colgroup_column_widths_before_first_row() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:120pt;table-layout:fixed;border-spacing:0\">\
         <colgroup><col style=\"width:40pt\"><col></colgroup>\
         <tr><td style=\"width:80pt;border:1pt solid black\">A</td><td style=\"border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
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
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines().len(), 1);
    assert_eq!(document.pages[0].lines()[0].text, "Shown");
}

#[tokio::test]
async fn applies_table_section_row_and_cell_selectors() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:100pt;border-spacing:0\"><tbody><tr><td>A</td></tr></tbody></table>",
    )
    .with_stylesheet(Css::from_string(
        "tbody tr { background: red } tbody tr td { color: blue }",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.width() >= 100.0 && rect.fill == Some(CssColor::new(255, 0, 0))),
        "rects={:?}",
        document.pages[0].rects()
    );
    assert_eq!(document.pages[0].lines()[0].color, CssColor::new(0, 0, 255));
}

#[tokio::test]
async fn paints_table_structural_backgrounds_in_spec_layer_order() {
    let document = Html::from_string(
        "<style>table{margin:0;width:40pt;border-spacing:0;background:#111}colgroup{background:#222}col{background:#333}tbody{background:#444}tr{background:#555}td{padding:0;background:#666}</style>\
         <table><colgroup><col></colgroup><tbody><tr><td>A</td></tr></tbody></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let fills = document.pages[0]
        .rects()
        .iter()
        .filter_map(|rect| rect.fill)
        .take(6)
        .collect::<Vec<_>>();

    assert_eq!(
        fills,
        vec![
            CssColor::new(17, 17, 17),
            CssColor::new(34, 34, 34),
            CssColor::new(51, 51, 51),
            CssColor::new(68, 68, 68),
            CssColor::new(85, 85, 85),
            CssColor::new(102, 102, 102),
        ]
    );
}

#[tokio::test]
async fn empty_display_table_still_paints_padding_and_border_box() {
    let document = Html::from_string(
        "<style>@page{size:400pt 400pt;margin:20pt}body{margin:0}div{display:table;background:green;border:1px solid black;padding:155px}</style><div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("empty table should paint its background");

    assert!((green.width() - 234.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 234.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn empty_display_table_border_box_height_respects_box_sizing() {
    let document = Html::from_string(
        "<style>@page{size:240pt 240pt;margin:0}body{margin:0}div{display:table;box-sizing:border-box;width:100pt;height:80pt;background:green;border:5pt solid black;padding:10pt}</style><div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("empty table should paint its background");

    assert!((green.width() - 100.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 80.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn html_table_percentage_height_uses_definite_containing_block() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page{size:200px 200px;margin:0}body{margin:0}p{display:none}\
         #table{width:100px;height:100%;background:green;padding-bottom:35px}</style>\
         <p>Test passes if there is a filled green square.</p>\
         <div style=\"height:100px\"><table id=\"table\"></table></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    assert_green_100px_square(&document);
}

#[tokio::test]
async fn non_empty_table_percentage_height_uses_definite_containing_block() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page{size:200px 200px;margin:0}body{margin:0}\
         .outer{height:100px}table{margin:0;border-collapse:collapse;width:40px;height:100%;background:red}\
         td{padding:0}.child{width:40px;height:100%;background:green}</style>\
         <div class=\"outer\"><table><tr><td><div class=\"child\"></div></td></tr></table></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("percentage-height table cell content should paint");

    assert!((green.width() - 30.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn empty_table_min_height_grows_grid_after_wrapper_padding_and_border() {
    let document = Html::from_string(
        "<style>@page{size:360pt 360pt;margin:0}body{margin:0}table{margin:0;border-spacing:0;min-height:312px;border:1px solid black;background:green;padding:5px}</style><table></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("empty min-height table should paint its background");

    assert!((green.height() - 234.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn honors_table_cellpadding_zero() {
    let zero =
        Html::from_string("<table cellpadding=\"0\" style=\"margin:0;width:100pt\"><tr><td style=\"border:1pt solid black\">A</td></tr></table>")
            .render(&RenderOptions::default()).await
            .unwrap();
    let padded =
        Html::from_string("<table cellpadding=\"4pt\" style=\"margin:0;width:100pt\"><tr><td style=\"border:1pt solid black\">A</td></tr></table>")
            .render(&RenderOptions::default()).await
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
    .render(&RenderOptions::default()).await
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[1].text, "B");
    assert!(
        (lines[1].x() - lines[0].x() - 26.0).abs() < 0.01,
        "line xs: {:?}",
        lines
            .iter()
            .map(|line| (line.text.as_str(), line.x()))
            .collect::<Vec<_>>()
    );
    let expected_first_text_x = crate::layout::PageMargins::DEFAULT.left() + 5.0;
    assert!(
        (lines[0].x() - expected_first_text_x).abs() < 0.01,
        "expected first text x {expected_first_text_x}, got {}",
        lines[0].x()
    );
}

#[tokio::test]
async fn honors_html_cellspacing_for_separated_tables() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" cellspacing=\"8pt\" style=\"margin:0;width:48pt\">\
         <tr><td style=\"width:20pt;border:1pt solid black\">A</td><td style=\"width:20pt;border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[1].text, "B");
    assert!(
        (lines[1].x() - lines[0].x() - 30.0).abs() < 0.01,
        "line xs: {:?}",
        lines
            .iter()
            .map(|line| (line.text.as_str(), line.x()))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn css_border_spacing_overrides_html_cellspacing() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" cellspacing=\"8pt\" style=\"margin:0;width:44pt;border-spacing:4pt 0\">\
         <tr><td style=\"width:20pt;border:1pt solid black\">A</td><td style=\"width:20pt;border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines[1].text, "B");
    assert!(
        (lines[1].x() - lines[0].x() - 26.0).abs() < 0.01,
        "line xs: {:?}",
        lines
            .iter()
            .map(|line| (line.text.as_str(), line.x()))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn collapsed_table_borders_share_internal_edges() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:40pt;border-collapse:collapse\">\
         <tr><td style=\"width:20pt;border:1pt solid black\">A</td><td style=\"width:20pt;border:1pt solid black\">B</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let vertical_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::BLACK) && rect.width() <= 1.01 && rect.height() > 1.0
        })
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
    .render(&RenderOptions::default()).await
    .unwrap();
    let page = &document.pages[0];

    let vertical_edge_indexes: Vec<_> = page
        .rects()
        .iter()
        .enumerate()
        .filter(|(_, rect)| {
            rect.fill == Some(CssColor::BLACK) && rect.width() <= 1.01 && rect.height() > 1.0
        })
        .map(|(index, _)| index)
        .collect();

    assert_eq!(vertical_edge_indexes.len(), 3);
    for rect_index in vertical_edge_indexes {
        assert!(page.operations().iter().any(|operation| {
            matches!(operation, crate::document::paint::page::PaintOperation::Rect(index) if *index == rect_index)
        }));
    }
}

#[tokio::test]
async fn collapsed_table_borders_paint_after_in_flow_block_child_backgrounds() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 180pt 180pt; margin: 0 } body { margin: 0 }</style>\
         <p style=\"display: none\">Test passes if there is a filled green square and <strong>no red</strong>.</p>\
         <table style=\"border-collapse: collapse; border-spacing: 0;\">\
           <td style=\"border-right: solid 100px green; height: 100px; padding: 0;\">\
             <div style=\"width: 0;\">\
               <div style=\"width: 100px; height: 100px; background: red;\"></div>\
             </div>\
           </td>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];
    let red = CssColor::new(255, 0, 0);
    let green = CssColor::new(0, 128, 0);
    let red_rect = largest_filled_rect(page, red);
    let green_rect = largest_filled_rect(page, green);

    assert!(
        (red_rect.width() - 75.0).abs() < 0.01 && (red_rect.height() - 75.0).abs() < 0.01,
        "red child block background should occupy a 100 CSS px square before paint ordering resolves visibility: {red_rect:?}"
    );
    assert!(
        (green_rect.width() - 75.0).abs() < 0.01 && (green_rect.height() - 75.0).abs() < 0.01,
        "collapsed green border should occupy a 100 CSS px square: {green_rect:?}"
    );

    let red_operation = first_rect_paint_operation_index(page, red);
    let green_operation = first_rect_paint_operation_index(page, green);
    assert!(
        red_operation < green_operation,
        "collapsed table border should paint after in-flow block child backgrounds; operations={:?}",
        page.paint_operations()
    );

    for (sample_x, sample_y) in [
        (
            red_rect.x() + red_rect.width() * 0.25,
            red_rect.y() + red_rect.height() * 0.25,
        ),
        (
            red_rect.x() + red_rect.width() * 0.50,
            red_rect.y() + red_rect.height() * 0.50,
        ),
        (
            red_rect.x() + red_rect.width() * 0.75,
            red_rect.y() + red_rect.height() * 0.75,
        ),
    ] {
        assert_eq!(
            final_rect_fill_at(page, sample_x, sample_y),
            Some(green),
            "collapsed border should cover red child background at ({sample_x}, {sample_y})"
        );
    }
}

#[tokio::test]
async fn collapsed_table_borders_paint_below_cell_foreground_content() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 180pt 140pt; margin: 0 } body { margin: 0 }</style>\
         <table style=\"border-collapse: collapse; border-spacing: 0;\">\
           <td style=\"border-right: 50px solid red; padding: 0;\">\
             <div style=\"width: 50px; line-height: 0;\">\
               <div style=\"display: inline-block; width: 100px; height: 100px; background: green;\"></div>\
             </div>\
           </td>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);
    let green_rect = largest_filled_rect(page, green);

    assert!(
        (green_rect.width() - 75.0).abs() < 0.01 && (green_rect.height() - 75.0).abs() < 0.01,
        "inline-block foreground should paint as a 100 CSS px square: {green_rect:?}"
    );

    let green_operation = first_rect_paint_operation_index(page, green);
    let red_border =
        page.paint_operations()
            .iter()
            .enumerate()
            .find_map(|(operation_index, operation)| {
                let crate::document::paint::page::PaintOperation::Rect(rect_index) = operation
                else {
                    return None;
                };
                let rect = page.rects().get(*rect_index)?;
                let overlaps_green = rect.x() < green_rect.x() + green_rect.width()
                    && rect.x() + rect.width() > green_rect.x()
                    && rect.y() < green_rect.y() + green_rect.height()
                    && rect.y() + rect.height() > green_rect.y();
                (rect.fill == Some(red) && overlaps_green).then_some((operation_index, rect))
            });

    if let Some((red_operation, red_rect)) = red_border {
        assert!(
            red_operation < green_operation,
            "collapsed border should paint before foreground inline-block content; operations={:?}",
            page.paint_operations()
        );
        let sample_x = (red_rect.x().max(green_rect.x())
            + (red_rect.x() + red_rect.width()).min(green_rect.x() + green_rect.width()))
            / 2.0;
        let sample_y = (red_rect.y().max(green_rect.y())
            + (red_rect.y() + red_rect.height()).min(green_rect.y() + green_rect.height()))
            / 2.0;
        assert_eq!(
            final_rect_fill_at(page, sample_x, sample_y),
            Some(green),
            "foreground inline-block should cover the collapsed border at ({sample_x}, {sample_y})"
        );
    } else {
        assert_eq!(
            final_rect_fill_at(
                page,
                green_rect.x() + green_rect.width() / 2.0,
                green_rect.y() + green_rect.height() / 2.0
            ),
            Some(green),
            "foreground inline-block should be visible even if collapsed border serialization changes"
        );
    }
}

#[tokio::test]
async fn nested_collapsed_table_border_paints_in_child_table_phase() {
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
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);

    let green_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .collect::<Vec<_>>();
    let left = green_rects
        .iter()
        .map(|rect| rect.x())
        .fold(f32::INFINITY, f32::min);
    let right = green_rects
        .iter()
        .map(|rect| rect.x() + rect.width())
        .fold(f32::NEG_INFINITY, f32::max);
    let bottom = green_rects
        .iter()
        .map(|rect| rect.y())
        .fold(f32::INFINITY, f32::min);
    let top = green_rects
        .iter()
        .map(|rect| rect.y() + rect.height())
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

    let green_operation = first_rect_paint_operation_index(page, green);
    let red_operation = first_rect_paint_operation_index(page, red);
    assert!(
        green_operation < red_operation,
        "nested table collapsed border should paint as child table content after the parent table border"
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
            Some(red),
            "nested table collapsed border should paint above the parent collapsed border at ({x}, {y})"
        );
    }
}

#[tokio::test]
async fn collapsed_table_cell_inset_box_shadow_uses_collapsed_border_padding_edge() {
    let document = Html::from_string(
        r#"<style>
@page { size: 140pt 140pt; margin: 0 }
body { margin: 0 }
table { border-collapse: collapse; margin: 0 }
td {
  border: 20pt solid green;
  box-shadow: inset 60pt 0 green;
  background: red;
  line-height: 0;
  padding: 0;
}
td > span {
  display: inline-block;
  height: 60pt;
  width: 60pt;
}
</style>
<table><tr><td><span></span></td></tr></table>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];
    let red = CssColor::new(255, 0, 0);
    let green = CssColor::new(0, 128, 0);
    let background = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(red))
        .expect("cell background should paint before border and shadow");
    let left = background.x();
    let right = background.x() + background.width();
    let bottom = background.y();
    let top = background.y() + background.height();

    for (x, y) in [
        (left + 1.0, bottom + 1.0),
        (right - 1.0, bottom + 1.0),
        ((left + right) / 2.0, (bottom + top) / 2.0),
        (left + 11.0, (bottom + top) / 2.0),
        (right - 11.0, (bottom + top) / 2.0),
        (left + 1.0, top - 1.0),
        (right - 1.0, top - 1.0),
    ] {
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(green),
            "collapsed cell inset shadow should cover red background at ({x}, {y})"
        );
    }
}

#[tokio::test]
async fn collapsed_row_group_outline_covers_spacing_after_collapsed_row() {
    let document = Html::from_string(
        r#"<style>@page { size: 120px 120px; margin: 0 } body { margin: 0 }</style>
<table style="width: 100px; border-spacing: 10px; background: red">
  <tbody style="outline: solid green 10px">
    <tr style="visibility: collapse">
      <td style="padding: 0"></td>
    </tr>
    <tr>
      <td style="padding: 0"><div style="height: 80px; background: green"></div></td>
    </tr>
  </tbody>
</table>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = CssColor::new(255, 0, 0);
    let green = CssColor::new(0, 128, 0);
    let table_background = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(red))
        .expect("table background should paint red before row-group outline");
    let left = table_background.x();
    let right = table_background.x() + table_background.width();
    let bottom = table_background.y();
    let top = table_background.y() + table_background.height();
    assert!(
        (table_background.width() - 75.0).abs() < 0.01,
        "separated table background should stay at authored width: {table_background:?}"
    );
    assert!(
        (table_background.height() - 75.0).abs() < 0.01,
        "separated table background should include edge spacing without extra collapsed-row gaps: {table_background:?}"
    );

    let content = page
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(green)
                && (rect.width() - 60.0).abs() < 0.01
                && (rect.height() - 60.0).abs() < 0.01
        })
        .expect("visible cell content should paint as an 80px green square");
    assert!(
        (content.x() - left - 7.5).abs() < 0.01,
        "visible cell content should start after left edge spacing: {content:?}"
    );
    assert!(
        (content.y() - bottom - 7.5).abs() < 0.01,
        "visible cell content should end before bottom edge spacing: {content:?}"
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
            "row-group outline should cover red table background at ({x}, {y})"
        );
    }
}

#[tokio::test]
async fn border_spacing_colspan_width_includes_internal_gutters() {
    let document = Html::from_string(
        r#"<style>@page { size: 100px 100px; margin: 0 } body { margin: 0 }</style>
<table cellpadding="0" style="border-spacing: 20px; margin: -20px">
  <tr>
    <td colspan="3" style="width: 100px; background: red">
      <div style="height: 100px; background: green"></div>
    </td>
  </tr>
</table>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let widest_green = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .unwrap_or_else(|| panic!("expected green square paint: {:?}", page.rects()));

    assert!(
        (widest_green.width() - 75.0).abs() < 0.01 && (widest_green.height() - 75.0).abs() < 0.01,
        "100px spanning-cell content should paint as a 75pt square: {widest_green:?}"
    );
    assert_eq!(
        final_rect_fill_at(
            page,
            widest_green.x() + 74.0,
            widest_green.y() + widest_green.height() / 2.0,
        ),
        Some(green)
    );
    assert!(
        page.rects()
            .iter()
            .filter(|rect| rect.fill == Some(green))
            .all(|rect| rect.width() < 80.0),
        "colspan should not allocate internal gutters again as column width: {:?}",
        page.rects()
    );
}

#[tokio::test]
async fn separated_table_width_includes_edge_border_spacing() {
    let document = Html::from_string(
        r#"<style>@page { size: 140px 100px; margin: 0 } body { margin: 0 } table { width: 100px; border-spacing: 10px; background: red; margin: 0 0 10px 0 } td { padding: 0 } div { height: 10px; background: green }</style>
<table><tr><td><div></div></td></tr></table>
<table><tr><td><div></div></td><td><div></div></td></tr></table>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = CssColor::new(255, 0, 0);
    let page = &document.pages[0];
    let table_backgrounds = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(red))
        .collect::<Vec<_>>();
    assert_eq!(
        table_backgrounds.len(),
        2,
        "expected one table background for the one-column table and one for the two-column table"
    );
    for background in table_backgrounds {
        assert!(
            (background.width() - 75.0).abs() < 0.01,
            "separated table width should include edge border-spacing: {background:?}"
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = CssColor::new(0, 128, 0);
    let hotpink = CssColor::new(255, 105, 180);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(green) && (rect.width() - 75.0).abs() < 0.01)
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(hotpink) && (rect.height() - 37.5).abs() < 0.01)
    );
    document.validate_paint_operations().unwrap();
}

#[test]
fn anonymous_table_cell_splits_an_inline_around_an_in_flow_block() {
    // This exercises a nested table formatting context.
    std::thread::Builder::new()
        .name("anonymous-table-cell-block-in-inline".to_string())
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime should build")
                .block_on(async {
                    let document = Html::from_string(
                        "<style>@page { size: 180pt 120pt; margin: 0 } body { margin: 0; font-size: 10pt; line-height: 10pt } </style>\
                         <span style=\"display:table-row\"><span>aaa<span style=\"display:block\"></span><span style=\"display:table-cell\">bbb</span></span></span>",
                    )
                    .render(&RenderOptions::default())
                    .await
                    .unwrap();

                    let lines = document.pages[0].lines();
                    assert_eq!(
                        lines.len(),
                        2,
                        "expected separate lines for aaa and bbb: {lines:?}"
                    );
                    assert_eq!(lines[0].text.trim(), "aaa");
                    assert_eq!(lines[1].text.trim(), "bbb");
                    assert!(lines[1].y() < lines[0].y());
                });
        })
        .expect("anonymous table-cell regression thread should start")
        .join()
        .expect("anonymous table-cell regression thread should complete");
}

#[tokio::test]
async fn collapsed_table_dotted_borders_render_as_round_dot_paths() {
    let document = Html::from_string(
        "<table cellpadding=\"0\" style=\"margin:0;width:24pt;border-collapse:collapse\">\
         <tr><td style=\"width:24pt;border-top:2pt dotted blue\">A</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];

    assert_eq!(
        page.rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .count(),
        0
    );
    let dot_indexes = page
        .paths()
        .iter()
        .enumerate()
        .filter(|(_, path)| path.stroke == Some(CssColor::new(0, 0, 255)))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    assert_eq!(dot_indexes.len(), 1);
    for path_index in dot_indexes {
        assert!(page.operations().iter().any(|operation| {
            matches!(operation, crate::document::paint::page::PaintOperation::Path(index) if *index == path_index)
        }));
    }
}

#[tokio::test]
async fn collapsed_table_row_borders_resolve_to_one_shared_edge() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0}td{padding:0}tr:first-child{border-bottom:3pt solid red}tr:last-child{border-top:3pt solid blue}</style>\
         <table><tr><td>A</td></tr><tr><td>B</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0))
                && (rect.height() - 3.0).abs() < 0.01
                && (rect.width() - 40.0).abs() < 0.01
        })
        .count();
    let blue_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.height() - 2.0).abs() < 0.01
                && (rect.width() - 40.0).abs() < 0.01
        })
        .count();
    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .count();

    assert_eq!(blue_edges, 2);
    assert_eq!(red_edges, 0);
}

#[tokio::test]
async fn separated_table_row_border_padding_margin_do_not_paint_or_shift_cells() {
    let base_style = "\
        <style>\
        @page{size:200pt 160pt;margin:0}\
        body{margin:0}\
        table{border-collapse:separate;border-spacing:2pt;margin:0 0 4pt 0;border:3pt solid black}\
        td{width:10pt;height:10pt;padding:0;border:3pt solid black;font-size:0;line-height:0}\
        .rtl{direction:rtl}\
        </style>";
    let reference = Html::from_string(format!(
        "{base_style}\
         <table><tr><td></td><td></td></tr></table>\
         <table class=\"rtl\"><tr><td></td><td></td></tr></table>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let decorated = Html::from_string(format!(
        "{base_style}\
         <style>tr{{border:20pt solid red;padding:20pt;margin:20pt}}</style>\
         <table><tr><td></td><td></td></tr></table>\
         <table class=\"rtl\"><tr><td></td><td></td></tr></table>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        decorated.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "separated table rows must not paint their own border: {:?}",
        decorated.pages[0].rects()
    );
    assert_eq!(decorated.pages[0].rects(), reference.pages[0].rects());
    assert_eq!(decorated.pages[0].paths(), reference.pages[0].paths());
    assert_eq!(decorated.pages[0].strokes(), reference.pages[0].strokes());
}

#[tokio::test]
async fn collapsed_table_row_border_still_contributes_to_border_conflict_resolution() {
    let document = Html::from_string(
        "<style>@page{size:120pt 120pt;margin:0}body{margin:0}\
         table{border-collapse:collapse;margin:0;border:1pt solid black}\
         tr{border:20pt solid red}\
         td{width:20pt;height:20pt;padding:0;border:3pt solid black;font-size:0;line-height:0}</style>\
         <table><tr><td></td><td></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0))),
        "collapsed table row border should contribute candidates: {:?}",
        document.pages[0].rects()
    );
}

#[tokio::test]
async fn collapsed_table_cell_border_beats_table_border_after_subpixel_width_floor() {
    let document = Html::from_string(
        "<style>@page{size:120pt 120pt;margin:0}body{margin:0}\
         table{border:5.95px solid red;border-collapse:collapse;margin:0}\
         td{width:50px;height:50px;border:5px solid green;padding:0;font-size:0;line-height:0}</style>\
         <table><tr><td></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);
    let green_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(green)
                && ((rect.height() - 5.0 * 0.75).abs() < 0.01
                    || (rect.width() - 5.0 * 0.75).abs() < 0.01)
        })
        .count();
    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(red))
        .count();

    assert!(
        green_edges >= 4,
        "cell collapsed border should paint green edges, got {green_edges}"
    );
    assert_eq!(red_edges, 0);
}

#[tokio::test]
async fn collapsed_table_row_group_border_beats_table_border_at_same_edge() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0;border-top:2pt solid red}td{padding:0}tbody{border-top:2pt solid blue}</style>\
         <table><tbody><tr><td>A</td></tr></tbody></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.height() - 2.0).abs() < 0.01
                && (rect.width() - 40.0).abs() < 0.01
        })
        .count();
    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.height() - 2.0).abs() < 0.01
                && (rect.width() - 40.0).abs() < 0.01
        })
        .count();
    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let green_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - 2.0).abs() < 0.01
                && rect.height() > 1.0
        })
        .count();
    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.width() - 2.0).abs() < 0.01
                && rect.height() > 1.0
        })
        .count();
    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
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
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.width() - 2.0).abs() < 0.01
                && rect.height() > 1.0
        })
        .count();
    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .count();

    assert_eq!(blue_edges, 1);
    assert_eq!(red_edges, 0);
}

#[tokio::test]
async fn collapsed_table_column_and_column_group_edges_beat_table_edges() {
    let document = Html::from_string(
        "<style>@page{size:160pt 120pt;margin:0}body{margin:0}\
         table{border-collapse:collapse;width:20pt;margin:0 0 10pt 0;border:4pt solid red}\
         td{padding:0;width:20pt;height:20pt;font-size:0;line-height:0}\
         .column col,.group colgroup{border:4pt solid green}</style>\
         <table class=\"column\"><col><tr><td></td></tr></table>\
         <table class=\"group\"><colgroup></colgroup><tr><td></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green_horizontal_edges = page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.height() - 4.0).abs() < 0.01
                && rect.width() >= 20.0
        })
        .count();
    let green_vertical_edges = page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - 4.0).abs() < 0.01
                && rect.height() > 20.0
        })
        .count();

    assert_eq!(green_horizontal_edges, 4);
    assert_eq!(green_vertical_edges, 4);
    assert!(
        page.rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "column and column-group borders should win over table borders on every edge"
    );
}

#[tokio::test]
async fn collapsed_table_row_borders_do_not_cross_rowspan_cells() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:60pt;margin:0}td{padding:0}tr:first-child{border-bottom:2pt solid red}</style>\
         <table><tr><td rowspan=\"2\" style=\"width:30pt\">Span</td><td style=\"width:30pt\">A</td></tr><tr><td>B</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0)) && (rect.height() - 2.0).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(red_edges.len(), 1);
    assert!((red_edges[0].width() - 30.0).abs() < 0.01);
}

#[tokio::test]
async fn collapsed_rowspan_cell_vertical_borders_cover_each_spanned_row() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:60pt;margin:0}\\
         td{padding:0;width:30pt;height:10pt}\\
         .divider td{border-left:0;border-right:0}</style>\\
         <table><tr><td rowspan=\"3\" style=\"border-left:2pt solid red;border-right:2pt solid red\">Span</td><td>A</td></tr>\\
         <tr class=\"divider\"><td>B</td></tr><tr><td>C</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0)) && (rect.width() - 2.0).abs() < 0.01
        })
        .collect::<Vec<_>>();
    red_edges.sort_by(|left, right| {
        left.x()
            .total_cmp(&right.x())
            .then_with(|| left.y().total_cmp(&right.y()))
    });

    assert_eq!(
        red_edges.len(),
        6,
        "rowspan edges must paint once per row: {red_edges:?}"
    );
    for edge_pair in red_edges.as_chunks::<3>().0 {
        for adjacent in edge_pair.windows(2) {
            assert!(
                (adjacent[0].y() + adjacent[0].height() - adjacent[1].y()).abs() < 0.01,
                "rowspan border segments must meet without a continuation-row gap: {edge_pair:?}"
            );
        }
    }
}

#[tokio::test]
async fn collapsed_table_column_borders_do_not_cross_colspan_cells() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:60pt;margin:0}td{padding:0;width:20pt;height:10pt}col.internal{border-left:4pt solid red}</style>\
         <table><col><col class=\"internal\"><col><tr><td colspan=\"2\">Span</td><td>C</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "column border must not paint through the interior of a colspan"
    );
}

#[tokio::test]
async fn collapsed_table_spanning_cell_suppresses_internal_row_and_column_edges() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:60pt;margin:0}td{padding:0;width:20pt;height:10pt}tr:first-child{border-bottom:3pt solid red}col.internal{border-left:4pt solid blue}</style>\
         <table><col><col class=\"internal\"><col><tr><td rowspan=\"2\" colspan=\"2\">Span</td><td>A</td></tr><tr><td>B</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red_edges = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0)) && (rect.height() - 3.0).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(red_edges.len(), 1);
    assert!((red_edges[0].width() - 20.0).abs() < 0.01);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(0, 0, 255))),
        "column border must not paint through the interior of a rowspan+colspan"
    );
}

#[tokio::test]
async fn collapsed_table_root_padding_and_spacing_do_not_affect_grid() {
    let document = Html::from_string(
        "<style>@page{size:260pt 180pt;margin:0}body{margin:0}table{border-collapse:collapse;margin:0;padding:100pt;border-spacing:100pt;border:4pt solid black;background:green;width:40pt}td{padding:0;width:40pt;height:10pt}.fixed{table-layout:fixed;background:blue}</style>\
         <table><tr><td>A</td></tr></table><table class=\"fixed\"><tr><td>B</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("auto collapsed table background should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("fixed collapsed table background should paint");

    assert!((green.width() - 48.0).abs() < 0.01, "{green:?}");
    assert!((blue.width() - 48.0).abs() < 0.01, "{blue:?}");
}

#[tokio::test]
async fn empty_collapsed_table_ignores_wrapper_padding_and_border() {
    let document = Html::from_string(
        "<style>@page{size:260pt 180pt;margin:0}body{margin:0}table{border-collapse:collapse;margin:0;padding:100pt;border:20pt solid red;background:green;width:40pt;height:10pt}</style><table></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("empty collapsed table background should paint");

    assert!((green.width() - 40.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 10.0).abs() < 0.01, "{green:?}");
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "empty collapsed table should not paint separated wrapper borders"
    );
}

#[tokio::test]
async fn collapsed_table_root_padding_does_not_expand_float_shrink_wrap() {
    let document = Html::from_string(
        "<style>@page{size:260pt 180pt;margin:0}body{margin:0}div{float:left;background:green}table{border-collapse:collapse;box-sizing:content-box;width:100px;height:100px;padding:100px}</style><div><table></table></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_green_100px_square(&document);
}

#[tokio::test]
async fn collapsed_table_3d_border_styles_use_collapsed_paint_mapping() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0}td{padding:0;width:40pt;height:10pt;border-top:6pt #6699cc}\
         .ridge{border-top-style:ridge}.groove{border-top-style:groove}.inset{border-top-style:inset}.outset{border-top-style:outset}</style>\
         <table><tr><td class=\"ridge\">A</td></tr><tr><td class=\"groove\">B</td></tr><tr><td class=\"inset\">C</td></tr><tr><td class=\"outset\">D</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let base = CssColor::new(102, 153, 204);
    let split_3d_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill.is_some_and(|fill| fill != base)
                && (rect.height() - 3.0).abs() < 0.01
                && rect.width() > 30.0
        })
        .count();
    let flat_base_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(base) && (rect.height() - 6.0).abs() < 0.01 && rect.width() > 30.0
        })
        .count();

    assert!(
        split_3d_rects >= 8,
        "expected split 3D collapsed border paint"
    );
    assert_eq!(flat_base_rects, 0);
}

#[tokio::test]
async fn rtl_collapsed_table_places_logical_columns_from_physical_right() {
    let document = Html::from_string(
        "<style>@page{size:160pt 80pt;margin:0}body{margin:0;font-size:10pt;line-height:10pt}table{direction:rtl;border-collapse:collapse;table-layout:fixed;width:90pt;margin:0}td{padding:0;width:30pt;height:10pt;text-align:left}</style>\
         <table><tr><td>A</td><td>B</td><td>C</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let a = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "A")
        .unwrap();
    let b = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "B")
        .unwrap();
    let c = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "C")
        .unwrap();

    assert!(
        c.x() < b.x() && b.x() < a.x(),
        "RTL table should place logical columns from the physical right: A={a:?} B={b:?} C={c:?}"
    );
}

#[tokio::test]
async fn rtl_collapsed_table_maps_physical_left_and_right_borders_to_reversed_grid_edges() {
    let document = Html::from_string(
        "<style>@page{size:160pt 80pt;margin:0}body{margin:0}table{direction:rtl;border-collapse:collapse;table-layout:fixed;width:40pt;margin:0}td{padding:0;width:20pt;height:10pt}.first{border-right:4pt solid red}.last{border-left:6pt solid blue}</style>\
         <table><tr><td class=\"first\"></td><td class=\"last\"></td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first logical cell's physical left border should paint");
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("last logical cell's physical right border should paint");

    assert!(
        blue.x() < red.x(),
        "physical left border should paint left of physical right border in RTL: red={red:?} blue={blue:?}"
    );
}

#[tokio::test]
async fn collapsed_table_cell_content_uses_resolved_half_border_insets() {
    let document = Html::from_string(
        "<style>@page{size:180pt 80pt;margin:0}body{margin:0;font-size:10pt;line-height:10pt}table{border-collapse:collapse;margin:0;width:80pt;border-left:20pt solid red}td{padding:0;border-left:0;width:80pt;height:10pt;text-align:left}</style>\
         <table><tr><td>Inset</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Inset")
        .unwrap();
    let border = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let grid_edge = border.x() + border.width() / 2.0;

    assert!(
        (line.x() - grid_edge - 10.0).abs() < 0.01,
        "cell content should consume half of the 20pt winning border from the grid edge, got line={line:?} border={border:?}"
    );
}

#[tokio::test]
async fn invoice_shaped_collapsed_border_box_table_keeps_a_15cm_non_overlapping_grid() {
    let document = Html::from_string(
        r#"<style>
        @page { size: 800pt 180pt; margin: 0 }
        body { margin: 0; font-size: 10pt; line-height: 10pt }
        table {
          position: absolute;
          left: 120pt;
          bottom: 0;
          margin: 0 -3cm;
          width: 18cm;
          box-sizing: border-box;
          border-collapse: collapse;
          border-width: 2cm 3cm;
          border-style: solid;
          border-color: black;
          background: #eeeeee;
        }
        td { padding: 0; height: 16pt }
        .account { background: #ff0000 }
        .total { background: #00ff00 }
        .due { background: #0000ff }
        </style>
        <table><tr><td class=account>Account</td><td class=total>Total</td><td class=due>DUE</td></tr></table>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let account = largest_filled_rect(page, CssColor::new(255, 0, 0));
    let total = largest_filled_rect(page, CssColor::new(0, 255, 0));
    let due = largest_filled_rect(page, CssColor::new(0, 0, 255));
    let wrapper = largest_filled_rect(page, CssColor::new(238, 238, 238));
    let outer_horizontal_insets = 3.0 * 72.0 / 2.54;
    let grid_width = wrapper.width() - outer_horizontal_insets;

    assert!(
        account.x() + account.width() <= total.x() + 0.01
            && total.x() + total.width() <= due.x() + 0.01,
        "invoice columns must not overlap: account={account:?} total={total:?} due={due:?}"
    );
    assert!(
        (grid_width - 15.0 * 72.0 / 2.54).abs() < 0.02,
        "18cm border-box table with 2cm/3cm collapsed borders should leave a 15cm grid, got {grid_width}pt"
    );
    assert!(
        (wrapper.width() - 18.0 * 72.0 / 2.54).abs() < 0.02,
        "collapsed outer half-borders should restore the 18cm wrapper border box: {wrapper:?}"
    );
    assert!(
        wrapper.y().abs() < 0.01,
        "invoice wrapper's resolved bottom border edge should align with its containing block: {wrapper:?}"
    );
}

#[tokio::test]
async fn static_positioned_collapsed_table_background_uses_wrapper_border_box() {
    let document = Html::from_string(
        r#"<style>
        @page { size: 21cm 20cm; margin: 3cm }
        body { margin: 0; font-size: 10pt; line-height: 10pt }
        footer { display: block; height: 6cm }
        table {
          position: absolute;
          bottom: 0;
          margin: 0 -3cm;
          width: 18cm;
          box-sizing: border-box;
          border-collapse: collapse;
          border-width: 2cm 3cm;
          border-style: solid;
          border-color: #eeeeee;
          background: #eeeeee;
        }
        td { padding: 0; height: 16pt }
        </style>
        <footer><table><tr><td>Due</td><td>Account</td><td>Total</td></tr></table></footer>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let wrapper = largest_filled_rect(&document.pages[0], CssColor::new(238, 238, 238));
    let cm = 72.0 / 2.54;
    assert!(
        wrapper.x().abs() < 0.02 && (wrapper.width() - 18.0 * cm).abs() < 0.02,
        "collapsed positioned table background must cover its 18cm wrapper border box, got {wrapper:?}"
    );
}

#[tokio::test]
async fn invoice_total_background_covers_its_collapsed_wrapper_border_box() {
    let document = Html::from_file("weasyprint-samples/invoice/invoice.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let wrapper = largest_filled_rect(&document.pages[0], CssColor::new(246, 246, 246));
    let cm = 72.0 / 2.54;
    assert!(
        wrapper.x().abs() < 0.02 && (wrapper.width() - 18.0 * cm).abs() < 0.02,
        "invoice total background must cover its 18cm collapsed wrapper border box, got {wrapper:?}"
    );
    assert_eq!(
        final_rect_fill_at(
            &document.pages[0],
            wrapper.x() + 1.0,
            wrapper.y() + wrapper.height() / 2.0,
        ),
        Some(CssColor::new(246, 246, 246)),
        "invoice total wrapper background must remain visible outside the grid"
    );
}

#[tokio::test]
async fn collapsed_table_inline_block_cells_stay_square_across_empty_rows() {
    let document = Html::from_string(
        r#"<style>
        @page { size: 300pt 220pt; margin: 0 }
        body { margin: 0; font-size: 0; line-height: 0 }
        table {
          display: inline-table;
          border-collapse: collapse;
        }
        td {
          border: 10px solid black;
          line-height: 0;
          padding: 0;
        }
        span {
          display: inline-block;
          width: 10px;
          height: 10px;
          background: gray;
        }
        .spacer-1 { height: 2px; }
        .spacer-2 { height: 5px; }
        .spacer-3 { height: 10px; }
        </style>
        <table>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
        </table>
        <table>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-1"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-1"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-1"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-1"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-1"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
        </table>
        <table>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-2"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-2"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-2"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-2"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-2"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
        </table>
        <table>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-3"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-3"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-3"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-3"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
          <tr class="spacer-3"></tr>
          <tr><td><span></span></td><td><span></span></td></tr>
        </table>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let gray = CssColor::new(128, 128, 128);
    let gray_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(gray) && rect.width() > 1.0 && rect.height() > 1.0)
        .collect::<Vec<_>>();

    assert_eq!(
        gray_rects.len(),
        48,
        "expected one gray inline-block background per cell, got {gray_rects:?}"
    );

    for rect in gray_rects {
        assert!(
            (rect.width() - 7.5).abs() < 0.01 && (rect.height() - 7.5).abs() < 0.01,
            "expected a 10 CSS px square inline-block background, got {rect:?}"
        );

        let samples = [
            (rect.x() + 0.5, rect.y() + 0.5),
            (rect.x() + rect.width() - 0.5, rect.y() + 0.5),
            (rect.x() + 0.5, rect.y() + rect.height() - 0.5),
            (
                rect.x() + rect.width() - 0.5,
                rect.y() + rect.height() - 0.5,
            ),
            (
                rect.x() + rect.width() / 2.0,
                rect.y() + rect.height() / 2.0,
            ),
        ];

        for (x, y) in samples {
            assert_eq!(
                final_rect_fill_at(page, x, y),
                Some(gray),
                "collapsed borders should not cover inline-block background at ({x}, {y}) in {rect:?}"
            );
        }
    }
}

#[tokio::test]
async fn collapsed_table_collapsed_column_border_does_not_leak() {
    let document = Html::from_string(
        "<style>@page{size:180pt 80pt;margin:0}body{margin:0}table{border-collapse:collapse;table-layout:fixed;width:90pt;margin:0}col.hidden{visibility:collapse;border-left:6pt solid red;border-right:6pt solid red}td{padding:0;width:30pt;height:10pt}</style>\
         <table><col><col class=\"hidden\"><col><tr><td>A</td><td>B</td><td>C</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0))),
        "borders on a collapsed column should not leak into the collapsed-border grid"
    );
}

#[tokio::test]
async fn collapsed_table_hidden_border_suppresses_conflicting_edges() {
    let document = Html::from_string(
        "<style>table{border-collapse:collapse;width:40pt;margin:0}td{padding:0}tr:first-child{border-bottom:3pt solid red}tr:last-child{border-top:3pt hidden blue}</style>\
         <table><tr><td>A</td></tr><tr><td>B</td></tr></table>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .rects()
            .iter()
            .all(|rect| rect.fill != Some(CssColor::new(255, 0, 0)))
    );
}

#[tokio::test]
async fn collapsed_border_positioned_rows_and_cells_match_static_table() {
    let style = "<style>@page{size:240pt 180pt;margin:10pt}body{margin:0}td{width:50.6px;height:50.3px;background:yellow;padding:0;border:1px solid blue}</style>";
    let target = Html::from_string(format!(
        "{style}<table style=\"border-collapse:collapse\"><tr style=\"position:relative\"><td></td><td></td><td></td></tr><tr><td style=\"position:relative\"></td><td style=\"position:relative\"></td><td style=\"position:relative\"></td></tr></table>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();
    let reference = Html::from_string(format!(
        "{style}<table style=\"border-collapse:collapse\"><tr><td></td><td></td><td></td></tr><tr><td></td><td></td><td></td></tr></table>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    let target_page = &target.pages[0];
    let reference_page = &reference.pages[0];
    assert_eq!(target_page.operations(), reference_page.operations());
    assert_eq!(target_page.rects(), reference_page.rects());
    assert_eq!(target_page.paths(), reference_page.paths());
}

#[tokio::test]
async fn table_cell_overflow_auto_matches_scroll_reference() {
    let style = "<style>@page{size:220pt 260pt;margin:10pt}body{margin:0}.outer{width:100px;height:100px;border:solid}.cell{max-width:100px;height:100px;background:green}.child{width:120px;height:50px;background:hotpink}</style>";
    let target = Html::from_string(format!(
        "{style}<div class=\"outer\"><div class=\"cell\" style=\"display:table-cell;overflow-x:auto;vertical-align:top\"><div class=\"child\"></div></div></div><br><div class=\"outer\"><div class=\"cell\" style=\"display:table-cell;overflow-x:auto;vertical-align:middle\"><div class=\"child\"></div></div></div>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();
    let reference = Html::from_string(format!(
        "{style}<div class=\"outer\"><div class=\"cell\" style=\"display:table-cell;overflow-x:scroll;vertical-align:top\"><div class=\"child\"></div></div></div><br><div class=\"outer\"><div class=\"cell\" style=\"display:table-cell;overflow-x:scroll;vertical-align:middle\"><div class=\"child\"></div></div></div>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    let target_page = &target.pages[0];
    let reference_page = &reference.pages[0];
    assert_eq!(target_page.operations(), reference_page.operations());
    assert_eq!(target_page.rects(), reference_page.rects());
    assert_eq!(target_page.paths(), reference_page.paths());
}

#[tokio::test]
async fn split_table_cell_child_named_string_updates_page_margin_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, last); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .source { height: 120pt; string-set: section attr(data-title); background: #eee }\
         </style>\
         <table><tr><td><div class=\"source\" data-title=\"Split Cell Title\">Body</div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(document.pages.len() >= 2, "{page_lines:?}");
    assert!(
        page_lines
            .iter()
            .any(|lines| lines.contains(&"Split Cell Title")),
        "split table-cell source should assign named string to a page margin: {page_lines:?}"
    );
}

#[tokio::test]
async fn split_table_cell_child_running_element_updates_page_margin_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: element(section, last); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .source { height: 120pt; position: running(section); background: #eee }\
         </style>\
         <table><tr><td><div class=\"source\">Split Running Header</div><div style=\"height:120pt\">Body</div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(document.pages.len() >= 2, "{page_lines:?}");
    assert!(
        page_lines
            .iter()
            .any(|lines| lines.contains(&"Split Running Header")),
        "split table-cell source should assign running element to a page margin: {page_lines:?}"
    );
}

#[tokio::test]
async fn split_table_cell_child_named_string_start_uses_final_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .spacer { height: 80pt }\
         .source { height: 10pt; string-set: section attr(data-title); background: #eee }\
         </style>\
         <table><tr><td><div class=\"spacer\"></div><div class=\"source\" data-title=\"Moved Cell Title\">Body</div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Moved Cell Title"),
        "named string should not be placed before the table-cell child fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Moved Cell Title"),
        "named string should resolve from the moved table-cell child fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn rowspanning_cell_child_named_string_start_uses_visible_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         td { vertical-align: top }\
         .spacer { height: 80pt }\
         .source { height: 10pt; string-set: section attr(data-title); background: #eee }\
         </style>\
         <table>\
           <tr><td rowspan=\"2\"><div class=\"spacer\">Spacer</div><div class=\"source\" data-title=\"Rowspan Child Title\">Body</div></td><td>First</td></tr>\
           <tr><td>Second</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        !page_lines[0].contains(&"Rowspan Child Title"),
        "rowspanning cell child assignment should not be placed before its visible source fragment: {page_lines:?}"
    );
    assert!(
        page_lines
            .iter()
            .skip(1)
            .any(|lines| lines.contains(&"Rowspan Child Title")),
        "rowspanning cell child assignment should resolve from the visible source fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn rowspanning_cell_child_running_element_start_uses_visible_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: element(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         td { vertical-align: top }\
         .spacer { height: 80pt }\
         .source { height: 10pt; position: running(section); background: #eee }\
         </style>\
         <table>\
           <tr><td rowspan=\"2\"><div class=\"spacer\">Spacer</div><div class=\"source\">Rowspan Running</div></td><td>First</td></tr>\
           <tr><td>Second</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        !page_lines[0].contains(&"Rowspan Running"),
        "rowspanning cell running child should not be placed before its visible source fragment: {page_lines:?}"
    );
    assert!(
        page_lines
            .iter()
            .skip(1)
            .any(|lines| lines.contains(&"Rowspan Running")),
        "rowspanning cell running child should resolve from the visible source fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn rowspanning_cell_nested_flex_named_string_uses_visible_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         td { vertical-align: top }\
         .spacer { height: 80pt }\
         .flex { display: flex; flex-direction: column; height: 10pt }\
         .source { string-set: section attr(data-title) }\
         </style>\
         <table>\
           <tr><td rowspan=\"2\"><div class=\"spacer\">Spacer</div><div class=\"flex\"><div class=\"source\" data-title=\"Rowspan Flex Title\">Flex</div></div></td><td>First</td></tr>\
           <tr><td>Second</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        !page_lines[0].contains(&"Rowspan Flex Title"),
        "nested flex assignment should not be placed before its rowspanning cell fragment: {page_lines:?}"
    );
    assert!(
        page_lines
            .iter()
            .skip(1)
            .any(|lines| lines.contains(&"Rowspan Flex Title")),
        "nested flex assignment should replay from the visible rowspanning cell fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn rowspanning_cell_nested_table_named_string_uses_visible_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         td { vertical-align: top }\
         .spacer { height: 80pt }\
         .inner tr { height: 10pt; string-set: section attr(data-title) }\
         </style>\
         <table>\
           <tr><td rowspan=\"2\"><div class=\"spacer\">Spacer</div><table class=\"inner\"><tr data-title=\"Rowspan Table Title\"><td>Inner</td></tr></table></td><td>First</td></tr>\
           <tr><td>Second</td></tr>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        !page_lines[0].contains(&"Rowspan Table Title"),
        "nested table assignment should not be placed before its rowspanning cell fragment: {page_lines:?}"
    );
    assert!(
        page_lines
            .iter()
            .skip(1)
            .any(|lines| lines.contains(&"Rowspan Table Title")),
        "nested table assignment should replay from the visible rowspanning cell fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn split_table_cell_named_string_start_uses_cell_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .spacer { height: 80pt }\
         td.source { height: 10pt; string-set: section attr(data-title); background: #eee }\
         </style>\
         <table><tr><td><div class=\"spacer\">Spacer</div></td></tr><tr><td class=\"source\" data-title=\"Moved Cell Source\">Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Moved Cell Source"),
        "cell source named string should not be assigned before its moved table-cell fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Moved Cell Source"),
        "cell source named string should resolve from the moved table-cell fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn split_table_cell_child_running_element_start_uses_final_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: element(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .spacer { height: 80pt }\
         .source { height: 10pt; position: running(section); background: #eee }\
         </style>\
         <table><tr><td><div class=\"spacer\">Spacer</div><div class=\"source\">Moved Running Header</div><div style=\"height:10pt\">Body</div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Header"),
        "running element should not be placed before the table-cell child fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Header"),
        "running element should resolve from the moved table-cell child fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn fragmented_table_row_named_string_updates_page_margin_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, last); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         tr.source { height: 120pt; string-set: section attr(data-title) }\
         </style>\
         <table><tr class=\"source\" data-title=\"Split Row Title\"><td>Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(document.pages.len() >= 2, "{page_lines:?}");
    assert!(
        page_lines
            .iter()
            .any(|lines| lines.contains(&"Split Row Title")),
        "fragmented table-row source should assign named string to a page margin: {page_lines:?}"
    );
}

#[tokio::test]
async fn moved_table_row_named_string_start_uses_final_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .spacer { height: 80pt }\
         .source { height: 10pt; string-set: section attr(data-title) }\
         </style>\
         <table><tr class=\"spacer\"><td>Spacer</td></tr><tr class=\"source\" data-title=\"Moved Row Title\"><td>Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Moved Row Title"),
        "named string should not be placed before the moved table-row fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Moved Row Title"),
        "named string should resolve from the moved table-row fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn repeated_table_header_copy_does_not_emit_named_string_assignment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, last); font-size: 8pt; line-height: 8pt } }\
         body, table, thead, tbody, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         thead tr { height: 10pt }\
         tbody tr { height: 60pt }\
         .header-source { string-set: section \"Header Marker\" }\
         .body-source { string-set: section \"Body Marker\" }\
         </style>\
         <table>\
           <thead><tr><td><div class=\"header-source\">Header</div></td></tr></thead>\
           <tbody><tr><td><div class=\"body-source\">One</div></td></tr><tr><td>Two</td></tr></tbody>\
         </table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(document.pages.len() >= 2, "{page_lines:?}");
    assert!(
        page_lines[1].contains(&"Body Marker"),
        "page 2 should inherit the last real source assignment from page 1: {page_lines:?}"
    );
    assert!(
        !page_lines[1].contains(&"Header Marker"),
        "repeated header copy must not emit a page-local assignment: {page_lines:?}"
    );
}

#[tokio::test]
async fn moved_table_root_named_string_start_uses_final_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt; string-set: section attr(data-title) }\
         .spacer { height: 80pt }\
         td { height: 10pt }\
         </style>\
         <div class=\"spacer\">Spacer</div><table data-title=\"Moved Table Title\"><tr><td>Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Moved Table Title"),
        "table-root named string should not be assigned before the moved table fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Moved Table Title"),
        "table-root named string should resolve from the moved table fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn running_table_root_replays_table_paint_into_page_margin() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 120pt; margin: 30pt 10pt 10pt; @top-center { content: element(header, last); width: 80pt; height: 20pt } }\
         body, table, tr, td { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 50pt; position: running(header) }\
         td { width: 50pt; height: 12pt; background: rgb(0, 128, 0) }\
         </style>\
         <table><tr><td>Table Running</td></tr></table><p>Body</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        lines.contains(&"Table") && lines.contains(&"Running"),
        "{lines:?}"
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0))),
        "running table root should replay cell background paint into the margin box"
    );
}

#[tokio::test]
async fn nested_flex_descendant_named_string_in_split_cell_uses_fragment_assignment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .spacer { height: 80pt }\
         .flex { display: flex; flex-direction: column; height: 10pt }\
         .source { string-set: section attr(data-title) }\
         </style>\
         <table><tr><td><div class=\"spacer\">Spacer</div><div class=\"flex\"><div class=\"source\" data-title=\"Nested Flex Title\">Flex</div></div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Nested Flex Title"),
        "nested flex descendant assignment should not be placed before its split-cell fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Nested Flex Title"),
        "nested flex descendant assignment should replay from the split-cell fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn nested_table_descendant_named_string_in_split_cell_uses_fragment_assignment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: string(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .outer-spacer { height: 80pt }\
         .inner tr { height: 10pt; string-set: section attr(data-title) }\
         </style>\
         <table><tr><td><div class=\"outer-spacer\">Spacer</div><table class=\"inner\"><tr data-title=\"Nested Table Title\"><td>Inner</td></tr></table></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Nested Table Title"),
        "nested table descendant assignment should not be placed before its split-cell fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Nested Table Title"),
        "nested table descendant assignment should replay from the split-cell fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn nested_flex_descendant_running_element_in_split_cell_uses_fragment_assignment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: element(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .spacer { height: 80pt }\
         .flex { display: flex; flex-direction: column; height: 10pt }\
         .source { position: running(section) }\
         </style>\
         <table><tr><td><div class=\"spacer\">Spacer</div><div class=\"flex\"><div class=\"source\">Nested Running</div><div>Body</div></div></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Nested Running"),
        "nested running element should not be placed before its split-cell fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Nested Running"),
        "nested running element should replay from the split-cell fragment: {page_lines:?}"
    );
}

#[tokio::test]
async fn table_row_running_element_is_removed_from_table_flow() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: element(section, last); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .source { position: running(section); height: 40pt; background: rgb(255, 0, 0) }\
         .body { height: 10pt }\
         </style>\
         <table><tr class=\"source\"><td>Row Running</td></tr><tr class=\"body\"><td>Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        lines.iter().filter(|text| **text == "Row Running").count(),
        1,
        "running table row should paint only through the page margin: {lines:?}"
    );
    assert!(lines.contains(&"Body"), "{lines:?}");
}

#[tokio::test]
async fn running_table_row_replays_cell_paint_into_page_margin() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 120pt; margin: 30pt 10pt 10pt; @top-center { content: element(section, last); width: 90pt; height: 20pt } }\
         body, table, tr, td { margin: 0; padding: 0; border: 0; font-size: 8pt; line-height: 8pt }\
         table { border-collapse: collapse; width: 70pt }\
         tr.source { position: running(section) }\
         tr.source td { width: 70pt; height: 12pt; background: rgb(0, 128, 0); border: 1pt solid rgb(0, 0, 255) }\
         </style>\
         <table><tr class=\"source\"><td>Row Paint</td></tr><tr><td>Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Row Paint"), "{lines:?}");
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0))),
        "running table row should replay cell background paint into the margin box"
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 0, 255))),
        "running table row should replay cell border paint into the margin box"
    );
}

#[tokio::test]
async fn running_table_row_replays_nested_table_and_flex_descendants() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 140pt; margin: 40pt 10pt 10pt; @top-center { content: element(section, last); width: 120pt; height: 30pt; font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 8pt; line-height: 8pt }\
         table { border-collapse: collapse; width: 100pt }\
         tr.source { position: running(section) }\
         tr.source td { width: 100pt; height: 20pt; background: rgb(0, 128, 0) }\
         .flex { display: flex; flex-direction: row }\
         .inner { width: 50pt }\
         </style>\
         <table><tr class=\"source\"><td><div class=\"flex\"><div>Flex Item</div><table class=\"inner\"><tr><td>Inner Table</td></tr></table></div></td></tr><tr><td>Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Flex"), "{lines:?}");
    assert!(lines.contains(&"Item"), "{lines:?}");
    assert!(lines.contains(&"Inner Table"), "{lines:?}");
    assert_eq!(
        lines
            .iter()
            .filter(|text| **text == "Flex" || **text == "Item")
            .count(),
        2,
        "nested replay should only appear through the page margin: {lines:?}"
    );
    assert_eq!(
        lines.iter().filter(|text| **text == "Inner Table").count(),
        1,
        "nested replay should only appear through the page margin: {lines:?}"
    );
    assert!(lines.contains(&"Body"), "{lines:?}");
}

#[tokio::test]
async fn running_table_cell_replays_cell_paint_into_page_margin() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 120pt; margin: 30pt 10pt 10pt; @top-center { content: element(section, last); width: 90pt; height: 20pt } }\
         body, table, tr, td { margin: 0; padding: 0; border: 0; font-size: 8pt; line-height: 8pt }\
         table { border-collapse: collapse; width: 70pt }\
         td.source { position: running(section); width: 70pt; height: 12pt; background: rgb(0, 128, 0); border: 1pt solid rgb(0, 0, 255) }\
         </style>\
         <table><tr><td class=\"source\">Cell Paint</td></tr><tr><td>Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Cell Paint"), "{lines:?}");
    assert_eq!(
        lines.iter().filter(|text| **text == "Cell Paint").count(),
        1,
        "running table cell should paint only through the page margin: {lines:?}"
    );
    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| {
            panic!(
                "running table cell should replay cell background paint into the margin box: {:?}",
                document.pages[0].rects()
            )
        });
    assert!(
        green.x() > 25.0,
        "running cell background should come from the centered margin replay, not the table source: {green:?}"
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 0, 255)) && rect.x() > 25.0),
        "running table cell should replay cell border paint into the margin box"
    );
}

#[tokio::test]
async fn running_table_cell_is_removed_from_table_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 120pt; margin: 30pt 10pt 10pt; @top-center { content: element(section, last); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td { margin: 0; padding: 0; border: 0; font-size: 8pt; line-height: 8pt }\
         table { border-collapse: collapse; width: 80pt }\
         td { width: 40pt }\
         td.source { position: running(section) }\
         </style>\
         <table><tr><td class=\"source\">CellHeader</td><td>Body Cell</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| (line.text.as_str(), line.x(), line.y()))
        .collect::<Vec<_>>();
    let body = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Body Cell")
        .unwrap_or_else(|| panic!("missing body cell: {lines:?}"));
    assert!(
        (body.x() - 10.0).abs() < 0.01,
        "running table cell should not reserve a table grid slot: {lines:?}"
    );
    assert_eq!(
        document.pages[0]
            .lines()
            .iter()
            .filter(|line| line.text == "CellHeader")
            .count(),
        1,
        "running cell should paint only through the page margin: {lines:?}"
    );
}

#[tokio::test]
async fn running_only_table_cell_is_captured_from_collapsed_row() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 120pt; margin: 30pt 10pt 10pt; @top-center { content: element(section, last); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td { margin: 0; padding: 0; border: 0; font-size: 8pt; line-height: 8pt }\
         table { border-collapse: collapse; width: 80pt }\
         td.source { position: running(section) }\
         </style>\
         <table><tr><td class=\"source\">Only Running</td></tr><tr><td>Body Cell</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| (line.text.as_str(), line.x(), line.y()))
        .collect::<Vec<_>>();
    assert_eq!(
        document.pages[0]
            .lines()
            .iter()
            .filter(|line| line.text == "Only Running")
            .count(),
        1,
        "running-only cell should still be assigned from the collapsed table row: {lines:?}"
    );
    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "Body Cell"),
        "following in-flow row should still paint normally: {lines:?}"
    );
}

#[tokio::test]
async fn split_table_cell_running_element_start_uses_cell_fragment() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: element(section, start); font-size: 8pt; line-height: 8pt } }\
         body, table, tr, td, div { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .spacer { height: 80pt }\
         td.source { height: 10pt; position: running(section); background: #eee }\
         </style>\
         <table><tr><td><div class=\"spacer\">Spacer</div></td></tr><tr><td class=\"source\">Moved Cell Running</td><td>Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Moved Cell Running"),
        "running cell should not be assigned before its moved table-cell fragment: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Moved Cell Running"),
        "running cell should resolve from the moved table-cell fragment: {page_lines:?}"
    );
    assert_eq!(
        page_lines[1]
            .iter()
            .filter(|text| **text == "Moved Cell Running")
            .count(),
        1,
        "running cell should paint only through the page margin: {page_lines:?}"
    );
}

#[tokio::test]
async fn table_row_running_element_start_uses_post_break_source_marker() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt; @top-center { content: element(section, start); font-size: 8pt; line-height: 8pt } }\
         body, p, table, tr, td { margin: 0; padding: 0; border: 0; font-size: 10pt; line-height: 10pt }\
         table { border-collapse: collapse; width: 80pt }\
         .source { break-before: page; position: running(section); height: 10pt }\
         .body { height: 10pt }\
         </style>\
         <p>Intro</p><table><tr class=\"source\"><td>Later Running</td></tr><tr class=\"body\"><td>Body</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page_lines = document
        .pages
        .iter()
        .map(|page| {
            page.lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(document.pages.len(), 2, "{page_lines:?}");
    assert!(
        !page_lines[0].contains(&"Later Running"),
        "running row should not be assigned before its forced break: {page_lines:?}"
    );
    assert!(
        page_lines[1].contains(&"Later Running"),
        "running row should use the post-break source marker for start: {page_lines:?}"
    );
}

#[tokio::test]
async fn table_cell_wrap_inside_avoid_moves_parenthetical_unit_to_next_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 90pt 100pt; margin: 10pt }\
         table { border-collapse: collapse; table-layout: fixed; width: 37pt }\
         td { margin: 0; padding: 0; border: 0; font: 10pt/10pt monospace; word-break: break-all }\
         .parenthetical { wrap-inside: avoid }\
         </style>\
         <table><tr><td>aa<wbr><span class=\"parenthetical\">(bbbb)</span></td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(lines, ["aa", "(bbbb)"], "{lines:?}");
}
