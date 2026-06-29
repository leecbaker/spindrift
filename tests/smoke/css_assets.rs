use super::*;

fn image_xobject_count_with_size(rendered: &str, width: u32, height: u32) -> usize {
    rendered
        .split("/Subtype /Image")
        .skip(1)
        .filter(|object| {
            object.contains(&format!("/Width {width}"))
                && object.contains(&format!("/Height {height}"))
        })
        .count()
}

fn filled_rect(page: &quire::Page, color: Color) -> &quire::RenderedRect {
    page.rects
        .iter()
        .find(|rect| rect.fill == Some(color))
        .unwrap_or_else(|| {
            panic!(
                "expected filled rect with color {color:?} in {:?}",
                page.rects
            )
        })
}

#[tokio::test]
async fn applies_inline_text_color() {
    let document = Html::from_string("<p style=\"color: red\">Hello</p>")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines[0].color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn draws_backgrounds_and_borders() {
    let html = Html::from_string(
        "<div style=\"margin: 0; padding: 2pt; border: 1pt solid blue; background: #ff0000\">Box</div>",
    );
    let document = html.render_async(&RenderOptions::default()).await.unwrap();
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(255, 0, 0)))
    );
    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
            .count(),
        4
    );

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("1 0 0 rg"));
    assert!(rendered.contains("0 0 1 rg"));
}

#[tokio::test]
async fn hwb_border_color_paints_vector_border() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border:2pt solid hwb(240 20% 0% / 75%)\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border_color = Color::rgba(51, 51, 255, 0.75);
    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(border_color))
            .count(),
        4
    );
}

#[tokio::test]
async fn srgb_color_function_border_color_paints_vector_border() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border:2pt solid color(srgb 0.2 0.2 1 / 75%)\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border_color = Color {
        r: 0.2,
        g: 0.2,
        b: 1.0,
        a: 0.75,
    };
    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(border_color))
            .count(),
        4
    );
}

#[tokio::test]
async fn logical_inline_start_border_paints_left_side_in_initial_writing_mode() {
    let document = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"width:20pt;height:10pt;border-inline-start:2pt solid red\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let border = document.pages[0]
        .rects
        .iter()
        .find(|rect| {
            rect.fill == Some(Color::new(255, 0, 0))
                && (rect.width() - 2.0).abs() < 0.01
                && rect.height() > 9.0
        })
        .unwrap();
    assert!((border.x() - 10.0).abs() < 0.01);
}

#[tokio::test]
async fn logical_inline_start_border_paints_right_side_in_rtl_direction() {
    let document = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"direction:rtl;width:20pt;height:10pt;border-inline-start:2pt solid red\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border = document.pages[0]
        .rects
        .iter()
        .find(|rect| {
            rect.fill == Some(Color::new(255, 0, 0))
                && (rect.width() - 2.0).abs() < 0.01
                && rect.height() > 9.0
        })
        .unwrap();
    assert!((border.x() - 30.0).abs() < 0.01);
}

#[tokio::test]
async fn logical_border_corner_radius_paints_initial_top_left_corner() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;background:black;border-start-start-radius:4pt;color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rounded = document.pages[0]
        .rounded_rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();

    assert_eq!(rounded.radii.top_left.x(), 4.0);
    assert_eq!(rounded.radii.top_left.y(), 4.0);
    assert_eq!(rounded.radii.top_right.x(), 0.0);
    assert_eq!(rounded.radii.bottom_left.y(), 0.0);
}

#[tokio::test]
async fn border_radius_paints_background_as_rounded_rect() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;background:black;border-radius:4pt;color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let rounded = document.pages[0]
        .rounded_rects
        .iter()
        .find(|rect| rect.fill == Some(Color::BLACK))
        .unwrap();

    assert_eq!(rounded.radii.top_left.x(), 4.0);
    assert_eq!(rounded.radii.top_left.y(), 4.0);
    assert_eq!(rounded.radii.top_right.x(), 4.0);
    assert_eq!(rounded.radii.bottom_right.y(), 4.0);
}

#[tokio::test]
async fn corner_shorthand_matches_equivalent_corner_longhands() {
    let shorthand = Html::from_string(
        "<div style=\"margin:0;width:120pt;height:120pt;border:18pt solid rgb(0 128 0);background:rgb(240 240 240);corner:36px round / 18px bevel / 28px scoop / 20px notch\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();
    let longhands = Html::from_string(
        "<div style=\"margin:0;width:120pt;height:120pt;border:18pt solid rgb(0 128 0);background:rgb(240 240 240);border-top-left-radius:36px;border-top-right-radius:18px;border-bottom-right-radius:28px;border-bottom-left-radius:20px;corner-top-left-shape:round;corner-top-right-shape:bevel;corner-bottom-right-shape:scoop;corner-bottom-left-shape:notch\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(shorthand.pages[0].rounded_rects.len(), 0);
    assert_eq!(shorthand.pages[0].paths, longhands.pages[0].paths);
    assert!(
        shorthand.pages[0]
            .paths
            .iter()
            .any(|path| path.fill == Some(Color::new(240, 240, 240)))
    );
    assert!(
        shorthand.pages[0]
            .paths
            .iter()
            .any(|path| path.fill == Some(Color::new(0, 128, 0))
                && path.fill_rule == quire::RenderedPathFillRule::EvenOdd)
    );
}

#[tokio::test]
async fn uniform_solid_rounded_border_paints_as_rounded_stroke() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;background:red;border:2pt solid blue;border-radius:4pt;color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
            .count(),
        0
    );
    let rounded_border = document.pages[0]
        .rounded_rects
        .iter()
        .find(|rect| rect.stroke == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert_eq!(rounded_border.stroke_width, 2.0);
    assert_eq!(rounded_border.radii.top_left.x(), 3.0);

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("2 w"));
    assert!(rendered.contains("0 0 1 RG"));
    assert!(rendered.contains("S"));
}

#[tokio::test]
async fn mixed_width_solid_rounded_border_paints_as_even_odd_path() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;background:white;border-style:solid;border-color:blue;border-width:1pt 3pt 5pt 7pt;border-radius:6pt;color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
            .count(),
        0
    );
    let border_path = document.pages[0]
        .paths
        .iter()
        .find(|path| path.fill == Some(Color::new(0, 0, 255)))
        .unwrap();

    assert_eq!(border_path.fill_rule, quire::RenderedPathFillRule::EvenOdd);
    assert!(border_path.commands.len() >= 10);

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("0 0 1 rg"));
    assert!(rendered.contains("f*"));
}

#[tokio::test]
async fn mixed_color_solid_rounded_border_paints_clipped_side_paths() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:24pt;height:14pt;background:white;border-style:solid;border-width:3pt;border-color:red green blue black;border-radius:7pt;color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border_paths = document.pages[0]
        .paths
        .iter()
        .filter(|path| path.clip.is_some())
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 4);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule == quire::RenderedPathFillRule::EvenOdd)
    );
    assert!(
        border_paths
            .iter()
            .all(|path| path.clip.as_ref().unwrap().commands.len() == 5)
    );
    assert!(
        border_paths
            .iter()
            .any(|path| path.fill == Some(Color::new(255, 0, 0)))
    );
    assert!(
        border_paths
            .iter()
            .any(|path| path.fill == Some(Color::new(0, 128, 0)))
    );
    assert!(
        border_paths
            .iter()
            .any(|path| path.fill == Some(Color::new(0, 0, 255)))
    );
    assert!(
        border_paths
            .iter()
            .any(|path| path.fill == Some(Color::new(0, 0, 0)))
    );

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("W\nn"));
    assert!(rendered.contains("f*"));
}

#[tokio::test]
async fn rounded_inset_border_paints_clipped_shaded_side_paths() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:24pt;height:14pt;background:white;border:4pt inset rgb(120 120 120);border-radius:7pt;color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert!(
        document.pages[0]
            .rects
            .iter()
            .all(|rect| rect.fill != Some(Color::new(120, 120, 120)))
    );
    let border_paths = document.pages[0]
        .paths
        .iter()
        .filter(|path| path.clip.is_some())
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 4);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule == quire::RenderedPathFillRule::EvenOdd)
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(border_dark_gray()))
            .count()
            >= 2
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(border_light_gray()))
            .count()
            >= 2
    );
}

#[tokio::test]
async fn rounded_groove_border_paints_clipped_outer_and_inner_side_paths() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:24pt;height:14pt;background:white;border:6pt groove rgb(120 120 120);border-radius:8pt;color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border_paths = document.pages[0]
        .paths
        .iter()
        .filter(|path| path.clip.is_some())
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 8);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule == quire::RenderedPathFillRule::EvenOdd)
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(border_dark_gray()))
            .count()
            >= 4
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(border_light_gray()))
            .count()
            >= 4
    );
}

#[tokio::test]
async fn uniform_double_rounded_border_paints_as_two_path_rings() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:24pt;height:14pt;background:white;border:6pt double blue;border-radius:8pt;color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
            .count(),
        0
    );
    let border_paths = document.pages[0]
        .paths
        .iter()
        .filter(|path| path.fill == Some(Color::new(0, 0, 255)))
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 2);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule == quire::RenderedPathFillRule::EvenOdd)
    );

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("f*"));
}

#[tokio::test]
async fn mixed_double_rounded_border_paints_clipped_outer_and_inner_side_paths() {
    let document = Html::from_string(
        "<span style=\"display:inline-block;width:28pt;height:18pt;background:white;border-style:double;border-width:3pt 6pt 9pt 12pt;border-color:red green blue black;border-radius:9pt;color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border_paths = document.pages[0]
        .paths
        .iter()
        .filter(|path| path.clip.is_some())
        .collect::<Vec<_>>();

    assert!(border_paths.len() >= 8);
    assert!(
        border_paths
            .iter()
            .all(|path| path.fill_rule == quire::RenderedPathFillRule::EvenOdd)
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(Color::new(255, 0, 0)))
            .count()
            >= 2
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(Color::new(0, 128, 0)))
            .count()
            >= 2
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(Color::new(0, 0, 255)))
            .count()
            >= 2
    );
    assert!(
        border_paths
            .iter()
            .filter(|path| path.fill == Some(Color::new(0, 0, 0)))
            .count()
            >= 2
    );
}

#[tokio::test]
async fn border_none_has_zero_used_width() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border:5pt none red;background:blue\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
            .count(),
        0
    );
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 0, 255)))
    );
}

#[tokio::test]
async fn dashed_borders_render_as_segments() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:40pt;height:10pt;border-top:2pt dashed red\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let red_segments = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_segments.len(), 4);
    assert!((red_segments[0].width() - (40.0 / 7.0)).abs() < 0.001);
    assert_eq!(red_segments[0].height(), 2.0);
    assert!(document.pages[0].strokes.is_empty());
}

#[tokio::test]
async fn dotted_borders_render_as_round_dot_paths() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border-top:2pt dotted blue\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
            .count(),
        0
    );
    let dots = document.pages[0]
        .paths
        .iter()
        .filter(|path| path.fill == Some(Color::new(0, 0, 255)))
        .collect::<Vec<_>>();

    assert_eq!(dots.len(), 6);
    assert!(dots.iter().all(|path| {
        path.fill_rule == quire::RenderedPathFillRule::NonZero
            && path
                .commands
                .iter()
                .filter(|command| matches!(command, quire::RenderedPathCommand::CurveTo { .. }))
                .count()
                == 4
    }));
}

#[tokio::test]
async fn rounded_dotted_borders_clip_dots_to_side_and_border_ring() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:20pt;height:10pt;border-top:2pt dotted blue;border-radius:4pt\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
            .count(),
        0
    );
    let dots = document.pages[0]
        .paths
        .iter()
        .filter(|path| path.fill == Some(Color::new(0, 0, 255)))
        .collect::<Vec<_>>();

    assert_eq!(dots.len(), 6);
    assert!(dots.iter().all(|path| {
        path.clip
            .as_ref()
            .is_some_and(|clip| clip.additional_clips.len() == 1)
    }));
}

#[tokio::test]
async fn rounded_dashed_borders_clip_dashes_to_side_and_border_ring() {
    let document = Html::from_string(
        "<div style=\"margin:0;width:40pt;height:10pt;border-top:2pt dashed red;border-radius:4pt\"></div>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
            .count(),
        0
    );
    let dashes = document.pages[0]
        .paths
        .iter()
        .filter(|path| path.fill == Some(Color::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(dashes.len(), 4);
    assert!(dashes.iter().all(|path| {
        path.commands.len() == 5
            && path.clip.as_ref().is_some_and(|clip| {
                clip.fill_rule == quire::RenderedPathFillRule::NonZero
                    && clip.additional_clips.len() == 1
                    && clip.additional_clips[0].fill_rule == quire::RenderedPathFillRule::EvenOdd
            })
    }));
}

#[tokio::test]
async fn paints_stretched_border_image_slices_from_source_pixels() {
    let dir = std::env::temp_dir().join(format!("reasyprint-border-image-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let image_path = dir.join("border.png");
    let mut image = image::RgbaImage::new(3, 3);
    for y in 0..3 {
        for x in 0..3 {
            image.put_pixel(
                x,
                y,
                image::Rgba([(x * 80) as u8, (y * 80) as u8, ((x + y) * 40) as u8, 255]),
            );
        }
    }
    image.save(&image_path).unwrap();

    let document = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 10pt } body { margin: 0 } div { width: 20pt; height: 12pt; border: 4pt solid red; border-image: url(border.png) 1; }</style><div></div>",
    )
    .with_base_url(&dir)
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        document.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
            .count(),
        0
    );
    let border_images = document.pages[0]
        .images
        .iter()
        .filter(|image| image.source_rect.is_some())
        .collect::<Vec<_>>();

    assert_eq!(border_images.len(), 8);
    assert!(
        border_images
            .iter()
            .all(|image| image.pixel_width == 3 && image.pixel_height == 3)
    );
    assert!(
        border_images
            .iter()
            .all(|image| image.source_rect.unwrap().width() > 0
                && image.source_rect.unwrap().height() > 0)
    );

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(image_xobject_count_with_size(&rendered, 1, 1) >= 1);
}

#[tokio::test]
async fn paints_repeated_border_image_tiles() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-border-image-repeat-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let image_path = dir.join("border.png");
    let mut image = image::RgbaImage::new(3, 3);
    for y in 0..3 {
        for x in 0..3 {
            image.put_pixel(
                x,
                y,
                image::Rgba([(x * 70) as u8, (y * 70) as u8, 180, 255]),
            );
        }
    }
    image.save(&image_path).unwrap();

    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 } div { width: 24pt; height: 8pt; border: 4pt solid red; border-image: url(border.png) 1 repeat; }</style><div></div>",
    )
    .with_base_url(&dir)
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border_images = document.pages[0]
        .images
        .iter()
        .filter(|image| image.source_rect.is_some())
        .collect::<Vec<_>>();
    assert!(border_images.len() > 8, "{border_images:#?}");
    assert!(
        border_images
            .iter()
            .all(|image| image.source_rect.unwrap().width() > 0
                && image.source_rect.unwrap().height() > 0)
    );

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(image_xobject_count_with_size(&rendered, 1, 1) > 1);
}

#[tokio::test]
async fn border_image_width_auto_uses_source_slice_size() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-border-image-auto-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let image_path = dir.join("border.png");
    let mut image = image::RgbaImage::new(6, 6);
    for y in 0..6 {
        for x in 0..6 {
            image.put_pixel(
                x,
                y,
                image::Rgba([(x * 30) as u8, (y * 30) as u8, 220, 255]),
            );
        }
    }
    image.save(&image_path).unwrap();

    let document = Html::from_string(
        "<style>@page { size: 100pt 80pt; margin: 10pt } body { margin: 0 } div { width: 20pt; height: 12pt; border: 4pt solid red; border-image: url(border.png) 2 / auto; }</style><div></div>",
    )
    .with_base_url(&dir)
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border_images = document.pages[0]
        .images
        .iter()
        .filter(|image| image.source_rect.is_some())
        .collect::<Vec<_>>();
    assert_eq!(border_images.len(), 8);
    assert!(
        border_images
            .iter()
            .any(|image| (image.height() - 2.0).abs() < 0.01)
    );
    assert!(
        border_images
            .iter()
            .any(|image| (image.width() - 2.0).abs() < 0.01)
    );
}

#[tokio::test]
async fn border_image_widths_scale_down_before_overlapping() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-border-image-fit-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let image_path = dir.join("border.png");
    let mut image = image::RgbaImage::new(3, 3);
    for y in 0..3 {
        for x in 0..3 {
            image.put_pixel(
                x,
                y,
                image::Rgba([(x * 80) as u8, (y * 80) as u8, 120, 255]),
            );
        }
    }
    image.save(&image_path).unwrap();

    let document = Html::from_string(
        "<style>@page { size: 80pt 80pt; margin: 10pt } body { margin: 0 } div { width: 10pt; height: 10pt; border: 2pt solid red; border-image: url(border.png) 1 / 20pt; }</style><div></div>",
    )
    .with_base_url(&dir)
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let border_images = document.pages[0]
        .images
        .iter()
        .filter(|image| image.source_rect.is_some())
        .collect::<Vec<_>>();
    assert_eq!(border_images.len(), 4);
    assert!(
        border_images
            .iter()
            .all(|image| image.width() <= 7.01 && image.height() <= 7.01)
    );
    assert!(
        border_images
            .iter()
            .any(|image| (image.width() - 7.0).abs() < 0.01)
    );
}

#[tokio::test]
async fn inset_and_groove_borders_use_3d_shading() {
    let inset = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;border:2pt inset rgb(120 120 120);color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    let dark = border_dark_gray();
    let light = border_light_gray();
    assert_eq!(
        inset.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(dark))
            .count(),
        2
    );
    assert_eq!(
        inset.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(light))
            .count(),
        2
    );

    let groove = Html::from_string(
        "<span style=\"display:inline-block;width:20pt;height:10pt;border:2pt groove rgb(120 120 120);color:white\">Box</span>",
    )
    .render_async(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(
        groove.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(dark) || rect.fill == Some(light))
            .count(),
        8
    );
}

fn border_dark_gray() -> Color {
    Color {
        r: 80.0 / 255.0,
        g: 80.0 / 255.0,
        b: 80.0 / 255.0,
        a: 1.0,
    }
}

fn border_light_gray() -> Color {
    Color {
        r: 165.0 / 255.0,
        g: 165.0 / 255.0,
        b: 165.0 / 255.0,
        a: 1.0,
    }
}

#[tokio::test]
async fn renders_horizontal_rules() {
    let document =
        Html::from_string("<hr style=\"margin:0;width:100pt;border:0;border-top:2pt solid red\">")
            .render_async(&RenderOptions::default())
            .await
            .unwrap();

    assert!(document.pages[0].lines.is_empty());
    let red = filled_rect(&document.pages[0], Color::new(255, 0, 0));
    assert_eq!(red.width(), 100.0);
    assert_eq!(red.height(), 2.0);
}

#[tokio::test]
async fn horizontal_rules_use_generic_patterned_border_painting() {
    let dashed =
        Html::from_string("<hr style=\"margin:0;width:40pt;border:0;border-top:2pt dashed red\">")
            .render_async(&RenderOptions::default())
            .await
            .unwrap();

    let red_segments = dashed.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_segments.len(), 4);
    assert!((red_segments[0].width() - (40.0 / 7.0)).abs() < 0.001);
    assert_eq!(red_segments[0].height(), 2.0);

    let dotted =
        Html::from_string("<hr style=\"margin:0;width:20pt;border:0;border-top:2pt dotted blue\">")
            .render_async(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(
        dotted.pages[0]
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::new(0, 0, 255)))
            .count(),
        0
    );
    assert_eq!(
        dotted.pages[0]
            .paths
            .iter()
            .filter(|path| path.fill == Some(Color::new(0, 0, 255)))
            .count(),
        6
    );
}

#[tokio::test]
async fn horizontal_rules_use_generic_per_side_border_painting() {
    let document = Html::from_string(
        "<hr style=\"margin:0;width:20pt;height:10pt;border-style:solid;border-width:1pt 2pt 3pt 4pt;border-color:red green blue black\">",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    for color in [
        Color::new(255, 0, 0),
        Color::new(0, 128, 0),
        Color::new(0, 0, 255),
        Color::new(0, 0, 0),
    ] {
        assert!(
            document.pages[0]
                .rects
                .iter()
                .any(|rect| rect.fill == Some(color)),
            "expected hr side color {color:?} in {:?}",
            document.pages[0].rects
        );
    }
}

#[tokio::test]
async fn hr_size_and_width_presentational_hints_render_with_generic_block_layout() {
    let options = RenderOptions {
        presentational_hints: true,
        ..RenderOptions::default()
    };
    let document = Html::from_string(
        "<style>@page{size:160pt 100pt;margin:10pt}body{margin:0}</style>\
         <hr size=\"8\" width=\"100\" style=\"margin:0;border:0;background:cyan\">",
    )
    .render_async(&options)
    .await
    .unwrap();

    let cyan = filled_rect(&document.pages[0], Color::new(0, 255, 255));
    assert!((cyan.width() - 75.0).abs() < 0.01);
    assert!((cyan.height() - 4.5).abs() < 0.01);
}

#[tokio::test]
async fn hr_color_and_size_presentational_hints_render_solid_red_border() {
    let options = RenderOptions {
        presentational_hints: true,
        ..RenderOptions::default()
    };
    let document = Html::from_string(
        "<style>@page{size:160pt 100pt;margin:10pt}body{margin:0}</style>\
         <hr color=\"red\" size=\"10\" style=\"margin:0;width:20pt\">",
    )
    .render_async(&options)
    .await
    .unwrap();

    let red_borders = document.pages[0]
        .rects
        .iter()
        .filter(|rect| rect.fill == Some(Color::new(255, 0, 0)))
        .collect::<Vec<_>>();
    assert_eq!(red_borders.len(), 4);
    assert!(
        red_borders
            .iter()
            .any(|rect| (rect.height() - 3.75).abs() < 0.01)
    );
}

#[tokio::test]
async fn normal_block_auto_margins_center_fixed_width() {
    let document = Html::from_string(
        "<style>@page{size:120pt 80pt;margin:10pt}body{margin:0}.box{width:20pt;height:10pt;margin-left:auto;margin-right:auto;background:green}</style><div class=\"box\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rect(&document.pages[0], Color::new(0, 128, 0));
    assert!((green.x() - 50.0).abs() < 0.01, "{green:?}");
    assert_eq!(green.width(), 20.0);
}

#[tokio::test]
async fn normal_block_one_sided_auto_margins_absorb_free_space() {
    let right_aligned = Html::from_string(
        "<style>@page{size:120pt 80pt;margin:10pt}body{margin:0}.box{width:20pt;height:10pt;margin-left:auto;margin-right:0;background:green}</style><div class=\"box\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let right_green = filled_rect(&right_aligned.pages[0], Color::new(0, 128, 0));
    assert!((right_green.x() - 90.0).abs() < 0.01, "{right_green:?}");

    let left_aligned = Html::from_string(
        "<style>@page{size:120pt 80pt;margin:10pt}body{margin:0}.box{width:20pt;height:10pt;margin-left:0;margin-right:auto;background:green}</style><div class=\"box\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();
    let left_green = filled_rect(&left_aligned.pages[0], Color::new(0, 128, 0));
    assert!((left_green.x() - 10.0).abs() < 0.01, "{left_green:?}");
}

#[tokio::test]
async fn rtl_overconstrained_fixed_width_blocks_keep_end_side() {
    let document = Html::from_string(
        "<style>@page{size:120pt 80pt;margin:10pt}body{margin:0;direction:rtl}.box{width:80pt;height:10pt;margin-left:15pt;margin-right:20pt;background:green}</style><div class=\"box\"></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let green = filled_rect(&document.pages[0], Color::new(0, 128, 0));
    assert!((green.x() - 25.0).abs() < 0.01, "{green:?}");
}

#[tokio::test]
async fn paginates_simple_text_blocks() {
    let html = Html::from_string(
        "<style>@page { size: 120pt 60pt; margin: 10pt } p { margin: 0; font-size: 10pt; line-height: 10pt }</style><p>one two three four five six seven eight nine ten eleven twelve thirteen fourteen</p>",
    );
    let document = html.render_async(&RenderOptions::default()).await.unwrap();

    assert!(document.pages.len() > 1);
}

#[tokio::test]
async fn extracts_title_metadata() {
    let document = Html::from_string("<title>Example PDF</title><p>Hello</p>")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    assert_eq!(document.metadata.title.as_deref(), Some("Example PDF"));
    assert_eq!(document.pages[0].lines[0].text, "Hello");

    let pdf = document.write_pdf_bytes().unwrap();
    let rendered = String::from_utf8_lossy(&pdf);
    assert!(rendered.contains("/Title (Example PDF)"));
    assert!(rendered.contains(r#"<rdf:li xml:lang="x-default">Example PDF</rdf:li>"#));
}

#[tokio::test]
async fn extracts_author_metadata() {
    let document = Html::from_string("<meta name=\"author\" content=\"Ada Lovelace\"><p>Hello</p>")
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.metadata.author.as_deref(), Some("Ada Lovelace"));
    let pdf = document.write_pdf_bytes().unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("/Author (Ada Lovelace)"));
}

#[tokio::test]
async fn extracts_creator_metadata_from_generator_meta() {
    let document =
        Html::from_string("<meta name=\"generator\" content=\"SNPSuite v3.20.0\"><p>Hello</p>")
            .render_async(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(
        document.metadata.creator.as_deref(),
        Some("SNPSuite v3.20.0")
    );
    let pdf = document.write_pdf_bytes().unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("/Creator (SNPSuite v3.20.0)"));
}

#[tokio::test]
async fn accepts_external_stylesheet_api() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string("p { color: #00ff00 }"))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines[0].color, Color::new(0, 255, 0));
}

#[tokio::test]
async fn external_stylesheets_resolve_imports() {
    let dir = std::env::temp_dir().join(format!("reasyprint-import-style-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let imported_path = dir.join("tokens.css");
    let main_path = dir.join("main.css");
    std::fs::write(&imported_path, "p { color: red }").unwrap();
    std::fs::write(&main_path, "@import url(tokens.css);").unwrap();

    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_file_async(&main_path).await.unwrap())
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].lines[0].color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn resolves_inherited_css_custom_properties() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string(
            ":root { --accent: #00ff00 } p { color: var(--accent, red) }",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines[0].color, Color::new(0, 255, 0));
}

#[tokio::test]
async fn applies_print_media_rules() {
    let document = Html::from_string("<p>Hello</p>")
        .with_stylesheet(Css::from_string(
            "@media print { p { color: red } } @media screen { p { font-family: Courier } }",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines[0].color, Color::new(255, 0, 0));
    assert!(!line_font_contains_any(
        &document,
        &document.pages[0].lines[0],
        &["courier"]
    ));
}

#[tokio::test]
async fn loads_linked_stylesheets_relative_to_html_file() {
    let dir = std::env::temp_dir().join(format!("reasyprint-linked-style-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    std::fs::write(&css_path, "p { color: red }").unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>Hello</p>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].lines[0].color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn loads_root_relative_stylesheets_from_base_url() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-root-linked-style-{}",
        std::process::id()
    ));
    let document_dir = dir.join("css/css-page");
    let root_fonts = dir.join("fonts");
    std::fs::create_dir_all(&document_dir).unwrap();
    std::fs::create_dir_all(&root_fonts).unwrap();
    let html_path = document_dir.join("document.html");
    let css_path = root_fonts.join("ahem.css");
    std::fs::write(&css_path, "p { color: red }").unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"/fonts/ahem.css\"><p>Hello</p>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .with_base_url(&dir)
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].lines[0].color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn loads_root_relative_font_face_urls_from_base_url() {
    let dir =
        std::env::temp_dir().join(format!("reasyprint-root-font-face-{}", std::process::id()));
    let document_dir = dir.join("css/css-page");
    let root_fonts = dir.join("fonts");
    std::fs::create_dir_all(&document_dir).unwrap();
    std::fs::create_dir_all(&root_fonts).unwrap();
    let html_path = document_dir.join("document.html");
    let css_path = root_fonts.join("fonts.css");
    let font_path = root_fonts.join("RootFont.ttf");
    std::fs::copy(
        "weasyprint-samples/invoice/SourceSans3-Regular.ttf",
        &font_path,
    )
    .unwrap();
    std::fs::write(
        &css_path,
        "@font-face { font-family: RootFont; src: url('/fonts/RootFont.ttf') } p { font-family: RootFont; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"/fonts/fonts.css\"><p>Hello</p>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .with_base_url(&dir)
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(line_font_contains_any(
        &document,
        &document.pages[0].lines[0],
        &["source", "sans"]
    ));
}

#[tokio::test]
async fn loads_images_relative_to_html_file() {
    let dir = std::env::temp_dir().join(format!("reasyprint-linked-image-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let image_path = dir.join("dot.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &html_path,
        "<body style=\"margin:0\"><img src=\"dot.png\" width=\"10\" height=\"20\"></body>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(document.pages[0].images[0].width(), 7.5);
    assert_eq!(document.pages[0].images[0].height(), 15.0);
}

#[tokio::test]
async fn loads_root_relative_images_from_base_url() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-root-linked-image-{}",
        std::process::id()
    ));
    let document_dir = dir.join("css/css-page");
    let root_images = dir.join("images");
    std::fs::create_dir_all(&document_dir).unwrap();
    std::fs::create_dir_all(&root_images).unwrap();
    let html_path = document_dir.join("document.html");
    let image_path = root_images.join("dot.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &html_path,
        "<body style=\"margin:0\"><img src=\"/images/dot.png\" width=\"10\" height=\"20\"></body>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .with_base_url(&dir)
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(document.pages[0].images[0].width(), 7.5);
    assert_eq!(document.pages[0].images[0].height(), 15.0);
}

#[tokio::test]
async fn paints_background_images_relative_to_stylesheet_file() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-background-image-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("bg.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "div { display:block; width:20pt; height:10pt; background: no-repeat top left / 100% 100% url(bg.png); }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><body style=\"margin:0\"><div></div></body>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(document.pages[0].images[0].width(), 20.0);
    assert_eq!(document.pages[0].images[0].height(), 10.0);
}

#[tokio::test]
async fn paints_first_page_background_image_from_page_rule() {
    let dir =
        std::env::temp_dir().join(format!("reasyprint-page-background-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("cover.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "@page { size: 40pt 40pt; margin: 0 } @page :first { background: url(cover.png) no-repeat center; background-size: cover; } article { display:block; break-before: page; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>One</p><article>Two</article>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].images.len(), 1);
    assert_eq!(document.pages[0].images[0].width(), 40.0);
    assert_eq!(document.pages[0].images[0].height(), 40.0);
    assert!(document.pages[1].images.is_empty());
}

#[tokio::test]
async fn page_background_origin_selects_page_box_positioning_area() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-page-background-origin-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("cover.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "@page { size: 100pt 80pt; margin: 10pt; border: 5pt solid blue; padding: 7pt; background: url(cover.png) no-repeat top left; background-origin: border-box; }\
         @page padding { background-origin: padding-box; }\
         @page content { background-origin: content-box; }\
         body, p, article { margin: 0; font-size: 10pt; line-height: 10pt; }\
         article { display: block; break-before: page; }\
         .padding { page: padding; }\
         .content { page: content; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>Border</p><article class=\"padding\">Padding</article><article class=\"content\">Content</article>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages.len(), 3);
    let images = document
        .pages
        .iter()
        .map(|page| {
            assert_eq!(page.images.len(), 1);
            &page.images[0]
        })
        .collect::<Vec<_>>();

    assert_eq!(images[0].x(), 10.0);
    assert_eq!(images[0].y(), 69.0);
    assert_eq!(images[1].x(), 15.0);
    assert_eq!(images[1].y(), 64.0);
    assert_eq!(images[2].x(), 22.0);
    assert_eq!(images[2].y(), 57.0);
}

#[tokio::test]
async fn page_background_clip_crops_image_to_page_content_box() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-page-background-clip-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("cover.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "@page { size: 100pt 80pt; margin: 10pt; border: 5pt solid blue; padding: 7pt;\
          background: url(cover.png) no-repeat top left / 80pt 60pt border-box content-box; }\
         body, p { margin: 0; font-size: 10pt; line-height: 10pt; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>Clip</p>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(document.pages[0].images.len(), 1);
    let image = &document.pages[0].images[0];
    assert_eq!(image.x(), 22.0);
    assert_eq!(image.y(), 22.0);
    assert_eq!(image.width(), 56.0);
    assert_eq!(image.height(), 36.0);
    assert!(image.source_rect.is_some());
}

#[tokio::test]
async fn page_background_repeat_y_tiles_from_positioned_image() {
    let dir = std::env::temp_dir().join(format!(
        "reasyprint-page-background-repeat-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let html_path = dir.join("document.html");
    let css_path = dir.join("style.css");
    let image_path = dir.join("tile.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();
    std::fs::write(
        &css_path,
        "@page { size: 30pt 35pt; margin: 0; background-image: url(tile.png); background-size: 10pt 10pt; background-repeat: repeat-y; background-position: top left; }\
         body { margin: 0; font-size: 10pt; line-height: 10pt; }",
    )
    .unwrap();
    std::fs::write(
        &html_path,
        "<link rel=\"stylesheet\" href=\"style.css\"><p>Tile</p>",
    )
    .unwrap();

    let document = Html::from_file_async(&html_path)
        .await
        .unwrap()
        .render_async(&RenderOptions::default())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    let images = &document.pages[0].images;
    assert_eq!(images.len(), 4);
    assert!(images.iter().all(|image| image.x() == 0.0));
    assert_eq!(
        images.iter().map(|image| image.y()).collect::<Vec<_>>(),
        vec![0.0, 5.0, 15.0, 25.0]
    );
    assert_eq!(images[0].height(), 5.0);
    assert!(images[0].source_rect.is_some());
    assert!(
        images[1..]
            .iter()
            .all(|image| image.width() == 10.0 && image.height() == 10.0)
    );
}

#[tokio::test]
async fn page_background_paints_multiple_image_layers_with_independent_geometry() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 40pt 40pt; margin: 0;\
           background-image: url({png}), url({png});\
           background-size: 10pt 10pt, 5pt 5pt;\
           background-repeat: no-repeat, no-repeat;\
           background-position: top left, bottom right;\
         }}\
         body {{ margin: 0; font-size: 10pt; line-height: 10pt }}\
         </style><p>Layers</p>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0]
        .images
        .iter()
        .filter(|image| image.background)
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert!(images.iter().any(|image| image.x() == 0.0
        && image.y() == 30.0
        && image.width() == 10.0
        && image.height() == 10.0));
    assert!(images.iter().any(|image| image.x() == 35.0
        && image.y() == 0.0
        && image.width() == 5.0
        && image.height() == 5.0));
}

#[tokio::test]
async fn normal_box_background_layers_use_independent_origin_and_clip() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 80pt 80pt; margin: 0 }}\
         body {{ margin: 0 }}\
         div {{ display: block; width: 40pt; height: 40pt; border: 5pt solid transparent; padding: 5pt;\
           background-image: url({png}), url({png});\
           background-size: 20pt 20pt, 40pt 40pt;\
           background-position: top left, top left;\
           background-repeat: no-repeat, no-repeat;\
           background-origin: content-box, border-box;\
           background-clip: content-box, padding-box;\
         }}\
         </style><div></div>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0]
        .images
        .iter()
        .filter(|image| image.background)
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert!(images.iter().any(|image| image.x() == 10.0
        && image.y() == 50.0
        && image.width() == 20.0
        && image.height() == 20.0
        && image.source_rect.is_some()));
    assert!(images.iter().any(|image| image.width() > 20.0
        && image.width() < 40.0
        && image.height() > 20.0
        && image.height() < 40.0
        && image.source_rect.is_some()));
}

#[tokio::test]
async fn page_margin_background_layers_use_independent_origin_and_clip() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 80pt 80pt; margin: 20pt;\
           @top-center {{ content: \"\"; width: 40pt; height: 20pt; border: 5pt solid transparent; padding: 5pt;\
             background-image: url({png}), url({png});\
             background-size: 10pt 10pt, 40pt 20pt;\
             background-position: top left, top left;\
             background-repeat: no-repeat, no-repeat;\
             background-origin: content-box, border-box;\
             background-clip: content-box, padding-box;\
           }}\
         }}\
         body {{ margin: 0 }}\
         </style><p>x</p>",
    ))
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    let images = document.pages[0]
        .images
        .iter()
        .filter(|image| image.background)
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert!(images.iter().any(|image| image.width() == 10.0
        && image.height() == 10.0
        && image.source_rect.is_some()));
    assert!(
        images.iter().any(|image| image.width() < 40.0
            && image.height() < 20.0
            && image.source_rect.is_some())
    );
}

#[tokio::test]
async fn background_paints_multiple_linear_gradient_layers() {
    let document = Html::from_string(
        "<style>\
         @page { size: 80pt 80pt; margin: 0 }\
         body { margin: 0 }\
         div { display: block; width: 40pt; height: 40pt;\
           background-image: linear-gradient(to bottom, red 0pt, red 40pt), linear-gradient(to right, blue 0pt, blue 40pt);\
         }\
         </style><div></div>",
    )
    .render_async(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(255, 0, 0)))
    );
    assert!(
        document.pages[0]
            .rects
            .iter()
            .any(|rect| rect.fill == Some(Color::new(0, 0, 255)))
    );
}

#[tokio::test]
async fn supports_class_and_id_selectors() {
    let document = Html::from_string("<p class=\"lead\">Lead</p><p id=\"note\">Note</p>")
        .with_stylesheet(Css::from_string(
            ".lead { color: blue } p#note { color: red }",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines[0].color, Color::new(0, 0, 255));
    assert_eq!(document.pages[0].lines[1].color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn supports_servo_attribute_and_link_selectors() {
    let document =
        Html::from_string("<p data-kind=\"lead\">Lead</p><a href=\"https://example.com\">Link</a>")
            .with_stylesheet(Css::from_string(
                "[data-kind=lead] { color: red } a:link { font-family: monospace }",
            ))
            .render_async(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(document.pages[0].lines[0].color, Color::new(255, 0, 0));
    assert!(line_font_is_monospace(
        &document,
        &document.pages[0].lines[1]
    ));
}

#[tokio::test]
async fn applies_simple_css_specificity() {
    let document = Html::from_string("<p class=\"lead\" id=\"hero\">Hero</p>")
        .with_stylesheet(Css::from_string(
            "#hero { color: red } .lead { color: blue } p { color: green }",
        ))
        .render_async(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines[0].color, Color::new(255, 0, 0));
}

#[tokio::test]
async fn supports_basic_descendant_and_child_selectors() {
    let document =
        Html::from_string("<div class=\"wrapper\"><p>Child</p><div><p>Nested</p></div></div>")
            .with_stylesheet(Css::from_string(
                "div.wrapper p { font-family: monospace } div.wrapper > p { color: red }",
            ))
            .render_async(&RenderOptions::default())
            .await
            .unwrap();

    assert_eq!(document.pages[0].lines[0].text, "Child");
    assert!(line_font_is_monospace(
        &document,
        &document.pages[0].lines[0]
    ));
    assert_eq!(document.pages[0].lines[0].color, Color::new(255, 0, 0));
    assert_eq!(document.pages[0].lines[1].text, "Nested");
    assert!(line_font_is_monospace(
        &document,
        &document.pages[0].lines[1]
    ));
    assert_eq!(document.pages[0].lines[1].color, Color::BLACK);
}
