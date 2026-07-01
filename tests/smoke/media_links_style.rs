use super::*;

#[tokio::test]
async fn renders_png_data_uri_images() {
    let html = Html::from_string(
        "<img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\" height=\"10pt\">",
    );
    let document = html.render_async(&RenderOptions::default()).await.unwrap();

    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(document.pages[0].images[0].pixel_width, 1);
    assert_eq!(document.pages[0].images[0].height(), 10.0);

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/Subtype /Image"));
    assert!(rendered.contains("/Interpolate false"));
    assert!(rendered.contains("/Im1 Do"));
}

#[tokio::test]
async fn embeds_png_alpha_as_pdf_soft_mask() {
    let html = Html::from_string(
        "<img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DQAAAEgQGALFXOsAAAAABJRU5ErkJggg==\" height=\"10\">",
    );
    let document = html.render_async(&RenderOptions::default()).await.unwrap();

    assert_eq!(
        document.pages[0].images[0].alpha.as_deref(),
        Some(&[128][..])
    );

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/SMask"));
    assert!(rendered.contains("/ColorSpace /DeviceGray"));
    assert!(rendered.matches("/Interpolate false").count() >= 2);
}

#[tokio::test]
async fn supports_percentage_image_widths() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 300pt; margin: 10pt } body { margin: 0 } img { width: 100%; }</style><div style=\"margin:0; width:50%\"><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].images[0].width(), 90.0);
}

#[tokio::test]
async fn floated_percentage_width_replays_resolved_used_width_once() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 120pt; margin: 0 } body { margin: 0 }\
         .test { float: left; width: 33.3333% }\
         p { margin: 0 10pt 0 0; height: 20pt; background: #ccc }</style>\
         <div class=\"test\"><p></p></div><div class=\"test\"><p></p></div><div class=\"test\"><p></p></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut rects = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(204, 204, 204)))
        .collect::<Vec<_>>();
    rects.sort_by(|a, b| a.x().total_cmp(&b.x()));

    assert_eq!(rects.len(), 3);
    for (rect, expected_x) in rects.iter().zip([0.0, 120.0, 240.0]) {
        assert!(
            (rect.x() - expected_x).abs() < 0.01,
            "expected float child at x={expected_x}, got {rect:?}"
        );
        assert!(
            (rect.width() - 110.0).abs() < 0.01,
            "float percentage width should not be resolved twice: {rect:?}"
        );
    }
}

#[tokio::test]
async fn floated_border_box_percentage_width_replay_preserves_content_width() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 0 } body { margin: 0 }\
         .float { float: left; width: 50%; box-sizing: border-box; padding: 0 10pt;\
                  border-left: 5pt solid green; border-right: 5pt solid green }\
         .fill { height: 20pt; background: blue }</style>\
         <div class=\"float\"><div class=\"fill\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let fill = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .expect("blue child should render");

    assert!((fill.x() - 15.0).abs() < 0.01, "blue child: {fill:?}");
    assert!(
        (fill.width() - 70.0).abs() < 0.01,
        "border-box float replay should preserve the resolved content width: {fill:?}"
    );
}

#[tokio::test]
async fn hidden_float_placeholders_reserve_reference_grid_cells() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>\
         @page { size: 900px 600px; margin: 0 } body { margin: 0 }\
         .flexContainer { height: 60px; width: 60px; font: 10px sans-serif;\
             background: yellow; float: left; border: 1px solid black }\
         .flexContainer > * { border: 1px dotted gray; width: 28px; height: 28px;\
             float: left }\
         .hidden { visibility: hidden }\
         </style>\
         <div class=\"flexContainer\"><div>1</div><div>2</div><div>3</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>2</div><div>1</div><div class=\"hidden\">4</div><div>3</div></div>\
         <div class=\"flexContainer\"><div>1</div><div>3</div><div>2</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>2</div><div class=\"hidden\">4</div><div>1</div><div>3</div></div>\
         <div style=\"clear:both\"></div>\
         <div class=\"flexContainer\"><div>1</div><div>2</div><div>3</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>2</div><div>1</div><div class=\"hidden\">4</div><div>3</div></div>\
         <div class=\"flexContainer\"><div>1</div><div>3</div><div>2</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>2</div><div class=\"hidden\">4</div><div>1</div><div>3</div></div>\
         <div style=\"clear:both\"></div>\
         <div class=\"flexContainer\"><div>3</div><div class=\"hidden\">4</div><div>1</div><div>2</div></div>\
         <div class=\"flexContainer\"><div class=\"hidden\">4</div><div>3</div><div>2</div><div>1</div></div>\
         <div class=\"flexContainer\"><div>3</div><div>1</div><div class=\"hidden\">4</div><div>2</div></div>\
         <div class=\"flexContainer\"><div class=\"hidden\">4</div><div>2</div><div>3</div><div>1</div></div>\
         <div style=\"clear:both\"></div>\
         <div class=\"flexContainer\"><div>3</div><div class=\"hidden\">4</div><div>1</div><div>2</div></div>\
         <div class=\"flexContainer\"><div class=\"hidden\">4</div><div>3</div><div>2</div><div>1</div></div>\
         <div class=\"flexContainer\"><div>3</div><div>1</div><div class=\"hidden\">4</div><div>2</div></div>\
         <div class=\"flexContainer\"><div class=\"hidden\">4</div><div>2</div><div>3</div><div>1</div></div>\
         <div style=\"clear:both\"></div>\
         <div class=\"flexContainer\"><div>1</div><div>2</div><div>3</div><div class=\"hidden\">4</div></div>\
         <div class=\"flexContainer\"><div>3</div><div class=\"hidden\">4</div><div>1</div><div>2</div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let mut containers = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 255, 0)))
        .collect::<Vec<_>>();
    containers.sort_by(|a, b| {
        b.y()
            .total_cmp(&a.y())
            .then_with(|| a.x().total_cmp(&b.x()))
    });

    let expected = [
        [Some("1"), Some("2"), Some("3"), None],
        [Some("2"), Some("1"), None, Some("3")],
        [Some("1"), Some("3"), Some("2"), None],
        [Some("2"), None, Some("1"), Some("3")],
        [Some("1"), Some("2"), Some("3"), None],
        [Some("2"), Some("1"), None, Some("3")],
        [Some("1"), Some("3"), Some("2"), None],
        [Some("2"), None, Some("1"), Some("3")],
        [Some("3"), None, Some("1"), Some("2")],
        [None, Some("3"), Some("2"), Some("1")],
        [Some("3"), Some("1"), None, Some("2")],
        [None, Some("2"), Some("3"), Some("1")],
        [Some("3"), None, Some("1"), Some("2")],
        [None, Some("3"), Some("2"), Some("1")],
        [Some("3"), Some("1"), None, Some("2")],
        [None, Some("2"), Some("3"), Some("1")],
        [Some("1"), Some("2"), Some("3"), None],
        [Some("3"), None, Some("1"), Some("2")],
    ];

    assert_eq!(
        containers.len(),
        expected.len(),
        "containers={containers:?}"
    );
    for (container_index, (container, expected_cells)) in
        containers.iter().zip(expected).enumerate()
    {
        let mut actual = [None, None, None, None];
        for line in document.pages[0].lines.iter().filter(|line| {
            line.x() >= container.x() - 0.01
                && line.x() <= container.x() + container.width() + 0.01
                && line.y() >= container.y() - 0.01
                && line.y() <= container.y() + container.height() + 0.01
        }) {
            let column = if line.x() < container.x() + container.width() / 2.0 {
                0
            } else {
                1
            };
            let row = if line.y() > container.y() + container.height() / 2.0 {
                0
            } else {
                1
            };
            actual[row * 2 + column] = Some(line.text.as_str());
        }
        assert_eq!(
            actual, expected_cells,
            "container {container_index} should match reference grid cells: container={container:?}, lines={:?}",
            document.pages[0].lines
        );
    }
}

#[tokio::test]
async fn direct_inline_images_reserve_baseline_descent_in_line_box() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 160pt; margin: 20pt } body, div, p { margin: 0; font-size: 12pt; line-height: 12pt } img { height: 20pt }</style>\
         <div><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"></div><p>After</p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let image = &document.pages[0].images[0];
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!(image.y() - after.y() >= 12.0 - 0.01);
}

#[tokio::test]
async fn anonymous_inline_runs_layout_replaced_atoms_with_text() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 160pt; margin: 20pt } body, div, p { margin: 0; font-size: 12pt; line-height: 12pt } img { width: 10pt; height: 10pt }</style>\
         <div>Before <img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"> After<p>Block</p></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let image = &document.pages[0].images[0];
    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(image.x() > before.x());
    assert!(after.x() > image.x());
    assert!((before.y() - after.y()).abs() < 0.1);
}

#[tokio::test]
async fn inline_formatting_context_places_atomic_image_between_text_fragments() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 20pt } body, div { margin: 0; font-size: 12pt; line-height: 12pt } img { width: 10pt; height: 10pt }</style>\
         <div>Before <img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"> After</div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let image = &document.pages[0].images[0];
    let before = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let after = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(image.x() > before.x());
    assert!(after.x() > image.x());
    assert!((before.y() - after.y()).abs() < 0.1);
}

#[tokio::test]
async fn flex_replaced_images_use_border_box_for_flex_distribution() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 260pt 100pt; margin: 10pt }} body {{ margin: 0 }} .flex {{ width: 200pt; display: flex; line-height: 8pt }} img {{ min-width: 0; width: 10pt; height: 20pt; border: 1pt dotted green }}</style>\
         <div class=\"flex\"><img src=\"{image}\" style=\"flex: 5\"><img src=\"{image}\" style=\"flex: 3\"></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let images = &document.pages[0].images;
    assert_eq!(images.len(), 2);
    assert!((images[0].width() - 122.5).abs() < 0.01);
    assert!((images[1].width() - 73.5).abs() < 0.01);
    assert!((images[1].x() - images[0].x() - 124.5).abs() < 0.01);
}

#[tokio::test]
async fn column_flex_replaced_image_min_height_transfers_through_aspect_ratio() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 180pt; margin: 10pt }} body {{ margin:0 }}\
         .flex {{ display:flex; flex-direction:column; align-items:flex-start; width:75pt; height:150pt }}\
         img {{ min-height:75pt; flex:1 0 auto }} .spacer {{ flex:1 0 1pt }}\
         </style><div class=\"flex\"><img src=\"{image}\"><div class=\"spacer\"></div></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let image = &document.pages[0].images[0];
    assert!((image.width() - 75.0).abs() < 0.01, "image={image:?}");
    assert!((image.height() - 75.0).abs() < 0.01, "image={image:?}");
}

#[tokio::test]
async fn column_flex_replaced_image_cross_min_width_transfers_to_main_basis() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAAFCAYAAABvsz2cAAAAFElEQVR4nGNg+A+EDUAMJkAQtwgAfnURcnh7KuYAAAAASUVORK5CYII=";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 260pt 160pt; margin: 10pt }} body {{ margin:0 }}\
         .flex {{ display:flex; flex-direction:column; align-items:flex-start; width:200pt }}\
         img {{ min-width:20%; min-height:0 }}\
         </style><div class=\"flex\"><img src=\"{image}\"></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let image = &document.pages[0].images[0];
    assert!((image.width() - 40.0).abs() < 0.01, "image={image:?}");
    assert!((image.height() - 100.0).abs() < 0.01, "image={image:?}");
}

#[tokio::test]
async fn inline_floated_image_is_removed_from_text_flow_and_shifted_right() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } div { width: 200pt; height: 22pt } img { width: 10pt; height: 20pt; border: 1pt dotted green; float: right }</style>\
         <div>some words <img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines
        .iter()
        .find(|line| line.text == "some words")
        .unwrap();
    let image = &document.pages[0].images[0];
    assert!((text.x() - 10.0).abs() < 0.01);
    assert!((image.x() - 199.0).abs() < 0.01);
}

#[tokio::test]
async fn inline_text_after_left_float_uses_float_exclusion_and_line_end_tracking() {
    let document = Html::from_string(
        "<style>\
         @page { size: 300pt 160pt; margin: 20pt }\
         body, div { margin: 0 }\
         div { font-family: monospace; font-size: 30pt; line-height: 30pt }\
         span { float: left; letter-spacing: 1ch }\
         </style>\
         <div>12345</div><div><span>aa</span>a</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    let ruler = lines.iter().find(|line| line.text == "12345").unwrap();
    let floated = lines.iter().find(|line| line.text == "aa").unwrap();
    let following = lines
        .iter()
        .filter(|line| line.text == "a")
        .max_by(|left, right| {
            left.x()
                .partial_cmp(&right.x())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();
    let ch = rendered_line_advance(ruler) / 5.0;

    assert!((floated.x() - ruler.x()).abs() < 0.01);
    assert!(
        (following.x() - (ruler.x() + ch * 3.0)).abs() < ch * 0.2,
        "expected following a under the fourth ruler column: ruler x={}, ch={}, following x={}",
        ruler.x(),
        ch,
        following.x()
    );
}

#[tokio::test]
async fn block_image_avoids_active_left_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 30pt; height: 30pt; background: green }\
         img { display: block; width: 20pt; height: 10pt }</style>\
         <div class=\"float\"></div><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\">",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let image = &document.pages[0].images[0];
    assert!(
        image.x() >= 39.0,
        "block image should avoid active float: {image:?}"
    );
}

#[tokio::test]
async fn clear_both_moves_block_image_below_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 40pt; height: 20pt; background: green }\
         img { display: block; clear: both; width: 10pt; height: 10pt }</style>\
         <div class=\"float\"></div><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\">",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let image = &document.pages[0].images[0];

    assert!(
        image.y() + image.height() <= green.y() + 0.01,
        "clear block image should start below float: green={green:?} image={image:?}"
    );
}

#[tokio::test]
async fn overwide_block_image_moves_below_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 30pt; height: 20pt; background: green }\
         img { display: block; width: 90pt; height: 10pt }</style>\
         <div class=\"float\"></div><img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\">",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let image = &document.pages[0].images[0];

    assert!((image.x() - 10.0).abs() < 0.01, "image={image:?}");
    assert!(
        image.y() + image.height() <= green.y() + 0.01,
        "overwide block image should move below float: green={green:?} image={image:?}"
    );
}

#[tokio::test]
async fn block_canvas_and_svg_avoid_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body, canvas, svg { margin: 0 }\
         .float { float: left; width: 30pt; height: 40pt; background: green }\
         canvas { display: block; width: 20pt; height: 10pt; background: blue }\
         svg { display: block; width: 20pt; height: 10pt }</style>\
         <div class=\"float\"></div><canvas></canvas><svg><rect width=\"20pt\" height=\"10pt\" fill=\"#ff0000\"></rect></svg>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 0, 255)))
        .unwrap();
    let red = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        blue.x() >= 39.0,
        "block canvas should avoid active float: {blue:?}"
    );
    assert!(
        red.x() >= 39.0,
        "block svg should avoid active float: {red:?}"
    );
}

#[tokio::test]
async fn flow_root_auto_height_expands_to_contain_internal_float() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 140pt; margin: 10pt }\
         body, div { margin: 0 }\
         .root { display: flow-root; width: 100pt; background: rgb(0 128 0) }\
         .float { float: left; width: 30pt; height: 40pt; background: rgb(0 0 255) }\
         </style>\
         <div class=\"root\"><div class=\"float\"></div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let root = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();

    assert!(
        root.height() >= 39.99,
        "flow-root background should include its internal float: {root:?}"
    );
}

#[tokio::test]
async fn internal_flow_root_float_does_not_leak_to_following_sibling() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body, div { margin: 0 }\
         .root { display: flow-root; width: 100pt; background: rgb(0 128 0) }\
         .float { float: left; width: 30pt; height: 40pt; background: rgb(0 0 255) }\
         .after { width: 100pt; height: 10pt; background: rgb(255 0 0) }\
         </style>\
         <div class=\"root\"><div class=\"float\"></div></div><div class=\"after\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let root = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(0, 128, 0)))
        .unwrap();
    let after = document.pages[0]
        .rects
        .iter()
        .find(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .unwrap();

    assert!(
        after.y() + after.height() <= root.y() + 0.01,
        "following sibling should start below the flow-root: root={root:?} after={after:?}"
    );
}

#[tokio::test]
async fn renders_simple_svg_rects_in_table_cells() {
    let document = Html::from_string(
        "<table style=\"margin:0;width:100pt\"><tr><td><svg width=\"15pt\" height=\"15pt\"><rect width=\"15pt\" height=\"15pt\" fill=\"#2292d4\"></rect></svg></td><td>Half Match</td></tr></table>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.width() == 15.0
                && rect.height() == 15.0
                && rect.fill == Some(Color::new(34, 146, 212)))
    );
    let text = document.pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("Half"));
    assert!(text.contains("Match"));
}

#[tokio::test]
async fn renders_uri_link_annotations() {
    let document = Html::from_string("<a href=\"https://example.com\">Example</a>")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].links.len(), 1);
    assert_eq!(document.pages[0].links[0].target, "https://example.com");

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/Subtype /Link"));
    assert!(rendered.contains("/URI (https://example.com)"));
    assert!(rendered.contains("/Annots ["));
}

#[tokio::test]
async fn draws_text_decorations() {
    let document = Html::from_string(
        "<p style=\"margin: 0; color: red; text-decoration: underline line-through\">Decorated</p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages[0].rects.len() >= 2);
    assert!(
        document.pages[0]
            .rects
            .iter()
            .all(|rect| rect.fill == Some(Color::new(255, 0, 0)))
    );
}

#[tokio::test]
async fn vertical_filled_text_emphasis_adds_sesame_marks() {
    let document = Html::from_string(
        "<div style=\"writing-mode: vertical-rl; text-emphasis-style: filled\">試験テスト</div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = document
        .pages
        .iter()
        .flat_map(|page| &page.lines)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"試験テスト"), "{lines:?}");
    assert_eq!(
        lines.iter().filter(|line| **line == "\u{FE45}").count(),
        5,
        "{lines:?}"
    );
}

#[tokio::test]
async fn preserves_basic_styled_inline_runs() {
    let document = Html::from_string(
        "<p style=\"margin:0;font-size:12pt\">A <em>italic</em> <strong>bold</strong> <small>small</small> ref<sup>1</sup></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    assert!(
        lines
            .iter()
            .any(|line| line_run_font_is_italic(&document, line, "italic"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line_run_font_is_bold(&document, line, "bold"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.text == "small" && (line.font_size - 10.0).abs() < 0.01)
    );

    let reference = lines.iter().find(|line| line.text == "ref").unwrap();
    let superscript = lines.iter().find(|line| line.text == "1").unwrap();
    assert!(superscript.font_size < reference.font_size);
    assert!(superscript.y() > reference.y());
}

#[tokio::test]
async fn supports_authored_vertical_align_super_and_sub() {
    let document = Html::from_string(
        "<p style=\"margin:0;font-size:12pt\">Base<span style=\"vertical-align: super; font-size: 9pt\">up</span><span style=\"vertical-align: sub; font-size: 9pt\">down</span><sup style=\"vertical-align: baseline\">flat</sup></p>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines;
    let base = lines.iter().find(|line| line.text == "Base").unwrap();
    let up = lines.iter().find(|line| line.text == "up").unwrap();
    let down = lines.iter().find(|line| line.text == "down").unwrap();
    let flat = lines.iter().find(|line| line.text == "flat").unwrap();

    assert!(up.y() > base.y());
    assert!(down.y() < base.y());
    assert!(flat.y() < up.y());
}

#[tokio::test]
async fn vertical_align_length_and_percentage_shift_inline_baselines() {
    let document = Html::from_string(
        "<p style=\"margin:0;font-size:20pt;line-height:20pt\">\
         Base<span style=\"vertical-align:10pt\">up</span>\
         <span style=\"vertical-align:-10pt\">down</span>\
         <span style=\"vertical-align:50%;line-height:20pt\">pct</span></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    let base = lines.iter().find(|line| line.text == "Base").unwrap();
    let up = lines.iter().find(|line| line.text == "up").unwrap();
    let down = lines.iter().find(|line| line.text == "down").unwrap();
    let pct = lines.iter().find(|line| line.text == "pct").unwrap();

    assert!(
        up.y() > base.y() + 9.0,
        "positive vertical-align length should raise the inline box: base={base:?}, up={up:?}"
    );
    assert!(
        down.y() < base.y() - 9.0,
        "negative vertical-align length should lower the inline box: base={base:?}, down={down:?}"
    );
    assert!(
        (pct.y() - up.y()).abs() < 1.0,
        "50% of a 20pt line-height should match a 10pt shift: up={up:?}, pct={pct:?}"
    );
}

#[tokio::test]
async fn baseline_shift_longhand_moves_inline_baselines() {
    let document = Html::from_string(
        "<p style=\"margin:0;font-size:20pt;line-height:20pt\">\
         Base<span style=\"baseline-shift:10pt\">up</span>\
         <span style=\"baseline-shift:-10pt\">down</span>\
         <span style=\"baseline-shift:50%;line-height:20pt\">pct</span></p>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let lines = &document.pages[0].lines;
    let base = lines.iter().find(|line| line.text == "Base").unwrap();
    let up = lines.iter().find(|line| line.text == "up").unwrap();
    let down = lines.iter().find(|line| line.text == "down").unwrap();
    let pct = lines.iter().find(|line| line.text == "pct").unwrap();

    assert!(up.y() > base.y() + 9.0);
    assert!(down.y() < base.y() - 9.0);
    assert!((pct.y() - up.y()).abs() < 1.0);
}
